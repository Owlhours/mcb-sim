package mcb.build;

import java.util.HashSet;
import java.util.Set;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.tags.BlockTags;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.BedBlock;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.BrushableBlock;
import net.minecraft.world.level.block.FallingBlock;
import net.minecraft.world.level.block.FlowerPotBlock;
import net.minecraft.world.level.block.LiquidBlock;
import net.minecraft.world.level.block.MultifaceBlock;
import net.minecraft.world.level.block.VineBlock;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BedPart;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.block.state.properties.DoubleBlockHalf;
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.level.block.state.properties.SlabType;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.shapes.VoxelShape;

/**
 * Per block-state lookup tables, derived once from the game's own block definitions (requires a booted server so
 * that tags are bound). Index = global state id.
 */
public final class StateInfo {
    public static int N;
    public static BlockState[] state;
    public static boolean[] air;          // air, cave_air, void_air
    public static boolean[] fluid;        // liquids and plants that only exist inside water
    public static boolean[] gravity;      // falls when unsupported (sand, gravel, concrete powder, anvil ...)
    public static boolean[] replaceable;  // placing "into" it replaces it (grass, snow layer ...)
    public static boolean[] fullCube;     // collision shape is the unit cube
    public static boolean[] noCollision;  // collision shape is empty
    public static boolean[] scaffolding;  // the scaffolding block: stood on from above, walked and climbed through otherwise
    public static boolean[] walkThrough;  // no obstacle to a player moving through the cell (no collision, or scaffolding)
    public static boolean[] sturdyTop;    // top face carries things that need a solid floor
    public static byte[] doorEdges;       // doors: which sides of the cell the leaf closes off (bit 0 east, 1 west, 2 south, 3 north)
    public static boolean[] handDoor;     // a door or gate that opens when clicked
    public static boolean[] closedGate;   // a closed fence gate
    /** Hurts a player who stands on it or in it: lit campfires, cactus, fire, wither roses. Never stood on or in. */
    public static boolean[] hazard;
    /** Hurts while stood on, but slowly (magma): allowed, with an eye on the bot's health. */
    public static boolean[] hot;
    /** Climbing: 1 = ladder; 2 = a trapdoor that opens by hand, open; 3 = the same, closed in the top half of its cell. */
    public static byte[] rung;
    /** For ladders and trapdoors: 1 + the 2D index of their facing. */
    public static byte[] rungFacing;
    public static boolean[] leaves;
    public static boolean[] climbable;
    public static boolean[] interactive;  // right-clicking it does something (so a block can only be placed against it while sneaking)
    public static boolean[] secondary;    // upper door half, bed head, upper half of a double plant: appears by itself with its primary part
    public static boolean[] late;         // doors, gates, trapdoors: placed last so that they do not shut the builder in or out
    public static boolean[] technical;    // not part of what a builder places: piston heads, moving pistons, portals, structure voids ...
    public static char[] dewater;         // same state with waterlogged=false
    public static char[] canon;           // representative of the state's equivalence class for judging
    public static char[] loose;           // canon, further ignoring what is set after placing (repeater delay, open/closed ...) or settles by itself (chest halves)
    public static Item[] item;            // the item that places this block (Items.AIR if none)
    public static byte[] itemCount;       // how many of that item the state takes (double slab 2, 3 snow layers 3 ...)
    /** Collision box extent within the player's footprint when centred on the cell, in 1/16: top and bottom; top=0 if nothing. */
    public static byte[] colTop, colBottom;
    /** The same over the whole cell (not just the footprint): top of the highest and bottom of the lowest collision box, in 1/16. */
    public static byte[] anyTop, anyBottom;

