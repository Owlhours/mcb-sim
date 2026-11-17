package mcb.bot;

import it.unimi.dsi.fastutil.ints.IntOpenHashSet;
import mcb.build.StateInfo;

/**
 * Route finding over the cells the player can stand in (A*). Moves: walk to a neighbouring column (stepping up to
 * 0.6), jump up one block, drop any distance, and three moves that involve temporary scaffolding: pillar (jump
 * and put a block under the feet), bridge (put a block beside the one stood on, then step onto it) and dig (break
 * the scaffold block stood on and sink one block - how pillars are taken down).
 */
public final class Nav {
    public static final byte WALK = 0, JUMP = 1, DROP = 2, PILLAR = 3, BRIDGE = 4, DIG = 5, CLEAR = 6, MINE = 7, CLIMB = 8, DESCEND = 9, TOWER = 10, GATE = 11, LADDER = 12;
    /** Says how long a block takes to break by hand, or -1 if the bot must not break it (see Builder). */
    public interface MineRule { int ticks(int x, int y, int z); }
    /** When set, routes may break through blocks of the build itself (a last resort; they are put back later). */
    public MineRule mine;
    /** Multiplier on the search effort allowed per route (routes that must break through things need more). */
    public int effort = 1;
    public static final int C_MINE = 400;
    /** Cost per scaffold block broken to walk through where it stood. */
    public static final int C_CLEAR = 12;
    public static final int C_WALK = 5, C_JUMP = 9, C_DROP = 6, C_PILLAR = 12, C_BRIDGE = 22, C_DIG = 2;
    /** Scaffolding columns: rising inside one, sinking through one, and adding a block to one (then rising). They stay up. */
    public static final int C_CLIMB = 9, C_DESCEND = 7, C_TOWER = 13;
    /** Opening a door or gate, walking through and closing it again. */
    public static final int C_GATE = 14;
    /** Extra cost of walking or jumping off scaffolding that is then left standing (it has to be come back for). */
    public static final int C_ABANDON = 45;
    public static final int C_PIT = 150;

    public interface CellRule { boolean ok(int x, int y, int z); }

    private final Grid g;
    private final IntOpenHashSet scaffold;
    /** Steps that failed when tried (from-cell, to-cell): not offered again. */
    public final it.unimi.dsi.fastutil.longs.LongOpenHashSet blockedEdges = new it.unimi.dsi.fastutil.longs.LongOpenHashSet();
    public static long edge(int from, int to) { return ((long) from << 32) | (to & 0xffffffffL); }
    private final int[] stamp, cost, parent;
    private final byte[] how;
    private int epoch;
    private long[] heap = new long[1024];
    private int heapSize;
    public int scaffoldPenalty = 40;
    public int towerPenalty = 3;
    public int perchPenalty = 0;   // (tried 40: far more scaffolding and time for nothing)
    /** Where a scaffolding column may be put up or extended (null = nowhere). */
    public CellRule towerRule;
    /** Whether scaffolding columns can be put up at all just now (sets what a climb is expected to cost). */
    public boolean towersOn = true;
    /** Added to the search effort allowed for the next route (a tall column to be put up from far below, say). */
    public int extraEffort;
    public int expanded;
    /** After a search that found nothing: true if every place that can be reached was looked at (so there is no route). */
    public boolean exhausted;
    /** Cells that end a search as well as the goal does (places from which a way to the goal is already known), or null. */
    public IntOpenHashSet stopAt;
    /** The cell at which the last successful search ended (the goal, or one of stopAt). */
    public int reached = -1;
    public long totalExpanded, searches;
    public boolean virtualStand;
    /** Height of the feet in the start cell (1/16) when the bot is known to be standing there, else -1: the body's
     *  real position overrides what the grid says about the start cell. */
    public int startH = -1;
    /** PFHEUR (off with MCB_NO_PFHEUR): the searches of wayOut that are not settled after ASK_FIRST cells go on as the
     *  search that looks at every place once. PFTUN (off with MCB_NO_PFTUN): tunnelTo first asks once for a way to any
     *  of the places next to the cell. (Simulator revision `askonce`.) */
    public static final boolean PFHEUR = System.getenv("MCB_NO_PFHEUR") == null, PFTUN = System.getenv("MCB_NO_PFTUN") == null;
    public static final int ASK_FIRST = 1000;
    /** The search that looks at every place once: places are taken in the order of their distance to the goal alone,
     *  and a place once reached is never taken up again. For questions of "is there a way", not for routes. */
    private boolean once;
    /** When > 0: a search that has expanded this many cells goes on as the search that looks at every place once. */
    private int onceAfter;
    /** Of the last search: whether it went on that way, and the budget it had (after the cut by distance). */
    public boolean wentOnce;
    public int lastBudget;
    /** When not null (and not empty): the search ends at any of these cells instead of the goal cell, and heads for
     *  the nearest of them (tunnelHopeless: "next to the cell"). */
    public int[] goalAny;

