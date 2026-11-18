//! The bot: action primitives on the world (with their tick costs) and the baseline policy, a coarse equivalent of
//! A's bot (`/a/sim-rs/sim/src/bot`): order of work, stance choice, scaffold columns, trips, put-off cells, clean-up.
use crate::nav::*;
use crate::prep::{Build, Pal, C2_FLOATS, C2_FULL, C_CLICK, C_GRAVITY, C_INTERACT, C_LATE, C_SOLID, NO_TAB, TAB_BYTES};
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
/// points of a face tried for a line of sight, in 1/64 block from its middle
const QUAD: [(i32, i32); 5] = [(0, 0), (16, 16), (16, -16), (-16, 16), (-16, -16)];

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

/// A way taken from the cost table instead of a search (see `Bot::route`).
#[derive(Clone, Copy, Debug)]
struct Virt { c: usize, y: usize, ticks: u32, towers: u32 }

#[derive(Clone, Copy, Debug)]
pub struct Stance { pub c: usize, pub y: usize, pub kind: StanceKind }

/// Policy switches (the baseline is A's behaviour; the others are the algorithm experiments).
#[derive(Clone, Debug)]
pub struct Policy {
    /// reach used when planning a stance / when clicking, in 1/100 block
    pub reach_plan: i32,
    pub reach: i32,
    pub seed: usize,
    /// search budget: cells expanded, a base plus so many per cell of distance
    pub search_base: u32,
    pub search_per_cell: u32,
    /// where the search finds no way, take one from the cost table
    pub virtual_routes: bool,
    /// 0 off; 1: where the short search fails, A's own route search (linked in, on a mirror of this world) is asked and
    /// its "no way" fails the route; 2: as 1, and on "no way" A's search is asked again with breaking out allowed.
    /// a way that needs more scaffolding than is carried sends the bot to the chests first (A); off: the way is taken with what is carried
    pub fetch_scaffold: bool,
    /// (switch, off by default) the load of a trip is limited by 36 inventory places and comes in whole stacks, as in A; off: exactly what the list asks for
    pub slots: bool,
    pub a_routes: u32,
    /// 0 off; 1: before a cell with an orientation or a click table is tried, A's stance planner (linked in, on the
    /// mirror) is asked, and its "no way to place" counts as a failed try; 2: asked for every cell.
    pub a_place: u32,
    /// the pick's weight of a layer of height difference, in tenths of a column (A: 4)
    pub pick_dy: i32,
    /// stance 0, walking into the cell and placing underfoot (A: on)
    pub self_place: bool,
    /// the pick's penalty for a cell whose material is not carried, in tenths (A: 4000)
    pub pick_uncarried: i32,
    /// the pick counts distance only beyond this many columns (0 = A): cells about within reach rank by height alone
    pub pick_near: i32,
    /// fidelity level (the ladder): 0 = the coarse default; 1 = A's click tables (face, quarter, yaw and pitch class,
    /// sneaking against things that open) instead of the 48-bit mask
    pub level: u32,
    /// A's rules for helper blocks: only for blocks that stay without support; a second helper to carry the first
    /// must itself be put against something real, three tries (default: the pair is charged and assumed to work)
    pub strict_helpers: bool,
    /// A's revision steps, for the benchmark (on by default; they act with `strict_helpers`): `orphan` = a cell about
    /// to be put off gets a try with a chained helper; `close` = a helper on every side in turn, and clicks from
    /// spots off the middle of the block once everything else stands
    pub orphan: bool,
    pub close: bool,
    /// `chain` = the chained helper also in builds with pistons (off: A before that step)
    pub chain: bool,
    /// level 2: a bot whose short search runs out of places (not out of budget) is shut in: it gets nowhere unless
    /// `escape` (A's step: an enclosed bot breaks a block), which is charged as one block broken and put back
    pub shut_in: bool,
    pub escape: bool,
}
impl Default for Policy {
    fn default() -> Policy { Policy { reach_plan: 415, reach: 450, seed: 0, search_base: 24, search_per_cell: 4, virtual_routes: true, fetch_scaffold: true, slots: false, a_routes: 0, a_place: 0, pick_dy: 4, self_place: true, pick_uncarried: 4000, pick_near: 0, level: 0, strict_helpers: false, orphan: true, close: true, chain: true, shut_in: false, escape: true } }
}

#[derive(Default, Debug, Clone)]
pub struct Stats {
    pub p_asked: u64, pub p_no: u64, pub p_unknown: u64,
    pub a_asked: u64, pub a_no: u64, pub a_unknown: u64, pub a_mined: u64, pub a_ns: u64,
    pub placed: u64,
    pub helpers: u64,
    pub shut: u64,
    pub picks: u64,
    pub forced: u64,
    pub chained: u64,
    pub virtual_routes: u64,
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
    /// scaffolding by source: cost-table towers, searched towers, bridging, helper blocks
    pub sc: [u64; 5],
    pub mvh: [u64; 8],
    /// scaffolding placed by the end of the main pass, and in the clean-ups
    pub ph: [u64; 2],
    /// trips by cause: material out, loose blocks low, a way needing scaffolding
    pub tw: [u64; 3],
    /// (debug, --a-routes 9) routes both find: count, fastsim tower, bridge, cost; A tower, scaffold, cost; path cells fastsim, A
    pub cmp: [u64; 9],
}

pub struct Bot<'a> {
    pub oracle: Option<mcsim::fsoracle::RouteOracle>,
    pub dirty: Vec<u32>,
    pub last_a: (i32, i32, i32, i32),
    pub last_path: String,
    no_trip: bool,
    pub w: World<'a>,
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
    /// the items of the load being planned, in the order A's list has them
    need_order: Vec<u32>,
    next: Vec<i16>,
    elig: Vec<u64>,
    att: Vec<u64>,
    car: Vec<u64>,
    cls: [Vec<u64>; 3],
    clt: [Vec<u64>; 3],
    n_good: usize,
    stale: Vec<u32>,
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
    virt: Option<Virt>,
    helper_base: Option<(usize, usize)>,
    /// where in its cell the bot stands, in 1/64 block from the middle (used by the off-centre click tries)
    eye_off: (i32, i32),
    /// a cell about to be put off gets a try with a chained helper (A's `orphan`)
    chain_now: bool,
    /// (replay) A's reason for giving up a cell -> what fastsim would do: counts
    pub q2: std::collections::BTreeMap<String, u64>,
}

