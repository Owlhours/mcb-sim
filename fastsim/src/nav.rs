//! Route search: A* over standing cells. Moves and their costs in game ticks follow A's route finder (`nav.rs`):
//! walk, jump up one, drop, put up scaffolding under oneself, climb or descend standing scaffolding, bridge with a
//! loose block, break through (only when asked).
//!
//! Nodes live in a small stamped open-addressed table that grows with the search, so a short search touches a few
//! cache lines whatever the size of the world.
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

/// The cost table: game ticks per action.
#[derive(Clone, Debug)]
pub struct Costs {
    pub walk: u32,
    pub jump: u32,
    pub drop: u32,
    /// extra ticks per two blocks fallen
    pub fall2: u32,
    pub tower: u32,
    pub climb: u32,
    pub descend: u32,
    pub bridge: u32,
    pub mine: u32,
    /// placing a block from where the bot stands
    pub place: u32,
    /// extra for a placement after moving (turning, settling)
    pub arrive: u32,
    /// placing under one's own feet with a jump
    pub self_place: u32,
    /// extra for leaning over an edge
    pub lean: u32,
    /// breaking a loose block / a scaffolding foot
    pub break_loose: u32,
    pub break_tower: u32,
    /// a trip: opening a chest, taking one stack
    pub chest_open: u32,
    pub chest_stack: u32,
    /// search penalty for a breaking through one block (not game time)
    pub mine_penalty: u32,
}
impl Default for Costs {
    fn default() -> Costs {
        Costs { walk: 5, jump: 9, drop: 6, fall2: 1, tower: 13, climb: 9, descend: 7, bridge: 22, mine: 30, place: 4, arrive: 4, self_place: 12, lean: 8,
            break_loose: 8, break_tower: 6, chest_open: 20, chest_stack: 6, mine_penalty: 400 }
    }
}

#[derive(Clone, Copy, Default)]
struct Node { key: u32, g: u32, parent: u32, stamp: u16, how: u8, closed: u8 }

#[derive(Clone, Copy)]
pub struct Opts { pub budget: u32, pub towers: bool, pub bridges: bool, pub mine: bool }

pub struct Nav {
    tab: Vec<Node>,
    mask: usize,
    used: usize,
    epoch: u16,
    heap: BinaryHeap<Reverse<u64>>,
    pub costs: Costs,
    /// the route found, start excluded: (how, cell reached)
    pub path: Vec<(u8, u32)>,
    pub goal_cost: u32,
    pub tower_moves: u32,
    pub bridge_moves: u32,
    pub searches: u64,
    pub expanded: u64,
    /// the last search ran out of budget (as opposed to: out of places)
    pub exhausted: bool,
}

const MIN_BITS: usize = 9;
const NOKEY: u32 = u32::MAX;

impl Nav {
    pub fn new() -> Nav {
        Nav { tab: vec![Node::default(); 1 << MIN_BITS], mask: (1 << MIN_BITS) - 1, used: 0, epoch: 0, heap: BinaryHeap::with_capacity(4096), costs: Costs::default(),
            path: Vec::with_capacity(1024), goal_cost: 0, tower_moves: 0, bridge_moves: 0, searches: 0, expanded: 0, exhausted: false }
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
    }

    #[inline(always)]
    fn relax(&mut self, key: u32, g: u32, h: u32, parent: u32, how: u8) {
        let i = self.slot(key);
        let n = unsafe { self.tab.get_unchecked_mut(i) };
        if n.stamp == self.epoch {
            if n.g <= g { return; }
        } else {
            self.used += 1;
        }
        *n = Node { key, g, parent, stamp: self.epoch, how, closed: 0 };
        self.heap.push(Reverse(((g + h) as u64) << 32 | key as u64));
        if self.used * 2 > self.mask { self.grow(); }
    }

    /// Searches a route from `start` to `goal` (cell ids). On success the route is in `path` and its cost in `goal_cost`.
    pub fn search(&mut self, w: &World, start: u32, goal: u32, o: Opts) -> bool {
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
        let (gx, gz) = ((gc % nx) as i32, (gc / nx) as i32);
        let hf = |x: i32, y: i32, z: i32| -> u32 {
            let m = ((x - gx).abs() + (z - gz).abs()) as u32;
            let up = (gy - y).max(0) as u32;
            k.walk * m + (k.jump - k.walk) * up
        };
        let dcs: [isize; 4] = [1, -1, nx as isize, -(nx as isize)];
        let dxs: [i32; 4] = [1, -1, 0, 0];
        let dzs: [i32; 4] = [0, 0, 1, -1];
        self.relax(start, 0, 0, NOKEY, WALK);
        let mut budget = o.budget;
        let top = w.ny - 3;
        while let Some(Reverse(e)) = self.heap.pop() {
            let key = e as u32;
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
            if budget == 0 { self.exhausted = true; return false; }
            budget -= 1;
            self.expanded += 1;
            let (c, y) = (id_c(key), id_y(key));
            let (x, z) = ((c % nx) as i32, (c / nx) as i32);
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
                        self.relax(cell_id(n, y), g + k.walk, hf(x2, yi, z2), key, WALK);
                    } else {
                        let land = w.floor_below(n, y - 1) + 1;
                        self.relax(cell_id(n, land), g + k.drop + k.fall2 * ((y - land) as u32 / 2), hf(x2, land as i32, z2), key, DROP);
                        if o.bridges && w.win(TGT, n, y - 1) & 1 == 0 && w.win(PLACED, n, y - 1) & 1 == 0 {
                            self.relax(cell_id(n, y), g + k.bridge, hf(x2, yi, z2), key, BRIDGE);
                        }
                    }
                } else if o.mine && fl & 1 != 0 {
                    // break through: only blocks the bot put there (target blocks, loose blocks)
                    let own = w.win(PLACED, n, y - 1) | w.win(LOOSE, n, y - 1);
                    if s & 0b110 & !own == 0 {
                        let cnt = (s & 0b110).count_ones();
                        self.relax(cell_id(n, y), g + k.walk + (k.mine + k.mine_penalty) * cnt, hf(x2, yi, z2), key, MINE);
                    }
                }
                if up_clear && s & 0b1100 == 0 && fl & 0b10 != 0 {
                    self.relax(cell_id(n, y + 1), g + k.jump, hf(x2, yi + 1, z2), key, JUMP);
                }
            }
            if up_clear {
                if own_t & 0b10 != 0 {
                    self.relax(cell_id(c, y + 1), g + k.climb, hf(x, yi + 1, z), key, CLIMB);
                } else if o.towers && w.win(TGT, c, y) & 1 == 0 && w.win(PLACED, c, y) & 1 == 0 {
                    self.relax(cell_id(c, y + 1), g + k.tower, hf(x, yi + 1, z), key, TOWER_UP);
                }
            }
            if own_t & 1 != 0 {
                self.relax(cell_id(c, y - 1), g + k.descend, hf(x, yi - 1, z), key, DESCEND);
            }
        }
        false
    }
}
