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
 * The builder. It works through the target layer by layer from the ground up. For every cell it finds a place to
 * stand (on the ground, on what it has built, leaning over an edge, or on a temporary pillar) from which a click
 * on a neighbouring block face yields exactly the wanted block state, goes there and clicks. Materials come from
 * the supply chests. Temporary blocks are tracked and taken away again: pillars are dug down when the bot leaves
 * them, anything in the way of the build is removed before it would be covered, and what is left goes at the end.
 */
public final class Builder extends Body {
    private final Oracle oracle;
    private final List<Site.Stock> stock;
    private final Item scaffoldItem;
    private final int scaffoldState;
    /** The climbable kind of temporary block (real scaffolding): put up as columns that stay until they are in the way or the build is done. */
    private final Item towerItem = Items.SCAFFOLDING;
    private final int towerState = Block.getId(net.minecraft.world.level.block.Blocks.SCAFFOLDING.defaultBlockState());
    private final Nav nav;
    private final IntOpenHashSet scaffold = new IntOpenHashSet();
    private final int[] scaffoldInColumn;         // per (x,z) column: number of scaffold cells
    private final short[] columnTop;              // per (x,z) column: highest target y (world), or Short.MIN_VALUE
    private final BitSet skipped;
    private final Click click = new Click();
    private final IntOpenHashSet badStand = new IntOpenHashSet();
    public long skippedCells, trips, scaffoldPlaced, replans, plans, planFails;
    /** Wall-clock budget for one build in milliseconds (a safety net for the batch runner; 0 = none). */
    public long wallLimitMs;
    private long wallStart = System.currentTimeMillis();
    public final TreeMap<String, Integer> skipReasons = new TreeMap<>();
    public String status = "";

    public Builder(Client c, Grid g, Site site, Oracle oracle) {
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

    private int col(int x, int z) { return (z - g.z0) * g.nx + (x - g.x0); }

    private boolean restocking;
    public long strayScreens;
    /** Ticks spent per kind of activity (the innermost one counts). */
    public final TreeMap<String, Long> tickUse = new TreeMap<>();
    private String activity = "place";
    private String enter(String a) { String old = activity; activity = a; return old; }

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

    private int failLogs, trapLogs, digLogs, tunnelLogs, clickDbgLogs, tryLogs;
    private boolean fail(String why) { if (verbose && failLogs++ < 60) log("FAIL " + why); return false; }

    private void log(String m) { if (verbose) System.out.println("[t" + ticks + " @" + (cx() - g.ox) + "," + (cy() - g.oy) + "," + (cz() - g.oz) + "] " + m); }
    private String rel(int x, int y, int z) { return (x - g.ox) + "," + (y - g.oy) + "," + (z - g.oz); }

    @Override protected void onBlockChange(int x, int y, int z, int state) {
        super.onBlockChange(x, y, z, state);
        if (!g.in(x, y, z)) return;
        if (StateInfo.air[state]) { if (scaffold.remove(g.idx(x, y, z))) scaffoldInColumn[col(x, z)]--; }
        else if (appeared.size() < 100000) appeared.add(g.idx(x, y, z));
    }

    private void addScaffold(int x, int y, int z) {
        if (scaffold.add(g.idx(x, y, z))) { scaffoldInColumn[col(x, z)]++; scaffoldPlaced++; }
    }

    /** Scaffolding is a quick-to-break block; a stray target block may take far longer to break by hand. */
    private int breakTicks(int state) {
        float h = StateInfo.state[state].getBlock().defaultDestroyTime();
        if (h < 0) return -1;
        return (int) Math.min(6000, 20 + h * (StateInfo.state[state].requiresCorrectToolForDrops() ? 100 : 30) * 1.2f);
    }

    // ------------------------------------------------------------------ how each block state gets placed
    private static final class Spec {
        boolean placeable;
        Item item;
        int first;        // the state the first click must produce
        char[] table;     // placement table, or null if any click that places the item will do
        boolean multi;    // needs further clicks on itself after the first (double slab, several snow layers ...)
        Item content;     // potted plant: the plant to put into the pot after placing it
        boolean chest;    // halves of a double chest: which neighbour it joins is decided when the second half goes in
    }

    private final Spec[] specs = new Spec[StateInfo.N];

    private Spec spec(int state) {
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

    private boolean matches(int world, int target) {
        if (world == 0) return false;
        if (StateInfo.canon[world] == StateInfo.canon[target]) return true;
        // a chest half that waits for (or has found) its other half
        return StateInfo.loose[world] == StateInfo.loose[target] && StateInfo.state[target].getBlock() instanceof net.minecraft.world.level.block.ChestBlock;
    }

    /** For a chest cell: the exact state to produce now - joined with its partner if that is there, single otherwise. */
    private int chestWant(int x, int y, int z, int t) {
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

    private boolean needs(int i) {
        int t = g.tgt[i];
        return t != 0 && !skipped.get(i) && !matches(g.cur[i], t) && !StateInfo.secondary[t];
    }

    private void skip(int i, String why) {
        if (skipped.get(i)) return;
        skipped.set(i);
        skippedCells++;
        skipReasons.merge(why, 1, Integer::sum);
        if (verbose && skippedCells < 60) log("skip " + rel(g.xOf(i), g.yOf(i), g.zOf(i)) + " " + StateInfo.name(g.tgt[i]) + ": " + why);
    }

    // ------------------------------------------------------------------ main loop
    private static final int NONE = Integer.MAX_VALUE;
    private int[] colNext;                 // per column: y of its lowest cell still to do, or NONE
    private final it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap blocked = new it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap();
    private final IntOpenHashSet deferred = new IntOpenHashSet();
    private final it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap tries = new it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap();
    private static final int PLACED = 0, DEFER = 1, SKIP = 2;

    /**
     * Order of work. Every column is built bottom-up, and a column may only take its next block when none of its
     * eight neighbours still has something lower to do. So the build front never roofs over unfinished work, wide
     * masses rise as terraces the bot can walk on, and a lone pillar or tower is finished in one visit. Among the
     * cells that are allowed, the bot takes the one nearest to it.
     */
    public void run() {
        try {
            colNext = new int[g.nx * g.nz];
            java.util.Arrays.fill(colNext, NONE);
            tilesX = (g.sx + 7) >> 3; tilesZ = (g.sz + 7) >> 3;
            tilePending = new int[tilesX * tilesZ];
            for (int z = g.oz; z < g.oz + g.sz; z++) for (int x = g.ox; x < g.ox + g.sx; x++) setNext(x, z, scanColumn(x, g.oy, z));
            restock(g.oy);
            int guard = 0;
            while (true) {
                int i = pickNext();
                if (i < 0) {
                    // everything that may go next has been tried and found no way: let those cells wait (they may
                    // hang from something built later) and move on above them
                    if (blocked.isEmpty()) break;
                    boolean any = false;
                    for (int cell : blocked.keySet().toIntArray()) {
                        blocked.remove(cell);
                        if (!needs(cell)) continue;
                        deferred.add(cell);
                        int x = g.xOf(cell), z = g.zOf(cell);
                        setNext(x, z, scanColumn(x, g.yOf(cell) + 1, z));
                        any = true;
                    }
                    if (!any && ++guard > 3) break;
                    continue;
                }
                guard = 0;
                work(i);
            }
            // what was put off: one more try each, now that everything else stands - first from places the bot can
            // leave again, then from anywhere (it breaks its way out afterwards and repairs that from outside)
            lateToo = false;
            deferredPass(false);
            undoRope();
            reconcile();
            // scaffolding comes out while the doorways are still open; then doors, gates and trapdoors go in
            layerNow = Integer.MAX_VALUE;
            cleanup();
            lateToo = true;
            deferredPass(false);
            deferredPass(true);
            undoRope();
            reconcile();
            status = "built";
            buildTicks = ticks; buildWallMs = System.currentTimeMillis() - wallStart;
            cleanup();
            // cells that waited for scaffolding under them to go
            deferredPass(false);
            undoRope();
            reconcile();
            cleanup();
            noRoofing = false;
            deferredPass(false);
            deferredPass(true);
            undoRope();
            reconcile();
            cleanup();
            for (int round = 0; round < 3 && (!scaffold.isEmpty() || anyDeferred()); round++) {
                // what is left is walled in: go in through the build, then put back what was taken out
                forceMine = true;
                acceptTraps = true;
                cleanup();
                forceMine = false;
                acceptTraps = false;
                undoRope();
                deferredPass(false);
                if (round > 0) deferredPass(true);
                undoRope();
                reconcile();
                cleanup();
            }
            for (int cell : deferred) if (needs(cell)) skip(cell, "no way to place");
            status = "done";
        } catch (OutOfTime e) {
            status = ticks >= maxTicks ? "out of time" : "wall limit";
        }
        c.releaseKeys();
        c.holdAttack(false);
    }

    private void deferredPass(boolean traps) {
        acceptTraps = traps;
        try {
            for (int cell : deferred.toIntArray()) {
                if (!needs(cell)) { deferred.remove(cell); continue; }
                if (!lateToo && StateInfo.late[g.tgt[cell]]) continue;
                if (doCell(cell) != DEFER) deferred.remove(cell);
            }
        } finally {
            acceptTraps = false;
        }
    }

    private boolean lateToo = true;

    private boolean anyDeferred() {
        for (int cell : deferred) if (needs(cell)) return true;
        return false;
    }

    private int scanColumn(int x, int fromY, int z) {
        for (int y = Math.max(fromY, g.oy); y < g.oy + g.sy; y++) {
            int i = g.idx(x, y, z);
            if (g.tgt[i] == 0 || !needs(i) || deferred.contains(i)) continue;
            // doors and the like wait until everything else is built
            if (StateInfo.late[g.tgt[i]]) { deferred.add(i); continue; }
            return y;
        }
        return NONE;
    }

    private boolean eligible(int x, int z) {
        int y = colNext[col(x, z)];
        if (y == NONE) return false;
        for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) {
            if (dx == 0 && dz == 0) continue;
            int nx = x + dx, nz = z + dz;
            if (nx < g.ox || nz < g.oz || nx >= g.ox + g.sx || nz >= g.oz + g.sz) continue;
            if (colNext[col(nx, nz)] < y) return false;
        }
        return true;
    }

    // pending columns are counted per 8x8 tile so that the search below can skip finished areas
    private int tilesX, tilesZ;
    private int[] tilePending;

    private void setNext(int x, int z, int y) {
        int c0 = col(x, z);
        int old = colNext[c0];
        colNext[c0] = y;
        if ((old == NONE) != (y == NONE)) tilePending[((z - g.oz) >> 3) * tilesX + ((x - g.ox) >> 3)] += y == NONE ? -1 : 1;
    }

    /** The allowed cell nearest to the bot, or -1. */
    private int pickNext() {
        int bx = Math.max(g.ox, Math.min(g.ox + g.sx - 1, cx())), bz = Math.max(g.oz, Math.min(g.oz + g.sz - 1, cz())), by = cy();
        int btx = (bx - g.ox) >> 3, btz = (bz - g.oz) >> 3;
        int best = -1;
        double bestCost = Double.MAX_VALUE;
        int maxR = Math.max(tilesX, tilesZ);
        carried.clear();
        for (int k = 0; k < 36; k++) { ItemStack st = c.inventorySlot(k); if (!st.isEmpty()) carried.add(st.getItem()); }
        for (int r = 0; r <= maxR; r++) {
            // every column in tile ring r is at least (r - 1) * 8 + 1 away
            if (best >= 0 && (r - 1) * 8 + 1 > bestCost) break;
            for (int dtz = -r; dtz <= r; dtz++) {
                int tz = btz + dtz;
                if (tz < 0 || tz >= tilesZ) continue;
                int step = (dtz == -r || dtz == r) ? 1 : 2 * r;
                for (int dtx = -r; dtx <= r; dtx += Math.max(1, step)) {
                    int tx = btx + dtx;
                    if (tx < 0 || tx >= tilesX || tilePending[tz * tilesX + tx] == 0) continue;
                    int x0 = g.ox + (tx << 3), z0 = g.oz + (tz << 3);
                    int x1 = Math.min(x0 + 8, g.ox + g.sx), z1 = Math.min(z0 + 8, g.oz + g.sz);
                    for (int z = z0; z < z1; z++) for (int x = x0; x < x1; x++) {
                        int y = colNext[col(x, z)];
                        if (y == NONE || !eligible(x, z)) continue;
                        int i = g.idx(x, y, z);
                        if (blocked.containsKey(i) && blocked.get(i) == surroundings(i)) continue;
                        double cost = Math.max(Math.abs(x - bx), Math.abs(z - bz)) + 0.4 * Math.abs(y - by);
                        // cells with nothing to attach to yet are left for later: usually a neighbour arrives first
                        boolean attached = false;
                        for (int d = 0; d < 6 && !attached; d++) attached = g.get(x + DX[d], y + DY[d], z + DZ[d]) != 0;
                        if (!attached) cost += 60 + 2 * Math.max(0, y - by);
                        if (!carried.contains(StateInfo.item[g.tgt[i]])) cost += 400;
                        if (cost < bestCost) { bestCost = cost; best = i; }
                    }
                }
            }
        }
        return best;
    }

    private final java.util.HashSet<Item> carried = new java.util.HashSet<>();

    public long escapes;
    private int escapeBudget = 40;

    /**
     * Standing on a half-height floor (bottom slabs) with no way up: nothing can be pillared from here. Take out
     * one slab next to the bot, put a scaffold block in its place and step onto that; the slab is put back at the
     * very end. Returns true if the bot now stands on a full block.
     */
    private boolean escapeFromHalfFloor() {
        int x = cx(), y = cy(), z = cz();
        if (!c.onGround() || g.stand16(x, y, z) <= 0 || count(scaffoldItem) == 0) return false;
        for (int d = 0; d < 4; d++) {
            int nx = x + DXH[d], nz = z + DZH[d];
            if (!g.in(nx, y, nz)) continue;
            int i = g.idx(nx, y, nz);
            int w = g.cur[i];
            if (w == 0 || scaffold.contains(i) || StateInfo.interactive[w] || StateInfo.anyTop[w] > 9) continue;
            int bt = breakTicks(w);
            if (bt < 0 || bt > 400) continue;
            if (!g.passable(nx, y + 1, nz) || !g.passable(nx, y + 2, nz) || !g.passable(x, y + 2, z) || !Nav.flatTop(g.get(nx, y - 1, nz))) continue;
            Spec sp = spec(scaffoldState);
            settle();
            var e = c.eye();
            if (!findBreak(nx, y, nz, e.x, e.y, e.z, click) || !breakBlock(nx, y, nz, click.ax, click.ay, click.az, bt + 20)) continue;
            escapes++;
            log("escape: took out " + StateInfo.name(w) + " at " + rel(nx, y, nz));
            if (g.tgt[i] != 0) { deferred.add(i); }
            if (!placeFromHere(nx, y, nz, scaffoldState, sp)) return false;
            addScaffold(nx, y, nz);
            one[0] = nx; one[1] = y + 1; one[2] = nz;
            return follow(one, 0, 1);
        }
        return false;
    }

    private long progressMark = -1, progressTick;

    private void work(int i) {
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        layerNow = y;
        // watchdog: a long time without a single block placed or removed means the bot is going in circles
        if (placed + broken != progressMark) { progressMark = placed + broken; progressTick = ticks; }
        if (ticks - progressTick > 4000) { progressTick = ticks; skip(i, "stuck"); setNext(x, z, scanColumn(x, y + 1, z)); badStand.clear(); return; }
        int r = doCell(i);
        reconcile();
        // placed but not yet in its final form (a slab still to be doubled, a pot still empty ...): keep it on the list
        if (r == PLACED && needs(i)) r = DEFER;
        if (r == DEFER) {
            // after a few vain tries the cell goes to the end of the queue so that the rest can go on
            if (tries.addTo(i, 1) >= 12) { deferred.add(i); blocked.remove(i); setNext(x, z, scanColumn(x, y + 1, z)); }
            else blocked.put(i, surroundings(i));
            return;
        }
        blocked.remove(i);
        setNext(x, z, scanColumn(x, y + 1, z));
        if (r == PLACED && !deferred.isEmpty()) {
            // something that was waiting for this block?
            for (int d = 0; d < 6; d++) {
                int nx = x + DX[d], ny = y + DY[d], nz = z + DZ[d];
                if (!g.in(nx, ny, nz)) continue;
                int n = g.idx(nx, ny, nz);
                if (deferred.contains(n) && needs(n) && !StateInfo.late[g.tgt[n]] && doCell(n) != DEFER) deferred.remove(n);
            }
        }
    }

    private int surroundings(int i) {
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i), h = 17;
        // (temporary blocks do not count as a change worth another try)
        for (int dy = -1; dy <= 1; dy++) for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) {
            int nx = x + dx, ny = y + dy, nz = z + dz;
            h = h * 31 + (g.in(nx, ny, nz) && scaffold.contains(g.idx(nx, ny, nz)) ? 0 : g.get(nx, ny, nz));
        }
        return h;
    }

