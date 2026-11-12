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
        if (StateInfo.air[state]) { if (scaffold.remove(i)) scaffoldInColumn[col(x, z)]--; }
        else if (appeared.size() < 100000) appeared.add(i);
        // a finished cell that changed again (toggled by a stray click, lost its support ...) is looked at once more
        if (g.tgt[i] != 0 && colNext != null && g.inBox(x, y, z) && colNext[col(x, z)] > y && changedAgain.size() < 100000) changedAgain.add(i);
    }

    final it.unimi.dsi.fastutil.ints.IntArrayList changedAgain = new it.unimi.dsi.fastutil.ints.IntArrayList();
    static final int[] WATCH = System.getenv("MCB_WATCH") == null ? null : java.util.Arrays.stream(System.getenv("MCB_WATCH").split(",")).mapToInt(Integer::parseInt).toArray();

    void addScaffold(int x, int y, int z) {
        if (scaffold.add(g.idx(x, y, z))) { scaffoldInColumn[col(x, z)]++; scaffoldPlaced++; }
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
        return s;
    }

    boolean matches(int world, int target) {
        if (world == 0) return false;
        if (StateInfo.canon[world] == StateInfo.canon[target]) return true;
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
    static boolean sameFamily(int a, int b) {
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
        if (z < g.oz - 1) return false;
        if (g.tgt[g.idx(x, y, z)] == 0 || (deferred.contains(g.idx(x, y, z)) && g.cur[g.idx(x, y, z)] == 0)) return true;
        if (!g.inBox(x, y, z) || colNext == null) return false;
        return layerNow == Integer.MAX_VALUE || y > colNext[col(x, z)];
    }

    /** False once the bot carries no scaffolding and cannot get more: it then plans without temporary blocks. */
    boolean haveScaffold = true;
    /** Set while looking for a route that must do without new scaffolding (the bot carries too little). */
    boolean noScaffoldNav;

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
        int i = g.idx(x, y, z);
        return g.tgt[i] == 0 || (deferred.contains(i) && g.cur[i] == 0);   // (a cell waiting to be redone may be borrowed)
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
    public long ropeUndone, ropeOrphaned;

    /** The item the next placement needs (so it can be taken in hand before the bot leans out over an edge). */
    Item wantHeld;

    public long gatesPassed;

    final int[] one = new int[3];
    static final boolean TRACE = System.getenv("MCB_TRACE") != null;

    public long nTowerBlocks;

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

    public String navStats() { return "ticksBy=" + tickUse + " helped=" + helped + " escapes=" + escapes + " mined=" + mined + " strays=" + strays + " misplaced=" + misplaced + " traps avoided/checks/searches=" + trapsAvoided + "/" + trapChecks + "/" + trapSearches + " towerBlocks=" + nTowerBlocks + " rescued=" + towersRescued + " gates=" + gatesPassed + " rope undone/orphaned=" + ropeUndone + "/" + ropeOrphaned + " stances self/natural/lean/pillar=" + nSelf + "/" + nNatural + "/" + nLean + "/" + nPillarStance + " moves pillar/bridge/dig=" + nPillarMoves + "/" + nBridgeMoves + "/" + nDigMoves
        + " buildTicks=" + buildTicks + " buildWall=" + buildWallMs + "ms " + "searches=" + nav.searches + " expanded=" + nav.totalExpanded + " plans=" + plans + " planFails=" + planFails + " predictions=" + oracle.predictions; }

    // ------------------------------------------------------------------ provided by the classes built on this one
    // Moves
    abstract boolean escapeFromHalfFloor();
    abstract int exits(int x, int y, int z);
    abstract boolean wayOut(int x, int y, int z);
    abstract boolean wayOut(int x, int y, int z, boolean breaking);
    abstract boolean sealsIn(int x, int y, int z, int t);
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
    abstract void phase(String what);
    abstract int scanColumn(int x, int fromY, int z);
    abstract boolean eligible(int x, int z);
    abstract void setNext(int x, int z, int y);
    abstract int pickNext();
    abstract void work(int i);
    abstract int surroundings(int i);
    abstract int doCell(int i);
    abstract int attempt(int x, int y, int z, int t, Spec sp, boolean mayEscape);
    abstract void reconcile();
}
