//! The bot: action primitives on the world (with their tick costs) and the baseline policy, a coarse equivalent of
//! A's bot (`/a/sim-rs/sim/src/bot`): order of work, stance choice, scaffold columns, trips, put-off cells, clean-up.
use crate::nav::*;
use crate::prep::{Build, C_CLICK, C_GRAVITY, C_LATE, C_SOLID};
use crate::world::*;

pub const TICK_LIMIT: u64 = 48 * 3600 * 20;
const NONE: i16 = i16::MAX;
const IT_LOOSE: usize = 0;
const IT_TOWER: usize = 1;

pub const PLACED_R: u8 = 0;
pub const DEFER_R: u8 = 1;
pub const SKIP_R: u8 = 2;

const DC6X: [i32; 6] = [1, -1, 0, 0, 0, 0];
const DC6Y: [i32; 6] = [0, 0, 1, -1, 0, 0];
const DC6Z: [i32; 6] = [0, 0, 0, 0, 1, -1];
/// the face of the neighbour in direction d that looks at the cell, in A's numbering (down, up, north, south, west, east)
const FACE_OF: [u32; 6] = [4, 5, 0, 1, 2, 3];
const ANY: u64 = !0;
/// points of a face tried for a line of sight, in 1/100 block from its middle
const QUAD: [(i32, i32); 5] = [(0, 0), (25, 25), (25, -25), (-25, 25), (-25, -25)];

/// Octant of the view direction (x, z), as the click masks count them (yaw from -180 in steps of 45 degrees).
#[inline(always)]
fn yaw_bin(vx: i32, vz: i32) -> u32 {
    if vx > 0 { if vz < 0 { if -vz >= vx { 0 } else { 1 } } else if vx >= vz { 2 } else { 3 } }
    else if vz > 0 { if -vx < vz { 4 } else { 5 } } else if -vx > -vz { 6 } else { 7 }
}
/// feet heights tried for a stance, relative to the cell (A's `DYS`)
const DY_LO: usize = 4;
const DY_HI: usize = 2;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StanceKind { Natural, SelfPlace, Lean, Column }

#[derive(Clone, Copy, Debug)]
pub struct Stance { pub c: usize, pub y: usize, pub kind: StanceKind }

/// Policy switches (the baseline is A's behaviour; the others are the algorithm experiments).
#[derive(Clone, Debug)]
pub struct Policy {
    /// reach used when planning a stance / when clicking, in 1/100 block
    pub reach_plan: i32,
    pub reach: i32,
    pub seed: usize,
}
impl Default for Policy {
    fn default() -> Policy { Policy { reach_plan: 415, reach: 450, seed: 0 } }
}

#[derive(Default, Debug, Clone)]
pub struct Stats {
    pub placed: u64,
    pub helpers: u64,
    pub escapes: u64,
    pub skipped: u64,
    pub put_off: u64,
    pub trips: u64,
    pub scaffold_placed: u64,
    pub broken: u64,
    pub from_here: u64,
    pub self_placed: u64,
    pub moves: u64,
    pub plans: u64,
    pub plan_fails: u64,
    pub main_ticks: u64,
    pub out_of_stock: u64,
    pub no_way: u64,
    pub trip_ticks: u64,
    pub move_ticks: u64,
    pub cleanup_ticks: u64,
}

pub struct Bot<'a> {
    pub w: World,
    pub nav: Nav,
    /// what actions take, in game ticks (the search's own costs, `nav.costs`, are the planner's weights)
    pub time: Times,
    pub b: &'a Build,
    pub pol: Policy,
    pub c: usize,
    pub y: usize,
    pub ticks: u64,
    pub max_ticks: u64,
    pub inv: Vec<u32>,
    pub stock: Vec<u32>,
    need: Vec<u32>,
    next: Vec<i16>,
    elig: Vec<u64>,
    n_elig: usize,
    deferred: Vec<u32>,
    chg: Vec<u32>,
    bsig: Vec<u32>,
    tries: Vec<u8>,
    blocked: Vec<u64>,
    blocked_cols: Vec<u32>,
    tower_feet: Vec<u32>,
    loose_list: Vec<u32>,
    bad_stand: Vec<u32>,
    ring: Vec<(i32, i32)>,
    scaffold_trips: u32,
    tower_trips: u32,
    main_done: bool,
    late_too: bool,
    pub status: &'static str,
    pub st: Stats,
    pub trace: bool,
    /// (trace) last stance plan: places to stand, of them clickable and routed to, columns not free, columns without a click, columns routed to
    dbg: [u32; 6],
}

