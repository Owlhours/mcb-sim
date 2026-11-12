package mcb.bot;

import it.unimi.dsi.fastutil.ints.IntOpenHashSet;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import mcb.build.Site;
import mcb.build.StateInfo;
import mcb.env.Client;
import net.minecraft.world.inventory.ContainerInput;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;

/**
 * The builder: works through the target layer by layer from the ground up, standing on the ground, on what it has
 * built or on temporary scaffolding, fetching materials from the supply chests as it runs out, and finally taking
 * its scaffolding away.
 */
public final class Builder extends Body {
    private final List<Site.Stock> stock;
    private final Item scaffoldItem;
    private final int scaffoldState;
    private final Nav nav;
    private final IntOpenHashSet scaffold = new IntOpenHashSet();
    private final java.util.BitSet skipped;
    private final Click click = new Click();
    /** Standing cells where the plan promised a placement that then did not work out (cleared whenever something is placed). */
    private final IntOpenHashSet badStand = new IntOpenHashSet();
    public long skippedCells, trips, scaffoldPlaced, replans;
    public String status = "";
    private int layerY;              // world y of the layer in progress
    private boolean starved;         // something could not be placed for lack of items

    public Builder(Client c, Grid g, Site site) {
        super(c, g);
        this.stock = site.stock;
        this.scaffoldItem = site.scaffoldItem;
        this.scaffoldState = site.scaffoldState;
        this.nav = new Nav(g);
        this.skipped = new java.util.BitSet(g.cur.length);
        // the supply chests are part of what the bot knows about the site
        for (Site.Stock s : stock) {
            int chest = net.minecraft.world.level.block.Block.getId(net.minecraft.world.level.block.Blocks.CHEST.defaultBlockState());
            g.set(s.chestX, s.chestY, s.chestZ, chest);
            g.set(s.chestX + 1, s.chestY, s.chestZ, chest);
        }
    }

    @Override protected void onBlockChange(int x, int y, int z, int state) {
        super.onBlockChange(x, y, z, state);
        if (g.in(x, y, z) && StateInfo.air[state]) scaffold.remove(g.idx(x, y, z));
    }

    // ------------------------------------------------------------------ what needs doing
    private boolean needs(int i) {
        int t = g.tgt[i];
        if (t == 0 || skipped.get(i)) return false;
        int w = g.cur[i];
        return w == 0 ? true : (StateInfo.canon[w] != StateInfo.canon[t] && scaffold.contains(i));
    }

    /** Blocks this version knows how to place: those whose state does not depend on how they are placed. */
    private static boolean simple(int state) { return StateInfo.state[state].getBlock().getStateDefinition().getPossibleStates().size() == 1 || StateInfo.canon[state] == StateInfo.canon[net.minecraft.world.level.block.Block.getId(StateInfo.state[state].getBlock().defaultBlockState())]; }

    private boolean have(int state) { Item it = StateInfo.item[state]; return count(it) > 0; }

    // ------------------------------------------------------------------ main loop
    public void run() {
        try {
            restock(g.oy);
            for (int ly = g.oy; ly < g.oy + g.sy; ly++) {
                layerY = ly;
                buildLayer();
            }
            status = "built";
            cleanup();
            status = "done";
        } catch (OutOfTime e) {
            status = "out of time";
        }
        c.releaseKeys();
        c.holdAttack(false);
    }

