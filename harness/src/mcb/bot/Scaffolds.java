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
                boolean open = wayOut(qx, qy, qz);
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

    boolean removeScaffoldImpl(int i) {
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
    boolean removeTower(int i) {
        int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
        if (TRACE && verbose) log("removeTower " + rel(x, y, z) + (cleaning ? " (cleanup)" : " (build)") + " by " + StackWalker.getInstance().walk(st -> st.skip(2).limit(3).map(f -> f.getMethodName()).toList()));
        int low = y;
        while (g.in(x, low - 1, z) && scaffold.contains(g.idx(x, low - 1, z)) && g.isTower(x, low - 1, z)) low--;
        if (breakTowerBlock(x, low, z) && !scaffold.contains(i)) return true;
        if (!scaffold.contains(i)) return true;
        if (low != y) breakTowerBlock(x, y, z);
        return !scaffold.contains(i);
    }

    boolean breakTowerBlock(int x, int y, int z) {
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

    int cleanupLogs;

    void cleanup() {
        String old = enter("cleanup");
        try { cleanupImpl(); } finally { activity = old; }
    }

    void cleanupImpl() {
        badStand.clear();
        comesBack.clear();   // (going round in circles is judged afresh in every clean-up)
        IntOpenHashSet hopeless = new IntOpenHashSet();
        int guard = 0;
        int least = scaffold.size(), sinceLeast = 0;
        cleaning = true;
        try {
        while (scaffold.size() > hopeless.size() && guard++ < 100000) {
            // going round in circles (taking one block away at the price of leaving others) is not progress
            if (scaffold.size() < least) { least = scaffold.size(); sinceLeast = 0; }
            else if (++sinceLeast > 24 + least / 4) { if (verbose) log("cleanup: no progress for " + sinceLeast + " rounds with " + scaffold.size() + " left (force=" + forceMine + ")"); break; }
            // highest first: pillars come down from the top
            int best = -1, bestY = Integer.MIN_VALUE;
            long bestD = Long.MAX_VALUE;
            int bx = cx(), bz = cz();
            boolean bestTower = true;
            for (int i : scaffold) {
                if (hopeless.contains(i)) continue;
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
            if (best < 0) { if (verbose) log("cleanup: nothing more to try with " + scaffold.size() + " left, " + hopeless.size() + " given up (force=" + forceMine + ")"); break; }
            int before = scaffold.size();
            long t0 = ticks, pm = nPillarMoves, dm = nDigMoves;
            boolean okRemove = removeScaffold(best);
            if (okRemove) comesBack.addTo(best, 1);
            if (okRemove) sweepScaffold();
            undoRope();
            if (verbose && (cleanupLogs++ < 60 || TRACE)) log("cleanup " + rel(g.xOf(best), g.yOf(best), g.zOf(best)) + (okRemove ? " ok" : " FAILED") + " ticks=" + (ticks - t0) + " pillars=" + (nPillarMoves - pm) + " digs=" + (nDigMoves - dm) + " left=" + scaffold.size());
            if (!okRemove) { hopeless.add(best); badStand.clear(); }
            else if (scaffold.size() < before) hopeless.clear();
            else if (scaffold.size() >= before) hopeless.add(best);
        }
        } finally {
            cleaning = false;
        }
    }

    final it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap comesBack = new it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap();

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
