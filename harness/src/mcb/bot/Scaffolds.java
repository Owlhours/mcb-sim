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
 * Upkeep of temporary blocks: taking in the "rope" of loose blocks, taking down scaffolding columns (from the
 * foot), rescuing columns that are about to be closed in, and the clean-up at the end.
 */
abstract class Scaffolds extends Stances {
    Scaffolds(Client c, Grid g, Site site, Oracle oracle) { super(c, g, site, oracle); }

    /**
     * Before a block goes in: scaffolding columns near it that would be left in a closed-off pocket (the last
     * block of a roof over a room with a column still in it) are taken down first. True if the bot went to do that.
     */
    boolean rescueTowers(int x, int y, int z, int t) {
        if (StateInfo.walkThrough[t] || cleaning || rescuing || !g.inBox(x, y, z)) return false;
        int found = -1;
        int i = g.idx(x, y, z);
        char old = g.cur[i];
        g.cur[i] = (char) t;
        Site.Stock first = stock.get(0);
        int ax = first.chestX, ay = g.oy, az = first.chestZ + 1;
        Nav.MineRule keep = nav.mine;
        nav.mine = null;
        towersOff = true;
        outer:
        for (int dz = -4; dz <= 4; dz++) for (int dx = -4; dx <= 4; dx++) {
            int qx = x + dx, qz = z + dz;
            if (!g.in(qx, y, qz) || scaffoldInColumn[col(qx, qz)] == 0) continue;
            for (int qy = Math.max(g.y0 + 1, y - 12); qy <= y + 2; qy++) {
                int q = g.idx(qx, qy, qz);
                if (!scaffold.contains(q) || !g.isTower(qx, qy, qz)) continue;
                if (scaffold.contains(g.idx(qx, qy - 1, qz)) && g.isTower(qx, qy - 1, qz)) continue;   // not the foot
                if (bodyIn(qx, qy, qz) || (cx() == qx && cz() == qz)) continue;                           // the bot's own column: see sealsIn
                if (rescueFailed.contains(q)) continue;
                towersOff = false;
                boolean usableWas = witnessUsable;
                witnessUsable = exitValid && !exitCells.contains(i);
                boolean open;
                try { open = wayOut(qx, qy, qz); } finally { witnessUsable = usableWas; }
                towersOff = true;
                if (!open) { found = q; break outer; }
            }
        }
        towersOff = false;
        nav.mine = keep;
        g.cur[i] = old;
        if (found < 0) return false;
        rescuing = true;
        try {
            if (verbose) log("column at " + rel(g.xOf(found), g.yOf(found), g.zOf(found)) + " would be closed in by " + rel(x, y, z) + ": taking it down first");
            if (removeScaffold(found)) towersRescued++; else rescueFailed.add(found);
        } finally { rescuing = false; }
        return true;
    }

    boolean rescuing;
    final IntOpenHashSet rescueFailed = new IntOpenHashSet();

    void undoRope() {
        String old = enter("undo");
        towersOff = true;
        try { undoRopeImpl(); } finally { activity = old; towersOff = false; }
    }

