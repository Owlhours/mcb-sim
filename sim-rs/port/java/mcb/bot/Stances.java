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

/** Choosing where to stand for a cell: somewhere that exists, an edge to lean over, or a temporary column. */
abstract class Stances extends Placing {
    Stances(Client c, Grid g, Site site, Oracle oracle) { super(c, g, site, oracle); }

    final int towerState = Block.getId(net.minecraft.world.level.block.Blocks.SCAFFOLDING.defaultBlockState());
    static final int[] DYS = {0, 1, -1, -2, 2, -3, -4};
    static final double PLAN_REACH = 4.15;
    final int[] candX = new int[64], candY = new int[64], candZ = new int[64], candD = new int[64];

    private final int[][] floating = new int[24][];
    private int nFloating;

    Stance planStance(int x, int y, int z, Spec sp, boolean breaking) {
        plans++;
        nFloating = 0;
        dbgStand = dbgClick = dbgNav = dbgPillar = dbgPillarClick = 0;
        Stance best = null;
        int found = 0;
        reach = PLAN_REACH;
        try {
            // 0. on top of the block below the cell itself, placing underfoot with a jump
            if (!breaking && selfPlaceable(x, y, z, sp) && !badStand.contains(g.idx(x, y, z)) && Math.abs(cx() - x) + Math.abs(cz() - z) <= 3 && cy() >= y - 1
                && route(x, y, z, 600)) {
                best = new Stance(); best.x = x; best.y = y; best.z = z; best.cost = nav.goalCost;
                nSelf++;
                return best;
            }
            // 1. somewhere one can already stand: collect the spots that would do, then take the first that can be reached,
            //    trying those nearest to the bot first
            int nc = 0;
            int bx = cx(), by = cy(), bz = cz();
            boolean touchesInteractive = false;
            for (int d = 0; d < 6 && !touchesInteractive; d++) touchesInteractive = StateInfo.interactive[g.get(x + DX[d], y + DY[d], z + DZ[d])];
            for (int[] o : RING) {
                int nx = x + o[0], nz = z + o[1];
                for (int dy : DYS) {
                    int ny = y + dy;
                    if (nx == x && nz == z && (ny == y || ny == y - 1)) continue;
                    int h = g.stand16(nx, ny, nz);
                    if (h < 0 || badStand.contains(g.idx(nx, ny, nz))) continue;
                    dbgStand++;
                    double ex = nx + 0.5, ey = ny + h / 16.0 + 1.62, ez = nz + 0.5;
                    boolean ok = breaking ? (planTopUp != 0 ? topUpVisible(x, y, z, planTopUp, sp, ex, ey, ez) : findBreak(x, y, z, ex, ey, ez, click)) : findClick(x, y, z, sp.first, sp.table, ex, ey, ez, false, 0x3f, click);
                    // against chests, doors and other things that react to a click one sneaks (eyes 0.35 lower)
                    if (!ok && !breaking && touchesInteractive && !g.isTower(nx, ny - 1, nz)) ok = findClick(x, y, z, sp.first, sp.table, ex, ey - 0.35, ez, true, 0x3f, click);
                    if (!ok) continue;
                    dbgClick++;
                    if (nc < candX.length) {
                        candX[nc] = nx; candY[nc] = ny; candZ[nc] = nz;
                        candD[nc] = Math.abs(nx - bx) + Math.abs(nz - bz) + 3 * Math.max(0, ny - by);
                        nc++;
                    }
                }
            }
            for (int tries = 0; tries < 4 && tries < nc && best == null; tries++) {
                int k = -1;
                for (int j = 0; j < nc; j++) if (candD[j] >= 0 && (k < 0 || candD[j] < candD[k])) k = j;
                if (k < 0) break;
                candD[k] = -1;
                if (avoidSeal != 0 && !breaking && !bodyIn(x, y, z)) {
                    // the block would close the room this spot is in (it was refused from in there once already):
                    // look for a spot on the other side
                    int ti = g.idx(x, y, z);
                    char was = g.cur[ti];
                    g.cur[ti] = (char) avoidSeal;
                    boolean open = wayOut(candX[k], candY[k], candZ[k]);
                    g.cur[ti] = was;
                    if (!open) { tries--; dbgNav++; continue; }   // (counted as reachable: the bot is not shut in, it declines)
                }
                if (!route(candX[k], candY[k], candZ[k], 6000)) continue;
                dbgNav++;
                best = new Stance(); best.x = candX[k]; best.y = candY[k]; best.z = candZ[k]; best.cost = nav.goalCost;
            }
            if (best != null) { nNatural++; return best; }
            // 2. on top of a block beside the cell, leaning out over it
            if (!breaking) {
                for (int d = 0; d < 4; d++) {
                    int nx = x + DXH[d], nz = z + DZH[d];
                    if (!g.solidCube(nx, y, nz) || g.stand16(nx, y + 1, nz) != 0) continue;
                    if (badStand.contains(g.idx(nx, y + 1, nz) ^ ((d ^ 1) + 1) << 28)) continue;
                    // leaning from (nx,nz) towards the cell means moving in the direction opposite to d
                    double ex = nx + 0.5 - DXH[d] * 0.75, ez = nz + 0.5 - DZH[d] * 0.75, ey = y + 1 + 1.27;
                    if (!findClick(x, y, z, sp.first, sp.table, ex, ey, ez, true, 0x3c, click)) continue;
                    if (!route(nx, y + 1, nz, 6000)) continue;
                    if (best == null || nav.goalCost < best.cost) { best = new Stance(); best.x = nx; best.y = y + 1; best.z = nz; best.lean = d ^ 1; best.cost = nav.goalCost; }
                }
                if (best != null) { nLean++; return best; }
            }
            // 3. a temporary column near the cell: scaffolding to climb if possible, else a pillar of loose blocks
            for (int[] o : RING) {
                if (o[0] == 0 && o[1] == 0) continue;
                if (o[0] * o[0] + o[1] * o[1] > 8) break;
                int qx = x + o[0], qz = z + o[1];
                if (!g.in(qx, y, qz)) continue;
                boolean adjacent = Math.abs(o[0]) + Math.abs(o[1]) == 1;
                for (int top = adjacent ? y : y - 1; top >= y - 2; top--) {  // top = y of the highest block of the column
                    if (!g.passable(qx, top + 1, qz) || !g.passable(qx, top + 2, qz)) continue;
                    if (badStand.contains(g.idx(qx, top + 1, qz))) continue;
                    for (int variant = haveTower ? 0 : 1; variant < 2; variant++) {
                        boolean tw = variant == 0;
                        // find the base of the column
                        int base = top;
                        while (base >= g.y0 && g.isAir(qx, base, qz) && (tw ? towerOk(qx, base, qz) : scaffoldOk(qx, base, qz))) base--;
                        if (base == top || base < g.y0) continue;
                        int floor = g.get(qx, base, qz);
                        if (tw) {
                            if (!StateInfo.scaffolding[floor] && (!StateInfo.sturdyTop[floor] || scaffold.contains(g.idx(qx, base, qz)))) continue;
                        } else if (!Nav.flatTop(floor)) continue;
                        // loose blocks all the way, or (better) a scaffolding column with one loose block on top to sneak on
                        boolean cap = !tw && haveTower && scaffoldOk(qx, top, qz) && (StateInfo.scaffolding[floor] || (top - base >= 2 && StateInfo.sturdyTop[floor] && !scaffold.contains(g.idx(qx, base, qz))));
                        if (cap) for (int yy = base + 1; yy < top && cap; yy++) cap = towerOk(qx, yy, qz);
                        if (!tw && !cap && StateInfo.scaffolding[floor]) continue;
                        dbgPillar++;
                        // pretend the column is there
                        for (int yy = base + 1; yy <= top; yy++) g.cur[g.idx(qx, yy, qz)] = (char) (tw || (cap && yy < top) ? towerState : scaffoldState);
                        boolean ok;
                        int lean = -1;
                        double ex = qx + 0.5, ey = top + 1 + 1.62, ez = qz + 0.5;
                        ok = breaking ? (planTopUp != 0 ? topUpVisible(x, y, z, planTopUp, sp, ex, ey, ez) : findBreak(x, y, z, ex, ey, ez, click)) : findClick(x, y, z, sp.first, sp.table, ex, ey, ez, false, 0x3f, click);
                        if (!ok && !tw && !breaking && touchesInteractive) ok = findClick(x, y, z, sp.first, sp.table, ex, ey - 0.35, ez, true, 0x3f, click);
                        if (!ok && !tw && !breaking && adjacent && top == y) {
                            int d = o[0] == 1 ? 0 : o[0] == -1 ? 1 : o[1] == 1 ? 2 : 3;   // direction from the cell to the pillar
                            ex = qx + 0.5 - DXH[d] * 0.75; ez = qz + 0.5 - DZH[d] * 0.75; ey = top + 1 + 1.27;
                            ok = findClick(x, y, z, sp.first, sp.table, ex, ey, ez, true, 0x3c, click);
                            lean = d ^ 1;
                        }
                        for (int yy = base + 1; yy <= top; yy++) g.cur[g.idx(qx, yy, qz)] = 0;
                        if (!ok) continue;
                        dbgPillarClick++;
                        towersOff = !tw && !cap;
                        nav.extraEffort = 400 * (top - base);
                        boolean reach2;
                        try { reach2 = cap ? route(qx, top, qz, 20000) : route(qx, top + 1, qz, 20000); } finally { towersOff = false; nav.extraEffort = 0; }
                        if (!reach2) { if (TRACE && verbose && failLogs++ < 200) log("column stance " + rel(qx, top + 1, qz) + (tw ? " tower" : " loose") + " for " + rel(x, y, z) + ": no route, expanded=" + nav.expanded + " base=" + (base - g.oy) + " unreachableCached=" + unreachable.contains(g.idx(qx, top + 1, qz)) + " haveScaffold=" + haveScaffold + " count=" + count(scaffoldItem)); continue; }
                        Stance s = new Stance();
                        s.x = qx; s.y = top + 1; s.z = qz; s.lean = lean; s.cost = nav.goalCost; s.moss = !tw && !cap; s.cap = cap; s.column = top - base;
                        nPillarStance++;
                        return s;
                    }
                    floating[nFloating % floating.length] = new int[] {qx, top, qz};
                    nFloating++;
                }
            }
            // 4. nothing near by to stand a column on (the tip of a steep roof ...): a loose block hung in the air beside
            //    the cell, reached by bridging out from wherever a column can stand
            for (int k = 0; k < Math.min(nFloating, floating.length) && k < 12; k++) {
                int qx = floating[k][0], top = floating[k][1], qz = floating[k][2];
                if (!g.in(qx, top, qz) || !g.isAir(qx, top, qz) || !scaffoldOk(qx, top, qz) || g.stand16(qx, top + 1, qz) >= 0) continue;
                if (!g.passable(qx, top + 1, qz) || !g.passable(qx, top + 2, qz) || badStand.contains(g.idx(qx, top + 1, qz))) continue;
                g.cur[g.idx(qx, top, qz)] = (char) scaffoldState;
                double ex = qx + 0.5, ey = top + 1 + 1.62, ez = qz + 0.5;
                boolean ok = breaking ? (planTopUp != 0 ? topUpVisible(x, y, z, planTopUp, sp, ex, ey, ez) : findBreak(x, y, z, ex, ey, ez, click)) : findClick(x, y, z, sp.first, sp.table, ex, ey, ez, false, 0x3f, click);
                g.cur[g.idx(qx, top, qz)] = 0;
                if (!ok) continue;
                nav.extraEffort = 6000;
                boolean reached;
                try { reached = route(qx, top + 1, qz, 40000); } finally { nav.extraEffort = 0; }
                if (!reached) continue;
                Stance s = new Stance();
                s.x = qx; s.y = top + 1; s.z = qz; s.cost = nav.goalCost;
                nPillarStance++;
                return s;
            }
        } finally {
            reach = 4.5;
        }
        return null;
    }
}
