package mcb.env;

import it.unimi.dsi.fastutil.ints.Int2ObjectMaps;
import java.util.ArrayDeque;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.network.HashedStack;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.ClientboundBlockUpdatePacket;
import net.minecraft.network.protocol.game.ClientboundChunkBatchFinishedPacket;
import net.minecraft.network.protocol.game.ServerboundChunkBatchReceivedPacket;
import net.minecraft.network.protocol.game.ClientboundContainerClosePacket;
import net.minecraft.network.protocol.game.ClientboundOpenScreenPacket;
import net.minecraft.network.protocol.game.ClientboundPlayerPositionPacket;
import net.minecraft.network.protocol.game.ClientboundSectionBlocksUpdatePacket;
import net.minecraft.network.protocol.game.ServerboundAcceptTeleportationPacket;
import net.minecraft.network.protocol.game.ServerboundContainerClickPacket;
import net.minecraft.network.protocol.game.ServerboundContainerClosePacket;
import net.minecraft.network.protocol.game.ServerboundPlayerActionPacket;
import net.minecraft.network.protocol.game.ServerboundPlayerLoadedPacket;
import net.minecraft.network.protocol.game.ServerboundSetCarriedItemPacket;
import net.minecraft.network.protocol.game.ServerboundSwingPacket;
import net.minecraft.network.protocol.game.ServerboundUseItemOnPacket;
import net.minecraft.server.network.ServerGamePacketListenerImpl;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.entity.player.Input;
import net.minecraft.world.inventory.AbstractContainerMenu;
import net.minecraft.world.inventory.ContainerInput;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.HitResult;
import net.minecraft.world.phys.Vec3;

/**
 * The bot's game client. This is the only way the bot may observe or affect the world, and it offers exactly what
 * a person at a keyboard and mouse has: turn the view, hold movement keys, left/right click, pick a hotbar slot,
 * click slots in an open inventory/chest screen. One call to {@link #tick()} is one client tick followed by one
 * server tick.
 *
 * Fidelity notes. Movement is computed by the vanilla player physics from the key states (see {@link BotPlayer}).
 * Clicks follow the vanilla client's Minecraft.tick/handleKeybinds/startUseItem/startAttack/continueAttack and
 * MultiPlayerGameMode block-breaking logic, and reach the server through the server's real packet handlers
 * (ServerGamePacketListenerImpl.handleUseItemOn, handlePlayerAction, handleContainerClick ...), so every server-side
 * check (reach, placement rules, break timing, container validity) applies. What the crosshair points at is found
 * with the same ray cast the client uses (Entity.pick over block outline shapes, block interaction range).
 */
public final class Client {
    private final Env env;
    public final BotPlayer player;
    private final FakeConnection conn;
    private ServerGamePacketListenerImpl net;

    // ---- inputs for the next tick ----
    private float yaw, pitch;
    private boolean kForward, kBack, kLeft, kRight, kJump, kSneak, kSprint;
    private boolean useHeld, attackHeld;
    private boolean useClick, attackClick;
    private int wantSlot = -1;
    private final ArrayDeque<int[]> screenClicks = new ArrayDeque<>();
    private boolean wantCloseScreen, wantOpenInventory;

    // ---- client-side state (mirrors Minecraft / MultiPlayerGameMode fields) ----
    private int rightClickDelay, missTime;
    private boolean isDestroying;
    private BlockPos destroyPos = new BlockPos(-1, -1, -1);
    private ItemStack destroyingItem = ItemStack.EMPTY;
    private float destroyProgress;
    private int destroyDelay;
    private int sequence;
    private boolean inventoryScreen;      // own inventory screen (client-side only)
    private int openContainerId = -1;     // server-opened container screen
    private HitResult hit;
    public long ticks;

    // ---- observations ----
    /** Block changes the server told this client about since the last drain: {x, y, z, stateId}. */
    private int[] changes = new int[256];
    private int nChanges;
    public long teleports;

    Client(Env env, BotPlayer player, FakeConnection conn) {
        this.env = env;
        this.player = player;
        this.conn = conn;
        conn.sink = this::onPacket;
    }

    void attach() {
        net = player.connection;
        net.handleAcceptPlayerLoad(new ServerboundPlayerLoadedPacket());
        yaw = player.getYRot();
        pitch = player.getXRot();
    }

    private final ArrayDeque<Integer> pendingTeleports = new ArrayDeque<>();

