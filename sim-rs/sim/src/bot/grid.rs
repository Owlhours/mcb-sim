//! The bot's own picture of the build site (the Java version's `Grid`).
use crate::tables::*;

pub struct Grid {
    pub x0: i32, pub y0: i32, pub z0: i32, pub nx: i32, pub ny: i32, pub nz: i32,
    pub ox: i32, pub oy: i32, pub oz: i32, pub sx: i32, pub sy: i32, pub sz: i32,
    pub cur: Vec<u16>,
    pub tgt: Vec<u16>,
    pub ground_state: u16,
    /// One record per cell of the padded grid: the route search's facts about the cell (`nav`: the `N_*` bits, with the
    /// `stand16` cache in the low byte) and its search node, side by side so that one cache line serves both.
    pub pc: Vec<PCell>,
    /// The grid is padded (one cell on each side, two rows below, three above) and stored column by column, so that the
    /// search reads neighbours without bounds tests. The stand cache (UNKNOWN = not computed) relies on every write to
    /// `cur` going through `put`.
    /// strides of the padded arrays: columns are contiguous (y is the fastest index), so the cells above and
    /// below one another share cache lines
    /// bit widths for packing a cell's grid-relative (y, z, x) into one integer that sorts like `idx` (see `pk`)
    pub bx: u32,
    pub bz: u32,
    pub pk_ok: bool,
    pub pys: usize,
    pub pzs: usize,
    pub pxs: usize,
    /// content hash of `cur` relative to the initial picture
    pub version: u64,
}

extern "C" { fn madvise(addr: *mut u8, len: usize, advice: i32) -> i32; }
/// A vector of `n` copies of `v`; large ones ask for transparent huge pages (the searches walk these arrays at random,
/// and on big grids address translation misses cost about as much as the cache misses).
pub fn huge_vec<T: Clone>(n: usize, v: T) -> Vec<T> {
    let mut out: Vec<T> = Vec::with_capacity(n);
    let bytes = n * std::mem::size_of::<T>();
    const HUGE: usize = 2 << 20;
    if bytes >= 2 * HUGE && std::env::var_os("MCSIM_NOHUGE").is_none() {
        let a = out.as_mut_ptr() as usize;
        let lo = (a + HUGE - 1) & !(HUGE - 1);
        let hi = (a + bytes) & !(HUGE - 1);
        if hi > lo { unsafe { madvise(lo as *mut u8, hi - lo, 14); } }
    }
    out.resize(n, v);
    out
}

const UNKNOWN: i8 = -2;
const UNKNOWN_BITS: u32 = 0xFE;

#[repr(C)]
#[derive(Clone)]
pub struct PCell {
    pub nav: std::cell::Cell<u32>,
    pub cost: i32,
    pub parent: i32,
    pub stamp: u16,
    pub how: u8,
    pub pad: u8,
}

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
/// the cell holds a full cube (nothing can ever stand in or pass through it without breaking it)
pub const N_FULL: u32 = 1 << 30;
const N_KEEP: u32 = N_SCAF | N_OPEN;

/// Per-state bits of `PCell::nav` (bits 8 and up; the low byte is the stand cache).
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
            if tb.full_cube[s] { b |= N_FULL; }
            b
        }).collect()
    })
}

/// The per-state `N_*` bits of a block state.
#[inline]
pub fn state_bits(s: u16) -> u32 { nav_bits()[s as usize] }

#[inline]
fn cell_hash(i: usize, v: u16) -> u64 {
    let mut h = (i as u64) << 16 | v as u64;
    h = (h ^ (h >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94d049bb133111eb);
    h ^ (h >> 31)
}

/// Per-state facts the standing test needs, packed: bit 0 walkThrough, 1-5 colTop, 6-10 anyTop, 11-15 anyBottom + 8 (it can be negative), 16 colBottom == 0,
/// 17 hazard (only where the revision has `X_HAZ`).
fn stand_info() -> &'static Vec<u32> {
    static V: std::sync::OnceLock<Vec<u32>> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let tb = t();
        let haz = crate::rev::rev().x_haz;
        (0..tb.n).map(|s| {
            (tb.walk_through[s] as u32) | ((tb.col_top[s] as u32 & 31) << 1) | ((tb.any_top[s] as u32 & 31) << 6) | (((tb.any_bottom[s] as i32 + 8) as u32 & 31) << 11) | (((tb.col_bottom[s] == 0) as u32) << 16)
                | (((haz && hazard(s as u16)) as u32) << 17)
        }).collect()
    })
}

