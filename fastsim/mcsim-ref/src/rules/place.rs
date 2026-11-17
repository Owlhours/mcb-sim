//! Placement rules: ports of the vanilla 26.2 code that decides what a click with a block item produces
//! (`BlockItem.place` up to the block write and its item subclasses, `Block.getStateForPlacement` of every block
//! class that overrides it), plus `BlockState.canSurvive`, `BlockState.canBeReplaced(BlockPlaceContext)` and the
//! support tests (`isFaceSturdy` for the three `SupportType`s, computed from the block's *support* shape).
//!
//! What the simulated world does not have is read as the bot's environment has it (see docs/porting-notes.md):
//! no fluids anywhere (waterlogged is always false, every `getFluidState` is empty), raw brightness 15 everywhere,
//! frozen noon, the clicking player is not an operator.
#![allow(dead_code, unused_variables)]
use super::PlaceCtx;
use crate::geom::*;
use crate::kinds::Kind;
use crate::props::*;
use crate::tables::*;
use crate::world::World;
use crate::{blk, in_tag, is_blk};
use std::sync::OnceLock;

// ====================================================================================================== shapes
const EPS: f64 = 1.0e-7;
pub const SUPPORT_FULL: u8 = 0;
pub const SUPPORT_CENTER: u8 = 1;
pub const SUPPORT_RIGID: u8 = 2;

/// VoxelShape.getFaceShape as plain geometry: the cross-section of the shape in the cell layer that touches the
/// face `dir` (the layer that holds coordinate 0.9999999 resp. 1.0E-7 along the face's axis; the whole shape if it is
/// "cube-like along" that axis), as rectangles [b0, b1, c0, c1] over the two other axes b = (a + 1) % 3, c = (a + 2) % 3.
fn face_rects(boxes: &[[f64; 6]], dir: Dir) -> Vec<[f64; 4]> {
    let a = axis(dir) as usize;
    let (b, c) = ((a + 1) % 3, (a + 2) % 3);
    let cube_like = boxes.iter().all(|x| x[a].abs() < EPS && (x[a + 3] - 1.0).abs() < EPS);
    let v = if is_positive(dir) { 0.9999999 } else { 1.0e-7 };
    boxes.iter().filter(|x| cube_like || (x[a] <= v && v < x[a + 3])).map(|x| [x[b], x[b + 3], x[c], x[c + 3]]).collect()
}

/// Is `region` ([b0, b1, c0, c1]) completely inside the union of the rectangles?
fn rects_cover(rects: &[[f64; 4]], region: [f64; 4]) -> bool {
    if rects.is_empty() { return false; }
    let mut gb = vec![region[0], region[1]];
    let mut gc = vec![region[2], region[3]];
    for r in rects {
        gb.push(r[0]); gb.push(r[1]); gc.push(r[2]); gc.push(r[3]);
    }
    gb.sort_by(|x, y| x.partial_cmp(y).unwrap());
    gb.dedup_by(|x, y| (*x - *y).abs() < EPS);
    gc.sort_by(|x, y| x.partial_cmp(y).unwrap());
    gc.dedup_by(|x, y| (*x - *y).abs() < EPS);
    for ib in 0..gb.len() - 1 {
        let mb = (gb[ib] + gb[ib + 1]) / 2.0;
        if mb <= region[0] || mb >= region[1] { continue; }
        for ic in 0..gc.len() - 1 {
            let mc = (gc[ic] + gc[ic + 1]) / 2.0;
            if mc <= region[2] || mc >= region[3] { continue; }
            if !rects.iter().any(|r| mb > r[0] && mb < r[1] && mc > r[2] && mc < r[3]) { return false; }
        }
    }
    true
}

/// SupportType.isSupporting for all faces of a (support) shape: bit `kind * 6 + dir`.
///  FULL:   Block.isFaceFull - the face shape is the full block
///  CENTER: the face shape contains Block.column(2, 0, 10)
///  RIGID:  the face shape contains the unit cube minus Block.column(12, 0, 16)
fn support_bits(boxes: &[[f64; 6]]) -> u32 {
    let mut bits = 0u32;
    for dir in DIRS {
        let r = face_rects(boxes, dir);
        if r.is_empty() { continue; }
        let a = axis(dir) as usize;
        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
        let full = rects_cover(&r, [0.0, 1.0, 0.0, 1.0]);
        if full { bits |= 1 << dir; }
        // the centre column: x and z in 7/16..9/16, y in 0..10/16, seen along the face's axis
        let span = |ax: usize| if ax == 1 { (0.0, 10.0 / 16.0) } else { (7.0 / 16.0, 9.0 / 16.0) };
        let (b0, b1) = span(b);
        let (c0, c1) = span(c);
        if full || rects_cover(&r, [b0, b1, c0, c1]) { bits |= 1 << (6 + dir); }
        // the rim: seen along Y it is the 2/16 wide frame, seen along a horizontal axis it covers the whole face
        let rigid = if full {
            true
        } else if a == 1 {
            let (lo, hi) = (2.0 / 16.0, 14.0 / 16.0);
            rects_cover(&r, [0.0, 1.0, 0.0, lo]) && rects_cover(&r, [0.0, 1.0, hi, 1.0]) && rects_cover(&r, [0.0, lo, lo, hi]) && rects_cover(&r, [hi, 1.0, lo, hi])
        } else {
            false
        };
        if rigid { bits |= 1 << (12 + dir); }
    }
    bits
}

/// BlockBehaviour.getBlockSupportShape where a block class overrides it (None: the collision shape).
fn support_shape_override(s: S) -> Option<Vec<[f64; 6]>> {
    let tb = t();
    let full = vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];
    match kind_of(s) {
        // LeavesBlock: Shapes.empty()
        Kind::LeavesBlock => Some(vec![]),
        // MudBlock, SoulSandBlock: Shapes.block(); ShulkerBoxBlock: Shapes.block() while the box is closed (a box is only
        // open while a player looks into it; the simulated boxes are always closed)
        Kind::MudBlock | Kind::SoulSandBlock | Kind::ShulkerBoxBlock => Some(full),
        // SnowLayerBlock: SHAPES[layers] = column(16, 0, layers * 2) (one layer higher than what one collides with)
        Kind::SnowLayerBlock => {
            let l = get(s, P_LAYERS).unwrap_or(1) as f64;
            Some(vec![[0.0, 0.0, 0.0, 1.0, l * 2.0 / 16.0, 1.0]])
        }
        // ChorusFlowerBlock: column(14, 0, 15)
        Kind::ChorusFlowerBlock => Some(vec![[1.0 / 16.0, 0.0, 1.0 / 16.0, 15.0 / 16.0, 15.0 / 16.0, 15.0 / 16.0]]),
        // FenceGateBlock: open -> empty, else SHAPE_SUPPORT by the facing's axis: Z = column(16, 4, 5, 24), X = that turned
        Kind::FenceGateBlock => {
            if is_true(s, P_OPEN) {
                Some(vec![])
            } else if matches!(get(s, P_FACING), Some(V_NORTH) | Some(V_SOUTH)) {
                Some(vec![[0.0, 5.0 / 16.0, 6.0 / 16.0, 1.0, 24.0 / 16.0, 10.0 / 16.0]])
            } else {
                Some(vec![[6.0 / 16.0, 5.0 / 16.0, 0.0, 10.0 / 16.0, 24.0 / 16.0, 1.0]])
            }
        }
        // CeilingHangingSignBlock, WallHangingSignBlock: getShape (the outline shape)
        Kind::CeilingHangingSignBlock | Kind::WallHangingSignBlock => Some(tb.shapes[tb.out_shape[s as usize] as usize].boxes.clone()),
        _ => None,
    }
}

struct Info {
    /// per state: SupportType bits of the support shape (bit kind * 6 + dir)
    sturdy: Vec<u32>,
    /// per state, from the collision shape: bits 0..6 Block.isFaceFull(dir), bits 6..12 the face shape is not empty
    col_face: Vec<u16>,
    /// per state: BlockState.isSolid (the legacy "solid" flag)
    solid: Vec<bool>,
    /// per block: 0 = isRedstoneConductor is the default (collision shape is a full block), 1 = never, 2 = always
    conductor: Vec<u8>,
    /// per block: NoteBlockInstrument of the block (value code)
    instrument: Vec<u8>,
}
static INFO: OnceLock<Info> = OnceLock::new();

fn info() -> &'static Info { INFO.get_or_init(build_info) }

fn build_info() -> Info {
    let tb = t();
    let ns = tb.shapes.len();
    let mut shape_support: Vec<u32> = Vec::with_capacity(ns);
    let mut shape_face: Vec<u16> = Vec::with_capacity(ns);
    for sh in &tb.shapes {
        let bits = support_bits(&sh.boxes);
        shape_support.push(bits);
        let mut f = (bits & 63) as u16;
        for dir in DIRS {
            if !face_rects(&sh.boxes, dir).is_empty() { f |= 1 << (6 + dir); }
        }
        shape_face.push(f);
    }
    let mut sturdy = Vec::with_capacity(tb.n);
    let mut col_face = Vec::with_capacity(tb.n);
    let mut solid = Vec::with_capacity(tb.n);
    for s in 0..tb.n {
        let cs = tb.col_shape[s] as usize;
        sturdy.push(match support_shape_override(s as S) {
            Some(boxes) => support_bits(&boxes),
            None => shape_support[cs],
        });
        col_face.push(shape_face[cs]);
        solid.push(calc_solid(s as S));
    }
    // ---- per block: redstone conductor overrides (Blocks.java: isRedstoneConductor(never / always))
    let mut conductor = vec![0u8; tb.blocks.len()];
    for (i, b) in tb.blocks.iter().enumerate() {
        let n = b.name.as_str();
        let never = matches!(
            b.kind,
            Kind::LeavesBlock | Kind::MovingPistonBlock | Kind::TntBlock | Kind::BeaconBlock | Kind::PoweredBlock | Kind::ChorusFlowerBlock | Kind::FrostedIceBlock
                | Kind::ObserverBlock | Kind::BambooStalkBlock | Kind::ScaffoldingBlock | Kind::PowderSnowBlock | Kind::CopperBulbBlock | Kind::PointedDripstoneBlock
                | Kind::SulfurSpikeBlock | Kind::PistonBaseBlock
        ) || n == "glass" || n.ends_with("stained_glass") || n == "tinted_glass" || n == "ice" || n == "glowstone" || n == "sea_lantern" || n.ends_with("copper_grate");
        let always = matches!(b.kind, Kind::SoulSandBlock | Kind::MudBlock);
        conductor[i] = if never { 1 } else if always { 2 } else { 0 };
    }
    // ---- per block: note block instrument
    let mut instrument = vec![V_HARP; tb.blocks.len()];
    for (v, names) in INSTRUMENTS {
        for n in names.split(' ') {
            if let Some(&i) = tb.block_by_name.get(n) { instrument[i as usize] = *v; }
        }
    }
    Info { sturdy, col_face, solid, conductor, instrument }
}

/// BlockStateBase.calculateSolid: forceSolidOn / forceSolidOff from Blocks.java (resolved through the property copies),
/// no cache (blocks with dynamicShape) -> false, else by the size of the collision shape.
fn calc_solid(s: S) -> bool {
    let tb = t();
    let b = tb.binfo(s);
    use Kind::*;
    match b.kind {
        // forceSolidOn
        AmethystClusterBlock | BambooSaplingBlock | BambooStalkBlock | BannerBlock | BellBlock | CakeBlock | CandleCakeBlock | CeilingHangingSignBlock | ChainBlock
        | ConduitBlock | DriedGhastBlock | FenceGateBlock | LanternBlock | LightningRodBlock | MovingPistonBlock | PointedDripstoneBlock | PressurePlateBlock
        | SculkVeinBlock | ShulkerBoxBlock | StandingSignBlock | SulfurSpikeBlock | TurtleEggBlock | WallBannerBlock | WallHangingSignBlock | WallSignBlock | WebBlock
        | WeightedPressurePlateBlock => return true,
        CoralFanBlock | CoralPlantBlock | CoralWallFanBlock if b.name.starts_with("dead_") => return true,
        // forceSolidOff
        AzaleaBlock | BigDripleafBlock | ChorusFlowerBlock | ChorusPlantBlock | EndRodBlock | LadderBlock | SnowLayerBlock => return false,
        // dynamicShape without forceSolidOn: there is no shape cache
        PowderSnowBlock | ScaffoldingBlock => return false,
        _ => {}
    }
    let sh = &tb.shapes[tb.col_shape[s as usize] as usize];
    if sh.empty { return false; }
    let (dx, dy, dz) = (sh.max[0] - sh.min[0], sh.max[1] - sh.min[1], sh.max[2] - sh.min[2]);
    (dx + dy + dz) / 3.0 >= 0.7291666666666666 || dy >= 1.0
}

/// BlockState.isFaceSturdy(level, pos, dir, supportType); kind: 0 FULL, 1 CENTER, 2 RIGID.
/// (Blocks whose shape is moved by a per-position offset - bamboo, pointed dripstone - are judged by their shape at
/// BlockPos.ZERO, which is what the tables hold.)
#[inline]
pub fn face_sturdy(s: S, dir: Dir, kind: u8) -> bool {
    let k = if kind > 2 { 0 } else { kind as u32 };
    info().sturdy[s as usize] >> (k * 6 + dir as u32) & 1 != 0
}

