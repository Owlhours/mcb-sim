//! The bot's body: the vanilla player physics as the Java version runs them (a server-side player driven by key
//! states the way the client's LocalPlayer drives it: `BotPlayer` + Player/LivingEntity/Entity).
#![allow(dead_code)]
use crate::geom::*;
use crate::kinds::Kind;
use crate::physics::*;
use crate::rules;
use crate::tables::*;
use crate::world::{Stack, World};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Input {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub shift: bool,
    pub sprint: bool,
}

pub const POSE_STANDING: u8 = 0;
pub const POSE_CROUCHING: u8 = 1;
pub const POSE_SWIMMING: u8 = 2;
const WIDTH: f32 = 0.6;
const HEIGHTS: [f32; 3] = [1.8, 1.5, 0.6];
const EYES: [f32; 3] = [1.62, 1.27, 0.4];

pub struct Player {
    pub pos: [f64; 3],
    pub vel: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub horizontal_collision: bool,
    pub vertical_collision: bool,
    pub vertical_collision_below: bool,
    pub minor_horizontal_collision: bool,
    pub pose: u8,
    pub bb: Aabb,
    pub shift_key_down: bool,
    pub sprinting: bool,
    jumping: bool,
    xxa: f32,
    zza: f32,
    no_jump_delay: i32,
    stuck_speed_multiplier: [f64; 3],
    main_supporting: Option<P>,
    on_ground_no_blocks: bool,
    block_pos: P,
    in_block_state: Option<S>,
    was_in_powder_snow: bool,
    is_in_powder_snow: bool,
    // client-side input state (KeyboardInput / LocalPlayer)
    pub pending_keys: Input,
    key_presses: Input,
    move_vector: [f32; 2],
    last_sent_input: Input,
    crouching: bool,
    sprint_trigger_time: i32,
    // inventory: 0-8 hotbar, 9-35 main
    pub inv: [Stack; 36],
    // ---- health (LivingEntity / Player / ServerPlayer) and food (FoodData)
    pub health: f32,
    pub food_level: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    food_timer: i32,
    pub invulnerable_time: i32,
    last_hurt: f32,
    /// Entity.tickCount: one per server-side tick of the player
    pub tick_count: i64,
    pub death_time: i32,
    /// ServerGamePacketListenerImpl.waitingForRespawn: set by ServerPlayer.die; from then on the server ignores the
    /// client's use / break / input packets and the player cannot be hurt (nothing here ever asks to respawn)
    pub died: bool,
    /// Entity.isRemoved: twenty ticks after the death the body leaves the world and stops being simulated
    pub removed: bool,
    pub selected: usize,
    /// Entity.movementThisTick: (from, to, the movement asked for)
    movements: Vec<([f64; 3], [f64; 3], [f64; 3])>,
    tick_start_pos: [f64; 3],
    piston_deltas: [f64; 3],
    piston_deltas_time: i64,
    /// Entity.moveDist / flyDist / nextStep (step sounds and STEP game events)
    move_dist: f32,
    fly_dist: f32,
    next_step: f32,
}

fn scaffolding_item() -> u16 {
    static V: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *V.get_or_init(|| *t().item_by_name.get("scaffolding").unwrap())
}

fn make_bb(pos: [f64; 3], pose: u8) -> Aabb {
    let w = (WIDTH / 2.0f32) as f64;
    Aabb::new(pos[0] - w, pos[1], pos[2] - w, pos[0] + w, pos[1] + HEIGHTS[pose as usize] as f64, pos[2] + w)
}

impl Player {
    pub fn new(x: f64, y: f64, z: f64) -> Player {
        let pos = [x, y, z];
        Player {
            pos, vel: [0.0; 3], yaw: 0.0, pitch: 0.0, on_ground: false, horizontal_collision: false, vertical_collision: false, vertical_collision_below: false,
            minor_horizontal_collision: false, pose: POSE_STANDING, bb: make_bb(pos, POSE_STANDING), shift_key_down: false, sprinting: false, jumping: false,
            xxa: 0.0, zza: 0.0, no_jump_delay: 0, stuck_speed_multiplier: [0.0; 3], main_supporting: None, on_ground_no_blocks: false,
            block_pos: p(floor(x), floor(y), floor(z)), in_block_state: None, was_in_powder_snow: false, is_in_powder_snow: false,
            pending_keys: Input::default(), key_presses: Input::default(), move_vector: [0.0; 2], last_sent_input: Input::default(), crouching: false,
            sprint_trigger_time: 0, inv: [Stack::default(); 36], health: 20.0, food_level: 20, saturation: 5.0, exhaustion: 0.0, food_timer: 0, invulnerable_time: 0, last_hurt: 0.0, tick_count: 0, death_time: 0, died: false, removed: false, selected: 0, movements: Vec::new(), tick_start_pos: pos, piston_deltas: [0.0; 3], piston_deltas_time: -1,
            move_dist: 0.0, fly_dist: 0.0, next_step: 1.0,
        }
    }

    pub fn eye_height(&self) -> f32 { EYES[self.pose as usize] }
    pub fn eye(&self) -> [f64; 3] { [self.pos[0], self.pos[1] + self.eye_height() as f64, self.pos[2]] }
    pub fn held(&self) -> Stack { self.inv[self.selected] }
    pub fn reach(&self) -> f64 { 4.5 }

    pub fn ctx(&self) -> Ctx {
        Ctx { descending: self.shift_key_down, bottom: self.pos[1], entity: true, falling_block: false, holding_scaffolding: self.held().count > 0 && self.held().item == scaffolding_item(), noclip: None }
    }