    public final java.util.TreeMap<String, Integer> packetStats = new java.util.TreeMap<>();
    public boolean collectPacketStats;

    private void onPacket(Packet<?> p) {
        if (collectPacketStats) packetStats.merge(p.getClass().getSimpleName(), 1, Integer::sum);
        if (p instanceof ClientboundBlockUpdatePacket b) {
            addChange(b.getPos(), b.getBlockState());
        } else if (p instanceof ClientboundSectionBlocksUpdatePacket s) {
            s.runUpdates(this::addChange);
        } else if (p instanceof ClientboundPlayerPositionPacket t) {
            pendingTeleports.add(t.id());
        } else if (p instanceof ClientboundChunkBatchFinishedPacket) {
            pendingBatchAcks++;
        } else if (p instanceof ClientboundOpenScreenPacket o) {
            openContainerId = o.getContainerId();
        } else if (p instanceof ClientboundContainerClosePacket) {
            openContainerId = -1;
        }
    }

    private void addChange(BlockPos pos, BlockState state) {
        if (nChanges * 4 + 4 > changes.length) changes = java.util.Arrays.copyOf(changes, changes.length * 2);
        int i = nChanges++ * 4;
        changes[i] = pos.getX(); changes[i + 1] = pos.getY(); changes[i + 2] = pos.getZ(); changes[i + 3] = Block.getId(state);
    }

    private int pendingBatchAcks;

    /**
     * Protocol housekeeping a real client does on its own: confirm teleports, and acknowledge chunk batches (the
     * server only reports block changes in chunks it has finished sending, and only sends more chunks once the
     * previous batch is acknowledged).
     */
    private void acceptTeleports() {
        while (pendingBatchAcks > 0) {
            pendingBatchAcks--;
            net.handleChunkBatchReceived(new ServerboundChunkBatchReceivedPacket(25.0f));
        }
        while (!pendingTeleports.isEmpty()) {
            net.handleAcceptTeleportPacket(new ServerboundAcceptTeleportationPacket(pendingTeleports.poll()));
            teleports++;
        }
        if (nChanges < 0) throw new IllegalStateException();
    }

    /** Administrator action (site setup only, never used by the bot): put the player somewhere. */
    public void adminTeleport(double x, double y, double z) {
        net.teleport(x, y, z, yaw, pitch);
        acceptTeleports();
        player.setDeltaMovement(Vec3.ZERO);
        env.level.getChunkSource().move(player);
    }

    // ------------------------------------------------------------------ observation
    public double x() { return player.getX(); }
    public double y() { return player.getY(); }
    public double z() { return player.getZ(); }
    public Vec3 velocity() { return player.getDeltaMovement(); }
    public boolean onGround() { return player.onGround(); }
    public boolean horizontalCollision() { return player.horizontalCollision; }
    public float yaw() { return yaw; }
    public float pitch() { return pitch; }
    public Vec3 eye() { return player.getEyePosition(); }
    public double reach() { return player.blockInteractionRange(); }
    public boolean sneaking() { return player.isShiftKeyDown(); }

    /** What the client knows of the world (loaded chunks around the player). */
    public BlockState block(BlockPos pos) { return env.level.getBlockState(pos); }
    public net.minecraft.world.level.Level level() { return env.level; }

    public int changeCount() { return nChanges; }
    /** Returns the backing array of {x,y,z,stateId} quadruples; valid entries: changeCount(). Call clearChanges() after. */
    public int[] changes() { return changes; }
    public void clearChanges() { nChanges = 0; }

    public ItemStack inventorySlot(int i) { return player.getInventory().getItem(i); } // 0-8 hotbar, 9-35 main
    public int selectedSlot() { return player.getInventory().getSelectedSlot(); }
    public boolean screenOpen() { return inventoryScreen || openContainerId >= 0; }
    public boolean containerOpen() { return openContainerId >= 0; }
    public AbstractContainerMenu menu() { return player.containerMenu; }
    /** The block face under the crosshair as of the last tick's pick (null if none within reach). */
    public BlockHitResult lastHit() { return hit instanceof BlockHitResult b && b.getType() == HitResult.Type.BLOCK ? b : null; }
    public boolean isBreaking() { return isDestroying; }

    // ------------------------------------------------------------------ inputs
    public void look(float yaw, float pitch) {
        this.yaw = yaw;
        this.pitch = Math.max(-90.0f, Math.min(90.0f, pitch));
    }

