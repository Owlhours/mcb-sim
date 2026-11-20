//! Route finding (the Java version's `Nav`): weighted A* over the cells the player can stand in.
use super::state::Bot;
use crate::tables::*;
use crate::fasthash::FiltSet;

pub const WALK: u8 = 0;
pub const JUMP: u8 = 1;
pub const DROP: u8 = 2;
pub const PILLAR: u8 = 3;
pub const BRIDGE: u8 = 4;
pub const DIG: u8 = 5;
pub const CLEAR: u8 = 6;
pub const MINE: u8 = 7;
pub const CLIMB: u8 = 8;
pub const DESCEND: u8 = 9;
pub const TOWER: u8 = 10;
pub const GATE: u8 = 11;
pub const LADDER: u8 = 12;
/// (candidate `walkway`) from the top of a scaffolding column: a piece of scaffolding put out sideways at the level of
/// the column's top piece, and a step onto it
pub const WING: u8 = 13;

pub const C_MINE: i32 = 400;
pub const C_CLEAR: i32 = 12;
pub const C_WALK: i32 = 5;
pub const C_JUMP: i32 = 9;
pub const C_DROP: i32 = 6;
pub const C_PILLAR: i32 = 12;
pub const C_BRIDGE: i32 = 22;
pub const C_DIG: i32 = 2;
pub const C_CLIMB: i32 = 9;
pub const C_DESCEND: i32 = 7;
pub const C_TOWER: i32 = 13;
pub const C_GATE: i32 = 14;
pub const C_ABANDON: i32 = 45;
pub const C_PIT: i32 = 150;

/// `Nav::front` when it holds nothing (larger than any entry).
pub const NO_FRONT: i64 = i64::MAX;

#[inline]
pub fn edge(from: i32, to: i32) -> i64 { ((from as i64) << 32) | (to as i64 & 0xffffffff) }

#[derive(Clone, Copy, Default)]
pub struct Node {
    pub stamp: i32,
    pub cost: i32,
    pub parent: i32,
    pub how: u8,
}

/// A search of the stance planner that is kept between its questions (`Bot::nav_search_kept`, revision `plankeep`).
/// While it runs, and until another fast search starts, its nodes are in the grid's padded cells like any search's;
/// then they are put by here (`saved`) and written back, under a new stamp, when it is continued.
#[derive(Default)]
pub struct Kept {
    pub live: bool,
    /// the plan, the start cell and the rules; and the world (grid hash, scaffold set, blocked steps) it was started in
    pub key: u64,
    pub world: u64,
    /// its heap while it does not run (`Nav::front` is put back into the heap first)
    pub heap: Vec<i64>,
    /// the cells expanded so far (`Grid::pk`), one entry per expansion
    pub closed: Vec<i32>,
    /// nodes put by: (padded index, cost, parent, how)
    pub saved: Vec<(u32, i32, i32, u8)>,
    pub expanded: i32,
    /// the goal the heap's keys are for
    pub t: [i32; 3],
    /// how many of `closed` rate how well for the present goal: counts by (cost + estimate) / 8
    pub hist: Vec<u32>,
}
const KEPT_BUCKETS: usize = 8192;
/// A continued question may expand this share of its budget beyond what the kept search has expanded already.
const KEPT_SHARE: i32 = 8;
pub static KEPT_WORLD_CHANGED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// MCSIM_PLANCHECK (self-check of the kept search): 1 - the question that starts a kept search is also put to the plain
/// search, and any difference a caller can see is a panic. 2 - also, every continued question is answered by `route`
/// alone (the rows must then be those of the revision before `plankeep`).
pub fn plan_check() -> i32 {
    static V: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MCSIM_PLANCHECK").ok().and_then(|v| v.parse().ok()).unwrap_or(0))
}
/// MCSIM_PLANSHADOW (measurement): every question of the kept search is also put to the plain search first, and the
/// two answers are counted against each other (printed with MCSIM_CALLSTAT).
pub fn plan_shadow() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCSIM_PLANSHADOW").is_some())
}
thread_local! { pub static SHADOW: std::cell::RefCell<std::collections::BTreeMap<String, (u64, u64, u64)>> = const { std::cell::RefCell::new(std::collections::BTreeMap::new()) }; }

pub struct Nav {
    /// The kept searches (one for each of the planner's two rule sets), see `Kept`.
    pub kept: [Kept; 2],
    /// 0, or 1 + the kept search that runs (set by `nav_search_kept` around the search)
    pub k: usize,
    pub k_resume: bool,
    /// whether the last kept search was a continued one
    pub k_was_resumed: bool,
    /// 0, or 1 + the kept search whose nodes are in the padded cells now
    pub kept_pc: usize,
    /// the budget the last kept question had; the cells it expanded itself (`expanded` counts from the search's start)
    pub k_budget: i32,
    pub new_expanded: i32,
    pub mine: bool,
    pub effort: i32,
    pub blocked_edges: FiltSet<i64>,
    pub nodes: Vec<Node>,
    pub epoch: i32,
    /// the fast search keeps its nodes in the grid's padded cells, stamped with this (16 bit) epoch
    pub fast: bool,
    pub epoch16: u16,
    /// For the caller that only asks "found, or more than 96 cells expanded?" (`way_out`): stop as soon as this many
    /// distinct cells have been reached. Every reached cell is expanded at least once before a search runs dry, so
    /// from then on the search can only end by finding the goal or by passing 96 expansions - the same answer.
    pub touch_limit: i32,
    pub touched: i32,
    pub heap: Vec<i64>,
    /// Fast search: an entry held outside the heap, not larger than any entry in it (`NO_FRONT`: none). A cell just
    /// reached is often the next one to expand; it then never enters the heap. Empty between searches.
    pub front: i64,
    pub scaffold_penalty: i32,
    pub tower_penalty: i32,
    pub perch_penalty: i32,
    pub towers_on: bool,
    pub extra_effort: i32,
    pub expanded: i32,
    /// After a search that found nothing: true if every place that can be reached was looked at (so there is no route).
    pub exhausted: bool,
    /// Java `stopAt`: the cells of `Bot::exit_nodes` end a search as well as the goal does (places from which a way to
    /// the goal is already known).
    pub stop_at: bool,
    /// The cell (grid index) at which the last successful search ended (the goal, or one of the stop cells).
    pub reached: i32,
    pub total_expanded: i64,
    pub searches: i64,
    /// wall time spent searching (nanoseconds), for the speed breakdown
    pub search_ns: u64,
    pub virtual_stand: bool,
    /// dry.rs: the loop of the search is run over a list of (cell, way of standing) only to see where each leads
    pub probe: bool,
    /// set by a caller that reads nothing of the search but found / not found (dry.rs may answer for it)
    pub only_found: bool,
    /// (candidate `nostrand2`) no dry pocket is looked up, stored or mended: a question that must leave no trace
    pub dry_off: bool,
    /// The search that looks at every place once (`nav_ask`): places are taken in the order of their distance to the
    /// goal alone, and a place once reached is never taken up again. For questions of "is there a way", not for routes.
    /// Set by the search itself when it has expanded `once_after` cells (> 0).
    pub once: bool,
    pub once_after: i32,
    /// Of the last search: whether it went on that way, and the budget it had (after the cut by distance).
    pub went_once: bool,
    pub last_budget: i32,
    /// When not empty: the search ends at any of these cells (grid indices; `goal_xyz` the same cells by coordinates)
    /// instead of the goal cell, and heads for the nearest of them (`tunnel_hopeless`: "next to the cell").
    pub goal_any: Vec<i32>,
    pub goal_xyz: Vec<(i32, i32, i32)>,
    pub start_h: i32,
    // result
    pub path: Vec<i32>,
    pub mv: Vec<u8>,
    pub length: usize,
    pub goal_cost: i32,
    pub scaffold_moves: i32,
    pub tower_moves: i32,
    tx: i32, ty: i32, tz: i32,
}

/// (candidate `nostrand2`) What a search leaves in `Nav` besides its nodes - the route, the counts, the switches the
/// callers set around it - kept by a question that must leave no trace (`Bot::quietly`).
pub struct NavQuiet {
    path: Vec<i32>, mv: Vec<u8>, length: usize, goal_cost: i32, scaffold_moves: i32, tower_moves: i32, t: (i32, i32, i32),
    start_h: i32, mine: bool, effort: i32, stop_at: bool, only_found: bool, dry_off: bool, touch_limit: i32, once_after: i32,
    expanded: i32, new_expanded: i32, exhausted: bool, reached: i32, searches: i64, total_expanded: i64, once: bool, went_once: bool,
    last_budget: i32, touched: i32, fast: bool, virtual_stand: bool, towers_on: bool, extra_effort: i32,
}

impl Nav {
    pub fn quiet_keep(&self) -> NavQuiet {
        NavQuiet {
            path: self.path[..self.length * 3].to_vec(), mv: self.mv[..self.length].to_vec(), length: self.length, goal_cost: self.goal_cost,
            scaffold_moves: self.scaffold_moves, tower_moves: self.tower_moves, t: (self.tx, self.ty, self.tz),
            start_h: self.start_h, mine: self.mine, effort: self.effort, stop_at: self.stop_at, only_found: self.only_found, dry_off: self.dry_off,
            touch_limit: self.touch_limit, once_after: self.once_after, expanded: self.expanded, new_expanded: self.new_expanded,
            exhausted: self.exhausted, reached: self.reached, searches: self.searches, total_expanded: self.total_expanded, once: self.once,
            went_once: self.went_once, last_budget: self.last_budget, touched: self.touched, fast: self.fast, virtual_stand: self.virtual_stand,
            towers_on: self.towers_on, extra_effort: self.extra_effort,
        }
    }
    pub fn quiet_put_back(&mut self, q: NavQuiet) {
        if self.path.len() < q.path.len() { self.path.resize(q.path.len(), 0); }
        if self.mv.len() < q.mv.len() { self.mv.resize(q.mv.len(), 0); }
        self.path[..q.path.len()].copy_from_slice(&q.path);
        self.mv[..q.mv.len()].copy_from_slice(&q.mv);
        self.length = q.length; self.goal_cost = q.goal_cost; self.scaffold_moves = q.scaffold_moves; self.tower_moves = q.tower_moves;
        (self.tx, self.ty, self.tz) = q.t;
        self.start_h = q.start_h; self.mine = q.mine; self.effort = q.effort; self.stop_at = q.stop_at; self.only_found = q.only_found;
        self.dry_off = q.dry_off; self.touch_limit = q.touch_limit; self.once_after = q.once_after; self.expanded = q.expanded;
        self.new_expanded = q.new_expanded; self.exhausted = q.exhausted; self.reached = q.reached; self.searches = q.searches;
        self.total_expanded = q.total_expanded; self.once = q.once; self.went_once = q.went_once; self.last_budget = q.last_budget;
        self.touched = q.touched; self.fast = q.fast; self.virtual_stand = q.virtual_stand; self.towers_on = q.towers_on; self.extra_effort = q.extra_effort;
    }

