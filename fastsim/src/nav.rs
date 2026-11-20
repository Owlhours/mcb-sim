//! Route search: A* over standing cells. Moves and their costs in game ticks follow A's route finder (`nav.rs`):
//! walk, jump up one, drop, put up scaffolding under oneself, climb or descend standing scaffolding, bridge with a
//! loose block, break through (only when asked).
//!
//! Nodes live in a small stamped open-addressed table that grows with the search, so a short search touches a few
//! cache lines whatever the size of the world.
//!
//! The open list is a binary heap of `f << 32 | cell id`, or with `FS_BUCKETQ=1` a bucket queue (`BucketQ`) that pops
//! the same entries in the same order.
use crate::world::*;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub const WALK: u8 = 0;
pub const JUMP: u8 = 1;
pub const DROP: u8 = 2;
pub const TOWER_UP: u8 = 3;
pub const CLIMB: u8 = 4;
pub const DESCEND: u8 = 5;
pub const BRIDGE: u8 = 6;
pub const MINE: u8 = 7;

/// The planner's weights per move (A's route finder's own: `C_WALK` ...). They decide which route is taken.
#[derive(Clone, Debug)]
pub struct Costs {
    pub walk: u32,
    pub jump: u32,
    pub drop: u32,
    /// extra per two blocks fallen
    pub fall2: u32,
    pub tower: u32,
    pub climb: u32,
    pub descend: u32,
    pub bridge: u32,
    /// A's loose pillar (`C_PILLAR` 12 + `scaffold_penalty` 40): a loose block put underfoot, up one; 0 = no pillars
    pub pillar: u32,
    /// breaking through one block
    pub mine: u32,
}
impl Default for Costs {
    fn default() -> Costs { Costs { walk: 5, jump: 9, drop: 6, fall2: 1, tower: 16, climb: 9, descend: 7, bridge: 62, pillar: 52, mine: 430 } }
}

/// What actions take in game ticks: fitted to A's simulator (`tools/fit_costs.py` on its event logs, see STATUS).
#[derive(Clone, Debug)]
pub struct Times {
    /// per move kind (WALK .. MINE)
    pub mv: [u32; 8],
    /// per route walked: starting, settling
    pub route: u32,
    /// after a route, before the first click: turning, aiming
    pub arrive: u32,
    /// one placement
    pub place: u32,
    /// placing under one's own feet with a jump
    pub self_place: u32,
    /// extra for leaning over an edge
    pub lean: u32,
    pub break_loose: u32,
    pub break_tower: u32,
    /// a trip: opening a chest, taking one stack
    pub chest_open: u32,
    pub chest_stack: u32,
    /// a walk to a stance that fails: fixed part, and the part of the way's ticks in per cent (a lean: its own part;
    /// in a clean-up: its own fixed part); a click that fails, in the main pass and after it
    pub wfail: u32,
    pub wfail_pct: u32,
    pub wfail_lean_pct: u32,
    pub wfail_clean: u32,
    pub cfail: u32,
    pub cfail_after: u32,
}
impl Default for Times {
    fn default() -> Times {
        Times { mv: [4, 12, 12, 14, 10, 9, 18, 170], route: 5, arrive: 14, place: 1, self_place: 8, lean: 6, break_loose: 13, break_tower: 1, chest_open: 4, chest_stack: 1, wfail: 20, wfail_pct: 30, wfail_lean_pct: 85, wfail_clean: 40, cfail: 4, cfail_after: 20 }
    }
}

#[derive(Clone, Copy, Default)]
struct Node { key: u32, g: u32, parent: u32, stamp: u16, how: u8, closed: u8 }

#[derive(Clone, Copy)]
pub struct Opts { pub budget: u32, pub towers: bool, pub bridges: bool, pub mine: bool,
    /// A's `towerBase`: a column starts only on scaffolding or on a block with a sturdy top (a placed slab, stair or
    /// fence is not one; the ground is)
    pub sturdy: bool }

pub struct Nav {
    /// `FS_BUCKETQ=1`: the open list is `bq`, not `heap`
    use_bq: bool,
    bq: BucketQ,
    tab: Vec<Node>,
    mask: usize,
    used: usize,
    epoch: u16,
    heap: BinaryHeap<Reverse<u64>>,
    pub costs: Costs,
    /// weight of the estimate, in eighths (8 = plain A*)
    pub weight: u32,
    /// the route found, start excluded: (how, cell reached)
    pub path: Vec<(u8, u32)>,
    pub goal_cost: u32,
    pub tower_moves: u32,
    pub bridge_moves: u32,
    pub searches: u64,
    pub expanded: u64,
    /// searches that found nothing, and the cells they expanded
    pub fails: u64,
    pub expanded_fail: u64,
    /// the last search ran out of budget (as opposed to: out of places)
    pub exhausted: bool,
}