    // result
    public int[] path = new int[96];
    public byte[] move = new byte[32];
    public int length;
    public int goalCost;
    /** Number of pillar/bridge moves on the route found. */
    public int scaffoldMoves;
    /** Number of scaffolding blocks the route adds to columns. */
    public int towerMoves;

    public Nav(Grid g, IntOpenHashSet scaffold) {
        this.g = g;
        this.scaffold = scaffold;
        int n = g.cur.length;
        stamp = new int[n]; cost = new int[n]; parent = new int[n]; how = new byte[n];
    }

    private void push(long key, int node) {
        if (heapSize == heap.length) heap = java.util.Arrays.copyOf(heap, heap.length * 2);
        long v = (key << 32) | (node & 0xffffffffL);
        int i = heapSize++;
        while (i > 0) {
            int p = (i - 1) >> 1;
            if (heap[p] <= v) break;
            heap[i] = heap[p];
            i = p;
        }
        heap[i] = v;
    }

    private long pop() {
        long top = heap[0];
        long v = heap[--heapSize];
        int i = 0;
        while (true) {
            int l = 2 * i + 1;
            if (l >= heapSize) break;
            int r = l + 1;
            int m = r < heapSize && heap[r] < heap[l] ? r : l;
            if (heap[m] >= v) break;
            heap[i] = heap[m];
            i = m;
        }
        heap[i] = v;
        return top;
    }

    private int tx, ty, tz;

    /**
     * Estimated remaining cost, deliberately weighted (x2): the search heads for the goal and climbs when it has
     * to instead of flooding every cell it could walk to. Routes are good, not guaranteed shortest.
     */
    private int heuristic(int x, int y, int z) {
        if (goalAny != null) {
            int best = Integer.MAX_VALUE;
            for (int k : goalAny) best = Math.min(best, heuristicTo(x, y, z, g.xOf(k), g.yOf(k), g.zOf(k)));
            return best;
        }
        return heuristicTo(x, y, z, tx, ty, tz);
    }

    private int heuristicTo(int x, int y, int z, int tx, int ty, int tz) {
        int dy = ty - y;
        return (mine != null ? 5 : 1) * (2 * (Math.abs(tx - x) + Math.abs(tz - z)) * C_WALK + (dy > 0 ? dy * (towersOn ? C_TOWER + towerPenalty : C_PILLAR + scaffoldPenalty) : -dy * 2));
    }

    private void relax(int from, int x, int y, int z, int c, byte h) {
        if (!g.in(x, y, z)) return;
        int i = g.idx(x, y, z);
        if (stamp[i] == epoch && (cost[i] <= c || once)) return;
        if (!blockedEdges.isEmpty() && blockedEdges.contains(edge(from, i))) return;
        stamp[i] = epoch; cost[i] = c; parent[i] = from; how[i] = h;
        push((once ? 0 : c) + heuristic(x, y, z), i);
    }

    private boolean atGoalAny(int i) {
        for (int k : goalAny) if (k == i) return true;
        return false;
    }

    /**
     * A search whose caller wants only "is there a way" (and, if so, some way to remember). As search; with PFHEUR
     * a search that has not settled the question after ASK_FIRST cells goes on as the search that looks at every
     * place once, under the same budget: the usual search takes cells up again and again as it finds cheaper ways to
     * them, and runs out of its budget in places it could have looked through.
     */
    public boolean ask(boolean on, int sx, int sy, int sz, int gx, int gy, int gz, int maxExpand, CellRule scaffoldOk) {
        if (!on) return search(sx, sy, sz, gx, gy, gz, maxExpand, scaffoldOk);
        onceAfter = ASK_FIRST;
        try { return search(sx, sy, sz, gx, gy, gz, maxExpand, scaffoldOk); } finally { onceAfter = 0; once = false; }
    }

