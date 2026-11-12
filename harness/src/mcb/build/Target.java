package mcb.build;

import mcb.schem.Schem;

/**
 * The thing to build: a schematic turned into the set of cells that count, following GOAL.md:
 * fluids are not part of the target, nor are gravity blocks with nothing under them; waterlogging is dropped.
 * Coordinates are schematic-local; y = 0 is the layer resting on the ground.
 */
public final class Target {
    public final int sx, sy, sz;
    public final char[] ids;          // normalised target state per cell (0 = nothing to build)
    public long fluidCells, floatingGravityCells, technicalCells, blocks;

    public Target(Schem s) {
        StateInfo.init();
        sx = s.sx; sy = s.sy; sz = s.sz;
        ids = new char[s.ids.length];
        int layer = sx * sz;
        for (int i = 0; i < ids.length; i++) {
            int id = s.ids[i];
            if (id == 0 || StateInfo.air[id]) continue;
            if (StateInfo.fluid[id]) { fluidCells++; continue; }
            if (StateInfo.technical[id]) { technicalCells++; continue; }
            id = StateInfo.dewater[id];
            if (StateInfo.gravity[id] && i >= layer) {
                int below = ids[i - layer]; // already normalised (we go bottom-up)
                if (below == 0 || StateInfo.replaceable[below]) { floatingGravityCells++; continue; }
            }
            ids[i] = (char) id;
            blocks++;
        }
    }

    public int index(int x, int y, int z) { return (y * sz + z) * sx + x; }
    public int get(int x, int y, int z) {
        if (x < 0 || y < 0 || z < 0 || x >= sx || y >= sy || z >= sz) return 0;
        return ids[(y * sz + z) * sx + x];
    }
}
