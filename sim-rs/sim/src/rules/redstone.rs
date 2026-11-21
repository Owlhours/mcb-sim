//! Redstone: ports of the vanilla 26.2 redstone block classes (RedStoneWireBlock with the default wire evaluator,
//! torches, repeaters/comparators, lamps, pistons with their moving-block entities, observers, dispensers, hoppers,
//! note blocks, levers/buttons/plates, tripwire, rails, bulbs, bells ...) and of the `SignalGetter` queries.
//!
//! State that vanilla keeps outside the block states lives in a thread-local side table (`Side`): the comparator
//! outputs, the piston moving-block entities and the other ticking block entities in creation order, the redstone
//! torch toggle history and RedStoneWireBlock's `shouldSignal` flag. `reset()` clears it between builds.
//!
//! What the simulator's world cannot do is said where it happens: nothing drops, there are no minecarts, arrows or
//! lightning, containers are empty, explosions only bump `w.unmodelled`. Sculk sensor vibrations: vibrations.rs.
#![allow(dead_code, unused_variables)]
use super::{Clicker, PlaceCtx, Use};
use super::vibrations as vib;
use crate::geom::*;
use crate::kinds::Kind as K;
use crate::props::*;
use crate::tables::*;
use crate::world::World;
use crate::{blk, in_tag, is_blk};
use std::cell::RefCell;
use crate::fasthash::FastMap as HashMap;
use std::sync::OnceLock;

// TickPriority values
const PRIO_EXTREMELY_HIGH: i8 = -3;
const PRIO_VERY_HIGH: i8 = -2;
const PRIO_HIGH: i8 = -1;
const PRIO_NORMAL: i8 = 0;
/// Level.getMinY of the overworld
const MIN_Y: i32 = -64;
/// the default update limit of the three-argument setBlock / updateNeighbourShapes
const LIMIT: i32 = 512;

// ------------------------------------------------------------------------------------------------ side tables
/// PistonMovingBlockEntity
#[derive(Clone, Debug)]
pub struct MovingBlock {
    pub pos: P,
    /// the carried state (for the piston's own arm: the piston head, or the piston base while it retracts)
    pub moved: S,
    pub dir: Dir,
    pub extending: bool,
    pub source: bool,
    pub progress: f32,
    pub progress_o: f32,
    pub last_ticked: i64,
}

#[derive(Clone, Debug)]
enum Ticker {
    Moving(MovingBlock),
    /// DaylightDetectorBlockEntity
    Daylight(P),
    /// SculkSensorBlockEntity / CalibratedSculkSensorBlockEntity (VibrationSystem.Ticker; the data is in vibrations.rs)
    Sculk(P),
}

struct Entry {
    id: u64,
    removed: bool,
    t: Ticker,
}

struct Side {
    /// RedStoneWireBlock.shouldSignal
    wire_signal: bool,
    /// RedstoneTorchBlock.RECENT_TOGGLES
    toggles: Vec<(P, i64)>,
    /// ComparatorBlockEntity.output
    comparator: HashMap<P, u8>,
    /// Level.blockEntityTickers (the ones that matter here), in creation order
    tickers: Vec<Entry>,
    next_id: u64,
    /// trigger times of the scheduled ticks asked for by this file, for LevelTicks.willTickThisTick
    due: HashMap<(P, u16), Vec<i64>>,
    /// depth of scheduled-tick / block-event processing (ServerLevel.isHandlingTick)
    handling: u32,
    /// Level.tickingBlockEntities
    ticking: bool,
    /// Level.pendingBlockEntityTickers is not empty: tickers were made during the last block entity pass
    pending: bool,
}

impl Side {
    fn new() -> Side {
        Side { wire_signal: true, toggles: Vec::new(), comparator: HashMap::default(), tickers: Vec::new(), next_id: 1, due: HashMap::default(), handling: 0, ticking: false, pending: false }
    }
}

thread_local! {
    static SIDE: RefCell<Side> = RefCell::new(Side::new());
}

/// Short access to the side table. Never call into the world (or anything else of this file) from inside `f`.
fn side<R>(f: impl FnOnce(&mut Side) -> R) -> R { SIDE.with(|s| f(&mut s.borrow_mut())) }

/// Clears all side tables (between builds).
pub fn reset() { side(|s| *s = Side::new()); }

/// Level.addBlockEntityTicker: appended to the list, or to the pending list while the block entities are ticking
fn push_ticker(x: &mut Side, t: Ticker) {
    let id = x.next_id;
    x.next_id += 1;
    if x.ticking { x.pending = true; }
    x.tickers.push(Entry { id, removed: false, t });
}
/// A sculk sensor's block entity ticker (vibrations.rs)
pub fn add_sculk_ticker(pos: P) { side(|x| push_ticker(x, Ticker::Sculk(pos))); }
pub fn remove_sculk_ticker(pos: P) {
    side(|x| for e in x.tickers.iter_mut() { if matches!(e.t, Ticker::Sculk(q) if q == pos) { e.removed = true; } });
}
/// Level.pendingBlockEntityTickers is not empty (Env.worldIdle)
pub fn pending_tickers() -> bool { side(|x| x.pending) }
/// Level.scheduleTick(pos, block, delay) at normal priority, for the other rule files
pub fn schedule_tick(w: &mut World, pos: P, block: u16, delay: i32) { sched(w, pos, block, delay, PRIO_NORMAL); }

/// Per-block properties from Blocks.java that the generated tables lack or have incomplete.
struct BlockTabs {
    /// PushReaction (PUSH_* of tables.rs)
    push: Vec<u8>,
    /// isRedstoneConductor override: 0 none (collision shape is a full cube), 1 never, 2 always
    conductor: Vec<u8>,
    /// NoteBlockInstrument value code of the block (BlockBehaviour.Properties.instrument)
    instrument: Vec<u8>,
    block_entity: Vec<bool>,
}
static TABS: OnceLock<BlockTabs> = OnceLock::new();

fn tabs() -> &'static BlockTabs {
    TABS.get_or_init(|| {
        let tb = t();
        let n = tb.blocks.len();
        let mut push = vec![PUSH_NORMAL; n];
        let mut conductor = vec![0u8; n];
        let mut instrument = vec![V_HARP; n];
        let mut block_entity = vec![false; n];
        for (i, b) in tb.blocks.iter().enumerate() {
            let nm = b.name.as_str();
            // the table's push reactions miss the ones set through the helper functions of Blocks.java
            // (leavesProperties, buttonProperties, flowerPotProperties, candleProperties, shulkerBoxProperties,
            // pistonProperties, the bed/glazed terracotta/copper collections)
            push[i] = b.push;
            if nm.ends_with("_leaves") || nm.ends_with("_bed") || nm.ends_with("_button") || nm == "flower_pot" || nm.starts_with("potted_")
                || nm.ends_with("shulker_box") || nm.ends_with("copper_lantern") || nm == "candle" || nm.ends_with("_candle") || nm.ends_with("candle_cake")
                || nm.ends_with("copper_door") || nm.ends_with("copper_golem_statue")
            {
                push[i] = PUSH_DESTROY;
            }
            if nm.ends_with("_glazed_terracotta") { push[i] = PUSH_ONLY; }
            if nm == "piston" || nm == "sticky_piston" { push[i] = PUSH_BLOCK; }
            // .isRedstoneConductor(Blocks::never) / (Blocks::always)
            if nm.ends_with("_leaves") || nm.ends_with("_stained_glass") || nm.ends_with("copper_grate") || nm.ends_with("copper_bulb")
                || matches!(nm, "glass" | "tinted_glass" | "piston" | "sticky_piston" | "moving_piston" | "tnt" | "ice" | "frosted_ice" | "glowstone" | "beacon"
                    | "redstone_block" | "sea_lantern" | "chorus_flower" | "observer" | "bamboo" | "scaffolding" | "powder_snow" | "pointed_dripstone" | "sulfur_spike")
            {
                conductor[i] = 1;
            }
            if nm == "soul_sand" || nm == "mud" { conductor[i] = 2; }
            // EntityBlock implementors
            block_entity[i] = matches!(b.kind,
                K::BannerBlock | K::WallBannerBlock | K::ChestBlock | K::EnderChestBlock | K::FurnaceBlock | K::SkullBlock | K::WallSkullBlock | K::BarrelBlock
                | K::BrushableBlock | K::CopperGolemStatueBlock | K::DecoratedPotBlock | K::EnchantingTableBlock | K::LecternBlock | K::PotentSulfurBlock
                | K::SculkShriekerBlock | K::GameMasterBlock | K::EndPortalBlock | K::TrialSpawnerBlock | K::BeaconBlock | K::BeehiveBlock | K::BrewingStandBlock
                | K::BellBlock | K::CampfireBlock | K::ChiseledBookShelfBlock | K::CommandBlock | K::ComparatorBlock | K::CrafterBlock | K::CreakingHeartBlock
                | K::ConduitBlock | K::DaylightDetectorBlock | K::DispenserBlock | K::EndGatewayBlock | K::JigsawBlock | K::JukeboxBlock | K::HopperBlock
                | K::SculkSensorBlock | K::ShelfBlock | K::ShulkerBoxBlock | K::StandingSignBlock | K::WallSignBlock | K::CeilingHangingSignBlock
                | K::WallHangingSignBlock | K::SpawnerBlock | K::VaultBlock | K::MovingPistonBlock)
                || nm == "sculk_catalyst" || nm == "test_block";
        }
        for line in INSTRUMENTS.lines() {
            let Some((ins, names)) = line.split_once(':') else { continue };
            let Some(vi) = VALUE_NAMES.iter().position(|x| *x == ins) else { continue };
            for nm in names.split(' ') {
                if let Some(&bi) = tb.block_by_name.get(nm) { instrument[bi as usize] = 32 + vi as u8; }
            }
        }
        BlockTabs { push, conductor, instrument, block_entity }
    })
}

/// BlockState.getPistonPushReaction
pub fn push_reaction(s: S) -> u8 { tabs().push[block_of(s) as usize] }
/// BlockState.hasBlockEntity
pub fn has_block_entity(s: S) -> bool { tabs().block_entity[block_of(s) as usize] }
/// BlockState.instrument (value code of the note block's `instrument` property)
pub fn instrument_of(s: S) -> u8 { tabs().instrument[block_of(s) as usize] }
/// BlockState.isRedstoneConductor
pub fn is_redstone_conductor(s: S) -> bool {
    match tabs().conductor[block_of(s) as usize] {
        1 => false,
        2 => true,
        _ => t().full_cube[s as usize],
    }
}

// ------------------------------------------------------------------------------------------------ small helpers
#[inline]
fn facing(s: S) -> Dir { get(s, P_FACING).map(dir_of).unwrap_or(NORTH) }
#[inline]
fn powered(s: S) -> bool { is_true(s, P_POWERED) }
#[inline]
fn power(s: S) -> i32 { get(s, P_POWER).unwrap_or(0) as i32 }
#[inline]
fn is_air(s: S) -> bool { t().air[s as usize] }
/// StateHolder.cycle
fn cycle(s: S, prop: u8) -> S {
    let b = t().binfo(s);
    let Some(cur) = get(s, prop) else { return s };
    for pd in &b.props {
        if pd.id == prop {
            let i = pd.vals.iter().position(|v| *v == cur).unwrap_or(0);
            return with(s, prop, pd.vals[(i + 1) % pd.vals.len()]);
        }
    }
    s
}
fn default_state(block: u16) -> S { t().blocks[block as usize].default }

/// Level.scheduleTick, remembering the trigger time for `will_tick_this_tick`.
fn sched(w: &mut World, pos: P, block: u16, delay: i32, prio: i8) {
    if !w.inside(pos) { return; }
    if !w.has_scheduled_tick(pos, block) {
        let tt = w.game_time + delay as i64;
        side(|x| x.due.entry((pos, block)).or_default().push(tt));
    }
    w.schedule_tick_prio(pos, block, delay, prio);
}
/// LevelTicks.willTickThisTick: a tick of this block that is due in the running tick and has not been run yet.
/// (World has no such query; the trigger times of the ticks this file schedules are kept in the side table. The
/// game time is advanced right before the scheduled ticks run, so "due now and not run yet" is that set.)
fn will_tick_this_tick(w: &World, pos: P, block: u16) -> bool {
    let now = w.game_time;
    side(|x| match x.due.get_mut(&(pos, block)) {
        Some(v) => {
            v.retain(|tt| *tt >= now);
            v.iter().any(|tt| *tt == now)
        }
        None => false,
    })
}
fn due_ran(pos: P, block: u16, now: i64) {
    side(|x| {
        let mut empty = false;
        if let Some(v) = x.due.get_mut(&(pos, block)) {
            if let Some(i) = v.iter().position(|tt| *tt <= now) { v.remove(i); }
            v.retain(|tt| *tt >= now);
            empty = v.is_empty();
        }
        if empty { x.due.remove(&(pos, block)); }
    });
}
/// ServerLevel.isHandlingTick: true while scheduled ticks and block events are processed. Known here only for the
/// ticks and events of this file's blocks (see the report: World could carry the flag).
fn is_handling_tick() -> bool { side(|x| x.handling > 0) }

/// Vec3i.hashCode
fn java_hash(q: P) -> i32 { q.y.wrapping_add(q.z.wrapping_mul(31)).wrapping_mul(31).wrapping_add(q.x) }
/// Iteration order of a java.util.HashMap/HashSet of block positions that never grew past its initial 16 buckets
/// (at most 12 entries): by bucket, within a bucket in insertion order.
fn java_hash_order<T: Clone>(items: &[(P, T)]) -> Vec<(P, T)> {
    let mut idx: Vec<(usize, usize)> = items
        .iter()
        .enumerate()
        .map(|(i, (q, _))| {
            let h = java_hash(*q) as u32;
            (((h ^ (h >> 16)) & 15) as usize, i)
        })
        .collect();
    idx.sort();
    idx.into_iter().map(|(_, i)| items[i].clone()).collect()
}

/// BlockState.updateNeighbourShapes(level, pos, flags)
fn update_neighbour_shapes(w: &mut World, s: S, pos: P, flags: u32) {
    for d in UPDATE_SHAPE_ORDER {
        w.neighbor_shape_changed(opposite(d), pos.rel(d), pos, s, flags, LIMIT);
    }
}
/// Block.updateFromNeighbourShapes
fn update_from_neighbour_shapes(w: &mut World, s: S, pos: P) -> S {
    let mut st = s;
    for d in UPDATE_SHAPE_ORDER {
        let np = pos.rel(d);
        let ns = w.get(np);
        st = super::update_shape(w, st, pos, d, np, ns);
    }
    st
}

/// The block classes this file owns completely.
fn owned(k: K) -> bool {
    matches!(k,
        K::RedStoneWireBlock | K::RedstoneTorchBlock | K::RedstoneWallTorchBlock | K::RepeaterBlock | K::ComparatorBlock | K::RedstoneLampBlock | K::PoweredBlock
        | K::PistonBaseBlock | K::PistonHeadBlock | K::MovingPistonBlock | K::ObserverBlock | K::DispenserBlock | K::HopperBlock | K::NoteBlock | K::LeverBlock
        | K::ButtonBlock | K::PressurePlateBlock | K::WeightedPressurePlateBlock | K::TripWireBlock | K::TripWireHookBlock | K::TargetBlock
        | K::DaylightDetectorBlock | K::TntBlock | K::PoweredRailBlock | K::DetectorRailBlock | K::RailBlock | K::CopperBulbBlock | K::CrafterBlock
        | K::LightningRodBlock | K::SculkSensorBlock | K::BellBlock | K::JukeboxBlock | K::CommandBlock)
}

/// BlockState.canSurvive for the owned classes (the others are asked through `rules::can_survive`).
pub fn can_survive(w: &World, s: S, pos: P) -> bool {
    match kind_of(s) {
        K::RedStoneWireBlock => wire_can_survive_on(w.get(pos.below())),
        K::RepeaterBlock | K::ComparatorBlock => w.can_support_rigid(pos.below()),
        K::RedstoneTorchBlock => w.can_support_center(pos.below(), UP),
        K::RedstoneWallTorchBlock => {
            let f = facing(s);
            w.is_face_sturdy(pos.rel(opposite(f)), f)
        }
        K::LeverBlock | K::ButtonBlock => {
            let d = opposite(connected_direction(s));
            w.is_face_sturdy(pos.rel(d), opposite(d))
        }
        K::PressurePlateBlock | K::WeightedPressurePlateBlock => w.can_support_rigid(pos.below()) || w.can_support_center(pos.below(), UP),
        K::TripWireHookBlock => {
            let f = facing(s);
            is_horizontal(f) && w.is_face_sturdy(pos.rel(opposite(f)), f)
        }
        K::RailBlock | K::PoweredRailBlock | K::DetectorRailBlock => w.can_support_rigid(pos.below()),
        K::PistonHeadBlock => head_can_survive(w, s, pos),
        K::BellBlock => {
            let d = opposite(bell_connected_direction(s));
            if d == UP {
                // Block.canSupportCenter(level, above, DOWN)
                let a = w.get(pos.above());
                !in_tag!(a, "unstable_bottom_center") && w.can_support_center(pos.above(), DOWN)
            } else {
                w.is_face_sturdy(pos.rel(d), opposite(d))
            }
        }
        _ => super::can_survive(w, s, pos),
    }
}

/// FaceAttachedHorizontalDirectionalBlock.getConnectedDirection
fn connected_direction(s: S) -> Dir {
    match get(s, P_FACE) {
        Some(V_CEILING) => DOWN,
        Some(V_FLOOR) => UP,
        _ => facing(s),
    }
}