impl Grid {
    #[allow(clippy::too_many_arguments)]
    pub fn new(ox: i32, oy: i32, oz: i32, sx: i32, sy: i32, sz: i32, target: &[u16], margin_x: i32, margin_north: i32, margin_south: i32, margin_top: i32, ground_state: u16) -> Grid {
        let (x0, z0, y0) = (ox - margin_x, oz - margin_north, oy - 1);
        let (nx, nz, ny) = (sx + 2 * margin_x, sz + margin_north + margin_south, sy + 1 + margin_top);
        let vol = nx as usize * ny as usize * nz as usize;
        let mut g = Grid { x0, y0, z0, nx, ny, nz, ox, oy, oz, sx, sy, sz, cur: huge_vec(vol, 0), tgt: huge_vec(vol, 0), ground_state, pc: Vec::new(), version: 0, bx: 0, bz: 0, pk_ok: false, pys: 1, pzs: ny as usize + 5, pxs: (ny as usize + 5) * (nz as usize + 2) };
        let pvol = g.pxs * (nx as usize + 2);
        let bits = |n: i32| 32 - ((n - 1).max(1) as u32).leading_zeros();
        g.bx = bits(nx); g.bz = bits(nz);
        g.pk_ok = g.bx + g.bz + bits(ny) <= 31;
        let nb = nav_bits();
        let mk = |bits: u32| PCell { nav: std::cell::Cell::new(bits | UNKNOWN_BITS), cost: 0, parent: -1, stamp: 0, how: 0, pad: 0 };
        g.pc = huge_vec(pvol, mk(nb[0]));
        // below the grid everything is ground (two padding rows), and so is the grid's own bottom layer
        let mut c = 0;
        while c < pvol { g.pc[c] = mk(nb[ground_state as usize]); g.pc[c + 1] = mk(nb[ground_state as usize]); c += g.pzs; }
        for z in 0..nz { for x in 0..nx { let pi = g.pidx(x0 + x, y0, z0 + z); g.pc[pi] = mk(nb[ground_state as usize]); } }
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
        if self.inside(x, y, z) { self.put_at(x, y, z, if t().air[state as usize] { 0 } else { state }); }
    }
    pub fn in_box(&self, x: i32, y: i32, z: i32) -> bool { x >= self.ox && x < self.ox + self.sx && y >= self.oy && y < self.oy + self.sy && z >= self.oz && z < self.oz + self.sz }

    /// Writes a cell of the picture (all writes go through here: the standing cache depends on it).
    #[inline]
    pub fn put(&mut self, i: usize, v: u16) {
        if self.cur[i] == v { return; }
        let pi = self.pidx(self.x_of(i), self.y_of(i), self.z_of(i));
        self.put_p(i, pi, v);
    }
    /// `put` for a caller that has the coordinates at hand (must be inside the grid).
    #[inline]
    pub fn put_at(&mut self, x: i32, y: i32, z: i32, v: u16) {
        let i = self.idx(x, y, z);
        if self.cur[i] == v { return; }
        let pi = self.pidx(x, y, z);
        self.put_p(i, pi, v);
    }
    #[inline]
    fn put_p(&mut self, i: usize, pi: usize, v: u16) {
        // content hash (the same picture gives the same value, however it was reached)
        self.version ^= cell_hash(i, self.cur[i]) ^ cell_hash(i, v);
        self.cur[i] = v;
        let c = &self.pc[pi].nav;
        c.set((c.get() & N_KEEP) | nav_bits()[v as usize] | UNKNOWN_BITS);
        // stand16 of a cell reads the cells one below to two above it
        for q in [pi - 1, pi - 2, pi + 1] { let c = &self.pc[q].nav; c.set((c.get() & !0xff) | UNKNOWN_BITS); }
    }

    /// A cell as one integer, (y, z, x) packed by bits: sorts exactly like `idx` and unpacks without divisions.
    /// The fast route search names its nodes this way.
    #[inline]
    pub fn pk(&self, x: i32, y: i32, z: i32) -> i32 { (((y - self.y0) << self.bz | (z - self.z0)) << self.bx) | (x - self.x0) }
    #[inline]
    pub fn unpk(&self, k: i32) -> (i32, i32, i32) {
        (self.x0 + (k & ((1 << self.bx) - 1)), self.y0 + (k >> (self.bx + self.bz)), self.z0 + ((k >> self.bx) & ((1 << self.bz) - 1)))
    }

    /// The `N_*` bits of a cell by padded index.
    #[inline]
    pub fn nv(&self, pi: usize) -> u32 { self.pc[pi].nav.get() }

