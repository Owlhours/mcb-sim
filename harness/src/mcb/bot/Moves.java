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
        // cheap test first: plenty of room to walk, climb and drop to from here means not shut in
        towersOff = true;
        boolean ok = nav.search(x, y, z, ax, ay, az, 96, null) || nav.expanded > 96;
        towersOff = false;
        if (!ok) { trapSearches++; ok = nav.search(x, y, z, ax, ay, az, 30000, this::scaffoldOk); }
        if (!ok && breaking) {
            nav.mine = this::mineTicks;
            nav.effort = 20;
            ok = nav.search(x, y, z, ax, ay, az, 60000, this::scaffoldOk);
            nav.effort = 1;
        }
        nav.mine = keep;
        System.arraycopy(path, 0, nav.path, 0, path.length);
        System.arraycopy(move, 0, nav.move, 0, move.length);
        nav.length = len; nav.goalCost = cost; nav.scaffoldMoves = sm; nav.towerMoves = tm;
        return ok;
    }

    /** Would putting the block into (x,y,z) from where the bot stands close its own way out? */
    boolean sealsIn(int x, int y, int z, int t) {
        if (StateInfo.walkThrough[t] || !c.onGround()) return false;
        int bx = cx(), by = cy(), bz = cz();
        // (whatever is being closed off may be some way off: a hatch in the ceiling of the room the bot stands in)
        if (Math.abs(x - bx) > 6 || Math.abs(z - bz) > 6 || y < by - 4 || y > by + 6 || !g.in(bx, by, bz)) return false;
        int i = g.idx(x, y, z);
        char old = g.cur[i];
        g.cur[i] = (char) t;
        boolean trapped = !wayOut(bx, by, bz, acceptTraps);
        g.cur[i] = old;
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
        if (!ok && nav.expanded <= 1 && !(cx() == x && cy() == y && cz() == z) && unstick()) {
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
    boolean unstick() {
        if (ticks - lastUnstick < 200) return false;
        lastUnstick = ticks;
        int x0 = cx(), y0 = cy(), z0 = cz();
        // standing on something low that fills the own cell (a comparator, a slab ...) with no step possible: take
        // it out from under the feet (it is put back later) to stand on the floor below
        int h0 = g.in(x0, y0, z0) ? g.stand16(x0, y0, z0) : -1;
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

    int mineTicks(int x, int y, int z) {
        if (!g.in(x, y, z) || y < g.oy) return -1;   // (never the ground the site stands on)
        int w = g.get(x, y, z);
        if (w == 0) return 0;
        if (StateInfo.gravity[g.get(x, y + 1, z)]) return -1;
        // (broken ice leaves water behind, and water runs)
        if (StateInfo.state[w].getBlock() == net.minecraft.world.level.block.Blocks.ICE) return -1;
        // what is broken is gone (nothing drops): only as many as the chests can make good
        if (spare(StateInfo.item[w]) <= 0) return -1;
        // nothing may hang on it or stand on it that would come off with it
        for (int d = 0; d < 6; d++) {
            int n = g.get(x + DX[d], y + DY[d], z + DZ[d]);
            if (n != 0 && d != 0 && !StateInfo.scaffolding[n] && !oracle.floats(n)) return -1;
        }
        if (z < g.oz - 2 && StateInfo.state[w].getBlock() instanceof net.minecraft.world.level.block.ChestBlock) return -1;
        int t = breakTicks(w);
        return t < 0 || t > 1500 ? -1 : t;
    }

    boolean route(int x, int y, int z, int maxExpand) {
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
                for (int k = 1; k < nav.length && !oneWay; k++) oneWay = nav.move[k] == Nav.DROP || (nav.move[k] == Nav.WALK && nav.path[k * 3 + 1] < nav.path[k * 3 - 2]);
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
        try { return travelImpl(); } finally { activity = old; }
    }

    boolean travelImpl() {
        int n = nav.length;
        // nav's arrays are reused by later searches: copy the route
        int[] path = java.util.Arrays.copyOf(nav.path, n * 3);
        byte[] move = java.util.Arrays.copyOf(nav.move, n);
        if (TRACE && verbose) {
            StringBuilder sb = new StringBuilder("route:");
            for (int k = 0; k < n; k++) sb.append(' ').append(k == 0 ? "" : "WJDPBdcMCSTG".charAt(move[k]) + ">").append(rel(path[k * 3], path[k * 3 + 1], path[k * 3 + 2]));
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

    /** Walks from cell a to the neighbouring cell b through a door or gate: opens what is in the way, steps through, closes it again. */
    boolean passGate(int ax, int ay, int az, int bx, int by, int bz) {
        int d = bx > ax ? 0 : bx < ax ? 1 : bz > az ? 2 : 3;
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
            toggle(g.xOf(cell), g.yOf(cell), g.zOf(cell));
        }
        if (ok) gatesPassed++;
        return ok;
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
            if (!ok && c.y() >= y0 + 1.0 && g.get(x, y, z) == 0) c.clickUse();
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
            nav.mine = null;
            nav.effort = 1;
            if (verbose && tunnelLogs++ < 30) log("tunnel to " + rel(nx, ny, nz) + " for " + rel(x, y, z) + ": " + ok + " expanded=" + nav.expanded + " cell=" + StateInfo.name(g.get(nx, ny, nz)) + " below=" + StateInfo.name(g.get(nx, ny - 1, nz)) + " mineTicks=" + mineTicks(nx, ny, nz) + "/" + mineTicks(nx, ny + 1, nz));
            if (ok && travel()) return true;
        }
        return false;
    }
}
