//! Upkeep of temporary blocks (the Java version's `Scaffolds`).
use super::nav;
use super::state::Bot;
use crate::tables::*;
use crate::fasthash::FastSet;

impl Bot {
    /// Before a block goes in: scaffolding columns near it that would be left in a closed-off pocket are taken down
    /// first. True if the bot went to do that.
    pub fn rescue_towers(&mut self, x: i32, y: i32, z: i32, tg: u16) -> bool {
        if t().walk_through[tg as usize] || self.cleaning || self.rescuing || !self.g.in_box(x, y, z) { return false; }
        let mut found: i32 = -1;
        let i = self.g.idx(x, y, z);
        let old = self.g.cur[i];
        self.g.cur[i] = tg;
        let keep = self.nav.mine;
        self.nav.mine = false;
        self.towers_off = true;
        'outer: for dz in -4..=4 { for dx in -4..=4 {
            let (qx, qz) = (x + dx, z + dz);
            if !self.g.inside(qx, y, qz) || self.scaffold_in_column[self.col(qx, qz)] == 0 { continue; }
            for qy in (self.g.y0 + 1).max(y - 12)..=y + 2 {
                // (above the grid there is nothing; the original's set lookup simply misses)
                if !self.g.inside(qx, qy, qz) { continue; }
                let q = self.g.idx(qx, qy, qz) as i32;
                if !self.scaffold.contains(q) || !self.g.is_tower(qx, qy, qz) { continue; }
                if self.scaffold.contains(self.g.idx(qx, qy - 1, qz) as i32) && self.g.is_tower(qx, qy - 1, qz) { continue; }
                if self.body_in(qx, qy, qz) || (self.cx() == qx && self.cz() == qz) { continue; }
                if self.rescue_failed.contains(&q) { continue; }
                self.towers_off = false;
                let open = self.way_out(qx, qy, qz, false);
                self.towers_off = true;
                if !open { found = q; break 'outer; }
            }
        } }
        self.towers_off = false;
        self.nav.mine = keep;
        self.g.cur[i] = old;
        if found < 0 { return false; }
        self.rescuing = true;
        if self.remove_scaffold(found as usize) { self.towers_rescued += 1; } else { self.rescue_failed.insert(found); }
        self.rescuing = false;
        true
    }

    pub fn undo_rope(&mut self) {
        self.towers_off = true;
        self.undo_rope_impl();
        self.towers_off = false;
    }

    fn undo_rope_impl(&mut self) {
        self.end_lean();
        while !self.rope.is_empty() {
            let e = self.rope[self.rope.len() - 1];
            let cell = e[7];
            if !self.scaffold.contains(cell) { self.rope.pop(); continue; }
            let (bx, by, bz) = (self.g.x_of(cell as usize), self.g.y_of(cell as usize), self.g.z_of(cell as usize));
            let mut there = self.cx() == e[4] && self.cy() == e[5] && self.cz() == e[6] && self.c.on_ground();
            if !there {
                self.nav.start_h = if self.c.on_ground() { (super::jround((self.c.y() - self.cy() as f64) * 16.0) as i32).clamp(0, 15) } else { -1 };
                there = self.nav_search(self.cx(), self.cy(), self.cz(), e[4], e[5], e[6], 3000, false) && self.travel();
            }
            let mut ok = false;
            if there {
                if e[0] == nav::PILLAR as i32 {
                    ok = self.break_underfoot(e[4], e[5], e[6], bx, by, bz);
                } else {
                    self.one = [e[1], e[2], e[3]];
                    if self.g.stand16(e[1], e[2], e[3]) >= 0 && self.follow_one() {
                        self.settle();
                        let eye = self.c.eye();
                        ok = self.find_break(bx, by, bz, eye[0], eye[1], eye[2]) && { let cl = self.click.clone(); self.break_block(bx, by, bz, cl.ax, cl.ay, cl.az, 80) };
                    }
                    if !ok && self.scaffold.contains(cell) && self.cx() == bx && self.cy() == by + 1 && self.cz() == bz && self.c.on_ground() { ok = self.break_underfoot(bx, by + 1, bz, bx, by, bz); }
                }
            }
            if !ok && self.scaffold.contains(cell) && self.c.on_ground() {
                self.settle();
                let eye = self.c.eye();
                ok = self.find_break(bx, by, bz, eye[0], eye[1], eye[2]) && { let cl = self.click.clone(); self.break_block(bx, by, bz, cl.ax, cl.ay, cl.az, 80) };
                if ok { let mut n = 0; while n < 60 && !self.c.on_ground() { self.tick(); n += 1; } self.settle(); }
            }
            if !ok && self.scaffold.contains(cell) { break; }
            self.rope_undone += 1;
            self.rope.pop();
        }
        if !self.rope.is_empty() { self.rope_orphaned += self.rope.len() as i64; self.rope.clear(); }
    }

    pub fn break_underfoot(&mut self, sx: i32, sy: i32, sz: i32, bx: i32, by: i32, bz: i32) -> bool {
        let ok = self.centre(sx, sy, sz) && self.break_block(bx, by, bz, bx as f64 + 0.5, by as f64 + 1.0, bz as f64 + 0.5, 80);
        let mut n = 0;
        while n < 200 && !self.c.on_ground() { self.tick(); n += 1; }
        self.settle();
        ok
    }

    /// Before a block goes in: take away scaffolding in the same column that it would roof over. False if some stays.
    pub fn clear_below(&mut self, x: i32, y: i32, z: i32) -> bool {
        if self.scaffold_in_column[self.col(x, z)] == 0 { return true; }
        let mut yy = y - 1;
        while yy >= self.g.y0 {
            let i = self.g.idx(x, yy, z);
            if self.scaffold.contains(i as i32) { if self.g.is_tower(x, yy, z) { return true; } if !self.remove_scaffold(i) { return false; } }
            else if self.g.cur[i] != 0 { return true; }
            yy -= 1;
        }
        true
    }

    /// Is there a loose scaffold block (not a column) in the open space under the cell?
    pub fn loose_below(&self, x: i32, y: i32, z: i32) -> bool {
        let mut yy = y - 1;
        while yy >= self.g.y0 {
            let i = self.g.idx(x, yy, z);
            if self.scaffold.contains(i as i32) { return !self.g.is_tower(x, yy, z); }
            if self.g.cur[i] != 0 { return false; }
            yy -= 1;
        }
        false
    }

    pub fn remove_scaffold(&mut self, i: usize) -> bool {
        self.check_scaffold();
        if t().scaffolding[self.g.cur[i] as usize] { return self.remove_tower(i); }
        let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
        for _attempt in 0..3 {
            if !self.scaffold.contains(i as i32) { return true; }
            if !self.c.on_ground() { self.settle(); }
            let e = self.c.eye();
            if self.find_break(x, y, z, e[0], e[1], e[2]) {
                let cl = self.click.clone();
                let bt = 80.max(self.break_ticks(self.g.get(x, y, z)));
                if self.break_block(x, y, z, cl.ax, cl.ay, cl.az, bt) { self.settle(); return true; }
            }
            let st = self.plan_stance(x, y, z, None, true);
            let Some(st) = st else {
                if !self.force_mine || !self.tunnel_to(x, y, z) { return false; }
                continue;
            };
            if !self.go_to(&st) { self.bad_stand.insert(self.g.idx_raw(st.x, st.y, st.z)); self.replans += 1; }
        }
        !self.scaffold.contains(i as i32)
    }

    /// Takes down a scaffolding column: breaks its lowest block, or failing that the given block.
    fn remove_tower(&mut self, i: usize) -> bool {
        let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
        let mut low = y;
        while self.g.inside(x, low - 1, z) && self.scaffold.contains(self.g.idx(x, low - 1, z) as i32) && self.g.is_tower(x, low - 1, z) { low -= 1; }
        if self.break_tower_block(x, low, z) && !self.scaffold.contains(i as i32) { return true; }
        if !self.scaffold.contains(i as i32) { return true; }
        if low != y { self.break_tower_block(x, y, z); }
        !self.scaffold.contains(i as i32)
    }

    fn break_tower_block(&mut self, x: i32, y: i32, z: i32) -> bool {
        let i = self.g.idx(x, y, z) as i32;
        for _attempt in 0..3 {
            if !self.scaffold.contains(i) { return true; }
            if !self.c.on_ground() { self.settle(); }
            let e = self.c.eye();
            if self.find_break(x, y, z, e[0], e[1], e[2]) {
                let mut top = y;
                while self.g.is_tower(x, top + 1, z) { top += 1; }
                let cl = self.click.clone();
                if self.break_block(x, y, z, cl.ax, cl.ay, cl.az, 40) {
                    let mut n = 0;
                    while n < 30 + 3 * (top - y) && self.g.is_tower(x, top, z) && top > y { self.tick(); n += 1; }
                    if !self.c.on_ground() { self.settle(); }
                    return true;
                }
            }
            let st = self.plan_stance(x, y, z, None, true);
            let Some(st) = st else {
                if !self.force_mine || !self.tunnel_to(x, y, z) { return false; }
                continue;
            };
            if !self.go_to(&st) { self.bad_stand.insert(self.g.idx_raw(st.x, st.y, st.z)); self.replans += 1; }
        }
        !self.scaffold.contains(i)
    }

    pub fn cleanup(&mut self) {
        let tb = t();
        self.bad_stand.clear();
        self.comes_back.clear();
        let mut hopeless: FastSet<i32> = FastSet::default();
        let mut guard = 0;
        let mut least = self.scaffold.len();
        let mut since_least = 0usize;
        self.cleaning = true;
        let layer = (self.g.nx * self.g.nz) as i32;
        while self.scaffold.len() > hopeless.len() && guard < 100000 {
            guard += 1;
            if self.scaffold.len() < least { least = self.scaffold.len(); since_least = 0; }
            else { since_least += 1; if since_least > 24 + least / 4 { break; } }
            let mut best: i32 = -1;
            let mut best_y = i32::MIN;
            let mut best_d = i64::MAX;
            let (bx, bz) = (self.cx(), self.cz());
            let mut best_tower = true;
            for i in self.scaffold.keys() {
                if hopeless.contains(&i) { continue; }
                if !self.force_mine && self.comes_back.get(i) >= 3 { continue; }
                let iu = i as usize;
                let tw = tb.scaffolding[self.g.cur[iu] as usize];
                if tw && !best_tower { continue; }
                if tw && i - layer >= 0 && self.scaffold.contains(i - layer) && tb.scaffolding[self.g.cur[(i - layer) as usize] as usize] { continue; }
                let y = self.g.y_of(iu);
                let d = ((self.g.x_of(iu) - bx).abs() + (self.g.z_of(iu) - bz).abs()) as i64;
                if (best_tower && !tw) || y > best_y || (y == best_y && d < best_d) { best = i; best_y = y; best_d = d; best_tower = tw; }
            }
            if best < 0 { break; }
            let before = self.scaffold.len();
            let ok_remove = self.remove_scaffold(best as usize);
            if ok_remove { self.comes_back.add_to(best, 1); }
            if ok_remove { self.sweep_scaffold(); }
            self.undo_rope();
            if !ok_remove { hopeless.insert(best); self.bad_stand.clear(); }
            else if self.scaffold.len() < before { hopeless.clear(); }
            else if self.scaffold.len() >= before { hopeless.insert(best); }
        }
        self.cleaning = false;
    }

    /// Breaks every scaffold block within reach of where the bot stands, except the one it stands on.
    fn sweep_scaffold(&mut self) {
        let mut again = true;
        while again {
            again = false;
            if !self.c.on_ground() { self.settle(); }
            let (x, y, z) = (self.cx(), self.cy(), self.cz());
            let e = self.c.eye();
            'scan: for dy in (-4..=5).rev() { for dz in -4..=4 { for dx in -4..=4 {
                let (tx, ty, tz) = (x + dx, y + dy, z + dz);
                if !self.g.inside(tx, ty, tz) || (dx == 0 && dz == 0 && dy == -1) { continue; }
                let si = self.g.idx(tx, ty, tz) as i32;
                if !self.scaffold.contains(si) { continue; }
                if self.rope.iter().any(|r| r[7] == si) { continue; }
                if self.find_break(tx, ty, tz, e[0], e[1], e[2]) {
                    let cl = self.click.clone();
                    let bt = 80.max(self.break_ticks(self.g.get(tx, ty, tz)));
                    if self.break_block(tx, ty, tz, cl.ax, cl.ay, cl.az, bt) { again = true; break 'scan; }
                }
            } } }
        }
    }
}