// ------------------------------------------------------------------------------------------------ signals
/// BlockState.isSignalSource
pub fn is_signal_source(s: S) -> bool {
    match kind_of(s) {
        K::RedStoneWireBlock => side(|x| x.wire_signal),
        K::PressurePlateBlock | K::WeightedPressurePlateBlock | K::JukeboxBlock | K::LeverBlock | K::LightningRodBlock | K::ButtonBlock
        | K::DaylightDetectorBlock | K::DetectorRailBlock | K::RepeaterBlock | K::ComparatorBlock | K::LecternBlock | K::RedstoneTorchBlock
        | K::RedstoneWallTorchBlock | K::ObserverBlock | K::PoweredBlock | K::SculkSensorBlock | K::TargetBlock | K::TripWireHookBlock => true,
        K::ChestBlock => is_blk!(s, "trapped_chest"),
        _ => false,
    }
}

/// BlockBehaviour.ownSignal
fn own_signal(w: &World, s: S, pos: P) -> i32 {
    match kind_of(s) {
        K::PressurePlateBlock | K::LeverBlock | K::ButtonBlock | K::LightningRodBlock | K::DetectorRailBlock | K::LecternBlock | K::ObserverBlock
        | K::TripWireHookBlock => if powered(s) { 15 } else { 0 },
        K::WeightedPressurePlateBlock | K::DaylightDetectorBlock | K::TargetBlock | K::SculkSensorBlock | K::RedStoneWireBlock => power(s),
        K::RepeaterBlock => if powered(s) { 15 } else { 0 },
        K::ComparatorBlock => if powered(s) { comparator_output(pos) } else { 0 },
        K::RedstoneTorchBlock | K::RedstoneWallTorchBlock => if is_true(s, P_LIT) { 15 } else { 0 },
        K::PoweredBlock => 15,
        // jukebox: no disc ever plays; trapped chest: never open (ChestBlockEntity.getOpenCount)
        _ => 0,
    }
}

/// BlockState.getSignal(level, pos, direction)
pub fn state_signal(w: &World, s: S, pos: P, dir: Dir) -> i32 {
    match kind_of(s) {
        K::RedStoneWireBlock => {
            if side(|x| x.wire_signal) && dir != DOWN {
                let pw = power(s);
                if pw == 0 {
                    0
                } else if dir != UP && !wire_connected(wire_connection_state(w, s, pos), opposite(dir)) {
                    0
                } else {
                    pw
                }
            } else {
                0
            }
        }
        K::RedstoneTorchBlock => if dir != UP { own_signal(w, s, pos) } else { 0 },
        K::RedstoneWallTorchBlock => if facing(s) != dir { own_signal(w, s, pos) } else { 0 },
        K::RepeaterBlock | K::ComparatorBlock | K::ObserverBlock => if facing(s) == dir { own_signal(w, s, pos) } else { 0 },
        // CalibratedSculkSensorBlock
        K::SculkSensorBlock if has(s, P_FACING) => if dir != facing(s) { power(s) } else { 0 },
        _ => own_signal(w, s, pos),
    }
}

/// BlockState.getDirectSignal(level, pos, direction)
pub fn state_direct_signal(w: &World, s: S, pos: P, dir: Dir) -> i32 {
    match kind_of(s) {
        K::LightningRodBlock | K::TripWireHookBlock => if powered(s) && facing(s) == dir { 15 } else { 0 },
        K::PressurePlateBlock | K::WeightedPressurePlateBlock => if dir == UP { own_signal(w, s, pos) } else { 0 },
        K::ButtonBlock | K::LeverBlock => if powered(s) && connected_direction(s) == dir { 15 } else { 0 },
        K::DetectorRailBlock | K::LecternBlock => if dir == UP && powered(s) { 15 } else { 0 },
        K::ObserverBlock | K::RepeaterBlock | K::ComparatorBlock => state_signal(w, s, pos, dir),
        K::RedStoneWireBlock => if !side(|x| x.wire_signal) { 0 } else { state_signal(w, s, pos, dir) },
        K::RedstoneTorchBlock | K::RedstoneWallTorchBlock => if dir == DOWN { state_signal(w, s, pos, dir) } else { 0 },
        K::SculkSensorBlock => if dir == UP { state_signal(w, s, pos, dir) } else { 0 },
        // trapped chest: getSignal when asked from above: its open count, never above 0 here
        _ => 0,
    }
}

/// Per block state: bit 0 = its kind can give a signal at all (every kind named in `own_signal`, `state_signal` or
/// `state_direct_signal`: for any other kind all three are 0), bit 1 = `is_redstone_conductor`. Most questions about
/// power are put to air and plain blocks. KEEP IN STEP with those three functions (MCSIM_UPDCHECK=1 asks them all the
/// same and panics if a kind left out here gives a signal).
fn sig_class() -> &'static [u8] {
    static V: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    V.get_or_init(|| (0..t().block.len()).map(|i| {
        let s = i as S;
        let src = matches!(kind_of(s), K::PressurePlateBlock | K::LeverBlock | K::ButtonBlock | K::LightningRodBlock | K::DetectorRailBlock | K::LecternBlock
            | K::ObserverBlock | K::TripWireHookBlock | K::WeightedPressurePlateBlock | K::DaylightDetectorBlock | K::TargetBlock | K::SculkSensorBlock
            | K::RedStoneWireBlock | K::RepeaterBlock | K::ComparatorBlock | K::RedstoneTorchBlock | K::RedstoneWallTorchBlock | K::PoweredBlock);
        src as u8 | (is_redstone_conductor(s) as u8) << 1
    }).collect())
}
fn sig_check() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCSIM_UPDCHECK").is_some())
}

/// SignalGetter.getDirectSignal
pub fn get_direct_signal(w: &World, pos: P, dir: Dir) -> i32 {
    let s = w.get(pos);
    if sig_class()[s as usize] & 1 == 0 {
        if sig_check() { assert!(state_direct_signal(w, s, pos, dir) == 0, "direct signal from a kind that is not a source"); }
        return 0;
    }
    state_direct_signal(w, s, pos, dir)
}

/// SignalGetter.getDirectSignalTo
pub fn get_direct_signal_to(w: &World, pos: P) -> i32 {
    let mut r = 0;
    for d in DIRS {
        r = r.max(get_direct_signal(w, pos.rel(d), d));
        if r >= 15 { return r; }
    }
    r
}

/// SignalGetter.getSignal
pub fn get_signal(w: &World, pos: P, dir: Dir) -> i32 {
    let s = w.get(pos);
    let c = sig_class()[s as usize];
    if c & 1 == 0 && sig_check() { assert!(state_signal(w, s, pos, dir) == 0, "signal from a kind that is not a source"); }
    let sig = if c & 1 != 0 { state_signal(w, s, pos, dir) } else { 0 };
    if c & 2 != 0 { sig.max(get_direct_signal_to(w, pos)) } else { sig }
}
/// SignalGetter.hasSignal
pub fn has_signal(w: &World, pos: P, dir: Dir) -> bool { get_signal(w, pos, dir) > 0 }

/// SignalGetter.hasNeighborSignal
pub fn has_neighbor_signal(w: &World, pos: P) -> bool {
    for d in DIRS {
        if get_signal(w, pos.rel(d), d) > 0 { return true; }
    }
    false
}

/// SignalGetter.getBestNeighborSignal
pub fn get_best_neighbor_signal(w: &World, pos: P) -> i32 {
    let mut best = 0;
    for d in DIRS {
        let sig = get_signal(w, pos.rel(d), d);
        if sig >= 15 { return 15; }
        if sig > best { best = sig; }
    }
    best
}

/// SignalGetter.getControlInputSignal
pub fn get_control_input_signal(w: &World, pos: P, dir: Dir, only_diodes: bool) -> i32 {
    let s = w.get(pos);
    if only_diodes {
        if is_diode(s) { get_direct_signal(w, pos, dir) } else { 0 }
    } else if is_blk!(s, "redstone_block") {
        15
    } else if is_blk!(s, "redstone_wire") {
        power(s)
    } else if is_signal_source(s) {
        get_direct_signal(w, pos, dir)
    } else {
        0
    }
}

/// BlockState.hasAnalogOutputSignal
pub fn has_analog_output(s: S) -> bool {
    matches!(kind_of(s),
        K::CauldronBlock | K::LayeredCauldronBlock | K::LavaCauldronBlock | K::BeehiveBlock | K::FurnaceBlock | K::BrewingStandBlock | K::DetectorRailBlock
        | K::LecternBlock | K::RespawnAnchorBlock | K::CrafterBlock | K::DecoratedPotBlock | K::DispenserBlock | K::CreakingHeartBlock | K::CopperBulbBlock
        | K::EndPortalFrameBlock | K::JukeboxBlock | K::ChiseledBookShelfBlock | K::HopperBlock | K::ShelfBlock | K::ShulkerBoxBlock
        | K::CopperGolemStatueBlock | K::BarrelBlock | K::CommandBlock | K::ComposterBlock | K::ChestBlock | K::CandleCakeBlock | K::SculkSensorBlock
        | K::CakeBlock)
}

/// BlockState.getAnalogOutputSignal
pub fn analog_output_signal(w: &World, s: S, pos: P, dir: Dir) -> i32 {
    match kind_of(s) {
        K::LayeredCauldronBlock => get(s, P_LEVEL).unwrap_or(0) as i32,
        K::LavaCauldronBlock => 3,
        K::BeehiveBlock => get(s, P_HONEY_LEVEL).unwrap_or(0) as i32,
        K::ComposterBlock => get(s, P_LEVEL).unwrap_or(0) as i32,
        // RespawnAnchorBlock.getScaledChargeLevel(state, 15)
        K::RespawnAnchorBlock => (get(s, P_CHARGES).unwrap_or(0) as f32 / 4.0 * 15.0).floor() as i32,
        K::CopperBulbBlock => if is_true(w.get(pos), P_LIT) { 15 } else { 0 },
        K::EndPortalFrameBlock => if is_true(s, P_EYE) { 15 } else { 0 },
        K::CakeBlock => (7 - get(s, P_BITES).unwrap_or(0) as i32) * 2,
        K::CandleCakeBlock => 14,
        K::CopperGolemStatueBlock => match get(s, P_COPPER_GOLEM_POSE) { Some(V_SITTING) => 2, Some(V_RUNNING) => 3, Some(V_STAR) => 4, _ => 1 },
        // SculkSensorBlock: the frequency of the last vibration while active
        K::SculkSensorBlock => if get(s, P_SCULK_SENSOR_PHASE) == Some(V_ACTIVE) { super::vibrations::last_frequency(pos) } else { 0 },
        // LecternBlockEntity.getRedstoneSignal with a book whose pages are not known here: a book open at its first of
        // several pages reads 1
        K::LecternBlock => if is_true(s, P_HAS_BOOK) { 1 } else { 0 },
        // containers (chest, barrel, furnace, hopper, dispenser, crafter, shulker box, brewing stand, decorated pot,
        // shelf, chiseled bookshelf) are empty; jukebox without disc, detector rail without
        // minecart, command block never run, creaking heart without creaking, empty cauldron: 0
        _ => 0,
    }
}

/// Level.updateNeighbourForOutputSignal
pub fn update_neighbour_for_output_signal(w: &mut World, pos: P, block: u16) {
    for d in HORIZONTAL {
        let mut rp = pos.rel(d);
        let mut st = w.get(rp);
        if is_blk!(st, "comparator") {
            w.neighbor_changed_state(st, rp, block, false);
        } else if is_redstone_conductor(st) {
            rp = rp.rel(d);
            st = w.get(rp);
            if is_blk!(st, "comparator") { w.neighbor_changed_state(st, rp, block, false); }
        }
    }
}

// ------------------------------------------------------------------------------------------------ redstone wire
fn wire_block() -> u16 { blk!("redstone_wire") }
fn wire_default() -> S {
    let mut s = default_state(wire_block());
    for d in HORIZONTAL { s = with(s, dir_prop(d), V_NONE); }
    with(s, P_POWER, 0)
}
fn wire_cross() -> S {
    let mut s = wire_default();
    for d in HORIZONTAL { s = with(s, dir_prop(d), V_SIDE); }
    s
}
#[inline]
fn wire_connected(s: S, d: Dir) -> bool { matches!(get(s, dir_prop(d)), Some(V_SIDE) | Some(V_UP)) }
fn wire_is_cross(s: S) -> bool { HORIZONTAL.iter().all(|d| wire_connected(s, *d)) }
fn wire_is_dot(s: S) -> bool { HORIZONTAL.iter().all(|d| !wire_connected(s, *d)) }
/// RedStoneWireBlock.canSurviveOn
fn wire_can_survive_on(below: S) -> bool { super::face_sturdy(below, UP, 0) || is_blk!(below, "hopper") }

/// RedStoneWireBlock.shouldConnectTo
fn wire_should_connect_to(s: S, dir: Option<Dir>) -> bool {
    if is_blk!(s, "redstone_wire") {
        true
    } else if is_blk!(s, "repeater") {
        let f = facing(s);
        dir == Some(f) || dir == Some(opposite(f))
    } else if is_blk!(s, "observer") {
        dir == Some(facing(s))
    } else {
        is_signal_source(s) && dir.is_some()
    }
}

/// RedStoneWireBlock.getConnectingSide: the value code (up/side/none)
fn wire_connecting_side(w: &World, pos: P, d: Dir, can_connect_up: bool) -> u8 {
    let rp = pos.rel(d);
    let rs = w.get(rp);
    if can_connect_up {
        let placeable_above = kind_of(rs) == K::TrapDoorBlock || wire_can_survive_on(rs);
        if placeable_above && wire_should_connect_to(w.get(rp.above()), None) {
            if super::face_sturdy(rs, opposite(d), 0) { return V_UP; }
            return V_SIDE;
        }
    }
    if !wire_should_connect_to(rs, Some(d)) && (is_redstone_conductor(rs) || !wire_should_connect_to(w.get(rp.below()), None)) { V_NONE } else { V_SIDE }
}

/// RedStoneWireBlock.getMissingConnections
fn wire_missing_connections(w: &World, mut s: S, pos: P) -> S {
    let can_connect_up = !is_redstone_conductor(w.get(pos.above()));
    for d in HORIZONTAL {
        if !wire_connected(s, d) {
            s = with(s, dir_prop(d), wire_connecting_side(w, pos, d, can_connect_up));
        }
    }
    s
}

/// RedStoneWireBlock.getConnectionState
pub fn wire_connection_state(w: &World, state: S, pos: P) -> S {
    let was_dot = wire_is_dot(state);
    let mut s = wire_missing_connections(w, with(wire_default(), P_POWER, power(state) as u8), pos);
    if was_dot && wire_is_dot(s) { return s; }
    let (n, so, e, we) = (wire_connected(s, NORTH), wire_connected(s, SOUTH), wire_connected(s, EAST), wire_connected(s, WEST));
    let ns_empty = !n && !so;
    let ew_empty = !e && !we;
    if !we && ns_empty { s = with(s, P_WEST, V_SIDE); }
    if !e && ns_empty { s = with(s, P_EAST, V_SIDE); }
    if !n && ew_empty { s = with(s, P_NORTH, V_SIDE); }
    if !so && ew_empty { s = with(s, P_SOUTH, V_SIDE); }
    s
}

/// RedStoneWireBlock.getStateForPlacement
pub fn wire_placement_state(w: &World, pos: P) -> S { wire_connection_state(w, wire_cross(), pos) }

fn wire_update_shape(w: &World, s: S, pos: P, dir: Dir, ns: S) -> S {
    if dir == DOWN {
        return if !wire_can_survive_on(ns) { 0 } else { s };
    }
    if dir == UP {
        return wire_connection_state(w, s, pos);
    }
    let sc = wire_connecting_side(w, pos, dir, !is_redstone_conductor(w.get(pos.above())));
    if (sc != V_NONE) == wire_connected(s, dir) && !wire_is_cross(s) {
        with(s, dir_prop(dir), sc)
    } else {
        wire_connection_state(w, with(with(wire_cross(), P_POWER, power(s) as u8), dir_prop(dir), sc), pos)
    }
}

/// RedStoneWireBlock.getBlockSignal
fn wire_block_signal(w: &World, pos: P) -> i32 {
    side(|x| x.wire_signal = false);
    let r = get_best_neighbor_signal(w, pos);
    side(|x| x.wire_signal = true);
    r
}

/// RedstoneWireEvaluator.getIncomingWireSignal
fn wire_incoming_signal(w: &World, pos: P) -> i32 {
    let wire_sig = |q: P| -> i32 {
        let st = w.get(q);
        if is_blk!(st, "redstone_wire") { power(st) } else { 0 }
    };
    let mut sig = 0;
    for d in HORIZONTAL {
        let np = pos.rel(d);
        let ns = w.get(np);
        sig = sig.max(wire_sig(np));
        let above = pos.above();
        if is_redstone_conductor(ns) && !is_redstone_conductor(w.get(above)) {
            sig = sig.max(wire_sig(np.above()));
        } else if !is_redstone_conductor(ns) {
            sig = sig.max(wire_sig(np.below()));
        }
    }
    (sig - 1).max(0)
}

/// DefaultRedstoneWireEvaluator.updatePowerStrength
fn wire_update_power_strength(w: &mut World, pos: P, state: S) {
    let block_signal = wire_block_signal(w, pos);
    let target = if block_signal == 15 { 15 } else { block_signal.max(wire_incoming_signal(w, pos)) };
    if power(state) != target {
        if w.get(pos) == state {
            w.set_block(pos, with(state, P_POWER, target as u8), 2);
        }
        // Set<BlockPos> toUpdate = Sets.newHashSet(): the position and its six neighbours, visited in hash order
        let mut to_update: Vec<(P, ())> = vec![(pos, ())];
        for d in DIRS { to_update.push((pos.rel(d), ())); }
        for (q, _) in java_hash_order(&to_update) {
            w.update_neighbors_at(q, wire_block());
        }
    }
}