/// Block.isFaceFull(state.getCollisionShape(level, pos), dir)
#[inline]
pub fn col_face_full(s: S, dir: Dir) -> bool { info().col_face[s as usize] >> dir & 1 != 0 }
/// !state.getCollisionShape(level, pos).getFaceShape(dir).isEmpty()
#[inline]
pub fn col_face_present(s: S, dir: Dir) -> bool { info().col_face[s as usize] >> (6 + dir) & 1 != 0 }
/// BlockState.isSolid (deprecated "legacy solid")
#[inline]
pub fn is_solid(s: S) -> bool { info().solid[s as usize] }
/// BlockState.isRedstoneConductor
#[inline]
pub fn is_redstone_conductor(s: S) -> bool {
    match info().conductor[block_of(s) as usize] {
        1 => false,
        2 => true,
        _ => t().full_cube[s as usize],
    }
}
/// BlockState.isSignalSource
pub fn is_signal_source(s: S) -> bool {
    use Kind::*;
    match kind_of(s) {
        ObserverBlock | RedStoneWireBlock | SculkSensorBlock | TargetBlock | ButtonBlock | LecternBlock | DetectorRailBlock | JukeboxBlock | PoweredBlock | RepeaterBlock
        | ComparatorBlock | RedstoneTorchBlock | RedstoneWallTorchBlock | LeverBlock | DaylightDetectorBlock | PressurePlateBlock | WeightedPressurePlateBlock
        | LightningRodBlock | TripWireHookBlock => true,
        ChestBlock => is_blk!(s, "trapped_chest"),
        _ => false,
    }
}
/// BlockState.instrument(): the NoteBlockInstrument value code of the state's block.
pub fn note_instrument(s: S) -> u8 { info().instrument[block_of(s) as usize] }

/// Block.canSupportCenter(level, pos, direction)
pub fn can_support_center(w: &World, pos: P, dir: Dir) -> bool {
    let s = w.get(pos);
    if dir == DOWN && in_tag!(s, "unstable_bottom_center") { false } else { face_sturdy(s, dir, SUPPORT_CENTER) }
}
/// Block.canSupportRigidBlock(level, pos)
pub fn can_support_rigid(w: &World, pos: P) -> bool { face_sturdy(w.get(pos), UP, SUPPORT_RIGID) }

/// The collision shape for `CollisionContext.placementContext`: only ScaffoldingBlock (empty for a placement) and
/// PowderSnowBlock (empty without an entity that can walk on it, which the table already has) consult the context.
pub fn placement_collision_shape(s: S) -> u32 {
    let tb = t();
    if tb.scaffolding[s as usize] || kind_of(s) == Kind::ScaffoldingBlock { tb.empty_shape } else { tb.col_shape[s as usize] }
}

// ====================================================================================================== the player's view
/// Mth.sin / Mth.cos: the 65536 entry sine table (the entry is recomputed here instead of stored).
fn mth_sin(x: f32) -> f32 {
    let i = ((x as f64 * 10430.378350470453) as i64 & 65535) as f64;
    (i / 10430.378350470453).sin() as f32
}
fn mth_cos(x: f32) -> f32 {
    let i = ((x as f64 * 10430.378350470453 + 16384.0) as i64 & 65535) as f64;
    (i / 10430.378350470453).sin() as f32
}

/// Direction.orderedByNearest(player)
pub fn ordered_by_nearest(yaw: f32, pitch: f32) -> [Dir; 6] {
    let k = (std::f64::consts::PI / 180.0) as f32;
    let p = pitch * k;
    let y = -yaw * k;
    let (ps, pc, ys, yc) = (mth_sin(p), mth_cos(p), mth_sin(y), mth_cos(y));
    let x_pos = ys > 0.0;
    let y_pos = ps < 0.0;
    let z_pos = yc > 0.0;
    let x_yaw = if x_pos { ys } else { -ys };
    let y_mag = if y_pos { -ps } else { ps };
    let z_yaw = if z_pos { yc } else { -yc };
    let x_mag = x_yaw * pc;
    let z_mag = z_yaw * pc;
    let ax = if x_pos { EAST } else { WEST };
    let ay = if y_pos { UP } else { DOWN };
    let az = if z_pos { SOUTH } else { NORTH };
    let mk = |a: Dir, b: Dir, c: Dir| [a, b, c, opposite(c), opposite(b), opposite(a)];
    if x_yaw > z_yaw {
        if y_mag > x_mag {
            mk(ay, ax, az)
        } else if z_mag > y_mag {
            mk(ax, az, ay)
        } else {
            mk(ax, ay, az)
        }
    } else if y_mag > z_mag {
        mk(ay, az, ax)
    } else if x_mag > y_mag {
        mk(az, ax, ay)
    } else {
        mk(az, ay, ax)
    }
}

/// BlockPlaceContext.getNearestLookingDirections
pub fn nearest_looking_directions(c: &PlaceCtx) -> [Dir; 6] {
    let mut d = ordered_by_nearest(c.yaw, c.pitch);
    if c.replace_clicked { return d; }
    let want = opposite(c.face);
    let mut i = 0;
    while i < 6 && d[i] != want { i += 1; }
    if i > 0 && i < 6 {
        for j in (1..=i).rev() { d[j] = d[j - 1]; }
        d[0] = want;
    }
    d
}
/// BlockPlaceContext.getNearestLookingDirection
pub fn nearest_looking_direction(c: &PlaceCtx) -> Dir { ordered_by_nearest(c.yaw, c.pitch)[0] }
/// BlockPlaceContext.getNearestLookingVerticalDirection (Direction.getFacingAxis(player, Y))
fn nearest_looking_vertical(c: &PlaceCtx) -> Dir { if c.pitch < 0.0 { UP } else { DOWN } }

/// RotationSegment.convertToSegment(float degrees): Math.round(degrees * (16 / 360f)) & 15
pub fn rotation_segment(deg: f32) -> u8 {
    let v = deg * (16.0f32 / 360.0f32);
    (((v as f64 + 0.5).floor() as i64) & 15) as u8
}
/// RotationSegment.convertToSegment(Direction)
fn rotation_segment_of(d: Dir) -> u8 { if is_horizontal(d) { data2d(d) << 2 } else { 0 } }
/// RotationSegment.convertToDirection
fn segment_direction(seg: u8) -> Option<Dir> {
    match seg { 0 => Some(NORTH), 4 => Some(EAST), 8 => Some(SOUTH), 12 => Some(WEST), _ => None }
}

// ====================================================================================================== small helpers
#[inline]
fn tw(s: S, p: u8, v: u8) -> S { try_with(s, p, v) }
#[inline]
fn twb(s: S, p: u8, v: bool) -> S { try_with(s, p, if v { V_TRUE } else { V_FALSE }) }
#[inline]
fn facing(s: S) -> Option<Dir> {
    match get(s, P_FACING) {
        Some(v @ (V_DOWN | V_UP | V_NORTH | V_SOUTH | V_WEST | V_EAST)) => Some(dir_of(v)),
        _ => None,
    }
}
#[inline]
fn is_air(s: S) -> bool { t().air[s as usize] }
#[inline]
fn sturdy(w: &World, pos: P, dir: Dir) -> bool { face_sturdy(w.get(pos), dir, SUPPORT_FULL) }
fn default_of_block(b: u16) -> S { t().blocks[b as usize].default }
/// The item that `block.asItem()` is (0 = none).
#[inline]
fn item_of(s: S) -> u16 { t().item[s as usize] }

/// ItemStack.is(block.asItem()) for the item in the clicking hand.
#[inline]
fn holding_item_of(c: &PlaceCtx, s: S) -> bool { c.item != 0 && item_of(s) == c.item }

/// Block.isExceptionForConnection
fn is_exception_for_connection(s: S) -> bool {
    kind_of(s) == Kind::LeavesBlock || is_blk!(s, "barrier") || is_blk!(s, "carved_pumpkin") || is_blk!(s, "jack_o_lantern") || is_blk!(s, "melon")
        || is_blk!(s, "pumpkin") || in_tag!(s, "shulker_boxes")
}
/// FenceGateBlock.connectsToDirection
fn gate_connects_to_direction(s: S, dir: Dir) -> bool {
    match facing(s) { Some(f) if is_horizontal(f) && is_horizontal(dir) => axis(f) == axis(clockwise(dir)), _ => false }
}

// ---- growing plants (GrowingPlantHeadBlock / GrowingPlantBodyBlock)
/// (head block, body block, growth direction) of a growing plant block.
fn growing_plant(b: u16) -> Option<(u16, u16, Dir)> {
    use Kind::*;
    Some(match t().blocks[b as usize].kind {
        WeepingVinesBlock | WeepingVinesPlantBlock => (blk!("weeping_vines"), blk!("weeping_vines_plant"), DOWN),
        TwistingVinesBlock | TwistingVinesPlantBlock => (blk!("twisting_vines"), blk!("twisting_vines_plant"), UP),
        CaveVinesBlock | CaveVinesPlantBlock => (blk!("cave_vines"), blk!("cave_vines_plant"), DOWN),
        KelpBlock | KelpPlantBlock => (blk!("kelp"), blk!("kelp_plant"), UP),
        _ => return None,
    })
}

// ---- scaffolding
/// ScaffoldingBlock.getDistance
pub fn scaffolding_distance(w: &World, pos: P) -> u8 {
    let below = w.get(pos.below());
    let mut d: u8 = 7;
    if is_blk!(below, "scaffolding") {
        d = get(below, P_DISTANCE).unwrap_or(7);
    } else if face_sturdy(below, UP, SUPPORT_FULL) {
        return 0;
    }
    for dir in HORIZONTAL {
        let r = w.get(pos.rel(dir));
        if is_blk!(r, "scaffolding") {
            d = d.min(get(r, P_DISTANCE).unwrap_or(7) + 1);
            if d == 1 { break; }
        }
    }
    d
}

// ---- leaves
/// LeavesBlock.updateDistance
pub fn leaves_update_distance(w: &World, s: S, pos: P) -> S {
    let mut nd: u8 = 7;
    for dir in DIRS {
        let n = w.get(pos.rel(dir));
        // getOptionalDistanceAt: the tag gives 0, LeavesBlock.DISTANCE (leaves only; scaffolding's "distance" is another property) its value, else 7
        let dn = if in_tag!(n, "prevents_nearby_leaf_decay") { 0 } else if kind_of(n) == Kind::LeavesBlock { get(n, P_DISTANCE).unwrap_or(7) } else { 7 };
        nd = nd.min(dn + 1);
        if nd == 1 { break; }
    }
    tw(s, P_DISTANCE, nd)
}

// ---- multiface blocks, vines, moss
/// MultifaceBlock.canAttachTo(level, directionTowardsNeighbour, neighbourPos, neighbourState)
pub fn multiface_can_attach_to(ns: S, dir_to_neighbour: Dir) -> bool {
    let o = opposite(dir_to_neighbour);
    face_sturdy(ns, o, SUPPORT_FULL) || col_face_full(ns, o)
}
fn is_multiface(s: S) -> bool { matches!(kind_of(s), Kind::MultifaceBlock | Kind::GlowLichenBlock | Kind::SculkVeinBlock) }
/// MultifaceBlock.hasFace (getValueOrElse(property, false))
fn multiface_has_face(s: S, d: Dir) -> bool { is_true(s, dir_prop(d)) }

/// VineBlock.canSupportAtFace
fn vine_can_support_at_face(w: &World, vine: u16, pos: P, dir: Dir) -> bool {
    if dir == DOWN { return false; }
    if multiface_can_attach_to(w.get(pos.rel(dir)), dir) { return true; }
    if axis(dir) == AXIS_Y { return false; }
    let above = w.get(pos.above());
    block_of(above) == vine && is_true(above, dir_prop(dir))
}
/// VineBlock.getUpdatedState
fn vine_updated_state(w: &World, mut s: S, pos: P) -> S {
    let vine = block_of(s);
    let above_pos = pos.above();
    if is_true(s, P_UP) { s = twb(s, P_UP, multiface_can_attach_to(w.get(above_pos), UP)); }
    for dir in HORIZONTAL {
        let p = dir_prop(dir);
        if is_true(s, p) {
            let mut can = vine_can_support_at_face(w, vine, pos, dir);
            if !can {
                let above = w.get(above_pos);
                can = block_of(above) == vine && is_true(above, p);
            }
            s = twb(s, p, can);
        }
    }
    s
}
fn vine_count_faces(s: S) -> usize { [UP, NORTH, SOUTH, WEST, EAST].iter().filter(|d| is_true(s, dir_prop(**d))).count() }

/// MossyCarpetBlock.getUpdatedState
fn mossy_carpet_updated_state(w: &World, mut s: S, pos: P, mut create_sides: bool) -> S {
    let base = is_true(s, P_BOTTOM);
    create_sides |= base;
    for dir in HORIZONTAL {
        let p = dir_prop(dir);
        // canSupportAtFace: never UP, else MultifaceBlock.canAttachTo
        let mut side = if multiface_can_attach_to(w.get(pos.rel(dir)), dir) { if create_sides { V_LOW } else { get(s, p).unwrap_or(V_NONE) } } else { V_NONE };
        if side == V_LOW {
            let above = w.get(pos.above());
            if is_blk!(above, "pale_moss_carpet") && get(above, p) != Some(V_NONE) && !is_true(above, P_BOTTOM) { side = V_TALL; }
            if !base {
                let below = w.get(pos.below());
                if is_blk!(below, "pale_moss_carpet") && get(below, p) == Some(V_NONE) { side = V_NONE; }
            }
        }
        s = tw(s, p, side);
    }
    s
}

