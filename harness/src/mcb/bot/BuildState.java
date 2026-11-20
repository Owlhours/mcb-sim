package mcb.bot;

import it.unimi.dsi.fastutil.ints.IntArrayList;
import it.unimi.dsi.fastutil.ints.IntOpenHashSet;
import java.util.ArrayList;
import java.util.BitSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import mcb.build.Site;
import mcb.build.StateInfo;
import mcb.env.Client;
import mcb.env.Oracle;
import net.minecraft.world.inventory.ContainerInput;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.block.state.properties.SlabType;
import net.minecraft.world.phys.BlockHitResult;

/**
 * What the parts of the builder share: the picture of the site, what is carried and where the supplies are, which
 * temporary blocks stand where, what has been put off, plus the small helpers everything uses. The builder itself
 * is the chain {@link BuildState} - {@link Moves} - {@link Placing} - {@link Stances} - {@link Scaffolds} -
 * {@link Supplies} - {@link Builder}; each class holds one concern, and what a lower one needs from a higher one
 * is declared here.
 */
abstract class BuildState extends Body {
    final Oracle oracle;
    final List<Site.Stock> stock;
    final Item scaffoldItem;
    final int scaffoldState;
    /** The climbable kind of temporary block (real scaffolding): put up as columns that stay until they are in the way or the build is done. */
    final Item towerItem = Items.SCAFFOLDING;
    final Nav nav;
    final IntOpenHashSet scaffold = new IntOpenHashSet();
    final int[] scaffoldInColumn;         // per (x,z) column: number of scaffold cells
    final short[] columnTop;              // per (x,z) column: highest target y (world), or Short.MIN_VALUE
    final BitSet skipped;
    final Click click = new Click();
    final IntOpenHashSet badStand = new IntOpenHashSet();
    public long skippedCells, trips, scaffoldPlaced, replans, plans, planFails;
    /** Wall-clock budget for one build in milliseconds (a safety net for the batch runner; 0 = none). */
    public long wallLimitMs;
    long wallStart = System.currentTimeMillis();
    public final TreeMap<String, Integer> skipReasons = new TreeMap<>();
    public String status = "";

    BuildState(Client c, Grid g, Site site, Oracle oracle) {
        super(c, g);
        this.oracle = oracle;
        this.stock = site.stock;
        this.scaffoldItem = site.scaffoldItem;
        this.scaffoldState = site.scaffoldState;
        this.nav = new Nav(g, scaffold);
        this.nav.towerRule = this::towerOk;
        this.skipped = new BitSet(g.cur.length);
        this.scaffoldInColumn = new int[g.nx * g.nz];
        this.columnTop = new short[g.nx * g.nz];
        java.util.Arrays.fill(columnTop, Short.MIN_VALUE);
        for (int y = g.oy; y < g.oy + g.sy; y++) for (int z = g.oz; z < g.oz + g.sz; z++) for (int x = g.ox; x < g.ox + g.sx; x++) {
            if (g.tgt[g.idx(x, y, z)] != 0) columnTop[col(x, z)] = (short) y;
        }
        int chest = Block.getId(net.minecraft.world.level.block.Blocks.CHEST.defaultBlockState());
        for (Site.Stock s : stock) {
            g.set(s.chestX, s.chestY, s.chestZ, chest);
            g.set(s.chestX + 1, s.chestY, s.chestZ, chest);
        }
    }

    int col(int x, int z) { return (z - g.z0) * g.nx + (x - g.x0); }

    boolean restocking;
    public long strayScreens;
    /** Ticks spent per kind of activity (the innermost one counts). */
    public final TreeMap<String, Long> tickUse = new TreeMap<>();
    String activity = "place";
    String enter(String a) { String old = activity; activity = a; return old; }

    @Override public void tick() {
        super.tick();
        tickUse.merge(activity, 1L, Long::sum);
        if (!restocking && c.screenOpen() && !inventoryOpenByMe) {
            // a click opened some block's screen (a chest, a hopper ...): close it again, or the bot stands frozen
            strayScreens++;
            if (verbose && strayScreens < 10) { log("UNEXPECTED SCREEN " + c.menu().getClass().getSimpleName() + " lastHit=" + (c.lastHit() == null ? "none" : StateInfo.name(g.get(c.lastHit().getBlockPos().getX(), c.lastHit().getBlockPos().getY(), c.lastHit().getBlockPos().getZ())))); new Exception().printStackTrace(System.out); }
            c.closeScreen();
            super.tick();
        }
        if (wallLimitMs > 0 && (ticks & 63) == 0 && System.currentTimeMillis() - wallStart > wallLimitMs) throw new OutOfTime();
    }

    int failLogs, trapLogs, digLogs, tunnelLogs, clickDbgLogs, tryLogs;
    boolean fail(String why) { if (verbose && failLogs++ < 60) log("FAIL " + why); return false; }

    void log(String m) { if (verbose) System.out.println("[t" + ticks + " @" + (cx() - g.ox) + "," + (cy() - g.oy) + "," + (cz() - g.oz) + "] " + m); }
    String rel(int x, int y, int z) { return (x - g.ox) + "," + (y - g.oy) + "," + (z - g.oz); }

