//! Line of sight on the bot's picture of the world (the Java version's `Ray`).
use super::grid::Grid;
use crate::tables::*;

#[derive(Default, Clone)]
pub struct Ray {
    pub scaffold_solid: bool,
    pub hx: i32, pub hy: i32, pub hz: i32, pub face: i32,
    pub px: f64, pub py: f64, pub pz: f64, pub dist: f64,
}

impl Ray {
    #[allow(clippy::too_many_arguments)]
    pub fn cast(&mut self, g: &Grid, ex: f64, ey: f64, ez: f64, tx: f64, ty: f64, tz: f64, max_dist: f64) -> bool {
        let tb = t();
        let (mut dx, mut dy, mut dz) = (tx - ex, ty - ey, tz - ez);
        let len = (dx * dx + dy * dy + dz * dz).sqrt();
        if len < 1e-9 { return false; }
        dx /= len; dy /= len; dz /= len;
        let (mut x, mut y, mut z) = (ex.floor() as i32, ey.floor() as i32, ez.floor() as i32);
        let (step_x, step_y, step_z) = (if dx > 0.0 { 1 } else { -1 }, if dy > 0.0 { 1 } else { -1 }, if dz > 0.0 { 1 } else { -1 });
        let tdx = if dx == 0.0 { f64::MAX } else { (1.0 / dx).abs() };
        let tdy = if dy == 0.0 { f64::MAX } else { (1.0 / dy).abs() };
        let tdz = if dz == 0.0 { f64::MAX } else { (1.0 / dz).abs() };
        let mut tmx = if dx == 0.0 { f64::MAX } else { (if dx > 0.0 { x as f64 + 1.0 - ex } else { ex - x as f64 }) * tdx };
        let mut tmy = if dy == 0.0 { f64::MAX } else { (if dy > 0.0 { y as f64 + 1.0 - ey } else { ey - y as f64 }) * tdy };
        let mut tmz = if dz == 0.0 { f64::MAX } else { (if dz > 0.0 { z as f64 + 1.0 - ez } else { ez - z as f64 }) * tdz };
        let mut tt = 0.0f64;
        let mut enter_face = -1i32;
        while tt <= max_dist {
            let s = g.get(x, y, z) as usize;
            if s != 0 {
                let b = &tb.out_boxes_f32[s];
                if !b.is_empty() {
                    if (tb.out_full[s] || (self.scaffold_solid && tb.scaffolding[s])) && enter_face >= 0 {
                        self.hx = x; self.hy = y; self.hz = z; self.face = enter_face; self.dist = tt;
                        self.px = ex + dx * tt; self.py = ey + dy * tt; self.pz = ez + dz * tt;
                        return true;
                    }
                    let mut best = f64::MAX;
                    let mut best_face = -1i32;
                    let mut k = 0;
                    'boxes: while k < b.len() {
                        // (int + float is float arithmetic in the original)
                        let (x1, y1, z1, x2, y2, z2) = ((x as f32 + b[k]) as f64, (y as f32 + b[k + 1]) as f64, (z as f32 + b[k + 2]) as f64, (x as f32 + b[k + 3]) as f64, (y as f32 + b[k + 4]) as f64, (z as f32 + b[k + 5]) as f64);
                        k += 6;
                        let mut tn = -f64::MAX;
                        let mut tf = f64::MAX;
                        let mut f = -1i32;
                        if dx != 0.0 {
                            let (a, c) = ((x1 - ex) / dx, (x2 - ex) / dx);
                            let (lo, hi) = (a.min(c), a.max(c));
                            if lo > tn { tn = lo; f = if dx > 0.0 { 4 } else { 5 }; }
                            if hi < tf { tf = hi; }
                        } else if ex <= x1 || ex >= x2 { continue 'boxes; }
                        if dy != 0.0 {
                            let (a, c) = ((y1 - ey) / dy, (y2 - ey) / dy);
                            let (lo, hi) = (a.min(c), a.max(c));
                            if lo > tn { tn = lo; f = if dy > 0.0 { 0 } else { 1 }; }
                            if hi < tf { tf = hi; }
                        } else if ey <= y1 || ey >= y2 { continue 'boxes; }
                        if dz != 0.0 {
                            let (a, c) = ((z1 - ez) / dz, (z2 - ez) / dz);
                            let (lo, hi) = (a.min(c), a.max(c));
                            if lo > tn { tn = lo; f = if dz > 0.0 { 2 } else { 3 }; }
                            if hi < tf { tf = hi; }
                        } else if ez <= z1 || ez >= z2 { continue 'boxes; }
                        if tn <= tf && tn >= 0.0 && tn < best { best = tn; best_face = f; }
                    }
                    if best_face >= 0 && best <= max_dist {
                        self.hx = x; self.hy = y; self.hz = z; self.face = best_face; self.dist = best;
                        self.px = ex + dx * best; self.py = ey + dy * best; self.pz = ez + dz * best;
                        return true;
                    }
                }
            }
            if tmx < tmy && tmx < tmz { x += step_x; tt = tmx; tmx += tdx; enter_face = if dx > 0.0 { 4 } else { 5 }; }
            else if tmy < tmz { y += step_y; tt = tmy; tmy += tdy; enter_face = if dy > 0.0 { 0 } else { 1 }; }
            else { z += step_z; tt = tmz; tmz += tdz; enter_face = if dz > 0.0 { 2 } else { 3 }; }
        }
        false
    }
}
