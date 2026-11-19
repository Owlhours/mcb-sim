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

fn triplog() -> bool { static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new(); *V.get_or_init(|| std::env::var_os("MCSIM_TRIPLOG").is_some()) }

impl Bot {
    fn inv_line(&self) -> String {
        let mut m: Vec<(u16, i32)> = Vec::new();
        for iv in 0..36 { let st = self.c.pl.inv[iv]; if st.count == 0 { continue; } if let Some(e) = m.iter_mut().find(|e| e.0 == st.item) { e.1 += st.count as i32; } else { m.push((st.item, st.count as i32)); } }
        format!("free={} {}", self.free_slots(), m.iter().map(|e| format!("{}:{}", t().items[e.0 as usize].name, e.1)).collect::<Vec<_>>().join(","))
    }
    /// Goes to the chests and loads up for the cells still to do, lowest layers first.
    pub fn restock(&mut self, from_layer: i32) { let _ag = super::nav::act("restock trip");
        self.restocking = true;
        // (diagnostic MCSIM_TRIPLOG: one line per trip - where from, for what, what came of it; no change to the bot)
        let lg = triplog();
        let (t0, at0, inv0) = (self.ticks, self.rel(self.cx(), self.cy(), self.cz()), if lg { self.inv_line() } else { String::new() });
        let (mf, st0) = (self.must_fetch, self.stranded);
        self.restock0(from_layer);
        if lg { eprintln!("TRIP {} t={} took={} from={} act={} must={} depot={} stranded={}->{} at={} | {} => {}", self.trips, t0, self.ticks - t0, at0, super::nav::act_now(), if mf != 0 { t().items[mf as usize].name.clone() } else { "-".into() }, if mf != 0 { self.in_depot(mf) } else { -1 }, st0, self.stranded, self.rel(self.cx(), self.cy(), self.cz()), inv0, self.inv_line()); }
        self.restocking = false;
    }

    /// (candidate "pots") How many of this plant the pots still to fill take (one stack at most).
    fn plants_wanted(&self, plant: u16) -> i32 {
        let mut n = 0i32;
        for i in 0..self.g.tgt.len() { let tg = self.g.tgt[i]; if tg != 0 && self.g.cur[i] != tg && content_item(tg) == plant && !self.matches(self.g.cur[i], tg) { n += 1; } }
        n.clamp(1, t().items[plant as usize].stack as i32)
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
        if self.must_fetch != 0 { need.put(self.must_fetch, if self.fetch_few { self.plants_wanted(self.must_fetch) } else { tb.items[self.must_fetch as usize].stack as i32 }); slots_used += 1; }
        if self.must_fetch_extra != 0 { need.merge(self.must_fetch_extra, self.plants_wanted(self.must_fetch_extra)); slots_used += 1; }
        let pots = crate::cand::on("pots") && crate::ckpt::armed(self.ticks);
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
            // (candidate "pots") the put-off pots' plants are on the list too
            if pots { let extra = content_item(tg as u16); if extra != 0 { if !need.contains(extra) { slots_used += 1; } need.merge(extra, 1); } }
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
        // (candidate "trips", rule b) after the main pass the list is the sum of ALL put-off cells: with thousands of
        // cells of two or three materials the first one on the list took every slot (the others were put back to make
        // room, stack for stack, at every chest slot of it), and the next cell of another material sent the bot back to
        // swap the whole load again. Each material gets an equal share of the slots instead.
        if self.main_done && (crate::cand::on("trips") || crate::cand::on("trips_b")) && crate::ckpt::armed(self.ticks) {
            let kinds = need.0.iter().filter(|e| e.0 != self.scaffold_item && e.0 != self.tower_item).count() as i32;
            if kinds > 0 {
                let share = (capacity / kinds).max(1);
                let (si, ti) = (self.scaffold_item, self.tower_item);
                for e in need.0.iter_mut() { if e.0 != si && e.0 != ti { e.1 = e.1.min(share * tb.items[e.0 as usize].stack as i32); } }
            }
        }
        let mut missing = Need::default();
        for &(it, v) in &need.0 { let m = v - self.count(it); if m > 0 { missing.put(it, m); } }
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
        if triplog() { eprintln!("  NEED t={} main_done={} deferred={} need=[{}] todo={}", self.ticks, self.main_done, self.deferred.len(), need.0.iter().map(|e| format!("{}:{}", tb.items[e.0 as usize].name, e.1)).collect::<Vec<_>>().join(","), todo.len()); }
        todo.sort_by_key(|&k| need.index_of(self.stock[k].item));
        // (candidate `stockswap`) what each slot on the list was listed for. Making room puts things back into the
        // chest, and the chest's slots are then read again: a slot the bot has just emptied may now hold what it put
        // back. Taking that again was a loop - with a full inventory of shulker boxes (one to a slot) the bot put a
        // stack of composters back to take the next box, found the composters in the box's slot, took them, and put
        // them back again, for the remaining 3.4M ticks of the build. A slot that now holds something else is passed.
        let swap = crate::cand::on("stockswap") && crate::ckpt::armed(self.ticks);
        let listed: Vec<u16> = if swap { todo.iter().map(|&j| self.stock[j].item).collect() } else { Vec::new() };
        let mut tidied = false;
        let mut k = 0usize;
        while k < todo.len() {
            let first = self.stock[todo[k]].clone();
            if first.count <= 0 { k += 1; continue; }
            if swap && first.item != listed[k] { k += 1; continue; }
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
                    if !ok && attempt == 1 { self.pocket = self.enclosed(); if super::moves::shutstat() { eprintln!("shutstat t={} at={} expanded={} exhausted={} pocket={}", self.ticks, self.rel(self.cx(), self.cy(), self.cz()), self.nav.expanded, self.nav.exhausted, self.pocket); } self.desperate = true; ok = self.search(ax, ay, az, 400000); if !ok && self.pocket && crate::rev::rev().last_break && crate::ckpt::armed(self.ticks) { self.reckless = true; ok = self.search(ax, ay, az, 400000); self.reckless = false; if ok { self.reckless_ways += 1; } } self.desperate = false; if ok && self.pocket { self.pocket_ways += 1; } self.pocket = false; }
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
                self.c.click_slot(menu_slot as i32, 0, CLICK_QUICK_MOVE);
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
                if swap && s.item != listed[j] { continue; }
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
                        if menu_slot < self.c.menu_slots() { self.c.click_slot(menu_slot as i32, 0, CLICK_QUICK_MOVE); self.tick(); deposited = true; }
                    }
                }
                self.c.click_slot(s.slot as i32, 0, CLICK_QUICK_MOVE);
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