impl<'a> Bot<'a> {
    #[inline(never)]
    pub fn new(b: &'a Build, pol: Policy, costs: Costs, time: Times) -> Bot<'a> {
        let w = World::new(b);
        let mut nav = Nav::new();
        nav.costs = costs;
        let mut ring: Vec<(i32, i32)> = Vec::new();
        for dx in -4..=4i32 { for dz in -4..=4i32 { if dx * dx + dz * dz <= 17 { ring.push((dx, dz)); } } }
        ring.sort_by_key(|a| a.0 * a.0 + a.1 * a.1);
        // stock: what the target needs plus A's reserve; the scaffold kinds as A's site provides them
        let mut stock = b.item_total.clone();
        for s in stock.iter_mut().skip(2) { *s += 4.max(*s / 20); }
        stock[IT_LOOSE] = (64u64 * 54 * 20).min(64 * 27 + b.blocks as u64 * 3 + 64 * b.sy as u64 * 2) as u32;
        stock[IT_TOWER] = (64u64 * 54 * 20).min(64 * 27 + b.blocks as u64 * 3 + 64 * b.sy as u64 * 8) as u32;
        let ncol = w.nx * w.nz;
        // A's start: two cells north of the build's corner (plus the seed along x)
        let sx = w.ox + pol.seed % b.sx.min(12).max(1);
        let c = (w.oz - 2) * w.nx + sx;
        Bot { oracle: None, dirty: Vec::new(), last_a: (0, 0, 0, 0), last_path: String::new(), no_trip: false, nav, time, b, pol, c, y: 1, ticks: 0, max_ticks: TICK_LIMIT, inv: vec![0; b.n_items], stock, need: vec![0; b.n_items], need_order: Vec::new(), next: vec![NONE; ncol],
            elig: vec![0; ncol.div_ceil(64) + 1], att: vec![0; ncol.div_ceil(64) + 1], car: vec![0; ncol.div_ceil(64) + 1], cls: [vec![0; ncol.div_ceil(64) + 1], vec![0; ncol.div_ceil(64) + 1], vec![0; ncol.div_ceil(64) + 1]], clt: [vec![0; ncol.div_ceil(64) + 1], vec![0; ncol.div_ceil(64) + 1], vec![0; ncol.div_ceil(64) + 1]], n_good: 0, stale: Vec::new(), n_elig: 0, deferred: Vec::new(), chg: vec![0; ncol], bsig: vec![0; ncol], tries: vec![0; ncol], blocked: vec![0; ncol.div_ceil(64) + 1], blocked_cols: Vec::new(), tower_feet: Vec::new(), loose_list: Vec::new(), bad_stand: Vec::new(), ring,
            scaffold_trips: 0, tower_trips: 0, main_done: false, late_too: false, status: "done", st: Stats::default(), trace: false, dbg: [0; 6], virt: None, helper_base: None, eye_off: (0, 0), chain_now: false, q2: Default::default(), w }
    }

    #[inline(always)]
    fn xz(&self, c: usize) -> (i32, i32) { self.w.xz(c) }

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
    fn upd_elig(&mut self, c: usize, always: bool) {
        let e = self.eligible_now(c);
        let (wi, bit) = (c >> 6, 1u64 << (c & 63));
        let was = self.elig[wi] & bit != 0;
        if e != was {
            self.elig[wi] ^= bit;
            if e { self.n_elig += 1; } else { self.n_elig -= 1; }
        }
        if e != was || always { self.upd_att(c); }
    }

    /// `att`: the column is eligible and its next cell has something beside it to click on. `car`: it is eligible
    /// and the material of its next cell is carried.
    #[inline(always)]
    fn upd_att(&mut self, c: usize) {
        let (wi, bit) = (c >> 6, 1u64 << (c & 63));
        if self.elig[wi] & bit != 0 {
            let y = self.next[c] as usize;
            if self.attached(c, y) { self.att[wi] |= bit; } else { self.att[wi] &= !bit; }
            if self.inv[self.w.pal_at(c, y).item as usize] > 0 { self.car[wi] |= bit; } else { self.car[wi] &= !bit; }
        } else {
            self.att[wi] &= !bit;
            self.car[wi] &= !bit;
        }
        self.upd_good(c);
    }

    /// The pick's three classes of columns (attached and carried / carried, not attached / not carried), each as a
    /// row-major and a column-major bitset, brought in line with `elig`, `att`, `car` for one column.
    #[inline(always)]
    fn upd_good(&mut self, c: usize) {
        let (wi, bit) = (c >> 6, 1u64 << (c & 63));
        let (e, a, k) = (self.elig[wi] & bit != 0, self.att[wi] & bit != 0, self.car[wi] & bit != 0);
        let want = [e && a && k, e && k && !a, e && !k];
        for i in 0..3 {
            if want[i] != (self.cls[i][wi] & bit != 0) {
                self.cls[i][wi] ^= bit;
                let (x, z) = self.w.xz(c);
                let t = x as usize * self.w.nz + z as usize;
                self.clt[i][t >> 6] ^= 1u64 << (t & 63);
                if i == 0 { if want[0] { self.n_good += 1; } else { self.n_good -= 1; } }
            }
        }
    }

    /// After a trip: which columns' material is carried is worked out afresh.
    fn rebuild_carried(&mut self) {
        for wi in 0..self.elig.len() {
            let mut m = self.elig[wi];
            while m != 0 {
                let c = (wi << 6) + m.trailing_zeros() as usize;
                m &= m - 1;
                let bit = 1u64 << (c & 63);
                if self.inv[self.w.pal_at(c, self.next[c] as usize).item as usize] > 0 { self.car[wi] |= bit; } else { self.car[wi] &= !bit; }
                self.upd_good(c);
            }
        }
    }

    /// Something appeared in or went from cell (`c`, `y`) that can be clicked on: the columns around it whose next
    /// cell is beside it may have changed their state in `att`.
    #[inline(always)]
    fn click_changed(&mut self, c: usize, y: usize) {
        if self.pol.a_routes | self.pol.a_place != 0 { self.dirty.push(cell_id(c, y)); }
        let nx = self.w.nx;
        let yy = y as i16;
        for n in [c - 1, c + 1, c - nx, c + nx] { if self.next[n] == yy { self.upd_att(n); } }
        if self.next[c] == yy + 1 || self.next[c] == yy - 1 { self.upd_att(c); }
    }

    fn set_next(&mut self, c: usize, y: i16) {
        if self.next[c] == y { return; }
        self.next[c] = y;
        let nx = self.w.nx;
        self.upd_elig(c, true);
        for n in [c - 1, c + 1, c - nx, c + nx, c - nx - 1, c - nx + 1, c + nx - 1, c + nx + 1] { self.upd_elig(n, false); }
    }

    #[inline(always)]
    fn attached(&self, c: usize, y: usize) -> bool {
        let nx = self.w.nx;
        self.w.clickable(c, y - 1) || self.w.clickable(c - 1, y) || self.w.clickable(c + 1, y) || self.w.clickable(c - nx, y) || self.w.clickable(c + nx, y) || self.w.clickable(c, y + 1)
    }

    /// A's `pick_next`: the eligible column whose next cell is nearest (with A's penalties). Costs are in tenths:
    /// 10 per column of distance, 4 per layer, +600 and 20 per layer up when nothing stands beside the cell,
    /// +4000 when its material is not carried.
    ///
    /// Scanning every eligible column out to where a penalised cell could still win was most of the pick's cost
    /// (3 % of the picks made 70 % of the evaluations). So the columns are kept in classes, as bitsets updated
    /// where the world changes: `att` (something stands beside the next cell), `car` (its material is carried; may
    /// be stale after the last piece went, and is put right when met), `good` = both. The classes are scanned in
    /// order of their penalty, each only as far out as it could still win. The result is the same.
    #[inline(never)]
    fn pick_next(&mut self) -> Option<(usize, usize)> {
        if self.n_elig == 0 { return None; }
        let mut best: Option<(usize, usize)> = None;
        let mut bc = i32::MAX;
        let max_r = self.w.sx.max(self.w.sz) as i32;
        let r60 = 60 + self.pol.pick_near;
        if self.n_good > 0 { self.scan_rings(0, 0, max_r.min(r60), &mut best, &mut bc); }
        if bc > 600 {
            self.scan_rings(1, 0, max_r, &mut best, &mut bc);
            if bc > 610 && self.n_good > 0 && max_r > r60 { self.scan_rings(0, r60 + 1, max_r, &mut best, &mut bc); }
        }
        // (columns found to have lost their material move to the last class before it is looked at)
        while let Some(c) = self.stale.pop() {
            let c = c as usize;
            self.car[c >> 6] &= !(1u64 << (c & 63));
            self.upd_good(c);
        }
        if bc >= self.pol.pick_uncarried { self.scan_rings(2, 0, max_r, &mut best, &mut bc); }
        self.st.picks += 1;
        best
    }

    /// One class of columns, ring by ring around the bot: 0 = attached and carried, 1 = not attached but carried,
    /// 2 = material not carried.
    #[inline(always)]
    fn scan_rings(&mut self, kind: u8, r_from: i32, r_to: i32, best: &mut Option<(usize, usize)>, best_cost: &mut i32) {
        let (px, pz) = self.xz(self.c);
        let (x0, x1, z0, z1) = (self.w.ox as i32, (self.w.ox + self.w.sx - 1) as i32, self.w.oz as i32, (self.w.oz + self.w.sz - 1) as i32);
        let (bx, bz, by) = (px.clamp(x0, x1), pz.clamp(z0, z1), self.y as i32);
        let nx = self.w.nx as i32;
        let base = match kind { 0 => 0, 1 => 600, _ => self.pol.pick_uncarried };
        let near = self.pol.pick_near;
        let mut stale = std::mem::take(&mut self.stale);
        {
            let this = &*self;
            let mut eval = |c: usize, r: i32, best: &mut Option<(usize, usize)>, best_cost: &mut i32| {
                let y = this.next[c] as i32;
                let mut cost = 10 * (r - near).max(0) + this.pol.pick_dy * (y - by).abs() + base;
                if cost >= *best_cost { return; }
                if this.blocked[c >> 6] >> (c & 63) & 1 != 0 && this.bsig[c] == this.sig(c) { return; }
                if kind == 1 { cost += 20 * (y - by).max(0); }
                else if kind == 2 && !this.attached(c, y as usize) { cost += 600 + 20 * (y - by).max(0); }
                if cost >= *best_cost { return; }
                if kind != 2 && this.inv[this.w.pal_at(c, y as usize).item as usize] == 0 { stale.push(c as u32); return; }
                *best_cost = cost;
                *best = Some((c, y as usize));
            };
            let (cls, clt) = (&this.cls[kind as usize], &this.clt[kind as usize]);
            let nz = this.w.nz as i32;
            for r in r_from..=r_to {
                if best.is_some() && 10 * (r - near).max(0) + base > *best_cost { break; }
                // the two rows of the ring: bits xa..xb of the row-major set
                let (xa, xb) = ((bx - r).max(x0), (bx + r).min(x1));
                for z in [bz - r, bz + r] {
                    if z < z0 || z > z1 || (r == 0 && z != bz - r) { continue; }
                    let (mut i, end) = ((z * nx + xa) as usize, (z * nx + xb) as usize);
                    while i <= end {
                        let wi = i >> 6;
                        let mut m = cls[wi] & (!0u64 << (i & 63));
                        let last = (wi << 6) + 63;
                        if last > end { m &= !0u64 >> (last - end); }
                        while m != 0 {
                            let c = (wi << 6) + m.trailing_zeros() as usize;
                            m &= m - 1;
                            eval(c, r, best, best_cost);
                        }
                        i = last + 1;
                    }
                }
                // its two columns: bits za..zb of the column-major set
                let (za, zb) = ((bz - r + 1).max(z0), (bz + r - 1).min(z1));
                if r > 0 && za <= zb {
                    for x in [bx - r, bx + r] {
                        if x < x0 || x > x1 { continue; }
                        let (mut i, end) = ((x * nz + za) as usize, (x * nz + zb) as usize);
                        while i <= end {
                            let wi = i >> 6;
                            let mut m = clt[wi] & (!0u64 << (i & 63));
                            let last = (wi << 6) + 63;
                            if last > end { m &= !0u64 >> (last - end); }
                            while m != 0 {
                                let t = (wi << 6) + m.trailing_zeros() as usize;
                                m &= m - 1;
                                let z = t as i32 - x * nz;
                                eval((z * nx + x) as usize, r, best, best_cost);
                            }
                            i = last + 1;
                        }
                    }
                }
            }
        }
        self.stale = stale;
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
        let (ex, ey, ez) = (sx * 64 + 32 + self.eye_off.0, sy as i32 * 64 + 104, sz * 64 + 32 + self.eye_off.1);
        let (cx, cy, cz) = (tx * 64 + 32, y as i32 * 64 + 32, tz * 64 + 32);
        if self.pol.level >= 1 && mask != ANY {
            let p = w.pal_at(c, y);
            if p.tab != NO_TAB { return self.click_exact(ex, ey, ez, c, y, reach, virt, p, 0x3f, false); }
        }
        let (ddx, ddy, ddz) = (cx - ex, cy - ey, cz - ez);
        let reach = reach * 64 / 100;
        let far = reach + 58;
        if ddx * ddx + ddy * ddy + ddz * ddz > far * far { return false; }
        for d in 0..6 {
            let (dx, dy, dz) = (DC6X[d], DC6Y[d], DC6Z[d]);
            let n = (c as i32 + dx + dz * nx) as usize;
            let ny = (y as i32 + dy) as usize;
            if !(w.clickable(n, ny) || (n == virt.0 && ny >= virt.1 && ny <= virt.2)) { continue; }
            // the face between the cell and that neighbour, seen from the cell's side
            let (fx, fy, fz) = (cx + 32 * dx, cy + 32 * dy, cz + 32 * dz);
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
                let (px, py, pz) = (cx + 26 * dx + ox - ex, cy + 26 * dy + oy - ey, cz + 26 * dz + oz - ez);
                let mut free = true;
                for k in 1..8 {
                    let (qx, qy, qz) = ((ex + ((px * k) >> 3)) >> 6, (ey + ((py * k) >> 3)) >> 6, (ez + ((pz * k) >> 3)) >> 6);
                    let qc = (qz * nx + qx) as usize;
                    if qc == c && qy as usize == y { continue; }
                    if self.blocks_sight(qc, qy as usize) || (qc == virt.0 && qy as usize >= virt.1 && qy as usize <= virt.2) { free = false; break; }
                }
                if free { clear = true; break; }
            }
            if clear { return true; }
        }
        false
    }