/// RedStoneWireBlock.checkCornerChangeAt
fn wire_check_corner_change_at(w: &mut World, pos: P) {
    if is_blk!(w.get(pos), "redstone_wire") {
        w.update_neighbors_at(pos, wire_block());
        for d in DIRS { w.update_neighbors_at(pos.rel(d), wire_block()); }
    }
}

/// RedStoneWireBlock.updateNeighborsOfNeighboringWires
fn wire_update_neighbors_of_neighboring_wires(w: &mut World, pos: P) {
    for d in HORIZONTAL { wire_check_corner_change_at(w, pos.rel(d)); }
    for d in HORIZONTAL {
        let target = pos.rel(d);
        if is_redstone_conductor(w.get(target)) {
            wire_check_corner_change_at(w, target.above());
        } else {
            wire_check_corner_change_at(w, target.below());
        }
    }
}

/// RedStoneWireBlock.updateIndirectNeighbourShapes (the only override of that hook)
pub fn update_indirect_neighbour_shapes(w: &mut World, s: S, pos: P, flags: u32, limit: i32) {
    if !is_blk!(s, "redstone_wire") { return; }
    for d in HORIZONTAL {
        if get(s, dir_prop(d)) != Some(V_NONE) && !is_blk!(w.get(pos.rel(d)), "redstone_wire") {
            let down = pos.rel(d).below();
            if is_blk!(w.get(down), "redstone_wire") {
                let np = down.rel(opposite(d));
                let ns = w.get(np);
                w.neighbor_shape_changed(opposite(d), down, np, ns, flags, limit);
            }
            let up = pos.rel(d).above();
            if is_blk!(w.get(up), "redstone_wire") {
                let np = up.rel(opposite(d));
                let ns = w.get(np);
                w.neighbor_shape_changed(opposite(d), up, np, ns, flags, limit);
            }
        }
    }
}

/// RedStoneWireBlock.useWithoutItem
fn wire_use(w: &mut World, s: S, pos: P) -> Use {
    if wire_is_cross(s) || wire_is_dot(s) {
        let mut ns = if wire_is_cross(s) { wire_default() } else { wire_cross() };
        ns = with(ns, P_POWER, power(s) as u8);
        ns = wire_connection_state(w, ns, pos);
        if ns != s {
            w.set_block(pos, ns, 3);
            // updatesOnShapeChange
            for d in HORIZONTAL {
                let rp = pos.rel(d);
                if wire_connected(s, d) != wire_connected(ns, d) && is_redstone_conductor(w.get(rp)) {
                    w.update_neighbors_at_except(rp, wire_block(), opposite(d));
                }
            }
            return Use::Consume;
        }
    }
    Use::Pass
}

// ------------------------------------------------------------------------------------------------ diodes
/// DiodeBlock.isDiode
pub fn is_diode(s: S) -> bool { matches!(kind_of(s), K::RepeaterBlock | K::ComparatorBlock) }
fn comparator_output(pos: P) -> i32 { side(|x| x.comparator.get(&pos).copied().unwrap_or(0) as i32) }

fn diode_delay(s: S) -> i32 { if kind_of(s) == K::RepeaterBlock { get(s, P_DELAY).unwrap_or(1) as i32 * 2 } else { 2 } }

/// DiodeBlock.getAlternateSignal
fn diode_alternate_signal(w: &World, pos: P, s: S) -> i32 {
    let d = facing(s);
    let cw = clockwise(d);
    let ccw = counter_clockwise(d);
    // RepeaterBlock.sideInputDiodesOnly
    let only = kind_of(s) == K::RepeaterBlock;
    get_control_input_signal(w, pos.rel(cw), cw, only).max(get_control_input_signal(w, pos.rel(ccw), ccw, only))
}

/// DiodeBlock.isLocked (RepeaterBlock; also what a repeater's placement state asks)
pub fn diode_locked(w: &World, pos: P, s: S) -> bool { kind_of(s) == K::RepeaterBlock && diode_alternate_signal(w, pos, s) > 0 }

/// DiodeBlock.getInputSignal, with ComparatorBlock's override
fn diode_input_signal(w: &World, pos: P, s: S) -> i32 {
    let d = facing(s);
    let tp = pos.rel(d);
    let ts = w.get(tp);
    let mut result = {
        let input = get_signal(w, tp, d);
        if input >= 15 { input } else { input.max(if is_blk!(ts, "redstone_wire") { power(ts) } else { 0 }) }
    };
    if kind_of(s) == K::ComparatorBlock {
        if has_analog_output(ts) {
            result = analog_output_signal(w, ts, tp, opposite(d));
        } else if result < 15 && is_redstone_conductor(ts) {
            let tp2 = tp.rel(d);
            let ts2 = w.get(tp2);
            // (no item frames in this world)
            if has_analog_output(ts2) { result = analog_output_signal(w, ts2, tp2, opposite(d)); }
        }
    }
    result
}

/// DiodeBlock.shouldTurnOn, with ComparatorBlock's override
fn diode_should_turn_on(w: &World, pos: P, s: S) -> bool {
    let input = diode_input_signal(w, pos, s);
    if kind_of(s) == K::ComparatorBlock {
        if input == 0 { return false; }
        let side_input = diode_alternate_signal(w, pos, s);
        if input > side_input { true } else { input == side_input && get(s, P_MODE) == Some(V_COMPARE) }
    } else {
        input > 0
    }
}

/// ComparatorBlock.calculateOutputSignal
fn comparator_calculate_output(w: &World, pos: P, s: S) -> i32 {
    let input = diode_input_signal(w, pos, s);
    if input == 0 { return 0; }
    let alt = diode_alternate_signal(w, pos, s);
    if alt > input { 0 } else if get(s, P_MODE) == Some(V_SUBTRACT) { input - alt } else { input }
}

/// DiodeBlock.shouldPrioritize
fn diode_should_prioritize(w: &World, pos: P, s: S) -> bool {
    let d = opposite(facing(s));
    let os = w.get(pos.rel(d));
    is_diode(os) && facing(os) != d
}

/// DiodeBlock.updateNeighborsInFront
fn diode_update_neighbors_in_front(w: &mut World, pos: P, s: S) {
    let d = facing(s);
    let op = pos.rel(opposite(d));
    let b = block_of(s);
    w.neighbor_changed(op, b);
    w.update_neighbors_at_except(op, b, d);
}

/// DiodeBlock.checkTickOnNeighbor, with ComparatorBlock's override
fn diode_check_tick_on_neighbor(w: &mut World, pos: P, s: S) {
    let b = block_of(s);
    if kind_of(s) == K::ComparatorBlock {
        if !will_tick_this_tick(w, pos, b) {
            let out = comparator_calculate_output(w, pos, s);
            let old = comparator_output(pos);
            if out != old || powered(s) != diode_should_turn_on(w, pos, s) {
                let prio = if diode_should_prioritize(w, pos, s) { PRIO_HIGH } else { PRIO_NORMAL };
                sched(w, pos, b, 2, prio);
            }
        }
        return;
    }
    if !diode_locked(w, pos, s) {
        let on = powered(s);
        let should = diode_should_turn_on(w, pos, s);
        if on != should && !will_tick_this_tick(w, pos, b) {
            let mut prio = PRIO_HIGH;
            if diode_should_prioritize(w, pos, s) {
                prio = PRIO_EXTREMELY_HIGH;
            } else if on {
                prio = PRIO_VERY_HIGH;
            }
            sched(w, pos, b, diode_delay(s), prio);
        }
    }
}

/// ComparatorBlock.refreshOutputState
fn comparator_refresh_output_state(w: &mut World, pos: P, s: S) {
    let out = comparator_calculate_output(w, pos, s);
    // (the block entity is there whenever the block is)
    let old = comparator_output(pos);
    side(|x| { x.comparator.insert(pos, out as u8); });
    if old != out || get(s, P_MODE) == Some(V_COMPARE) {
        let source_on = diode_should_turn_on(w, pos, s);
        let is_on = powered(s);
        if is_on && !source_on {
            w.set_block(pos, with_bool(s, P_POWERED, false), 2);
        } else if !is_on && source_on {
            w.set_block(pos, with_bool(s, P_POWERED, true), 2);
        }
        diode_update_neighbors_in_front(w, pos, s);
    }
}

/// DiodeBlock.tick (RepeaterBlock)
fn repeater_tick(w: &mut World, s: S, pos: P) {
    if !diode_locked(w, pos, s) {
        let on = powered(s);
        let should = diode_should_turn_on(w, pos, s);
        if on && !should {
            w.set_block(pos, with_bool(s, P_POWERED, false), 2);
        } else if !on {
            w.set_block(pos, with_bool(s, P_POWERED, true), 2);
            if !should { sched(w, pos, block_of(s), diode_delay(s), PRIO_VERY_HIGH); }
        }
    }
}

// ------------------------------------------------------------------------------------------------ torches
/// RedstoneTorchBlock.hasNeighborSignal (and RedstoneWallTorchBlock's)
fn torch_has_neighbor_signal(w: &World, pos: P, s: S) -> bool {
    if kind_of(s) == K::RedstoneWallTorchBlock {
        let o = opposite(facing(s));
        has_signal(w, pos.rel(o), o)
    } else {
        has_signal(w, pos.below(), DOWN)
    }
}
/// RedstoneTorchBlock.notifyNeighbors
fn torch_notify_neighbors(w: &mut World, pos: P, s: S) {
    for d in DIRS { w.update_neighbors_at(pos.rel(d), block_of(s)); }
}
/// RedstoneTorchBlock.isToggledTooFrequently
fn torch_toggled_too_frequently(pos: P, add: bool, now: i64) -> bool {
    side(|x| {
        if add { x.toggles.push((pos, now)); }
        let mut count = 0;
        for (q, _) in &x.toggles {
            if *q == pos {
                count += 1;
                if count >= 8 { return true; }
            }
        }
        false
    })
}
/// RedstoneTorchBlock.tick
fn torch_tick(w: &mut World, s: S, pos: P) {
    let ns = torch_has_neighbor_signal(w, pos, s);
    let now = w.game_time;
    side(|x| while !x.toggles.is_empty() && now - x.toggles[0].1 > 60 { x.toggles.remove(0); });
    if is_true(s, P_LIT) {
        if ns {
            w.set_block(pos, with_bool(s, P_LIT, false), 3);
            if torch_toggled_too_frequently(pos, true, now) {
                // (levelEvent 1502: the fizz) the burnt-out torch is looked at again after RESTART_DELAY
                let b = block_of(w.get(pos));
                sched(w, pos, b, 160, PRIO_NORMAL);
            }
        }
    } else if !ns && !torch_toggled_too_frequently(pos, false, now) {
        w.set_block(pos, with_bool(s, P_LIT, true), 3);
    }
}

// ------------------------------------------------------------------------------------------------ entities on plates and wires
/// Number of entities of the asked kind whose box intersects `bx` (Level.getEntitiesOfClass). The world knows the
/// player and whatever else the integrator lists in `entities`, plus falling blocks.
fn entity_count(w: &World, bx: &Aabb, living_only: bool) -> usize {
    let mut n = w.entities.iter().filter(|e| (!living_only || e.is_player) && e.bb.intersects(bx)).count();
    if !living_only {
        n += w
            .falling
            .iter()
            .filter(|f| !f.removed && Aabb::new(f.pos[0] - 0.49, f.pos[1], f.pos[2] - 0.49, f.pos[0] + 0.49, f.pos[1] + 0.98, f.pos[2] + 0.49).intersects(bx))
            .count();
    }
    n
}
fn block_box(pos: P, x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) -> Aabb {
    Aabb::new(pos.x as f64 + x0, pos.y as f64 + y0, pos.z as f64 + z0, pos.x as f64 + x1, pos.y as f64 + y1, pos.z as f64 + z1)
}
/// state.getShape(level, pos).bounds().move(pos)
fn outline_bounds(s: S, pos: P) -> Option<Aabb> {
    let tb = t();
    let sh = &tb.shapes[tb.out_shape[s as usize] as usize];
    if sh.empty { return None; }
    Some(block_box(pos, sh.min[0], sh.min[1], sh.min[2], sh.max[0], sh.max[1], sh.max[2]))
}

// ---- pressure plates
fn plate_signal_for_state(s: S) -> i32 { if kind_of(s) == K::WeightedPressurePlateBlock { power(s) } else if powered(s) { 15 } else { 0 } }
/// BasePressurePlateBlock.getSignalStrength
fn plate_signal_strength(w: &World, s: S, pos: P) -> i32 {
    // TOUCH_AABB = Block.column(14, 0, 4)
    let bx = block_box(pos, 0.0625, 0.0, 0.0625, 0.9375, 0.25, 0.9375);
    if kind_of(s) == K::WeightedPressurePlateBlock {
        let max_weight: i32 = if is_blk!(s, "light_weighted_pressure_plate") { 15 } else { 150 };
        let count = (entity_count(w, &bx, false) as i32).min(max_weight);
        if count > 0 {
            let percent = count.min(max_weight) as f32 / max_weight as f32;
            (percent * 15.0).ceil() as i32
        } else {
            0
        }
    } else {
        // BlockSetType.pressurePlateSensitivity: stone and polished blackstone react to living entities only
        let mobs = in_tag!(s, "stone_pressure_plates");
        if entity_count(w, &bx, mobs) > 0 { 15 } else { 0 }
    }
}
/// BasePressurePlateBlock.checkPressed
fn plate_check_pressed(w: &mut World, pos: P, s: S, old_signal: i32) {
    let signal = plate_signal_strength(w, s, pos);
    let is_pressed = signal > 0;
    let b = block_of(s);
    if old_signal != signal {
        let ns = if kind_of(s) == K::WeightedPressurePlateBlock { with(s, P_POWER, signal as u8) } else { with_bool(s, P_POWERED, signal > 0) };
        w.set_block(pos, ns, 2);
        plate_update_neighbours(w, pos, b);
    }
    let was_pressed = old_signal > 0;
    if !is_pressed && was_pressed {
        vib::emit_at(w, vib::Ev::BlockDeactivate, pos, vib::Ctx::NONE);
    } else if is_pressed && !was_pressed {
        vib::emit_at(w, vib::Ev::BlockActivate, pos, vib::Ctx::NONE);
    }
    if is_pressed {
        let t = if kind_of(s) == K::WeightedPressurePlateBlock { 10 } else { 20 };
        sched(w, pos, b, t, PRIO_NORMAL);
    }
}
fn plate_update_neighbours(w: &mut World, pos: P, b: u16) {
    w.update_neighbors_at(pos, b);
    w.update_neighbors_at(pos.below(), b);
}

// ---- lever, buttons
/// LeverBlock.updateNeighbours / ButtonBlock.updateNeighbours
fn face_attached_update_neighbours(w: &mut World, s: S, pos: P) {
    let front = opposite(connected_direction(s));
    let b = block_of(s);
    w.update_neighbors_at(pos, b);
    w.update_neighbors_at(pos.rel(front), b);
}
fn button_ticks(s: S) -> i32 { if in_tag!(s, "stone_buttons") { 20 } else { 30 } }

