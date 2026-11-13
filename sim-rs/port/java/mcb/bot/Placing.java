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
 * Putting a block into a cell from where the bot stands: choosing the click, checking it against the game's own
 * rules, further clicks (second slab half, opening a trapdoor, filling a pot), helper blocks for cells with
 * nothing to click on.
 */
abstract class Placing extends Moves {
    Placing(Client c, Grid g, Site site, Oracle oracle) { super(c, g, site, oracle); }

    /** Does clicking the given face of a neighbour ever give the wanted state? */
    boolean faceWorks(Spec sp, int face) {
        if (sp.table == null) return true;
        int want = StateInfo.loose[sp.first];
        int from = Oracle.index(face, 0, 0, 0, false), to = Oracle.index(face, 3, Oracle.NYAW - 1, Oracle.NPITCH - 1, true);
        for (int k = from; k <= to; k++) { int e = sp.table[k]; if (e != 0 && StateInfo.loose[e] == want) return true; }
        return false;
    }

    /**
     * Puts a scaffold block into an empty neighbouring cell of (x,y,z) whose face would let the wanted block be
     * placed. Returns that cell's index, or -1. Blocks that must rest on something real are not given a helper.
     */
    int placeHelper(int x, int y, int z, Spec sp) {
        if (!haveScaffold || count(scaffoldItem) == 0 || !oracle.floats(sp.first)) return -1;
        Spec ss = spec(scaffoldState);
        helperBase = -1;
        boolean direct = false;
        for (int d = 0; d < 6; d++) {
            if ((helperSkip & (1 << d)) != 0) continue;   // (that side has had its helper: see doCell)
            int nx = x + DX[d], ny = y + DY[d], nz = z + DZ[d];
            if (!g.in(nx, ny, nz) || ny < g.oy || g.get(nx, ny, nz) != 0 || !scaffoldOk(nx, ny, nz)) continue;
            if (!faceWorks(sp, OPP[d])) continue;
            // the helper itself needs something to be placed against
            boolean anchored = false;
            for (int e = 0; e < 6 && !anchored; e++) {
                int ax = nx + DX[e], ay = ny + DY[e], az = nz + DZ[e];
                if (ax == x && ay == y && az == z) continue;
                int w = g.get(ax, ay, az);
                anchored = w != 0 && !StateInfo.replaceable[w] && Ray.hasOutline(w);
            }
            if (!anchored) continue;
            direct = true;
            if (attempt(nx, ny, nz, scaffoldState, ss, false) == PLACED) {
                addScaffold(nx, ny, nz);
                helperDir = d;
                return g.idx(nx, ny, nz);
            }
        }
        // (Only once everything else stands: before that, most such cells get a neighbour in time, and trying this
        // for each of them changed the course of three builds in four.)
        helperDir = -1;
        if (direct || !(mainDone || helperChainNow) || count(scaffoldItem) < 2 || (!mainDone && hasPistons())) return -1;   // (in machines only once everything else stands: before that a block too many sets them off)
        // Nothing beside the cell that a helper could be put against (a log that must stand upright, out on a
        // branch with nothing under or over it): a second helper, one step further, to carry the first.
        int tries = 0;
        for (int d = 0; d < 6 && tries < 3; d++) {
            int nx = x + DX[d], ny = y + DY[d], nz = z + DZ[d];
            if (!g.in(nx, ny, nz) || ny < g.oy || g.get(nx, ny, nz) != 0 || !scaffoldOk(nx, ny, nz) || !faceWorks(sp, OPP[d])) continue;
            for (int e = 0; e < 6 && tries < 3; e++) {
                int bx = nx + DX[e], by = ny + DY[e], bz = nz + DZ[e];
                if ((bx == x && by == y && bz == z) || !g.in(bx, by, bz) || by < g.oy || g.get(bx, by, bz) != 0 || !scaffoldOk(bx, by, bz)) continue;
                boolean anchored = false;
                for (int f = 0; f < 6 && !anchored; f++) {
                    int ax = bx + DX[f], ay = by + DY[f], az = bz + DZ[f];
                    if ((ax == x && ay == y && az == z) || (ax == nx && ay == ny && az == nz)) continue;
                    int w = g.get(ax, ay, az);
                    anchored = w != 0 && !StateInfo.replaceable[w] && Ray.hasOutline(w);
                }
                if (!anchored) continue;
                tries++;
                if (attempt(bx, by, bz, scaffoldState, ss, false) != PLACED) continue;
                addScaffold(bx, by, bz);
                if (attempt(nx, ny, nz, scaffoldState, ss, false) == PLACED) {
                    addScaffold(nx, ny, nz);
                    helperBase = g.idx(bx, by, bz);
                    return g.idx(nx, ny, nz);
                }
                mustRemove = true;
                try { removeScaffold(g.idx(bx, by, bz)); } finally { mustRemove = false; }
            }
        }
        return -1;
    }

    @Override protected boolean exactClick(int x, int y, int z, int want, int nx, int ny, int nz, int face, double ax, double ay, double az, float yaw, float pitch, boolean sneak) {
        Item it = StateInfo.item[want];
        if (it == Items.AIR) return true;
        // (a block that is only imagined there - a column still to be put up - cannot be asked about: the table has to do)
        int believed = g.get(nx, ny, nz);
        if (believed == 0 || c.block(new net.minecraft.core.BlockPos(nx, ny, nz)) != StateInfo.state[believed]) return true;
        BlockHitResult hit = new BlockHitResult(new net.minecraft.world.phys.Vec3(ax, ay, az), net.minecraft.core.Direction.from3DDataValue(face), new net.minecraft.core.BlockPos(nx, ny, nz), false);
        int pred = oracle.predict(it, hit, yaw, pitch, sneak);
        return pred != 0 && StateInfo.loose[pred] == StateInfo.loose[want] && oracle.placedAt.getX() == x && oracle.placedAt.getY() == y && oracle.placedAt.getZ() == z;
    }

