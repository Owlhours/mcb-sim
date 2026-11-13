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
    pub selected: usize,
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
            sprint_trigger_time: 0, inv: [Stack::default(); 36], selected: 0,
        }
    }

    pub fn eye_height(&self) -> f32 { EYES[self.pose as usize] }
    pub fn eye(&self) -> [f64; 3] { [self.pos[0], self.pos[1] + self.eye_height() as f64, self.pos[2]] }
    pub fn held(&self) -> Stack { self.inv[self.selected] }
    pub fn reach(&self) -> f64 { 4.5 }

    pub fn ctx(&self) -> Ctx {
        Ctx { descending: self.shift_key_down, bottom: self.pos[1], entity: true, falling_block: false, holding_scaffolding: self.held().count > 0 && t().items[self.held().item as usize].name == "scaffolding" }
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
        self.in_block_state = None;
        self.was_in_powder_snow = self.is_in_powder_snow;
        self.is_in_powder_snow = false;
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
            self.shift_key_down = self.key_presses.shift;
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
        // ---- LivingEntity.aiStep
        if self.no_jump_delay > 0 { self.no_jump_delay -= 1; }
        if self.vel[0] * self.vel[0] + self.vel[2] * self.vel[2] < 9.0e-6 { self.vel[0] = 0.0; self.vel[2] = 0.0; }
        if self.vel[1].abs() < 0.003 { self.vel[1] = 0.0; }
        // applyInput (LocalPlayer.applyInput + modifyInput)
        let mv = self.modify_input(self.move_vector, moving_slowly);
        self.xxa = mv[0];
        self.zza = mv[1];
        self.jumping = self.key_presses.jump;
        if self.jumping {
            if self.on_ground && self.no_jump_delay == 0 {
                self.jump_from_ground(w);
                self.no_jump_delay = 10;
            }
        } else {
            self.no_jump_delay = 0;
        }
        self.travel_in_air(w);
        self.apply_effects_from_blocks(w);
        // ---- Player.tick tail: updatePlayerPose
        self.update_pose(w);
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
    fn do_move(&mut self, w: &mut World, mut delta: [f64; 3]) {
        let ssm = self.stuck_speed_multiplier;
        if ssm[0] * ssm[0] + ssm[1] * ssm[1] + ssm[2] * ssm[2] > 1.0e-7 {
            delta = [delta[0] * ssm[0], delta[1] * ssm[1], delta[2] * ssm[2]];
            self.stuck_speed_multiplier = [0.0; 3];
            self.vel = [0.0; 3];
        }
        delta = self.back_off_from_edge(w, delta);
        let ctx = self.ctx();
        let movement = collide(w, &ctx, &self.bb, delta, self.on_ground, 0.6f32);
        let mlen = movement[0] * movement[0] + movement[1] * movement[1] + movement[2] * movement[2];
        let dlen = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
        if mlen > 1.0e-7 || dlen - mlen < 1.0e-7 {
            self.set_pos(self.pos[0] + movement[0], self.pos[1] + movement[1], self.pos[2] + movement[2]);
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
            }
            self.vel = after;
        }
        let f = self.block_speed_factor(w) as f64;
        self.vel[0] *= f;
        self.vel[2] *= f;
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
                rules::step_on(w, es, ep);
            }
        }
        let bx = self.bb.deflate(1.0e-5f32 as f64);
        let (x0, y0, z0) = (floor(bx.min[0]), floor(bx.min[1]), floor(bx.min[2]));
        let (x1, y1, z1) = (floor(bx.max[0]), floor(bx.max[1]), floor(bx.max[2]));
        for x in x0..=x1 { for y in y0..=y1 { for z in z0..=z1 {
            let q = p(x, y, z);
            let s = w.get(q);
            if s == 0 { continue; }
            match kind_of(s) {
                Kind::WebBlock => { self.stuck_speed_multiplier = [0.25, 0.05f32 as f64, 0.25]; }
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
        } } }
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
