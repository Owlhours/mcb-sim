//! The game as the bot meets it: the Java version's `Client` (one client tick: view, clicks, movement) and `Env`
//! (one server tick), over the world model. Clicks follow the vanilla client's tick order and reach the world
//! through ports of the server's packet handlers (use item on block, block breaking, container clicks).
#![allow(dead_code)]
use crate::geom::*;
use crate::physics::{self, Ctx, Hit};
use crate::player::{Input, Player};
use crate::rules::{self, Clicker, PlaceCtx, Use};
use crate::tables::*;
use crate::world::{FallingBlock, Stack, World, EntityView, F_ALL};
use std::collections::VecDeque;

pub const CLICK_QUICK_MOVE: u8 = 1;
pub const CLICK_SWAP: u8 = 2;

pub struct Menu {
    pub pos: P,
    /// container slots (0 for screens without storage)
    pub n: usize,
    /// positions of the chest halves holding slots 0..27 and 27..54
    pub parts: Vec<P>,
}

pub struct Sim {
    pub w: World,
    pub pl: Player,
    // inputs for the next tick
    yaw: f32,
    pitch: f32,
    keys: Input,
    use_held: bool,
    attack_held: bool,
    use_click: bool,
    attack_click: bool,
    want_slot: i32,
    screen_clicks: VecDeque<(i32, i32, u8)>,
    want_close: bool,
    want_open_inv: bool,
    // client state
    right_click_delay: i32,
    miss_time: i32,
    is_destroying: bool,
    destroy_pos: P,
    destroying_item: Stack,
    destroy_progress: f32,
    destroy_delay: i32,
    inventory_screen: bool,
    pub menu: Option<Menu>,
    pub hit: Option<Hit>,
    pub ticks: u64,
    /// block changes the server told the client about: (x, y, z, state)
    pub changes: Vec<(i32, i32, i32, S)>,
    // server-side ServerPlayerGameMode
    gm_ticks: i32,
    gm_destroying: bool,
    gm_destroy_pos: P,
    gm_destroy_start: i32,
    gm_delayed: bool,
    gm_delayed_pos: P,
    gm_delayed_start: i32,
    pub items_lost: u64,
}

impl Sim {
    pub fn new(w: World, x: f64, y: f64, z: f64) -> Sim {
        let mut s = Sim {
            w, pl: Player::new(x, y, z), yaw: 0.0, pitch: 0.0, keys: Input::default(), use_held: false, attack_held: false, use_click: false, attack_click: false,
            want_slot: -1, screen_clicks: VecDeque::new(), want_close: false, want_open_inv: false, right_click_delay: 0, miss_time: 0, is_destroying: false,
            destroy_pos: p(-1, -1, -1), destroying_item: Stack::default(), destroy_progress: 0.0, destroy_delay: 0, inventory_screen: false, menu: None, hit: None,
            ticks: 0, changes: Vec::new(), gm_ticks: 0, gm_destroying: false, gm_destroy_pos: p(0, 0, 0), gm_destroy_start: 0, gm_delayed: false,
            gm_delayed_pos: p(0, 0, 0), gm_delayed_start: 0, items_lost: 0,
        };
        s.refresh_entities();
        s
    }

    // ------------------------------------------------------------------ observation
    pub fn x(&self) -> f64 { self.pl.pos[0] }
    pub fn y(&self) -> f64 { self.pl.pos[1] }
    pub fn z(&self) -> f64 { self.pl.pos[2] }
    pub fn on_ground(&self) -> bool { self.pl.on_ground }
    pub fn yaw(&self) -> f32 { self.yaw }
    pub fn pitch(&self) -> f32 { self.pitch }
    pub fn eye(&self) -> [f64; 3] { self.pl.eye() }
    pub fn sneaking(&self) -> bool { self.pl.shift_key_down }
    pub fn screen_open(&self) -> bool { self.inventory_screen || self.menu.is_some() }
    pub fn container_open(&self) -> bool { self.menu.is_some() }
    pub fn is_breaking(&self) -> bool { self.is_destroying }
    /// Number of slots of the open screen (container slots + 36), as `menu().slots.size()`.
    pub fn menu_slots(&self) -> usize { match &self.menu { Some(m) => m.n + 36, None => 46 } }
    pub fn menu_slot(&self, slot: usize) -> Stack {
        match &self.menu {
            Some(m) if slot < m.n => self.w.containers.get(&m.parts[slot / 27]).map(|v| v[slot % 27]).unwrap_or_default(),
            _ => Stack::default(),
        }
    }