    boolean placeFromHere(int x, int y, int z, int t, Spec sp) {
        if (sealsIn(x, y, z, sp.first)) { sealRefused = true; if (watched(x, y, z)) log("WATCH placeFromHere: would shut the bot in"); return false; }
        if (rescueTowers(x, y, z, sp.first)) { if (watched(x, y, z)) log("WATCH placeFromHere: columns rescued first"); return false; }
        if (watched(x, y, z)) log("WATCH placeFromHere from " + rel(cx(), cy(), cz()) + " sneaking=" + c.sneaking() + " leaning=" + leaning);
        if (!c.onGround() || hspeed() > 0.003) settle();
        // get the item into the hand first: fetching it from the inventory screen lets go of all keys
        if (!hold(sp.item, !leaning && !c.sneaking())) return false;
        if (tryPlace(x, y, z, t, sp)) return true;
        if (leaning || c.sneaking()) return false;
        // against blocks that react to a click (chests, doors ...) one has to sneak; that also lowers the eyes
        boolean wanted = false;
        for (int d = 0; d < 6 && !wanted; d++) wanted = StateInfo.interactive[g.get(x + DX[d], y + DY[d], z + DZ[d])];
        if (!wanted || onTower()) return false;   // (sneaking on scaffolding means sinking through it)
        c.keys(false, false, false, false, false, true, false);
        tick(); tick();
        boolean ok = tryPlace(x, y, z, t, sp);
        c.releaseKeys();
        tick();
        return ok;
    }

    boolean tryPlace(int x, int y, int z, int t, Spec sp) {
        var e = c.eye();
        boolean sneak = c.sneaking();
        boolean needsSupport = !oracle.floats(sp.first);
        for (int d = 0; d < 6; d++) {
            // something that has to sit on or hang from a block must not be put on scaffolding: it would fall off with it
            if (needsSupport && g.in(x + DX[d], y + DY[d], z + DZ[d]) && scaffold.contains(g.idx(x + DX[d], y + DY[d], z + DZ[d]))) continue;
            if (!findClick(x, y, z, sp.first, sp.table, e.x, e.y, e.z, sneak, 1 << d, click)) continue;
            if (!hold(sp.item, !leaning && !sneak)) return false;
            // what would really happen? (the bot's picture can be off for odd shapes; the rules decide)
            BlockHitResult hit = oracle.pick(c, click.yaw, click.pitch);
            if (hit == null || hit.getBlockPos().getX() != click.nx || hit.getBlockPos().getY() != click.ny || hit.getBlockPos().getZ() != click.nz
                || hit.getDirection().get3DDataValue() != click.face) {
                if (verbose && (tryLogs++ < 40 || watched(x, y, z))) log("tryPlace " + rel(x, y, z) + ": aimed at " + rel(click.nx, click.ny, click.nz) + " face " + click.face + " but the crosshair is on " + (hit == null ? "nothing" : rel(hit.getBlockPos().getX(), hit.getBlockPos().getY(), hit.getBlockPos().getZ()) + " face " + hit.getDirection()));
                continue;
            }
            int pred = oracle.predict(sp.item, hit, click.yaw, click.pitch, sneak);
            if (pred == 0 || (StateInfo.loose[pred] != StateInfo.loose[sp.first] && !shootFor(pred, sp.first)) || oracle.placedAt.getX() != x || oracle.placedAt.getY() != y || oracle.placedAt.getZ() != z) {
                if (verbose && (tryLogs++ < 40 || watched(x, y, z))) log("tryPlace " + rel(x, y, z) + ": click on " + rel(click.nx, click.ny, click.nz) + " face " + click.face + " would give " + StateInfo.name(pred) + " at " + rel(oracle.placedAt.getX(), oracle.placedAt.getY(), oracle.placedAt.getZ()) + ", wanted " + StateInfo.name(sp.first) + " yaw=" + click.yaw + " pitch=" + click.pitch + " sneak=" + sneak);
                continue;
            }
            if (sp.chest && StateInfo.canon[pred] != StateInfo.canon[chestWant(x, y, z, t)]) continue;
            c.look(click.yaw, click.pitch);
            c.clickUse();
            if (sneak) c.sneak(true);
            tick();
            int now = g.get(x, y, z);
            if (now == 0) { failedClicks++; continue; }
            placed++;
            if (!sameKind(now, t) && StateInfo.loose[now] != StateInfo.loose[sp.first] && !shootFor(now, t)) {
                // not what was meant to go here (should not happen): take it out again
                misplaced++;
                log("MISPLACED " + StateInfo.name(now) + " in " + rel(x, y, z) + " wanted " + StateInfo.name(t) + " held=" + heldItem() + " predicted=" + StateInfo.name(pred));
                var e2 = c.eye();
                if (findBreak(x, y, z, e2.x, e2.y, e2.z, click)) breakBlock(x, y, z, click.ax, click.ay, click.az, 400);
                return false;
            }
            if (sp.multi && !matches(now, t)) topUp(x, y, z, t, sp);
            if (sp.content != null) fillPot(x, y, z, t, sp);
            if (!matches(g.get(x, y, z), t) && StateInfo.loose[g.get(x, y, z)] == StateInfo.loose[t]) adjust(x, y, z, t);
            return true;   // (a block is there; if it is not finished yet, doCell comes back to it)
        }
        return false;
    }