    /**
     * ask for the search whose failure is read as "shut in" (dry) or "unknown" (out of budget). The usual search runs
     * dry within its budget only in a region it can look through with all its taking-up-again; the search that looks
     * at every place once runs dry in any region smaller than the budget. So that "shut in" is said where it was said
     * before: dry after looking at more than a quarter of the budget's worth of places - the usual search decides.
     * (Out of budget with every place looked at once: the usual search cannot have run dry either.)
     */
    public boolean askLast(boolean on, int sx, int sy, int sz, int gx, int gy, int gz, int maxExpand, CellRule scaffoldOk) {
        boolean r = ask(on, sx, sy, sz, gx, gy, gz, maxExpand, scaffoldOk);
        if (on && !r && exhausted && wentOnce && expanded > lastBudget / 4) r = search(sx, sy, sz, gx, gy, gz, maxExpand, scaffoldOk);
        return r;
    }

    /** From here on every place once: what is waiting is put in the order of its distance to the goal (entries
     *  outdated by a cheaper way to the same cell are dropped), and no cell is taken up again. */
    private void goOnOnce() {
        once = true;
        wentOnce = true;
        long[] old = java.util.Arrays.copyOf(heap, heapSize);
        heapSize = 0;
        for (long v : old) {
            int n = (int) v;
            int h = heuristic(g.xOf(n), g.yOf(n), g.zOf(n));
            if ((int) (v >>> 32) == cost[n] + h) push(h, n);
        }
    }

    private static final int[] DX = {1, -1, 0, 0}, DZ = {0, 0, 1, -1};
    static final boolean X_FPILLAR = System.getenv("MCB_NO_FPILLAR") == null;