    @Override protected void onBlockChange(int x, int y, int z, int state) {
        super.onBlockChange(x, y, z, state);
        if (!g.in(x, y, z)) return;
        if (WATCH != null && x - g.ox == WATCH[0] && y - g.oy == WATCH[1] && z - g.oz == WATCH[2]) {
            log("WATCH " + rel(x, y, z) + " -> " + StateInfo.name(state) + " by " + StackWalker.getInstance().walk(st -> st.skip(3).limit(7).map(f -> f.getMethodName()).toList()) + " lastHit=" + (c.lastHit() == null ? "none" : rel(c.lastHit().getBlockPos().getX(), c.lastHit().getBlockPos().getY(), c.lastHit().getBlockPos().getZ())));
        }
        int i = g.idx(x, y, z);
        if (StateInfo.technical[state] && pistonFaces(x, y, z)) { machineLive = true; if (g.tgt[i] != 0) pushedInto.set(i); }
        if (!StateInfo.air[state] && !mainDone && doorWaiters.containsKey(i)) doorEvents.add(i);
        // the remembered way out (see wayOut): gone when anything was taken away, or something now stands in it
        worldEdits++;   // (THOUGHT_WAY: a world-in-thought is the same only while nothing really changes)
        if (exitValid && (StateInfo.air[state] || exitCells.contains(i))) exitValid = false;
        if (StateInfo.air[state]) { if (scaffold.remove(i)) scaffoldInColumn[col(x, z)]--; }
        else if (appeared.size() < 100000) appeared.add(i);
        // a finished cell that changed again (toggled by a stray click, lost its support ...) is looked at once more
        if (g.tgt[i] != 0 && colNext != null && g.inBox(x, y, z) && colNext[col(x, z)] > y && changedAgain.size() < 100000) changedAgain.add(i);
    }

    /** The last plain way found from the bot to the chests: the cells stood in, and every cell the body passes through. */
    final IntOpenHashSet exitNodes = new IntOpenHashSet(), exitCells = new IntOpenHashSet();
    long worldEdits;
    boolean exitValid;
    final it.unimi.dsi.fastutil.ints.IntArrayList doorEvents = new it.unimi.dsi.fastutil.ints.IntArrayList();
    final it.unimi.dsi.fastutil.ints.IntArrayList changedAgain = new it.unimi.dsi.fastutil.ints.IntArrayList();
    boolean watched(int x, int y, int z) { return WATCH != null && x - g.ox == WATCH[0] && y - g.oy == WATCH[1] && z - g.oz == WATCH[2]; }
    static final int[] WATCH = System.getenv("MCB_WATCH") == null ? null : java.util.Arrays.stream(System.getenv("MCB_WATCH").split(",")).mapToInt(Integer::parseInt).toArray();

    void addScaffold(int x, int y, int z) {
        if (scaffold.add(g.idx(x, y, z))) { scaffoldInColumn[col(x, z)]++; scaffoldPlaced++; scafLog.add(g.idx(x, y, z)); }
    }

    /** Scaffolding is a quick-to-break block; a stray target block may take far longer to break by hand. */
    int breakTicks(int state) {
        float h = StateInfo.state[state].getBlock().defaultDestroyTime();
        if (h < 0) return -1;
        return (int) Math.min(6000, 20 + h * (StateInfo.state[state].requiresCorrectToolForDrops() ? 100 : 30) * 1.2f);
    }

    static final class Spec {
        boolean placeable;
        Item item;
        int first;        // the state the first click must produce
        char[] table;     // placement table, or null if any click that places the item will do
        boolean multi;    // needs further clicks on itself after the first (double slab, several snow layers ...)
        Item content;     // potted plant: the plant to put into the pot after placing it
        boolean chest;    // halves of a double chest: which neighbour it joins is decided when the second half goes in
    }

    final Spec[] specs = new Spec[StateInfo.N];

    Spec spec(int state) {
        Spec s = specs[state];
        if (s != null) return s;
        s = new Spec();
        specs[state] = s;
        s.item = StateInfo.item[state];
        if (s.item == Items.AIR || StateInfo.itemCount[state] == 0) return s;
        BlockState st = StateInfo.state[state], first = st;
        if (StateInfo.itemCount[state] > 1) {
            s.multi = true;
            if (st.hasProperty(BlockStateProperties.SLAB_TYPE)) first = st.setValue(BlockStateProperties.SLAB_TYPE, SlabType.BOTTOM);
            if (st.hasProperty(BlockStateProperties.LAYERS)) first = st.setValue(BlockStateProperties.LAYERS, 1);
            if (st.hasProperty(BlockStateProperties.CANDLES)) first = st.setValue(BlockStateProperties.CANDLES, 1);
            if (st.hasProperty(BlockStateProperties.PICKLES)) first = st.setValue(BlockStateProperties.PICKLES, 1);
            if (st.hasProperty(BlockStateProperties.EGGS)) first = st.setValue(BlockStateProperties.EGGS, 1);
            if (st.hasProperty(BlockStateProperties.FLOWER_AMOUNT)) first = st.setValue(BlockStateProperties.FLOWER_AMOUNT, 1);
            if (st.hasProperty(BlockStateProperties.SEGMENT_AMOUNT)) first = st.setValue(BlockStateProperties.SEGMENT_AMOUNT, 1);
        }
        s.content = StateInfo.contentItem(state);
        if (s.content != null) first = net.minecraft.world.level.block.Blocks.FLOWER_POT.defaultBlockState();
        s.first = Block.getId(first);
        s.chest = st.getBlock() instanceof net.minecraft.world.level.block.ChestBlock;
        char[] t = oracle.table(s.item);
        int want = StateInfo.loose[s.first];
        boolean any = false, all = true;
        for (char e : t) {
            if (e != 0 && StateInfo.loose[e] == want) any = true; else all = false;
        }
        s.placeable = any;
        s.table = all ? null : t;
        if (st.getBlock() == net.minecraft.world.level.block.Blocks.BAMBOO) { s.placeable = true; s.table = null; }   // (see shootFor)
        return s;
    }

