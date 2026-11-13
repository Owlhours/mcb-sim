//! Sculk sensor vibrations: ports of the vanilla 26.2 game-event machinery as far as sculk sensors (plain and
//! calibrated) listen to it - GameEventDispatcher.post / EuclideanGameEventListenerRegistry (who hears an event),
//! VibrationSystem.Listener.handleGameEvent / User (SculkSensorBlockEntity.VibrationUser,
//! CalibratedSculkSensorBlockEntity.VibrationUser), VibrationSelector, VibrationSystem.Ticker (the block entity's
//! ticker) and SculkSensorBlock.activate / tryResonateVibration / stepOn / getAnalogOutputSignal.
//!
//! The listeners are the sensors' block entities: they exist from the block's placement to its removal (a state
//! change of the same block keeps them, `World::set_block` calls `created` / `removed`). Their tickers sit in the
//! redstone side table's ticker list (Level.blockEntityTickers order). Game time here is the Java run's game time,
//! `World::level_time`, which only advances in complete server ticks (the Java `Env` skips idle ones).
//!
//! The emitting sites call `emit`; it returns at once while the world has no sensor, so a build without sensors
//! pays one integer test per game event.
#![allow(dead_code)]
use crate::geom::*;
use crate::kinds::Kind as K;
use crate::props::*;
use crate::tables::*;
use crate::world::World;
use std::cell::RefCell;

/// The game events that can reach a sculk sensor from this world (all are in the `vibrations` tag).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ev {
    Step,
    HitGround,
    Bounce,
    BlockPlace,
    BlockDestroy,
    BlockChange,
    BlockOpen,
    BlockClose,
    BlockActivate,
    BlockDeactivate,
    BlockAttach,
    BlockDetach,
    ContainerOpen,
    ContainerClose,
    NoteBlockPlay,
    PrimeFuse,
    Shear,
    Eat,
    Equip,
    Unequip,
    /// RESONATE_1 .. RESONATE_15
    Resonate(u8),
}

impl Ev {
    /// VibrationSystem.VIBRATION_FREQUENCY_FOR_EVENT
    pub fn frequency(self) -> i32 {
        match self {
            Ev::Step => 1,
            Ev::HitGround | Ev::Bounce => 2,
            Ev::Unequip => 4,
            Ev::Equip => 5,
            Ev::Shear => 6,
            Ev::Eat => 8,
            Ev::ContainerClose | Ev::BlockClose | Ev::BlockDeactivate | Ev::BlockDetach => 9,
            Ev::ContainerOpen | Ev::BlockOpen | Ev::BlockActivate | Ev::BlockAttach | Ev::PrimeFuse | Ev::NoteBlockPlay => 10,
            Ev::BlockChange => 11,
            Ev::BlockDestroy => 12,
            Ev::BlockPlace => 13,
            Ev::Resonate(f) => f as i32,
        }
    }
    /// GameEventTags.IGNORE_VIBRATIONS_SNEAKING (of these: hit_ground, step)
    fn ignored_sneaking(self) -> bool { matches!(self, Ev::Step | Ev::HitGround) }
}

/// GameEvent.Context: the source entity (here only ever the player: Some(sneaking), i.e. isSteppingCarefully)
/// and the affected block state.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub player: Option<bool>,
    pub affected: Option<S>,
}
impl Ctx {
    pub const NONE: Ctx = Ctx { player: None, affected: None };
    pub fn player(sneaking: bool) -> Ctx { Ctx { player: Some(sneaking), affected: None } }
    pub fn state(s: S) -> Ctx { Ctx { player: None, affected: Some(s) } }
    pub fn of(sneaking: bool, s: S) -> Ctx { Ctx { player: Some(sneaking), affected: Some(s) } }
}

/// VibrationInfo (the source entity is not kept: nothing downstream asks more of it than the sensor already did)
#[derive(Clone, Copy, Debug)]
struct Info {
    ev: Ev,
    distance: f32,
    pos: [f64; 3],
}

/// SculkSensorBlockEntity with its VibrationSystem.Data
#[derive(Clone, Debug)]
struct Sensor {
    pos: P,
    calibrated: bool,
    block: u16,
    current: Option<Info>,
    travel: i32,
    /// VibrationSelector.currentVibrationData: (candidate, tick)
    selector: Option<(Info, i64)>,
    last_frequency: i32,
}

impl Sensor {
    /// VibrationUser.getListenerRadius
    fn radius(&self) -> i32 { if self.calibrated { 16 } else { 8 } }
}