    /**
     * Cheapest route from the start cell to the goal cell. scaffoldOk says where temporary blocks may go (null = no
     * pillar/bridge moves). Gives up after maxExpand cells. The goal need not be standable yet if it can be reached
     * by a scaffolding move.
     */
    public boolean search(int sx, int sy, int sz, int gx, int gy, int gz, int maxExpand, CellRule scaffoldOk) {
        epoch++;
        heapSize = 0;
        once = false;
        expanded = 0;
        length = 0;
        searches++;
        exhausted = false;
        reached = -1;
        tx = gx; ty = gy; tz = gz;
        if (!g.in(sx, sy, sz) || !g.in(gx, gy, gz)) return false;
        // effort in proportion to the distance: a hopeless search near by must not cost as much as a long trip
        maxExpand = Math.min(maxExpand, effort * (400 + 80 * (Math.abs(gx - sx) + Math.abs(gz - sz) + 3 * Math.abs(gy - sy))) + extraEffort);
        scaffoldMoves = 0;
        towerMoves = 0;
        wentOnce = false;
        lastBudget = maxExpand;
        int start = g.idx(sx, sy, sz), goal = g.idx(gx, gy, gz);
        stamp[start] = epoch; cost[start] = 0; parent[start] = -1; how[start] = WALK;
        push(heuristic(sx, sy, sz), start);
        while (heapSize > 0) {
            long top = pop();
            int i = (int) top;
            int c = cost[i];
            if (!once && (int) (top >>> 32) != c + heuristic(g.xOf(i), g.yOf(i), g.zOf(i))) continue; // stale entry
            int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
            if ((goalAny != null ? atGoalAny(i) : i == goal) || (stopAt != null && stopAt.contains(i))) { build(i); goalCost = c; reached = i; totalExpanded += expanded; return true; }
            if (++expanded > maxExpand) break;
            if (expanded == onceAfter && !once) goOnOnce();
            boolean onTower = how[i] == TOWER;
            virtualStand = how[i] == PILLAR || how[i] == BRIDGE || onTower;
            // a cell entered by digging or clearing holds scaffold now, but will be empty with the bot standing in it
            boolean dug = how[i] == DIG || how[i] == CLEAR || how[i] == MINE;
            // (where the bot sinks to by digging or mining, it stands on whatever is below: on a post or wall that
            // is half a block up, and the head room is wanted for that - a drop planned from the top of a fence
            // under a ceiling as if from the floor left the bot there for good)
            int h = virtualStand ? 0 : dug ? Math.max(0, StateInfo.colTop[g.get(x, y - 1, z)] - 16) : g.stand16(x, y, z);
            if (i == start && h < 0 && startH >= 0) h = startH;
            if (h < 0 && g.hang(x, y, z)) { ladderMoves(i, x, y, z, c); continue; }
            if (h < 0) {
                for (int ty = y - 1; ty >= g.y0; ty--) {
                    if (g.stand16(x, ty, z) >= 0) { relax(i, x, ty, z, c + C_DROP, DROP); break; }
                    if (!g.passable(x, ty, z)) break;
                }
                continue;
            }
            boolean headUp = g.passable(x, y + 2, z) || (dug && g.passable(x, y + 3, z) && false);
            if (dug) headUp = true;
            boolean floorTower = onTower || g.isTower(x, y - 1, z);
            boolean onScaffold = !virtualStand && !floorTower && h == 0 && g.in(x, y - 1, z) && scaffold.contains(g.idx(x, y - 1, z));
            int leave = onScaffold ? C_ABANDON : 0;
            for (int d = 0; d < 4; d++) {
                int nx = x + DX[d], nz = z + DZ[d];
                // doors and gates: closed ones are opened to walk through on the level; nothing else goes through a door leaf
                int door = doorway(x, y, z, d, nx, nz);
                if (door != 0) {
                    if (door > 0 && h == 0 && !virtualStand) {
                        int nb = g.get(nx, y, nz);
                        int floor = g.get(nx, y - 1, nz);
                        boolean stand = StateInfo.closedGate[nb] ? StateInfo.colTop[floor] == 16 && StateInfo.anyTop[floor] == 16 && g.passable(nx, y + 1, nz) : g.stand16(nx, y, nz) == 0;
                        if (stand) relax(i, nx, y, nz, c + C_WALK + C_GATE, GATE);
                    }
                    continue;
                }
                // ladders: onto one beside the bot ...
                if (h <= 8 && g.hang(nx, y, nz)) { relax(i, nx, y, nz, c + C_WALK + (g.rung(nx, y, nz) == 3 ? C_GATE : 0), LADDER); continue; }
                int hu = headUp ? g.stand16(nx, y + 1, nz) : -1;
                int hl = g.stand16(nx, y, nz);
                // ... or down one through the closed trapdoor in the floor beside the bot
                if (h == 0 && hl == 0 && g.rung(nx, y - 1, nz) == 3) relax(i, nx, y - 1, nz, c + C_WALK + C_GATE + C_DESCEND, LADDER);
                if (hl >= 0) {
                    int rise = hl - h;
                    // stepping up needs the head room on this side too
                    if (rise <= 9) { if (rise <= 0 ? headClear(nx, y, nz, h) : (hl + 29 <= 32 || headUp)) relax(i, nx, y, nz, c + C_WALK, WALK); }
                    else if (rise <= 19 && headUp) relax(i, nx, y, nz, c + C_JUMP, JUMP);
                } else if (hu >= 0) {
                    int rise = 16 + hu - h;
                    if (rise <= 9) relax(i, nx, y + 1, nz, c + C_WALK, WALK);
                    else if (rise <= 19) relax(i, nx, y + 1, nz, c + C_JUMP, JUMP);
                } else if (h == 0 && clearable(nx, y, nz)) {
                    // own scaffolding in the way on level ground: take it down and walk on
                    int n = (isScaffold(nx, y, nz) ? 1 : 0) + (isScaffold(nx, y + 1, nz) ? 1 : 0);
                    relax(i, nx, y, nz, c + C_WALK + n * C_CLEAR, CLEAR);
                } else if (mine != null && mineThrough(i, nx, y, nz, c)) {
                    // (relaxed inside)
                } else if (g.passable(nx, y, nz) && g.passable(nx, y + 1, nz) && headClear(nx, y, nz, h)) {
                    boolean stepDown = false;
                    for (int ty = y - 1; ty >= g.y0; ty--) {
                        int hd = g.stand16(nx, ty, nz);
                        if (hd >= 0) {
                            int fall = (y - ty) * 16 + h - hd;
                            // going down onto a slab-like floor can be a one-way trip (no pillaring up from it)
                            relax(i, nx, ty, nz, c + (fall <= 9 ? C_WALK : C_DROP + (y - ty) / 2 + leave + (hd != 0 ? C_PIT : 0)), fall <= 9 ? WALK : DROP);
                            stepDown = fall <= 10;
                            break;
                        }
                        // (a ladder in the hole: caught by it)
                        if (g.hang(nx, ty, nz)) { relax(i, nx, ty, nz, c + C_WALK + C_DESCEND * (y - ty), LADDER); break; }
                        if (!g.passable(nx, ty, nz)) break;
                    }
                    // (a sneaking player is not held back at an edge that is only a step down, so no bridging there)
                    if (scaffoldOk != null && h == 0 && !stepDown && !floorTower
                        && (virtualStand || Ray.fullOutline(g.get(x, y - 1, z))) && g.isAir(nx, y - 1, nz) && scaffoldOk.ok(nx, y - 1, nz)) {
                        relax(i, nx, y, nz, c + C_BRIDGE + scaffoldPenalty, BRIDGE);
                    }
                }
            }
            // (a loose block may also go on top of a scaffolding column: from it one can bridge out, which scaffolding does not allow)
            if (scaffoldOk != null && h == 0 && headUp && g.isAir(x, y, z) && g.passable(x, y + 1, z) && scaffoldOk.ok(x, y, z)
                && (virtualStand || dug || flatTop(g.get(x, y - 1, z)))) {
                relax(i, x, y + 1, z, c + C_PILLAR + scaffoldPenalty, PILLAR);
            }
            // standing on a fence or wall (half a block up in an empty cell): a jump still clears the cell, the block
            // goes onto the post
            if (X_FPILLAR && scaffoldOk != null && h > 0 && h <= 8 && !virtualStand && !dug && headUp && g.isAir(x, y, z) && g.passable(x, y + 1, z) && scaffoldOk.ok(x, y, z)
                && StateInfo.colTop[g.get(x, y - 1, z)] == 16 + h && !StateInfo.interactive[g.get(x, y - 1, z)]) {
                relax(i, x, y + 1, z, c + C_PILLAR + scaffoldPenalty, PILLAR);
            }
            // last resort: break the block of the build underfoot and drop to whatever is below
            if (mine != null && !virtualStand && !floorTower && h == 0 && g.in(x, y - 1, z) && !scaffold.contains(g.idx(x, y - 1, z))) {
                int t = mine.ticks(x, y - 1, z);
                if (t > 0) {
                    for (int ty = y - 2; ty >= g.y0 - 1; ty--) {
                        int below = g.get(x, ty, z);
                        if (StateInfo.colTop[below] >= 16) { relax(i, x, ty + 1, z, c + C_DROP + t + C_MINE, MINE); break; }
                        if (StateInfo.colTop[below] != 0) break;
                    }
                }
            }
            // the same when standing half a block up on a post or wall below (nothing in the own cell): break the post
            if (mine != null && !virtualStand && !dug && h > 0 && g.passable(x, y, z) && g.in(x, y - 1, z) && !scaffold.contains(g.idx(x, y - 1, z))) {
                int t = mine.ticks(x, y - 1, z);
                if (t > 0) {
                    for (int ty = y - 2; ty >= g.y0 - 1; ty--) {
                        int below = g.get(x, ty, z);
                        if (StateInfo.colTop[below] >= 16) { relax(i, x, ty + 1, z, c + C_DROP + t + C_MINE, MINE); break; }
                        if (StateInfo.colTop[below] != 0) break;
                    }
                }
            }
            // the same when standing on something low inside the own cell (a slab ...): break that
            if (mine != null && !virtualStand && h > 0 && h < 16 && g.in(x, y, z)) {
                int t = mine.ticks(x, y, z);
                if (t > 0) {
                    for (int ty = y - 1; ty >= g.y0 - 1; ty--) {
                        int below = g.get(x, ty, z);
                        if (StateInfo.colTop[below] >= 16) { if (ty + 1 < y) relax(i, x, ty + 1, z, c + C_DROP + t + C_MINE, MINE); break; }
                        if (StateInfo.colTop[below] != 0) break;
                    }
                }
            }
            // take down the scaffold block underfoot and sink
            if (!virtualStand && !floorTower && h == 0 && g.in(x, y - 1, z) && scaffold.contains(g.idx(x, y - 1, z))) {
                for (int ty = y - 1; ty >= g.y0; ty--) {
                    if (ty < y - 1 && !g.passable(x, ty, z)) break;
                    // the cell we sink into: standable once the block is gone if there is support below it
                    int below = g.get(x, ty - 1, z);
                    if (StateInfo.colTop[below] >= 16) { relax(i, x, ty, z, c + C_DIG + (y - 1 - ty) / 2, DIG); break; }
                    if (StateInfo.colTop[below] != 0) break;
                }
            }
            // scaffolding columns: rise inside one, sink through one, or put one up / make one taller
            if (h == 0) {
                boolean sHere = !virtualStand && g.isTower(x, y, z);
                boolean head2 = g.passable(x, y + 2, z);
                if (sHere && head2) relax(i, x, y + 1, z, c + C_CLIMB, CLIMB);
                if (!virtualStand && !dug && g.isTower(x, y - 1, z) && g.stand16(x, y - 1, z) >= 0) relax(i, x, y - 1, z, c + C_DESCEND, DESCEND);
                if (towerRule != null && !sHere && !dug && head2 && g.isAir(x, y, z) && (onTower || towerBase(x, y, z)) && towerRule.ok(x, y, z)) {
                    // a new column that starts up on the build (not on the ground, not on another column) is harder to
                    // get back to when it has to come down
                    boolean perched = !onTower && y > g.oy && !StateInfo.scaffolding[g.get(x, y - 1, z)];
                    relax(i, x, y + 1, z, c + C_TOWER + towerPenalty + (perched ? perchPenalty : 0), TOWER);
                }
            }
        }
        // (nothing left to expand: there is no route. Stopped by the budget: there may be one - the caller must not
        // take that for "no")
        exhausted = heapSize == 0 && expanded <= maxExpand;
        totalExpanded += expanded;
        return false;
    }

