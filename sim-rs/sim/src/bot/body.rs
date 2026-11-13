//! Low-level skills (the Java version's `Body`): keeping the grid in sync, walking a path, aiming and clicking,
//! holding the right item.
use super::state::{Bot, Click};
use super::*;
use crate::oracle;
use crate::sim::CLICK_SWAP;
use crate::tables::*;

// aim offsets from the face centre
const FREE: [[f64; 2]; 9] = [[0.0, 0.0], [0.3, 0.3], [-0.3, 0.3], [0.3, -0.3], [-0.3, -0.3], [0.42, 0.0], [-0.42, 0.0], [0.0, 0.42], [0.0, -0.42]];
const QUAD: [[f64; 2]; 12] = [[0.25, 0.25], [-0.25, 0.25], [0.25, -0.25], [-0.25, -0.25], [0.4, 0.4], [-0.4, 0.4], [0.4, -0.4], [-0.4, -0.4], [0.1, 0.25], [-0.1, 0.25], [0.1, -0.25], [-0.1, -0.25]];

/// MCSIM_BODY=from,to: one line per tick in that range with the body's position (build coordinates), velocity,
/// onGround, yaw and pitch after the tick.
fn body_trace() -> Option<(i64, i64)> {
    static V: std::sync::OnceLock<Option<(i64, i64)>> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MCSIM_BODY").ok().map(|v| { let p: Vec<i64> = v.split(',').map(|x| x.parse().unwrap()).collect(); (p[0], p[1]) }))
}

impl Bot {
    // ------------------------------------------------------------------ time & sync
    pub fn body_tick(&mut self) {
        if self.ticks >= self.max_ticks { std::panic::panic_any(OutOfTime); }
        self.c.tick();
        self.ticks += 1;
        if let Some((a, b)) = body_trace() {
            if self.ticks >= a && self.ticks <= b {
                let pl = &self.c.pl;
                println!("BODY {} {:?} {:?} {:?} {:?} {:?} {:?} {} {:?} {:?} sneak={}", self.ticks, pl.pos[0] - self.g.ox as f64, pl.pos[1] - self.g.oy as f64, pl.pos[2] - self.g.oz as f64,
                    pl.vel[0], pl.vel[1], pl.vel[2], pl.on_ground, self.c.yaw(), self.c.pitch(), pl.shift_key_down);
            }
        }
        self.sync();
    }

    fn sync(&mut self) {
        if self.c.changes.is_empty() { return; }
        let ch = std::mem::take(&mut self.c.changes);
        for &(x, y, z, s) in &ch { self.on_block_change(x, y, z, s); }
        let mut ch = ch;
        ch.clear();
        self.c.changes = ch;
    }

    pub fn idle(&mut self, n: i32) { self.c.release_keys(); for _ in 0..n { self.tick(); } }

    // ------------------------------------------------------------------ where am I
    #[inline]
    pub fn cx(&self) -> i32 { self.c.x().floor() as i32 }
    #[inline]
    pub fn cz(&self) -> i32 { self.c.z().floor() as i32 }
    #[inline]
    pub fn cy(&self) -> i32 { (self.c.y() + 1e-4).floor() as i32 }
    pub fn hspeed(&self) -> f64 { let v = self.c.pl.vel; (v[0] * v[0] + v[2] * v[2]).sqrt() }
    pub fn feet_of(&self, x: i32, y: i32, z: i32) -> f64 { let h = self.g.stand16(x, y, z); if h < 0 { f64::NAN } else { y as f64 + h as f64 / 16.0 } }

    pub fn look_at(&mut self, tx: f64, ty: f64, tz: f64) {
        let e = self.c.eye();
        let (dx, dy, dz) = (tx - e[0], ty - e[1], tz - e[2]);
        let h = (dx * dx + dz * dz).sqrt();
        let yaw = if h < 1e-6 { self.c.yaw() } else { yaw_to(dx, dz) };
        self.c.look(yaw, pitch_to(dy, h));
    }

