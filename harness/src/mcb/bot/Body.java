package mcb.bot;

import mcb.build.StateInfo;
import mcb.env.Client;
import net.minecraft.world.inventory.ContainerInput;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.phys.Vec3;

/**
 * Low-level skills: keeping the grid in sync, walking a path cell by cell, jumping, dropping, aiming and clicking,
 * holding the right item. Everything goes through {@link Client}; every method that lets time pass does so by
 * calling {@link #tick()}.
 */
public class Body {
    public final Client c;
    public final Grid g;
    public final Ray ray = new Ray();
    public long ticks;
    public long maxTicks = Long.MAX_VALUE;
    public long placed, broken, failedClicks;
    public boolean verbose;

    public static final class OutOfTime extends RuntimeException { }

    public Body(Client c, Grid g) {
        this.c = c;
        this.g = g;
        Ray.init();
    }

    // ------------------------------------------------------------------ time & sync
    public void tick() {
        if (ticks >= maxTicks) throw new OutOfTime();
        c.tick();
        ticks++;
        sync();
    }

    protected void sync() {
        int n = c.changeCount();
        if (n == 0) return;
        int[] ch = c.changes();
        for (int i = 0; i < n; i++) {
            int k = i * 4;
            onBlockChange(ch[k], ch[k + 1], ch[k + 2], ch[k + 3]);
        }
        c.clearChanges();
    }

    protected void onBlockChange(int x, int y, int z, int state) { g.set(x, y, z, state); }

    public void idle(int n) { c.releaseKeys(); for (int i = 0; i < n; i++) tick(); }

    // ------------------------------------------------------------------ where am I
    public int cx() { return (int) Math.floor(c.x()); }
    public int cz() { return (int) Math.floor(c.z()); }
    public int cy() { return (int) Math.floor(c.y() + 1e-4); }
    public double hspeed() { Vec3 v = c.velocity(); return Math.sqrt(v.x * v.x + v.z * v.z); }
    public double feetOf(int x, int y, int z) { int h = g.stand16(x, y, z); return h < 0 ? Double.NaN : y + h / 16.0; }

    public static float yawTo(double dx, double dz) { return (float) Math.toDegrees(Math.atan2(-dx, dz)); }

    public void lookAt(double tx, double ty, double tz) {
        Vec3 e = c.eye();
        double dx = tx - e.x, dy = ty - e.y, dz = tz - e.z;
        double h = Math.sqrt(dx * dx + dz * dz);
        float yaw = h < 1e-6 ? c.yaw() : yawTo(dx, dz);
        c.look(yaw, (float) -Math.toDegrees(Math.atan2(dy, h)));
    }

    // ------------------------------------------------------------------ walking
    /**
     * Walks/jumps/drops through the given cells (x,y,z triples of feet cells; consecutive ones adjacent in the
     * plane). Returns false if the body ended up somewhere unexpected or got stuck (the caller replans).
     */
    public boolean follow(int[] path, int from, int n) {
        int i = from;
        int stuck = 0;
        double lastX = c.x(), lastZ = c.z(), lastY = c.y();
        int budget = 40 + 30 * (n - from);
        c.holdAttack(false);
        while (i < n) {
            int wx = path[i * 3], wy = path[i * 3 + 1], wz = path[i * 3 + 2];
            boolean last = i == n - 1;
            double tx = wx + 0.5, tz = wz + 0.5;
            double dx = tx - c.x(), dz = tz - c.z();
            double dist = Math.sqrt(dx * dx + dz * dz);
            double feet = feetOf(wx, wy, wz);
            if (Double.isNaN(feet)) { c.releaseKeys(); return false; }
            boolean atHeight = c.onGround() && Math.abs(c.y() - feet) < 0.02;
            if (atHeight && cx() == wx && cz() == wz && (last ? dist < 0.18 && hspeed() < 0.03 : dist < 0.38)) { i++; continue; }
            if (--budget < 0) { c.releaseKeys(); return false; }
            boolean fwd = true, jump = false, sneak = false;
            double rise = feet - c.y();
            if (c.onGround()) {
                if (rise > 0.6) {
                    if (dist < 1.25) jump = true;
                } else if (last && rise > -0.6) {
                    double coast = hspeed() * 2.2;
                    if (dist <= coast + 0.02) fwd = false;
                    else if (dist < 0.45) sneak = true; // fine approach
                }
            }
            c.look(yawTo(dx, dz), c.pitch());
            c.keys(fwd, false, false, false, jump, sneak, false);
            tick();
            double moved = Math.abs(c.x() - lastX) + Math.abs(c.z() - lastZ) + Math.abs(c.y() - lastY);
            if (moved < 1e-4) { if (++stuck > 25) { c.releaseKeys(); return false; } } else stuck = 0;
            lastX = c.x(); lastZ = c.z(); lastY = c.y();
            // fell somewhere we did not mean to go?
            if (c.onGround() && c.y() < feet - 0.6 && c.y() < Math.min(feet, feetOf(path[Math.max(0, i - 1) * 3], path[Math.max(0, i - 1) * 3 + 1], path[Math.max(0, i - 1) * 3 + 2])) - 0.6
                && !(cx() == wx && cz() == wz)) { c.releaseKeys(); return false; }
        }
        c.releaseKeys();
        return true;
    }

