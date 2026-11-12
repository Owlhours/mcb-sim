package mcb.env;

import com.mojang.authlib.GameProfile;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.network.protocol.game.ServerboundPlayerInputPacket;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ClientInformation;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.Pose;
import net.minecraft.world.entity.ai.attributes.Attributes;
import net.minecraft.world.entity.player.Input;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec2;
import net.minecraft.world.phys.Vec3;

/**
 * The bot's body: an ordinary server-side player entity whose movement is simulated by the unmodified vanilla
 * physics ({@code LivingEntity.aiStep/travel/move}), driven by key states exactly the way the vanilla client's
 * {@code LocalPlayer} + {@code KeyboardInput} drive it. The overrides below are transcriptions of the 26.2 client
 * logic (LocalPlayer.aiStep prefix, applyInput, modifyInput, sprint/crouch state machine); nothing here adds an
 * ability the client does not have.
 */
public final class BotPlayer extends ServerPlayer {
    /** Keys held during the tick being simulated (set by {@link Client} before each tick). */
    Input pendingKeys = Input.EMPTY;
    /** KeyboardInput state: keys as of the last input.tick(), and the normalised move vector. */
    private Input keyPresses = Input.EMPTY;
    private Vec2 moveVector = Vec2.ZERO;
    private Input lastSentInput = Input.EMPTY;
    private boolean crouching;
    private int sprintTriggerTime;
    private static final int SPRINT_WINDOW = 7; // client option default

    public BotPlayer(MinecraftServer server, ServerLevel level, GameProfile profile, int viewDistance) {
        super(server, level, profile, clientInfo(viewDistance));
    }

    /** Default client settings, except for the render distance. */
    private static ClientInformation clientInfo(int viewDistance) {
        ClientInformation d = ClientInformation.createDefault();
        return new ClientInformation(d.language(), viewDistance, d.chatVisibility(), d.chatColors(), d.modelCustomisation(), d.mainHand(), d.textFilteringEnabled(), d.allowsListing(), d.particleStatus());
    }

    private static float impulse(boolean pos, boolean neg) { return pos == neg ? 0.0f : (pos ? 1.0f : -1.0f); }

    private boolean hasForwardImpulse() { return moveVector.y > 1.0E-5f; }

    /** LocalPlayer.isMovingSlowly */
    public boolean isMovingSlowlyClient() { return crouching || isVisuallyCrawling(); }

    private boolean isSprintingPossible() {
        // LocalPlayer.isSprintingPossible(flying=false), not a passenger
        return !isMobilityRestricted() && hasEnoughFoodToDoExhaustiveManoeuvres() && !isInShallowWater();
    }

    private boolean canStartSprinting() {
        return !isSprinting() && hasForwardImpulse() && isSprintingPossible() && !isUsingItem()
            && (!isFallFlying() || isUnderWater()) && (!isMovingSlowlyClient() || isUnderWater());
    }

    private boolean shouldStopRunSprinting() {
        return !isSprintingPossible() || !hasForwardImpulse() || (horizontalCollision && !minorHorizontalCollision);
    }

    @Override
    public void aiStep() {
        // ---- LocalPlayer.aiStep, up to super.aiStep() ----
        if (sprintTriggerTime > 0) sprintTriggerTime--;
        boolean wasShift = keyPresses.shift();
        boolean hadForward = hasForwardImpulse();
        crouching = !getAbilities().flying && !isSwimming() && !isPassenger()
            && canPlayerFitWithinBlocksAndEntitiesWhen(Pose.CROUCHING)
            && (isShiftKeyDown() || (!isSleeping() && !canPlayerFitWithinBlocksAndEntitiesWhen(Pose.STANDING)));
        // KeyboardInput.tick()
        keyPresses = pendingKeys;
        moveVector = new Vec2(impulse(keyPresses.left(), keyPresses.right()), impulse(keyPresses.forward(), keyPresses.backward())).normalized();
        // the client reports its key state to the server (ServerboundPlayerInputPacket); the handler sets the
        // shift flag that both sides read through isShiftKeyDown()
        if (!lastSentInput.equals(keyPresses)) {
            connection.handlePlayerInput(new ServerboundPlayerInputPacket(keyPresses));
            lastSentInput = keyPresses;
        }
        if (!noPhysics) {
            double w = getBbWidth() * 0.35;
            moveTowardsClosestSpace(getX() - w, getZ() + w);
            moveTowardsClosestSpace(getX() - w, getZ() - w);
            moveTowardsClosestSpace(getX() + w, getZ() - w);
            moveTowardsClosestSpace(getX() + w, getZ() + w);
        }
        if (wasShift || isUsingItem() || keyPresses.backward()) sprintTriggerTime = 0;
        if (canStartSprinting()) {
            if (!hadForward) {
                if (sprintTriggerTime > 0) setSprinting(true); else sprintTriggerTime = SPRINT_WINDOW;
            }
            if (keyPresses.sprint()) setSprinting(true);
        }
        if (isSprinting() && shouldStopRunSprinting()) setSprinting(false);
        super.aiStep();
    }

    /** LocalPlayer.applyInput + modifyInput (replaces the server-side "xxa *= 0.98" of LivingEntity.applyInput). */
    @Override
    protected void applyInput() {
        Vec2 v = modifyInput(moveVector);
        this.xxa = v.x;
        this.zza = v.y;
        this.jumping = keyPresses.jump();
    }

    private Vec2 modifyInput(Vec2 in) {
        if (in.lengthSquared() == 0.0f) return in;
        Vec2 v = in.scale(0.98f);
        if (isMovingSlowlyClient()) v = v.scale((float) getAttributeValue(Attributes.SNEAKING_SPEED));
        float len = v.length();
        if (len <= 0.0f) return v;
        Vec2 dir = v.scale(1.0f / len);
        float ax = Math.abs(dir.x), ay = Math.abs(dir.y);
        float r = ay > ax ? ax / ay : ay / ax;
        float dist = (float) Math.sqrt(1.0f + r * r); // Mth.sqrt(float) == (float)Math.sqrt
        return dir.scale(Math.min(len * dist, 1.0f));
    }

    private void moveTowardsClosestSpace(double x, double z) {
        BlockPos pos = BlockPos.containing(x, getY(), z);
        if (!suffocatesAt(pos)) return;
        double fx = x - pos.getX(), fz = z - pos.getZ();
        Direction best = null;
        double bestD = Double.MAX_VALUE;
        for (Direction d : new Direction[] {Direction.WEST, Direction.EAST, Direction.NORTH, Direction.SOUTH}) {
            double a = d.getAxis().choose(fx, 0.0, fz);
            double dist = d.getAxisDirection() == Direction.AxisDirection.POSITIVE ? 1.0 - a : a;
            if (dist < bestD && !suffocatesAt(pos.relative(d))) { bestD = dist; best = d; }
        }
        if (best != null) {
            Vec3 v = getDeltaMovement();
            if (best.getAxis() == Direction.Axis.X) setDeltaMovement(0.1 * best.getStepX(), v.y, v.z);
            else setDeltaMovement(v.x, v.y, 0.1 * best.getStepZ());
        }
    }

    private boolean suffocatesAt(BlockPos pos) {
        AABB bb = getBoundingBox();
        AABB box = new AABB(pos.getX(), bb.minY, pos.getZ(), pos.getX() + 1.0, bb.maxY, pos.getZ() + 1.0).deflate(1.0E-7);
        return level().collidesWithSuffocatingBlock(this, box);
    }
}