// ---- speleothems (pointed dripstone, sulfur spike)
fn speleothem_with_direction(s: S, tip: Dir) -> bool { in_tag!(s, "speleothems") && get(s, P_VERTICAL_DIRECTION) == Some(dir_val(tip)) }
/// SpeleothemBlock.isValidSpeleothemPlacement
fn speleothem_valid(w: &World, block: u16, pos: P, tip: Dir) -> bool {
    let bs = w.get(pos.rel(opposite(tip)));
    face_sturdy(bs, tip, SUPPORT_FULL) || speleothem_with_direction(bs, tip) && block_of(bs) == block
}

// ---- redstone wire
/// RedStoneWireBlock.shouldConnectTo(state, direction)
fn wire_should_connect_to(s: S, dir: Option<Dir>) -> bool {
    if is_blk!(s, "redstone_wire") {
        true
    } else if is_blk!(s, "repeater") {
        match (facing(s), dir) { (Some(f), Some(d)) => f == d || opposite(f) == d, _ => false }
    } else if is_blk!(s, "observer") {
        dir.is_some() && dir == facing(s)
    } else {
        is_signal_source(s) && dir.is_some()
    }
}
/// RedStoneWireBlock.canSurviveOn
fn wire_can_survive_on(s: S) -> bool { face_sturdy(s, UP, SUPPORT_FULL) || is_blk!(s, "hopper") }
/// RedStoneWireBlock.getConnectingSide(level, pos, direction, canConnectUp)
fn wire_connecting_side(w: &World, pos: P, dir: Dir, can_connect_up: bool) -> u8 {
    let rp = pos.rel(dir);
    let rs = w.get(rp);
    if can_connect_up {
        let placeable_above = kind_of(rs) == Kind::TrapDoorBlock || wire_can_survive_on(rs);
        if placeable_above && wire_should_connect_to(w.get(rp.above()), None) {
            return if face_sturdy(rs, opposite(dir), SUPPORT_FULL) { V_UP } else { V_SIDE };
        }
    }
    if !wire_should_connect_to(rs, Some(dir)) && (is_redstone_conductor(rs) || !wire_should_connect_to(w.get(rp.below()), None)) { V_NONE } else { V_SIDE }
}
/// RedStoneWireBlock.getConnectionState(level, state, pos); `state` is a wire state (its power is kept).
pub fn wire_connection_state(w: &World, state: S, pos: P) -> S {
    let connected = |s: S, d: Dir| matches!(get(s, dir_prop(d)), Some(V_UP) | Some(V_SIDE));
    let is_dot = |s: S| !connected(s, NORTH) && !connected(s, SOUTH) && !connected(s, EAST) && !connected(s, WEST);
    let was_dot = is_dot(state);
    // getMissingConnections(level, defaultBlockState().setValue(POWER, state.power), pos)
    let mut s = state;
    for d in HORIZONTAL { s = tw(s, dir_prop(d), V_NONE); }
    let can_connect_up = !is_redstone_conductor(w.get(pos.above()));
    for d in HORIZONTAL {
        if !connected(s, d) { s = tw(s, dir_prop(d), wire_connecting_side(w, pos, d, can_connect_up)); }
    }
    if was_dot && is_dot(s) { return s; }
    let (north, south, east, west) = (connected(s, NORTH), connected(s, SOUTH), connected(s, EAST), connected(s, WEST));
    let ns_empty = !north && !south;
    let ew_empty = !east && !west;
    if !west && ns_empty { s = tw(s, P_WEST, V_SIDE); }
    if !east && ns_empty { s = tw(s, P_EAST, V_SIDE); }
    if !north && ew_empty { s = tw(s, P_NORTH, V_SIDE); }
    if !south && ew_empty { s = tw(s, P_SOUTH, V_SIDE); }
    s
}

// ---- walls
/// WallBlock.updateShape(level, state, topPos, topNeighbour, north, east, south, west)
pub fn wall_shape(state: S, top: S, north: bool, east: bool, south: bool, west: bool) -> S {
    let tb = t();
    // the bottom face of whatever is above, as rectangles over (z, x)
    let above = face_rects(&tb.shapes[tb.col_shape[top as usize] as usize].boxes, DOWN);
    let (a, b, e) = (7.0 / 16.0, 9.0 / 16.0, 1.0);
    // isCovered(aboveShape, test) = the test shape lies inside the face shape; regions are [z0, z1, x0, x1]
    let side = |con: bool, region: [f64; 4]| if con { if rects_cover(&above, region) { V_TALL } else { V_LOW } } else { V_NONE };
    // TEST_SHAPES_WALL = rotateHorizontal(boxZ(2, 16, 0, 9)): north z 0..9, east x 7..16, south z 7..16, west x 0..9 (each 2 wide)
    let n = side(north, [0.0, b, a, b]);
    let ea = side(east, [a, b, a, e]);
    let so = side(south, [a, e, a, b]);
    let we = side(west, [a, b, 0.0, b]);
    let mut s = tw(tw(tw(tw(state, P_NORTH, n), P_EAST, ea), P_SOUTH, so), P_WEST, we);
    // shouldRaisePost
    let up = if kind_of(top) == Kind::WallBlock && is_true(top, P_UP) {
        true
    } else {
        let (nn, sn, en, wn) = (n == V_NONE, so == V_NONE, ea == V_NONE, we == V_NONE);
        let has_corner = nn && sn && wn && en || nn != sn || wn != en;
        if has_corner {
            true
        } else {
            let high = n == V_TALL && so == V_TALL || ea == V_TALL && we == V_TALL;
            // TEST_SHAPE_POST = column(2, 0, 16)
            if high { false } else { in_tag!(top, "wall_post_override") || rects_cover(&above, [a, b, a, b]) }
        }
    };
    s = twb(s, P_UP, up);
    s
}
/// WallBlock.connectsTo
fn wall_connects_to(s: S, face_solid: bool, dir: Dir) -> bool {
    in_tag!(s, "walls") || !is_exception_for_connection(s) && face_solid || kind_of(s) == Kind::IronBarsBlock
        || kind_of(s) == Kind::FenceGateBlock && gate_connects_to_direction(s, dir)
}

// ---- stairs
fn is_stairs(s: S) -> bool { kind_of(s) == Kind::StairBlock }
/// StairBlock.getStairsShape
pub fn stairs_shape(w: &World, s: S, pos: P) -> u8 {
    let Some(f) = facing(s) else { return V_STRAIGHT };
    if !is_horizontal(f) { return V_STRAIGHT; }
    let half = get(s, P_HALF);
    let can_take = |nb: Dir| {
        let n = w.get(pos.rel(nb));
        !is_stairs(n) || facing(n) != Some(f) || get(n, P_HALF) != half
    };
    let behind = w.get(pos.rel(f));
    if is_stairs(behind) && half == get(behind, P_HALF) {
        if let Some(bf) = facing(behind) {
            if axis(bf) != axis(f) && can_take(opposite(bf)) {
                return if bf == counter_clockwise(f) { V_OUTER_LEFT } else { V_OUTER_RIGHT };
            }
        }
    }
    let front = w.get(pos.rel(opposite(f)));
    if is_stairs(front) && half == get(front, P_HALF) {
        if let Some(ff) = facing(front) {
            if axis(ff) != axis(f) && can_take(ff) {
                return if ff == counter_clockwise(f) { V_INNER_LEFT } else { V_INNER_RIGHT };
            }
        }
    }
    V_STRAIGHT
}

// ---- chests
fn is_copper_chest(s: S) -> bool { kind_of(s) == Kind::ChestBlock && in_tag!(s, "copper_chests") }
/// ChestBlock.chestCanConnectTo / CopperChestBlock.chestCanConnectTo for a chest of block `b`
fn chest_can_connect_to(b: u16, s: S) -> bool {
    if in_tag!(default_of_block(b), "copper_chests") { is_copper_chest(s) } else { block_of(s) == b }
}
/// ChestBlock.candidatePartnerFacing
fn chest_candidate_partner_facing(w: &World, b: u16, pos: P, dir: Dir) -> Option<Dir> {
    let s = w.get(pos.rel(dir));
    if chest_can_connect_to(b, s) && get(s, P_TYPE) == Some(V_SINGLE) { facing(s) } else { None }
}
/// ChestBlock.getConnectedDirection
pub fn chest_connected_direction(s: S) -> Option<Dir> {
    let f = facing(s)?;
    if !is_horizontal(f) { return None; }
    Some(if get(s, P_TYPE) == Some(V_LEFT) { clockwise(f) } else { counter_clockwise(f) })
}
/// (waxed, WeatherState ordinal) of a copper chest block name.
fn copper_chest_parts(name: &str) -> (bool, u8) {
    let (waxed, rest) = match name.strip_prefix("waxed_") { Some(r) => (true, r), None => (false, name) };
    let w = if rest.starts_with("exposed_") { 1 } else if rest.starts_with("weathered_") { 2 } else if rest.starts_with("oxidized_") { 3 } else { 0 };
    (waxed, w)
}
fn copper_chest_block(waxed: bool, weather: u8) -> Option<u16> {
    let n = format!("{}{}copper_chest", if waxed { "waxed_" } else { "" }, ["", "exposed_", "weathered_", "oxidized_"][weather as usize & 3]);
    t().block_by_name.get(&n).copied()
}
/// Block.withPropertiesOf for chests
fn chest_with_properties_of(block: u16, s: S) -> S {
    let mut r = default_of_block(block);
    for p in [P_FACING, P_TYPE, P_WATERLOGGED] {
        if let Some(v) = get(s, p) { r = tw(r, p, v); }
    }
    r
}
/// CopperChestBlock.getLeastOxidizedChestOfConnectedBlocks
fn copper_chest_least_oxidized(w: &World, s: S, pos: P) -> S {
    let Some(cd) = chest_connected_direction(s) else { return s };
    let cs = w.get(pos.rel(cd));
    if get(s, P_TYPE) == Some(V_SINGLE) || !is_copper_chest(s) || !is_copper_chest(cs) { return s; }
    let tb = t();
    let (mw, mo) = copper_chest_parts(&tb.binfo(s).name);
    let (tw_, to) = copper_chest_parts(&tb.binfo(cs).name);
    // different waxing: both are taken unwaxed
    let (mw2, tw2) = if mw != tw_ { (false, false) } else { (mw, tw_) };
    let Some(mine) = copper_chest_block(mw2, mo) else { return s };
    let Some(theirs) = copper_chest_block(tw2, to) else { return s };
    let updated = chest_with_properties_of(mine, s);
    let least = if mo <= to { mine } else { theirs };
    chest_with_properties_of(least, updated)
}

// ---- repeaters
/// RepeaterBlock.isLocked: DiodeBlock.getAlternateSignal with sideInputDiodesOnly - the direct signal of a diode at either
/// side that points at this block. (A comparator's output is non-zero exactly when its POWERED property is set, once it
/// has settled, so the property stands in for the block entity's output signal.)
pub fn repeater_is_locked(w: &World, pos: P, s: S) -> bool {
    let Some(f) = facing(s) else { return false };
    if !is_horizontal(f) { return false; }
    [clockwise(f), counter_clockwise(f)].iter().any(|&side| {
        let n = w.get(pos.rel(side));
        matches!(kind_of(n), Kind::RepeaterBlock | Kind::ComparatorBlock) && is_true(n, P_POWERED) && facing(n) == Some(side)
    })
}

// ---- FrontAndTop (jigsaw, crafter)
fn front_and_top(front: Dir, top: Dir) -> Option<u8> {
    Some(match (front, top) {
        (DOWN, EAST) => V_DOWN_EAST,
        (DOWN, NORTH) => V_DOWN_NORTH,
        (DOWN, SOUTH) => V_DOWN_SOUTH,
        (DOWN, WEST) => V_DOWN_WEST,
        (UP, EAST) => V_UP_EAST,
        (UP, NORTH) => V_UP_NORTH,
        (UP, SOUTH) => V_UP_SOUTH,
        (UP, WEST) => V_UP_WEST,
        (WEST, UP) => V_WEST_UP,
        (EAST, UP) => V_EAST_UP,
        (NORTH, UP) => V_NORTH_UP,
        (SOUTH, UP) => V_SOUTH_UP,
        _ => return None,
    })
}

// ---- hanging signs
/// WallHangingSignBlock.canAttachTo
fn wall_hanging_sign_can_attach_to(w: &World, s: S, attach_pos: P, attach_face: Dir) -> bool {
    let a = w.get(attach_pos);
    if in_tag!(a, "wall_hanging_signs") {
        match (facing(a), facing(s)) { (Some(x), Some(y)) => axis(x) == axis(y), _ => false }
    } else {
        face_sturdy(a, attach_face, SUPPORT_FULL)
    }
}
/// WallHangingSignBlock.canPlace
pub fn wall_hanging_sign_can_place(w: &World, s: S, pos: P) -> bool {
    let Some(f) = facing(s) else { return false };
    if !is_horizontal(f) { return false; }
    let (cw, ccw) = (clockwise(f), counter_clockwise(f));
    wall_hanging_sign_can_attach_to(w, s, pos.rel(cw), ccw) || wall_hanging_sign_can_attach_to(w, s, pos.rel(ccw), cw)
}

// ---- note blocks
/// NoteBlock.setInstrument
pub fn note_block_instrument(w: &World, pos: P, s: S) -> S {
    let works_above = |v: u8| matches!(v, V_ZOMBIE | V_SKELETON | V_CREEPER | V_DRAGON | V_WITHER_SKELETON | V_PIGLIN | V_CUSTOM_HEAD);
    let above = note_instrument(w.get(pos.above()));
    if works_above(above) { return tw(s, P_INSTRUMENT, above); }
    let below = note_instrument(w.get(pos.below()));
    tw(s, P_INSTRUMENT, if works_above(below) { V_HARP } else { below })
}

