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
public final class Builder extends Supplies {
    public Builder(Client c, Grid g, Site site, Oracle oracle) { super(c, g, site, oracle); }

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
                // near the end of the time allowed: what stands must still be cleared of scaffolding, and a few
                // blocks short is better than scaffolding all over
                if (timeShort()) { if (verbose) log("time is running out: " + (maxTicks - ticks) + " ticks left, " + scaffold.size() + " scaffold blocks standing - on to the clean-up"); break; }
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
                        deferReasons.merge("blocked when nothing else was left", 1, Integer::sum);
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
            { int nd = 0; for (int cell : deferred) if (needs(cell)) nd++; noteMainDone(nd); }
            mainDone = true;
            lateToo = false;
            deferredPass(false);
            undoRope();
            reconcile();
            layerNow = Integer.MAX_VALUE;
            // then trapdoors and doors that only open by redstone
            lateToo = true;
            deferredPass(false);
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
            // (only now from places the bot may not get out of again: whatever shuts it in for good then costs the
            // scaffolding of that last errand, not all that stood at the end of the building)
            noRoofing = false;
            deferredPass(false);
            deferredPass(true);
            undoRope();
            reconcile();
            cleanup();
            for (int round = 0; round < 3 && (!scaffold.isEmpty() || anyDeferred()) && !timeShort(); round++) {
                // what is left is walled in: go in through the build, then put back what was taken out
                forceMine = true;
                acceptTraps = true;
                unreachable.clear();   // (what could not be reached before may be reachable by breaking through)
                badStand.clear();
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
            if (!scaffold.isEmpty()) {
                // the last cells may have been placed from somewhere the bot cannot leave: out it comes (breaking
                // through if it must), the scaffolding goes, the hole is mended from outside
                forceMine = true;
                unreachable.clear();
                badStand.clear();
                cleanup();
                forceMine = false;
                undoRope();
                deferredPass(false);
                undoRope();
                reconcile();
                cleanup();
                // shut in again while mending, or columns that only come down in a certain order: keep taking turns
                // (with new columns allowed / breaking through instead) for as long as that still removes something
                for (int turn = 0; turn < 4 && !scaffold.isEmpty(); turn++) {
                    int before = scaffold.size();
                    it.unimi.dsi.fastutil.ints.IntOpenHashSet stood = new it.unimi.dsi.fastutil.ints.IntOpenHashSet(scaffold);
                    forceMine = true;
                    unreachable.clear();
                    badStand.clear();
                    cleanup();
                    forceMine = false;
                    undoRope();
                    if (scaffold.isEmpty()) break;
                    unreachable.clear();
                    // (this late nothing new is put up to get at what is left: it would only have to come down again -
                    // a tower down a shaft for two blocks at its bottom ended as 28 blocks left instead of 2)
                    noScaffoldNav = true;
                    try { cleanup(); } finally { noScaffoldNav = false; }
                    undoRope();
                    // A round that breaks through gives up after a number of tries without getting below where it
                    // began - and what it put up on those tries to get at the rest stands where it stopped (a tree
                    // top: 4 pieces at the start, 104 at the end, 56 of them out of reach without new scaffolding).
                    // Those the bot can get at, it has just been there: they come down the ordinary way, with
                    // whatever that takes. The old pieces are left alone in this.
                    int put = 0;
                    for (int i : scaffold) if (!stood.contains(i)) put++;
                    if (put > 0) {
                        forcedWorse++;
                        unreachable.clear();
                        leaveAlone = stood;
                        try { cleanup(); } finally { leaveAlone = null; }
                        undoRope();
                        int still = 0;
                        for (int i : scaffold) if (!stood.contains(i)) still++;
                        tidied += put - still;
                        if (verbose) log("end game: " + put + " pieces of the round's own were out of reach without scaffolding, " + still + " of them still standing, " + scaffold.size() + " in all");
                        unreachable.clear();
                        noScaffoldNav = true;
                        try { cleanup(); } finally { noScaffoldNav = false; }
                        undoRope();
                    }
                    if (scaffold.size() >= before) break;
                }
                // The turns above break through the build to get at what stands behind it, and nothing after them
                // puts back what they broke (four pieces fewer for thirteen blocks of a wall missing). So: one more
                // pass, over the cells the bot broke out itself and nothing else (what never went in has had its
                // passes), with nothing new put up for it (scaffolding for this would stand at the end: in three
                // builds one to five pieces left became 40 to 112) and without breaking out again to get anywhere
                // (that broke more than was mended; nothing follows this pass, so the bot need not get back).
                if (brokenToMend() > 0 && !timeShort()) {
                    int missing = brokenToMend();
                    unreachable.clear();
                    badStand.clear();
                    mendOnly = true;
                    noScaffoldNav = true;
                    try { deferredPass(false); } finally { noScaffoldNav = false; mendOnly = false; }
                    undoRope();
                    reconcile();
                    mended += missing - brokenToMend();
                }
            }
            for (int cell : deferred) if (needs(cell)) skip(cell, "no way to place");
            if (verbose && !scaffold.isEmpty()) {
                // what is the bot's situation, with scaffolding still standing?
                Site.Stock first = stock.get(0);
                boolean plain = search(first.chestX, g.oy, first.chestZ + 1, 200000);
                int e1 = nav.expanded;
                nav.mine = this::mineTicks; nav.effort = 20;
                boolean mining = search(first.chestX, g.oy, first.chestZ + 1, 200000);
                int e2 = nav.expanded;
                desperate = true;
                boolean anyhow = search(first.chestX, g.oy, first.chestZ + 1, 200000);
                desperate = false;
                nav.mine = null; nav.effort = 1;
                StringBuilder sb = new StringBuilder("END with " + scaffold.size() + " scaffold blocks left; way to the chests: plain=" + plain + " (" + e1 + ") breaking=" + mining + " (" + e2 + ") stand=" + g.stand16(cx(), cy(), cz()) + " carried loose/scaffolding=" + count(scaffoldItem) + "/" + count(towerItem) + " in the chests=" + inDepot(scaffoldItem) + "/" + inDepot(towerItem) + " stranded=" + stranded + " around (y+2..y-1; rows z-1..z+1, cols x-1..x+1):");
                for (int dy = 2; dy >= -1; dy--) { sb.append(" | y").append(dy >= 0 ? "+" : "").append(dy).append(':'); for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) sb.append(' ').append(StateInfo.name(g.get(cx() + dx, cy() + dy, cz() + dz))); }
                log(sb.toString());
            }
            status = "done";
        } catch (OutOfTime e) {
            status = died ? "died" : ticks >= maxTicks ? "out of time" : "wall limit";
        }
        c.releaseKeys();
        c.holdAttack(false);
    }

    /** True when the ticks left are about what taking down the standing scaffolding will need. */
    boolean timeShort() { return maxTicks - ticks < 45L * scaffold.size() + 40000; }

    /** Cells the bot broke out of the build itself that are still missing. */
    int brokenToMend() {
        int n = 0;
        for (int c : deferred) if (brokenOut.contains(c) && needs(c)) n++;
        return n;
    }

    void deferredPass(boolean traps) {
        if (timeShort()) return;
        phase("deferred cells" + (traps ? " (traps accepted)" : "") + ": " + deferred.size());
        acceptTraps = traps;
        try {
            // nearest first, from wherever the last one left the bot (height counts double: every level is a climb)
            int[] cells = deferred.toIntArray();
            boolean[] done = new boolean[cells.length];
            for (int n = 0; n < cells.length; n++) {
                int bx = cx(), by = cy(), bz = cz(), k = -1;
                long best = Long.MAX_VALUE;
                for (int j = 0; j < cells.length; j++) {
                    if (done[j]) continue;
                    long d = Math.abs(g.xOf(cells[j]) - bx) + Math.abs(g.zOf(cells[j]) - bz) + 2L * Math.abs(g.yOf(cells[j]) - by);
                    if (d < best) { best = d; k = j; }
                }
                if (k < 0) break;
                done[k] = true;
                int cell = cells[k];
                if (timeShort()) break;
                if (!needs(cell)) { deferred.remove(cell); continue; }
                if (!lateToo && isLate(cell)) continue;
                if (mendOnly && !brokenOut.contains(cell)) continue;
                if (doCell(cell) != DEFER) deferred.remove(cell);
            }
        } finally {
            acceptTraps = false;
        }
    }

    /** Marks the start of a stage of the run in the log (with the tick, so that what each stage costs can be read off). */
    void phase(String what) { if (verbose) log("PHASE " + what + " | scaffold blocks standing: " + scaffold.size()); }

    boolean anyDeferred() {
        for (int cell : deferred) if (needs(cell)) return true;
        return false;
    }

    int scanColumn(int x, int fromY, int z) {
        for (int y = Math.max(fromY, g.oy); y < g.oy + g.sy; y++) {
            int i = g.idx(x, y, z);
            if (g.tgt[i] == 0 || !needs(i) || deferred.contains(i)) continue;
            // doors and the like wait until everything else is built
            if (isLate(i) || waitsForDoor(i)) { if (deferred.add(i)) deferReasons.merge(isLate(i) ? "late kind" : "door order", 1, Integer::sum); continue; }
            return y;
        }
        return NONE;
    }

    boolean eligible(int x, int z) {
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
    int tilesX, tilesZ;
    int[] tilePending;

    void setNext(int x, int z, int y) {
        int c0 = col(x, z);
        int old = colNext[c0];
        colNext[c0] = y;
        if ((old == NONE) != (y == NONE)) tilePending[((z - g.oz) >> 3) * tilesX + ((x - g.ox) >> 3)] += y == NONE ? -1 : 1;
    }

    /** The allowed cell nearest to the bot, or -1. */
    int pickNext() {
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
    int escapeBudget = 40;

    long progressMark = -1, progressTick;

    void work(int i) {
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        layerNow = y;
        // watchdog: a long time without a single block placed or removed means the bot is going in circles
        if (placed + broken != progressMark) { progressMark = placed + broken; progressTick = ticks; }
        if (ticks - progressTick > 4000) { progressTick = ticks; skip(i, "stuck"); setNext(x, z, scanColumn(x, y + 1, z)); badStand.clear(); return; }
        monitorExit();
        guardHealth();
        int r = doCell(i);
        reconcile();
        if (PARANOID && paranoidLogs < 3 && c.onGround() && g.stand16(cx(), cy(), cz()) >= 0 && !wayOut(cx(), cy(), cz())) {
            paranoidLogs++;
            StringBuilder sb = new StringBuilder("PARANOID: no way out after " + rel(x, y, z) + " " + StateInfo.name(g.tgt[i]) + " result=" + r + " stand=" + g.stand16(cx(), cy(), cz()) + "; around (y+3..y-1; rows z-1..z+1, cols x-1..x+1):");
            for (int dy = 3; dy >= -1; dy--) { sb.append(" | y").append(dy >= 0 ? "+" : "").append(dy).append(':'); for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) sb.append(' ').append(StateInfo.name(g.get(cx() + dx, cy() + dy, cz() + dz)).replaceAll("\\[.*", "")); }
            log(sb.toString());
        }
        // placed but not yet in its final form (a slab still to be doubled, a pot still empty ...): keep it on the list
        if (r == PLACED && needs(i)) r = DEFER;
        if (r == DEFER) {
            // after a few vain tries the cell goes to the end of the queue so that the rest can go on
            if (tries.addTo(i, 1) >= 12) { deferred.add(i); deferReasons.merge(deferWhy, 1, Integer::sum); blocked.remove(i); setNext(x, z, scanColumn(x, y + 1, z)); }
            else { blocked.put(i, surroundings(i)); blockedWhy.merge(deferWhy, 1, Integer::sum); }
            return;
        }
        blocked.remove(i);
        setNext(x, z, scanColumn(x, y + 1, z));
        doorsReady();
        if (r == PLACED && !deferred.isEmpty()) {
            // something that was waiting for this block?
            for (int d = 0; d < 6; d++) {
                int nx = x + DX[d], ny = y + DY[d], nz = z + DZ[d];
                if (!g.in(nx, ny, nz)) continue;
                int n = g.idx(nx, ny, nz);
                if (deferred.contains(n) && needs(n) && !isLate(n) && count(StateInfo.item[g.tgt[n]]) > 0 && doCell(n) != DEFER) deferred.remove(n);
            }
        }
    }

    /**
     * Doors that waited for the blocks beside them, and blocks that waited for a door (see waitsForDoor): once what
     * they waited for stands - however it got there - they go back into their column's queue, so that their material
     * is fetched and they are placed while the bot can still get at them.
     */
    void doorsReady() {
        while (!doorEvents.isEmpty()) {
            int p = doorEvents.removeInt(doorEvents.size() - 1);
            int[] waiters = doorWaiters.get(p);
            if (waiters == null || needs(p)) continue;
            for (int n : waiters) {
                if (!deferred.contains(n) || !needs(n) || isLate(n) || waitsForDoor(n)) continue;
                deferred.remove(n);
                int wx = g.xOf(n), wz = g.zOf(n), wy = g.yOf(n), cur = colNext[col(wx, wz)];
                if (cur == NONE || cur > wy) setNext(wx, wz, wy);
            }
        }
    }

    static final boolean MONITOR = System.getenv("MCB_NO_MONITOR") == null;
    private long lastMonitor = -100000;
    private boolean wasShut;

    /**
     * The safety invariant, sampled: from where the bot stands there is a way back to the chests (2 = with what
     * stands, 1 = with new scaffolding, 0 = none). Counts episodes of "none" and when the first began. Reads only:
     * the searches it makes leave no trace in what the bot remembers (checked: same ticks with and without).
     */
    void monitorExit() {
        if (!MONITOR || ticks - lastMonitor < 600 || !c.onGround() || !g.in(cx(), cy(), cz()) || g.stand16(cx(), cy(), cz()) < 0) return;
        lastMonitor = ticks;
        int tc = (int) trapChecks;
        int level = exitLevel(cx(), cy(), cz());
        trapChecks = tc;
        exitSamples[level]++;
        boolean shut = level == 0;
        if (shut && !wasShut) { shutEpisodes++; if (firstShut == 0) firstShut = ticks; }
        wasShut = shut;
    }

    long retreats;
    long retreatCount() { return retreats; }

    /**
     * Hurt (a magma floor, a brush with a campfire): get off whatever does it and wait until the damage has mended
     * (the world is peaceful: health comes back by itself).
     */
    void guardHealth() {
        if (c.player.getHealth() >= 10 || c.player.isDeadOrDying()) return;
        retreats++;
        if (verbose && retreats <= 5) log("hurt (health " + c.player.getHealth() + "): stepping aside to recover");
        if (!rope.isEmpty()) undoRope();
        int bx = cx(), by = cy(), bz = cz();
        if (!safeSpot(bx, by, bz)) {
            int best = -1, bestD = Integer.MAX_VALUE;
            for (int dy = -3; dy <= 3; dy++) for (int dz = -8; dz <= 8; dz++) for (int dx = -8; dx <= 8; dx++) {
                int x = bx + dx, y = by + dy, z = bz + dz;
                if (!g.in(x, y, z) || !safeSpot(x, y, z)) continue;
                int d = Math.abs(dx) + Math.abs(dz) + 2 * Math.abs(dy);
                if (d < bestD && (d < 6 || tryCount++ < 40) && search(x, y, z, 800) && nav.scaffoldMoves == 0 && nav.towerMoves == 0) { bestD = d; best = g.idx(x, y, z); }
            }
            tryCount = 0;
            if (best >= 0 && search(g.xOf(best), g.yOf(best), g.zOf(best), 800)) travel();
        }
        for (int n = 0; n < 500 && c.player.getHealth() < 18 && !c.player.isDeadOrDying(); n++) tick();
    }

    private int tryCount;

    boolean safeSpot(int x, int y, int z) {
        if (g.stand16(x, y, z) < 0) return false;
        int below = g.get(x, y - 1, z);
        return !StateInfo.hot[below] && !StateInfo.hot[g.get(x, y, z)];
    }

    static final boolean PARANOID = System.getenv("MCB_PARANOID") != null;
    int paranoidLogs;

    int surroundings(int i) {
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i), h = 17;
        // (temporary blocks do not count as a change worth another try)
        for (int dy = -1; dy <= 1; dy++) for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) {
            int nx = x + dx, ny = y + dy, nz = z + dz;
            h = h * 31 + (g.in(nx, ny, nz) && scaffold.contains(g.idx(nx, ny, nz)) ? 0 : g.get(nx, ny, nz));
        }
        return h;
    }

    int doCell(int i) {
        if (WATCH != null && g.xOf(i) - g.ox == WATCH[0] && g.yOf(i) - g.oy == WATCH[1] && g.zOf(i) - g.oz == WATCH[2]) log("WATCH doCell: waitsForDoor=" + waitsForDoor(i) + " acceptTraps=" + acceptTraps + " deferred=" + deferred.contains(i));
        if (waitsForDoor(i)) { deferWhy = "door order"; return DEFER; }
        checkScaffold();
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        int t = g.tgt[i];
        Spec sp = spec(t);
        if (!sp.placeable) { skip(i, "cannot be placed by hand: " + StateInfo.state[t].getBlock().getDescriptionId().replace("block.minecraft.", "")); return SKIP; }
        int w = g.cur[i];
        if (w != 0) {
            if (scaffold.contains(i)) { long kept = keptForExit; if (!removeScaffold(i)) { if (keptForExit > kept) { deferWhy = "own way out stands in the cell"; return DEFER; } skip(i, "scaffold in the way"); return SKIP; } }
            else if (sp.multi && StateInfo.state[w].getBlock() == StateInfo.state[t].getBlock()) { deferWhy = "top-up failed"; return topUp(x, y, z, t, sp) ? PLACED : DEFER; }
            else if (sp.content != null && w == sp.first) { fillPot(x, y, z, t, sp); if (matches(g.get(x, y, z), t)) return PLACED; skip(i, "pot could not be filled"); return SKIP; }
            else if (StateInfo.loose[w] == StateInfo.loose[t]) {
                // there, but not set the way it should be (a door that should stand open, a repeater's delay ...)
                for (int attempt = 0; attempt < 4 && !matches(g.get(x, y, z), t); attempt++) {
                    if (!c.onGround()) settle();
                    if (!aimAtBlock(x, y, z)) {
                        Stance st = planStance(x, y, z, null, true);
                        if (st == null || !goTo(st)) break;
                        if (!aimAtBlock(x, y, z)) { badStand.add(g.idx(st.x, st.y, st.z)); continue; }
                    }
                    adjust(x, y, z, t);
                }
                if (matches(g.get(x, y, z), t)) return PLACED;
                skip(i, "could not be set right"); return SKIP;
            }
            else { skip(i, "occupied by another block"); return SKIP; }
        }
        if (count(sp.item) == 0 || (count(scaffoldItem) < 12 && !stranded)) {
            // cut off from the chests a moment ago: this cell waits; the way there is tried again later
            if (stranded && count(sp.item) == 0 && ticks - strandedTick < 1200) { deferWhy = "stranded"; return DEFER; }
            mustFetch = sp.item;
            if (verbose) log("restock wanted: " + (count(sp.item) == 0 ? "no " + sp.item : "loose blocks low (" + count(scaffoldItem) + ")") + " placed=" + placed + " for " + rel(x, y, z) + " free=" + freeSlots());
            restock(y);
            mustFetch = null;
            if (count(sp.item) == 0) {
                if (verbose) { int inStock = 0, entries = 0; for (Site.Stock st : stock) if (st.item == sp.item) { inStock += st.count; entries++; } log("OUT OF STOCK " + sp.item + ": depot has " + inStock + " in " + entries + " slots; free inventory slots " + freeSlots() + "; " + inventorySummary()); }
                skip(i, stranded ? "stranded without material" : "out of stock"); return SKIP;
            }
        }
        if (!clearBelow(x, y, z) && noRoofing) { deferWhy = "scaffold below"; return DEFER; }
        // scaffolding that is part of the build must not hang on the bot's own columns (it would come down with them)
        if (StateInfo.scaffolding[t]) {
            boolean held = false;
            int below = g.get(x, y - 1, z);
            boolean mineBelow = g.in(x, y - 1, z) && scaffold.contains(g.idx(x, y - 1, z));
            if (!mineBelow && (StateInfo.scaffolding[below] || StateInfo.sturdyTop[below])) held = true;
            for (int d = 0; d < 4 && !held; d++) {
                int nx = x + DXH[d], nz = z + DZH[d];
                held = g.isTower(nx, y, nz) && !scaffold.contains(g.idx(nx, y, nz));
            }
            if (!held) { deferWhy = "target scaffolding unsupported"; return DEFER; }
        }
        // TNT next to anything that carries power would go off the moment it is placed
        if (StateInfo.state[t].getBlock() instanceof net.minecraft.world.level.block.TntBlock && c.level().hasNeighborSignal(new net.minecraft.core.BlockPos(x, y, z))) {
            skip(i, "would be set off"); return SKIP;
        }
        // sand, gravel, anvils ...: only onto something that will stay there
        if (StateInfo.gravity[t]) {
            int below = g.get(x, y - 1, z);
            if (below == 0 || StateInfo.walkThrough[below] || StateInfo.replaceable[below] || (g.in(x, y - 1, z) && scaffold.contains(g.idx(x, y - 1, z)))) { deferWhy = "gravity block without support"; return DEFER; }
        }
        int r = attempt(x, y, z, t, sp, true);
        // (the last resorts below wait until everything else stands: see placeHelper)
        boolean noFace = mainDone && r == DEFER && "no face to click from anywhere".equals(deferWhy);
        if (r == DEFER) {
            // no face to click that gives the wanted state: lend it one with a temporary block, then take that away
            int helper = placeHelper(x, y, z, sp);
            if (helper >= 0) {
                int base = helperBase;
                helperCell = helper;
                try { r = attempt(x, y, z, t, sp, false); } finally { helperCell = -1; }
                mustRemove = true;
                try { removeScaffold(helper); if (base >= 0) removeScaffold(base); } finally { mustRemove = false; }
                if (r == PLACED) { helped++; if (base >= 0) chained++; }
            }
        }
        // still nowhere to click from: from inside the cell itself, if the thing is thin enough to stand beside
        if (r == DEFER && noFace && placeFromInside(x, y, z, t, sp)) { fromInside++; r = PLACED; }
        // ... or from right beside it, off the middle of the block and sneaking (things that want a steep view)
        if (r == DEFER && noFace && placeClose(x, y, z, t, sp)) { fromClose++; r = PLACED; }
        return r;
    }

    /** Tries to place the cell from here or from a planned stance. */
    int attempt(int x, int y, int z, int t, Spec sp, boolean mayEscape) {
        wantHeld = sp.item;
        sealRefused = false;
        if (bodyIn(x, y, z)) {
            if (selfPlace(x, y, z, t, sp)) return PLACED;
        } else if (placeFromHere(x, y, z, t, sp)) return PLACED;
        // go somewhere better
        try {
        for (int attempt = 0; attempt < 3; attempt++) {
            avoidSeal = sealRefused && !acceptTraps ? sp.first : 0;
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
                deferWhy = sealRefused ? "would shut the bot in" : dbgClick + dbgPillarClick == 0 ? "no face to click from anywhere" : "no way to a spot";
                return DEFER;
            }
            if (!goTo(st)) { replans++; badStand.add(g.idx(st.x, st.y, st.z)); deferWhy = "could not get to the spot"; continue; }
            if (bodyIn(x, y, z)) { if (selfPlace(x, y, z, t, sp)) { badStand.clear(); return PLACED; } }
            else if (placeFromHere(x, y, z, t, sp)) { badStand.clear(); endLean(); return PLACED; }
            endLean();
            badStand.add(g.idx(st.x, st.y, st.z) ^ (st.lean >= 0 ? (st.lean + 1) << 28 : 0));
            deferWhy = sealRefused ? "would shut the bot in" : "click failed at the spot";
        }
        return DEFER;
        } finally { avoidSeal = 0; }
    }

    /**
     * Anything that turned up where it does not belong - a block in a cell the target leaves empty, or of another
     * kind than the target wants - and is not known scaffolding is treated like scaffolding from now on: removed
     * before the right block goes in, or at the end. (Sand that slid, a click that landed on another face ...)
     */
    void reconcile() {
        if (!changedAgain.isEmpty()) {
            for (int k = 0; k < changedAgain.size(); k++) {
                int i = changedAgain.getInt(k);
                if (needs(i) && !skipped.get(i)) deferred.add(i);
            }
            changedAgain.clear();
        }
        if (appeared.isEmpty()) return;
        for (int k = 0; k < appeared.size(); k++) {
            int i = appeared.getInt(k);
            int w = g.cur[i];
            if (w == 0 || scaffold.contains(i)) continue;
            int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
            if (y < g.oy) continue;
            int t = g.tgt[i];
            // (a piston's head is part of the piston: breaking it would take the piston with it)
            // (nor is the upper half of a door, the head of a bed ... that came with its other part, even where the
            // target leaves that cell empty: breaking it would take the whole thing)
            boolean wrong = !StateInfo.technical[w] && !StateInfo.secondary[w] && (t == 0 ? !isSupplyChest(x, y, z) : !sameKind(w, t) && !sameFamily(w, t) && w != spec(t).first);
            if (!wrong) continue;
            strays++;
            if (verbose && strays < 20) log("STRAY " + StateInfo.name(w) + " at " + rel(x, y, z) + " (target " + StateInfo.name(t) + ")");
            scaffold.add(i);
            scaffoldInColumn[col(x, z)]++;
        }
        appeared.clear();
    }

    /** While true, a block is not put on top of scaffolding that is still there (it waits instead). */
    boolean noRoofing = true;
}