    // ------------------------------------------------------------------ inputs
    pub fn look(&mut self, yaw: f32, pitch: f32) { self.yaw = yaw; self.pitch = pitch.clamp(-90.0, 90.0); }
    pub fn keys(&mut self, forward: bool, back: bool, left: bool, right: bool, jump: bool, sneak: bool, sprint: bool) {
        self.keys = Input { forward, backward: back, left, right, jump, shift: sneak, sprint };
    }
    pub fn release_keys(&mut self) { self.keys = Input::default(); }
    pub fn sneak(&mut self, s: bool) { self.keys.shift = s; }
    pub fn jump(&mut self, j: bool) { self.keys.jump = j; }
    pub fn select_slot(&mut self, slot: usize) { self.want_slot = slot as i32; }
    pub fn click_use(&mut self) { self.use_click = true; }
    pub fn click_attack(&mut self) { self.attack_click = true; }
    pub fn hold_attack(&mut self, h: bool) { self.attack_held = h; }
    pub fn hold_use(&mut self, h: bool) { self.use_held = h; }
    pub fn open_inventory(&mut self) { self.want_open_inv = true; }
    pub fn close_screen(&mut self) { self.want_close = true; }
    pub fn click_slot(&mut self, slot: i32, button: i32, kind: u8) { self.screen_clicks.push_back((slot, button, kind)); }

    fn refresh_entities(&mut self) {
        self.w.entities.clear();
        self.w.entities.push(EntityView { bb: self.pl.bb, pos: self.pl.pos, is_player: true });
        for f in &self.w.falling {
            if f.removed { continue; }
            self.w.entities.push(EntityView { bb: Aabb::new(f.pos[0] - 0.49, f.pos[1], f.pos[2] - 0.49, f.pos[0] + 0.49, f.pos[1] + 0.98, f.pos[2] + 0.49), pos: f.pos, is_player: false });
        }
    }

    fn add_change(&mut self, q: P) {
        let s = self.w.get(q);
        self.changes.push((q.x, q.y, q.z, s));
    }

    /// What the crosshair of a player with the current eye position and the given view direction rests on (Oracle.pick).
    pub fn pick(&self, yaw: f32, pitch: f32) -> Option<Hit> {
        let e = self.pl.eye();
        let v = physics::view_vector(pitch, yaw);
        let r = self.pl.reach();
        physics::clip(&self.w, &self.pl.ctx(), e, [e[0] + v[0] * r, e[1] + v[1] * r, e[2] + v[2] * r])
    }

    // ------------------------------------------------------------------ the tick
    pub fn tick(&mut self) {
        self.ticks += 1;
        self.pl.yaw = self.yaw;
        self.pl.pitch = self.pitch;
        if self.right_click_delay > 0 { self.right_click_delay -= 1; }
        if self.want_slot >= 0 && self.want_slot as usize != self.pl.selected && !self.screen_open() {
            if (0..9).contains(&self.want_slot) { self.pl.selected = self.want_slot as usize; }
        }
        self.want_slot = -1;
        self.refresh_entities();
        if !self.screen_open() {
            self.hit = self.pick(self.pl.yaw, self.pl.pitch);
            self.handle_keybinds();
            if self.miss_time > 0 { self.miss_time -= 1; }
        } else {
            self.hit = None;
            self.miss_time = 10000;
            self.handle_screen();
        }
        self.use_click = false;
        self.attack_click = false;
        self.pl.pending_keys = if self.screen_open() { Input::default() } else { self.keys };
        self.pl.tick(&mut self.w);
        self.server_tick();
    }

