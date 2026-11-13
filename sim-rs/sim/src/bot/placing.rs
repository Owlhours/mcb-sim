//! Putting a block into a cell from where the bot stands (the Java version's `Placing`).
use super::state::{same_kind, shoot_for, Bot, Spec};
use super::*;
use crate::kinds::Kind;
use crate::oracle;
use crate::tables::*;

impl Bot {
    /// Does clicking the given face of a neighbour ever give the wanted state?
    pub fn face_works(&self, sp: &Spec, face: usize) -> bool {
        let Some(tbl) = &sp.table else { return true };
        let want = t().loose[sp.first as usize];
        let from = oracle::index(face, 0, 0, 0, false);
        let to = oracle::index(face, 3, oracle::NYAW - 1, oracle::NPITCH - 1, true);
        for k in from..=to { let e = tbl[k]; if e != 0 && t().loose[e as usize] == want { return true; } }
        false
    }

    /// Puts a scaffold block into an empty neighbouring cell of (x,y,z) whose face would let the wanted block be placed.
    pub fn place_helper(&mut self, x: i32, y: i32, z: i32, sp: &Spec) -> i32 {
        let tb = t();
        if !self.have_scaffold || self.count(self.scaffold_item) == 0 || !self.oracle.floats(sp.first) { return -1; }
        let ss = self.spec(self.scaffold_state);
        for d in 0..6 {
            let (nx, ny, nz) = (x + DX[d], y + DY[d], z + DZ[d]);
            if !self.g.inside(nx, ny, nz) || ny < self.g.oy || self.g.get(nx, ny, nz) != 0 || !self.scaffold_ok(nx, ny, nz) { continue; }
            if !self.face_works(sp, OPP[d]) { continue; }
            let mut anchored = false;
            for e in 0..6 {
                if anchored { break; }
                let (ax, ay, az) = (nx + DX[e], ny + DY[e], nz + DZ[e]);
                if ax == x && ay == y && az == z { continue; }
                let w = self.g.get(ax, ay, az) as usize;
                anchored = w != 0 && !tb.replaceable[w] && !tb.out_boxes_f32[w].is_empty();
            }
            if !anchored { continue; }
            if self.attempt(nx, ny, nz, self.scaffold_state, &ss, false) == PLACED {
                self.add_scaffold(nx, ny, nz);
                return self.g.idx(nx, ny, nz) as i32;
            }
        }
        -1
    }

    /// Would this exact click put the wanted state into cell (x,y,z)?
    pub fn exact_click(&mut self, x: i32, y: i32, z: i32, want: u16, nx: i32, ny: i32, nz: i32, face: usize, ax: f64, ay: f64, az: f64, yaw: f32, pitch: f32, sneak: bool) -> bool {
        let tb = t();
        let it = tb.item[want as usize];
        if it == 0 { return true; }
        let believed = self.g.get(nx, ny, nz);
        let q = crate::geom::p(nx, ny, nz);
        if believed == 0 || self.c.w.get(q) != believed { return true; }
        let pred = self.oracle.predict(&self.c.w, it, q, face as u8, [ax, ay, az], false, yaw, pitch, sneak);
        let pa = self.oracle.placed_at;
        pred != 0 && tb.loose[pred as usize] == tb.loose[want as usize] && pa.x == x && pa.y == y && pa.z == z
    }