// ---- tripwire
fn tripwire_should_connect_to(s: S, d: Dir) -> bool {
    if is_blk!(s, "tripwire_hook") { facing(s) == opposite(d) } else { is_blk!(s, "tripwire") }
}
/// TripWireBlock.updateSource
fn tripwire_update_source(w: &mut World, pos: P, state: S) {
    for d in [SOUTH, WEST] {
        for i in 1..42 {
            let tp = pos.rel_n(d, i);
            let st = w.get(tp);
            if is_blk!(st, "tripwire_hook") {
                if facing(st) == opposite(d) {
                    hook_calculate_state(w, tp, st, false, true, i, Some(state));
                }
                break;
            }
            if !is_blk!(st, "tripwire") { break; }
        }
    }
}
/// TripWireBlock.checkPressed(level, pos, entities)
fn tripwire_check_pressed(w: &mut World, pos: P, should_be_pressed: bool) {
    let mut s = w.get(pos);
    if !is_blk!(s, "tripwire") { return; }
    let was = powered(s);
    let b = block_of(s);
    if should_be_pressed != was {
        s = with_bool(s, P_POWERED, should_be_pressed);
        w.set_block(pos, s, 3);
        tripwire_update_source(w, pos, s);
    }
    if should_be_pressed {
        sched(w, pos, b, 10, PRIO_NORMAL);
    } else if was {
        sched(w, pos, b, 0, PRIO_NORMAL);
    }
}
/// TripWireHookBlock.notifyNeighbors
fn hook_notify_neighbors(w: &mut World, b: u16, pos: P, d: Dir) {
    let front = opposite(d);
    w.update_neighbors_at(pos, b);
    w.update_neighbors_at(pos.rel(front), b);
}
/// TripWireHookBlock.calculateState
fn hook_calculate_state(w: &mut World, pos: P, state: S, being_destroyed: bool, can_update: bool, wire_source: i32, wire_source_state: Option<S>) {
    let Some(fv) = get(state, P_FACING) else { return };
    let d = dir_of(fv);
    let was_attached = is_true(state, P_ATTACHED);
    let was_powered = powered(state);
    let b = block_of(state);
    let mut attached = !being_destroyed;
    let mut pw = false;
    let mut receiver = 0;
    let mut wire_states: [Option<S>; 42] = [None; 42];
    for i in 1..42 {
        let tp = pos.rel_n(d, i);
        let mut ws = w.get(tp);
        if is_blk!(ws, "tripwire_hook") {
            if facing(ws) == opposite(d) { receiver = i; }
            break;
        }
        if !is_blk!(ws, "tripwire") && i != wire_source {
            wire_states[i as usize] = None;
            attached = false;
        } else {
            if i == wire_source {
                if let Some(x) = wire_source_state { ws = x; }
            }
            let armed = !is_true(ws, P_DISARMED);
            let wire_powered = powered(ws);
            pw |= armed && wire_powered;
            wire_states[i as usize] = Some(ws);
            if i == wire_source {
                sched(w, pos, b, 10, PRIO_NORMAL);
                attached &= armed;
            }
        }
    }
    attached &= receiver > 1;
    pw &= attached;
    let ns = try_with(try_with(default_state(b), P_ATTACHED, if attached { V_TRUE } else { V_FALSE }), P_POWERED, if pw { V_TRUE } else { V_FALSE });
    if receiver > 0 {
        let tp = pos.rel_n(d, receiver);
        let o = opposite(d);
        w.set_block(tp, try_with(ns, P_FACING, dir_val(o)), 3);
        hook_notify_neighbors(w, b, tp, o);
        if !is_blk!(w.get(pos), "tripwire_hook") {
            hook_on_removed(w, ns, pos);
            return;
        }
        hook_emit_state(w, tp, attached, pw, was_attached, was_powered);
    }
    hook_emit_state(w, pos, attached, pw, was_attached, was_powered);
    if !being_destroyed {
        w.set_block(pos, try_with(ns, P_FACING, dir_val(d)), 3);
        if can_update { hook_notify_neighbors(w, b, pos, d); }
    }
    if was_attached != attached {
        for i in 1..receiver {
            let tp = pos.rel_n(d, i);
            if let Some(wd) = wire_states[i as usize] {
                let ts = w.get(tp);
                if is_blk!(ts, "tripwire") || is_blk!(ts, "tripwire_hook") {
                    w.set_block(tp, try_with(wd, P_ATTACHED, if attached { V_TRUE } else { V_FALSE }), 3);
                }
            }
        }
    }
}
/// TripWireHookBlock.emitState (the game events; sounds are not modelled)
fn hook_emit_state(w: &World, pos: P, attached: bool, pw: bool, was_attached: bool, was_powered: bool) {
    let ev = if pw && !was_powered {
        vib::Ev::BlockActivate
    } else if !pw && was_powered {
        vib::Ev::BlockDeactivate
    } else if attached && !was_attached {
        vib::Ev::BlockAttach
    } else if !attached && was_attached {
        vib::Ev::BlockDetach
    } else {
        return;
    };
    vib::emit_at(w, ev, pos, vib::Ctx::NONE);
}
/// TripWireHookBlock.onRemoved
fn hook_on_removed(w: &mut World, state: S, pos: P) {
    let attached = is_true(state, P_ATTACHED);
    let pw = powered(state);
    if attached || pw { hook_calculate_state(w, pos, state, true, false, -1, None); }
    if pw { hook_notify_neighbors(w, block_of(state), pos, facing(state)); }
}

// ------------------------------------------------------------------------------------------------ misc blocks
/// NoteBlock.setInstrument (also the note block's placement state)
pub fn note_set_instrument(w: &World, pos: P, s: S) -> S {
    let works_above = |v: u8| matches!(v, V_ZOMBIE | V_SKELETON | V_CREEPER | V_DRAGON | V_WITHER_SKELETON | V_PIGLIN | V_CUSTOM_HEAD);
    let above = instrument_of(w.get(pos.above()));
    if works_above(above) { return try_with(s, P_INSTRUMENT, above); }
    let below = instrument_of(w.get(pos.below()));
    try_with(s, P_INSTRUMENT, if works_above(below) { V_HARP } else { below })
}
/// NoteBlock.playNote
fn note_play(w: &mut World, s: S, pos: P) {
    let ins = get(s, P_INSTRUMENT).unwrap_or(V_HARP);
    let works_above = matches!(ins, V_ZOMBIE | V_SKELETON | V_CREEPER | V_DRAGON | V_WITHER_SKELETON | V_PIGLIN | V_CUSTOM_HEAD);
    if works_above || is_air(w.get(pos.above())) {
        w.block_event(pos, block_of(s), 0, 0);
        vib::emit_at(w, vib::Ev::NoteBlockPlay, pos, vib::Ctx::NONE);
    }
}

/// BellBlock.getConnectedDirection
fn bell_connected_direction(s: S) -> Dir {
    match get(s, P_ATTACHMENT) {
        Some(V_FLOOR) => UP,
        Some(V_CEILING) => DOWN,
        _ => opposite(facing(s)),
    }
}
/// BellBlock.attemptToRing: BellBlockEntity.onHit sends a block event; the bell's state does not change
fn bell_ring(w: &mut World, pos: P, dir: Option<Dir>) {
    let d = dir.unwrap_or_else(|| facing(w.get(pos)));
    let b = block_of(w.get(pos));
    w.block_event(pos, b, 1, d);
    vib::emit_at(w, vib::Ev::BlockChange, pos, vib::Ctx::NONE);
}
fn bell_update_shape(w: &World, s: S, pos: P, dir: Dir, npos: P, ns: S) -> S {
    let att = get(s, P_ATTACHMENT);
    let cd = opposite(bell_connected_direction(s));
    if cd == dir && !can_survive(w, s, pos) && att != Some(V_DOUBLE_WALL) { return 0; }
    if axis(dir) == axis(facing(s)) {
        if att == Some(V_DOUBLE_WALL) && !super::face_sturdy(ns, dir, 0) {
            return with(with(s, P_ATTACHMENT, V_SINGLE_WALL), P_FACING, dir_val(opposite(dir)));
        }
        if att == Some(V_SINGLE_WALL) && opposite(cd) == dir && super::face_sturdy(ns, facing(s), 0) {
            return with(s, P_ATTACHMENT, V_DOUBLE_WALL);
        }
    }
    s
}

/// TntBlock.prime + removal. Vanilla spawns a primed TNT entity that explodes 80 ticks later; explosions are not
/// modelled: the block goes and the run is marked.
fn tnt_prime(w: &mut World, pos: P) {
    w.unmodelled += 1;
    w.remove_block(pos, false);
}

/// DaylightDetectorBlock.updateSignalStrength. Time is frozen at noon with a clear sky: the effective sky
/// brightness is 15 and the sun stands at its highest (cos = 1), so the target is 15, or 0 when inverted.
fn daylight_update_signal_strength(w: &mut World, s: S, pos: P) {
    // Level.getEffectiveSkyBrightness: sky light at the detector (the sky itself is at full brightness: frozen daytime,
    // clear weather). A detector under a roof sees less, and an inverted one then gives a signal.
    let light = sky_light(w, pos);
    let target: i32 = if is_true(s, P_INVERTED) { 15 - light } else { light };
    if power(s) != target { w.set_block(pos, with(s, P_POWER, target as u8), 3); }
}

/// CopperBulbBlock.checkAndFlip
fn bulb_check_and_flip(w: &mut World, s: S, pos: P) {
    let signal = has_neighbor_signal(w, pos);
    if signal != powered(s) {
        let mut ns = s;
        if !powered(s) { ns = cycle(ns, P_LIT); }
        w.set_block(pos, with_bool(ns, P_POWERED, signal), 3);
    }
}

/// HopperBlock.checkPoweredState
fn hopper_check_powered_state(w: &mut World, pos: P, s: S) {
    let should_be_on = !has_neighbor_signal(w, pos);
    if should_be_on != is_true(s, P_ENABLED) { w.set_block(pos, with_bool(s, P_ENABLED, should_be_on), 2); }
}

/// ObserverBlock.updateNeighborsInFront
fn observer_update_neighbors_in_front(w: &mut World, pos: P, s: S) {
    let d = facing(s);
    let op = pos.rel(opposite(d));
    let b = block_of(s);
    w.neighbor_changed(op, b);
    w.update_neighbors_at_except(op, b, d);
}

/// LightningRodBlock.updateNeighbours
fn rod_update_neighbours(w: &mut World, s: S, pos: P) {
    let front = opposite(facing(s));
    w.update_neighbors_at(pos.rel(front), block_of(s));
}
/// SculkSensorBlock.updateNeighbours
fn sculk_update_neighbours(w: &mut World, pos: P, s: S) {
    let b = block_of(s);
    w.update_neighbors_at(pos, b);
    w.update_neighbors_at(pos.below(), b);
}

// ------------------------------------------------------------------------------------------------ rails
fn rail_shape(s: S) -> u8 { get(s, P_SHAPE).unwrap_or(V_NORTH_SOUTH) }
fn rail_is_slope(shape: u8) -> bool { matches!(shape, V_ASCENDING_EAST | V_ASCENDING_WEST | V_ASCENDING_NORTH | V_ASCENDING_SOUTH) }
/// BaseRailBlock.isStraight (false for the plain rail only)
fn rail_is_straight(s: S) -> bool { kind_of(s) != K::RailBlock }
/// BaseRailBlock.isRail
fn is_rail(s: S) -> bool { super::is_rail(s) }

/// RailState
struct RailState {
    pos: P,
    state: S,
    straight: bool,
    connections: Vec<P>,
}

impl RailState {
    fn new(pos: P, state: S) -> RailState {
        let mut r = RailState { pos, state, straight: rail_is_straight(state), connections: Vec::new() };
        r.update_connections(rail_shape(state));
        r
    }
    fn update_connections(&mut self, shape: u8) {
        let q = self.pos;
        self.connections.clear();
        let (a, b) = match shape {
            V_NORTH_SOUTH => (q.rel(NORTH), q.rel(SOUTH)),
            V_EAST_WEST => (q.rel(WEST), q.rel(EAST)),
            V_ASCENDING_EAST => (q.rel(WEST), q.rel(EAST).above()),
            V_ASCENDING_WEST => (q.rel(WEST).above(), q.rel(EAST)),
            V_ASCENDING_NORTH => (q.rel(NORTH).above(), q.rel(SOUTH)),
            V_ASCENDING_SOUTH => (q.rel(NORTH), q.rel(SOUTH).above()),
            V_SOUTH_EAST => (q.rel(EAST), q.rel(SOUTH)),
            V_SOUTH_WEST => (q.rel(WEST), q.rel(SOUTH)),
            V_NORTH_WEST => (q.rel(WEST), q.rel(NORTH)),
            _ => (q.rel(EAST), q.rel(NORTH)),
        };
        self.connections.push(a);
        self.connections.push(b);
    }
    fn get_rail(w: &World, pos: P) -> Option<RailState> {
        for q in [pos, pos.above(), pos.below()] {
            let st = w.get(q);
            if is_rail(st) { return Some(RailState::new(q, st)); }
        }
        None
    }
    fn has_rail(w: &World, pos: P) -> bool { is_rail(w.get(pos)) || is_rail(w.get(pos.above())) || is_rail(w.get(pos.below())) }
    fn has_connection(&self, rp: P) -> bool { self.connections.iter().any(|c| c.x == rp.x && c.z == rp.z) }
    fn remove_soft_connections(&mut self, w: &World) {
        let mut i = 0;
        while i < self.connections.len() {
            match RailState::get_rail(w, self.connections[i]) {
                Some(r) if r.has_connection(self.pos) => {
                    self.connections[i] = r.pos;
                    i += 1;
                }
                _ => {
                    self.connections.remove(i);
                }
            }
        }
    }
    fn count_potential_connections(&self, w: &World) -> i32 { HORIZONTAL.iter().filter(|d| RailState::has_rail(w, self.pos.rel(**d))).count() as i32 }
    fn can_connect_to(&self, rail_pos: P) -> bool { self.has_connection(rail_pos) || self.connections.len() != 2 }
    fn connect_to(&mut self, w: &mut World, rail_pos: P) {
        self.connections.push(rail_pos);
        let (north, south, west, east) = (self.pos.rel(NORTH), self.pos.rel(SOUTH), self.pos.rel(WEST), self.pos.rel(EAST));
        let (n, s, we, e) = (self.has_connection(north), self.has_connection(south), self.has_connection(west), self.has_connection(east));
        let mut shape: Option<u8> = None;
        if n || s { shape = Some(V_NORTH_SOUTH); }
        if we || e { shape = Some(V_EAST_WEST); }
        if !self.straight {
            if s && e && !n && !we { shape = Some(V_SOUTH_EAST); }
            if s && we && !n && !e { shape = Some(V_SOUTH_WEST); }
            if n && we && !s && !e { shape = Some(V_NORTH_WEST); }
            if n && e && !s && !we { shape = Some(V_NORTH_EAST); }
        }
        if shape == Some(V_NORTH_SOUTH) {
            if is_rail(w.get(north.above())) { shape = Some(V_ASCENDING_NORTH); }
            if is_rail(w.get(south.above())) { shape = Some(V_ASCENDING_SOUTH); }
        }
        if shape == Some(V_EAST_WEST) {
            if is_rail(w.get(east.above())) { shape = Some(V_ASCENDING_EAST); }
            if is_rail(w.get(west.above())) { shape = Some(V_ASCENDING_WEST); }
        }
        let shape = shape.unwrap_or(V_NORTH_SOUTH);
        self.state = try_with(self.state, P_SHAPE, shape);
        w.set_block(self.pos, self.state, 3);
    }
    fn has_neighbor_rail(&self, w: &World, rp: P) -> bool {
        match RailState::get_rail(w, rp) {
            None => false,
            Some(mut nb) => {
                nb.remove_soft_connections(w);
                nb.can_connect_to(self.pos)
            }
        }
    }
    fn place(&mut self, w: &mut World, has_signal: bool, first: bool, default_shape: u8) {
        let (north, south, west, east) = (self.pos.rel(NORTH), self.pos.rel(SOUTH), self.pos.rel(WEST), self.pos.rel(EAST));
        let n = self.has_neighbor_rail(w, north);
        let s = self.has_neighbor_rail(w, south);
        let we = self.has_neighbor_rail(w, west);
        let e = self.has_neighbor_rail(w, east);
        let mut shape: Option<u8> = None;
        let north_or_south = n || s;
        let west_or_east = we || e;
        if north_or_south && !west_or_east { shape = Some(V_NORTH_SOUTH); }
        if west_or_east && !north_or_south { shape = Some(V_EAST_WEST); }
        let (south_and_east, south_and_west, north_and_east, north_and_west) = (s && e, s && we, n && e, n && we);
        if !self.straight {
            if south_and_east && !n && !we { shape = Some(V_SOUTH_EAST); }
            if south_and_west && !n && !e { shape = Some(V_SOUTH_WEST); }
            if north_and_west && !s && !e { shape = Some(V_NORTH_WEST); }
            if north_and_east && !s && !we { shape = Some(V_NORTH_EAST); }
        }
        if shape.is_none() {
            if north_or_south && west_or_east {
                shape = Some(default_shape);
            } else if north_or_south {
                shape = Some(V_NORTH_SOUTH);
            } else if west_or_east {
                shape = Some(V_EAST_WEST);
            }
            if !self.straight {
                if has_signal {
                    if south_and_east { shape = Some(V_SOUTH_EAST); }
                    if south_and_west { shape = Some(V_SOUTH_WEST); }
                    if north_and_east { shape = Some(V_NORTH_EAST); }
                    if north_and_west { shape = Some(V_NORTH_WEST); }
                } else {
                    if north_and_west { shape = Some(V_NORTH_WEST); }
                    if north_and_east { shape = Some(V_NORTH_EAST); }
                    if south_and_west { shape = Some(V_SOUTH_WEST); }
                    if south_and_east { shape = Some(V_SOUTH_EAST); }
                }
            }
        }
        if shape == Some(V_NORTH_SOUTH) {
            if is_rail(w.get(north.above())) { shape = Some(V_ASCENDING_NORTH); }
            if is_rail(w.get(south.above())) { shape = Some(V_ASCENDING_SOUTH); }
        }
        if shape == Some(V_EAST_WEST) {
            if is_rail(w.get(east.above())) { shape = Some(V_ASCENDING_EAST); }
            if is_rail(w.get(west.above())) { shape = Some(V_ASCENDING_WEST); }
        }
        let shape = shape.unwrap_or(default_shape);
        self.update_connections(shape);
        self.state = try_with(self.state, P_SHAPE, shape);
        if first || w.get(self.pos) != self.state {
            w.set_block(self.pos, self.state, 3);
            for i in 0..self.connections.len() {
                if let Some(mut nb) = RailState::get_rail(w, self.connections[i]) {
                    nb.remove_soft_connections(w);
                    if nb.can_connect_to(self.pos) { nb.connect_to(w, self.pos); }
                }
            }
        }
    }
}