    private int doCell(int i) {
        checkScaffold();
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        int t = g.tgt[i];
        Spec sp = spec(t);
        if (!sp.placeable) { skip(i, "cannot be placed by hand: " + StateInfo.state[t].getBlock().getDescriptionId().replace("block.minecraft.", "")); return SKIP; }
        int w = g.cur[i];
        if (w != 0) {
            if (scaffold.contains(i)) { if (!removeScaffold(i)) { skip(i, "scaffold in the way"); return SKIP; } }
            else if (sp.multi && StateInfo.state[w].getBlock() == StateInfo.state[t].getBlock()) return topUp(x, y, z, t, sp) ? PLACED : DEFER;
            else if (sp.content != null && w == sp.first) { fillPot(x, y, z, t, sp); if (matches(g.get(x, y, z), t)) return PLACED; skip(i, "pot could not be filled"); return SKIP; }
            else { skip(i, "occupied by another block"); return SKIP; }
        }
        if (count(sp.item) == 0 || (count(scaffoldItem) < 12 && !stranded)) {
            if (stranded && count(sp.item) == 0) { skip(i, "stranded without material"); return SKIP; }
            mustFetch = sp.item;
            if (verbose) log("restock wanted: " + (count(sp.item) == 0 ? "no " + sp.item : "loose blocks low (" + count(scaffoldItem) + ")") + " placed=" + placed + " for " + rel(x, y, z) + " free=" + freeSlots());
            restock(y);
            mustFetch = null;
            if (count(sp.item) == 0) {
                if (verbose) { int inStock = 0, entries = 0; for (Site.Stock st : stock) if (st.item == sp.item) { inStock += st.count; entries++; } log("OUT OF STOCK " + sp.item + ": depot has " + inStock + " in " + entries + " slots; free inventory slots " + freeSlots() + "; " + inventorySummary()); }
                skip(i, stranded ? "stranded without material" : "out of stock"); return SKIP;
            }
        }
        if (!clearBelow(x, y, z) && noRoofing) return DEFER;
        // sand, gravel, anvils ...: only onto something that will stay there
        if (StateInfo.gravity[t]) {
            int below = g.get(x, y - 1, z);
            if (below == 0 || StateInfo.walkThrough[below] || StateInfo.replaceable[below] || (g.in(x, y - 1, z) && scaffold.contains(g.idx(x, y - 1, z)))) return DEFER;
        }
        int r = attempt(x, y, z, t, sp, true);
        if (r == DEFER && !sp.chest) {
            // no face to click that gives the wanted state: lend it one with a temporary block, then take that away
            int helper = placeHelper(x, y, z, sp);
            if (helper >= 0) {
                r = attempt(x, y, z, t, sp, false);
                removeScaffold(helper);
                if (r == PLACED) helped++;
            }
        }
        return r;
    }

    public long helped;

    /** Tries to place the cell from here or from a planned stance. */
    private int attempt(int x, int y, int z, int t, Spec sp, boolean mayEscape) {
        wantHeld = sp.item;
        if (bodyIn(x, y, z)) {
            if (selfPlace(x, y, z, t, sp)) return PLACED;
        } else if (placeFromHere(x, y, z, t, sp)) return PLACED;
        // go somewhere better
        for (int attempt = 0; attempt < 3; attempt++) {
            Stance st = planStance(x, y, z, sp, false);
            if (mayEscape && st == null && dbgClick + dbgPillarClick > 0 && dbgNav == 0 && g.stand16(cx(), cy(), cz()) > 0 && escapeBudget-- > 0 && escapeFromHalfFloor()) {
                unreachable.clear();
                st = planStance(x, y, z, sp, false);
            }
            if (mayEscape && st == null && dbgClick + dbgPillarClick > 0 && dbgNav == 0) {
                if (++shutIn == 3) { unreachable.clear(); st = planStance(x, y, z, sp, false); }
            } else if (st != null) shutIn = 0;
            if (st == null) {
                planFails++;
                if (verbose && (planFails <= 12 || (TRACE && planFails < 400))) {
                    StringBuilder sb = new StringBuilder("no stance for " + rel(x, y, z) + " " + StateInfo.name(t) + " table=" + (sp.table != null) + " neighbours:");
                    for (int d = 0; d < 6; d++) sb.append(' ').append(StateInfo.name(g.get(x + DX[d], y + DY[d], z + DZ[d])));
                    sb.append(" | dbg standable=" + dbgStand + " clickOk=" + dbgClick + " navOk=" + dbgNav + " pillarCols=" + dbgPillar + " pillarClick=" + dbgPillarClick);
                    log(sb.toString());
                    if (sp.table != null && dbgClick == 0 && clickDbgLogs++ < 1) {
                        for (int dz = -1; dz <= 1; dz += 2) {
                            int h = g.stand16(x, y, z + dz);
                            System.out.println("  stance " + rel(x, y, z + dz) + " stand=" + h);
                            if (h < 0) continue;
                            debugClick = true;
                            findClick(x, y, z, sp.first, sp.table, x + 0.5, y + h / 16.0 + 1.62, z + dz + 0.5, false, 0x3f, click);
                            debugClick = false;
                        }
                    }
                }
                return DEFER;
            }
            if (!goTo(st)) { replans++; badStand.add(g.idx(st.x, st.y, st.z)); continue; }
            if (bodyIn(x, y, z)) { if (selfPlace(x, y, z, t, sp)) { badStand.clear(); return PLACED; } }
            else if (placeFromHere(x, y, z, t, sp)) { badStand.clear(); endLean(); return PLACED; }
            endLean();
            badStand.add(g.idx(st.x, st.y, st.z) ^ (st.lean >= 0 ? (st.lean + 1) << 28 : 0));
        }
        return DEFER;
    }

    /** Does clicking the given face of a neighbour ever give the wanted state? */
    private boolean faceWorks(Spec sp, int face) {
        if (sp.table == null) return true;
        int want = StateInfo.loose[sp.first];
        int from = Oracle.index(face, 0, 0, 0, false), to = Oracle.index(face, 3, Oracle.NYAW - 1, Oracle.NPITCH - 1, true);
        for (int k = from; k <= to; k++) { int e = sp.table[k]; if (e != 0 && StateInfo.loose[e] == want) return true; }
        return false;
    }

    /**
     * Puts a scaffold block into an empty neighbouring cell of (x,y,z) whose face would let the wanted block be
     * placed. Returns that cell's index, or -1. Blocks that must rest on something real are not given a helper.
     */
    private int placeHelper(int x, int y, int z, Spec sp) {
        if (!haveScaffold || count(scaffoldItem) == 0 || !oracle.floats(sp.first)) return -1;
        Spec ss = spec(scaffoldState);
        for (int d = 0; d < 6; d++) {
            int nx = x + DX[d], ny = y + DY[d], nz = z + DZ[d];
            if (!g.in(nx, ny, nz) || ny < g.oy || g.get(nx, ny, nz) != 0 || !scaffoldOk(nx, ny, nz)) continue;
            if (!faceWorks(sp, OPP[d])) continue;
            // the helper itself needs something to be placed against
            boolean anchored = false;
            for (int e = 0; e < 6 && !anchored; e++) {
                int ax = nx + DX[e], ay = ny + DY[e], az = nz + DZ[e];
                if (ax == x && ay == y && az == z) continue;
                int w = g.get(ax, ay, az);
                anchored = w != 0 && !StateInfo.replaceable[w] && Ray.hasOutline(w);
            }
            if (!anchored) continue;
            if (attempt(nx, ny, nz, scaffoldState, ss, false) == PLACED) {
                addScaffold(nx, ny, nz);
                return g.idx(nx, ny, nz);
            }
        }
        return -1;
    }

    // ------------------------------------------------------------------ placing from the current position
    /** False during the main part of the build: no stance that cannot be left again, no block that shuts the bot in. */
    private boolean acceptTraps;
    public long trapsAvoided, trapChecks, trapSearches;

    /** Bits for the four sides by which the bot could leave cell (x,y,z) with a plain step, jump or drop; bit 4: head room to jump. */
    private int exits(int x, int y, int z) {
        int h = g.stand16(x, y, z);
        if (h < 0) h = 0;
        boolean headUp = g.passable(x, y + 2, z);
        int m = headUp ? 16 : 0;
        for (int d = 0; d < 4; d++) {
            int nx = x + DXH[d], nz = z + DZH[d];
            int hl = g.stand16(nx, y, nz);
            if (hl >= 0) { if (hl - h <= 9 || (hl - h <= 19 && headUp)) m |= 1 << d; continue; }
            int hu = headUp ? g.stand16(nx, y + 1, nz) : -1;
            if (hu >= 0) { if (16 + hu - h <= 19) m |= 1 << d; continue; }
            if (g.passable(nx, y, nz) && g.passable(nx, y + 1, nz)) m |= 1 << d;
        }
        return m;
    }

    /** Is there a way from cell (x,y,z) back to the supply chests without breaking through the build? */
    private boolean wayOut(int x, int y, int z) {
        trapChecks++;
        Site.Stock first = stock.get(0);
        int ax = first.chestX, ay = g.oy, az = first.chestZ + 1;
        int[] path = java.util.Arrays.copyOf(nav.path, nav.length * 3);
        byte[] move = java.util.Arrays.copyOf(nav.move, nav.length);
        int len = nav.length, cost = nav.goalCost, sm = nav.scaffoldMoves, tm = nav.towerMoves;
        nav.startH = 0;
        Nav.MineRule keep = nav.mine;
        nav.mine = null;
        // cheap test first: plenty of room to walk, climb and drop to from here means not shut in
        towersOff = true;
        boolean ok = nav.search(x, y, z, ax, ay, az, 96, null) || nav.expanded > 96;
        towersOff = false;
        if (!ok) { trapSearches++; ok = nav.search(x, y, z, ax, ay, az, 30000, this::scaffoldOk); }
        nav.mine = keep;
        System.arraycopy(path, 0, nav.path, 0, path.length);
        System.arraycopy(move, 0, nav.move, 0, move.length);
        nav.length = len; nav.goalCost = cost; nav.scaffoldMoves = sm; nav.towerMoves = tm;
        return ok;
    }

    /** Would putting the block into (x,y,z) from where the bot stands close its own way out? */
    private boolean sealsIn(int x, int y, int z, int t) {
        if (acceptTraps || StateInfo.walkThrough[t] || !c.onGround()) return false;
        int bx = cx(), by = cy(), bz = cz();
        if (Math.abs(x - bx) > 2 || Math.abs(z - bz) > 2 || y < by - 1 || y > by + 3 || !g.in(bx, by, bz)) return false;
        int i = g.idx(x, y, z);
        char old = g.cur[i];
        g.cur[i] = (char) t;
        boolean trapped = !wayOut(bx, by, bz);
        g.cur[i] = old;
        if (trapped) trapsAvoided++;
        return trapped;
    }

    @Override protected boolean exactClick(int x, int y, int z, int want, int nx, int ny, int nz, int face, double ax, double ay, double az, float yaw, float pitch, boolean sneak) {
        Item it = StateInfo.item[want];
        if (it == Items.AIR) return true;
        BlockHitResult hit = new BlockHitResult(new net.minecraft.world.phys.Vec3(ax, ay, az), net.minecraft.core.Direction.from3DDataValue(face), new net.minecraft.core.BlockPos(nx, ny, nz), false);
        int pred = oracle.predict(it, hit, yaw, pitch, sneak);
        return pred != 0 && StateInfo.loose[pred] == StateInfo.loose[want] && oracle.placedAt.getX() == x && oracle.placedAt.getY() == y && oracle.placedAt.getZ() == z;
    }