    fn handle_keybinds(&mut self) {
        if self.want_open_inv {
            self.want_open_inv = false;
            self.inventory_screen = true;
            return;
        }
        self.want_close = false;
        let mut instantly = false;
        if self.attack_click { instantly = self.start_attack(); }
        if self.use_click { self.start_use_item(); }
        if self.use_held && self.right_click_delay == 0 { self.start_use_item(); }
        self.continue_attack(!instantly && self.attack_held);
    }

    fn handle_screen(&mut self) {
        self.want_open_inv = false;
        if self.want_close {
            self.want_close = false;
            self.screen_clicks.clear();
            if let Some(m) = self.menu.take() { rules::basic::menu_closed(&mut self.w, m.pos); }
            self.inventory_screen = false;
            return;
        }
        if let Some(c) = self.screen_clicks.pop_front() { self.container_click(c.0, c.1, c.2); }
    }

    // ---- Minecraft.startUseItem
    fn start_use_item(&mut self) {
        if self.is_destroying { return; }
        self.right_click_delay = 4;
        if let Some(h) = self.hit { self.handle_use_item_on(h); }
    }

    fn within_range(&self, q: P, buffer: f64) -> bool {
        let e = self.pl.eye();
        let d = |v: f64, lo: f64| -> f64 { if v < lo { lo - v } else if v > lo + 1.0 { v - lo - 1.0 } else { 0.0 } };
        let (dx, dy, dz) = (d(e[0], q.x as f64), d(e[1], q.y as f64), d(e[2], q.z as f64));
        let r = self.pl.reach() + buffer;
        dx * dx + dy * dy + dz * dz < r * r
    }

    // ---- ServerGamePacketListenerImpl.handleUseItemOn + ServerPlayerGameMode.useItemOn
    fn handle_use_item_on(&mut self, h: Hit) {
        if !self.within_range(h.pos, 1.0) { return; }
        let c = [h.pos.x as f64 + 0.5, h.pos.y as f64 + 0.5, h.pos.z as f64 + 0.5];
        if !((h.loc[0] - c[0]).abs() < 1.0000001 && (h.loc[1] - c[1]).abs() < 1.0000001 && (h.loc[2] - c[2]).abs() < 1.0000001) { return; }
        if h.pos.y <= self.w.max_y {
            self.use_item_on(h);
        }
        self.add_change(h.pos);
        self.add_change(h.pos.rel(h.face));
    }

    fn use_item_on(&mut self, h: Hit) {
        let state = self.w.get(h.pos);
        let held = self.pl.held();
        let have = held.count > 0;
        let suppress = self.pl.shift_key_down && have;
        let mut who = Clicker { pos: self.pl.pos, yaw: self.pl.yaw, pitch: self.pl.pitch, sneak: self.pl.shift_key_down, item: if have { held.item } else { 0 }, consumed: false };
        if !suppress {
            self.w.opened_menu = None;
            let r = rules::use_item_on(&mut self.w, state, h.pos, h.face, h.loc, &mut who);
            let mut done = r == Use::Consume;
            if !done && r == Use::TryEmptyHand {
                done = rules::use_without_item(&mut self.w, state, h.pos, h.face, h.loc, &mut who) == Use::Consume;
            }
            if who.consumed { self.pl.consume_held(); }
            if let Some((mp, n)) = self.w.opened_menu.take() { self.open_menu(mp, n as usize); }
            if done || r == Use::Fail { return; }
        }
        if !have { return; }
        // ItemStack.useOn -> BlockItem.place
        let ctx = PlaceCtx { item: held.item, hit_pos: h.pos, face: h.face, hit: h.loc, inside: h.inside, yaw: self.pl.yaw, pitch: self.pl.pitch, sneak: self.pl.shift_key_down, player_pos: self.pl.pos, replace_clicked: true }.resolve(&self.w);
        if let Some((pos, st)) = rules::predict_place(&self.w, &ctx) {
            if !rules::place::item_place_block(&mut self.w, &ctx, pos, st) { return; }
            let placed = self.w.get(pos);
            if same_block(placed, st) { rules::set_placed_by(&mut self.w, pos, placed, &ctx); }
            self.pl.consume_held();
        }
    }