    pub fn new(n: usize) -> Nav {
        Nav {
            kept: Default::default(), k: 0, k_resume: false, k_was_resumed: false, kept_pc: 0, k_budget: 0, new_expanded: 0,
            mine: false, effort: 1, blocked_edges: FiltSet::default(), nodes: vec![Node::default(); n], epoch: 0, fast: false, epoch16: 0, touch_limit: 0, touched: 0, heap: Vec::with_capacity(1024), front: NO_FRONT,
            scaffold_penalty: 40, tower_penalty: 3, perch_penalty: 0, towers_on: true, extra_effort: 0, expanded: 0, exhausted: false, stop_at: false, reached: -1, total_expanded: 0, searches: 0, search_ns: 0,
            virtual_stand: false, probe: false, only_found: false, dry_off: false, once: false, once_after: 0, went_once: false, last_budget: 0, goal_any: Vec::new(), goal_xyz: Vec::new(), start_h: -1, path: Vec::new(), mv: Vec::new(), length: 0, goal_cost: 0, scaffold_moves: 0, tower_moves: 0, tx: 0, ty: 0, tz: 0,
        }
    }

    #[inline]
    pub(super) fn push(&mut self, key: i64, node: i32) {
        let v = (key << 32) | (node as i64 & 0xffffffff);
        let mut i = self.heap.len();
        self.heap.push(v);
        while i > 0 {
            let p = (i - 1) >> 1;
            if self.heap[p] <= v { break; }
            self.heap[i] = self.heap[p];
            i = p;
        }
        self.heap[i] = v;
    }

    #[inline]
    fn pop(&mut self) -> i64 {
        let top = self.heap[0];
        let v = self.heap.pop().unwrap();
        let n = self.heap.len();
        if n == 0 { return top; }
        let mut i = 0;
        loop {
            let l = 2 * i + 1;
            if l >= n { break; }
            let r = l + 1;
            let m = if r < n && self.heap[r] < self.heap[l] { r } else { l };
            if self.heap[m] >= v { break; }
            self.heap[i] = self.heap[m];
            i = m;
        }
        self.heap[i] = v;
        top
    }

    /// `push` / `pop` of the fast search. The entries of a search are distinct integers (key, then node), so whatever
    /// shape the heap has, `pop` gives them in increasing order: the two pairs can be mixed freely.
    /// `pop_f` lets the hole sink to the bottom along the smaller children (no test of the moved entry on the way,
    /// the child is chosen without a branch), then lifts the last entry from there - it mostly belongs near the bottom.
    #[inline(always)]
    fn push_v(&mut self, v: i64) {
        let mut i = self.heap.len();
        self.heap.push(v);
        let h = self.heap.as_mut_ptr();
        // (i < len throughout; parents are below i)
        unsafe {
            while i > 0 {
                let p = (i - 1) >> 1;
                let pv = *h.add(p);
                if pv <= v { break; }
                *h.add(i) = pv;
                i = p;
            }
            *h.add(i) = v;
        }
    }

    #[inline(always)]
    fn pop_f(&mut self) -> i64 {
        let n = self.heap.len() - 1;
        let h = self.heap.as_mut_ptr();
        // (the heap is not empty: the caller's loop tests it; every index below is < n + 1 = the old length)
        unsafe {
            let top = *h;
            let v = *h.add(n);
            self.heap.set_len(n);
            if n == 0 { return top; }
            let mut i = 0;
            loop {
                let l = 2 * i + 1;
                if l + 1 >= n {
                    if l < n { *h.add(i) = *h.add(l); i = l; }
                    break;
                }
                let (a, b) = (*h.add(l), *h.add(l + 1));
                let lt = b < a;
                *h.add(i) = if lt { b } else { a };
                i = l + lt as usize;
            }
            while i > 0 {
                let p = (i - 1) >> 1;
                let pv = *h.add(p);
                if pv <= v { break; }
                *h.add(i) = pv;
                i = p;
            }
            *h.add(i) = v;
            top
        }
    }

    #[inline]
    fn heuristic(&self, x: i32, y: i32, z: i32) -> i32 {
        if !self.goal_any.is_empty() { return self.goal_xyz.iter().map(|&(gx, gy, gz)| self.heuristic_to(x, y, z, gx, gy, gz)).min().unwrap_or(0); }
        self.heuristic_to(x, y, z, self.tx, self.ty, self.tz)
    }
    #[inline]
    fn heuristic_to(&self, x: i32, y: i32, z: i32, tx: i32, ty: i32, tz: i32) -> i32 {
        let dy = ty - y;
        (if self.mine { 5 } else { 1 }) * (2 * ((tx - x).abs() + (tz - z).abs()) * C_WALK
            + if dy > 0 { dy * if self.towers_on { C_TOWER + self.tower_penalty } else { C_PILLAR + self.scaffold_penalty } } else { -dy * 2 })
    }
}

// MCSIM_CALLSTAT=1 (measurement only): every search is counted under the path of its tagged callers.
thread_local! { pub static TAGS: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) }; }
thread_local! { pub static CALLSTAT: std::cell::RefCell<std::collections::BTreeMap<(String, &'static str), (u64, u64, u64)>> = const { std::cell::RefCell::new(std::collections::BTreeMap::new()) }; }
pub fn callstat() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCSIM_CALLSTAT").is_some())
}
pub struct TagGuard(bool);
#[inline]
pub fn tag(t: &'static str) -> TagGuard { let on = callstat(); if on { TAGS.with(|s| s.borrow_mut().push(t)); } TagGuard(on) }
impl Drop for TagGuard { #[inline] fn drop(&mut self) { if self.0 { TAGS.with(|s| { s.borrow_mut().pop(); }); } } }
// ---- what the ticks go to (MCSIM_CAUSES): a stack of activities, a tick is counted for (outermost, innermost)
thread_local! { static ACTS: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) }; }
thread_local! { static ACT_TICKS: std::cell::RefCell<std::collections::BTreeMap<(&'static str, &'static str), u64>> = const { std::cell::RefCell::new(std::collections::BTreeMap::new()) }; }
pub fn acts_on() -> bool { static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new(); *V.get_or_init(|| std::env::var_os("MCSIM_CAUSES").is_some()) }
pub struct ActGuard(bool);
#[inline]
pub fn act(t: &'static str) -> ActGuard { let on = acts_on(); if on { ACTS.with(|s| s.borrow_mut().push(t)); } ActGuard(on) }
impl Drop for ActGuard { #[inline] fn drop(&mut self) { if self.0 { ACTS.with(|s| { s.borrow_mut().pop(); }); } } }
#[inline]
pub fn act_tick() { if acts_on() { ACTS.with(|s| { let s = s.borrow(); let k = (s.first().copied().unwrap_or("main pass"), s.last().copied().unwrap_or("placing")); ACT_TICKS.with(|m| *m.borrow_mut().entry(k).or_insert(0) += 1); }); } }
pub fn act_now() -> String { ACTS.with(|s| s.borrow().join(" > ")) }
pub fn act_lines() -> Vec<String> { ACT_TICKS.with(|m| m.borrow().iter().map(|((a, b), n)| format!("T {} | {} | {}", a, b, n)).collect()) }
pub fn dump_callstat() {
    SHADOW.with(|m| { for (k, (n, e, e2)) in m.borrow().iter() { eprintln!("shadow {k:50} n={n:8} oldcells={e:11} newcells={e2:11}"); } });
    let n = KEPT_WORLD_CHANGED.load(std::sync::atomic::Ordering::Relaxed); if n > 0 { eprintln!("callstat KEPT-WORLD-CHANGED {n}"); }
    CALLSTAT.with(|m| { for ((a, b), (n, e, ns)) in m.borrow().iter() { eprintln!("callstat {a:50} {b:9} searches={n:9} expanded={e:11} ms={}", ns / 1000000); } }); }
thread_local! { pub static NAVSTAT: std::cell::RefCell<[[(u64, u64); 24]; 3]> = const { std::cell::RefCell::new([[(0, 0); 24]; 3]) }; }

thread_local! { pub static MEMOSTAT: std::cell::RefCell<(crate::fasthash::FastSet<u64>, u64, u64, u64, u64)> = std::cell::RefCell::new((Default::default(), 0, 0, 0, 0)); }

thread_local! { pub static CERTSTAT: std::cell::RefCell<(u64, u64, u64, u64, u64)> = const { std::cell::RefCell::new((0, 0, 0, 0, 0)) }; }

/// Diagnostic switches read once from the environment: 0 MCSIM_NAVSTAT, 1 MCSIM_MEMOSTAT, 2 MCSIM_CERTSTAT.
#[inline]
fn envflag(k: usize) -> bool {
    static V: std::sync::OnceLock<[bool; 3]> = std::sync::OnceLock::new();
    V.get_or_init(|| [std::env::var_os("MCSIM_NAVSTAT").is_some(), std::env::var_os("MCSIM_MEMOSTAT").is_some(), std::env::var_os("MCSIM_CERTSTAT").is_some()])[k]
}

thread_local! { pub static REGSTAT: std::cell::RefCell<(u64, Vec<u64>, u64, u64, u64, u64, u64)> = const { std::cell::RefCell::new((0, Vec::new(), 0, 0, 0, 0, 0)) }; }
fn regstat() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCSIM_REGSTAT").is_some())
}

/// MCSIM_PFHEURCHECK: `way_out` asks its last question the old way as well and reports a different answer.
pub fn pfheur_check() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCSIM_PFHEURCHECK").is_some())
}
/// MCSIM_PFTUNCHECK: `tunnel_to` runs its loop also after "no way to any neighbour" and reports a way it finds.
pub fn pftun_check() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCSIM_PFTUNCHECK").is_some())
}
/// `nav_ask`: the number of cells the usual search gets before it goes on looking at every place once.
pub const ASK_FIRST: i32 = 1000;

fn lossy() -> i32 {
    static V: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MCSIM_LOSSY").ok().and_then(|v| v.parse().ok()).unwrap_or(1))
}

fn navcheck() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCSIM_NAVCHECK").is_some())
}

/// `PCell::pad` while a search runs: bit 0-3 "the search tried to enter the cell this way" (0 walking or the like,
/// 1 by digging or clearing, 2 on a block it would put, 3 on a column it would put up), bit 4-7 "the cell was expanded
/// as entered that way". (Read by dry.rs after a search that ran dry.)
#[inline(always)]
fn how_bit(h: u8) -> u8 { if h == DIG || h == CLEAR || h == MINE { 2 } else if h == PILLAR || h == BRIDGE { 4 } else if h == TOWER || h == WING { 8 } else { 1 } }
/// `how_bit` as a table (WALK ... LADDER)
const HOW_BIT: [u8; 16] = [1, 1, 1, 4, 4, 2, 2, 2, 1, 1, 8, 1, 1, 8, 1, 1];

const NDX: [i32; 4] = [1, -1, 0, 0];
const NDZ: [i32; 4] = [0, 0, 1, -1];

/// A block one can put another block on top of and stand on: full, level top face.
/// (candidate "pieces") the cobweb's state
fn web_state() -> u16 {
    static V: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *V.get_or_init(|| t().default_of("cobweb"))
}

pub fn flat_top(state: u16) -> bool {
    let tb = t();
    let s = state as usize;
    if tb.full_cube[s] { return true; }
    if tb.col_top[s] != 16 || tb.any_top[s] != 16 || tb.interactive[s] { return false; }
    let b = &tb.out_boxes_f32[s];
    let mut k = 0;
    while k < b.len() {
        if b[k + 4] == 1.0 && b[k] == 0.0 && b[k + 2] == 0.0 && b[k + 3] == 1.0 && b[k + 5] == 1.0 { return true; }
        k += 6;
    }
    false
}

