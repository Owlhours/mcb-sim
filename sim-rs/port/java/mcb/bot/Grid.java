package mcb.bot;

import mcb.build.StateInfo;

/**
 * The bot's own picture of the build site: what it believes is in every cell (kept current from the block changes
 * its client is told about) and what the target wants there. World coordinates; arrays cover the target box plus a
 * margin for walking, the supply chests and scaffolding.
 */
public final class Grid {
    public final int x0, y0, z0, nx, ny, nz;       // array origin (world) and size
    public final int ox, oy, oz, sx, sy, sz;       // target box origin (world) and size
    public final char[] cur;                       // believed world state id
    public final char[] tgt;                       // target state id (0 = nothing)
    public final int groundState;

    public Grid(int ox, int oy, int oz, int sx, int sy, int sz, char[] target, int marginX, int marginNorth, int marginSouth, int marginTop, int groundState) {
        this.ox = ox; this.oy = oy; this.oz = oz; this.sx = sx; this.sy = sy; this.sz = sz;
        x0 = ox - marginX; z0 = oz - marginNorth; y0 = oy - 1;
        nx = sx + 2 * marginX; nz = sz + marginNorth + marginSouth; ny = sy + 1 + marginTop;
        long vol = (long) nx * ny * nz;
        if (vol > Integer.MAX_VALUE - 16) throw new IllegalArgumentException("site too large");
        cur = new char[(int) vol];
        tgt = new char[(int) vol];
        this.groundState = groundState;
        java.util.Arrays.fill(cur, 0, nx * nz, (char) groundState); // the layer under the build is the ground
        for (int y = 0; y < sy; y++) for (int z = 0; z < sz; z++) {
            System.arraycopy(target, (y * sz + z) * sx, tgt, idx(ox, oy + y, oz + z), sx);
        }
    }

    public int idx(int x, int y, int z) { return ((y - y0) * nz + (z - z0)) * nx + (x - x0); }
    public boolean in(int x, int y, int z) { return x >= x0 && x < x0 + nx && y >= y0 && y < y0 + ny && z >= z0 && z < z0 + nz; }
    public int xOf(int i) { return x0 + i % nx; }
    public int zOf(int i) { return z0 + (i / nx) % nz; }
    public int yOf(int i) { return y0 + i / (nx * nz); }

    /** Believed state at a world position; below the site it is ground, elsewhere outside it is air. */
    public int get(int x, int y, int z) {
        if (y < y0) return groundState;
        if (!in(x, y, z)) return 0;
        return cur[idx(x, y, z)];
    }

    public int target(int x, int y, int z) { return in(x, y, z) ? tgt[idx(x, y, z)] : 0; }

    public void set(int x, int y, int z, int state) { if (in(x, y, z)) cur[idx(x, y, z)] = (char) (StateInfo.air[state] ? 0 : state); }

    public boolean inBox(int x, int y, int z) { return x >= ox && x < ox + sx && y >= oy && y < oy + sy && z >= oz && z < oz + sz; }

    // ------------------------------------------------------------------ standing
    /**
     * Height of the player's feet above the bottom of cell (x,y,z) in 1/16 block when standing centred in that
     * column with the feet inside the cell, or -1 if one cannot stand there.
     */
    public int stand16(int x, int y, int z) {
        int a = get(x, y, z), b = get(x, y - 1, z);
        int h;
        int ta = StateInfo.colTop[a];
        if (StateInfo.walkThrough[a]) {
            int tb = StateInfo.colTop[b];
            if (tb < 16 || StateInfo.anyTop[b] != tb) return -1;   // nothing to stand on, or an uneven top (stairs ...)
            h = tb - 16;
        } else if (ta > 0 && ta < 16 && StateInfo.colBottom[a] == 0 && StateInfo.anyTop[a] == ta) {
            h = ta;                                                 // a slab, carpet, snow layer ... in the feet cell
        } else return -1;
        // head room: the body occupies [h, h + 28.8] sixteenths above the cell bottom; anything solid in those cells is in the way
        int top = h + 29;
        int s1 = get(x, y + 1, z);
        if (!StateInfo.walkThrough[s1] && 16 + StateInfo.anyBottom[s1] < top) return -1;
        if (top > 32) {
            int s2 = get(x, y + 2, z);
            if (!StateInfo.walkThrough[s2] && 32 + StateInfo.anyBottom[s2] < top) return -1;
        }
        return h;
    }

    public boolean passable(int x, int y, int z) { return StateInfo.walkThrough[get(x, y, z)]; }
    /**
     * Climbing: 0 = nothing to climb in this cell; 1 = a ladder; 2 = an open trapdoor that carries on the ladder under
     * it (same facing: the game lets one climb it); 3 = such a trapdoor, closed - to be opened by hand first.
     */
    public int rung(int x, int y, int z) {
        int s = get(x, y, z), k = StateInfo.rung[s];
        if (k <= 1) return k;
        int b = get(x, y - 1, z);
        return StateInfo.rung[b] == 1 && StateInfo.rungFacing[b] == StateInfo.rungFacing[s] ? k : 0;
    }

    /** Can the player be on the ladder in cell (x,y,z), feet at the bottom of the cell, centred: room for the head? */
    public boolean hang(int x, int y, int z) {
        if (rung(x, y, z) == 0) return false;
        int s1 = get(x, y + 1, z);
        return StateInfo.walkThrough[s1] || StateInfo.rung[s1] != 0;
    }

    public boolean isTower(int x, int y, int z) { return StateInfo.scaffolding[get(x, y, z)]; }
    public boolean solidCube(int x, int y, int z) { return StateInfo.fullCube[get(x, y, z)]; }
    public boolean isAir(int x, int y, int z) { return get(x, y, z) == 0; }
}