    fn open_menu(&mut self, pos: P, n: usize) {
        let mut parts = vec![pos];
        if n == 54 {
            // the half of type RIGHT comes first (DoubleBlockCombiner)
            let st = self.w.get(pos);
            if let (Some(ty), Some(f)) = (get(st, crate::props::P_TYPE), get(st, crate::props::P_FACING)) {
                let facing = dir_of(f);
                // ChestBlock.getConnectedDirection: LEFT -> clockwise of facing, RIGHT -> counter-clockwise
                let other = if ty == crate::props::V_LEFT { pos.rel(clockwise(facing)) } else { pos.rel(counter_clockwise(facing)) };
                parts = if ty == crate::props::V_RIGHT { vec![pos, other] } else { vec![other, pos] };
            }
        }
        for q in &parts { self.w.containers.entry(*q).or_insert_with(|| vec![Stack::default(); 27]); }
        self.menu = Some(Menu { pos, n, parts });
    }

    // ---- Minecraft.startAttack (block case)
    fn start_attack(&mut self) -> bool {
        if self.miss_time > 0 { return false; }
        let mut instant = false;
        match self.hit {
            Some(h) if self.w.get(h.pos) != 0 => {
                self.start_destroy_block(h.pos, h.face);
                if self.w.get(h.pos) == 0 { instant = true; }
            }
            _ => { self.miss_time = 10; }
        }
        instant
    }

    fn continue_attack(&mut self, down: bool) {
        if !down { self.miss_time = 0; }
        if self.miss_time > 0 { return; }
        if down {
            if let Some(h) = self.hit {
                if self.w.get(h.pos) != 0 { self.continue_destroy_block(h.pos, h.face); }
                return;
            }
        }
        self.stop_destroy_block();
    }

    fn same_destroy_target(&self, q: P) -> bool {
        let held = self.pl.held();
        let h = if held.count > 0 { held.item } else { 0 };
        let d = if self.destroying_item.count > 0 { self.destroying_item.item } else { 0 };
        q == self.destroy_pos && h == d
    }

    /// BlockState.getDestroyProgress for the bare-handed player
    fn destroy_progress_of(&self, s: S) -> f32 {
        let tb = t();
        let hard = tb.destroy_time[s as usize];
        if hard == -1.0 { return 0.0; }
        let modifier = if tb.requires_tool[s as usize] { 100 } else { 30 };
        let mut speed = 1.0f32;
        if !self.pl.on_ground { speed /= 5.0; }
        speed / hard / modifier as f32
    }

    fn start_destroy_block(&mut self, q: P, face: Dir) -> bool {
        if !self.is_destroying || !self.same_destroy_target(q) {
            if self.is_destroying { let dp = self.destroy_pos; self.gm_action(2, dp); }
            let st = self.w.get(q);
            let instant = st != 0 && self.destroy_progress_of(st) >= 1.0;
            if !instant {
                self.is_destroying = true;
                self.destroy_pos = q;
                self.destroying_item = self.pl.held();
                self.destroy_progress = 0.0;
            }
            self.gm_action(0, q);
        }
        let _ = face;
        true
    }

    fn stop_destroy_block(&mut self) {
        if self.is_destroying {
            let dp = self.destroy_pos;
            self.gm_action(2, dp);
            self.is_destroying = false;
            self.destroy_progress = 0.0;
        }
    }

    fn continue_destroy_block(&mut self, q: P, face: Dir) -> bool {
        if self.destroy_delay > 0 { self.destroy_delay -= 1; return true; }
        if self.same_destroy_target(q) {
            let st = self.w.get(q);
            if st == 0 { self.is_destroying = false; return false; }
            self.destroy_progress += self.destroy_progress_of(st);
            if self.destroy_progress >= 1.0 {
                self.is_destroying = false;
                self.gm_action(1, q);
                self.destroy_progress = 0.0;
                self.destroy_delay = 5;
            }
            return true;
        }
        self.start_destroy_block(q, face)
    }