// ====================================================================================================== canBeReplaced
/// BlockState.canBeReplaced(BlockPlaceContext). `pos` is where the state is; the overrides that ask the context for the
/// clicked position (slabs, vines) get it from the context as vanilla does.
pub fn can_be_replaced(w: &World, s: S, pos: P, ctx: &PlaceCtx) -> bool {
    let tb = t();
    // BlockBehaviour.canBeReplaced(state, context)
    let dflt = || tb.replaceable[s as usize] && (ctx.item == 0 || item_of(s) != ctx.item);
    use Kind::*;
    match kind_of(s) {
        CandleBlock => !ctx.sneak && holding_item_of(ctx, s) && get(s, P_CANDLES).unwrap_or(4) < 4 || dflt(),
        SeaPickleBlock => !ctx.sneak && holding_item_of(ctx, s) && get(s, P_PICKLES).unwrap_or(4) < 4 || dflt(),
        TurtleEggBlock => !ctx.sneak && holding_item_of(ctx, s) && get(s, P_EGGS).unwrap_or(4) < 4 || dflt(),
        // SegmentableBlock.canBeReplaced
        FlowerBedBlock => !ctx.sneak && holding_item_of(ctx, s) && get(s, P_FLOWER_AMOUNT).unwrap_or(4) < 4 || dflt(),
        LeafLitterBlock => !ctx.sneak && holding_item_of(ctx, s) && get(s, P_SEGMENT_AMOUNT).unwrap_or(4) < 4 || dflt(),
        // GrowingPlantBodyBlock: not by the head's item
        CaveVinesPlantBlock | WeepingVinesPlantBlock | TwistingVinesPlantBlock | KelpPlantBlock => {
            let r = dflt();
            let head_item = growing_plant(block_of(s)).map(|g| tb.blocks[g.0 as usize].item).unwrap_or(0);
            if r && ctx.item != 0 && ctx.item == head_item { false } else { r }
        }
        // MultifaceBlock: by anything else, and by its own item while a face is vacant
        MultifaceBlock | GlowLichenBlock | SculkVeinBlock => !holding_item_of(ctx, s) || DIRS.iter().any(|d| !multiface_has_face(s, *d)),
        PitcherCropBlock => false,
        ScaffoldingBlock => holding_item_of(ctx, s),
        SlabBlock => {
            let ty = get(s, P_TYPE);
            if ty == Some(V_DOUBLE) || !holding_item_of(ctx, s) {
                false
            } else if ctx.replace_clicked {
                let above = ctx.hit[1] - ctx.pos().y as f64 > 0.5;
                let face = ctx.face;
                if ty == Some(V_BOTTOM) { face == UP || above && is_horizontal(face) } else { face == DOWN || !above && is_horizontal(face) }
            } else {
                true
            }
        }
        SnowLayerBlock => {
            let layers = get(s, P_LAYERS).unwrap_or(1);
            if !holding_item_of(ctx, s) || layers >= 8 {
                layers == 1
            } else if ctx.replace_clicked {
                ctx.face == UP
            } else {
                true
            }
        }
        VineBlock => {
            let clicked = w.get(ctx.pos());
            if same_block(clicked, s) { vine_count_faces(clicked) < 5 } else { dflt() }
        }
        _ => dflt(),
    }
}

// ====================================================================================================== canSurvive
/// VegetationBlock.mayPlaceOn and its overrides: can the plant state `s` stand on the block `below` (at `bpos`)?
fn may_place_on(w: &World, s: S, below: S, bpos: P) -> bool {
    use Kind::*;
    match kind_of(s) {
        AzaleaBlock => in_tag!(below, "supports_azalea"),
        AttachedStemBlock | StemBlock => {
            if is_blk!(s, "pumpkin_stem") || is_blk!(s, "attached_pumpkin_stem") { in_tag!(below, "supports_pumpkin_stem") } else { in_tag!(below, "supports_melon_stem") }
        }
        CactusFlowerBlock => in_tag!(below, "support_override_cactus_flower") || face_sturdy(below, UP, SUPPORT_CENTER),
        CropBlock | TorchflowerCropBlock | PitcherCropBlock => in_tag!(below, "supports_crops"),
        DryVegetationBlock => in_tag!(below, "supports_dry_vegetation"),
        NetherFungusBlock => if is_blk!(s, "warped_fungus") { in_tag!(below, "supports_warped_fungus") } else { in_tag!(below, "supports_crimson_fungus") },
        NetherRootsBlock => if is_blk!(s, "warped_roots") { in_tag!(below, "supports_warped_roots") } else { in_tag!(below, "supports_crimson_roots") },
        NetherSproutsBlock => in_tag!(below, "supports_nether_sprouts"),
        NetherWartBlock => in_tag!(below, "supports_nether_wart"),
        MangrovePropaguleBlock => in_tag!(below, "supports_mangrove_propagule"),
        // LilyPadBlock: (water below or the tag) and nothing fluid above the support - there is no water
        LilyPadBlock => in_tag!(below, "supports_lily_pad"),
        SeaPickleBlock => col_face_present(below, UP) || face_sturdy(below, UP, SUPPORT_FULL),
        SeagrassBlock | TallSeagrassBlock => face_sturdy(below, UP, SUPPORT_FULL) && !in_tag!(below, "cannot_support_seagrass"),
        // SmallDripleafBlock: the tag, or under water on anything that supports vegetation - there is no water
        SmallDripleafBlock => in_tag!(below, "supports_small_dripleaf"),
        FlowerBlock if is_blk!(s, "wither_rose") => in_tag!(below, "supports_wither_rose"),
        _ => in_tag!(below, "supports_vegetation"),
    }
}
fn vegetation_can_survive(w: &World, s: S, pos: P) -> bool { may_place_on(w, s, w.get(pos.below()), pos.below()) }
/// DoublePlantBlock.canSurvive
fn double_plant_can_survive(w: &World, s: S, pos: P) -> bool {
    if get(s, P_HALF) != Some(V_UPPER) { return vegetation_can_survive(w, s, pos); }
    let below = w.get(pos.below());
    same_block(below, s) && get(below, P_HALF) == Some(V_LOWER)
}

/// BlockState.canSurvive(level, pos)
pub fn can_survive(w: &World, s: S, pos: P) -> bool {
    use Kind::*;
    let below = || w.get(pos.below());
    let attached_sturdy = |f: Option<Dir>| match f {
        // the block behind the state (opposite its facing) is sturdy on the face towards it
        Some(f) => sturdy(w, pos.rel(opposite(f)), f),
        None => false,
    };
    match kind_of(s) {
        AmethystClusterBlock => attached_sturdy(facing(s)),
        BambooSaplingBlock | BambooStalkBlock => in_tag!(below(), "supports_bamboo"),
        BannerBlock | CakeBlock | CandleCakeBlock | StandingSignBlock => is_solid(below()),
        CoralPlantBlock | CoralFanBlock => sturdy(w, pos.below(), UP),
        CoralWallFanBlock => attached_sturdy(facing(s)),
        PressurePlateBlock | WeightedPressurePlateBlock => can_support_rigid(w, pos.below()) || can_support_center(w, pos.below(), UP),
        RailBlock | PoweredRailBlock | DetectorRailBlock => can_support_rigid(w, pos.below()),
        TorchBlock | RedstoneTorchBlock => can_support_center(w, pos.below(), UP),
        WallTorchBlock | RedstoneWallTorchBlock => attached_sturdy(facing(s)),
        BellBlock => match get(s, P_ATTACHMENT) {
            // getConnectedDirection: FLOOR -> UP, CEILING -> DOWN, walls -> facing.getOpposite(); the support is on the other side
            Some(V_FLOOR) => sturdy(w, pos.below(), UP),
            Some(V_CEILING) => can_support_center(w, pos.above(), DOWN),
            _ => match facing(s) { Some(f) => sturdy(w, pos.rel(f), opposite(f)), None => false },
        },
        BigDripleafBlock => {
            let b = below();
            same_block(b, s) || is_blk!(b, "big_dripleaf_stem") || in_tag!(b, "supports_big_dripleaf")
        }
        BigDripleafStemBlock => {
            let b = below();
            let a = w.get(pos.above());
            (same_block(b, s) || in_tag!(b, "supports_big_dripleaf")) && (same_block(a, s) || is_blk!(a, "big_dripleaf"))
        }
        BubbleColumnBlock => {
            let b = below();
            same_block(b, s) || in_tag!(b, "enables_bubble_column_push_up") || in_tag!(b, "enables_bubble_column_drag_down")
        }
        CactusBlock => {
            for d in HORIZONTAL {
                // (or lava next to it: there is none)
                if is_solid(w.get(pos.rel(d))) { return false; }
            }
            let b = below();
            // (and no liquid above)
            same_block(b, s) || in_tag!(b, "supports_cactus")
        }
        CandleBlock => can_support_center(w, pos.below(), UP),
        CarpetBlock => !is_air(below()),
        CeilingHangingSignBlock => face_sturdy(w.get(pos.above()), DOWN, SUPPORT_CENTER),
        ChorusFlowerBlock => {
            let b = below();
            if !is_blk!(b, "chorus_plant") && !in_tag!(b, "supports_chorus_flower") {
                if !is_air(b) { return false; }
                let mut one = false;
                for d in HORIZONTAL {
                    let n = w.get(pos.rel(d));
                    if is_blk!(n, "chorus_plant") {
                        if one { return false; }
                        one = true;
                    } else if !is_air(n) {
                        return false;
                    }
                }
                one
            } else {
                true
            }
        }
        ChorusPlantBlock => {
            let b = below();
            let above_or_below = !is_air(w.get(pos.above())) && !is_air(b);
            for d in HORIZONTAL {
                let np = pos.rel(d);
                if same_block(w.get(np), s) {
                    if above_or_below { return false; }
                    let nb = w.get(np.below());
                    if same_block(nb, s) || in_tag!(nb, "supports_chorus_plant") { return true; }
                }
            }
            same_block(b, s) || in_tag!(b, "supports_chorus_plant")
        }
        CocoaBlock => match facing(s) { Some(f) => in_tag!(w.get(pos.rel(f)), "supports_cocoa"), None => false },
        // CropBlock: hasSufficientLight (raw brightness >= 8; it is 15) && VegetationBlock.canSurvive
        CropBlock | TorchflowerCropBlock => vegetation_can_survive(w, s, pos),
        // DiodeBlock.canSurviveOn: rigid support
        RepeaterBlock | ComparatorBlock => can_support_rigid(w, pos.below()),
        DirtPathBlock => {
            let a = w.get(pos.above());
            !is_solid(a) || kind_of(a) == FenceGateBlock
        }
        DoorBlock => {
            let b = below();
            if get(s, P_HALF) == Some(V_LOWER) { face_sturdy(b, UP, SUPPORT_FULL) } else { same_block(b, s) }
        }
        DoublePlantBlock | TallFlowerBlock => double_plant_can_survive(w, s, pos),
        // PitcherCropBlock: the lower half also needs light (it has it)
        PitcherCropBlock => double_plant_can_survive(w, s, pos),
        SmallDripleafBlock => {
            if get(s, P_HALF) == Some(V_UPPER) { double_plant_can_survive(w, s, pos) } else { may_place_on(w, s, below(), pos.below()) }
        }
        TallSeagrassBlock => {
            if get(s, P_HALF) == Some(V_UPPER) {
                let b = below();
                same_block(b, s) && get(b, P_HALF) == Some(V_LOWER)
            } else {
                // needs to stand in water: there is none
                false
            }
        }
        // FaceAttachedHorizontalDirectionalBlock.canSurvive: canAttach(level, pos, getConnectedDirection(state).getOpposite())
        ButtonBlock | LeverBlock => match get(s, P_FACE) {
            Some(V_CEILING) => sturdy(w, pos.above(), DOWN),
            Some(V_FLOOR) => sturdy(w, pos.below(), UP),
            _ => attached_sturdy(facing(s)),
        },
        GrindstoneBlock => true,
        FarmlandBlock => {
            let a = w.get(pos.above());
            !is_solid(a) || in_tag!(a, "maintains_farmland")
        }
        FireBlock => {
            if is_blk!(s, "soul_fire") {
                in_tag!(below(), "soul_fire_base_blocks")
            } else {
                // FireBlock: sturdy below or a flammable neighbour (isValidFireLocation). The flammability table is not
                // ported: no block item places fire and nothing in the simulated world can light one.
                sturdy(w, pos.below(), UP)
            }
        }
        // FrogspawnBlock.mayPlaceOn: water (or the - empty - tag) below
        FrogspawnBlock => in_tag!(below(), "supports_frogspawn"),
        // GrowingPlantBlock.canSurvive
        WeepingVinesBlock | WeepingVinesPlantBlock | TwistingVinesBlock | TwistingVinesPlantBlock | CaveVinesBlock | CaveVinesPlantBlock | KelpBlock | KelpPlantBlock => {
            let Some((head, body, growth)) = growing_plant(block_of(s)) else { return true };
            let ap = pos.rel(opposite(growth));
            let a = w.get(ap);
            // canAttachTo: kelp does not attach to CANNOT_SUPPORT_KELP
            if matches!(kind_of(s), KelpBlock | KelpPlantBlock) && in_tag!(a, "cannot_support_kelp") { return false; }
            block_of(a) == head || block_of(a) == body || face_sturdy(a, growth, SUPPORT_FULL)
        }
        HangingMossBlock => {
            let a = w.get(pos.above());
            multiface_can_attach_to(a, UP) || same_block(a, s)
        }
        HangingRootsBlock => sturdy(w, pos.above(), DOWN),
        LadderBlock | TripWireHookBlock => match facing(s) { Some(f) if is_horizontal(f) => attached_sturdy(Some(f)), _ => false },
        LanternBlock => {
            // getConnectedDirection(state).getOpposite(): hanging -> UP, else DOWN
            let d = if is_true(s, P_HANGING) { UP } else { DOWN };
            can_support_center(w, pos.rel(d), opposite(d))
        }
        LeafLitterBlock => sturdy(w, pos.below(), UP),
        MangrovePropaguleBlock => {
            if is_true(s, P_HANGING) { in_tag!(w.get(pos.above()), "supports_hanging_mangrove_propagule") } else { vegetation_can_survive(w, s, pos) }
        }
        MossyCarpetBlock => {
            let b = below();
            if is_true(s, P_BOTTOM) { !is_air(b) } else { same_block(b, s) && is_true(b, P_BOTTOM) }
        }
        MultifaceBlock | GlowLichenBlock | SculkVeinBlock => {
            let mut any = false;
            for d in DIRS {
                if multiface_has_face(s, d) {
                    if !multiface_can_attach_to(w.get(pos.rel(d)), d) { return false; }
                    any = true;
                }
            }
            any
        }
        // MushroomBlock: on OVERRIDES_MUSHROOM_LIGHT_REQUIREMENT, or raw brightness < 13 (it is 15) on a solid-render block
        MushroomBlock => in_tag!(below(), "overrides_mushroom_light_requirement"),
        RedStoneWireBlock => wire_can_survive_on(below()),
        ScaffoldingBlock => scaffolding_distance(w, pos) < 7,
        SnowLayerBlock => {
            let b = below();
            if in_tag!(b, "cannot_support_snow_layer") {
                false
            } else if in_tag!(b, "support_override_snow_layer") {
                true
            } else {
                col_face_full(b, UP) || same_block(b, s) && get(b, P_LAYERS) == Some(8)
            }
        }
        PointedDripstoneBlock | SulfurSpikeBlock => {
            let tip = if get(s, P_VERTICAL_DIRECTION) == Some(V_DOWN) { DOWN } else { UP };
            speleothem_valid(w, block_of(s), pos, tip)
        }
        // SporeBlossomBlock: centre support above (and not in water)
        SporeBlossomBlock => can_support_center(w, pos.above(), DOWN),
        SugarCaneBlock => {
            let b = below();
            if same_block(b, s) { return true; }
            if in_tag!(b, "supports_sugar_cane") {
                for d in HORIZONTAL {
                    // (water next to the support, or a block of the tag)
                    if in_tag!(w.get(pos.below().rel(d)), "supports_sugar_cane_adjacently") { return true; }
                }
            }
            false
        }
        VineBlock => vine_count_faces(vine_updated_state(w, s, pos)) > 0,
        WallBannerBlock | WallSignBlock => match facing(s) { Some(f) => is_solid(w.get(pos.rel(opposite(f)))), None => false },
        PistonHeadBlock => {
            let Some(f) = facing(s) else { return false };
            let base = w.get(pos.rel(opposite(f)));
            let fitting = if get(s, P_TYPE) == Some(V_STICKY) { is_blk!(base, "sticky_piston") } else { is_blk!(base, "piston") };
            fitting && is_true(base, P_EXTENDED) && facing(base) == Some(f) || is_blk!(base, "moving_piston") && facing(base) == Some(f)
        }
        // VegetationBlock.canSurvive with the class's mayPlaceOn
        AttachedStemBlock | AzaleaBlock | BushBlock | CactusFlowerBlock | DryVegetationBlock | FireflyBushBlock | FlowerBedBlock | FlowerBlock | LilyPadBlock
        | NetherFungusBlock | NetherRootsBlock | NetherSproutsBlock | NetherWartBlock | SaplingBlock | SeaPickleBlock | SeagrassBlock | StemBlock
        | SweetBerryBushBlock | TallGrassBlock => vegetation_can_survive(w, s, pos),
        _ => true,
    }
}