    pub fn place_from_here(&mut self, x: i32, y: i32, z: i32, tg: u16, sp: &Spec) -> bool {
        let wt = self.watching(x, y, z);
        if self.seals_in(x, y, z, sp.first) { self.seal_refused = true; if wt { self.wlog("placeFromHere: would seal the bot in"); } return false; }
        if self.rescue_towers(x, y, z, sp.first) { if wt { self.wlog("placeFromHere: went to rescue a column"); } return false; }
        if !self.c.on_ground() || self.hspeed() > 0.003 { self.settle(); }
        if !self.hold2(sp.item, !self.leaning && !self.c.sneaking()) { if wt { self.wlog("placeFromHere: cannot hold the item"); } return false; }
        if self.try_place(x, y, z, tg, sp) { return true; }
        if wt { self.wlog("placeFromHere: tryPlace found nothing"); }
        if self.leaning || self.c.sneaking() { return false; }
        let mut wanted = false;
        for d in 0..6 { if wanted { break; } wanted = t().interactive[self.g.get(x + DX[d], y + DY[d], z + DZ[d]) as usize]; }
        if !wanted || self.on_tower() { return false; }
        self.c.keys(false, false, false, false, false, true, false);
        self.tick(); self.tick();
        let ok = self.try_place(x, y, z, tg, sp);
        self.c.release_keys();
        self.tick();
        ok
    }

    pub fn try_place(&mut self, x: i32, y: i32, z: i32, tg: u16, sp: &Spec) -> bool {
        let tb = t();
        let e = self.c.eye();
        let sneak = self.c.sneaking();
        let needs_support = !self.oracle.floats(sp.first);
        for d in 0..6 {
            if needs_support && self.g.inside(x + DX[d], y + DY[d], z + DZ[d]) && self.scaffold.contains(self.g.idx(x + DX[d], y + DY[d], z + DZ[d]) as i32) { continue; }
            if !self.find_click(x, y, z, sp.first, sp.table.as_ref().map(|v| v.as_slice()), e[0], e[1], e[2], sneak, 1 << d) { continue; }
            if !self.hold2(sp.item, !self.leaning && !sneak) { return false; }
            let cl = self.click.clone();
            let wt = self.watching(x, y, z);
            if wt { self.wlog(&format!("tryPlace: click on {} face {} aim {:.3},{:.3},{:.3} yaw {} pitch {}", self.rel(cl.nx, cl.ny, cl.nz), cl.face, cl.ax - self.g.ox as f64, cl.ay - self.g.oy as f64, cl.az - self.g.oz as f64, cl.yaw, cl.pitch)); }
            let Some(hit) = self.c.pick(cl.yaw, cl.pitch) else { if wt { self.wlog("tryPlace: crosshair on nothing"); } continue };
            if hit.pos.x != cl.nx || hit.pos.y != cl.ny || hit.pos.z != cl.nz || hit.face as i32 != cl.face {
                if wt { self.wlog(&format!("tryPlace: crosshair is on {} face {} ({})", self.rel(hit.pos.x, hit.pos.y, hit.pos.z), hit.face, t().name(self.c.w.get(hit.pos)))); }
                continue;
            }
            let pred = self.oracle.predict(&self.c.w, sp.item, hit.pos, hit.face, hit.loc, hit.inside, cl.yaw, cl.pitch, sneak);
            let pa = self.oracle.placed_at;
            if pred == 0 || (tb.loose[pred as usize] != tb.loose[sp.first as usize] && !shoot_for(pred, sp.first)) || pa.x != x || pa.y != y || pa.z != z {
                if wt { self.wlog(&format!("tryPlace: click would give {} at {}", tb.name(pred), self.rel(pa.x, pa.y, pa.z))); }
                continue;
            }
            if sp.chest && tb.canon[pred as usize] != tb.canon[self.chest_want(x, y, z, tg) as usize] { continue; }
            self.c.look(cl.yaw, cl.pitch);
            self.c.click_use();
            if sneak { self.c.sneak(true); }
            self.tick();
            let now = self.g.get(x, y, z);
            if wt { self.wlog(&format!("tryPlace: clicked; cell now {} (world {})", tb.name(now), tb.name(self.c.w.get(crate::geom::p(x, y, z))))); }
            if now == 0 { self.failed_clicks += 1; continue; }
            self.placed += 1;
            self.ev_placed(x, y, z);
            if !same_kind(now, tg) && tb.loose[now as usize] != tb.loose[sp.first as usize] && !shoot_for(now, tg) {
                self.misplaced += 1;
                let e2 = self.c.eye();
                if self.find_break(x, y, z, e2[0], e2[1], e2[2]) { let c2 = self.click.clone(); self.break_block(x, y, z, c2.ax, c2.ay, c2.az, 400); }
                return false;
            }
            if sp.multi && !self.matches(now, tg) { self.top_up(x, y, z, tg, sp); }
            if sp.content != 0 { self.fill_pot(x, y, z, tg, sp); }
            let g2 = self.g.get(x, y, z);
            if !self.matches(g2, tg) && tb.loose[g2 as usize] == tb.loose[tg as usize] { self.adjust(x, y, z, tg); }
            return true;
        }
        false
    }