    /// ServerPlayerGameMode.handleBlockBreakAction: 0 start, 1 stop, 2 abort
    fn gm_action(&mut self, action: u8, q: P) {
        if !self.within_range(q, 1.0) { return; }
        if q.y > self.w.max_y { self.add_change(q); return; }
        match action {
            0 => {
                self.gm_destroy_start = self.gm_ticks;
                let mut progress = 1.0f32;
                let st = self.w.get(q);
                if st != 0 {
                    rules::attack(&mut self.w, st, q);
                    progress = self.destroy_progress_of(st);
                }
                if st != 0 && progress >= 1.0 {
                    self.destroy_and_ack(q);
                } else {
                    if self.gm_destroying { let dp = self.gm_destroy_pos; self.add_change(dp); }
                    self.gm_destroying = true;
                    self.gm_destroy_pos = q;
                }
            }
            1 => {
                if q == self.gm_destroy_pos {
                    let spent = self.gm_ticks - self.gm_destroy_start;
                    let st = self.w.get(q);
                    if st != 0 {
                        let pr = self.destroy_progress_of(st) * (spent + 1) as f32;
                        if pr >= 0.7 {
                            self.gm_destroying = false;
                            self.destroy_and_ack(q);
                            return;
                        }
                        if !self.gm_delayed {
                            self.gm_destroying = false;
                            self.gm_delayed = true;
                            self.gm_delayed_pos = q;
                            self.gm_delayed_start = self.gm_destroy_start;
                        }
                    }
                }
            }
            _ => { self.gm_destroying = false; }
        }
    }

    fn destroy_and_ack(&mut self, q: P) { if !self.destroy_block(q) { self.add_change(q); } }

    /// ServerPlayerGameMode.destroyBlock
    fn destroy_block(&mut self, q: P) -> bool {
        let st = self.w.get(q);
        use crate::kinds::Kind;
        if matches!(kind_of(st), Kind::CommandBlock | Kind::GameMasterBlock | Kind::JigsawBlock) {
            self.w.send_block_updated(q);
            return false;
        }
        rules::player_will_destroy(&mut self.w, q, st);
        let changed = self.w.remove_block(q, false);
        if changed { rules::destroyed(&mut self.w, q, st); }
        true
    }

    fn gm_tick(&mut self) {
        self.gm_ticks += 1;
        if self.gm_delayed {
            let st = self.w.get(self.gm_delayed_pos);
            if st == 0 {
                self.gm_delayed = false;
            } else {
                let pr = self.destroy_progress_of(st) * (self.gm_ticks - self.gm_delayed_start + 1) as f32;
                if pr >= 1.0 {
                    self.gm_delayed = false;
                    let q = self.gm_delayed_pos;
                    self.destroy_block(q);
                }
            }
        } else if self.gm_destroying {
            let st = self.w.get(self.gm_destroy_pos);
            if st == 0 { self.gm_destroying = false; }
        }
    }

    // ------------------------------------------------------------------ containers
    fn slot_get(&self, slot: usize) -> Stack {
        let n = self.menu.as_ref().map(|m| m.n).unwrap_or(0);
        if self.menu.is_some() {
            if slot < n { return self.menu_slot(slot); }
            let k = slot - n;
            return if k < 27 { self.pl.inv[9 + k] } else if k < 36 { self.pl.inv[k - 27] } else { Stack::default() };
        }
        // own inventory screen: 9..35 main, 36..44 hotbar
        if (9..36).contains(&slot) { self.pl.inv[slot] } else if (36..45).contains(&slot) { self.pl.inv[slot - 36] } else { Stack::default() }
    }
    fn slot_set(&mut self, slot: usize, st: Stack) {
        let st = if st.count == 0 { Stack::default() } else { st };
        if let Some(m) = &self.menu {
            let n = m.n;
            if slot < n {
                let part = m.parts[slot / 27];
                if let Some(v) = self.w.containers.get_mut(&part) { v[slot % 27] = st; }
                return;
            }
            let k = slot - n;
            if k < 27 { self.pl.inv[9 + k] = st; } else if k < 36 { self.pl.inv[k - 27] = st; }
            return;
        }
        if (9..36).contains(&slot) { self.pl.inv[slot] = st; } else if (36..45).contains(&slot) { self.pl.inv[slot - 36] = st; }
    }