    // ------------------------------------------------------------------ walking
    /// Walks/jumps/drops through cells path[from..n) (x,y,z triples). False if the body got stuck or ended up elsewhere.
    pub fn follow(&mut self, path: &[i32], from: usize, n: usize) -> bool {
        let mut i = from;
        self.failed_step = -1;
        let mut stuck = 0;
        let (mut last_x, mut last_z, mut last_y) = (self.c.x(), self.c.z(), self.c.y());
        let mut budget: i32 = 40 + 30 * (n as i32 - from as i32);
        for k in from.max(1)..n { budget += 9 * 0.max(path[k * 3 - 2] - path[k * 3 + 1]); }
        self.c.hold_attack(false);
        while i < n {
            let (wx, wy, wz) = (path[i * 3], path[i * 3 + 1], path[i * 3 + 2]);
            let last = i == n - 1;
            let (tx, tz) = (wx as f64 + 0.5, wz as f64 + 0.5);
            let (dx, dz) = (tx - self.c.x(), tz - self.c.z());
            let dist = (dx * dx + dz * dz).sqrt();
            let feet = self.feet_of(wx, wy, wz);
            self.failed_step = i as i32;
            if feet.is_nan() { self.c.release_keys(); return false; }
            let at_height = self.c.on_ground() && (self.c.y() - feet).abs() < 0.02;
            if at_height && self.cx() == wx && self.cz() == wz && (if last { dist < 0.18 && self.hspeed() < 0.03 } else { dist < 0.38 }) { i += 1; continue; }
            budget -= 1;
            if budget < 0 { self.c.release_keys(); return false; }
            let (mut fwd, mut jump, mut sneak) = (true, false, false);
            let rise = feet - self.c.y();
            if self.c.on_ground() {
                if rise > 0.6 {
                    if dist < 1.25 { jump = true; }
                } else if last && rise > -0.6 {
                    let coast = self.hspeed() * 2.2;
                    if dist <= coast + 0.02 { fwd = false; }
                    else if dist < 0.45 && !self.on_tower() { sneak = true; }
                }
            }
            let mut sprint = false;
            if fwd && !sneak && !jump && self.c.on_ground() && i + 3 < n {
                sprint = true;
                let (ddx, ddz) = (path[(i + 1) * 3] - wx, path[(i + 1) * 3 + 2] - wz);
                let mut k = i;
                while k < i + 3 && sprint {
                    if path[(k + 1) * 3 + 1] != wy || path[(k + 1) * 3] - path[k * 3] != ddx || path[(k + 1) * 3 + 2] - path[k * 3 + 2] != ddz { sprint = false; }
                    k += 1;
                }
                if sprint && (dx.abs() > 0.3 && ddx == 0 || dz.abs() > 0.3 && ddz == 0) { sprint = false; }
            }
            let pitch = self.c.pitch();
            self.c.look(yaw_to(dx, dz), pitch);
            self.c.keys(fwd, false, false, false, jump, sneak, sprint);
            self.tick();
            let moved = (self.c.x() - last_x).abs() + (self.c.z() - last_z).abs() + (self.c.y() - last_y).abs();
            if moved < 1e-4 { stuck += 1; if stuck > 25 { self.c.release_keys(); return false; } } else { stuck = 0; }
            last_x = self.c.x(); last_z = self.c.z(); last_y = self.c.y();
            // fell somewhere we did not mean to go?
            let pi = if i == 0 { 0 } else { i - 1 };
            if self.c.on_ground() && self.c.y() < feet - 0.6 && self.c.y() < jmin(feet, self.feet_of(path[pi * 3], path[pi * 3 + 1], path[pi * 3 + 2])) - 0.6
                && !(self.cx() == wx && self.cz() == wz) { self.c.release_keys(); return false; }
        }
        self.c.release_keys();
        true
    }

    /// `follow` for the single cell kept in `self.one`.
    pub fn follow_one(&mut self) -> bool { let one = self.one; self.follow(&one, 0, 1) }

    /// True if what carries the bot is (or may be) a scaffolding block.
    pub fn on_tower(&self) -> bool {
        let y = (self.c.y() - 0.02).floor() as i32;
        let (px, pz) = (self.c.x(), self.c.z());
        for k in 0..4 {
            let x = (px + if k & 1 == 0 { -0.3 } else { 0.3 }).floor() as i32;
            let z = (pz + if k & 2 == 0 { -0.3 } else { 0.3 }).floor() as i32;
            if self.g.is_tower(x, y, z) { return true; }
        }
        false
    }

