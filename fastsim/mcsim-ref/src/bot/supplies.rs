//! Trips to the supply chests (the Java version's `Supplies`).
use super::state::{content_item, Bot, Stock};
use super::*;
use crate::sim::CLICK_QUICK_MOVE;
use crate::tables::*;

/// Insertion-ordered item -> count map (LinkedHashMap<Item, Integer>).
#[derive(Default)]
struct Need(Vec<(u16, i32)>);
impl Need {
    fn get(&self, it: u16) -> Option<i32> { self.0.iter().find(|e| e.0 == it).map(|e| e.1) }
    fn get_or0(&self, it: u16) -> i32 { self.get(it).unwrap_or(0) }
    fn contains(&self, it: u16) -> bool { self.0.iter().any(|e| e.0 == it) }
    fn put(&mut self, it: u16, v: i32) { if let Some(e) = self.0.iter_mut().find(|e| e.0 == it) { e.1 = v; } else { self.0.push((it, v)); } }
    fn merge(&mut self, it: u16, v: i32) { if let Some(e) = self.0.iter_mut().find(|e| e.0 == it) { e.1 += v; } else { self.0.push((it, v)); } }
    fn index_of(&self, it: u16) -> i32 { self.0.iter().position(|e| e.0 == it).map(|k| k as i32).unwrap_or(-1) }
}

impl Bot {
    /// Goes to the chests and loads up for the cells still to do, lowest layers first.
    pub fn restock(&mut self, from_layer: i32) {
        self.restocking = true;
        self.restock0(from_layer);
        self.restocking = false;
        if self.events.is_some() { let mut kinds = 0; let mut items = 0; for iv in 0..36 { let st = self.c.pl.inv[iv]; if st.count > 0 { kinds += 1; items += st.count as i32; } } let l = format!("{} TA stacks={} items={} free={} chests={} clicks={}\n", self.ticks, kinds, items, self.free_slots(), self.fs_chests, self.fs_clicks); self.fs_chests = 0; self.fs_clicks = 0; self.ev_write(&l); }
    }

