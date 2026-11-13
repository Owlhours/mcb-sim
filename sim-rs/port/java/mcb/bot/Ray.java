package mcb.bot;

import mcb.build.StateInfo;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.phys.AABB;

/**
 * Line of sight on the bot's picture of the world: which block face a ray from the eye meets first. Mirrors the
 * game's own pick (a walk through the cells along the ray, testing each block's outline shape); used for planning,
 * the real pick is what counts when the bot actually clicks.
 */
public final class Ray {
    private static float[][] boxes;      // per state: outline boxes, 6 floats each (empty array = nothing to hit)
    private static boolean[] full;

    public static void init() {
        if (boxes != null) return;
        StateInfo.init();
        int n = StateInfo.N;
        boxes = new float[n][];
        full = new boolean[n];
        for (int i = 0; i < n; i++) {
            var shape = StateInfo.state[i].getShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO);
            var list = shape.toAabbs();
            float[] b = new float[list.size() * 6];
            int k = 0;
            for (AABB a : list) {
                b[k++] = (float) a.minX; b[k++] = (float) a.minY; b[k++] = (float) a.minZ; b[k++] = (float) a.maxX; b[k++] = (float) a.maxY; b[k++] = (float) a.maxZ;
            }
            boxes[i] = b;
            full[i] = list.size() == 1 && b[0] == 0 && b[1] == 0 && b[2] == 0 && b[3] == 1 && b[4] == 1 && b[5] == 1;
        }
    }

    public static boolean fullOutline(int state) { return full[state]; }
    public static boolean hasOutline(int state) { return boxes[state].length > 0; }
    public static float[] outline(int state) { return boxes[state]; }

    /** While set, scaffolding blocks count as whole cubes: that is their outline for a player holding scaffolding. */
    public boolean scaffoldSolid;

    // result of the last cast
    public int hx, hy, hz, face; // face: 0 down(-y) 1 up(+y) 2 north(-z) 3 south(+z) 4 west(-x) 5 east(+x)  (Direction.get3DDataValue order)
    public double px, py, pz, dist;

    /** Casts from (ex,ey,ez) towards (tx,ty,tz), up to maxDist. Returns true if a block outline is hit. */
    public boolean cast(Grid g, double ex, double ey, double ez, double tx, double ty, double tz, double maxDist) {
        double dx = tx - ex, dy = ty - ey, dz = tz - ez;
        double len = Math.sqrt(dx * dx + dy * dy + dz * dz);
        if (len < 1e-9) return false;
        dx /= len; dy /= len; dz /= len;
        int x = (int) Math.floor(ex), y = (int) Math.floor(ey), z = (int) Math.floor(ez);
        int stepX = dx > 0 ? 1 : -1, stepY = dy > 0 ? 1 : -1, stepZ = dz > 0 ? 1 : -1;
        double tDX = dx == 0 ? Double.MAX_VALUE : Math.abs(1 / dx), tDY = dy == 0 ? Double.MAX_VALUE : Math.abs(1 / dy), tDZ = dz == 0 ? Double.MAX_VALUE : Math.abs(1 / dz);
        double tMX = dx == 0 ? Double.MAX_VALUE : ((dx > 0 ? x + 1 - ex : ex - x) * tDX);
        double tMY = dy == 0 ? Double.MAX_VALUE : ((dy > 0 ? y + 1 - ey : ey - y) * tDY);
        double tMZ = dz == 0 ? Double.MAX_VALUE : ((dz > 0 ? z + 1 - ez : ez - z) * tDZ);
        double t = 0;
        int enterFace = -1;
        while (t <= maxDist) {
            int s = g.get(x, y, z);
            if (s != 0) {
                float[] b = boxes[s];
                if (b.length > 0) {
                    if ((full[s] || (scaffoldSolid && StateInfo.scaffolding[s])) && enterFace >= 0) {
                        hx = x; hy = y; hz = z; face = enterFace; dist = t;
                        px = ex + dx * t; py = ey + dy * t; pz = ez + dz * t;
                        return true;
                    }
                    double best = Double.MAX_VALUE;
                    int bestFace = -1;
                    for (int k = 0; k < b.length; k += 6) {
                        // slab test against box k, in world coordinates
                        double x1 = x + b[k], y1 = y + b[k + 1], z1 = z + b[k + 2], x2 = x + b[k + 3], y2 = y + b[k + 4], z2 = z + b[k + 5];
                        double tn = -Double.MAX_VALUE, tf = Double.MAX_VALUE;
                        int f = -1;
                        if (dx != 0) {
                            double a = (x1 - ex) / dx, c = (x2 - ex) / dx;
                            double lo = Math.min(a, c), hi = Math.max(a, c);
                            if (lo > tn) { tn = lo; f = dx > 0 ? 4 : 5; }
                            if (hi < tf) tf = hi;
                        } else if (ex <= x1 || ex >= x2) continue;
                        if (dy != 0) {
                            double a = (y1 - ey) / dy, c = (y2 - ey) / dy;
                            double lo = Math.min(a, c), hi = Math.max(a, c);
                            if (lo > tn) { tn = lo; f = dy > 0 ? 0 : 1; }
                            if (hi < tf) tf = hi;
                        } else if (ey <= y1 || ey >= y2) continue;
                        if (dz != 0) {
                            double a = (z1 - ez) / dz, c = (z2 - ez) / dz;
                            double lo = Math.min(a, c), hi = Math.max(a, c);
                            if (lo > tn) { tn = lo; f = dz > 0 ? 2 : 3; }
                            if (hi < tf) tf = hi;
                        } else if (ez <= z1 || ez >= z2) continue;
                        if (tn <= tf && tn >= 0 && tn < best) { best = tn; bestFace = f; }
                    }
                    if (bestFace >= 0 && best <= maxDist) {
                        hx = x; hy = y; hz = z; face = bestFace; dist = best;
                        px = ex + dx * best; py = ey + dy * best; pz = ez + dz * best;
                        return true;
                    }
                }
            }
            if (tMX < tMY && tMX < tMZ) { x += stepX; t = tMX; tMX += tDX; enterFace = dx > 0 ? 4 : 5; }
            else if (tMY < tMZ) { y += stepY; t = tMY; tMY += tDY; enterFace = dy > 0 ? 0 : 1; }
            else { z += stepZ; t = tMZ; tMZ += tDZ; enterFace = dz > 0 ? 2 : 3; }
        }
        return false;
    }
}
