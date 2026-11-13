//! The bot's own picture of the build site (the Java version's `Grid`).
use crate::tables::*;

pub struct Grid {
    pub x0: i32, pub y0: i32, pub z0: i32, pub nx: i32, pub ny: i32, pub nz: i32,
    pub ox: i32, pub oy: i32, pub oz: i32, pub sx: i32, pub sy: i32, pub sz: i32,
    pub cur: Vec<u16>,
    pub tgt: Vec<u16>,
    pub ground_state: u16,
    /// cache of `stand16` per cell (UNKNOWN = not computed); every write to `cur` goes through `put`
    stand: Vec<std::cell::Cell<i8>>,
    /// Per-cell facts for the route search, on a padded copy of the grid (one cell on each side, two rows below,
    /// three above) so that the search reads neighbours without bounds tests. See the `N_*` bits.
    pub navc: Vec<u32>,
    pub pnx: usize,
    pub pnz: usize,
    pub player: usize,
    /// content hash of `cur` relative to the initial picture
    pub version: u64,
}

const UNKNOWN: i8 = -2;

pub const N_PASS: u32 = 1 << 8;
pub const N_AIR: u32 = 1 << 9;
pub const N_TOWER: u32 = 1 << 10;
pub const N_OUT_FULL: u32 = 1 << 11;
pub const N_FLAT_TOP: u32 = 1 << 12;
pub const N_STURDY_TOP: u32 = 1 << 13;
pub const N_COL16: u32 = 1 << 14;
pub const N_COLNZ: u32 = 1 << 15;
pub const N_FLOOR16: u32 = 1 << 16;
pub const N_RUNG_SHIFT: u32 = 17;
pub const N_RFACE_SHIFT: u32 = 19;
pub const N_DOOR_SHIFT: u32 = 22;
pub const N_DOOR_MASK: u32 = 15 << 22;
pub const N_CLOSED_GATE: u32 = 1 << 26;
pub const N_HAND_DOOR: u32 = 1 << 27;
/// the cell holds one of the bot's temporary blocks (mirrors the scaffold set)
pub const N_SCAF: u32 = 1 << 28;
/// nothing of the target here, not in front of a piston, not among the chests: temporary blocks may always go here
pub const N_OPEN: u32 = 1 << 29;
const N_KEEP: u32 = N_SCAF | N_OPEN;

/// Per-state bits of `navc`.
fn nav_bits() -> &'static Vec<u32> {
    static V: std::sync::OnceLock<Vec<u32>> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let tb = t();
        (0..tb.n).map(|s| {
            let mut b = 0u32;
            if tb.walk_through[s] { b |= N_PASS; }
            if s == 0 { b |= N_AIR; }
            if tb.scaffolding[s] { b |= N_TOWER; }
            if tb.out_full[s] { b |= N_OUT_FULL; }
            if crate::bot::nav::flat_top(s as u16) { b |= N_FLAT_TOP; }
            if tb.sturdy_top[s] { b |= N_STURDY_TOP; }
            if tb.col_top[s] >= 16 { b |= N_COL16; }
            if tb.col_top[s] != 0 { b |= N_COLNZ; }
            if tb.col_top[s] == 16 && tb.any_top[s] == 16 { b |= N_FLOOR16; }
            b |= (tb.rung[s] as u32 & 3) << N_RUNG_SHIFT;
            b |= (tb.rung_facing[s] as u32 & 7) << N_RFACE_SHIFT;
            b |= (tb.door_edges[s] as u32 & 15) << N_DOOR_SHIFT;
            if tb.closed_gate[s] { b |= N_CLOSED_GATE; }
            if tb.hand_door[s] { b |= N_HAND_DOOR; }
            b
        }).collect()
    })
}

#[inline]
fn cell_hash(i: usize, v: u16) -> u64 {
    let mut h = (i as u64) << 16 | v as u64;
    h = (h ^ (h >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94d049bb133111eb);
    h ^ (h >> 31)
}

/// Per-state facts the standing test needs, packed: bit 0 walkThrough, 1-5 colTop, 6-10 anyTop, 11-15 anyBottom, 16 colBottom == 0.
fn stand_info() -> &'static Vec<u32> {
    static V: std::sync::OnceLock<Vec<u32>> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let tb = t();
        (0..tb.n).map(|s| {
            (tb.walk_through[s] as u32) | ((tb.col_top[s] as u32 & 31) << 1) | ((tb.any_top[s] as u32 & 31) << 6) | ((tb.any_bottom[s] as u32 & 31) << 11) | (((tb.col_bottom[s] == 0) as u32) << 16)
        }).collect()
    })
}