const MIN_BITS: usize = 9;
const NOKEY: u32 = u32::MAX;
const NIL: u32 = u32::MAX;

/// Bucket queue for the open list: one bucket per value of f = g + h (costs are whole numbers), a bit per bucket
/// that is not empty, and in each bucket a list of cell ids. Pops the smallest f, and in it the smallest cell id:
/// exactly the order of the binary heap of `f << 32 | id`. The estimate is weighted, so f is not monotone (a push
/// can go below the f last popped); `cur` is only a lower bound of the smallest f in the queue.
/// Buckets are indexed by f itself (the start has f = 0); the arrays grow to the largest f seen.
struct BucketQ {
    /// per f: the first entry of its list (valid where the bit is set)
    head: Vec<u32>,
    /// per f: the bucket is not empty
    bits: Vec<u64>,
    /// entries: (cell id, next entry in the same bucket)
    pool: Vec<(u32, u32)>,
    len: usize,
    /// no entry has an f below this
    cur: usize,
    /// no entry has an f above this
    hi: usize,
}

impl BucketQ {
    fn new() -> BucketQ { BucketQ { head: Vec::new(), bits: Vec::new(), pool: Vec::with_capacity(4096), len: 0, cur: 0, hi: 0 } }

    fn clear(&mut self) {
        if self.len > 0 { for w in &mut self.bits[self.cur >> 6..=self.hi >> 6] { *w = 0; } }
        self.len = 0;
        self.cur = 0;
        self.hi = 0;
        self.pool.clear();
    }

    #[cold]
    fn grow(&mut self, f: usize) {
        let n = (f + 1).next_power_of_two().max(1 << 12);
        self.head.resize(n, 0);
        self.bits.resize(n >> 6, 0);
    }

    #[inline(always)]
    fn push(&mut self, f: u32, key: u32) {
        let f = f as usize;
        if f >= self.head.len() { self.grow(f); }
        let (w, b) = (f >> 6, 1u64 << (f & 63));
        unsafe {
            let bw = self.bits.get_unchecked_mut(w);
            let h = self.head.get_unchecked_mut(f);
            let next = if *bw & b != 0 { *h } else { NIL };
            *h = self.pool.len() as u32;
            *bw |= b;
            self.pool.push((key, next));
        }
        self.cur = if self.len == 0 { f } else { self.cur.min(f) };
        self.hi = self.hi.max(f);
        self.len += 1;
    }

    /// The smallest cell id of the smallest f (and removes it).
    #[inline(always)]
    fn pop(&mut self) -> Option<u32> {
        if self.len == 0 { return None; }
        self.len -= 1;
        unsafe {
            let mut w = self.cur >> 6;
            let mut word = *self.bits.get_unchecked(w) & (!0u64 << (self.cur & 63));
            while word == 0 { w += 1; word = *self.bits.get_unchecked(w); }
            let f = (w << 6) | word.trailing_zeros() as usize;
            self.cur = f;
            let h = *self.head.get_unchecked(f);
            let (k0, n0) = *self.pool.get_unchecked(h as usize);
            if n0 == NIL {
                *self.bits.get_unchecked_mut(w) &= !(1u64 << (f & 63));
                return Some(k0);
            }
            // more than one: take the smallest id out of the list
            let (mut bk, mut bi, mut bprev, mut prev, mut i) = (k0, h, NIL, h, n0);
            while i != NIL {
                let (k, n) = *self.pool.get_unchecked(i as usize);
                if k < bk { bk = k; bi = i; bprev = prev; }
                prev = i;
                i = n;
            }
            let bn = self.pool.get_unchecked(bi as usize).1;
            if bprev == NIL { *self.head.get_unchecked_mut(f) = bn; } else { self.pool.get_unchecked_mut(bprev as usize).1 = bn; }
            Some(bk)
        }
    }
}

impl Nav {
    pub fn new() -> Nav {
        let use_bq = std::env::var("FS_BUCKETQ").map_or(false, |v| v == "1");
        Nav { use_bq, bq: BucketQ::new(), tab: vec![Node::default(); 1 << MIN_BITS], mask: (1 << MIN_BITS) - 1, used: 0, epoch: 0, heap: BinaryHeap::with_capacity(4096), costs: Costs::default(), weight: 16,
            path: Vec::with_capacity(1024), goal_cost: 0, tower_moves: 0, bridge_moves: 0, searches: 0, expanded: 0, fails: 0, expanded_fail: 0, exhausted: false }
    }

