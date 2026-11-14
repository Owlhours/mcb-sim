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
 * Getting about: finding routes and walking them (steps, jumps, drops, doors, scaffolding columns, pillars and
 * bridges of loose blocks, breaking through as a last resort), leaning over an edge, and the checks that keep
 * the bot from going where it could not get out again.
 */
abstract class Moves extends BuildState {
    Moves(Client c, Grid g, Site site, Oracle oracle) { super(c, g, site, oracle); }

    /**
     * Standing on a half-height floor (bottom slabs) with no way up: nothing can be pillared from here. Take out
     * one slab next to the bot, put a scaffold block in its place and step onto that; the slab is put back at the
     * very end. Returns true if the bot now stands on a full block.
     */
    boolean escapeFromHalfFloor() {
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

    /** Bits for the four sides by which the bot could leave cell (x,y,z) with a plain step, jump or drop; bit 4: head room to jump. */
    int exits(int x, int y, int z) {
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
    boolean wayOut(int x, int y, int z) { return wayOut(x, y, z, false); }

    /** @param breaking also count a way that breaks through the build (by hand, in reasonable time) */
    boolean wayOut(int x, int y, int z, boolean breaking) {
        trapChecks++;
        Site.Stock first = stock.get(0);
        int ax = first.chestX, ay = g.oy, az = first.chestZ + 1;
        int[] path = java.util.Arrays.copyOf(nav.path, nav.length * 3);
        byte[] move = java.util.Arrays.copyOf(nav.move, nav.length);
        int len = nav.length, cost = nav.goalCost, sm = nav.scaffoldMoves, tm = nav.towerMoves;
        nav.startH = 0;
        Nav.MineRule keep = nav.mine;
        nav.mine = null;
        // First with what stands (walking, dropping, the columns and ladders that are there): the usual case. Every
        // way found is remembered - the places on it, and every cell the body passes through - so that
        //  - a block that touches none of those cells needs no search at all (sealsIn),
        //  - and a search from somewhere else only has to reach any remembered place, not the chests.
        // (This used to be "room for a hundred steps means not shut in", which a hall or a shaft with a column in it
        // passes too; then a search to the chests with a fixed budget, which far from the chests ran out and was
        // taken for "no way": 78,000 times in 02dc970d.)
        boolean relaxWas = relaxNext;
        relaxNext = X_NEXT;
        towersOff = true;
        boolean join = X_EXIT && exitValid && witnessUsable && !exitNodes.isEmpty();
        nav.stopAt = join ? exitNodes : null;
        nav.effort = 4;
        boolean ok;
        try { ok = X_EXIT ? nav.search(x, y, z, ax, ay, az, 20000, null) : nav.search(x, y, z, ax, ay, az, 96, null) || nav.expanded > 96; }
        finally { nav.stopAt = null; nav.effort = 1; towersOff = false; }
        boolean certainNo = !ok && nav.exhausted;
        if (ok && X_EXIT && witnessLearn) {
            if (!join) { exitNodes.clear(); exitCells.clear(); } else exitJoined++;
            for (int k = 0; k < nav.length; k++) {
                int px = nav.path[k * 3], py = nav.path[k * 3 + 1], pz = nav.path[k * 3 + 2];
                if (!g.in(px, py, pz)) continue;
                exitNodes.add(g.idx(px, py, pz));
                int lo = py, hi = py + 2;
                if (k > 0) { lo = Math.min(lo, nav.path[k * 3 - 2]); hi = Math.max(hi, nav.path[k * 3 - 2] + 2); }
                for (int yy = lo; yy <= hi; yy++) {
                    if (g.in(px, yy, pz)) exitCells.add(g.idx(px, yy, pz));
                    if (k > 0 && g.in(nav.path[k * 3 - 3], yy, nav.path[k * 3 - 1])) exitCells.add(g.idx(nav.path[k * 3 - 3], yy, nav.path[k * 3 - 1]));
                }
            }
            exitValid = true;
            if (Builder.PARANOID) {
                StringBuilder sb = new StringBuilder("(found at t" + ticks + ")");
                for (int k = 0; k < Math.min(nav.length, 14); k++) sb.append(' ').append(k == 0 ? "" : "WJDPBdcMCSTGL".charAt(nav.move[k]) + ">").append(rel(nav.path[k * 3], nav.path[k * 3 + 1], nav.path[k * 3 + 2]));
                exitTrace = sb.toString();
            }
        }
        boolean plainOk = ok;
        if (!ok) { trapSearches++; nav.effort = 4; try { ok = nav.search(x, y, z, ax, ay, az, 30000, this::scaffoldOk); } finally { nav.effort = 1; } certainNo = !ok && nav.exhausted; }
        if (Builder.PARANOID && !plainOk && ok && verbose && wayOutLogs++ < 400) {
            StringBuilder sb = new StringBuilder("wayOut from " + rel(x, y, z) + " only with scaffolding:");
            for (int k = 0; k < Math.min(nav.length, 8); k++) sb.append(' ').append(k == 0 ? "" : "WJDPBdcMCSTGL".charAt(nav.move[k]) + ">").append(rel(nav.path[k * 3], nav.path[k * 3 + 1], nav.path[k * 3 + 2])).append("{").append(StateInfo.name(g.get(nav.path[k * 3], nav.path[k * 3 + 1], nav.path[k * 3 + 2])).replaceAll("\\[.*", "")).append(" on ").append(StateInfo.name(g.get(nav.path[k * 3], nav.path[k * 3 + 1] - 1, nav.path[k * 3 + 2])).replaceAll("\\[.*", "")).append(" h=").append(g.stand16(nav.path[k * 3], nav.path[k * 3 + 1], nav.path[k * 3 + 2])).append("}");
            sb.append("  <- ").append(String.valueOf((Object) StackWalker.getInstance().walk(st -> st.skip(1).limit(6).map(f -> f.getMethodName()).toList())));
            log(sb.toString());
        }
        if (!ok && breaking) {
            nav.mine = this::mineTicks;
            nav.effort = 20;
            ok = nav.search(x, y, z, ax, ay, az, 60000, this::scaffoldOk);
            nav.effort = 1;
            certainNo = !ok && nav.exhausted;
        }
        // (a search that ran out of budget has not shown that there is no way: only "every reachable place was looked
        // at and none is the way" counts as shut in)
        if (!ok && !certainNo && X_EXIT) { exitUnknown++; ok = true; }
        nav.mine = keep;
        relaxNext = relaxWas;
        System.arraycopy(path, 0, nav.path, 0, path.length);
        System.arraycopy(move, 0, nav.move, 0, move.length);
        nav.length = len; nav.goalCost = cost; nav.scaffoldMoves = sm; nav.towerMoves = tm;
        return ok;
    }

    /**
     * A way from cell (x,y,z) to the supply chests that needs nothing new: walking, jumping, dropping, doors,
     * ladders and the scaffolding columns that stand. (Searched in full: for the clean-up, where a room with a
     * column in it must not pass for open.)
     */
    boolean plainWayOut(int x, int y, int z) {
        trapChecks++;
        Site.Stock first = stock.get(0);
        int ax = first.chestX, ay = g.oy, az = first.chestZ + 1;
        int[] path = java.util.Arrays.copyOf(nav.path, nav.length * 3);
        byte[] move = java.util.Arrays.copyOf(nav.move, nav.length);
        int len = nav.length, cost = nav.goalCost, sm = nav.scaffoldMoves, tm = nav.towerMoves;
        nav.startH = 0;
        Nav.MineRule keep = nav.mine;
        nav.mine = null;
        boolean wasOff = towersOff;
        towersOff = true;
        nav.effort = 8;
        // In the clean-up: like wayOut, the search only has to reach a place on the way that was found last (it
        // is a plain way too), and a way found is remembered. The careful clean-up asks this question for every
        // place it might stand to take a piece away - tens of thousands of times in a build; asked this way,
        // most of them are answered after a few steps.
        boolean remembered = cleaning && X_EXIT;
        boolean join = remembered && exitValid && witnessUsable && !exitNodes.isEmpty();
        nav.stopAt = join ? exitNodes : null;
        boolean ok;
        try { ok = nav.search(x, y, z, ax, ay, az, 20000, null); }
        finally { nav.stopAt = null; towersOff = wasOff; nav.effort = 1; nav.mine = keep; }
        if (remembered && ok && witnessLearn) {
            if (!join) { exitNodes.clear(); exitCells.clear(); } else exitJoined++;
            for (int k = 0; k < nav.length; k++) {
                int px = nav.path[k * 3], py = nav.path[k * 3 + 1], pz = nav.path[k * 3 + 2];
                if (!g.in(px, py, pz)) continue;
                exitNodes.add(g.idx(px, py, pz));
                int lo = py, hi = py + 2;
                if (k > 0) { lo = Math.min(lo, nav.path[k * 3 - 2]); hi = Math.max(hi, nav.path[k * 3 - 2] + 2); }
                for (int yy = lo; yy <= hi; yy++) {
                    if (g.in(px, yy, pz)) exitCells.add(g.idx(px, yy, pz));
                    if (k > 0 && g.in(nav.path[k * 3 - 3], yy, nav.path[k * 3 - 1])) exitCells.add(g.idx(nav.path[k * 3 - 3], yy, nav.path[k * 3 - 1]));
                }
            }
            exitValid = true;
        }
        System.arraycopy(path, 0, nav.path, 0, path.length);
        System.arraycopy(move, 0, nav.move, 0, move.length);
        nav.length = len; nav.goalCost = cost; nav.scaffoldMoves = sm; nav.towerMoves = tm;
        return ok;
    }

    /**
     * With the cells in goingAway gone: can the bot, standing at (sx,sy,sz) (or wherever it lands from there when
     * what carries it is among them), still get back to the chests without putting up anything new?
     */
    int unsafeLogs;
    /** False while wayOut is asked about a world in which the remembered ways may be cut (cells taken away in thought). */
    boolean witnessUsable = true;
    /** False while the world asked about has cells taken away in thought: a way found there may not exist. (A world with a
     *  block added in thought is fine: what avoids the block is there without it too.) */
    boolean witnessLearn = true;
    long exitUnknown, exitJoined;
    long exitUnknownCount() { return exitUnknown; }
    long exitJoinedCount() { return exitJoined; }
    long relaxedRoutes;
    long relaxedCount() { return relaxedRoutes; }
    String sealDbg = "";
    String exitTrace = "";
    long alreadyShut;
    long shutCount() { return alreadyShut; }
    int wayOutLogs;
    int lastExitExpanded;
    /** What the clean-up must keep (see exitLevel): the level the bot had when it set about the piece in hand. */
    int needLevel = 2;

    /**
     * How the bot could get from cell (x,y,z) to the chests: 2 = with nothing new (walking, the columns that stand,
     * ladders, doors), 1 = only by putting up new scaffolding, 0 = not at all short of breaking through.
     */
    int exitLevel(int x, int y, int z) {
        if (plainWayOut(x, y, z)) return 2;
        Site.Stock first = stock.get(0);
        int ax = first.chestX, ay = g.oy, az = first.chestZ + 1;
        int[] path = java.util.Arrays.copyOf(nav.path, nav.length * 3);
        byte[] move = java.util.Arrays.copyOf(nav.move, nav.length);
        int len = nav.length, cost = nav.goalCost, sm = nav.scaffoldMoves, tm = nav.towerMoves;
        nav.startH = 0;
        Nav.MineRule keep = nav.mine;
        nav.mine = null;
        nav.effort = 8;
        boolean ok;
        boolean relaxWas = relaxNext;
        relaxNext = X_NEXT;
        try { ok = nav.search(x, y, z, ax, ay, az, 60000, this::scaffoldOk); }
        finally { nav.effort = 1; nav.mine = keep; relaxNext = relaxWas; }
        lastExitExpanded = nav.expanded;
        System.arraycopy(path, 0, nav.path, 0, path.length);
        System.arraycopy(move, 0, nav.move, 0, move.length);
        nav.length = len; nav.goalCost = cost; nav.scaffoldMoves = sm; nav.towerMoves = tm;
        return ok ? 1 : 0;
    }

    boolean safeAfterRemoval(int sx, int sy, int sz) {
        if (goingAway == null) return true;
        char[] was = new char[goingAway.length];
        for (int k = 0; k < goingAway.length; k++) { was[k] = g.cur[goingAway[k]]; g.cur[goingAway[k]] = 0; }
        try {
            int ty = sy;
            while (ty > g.y0 && g.stand16(sx, ty, sz) < 0 && g.passable(sx, ty, sz)) ty--;
            if (g.stand16(sx, ty, sz) < 0) { if (TRACE && verbose && unsafeLogs++ < 40) log("unsafe: nowhere to stand at/below " + rel(sx, sy, sz)); return false; }
            // (the remembered way counts only if it neither passes through nor stands on what is going away; and a
            // way found in this world-in-thought is not remembered)
            boolean usableWas = witnessUsable, learnWas = witnessLearn;
            boolean clear = exitValid;
            int layer = g.nx * g.nz;
            for (int k = 0; k < goingAway.length && clear; k++) clear = !exitCells.contains(goingAway[k]) && !exitNodes.contains(goingAway[k] + layer);
            witnessUsable = clear;
            witnessLearn = false;
            boolean ok;
            try { ok = plainWayOut(sx, ty, sz); } finally { witnessUsable = usableWas; witnessLearn = learnWas; }
            if (!ok && TRACE && verbose && unsafeLogs++ < 40) log("unsafe: no plain way out from " + rel(sx, ty, sz) + " (asked " + rel(sx, sy, sz) + ") expanded=" + nav.expanded + " with " + goingAway.length + " cells gone from " + rel(g.xOf(goingAway[0]), g.yOf(goingAway[0]), g.zOf(goingAway[0])));
            return ok;
        } finally {
            for (int k = 0; k < goingAway.length; k++) g.cur[goingAway[k]] = was[k];
        }
    }

    /** wayOut for the world as it is, with the helper block (if any) taken away in thought. */
    boolean wayOutPlainWorld(int x, int y, int z) {
        boolean usableWas = witnessUsable, learnWas = witnessLearn;
        witnessUsable = helperCell < 0;
        witnessLearn = helperCell < 0;
        try { return wayOut(x, y, z, acceptTraps); } finally { witnessUsable = usableWas; witnessLearn = learnWas; }
    }

    /** Would putting the block into (x,y,z) from where the bot stands close its own way out? */
    boolean sealsIn(int x, int y, int z, int t) {
        sealDbg = "";
        boolean r = sealsIn0(x, y, z, t);
        if (WATCH != null && x - g.ox == WATCH[0] && y - g.oy == WATCH[1] && z - g.oz == WATCH[2])
            log("WATCH sealsIn -> " + r + " walkThrough=" + StateInfo.walkThrough[t] + " ground=" + c.onGround() + " exitValid=" + exitValid + " state=" + StateInfo.name(t) + " pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz) + " stand=" + g.stand16(cx(), cy(), cz()) + " traps=" + trapsAvoided + " shut=" + alreadyShut + " dbg=" + sealDbg + " lastPlain=" + exitTrace);
        return r;
    }

    boolean sealsIn0(int x, int y, int z, int t) {
        if (StateInfo.walkThrough[t] || !c.onGround()) return false;
        int bx = cx(), by = cy(), bz = cz();
        // (whatever is being closed off may be some way off: a hatch in the ceiling of the room the bot stands in)
        if (Math.abs(x - bx) > 6 || Math.abs(z - bz) > 6 || y < by - 4 || y > by + 6 || !g.in(bx, by, bz)) return false;
        int i = g.idx(x, y, z);
        // a block that takes the room the bot itself stands in (over its head while it sneaks on a fence or a slab:
        // the game allows that, and the bot could never stand up again)
        if (X_REL && g.stand16(bx, by, bz) >= 0) {
            char was = g.cur[i];
            g.cur[i] = (char) t;
            boolean wedged = g.stand16(bx, by, bz) < 0;
            g.cur[i] = was;
            if (wedged) { trapsAvoided++; return true; }
        }
        // (the way out found last time still stands and this block is not in it)
        if (exitValid && !exitCells.contains(i) && exitNodes.contains(g.idx(bx, by, bz))) {
            trapChecks++;
            if (!Builder.PARANOID) return false;
            // debugging: is the remembered way really still there?
            char o2 = g.cur[i];
            g.cur[i] = (char) t;
            String cached = exitTrace;
            boolean really = wayOut(bx, by, bz, acceptTraps);
            g.cur[i] = o2;
            if (really) return false;
            if (wayOut(bx, by, bz, acceptTraps)) log("CACHE MISMATCH: remembered way out is gone with " + StateInfo.name(t) + " at " + rel(x, y, z) + "; it was: " + cached);
            else log("CACHE MISMATCH: remembered way out is gone even without the block at " + rel(x, y, z) + "; it was: " + cached);
            return false;
        }
        char old = g.cur[i];
        g.cur[i] = (char) t;
        // (a block lent for the click is about to go again: no way out may count on it)
        int hc = System.getenv("MCB_NO_HELPER") == null ? helperCell : -1;
        char helperWas = 0;
        if (hc >= 0 && hc != i) { helperWas = g.cur[hc]; g.cur[hc] = 0; }
        boolean usableWas = witnessUsable, learnWas = witnessLearn;
        witnessUsable = exitValid && !exitCells.contains(i) && !(hc >= 0 && hc != i);
        witnessLearn = !(hc >= 0 && hc != i);
        boolean trapped;
        try { trapped = !wayOut(bx, by, bz, acceptTraps); } finally { witnessUsable = usableWas; witnessLearn = learnWas; }
        sealDbg = "with block: trapped=" + trapped + " expanded=" + nav.expanded + " helper=" + hc;
        g.cur[i] = old;
        // (no way out without the block either - shut in already, or too far for the search: then it is not this
        // block that does it, and refusing everything from here on would only stop the work)
        if (X_REL && trapped && !wayOutPlainWorld(bx, by, bz)) { trapped = false; alreadyShut++; sealDbg += " | without: no way either, expanded=" + nav.expanded; }
        if (hc >= 0 && hc != i) g.cur[hc] = helperWas;
        if (trapped) trapsAvoided++;
        return trapped;
    }
    int unreachableFrom = -1;
    long unreachableAt = -1;

    boolean search(int x, int y, int z, int maxExpand) {
        if (!c.onGround()) settle();   // (in the air, or sliding down a ladder: wait until standing)
        nav.startH = c.onGround() ? Math.max(0, Math.min(15, (int) Math.round((c.y() - cy()) * 16))) : -1;
        nav.towersOn = haveTower && !towersOff && !noScaffoldNav;
        // in the last rounds of the clean-up (what is left then is what kept coming back) a new tall column is the
        // last thing wanted - it has to be taken down again, and from the far side of a wall that starts the whole
        // thing over: rather break through and mend that later
        boolean thrifty = cleaning && forceMine;
        if (thrifty) { nav.mine = this::mineTicks; nav.towerPenalty = 120; nav.scaffoldPenalty = 170; nav.effort = 4; }
        boolean ok;
        try {
            ok = nav.search(cx(), cy(), cz(), x, y, z, maxExpand, this::scaffoldOk);
        } finally {
            if (thrifty) { nav.mine = null; nav.towerPenalty = 3; nav.scaffoldPenalty = 40; nav.effort = 1; }
        }
        if (!ok && nav.expanded <= 12 && !looseBlocked.isEmpty() && !(cx() == x && cy() == y && cz() == z)) {
            // nowhere to go from here: perhaps only because of steps with loose blocks that failed - let those be
            // tried again (walking steps that failed stay out: with all of them gone the bot gets itself unstuck)
            for (long e : looseBlocked) nav.blockedEdges.remove(e);
            looseBlocked.clear();
            looseFails.clear();
            unreachable.clear();
            if (thrifty) { nav.mine = this::mineTicks; nav.towerPenalty = 120; nav.scaffoldPenalty = 170; nav.effort = 4; }
            try {
                ok = nav.search(cx(), cy(), cz(), x, y, z, maxExpand, this::scaffoldOk);
            } finally {
                if (thrifty) { nav.mine = null; nav.towerPenalty = 3; nav.scaffoldPenalty = 40; nav.effort = 1; }
            }
        }
        if (!ok && X_NEXT && !relaxNext && nav.expanded <= 400 && !(cx() == x && cy() == y && cz() == z)) {
            // no route by the usual rules, and hardly anywhere to go from here (a bot that can roam fails to reach a
            // goal because of the goal, and searching twice only costs time - 02dc970d took three times as long):
            // one more try with temporary blocks allowed in a column's next cell
            relaxNext = true;
            if (thrifty) { nav.mine = this::mineTicks; nav.towerPenalty = 120; nav.scaffoldPenalty = 170; nav.effort = 4; }
            try {
                ok = nav.search(cx(), cy(), cz(), x, y, z, maxExpand, this::scaffoldOk);
            } finally {
                relaxNext = false;
                if (thrifty) { nav.mine = null; nav.towerPenalty = 3; nav.scaffoldPenalty = 40; nav.effort = 1; }
            }
            if (ok) relaxedRoutes++;
        }
        boxedExpanded = nav.expanded;
        if (!ok && nav.expanded <= (X_UNSTICK ? 3 : 1) && !(cx() == x && cy() == y && cz() == z) && unstick()) {
            nav.startH = c.onGround() ? Math.max(0, Math.min(15, (int) Math.round((c.y() - cy()) * 16))) : -1;
            ok = nav.search(cx(), cy(), cz(), x, y, z, maxExpand, this::scaffoldOk);
        }
        return ok;
    }

    java.util.HashMap<Item, int[]> spares;

    /** How many more blocks of this kind the bot may break (and replace from the reserve in the chests). */
    int spare(Item it) {
        if (spares == null) {
            spares = new java.util.HashMap<>();
            for (int i = 0; i < g.tgt.length; i++) {
                int t = g.tgt[i];
                if (t != 0 && StateInfo.itemCount[t] > 0) spares.computeIfAbsent(StateInfo.item[t], k -> new int[1])[0] += StateInfo.itemCount[t];
            }
            for (int[] v : spares.values()) v[0] = Math.max(4, v[0] / 20) / 2;
        }
        int[] v = spares.get(it);
        return v == null ? 0 : v[0];
    }

    void spent(int state) {
        if (spares == null) spare(Items.AIR);
        int[] v = spares.get(StateInfo.item[state]);
        if (v != null) v[0]--;
    }
    long lastUnstick = -1000;

    /**
     * No step can even be planned from where the bot is (wedged against something that fell beside it, standing
     * on an odd shape ...): shuffle and hop about until it stands in a different, ordinary spot.
     */
    int boxedExpanded;
    boolean unstick() {
        if (ticks - lastUnstick < 200) return false;
        lastUnstick = ticks;
        int x0 = cx(), y0 = cy(), z0 = cz();
        // a gate, door or trapdoor shut around the bot: open it
        for (int dy = 0; dy <= 1; dy++) for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) {
            int qx = x0 + dx, qy = y0 + dy, qz = z0 + dz;
            if (!g.in(qx, qy, qz) || !bodyIn(qx, qy, qz)) continue;
            int st = g.get(qx, qy, qz);
            if (st == 0 || StateInfo.walkThrough[st] && StateInfo.doorEdges[st] == 0) continue;
            boolean opens = StateInfo.handDoor[st] || (StateInfo.state[st].getBlock() instanceof net.minecraft.world.level.block.TrapDoorBlock tb && StateInfo.state[st].getBlock() != net.minecraft.world.level.block.Blocks.IRON_TRAPDOOR);
            if (opens && toggle(qx, qy, qz)) {
                settle();
                nav.blockedEdges.clear();
                unreachable.clear();
                unstuck++;
                if (verbose) log("unstuck: opened " + StateInfo.name(st) + " at " + rel(qx, qy, qz) + " around the bot");
                return true;
            }
        }
        // redstone dust, a torch, a flower ... in the cell the bot stands in (or at head height): nothing can be put
        // there while it is, so no column, no way up - out it comes (and is put back later)
        if (X_UNSTICK && c.onGround() && count(scaffoldItem) > 0) {
            for (int dy = 0; dy <= 1; dy++) {
                if (!g.in(x0, y0 + dy, z0)) continue;
                int st = g.get(x0, y0 + dy, z0);
                int ci = g.idx(x0, y0 + dy, z0);
                if (st == 0 || !StateInfo.noCollision[st] || scaffold.contains(ci) || StateInfo.doorEdges[st] != 0) continue;
                int bt = breakTicks(st);
                if (bt < 0 || bt > 200) continue;
                var e = c.eye();
                if (!findBreak(x0, y0 + dy, z0, e.x, e.y, e.z, click) || !breakBlock(x0, y0 + dy, z0, click.ax, click.ay, click.az, bt + 40)) continue;
                spent(st);
                if (g.tgt[ci] != 0) { deferred.add(ci); brokenOut.add(ci); }
                settle();
                nav.blockedEdges.clear();
                unreachable.clear();
                unstuck++;
                if (verbose) log("unstuck: took " + StateInfo.name(st) + " out of the bot's own cell at " + rel(x0, y0 + dy, z0));
                return true;
            }
        }
        int h0 = g.in(x0, y0, z0) ? g.stand16(x0, y0, z0) : -1;
        // a post, wall or pane in the bot's own cell with the bot squeezed in beside it (it went in from next door
        // while the bot stood at the edge, or the bot was pushed in): out it comes (and is put back later)
        if (h0 < 0 && c.onGround()) {
            for (int dy = 0; dy <= 1; dy++) {
                int st = g.get(x0, y0 + dy, z0);
                if (st == 0 || StateInfo.walkThrough[st] || !g.in(x0, y0 + dy, z0)) continue;
                int bt = breakTicks(st);
                if (bt < 0 || bt > 1500 || StateInfo.fullCube[st]) continue;
                var e = c.eye();
                if (!findBreak(x0, y0 + dy, z0, e.x, e.y, e.z, click) || !breakBlock(x0, y0 + dy, z0, click.ax, click.ay, click.az, bt + 40)) continue;
                int ci = g.idx(x0, y0 + dy, z0);
                if (!scaffold.contains(ci)) { mined++; spent(st); if (g.tgt[ci] != 0) { deferred.add(ci); brokenOut.add(ci); } }
                settle();
                middle(x0, z0);
                nav.blockedEdges.clear();
                unreachable.clear();
                unstuck++;
                if (verbose) log("unstuck: took " + StateInfo.name(st) + " out of the bot's own cell at " + rel(x0, y0 + dy, z0));
                return true;
            }
        }
        // standing on something low that fills the own cell (a comparator, a slab ...) with no step possible: take
        // it out from under the feet (it is put back later) to stand on the floor below
        int own = g.get(x0, y0, z0);
        if (h0 > 0 && h0 < 16 && own != 0 && !scaffold.contains(g.idx(x0, y0, z0)) && breakTicks(own) >= 0 && breakTicks(own) <= 1500) {
            if (breakBlock(x0, y0, z0, x0 + 0.5, y0 + h0 / 16.0, z0 + 0.5, breakTicks(own) + 40)) {
                mined++;
                spent(own);
                if (g.tgt[g.idx(x0, y0, z0)] != 0) { deferred.add(g.idx(x0, y0, z0)); brokenOut.add(g.idx(x0, y0, z0)); }
                settle();
                nav.blockedEdges.clear();
                unreachable.clear();
                unstuck++;
                if (verbose) log("unstuck: took " + StateInfo.name(own) + " out from under the feet at " + rel(x0, y0, z0));
                return true;
            }
        }
        if (boxedExpanded > 1) return false;   // (the rest is for a bot that cannot take a single step)
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

    /** Set for a search that may break blocks even when the chests cannot make them good (better one hole than a bot shut in for good). */
    boolean desperate;

    int mineTicks(int x, int y, int z) {
        if (!g.in(x, y, z) || y < g.oy) return -1;   // (never the ground the site stands on)
        int w = g.get(x, y, z);
        if (w == 0) return 0;
        if (StateInfo.gravity[g.get(x, y + 1, z)]) return -1;
        // (broken ice leaves water behind, and water runs)
        if (StateInfo.state[w].getBlock() == net.minecraft.world.level.block.Blocks.ICE) return -1;
        // what is broken is gone (nothing drops): only as many as the chests can make good
        if (!desperate && spare(StateInfo.item[w]) <= 0) return -1;
        // nothing may hang on it or stand on it that would come off with it
        for (int d = 0; d < 6; d++) {
            int n = g.get(x + DX[d], y + DY[d], z + DZ[d]);
            if (n != 0 && d != 0 && !StateInfo.scaffolding[n] && !oracle.floats(n)) return -1;
        }
        if (z < g.oz - 2 && StateInfo.state[w].getBlock() instanceof net.minecraft.world.level.block.ChestBlock) return -1;
        int t = breakTicks(w);
        // (obsidian takes four minutes by hand: only to get out of somewhere there is no other way out of)
        return t < 0 || t > (desperate ? 6000 : 1500) ? -1 : t;
    }

    boolean route(int x, int y, int z, int maxExpand) {
        int here = g.in(cx(), cy(), cz()) ? g.idx(cx(), cy(), cz()) : -1;
        if (here != unreachableFrom || placed - unreachableAt > 30) { unreachable.clear(); unreachableFrom = here; unreachableAt = placed; }
        int goal = g.idx(x, y, z);
        if (unreachable.contains(goal)) return false;
        if (search(x, y, z, maxExpand)) return true;
        if ((shutIn >= 3 || forceMine) && !mendOnly) {
            // last resort: break out through the build itself (not while mending what was broken out)
            nav.mine = this::mineTicks;
            boolean ok = search(x, y, z, maxExpand);
            if (!ok) { desperate = true; ok = search(x, y, z, maxExpand); desperate = false; }
            nav.mine = null;
            if (ok) return true;
        }
        unreachable.add(goal);
        return false;
    }

    boolean goTo(Stance st) {
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
            if (!restocking) {
                boolean oneWay = false;
                for (int k = 1; k < nav.length && !oneWay; k++) oneWay = nav.move[k] == Nav.DROP || ((nav.move[k] == Nav.WALK || nav.move[k] == Nav.LADDER) && nav.path[k * 3 + 1] < nav.path[k * 3 - 2]);
                if (oneWay && !wayOut(st.x, gy, st.z, acceptTraps)) { trapsAvoided++; return false; }
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

    boolean restockWorthwhile() {
        for (Site.Stock st : stock) if (st.count > 0 && ((st.item == scaffoldItem && nav.scaffoldMoves > 0 && count(scaffoldItem) < nav.scaffoldMoves + 4) || (st.item == towerItem && nav.towerMoves > count(towerItem)))) return true;
        return false;
    }

    /** Sneaks to the edge of the block stood on, in the given direction, and stays there (sneak held). */
    boolean lean(int x, int y, int z, int dir) {
        String old = enter("lean");
        try { return leanImpl(x, y, z, dir); } finally { activity = old; }
    }

    boolean leanImpl(int x, int y, int z, int dir) {
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

    void endLean() {
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
    boolean travel() {
        String old = enter("travel");
        try {
            tStep = -1;
            boolean ok = travelImpl();
            if (!ok && tStep >= 1 && tPath != null && tStep * 3 + 2 < tPath.length) {
                // a step with a loose block (or through one, or breaking something) that did not work from here: not
                // offered again, or the same route comes up for ever (walking steps see to that themselves)
                byte m = tMove[tStep];
                boolean loose = (m == Nav.BRIDGE || m == Nav.PILLAR) && count(scaffoldItem) > 0;
                if (loose || m == Nav.MINE || m == Nav.CLEAR || m == Nav.DIG) {
                    int a = tStep * 3 - 3, b = tStep * 3;
                    if (g.in(tPath[a], tPath[a + 1], tPath[a + 2]) && g.in(tPath[b], tPath[b + 1], tPath[b + 2])) {
                        // (the second time: once can be a slip)
                        long e = Nav.edge(g.idx(tPath[a], tPath[a + 1], tPath[a + 2]), g.idx(tPath[b], tPath[b + 1], tPath[b + 2]));
                        if (looseFails.addTo(e, 1) >= 1) { nav.blockedEdges.add(e); looseBlocked.add(e); unreachable.clear(); }
                    }
                }
            }
            return ok;
        } finally { activity = old; }
    }

    final it.unimi.dsi.fastutil.longs.Long2IntOpenHashMap looseFails = new it.unimi.dsi.fastutil.longs.Long2IntOpenHashMap();
    final it.unimi.dsi.fastutil.longs.LongOpenHashSet looseBlocked = new it.unimi.dsi.fastutil.longs.LongOpenHashSet();
    private int[] tPath;
    private byte[] tMove;
    private int tStep;

    boolean travelImpl() {
        int n = nav.length;
        // nav's arrays are reused by later searches: copy the route
        int[] path = java.util.Arrays.copyOf(nav.path, n * 3);
        byte[] move = java.util.Arrays.copyOf(nav.move, n);
        if (TRACE && verbose) {
            StringBuilder sb = new StringBuilder("route:");
            for (int k = 0; k < n; k++) sb.append(' ').append(k == 0 ? "" : "WJDPBdcMCSTGL".charAt(move[k]) + ">").append(rel(path[k * 3], path[k * 3 + 1], path[k * 3 + 2]));
            sb.append("  <- ").append(String.valueOf((Object) StackWalker.getInstance().walk(st -> st.skip(2).limit(5).map(f -> f.getMethodName()).toList())));
            log(sb.toString());
        }
        int i = 1;
        routesTravelled++; routeSteps += n - 1;
        tPath = path; tMove = move;
        while (i < n) {
            byte m = move[i];
            tStep = i;
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
                if (g.stand16(x, y, z) > 0 && !g.passable(x, y, z)) y++;   // standing on something low in the own cell: that is what goes (on a post below: the post)
                int w = g.get(x, y - 1, z);
                if (!centre(x, y - (g.stand16(x, y - 1, z) > 0 ? 1 : 0), z)) return fail("centre before mining down at " + rel(x, y, z));
                int onTop = g.get(x, y, z);
                if (onTop != 0 && StateInfo.noCollision[onTop]) breakBlock(x, y, z, x + 0.5, y + 0.03, z + 0.5, Math.max(40, breakTicks(onTop)));
                if (!breakBlock(x, y - 1, z, x + 0.5, y - 1 + Math.max(1, StateInfo.anyTop[w]) / 16.0, z + 0.5, Math.max(80, breakTicks(w) + 20))) return fail("mine down at " + rel(x, y - 1, z));
                mined++;
                spent(w);
                log("mined down through " + StateInfo.name(w) + " at " + rel(x, y - 1, z));
                int ci = g.idx(x, y - 1, z);
                if (g.tgt[ci] != 0) { deferred.add(ci); brokenOut.add(ci); }
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
                        spent(w);
                        log("mined through " + StateInfo.name(w) + " at " + rel(x, yy, z));
                        int ci = g.idx(x, yy, z);
                        if (g.tgt[ci] != 0 && !scaffold.contains(ci)) { deferred.add(ci); brokenOut.add(ci); }   // put back at the end
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
                long had = nTowerBlocks;
                boolean tOk = tower(x, path[(i - 1) * 3 + 1], z, toY);
                if (nTowerBlocks > had) roped += ropeIn(x, z, toY);
                if (!tOk) return fail("tower at " + rel(x, toY, z) + " from y=" + (path[(i - 1) * 3 + 1] - g.oy) + " have=" + count(towerItem) + " pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz));
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
            } else if (m == Nav.LADDER) {
                int j = i;
                while (j < n && move[j] == Nav.LADDER) j++;
                String old = enter("ladder");
                boolean lOk;
                try { lOk = ladder(path, i, j); } finally { activity = old; }
                if (!lOk) {
                    int k = Math.max(1, Math.min(n - 1, failedStep));
                    nav.blockedEdges.add(Nav.edge(g.idx(path[k * 3 - 3], path[k * 3 - 2], path[k * 3 - 1]), g.idx(path[k * 3], path[k * 3 + 1], path[k * 3 + 2])));
                    unreachable.clear();
                    return fail("ladder " + rel(path[k * 3 - 3], path[k * 3 - 2], path[k * 3 - 1]) + " -> " + rel(path[k * 3], path[k * 3 + 1], path[k * 3 + 2]) + " pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz));
                }
                laddersUsed++;
                i = j;
            } else if (m == Nav.GATE) {
                if (!passGate(path[i * 3 - 3], path[i * 3 - 2], path[i * 3 - 1], path[i * 3], path[i * 3 + 1], path[i * 3 + 2])) {
                    nav.blockedEdges.add(Nav.edge(g.idx(path[i * 3 - 3], path[i * 3 - 2], path[i * 3 - 1]), g.idx(path[i * 3], path[i * 3 + 1], path[i * 3 + 2])));
                    return fail("door between " + rel(path[i * 3 - 3], path[i * 3 - 2], path[i * 3 - 1]) + " and " + rel(path[i * 3], path[i * 3 + 1], path[i * 3 + 2]));
                }
                i++;
            } else if (m == Nav.DIG) {
                if (!digDown(path[(i - 1) * 3], path[(i - 1) * 3 + 1], path[(i - 1) * 3 + 2], path[i * 3 + 1])) return fail("dig at " + rel(path[(i - 1) * 3], path[(i - 1) * 3 + 1], path[(i - 1) * 3 + 2]));
                i++;
            } else {
                int j = i;
                while (j < n && move[j] <= Nav.DROP) j++;
                boolean off = stepOff && (path[i * 3] != behindX || path[i * 3 + 2] != behindZ);
                if (off) j = i + 1;   // first only the step off the column, to take it down from there
                if (!pendingClose.isEmpty()) j = i + 1;   // one step at a time until the gate behind is closed
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
                closePending();
                i = j;
            }
        }
        stepOff = false;
        tStep = -1;
        return true;
    }

    long laddersUsed;

    /**
     * Ladder moves path[from..n): into a ladder's cell, up or down inside it, off it onto a floor. Jump held makes the
     * player rise on a ladder, nothing held lets it sink slowly; pushing towards the way off does the rest. Closed
     * trapdoors over the ladder are opened on the way and closed again behind.
     */
    boolean ladder(int[] path, int from, int n) {
        c.holdAttack(false);
        it.unimi.dsi.fastutil.ints.IntArrayList opened = new it.unimi.dsi.fastutil.ints.IntArrayList();
        boolean ok = true;
        for (int i = from; i < n && ok; i++) {
            failedStep = i;
            int wx = path[i * 3], wy = path[i * 3 + 1], wz = path[i * 3 + 2];
            if (g.rung(wx, wy, wz) == 3) {
                if (!clickHatch(wx, wy, wz)) { ok = false; break; }
                opened.add(g.idx(wx, wy, wz));
            }
            double feet = feetOf(wx, wy, wz);
            boolean hang = Double.isNaN(feet);
            if (hang && !g.hang(wx, wy, wz)) { ok = false; break; }
            double ty = hang ? wy : feet;
            int budget = 80 + 20 * Math.abs(wy - cy());
            while (true) {
                double dx = wx + 0.5 - c.x(), dz = wz + 0.5 - c.z();
                double dist = Math.sqrt(dx * dx + dz * dz);
                boolean inCol = cx() == wx && cz() == wz;
                if (hang ? inCol && dist < 0.2 && c.y() >= ty - 0.05 && c.y() < ty + 0.6 : inCol && c.onGround() && Math.abs(c.y() - feet) < 0.02) break;
                if (--budget < 0) { ok = false; break; }
                boolean onRung = g.rung(cx(), cy(), cz()) != 0;
                boolean jump = c.y() < ty + 0.02 && (!c.onGround() || onRung);
                if (dist > 1e-3) c.look(yawTo(dx, dz), c.pitch());
                c.keys(dist > 0.06, false, false, false, jump, false, false);
                tick();
            }
            if (ok) closeHatches(opened);
        }
        c.releaseKeys();
        if (ok && g.stand16(path[n * 3 - 3], path[n * 3 - 2], path[n * 3 - 1]) >= 0) {
            one[0] = path[n * 3 - 3]; one[1] = path[n * 3 - 2]; one[2] = path[n * 3 - 1];
            ok = follow(one, 0, 1);
            closeHatches(opened);
        }
        return ok;
    }

    /** One click on a trapdoor from wherever the bot is (on the ground or on a ladder); true if it changed. */
    boolean clickHatch(int x, int y, int z) {
        c.releaseKeys();
        if (c.onGround() && hspeed() > 0.003) settle();
        int before = g.get(x, y, z);
        if (before == 0 || !aimAtBlock(x, y, z)) return false;
        c.clickUse();
        tick();
        if (g.get(x, y, z) == before) tick();
        return g.get(x, y, z) != before;
    }

    /** Closes the trapdoors opened on the way as soon as the bot is clear of them (above, or far enough below). */
    void closeHatches(it.unimi.dsi.fastutil.ints.IntArrayList opened) {
        for (int k = opened.size() - 1; k >= 0; k--) {
            int cell = opened.getInt(k);
            int x = g.xOf(cell), y = g.yOf(cell), z = g.zOf(cell);
            if (StateInfo.rung[g.cur[cell]] != 2) { opened.removeInt(k); continue; }
            boolean above = c.y() >= y + 0.99, below = c.y() <= y - 0.99 && c.y() > y - 3;
            if (!above && !below && c.y() < y && c.y() > y - 1.6 && !c.onGround()) {
                // just under it, on the ladder: sink until the head is clear of where the trapdoor will be
                c.releaseKeys();
                for (int t = 0; t < 12 && c.y() > y - 0.99 && !c.onGround(); t++) tick();
                below = c.y() <= y - 0.99;
            }
            if (!above && !below) continue;
            if (above && !c.onGround()) continue;
            opened.removeInt(k);
            if (g.tgt[cell] != 0 && matches(g.cur[cell], g.tgt[cell])) continue;   // (the build wants it open)
            clickHatch(x, y, z);
        }
    }

    /** Walks from cell a to the neighbouring cell b through a door or gate: opens what is in the way, steps through, closes it again. */
    boolean passGate(int ax, int ay, int az, int bx, int by, int bz) {
        int d = bx > ax ? 0 : bx < ax ? 1 : bz > az ? 2 : 3;
        closePending();
        if (!centre(ax, ay, az) || !middle(ax, az)) return false;
        int[] opened = new int[4];
        int n = 0;
        for (int k = 0; k < 4; k++) {
            int x = k < 2 ? ax : bx, y = (k < 2 ? ay : by) + (k & 1), z = k < 2 ? az : bz;
            int st = g.get(x, y, z);
            boolean blocks = (StateInfo.doorEdges[st] >> (k < 2 ? d : d ^ 1) & 1) != 0 || (k == 2 && StateInfo.closedGate[st]);
            if (!blocks) continue;
            if (!StateInfo.handDoor[st] || !toggle(x, y, z)) { for (int j = 0; j < n; j++) toggle(g.xOf(opened[j]), g.yOf(opened[j]), g.zOf(opened[j])); return false; }
            opened[n++] = g.idx(x, y, z);
        }
        one[0] = bx; one[1] = by; one[2] = bz;
        boolean ok = follow(one, 0, 1);
        if (ok) middle(bx, bz);
        // close behind (unless it now is the way the build wants it)
        for (int j = 0; j < n; j++) {
            int cell = opened[j];
            if (g.tgt[cell] != 0 && matches(g.cur[cell], g.tgt[cell])) continue;
            // (a gate fills its cell: it is closed once the bot has left it, not around the bot)
            boolean fillsCell = StateInfo.state[g.cur[cell]].getBlock() instanceof net.minecraft.world.level.block.FenceGateBlock;
            if (fillsCell && (bodyIn(g.xOf(cell), g.yOf(cell), g.zOf(cell)) || bodyIn(g.xOf(cell), g.yOf(cell) - 1, g.zOf(cell)))) { pendingClose.add(cell); continue; }
            if (!toggle(g.xOf(cell), g.yOf(cell), g.zOf(cell))) {
                if (TRACE && verbose) log("gate at " + rel(g.xOf(cell), g.yOf(cell), g.zOf(cell)) + " not closed again: " + StateInfo.name(g.cur[cell]) + " eye=" + String.format("%.2f,%.2f,%.2f", c.eye().x - g.ox, c.eye().y - g.oy, c.eye().z - g.oz));
                pendingClose.add(cell);
            }
        }
        if (ok) gatesPassed++;
        return ok;
    }

    /** Gates the bot opened and is (or was) still standing in. */
    final it.unimi.dsi.fastutil.ints.IntArrayList pendingClose = new it.unimi.dsi.fastutil.ints.IntArrayList();

    /** Closes the gates left open behind as soon as the bot is out of them (what cannot be reached any more is set right at the end). */
    void closePending() {
        if (pendingClose.isEmpty()) return;
        for (int k = pendingClose.size() - 1; k >= 0; k--) {
            int cell = pendingClose.getInt(k);
            int x = g.xOf(cell), y = g.yOf(cell), z = g.zOf(cell);
            if (bodyIn(x, y, z) || bodyIn(x, y - 1, z)) continue;
            pendingClose.removeInt(k);
            if (g.tgt[cell] != 0 && g.cur[cell] != 0 && !matches(g.cur[cell], g.tgt[cell]) && StateInfo.handDoor[g.cur[cell]]) toggle(x, y, z);
        }
    }

    /** One click on a door or gate; true if it changed. */
    boolean toggle(int x, int y, int z) {
        if (!c.onGround() || hspeed() > 0.003) settle();
        int before = g.get(x, y, z);
        if (before == 0 || !aimAtBlock(x, y, z)) return false;
        c.clickUse();
        tick();
        if (g.get(x, y, z) == before) tick();
        return g.get(x, y, z) != before;
    }

    boolean stepOff;
    int behindX, behindZ;

    /** During the final clean-up: having stepped off a scaffolding column, break as much of it as can be reached from here. */
    void takeDownBehind(int x, int z) {
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

    boolean centre(int x, int y, int z) {
        if (cx() == x && cz() == z && Math.abs(c.x() - x - 0.5) < 0.2 && Math.abs(c.z() - z - 0.5) < 0.2 && c.onGround()) return true;
        one[0] = x; one[1] = y; one[2] = z;
        return follow(one, 0, 1);
    }

    /**
     * Standing in column (x,z) at level y: makes sure a scaffolding column carries the bot up to level toY (feet),
     * adding blocks to it as needed, and climbs there. Blocks are added the way a player does it: the first one
     * onto the floor, further ones by clicking the column from inside it (they appear on top).
     */
    boolean tower(int x, int y, int z, int toY) {
        String old = enter("tower");
        try { return towerImpl(x, y, z, toY); } finally { activity = old; }
    }

    boolean towerImpl(int x, int y, int z, int toY) {
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
            else if (!sHere && !sBelow) { pitch = 90; sneak = StateInfo.interactive[g.get(x, f - 1, z)]; }   // first block: onto the floor, into the own cell
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



    /**
     * Off. Tried: it clears the build of short columns as it goes (no scaffolding left on three test builds), but the
     * building takes up to 75 % longer, and on a 14k-block tower the bot rode a column down into a spot it could not
     * leave. Needs the trap check applied to where the ride down ends before it can be switched on.
     */
    static final boolean ROPE_PERCHED = false;

    /**
     * A scaffolding column that does not stand on the ground but somewhere up on the build is hard to get back to
     * later, so it is treated like loose blocks: its new blocks go onto the rope and come down again (the bot rides
     * it down, breaking it underfoot) before the bot goes elsewhere. Columns from the ground stay as lifts.
     */
    int ropeIn(int x, int z, int toY) {
        if (!ROPE_PERCHED) return 0;
        int top = toY - 1;
        while (top >= g.y0 && !(g.isTower(x, top, z) && scaffold.contains(g.idx(x, top, z)))) top--;
        if (top < g.y0) return 0;
        int foot = top;
        while (foot - 1 >= g.y0 && g.isTower(x, foot - 1, z) && scaffold.contains(g.idx(x, foot - 1, z))) foot--;
        // (on or just above the ground, or tall: worth keeping; a tall one is dear to put up again)
        if (foot <= g.oy + 3 || top - foot >= 8) return 0;
        int n = 0;
        for (int y = foot; y <= top; y++) {
            int cell = g.idx(x, y, z);
            boolean on = false;
            for (int[] r : rope) if (r[7] == cell) { on = true; break; }
            if (on) continue;
            rope.add(new int[] {Nav.PILLAR, x, y, z, x, y + 1, z, cell});
            n++;
        }
        return n;
    }

    /** Standing in cell (x,y,z) on a full block: jump and put scaffolding into that cell, ending on top of it. */
    boolean pillar(int x, int y, int z) {
        String old = enter("pillar");
        try { return pillarImpl(x, y, z); } finally { activity = old; }
    }

    boolean pillarImpl(int x, int y, int z) {
        if (count(scaffoldItem) == 0) return false;
        if (!hold(scaffoldItem) || !centre(x, y, z)) return false;
        settle();
        double y0 = c.y();
        c.look(c.yaw(), 90);
        // (on a chest, a dropper ...: a plain click would open it, so the bot sneaks)
        boolean sneak = StateInfo.interactive[g.get(x, y - 1, z)];
        if (sneak) { c.keys(false, false, false, false, false, true, false); tick(); }
        c.keys(false, false, false, false, true, sneak, false);
        boolean ok = false;
        for (int n = 0; n < 14; n++) {
            if (!ok && c.y() >= y + 1.0 && g.get(x, y, z) == 0) c.clickUse();   // (y0 is y, or half a block more on a fence)
            tick();
            if (n == 0) c.keys(false, false, false, false, false, sneak, false);
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
    boolean bridge(int px, int py, int pz, int x, int z) {
        String old = enter("bridge");
        try { return bridgeImpl(px, py, pz, x, z); } finally { activity = old; }
    }

    boolean bridgeImpl(int px, int py, int pz, int x, int z) {
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
    boolean digDown(int x, int y, int z, int targetY) {
        String old = enter("dig");
        try { return digDownImpl(x, y, z, targetY); } finally { activity = old; }
    }

    boolean digDownImpl(int x, int y, int z, int targetY) {
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

    /** Gets next to a walled-in cell by breaking a way to it through the build (what is broken is put back later). */
    boolean tunnelTo(int x, int y, int z) {
        undoRope();
        for (int dy = 0; dy >= -1; dy--) for (int d = 0; d < 4; d++) {
            int nx = x + DXH[d], ny = y + dy, nz = z + DZH[d];
            if (!g.in(nx, ny, nz) || ny < g.oy) continue;
            nav.mine = this::mineTicks;
            nav.effort = 20;
            boolean ok = search(nx, ny, nz, 60000);
            if (!ok) { desperate = true; ok = search(nx, ny, nz, 60000); desperate = false; }
            nav.mine = null;
            nav.effort = 1;
            if (verbose && tunnelLogs++ < 30) log("tunnel to " + rel(nx, ny, nz) + " for " + rel(x, y, z) + ": " + ok + " expanded=" + nav.expanded + " cell=" + StateInfo.name(g.get(nx, ny, nz)) + " below=" + StateInfo.name(g.get(nx, ny - 1, nz)) + " mineTicks=" + mineTicks(nx, ny, nz) + "/" + mineTicks(nx, ny + 1, nz));
            if (ok && travel()) return true;
        }
        return false;
    }
}