    /** Lets the body come to rest (on the ground, not sliding). */
    public void settle() {
        c.releaseKeys();
        for (int i = 0; i < 40 && !(c.onGround() && hspeed() < 0.003); i++) tick();
    }

    // ------------------------------------------------------------------ items
    public int count(Item item) {
        int n = 0;
        for (int i = 0; i < 36; i++) { ItemStack s = c.inventorySlot(i); if (!s.isEmpty() && s.getItem() == item) n += s.getCount(); }
        return n;
    }

    public int freeSlots() {
        int n = 0;
        for (int i = 0; i < 36; i++) if (c.inventorySlot(i).isEmpty()) n++;
        return n;
    }

    private int hotbarUse = 0;
    private final long[] hotbarStamp = new long[9];

    /** Makes the main hand hold the item (number key, or a swap in the inventory screen). False if the bot has none. */
    public boolean hold(Item item) {
        ItemStack cur = c.inventorySlot(c.selectedSlot());
        if (!cur.isEmpty() && cur.getItem() == item) { hotbarStamp[c.selectedSlot()] = ++hotbarUse; return true; }
        for (int i = 0; i < 9; i++) {
            ItemStack s = c.inventorySlot(i);
            if (!s.isEmpty() && s.getItem() == item) { c.selectSlot(i); hotbarStamp[i] = ++hotbarUse; tickSelect(); return true; }
        }
        for (int i = 9; i < 36; i++) {
            ItemStack s = c.inventorySlot(i);
            if (!s.isEmpty() && s.getItem() == item) {
                // least recently used hotbar slot (prefer an empty one)
                int best = 0;
                long bestStamp = Long.MAX_VALUE;
                for (int h = 0; h < 9; h++) {
                    long st = c.inventorySlot(h).isEmpty() ? -1 : hotbarStamp[h];
                    if (st < bestStamp) { bestStamp = st; best = h; }
                }
                c.releaseKeys();
                c.openInventory(); tick();
                c.clickSlot(i, best, ContainerInput.SWAP); tick();   // inventory screen: main slots keep their index 9..35
                c.closeScreen(); tick();
                c.selectSlot(best); hotbarStamp[best] = ++hotbarUse; tickSelect();
                ItemStack now = c.inventorySlot(c.selectedSlot());
                return !now.isEmpty() && now.getItem() == item;
            }
        }
        return false;
    }

    /** The slot change takes effect at the start of the next client tick, before that tick's clicks; nothing to wait for. */
    private void tickSelect() { }

    public Item heldItem() { ItemStack s = c.inventorySlot(c.selectedSlot()); return s.isEmpty() ? null : s.getItem(); }

    // ------------------------------------------------------------------ clicking
    /** Result of {@link #findClick}: the neighbour block face to click and the point to aim at. */
    public static final class Click {
        public int nx, ny, nz, face;
        public double ax, ay, az;
    }

    private static final int[] DX = {0, 0, 0, 0, -1, 1}, DY = {-1, 1, 0, 0, 0, 0}, DZ = {0, 0, -1, 1, 0, 0};
    private static final int[] OPP = {1, 0, 3, 2, 5, 4};
    private static final double[][] OFFS = {{0, 0}, {0.3, 0.3}, {-0.3, 0.3}, {0.3, -0.3}, {-0.3, -0.3}, {0.42, 0}, {-0.42, 0}, {0, 0.42}, {0, -0.42}};
    public double reach = 4.5;

