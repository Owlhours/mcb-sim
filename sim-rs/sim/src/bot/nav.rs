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

pub struct Nav {
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

impl Nav {
    pub fn new(n: usize) -> Nav {
        Nav {
            mine: false, effort: 1, blocked_edges: FiltSet::default(), nodes: vec![Node::default(); n], epoch: 0, fast: false, epoch16: 0, touch_limit: 0, touched: 0, heap: Vec::with_capacity(1024), front: NO_FRONT,
            scaffold_penalty: 40, tower_penalty: 3, perch_penalty: 0, towers_on: true, extra_effort: 0, expanded: 0, exhausted: false, stop_at: false, reached: -1, total_expanded: 0, searches: 0, search_ns: 0,
            virtual_stand: false, probe: false, only_found: false, start_h: -1, path: Vec::new(), mv: Vec::new(), length: 0, goal_cost: 0, scaffold_moves: 0, tower_moves: 0, tx: 0, ty: 0, tz: 0,
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
        let dy = self.ty - y;
        (if self.mine { 5 } else { 1 }) * (2 * ((self.tx - x).abs() + (self.tz - z).abs()) * C_WALK
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
pub fn dump_callstat() { CALLSTAT.with(|m| { for ((a, b), (n, e, ns)) in m.borrow().iter() { eprintln!("callstat {a:50} {b:9} searches={n:9} expanded={e:11} ms={}", ns / 1000000); } }); }
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
fn how_bit(h: u8) -> u8 { if h == DIG || h == CLEAR || h == MINE { 2 } else if h == PILLAR || h == BRIDGE { 4 } else if h == TOWER { 8 } else { 1 } }
/// `how_bit` as a table (WALK ... LADDER)
const HOW_BIT: [u8; 16] = [1, 1, 1, 4, 4, 2, 2, 2, 1, 1, 8, 1, 1, 1, 1, 1];

const NDX: [i32; 4] = [1, -1, 0, 0];
const NDZ: [i32; 4] = [0, 0, 1, -1];

/// A block one can put another block on top of and stand on: full, level top face.
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
            if cell.stamp == e && cell.cost <= c { self.g.pc[pi].pad |= how_bit(h); return; }
            self.relax_new(from, pi, x, y, z, c, h);
            return;
        }
        let i = self.g.idx(x, y, z);
        let nd = &self.nav.nodes[i];
        if nd.stamp == self.nav.epoch && nd.cost <= c { return; }
        if !self.nav.blocked_edges.is_empty() && self.nav.blocked_edges.contains_plain(&edge(from, i as i32)) { return; }
        self.nav.nodes[i] = Node { stamp: self.nav.epoch, cost: c, parent: from, how: h };
        let key = (c + self.nav.heuristic(x, y, z)) as i64;
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
        let key = (c + self.nav.heuristic(x, y, z)) as i64;
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
            if cell.stamp == fc.0 && cell.cost <= c { if fc.1 { cell.pad |= HOW_BIT[h as usize & 15]; } return; }
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
        let dry = self.nav.only_found && super::dry::on() && self.g.pk_ok && !scaffold_rule && !self.nav.mine && (self.towers_off || !self.have_tower || self.no_scaffold_nav)
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
            } else if !r && self.nav.exhausted { self.dry_store(sx, sy, sz); }
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
    pub(super) fn nav_search_g<const F: bool>(&mut self, sx: i32, sy: i32, sz: i32, gx: i32, gy: i32, gz: i32, max_expand: i32, scaffold_rule: bool) -> bool {
        use super::grid::*;
        // (a padded index that is never read: the checking form passes it around untouched)
        const NOWHERE: usize = 1 << 40;

        // (probe: dry.rs has filled the heap with (way of standing, cell); nothing of the last search is touched)
        let probe = F && self.nav.probe;
        let rv = crate::rev::rev();
        let tb = t();
        if !probe {
        self.nav.epoch += 1;
        self.nav.heap.clear();
        self.nav.expanded = 0;
        self.nav.length = 0;
        self.nav.searches += 1;
        self.nav.exhausted = false;
        self.nav.reached = -1;
        self.nav.tx = gx; self.nav.ty = gy; self.nav.tz = gz;
        if !self.g.inside(sx, sy, sz) || !self.g.inside(gx, gy, gz) { return false; }
        }
        let max_expand = max_expand.min(self.nav.effort * (400 + 80 * ((gx - sx).abs() + (gz - sz).abs() + 3 * (gy - sy).abs())) + self.nav.extra_effort);
        // MCSIM_LOSSY=k (not faithful; for the speed/fidelity trade-off only): searches give up after 1/k of their budget
        let max_expand = if F && max_expand > 96 && lossy() > 1 { (max_expand / lossy()).max(97) } else { max_expand };
        let (start, goal) = if probe { (NOWHERE, NOWHERE) } else if F { (self.g.pk(sx, sy, sz) as usize, self.g.pk(gx, gy, gz) as usize) } else { (self.g.idx(sx, sy, sz), self.g.idx(gx, gy, gz)) };
        if !probe {
        self.nav.scaffold_moves = 0;
        self.nav.tower_moves = 0;
        self.nav.fast = F;
        if F {
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
        let mut found = false;
        while self.nav.front != NO_FRONT || !self.nav.heap.is_empty() {
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
            let hx = if F {
                // (`Nav::heuristic`)
                let dy = gy - y;
                h_mul * (2 * ((gx - x).abs() + (gz - z).abs()) * C_WALK + if dy > 0 { dy * h_up } else { -dy * 2 })
            } else { self.nav.heuristic(x, y, z) };
            debug_assert!(hx == self.nav.heuristic(x, y, z));
            if (top >> 32) as i32 != c + hx { continue; }   // stale entry
            if i == goal || (if F { stop_at && self.exit_nodes.contains(&(self.g.idx(x, y, z) as i32)) } else { self.nav.stop_at && self.exit_nodes.contains_plain(&(self.g.idx(x, y, z) as i32)) }) { self.nav_build(i); self.nav.goal_cost = c; self.nav.reached = self.g.idx(x, y, z) as i32; found = true; break; }
            if self.nav.touched >= touch_limit { self.nav.total_expanded -= (max_expand + 1 - self.nav.expanded) as i64; self.nav.expanded = max_expand + 1; break; }
            self.nav.expanded += 1;
            if self.nav.expanded > max_expand { break; }
            }
            let how_i = if probe { (top >> 32) as u8 } else if F { let cell = unsafe { self.g.pc.get_unchecked_mut(p) }; if fc.1 { cell.pad |= HOW_BIT[cell.how as usize & 15] << 4; } cell.how } else { self.nav.nodes[i].how };
            let on_tower = how_i == TOWER;
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
                    }
                }
            }
            if sc_on && h == 0 && head_up && here & N_AIR != 0 && up1 & N_PASS != 0 && (virtual_stand || dug || below & N_FLAT_TOP != 0)
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
                let head2 = up2 & N_PASS != 0;
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
        self.nav.exhausted = !found && self.nav.heap.is_empty() && self.nav.expanded <= max_expand;
        self.nav.total_expanded += self.nav.expanded as i64;
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
            if how == TOWER { self.nav.tower_moves += 1; }
            i = par;
        }
    }
}