struct State {
    /// the registered listeners (in registration order; the order of delivery does not matter here, since a
    /// sensor's handleGameEvent only adds a candidate to its own selector)
    sensors: Vec<Sensor>,
    /// block entities made since the last reset
    made: u64,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State { sensors: Vec::new(), made: 0 });
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R { ST.with(|s| f(&mut s.borrow_mut())) }

/// Clears everything (between builds).
pub fn reset() { st(|s| { s.sensors.clear(); s.made = 0; }); }
/// Number of sensor block entities made since the last reset.
pub fn made() -> u64 { st(|s| s.made) }

#[inline]
pub fn is_sensor(s: S) -> bool { kind_of(s) == K::SculkSensorBlock }

fn center(q: P) -> [f64; 3] { [q.x as f64 + 0.5, q.y as f64 + 0.5, q.z as f64 + 0.5] }
fn containing(v: [f64; 3]) -> P { p(floor(v[0]), floor(v[1]), floor(v[2])) }
fn dist_sqr(a: P, b: P) -> f64 {
    let (dx, dy, dz) = ((a.x - b.x) as f64, (a.y - b.y) as f64, (a.z - b.z) as f64);
    dx * dx + dy * dy + dz * dz
}
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

// ------------------------------------------------------------------------------------------------ block entity life
/// LevelChunk.addAndRegisterBlockEntity for a sculk sensor: the block entity, its game event listener and its ticker.
pub fn created(w: &mut World, pos: P, s: S) {
    let have = st(|x| x.sensors.iter().any(|e| e.pos == pos));
    if have { return; }
    st(|x| x.made += 1);
    st(|x| x.sensors.push(Sensor { pos, calibrated: has(s, P_FACING), block: block_of(s), current: None, travel: 0, selector: None, last_frequency: 0 }));
    w.vib_listeners += 1;
    super::redstone::add_sculk_ticker(pos);
}

/// LevelChunk.removeBlockEntity for a sculk sensor.
pub fn removed(w: &mut World, pos: P) {
    let n = st(|x| {
        let before = x.sensors.len();
        x.sensors.retain(|e| e.pos != pos);
        before - x.sensors.len()
    });
    if n > 0 {
        w.vib_listeners -= n as u32;
        super::redstone::remove_sculk_ticker(pos);
    }
}

/// SculkSensorBlockEntity.getLastVibrationFrequency (0 without a block entity)
pub fn last_frequency(pos: P) -> i32 { st(|x| x.sensors.iter().find(|e| e.pos == pos).map(|e| e.last_frequency).unwrap_or(0)) }

// ------------------------------------------------------------------------------------------------ reception
/// VibrationUser.canReceiveVibration (with the calibrated sensor's frequency filter)
fn can_receive(w: &World, e: &Sensor, pos: P, ev: Ev) -> bool {
    let state = w.get(e.pos);
    if e.calibrated && has(state, P_FACING) {
        // getBackSignal
        let d = opposite(dir_of(val(state, P_FACING)));
        let comparison = super::redstone::get_signal(w, e.pos.rel(d), d);
        if comparison != 0 && ev.frequency() != comparison { return false; }
    }
    if pos == e.pos && matches!(ev, Ev::BlockDestroy | Ev::BlockPlace) { return false; }
    if ev.frequency() == 0 { return false; }
    can_activate(state)
}

/// SculkSensorBlock.canActivate
fn can_activate(s: S) -> bool { is_sensor(s) && get(s, P_SCULK_SENSOR_PHASE) == Some(V_INACTIVE) }

/// VibrationSelector.addCandidate
fn add_candidate(e: &mut Sensor, info: Info, tick: i64) {
    let replace = match &e.selector {
        None => true,
        Some((prev, ptick)) => {
            if tick != *ptick {
                false
            } else if info.distance < prev.distance {
                true
            } else if info.distance > prev.distance {
                false
            } else {
                info.ev.frequency() > prev.ev.frequency()
            }
        }
    };
    if replace { e.selector = Some((info, tick)); }
}

/// Mth.frac
fn frac(v: f64) -> f64 { v - v.floor() }
/// Mth.lerp
fn lerp(t: f64, a: f64, b: f64) -> f64 { a + t * (b - a) }
fn sign(v: f64) -> i32 { if v == 0.0 { 0 } else if v > 0.0 { 1 } else { -1 } }