    /** Uses the plant on the empty pot just placed. */
    void fillPot(int x, int y, int z, int t, Spec sp) {
        if (verbose) log("fillPot " + rel(x, y, z) + " have " + count(sp.content) + " of " + sp.content + " now=" + StateInfo.name(g.get(x, y, z)));
        if (matches(g.get(x, y, z), t) || count(sp.content) == 0) return;
        boolean wasSneak = c.sneaking();
        if (!hold(sp.content)) return;
        if (c.sneaking()) { c.releaseKeys(); tick(); tick(); }
        for (int attempt = 0; attempt < 3 && !matches(g.get(x, y, z), t); attempt++) {
            if (!c.onGround()) settle();
            if (!aimAtBlock(x, y, z)) {
                // not in view from here: somewhere it is
                if (leaning) break;
                Stance st = planStance(x, y, z, null, true);
                if (st == null || !goTo(st)) break;
                if (!hold(sp.content) || !aimAtBlock(x, y, z)) { badStand.add(g.idx(st.x, st.y, st.z)); continue; }
            }
            c.clickUse();
            tick();
            if (verbose) log("fillPot clicked; now=" + StateInfo.name(g.get(x, y, z)) + " hit=" + (c.lastHit() == null ? null : c.lastHit().getBlockPos()));
        }
        if (wasSneak) { c.keys(false, false, false, false, false, true, false); tick(); tick(); }
    }

    /**
     * Turns the view to some point of block (x,y,z) that the crosshair really rests on from where the bot stands
     * (asked of the game itself: thin shapes are easily hidden behind their neighbours). False if there is none in reach.
     */
    boolean aimAtBlock(int x, int y, int z) {
        int s = g.get(x, y, z);
        if (s == 0) return false;
        float[] b = Ray.outline(s);
        var e = c.eye();
        for (int k = 0; k < b.length; k += 6) {
            double x0 = x + b[k], y0 = y + b[k + 1], z0 = z + b[k + 2], x1 = x + b[k + 3], y1 = y + b[k + 4], z1 = z + b[k + 5];
            double mx = (x0 + x1) / 2, my = (y0 + y1) / 2, mz = (z0 + z1) / 2;
            double[][] pts = {{mx, my, mz}, {mx, y1 - 0.01, mz}, {mx, y0 + 0.01, mz}, {x0 + 0.01, my, mz}, {x1 - 0.01, my, mz}, {mx, my, z0 + 0.01}, {mx, my, z1 - 0.01},
                {x0 + 0.1, my, z0 + 0.1}, {x1 - 0.1, my, z0 + 0.1}, {x0 + 0.1, my, z1 - 0.1}, {x1 - 0.1, my, z1 - 0.1}};
            for (double[] pt : pts) {
                double dx = pt[0] - e.x, dy = pt[1] - e.y, dz = pt[2] - e.z;
                double dist = Math.sqrt(dx * dx + dy * dy + dz * dz), hd = Math.sqrt(dx * dx + dz * dz);
                if (dist > 4.45 || dist < 1e-6) continue;
                float yaw = hd < 1e-6 ? c.yaw() : yawTo(dx, dz), pitch = (float) -Math.toDegrees(Math.atan2(dy, hd));
                BlockHitResult hit = oracle.pick(c, yaw, pitch);
                if (hit != null && hit.getBlockPos().getX() == x && hit.getBlockPos().getY() == y && hit.getBlockPos().getZ() == z) { c.look(yaw, pitch); return true; }
            }
        }
        return false;
    }

    /** Right-clicks a placed block until its click-settable properties match (repeater delay, open trapdoor ...). */
    void adjust(int x, int y, int z, int t) {
        if (StateInfo.state[t].getBlock() instanceof net.minecraft.world.level.block.ChestBlock) return; // halves join by themselves
        if (bodyIn(x, y, z) && !StateInfo.walkThrough[t]) return;   // (not while standing in it: a gate would close around the bot)
        boolean wasSneak = c.sneaking();
        if (wasSneak) { c.releaseKeys(); tick(); tick(); }
        for (int n = 0; n < 26; n++) {
            int now = g.get(x, y, z);
            if (now == 0 || matches(now, t) || StateInfo.loose[now] != StateInfo.loose[t]) break;
            var e = c.eye();
            if (!aimAtBlock(x, y, z)) { if (TRACE && verbose && failLogs++ < 300) log("adjust " + rel(x, y, z) + " " + StateInfo.name(now) + ": not in view from " + String.format("%.2f,%.2f,%.2f", e.x - g.ox, e.y - g.oy, e.z - g.oz)); break; }
            c.clickUse();
            tick();
            if (g.get(x, y, z) == now) { tick(); if (g.get(x, y, z) == now) {
                if (TRACE && verbose && failLogs++ < 300) log("adjust " + rel(x, y, z) + " " + StateInfo.name(now) + ": click did nothing; hit=" + (c.lastHit() == null ? "none" : rel(c.lastHit().getBlockPos().getX(), c.lastHit().getBlockPos().getY(), c.lastHit().getBlockPos().getZ()) + " " + c.lastHit().getDirection()) + " held=" + heldItem() + " sneaking=" + c.sneaking() + " screen=" + c.screenOpen());
                break; } }
        }
    }