/// BaseRailBlock.updateDir
fn rail_update_dir(w: &mut World, pos: P, s: S, first: bool) -> S {
    let current = rail_shape(s);
    let mut r = RailState::new(pos, s);
    let sig = has_neighbor_signal(w, pos);
    r.place(w, sig, first, current);
    r.state
}
/// BaseRailBlock.updateState(state, level, pos, movedByPiston)
fn rail_update_state_placed(w: &mut World, s: S, pos: P, moved: bool) -> S {
    let st = rail_update_dir(w, pos, s, true);
    if rail_is_straight(s) { w.neighbor_changed_state(st, pos, block_of(s), moved); }
    st
}
/// BaseRailBlock.shouldBeRemoved
fn rail_should_be_removed(w: &World, pos: P, shape: u8) -> bool {
    if !w.can_support_rigid(pos.below()) { return true; }
    match shape {
        V_ASCENDING_EAST => !w.can_support_rigid(pos.rel(EAST)),
        V_ASCENDING_WEST => !w.can_support_rigid(pos.rel(WEST)),
        V_ASCENDING_NORTH => !w.can_support_rigid(pos.rel(NORTH)),
        V_ASCENDING_SOUTH => !w.can_support_rigid(pos.rel(SOUTH)),
        _ => false,
    }
}
/// PoweredRailBlock.findPoweredRailSignal
fn powered_rail_find_signal(w: &World, pos: P, s: S, forward: bool, depth: i32) -> bool {
    if depth >= 8 { return false; }
    let (mut x, mut y, mut z) = (pos.x, pos.y, pos.z);
    let mut check_below = true;
    let mut shape = rail_shape(s);
    match shape {
        V_NORTH_SOUTH => if forward { z += 1 } else { z -= 1 },
        V_EAST_WEST => if forward { x -= 1 } else { x += 1 },
        V_ASCENDING_EAST => {
            if forward { x -= 1 } else { x += 1; y += 1; check_below = false; }
            shape = V_EAST_WEST;
        }
        V_ASCENDING_WEST => {
            if forward { x -= 1; y += 1; check_below = false; } else { x += 1 }
            shape = V_EAST_WEST;
        }
        V_ASCENDING_NORTH => {
            if forward { z += 1 } else { z -= 1; y += 1; check_below = false; }
            shape = V_NORTH_SOUTH;
        }
        V_ASCENDING_SOUTH => {
            if forward { z += 1; y += 1; check_below = false; } else { z -= 1 }
            shape = V_NORTH_SOUTH;
        }
        _ => {}
    }
    let b = block_of(s);
    if powered_rail_same_with_power(w, p(x, y, z), b, forward, depth, shape) { true } else { check_below && powered_rail_same_with_power(w, p(x, y - 1, z), b, forward, depth, shape) }
}
/// PoweredRailBlock.isSameRailWithPower
fn powered_rail_same_with_power(w: &World, pos: P, block: u16, forward: bool, depth: i32, dir: u8) -> bool {
    let s = w.get(pos);
    if block_of(s) != block { return false; }
    let my = rail_shape(s);
    if dir == V_EAST_WEST && matches!(my, V_NORTH_SOUTH | V_ASCENDING_NORTH | V_ASCENDING_SOUTH) { return false; }
    if dir == V_NORTH_SOUTH && matches!(my, V_EAST_WEST | V_ASCENDING_EAST | V_ASCENDING_WEST) { return false; }
    if !powered(s) { return false; }
    if has_neighbor_signal(w, pos) { true } else { powered_rail_find_signal(w, pos, s, forward, depth + 1) }
}
/// DetectorRailBlock.checkPressed: there are no minecarts, so a detector rail is never (or no longer) pressed
fn detector_check_pressed(w: &mut World, pos: P, s: S) {
    if can_survive(w, s, pos) {
        let b = block_of(s);
        if powered(s) {
            let ns = with_bool(s, P_POWERED, false);
            w.set_block(pos, ns, 3);
            // updatePowerToConnected
            for cp in RailState::new(pos, ns).connections {
                let cs = w.get(cp);
                w.neighbor_changed_state(cs, cp, block_of(cs), false);
            }
            w.update_neighbors_at(pos, b);
            w.update_neighbors_at(pos.below(), b);
        }
        update_neighbour_for_output_signal(w, pos, b);
    }
}

// ------------------------------------------------------------------------------------------------ pistons
fn moving_piston_block() -> u16 { blk!("moving_piston") }
fn is_piston_base(s: S) -> bool { kind_of(s) == K::PistonBaseBlock }
fn piston_is_sticky(s: S) -> bool { is_blk!(s, "sticky_piston") }

/// Level.getBlockEntity(pos) as a PistonMovingBlockEntity
fn moving_at(w: &World, pos: P) -> Option<MovingBlock> {
    if !is_blk!(w.get(pos), "moving_piston") { return None; }
    side(|x| {
        x.tickers.iter().rev().find_map(|e| match &e.t {
            Ticker::Moving(m) if !e.removed && m.pos == pos => Some(m.clone()),
            _ => None,
        })
    })
}
/// The block in transit at a position, for the integrator (collision shapes, rendering of the client view).
pub fn moving_block_at(w: &World, pos: P) -> Option<MovingBlock> { moving_at(w, pos) }
/// Number of block entities still to be ticked (moving blocks and daylight detectors): the world is not idle while
/// a moving block is on its way.
pub fn moving_blocks_pending() -> usize { side(|x| x.tickers.iter().filter(|e| !e.removed && matches!(e.t, Ticker::Moving(_))).count()) }

/// Level.removeBlockEntity
fn remove_moving(pos: P) {
    side(|x| {
        for e in x.tickers.iter_mut() {
            if let Ticker::Moving(m) = &e.t {
                if m.pos == pos { e.removed = true; }
            }
        }
    });
}
fn remove_ticker(id: u64) {
    side(|x| {
        for e in x.tickers.iter_mut() {
            if e.id == id { e.removed = true; }
        }
    });
}
/// level.setBlock(pos, movingPistonState, flags) followed by level.setBlockEntity(newMovingBlockEntity(...))
fn place_moving(w: &mut World, pos: P, state: S, flags: u32, moved: S, dir: Dir, extending: bool, source: bool) {
    if !is_blk!(w.get(pos), "moving_piston") { remove_moving(pos); }
    if !w.inside(pos) {
        // the block leaves the simulated box and is lost
        w.unmodelled += 1;
    }
    w.set_block(pos, state, flags);
    if !is_blk!(w.get(pos), "moving_piston") { return; }
    remove_moving(pos);
    side(|x| push_ticker(x, Ticker::Moving(MovingBlock { pos, moved, dir, extending, source, progress: 0.0, progress_o: 0.0, last_ticked: 0 })));
}

/// PistonMath.getMovementArea
fn movement_area(a: &Aabb, dir: Dir, amount: f64) -> Aabb {
    let delta = amount * if is_positive(dir) { 1.0 } else { -1.0 };
    let (mn, mx) = (delta.min(0.0), delta.max(0.0));
    let mut r = *a;
    match dir {
        WEST => { r.max[0] = a.min[0] + mx; r.min[0] = a.min[0] + mn; }
        EAST => { r.min[0] = a.max[0] + mn; r.max[0] = a.max[0] + mx; }
        DOWN => { r.max[1] = a.min[1] + mx; r.min[1] = a.min[1] + mn; }
        UP => { r.min[1] = a.max[1] + mn; r.max[1] = a.max[1] + mx; }
        NORTH => { r.max[2] = a.min[2] + mx; r.min[2] = a.min[2] + mn; }
        _ => { r.min[2] = a.max[2] + mn; r.max[2] = a.max[2] + mx; }
    }
    r
}

/// The entity part of PistonMovingBlockEntity.moveCollidedEntities: would the moving block shove the player this
/// tick? (The shove itself is not modelled.)
fn moving_hits_player(w: &World, m: &MovingBlock, new_progress: f32) -> bool {
    if !w.entities.iter().any(|e| e.is_player) { return false; }
    let movement = if m.extending { m.dir } else { opposite(m.dir) };
    let delta = (new_progress - m.progress) as f64;
    // getCollisionRelatedBlockState
    let cs = if !m.extending && m.source && is_piston_base(m.moved) {
        let mut h = default_state(blk!("piston_head"));
        h = with_bool(h, P_SHORT, m.progress > 0.25);
        h = with(h, P_TYPE, if piston_is_sticky(m.moved) { V_STICKY } else { V_NORMAL });
        with(h, P_FACING, dir_val(facing(m.moved)))
    } else {
        m.moved
    };
    let tb = t();
    let shape = &tb.shapes[tb.col_shape[cs as usize] as usize];
    if shape.empty { return false; }
    let cur = (if m.extending { m.progress - 1.0 } else { 1.0 - m.progress }) as f64;
    let st = STEP[m.dir as usize];
    let off = [m.pos.x as f64 + cur * st.0 as f64, m.pos.y as f64 + cur * st.1 as f64, m.pos.z as f64 + cur * st.2 as f64];
    for b in &shape.boxes {
        let bx = Aabb::new(b[0] + off[0], b[1] + off[1], b[2] + off[2], b[3] + off[0], b[4] + off[1], b[5] + off[2]);
        let area = movement_area(&bx, movement, delta);
        if w.entities.iter().any(|e| e.is_player && area.intersects(&e.bb)) { return true; }
    }
    false
}

/// PistonMovingBlockEntity.tick
fn moving_tick(w: &mut World, id: u64, m: MovingBlock, push: &mut dyn FnMut(&mut World, &MovingBlock, f32)) {
    let now = w.game_time;
    if m.progress >= 1.0 {
        // level.removeBlockEntity(pos); entity.setRemoved()
        remove_ticker(id);
        if is_blk!(w.get(m.pos), "moving_piston") {
            let ns = update_from_neighbour_shapes(w, m.moved, m.pos);
            if is_air(ns) {
                w.set_block(m.pos, m.moved, 340);
                w.update_or_destroy(m.moved, ns, m.pos, 3, LIMIT);
            } else {
                // (never waterlogged here)
                w.set_block(m.pos, ns, 67);
                w.neighbor_changed(m.pos, block_of(ns));
            }
        }
    } else {
        let np = m.progress + 0.5;
        // moveCollidedEntities / moveStuckEntities: the caller moves the player
        push(w, &m, np);
        side(|x| {
            for e in x.tickers.iter_mut() {
                if e.id == id {
                    if let Ticker::Moving(mm) = &mut e.t {
                        mm.last_ticked = now;
                        mm.progress_o = mm.progress;
                        mm.progress = if np >= 1.0 { 1.0 } else { np };
                    }
                }
            }
        });
    }
}

/// PistonMovingBlockEntity.finalTick
fn moving_final_tick(w: &mut World, m: &MovingBlock) {
    if m.progress_o < 1.0 {
        remove_moving(m.pos);
        if is_blk!(w.get(m.pos), "moving_piston") {
            let ns = if m.source { 0 } else { update_from_neighbour_shapes(w, m.moved, m.pos) };
            w.set_block(m.pos, ns, 3);
            w.neighbor_changed(m.pos, block_of(ns));
        }
    }
}

/// Ticks the block entities of this file (the place of Level.tickBlockEntities in the server tick: after the
/// scheduled ticks, the block events and the entities). Entities made during this pass tick from the next one.
pub fn tick_block_entities(w: &mut World) { tick_block_entities_with(w, &mut |_, _, _| {}); }

/// The same, with a callback for the entity pushing of a moving block (called with the block and its new progress,
/// before the progress is applied, where PistonMovingBlockEntity.tick calls moveCollidedEntities).
pub fn tick_block_entities_with(w: &mut World, push: &mut dyn FnMut(&mut World, &MovingBlock, f32)) {
    let ids: Vec<u64> = side(|x| { x.ticking = true; x.pending = false; x.tickers.iter().filter(|e| !e.removed).map(|e| e.id).collect() });
    for id in ids {
        let Some(tk) = side(|x| x.tickers.iter().find(|e| e.id == id && !e.removed).map(|e| e.t.clone())) else { continue };
        match tk {
            Ticker::Moving(m) => {
                if !is_blk!(w.get(m.pos), "moving_piston") {
                    remove_ticker(id);
                    continue;
                }
                moving_tick(w, id, m, push);
            }
            Ticker::Daylight(pos) => {
                let s = w.get(pos);
                if kind_of(s) != K::DaylightDetectorBlock {
                    remove_ticker(id);
                    continue;
                }
                // DaylightDetectorBlock.tickEntity
                if w.game_time % 20 == 0 { daylight_update_signal_strength(w, s, pos); }
            }
            Ticker::Sculk(pos) => {
                if kind_of(w.get(pos)) != K::SculkSensorBlock {
                    remove_ticker(id);
                    continue;
                }
                super::vibrations::tick(w, pos);
            }
        }
    }
    side(|x| { x.ticking = false; x.tickers.retain(|e| !e.removed) });
}

/// PistonBaseBlock.isPushable
pub fn is_pushable(w: &World, s: S, pos: P, dir: Dir, allow_destroyable: bool, connection_dir: Dir) -> bool {
    if pos.y < MIN_Y || pos.y > w.max_y { return false; }
    if is_air(s) { return true; }
    if is_blk!(s, "obsidian") || is_blk!(s, "crying_obsidian") || is_blk!(s, "respawn_anchor") || is_blk!(s, "reinforced_deepslate") { return false; }
    if dir == DOWN && pos.y == MIN_Y { return false; }
    if dir == UP && pos.y == w.max_y { return false; }
    if !is_piston_base(s) {
        if t().destroy_time[s as usize] == -1.0 { return false; }
        match push_reaction(s) {
            PUSH_BLOCK => return false,
            PUSH_DESTROY => return allow_destroyable,
            PUSH_ONLY => return dir == connection_dir,
            _ => {}
        }
    } else if is_true(s, P_EXTENDED) {
        return false;
    }
    !has_block_entity(s)
}

/// PistonStructureResolver
struct Resolver {
    piston_pos: P,
    extending: bool,
    start: P,
    push_dir: Dir,
    piston_dir: Dir,
    to_push: Vec<P>,
    to_destroy: Vec<P>,
}

fn is_sticky_block(s: S) -> bool { is_blk!(s, "slime_block") || is_blk!(s, "honey_block") }
fn can_stick_to_each_other(a: S, b: S) -> bool {
    if is_blk!(a, "honey_block") && is_blk!(b, "slime_block") {
        false
    } else if is_blk!(a, "slime_block") && is_blk!(b, "honey_block") {
        false
    } else {
        is_sticky_block(a) || is_sticky_block(b)
    }
}

impl Resolver {
    fn new(piston_pos: P, dir: Dir, extending: bool) -> Resolver {
        let (push_dir, start) = if extending { (dir, piston_pos.rel(dir)) } else { (opposite(dir), piston_pos.rel_n(dir, 2)) };
        Resolver { piston_pos, extending, start, push_dir, piston_dir: dir, to_push: Vec::new(), to_destroy: Vec::new() }
    }
    fn resolve(&mut self, w: &World) -> bool {
        self.to_push.clear();
        self.to_destroy.clear();
        let next = w.get(self.start);
        if !is_pushable(w, next, self.start, self.push_dir, false, self.piston_dir) {
            if self.extending && push_reaction(next) == PUSH_DESTROY {
                self.to_destroy.push(self.start);
                true
            } else {
                false
            }
        } else {
            if !self.add_block_line(w, self.start, self.push_dir) { return false; }
            let mut i = 0;
            while i < self.to_push.len() {
                let q = self.to_push[i];
                if is_sticky_block(w.get(q)) && !self.add_branching_blocks(w, q) { return false; }
                i += 1;
            }
            true
        }
    }
    fn add_block_line(&mut self, w: &World, start: P, direction: Dir) -> bool {
        let mut next = w.get(start);
        if is_air(next) { return true; }
        if !is_pushable(w, next, start, self.push_dir, false, direction) { return true; }
        if start == self.piston_pos { return true; }
        if self.to_push.contains(&start) { return true; }
        let mut block_count: i32 = 1;
        if block_count as usize + self.to_push.len() > 12 { return false; }
        let back = opposite(self.push_dir);
        while is_sticky_block(next) {
            let q = start.rel_n(back, block_count);
            let prev = next;
            next = w.get(q);
            if is_air(next) || !can_stick_to_each_other(prev, next) || !is_pushable(w, next, q, self.push_dir, false, back) || q == self.piston_pos { break; }
            block_count += 1;
            if block_count as usize + self.to_push.len() > 12 { return false; }
        }
        let mut blocks_added: usize = 0;
        for i in (0..block_count).rev() {
            self.to_push.push(start.rel_n(back, i));
            blocks_added += 1;
        }
        let mut i = 1;
        loop {
            let q = start.rel_n(self.push_dir, i);
            if let Some(cp) = self.to_push.iter().position(|x| *x == q) {
                self.reorder_list_at_collision(blocks_added, cp);
                let mut j = 0;
                while j <= cp + blocks_added {
                    let bp = self.to_push[j];
                    if is_sticky_block(w.get(bp)) && !self.add_branching_blocks(w, bp) { return false; }
                    j += 1;
                }
                return true;
            }
            next = w.get(q);
            if is_air(next) { return true; }
            if !is_pushable(w, next, q, self.push_dir, true, self.push_dir) || q == self.piston_pos { return false; }
            if push_reaction(next) == PUSH_DESTROY {
                self.to_destroy.push(q);
                return true;
            }
            if self.to_push.len() >= 12 { return false; }
            self.to_push.push(q);
            blocks_added += 1;
            i += 1;
        }
    }
    fn reorder_list_at_collision(&mut self, blocks_added: usize, collision_pos: usize) {
        let n = self.to_push.len();
        let head: Vec<P> = self.to_push[0..collision_pos].to_vec();
        let last_line: Vec<P> = self.to_push[n - blocks_added..n].to_vec();
        let collision_to_line: Vec<P> = self.to_push[collision_pos..n - blocks_added].to_vec();
        self.to_push.clear();
        self.to_push.extend(head);
        self.to_push.extend(last_line);
        self.to_push.extend(collision_to_line);
    }
    fn add_branching_blocks(&mut self, w: &World, from: P) -> bool {
        let from_state = w.get(from);
        for d in DIRS {
            if axis(d) != axis(self.push_dir) {
                let np = from.rel(d);
                let ns = w.get(np);
                if can_stick_to_each_other(ns, from_state) && !self.add_block_line(w, np, d) { return false; }
            }
        }
        true
    }
}

/// PistonBaseBlock.getNeighborSignal (with the quasi-connectivity look at the block above)
fn piston_neighbor_signal(w: &World, pos: P, push_dir: Dir) -> bool {
    for d in DIRS {
        if d != push_dir && has_signal(w, pos.rel(d), d) { return true; }
    }
    if has_signal(w, pos, DOWN) { return true; }
    let above = pos.above();
    for d in DIRS {
        if d != DOWN && has_signal(w, above.rel(d), d) { return true; }
    }
    false
}