    /**
     * Finds a way to put a block into cell (x,y,z) by clicking a face of one of its six neighbours, as seen from
     * eye position (ex,ey,ez): the face must point at the cell, lie within reach and be the first thing the line of
     * sight meets. faceMask restricts which neighbour directions may be used (bit d = neighbour in direction d),
     * vHalf restricts the height of the aim point on side faces (0 any, 1 lower half, 2 upper half).
     */
    public boolean findClick(int x, int y, int z, double ex, double ey, double ez, int faceMask, int vHalf, Click out) {
        for (int d = 0; d < 6; d++) {
            if ((faceMask & (1 << d)) == 0) continue;
            int nx = x + DX[d], ny = y + DY[d], nz = z + DZ[d];
            int s = g.get(nx, ny, nz);
            if (s == 0 || !Ray.fullOutline(s) || StateInfo.replaceable[s]) continue;
            if (StateInfo.interactive[s] && !c.sneaking()) continue;
            int face = OPP[d];
            // the eye must be in front of the face
            double plane;
            switch (face) {
                case 0: plane = ny; if (ey >= plane - 0.02) continue; break;
                case 1: plane = ny + 1; if (ey <= plane + 0.02) continue; break;
                case 2: plane = nz; if (ez >= plane - 0.02) continue; break;
                case 3: plane = nz + 1; if (ez <= plane + 0.02) continue; break;
                case 4: plane = nx; if (ex >= plane - 0.02) continue; break;
                default: plane = nx + 1; if (ex <= plane + 0.02) continue; break;
            }
            for (double[] o : OFFS) {
                double ax, ay, az;
                if (face < 2) { ax = nx + 0.5 + o[0]; az = nz + 0.5 + o[1]; ay = plane; }
                else {
                    double v = 0.5 + o[1];
                    if (vHalf == 1 && v > 0.45) v = 0.25 + o[1] * 0.4; else if (vHalf == 2 && v < 0.55) v = 0.75 + o[1] * 0.4;
                    ay = ny + v;
                    if (face < 4) { ax = nx + 0.5 + o[0]; az = plane; } else { az = nz + 0.5 + o[0]; ax = plane; }
                }
                double dx = ax - ex, dy = ay - ey, dz = az - ez;
                double dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
                if (dist > reach - 0.05) continue;
                // aim a hair past the face so the ray is sure to arrive
                if (ray.cast(g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, reach) && ray.hx == nx && ray.hy == ny && ray.hz == nz && ray.face == face) {
                    out.nx = nx; out.ny = ny; out.nz = nz; out.face = face; out.ax = ax; out.ay = ay; out.az = az;
                    return true;
                }
            }
        }
        return false;
    }

    /** True if the player's body currently overlaps cell (x,y,z). */
    public boolean bodyIn(int x, int y, int z) {
        double px = c.x(), py = c.y(), pz = c.z();
        return px + 0.3 > x && px - 0.3 < x + 1 && pz + 0.3 > z && pz - 0.3 < z + 1 && py + 1.8 > y && py < y + 1;
    }

    /** Aims at the click point and presses use once. Returns true if the cell now holds something. */
    public boolean clickPlace(Click k, int x, int y, int z) {
        lookAt(k.ax, k.ay, k.az);
        c.clickUse();
        tick();
        if (g.get(x, y, z) != 0) { placed++; return true; }
        failedClicks++;
        return false;
    }

    /** Breaks the block at (x,y,z) by aiming at the given point on it and holding attack. */
    public boolean breakBlock(int x, int y, int z, double ax, double ay, double az, int maxTicks) {
        int before = g.get(x, y, z);
        if (before == 0) return true;
        c.clickAttack();
        c.holdAttack(true);
        for (int t = 0; t < maxTicks; t++) {
            lookAt(ax, ay, az);
            tick();
            if (g.get(x, y, z) == 0) { c.holdAttack(false); broken++; return true; }
            var hit = c.lastHit();
            if (t > 2 && (hit == null || hit.getBlockPos().getX() != x || hit.getBlockPos().getY() != y || hit.getBlockPos().getZ() != z)) break;
        }
        c.holdAttack(false);
        tick();
        return g.get(x, y, z) == 0;
    }

    /** A point on a visible face of block (x,y,z) from the eye, or false. Used for breaking. */
    public boolean findBreak(int x, int y, int z, double ex, double ey, double ez, Click out) {
        int s = g.get(x, y, z);
        if (s == 0) return false;
        for (int face = 0; face < 6; face++) {
            for (int o = 0; o < 5; o++) {
                double u = 0.5 + OFFS[o][0], v = 0.5 + OFFS[o][1];
                double ax, ay, az;
                switch (face) {
                    case 0: ax = x + u; ay = y; az = z + v; if (ey >= ay) continue; break;
                    case 1: ax = x + u; ay = y + 1; az = z + v; if (ey <= ay) continue; break;
                    case 2: ax = x + u; ay = y + v; az = z; if (ez >= az) continue; break;
                    case 3: ax = x + u; ay = y + v; az = z + 1; if (ez <= az) continue; break;
                    case 4: ax = x; ay = y + v; az = z + u; if (ex >= ax) continue; break;
                    default: ax = x + 1; ay = y + v; az = z + u; if (ex <= ax) continue; break;
                }
                double dx = ax - ex, dy = ay - ey, dz = az - ez;
                double dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
                if (dist > reach - 0.05) continue;
                if (ray.cast(g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, reach) && ray.hx == x && ray.hy == y && ray.hz == z) {
                    out.nx = x; out.ny = y; out.nz = z; out.face = ray.face; out.ax = ax; out.ay = ay; out.az = az;
                    return true;
                }
            }
        }
        return false;
    }
}