    fn set_pos(&mut self, x: f64, y: f64, z: f64) {
        if self.pos[0] != x || self.pos[1] != y || self.pos[2] != z {
            self.pos = [x, y, z];
            let np = p(floor(x), floor(y), floor(z));
            if np != self.block_pos { self.block_pos = np; self.in_block_state = None; }
        }
        self.bb = make_bb(self.pos, self.pose);
    }

    fn in_block_state(&mut self, w: &World) -> S {
        if self.in_block_state.is_none() { self.in_block_state = Some(w.get(self.block_pos)); }
        self.in_block_state.unwrap()
    }

    fn can_fit(&self, w: &World, pose: u8) -> bool {
        no_collision(w, &self.ctx(), &make_bb(self.pos, pose).deflate(1.0e-7))
    }

    /// Entity.getOnPos(offset)
    fn on_pos(&self, w: &World, offset: f32) -> P {
        if let Some(sp) = self.main_supporting {
            if !(offset > 1.0e-5f32) { return sp; }
            let below = w.get(sp);
            let keep = (offset <= 0.5 && crate::in_tag!(below, "fences")) || crate::in_tag!(below, "walls") || kind_of(below) == Kind::FenceGateBlock;
            if keep { sp } else { p(sp.x, floor(self.pos[1] - offset as f64), sp.z) }
        } else {
            p(floor(self.pos[0]), floor(self.pos[1] - offset as f64), floor(self.pos[2]))
        }
    }

    fn block_speed_factor(&self, w: &World) -> f32 {
        let here = t().speed_factor[w.get(self.block_pos) as usize];
        if here == 1.0 { t().speed_factor[w.get(self.on_pos(w, 0.500001f32)) as usize] } else { here }
    }
    fn block_jump_factor(&self, w: &World) -> f32 {
        let here = t().jump_factor[w.get(self.block_pos) as usize];
        if here == 1.0 { t().jump_factor[w.get(self.on_pos(w, 0.500001f32)) as usize] } else { here }
    }

    /// Player.getSpeed: the movement speed attribute (0.1 base, sprinting x1.3)
    fn speed(&self) -> f32 {
        let base = 0.1f32 as f64;
        if self.sprinting { (base * (1.0 + 0.3f32 as f64)) as f32 } else { base as f32 }
    }

    /// LivingEntity.onClimbable
    fn on_climbable(&mut self, w: &World) -> bool {
        let s = self.in_block_state(w);
        if t().climbable[s as usize] { return true; }
        if kind_of(s) == Kind::TrapDoorBlock && is_true(s, crate::props::P_OPEN) {
            let below = w.get(self.block_pos.below());
            if crate::is_blk!(below, "ladder") && get(below, crate::props::P_FACING) == get(s, crate::props::P_FACING) { return true; }
        }
        false
    }

    // ------------------------------------------------------------------ one tick (ServerPlayer.doTick)
    pub fn tick(&mut self, w: &mut World) {
        // Entity.baseTick
        self.tick_start_pos = self.pos;
        self.movements.clear();
        self.in_block_state = None;
        self.was_in_powder_snow = self.is_in_powder_snow;
        self.is_in_powder_snow = false;
        // ---- LivingEntity.baseTick: suffocation, then the death timer
        if self.health > 0.0 && self.is_in_wall(w) { self.hurt(w, 1.0, 0.0); }
        if self.health <= 0.0 {
            self.death_time += 1;
            if self.death_time >= 20 { self.removed = true; }
        }
        // LivingEntity.tick: a removed entity has no aiStep; ServerPlayer.doTick still ticks the food data
        if self.removed { self.food_tick(); return; }
        // ---- BotPlayer.aiStep: LocalPlayer.aiStep up to super.aiStep()
        if self.sprint_trigger_time > 0 { self.sprint_trigger_time -= 1; }
        let was_shift = self.key_presses.shift;
        let had_forward = self.move_vector[1] > 1.0e-5f32;
        self.crouching = self.can_fit(w, POSE_CROUCHING) && (self.shift_key_down || !self.can_fit(w, POSE_STANDING));
        self.key_presses = self.pending_keys;
        let imp = |a: bool, b: bool| -> f32 { if a == b { 0.0 } else if a { 1.0 } else { -1.0 } };
        let (mx, my) = (imp(self.key_presses.left, self.key_presses.right), imp(self.key_presses.forward, self.key_presses.backward));
        let dist = (mx * mx + my * my).sqrt();
        self.move_vector = if dist < 1.0e-4f32 { [0.0, 0.0] } else { [mx / dist, my / dist] };
        if self.last_sent_input != self.key_presses {
            // handlePlayerInput: ignored while the server waits for a respawn
            if !self.died { self.shift_key_down = self.key_presses.shift; }
            self.last_sent_input = self.key_presses;
        }
        let wd = WIDTH as f64 * 0.35;
        self.move_towards_closest_space(w, self.pos[0] - wd, self.pos[2] + wd);
        self.move_towards_closest_space(w, self.pos[0] - wd, self.pos[2] - wd);
        self.move_towards_closest_space(w, self.pos[0] + wd, self.pos[2] - wd);
        self.move_towards_closest_space(w, self.pos[0] + wd, self.pos[2] + wd);
        if was_shift || self.key_presses.backward { self.sprint_trigger_time = 0; }
        let has_forward = self.move_vector[1] > 1.0e-5f32;
        let moving_slowly = self.crouching || self.pose == POSE_SWIMMING;
        if !self.sprinting && has_forward && !moving_slowly {
            if !had_forward {
                if self.sprint_trigger_time > 0 { self.sprinting = true; } else { self.sprint_trigger_time = 7; }
            }
            if self.key_presses.sprint { self.sprinting = true; }
        }
        if self.sprinting && (!has_forward || (self.horizontal_collision && !self.minor_horizontal_collision)) { self.sprinting = false; }
        // ---- Player.aiStep: ServerPlayer.tickRegeneration (peaceful difficulty)
        if self.tick_count % 20 == 0 {
            if self.health < 20.0 { self.heal(1.0); }
            if self.saturation < 20.0 { self.saturation += 1.0; }
        }
        if self.tick_count % 10 == 0 && self.food_level < 20 { self.food_level += 1; }
        // ---- LivingEntity.aiStep
        if self.no_jump_delay > 0 { self.no_jump_delay -= 1; }
        if self.vel[0] * self.vel[0] + self.vel[2] * self.vel[2] < 9.0e-6 { self.vel[0] = 0.0; self.vel[2] = 0.0; }
        if self.vel[1].abs() < 0.003 { self.vel[1] = 0.0; }
        // applyInput (LocalPlayer.applyInput + modifyInput)
        let mv = self.modify_input(self.move_vector, moving_slowly);
        self.xxa = mv[0];
        self.zza = mv[1];
        self.jumping = self.key_presses.jump;
        // isImmobile: a dying body takes no input
        if self.health <= 0.0 { self.jumping = false; self.xxa = 0.0; self.zza = 0.0; }
        if self.jumping {
            if self.on_ground && self.no_jump_delay == 0 {
                self.jump_from_ground(w);
                self.no_jump_delay = 10;
            }
        } else {
            self.no_jump_delay = 0;
        }
        self.travel_in_air(w);
        // the blocks asked below (pressure plates, tripwire) look for entities by their current boxes
        for e in w.entities.iter_mut() { if e.is_player { e.bb = self.bb; e.pos = self.pos; } }
        self.apply_effects_from_blocks(w);
        // (freezing in powder snow - ticksFrozen, the frost slow-down, freeze damage - is not modelled)
        if self.is_in_powder_snow && !self.was_in_powder_snow { w.unmodelled += 1; }
        // ---- Player.tick tail: updatePlayerPose
        self.update_pose(w);
        // ---- ServerPlayer.doTick: foodData.tick
        self.food_tick();
    }

