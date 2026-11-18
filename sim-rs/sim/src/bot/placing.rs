//! Putting a block into a cell from where the bot stands (the Java version's `Placing`).
use super::state::{same_kind, shoot_for, Bot, Spec};
use super::*;
use crate::kinds::Kind;
use crate::oracle;
use crate::tables::*;

const INSIDE: [f64; 7] = [0.5, 0.3, 0.7, 0.12, 0.88, 0.04, 0.96];
const NEAR: [f64; 3] = [0.0, -0.19, 0.19];

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
        if self.watch.is_some() && self.watching(x, y, z) { let fl = self.oracle.floats(sp.first); let m = format!("helper asked: haveScaffold={} count={} floats={} mainDone={} chainNow={} pistons={}", self.have_scaffold, self.count(self.scaffold_item), fl, self.main_done, self.helper_chain_now, self.any_piston); self.wlog(&m); }
        if !self.have_scaffold || self.count(self.scaffold_item) == 0 || !self.oracle.floats(sp.first) { return -1; }
        let ss = self.spec(self.scaffold_state);
        self.helper_base = -1;
        let mut direct = false;
        for d in 0..6 {
            if self.helper_skip & (1 << d) != 0 { continue; }
            let (nx, ny, nz) = (x + DX[d], y + DY[d], z + DZ[d]);
            if self.watch.is_some() && self.watching(x, y, z) { let (so, fw) = (self.scaffold_ok(nx, ny, nz), self.face_works(sp, OPP[d])); let m = format!("helper d={d}: inside={} aboveOrigin={} empty={} ({}) scaffoldOk={so} faceWorks={fw}", self.g.inside(nx, ny, nz), ny >= self.g.oy, self.g.get(nx, ny, nz) == 0, t().name(self.g.get(nx, ny, nz))); self.wlog(&m); }
            // (candidate `hang`: the helper may stand in a cell that is still to be built - it is taken away again
            // at once. Two things that each want the other's side to be clicked - a pair of end rods tip to tip -
            // have no other cell for it.)
            let lend = self.hang(8) && self.hang_retry && self.g.inside(nx, ny, nz) && ny >= self.g.oy && self.g.get(nx, ny, nz) == 0 && self.g.tgt[self.g.idx(nx, ny, nz)] != 0 && !self.piston_faces(nx, ny, nz);
            if !self.g.inside(nx, ny, nz) || ny < self.g.oy || self.g.get(nx, ny, nz) != 0 || !(self.scaffold_ok(nx, ny, nz) || lend) { continue; }
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
            direct = true;
            if self.attempt(nx, ny, nz, self.scaffold_state, &ss, false) == PLACED {
                self.add_scaffold(nx, ny, nz);
                self.helper_dir = d as i32;
                return self.g.idx(nx, ny, nz) as i32;
            }
        }
        if !crate::rev::rev().a1 { return -1; }
        // (Only once everything else stands: before that, most such cells get a neighbour in time. Not in machines:
        // a block too many sets them off.)
        self.helper_dir = -1;
        // (from `chain` on: in builds with pistons too, once everything else stands - before that a block too many sets
        // a machine off)
        let machines_too = self.main_done && crate::rev::rev().chain;
        if direct || !(self.main_done || self.helper_chain_now) || self.count(self.scaffold_item) < 2 || (!machines_too && self.has_pistons()) { return -1; }
        // Nothing beside the cell that a helper could be put against (a log that must stand upright, out on a
        // branch with nothing under or over it): a second helper, one step further, to carry the first.
        let mut tries = 0;
        for d in 0..6 {
            if tries >= 3 { break; }
            let (nx, ny, nz) = (x + DX[d], y + DY[d], z + DZ[d]);
            if !self.g.inside(nx, ny, nz) || ny < self.g.oy || self.g.get(nx, ny, nz) != 0 || !self.scaffold_ok(nx, ny, nz) || !self.face_works(sp, OPP[d]) { continue; }
            for e in 0..6 {
                if tries >= 3 { break; }
                let (bx, by, bz) = (nx + DX[e], ny + DY[e], nz + DZ[e]);
                if (bx == x && by == y && bz == z) || !self.g.inside(bx, by, bz) || by < self.g.oy || self.g.get(bx, by, bz) != 0 || !self.scaffold_ok(bx, by, bz) { continue; }
                let mut anchored = false;
                for f in 0..6 {
                    if anchored { break; }
                    let (ax, ay, az) = (bx + DX[f], by + DY[f], bz + DZ[f]);
                    if (ax == x && ay == y && az == z) || (ax == nx && ay == ny && az == nz) { continue; }
                    let w = self.g.get(ax, ay, az) as usize;
                    anchored = w != 0 && !tb.replaceable[w] && !tb.out_boxes_f32[w].is_empty();
                }
                if !anchored { continue; }
                tries += 1;
                if self.attempt(bx, by, bz, self.scaffold_state, &ss, false) != PLACED { continue; }
                self.add_scaffold(bx, by, bz);
                if self.attempt(nx, ny, nz, self.scaffold_state, &ss, false) == PLACED {
                    self.add_scaffold(nx, ny, nz);
                    self.helper_base = self.g.idx(bx, by, bz) as i32;
                    return self.g.idx(nx, ny, nz) as i32;
                }
                self.must_remove = true;
                self.remove_scaffold(self.g.idx(bx, by, bz));
                self.must_remove = false;
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

    /// Could the bot stand in the middle of this cell while the block goes in around it? A thin thing at the edge of
    /// its cell - a door flat against a wall, a ladder, an open trapdoor - or anything one can walk through.
    fn inside_placeable(&self, x: i32, y: i32, z: i32, sp: &Spec) -> bool {
        let tb = t();
        if sp.multi || sp.chest || sp.content != 0 || tb.full_cube[sp.first as usize] { return false; }
        if self.g.stand16(x, y, z) < 0 { return false; }
        if tb.walk_through[sp.first as usize] { return true; }
        let b = &tb.out_boxes_f32[sp.first as usize];
        let mut k = 0;
        while k < b.len() {
            // (the body is 0.6 wide)
            if (b[k] as f64) < 0.805 && (b[k + 3] as f64) > 0.195 && (b[k + 2] as f64) < 0.805 && (b[k + 5] as f64) > 0.195 { return false; }
            k += 6;
        }
        true
    }

    /// A point on the side of the neighbour in direction d that looks into cell (x,y,z), at (u, v) across that face.
    #[inline]
    fn face_point(x: i32, y: i32, z: i32, d: usize, u: f64, v: f64) -> (f64, f64, f64) {
        let (mut ax, mut ay, mut az) = (x as f64 + 0.5 + 0.5 * DX[d] as f64, y as f64 + 0.5 + 0.5 * DY[d] as f64, z as f64 + 0.5 + 0.5 * DZ[d] as f64);
        if DX[d] != 0 { ay = y as f64 + u; az = z as f64 + v; } else if DY[d] != 0 { ax = x as f64 + u; az = z as f64 + v; } else { ax = x as f64 + u; ay = y as f64 + v; }
        (ax, ay, az)
    }

    /// Is there a click that gives the wanted state for a bot standing in the middle of the cell? (Asked before going there.)
    fn inside_click(&mut self, x: i32, y: i32, z: i32, sp: &Spec) -> bool {
        let tb = t();
        let (ex, ey, ez) = (x as f64 + 0.5, y as f64 + self.g.stand16(x, y, z) as f64 / 16.0 + 1.62, z as f64 + 0.5);
        for d in 0..6 {
            let (nx, ny, nz) = (x + DX[d], y + DY[d], z + DZ[d]);
            let s = self.g.get(nx, ny, nz) as usize;
            if s == 0 || tb.replaceable[s] || tb.interactive[s] { continue; }
            for &u in &INSIDE { for &v in &INSIDE {
                let (ax, ay, az) = Self::face_point(x, y, z, d, u, v);
                let (dx, dy, dz) = (ax - ex, ay - ey, az - ez);
                let hd = (dx * dx + dz * dz).sqrt();
                if hd < 1e-6 || (dx * dx + dy * dy + dz * dz).sqrt() > self.reach - 0.05 { continue; }
                if !self.ray.cast(&self.g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, self.reach) || self.ray.hx != nx || self.ray.hy != ny || self.ray.hz != nz || self.ray.face != OPP[d] as i32 { continue; }
                let (yaw, pitch) = (yaw_to(dx, dz), pitch_to(dy, hd));
                let pred = self.oracle.predict(&self.c.w, sp.item, crate::geom::p(nx, ny, nz), OPP[d] as u8, [ax, ay, az], false, yaw, pitch, false);
                let pa = self.oracle.placed_at;
                if pred != 0 && tb.loose[pred as usize] == tb.loose[sp.first as usize] && pa.x == x && pa.y == y && pa.z == z { return true; }
            } }
        }
        false
    }

    /// The last way of placing something: from inside its own cell. Where a door stands flat against a wall, the
    /// side one would normally place it from is that wall; the same click (looking at the wall, on the floor) can
    /// only be made standing where the door goes. Every click is put to the rules first, as everywhere else.
    pub fn place_from_inside(&mut self, x: i32, y: i32, z: i32, tg: u16, sp: &Spec) -> bool {
        let tb = t();
        if !self.inside_placeable(x, y, z, sp) || !self.inside_click(x, y, z, sp) { return false; }
        if self.seals_in(x, y, z, sp.first) { self.seal_refused = true; return false; }
        if !(self.cx() == x && self.cy() == y && self.cz() == z && self.c.on_ground()) {
            self.end_lean();
            if !self.rope.is_empty() { self.undo_rope(); }
            if !self.route(x, y, z, 20000) || !self.travel() { return false; }
        }
        if !(self.cx() == x && self.cy() == y && self.cz() == z) || !self.centre(x, y, z) { return false; }
        self.settle();
        if !self.hold(sp.item) { return false; }
        let needs_support = !self.oracle.floats(sp.first);
        let e = self.c.eye();
        for d in 0..6 {
            let (nx, ny, nz) = (x + DX[d], y + DY[d], z + DZ[d]);
            let s = self.g.get(nx, ny, nz) as usize;
            if s == 0 || tb.replaceable[s] || tb.interactive[s] { continue; }
            if needs_support && self.g.inside(nx, ny, nz) && self.scaffold.contains(self.g.idx(nx, ny, nz) as i32) { continue; }
            for &u in &INSIDE { for &v in &INSIDE {
                // a point on the side of the neighbour that looks into the cell
                let (ax, ay, az) = Self::face_point(x, y, z, d, u, v);
                let (dx, dy, dz) = (ax - e[0], ay - e[1], az - e[2]);
                let hd = (dx * dx + dz * dz).sqrt();
                if (dx * dx + dy * dy + dz * dz).sqrt() > self.reach - 0.05 { continue; }
                let yaw = if hd < 1e-6 { self.c.yaw() } else { yaw_to(dx, dz) };
                let pitch = pitch_to(dy, hd);
                let Some(hit) = self.c.pick(yaw, pitch) else { continue };
                if hit.pos.x != nx || hit.pos.y != ny || hit.pos.z != nz || hit.face as usize != OPP[d] { continue; }
                let pred = self.oracle.predict(&self.c.w, sp.item, hit.pos, hit.face, hit.loc, hit.inside, yaw, pitch, false);
                let pa = self.oracle.placed_at;
                if pred == 0 || tb.loose[pred as usize] != tb.loose[sp.first as usize] || pa.x != x || pa.y != y || pa.z != z { continue; }
                self.c.look(yaw, pitch);
                self.c.click_use();
                self.tick();
                let now = self.g.get(x, y, z);
                if now == 0 { self.failed_clicks += 1; continue; }
                self.placed += 1;
                self.ev_placed(x, y, z);
                if !same_kind(now, tg) && tb.loose[now as usize] != tb.loose[sp.first as usize] {
                    self.misplaced += 1;
                    self.log(&format!("MISPLACED {} in {} wanted {} (from inside) predicted={}", tb.name(now), self.rel(x, y, z), tb.name(tg), tb.name(pred)));
                    return false;
                }
                let g2 = self.g.get(x, y, z);
                if !self.matches(g2, tg) && tb.loose[g2 as usize] == tb.loose[tg as usize] { self.adjust(x, y, z, tg); }
                return true;
            } }
        }
        false
    }

    /// Is there a spot right beside the cell - not just the middle of a block, and sneaking or not - from which some
    /// click gives the wanted state? This is the fine-grained search for things that take their direction from the
    /// view and want it steep (an observer looking up, a dropper or piston pointing down, when the cell is low and
    /// the eyes are almost level with it). Result in close_sx.. / close_x, close_z / close_sneak.
    fn plan_close(&mut self, x: i32, y: i32, z: i32, sp: &Spec) -> bool {
        self.close_pit = false;
        // (from `close` on: from two below the cell's level up to one above it, and from up to two blocks away - also
        // the things that take their direction from a level view: an observer or dropper whose back is built up)
        let wide = crate::rev::rev().close;
        let (top, bottom, r) = if wide { (y + 1, y - 3, 2) } else { (y - 1, y - 2, 1) };
        let mut sy = top;
        while sy >= bottom {
            for ox in -r..=r { for oz in -r..=r {
                if ox == 0 && oz == 0 { continue; }
                let (sx, sz) = (x + ox, z + oz);
                let h = self.g.stand16(sx, sy, sz);
                if h < 0 || self.g.is_tower(sx, sy - 1, sz) || self.bad_stand.contains(&self.g.idx_raw(sx, sy, sz)) { continue; }
                for sn in 0..2 { for &px in &NEAR { for &pz in &NEAR {
                    let (ex, ey, ez) = (sx as f64 + 0.5 + px, sy as f64 + h as f64 / 16.0 + (if sn == 1 { 1.27 } else { 1.62 }), sz as f64 + 0.5 + pz);
                    if self.close_aim(x, y, z, sp, ex, ey, ez, sn == 1, false) {
                        self.close_sx = sx; self.close_sy = sy; self.close_sz = sz; self.close_x = sx as f64 + 0.5 + px; self.close_z = sz as f64 + 0.5 + pz; self.close_sneak = sn == 1;
                        return true;
                    }
                } } }
            } }
            sy -= 1;
        }
        // A cell on the ground: the eyes are above it wherever the bot stands. What a player does then is dig a
        // hole beside it and look up from in there. (One block of the ground, where nothing is to be built.)
        if y != self.g.oy { return false; }
        for ox in -1..=1 { for oz in -1..=1 {
            if ox == 0 && oz == 0 { continue; }
            let (sx, sz, sy) = (x + ox, z + oz, self.g.y0);
            if !self.g.inside(sx, sy, sz) || sz < self.g.oz - 1 || self.g.get(sx, sy, sz) != self.g.ground_state || self.g.get(sx, sy + 1, sz) != 0 || self.g.get(sx, sy + 2, sz) != 0 { continue; }
            if self.g.target(sx, sy + 1, sz) != 0 || self.g.stand16(sx, sy + 1, sz) != 0 { continue; }
            let pi = self.g.idx(sx, sy, sz);
            self.g.put(pi, 0);
            let mut found = false;
            'pit: for sn in 0..2 { for &px in &NEAR { for &pz in &NEAR {
                let (ex, ey, ez) = (sx as f64 + 0.5 + px, sy as f64 + (if sn == 1 { 1.27 } else { 1.62 }), sz as f64 + 0.5 + pz);
                if self.close_aim(x, y, z, sp, ex, ey, ez, sn == 1, false) {
                    self.close_sx = sx; self.close_sy = sy; self.close_sz = sz; self.close_x = sx as f64 + 0.5 + px; self.close_z = sz as f64 + 0.5 + pz; self.close_sneak = sn == 1;
                    self.close_pit = true;
                    found = true;
                    break 'pit;
                }
            } } }
            self.g.put(pi, self.g.ground_state);
            if found { return true; }
        } }
        false
    }

    /// One eye position against every face around the cell; with act, the click is made (through the game's own ray cast).
    #[allow(clippy::too_many_arguments)]
    fn close_aim(&mut self, x: i32, y: i32, z: i32, sp: &Spec, ex: f64, ey: f64, ez: f64, sneak: bool, act: bool) -> bool {
        let tb = t();
        for d in 0..6 {
            let (nx, ny, nz) = (x + DX[d], y + DY[d], z + DZ[d]);
            let s = self.g.get(nx, ny, nz) as usize;
            if s == 0 || tb.replaceable[s] || (tb.interactive[s] && !sneak) { continue; }
            if self.g.inside(nx, ny, nz) && self.scaffold.contains(self.g.idx(nx, ny, nz) as i32) && !self.oracle.floats(sp.first) { continue; }
            // the face looks into the cell: the eye has to be on that side of it
            let side = if DX[d] != 0 { (x as f64 + 0.5 + 0.5 * DX[d] as f64 - ex) * DX[d] as f64 } else if DY[d] != 0 { (y as f64 + 0.5 + 0.5 * DY[d] as f64 - ey) * DY[d] as f64 } else { (z as f64 + 0.5 + 0.5 * DZ[d] as f64 - ez) * DZ[d] as f64 };
            if side < 0.02 { continue; }
            for &u in &INSIDE { for &v in &INSIDE {
                let (ax, ay, az) = Self::face_point(x, y, z, d, u, v);
                let (dx, dy, dz) = (ax - ex, ay - ey, az - ez);
                let hd = (dx * dx + dz * dz).sqrt();
                if hd < 1e-6 || (dx * dx + dy * dy + dz * dz).sqrt() > self.reach - 0.05 { continue; }
                let (yaw, pitch) = (yaw_to(dx, dz), pitch_to(dy, hd));
                let pred = if act {
                    let Some(hit) = self.c.pick(yaw, pitch) else { continue };
                    if hit.pos.x != nx || hit.pos.y != ny || hit.pos.z != nz || hit.face as usize != OPP[d] { continue; }
                    self.oracle.predict(&self.c.w, sp.item, hit.pos, hit.face, hit.loc, hit.inside, yaw, pitch, sneak)
                } else {
                    if !self.ray.cast(&self.g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, self.reach) || self.ray.hx != nx || self.ray.hy != ny || self.ray.hz != nz || self.ray.face != OPP[d] as i32 { continue; }
                    self.oracle.predict(&self.c.w, sp.item, crate::geom::p(nx, ny, nz), OPP[d] as u8, [ax, ay, az], false, yaw, pitch, sneak)
                };
                let pa = self.oracle.placed_at;
                if pred == 0 || tb.loose[pred as usize] != tb.loose[sp.first as usize] || pa.x != x || pa.y != y || pa.z != z { continue; }
                if !act { return true; }
                self.c.look(yaw, pitch);
                self.c.click_use();
                self.tick();
                if self.g.get(x, y, z) != 0 { return true; }
                self.failed_clicks += 1;
            } }
        }
        false
    }

    /// See plan_close: goes to the spot found there and places the block.
    pub fn place_close(&mut self, x: i32, y: i32, z: i32, tg: u16, sp: &Spec) -> bool {
        let tb = t();
        if sp.multi || sp.chest || sp.content != 0 { return false; }
        let i = self.g.idx(x, y, z);
        let sur = self.surroundings(i);
        if self.close_failed.get(&(i as i32)) == Some(&sur) { return false; }
        self.reach = 4.5;
        if !self.plan_close(x, y, z, sp) { self.close_failed.insert(i as i32, sur); return false; }
        if self.seals_in(x, y, z, sp.first) { self.seal_refused = true; return false; }
        let (sx, sy, sz) = (self.close_sx, self.close_sy, self.close_sz);
        if self.close_pit {
            // onto the spot, then the block underfoot goes
            self.end_lean();
            if !self.rope.is_empty() { self.undo_rope(); }
            if !(self.cx() == sx && self.cy() == sy + 1 && self.cz() == sz && self.c.on_ground()) && (!self.route(sx, sy + 1, sz, 20000) || !self.travel()) { return false; }
            if !(self.cx() == sx && self.cy() == sy + 1 && self.cz() == sz) || !self.centre(sx, sy + 1, sz) { return false; }
            self.settle();
            if !self.break_block(sx, sy, sz, sx as f64 + 0.5, sy as f64 + 1.0, sz as f64 + 0.5, 200) { return false; }
            self.pits += 1;
            let mut n = 0;
            while n < 20 && !(self.cy() == sy && self.c.on_ground()) { self.tick(); n += 1; }
        } else if !(self.cx() == sx && self.cy() == sy && self.cz() == sz && self.c.on_ground()) {
            self.end_lean();
            if !self.rope.is_empty() { self.undo_rope(); }
            if !self.route(sx, sy, sz, 20000) || !self.travel() { return false; }
        }
        if !(self.cx() == sx && self.cy() == sy && self.cz() == sz) || !self.centre(sx, sy, sz) || !self.hold(sp.item) { return false; }
        self.settle();
        // the last few tenths of a block, slowly
        for _ in 0..40 {
            let (dx, dz) = (self.close_x - self.c.x(), self.close_z - self.c.z());
            let dist = (dx * dx + dz * dz).sqrt();
            if dist < 0.05 { break; }
            self.c.look(yaw_to(dx, dz), self.c.pitch());
            self.c.keys(self.hspeed() < dist, false, false, false, false, true, false);
            self.tick();
        }
        self.c.keys(false, false, false, false, false, self.close_sneak, false);
        let mut n = 0;
        while n < 6 && (self.hspeed() > 0.003 || self.c.sneaking() != self.close_sneak) { self.tick(); n += 1; }
        self.tick();
        let e = self.c.eye();
        let ok = self.cy() == sy && self.close_aim(x, y, z, sp, e[0], e[1], e[2], self.c.sneaking(), true);
        self.c.release_keys();
        self.tick();
        self.centre(sx, sy, sz);
        if !ok { self.bad_stand.insert(self.g.idx_raw(sx, sy, sz)); return false; }
        self.placed += 1;
        self.ev_placed(x, y, z);
        let now = self.g.get(x, y, z);
        if !same_kind(now, tg) && tb.loose[now as usize] != tb.loose[sp.first as usize] {
            self.misplaced += 1;
            self.log(&format!("MISPLACED {} in {} wanted {} (from close by)", tb.name(now), self.rel(x, y, z), tb.name(tg)));
            return false;
        }
        if !self.matches(now, tg) && tb.loose[now as usize] == tb.loose[tg as usize] { self.adjust(x, y, z, tg); }
        true
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