    #[inline(always)]
    fn slot(&self, key: u32) -> usize {
        let mut i = (key.wrapping_mul(0x9E37_79B1) >> 12) as usize & self.mask;
        loop {
            let n = unsafe { self.tab.get_unchecked(i) };
            if n.stamp != self.epoch || n.key == key { return i; }
            i = (i + 1) & self.mask;
        }
    }

    #[cold]
    fn grow(&mut self) {
        let old = std::mem::replace(&mut self.tab, vec![Node::default(); (self.mask + 1) * 4]);
        self.mask = self.tab.len() - 1;
        for n in old { if n.stamp == self.epoch { let i = self.slot(n.key); self.tab[i] = n; } }
    }

    fn begin(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 || self.mask + 1 > 1 << 12 {
            // (a table grown by a long search is dropped again: short searches stay in a few cache lines)
            if self.mask + 1 > 1 << 12 { self.tab = vec![Node::default(); 1 << MIN_BITS]; self.mask = (1 << MIN_BITS) - 1; } else { for n in self.tab.iter_mut() { n.stamp = 0; } }
            self.epoch = 1;
        }
        self.used = 0;
        self.heap.clear();
        self.bq.clear();
    }

    #[inline(always)]
    fn relax<const BQ: bool>(&mut self, key: u32, g: u32, h: u32, parent: u32, how: u8) {
        let i = self.slot(key);
        let n = unsafe { self.tab.get_unchecked_mut(i) };
        if n.stamp == self.epoch {
            if n.g <= g { return; }
        } else {
            self.used += 1;
        }
        *n = Node { key, g, parent, stamp: self.epoch, how, closed: 0 };
        if BQ { self.bq.push(g + h, key); } else { self.heap.push(Reverse(((g + h) as u64) << 32 | key as u64)); }
        if self.used * 2 > self.mask { self.grow(); }
    }

