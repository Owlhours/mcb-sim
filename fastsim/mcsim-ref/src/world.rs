//! The world the bot builds in: block states in a box around the site, the game's block-update machinery
//! (Level.setBlock, shape updates, the collecting neighbour updater, scheduled ticks, block events), containers,
//! and the list of changes to report to the client.
#![allow(dead_code)]
use crate::geom::*;
use crate::rules;
use crate::tables::*;
use crate::fasthash::{FastMap, FastSet};
use std::collections::{BTreeSet, VecDeque};

pub const F_NEIGHBORS: u32 = 1;
pub const F_CLIENTS: u32 = 2;
pub const F_KNOWN_SHAPE: u32 = 16;
pub const F_NO_DROPS: u32 = 32;
pub const F_MOVED_BY_PISTON: u32 = 64;
pub const F_SKIP_WIRE_SHAPE: u32 = 128;
pub const F_SKIP_SIDE_EFFECTS: u32 = 256;
pub const F_SKIP_ON_PLACE: u32 = 512;
pub const F_ALL: u32 = 3;

#[derive(Clone, Debug)]
enum Update {
    Shape { dir: Dir, nstate: S, pos: P, npos: P, flags: u32, limit: i32 },
    Simple { pos: P, block: u16 },
    Full { state: S, pos: P, block: u16, moved: bool },
    Multi { src: P, block: u16, skip: Option<Dir>, idx: usize },
}

/// What the rules can see of the player (and other entities) without owning them.
#[derive(Clone, Debug)]
pub struct EntityView {
    pub bb: Aabb,
    pub pos: [f64; 3],
    pub is_player: bool,
}

#[derive(Clone, Debug)]
pub struct FallingBlock {
    pub state: S,
    pub pos: [f64; 3],
    pub vel: [f64; 3],
    pub time: i32,
    pub on_ground: bool,
    pub cancel_drop: bool,
    pub removed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Stack {
    pub item: u16,
    pub count: u8,
}

pub struct World {
    pub x0: i32, pub y0: i32, pub z0: i32,
    pub nx: i32, pub ny: i32, pub nz: i32,
    pub cells: Vec<S>,
    pub ground_top: i32,
    pub max_y: i32,
    pub game_time: i64,
    grass: S, dirt: S, bedrock: S,
    // scheduled ticks: (trigger, priority, order, pos, block)
    sched: BTreeSet<(i64, i8, u64, i32, i32, i32, u16)>,
    sched_set: FastSet<(P, u16)>,
    sched_order: u64,
    block_events: VecDeque<(P, u16, u8, u8)>,
    block_event_set: FastSet<(P, u16, u8, u8)>,
    // neighbour updater
    stack: Vec<Update>,
    added: Vec<Update>,
    upd_count: i64,
    // changes to tell the client: chunk (insertion order) -> section -> positions
    changed: Vec<P>,
    changed_set: FastSet<P>,
    pub falling: Vec<FallingBlock>,
    pub entities: Vec<EntityView>,
    pub containers: FastMap<P, Vec<Stack>>,
    rng: u64,
    /// set by block rules when a click opens a container or other screen: (position, number of container slots; 0 = no slots of interest)
    pub opened_menu: Option<(P, u16)>,
    /// counters for diagnostics
    pub n_set: u64, pub n_shape: u64, pub n_neighbor: u64, pub n_ticks: u64,
    /// set when something the model does not cover happened (explosions and the like); the run notes it
    pub unmodelled: u64,
    /// number of registered game event listeners (sculk sensor block entities, rules::vibrations)
    pub vib_listeners: u32,
    /// the player died during this build (reported in the run log)
    pub player_died: bool,
    /// draws from the game's Level.random (clock-seeded in the Java run: what depends on them differs between Java runs)
    pub rand_draws: u64,
    /// the Java run's ServerLevel.getGameTime: advances only in complete server ticks (the Java `Env` skips idle
    /// ones; `game_time` advances every tick). Read by the vibration selector.
    pub level_time: i64,
}

impl World {
    pub fn new(x0: i32, y0: i32, z0: i32, nx: i32, ny: i32, nz: i32, ground_top: i32, max_y: i32) -> World {
        let tb = t();
        let grass = tb.default_of("grass_block");
        let mut w = World {
            x0, y0, z0, nx, ny, nz,
            cells: vec![0; (nx as usize) * (ny as usize) * (nz as usize)],
            ground_top, max_y, game_time: 0,
            grass, dirt: tb.default_of("dirt"), bedrock: tb.default_of("bedrock"),
            sched: BTreeSet::new(), sched_set: FastSet::default(), sched_order: 0,
            block_events: VecDeque::new(), block_event_set: FastSet::default(),
            stack: Vec::new(), added: Vec::new(), upd_count: 0,
            changed: Vec::new(), changed_set: FastSet::default(),
            falling: Vec::new(), entities: Vec::new(), containers: FastMap::default(),
            rng: 0x5DEECE66D, opened_menu: None,
            n_set: 0, n_shape: 0, n_neighbor: 0, n_ticks: 0, unmodelled: 0, vib_listeners: 0, player_died: false, rand_draws: 0, level_time: 0,
        };
        // the ground layer inside the box
        if ground_top >= y0 && ground_top < y0 + ny {
            for z in 0..nz { for x in 0..nx { let i = w.idx(p(x0 + x, ground_top, z0 + z)); w.cells[i] = grass; } }
        }
        w
    }