/// PistonBaseBlock.checkIfExtend
fn piston_check_if_extend(w: &mut World, pos: P, s: S) {
    let dir = facing(s);
    let extend = piston_neighbor_signal(w, pos, dir);
    let extended = is_true(s, P_EXTENDED);
    if extend && !extended {
        if Resolver::new(pos, dir, true).resolve(w) {
            w.block_event(pos, block_of(s), 0, dir);
        }
    } else if !extend && extended {
        let pp = pos.rel_n(dir, 2);
        let ps = w.get(pp);
        let mut event = 1;
        if is_blk!(ps, "moving_piston") && facing(ps) == dir {
            if let Some(m) = moving_at(w, pp) {
                if m.extending && (m.progress_o < 0.5 || w.game_time == m.last_ticked || is_handling_tick()) { event = 2; }
            }
        }
        w.block_event(pos, block_of(s), event, dir);
    }
}

/// PistonBaseBlock.triggerEvent (server side)
fn piston_trigger_event(w: &mut World, s: S, pos: P, b0: u8, b1: u8) -> bool {
    let dir = facing(s);
    let extended_state = with_bool(s, P_EXTENDED, true);
    let sticky = piston_is_sticky(s);
    let extend = piston_neighbor_signal(w, pos, dir);
    if extend && (b0 == 1 || b0 == 2) {
        w.set_block(pos, extended_state, 2);
        return false;
    }
    if !extend && b0 == 0 { return false; }
    if b0 == 0 {
        if !piston_move_blocks(w, pos, dir, true, sticky) { return false; }
        w.set_block(pos, extended_state, 67);
        vib::emit_at(w, vib::Ev::BlockActivate, pos, vib::Ctx::state(extended_state));
    } else if b0 == 1 || b0 == 2 {
        if let Some(m) = moving_at(w, pos.rel(dir)) { moving_final_tick(w, &m); }
        let mp = with(with(default_state(moving_piston_block()), P_FACING, dir_val(dir)), P_TYPE, if sticky { V_STICKY } else { V_NORMAL });
        let base = with_bool(with(default_state(block_of(s)), P_FACING, dir_val((b1 & 7) % 6)), P_EXTENDED, false);
        place_moving(w, pos, mp, 276, base, dir, false, true);
        w.update_neighbors_at(pos, moving_piston_block());
        update_neighbour_shapes(w, mp, pos, 2);
        if sticky {
            let two = pos.rel_n(dir, 2);
            let ms = w.get(two);
            let mut piston_piece = false;
            if is_blk!(ms, "moving_piston") {
                if let Some(e) = moving_at(w, two) {
                    if e.dir == dir && e.extending {
                        moving_final_tick(w, &e);
                        piston_piece = true;
                    }
                }
            }
            if !piston_piece {
                if b0 != 1 || is_air(ms) || !is_pushable(w, ms, two, opposite(dir), false, dir) || (push_reaction(ms) != PUSH_NORMAL && !is_piston_base(ms)) {
                    w.remove_block(pos.rel(dir), false);
                } else {
                    piston_move_blocks(w, pos, dir, false, sticky);
                }
            }
        } else {
            w.remove_block(pos.rel(dir), false);
        }
        vib::emit_at(w, vib::Ev::BlockDeactivate, pos, vib::Ctx::state(mp));
    }
    true
}

/// PistonBaseBlock.moveBlocks
fn piston_move_blocks(w: &mut World, piston_pos: P, dir: Dir, extending: bool, sticky: bool) -> bool {
    let arm = piston_pos.rel(dir);
    if !extending && is_blk!(w.get(arm), "piston_head") {
        w.set_block(arm, 0, 276);
    }
    let mut resolver = Resolver::new(piston_pos, dir, extending);
    if !resolver.resolve(w) { return false; }
    // Map<BlockPos, BlockState> deleteAfterMove = Maps.newHashMap(): kept in insertion order, read in hash order
    let mut delete_after_move: Vec<(P, S)> = Vec::new();
    let to_push = resolver.to_push.clone();
    let mut to_push_shapes: Vec<S> = Vec::new();
    for q in &to_push {
        let st = w.get(*q);
        to_push_shapes.push(st);
        delete_after_move.push((*q, st));
    }
    let to_destroy = resolver.to_destroy.clone();
    let mut to_update: Vec<S> = Vec::with_capacity(to_push.len() + to_destroy.len());
    let push_dir = if extending { dir } else { opposite(dir) };
    for i in (0..to_destroy.len()).rev() {
        let q = to_destroy[i];
        let st = w.get(q);
        // (dropResources: nothing drops in this world)
        w.set_block(q, 0, 18);
        vib::emit_at(w, vib::Ev::BlockDestroy, q, vib::Ctx::state(st));
        to_update.push(st);
    }
    for i in (0..to_push.len()).rev() {
        let st = w.get(to_push[i]);
        let q = to_push[i].rel(push_dir);
        delete_after_move.retain(|(x, _)| *x != q);
        let actual = with(default_state(moving_piston_block()), P_FACING, dir_val(dir));
        place_moving(w, q, actual, 324, to_push_shapes[i], dir, extending, false);
        to_update.push(st);
    }
    if extending {
        let ty = if sticky { V_STICKY } else { V_NORMAL };
        let head = with(with(default_state(blk!("piston_head")), P_FACING, dir_val(dir)), P_TYPE, ty);
        let mp = with(with(default_state(moving_piston_block()), P_FACING, dir_val(dir)), P_TYPE, ty);
        delete_after_move.retain(|(x, _)| *x != arm);
        place_moving(w, arm, mp, 324, head, dir, true, true);
    }
    let order = java_hash_order(&delete_after_move);
    for (q, _) in &order {
        w.set_block(*q, 0, 82);
    }
    for (q, old) in &order {
        update_indirect_neighbour_shapes(w, *old, *q, 2, LIMIT);
        update_neighbour_shapes(w, 0, *q, 2);
        // (air.updateIndirectNeighbourShapes: nothing)
    }
    let mut ui = 0;
    for i in (0..to_destroy.len()).rev() {
        let st = to_update[ui];
        ui += 1;
        let q = to_destroy[i];
        super::affect_neighbors_after_removal(w, st, q, false);
        update_indirect_neighbour_shapes(w, st, q, 2, LIMIT);
        w.update_neighbors_at(q, block_of(st));
    }
    for i in (0..to_push.len()).rev() {
        w.update_neighbors_at(to_push[i], block_of(to_update[ui]));
        ui += 1;
    }
    if extending {
        w.update_neighbors_at(arm, blk!("piston_head"));
    }
    true
}

/// PistonHeadBlock.isFittingBase
fn head_is_fitting_base(arm: S, base: S) -> bool {
    let want = if get(arm, P_TYPE) == Some(V_STICKY) { blk!("sticky_piston") } else { blk!("piston") };
    block_of(base) == want && is_true(base, P_EXTENDED) && facing(base) == facing(arm)
}
/// PistonHeadBlock.canSurvive
fn head_can_survive(w: &World, s: S, pos: P) -> bool {
    let base = w.get(pos.rel(opposite(facing(s))));
    head_is_fitting_base(s, base) || (is_blk!(base, "moving_piston") && facing(base) == facing(s))
}

// ------------------------------------------------------------------------------------------------ the hooks
pub fn update_shape(w: &mut World, s: S, pos: P, dir: Dir, npos: P, ns: S) -> Option<S> {
    let k = kind_of(s);
    if !owned(k) { return None; }
    Some(match k {
        K::RedStoneWireBlock => wire_update_shape(w, s, pos, dir, ns),
        K::RepeaterBlock => {
            if dir == DOWN && !super::face_sturdy(ns, UP, 2) {
                0
            } else if axis(dir) != axis(facing(s)) {
                with_bool(s, P_LOCKED, diode_locked(w, pos, s))
            } else {
                s
            }
        }
        K::ComparatorBlock => if dir == DOWN && !super::face_sturdy(ns, UP, 2) { 0 } else { s },
        K::RedstoneTorchBlock => if dir == DOWN && !can_survive(w, s, pos) { 0 } else { s },
        K::RedstoneWallTorchBlock | K::PistonHeadBlock | K::TripWireHookBlock => if opposite(dir) == facing(s) && !can_survive(w, s, pos) { 0 } else { s },
        K::ObserverBlock => {
            if facing(s) == dir && !powered(s) {
                // startSignal
                let b = block_of(s);
                if !w.has_scheduled_tick(pos, b) { sched(w, pos, b, 2, PRIO_NORMAL); }
            }
            s
        }
        K::NoteBlock => if axis(dir) == AXIS_Y { note_set_instrument(w, pos, s) } else { s },
        K::LeverBlock | K::ButtonBlock => if opposite(connected_direction(s)) == dir && !can_survive(w, s, pos) { 0 } else { s },
        K::PressurePlateBlock | K::WeightedPressurePlateBlock => if dir == DOWN && !can_survive(w, s, pos) { 0 } else { s },
        K::TripWireBlock => if is_horizontal(dir) { with_bool(s, dir_prop(dir), tripwire_should_connect_to(ns, dir)) } else { s },
        K::BellBlock => bell_update_shape(w, s, pos, dir, npos, ns),
        // (rails, lightning rods and sculk sensors only look after their water: there is none)
        _ => s,
    })
}

pub fn neighbor_changed(w: &mut World, s: S, pos: P, from_block: u16, moved: bool) -> bool {
    let k = kind_of(s);
    let b = block_of(s);
    match k {
        // the power reaction of the three openable blocks (their other hooks are in basic.rs)
        K::DoorBlock => {
            let other = if get(s, P_HALF) == Some(V_LOWER) { UP } else { DOWN };
            let signal = has_neighbor_signal(w, pos) || has_neighbor_signal(w, pos.rel(other));
            if b != from_block && signal != powered(s) {
                if signal != is_true(s, P_OPEN) { vib::emit_at(w, if signal { vib::Ev::BlockOpen } else { vib::Ev::BlockClose }, pos, vib::Ctx::NONE); }
                w.set_block(pos, with_bool(with_bool(s, P_POWERED, signal), P_OPEN, signal), 2);
            }
            return true;
        }
        K::TrapDoorBlock => {
            let signal = has_neighbor_signal(w, pos);
            if signal != powered(s) {
                let mut st = s;
                if is_true(st, P_OPEN) != signal {
                    st = with_bool(st, P_OPEN, signal);
                    vib::emit_at(w, if signal { vib::Ev::BlockOpen } else { vib::Ev::BlockClose }, pos, vib::Ctx::NONE);
                }
                w.set_block(pos, with_bool(st, P_POWERED, signal), 2);
            }
            return true;
        }
        K::FenceGateBlock => {
            let has_power = has_neighbor_signal(w, pos);
            if powered(s) != has_power {
                w.set_block(pos, with_bool(with_bool(s, P_POWERED, has_power), P_OPEN, has_power), 2);
                if is_true(s, P_OPEN) != has_power { vib::emit_at(w, if has_power { vib::Ev::BlockOpen } else { vib::Ev::BlockClose }, pos, vib::Ctx::NONE); }
            }
            return true;
        }
        _ => {}
    }
    if !owned(k) { return false; }
    match k {
        K::RedStoneWireBlock => {
            if can_survive(w, s, pos) {
                wire_update_power_strength(w, pos, s);
            } else {
                w.remove_block(pos, false);
            }
        }
        K::RepeaterBlock | K::ComparatorBlock => {
            if block_of(w.get(pos)) == b {
                if can_survive(w, s, pos) {
                    diode_check_tick_on_neighbor(w, pos, s);
                } else {
                    w.remove_block(pos, false);
                    for d in DIRS { w.update_neighbors_at(pos.rel(d), b); }
                }
            }
        }
        K::RedstoneTorchBlock | K::RedstoneWallTorchBlock => {
            if is_true(s, P_LIT) == torch_has_neighbor_signal(w, pos, s) && !will_tick_this_tick(w, pos, b) {
                sched(w, pos, b, 2, PRIO_NORMAL);
            }
        }
        K::RedstoneLampBlock => {
            let lit = is_true(s, P_LIT);
            if lit != has_neighbor_signal(w, pos) {
                if lit {
                    sched(w, pos, b, 4, PRIO_NORMAL);
                } else {
                    w.set_block(pos, cycle(s, P_LIT), 2);
                }
            }
        }
        K::PistonBaseBlock => piston_check_if_extend(w, pos, s),
        K::PistonHeadBlock => {
            if head_can_survive(w, s, pos) {
                w.neighbor_changed(pos.rel(opposite(facing(s))), from_block);
            }
        }
        K::DispenserBlock => {
            let should = has_neighbor_signal(w, pos) || has_neighbor_signal(w, pos.above());
            let trig = is_true(s, P_TRIGGERED);
            if should && !trig {
                sched(w, pos, b, 4, PRIO_NORMAL);
                w.set_block(pos, with_bool(s, P_TRIGGERED, true), 2);
            } else if !should && trig {
                w.set_block(pos, with_bool(s, P_TRIGGERED, false), 2);
            }
        }
        K::CrafterBlock => {
            let should = has_neighbor_signal(w, pos);
            let trig = is_true(s, P_TRIGGERED);
            if should && !trig {
                sched(w, pos, b, 4, PRIO_NORMAL);
                w.set_block(pos, with_bool(s, P_TRIGGERED, true), 2);
            } else if !should && trig {
                w.set_block(pos, with_bool(with_bool(s, P_TRIGGERED, false), P_CRAFTING, false), 2);
            }
        }
        K::HopperBlock => hopper_check_powered_state(w, pos, s),
        K::NoteBlock => {
            let signal = has_neighbor_signal(w, pos);
            if signal != powered(s) {
                if signal { note_play(w, s, pos); }
                w.set_block(pos, with_bool(s, P_POWERED, signal), 3);
            }
        }
        K::BellBlock => {
            let signal = has_neighbor_signal(w, pos);
            if signal != powered(s) {
                if signal { bell_ring(w, pos, None); }
                w.set_block(pos, with_bool(s, P_POWERED, signal), 3);
            }
        }
        K::TntBlock => {
            if has_neighbor_signal(w, pos) { tnt_prime(w, pos); }
        }
        K::CopperBulbBlock => bulb_check_and_flip(w, s, pos),
        K::RailBlock | K::PoweredRailBlock | K::DetectorRailBlock => {
            if block_of(w.get(pos)) == b {
                let shape = rail_shape(s);
                if rail_should_be_removed(w, pos, shape) {
                    w.remove_block(pos, moved);
                } else if k == K::RailBlock {
                    // RailBlock.updateState(state, level, pos, block)
                    if is_signal_source(default_state(from_block)) && RailState::new(pos, s).count_potential_connections(w) == 3 {
                        rail_update_dir(w, pos, s, false);
                    }
                } else if k == K::PoweredRailBlock {
                    // PoweredRailBlock.updateState(state, level, pos, block)
                    let is_powered = powered(s);
                    let should = has_neighbor_signal(w, pos) || powered_rail_find_signal(w, pos, s, true, 0) || powered_rail_find_signal(w, pos, s, false, 0);
                    if should != is_powered {
                        w.set_block(pos, with_bool(s, P_POWERED, should), 3);
                        w.update_neighbors_at(pos.below(), b);
                        if rail_is_slope(shape) { w.update_neighbors_at(pos.above(), b); }
                    }
                }
            }
        }
        // CommandBlock: its block entity would note the power and schedule a run; command blocks never run here.
        // The rest of the owned classes does not react to neighbour changes.
        _ => {}
    }
    true
}

pub fn on_place(w: &mut World, s: S, pos: P, old: S, moved: bool) -> bool {
    let k = kind_of(s);
    if !owned(k) { return false; }
    let b = block_of(s);
    let new_block = !same_block(old, s);
    match k {
        K::RedStoneWireBlock => {
            if new_block {
                wire_update_power_strength(w, pos, s);
                for d in [UP, DOWN] { w.update_neighbors_at(pos.rel(d), b); }
                wire_update_neighbors_of_neighboring_wires(w, pos);
            }
        }
        K::RepeaterBlock | K::ComparatorBlock => {
            if k == K::ComparatorBlock && new_block {
                // a fresh ComparatorBlockEntity
                side(|x| { x.comparator.insert(pos, 0); });
            }
            diode_update_neighbors_in_front(w, pos, s);
        }
        K::RedstoneTorchBlock | K::RedstoneWallTorchBlock => torch_notify_neighbors(w, pos, s),
        K::PistonBaseBlock => {
            if new_block && moving_at(w, pos).is_none() { piston_check_if_extend(w, pos, s); }
        }
        K::ObserverBlock => {
            if new_block && powered(s) && !w.has_scheduled_tick(pos, b) {
                let ns = with_bool(s, P_POWERED, false);
                w.set_block(pos, ns, 18);
                observer_update_neighbors_in_front(w, pos, ns);
            }
        }
        K::HopperBlock => if new_block { hopper_check_powered_state(w, pos, s) },
        K::TntBlock => {
            if new_block && has_neighbor_signal(w, pos) { tnt_prime(w, pos); }
        }
        K::RailBlock | K::PoweredRailBlock | K::DetectorRailBlock => {
            if new_block {
                let st = rail_update_state_placed(w, s, pos, moved);
                if k == K::DetectorRailBlock { detector_check_pressed(w, pos, st); }
            }
        }
        K::CopperBulbBlock => if new_block { bulb_check_and_flip(w, s, pos) },
        K::LightningRodBlock => {
            if new_block && powered(s) && !w.has_scheduled_tick(pos, b) { sched(w, pos, b, 8, PRIO_NORMAL); }
        }
        K::SculkSensorBlock | K::TargetBlock => {
            if new_block && power(s) > 0 && !w.has_scheduled_tick(pos, b) {
                w.set_block(pos, with(s, P_POWER, 0), 18);
            }
        }
        K::TripWireBlock => if new_block { tripwire_update_source(w, pos, s) },
        K::DaylightDetectorBlock => {
            if new_block {
                // a fresh DaylightDetectorBlockEntity with its ticker
                side(|x| {
                    let have = x.tickers.iter().any(|e| !e.removed && matches!(e.t, Ticker::Daylight(q) if q == pos));
                    if !have { push_ticker(x, Ticker::Daylight(pos)); }
                });
            }
        }
        _ => {}
    }
    true
}

