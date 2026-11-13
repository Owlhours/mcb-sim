//! Route finding (the Java version's `Nav`): weighted A* over the cells the player can stand in.
use super::state::Bot;
use crate::tables::*;
use crate::fasthash::FastSet;

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
    pub blocked_edges: FastSet<i64>,
    pub nodes: Vec<Node>,
    pub epoch: i32,
    /// the fast search keeps its nodes in the grid's padded cells, stamped with this (16 bit) epoch
    pub fast: bool,
    pub epoch16: u16,
    pub heap: Vec<i64>,
    pub scaffold_penalty: i32,
    pub tower_penalty: i32,
    pub perch_penalty: i32,
    pub towers_on: bool,
    pub extra_effort: i32,
    pub expanded: i32,
    pub total_expanded: i64,
    pub searches: i64,
    /// wall time spent searching (nanoseconds), for the speed breakdown
    pub search_ns: u64,
    pub virtual_stand: bool,
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
            mine: false, effort: 1, blocked_edges: FastSet::default(), nodes: vec![Node::default(); n], epoch: 0, fast: false, epoch16: 0, heap: Vec::with_capacity(1024),
            scaffold_penalty: 40, tower_penalty: 3, perch_penalty: 0, towers_on: true, extra_effort: 0, expanded: 0, total_expanded: 0, searches: 0, search_ns: 0,
            virtual_stand: false, start_h: -1, path: Vec::new(), mv: Vec::new(), length: 0, goal_cost: 0, scaffold_moves: 0, tower_moves: 0, tx: 0, ty: 0, tz: 0,
        }
    }

    #[inline]
    fn push(&mut self, key: i64, node: i32) {
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

    #[inline]
    fn heuristic(&self, x: i32, y: i32, z: i32) -> i32 {
        let dy = self.ty - y;
        (if self.mine { 5 } else { 1 }) * (2 * ((self.tx - x).abs() + (self.tz - z).abs()) * C_WALK
            + if dy > 0 { dy * if self.towers_on { C_TOWER + self.tower_penalty } else { C_PILLAR + self.scaffold_penalty } } else { -dy * 2 })
    }
}

thread_local! { pub static NAVSTAT: std::cell::RefCell<[[(u64, u64); 24]; 3]> = const { std::cell::RefCell::new([[(0, 0); 24]; 3]) }; }

thread_local! { pub static MEMOSTAT: std::cell::RefCell<(crate::fasthash::FastSet<u64>, u64, u64, u64, u64)> = std::cell::RefCell::new((Default::default(), 0, 0, 0, 0)); }

thread_local! { pub static CERTSTAT: std::cell::RefCell<(u64, u64, u64, u64, u64)> = const { std::cell::RefCell::new((0, 0, 0, 0, 0)) }; }

/// Diagnostic switches read once from the environment: 0 MCSIM_NAVSTAT, 1 MCSIM_MEMOSTAT, 2 MCSIM_CERTSTAT.
#[inline]
fn envflag(k: usize) -> bool {
    static V: std::sync::OnceLock<[bool; 3]> = std::sync::OnceLock::new();
    V.get_or_init(|| [std::env::var_os("MCSIM_NAVSTAT").is_some(), std::env::var_os("MCSIM_MEMOSTAT").is_some(), std::env::var_os("MCSIM_CERTSTAT").is_some()])[k]
}

