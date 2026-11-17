//! The world as bit planes, column-major.
//!
//! Grid coordinates: `gx`, `gz` horizontal with a margin around the build and one ring of wall columns outside it;
//! `y` 0 is the ground layer, the build's layer 0 is `y` 1. A column is `c = gz * nx + gx`; a cell id is `c << 9 | y`.
//! For column `c` and word `yw = y >> 6` the eight planes sit together: `pl[(c * w + yw) * 8 + plane]`.
use crate::prep::{Build, Pal};

/// blocks movement and can be stood on (ground, placed blocks with a collision box, loose scaffold blocks, chests, walls)
pub const SOLID: usize = 0;
/// scaffolding: can be passed through, stood on and in, climbed
pub const TOWER: usize = 1;
/// the target block of this cell stands
pub const PLACED: usize = 2;
/// target cell the main pass still has to do
pub const NEED: usize = 3;
/// the target has a block here
pub const TGT: usize = 4;
/// loose scaffold block (temporary)
pub const LOOSE: usize = 5;
/// target cell that was put off (or belongs to the late kinds)
pub const DEFER: usize = 6;
/// something stands here that a click can land on (ground, a placed block with an outline, scaffold of either kind)
pub const CLICK: usize = 7;

pub const YBITS: u32 = 9;
pub const YMASK: u32 = (1 << YBITS) - 1;
/// free cells between the build and the wall ring, west / east / south
pub const MARGIN: usize = 7;
/// A's site: chest rows start 5 north of the build, 3 apart
pub const ROW_GAP: usize = 5;
pub const ROW_PITCH: usize = 3;
pub const TOP_PAD: usize = 8;

pub struct World<'a> {
    pub nx: usize,
    pub nz: usize,
    pub ny: usize,
    pub w: usize,
    /// grid position of the build's (0, *, 0)
    pub ox: usize,
    pub oz: usize,
    pub sx: usize,
    pub sy: usize,
    pub sz: usize,
    pub pl: Vec<u64>,
    /// target cells the judge counts: one word per column word (not part of the hot planes)
    pub judged: Vec<u64>,
    pub cells: &'a [u16],
    pub pal: &'a [Pal],
    /// per grid column: where its cells start in `cells`, less one (the ground layer); `u32::MAX` outside the build
    pub colbase: Vec<u32>,
    /// 2^40 / nx, rounded up: column -> row by a multiplication
    pub inv_nx: u64,
    pub depot_cols: usize,
    pub skipped_unplaceable: u32,
}

#[inline(always)]
pub fn cell_id(c: usize, y: usize) -> u32 { ((c as u32) << YBITS) | y as u32 }
#[inline(always)]
pub fn id_c(id: u32) -> usize { (id >> YBITS) as usize }
#[inline(always)]
pub fn id_y(id: u32) -> usize { (id & YMASK) as usize }