    void undoRopeImpl() {
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

    boolean breakUnderfoot(int sx, int sy, int sz, int bx, int by, int bz) {
        boolean ok = centre(sx, sy, sz) && breakBlock(bx, by, bz, bx + 0.5, by + 1, bz + 0.5, 80);
        for (int n = 0; n < 200 && !c.onGround(); n++) tick();
        settle();
        return ok;
    }

    /** Before a block goes in: take away scaffolding in the same column that it would roof over. */
    /** Returns false if scaffolding stays under the cell that could not be taken away now. */
    boolean clearBelow(int x, int y, int z) {
        if (scaffoldInColumn[col(x, z)] == 0) return true;
        for (int yy = y - 1; yy >= g.y0; yy--) {
            int i = g.idx(x, yy, z);
            if (scaffold.contains(i)) { if (g.isTower(x, yy, z)) return true; if (!removeScaffold(i)) return false; }
            else if (g.cur[i] != 0) return true;
        }
        return true;
    }

    /** Is there a loose scaffold block (not a column) in the open space under the cell? */
    boolean looseBelow(int x, int y, int z) {
        for (int yy = y - 1; yy >= g.y0; yy--) {
            int i = g.idx(x, yy, z);
            if (scaffold.contains(i)) return !g.isTower(x, yy, z);
            if (g.cur[i] != 0) return false;
        }
        return false;
    }

    boolean removeScaffold(int i) {
        String old = enter("unscaffold");
        try { return removeScaffoldImpl(i); } finally { activity = old; }
    }

    static final boolean CAREFUL = System.getenv("MCB_NO_CAREFUL") == null;   // (on; the variable switches it off for a comparison)
    long keptForExit;

    boolean removeScaffoldImpl(int i) {
        checkScaffold();
        // while building: a temporary block (or column) that is the bot's only way out of where it stands stays for
        // now - what wanted its place is put off
        if (X_KEEP && !cleaning && !mustRemove && !acceptTraps && !forceMine && !rescuing && c.onGround() && scaffold.contains(i) && g.stand16(cx(), cy(), cz()) >= 0) {
            int x0 = g.xOf(i), y0 = g.yOf(i), z0 = g.zOf(i);
            if (Math.abs(x0 - cx()) <= 6 && Math.abs(z0 - cz()) <= 6) {
                int top = y0;
                if (g.isTower(x0, y0, z0)) while (g.isTower(x0, top + 1, z0)) top++;
                int[] cells = columnCells(x0, y0, top, z0);
                char[] was = new char[cells.length];
                for (int k = 0; k < cells.length; k++) { was[k] = g.cur[cells[k]]; g.cur[cells[k]] = 0; }
                boolean open;
                boolean usableWas = witnessUsable, learnWas = witnessLearn;
                witnessUsable = false;   // (cells are gone in thought: a remembered way may have stood on them)
                witnessLearn = false;
                try {
                    int ty = cy();
                    while (ty > g.y0 && g.stand16(cx(), ty, cz()) < 0 && g.passable(cx(), ty, cz())) ty--;
                    open = g.stand16(cx(), ty, cz()) >= 0 && wayOut(cx(), ty, cz());
                } finally { witnessUsable = usableWas; witnessLearn = learnWas; for (int k = 0; k < cells.length; k++) g.cur[cells[k]] = was[k]; }
                if (!open && wayOut(cx(), cy(), cz())) { keptForExit++; trapsAvoided++; return false; }
            }
        }
        if (StateInfo.scaffolding[g.cur[i]]) return removeTower(i);
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        boolean careful = CAREFUL && cleaning && !forceMine && !acceptTraps && c.onGround() && plainWayOut(cx(), cy(), cz());
        if (careful) goingAway = new int[] {i};
        try {
        for (int attempt = 0; attempt < 3; attempt++) {
            if (!scaffold.contains(i)) return true;
            if (!c.onGround()) settle();
            var e = c.eye();
            boolean hereOk = goingAway == null || safeAfterRemoval(cx(), cy(), cz());
            if (!hereOk) { unsafeSpots++; badStand.add(g.idx(cx(), cy(), cz())); }
            if (hereOk && findBreak(x, y, z, e.x, e.y, e.z, click)) {
                if (breakBlock(x, y, z, click.ax, click.ay, click.az, Math.max(80, breakTicks(g.get(x, y, z))))) { settle(); return true; }
            }
            Stance st = planStance(x, y, z, null, true);
            if (st == null) {
                if (TRACE && verbose && cleaning && dbgNav == 0 && aroundLogs++ < 4) {
                    StringBuilder sb = new StringBuilder("nothing reached; around the bot (y+3 .. y-1; rows z-1..z+1, cols x-1..x+1):");
                    for (int dy = 3; dy >= -1; dy--) { sb.append(" | y").append(dy >= 0 ? "+" : "").append(dy).append(':'); for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) { int q = g.get(cx() + dx, cy() + dy, cz() + dz); sb.append(' ').append(StateInfo.name(q).replaceAll("\\[.*", "")).append(g.in(cx() + dx, cy() + dy, cz() + dz) && scaffold.contains(g.idx(cx() + dx, cy() + dy, cz() + dz)) ? "*" : "").append(g.target(cx() + dx, cy() + dy, cz() + dz) != 0 && q == 0 ? "(t)" : ""); } }
                    sb.append(" exitLevel=").append(exitLevel(cx(), cy(), cz())).append(" expanded=").append(lastExitExpanded).append(" towersOff=").append(towersOff).append(" haveTower=").append(haveTower).append(" towerOk above=").append(towerOk(cx(), cy(), cz())).append(" scaffoldOk=").append(scaffoldOk(cx(), cy(), cz()));
                    log(sb.toString());
                }
                if (TRACE && verbose && cleaning) log("no stance to break " + rel(x, y, z) + ": standable=" + dbgStand + " seen=" + dbgClick + " reached=" + dbgNav + " unsafe=" + dbgUnsafe + " columns=" + dbgPillar + " seen=" + dbgPillarClick + " badStand=" + badStand.size() + " loose/scaffolding=" + count(scaffoldItem) + "/" + count(towerItem));
                if (!forceMine || !tunnelTo(x, y, z)) return false;
                continue;
            }
            if (!goTo(st)) { if (TRACE && verbose && cleaning) log("could not go to " + rel(st.x, st.y, st.z) + " to break " + rel(x, y, z)); badStand.add(g.idx(st.x, st.y, st.z)); replans++; }
        }
        return !scaffold.contains(i);
        } finally { if (careful) goingAway = null; }
    }

