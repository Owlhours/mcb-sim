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

    @Override public void tick() {
        super.tick();
        if (!restocking && c.screenOpen() && !inventoryOpenByMe) {
            // a click opened some block's screen (a chest, a hopper ...): close it again, or the bot stands frozen
            strayScreens++;
            if (verbose && strayScreens < 10) { log("UNEXPECTED SCREEN " + c.menu().getClass().getSimpleName() + " lastHit=" + (c.lastHit() == null ? "none" : StateInfo.name(g.get(c.lastHit().getBlockPos().getX(), c.lastHit().getBlockPos().getY(), c.lastHit().getBlockPos().getZ())))); new Exception().printStackTrace(System.out); }
            c.closeScreen();
            super.tick();
        }
        if (wallLimitMs > 0 && (ticks & 63) == 0 && System.currentTimeMillis() - wallStart > wallLimitMs) throw new OutOfTime();
    }

    private int failLogs, trapLogs, digLogs;
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
            // what was put off: one more try each, now that everything else stands
            for (int pass = 0; pass < 2; pass++) {
                for (int cell : deferred.toIntArray()) {
                    if (!needs(cell)) { deferred.remove(cell); continue; }
                    int r = doCell(cell);
                    if (r != DEFER) deferred.remove(cell);
                }
            }
            for (int cell : deferred) if (needs(cell)) skip(cell, "no way to place");
            undoRope();
            reconcile();
            status = "built";
            buildTicks = ticks; buildWallMs = System.currentTimeMillis() - wallStart;
            layerNow = Integer.MAX_VALUE;
            cleanup();
            status = "done";
        } catch (OutOfTime e) {
            status = ticks >= maxTicks ? "out of time" : "wall limit";
        }
        c.releaseKeys();
        c.holdAttack(false);
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
                        if (cost < bestCost) { bestCost = cost; best = i; }
                    }
                }
            }
        }
        return best;
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
        if (r == DEFER) { blocked.put(i, surroundings(i)); return; }
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
        for (int dy = -1; dy <= 1; dy++) for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) h = h * 31 + g.get(x + dx, y + dy, z + dz);
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
            else { skip(i, "occupied by another block"); return SKIP; }
        }
        if (count(sp.item) == 0 || (count(scaffoldItem) < 12 && !stranded)) {
            if (stranded && count(sp.item) == 0) { skip(i, "stranded without material"); return SKIP; }
            restock(y);
            if (count(sp.item) == 0) { skip(i, stranded ? "stranded without material" : "out of stock"); return SKIP; }
        }
        clearBelow(x, y, z);
        if (bodyIn(x, y, z)) {
            if (selfPlace(x, y, z, t, sp)) return PLACED;
        } else if (placeFromHere(x, y, z, t, sp)) return PLACED;
        // go somewhere better
        for (int attempt = 0; attempt < 3; attempt++) {
            Stance st = planStance(x, y, z, sp, false);
            if (st == null) {
                planFails++;
                if (verbose && planFails <= 12) {
                    StringBuilder sb = new StringBuilder("no stance for " + rel(x, y, z) + " " + StateInfo.name(t) + " table=" + (sp.table != null) + " neighbours:");
                    for (int d = 0; d < 6; d++) sb.append(' ').append(StateInfo.name(g.get(x + DX[d], y + DY[d], z + DZ[d])));
                    sb.append(" | dbg standable=" + dbgStand + " clickOk=" + dbgClick + " navOk=" + dbgNav + " pillarCols=" + dbgPillar + " pillarClick=" + dbgPillarClick);
                    log(sb.toString());
                    if (dbgClick > 0 && dbgNav == 0 && trapLogs++ < 2) {
                        StringBuilder sb2 = new StringBuilder("TRAPPED? around the bot (y+2 .. y-1; rows z-1..z+1, cols x-1..x+1): pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz) + " ground=" + c.onGround() + " scaffold=" + count(scaffoldItem) + " have=" + haveScaffold);
                        for (int dy = 2; dy >= -1; dy--) { sb2.append(" | y").append(dy >= 0 ? "+" : "").append(dy).append(':'); for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) sb2.append(' ').append(StateInfo.name(g.get(cx() + dx, cy() + dy, cz() + dz))); }
                        log(sb2.toString());
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

    // ------------------------------------------------------------------ placing from the current position
    private boolean placeFromHere(int x, int y, int z, int t, Spec sp) {
        if (!c.onGround() || hspeed() > 0.003) settle();
        // get the item into the hand first: fetching it from the inventory screen lets go of all keys
        boolean wasSneak = c.sneaking();
        if (!hold(sp.item)) return false;
        if (wasSneak && !c.sneaking()) { c.keys(false, false, false, false, false, true, false); tick(); tick(); }
        if (tryPlace(x, y, z, t, sp)) return true;
        if (leaning || c.sneaking()) return false;
        // against blocks that react to a click (chests, doors ...) one has to sneak; that also lowers the eyes
        boolean wanted = false;
        for (int d = 0; d < 6 && !wanted; d++) wanted = StateInfo.interactive[g.get(x + DX[d], y + DY[d], z + DZ[d])];
        if (!wanted) return false;
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
            if (!hold(sp.item)) return false;
            // what would really happen? (the bot's picture can be off for odd shapes; the rules decide)
            BlockHitResult hit = oracle.pick(c, click.yaw, click.pitch);
            if (hit == null || hit.getBlockPos().getX() != click.nx || hit.getBlockPos().getY() != click.ny || hit.getBlockPos().getZ() != click.nz
                || hit.getDirection().get3DDataValue() != click.face) continue;
            int pred = oracle.predict(sp.item, hit, click.yaw, click.pitch, sneak);
            if (pred == 0 || StateInfo.loose[pred] != StateInfo.loose[sp.first] || oracle.placedAt.getX() != x || oracle.placedAt.getY() != y || oracle.placedAt.getZ() != z) continue;
            if (sp.chest && StateInfo.canon[pred] != StateInfo.canon[chestWant(x, y, z, t)]) continue;
            c.look(click.yaw, click.pitch);
            c.clickUse();
            if (sneak) c.sneak(true);
            tick();
            int now = g.get(x, y, z);
            if (now == 0) { failedClicks++; continue; }
            placed++;
            if (StateInfo.state[now].getBlock() != StateInfo.state[t].getBlock()) {
                // not what was meant to go here (should not happen): take it out again
                misplaced++;
                log("MISPLACED " + StateInfo.name(now) + " in " + rel(x, y, z) + " wanted " + StateInfo.name(t) + " held=" + heldItem() + " predicted=" + StateInfo.name(pred));
                var e2 = c.eye();
                if (findBreak(x, y, z, e2.x, e2.y, e2.z, click)) breakBlock(x, y, z, click.ax, click.ay, click.az, 400);
                return false;
            }
            if (sp.multi && !matches(now, t)) topUp(x, y, z, t, sp);
            if (!matches(g.get(x, y, z), t) && StateInfo.loose[g.get(x, y, z)] == StateInfo.loose[t]) adjust(x, y, z, t);
            return true;
        }
        return false;
    }

    public long misplaced;

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
            boolean wrong = t == 0 ? !isSupplyChest(x, y, z) : StateInfo.state[w].getBlock() != StateInfo.state[t].getBlock() && !sameFamily(w, t);
            if (!wrong) continue;
            strays++;
            if (verbose && strays < 20) log("STRAY " + StateInfo.name(w) + " at " + rel(x, y, z) + " (target " + StateInfo.name(t) + ")");
            scaffold.add(i);
            scaffoldInColumn[col(x, z)]++;
        }
        appeared.clear();
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
            var e = c.eye();
            boolean done = false;
            for (int k = 0; k < 9 && !done; k++) {
                // aim at points on top of the block, then its sides
                double ax = x + 0.5 + (k % 3 - 1) * 0.3, az = z + 0.5 + (k / 3 - 1) * 0.3, ay = y + StateInfo.colTop[now] / 16.0;
                if (StateInfo.colTop[now] == 0) ay = y + 0.1;
                double dx = ax - e.x, dy = ay - e.y, dz = az - e.z, hd = Math.sqrt(dx * dx + dz * dz);
                if (Math.sqrt(dx * dx + dy * dy + dz * dz) > reach - 0.1) continue;
                float yaw = hd < 1e-6 ? c.yaw() : yawTo(dx, dz), pitch = (float) -Math.toDegrees(Math.atan2(dy, hd));
                BlockHitResult hit = oracle.pick(c, yaw, pitch);
                if (hit == null || hit.getBlockPos().getX() != x || hit.getBlockPos().getY() != y || hit.getBlockPos().getZ() != z) continue;
                int pred = oracle.predict(sp.item, hit, yaw, pitch, c.sneaking());
                if (pred == 0 || oracle.placedAt.getX() != x || oracle.placedAt.getY() != y || oracle.placedAt.getZ() != z) continue;
                if (StateInfo.state[pred].getBlock() != StateInfo.state[t].getBlock() || pred == now) continue;
                c.look(yaw, pitch);
                c.clickUse();
                tick();
                if (g.get(x, y, z) != now) { placed++; done = true; }
            }
            if (!done) return false;
        }
        return matches(g.get(x, y, z), t);
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
                    boolean ok = breaking ? findBreak(x, y, z, ex, ey, ez, click) : findClick(x, y, z, sp.first, sp.table, ex, ey, ez, false, 0x3f, click);
                    // against chests, doors and other things that react to a click one sneaks (eyes 0.35 lower)
                    if (!ok && !breaking && touchesInteractive) ok = findClick(x, y, z, sp.first, sp.table, ex, ey - 0.35, ez, true, 0x3f, click);
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
            // 3. a temporary pillar in a free column near the cell
            for (int[] o : RING) {
                if (o[0] == 0 && o[1] == 0) continue;
                if (o[0] * o[0] + o[1] * o[1] > 8) break;
                int qx = x + o[0], qz = z + o[1];
                if (!g.in(qx, y, qz)) continue;
                boolean adjacent = Math.abs(o[0]) + Math.abs(o[1]) == 1;
                for (int top = adjacent ? y : y - 1; top >= y - 2; top--) {  // top = y of the highest pillar block
                    // find the base of the column
                    int base = top;
                    while (base >= g.y0 && g.isAir(qx, base, qz) && scaffoldOk(qx, base, qz)) base--;
                    if (base == top || base < g.y0 || !StateInfo.fullCube[g.get(qx, base, qz)]) continue;
                    if (!g.passable(qx, top + 1, qz) || !g.passable(qx, top + 2, qz)) continue;
                    if (badStand.contains(g.idx(qx, top + 1, qz))) continue;
                    dbgPillar++;
                    // pretend the pillar is there
                    for (int yy = base + 1; yy <= top; yy++) g.cur[g.idx(qx, yy, qz)] = (char) scaffoldState;
                    boolean ok;
                    int lean = -1;
                    double ex = qx + 0.5, ey = top + 1 + 1.62, ez = qz + 0.5;
                    ok = breaking ? findBreak(x, y, z, ex, ey, ez, click) : findClick(x, y, z, sp.first, sp.table, ex, ey, ez, false, 0x3f, click);
                    if (!ok && !breaking && adjacent && top == y) {
                        int d = o[0] == 1 ? 0 : o[0] == -1 ? 1 : o[1] == 1 ? 2 : 3;   // direction from the cell to the pillar
                        ex = qx + 0.5 - DXH[d] * 0.75; ez = qz + 0.5 - DZH[d] * 0.75; ey = top + 1 + 1.27;
                        ok = findClick(x, y, z, sp.first, sp.table, ex, ey, ez, true, 0x3c, click);
                        lean = d ^ 1;
                    }
                    for (int yy = base + 1; yy <= top; yy++) g.cur[g.idx(qx, yy, qz)] = 0;
                    if (!ok) continue;
                    dbgPillarClick++;
                    if (!route(qx, top + 1, qz, 12000)) continue;
                    Stance s = new Stance();
                    s.x = qx; s.y = top + 1; s.z = qz; s.lean = lean; s.cost = nav.goalCost;
                    nPillarStance++;
                    return s;
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
        nav.startH = c.onGround() ? Math.max(0, Math.min(15, (int) Math.round((c.y() - cy()) * 16))) : -1;
        return nav.search(cx(), cy(), cz(), x, y, z, maxExpand, this::scaffoldOk);
    }

    private boolean route(int x, int y, int z, int maxExpand) {
        int here = g.in(cx(), cy(), cz()) ? g.idx(cx(), cy(), cz()) : -1;
        if (here != unreachableFrom || placed - unreachableAt > 30) { unreachable.clear(); unreachableFrom = here; unreachableAt = placed; }
        int goal = g.idx(x, y, z);
        if (unreachable.contains(goal)) return false;
        if (search(x, y, z, maxExpand)) return true;
        unreachable.add(goal);
        return false;
    }

    /** Temporary blocks may go where the target has nothing, or where its turn has not come yet (they are removed before it does). */
    private boolean scaffoldOk(int x, int y, int z) {
        if (!g.in(x, y, z) || !haveScaffold) return false;
        if (g.tgt[g.idx(x, y, z)] == 0) return true;
        if (!g.inBox(x, y, z) || colNext == null) return false;
        return layerNow == Integer.MAX_VALUE || y > colNext[col(x, z)];
    }

    /** False once the bot carries no scaffolding and cannot get more: it then plans without temporary blocks. */
    private boolean haveScaffold = true;

    private void checkScaffold() {
        boolean have = count(scaffoldItem) > 0;
        if (!have && !stranded) for (Site.Stock st : stock) if (st.item == scaffoldItem && st.count > 0) { have = true; break; }
        if (have != haveScaffold) { haveScaffold = have; unreachable.clear(); log(have ? "scaffolding available again" : "out of scaffolding"); }
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
        endLean();
        while (!rope.isEmpty()) {
            int[] e = rope.get(rope.size() - 1);
            int cell = e[7];
            if (!scaffold.contains(cell)) { rope.remove(rope.size() - 1); continue; }
            // get to the far end of this piece (walking or dropping only)
            if (!(cx() == e[4] && cy() == e[5] && cz() == e[6] && c.onGround())) {
                nav.startH = c.onGround() ? Math.max(0, Math.min(15, (int) Math.round((c.y() - cy()) * 16))) : -1;
                if (!nav.search(cx(), cy(), cz(), e[4], e[5], e[6], 3000, null) || !travel()) break;
            }
            boolean ok;
            if (e[0] == Nav.PILLAR) {
                ok = centre(e[4], e[5], e[6]) && breakBlock(e[1], e[2], e[3], e[1] + 0.5, e[2] + 1, e[3] + 0.5, 80);
                for (int n = 0; n < 60 && !c.onGround(); n++) tick();
                settle();
            } else {
                one[0] = e[1]; one[1] = e[2]; one[2] = e[3];
                ok = follow(one, 0, 1);
                if (ok) {
                    settle();
                    var eye = c.eye();
                    int bx = g.xOf(cell), by = g.yOf(cell), bz = g.zOf(cell);
                    ok = findBreak(bx, by, bz, eye.x, eye.y, eye.z, click) && breakBlock(bx, by, bz, click.ax, click.ay, click.az, 80);
                }
            }
            if (!ok) break;
            ropeUndone++;
            rope.remove(rope.size() - 1);
        }
        if (!rope.isEmpty()) { ropeOrphaned += rope.size(); rope.clear(); }   // could not be undone now: the final clean-up gets them
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
                    if (scaffoldInColumn[col(x, z)] != 0) continue;
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
        if (!(cx() == st.x && cy() == st.y && cz() == st.z && c.onGround())) {
            if (!rope.isEmpty() || nav.scaffoldMoves > 0) sweepHere();
            if (!rope.isEmpty()) {
                // growing the pillar the bot stands on is fine; anything else means taking the scaffolding down first
                int[] top = rope.get(rope.size() - 1);
                boolean extend = top[0] == Nav.PILLAR && st.x == top[4] && st.z == top[6] && st.y > top[5] && cx() == top[4] && cz() == top[6];
                if (!extend) undoRope();
            }
            if (!route(st.x, st.y, st.z, 12000)) return false;
            // do not set out on a climb without the blocks for it
            if (nav.scaffoldMoves > 0 && count(scaffoldItem) < nav.scaffoldMoves + 4 && !stranded) {
                restock(layerNow == Integer.MAX_VALUE ? g.oy + g.sy : layerNow);
                if (!route(st.x, st.y, st.z, 12000)) return false;
            }
            if (!travel()) return false;
        }
        if (st.lean >= 0) return lean(st.x, st.y, st.z, st.lean);
        return true;
    }

    /** Sneaks to the edge of the block stood on, in the given direction, and stays there (sneak held). */
    private boolean lean(int x, int y, int z, int dir) {
        // the edge only holds a sneaking player if the drop beyond it is more than a step
        int bx = x + DXH[dir], bz = z + DZH[dir];
        if (!g.passable(bx, y, bz) || !g.passable(bx, y + 1, bz) || StateInfo.anyTop[g.get(bx, y - 1, bz)] > 6 || StateInfo.anyTop[g.get(bx, y - 2, bz)] > 22) return false;
        if (!centre(x, y, z)) return false;
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
        return c.onGround() && cy() == y;
    }

    private void endLean() {
        if (!leaning) return;
        leaning = false;
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
        int n = nav.length;
        // nav's arrays are reused by later searches: copy the route
        int[] path = java.util.Arrays.copyOf(nav.path, n * 3);
        byte[] move = java.util.Arrays.copyOf(nav.move, n);
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
            } else if (m == Nav.DIG) {
                if (!digDown(path[(i - 1) * 3], path[(i - 1) * 3 + 1], path[(i - 1) * 3 + 2], path[i * 3 + 1])) return fail("dig at " + rel(path[(i - 1) * 3], path[(i - 1) * 3 + 1], path[(i - 1) * 3 + 2]));
                i++;
            } else {
                int j = i;
                while (j < n && move[j] <= Nav.DROP) j++;
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
                    return fail("walk " + rel(path[k * 3 - 3], path[k * 3 - 2], path[k * 3 - 1]) + " -> " + rel(path[k * 3], path[k * 3 + 1], path[k * 3 + 2]) + " (move " + move[k] + ") .. " + rel(path[j * 3 - 3], path[j * 3 - 2], path[j * 3 - 1]) + " pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz) + " ground=" + c.onGround());
                }
                i = j;
            }
        }
        return true;
    }

    private final int[] one = new int[3];

    private boolean centre(int x, int y, int z) {
        if (cx() == x && cz() == z && Math.abs(c.x() - x - 0.5) < 0.2 && Math.abs(c.z() - z - 0.5) < 0.2 && c.onGround()) return true;
        one[0] = x; one[1] = y; one[2] = z;
        return follow(one, 0, 1);
    }

    /** Standing in cell (x,y,z) on a full block: jump and put scaffolding into that cell, ending on top of it. */
    private boolean pillar(int x, int y, int z) {
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
    private void clearBelow(int x, int y, int z) {
        if (scaffoldInColumn[col(x, z)] == 0) return;
        for (int yy = y - 1; yy >= g.y0; yy--) {
            int i = g.idx(x, yy, z);
            if (scaffold.contains(i)) { if (!removeScaffold(i)) return; }
            else if (g.cur[i] != 0) return;
        }
    }

    private boolean removeScaffold(int i) {
        checkScaffold();
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        for (int attempt = 0; attempt < 3; attempt++) {
            if (!scaffold.contains(i)) return true;
            if (!c.onGround()) settle();
            var e = c.eye();
            if (findBreak(x, y, z, e.x, e.y, e.z, click)) {
                if (breakBlock(x, y, z, click.ax, click.ay, click.az, Math.max(80, breakTicks(g.get(x, y, z))))) { settle(); return true; }
            }
            Stance st = planStance(x, y, z, null, true);
            if (st == null) return false;
            if (!goTo(st)) { badStand.add(g.idx(st.x, st.y, st.z)); replans++; }
        }
        return !scaffold.contains(i);
    }

    private int cleanupLogs;

    private void cleanup() {
        badStand.clear();
        IntOpenHashSet hopeless = new IntOpenHashSet();
        int guard = 0;
        while (scaffold.size() > hopeless.size() && guard++ < 100000) {
            // highest first: pillars come down from the top
            int best = -1, bestY = Integer.MIN_VALUE;
            long bestD = Long.MAX_VALUE;
            int bx = cx(), bz = cz();
            for (int i : scaffold) {
                if (hopeless.contains(i)) continue;
                int y = g.yOf(i);
                long d = Math.abs(g.xOf(i) - bx) + Math.abs(g.zOf(i) - bz);
                if (y > bestY || (y == bestY && d < bestD)) { best = i; bestY = y; bestD = d; }
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
            else if (scaffold.size() > before + 2) hopeless.add(best);
        }
    }

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
    /** Set when the bot could not get to the chests; it then works with what it has. */
    private boolean stranded;
    private int scaffoldTrips;

    /** Goes to the chests and loads up for the cells still to do, lowest layers first. */
    private void restock(int fromLayer) {
        restocking = true;
        try {
            restock0(fromLayer);
        } finally {
            restocking = false;
        }
    }

    private void restock0(int fromLayer) {
        endLean();
        if (!rope.isEmpty()) undoRope();
        trips++;
        Map<Item, Integer> need = new LinkedHashMap<>();
        int slotsUsed = 0;
        int capacity = Math.max(20, 32 - 2 * Math.min(4, scaffoldTrips));
        // what the cells nearest to the bot need, working outwards, until the inventory would be full
        int bx = Math.max(g.ox, Math.min(g.ox + g.sx - 1, cx())), bz = Math.max(g.oz, Math.min(g.oz + g.sz - 1, cz()));
        int maxR = Math.max(g.sx, g.sz);
        long budget = 36L * 64 * 3;
        outer:
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
                    for (int y = y0; y < g.oy + g.sy; y++) {
                        int i = g.idx(x, y, z);
                        if (!needs(i)) continue;
                        int t = g.tgt[i];
                        Item it = StateInfo.item[t];
                        if (it == Items.AIR || StateInfo.itemCount[t] == 0) continue;
                        int max = new ItemStack(it).getMaxStackSize();
                        int now = need.getOrDefault(it, 0);
                        int add = StateInfo.itemCount[t];
                        if (now == 0 || (now + add - 1) / max != (now - 1) / max) {
                            if (slotsUsed + 1 > capacity) break outer;
                            slotsUsed++;
                        }
                        need.put(it, now + add);
                        if (--budget < 0) break outer;
                    }
                }
            }
        }
        // scaffolding is used up (broken blocks drop nothing): carry more of it the more the build turns out to need
        need.merge(scaffoldItem, Math.min(64 * 12, 192 + 128 * scaffoldTrips), Integer::sum);
        if (count(scaffoldItem) < 16) scaffoldTrips++;
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
        int k = 0;
        while (k < todo.size()) {
            Site.Stock first = todo.get(k);
            int ax = first.chestX, ay = g.oy, az = first.chestZ + 1;
            if (!(cx() == ax && cy() == ay && cz() == az)) {
                boolean ok = search(ax, ay, az, 200000) && travel();
                if (!ok) ok = search(ax, ay, az, 200000) && travel();
                if (!ok) { log("cannot reach chest"); stranded = true; return; }
                stranded = false;
            }
            settle();
            lookAt(first.chestX + 0.5, first.chestY + 0.45, first.chestZ + 0.7);
            c.clickUse();
            tick();
            if (!c.containerOpen()) { tick(); if (!c.containerOpen()) { k++; continue; } }
            while (k < todo.size() && todo.get(k).chestX == first.chestX && todo.get(k).chestY == first.chestY && todo.get(k).chestZ == first.chestZ) {
                Site.Stock s = todo.get(k++);
                if (freeSlots() == 0) break;
                c.clickSlot(s.slot, 0, ContainerInput.QUICK_MOVE);
                tick();
                s.count = c.menu().getSlot(s.slot).getItem().getCount();
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

    public String navStats() { return "rope undone/orphaned=" + ropeUndone + "/" + ropeOrphaned + " stances self/natural/lean/pillar=" + nSelf + "/" + nNatural + "/" + nLean + "/" + nPillarStance + " moves pillar/bridge/dig=" + nPillarMoves + "/" + nBridgeMoves + "/" + nDigMoves
        + " buildTicks=" + buildTicks + " buildWall=" + buildWallMs + "ms " + "searches=" + nav.searches + " expanded=" + nav.totalExpanded + " plans=" + plans + " planFails=" + planFails + " predictions=" + oracle.predictions; }
}