    private boolean placeFromHere(int x, int y, int z, int t, Spec sp) {
        if (sealsIn(x, y, z, sp.first)) return false;
        if (!c.onGround() || hspeed() > 0.003) settle();
        // get the item into the hand first: fetching it from the inventory screen lets go of all keys
        if (!hold(sp.item, !leaning && !c.sneaking())) return false;
        if (tryPlace(x, y, z, t, sp)) return true;
        if (leaning || c.sneaking()) return false;
        // against blocks that react to a click (chests, doors ...) one has to sneak; that also lowers the eyes
        boolean wanted = false;
        for (int d = 0; d < 6 && !wanted; d++) wanted = StateInfo.interactive[g.get(x + DX[d], y + DY[d], z + DZ[d])];
        if (!wanted || onTower()) return false;   // (sneaking on scaffolding means sinking through it)
        c.keys(false, false, false, false, false, true, false);
        tick(); tick();
        boolean ok = tryPlace(x, y, z, t, sp);
        c.releaseKeys();
        tick();
        return ok;
    }

    private boolean tryPlace(int x, int y, int z, int t, Spec sp) {
        var e = c.eye();
        boolean sneak = c.sneaking();
        boolean needsSupport = !oracle.floats(sp.first);
        for (int d = 0; d < 6; d++) {
            // something that has to sit on or hang from a block must not be put on scaffolding: it would fall off with it
            if (needsSupport && g.in(x + DX[d], y + DY[d], z + DZ[d]) && scaffold.contains(g.idx(x + DX[d], y + DY[d], z + DZ[d]))) continue;
            if (!findClick(x, y, z, sp.first, sp.table, e.x, e.y, e.z, sneak, 1 << d, click)) continue;
            if (!hold(sp.item, !leaning && !sneak)) return false;
            // what would really happen? (the bot's picture can be off for odd shapes; the rules decide)
            BlockHitResult hit = oracle.pick(c, click.yaw, click.pitch);
            if (hit == null || hit.getBlockPos().getX() != click.nx || hit.getBlockPos().getY() != click.ny || hit.getBlockPos().getZ() != click.nz
                || hit.getDirection().get3DDataValue() != click.face) {
                if (verbose && tryLogs++ < 40) log("tryPlace " + rel(x, y, z) + ": aimed at " + rel(click.nx, click.ny, click.nz) + " face " + click.face + " but the crosshair is on " + (hit == null ? "nothing" : rel(hit.getBlockPos().getX(), hit.getBlockPos().getY(), hit.getBlockPos().getZ()) + " face " + hit.getDirection()));
                continue;
            }
            int pred = oracle.predict(sp.item, hit, click.yaw, click.pitch, sneak);
            if (pred == 0 || StateInfo.loose[pred] != StateInfo.loose[sp.first] || oracle.placedAt.getX() != x || oracle.placedAt.getY() != y || oracle.placedAt.getZ() != z) {
                if (verbose && tryLogs++ < 40) log("tryPlace " + rel(x, y, z) + ": click on " + rel(click.nx, click.ny, click.nz) + " face " + click.face + " would give " + StateInfo.name(pred) + " at " + rel(oracle.placedAt.getX(), oracle.placedAt.getY(), oracle.placedAt.getZ()) + ", wanted " + StateInfo.name(sp.first) + " yaw=" + click.yaw + " pitch=" + click.pitch + " sneak=" + sneak);
                continue;
            }
            if (sp.chest && StateInfo.canon[pred] != StateInfo.canon[chestWant(x, y, z, t)]) continue;
            c.look(click.yaw, click.pitch);
            c.clickUse();
            if (sneak) c.sneak(true);
            tick();
            int now = g.get(x, y, z);
            if (now == 0) { failedClicks++; continue; }
            placed++;
            if (!sameKind(now, t) && StateInfo.loose[now] != StateInfo.loose[sp.first]) {
                // not what was meant to go here (should not happen): take it out again
                misplaced++;
                log("MISPLACED " + StateInfo.name(now) + " in " + rel(x, y, z) + " wanted " + StateInfo.name(t) + " held=" + heldItem() + " predicted=" + StateInfo.name(pred));
                var e2 = c.eye();
                if (findBreak(x, y, z, e2.x, e2.y, e2.z, click)) breakBlock(x, y, z, click.ax, click.ay, click.az, 400);
                return false;
            }
            if (sp.multi && !matches(now, t)) topUp(x, y, z, t, sp);
            if (sp.content != null) fillPot(x, y, z, t, sp);
            if (!matches(g.get(x, y, z), t) && StateInfo.loose[g.get(x, y, z)] == StateInfo.loose[t]) adjust(x, y, z, t);
            return true;   // (a block is there; if it is not finished yet, doCell comes back to it)
        }
        return false;
    }

    public long misplaced;

    /** Uses the plant on the empty pot just placed. */
    private void fillPot(int x, int y, int z, int t, Spec sp) {
        if (verbose) log("fillPot " + rel(x, y, z) + " have " + count(sp.content) + " of " + sp.content + " now=" + StateInfo.name(g.get(x, y, z)));
        if (matches(g.get(x, y, z), t) || count(sp.content) == 0) return;
        boolean wasSneak = c.sneaking();
        if (!hold(sp.content)) return;
        if (c.sneaking()) { c.releaseKeys(); tick(); tick(); }
        var e = c.eye();
        if (findBreak(x, y, z, e.x, e.y, e.z, click)) {
            lookAt(click.ax, click.ay, click.az);
            c.clickUse();
            tick();
            if (verbose) log("fillPot clicked; now=" + StateInfo.name(g.get(x, y, z)) + " hit=" + (c.lastHit() == null ? null : c.lastHit().getBlockPos()));
        } else if (verbose) log("fillPot: pot not in sight");
        if (wasSneak) { c.keys(false, false, false, false, false, true, false); tick(); tick(); }
    }

    /** Cells where a block appeared since the last look (from the server's block updates). */
    private final IntArrayList appeared = new IntArrayList();
    public long strays;

    /**
     * Anything that turned up where it does not belong - a block in a cell the target leaves empty, or of another
     * kind than the target wants - and is not known scaffolding is treated like scaffolding from now on: removed
     * before the right block goes in, or at the end. (Sand that slid, a click that landed on another face ...)
     */
    private void reconcile() {
        if (appeared.isEmpty()) return;
        for (int k = 0; k < appeared.size(); k++) {
            int i = appeared.getInt(k);
            int w = g.cur[i];
            if (w == 0 || scaffold.contains(i)) continue;
            int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
            if (y < g.oy) continue;
            int t = g.tgt[i];
            boolean wrong = t == 0 ? !isSupplyChest(x, y, z) : !sameKind(w, t) && !sameFamily(w, t) && w != spec(t).first;
            if (!wrong) continue;
            strays++;
            if (verbose && strays < 20) log("STRAY " + StateInfo.name(w) + " at " + rel(x, y, z) + " (target " + StateInfo.name(t) + ")");
            scaffold.add(i);
            scaffoldInColumn[col(x, z)]++;
        }
        appeared.clear();
    }

    /** Same block as far as judging goes (a filled cauldron counts as a cauldron ...). */
    private static boolean sameKind(int a, int b) {
        return StateInfo.state[StateInfo.canon[a]].getBlock() == StateInfo.state[StateInfo.canon[b]].getBlock();
    }

    private boolean isSupplyChest(int x, int y, int z) { return z < g.oz - 2 && StateInfo.state[g.get(x, y, z)].getBlock() instanceof net.minecraft.world.level.block.ChestBlock; }

    /** Blocks that turn into one another by themselves and need not be told apart here. */
    private static boolean sameFamily(int a, int b) {
        String na = StateInfo.state[a].getBlock().getDescriptionId(), nb = StateInfo.state[b].getBlock().getDescriptionId();
        return na.replace("wall_", "").equals(nb.replace("wall_", "")) || na.replace("_plant", "").equals(nb.replace("_plant", ""));
    }

    /** Right-clicks a placed block until its click-settable properties match (repeater delay, open trapdoor ...). */
    private void adjust(int x, int y, int z, int t) {
        if (StateInfo.state[t].getBlock() instanceof net.minecraft.world.level.block.ChestBlock) return; // halves join by themselves
        boolean wasSneak = c.sneaking();
        if (wasSneak) { c.releaseKeys(); tick(); tick(); }
        for (int n = 0; n < 26; n++) {
            int now = g.get(x, y, z);
            if (now == 0 || matches(now, t) || StateInfo.loose[now] != StateInfo.loose[t]) break;
            var e = c.eye();
            if (!findBreak(x, y, z, e.x, e.y, e.z, click)) break;
            lookAt(click.ax, click.ay, click.az);
            c.clickUse();
            tick();
            if (g.get(x, y, z) == now) { tick(); if (g.get(x, y, z) == now) break; }
        }
    }

    /** Further clicks on a block already there until it has the wanted count (second slab half, more snow layers ...). */
    private boolean topUp(int x, int y, int z, int t, Spec sp) {
        for (int n = 0; n < 8; n++) {
            int now = g.get(x, y, z);
            if (matches(now, t)) return true;
            if (now == 0 || count(sp.item) == 0 || !hold(sp.item)) return false;
            if (topUpClick(x, y, z, t, sp, now)) continue;
            // not from here: look for a place from which the top of the block can be seen
            planTopUp = t;
            Stance st;
            try { st = planStance(x, y, z, sp, true); } finally { planTopUp = 0; }
            if (st == null || !goTo(st)) { if (TRACE && verbose && failLogs++ < 300) log("topUp " + rel(x, y, z) + ": " + (st == null ? "no stance" : "stance " + rel(st.x, st.y, st.z) + " not reached")); return false; }
            if (!hold(sp.item) || !topUpClick(x, y, z, t, sp, now)) { if (TRACE && verbose && failLogs++ < 300) log("topUp " + rel(x, y, z) + ": no click from stance " + rel(st.x, st.y, st.z) + " eye=" + String.format("%.2f,%.2f,%.2f", c.eye().x - g.ox, c.eye().y - g.oy, c.eye().z - g.oz)); return false; }
        }
        return matches(g.get(x, y, z), t);
    }

    /** While planning where to stand for a further click on a block already there: the wanted final state (else 0). */
    private int planTopUp;

    /** Could a click from the given eye position add to the block in (x,y,z) (second slab half, another snow layer ...)? */
    private boolean topUpVisible(int x, int y, int z, int t, Spec sp, double ex, double ey, double ez) {
        int now = g.get(x, y, z);
        if (now == 0) return false;
        // 1. on the block itself, from above
        double ay = y + (StateInfo.anyTop[now] == 0 ? 0.1 : StateInfo.anyTop[now] / 16.0);
        if (ey > ay + 0.05) {
            for (int k = 0; k < 9; k++) {
                double ax = x + 0.5 + (k % 3 - 1) * 0.3, az = z + 0.5 + (k / 3 - 1) * 0.3;
                double dx = ax - ex, dy = ay - ey, dz = az - ez, hd = Math.sqrt(dx * dx + dz * dz);
                if (Math.sqrt(dx * dx + dy * dy + dz * dz) > reach - 0.1) continue;
                if (!ray.cast(g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, reach) || ray.hx != x || ray.hy != y || ray.hz != z || ray.face != 1) continue;
                float yaw = hd < 1e-6 ? 0f : yawTo(dx, dz), pitch = (float) -Math.toDegrees(Math.atan2(dy, hd));
                BlockHitResult hit = new BlockHitResult(new net.minecraft.world.phys.Vec3(ax, ay, az), net.minecraft.core.Direction.UP, new net.minecraft.core.BlockPos(x, y, z), false);
                int pred = oracle.predict(sp.item, hit, yaw, pitch, false);
                if (pred != 0 && pred != now && StateInfo.state[pred].getBlock() == StateInfo.state[t].getBlock() && oracle.placedAt.getX() == x && oracle.placedAt.getY() == y && oracle.placedAt.getZ() == z) return true;
            }
        }
        // 2. on a neighbour's face that looks into the cell
        for (int d = 0; d < 6; d++) {
            if (!findClick(x, y, z, t, null, ex, ey, ez, false, 1 << d, click)) continue;
            BlockHitResult hit = new BlockHitResult(new net.minecraft.world.phys.Vec3(click.ax, click.ay, click.az), net.minecraft.core.Direction.from3DDataValue(click.face), new net.minecraft.core.BlockPos(click.nx, click.ny, click.nz), false);
            int pred = oracle.predict(sp.item, hit, click.yaw, click.pitch, false);
            if (pred != 0 && pred != now && StateInfo.state[pred].getBlock() == StateInfo.state[t].getBlock() && oracle.placedAt.getX() == x && oracle.placedAt.getY() == y && oracle.placedAt.getZ() == z) return true;
        }
        return false;
    }

    private boolean topUpClick(int x, int y, int z, int t, Spec sp, int now) {
        if (!c.onGround() || hspeed() > 0.003) settle();
        var e = c.eye();
        // 1. on the block itself
        for (int k = 0; k < 9; k++) {
            double ax = x + 0.5 + (k % 3 - 1) * 0.3, az = z + 0.5 + (k / 3 - 1) * 0.3, ay = y + StateInfo.anyTop[now] / 16.0;
            if (StateInfo.anyTop[now] == 0) ay = y + 0.1;
            double dx = ax - e.x, dy = ay - e.y, dz = az - e.z, hd = Math.sqrt(dx * dx + dz * dz);
            if (Math.sqrt(dx * dx + dy * dy + dz * dz) > reach - 0.1) continue;
            float yaw = hd < 1e-6 ? c.yaw() : yawTo(dx, dz), pitch = (float) -Math.toDegrees(Math.atan2(dy, hd));
            if (topUpTry(x, y, z, t, sp, now, yaw, pitch)) return true;
        }
        // 2. on a neighbour's face that looks into the cell (how a second slab half goes in from the side)
        for (int d = 0; d < 6; d++) {
            if (!findClick(x, y, z, t, null, e.x, e.y, e.z, c.sneaking(), 1 << d, click)) continue;
            if (topUpTry(x, y, z, t, sp, now, click.yaw, click.pitch)) return true;
        }
        return false;
    }