    #[inline]
    pub fn inside(&self, q: P) -> bool {
        q.x >= self.x0 && q.x < self.x0 + self.nx && q.y >= self.y0 && q.y < self.y0 + self.ny && q.z >= self.z0 && q.z < self.z0 + self.nz
    }
    #[inline]
    pub fn idx(&self, q: P) -> usize {
        (((q.y - self.y0) as usize * self.nz as usize) + (q.z - self.z0) as usize) * self.nx as usize + (q.x - self.x0) as usize
    }
    /// Block state at a position (Level.getBlockState).
    #[inline]
    pub fn get(&self, q: P) -> S {
        if self.inside(q) { return self.cells[self.idx(q)]; }
        if q.y > self.ground_top { 0 } else if q.y == self.ground_top { self.grass } else if q.y >= self.ground_top - 2 { self.dirt } else if q.y == self.ground_top - 3 { self.bedrock } else { 0 }
    }
    /// Writes a cell without any of the game's side effects (site setup).
    pub fn put_raw(&mut self, q: P, s: S) { if self.inside(q) { let i = self.idx(q); self.cells[i] = s; } }

    pub fn rand_u32(&mut self) -> u32 {
        self.rand_draws += 1;
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.rng >> 33) as u32
    }
    pub fn rand_f32(&mut self) -> f32 { (self.rand_u32() >> 8) as f32 / (1u32 << 24) as f32 }
    pub fn rand_int(&mut self, bound: u32) -> u32 { self.rand_u32() % bound.max(1) }

    // ------------------------------------------------------------------ changes for the client
    fn mark_changed(&mut self, q: P) { if self.changed_set.insert(q) { self.changed.push(q); } }
    /// Level.sendBlockUpdated: the position is reported to the client with the next broadcast.
    pub fn send_block_updated(&mut self, q: P) { self.mark_changed(q); }
    /// Takes the positions changed since the last call (in the order chunks were first touched; within a chunk in order of change).
    pub fn take_changes(&mut self) -> Vec<P> {
        self.changed_set.clear();
        let ch = std::mem::take(&mut self.changed);
        if ch.len() <= 1 { return ch; }
        // ChunkHolder.broadcastChanges: per chunk, sections from the
        // bottom up, and within a section the iteration order of the ShortOpenHashSet of packed positions
        let mut chunks: Vec<(i32, i32)> = Vec::new();
        for q in &ch { let c = (q.x >> 4, q.z >> 4); if !chunks.contains(&c) { chunks.push(c); } }
        // The game keeps the chunks with changes in an identity-hash set: their order is fixed for a pair of chunks
        // within one run and arbitrary between runs. A fixed order stands in for it (see docs/architecture.md).
        chunks.sort_by(|a, b| b.cmp(a));
        let mut out = Vec::with_capacity(ch.len());
        for c in chunks {
            let mut secs: Vec<i32> = ch.iter().filter(|q| (q.x >> 4, q.z >> 4) == c).map(|q| q.y >> 4).collect();
            secs.sort();
            secs.dedup();
            for sy in secs {
                let mut set = crate::fastutil::IntSet::new();
                for q in ch.iter().filter(|q| (q.x >> 4, q.z >> 4) == c && q.y >> 4 == sy) { set.add(((q.x & 15) << 8) | ((q.z & 15) << 4) | (q.y & 15)); }
                for k in set.keys() { out.push(p((c.0 << 4) | (k >> 8 & 15), (sy << 4) | (k & 15), (c.1 << 4) | (k >> 4 & 15))); }
            }
        }
        out
    }
    pub fn has_pending_changes(&self) -> bool { !self.changed.is_empty() }

