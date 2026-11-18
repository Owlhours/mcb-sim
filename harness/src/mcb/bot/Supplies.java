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

/** Trips to the supply chests: what to carry for the work ahead, what to put back, in which order to fetch. */
abstract class Supplies extends Scaffolds {
    Supplies(Client c, Grid g, Site site, Oracle oracle) { super(c, g, site, oracle); }

    int scaffoldTrips, towerTrips, chestLogs;

    int missingNow(Map<Item, Integer> need, Item it) {
        Integer n = need.get(it);
        return n == null ? 0 : n - count(it);
    }

    /** Goes to the chests and loads up for the cells still to do, lowest layers first. */
    void restock(int fromLayer) {
        restocking = true;
        try {
            restock0(fromLayer);
        } finally {
            restocking = false;
        }
        if (verbose) log("after restock: " + inventorySummary());
        if (verbose && count(scaffoldItem) == 0) {
            StringBuilder sb = new StringBuilder("no loose blocks after restock; item=" + scaffoldItem + " depot:");
            int n = 0;
            for (Site.Stock st : stock) if (st.item == scaffoldItem && n++ < 6) sb.append(' ').append(st.count).append('@').append(rel(st.chestX, st.chestY, st.chestZ)).append('/').append(st.slot);
            log(sb.toString());
        }
    }

    void restock0(int fromLayer) {
        String old = enter("restock");
        try { restock0Impl(fromLayer); } finally { activity = old; }
    }