    private boolean topUpTry(int x, int y, int z, int t, Spec sp, int now, float yaw, float pitch) {
        BlockHitResult hit = oracle.pick(c, yaw, pitch);
        if (hit == null) return false;
        if (StateInfo.interactive[g.get(hit.getBlockPos().getX(), hit.getBlockPos().getY(), hit.getBlockPos().getZ())] && !c.sneaking()) return false;
        int pred = oracle.predict(sp.item, hit, yaw, pitch, c.sneaking());
        if (pred == 0 || pred == now || oracle.placedAt.getX() != x || oracle.placedAt.getY() != y || oracle.placedAt.getZ() != z) return false;
        if (StateInfo.state[pred].getBlock() != StateInfo.state[t].getBlock()) return false;
        c.look(yaw, pitch);
        c.clickUse();
        if (c.sneaking()) c.sneak(true);
        tick();
        if (g.get(x, y, z) == now) return false;
        placed++;
        return true;
    }

    private boolean selfPlaceable(int x, int y, int z, Spec sp) {
        // only for blocks one ends up standing on: anything thinner would leave the bot standing inside it
        if (!StateInfo.fullCube[sp.first] || sp.multi) return false;
        if (g.stand16(x, y, z) != 0 || !g.passable(x, y + 2, z)) return false;
        int below = g.get(x, y - 1, z);
        if (!Ray.fullOutline(below) || StateInfo.interactive[below] || StateInfo.replaceable[below]) return false;
        if (sp.table == null) return true;
        for (int yc = 0; yc < Oracle.NYAW; yc++) {
            boolean ok = true;
            for (int q = 0; q < 4 && ok; q++) {
                int st = sp.table[Oracle.index(1, q, yc, Oracle.pitchClass(90), false)];
                if (st == 0 || StateInfo.loose[st] != StateInfo.loose[sp.first]) ok = false;
            }
            if (ok) return true;
        }
        return false;
    }

    /** The bot stands in the cell: jump and place the block underfoot, if the block can be placed looking straight down. */
    private boolean selfPlace(int x, int y, int z, int t, Spec sp) {
        if (!StateInfo.fullCube[sp.first] || sp.multi) return false;
        if (cy() != y || g.stand16(x, y, z) != 0 || !g.passable(x, y + 2, z)) return false;
        int below = g.get(x, y - 1, z);
        if (!Ray.fullOutline(below) || StateInfo.interactive[below] || StateInfo.replaceable[below]) return false;
        // which way to face? (for blocks that take their direction from the player)
        float yaw = c.yaw();
        if (sp.table != null) {
            int found = -1;
            for (int yc = 0; yc < Oracle.NYAW && found < 0; yc++) {
                boolean ok = true;
                for (int q = 0; q < 4 && ok; q++) {
                    int st = sp.table[Oracle.index(1, q, yc, Oracle.pitchClass(90), false)];
                    if (st == 0 || StateInfo.loose[st] != StateInfo.loose[sp.first]) ok = false;
                }
                if (ok) found = yc;
            }
            if (found < 0) return false;
            yaw = Oracle.yawCentre(found);
        }
        if (!hold(sp.item) || !centre(x, y, z)) return false;
        settle();
        double y0 = c.y();
        c.look(yaw, 90);
        c.keys(false, false, false, false, true, false, false);
        boolean ok = false;
        for (int n = 0; n < 14; n++) {
            if (!ok && c.y() >= y0 + 1.0 && g.get(x, y, z) == 0) {
                BlockHitResult hit = oracle.pick(c, yaw, 90);
                if (hit != null && hit.getBlockPos().getX() == x && hit.getBlockPos().getY() == y - 1 && hit.getBlockPos().getZ() == z) {
                    int pred = oracle.predict(sp.item, hit, yaw, 90, false);
                    if (pred != 0 && StateInfo.loose[pred] == StateInfo.loose[sp.first]) c.clickUse();
                }
            }
            tick();
            if (n == 0) c.jump(false);
            if (g.get(x, y, z) != 0) ok = true;
            if (c.onGround() && n > 2) break;
        }
        c.releaseKeys();
        if (ok) {
            placed++;
            settle();
            if (sp.multi) topUp(x, y, z, t, sp);
            if (!matches(g.get(x, y, z), t) && StateInfo.loose[g.get(x, y, z)] == StateInfo.loose[t]) adjust(x, y, z, t);
        }
        return ok && g.get(x, y, z) != 0;
    }

    // ------------------------------------------------------------------ choosing where to stand
    private static final class Stance {
        int x, y, z;       // the cell to stand in
        int lean = -1;     // 0..3: sneak to the edge towards DXH/DZH[lean] before clicking
        boolean moss;      // to be reached with loose blocks only (no scaffolding column: it will be leant from)
        int column;        // height of the temporary column the stance stands on (0 = none)
        boolean cap;       // stands on one loose block put on top of a scaffolding column
        int cost;
    }

    private static final int[] DXH = {1, -1, 0, 0}, DZH = {0, 0, 1, -1};
    // offsets around a cell, nearest first
    private static final int[][] RING;
    static {
        List<int[]> l = new ArrayList<>();
        for (int dx = -4; dx <= 4; dx++) for (int dz = -4; dz <= 4; dz++) if (dx * dx + dz * dz <= 17) l.add(new int[] {dx, dz});
        l.sort((a, b) -> (a[0] * a[0] + a[1] * a[1]) - (b[0] * b[0] + b[1] * b[1]));
        RING = l.toArray(new int[0][]);
    }
    private static final int[] DYS = {0, 1, -1, -2, 2, -3, -4};
    private static final double PLAN_REACH = 4.15;

    /**
     * Where to stand to place (or, with breaking=true, to break) cell (x,y,z). Tries places that exist first,
     * then leaning over the edge of a neighbouring block, then a temporary pillar next to the cell.
     */
    private int dbgStand, dbgClick, dbgNav, dbgPillar, dbgPillarClick;
    private final int[] candX = new int[64], candY = new int[64], candZ = new int[64], candD = new int[64];

    private Stance planStance(int x, int y, int z, Spec sp, boolean breaking) {
        plans++;
        dbgStand = dbgClick = dbgNav = dbgPillar = dbgPillarClick = 0;
        Stance best = null;
        int found = 0;
        reach = PLAN_REACH;
        try {
            // 0. on top of the block below the cell itself, placing underfoot with a jump
            if (!breaking && selfPlaceable(x, y, z, sp) && !badStand.contains(g.idx(x, y, z)) && Math.abs(cx() - x) + Math.abs(cz() - z) <= 3 && cy() >= y - 1
                && route(x, y, z, 600)) {
                best = new Stance(); best.x = x; best.y = y; best.z = z; best.cost = nav.goalCost;
                nSelf++;
                return best;
            }
            // 1. somewhere one can already stand: collect the spots that would do, then take the first that can be reached,
            //    trying those nearest to the bot first
            int nc = 0;
            int bx = cx(), by = cy(), bz = cz();
            boolean touchesInteractive = false;
            for (int d = 0; d < 6 && !touchesInteractive; d++) touchesInteractive = StateInfo.interactive[g.get(x + DX[d], y + DY[d], z + DZ[d])];
            for (int[] o : RING) {
                int nx = x + o[0], nz = z + o[1];
                for (int dy : DYS) {
                    int ny = y + dy;
                    if (nx == x && nz == z && (ny == y || ny == y - 1)) continue;
                    int h = g.stand16(nx, ny, nz);
                    if (h < 0 || badStand.contains(g.idx(nx, ny, nz))) continue;
                    dbgStand++;
                    double ex = nx + 0.5, ey = ny + h / 16.0 + 1.62, ez = nz + 0.5;
                    boolean ok = breaking ? (planTopUp != 0 ? topUpVisible(x, y, z, planTopUp, sp, ex, ey, ez) : findBreak(x, y, z, ex, ey, ez, click)) : findClick(x, y, z, sp.first, sp.table, ex, ey, ez, false, 0x3f, click);
                    // against chests, doors and other things that react to a click one sneaks (eyes 0.35 lower)
                    if (!ok && !breaking && touchesInteractive && !g.isTower(nx, ny - 1, nz)) ok = findClick(x, y, z, sp.first, sp.table, ex, ey - 0.35, ez, true, 0x3f, click);
                    if (!ok) continue;
                    dbgClick++;
                    if (nc < candX.length) {
                        candX[nc] = nx; candY[nc] = ny; candZ[nc] = nz;
                        candD[nc] = Math.abs(nx - bx) + Math.abs(nz - bz) + 3 * Math.max(0, ny - by);
                        nc++;
                    }
                }
            }
            for (int tries = 0; tries < 4 && tries < nc && best == null; tries++) {
                int k = -1;
                for (int j = 0; j < nc; j++) if (candD[j] >= 0 && (k < 0 || candD[j] < candD[k])) k = j;
                if (k < 0) break;
                candD[k] = -1;
                if (!route(candX[k], candY[k], candZ[k], 6000)) continue;
                dbgNav++;
                best = new Stance(); best.x = candX[k]; best.y = candY[k]; best.z = candZ[k]; best.cost = nav.goalCost;
            }
            if (best != null) { nNatural++; return best; }
            // 2. on top of a block beside the cell, leaning out over it
            if (!breaking) {
                for (int d = 0; d < 4; d++) {
                    int nx = x + DXH[d], nz = z + DZH[d];
                    if (!g.solidCube(nx, y, nz) || g.stand16(nx, y + 1, nz) != 0) continue;
                    if (badStand.contains(g.idx(nx, y + 1, nz) ^ ((d ^ 1) + 1) << 28)) continue;
                    // leaning from (nx,nz) towards the cell means moving in the direction opposite to d
                    double ex = nx + 0.5 - DXH[d] * 0.75, ez = nz + 0.5 - DZH[d] * 0.75, ey = y + 1 + 1.27;
                    if (!findClick(x, y, z, sp.first, sp.table, ex, ey, ez, true, 0x3c, click)) continue;
                    if (!route(nx, y + 1, nz, 6000)) continue;
                    if (best == null || nav.goalCost < best.cost) { best = new Stance(); best.x = nx; best.y = y + 1; best.z = nz; best.lean = d ^ 1; best.cost = nav.goalCost; }
                }
                if (best != null) { nLean++; return best; }
            }
            // 3. a temporary column near the cell: scaffolding to climb if possible, else a pillar of loose blocks
            for (int[] o : RING) {
                if (o[0] == 0 && o[1] == 0) continue;
                if (o[0] * o[0] + o[1] * o[1] > 8) break;
                int qx = x + o[0], qz = z + o[1];
                if (!g.in(qx, y, qz)) continue;
                boolean adjacent = Math.abs(o[0]) + Math.abs(o[1]) == 1;
                for (int top = adjacent ? y : y - 1; top >= y - 2; top--) {  // top = y of the highest block of the column
                    if (!g.passable(qx, top + 1, qz) || !g.passable(qx, top + 2, qz)) continue;
                    if (badStand.contains(g.idx(qx, top + 1, qz))) continue;
                    for (int variant = haveTower ? 0 : 1; variant < 2; variant++) {
                        boolean tw = variant == 0;
                        // find the base of the column
                        int base = top;
                        while (base >= g.y0 && g.isAir(qx, base, qz) && (tw ? towerOk(qx, base, qz) : scaffoldOk(qx, base, qz))) base--;
                        if (base == top || base < g.y0) continue;
                        int floor = g.get(qx, base, qz);
                        if (tw) {
                            if (!StateInfo.scaffolding[floor] && (!StateInfo.sturdyTop[floor] || StateInfo.interactive[floor] || scaffold.contains(g.idx(qx, base, qz)))) continue;
                        } else if (!Nav.flatTop(floor)) continue;
                        // loose blocks all the way, or (better) a scaffolding column with one loose block on top to sneak on
                        boolean cap = !tw && haveTower && scaffoldOk(qx, top, qz) && (StateInfo.scaffolding[floor] || (top - base >= 2 && StateInfo.sturdyTop[floor] && !StateInfo.interactive[floor] && !scaffold.contains(g.idx(qx, base, qz))));
                        if (cap) for (int yy = base + 1; yy < top && cap; yy++) cap = towerOk(qx, yy, qz);
                        if (!tw && !cap && StateInfo.scaffolding[floor]) continue;
                        dbgPillar++;
                        // pretend the column is there
                        for (int yy = base + 1; yy <= top; yy++) g.cur[g.idx(qx, yy, qz)] = (char) (tw || (cap && yy < top) ? towerState : scaffoldState);
                        boolean ok;
                        int lean = -1;
                        double ex = qx + 0.5, ey = top + 1 + 1.62, ez = qz + 0.5;
                        ok = breaking ? (planTopUp != 0 ? topUpVisible(x, y, z, planTopUp, sp, ex, ey, ez) : findBreak(x, y, z, ex, ey, ez, click)) : findClick(x, y, z, sp.first, sp.table, ex, ey, ez, false, 0x3f, click);
                        if (!ok && !tw && !breaking && touchesInteractive) ok = findClick(x, y, z, sp.first, sp.table, ex, ey - 0.35, ez, true, 0x3f, click);
                        if (!ok && !tw && !breaking && adjacent && top == y) {
                            int d = o[0] == 1 ? 0 : o[0] == -1 ? 1 : o[1] == 1 ? 2 : 3;   // direction from the cell to the pillar
                            ex = qx + 0.5 - DXH[d] * 0.75; ez = qz + 0.5 - DZH[d] * 0.75; ey = top + 1 + 1.27;
                            ok = findClick(x, y, z, sp.first, sp.table, ex, ey, ez, true, 0x3c, click);
                            lean = d ^ 1;
                        }
                        for (int yy = base + 1; yy <= top; yy++) g.cur[g.idx(qx, yy, qz)] = 0;
                        if (!ok) continue;
                        dbgPillarClick++;
                        towersOff = !tw && !cap;
                        nav.extraEffort = 400 * (top - base);
                        boolean reach2;
                        try { reach2 = cap ? route(qx, top, qz, 20000) : route(qx, top + 1, qz, 20000); } finally { towersOff = false; nav.extraEffort = 0; }
                        if (!reach2) { if (TRACE && verbose && failLogs++ < 200) log("column stance " + rel(qx, top + 1, qz) + (tw ? " tower" : " loose") + " for " + rel(x, y, z) + ": no route, expanded=" + nav.expanded + " base=" + (base - g.oy) + " unreachableCached=" + unreachable.contains(g.idx(qx, top + 1, qz)) + " haveScaffold=" + haveScaffold + " count=" + count(scaffoldItem)); continue; }
                        Stance s = new Stance();
                        s.x = qx; s.y = top + 1; s.z = qz; s.lean = lean; s.cost = nav.goalCost; s.moss = !tw && !cap; s.cap = cap; s.column = top - base;
                        nPillarStance++;
                        return s;
                    }
                }
            }
        } finally {
            reach = 4.5;
        }
        return null;
    }

