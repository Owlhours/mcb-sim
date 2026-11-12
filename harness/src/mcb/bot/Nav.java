package mcb.bot;

import mcb.build.StateInfo;

/**
 * Route finding over the cells the player can stand in. Moves: walk to a neighbouring column (stepping up to
 * 0.6), jump up one block, drop any distance, and - when allowed - two moves that leave temporary scaffolding
 * behind: pillar (jump and put a block under the feet) and bridge (put a block beside the one stood on, then step
 * onto it).
 */
public final class Nav {
    public static final byte WALK = 0, JUMP = 1, DROP = 2, PILLAR = 3, BRIDGE = 4;
    public static final int C_WALK = 5, C_JUMP = 9, C_DROP = 6, C_PILLAR = 12, C_BRIDGE = 22;

    public interface Goal { boolean test(int x, int y, int z); }
    public interface CellRule { boolean ok(int x, int y, int z); }

    private final Grid g;
    private final int[] stamp, cost, parent;
    private final byte[] how;
    private int epoch;
    // binary heap of (cost, node)
    private long[] heap = new long[1024];
    private int heapSize;
    public int scaffoldPenalty = 40;
    public int expanded;
    /** While a goal test runs: the cell being tested will be stood in on top of scaffolding that is not there yet. */
    public boolean virtualStand;

    // result
    public int[] path = new int[96];   // x,y,z triples from start to goal (inclusive)
    public byte[] move = new byte[32]; // move[i] = how node i was entered
    public int length;
    public int goalCost;

    public Nav(Grid g) {
        this.g = g;
        int n = g.cur.length;
        stamp = new int[n]; cost = new int[n]; parent = new int[n]; how = new byte[n];
    }

    private void push(int c, int node) {
        if (heapSize == heap.length) heap = java.util.Arrays.copyOf(heap, heap.length * 2);
        long v = ((long) c << 32) | (node & 0xffffffffL);
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

    private void relax(int from, int x, int y, int z, int c, byte h) {
        if (!g.in(x, y, z)) return;
        int i = g.idx(x, y, z);
        if (stamp[i] == epoch && cost[i] <= c) return;
        stamp[i] = epoch; cost[i] = c; parent[i] = from; how[i] = h;
        push(c, i);
    }

    private static final int[] DX = {1, -1, 0, 0}, DZ = {0, 0, 1, -1};

    /**
     * Cheapest route from the start cell to the first cell accepted by goal. scaffoldOk says where temporary
     * blocks may go (null = no scaffolding moves). Returns false if nothing is found within maxCost.
     */
    public boolean search(int sx, int sy, int sz, Goal goal, int maxCost, CellRule scaffoldOk) {
        epoch++;
        heapSize = 0;
        expanded = 0;
        length = 0;
        if (!g.in(sx, sy, sz)) return false;
        int start = g.idx(sx, sy, sz);
        stamp[start] = epoch; cost[start] = 0; parent[start] = -1; how[start] = WALK;
        push(0, start);
        while (heapSize > 0) {
            long top = pop();
            int c = (int) (top >>> 32), i = (int) top;
            if (cost[i] != c) continue;
            int x = g.xOf(i), y = g.yOf(i), z = g.zOf(i);
            expanded++;
            // a cell entered by a scaffolding move will have a block under it by the time the bot is there
            virtualStand = how[i] == PILLAR || how[i] == BRIDGE;
            if (goal.test(x, y, z)) { build(i); goalCost = c; return true; }
            if (c > maxCost) return false;
            int h = virtualStand ? 0 : g.stand16(x, y, z);
            if (h < 0) {
                // not standing (start position in mid-air or on something odd): the only move is to drop straight down
                for (int ty = y - 1; ty >= g.y0; ty--) {
                    if (g.stand16(x, ty, z) >= 0) { relax(i, x, ty, z, c + C_DROP, DROP); break; }
                    if (!g.passable(x, ty, z)) break;
                }
                continue;
            }
            boolean headUp = g.passable(x, y + 2, z);
            for (int d = 0; d < 4; d++) {
                int nx = x + DX[d], nz = z + DZ[d];
                int hu = headUp ? g.stand16(nx, y + 1, nz) : -1;
                int hl = g.stand16(nx, y, nz);
                if (hl >= 0) {
                    int rise = hl - h;
                    if (rise <= 9) relax(i, nx, y, nz, c + C_WALK, WALK);
                    else if (rise <= 19 && headUp) relax(i, nx, y, nz, c + C_JUMP, JUMP);
                } else if (hu >= 0) {
                    int rise = 16 + hu - h;
                    if (rise <= 9) relax(i, nx, y + 1, nz, c + C_WALK, WALK);
                    else if (rise <= 19) relax(i, nx, y + 1, nz, c + C_JUMP, JUMP);
                } else if (g.passable(nx, y, nz) && g.passable(nx, y + 1, nz)) {
                    // step off the edge and fall to the first place one can stand
                    boolean found = false;
                    for (int ty = y - 1; ty >= g.y0; ty--) {
                        int hd = g.stand16(nx, ty, nz);
                        if (hd >= 0) {
                            int fall = (y - ty) * 16 + h - hd;
                            relax(i, nx, ty, nz, c + (fall <= 9 ? C_WALK : C_DROP + (y - ty) / 2), fall <= 9 ? WALK : DROP);
                            found = true;
                            break;
                        }
                        if (!g.passable(nx, ty, nz)) break;
                    }
                    if (!found && scaffoldOk != null && h == 0 && (virtualStand || Ray.fullOutline(g.get(x, y - 1, z))) && g.isAir(nx, y - 1, nz) && scaffoldOk.ok(nx, y - 1, nz)) {
                        relax(i, nx, y, nz, c + C_BRIDGE + scaffoldPenalty, BRIDGE);
                    }
                }
            }
            if (scaffoldOk != null && h == 0 && headUp && g.isAir(x, y, z) && g.passable(x, y + 1, z) && scaffoldOk.ok(x, y, z)
                && (virtualStand || StateInfo.fullCube[g.get(x, y - 1, z)])) {
                relax(i, x, y + 1, z, c + C_PILLAR + scaffoldPenalty, PILLAR);
            }
        }
        return false;
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
        }
    }
}