    /// Shuffles to the middle of column (x,z).
    pub fn middle(&mut self, x: i32, z: i32) -> bool {
        for _ in 0..40 {
            let (dx, dz) = (x as f64 + 0.5 - self.c.x(), z as f64 + 0.5 - self.c.z());
            let dist = (dx * dx + dz * dz).sqrt();
            if dist < 0.12 && self.hspeed() < 0.01 { self.c.release_keys(); return true; }
            let pitch = self.c.pitch();
            self.c.look(yaw_to(dx, dz), pitch);
            self.c.keys(dist > self.hspeed() * 2.2 + 0.05, false, false, false, false, false, false);
            self.tick();
        }
        self.c.release_keys();
        (x as f64 + 0.5 - self.c.x()).abs() < 0.2 && (z as f64 + 0.5 - self.c.z()).abs() < 0.2
    }

    /// Inside a scaffolding column: rise until the feet are at level to_y.
    pub fn climb(&mut self, x: i32, to_y: i32, z: i32) -> bool {
        self.c.hold_attack(false);
        let budget = 30 + 12 * 0.max(to_y - self.cy());
        let mut n = 0;
        while n < budget && self.c.y() < to_y as f64 {
            let (dx, dz) = (x as f64 + 0.5 - self.c.x(), z as f64 + 0.5 - self.c.z());
            let drift = dx * dx + dz * dz > 0.01;
            if drift { let pitch = self.c.pitch(); self.c.look(yaw_to(dx, dz), pitch); }
            self.c.keys(drift, false, false, false, true, false, false);
            self.tick();
            n += 1;
        }
        self.c.release_keys();
        let mut n = 0;
        while n < 30 && !(self.c.on_ground() && self.hspeed() < 0.003) { self.tick(); n += 1; }
        self.c.on_ground() && self.cy() == to_y && self.cx() == x && self.cz() == z
    }

    /// On or inside a scaffolding column: sink until the feet are at level to_y.
    pub fn descend(&mut self, x: i32, to_y: i32, z: i32) -> bool {
        self.c.hold_attack(false);
        let budget = 30 + 10 * 0.max(self.cy() - to_y);
        self.c.keys(false, false, false, false, false, true, false);
        let mut n = 0;
        while n < budget && self.c.y() > to_y as f64 + 0.155 { self.tick(); n += 1; }
        self.c.release_keys();
        let mut n = 0;
        while n < 30 && !(self.c.on_ground() && self.hspeed() < 0.003) { self.tick(); n += 1; }
        self.c.on_ground() && self.cy() == to_y && self.cx() == x && self.cz() == z
    }

    /// Lets the body come to rest.
    pub fn settle(&mut self) {
        if self.hold_sneak { self.c.keys(false, false, false, false, false, true, false); } else { self.c.release_keys(); }
        let mut i = 0;
        while i < 40 && !(self.c.on_ground() && self.hspeed() < 0.003) { self.tick(); i += 1; }
    }

    // ------------------------------------------------------------------ items
    pub fn count(&self, item: u16) -> i32 { self.c.pl.count(item) }
    pub fn free_slots(&self) -> i32 { self.c.pl.free_slots() }

    pub fn hold(&mut self, item: u16) -> bool { self.hold2(item, true) }

    pub fn hold2(&mut self, item: u16, may_open_inventory: bool) -> bool {
        let sel = self.c.pl.selected;
        let cur = self.c.pl.inv[sel];
        if cur.count > 0 && cur.item == item {
            self.hotbar_use += 1;
            self.hotbar_stamp[sel] = self.hotbar_use;
            self.c.select_slot(sel);
            return true;
        }
        for i in 0..9 {
            let s = self.c.pl.inv[i];
            if s.count > 0 && s.item == item { self.c.select_slot(i); self.hotbar_use += 1; self.hotbar_stamp[i] = self.hotbar_use; return true; }
        }
        if !may_open_inventory { return false; }
        for i in 9..36 {
            let s = self.c.pl.inv[i];
            if s.count > 0 && s.item == item {
                let mut best = 0usize;
                let mut best_stamp = i64::MAX;
                for h in 0..9 {
                    let st = if self.c.pl.inv[h].count == 0 { -1 } else { self.hotbar_stamp[h] };
                    if st < best_stamp { best_stamp = st; best = h; }
                }
                self.c.release_keys();
                self.inventory_open_by_me = true;
                self.c.open_inventory(); self.tick();
                self.c.click_slot(i as i32, best as i32, CLICK_SWAP); self.tick();
                self.c.close_screen(); self.tick();
                self.inventory_open_by_me = false;
                self.c.select_slot(best);
                self.hotbar_use += 1;
                self.hotbar_stamp[best] = self.hotbar_use;
                let now = self.c.pl.inv[best];
                return now.count > 0 && now.item == item;
            }
        }
        false
    }