/// BlockGetter.isBlockInLine with the `occludes_vibration_signals` test: true when it hits such a block.
fn line_hits_occluder(w: &World, from: [f64; 3], to: [f64; 3]) -> bool {
    if from == to { return false; }
    let occ = |q: P| crate::in_tag!(w.get(q), "occludes_vibration_signals");
    let (tx, ty, tz) = (lerp(-1.0e-7, to[0], from[0]), lerp(-1.0e-7, to[1], from[1]), lerp(-1.0e-7, to[2], from[2]));
    let (fx, fy, fz) = (lerp(-1.0e-7, from[0], to[0]), lerp(-1.0e-7, from[1], to[1]), lerp(-1.0e-7, from[2], to[2]));
    let (mut cx, mut cy, mut cz) = (floor(fx), floor(fy), floor(fz));
    if occ(p(cx, cy, cz)) { return true; }
    let (dx, dy, dz) = (tx - fx, ty - fy, tz - fz);
    let (sx, sy, sz) = (sign(dx), sign(dy), sign(dz));
    let tdx = if sx == 0 { f64::MAX } else { sx as f64 / dx };
    let tdy = if sy == 0 { f64::MAX } else { sy as f64 / dy };
    let tdz = if sz == 0 { f64::MAX } else { sz as f64 / dz };
    let mut tx_ = tdx * if sx > 0 { 1.0 - frac(fx) } else { frac(fx) };
    let mut ty_ = tdy * if sy > 0 { 1.0 - frac(fy) } else { frac(fy) };
    let mut tz_ = tdz * if sz > 0 { 1.0 - frac(fz) } else { frac(fz) };
    while tx_ <= 1.0 || ty_ <= 1.0 || tz_ <= 1.0 {
        if tx_ < ty_ {
            if tx_ < tz_ { cx += sx; tx_ += tdx; } else { cz += sz; tz_ += tdz; }
        } else if ty_ < tz_ {
            cy += sy; ty_ += tdy;
        } else {
            cz += sz; tz_ += tdz;
        }
        if occ(p(cx, cy, cz)) { return true; }
    }
    false
}

/// VibrationSystem.Listener.isOccluded
fn is_occluded(w: &World, origin: [f64; 3], dest: [f64; 3]) -> bool {
    let from = [floor(origin[0]) as f64 + 0.5, floor(origin[1]) as f64 + 0.5, floor(origin[2]) as f64 + 0.5];
    let to = [floor(dest[0]) as f64 + 0.5, floor(dest[1]) as f64 + 0.5, floor(dest[2]) as f64 + 0.5];
    let nudge = 1.0e-5f32 as f64;
    for d in DIRS {
        let s = STEP[d as usize];
        let src = [from[0] + s.0 as f64 * nudge, from[1] + s.1 as f64 * nudge, from[2] + s.2 as f64 * nudge];
        if !line_hits_occluder(w, src, to) { return false; }
    }
    true
}

/// ServerLevel.gameEvent -> GameEventDispatcher.post, for the sensors' listeners (VibrationSystem.Listener.handleGameEvent).
pub fn emit(w: &World, ev: Ev, pos: [f64; 3], ctx: Ctx) {
    if w.vib_listeners == 0 { return; }
    // VibrationSystem.User.isValidVibration (the parts that do not depend on the listener)
    if let Some(sneaking) = ctx.player {
        if sneaking && ev.ignored_sneaking() { return; }
    }
    if let Some(a) = ctx.affected {
        if crate::in_tag!(a, "dampens_vibrations") { return; }
    }
    let src = containing(pos);
    let now = w.level_time;
    ST.with(|cell| {
        let mut x = cell.borrow_mut();
        for e in x.sensors.iter_mut() {
            // EuclideanGameEventListenerRegistry.getPostableListenerPosition
            let r = e.radius();
            if dist_sqr(e.pos, src) > (r * r) as f64 { continue; }
            if e.current.is_some() { continue; }
            let dest = center(e.pos);
            if !can_receive(w, e, src, ev) { continue; }
            if is_occluded(w, pos, dest) { continue; }
            add_candidate(e, Info { ev, distance: distance(pos, dest) as f32, pos }, now);
        }
    });
}
/// Level.gameEvent at a block position (Vec3.atCenterOf)
pub fn emit_at(w: &World, ev: Ev, q: P, ctx: Ctx) { if w.vib_listeners != 0 { emit(w, ev, center(q), ctx); } }