    // ------------------------------------------------------------------ Level.setBlock
    pub fn set_block(&mut self, q: P, s: S, flags: u32) -> bool { self.set_block_limit(q, s, flags, 512) }

    pub fn set_block_limit(&mut self, q: P, s: S, flags: u32, limit: i32) -> bool {
        if !self.inside(q) || q.y > self.max_y { return false; }
        let i = self.idx(q);
        let old = self.cells[i];
        if old == s { return false; }
        self.n_set += 1;
        self.cells[i] = s;
        let tb = t();
        let block_changed = tb.block[old as usize] != tb.block[s as usize];
        let moved = flags & F_MOVED_BY_PISTON != 0;
        if block_changed && self.containers.contains_key(&q) && !tb.air[old as usize] {
            // (the block entity goes with its block)
            if !rules::keeps_block_entity(old, s) { self.containers.remove(&q); }
        }
        // LevelChunk.setBlockState: the old block's block entity goes (sculk sensors: their game event listener)
        if block_changed && rules::vibrations::is_sensor(old) { rules::vibrations::removed(self, q); }
        if (block_changed || rules::is_rail(s)) && (flags & F_NEIGHBORS != 0 || moved) {
            rules::affect_neighbors_after_removal(self, old, q, moved);
        }
        if tb.block[self.cells[i] as usize] != tb.block[s as usize] { return false; }
        if flags & F_SKIP_ON_PLACE == 0 { rules::on_place(self, s, q, old, moved); }
        // ... and the new one's is made (a state change of the same block keeps it)
        if block_changed && rules::vibrations::is_sensor(s) && tb.block[self.cells[i] as usize] == tb.block[s as usize] { rules::vibrations::created(self, q, s); }
        let now = self.cells[self.idx(q)];
        if now == s {
            if flags & F_CLIENTS != 0 { self.mark_changed(q); }
            if flags & F_NEIGHBORS != 0 {
                self.update_neighbors_at(q, tb.block[old as usize]);
                if rules::has_analog_output(s) { rules::update_neighbour_for_output_signal(self, q, tb.block[s as usize]); }
            }
            if flags & F_KNOWN_SHAPE == 0 && limit > 0 {
                let nf = flags & !33;
                rules::update_indirect_neighbour_shapes(self, old, q, nf, limit - 1);
                for d in UPDATE_SHAPE_ORDER {
                    self.neighbor_shape_changed(opposite(d), q.rel(d), q, s, nf, limit - 1);
                }
                rules::update_indirect_neighbour_shapes(self, s, q, nf, limit - 1);
            }
        }
        true
    }

    /// Level.removeBlock
    pub fn remove_block(&mut self, q: P, moved: bool) -> bool {
        self.set_block(q, 0, F_ALL | if moved { F_MOVED_BY_PISTON } else { 0 })
    }
    /// Level.destroyBlock (nothing drops in this world)
    pub fn destroy_block(&mut self, q: P, _drop: bool) -> bool { self.destroy_block_limit(q, _drop, 512) }
    pub fn destroy_block_limit(&mut self, q: P, _drop: bool, limit: i32) -> bool {
        let old = self.get(q);
        if t().air[old as usize] { return false; }
        let destroyed = self.set_block_limit(q, 0, F_ALL, limit);
        if destroyed { rules::vibrations::emit_at(self, rules::vibrations::Ev::BlockDestroy, q, rules::vibrations::Ctx::state(old)); }
        destroyed
    }