// ====================================================================================================== getStateForPlacement
/// Block.getStateForPlacement(context) for the block with index `b`.
pub fn state_for_placement(w: &World, c: &PlaceCtx, b: u16) -> Option<S> {
    let r = state_for_placement_inner(w, c, b)?;
    // every `.setValue(WATERLOGGED, <fluid at the clicked position is water>)`: there is no water
    Some(tw(r, P_WATERLOGGED, V_FALSE))
}

fn state_for_placement_inner(w: &World, c: &PlaceCtx, b: u16) -> Option<S> {
    let tb = t();
    let bi = &tb.blocks[b as usize];
    let d = bi.default;
    let pos = c.pos();
    let hd = c.horizontal_direction();
    let here = w.get(pos);
    let signal = || super::has_neighbor_signal(w, pos);
    use Kind::*;
    Some(match bi.kind {
        // ---- facing blocks
        FurnaceBlock | BeehiveBlock | CarvedPumpkinBlock | ChiseledBookShelfBlock | GlazedTerracottaBlock | LoomBlock | StonecutterBlock | VaultBlock | EnderChestBlock
        | CopperGolemStatueBlock | DriedGhastBlock | ComparatorBlock => tw(d, P_FACING, dir_val(opposite(hd))),
        EndPortalFrameBlock => twb(tw(d, P_FACING, dir_val(opposite(hd))), P_EYE, false),
        // LecternBlock: HAS_BOOK only from an operator's item data
        LecternBlock => twb(tw(d, P_FACING, dir_val(opposite(hd))), P_HAS_BOOK, false),
        ShelfBlock => twb(tw(d, P_FACING, dir_val(opposite(hd))), P_POWERED, signal()),
        AnvilBlock => tw(d, P_FACING, dir_val(clockwise(hd))),
        DecoratedPotBlock => twb(tw(d, P_FACING, dir_val(hd)), P_CRACKED, false),
        BarrelBlock | DispenserBlock | CommandBlock => tw(d, P_FACING, dir_val(opposite(nearest_looking_direction(c)))),
        // ObserverBlock: getNearestLookingDirection().getOpposite().getOpposite()
        ObserverBlock => tw(d, P_FACING, dir_val(nearest_looking_direction(c))),
        PistonBaseBlock => twb(tw(d, P_FACING, dir_val(opposite(nearest_looking_direction(c)))), P_EXTENDED, false),
        AmethystClusterBlock | LightningRodBlock | ShulkerBoxBlock => tw(d, P_FACING, dir_val(c.face)),
        EndRodBlock => {
            let o = w.get(pos.rel(opposite(c.face)));
            if same_block(o, d) && facing(o) == Some(c.face) { tw(d, P_FACING, dir_val(opposite(c.face))) } else { tw(d, P_FACING, dir_val(c.face)) }
        }
        HopperBlock => {
            let dir = opposite(c.face);
            twb(tw(d, P_FACING, dir_val(if axis(dir) == AXIS_Y { DOWN } else { dir })), P_ENABLED, true)
        }
        RotatedPillarBlock | ChainBlock => tw(d, P_AXIS, axis_val(axis(c.face))),
        CreakingHeartBlock => {
            let s = tw(d, P_AXIS, axis_val(axis(c.face)));
            // updateState: with pale oak logs of the same axis at both ends an uprooted heart becomes dormant by day (awake at
            // night: CREAKING_ACTIVE; the simulated time is noon)
            let ax = axis(c.face);
            let ends: [Dir; 2] = match ax { AXIS_X => [WEST, EAST], AXIS_Y => [DOWN, UP], _ => [NORTH, SOUTH] };
            let has_logs = ends.iter().all(|e| {
                let n = w.get(pos.rel(*e));
                in_tag!(n, "pale_oak_logs") && get(n, P_AXIS) == Some(axis_val(ax))
            });
            if has_logs && get(s, P_CREAKING_HEART_STATE) == Some(V_UPROOTED) { tw(s, P_CREAKING_HEART_STATE, V_DORMANT) } else { s }
        }
        JigsawBlock => {
            let front = c.face;
            let top = if axis(front) == AXIS_Y { opposite(hd) } else { UP };
            tw(d, P_ORIENTATION, front_and_top(front, top)?)
        }
        CrafterBlock => {
            let front = opposite(nearest_looking_direction(c));
            let top = match front { DOWN => opposite(hd), UP => hd, _ => UP };
            twb(tw(d, P_ORIENTATION, front_and_top(front, top)?), P_TRIGGERED, signal())
        }
        // ---- skulls, banners, signs
        BannerBlock | StandingSignBlock => tw(d, P_ROTATION, rotation_segment(c.yaw + 180.0)),
        SkullBlock => tw(twb(d, P_POWERED, signal()), P_ROTATION, rotation_segment(c.yaw)),
        WallSkullBlock => {
            let mut s = twb(d, P_POWERED, signal());
            for dir in nearest_looking_directions(c) {
                if is_horizontal(dir) {
                    s = tw(s, P_FACING, dir_val(opposite(dir)));
                    let np = pos.rel(dir);
                    if !can_be_replaced(w, w.get(np), np, c) { return Some(s); }
                }
            }
            return None;
        }
        WallBannerBlock | WallSignBlock | WallTorchBlock | TripWireHookBlock | CoralWallFanBlock => {
            let mut s = d;
            if bi.kind == TripWireHookBlock { s = twb(twb(s, P_POWERED, false), P_ATTACHED, false); }
            for dir in nearest_looking_directions(c) {
                if is_horizontal(dir) {
                    s = tw(s, P_FACING, dir_val(opposite(dir)));
                    if can_survive(w, s, pos) { return Some(s); }
                }
            }
            return None;
        }
        // RedstoneWallTorchBlock: Blocks.WALL_TORCH.getStateForPlacement(context), then its facing on this block
        RedstoneWallTorchBlock => {
            let mut s = d;
            for dir in nearest_looking_directions(c) {
                if is_horizontal(dir) {
                    s = tw(s, P_FACING, dir_val(opposite(dir)));
                    if sturdy(w, pos.rel(dir), opposite(dir)) { return Some(s); }
                }
            }
            return None;
        }
        WallHangingSignBlock => {
            let mut s = d;
            for dir in nearest_looking_directions(c) {
                if is_horizontal(dir) && axis(dir) != axis(c.face) {
                    s = tw(s, P_FACING, dir_val(opposite(dir)));
                    if can_survive(w, s, pos) && wall_hanging_sign_can_place(w, s, pos) { return Some(s); }
                }
            }
            return None;
        }
        CeilingHangingSignBlock => {
            let above = w.get(pos.above());
            let below_hanging_sign = in_tag!(above, "all_hanging_signs");
            let direction = from_yrot(c.yaw as f64);
            let mut attached_to_middle = !col_face_full(above, DOWN) || c.sneak;
            if below_hanging_sign && !c.sneak {
                if kind_of(above) == WallHangingSignBlock {
                    if let Some(f) = facing(above) {
                        if axis(f) == axis(direction) { attached_to_middle = false; }
                    }
                } else if let Some(rot) = get(above, P_ROTATION) {
                    if let Some(ad) = segment_direction(rot) {
                        if axis(ad) == axis(direction) { attached_to_middle = false; }
                    }
                }
            }
            let seg = if !attached_to_middle { rotation_segment_of(opposite(direction)) } else { rotation_segment(c.yaw + 180.0) };
            tw(twb(d, P_ATTACHED, attached_to_middle), P_ROTATION, seg)
        }
        // ---- plants and things that stand on something
        BambooStalkBlock => {
            let below = w.get(pos.below());
            if !in_tag!(below, "supports_bamboo") { return None; }
            if is_blk!(below, "bamboo_sapling") {
                tw(d, P_AGE, 0)
            } else if is_blk!(below, "bamboo") {
                tw(d, P_AGE, if get(below, P_AGE).unwrap_or(0) > 0 { 1 } else { 0 })
            } else {
                let above = w.get(pos.above());
                if is_blk!(above, "bamboo") { tw(d, P_AGE, get(above, P_AGE).unwrap_or(0)) } else { tb.default_of("bamboo_sapling") }
            }
        }
        BigDripleafBlock => {
            let below = w.get(pos.below());
            let part = is_blk!(below, "big_dripleaf") || is_blk!(below, "big_dripleaf_stem");
            let f = if part { facing(below).unwrap_or(opposite(hd)) } else { opposite(hd) };
            tw(d, P_FACING, dir_val(f))
        }
        SmallDripleafBlock => {
            if !(pos.y < w.max_y && can_be_replaced(w, w.get(pos.above()), pos.above(), c)) { return None; }
            tw(d, P_FACING, dir_val(opposite(hd)))
        }
        DoublePlantBlock | TallFlowerBlock => {
            if !(pos.y < w.max_y && can_be_replaced(w, w.get(pos.above()), pos.above(), c)) { return None; }
            d
        }
        // need water at the clicked position
        TallSeagrassBlock | SeagrassBlock | KelpBlock => return None,
        MangrovePropaguleBlock => tw(d, P_AGE, 4),
        CocoaBlock => {
            let mut s = d;
            for dir in nearest_looking_directions(c) {
                if is_horizontal(dir) {
                    s = tw(s, P_FACING, dir_val(dir));
                    if can_survive(w, s, pos) { return Some(s); }
                }
            }
            return None;
        }
        // GrowingPlantBlock: the body block if the plant already continues in the growth direction, else the block's own
        // state (heads get a random age 0..24: RandomSource of the level; here a fixed function of the position)
        WeepingVinesBlock | WeepingVinesPlantBlock | TwistingVinesBlock | TwistingVinesPlantBlock | CaveVinesBlock | CaveVinesPlantBlock | KelpPlantBlock => {
            let (head, body, growth) = growing_plant(b)?;
            let g = w.get(pos.rel(growth));
            if block_of(g) != head && block_of(g) != body {
                if b == head {
                    let h = (pos.x as i64).wrapping_mul(3129871) ^ (pos.z as i64).wrapping_mul(116129781) ^ pos.y as i64;
                    tw(d, P_AGE, (h.rem_euclid(25)) as u8)
                } else {
                    d
                }
            } else {
                default_of_block(body)
            }
        }
        FlowerBedBlock | LeafLitterBlock => {
            let p = if bi.kind == FlowerBedBlock { P_FLOWER_AMOUNT } else { P_SEGMENT_AMOUNT };
            if block_of(here) == b { tw(here, p, (get(here, p).unwrap_or(1) + 1).min(4)) } else { tw(d, P_FACING, dir_val(opposite(hd))) }
        }
        CandleBlock => {
            if block_of(here) == b {
                // state.cycle(CANDLES)
                let n = get(here, P_CANDLES).unwrap_or(1);
                tw(here, P_CANDLES, if n >= 4 { 1 } else { n + 1 })
            } else {
                d
            }
        }
        SeaPickleBlock => {
            if block_of(here) == b { tw(here, P_PICKLES, (get(here, P_PICKLES).unwrap_or(1) + 1).min(4)) } else { d }
        }
        TurtleEggBlock => {
            if block_of(here) == b { tw(here, P_EGGS, (get(here, P_EGGS).unwrap_or(1) + 1).min(4)) } else { d }
        }
        SnowLayerBlock => {
            if block_of(here) == b { tw(here, P_LAYERS, (get(here, P_LAYERS).unwrap_or(1) + 1).min(8)) } else { d }
        }
        SnowyBlock => twb(d, P_SNOWY, in_tag!(w.get(pos.above()), "snow")),
        // FarmlandBlock / DirtPathBlock: dirt where the block could not stay
        FarmlandBlock | DirtPathBlock => {
            if !can_survive(w, d, pos) { tb.default_of("dirt") } else { d }
        }
        LeavesBlock => leaves_update_distance(w, twb(d, P_PERSISTENT, true), pos),
        HugeMushroomBlock => {
            let mut s = d;
            for dir in DIRS { s = twb(s, dir_prop(dir), block_of(w.get(pos.rel(dir))) != b); }
            s
        }
        ChorusPlantBlock => {
            let mut s = d;
            for dir in DIRS {
                let n = w.get(pos.rel(dir));
                let v = block_of(n) == b || is_blk!(n, "chorus_flower") || dir == DOWN && in_tag!(n, "supports_chorus_plant");
                s = twb(s, dir_prop(dir), v);
            }
            s
        }
        MossyCarpetBlock => mossy_carpet_updated_state(w, d, pos, true),
        VineBlock => {
            let clicked_vine = block_of(here) == b;
            let result = if clicked_vine { here } else { d };
            for dir in nearest_looking_directions(c) {
                if dir != DOWN {
                    let p = dir_prop(dir);
                    let occupied = clicked_vine && is_true(here, p);
                    if !occupied && vine_can_support_at_face(w, b, pos, dir) { return Some(twb(result, p, true)); }
                }
            }
            if clicked_vine { result } else { return None }
        }
        MultifaceBlock | GlowLichenBlock | SculkVeinBlock => {
            for dir in nearest_looking_directions(c) {
                // isValidStateForPlacement
                if has(d, dir_prop(dir)) && (block_of(here) != b || !multiface_has_face(here, dir)) && multiface_can_attach_to(w.get(pos.rel(dir)), dir) {
                    let base = if block_of(here) == b { here } else { d };
                    return Some(twb(base, dir_prop(dir), true));
                }
            }
            return None;
        }
        PointedDripstoneBlock | SulfurSpikeBlock => {
            let default_tip = opposite(nearest_looking_vertical(c));
            let tip = if speleothem_valid(w, b, pos, default_tip) {
                default_tip
            } else if speleothem_valid(w, b, pos, opposite(default_tip)) {
                opposite(default_tip)
            } else {
                return None;
            };
            let merge = !c.sneak;
            // calculateSpeleothemThickness
            let base_dir = opposite(tip);
            let front = w.get(pos.rel(tip));
            let thickness = if speleothem_with_direction(front, base_dir) && block_of(front) == b {
                if !merge && get(front, P_THICKNESS) != Some(V_TIP_MERGE) { V_TIP } else { V_TIP_MERGE }
            } else if !speleothem_with_direction(front, tip) {
                V_TIP
            } else {
                let ft = get(front, P_THICKNESS);
                if ft != Some(V_TIP) && ft != Some(V_TIP_MERGE) {
                    let behind = w.get(pos.rel(base_dir));
                    if !speleothem_with_direction(behind, tip) { V_BASE } else { V_MIDDLE }
                } else {
                    V_FRUSTUM
                }
            };
            tw(tw(d, P_VERTICAL_DIRECTION, dir_val(tip)), P_THICKNESS, thickness)
        }
        // ---- attached things
        ButtonBlock | LeverBlock | GrindstoneBlock => {
            for dir in nearest_looking_directions(c) {
                let s = if axis(dir) == AXIS_Y {
                    tw(tw(d, P_FACE, if dir == UP { V_CEILING } else { V_FLOOR }), P_FACING, dir_val(hd))
                } else {
                    tw(tw(d, P_FACE, V_WALL), P_FACING, dir_val(opposite(dir)))
                };
                if can_survive(w, s, pos) { return Some(s); }
            }
            return None;
        }
        LadderBlock => {
            if !c.replace_clicked {
                let o = w.get(pos.rel(opposite(c.face)));
                if block_of(o) == b && facing(o) == Some(c.face) { return None; }
            }
            let mut s = d;
            for dir in nearest_looking_directions(c) {
                if is_horizontal(dir) {
                    s = tw(s, P_FACING, dir_val(opposite(dir)));
                    if can_survive(w, s, pos) { return Some(s); }
                }
            }
            return None;
        }
        LanternBlock => {
            for dir in nearest_looking_directions(c) {
                if axis(dir) == AXIS_Y {
                    let s = twb(d, P_HANGING, dir == UP);
                    if can_survive(w, s, pos) { return Some(s); }
                }
            }
            return None;
        }
        BellBlock => {
            let face = c.face;
            if axis(face) == AXIS_Y {
                let s = tw(tw(d, P_ATTACHMENT, if face == DOWN { V_CEILING } else { V_FLOOR }), P_FACING, dir_val(hd));
                if can_survive(w, s, pos) { return Some(s); }
            } else {
                let double = axis(face) == AXIS_X && sturdy(w, pos.rel(WEST), EAST) && sturdy(w, pos.rel(EAST), WEST)
                    || axis(face) == AXIS_Z && sturdy(w, pos.rel(NORTH), SOUTH) && sturdy(w, pos.rel(SOUTH), NORTH);
                let mut s = tw(tw(d, P_FACING, dir_val(opposite(face))), P_ATTACHMENT, if double { V_DOUBLE_WALL } else { V_SINGLE_WALL });
                if can_survive(w, s, pos) { return Some(s); }
                let attach_below = sturdy(w, pos.below(), UP);
                s = tw(s, P_ATTACHMENT, if attach_below { V_FLOOR } else { V_CEILING });
                if can_survive(w, s, pos) { return Some(s); }
            }
            return None;
        }
        // ---- building blocks with a shape
        StairBlock => {
            let top = !(c.face != DOWN && (c.face == UP || !(c.hit[1] - pos.y as f64 > 0.5)));
            let s = tw(tw(d, P_FACING, dir_val(hd)), P_HALF, if top { V_TOP } else { V_BOTTOM });
            tw(s, P_SHAPE, stairs_shape(w, s, pos))
        }
        SlabBlock => {
            if block_of(here) == b { return Some(tw(here, P_TYPE, V_DOUBLE)); }
            let bottom = c.face != DOWN && (c.face == UP || !(c.hit[1] - pos.y as f64 > 0.5));
            tw(d, P_TYPE, if bottom { V_BOTTOM } else { V_TOP })
        }
        TrapDoorBlock => {
            let mut s = if !c.replace_clicked && is_horizontal(c.face) {
                tw(tw(d, P_FACING, dir_val(c.face)), P_HALF, if c.hit[1] - pos.y as f64 > 0.5 { V_TOP } else { V_BOTTOM })
            } else {
                tw(tw(d, P_FACING, dir_val(opposite(hd))), P_HALF, if c.face == UP { V_BOTTOM } else { V_TOP })
            };
            if signal() { s = twb(twb(s, P_OPEN, true), P_POWERED, true); }
            s
        }
        DoorBlock => {
            if !(pos.y < w.max_y && can_be_replaced(w, w.get(pos.above()), pos.above(), c)) { return None; }
            let powered = signal() || super::has_neighbor_signal(w, pos.above());
            let s = tw(tw(d, P_FACING, dir_val(hd)), P_HINGE, door_hinge(w, c, pos, hd));
            tw(twb(twb(s, P_POWERED, powered), P_OPEN, powered), P_HALF, V_LOWER)
        }
        FenceGateBlock => {
            let open = signal();
            let is_wall = |q: P| in_tag!(w.get(q), "walls");
            let in_wall = axis(hd) == AXIS_Z && (is_wall(pos.rel(WEST)) || is_wall(pos.rel(EAST))) || axis(hd) == AXIS_X && (is_wall(pos.rel(NORTH)) || is_wall(pos.rel(SOUTH)));
            twb(twb(twb(tw(d, P_FACING, dir_val(hd)), P_OPEN, open), P_POWERED, open), P_IN_WALL, in_wall)
        }
        FenceBlock => {
            let wooden = in_tag!(d, "wooden_fences");
            let mut s = d;
            for dir in HORIZONTAL {
                let n = w.get(pos.rel(dir));
                let o = opposite(dir);
                let same_fence = in_tag!(n, "fences") && in_tag!(n, "wooden_fences") == wooden;
                let gate = kind_of(n) == FenceGateBlock && gate_connects_to_direction(n, o);
                let con = !is_exception_for_connection(n) && face_sturdy(n, o, SUPPORT_FULL) || same_fence || gate;
                s = twb(s, dir_prop(dir), con);
            }
            s
        }
        IronBarsBlock => {
            let mut s = d;
            for dir in HORIZONTAL {
                let n = w.get(pos.rel(dir));
                let con = !is_exception_for_connection(n) && face_sturdy(n, opposite(dir), SUPPORT_FULL) || kind_of(n) == IronBarsBlock || in_tag!(n, "walls");
                s = twb(s, dir_prop(dir), con);
            }
            s
        }
        WallBlock => {
            let con = |dir: Dir| {
                let n = w.get(pos.rel(dir));
                wall_connects_to(n, face_sturdy(n, opposite(dir), SUPPORT_FULL), opposite(dir))
            };
            wall_shape(d, w.get(pos.above()), con(NORTH), con(EAST), con(SOUTH), con(WEST))
        }
        // ---- containers and machines
        ChestBlock => {
            let mut ty = V_SINGLE;
            let mut f = opposite(hd);
            let face = c.face;
            if is_horizontal(face) && c.sneak {
                if let Some(nf) = chest_candidate_partner_facing(w, b, pos, opposite(face)) {
                    if axis(nf) != axis(face) && is_horizontal(nf) {
                        f = nf;
                        ty = if counter_clockwise(f) == opposite(face) { V_RIGHT } else { V_LEFT };
                    }
                }
            }
            if ty == V_SINGLE && !c.sneak {
                // getChestType
                ty = if Some(f) == chest_candidate_partner_facing(w, b, pos, clockwise(f)) {
                    V_LEFT
                } else if Some(f) == chest_candidate_partner_facing(w, b, pos, counter_clockwise(f)) {
                    V_RIGHT
                } else {
                    V_SINGLE
                };
            }
            let s = tw(tw(d, P_FACING, dir_val(f)), P_TYPE, ty);
            if in_tag!(d, "copper_chests") { copper_chest_least_oxidized(w, s, pos) } else { s }
        }
        BedBlock => {
            let rel = pos.rel(hd);
            // (and inside the world border)
            if !can_be_replaced(w, w.get(rel), rel, c) { return None; }
            tw(d, P_FACING, dir_val(hd))
        }
        CampfireBlock => {
            let s = twb(d, P_SIGNAL_FIRE, is_blk!(w.get(pos.below()), "hay_block"));
            tw(twb(s, P_LIT, true), P_FACING, dir_val(hd))
        }
        // SculkSensorBlock: default; CalibratedSculkSensorBlock adds FACING = horizontal direction
        SculkSensorBlock => tw(d, P_FACING, dir_val(hd)),
        NoteBlock => note_block_instrument(w, pos, d),
        RedstoneLampBlock => twb(d, P_LIT, signal()),
        RepeaterBlock => {
            let s = tw(d, P_FACING, dir_val(opposite(hd)));
            twb(s, P_LOCKED, repeater_is_locked(w, pos, s))
        }
        RedStoneWireBlock => {
            // getConnectionState(level, crossState, pos)
            let mut cross = d;
            for dir in HORIZONTAL { cross = tw(cross, dir_prop(dir), V_SIDE); }
            wire_connection_state(w, cross, pos)
        }
        RailBlock | PoweredRailBlock | DetectorRailBlock => tw(d, P_SHAPE, if hd == EAST || hd == WEST { V_EAST_WEST } else { V_NORTH_SOUTH }),
        TripWireBlock => {
            let mut s = d;
            for dir in HORIZONTAL {
                let n = w.get(pos.rel(dir));
                // shouldConnectTo(state, direction): a hook facing back at the wire, or more wire
                let con = if is_blk!(n, "tripwire_hook") { facing(n) == Some(opposite(dir)) } else { block_of(n) == b };
                s = twb(s, dir_prop(dir), con);
            }
            s
        }
        ScaffoldingBlock => {
            let dist = scaffolding_distance(w, pos);
            let bottom = dist > 0 && block_of(w.get(pos.below())) != b;
            twb(tw(d, P_DISTANCE, dist), P_BOTTOM, bottom)
        }
        // PotentSulfurBlock.validBlockState: without a water source above it is dry
        PotentSulfurBlock => tw(d, P_POTENT_SULFUR_STATE, V_DRY),
        // BaseFireBlock.getState (never reached through an item)
        FireBlock => {
            if in_tag!(w.get(pos.below()), "soul_fire_base_blocks") { tb.default_of("soul_fire") } else { tb.default_of("fire") }
        }
        // ConcretePowderBlock hardens only next to water; CoralBlock schedules its dying when placed (item_place_block);
        // waterloggable blocks (barrier, conduit, coral plants, grates, hanging roots, heavy core, mangrove roots, sculk
        // shrieker ...) only read the fluid; everything else is Block.getStateForPlacement
        _ => d,
    })
}

