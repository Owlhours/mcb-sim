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
    /** True while the bot itself has its inventory screen open (to tell that from a screen opened by a stray click). */
    protected boolean inventoryOpenByMe;

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
        float hp = c.player.getHealth();
        if (hp < lastHealth && lastHealth >= 0) { hurts++; if (hurtAt == null) hurtAt = String.format("%.1f,%.1f,%.1f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz); }
        lastHealth = hp;
        if (c.player.isDeadOrDying()) { died = true; throw new OutOfTime(); }
        sync();
    }

    /** Damage taken (number of times), where first, and whether it ended the run. */
    public long hurts;
    public String hurtAt;
    public boolean died;
    private float lastHealth = -1;

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
    /** After a failed {@link #follow}: index of the path step that was not completed. */
    public int failedStep = -1;

    /** WALK_FAST (candidate "walkfast"; off, flips to System.getenv("MCB_NO_WALKFAST") == null on acceptance): see follow0. */
    static final boolean WALK_FAST = System.getenv("MCB_WALKFAST") != null;

    public boolean follow(int[] path, int from, int n) {
        boolean ok = follow0(path, from, n);
        return ok;
    }

    private boolean follow0(int[] path, int from, int n) {
        int i = from;
        failedStep = -1;
        int stuck = 0;
        double lastX = c.x(), lastZ = c.z(), lastY = c.y();
        int budget = 40 + 30 * (n - from);
        for (int k = Math.max(from, 1); k < n; k++) budget += 9 * Math.max(0, path[k * 3 - 2] - path[k * 3 + 1]); // falling takes time (more so past ladders or vines)
        c.holdAttack(false);
        while (i < n) {
            int wx = path[i * 3], wy = path[i * 3 + 1], wz = path[i * 3 + 2];
            boolean last = i == n - 1;
            double tx = wx + 0.5, tz = wz + 0.5;
            double dx = tx - c.x(), dz = tz - c.z();
            double dist = Math.sqrt(dx * dx + dz * dz);
            double feet = feetOf(wx, wy, wz);
            failedStep = i;
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
                    else if (dist < 0.45 && !onTower()) sneak = true; // fine approach (sneaking on scaffolding means sinking into it)
                }
            }
            // sprint along straight, level stretches (and stop sprinting well before the end or a turn)
            boolean sprint = false;
            if (WALK_FAST) {
                // WALK_FAST: sprint on any level run with two more steps beyond the next, turns and staircases too (not on the
                // last two steps); the off-line test stays
                int ddx = i + 1 < n ? path[(i + 1) * 3] - wx : 0, ddz = i + 1 < n ? path[(i + 1) * 3 + 2] - wz : 0;
                sprint = fwd && !sneak && !jump && c.onGround() && i + 2 < n
                        && path[(i + 1) * 3 + 1] == wy && feetOf(path[(i + 1) * 3], wy, path[(i + 1) * 3 + 2]) == feet
                        && path[(i + 2) * 3 + 1] == wy && feetOf(path[(i + 2) * 3], wy, path[(i + 2) * 3 + 2]) == feet
                        && !(Math.abs(dx) > 0.3 && ddx == 0 || Math.abs(dz) > 0.3 && ddz == 0);
            } else if (fwd && !sneak && !jump && c.onGround() && i + 3 < n) {
                sprint = true;
                int ddx = path[(i + 1) * 3] - wx, ddz = path[(i + 1) * 3 + 2] - wz;
                for (int k = i; k < i + 3 && sprint; k++) {
                    if (path[(k + 1) * 3 + 1] != wy || path[(k + 1) * 3] - path[k * 3] != ddx || path[(k + 1) * 3 + 2] - path[k * 3 + 2] != ddz) sprint = false;
                }
                // only when already heading that way (not while turning into the stretch)
                if (sprint && (Math.abs(dx) > 0.3 && ddx == 0 || Math.abs(dz) > 0.3 && ddz == 0)) sprint = false;
            }
            c.look(yawTo(dx, dz), c.pitch());
            c.keys(fwd, false, false, false, jump, sneak, sprint);
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

    /** True if what carries the bot is (or may be) a scaffolding block: sneaking would make it sink. */
    public boolean onTower() {
        int y = (int) Math.floor(c.y() - 0.02);
        double px = c.x(), pz = c.z();
        for (int k = 0; k < 4; k++) {
            int x = (int) Math.floor(px + ((k & 1) == 0 ? -0.3 : 0.3)), z = (int) Math.floor(pz + ((k & 2) == 0 ? -0.3 : 0.3));
            if (g.isTower(x, y, z)) return true;
        }
        return false;
    }

    /** Shuffles to the middle of column (x,z) so that the body does not overlap (and rest on) anything beside it. */
    public boolean middle(int x, int z) {
        for (int n = 0; n < 40; n++) {
            double dx = x + 0.5 - c.x(), dz = z + 0.5 - c.z();
            double dist = Math.sqrt(dx * dx + dz * dz);
            if (dist < 0.12 && hspeed() < 0.01) { c.releaseKeys(); return true; }
            c.look(yawTo(dx, dz), c.pitch());
            c.keys(dist > hspeed() * 2.2 + 0.05, false, false, false, false, false, false);
            tick();
        }
        c.releaseKeys();
        return Math.abs(x + 0.5 - c.x()) < 0.2 && Math.abs(z + 0.5 - c.z()) < 0.2;
    }

    /** Inside a scaffolding column at (x,z): rise (jump held) until the feet are at level toY, on top of the block below it. */
    public boolean climb(int x, int toY, int z) {
        c.holdAttack(false);
        int budget = 30 + 12 * Math.max(0, toY - cy());
        for (int n = 0; n < budget && c.y() < toY; n++) {
            double dx = x + 0.5 - c.x(), dz = z + 0.5 - c.z();
            boolean drift = dx * dx + dz * dz > 0.01;
            if (drift) c.look(yawTo(dx, dz), c.pitch());
            c.keys(drift, false, false, false, true, false, false);
            tick();
        }
        c.releaseKeys();
        for (int n = 0; n < 30 && !(c.onGround() && hspeed() < 0.003); n++) tick();
        return c.onGround() && cy() == toY && cx() == x && cz() == z;
    }

    /** On or inside a scaffolding column at (x,z): sink (sneak held) until the feet are at level toY. */
    public boolean descend(int x, int toY, int z) {
        c.holdAttack(false);
        int budget = 30 + 10 * Math.max(0, cy() - toY);
        c.keys(false, false, false, false, false, true, false);
        for (int n = 0; n < budget && c.y() > toY + 0.155; n++) tick();
        c.releaseKeys();
        for (int n = 0; n < 30 && !(c.onGround() && hspeed() < 0.003); n++) tick();
        return c.onGround() && cy() == toY && cx() == x && cz() == z;
    }

    /** Lets the body come to rest (on the ground, not sliding). */
    public void settle() {
        // (a bot sneaking at an edge keeps sneaking: letting go would drop it off)
        if (holdSneak) c.keys(false, false, false, false, false, true, false); else c.releaseKeys();
        for (int i = 0; i < 40 && !(c.onGround() && hspeed() < 0.003); i++) tick();
    }

    /** Set while the bot leans out over an edge. */
    protected boolean holdSneak;

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
    public boolean hold(Item item) { return hold(item, true); }

    /**
     * @param mayOpenInventory false while sneaking at an edge: opening the inventory screen lets go of every key,
     *                         sneak included, and the player would drop off
     */
    public boolean hold(Item item, boolean mayOpenInventory) {
        ItemStack cur = c.inventorySlot(c.selectedSlot());
        if (!cur.isEmpty() && cur.getItem() == item) {
            hotbarStamp[c.selectedSlot()] = ++hotbarUse;
            c.selectSlot(c.selectedSlot());   // cancels a slot change asked for earlier that has not taken effect yet
            return true;
        }
        for (int i = 0; i < 9; i++) {
            ItemStack s = c.inventorySlot(i);
            if (!s.isEmpty() && s.getItem() == item) { c.selectSlot(i); hotbarStamp[i] = ++hotbarUse; tickSelect(); return true; }
        }
        if (!mayOpenInventory) return false;
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
                inventoryOpenByMe = true;
                c.openInventory(); tick();
                c.clickSlot(i, best, ContainerInput.SWAP); tick();   // inventory screen: main slots keep their index 9..35
                c.closeScreen(); tick();
                inventoryOpenByMe = false;
                c.selectSlot(best); hotbarStamp[best] = ++hotbarUse; tickSelect();
                ItemStack now = c.inventorySlot(best);   // (the slot change itself takes effect with the next tick)
                return !now.isEmpty() && now.getItem() == item;
            }
        }
        return false;
    }

    /** The slot change takes effect at the start of the next client tick, before that tick's clicks; nothing to wait for. */
    private void tickSelect() { }

    public Item heldItem() { ItemStack s = c.inventorySlot(c.selectedSlot()); return s.isEmpty() ? null : s.getItem(); }

    // ------------------------------------------------------------------ clicking
    /** Result of {@link #findClick}: the neighbour block face to click, the point to aim at and the view angles. */
    public static final class Click {
        public int nx, ny, nz, face;
        public double ax, ay, az;
        public float yaw, pitch;
    }

    protected static final int[] DX = {0, 0, 0, 0, -1, 1}, DY = {-1, 1, 0, 0, 0, 0}, DZ = {0, 0, -1, 1, 0, 0};
    protected static final int[] OPP = {1, 0, 3, 2, 5, 4};
    // aim offsets from the face centre: {u, v}; for side faces v is the vertical one
    private static final double[][] FREE = {{0, 0}, {0.3, 0.3}, {-0.3, 0.3}, {0.3, -0.3}, {-0.3, -0.3}, {0.42, 0}, {-0.42, 0}, {0, 0.42}, {0, -0.42}};
    private static final double[][] QUAD = {{0.25, 0.25}, {-0.25, 0.25}, {0.25, -0.25}, {-0.25, -0.25}, {0.4, 0.4}, {-0.4, 0.4}, {0.4, -0.4}, {-0.4, -0.4}, {0.1, 0.25}, {-0.1, 0.25}, {0.1, -0.25}, {-0.1, -0.25}};
    public double reach = 4.5;
    public boolean debugClick;

    /**
     * Finds a way to put the wanted state into cell (x,y,z) by clicking a face of one of its six neighbours, as
     * seen from eye position (ex,ey,ez): the face must point at the cell, lie within reach, be the first thing the
     * line of sight meets, and - when a placement table is given - clicking it at that spot with that view
     * direction must produce the wanted state. faceMask restricts the neighbour directions (bit d).
     */
    public boolean findClick(int x, int y, int z, int want, char[] table, double ex, double ey, double ez, boolean sneak, int faceMask, Click out) {
        int wantCanon = StateInfo.loose[want];
        for (int d = 0; d < 6; d++) {
            if ((faceMask & (1 << d)) == 0) continue;
            int nx = x + DX[d], ny = y + DY[d], nz = z + DZ[d];
            int s = g.get(nx, ny, nz);
            if (debugClick) System.out.println("   click dbg: d=" + d + " neighbour " + StateInfo.name(s) + " replaceable=" + StateInfo.replaceable[s] + " interactive=" + StateInfo.interactive[s] + " boxes=" + Ray.outline(s).length);
            if (s == 0 || StateInfo.replaceable[s]) continue;
            if (StateInfo.interactive[s] && !sneak) continue;
            float[] boxes = Ray.outline(s);
            if (boxes.length == 0) continue;
            int face = OPP[d];
            double[][] offs = table == null ? FREE : QUAD;
            for (int b = 0; b < boxes.length; b += 6) {
                // the face of this box that looks towards the cell: plane coordinate and the rectangle (u, v) it spans
                double plane, u0, u1, v0, v1;
                switch (face) {
                    case 0: plane = ny + boxes[b + 1]; if (ey >= plane - 0.02) continue; u0 = boxes[b]; u1 = boxes[b + 3]; v0 = boxes[b + 2]; v1 = boxes[b + 5]; break;
                    case 1: plane = ny + boxes[b + 4]; if (ey <= plane + 0.02) continue; u0 = boxes[b]; u1 = boxes[b + 3]; v0 = boxes[b + 2]; v1 = boxes[b + 5]; break;
                    case 2: plane = nz + boxes[b + 2]; if (ez >= plane - 0.02) continue; u0 = boxes[b]; u1 = boxes[b + 3]; v0 = boxes[b + 1]; v1 = boxes[b + 4]; break;
                    case 3: plane = nz + boxes[b + 5]; if (ez <= plane + 0.02) continue; u0 = boxes[b]; u1 = boxes[b + 3]; v0 = boxes[b + 1]; v1 = boxes[b + 4]; break;
                    case 4: plane = nx + boxes[b]; if (ex >= plane - 0.02) continue; u0 = boxes[b + 2]; u1 = boxes[b + 5]; v0 = boxes[b + 1]; v1 = boxes[b + 4]; break;
                    default: plane = nx + boxes[b + 3]; if (ex <= plane + 0.02) continue; u0 = boxes[b + 2]; u1 = boxes[b + 5]; v0 = boxes[b + 1]; v1 = boxes[b + 4]; break;
                }
                if (u1 - u0 < 0.05 || v1 - v0 < 0.05) continue;
                for (double[] o : offs) {
                    // offsets are given for a unit face; keep the aim point inside this (possibly smaller) rectangle
                    double u = Math.max(u0 + 0.02, Math.min(u1 - 0.02, 0.5 + o[0])), v = Math.max(v0 + 0.02, Math.min(v1 - 0.02, 0.5 + o[1]));
                    double ax, ay, az;
                    if (face < 2) { ax = nx + u; az = nz + v; ay = plane; }
                    else if (face < 4) { ax = nx + u; ay = ny + v; az = plane; }
                    else { az = nz + u; ay = ny + v; ax = plane; }
                    int quad = (u > 0.5 ? 1 : 0) | (v > 0.5 ? 2 : 0);
                    double dx = ax - ex, dy = ay - ey, dz = az - ez;
                    double dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
                    if (dist > reach - 0.05) continue;
                    double hd = Math.sqrt(dx * dx + dz * dz);
                    float yaw = hd < 1e-6 ? 0f : yawTo(dx, dz);
                    float pitch = (float) -Math.toDegrees(Math.atan2(dy, hd));
                    if (table != null) {
                        int st = table[mcb.env.Oracle.index(face, quad, mcb.env.Oracle.yawClass(yaw), mcb.env.Oracle.pitchClass(pitch), sneak)];
                        if (debugClick) System.out.println("   click dbg: face " + face + " aim " + ax + "," + ay + "," + az + " yaw " + yaw + " pitch " + pitch + " table says " + StateInfo.name(st) + " want " + StateInfo.name(want));
                        if (st == 0 || StateInfo.loose[st] != wantCanon) continue;
                    }
                    if (debugClick) { boolean hitAny = ray.cast(g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, reach); System.out.println("   click dbg: ray " + hitAny + " hit " + ray.hx + "," + ray.hy + "," + ray.hz + " face " + ray.face + " wanted " + nx + "," + ny + "," + nz + " face " + face); }
                    if (ray.cast(g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, reach) && ray.hx == nx && ray.hy == ny && ray.hz == nz && ray.face == face) {
                        // the table is coarse (view direction in steps): ask for this very click
                        if (table != null && !exactClick(x, y, z, want, nx, ny, nz, face, ax, ay, az, yaw, pitch, sneak)) continue;
                        out.nx = nx; out.ny = ny; out.nz = nz; out.face = face; out.ax = ax; out.ay = ay; out.az = az; out.yaw = yaw; out.pitch = pitch;
                        return true;
                    }
                    if (u1 - u0 < 0.3 && v1 - v0 < 0.3) break; // tiny face: one aim point is enough
                }
            }
        }
        return StateInfo.scaffolding[want] && findScaffoldClick(x, y, z, want, ex, ey, ez, sneak, out);
    }

    /**
     * Scaffolding goes by rules of its own: used on the side of a scaffolding block it lands on top of that column,
     * used on the top of one it lands beside it, in the direction the player faces. So for a scaffolding block of
     * the build: the side of the one below it, or the top of one beside it seen from behind.
     */
    private boolean findScaffoldClick(int x, int y, int z, int want, double ex, double ey, double ez, boolean sneak, Click out) {
        if (sneak) return false;
        ray.scaffoldSolid = true;
        try {
            for (int k = 0; k < 8; k++) {
                int bx, by, bz, face;
                double ax, ay, az;
                if (k < 4) {
                    // a side of the block below
                    bx = x; by = y - 1; bz = z; face = 2 + k;
                    if (!g.isTower(bx, by, bz)) continue;
                    ax = bx + 0.5 + (face == 4 ? -0.5 : face == 5 ? 0.5 : 0); az = bz + 0.5 + (face == 2 ? -0.5 : face == 3 ? 0.5 : 0); ay = by + 0.5;
                } else {
                    // the top of a neighbour on the level
                    int d = k - 4;
                    bx = x + (d == 0 ? 1 : d == 1 ? -1 : 0); bz = z + (d == 2 ? 1 : d == 3 ? -1 : 0); by = y; face = 1;
                    if (!g.isTower(bx, by, bz)) continue;
                    ax = bx + 0.5; az = bz + 0.5; ay = by + 1;
                }
                double dx = ax - ex, dy = ay - ey, dz = az - ez;
                double dist = Math.sqrt(dx * dx + dy * dy + dz * dz), hd = Math.sqrt(dx * dx + dz * dz);
                if (dist > reach - 0.05 || dist < 1e-6) continue;
                float yaw = hd < 1e-6 ? 0f : yawTo(dx, dz), pitch = (float) -Math.toDegrees(Math.atan2(dy, hd));
                if (!ray.cast(g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, reach) || ray.hx != bx || ray.hy != by || ray.hz != bz || ray.face != face) continue;
                if (!exactClick(x, y, z, want, bx, by, bz, face, ax, ay, az, yaw, pitch, false)) continue;
                out.nx = bx; out.ny = by; out.nz = bz; out.face = face; out.ax = ax; out.ay = ay; out.az = az; out.yaw = yaw; out.pitch = pitch;
                return true;
            }
            return false;
        } finally {
            ray.scaffoldSolid = false;
        }
    }

    /** Would this exact click (aim point on a face of block n, view direction) put the wanted state into cell (x,y,z)? */
    protected boolean exactClick(int x, int y, int z, int want, int nx, int ny, int nz, int face, double ax, double ay, double az, float yaw, float pitch, boolean sneak) { return true; }

    /** True if the player's body currently overlaps cell (x,y,z). */
    public boolean bodyIn(int x, int y, int z) {
        double px = c.x(), py = c.y(), pz = c.z();
        return px + 0.3 > x && px - 0.3 < x + 1 && pz + 0.3 > z && pz - 0.3 < z + 1 && py + 1.8 > y && py < y + 1;
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
        if (!Ray.fullOutline(s)) {
            // a small or odd shape: aim at the middle of its boxes
            float[] b = Ray.outline(s);
            // (then at points just inside each of their faces: the middle may be hidden behind an edge)
            for (int pass = 0; pass < 7; pass++) {
                for (int k = 0; k < b.length; k += 6) {
                    double ax = x + (b[k] + b[k + 3]) / 2, ay = y + (b[k + 1] + b[k + 4]) / 2, az = z + (b[k + 2] + b[k + 5]) / 2;
                    switch (pass) {
                        case 1: ay = y + b[k + 4] - 0.01; break;
                        case 2: ay = y + b[k + 1] + 0.01; break;
                        case 3: ax = x + b[k] + 0.01; break;
                        case 4: ax = x + b[k + 3] - 0.01; break;
                        case 5: az = z + b[k + 2] + 0.01; break;
                        case 6: az = z + b[k + 5] - 0.01; break;
                        default: break;
                    }
                    double dx = ax - ex, dy = ay - ey, dz = az - ez;
                    double dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
                    if (dist > reach - 0.05 || dist < 1e-6) continue;
                    if (ray.cast(g, ex, ey, ez, ax, ay, az, reach) && ray.hx == x && ray.hy == y && ray.hz == z) {
                        out.nx = x; out.ny = y; out.nz = z; out.face = ray.face; out.ax = ax; out.ay = ay; out.az = az;
                        return true;
                    }
                }
            }
            return false;
        }
        for (int face = 0; face < 6; face++) {
            for (int o = 0; o < 5; o++) {
                double u = 0.5 + FREE[o][0], v = 0.5 + FREE[o][1];
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