    private void buildLayer() {
        // give up on cells this version cannot do, so they do not stall the layer
        int base = g.idx(g.ox, layerY, g.oz);
        int remaining = 0;
        for (int z = 0; z < g.sz; z++) for (int x = 0; x < g.sx; x++) {
            int i = base + z * g.nx + x;
            int t = g.tgt[i];
            if (t == 0) continue;
            if (!simple(t) || StateInfo.itemCount[t] != 1) { if (!skipped.get(i)) { skipped.set(i); skippedCells++; } continue; }
            if (needs(i)) remaining++;
        }
        int idle = 0;
        while (remaining > 0) {
            starved = false;
            int done = placeFromHere();
            remaining -= done;
            if (done > 0) badStand.clear();
            if (remaining <= 0) break;
            if (done == 0 && selfCell()) { remaining--; continue; }
            // move to the nearest spot from which something can be placed
            int sx = cx(), sy = cy(), sz = cz();
            boolean found = nav.search(sx, sy, sz, this::canPlaceFrom, 4000, this::scaffoldOk);
            if (!found) {
                if (verbose) System.out.println("layer " + (layerY - g.oy) + ": no stand found; remaining=" + remaining + " starved=" + starved + " at " + sx + "," + sy + "," + sz + " expanded=" + nav.expanded + " inv=" + inventorySummary());
                if (starved) { restock(layerY); if (++idle > 3) { giveUpLayer(); return; } continue; }
                giveUpLayer();
                return;
            }
            if (nav.length <= 1 && done == 0) {
                // the planner thinks we can place from here but the real attempt failed: look for another spot
                badStand.add(g.idx(sx, sy, sz));
                if (++idle > 40) { skipNearest(); idle = 0; remaining = countRemaining(); badStand.clear(); }
                continue;
            }
            idle = 0;
            log("travel " + nav.length + " steps cost " + nav.goalCost + " to " + (nav.path[nav.length * 3 - 3] - g.ox) + "," + (nav.path[nav.length * 3 - 2] - g.oy) + "," + (nav.path[nav.length * 3 - 1] - g.oz));
            if (!travel()) { replans++; log("travel failed"); }
        }
    }

    private int countRemaining() {
        int base = g.idx(g.ox, layerY, g.oz), n = 0;
        for (int z = 0; z < g.sz; z++) for (int x = 0; x < g.sx; x++) if (needs(base + z * g.nx + x)) n++;
        return n;
    }

    private void giveUpLayer() {
        int base = g.idx(g.ox, layerY, g.oz);
        for (int z = 0; z < g.sz; z++) for (int x = 0; x < g.sx; x++) {
            int i = base + z * g.nx + x;
            if (needs(i)) { skipped.set(i); skippedCells++; }
        }
    }

    private void log(String m) { if (verbose) System.out.println("[t" + ticks + " @" + (cx() - g.ox) + "," + (cy() - g.oy) + "," + (cz() - g.oz) + "] " + m); }

    private void skipNearest() {
        int bx = cx(), bz = cz(), best = -1;
        long bd = Long.MAX_VALUE;
        int base = g.idx(g.ox, layerY, g.oz);
        for (int z = 0; z < g.sz; z++) for (int x = 0; x < g.sx; x++) {
            int i = base + z * g.nx + x;
            if (!needs(i)) continue;
            long d = (long) (g.ox + x - bx) * (g.ox + x - bx) + (long) (g.oz + z - bz) * (g.oz + z - bz);
            if (d < bd) { bd = d; best = i; }
        }
        if (best >= 0) { skipped.set(best); skippedCells++; log("skip cell " + (g.xOf(best) - g.ox) + "," + (g.yOf(best) - g.oy) + "," + (g.zOf(best) - g.oz) + " " + StateInfo.name(g.tgt[best])); }
    }

    private static final int R = 4;

    /** Nav goal: standing in this cell, is there a cell of the current layer that can be placed? */
    private boolean canPlaceFrom(int x, int y, int z) {
        if (y > layerY + 2 || y < layerY - 4) return false;
        int h = nav.virtualStand ? 0 : g.stand16(x, y, z);
        if (h < 0 || badStand.contains(g.idx(x, y, z))) return false;
        double ex = x + 0.5, ey = y + h / 16.0 + 1.62, ez = z + 0.5;
        reach = PLAN_REACH;
        try {
            return canPlaceFromEye(x, y, z, h, ex, ey, ez);
        } finally {
            reach = 4.5;
        }
    }

    /** When choosing where to stand, leave some slack: the bot will not be exactly at the cell centre. */
    private static final double PLAN_REACH = 4.1;

    private boolean canPlaceFromEye(int x, int y, int z, int h, double ex, double ey, double ez) {
        for (int dz = -R; dz <= R; dz++) for (int dx = -R; dx <= R; dx++) {
            int tx = x + dx, tz = z + dz;
            if (!g.inBox(tx, layerY, tz)) continue;
            int i = g.idx(tx, layerY, tz);
            if (!needs(i)) continue;
            if (tx == x && tz == z && (layerY == y || layerY == y + 1)) {
                if (layerY == y && h == 0 && have(g.tgt[i])) return true; // can be done by jumping and placing underfoot
                continue;
            }
            if (!have(g.tgt[i])) { starved = true; continue; }
            if (findClick(tx, layerY, tz, ex, ey, ez, 0x3f, 0, click)) return true;
        }
        return false;
    }