pub fn affect_neighbors_after_removal(w: &mut World, old: S, pos: P, moved: bool) -> bool {
    let k = kind_of(old);
    if !owned(k) { return false; }
    let b = block_of(old);
    match k {
        K::RedStoneWireBlock => {
            if !moved {
                for d in DIRS { w.update_neighbors_at(pos.rel(d), b); }
                wire_update_power_strength(w, pos, old);
                wire_update_neighbors_of_neighboring_wires(w, pos);
            }
        }
        K::RepeaterBlock | K::ComparatorBlock => {
            if k == K::ComparatorBlock && !is_blk!(w.get(pos), "comparator") {
                side(|x| { x.comparator.remove(&pos); });
            }
            if !moved { diode_update_neighbors_in_front(w, pos, old); }
        }
        K::RedstoneTorchBlock | K::RedstoneWallTorchBlock => if !moved { torch_notify_neighbors(w, pos, old) },
        K::PistonHeadBlock => {
            let base_pos = pos.rel(opposite(facing(old)));
            if head_is_fitting_base(old, w.get(base_pos)) { w.destroy_block(base_pos, true); }
        }
        K::MovingPistonBlock => {
            // the block entity goes with its block
            if !is_blk!(w.get(pos), "moving_piston") { remove_moving(pos); }
        }
        K::ObserverBlock => {
            if powered(old) && w.has_scheduled_tick(pos, b) {
                observer_update_neighbors_in_front(w, pos, with_bool(old, P_POWERED, false));
            }
        }
        // Containers.updateNeighboursAfterDestroy
        K::DispenserBlock | K::HopperBlock | K::CrafterBlock | K::JukeboxBlock => update_neighbour_for_output_signal(w, pos, b),
        K::LeverBlock | K::ButtonBlock => if !moved && powered(old) { face_attached_update_neighbours(w, old, pos) },
        K::PressurePlateBlock | K::WeightedPressurePlateBlock => if !moved && plate_signal_for_state(old) > 0 { plate_update_neighbours(w, pos, b) },
        K::TripWireBlock => if !moved { tripwire_update_source(w, pos, with_bool(old, P_POWERED, true)) },
        K::TripWireHookBlock => if !moved { hook_on_removed(w, old, pos) },
        K::RailBlock | K::PoweredRailBlock | K::DetectorRailBlock => {
            if !moved {
                if rail_is_slope(rail_shape(old)) { w.update_neighbors_at(pos.above(), b); }
                if rail_is_straight(old) {
                    w.update_neighbors_at(pos, b);
                    w.update_neighbors_at(pos.below(), b);
                }
            }
        }
        K::LightningRodBlock => if powered(old) { rod_update_neighbours(w, old, pos) },
        K::SculkSensorBlock => if get(old, P_SCULK_SENSOR_PHASE) == Some(V_ACTIVE) { sculk_update_neighbours(w, pos, old) },
        _ => {}
    }
    true
}

pub fn tick(w: &mut World, s: S, pos: P) -> bool {
    let k = kind_of(s);
    if !owned(k) { return false; }
    let b = block_of(s);
    due_ran(pos, b, w.game_time);
    side(|x| x.handling += 1);
    match k {
        K::RepeaterBlock => repeater_tick(w, s, pos),
        K::ComparatorBlock => comparator_refresh_output_state(w, pos, s),
        K::RedstoneTorchBlock | K::RedstoneWallTorchBlock => torch_tick(w, s, pos),
        K::RedstoneLampBlock => {
            if is_true(s, P_LIT) && !has_neighbor_signal(w, pos) { w.set_block(pos, cycle(s, P_LIT), 2); }
        }
        K::ObserverBlock => {
            if powered(s) {
                w.set_block(pos, with_bool(s, P_POWERED, false), 2);
            } else {
                w.set_block(pos, with_bool(s, P_POWERED, true), 2);
                sched(w, pos, b, 2, PRIO_NORMAL);
            }
            observer_update_neighbors_in_front(w, pos, s);
        }
        // DispenserBlock.dispenseFrom / CrafterBlock.dispenseFrom with nothing inside: the "empty" click (the
        // dispenser's also a game event; DropperBlock.dispenseFrom has none)
        K::DispenserBlock => if !is_blk!(s, "dropper") { vib::emit_at(w, vib::Ev::BlockActivate, pos, vib::Ctx::state(s)) },
        K::CrafterBlock => {}
        K::ButtonBlock => {
            // checkPressed: no arrows, so a pressed button comes out
            if powered(s) {
                w.set_block(pos, with_bool(s, P_POWERED, false), 3);
                face_attached_update_neighbours(w, s, pos);
                vib::emit_at(w, vib::Ev::BlockDeactivate, pos, vib::Ctx::NONE);
            }
        }
        K::PressurePlateBlock | K::WeightedPressurePlateBlock => {
            let signal = plate_signal_for_state(s);
            if signal > 0 { plate_check_pressed(w, pos, s, signal); }
        }
        K::TripWireBlock => {
            let cur = w.get(pos);
            if is_blk!(cur, "tripwire") && powered(cur) {
                let pressed = match outline_bounds(cur, pos) { Some(bx) => entity_count(w, &bx, false) > 0, None => false };
                tripwire_check_pressed(w, pos, pressed);
            }
        }
        K::TripWireHookBlock => hook_calculate_state(w, pos, s, false, true, -1, None),
        K::TargetBlock => {
            if power(s) != 0 { w.set_block(pos, with(s, P_POWER, 0), 3); }
        }
        K::DetectorRailBlock => if powered(s) { detector_check_pressed(w, pos, s) },
        K::LightningRodBlock => {
            w.set_block(pos, with_bool(s, P_POWERED, false), 3);
            rod_update_neighbours(w, s, pos);
        }
        K::SculkSensorBlock => {
            let phase = get(s, P_SCULK_SENSOR_PHASE);
            if phase != Some(V_ACTIVE) {
                if phase == Some(V_COOLDOWN) { w.set_block(pos, with(s, P_SCULK_SENSOR_PHASE, V_INACTIVE), 3); }
            } else {
                // deactivate
                w.set_block(pos, with(with(s, P_SCULK_SENSOR_PHASE, V_COOLDOWN), P_POWER, 0), 3);
                sched(w, pos, b, 10, PRIO_NORMAL);
                sculk_update_neighbours(w, pos, s);
            }
        }
        _ => {}
    }
    side(|x| x.handling = x.handling.saturating_sub(1));
    true
}

pub fn trigger_event(w: &mut World, s: S, pos: P, b0: u8, b1: u8) -> Option<bool> {
    let k = kind_of(s);
    if !owned(k) { return None; }
    Some(match k {
        K::PistonBaseBlock => {
            side(|x| x.handling += 1);
            let r = piston_trigger_event(w, s, pos, b0, b1);
            side(|x| x.handling = x.handling.saturating_sub(1));
            r
        }
        // the note sounds (heard by the clients)
        K::NoteBlock => true,
        // BellBlockEntity.triggerEvent: the bell swings
        K::BellBlock => b0 == 1,
        _ => false,
    })
}

pub fn set_placed_by(w: &mut World, pos: P, s: S, ctx: &PlaceCtx) -> bool {
    let k = kind_of(s);
    if !owned(k) { return false; }
    let b = block_of(s);
    match k {
        K::PistonBaseBlock => piston_check_if_extend(w, pos, s),
        K::RepeaterBlock | K::ComparatorBlock => {
            if diode_should_turn_on(w, pos, s) { sched(w, pos, b, 1, PRIO_NORMAL); }
        }
        K::TripWireHookBlock => hook_calculate_state(w, pos, s, false, false, -1, None),
        K::CrafterBlock => if is_true(s, P_TRIGGERED) { sched(w, pos, b, 4, PRIO_NORMAL) },
        _ => {}
    }
    true
}

pub fn attack(w: &mut World, s: S, pos: P) -> bool {
    let k = kind_of(s);
    if !owned(k) { return false; }
    if k == K::NoteBlock { note_play(w, s, pos); }
    true
}

pub fn use_item_on(w: &mut World, s: S, pos: P, face: Dir, hit: [f64; 3], who: &mut Clicker) -> Option<Use> {
    let k = kind_of(s);
    if !owned(k) { return None; }
    let item = t().items.get(who.item as usize).map(|i| i.name.as_str()).unwrap_or("air");
    Some(match k {
        K::NoteBlock => {
            // ItemTags.NOTE_BLOCK_TOP_INSTRUMENTS on the top face: the head is placed instead
            let head = matches!(item, "zombie_head" | "skeleton_skull" | "creeper_head" | "dragon_head" | "wither_skeleton_skull" | "piglin_head" | "player_head");
            if head && face == UP { Use::Pass } else { Use::TryEmptyHand }
        }
        K::TntBlock => {
            if item == "flint_and_steel" || item == "fire_charge" {
                // prime(level, pos, player): the primed TNT and its explosion are not modelled
                w.unmodelled += 1;
                w.set_block(pos, 0, 11);
                if item == "fire_charge" { who.consumed = true; }
                Use::Consume
            } else {
                Use::TryEmptyHand
            }
        }
        K::JukeboxBlock => {
            if !is_true(s, P_HAS_RECORD) && item.starts_with("music_disc_") {
                // JukeboxPlayable.tryInsertIntoJukebox: discs are not modelled
                w.unmodelled += 1;
            }
            Use::TryEmptyHand
        }
        _ => Use::TryEmptyHand,
    })
}

pub fn use_without_item(w: &mut World, s: S, pos: P, face: Dir, hit: [f64; 3], who: &mut Clicker) -> Option<Use> {
    let k = kind_of(s);
    if !owned(k) { return None; }
    let b = block_of(s);
    Some(match k {
        K::RedStoneWireBlock => wire_use(w, s, pos),
        K::RepeaterBlock => {
            w.set_block(pos, cycle(s, P_DELAY), 3);
            Use::Consume
        }
        K::ComparatorBlock => {
            let ns = cycle(s, P_MODE);
            w.set_block(pos, ns, 2);
            if block_of(w.get(pos)) == b { comparator_refresh_output_state(w, pos, ns); }
            Use::Consume
        }
        K::LeverBlock => {
            // pull
            let ns = cycle(s, P_POWERED);
            w.set_block(pos, ns, 3);
            face_attached_update_neighbours(w, ns, pos);
            vib::emit_at(w, if powered(ns) { vib::Ev::BlockActivate } else { vib::Ev::BlockDeactivate }, pos, vib::Ctx::player(who.sneak));
            Use::Consume
        }
        K::ButtonBlock => {
            if !powered(s) {
                // press
                w.set_block(pos, with_bool(s, P_POWERED, true), 3);
                face_attached_update_neighbours(w, s, pos);
                sched(w, pos, b, button_ticks(s), PRIO_NORMAL);
                vib::emit_at(w, vib::Ev::BlockActivate, pos, vib::Ctx::player(who.sneak));
            }
            Use::Consume
        }
        K::NoteBlock => {
            let ns = cycle(s, P_NOTE);
            w.set_block(pos, ns, 3);
            note_play(w, ns, pos);
            Use::Consume
        }
        K::DaylightDetectorBlock => {
            let ns = cycle(s, P_INVERTED);
            w.set_block(pos, ns, 2);
            vib::emit_at(w, vib::Ev::BlockChange, pos, vib::Ctx::of(who.sneak, ns));
            daylight_update_signal_strength(w, ns, pos);
            Use::Consume
        }
        K::BellBlock => {
            // onHit / isProperHit
            let click_y = hit[1] - pos.y as f64;
            let proper = if axis(face) != AXIS_Y && !(click_y > 0.8124f32 as f64) {
                match get(s, P_ATTACHMENT) {
                    Some(V_FLOOR) => axis(facing(s)) == axis(face),
                    Some(V_SINGLE_WALL) | Some(V_DOUBLE_WALL) => axis(facing(s)) != axis(face),
                    Some(V_CEILING) => true,
                    _ => false,
                }
            } else {
                false
            };
            if proper {
                bell_ring(w, pos, Some(face));
                Use::Consume
            } else {
                Use::Pass
            }
        }
        K::JukeboxBlock => {
            if is_true(s, P_HAS_RECORD) {
                // popOutTheItem: discs are not modelled
                w.unmodelled += 1;
                Use::Consume
            } else {
                Use::Pass
            }
        }
        K::DispenserBlock | K::HopperBlock | K::CrafterBlock => {
            w.opened_menu = Some((pos, 0));
            Use::Consume
        }
        K::MovingPistonBlock => {
            if moving_at(w, pos).is_none() {
                w.remove_block(pos, false);
                Use::Consume
            } else {
                Use::Pass
            }
        }
        // CommandBlock: only for players that may use game master blocks
        _ => Use::Pass,
    })
}

pub fn step_on(w: &mut World, s: S, pos: P) -> bool {
    let k = kind_of(s);
    if !owned(k) { return false; }
    // (SculkSensorBlock.stepOn: vibrations::step_on, called by the player who knows where it stands)
    true
}

pub fn entity_inside(w: &mut World, s: S, pos: P) -> bool {
    let k = kind_of(s);
    if !owned(k) { return false; }
    match k {
        K::PressurePlateBlock | K::WeightedPressurePlateBlock => {
            if plate_signal_for_state(s) == 0 { plate_check_pressed(w, pos, s, 0); }
        }
        K::TripWireBlock => {
            if !powered(s) && !w.has_scheduled_tick(pos, block_of(s)) {
                // getEntityInsideCollisionShape is the wire's own thin shape
                let tb = t();
                let shape = &tb.shapes[tb.out_shape[s as usize] as usize];
                let off = [pos.x as f64, pos.y as f64, pos.z as f64];
                let touching = match w.entities.iter().find(|e| e.is_player) {
                    Some(e) => shape.intersects(off, &e.bb.deflate(1.0e-5f32 as f64)),
                    None => true,
                };
                if touching { tripwire_check_pressed(w, pos, true); }
            }
        }
        // detector rails react to minecarts only, buttons to arrows only, hoppers take item entities: none here
        _ => {}
    }
    true
}