    public void keys(boolean forward, boolean back, boolean left, boolean right, boolean jump, boolean sneak, boolean sprint) {
        kForward = forward; kBack = back; kLeft = left; kRight = right; kJump = jump; kSneak = sneak; kSprint = sprint;
    }

    public void releaseKeys() { keys(false, false, false, false, false, false, false); }
    public void sneak(boolean s) { kSneak = s; }
    public void jump(boolean j) { kJump = j; }

    /** Number key 1-9. */
    public void selectSlot(int slot) { wantSlot = slot; }
    /** One press of the use button (right click) during the next tick. */
    public void clickUse() { useClick = true; }
    /** One press of the attack button (left click) during the next tick. */
    public void clickAttack() { attackClick = true; }
    public void holdAttack(boolean held) { attackHeld = held; }
    public void holdUse(boolean held) { useHeld = held; }

    /** 'E': open the own-inventory screen. While any screen is open, movement keys and world clicks are ignored. */
    public void openInventory() { wantOpenInventory = true; }
    /** Esc. */
    public void closeScreen() { wantCloseScreen = true; }
    /** A mouse click on a slot of the open screen (processed one per tick). */
    public void clickSlot(int slot, int button, ContainerInput type) { screenClicks.add(new int[] {slot, button, type.ordinal()}); }
    public int queuedSlotClicks() { return screenClicks.size(); }

    // ------------------------------------------------------------------ the tick
    public void tick() {
        ticks++;
        acceptTeleports();
        // view direction: the mouse turns the player continuously; by tick time the new angles are in effect
        player.setYRot(yaw);
        player.setXRot(pitch);
        player.setYHeadRot(yaw);

        // --- Minecraft.tick ---
        if (rightClickDelay > 0) rightClickDelay--;
        // MultiPlayerGameMode.tick -> ensureHasSentCarriedItem
        if (wantSlot >= 0 && wantSlot != player.getInventory().getSelectedSlot() && !screenOpen()) {
            net.handleSetCarriedItem(new ServerboundSetCarriedItemPacket(wantSlot));
        }
        wantSlot = -1;
        if (!screenOpen()) {
            hit = player.pick(player.blockInteractionRange(), 1.0f, false);
            handleKeybinds();
            if (missTime > 0) missTime--;
        } else {
            hit = null;
            missTime = 10000;
            handleScreen();
        }
        useClick = false;
        attackClick = false;

        // --- LocalPlayer tick: movement from key states (no keys reach the player while a screen is open) ---
        player.pendingKeys = screenOpen() ? Input.EMPTY : new Input(kForward, kBack, kLeft, kRight, kJump, kSneak, kSprint);
        player.doTick();
        // On a real connection the client now reports its new position and the server's move handler updates
        // which chunks are kept loaded around the player; do that part here.
        if (!env.pinned()) env.level.getChunkSource().move(player);

        // --- the server's tick ---
        env.tick(player);
        acceptTeleports();
    }

    private void handleKeybinds() {
        if (wantOpenInventory) {
            wantOpenInventory = false;
            inventoryScreen = true;
            return;
        }
        wantCloseScreen = false;
        boolean attackedInstantly = false;
        if (attackClick) attackedInstantly = startAttack();
        if (useClick) startUseItem();
        if (useHeld && rightClickDelay == 0 && !player.isUsingItem()) startUseItem();
        continueAttack(!attackedInstantly && attackHeld);
    }

    private void handleScreen() {
        env.noteAction();
        wantOpenInventory = false;
        if (wantCloseScreen) {
            wantCloseScreen = false;
            screenClicks.clear();
            if (openContainerId >= 0) net.handleContainerClose(new ServerboundContainerClosePacket(openContainerId));
            else net.handleContainerClose(new ServerboundContainerClosePacket(player.inventoryMenu.containerId));
            openContainerId = -1;
            inventoryScreen = false;
            return;
        }
        int[] c = screenClicks.poll();
        if (c != null) {
            AbstractContainerMenu menu = player.containerMenu;
            net.handleContainerClick(new ServerboundContainerClickPacket(menu.containerId, menu.getStateId(), (short) c[0], (byte) c[1],
                ContainerInput.values()[c[2]], Int2ObjectMaps.emptyMap(), HashedStack.EMPTY));
        }
    }