    private static final Set<String> IGNORED = Set.of("waterlogged", "powered", "power", "lit", "triggered", "extended", "locked", "enabled",
        "in_wall", "snowy", "distance", "persistent", "age", "moisture", "stage", "honey_level", "level", "bites", "charges", "eye", "has_record",
        "has_book", "has_bottle_0", "has_bottle_1", "has_bottle_2", "slot_0_occupied", "slot_1_occupied", "slot_2_occupied", "slot_3_occupied",
        "slot_4_occupied", "slot_5_occupied", "occupied", "hatch", "dusted", "unstable", "conditional", "drag", "tilt", "sculk_sensor_phase", "bloom",
        "can_summon", "shrieking", "cracked", "crafting", "ominous", "trial_spawner_state", "vault_state", "tip", "bottom", "natural",
        "creaking_heart_state", "hydration", "potent_sulfur_state", "berries", "attached", "disarmed", "short", "thickness", "signal_fire",
        "instrument", "side_chain", "leaves", "copper_golem_pose");
    private static final Set<String> CONNECTION = Set.of("north", "east", "south", "west", "up", "down");
    /** Properties a player sets by right-clicking the placed block. */
    public static final Set<String> ADJUSTABLE = Set.of("delay", "mode", "open", "inverted", "note");

    public static boolean ready() { return state != null; }