    /// Uses the plant on the empty pot just placed.
    pub fn fill_pot(&mut self, x: i32, y: i32, z: i32, tg: u16, sp: &Spec) {
        if self.matches(self.g.get(x, y, z), tg) || self.count(sp.content) == 0 { return; }
        let was_sneak = self.c.sneaking();
        if !self.hold(sp.content) { return; }
        if self.c.sneaking() { self.c.release_keys(); self.tick(); self.tick(); }
        let mut attempt = 0;
        while attempt < 3 && !self.matches(self.g.get(x, y, z), tg) {
            attempt += 1;
            if !self.c.on_ground() { self.settle(); }
            if !self.aim_at_block(x, y, z) {
                if self.leaning { break; }
                let st = self.plan_stance(x, y, z, None, true);
                let Some(st) = st else { break };
                if !self.go_to(&st) { break; }
                if !self.hold(sp.content) || !self.aim_at_block(x, y, z) { self.bad_stand.insert(self.g.idx(st.x, st.y, st.z) as i32); continue; }
            }
            self.c.click_use();
            self.tick();
        }
        if was_sneak { self.c.keys(false, false, false, false, false, true, false); self.tick(); self.tick(); }
    }

    /// Turns the view to some point of block (x,y,z) that the crosshair really rests on from where the bot stands.
    pub fn aim_at_block(&mut self, x: i32, y: i32, z: i32) -> bool {
        let s = self.g.get(x, y, z) as usize;
        if s == 0 { return false; }
        let b = &t().out_boxes_f32[s];
        let e = self.c.eye();
        let mut k = 0;
        while k < b.len() {
            // (int + float is float arithmetic in the original)
            let (x0, y0, z0, x1, y1, z1) = ((x as f32 + b[k]) as f64, (y as f32 + b[k + 1]) as f64, (z as f32 + b[k + 2]) as f64, (x as f32 + b[k + 3]) as f64, (y as f32 + b[k + 4]) as f64, (z as f32 + b[k + 5]) as f64);
            k += 6;
            let (mx, my, mz) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (z0 + z1) / 2.0);
            let pts = [[mx, my, mz], [mx, y1 - 0.01, mz], [mx, y0 + 0.01, mz], [x0 + 0.01, my, mz], [x1 - 0.01, my, mz], [mx, my, z0 + 0.01], [mx, my, z1 - 0.01],
                [x0 + 0.1, my, z0 + 0.1], [x1 - 0.1, my, z0 + 0.1], [x0 + 0.1, my, z1 - 0.1], [x1 - 0.1, my, z1 - 0.1]];
            for pt in pts {
                let (dx, dy, dz) = (pt[0] - e[0], pt[1] - e[1], pt[2] - e[2]);
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                let hd = (dx * dx + dz * dz).sqrt();
                if dist > 4.45 || dist < 1e-6 { continue; }
                let yaw = if hd < 1e-6 { self.c.yaw() } else { yaw_to(dx, dz) };
                let pitch = pitch_to(dy, hd);
                if let Some(h) = self.c.pick(yaw, pitch) {
                    if h.pos.x == x && h.pos.y == y && h.pos.z == z { self.c.look(yaw, pitch); return true; }
                }
            }
        }
        false
    }

    /// Right-clicks a placed block until its click-settable properties match.
    pub fn adjust(&mut self, x: i32, y: i32, z: i32, tg: u16) {
        let tb = t();
        if kind_of(tg) == Kind::ChestBlock { return; }
        if self.body_in(x, y, z) && !tb.walk_through[tg as usize] { return; }
        let was_sneak = self.c.sneaking();
        if was_sneak { self.c.release_keys(); self.tick(); self.tick(); }
        for _ in 0..26 {
            let now = self.g.get(x, y, z);
            if now == 0 || self.matches(now, tg) || tb.loose[now as usize] != tb.loose[tg as usize] { break; }
            if !self.aim_at_block(x, y, z) { break; }
            self.c.click_use();
            self.tick();
            if self.g.get(x, y, z) == now { self.tick(); if self.g.get(x, y, z) == now { break; } }
        }
    }

    /// Further clicks on a block already there until it has the wanted count.
    pub fn top_up(&mut self, x: i32, y: i32, z: i32, tg: u16, sp: &Spec) -> bool {
        for _ in 0..8 {
            let now = self.g.get(x, y, z);
            if self.matches(now, tg) { return true; }
            if now == 0 || self.count(sp.item) == 0 || !self.hold(sp.item) { return false; }
            if self.top_up_click(x, y, z, tg, sp, now) { continue; }
            self.plan_top_up = tg;
            let st = self.plan_stance(x, y, z, Some(sp), true);
            self.plan_top_up = 0;
            let Some(st) = st else { return false };
            if !self.go_to(&st) { return false; }
            if !self.hold(sp.item) || !self.top_up_click(x, y, z, tg, sp, now) { return false; }
        }
        self.matches(self.g.get(x, y, z), tg)
    }

    /// Could a click from the given eye position add to the block in (x,y,z)?
    pub fn top_up_visible(&mut self, x: i32, y: i32, z: i32, tg: u16, sp: &Spec, ex: f64, ey: f64, ez: f64) -> bool {
        let tb = t();
        let now = self.g.get(x, y, z);
        if now == 0 { return false; }
        let at = tb.any_top[now as usize];
        let ay = y as f64 + if at == 0 { 0.1 } else { at as f64 / 16.0 };
        let q = crate::geom::p(x, y, z);
        if ey > ay + 0.05 {
            for k in 0..9 {
                let (ax, az) = (x as f64 + 0.5 + (k % 3 - 1) as f64 * 0.3, z as f64 + 0.5 + (k / 3 - 1) as f64 * 0.3);
                let (dx, dy, dz) = (ax - ex, ay - ey, az - ez);
                let hd = (dx * dx + dz * dz).sqrt();
                if (dx * dx + dy * dy + dz * dz).sqrt() > self.reach - 0.1 { continue; }
                if !self.ray.cast(&self.g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, self.reach) || self.ray.hx != x || self.ray.hy != y || self.ray.hz != z || self.ray.face != 1 { continue; }
                let yaw = if hd < 1e-6 { 0f32 } else { yaw_to(dx, dz) };
                let pitch = pitch_to(dy, hd);
                let pred = self.oracle.predict(&self.c.w, sp.item, q, crate::geom::UP, [ax, ay, az], false, yaw, pitch, false);
                let pa = self.oracle.placed_at;
                if pred != 0 && pred != now && same_block(pred, tg) && pa == q { return true; }
            }
        }
        for d in 0..6 {
            if !self.find_click(x, y, z, tg, None, ex, ey, ez, false, 1 << d) { continue; }
            let cl = self.click.clone();
            let pred = self.oracle.predict(&self.c.w, sp.item, crate::geom::p(cl.nx, cl.ny, cl.nz), cl.face as u8, [cl.ax, cl.ay, cl.az], false, cl.yaw, cl.pitch, false);
            let pa = self.oracle.placed_at;
            if pred != 0 && pred != now && same_block(pred, tg) && pa == q { return true; }
        }
        false
    }

    pub fn top_up_click(&mut self, x: i32, y: i32, z: i32, tg: u16, sp: &Spec, now: u16) -> bool {
        if !self.c.on_ground() || self.hspeed() > 0.003 { self.settle(); }
        let e = self.c.eye();
        let at = t().any_top[now as usize];
        for k in 0..9 {
            let (ax, az) = (x as f64 + 0.5 + (k % 3 - 1) as f64 * 0.3, z as f64 + 0.5 + (k / 3 - 1) as f64 * 0.3);
            let mut ay = y as f64 + at as f64 / 16.0;
            if at == 0 { ay = y as f64 + 0.1; }
            let (dx, dy, dz) = (ax - e[0], ay - e[1], az - e[2]);
            let hd = (dx * dx + dz * dz).sqrt();
            if (dx * dx + dy * dy + dz * dz).sqrt() > self.reach - 0.1 { continue; }
            let yaw = if hd < 1e-6 { self.c.yaw() } else { yaw_to(dx, dz) };
            let pitch = pitch_to(dy, hd);
            if self.top_up_try(x, y, z, tg, sp, now, yaw, pitch) { return true; }
        }
        for d in 0..6 {
            let sn = self.c.sneaking();
            if !self.find_click(x, y, z, tg, None, e[0], e[1], e[2], sn, 1 << d) { continue; }
            let cl = self.click.clone();
            if self.top_up_try(x, y, z, tg, sp, now, cl.yaw, cl.pitch) { return true; }
        }
        false
    }

    fn top_up_try(&mut self, x: i32, y: i32, z: i32, tg: u16, sp: &Spec, now: u16, yaw: f32, pitch: f32) -> bool {
        let Some(hit) = self.c.pick(yaw, pitch) else { return false };
        if t().interactive[self.g.get(hit.pos.x, hit.pos.y, hit.pos.z) as usize] && !self.c.sneaking() { return false; }
        let sn = self.c.sneaking();
        let pred = self.oracle.predict(&self.c.w, sp.item, hit.pos, hit.face, hit.loc, hit.inside, yaw, pitch, sn);
        let pa = self.oracle.placed_at;
        if pred == 0 || pred == now || pa.x != x || pa.y != y || pa.z != z { return false; }
        if !same_block(pred, tg) { return false; }
        self.c.look(yaw, pitch);
        self.c.click_use();
        if self.c.sneaking() { self.c.sneak(true); }
        self.tick();
        if self.g.get(x, y, z) == now { return false; }
        self.placed += 1;
        self.ev_placed(x, y, z);
        true
    }

    fn straight_down_yaw(&self, sp: &Spec) -> Option<usize> {
        let tbl = sp.table.as_ref()?;
        let tb = t();
        for yc in 0..oracle::NYAW {
            let mut ok = true;
            for q in 0..4 {
                let st = tbl[oracle::index(1, q, yc, oracle::pitch_class(90.0), false)];
                if st == 0 || tb.loose[st as usize] != tb.loose[sp.first as usize] { ok = false; break; }
            }
            if ok { return Some(yc); }
        }
        None
    }

    pub fn self_placeable(&self, x: i32, y: i32, z: i32, sp: &Spec) -> bool {
        let tb = t();
        if !tb.full_cube[sp.first as usize] || sp.multi { return false; }
        if self.g.stand16(x, y, z) != 0 || !self.g.passable(x, y + 2, z) { return false; }
        let below = self.g.get(x, y - 1, z) as usize;
        if !tb.out_full[below] || tb.interactive[below] || tb.replaceable[below] { return false; }
        if sp.table.is_none() { return true; }
        self.straight_down_yaw(sp).is_some()
    }

    /// The bot stands in the cell: jump and place the block underfoot.
    pub fn self_place(&mut self, x: i32, y: i32, z: i32, tg: u16, sp: &Spec) -> bool {
        let tb = t();
        if !tb.full_cube[sp.first as usize] || sp.multi { return false; }
        if self.cy() != y || self.g.stand16(x, y, z) != 0 || !self.g.passable(x, y + 2, z) { return false; }
        let below = self.g.get(x, y - 1, z) as usize;
        if !tb.out_full[below] || tb.interactive[below] || tb.replaceable[below] { return false; }
        let mut yaw = self.c.yaw();
        if sp.table.is_some() {
            match self.straight_down_yaw(sp) { Some(f) => yaw = oracle::yaw_centre(f), None => return false }
        }
        if !self.hold(sp.item) || !self.centre(x, y, z) { return false; }
        self.settle();
        let y0 = self.c.y();
        self.c.look(yaw, 90.0);
        self.c.keys(false, false, false, false, true, false, false);
        let mut ok = false;
        for n in 0..14 {
            if !ok && self.c.y() >= y0 + 1.0 && self.g.get(x, y, z) == 0 {
                if let Some(hit) = self.c.pick(yaw, 90.0) {
                    if hit.pos.x == x && hit.pos.y == y - 1 && hit.pos.z == z {
                        let pred = self.oracle.predict(&self.c.w, sp.item, hit.pos, hit.face, hit.loc, hit.inside, yaw, 90.0, false);
                        if pred != 0 && tb.loose[pred as usize] == tb.loose[sp.first as usize] { self.c.click_use(); }
                    }
                }
            }
            self.tick();
            if n == 0 { self.c.jump(false); }
            if self.g.get(x, y, z) != 0 { ok = true; }
            if self.c.on_ground() && n > 2 { break; }
        }
        self.c.release_keys();
        if ok {
            self.placed += 1;
            self.ev_placed(x, y, z);
            self.settle();
            if sp.multi { self.top_up(x, y, z, tg, sp); }
            let g2 = self.g.get(x, y, z);
            if !self.matches(g2, tg) && tb.loose[g2 as usize] == tb.loose[tg as usize] { self.adjust(x, y, z, tg); }
        }
        ok && self.g.get(x, y, z) != 0
    }

    /// Before leaving a spot: place whatever else is allowed and within reach from here.
    pub fn sweep_here(&mut self) {
        if !self.col_next_set || self.sweeping { return; }
        self.sweeping = true;
        let mut again = true;
        let mut rounds = 0;
        while again && rounds < 40 {
            rounds += 1;
            again = false;
            let (bx, by, bz) = (self.cx(), self.cy(), self.cz());
            for dz in -4..=4 { for dx in -4..=4 {
                let (x, z) = (bx + dx, bz + dz);
                if x < self.g.ox || z < self.g.oz || x >= self.g.ox + self.g.sx || z >= self.g.oz + self.g.sz { continue; }
                let y = self.col_next[self.col(x, z)];
                if y == NONE || y > by + 5 || y < by - 5 || !self.eligible(x, z) { continue; }
                let i = self.g.idx(x, y, z);
                let tg = self.g.tgt[i];
                let sp = self.spec(tg);
                if self.watching(x, y, z) { self.wlog(&format!("sweepHere: placeable={} cur={} have={} bodyIn={} scafInCol={}", sp.placeable, self.g.cur[i], self.count(sp.item), self.body_in(x, y, z), self.scaffold_in_column[self.col(x, z)])); }
                if !sp.placeable || self.g.cur[i] != 0 || self.count(sp.item) == 0 || self.body_in(x, y, z) { continue; }
                if self.scaffold_in_column[self.col(x, z)] != 0 && self.loose_below(x, y, z) { continue; }
                if self.watch.is_some() { self.wlog(&format!("sweepHere tries {} {}", self.rel(x, y, z), t().name(tg))); }
                if self.place_from_here(x, y, z, tg, &sp) {
                    self.blocked.remove(i as i32);
                    let ny = self.scan_column(x, y + 1, z);
                    self.set_next(x, z, ny);
                    again = true;
                }
            }
            }
        }
        self.sweeping = false;
    }
}