    fn restock0(&mut self, _from_layer: i32) {
        let tb = t();
        self.loose_fails.clear();
        self.loose_blocked.clear();
        self.nav.blocked_edges.clear();
        self.end_lean();
        if !self.rope.is_empty() { self.undo_rope(); }
        self.trips += 1;
        if self.events.is_some() { let l = format!("{} T {}\n", self.ticks, self.trips); self.ev_write(&l); }
        let mut need = Need::default();
        let mut slots_used = 0i32;
        if self.must_fetch != 0 { need.put(self.must_fetch, tb.items[self.must_fetch as usize].stack as i32); slots_used += 1; }
        let want_loose = (64 * 4).min(64 + 64 * self.scaffold_trips);
        let want_tower = (64 * 6).min(128 + 64 * self.tower_trips);
        need.merge(self.scaffold_item, want_loose);
        if self.count(self.scaffold_item) < 16 { self.scaffold_trips += 1; }
        need.merge(self.tower_item, want_tower);
        if self.count(self.tower_item) < 16 { self.tower_trips += 1; }
        let reserved = (want_loose + 63) / 64 + (want_tower + 63) / 64;
        for cell in self.deferred.keys() {
            let cu = cell as usize;
            if !self.main_done || !self.needs(cu) || slots_used >= 16 { continue; }
            if !self.late_too && self.is_late(cu) { continue; }
            let tg = self.g.tgt[cu] as usize;
            let it = tb.item[tg];
            if it == 0 || tb.item_count[tg] == 0 { continue; }
            let now = need.get_or0(it);
            if now == 0 { slots_used += 1; }
            need.put(it, now + tb.item_count[tg] as i32);
        }
        let capacity = 35 - reserved;
        let bx = self.g.ox.max((self.g.ox + self.g.sx - 1).min(self.cx()));
        let bz = self.g.oz.max((self.g.oz + self.g.sz - 1).min(self.cz()));
        let max_r = self.g.sx.max(self.g.sz);
        let mut budget: i64 = 36 * 64 * 3;
        let top_y = self.g.oy + self.g.sy;
        if self.col_next_set {
            'outer: for pass in 0..4 {
                let from = if pass < 2 { 0 } else if pass == 2 { 1 } else { 4 };
                let to = if pass < 2 { 1 } else if pass == 2 { 4 } else { 10 };
                if slots_used >= capacity && pass >= 2 { break; }
                for r in 0..=max_r {
                    for dz in -r..=r {
                        let z = bz + dz;
                        if z < self.g.oz || z >= self.g.oz + self.g.sz { continue; }
                        let step = if dz == -r || dz == r { 1 } else { 2 * r };
                        let mut dx = -r;
                        while dx <= r {
                            let x = bx + dx;
                            dx += 1.max(step);
                            if x < self.g.ox || x >= self.g.ox + self.g.sx { continue; }
                            let y0 = self.col_next[self.col(x, z)];
                            if y0 == NONE { continue; }
                            if pass == 0 && !self.eligible(x, z) { continue; }
                            if pass == 1 && self.eligible(x, z) { continue; }
                            let mut y = y0 + from;
                            while y < (y0 + to).min(top_y) {
                                let yy = y;
                                y += 1;
                                let i = self.g.idx(x, yy, z);
                                if !self.needs(i) || self.deferred.contains(i as i32) { continue; }
                                if self.blocked.contains(i as i32) && self.blocked.get(i as i32) == self.surroundings(i) { continue; }
                                let tg = self.g.tgt[i];
                                let it = tb.item[tg as usize];
                                if it == 0 || tb.item_count[tg as usize] == 0 || self.is_late(i) || !self.spec(tg).placeable { continue; }
                                let max = tb.items[it as usize].stack as i32;
                                let now = need.get_or0(it);
                                let add = tb.item_count[tg as usize] as i32;
                                // (Java integer division truncates toward zero: (now - 1) / max is 0 for now = 0)
                                if now == 0 || (now + add - 1) / max != (now - 1) / max {
                                    if slots_used + 1 > capacity { continue; }
                                    slots_used += 1;
                                }
                                need.put(it, now + add);
                                let extra = content_item(tg);
                                if extra != 0 { if !need.contains(extra) { slots_used += 1; } need.merge(extra, 1); }
                                budget -= 1;
                                if budget < 0 { break 'outer; }
                            }
                        }
                    }
                }
            }
        }
        let mut missing = Need::default();
        for &(it, v) in &need.0 { let m = v - self.count(it); if m > 0 { missing.put(it, m); } }
        if self.events.is_some() { let tot: i32 = need.0.iter().map(|e| e.1).sum(); let l = format!("{} TP kinds={} slotsUsed={} capacity={} planned={} missingKinds={} freeBefore={}\n", self.ticks, need.0.len(), slots_used, capacity, tot, missing.0.len(), self.free_slots()); self.ev_write(&l); }
        if missing.0.is_empty() { return; }
        let mut todo: Vec<usize> = Vec::new();
        for (k, s) in self.stock.iter().enumerate() {
            if s.count <= 0 { continue; }
            let Some(m) = missing.get(s.item) else { continue };
            if m <= 0 { continue; }
            todo.push(k);
            missing.put(s.item, m - s.count);
        }
        if todo.is_empty() { return; }
        todo.sort_by_key(|&k| need.index_of(self.stock[k].item));
        let mut tidied = false;
        let mut k = 0usize;
        while k < todo.len() {
            let first = self.stock[todo[k]].clone();
            if first.count <= 0 { k += 1; continue; }
            let (ax, ay, az) = (first.chest_x, self.g.oy, first.chest_z + 1);
            if !(self.cx() == ax && self.cy() == ay && self.cz() == az) {
                let mut ok = false;
                let mut attempt = 0;
                while attempt < 2 && !ok {
                    ok = self.search(ax, ay, az, 200000);
                    if ok && self.short_of_scaffold() { self.no_scaffold_nav = true; ok = self.search(ax, ay, az, 200000); self.no_scaffold_nav = false; }
                    ok = ok && self.travel();
                    attempt += 1;
                }
                let mut attempt = 0;
                while attempt < 2 && !ok && !self.mend_only {
                    self.nav.mine = true;
                    self.nav.effort = 20;
                    ok = self.search(ax, ay, az, 200000);
                    if (ok && self.short_of_scaffold()) || (!ok && attempt == 1) { self.no_scaffold_nav = true; ok = self.search(ax, ay, az, 400000); self.no_scaffold_nav = false; }
                    if !ok && attempt == 1 { self.pocket = self.enclosed(); self.desperate = true; ok = self.search(ax, ay, az, 400000); self.desperate = false; if ok && self.pocket { self.pocket_ways += 1; } self.pocket = false; }
                    self.nav.mine = false;
                    self.nav.effort = 1;
                    ok = ok && self.travel();
                    attempt += 1;
                }
                if !ok { self.stranded = true; self.stranded_tick = self.ticks; return; }
                self.stranded = false;
            }
            self.settle();
            self.look_at(first.chest_x as f64 + 0.5, first.chest_y as f64 + 0.45, first.chest_z as f64 + 0.7);
            self.c.click_use();
            self.tick();
            if !self.c.container_open() { self.tick(); if !self.c.container_open() { k += 1; continue; } }
            self.fs_chests += 1;
            // make room: put back what this load does not call for, and whole stacks beyond what it calls for
            let mut deposited = false;
            let mut inv = 0usize;
            while inv < 36 && !tidied {
                let iv = inv;
                inv += 1;
                let st = self.c.pl.inv[iv];
                if st.count == 0 { continue; }
                if let Some(want) = need.get(st.item) { if self.count(st.item) - (st.count as i32) < want { continue; } }
                let menu_slot = if iv < 9 { 81 + iv } else { 54 + (iv - 9) };
                if menu_slot >= self.c.menu_slots() { break; }
                let before = st.count;
                self.fs_clicks += 1; self.c.click_slot(menu_slot as i32, 0, CLICK_QUICK_MOVE);
                self.tick();
                if self.c.pl.inv[iv].count == 0 || self.c.pl.inv[iv].count != before { deposited = true; }
            }
            let mut unneeded = 0;
            for iv in 0..36 { let st = self.c.pl.inv[iv]; if st.count > 0 && !need.contains(st.item) { unneeded += 1; } }
            tidied = unneeded == 0 || self.free_slots() >= 6;
            // everything on the list that this chest holds
            for j in k..todo.len() {
                let sj = todo[j];
                let s: Stock = self.stock[sj].clone();
                if s.count <= 0 || s.chest_x != first.chest_x || s.chest_y != first.chest_y || s.chest_z != first.chest_z { continue; }
                if j != k && need.get(s.item).map(|n| n - self.count(s.item)).unwrap_or(0) <= 0 { continue; }
                let in_slot = self.c.menu_slot(s.slot);
                if in_slot.count == 0 || in_slot.item != s.item {
                    self.stock[sj].count = 0;
                    continue;
                }
                let before = self.count(s.item);
                if self.free_slots() == 0 && need.index_of(s.item) < 8 {
                    let mut victim: i32 = -1;
                    let mut worst = need.index_of(s.item);
                    for iv in 0..36 {
                        let st = self.c.pl.inv[iv];
                        if st.count == 0 || st.item == self.scaffold_item || st.item == self.tower_item { continue; }
                        let mut pr = need.index_of(st.item);
                        if pr < 0 { pr = i32::MAX; }
                        if pr > worst { worst = pr; victim = iv as i32; }
                    }
                    if victim >= 0 {
                        let v = victim as usize;
                        let menu_slot = if v < 9 { 81 + v } else { 54 + (v - 9) };
                        if menu_slot < self.c.menu_slots() { self.fs_clicks += 1; self.c.click_slot(menu_slot as i32, 0, CLICK_QUICK_MOVE); self.tick(); deposited = true; }
                    }
                }
                self.fs_clicks += 1; self.c.click_slot(s.slot as i32, 0, CLICK_QUICK_MOVE);
                self.tick();
                self.stock[sj].count = self.c.menu_slot(s.slot).count as i32;
                if self.count(s.item) == before && j == k { self.stock[sj].count = -self.stock[sj].count - 1; }
            }
            // (a stack that would not come - no room - is left for the next trip)
            let fk = todo[k];
            if self.stock[fk].count < 0 { self.stock[fk].count = -self.stock[fk].count - 1; k += 1; }
            if deposited && self.c.menu_slots() >= 54 {
                for s in self.stock.iter_mut() { if s.chest_x == first.chest_x && s.chest_y == first.chest_y && s.chest_z == first.chest_z { s.count = 0; } }
                for slot in 0..54 {
                    let st = self.c.menu_slot(slot);
                    if st.count == 0 { continue; }
                    let found = self.stock.iter().position(|s| s.chest_x == first.chest_x && s.chest_y == first.chest_y && s.chest_z == first.chest_z && s.slot == slot);
                    match found {
                        None => self.stock.push(Stock { item: st.item, count: st.count as i32, chest_x: first.chest_x, chest_y: first.chest_y, chest_z: first.chest_z, slot }),
                        Some(f) => { self.stock[f].item = st.item; self.stock[f].count = st.count as i32; }
                    }
                }
            }
            self.c.close_screen();
            self.tick();
        }
    }
}