    pub fn held_item(&self) -> u16 { let s = self.c.pl.held(); if s.count == 0 { 0 } else { s.item } }

    // ------------------------------------------------------------------ clicking
    /// Finds a way to put the wanted state into cell (x,y,z) by clicking a face of one of its six neighbours, as seen
    /// from the eye position. Result in `self.click`.
    pub fn find_click(&mut self, x: i32, y: i32, z: i32, want: u16, table: Option<&[u16]>, ex: f64, ey: f64, ez: f64, sneak: bool, face_mask: i32) -> bool {
        let tb = t();
        let want_canon = tb.loose[want as usize];
        for d in 0..6usize {
            if face_mask & (1 << d) == 0 { continue; }
            let (nx, ny, nz) = (x + DX[d], y + DY[d], z + DZ[d]);
            let s = self.g.get(nx, ny, nz) as usize;
            if s == 0 || tb.replaceable[s] { continue; }
            if tb.interactive[s] && !sneak { continue; }
            let boxes = &tb.out_boxes_f32[s];
            if boxes.is_empty() { continue; }
            let face = OPP[d];
            let offs: &[[f64; 2]] = if table.is_none() { &FREE } else { &QUAD };
            let mut b = 0;
            while b < boxes.len() {
                let bb = b;
                b += 6;
                let (plane, u0, u1, v0, v1): (f64, f64, f64, f64, f64);
                // (int + float is float arithmetic in the original)
                match face {
                    0 => { plane = (ny as f32 + boxes[bb + 1]) as f64; if ey >= plane - 0.02 { continue; } u0 = boxes[bb] as f64; u1 = boxes[bb + 3] as f64; v0 = boxes[bb + 2] as f64; v1 = boxes[bb + 5] as f64; }
                    1 => { plane = (ny as f32 + boxes[bb + 4]) as f64; if ey <= plane + 0.02 { continue; } u0 = boxes[bb] as f64; u1 = boxes[bb + 3] as f64; v0 = boxes[bb + 2] as f64; v1 = boxes[bb + 5] as f64; }
                    2 => { plane = (nz as f32 + boxes[bb + 2]) as f64; if ez >= plane - 0.02 { continue; } u0 = boxes[bb] as f64; u1 = boxes[bb + 3] as f64; v0 = boxes[bb + 1] as f64; v1 = boxes[bb + 4] as f64; }
                    3 => { plane = (nz as f32 + boxes[bb + 5]) as f64; if ez <= plane + 0.02 { continue; } u0 = boxes[bb] as f64; u1 = boxes[bb + 3] as f64; v0 = boxes[bb + 1] as f64; v1 = boxes[bb + 4] as f64; }
                    4 => { plane = (nx as f32 + boxes[bb]) as f64; if ex >= plane - 0.02 { continue; } u0 = boxes[bb + 2] as f64; u1 = boxes[bb + 5] as f64; v0 = boxes[bb + 1] as f64; v1 = boxes[bb + 4] as f64; }
                    _ => { plane = (nx as f32 + boxes[bb + 3]) as f64; if ex <= plane + 0.02 { continue; } u0 = boxes[bb + 2] as f64; u1 = boxes[bb + 5] as f64; v0 = boxes[bb + 1] as f64; v1 = boxes[bb + 4] as f64; }
                }
                if u1 - u0 < 0.05 || v1 - v0 < 0.05 { continue; }
                for o in offs {
                    let u = (u0 + 0.02).max((u1 - 0.02).min(0.5 + o[0]));
                    let v = (v0 + 0.02).max((v1 - 0.02).min(0.5 + o[1]));
                    let (ax, ay, az);
                    if face < 2 { ax = nx as f64 + u; az = nz as f64 + v; ay = plane; }
                    else if face < 4 { ax = nx as f64 + u; ay = ny as f64 + v; az = plane; }
                    else { az = nz as f64 + u; ay = ny as f64 + v; ax = plane; }
                    let quad = (if u > 0.5 { 1 } else { 0 }) | (if v > 0.5 { 2 } else { 0 });
                    let (dx, dy, dz) = (ax - ex, ay - ey, az - ez);
                    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                    if dist > self.reach - 0.05 { continue; }
                    let hd = (dx * dx + dz * dz).sqrt();
                    let yaw = if hd < 1e-6 { 0f32 } else { yaw_to(dx, dz) };
                    let pitch = pitch_to(dy, hd);
                    if let Some(tbl) = table {
                        let st = tbl[oracle::index(face, quad, oracle::yaw_class(yaw), oracle::pitch_class(pitch), sneak)];
                        if st == 0 || tb.loose[st as usize] != want_canon { continue; }
                    }
                    if self.ray.cast(&self.g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, self.reach) && self.ray.hx == nx && self.ray.hy == ny && self.ray.hz == nz && self.ray.face == face as i32 {
                        if table.is_some() && !self.exact_click(x, y, z, want, nx, ny, nz, face, ax, ay, az, yaw, pitch, sneak) { continue; }
                        self.click = Click { nx, ny, nz, face: face as i32, ax, ay, az, yaw, pitch };
                        return true;
                    }
                    if u1 - u0 < 0.3 && v1 - v0 < 0.3 { break; }
                }
            }
        }
        tb.scaffolding[want as usize] && self.find_scaffold_click(x, y, z, want, ex, ey, ez, sneak)
    }