    /// ServerPlayer.tick, the part that concerns the body: called once per server-side tick of the player.
    pub fn server_side_tick(&mut self) {
        self.tick_count += 1;
        if self.invulnerable_time > 0 { self.invulnerable_time -= 1; }
    }

    pub fn is_hurt(&self) -> bool { self.health > 0.0 && self.health < 20.0 }
    /// LivingEntity.heal
    fn heal(&mut self, amount: f32) { if self.health > 0.0 { self.health = (self.health + amount).clamp(0.0, 20.0); } }
    /// Player.causeFoodExhaustion -> FoodData.addExhaustion
    pub fn cause_food_exhaustion(&mut self, amount: f32) { self.exhaustion = (self.exhaustion + amount).min(40.0); }

    /// FoodData.tick (peaceful difficulty, natural regeneration on)
    fn food_tick(&mut self) {
        if self.exhaustion > 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 { self.saturation = (self.saturation - 1.0).max(0.0); }
        }
        if self.saturation > 0.0 && self.is_hurt() && self.food_level >= 20 {
            self.food_timer += 1;
            if self.food_timer >= 10 {
                let spent = self.saturation.min(6.0);
                self.heal(spent / 6.0);
                self.cause_food_exhaustion(spent);
                self.food_timer = 0;
            }
        } else if self.food_level >= 18 && self.is_hurt() {
            self.food_timer += 1;
            if self.food_timer >= 80 {
                self.heal(1.0);
                self.cause_food_exhaustion(6.0);
                self.food_timer = 0;
            }
        } else {
            self.food_timer = 0;
        }
    }

    /// Entity.isInWall: the eyes are inside a suffocating block's collision shape.
    fn is_in_wall(&self, w: &World) -> bool {
        let e = self.eye();
        let cw = (WIDTH * 0.8f32) as f64;
        let bx = Aabb::new(e[0] - cw / 2.0, e[1] - 1.0e-6 / 2.0, e[2] - cw / 2.0, e[0] + cw / 2.0, e[1] + 1.0e-6 / 2.0, e[2] + cw / 2.0);
        let ctx = Ctx { descending: false, bottom: 0.0, entity: false, falling_block: false, holding_scaffolding: false, noclip: None };
        for x in floor(bx.min[0])..=floor(bx.max[0]) { for y in floor(bx.min[1])..=floor(bx.max[1]) { for z in floor(bx.min[2])..=floor(bx.max[2]) {
            let q = p(x, y, z);
            let s = w.get(q);
            if s == 0 || t().air[s as usize] || !crate::physics::is_suffocating(s) { continue; }
            let (sh, off) = crate::physics::col_shape(s, q, &ctx);
            if !sh.empty && sh.intersects(off, &bx) { return true; }
        } } }
        false
    }

    /// Entity.hurt for the damage sources this world has (campfire, magma, cactus, suffocation): ServerPlayer /
    /// Player / LivingEntity.hurtServer without armour, effects, absorption or knockback (all these sources are in
    /// the `no_knockback` tag and none scales with difficulty). `food_exhaustion` is the damage type's.
    pub fn hurt(&mut self, w: &mut World, damage: f32, food_exhaustion: f32) -> bool {
        if self.died || self.health <= 0.0 || damage == 0.0 { return false; }
        if self.invulnerable_time > 10 {
            if damage <= self.last_hurt { return false; }
            let d = damage - self.last_hurt;
            self.actually_hurt(w, d, food_exhaustion);
            self.last_hurt = damage;
        } else {
            self.last_hurt = damage;
            self.invulnerable_time = 20;
            self.actually_hurt(w, damage, food_exhaustion);
        }
        if self.health <= 0.0 { self.die(w); }
        true
    }
    /// Player.actuallyHurt
    fn actually_hurt(&mut self, w: &World, damage: f32, food_exhaustion: f32) {
        if damage != 0.0 {
            self.cause_food_exhaustion(food_exhaustion);
            self.health = (self.health - damage).clamp(0.0, 20.0);
            rules::vibrations::emit(w, rules::vibrations::Ev::EntityDamage, self.pos, rules::vibrations::Ctx::player(self.shift_key_down));
        }
    }
    /// ServerPlayer.die: everything carried is dropped (the items lie about as entities, which are not modelled:
    /// they scatter at random) and the connection waits for a respawn request that never comes.
    fn die(&mut self, w: &mut World) {
        rules::vibrations::emit(w, rules::vibrations::Ev::EntityDie, self.pos, rules::vibrations::Ctx::player(self.shift_key_down));
        self.inv = [Stack::default(); 36];
        self.died = true;
        w.player_died = true;
    }

    fn modify_input(&self, v: [f32; 2], moving_slowly: bool) -> [f32; 2] {
        if v[0] * v[0] + v[1] * v[1] == 0.0 { return v; }
        let mut x = v[0] * 0.98f32;
        let mut y = v[1] * 0.98f32;
        if moving_slowly { let k = 0.3f64 as f32; x *= k; y *= k; }
        let len = (x * x + y * y).sqrt();
        if len <= 0.0 { return [x, y]; }
        let (dx, dy) = (x * (1.0f32 / len), y * (1.0f32 / len));
        let (ax, ay) = (dx.abs(), dy.abs());
        let r = if ay > ax { ax / ay } else { ay / ax };
        let d = ((1.0f32 + r * r) as f64).sqrt() as f32;
        let k = (len * d).min(1.0f32);
        [dx * k, dy * k]
    }

    fn suffocates_at(&self, w: &World, q: P) -> bool {
        let bx = Aabb::new(q.x as f64, self.bb.min[1], q.z as f64, q.x as f64 + 1.0, self.bb.max[1], q.z as f64 + 1.0).deflate(1.0e-7);
        collides_with_suffocating(w, &self.ctx(), &bx)
    }

    fn move_towards_closest_space(&mut self, w: &World, x: f64, z: f64) {
        let q = p(floor(x), floor(self.pos[1]), floor(z));
        if !self.suffocates_at(w, q) { return; }
        let (fx, fz) = (x - q.x as f64, z - q.z as f64);
        let mut best: Option<Dir> = None;
        let mut bd = f64::MAX;
        for d in [WEST, EAST, NORTH, SOUTH] {
            let a = if axis(d) == AXIS_X { fx } else { fz };
            let dist = if is_positive(d) { 1.0 - a } else { a };
            if dist < bd && !self.suffocates_at(w, q.rel(d)) { bd = dist; best = Some(d); }
        }
        if let Some(d) = best {
            if axis(d) == AXIS_X { self.vel[0] = 0.1 * STEP[d as usize].0 as f64; } else { self.vel[2] = 0.1 * STEP[d as usize].2 as f64; }
        }
    }

    fn jump_from_ground(&mut self, w: &World) {
        // ServerPlayer.jumpFromGround
        self.cause_food_exhaustion(if self.sprinting { 0.2 } else { 0.05 });
        let jp = (0.42f32 as f64) as f32 * 1.0f32 * self.block_jump_factor(w) + 0.0f32;
        if !(jp <= 1.0e-5f32) {
            self.vel[1] = (jp as f64).max(self.vel[1]);
            if self.sprinting {
                let ang = self.yaw * (std::f64::consts::PI / 180.0) as f32;
                self.vel[0] += -msin(ang as f64) as f64 * 0.2;
                self.vel[2] += mcos(ang as f64) as f64 * 0.2;
            }
        }
    }

    fn travel_in_air(&mut self, w: &mut World) {
        let below = self.on_pos(w, 0.500001f32);
        let block_friction: f32 = if self.on_ground { (1.0f32 - (1.0f32 - t().friction[w.get(below) as usize]) * 1.0f32).clamp(0.0, 1.0) } else { 1.0 };
        // handleRelativeFrictionAndCalculateMovement
        let sp = if self.on_ground {
            if block_friction as f64 > 0.6 { self.speed() * (0.21600002f32 / (block_friction * block_friction * block_friction)) } else { self.speed() }
        } else if self.sprinting { 0.025999999f32 } else { 0.02f32 };
        // moveRelative
        let (ix, iz) = (self.xxa as f64, self.zza as f64);
        let len = ix * ix + iz * iz;
        if len >= 1.0e-7 {
            let (mut nx, mut nz) = (ix, iz);
            if len > 1.0 { let l = len.sqrt(); nx /= l; nz /= l; }
            nx *= sp as f64; nz *= sp as f64;
            let ang = self.yaw * (std::f64::consts::PI / 180.0) as f32;
            let (sn, cs) = (msin(ang as f64) as f64, mcos(ang as f64) as f64);
            self.vel[0] += nx * cs - nz * sn;
            self.vel[2] += nz * cs + nx * sn;
        }
        // handleOnClimbable
        if self.on_climbable(w) {
            let m = 0.15f32 as f64;
            self.vel[0] = self.vel[0].clamp(-m, m);
            self.vel[2] = self.vel[2].clamp(-m, m);
            let mut yd = self.vel[1].max(-m);
            let ins = self.in_block_state(w);
            if yd < 0.0 && !t().scaffolding[ins as usize] && self.shift_key_down { yd = 0.0; }
            self.vel[1] = yd;
        }
        let d = self.vel;
        self.do_move(w, d);
        let mut movement = self.vel;
        if (self.horizontal_collision || self.jumping) && self.on_climbable(w) { movement[1] = 0.2; }
        let my = movement[1] - 0.08;
        let air_drag = 1.0f32 - (1.0f32 - 0.91f32) * 1.0f32;
        let friction = block_friction * air_drag;
        let vfriction = 1.0f32 - (1.0f32 - 0.98f32) * 1.0f32;
        self.vel = [movement[0] * friction as f64, my * vfriction as f64, movement[2] * friction as f64];
    }

    fn can_fall_at_least(&self, w: &World, dx: f64, dz: f64, min_height: f64) -> bool {
        let b = &self.bb;
        no_collision(w, &self.ctx(), &Aabb::new(b.min[0] + 1.0e-7 + dx, b.min[1] - min_height - 1.0e-7, b.min[2] + 1.0e-7 + dz, b.max[0] - 1.0e-7 + dx, b.min[1], b.max[2] - 1.0e-7 + dz))
    }

    /// Player.maybeBackOffFromEdge
    fn back_off_from_edge(&self, w: &World, delta: [f64; 3]) -> [f64; 3] {
        let max_down = 0.6f32;
        let above_ground = self.on_ground || (0.0 < max_down as f64 && !self.can_fall_at_least(w, 0.0, 0.0, max_down as f64 - 0.0));
        if !(delta[1] > 0.0) && self.shift_key_down && above_ground {
            let (mut dx, mut dz) = (delta[0], delta[2]);
            let sgn = |v: f64| if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { v };
            let (sx, sz) = (sgn(dx) * 0.05, sgn(dz) * 0.05);
            let md = max_down as f64;
            while dx != 0.0 && self.can_fall_at_least(w, dx, 0.0, md) {
                if dx.abs() <= 0.05 { dx = 0.0; break; }
                dx -= sx;
            }
            while dz != 0.0 && self.can_fall_at_least(w, 0.0, dz, md) {
                if dz.abs() <= 0.05 { dz = 0.0; break; }
                dz -= sz;
            }
            while dx != 0.0 && dz != 0.0 && self.can_fall_at_least(w, dx, dz, md) {
                if dx.abs() <= 0.05 { dx = 0.0; } else { dx -= sx; }
                if dz.abs() <= 0.05 { dz = 0.0; } else { dz -= sz; }
            }
            [dx, delta[1], dz]
        } else {
            delta
        }
    }

    /// Entity.move(MoverType.SELF, delta)
    fn do_move(&mut self, w: &mut World, delta: [f64; 3]) { self.do_move_as(w, delta, None) }

    /// Entity.move; `piston`: moved by a piston travelling in that direction (MoverType.PISTON), else by itself.
    fn do_move_as(&mut self, w: &mut World, mut delta: [f64; 3], piston: Option<Dir>) {
        if piston.is_some() {
            // limitPistonMovement: at most 0.51 per axis and tick
            if delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2] > 1.0e-7 {
                if w.game_time != self.piston_deltas_time { self.piston_deltas = [0.0; 3]; self.piston_deltas_time = w.game_time; }
                let a = if delta[0] != 0.0 { 0 } else if delta[1] != 0.0 { 1 } else if delta[2] != 0.0 { 2 } else { 3 };
                if a == 3 { return; }
                let mn = (delta[a] + self.piston_deltas[a]).clamp(-0.51, 0.51);
                let amount = mn - self.piston_deltas[a];
                self.piston_deltas[a] = mn;
                delta = [0.0; 3];
                if amount.abs() <= 1.0e-5f32 as f64 { return; }
                delta[a] = amount;
            }
            if delta == [0.0; 3] { return; }
        }
        let ssm = self.stuck_speed_multiplier;
        if ssm[0] * ssm[0] + ssm[1] * ssm[1] + ssm[2] * ssm[2] > 1.0e-7 {
            if piston.is_none() { delta = [delta[0] * ssm[0], delta[1] * ssm[1], delta[2] * ssm[2]]; }
            self.stuck_speed_multiplier = [0.0; 3];
            self.vel = [0.0; 3];
        }
        if piston.is_none() { delta = self.back_off_from_edge(w, delta); }
        let mut ctx = self.ctx();
        ctx.noclip = piston;
        let movement = collide(w, &ctx, &self.bb, delta, self.on_ground, 0.6f32);
        let mlen = movement[0] * movement[0] + movement[1] * movement[1] + movement[2] * movement[2];
        let dlen = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
        if mlen > 1.0e-7 || dlen - mlen < 1.0e-7 {
            let from = self.pos;
            let to = [self.pos[0] + movement[0], self.pos[1] + movement[1], self.pos[2] + movement[2]];
            self.movements.push((from, to, delta));
            self.set_pos(to[0], to[1], to[2]);
        }
        let eq = |a: f64, b: f64| (b - a).abs() < 1.0e-5f32 as f64;
        let xc = !eq(delta[0], movement[0]);
        let zc = !eq(delta[2], movement[2]);
        self.horizontal_collision = xc || zc;
        let moved_vertically = delta[1].abs() > 0.0;
        if moved_vertically {
            self.vertical_collision = delta[1] != movement[1];
            self.vertical_collision_below = self.vertical_collision && delta[1] < 0.0;
            self.on_ground = self.vertical_collision_below;
            self.check_supporting_block(w, Some(movement));
        }
        self.minor_horizontal_collision = false;
        let effect_pos = self.on_pos(w, 0.2f32);
        let effect_state = w.get(effect_pos);
        if (moved_vertically && self.vertical_collision) || self.horizontal_collision {
            // restituteMovementAfterCollisions (the player itself has no bounciness)
            let cur = self.vel;
            let mut after = cur;
            if xc { after[0] = -cur[0] * 0.0; }
            if zc { after[2] = -cur[2] * 0.0; }
            if self.vertical_collision {
                let mut restitution = 0.0f64;
                if self.vertical_collision_below {
                    let b = if crate::is_blk!(effect_state, "slime_block") { 1.0f32 } else if kind_of(effect_state) == Kind::BedBlock { 0.75f32 } else { 0.0f32 };
                    restitution = if !(-cur[1] < 0.08) && !self.shift_key_down && !crate::is_blk!(effect_state, "honey_block") { (b as f64).max(0.0) } else { 0.0 };
                }
                let (gc, drag) = if restitution > 0.0 {
                    let portion = movement[1] / cur[1];
                    let ad = (1.0f32 - (1.0f32 - 0.98f32) * 1.0f32) as f64;
                    (portion * 0.08, 1.0 + portion * (ad - 1.0))
                } else { (0.0, 1.0) };
                after[1] = (gc - cur[1]) * drag * restitution;
                if restitution > 0.0 {
                    // bounced: Entity.gameEvent(BOUNCE)
                    rules::vibrations::emit(w, rules::vibrations::Ev::Bounce, self.pos, rules::vibrations::Ctx::player(self.shift_key_down));
                }
            }
            self.vel = after;
        }
        // Player.getMovementEmission: nothing while sneaking on the ground
        if !(self.on_ground && self.shift_key_down) {
            self.apply_movement_emission(w, movement, effect_pos, effect_state);
        }
        let f = self.block_speed_factor(w) as f64;
        self.vel[0] *= f;
        self.vel[2] *= f;
    }

    /// Entity.isStateClimbable
    fn state_climbable(s: S) -> bool { crate::in_tag!(s, "climbable") || crate::is_blk!(s, "powder_snow") }

    /// Entity.applyMovementEmissionAndPlaySound (emission ALL: the sounds are not modelled)
    fn apply_movement_emission(&mut self, w: &mut World, movement: [f64; 3], effect_pos: P, effect_state: S) {
        let moved = ((movement[0] * movement[0] + movement[1] * movement[1] + movement[2] * movement[2]).sqrt() * 0.6f32 as f64) as f32;
        let hmoved = ((movement[0] * movement[0] + movement[2] * movement[2]).sqrt() * 0.6f32 as f64) as f32;
        let supporting_pos = self.on_pos(w, 1.0e-5f32);
        let supporting_state = w.get(supporting_pos);
        let climbing = Self::state_climbable(supporting_state);
        self.move_dist += if climbing { moved } else { hmoved };
        self.fly_dist += moved;
        let tb = t();
        if self.move_dist > self.next_step && !tb.air[supporting_state as usize] {
            let only_effect = supporting_pos == effect_pos;
            let mut produced = self.vibration_from_block(w, effect_state, only_effect, movement);
            if !only_effect { produced |= self.vibration_from_block(w, supporting_state, true, movement); }
            if produced { self.next_step = (self.move_dist as i32 + 1) as f32; }
            // (isInWater: never)
        }
        // (processFlappingMovement: players do not flap)
    }

    /// Entity.vibrationAndSoundEffectsFromBlock
    fn vibration_from_block(&mut self, w: &mut World, s: S, vibrate: bool, movement: [f64; 3]) -> bool {
        if t().air[s as usize] { return false; }
        let climbable = Self::state_climbable(s);
        if self.on_ground || climbable || (self.pose == POSE_CROUCHING && movement[1] == 0.0) {
            if vibrate { rules::vibrations::emit(w, rules::vibrations::Ev::Step, self.pos, rules::vibrations::Ctx::of(self.shift_key_down, s)); }
            return true;
        }
        false
    }

    /// PistonMovingBlockEntity.moveCollidedEntities / moveStuckEntities for the player: a block in transit shoves the
    /// body along (called before the block's progress goes from `m.progress` to `new_progress`).
    pub fn piston_push(&mut self, w: &mut World, m: &crate::rules::redstone::MovingBlock, new_progress: f32) {
        use crate::props::*;
        let tb = t();
        let movement = if m.extending { m.dir } else { opposite(m.dir) };
        let dp = (new_progress - m.progress) as f64;
        let st = STEP[m.dir as usize];
        let cur = (if m.extending { m.progress - 1.0f32 } else { 1.0f32 - m.progress }) as f64;
        let org = [m.pos.x as f64 + cur * st.0 as f64, m.pos.y as f64 + cur * st.1 as f64, m.pos.z as f64 + cur * st.2 as f64];
        let area = |b: &Aabb| -> Aabb {
            // PistonMath.getMovementArea
            let d = dp * if is_positive(movement) { 1.0 } else { -1.0 };
            let (mn, mx) = (d.min(0.0), d.max(0.0));
            let a = axis(movement) as usize;
            let mut r = *b;
            if is_positive(movement) { r.min[a] = b.max[a] + mn; r.max[a] = b.max[a] + mx; } else { r.min[a] = b.min[a] + mn; r.max[a] = b.min[a] + mx; }
            r
        };
        let get_movement = |outside: &Aabb, e: &Aabb| -> f64 {
            let a = axis(movement) as usize;
            if is_positive(movement) { outside.max[a] - e.min[a] } else { e.max[a] - outside.min[a] }
        };
        let retracting_base = !m.extending && m.source && kind_of(m.moved) == Kind::PistonBaseBlock;
        let crs = if retracting_base {
            let mut h = with(tb.default_of("piston_head"), P_FACING, val(m.moved, P_FACING));
            h = with_bool(h, P_SHORT, m.progress > 0.25f32);
            with(h, P_TYPE, if crate::is_blk!(m.moved, "sticky_piston") { V_STICKY } else { V_NORMAL })
        } else { m.moved };
        let shape = &tb.shapes[tb.col_shape[crs as usize] as usize];
        // (a slime block bounces other entities; server players are left alone)
        if !shape.empty && !crate::is_blk!(m.moved, "slime_block") {
            let bounds = Aabb::new(shape.min[0] + org[0], shape.min[1] + org[1], shape.min[2] + org[2], shape.max[0] + org[0], shape.max[1] + org[1], shape.max[2] + org[2]);
            let ar = area(&bounds);
            let whole = Aabb::new(ar.min[0].min(bounds.min[0]), ar.min[1].min(bounds.min[1]), ar.min[2].min(bounds.min[2]), ar.max[0].max(bounds.max[0]), ar.max[1].max(bounds.max[1]), ar.max[2].max(bounds.max[2]));
            if whole.intersects(&self.bb) {
                let mut delta = 0.0f64;
                for b in &shape.boxes {
                    let moved = Aabb::new(b[0] + org[0], b[1] + org[1], b[2] + org[2], b[3] + org[0], b[4] + org[1], b[5] + org[2]);
                    let ma = area(&moved);
                    if ma.intersects(&self.bb) {
                        delta = delta.max(get_movement(&ma, &self.bb));
                        if delta >= dp { break; }
                    }
                }
                if !(delta <= 0.0) {
                    delta = delta.min(dp) + 0.01;
                    let s2 = STEP[movement as usize];
                    self.do_move_as(w, [delta * s2.0 as f64, delta * s2.1 as f64, delta * s2.2 as f64], Some(movement));
                    if !m.extending && m.source {
                        // fixEntityWithinPistonBase
                        let bx = Aabb::new(m.pos.x as f64, m.pos.y as f64, m.pos.z as f64, m.pos.x as f64 + 1.0, m.pos.y as f64 + 1.0, m.pos.z as f64 + 1.0);
                        if self.bb.intersects(&bx) {
                            let opp = opposite(movement);
                            let gm = |outside: &Aabb, e: &Aabb| -> f64 { let a = axis(opp) as usize; if is_positive(opp) { outside.max[a] - e.min[a] } else { e.max[a] - outside.min[a] } };
                            let d1 = gm(&bx, &self.bb) + 0.01;
                            let inter = Aabb::new(self.bb.min[0].max(bx.min[0]), self.bb.min[1].max(bx.min[1]), self.bb.min[2].max(bx.min[2]), self.bb.max[0].min(bx.max[0]), self.bb.max[1].min(bx.max[1]), self.bb.max[2].min(bx.max[2]));
                            let d2 = gm(&bx, &inter) + 0.01;
                            if (d1 - d2).abs() < 0.01 {
                                let d = d1.min(dp) + 0.01;
                                let s3 = STEP[opp as usize];
                                self.do_move_as(w, [d * s3.0 as f64, d * s3.1 as f64, d * s3.2 as f64], Some(movement));
                            }
                        }
                    }
                }
            }
        }
        // moveStuckEntities: a honey block carries along what stands on it
        if crate::is_blk!(m.moved, "honey_block") && is_horizontal(movement) && self.on_ground {
            let msh = &tb.shapes[tb.col_shape[m.moved as usize] as usize];
            let ab = Aabb::new(org[0], msh.max[1] + org[1], org[2], 1.0 + org[0], 1.5000010000000001 + org[1], 1.0 + org[2]);
            let on = self.main_supporting == Some(m.pos) || (self.pos[0] >= ab.min[0] && self.pos[0] <= ab.max[0] && self.pos[2] >= ab.min[2] && self.pos[2] <= ab.max[2]);
            if ab.intersects(&self.bb) && on {
                let s2 = STEP[movement as usize];
                self.do_move_as(w, [dp * s2.0 as f64, dp * s2.1 as f64, dp * s2.2 as f64], Some(movement));
            }
        }
    }

    fn check_supporting_block(&mut self, w: &World, movement: Option<[f64; 3]>) {
        if self.on_ground {
            let b = &self.bb;
            let test = Aabb::new(b.min[0], b.min[1] - 1.0e-6, b.min[2], b.max[0], b.min[1], b.max[2]);
            let ctx = self.ctx();
            let mut sup = find_supporting_block(w, &ctx, &test, self.pos);
            if sup.is_some() || self.on_ground_no_blocks {
                self.main_supporting = sup;
            } else if let Some(m) = movement {
                let t2 = test.moved(-m[0], 0.0, -m[2]);
                sup = find_supporting_block(w, &ctx, &t2, self.pos);
                self.main_supporting = sup;
            }
            self.on_ground_no_blocks = sup.is_none();
        } else {
            self.on_ground_no_blocks = false;
            self.main_supporting = None;
        }
    }

    /// Entity.applyEffectsFromBlocks: what the blocks the body touches do to it and it to them.
    fn apply_effects_from_blocks(&mut self, w: &mut World) {
        if self.on_ground {
            let ep = self.on_pos(w, 0.2f32);
            let es = w.get(ep);
            if es != 0 {
                if crate::is_blk!(es, "slime_block") {
                    let ady = self.vel[1].abs();
                    if ady < 0.1 && !self.shift_key_down {
                        let sc = 0.4 + ady * 0.2;
                        self.vel[0] *= sc; self.vel[2] *= sc;
                    }
                }
                if rules::vibrations::is_sensor(es) { rules::vibrations::step_on(w, ep, es, self.pos); }
                // MagmaBlock.stepOn
                if crate::is_blk!(es, "magma_block") && !self.shift_key_down { self.hurt(w, 1.0, 0.1); }
                rules::step_on(w, es, ep);
            }
        }
        // checkInsideBlocks over this tick's movements (axis by axis, as the game sweeps them)
        let mut visited: Vec<P> = Vec::new();
        let mut cells: Vec<P> = Vec::new();
        let moves: Vec<([f64; 3], [f64; 3], Option<[f64; 3]>)> = if self.movements.is_empty() {
            vec![(self.tick_start_pos, self.pos, None)]
        } else {
            self.movements.iter().map(|m| (m.0, m.1, Some(m.2))).collect()
        };
        for (from, to, orig) in moves {
            let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
            let mut segs: Vec<([f64; 3], [f64; 3])> = Vec::new();
            match orig {
                Some(o) if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > 0.0 => {
                    let order: [usize; 3] = if o[0].abs() < o[2].abs() { [1, 2, 0] } else { [1, 0, 2] };
                    let mut pos = from;
                    for a in order {
                        if d[a] != 0.0 {
                            let mut t2 = pos;
                            t2[a] += d[a];
                            segs.push((pos, t2));
                            pos = t2;
                        }
                    }
                }
                _ => segs.push((from, to)),
            }
            for (f, t2) in segs {
                let bb = make_bb(t2, self.pose).deflate(1.0e-5f32 as f64);
                cells.clear();
                blocks_intersected_between(f, t2, &bb, &mut cells);
                for &q in &cells {
                    let s = w.get(q);
                    if s == 0 { continue; }
                    if visited.contains(&q) { continue; }
                    visited.push(q);
                    self.entity_inside(w, s, q);
                }
            }
        }
    }

    /// BlockState.entityInside for the player
    fn entity_inside(&mut self, w: &mut World, s: S, q: P) {
        match kind_of(s) {
            Kind::WebBlock => { self.stuck_speed_multiplier = [0.25, 0.05f32 as f64, 0.25]; }
            Kind::CampfireBlock => {
                // CampfireBlock.entityInside: fireDamage 1, soul campfire 2
                if crate::tables::is_true(s, crate::props::P_LIT) { self.hurt(w, if crate::is_blk!(s, "soul_campfire") { 2.0 } else { 1.0 }, 0.1); }
            }
            Kind::CactusBlock => { self.hurt(w, 1.0, 0.1); }
            Kind::SweetBerryBushBlock => { self.stuck_speed_multiplier = [0.8f32 as f64, 0.75, 0.8f32 as f64]; }
            Kind::PowderSnowBlock => {
                let ins = self.in_block_state(w);
                if kind_of(ins) == Kind::PowderSnowBlock { self.stuck_speed_multiplier = [0.9f32 as f64, 1.5, 0.9f32 as f64]; }
                self.is_in_powder_snow = true;
            }
            Kind::HoneyBlock => {
                // HoneyBlock.entityInside: sliding down its side
                let old_dy = self.vel[1] / 0.98f32 as f64 + 0.08;
                let sliding = !self.on_ground && !(self.pos[1] > q.y as f64 + 0.9375 - 1.0e-7) && !(old_dy >= -0.08) && {
                    let dx = (q.x as f64 + 0.5 - self.pos[0]).abs();
                    let dz = (q.z as f64 + 0.5 - self.pos[2]).abs();
                    let od = 0.4375 + (WIDTH / 2.0f32) as f64;
                    dx + 1.0e-7 > od || dz + 1.0e-7 > od
                };
                if sliding {
                    let ny = (-0.05 - 0.08) * 0.98f32 as f64;
                    if old_dy < -0.13 {
                        let k = -0.05 / old_dy;
                        self.vel = [self.vel[0] * k, ny, self.vel[2] * k];
                    } else {
                        self.vel[1] = ny;
                    }
                }
            }
            _ => {}
        }
        rules::entity_inside(w, s, q);
    }

    fn update_pose(&mut self, w: &World) {
        if self.can_fit(w, POSE_SWIMMING) {
            let desired = if self.shift_key_down { POSE_CROUCHING } else { POSE_STANDING };
            let actual = if self.can_fit(w, desired) { desired } else if self.can_fit(w, POSE_CROUCHING) { POSE_CROUCHING } else { POSE_SWIMMING };
            if actual != self.pose {
                self.pose = actual;
                self.bb = make_bb(self.pos, self.pose);
            }
        }
    }

    /// Administrator teleport (site setup).
    pub fn teleport(&mut self, x: f64, y: f64, z: f64) {
        self.set_pos(x, y, z);
        self.vel = [0.0; 3];
    }

    // ------------------------------------------------------------------ inventory
    pub fn count(&self, item: u16) -> i32 {
        self.inv.iter().filter(|s| s.count > 0 && s.item == item).map(|s| s.count as i32).sum()
    }
    pub fn free_slots(&self) -> i32 { self.inv.iter().filter(|s| s.count == 0).count() as i32 }
    pub fn consume_held(&mut self) {
        let s = &mut self.inv[self.selected];
        if s.count > 0 { s.count -= 1; if s.count == 0 { s.item = 0; } }
    }
}