    /** Further clicks on a block already there until it has the wanted count (second slab half, more snow layers ...). */
    boolean topUp(int x, int y, int z, int t, Spec sp) {
        for (int n = 0; n < 8; n++) {
            int now = g.get(x, y, z);
            if (matches(now, t)) return true;
            if (now == 0 || count(sp.item) == 0 || !hold(sp.item)) return false;
            if (topUpClick(x, y, z, t, sp, now)) continue;
            // not from here: look for a place from which the top of the block can be seen
            planTopUp = t;
            Stance st;
            try { st = planStance(x, y, z, sp, true); } finally { planTopUp = 0; }
            if (st == null || !goTo(st)) { if (TRACE && verbose && failLogs++ < 300) log("topUp " + rel(x, y, z) + ": " + (st == null ? "no stance" : "stance " + rel(st.x, st.y, st.z) + " not reached")); return false; }
            if (!hold(sp.item) || !topUpClick(x, y, z, t, sp, now)) { if (TRACE && verbose && failLogs++ < 300) log("topUp " + rel(x, y, z) + ": no click from stance " + rel(st.x, st.y, st.z) + " eye=" + String.format("%.2f,%.2f,%.2f", c.eye().x - g.ox, c.eye().y - g.oy, c.eye().z - g.oz)); return false; }
        }
        return matches(g.get(x, y, z), t);
    }

    /** Could a click from the given eye position add to the block in (x,y,z) (second slab half, another snow layer ...)? */
    boolean topUpVisible(int x, int y, int z, int t, Spec sp, double ex, double ey, double ez) {
        int now = g.get(x, y, z);
        if (now == 0) return false;
        // 1. on the block itself, from above
        double ay = y + (StateInfo.anyTop[now] == 0 ? 0.1 : StateInfo.anyTop[now] / 16.0);
        if (ey > ay + 0.05) {
            for (int k = 0; k < 9; k++) {
                double ax = x + 0.5 + (k % 3 - 1) * 0.3, az = z + 0.5 + (k / 3 - 1) * 0.3;
                double dx = ax - ex, dy = ay - ey, dz = az - ez, hd = Math.sqrt(dx * dx + dz * dz);
                if (Math.sqrt(dx * dx + dy * dy + dz * dz) > reach - 0.1) continue;
                if (!ray.cast(g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, reach) || ray.hx != x || ray.hy != y || ray.hz != z || ray.face != 1) continue;
                float yaw = hd < 1e-6 ? 0f : yawTo(dx, dz), pitch = (float) -Math.toDegrees(Math.atan2(dy, hd));
                BlockHitResult hit = new BlockHitResult(new net.minecraft.world.phys.Vec3(ax, ay, az), net.minecraft.core.Direction.UP, new net.minecraft.core.BlockPos(x, y, z), false);
                int pred = oracle.predict(sp.item, hit, yaw, pitch, false);
                if (pred != 0 && pred != now && StateInfo.state[pred].getBlock() == StateInfo.state[t].getBlock() && oracle.placedAt.getX() == x && oracle.placedAt.getY() == y && oracle.placedAt.getZ() == z) return true;
            }
        }
        // 2. on a neighbour's face that looks into the cell
        for (int d = 0; d < 6; d++) {
            if (!findClick(x, y, z, t, null, ex, ey, ez, false, 1 << d, click)) continue;
            BlockHitResult hit = new BlockHitResult(new net.minecraft.world.phys.Vec3(click.ax, click.ay, click.az), net.minecraft.core.Direction.from3DDataValue(click.face), new net.minecraft.core.BlockPos(click.nx, click.ny, click.nz), false);
            int pred = oracle.predict(sp.item, hit, click.yaw, click.pitch, false);
            if (pred != 0 && pred != now && StateInfo.state[pred].getBlock() == StateInfo.state[t].getBlock() && oracle.placedAt.getX() == x && oracle.placedAt.getY() == y && oracle.placedAt.getZ() == z) return true;
        }
        return false;
    }

    boolean topUpClick(int x, int y, int z, int t, Spec sp, int now) {
        if (!c.onGround() || hspeed() > 0.003) settle();
        var e = c.eye();
        // 1. on the block itself
        for (int k = 0; k < 9; k++) {
            double ax = x + 0.5 + (k % 3 - 1) * 0.3, az = z + 0.5 + (k / 3 - 1) * 0.3, ay = y + StateInfo.anyTop[now] / 16.0;
            if (StateInfo.anyTop[now] == 0) ay = y + 0.1;
            double dx = ax - e.x, dy = ay - e.y, dz = az - e.z, hd = Math.sqrt(dx * dx + dz * dz);
            if (Math.sqrt(dx * dx + dy * dy + dz * dz) > reach - 0.1) continue;
            float yaw = hd < 1e-6 ? c.yaw() : yawTo(dx, dz), pitch = (float) -Math.toDegrees(Math.atan2(dy, hd));
            if (topUpTry(x, y, z, t, sp, now, yaw, pitch)) return true;
        }
        // 2. on a neighbour's face that looks into the cell (how a second slab half goes in from the side)
        for (int d = 0; d < 6; d++) {
            if (!findClick(x, y, z, t, null, e.x, e.y, e.z, c.sneaking(), 1 << d, click)) continue;
            if (topUpTry(x, y, z, t, sp, now, click.yaw, click.pitch)) return true;
        }
        return false;
    }