    /// Searches a route from `start` to `goal` (cell ids). On success the route is in `path` and its cost in `goal_cost`.
    pub fn search(&mut self, w: &World<'_>, start: u32, goal: u32, o: Opts) -> bool { self.search_in(w, &[], start, goal, o) }

    /// `next`: each column's next cell (the bot's `next`), for A's rule that a loose block may go into a target cell
    /// whose turn has not come (`scaffold_ok`); empty: never into a target cell.
    pub fn search_in(&mut self, w: &World<'_>, next: &[i16], start: u32, goal: u32, o: Opts) -> bool {
        if self.use_bq { self.search_q::<true>(w, next, start, goal, o) } else { self.search_q::<false>(w, next, start, goal, o) }
    }

    #[inline(never)]
    fn search_q<const BQ: bool>(&mut self, w: &World<'_>, next: &[i16], start: u32, goal: u32, o: Opts) -> bool {
        self.searches += 1;
        self.exhausted = false;
        self.path.clear();
        self.goal_cost = 0;
        self.tower_moves = 0;
        self.bridge_moves = 0;
        if start == goal { return true; }
        self.begin();
        let k = self.costs.clone();
        let nx = w.nx;
        let (gc, gy) = (id_c(goal), id_y(goal) as i32);
        let (gx, gz) = w.xz(gc);
        // every level up costs a jump (which also moves one cell sideways) or a climb; the estimate is weighted
        // (A's search is a weighted A* too): fewer cells expanded, routes a little longer than the best
        let (hw, hj, hc, wt) = (k.walk, k.jump - k.walk, k.jump.min(k.climb), self.weight);
        let hf = |x: i32, y: i32, z: i32| -> u32 {
            let m = ((x - gx).abs() + (z - gz).abs()) as u32;
            let up = (gy - y).max(0) as u32;
            (hw * m + hj * up.min(m) + hc * up.saturating_sub(m)) * wt / 8
        };
        let e0 = self.expanded;
        let dcs: [isize; 4] = [1, -1, nx as isize, -(nx as isize)];
        let dxs: [i32; 4] = [1, -1, 0, 0];
        let dzs: [i32; 4] = [0, 0, 1, -1];
        self.relax::<BQ>(start, 0, 0, NOKEY, WALK);
        let mut budget = o.budget;
        let top = w.ny - 3;
        loop {
            let key = if BQ {
                match self.bq.pop() { Some(k) => k, None => break }
            } else {
                match self.heap.pop() { Some(Reverse(e)) => e as u32, None => break }
            };
            let i = self.slot(key);
            let node = unsafe { self.tab.get_unchecked_mut(i) };
            if node.closed != 0 { continue; }
            node.closed = 1;
            let g = node.g;
            if key == goal {
                self.goal_cost = g;
                let mut kk = key;
                while kk != start {
                    let n = self.tab[self.slot(kk)];
                    self.path.push((n.how, kk));
                    if n.how == TOWER_UP { self.tower_moves += 1; } else if n.how == BRIDGE { self.bridge_moves += 1; }
                    kk = n.parent;
                }
                self.path.reverse();
                return true;
            }
            if budget == 0 { self.exhausted = true; self.fails += 1; self.expanded_fail += self.expanded - e0; return false; }
            budget -= 1;
            self.expanded += 1;
            let (c, y) = (id_c(key), id_y(key));
            let (x, z) = w.xz(c);
            let yi = y as i32;
            let own_s = w.win(SOLID, c, y - 1);
            let own_t = w.win(TOWER, c, y - 1);
            let up_clear = own_s & 8 == 0 && y < top;
            for d in 0..4 {
                let n = (c as isize + dcs[d]) as usize;
                let s = w.win(SOLID, n, y - 1);
                let t = w.win(TOWER, n, y - 1);
                let fl = s | t | (t >> 1);
                let (x2, z2) = (x + dxs[d], z + dzs[d]);
                if s & 0b110 == 0 {
                    if fl & 1 != 0 {
                        self.relax::<BQ>(cell_id(n, y), g + k.walk, hf(x2, yi, z2), key, WALK);
                    } else {
                        let land = w.floor_below(n, y - 1) + 1;
                        self.relax::<BQ>(cell_id(n, land), g + k.drop + k.fall2 * ((y - land) as u32 / 2), hf(x2, land as i32, z2), key, DROP);
                        // (A: no bridging off the top of a scaffolding column)
                        if o.bridges && own_t & 1 == 0 && w.win(PLACED, n, y - 1) & 1 == 0 && (w.win(TGT, n, y - 1) & 1 == 0 || (!next.is_empty() && next[n] != i16::MAX && (y - 1) as i16 > next[n])) {
                            self.relax::<BQ>(cell_id(n, y), g + k.bridge, hf(x2, yi, z2), key, BRIDGE);
                        }
                    }
                } else if o.mine && fl & 1 != 0 {
                    // break through: only blocks the bot put there (target blocks, loose blocks)
                    let own = w.win(PLACED, n, y - 1) | w.win(LOOSE, n, y - 1);
                    if s & 0b110 & !own == 0 {
                        let cnt = (s & 0b110).count_ones();
                        self.relax::<BQ>(cell_id(n, y), g + k.walk + k.mine * cnt, hf(x2, yi, z2), key, MINE);
                    }
                }
                if up_clear && s & 0b1100 == 0 && fl & 0b10 != 0 {
                    self.relax::<BQ>(cell_id(n, y + 1), g + k.jump, hf(x2, yi + 1, z2), key, JUMP);
                }
            }
            if up_clear {
                if own_t & 0b10 != 0 {
                    self.relax::<BQ>(cell_id(c, y + 1), g + k.climb, hf(x, yi + 1, z), key, CLIMB);
                } else if o.towers && w.win(TGT, c, y) & 1 == 0 && w.win(PLACED, c, y) & 1 == 0 && (own_t & 1 != 0 || (w.win(LOOSE, c, y - 1) & 1 == 0
                    && !(o.sturdy && w.win(PLACED, c, y - 1) & 1 != 0 && w.win(TGT, c, y - 1) & 1 != 0 && w.pal_at(c, y - 1).class2 & crate::prep::C2_STURDY == 0))) {
                    // (A: a column starts on a block of the build or the ground, not on a loose temporary block)
                    self.relax::<BQ>(cell_id(c, y + 1), g + k.tower, hf(x, yi + 1, z), key, TOWER_UP);
                }
                // (A's loose pillar: a loose block into the own cell, up one - where a column may not stand, on a loose
                // block or in a target cell whose turn has not come; walked as a bridge move straight up)
                if k.pillar > 0 && o.bridges && own_t & 0b10 == 0 && w.win(PLACED, c, y) & 1 == 0
                    && (w.win(TGT, c, y) & 1 == 0 || (!next.is_empty() && next[c] != i16::MAX && y as i16 > next[c])) {
                    self.relax::<BQ>(cell_id(c, y + 1), g + k.pillar, hf(x, yi + 1, z), key, BRIDGE);
                }
            }
            if own_t & 1 != 0 {
                self.relax::<BQ>(cell_id(c, y - 1), g + k.descend, hf(x, yi - 1, z), key, DESCEND);
            }
        }
        self.fails += 1;
        self.expanded_fail += self.expanded - e0;
        false
    }
}