    @SuppressWarnings({"unchecked", "rawtypes"})
    public static void init() {
        if (state != null) return;
        N = Block.BLOCK_STATE_REGISTRY.size();
        state = new BlockState[N];
        air = new boolean[N]; fluid = new boolean[N]; gravity = new boolean[N]; replaceable = new boolean[N]; fullCube = new boolean[N];
        noCollision = new boolean[N]; leaves = new boolean[N]; climbable = new boolean[N]; interactive = new boolean[N]; secondary = new boolean[N];
        technical = new boolean[N]; late = new boolean[N];
        scaffolding = new boolean[N]; walkThrough = new boolean[N]; sturdyTop = new boolean[N];
        doorEdges = new byte[N]; handDoor = new boolean[N]; closedGate = new boolean[N];
        rung = new byte[N]; rungFacing = new byte[N];
        hazard = new boolean[N]; hot = new boolean[N];
        dewater = new char[N]; canon = new char[N]; loose = new char[N]; item = new Item[N]; itemCount = new byte[N]; colTop = new byte[N]; colBottom = new byte[N]; anyTop = new byte[N]; anyBottom = new byte[N];
        Set<Class<?>> interactiveClasses = new HashSet<>(), plainClasses = new HashSet<>();
        EmptyBlockGetter g = EmptyBlockGetter.INSTANCE;
        for (int i = 0; i < N; i++) {
            BlockState s = Block.stateById(i);
            state[i] = s;
            Block b = s.getBlock();
            air[i] = s.isAir();
            boolean waterPlant = !s.getFluidState().isEmpty() && !s.hasProperty(BlockStateProperties.WATERLOGGED);
            fluid[i] = b instanceof LiquidBlock || waterPlant || b == Blocks.BUBBLE_COLUMN;
            gravity[i] = b instanceof FallingBlock || b instanceof BrushableBlock;
            replaceable[i] = s.canBeReplaced();
            VoxelShape col;
            try { col = s.getCollisionShape(g, BlockPos.ZERO); } catch (RuntimeException e) { col = net.minecraft.world.phys.shapes.Shapes.block(); }
            fullCube[i] = Block.isShapeFullBlock(col);
            noCollision[i] = col.isEmpty();
            scaffolding[i] = b == Blocks.SCAFFOLDING;
            walkThrough[i] = noCollision[i] || scaffolding[i];
            try { sturdyTop[i] = s.isFaceSturdy(g, BlockPos.ZERO, net.minecraft.core.Direction.UP); } catch (RuntimeException e) { sturdyTop[i] = false; }
            if (b instanceof net.minecraft.world.level.block.DoorBlock door) {
                // the leaf is a thin slab along one side of the cell: one can stand in the cell, but not cross that side
                int m = 0;
                for (AABB box : col.toAabbs()) {
                    if (box.maxX > 0.8 && box.minZ < 0.8 && box.maxZ > 0.2) m |= 1;
                    if (box.minX < 0.2 && box.minZ < 0.8 && box.maxZ > 0.2) m |= 2;
                    if (box.maxZ > 0.8 && box.minX < 0.8 && box.maxX > 0.2) m |= 4;
                    if (box.minZ < 0.2 && box.minX < 0.8 && box.maxX > 0.2) m |= 8;
                }
                doorEdges[i] = (byte) m;
                handDoor[i] = door.type().canOpenByHand();
                walkThrough[i] = true;
            }
            // (not magma: whole floors are made of it, and a second or two on it is survivable - see Builder.guardHealth;
            // not berry bushes: placed without berries they do nothing, and nothing grows during a build)
            hazard[i] = (b instanceof net.minecraft.world.level.block.CampfireBlock && s.getValue(BlockStateProperties.LIT))
                || b == Blocks.CACTUS || b == Blocks.WITHER_ROSE || b instanceof net.minecraft.world.level.block.BaseFireBlock;
            hot[i] = b == Blocks.MAGMA_BLOCK;
            if (b instanceof net.minecraft.world.level.block.LadderBlock) {
                rung[i] = 1;
                rungFacing[i] = (byte) (1 + s.getValue(BlockStateProperties.HORIZONTAL_FACING).get2DDataValue());
            }
            if (b instanceof net.minecraft.world.level.block.TrapDoorBlock && b != Blocks.IRON_TRAPDOOR) {
                boolean open = s.getValue(BlockStateProperties.OPEN);
                if (open) rung[i] = 2;
                else if (s.getValue(BlockStateProperties.HALF) == net.minecraft.world.level.block.state.properties.Half.TOP) rung[i] = 3;
                rungFacing[i] = (byte) (1 + s.getValue(BlockStateProperties.HORIZONTAL_FACING).get2DDataValue());
            }
            if (b instanceof net.minecraft.world.level.block.FenceGateBlock) {
                handDoor[i] = true;
                closedGate[i] = !s.getValue(BlockStateProperties.OPEN);
            }
            double top = 0, bottom = 99, atop = 0, abottom = 99;
            for (AABB box : col.toAabbs()) {
                if (box.maxX > 0.2 && box.minX < 0.8 && box.maxZ > 0.2 && box.minZ < 0.8) { top = Math.max(top, box.maxY); bottom = Math.min(bottom, box.minY); }
                atop = Math.max(atop, box.maxY); abottom = Math.min(abottom, box.minY);
            }
            anyTop[i] = (byte) Math.round(atop * 16);
            anyBottom[i] = (byte) (abottom > 90 ? 0 : Math.round(abottom * 16));
            colTop[i] = (byte) Math.round(top * 16);
            colBottom[i] = (byte) (bottom > 90 ? 0 : Math.round(bottom * 16));
            leaves[i] = s.is(BlockTags.LEAVES);
            climbable[i] = s.is(BlockTags.CLIMBABLE);
            interactive[i] = isInteractive(b.getClass(), interactiveClasses, plainClasses);
            secondary[i] = (s.hasProperty(BlockStateProperties.DOUBLE_BLOCK_HALF) && s.getValue(BlockStateProperties.DOUBLE_BLOCK_HALF) == DoubleBlockHalf.UPPER)
                || (b instanceof BedBlock && s.getValue(BlockStateProperties.BED_PART) == BedPart.HEAD);
            // (doors and gates that open by hand are no obstacle to the builder; the rest could shut it in or out)
            // (and TNT: it is payload, not footing, and anything live next to it sets it off)
            late[i] = (b instanceof net.minecraft.world.level.block.DoorBlock d0 && !d0.type().canOpenByHand()) || b instanceof net.minecraft.world.level.block.TntBlock;
            technical[i] = b == Blocks.PISTON_HEAD || b == Blocks.MOVING_PISTON || b == Blocks.STRUCTURE_VOID || b == Blocks.NETHER_PORTAL
                || b == Blocks.END_PORTAL || b == Blocks.END_GATEWAY || b == Blocks.LIGHT || b == Blocks.FROSTED_ICE;
            BlockState dw = s.hasProperty(BlockStateProperties.WATERLOGGED) ? s.setValue(BlockStateProperties.WATERLOGGED, false) : s;
            dewater[i] = (char) Block.getId(dw);
            // ---- items
            Item it = b.asItem();
            if (b instanceof FlowerPotBlock) it = Items.FLOWER_POT;
            if (b == Blocks.WATER_CAULDRON || b == Blocks.LAVA_CAULDRON || b == Blocks.POWDER_SNOW_CAULDRON) it = Items.CAULDRON;
            item[i] = it;
            int cnt = 1;
            if (s.hasProperty(BlockStateProperties.SLAB_TYPE) && s.getValue(BlockStateProperties.SLAB_TYPE) == SlabType.DOUBLE) cnt = 2;
            if (s.hasProperty(BlockStateProperties.LAYERS)) cnt = s.getValue(BlockStateProperties.LAYERS);
            if (s.hasProperty(BlockStateProperties.CANDLES)) cnt = s.getValue(BlockStateProperties.CANDLES);
            if (s.hasProperty(BlockStateProperties.PICKLES)) cnt = s.getValue(BlockStateProperties.PICKLES);
            if (s.hasProperty(BlockStateProperties.EGGS)) cnt = s.getValue(BlockStateProperties.EGGS);
            if (s.hasProperty(BlockStateProperties.FLOWER_AMOUNT)) cnt = s.getValue(BlockStateProperties.FLOWER_AMOUNT);
            if (s.hasProperty(BlockStateProperties.SEGMENT_AMOUNT)) cnt = s.getValue(BlockStateProperties.SEGMENT_AMOUNT);
            if (secondary[i] || it == Items.AIR) cnt = 0;
            itemCount[i] = (byte) cnt;
            // ---- canonical form
            BlockState c = s;
            if (air[i]) c = Blocks.AIR.defaultBlockState();
            else if (b == Blocks.WATER_CAULDRON || b == Blocks.LAVA_CAULDRON || b == Blocks.POWDER_SNOW_CAULDRON) c = Blocks.CAULDRON.defaultBlockState();
            else {
                BlockState def = b.defaultBlockState();
                boolean multiface = b instanceof MultifaceBlock || b instanceof VineBlock;
                boolean ironOpen = b == Blocks.IRON_DOOR || b == Blocks.IRON_TRAPDOOR;
                for (Property p : s.getProperties()) {
                    String n = p.getName();
                    boolean ignore = IGNORED.contains(n) || (CONNECTION.contains(n) && !multiface)
                        || (n.equals("shape")) || (n.equals("open") && ironOpen);
                    if (ignore) c = c.setValue(p, def.getValue(p));
                }
            }
            // a closed fence gate looks the same from both sides, and opening one turns it to swing away from whoever
            // opens it: north/south and east/west are one state each
            if (b instanceof net.minecraft.world.level.block.FenceGateBlock && !c.getValue(BlockStateProperties.OPEN)) c = gateAxis(c);
            canon[i] = (char) Block.getId(c);
            BlockState l = c;
            if (!air[i]) {
                BlockState def = b.defaultBlockState();
                for (Property p : c.getProperties()) {
                    String n = p.getName();
                    if (ADJUSTABLE.contains(n) || (n.equals("type") && b instanceof net.minecraft.world.level.block.ChestBlock)) l = l.setValue(p, def.getValue(p));
                }
            }
            if (b instanceof net.minecraft.world.level.block.FenceGateBlock) l = gateAxis(l);
            loose[i] = (char) Block.getId(l);
        }
    }