    boolean matches(int world, int target) {
        if (world == 0) return false;
        if (StateInfo.canon[world] == StateInfo.canon[target]) return true;
        // bamboo goes in as a shoot; the shoot turns into bamboo when the next piece is put on top of it
        if (shootFor(world, target)) return true;
        // a chest half that waits for (or has found) its other half
        return StateInfo.loose[world] == StateInfo.loose[target] && StateInfo.state[target].getBlock() instanceof net.minecraft.world.level.block.ChestBlock;
    }

    /** For a chest cell: the exact state to produce now - joined with its partner if that is there, single otherwise. */
    int chestWant(int x, int y, int z, int t) {
        BlockState st = StateInfo.state[t];
        var type = st.getValue(net.minecraft.world.level.block.ChestBlock.TYPE);
        if (type == net.minecraft.world.level.block.state.properties.ChestType.SINGLE) return t;
        var dir = net.minecraft.world.level.block.ChestBlock.getConnectedDirection(st);
        int p = g.get(x + dir.getStepX(), y, z + dir.getStepZ());
        if (p != 0 && StateInfo.state[p].getBlock() == st.getBlock()
            && StateInfo.state[p].getValue(net.minecraft.world.level.block.ChestBlock.FACING) == st.getValue(net.minecraft.world.level.block.ChestBlock.FACING)
            && StateInfo.state[p].getValue(net.minecraft.world.level.block.ChestBlock.TYPE) != type) return t;
        return Block.getId(st.setValue(net.minecraft.world.level.block.ChestBlock.TYPE, net.minecraft.world.level.block.state.properties.ChestType.SINGLE));
    }

    boolean needs(int i) {
        int t = g.tgt[i];
        return t != 0 && !skipped.get(i) && !matches(g.cur[i], t) && !StateInfo.secondary[t];
    }

    void skip(int i, String why) {
        if (skipped.get(i)) return;
        skipped.set(i);
        skippedCells++;
        skipReasons.merge(why, 1, Integer::sum);
        if (verbose && skippedCells < 60) log("skip " + rel(g.xOf(i), g.yOf(i), g.zOf(i)) + " " + StateInfo.name(g.tgt[i]) + ": " + why);
    }

    static final int NONE = Integer.MAX_VALUE;
    int[] colNext;                 // per column: y of its lowest cell still to do, or NONE
    final it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap blocked = new it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap();
    final IntOpenHashSet deferred = new IntOpenHashSet();
    final it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap tries = new it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap();
    static final int PLACED = 0, DEFER = 1, SKIP = 2;

    boolean lateToo = true;
    /** Set once every column has been gone through: what is left are the cells that were put off. */
    boolean mainDone;

    final java.util.HashSet<Item> carried = new java.util.HashSet<>();

    public long escapes;

    public long helped;
    public long fromInside;
    public long chained;
    public long fromClose;
    public long pits;
    /** Set by placeHelper: the helper that carries the helper (or -1). */
    int helperBase = -1;
    /** Set while a block lent for a click is being taken away again (that one always goes). */
    boolean mustRemove;

    /** False during the main part of the build: no stance that cannot be left again, no block that shuts the bot in. */
    boolean acceptTraps;
    public long trapsAvoided, trapChecks, trapSearches;

    public long towersRescued;

    public long misplaced;

    /** Cells where a block appeared since the last look (from the server's block updates). */
    final IntArrayList appeared = new IntArrayList();
    public long strays;

    /** Same block as far as judging goes (a filled cauldron counts as a cauldron ...). */
    static boolean sameKind(int a, int b) {
        return StateInfo.state[StateInfo.canon[a]].getBlock() == StateInfo.state[StateInfo.canon[b]].getBlock();
    }

    boolean isSupplyChest(int x, int y, int z) { return z < g.oz - 2 && StateInfo.state[g.get(x, y, z)].getBlock() instanceof net.minecraft.world.level.block.ChestBlock; }

    /** Blocks that turn into one another by themselves and need not be told apart here. */
    /** Is the world block the bamboo shoot that stands in for a piece of bamboo in the target? */
    static boolean shootFor(int world, int target) {
        return StateInfo.state[target].getBlock() == net.minecraft.world.level.block.Blocks.BAMBOO && StateInfo.state[world].getBlock() == net.minecraft.world.level.block.Blocks.BAMBOO_SAPLING;
    }

    static boolean sameFamily(int a, int b) {
        if (shootFor(a, b) || shootFor(b, a)) return true;
        String na = StateInfo.state[a].getBlock().getDescriptionId(), nb = StateInfo.state[b].getBlock().getDescriptionId();
        return na.replace("wall_", "").equals(nb.replace("wall_", "")) || na.replace("_plant", "").equals(nb.replace("_plant", ""));
    }