    /** Places every cell of the current layer that can be placed from where the bot stands. */
    private int placeFromHere() {
        settle();
        int done = 0;
        boolean again = true;
        while (again) {
            again = false;
            int bx = cx(), bz = cz();
            // far cells first, so that nearer ones do not hide them
            for (int ring = R; ring >= 0; ring--) {
                for (int dz = -ring; dz <= ring; dz++) for (int dx = -ring; dx <= ring; dx++) {
                    if (Math.max(Math.abs(dx), Math.abs(dz)) != ring) continue;
                    int tx = bx + dx, tz = bz + dz;
                    if (!g.inBox(tx, layerY, tz)) continue;
                    int i = g.idx(tx, layerY, tz);
                    if (!needs(i) || bodyIn(tx, layerY, tz)) continue;
                    int t = g.tgt[i];
                    if (!have(t)) { starved = true; continue; }
                    var e = c.eye();
                    if (!findClick(tx, layerY, tz, e.x, e.y, e.z, 0x3f, 0, click)) continue;
                    if (!hold(StateInfo.item[t])) { starved = true; continue; }
                    if (clickPlace(click, tx, layerY, tz)) { done++; again = true; }
                }
            }
        }
        return done;
    }

    /** If the bot is standing in a cell that needs a block, jump and place it underfoot. */
    private boolean selfCell() {
        int x = cx(), y = cy(), z = cz();
        log("selfCell layer " + (layerY - g.oy) + " stand=" + g.stand16(x, y, z));
        if (y != layerY || !g.inBox(x, y, z)) return false;
        int i = g.idx(x, y, z);
        if (!needs(i) || g.stand16(x, y, z) != 0 || !g.passable(x, y + 2, z)) return false;
        int t = g.tgt[i];
        if (!have(t)) { starved = true; return false; }
        return jumpPlace(StateInfo.item[t], x, y, z);
    }

    // ------------------------------------------------------------------ moving, with scaffolding
    private boolean scaffoldOk(int x, int y, int z) {
        return g.in(x, y, z) && g.target(x, y, z) == 0 && count(scaffoldItem) > 0;
    }

    /** Executes the route last found by nav. */
    private boolean travel() {
        int n = nav.length;
        int i = 1;
        while (i < n) {
            byte m = nav.move[i];
            if (m == Nav.PILLAR) {
                int x = nav.path[i * 3], y = nav.path[i * 3 + 1] - 1, z = nav.path[i * 3 + 2];
                if (!centre(x, y, z) || !jumpPlace(scaffoldItem, x, y, z)) return false;
                scaffold.add(g.idx(x, y, z)); scaffoldPlaced++;
                i++;
            } else if (m == Nav.BRIDGE) {
                int px = nav.path[(i - 1) * 3], py = nav.path[(i - 1) * 3 + 1], pz = nav.path[(i - 1) * 3 + 2];
                int x = nav.path[i * 3], z = nav.path[i * 3 + 2];
                if (!bridge(px, py, pz, x, z)) return false;
                i++;
            } else {
                int j = i;
                while (j < n && nav.move[j] != Nav.PILLAR && nav.move[j] != Nav.BRIDGE) j++;
                if (!follow(nav.path, i, j)) return false;
                i = j;
            }
        }
        return true;
    }

    private final int[] one = new int[3];

    private boolean centre(int x, int y, int z) {
        if (cx() == x && cz() == z && Math.abs(c.x() - x - 0.5) < 0.2 && Math.abs(c.z() - z - 0.5) < 0.2 && c.onGround()) return true;
        one[0] = x; one[1] = y; one[2] = z;
        return follow(one, 0, 1);
    }