    boolean topUpTry(int x, int y, int z, int t, Spec sp, int now, float yaw, float pitch) {
        BlockHitResult hit = oracle.pick(c, yaw, pitch);
        if (hit == null) return false;
        if (StateInfo.interactive[g.get(hit.getBlockPos().getX(), hit.getBlockPos().getY(), hit.getBlockPos().getZ())] && !c.sneaking()) return false;
        int pred = oracle.predict(sp.item, hit, yaw, pitch, c.sneaking());
        if (pred == 0 || pred == now || oracle.placedAt.getX() != x || oracle.placedAt.getY() != y || oracle.placedAt.getZ() != z) return false;
        if (StateInfo.state[pred].getBlock() != StateInfo.state[t].getBlock()) return false;
        c.look(yaw, pitch);
        c.clickUse();
        if (c.sneaking()) c.sneak(true);
        tick();
        if (g.get(x, y, z) == now) return false;
        placed++;
        return true;
    }

    boolean selfPlaceable(int x, int y, int z, Spec sp) {
        // only for blocks one ends up standing on: anything thinner would leave the bot standing inside it
        if (!StateInfo.fullCube[sp.first] || sp.multi) return false;
        if (g.stand16(x, y, z) != 0 || !g.passable(x, y + 2, z)) return false;
        int below = g.get(x, y - 1, z);
        if (!Ray.fullOutline(below) || StateInfo.interactive[below] || StateInfo.replaceable[below]) return false;
        if (sp.table == null) return true;
        for (int yc = 0; yc < Oracle.NYAW; yc++) {
            boolean ok = true;
            for (int q = 0; q < 4 && ok; q++) {
                int st = sp.table[Oracle.index(1, q, yc, Oracle.pitchClass(90), false)];
                if (st == 0 || StateInfo.loose[st] != StateInfo.loose[sp.first]) ok = false;
            }
            if (ok) return true;
        }
        return false;
    }

    /** The bot stands in the cell: jump and place the block underfoot, if the block can be placed looking straight down. */
    boolean selfPlace(int x, int y, int z, int t, Spec sp) {
        if (!StateInfo.fullCube[sp.first] || sp.multi) return false;
        if (cy() != y || g.stand16(x, y, z) != 0 || !g.passable(x, y + 2, z)) return false;
        int below = g.get(x, y - 1, z);
        if (!Ray.fullOutline(below) || StateInfo.interactive[below] || StateInfo.replaceable[below]) return false;
        // which way to face? (for blocks that take their direction from the player)
        float yaw = c.yaw();
        if (sp.table != null) {
            int found = -1;
            for (int yc = 0; yc < Oracle.NYAW && found < 0; yc++) {
                boolean ok = true;
                for (int q = 0; q < 4 && ok; q++) {
                    int st = sp.table[Oracle.index(1, q, yc, Oracle.pitchClass(90), false)];
                    if (st == 0 || StateInfo.loose[st] != StateInfo.loose[sp.first]) ok = false;
                }
                if (ok) found = yc;
            }
            if (found < 0) return false;
            yaw = Oracle.yawCentre(found);
        }
        if (!hold(sp.item) || !centre(x, y, z)) return false;
        settle();
        double y0 = c.y();
        c.look(yaw, 90);
        c.keys(false, false, false, false, true, false, false);
        boolean ok = false;
        for (int n = 0; n < 14; n++) {
            if (!ok && c.y() >= y0 + 1.0 && g.get(x, y, z) == 0) {
                BlockHitResult hit = oracle.pick(c, yaw, 90);
                if (hit != null && hit.getBlockPos().getX() == x && hit.getBlockPos().getY() == y - 1 && hit.getBlockPos().getZ() == z) {
                    int pred = oracle.predict(sp.item, hit, yaw, 90, false);
                    if (pred != 0 && StateInfo.loose[pred] == StateInfo.loose[sp.first]) c.clickUse();
                }
            }
            tick();
            if (n == 0) c.jump(false);
            if (g.get(x, y, z) != 0) ok = true;
            if (c.onGround() && n > 2) break;
        }
        c.releaseKeys();
        if (ok) {
            placed++;
            settle();
            if (sp.multi) topUp(x, y, z, t, sp);
            if (!matches(g.get(x, y, z), t) && StateInfo.loose[g.get(x, y, z)] == StateInfo.loose[t]) adjust(x, y, z, t);
        }
        return ok && g.get(x, y, z) != 0;
    }

    private static final double[] INSIDE = {0.5, 0.3, 0.7, 0.12, 0.88, 0.04, 0.96};

    /**
     * Could the bot stand in the middle of this cell while the block goes in around it? A thin thing at the edge of
     * its cell - a door flat against a wall, a ladder, an open trapdoor - or anything one can walk through.
     */
    boolean insidePlaceable(int x, int y, int z, Spec sp) {
        if (sp.multi || sp.chest || sp.content != null || StateInfo.fullCube[sp.first]) return false;
        if (g.stand16(x, y, z) < 0) return false;
        if (StateInfo.walkThrough[sp.first]) return true;
        float[] b = Ray.outline(sp.first);
        for (int k = 0; k < b.length; k += 6)
            if (b[k] < 0.805 && b[k + 3] > 0.195 && b[k + 2] < 0.805 && b[k + 5] > 0.195) return false;   // (the body is 0.6 wide)
        return true;
    }