    /** While planning where to stand for a further click on a block already there: the wanted final state (else 0). */
    int planTopUp;

    static final class Stance {
        int x, y, z;       // the cell to stand in
        int lean = -1;     // 0..3: sneak to the edge towards DXH/DZH[lean] before clicking
        boolean moss;      // to be reached with loose blocks only (no scaffolding column: it will be leant from)
        int column;        // height of the temporary column the stance stands on (0 = none)
        boolean cap;       // stands on one loose block put on top of a scaffolding column
        int cost;
    }

    static final int[] DXH = {1, -1, 0, 0}, DZH = {0, 0, 1, -1};
    // offsets around a cell, nearest first
    static final int[][] RING;
    static {
        List<int[]> l = new ArrayList<>();
        for (int dx = -4; dx <= 4; dx++) for (int dz = -4; dz <= 4; dz++) if (dx * dx + dz * dz <= 17) l.add(new int[] {dx, dz});
        l.sort((a, b) -> (a[0] * a[0] + a[1] * a[1]) - (b[0] * b[0] + b[1] * b[1]));
        RING = l.toArray(new int[0][]);
    }

    /**
     * Where to stand to place (or, with breaking=true, to break) cell (x,y,z). Tries places that exist first,
     * then leaning over the edge of a neighbouring block, then a temporary pillar next to the cell.
     */
    int dbgStand, dbgClick, dbgNav, dbgPillar, dbgPillarClick;

    int layerNow;
    /** Destinations that could not be reached from (about) where the bot is now; forgotten when it moves or builds. */
    final IntOpenHashSet unreachable = new IntOpenHashSet();

    public long unstuck;

    public long mined;
    /** During the last clean-up rounds: scaffolding that got walled in may be reached by breaking through the build. */
    boolean forceMine;
    /** Counts cells in a row for which places to stand existed but none could be reached: the bot is shut in. */
    int shutIn;

    /** Temporary blocks may go where the target has nothing, or where its turn has not come yet (they are removed before it does). */
    boolean scaffoldOk(int x, int y, int z) {
        if (!g.in(x, y, z) || !haveScaffold || noScaffoldNav) return false;
        if (z < g.oz - 1 || pistonFaces(x, y, z)) return false;
        if (g.tgt[g.idx(x, y, z)] == 0 || (brokenOut.contains(g.idx(x, y, z)) && g.cur[g.idx(x, y, z)] == 0)) return true;
        if (!g.inBox(x, y, z) || colNext == null) return false;
        // (relaxNext: also in the column's next cell itself. Only for ways out and for routes that cannot be had
        // otherwise - the way off a spot may be a pillar there (f20cfe46); as a general rule it put loose blocks
        // next to observers and pistons and cost three small redstone builds their pass.)
        return layerNow == Integer.MAX_VALUE || y > colNext[col(x, z)] || (relaxNext && y == colNext[col(x, z)]);
    }

    /**
     * Is this the cell a piston (standing, or still to come) pushes into? A temporary block there gets shoved along
     * with whatever is behind it when the piston fires - or makes it fire. (From the earlier attempt's notes: keeping
     * its helper blocks out of these cells was one of the few things that helped with redstone machines.)
     */
    boolean pistonFaces(int x, int y, int z) {
        if (pushCells == null) {
            // (pistons only ever stand where the target has them: worked out once)
            pushCells = new BitSet(g.tgt.length);
            for (int i = 0; i < g.tgt.length; i++) {
                int st = g.tgt[i];
                if (st == 0 || !(StateInfo.state[st].getBlock() instanceof net.minecraft.world.level.block.piston.PistonBaseBlock)) continue;
                anyPiston = true;
                var f = StateInfo.state[st].getValue(BlockStateProperties.FACING);
                int px = g.xOf(i) + f.getStepX(), py = g.yOf(i) + f.getStepY(), pz = g.zOf(i) + f.getStepZ();
                if (g.in(px, py, pz)) pushCells.set(g.idx(px, py, pz));
            }
        }
        return pushCells.get(g.idx(x, y, z));
    }

    private BitSet pushCells;
    private boolean anyPiston;
    /** Does the build contain a piston anywhere? (Then it is a machine, and temporary blocks are used sparingly.) */
    boolean hasPistons() { pistonFaces(g.ox, g.oy, g.oz); return anyPiston; }

    /**
     * Which side a door's hinge goes on is forced by what stands beside it when it is placed (a full block on one
     * side only puts the hinge there; a door beside it puts the hinge on the far side). Where the target shows the
     * hinge on the other side, the door has to go in BEFORE whatever would force it: those cells wait for the door.
     * Map: waiting cell -> the door's lower cell.
     */
    private it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap afterDoor;
    /** placed cell -> cells that were waiting for it (to be tried again at once). */
    final it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<int[]> doorWaiters = new it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<>();
    /** door (lower cell) -> the full blocks beside it that have to stand first. */
    private final it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<it.unimi.dsi.fastutil.ints.IntArrayList> doorNeeds = new it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<>();

    private static final boolean TRACE_DOORS = System.getenv("MCB_TRACE") != null;