    /** Standing in cell (x,y,z) on a full block: jump and place the item into that cell, ending on top of it. */
    private boolean jumpPlace(Item item, int x, int y, int z) {
        if (!hold(item)) return false;
        if (!centre(x, y, z)) return false;
        settle();
        double y0 = c.y();
        c.look(c.yaw(), 90);
        c.keys(false, false, false, false, true, false, false);
        boolean ok = false;
        for (int t = 0; t < 14; t++) {
            if (c.y() >= y0 + 1.0 && g.get(x, y, z) == 0) c.clickUse();
            tick();
            if (t == 0) c.jump(false);
            if (g.get(x, y, z) != 0) ok = true;
            if (ok && c.onGround()) break;
        }
        c.releaseKeys();
        if (ok) placed++;
        settle();
        return ok;
    }

    /** From cell (px,py,pz), standing on a full block, put scaffolding beside that block towards (x,z) and step onto it. */
    private boolean bridge(int px, int py, int pz, int x, int z) {
        if (!hold(scaffoldItem) || !centre(px, py, pz)) return false;
        int dx = x - px, dz = z - pz;
        // sneak to the edge: the game itself stops a sneaking player at the brink
        double lastX = Double.NaN, lastZ = Double.NaN;
        for (int t = 0; t < 40; t++) {
            c.look(yawTo(dx, dz), c.pitch());
            c.keys(true, false, false, false, false, true, false);
            tick();
            if (Math.abs(c.x() - lastX) + Math.abs(c.z() - lastZ) < 1e-4 && t > 3) break;
            lastX = c.x(); lastZ = c.z();
        }
        c.keys(false, false, false, false, false, true, false);
        tick();
        var e = c.eye();
        boolean ok = false;
        if (findClick(x, py - 1, z, e.x, e.y, e.z, 1 << (dx > 0 ? 4 : dx < 0 ? 5 : dz > 0 ? 2 : 3), 0, click)) {
            lookAt(click.ax, click.ay, click.az);
            c.clickUse();
            tick();
            ok = g.get(x, py - 1, z) != 0;
        }
        if (!ok) { c.releaseKeys(); tick(); return false; }
        placed++; scaffoldPlaced++;
        scaffold.add(g.idx(x, py - 1, z));
        one[0] = x; one[1] = py; one[2] = z;
        boolean r = follow(one, 0, 1);
        return r;
    }

    // ------------------------------------------------------------------ supplies
    /** Goes to the chests and loads up for the cells still to do, lowest layers first. */
    private void restock(int fromLayer) {
        trips++;
        // what will be needed, in build order, until the inventory would be full
        Map<Item, Integer> need = new LinkedHashMap<>();
        int slotsUsed = 0;
        int capacity = 33;
        int scaffoldWant = 128;
        outer:
        for (int ly = fromLayer; ly < g.oy + g.sy; ly++) {
            int base = g.idx(g.ox, ly, g.oz);
            for (int z = 0; z < g.sz; z++) for (int x = 0; x < g.sx; x++) {
                int i = base + z * g.nx + x;
                int t = g.tgt[i];
                if (t == 0 || skipped.get(i) || g.cur[i] != 0 || !simple(t) || StateInfo.itemCount[t] != 1) continue;
                Item it = StateInfo.item[t];
                int max = new ItemStack(it).getMaxStackSize();
                Integer cur = need.get(it);
                int now = cur == null ? 0 : cur;
                if (now % max == 0) { // would start a new stack
                    if (slotsUsed + 1 > capacity) break outer;
                    slotsUsed++;
                }
                need.put(it, now + 1);
            }
        }
        need.merge(scaffoldItem, scaffoldWant, Integer::sum);
        // what we already carry counts
        List<Site.Stock> todo = new ArrayList<>();
        Map<Item, Integer> missing = new LinkedHashMap<>();
        for (var e : need.entrySet()) { int m = e.getValue() - count(e.getKey()); if (m > 0) missing.put(e.getKey(), m); }
        if (missing.isEmpty()) return;
        for (Site.Stock s : stock) {
            if (s.count <= 0) continue;
            Integer m = missing.get(s.item);
            if (m == null || m <= 0) continue;
            todo.add(s);
            missing.put(s.item, m - s.count);
        }
        // visit chest by chest
        int k = 0;
        while (k < todo.size()) {
            Site.Stock first = todo.get(k);
            int ax = first.chestX, ay = g.oy, az = first.chestZ + 1;
            if (!(cx() == ax && cy() == ay && cz() == az)) {
                if (!nav.search(cx(), cy(), cz(), (x, y, z) -> x == ax && y == ay && z == az, 200000, this::scaffoldOk) || !travel()) {
                    // try once more from wherever we ended up
                    if (!nav.search(cx(), cy(), cz(), (x, y, z) -> x == ax && y == ay && z == az, 200000, null) || !travel()) { k++; continue; }
                }
            }
            settle();
            lookAt(first.chestX + 0.5, first.chestY + 0.45, first.chestZ + 0.7);
            c.clickUse();
            tick();
            if (!c.containerOpen()) { tick(); if (!c.containerOpen()) { k++; continue; } }
            while (k < todo.size() && todo.get(k).chestX == first.chestX && todo.get(k).chestY == first.chestY && todo.get(k).chestZ == first.chestZ) {
                Site.Stock s = todo.get(k++);
                if (freeSlots() == 0) break;
                c.clickSlot(s.slot, 0, ContainerInput.QUICK_MOVE);
                tick();
                s.count = c.menu().getSlot(s.slot).getItem().getCount();
            }
            c.closeScreen();
            tick();
        }
    }