    /// Whether what stands in the cell is in the way of a click. Level 0: anything solid. Level 1: full cubes only
    /// (slabs, stairs, walls, panes and the like are solid to walk on here, but most of a line of sight passes them).
    #[inline(always)]
    fn blocks_sight(&self, c: usize, y: usize) -> bool {
        if !self.w.get(SOLID, c, y) { return false; }
        if self.pol.level == 0 { return true; }
        !(self.w.get(PLACED, c, y) && self.w.get(TGT, c, y) && self.w.pal_at(c, y).class2 & C2_FULL == 0)
    }

    /// Level 1: A's `find_click` for a block with a click table. Eye in 1/64 block; for each neighbour that can be
    /// clicked on, the four quarter points of the face between it and the cell: within reach, the table's entry for
    /// (face, quarter, yaw class, pitch class, sneaking) gives the block, and nothing solid on the line.
    /// `faces`: bits of the directions (as `DC6*`) that may be used. `sneak_only`: the stance is a sneaking one.
    fn click_exact(&self, ex: i32, ey: i32, ez: i32, c: usize, y: usize, reach: i32, virt: (usize, usize, usize), p: Pal, faces: u32, sneak_only: bool) -> bool {
        let w = &self.w;
        let nx = w.nx as i32;
        let (tx, tz) = self.xz(c);
        let (cx, cy, cz) = (tx * 64 + 32, y as i32 * 64 + 32, tz * 64 + 32);
        let reach = reach * 64 / 100 - 3;
        let tab = &self.b.tables[p.tab as usize * TAB_BYTES..(p.tab as usize + 1) * TAB_BYTES];
        for d in 0..6 {
            if faces >> d & 1 == 0 { continue; }
            let (dx, dy, dz) = (DC6X[d], DC6Y[d], DC6Z[d]);
            let n = (c as i32 + dx + dz * nx) as usize;
            let ny = (y as i32 + dy) as usize;
            let virt_here = n == virt.0 && ny >= virt.1 && ny <= virt.2;
            if !(w.clickable(n, ny) || virt_here) { continue; }
            // a click on something that opens or switches does that instead of placing, unless the bot sneaks
            let inter = !virt_here && w.get(PLACED, n, ny) && w.get(TGT, n, ny) && w.pal_at(n, ny).class & C_INTERACT != 0;
            let sneak = inter || sneak_only;
            let ey = if inter && !sneak_only { ey - 22 } else { ey };
            let (fx, fy, fz) = (cx + 32 * dx, cy + 32 * dy, cz + 32 * dz);
            if (fx - ex) * dx + (fy - ey) * dy + (fz - ez) * dz <= 1 { continue; }
            let face = FACE_OF[d] as usize;
            for q in 1..5 {
                let (oa, ob) = QUAD[q];
                // (u, v) on the face as A's tables count the quarters: x then z on top and bottom, x then y on north
                // and south, z then y on west and east
                let (ox, oy, oz, qd) = if dx != 0 { (0, oa, ob, (ob > 0) as usize | ((oa > 0) as usize) << 1) }
                    else if dy != 0 { (oa, 0, ob, (oa > 0) as usize | ((ob > 0) as usize) << 1) }
                    else { (oa, ob, 0, (oa > 0) as usize | ((ob > 0) as usize) << 1) };
                let (vx, vy, vz) = (fx + ox - ex, fy + oy - ey, fz + oz - ez);
                if vx * vx + vy * vy + vz * vz > reach * reach { continue; }
                let hd = ((vx * vx + vz * vz) as f32).sqrt();
                let yaw = if hd < 0.01 { 0.0 } else { (-(vx as f32)).atan2(vz as f32).to_degrees() };
                let pitch = -(vy as f32).atan2(hd).to_degrees();
                let yc = (((yaw + 180.0) / 11.25).floor() as i32).clamp(0, 31) as usize;
                let pc = (((pitch + 80.0) / 20.0 + 0.5).floor() as i32).clamp(0, 8) as usize;
                let idx = (((face * 4 + qd) * 32 + yc) * 9 + pc) * 2 + sneak as usize;
                if tab[idx >> 3] >> (idx & 7) & 1 == 0 { continue; }
                let (px, py, pz) = (vx - 6 * dx, vy - 6 * dy, vz - 6 * dz);
                let mut free = true;
                for k in 1..8 {
                    let (qx, qy, qz) = ((ex + ((px * k) >> 3)) >> 6, (ey + ((py * k) >> 3)) >> 6, (ez + ((pz * k) >> 3)) >> 6);
                    let qc = (qz * nx + qx) as usize;
                    if qc == c && qy as usize == y { continue; }
                    if self.blocks_sight(qc, qy as usize) || (qc == virt.0 && qy as usize >= virt.1 && qy as usize <= virt.2) { free = false; break; }
                }
                if free { return true; }
            }
        }
        false
    }