    /// AbstractContainerMenu.moveItemStackTo; returns what is left of the stack and whether anything moved.
    fn move_item_stack_to(&mut self, mut st: Stack, start: usize, end: usize, backwards: bool) -> (Stack, bool) {
        let max = t().items[st.item as usize].stack;
        let mut changed = false;
        let order: Vec<usize> = if backwards { (start..end).rev().collect() } else { (start..end).collect() };
        if max > 1 {
            for &d in &order {
                if st.count == 0 { break; }
                let tg = self.slot_get(d);
                if tg.count > 0 && tg.item == st.item {
                    let total = tg.count as u32 + st.count as u32;
                    if total <= max as u32 {
                        st.count = 0;
                        self.slot_set(d, Stack { item: tg.item, count: total as u8 });
                        changed = true;
                    } else if tg.count < max {
                        st.count -= max - tg.count;
                        self.slot_set(d, Stack { item: tg.item, count: max });
                        changed = true;
                    }
                }
            }
        }
        if st.count > 0 {
            for &d in &order {
                if self.slot_get(d).count == 0 {
                    let n = st.count.min(max);
                    self.slot_set(d, Stack { item: st.item, count: n });
                    st.count -= n;
                    changed = true;
                    break;
                }
            }
        }
        (st, changed)
    }

    /// quickMoveStack of the open menu (chest menu; in the own inventory screen: between hotbar and main)
    fn quick_move_stack(&mut self, slot: usize) -> Stack {
        let st = self.slot_get(slot);
        if st.count == 0 { return Stack::default(); }
        let (left, moved) = if let Some(m) = &self.menu {
            let n = m.n;
            if slot < n { self.move_item_stack_to(st, n, n + 36, true) } else { self.move_item_stack_to(st, 0, n, false) }
        } else if (9..36).contains(&slot) {
            self.move_item_stack_to(st, 36, 45, false)
        } else {
            self.move_item_stack_to(st, 9, 36, false)
        };
        if !moved { return Stack::default(); }
        self.slot_set(slot, left);
        st
    }