/// DoorBlock.getHinge
fn door_hinge(w: &World, c: &PlaceCtx, pos: P, place_dir: Dir) -> u8 {
    let tb = t();
    let above = pos.above();
    let left_dir = counter_clockwise(place_dir);
    let right_dir = clockwise(place_dir);
    let left = w.get(pos.rel(left_dir));
    let left_above = w.get(above.rel(left_dir));
    let right = w.get(pos.rel(right_dir));
    let right_above = w.get(above.rel(right_dir));
    let full = |s: S| tb.full_cube[s as usize];
    let balance = (if full(left) { -1 } else { 0 }) + (if full(left_above) { -1 } else { 0 }) + (if full(right) { 1 } else { 0 }) + (if full(right_above) { 1 } else { 0 });
    let door_left = kind_of(left) == Kind::DoorBlock && get(left, P_HALF) == Some(V_LOWER);
    let door_right = kind_of(right) == Kind::DoorBlock && get(right, P_HALF) == Some(V_LOWER);
    if (!door_left || door_right) && balance <= 0 {
        if (!door_right || door_left) && balance >= 0 {
            let (sx, _, sz) = STEP[place_dir as usize];
            let cx = c.hit[0] - pos.x as f64;
            let cz = c.hit[2] - pos.z as f64;
            if (sx >= 0 || !(cz < 0.5)) && (sx <= 0 || !(cz > 0.5)) && (sz >= 0 || !(cx > 0.5)) && (sz <= 0 || !(cx < 0.5)) { V_LEFT } else { V_RIGHT }
        } else {
            V_LEFT
        }
    } else {
        V_RIGHT
    }
}