impl<'a> World<'a> {
    #[inline(never)]
    pub fn new(b: &'a Build) -> World<'a> {
        let depot_cols = ((b.sx + 1) / 2).clamp(3, 24);
        let depot_rows = (b.chest_stacks as usize).div_ceil(depot_cols);
        let north = ROW_GAP + depot_rows * ROW_PITCH + 2 + 1;
        let east = (MARGIN + 1).max(depot_cols * 2 + 5 - b.sx.min(depot_cols * 2 + 5));
        let nx = MARGIN + 1 + b.sx + east;
        let nz = north + b.sz + MARGIN + 1;
        let ny = b.sy + 1 + TOP_PAD;
        let w = ny.div_ceil(64);
        let mut wd = World { nx, nz, ny, w, ox: MARGIN + 1, oz: north, sx: b.sx, sy: b.sy, sz: b.sz, pl: vec![0u64; nx * nz * w * 8], judged: vec![0u64; nx * nz * w],
            cells: &b.cells, pal: &b.pal, colbase: vec![u32::MAX; nx * nz], inv_nx: (1u64 << 40) / nx as u64 + 1, depot_cols, skipped_unplaceable: b.unplaceable };
        // ground everywhere, walls on the outer ring
        for gz in 0..nz { for gx in 0..nx {
            let c = gz * nx + gx;
            let wall = gx == 0 || gz == 0 || gx == nx - 1 || gz == nz - 1;
            for yw in 0..w { wd.pl[(c * w + yw) * 8 + SOLID] = if wall { !0 } else if yw == 0 { 1 } else { 0 }; }
            if !wall { wd.pl[c * w * 8 + CLICK] = 1; }
        } }
        // chests (three high), as A's site puts them
        for k in 0..b.chest_stacks as usize {
            let (row, col) = (k / depot_cols, k % depot_cols);
            let gz = wd.oz - ROW_GAP - row * ROW_PITCH;
            for dx in 0..2 { let c = gz * nx + wd.ox + col * 2 + dx; wd.pl[c * w * 8 + SOLID] |= 0b1110; wd.pl[c * w * 8 + CLICK] |= 0b1110; }
        }
        // the target: the planes come ready from the packed build
        debug_assert_eq!(w, b.w);
        for bz in 0..b.sz {
            for bx in 0..b.sx {
                let c = (wd.oz + bz) * nx + wd.ox + bx;
                let col = bz * b.sx + bx;
                wd.colbase[c] = (col * b.sy) as u32;
                for yw in 0..w {
                    let src = (col * w + yw) * 5;
                    let dst = (c * w + yw) * 8;
                    wd.pl[dst + TGT] = b.planes[src];
                    wd.pl[dst + NEED] = b.planes[src + 1];
                    wd.pl[dst + DEFER] = b.planes[src + 2];
                    wd.pl[dst + PLACED] = b.planes[src + 3];
                    wd.judged[c * w + yw] = b.planes[src + 4];
                }
            }
        }
        wd
    }

    #[inline(always)]
    pub fn word(&self, plane: usize, c: usize, yw: usize) -> u64 { unsafe { *self.pl.get_unchecked((c * self.w + yw) * 8 + plane) } }
    #[inline(always)]
    pub fn get(&self, plane: usize, c: usize, y: usize) -> bool { (self.word(plane, c, y >> 6) >> (y & 63)) & 1 != 0 }
    #[inline(always)]
    pub fn set(&mut self, plane: usize, c: usize, y: usize) { let i = (c * self.w + (y >> 6)) * 8 + plane; self.pl[i] |= 1u64 << (y & 63); }
    #[inline(always)]
    pub fn clear(&mut self, plane: usize, c: usize, y: usize) { let i = (c * self.w + (y >> 6)) * 8 + plane; self.pl[i] &= !(1u64 << (y & 63)); }

    /// Bits of a plane from `y0` upward: bit i is cell `y0 + i`. At least 16 bits are valid (cells above the world read 0).
    #[inline(always)]
    pub fn win(&self, plane: usize, c: usize, y0: usize) -> u64 {
        let (yw, s) = (y0 >> 6, y0 & 63);
        let mut v = self.word(plane, c, yw) >> s;
        if s > 48 && yw + 1 < self.w { v |= self.word(plane, c, yw + 1) << (64 - s); }
        v
    }

    /// Something stands in the cell that a click can land on.
    #[inline(always)]
    pub fn clickable(&self, c: usize, y: usize) -> bool { self.get(CLICK, c, y) }

    /// Mask of feet heights the bot can stand at in column `c`, bit i = height `y0 + i` (`y0 >= 1`), valid for i < 13.
    #[inline(always)]
    pub fn stand_mask(&self, c: usize, y0: usize) -> u64 {
        let s = self.win(SOLID, c, y0 - 1);
        let t = self.win(TOWER, c, y0 - 1);
        (s | t | (t >> 1)) & !(s >> 1) & !(s >> 2)
    }
    #[inline(always)]
    pub fn standable(&self, c: usize, y: usize) -> bool { self.stand_mask(c, y) & 1 != 0 }

    /// The highest cell below `y` in column `c` with a solid block or scaffolding in it.
    #[inline(always)]
    pub fn floor_below(&self, c: usize, y: usize) -> usize {
        let mut yw = y >> 6;
        let mut m = (self.word(SOLID, c, yw) | self.word(TOWER, c, yw)) & ((1u64 << (y & 63)) - 1);
        while m == 0 { yw -= 1; m = self.word(SOLID, c, yw) | self.word(TOWER, c, yw); }
        (yw << 6) + 63 - m.leading_zeros() as usize
    }

    /// The lowest cell at or above `from` in column `c` with the plane's bit set, if any.
    #[inline(always)]
    pub fn first_at_or_above(&self, plane: usize, c: usize, from: usize) -> Option<usize> {
        let mut yw = from >> 6;
        if yw >= self.w { return None; }
        let mut m = self.word(plane, c, yw) & (!0u64 << (from & 63));
        loop {
            if m != 0 { return Some((yw << 6) + m.trailing_zeros() as usize); }
            yw += 1;
            if yw >= self.w { return None; }
            m = self.word(plane, c, yw);
        }
    }

    /// Palette entry of a target cell inside the build box (`y >= 1`).
    #[inline(always)]
    pub fn pal_at(&self, c: usize, y: usize) -> Pal {
        unsafe {
            let p = *self.cells.get_unchecked(*self.colbase.get_unchecked(c) as usize + y - 1);
            *self.pal.get_unchecked(p as usize)
        }
    }

    /// Grid x and z of a column.
    #[inline(always)]
    pub fn xz(&self, c: usize) -> (i32, i32) {
        let z = (c as u64 * self.inv_nx) >> 40;
        ((c as u64 - z * self.nx as u64) as i32, z as i32)
    }

}

#[derive(Default, Debug, Clone)]
pub struct Judged {
    pub sections: u32,
    pub failed: u32,
    pub target_cells: u64,
    pub missing: u64,
    pub leftover: u64,
    pub left_loose: u64,
    pub exempt: u64,
    pub worst: f64,
    pub pass: bool,
}

/// A's judge (strict reading): per 16x16x16 section, bad = missing + left-behind; a section fails above 3 % of its
/// judged target cells. A cell holds either its target block or not, so "wrong" does not occur here: a temporary
/// block standing in a target cell counts once, as missing.
#[inline(never)]
pub fn judge(w: &World<'_>) -> Judged {
    let mut r = Judged::default();
    let (nsx, nsy, nsz) = (w.sx.div_ceil(16), w.sy.div_ceil(16), w.sz.div_ceil(16));
    for cy in 0..nsy { for cz in 0..nsz { for cx in 0..nsx {
        let (mut tgt, mut bad) = (0u32, 0u32);
        let (y0, y1) = (cy * 16 + 1, (cy * 16 + 16).min(w.sy) + 1);
        let mask = (1u64 << (y1 - y0)) - 1;
        for bz in cz * 16..(cz * 16 + 16).min(w.sz) { for bx in cx * 16..(cx * 16 + 16).min(w.sx) {
            let c = (w.oz + bz) * w.nx + w.ox + bx;
            let t = w.win(TGT, c, y0) & mask;
            let j = { let (yw, sh) = (y0 >> 6, y0 & 63); let mut v = w.judged[c * w.w + yw] >> sh; if sh > 48 && yw + 1 < w.w { v |= w.judged[c * w.w + yw + 1] << (64 - sh); } v } & mask;
            let p = w.win(PLACED, c, y0) & mask;
            let tmp = (w.win(LOOSE, c, y0) | w.win(TOWER, c, y0)) & mask;
            tgt += j.count_ones();
            let miss = (j & !p).count_ones();
            let left = (tmp & !t).count_ones();
            r.missing += miss as u64;
            r.leftover += left as u64;
            r.left_loose += (w.win(LOOSE, c, y0) & mask & !t).count_ones() as u64;
            r.exempt += (t & !j).count_ones() as u64;
            bad += miss + left;
        } }
        r.sections += 1;
        r.target_cells += tgt as u64;
        let ratio = if tgt == 0 { if bad == 0 { 0.0 } else { 1.0 } } else { bad as f64 / tgt as f64 };
        if ratio > r.worst { r.worst = ratio; }
        if ratio > 0.03 { r.failed += 1; }
    } } }
    r.pass = r.failed == 0;
    r
}