    /// ServerGamePacketListenerImpl.handleContainerClick -> AbstractContainerMenu.clicked
    fn container_click(&mut self, slot: i32, button: i32, kind: u8) {
        if slot < 0 || slot as usize >= self.menu_slots() { return; }
        let slot = slot as usize;
        match kind {
            CLICK_QUICK_MOVE => {
                let mut clicked = self.quick_move_stack(slot);
                let mut guard = 0;
                while clicked.count > 0 && self.slot_get(slot).count > 0 && self.slot_get(slot).item == clicked.item && guard < 64 {
                    clicked = self.quick_move_stack(slot);
                    guard += 1;
                }
            }
            CLICK_SWAP => {
                if !(0..9).contains(&button) { return; }
                let b = button as usize;
                let source = self.pl.inv[b];
                let target = self.slot_get(slot);
                if source.count > 0 || target.count > 0 {
                    // (stack sizes never exceed what a slot takes here)
                    self.pl.inv[b] = if target.count > 0 { target } else { Stack::default() };
                    self.slot_set(slot, source);
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ the server's tick (Env.tick / ServerLevel.tick)
    fn server_tick(&mut self) {
        self.refresh_entities();
        self.w.game_time += 1;
        self.w.run_scheduled();
        // ServerChunkCache.broadcastChangedChunks
        for q in self.w.take_changes() { self.add_change(q); }
        self.w.run_block_events();
        // entities: the player's server-side tick, then falling blocks
        self.gm_tick();
        if let Some(m) = &self.menu {
            // AbstractContainerMenu.stillValid: the block is still there and within 8 blocks
            let (dx, dy, dz) = (m.pos.x as f64 + 0.5 - self.pl.pos[0], m.pos.y as f64 + 0.5 - (self.pl.pos[1] + self.pl.eye_height() as f64), m.pos.z as f64 + 0.5 - self.pl.pos[2]);
            if self.w.get(m.pos) == 0 || dx * dx + dy * dy + dz * dz > 64.0 { let mp = m.pos; self.menu = None; rules::basic::menu_closed(&mut self.w, mp); }
        }
        self.tick_falling();
        rules::redstone::tick_block_entities(&mut self.w);
    }

    /// FallingBlockEntity.tick for every falling block
    fn tick_falling(&mut self) {
        if self.w.falling.is_empty() { return; }
        let mut i = 0;
        while i < self.w.falling.len() {
            let mut f: FallingBlock = self.w.falling[i].clone();
            if !f.removed {
                f.time += 1;
                f.vel[1] -= 0.04;
                let bb = Aabb::new(f.pos[0] - 0.49f32 as f64, f.pos[1], f.pos[2] - 0.49f32 as f64, f.pos[0] + 0.49f32 as f64, f.pos[1] + 0.98f32 as f64, f.pos[2] + 0.49f32 as f64);
                let ctx = Ctx { descending: false, bottom: f.pos[1], entity: true, falling_block: true, holding_scaffolding: false };
                let mv = physics::collide(&self.w, &ctx, &bb, f.vel, f.on_ground, 0.0);
                let ml = mv[0] * mv[0] + mv[1] * mv[1] + mv[2] * mv[2];
                let dl = f.vel[0] * f.vel[0] + f.vel[1] * f.vel[1] + f.vel[2] * f.vel[2];
                if ml > 1.0e-7 || dl - ml < 1.0e-7 { f.pos = [f.pos[0] + mv[0], f.pos[1] + mv[1], f.pos[2] + mv[2]]; }
                let vcol = f.vel[1] != mv[1];
                f.on_ground = vcol && f.vel[1] < 0.0;
                if (f.vel[0] - mv[0]).abs() >= 1.0e-5 { f.vel[0] = 0.0; }
                if (f.vel[2] - mv[2]).abs() >= 1.0e-5 { f.vel[2] = 0.0; }
                if vcol { f.vel[1] = 0.0; }
                let q = p(floor(f.pos[0]), floor(f.pos[1]), floor(f.pos[2]));
                if !f.on_ground {
                    if f.time > 100 && (q.y <= self.w.ground_top - 4 || q.y > self.w.max_y) || f.time > 600 { f.removed = true; self.items_lost += 1; }
                } else {
                    let cur = self.w.get(q);
                    f.vel = [f.vel[0] * 0.7, f.vel[1] * -0.5, f.vel[2] * 0.7];
                    if !crate::is_blk!(cur, "moving_piston") {
                        f.removed = true;
                        let tb = t();
                        let may_replace = tb.replaceable[cur as usize];
                        let below = self.w.get(q.below());
                        let would_fall = below == 0 || tb.replaceable[below as usize] || crate::in_tag!(below, "fire");
                        let survive = rules::can_survive(&self.w, f.state, q) && !would_fall;
                        if may_replace && survive {
                            if self.w.set_block(q, f.state, F_ALL) {
                                self.add_change(q);
                                rules::basic::on_land(&mut self.w, q, f.state, cur);
                            } else { self.items_lost += 1; }
                        } else {
                            // (the block breaks and would lie there as an item; items are not modelled)
                            self.items_lost += 1;
                            self.w.unmodelled += 1;
                        }
                    }
                }
                f.vel = [f.vel[0] * 0.98f32 as f64, f.vel[1] * 0.98f32 as f64, f.vel[2] * 0.98f32 as f64];
            }
            // (the block's own tick may have added entities: write back by index)
            self.w.falling[i] = f;
            i += 1;
        }
        self.w.falling.retain(|f| !f.removed);
    }

    // ------------------------------------------------------------------ administrator actions
    pub fn admin_teleport(&mut self, x: f64, y: f64, z: f64) { self.pl.teleport(x, y, z); }
}