    // ------------------------------------------------------------------ neighbour updater (CollectingNeighborUpdater)
    /// LevelAccessor.neighborShapeChanged
    pub fn neighbor_shape_changed(&mut self, dir: Dir, pos: P, npos: P, nstate: S, flags: u32, limit: i32) {
        self.add_and_run(Update::Shape { dir, nstate, pos, npos, flags, limit });
    }
    /// Level.updateNeighborsAt
    pub fn update_neighbors_at(&mut self, pos: P, block: u16) { self.add_and_run(Update::Multi { src: pos, block, skip: None, idx: 0 }); }
    /// Level.updateNeighborsAtExceptFromFacing
    pub fn update_neighbors_at_except(&mut self, pos: P, block: u16, skip: Dir) {
        let idx = if NEIGHBOR_UPDATE_ORDER[0] == skip { 1 } else { 0 };
        self.add_and_run(Update::Multi { src: pos, block, skip: Some(skip), idx });
    }
    /// Level.neighborChanged(pos, block, orientation)
    pub fn neighbor_changed(&mut self, pos: P, block: u16) { self.add_and_run(Update::Simple { pos, block }); }
    /// Level.neighborChanged(state, pos, block, orientation, movedByPiston)
    pub fn neighbor_changed_state(&mut self, state: S, pos: P, block: u16, moved: bool) { self.add_and_run(Update::Full { state, pos, block, moved }); }

    fn add_and_run(&mut self, u: Update) {
        let running = self.upd_count > 0;
        let too_many = self.upd_count >= 1_000_000;
        self.upd_count += 1;
        if !too_many {
            if running { self.added.push(u); } else { self.stack.push(u); }
        }
        if !running { self.run_updates(); }
    }

    fn run_updates(&mut self) {
        while !self.stack.is_empty() || !self.added.is_empty() {
            for i in (0..self.added.len()).rev() {
                let u = self.added[i].clone();
                self.stack.push(u);
            }
            self.added.clear();
            while self.added.is_empty() {
                let top = self.stack.len() - 1;
                if !self.run_next(top) {
                    self.stack.remove(top);
                    break;
                }
            }
        }
        self.stack.clear();
        self.added.clear();
        self.upd_count = 0;
    }

    /// Runs one step of the update at stack index `i`; false when that update is finished.
    fn run_next(&mut self, i: usize) -> bool {
        match self.stack[i].clone() {
            Update::Shape { dir, nstate, pos, npos, flags, limit } => {
                self.n_shape += 1;
                let old = self.get(pos);
                if flags & F_SKIP_WIRE_SHAPE == 0 || !crate::is_blk!(old, "redstone_wire") {
                    let new = rules::update_shape(self, old, pos, dir, npos, nstate);
                    self.update_or_destroy(old, new, pos, flags, limit);
                }
                false
            }
            Update::Simple { pos, block } => {
                let st = self.get(pos);
                self.n_neighbor += 1;
                rules::neighbor_changed(self, st, pos, block, false);
                false
            }
            Update::Full { state, pos, block, moved } => {
                self.n_neighbor += 1;
                rules::neighbor_changed(self, state, pos, block, moved);
                false
            }
            Update::Multi { src, block, skip, idx } => {
                let d = NEIGHBOR_UPDATE_ORDER[idx];
                let mut next = idx + 1;
                if next < 6 && Some(NEIGHBOR_UPDATE_ORDER[next]) == skip { next += 1; }
                if let Update::Multi { idx: ref mut ix, .. } = self.stack[i] { *ix = next; }
                let np = src.rel(d);
                let st = self.get(np);
                self.n_neighbor += 1;
                rules::neighbor_changed(self, st, np, block, false);
                next < 6
            }
        }
    }

    /// Block.updateOrDestroy
    pub fn update_or_destroy(&mut self, old: S, new: S, pos: P, flags: u32, limit: i32) {
        if new != old {
            if t().air[new as usize] {
                self.destroy_block_limit(pos, flags & F_NO_DROPS == 0, limit);
            } else {
                self.set_block_limit(pos, new, flags & !F_NO_DROPS, limit);
            }
        }
    }