/// NoteBlockInstrument per block, from the `.instrument(...)` calls of Blocks.java (with the copies resolved);
/// blocks not listed play the harp. One line per instrument: `name:block block ...`.
const INSTRUMENTS: &str = "\
basedrum:stone granite polished_granite diorite polished_diorite andesite polished_andesite cobblestone bedrock gold_ore deepslate_gold_ore iron_ore deepslate_iron_ore coal_ore deepslate_coal_ore nether_gold_ore lapis_ore deepslate_lapis_ore dispenser sandstone chiseled_sandstone cut_sandstone bricks mossy_cobblestone obsidian spawner creaking_heart diamond_ore deepslate_diamond_ore furnace cobblestone_stairs stone_pressure_plate redstone_ore deepslate_redstone_ore netherrack basalt polished_basalt stone_bricks mossy_stone_bricks cracked_stone_bricks chiseled_stone_bricks mud_bricks brick_stairs stone_brick_stairs mud_brick_stairs resin_block resin_bricks resin_brick_stairs resin_brick_slab resin_brick_wall chiseled_resin_bricks nether_bricks nether_brick_fence nether_brick_stairs enchanting_table end_portal_frame end_stone sandstone_stairs emerald_ore deepslate_emerald_ore ender_chest cobblestone_wall mossy_cobblestone_wall nether_quartz_ore quartz_block chiseled_quartz_block quartz_pillar quartz_stairs dropper white_terracotta orange_terracotta magenta_terracotta light_blue_terracotta yellow_terracotta lime_terracotta pink_terracotta gray_terracotta light_gray_terracotta cyan_terracotta purple_terracotta blue_terracotta brown_terracotta green_terracotta red_terracotta black_terracotta prismarine prismarine_bricks dark_prismarine prismarine_stairs prismarine_brick_stairs dark_prismarine_stairs prismarine_slab prismarine_brick_slab dark_prismarine_slab terracotta coal_block red_sandstone chiseled_red_sandstone cut_red_sandstone red_sandstone_stairs stone_slab smooth_stone_slab sandstone_slab cut_sandstone_slab petrified_oak_slab cobblestone_slab brick_slab stone_brick_slab mud_brick_slab nether_brick_slab quartz_slab red_sandstone_slab cut_red_sandstone_slab purpur_slab smooth_stone smooth_sandstone smooth_quartz smooth_red_sandstone purpur_block purpur_pillar purpur_stairs end_stone_bricks magma_block red_nether_bricks observer white_glazed_terracotta orange_glazed_terracotta magenta_glazed_terracotta light_blue_glazed_terracotta yellow_glazed_terracotta lime_glazed_terracotta pink_glazed_terracotta gray_glazed_terracotta light_gray_glazed_terracotta cyan_glazed_terracotta purple_glazed_terracotta blue_glazed_terracotta brown_glazed_terracotta green_glazed_terracotta red_glazed_terracotta black_glazed_terracotta white_concrete orange_concrete magenta_concrete light_blue_concrete yellow_concrete lime_concrete pink_concrete gray_concrete light_gray_concrete cyan_concrete purple_concrete blue_concrete brown_concrete green_concrete red_concrete black_concrete dead_tube_coral_block dead_brain_coral_block dead_bubble_coral_block dead_fire_coral_block dead_horn_coral_block tube_coral_block brain_coral_block bubble_coral_block fire_coral_block horn_coral_block dead_tube_coral dead_brain_coral dead_bubble_coral dead_fire_coral dead_horn_coral dead_tube_coral_fan dead_brain_coral_fan dead_bubble_coral_fan dead_fire_coral_fan dead_horn_coral_fan dead_tube_coral_wall_fan dead_brain_coral_wall_fan dead_bubble_coral_wall_fan dead_fire_coral_wall_fan dead_horn_coral_wall_fan polished_granite_stairs smooth_red_sandstone_stairs mossy_stone_brick_stairs polished_diorite_stairs mossy_cobblestone_stairs end_stone_brick_stairs stone_stairs smooth_sandstone_stairs smooth_quartz_stairs granite_stairs andesite_stairs red_nether_brick_stairs polished_andesite_stairs diorite_stairs polished_granite_slab smooth_red_sandstone_slab mossy_stone_brick_slab polished_diorite_slab mossy_cobblestone_slab end_stone_brick_slab smooth_sandstone_slab smooth_quartz_slab granite_slab andesite_slab red_nether_brick_slab polished_andesite_slab diorite_slab brick_wall prismarine_wall red_sandstone_wall mossy_stone_brick_wall granite_wall stone_brick_wall mud_brick_wall nether_brick_wall andesite_wall red_nether_brick_wall sandstone_wall end_stone_brick_wall diorite_wall smoker blast_furnace stonecutter warped_nylium crimson_nylium crying_obsidian respawn_anchor blackstone blackstone_stairs blackstone_wall blackstone_slab polished_blackstone polished_blackstone_bricks cracked_polished_blackstone_bricks chiseled_polished_blackstone polished_blackstone_brick_slab polished_blackstone_brick_stairs polished_blackstone_brick_wall gilded_blackstone polished_blackstone_stairs polished_blackstone_slab polished_blackstone_pressure_plate polished_blackstone_wall chiseled_nether_bricks cracked_nether_bricks quartz_bricks tuff tuff_slab tuff_stairs tuff_wall polished_tuff polished_tuff_slab polished_tuff_stairs polished_tuff_wall chiseled_tuff tuff_bricks tuff_brick_slab tuff_brick_stairs tuff_brick_wall chiseled_tuff_bricks sulfur potent_sulfur sulfur_slab sulfur_stairs sulfur_wall polished_sulfur polished_sulfur_slab polished_sulfur_stairs polished_sulfur_wall sulfur_bricks sulfur_brick_slab sulfur_brick_stairs sulfur_brick_wall chiseled_sulfur cinnabar cinnabar_slab cinnabar_stairs cinnabar_wall polished_cinnabar polished_cinnabar_slab polished_cinnabar_stairs polished_cinnabar_wall cinnabar_bricks cinnabar_brick_slab cinnabar_brick_stairs cinnabar_brick_wall chiseled_cinnabar calcite copper_ore deepslate_copper_ore dripstone_block pointed_dripstone sulfur_spike deepslate cobbled_deepslate cobbled_deepslate_stairs cobbled_deepslate_slab cobbled_deepslate_wall polished_deepslate polished_deepslate_stairs polished_deepslate_slab polished_deepslate_wall deepslate_tiles deepslate_tile_stairs deepslate_tile_slab deepslate_tile_wall deepslate_bricks deepslate_brick_stairs deepslate_brick_slab deepslate_brick_wall chiseled_deepslate cracked_deepslate_bricks cracked_deepslate_tiles smooth_basalt raw_iron_block raw_copper_block raw_gold_block reinforced_deepslate trial_spawner vault\n\
bass:oak_planks spruce_planks birch_planks jungle_planks acacia_planks cherry_planks dark_oak_planks pale_oak_wood pale_oak_planks mangrove_planks bamboo_planks bamboo_mosaic oak_log spruce_log birch_log jungle_log acacia_log cherry_log dark_oak_log pale_oak_log mangrove_log mangrove_roots bamboo_block stripped_spruce_log stripped_birch_log stripped_jungle_log stripped_acacia_log stripped_cherry_log stripped_dark_oak_log stripped_pale_oak_log stripped_oak_log stripped_mangrove_log stripped_bamboo_block oak_wood spruce_wood birch_wood jungle_wood acacia_wood cherry_wood dark_oak_wood mangrove_wood stripped_oak_wood stripped_spruce_wood stripped_birch_wood stripped_jungle_wood stripped_acacia_wood stripped_cherry_wood stripped_dark_oak_wood stripped_pale_oak_wood stripped_mangrove_wood note_block bookshelf chiseled_bookshelf acacia_shelf bamboo_shelf birch_shelf cherry_shelf crimson_shelf dark_oak_shelf jungle_shelf mangrove_shelf oak_shelf pale_oak_shelf spruce_shelf warped_shelf oak_stairs chest crafting_table oak_sign spruce_sign birch_sign acacia_sign cherry_sign jungle_sign dark_oak_sign pale_oak_sign mangrove_sign bamboo_sign oak_door oak_wall_sign spruce_wall_sign birch_wall_sign acacia_wall_sign cherry_wall_sign jungle_wall_sign dark_oak_wall_sign pale_oak_wall_sign mangrove_wall_sign bamboo_wall_sign oak_hanging_sign spruce_hanging_sign birch_hanging_sign acacia_hanging_sign cherry_hanging_sign jungle_hanging_sign dark_oak_hanging_sign pale_oak_hanging_sign crimson_hanging_sign warped_hanging_sign mangrove_hanging_sign bamboo_hanging_sign oak_wall_hanging_sign spruce_wall_hanging_sign birch_wall_hanging_sign acacia_wall_hanging_sign cherry_wall_hanging_sign jungle_wall_hanging_sign dark_oak_wall_hanging_sign pale_oak_wall_hanging_sign mangrove_wall_hanging_sign crimson_wall_hanging_sign warped_wall_hanging_sign bamboo_wall_hanging_sign oak_pressure_plate spruce_pressure_plate birch_pressure_plate jungle_pressure_plate acacia_pressure_plate cherry_pressure_plate dark_oak_pressure_plate pale_oak_pressure_plate mangrove_pressure_plate bamboo_pressure_plate jukebox oak_fence oak_trapdoor spruce_trapdoor birch_trapdoor jungle_trapdoor acacia_trapdoor cherry_trapdoor dark_oak_trapdoor pale_oak_trapdoor mangrove_trapdoor bamboo_trapdoor brown_mushroom_block red_mushroom_block mushroom_stem oak_fence_gate spruce_stairs birch_stairs jungle_stairs trapped_chest daylight_detector acacia_stairs cherry_stairs dark_oak_stairs pale_oak_stairs mangrove_stairs bamboo_stairs bamboo_mosaic_stairs white_banner orange_banner magenta_banner light_blue_banner yellow_banner lime_banner pink_banner gray_banner light_gray_banner cyan_banner purple_banner blue_banner brown_banner green_banner red_banner black_banner white_wall_banner orange_wall_banner magenta_wall_banner light_blue_wall_banner yellow_wall_banner lime_wall_banner pink_wall_banner gray_wall_banner light_gray_wall_banner cyan_wall_banner purple_wall_banner blue_wall_banner brown_wall_banner green_wall_banner red_wall_banner black_wall_banner oak_slab spruce_slab birch_slab jungle_slab acacia_slab cherry_slab dark_oak_slab pale_oak_slab mangrove_slab bamboo_slab bamboo_mosaic_slab spruce_fence_gate birch_fence_gate jungle_fence_gate acacia_fence_gate cherry_fence_gate dark_oak_fence_gate pale_oak_fence_gate mangrove_fence_gate bamboo_fence_gate spruce_fence birch_fence jungle_fence acacia_fence cherry_fence dark_oak_fence pale_oak_fence mangrove_fence bamboo_fence spruce_door birch_door jungle_door acacia_door cherry_door dark_oak_door pale_oak_door mangrove_door bamboo_door loom barrel cartography_table fletching_table lectern smithing_table campfire soul_campfire warped_stem stripped_warped_stem warped_hyphae stripped_warped_hyphae crimson_stem stripped_crimson_stem crimson_hyphae stripped_crimson_hyphae crimson_planks warped_planks crimson_slab warped_slab crimson_pressure_plate warped_pressure_plate crimson_fence warped_fence crimson_trapdoor warped_trapdoor crimson_fence_gate warped_fence_gate crimson_stairs warped_stairs crimson_door warped_door crimson_sign warped_sign crimson_wall_sign warped_wall_sign composter bee_nest beehive\n\
snare:sand suspicious_sand red_sand gravel suspicious_gravel white_concrete_powder orange_concrete_powder magenta_concrete_powder light_blue_concrete_powder yellow_concrete_powder lime_concrete_powder pink_concrete_powder gray_concrete_powder light_gray_concrete_powder cyan_concrete_powder purple_concrete_powder blue_concrete_powder brown_concrete_powder green_concrete_powder red_concrete_powder black_concrete_powder heavy_core\n\
hat:glass white_stained_glass orange_stained_glass magenta_stained_glass light_blue_stained_glass yellow_stained_glass lime_stained_glass pink_stained_glass gray_stained_glass light_gray_stained_glass cyan_stained_glass purple_stained_glass blue_stained_glass brown_stained_glass green_stained_glass red_stained_glass black_stained_glass glass_pane beacon white_stained_glass_pane orange_stained_glass_pane magenta_stained_glass_pane light_blue_stained_glass_pane yellow_stained_glass_pane lime_stained_glass_pane pink_stained_glass_pane gray_stained_glass_pane light_gray_stained_glass_pane cyan_stained_glass_pane purple_stained_glass_pane blue_stained_glass_pane brown_stained_glass_pane green_stained_glass_pane red_stained_glass_pane black_stained_glass_pane sea_lantern conduit tinted_glass\n\
guitar:white_wool orange_wool magenta_wool light_blue_wool yellow_wool lime_wool pink_wool gray_wool light_gray_wool cyan_wool purple_wool blue_wool brown_wool green_wool red_wool black_wool\n\
bell:gold_block\n\
iron_xylophone:iron_block\n\
flute:clay\n\
cow_bell:soul_sand\n\
pling:glowstone\n\
didgeridoo:pumpkin\n\
bit:emerald_block\n\
skeleton:skeleton_skull\n\
wither_skeleton:wither_skeleton_skull\n\
zombie:zombie_head\n\
custom_head:player_head\n\
creeper:creeper_head\n\
dragon:dragon_head\n\
piglin:piglin_head\n\
banjo:hay_block\n\
chime:packed_ice\n\
xylophone:bone_block\n\
trumpet:copper_block waxed_copper_block cut_copper waxed_cut_copper chiseled_copper waxed_chiseled_copper cut_copper_stairs waxed_cut_copper_stairs cut_copper_slab waxed_cut_copper_slab\n\
trumpet_exposed:exposed_copper waxed_exposed_copper exposed_cut_copper waxed_exposed_cut_copper exposed_chiseled_copper waxed_exposed_chiseled_copper exposed_cut_copper_stairs waxed_exposed_cut_copper_stairs exposed_cut_copper_slab waxed_exposed_cut_copper_slab\n\
trumpet_weathered:weathered_copper waxed_weathered_copper weathered_cut_copper waxed_weathered_cut_copper weathered_chiseled_copper waxed_weathered_chiseled_copper weathered_cut_copper_stairs waxed_weathered_cut_copper_stairs weathered_cut_copper_slab waxed_weathered_cut_copper_slab\n\
trumpet_oxidized:oxidized_copper waxed_oxidized_copper oxidized_cut_copper waxed_oxidized_cut_copper oxidized_chiseled_copper waxed_oxidized_chiseled_copper oxidized_cut_copper_stairs waxed_oxidized_cut_copper_stairs oxidized_cut_copper_slab waxed_oxidized_cut_copper_slab\n\
";

// ---------------------------------------------------------------------------------------------- sky light (approximate)
/// How a state lets sky light through: 0 freely, 1 dimmed by one (leaves), 2 not at all.
fn light_opacity(s: S) -> u8 {
    if s == 0 { return 0; }
    let tb = t();
    if kind_of(s) == K::LeavesBlock { return 1; }
    if !tb.full_cube[s as usize] { return 0; }
    let n = tb.binfo(s).name.as_str();
    if n == "tinted_glass" { return 2; }
    if n.ends_with("glass") || n == "ice" || n == "slime_block" || n == "honey_block" || n == "barrier" || n == "spawner" || n == "beacon" || n.ends_with("copper_grate") { return 0; }
    2
}
/// Blocks whose shape decides where light passes (slabs, stairs ...): a full face stops it.
fn light_face_blocked(s: S, d: Dir) -> bool {
    if s == 0 { return false; }
    if !matches!(kind_of(s), K::SlabBlock | K::StairBlock | K::SnowLayerBlock | K::FarmlandBlock | K::DirtPathBlock | K::PistonBaseBlock | K::PistonHeadBlock | K::DaylightDetectorBlock) { return false; }
    super::face_sturdy(s, d, 0)
}
fn light_passes(w: &World, from: P, d: Dir) -> bool {
    let to = from.rel(d);
    let (a, b) = (w.get(from), w.get(to));
    light_opacity(b) < 2 && !light_face_blocked(a, d) && !light_face_blocked(b, opposite(d))
}
fn sky_exposed(w: &World, pos: P) -> bool {
    let top = w.y0 + w.ny;
    let mut q = pos;
    while q.y < top {
        if !light_passes(w, q, UP) || light_opacity(w.get(q.above())) != 0 { return false; }
        q = q.above();
    }
    true
}
/// Sky light level at a cell: 15 under the open sky, one less per step away from it. This follows the game's light
/// engine for the common cases (openings in walls and roofs); it does not model every block's light filtering.
pub fn sky_light(w: &World, pos: P) -> i32 {
    if sky_exposed(w, pos) { return 15; }
    let mut seen: std::collections::HashSet<P> = std::collections::HashSet::new();
    let mut frontier = vec![(pos, 0i32)];
    seen.insert(pos);
    let mut best = 0;
    let mut k = 0;
    while k < frontier.len() {
        let (q, dist) = frontier[k];
        k += 1;
        if 15 - dist <= best || dist >= 14 { continue; }
        for d in DIRS {
            if !light_passes(w, q, d) { continue; }
            let n = q.rel(d);
            if !seen.insert(n) { continue; }
            let nd = dist + 1 + if light_opacity(w.get(n)) == 1 { 1 } else { 0 };
            if sky_exposed(w, n) { best = best.max(15 - nd); } else { frontier.push((n, nd)); }
        }
    }
    best
}

/// (candidate `powerorder`) The kinds `power_state` reads.
pub fn power_state_kind(s: S) -> bool {
    matches!(kind_of(s), K::PistonBaseBlock | K::RedstoneTorchBlock | K::RedstoneWallTorchBlock | K::RedstoneLampBlock | K::DoorBlock | K::TrapDoorBlock
        | K::FenceGateBlock | K::NoteBlock | K::BellBlock | K::DispenserBlock | K::CrafterBlock | K::HopperBlock | K::PoweredRailBlock
        | K::RepeaterBlock | K::ComparatorBlock | K::RedStoneWireBlock)
}

/// (candidate `powerorder`) The power state of a part with target state `s`, were it put in at the empty cell `pos`
/// of the world as it now stands: (on in the target, on if put in now), "on" being what the block shows when it acts
/// - a piston out, a redstone torch or lamp lit, a door, trapdoor or gate powered, a dispenser/dropper or crafter
/// triggered, a hopper enabled, a note block, bell, powered or activator rail, repeater or comparator powered,
/// redstone wire carrying power. Each "if put in now" is read the way the block itself reads its power on a
/// neighbour change. None for a kind with no power state of its own here (an observer's is a pulse it sends, not
/// one it gets).
pub fn power_state(w: &World, s: S, pos: P) -> Option<(bool, bool)> {
    let sig = || has_neighbor_signal(w, pos);
    Some(match kind_of(s) {
        K::PistonBaseBlock => (is_true(s, P_EXTENDED), piston_neighbor_signal(w, pos, facing(s))),
        K::RedstoneTorchBlock | K::RedstoneWallTorchBlock => (is_true(s, P_LIT), !torch_has_neighbor_signal(w, pos, s)),
        K::RedstoneLampBlock => (is_true(s, P_LIT), sig()),
        K::DoorBlock => {
            let other = if get(s, P_HALF) == Some(V_LOWER) { UP } else { DOWN };
            (powered(s), sig() || has_neighbor_signal(w, pos.rel(other)))
        }
        K::TrapDoorBlock | K::FenceGateBlock | K::NoteBlock | K::BellBlock => (powered(s), sig()),
        K::DispenserBlock => (is_true(s, P_TRIGGERED), sig() || has_neighbor_signal(w, pos.above())),
        K::CrafterBlock => (is_true(s, P_TRIGGERED), sig()),
        K::HopperBlock => (is_true(s, P_ENABLED), !sig()),
        K::PoweredRailBlock => (powered(s), sig() || powered_rail_find_signal(w, pos, s, true, 0) || powered_rail_find_signal(w, pos, s, false, 0)),
        K::RepeaterBlock | K::ComparatorBlock => (powered(s), diode_should_turn_on(w, pos, s)),
        K::RedStoneWireBlock => {
            let bs = wire_block_signal(w, pos);
            let carry = if bs == 15 { 15 } else { bs.max(wire_incoming_signal(w, pos)) };
            (power(s) > 0, carry > 0)
        }
        _ => return None,
    })
}