    private void findDoorOrder() {
        afterDoor = new it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap();
        afterDoor.defaultReturnValue(-1);
        for (int i = 0; i < g.tgt.length; i++) {
            int t = g.tgt[i];
            if (t == 0 || !(StateInfo.state[t].getBlock() instanceof net.minecraft.world.level.block.DoorBlock)) continue;
            BlockState st = StateInfo.state[t];
            if (st.getValue(BlockStateProperties.DOUBLE_BLOCK_HALF) != net.minecraft.world.level.block.state.properties.DoubleBlockHalf.LOWER) continue;
            var facing = st.getValue(BlockStateProperties.HORIZONTAL_FACING);
            boolean wantRight = st.getValue(BlockStateProperties.DOOR_HINGE) == net.minecraft.world.level.block.state.properties.DoorHingeSide.RIGHT;
            var left = facing.getCounterClockWise(); var right = facing.getClockWise();
            int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
            int[] l = {cellOrMinus(x + left.getStepX(), y, z + left.getStepZ()), cellOrMinus(x + left.getStepX(), y + 1, z + left.getStepZ())};
            int[] r = {cellOrMinus(x + right.getStepX(), y, z + right.getStepZ()), cellOrMinus(x + right.getStepX(), y + 1, z + right.getStepZ())};
            int balance = 0;
            for (int c2 : l) if (c2 >= 0 && StateInfo.fullCube[g.tgt[c2]]) balance--;
            for (int c2 : r) if (c2 >= 0 && StateInfo.fullCube[g.tgt[c2]]) balance++;
            boolean leftDoor = l[0] >= 0 && lowerDoor(g.tgt[l[0]]), rightDoor = r[0] >= 0 && lowerDoor(g.tgt[r[0]]);
            boolean forcedRight = (leftDoor && !rightDoor) || balance > 0;
            boolean forcedLeft = !forcedRight && ((rightDoor && !leftDoor) || balance < 0);
            if (!(wantRight ? forcedLeft : forcedRight)) {
                // the finished surroundings give the right hinge (or leave the choice to the click): then the door
                // waits until they ARE finished - half of them could force the other side
                for (int c2 : new int[] {l[0], l[1], r[0], r[1]}) {
                    if (c2 < 0 || !StateInfo.fullCube[g.tgt[c2]]) continue;
                    doorNeeds.computeIfAbsent(i, k -> new it.unimi.dsi.fastutil.ints.IntArrayList()).add(c2);
                    int[] old = doorWaiters.get(c2);
                    int[] now = old == null ? new int[] {i} : java.util.Arrays.copyOf(old, old.length + 1);
                    if (old != null) now[old.length] = i;
                    doorWaiters.put(c2, now);
                }
                continue;
            }
            // the side that forces the wrong hinge waits: its full blocks, or the door on the other side
            it.unimi.dsi.fastutil.ints.IntArrayList wait = new it.unimi.dsi.fastutil.ints.IntArrayList();
            int[] side = wantRight ? l : r;
            for (int c2 : side) if (c2 >= 0 && StateInfo.fullCube[g.tgt[c2]]) wait.add(c2);
            int otherDoor = wantRight ? r[0] : l[0];
            if (otherDoor >= 0 && lowerDoor(g.tgt[otherDoor]) && !(wantRight ? leftDoor : rightDoor)) wait.add(otherDoor);
            for (int k = 0; k < wait.size(); k++) if (afterDoor.get(wait.getInt(k)) < 0 && afterDoor.get(i) != wait.getInt(k)) afterDoor.put(wait.getInt(k), i);
            if (TRACE_DOORS && verbose) log("door " + rel(x, y, z) + " " + StateInfo.name(t) + " balance=" + balance + " forced " + (forcedLeft ? "left" : "right") + ": " + wait.size() + " cells wait for it");
            doorWaiters.put(i, wait.toIntArray());
        }
    }

    private int cellOrMinus(int x, int y, int z) { return g.in(x, y, z) ? g.idx(x, y, z) : -1; }

    private static boolean lowerDoor(int t) {
        return t != 0 && StateInfo.state[t].getBlock() instanceof net.minecraft.world.level.block.DoorBlock
            && StateInfo.state[t].getValue(BlockStateProperties.DOUBLE_BLOCK_HALF) == net.minecraft.world.level.block.state.properties.DoubleBlockHalf.LOWER;
    }

    /** Must this cell still wait for a door beside it to go in first? (Not any more once traps are accepted: by then the door has had its chances.) */
    boolean waitsForDoor(int cell) {
        if (afterDoor == null) findDoorOrder();
        if ((afterDoor.isEmpty() && doorNeeds.isEmpty()) || acceptTraps) return false;
        int door = afterDoor.get(cell);
        if (door >= 0 && needs(door)) return true;
        it.unimi.dsi.fastutil.ints.IntArrayList pre = doorNeeds.get(cell);
        if (pre != null) for (int k = 0; k < pre.size(); k++) if (needs(pre.getInt(k))) return true;
        return false;
    }

    /** Cells in front of a piston's face into which that piston has been seen to push while the build was going on. */
    final BitSet pushedInto = new BitSet();
    /** Set once any piston of the build has fired: from then on nothing more is put in front of pistons until the end. */
    boolean machineLive;

