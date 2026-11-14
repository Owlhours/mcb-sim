package mcb.env;

import com.mojang.authlib.GameProfile;
import java.lang.reflect.Method;
import java.util.HashMap;
import java.util.Map;
import java.util.UUID;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.item.BlockItem;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.context.BlockPlaceContext;
import net.minecraft.world.level.ClipContext;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.HitResult;
import net.minecraft.world.phys.Vec3;

/**
 * Knowledge of the game's rules, as a player has it: "if I click there, holding this, looking that way, what
 * appears?". It answers by running the game's own placement code without placing anything
 * (BlockItem.updatePlacementContext / getPlacementState, the two steps BlockItem.place takes before it writes the
 * block). It never changes the world the bot builds in and gives the bot no way to act.
 *
 * Two services: {@link #predict} for a concrete click in the real surroundings, and {@link #table} - what each
 * block item turns into for every combination of clicked face, part of the face, view direction and sneaking,
 * measured once against a lone support block in a scratch corner of the world.
 */
public final class Oracle {
    public static final int NYAW = 32, NPITCH = 9;
    public static final int TABLE_SIZE = 6 * 4 * NYAW * NPITCH * 2;

    private final Env env;
    private final BotPlayer probe;
    private final Method getPlacementState;
    private final Map<Item, char[]> tables = new HashMap<>();
    private final BlockPos scratch;
    public long predictions;

    public Oracle(Env env) {
        this.env = env;
        this.probe = new BotPlayer(env.server, env.level, new GameProfile(UUID.nameUUIDFromBytes("mcb:probe".getBytes()), "Probe"), 3);
        try {
            getPlacementState = BlockItem.class.getDeclaredMethod("getPlacementState", BlockPlaceContext.class);
            getPlacementState.setAccessible(true);
        } catch (NoSuchMethodException e) { throw new RuntimeException(e); }
        scratch = new BlockPos(-3000, 150, -3000);
    }

    public static int yawClass(float yaw) {
        float y = yaw % 360f;
        if (y < -180f) y += 360f; else if (y >= 180f) y -= 360f;
        int k = (int) Math.floor((y + 180f) / 11.25f);
        return Math.max(0, Math.min(NYAW - 1, k));
    }
    public static float yawCentre(int k) { return -180f + 5.625f + 11.25f * k; }
    public static int pitchClass(float pitch) { return Math.max(0, Math.min(NPITCH - 1, Math.round((pitch + 80f) / 20f))); }
    public static float pitchCentre(int k) { return -80f + 20f * k; }
    public static int index(int face, int quad, int yawClass, int pitchClass, boolean sneak) {
        return (((face * 4 + quad) * NYAW + yawClass) * NPITCH + pitchClass) * 2 + (sneak ? 1 : 0);
    }

    /** What the crosshair of a player with the given eye position and view direction rests on. */
    public BlockHitResult pick(Client c, float yaw, float pitch) {
        Vec3 eye = c.eye();
        Vec3 view = c.player.calculateViewVector(pitch, yaw);
        double r = c.reach();
        BlockHitResult hit = env.level.clip(new ClipContext(eye, eye.add(view.x * r, view.y * r, view.z * r), ClipContext.Block.OUTLINE, ClipContext.Fluid.NONE, c.player));
        return hit.getType() == HitResult.Type.BLOCK ? hit : null;
    }

    public final BlockPos.MutableBlockPos placedAt = new BlockPos.MutableBlockPos();

    /**
     * The state that would appear, and where ({@link #placedAt}), if the player used the item now on the given hit
     * while looking along (yaw, pitch). 0 if nothing would be placed.
     */
    public int predict(Item item, BlockHitResult hit, float yaw, float pitch, boolean sneak) {
        predictions++;
        if (!(item instanceof BlockItem bi)) return 0;
        probe.setYRot(yaw);
        probe.setYHeadRot(yaw);
        probe.setXRot(pitch);
        probe.setShiftKeyDown(sneak);
        try {
            BlockPlaceContext ctx = new BlockPlaceContext(probe, InteractionHand.MAIN_HAND, new ItemStack(item), hit);
            if (!ctx.canPlace()) return 0;
            BlockPlaceContext ctx2 = bi.updatePlacementContext(ctx);
            if (ctx2 == null) return 0;
            BlockState st = (BlockState) getPlacementState.invoke(bi, ctx2);
            if (st == null) return 0;
            placedAt.set(ctx2.getClickedPos());
            return Block.getId(st);
        } catch (ReflectiveOperationException | RuntimeException e) {
            return 0;
        }
    }

    private byte[] floats;