    // ------------------------------------------------------------------ scheduled ticks
    pub fn schedule_tick(&mut self, pos: P, block: u16, delay: i32) { self.schedule_tick_prio(pos, block, delay, 0); }
    /// priority: TickPriority value (-3 extremely high .. 3 extremely low; 0 normal)
    pub fn schedule_tick_prio(&mut self, pos: P, block: u16, delay: i32, prio: i8) {
        if !self.inside(pos) { return; }
        if !self.sched_set.insert((pos, block)) { return; }
        self.sched_order += 1;
        self.sched.insert((self.game_time + delay as i64, prio, self.sched_order, pos.x, pos.y, pos.z, block));
    }
    pub fn has_scheduled_tick(&self, pos: P, block: u16) -> bool { self.sched_set.contains(&(pos, block)) }
    /// LevelTicks.willTickThisTick is not modelled separately: a tick being run counts as no longer scheduled.
    pub fn scheduled_count(&self) -> usize { self.sched.len() }

    /// Runs the block ticks that are due (ServerLevel.tick: blockTicks.tick).
    pub fn run_scheduled(&mut self) {
        let now = self.game_time;
        let mut due = Vec::new();
        while let Some(first) = self.sched.iter().next().cloned() {
            if first.0 > now || due.len() >= 65536 { break; }
            self.sched.remove(&first);
            due.push(first);
        }
        for d in &due { self.sched_set.remove(&(p(d.3, d.4, d.5), d.6)); }
        for d in due {
            let q = p(d.3, d.4, d.5);
            let st = self.get(q);
            if t().block[st as usize] == d.6 {
                self.n_ticks += 1;
                rules::tick(self, st, q);
            }
        }
    }

    // ------------------------------------------------------------------ block events
    pub fn block_event(&mut self, pos: P, block: u16, b0: u8, b1: u8) {
        if self.block_event_set.insert((pos, block, b0, b1)) { self.block_events.push_back((pos, block, b0, b1)); }
    }
    pub fn run_block_events(&mut self) {
        let mut guard = 0;
        while let Some(e) = self.block_events.pop_front() {
            self.block_event_set.remove(&e);
            let st = self.get(e.0);
            if t().block[st as usize] == e.1 { rules::trigger_event(self, st, e.0, e.2, e.3); }
            guard += 1;
            if guard > 100000 { self.block_events.clear(); self.block_event_set.clear(); break; }
        }
    }
    pub fn pending_block_events(&self) -> usize { self.block_events.len() }

    // ------------------------------------------------------------------ support
    /// BlockState.isFaceSturdy(level, pos, direction, FULL)
    pub fn is_face_sturdy(&self, pos: P, dir: Dir) -> bool { rules::face_sturdy(self.get(pos), dir, 0) }
    /// Block.canSupportCenter
    pub fn can_support_center(&self, pos: P, dir: Dir) -> bool { rules::face_sturdy(self.get(pos), dir, 1) }
    /// Block.canSupportRigidBlock (top face, rigid support)
    pub fn can_support_rigid(&self, pos: P) -> bool { rules::face_sturdy(self.get(pos), UP, 2) }

    /// CollisionGetter.isUnobstructed for a block about to be placed: no entity that blocks building overlaps its collision shape.
    pub fn is_unobstructed(&self, s: S, pos: P) -> bool {
        let sh = rules::placement_collision_shape(s);
        let shape = &t().shapes[sh as usize];
        if shape.empty { return true; }
        let off = [pos.x as f64, pos.y as f64, pos.z as f64];
        !self.entities.iter().any(|e| shape.intersects(off, &e.bb))
    }

    /// FallingBlockEntity.fall: the block becomes a falling entity.
    pub fn spawn_falling(&mut self, pos: P, state: S) {
        self.falling.push(FallingBlock { state, pos: [pos.x as f64 + 0.5, pos.y as f64, pos.z as f64 + 0.5], vel: [0.0; 3], time: 0, on_ground: false, cancel_drop: false, removed: false });
        self.set_block(pos, 0, F_ALL);
    }
}
