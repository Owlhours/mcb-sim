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
    public static final byte WALK = 0, JUMP = 1, DROP = 2, PILLAR = 3, BRIDGE = 4, DIG = 5, CLEAR = 6;
    /** Cost per scaffold block broken to walk through where it stood. */
    public static final int C_CLEAR = 12;
    public static final int C_WALK = 5, C_JUMP = 9, C_DROP = 6, C_PILLAR = 12, C_BRIDGE = 22, C_DIG = 2;
    /** Extra cost of walking or jumping off scaffolding that is then left standing (it has to be come back for). */
    public static final int C_ABANDON = 45;

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
    public int expanded;
    public long totalExpanded, searches;
    public boolean virtualStand;
    /** Height of the feet in the start cell (1/16) when the bot is known to be standing there, else -1: the body's
     *  real position overrides what the grid says about the start cell. */
    public int startH = -1;

    // result
    public int[] path = new int[96];
    public byte[] move = new byte[32];
    public int length;
    public int goalCost;
    /** Number of pillar/bridge moves on the route found. */
    public int scaffoldMoves;

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
        int dy = ty - y;
        return 2 * (Math.abs(tx - x) + Math.abs(tz - z)) * C_WALK + (dy > 0 ? dy * (C_PILLAR + scaffoldPenalty) : 0);
    }

    private void relax(int from, int x, int y, int z, int c, byte h) {
        if (!g.in(x, y, z)) return;
        int i = g.idx(x, y, z);
        if (stamp[i] == epoch && cost[i] <= c) return;
        if (!blockedEdges.isEmpty() && blockedEdges.contains(edge(from, i))) return;
        stamp[i] = epoch; cost[i] = c; parent[i] = from; how[i] = h;
        push(c + heuristic(x, y, z), i);
    }

    private static final int[] DX = {1, -1, 0, 0}, DZ = {0, 0, 1, -1};

    /**
     * Cheapest route from the start cell to the goal cell. scaffoldOk says where temporary blocks may go (null = no
     * pillar/bridge moves). Gives up after maxExpand cells. The goal need not be standable yet if it can be reached
     * by a scaffolding move.
     */
    public boolean search(int sx, int sy, int sz, int gx, int gy, int gz, int maxExpand, CellRule scaffoldOk) {
        epoch++;
        heapSize = 0;
        expanded = 0;
        length = 0;
        searches++;
        tx = gx; ty = gy; tz = gz;
        if (!g.in(sx, sy, sz) || !g.in(gx, gy, gz)) return false;
        // effort in proportion to the distance: a hopeless search near by must not cost as much as a long trip
        maxExpand = Math.min(maxExpand, 400 + 80 * (Math.abs(gx - sx) + Math.abs(gz - sz) + 3 * Math.abs(gy - sy)));
        scaffoldMoves = 0;
        int start = g.idx(sx, sy, sz), goal = g.idx(gx, gy, gz);
        stamp[start] = epoch; cost[start] = 0; parent[start] = -1; how[start] = WALK;
        push(heuristic(sx, sy, sz), start);
        while (heapSize > 0) {
            long top = pop();
            int i = (int) top;
            int c = cost[i];
            if ((int) (top >>> 32) != c + heuristic(g.xOf(i), g.yOf(i), g.zOf(i))) continue; // stale entry
            int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
            if (i == goal) { build(i); goalCost = c; totalExpanded += expanded; return true; }
            if (++expanded > maxExpand) break;
            virtualStand = how[i] == PILLAR || how[i] == BRIDGE;
            // a cell entered by digging or clearing holds scaffold now, but will be empty with the bot standing in it
            boolean dug = how[i] == DIG || how[i] == CLEAR;
            int h = virtualStand || dug ? 0 : g.stand16(x, y, z);
            if (i == start && h < 0 && startH >= 0) h = startH;
            if (h < 0) {
                for (int ty = y - 1; ty >= g.y0; ty--) {
                    if (g.stand16(x, ty, z) >= 0) { relax(i, x, ty, z, c + C_DROP, DROP); break; }
                    if (!g.passable(x, ty, z)) break;
                }
                continue;
            }
            boolean headUp = g.passable(x, y + 2, z) || (dug && g.passable(x, y + 3, z) && false);
            if (dug) headUp = true;
            boolean onScaffold = !virtualStand && h == 0 && g.in(x, y - 1, z) && scaffold.contains(g.idx(x, y - 1, z));
            int leave = onScaffold ? C_ABANDON : 0;
            for (int d = 0; d < 4; d++) {
                int nx = x + DX[d], nz = z + DZ[d];
                int hu = headUp ? g.stand16(nx, y + 1, nz) : -1;
                int hl = g.stand16(nx, y, nz);
                if (hl >= 0) {
                    int rise = hl - h;
                    // stepping up needs the head room on this side too
                    if (rise <= 9) { if (rise <= 0 || hl + 29 <= 32 || headUp) relax(i, nx, y, nz, c + C_WALK, WALK); }
                    else if (rise <= 19 && headUp) relax(i, nx, y, nz, c + C_JUMP, JUMP);
                } else if (hu >= 0) {
                    int rise = 16 + hu - h;
                    if (rise <= 9) relax(i, nx, y + 1, nz, c + C_WALK, WALK);
                    else if (rise <= 19) relax(i, nx, y + 1, nz, c + C_JUMP, JUMP);
                } else if (h == 0 && clearable(nx, y, nz)) {
                    // own scaffolding in the way on level ground: take it down and walk on
                    int n = (isScaffold(nx, y, nz) ? 1 : 0) + (isScaffold(nx, y + 1, nz) ? 1 : 0);
                    relax(i, nx, y, nz, c + C_WALK + n * C_CLEAR, CLEAR);
                } else if (g.passable(nx, y, nz) && g.passable(nx, y + 1, nz)) {
                    boolean stepDown = false;
                    for (int ty = y - 1; ty >= g.y0; ty--) {
                        int hd = g.stand16(nx, ty, nz);
                        if (hd >= 0) {
                            int fall = (y - ty) * 16 + h - hd;
                            relax(i, nx, ty, nz, c + (fall <= 9 ? C_WALK : C_DROP + (y - ty) / 2 + leave), fall <= 9 ? WALK : DROP);
                            stepDown = fall <= 10;
                            break;
                        }
                        if (!g.passable(nx, ty, nz)) break;
                    }
                    // (a sneaking player is not held back at an edge that is only a step down, so no bridging there)
                    if (scaffoldOk != null && h == 0 && !stepDown
                        && (virtualStand || Ray.fullOutline(g.get(x, y - 1, z))) && g.isAir(nx, y - 1, nz) && scaffoldOk.ok(nx, y - 1, nz)) {
                        relax(i, nx, y, nz, c + C_BRIDGE + scaffoldPenalty, BRIDGE);
                    }
                }
            }
            if (scaffoldOk != null && h == 0 && headUp && g.isAir(x, y, z) && g.passable(x, y + 1, z) && scaffoldOk.ok(x, y, z)
                && (virtualStand || StateInfo.fullCube[g.get(x, y - 1, z)])) {
                relax(i, x, y + 1, z, c + C_PILLAR + scaffoldPenalty, PILLAR);
            }
            // take down the scaffold block underfoot and sink
            if (!virtualStand && h == 0 && g.in(x, y - 1, z) && scaffold.contains(g.idx(x, y - 1, z))) {
                for (int ty = y - 1; ty >= g.y0; ty--) {
                    if (ty < y - 1 && !g.passable(x, ty, z)) break;
                    // the cell we sink into: standable once the block is gone if there is support below it
                    int below = g.get(x, ty - 1, z);
                    if (StateInfo.colTop[below] >= 16) { relax(i, x, ty, z, c + C_DIG + (y - 1 - ty) / 2, DIG); break; }
                    if (StateInfo.colTop[below] != 0) break;
                }
            }
        }
        totalExpanded += expanded;
        return false;
    }

    private boolean isScaffold(int x, int y, int z) { return g.in(x, y, z) && scaffold.contains(g.idx(x, y, z)); }

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
        }
    }
}