    private int layerNow;
    /** Destinations that could not be reached from (about) where the bot is now; forgotten when it moves or builds. */
    private final IntOpenHashSet unreachable = new IntOpenHashSet();
    private int unreachableFrom = -1;
    private long unreachableAt = -1;

    private boolean search(int x, int y, int z, int maxExpand) {
        if (!c.onGround()) settle();   // (in the air, or sliding down a ladder: wait until standing)
        nav.startH = c.onGround() ? Math.max(0, Math.min(15, (int) Math.round((c.y() - cy()) * 16))) : -1;
        nav.towersOn = haveTower && !towersOff && !noScaffoldNav;
        boolean ok = nav.search(cx(), cy(), cz(), x, y, z, maxExpand, this::scaffoldOk);
        if (!ok && nav.expanded <= 1 && !(cx() == x && cy() == y && cz() == z) && unstick()) {
            nav.startH = c.onGround() ? Math.max(0, Math.min(15, (int) Math.round((c.y() - cy()) * 16))) : -1;
            ok = nav.search(cx(), cy(), cz(), x, y, z, maxExpand, this::scaffoldOk);
        }
        return ok;
    }

    public long unstuck;
    private long lastUnstick = -1000;

    /**
     * No step can even be planned from where the bot is (wedged against something that fell beside it, standing
     * on an odd shape ...): shuffle and hop about until it stands in a different, ordinary spot.
     */
    private boolean unstick() {
        if (ticks - lastUnstick < 200) return false;
        lastUnstick = ticks;
        int x0 = cx(), y0 = cy(), z0 = cz();
        for (int round = 0; round < 2; round++) {
            for (int d = 0; d < 4; d++) {
                c.look(yawTo(DXH[d], DZH[d]), 0);
                for (int n = 0; n < 10; n++) { c.keys(true, false, false, false, round == 1 || n > 3, false, false); tick(); }
                c.releaseKeys();
                settle();
                if ((cx() != x0 || cy() != y0 || cz() != z0) && g.stand16(cx(), cy(), cz()) >= 0) {
                    unstuck++;
                    nav.blockedEdges.clear();
                    unreachable.clear();
                    if (verbose) log("unstuck: moved from " + rel(x0, y0, z0));
                    return true;
                }
            }
        }
        nav.blockedEdges.clear();
        unreachable.clear();
        return true;
    }

    public long mined;
    /** During the last clean-up rounds: scaffolding that got walled in may be reached by breaking through the build. */
    private boolean forceMine;
    /** Counts cells in a row for which places to stand existed but none could be reached: the bot is shut in. */
    private int shutIn;

    private int mineTicks(int x, int y, int z) {
        if (!g.in(x, y, z)) return -1;
        int w = g.get(x, y, z);
        if (w == 0) return 0;
        if (StateInfo.gravity[g.get(x, y + 1, z)]) return -1;
        if (z < g.oz - 2 && StateInfo.state[w].getBlock() instanceof net.minecraft.world.level.block.ChestBlock) return -1;
        int t = breakTicks(w);
        return t < 0 || t > 500 ? -1 : t;
    }

    private boolean route(int x, int y, int z, int maxExpand) {
        int here = g.in(cx(), cy(), cz()) ? g.idx(cx(), cy(), cz()) : -1;
        if (here != unreachableFrom || placed - unreachableAt > 30) { unreachable.clear(); unreachableFrom = here; unreachableAt = placed; }
        int goal = g.idx(x, y, z);
        if (unreachable.contains(goal)) return false;
        if (search(x, y, z, maxExpand)) return true;
        if (shutIn >= 3 || forceMine) {
            // last resort: break out through the build itself
            nav.mine = this::mineTicks;
            boolean ok = search(x, y, z, maxExpand);
            nav.mine = null;
            if (ok) return true;
        }
        unreachable.add(goal);
        return false;
    }

    /** Temporary blocks may go where the target has nothing, or where its turn has not come yet (they are removed before it does). */
    private boolean scaffoldOk(int x, int y, int z) {
        if (!g.in(x, y, z) || !haveScaffold || noScaffoldNav) return false;
        if (g.tgt[g.idx(x, y, z)] == 0) return true;
        if (!g.inBox(x, y, z) || colNext == null) return false;
        return layerNow == Integer.MAX_VALUE || y > colNext[col(x, z)];
    }

    /** False once the bot carries no scaffolding and cannot get more: it then plans without temporary blocks. */
    private boolean haveScaffold = true;
    /** Set while looking for a route that must do without new scaffolding (the bot carries too little). */
    private boolean noScaffoldNav;

    private void checkScaffold() {
        boolean have = count(scaffoldItem) > 0;
        if (!have && !stranded) for (Site.Stock st : stock) if (st.item == scaffoldItem && st.count > 0) { have = true; break; }
        if (have != haveScaffold) { haveScaffold = have; unreachable.clear(); log(have ? "scaffolding available again" : "out of scaffolding"); }
        boolean tw = count(towerItem) > 0;
        if (!tw && !stranded) for (Site.Stock st : stock) if (st.item == towerItem && st.count > 0) { tw = true; break; }
        if (tw != haveTower) { haveTower = tw; unreachable.clear(); }
    }

    private boolean haveTower = true;
    /** Set while only walking and climbing are wanted (taking the rope in). */
    private boolean towersOff;

    /** Where a scaffolding column may stand: wherever the target has nothing. (It may be built over: it comes down from its foot.) */
    private boolean towerOk(int x, int y, int z) {
        if (!g.in(x, y, z) || !haveTower || noScaffoldNav || towersOff) return false;
        return g.tgt[g.idx(x, y, z)] == 0;
    }

    private boolean shortOfScaffold() {
        return (nav.scaffoldMoves > 0 && count(scaffoldItem) < nav.scaffoldMoves) || nav.towerMoves > count(towerItem);
    }

    // ------------------------------------------------------------------ moving
    private boolean leaning;

    /**
     * The scaffolding the bot is "hanging on": the pillar and bridge blocks it placed to get where it is, in order.
     * Each entry: {move, from x,y,z, to x,y,z, scaffold cell}. Before the bot goes anywhere else it takes them
     * away again in reverse order, so temporary blocks never pile up behind it.
     */
    private final ArrayList<int[]> rope = new ArrayList<>();
    public long ropeUndone, ropeOrphaned;

    private void undoRope() {
        String old = enter("undo");
        towersOff = true;
        try { undoRopeImpl(); } finally { activity = old; towersOff = false; }
    }

    private void undoRopeImpl() {
        endLean();
        while (!rope.isEmpty()) {
            int[] e = rope.get(rope.size() - 1);
            int cell = e[7];
            if (!scaffold.contains(cell)) { rope.remove(rope.size() - 1); continue; }
            int bx = g.xOf(cell), by = g.yOf(cell), bz = g.zOf(cell);
            // get to the far end of this piece (walking or dropping only)
            boolean there = cx() == e[4] && cy() == e[5] && cz() == e[6] && c.onGround();
            if (!there) {
                nav.startH = c.onGround() ? Math.max(0, Math.min(15, (int) Math.round((c.y() - cy()) * 16))) : -1;
                there = nav.search(cx(), cy(), cz(), e[4], e[5], e[6], 3000, null) && travel();
            }
            boolean ok = false;
            if (there) {
                if (e[0] == Nav.PILLAR) {
                    ok = breakUnderfoot(e[4], e[5], e[6], bx, by, bz);
                } else {
                    one[0] = e[1]; one[1] = e[2]; one[2] = e[3];
                    if (g.stand16(e[1], e[2], e[3]) >= 0 && follow(one, 0, 1)) {
                        settle();
                        var eye = c.eye();
                        ok = findBreak(bx, by, bz, eye.x, eye.y, eye.z, click) && breakBlock(bx, by, bz, click.ax, click.ay, click.az, 80);
                    }
                    // the way back was built over: take the piece away from under the feet instead
                    if (!ok && scaffold.contains(cell) && cx() == bx && cy() == by + 1 && cz() == bz && c.onGround()) ok = breakUnderfoot(bx, by + 1, bz, bx, by, bz);
                }
            }
            if (!ok && scaffold.contains(cell) && c.onGround()) {
                // out of the way but perhaps still within reach from here
                settle();
                var eye = c.eye();
                ok = findBreak(bx, by, bz, eye.x, eye.y, eye.z, click) && breakBlock(bx, by, bz, click.ax, click.ay, click.az, 80);
                if (ok) { for (int n = 0; n < 60 && !c.onGround(); n++) tick(); settle(); }
            }
            if (!ok && scaffold.contains(cell)) { if (verbose) log("rope: could not take down " + rel(bx, by, bz) + " kind=" + e[0] + " pieces=" + rope.size() + " there=" + there); break; }
            ropeUndone++;
            rope.remove(rope.size() - 1);
        }
        if (!rope.isEmpty()) { ropeOrphaned += rope.size(); rope.clear(); }   // could not be undone now: the final clean-up gets them
    }

    private boolean breakUnderfoot(int sx, int sy, int sz, int bx, int by, int bz) {
        boolean ok = centre(sx, sy, sz) && breakBlock(bx, by, bz, bx + 0.5, by + 1, bz + 0.5, 80);
        for (int n = 0; n < 200 && !c.onGround(); n++) tick();
        settle();
        return ok;
    }

    /** Before leaving a spot: place whatever else is allowed and within reach from here. */
    private void sweepHere() {
        if (colNext == null || sweeping) return;
        sweeping = true;
        try {
            boolean again = true;
            int rounds = 0;
            while (again && rounds++ < 40) {
                again = false;
                int bx = cx(), by = cy(), bz = cz();
                for (int dz = -4; dz <= 4; dz++) for (int dx = -4; dx <= 4; dx++) {
                    int x = bx + dx, z = bz + dz;
                    if (x < g.ox || z < g.oz || x >= g.ox + g.sx || z >= g.oz + g.sz) continue;
                    int y = colNext[col(x, z)];
                    if (y == NONE || y > by + 5 || y < by - 5 || !eligible(x, z)) continue;
                    int i = g.idx(x, y, z);
                    int t = g.tgt[i];
                    Spec sp = spec(t);
                    if (!sp.placeable || g.cur[i] != 0 || count(sp.item) == 0 || bodyIn(x, y, z)) continue;
                    if (scaffoldInColumn[col(x, z)] != 0 && looseBelow(x, y, z)) continue;
                    if (placeFromHere(x, y, z, t, sp)) {
                        blocked.remove(i);
                        setNext(x, z, scanColumn(x, y + 1, z));
                        again = true;
                    }
                }
            }
        } finally {
            sweeping = false;
        }
    }
    private boolean sweeping;

    private boolean goTo(Stance st) {
        endLean();
        int gy = st.cap ? st.y - 1 : st.y;   // with a cap: first to the top of the scaffolding column
        if (st.cap && cx() == st.x && cy() == st.y && cz() == st.z && c.onGround() && scaffold.contains(g.idx(st.x, st.y - 1, st.z)) && !g.isTower(st.x, st.y - 1, st.z)) gy = st.y;
        if (!(cx() == st.x && cy() == gy && cz() == st.z && c.onGround())) {
            sweepHere();   // first everything that can be done from where the bot already stands
            if (!rope.isEmpty()) {
                // Growing the pillar the bot stands on is fine; anything else means taking the scaffolding down first
                // (hopping from pillar to pillar leaves pieces behind that are hard to get back to).
                int[] top = rope.get(rope.size() - 1);
                boolean keep = !st.cap && top[0] == Nav.PILLAR && st.x == top[4] && st.z == top[6] && st.y > top[5] && cx() == top[4] && cz() == top[6];
                if (!keep) undoRope();
            }
            towersOff = st.moss;
            nav.extraEffort = st.column * 400;
            try {
            if (!route(st.x, gy, st.z, 20000)) return false;
            // do not set out on a climb without the blocks for it
            if (((nav.scaffoldMoves > 0 && count(scaffoldItem) < nav.scaffoldMoves + 4) || nav.towerMoves > count(towerItem)) && !stranded && restockWorthwhile()) {
                if (verbose) log("restock wanted: route needs " + nav.scaffoldMoves + " loose / " + nav.towerMoves + " scaffolding, have " + count(scaffoldItem) + " / " + count(towerItem));
                restock(layerNow == Integer.MAX_VALUE ? g.oy + g.sy : layerNow);
                if (!route(st.x, gy, st.z, 20000)) return false;
            }
            if (!acceptTraps && !restocking) {
                boolean oneWay = false;
                for (int k = 1; k < nav.length && !oneWay; k++) oneWay = nav.move[k] == Nav.DROP || (nav.move[k] == Nav.WALK && nav.path[k * 3 + 1] < nav.path[k * 3 - 2]);
                if (oneWay && !wayOut(st.x, gy, st.z)) { trapsAvoided++; return false; }
            }
            if (!travel()) return false;
            } finally { towersOff = false; nav.extraEffort = 0; }
        }
        if (st.cap && cy() == st.y - 1) {
            // the loose block on top of the column
            if (count(scaffoldItem) == 0 || g.get(st.x, st.y - 1, st.z) != 0 || !g.isTower(st.x, st.y - 2, st.z)) return false;
            if (!pillar(st.x, st.y - 1, st.z)) return false;
            rope.add(new int[] {Nav.PILLAR, st.x, st.y - 1, st.z, st.x, st.y, st.z, g.idx(st.x, st.y - 1, st.z)});
        }
        if (st.lean >= 0) {
            if (wantHeld != null && !hold(wantHeld)) return false;   // while there is still a free hand for the inventory
            return lean(st.x, st.y, st.z, st.lean);
        }
        return true;
    }

    private boolean restockWorthwhile() {
        for (Site.Stock st : stock) if (st.count > 0 && ((st.item == scaffoldItem && nav.scaffoldMoves > 0 && count(scaffoldItem) < nav.scaffoldMoves + 4) || (st.item == towerItem && nav.towerMoves > count(towerItem)))) return true;
        return false;
    }