    // ---- Minecraft.startUseItem (main hand; the bot keeps its off hand empty) ----
    private void startUseItem() {
        if (isDestroying) return;
        rightClickDelay = 4;
        if (hit instanceof BlockHitResult b && b.getType() == HitResult.Type.BLOCK) {
            env.noteAction();
            net.handleUseItemOn(new ServerboundUseItemOnPacket(InteractionHand.MAIN_HAND, b, ++sequence));
            net.handleAnimate(new ServerboundSwingPacket(InteractionHand.MAIN_HAND));
        }
    }

    // ---- Minecraft.startAttack, block case ----
    private boolean startAttack() {
        if (missTime > 0) return false;
        boolean instant = false;
        if (hit instanceof BlockHitResult b && b.getType() == HitResult.Type.BLOCK && !env.level.getBlockState(b.getBlockPos()).isAir()) {
            startDestroyBlock(b.getBlockPos(), b.getDirection());
            if (env.level.getBlockState(b.getBlockPos()).isAir()) instant = true;
        } else {
            missTime = 10;
        }
        net.handleAnimate(new ServerboundSwingPacket(InteractionHand.MAIN_HAND));
        return instant;
    }

    // ---- Minecraft.continueAttack ----
    private void continueAttack(boolean down) {
        if (!down) missTime = 0;
        if (missTime > 0 || player.isUsingItem()) return;
        if (down && hit instanceof BlockHitResult b && b.getType() == HitResult.Type.BLOCK) {
            if (!env.level.getBlockState(b.getBlockPos()).isAir()) {
                if (continueDestroyBlock(b.getBlockPos(), b.getDirection())) net.handleAnimate(new ServerboundSwingPacket(InteractionHand.MAIN_HAND));
            }
            return;
        }
        stopDestroyBlock();
    }

    // ---- MultiPlayerGameMode (survival branch) ----
    private boolean sameDestroyTarget(BlockPos pos) {
        return pos.equals(destroyPos) && ItemStack.isSameItemSameComponents(player.getMainHandItem(), destroyingItem);
    }

    private boolean startDestroyBlock(BlockPos pos, Direction face) {
        env.noteAction();
        if (player.blockActionRestricted(env.level, pos, player.gameMode.getGameModeForPlayer())) return false;
        if (!isDestroying || !sameDestroyTarget(pos)) {
            if (isDestroying) net.handlePlayerAction(new ServerboundPlayerActionPacket(ServerboundPlayerActionPacket.Action.ABORT_DESTROY_BLOCK, destroyPos, face));
            BlockState state = env.level.getBlockState(pos);
            boolean instant = !state.isAir() && state.getDestroyProgress(player, env.level, pos) >= 1.0f;
            if (!instant) {
                isDestroying = true;
                destroyPos = pos.immutable();
                destroyingItem = player.getMainHandItem().copy();
                destroyProgress = 0.0f;
            }
            // the server destroys the block itself when it computes the same instant-break condition
            net.handlePlayerAction(new ServerboundPlayerActionPacket(ServerboundPlayerActionPacket.Action.START_DESTROY_BLOCK, pos, face, ++sequence));
        }
        return true;
    }

    private void stopDestroyBlock() {
        if (isDestroying) {
            net.handlePlayerAction(new ServerboundPlayerActionPacket(ServerboundPlayerActionPacket.Action.ABORT_DESTROY_BLOCK, destroyPos, Direction.DOWN));
            isDestroying = false;
            destroyProgress = 0.0f;
        }
    }

    private boolean continueDestroyBlock(BlockPos pos, Direction face) {
        if (destroyDelay > 0) {
            destroyDelay--;
            return true;
        }
        if (sameDestroyTarget(pos)) {
            BlockState state = env.level.getBlockState(pos);
            if (state.isAir()) {
                isDestroying = false;
                return false;
            }
            destroyProgress += state.getDestroyProgress(player, env.level, pos);
            if (destroyProgress >= 1.0f) {
                env.noteAction();
                isDestroying = false;
                net.handlePlayerAction(new ServerboundPlayerActionPacket(ServerboundPlayerActionPacket.Action.STOP_DESTROY_BLOCK, pos, face, ++sequence));
                destroyProgress = 0.0f;
                destroyDelay = 5;
            }
            return true;
        }
        return startDestroyBlock(pos, face);
    }
}