    /// Scaffolding goes by rules of its own (see the original).
    fn find_scaffold_click(&mut self, x: i32, y: i32, z: i32, want: u16, ex: f64, ey: f64, ez: f64, sneak: bool) -> bool {
        if sneak { return false; }
        self.ray.scaffold_solid = true;
        let mut found = false;
        for k in 0..8 {
            let (bx, by, bz, face): (i32, i32, i32, usize);
            let (ax, ay, az): (f64, f64, f64);
            if k < 4 {
                bx = x; by = y - 1; bz = z; face = 2 + k;
                if !self.g.is_tower(bx, by, bz) { continue; }
                ax = bx as f64 + 0.5 + if face == 4 { -0.5 } else if face == 5 { 0.5 } else { 0.0 };
                az = bz as f64 + 0.5 + if face == 2 { -0.5 } else if face == 3 { 0.5 } else { 0.0 };
                ay = by as f64 + 0.5;
            } else {
                let d = k - 4;
                bx = x + if d == 0 { 1 } else if d == 1 { -1 } else { 0 };
                bz = z + if d == 2 { 1 } else if d == 3 { -1 } else { 0 };
                by = y; face = 1;
                if !self.g.is_tower(bx, by, bz) { continue; }
                ax = bx as f64 + 0.5; az = bz as f64 + 0.5; ay = by as f64 + 1.0;
            }
            let (dx, dy, dz) = (ax - ex, ay - ey, az - ez);
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            let hd = (dx * dx + dz * dz).sqrt();
            if dist > self.reach - 0.05 || dist < 1e-6 { continue; }
            let yaw = if hd < 1e-6 { 0f32 } else { yaw_to(dx, dz) };
            let pitch = pitch_to(dy, hd);
            if !self.ray.cast(&self.g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, self.reach) || self.ray.hx != bx || self.ray.hy != by || self.ray.hz != bz || self.ray.face != face as i32 { continue; }
            if !self.exact_click(x, y, z, want, bx, by, bz, face, ax, ay, az, yaw, pitch, false) { continue; }
            self.click = Click { nx: bx, ny: by, nz: bz, face: face as i32, ax, ay, az, yaw, pitch };
            found = true;
            break;
        }
        self.ray.scaffold_solid = false;
        found
    }

    /// True if the player's body currently overlaps cell (x,y,z).
    pub fn body_in(&self, x: i32, y: i32, z: i32) -> bool {
        let (px, py, pz) = (self.c.x(), self.c.y(), self.c.z());
        px + 0.3 > x as f64 && px - 0.3 < x as f64 + 1.0 && pz + 0.3 > z as f64 && pz - 0.3 < z as f64 + 1.0 && py + 1.8 > y as f64 && py < y as f64 + 1.0
    }