    /** The item the next placement needs (so it can be taken in hand before the bot leans out over an edge). */
    private Item wantHeld;

    /** Sneaks to the edge of the block stood on, in the given direction, and stays there (sneak held). */
    private boolean lean(int x, int y, int z, int dir) {
        String old = enter("lean");
        try { return leanImpl(x, y, z, dir); } finally { activity = old; }
    }

    private boolean leanImpl(int x, int y, int z, int dir) {
        // the edge only holds a sneaking player if the drop beyond it is more than a step
        int bx = x + DXH[dir], bz = z + DZH[dir];
        if (!g.passable(bx, y, bz) || !g.passable(bx, y + 1, bz) || StateInfo.anyTop[g.get(bx, y - 1, bz)] > 6 || StateInfo.anyTop[g.get(bx, y - 2, bz)] > 22) return false;
        if (g.isTower(x, y - 1, z)) return false;
        if (!centre(x, y, z) || onTower()) return false;
        double lastX = Double.NaN, lastZ = Double.NaN;
        for (int n = 0; n < 40; n++) {
            c.look(yawTo(DXH[dir], DZH[dir]), c.pitch());
            c.keys(true, false, false, false, false, true, false);
            tick();
            if (Math.abs(c.x() - lastX) + Math.abs(c.z() - lastZ) < 1e-4 && n > 3) break;
            lastX = c.x(); lastZ = c.z();
        }
        c.keys(false, false, false, false, false, true, false);
        tick();
        leaning = true;
        holdSneak = true;
        return c.onGround() && cy() == y;
    }

    private void endLean() {
        if (!leaning) return;
        leaning = false;
        holdSneak = false;
        // step back onto the middle of the block before letting go of sneak
        int x = cx(), y = cy(), z = cz();
        if (g.stand16(x, y, z) < 0) {
            // the body's centre hangs over the neighbouring cell: find the block we are actually on
            for (int d = 0; d < 4; d++) if (g.stand16(x + DXH[d], y, z + DZH[d]) >= 0 && Math.abs(c.x() - (x + DXH[d] + 0.5)) < 0.85 && Math.abs(c.z() - (z + DZH[d] + 0.5)) < 0.85) { x += DXH[d]; z += DZH[d]; break; }
        }
        for (int n = 0; n < 30; n++) {
            double dx = x + 0.5 - c.x(), dz = z + 0.5 - c.z();
            if (dx * dx + dz * dz < 0.04) break;
            c.look(yawTo(dx, dz), c.pitch());
            c.keys(true, false, false, false, false, true, false);
            tick();
        }
        c.releaseKeys();
        tick();
    }

    /** Executes the route last found by nav. */
    private boolean travel() {
        String old = enter("travel");
        try { return travelImpl(); } finally { activity = old; }
    }