    /**
     * Cells that go in after everything else: iron doors, TNT, and whatever stands in front of a piston that has
     * fired during the build (while the circuit around a piston is half built it may be powered when the finished
     * one is not, and shoves the block away again and again). In a build whose pistons keep still, their cells are
     * built in the normal order - later they may be walled in.
     */
    boolean isLate(int cell) {
        int t = g.tgt[cell];
        return t != 0 && (StateInfo.late[t] || pushedInto.get(cell) || (WEB_LATE && isCobweb(t)) || (machineLive && pistonFaces(g.xOf(cell), g.yOf(cell), g.zOf(cell))));
    }

    /** (revision "weblate") Cobwebs are late blocks and wait until the first clean-up is over: the bot cannot move in them. */
    static final boolean WEB_LATE = System.getenv("MCB_NO_WEBLATE") == null;
    /** set once the end game's first clean-up is over */
    boolean websToo;
    static boolean isCobweb(int state) { return StateInfo.state[state].getBlock() == net.minecraft.world.level.block.Blocks.COBWEB; }

    /** Cells of the build that the bot itself broke open (to get out of somewhere); they are put back at the end. */
    final IntOpenHashSet brokenOut = new IntOpenHashSet();

    /** False once the bot carries no scaffolding and cannot get more: it then plans without temporary blocks. */
    boolean haveScaffold = true;
    /** Set while looking for a route that must do without new scaffolding (the bot carries too little). */
    boolean noScaffoldNav;
    /** The pass over put-off cells looks only at cells the bot broke out itself (the mending pass at the very end). */
    boolean mendOnly;
    /** Cells put back by the mending pass after the last clean-up turns. */
    public long mended;
    /** Set for the one try a cell gets before the main pass puts it off: the helper on a second helper is allowed. */
    boolean helperChainNow;
    /** Cells that went in on that try. */
    public long orphans;
    /** Sides of the cell in hand that have had their helper block (bits by direction), and the side of the last one. */
    int helperSkip, helperDir = -1;
    /** Cells placed by the fine search with a helper block standing. */
    public long helpedClose;
    /** Ways found by an enclosed bot on its last try (Moves.enclosed). */
    public long pocketWays;
    /** Scaffold pieces the clean-up does not go for at the moment (it still breaks them where it comes by), or null. */
    it.unimi.dsi.fastutil.ints.IntOpenHashSet leaveAlone;
    /** Every scaffolding piece in the order it was put up (OWN_DOWN: what one clean-up attempt put up). */
    final it.unimi.dsi.fastutil.ints.IntArrayList scafLog = new it.unimi.dsi.fastutil.ints.IntArrayList();
    public long forcedWorse, tidied;

    int inDepot(Item it) { int n = 0; for (Site.Stock st : stock) if (st.item == it && st.count > 0) n += st.count; return n; }

    void checkScaffold() {
        boolean have = count(scaffoldItem) > 0;
        if (!have && !stranded) for (Site.Stock st : stock) if (st.item == scaffoldItem && st.count > 0) { have = true; break; }
        if (have != haveScaffold) { haveScaffold = have; unreachable.clear(); log(have ? "scaffolding available again" : "out of scaffolding"); }
        boolean tw = count(towerItem) > 0;
        if (!tw && !stranded) for (Site.Stock st : stock) if (st.item == towerItem && st.count > 0) { tw = true; break; }
        if (tw != haveTower) { haveTower = tw; unreachable.clear(); }
    }

    boolean haveTower = true;
    /** Set while only walking and climbing are wanted (taking the rope in). */
    boolean towersOff;

    /** Where a scaffolding column may stand: wherever the target has nothing. (It may be built over: it comes down from its foot.) */
    boolean towerOk(int x, int y, int z) {
        if (!g.in(x, y, z) || !haveTower || noScaffoldNav || towersOff) return false;
        if (z < g.oz - 1) return false;   // (not among the supply chests: a column there gets in the way of opening them)
        if (pistonFaces(x, y, z)) return false;
        int i = g.idx(x, y, z);
        return g.tgt[i] == 0 || (brokenOut.contains(i) && g.cur[i] == 0);   // (a cell the bot broke its way out through may be borrowed until it is mended)
    }

    boolean shortOfScaffold() {
        return (nav.scaffoldMoves > 0 && count(scaffoldItem) < nav.scaffoldMoves) || nav.towerMoves > count(towerItem);
    }

    boolean leaning;

    /**
     * The scaffolding the bot is "hanging on": the pillar and bridge blocks it placed to get where it is, in order.
     * Each entry: {move, from x,y,z, to x,y,z, scaffold cell}. Before the bot goes anywhere else it takes them
     * away again in reverse order, so temporary blocks never pile up behind it.
     */
    final ArrayList<int[]> rope = new ArrayList<>();
    /** TOWER_SERVE (revision "hungtower"): the bot's own way stays up while it walks off from its top (Scaffolds.towerServe). */
    boolean keepRope;
    public long ropeUndone, ropeOrphaned;

    /** The item the next placement needs (so it can be taken in hand before the bot leans out over an edge). */
    Item wantHeld;

    public long gatesPassed;

    final int[] one = new int[3];
    static final boolean TRACE = System.getenv("MCB_TRACE") != null;

    public long nTowerBlocks, roped;

    /** True during the final clean-up: every trip up is taken down again before the next one. */
    boolean cleaning;

