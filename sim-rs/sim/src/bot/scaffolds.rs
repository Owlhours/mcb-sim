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
        self.g.put(i, tg);
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
                let usable_was = self.witness_usable;
                self.witness_usable = self.exit_valid && !self.exit_cells.contains(&(i as i32));
                let open = self.way_out(qx, qy, qz, false);
                self.witness_usable = usable_was;
                self.towers_off = true;
                if !open { found = q; break 'outer; }
            }
        } }
        self.towers_off = false;
        self.nav.mine = keep;
        self.g.put(i, old);
        if found < 0 { return false; }
        if self.watch.is_some() { self.wlog(&format!("rescueTowers: column at {} would be closed in by {}", self.rel(self.g.x_of(found as usize), self.g.y_of(found as usize), self.g.z_of(found as usize)), self.rel(x, y, z))); }
        self.rescuing = true;
        if self.remove_scaffold(found as usize) { self.towers_rescued += 1; } else { self.rescue_failed.insert(found); }
        self.rescuing = false;
        true
    }

    pub fn undo_rope(&mut self) { let _ag = super::nav::act("taking own pillar down");
        self.towers_off = true;
        let hd_was = super::hungdiag::set_ctx("undo");
        self.undo_rope_impl();
        super::hungdiag::set_ctx(hd_was);
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
            // (revision "ropefar"; MCSIM_ROPEFAR=<blocks>, 6) Pieces of the bot's own way that lie this far behind it are not walked
            // back for while it builds - in a build that is behind its pace (`behind_pace`): they stand as ordinary
            // scaffolding and go with the clean-up, or when the bot next passes. (Walking back for them was up to a
            // third of all ticks of builds that run out of time. Applied to every build it cost as many as it won.)
            if !there && !self.cleaning && !self.main_done && crate::rev::rev().rope_far && crate::ckpt::armed(self.ticks) && self.behind_pace() {
                let far = rope_far();
                if (self.cx() - e[4]).abs() + (self.cy() - e[5]).abs() + (self.cz() - e[6]).abs() >= far {
                    self.rope_orphaned += self.rope.len() as i64;
                    self.hd_rope_lost(1);
                    self.rope.clear();
                    break;
                }
            }
            if !there {
                self.nav.start_h = if self.c.on_ground() { (super::jround((self.c.y() - self.cy() as f64) * 16.0) as i32).clamp(0, 15) } else { -1 };
                let _ag = super::nav::act("own pillar: getting back to it");
                there = self.nav_search(self.cx(), self.cy(), self.cz(), e[4], e[5], e[6], 3000, false) && self.travel();
                if !there && self.cleaning && !self.no_scaffold_nav && self.c.on_ground() && crate::rev::rev().hung_back && crate::ckpt::armed(self.ticks)
                    && hung_worth(self.rope.len() as i64, (e[5] - self.g.oy).max(1) as i64) && self.behind_pace() {
                    there = self.hung_way_back(e[4], e[5], e[6]);
                }
            }
            let mut ok = false;
            if there {
                if e[0] == nav::PILLAR as i32 {
                    let _ag = super::nav::act("own pillar: breaking underfoot");
                    ok = self.break_underfoot(e[4], e[5], e[6], bx, by, bz);
                } else {
                    let _ag = super::nav::act("own bridge: stepping back and breaking");
                    self.one = [e[1], e[2], e[3]];
                    if super::hungdiag::on() && self.g.stand16(e[1], e[2], e[3]) < 0 {
                        let fl = self.g.idx(e[1], e[2] - 1, e[3]) as i32;
                        let by = super::hungdiag::gone_by(fl);
                        let k: &'static str = match (self.cleaning, by) { (true, "sweep") => "floorless_cleanup_sweep", (true, "target") => "floorless_cleanup_target", (true, "undo") => "floorless_cleanup_undo", (true, "never") => "floorless_cleanup_never", (true, _) => "floorless_cleanup_other",
                            (false, "sweep") => "floorless_build_sweep", (false, "target") => "floorless_build_target", (false, "undo") => "floorless_build_undo", (false, "never") => "floorless_build_never", (false, _) => "floorless_build_other" };
                        super::hungdiag::count(k, 1);
                        super::hungdiag::count(if self.cleaning { "floorless_cleanup_rope_after" } else { "floorless_build_rope_after" }, self.rope.len() as i64 - 1);
                    }
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
            if !ok && self.scaffold.contains(cell) {
                if super::hungdiag::on() {
                    let k = match (self.cleaning, self.main_done, e[0] == nav::PILLAR as i32, there) {
                        (true, _, true, true) => "fail_cleanup_pillar_there", (true, _, true, false) => "fail_cleanup_pillar_noway",
                        (true, _, false, true) => "fail_cleanup_bridge_there", (true, _, false, false) => "fail_cleanup_bridge_noway",
                        (false, false, true, true) => "fail_main_pillar_there", (false, false, true, false) => "fail_main_pillar_noway",
                        (false, false, false, true) => "fail_main_bridge_there", (false, false, false, false) => "fail_main_bridge_noway",
                        (false, true, true, _) => "fail_after_pillar", (false, true, false, _) => "fail_after_bridge" };
                    super::hungdiag::count(k, 1);
                    super::hungdiag::count(if self.cleaning { "fail_cleanup_pieces" } else { "fail_noncleanup_pieces" }, self.rope.len() as i64);
                    if self.verbose { let r0 = self.rope[0]; self.log(&format!("HD rope lost: {k} piece {} stand {} rope {} from {} on_ground={}", self.rel(bx, by, bz), self.rel(e[4], e[5], e[6]), self.rope.len(), self.rel(r0[4], r0[5], r0[6]), self.c.on_ground())); }
                }
                break;
            }
            self.rope_undone += 1;
            self.rope.pop();
        }
        if !self.rope.is_empty() { self.rope_orphaned += self.rope.len() as i64; self.hd_rope_lost(2); self.rope.clear(); }
    }

    /// (candidate `hung`, MCSIM_CAND=hung; in the clean-up of a build behind its pace) The way back to the top of the
    /// bot's own way (the "rope") is lost - it dropped down from it, or what it stood on went - and no plain walk leads
    /// back up. Until now the whole rope was then given up, and it hung in the air for the clean-up to come back for
    /// with new scaffolding of its own: in the out-of-time huge builds that was most of what the clean-up put up
    /// (25,900 of 43,000 pieces in one, a rope of 200 lost at a time in another). Here the bot climbs back to it, with
    /// a climbing column or loose blocks as the search finds best (MCSIM_HUNGWAY expansions, 6000), and takes the rope
    /// down from the top as usual. What it put up for the climb back is not part of the rope: a column is taken down
    /// behind it as it steps off (as in the clean-up's other walks); loose blocks stand on what the bot stood on.
    fn hung_way_back(&mut self, x: i32, y: i32, z: i32) -> bool {
        let n0 = self.rope.len();
        let (tw, on) = (self.towers_off, self.nav.towers_on);
        self.towers_off = false;
        self.nav.towers_on = self.have_tower;
        self.nav.start_h = (super::jround((self.c.y() - self.cy() as f64) * 16.0) as i32).clamp(0, 15);
        let budget = crate::cand::num("MCSIM_HUNGWAY", 6000) as i32;
        let found = { let _ag = super::nav::act("own pillar: climbing back to it"); self.nav_search(self.cx(), self.cy(), self.cz(), x, y, z, budget, true) };
        let (p0, t0) = (self.scaffold_placed, self.ticks);
        let ok = found && self.count(self.scaffold_item) >= self.nav.scaffold_moves && self.count(self.tower_item) >= self.nav.tower_moves && self.travel();
        self.towers_off = tw;
        self.nav.towers_on = on;
        super::hungdiag::count(if !found { "hung_way_none" } else if ok { "hung_way_ok" } else { "hung_way_failed" }, 1);
        super::hungdiag::count("hung_way_pieces", self.scaffold_placed - p0);
        super::hungdiag::count("hung_way_ticks", self.ticks - t0);
        super::hungdiag::count("hung_way_rope_saved", n0 as i64);
        if self.rope.len() > n0 {
            let extra = self.rope.split_off(n0);
            self.rope_orphaned += extra.len() as i64;
            let old = std::mem::replace(&mut self.rope, extra);
            self.hd_rope_lost(2);
            self.rope = old;
        }
        ok && self.cx() == x && self.cy() == y && self.cz() == z && self.c.on_ground()
    }

    pub fn break_underfoot(&mut self, sx: i32, sy: i32, sz: i32, bx: i32, by: i32, bz: i32) -> bool {
        let ok = { let _a = super::nav::act("underfoot: centring"); self.centre(sx, sy, sz) } && self.break_block(bx, by, bz, bx as f64 + 0.5, by as f64 + 1.0, bz as f64 + 0.5, 80);
        let mut n = 0;
        { let _a = super::nav::act("underfoot: falling"); while n < 200 && !self.c.on_ground() { self.tick(); n += 1; } }
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
        let rv = crate::rev::rev();
        self.check_scaffold();
        // while building: a temporary block (or column) that is the bot's only way out of where it stands stays for
        // now - what wanted its place is put off
        if rv.x_keep && !self.cleaning && !self.must_remove && !self.accept_traps && !self.force_mine && !self.rescuing && self.c.on_ground() && self.scaffold.contains(i as i32)
            && self.g.stand16(self.cx(), self.cy(), self.cz()) >= 0 {
            let (x0, y0, z0) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
            if (x0 - self.cx()).abs() <= 6 && (z0 - self.cz()).abs() <= 6 {
                let mut top = y0;
                if self.g.is_tower(x0, y0, z0) { while self.g.is_tower(x0, top + 1, z0) { top += 1; } }
                let cells = self.column_cells(x0, y0, top, z0);
                let was: Vec<u16> = cells.iter().map(|&k| self.g.cur[k as usize]).collect();
                for &k in &cells { self.g.put(k as usize, 0); }
                let (usable_was, learn_was) = (self.witness_usable, self.witness_learn);
                self.witness_usable = false;   // (cells are gone in thought: a remembered way may have stood on them)
                self.witness_learn = false;
                let (cx, cz) = (self.cx(), self.cz());
                let mut ty = self.cy();
                while ty > self.g.y0 && self.g.stand16(cx, ty, cz) < 0 && self.g.passable(cx, ty, cz) { ty -= 1; }
                let open = self.g.stand16(cx, ty, cz) >= 0 && self.way_out(cx, ty, cz, false);
                self.witness_usable = usable_was; self.witness_learn = learn_was;
                for (n, &k) in cells.iter().enumerate() { self.g.put(k as usize, was[n]); }
                if !open && self.way_out(self.cx(), self.cy(), self.cz(), false) {
                    self.kept_for_exit += 1;
                    if self.events.is_some() { let l = format!("{} K {}\n", self.ticks, self.rel(x0, y0, z0)); self.ev_write(&l); }
                    self.traps_avoided += 1;
                    return false;
                }
            }
        }
        if t().scaffolding[self.g.cur[i] as usize] { return self.remove_tower(i); }
        let careful = rv.careful && self.cleaning && !self.force_mine && !self.accept_traps && self.c.on_ground() && self.plain_way_out(self.cx(), self.cy(), self.cz());
        if careful { self.going_away = Some(vec![i as i32]); }
        let r = self.remove_scaffold_tries(i);
        if careful { self.going_away = None; }
        r
    }

    fn remove_scaffold_tries(&mut self, i: usize) -> bool {
        let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
        for _attempt in 0..3 {
            if !self.scaffold.contains(i as i32) { return true; }
            if !self.c.on_ground() { self.settle(); }
            let e = self.c.eye();
            let here_ok = self.going_away.is_none() || self.safe_after_removal(self.cx(), self.cy(), self.cz());
            if !here_ok { self.unsafe_spots += 1; self.bad_stand.insert(self.g.idx_raw(self.cx(), self.cy(), self.cz())); }
            if here_ok && self.find_break(x, y, z, e[0], e[1], e[2]) {
                let cl = self.click.clone();
                let bt = 80.max(self.break_ticks(self.g.get(x, y, z)));
                let hd_y = self.cy();
                if self.break_block(x, y, z, cl.ax, cl.ay, cl.az, bt) { self.settle(); if super::hungdiag::on() && self.cleaning && self.cy() < hd_y { super::hungdiag::count(if self.rope.is_empty() { "target_fall_norope" } else { "target_fall_rope" }, 1); } return true; }
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
        // (careful while the bot is free to walk away: it stays so. Where it could only get out with new scaffolding
        // anyway, it clears what is in there with it first.)
        let careful = crate::rev::rev().careful && self.cleaning && !self.force_mine && !self.accept_traps && self.c.on_ground() && self.plain_way_out(self.cx(), self.cy(), self.cz());
        if !careful {
            if self.break_tower_block(x, low, z) && !self.scaffold.contains(i as i32) { return true; }
            if !self.scaffold.contains(i as i32) { return true; }
            if low != y { self.break_tower_block(x, y, z); }
            return !self.scaffold.contains(i as i32);
        }
        // the clean-up proper: only from where the bot can get away again once the column is gone - a column that is
        // the only way out of the room it stands in is taken from outside, top down as far as can be reached (what
        // stays is left for the rounds that may break through walls)
        let mut top = y;
        while self.g.is_tower(x, top + 1, z) { top += 1; }
        let r = self.remove_tower_carefully(i, x, low, top, z);
        self.going_away = None;
        r.unwrap_or_else(|| !self.scaffold.contains(i as i32))
    }

    fn remove_tower_carefully(&mut self, i: usize, x: i32, low: i32, top: i32, z: i32) -> Option<bool> {
        self.going_away = Some(self.column_cells(x, low, top, z));
        if self.break_tower_block(x, low, z) && !self.scaffold.contains(i as i32) { return Some(true); }
        if !self.scaffold.contains(self.g.idx(x, low, z) as i32) { return Some(!self.scaffold.contains(i as i32)); }
        let mut yy = (low + 1).max(top - 4);
        while yy <= top {
            if !self.scaffold.contains(self.g.idx(x, yy, z) as i32) || !self.g.is_tower(x, yy, z) { break; }
            self.going_away = Some(self.column_cells(x, yy, top, z));
            if self.break_tower_block(x, yy, z) && !self.scaffold.contains(self.g.idx(x, yy, z) as i32) { self.stubs += 1; break; }
            yy += 2;
        }
        None
    }

    fn column_cells(&self, x: i32, from: i32, to: i32, z: i32) -> Vec<i32> { (from..=to).map(|yy| self.g.idx(x, yy, z) as i32).collect() }

    fn break_tower_block(&mut self, x: i32, y: i32, z: i32) -> bool {
        let i = self.g.idx(x, y, z) as i32;
        for _attempt in 0..3 {
            if !self.scaffold.contains(i) { return true; }
            if !self.c.on_ground() { self.settle(); }
            let e = self.c.eye();
            let here_ok = self.going_away.is_none() || self.safe_after_removal(self.cx(), self.cy(), self.cz());
            if !here_ok { self.unsafe_spots += 1; self.bad_stand.insert(self.g.idx_raw(self.cx(), self.cy(), self.cz())); }
            if here_ok && self.find_break(x, y, z, e[0], e[1], e[2]) {
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

    pub fn cleanup(&mut self) { let _ag = super::nav::act("clean-up");
        let tb = t();
        if self.verbose && !self.own_down { self.log(&format!("PHASE clean-up{} | scaffold blocks standing: {}", if self.force_mine { " (breaking through allowed)" } else { "" }, self.scaffold.len())); }
        self.bad_stand.clear();
        self.comes_back.clear();
        let mut hopeless: FastSet<i32> = FastSet::default();
        let mut guard = 0;
        let mut least = self.scaffold.len();
        let mut since_least = 0usize;
        let mut progress = false;
        let rv = crate::rev::rev();
        self.cleaning = true;
        let own_on = rv.own_down || self.sweep_mode >= 2;
        // (candidate `hdhigh`: in the last sweep's rounds with scaffolding no round ends on a count of tries - see `last_sweep`)
        let no_cut = self.sweep_mode >= 2 && crate::cand::on("hdhigh") && crate::ckpt::armed(self.ticks);
        let mut own_from = self.scaf_log.len();
        let mut last_best: i32 = -1;
        let layer = (self.g.nx * self.g.nz) as i32;
        while self.scaffold.len() > hopeless.len() && guard < 100000 {
            guard += 1;
            if self.scaffold.len() < least { least = self.scaffold.len(); since_least = 0; }
            else if rv.a1 {
                // (counted in pieces taken away, not in tries: what cannot be done now costs nothing and is set aside)
                if since_least > 24 + least / 4 && self.sweep_mode != 1 && !no_cut { if let Some(c) = self.causes.as_mut() { c.cleanup_end = "clean-up: stopped for lack of progress"; } if self.verbose { self.log(&format!("cleanup: no progress for {since_least} rounds with {} left (force={})", self.scaffold.len(), self.force_mine)); } break; }
            } else { since_least += 1; if since_least > 24 + least / 4 { break; } }
            if no_cut && self.time_short() { break; }
            // the clean-up uses scaffolding up too (what is broken drops nothing): fetch more when it runs low - also
            // after having been cut off from the chests for a while (that was then; the bot has moved since)
            // (candidate "trips", rule c) a trip only for the kind that is short AND that the chests still hold
            let trips_c = (crate::cand::on("trips") || crate::cand::on("trips_c")) && crate::ckpt::armed(self.ticks);
            let worth = !trips_c || (self.count(self.scaffold_item) < 24 && self.in_depot(self.scaffold_item) > 0) || (self.count(self.tower_item) < 24 && self.in_depot(self.tower_item) > 0);
            if rv.a1 && worth && (self.count(self.scaffold_item) < 24 || self.count(self.tower_item) < 24) && (self.in_depot(self.scaffold_item) > 0 || self.in_depot(self.tower_item) > 0)
                && self.ticks - self.last_cleanup_restock > 600 && self.rope.is_empty() && self.c.on_ground() {
                self.last_cleanup_restock = self.ticks;
                self.stranded = false;
                self.restock(self.g.oy + self.g.sy);
                self.check_scaffold();
            }
            let mut best: i32 = -1;
            let mut best_y = i32::MIN;
            let mut best_d = i64::MAX;
            let (bx, bz) = (self.cx(), self.cz());
            let mut best_tower = true;
            // (revision "nearclean", the candidate `huge`; MCB_NO_NEARCLEAN; MCSIM_HUGEW=<blocks per level>, 4; 2, 8 and 16 were tried): in a build that is behind its
            // pace, or past half its time, the next piece is not the highest one wherever it stands (a walk over the
            // whole footprint for every level: a quarter of all ticks of the biggest builds went to walking in the
            // clean-up, half of that in hops of more than 24 blocks), but the one that is cheapest in blocks to walk
            // plus MCSIM_HUGEW blocks for every level it lies below the highest - still from the top down, but
            // region by region.
            let near_first = crate::rev::rev().near_clean && crate::ckpt::armed(self.ticks) && (self.ticks * 2 > self.max_ticks || self.behind_pace());
            let mut pool: Vec<(bool, i32, i64, i32)> = Vec::new();
            for i in self.scaffold.keys() {
                if hopeless.contains(&i) || self.leave_alone.as_ref().is_some_and(|l| l.contains(&i)) { continue; }
                // (revision "owndown": twice the bot put up scaffolding to get at this piece and failed all the same -
                // it is not climbed for again; a round that puts up nothing may still try it)
                if own_on && (od_futile() || self.sweep_mode >= 2) && !self.no_scaffold_nav && self.futile.get(&i).copied().unwrap_or(0) >= 2 { continue; }
                if !self.force_mine && self.comes_back.get(i) >= 3 { continue; }
                let iu = i as usize;
                let tw = tb.scaffolding[self.g.cur[iu] as usize];
                if tw && !best_tower { continue; }
                if tw && i - layer >= 0 && self.scaffold.contains(i - layer) && tb.scaffolding[self.g.cur[(i - layer) as usize] as usize] { continue; }
                let y = self.g.y_of(iu);
                let d = ((self.g.x_of(iu) - bx).abs() + (self.g.z_of(iu) - bz).abs()) as i64;
                if near_first { pool.push((tw, y, d, i)); }
                if (best_tower && !tw) || y > best_y || (y == best_y && d < best_d) { best = i; best_y = y; best_d = d; best_tower = tw; }
            }
            if near_first && best >= 0 {
                // (the highest of the kind that goes first is `best_y`; plain pieces still go before columns)
                let w = crate::cand::num("MCSIM_HUGEW", 4);
                let mut cost = best_d;
                for &(tw, y, d, i) in &pool {
                    if tw != best_tower { continue; }
                    let c = d + w * (best_y - y) as i64;
                    if c < cost { cost = c; best = i; }
                }
            }
            if best < 0 {
                // everything else has had its turn: what could not be done before may be possible now that the rest is gone
                if rv.x_loop && progress && !hopeless.is_empty() { hopeless.clear(); progress = false; continue; }
                if self.verbose { self.log(&format!("cleanup: nothing more to try with {} left, {} given up (force={})", self.scaffold.len(), hopeless.len(), self.force_mine)); }
                break;
            }
            // (revision "owndown") What the tries so far put up to get at their pieces comes down when the next
            // piece lies somewhere else - while the bot is still where it put it up. A column standing on a roof has
            // its foot up there, and from the ground there is no way back to it short of another column. As long as
            // the next piece is close by, what stands is used again.
            if own_on && !self.own_down && self.scaf_log.len() > own_from && last_best >= 0 && {
                let (a, b) = (best as usize, last_best as usize);
                (self.g.x_of(a) - self.g.x_of(b)).abs() + (self.g.z_of(a) - self.g.z_of(b)).abs() > 8 || (self.g.y_of(a) - self.g.y_of(b)).abs() > 8 } {
                let had = self.scaffold.len();
                self.own_down_now(own_from);
                own_from = self.scaf_log.len();
                last_best = -1;
                if self.scaffold.len() != had { continue; }
            }
            last_best = best;
            let before = self.scaffold.len();
            let log_try = self.scaf_log.len();
            let hd_k = self.hd_key(best);
            self.hd_try_begin();
            let hd_was = super::hungdiag::set_ctx("target");
            let ok_remove = self.remove_scaffold(best as usize);
            super::hungdiag::set_ctx(hd_was);
            if own_on && !ok_remove && self.scaf_log.len() > log_try { *self.futile.entry(best).or_insert(0) += 1; }
            if ok_remove { self.comes_back.add_to(best, 1); }
            if ok_remove { self.sweep_scaffold(); }
            self.hd_try_reach(best, log_try);
            // (candidate `hungtower`) a try that put up a column or a long way: what it reaches goes while the bot is up there
            let big = !self.own_down && self.scaf_log.len() - log_try >= crate::cand::num("MCSIM_HTMIN", 6) as usize;
            let ht = big && crate::rev::rev().tower_serve && crate::ckpt::armed(self.ticks) && self.behind_pace();
            if ht { self.tower_serve(log_try, &hopeless); }
            self.undo_rope();
            if ht && crate::cand::num("MCSIM_HTDOWN", 0) != 0 && self.scaf_log[log_try..].iter().any(|&k| self.scaffold.contains(k)) { self.own_down_now(log_try); }
            self.hd_try(hd_k, log_try, !self.scaffold.contains(best));
            if self.verbose { let b = best as usize; self.log(&format!("cleanup {} {} left={}", self.rel(self.g.x_of(b), self.g.y_of(b), self.g.z_of(b)), if ok_remove { "ok" } else { "FAILED" }, self.scaffold.len())); }
            if !ok_remove && self.watch.is_some() { let b = best as usize; if self.watching(self.g.x_of(b), self.g.y_of(b), self.g.z_of(b)) { self.wlog(&format!("cleanup failed on the watched piece: dbg stand/click/nav/pillar/pillarClick/unsafe={}/{}/{}/{}/{}/{} navExpanded={} exhausted={} force={} noScaffoldNav={}", self.dbg_stand, self.dbg_click, self.dbg_nav, self.dbg_pillar, self.dbg_pillar_click, self.dbg_unsafe, self.nav.expanded, self.nav.exhausted, self.force_mine, self.no_scaffold_nav)); } }
            if self.causes.is_some() {
                let why: &'static str = if ok_remove { if self.scaffold.len() < before { "" } else { "clean-up: taken away, and it stood again" } }
                    else if self.dbg_unsafe > 0 { "clean-up: taking it away would cut the bot's way out" }
                    else if self.dbg_click + self.dbg_pillar_click == 0 { "clean-up: no place to stand sees the piece" }
                    else if self.dbg_cut > 0 { "clean-up: no way to a place to stand (a search was cut short)" }
                    else { "clean-up: no way to a place to stand (all searches ran dry)" };
                let ticks = self.ticks;
                let c = self.causes.as_mut().unwrap();
                if why.is_empty() { c.scaf.remove(&best); } else { c.scaf.insert(best, why); c.first.entry(best).or_insert(ticks); c.last.insert(best, ticks); }
            }
            if !ok_remove { hopeless.insert(best); self.bad_stand.clear(); }
            else if self.scaffold.len() < before { progress = true; if !rv.x_loop { hopeless.clear(); } }
            else { hopeless.insert(best); if rv.a1 { since_least += 1; } }
            if rv.a1 && ok_remove && self.scaffold.len() >= least { since_least += 1; }
        }
        if own_on && !self.own_down && self.scaf_log.len() > own_from { self.own_down_now(own_from); }
        self.cleaning = false;
    }

    /// (candidate `hungtower`, MCSIM_CAND=hungtower; in the clean-up of a build behind its pace) A try put up a column
    /// or a long way of its own (MCSIM_HTMIN pieces, 6, since `from` in `scaf_log`) to get at one piece. Until now the
    /// next piece was chosen by the usual rule from wherever the bot stood, and the way came down at once (the bot's
    /// own pillar and bridges) or stood for later (climbing columns); in the huge builds that run out of time the
    /// rounds that may break through ended with more pieces standing than they began with, a column of 20 to 180 for
    /// every piece. Here, while the bot is still up there, it takes every other piece within reach of a place the
    /// try's way let it stand (the top of every piece it put up, every level of its climbing columns, where it stands)
    /// or a short walk (MCSIM_HTWALK blocks, 4) beyond that: the nearest first, with nothing new put up and nothing of
    /// the build broken, and its own way (pillar, bridges) kept up meanwhile (`keep_rope`) - it comes down after, the
    /// bot walking back to its top as usual; at most MCSIM_HTMAX tries (48). (Tried: serving only when nothing of the
    /// bot's own way stands, sweeping instead at every level of its pillar as it comes down - the bot lost its way
    /// twice as often, c0844480 PASS -> out of time; stopping after 6 failures in a row and no trapping routes -
    /// d57983dc PASS -> out of time. MCSIM_HTFAILS: failures in a row before it stops, off. MCSIM_HTDOWN=1: the try's
    /// climbing columns come down after it, with `own_down_now`.)
    fn tower_serve(&mut self, from: usize, hopeless: &FastSet<i32>) -> usize {
        let tb = t();
        let own: FastSet<i32> = self.scaf_log[from..].iter().copied().collect();
        let mut stands: Vec<(i32, i32, i32)> = vec![(self.cx(), self.cy(), self.cz())];
        for &c in &own {
            if !self.scaffold.contains(c) { continue; }
            let cu = c as usize;
            let (x, y, z) = (self.g.x_of(cu), self.g.y_of(cu), self.g.z_of(cu));
            if tb.scaffolding[self.g.cur[cu] as usize] { stands.push((x, y, z)); }
            if !self.g.inside(x, y + 1, z) || !own.contains(&(self.g.idx(x, y + 1, z) as i32)) { stands.push((x, y + 1, z)); }
        }
        let w = crate::cand::num("MCSIM_HTWALK", 4) as i32;
        let max = crate::cand::num("MCSIM_HTMAX", 48);
        // (the columns the bot's own way stands in, and the one it stands in, are not touched: they are its way down)
        let mut way_cols: FastSet<(i32, i32)> = self.rope.iter().map(|e| (e[4], e[6])).collect();
        way_cols.insert((self.cx(), self.cz()));
        let mut cand: Vec<i32> = Vec::new();
        let mut seen: FastSet<i32> = FastSet::default();
        for &(x, y, z) in &stands { for dy in -4..=5 { for dz in -(4 + w)..=(4 + w) { for dx in -(4 + w)..=(4 + w) {
            let (qx, qy, qz) = (x + dx, y + dy, z + dz);
            if !self.g.inside(qx, qy, qz) { continue; }
            let q = self.g.idx(qx, qy, qz) as i32;
            if !self.scaffold.contains(q) || own.contains(&q) || !seen.insert(q) { continue; }
            if way_cols.contains(&(qx, qz)) || hopeless.contains(&q) || self.leave_alone.as_ref().is_some_and(|l| l.contains(&q)) { continue; }
            if !self.force_mine && self.comes_back.get(q) >= 3 { continue; }
            cand.push(q);
        } } } }
        if cand.is_empty() { return 0; }
        let (nsn, force, keep) = (self.no_scaffold_nav, self.force_mine, self.keep_rope);
        self.no_scaffold_nav = true;
        self.force_mine = false;
        self.keep_rope = true;
        let (mut served, mut tries, mut fails) = (0usize, 0i64, 0i64);
        let max_fails = crate::cand::num("MCSIM_HTFAILS", 1_000_000);
        let had = self.scaffold.len();
        while tries < max && fails < max_fails && !self.time_short() {
            let (bx, by, bz) = (self.cx(), self.cy(), self.cz());
            let mut best = -1;
            let mut best_c = i64::MAX;
            for &q in &cand {
                if !self.scaffold.contains(q) { continue; }
                let qu = q as usize;
                let c = ((self.g.x_of(qu) - bx).abs() + (self.g.z_of(qu) - bz).abs() + 2 * (self.g.y_of(qu) - by).abs()) as i64;
                if c < best_c { best_c = c; best = q; }
            }
            if best < 0 { break; }
            tries += 1;
            cand.retain(|&q| q != best);
            if self.remove_scaffold(best as usize) {
                self.comes_back.add_to(best, 1);
                self.sweep_scaffold();
                served += 1;
                fails = 0;
            } else { fails += 1; }
            if !self.c.on_ground() { self.settle(); }
        }
        self.no_scaffold_nav = nsn;
        self.force_mine = force;
        self.keep_rope = keep;
        super::hungdiag::count("ht_serves", 1);
        super::hungdiag::count("ht_tries", tries);
        super::hungdiag::count("ht_served", served as i64);
        super::hungdiag::count("ht_gone", had as i64 - self.scaffold.len() as i64);
        if self.verbose { self.log(&format!("hungtower: {} pieces put up for the try, {tries} tries from up there, {served} taken, {} standing fewer", self.scaf_log.len() - from, had as i64 - self.scaffold.len() as i64)); }
        served
    }

    /// (revision "owndown") Takes down the pieces put up since `from` in `scaf_log` that still stand. Nothing new
    /// is put up for this, and every other piece is left alone in it.
    pub(super) fn own_down_now(&mut self, from: usize) {
        // (candidate `hdmend`: what the mend put up comes down even when time is short - "short" is the time kept for just that)
        if self.time_short() && !(self.mend_only && crate::cand::on("hdmend")) { return; }
        let own: Vec<i32> = self.scaf_log[from..].iter().copied().filter(|&k| self.scaffold.contains(k)).collect();
        if own.is_empty() { return; }
        let ownset: FastSet<i32> = own.iter().copied().collect();
        let others: FastSet<i32> = self.scaffold.keys().into_iter().filter(|k| !ownset.contains(k)).collect();
        let (leave_was, nsn_was, back_was, bad_was) = (self.leave_alone.take(), self.no_scaffold_nav, self.comes_back.clone(), self.bad_stand.clone());
        // (and nothing of the build is broken for it: a round that may break through keeps that for its own pieces)
        let (force_was, traps_was) = (self.force_mine, self.accept_traps);
        if !own_down_force() { self.force_mine = false; if !(self.sweep_mode == 2 && crate::cand::num("MCSIM_HUGEDONE_OWNTRAPS", 0) != 0) { self.accept_traps = false; } }
        self.leave_alone = Some(others);
        self.no_scaffold_nav = true;
        self.own_down = true;
        // (what is known as out of reach was learnt under other rules, and what is learnt now holds only for these)
        self.unreachable.clear();
        self.cleanup();
        self.unreachable.clear();
        self.own_down = false;
        self.force_mine = force_was; self.accept_traps = traps_was;
        self.cleaning = true;
        self.no_scaffold_nav = nsn_was; self.leave_alone = leave_was; self.comes_back = back_was; self.bad_stand = bad_was;
        self.undo_rope();
        if self.verbose { let still = own.iter().filter(|&&k| self.scaffold.contains(k)).count(); self.log(&format!("cleanup: {} pieces put up for the tries here, {} of them still standing", own.len(), still)); }
    }

    /// Breaks every scaffold block within reach of where the bot stands, except the one it stands on.
    fn sweep_scaffold(&mut self) {
        let hd_was = super::hungdiag::set_ctx("sweep");
        self.sweep_scaffold_impl();
        super::hungdiag::set_ctx(hd_was);
    }
    fn sweep_scaffold_impl(&mut self) {
        let mut again = true;
        let mut hd_last: Option<(i32, bool)> = None;
        while again {
            again = false;
            if !self.c.on_ground() { self.settle(); }
            if let Some((hy, lean)) = hd_last.take() { if super::hungdiag::on() && self.cleaning && self.cy() < hy { super::hungdiag::count(if self.rope.is_empty() { "sweep_fall_norope" } else { "sweep_fall_rope" }, 1); super::hungdiag::count("sweep_fall_leaning", lean as i64); } }
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
                    let hd_y = self.cy();
                    if self.break_block(tx, ty, tz, cl.ax, cl.ay, cl.az, bt) { hd_last = Some((hd_y, self.leaning)); again = true; break 'scan; }
                }
            } } }
        }
    }
}

/// (measurement) MCB_OWNDOWN_FORCE: the take-down of `own_down_now` keeps a round's leave to break through the build.
fn own_down_force() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCB_OWNDOWN_FORCE").is_some())
}