    /**
     * Would the block at one position still stand if the block at another (next to it) were taken away? Asked of the
     * game's own canSurvive, against a view of the world in which that one position is air (nothing is changed).
     */
    public boolean survivesWithout(BlockPos at, BlockPos gone) {
        BlockState st = env.level.getBlockState(at);
        java.lang.reflect.InvocationHandler h = (proxy, m, args) -> {
            if (args != null && args.length == 1 && gone.equals(args[0])) {
                if (m.getName().equals("getBlockState")) return Blocks.AIR.defaultBlockState();
                if (m.getName().equals("getFluidState")) return net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState();
            }
            try { return m.isDefault() ? java.lang.reflect.InvocationHandler.invokeDefault(proxy, m, args) : m.invoke(env.level, args); }
            catch (java.lang.reflect.InvocationTargetException e) { throw e.getCause(); }
        };
        net.minecraft.world.level.LevelReader view = (net.minecraft.world.level.LevelReader) java.lang.reflect.Proxy.newProxyInstance(
            net.minecraft.world.level.LevelReader.class.getClassLoader(), new Class<?>[] {net.minecraft.world.level.LevelReader.class}, h);
        try { return st.canSurvive(view, at); } catch (RuntimeException e) { return true; }
    }

    /** Can this block state exist with nothing around it (true), or does it need something to sit on / hang from (false)? */
    public boolean floats(int stateId) {
        if (floats == null) floats = new byte[Block.BLOCK_STATE_REGISTRY.size()];
        if (floats[stateId] == 0) {
            for (int dx = -1; dx <= 1; dx++) for (int dy = -1; dy <= 1; dy++) for (int dz = -1; dz <= 1; dz++) {
                BlockPos p = scratch.offset(dx, dy, dz);
                if (!env.level.getBlockState(p).isAir()) env.level.setBlock(p, Blocks.AIR.defaultBlockState(), 2 | 16);
            }
            boolean ok;
            try { ok = Block.stateById(stateId).canSurvive(env.level, scratch); } catch (RuntimeException e) { ok = true; }
            floats[stateId] = (byte) (ok ? 1 : 2);
        }
        return floats[stateId] == 1;
    }

    private static final Block[] BASES = {Blocks.STONE, Blocks.DIRT, Blocks.FARMLAND, Blocks.SAND, Blocks.SOUL_SAND, Blocks.END_STONE, Blocks.JUNGLE_LOG};

    /** See class comment. Index with {@link #index}. Entries are state ids, 0 = that click places nothing. */
    public char[] table(Item item) {
        char[] t = tables.get(item);
        if (t != null) return t;
        t = new char[TABLE_SIZE];
        if (item instanceof BlockItem) {
            for (Block base : BASES) {
                // a lone support block with nothing around it
                for (int dx = -2; dx <= 2; dx++) for (int dy = -2; dy <= 2; dy++) for (int dz = -2; dz <= 2; dz++) {
                    BlockPos p = scratch.offset(dx, dy, dz);
                    if (!env.level.getBlockState(p).isAir()) env.level.setBlock(p, Blocks.AIR.defaultBlockState(), 2 | 16);
                }
                env.level.setBlock(scratch, base.defaultBlockState(), 2 | 16);
                boolean any = false;
                for (int face = 0; face < 6; face++) {
                    Direction dir = Direction.from3DDataValue(face);
                    for (int quad = 0; quad < 4; quad++) {
                        double a = (quad & 1) != 0 ? 0.75 : 0.25, b = (quad & 2) != 0 ? 0.75 : 0.25;
                        Vec3 loc = switch (face) {
                            case 0 -> new Vec3(scratch.getX() + a, scratch.getY(), scratch.getZ() + b);
                            case 1 -> new Vec3(scratch.getX() + a, scratch.getY() + 1, scratch.getZ() + b);
                            case 2 -> new Vec3(scratch.getX() + a, scratch.getY() + b, scratch.getZ());
                            case 3 -> new Vec3(scratch.getX() + a, scratch.getY() + b, scratch.getZ() + 1);
                            case 4 -> new Vec3(scratch.getX(), scratch.getY() + b, scratch.getZ() + a);
                            default -> new Vec3(scratch.getX() + 1, scratch.getY() + b, scratch.getZ() + a);
                        };
                        BlockHitResult hit = new BlockHitResult(loc, dir, scratch, false);
                        for (int yc = 0; yc < NYAW; yc++) for (int pc = 0; pc < NPITCH; pc++) for (int sn = 0; sn < 2; sn++) {
                            int st = predict(item, hit, yawCentre(yc), pitchCentre(pc), sn == 1);
                            if (st != 0) any = true;
                            t[index(face, quad, yc, pc, sn == 1)] = (char) st;
                        }
                    }
                }
                env.level.setBlock(scratch, Blocks.AIR.defaultBlockState(), 2 | 16);
                if (any) break;
            }
        }
        tables.put(item, t);
        return t;
    }
}
