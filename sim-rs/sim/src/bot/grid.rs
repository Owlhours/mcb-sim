//! The bot's own picture of the build site (the Java version's `Grid`).
use crate::tables::*;

pub struct Grid {
    pub x0: i32, pub y0: i32, pub z0: i32, pub nx: i32, pub ny: i32, pub nz: i32,
    pub ox: i32, pub oy: i32, pub oz: i32, pub sx: i32, pub sy: i32, pub sz: i32,
    pub cur: Vec<u16>,
    pub tgt: Vec<u16>,
    pub ground_state: u16,
}

impl Grid {
    #[allow(clippy::too_many_arguments)]
    pub fn new(ox: i32, oy: i32, oz: i32, sx: i32, sy: i32, sz: i32, target: &[u16], margin_x: i32, margin_north: i32, margin_south: i32, margin_top: i32, ground_state: u16) -> Grid {
        let (x0, z0, y0) = (ox - margin_x, oz - margin_north, oy - 1);
        let (nx, nz, ny) = (sx + 2 * margin_x, sz + margin_north + margin_south, sy + 1 + margin_top);
        let vol = nx as usize * ny as usize * nz as usize;
        let mut g = Grid { x0, y0, z0, nx, ny, nz, ox, oy, oz, sx, sy, sz, cur: vec![0; vol], tgt: vec![0; vol], ground_state };
        for i in 0..(nx * nz) as usize { g.cur[i] = ground_state; }
        for y in 0..sy { for z in 0..sz {
            let src = ((y * sz + z) * sx) as usize;
            let dst = g.idx(ox, oy + y, oz + z);
            g.tgt[dst..dst + sx as usize].copy_from_slice(&target[src..src + sx as usize]);
        } }
        g
    }
    #[inline]
    pub fn idx(&self, x: i32, y: i32, z: i32) -> usize { (((y - self.y0) * self.nz + (z - self.z0)) * self.nx + (x - self.x0)) as usize }
    /// The index arithmetic without a bounds check, as the original computes it (used as a set key only).
    #[inline]
    pub fn idx_raw(&self, x: i32, y: i32, z: i32) -> i32 { ((y - self.y0).wrapping_mul(self.nz).wrapping_add(z - self.z0)).wrapping_mul(self.nx).wrapping_add(x - self.x0) }
    #[inline]
    pub fn inside(&self, x: i32, y: i32, z: i32) -> bool { x >= self.x0 && x < self.x0 + self.nx && y >= self.y0 && y < self.y0 + self.ny && z >= self.z0 && z < self.z0 + self.nz }
    #[inline]
    pub fn x_of(&self, i: usize) -> i32 { self.x0 + (i as i32) % self.nx }
    #[inline]
    pub fn z_of(&self, i: usize) -> i32 { self.z0 + ((i as i32) / self.nx) % self.nz }
    #[inline]
    pub fn y_of(&self, i: usize) -> i32 { self.y0 + (i as i32) / (self.nx * self.nz) }
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> u16 {
        if y < self.y0 { return self.ground_state; }
        if !self.inside(x, y, z) { return 0; }
        self.cur[self.idx(x, y, z)]
    }
    pub fn target(&self, x: i32, y: i32, z: i32) -> u16 { if self.inside(x, y, z) { self.tgt[self.idx(x, y, z)] } else { 0 } }
    pub fn set(&mut self, x: i32, y: i32, z: i32, state: u16) {
        if self.inside(x, y, z) { let i = self.idx(x, y, z); self.cur[i] = if t().air[state as usize] { 0 } else { state }; }
    }
    pub fn in_box(&self, x: i32, y: i32, z: i32) -> bool { x >= self.ox && x < self.ox + self.sx && y >= self.oy && y < self.oy + self.sy && z >= self.oz && z < self.oz + self.sz }

    /// Height of the feet above the bottom of the cell in 1/16 when standing centred there, or -1.
    pub fn stand16(&self, x: i32, y: i32, z: i32) -> i32 {
        let tb = t();
        let (a, b) = (self.get(x, y, z) as usize, self.get(x, y - 1, z) as usize);
        let h: i32;
        let ta = tb.col_top[a] as i32;
        if tb.walk_through[a] {
            let tbb = tb.col_top[b] as i32;
            if tbb < 16 || tb.any_top[b] as i32 != tbb { return -1; }
            h = tbb - 16;
        } else if ta > 0 && ta < 16 && tb.col_bottom[a] == 0 && tb.any_top[a] as i32 == ta {
            h = ta;
        } else { return -1; }
        let top = h + 29;
        let s1 = self.get(x, y + 1, z) as usize;
        if !tb.walk_through[s1] && 16 + (tb.any_bottom[s1] as i32) < top { return -1; }
        if top > 32 {
            let s2 = self.get(x, y + 2, z) as usize;
            if !tb.walk_through[s2] && 32 + (tb.any_bottom[s2] as i32) < top { return -1; }
        }
        h
    }
    #[inline]
    pub fn passable(&self, x: i32, y: i32, z: i32) -> bool { t().walk_through[self.get(x, y, z) as usize] }
    pub fn rung(&self, x: i32, y: i32, z: i32) -> i32 {
        let tb = t();
        let s = self.get(x, y, z) as usize;
        let k = tb.rung[s] as i32;
        if k <= 1 { return k; }
        let b = self.get(x, y - 1, z) as usize;
        if tb.rung[b] == 1 && tb.rung_facing[b] == tb.rung_facing[s] { k } else { 0 }
    }
    pub fn hang(&self, x: i32, y: i32, z: i32) -> bool {
        if self.rung(x, y, z) == 0 { return false; }
        let s1 = self.get(x, y + 1, z) as usize;
        t().walk_through[s1] || t().rung[s1] != 0
    }
    #[inline]
    pub fn is_tower(&self, x: i32, y: i32, z: i32) -> bool { t().scaffolding[self.get(x, y, z) as usize] }
    #[inline]
    pub fn solid_cube(&self, x: i32, y: i32, z: i32) -> bool { t().full_cube[self.get(x, y, z) as usize] }
    #[inline]
    pub fn is_air(&self, x: i32, y: i32, z: i32) -> bool { self.get(x, y, z) == 0 }
}