    private boolean travelImpl() {
        int n = nav.length;
        // nav's arrays are reused by later searches: copy the route
        int[] path = java.util.Arrays.copyOf(nav.path, n * 3);
        byte[] move = java.util.Arrays.copyOf(nav.move, n);
        if (TRACE && verbose) {
            StringBuilder sb = new StringBuilder("route:");
            for (int k = 0; k < n; k++) sb.append(' ').append(k == 0 ? "" : "WJDPBdcMCST".charAt(move[k]) + ">").append(rel(path[k * 3], path[k * 3 + 1], path[k * 3 + 2]));
            log(sb.toString());
        }
        int i = 1;
        while (i < n) {
            byte m = move[i];
            if (m == Nav.PILLAR) {
                int x = path[i * 3], y = path[i * 3 + 1] - 1, z = path[i * 3 + 2];
                boolean pOk = pillar(x, y, z);
                if (pOk) rope.add(new int[] {Nav.PILLAR, x, y, z, x, y + 1, z, g.idx(x, y, z)});
                if (!pOk) return fail("pillar at " + rel(x, y, z) + " have=" + count(scaffoldItem) + " cell=" + StateInfo.name(g.get(x, y, z)) + " below=" + StateInfo.name(g.get(x, y - 1, z)));
                i++;
            } else if (m == Nav.BRIDGE) {
                boolean bOk = bridge(path[(i - 1) * 3], path[(i - 1) * 3 + 1], path[(i - 1) * 3 + 2], path[i * 3], path[i * 3 + 2]);
                if (bOk) rope.add(new int[] {Nav.BRIDGE, path[(i - 1) * 3], path[(i - 1) * 3 + 1], path[(i - 1) * 3 + 2], path[i * 3], path[i * 3 + 1], path[i * 3 + 2], g.idx(path[i * 3], path[i * 3 + 1] - 1, path[i * 3 + 2])});
                if (!bOk) return fail("bridge from " + rel(path[(i - 1) * 3], path[(i - 1) * 3 + 1], path[(i - 1) * 3 + 2]) + " to " + rel(path[i * 3], path[i * 3 + 1], path[i * 3 + 2]) + " on=" + StateInfo.name(g.get(path[(i - 1) * 3], path[(i - 1) * 3 + 1] - 1, path[(i - 1) * 3 + 2])));
                i++;
            } else if (m == Nav.CLEAR) {
                int x = path[i * 3], y = path[i * 3 + 1], z = path[i * 3 + 2];
                boolean cOk = true;
                for (int yy = y + 1; yy >= y && cOk; yy--) {
                    if (!scaffold.contains(g.idx(x, yy, z))) continue;
                    settle();
                    var e = c.eye();
                    cOk = findBreak(x, yy, z, e.x, e.y, e.z, click) && breakBlock(x, yy, z, click.ax, click.ay, click.az, Math.max(80, breakTicks(g.get(x, yy, z))));
                }
                if (!cOk) return fail("clear scaffold at " + rel(x, y, z));
                one[0] = x; one[1] = y; one[2] = z;
                if (!follow(one, 0, 1)) return fail("walk after clearing " + rel(x, y, z));
                i++;
            } else if (m == Nav.MINE && path[i * 3] == path[i * 3 - 3] && path[i * 3 + 2] == path[i * 3 - 1]) {
                // straight down: break the block underfoot and fall
                int x = path[i * 3 - 3], y = path[i * 3 - 2], z = path[i * 3 - 1];
                if (g.stand16(x, y, z) > 0) y++;   // standing on something low in the own cell: that is what goes
                int w = g.get(x, y - 1, z);
                if (!centre(x, y - (g.stand16(x, y - 1, z) > 0 ? 1 : 0), z)) return fail("centre before mining down at " + rel(x, y, z));
                int onTop = g.get(x, y, z);
                if (onTop != 0 && StateInfo.noCollision[onTop]) breakBlock(x, y, z, x + 0.5, y + 0.03, z + 0.5, Math.max(40, breakTicks(onTop)));
                if (!breakBlock(x, y - 1, z, x + 0.5, y - 1 + Math.max(1, StateInfo.anyTop[w]) / 16.0, z + 0.5, Math.max(80, breakTicks(w) + 20))) return fail("mine down at " + rel(x, y - 1, z));
                mined++;
                log("mined down through " + StateInfo.name(w) + " at " + rel(x, y - 1, z));
                int ci = g.idx(x, y - 1, z);
                if (g.tgt[ci] != 0) deferred.add(ci);
                for (int n2 = 0; n2 < 200 && !(c.onGround() && cy() <= path[i * 3 + 1]); n2++) tick();
                settle();
                if (cy() != path[i * 3 + 1]) return fail("fall after mining down");
                i++;
            } else if (m == Nav.MINE) {
                int x = path[i * 3], y = path[i * 3 + 1], z = path[i * 3 + 2];
                boolean mOk = true;
                for (int yy = y + 1; yy >= y && mOk; yy--) {
                    int w = g.get(x, yy, z);
                    if (w == 0 || StateInfo.noCollision[w]) continue;
                    settle();
                    var e = c.eye();
                    mOk = findBreak(x, yy, z, e.x, e.y, e.z, click) && breakBlock(x, yy, z, click.ax, click.ay, click.az, Math.max(80, breakTicks(w) + 20));
                    if (mOk) {
                        mined++;
                        log("mined through " + StateInfo.name(w) + " at " + rel(x, yy, z));
                        int ci = g.idx(x, yy, z);
                        if (g.tgt[ci] != 0 && !scaffold.contains(ci)) deferred.add(ci);   // put back at the end
                    }
                }
                if (!mOk) return fail("mine at " + rel(x, y, z));
                one[0] = x; one[1] = y; one[2] = z;
                if (!follow(one, 0, 1)) return fail("walk after mining " + rel(x, y, z));
                i++;
            } else if (m == Nav.CLIMB || m == Nav.TOWER) {
                int x = path[i * 3], z = path[i * 3 + 2];
                int j = i;
                while (j < n && (move[j] == Nav.CLIMB || move[j] == Nav.TOWER) && path[j * 3] == x && path[j * 3 + 2] == z) j++;
                int toY = path[(j - 1) * 3 + 1];
                if (cleaning) { behindX = x; behindZ = z; stepOff = true; }
                if (!tower(x, path[(i - 1) * 3 + 1], z, toY)) return fail("tower at " + rel(x, toY, z) + " from y=" + (path[(i - 1) * 3 + 1] - g.oy) + " have=" + count(towerItem) + " pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz));
                i = j;
            } else if (m == Nav.DESCEND) {
                int x = path[i * 3], z = path[i * 3 + 2];
                int j = i;
                while (j < n && move[j] == Nav.DESCEND && path[j * 3] == x && path[j * 3 + 2] == z) j++;
                int toY = path[(j - 1) * 3 + 1];
                String old = enter("tower");
                boolean dOk;
                try { dOk = centre(x, path[(i - 1) * 3 + 1], z) && middle(x, z) && descend(x, toY, z); } finally { activity = old; }
                if (!dOk) return fail("sink at " + rel(x, toY, z) + " pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz));
                i = j;
            } else if (m == Nav.DIG) {
                if (!digDown(path[(i - 1) * 3], path[(i - 1) * 3 + 1], path[(i - 1) * 3 + 2], path[i * 3 + 1])) return fail("dig at " + rel(path[(i - 1) * 3], path[(i - 1) * 3 + 1], path[(i - 1) * 3 + 2]));
                i++;
            } else {
                int j = i;
                while (j < n && move[j] <= Nav.DROP) j++;
                boolean off = stepOff && (path[i * 3] != behindX || path[i * 3 + 2] != behindZ);
                if (off) j = i + 1;   // first only the step off the column, to take it down from there
                if (!follow(path, i, j)) {
                    // remember the step that did not work so that the next route avoids it
                    int k = Math.max(1, failedStep);
                    if (k < n && g.in(path[k * 3 - 3], path[k * 3 - 2], path[k * 3 - 1]) && g.in(path[k * 3], path[k * 3 + 1], path[k * 3 + 2]))
                        nav.blockedEdges.add(Nav.edge(g.idx(path[k * 3 - 3], path[k * 3 - 2], path[k * 3 - 1]), g.idx(path[k * 3], path[k * 3 + 1], path[k * 3 + 2])));
                    unreachable.clear();
                    if (verbose && failLogs < 10) {
                        StringBuilder sb = new StringBuilder("around the bot (y+2 .. y-1; rows z-1..z+1, cols x-1..x+1):");
                        for (int dy = 2; dy >= -1; dy--) { sb.append(" | y").append(dy >= 0 ? "+" : "").append(dy).append(':'); for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) sb.append(' ').append(StateInfo.name(g.get(cx() + dx, cy() + dy, cz() + dz))); }
                        log(sb.toString());
                    }
                    stepOff = false;
                    return fail("walk " + rel(path[k * 3 - 3], path[k * 3 - 2], path[k * 3 - 1]) + " -> " + rel(path[k * 3], path[k * 3 + 1], path[k * 3 + 2]) + " (move " + move[k] + ") .. " + rel(path[j * 3 - 3], path[j * 3 - 2], path[j * 3 - 1]) + " pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz) + " ground=" + c.onGround());
                }
                if (off) { stepOff = false; takeDownBehind(behindX, behindZ); }
                i = j;
            }
        }
        stepOff = false;
        return true;
    }

    private boolean stepOff;
    private int behindX, behindZ;

    /** During the final clean-up: having stepped off a scaffolding column, break as much of it as can be reached from here. */
    private void takeDownBehind(int x, int z) {
        if (!c.onGround()) settle();
        var e = c.eye();
        for (int y = cy() - 5; y <= cy() + 2; y++) {
            if (!g.in(x, y, z)) continue;
            int i = g.idx(x, y, z);
            if (!scaffold.contains(i) || !g.isTower(x, y, z)) continue;
            if (!findBreak(x, y, z, e.x, e.y, e.z, click)) continue;
            int top = y;
            while (g.isTower(x, top + 1, z)) top++;
            if (breakBlock(x, y, z, click.ax, click.ay, click.az, 40)) {
                for (int n = 0; n < 30 + 3 * (top - y) && g.isTower(x, top, z) && top > y; n++) tick();
                if (!c.onGround()) settle();
            }
            return;
        }
    }

    private final int[] one = new int[3];
    private static final boolean TRACE = System.getenv("MCB_TRACE") != null;

    private boolean centre(int x, int y, int z) {
        if (cx() == x && cz() == z && Math.abs(c.x() - x - 0.5) < 0.2 && Math.abs(c.z() - z - 0.5) < 0.2 && c.onGround()) return true;
        one[0] = x; one[1] = y; one[2] = z;
        return follow(one, 0, 1);
    }

    public long nTowerBlocks;

    /**
     * Standing in column (x,z) at level y: makes sure a scaffolding column carries the bot up to level toY (feet),
     * adding blocks to it as needed, and climbs there. Blocks are added the way a player does it: the first one
     * onto the floor, further ones by clicking the column from inside it (they appear on top).
     */
    private boolean tower(int x, int y, int z, int toY) {
        String old = enter("tower");
        try { return towerImpl(x, y, z, toY); } finally { activity = old; }
    }

    private boolean towerImpl(int x, int y, int z, int toY) {
        if (!centre(x, y, z) || !middle(x, z)) return false;
        for (int guard = 0; guard < 800; guard++) {
            if (!c.onGround()) settle();
            if (cx() != x || cz() != z) return false;
            if (Math.abs(c.y() - cy()) > 0.02 && !middle(x, z)) return false;
            int f = cy();
            int top = g.isTower(x, f, z) ? f : f - 1;
            boolean any = g.isTower(x, top, z);
            if (any) while (g.isTower(x, top + 1, z)) top++;
            if (any && top >= toY - 1) break;
            int next = any ? top + 1 : f;                     // the cell the next block goes into
            int there = g.get(x, next, z);
            if (there != 0 && !StateInfo.replaceable[there]) return fail("tower: " + StateInfo.name(there) + " in the way at " + rel(x, next, z));
            if (count(towerItem) == 0 || !hold(towerItem)) return false;
            boolean sHere = g.isTower(x, f, z), sAbove = g.isTower(x, f + 1, z), sBelow = g.isTower(x, f - 1, z);
            float pitch;
            boolean sneak = false;
            if (g.isTower(x, (int) Math.floor(c.y() + 1.62), z)) pitch = -90;   // from inside: a click on the block around the head adds one on top
            else if (sHere && !sBelow) { pitch = 90; sneak = true; }   // second block: sneak-click the top of the first
            else if (!sHere && !sBelow) pitch = 90;           // first block: onto the floor, into the own cell
            else { if (!descend(x, f - 1, z)) return fail("tower: could not get into the column at " + rel(x, f, z)); continue; }
            if (sneak) { c.keys(false, false, false, false, false, true, false); tick(); tick(); }
            c.look(c.yaw(), pitch);
            tick();                                           // (the item is in the hand from this tick on)
            BlockHitResult hit = oracle.pick(c, c.yaw(), pitch);
            boolean ok = false;
            if (hit != null) {
                int pred = oracle.predict(towerItem, hit, c.yaw(), pitch, sneak);
                ok = pred != 0 && StateInfo.scaffolding[pred] && oracle.placedAt.getX() == x && oracle.placedAt.getY() == next && oracle.placedAt.getZ() == z;
            }
            if (ok) { c.clickUse(); tick(); }
            if (sneak) { c.releaseKeys(); tick(); }
            if (ok && !g.isTower(x, next, z)) tick();
            if (!ok || !g.isTower(x, next, z)) {
                return fail("tower: no block at " + rel(x, next, z) + " ok=" + ok + " hit=" + (hit == null ? "none" : rel(hit.getBlockPos().getX(), hit.getBlockPos().getY(), hit.getBlockPos().getZ()) + " " + hit.getDirection() + " inside=" + hit.isInside()) + " feet=" + (f - g.oy) + " here/above/below=" + sHere + "/" + sAbove + "/" + sBelow);
            }
            placed++; nTowerBlocks++;
            addScaffold(x, next, z);
            tick(); tick();
        }
        if (cy() == toY && c.onGround()) return true;
        return cy() > toY ? descend(x, toY, z) : climb(x, toY, z);
    }

    /** Standing in cell (x,y,z) on a full block: jump and put scaffolding into that cell, ending on top of it. */
    private boolean pillar(int x, int y, int z) {
        String old = enter("pillar");
        try { return pillarImpl(x, y, z); } finally { activity = old; }
    }

    private boolean pillarImpl(int x, int y, int z) {
        if (count(scaffoldItem) == 0) return false;
        if (!hold(scaffoldItem) || !centre(x, y, z)) return false;
        settle();
        double y0 = c.y();
        c.look(c.yaw(), 90);
        c.keys(false, false, false, false, true, false, false);
        boolean ok = false;
        for (int n = 0; n < 14; n++) {
            if (!ok && c.y() >= y0 + 1.0 && g.get(x, y, z) == 0) c.clickUse();
            tick();
            if (n == 0) c.jump(false);
            if (g.get(x, y, z) != 0) ok = true;
            if (c.onGround() && n > 2) break;
        }
        c.releaseKeys();
        if (ok) { placed++; addScaffold(x, y, z); nPillarMoves++; }
        else fail("pillar: no block appeared; y0=" + String.format("%.2f", y0 - g.oy) + " now=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz) + " hit=" + (c.lastHit() == null ? "none" : rel(c.lastHit().getBlockPos().getX(), c.lastHit().getBlockPos().getY(), c.lastHit().getBlockPos().getZ()) + "/" + c.lastHit().getDirection()) + " above=" + StateInfo.name(g.get(x, y + 2, z)) + "," + StateInfo.name(g.get(x, y + 3, z)));
        settle();
        return ok;
    }

    /** From cell (px,py,pz), standing on a full block, put scaffolding beside that block towards (x,z) and step onto it. */
    private boolean bridge(int px, int py, int pz, int x, int z) {
        String old = enter("bridge");
        try { return bridgeImpl(px, py, pz, x, z); } finally { activity = old; }
    }

    private boolean bridgeImpl(int px, int py, int pz, int x, int z) {
        if (count(scaffoldItem) == 0) return false;
        int dir = x > px ? 0 : x < px ? 1 : z > pz ? 2 : 3;
        if (!hold(scaffoldItem) || !lean(px, py, pz, dir)) { fail("bridge: lean failed pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz) + " ground=" + c.onGround() + " sneak=" + c.sneaking()); endLean(); return false; }
        var e = c.eye();
        boolean ok = false;
        if (verbose && failLogs < 60 && !findClick(x, py - 1, z, scaffoldState, null, e.x, e.y, e.z, true, 1 << (dir == 0 ? 4 : dir == 1 ? 5 : dir == 2 ? 2 : 3), click))
            log("bridge: no click; eye=" + String.format("%.2f,%.2f,%.2f", e.x - g.ox, e.y - g.oy, e.z - g.oz) + " dir=" + dir + " target cell=" + StateInfo.name(g.get(x, py - 1, z)) + " support=" + StateInfo.name(g.get(px, py - 1, pz)));
        if (findClick(x, py - 1, z, scaffoldState, null, e.x, e.y, e.z, true, 1 << (dir == 0 ? 4 : dir == 1 ? 5 : dir == 2 ? 2 : 3), click)) {
            c.look(click.yaw, click.pitch);
            c.clickUse();
            c.keys(false, false, false, false, false, true, false);
            tick();
            ok = g.get(x, py - 1, z) != 0;
        }
        leaning = false;
        holdSneak = false;
        c.releaseKeys();
        if (!ok) { tick(); return false; }
        placed++;
        nBridgeMoves++;
        addScaffold(x, py - 1, z);
        one[0] = x; one[1] = py; one[2] = z;
        return follow(one, 0, 1);
    }

    /** Standing on a scaffold block in cell (x,y,z): break it (and any directly under it down to targetY) and sink. */
    private boolean digDown(int x, int y, int z, int targetY) {
        String old = enter("dig");
        try { return digDownImpl(x, y, z, targetY); } finally { activity = old; }
    }

    private boolean digDownImpl(int x, int y, int z, int targetY) {
        nDigMoves++;
        if (verbose && digLogs++ < 6) log("digDown from " + rel(x, y, z) + " to y=" + (targetY - g.oy) + " pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz) + " ground=" + c.onGround() + " under=" + StateInfo.name(g.get(x, y - 1, z)) + " own=" + StateInfo.name(g.get(x, y, z)) + " stand=" + g.stand16(x, y, z) + " scaf=" + scaffold.contains(g.idx(x, y - 1, z)));
        if (!centre(x, y, z)) return false;
        for (int yy = y - 1; yy >= targetY; yy--) {
            if (g.get(x, yy, z) == 0) continue;
            if (!scaffold.contains(g.idx(x, yy, z))) break;
            // something thin lying on top of the block (it would not survive the block's removal anyway)
            int onTop = g.get(x, yy + 1, z);
            if (onTop != 0 && StateInfo.noCollision[onTop]) breakBlock(x, yy + 1, z, x + 0.5, yy + 1.03, z + 0.5, Math.max(40, breakTicks(onTop)));
            if (!breakBlock(x, yy, z, x + 0.5, yy + 1, z + 0.5, Math.max(80, breakTicks(g.get(x, yy, z))))) return false;
        }
        for (int n = 0; n < 200 && !(c.onGround() && cy() <= targetY); n++) tick();
        settle();
        return cy() == targetY;
    }

    // ------------------------------------------------------------------ scaffolding upkeep
    /** Before a block goes in: take away scaffolding in the same column that it would roof over. */
    /** Returns false if scaffolding stays under the cell that could not be taken away now. */
    private boolean clearBelow(int x, int y, int z) {
        if (scaffoldInColumn[col(x, z)] == 0) return true;
        for (int yy = y - 1; yy >= g.y0; yy--) {
            int i = g.idx(x, yy, z);
            if (scaffold.contains(i)) { if (g.isTower(x, yy, z)) return true; if (!removeScaffold(i)) return false; }
            else if (g.cur[i] != 0) return true;
        }
        return true;
    }

    /** Is there a loose scaffold block (not a column) in the open space under the cell? */
    private boolean looseBelow(int x, int y, int z) {
        for (int yy = y - 1; yy >= g.y0; yy--) {
            int i = g.idx(x, yy, z);
            if (scaffold.contains(i)) return !g.isTower(x, yy, z);
            if (g.cur[i] != 0) return false;
        }
        return false;
    }

    /** While true, a block is not put on top of scaffolding that is still there (it waits instead). */
    private boolean noRoofing = true;

    private boolean removeScaffold(int i) {
        String old = enter("unscaffold");
        try { return removeScaffoldImpl(i); } finally { activity = old; }
    }

    private boolean removeScaffoldImpl(int i) {
        checkScaffold();
        if (StateInfo.scaffolding[g.cur[i]]) return removeTower(i);
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        for (int attempt = 0; attempt < 3; attempt++) {
            if (!scaffold.contains(i)) return true;
            if (!c.onGround()) settle();
            var e = c.eye();
            if (findBreak(x, y, z, e.x, e.y, e.z, click)) {
                if (breakBlock(x, y, z, click.ax, click.ay, click.az, Math.max(80, breakTicks(g.get(x, y, z))))) { settle(); return true; }
            }
            Stance st = planStance(x, y, z, null, true);
            if (st == null) {
                if (!forceMine || !tunnelTo(x, y, z)) return false;
                continue;
            }
            if (!goTo(st)) { badStand.add(g.idx(st.x, st.y, st.z)); replans++; }
        }
        return !scaffold.contains(i);
    }

    /**
     * Takes down a scaffolding column: breaks its lowest block (everything above a broken block comes down by
     * itself), or failing that the given block.
     */
    private boolean removeTower(int i) {
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        if (TRACE && verbose) log("removeTower " + rel(x, y, z) + (cleaning ? " (cleanup)" : " (build)") + " by " + StackWalker.getInstance().walk(st -> st.skip(2).limit(3).map(f -> f.getMethodName()).toList()));
        int low = y;
        while (g.in(x, low - 1, z) && scaffold.contains(g.idx(x, low - 1, z)) && g.isTower(x, low - 1, z)) low--;
        if (breakTowerBlock(x, low, z) && !scaffold.contains(i)) return true;
        if (!scaffold.contains(i)) return true;
        if (low != y) breakTowerBlock(x, y, z);
        return !scaffold.contains(i);
    }

    private boolean breakTowerBlock(int x, int y, int z) {
        int i = g.idx(x, y, z);
        for (int attempt = 0; attempt < 3; attempt++) {
            if (!scaffold.contains(i)) return true;
            if (!c.onGround()) settle();
            var e = c.eye();
            if (findBreak(x, y, z, e.x, e.y, e.z, click)) {
                int top = y;
                while (g.isTower(x, top + 1, z)) top++;
                if (breakBlock(x, y, z, click.ax, click.ay, click.az, 40)) {
                    // wait for the rest to come down
                    for (int n = 0; n < 30 + 3 * (top - y) && g.isTower(x, top, z) && top > y; n++) tick();
                    if (!c.onGround()) settle();
                    return true;
                }
            }
            Stance st = planStance(x, y, z, null, true);
            if (st == null) {
                if (!forceMine || !tunnelTo(x, y, z)) return false;
                continue;
            }
            if (!goTo(st)) { badStand.add(g.idx(st.x, st.y, st.z)); replans++; }
        }
        return !scaffold.contains(i);
    }

    /** Gets next to a walled-in cell by breaking a way to it through the build (what is broken is put back later). */
    private boolean tunnelTo(int x, int y, int z) {
        undoRope();
        for (int dy = 0; dy >= -1; dy--) for (int d = 0; d < 4; d++) {
            int nx = x + DXH[d], ny = y + dy, nz = z + DZH[d];
            if (!g.in(nx, ny, nz) || ny < g.oy) continue;
            nav.mine = this::mineTicks;
            nav.effort = 20;
            boolean ok = search(nx, ny, nz, 60000);
            nav.mine = null;
            nav.effort = 1;
            if (verbose && tunnelLogs++ < 30) log("tunnel to " + rel(nx, ny, nz) + " for " + rel(x, y, z) + ": " + ok + " expanded=" + nav.expanded + " cell=" + StateInfo.name(g.get(nx, ny, nz)) + " below=" + StateInfo.name(g.get(nx, ny - 1, nz)) + " mineTicks=" + mineTicks(nx, ny, nz) + "/" + mineTicks(nx, ny + 1, nz));
            if (ok && travel()) return true;
        }
        return false;
    }

    private int cleanupLogs;

    private void cleanup() {
        String old = enter("cleanup");
        try { cleanupImpl(); } finally { activity = old; }
    }

    private void cleanupImpl() {
        badStand.clear();
        IntOpenHashSet hopeless = new IntOpenHashSet();
        int guard = 0;
        int least = scaffold.size(), sinceLeast = 0;
        cleaning = true;
        try {
        while (scaffold.size() > hopeless.size() && guard++ < 100000) {
            // going round in circles (taking one block away at the price of leaving others) is not progress
            if (scaffold.size() < least) { least = scaffold.size(); sinceLeast = 0; }
            else if (++sinceLeast > 24 + least / 4) break;
            // highest first: pillars come down from the top
            int best = -1, bestY = Integer.MIN_VALUE;
            long bestD = Long.MAX_VALUE;
            int bx = cx(), bz = cz();
            boolean bestTower = true;
            for (int i : scaffold) {
                if (hopeless.contains(i)) continue;
                boolean tw = StateInfo.scaffolding[g.cur[i]];
                if (tw && !bestTower) continue;
                int y = tw ? -g.yOf(i) : g.yOf(i);   // columns: lowest block first
                long d = Math.abs(g.xOf(i) - bx) + Math.abs(g.zOf(i) - bz);
                if ((bestTower && !tw) || y > bestY || (y == bestY && d < bestD)) { best = i; bestY = y; bestD = d; bestTower = tw; }
            }
            if (best < 0) break;
            int before = scaffold.size();
            long t0 = ticks, pm = nPillarMoves, dm = nDigMoves;
            boolean okRemove = removeScaffold(best);
            if (okRemove) sweepScaffold();
            undoRope();
            if (verbose && cleanupLogs++ < 60) log("cleanup " + rel(g.xOf(best), g.yOf(best), g.zOf(best)) + (okRemove ? " ok" : " FAILED") + " ticks=" + (ticks - t0) + " pillars=" + (nPillarMoves - pm) + " digs=" + (nDigMoves - dm) + " left=" + scaffold.size());
            if (!okRemove) { hopeless.add(best); badStand.clear(); }
            else if (scaffold.size() < before) hopeless.clear();
            else if (scaffold.size() >= before) hopeless.add(best);
        }
        } finally {
            cleaning = false;
        }
    }

    /** True during the final clean-up: every trip up is taken down again before the next one. */
    private boolean cleaning;

    /** Breaks every scaffold block within reach of where the bot stands, except the one it stands on. */
    private void sweepScaffold() {
        boolean again = true;
        while (again) {
            again = false;
            if (!c.onGround()) settle();
            int x = cx(), y = cy(), z = cz();
            var e = c.eye();
            for (int dy = 5; dy >= -4 && !again; dy--) for (int dz = -4; dz <= 4 && !again; dz++) for (int dx = -4; dx <= 4 && !again; dx++) {
                int tx = x + dx, ty = y + dy, tz = z + dz;
                if (!g.in(tx, ty, tz) || (dx == 0 && dz == 0 && dy == -1)) continue;
                int si = g.idx(tx, ty, tz);
                if (!scaffold.contains(si)) continue;
                boolean mine = false;
                for (int[] r : rope) if (r[7] == si) { mine = true; break; }
                if (mine) continue;
                if (findBreak(tx, ty, tz, e.x, e.y, e.z, click) && breakBlock(tx, ty, tz, click.ax, click.ay, click.az, Math.max(80, breakTicks(g.get(tx, ty, tz))))) again = true;
            }
        }
    }

    public int scaffoldLeft() { return scaffold.size(); }

    // ------------------------------------------------------------------ supplies
    private Item mustFetch;
    /** Set when the bot could not get to the chests; it then works with what it has. */
    private boolean stranded;
    private int scaffoldTrips, towerTrips;

    private int missingNow(Map<Item, Integer> need, Item it) {
        Integer n = need.get(it);
        return n == null ? 0 : n - count(it);
    }

    /** Goes to the chests and loads up for the cells still to do, lowest layers first. */
    private void restock(int fromLayer) {
        restocking = true;
        try {
            restock0(fromLayer);
        } finally {
            restocking = false;
        }
        if (verbose) log("after restock: " + inventorySummary());
    }

    private void restock0(int fromLayer) {
        String old = enter("restock");
        try { restock0Impl(fromLayer); } finally { activity = old; }
    }

    private void restock0Impl(int fromLayer) {
        endLean();
        if (!rope.isEmpty()) undoRope();
        trips++;
        Map<Item, Integer> need = new LinkedHashMap<>();
        int slotsUsed = 0;
        // first what the cell in hand needs, and what the cells put off until later need
        if (mustFetch != null) { need.put(mustFetch, new ItemStack(mustFetch).getMaxStackSize()); slotsUsed++; }
        // scaffolding is used up (broken blocks drop nothing): carry more of it the more the build turns out to need
        int wantLoose = Math.min(64 * 4, 64 + 64 * scaffoldTrips), wantTower = Math.min(64 * 6, 128 + 64 * towerTrips);
        need.merge(scaffoldItem, wantLoose, Integer::sum);
        if (count(scaffoldItem) < 16) scaffoldTrips++;
        need.merge(towerItem, wantTower, Integer::sum);
        if (count(towerItem) < 16) towerTrips++;
        int reserved = (wantLoose + 63) / 64 + (wantTower + 63) / 64;
        for (int cell : deferred) {
            if (!needs(cell) || slotsUsed >= 12) continue;
            int t = g.tgt[cell];
            Item it = StateInfo.item[t];
            if (it == Items.AIR || StateInfo.itemCount[t] == 0) continue;
            int now = need.getOrDefault(it, 0);
            if (now == 0) slotsUsed++;
            need.put(it, now + StateInfo.itemCount[t]);
        }
        int capacity = 35 - reserved;
        // then what the next layers need - the lowest unfinished ones first, in each the cells nearest to the bot -
        // until the inventory would be full
        int bx = Math.max(g.ox, Math.min(g.ox + g.sx - 1, cx())), bz = Math.max(g.oz, Math.min(g.oz + g.sz - 1, cz()));
        int maxR = Math.max(g.sx, g.sz);
        long budget = 36L * 64 * 3;
        int low = Integer.MAX_VALUE, topY = g.oy + g.sy;
        if (colNext != null) for (int z = g.oz; z < g.oz + g.sz; z++) for (int x = g.ox; x < g.ox + g.sx; x++) low = Math.min(low, colNext[col(x, z)]);
        if (low == Integer.MAX_VALUE) low = topY;
        int fullAt = -1;
        outer:
        for (int band = low; band < topY; band += 3) {
            if (slotsUsed >= capacity) { if (fullAt < 0) fullAt = band; else if (band >= fullAt + 6) break; }
            for (int r = 0; r <= maxR; r++) {
                for (int dz = -r; dz <= r; dz++) {
                    int z = bz + dz;
                    if (z < g.oz || z >= g.oz + g.sz) continue;
                    int step = (dz == -r || dz == r) ? 1 : 2 * r;
                    for (int dx = -r; dx <= r; dx += Math.max(1, step)) {
                        int x = bx + dx;
                        if (x < g.ox || x >= g.ox + g.sx) continue;
                        int y0 = colNext == null ? g.oy : colNext[col(x, z)];
                        if (y0 == NONE) continue;
                        for (int y = Math.max(y0, band); y < Math.min(band + 3, topY); y++) {
                            int i = g.idx(x, y, z);
                            if (!needs(i)) continue;
                            int t = g.tgt[i];
                            Item it = StateInfo.item[t];
                            if (it == Items.AIR || StateInfo.itemCount[t] == 0) continue;
                            int max = new ItemStack(it).getMaxStackSize();
                            int now = need.getOrDefault(it, 0);
                            int add = StateInfo.itemCount[t];
                            if (now == 0 || (now + add - 1) / max != (now - 1) / max) {
                                if (slotsUsed + 1 > capacity) continue;   // no room for another stack: this cell waits for the next trip
                                slotsUsed++;
                            }
                            need.put(it, now + add);
                            Item extra = StateInfo.contentItem(t);
                            if (extra != null) { if (!need.containsKey(extra)) slotsUsed++; need.merge(extra, 1, Integer::sum); }
                            if (--budget < 0) break outer;
                        }
                    }
                }
            }
        }
        List<Site.Stock> todo = new ArrayList<>();
        Map<Item, Integer> missing = new LinkedHashMap<>();
        for (var e : need.entrySet()) { int m = e.getValue() - count(e.getKey()); if (m > 0) missing.put(e.getKey(), m); }
        if (missing.isEmpty()) return;
        for (Site.Stock s : stock) {
            if (s.count <= 0) continue;
            Integer m = missing.get(s.item);
            if (m == null || m <= 0) continue;
            todo.add(s);
            missing.put(s.item, m - s.count);
        }
        if (todo.isEmpty()) return;
        log("restock: " + todo.size() + " stacks");
        // most urgent first (the order in which the needs were collected)
        List<Item> order = new ArrayList<>(need.keySet());
        todo.sort(java.util.Comparator.comparingInt(st -> order.indexOf(st.item)));
        boolean tidied = false;
        int k = 0;
        while (k < todo.size()) {
            Site.Stock first = todo.get(k);
            if (first.count <= 0) { k++; continue; }
            int ax = first.chestX, ay = g.oy, az = first.chestZ + 1;
            if (!(cx() == ax && cy() == ay && cz() == az)) {
                boolean ok = false;
                for (int attempt = 0; attempt < 2 && !ok; attempt++) {
                    // a route may not need more scaffolding than the bot still carries
                    ok = search(ax, ay, az, 200000);
                    if (ok && shortOfScaffold()) { noScaffoldNav = true; ok = search(ax, ay, az, 200000); noScaffoldNav = false; }
                    ok = ok && travel();
                }
                for (int attempt = 0; attempt < 2 && !ok; attempt++) {
                    // shut in: break a way out through the build (what is broken is put back at the end)
                    nav.mine = this::mineTicks;
                    nav.effort = 20;
                    ok = search(ax, ay, az, 200000);
                    if ((ok && shortOfScaffold()) || (!ok && attempt == 1)) { noScaffoldNav = true; ok = search(ax, ay, az, 400000); noScaffoldNav = false; }
                    nav.mine = null;
                    nav.effort = 1;
                    ok = ok && travel();
                }
                if (!ok) {
                    if (verbose) {
                        StringBuilder sb2 = new StringBuilder("cannot reach chest; pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz) + " ground=" + c.onGround() + " stand=" + g.stand16(cx(), cy(), cz()) + " expanded=" + nav.expanded + " around (y+2..y-1; rows z-1..z+1, cols x-1..x+1):");
                        for (int dy = 2; dy >= -1; dy--) { sb2.append(" | y").append(dy >= 0 ? "+" : "").append(dy).append(':'); for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) sb2.append(' ').append(StateInfo.name(g.get(cx() + dx, cy() + dy, cz() + dz))); }
                        log(sb2.toString());
                    }
                    stranded = true; return;
                }
                stranded = false;
            }
            settle();
            lookAt(first.chestX + 0.5, first.chestY + 0.45, first.chestZ + 0.7);
            c.clickUse();
            tick();
            if (!c.containerOpen()) { tick(); if (!c.containerOpen()) { k++; continue; } }
            // make room: put back what this load does not call for
            boolean deposited = false;
            for (int inv = 0; inv < 36 && !tidied; inv++) {
                ItemStack st = c.inventorySlot(inv);
                if (st.isEmpty() || st.getItem() == scaffoldItem || st.getItem() == towerItem || need.containsKey(st.getItem())) continue;
                int menuSlot = inv < 9 ? 81 + inv : 54 + (inv - 9);
                if (menuSlot >= c.menu().slots.size()) break;
                c.clickSlot(menuSlot, 0, ContainerInput.QUICK_MOVE);
                tick();
                deposited = true;
            }
            tidied = true;
            // everything on the list that this chest holds
            for (int j = k; j < todo.size(); j++) {
                Site.Stock s = todo.get(j);
                if (s.count <= 0 || s.chestX != first.chestX || s.chestY != first.chestY || s.chestZ != first.chestZ) continue;
                if (j != k && missingNow(need, s.item) <= 0) continue;
                ItemStack inSlot = c.menu().getSlot(s.slot).getItem();
                if (inSlot.isEmpty() || inSlot.getItem() != s.item) {
                    // not what the list says is here: believe the chest
                    log("chest slot " + s.slot + " at " + rel(s.chestX, s.chestY, s.chestZ) + " holds " + inSlot + ", expected " + s.item + " x" + s.count);
                    s.count = inSlot.isEmpty() || inSlot.getItem() != s.item ? 0 : inSlot.getCount();
                    continue;
                }
                int before = count(s.item);
                c.clickSlot(s.slot, 0, ContainerInput.QUICK_MOVE);
                tick();
                s.count = c.menu().getSlot(s.slot).getItem().getCount();
                if (count(s.item) == before) { if (verbose) log("taking " + s.item + " from slot " + s.slot + " did not arrive; free=" + freeSlots()); if (j == k) s.count = -s.count - 1; }
            }
            // (a stack that would not come - no room - is left for the next trip)
            if (first.count < 0) { first.count = -first.count - 1; k++; }
            if (deposited && c.menu().slots.size() >= 54) {
                // the chest's contents changed in ways the list does not know: read them off the open screen
                for (Site.Stock s : stock) if (s.chestX == first.chestX && s.chestY == first.chestY && s.chestZ == first.chestZ) s.count = 0;
                for (int slot = 0; slot < 54; slot++) {
                    ItemStack st = c.menu().getSlot(slot).getItem();
                    if (st.isEmpty()) continue;
                    Site.Stock found = null;
                    for (Site.Stock s : stock) if (s.chestX == first.chestX && s.chestY == first.chestY && s.chestZ == first.chestZ && s.slot == slot) { found = s; break; }
                    if (found == null) stock.add(new Site.Stock(st.getItem(), st.getCount(), first.chestX, first.chestY, first.chestZ, slot));
                    else { found.item = st.getItem(); found.count = st.getCount(); }
                }
            }
            c.closeScreen();
            tick();
        }
    }

    public String inventorySummary() {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 36; i++) { ItemStack st = c.inventorySlot(i); if (!st.isEmpty()) sb.append(i).append(':').append(st.getItem()).append('x').append(st.getCount()).append(' '); }
        return sb.toString();
    }

    public long nSelf, nNatural, nLean, nPillarStance, nPillarMoves, nBridgeMoves, nDigMoves, buildTicks, buildWallMs;

    public String navStats() { return "ticksBy=" + tickUse + " helped=" + helped + " escapes=" + escapes + " mined=" + mined + " strays=" + strays + " misplaced=" + misplaced + " traps avoided/checks/searches=" + trapsAvoided + "/" + trapChecks + "/" + trapSearches + " towerBlocks=" + nTowerBlocks + " rope undone/orphaned=" + ropeUndone + "/" + ropeOrphaned + " stances self/natural/lean/pillar=" + nSelf + "/" + nNatural + "/" + nLean + "/" + nPillarStance + " moves pillar/bridge/dig=" + nPillarMoves + "/" + nBridgeMoves + "/" + nDigMoves
        + " buildTicks=" + buildTicks + " buildWall=" + buildWallMs + "ms " + "searches=" + nav.searches + " expanded=" + nav.totalExpanded + " plans=" + plans + " planFails=" + planFails + " predictions=" + oracle.predictions; }
}