    /** True if the block class (below BlockBehaviour) overrides either right-click hook. */
    private static boolean isInteractive(Class<?> c, Set<Class<?>> yes, Set<Class<?>> no) {
        if (yes.contains(c)) return true;
        if (no.contains(c)) return false;
        boolean r = false;
        for (Class<?> k = c; k != null && k != BlockBehaviour.class && k != Object.class; k = k.getSuperclass()) {
            for (var m : k.getDeclaredMethods()) {
                if (m.getName().equals("useWithoutItem") || m.getName().equals("useItemOn")) { r = true; break; }
            }
            if (r) break;
        }
        (r ? yes : no).add(c);
        return r;
    }

    /** For a potted plant: the plant that goes into the pot (used on the placed, empty pot). Otherwise null. */
    public static Item contentItem(int id) {
        if (state[id].getBlock() instanceof FlowerPotBlock pot && pot.getPotted() != Blocks.AIR) {
            Item it = pot.getPotted().asItem();
            return it == Items.AIR ? null : it;
        }
        return null;
    }

    /** "Decorative" for judging purposes: a block with no collision box at all (flowers, torches, signs, rails, redstone dust ...). */
    private static BlockState gateAxis(BlockState s) {
        net.minecraft.core.Direction f = s.getValue(BlockStateProperties.HORIZONTAL_FACING);
        if (f == net.minecraft.core.Direction.SOUTH) return s.setValue(BlockStateProperties.HORIZONTAL_FACING, net.minecraft.core.Direction.NORTH);
        if (f == net.minecraft.core.Direction.WEST) return s.setValue(BlockStateProperties.HORIZONTAL_FACING, net.minecraft.core.Direction.EAST);
        return s;
    }

