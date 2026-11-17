//! The builder's order of work and main loop (the Java version's `Builder`).
use super::state::{same_family, same_kind, Bot, Spec};
use super::*;
use crate::kinds::Kind;
use crate::rules;
use crate::tables::*;

impl Bot {
    /// Runs the whole build; sets `status`.
    pub fn run(&mut self) {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.run_inner()));
        match r {
            Ok(()) => self.status = "done".to_string(),
            Err(e) => {
                if e.downcast_ref::<OutOfTime>().is_some() {
                    self.status = if self.died { "died".to_string() } else if self.ticks >= self.max_ticks { "out of time".to_string() } else { "wall limit".to_string() };
                } else {
                    let msg = if let Some(s) = e.downcast_ref::<String>() { s.clone() } else if let Some(s) = e.downcast_ref::<&str>() { s.to_string() } else { "?".to_string() };
                    self.status = format!("crash: {msg}");
                }
            }
        }
        self.c.release_keys();
        self.c.hold_attack(false);
    }

    fn run_inner(&mut self) {
        let rv = crate::rev::rev();
        self.col_next = vec![NONE; (self.g.nx * self.g.nz) as usize];
        self.col_next_set = true;
        self.tiles_x = (self.g.sx + 7) >> 3;
        self.tiles_z = (self.g.sz + 7) >> 3;
        self.tile_pending = vec![0; (self.tiles_x * self.tiles_z) as usize];
        for z in self.g.oz..self.g.oz + self.g.sz { for x in self.g.ox..self.g.ox + self.g.sx { let y = self.scan_column(x, self.g.oy, z); self.set_next(x, z, y); } }
        self.restock(self.g.oy);
        let mut guard = 0;
        loop {
            if self.time_short() { if let Some(c) = self.causes.as_mut() { c.time_cut = true; } break; }
            let i = self.pick_next();
            if i < 0 {
                if self.blocked.is_empty() { break; }
                let mut any = false;
                for cell in self.blocked.keys() {
                    self.blocked.remove(cell);
                    if !self.needs(cell as usize) { continue; }
                    // Before the cell is put off until everything else stands: one more try, with the helper block
                    // that may rest on a second helper (place_helper). By the time the passes after the main one get
                    // to it, an upright log that hangs in the air is walled in by its leaves, and every log of the
                    // trunk above it has waited in vain for the face of the one below. (Not in machines.)
                    if rv.orphan && !self.has_pistons() {
                        self.helper_chain_now = true;
                        let r = self.do_cell(cell as usize);
                        self.helper_chain_now = false;
                        self.reconcile();
                        if r != DEFER && r != SKIP && !self.needs(cell as usize) {
                            self.orphans += 1;
                            let (x, z) = (self.g.x_of(cell as usize), self.g.z_of(cell as usize));
                            let ny = self.scan_column(x, self.g.y_of(cell as usize) + 1, z);
                            self.set_next(x, z, ny);
                            any = true;
                            continue;
                        }
                        if r == SKIP { let (x, z) = (self.g.x_of(cell as usize), self.g.z_of(cell as usize)); let ny = self.scan_column(x, self.g.y_of(cell as usize) + 1, z); self.set_next(x, z, ny); any = true; continue; }
                    }
                    self.deferred.add(cell);
                    let (x, z) = (self.g.x_of(cell as usize), self.g.z_of(cell as usize));
                    let ny = self.scan_column(x, self.g.y_of(cell as usize) + 1, z);
                    self.set_next(x, z, ny);
                    any = true;
                }
                if !any { guard += 1; if guard > 3 { break; } }
                continue;
            }
            guard = 0;
            self.work(i as usize);
        }
        // (Java `noteMainDone`: the state of the run when the main building was over, for the result file)
        self.main_ticks_at = self.ticks;
        self.put_off_at_main = self.deferred.keys().iter().filter(|&&c| self.needs(c as usize)).count() as i64;
        self.scaffold_at_main = self.scaffold.len() as i64;
        self.main_done = true;
        self.late_too = false;
        self.deferred_pass(false);
        self.undo_rope();
        self.reconcile();
        self.layer_now = i32::MAX;
        self.late_too = true;
        self.deferred_pass(false);
        self.undo_rope();
        self.reconcile();
        self.status = "built".to_string();
        self.build_ticks = self.ticks;
        self.cleanup();
        self.deferred_pass(false);
        self.undo_rope();
        self.reconcile();
        self.cleanup();
        self.no_roofing = false;
        self.deferred_pass(false);
        self.deferred_pass(true);
        self.undo_rope();
        self.reconcile();
        self.cleanup();
        let mut round = 0;
        while round < 3 && (!self.scaffold.is_empty() || self.any_deferred()) && !self.time_short() {
            self.force_mine = true;
            self.accept_traps = true;
            self.unreachable.clear();
            self.bad_stand.clear();
            self.cleanup();
            self.force_mine = false;
            self.accept_traps = false;
            self.undo_rope();
            self.deferred_pass(false);
            if round > 0 { self.deferred_pass(true); }
            self.undo_rope();
            self.reconcile();
            self.cleanup();
            round += 1;
        }
        if !self.scaffold.is_empty() {
            self.force_mine = true;
            self.unreachable.clear();
            self.bad_stand.clear();
            self.cleanup();
            self.force_mine = false;
            self.undo_rope();
            self.deferred_pass(false);
            self.undo_rope();
            self.reconcile();
            self.cleanup();
            let mut turn = 0;
            while turn < 4 && !self.scaffold.is_empty() {
                let before = self.scaffold.len();
                let stood: crate::fasthash::FastSet<i32> = if rv.a1 { self.scaffold.keys().into_iter().collect() } else { Default::default() };
                self.force_mine = true;
                self.unreachable.clear();
                self.bad_stand.clear();
                self.cleanup();
                self.force_mine = false;
                self.undo_rope();
                if self.scaffold.is_empty() { break; }
                self.unreachable.clear();
                if !rv.a1 {
                    self.cleanup();
                    self.undo_rope();
                } else {
                    // (this late nothing new is put up to get at what is left: it would only have to come down again)
                    self.no_scaffold_nav = true;
                    self.cleanup();
                    self.no_scaffold_nav = false;
                    self.undo_rope();
                    // A round that breaks through gives up after a number of tries without getting below where it
                    // began - and what it put up on those tries stands where it stopped. Those pieces come down the
                    // ordinary way, with whatever that takes. The old pieces are left alone in this.
                    let put = self.scaffold.keys().iter().filter(|i| !stood.contains(i)).count() as i64;
                    if put > 0 {
                        self.forced_worse += 1;
                        self.unreachable.clear();
                        self.leave_alone = Some(stood.clone());
                        self.cleanup();
                        self.leave_alone = None;
                        self.undo_rope();
                        let still = self.scaffold.keys().iter().filter(|i| !stood.contains(i)).count() as i64;
                        self.tidied += put - still;
                        if self.verbose { self.log(&format!("end game: {put} pieces of the round's own were out of reach without scaffolding, {still} of them still standing, {} in all", self.scaffold.len())); }
                        self.unreachable.clear();
                        self.no_scaffold_nav = true;
                        self.cleanup();
                        self.no_scaffold_nav = false;
                        self.undo_rope();
                    }
                }
                if self.scaffold.len() >= before { break; }
                turn += 1;
            }
            // The turns above break through the build to get at what stands behind it, and nothing after them puts
            // back what they broke (four pieces fewer for thirteen blocks of a wall missing). So: one more pass,
            // over the cells the bot broke out itself and nothing else (what never went in has had its passes), and
            // with nothing new put up for it - scaffolding for this would stand at the end (tried: in three builds
            // one to five pieces left became 40 to 112).
            if rv.mend && self.broken_to_mend() > 0 && !self.time_short() {
                let missing = self.broken_to_mend();
                self.unreachable.clear();
                self.bad_stand.clear();
                self.mend_only = true;
                self.no_scaffold_nav = true;
                self.deferred_pass(false);
                self.no_scaffold_nav = false;
                self.mend_only = false;
                self.undo_rope();
                self.reconcile();
                self.mended += missing - self.broken_to_mend();
            }
        }
        for cell in self.deferred.keys() { if self.needs(cell as usize) { self.skip(cell as usize, "no way to place"); } }
    }

    /// True when the ticks left are about what taking down the standing scaffolding will need.
    pub fn time_short(&self) -> bool { self.max_ticks - self.ticks < 45 * self.scaffold.len() as i64 + 40000 }

    fn deferred_pass(&mut self, traps: bool) {
        if self.time_short() { if let Some(c) = self.causes.as_mut() { c.time_cut = true; } return; }
        if self.verbose { self.log(&format!("PHASE deferred cells{}: {} | scaffold blocks standing: {}", if traps { " (traps accepted)" } else { "" }, self.deferred.len(), self.scaffold.len())); }
        self.accept_traps = traps;
        // The original picks, again and again, the first of the nearest cells not yet visited (a quadratic scan).
        // Same choice here: while the bot stays where it is, the picks are the unvisited cells in order of
        // (distance, position in the list), so the second pick from one spot sorts them once and later picks read on.
        let cells = self.deferred.keys();
        let n = cells.len();
        let xs: Vec<i32> = cells.iter().map(|&c| self.g.x_of(c as usize)).collect();
        let ys: Vec<i32> = cells.iter().map(|&c| self.g.y_of(c as usize)).collect();
        let zs: Vec<i32> = cells.iter().map(|&c| self.g.z_of(c as usize)).collect();
        let mut done = vec![false; n];
        let mut last_pos = (i32::MIN, 0, 0);
        let mut sorted: Vec<(i32, u32)> = Vec::new();
        let mut sorted_at = 0;
        let mut sorted_pos = (i32::MIN, 0, 0);
        for _ in 0..n {
            let pos = (self.cx(), self.cy(), self.cz());
            let (bx, by, bz) = pos;
            let dist = |j: usize| (xs[j] - bx).abs() + (zs[j] - bz).abs() + 2 * (ys[j] - by).abs();
            if sorted_pos != pos && last_pos == pos {
                sorted.clear();
                for j in 0..n { if !done[j] { sorted.push((dist(j), j as u32)); } }
                sorted.sort_unstable();
                sorted_at = 0;
                sorted_pos = pos;
            }
            last_pos = pos;
            let k = if sorted_pos == pos {
                // (cells visited from elsewhere since the list was made are passed over)
                while done[sorted[sorted_at].1 as usize] { sorted_at += 1; }
                sorted_at += 1;
                sorted[sorted_at - 1].1 as usize
            } else {
                let mut k = usize::MAX;
                let mut best = i32::MAX;
                for j in 0..n { if !done[j] { let d = dist(j); if d < best { best = d; k = j; } } }
                k
            };
            done[k] = true;
            let cell = cells[k];
            if self.time_short() { if let Some(c) = self.causes.as_mut() { c.time_cut = true; } break; }
            if !self.needs(cell as usize) { self.deferred.remove(cell); continue; }
            if !self.late_too && self.is_late(cell as usize) { continue; }
            if self.mend_only && !self.broken_out.contains(&cell) { continue; }
            if self.verbose { let cu = cell as usize; self.log(&format!("deferred cell {} {} (now {})", self.rel(self.g.x_of(cu), self.g.y_of(cu), self.g.z_of(cu)), t().name(self.g.tgt[cu]), t().name(self.g.cur[cu]))); }
            let r = self.do_cell(cell as usize);
            if self.watch.is_some() { let cu = cell as usize; if self.watching(self.g.x_of(cu), self.g.y_of(cu), self.g.z_of(cu)) { self.wlog(&format!("deferred pass: doCell gave {r} (0 placed, 1 defer, 2 skip) why={} dbg stand/click/nav/pillar/pillarClick={}/{}/{}/{}/{}", self.defer_why, self.dbg_stand, self.dbg_click, self.dbg_nav, self.dbg_pillar, self.dbg_pillar_click)); } }
            if r != DEFER { self.deferred.remove(cell); }
        }
        self.accept_traps = false;
    }

    /// Cells the bot broke out of the build itself that are still missing.
    fn broken_to_mend(&self) -> i64 { self.deferred.keys().iter().filter(|&&c| self.broken_out.contains(&c) && self.needs(c as usize)).count() as i64 }

    fn any_deferred(&self) -> bool { self.deferred.keys().iter().any(|&c| self.needs(c as usize)) }

    pub fn scan_column(&mut self, x: i32, from_y: i32, z: i32) -> i32 {
        for y in from_y.max(self.g.oy)..self.g.oy + self.g.sy {
            let i = self.g.idx(x, y, z);
            if self.g.tgt[i] == 0 || !self.needs(i) || self.deferred.contains(i as i32) { continue; }
            if self.is_late(i) || self.waits_for_door(i) { self.deferred.add(i as i32); continue; }
            return y;
        }
        NONE
    }

    pub fn eligible(&self, x: i32, z: i32) -> bool {
        let y = self.col_next[self.col(x, z)];
        if y == NONE { return false; }
        for dz in -1..=1 { for dx in -1..=1 {
            if dx == 0 && dz == 0 { continue; }
            let (nx, nz) = (x + dx, z + dz);
            if nx < self.g.ox || nz < self.g.oz || nx >= self.g.ox + self.g.sx || nz >= self.g.oz + self.g.sz { continue; }
            if self.col_next[self.col(nx, nz)] < y { return false; }
        } }
        true
    }

    pub fn set_next(&mut self, x: i32, z: i32, y: i32) {
        let c0 = self.col(x, z);
        let old = self.col_next[c0];
        self.col_next[c0] = y;
        if (old == NONE) != (y == NONE) {
            let ti = (((z - self.g.oz) >> 3) * self.tiles_x + ((x - self.g.ox) >> 3)) as usize;
            self.tile_pending[ti] += if y == NONE { -1 } else { 1 };
        }
    }

    /// The allowed cell nearest to the bot, or -1.
    fn pick_next(&mut self) -> i32 {
        let tb = t();
        let bx = self.g.ox.max((self.g.ox + self.g.sx - 1).min(self.cx()));
        let bz = self.g.oz.max((self.g.oz + self.g.sz - 1).min(self.cz()));
        let by = self.cy();
        let (btx, btz) = ((bx - self.g.ox) >> 3, (bz - self.g.oz) >> 3);
        let mut best: i32 = -1;
        let mut best_cost = f64::MAX;
        let max_r = self.tiles_x.max(self.tiles_z);
        if let Some(wc) = self.watch {
            let (wx, wz) = (self.g.ox + wc[0], self.g.oz + wc[2]);
            if self.g.in_box(wx, self.g.oy, wz) {
                let cn = self.col_next[self.col(wx, wz)];
                let mut nb = String::new();
                for dz in -1..=1 { for dx in -1..=1 { let (nx, nz) = (wx + dx, wz + dz); if self.g.in_box(nx, self.g.oy, nz) { let v = self.col_next[self.col(nx, nz)]; nb += &format!(" {}", if v == NONE { "-".to_string() } else { (v - self.g.oy).to_string() }); } } }
                self.wlog(&format!("pickNext: watched column next={} neighbours:{} eligible={}", if cn == NONE { "-".to_string() } else { (cn - self.g.oy).to_string() }, nb, self.eligible(wx, wz)));
            }
        }
        self.carried.clear();
        for k in 0..36 { let st = self.c.pl.inv[k]; if st.count > 0 { self.carried.insert(st.item); } }
        for r in 0..=max_r {
            if best >= 0 && ((r - 1) * 8 + 1) as f64 > best_cost { break; }
            for dtz in -r..=r {
                let tz = btz + dtz;
                if tz < 0 || tz >= self.tiles_z { continue; }
                let step = if dtz == -r || dtz == r { 1 } else { 2 * r };
                let mut dtx = -r;
                while dtx <= r {
                    let tx = btx + dtx;
                    dtx += 1.max(step);
                    if tx < 0 || tx >= self.tiles_x || self.tile_pending[(tz * self.tiles_x + tx) as usize] == 0 { continue; }
                    let (x0, z0) = (self.g.ox + (tx << 3), self.g.oz + (tz << 3));
                    let (x1, z1) = ((x0 + 8).min(self.g.ox + self.g.sx), (z0 + 8).min(self.g.oz + self.g.sz));
                    for z in z0..z1 { for x in x0..x1 {
                        let y = self.col_next[self.col(x, z)];
                        if y == NONE || !self.eligible(x, z) { continue; }
                        let i = self.g.idx(x, y, z);
                        if self.blocked.contains(i as i32) && self.blocked.get(i as i32) == self.surroundings(i) { continue; }
                        let mut cost = (x - bx).abs().max((z - bz).abs()) as f64 + 0.4 * (y - by).abs() as f64;
                        let mut attached = false;
                        for d in 0..6 { if attached { break; } attached = self.g.get(x + DX[d], y + DY[d], z + DZ[d]) != 0; }
                        if !attached { cost += (60 + 2 * 0.max(y - by)) as f64; }
                        if !self.carried.contains(&tb.item[self.g.tgt[i] as usize]) { cost += 400.0; }
                        if cost < best_cost { best_cost = cost; best = i as i32; }
                    } }
                }
            }
        }
        if self.watch.is_some() && best >= 0 { let b = best as usize; self.wlog(&format!("pickNext -> {} {} cost {:.1}", self.rel(self.g.x_of(b), self.g.y_of(b), self.g.z_of(b)), t().name(self.g.tgt[b]), best_cost)); }
        best
    }

    fn work(&mut self, i: usize) {
        let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
        self.layer_now = y;
        if self.placed + self.broken != self.progress_mark { self.progress_mark = self.placed + self.broken; self.progress_tick = self.ticks; }
        if self.ticks - self.progress_tick > 4000 {
            self.progress_tick = self.ticks;
            self.skip(i, "stuck");
            let ny = self.scan_column(x, y + 1, z);
            self.set_next(x, z, ny);
            self.bad_stand.clear();
            return;
        }
        if self.watching(x, y, z) { self.wlog("work: doCell"); }
        if crate::rev::rev().a1 {
            self.monitor_exit();
            self.guard_health();
        }
        let mut r = self.do_cell(i);
        if self.watching(x, y, z) { self.wlog(&format!("work: doCell gave {r} (0 placed, 1 defer, 2 skip); plans={} planFails={} dbg stand/click/nav/pillar/pillarClick={}/{}/{}/{}/{}", self.plans, self.plan_fails, self.dbg_stand, self.dbg_click, self.dbg_nav, self.dbg_pillar, self.dbg_pillar_click)); }
        self.reconcile();
        if r == PLACED && self.needs(i) { r = DEFER; }
        if r == DEFER {
            if self.tries.add_to(i as i32, 1) >= 12 {
                self.deferred.add(i as i32);
                self.blocked.remove(i as i32);
                let ny = self.scan_column(x, y + 1, z);
                self.set_next(x, z, ny);
            } else {
                let s = self.surroundings(i);
                self.blocked.put(i as i32, s);
            }
            return;
        }
        self.blocked.remove(i as i32);
        let ny = self.scan_column(x, y + 1, z);
        self.set_next(x, z, ny);
        self.doors_ready();
        if r == PLACED && !self.deferred.is_empty() {
            for d in 0..6 {
                let (nx, ny, nz) = (x + DX[d], y + DY[d], z + DZ[d]);
                if !self.g.inside(nx, ny, nz) { continue; }
                let n = self.g.idx(nx, ny, nz);
                if self.deferred.contains(n as i32) && self.needs(n) && !self.is_late(n) && self.count(t().item[self.g.tgt[n] as usize]) > 0 && self.do_cell(n) != DEFER { self.deferred.remove(n as i32); }
            }
        }
    }

    fn doors_ready(&mut self) {
        while let Some(pc) = self.door_events.pop() {
            let Some(waiters) = self.door_waiters.get(&pc).cloned() else { continue };
            if self.needs(pc as usize) { continue; }
            for n in waiters {
                let nu = n as usize;
                if !self.deferred.contains(n) || !self.needs(nu) || self.is_late(nu) || self.waits_for_door(nu) { continue; }
                self.deferred.remove(n);
                let (wx, wz, wy) = (self.g.x_of(nu), self.g.z_of(nu), self.g.y_of(nu));
                let cur = self.col_next[self.col(wx, wz)];
                if cur == NONE || cur > wy { self.set_next(wx, wz, wy); }
            }
        }
    }

    /// The safety invariant, sampled: from where the bot stands there is a way back to the chests (2 = with what
    /// stands, 1 = with new scaffolding, 0 = none). Counts episodes of "none" and when the first began.
    fn monitor_exit(&mut self) {
        let (cx, cy, cz) = (self.cx(), self.cy(), self.cz());
        if !crate::rev::rev().monitor || self.ticks - self.last_monitor < 600 || !self.c.on_ground() || !self.g.inside(cx, cy, cz) || self.g.stand16(cx, cy, cz) < 0 { return; }
        self.last_monitor = self.ticks;
        let tc = self.trap_checks as i32;
        let level = self.exit_level(cx, cy, cz);
        self.trap_checks = tc as i64;
        self.exit_samples[level as usize] += 1;
        let shut = level == 0;
        if shut && !self.was_shut { self.shut_episodes += 1; if self.first_shut == 0 { self.first_shut = self.ticks; } }
        self.was_shut = shut;
    }

    /// Hurt (a magma floor, a brush with a campfire): get off whatever does it and wait until the damage has mended
    /// (the world is peaceful: health comes back by itself).
    fn guard_health(&mut self) { let _tg = super::nav::tag("guard_health");
        if self.c.pl.health >= 10.0 || self.c.pl.health <= 0.0 { return; }
        self.retreats += 1;
        if self.verbose && self.retreats <= 5 { self.log(&format!("hurt (health {:?}): stepping aside to recover", self.c.pl.health)); }
        if !self.rope.is_empty() { self.undo_rope(); }
        let (bx, by, bz) = (self.cx(), self.cy(), self.cz());
        if !self.safe_spot(bx, by, bz) {
            let mut best: i32 = -1;
            let mut best_d = i32::MAX;
            for dy in -3..=3i32 { for dz in -8..=8i32 { for dx in -8..=8i32 {
                let (x, y, z) = (bx + dx, by + dy, bz + dz);
                if !self.g.inside(x, y, z) || !self.safe_spot(x, y, z) { continue; }
                let d = dx.abs() + dz.abs() + 2 * dy.abs();
                if d < best_d && (d < 6 || { let n = self.try_count; self.try_count += 1; n < 40 }) && self.search(x, y, z, 800) && self.nav.scaffold_moves == 0 && self.nav.tower_moves == 0 { best_d = d; best = self.g.idx(x, y, z) as i32; }
            } } }
            self.try_count = 0;
            if best >= 0 && { let b = best as usize; self.search(self.g.x_of(b), self.g.y_of(b), self.g.z_of(b), 800) } { self.travel(); }
        }
        let mut n = 0;
        while n < 500 && self.c.pl.health < 18.0 && self.c.pl.health > 0.0 { self.tick(); n += 1; }
    }

    fn safe_spot(&self, x: i32, y: i32, z: i32) -> bool {
        if self.g.stand16(x, y, z) < 0 { return false; }
        !hot(self.g.get(x, y - 1, z)) && !hot(self.g.get(x, y, z))
    }

    pub fn surroundings(&self, i: usize) -> i32 {
        let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
        let mut h: i32 = 17;
        let g = &self.g;
        if x > g.x0 && x < g.x0 + g.nx - 1 && y > g.y0 && y < g.y0 + g.ny - 1 && z > g.z0 && z < g.z0 + g.nz - 1 {
            // all 27 cells are inside: read the grid and the scaffold bit of the mirror directly
            let pi = g.pidx(x, y, z) as isize;
            let (sy, sz) = ((g.nx * g.nz) as isize, g.nx as isize);
            for dy in -1..=1isize { for dz in -1..=1isize {
                let row = (i as isize + dy * sy + dz * sz) as usize;
                let prow = pi + dy * g.pys as isize + dz * g.pzs as isize;
                for dx in -1..=1isize {
                    let v = if g.nv((prow + dx * g.pxs as isize) as usize) & super::grid::N_SCAF != 0 { 0 } else { g.cur[(row as isize + dx) as usize] as i32 };
                    h = h.wrapping_mul(31).wrapping_add(v);
                }
            } }
            return h;
        }
        for dy in -1..=1 { for dz in -1..=1 { for dx in -1..=1 {
            let (nx, ny, nz) = (x + dx, y + dy, z + dz);
            let v = if self.g.inside(nx, ny, nz) && self.scaffold.contains(self.g.idx(nx, ny, nz) as i32) { 0 } else { self.g.get(nx, ny, nz) as i32 };
            h = h.wrapping_mul(31).wrapping_add(v);
        } } }
        h
    }

    pub fn do_cell(&mut self, i: usize) -> i32 {
        let r = self.do_cell_inner(i);
        if self.causes.is_some() {
            let (why, ticks) = (self.defer_why, self.ticks);
            let c = self.causes.as_mut().unwrap();
            if r == DEFER { c.defer.insert(i as i32, why); c.first.entry(i as i32).or_insert(ticks); } else if r == PLACED { c.placed.insert(i as i32); }
        }
        r
    }
    fn do_cell_inner(&mut self, i: usize) -> i32 {
        let tb = t();
        if self.waits_for_door(i) { self.defer_why = "door order"; return DEFER; }
        self.check_scaffold();
        let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
        let tg = self.g.tgt[i];
        let sp = self.spec(tg);
        if !sp.placeable { self.skip(i, &format!("cannot be placed by hand: {}", tb.binfo(tg).name)); return SKIP; }
        let w = self.g.cur[i];
        if w != 0 {
            if self.scaffold.contains(i as i32) {
                let kept = self.kept_for_exit;
                if !self.remove_scaffold(i) {
                    if self.kept_for_exit > kept { self.defer_why = "own way out stands in the cell"; return DEFER; }
                    self.skip(i, "scaffold in the way"); return SKIP;
                }
            }
            else if sp.multi && same_block(w, tg) { self.defer_why = "top-up failed"; return if self.top_up(x, y, z, tg, &sp) { PLACED } else { DEFER }; }
            else if sp.content != 0 && w == sp.first { self.fill_pot(x, y, z, tg, &sp); if self.matches(self.g.get(x, y, z), tg) { return PLACED; } self.skip(i, "pot could not be filled"); return SKIP; }
            else if tb.loose[w as usize] == tb.loose[tg as usize] {
                let mut attempt = 0;
                while attempt < 4 && !self.matches(self.g.get(x, y, z), tg) {
                    attempt += 1;
                    if !self.c.on_ground() { self.settle(); }
                    if !self.aim_at_block(x, y, z) {
                        let st = self.plan_stance(x, y, z, None, true);
                        let Some(st) = st else { break };
                        if !self.go_to(&st) { break; }
                        if !self.aim_at_block(x, y, z) { self.bad_stand.insert(self.g.idx_raw(st.x, st.y, st.z)); continue; }
                    }
                    self.adjust(x, y, z, tg);
                }
                if self.matches(self.g.get(x, y, z), tg) { return PLACED; }
                self.skip(i, "could not be set right"); return SKIP;
            }
            else { self.skip(i, "occupied by another block"); return SKIP; }
        }
        if self.count(sp.item) == 0 || (self.count(self.scaffold_item) < 12 && !self.stranded) {
            if self.stranded && self.count(sp.item) == 0 && self.ticks - self.stranded_tick < 1200 { self.defer_why = "stranded"; return DEFER; }
            self.must_fetch = sp.item;
            self.restock(y);
            self.must_fetch = 0;
            if self.count(sp.item) == 0 {
                self.skip(i, if self.stranded { "stranded without material" } else { "out of stock" }); return SKIP;
            }
        }
        if !self.clear_below(x, y, z) && self.no_roofing { self.defer_why = "scaffold below"; return DEFER; }
        if tb.scaffolding[tg as usize] {
            let mut held = false;
            let below = self.g.get(x, y - 1, z) as usize;
            let mine_below = self.g.inside(x, y - 1, z) && self.scaffold.contains(self.g.idx(x, y - 1, z) as i32);
            if !mine_below && (tb.scaffolding[below] || tb.sturdy_top[below]) { held = true; }
            for d in 0..4 {
                if held { break; }
                let (nx, nz) = (x + DXH[d], z + DZH[d]);
                held = self.g.is_tower(nx, y, nz) && !self.scaffold.contains(self.g.idx_raw(nx, y, nz));
            }
            if !held { self.defer_why = "target scaffolding unsupported"; return DEFER; }
        }
        if kind_of(tg) == Kind::TntBlock && rules::has_neighbor_signal(&self.c.w, crate::geom::p(x, y, z)) {
            self.skip(i, "would be set off"); return SKIP;
        }
        if tb.gravity[tg as usize] {
            let below = self.g.get(x, y - 1, z) as usize;
            if below == 0 || tb.walk_through[below] || tb.replaceable[below] || (self.g.inside(x, y - 1, z) && self.scaffold.contains(self.g.idx(x, y - 1, z) as i32)) { self.defer_why = "gravity block without support"; return DEFER; }
        }
        let mut r = self.attempt(x, y, z, tg, &sp, true);
        // (the last resorts below wait until everything else stands: see place_helper)
        let no_face = crate::rev::rev().a1 && self.main_done && r == DEFER && self.defer_why == "no face to click from anywhere";
        if r == DEFER {
            // no face to click that gives the wanted state: lend it one with a temporary block, then take that away
            // From `close` on, once everything else stands (no_face): with the helper in place also the fine search
            // of place_close - spots off the middle of a block, sneaking or not - and if this helper gives nothing,
            // the next side of the cell gets one. (An observer whose back is built up: only a helper on one
            // particular side, seen from one particular spot, gives the click that makes it look the right way.)
            let wide = crate::rev::rev().close;
            self.helper_skip = 0;
            loop {
                let helper = self.place_helper(x, y, z, &sp);
                if helper < 0 { break; }
                let base = self.helper_base;
                let dir = self.helper_dir;
                self.helper_cell = helper;
                r = self.attempt(x, y, z, tg, &sp, false);
                if wide && no_face && r == DEFER {
                    self.close_failed.remove(&(i as i32));
                    if self.place_close(x, y, z, tg, &sp) { self.from_close += 1; self.helped_close += 1; r = PLACED; }
                }
                self.helper_cell = -1;
                self.must_remove = true;
                self.remove_scaffold(helper as usize);
                if base >= 0 { self.remove_scaffold(base as usize); }
                self.must_remove = false;
                if r == PLACED { self.helped += 1; if base >= 0 { self.chained += 1; } }
                if !(wide && no_face) || r != DEFER || dir < 0 { break; }
                self.helper_skip |= 1 << dir;
            }
            self.helper_skip = 0;
        }
        // still nowhere to click from: from inside the cell itself, if the thing is thin enough to stand beside
        if r == DEFER && no_face && self.place_from_inside(x, y, z, tg, &sp) { self.from_inside += 1; r = PLACED; }
        // ... or from right beside it, off the middle of the block and sneaking (things that want a steep view)
        if r == DEFER && no_face && self.place_close(x, y, z, tg, &sp) { self.from_close += 1; r = PLACED; }
        r
    }

    /// Tries to place the cell from here or from a planned stance.
    pub fn attempt(&mut self, x: i32, y: i32, z: i32, tg: u16, sp: &Spec, may_escape: bool) -> i32 {
        self.want_held = sp.item;
        self.seal_refused = false;
        if self.body_in(x, y, z) {
            if self.self_place(x, y, z, tg, sp) { return PLACED; }
        } else if self.place_from_here(x, y, z, tg, sp) { return PLACED; }
        let r = self.attempt_stances(x, y, z, tg, sp, may_escape);
        self.avoid_seal = 0;
        r
    }

    fn attempt_stances(&mut self, x: i32, y: i32, z: i32, tg: u16, sp: &Spec, may_escape: bool) -> i32 {
        for _attempt in 0..3 {
            self.avoid_seal = if self.seal_refused && !self.accept_traps { sp.first } else { 0 };
            let mut st = self.plan_stance(x, y, z, Some(sp), false);
            if may_escape && st.is_none() && self.dbg_click + self.dbg_pillar_click > 0 && self.dbg_nav == 0 && self.g.stand16(self.cx(), self.cy(), self.cz()) > 0 && { let b = self.escape_budget; self.escape_budget -= 1; b > 0 } && self.escape_from_half_floor() {
                self.unreachable.clear();
                st = self.plan_stance(x, y, z, Some(sp), false);
            }
            if may_escape && st.is_none() && self.dbg_click + self.dbg_pillar_click > 0 && self.dbg_nav == 0 {
                self.shut_in += 1;
                if self.shut_in == 3 { self.unreachable.clear(); st = self.plan_stance(x, y, z, Some(sp), false); }
            } else if st.is_some() { self.shut_in = 0; }
            let Some(st) = st else {
                self.plan_fails += 1;
                self.defer_why = if self.seal_refused { "would shut the bot in" } else if self.dbg_click + self.dbg_pillar_click == 0 { "no face to click from anywhere" } else { "no way to a spot" };
                return DEFER;
            };
            if !self.go_to(&st) { self.replans += 1; self.bad_stand.insert(self.g.idx_raw(st.x, st.y, st.z)); self.defer_why = "could not get to the spot"; continue; }
            if self.body_in(x, y, z) { if self.self_place(x, y, z, tg, sp) { self.bad_stand.clear(); return PLACED; } }
            else if self.place_from_here(x, y, z, tg, sp) { self.bad_stand.clear(); self.end_lean(); return PLACED; }
            self.end_lean();
            self.bad_stand.insert(self.g.idx_raw(st.x, st.y, st.z) ^ (if st.lean >= 0 { (st.lean + 1) << 28 } else { 0 }));
            self.defer_why = if self.seal_refused { "would shut the bot in" } else { "click failed at the spot" };
        }
        DEFER
    }

    /// Anything that turned up where it does not belong and is not known scaffolding is treated like scaffolding from now on.
    pub fn reconcile(&mut self) {
        let tb = t();
        if !self.changed_again.is_empty() {
            let ca = std::mem::take(&mut self.changed_again);
            for &i in &ca { if self.needs(i as usize) && !self.skipped[i as usize] { self.deferred.add(i); } }
        }
        if self.appeared.is_empty() { return; }
        let ap = std::mem::take(&mut self.appeared);
        for &i in &ap {
            let iu = i as usize;
            let w = self.g.cur[iu];
            if w == 0 || self.scaffold.contains(i) { continue; }
            let (x, y, z) = (self.g.x_of(iu), self.g.y_of(iu), self.g.z_of(iu));
            if y < self.g.oy { continue; }
            let tg = self.g.tgt[iu];
            let wrong = !tb.technical[w as usize] && !tb.secondary[w as usize]
                && (if tg == 0 { !self.is_supply_chest(x, y, z) } else { !same_kind(w, tg) && !same_family(w, tg) && w != self.spec(tg).first });
            if !wrong { continue; }
            self.strays += 1;
            if self.verbose && self.strays < 20 { self.log(&format!("STRAY {} at {} (target {})", tb.name(w), self.rel(x, y, z), tb.name(tg))); }
            self.scaffold.add(i);
            self.g.set_nav_flag(iu, super::grid::N_SCAF, true);
            let c = self.col(x, z);
            self.scaffold_in_column[c] += 1;
        }
    }
}