    /** Is there a click that gives the wanted state for a bot standing in the middle of the cell? (Asked before going there.) */
    private boolean insideClick(int x, int y, int z, Spec sp) {
        double ex = x + 0.5, ey = y + g.stand16(x, y, z) / 16.0 + 1.62, ez = z + 0.5;
        for (int d = 0; d < 6; d++) {
            int nx = x + DX[d], ny = y + DY[d], nz = z + DZ[d];
            int s = g.get(nx, ny, nz);
            if (s == 0 || StateInfo.replaceable[s] || StateInfo.interactive[s]) continue;
            for (double u : INSIDE) for (double v : INSIDE) {
                double ax = x + 0.5 + 0.5 * DX[d], ay = y + 0.5 + 0.5 * DY[d], az = z + 0.5 + 0.5 * DZ[d];
                if (DX[d] != 0) { ay = y + u; az = z + v; } else if (DY[d] != 0) { ax = x + u; az = z + v; } else { ax = x + u; ay = y + v; }
                double dx = ax - ex, dy = ay - ey, dz = az - ez, hd = Math.sqrt(dx * dx + dz * dz);
                if (hd < 1e-6 || Math.sqrt(dx * dx + dy * dy + dz * dz) > reach - 0.05) continue;
                if (!ray.cast(g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, reach) || ray.hx != nx || ray.hy != ny || ray.hz != nz || ray.face != OPP[d]) continue;
                float yaw = yawTo(dx, dz), pitch = (float) -Math.toDegrees(Math.atan2(dy, hd));
                Item it = sp.item;
                BlockHitResult hit = new BlockHitResult(new net.minecraft.world.phys.Vec3(ax, ay, az), net.minecraft.core.Direction.from3DDataValue(OPP[d]), new net.minecraft.core.BlockPos(nx, ny, nz), false);
                int pred = oracle.predict(it, hit, yaw, pitch, false);
                if (pred != 0 && StateInfo.loose[pred] == StateInfo.loose[sp.first] && oracle.placedAt.getX() == x && oracle.placedAt.getY() == y && oracle.placedAt.getZ() == z) return true;
            }
        }
        return false;
    }

    /**
     * The last way of placing something: from inside its own cell. Where a door stands flat against a wall, the
     * side one would normally place it from is that wall; the same click (looking at the wall, on the floor) can
     * only be made standing where the door goes. Every click is put to the rules first, as everywhere else.
     */
    boolean placeFromInside(int x, int y, int z, int t, Spec sp) {
        if (!insidePlaceable(x, y, z, sp) || !insideClick(x, y, z, sp)) return false;
        if (sealsIn(x, y, z, sp.first)) { sealRefused = true; return false; }
        if (!(cx() == x && cy() == y && cz() == z && c.onGround())) {
            endLean();
            if (!rope.isEmpty()) undoRope();
            if (!route(x, y, z, 20000) || !travel()) return false;
        }
        if (!(cx() == x && cy() == y && cz() == z) || !centre(x, y, z)) return false;
        settle();
        if (!hold(sp.item)) return false;
        boolean needsSupport = !oracle.floats(sp.first);
        var e = c.eye();
        for (int d = 0; d < 6; d++) {
            int nx = x + DX[d], ny = y + DY[d], nz = z + DZ[d];
            int s = g.get(nx, ny, nz);
            if (s == 0 || StateInfo.replaceable[s] || StateInfo.interactive[s]) continue;
            if (needsSupport && g.in(nx, ny, nz) && scaffold.contains(g.idx(nx, ny, nz))) continue;
            for (double u : INSIDE) for (double v : INSIDE) {
                // a point on the side of the neighbour that looks into the cell
                double ax = x + 0.5 + 0.5 * DX[d], ay = y + 0.5 + 0.5 * DY[d], az = z + 0.5 + 0.5 * DZ[d];
                if (DX[d] != 0) { ay = y + u; az = z + v; } else if (DY[d] != 0) { ax = x + u; az = z + v; } else { ax = x + u; ay = y + v; }
                double dx = ax - e.x, dy = ay - e.y, dz = az - e.z, hd = Math.sqrt(dx * dx + dz * dz);
                if (Math.sqrt(dx * dx + dy * dy + dz * dz) > reach - 0.05) continue;
                float yaw = hd < 1e-6 ? c.yaw() : yawTo(dx, dz), pitch = (float) -Math.toDegrees(Math.atan2(dy, hd));
                BlockHitResult hit = oracle.pick(c, yaw, pitch);
                if (hit == null || hit.getBlockPos().getX() != nx || hit.getBlockPos().getY() != ny || hit.getBlockPos().getZ() != nz || hit.getDirection().get3DDataValue() != OPP[d]) continue;
                int pred = oracle.predict(sp.item, hit, yaw, pitch, false);
                if (pred == 0 || StateInfo.loose[pred] != StateInfo.loose[sp.first] || oracle.placedAt.getX() != x || oracle.placedAt.getY() != y || oracle.placedAt.getZ() != z) continue;
                c.look(yaw, pitch);
                c.clickUse();
                tick();
                int now = g.get(x, y, z);
                if (now == 0) { failedClicks++; continue; }
                placed++;
                if (!sameKind(now, t) && StateInfo.loose[now] != StateInfo.loose[sp.first]) {
                    misplaced++;
                    log("MISPLACED " + StateInfo.name(now) + " in " + rel(x, y, z) + " wanted " + StateInfo.name(t) + " (from inside) predicted=" + StateInfo.name(pred));
                    return false;
                }
                if (!matches(g.get(x, y, z), t) && StateInfo.loose[g.get(x, y, z)] == StateInfo.loose[t]) adjust(x, y, z, t);
                return true;
            }
        }
        return false;
    }

    private static final double[] NEAR = {0, -0.19, 0.19};
    private final it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap closeFailed = new it.unimi.dsi.fastutil.ints.Int2IntOpenHashMap();
    private double closeX, closeZ;
    private boolean closeSneak;
    private int closeSx, closeSy, closeSz;
    private boolean closePit;