/// (measurement; both off) MCB_OD_FUTILE: a piece the clean-up failed on twice after putting up scaffolding for it is
/// not climbed for again; MCB_OD_FINAL: at the very end the bot goes back to the chests and has one more round with
/// nothing new put up (`Bot::run_inner`). On the lead's 80 under six seeds the two together cost 4.5 passes a seed.
pub fn od_futile() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCB_OD_FUTILE").is_some())
}
pub fn od_final() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCB_OD_FINAL").is_some())
}

/// (candidate `hung`) Is a lost rope of `n` pieces whose top stands `h` above the floor of the build worth the climb
/// back? A piece lost up there costs about one piece a level to come back for later (in 3f22ec42, 256 high, about 90
/// pieces a try), the climb back a few: n * h >= MCSIM_HUNGWORTH (64), or n >= MCSIM_HUNGMIN (0: off).
/// Tried: every rope (14b4acf1 2.70M -> 3.40M ticks, 3b469589 PASS -> out of time: small ropes low down are not
/// worth it); n >= 8 or n * h >= 400 (controls 2.76M and 3.02M, but 3f22ec42 left with 4,129 pieces, not 473).
fn hung_worth(n: i64, h: i64) -> bool {
    let (min, worth) = (crate::cand::num("MCSIM_HUNGMIN", 0), crate::cand::num("MCSIM_HUNGWORTH", 64));
    (min > 0 && n >= min) || n * h >= worth
}

fn rope_far() -> i32 {
    static V: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MCSIM_ROPEFAR").ok().and_then(|v| v.parse().ok()).unwrap_or(6))
}