impl Grid {
    #[allow(clippy::too_many_arguments)]
    pub fn new(ox: i32, oy: i32, oz: i32, sx: i32, sy: i32, sz: i32, target: &[u16], margin_x: i32, margin_north: i32, margin_south: i32, margin_top: i32, ground_state: u16) -> Grid {
        let (x0, z0, y0) = (ox - margin_x, oz - margin_north, oy - 1);
        let (nx, nz, ny) = (sx + 2 * margin_x, sz + margin_north + margin_south, sy + 1 + margin_top);
        let vol = nx as usize * ny as usize * nz as usize;
        let mut g = Grid { x0, y0, z0, nx, ny, nz, ox, oy, oz, sx, sy, sz, cur: vec![0; vol], tgt: vec![0; vol], ground_state, stand: Vec::new(), version: 0, navc: Vec::new(), pnx: (nx + 2) as usize, pnz: (nz + 2) as usize, player: ((nx + 2) * (nz + 2)) as usize };
        let pvol = g.player * (ny as usize + 5);
        g.stand = vec![std::cell::Cell::new(UNKNOWN); pvol];
        let nb = nav_bits();
        g.navc = vec![nb[0]; pvol];
        for i in 0..2 * g.player { g.navc[i] = nb[ground_state as usize]; }
        for z in 0..nz { for x in 0..nx { let pi = g.pidx(x0 + x, y0, z0 + z); g.navc[pi] = nb[ground_state as usize]; } }
        for i in 0..(nx * nz) as usize { g.cur[i] = ground_state; }   // (the cache starts empty)
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
        if self.inside(x, y, z) { let i = self.idx(x, y, z); self.put(i, if t().air[state as usize] { 0 } else { state }); }
    }
    pub fn in_box(&self, x: i32, y: i32, z: i32) -> bool { x >= self.ox && x < self.ox + self.sx && y >= self.oy && y < self.oy + self.sy && z >= self.oz && z < self.oz + self.sz }

    /// Writes a cell of the picture (all writes go through here: the standing cache depends on it).
    #[inline]
    pub fn put(&mut self, i: usize, v: u16) {
        if self.cur[i] == v { return; }
        // content hash (the same picture gives the same value, however it was reached)
        self.version ^= cell_hash(i, self.cur[i]) ^ cell_hash(i, v);
        self.cur[i] = v;
        let pi = self.pidx(self.x_of(i), self.y_of(i), self.z_of(i));
        self.navc[pi] = (self.navc[pi] & N_KEEP) | nav_bits()[v as usize];
        // stand16 of a cell reads the cells one below to two above it
        let l = self.player;
        self.stand[pi].set(UNKNOWN);
        self.stand[pi - l].set(UNKNOWN);
        self.stand[pi - 2 * l].set(UNKNOWN);
        self.stand[pi + l].set(UNKNOWN);
    }

    /// Index into the padded arrays (`navc`); valid for cells up to one outside the grid sideways, two below, three above.
    #[inline]
    pub fn pidx(&self, x: i32, y: i32, z: i32) -> usize { (((y - self.y0 + 2) as usize * self.pnz) + (z - self.z0 + 1) as usize) * self.pnx + (x - self.x0 + 1) as usize }
    pub fn set_nav_flag(&mut self, i: usize, flag: u32, on: bool) {
        let pi = self.pidx(self.x_of(i), self.y_of(i), self.z_of(i));
        if on { self.navc[pi] |= flag; } else { self.navc[pi] &= !flag; }
    }
    /// `stand16` by padded index (the coordinates are only needed when the value is not cached yet).
    #[inline]
    pub fn p_stand(&self, pi: usize, x: i32, y: i32, z: i32) -> i32 {
        let c = self.stand[pi].get();
        if c != UNKNOWN { return c as i32; }
        let v = self.stand16_compute(x, y, z);
        self.stand[pi].set(v as i8);
        v
    }
    /// `rung` by padded index
    #[inline]
    pub fn p_rung(&self, pi: usize) -> i32 {
        let c = self.navc[pi];
        let k = (c >> N_RUNG_SHIFT & 3) as i32;
        if k <= 1 { return k; }
        let b = self.navc[pi - self.player];
        if (b >> N_RUNG_SHIFT & 3) == 1 && (b >> N_RFACE_SHIFT & 7) == (c >> N_RFACE_SHIFT & 7) { k } else { 0 }
    }
    /// `hang` by padded index
    #[inline]
    pub fn p_hang(&self, pi: usize) -> bool {
        if self.navc[pi] >> N_RUNG_SHIFT & 3 == 0 { return false; }
        if self.p_rung(pi) == 0 { return false; }
        let s1 = self.navc[pi + self.player];
        s1 & N_PASS != 0 || (s1 >> N_RUNG_SHIFT & 3) != 0
    }

    /// Height of the feet above the bottom of the cell in 1/16 when standing centred there, or -1.
    #[inline]
    pub fn stand16(&self, x: i32, y: i32, z: i32) -> i32 {
        if !self.inside(x, y, z) { return self.stand16_compute(x, y, z); }
        self.p_stand(self.pidx(x, y, z), x, y, z)
    }

    fn stand16_compute(&self, x: i32, y: i32, z: i32) -> i32 {
        let si = stand_info();
        let (a, b) = (si[self.get(x, y, z) as usize], si[self.get(x, y - 1, z) as usize]);
        let h: i32;
        let ta = (a >> 1 & 31) as i32;
        if a & 1 != 0 {
            let tbb = (b >> 1 & 31) as i32;
            if tbb < 16 || (b >> 6 & 31) as i32 != tbb { return -1; }
            h = tbb - 16;
        } else if ta > 0 && ta < 16 && a & (1 << 16) != 0 && (a >> 6 & 31) as i32 == ta {
            h = ta;
        } else { return -1; }
        let top = h + 29;
        let s1 = si[self.get(x, y + 1, z) as usize];
        if s1 & 1 == 0 && 16 + ((s1 >> 11 & 31) as i32) < top { return -1; }
        if top > 32 {
            let s2 = si[self.get(x, y + 2, z) as usize];
            if s2 & 1 == 0 && 32 + ((s2 >> 11 & 31) as i32) < top { return -1; }
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