    /**
     * Is there a spot right beside the cell - not just the middle of a block, and sneaking or not - from which some
     * click gives the wanted state? This is the fine-grained search for things that take their direction from the
     * view and want it steep (an observer looking up, a dropper or piston pointing down, when the cell is low and
     * the eyes are almost level with it). Result in closeSx.. / closeX, closeZ / closeSneak.
     */
    /** The fine search is made afresh for the cell (its surroundings have a helper block now). */
    void forgetCloseFailed(int cell) { closeFailed.remove(cell); }

    private boolean planClose(int x, int y, int z, Spec sp) {
        closePit = false;
        // (from two below the cell's level up to one above it, and from up to two blocks away: also the things that
        // take their direction from a level view - an observer or dropper whose back is built up)
        for (int sy = y + 1; sy >= y - 3; sy--) for (int ox = -2; ox <= 2; ox++) for (int oz = -2; oz <= 2; oz++) {
            if (ox == 0 && oz == 0) continue;
            int sx = x + ox, sz = z + oz;
            int h = g.stand16(sx, sy, sz);
            if (h < 0 || g.isTower(sx, sy - 1, sz) || badStand.contains(g.idx(sx, sy, sz))) continue;
            for (int sn = 0; sn < 2; sn++) for (double px : NEAR) for (double pz : NEAR) {
                double ex = sx + 0.5 + px, ey = sy + h / 16.0 + (sn == 1 ? 1.27 : 1.62), ez = sz + 0.5 + pz;
                if (closeAim(x, y, z, sp, ex, ey, ez, sn == 1, false)) {
                    closeSx = sx; closeSy = sy; closeSz = sz; closeX = sx + 0.5 + px; closeZ = sz + 0.5 + pz; closeSneak = sn == 1;
                    return true;
                }
            }
        }
        // A cell on the ground: the eyes are above it wherever the bot stands. What a player does then is dig a
        // hole beside it and look up from in there. (One block of the ground, where nothing is to be built.)
        if (y != g.oy) return false;
        for (int ox = -1; ox <= 1; ox++) for (int oz = -1; oz <= 1; oz++) {
            if (ox == 0 && oz == 0) continue;
            int sx = x + ox, sz = z + oz, sy = g.y0;
            if (!g.in(sx, sy, sz) || sz < g.oz - 1 || g.get(sx, sy, sz) != g.groundState || g.get(sx, sy + 1, sz) != 0 || g.get(sx, sy + 2, sz) != 0) continue;
            if (g.target(sx, sy + 1, sz) != 0 || g.stand16(sx, sy + 1, sz) != 0) continue;
            int pi = g.idx(sx, sy, sz);
            g.cur[pi] = 0;
            try {
                for (int sn = 0; sn < 2; sn++) for (double px : NEAR) for (double pz : NEAR) {
                    double ex = sx + 0.5 + px, ey = sy + (sn == 1 ? 1.27 : 1.62), ez = sz + 0.5 + pz;
                    if (closeAim(x, y, z, sp, ex, ey, ez, sn == 1, false)) {
                        closeSx = sx; closeSy = sy; closeSz = sz; closeX = sx + 0.5 + px; closeZ = sz + 0.5 + pz; closeSneak = sn == 1;
                        closePit = true;
                        return true;
                    }
                }
            } finally { g.cur[pi] = (char) g.groundState; }
        }
        return false;
    }

    /** One eye position against every face around the cell; with act, the click is made (through the game's own ray cast). */
    private boolean closeAim(int x, int y, int z, Spec sp, double ex, double ey, double ez, boolean sneak, boolean act) {
        for (int d = 0; d < 6; d++) {
            int nx = x + DX[d], ny = y + DY[d], nz = z + DZ[d];
            int s = g.get(nx, ny, nz);
            if (s == 0 || StateInfo.replaceable[s] || (StateInfo.interactive[s] && !sneak)) continue;
            if (g.in(nx, ny, nz) && scaffold.contains(g.idx(nx, ny, nz)) && !oracle.floats(sp.first)) continue;
            // the face looks into the cell: the eye has to be on that side of it
            double side = DX[d] != 0 ? (x + 0.5 + 0.5 * DX[d] - ex) * DX[d] : DY[d] != 0 ? (y + 0.5 + 0.5 * DY[d] - ey) * DY[d] : (z + 0.5 + 0.5 * DZ[d] - ez) * DZ[d];
            if (side < 0.02) continue;
            for (double u : INSIDE) for (double v : INSIDE) {
                double ax = x + 0.5 + 0.5 * DX[d], ay = y + 0.5 + 0.5 * DY[d], az = z + 0.5 + 0.5 * DZ[d];
                if (DX[d] != 0) { ay = y + u; az = z + v; } else if (DY[d] != 0) { ax = x + u; az = z + v; } else { ax = x + u; ay = y + v; }
                double dx = ax - ex, dy = ay - ey, dz = az - ez, hd = Math.sqrt(dx * dx + dz * dz);
                if (hd < 1e-6 || Math.sqrt(dx * dx + dy * dy + dz * dz) > reach - 0.05) continue;
                float yaw = yawTo(dx, dz), pitch = (float) -Math.toDegrees(Math.atan2(dy, hd));
                BlockHitResult hit;
                if (act) {
                    hit = oracle.pick(c, yaw, pitch);
                    if (hit == null || hit.getBlockPos().getX() != nx || hit.getBlockPos().getY() != ny || hit.getBlockPos().getZ() != nz || hit.getDirection().get3DDataValue() != OPP[d]) continue;
                } else {
                    if (!ray.cast(g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, reach) || ray.hx != nx || ray.hy != ny || ray.hz != nz || ray.face != OPP[d]) continue;
                    hit = new BlockHitResult(new net.minecraft.world.phys.Vec3(ax, ay, az), net.minecraft.core.Direction.from3DDataValue(OPP[d]), new net.minecraft.core.BlockPos(nx, ny, nz), false);
                }
                int pred = oracle.predict(sp.item, hit, yaw, pitch, sneak);
                if (pred == 0 || StateInfo.loose[pred] != StateInfo.loose[sp.first] || oracle.placedAt.getX() != x || oracle.placedAt.getY() != y || oracle.placedAt.getZ() != z) continue;
                if (!act) return true;
                c.look(yaw, pitch);
                c.clickUse();
                tick();
                if (g.get(x, y, z) != 0) return true;
                failedClicks++;
            }
        }
        return false;
    }