    /**
     * Moves of a player on a ladder, feet at the bottom of ladder cell (x,y,z): up and down the ladder, off it onto a
     * floor beside it (at this level, or one up from the top rung), out of its lower end. A closed trapdoor over a
     * ladder counts as part of it (it is opened in passing and closed again).
     */
    private void ladderMoves(int i, int x, int y, int z, int c) {
        if (g.hang(x, y + 1, z)) relax(i, x, y + 1, z, c + C_CLIMB + (g.rung(x, y + 1, z) == 3 ? C_GATE : 0), LADDER);
        boolean out = g.rung(x, y + 1, z) == 0 && g.passable(x, y + 1, z) && g.passable(x, y + 2, z);
        for (int d = 0; d < 4; d++) {
            int nx = x + DX[d], nz = z + DZ[d];
            if (doorway(x, y, z, d, nx, nz) == 0) {
                int hl = g.stand16(nx, y, nz);
                if (hl >= 0 && hl <= 8) relax(i, nx, y, nz, c + C_WALK, LADDER);
            }
            if (out && g.stand16(nx, y + 1, nz) == 0 && doorway(x, y + 1, z, d, nx, nz) == 0) relax(i, nx, y + 1, nz, c + C_CLIMB + C_WALK, LADDER);
        }
        if (g.rung(x, y - 1, z) != 3 && g.hang(x, y - 1, z)) { relax(i, x, y - 1, z, c + C_DESCEND, LADDER); return; }
        for (int ty = y - 1; ty >= g.y0; ty--) {
            if (g.stand16(x, ty, z) >= 0) { relax(i, x, ty, z, c + C_DROP + (y - ty) / 2, LADDER); break; }
            if (g.rung(x, ty, z) != 3 && g.hang(x, ty, z)) { relax(i, x, ty, z, c + C_DESCEND * (y - ty), LADDER); break; }
            if (!g.passable(x, ty, z)) break;
        }
    }