    // ------------------------------------------------------------------ taking the scaffolding away
    private boolean canBreakFrom(int x, int y, int z) {
        int h = nav.virtualStand ? 0 : g.stand16(x, y, z);
        if (h < 0) return false;
        if (scaffold.contains(g.idx(x, y - 1, z)) && h == 0) return true;
        double ex = x + 0.5, ey = y + h / 16.0 + 1.62, ez = z + 0.5;
        for (int dy = -4; dy <= 5; dy++) for (int dz = -4; dz <= 4; dz++) for (int dx = -4; dx <= 4; dx++) {
            int tx = x + dx, ty = y + dy, tz = z + dz;
            if (!g.in(tx, ty, tz) || !scaffold.contains(g.idx(tx, ty, tz))) continue;
            if (findBreak(tx, ty, tz, ex, ey, ez, click)) return true;
        }
        return false;
    }

    private void cleanup() {
        int best = scaffold.size();
        long bestTick = ticks;
        int fails = 0;
        while (!scaffold.isEmpty()) {
            if (scaffold.size() < best) { best = scaffold.size(); bestTick = ticks; fails = 0; }
            if (ticks - bestTick > 40000 || fails > 30) break;
            settle();
            // break everything in reach, the block underfoot last
            int x = cx(), y = cy(), z = cz();
            boolean any = false;
            var e = c.eye();
            int under = g.in(x, y - 1, z) ? g.idx(x, y - 1, z) : -1;
            for (int dy = 5; dy >= -4 && !any; dy--) for (int dz = -4; dz <= 4 && !any; dz++) for (int dx = -4; dx <= 4 && !any; dx++) {
                int tx = x + dx, ty = y + dy, tz = z + dz;
                if (!g.in(tx, ty, tz)) continue;
                int i = g.idx(tx, ty, tz);
                if (i == under || !scaffold.contains(i)) continue;
                if (findBreak(tx, ty, tz, e.x, e.y, e.z, click)) {
                    if (breakBlock(tx, ty, tz, click.ax, click.ay, click.az, 60)) any = true; else fails++;
                }
            }
            if (any) continue;
            if (under >= 0 && scaffold.contains(under)) {
                if (!breakBlock(x, y - 1, z, x + 0.5, y, z + 0.5, 60)) fails++;
                settle();
                continue;
            }
            if (!nav.search(x, y, z, this::canBreakFrom, 20000, this::scaffoldOk)) { log("cleanup: no way to reach remaining scaffold " + scaffold.size()); break; }
            log("cleanup travel " + nav.length + " steps, scaffold left " + scaffold.size());
            if (nav.length <= 1) { fails++; continue; }
            if (!travel()) { replans++; fails++; }
        }
    }

    public int scaffoldLeft() { return scaffold.size(); }

    public String inventorySummary() {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 36; i++) { ItemStack st = c.inventorySlot(i); if (!st.isEmpty()) sb.append(i).append(':').append(st.getItem()).append('x').append(st.getCount()).append(' '); }
        return sb.toString();
    }
}