    /**
     * Takes down a scaffolding column: breaks its lowest block (everything above a broken block comes down by
     * itself), or failing that the given block.
     */
    boolean removeTower(int i) {
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        if (TRACE && verbose) log("removeTower " + rel(x, y, z) + (cleaning ? " (cleanup)" : " (build)") + " by " + StackWalker.getInstance().walk(st -> st.skip(2).limit(3).map(f -> f.getMethodName()).toList()));
        int low = y;
        while (g.in(x, low - 1, z) && scaffold.contains(g.idx(x, low - 1, z)) && g.isTower(x, low - 1, z)) low--;
        // (careful while the bot is free to walk away: it stays so. Where it could only get out with new scaffolding
        // anyway, it clears what is in there with it first.)
        boolean careful = CAREFUL && cleaning && !forceMine && !acceptTraps && c.onGround() && plainWayOut(cx(), cy(), cz());
        if (!careful) {
            if (breakTowerBlock(x, low, z) && !scaffold.contains(i)) return true;
            if (!scaffold.contains(i)) return true;
            if (low != y) breakTowerBlock(x, y, z);
            return !scaffold.contains(i);
        }
        // the clean-up proper: only from where the bot can get away again once the column is gone - a column that is
        // the only way out of the room it stands in is taken from outside, top down as far as can be reached (what
        // stays is left for the rounds that may break through walls)
        int top = y;
        while (g.isTower(x, top + 1, z)) top++;
        try {
            goingAway = columnCells(x, low, top, z);
            if (breakTowerBlock(x, low, z) && !scaffold.contains(i)) return true;
            if (!scaffold.contains(g.idx(x, low, z))) return !scaffold.contains(i);
            for (int yy = Math.max(low + 1, top - 4); yy <= top; yy += 2) {
                if (!scaffold.contains(g.idx(x, yy, z)) || !g.isTower(x, yy, z)) break;
                goingAway = columnCells(x, yy, top, z);
                if (breakTowerBlock(x, yy, z) && !scaffold.contains(g.idx(x, yy, z))) { stubs++; break; }
            }
        } finally { goingAway = null; }
        return !scaffold.contains(i);
    }

    long stubs, unsafeSpots;
    int aroundLogs;
    long stubsLeft() { return stubs; }
    long unsafeCount() { return unsafeSpots; }

    int[] columnCells(int x, int from, int to, int z) {
        int[] cells = new int[to - from + 1];
        for (int yy = from; yy <= to; yy++) cells[yy - from] = g.idx(x, yy, z);
        return cells;
    }