    /// Whether the block in (`c`, `y`) can be broken from the stance: within reach, nothing solid in between.
    fn can_break(&self, sc: usize, sy: usize, c: usize, y: usize) -> bool {
        let w = &self.w;
        let nx = w.nx as i32;
        let (sx, sz) = self.xz(sc);
        let (tx, tz) = self.xz(c);
        let (ex, ey, ez) = (sx * 64 + 32, sy as i32 * 64 + 104, sz * 64 + 32);
        let (px, py, pz) = (tx * 64 + 32 - ex, y as i32 * 64 + 32 - ey, tz * 64 + 32 - ez);
        let reach = self.pol.reach * 64 / 100;
        if px * px + py * py + pz * pz > reach * reach { return false; }
        for k in 1..8 {
            let (qx, qy, qz) = ((ex + ((px * k) >> 3)) >> 6, (ey + ((py * k) >> 3)) >> 6, (ez + ((pz * k) >> 3)) >> 6);
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
        if self.pol.a_routes | self.pol.a_place != 0 { self.dirty.push(cell_id(c, y)); }
        self.w.clear(NEED, c, y);
        self.w.clear(DEFER, c, y);
        if p.class & C_SOLID != 0 { self.w.set(SOLID, c, y); }
        if p.class & C_CLICK != 0 { self.w.set(CLICK, c, y); self.click_changed(c, y); }
        self.st.placed += 1;
        self.chg[c] = self.chg[c].wrapping_add(1);
        if self.trace { let (x, z) = self.xz(c); let (bx, bz) = self.xz(self.c); println!("{} P {},{},{} blk @{},{},{}", self.ticks, x - self.w.ox as i32, y - 1, z - self.w.oz as i32, bx - self.w.ox as i32, self.y - 1, bz - self.w.oz as i32); }
    }

    /// Walks the route of the last search, putting up and breaking what its moves say.
    #[inline(never)]
    fn travel(&mut self) {
        if let Some(v) = self.virt.take() {
            // the way from the cost table: the scaffolding it stands for is put under the place arrived at
            let mut yy = v.y - 1;
            let mut n = 0;
            while n < v.towers && yy >= 1 && !self.w.get(SOLID, v.c, yy) && !self.w.get(TOWER, v.c, yy) && !self.w.get(TGT, v.c, yy) {
                self.w.set(TOWER, v.c, yy);
                self.w.set(CLICK, v.c, yy); self.click_changed(v.c, yy);
                n += 1;
                yy -= 1;
            }
            if n > 0 && !self.w.get(TOWER, v.c, yy) { self.st.sc[4] += 1; self.tower_feet.push(cell_id(v.c, yy + 1)); }
            self.inv[IT_TOWER] -= v.towers;
            self.st.scaffold_placed += v.towers as u64; self.st.sc[0] += v.towers as u64;
            self.c = v.c;
            self.y = v.y;
            self.ticks += v.ticks as u64;
            self.st.move_ticks += v.ticks as u64;
            self.st.moves += 1;
            self.st.virtual_routes += 1;
            return;
        }
        let mut t = if self.nav.path.is_empty() { 0 } else { self.time.route };
        for k in 0..self.nav.path.len() {
            let (how, id) = self.nav.path[k];
            let (c, y) = (id_c(id), id_y(id));
            t += self.time.mv[how as usize]; self.st.mvh[how as usize] += 1;
            match how {
                TOWER_UP => {
                    self.w.set(TOWER, c, y - 1);
                    self.w.set(CLICK, c, y - 1); self.click_changed(c, y - 1);
                    self.inv[IT_TOWER] = self.inv[IT_TOWER].saturating_sub(1);
                    self.st.scaffold_placed += 1; self.st.sc[1] += 1;
                    if !self.w.get(TOWER, c, y - 2) { self.st.sc[4] += 1; self.tower_feet.push(cell_id(c, y - 1)); }
                }
                BRIDGE => {
                    self.w.set(LOOSE, c, y - 1);
                    self.w.set(SOLID, c, y - 1);
                    self.w.set(CLICK, c, y - 1); self.click_changed(c, y - 1);
                    self.inv[IT_LOOSE] = self.inv[IT_LOOSE].saturating_sub(1);
                    self.st.scaffold_placed += 1; self.st.sc[2] += 1;
                    self.loose_list.push(cell_id(c, y - 1));
                }
                MINE => {
                    for yy in [y, y + 1] {
                        if !self.w.get(SOLID, c, yy) { continue; }
                        t += self.time.mv[MINE as usize];
                        self.w.clear(SOLID, c, yy);
                        self.w.clear(CLICK, c, yy); self.click_changed(c, yy);
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

    /// Finds a way to (`c`, `y`): a short search first (most moves are a few cells); where that finds nothing, the
    /// way is taken from the cost table instead (`Virt`): walking distance with a detour factor, the height gained
    /// on new scaffolding. The bot always gets there; hard places cost time, not the build.
    #[inline(never)]
    fn route(&mut self, c: usize, y: usize, budget: u32, towers: bool, _mine: bool) -> bool {
        let (bx, bz) = self.xz(self.c);
        let (gx, gz) = self.xz(c);
        let manh = ((bx - gx).abs() + (bz - gz).abs()) as u32;
        // (A, `go_to_travel`: a way that needs more scaffolding than is carried sends the bot to the chests first)
        let fetch = self.pol.fetch_scaffold && !self.no_trip;
        let o = Opts { budget: budget.min(self.pol.search_base + self.pol.search_per_cell * manh), towers: towers && (self.inv[IT_TOWER] > 0 || (fetch && self.stock[IT_TOWER] > 0)), bridges: towers && self.inv[IT_LOOSE] > 0, mine: false };
        self.virt = None;
        let found = self.nav.search(&self.w, cell_id(self.c, self.y), cell_id(c, y), o);
        if found && fetch && ((self.nav.tower_moves > self.inv[IT_TOWER] && self.stock[IT_TOWER] > 0) || (self.nav.bridge_moves > 0 && self.inv[IT_LOOSE] < self.nav.bridge_moves + 4 && self.stock[IT_LOOSE] > 0)) {
            self.no_trip = true; self.st.tw[2] += 1; self.restock(None); let r = self.route(c, y, budget, towers, _mine); self.no_trip = false;
            return r;
        }
        if found && self.nav.tower_moves <= self.inv[IT_TOWER] && self.nav.bridge_moves <= self.inv[IT_LOOSE] {
            if self.pol.a_routes >= 9 && self.oracle.is_some() {
                let (pl, gc) = (self.nav.path.clone(), self.nav.goal_cost);
                let k = self.ask_a(c, y, towers, false);
                self.st.a_mined += 1; if k == 0 { self.st.a_no += 1; if self.st.a_no <= 3 { let (sx, sz) = self.xz(self.c); let (gx, gz) = self.xz(c); eprintln!("A says no: from {},{},{} to {},{},{} (fastsim path {} cells, cost {})", sx - self.w.ox as i32, self.y, sz - self.w.oz as i32, gx - self.w.ox as i32, y, gz - self.w.oz as i32, pl.len(), gc); } } if k == 2 { self.st.a_unknown += 1; }
                if k == 1 { let (tm, bm) = (self.nav.tower_moves as u64, self.nav.bridge_moves as u64); let la = self.last_a; let q = &mut self.st.cmp; q[0] += 1; q[1] += tm; q[2] += bm; q[3] += gc as u64; q[4] += la.0 as u64; q[5] += la.1 as u64; q[6] += la.2 as u64; q[7] += pl.len() as u64; q[8] += la.3 as u64; }
                if self.trace && k != 2 {
                    let or = self.oracle.take().unwrap();
                    let (ox, oy, oz) = or.origin();
                    for &(how, id) in pl.iter() {
                        if how == TOWER_UP || how == BRIDGE { continue; }
                        let (x, z) = self.xz(id_c(id));
                        let (h, below, feet) = or.stand(ox + x - self.w.ox as i32, oy + id_y(id) as i32 - 1, oz + z - self.w.oz as i32);
                        println!("ST {} {} {} {}", how, h, below, feet);
                    }
                    self.oracle = Some(or);
                }
                if k == 1 && self.trace && self.last_a.0 as u32 > self.nav.tower_moves {
                    // which single steps of fastsim's route does A's search not have?
                    let mut or = self.oracle.take().unwrap();
                    let (ox, oy, oz) = or.origin();
                    let (wox, woz) = (self.w.ox as i32, self.w.oz as i32);
                    let (mut px, mut pz) = self.xz(self.c); let mut py = self.y as i32;
                    for &(how, id) in pl.iter() {
                        let (x, z) = self.xz(id_c(id)); let y = id_y(id) as i32;
                        if how != TOWER_UP && how != BRIDGE {
                            let a = or.route(ox + px - wox, oy + py - 1, oz + pz - woz, ox + x - wox, oy + y - 1, oz + z - woz, 300, false, false);
                            let want = [5, 9, 6 + (py - y).max(0) / 2, 0, 9, 7, 0, 0][how as usize];
                            if a.kind != 1 || a.cost > want + 4 {
                                let f = or.stand(ox + px - wox, oy + py - 1, oz + pz - woz); let t = or.stand(ox + x - wox, oy + y - 1, oz + z - woz);
                                let up = or.stand(ox + px - wox, oy + py + 2, oz + pz - woz); let up2 = or.stand(ox + x - wox, oy + y + 1, oz + z - woz);
                                println!("STEP how {} dy {} A {} cost {} from h{} on {} in {} to h{} on {} in {} | above-from {} above-to {}", how, y - py, a.kind, a.cost, f.0, f.1, f.2, t.0, t.1, t.2, up.1, up2.1);
                            }
                        }
                        px = x; pz = z; py = y;
                    }
                    self.oracle = Some(or);
                }
                if k == 1 && self.trace && self.last_a.0 as u32 > self.nav.tower_moves + 1 {
                    let (sx, sz) = self.xz(self.c);
                    let fp = pl.iter().map(|&(how, id)| { let (x, z) = self.xz(id_c(id)); format!("{}:{},{},{}", how, x - self.w.ox as i32, id_y(id) as i32 - 1, z - self.w.oz as i32) }).collect::<Vec<_>>().join(" ");
                    println!("RC from {},{},{} fastsim towers {} cost {}: {}\nRC   A towers {} cost {}: {}", sx - self.w.ox as i32, self.y as i32 - 1, sz - self.w.oz as i32, self.nav.tower_moves, gc, fp, self.last_a.0, self.last_a.2, self.last_path);
                }
                self.nav.path = pl; self.nav.goal_cost = gc;
            }
            return true;
        }
        if !self.pol.virtual_routes { return false; }
        let mut extra = 0;
        if self.pol.a_routes != 0 && self.pol.a_routes < 9 && self.oracle.is_some() {
            match self.ask_a(c, y, towers, false) {
                0 => {
                    if self.pol.a_routes >= 2 && self.ask_a(c, y, towers, true) == 1 { self.st.a_mined += 1; extra = self.time.mv[MINE as usize] + self.time.arrive + self.time.place; }
                    else { self.st.a_no += 1; return false; }
                }
                2 => self.st.a_unknown += 1,
                _ => {}
            }
        }
        if self.pol.shut_in && !self.nav.exhausted {
            self.st.shut += 1;
            if !self.pol.escape { return false; }
            extra = self.time.mv[MINE as usize] + self.time.arrive + self.time.place;
        }
        // the scaffolding the way stands for: A's bot, finding no way across, comes down and puts a column up under
        // the place from whatever is below it (not just the height gained)
        let mut gap = 0u32;
        { let mut yy = y - 1; while yy >= 1 && !self.w.get(SOLID, c, yy) && !self.w.get(TOWER, c, yy) && !self.w.get(TGT, c, yy) { gap += 1; yy -= 1; } }
        let rise = (y as i32 - self.y as i32).max(0) as u32;
        let up = gap.max(rise);
        if up > self.inv[IT_TOWER] && self.stock[IT_TOWER] > 0 && self.pol.fetch_scaffold && !self.no_trip {
            self.no_trip = true; self.st.tw[2] += 1; self.restock(None); let r = self.route(c, y, budget, towers, _mine); self.no_trip = false;
            return r;
        }
        let up = up.min(self.inv[IT_TOWER]);
        let t = extra + self.time.route + self.time.mv[WALK as usize] * manh * 3 / 2 + self.time.mv[TOWER_UP as usize] * up + if up > rise || y < self.y { self.time.mv[DROP as usize] } else { 0 };
        self.nav.path.clear();
        self.nav.goal_cost = t;
        self.virt = Some(Virt { c, y, ticks: t, towers: up });
        true
    }

    fn sync_a(&mut self, or: &mut mcsim::fsoracle::RouteOracle) {
        let (wox, woz) = (self.w.ox as i32, self.w.oz as i32);
        let (ox, oy, oz) = or.origin();
        for id in self.dirty.drain(..) {
            let (dc, dy) = (id_c(id), id_y(id));
            if dy == 0 { continue; }
            let (x, z) = self.w.xz(dc);
            let kind = if self.w.get(TOWER, dc, dy) { 3 } else if self.w.get(LOOSE, dc, dy) { 2 } else if self.w.get(PLACED, dc, dy) && self.w.get(TGT, dc, dy) { 1 } else { 0 };
            or.set(ox + x - wox, oy + dy as i32 - 1, oz + z - woz, kind);
        }
    }

    /// A's stance planner on the mirror: may this cell be tried? false = A finds no way to place it from here.
    #[inline(never)]
    fn a_allows(&mut self, c: usize, y: usize) -> bool {
        if self.pol.a_place == 0 || self.oracle.is_none() { return true; }
        let p = self.w.pal_at(c, y);
        if self.pol.a_place == 1 && p.tab == NO_TAB && p.mask == ANY { return true; }
        let t0 = std::time::Instant::now();
        let mut or = self.oracle.take().unwrap();
        self.sync_a(&mut or);
        let (wox, woz) = (self.w.ox as i32, self.w.oz as i32);
        let (ox, oy, oz) = or.origin();
        let (sx, sz) = self.xz(self.c);
        let (gx, gz) = self.xz(c);
        let k = or.can_place(ox + sx - wox, oy + self.y as i32 - 1, oz + sz - woz, ox + gx - wox, oy + y as i32 - 1, oz + gz - woz);
        self.oracle = Some(or);
        self.st.p_asked += 1; self.st.a_ns += t0.elapsed().as_nanos() as u64;
        if k == 2 { self.st.p_unknown += 1; }
        if k == 0 { self.st.p_no += 1; }
        k != 0
    }

    /// A's route search on the mirror of this world: 0 no way, 1 a way, 2 cannot be asked.
    #[inline(never)]
    fn ask_a(&mut self, c: usize, y: usize, towers: bool, mine: bool) -> u8 {
        let t0 = std::time::Instant::now();
        let (wox, woz) = (self.w.ox as i32, self.w.oz as i32);
        let (sx, sz) = self.xz(self.c);
        let (gx, gz) = self.xz(c);
        let mut or = self.oracle.take().unwrap();
        self.sync_a(&mut or);
        let (ox, oy, oz) = or.origin();
        let a = or.route(ox + sx - wox, oy + self.y as i32 - 1, oz + sz - woz, ox + gx - wox, oy + y as i32 - 1, oz + gz - woz, 20000, towers, mine);
        self.last_a = (a.tower_moves, a.scaffold_moves, a.cost, a.length as i32);
        if self.trace { let n = &or.bot.nav; self.last_path = (0..n.length).map(|k| format!("{}:{},{},{}", n.mv[k], n.path[k * 3] - ox, n.path[k * 3 + 1] - oy, n.path[k * 3 + 2] - oz)).collect::<Vec<_>>().join(" "); }
        self.oracle = Some(or);
        self.st.a_asked += 1;
        self.st.a_ns += t0.elapsed().as_nanos() as u64;
        a.kind
    }

    // ------------------------------------------------------------------------------------------ stances
    fn self_placeable(&self, c: usize, y: usize) -> bool {
        let p = self.w.pal_at(c, y);
        p.class & C_SOLID != 0 && self.underfoot_ok(p) && self.w.standable(c, y) && !self.w.get(SOLID, c, y + 2)
    }

    /// Some neighbour of the cell stands and offers a face that can give the wanted block.
    #[inline(always)]
    fn has_face(&self, c: usize, y: usize, mask: u64) -> bool {
        let nx = self.w.nx;
        (mask >> 32 & 0xff != 0 && self.w.clickable(c + 1, y)) || (mask >> 40 & 0xff != 0 && self.w.clickable(c - 1, y))
            || (mask & 0xff != 0 && self.w.clickable(c, y + 1)) || (mask >> 8 & 0xff != 0 && self.w.clickable(c, y - 1))
            || (mask >> 16 & 0xff != 0 && self.w.clickable(c + nx, y)) || (mask >> 24 & 0xff != 0 && self.w.clickable(c - nx, y))
    }

    /// The block comes out right when placed on the top of the block underfoot, looking straight down.
    #[inline(always)]
    fn underfoot_ok(&self, p: Pal) -> bool {
        if self.pol.level >= 1 && p.tab != NO_TAB {
            let tab = &self.b.tables[p.tab as usize * TAB_BYTES..(p.tab as usize + 1) * TAB_BYTES];
            // (face up, any quarter, yaw 0, pitch class 8 = straight down, not sneaking)
            return (0..4).any(|q| { let idx = (((4 + q) * 32 + 16) * 9 + 8) * 2; tab[idx >> 3] >> (idx & 7) & 1 != 0 });
        }
        p.mask >> 8 & 0xff != 0
    }

    fn is_bad(&self, c: usize, y: usize) -> bool { self.bad_stand.contains(&cell_id(c, y)) }

    /// A's `plan_stance` for placing: leaves the route to the stance in the search state.
    #[inline(never)]
    fn plan_stance(&mut self, c: usize, y: usize, target: bool) -> Option<Stance> {
        self.st.plans += 1;
        let nx = self.w.nx as i32;
        let (tx, tz) = self.xz(c);
        let (bx, bz) = self.xz(self.c);
        let by = self.y as i32;
        let mask = if target { self.w.pal_at(c, y).mask } else { ANY };
        // (no neighbour with a face that can give the block: only a column right beside the cell can help)
        let faceless = !self.has_face(c, y, mask);
        if !faceless {
        // 0. into the cell itself, placing underfoot with a jump
        if target && self.pol.self_place && self.self_placeable(c, y) && !self.is_bad(c, y) && (bx - tx).abs() + (bz - tz).abs() <= 3 && self.y + 1 >= y && self.route(c, y, 600, true, false) {
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
        }
        // 3. a scaffold column near the cell
        if self.inv[IT_TOWER] == 0 { return None; }
        for k in 1..self.ring.len() {
            let (dx, dz) = self.ring[k];
            if dx * dx + dz * dz > 8 { break; }
            let q = (c as i32 + dz * nx + dx) as usize;
            let adjacent = dx.abs() + dz.abs() == 1;
            if faceless && !adjacent { break; }
            let hi = if adjacent { y } else { y - 1 };
            let lo = if faceless { y } else if y > 2 { y - 2 } else { 1 };
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
                if !self.can_click(q, t + 1, c, y, self.pol.reach_plan, (q, base + 1, t), mask) {
                    // (from the top of a column right beside the cell: leaning over, a click on the column's own side)
                    let k = if dx == 1 { 0 } else if dx == -1 { 1 } else if dz == 1 { 4 } else { 5 };
                    let lean = adjacent && t == y && if self.pol.level >= 1 && mask != ANY && self.w.pal_at(c, y).tab != NO_TAB {
                        // (level 1: A's sneaking click at the edge of the column's top, on the column's own side)
                        let (qx, qz) = self.xz(q);
                        let (ex, ey, ez) = (qx * 64 + 32 - 48 * DC6X[k], (t as i32 + 1) * 64 + 81, qz * 64 + 32 - 48 * DC6Z[k]);
                        self.click_exact(ex, ey, ez, c, y, self.pol.reach_plan, (q, base + 1, t), self.w.pal_at(c, y), 1 << k, true)
                    } else { mask == ANY || mask >> (FACE_OF[k] * 8 + yaw_bin(DC6X[k], DC6Z[k])) & 1 != 0 };
                    if !lean { self.dbg[3] += 1; continue; }
                }
                self.dbg[4] += 1;
                if self.route(q, t + 1, 6000, true, false) { return Some(Stance { c: q, y: t + 1, kind: StanceKind::Column }); }
            }
        }
        None
    }

    /// A stance to break the block in (`c`, `y`) from; not in or on column `c` when `off_column`.
    #[inline(never)]
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

    fn can_self(&self, c: usize, y: usize) -> bool { y == self.y && c == self.c && !self.w.get(SOLID, c, y + 2) && { let p = self.w.pal_at(c, y); p.class & C_SOLID != 0 && self.underfoot_ok(p) } }

    /// Standing on a block right beside the cell, one up: the cell can be had by leaning over the edge (a click on
    /// the side of the block underfoot, looking back at it).
    fn lean_ok(&self, sc: usize, sy: usize, c: usize, y: usize, mask: u64) -> bool {
        if sy != y + 1 || !self.w.clickable(sc, y) { return false; }
        let nx = self.w.nx as isize;
        let d = sc as isize - c as isize;
        let k = if d == 1 { 0 } else if d == -1 { 1 } else if d == nx { 4 } else if d == -nx { 5 } else { return false };
        if self.pol.level >= 1 && mask != ANY {
            let p = self.w.pal_at(c, y);
            if p.tab != NO_TAB {
                let (sx, sz) = self.xz(sc);
                // (A: sneaking at the edge, the eye 0.75 toward the cell and 1.27 above the feet; side faces only)
                let (ex, ey, ez) = (sx * 64 + 32 - 48 * DC6X[k], sy as i32 * 64 + 81, sz * 64 + 32 - 48 * DC6Z[k]);
                return self.click_exact(ex, ey, ez, c, y, self.pol.reach_plan, (0, 0, 0), p, 0x33, true);
            }
        }
        mask == ANY || mask >> (FACE_OF[k] * 8 + yaw_bin(DC6X[k], DC6Z[k])) & 1 != 0
    }

    /// Whether the cell can be clicked into from where the bot is now: 0 no, 1 by a click, 2 underfoot (with a jump).
    fn click_here(&mut self, c: usize, y: usize, target: bool) -> u8 {
        let mask = if target { self.w.pal_at(c, y).mask } else { ANY };
        if self.body_in(c, y) {
            if target && self.can_self(c, y) { 2 } else { 0 }
        } else if self.can_click(self.c, self.y, c, y, self.pol.reach, (0, 0, 0), mask) { 1 }
        else if self.lean_ok(self.c, self.y, c, y, mask) { self.ticks += self.time.lean as u64; 1 }
        else if self.pol.level >= 1 && self.pol.close && self.main_done && target && mask != ANY {
            // A's last resort once everything else stands (`place_close`): from spots off the middle of the block
            let mut ok = 0;
            for off in [(19, 0), (-19, 0), (0, 19), (0, -19)] {
                self.eye_off = off;
                if self.can_click(self.c, self.y, c, y, self.pol.reach, (0, 0, 0), mask) { ok = 1; break; }
            }
            self.eye_off = (0, 0);
            if ok == 1 { self.ticks += self.time.lean as u64; }
            ok
        }
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
    #[inline(never)]
    fn place_helper(&mut self, c: usize, y: usize, mask: u64, skip: u32) -> Option<(usize, usize, usize)> {
        if self.inv[IT_LOOSE] == 0 { return None; }
        if self.pol.strict_helpers && self.w.pal_at(c, y).class2 & C2_FLOATS == 0 { return None; }
        let nx = self.w.nx as i32;
        let mut chain = None;
        let mut direct = false;
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
            if !anchored {
                if chain.is_none() { chain = Some((hc, hy, d)); }
                continue;
            }
            direct = true;
            if self.get_to(hc, hy, false) != 0 {
                self.ticks += self.time.place as u64;
                self.w.set(LOOSE, hc, hy);
                self.w.set(SOLID, hc, hy);
                self.w.set(CLICK, hc, hy); self.click_changed(hc, hy);
                self.inv[IT_LOOSE] -= 1;
                self.st.scaffold_placed += 1;
                self.st.helpers += 1;
                return Some((hc, hy, d));
            }
        }
        // A (once everything else stands): a second helper, one step further, to carry the first. Here: the pair is
        // charged, the first is put up without asking where the second would go.
        if self.pol.strict_helpers {
            // A: only where no side offered a place for a single helper; the second helper (the base) goes against
            // something real beside the first one's cell; three tries
            if direct || !(self.main_done || self.chain_now) || self.inv[IT_LOOSE] < 2 { return None; }
            if self.b.has_pistons && (!self.pol.chain || !self.main_done) { return None; }
            let mut tries = 0;
            for d in 0..6 {
                if tries >= 3 { break; }
                if skip >> d & 1 != 0 || mask >> (FACE_OF[d] * 8) & 0xff == 0 { continue; }
                let hc = (c as i32 + DC6X[d] + DC6Z[d] * nx) as usize;
                let hy = (y as i32 + DC6Y[d]) as usize;
                if hy < 1 || hy + 3 >= self.w.ny || !self.loose_ok(hc, hy) { continue; }
                for e in 0..6 {
                    if tries >= 3 { break; }
                    let bc = (hc as i32 + DC6X[e] + DC6Z[e] * nx) as usize;
                    let by = (hy as i32 + DC6Y[e]) as usize;
                    if (bc == c && by == y) || by < 1 || by + 3 >= self.w.ny || !self.loose_ok(bc, by) { continue; }
                    let mut anchored = false;
                    for g in 0..6 {
                        let ac = (bc as i32 + DC6X[g] + DC6Z[g] * nx) as usize;
                        let ay = (by as i32 + DC6Y[g]) as usize;
                        if (ac == c && ay == y) || (ac == hc && ay == hy) { continue; }
                        if self.w.clickable(ac, ay) { anchored = true; break; }
                    }
                    if !anchored { continue; }
                    tries += 1;
                    if self.get_to(bc, by, false) == 0 { continue; }
                    self.ticks += self.time.place as u64;
                    self.w.set(LOOSE, bc, by); self.w.set(SOLID, bc, by); self.w.set(CLICK, bc, by); self.click_changed(bc, by);
                    self.inv[IT_LOOSE] -= 1;
                    self.st.scaffold_placed += 1;
                    if self.get_to(hc, hy, false) != 0 {
                        self.ticks += self.time.place as u64;
                        self.w.set(LOOSE, hc, hy); self.w.set(SOLID, hc, hy); self.w.set(CLICK, hc, hy); self.click_changed(hc, hy);
                        self.inv[IT_LOOSE] -= 1;
                        self.st.scaffold_placed += 1;
                        self.st.helpers += 1;
                        self.st.chained += 1;
                        self.helper_base = Some((bc, by));
                        return Some((hc, hy, d));
                    }
                    self.remove_loose(bc, by);
                }
            }
            return None;
        }
        if let Some((hc, hy, d)) = chain {
            if self.main_done && self.inv[IT_LOOSE] >= 2 {
                self.ticks += (2 * self.time.place + self.time.arrive + self.time.break_loose) as u64;
                self.w.set(LOOSE, hc, hy);
                self.w.set(SOLID, hc, hy);
                self.w.set(CLICK, hc, hy); self.click_changed(hc, hy);
                self.inv[IT_LOOSE] -= 2;
                self.st.scaffold_placed += 2;
                self.st.helpers += 1;
                self.st.chained += 1;
                return Some((hc, hy, d));
            }
        }
        None
    }

    fn remove_loose(&mut self, c: usize, y: usize) {
        if self.break_at(c, y, false, false, false) {
            self.w.clear(LOOSE, c, y);
            self.w.clear(SOLID, c, y);
            self.w.clear(CLICK, c, y); self.click_changed(c, y);
            self.ticks += self.time.break_loose as u64;
        } else {
            self.loose_list.push(cell_id(c, y));
        }
    }

    #[inline(never)]
    fn do_cell(&mut self, c: usize, y: usize) -> u8 {
        let p = self.w.pal_at(c, y);
        let it = p.item as usize;
        if self.w.get(LOOSE, c, y) {
            // a loose block of the bot's own stands in the cell: it goes first
            self.w.clear(LOOSE, c, y);
            self.w.clear(SOLID, c, y);
            self.w.clear(CLICK, c, y); self.click_changed(c, y);
            self.ticks += self.time.break_loose as u64;
        }
        if self.inv[it] == 0 || (self.inv[IT_LOOSE] < 12 && self.stock[IT_LOOSE] > 0) {
            self.st.tw[if self.inv[it] == 0 { 0 } else { 1 }] += 1;
            self.restock(Some(it));
            if self.inv[it] == 0 { self.st.out_of_stock += 1; return SKIP_R; }
        }
        if p.class & C_GRAVITY != 0 && !self.w.get(SOLID, c, y - 1) { return DEFER_R; }
        let mut r = if self.a_allows(c, y) { self.get_to(c, y, true) } else { 0 };
        if r == 0 && (self.main_done || !self.attached(c, y) || p.mask != ANY) {
            // no face to click that gives the wanted block: lend it one, on one side after the other
            let mut skip = 0u32;
            while r == 0 {
                let Some((hc, hy, d)) = self.place_helper(c, y, p.mask, skip) else { break };
                skip |= 1 << d;
                r = if self.a_allows(c, y) { self.get_to(c, y, true) } else { 0 };
                if r != 0 { self.finish(c, y, r); }
                self.remove_loose(hc, hy);
                if let Some((bc, by)) = self.helper_base.take() { self.remove_loose(bc, by); }
                if self.pol.strict_helpers && !self.pol.close { break; }
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
        if r == PLACED_R && !self.deferred.is_empty() { self.retry_neighbours(c, y); }
    }

    /// A: a put-off neighbour of what was just placed gets another try at once (and here, so do its neighbours in turn).
    #[inline(never)]
    fn retry_neighbours(&mut self, c: usize, y: usize) {
        let nx = self.w.nx as isize;
        let mut stack: [u32; 64] = [0; 64];
        let mut n = 1;
        stack[0] = cell_id(c, y);
        while n > 0 {
            n -= 1;
            let (c, y) = (id_c(stack[n]), id_y(stack[n]));
            for (dc, dy) in [(1isize, 0isize), (-1, 0), (0, 1), (0, -1), (nx, 0), (-nx, 0)] {
                let (m, yy) = ((c as isize + dc) as usize, (y as isize + dy) as usize);
                if !self.w.get(DEFER, m, yy) { continue; }
                let p = self.w.pal_at(m, yy);
                if (p.class & C_LATE != 0 && !self.late_too) || self.inv[p.item as usize] == 0 { continue; }
                match self.do_cell(m, yy) {
                    SKIP_R => { self.w.clear(DEFER, m, yy); self.st.skipped += 1; }
                    PLACED_R => { if n < 64 { stack[n] = cell_id(m, yy); n += 1; } }
                    _ => {}
                }
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
            if self.pol.strict_helpers && self.pol.orphan && self.w.get(NEED, c, y as usize) {
                self.chain_now = true;
                let r = self.do_cell(c, y as usize);
                self.chain_now = false;
                if r == PLACED_R { self.advance(c, y as usize); any = true; continue; }
            }
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

    /// Whether ring `r` around (`bx`, `bz`) holds an eligible column (the pick's class bitsets, both orientations).
    #[inline(always)]
    fn ring_has_eligible(&self, bx: i32, bz: i32, r: i32) -> bool {
        let (x0, x1, z0, z1) = (self.w.ox as i32, (self.w.ox + self.w.sx - 1) as i32, self.w.oz as i32, (self.w.oz + self.w.sz - 1) as i32);
        let (nx, nz) = (self.w.nx as i32, self.w.nz as i32);
        let any = |set: usize, lo: usize, hi: usize| -> bool {
            let mut i = lo;
            while i <= hi {
                let wi = i >> 6;
                let mut m = if set == 0 { self.cls[0][wi] | self.cls[1][wi] | self.cls[2][wi] } else { self.clt[0][wi] | self.clt[1][wi] | self.clt[2][wi] } & (!0u64 << (i & 63));
                let last = (wi << 6) + 63;
                if last > hi { m &= !0u64 >> (last - hi); }
                if m != 0 { return true; }
                i = last + 1;
            }
            false
        };
        let (xa, xb) = ((bx - r).max(x0), (bx + r).min(x1));
        let (za, zb) = ((bz - r).max(z0), (bz + r).min(z1));
        if xa > xb || za > zb { return false; }
        for z in [bz - r, bz + r] { if z >= z0 && z <= z1 && any(0, (z * nx + xa) as usize, (z * nx + xb) as usize) { return true; } }
        for x in [bx - r, bx + r] { if x >= x0 && x <= x1 && any(1, (x * nz + za) as usize, (x * nz + zb) as usize) { return true; } }
        false
    }

    /// A's `restock`: goes to the chests and loads up for the cells that come next (nearest columns, lowest cells first).
    #[inline(never)]
    fn restock(&mut self, must: Option<usize>) {
        self.restock0(must);
        self.rebuild_carried();
    }

    #[inline(never)]
    fn restock0(&mut self, must: Option<usize>) {
        let t0 = self.ticks;
        self.st.trips += 1;
        for n in self.need.iter_mut() { *n = 0; }
        self.need_order.clear();
        let mut slots = 0i32;
        if let Some(it) = must { self.need[it] = self.stack_of(it); slots += 1; self.need_order.push(it as u32); }
        self.need_order.push(IT_LOOSE as u32); self.need_order.push(IT_TOWER as u32);
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
                if self.need[p.item as usize] == 0 { slots += 1; self.need_order.push(p.item as u32); }
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
            // (A does not start the passes over the higher cells once the slots are used up, part stacks or not)
            if slots >= capacity && pass >= 2 { break; }
            if self.n_elig == 0 && pass == 0 { continue; }
            let mut r_full = -1i32;
            for r in 0..=max_r {
                // (A goes on over the whole site to fill the last stacks; here that stops 16 rings after the slots are full)
                if slots >= capacity { if r_full < 0 { r_full = r; } else if r > r_full + 16 { break; } }
                if pass == 0 && !self.ring_has_eligible(bx, bz, r) { continue; }
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
                            if now == 0 { self.need_order.push(it as u32); }
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
        if self.pol.slots {
            // A's inventory has 36 places and a chest gives whole stacks: what is carried and not on the list goes
            // back (and whole stacks beyond the list), then the list is served in its order until no place is free.
            // The first eight entries may push out the entry furthest down the list.
            for it in 2..self.need.len() {
                let st = self.stack_of(it);
                let keep = self.need[it].div_ceil(st) * st;
                if self.inv[it] > keep { self.stock[it] += self.inv[it] - keep; self.inv[it] = keep; }
            }
            let mut free = 36i32;
            for it in 0..self.need.len() { free -= self.inv[it].div_ceil(self.stack_of(it)) as i32; }
            for k in 0..self.need_order.len() {
                let it = self.need_order[k] as usize;
                let st = self.stack_of(it);
                while self.inv[it] < self.need[it] && self.stock[it] > 0 {
                    let mut take = st.min(self.stock[it]);
                    let more = ((self.inv[it] + take).div_ceil(st) - self.inv[it].div_ceil(st)) as i32;
                    if more > free && k < 8 {
                        // the carried entry furthest down the list (or not on it) gives up a stack
                        let mut victim = usize::MAX;
                        for j in (k + 1..self.need_order.len()).rev() { let v = self.need_order[j] as usize; if v >= 2 && self.inv[v] > 0 { victim = v; break; } }
                        if victim != usize::MAX {
                            let vs = self.stack_of(victim);
                            let give = if self.inv[victim] % vs != 0 { self.inv[victim] % vs } else { vs };
                            self.inv[victim] -= give; self.stock[victim] += give; free += 1;
                        }
                    }
                    if more > free {
                        let room = self.inv[it].div_ceil(st) * st - self.inv[it];
                        if room == 0 { break; }
                        take = take.min(room);
                    }
                    free -= ((self.inv[it] + take).div_ceil(st) - self.inv[it].div_ceil(st)) as i32;
                    any = true;
                    self.stock[it] -= take;
                    self.inv[it] += take;
                    stacks += 1;
                    let ch = self.b.item_chest[it] as usize;
                    if !chests[..nch].contains(&ch) && nch < 64 { chests[nch] = ch; nch += 1; }
                }
            }
            if !any { return; }
        } else {
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
        }
        let k = self.time.clone();
        let first = self.access(chests[0]);
        if !(self.c == first && self.y == 1) {
            if self.route(first, 1, 20000, true, false) { self.travel(); } else {
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

    #[inline(always)]
    fn stack_of(&self, it: usize) -> u32 { self.b.item_stack[it] }

    // ------------------------------------------------------------------------------------------ after the main pass
    fn time_short(&self) -> bool {
        let standing = self.st.scaffold_placed;
        self.max_ticks.saturating_sub(self.ticks) < 45 * standing.min(20000) + 40000
    }

    /// A's `deferred_pass`: the put-off cells, the nearest first (distance: |dx| + |dz| + 2 |dy|). The cells are
    /// bucketed in tiles of 8 x 8 columns and the nearest is looked for ring by ring around the bot's tile.
    #[inline(never)]
    fn deferred_pass(&mut self) {
        if self.time_short() { return; }
        self.deferred.sort_unstable();
        self.deferred.dedup();
        let mut cells: Vec<u32> = self.deferred.clone();
        cells.retain(|&id| self.w.get(DEFER, id_c(id), id_y(id)));
        let n = cells.len();
        if n == 0 { self.deferred.clear(); return; }
        let (tw, th) = ((self.w.nx + 7) >> 3, (self.w.nz + 7) >> 3);
        let pos: Vec<(i32, i32, i32)> = cells.iter().map(|&id| { let (x, z) = self.xz(id_c(id)); (x, id_y(id) as i32, z) }).collect();
        let tile = |p: &(i32, i32, i32)| (p.2 as usize >> 3) * tw + (p.0 as usize >> 3);
        let mut start = vec![0u32; tw * th + 1];
        for p in &pos { start[tile(p) + 1] += 1; }
        for t in 0..tw * th { start[t + 1] += start[t]; }
        let mut fill = start.clone();
        let mut order = vec![0u32; n];
        for (j, p) in pos.iter().enumerate() { let t = tile(p); order[fill[t] as usize] = j as u32; fill[t] += 1; }
        let mut live: Vec<u32> = (0..tw * th).map(|t| start[t + 1] - start[t]).collect();
        let mut done = vec![false; n];
        for _ in 0..n {
            let (bx, bz) = self.xz(self.c);
            let by = self.y as i32;
            let (tx, tz) = (bx >> 3, bz >> 3);
            let mut k = usize::MAX;
            let mut best = i32::MAX;
            for r in 0..(tw.max(th) as i32) {
                // (a cell in ring r of tiles is at least (r - 1) * 8 + 1 away)
                if k != usize::MAX && (r - 1) * 8 >= best { break; }
                for dz in -r..=r {
                    let z = tz + dz;
                    if z < 0 || z >= th as i32 { continue; }
                    let step = if dz == -r || dz == r { 1 } else { (2 * r).max(1) };
                    let mut dx = -r;
                    while dx <= r {
                        let x = tx + dx;
                        dx += step;
                        if x < 0 || x >= tw as i32 { continue; }
                        let t = z as usize * tw + x as usize;
                        if live[t] == 0 { continue; }
                        for q in start[t]..start[t + 1] {
                            let j = order[q as usize] as usize;
                            if done[j] { continue; }
                            let d = (pos[j].0 - bx).abs() + (pos[j].2 - bz).abs() + 2 * (pos[j].1 - by).abs();
                            if d < best { best = d; k = j; }
                        }
                    }
                }
            }
            if k == usize::MAX { break; }
            done[k] = true;
            live[tile(&pos[k])] -= 1;
            if self.time_short() { break; }
            let (c, y) = (id_c(cells[k]), id_y(cells[k]));
            if !self.w.get(DEFER, c, y) { continue; }
            if !self.late_too && self.w.pal_at(c, y).class & C_LATE != 0 { continue; }
            match self.do_cell(c, y) {
                SKIP_R => { self.w.clear(DEFER, c, y); self.st.skipped += 1; }
                PLACED_R => self.retry_neighbours(c, y),
                _ => {}
            }
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

    /// The last rounds of A's clean-up break through the build to get at a piece and mend the hole afterwards.
    /// Here that is a charge from the cost table: the way there with a detour, one block broken and put back.
    fn forced_break(&mut self, c: usize, _y: usize) -> bool {
        let (bx, bz) = self.xz(self.c);
        let (tx, tz) = self.xz(c);
        let manh = ((bx - tx).abs() + (bz - tz).abs()) as u32;
        let t = self.time.route + self.time.mv[WALK as usize] * manh * 3 + self.time.mv[MINE as usize] + self.time.arrive + self.time.place;
        self.ticks += t as u64;
        self.st.forced += 1;
        true
    }

    /// Takes down what scaffolding stands: columns from the foot (the rest comes down with it), the highest first;
    /// then the loose blocks.
    #[inline(never)]
    fn cleanup(&mut self, mine: bool) {
        let t0 = self.ticks;
        let sc0 = self.st.scaffold_placed;
        let mut feet: Vec<u32> = std::mem::take(&mut self.tower_feet);
        feet.retain(|&id| self.w.get(TOWER, id_c(id), id_y(id)) && !self.w.get(TOWER, id_c(id), id_y(id) - 1));
        feet.sort_unstable();
        feet.dedup();
        let top_of = |w: &World<'_>, id: u32| -> usize { let mut y = id_y(id); while w.get(TOWER, id_c(id), y + 1) { y += 1; } y };
        feet.sort_by_key(|&id| std::cmp::Reverse(top_of(&self.w, id)));
        let mut left: Vec<u32> = Vec::new();
        for &id in &feet {
            let (c, y) = (id_c(id), id_y(id));
            if !self.w.get(TOWER, c, y) { continue; }
            if self.break_at(c, y, true, false, mine) || (mine && self.forced_break(c, y)) {
                let mut yy = y;
                while self.w.get(TOWER, c, yy) { self.w.clear(TOWER, c, yy); self.w.clear(CLICK, c, yy); self.click_changed(c, yy); yy += 1; }
                // (fitted: the time goes with the pieces of the column, which come down one after another)
                self.ticks += self.time.break_tower as u64 * (yy - y) as u64;
            } else { left.push(id); }
        }
        left.extend_from_slice(&self.tower_feet);
        self.tower_feet = left;
        let mut loose: Vec<u32> = std::mem::take(&mut self.loose_list);
        loose.retain(|&id| self.w.get(LOOSE, id_c(id), id_y(id)));
        let mut left: Vec<u32> = Vec::new();
        for &id in &loose {
            let (c, y) = (id_c(id), id_y(id));
            if self.break_at(c, y, false, false, mine) || self.break_at(c, y, false, true, mine) || (mine && self.forced_break(c, y)) {
                self.w.clear(LOOSE, c, y);
                self.w.clear(SOLID, c, y);
                self.w.clear(CLICK, c, y); self.click_changed(c, y);
                self.ticks += self.time.break_loose as u64;
            } else { left.push(id); }
        }
        left.extend_from_slice(&self.loose_list);
        self.loose_list = left;
        self.st.cleanup_ticks += self.ticks - t0;
        self.st.ph[1] += self.st.scaffold_placed - sc0;
    }

    fn scaffold_standing(&self) -> bool { !self.tower_feet.is_empty() || !self.loose_list.is_empty() }

    // ------------------------------------------------------------------------------------------ the run
    pub fn run(&mut self) {
        self.st.skipped = self.w.skipped_unplaceable as u64;
        for bz in 0..self.w.sz { for bx in 0..self.w.sx {
            let c = (self.w.oz + bz) * self.w.nx + self.w.ox + bx;
            self.next[c] = self.scan_column(c, 1);
        } }
        for bz in 0..self.w.sz { for bx in 0..self.w.sx { let c = (self.w.oz + bz) * self.w.nx + self.w.ox + bx; self.upd_elig(c, true); } }
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
        self.st.ph[0] = self.st.scaffold_placed;
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

    // ------------------------------------------------------------------------------------------ replay (fidelity tooling)
    /// Walks an event log of A's simulator (`P` placements with the bot's position, `S+` / `S-` temporary blocks, `X`
    /// skips) through this world and asks fastsim A's questions on A's own states:
    /// Q1, at every block A placed: can fastsim click that cell from where A's bot stood?
    /// Q2, at every cell A gave up as "no way to place": does fastsim find a way (a stance, or a helper spot)?
    /// Counts by kind of block: [plain, oriented (click mask not free)] x [asked, fastsim says yes].
    pub fn replay(&mut self, log: &str) -> [[u64; 4]; 2] {
        let mut r = [[0u64; 4]; 2];
        self.inv.iter_mut().for_each(|v| *v = 1000);
        self.main_done = true;
        let (ox, oz, nx, nz, ny) = (self.w.ox as i32, self.w.oz as i32, self.w.nx as i32, self.w.nz as i32, self.w.ny as i32);
        let (sx, sy, sz) = (self.w.sx, self.w.sy, self.w.sz);
        let cell = move |t: &str| -> Option<(usize, usize, bool)> {
            let mut it = t.trim_start_matches('@').split(',');
            let (x, y, z): (i32, i32, i32) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
            let (gx, gz, gy) = (x + ox, z + oz, y + 1);
            if gx < 1 || gz < 1 || gx >= nx - 1 || gz >= nz - 1 || gy < 1 || gy >= ny - 3 { return None; }
            let inbox = x >= 0 && z >= 0 && (x as usize) < sx && (z as usize) < sz && (y as usize) < sy && y >= 0;
            Some(((gz * nx + gx) as usize, gy as usize, inbox))
        };
        let mut last_p: Option<(usize, usize, bool)> = None;
        for l in log.lines() {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 3 { continue; }
            match f[1] {
                "P" if f.len() >= 5 => {
                    let Some((c, y, inbox)) = cell(f[2]) else { last_p = None; continue };
                    let temp = f[3].starts_with("scaffolding") || f[3].starts_with("moss_block");
                    let target = inbox && self.w.get(TGT, c, y) && !temp;
                    if let Some((bc, by, _)) = cell(f[4]) {
                        self.c = bc; self.y = by;
                        if target && !self.w.get(PLACED, c, y) {
                            let k = (self.w.pal_at(c, y).mask != ANY) as usize;
                            r[0][k * 2] += 1;
                            if self.click_here(c, y, true) != 0 { r[0][k * 2 + 1] += 1; }
                            else if self.trace { let (bx0, bz0) = self.xz(self.c); let (cx0, cz0) = self.xz(c); println!("Q1MISS {} {} d={},{},{} sup={}", f[3].split('[').next().unwrap(), if self.w.pal_at(c, y).tab != NO_TAB { "table" } else { "free" }, cx0 - bx0, y as i32 - self.y as i32, cz0 - bz0, self.dbg_sup(c, y, true)); }
                        }
                    }
                    if target {
                        let p = self.w.pal_at(c, y);
                        self.w.set(PLACED, c, y); self.w.clear(NEED, c, y); self.w.clear(DEFER, c, y);
                        if p.class & C_SOLID != 0 { self.w.set(SOLID, c, y); }
                        if p.class & C_CLICK != 0 { self.w.set(CLICK, c, y); }
                        last_p = None;
                    } else {
                        // a temporary block (the S+ line that follows confirms it)
                        if f[3].starts_with("scaffolding") { self.w.set(TOWER, c, y); } else { self.w.set(LOOSE, c, y); self.w.set(SOLID, c, y); }
                        self.w.set(CLICK, c, y);
                        last_p = Some((c, y, inbox));
                    }
                }
                "S-" => {
                    if let Some((c, y, _)) = cell(f[2]) {
                        if self.w.get(TOWER, c, y) || self.w.get(LOOSE, c, y) {
                            self.w.clear(TOWER, c, y); self.w.clear(LOOSE, c, y); self.w.clear(SOLID, c, y); self.w.clear(CLICK, c, y);
                        }
                    }
                }
                "X" if f.len() >= 4 && f[2] == "no_way_to_place" => {
                    let Some((c, y, inbox)) = cell(f[3]) else { continue };
                    if !inbox || !self.w.get(TGT, c, y) || self.w.get(PLACED, c, y) { continue; }
                    let p = self.w.pal_at(c, y);
                    if p.class & crate::prep::C_UNPLACEABLE != 0 { continue; }
                    // (only cells the judge counts)
                    if self.w.judged[c * self.w.w + (y >> 6)] >> (y & 63) & 1 == 0 { continue; }
                    let k = (p.mask != ANY) as usize;
                    r[1][k * 2] += 1;
                    self.w.set(DEFER, c, y);
                    let st = self.plan_stance(c, y, true);
                    let was_virt = self.virt.is_some();
                    let mut yes = st.is_some();
                    self.virt = None;
                    // (how fastsim would do it, against A's own reason for giving up)
                    let how = match st { Some(Stance { kind: StanceKind::Natural, .. }) => "stand", Some(Stance { kind: StanceKind::SelfPlace, .. }) => "underfoot", Some(Stance { kind: StanceKind::Lean, .. }) => "lean", Some(Stance { kind: StanceKind::Column, .. }) => "column", None => "helper" };
                    let why = f.iter().find(|t| t.starts_with("why=")).map(|t| &t[4..]).unwrap_or("?");
                    if p.class & C_GRAVITY != 0 && !self.w.get(SOLID, c, y - 1) { yes = false; }
                    else if !yes && !(self.pol.strict_helpers && p.class2 & C2_FLOATS == 0) {
                        let nxs = self.w.nx as i32;
                        let anchored = |me: &Self, hc: usize, hy: usize, not: &[(usize, usize)]| (0..6).any(|e| { let ac = (hc as i32 + DC6X[e] + DC6Z[e] * nxs) as usize; let ay = (hy as i32 + DC6Y[e]) as usize; !not.contains(&(ac, ay)) && me.w.clickable(ac, ay) });
                        for d in 0..6 {
                            if p.mask >> (FACE_OF[d] * 8) & 0xff == 0 { continue; }
                            let hc = (c as i32 + DC6X[d] + DC6Z[d] * nxs) as usize;
                            let hy = (y as i32 + DC6Y[d]) as usize;
                            if !(hy >= 1 && hy + 3 < self.w.ny && self.loose_ok(hc, hy)) { continue; }
                            if !self.pol.strict_helpers { yes = true; break; }
                            if anchored(self, hc, hy, &[(c, y)]) {
                                // (the helper put there in thought: does the cell then have a click from somewhere?)
                                self.w.set(LOOSE, hc, hy); self.w.set(SOLID, hc, hy); self.w.set(CLICK, hc, hy);
                                let ok = self.plan_stance(c, y, true).is_some();
                                self.virt = None;
                                self.w.clear(LOOSE, hc, hy); self.w.clear(SOLID, hc, hy); self.w.clear(CLICK, hc, hy);
                                if ok { yes = true; break; }
                                continue;
                            }
                            for e in 0..6 {
                                let bc = (hc as i32 + DC6X[e] + DC6Z[e] * nxs) as usize;
                                let by = (hy as i32 + DC6Y[e]) as usize;
                                if (bc, by) != (c, y) && by >= 1 && by + 3 < self.w.ny && self.loose_ok(bc, by) && anchored(self, bc, by, &[(c, y), (hc, hy)]) { yes = true; break; }
                            }
                            if yes { break; }
                        }
                    }
                    if yes { r[1][k * 2 + 1] += 1; }
                    if yes && self.trace && st.is_some() {
                        let s0 = st.unwrap();
                        let (sx0, sz0) = self.xz(s0.c);
                        println!("Q2DBG {} {} {} cell {} stance {},{},{} sup={} mask={:x}", self.b.hash, why, how, f[3], sx0 - ox, s0.y as i32 - 1, sz0 - oz, self.dbg_sup(c, y, true), p.mask);
                    }
                    *self.q2.entry(format!("{why} -> {}{}{}", if yes { how } else { "no way" }, if yes && was_virt { " (cost-table route)" } else { "" }, if self.attached(c, y) { "" } else { " [nothing beside it]" })).or_insert(0) += 1;
                }
                _ => {}
            }
        }
        let _ = last_p;
        r
    }
}