    /**
     * What stands between cell (x,y,z) and its neighbour in direction d for someone walking across on the level:
     * 0 nothing, 1 a door leaf or closed gate that can be opened by hand, -1 one that cannot.
     */
    private int doorway(int x, int y, int z, int d, int nx, int nz) {
        int r = 0;
        for (int k = 0; k < 4; k++) {
            int s = k < 2 ? g.get(x, y + k, z) : g.get(nx, y + k - 2, nz);
            int bit = k < 2 ? d : d ^ 1;
            boolean blocks = (StateInfo.doorEdges[s] >> bit & 1) != 0 || (k == 2 && StateInfo.closedGate[s]);
            if (!blocks) continue;
            if (!StateInfo.handDoor[s]) return -1;
            r = 1;
        }
        return r;
    }

    /** Room for the head in column (x,z) for a body whose feet are h/16 above the bottom of cell y (it is 1.8 high). */
    private boolean headClear(int x, int y, int z, int h) {
        if (h + 29 <= 32) return true;
        int s2 = g.get(x, y + 2, z);
        return StateInfo.walkThrough[s2] || 32 + StateInfo.anyBottom[s2] >= h + 29;
    }

    /** Can a scaffolding block be put into (x,y,z) by someone standing there: on a column, or on a solid, level floor. */
    private boolean towerBase(int x, int y, int z) {
        int below = g.get(x, y - 1, z);
        if (StateInfo.scaffolding[below]) return true;
        if (!StateInfo.sturdyTop[below]) return false;
        return !(g.in(x, y - 1, z) && scaffold.contains(g.idx(x, y - 1, z)));
    }