    public static boolean decorative(int id) { return noCollision[id] && !air[id]; }

    /**
     * The wider reading of "decorative" and "leaves judged leniently", after the rules the project's owner settled on
     * for the earlier attempt (phase1-reference/crew/JUDGE-PROPOSALS.md, 2026-11-10): decoration that is not a full
     * block - redstone parts, rails, lecterns, flower pots, candles, lanterns, torches, flowers and other plants,
     * carpets, buttons, levers, signs, banners, heads - is exempt, missing or in the wrong state, and leaves count as
     * foliage. Stairs, slabs, fences, walls, panes, doors and trapdoors still count; so do cactus, bamboo, sea
     * pickles, dripleaf and chorus. (The owner's list of 322 ids is not available here; this is the same idea
     * expressed through block classes.)
     */
    public static boolean decorativeWide(int id) {
        if (air[id]) return false;
        if (noCollision[id] || leaves[id]) return true;
        Block b = state[id].getBlock();
        return b instanceof net.minecraft.world.level.block.CarpetBlock || b.getDescriptionId().endsWith("carpet") || b instanceof net.minecraft.world.level.block.LanternBlock
            || b instanceof FlowerPotBlock || b instanceof net.minecraft.world.level.block.CandleBlock
            || b instanceof net.minecraft.world.level.block.LecternBlock || b instanceof net.minecraft.world.level.block.DiodeBlock
            || b instanceof net.minecraft.world.level.block.AbstractSkullBlock || b instanceof net.minecraft.world.level.block.DaylightDetectorBlock
            || b instanceof net.minecraft.world.level.block.SignBlock || b instanceof net.minecraft.world.level.block.AbstractBannerBlock;
    }

    /** Blocks only an operator in creative mode can place (the earlier attempt took them out of its targets). */
    public static boolean gameMaster(int id) {
        Block b = state[id].getBlock();
        return b instanceof net.minecraft.world.level.block.GameMasterBlock;
    }

    public static String name(int id) { return state[id].toString().replace("Block{minecraft:", "").replace("}", ""); }

    public static boolean faceSturdy(int id, Direction d) { return state[id].isFaceSturdy(EmptyBlockGetter.INSTANCE, BlockPos.ZERO, d); }
}