// ====================================================================================================== the items
/// Which Item class places a block (world/item/Items.java).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemClass {
    /// BlockItem (also SolidBucketItem and the items named differently from their block)
    Plain,
    /// StandingAndWallBlockItem / SignItem / BannerItem / PlayerHeadItem with its attachment direction; HangingSignItem is `hanging`
    StandingAndWall { wall: u16, attach: Dir, hanging: bool },
    DoubleHigh,
    Bed,
    Scaffolding,
    PlaceOnWater,
    GameMaster,
}

/// The block an item places first (BlockItem.getBlock) and the item's class; None for an item that places no block.
pub fn item_class(item: u16) -> Option<(u16, ItemClass)> {
    let tb = t();
    if item == 0 || item as usize >= tb.items.len() { return None; }
    let it = &tb.items[item as usize];
    let primary = *it.blocks.first()?;
    let pb = &tb.blocks[primary as usize];
    use Kind::*;
    let wall_of = |k: Kind| it.blocks.iter().copied().find(|x| tb.blocks[*x as usize].kind == k);
    let standing = |k: Kind, attach: Dir, hanging: bool| match wall_of(k) {
        Some(wall) => ItemClass::StandingAndWall { wall, attach, hanging },
        None => ItemClass::Plain,
    };
    let class = match pb.kind {
        TorchBlock => standing(WallTorchBlock, DOWN, false),
        RedstoneTorchBlock => standing(RedstoneWallTorchBlock, DOWN, false),
        SkullBlock => standing(WallSkullBlock, DOWN, false),
        CoralFanBlock => standing(CoralWallFanBlock, DOWN, false),
        StandingSignBlock => standing(WallSignBlock, DOWN, false),
        BannerBlock => standing(WallBannerBlock, DOWN, false),
        CeilingHangingSignBlock => standing(WallHangingSignBlock, UP, true),
        DoorBlock | TallFlowerBlock | SmallDripleafBlock => ItemClass::DoubleHigh,
        DoublePlantBlock if pb.name == "tall_grass" || pb.name == "large_fern" => ItemClass::DoubleHigh,
        BedBlock => ItemClass::Bed,
        ScaffoldingBlock => ItemClass::Scaffolding,
        LilyPadBlock | FrogspawnBlock => ItemClass::PlaceOnWater,
        CommandBlock | JigsawBlock | GameMasterBlock => ItemClass::GameMaster,
        _ if pb.name == "test_block" => ItemClass::GameMaster,
        _ => ItemClass::Plain,
    };
    Some((primary, class))
}

/// ScaffoldingBlockItem.updatePlacementContext
fn scaffolding_update_context(w: &World, ctx: &PlaceCtx, block: u16) -> Option<PlaceCtx> {
    let pos = ctx.pos();
    if block_of(w.get(pos)) != block {
        return if scaffolding_distance(w, pos) == 7 { None } else { Some(ctx.clone()) };
    }
    let direction = if ctx.sneak {
        if ctx.inside { opposite(ctx.face) } else { ctx.face }
    } else if ctx.face == UP {
        ctx.horizontal_direction()
    } else {
        UP
    };
    let mut horizontal_distance = 0;
    let mut p = pos.rel(direction);
    while horizontal_distance < 7 {
        // level.isInWorldBounds (the build limit; the lower limit and the horizontal one are out of the bot's reach)
        if p.y > w.max_y { break; }
        let replaced = w.get(p);
        if block_of(replaced) != block {
            if can_be_replaced(w, replaced, p, ctx) { return Some(ctx.at(w, p, direction)); }
            break;
        }
        p = p.rel(direction);
        if is_horizontal(direction) { horizontal_distance += 1; }
    }
    None
}

/// The part of BlockItem.place that follows the item's own entry point: canPlace of the context, updatePlacementContext,
/// getPlacementState (getStateForPlacement and BlockItem.canPlace). Returns the clicked position and the state.
fn place_pipeline(w: &World, ctx: &PlaceCtx, primary: u16, class: ItemClass) -> Option<(P, S)> {
    // BlockPlaceContext.canPlace
    if !(ctx.replace_clicked || can_be_replaced(w, w.get(ctx.pos()), ctx.pos(), ctx)) { return None; }
    // updatePlacementContext
    let moved;
    let c: &PlaceCtx = if class == ItemClass::Scaffolding {
        moved = scaffolding_update_context(w, ctx, primary)?;
        &moved
    } else {
        ctx
    };
    let pos = c.pos();
    // getPlacementState
    let state = match class {
        // GameMasterBlockItem: nothing for a player that cannot use game master blocks
        ItemClass::GameMaster => return None,
        ItemClass::StandingAndWall { wall, attach, hanging } => {
            let wall_state = state_for_placement(w, c, wall);
            let mut found = None;
            for dir in nearest_looking_directions(c) {
                if dir != opposite(attach) {
                    let possible = if dir == attach { state_for_placement(w, c, primary) } else { wall_state };
                    if let Some(ps) = possible {
                        // HangingSignItem.canPlace: a wall hanging sign also needs WallHangingSignBlock.canPlace
                        let ok = if hanging && kind_of(ps) == Kind::WallHangingSignBlock && !wall_hanging_sign_can_place(w, ps, pos) { false } else { can_survive(w, ps, pos) };
                        if ok {
                            found = Some(ps);
                            break;
                        }
                    }
                }
            }
            let st = found?;
            // (CollisionContext.empty() here instead of the placement context: no difference for these blocks)
            if !w.is_unobstructed(st, pos) { return None; }
            st
        }
        _ => {
            let st = state_for_placement(w, c, primary)?;
            // BlockItem.canPlace: mustSurvive (not ScaffoldingBlockItem) -> canSurvive, and the space is free of entities
            let must_survive = class != ItemClass::Scaffolding;
            if must_survive && !can_survive(w, st, pos) { return None; }
            if !w.is_unobstructed(st, pos) { return None; }
            st
        }
    };
    Some((pos, state))
}

/// Everything BlockItem.place does for `ctx.item` before the block is written (as reached through Item.useOn): the
/// position the block goes to and its state, or None if the click places nothing.
/// PlaceOnWaterBlockItem (lily pad, frogspawn) passes in useOn - it places through `use`, see `predict_place_on_water`.
pub fn predict_place(w: &World, ctx: &PlaceCtx) -> Option<(P, S)> {
    let (primary, class) = item_class(ctx.item)?;
    if class == ItemClass::PlaceOnWater { return None; }
    place_pipeline(w, ctx, primary, class)
}

/// PlaceOnWaterBlockItem.use: `ray` is the context of the player's view ray hit (ClipContext.Fluid.SOURCE_ONLY; without
/// water that is the block looked at, or the end of the ray on a miss); the item is then used on the position above the
/// hit. With no water the only supports are the blocks of the item's tag (ice for a lily pad, nothing for frogspawn).
pub fn predict_place_on_water(w: &World, ray: &PlaceCtx) -> Option<(P, S)> {
    let (primary, class) = item_class(ray.item)?;
    if class != ItemClass::PlaceOnWater { return None; }
    let mut c = ray.clone();
    c.hit_pos = ray.hit_pos.above();
    let c = c.resolve(w);
    place_pipeline(w, &c, primary, ItemClass::Plain)
}

/// BlockItem.placeBlock and its overrides: writes the block of a placement that `predict_place` returned.
pub fn item_place_block(w: &mut World, ctx: &PlaceCtx, pos: P, state: S) -> bool {
    let class = item_class(ctx.item).map(|x| x.1).unwrap_or(ItemClass::Plain);
    // CoralBlock.getStateForPlacement: a coral block with no water around it gets its dying tick scheduled (vanilla does
    // this while it computes the state, i.e. just before the write)
    if kind_of(state) == Kind::CoralBlock {
        let delay = 60 + w.rand_int(40) as i32;
        w.schedule_tick(pos, block_of(state), delay);
    }
    match class {
        ItemClass::DoubleHigh => {
            // DoubleHighBlockItem: the block above is cleared first (air; water if there were any), flags 27
            w.set_block(pos.above(), 0, 27);
            w.set_block(pos, state, 11)
        }
        // BedItem: flags 26
        ItemClass::Bed => w.set_block(pos, state, 26),
        _ => w.set_block(pos, state, 11),
    }
}

