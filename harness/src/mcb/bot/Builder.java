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
            mainLoop();
            // WALK_WAY: if none of the walkway is left hanging, the clean-up is the one it was (Rust's `walkdown` line before
            // this one - undo_rope and wings_down when wing_recs is not empty - is left out: wing_recs stays empty without it)
            if (!wingsHang) setHangFloor(false);
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
            endGame();
            if (REPAIR) repair();
            // OWN_DOWN: the last word on the scaffolding is had from the ground. The rounds above can end with the bot
            // up on the build, on what it put up to get there - and from up there it cannot take its own way down
            // away. It goes back to the chests, by whatever that takes, and takes down what can be had from there with
            // nothing new put up. (OD_FINAL: off, see Scaffolds.)
            if (Scaffolds.OWN_DOWN && Scaffolds.OD_FINAL && !scaffold.isEmpty() && !timeShort()) {
                comeDown();
                unreachable.clear();
                badStand.clear();
                noScaffoldNav = true;
                try { cleanup(); } finally { noScaffoldNav = false; }
                undoRope();
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
    /** The main programme: the nearest allowed cell, again and again, until none is left or time is short. */
    void mainLoop() {
            int guard = 0;
            while (true) {
                // near the end of the time allowed: what stands must still be cleared of scaffolding, and a few
                // blocks short is better than scaffolding all over
                if (timeShort()) { if (verbose) log("time is running out: " + (maxTicks - ticks) + " ticks left, " + scaffold.size() + " scaffold blocks standing - on to the clean-up"); break; }
                // COVER_TOUR: the spot nearby that serves the most cells first (coverTour)
                if (coverTour()) { guard = 0; continue; }
                int i = pickNext();
                if (i < 0) {
                    // everything that may go next has been tried and found no way: let those cells wait (they may
                    // hang from something built later) and move on above them
                    if (blocked.isEmpty()) break;
                    boolean any = false;
                    for (int cell : blocked.keySet().toIntArray()) {
                        blocked.remove(cell);
                        if (!needs(cell)) continue;
                        // Before the cell is put off until everything else stands: one more try, with the helper block
                        // that may rest on a second helper (placeHelper). By the time the passes after the main one
                        // get to it, an upright log that hangs in the air is walled in by its leaves, and every log
                        // of the trunk above it has waited in vain for the face of the one below. (Not in machines.)
                        if (!hasPistons()) {
                            int r;
                            helperChainNow = true;
                            try { r = doCell(cell); } finally { helperChainNow = false; }
                            reconcile();
                            if ((r != DEFER && !needs(cell)) || r == SKIP) {
                                if (r != SKIP) orphans++;
                                int x = g.xOf(cell), z = g.zOf(cell);
                                setNext(x, z, scanColumn(x, g.yOf(cell) + 1, z));
                                any = true;
                                continue;
                            }
                        }
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
    }

    // ------------------------------------------------------------------ COVER_TOUR (Rust cover.rs)
    // Stands chosen by the cells they serve, in the main pass of a build behind its pace. Measured in the simulator on
    // huge builds out of time: most stands are on the build's own finished blocks already, but each serves 3.7-5.6
    // cells, while the best spot within four blocks of a departure would serve 8-11 at once under the bot's own rules.
    // So: before the next cell is picked, a walk (nothing new put up) to the spot nearby that serves the most cells
    // now, and sweepHere there.

    /** COVER_TOUR (Rust `cover_on`): the main pass of a build behind its pace. The terms in the Rust order, behindPace()
     *  last (it latches); Rust's ckpt::armed has no Java counterpart. */
    boolean coverOn() {
        return COVER_TOUR && !mainDone && !cleaning && colNext != null && behindPace();
    }

    /** COVER_TOUR: scratch click for coverAt (Rust saves and restores `self.click` around its looks instead). */
    private final Click coverClick = new Click();

    /**
     * COVER_TOUR (Rust `cover_at`): the cells other than {@code skip} that a click from the eye of one standing at
     * (sx,sy,sz) would place now under the bot's own rules: the next cell of its column, the layer rule kept, a kind it
     * carries, not put off, a falling block only on something that holds it, not one the sweep passes over (loose
     * block below in a column with scaffolding), in reach and with a face to click. Stops counting at {@code cap}.
     */
    long coverAt(int sx, int sy, int sz, int skip, long cap) {
        int h = g.stand16(sx, sy, sz);
        if (h < 0) return 0;
        double ex = sx + 0.5, ey = sy + h / 16.0 + 1.62, ez = sz + 0.5;
        double r2 = (reach + 0.9) * (reach + 0.9);
        long n = 0;
        all:
        for (int z = sz - 4; z <= sz + 4; z++) for (int x = sx - 4; x <= sx + 4; x++) {
            if (!g.inBox(x, g.oy, z)) continue;
            int y = colNext[col(x, z)];
            if (y == NONE || y > sy + 5 || y < sy - 4) continue;
            if (x == sx && z == sz && (y == sy || y == sy + 1)) continue;
            int i = g.idx(x, y, z);
            if (i == skip || g.cur[i] != 0 || deferred.contains(i)) continue;
            double cx = x + 0.5 - ex, cy = y + 0.5 - ey, cz = z + 0.5 - ez;
            if (cx * cx + cy * cy + cz * cz > r2) continue;
            int t = g.tgt[i];
            Spec sp = spec(t);
            if (!sp.placeable || count(sp.item) == 0 || !eligible(x, z)) continue;
            if (StateInfo.gravity[t]) { int b = g.get(x, y - 1, z); if (b == 0 || StateInfo.walkThrough[b] || StateInfo.replaceable[b]) continue; }
            if (blocked.containsKey(i) && blocked.get(i) == surroundings(i)) continue;
            if (scaffoldInColumn[col(x, z)] != 0 && looseBelow(x, y, z)) continue;
            if (findClick(x, y, z, sp.first, sp.table, ex, ey, ez, false, 63, coverClick)) { n++; if (n >= cap) break all; }
        }
        return n;
    }

    /** COVER_TOUR: score weight of a cell in quarters of a block (Rust MCSIM_COVW), least cells a spot must serve
     *  (MCSIM_COVK), how far across (MCSIM_COVR) and up or down (MCSIM_COVDY) spots are looked for: the Rust defaults. */
    static final int COVER_W = 8, COVER_K = 3, COVER_R = 4, COVER_DY = 3;

    /**
     * COVER_TOUR (Rust `cover_tour`): one step of the tour - the spot within COVER_R blocks (COVER_DY up or down) that
     * serves the most cells now (COVER_W quarters of a block a cell), less its distance, if it serves COVER_K or more;
     * walked to with nothing new put up and swept. Of the three best spots, the first one a route is found to is gone
     * to. True when target cells were placed there.
     */
    boolean coverTour() {
        if (!coverOn()) return false;
        guardHealth();
        // what is allowed from here goes first (the step must not walk off from cells in reach)
        sweepHere();
        if (!c.onGround()) return false;
        int bx = cx(), by = cy(), bz = cz();
        long now = ticks;
        for (var it = coverDud.int2LongEntrySet().fastIterator(); it.hasNext(); ) if (it.next().getLongValue() <= now) it.remove();
        // the three best spots, best first; a spot goes after those with the same score found before it (Rust: push,
        // stable sort by score descending, truncate to 3; a spot no better than the third of three is not taken)
        int[][] cands = new int[3][];
        int nc = 0;
        for (int sy = by - COVER_DY; sy <= by + COVER_DY; sy++) for (int sz = bz - COVER_R; sz <= bz + COVER_R; sz++) for (int sx = bx - COVER_R; sx <= bx + COVER_R; sx++) {
            if ((sx == bx && sy == by && sz == bz) || !g.in(sx, sy, sz) || g.stand16(sx, sy, sz) < 0) continue;
            int k = g.idx(sx, sy, sz);
            if (badStand.contains(k) || coverDud.containsKey(k)) continue;
            // no stand on a piece of the bot's own: the tour goes over the build and the ground
            if (g.in(sx, sy - 1, sz) && scaffold.contains(g.idx(sx, sy - 1, sz))) continue;
            int e = Math.max(Math.abs(sx - bx), Math.abs(sz - bz)) + 2 * Math.max(0, sy - by) + Math.max(0, by - sy);
            long n = coverAt(sx, sy, sz, -1, 64);
            if (n < COVER_K) continue;
            int score = COVER_W * (int) n - 4 * e;
            if (nc >= 3 && score <= cands[2][3]) continue;
            int p = Math.min(nc, 2);
            while (p > 0 && cands[p - 1][3] < score) { if (p < 3) cands[p] = cands[p - 1]; p--; }
            cands[p] = new int[] {sx, sy, sz, score};
            if (nc < 3) nc++;
        }
        for (int j = 0; j < nc; j++) {
            int sx = cands[j][0], sy = cands[j][1], sz = cands[j][2];
            // (the way there, and the route goTo makes again, with nothing new put up: no column, pillar or bridge)
            boolean gone;
            noScaffoldNav = true;
            try {
                if (!route(sx, sy, sz, 4000)) continue;
                coverMoves++;
                Stance st = new Stance();
                st.x = sx; st.y = sy; st.z = sz; st.lean = -1; st.cost = nav.goalCost;
                gone = goTo(st);
            } finally { noScaffoldNav = false; }
            int dud = g.idx(sx, sy, sz);
            if (!gone) { coverFails++; coverDud.put(dud, now + 20000); return false; }
            // (counted: target cells the sweep placed, not blocks put down on the way)
            long s0 = coverSwept;
            sweepHere();
            long got = coverSwept - s0;
            coverPlaced += got;
            if (got == 0) { coverFails++; coverDud.put(dud, now + 20000); return false; }
            return true;
        }
        return false;
    }

    /** Everything after the build proper: the clean-up rounds, the put-off cells in between, the rounds that may
     *  break through, and the mending of what they broke. */
    void endGame() {
            cleanup();
            websToo = true;
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
    }

    // ------------------------------------------------------------------------------------------------ the repair pass
    /** On unless MCB_NO_REPAIR is set (simulator revision "repair", bot/repair.rs; from the owner's monitor,
     *  FROM-MONITOR-repair.md): when the programme is over and sections still fail by the bot's own picture, it
     *  takes them up again with its own routines. Up to three rounds: if more than six sections fail and more than
     *  150,000 ticks are left the whole build goes back into the main programme (the bot had stopped early);
     *  otherwise the failing sections with the fewest bad cells, up to 24, sections with machine parts left alone.
     *  Their bad cells are forgotten as "given up" and built again by the main loop, then tried with the helper rules
     *  and with the trap rules; then the whole end game runs again. A round that does not lower the bad cells is the
     *  last. */
    static final boolean REPAIR = System.getenv("MCB_NO_REPAIR") == null;

    /** What the judge would say of a cell, by the bot's picture: 0 right or not judged, 1 missing, 2 wrong, 3 left over. */
    int badKind(int i) {
        int a = g.tgt[i], w = g.cur[i];
        if (StateInfo.air[w]) w = 0;
        if (a != 0 && (StateInfo.decorative(a) || StateInfo.gameMaster(a))) return 0;
        if (a == 0) return w != 0 && !StateInfo.fluid[w] && !StateInfo.technical[w] && !StateInfo.secondary[w] ? 3 : 0;
        if (w == 0) return 1;
        return mcb.build.Judge.cellOk(a, w) ? 0 : 2;
    }

    /** The failing sections as {bad, missing + wrong, machine parts (0/1), x0, y0, z0, x1, y1, z1}; total[0] gets the bad cells of the build. */
    java.util.ArrayList<int[]> failingSections(long[] total) {
        java.util.ArrayList<int[]> out = new java.util.ArrayList<>();
        int nx = (g.sx + 15) >> 4, ny = (g.sy + 15) >> 4, nz = (g.sz + 15) >> 4;
        for (int cy = 0; cy < ny; cy++) for (int cz = 0; cz < nz; cz++) for (int cx = 0; cx < nx; cx++) {
            int x0 = g.ox + (cx << 4), y0 = g.oy + (cy << 4), z0 = g.oz + (cz << 4);
            int x1 = g.ox + Math.min(g.sx, (cx + 1) << 4) - 1, y1 = g.oy + Math.min(g.sy, (cy + 1) << 4) - 1, z1 = g.oz + Math.min(g.sz, (cz + 1) << 4) - 1;
            int tgt = 0, bad = 0, mw = 0, machine = 0;
            for (int y = y0; y <= y1; y++) for (int z = z0; z <= z1; z++) for (int x = x0; x <= x1; x++) {
                int i = g.idx(x, y, z), a = g.tgt[i];
                if (a != 0 && !StateInfo.decorative(a) && !StateInfo.gameMaster(a)) tgt++;
                if (a != 0) {
                    net.minecraft.world.level.block.Block bl = StateInfo.state[a].getBlock();
                    if (bl instanceof net.minecraft.world.level.block.piston.PistonBaseBlock || bl instanceof net.minecraft.world.level.block.piston.PistonHeadBlock
                            || bl instanceof net.minecraft.world.level.block.ObserverBlock || bl == net.minecraft.world.level.block.Blocks.SLIME_BLOCK || bl == net.minecraft.world.level.block.Blocks.HONEY_BLOCK) machine = 1;
                }
                int k = badKind(i);
                if (k != 0) { bad++; if (k != 3) mw++; }
            }
            total[0] += bad;
            if (tgt == 0 ? bad > 0 : (double) bad / tgt > 0.03) out.add(new int[]{bad, mw, machine, x0, y0, z0, x1, y1, z1});
        }
        return out;
    }

    /** Puts the bad target cells of a box back into the ordinary programme: no longer given up, put off or blocked,
     *  and the columns' next cell set back to the lowest of them. */
    int requeue(int x0, int y0, int z0, int x1, int y1, int z1) {
        int n = 0;
        for (int y = y0; y <= y1; y++) for (int z = z0; z <= z1; z++) for (int x = x0; x <= x1; x++) {
            int i = g.idx(x, y, z), k = badKind(i);
            if (k != 1 && k != 2) continue;
            if (skipped.get(i)) { skipped.clear(i); skippedCells--; }
            if (!needs(i)) continue;
            deferred.remove(i);
            tries.remove(i);
            blocked.remove(i);
            n++;
        }
        for (int z = z0; z <= z1; z++) for (int x = x0; x <= x1; x++) {
            int y = scanColumn(x, y0, z);
            if (y != NONE && y <= y1 && y < colNext[col(x, z)]) setNext(x, z, y);
        }
        return n;
    }

    /** On unless MCB_NO_EXCAVATE is set: in the repair pass, cells still bad and shut in by standing blocks are reached
     *  by a short shaft (excavate). */
    static final boolean EXCAVATE = System.getenv("MCB_NO_EXCAVATE") == null;

    boolean inBuild(int x, int y, int z) { return x >= g.ox && x < g.ox + g.sx && y >= g.oy && y < g.oy + g.sy && z >= g.oz && z < g.oz + g.sz; }

    /** Whatever stands in the cell is from now on a block to take away (as the bot treats strays). */
    boolean markStray(int i) {
        if (g.cur[i] == 0 || scaffold.contains(i)) return g.cur[i] != 0;
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        if (y < g.oy || isSupplyChest(x, y, z) || StateInfo.technical[g.cur[i]] || StateInfo.secondary[g.cur[i]]) return false;
        scaffold.add(i);
        scaffoldInColumn[col(x, z)]++;
        return true;
    }
    void unmarkStray(int i) { if (scaffold.remove(i)) scaffoldInColumn[col(g.xOf(i), g.zOf(i))]--; }

    /** Breaks whatever stands in a cell; true when the cell is empty afterwards. */
    boolean takeOut(int i) {
        if (g.cur[i] == 0) return true;
        boolean had = scaffold.contains(i);
        if (!markStray(i)) return false;
        boolean gone;
        mustRemove = true;
        try { gone = removeScaffold(i); } finally { mustRemove = false; }
        if (g.cur[i] != 0) { if (!had) unmarkStray(i); return false; }
        return gone;
    }

    /** One more try at a cell with the ordinary routine; true when it stands right afterwards (deferWhy says why not). */
    boolean tryCell(int i) {
        if (g.tgt[i] == 0) return false;
        if (skipped.get(i)) { skipped.clear(i); skippedCells--; }
        if (badKind(i) == 2 && !takeOut(i)) { deferWhy = "the wrong block there could not be taken out"; return false; }
        if (!needs(i)) return badKind(i) == 0;
        int r = doCell(i);
        reconcile();
        if (r == PLACED && !needs(i)) { deferred.remove(i); return true; }
        if (r != SKIP) deferred.add(i);
        return false;
    }

    /** Straight shafts to a cell from open space, fewest blocks first (up before sideways): the cells to break,
     *  outermost first. Open space is two free cells in a row beyond the last block, or the edge of the build. */
    java.util.ArrayList<int[]> shaftsFor(int i, int maxDepth) {
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        int[][] D = {{0, 1, 0}, {1, 0, 0}, {-1, 0, 0}, {0, 0, 1}, {0, 0, -1}};
        java.util.ArrayList<int[]> found = new java.util.ArrayList<>();
        java.util.ArrayList<Integer> cost = new java.util.ArrayList<>();
        for (int k = 0; k < D.length; k++) {
            it.unimi.dsi.fastutil.ints.IntArrayList solid = new it.unimi.dsi.fastutil.ints.IntArrayList();
            int air = 0;
            boolean open = false;
            for (int step = 1; step <= maxDepth + 8; step++) {
                int nx = x + D[k][0] * step, ny = y + D[k][1] * step, nz = z + D[k][2] * step;
                if (!g.in(nx, ny, nz)) { open = air >= 1 || solid.isEmpty(); break; }
                int n = g.idx(nx, ny, nz), w = g.cur[n], a = g.tgt[n];
                // another hole on the way: nothing to break, but not open space either
                if (w == 0 && inBuild(nx, ny, nz) && a != 0 && !StateInfo.decorative(a) && !StateInfo.gameMaster(a)) continue;
                if (w == 0 || StateInfo.walkThrough[w]) {
                    if (++air >= 2 || !inBuild(nx, ny, nz)) { open = true; break; }
                } else {
                    if (isSupplyChest(nx, ny, nz) || ny < g.oy || StateInfo.technical[w] || StateInfo.secondary[w]) break;
                    air = 0;
                    solid.add(n);
                    if (solid.size() > maxDepth) break;
                }
            }
            if (!open || solid.isEmpty() || solid.size() > maxDepth) continue;
            int[] shaft = new int[solid.size()];
            for (int q = 0; q < shaft.length; q++) shaft[q] = solid.getInt(shaft.length - 1 - q);
            int c = shaft.length * 2 + (k == 0 ? 0 : 1), at = 0;
            while (at < cost.size() && cost.get(at) <= c) at++;   // (stable: equal costs stay in the order up, east, west, south, north)
            found.add(at, shaft); cost.add(at, c);
        }
        return found;
    }

    /** For bad cells shut in by blocks that stand around them: a short straight shaft from the nearest open side, the
     *  cell put right through it, and the shaft closed again from the inside out - what a player does to mend a hole
     *  inside a wall. Only where every block taken out can be put back from the stock (what is broken drops nothing).
     *  Returns the cells put right. */
    int excavate(int[] cells) {
        undoRope();
        unreachable.clear();
        badStand.clear();
        boolean[] done = new boolean[cells.length];
        int fixed = 0;
        for (int n = 0; n < cells.length && n < 300; n++) {
            if (timeShort()) break;
            int bx = cx(), by = cy(), bz = cz(), k = -1;
            long best = Long.MAX_VALUE;
            for (int j = 0; j < cells.length; j++) {
                if (done[j]) continue;
                long d = Math.abs(g.xOf(cells[j]) - bx) + Math.abs(g.zOf(cells[j]) - bz) + 2L * Math.abs(g.yOf(cells[j]) - by);
                if (d < best) { best = d; k = j; }
            }
            if (k < 0) break;
            done[k] = true;
            int i = cells[k];
            if (badKind(i) == 0) continue;
            if (tryCell(i)) { fixed++; continue; }
            // a shaft only helps where the trouble is getting at the cell
            String why = String.valueOf(deferWhy);
            if (!(why.startsWith("no face") || why.startsWith("no way") || why.startsWith("could not get") || why.startsWith("click failed") || why.startsWith("would shut") || why.startsWith("the wrong block"))) continue;
            boolean ok = false;
            for (int[] shaft : shaftsFor(i, 4)) {
                // only what can be put back (one of the item for each block taken out, and one for the cell)
                java.util.HashMap<net.minecraft.world.item.Item, Integer> need = new java.util.HashMap<>();
                for (int v : shaft) if (g.cur[v] != 0 && g.tgt[v] != 0) need.merge(StateInfo.item[g.tgt[v]], 1, Integer::sum);
                if (g.cur[i] == 0) need.merge(StateInfo.item[g.tgt[i]], 1, Integer::sum);
                boolean enough = true;
                for (var e : need.entrySet()) if (count(e.getKey()) + inDepot(e.getKey()) < e.getValue() + 1) enough = false;
                if (!enough) continue;
                int opened = 0;
                for (int v : shaft) {
                    if (g.cur[v] == 0) { opened++; continue; }
                    boolean wasTarget = g.tgt[v] != 0 && matches(g.cur[v], g.tgt[v]);
                    if (!takeOut(v)) break;
                    if (wasTarget) { brokenOut.add(v); deferred.add(v); }
                    opened++;
                }
                if (opened == shaft.length) { unreachable.clear(); badStand.clear(); ok = tryCell(i); }
                // close the shaft, innermost first
                for (int q = opened - 1; q >= 0; q--) {
                    int v = shaft[q];
                    if (g.tgt[v] == 0 || !inBuild(g.xOf(v), g.yOf(v), g.zOf(v))) continue;
                    unreachable.clear(); badStand.clear();
                    tryCell(v);
                }
                undoRope();
                if (ok) break;
            }
            if (ok) fixed++;
        }
        undoRope();
        reconcile();
        return fixed;
    }

    void repair() {
        inRepair = true;
        for (int round = 0; round < 3; round++) {
            if (timeShort()) break;
            long[] before = new long[1];
            java.util.ArrayList<int[]> secs = failingSections(before);
            if (secs.isEmpty()) break;
            undoRope();
            unreachable.clear();
            badStand.clear();
            boolean whole = secs.size() > 6 && maxTicks - ticks > 150000;
            int queued = 0;
            if (whole) queued = requeue(g.ox, g.oy, g.oz, g.ox + g.sx - 1, g.oy + g.sy - 1, g.oz + g.sz - 1);
            else {
                secs.sort((p, q) -> Integer.compare(p[0], q[0]));   // (a stable sort: sections of equal count stay in scan order)
                for (int n = 0; n < secs.size() && n < 24; n++) {
                    int[] s = secs.get(n);
                    if (s[2] != 0 || s[1] == 0) continue;
                    queued += requeue(s[3], s[4], s[5], s[6], s[7], s[8]);
                }
            }
            if (verbose) log("repair round " + round + ": " + secs.size() + " sections fail, " + before[0] + " bad cells in the build; " + queued + " cells taken up again" + (whole ? " (the whole build)" : ""));
            if (queued > 0) {
                boolean lateWas = lateToo; int layerWas = layerNow;
                mainLoop();
                lateToo = true;
                layerNow = Integer.MAX_VALUE;
                deferredPass(false);
                undoRope();
                reconcile();
                // the other ways of placing: a helper block that may rest on a second one, then with traps accepted
                helperChainNow = true;
                try { deferredPass(false); } finally { helperChainNow = false; }
                undoRope();
                deferredPass(true);
                undoRope();
                reconcile();
                // what is still bad in the sections taken up (the 24 with the fewest bad cells) and shut in by standing blocks
                if (EXCAVATE) {
                    if (whole) secs.sort((p, q) -> Integer.compare(p[0], q[0]));
                    it.unimi.dsi.fastutil.ints.IntArrayList cells = new it.unimi.dsi.fastutil.ints.IntArrayList();
                    for (int n = 0; n < secs.size() && n < 24; n++) {
                        int[] sc = secs.get(n);
                        if (sc[2] != 0 || sc[1] == 0) continue;
                        for (int y = sc[4]; y <= sc[7]; y++) for (int z = sc[5]; z <= sc[8]; z++) for (int x = sc[3]; x <= sc[6]; x++) { int i = g.idx(x, y, z), k = badKind(i); if (k == 1 || k == 2) cells.add(i); }
                    }
                    int fixed = excavate(cells.toIntArray());
                    if (verbose) log("repair round " + round + ": " + cells.size() + " cells still bad in those sections, " + fixed + " put right through a shaft");
                }
                lateToo = lateWas; layerNow = layerWas;
            }
            endGame();   // the clean-up again in full
            long[] after = new long[1];
            failingSections(after);
            if (verbose) log("repair round " + round + ": bad cells " + before[0] + " -> " + after[0]);
            if (after[0] >= before[0]) break;
        }
    }

    /** OWN_DOWN: back to the chests, unless the bot can simply walk there from where it is. */
    void comeDown() {
        if (!c.onGround()) settle();
        int x = cx(), y = cy(), z = cz();
        if (g.in(x, y, z) && plainWayOut(x, y, z)) return;
        Stance st = new Stance();
        Site.Stock first = stock.get(0);
        st.x = first.chestX; st.y = g.oy; st.z = first.chestZ + 1;
        goTo(st);
        undoRope();
    }

    boolean timeShort() { return maxTicks - ticks < 45L * scaffold.size() + 40000; }

    /** Cells the bot broke out of the build itself that are still missing. */
    int brokenToMend() {
        int n = 0;
        for (int c : deferred) if (brokenOut.contains(c) && needs(c)) n++;
        return n;
    }

    static final boolean SAME_FAIL = System.getenv("MCB_NO_SAMEFAIL") == null;
    final java.util.HashMap<Integer, Long> failedAs = new java.util.HashMap<>();
    /** set when the repair pass begins (the build fails by the bot's own picture) */
    boolean inRepair;

    /** The rules in force, the blocks within two cells (scaffolding counted as nothing) and whether the block is in the inventory. */
    long failSig(int i, boolean traps) {
        long h = (traps ? 1 : 0) | (noRoofing ? 2 : 0) | (helperChainNow ? 4 : 0) | (forceMine ? 8 : 0) | (mendOnly ? 16 : 0) | (noScaffoldNav ? 32 : 0) | (lateToo ? 64 : 0);
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        for (int dy = -2; dy <= 2; dy++) for (int dz = -2; dz <= 2; dz++) for (int dx = -2; dx <= 2; dx++) {
            int v = 0;
            if (g.in(x + dx, y + dy, z + dz)) { int n = g.idx(x + dx, y + dy, z + dz); v = scaffold.contains(n) ? 0 : g.cur[n]; }
            h = h * 1000003L + v;
        }
        return h * 31 + (count(StateInfo.item[g.tgt[i]]) > 0 ? 1 : 0);
    }

    void deferredPass(boolean traps) {
        if (timeShort()) return;
        phase("deferred cells" + (traps ? " (traps accepted)" : "") + ": " + deferred.size());
        acceptTraps = traps;
        try {
            // nearest first, from wherever the last one left the bot (height counts double: every level is a climb)
            int[] cells = deferred.toIntArray();
            boolean[] done = new boolean[cells.length];
            // OFF_STOCK_B (candidate "offstock", part b): the cells whose block the bot holds come first, nearest first as ever; a
            // cell of a kind it does not hold (which means a trip) is taken only when no cell is left that the load in hand serves
            boolean offs = Supplies.OFF_STOCK_B && mainDone;
            int[] kind = null;
            java.util.List<Item> kinds = new java.util.ArrayList<>();
            if (offs) {
                kind = new int[cells.length];
                for (int j = 0; j < cells.length; j++) {
                    Item it = StateInfo.item[g.tgt[cells[j]]];
                    int p = kinds.indexOf(it);
                    if (p < 0) { kinds.add(it); p = kinds.size() - 1; }
                    kind[j] = p;
                }
            }
            for (int n = 0; n < cells.length; n++) {
                int bx = cx(), by = cy(), bz = cz(), k = -1, kh = -1;
                long best = Long.MAX_VALUE, bestH = Long.MAX_VALUE;
                boolean[] held = null;
                if (offs) { held = new boolean[kinds.size()]; for (int q = 0; q < held.length; q++) held[q] = kinds.get(q) == Items.AIR || count(kinds.get(q)) > 0; }
                for (int j = 0; j < cells.length; j++) {
                    if (done[j]) continue;
                    long d = Math.abs(g.xOf(cells[j]) - bx) + Math.abs(g.zOf(cells[j]) - bz) + 2L * Math.abs(g.yOf(cells[j]) - by);
                    if (d < best) { best = d; k = j; }
                    if (offs && d < bestH && held[kind[j]]) { bestH = d; kh = j; }
                }
                if (offs && kh >= 0) k = kh;
                if (k < 0) break;
                done[k] = true;
                int cell = cells[k];
                if (timeShort()) break;
                if (!needs(cell)) { deferred.remove(cell); continue; }
                if (!lateToo && isLate(cell)) continue;
                if (WEB_LATE && !websToo && isCobweb(g.tgt[cell])) continue;   // (revision "weblate")
                if (mendOnly && !brokenOut.contains(cell)) continue;
                // (revision "samefail") in the repair pass: a cell that failed under these rules with these surroundings is
                // not tried again until something within two cells of it has changed (scaffolding not counted)
                boolean sf = SAME_FAIL && mainDone && inRepair;
                if (sf) { Long was = failedAs.get(cell); if (was != null && was == failSig(cell, traps)) continue; }
                int r = doCell(cell);
                if (sf) { if (r == PLACED) failedAs.remove(cell); else failedAs.put(cell, failSig(cell, traps)); }
                if (r != DEFER) deferred.remove(cell);
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
    @Override void guardHealth() {
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
        // MAGMA_SNEAK (revision "magmasneak"): no safe spot was reached and the bot still stands on the hot floor - crouch while
        // it waits (a sneaking player takes no harm from magma) instead of standing on it until the wait kills it
        boolean crouch = MAGMA_SNEAK && !holdSneak && (StateInfo.hot[g.get(cx(), cy() - 1, cz())] || StateInfo.hot[g.get(cx(), cy(), cz())]);
        if (crouch) c.keys(false, false, false, false, false, true, false);
        for (int n = 0; n < 500 && c.player.getHealth() < 18 && !c.player.isDeadOrDying(); n++) tick();
        if (crouch) c.releaseKeys();
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
        int r = doCell0(i);
        if (r == PLACED) cellsPlaced++;
        return r;
    }

    /** ROPE_FAR: target cells placed, the target cells of the build, and whether the build was found behind its pace. */
    long cellsPlaced, cellsTotal = -1;
    boolean behind;

    /** ROPE_FAR: is the build behind its pace - at the rate so far, would placing every target cell alone take more
     *  than 60 % of the ticks allowed? Asked once 2 % of the cells (300 at least) stand; a build found behind stays so. */
    boolean behindPace() {
        if (behind) return true;
        if (cellsTotal < 0) { long n = 0; for (int t : g.tgt) if (t != 0) n++; cellsTotal = n; }
        if (cellsPlaced < Math.max(300, cellsTotal / 50)) return false;
        behind = (double) ticks * cellsTotal / cellsPlaced > maxTicks * 0.60;
        return behind;
    }

    int doCell0(int i) {
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
            // Once everything else stands (noFace): with the helper in place also the fine search of placeClose -
            // spots off the middle of a block, sneaking or not - and if this helper gives nothing, the next side of
            // the cell gets one. (An observer whose back is built up: only a helper on one particular side, seen
            // from one particular spot, gives the click that makes it look the right way.)
            helperSkip = 0;
            try {
                while (true) {
                    int helper = placeHelper(x, y, z, sp);
                    if (helper < 0) break;
                    int base = helperBase, dir = helperDir;
                    helperCell = helper;
                    try {
                        r = attempt(x, y, z, t, sp, false);
                        if (noFace && r == DEFER) {
                            forgetCloseFailed(g.idx(x, y, z));
                            if (placeClose(x, y, z, t, sp)) { fromClose++; helpedClose++; r = PLACED; }
                        }
                    } finally { helperCell = -1; }
                    mustRemove = true;
                    try { removeScaffold(helper); if (base >= 0) removeScaffold(base); } finally { mustRemove = false; }
                    if (r == PLACED) { helped++; if (base >= 0) chained++; }
                    if (!noFace || r != DEFER || dir < 0) break;
                    helperSkip |= 1 << dir;
                }
            } finally { helperSkip = 0; }
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