    boolean breakTowerBlock(int x, int y, int z) {
        int i = g.idx(x, y, z);
        for (int attempt = 0; attempt < 3; attempt++) {
            if (!scaffold.contains(i)) return true;
            if (!c.onGround()) settle();
            var e = c.eye();
            boolean hereOk = goingAway == null || safeAfterRemoval(cx(), cy(), cz());
            if (!hereOk) { unsafeSpots++; badStand.add(g.idx(cx(), cy(), cz())); }
            if (hereOk && findBreak(x, y, z, e.x, e.y, e.z, click)) {
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

    int cleanupLogs;
    long lastCleanupRestock = -100000;

    void cleanup() {
        String old = enter("cleanup");
        try { cleanupImpl(); } finally { activity = old; }
    }

    void cleanupImpl() {
        if (!ownDown) phase("clean-up" + (forceMine ? " (breaking through allowed)" : ""));
        badStand.clear();
        comesBack.clear();   // (going round in circles is judged afresh in every clean-up)
        IntOpenHashSet hopeless = new IntOpenHashSet();
        int guard = 0;
        int least = scaffold.size(), sinceLeast = 0;
        boolean progress = false;
        int ownFrom = scafLog.size(), lastBest = -1;
        cleaning = true;
        try {
        while (scaffold.size() > hopeless.size() && guard++ < 100000) {
            // going round in circles (taking one block away at the price of leaving others) is not progress
            // (counted in pieces taken away, not in tries: what cannot be done now costs nothing and is set aside)
            if (scaffold.size() < least) { least = scaffold.size(); sinceLeast = 0; }
            else if (sinceLeast > 24 + least / 4) { if (verbose) log("cleanup: no progress for " + sinceLeast + " rounds with " + scaffold.size() + " left (force=" + forceMine + ")"); break; }
            // the clean-up uses scaffolding up too (what is broken drops nothing): fetch more when it runs low - also
            // after having been cut off from the chests for a while (that was then; the bot has moved since)
            if ((count(scaffoldItem) < 24 || count(towerItem) < 24) && (inDepot(scaffoldItem) > 0 || inDepot(towerItem) > 0) && ticks - lastCleanupRestock > 600 && rope.isEmpty() && c.onGround()) {
                lastCleanupRestock = ticks;
                stranded = false;
                restock(g.oy + g.sy);
                checkScaffold();
            }
            // highest first: pillars come down from the top
            int best = -1, bestY = Integer.MIN_VALUE;
            long bestD = Long.MAX_VALUE;
            int bx = cx(), bz = cz();
            boolean bestTower = true;
            for (int i : scaffold) {
                if (hopeless.contains(i) || (leaveAlone != null && leaveAlone.contains(i))) continue;
                // (OWN_DOWN: twice the bot put up scaffolding to get at this piece and failed all the same - it is
                // not climbed for again; a round that puts up nothing may still try it)
                if (OWN_DOWN && OD_FUTILE && !noScaffoldNav && futile.get(i) >= 2) continue;
                // taken away several times and there again each time: getting to it costs as much as it removes
                if (!forceMine && comesBack.get(i) >= 3) continue;
                boolean tw = StateInfo.scaffolding[g.cur[i]];
                if (tw && !bestTower) continue;
                // columns are taken by the foot - and the ones standing highest up first, while the lower ones are
                // still there to climb
                if (tw && scaffold.contains(i - g.nx * g.nz) && StateInfo.scaffolding[g.cur[i - g.nx * g.nz]]) continue;
                int y = g.yOf(i);
                long d = Math.abs(g.xOf(i) - bx) + Math.abs(g.zOf(i) - bz);
                if ((bestTower && !tw) || y > bestY || (y == bestY && d < bestD)) { best = i; bestY = y; bestD = d; bestTower = tw; }
            }
            if (best < 0) {
                // everything else has had its turn: what could not be done before may be possible now that the rest is gone
                if (X_LOOP && progress && !hopeless.isEmpty()) { hopeless.clear(); progress = false; continue; }
                if (verbose) log("cleanup: nothing more to try with " + scaffold.size() + " left, " + hopeless.size() + " given up (force=" + forceMine + ")");
                break;
            }
            // OWN_DOWN: what the tries so far put up to get at their pieces comes down when the next piece lies
            // somewhere else - while the bot is still where it put it up. A column standing on a roof has its foot up
            // there, and from the ground there is no way back to it short of another column. As long as the next
            // piece is close by (within 8 across and 8 up or down), what stands is used again.
            if (OWN_DOWN && !ownDown && scafLog.size() > ownFrom && lastBest >= 0
                    && (Math.abs(g.xOf(best) - g.xOf(lastBest)) + Math.abs(g.zOf(best) - g.zOf(lastBest)) > 8 || Math.abs(g.yOf(best) - g.yOf(lastBest)) > 8)) {
                int had = scaffold.size();
                ownDownNow(ownFrom);
                ownFrom = scafLog.size();
                lastBest = -1;
                if (scaffold.size() != had) continue;
            }
            lastBest = best;
            int before = scaffold.size();
            long t0 = ticks, pm = nPillarMoves, dm = nDigMoves;
            int logTry = scafLog.size();
            boolean okRemove = removeScaffold(best);
            if (OWN_DOWN && !okRemove && scafLog.size() > logTry) futile.addTo(best, 1);
            if (okRemove) comesBack.addTo(best, 1);
            if (okRemove) sweepScaffold();
            undoRope();
            if (verbose && (cleanupLogs++ < 60 || TRACE)) log("cleanup " + rel(g.xOf(best), g.yOf(best), g.zOf(best)) + (okRemove ? " ok" : " FAILED") + " ticks=" + (ticks - t0) + " pillars=" + (nPillarMoves - pm) + " digs=" + (nDigMoves - dm) + " left=" + scaffold.size());
            if (!okRemove) { hopeless.add(best); badStand.clear(); }
            else if (scaffold.size() < before) { progress = true; if (!X_LOOP) hopeless.clear(); }
            else { hopeless.add(best); sinceLeast++; }
            if (okRemove && scaffold.size() >= least) sinceLeast++;
        }
        if (OWN_DOWN && !ownDown && scafLog.size() > ownFrom) ownDownNow(ownFrom);
        } finally {
            cleaning = false;
        }
    }

    /** OWN_DOWN: takes down the pieces put up since `from` in scafLog that still stand. Nothing new is put up for
     *  this, and every other piece is left alone in it. (What is known as out of reach was learnt under other rules:
     *  it is forgotten before and after.) */
    void ownDownNow(int from) {
        if (timeShort()) return;
        IntOpenHashSet own = new IntOpenHashSet();
        for (int k = from; k < scafLog.size(); k++) if (scaffold.contains(scafLog.getInt(k))) own.add(scafLog.getInt(k));
        if (own.isEmpty()) return;
        IntOpenHashSet others = new IntOpenHashSet(scaffold);
        others.removeAll(own);
        IntOpenHashSet leaveWas = leaveAlone, badWas = new IntOpenHashSet(badStand);
        boolean nsnWas = noScaffoldNav;
        it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap backWas = new it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap(comesBack);
        // (and nothing of the build is broken for it: a round that may break through keeps that for its own pieces)
        boolean forceWas = forceMine, trapsWas = acceptTraps;
        forceMine = false; acceptTraps = false;
        leaveAlone = others; noScaffoldNav = true; ownDown = true;
        unreachable.clear();
        try { cleanupImpl(); }
        finally {
            unreachable.clear();
            ownDown = false; forceMine = forceWas; acceptTraps = trapsWas; cleaning = true; noScaffoldNav = nsnWas; leaveAlone = leaveWas;
            comesBack.clear(); comesBack.putAll(backWas); badStand.clear(); badStand.addAll(badWas);
        }
        undoRope();
    }

    final it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap comesBack = new it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap();
    /** MCB_OWNDOWN, a measurement that is off (not taken on 2026-11-18: within noise over the seeds): see cleanupImpl. */
    static final boolean OWN_DOWN = System.getenv("MCB_OWNDOWN") != null;
    boolean ownDown;
    /** Measurement switches of OWN_DOWN, both off (simulator: the same variables): a piece climbed for in vain twice is
     *  not climbed for again / at the very end the bot goes back to the chests for one more round with nothing new
     *  put up. On the lead's 80 large builds under six seeds the two together cost 4.5 passes a seed. */
    static final boolean OD_FUTILE = System.getenv("MCB_OD_FUTILE") != null, OD_FINAL = System.getenv("MCB_OD_FINAL") != null;
    /** OWN_DOWN: pieces the clean-up failed on after putting up scaffolding to get at them, and how often. */
    final it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap futile = new it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap();

    /** Breaks every scaffold block within reach of where the bot stands, except the one it stands on. */
    void sweepScaffold() {
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
}