/// SculkSensorBlock.stepOn for the player standing at `player_pos`: Listener.forceScheduleVibration.
pub fn step_on(w: &World, pos: P, s: S, player_pos: [f64; 3]) {
    if !can_activate(s) { return; }
    let now = w.level_time;
    ST.with(|cell| {
        let mut x = cell.borrow_mut();
        if let Some(e) = x.sensors.iter_mut().find(|e| e.pos == pos) {
            if can_receive(w, e, pos, Ev::Step) {
                let d = distance(player_pos, center(pos)) as f32;
                add_candidate(e, Info { ev: Ev::Step, distance: d, pos: player_pos }, now);
            }
        }
    });
}

// ------------------------------------------------------------------------------------------------ the ticker
fn get_sensor(pos: P) -> Option<Sensor> { st(|x| x.sensors.iter().find(|e| e.pos == pos).cloned()) }
fn put_sensor(e: &Sensor) { st(|x| if let Some(o) = x.sensors.iter_mut().find(|o| o.pos == e.pos) { *o = e.clone(); }); }

/// VibrationUser.onDataChanged -> BlockEntity.setChanged
fn data_changed(w: &mut World, e: &Sensor) { super::redstone::update_neighbour_for_output_signal(w, e.pos, e.block); }

/// VibrationSystem.Ticker.tick for the sensor at `pos`.
pub fn tick(w: &mut World, pos: P) {
    let Some(mut e) = get_sensor(pos) else { return };
    if e.current.is_none() {
        // trySelectAndScheduleVibration
        if let Some((info, tk)) = e.selector {
            if tk < w.level_time {
                e.current = Some(info);
                e.travel = floor(info.distance as f64);
                e.selector = None;
                put_sensor(&e);
                data_changed(w, &e);
                match get_sensor(pos) { Some(x) => e = x, None => return }
            }
        }
    }
    let Some(cur) = e.current else { return };
    let mut changed = e.travel > 0;
    e.travel = (e.travel - 1).max(0);
    put_sensor(&e);
    if e.travel <= 0 {
        changed = receive(w, &e, cur);
    }
    if changed { data_changed(w, &e); }
}

/// VibrationSystem.Ticker.receiveVibration
fn receive(w: &mut World, e: &Sensor, cur: Info) -> bool {
    let origin = containing(cur.pos);
    let dest = e.pos;
    // VibrationUser.onReceiveVibration
    let state = w.get(e.pos);
    if can_activate(state) {
        let freq = cur.ev.frequency();
        st(|x| if let Some(o) = x.sensors.iter_mut().find(|o| o.pos == e.pos) { o.last_frequency = freq; });
        // Listener.distanceBetweenInBlocks
        let receiving = dist_sqr(origin, dest).sqrt() as f32;
        // VibrationSystem.getRedstoneStrengthForDistance
        let scale = 15.0 / e.radius() as f64;
        let power = 1.max(15 - floor(scale * receiving as f64));
        activate(w, e, state, power, freq);
    }
    st(|x| if let Some(o) = x.sensors.iter_mut().find(|o| o.pos == e.pos) { o.current = None; });
    true
}

/// SculkSensorBlock.activate
fn activate(w: &mut World, e: &Sensor, state: S, power: i32, freq: i32) {
    let pos = e.pos;
    w.set_block(pos, with(with(state, P_SCULK_SENSOR_PHASE, V_ACTIVE), P_POWER, power as u8), 3);
    // getActiveTicks: 30, calibrated 10
    super::redstone::schedule_tick(w, pos, block_of(state), if e.calibrated { 10 } else { 30 });
    w.update_neighbors_at(pos, block_of(state));
    w.update_neighbors_at(pos.below(), block_of(state));
    // tryResonateVibration
    for d in DIRS {
        let rp = pos.rel(d);
        let bs = w.get(rp);
        if crate::in_tag!(bs, "vibration_resonators") {
            emit_at(w, Ev::Resonate(freq as u8), rp, Ctx { player: None, affected: Some(bs) });
        }
    }
    // SCULK_SENSOR_TENDRILS_CLICKING is not a vibration: no sensor hears it. A sculk shrieker within its listener
    // radius (8) would, when the vibration came from the player: shrieking is not modelled.
    'scan: for dy in -8..=8 {
        for dz in -8..=8 {
            for dx in -8..=8 {
                if dx * dx + dy * dy + dz * dz > 64 { continue; }
                if kind_of(w.get(pos.off(dx, dy, dz))) == K::SculkShriekerBlock { w.unmodelled += 1; break 'scan; }
            }
        }
    }
    // (sounds)
}