    /** See planClose: goes to the spot found there and places the block. */
    boolean placeClose(int x, int y, int z, int t, Spec sp) {
        if (sp.multi || sp.chest || sp.content != null) return false;
        int i = g.idx(x, y, z), sur = surroundings(i);
        if (closeFailed.containsKey(i) && closeFailed.get(i) == sur) return false;
        reach = 4.5;
        if (!planClose(x, y, z, sp)) { closeFailed.put(i, sur); return false; }
        if (sealsIn(x, y, z, sp.first)) { sealRefused = true; return false; }
        int sx = closeSx, sy = closeSy, sz = closeSz;
        if (closePit) {
            // onto the spot, then the block underfoot goes
            endLean();
            if (!rope.isEmpty()) undoRope();
            if (!(cx() == sx && cy() == sy + 1 && cz() == sz && c.onGround()) && (!route(sx, sy + 1, sz, 20000) || !travel())) return false;
            if (!(cx() == sx && cy() == sy + 1 && cz() == sz) || !centre(sx, sy + 1, sz)) return false;
            settle();
            if (!breakBlock(sx, sy, sz, sx + 0.5, sy + 1.0, sz + 0.5, 200)) return false;
            pits++;
            for (int n = 0; n < 20 && !(cy() == sy && c.onGround()); n++) tick();
        } else if (!(cx() == sx && cy() == sy && cz() == sz && c.onGround())) {
            endLean();
            if (!rope.isEmpty()) undoRope();
            if (!route(sx, sy, sz, 20000) || !travel()) return false;
        }
        if (!(cx() == sx && cy() == sy && cz() == sz) || !centre(sx, sy, sz) || !hold(sp.item)) return false;
        settle();
        // the last few tenths of a block, slowly
        for (int n = 0; n < 40; n++) {
            double dx = closeX - c.x(), dz = closeZ - c.z(), dist = Math.sqrt(dx * dx + dz * dz);
            if (dist < 0.05) break;
            c.look(yawTo(dx, dz), c.pitch());
            c.keys(hspeed() < dist, false, false, false, false, true, false);
            tick();
        }
        c.keys(false, false, false, false, false, closeSneak, false);
        for (int n = 0; n < 6 && (hspeed() > 0.003 || c.sneaking() != closeSneak); n++) tick();
        tick();
        var e = c.eye();
        boolean ok = cy() == sy && closeAim(x, y, z, sp, e.x, e.y, e.z, c.sneaking(), true);
        c.releaseKeys();
        tick();
        centre(sx, sy, sz);
        if (!ok) { badStand.add(g.idx(sx, sy, sz)); return false; }
        placed++;
        int now = g.get(x, y, z);
        if (!sameKind(now, t) && StateInfo.loose[now] != StateInfo.loose[sp.first]) {
            misplaced++;
            log("MISPLACED " + StateInfo.name(now) + " in " + rel(x, y, z) + " wanted " + StateInfo.name(t) + " (from close by)");
            return false;
        }
        if (!matches(now, t) && StateInfo.loose[now] == StateInfo.loose[t]) adjust(x, y, z, t);
        return true;
    }

    /** Before leaving a spot: place whatever else is allowed and within reach from here. */
    void sweepHere() {
        if (colNext == null || sweeping) return;
        sweeping = true;
        try {
            boolean again = true;
            int rounds = 0;
            while (again && rounds++ < 40) {
                again = false;
                int bx = cx(), by = cy(), bz = cz();
                for (int dz = -4; dz <= 4; dz++) for (int dx = -4; dx <= 4; dx++) {
                    int x = bx + dx, z = bz + dz;
                    if (x < g.ox || z < g.oz || x >= g.ox + g.sx || z >= g.oz + g.sz) continue;
                    int y = colNext[col(x, z)];
                    if (y == NONE || y > by + 5 || y < by - 5 || !eligible(x, z)) continue;
                    int i = g.idx(x, y, z);
                    int t = g.tgt[i];
                    Spec sp = spec(t);
                    if (!sp.placeable || g.cur[i] != 0 || count(sp.item) == 0 || bodyIn(x, y, z)) continue;
                    if (scaffoldInColumn[col(x, z)] != 0 && looseBelow(x, y, z)) continue;
                    if (placeFromHere(x, y, z, t, sp)) {
                        blocked.remove(i);
                        setNext(x, z, scanColumn(x, y + 1, z));
                        again = true;
                    }
                }
            }
        } finally {
            sweeping = false;
        }
    }
    boolean sweeping;
}