impl<'a> Bot<'a> {
    pub fn new(b: &'a Build, pol: Policy, costs: Costs, time: Times) -> Bot<'a> {
        let w = World::new(b);
        let mut nav = Nav::new();
        nav.costs = costs;
        let mut ring: Vec<(i32, i32)> = Vec::new();
        for dx in -4..=4i32 { for dz in -4..=4i32 { if dx * dx + dz * dz <= 17 { ring.push((dx, dz)); } } }
        ring.sort_by_key(|a| a.0 * a.0 + a.1 * a.1);
        // stock: what the target needs plus A's reserve; the scaffold kinds as A's site provides them
        let mut stock = vec![0u32; b.n_items];
        for &p in &b.cells { if p != 0 { let e = b.pal[p as usize]; if e.item >= 2 { stock[e.item as usize] += e.icount as u32; } } }
        for s in stock.iter_mut().skip(2) { *s += 4.max(*s / 20); }
        stock[IT_LOOSE] = (64u64 * 54 * 20).min(64 * 27 + b.blocks as u64 * 3 + 64 * b.sy as u64 * 2) as u32;
        stock[IT_TOWER] = (64u64 * 54 * 20).min(64 * 27 + b.blocks as u64 * 3 + 64 * b.sy as u64 * 8) as u32;
        let ncol = w.nx * w.nz;
        // A's start: two cells north of the build's corner (plus the seed along x)
        let sx = w.ox + pol.seed % b.sx.min(12).max(1);
        let c = (w.oz - 2) * w.nx + sx;
        Bot { nav, time, b, pol, c, y: 1, ticks: 0, max_ticks: TICK_LIMIT, inv: vec![0; b.n_items], stock, need: vec![0; b.n_items], next: vec![NONE; ncol],
            elig: vec![0; ncol.div_ceil(64) + 1], n_elig: 0, deferred: Vec::new(), chg: vec![0; ncol], bsig: vec![0; ncol], tries: vec![0; ncol], blocked: vec![0; ncol.div_ceil(64) + 1], blocked_cols: Vec::new(), tower_feet: Vec::new(), loose_list: Vec::new(), bad_stand: Vec::new(), ring,
            scaffold_trips: 0, tower_trips: 0, main_done: false, late_too: false, status: "done", st: Stats::default(), trace: false, dbg: [0; 6], w }
    }

    #[inline(always)]
    fn xz(&self, c: usize) -> (i32, i32) { ((c % self.w.nx) as i32, (c / self.w.nx) as i32) }

    // ------------------------------------------------------------------------------------------ order of work
    fn scan_column(&self, c: usize, from: usize) -> i16 {
        match self.w.first_at_or_above(NEED, c, from) { Some(y) => y as i16, None => NONE }
    }

    #[inline(always)]
    fn eligible_now(&self, c: usize) -> bool {
        let y = self.next[c];
        if y == NONE { return false; }
        let nx = self.w.nx;
        let n = &self.next;
        // (columns outside the box hold NONE)
        n[c - 1] >= y && n[c + 1] >= y && n[c - nx] >= y && n[c + nx] >= y && n[c - nx - 1] >= y && n[c - nx + 1] >= y && n[c + nx - 1] >= y && n[c + nx + 1] >= y
    }

    #[inline(always)]
    fn upd_elig(&mut self, c: usize) {
        let e = self.eligible_now(c);
        let (wi, bit) = (c >> 6, 1u64 << (c & 63));
        let was = self.elig[wi] & bit != 0;
        if e != was {
            self.elig[wi] ^= bit;
            if e { self.n_elig += 1; } else { self.n_elig -= 1; }
        }
    }

    fn set_next(&mut self, c: usize, y: i16) {
        if self.next[c] == y { return; }
        self.next[c] = y;
        let nx = self.w.nx;
        for n in [c, c - 1, c + 1, c - nx, c + nx, c - nx - 1, c - nx + 1, c + nx - 1, c + nx + 1] { self.upd_elig(n); }
    }

    #[inline(always)]
    fn attached(&self, c: usize, y: usize) -> bool {
        let nx = self.w.nx;
        self.w.clickable(c, y - 1) || self.w.clickable(c - 1, y) || self.w.clickable(c + 1, y) || self.w.clickable(c - nx, y) || self.w.clickable(c + nx, y) || self.w.clickable(c, y + 1)
    }

    /// A's `pick_next`: the eligible column whose next cell is nearest (with A's penalties). Costs are in tenths.
    fn pick_next(&self) -> Option<(usize, usize)> {
        if self.n_elig == 0 { return None; }
        let w = &self.w;
        let (px, pz) = self.xz(self.c);
        let (x0, x1, z0, z1) = (w.ox as i32, (w.ox + w.sx - 1) as i32, w.oz as i32, (w.oz + w.sz - 1) as i32);
        let (bx, bz, by) = (px.clamp(x0, x1), pz.clamp(z0, z1), self.y as i32);
        let mut best: Option<(usize, usize)> = None;
        let mut best_cost = i32::MAX;
        let max_r = (w.sx.max(w.sz)) as i32;
        let nx = w.nx as i32;
        let eval = |c: usize, r: i32, best: &mut Option<(usize, usize)>, best_cost: &mut i32| {
            if self.blocked[c >> 6] >> (c & 63) & 1 != 0 && self.bsig[c] == self.sig(c) { return; }
            let y = self.next[c] as i32;
            let mut cost = 10 * r + 4 * (y - by).abs();
            if cost >= *best_cost { return; }
            if !self.attached(c, y as usize) { cost += 600 + 20 * (y - by).max(0); }
            if cost >= *best_cost { return; }
            let it = w.pal_at(c, y as usize).item as usize;
            if self.inv[it] == 0 { cost += 4000; }
            if cost < *best_cost { *best_cost = cost; *best = Some((c, y as usize)); }
        };
        for r in 0..=max_r {
            if best.is_some() && 10 * r > best_cost { break; }
            let (xa, xb) = ((bx - r).max(x0), (bx + r).min(x1));
            for z in [bz - r, bz + r] {
                if z < z0 || z > z1 || (r == 0 && z != bz - r) { continue; }
                // bits of the row from xa to xb
                let (mut i, end) = ((z * nx + xa) as usize, (z * nx + xb) as usize);
                while i <= end {
                    let wi = i >> 6;
                    let mut m = self.elig[wi] & (!0u64 << (i & 63));
                    let last = (wi << 6) + 63;
                    if last > end { m &= !0u64 >> (last - end); }
                    while m != 0 {
                        let c = (wi << 6) + m.trailing_zeros() as usize;
                        m &= m - 1;
                        eval(c, r, &mut best, &mut best_cost);
                    }
                    i = last + 1;
                }
            }
            if r > 0 {
                for z in (bz - r + 1).max(z0)..=(bz + r - 1).min(z1) {
                    for x in [bx - r, bx + r] {
                        if x < x0 || x > x1 { continue; }
                        let c = (z * nx + x) as usize;
                        if self.elig[c >> 6] >> (c & 63) & 1 != 0 { eval(c, r, &mut best, &mut best_cost); }
                    }
                }
            }
        }
        best
    }

    // ------------------------------------------------------------------------------------------ clicking
    /// Whether a click from the stance (feet at `sc`, `sy`) can place into cell (`c`, `y`): some neighbour of the
    /// cell offers a face that faces the eye, within reach, with nothing solid on the line. `virt` is a column that
    /// is to be thought standing (column, lowest, highest cell).
    fn can_click(&self, sc: usize, sy: usize, c: usize, y: usize, reach: i32, virt: (usize, usize, usize), mask: u64) -> bool {
        let w = &self.w;
        let nx = w.nx as i32;
        let (sx, sz) = self.xz(sc);
        let (tx, tz) = self.xz(c);
        let (ex, ey, ez) = (sx * 100 + 50, sy as i32 * 100 + 162, sz * 100 + 50);
        let (cx, cy, cz) = (tx * 100 + 50, y as i32 * 100 + 50, tz * 100 + 50);
        let (ddx, ddy, ddz) = (cx - ex, cy - ey, cz - ez);
        let far = reach + 90;
        if ddx * ddx + ddy * ddy + ddz * ddz > far * far { return false; }
        for d in 0..6 {
            let (dx, dy, dz) = (DC6X[d], DC6Y[d], DC6Z[d]);
            let n = (c as i32 + dx + dz * nx) as usize;
            let ny = (y as i32 + dy) as usize;
            if !(w.clickable(n, ny) || (n == virt.0 && ny >= virt.1 && ny <= virt.2)) { continue; }
            // the face between the cell and that neighbour, seen from the cell's side
            let (fx, fy, fz) = (cx + 50 * dx, cy + 50 * dy, cz + 50 * dz);
            if (fx - ex) * dx + (fy - ey) * dy + (fz - ez) * dz <= 0 { continue; }
            let (vx, vy, vz) = (fx - ex, fy - ey, fz - ez);
            if vx * vx + vy * vy + vz * vz > reach * reach { continue; }
            // the block's orientation: this face, seen in this direction, must give the wanted state
            if mask != ANY {
                let fm = mask >> (FACE_OF[d] * 8);
                if if vx == 0 && vz == 0 { fm & 0xff == 0 } else { fm >> yaw_bin(vx, vz) & 1 == 0 } { continue; }
            }
            // line of sight to a point just inside the cell: the middle of the face, else one of its four quarters
            let mut clear = false;
            for q in 0..5 {
                let (oa, ob) = QUAD[q];
                let (ox, oy, oz) = if dx != 0 { (0, oa, ob) } else if dy != 0 { (oa, 0, ob) } else { (oa, ob, 0) };
                let (px, py, pz) = (cx + 40 * dx + ox - ex, cy + 40 * dy + oy - ey, cz + 40 * dz + oz - ez);
                let mut free = true;
                for k in 1..8 {
                    let (qx, qy, qz) = ((ex + px * k / 8) / 100, (ey + py * k / 8) / 100, (ez + pz * k / 8) / 100);
                    let qc = (qz * nx + qx) as usize;
                    if qc == c && qy as usize == y { continue; }
                    if w.get(SOLID, qc, qy as usize) || (qc == virt.0 && qy as usize >= virt.1 && qy as usize <= virt.2) { free = false; break; }
                }
                if free { clear = true; break; }
            }
            if clear { return true; }
        }
        false
    }

    /// Whether the block in (`c`, `y`) can be broken from the stance: within reach, nothing solid in between.
    fn can_break(&self, sc: usize, sy: usize, c: usize, y: usize) -> bool {
        let w = &self.w;
        let nx = w.nx as i32;
        let (sx, sz) = self.xz(sc);
        let (tx, tz) = self.xz(c);
        let (ex, ey, ez) = (sx * 100 + 50, sy as i32 * 100 + 162, sz * 100 + 50);
        let (px, py, pz) = (tx * 100 + 50 - ex, y as i32 * 100 + 50 - ey, tz * 100 + 50 - ez);
        if px * px + py * py + pz * pz > self.pol.reach * self.pol.reach { return false; }
        for k in 1..8 {
            let (qx, qy, qz) = ((ex + px * k / 8) / 100, (ey + py * k / 8) / 100, (ez + pz * k / 8) / 100);
            let qc = (qz * nx + qx) as usize;
            if qc == c && qy as usize == y { continue; }
            if w.get(SOLID, qc, qy as usize) { return false; }
        }
        true
    }

    // ------------------------------------------------------------------------------------------ primitives
    fn place(&mut self, c: usize, y: usize) {
        let p = self.w.pal_at(c, y);
        let it = p.item as usize;
        self.inv[it] = self.inv[it].saturating_sub(p.icount.max(1) as u32);
        self.w.set(PLACED, c, y);
        self.w.clear(NEED, c, y);
        self.w.clear(DEFER, c, y);
        if p.class & C_SOLID != 0 { self.w.set(SOLID, c, y); }
        if p.class & C_CLICK != 0 { self.w.set(CLICK, c, y); }
        self.st.placed += 1;
        self.chg[c] = self.chg[c].wrapping_add(1);
        if self.trace { let (x, z) = self.xz(c); let (bx, bz) = self.xz(self.c); println!("{} P {},{},{} blk @{},{},{}", self.ticks, x - self.w.ox as i32, y - 1, z - self.w.oz as i32, bx - self.w.ox as i32, self.y - 1, bz - self.w.oz as i32); }
    }

    /// Walks the route of the last search, putting up and breaking what its moves say.
    fn travel(&mut self) {
        let mut t = if self.nav.path.is_empty() { 0 } else { self.time.route };
        for k in 0..self.nav.path.len() {
            let (how, id) = self.nav.path[k];
            let (c, y) = (id_c(id), id_y(id));
            t += self.time.mv[how as usize];
            match how {
                TOWER_UP => {
                    self.w.set(TOWER, c, y - 1);
                    self.w.set(CLICK, c, y - 1);
                    self.inv[IT_TOWER] = self.inv[IT_TOWER].saturating_sub(1);
                    self.st.scaffold_placed += 1;
                    if !self.w.get(TOWER, c, y - 2) { self.tower_feet.push(cell_id(c, y - 1)); }
                }
                BRIDGE => {
                    self.w.set(LOOSE, c, y - 1);
                    self.w.set(SOLID, c, y - 1);
                    self.w.set(CLICK, c, y - 1);
                    self.inv[IT_LOOSE] = self.inv[IT_LOOSE].saturating_sub(1);
                    self.st.scaffold_placed += 1;
                    self.loose_list.push(cell_id(c, y - 1));
                }
                MINE => {
                    for yy in [y, y + 1] {
                        if !self.w.get(SOLID, c, yy) { continue; }
                        t += self.time.mv[MINE as usize];
                        self.w.clear(SOLID, c, yy);
                        self.w.clear(CLICK, c, yy);
                        if self.w.get(LOOSE, c, yy) { self.w.clear(LOOSE, c, yy); }
                        else if self.w.get(PLACED, c, yy) {
                            self.w.clear(PLACED, c, yy);
                            self.chg[c] = self.chg[c].wrapping_add(1);
                            self.w.set(DEFER, c, yy);
                            self.deferred.push(cell_id(c, yy));
                            self.st.broken += 1;
                            self.st.put_off += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(&(_, id)) = self.nav.path.last() {
            self.c = id_c(id);
            self.y = id_y(id);
            self.st.moves += 1;
        }
        self.ticks += t as u64;
        self.st.move_ticks += t as u64;
    }

    fn route(&mut self, c: usize, y: usize, budget: u32, towers: bool, mine: bool) -> bool {
        let mut o = Opts { budget, towers: towers && self.inv[IT_TOWER] > 0, bridges: towers && self.inv[IT_LOOSE] > 0, mine };
        let (from, to) = (cell_id(self.c, self.y), cell_id(c, y));
        if !self.nav.search(&self.w, from, to, o) {
            // Out of places, not out of budget: the bot is shut in. A's bot avoids this beforehand (it refuses the
            // block that would shut it in) and breaks out as a last resort; here it breaks out, and mends later.
            if mine || self.nav.exhausted { return false; }
            o.mine = true;
            if !self.nav.search(&self.w, from, to, o) { return false; }
            self.st.escapes += 1;
        }
        // (a route that needs more scaffolding than is carried is no route; the next trip brings more)
        self.nav.tower_moves <= self.inv[IT_TOWER] && self.nav.bridge_moves <= self.inv[IT_LOOSE]
    }

    // ------------------------------------------------------------------------------------------ stances
    fn self_placeable(&self, c: usize, y: usize) -> bool {
        let p = self.w.pal_at(c, y);
        p.class & C_SOLID != 0 && p.mask >> 8 & 0xff != 0 && self.w.standable(c, y) && !self.w.get(SOLID, c, y + 2)
    }

    fn is_bad(&self, c: usize, y: usize) -> bool { self.bad_stand.contains(&cell_id(c, y)) }

    /// A's `plan_stance` for placing: leaves the route to the stance in the search state.
    fn plan_stance(&mut self, c: usize, y: usize, target: bool) -> Option<Stance> {
        self.st.plans += 1;
        let nx = self.w.nx as i32;
        let (tx, tz) = self.xz(c);
        let (bx, bz) = self.xz(self.c);
        let by = self.y as i32;
        let mask = if target { self.w.pal_at(c, y).mask } else { ANY };
        // 0. into the cell itself, placing underfoot with a jump
        if target && self.self_placeable(c, y) && !self.is_bad(c, y) && (bx - tx).abs() + (bz - tz).abs() <= 3 && self.y + 1 >= y && self.route(c, y, 600, true, false) {
            return Some(Stance { c, y, kind: StanceKind::SelfPlace });
        }
        // 1. somewhere one can already stand
        let mut cand: [(u32, u32); 128] = [(0, 0); 128];
        let mut nc = 0;
        let y0 = if y > DY_LO { y - DY_LO } else { 1 };
        let nbits = y + DY_HI - y0 + 1;
        for k in 0..self.ring.len() {
            let (dx, dz) = self.ring[k];
            let q = (c as i32 + dz * nx + dx) as usize;
            let mut m = self.w.stand_mask(q, y0) & ((1u64 << nbits) - 1);
            if dx == 0 && dz == 0 { m &= !(1u64 << (y - y0)); if y > y0 { m &= !(1u64 << (y - 1 - y0)); } }
            let hd = ((tx + dx - bx).abs() + (tz + dz - bz).abs()) as u32;
            while m != 0 {
                let fy = y0 + m.trailing_zeros() as usize;
                m &= m - 1;
                if nc < 128 { cand[nc] = (hd + 3 * (fy as i32 - by).max(0) as u32, cell_id(q, fy)); nc += 1; }
            }
        }
        let mut tries = 0;
        self.dbg = [nc as u32, 0, 0, 0, 0, 0];
        while tries < 4 {
            let mut k = usize::MAX;
            let mut best = u32::MAX;
            for j in 0..nc { if cand[j].0 < best { best = cand[j].0; k = j; } }
            if k == usize::MAX { break; }
            cand[k].0 = u32::MAX;
            let (q, fy) = (id_c(cand[k].1), id_y(cand[k].1));
            if self.is_bad(q, fy) || !self.can_click(q, fy, c, y, self.pol.reach_plan, (0, 0, 0), mask) { continue; }
            tries += 1;
            self.dbg[1] += 1;
            if self.route(q, fy, 6000, true, false) { return Some(Stance { c: q, y: fy, kind: StanceKind::Natural }); }
        }
        // 2. on top of a block beside the cell, leaning out over it
        let mut best: Option<(u32, usize)> = None;
        for d in [1isize, -1, nx as isize, -(nx as isize)] {
            let n = (c as isize + d) as usize;
            if !self.lean_ok(n, y + 1, c, y, mask) || !self.w.standable(n, y + 1) || self.is_bad(n, y + 1) { continue; }
            if self.route(n, y + 1, 6000, true, false) && best.map_or(true, |b| self.nav.goal_cost < b.0) { best = Some((self.nav.goal_cost, n)); }
        }
        if let Some((_, n)) = best {
            if self.route(n, y + 1, 6000, true, false) { return Some(Stance { c: n, y: y + 1, kind: StanceKind::Lean }); }
        }
        // 3. a scaffold column near the cell
        if self.inv[IT_TOWER] == 0 { return None; }
        for k in 1..self.ring.len() {
            let (dx, dz) = self.ring[k];
            if dx * dx + dz * dz > 8 { break; }
            let q = (c as i32 + dz * nx + dx) as usize;
            let adjacent = dx.abs() + dz.abs() == 1;
            let hi = if adjacent { y } else { y - 1 };
            let lo = if y > 2 { y - 2 } else { 1 };
            let mut tp = hi;
            while tp >= lo && tp >= 1 {
                let t = tp;
                tp -= 1;
                if self.w.win(SOLID, q, t + 1) & 3 != 0 || self.is_bad(q, t + 1) { continue; }
                let base = self.w.floor_below(q, t + 1);
                if base == t || base + 40 < t { continue; }
                if !self.w.get(SOLID, q, base) && !self.w.get(TOWER, q, base) { continue; }
                // the column's cells must be free and outside the target
                let mut free = true;
                for yy in base + 1..=t { if self.w.get(TGT, q, yy) || self.w.get(PLACED, q, yy) { free = false; break; } }
                if !free { self.dbg[2] += 1; continue; }
                if !self.can_click(q, t + 1, c, y, self.pol.reach_plan, (q, base + 1, t), mask) { self.dbg[3] += 1; continue; }
                self.dbg[4] += 1;
                if self.route(q, t + 1, 6000, true, false) { return Some(Stance { c: q, y: t + 1, kind: StanceKind::Column }); }
            }
        }
        None
    }

    /// A stance to break the block in (`c`, `y`) from; not in or on column `c` when `off_column`.
    fn plan_break(&mut self, c: usize, y: usize, off_column: bool, towers: bool, mine: bool) -> bool {
        let nx = self.w.nx as i32;
        let (tx, tz) = self.xz(c);
        let (bx, bz) = self.xz(self.c);
        let by = self.y as i32;
        let mut cand: [(u32, u32); 128] = [(0, 0); 128];
        let mut nc = 0;
        let y0 = if y > DY_LO { y - DY_LO } else { 1 };
        let nbits = y + DY_HI - y0 + 1;
        for k in 0..self.ring.len() {
            let (dx, dz) = self.ring[k];
            if dx == 0 && dz == 0 && off_column { continue; }
            let q = (c as i32 + dz * nx + dx) as usize;
            let mut m = self.w.stand_mask(q, y0) & ((1u64 << nbits) - 1);
            if dx == 0 && dz == 0 { m &= !(1u64 << (y + 1 - y0)); }
            let hd = ((tx + dx - bx).abs() + (tz + dz - bz).abs()) as u32;
            while m != 0 {
                let fy = y0 + m.trailing_zeros() as usize;
                m &= m - 1;
                if nc < 128 { cand[nc] = (hd + 3 * (fy as i32 - by).max(0) as u32, cell_id(q, fy)); nc += 1; }
            }
        }
        let mut tries = 0;
        while tries < 4 {
            let mut k = usize::MAX;
            let mut best = u32::MAX;
            for j in 0..nc { if cand[j].0 < best { best = cand[j].0; k = j; } }
            if k == usize::MAX { break; }
            cand[k].0 = u32::MAX;
            let (q, fy) = (id_c(cand[k].1), id_y(cand[k].1));
            if !self.can_break(q, fy, c, y) { continue; }
            tries += 1;
            if self.route(q, fy, 6000, towers, mine) { return true; }
        }
        false
    }

    // ------------------------------------------------------------------------------------------ one cell
    fn body_in(&self, c: usize, y: usize) -> bool { c == self.c && (y == self.y || y == self.y + 1) }

    fn can_self(&self, c: usize, y: usize) -> bool { y == self.y && c == self.c && !self.w.get(SOLID, c, y + 2) && { let p = self.w.pal_at(c, y); p.class & C_SOLID != 0 && p.mask >> 8 & 0xff != 0 } }

    /// Standing on a block right beside the cell, one up: the cell can be had by leaning over the edge (a click on
    /// the side of the block underfoot, looking back at it).
    fn lean_ok(&self, sc: usize, sy: usize, c: usize, y: usize, mask: u64) -> bool {
        if sy != y + 1 || !self.w.get(SOLID, sc, y) { return false; }
        let nx = self.w.nx as isize;
        let d = sc as isize - c as isize;
        let k = if d == 1 { 0 } else if d == -1 { 1 } else if d == nx { 4 } else if d == -nx { 5 } else { return false };
        mask == ANY || mask >> (FACE_OF[k] * 8 + yaw_bin(DC6X[k], DC6Z[k])) & 1 != 0
    }

    /// Whether the cell can be clicked into from where the bot is now: 0 no, 1 by a click, 2 underfoot (with a jump).
    fn click_here(&mut self, c: usize, y: usize, target: bool) -> u8 {
        let mask = if target { self.w.pal_at(c, y).mask } else { ANY };
        if self.body_in(c, y) {
            if target && self.can_self(c, y) { 2 } else { 0 }
        } else if self.can_click(self.c, self.y, c, y, self.pol.reach, (0, 0, 0), mask) { 1 }
        else if self.lean_ok(self.c, self.y, c, y, mask) { self.ticks += self.time.lean as u64; 1 }
        else { 0 }
    }

    /// Gets the bot to where it can click into the cell (A's `attempt`): from here, or from a planned stance.
    /// Returns as `click_here`.
    fn get_to(&mut self, c: usize, y: usize, target: bool) -> u8 {
        let r = self.click_here(c, y, target);
        if r != 0 { self.st.from_here += 1; return r; }
        for _ in 0..3 {
            let Some(st) = self.plan_stance(c, y, target) else {
                self.st.plan_fails += 1;
                if self.trace { let (x, z) = self.xz(c); let (bx, bz) = self.xz(self.c); println!("{} F {},{},{} from {},{},{} attached={} mask={:x} target={} sup={} dbg={:?} exh={} bad={}", self.ticks, x - self.w.ox as i32, y as i32 - 1, z - self.w.oz as i32, bx - self.w.ox as i32, self.y - 1, bz - self.w.oz as i32, self.attached(c, y), if target { self.w.pal_at(c, y).mask } else { 0 }, target, self.dbg_sup(c, y, target), self.dbg, self.nav.exhausted, self.bad_stand.len()); }
                return 0;
            };
            self.travel();
            self.ticks += self.time.arrive as u64;
            let r = self.click_here(c, y, target);
            if r != 0 { self.bad_stand.clear(); return r; }
            self.bad_stand.push(cell_id(st.c, st.y));
        }
        0
    }

    /// (trace) the six neighbours of a cell: c = can be clicked on, n = target cell still to do, d = put off, . = empty;
    /// upper case where the block's click mask allows that side.
    fn dbg_sup(&self, c: usize, y: usize, target: bool) -> String {
        let nx = self.w.nx as i32;
        let mask = if target { self.w.pal_at(c, y).mask } else { ANY };
        let mut s = String::new();
        for d in 0..6 {
            let n = (c as i32 + DC6X[d] + DC6Z[d] * nx) as usize;
            let ny = (y as i32 + DC6Y[d]) as usize;
            let ch = if self.w.clickable(n, ny) { 'c' } else if self.w.get(NEED, n, ny) { 'n' } else if self.w.get(DEFER, n, ny) { 'd' } else if self.w.get(PLACED, n, ny) { 'p' } else if self.w.get(TGT, n, ny) { 's' } else { '.' };
            s.push(if mask >> (FACE_OF[d] * 8) & 0xff != 0 { ch.to_ascii_uppercase() } else { ch });
        }
        s
    }

    /// A loose block may stand in the cell for a while (A's `scaffold_ok`): outside the target, or in a target cell
    /// whose turn has not come.
    fn loose_ok(&self, c: usize, y: usize) -> bool {
        if self.w.get(SOLID, c, y) || self.w.get(TOWER, c, y) || self.w.get(PLACED, c, y) { return false; }
        !self.w.get(TGT, c, y) || self.main_done || (self.next[c] != NONE && y as i16 > self.next[c])
    }

    /// A's `place_helper`: lends the cell a face to click on, a loose block put against something beside it, on a
    /// side whose face can give the wanted block. Sides in `skip` (bits) have been tried.
    fn place_helper(&mut self, c: usize, y: usize, mask: u64, skip: u32) -> Option<(usize, usize, usize)> {
        if self.inv[IT_LOOSE] == 0 { return None; }
        let nx = self.w.nx as i32;
        for d in 0..6 {
            if skip >> d & 1 != 0 || mask >> (FACE_OF[d] * 8) & 0xff == 0 { continue; }
            let hc = (c as i32 + DC6X[d] + DC6Z[d] * nx) as usize;
            let hy = (y as i32 + DC6Y[d]) as usize;
            if hy < 1 || hy + 3 >= self.w.ny || !self.loose_ok(hc, hy) { continue; }
            let mut anchored = false;
            for e in 0..6 {
                let ac = (hc as i32 + DC6X[e] + DC6Z[e] * nx) as usize;
                let ay = (hy as i32 + DC6Y[e]) as usize;
                if ac == c && ay == y { continue; }
                if self.w.clickable(ac, ay) { anchored = true; break; }
            }
            if !anchored { continue; }
            if self.get_to(hc, hy, false) != 0 {
                self.ticks += self.time.place as u64;
                self.w.set(LOOSE, hc, hy);
                self.w.set(SOLID, hc, hy);
                self.w.set(CLICK, hc, hy);
                self.inv[IT_LOOSE] -= 1;
                self.st.scaffold_placed += 1;
                self.st.helpers += 1;
                return Some((hc, hy, d));
            }
        }
        None
    }

    fn remove_loose(&mut self, c: usize, y: usize) {
        if self.break_at(c, y, false, false, false) {
            self.w.clear(LOOSE, c, y);
            self.w.clear(SOLID, c, y);
            self.w.clear(CLICK, c, y);
            self.ticks += self.time.break_loose as u64;
        } else {
            self.loose_list.push(cell_id(c, y));
        }
    }

    fn do_cell(&mut self, c: usize, y: usize) -> u8 {
        let p = self.w.pal_at(c, y);
        let it = p.item as usize;
        if self.w.get(LOOSE, c, y) {
            // a loose block of the bot's own stands in the cell: it goes first
            self.w.clear(LOOSE, c, y);
            self.w.clear(SOLID, c, y);
            self.w.clear(CLICK, c, y);
            self.ticks += self.time.break_loose as u64;
        }
        if self.inv[it] == 0 || (self.inv[IT_LOOSE] < 12 && self.stock[IT_LOOSE] > 0) {
            self.restock(Some(it));
            if self.inv[it] == 0 { self.st.out_of_stock += 1; return SKIP_R; }
        }
        if p.class & C_GRAVITY != 0 && !self.w.get(SOLID, c, y - 1) { return DEFER_R; }
        let mut r = self.get_to(c, y, true);
        if r == 0 && (self.main_done || !self.attached(c, y) || p.mask != ANY) {
            // no face to click that gives the wanted block: lend it one, on one side after the other
            let mut skip = 0u32;
            while r == 0 {
                let Some((hc, hy, d)) = self.place_helper(c, y, p.mask, skip) else { break };
                skip |= 1 << d;
                r = self.get_to(c, y, true);
                if r != 0 { self.finish(c, y, r); }
                self.remove_loose(hc, hy);
            }
            return if r != 0 { PLACED_R } else { DEFER_R };
        }
        if r != 0 { self.finish(c, y, r); }
        if r != 0 { PLACED_R } else { DEFER_R }
    }

    fn finish(&mut self, c: usize, y: usize, r: u8) {
        if r == 2 { self.ticks += self.time.self_place as u64; self.place(c, y); self.y += 1; self.st.self_placed += 1; }
        else { self.ticks += self.time.place as u64; self.place(c, y); }
    }

    #[inline(always)]
    fn sig(&self, c: usize) -> u32 {
        let nx = self.w.nx;
        let g = &self.chg;
        g[c] + g[c - 1] + g[c + 1] + g[c - nx] + g[c + nx] + g[c - nx - 1] + g[c - nx + 1] + g[c + nx - 1] + g[c + nx + 1]
    }

    fn put_off(&mut self, c: usize, y: usize) {
        self.w.clear(NEED, c, y);
        self.w.set(DEFER, c, y);
        self.deferred.push(cell_id(c, y));
        self.st.put_off += 1;
    }

    fn advance(&mut self, c: usize, y: usize) {
        self.tries[c] = 0;
        self.blocked[c >> 6] &= !(1u64 << (c & 63));
        let ny = self.scan_column(c, y + 1);
        self.set_next(c, ny);
    }

    fn work(&mut self, c: usize, y: usize) {
        let r = self.do_cell(c, y);
        match r {
            PLACED_R => {}
            DEFER_R => {
                // A: the cell waits until something around it changes; after 12 tries it is put off
                self.tries[c] += 1;
                if self.tries[c] < 12 {
                    if self.blocked[c >> 6] >> (c & 63) & 1 == 0 { self.blocked[c >> 6] |= 1u64 << (c & 63); self.blocked_cols.push(c as u32); }
                    self.bsig[c] = self.sig(c);
                    return;
                }
                self.put_off(c, y);
            }
            _ => { self.w.clear(NEED, c, y); self.st.skipped += 1; }
        }
        self.advance(c, y);
        if r == PLACED_R && !self.deferred.is_empty() {
            // A: a put-off neighbour of what was just placed gets another try at once
            let nx = self.w.nx as isize;
            for (dc, dy) in [(1isize, 0isize), (-1, 0), (0, 1), (0, -1), (nx, 0), (-nx, 0)] {
                let (n, yy) = ((c as isize + dc) as usize, (y as isize + dy) as usize);
                if !self.w.get(DEFER, n, yy) { continue; }
                let p = self.w.pal_at(n, yy);
                if p.class & C_LATE != 0 || self.inv[p.item as usize] == 0 { continue; }
                if self.do_cell(n, yy) == SKIP_R { self.w.clear(DEFER, n, yy); self.st.skipped += 1; }
            }
        }
    }

    /// Nothing can be picked: every cell that waits is put off, and its column goes on (A's main loop).
    fn flush_blocked(&mut self) -> bool {
        let cols = std::mem::take(&mut self.blocked_cols);
        let mut any = false;
        for &c in &cols {
            let c = c as usize;
            if self.blocked[c >> 6] >> (c & 63) & 1 == 0 { continue; }
            let y = self.next[c];
            if y == NONE { self.blocked[c >> 6] &= !(1u64 << (c & 63)); continue; }
            self.put_off(c, y as usize);
            self.advance(c, y as usize);
            any = true;
        }
        any
    }

    // ------------------------------------------------------------------------------------------ trips
    fn access(&self, chest: usize) -> usize {
        let (row, col) = (chest / self.w.depot_cols, chest % self.w.depot_cols);
        (self.w.oz - ROW_GAP - row * ROW_PITCH + 1) * self.w.nx + self.w.ox + col * 2
    }

    /// A's `restock`: goes to the chests and loads up for the cells that come next (nearest columns, lowest cells first).
    fn restock(&mut self, must: Option<usize>) {
        let t0 = self.ticks;
        self.st.trips += 1;
        for n in self.need.iter_mut() { *n = 0; }
        let mut slots = 0i32;
        if let Some(it) = must { self.need[it] = self.stack_of(it); slots += 1; }
        let want_loose = (64 * 4).min(64 + 64 * self.scaffold_trips);
        let want_tower = (64 * 6).min(128 + 64 * self.tower_trips);
        if self.inv[IT_LOOSE] < 16 { self.scaffold_trips += 1; }
        if self.inv[IT_TOWER] < 16 { self.tower_trips += 1; }
        let reserved = (want_loose.div_ceil(64) + want_tower.div_ceil(64)) as i32;
        let capacity = 35 - reserved;
        if self.main_done {
            for k in 0..self.deferred.len() {
                let id = self.deferred[k];
                let (c, y) = (id_c(id), id_y(id));
                if !self.w.get(DEFER, c, y) || slots >= 16 { continue; }
                let p = self.w.pal_at(c, y);
                if !self.late_too && p.class & C_LATE != 0 { continue; }
                if self.need[p.item as usize] == 0 { slots += 1; }
                self.need[p.item as usize] += p.icount.max(1) as u32;
            }
        }
        // the columns around the bot, ring by ring, in A's four passes
        let (px, pz) = self.xz(self.c);
        let (x0, x1, z0, z1) = (self.w.ox as i32, (self.w.ox + self.w.sx - 1) as i32, self.w.oz as i32, (self.w.oz + self.w.sz - 1) as i32);
        let (bx, bz) = (px.clamp(x0, x1), pz.clamp(z0, z1));
        let max_r = self.w.sx.max(self.w.sz) as i32;
        let nx = self.w.nx as i32;
        let mut budget: i32 = 36 * 64 * 3;
        // (number of items whose planned count is not a whole number of stacks: when there is none and no slot is
        // free, nothing more can be added, and the pass is over)
        let mut partial = 0i32;
        if let Some(it) = must { let _ = it; }
        'outer: for pass in 0..4 {
            let (from, to) = match pass { 0 | 1 => (0usize, 1usize), 2 => (1, 4), _ => (4, 10) };
            if slots >= capacity && pass >= 2 && partial == 0 { break; }
            if self.n_elig == 0 && pass == 0 { continue; }
            for r in 0..=max_r {
                if slots >= capacity && partial == 0 { break; }
                for dz in -r..=r {
                    let z = bz + dz;
                    if z < z0 || z > z1 { continue; }
                    let step = if dz == -r || dz == r { 1 } else { (2 * r).max(1) };
                    let mut dx = -r;
                    while dx <= r {
                        let x = bx + dx;
                        dx += step;
                        if x < x0 || x > x1 { continue; }
                        let c = (z * nx + x) as usize;
                        let yn = self.next[c];
                        if yn == NONE { continue; }
                        if pass < 2 && (self.elig[c >> 6] >> (c & 63) & 1 != 0) != (pass == 0) { continue; }
                        let ya = yn as usize + from;
                        let mut m = self.w.win(NEED, c, ya) & ((1u64 << (to - from)) - 1);
                        while m != 0 {
                            let yy = ya + m.trailing_zeros() as usize;
                            m &= m - 1;
                            let p = self.w.pal_at(c, yy);
                            let it = p.item as usize;
                            let (max, now, add) = (p.stack as u32, self.need[it], p.icount.max(1) as u32);
                            if now == 0 || (now + add - 1) / max != (now - 1) / max {
                                if slots + 1 > capacity { continue; }
                                slots += 1;
                            }
                            if now % max != 0 { partial -= 1; }
                            self.need[it] = now + add;
                            if (now + add) % max != 0 { partial += 1; }
                            budget -= 1;
                            if budget < 0 { break 'outer; }
                        }
                    }
                }
            }
        }
        self.need[IT_LOOSE] += want_loose;
        self.need[IT_TOWER] += want_tower;
        // what to take, and from which chests
        let mut stacks = 0u32;
        let mut chests: [usize; 64] = [usize::MAX; 64];
        let mut nch = 0;
        let mut any = false;
        for it in 0..self.need.len() {
            let (need, have) = (self.need[it], self.inv[it]);
            if need <= have { continue; }
            let take = (need - have).min(self.stock[it]);
            if take == 0 { continue; }
            any = true;
            self.stock[it] -= take;
            self.inv[it] += take;
            stacks += take.div_ceil(self.stack_of(it));
            let ch = self.b.item_chest[it] as usize;
            if !chests[..nch].contains(&ch) && nch < 64 { chests[nch] = ch; nch += 1; }
        }
        if !any { return; }
        // what is carried and not wanted goes back
        for it in 2..self.need.len() { if self.inv[it] > self.need[it] { self.stock[it] += self.inv[it] - self.need[it]; self.inv[it] = self.need[it]; } }
        let k = self.time.clone();
        let first = self.access(chests[0]);
        if !(self.c == first && self.y == 1) {
            let mut ok = self.route(first, 1, 20000, true, false) || self.route(first, 1, 20000, false, false);
            if !ok { ok = self.route(first, 1, 40000, false, true); }
            if ok { self.travel(); } else {
                // no way found: the walk is charged as the crow flies, and the bot is put there (counted, see STATUS)
                let (ax, az) = self.xz(first);
                self.ticks += (k.mv[WALK as usize] * ((px - ax).abs() + (pz - az).abs()) as u32 + k.mv[DROP as usize] * self.y as u32) as u64;
                self.c = first; self.y = 1;
                self.st.no_way += 1;
            }
        }
        let mut at = first;
        for j in 1..nch {
            let a = self.access(chests[j]);
            let ((ax, az), (qx, qz)) = (self.xz(a), self.xz(at));
            self.ticks += (k.mv[WALK as usize] * ((ax - qx).abs() + (az - qz).abs()) as u32) as u64;
            at = a;
        }
        self.c = at;
        self.ticks += (k.chest_open * nch as u32 + k.chest_stack * stacks) as u64;
        self.st.trip_ticks += self.ticks - t0;
        if self.trace { println!("{} T {} stacks={} chests={}", self.ticks, self.st.trips, stacks, nch); }
    }

    fn stack_of(&self, it: usize) -> u32 {
        if it < 2 { return 64; }
        self.b.pal.iter().find(|p| p.item as usize == it).map(|p| p.stack as u32).unwrap_or(64)
    }

    // ------------------------------------------------------------------------------------------ after the main pass
    fn time_short(&self) -> bool {
        let standing = self.st.scaffold_placed;
        self.max_ticks.saturating_sub(self.ticks) < 45 * standing.min(20000) + 40000
    }

    /// A's `deferred_pass`: the put-off cells, the nearest first.
    fn deferred_pass(&mut self) {
        if self.time_short() { return; }
        self.deferred.sort_unstable();
        self.deferred.dedup();
        let mut cells: Vec<u32> = self.deferred.clone();
        cells.retain(|&id| self.w.get(DEFER, id_c(id), id_y(id)));
        let n = cells.len();
        let mut done = vec![false; n];
        let pos: Vec<(i32, i32, i32)> = cells.iter().map(|&id| { let (x, z) = self.xz(id_c(id)); (x, id_y(id) as i32, z) }).collect();
        for _ in 0..n {
            let (bx, bz) = self.xz(self.c);
            let by = self.y as i32;
            let mut k = usize::MAX;
            let mut best = i32::MAX;
            for j in 0..n { if !done[j] { let d = (pos[j].0 - bx).abs() + (pos[j].2 - bz).abs() + 2 * (pos[j].1 - by).abs(); if d < best { best = d; k = j; } } }
            if k == usize::MAX { break; }
            done[k] = true;
            if self.time_short() { break; }
            let (c, y) = (id_c(cells[k]), id_y(cells[k]));
            if !self.w.get(DEFER, c, y) { continue; }
            if !self.late_too && self.w.pal_at(c, y).class & C_LATE != 0 { continue; }
            if self.do_cell(c, y) == SKIP_R { self.w.clear(DEFER, c, y); self.st.skipped += 1; }
        }
        let mut all = std::mem::take(&mut self.deferred);
        all.retain(|&id| self.w.get(DEFER, id_c(id), id_y(id)));
        self.deferred = all;
    }

    fn break_at(&mut self, c: usize, y: usize, off_column: bool, towers: bool, mine: bool) -> bool {
        let here = self.can_break(self.c, self.y, c, y) && !(off_column && self.c == c) && !(self.c == c && self.y == y + 1);
        if !here {
            if !self.plan_break(c, y, off_column, towers, mine) { return false; }
            self.travel();
            self.ticks += self.time.arrive as u64;
        }
        true
    }

    /// Takes down what scaffolding stands: columns from the foot (the rest comes down with it), the highest first;
    /// then the loose blocks.
    fn cleanup(&mut self, mine: bool) {
        let t0 = self.ticks;
        let mut feet: Vec<u32> = std::mem::take(&mut self.tower_feet);
        feet.retain(|&id| self.w.get(TOWER, id_c(id), id_y(id)) && !self.w.get(TOWER, id_c(id), id_y(id) - 1));
        feet.sort_unstable();
        feet.dedup();
        let top_of = |w: &World, id: u32| -> usize { let mut y = id_y(id); while w.get(TOWER, id_c(id), y + 1) { y += 1; } y };
        feet.sort_by_key(|&id| std::cmp::Reverse(top_of(&self.w, id)));
        let mut left: Vec<u32> = Vec::new();
        for &id in &feet {
            let (c, y) = (id_c(id), id_y(id));
            if !self.w.get(TOWER, c, y) { continue; }
            if self.break_at(c, y, true, false, mine) {
                let mut yy = y;
                while self.w.get(TOWER, c, yy) { self.w.clear(TOWER, c, yy); self.w.clear(CLICK, c, yy); yy += 1; }
                self.ticks += self.time.break_tower as u64;
            } else { left.push(id); }
        }
        left.extend_from_slice(&self.tower_feet);
        self.tower_feet = left;
        let mut loose: Vec<u32> = std::mem::take(&mut self.loose_list);
        loose.retain(|&id| self.w.get(LOOSE, id_c(id), id_y(id)));
        let mut left: Vec<u32> = Vec::new();
        for &id in &loose {
            let (c, y) = (id_c(id), id_y(id));
            if self.break_at(c, y, false, false, mine) || self.break_at(c, y, false, true, mine) {
                self.w.clear(LOOSE, c, y);
                self.w.clear(SOLID, c, y);
                self.w.clear(CLICK, c, y);
                self.ticks += self.time.break_loose as u64;
            } else { left.push(id); }
        }
        left.extend_from_slice(&self.loose_list);
        self.loose_list = left;
        self.st.cleanup_ticks += self.ticks - t0;
    }

    fn scaffold_standing(&self) -> bool { !self.tower_feet.is_empty() || !self.loose_list.is_empty() }

    // ------------------------------------------------------------------------------------------ the run
    pub fn run(&mut self) {
        self.st.skipped = self.w.skipped_unplaceable as u64;
        for bz in 0..self.w.sz { for bx in 0..self.w.sx {
            let c = (self.w.oz + bz) * self.w.nx + self.w.ox + bx;
            self.next[c] = self.scan_column(c, 1);
        } }
        for bz in 0..self.w.sz { for bx in 0..self.w.sx { let c = (self.w.oz + bz) * self.w.nx + self.w.ox + bx; self.upd_elig(c); } }
        for k in 0..self.w.sx * self.w.sz {
            // (the late kinds wait in the put-off list from the start)
            let c = (self.w.oz + k / self.w.sx) * self.w.nx + self.w.ox + k % self.w.sx;
            let mut from = 1;
            while let Some(y) = self.w.first_at_or_above(DEFER, c, from) { self.deferred.push(cell_id(c, y)); from = y + 1; }
        }
        self.restock(None);
        let mut out_of_time = false;
        loop {
            if self.time_short() { out_of_time = true; break; }
            let Some((c, y)) = self.pick_next() else { if self.flush_blocked() { continue; } break };
            self.work(c, y);
        }
        self.st.main_ticks = self.ticks;
        self.main_done = true;
        self.deferred_pass();
        self.late_too = true;
        self.deferred_pass();
        self.cleanup(false);
        self.deferred_pass();
        self.cleanup(false);
        let mut round = 0;
        while round < 3 && (self.scaffold_standing() || self.deferred.iter().any(|&id| self.w.get(DEFER, id_c(id), id_y(id)))) && !self.time_short() {
            self.cleanup(true);
            self.deferred_pass();
            self.cleanup(false);
            round += 1;
        }
        if self.scaffold_standing() { self.cleanup(true); }
        for k in 0..self.deferred.len() {
            let id = self.deferred[k];
            if self.w.get(DEFER, id_c(id), id_y(id)) { self.w.clear(DEFER, id_c(id), id_y(id)); self.st.skipped += 1; }
        }
        if out_of_time || self.ticks >= self.max_ticks { self.status = "out of time"; }
    }
}