fn navcheck() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCSIM_NAVCHECK").is_some())
}

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
        let i = self.g.idx(x, y, z);
        if self.nav.fast {
            let pi = self.g.pidx(x, y, z);
            let e = self.nav.epoch16;
            let cell = &self.g.pc[pi];
            if cell.stamp == e && cell.cost <= c { return; }
            if !self.nav.blocked_edges.is_empty() && self.nav.blocked_edges.contains(&edge(from, i as i32)) { return; }
            let cell = &mut self.g.pc[pi];
            cell.stamp = e; cell.cost = c; cell.parent = from; cell.how = h;
            let key = (c + self.nav.heuristic(x, y, z)) as i64;
            self.nav.push(key, i as i32);
            return;
        }
        let nd = &self.nav.nodes[i];
        if nd.stamp == self.nav.epoch && nd.cost <= c { return; }
        if !self.nav.blocked_edges.is_empty() && self.nav.blocked_edges.contains(&edge(from, i as i32)) { return; }
        self.nav.nodes[i] = Node { stamp: self.nav.epoch, cost: c, parent: from, how: h };
        let key = (c + self.nav.heuristic(x, y, z)) as i64;
        self.nav.push(key, i as i32);
    }

    #[inline]
    fn in_scaffold(&self, x: i32, y: i32, z: i32) -> bool { self.g.inside(x, y, z) && self.scaffold.contains(self.g.idx(x, y, z) as i32) }

    /// Cheapest route from the start cell to the goal cell. `scaffold_rule`: whether loose blocks may be used
    /// (pillar/bridge moves, where `scaffold_ok` allows). Gives up after max_expand cells.
    pub fn nav_search(&mut self, sx: i32, sy: i32, sz: i32, gx: i32, gy: i32, gz: i32, max_expand: i32, scaffold_rule: bool) -> bool {
        let t0 = std::time::Instant::now();
        let r = self.nav_search_impl(sx, sy, sz, gx, gy, gz, max_expand, scaffold_rule);
        if envflag(2) && !r && !self.nav.heap.is_empty() && !self.nav.mine && self.nav.expanded > 200 {
            // would a backward flood from the goal have proved it unreachable? (measurement only)
            let cap = 3000usize;
            let (n, hit) = self.backward_region(gx, gy, gz, sx, sy, sz, cap);
            CERTSTAT.with(|c| { let mut c = c.borrow_mut(); c.0 += 1; c.1 += self.nav.expanded as u64; if n < cap && !hit { c.2 += 1; c.3 += self.nav.expanded as u64; c.4 += n as u64; } });
        }
        if navcheck() {
            // run the reference search on the same inputs and compare everything a caller can see
            let (e, len, gc, sm, tm) = (self.nav.expanded, self.nav.length, self.nav.goal_cost, self.nav.scaffold_moves, self.nav.tower_moves);
            let (path, mv) = (self.nav.path[..len * 3].to_vec(), self.nav.mv[..len].to_vec());
            let (se, te) = (self.nav.searches, self.nav.total_expanded);
            let r2 = self.nav_search_ref(sx, sy, sz, gx, gy, gz, max_expand, scaffold_rule);
            let same = r == r2 && e == self.nav.expanded && len == self.nav.length && (!r || (gc == self.nav.goal_cost && sm == self.nav.scaffold_moves && tm == self.nav.tower_moves && path[..] == self.nav.path[..len * 3] && mv[..] == self.nav.mv[..len]));
            if !same { panic!("route search mismatch: ({sx},{sy},{sz}) -> ({gx},{gy},{gz}) budget {max_expand}: {r}/{e}/{len}/{gc} vs {r2}/{}/{}/{}", self.nav.expanded, self.nav.length, self.nav.goal_cost); }
            self.nav.searches = se; self.nav.total_expanded = te;
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
        self.nav.search_ns += t0.elapsed().as_nanos() as u64;
        r
    }

    /// The search over the padded per-cell mirror (`Grid::pc`): the same moves, costs and order as `nav_search_ref`.
    fn nav_search_impl(&mut self, sx: i32, sy: i32, sz: i32, gx: i32, gy: i32, gz: i32, max_expand: i32, scaffold_rule: bool) -> bool {
        use super::grid::*;
        self.nav.epoch += 1;
        self.nav.heap.clear();
        self.nav.expanded = 0;
        self.nav.length = 0;
        self.nav.searches += 1;
        self.nav.tx = gx; self.nav.ty = gy; self.nav.tz = gz;
        if !self.g.inside(sx, sy, sz) || !self.g.inside(gx, gy, gz) { return false; }
        let max_expand = max_expand.min(self.nav.effort * (400 + 80 * ((gx - sx).abs() + (gz - sz).abs() + 3 * (gy - sy).abs())) + self.nav.extra_effort);
        self.nav.scaffold_moves = 0;
        self.nav.tower_moves = 0;
        let start = self.g.idx(sx, sy, sz);
        let goal = self.g.idx(gx, gy, gz);
        self.nav.fast = true;
        self.nav.epoch16 = self.nav.epoch16.wrapping_add(1);
        if self.nav.epoch16 == 0 { for c in self.g.pc.iter_mut() { c.stamp = 0; } self.nav.epoch16 = 1; }
        {
            let ps = self.g.pidx(sx, sy, sz);
            let cell = &mut self.g.pc[ps];
            cell.stamp = self.nav.epoch16; cell.cost = 0; cell.parent = -1; cell.how = WALK;
        }
        let hk = self.nav.heuristic(sx, sy, sz) as i64;
        self.nav.push(hk, start as i32);
        let y0 = self.g.y0;
        let oy = self.g.oy;
        let mine = self.nav.mine;
        let l = self.g.pys;
        let pd: [isize; 4] = [self.g.pxs as isize, -(self.g.pxs as isize), self.g.pzs as isize, -(self.g.pzs as isize)];
        // the parts of scaffold_ok / tower_ok that do not depend on the cell
        let sc_on = scaffold_rule && self.have_scaffold && !self.no_scaffold_nav;
        let tw_on = self.have_tower && !self.no_scaffold_nav && !self.towers_off;
        let door_bits = N_DOOR_MASK | N_CLOSED_GATE;
        while !self.nav.heap.is_empty() {
            let top = self.nav.pop();
            let i = (top as i32) as usize;
            let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
            let p = self.g.pidx(x, y, z);
            let c = self.g.pc[p].cost;
            if (top >> 32) as i32 != c + self.nav.heuristic(x, y, z) { continue; }
            if i == goal { self.nav_build(i); self.nav.goal_cost = c; self.nav.total_expanded += self.nav.expanded as i64; self.nav.fast = false; return true; }
            self.nav.expanded += 1;
            if self.nav.expanded > max_expand { break; }
            let how_i = self.g.pc[p].how;
            let on_tower = how_i == TOWER;
            let virtual_stand = how_i == PILLAR || how_i == BRIDGE || on_tower;
            self.nav.virtual_stand = virtual_stand;
            let dug = how_i == DIG || how_i == CLEAR || how_i == MINE;
            let mut h = if virtual_stand || dug { 0 } else { self.g.p_stand(p, x, y, z) };
            if i == start && h < 0 && self.nav.start_h >= 0 { h = self.nav.start_h; }
            let ii = i as i32;
            if h < 0 && self.g.p_hang(p) { self.ladder_moves(ii, x, y, z, c); continue; }
            if h < 0 {
                let mut ty = y - 1;
                let mut pt = p - l;
                while ty >= y0 {
                    if self.g.p_stand(pt, x, ty, z) >= 0 { self.relax(ii, x, ty, z, c + C_DROP, DROP); break; }
                    if self.g.nv(pt) & N_PASS == 0 { break; }
                    ty -= 1;
                    pt -= l;
                }
                continue;
            }
            let below = self.g.nv(p - l);
            let here = self.g.nv(p);
            let up2 = self.g.nv(p + 2 * l);
            let head_up = dug || up2 & N_PASS != 0;
            let floor_tower = on_tower || below & N_TOWER != 0;
            let on_scaffold = !virtual_stand && !floor_tower && h == 0 && below & N_SCAF != 0;
            let leave = if on_scaffold { C_ABANDON } else { 0 };
            let up1 = self.g.nv(p + l);
            for d in 0..4usize {
                let (nx, nz) = (x + NDX[d], z + NDZ[d]);
                let np = (p as isize + pd[d]) as usize;
                let n0 = self.g.nv(np);
                let n1 = self.g.nv(np + l);
                // doors and gates between the two columns
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
                            let floor = self.g.nv(np - l);
                            let stand = if n0 & N_CLOSED_GATE != 0 { floor & N_FLOOR16 != 0 && n1 & N_PASS != 0 } else { self.g.p_stand(np, nx, y, nz) == 0 };
                            if stand { self.relax(ii, nx, y, nz, c + C_WALK + C_GATE, GATE); }
                        }
                        continue;
                    }
                }
                if h <= 8 && self.g.p_hang(np) { let k = if self.g.p_rung(np) == 3 { C_GATE } else { 0 }; self.relax(ii, nx, y, nz, c + C_WALK + k, LADDER); continue; }
                let hu = if head_up { self.g.p_stand(np + l, nx, y + 1, nz) } else { -1 };
                let hl = self.g.p_stand(np, nx, y, nz);
                if h == 0 && hl == 0 && self.g.p_rung(np - l) == 3 { self.relax(ii, nx, y - 1, nz, c + C_WALK + C_GATE + C_DESCEND, LADDER); }
                if hl >= 0 {
                    let rise = hl - h;
                    if rise <= 9 { if if rise <= 0 { self.head_clear(nx, y, nz, h) } else { hl + 29 <= 32 || head_up } { self.relax(ii, nx, y, nz, c + C_WALK, WALK); } }
                    else if rise <= 19 && head_up { self.relax(ii, nx, y, nz, c + C_JUMP, JUMP); }
                } else if hu >= 0 {
                    let rise = 16 + hu - h;
                    if rise <= 9 { self.relax(ii, nx, y + 1, nz, c + C_WALK, WALK); }
                    else if rise <= 19 { self.relax(ii, nx, y + 1, nz, c + C_JUMP, JUMP); }
                } else {
                    // own scaffolding in the way on level ground?
                    let s0 = n0 & (N_SCAF | N_TOWER) == N_SCAF;
                    let s1 = n1 & (N_SCAF | N_TOWER) == N_SCAF;
                    let clearable = h == 0 && (s0 || s1) && (s0 || n0 & N_PASS != 0) && (s1 || n1 & N_PASS != 0) && self.g.nv(np - l) & N_FLOOR16 != 0;
                    if clearable {
                        let n = (s0 as i32) + (s1 as i32);
                        self.relax(ii, nx, y, nz, c + C_WALK + n * C_CLEAR, CLEAR);
                    } else if mine && self.mine_through(ii, nx, y, nz, c) {
                        // (relaxed inside)
                    } else if n0 & N_PASS != 0 && n1 & N_PASS != 0 && self.head_clear(nx, y, nz, h) {
                        let mut step_down = false;
                        let mut ty = y - 1;
                        let mut pt = np - l;
                        while ty >= y0 {
                            let hd = self.g.p_stand(pt, nx, ty, nz);
                            if hd >= 0 {
                                let fall = (y - ty) * 16 + h - hd;
                                let cc = c + if fall <= 9 { C_WALK } else { C_DROP + (y - ty) / 2 + leave + if hd != 0 { C_PIT } else { 0 } };
                                self.relax(ii, nx, ty, nz, cc, if fall <= 9 { WALK } else { DROP });
                                step_down = fall <= 10;
                                break;
                            }
                            if self.g.p_hang(pt) { self.relax(ii, nx, ty, nz, c + C_WALK + C_DESCEND * (y - ty), LADDER); break; }
                            if self.g.nv(pt) & N_PASS == 0 { break; }
                            ty -= 1;
                            pt -= l;
                        }
                        if sc_on && h == 0 && !step_down && !floor_tower && (virtual_stand || below & N_OUT_FULL != 0) {
                            let q = self.g.nv(np - l);
                            if q & N_AIR != 0 && (q & N_OPEN != 0 || self.scaffold_ok(nx, y - 1, nz)) {
                                let sp = self.nav.scaffold_penalty;
                                self.relax(ii, nx, y, nz, c + C_BRIDGE + sp, BRIDGE);
                            }
                        }
                    }
                }
            }
            if sc_on && h == 0 && head_up && here & N_AIR != 0 && up1 & N_PASS != 0 && (virtual_stand || dug || below & N_FLAT_TOP != 0)
                && (here & N_OPEN != 0 || self.scaffold_ok(x, y, z)) {
                let sp = self.nav.scaffold_penalty;
                self.relax(ii, x, y + 1, z, c + C_PILLAR + sp, PILLAR);
            }
            // last resort: break the block of the build underfoot and drop to whatever is below
            if mine && !virtual_stand && !floor_tower && h == 0 && self.g.inside(x, y - 1, z) && below & N_SCAF == 0 {
                let tk = self.mine_ticks(x, y - 1, z);
                if tk > 0 {
                    let mut ty = y - 2;
                    while ty >= y0 - 1 {
                        let bl = self.g.nv(self.g.pidx(x, ty, z));
                        if bl & N_COL16 != 0 { self.relax(ii, x, ty + 1, z, c + C_DROP + tk + C_MINE, MINE); break; }
                        if bl & N_COLNZ != 0 { break; }
                        ty -= 1;
                    }
                }
            }
            if mine && !virtual_stand && h > 0 && h < 16 {
                let tk = self.mine_ticks(x, y, z);
                if tk > 0 {
                    let mut ty = y - 1;
                    while ty >= y0 - 1 {
                        let bl = self.g.nv(self.g.pidx(x, ty, z));
                        if bl & N_COL16 != 0 { if ty + 1 < y { self.relax(ii, x, ty + 1, z, c + C_DROP + tk + C_MINE, MINE); } break; }
                        if bl & N_COLNZ != 0 { break; }
                        ty -= 1;
                    }
                }
            }
            // take down the scaffold block underfoot and sink
            if !virtual_stand && !floor_tower && h == 0 && below & N_SCAF != 0 {
                let mut ty = y - 1;
                let mut pt = p - l;
                while ty >= y0 {
                    if ty < y - 1 && self.g.nv(pt) & N_PASS == 0 { break; }
                    let bl = self.g.nv(pt - l);
                    if bl & N_COL16 != 0 { self.relax(ii, x, ty, z, c + C_DIG + (y - 1 - ty) / 2, DIG); break; }
                    if bl & N_COLNZ != 0 { break; }
                    ty -= 1;
                    pt -= l;
                }
            }
            // scaffolding columns: rise inside one, sink through one, or put one up / make one taller
            if h == 0 {
                let s_here = !virtual_stand && here & N_TOWER != 0;
                let head2 = up2 & N_PASS != 0;
                if s_here && head2 { self.relax(ii, x, y + 1, z, c + C_CLIMB, CLIMB); }
                if !virtual_stand && !dug && below & N_TOWER != 0 && self.g.p_stand(p - l, x, y - 1, z) >= 0 { self.relax(ii, x, y - 1, z, c + C_DESCEND, DESCEND); }
                if !s_here && !dug && head2 && here & N_AIR != 0 {
                    let base = on_tower || below & N_TOWER != 0 || (below & N_STURDY_TOP != 0 && below & N_SCAF == 0);
                    if base && ((tw_on && here & N_OPEN != 0) || (here & N_OPEN == 0 && self.tower_ok(x, y, z))) {
                        let perched = !on_tower && y > oy && below & N_TOWER == 0;
                        let cc = c + C_TOWER + self.nav.tower_penalty + if perched { self.nav.perch_penalty } else { 0 };
                        self.relax(ii, x, y + 1, z, cc, TOWER);
                    }
                }
            }
        }
        self.nav.total_expanded += self.nav.expanded as i64;
        self.nav.fast = false;
        false
    }

    /// The search as first ported (reads the grid through `Grid::get`); kept as the reference for `MCSIM_NAVCHECK`.
    fn nav_search_ref(&mut self, sx: i32, sy: i32, sz: i32, gx: i32, gy: i32, gz: i32, max_expand: i32, scaffold_rule: bool) -> bool {
        let tb = t();
        self.nav.epoch += 1;
        self.nav.heap.clear();
        self.nav.expanded = 0;
        self.nav.length = 0;
        self.nav.searches += 1;
        self.nav.tx = gx; self.nav.ty = gy; self.nav.tz = gz;
        if !self.g.inside(sx, sy, sz) || !self.g.inside(gx, gy, gz) { return false; }
        let max_expand = max_expand.min(self.nav.effort * (400 + 80 * ((gx - sx).abs() + (gz - sz).abs() + 3 * (gy - sy).abs())) + self.nav.extra_effort);
        self.nav.scaffold_moves = 0;
        self.nav.tower_moves = 0;
        let start = self.g.idx(sx, sy, sz);
        let goal = self.g.idx(gx, gy, gz);
        self.nav.nodes[start] = Node { stamp: self.nav.epoch, cost: 0, parent: -1, how: WALK };
        let hk = self.nav.heuristic(sx, sy, sz) as i64;
        self.nav.push(hk, start as i32);
        let y0 = self.g.y0;
        let mine = self.nav.mine;
        while !self.nav.heap.is_empty() {
            let top = self.nav.pop();
            let i = (top as i32) as usize;
            let c = self.nav.nodes[i].cost;
            let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
            if (top >> 32) as i32 != c + self.nav.heuristic(x, y, z) { continue; }
            if i == goal { self.nav_build(i); self.nav.goal_cost = c; self.nav.total_expanded += self.nav.expanded as i64; return true; }
            self.nav.expanded += 1;
            if self.nav.expanded > max_expand { break; }
            let how_i = self.nav.nodes[i].how;
            let on_tower = how_i == TOWER;
            let virtual_stand = how_i == PILLAR || how_i == BRIDGE || on_tower;
            self.nav.virtual_stand = virtual_stand;
            let dug = how_i == DIG || how_i == CLEAR || how_i == MINE;
            let mut h = if virtual_stand || dug { 0 } else { self.g.stand16(x, y, z) };
            if i == start && h < 0 && self.nav.start_h >= 0 { h = self.nav.start_h; }
            let ii = i as i32;
            if h < 0 && self.g.hang(x, y, z) { self.ladder_moves(ii, x, y, z, c); continue; }
            if h < 0 {
                let mut ty = y - 1;
                while ty >= y0 {
                    if self.g.stand16(x, ty, z) >= 0 { self.relax(ii, x, ty, z, c + C_DROP, DROP); break; }
                    if !self.g.passable(x, ty, z) { break; }
                    ty -= 1;
                }
                continue;
            }
            let mut head_up = self.g.passable(x, y + 2, z);
            if dug { head_up = true; }
            let floor_tower = on_tower || self.g.is_tower(x, y - 1, z);
            let on_scaffold = !virtual_stand && !floor_tower && h == 0 && self.in_scaffold(x, y - 1, z);
            let leave = if on_scaffold { C_ABANDON } else { 0 };
            for d in 0..4usize {
                let (nx, nz) = (x + NDX[d], z + NDZ[d]);
                let door = self.doorway(x, y, z, d, nx, nz);
                if door != 0 {
                    if door > 0 && h == 0 && !virtual_stand {
                        let nb = self.g.get(nx, y, nz) as usize;
                        let floor = self.g.get(nx, y - 1, nz) as usize;
                        let stand = if tb.closed_gate[nb] { tb.col_top[floor] == 16 && tb.any_top[floor] == 16 && self.g.passable(nx, y + 1, nz) } else { self.g.stand16(nx, y, nz) == 0 };
                        if stand { self.relax(ii, nx, y, nz, c + C_WALK + C_GATE, GATE); }
                    }
                    continue;
                }
                if h <= 8 && self.g.hang(nx, y, nz) { let k = if self.g.rung(nx, y, nz) == 3 { C_GATE } else { 0 }; self.relax(ii, nx, y, nz, c + C_WALK + k, LADDER); continue; }
                let hu = if head_up { self.g.stand16(nx, y + 1, nz) } else { -1 };
                let hl = self.g.stand16(nx, y, nz);
                if h == 0 && hl == 0 && self.g.rung(nx, y - 1, nz) == 3 { self.relax(ii, nx, y - 1, nz, c + C_WALK + C_GATE + C_DESCEND, LADDER); }
                if hl >= 0 {
                    let rise = hl - h;
                    if rise <= 9 { if if rise <= 0 { self.head_clear(nx, y, nz, h) } else { hl + 29 <= 32 || head_up } { self.relax(ii, nx, y, nz, c + C_WALK, WALK); } }
                    else if rise <= 19 && head_up { self.relax(ii, nx, y, nz, c + C_JUMP, JUMP); }
                } else if hu >= 0 {
                    let rise = 16 + hu - h;
                    if rise <= 9 { self.relax(ii, nx, y + 1, nz, c + C_WALK, WALK); }
                    else if rise <= 19 { self.relax(ii, nx, y + 1, nz, c + C_JUMP, JUMP); }
                } else if h == 0 && self.clearable(nx, y, nz) {
                    let n = (self.is_scaffold_block(nx, y, nz) as i32) + (self.is_scaffold_block(nx, y + 1, nz) as i32);
                    self.relax(ii, nx, y, nz, c + C_WALK + n * C_CLEAR, CLEAR);
                } else if mine && self.mine_through(ii, nx, y, nz, c) {
                    // (relaxed inside)
                } else if self.g.passable(nx, y, nz) && self.g.passable(nx, y + 1, nz) && self.head_clear(nx, y, nz, h) {
                    let mut step_down = false;
                    let mut ty = y - 1;
                    while ty >= y0 {
                        let hd = self.g.stand16(nx, ty, nz);
                        if hd >= 0 {
                            let fall = (y - ty) * 16 + h - hd;
                            let cc = c + if fall <= 9 { C_WALK } else { C_DROP + (y - ty) / 2 + leave + if hd != 0 { C_PIT } else { 0 } };
                            self.relax(ii, nx, ty, nz, cc, if fall <= 9 { WALK } else { DROP });
                            step_down = fall <= 10;
                            break;
                        }
                        if self.g.hang(nx, ty, nz) { self.relax(ii, nx, ty, nz, c + C_WALK + C_DESCEND * (y - ty), LADDER); break; }
                        if !self.g.passable(nx, ty, nz) { break; }
                        ty -= 1;
                    }
                    if scaffold_rule && h == 0 && !step_down && !floor_tower
                        && (virtual_stand || tb.out_full[self.g.get(x, y - 1, z) as usize]) && self.g.is_air(nx, y - 1, nz) && self.scaffold_ok(nx, y - 1, nz) {
                        let sp = self.nav.scaffold_penalty;
                        self.relax(ii, nx, y, nz, c + C_BRIDGE + sp, BRIDGE);
                    }
                }
            }
            if scaffold_rule && h == 0 && head_up && self.g.is_air(x, y, z) && self.g.passable(x, y + 1, z) && self.scaffold_ok(x, y, z)
                && (virtual_stand || dug || flat_top(self.g.get(x, y - 1, z))) {
                let sp = self.nav.scaffold_penalty;
                self.relax(ii, x, y + 1, z, c + C_PILLAR + sp, PILLAR);
            }
            // last resort: break the block of the build underfoot and drop to whatever is below
            if mine && !virtual_stand && !floor_tower && h == 0 && self.g.inside(x, y - 1, z) && !self.scaffold.contains(self.g.idx(x, y - 1, z) as i32) {
                let tk = self.mine_ticks(x, y - 1, z);
                if tk > 0 {
                    let mut ty = y - 2;
                    while ty >= y0 - 1 {
                        let below = self.g.get(x, ty, z) as usize;
                        if tb.col_top[below] >= 16 { self.relax(ii, x, ty + 1, z, c + C_DROP + tk + C_MINE, MINE); break; }
                        if tb.col_top[below] != 0 { break; }
                        ty -= 1;
                    }
                }
            }
            if mine && !virtual_stand && h > 0 && h < 16 && self.g.inside(x, y, z) {
                let tk = self.mine_ticks(x, y, z);
                if tk > 0 {
                    let mut ty = y - 1;
                    while ty >= y0 - 1 {
                        let below = self.g.get(x, ty, z) as usize;
                        if tb.col_top[below] >= 16 { if ty + 1 < y { self.relax(ii, x, ty + 1, z, c + C_DROP + tk + C_MINE, MINE); } break; }
                        if tb.col_top[below] != 0 { break; }
                        ty -= 1;
                    }
                }
            }
            // take down the scaffold block underfoot and sink
            if !virtual_stand && !floor_tower && h == 0 && self.in_scaffold(x, y - 1, z) {
                let mut ty = y - 1;
                while ty >= y0 {
                    if ty < y - 1 && !self.g.passable(x, ty, z) { break; }
                    let below = self.g.get(x, ty - 1, z) as usize;
                    if tb.col_top[below] >= 16 { self.relax(ii, x, ty, z, c + C_DIG + (y - 1 - ty) / 2, DIG); break; }
                    if tb.col_top[below] != 0 { break; }
                    ty -= 1;
                }
            }
            // scaffolding columns: rise inside one, sink through one, or put one up / make one taller
            if h == 0 {
                let s_here = !virtual_stand && self.g.is_tower(x, y, z);
                let head2 = self.g.passable(x, y + 2, z);
                if s_here && head2 { self.relax(ii, x, y + 1, z, c + C_CLIMB, CLIMB); }
                if !virtual_stand && !dug && self.g.is_tower(x, y - 1, z) && self.g.stand16(x, y - 1, z) >= 0 { self.relax(ii, x, y - 1, z, c + C_DESCEND, DESCEND); }
                if !s_here && !dug && head2 && self.g.is_air(x, y, z) && (on_tower || self.tower_base(x, y, z)) && self.tower_ok(x, y, z) {
                    let perched = !on_tower && y > self.g.oy && !tb.scaffolding[self.g.get(x, y - 1, z) as usize];
                    let cc = c + C_TOWER + self.nav.tower_penalty + if perched { self.nav.perch_penalty } else { 0 };
                    self.relax(ii, x, y + 1, z, cc, TOWER);
                }
            }
        }
        self.nav.total_expanded += self.nav.expanded as i64;
        false
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

    fn tower_base(&self, x: i32, y: i32, z: i32) -> bool {
        let below = self.g.get(x, y - 1, z) as usize;
        if t().scaffolding[below] { return true; }
        if !t().sturdy_top[below] { return false; }
        !self.in_scaffold(x, y - 1, z)
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

    fn is_scaffold_block(&self, x: i32, y: i32, z: i32) -> bool { self.in_scaffold(x, y, z) && !self.g.is_tower(x, y, z) }

    fn clearable(&self, x: i32, y: i32, z: i32) -> bool {
        let (s0, s1) = (self.is_scaffold_block(x, y, z), self.is_scaffold_block(x, y + 1, z));
        if !s0 && !s1 { return false; }
        if !(s0 || self.g.passable(x, y, z)) || !(s1 || self.g.passable(x, y + 1, z)) { return false; }
        let below = self.g.get(x, y - 1, z) as usize;
        t().col_top[below] == 16 && t().any_top[below] == 16
    }

    fn nav_build(&mut self, goal: usize) {
        let fast = self.nav.fast;
        let node_of = |b: &Bot, i: usize| -> (i32, u8) {
            if fast { let c = &b.g.pc[b.g.pidx(b.g.x_of(i), b.g.y_of(i), b.g.z_of(i))]; (c.parent, c.how) } else { (b.nav.nodes[i].parent, b.nav.nodes[i].how) }
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
            self.nav.path[k * 3] = self.g.x_of(iu); self.nav.path[k * 3 + 1] = self.g.y_of(iu); self.nav.path[k * 3 + 2] = self.g.z_of(iu);
            let (par, how) = node_of(self, iu);
            self.nav.mv[k] = how;
            if how == PILLAR || how == BRIDGE { self.nav.scaffold_moves += 1; }
            if how == TOWER { self.nav.tower_moves += 1; }
            i = par;
        }
    }
}