    void restock0Impl(int fromLayer) {
        looseFails.clear();
        looseBlocked.clear();
        nav.blockedEdges.clear();   // (steps that failed once: the build has moved on since, let them be tried again)
        endLean();
        if (!rope.isEmpty()) undoRope();
        trips++;
        Map<Item, Integer> need = new LinkedHashMap<>();
        int slotsUsed = 0;
        // first what the cell in hand needs, and what the cells put off until later need
        if (mustFetch != null) { need.put(mustFetch, new ItemStack(mustFetch).getMaxStackSize()); slotsUsed++; }
        // scaffolding is used up (broken blocks drop nothing): carry more of it the more the build turns out to need
        int wantLoose = Math.min(64 * 4, 64 + 64 * scaffoldTrips), wantTower = Math.min(64 * 6, 128 + 64 * towerTrips);
        need.merge(scaffoldItem, wantLoose, Integer::sum);
        if (count(scaffoldItem) < 16) scaffoldTrips++;
        need.merge(towerItem, wantTower, Integer::sum);
        if (count(towerItem) < 16) towerTrips++;
        int reserved = (wantLoose + 63) / 64 + (wantTower + 63) / 64;
        // (what was put off is fetched when its time comes, at the end; until then it would only fill the inventory)
        for (int cell : deferred) {
            if (!mainDone || !needs(cell) || slotsUsed >= 16) continue;
            if (!lateToo && isLate(cell)) continue;
            int t = g.tgt[cell];
            Item it = StateInfo.item[t];
            if (it == Items.AIR || StateInfo.itemCount[t] == 0) continue;
            int now = need.getOrDefault(it, 0);
            if (now == 0) slotsUsed++;
            need.put(it, now + StateInfo.itemCount[t]);
        }
        int capacity = 35 - reserved;
        // then what the bot will be doing next: the cells that may go in now, nearest first; then what follows them
        // in their columns; then the same for columns that have to wait for a neighbour - until the inventory is full
        int bx = Math.max(g.ox, Math.min(g.ox + g.sx - 1, cx())), bz = Math.max(g.oz, Math.min(g.oz + g.sz - 1, cz()));
        int maxR = Math.max(g.sx, g.sz);
        long budget = 36L * 64 * 3;
        int topY = g.oy + g.sy;
        outer:
        for (int pass = 0; pass < 4 && colNext != null; pass++) {
            int from = pass < 2 ? 0 : pass == 2 ? 1 : 4, to = pass < 2 ? 1 : pass == 2 ? 4 : 10;   // cells above the column's next one
            if (slotsUsed >= capacity && pass >= 2) break;
            for (int r = 0; r <= maxR; r++) {
                for (int dz = -r; dz <= r; dz++) {
                    int z = bz + dz;
                    if (z < g.oz || z >= g.oz + g.sz) continue;
                    int step = (dz == -r || dz == r) ? 1 : 2 * r;
                    for (int dx = -r; dx <= r; dx += Math.max(1, step)) {
                        int x = bx + dx;
                        if (x < g.ox || x >= g.ox + g.sx) continue;
                        int y0 = colNext[col(x, z)];
                        if (y0 == NONE) continue;
                        if (pass == 0 && !eligible(x, z)) continue;
                        if (pass == 1 && eligible(x, z)) continue;
                        for (int y = y0 + from; y < Math.min(y0 + to, topY); y++) {
                            int i = g.idx(x, y, z);
                            if (!needs(i) || deferred.contains(i)) continue;
                            if (blocked.containsKey(i) && blocked.get(i) == surroundings(i)) continue;   // cannot go in as things stand
                            int t = g.tgt[i];
                            Item it = StateInfo.item[t];
                            if (it == Items.AIR || StateInfo.itemCount[t] == 0 || isLate(i) || !spec(t).placeable) continue;
                            int max = new ItemStack(it).getMaxStackSize();
                            int now = need.getOrDefault(it, 0);
                            int add = StateInfo.itemCount[t];
                            if (now == 0 || (now + add - 1) / max != (now - 1) / max) {
                                if (slotsUsed + 1 > capacity) continue;   // no room for another stack: this cell waits for the next trip
                                slotsUsed++;
                            }
                            need.put(it, now + add);
                            Item extra = StateInfo.contentItem(t);
                            if (extra != null) { if (!need.containsKey(extra)) slotsUsed++; need.merge(extra, 1, Integer::sum); }
                            if (--budget < 0) break outer;
                        }
                    }
                }
            }
        }
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
        if (todo.isEmpty()) return;
        log("restock: " + todo.size() + " stacks");
        // most urgent first (the order in which the needs were collected)
        List<Item> order = new ArrayList<>(need.keySet());
        todo.sort(java.util.Comparator.comparingInt(st -> order.indexOf(st.item)));
        boolean tidied = false;
        int k = 0;
        while (k < todo.size()) {
            Site.Stock first = todo.get(k);
            if (first.count <= 0) { k++; continue; }
            int ax = first.chestX, ay = g.oy, az = first.chestZ + 1;
            if (!(cx() == ax && cy() == ay && cz() == az)) {
                boolean ok = false;
                for (int attempt = 0; attempt < 2 && !ok; attempt++) {
                    // a route may not need more scaffolding than the bot still carries
                    ok = search(ax, ay, az, 200000);
                    if (ok && shortOfScaffold()) { noScaffoldNav = true; ok = search(ax, ay, az, 200000); noScaffoldNav = false; }
                    ok = ok && travel();
                }
                for (int attempt = 0; attempt < 2 && !ok && !mendOnly; attempt++) {
                    // shut in: break a way out through the build (what is broken is put back at the end)
                    nav.mine = this::mineTicks;
                    nav.effort = 20;
                    ok = search(ax, ay, az, 200000);
                    if ((ok && shortOfScaffold()) || (!ok && attempt == 1)) { noScaffoldNav = true; ok = search(ax, ay, az, 400000); noScaffoldNav = false; }
                    if (!ok && attempt == 1) { pocket = enclosed(); desperate = true; ok = search(ax, ay, az, 400000); if (!ok && pocket && Moves.LAST_BREAK) { reckless = true; try { ok = search(ax, ay, az, 400000); } finally { reckless = false; } if (ok) recklessWays++; } desperate = false; if (ok && pocket) pocketWays++; pocket = false; }
                    nav.mine = null;
                    nav.effort = 1;
                    ok = ok && travel();
                }
                if (!ok) {
                    if (verbose) {
                        StringBuilder sb2 = new StringBuilder("cannot reach chest; pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz) + " ground=" + c.onGround() + " stand=" + g.stand16(cx(), cy(), cz()) + " expanded=" + nav.expanded + " around (y+2..y-1; rows z-1..z+1, cols x-1..x+1):");
                        for (int dy = 2; dy >= -1; dy--) { sb2.append(" | y").append(dy >= 0 ? "+" : "").append(dy).append(':'); for (int dz = -1; dz <= 1; dz++) for (int dx = -1; dx <= 1; dx++) sb2.append(' ').append(StateInfo.name(g.get(cx() + dx, cy() + dy, cz() + dz))); }
                        log(sb2.toString());
                    }
                    stranded = true; strandedTick = ticks; return;
                }
                stranded = false;
            }
            settle();
            lookAt(first.chestX + 0.5, first.chestY + 0.45, first.chestZ + 0.7);
            c.clickUse();
            tick();
            if (!c.containerOpen()) { tick(); if (!c.containerOpen()) {
                if (verbose && chestLogs++ < 20) log("chest at " + rel(first.chestX, first.chestY, first.chestZ) + " did not open; hit=" + (c.lastHit() == null ? "none" : rel(c.lastHit().getBlockPos().getX(), c.lastHit().getBlockPos().getY(), c.lastHit().getBlockPos().getZ()) + " " + StateInfo.name(g.get(c.lastHit().getBlockPos().getX(), c.lastHit().getBlockPos().getY(), c.lastHit().getBlockPos().getZ()))) + " above=" + StateInfo.name(g.get(first.chestX, first.chestY + 1, first.chestZ)) + " pos=" + String.format("%.2f,%.2f,%.2f", c.x() - g.ox, c.y() - g.oy, c.z() - g.oz) + " screen=" + c.screenOpen() + " sneaking=" + c.sneaking());
                k++; continue; } }
            // make room: put back what this load does not call for, and whole stacks beyond what it calls for
            boolean deposited = false;
            for (int inv = 0; inv < 36 && !tidied; inv++) {
                ItemStack st = c.inventorySlot(inv);
                if (st.isEmpty()) continue;
                Integer want = need.get(st.getItem());
                if (want != null && count(st.getItem()) - st.getCount() < want) continue;
                int menuSlot = inv < 9 ? 81 + inv : 54 + (inv - 9);
                if (menuSlot >= c.menu().slots.size()) break;
                int before = st.getCount();
                c.clickSlot(menuSlot, 0, ContainerInput.QUICK_MOVE);
                tick();
                if (c.inventorySlot(inv).isEmpty() || c.inventorySlot(inv).getCount() != before) deposited = true;
            }
            // still crowded (the chest took nothing more)? then the tidying goes on at the next chest
            int unneeded = 0;
            for (int inv = 0; inv < 36; inv++) { ItemStack st = c.inventorySlot(inv); if (!st.isEmpty() && !need.containsKey(st.getItem())) unneeded++; }
            tidied = unneeded == 0 || freeSlots() >= 6;
            // everything on the list that this chest holds
            for (int j = k; j < todo.size(); j++) {
                Site.Stock s = todo.get(j);
                if (s.count <= 0 || s.chestX != first.chestX || s.chestY != first.chestY || s.chestZ != first.chestZ) continue;
                if (j != k && missingNow(need, s.item) <= 0) continue;
                ItemStack inSlot = c.menu().getSlot(s.slot).getItem();
                if (inSlot.isEmpty() || inSlot.getItem() != s.item) {
                    // not what the list says is here: believe the chest
                    log("chest slot " + s.slot + " at " + rel(s.chestX, s.chestY, s.chestZ) + " holds " + inSlot + ", expected " + s.item + " x" + s.count);
                    s.count = inSlot.isEmpty() || inSlot.getItem() != s.item ? 0 : inSlot.getCount();
                    continue;
                }
                int before = count(s.item);
                if (freeSlots() == 0 && order.indexOf(s.item) < 8) {
                    // no room for something urgent: the least urgent stack carried goes back into this chest
                    int victim = -1, worst = order.indexOf(s.item);
                    for (int inv = 0; inv < 36; inv++) {
                        ItemStack st = c.inventorySlot(inv);
                        if (st.isEmpty() || st.getItem() == scaffoldItem || st.getItem() == towerItem) continue;
                        int pr = order.indexOf(st.getItem());
                        if (pr < 0) pr = Integer.MAX_VALUE;
                        if (pr > worst) { worst = pr; victim = inv; }
                    }
                    if (victim >= 0) {
                        int menuSlot = victim < 9 ? 81 + victim : 54 + (victim - 9);
                        if (menuSlot < c.menu().slots.size()) { c.clickSlot(menuSlot, 0, ContainerInput.QUICK_MOVE); tick(); deposited = true; }
                    }
                }
                c.clickSlot(s.slot, 0, ContainerInput.QUICK_MOVE);
                tick();
                s.count = c.menu().getSlot(s.slot).getItem().getCount();
                if (count(s.item) == before) { if (verbose) log("taking " + s.item + " from slot " + s.slot + " did not arrive; free=" + freeSlots()); if (j == k) s.count = -s.count - 1; }
            }
            // (a stack that would not come - no room - is left for the next trip)
            if (first.count < 0) { first.count = -first.count - 1; k++; }
            if (deposited && c.menu().slots.size() >= 54) {
                // the chest's contents changed in ways the list does not know: read them off the open screen
                for (Site.Stock s : stock) if (s.chestX == first.chestX && s.chestY == first.chestY && s.chestZ == first.chestZ) s.count = 0;
                for (int slot = 0; slot < 54; slot++) {
                    ItemStack st = c.menu().getSlot(slot).getItem();
                    if (st.isEmpty()) continue;
                    Site.Stock found = null;
                    for (Site.Stock s : stock) if (s.chestX == first.chestX && s.chestY == first.chestY && s.chestZ == first.chestZ && s.slot == slot) { found = s; break; }
                    if (found == null) stock.add(new Site.Stock(st.getItem(), st.getCount(), first.chestX, first.chestY, first.chestZ, slot));
                    else { found.item = st.getItem(); found.count = st.getCount(); }
                }
            }
            c.closeScreen();
            tick();
        }
    }
}