    public int scaffoldLeft() { return scaffold.size(); }

    Item mustFetch;
    /** Set when the bot could not get to the chests; it then works with what it has. */
    boolean stranded;
    long strandedTick;

    public String inventorySummary() {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 36; i++) { ItemStack st = c.inventorySlot(i); if (!st.isEmpty()) sb.append(i).append(':').append(st.getItem()).append('x').append(st.getCount()).append(' '); }
        return sb.toString();
    }

    public long nSelf, nNatural, nLean, nPillarStance, nPillarMoves, nBridgeMoves, nDigMoves, buildTicks, buildWallMs;

    /** Why the cell in hand was last put off (for the statistics). */
    String deferWhy = "?";
    final java.util.TreeMap<String, Integer> deferReasons = new java.util.TreeMap<>();
    final java.util.TreeMap<String, Integer> blockedWhy = new java.util.TreeMap<>();
    /** State of the run when the main building was over: ticks, how they were spent, cells put off (by reason). */
    String mainStats = "";
    void noteMainDone(int deferredCells) {
        mainTicksAt = ticks; putOffAtMain = deferredCells; scaffoldAtMain = scaffold.size();
        mainStats = " MAIN ticks=" + ticks + " by=" + tickUse + " putOff=" + deferredCells + " why=" + deferReasons + " triedInVain=" + blockedWhy + " scaffoldStanding=" + scaffold.size() + " placed=" + placed + " plans=" + plans + " routes=" + routesTravelled + " routeSteps=" + routeSteps;
    }
    long routesTravelled, routeSteps;
    abstract long stubsLeft();
    abstract long exitUnknownCount();
    abstract long exitJoinedCount();
    abstract long relaxedCount();
    abstract long retreatCount();
    abstract long shutCount();
    abstract long unsafeCount();

    /** Counters for the result file (one column, key=value;...): what happened to the bot, apart from the verdict. */
    public String metrics() {
        return "died=" + (died ? 1 : 0) + ";hurt=" + hurts + ";retreats=" + retreatCount() + ";shutEpisodes=" + shutEpisodes + ";firstShut=" + firstShut
            + ";exitSamples=" + exitSamples[2] + "/" + exitSamples[1] + "/" + exitSamples[0] + ";stubs=" + stubsLeft() + ";alreadyShut=" + shutCount() + ";exitUnknown=" + exitUnknownCount() + ";exitJoined=" + exitJoinedCount()
            + ";mainTicks=" + mainTicksAt + ";putOff=" + putOffAtMain + ";scaffoldAtMain=" + scaffoldAtMain + ";relaxedRoutes=" + relaxedCount() + ";unstuck=" + unstuck + ";mined=" + mined + ";trapsAvoided=" + trapsAvoided
            + ";fromInside=" + fromInside + ";chained=" + chained + ";fromClose=" + fromClose + ";pits=" + pits + ";forcedWorse=" + forcedWorse + ";tidied=" + tidied + ";mended=" + mended + ";orphans=" + orphans + ";helpedClose=" + helpedClose + ";pocketWays=" + pocketWays;
    }
    /** The invariant monitor (see Builder.monitorExit): times the bot was found with no way back to the chests, and when first. */
    long shutEpisodes, firstShut;
    final long[] exitSamples = new long[3];
    long mainTicksAt, putOffAtMain, scaffoldAtMain;

    public String navStats() { return "ticksBy=" + tickUse + mainStats + " alreadyShut=" + shutCount() + " hurt=" + hurts + " retreats=" + retreatCount() + (hurtAt == null ? "" : " first at " + hurtAt) + " END stubs=" + stubsLeft() + " unsafeSpots=" + unsafeCount() + " routes=" + routesTravelled + " routeSteps=" + routeSteps + " helped=" + helped + " fromInside=" + fromInside + " chained=" + chained + " fromClose=" + fromClose + " pits=" + pits + " escapes=" + escapes + " mined=" + mined + " strays=" + strays + " misplaced=" + misplaced + " traps avoided/checks/searches=" + trapsAvoided + "/" + trapChecks + "/" + trapSearches + " towerBlocks=" + nTowerBlocks + " roped=" + roped + " rescued=" + towersRescued + " gates=" + gatesPassed + " rope undone/orphaned=" + ropeUndone + "/" + ropeOrphaned + " stances self/natural/lean/pillar=" + nSelf + "/" + nNatural + "/" + nLean + "/" + nPillarStance + " moves pillar/bridge/dig=" + nPillarMoves + "/" + nBridgeMoves + "/" + nDigMoves
        + " buildTicks=" + buildTicks + " buildWall=" + buildWallMs + "ms " + "searches=" + nav.searches + " expanded=" + nav.totalExpanded + " plans=" + plans + " planFails=" + planFails + " predictions=" + oracle.predictions; }

    // ------------------------------------------------------------------ provided by the classes built on this one
    // Moves
    abstract boolean escapeFromHalfFloor();
    abstract int exits(int x, int y, int z);
    abstract boolean wayOut(int x, int y, int z);
    abstract boolean wayOut(int x, int y, int z, boolean breaking);
    abstract boolean sealsIn(int x, int y, int z, int t);
    /** During the clean-up: the cells about to be taken away (a column from one block up, or one loose block), or null. */
    int[] goingAway;
    abstract boolean safeAfterRemoval(int sx, int sy, int sz);
    abstract int exitLevel(int x, int y, int z);
    // switches for comparing the parts of a batch (to go once decided)
    static final boolean X_EXIT = System.getenv("MCB_NO_EXIT") == null, X_REL = System.getenv("MCB_NO_REL") == null, X_UNSTICK = System.getenv("MCB_NO_UNSTICK") == null,
        X_LOOP = System.getenv("MCB_NO_LOOP") == null, X_KEEP = System.getenv("MCB_NO_KEEP") == null, X_NEXT = System.getenv("MCB_NO_NEXT") == null;
    /** Set while a search may put temporary blocks into a column's next cell (see scaffoldOk). */
    boolean relaxNext;
    /** The temporary block that lends a face to the placement in hand (see placeHelper), or -1. */
    int helperCell = -1;
    /** Set when a block was not placed because it would have shut the bot in where it stood. */
    boolean sealRefused;
    /** While planning where to stand for a block of this state: no spot that the block would shut in (0 = any spot). */
    int avoidSeal;
    abstract boolean search(int x, int y, int z, int maxExpand);
    abstract int spare(Item it);
    abstract void spent(int state);
    abstract boolean unstick();
    abstract int mineTicks(int x, int y, int z);
    abstract boolean route(int x, int y, int z, int maxExpand);
    abstract boolean goTo(Stance st);
    abstract boolean restockWorthwhile();
    abstract boolean lean(int x, int y, int z, int dir);
    abstract boolean leanImpl(int x, int y, int z, int dir);
    abstract void endLean();
    abstract boolean travel();
    abstract boolean travelImpl();
    abstract boolean passGate(int ax, int ay, int az, int bx, int by, int bz);
    abstract boolean toggle(int x, int y, int z);
    abstract void takeDownBehind(int x, int z);
    abstract boolean centre(int x, int y, int z);
    abstract boolean tower(int x, int y, int z, int toY);
    abstract boolean towerImpl(int x, int y, int z, int toY);
    abstract boolean pillar(int x, int y, int z);
    abstract boolean pillarImpl(int x, int y, int z);
    abstract boolean bridge(int px, int py, int pz, int x, int z);
    abstract boolean bridgeImpl(int px, int py, int pz, int x, int z);
    abstract boolean digDown(int x, int y, int z, int targetY);
    abstract boolean digDownImpl(int x, int y, int z, int targetY);
    abstract boolean tunnelTo(int x, int y, int z);
    // Placing
    abstract boolean faceWorks(Spec sp, int face);
    abstract int placeHelper(int x, int y, int z, Spec sp);
    abstract boolean placeFromInside(int x, int y, int z, int t, Spec sp);
    abstract boolean placeClose(int x, int y, int z, int t, Spec sp);
    abstract int surroundings(int i);
    abstract boolean placeFromHere(int x, int y, int z, int t, Spec sp);
    abstract boolean tryPlace(int x, int y, int z, int t, Spec sp);
    abstract void fillPot(int x, int y, int z, int t, Spec sp);
    abstract boolean aimAtBlock(int x, int y, int z);
    abstract void adjust(int x, int y, int z, int t);
    abstract boolean topUp(int x, int y, int z, int t, Spec sp);
    abstract boolean topUpVisible(int x, int y, int z, int t, Spec sp, double ex, double ey, double ez);
    abstract boolean topUpClick(int x, int y, int z, int t, Spec sp, int now);
    abstract boolean topUpTry(int x, int y, int z, int t, Spec sp, int now, float yaw, float pitch);
    abstract boolean selfPlaceable(int x, int y, int z, Spec sp);
    abstract boolean selfPlace(int x, int y, int z, int t, Spec sp);
    abstract void sweepHere();
    // Stances
    abstract Stance planStance(int x, int y, int z, Spec sp, boolean breaking);
    // Scaffolds
    abstract boolean rescueTowers(int x, int y, int z, int t);
    abstract void undoRope();
    abstract void undoRopeImpl();
    abstract boolean breakUnderfoot(int sx, int sy, int sz, int bx, int by, int bz);
    abstract boolean clearBelow(int x, int y, int z);
    abstract boolean looseBelow(int x, int y, int z);
    abstract boolean removeScaffold(int i);
    abstract boolean removeScaffoldImpl(int i);
    abstract boolean removeTower(int i);
    abstract boolean breakTowerBlock(int x, int y, int z);
    abstract void cleanup();
    abstract void cleanupImpl();
    abstract void sweepScaffold();
    // Supplies
    abstract int missingNow(Map<Item, Integer> need, Item it);
    abstract void restock(int fromLayer);
    abstract void restock0(int fromLayer);
    abstract void restock0Impl(int fromLayer);
    // Builder
    public abstract void run();
    abstract void deferredPass(boolean traps);
    abstract boolean anyDeferred();
    abstract boolean timeShort();
    abstract boolean behindPace();
    abstract void phase(String what);
    abstract int scanColumn(int x, int fromY, int z);
    abstract boolean eligible(int x, int z);
    abstract void setNext(int x, int z, int y);
    abstract int pickNext();
    abstract void work(int i);
    abstract int doCell(int i);
    abstract int attempt(int x, int y, int z, int t, Spec sp, boolean mayEscape);
    abstract void reconcile();
}