    /** A block one can put another block on top of and stand on: full, level top face. */
    public static boolean flatTop(int state) {
        if (StateInfo.fullCube[state]) return true;
        if (StateInfo.colTop[state] != 16 || StateInfo.anyTop[state] != 16 || StateInfo.interactive[state]) return false;
        float[] b = Ray.outline(state);
        for (int k = 0; k < b.length; k += 6) if (b[k + 4] == 1f && b[k] == 0f && b[k + 2] == 0f && b[k + 3] == 1f && b[k + 5] == 1f) return true;
        return false;
    }

    /** Break the one or two blocks in the way at feet and head height and walk on, if there is a floor to stand on. */
    private boolean mineThrough(int from, int x, int y, int z, int c) {
        if (!g.in(x, y, z)) return false;
        int below = g.get(x, y - 1, z);
        if (StateInfo.colTop[below] != 16 || StateInfo.anyTop[below] != 16) return false;
        int cost = 0;
        for (int yy = y; yy <= y + 1; yy++) {
            if (g.passable(x, yy, z)) continue;
            int t = mine.ticks(x, yy, z);
            if (t < 0) return false;
            cost += t + C_MINE;
        }
        if (cost == 0) return false;
        relax(from, x, y, z, c + C_WALK + cost, MINE);
        return true;
    }

    private boolean isScaffold(int x, int y, int z) { return g.in(x, y, z) && scaffold.contains(g.idx(x, y, z)) && !g.isTower(x, y, z); }

    /** The two cells of column (x,z) at feet and head height hold nothing but scaffolding (at least one block of it) and there is a floor. */
    private boolean clearable(int x, int y, int z) {
        boolean s0 = isScaffold(x, y, z), s1 = isScaffold(x, y + 1, z);
        if (!s0 && !s1) return false;
        if (!(s0 || g.passable(x, y, z)) || !(s1 || g.passable(x, y + 1, z))) return false;
        int below = g.get(x, y - 1, z);
        return StateInfo.colTop[below] == 16 && StateInfo.anyTop[below] == 16;
    }

    private void build(int goal) {
        int n = 0;
        for (int i = goal; i >= 0; i = parent[i]) n++;
        if (path.length < n * 3) { path = new int[n * 3 + 48]; move = new byte[n + 16]; }
        length = n;
        int k = n - 1;
        for (int i = goal; i >= 0; i = parent[i], k--) {
            path[k * 3] = g.xOf(i); path[k * 3 + 1] = g.yOf(i); path[k * 3 + 2] = g.zOf(i);
            move[k] = how[i];
            if (how[i] == PILLAR || how[i] == BRIDGE) scaffoldMoves++;
            if (how[i] == TOWER) towerMoves++;
        }
    }
}