/// What a column of loose blocks can be started on: a flat top - or (candidate `nostand2`) a hopper: the first block
/// goes onto it with a sneaking click, from the rim (`Bot::pillar`).
pub fn pillar_floor(state: u16) -> bool {
    flat_top(state) || (super::grid::bowl_floor() && t().kind(state) == crate::kinds::Kind::HopperBlock)
}

impl Bot {
    /// Flood from a cell over everything that is not a full block, with the most generous reading of the moves
    /// (side steps one up or down, any vertical run): every cell from which the goal could conceivably be reached without
    /// breaking blocks of the build. Returns the number of cells found (stops at `cap`) and whether the start is among them.
    pub fn backward_region(&mut self, gx: i32, gy: i32, gz: i32, sx: i32, sy: i32, sz: i32, cap: usize) -> (usize, bool) {
        use super::grid::*;
        let mut seen: crate::fasthash::FastSet<i32> = Default::default();
        let mut stack: Vec<(i32, i32, i32)> = vec![(gx, gy, gz)];
        seen.insert(self.g.idx(gx, gy, gz) as i32);
        let start = self.g.idx(sx, sy, sz) as i32;
        let mut hit = false;
        while let Some((x, y, z)) = stack.pop() {
            if seen.len() >= cap { break; }
            for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0), (0, -1, 0), (1, 1, 0), (-1, 1, 0), (0, 1, 1), (0, 1, -1), (1, -1, 0), (-1, -1, 0), (0, -1, 1), (0, -1, -1)] {
                let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                if !self.g.inside(nx, ny, nz) { continue; }
                let c = self.g.nv(self.g.pidx(nx, ny, nz));
                if c & N_FULL != 0 && c & N_SCAF == 0 { continue; }
                let i = self.g.idx(nx, ny, nz) as i32;
                if seen.insert(i) { if i == start { hit = true; } stack.push((nx, ny, nz)); }
            }
        }
        (seen.len(), hit || start == self.g.idx(gx, gy, gz) as i32)
    }

    #[inline]
    fn relax(&mut self, from: i32, x: i32, y: i32, z: i32, c: i32, h: u8) {
        if !self.g.inside(x, y, z) { return; }
        if self.nav.probe { self.dry_probe(x, y, z, how_bit(h)); return; }
        if self.nav.fast {
            // (in the fast search nodes are named by `Grid::pk`, not by grid index)
            let pi = self.g.pidx(x, y, z);
            let e = self.nav.epoch16;
            let cell = &self.g.pc[pi];
            if cell.stamp == e && (cell.cost <= c || self.nav.once) { self.g.pc[pi].pad |= how_bit(h); return; }
            self.relax_new(from, pi, x, y, z, c, h);
            return;
        }
        let i = self.g.idx(x, y, z);
        let nd = &self.nav.nodes[i];
        if nd.stamp == self.nav.epoch && (nd.cost <= c || self.nav.once) { return; }
        if !self.nav.blocked_edges.is_empty() && self.nav.blocked_edges.contains_plain(&edge(from, i as i32)) { return; }
        self.nav.nodes[i] = Node { stamp: self.nav.epoch, cost: c, parent: from, how: h };
        let key = (if self.nav.once { 0 } else { c } + self.nav.heuristic(x, y, z)) as i64;
        self.nav.push(key, i as i32);
    }

    /// The fast search's `relax` once the caller has seen that the cell (padded index `pi`, which is valid for cells
    /// up to one outside the grid) is not held at this cost or less: the rest of the tests, then the node and its entry.
    #[inline(never)]
    fn relax_new(&mut self, from: i32, pi: usize, x: i32, y: i32, z: i32, c: i32, h: u8) {
        debug_assert!(pi < self.g.pc.len() && (self.g.nv(pi) & super::grid::N_OUT != 0) == !self.g.inside(x, y, z));
        if self.g.nv(pi) & super::grid::N_OUT != 0 { return; }
        debug_assert!(pi == self.g.pidx(x, y, z));
        if !self.nav.blocked_edges.is_empty() {
            let to = self.g.idx(x, y, z) as i32;
            if self.nav.blocked_edges.maybe(crate::fasthash::FiltKey::slot(to)) {
                let (fx, fy, fz) = self.g.unpk(from);
                if self.nav.blocked_edges.contains(&edge(self.g.idx(fx, fy, fz) as i32, to)) { return; }
            }
        }
        let e = self.nav.epoch16;
        let cell = &mut self.g.pc[pi];
        let fresh = cell.stamp != e;
        if fresh { self.nav.touched += 1; cell.pad = 0; }
        cell.stamp = e; cell.cost = c; cell.parent = from; cell.how = h; cell.pad |= HOW_BIT[h as usize & 15];
        let key = (if self.nav.once { 0 } else { c } + self.nav.heuristic(x, y, z)) as i64;
        let k = self.g.pk(x, y, z);
        if fresh && self.g.dry.rec { self.g.dry.touched.push(k); }
        // the entry: held in front if it is the smallest there is, else into the heap (see `Nav::front`)
        let v = (key << 32) | (k as i64 & 0xffffffff);
        let f = self.nav.front;
        if f != NO_FRONT {
            if v < f { self.nav.front = v; self.nav.push_v(f); } else { self.nav.push_v(v); }
        } else if self.nav.heap.first().is_none_or(|&m| v < m) { self.nav.front = v; } else { self.nav.push_v(v); }
    }

    /// `relax` as the search's loop calls it. Fast: the test that throws most calls away ("the cell is held at this
    /// cost or less") is made here, on the padded index the loop has at hand - before the inside test, which a cell
    /// outside the grid passes on to `relax_new` all the same, as it is never stamped. `PCell::pad` is only kept when
    /// dry.rs will read it (`rec`).
    #[inline(always)]
    fn rx<const F: bool>(&mut self, fc: (u16, bool, bool), from: i32, pi: usize, x: i32, y: i32, z: i32, c: i32, h: u8) {
        if F && !fc.2 {
            debug_assert!(pi < self.g.pc.len());
            let cell = unsafe { self.g.pc.get_unchecked_mut(pi) };
            // (`nav.once`: a cell reached is not taken up again, at whatever cost - read only when the cost is lower)
            if cell.stamp == fc.0 && (cell.cost <= c || self.nav.once) { if fc.1 { cell.pad |= HOW_BIT[h as usize & 15]; } return; }
            self.relax_new(from, pi, x, y, z, c, h);
        } else {
            self.relax(from, x, y, z, c, h);
        }
    }

    #[inline]
    fn in_scaffold(&self, x: i32, y: i32, z: i32) -> bool { self.g.inside(x, y, z) && self.scaffold.contains(self.g.idx(x, y, z) as i32) }

    /// Cheapest route from the start cell to the goal cell. `scaffold_rule`: whether loose blocks may be used
    /// (pillar/bridge moves, where `scaffold_ok` allows). Gives up after max_expand cells.
    pub fn nav_search(&mut self, sx: i32, sy: i32, sz: i32, gx: i32, gy: i32, gz: i32, max_expand: i32, scaffold_rule: bool) -> bool {
        let t0 = std::time::Instant::now();
        // dry pockets (MCSIM_DRYMEMO, dry.rs): the plain rule set only - there the search reads the picture and nothing else
        let dry = self.nav.only_found && !self.nav.dry_off && super::dry::on() && self.g.pk_ok && !scaffold_rule && !self.nav.mine && (self.towers_off || !self.have_tower || self.no_scaffold_nav)
            && self.nav.touch_limit == 0 && lossy() <= 1 && self.g.inside(sx, sy, sz) && self.g.inside(gx, gy, gz);
        let mut said: Option<i32> = None;
        if dry {
            self.g.dry.st[11] += 1;
            said = self.dry_lookup(sx, sy, sz, gx, gy, gz);
            if let (Some(n), false) = (said, super::dry::check()) {
                self.nav.epoch += 1; self.nav.heap.clear(); self.nav.expanded = n; self.nav.length = 0; self.nav.searches += 1; self.nav.exhausted = true; self.nav.reached = -1;
                self.nav.tx = gx; self.nav.ty = gy; self.nav.tz = gz; self.nav.scaffold_moves = 0; self.nav.tower_moves = 0;   // (total_expanded: nothing was expanded)
                self.g.dry.st[0] += 1;
                let dt = t0.elapsed().as_nanos() as u64;
                self.nav.search_ns += dt;
                if callstat() {
                    let path = TAGS.with(|s| { let v = s.borrow(); if v.is_empty() { "-".to_string() } else { v.join(">") } });
                    CALLSTAT.with(|m| { let mut m = m.borrow_mut(); let e = m.entry((path, "pocket")).or_insert((0, 0, 0)); e.0 += 1; e.2 += dt; });
                }
                return false;
            }
            self.g.dry.rec = true; self.g.dry.touched.clear();
        }
        let r = if self.g.pk_ok { self.nav_search_g::<true>(sx, sy, sz, gx, gy, gz, max_expand, scaffold_rule) } else { self.nav_search_g::<false>(sx, sy, sz, gx, gy, gz, max_expand, scaffold_rule) };
        if envflag(2) && !r && !self.nav.heap.is_empty() && !self.nav.mine && self.nav.expanded > 200 {
            // would a backward flood from the goal have proved it unreachable? (measurement only)
            let cap = 3000usize;
            let (n, hit) = self.backward_region(gx, gy, gz, sx, sy, sz, cap);
            CERTSTAT.with(|c| { let mut c = c.borrow_mut(); c.0 += 1; c.1 += self.nav.expanded as u64; if n < cap && !hit { c.2 += 1; c.3 += self.nav.expanded as u64; c.4 += n as u64; } });
        }
        if navcheck() {
            // run the reference search on the same inputs and compare everything a caller can see
            let (e, len, gc, sm, tm, ex, rc) = (self.nav.expanded, self.nav.length, self.nav.goal_cost, self.nav.scaffold_moves, self.nav.tower_moves, self.nav.exhausted, self.nav.reached);
            let (path, mv) = (self.nav.path[..len * 3].to_vec(), self.nav.mv[..len].to_vec());
            let (se, te) = (self.nav.searches, self.nav.total_expanded);
            let r2 = self.nav_search_g::<false>(sx, sy, sz, gx, gy, gz, max_expand, scaffold_rule);
            let same = if self.nav.touch_limit > 0 { (r || e > 96) == (r2 || self.nav.expanded > 96) } else { r == r2 && e == self.nav.expanded && len == self.nav.length && ex == self.nav.exhausted && rc == self.nav.reached && (!r || (gc == self.nav.goal_cost && sm == self.nav.scaffold_moves && tm == self.nav.tower_moves && path[..] == self.nav.path[..len * 3] && mv[..] == self.nav.mv[..len])) };
            if !same { panic!("route search mismatch: ({sx},{sy},{sz}) -> ({gx},{gy},{gz}) budget {max_expand}: {r}/{e}/{len}/{gc} vs {r2}/{}/{}/{}", self.nav.expanded, self.nav.length, self.nav.goal_cost); }
            self.nav.searches = se; self.nav.total_expanded = te;
        }
        if regstat() && !r && self.nav.expanded > 96 {
            // measurement: would the cells reached by the last exhausted search (same inputs) have settled this one?
            use std::hash::{Hash, Hasher};
            let mut h = crate::fasthash::Fx::default();
            (self.g.version, self.scaffold.len(), self.scaffold_placed, self.nav.blocked_edges.len(), scaffold_rule, self.nav.mine, self.towers_off, self.nav.start_h).hash(&mut h);
            (self.no_scaffold_nav, self.have_tower, self.have_scaffold, self.layer_now, self.nav.towers_on, self.desperate, self.nav.scaffold_penalty, self.nav.tower_penalty).hash(&mut h);
            let key = h.finish();
            let (s, g) = (self.g.idx(sx, sy, sz), self.g.idx(gx, gy, gz));
            let e = self.nav.expanded as u64;
            let exhausted = self.nav.heap.is_empty();
            let e16 = self.nav.epoch16;
            REGSTAT.with(|m| {
                let mut m = m.borrow_mut();
                m.2 += 1; m.3 += e;
                let hit = m.0 == key && m.1.len() > s / 64 && m.1[s / 64] >> (s % 64) & 1 != 0 && m.1[g / 64] >> (g % 64) & 1 == 0;
                if hit { m.4 += 1; m.5 += e - 97; }
                if exhausted && !hit {
                    m.0 = key;
                    m.1.clear();
                    m.1.resize(self.g.cur.len() / 64 + 1, 0);
                    for i in 0..self.g.cur.len() {
                        let pi = self.g.pidx(self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
                        if self.g.pc[pi].stamp == e16 { m.1[i / 64] |= 1 << (i % 64); }
                    }
                    m.6 += 1;
                }
            });
        }
        if envflag(1) {
            // how often is a search repeated with nothing changed? (approximate key: for measurement only)
            use std::hash::{Hash, Hasher};
            let mut h = crate::fasthash::Fx::default();
            (self.g.version, self.scaffold.len(), self.scaffold_placed, self.nav.blocked_edges.len(), sx, sy, sz, gx, gy, gz).hash(&mut h);
            (max_expand, scaffold_rule, self.nav.mine, self.towers_off, self.nav.start_h, self.nav.effort, self.nav.extra_effort).hash(&mut h);
            (self.no_scaffold_nav, self.have_tower, self.have_scaffold, self.layer_now, self.nav.towers_on, self.desperate, self.nav.scaffold_penalty).hash(&mut h);
            let key = h.finish();
            let e = self.nav.expanded as u64;
            MEMOSTAT.with(|m| { let mut m = m.borrow_mut(); if !m.0.insert(key) { m.1 += 1; m.2 += e; if !r { m.3 += e; } } m.4 += e; });
        }
        if envflag(0) {
            let e = self.nav.expanded as u64;
            let b = (64 - e.leading_zeros()) as usize;
            let k = if r { 0 } else if self.nav.heap.is_empty() { 1 } else { 2 };
            NAVSTAT.with(|st| { let mut st = st.borrow_mut(); st[k][b.min(23)].0 += 1; st[k][b.min(23)].1 += e; });
        }
        if dry {
            self.g.dry.rec = false;
            if !r && self.nav.exhausted { self.g.dry.st[0] += 1; }
            if r { self.g.dry.st[21] += 1; self.g.dry.st[22] += self.nav.expanded as u64; }
            if let Some(n) = said {
                // (MCSIM_DRYCHECK: the pocket said "no way"; the callers read nothing else)
                if r { panic!("dry pocket mismatch: ({sx},{sy},{sz}) -> ({gx},{gy},{gz}) budget {max_expand}: the pocket said no, the search found a way after {}", self.nav.expanded); }
                let st = &mut self.g.dry.st;
                if n != self.nav.expanded { st[12] += 1; }
                if !self.nav.exhausted { st[17] += 1; }
                st[18] = st[18].max(self.nav.expanded as u64 * 100 / self.nav.touched.max(1) as u64);
            } else if !r && self.nav.exhausted && !self.nav.went_once { self.dry_store(sx, sy, sz); }   // (dry by the usual search's rules only)
        }
        let dt = t0.elapsed().as_nanos() as u64;
        self.nav.search_ns += dt;
        if callstat() {
            let k = if r { "found" } else if self.nav.exhausted { "exhausted" } else { "budget" };
            let path = TAGS.with(|s| { let v = s.borrow(); if v.is_empty() { "-".to_string() } else { v.join(">") } });
            CALLSTAT.with(|m| { let mut m = m.borrow_mut(); let e = m.entry((path, k)).or_insert((0, 0, 0)); e.0 += 1; e.1 += self.nav.expanded as u64; e.2 += dt; });
        }
        r
    }

    /// A search whose caller wants only "is there a way" (and, if so, some way to remember). As `nav_search`; when `on`
    /// (revision `askonce`, `rev().pf_heur`) a search that has not settled the question after `ASK_FIRST` cells goes on
    /// as the search that looks at every place once, under the same budget: the usual search takes cells up again and
    /// again as it finds cheaper ways to them, and runs out of its budget in places it could have looked through.
    pub fn nav_ask(&mut self, on: bool, sx: i32, sy: i32, sz: i32, gx: i32, gy: i32, gz: i32, max_expand: i32, scaffold_rule: bool) -> bool {
        if !on { return self.nav_search(sx, sy, sz, gx, gy, gz, max_expand, scaffold_rule); }
        self.nav.once_after = ASK_FIRST;
        let r = self.nav_search(sx, sy, sz, gx, gy, gz, max_expand, scaffold_rule);
        self.nav.once_after = 0;
        r
    }

    /// `nav_ask` for the search whose failure is read as "shut in" (dry) or "unknown" (out of budget). The usual search
    /// runs dry within its budget only in a region it can look through with all its taking-up-again; the search that
    /// looks at every place once runs dry in any region smaller than the budget. So that the cheaper search adds no
    /// "shut in" of its own: dry after looking at more than a quarter of the budget's worth of places - the usual
    /// search decides. (Out of budget with every place looked at once: the usual search cannot have run dry either.)
    pub fn nav_ask_last(&mut self, on: bool, sx: i32, sy: i32, sz: i32, gx: i32, gy: i32, gz: i32, max_expand: i32, scaffold_rule: bool) -> bool {
        let mut r = self.nav_ask(on, sx, sy, sz, gx, gy, gz, max_expand, scaffold_rule);
        if on && !r && self.nav.exhausted && self.nav.went_once && self.nav.expanded > self.nav.last_budget / 4 {
            r = self.nav_search(sx, sy, sz, gx, gy, gz, max_expand, scaffold_rule);
        }
        if on && pfheur_check() {
            let (r1, x1, e1, w1) = (r, self.nav.exhausted, self.nav.expanded, self.nav.went_once);
            let kept = (self.nav.path.clone(), self.nav.mv.clone(), self.nav.length, self.nav.goal_cost, self.nav.scaffold_moves, self.nav.tower_moves, self.nav.reached, self.nav.searches, self.nav.total_expanded);
            let r0 = self.nav_search(sx, sy, sz, gx, gy, gz, max_expand, scaffold_rule);
            let (x0, e0) = (self.nav.exhausted, self.nav.expanded);
            if (r0 || !x0) != (r1 || !x1) { eprintln!("pfheur MISS: old found={r0} dry={x0} cells={e0}; new found={r1} dry={x1} cells={e1} once={w1}"); }
            (self.nav.path, self.nav.mv, self.nav.length, self.nav.goal_cost, self.nav.scaffold_moves, self.nav.tower_moves, self.nav.reached, self.nav.searches, self.nav.total_expanded) = kept;
            self.nav.exhausted = x1; self.nav.expanded = e1;
        }
        r
    }

    /// Puts the nodes of the kept search that are in the padded cells by (another search is about to write there).
    /// Every cell the search has reached is in its list of expansions or has an entry in its heap.
    fn kept_save(&mut self) {
        let kk = self.nav.kept_pc - 1;
        self.nav.kept_pc = 0;
        let e = self.nav.epoch16;
        let mut saved = std::mem::take(&mut self.nav.kept[kk].saved);
        saved.clear();
        let kp = &self.nav.kept[kk];
        let (nc, nh) = (kp.closed.len(), kp.heap.len());
        for j in 0..nc + nh {
            let k = if j < nc { kp.closed[j] } else { kp.heap[j - nc] as i32 };
            let (x, y, z) = self.g.unpk(k);
            let pi = self.g.pidx(x, y, z);
            let c = &self.g.pc[pi];
            debug_assert!(c.stamp == e);
            if c.stamp == e { saved.push((pi as u32, c.cost, c.parent, c.how)); }
        }
        if plan_check() > 0 {
            // self-check: no cell carries the search's stamp that was not put by
            let mut n = 0usize;
            for c in self.g.pc.iter() { if c.stamp == e { n += 1; } }
            let mut seen: crate::fasthash::FastSet<u32> = Default::default();
            for sv in saved.iter() { seen.insert(sv.0); }
            if n != seen.len() || saved.len() != nc + nh { panic!("kept search: {n} cells stamped, {} put by ({} entries of {})", seen.len(), saved.len(), nc + nh); }
        }
        self.nav.kept[kk].saved = saved;
    }

    /// Writes the nodes of kept search `kk` back into the padded cells, under a stamp of their own.
    fn kept_restore(&mut self, kk: usize) {
        if self.nav.kept_pc != 0 { self.kept_save(); }
        self.nav.epoch16 = self.nav.epoch16.wrapping_add(1);
        if self.nav.epoch16 == 0 { for c in self.g.pc.iter_mut() { c.stamp = 0; } self.nav.epoch16 = 1; }
        let e = self.nav.epoch16;
        for j in 0..self.nav.kept[kk].saved.len() {
            let (pi, cost, parent, how) = self.nav.kept[kk].saved[j];
            let c = &mut self.g.pc[pi as usize];
            c.stamp = e; c.cost = cost; c.parent = parent; c.how = how; c.pad = 0;
        }
        self.nav.kept_pc = kk + 1;
    }

    /// `nav_search` with loose blocks allowed, as kept search `slot` of the stance planner (revision `plankeep`).
    /// The first question of a plan (or after the start cell, the rules or the world changed) starts the search and is
    /// answered exactly as `nav_search` answers it. A later question continues that search: the heap's keys are made
    /// anew for the new goal, and it goes on until the goal has been reached or a search from zero for that goal would,
    /// as far as can be told, be out of budget (see the count in the loop) and a share of the budget has been spent.
    pub fn nav_search_kept(&mut self, slot: usize, sx: i32, sy: i32, sz: i32, gx: i32, gy: i32, gz: i32, max_expand: i32) -> bool {
        self.nav.k_was_resumed = false;
        if !self.g.pk_ok || self.nav.touch_limit > 0 || lossy() > 1 || self.nav.once_after != 0 || !self.nav.goal_any.is_empty() { return self.nav_search(sx, sy, sz, gx, gy, gz, max_expand, true); }
        let t0 = std::time::Instant::now();
        use std::hash::{Hash, Hasher};
        // (the plan, the start and the rules: what the Java version keys on. Nothing real changes within a plan
        // unless `route` gets the bot unstuck - the kept searches are dropped there - so the world is the same too;
        // here that is checked as well, by the grid's content hash, and counted if it ever fails)
        let mut h = crate::fasthash::Fx::default();
        (self.plans, sx, sy, sz, self.nav.mine, self.towers_off, self.nav.start_h).hash(&mut h);
        (self.no_scaffold_nav, self.have_tower, self.have_scaffold, self.nav.towers_on, self.desperate, self.relax_next, self.nav.stop_at).hash(&mut h);
        (self.nav.scaffold_penalty, self.nav.tower_penalty, self.nav.perch_penalty, self.nav.effort).hash(&mut h);
        let key = h.finish();
        let mut h = crate::fasthash::Fx::default();
        (self.g.version, self.scaffold.len(), self.scaffold_placed, self.nav.blocked_edges.len(), self.layer_now, self.col_next_set).hash(&mut h);
        let world = h.finish();
        let mut resume = self.nav.kept[slot].live && self.nav.kept[slot].key == key;
        if resume && self.nav.kept[slot].world != world { resume = false; KEPT_WORLD_CHANGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
        self.nav.kept[slot].world = world;
        self.nav.kept[slot].key = key;
        self.nav.kept[slot].live = false;
        let inside = self.g.inside(sx, sy, sz) && self.g.inside(gx, gy, gz);
        let r;
        if !inside {
            // (as the search itself answers; the kept search is over)
            if !resume { self.nav.epoch += 1; self.nav.heap.clear(); }
            self.nav.expanded = if resume { self.nav.kept[slot].expanded } else { 0 };
            self.nav.new_expanded = 0; self.nav.length = 0; self.nav.searches += 1; self.nav.exhausted = false; self.nav.reached = -1;
            self.nav.tx = gx; self.nav.ty = gy; self.nav.tz = gz;
            r = false;
        } else {
            if resume {
                if self.nav.kept_pc != slot + 1 { self.kept_restore(slot); }
            } else {
                if self.nav.kept_pc == slot + 1 { self.nav.kept_pc = 0; }
                self.nav.kept[slot].closed.clear();
                self.nav.kept[slot].saved.clear();
                self.nav.kept[slot].t = [gx, gy, gz];
            }
            std::mem::swap(&mut self.nav.heap, &mut self.nav.kept[slot].heap);
            if resume {
                let t = self.nav.kept[slot].t;
                if t != [gx, gy, gz] {
                    // aim the search at this question's goal: every open cell gets its key for the new goal (entries
                    // that are out of date are dropped on the way), and the heap is put in order again
                    let mut hp = std::mem::take(&mut self.nav.heap);
                    let mut n = 0;
                    for j in 0..hp.len() {
                        let v = hp[j];
                        let node = v as i32;
                        let (x, y, z) = self.g.unpk(node);
                        let c = self.g.pc[self.g.pidx(x, y, z)].cost;
                        self.nav.tx = t[0]; self.nav.ty = t[1]; self.nav.tz = t[2];
                        if (v >> 32) as i32 != c + self.nav.heuristic(x, y, z) { continue; }
                        self.nav.tx = gx; self.nav.ty = gy; self.nav.tz = gz;
                        hp[n] = (((c + self.nav.heuristic(x, y, z)) as i64) << 32) | (node as i64 & 0xffffffff);
                        n += 1;
                    }
                    hp.truncate(n);
                    let mut st = n / 2;
                    while st > 0 {
                        st -= 1;
                        let v = hp[st];
                        let mut i = st;
                        loop {
                            let l = 2 * i + 1;
                            if l >= n { break; }
                            let r = l + 1;
                            let m = if r < n && hp[r] < hp[l] { r } else { l };
                            if hp[m] >= v { break; }
                            hp[i] = hp[m];
                            i = m;
                        }
                        hp[i] = v;
                    }
                    self.nav.heap = hp;
                    self.nav.kept[slot].t = [gx, gy, gz];
                }
                // how the cells expanded so far rate for this goal
                self.nav.tx = gx; self.nav.ty = gy; self.nav.tz = gz;
                let mut hist = std::mem::take(&mut self.nav.kept[slot].hist);
                hist.clear();
                hist.resize(KEPT_BUCKETS, 0);
                for j in 0..self.nav.kept[slot].closed.len() {
                    let (x, y, z) = self.g.unpk(self.nav.kept[slot].closed[j]);
                    let f = self.g.pc[self.g.pidx(x, y, z)].cost + self.nav.heuristic(x, y, z);
                    hist[((f.max(0) >> 3) as usize).min(KEPT_BUCKETS - 1)] += 1;
                }
                self.nav.kept[slot].hist = hist;
            }
            self.nav.k = slot + 1;
            self.nav.k_resume = resume;
            r = self.nav_search_k::<true, true>(sx, sy, sz, gx, gy, gz, max_expand, true);
            self.nav.k = 0;
            self.nav.k_resume = false;
            std::mem::swap(&mut self.nav.heap, &mut self.nav.kept[slot].heap);
            self.nav.kept[slot].live = true;
            self.nav.kept_pc = slot + 1;
        }
        self.nav.k_was_resumed = resume;
        let dt = t0.elapsed().as_nanos() as u64;
        self.nav.search_ns += dt;
        if callstat() {
            let k = if r { "found" } else if self.nav.exhausted { "exhausted" } else { "budget" };
            let path = TAGS.with(|s| { let mut v = s.borrow().clone(); v.push(if resume { "cont" } else { "first" }); v.join(">") });
            CALLSTAT.with(|m| { let mut m = m.borrow_mut(); let e = m.entry((path, k)).or_insert((0, 0, 0)); e.0 += 1; e.1 += self.nav.new_expanded as u64; e.2 += dt; });
        }
        r
    }

    /// The `N_*` facts of a cell. Fast: read from the grid's padded mirror by index `p`. Otherwise: worked out on the
    /// spot from the cell's state and the scaffold set (no `N_OPEN`: the rules are asked every time).
    #[inline(always)]
    fn cb<const F: bool>(&self, p: usize, x: i32, y: i32, z: i32) -> u32 {
        if F { self.g.nvu(p) } else { super::grid::state_bits(self.g.get(x, y, z)) | if self.in_scaffold(x, y, z) { super::grid::N_SCAF } else { 0 } }
    }
    /// `stand16` (fast: through the per-cell cache)
    #[inline(always)]
    fn st<const F: bool>(&self, p: usize, x: i32, y: i32, z: i32) -> i32 { if F { self.g.p_stand_u(p, x, y, z) } else { self.g.stand16_compute(x, y, z) } }
    #[inline(always)]
    fn hg<const F: bool>(&self, p: usize, x: i32, y: i32, z: i32) -> bool { if F { self.g.p_hang_u(p) } else { self.g.hang(x, y, z) } }
    #[inline(always)]
    fn rg<const F: bool>(&self, p: usize, x: i32, y: i32, z: i32) -> i32 { if F { self.g.p_rung_u(p) } else { self.g.rung(x, y, z) } }
    /// `head_clear` (fast: the test that settles nearly every call is made in place)
    #[inline(always)]
    fn hc<const F: bool>(&self, x: i32, y: i32, z: i32, h: i32) -> bool { if F { h + 29 <= 32 || self.head_clear(x, y, z, h) } else { self.head_clear(x, y, z, h) } }

    /// The search (Java `Nav.search`), written once. `F = true` is the form that runs: cells are read from the grid's
    /// padded mirror (`Grid::pc`: the `N_*` bits, the cached `stand16`, the search node) and nodes are named by
    /// `Grid::pk`. `F = false` reads every fact afresh from `Grid::get`, the state tables and the scaffold set, keeps
    /// its nodes in `nav.nodes` under the grid index, and takes none of the shortcuts: it is what `MCSIM_NAVCHECK`
    /// compares against, so a mirror that has gone stale shows as a mismatch.
    #[inline]
    pub(super) fn nav_search_g<const F: bool>(&mut self, sx: i32, sy: i32, sz: i32, gx: i32, gy: i32, gz: i32, max_expand: i32, scaffold_rule: bool) -> bool {
        self.nav_search_k::<F, false>(sx, sy, sz, gx, gy, gz, max_expand, scaffold_rule)
    }

    /// `K = true` (with `F`): the search is one of the kept ones (`nav_search_kept`, which has put its heap and nodes
    /// in place): it leaves heap and nodes fit to go on with, lists what it expands, and - `nav.k_resume` - does not
    /// start but goes on. Everything `K` adds is compiled out of the plain search.
    fn nav_search_k<const F: bool, const K: bool>(&mut self, sx: i32, sy: i32, sz: i32, gx: i32, gy: i32, gz: i32, max_expand: i32, scaffold_rule: bool) -> bool {
        use super::grid::*;

        // (a padded index that is never read: the checking form passes it around untouched)
        const NOWHERE: usize = 1 << 40;

        // (probe: dry.rs has filled the heap with (way of standing, cell); nothing of the last search is touched)
        let probe = F && self.nav.probe;
        let rv = crate::rev::rev();
        let tb = t();
        let resume = K && self.nav.k_resume;
        // (candidate "pieces") No way leads straight up into a cobweb: the bot does not climb a scaffolding column,
        // put one up or jump up on a block of its own where its body would then be in a web. In a web it rises a
        // hundredth of a block a tick, the climb gives up before it is up, and every way from there begins with the
        // same climb - in two large builds the bot hung like that under a web of the build for 1.0M and 2.4M ticks,
        // and the clean-up found "no way" to any piece from there.
        let web: u16 = 0;   // (candidate "pieces", screened out: no climb into a cobweb - superseded by `weblate`)
        let kk = if K { self.nav.k - 1 } else { 0 };
        if resume {
            // (the start, the goal's being inside and the heap are `nav_search_kept`'s business)
            self.nav.expanded = self.nav.kept[kk].expanded;
            self.nav.length = 0; self.nav.searches += 1; self.nav.exhausted = false; self.nav.reached = -1;
            self.nav.once = false; self.nav.went_once = false;
            self.nav.tx = gx; self.nav.ty = gy; self.nav.tz = gz;
            self.nav.scaffold_moves = 0; self.nav.tower_moves = 0; self.nav.fast = F; self.nav.touched = 1;
        }
        let expanded_before = if resume { self.nav.expanded } else { 0 };
        if !probe && !resume {
        self.nav.epoch += 1;
        self.nav.heap.clear();
        self.nav.expanded = 0;
        self.nav.length = 0;
        self.nav.searches += 1;
        self.nav.exhausted = false;
        self.nav.reached = -1;
        self.nav.once = false;
        self.nav.went_once = false;
        self.nav.tx = gx; self.nav.ty = gy; self.nav.tz = gz;
        if !self.g.inside(sx, sy, sz) || !self.g.inside(gx, gy, gz) { return false; }
        }
        let max_expand = max_expand.min(self.nav.effort * (400 + 80 * ((gx - sx).abs() + (gz - sz).abs() + 3 * (gy - sy).abs())) + self.nav.extra_effort);
        // MCSIM_LOSSY=k (not faithful; for the speed/fidelity trade-off only): searches give up after 1/k of their budget
        let max_expand = if F && max_expand > 96 && lossy() > 1 { (max_expand / lossy()).max(97) } else { max_expand };
        // a continued question: its budget counts from the search's beginning, but `KEPT_SHARE` of it is granted anew;
        // and it is not the budget that ends it but the count below (`e_old`)
        if K { self.nav.k_budget = max_expand; }
        let k_budget = max_expand;
        let share_limit = if resume { max_expand.max(self.nav.expanded + max_expand / KEPT_SHARE) } else { max_expand };
        let max_expand = if resume { i32::MAX / 2 } else { max_expand };
        let (mut e_ptr, mut e_old, mut spent, mut count_spent) = (0usize, 0i32, false, false);
        let (start, goal) = if probe { (NOWHERE, NOWHERE) } else if F { (self.g.pk(sx, sy, sz) as usize, self.g.pk(gx, gy, gz) as usize) } else { (self.g.idx(sx, sy, sz), self.g.idx(gx, gy, gz)) };
        if !probe && !resume {
        self.nav.scaffold_moves = 0;
        self.nav.tower_moves = 0;
        self.nav.fast = F;
        if F {
            // (a kept search whose nodes are in the padded cells: they are put by before this search writes there)
            if self.nav.kept_pc != 0 { self.kept_save(); }
            self.nav.epoch16 = self.nav.epoch16.wrapping_add(1);
            if self.nav.epoch16 == 0 { for c in self.g.pc.iter_mut() { c.stamp = 0; } self.nav.epoch16 = 1; }
            let ps = self.g.pidx(sx, sy, sz);
            let cell = &mut self.g.pc[ps];
            cell.stamp = self.nav.epoch16; cell.cost = 0; cell.parent = -1; cell.how = WALK; cell.pad = 0;
        } else {
            self.nav.nodes[start] = Node { stamp: self.nav.epoch, cost: 0, parent: -1, how: WALK };
        }
        let hk = self.nav.heuristic(sx, sy, sz) as i64;
        self.nav.push(hk, start as i32);
        self.nav.touched = 1;
        }
        let touch_limit = if F && self.nav.touch_limit > 0 && max_expand < self.nav.touch_limit { self.nav.touch_limit } else { i32::MAX };
        let y0 = self.g.y0;
        let oy = self.g.oy;
        let mine = self.nav.mine;
        let l = self.g.pys;
        let pd: [isize; 4] = [self.g.pxs as isize, -(self.g.pxs as isize), self.g.pzs as isize, -(self.g.pzs as isize)];
        // the parts of scaffold_ok / tower_ok that do not depend on the cell (with N_OPEN they settle most cells)
        let sc_on = scaffold_rule && (!F || (self.have_scaffold && !self.no_scaffold_nav));
        let tw_on = self.have_tower && !self.no_scaffold_nav && !self.towers_off;
        // (candidate `walkway`, only in the main pass of a build behind its pace) From the top of a scaffolding column
        // the bot may put a piece of scaffolding out sideways - one click on the piece underfoot, looking down, puts
        // it beside that piece in the direction faced - and step onto it: a walkway at working height along a wall,
        // instead of going down to the floor and putting up a fresh column a block further on. A piece hangs on
        // the column through at most 6 others (the game lets it fall otherwise); the walkway goes when the columns
        // that carry it are taken down.
        let wing_on = scaffold_rule && tw_on && !probe && !self.no_wings && !self.cleaning && !self.main_done && crate::cand::on("walkway") && crate::ckpt::armed(self.ticks) && self.behind_pace();
        let wing_lone = wing_on && self.walk_down();
        let wing_len = if wing_on { crate::cand::num("MCSIM_WALKLEN", 6) as i32 } else { 0 };
        let door_bits = N_DOOR_MASK | N_CLOSED_GATE;
        // the fast form keeps what does not change during a search in locals: (epoch, whether dry.rs lists the cells,
        // probe) for `rx`, the grid's packing and strides, the heuristic's terms, the stop cells' switch
        let fc = (self.nav.epoch16, self.g.dry.rec, probe);
        let (gx0, gz0, bx, bxz) = (self.g.x0, self.g.z0, self.g.bx, self.g.bx + self.g.bz);
        let (mx, mz) = ((1i32 << bx) - 1, (1i32 << self.g.bz) - 1);
        let (pxs, pzs) = (self.g.pxs, self.g.pzs);
        let h_mul = if mine { 5 } else { 1 };
        let h_up = if self.nav.towers_on { C_TOWER + self.nav.tower_penalty } else { C_PILLAR + self.nav.scaffold_penalty };
        let stop_at = self.nav.stop_at;
        let mut once = false;
        let once_after = if probe { 0 } else { self.nav.once_after };
        let any = !probe && !self.nav.goal_any.is_empty();
        let mut found = false;
        let goal_p = if resume { self.g.pidx(gx, gy, gz) } else { NOWHERE };
        while self.nav.front != NO_FRONT || !self.nav.heap.is_empty() {
            // (a continued search: the goal counts as found once it has been reached at all)
            if K && resume && self.g.pc[goal_p].stamp == fc.0 {
                self.nav_build(goal); self.nav.goal_cost = self.g.pc[goal_p].cost; self.nav.reached = self.g.idx(gx, gy, gz) as i32; found = true; break;
            }
            let top = if !F { self.nav.pop() } else if self.nav.front != NO_FRONT { std::mem::replace(&mut self.nav.front, NO_FRONT) } else { self.nav.pop_f() };
            let i = (top as i32) as usize;
            let (x, y, z, p) = if F {
                // (`Grid::unpk`, `Grid::pidx`)
                let k = i as i32;
                let (rx, ry, rz) = (k & mx, k >> bxz, (k >> bx) & mz);
                (gx0 + rx, y0 + ry, gz0 + rz, (rx + 1) as usize * pxs + (rz + 1) as usize * pzs + (ry + 2) as usize)
            } else { (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i), NOWHERE) };
            debug_assert!(!F || ((x, y, z) == self.g.unpk(i as i32) && p == self.g.pidx(x, y, z)));
            let c = if probe { 0 } else if F { unsafe { self.g.pc.get_unchecked(p).cost } } else { self.nav.nodes[i].cost };
            if probe {
                if !self.dry_probe_cell(x, y, z) { break; }
            } else {
            let hx = if F && !any {
                // (`Nav::heuristic`)
                let dy = gy - y;
                h_mul * (2 * ((gx - x).abs() + (gz - z).abs()) * C_WALK + if dy > 0 { dy * h_up } else { -dy * 2 })
            } else { self.nav.heuristic(x, y, z) };
            debug_assert!(hx == self.nav.heuristic(x, y, z));
            if !once && (top >> 32) as i32 != c + hx { continue; }   // stale entry
            if (if any { self.nav.goal_any.contains(&(self.g.idx(x, y, z) as i32)) } else { i == goal }) || (if F { stop_at && self.exit_nodes.contains(&(self.g.idx(x, y, z) as i32)) } else { self.nav.stop_at && self.exit_nodes.contains_plain(&(self.g.idx(x, y, z) as i32)) }) { self.nav_build(i); self.nav.goal_cost = c; self.nav.reached = self.g.idx(x, y, z) as i32; found = true; if K { self.nav.push_v(top); } break; }
            if K && resume {
                // would a search from zero for this goal be out of budget by now? It takes cells in the order of cost
                // so far plus estimate: counted are the cells expanded before this question that rate better than
                // the one now taken, and the cells expanded for this question. Both must say stop: that, and the share.
                let lb = ((((top >> 32) as i32).max(0) >> 3) as usize).min(KEPT_BUCKETS);
                while e_ptr < lb { e_old += self.nav.kept[kk].hist[e_ptr] as i32; e_ptr += 1; }
                if e_old + (self.nav.expanded - expanded_before) >= k_budget && self.nav.expanded >= share_limit { self.nav.push_v(top); spent = true; count_spent = true; break; }
            }
            if self.nav.touched >= touch_limit { self.nav.total_expanded -= (max_expand + 1 - self.nav.expanded) as i64; self.nav.expanded = max_expand + 1; break; }
            self.nav.expanded += 1;
            if self.nav.expanded > max_expand { if K { self.nav.push_v(top); spent = true; } break; }
            if K { self.nav.kept[kk].closed.push(i as i32); }
            if self.nav.expanded == once_after && !once {
                // from here on every place once: what is waiting (in the heap and in front) is put in the order of its
                // distance to the goal - entries outdated by a cheaper way to the same cell are dropped - and no cell
                // is taken up again
                once = true;
                self.nav.once = true;
                let mut old = std::mem::take(&mut self.nav.heap);
                if self.nav.front != NO_FRONT { old.push(std::mem::replace(&mut self.nav.front, NO_FRONT)); }
                for &v in &old {
                    let n = (v as i32) as usize;
                    let (qx, qy, qz) = if F { self.g.unpk(n as i32) } else { (self.g.x_of(n), self.g.y_of(n), self.g.z_of(n)) };
                    let qc = if F { self.g.pc[self.g.pidx(qx, qy, qz)].cost } else { self.nav.nodes[n].cost };
                    let hq = self.nav.heuristic(qx, qy, qz);
                    if (v >> 32) as i32 == qc + hq { self.nav.push(hq as i64, n as i32); }
                }
            }
            }
            let how_i = if probe { (top >> 32) as u8 } else if F { let cell = unsafe { self.g.pc.get_unchecked_mut(p) }; if fc.1 { cell.pad |= HOW_BIT[cell.how as usize & 15] << 4; } cell.how } else { self.nav.nodes[i].how };
            let on_tower = how_i == TOWER || how_i == WING;
            let virtual_stand = how_i == PILLAR || how_i == BRIDGE || on_tower;
            self.nav.virtual_stand = virtual_stand;
            // a cell entered by digging or clearing holds scaffold now, but will be empty with the bot standing in it
            let dug = how_i == DIG || how_i == CLEAR || how_i == MINE;
            // (where the bot sinks to by digging or mining, it stands on whatever is below: on a post or wall that is
            // half a block up, and the head room is wanted for that)
            let mut h = if virtual_stand { 0 } else if dug { if rv.a1 { 0.max(tb.col_top[self.g.get(x, y - 1, z) as usize] as i32 - 16) } else { 0 } } else { self.st::<F>(p, x, y, z) };
            if i == start && h < 0 && self.nav.start_h >= 0 { h = self.nav.start_h; }
            let ii = i as i32;
            if h < 0 && self.hg::<F>(p, x, y, z) { if F { self.ladder_moves_f(fc, ii, p, x, y, z, c); } else { self.ladder_moves(ii, x, y, z, c); } continue; }
            if h < 0 {
                // in the air: fall
                let mut ty = y - 1;
                let mut pt = p - l;
                while ty >= y0 {
                    if self.st::<F>(pt, x, ty, z) >= 0 { self.rx::<F>(fc, ii, pt, x, ty, z, c + C_DROP, DROP); break; }
                    if self.cb::<F>(pt, x, ty, z) & N_PASS == 0 { break; }
                    ty -= 1;
                    pt -= l;
                }
                continue;
            }
            let below = self.cb::<F>(p - l, x, y - 1, z);
            let here = self.cb::<F>(p, x, y, z);
            let up1 = self.cb::<F>(p + l, x, y + 1, z);
            let up2 = self.cb::<F>(p + 2 * l, x, y + 2, z);
            let head_up = dug || up2 & N_PASS != 0;
            let floor_tower = on_tower || below & N_TOWER != 0;
            let on_scaffold = !virtual_stand && !floor_tower && h == 0 && below & N_SCAF != 0;
            let leave = if on_scaffold { C_ABANDON } else { 0 };
            for d in 0..4usize {
                let (nx, nz) = (x + NDX[d], z + NDZ[d]);
                let np = (p as isize + pd[d]) as usize;
                let n0 = self.cb::<F>(np, nx, y, nz);
                let n1 = self.cb::<F>(np + l, nx, y + 1, nz);
                // doors and gates between the two columns (Java `doorway`): 1 opens by hand, -1 does not
                if (here | up1 | n0 | n1) & door_bits != 0 {
                    let mut door = 0;
                    for k in 0..4 {
                        let s = match k { 0 => here, 1 => up1, 2 => n0, _ => n1 };
                        let bit = if k < 2 { d } else { d ^ 1 } as u32;
                        let blocks = (s >> (N_DOOR_SHIFT + bit)) & 1 != 0 || (k == 2 && s & N_CLOSED_GATE != 0);
                        if !blocks { continue; }
                        if s & N_HAND_DOOR == 0 { door = -1; break; }
                        door = 1;
                    }
                    if door != 0 {
                        if door > 0 && h == 0 && !virtual_stand {
                            let floor = self.cb::<F>(np - l, nx, y - 1, nz);
                            let stand = if n0 & N_CLOSED_GATE != 0 { floor & N_FLOOR16 != 0 && n1 & N_PASS != 0 } else { self.st::<F>(np, nx, y, nz) == 0 };
                            if stand { self.rx::<F>(fc, ii, np, nx, y, nz, c + C_WALK + C_GATE, GATE); }
                        }
                        continue;
                    }
                }
                if h <= 8 && self.hg::<F>(np, nx, y, nz) { let k = if self.rg::<F>(np, nx, y, nz) == 3 { C_GATE } else { 0 }; self.rx::<F>(fc, ii, np, nx, y, nz, c + C_WALK + k, LADDER); continue; }
                let hu = if head_up { self.st::<F>(np + l, nx, y + 1, nz) } else { -1 };
                let hl = self.st::<F>(np, nx, y, nz);
                if h == 0 && hl == 0 && self.rg::<F>(np - l, nx, y - 1, nz) == 3 { self.rx::<F>(fc, ii, np - l, nx, y - 1, nz, c + C_WALK + C_GATE + C_DESCEND, LADDER); }
                if hl >= 0 {
                    let rise = hl - h;
                    if rise <= 9 { if if rise <= 0 { self.hc::<F>(nx, y, nz, h) } else { hl + 29 <= 32 || head_up } { self.rx::<F>(fc, ii, np, nx, y, nz, c + C_WALK, WALK); } }
                    else if rise <= 19 && head_up { self.rx::<F>(fc, ii, np, nx, y, nz, c + C_JUMP, JUMP); }
                } else if hu >= 0 {
                    let rise = 16 + hu - h;
                    if rise <= 9 { self.rx::<F>(fc, ii, np + l, nx, y + 1, nz, c + C_WALK, WALK); }
                    else if rise <= 19 { self.rx::<F>(fc, ii, np + l, nx, y + 1, nz, c + C_JUMP, JUMP); }
                    // (revision "minefence") What stands there can be stood on but is too high to get onto - a fence,
                    // a wall: a search that may break through may break through it. Before, such a block was never
                    // offered for breaking, and a bot fenced in stayed fenced in.
                    else if mine && rv.mine_fence { self.mine_through(ii, nx, y, nz, c); }
                } else {
                    // own scaffolding in the way on level ground? (Java `clearable`, `isScaffold`)
                    let s0 = n0 & (N_SCAF | N_TOWER) == N_SCAF;
                    let s1 = n1 & (N_SCAF | N_TOWER) == N_SCAF;
                    let clearable = h == 0 && (s0 || s1) && (s0 || n0 & N_PASS != 0) && (s1 || n1 & N_PASS != 0) && self.cb::<F>(np - l, nx, y - 1, nz) & N_FLOOR16 != 0;
                    if clearable {
                        let n = (s0 as i32) + (s1 as i32);
                        self.rx::<F>(fc, ii, np, nx, y, nz, c + C_WALK + n * C_CLEAR, CLEAR);
                    } else if mine && self.mine_through(ii, nx, y, nz, c) {
                        // (relaxed inside)
                    } else if n0 & N_PASS != 0 && n1 & N_PASS != 0 && self.hc::<F>(nx, y, nz, h) {
                        let mut step_down = false;
                        let mut ty = y - 1;
                        let mut pt = np - l;
                        while ty >= y0 {
                            // (fast: cells one can pass, cannot stand in and that have no rung - nothing happens
                            // there, and `Grid::scan` says how many of them follow one another)
                            // (a run ends at the first cell that is not marked, or where its word of marks ends: the
                            // cell after it is looked at in full either way)
                            if F { let run = self.g.scan_run(pt); ty -= run as i32; pt = pt.wrapping_sub(run); if ty < y0 { break; } }
                            let hd = self.st::<F>(pt, nx, ty, nz);
                            if hd >= 0 {
                                let fall = (y - ty) * 16 + h - hd;
                                let cc = c + if fall <= 9 { C_WALK } else { C_DROP + (y - ty) / 2 + leave + if hd != 0 { C_PIT } else { 0 } };
                                self.rx::<F>(fc, ii, pt, nx, ty, nz, cc, if fall <= 9 { WALK } else { DROP });
                                step_down = fall <= 10;
                                break;
                            }
                            if self.hg::<F>(pt, nx, ty, nz) { self.rx::<F>(fc, ii, pt, nx, ty, nz, c + C_WALK + C_DESCEND * (y - ty), LADDER); break; }
                            if self.cb::<F>(pt, nx, ty, nz) & N_PASS == 0 { break; }
                            ty -= 1;
                            pt -= l;
                        }
                        if sc_on && h == 0 && !step_down && !floor_tower && (virtual_stand || below & N_OUT_FULL != 0) {
                            let q = self.cb::<F>(np - l, nx, y - 1, nz);
                            if q & N_AIR != 0 && (q & N_OPEN != 0 || self.scaffold_ok(nx, y - 1, nz)) {
                                let sp = self.nav.scaffold_penalty;
                                self.rx::<F>(fc, ii, np, nx, y, nz, c + C_BRIDGE + sp, BRIDGE);
                            }
                        }
                        if wing_on && h == 0 && !step_down && floor_tower && !dug && (on_tower || !virtual_stand) && (here | up1) & N_TOWER == 0 {
                            let q = self.cb::<F>(np - l, nx, y - 1, nz);
                            // (not beside any other scaffolding: a run hangs on one column, so that one break drops it)
                            let lone = !wing_lone || F && (0..4).all(|d2| d2 == (d ^ 1) || self.g.nvu(((np - l) as isize + pd[d2]) as usize) & N_TOWER == 0);
                            if lone && q & N_AIR != 0 && n0 & N_AIR != 0 && (q & N_OPEN != 0 || self.tower_ok(nx, y - 1, nz)) && self.wing_dist(ii) < wing_len {
                                let tp = self.nav.tower_penalty;
                                self.rx::<F>(fc, ii, np, nx, y, nz, c + C_TOWER + tp, WING);
                            }
                        }
                    }
                }
            }
            let webbed = web != 0 && (self.g.get(x, y + 1, z) == web || self.g.get(x, y + 2, z) == web);
            if sc_on && !webbed && h == 0 && head_up && here & N_AIR != 0 && up1 & N_PASS != 0 && (virtual_stand || dug || below & N_FLAT_TOP != 0)
                && (here & N_OPEN != 0 || self.scaffold_ok(x, y, z)) {
                let sp = self.nav.scaffold_penalty;
                self.rx::<F>(fc, ii, p + l, x, y + 1, z, c + C_PILLAR + sp, PILLAR);
            }
            // standing on a fence or wall (half a block up in an empty cell): a jump still clears the cell, the block
            // goes onto the post
            if rv.x_fpillar && sc_on && h > 0 && h <= 8 && !virtual_stand && !dug && head_up && here & N_AIR != 0 && up1 & N_PASS != 0
                && (here & N_OPEN != 0 || self.scaffold_ok(x, y, z)) {
                let bs = self.g.get(x, y - 1, z) as usize;
                if tb.col_top[bs] as i32 == 16 + h && !tb.interactive[bs] {
                    let sp = self.nav.scaffold_penalty;
                    self.rx::<F>(fc, ii, p + l, x, y + 1, z, c + C_PILLAR + sp, PILLAR);
                }
            }
            // last resort: break the block of the build underfoot and drop to whatever is below
            if mine && !virtual_stand && !floor_tower && h == 0 && self.g.inside(x, y - 1, z) && below & N_SCAF == 0 {
                let tk = self.mine_ticks(x, y - 1, z);
                if tk > 0 {
                    let mut ty = y - 2;
                    let mut pt = p - 2 * l;
                    while ty >= y0 - 1 {
                        let bl = self.cb::<F>(pt, x, ty, z);
                        if bl & N_COL16 != 0 { self.rx::<F>(fc, ii, pt + l, x, ty + 1, z, c + C_DROP + tk + C_MINE, MINE); break; }
                        if bl & N_COLNZ != 0 { break; }
                        ty -= 1;
                        pt -= l;
                    }
                }
            }
            // the same when standing half a block up on a post or wall below (nothing in the own cell): break the post
            if rv.a1 && mine && !virtual_stand && !dug && h > 0 && here & N_PASS != 0 && self.g.inside(x, y - 1, z) && below & N_SCAF == 0 {
                let tk = self.mine_ticks(x, y - 1, z);
                if tk > 0 {
                    let mut ty = y - 2;
                    let mut pt = p - 2 * l;
                    while ty >= y0 - 1 {
                        let bl = self.cb::<F>(pt, x, ty, z);
                        if bl & N_COL16 != 0 { self.rx::<F>(fc, ii, pt + l, x, ty + 1, z, c + C_DROP + tk + C_MINE, MINE); break; }
                        if bl & N_COLNZ != 0 { break; }
                        ty -= 1;
                        pt -= l;
                    }
                }
            }
            // the same when standing on something low inside the own cell (a slab ...): break that
            if mine && !virtual_stand && h > 0 && h < 16 && self.g.inside(x, y, z) {
                let tk = self.mine_ticks(x, y, z);
                if tk > 0 {
                    let mut ty = y - 1;
                    let mut pt = p - l;
                    while ty >= y0 - 1 {
                        let bl = self.cb::<F>(pt, x, ty, z);
                        if bl & N_COL16 != 0 { if ty + 1 < y { self.rx::<F>(fc, ii, pt + l, x, ty + 1, z, c + C_DROP + tk + C_MINE, MINE); } break; }
                        if bl & N_COLNZ != 0 { break; }
                        ty -= 1;
                        pt -= l;
                    }
                }
            }
            // take down the scaffold block underfoot and sink
            if !virtual_stand && !floor_tower && h == 0 && below & N_SCAF != 0 {
                let mut ty = y - 1;
                let mut pt = p - l;
                while ty >= y0 {
                    if ty < y - 1 && self.cb::<F>(pt, x, ty, z) & N_PASS == 0 { break; }
                    let bl = self.cb::<F>(pt - l, x, ty - 1, z);
                    if bl & N_COL16 != 0 { self.rx::<F>(fc, ii, pt, x, ty, z, c + C_DIG + (y - 1 - ty) / 2, DIG); break; }
                    if bl & N_COLNZ != 0 { break; }
                    ty -= 1;
                    pt -= l;
                }
            }
            // scaffolding columns: rise inside one, sink through one, or put one up / make one taller
            if h == 0 {
                let s_here = !virtual_stand && here & N_TOWER != 0;
                let head2 = up2 & N_PASS != 0 && !webbed;
                if s_here && head2 { self.rx::<F>(fc, ii, p + l, x, y + 1, z, c + C_CLIMB, CLIMB); }
                if !virtual_stand && !dug && below & N_TOWER != 0 && self.st::<F>(p - l, x, y - 1, z) >= 0 { self.rx::<F>(fc, ii, p - l, x, y - 1, z, c + C_DESCEND, DESCEND); }
                if !s_here && !dug && head2 && here & N_AIR != 0 {
                    // (Java `towerBase`, then `towerOk`)
                    let base = on_tower || below & N_TOWER != 0 || (below & N_STURDY_TOP != 0 && below & N_SCAF == 0);
                    if base && ((tw_on && here & N_OPEN != 0) || (here & N_OPEN == 0 && self.tower_ok(x, y, z))) {
                        let perched = !on_tower && y > oy && below & N_TOWER == 0;
                        let cc = c + C_TOWER + self.nav.tower_penalty + if perched { self.nav.perch_penalty } else { 0 };
                        self.rx::<F>(fc, ii, p + l, x, y + 1, z, cc, TOWER);
                    }
                }
            }
        }
        // (nothing left to expand: there is no route. Stopped by the budget: there may be one - the caller must not
        // take that for "no")
        if probe { return false; }
        // (what is held in front goes back among the rest: callers see the heap alone)
        if self.nav.front != NO_FRONT { let f = std::mem::replace(&mut self.nav.front, NO_FRONT); self.nav.push_v(f); }
        if K {
            // (a goal reached by the very last expansion of a continued search)
            if resume && !found && !spent && self.g.pc[goal_p].stamp == fc.0 {
                self.nav_build(goal); self.nav.goal_cost = self.g.pc[goal_p].cost; self.nav.reached = self.g.idx(gx, gy, gz) as i32; found = true;
            }
            self.nav.exhausted = !found && self.nav.heap.is_empty();
            // (what was really expanded: the cell at which the budget ran out went back on the heap)
            let real = if count_spent { self.nav.expanded } else if spent { expanded_before.max(self.nav.expanded - 1) } else { self.nav.expanded };
            if count_spent { self.nav.expanded = k_budget + 1; }
            self.nav.kept[kk].expanded = real;
            self.nav.new_expanded = real - expanded_before;
            self.nav.total_expanded += (real - expanded_before) as i64;
            self.nav.last_budget = k_budget;
            self.nav.fast = false;
            return found;
        }
        self.nav.exhausted = !found && self.nav.heap.is_empty() && self.nav.expanded <= max_expand;
        self.nav.total_expanded += self.nav.expanded as i64;
        self.nav.went_once = once;
        self.nav.last_budget = max_expand;
        self.nav.once = false;
        self.nav.fast = false;
        found
    }

    fn ladder_moves(&mut self, i: i32, x: i32, y: i32, z: i32, c: i32) {
        if self.g.hang(x, y + 1, z) { let k = if self.g.rung(x, y + 1, z) == 3 { C_GATE } else { 0 }; self.relax(i, x, y + 1, z, c + C_CLIMB + k, LADDER); }
        let out = self.g.rung(x, y + 1, z) == 0 && self.g.passable(x, y + 1, z) && self.g.passable(x, y + 2, z);
        for d in 0..4usize {
            let (nx, nz) = (x + NDX[d], z + NDZ[d]);
            if self.doorway(x, y, z, d, nx, nz) == 0 {
                let hl = self.g.stand16(nx, y, nz);
                if (0..=8).contains(&hl) { self.relax(i, nx, y, nz, c + C_WALK, LADDER); }
            }
            if out && self.g.stand16(nx, y + 1, nz) == 0 && self.doorway(x, y + 1, z, d, nx, nz) == 0 { self.relax(i, nx, y + 1, nz, c + C_CLIMB + C_WALK, LADDER); }
        }
        if self.g.rung(x, y - 1, z) != 3 && self.g.hang(x, y - 1, z) { self.relax(i, x, y - 1, z, c + C_DESCEND, LADDER); return; }
        let mut ty = y - 1;
        while ty >= self.g.y0 {
            if self.g.stand16(x, ty, z) >= 0 { self.relax(i, x, ty, z, c + C_DROP + (y - ty) / 2, LADDER); break; }
            if self.g.rung(x, ty, z) != 3 && self.g.hang(x, ty, z) { self.relax(i, x, ty, z, c + C_DESCEND * (y - ty), LADDER); break; }
            if !self.g.passable(x, ty, z) { break; }
            ty -= 1;
        }
    }

    /// `ladder_moves` of the fast search: the same questions asked of the padded mirror (`p`: the cell's padded index).
    fn ladder_moves_f(&mut self, fc: (u16, bool, bool), i: i32, p: usize, x: i32, y: i32, z: i32, c: i32) {
        use super::grid::N_PASS;
        let l = self.g.pys;
        let pd: [isize; 4] = [self.g.pxs as isize, -(self.g.pxs as isize), self.g.pzs as isize, -(self.g.pzs as isize)];
        if self.g.p_hang_u(p + l) { let k = if self.g.p_rung_u(p + l) == 3 { C_GATE } else { 0 }; self.rx::<true>(fc, i, p + l, x, y + 1, z, c + C_CLIMB + k, LADDER); }
        let out = self.g.p_rung_u(p + l) == 0 && self.g.nvu(p + l) & N_PASS != 0 && self.g.nvu(p + 2 * l) & N_PASS != 0;
        for d in 0..4usize {
            let (nx, nz) = (x + NDX[d], z + NDZ[d]);
            let np = (p as isize + pd[d]) as usize;
            if self.doorway_p(p, np, d) == 0 {
                let hl = self.g.p_stand_u(np, nx, y, nz);
                if (0..=8).contains(&hl) { self.rx::<true>(fc, i, np, nx, y, nz, c + C_WALK, LADDER); }
            }
            if out && self.g.p_stand_u(np + l, nx, y + 1, nz) == 0 && self.doorway_p(p + l, np + l, d) == 0 { self.rx::<true>(fc, i, np + l, nx, y + 1, nz, c + C_CLIMB + C_WALK, LADDER); }
        }
        if self.g.p_rung_u(p - l) != 3 && self.g.p_hang_u(p - l) { self.rx::<true>(fc, i, p - l, x, y - 1, z, c + C_DESCEND, LADDER); return; }
        let mut ty = y - 1;
        let mut pt = p - l;
        while ty >= self.g.y0 {
            if self.g.p_stand_u(pt, x, ty, z) >= 0 { self.rx::<true>(fc, i, pt, x, ty, z, c + C_DROP + (y - ty) / 2, LADDER); break; }
            if self.g.p_rung_u(pt) != 3 && self.g.p_hang_u(pt) { self.rx::<true>(fc, i, pt, x, ty, z, c + C_DESCEND * (y - ty), LADDER); break; }
            if self.g.nvu(pt) & N_PASS == 0 { break; }
            ty -= 1;
            pt -= l;
        }
    }

    /// `doorway` from the padded mirror: `p` the lower cell of the column one is in, `np` that of the column beside it.
    #[inline(always)]
    fn doorway_p(&self, p: usize, np: usize, d: usize) -> i32 {
        use super::grid::*;
        let l = self.g.pys;
        let mut r = 0;
        for k in 0..4 {
            let s = self.g.nvu(match k { 0 => p, 1 => p + l, 2 => np, _ => np + l });
            let bit = if k < 2 { d } else { d ^ 1 } as u32;
            let blocks = (s >> (N_DOOR_SHIFT + bit)) & 1 != 0 || (k == 2 && s & N_CLOSED_GATE != 0);
            if !blocks { continue; }
            if s & N_HAND_DOOR == 0 { return -1; }
            r = 1;
        }
        r
    }

    /// 0 nothing between the cells, 1 a door leaf or closed gate that opens by hand, -1 one that does not.
    fn doorway(&self, x: i32, y: i32, z: i32, d: usize, nx: i32, nz: i32) -> i32 {
        let tb = t();
        let mut r = 0;
        for k in 0..4 {
            let s = if k < 2 { self.g.get(x, y + k, z) } else { self.g.get(nx, y + k - 2, nz) } as usize;
            let bit = if k < 2 { d } else { d ^ 1 };
            let blocks = (tb.door_edges[s] >> bit & 1) != 0 || (k == 2 && tb.closed_gate[s]);
            if !blocks { continue; }
            if !tb.hand_door[s] { return -1; }
            r = 1;
        }
        r
    }

    fn head_clear(&self, x: i32, y: i32, z: i32, h: i32) -> bool {
        if h + 29 <= 32 { return true; }
        let s2 = self.g.get(x, y + 2, z) as usize;
        t().walk_through[s2] || 32 + t().any_bottom[s2] as i32 >= h + 29
    }


    fn mine_through(&mut self, from: i32, x: i32, y: i32, z: i32, c: i32) -> bool {
        if !self.g.inside(x, y, z) { return false; }
        let below = self.g.get(x, y - 1, z) as usize;
        if t().col_top[below] != 16 || t().any_top[below] != 16 { return false; }
        let mut cost = 0;
        for yy in y..=y + 1 {
            if self.g.passable(x, yy, z) { continue; }
            let tk = self.mine_ticks(x, yy, z);
            if tk < 0 { return false; }
            cost += tk + C_MINE;
        }
        if cost == 0 { return false; }
        self.relax(from, x, y, z, c + C_WALK + cost, MINE);
        true
    }



    /// (candidate `walkway`) Through how many pieces put out sideways does the piece under search node `i` hang on a
    /// column that stands on something (the game's `distance` of that piece, real or still to be put)?
    fn wing_dist(&self, mut i: i32) -> i32 {
        let fast = self.nav.fast;
        let mut d = 0;
        loop {
            let (x, y, z) = if fast { self.g.unpk(i) } else { (self.g.x_of(i as usize), self.g.y_of(i as usize), self.g.z_of(i as usize)) };
            let (par, how) = if fast { let c = &self.g.pc[self.g.pidx(x, y, z)]; (c.parent, c.how) } else { (self.nav.nodes[i as usize].parent, self.nav.nodes[i as usize].how) };
            if par >= 0 && how == WING { d += 1; i = par; continue; }
            if par >= 0 && how == TOWER { i = par; continue; }
            let b = self.g.get(x, y - 1, z);
            if t().scaffolding[b as usize] { d += val(b, crate::props::P_DISTANCE) as i32; }
            return d;
        }
    }

    fn nav_build(&mut self, goal: usize) {
        let fast = self.nav.fast;
        let xyz = |b: &Bot, i: usize| -> (i32, i32, i32) { if fast { b.g.unpk(i as i32) } else { (b.g.x_of(i), b.g.y_of(i), b.g.z_of(i)) } };
        let node_of = |b: &Bot, i: usize| -> (i32, u8) {
            if fast { let (x, y, z) = b.g.unpk(i as i32); let c = &b.g.pc[b.g.pidx(x, y, z)]; (c.parent, c.how) } else { (b.nav.nodes[i].parent, b.nav.nodes[i].how) }
        };
        let mut n = 0;
        let mut i = goal as i32;
        while i >= 0 { n += 1; i = node_of(self, i as usize).0; }
        self.nav.path.resize(n * 3, 0);
        self.nav.mv.resize(n, 0);
        self.nav.length = n;
        let mut k = n;
        let mut i = goal as i32;
        while i >= 0 {
            k -= 1;
            let iu = i as usize;
            let (px, py, pz) = xyz(self, iu);
            self.nav.path[k * 3] = px; self.nav.path[k * 3 + 1] = py; self.nav.path[k * 3 + 2] = pz;
            let (par, how) = node_of(self, iu);
            self.nav.mv[k] = how;
            if how == PILLAR || how == BRIDGE { self.nav.scaffold_moves += 1; }
            if how == TOWER || how == WING { self.nav.tower_moves += 1; }
            i = par;
        }
    }
}