    /// Index into the padded array (`pc`); valid for cells up to one outside the grid sideways, two below, three above.
    #[inline]
    pub fn pidx(&self, x: i32, y: i32, z: i32) -> usize { (x - self.x0 + 1) as usize * self.pxs + (z - self.z0 + 1) as usize * self.pzs + (y - self.y0 + 2) as usize }
    pub fn set_nav_flag(&mut self, i: usize, flag: u32, on: bool) {
        let pi = self.pidx(self.x_of(i), self.y_of(i), self.z_of(i));
        let c = &self.pc[pi].nav;
        if on { c.set(c.get() | flag); } else { c.set(c.get() & !flag); }
    }
    /// `stand16` by padded index (the coordinates are only needed when the value is not cached yet).
    #[inline]
    pub fn p_stand(&self, pi: usize, x: i32, y: i32, z: i32) -> i32 {
        let cell = &self.pc[pi].nav;
        let bits = cell.get();
        let c = bits as u8 as i8;
        if c != UNKNOWN {
            #[cfg(debug_assertions)]
            { let v = self.stand16_compute(x, y, z); if v != c as i32 { panic!("stale stand cache at {x},{y},{z}: {c} vs {v}"); } }
            return c as i32;
        }
        let v = self.stand16_compute(x, y, z);
        cell.set((bits & !0xff) | (v as i8 as u8 as u32));
        v
    }
    /// `rung` by padded index
    #[inline]
    pub fn p_rung(&self, pi: usize) -> i32 {
        let c = self.nv(pi);
        let k = (c >> N_RUNG_SHIFT & 3) as i32;
        if k <= 1 { return k; }
        let b = self.nv(pi - self.pys);
        if (b >> N_RUNG_SHIFT & 3) == 1 && (b >> N_RFACE_SHIFT & 7) == (c >> N_RFACE_SHIFT & 7) { k } else { 0 }
    }
    /// `hang` by padded index
    #[inline]
    pub fn p_hang(&self, pi: usize) -> bool {
        if self.nv(pi) >> N_RUNG_SHIFT & 3 == 0 { return false; }
        if self.p_rung(pi) == 0 { return false; }
        let s1 = self.nv(pi + self.pys);
        s1 & N_PASS != 0 || (s1 >> N_RUNG_SHIFT & 3) != 0
    }

    /// Height of the feet above the bottom of the cell in 1/16 when standing centred there, or -1.
    #[inline]
    pub fn stand16(&self, x: i32, y: i32, z: i32) -> i32 {
        if !self.inside(x, y, z) { return self.stand16_compute(x, y, z); }
        self.p_stand(self.pidx(x, y, z), x, y, z)
    }

    /// Java `Grid.stand16`.
    pub fn stand16_compute(&self, x: i32, y: i32, z: i32) -> i32 {
        const HAZ: u32 = 1 << 17;
        let si = stand_info();
        let (a, b) = (si[self.get(x, y, z) as usize], si[self.get(x, y - 1, z) as usize]);
        if (a | b) & HAZ != 0 { return -1; }   // (a lit campfire, cactus ...: it would hurt)
        let mut h: i32;
        let ta = (a >> 1 & 31) as i32;
        let tbb = (b >> 1 & 31) as i32;
        if a & 1 != 0 {
            if tbb < 16 || (b >> 6 & 31) as i32 != tbb { return -1; }   // nothing to stand on, or an uneven top (stairs ...)
            h = tbb - 16;
        } else if ta > 0 && ta < 16 && a & (1 << 16) != 0 && (a >> 6 & 31) as i32 == ta {
            h = ta;                                                     // a slab, carpet, snow layer ... in the feet cell
            // (a carpet on a fence: the fence reaches up through it, and that is what carries the feet)
            let over = tbb - 16;
            if over > h && crate::rev::rev().x_carpet { if (b >> 6 & 31) as i32 != tbb { return -1; } h = over; }
        } else { return -1; }
        // head room: the body occupies [h, h + 28.8] sixteenths above the cell bottom
        let top = h + 29;
        let s1 = si[self.get(x, y + 1, z) as usize];
        if s1 & HAZ != 0 { return -1; }
        if s1 & 1 == 0 && 16 + ((s1 >> 11 & 31) as i32 - 8) < top { return -1; }
        if top > 32 {
            let s2 = si[self.get(x, y + 2, z) as usize];
            if s2 & 1 == 0 && 32 + ((s2 >> 11 & 31) as i32 - 8) < top { return -1; }
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