// ====================================================================================================== data
/// NoteBlockInstrument per block, from the `instrument(...)` calls in Blocks.java resolved through the property copies
/// (ofFullCopy / ofLegacyCopy, the stair / slab / wall helpers, the copper families). Everything else is HARP.
const INSTRUMENTS: &[(u8, &str)] = &[
    (V_BANJO, "hay_block"),
    (V_BASEDRUM, "stone granite polished_granite diorite polished_diorite andesite polished_andesite cobblestone bedrock gold_ore deepslate_gold_ore iron_ore deepslate_iron_ore coal_ore deepslate_coal_ore nether_gold_ore lapis_ore deepslate_lapis_ore dispenser sandstone chiseled_sandstone cut_sandstone bricks mossy_cobblestone obsidian spawner creaking_heart diamond_ore deepslate_diamond_ore furnace cobblestone_stairs stone_pressure_plate redstone_ore deepslate_redstone_ore netherrack basalt polished_basalt stone_bricks mossy_stone_bricks cracked_stone_bricks chiseled_stone_bricks mud_bricks brick_stairs stone_brick_stairs mud_brick_stairs resin_block resin_bricks resin_brick_stairs resin_brick_slab resin_brick_wall chiseled_resin_bricks nether_bricks nether_brick_fence nether_brick_stairs enchanting_table end_portal_frame end_stone sandstone_stairs emerald_ore deepslate_emerald_ore ender_chest cobblestone_wall mossy_cobblestone_wall nether_quartz_ore quartz_block chiseled_quartz_block quartz_pillar quartz_stairs dropper white_terracotta orange_terracotta magenta_terracotta light_blue_terracotta yellow_terracotta lime_terracotta pink_terracotta gray_terracotta light_gray_terracotta cyan_terracotta purple_terracotta blue_terracotta brown_terracotta green_terracotta red_terracotta black_terracotta prismarine prismarine_bricks dark_prismarine prismarine_stairs prismarine_brick_stairs dark_prismarine_stairs prismarine_slab prismarine_brick_slab dark_prismarine_slab terracotta coal_block red_sandstone chiseled_red_sandstone cut_red_sandstone red_sandstone_stairs stone_slab smooth_stone_slab sandstone_slab cut_sandstone_slab petrified_oak_slab cobblestone_slab brick_slab stone_brick_slab mud_brick_slab nether_brick_slab quartz_slab red_sandstone_slab cut_red_sandstone_slab purpur_slab smooth_stone smooth_sandstone smooth_quartz smooth_red_sandstone purpur_block purpur_pillar purpur_stairs end_stone_bricks magma_block red_nether_bricks observer white_glazed_terracotta orange_glazed_terracotta magenta_glazed_terracotta light_blue_glazed_terracotta yellow_glazed_terracotta lime_glazed_terracotta pink_glazed_terracotta gray_glazed_terracotta light_gray_glazed_terracotta cyan_glazed_terracotta purple_glazed_terracotta blue_glazed_terracotta brown_glazed_terracotta green_glazed_terracotta red_glazed_terracotta black_glazed_terracotta white_concrete orange_concrete magenta_concrete light_blue_concrete yellow_concrete lime_concrete pink_concrete gray_concrete light_gray_concrete cyan_concrete purple_concrete blue_concrete brown_concrete green_concrete red_concrete black_concrete dead_tube_coral_block dead_brain_coral_block dead_bubble_coral_block dead_fire_coral_block dead_horn_coral_block tube_coral_block brain_coral_block bubble_coral_block fire_coral_block horn_coral_block dead_tube_coral dead_brain_coral dead_bubble_coral dead_fire_coral dead_horn_coral dead_tube_coral_fan dead_brain_coral_fan dead_bubble_coral_fan dead_fire_coral_fan dead_horn_coral_fan dead_tube_coral_wall_fan dead_brain_coral_wall_fan dead_bubble_coral_wall_fan dead_fire_coral_wall_fan dead_horn_coral_wall_fan polished_granite_stairs smooth_red_sandstone_stairs mossy_stone_brick_stairs polished_diorite_stairs mossy_cobblestone_stairs end_stone_brick_stairs stone_stairs smooth_sandstone_stairs smooth_quartz_stairs granite_stairs andesite_stairs red_nether_brick_stairs polished_andesite_stairs diorite_stairs polished_granite_slab smooth_red_sandstone_slab mossy_stone_brick_slab polished_diorite_slab mossy_cobblestone_slab end_stone_brick_slab smooth_sandstone_slab smooth_quartz_slab granite_slab andesite_slab red_nether_brick_slab polished_andesite_slab diorite_slab brick_wall prismarine_wall red_sandstone_wall mossy_stone_brick_wall granite_wall stone_brick_wall mud_brick_wall nether_brick_wall andesite_wall red_nether_brick_wall sandstone_wall end_stone_brick_wall diorite_wall smoker blast_furnace stonecutter warped_nylium crimson_nylium crying_obsidian respawn_anchor blackstone blackstone_stairs blackstone_wall blackstone_slab polished_blackstone polished_blackstone_bricks cracked_polished_blackstone_bricks chiseled_polished_blackstone polished_blackstone_brick_slab polished_blackstone_brick_stairs polished_blackstone_brick_wall gilded_blackstone polished_blackstone_stairs polished_blackstone_slab polished_blackstone_pressure_plate polished_blackstone_wall chiseled_nether_bricks cracked_nether_bricks quartz_bricks tuff tuff_slab tuff_stairs tuff_wall polished_tuff polished_tuff_slab polished_tuff_stairs polished_tuff_wall chiseled_tuff tuff_bricks tuff_brick_slab tuff_brick_stairs tuff_brick_wall chiseled_tuff_bricks sulfur potent_sulfur sulfur_slab sulfur_stairs sulfur_wall polished_sulfur polished_sulfur_slab polished_sulfur_stairs polished_sulfur_wall sulfur_bricks sulfur_brick_slab sulfur_brick_stairs sulfur_brick_wall chiseled_sulfur cinnabar cinnabar_slab cinnabar_stairs cinnabar_wall polished_cinnabar polished_cinnabar_slab polished_cinnabar_stairs polished_cinnabar_wall cinnabar_bricks cinnabar_brick_slab cinnabar_brick_stairs cinnabar_brick_wall chiseled_cinnabar calcite copper_ore deepslate_copper_ore dripstone_block pointed_dripstone sulfur_spike deepslate cobbled_deepslate cobbled_deepslate_stairs cobbled_deepslate_slab cobbled_deepslate_wall polished_deepslate polished_deepslate_stairs polished_deepslate_slab polished_deepslate_wall deepslate_tiles deepslate_tile_stairs deepslate_tile_slab deepslate_tile_wall deepslate_bricks deepslate_brick_stairs deepslate_brick_slab deepslate_brick_wall chiseled_deepslate cracked_deepslate_bricks cracked_deepslate_tiles smooth_basalt raw_iron_block raw_copper_block raw_gold_block reinforced_deepslate trial_spawner vault"),
    (V_BASS, "oak_planks spruce_planks birch_planks jungle_planks acacia_planks cherry_planks dark_oak_planks pale_oak_wood pale_oak_planks mangrove_planks bamboo_planks bamboo_mosaic oak_log spruce_log birch_log jungle_log acacia_log cherry_log dark_oak_log pale_oak_log mangrove_log mangrove_roots bamboo_block stripped_spruce_log stripped_birch_log stripped_jungle_log stripped_acacia_log stripped_cherry_log stripped_dark_oak_log stripped_pale_oak_log stripped_oak_log stripped_mangrove_log stripped_bamboo_block oak_wood spruce_wood birch_wood jungle_wood acacia_wood cherry_wood dark_oak_wood mangrove_wood stripped_oak_wood stripped_spruce_wood stripped_birch_wood stripped_jungle_wood stripped_acacia_wood stripped_cherry_wood stripped_dark_oak_wood stripped_pale_oak_wood stripped_mangrove_wood note_block bookshelf chiseled_bookshelf acacia_shelf bamboo_shelf birch_shelf cherry_shelf crimson_shelf dark_oak_shelf jungle_shelf mangrove_shelf oak_shelf pale_oak_shelf spruce_shelf warped_shelf oak_stairs chest crafting_table oak_sign spruce_sign birch_sign acacia_sign cherry_sign jungle_sign dark_oak_sign pale_oak_sign mangrove_sign bamboo_sign oak_door oak_wall_sign spruce_wall_sign birch_wall_sign acacia_wall_sign cherry_wall_sign jungle_wall_sign dark_oak_wall_sign pale_oak_wall_sign mangrove_wall_sign bamboo_wall_sign oak_hanging_sign spruce_hanging_sign birch_hanging_sign acacia_hanging_sign cherry_hanging_sign jungle_hanging_sign dark_oak_hanging_sign pale_oak_hanging_sign crimson_hanging_sign warped_hanging_sign mangrove_hanging_sign bamboo_hanging_sign oak_wall_hanging_sign spruce_wall_hanging_sign birch_wall_hanging_sign acacia_wall_hanging_sign cherry_wall_hanging_sign jungle_wall_hanging_sign dark_oak_wall_hanging_sign pale_oak_wall_hanging_sign mangrove_wall_hanging_sign crimson_wall_hanging_sign warped_wall_hanging_sign bamboo_wall_hanging_sign oak_pressure_plate spruce_pressure_plate birch_pressure_plate jungle_pressure_plate acacia_pressure_plate cherry_pressure_plate dark_oak_pressure_plate pale_oak_pressure_plate mangrove_pressure_plate bamboo_pressure_plate jukebox oak_fence oak_trapdoor spruce_trapdoor birch_trapdoor jungle_trapdoor acacia_trapdoor cherry_trapdoor dark_oak_trapdoor pale_oak_trapdoor mangrove_trapdoor bamboo_trapdoor brown_mushroom_block red_mushroom_block mushroom_stem oak_fence_gate spruce_stairs birch_stairs jungle_stairs trapped_chest daylight_detector acacia_stairs cherry_stairs dark_oak_stairs pale_oak_stairs mangrove_stairs bamboo_stairs bamboo_mosaic_stairs white_banner orange_banner magenta_banner light_blue_banner yellow_banner lime_banner pink_banner gray_banner light_gray_banner cyan_banner purple_banner blue_banner brown_banner green_banner red_banner black_banner white_wall_banner orange_wall_banner magenta_wall_banner light_blue_wall_banner yellow_wall_banner lime_wall_banner pink_wall_banner gray_wall_banner light_gray_wall_banner cyan_wall_banner purple_wall_banner blue_wall_banner brown_wall_banner green_wall_banner red_wall_banner black_wall_banner oak_slab spruce_slab birch_slab jungle_slab acacia_slab cherry_slab dark_oak_slab pale_oak_slab mangrove_slab bamboo_slab bamboo_mosaic_slab spruce_fence_gate birch_fence_gate jungle_fence_gate acacia_fence_gate cherry_fence_gate dark_oak_fence_gate pale_oak_fence_gate mangrove_fence_gate bamboo_fence_gate spruce_fence birch_fence jungle_fence acacia_fence cherry_fence dark_oak_fence pale_oak_fence mangrove_fence bamboo_fence spruce_door birch_door jungle_door acacia_door cherry_door dark_oak_door pale_oak_door mangrove_door bamboo_door loom barrel cartography_table fletching_table lectern smithing_table campfire soul_campfire warped_stem stripped_warped_stem warped_hyphae stripped_warped_hyphae crimson_stem stripped_crimson_stem crimson_hyphae stripped_crimson_hyphae crimson_planks warped_planks crimson_slab warped_slab crimson_pressure_plate warped_pressure_plate crimson_fence warped_fence crimson_trapdoor warped_trapdoor crimson_fence_gate warped_fence_gate crimson_stairs warped_stairs crimson_door warped_door crimson_sign warped_sign crimson_wall_sign warped_wall_sign composter bee_nest beehive"),
    (V_BELL, "gold_block"),
    (V_BIT, "emerald_block"),
    (V_CHIME, "packed_ice"),
    (V_COW_BELL, "soul_sand"),
    (V_CREEPER, "creeper_head"),
    (V_CUSTOM_HEAD, "player_head"),
    (V_DIDGERIDOO, "pumpkin"),
    (V_DRAGON, "dragon_head"),
    (V_FLUTE, "clay"),
    (V_GUITAR, "white_wool orange_wool magenta_wool light_blue_wool yellow_wool lime_wool pink_wool gray_wool light_gray_wool cyan_wool purple_wool blue_wool brown_wool green_wool red_wool black_wool"),
    (V_HAT, "glass white_stained_glass orange_stained_glass magenta_stained_glass light_blue_stained_glass yellow_stained_glass lime_stained_glass pink_stained_glass gray_stained_glass light_gray_stained_glass cyan_stained_glass purple_stained_glass blue_stained_glass brown_stained_glass green_stained_glass red_stained_glass black_stained_glass glass_pane beacon white_stained_glass_pane orange_stained_glass_pane magenta_stained_glass_pane light_blue_stained_glass_pane yellow_stained_glass_pane lime_stained_glass_pane pink_stained_glass_pane gray_stained_glass_pane light_gray_stained_glass_pane cyan_stained_glass_pane purple_stained_glass_pane blue_stained_glass_pane brown_stained_glass_pane green_stained_glass_pane red_stained_glass_pane black_stained_glass_pane sea_lantern conduit tinted_glass"),
    (V_IRON_XYLOPHONE, "iron_block"),
    (V_PIGLIN, "piglin_head"),
    (V_PLING, "glowstone"),
    (V_SKELETON, "skeleton_skull"),
    (V_SNARE, "sand suspicious_sand red_sand gravel suspicious_gravel white_concrete_powder orange_concrete_powder magenta_concrete_powder light_blue_concrete_powder yellow_concrete_powder lime_concrete_powder pink_concrete_powder gray_concrete_powder light_gray_concrete_powder cyan_concrete_powder purple_concrete_powder blue_concrete_powder brown_concrete_powder green_concrete_powder red_concrete_powder black_concrete_powder heavy_core"),
    (V_TRUMPET, "copper_block waxed_copper_block cut_copper waxed_cut_copper chiseled_copper waxed_chiseled_copper cut_copper_stairs waxed_cut_copper_stairs cut_copper_slab waxed_cut_copper_slab"),
    (V_TRUMPET_EXPOSED, "exposed_copper waxed_exposed_copper exposed_cut_copper waxed_exposed_cut_copper exposed_chiseled_copper waxed_exposed_chiseled_copper exposed_cut_copper_stairs waxed_exposed_cut_copper_stairs exposed_cut_copper_slab waxed_exposed_cut_copper_slab"),
    (V_TRUMPET_OXIDIZED, "oxidized_copper waxed_oxidized_copper oxidized_cut_copper waxed_oxidized_cut_copper oxidized_chiseled_copper waxed_oxidized_chiseled_copper oxidized_cut_copper_stairs waxed_oxidized_cut_copper_stairs oxidized_cut_copper_slab waxed_oxidized_cut_copper_slab"),
    (V_TRUMPET_WEATHERED, "weathered_copper waxed_weathered_copper weathered_cut_copper waxed_weathered_cut_copper weathered_chiseled_copper waxed_weathered_chiseled_copper weathered_cut_copper_stairs waxed_weathered_cut_copper_stairs weathered_cut_copper_slab waxed_weathered_cut_copper_slab"),
    (V_WITHER_SKELETON, "wither_skeleton_skull"),
    (V_XYLOPHONE, "bone_block"),
    (V_ZOMBIE, "zombie_head"),
];