    /// Breaks the block at (x,y,z) by aiming at the given point on it and holding attack.
    pub fn break_block(&mut self, x: i32, y: i32, z: i32, ax: f64, ay: f64, az: f64, max_ticks: i32) -> bool {
        let before = self.g.get(x, y, z);
        if before == 0 { return true; }
        self.c.click_attack();
        self.c.hold_attack(true);
        for tk in 0..max_ticks {
            self.look_at(ax, ay, az);
            self.tick();
            if self.g.get(x, y, z) == 0 { self.c.hold_attack(false); self.broken += 1; return true; }
            let hit = self.c.hit;
            if tk > 2 && match hit { None => true, Some(h) => h.pos.x != x || h.pos.y != y || h.pos.z != z } { break; }
        }
        self.c.hold_attack(false);
        self.tick();
        self.g.get(x, y, z) == 0
    }

    /// A point on a visible face of block (x,y,z) from the eye, or false. Result in `self.click`.
    pub fn find_break(&mut self, x: i32, y: i32, z: i32, ex: f64, ey: f64, ez: f64) -> bool {
        let tb = t();
        let s = self.g.get(x, y, z) as usize;
        if s == 0 { return false; }
        if !tb.out_full[s] {
            let b = &tb.out_boxes_f32[s];
            for pass in 0..7 {
                let mut k = 0;
                while k < b.len() {
                    // (float arithmetic as in the original)
                    let mut ax = (x as f32 + (b[k] + b[k + 3]) / 2.0f32) as f64;
                    let mut ay = (y as f32 + (b[k + 1] + b[k + 4]) / 2.0f32) as f64;
                    let mut az = (z as f32 + (b[k + 2] + b[k + 5]) / 2.0f32) as f64;
                    match pass {
                        1 => ay = (y as f32 + b[k + 4]) as f64 - 0.01,
                        2 => ay = (y as f32 + b[k + 1]) as f64 + 0.01,
                        3 => ax = (x as f32 + b[k]) as f64 + 0.01,
                        4 => ax = (x as f32 + b[k + 3]) as f64 - 0.01,
                        5 => az = (z as f32 + b[k + 2]) as f64 + 0.01,
                        6 => az = (z as f32 + b[k + 5]) as f64 - 0.01,
                        _ => {}
                    }
                    k += 6;
                    let (dx, dy, dz) = (ax - ex, ay - ey, az - ez);
                    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                    if dist > self.reach - 0.05 || dist < 1e-6 { continue; }
                    if self.ray.cast(&self.g, ex, ey, ez, ax, ay, az, self.reach) && self.ray.hx == x && self.ray.hy == y && self.ray.hz == z {
                        self.click.nx = x; self.click.ny = y; self.click.nz = z; self.click.face = self.ray.face; self.click.ax = ax; self.click.ay = ay; self.click.az = az;
                        return true;
                    }
                }
            }
            return false;
        }
        for face in 0..6 {
            for o in 0..5 {
                let (u, v) = (0.5 + FREE[o][0], 0.5 + FREE[o][1]);
                let (ax, ay, az): (f64, f64, f64);
                match face {
                    0 => { ax = x as f64 + u; ay = y as f64; az = z as f64 + v; if ey >= ay { continue; } }
                    1 => { ax = x as f64 + u; ay = y as f64 + 1.0; az = z as f64 + v; if ey <= ay { continue; } }
                    2 => { ax = x as f64 + u; ay = y as f64 + v; az = z as f64; if ez >= az { continue; } }
                    3 => { ax = x as f64 + u; ay = y as f64 + v; az = z as f64 + 1.0; if ez <= az { continue; } }
                    4 => { ax = x as f64; ay = y as f64 + v; az = z as f64 + u; if ex >= ax { continue; } }
                    _ => { ax = x as f64 + 1.0; ay = y as f64 + v; az = z as f64 + u; if ex <= ax { continue; } }
                }
                let (dx, dy, dz) = (ax - ex, ay - ey, az - ez);
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                if dist > self.reach - 0.05 { continue; }
                if self.ray.cast(&self.g, ex, ey, ez, ax + dx * 0.01, ay + dy * 0.01, az + dz * 0.01, self.reach) && self.ray.hx == x && self.ray.hy == y && self.ray.hz == z {
                    self.click.nx = x; self.click.ny = y; self.click.nz = z; self.click.face = self.ray.face; self.click.ax = ax; self.click.ay = ay; self.click.az = az;
                    return true;
                }
            }
        }
        false
    }
}

/// Java's Math.min for doubles (NaN if either is NaN)
#[inline]
fn jmin(a: f64, b: f64) -> f64 { if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) } }
