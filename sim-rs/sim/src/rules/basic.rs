//! Non-redstone block behaviour: ports of the vanilla 26.2 block classes' `updateShape`, `neighborChanged`,
//! `onPlace`, `affectNeighborsAfterRemoval`, `tick`, `setPlacedBy`, `playerWillDestroy`, `attack`, `useItemOn`,
//! `useWithoutItem`, `stepOn`, `entityInside` and `triggerEvent`.
//!
//! The environment these ports assume (see docs/porting-notes.md): no water or lava anywhere (waterlogged is
//! always false, the "schedule a water tick" lines are dropped), no random ticks, nothing drops, noon, the
//! overworld, the only living entity is the player (survival mode). Where a block needs something the model
//! does not have, the nearest harmless thing is done and `w.unmodelled` is bumped.
//!
//! Besides the hooks that `rules/mod.rs` chains, a few helpers are public for the other modules:
//! `menu_closed` (the player closed the screen a click opened: barrel lid, chest lid event), `keeps_block_entity`,
//! `anvil_damage`, `scaffolding_distance` / `scaffolding_can_survive`, `is_redstone_conductor`, `can_attach_to`,
//! `is_free`, `stairs_shape`, `wall_shape`, `fence_connects_to` / `bars_attach_to` / `wall_connects_to`,
//! `chest_connected_dir` / `chest_menu_parts` / `is_chest_blocked_at`, `with_props_of`.
#![allow(dead_code, unused_variables)]
use super::{Clicker, PlaceCtx, Use};
use crate::geom::*;
use crate::kinds::Kind;
use crate::props::*;
use crate::tables::*;
use crate::world::World;
use crate::{blk, in_tag, is_blk};
use std::collections::HashMap;
use std::sync::OnceLock;

const AIR: S = 0;

// ================================================================================================ helpers
#[inline]
fn survives(w: &World, s: S, pos: P) -> bool { crate::rules::can_survive(w, s, pos) }
#[inline]
fn facing(s: S) -> Dir { dir_of(val(s, P_FACING)) }
#[inline]
fn facing_opt(s: S) -> Option<Dir> { get(s, P_FACING).map(dir_of) }
#[inline]
fn set_bool(s: S, p: u8, v: bool) -> S { try_with(s, p, if v { V_TRUE } else { V_FALSE }) }
#[inline]
fn sched(w: &mut World, s: S, pos: P, delay: i32) { w.schedule_tick(pos, block_of(s), delay); }
#[inline]
fn name_of(s: S) -> &'static str { t().binfo(s).name.as_str() }
#[inline]
fn item_name(item: u16) -> &'static str { t().items.get(item as usize).map(|i| i.name.as_str()).unwrap_or("air") }
#[inline]
fn default_state(block: u16) -> S { t().blocks[block as usize].default }
fn block_named(name: &str) -> Option<u16> { t().block_by_name.get(name).copied() }

/// Block.withPropertiesOf: the default state of `block` with every property it shares with `src` copied over.
pub fn with_props_of(block: u16, src: S) -> S {
    let tb = t();
    let mut r = tb.blocks[block as usize].default;
    for pd in &tb.binfo(src).props {
        if let Some(v) = get(src, pd.id) { r = try_with(r, pd.id, v); }
    }
    r
}

/// BlockState.isFaceSturdy(level, pos, dir) of a given state (support shape, FULL).
#[inline]
fn sturdy(s: S, dir: Dir) -> bool { crate::rules::face_sturdy(s, dir, 0) }
/// Block.isFaceFull(state.getCollisionShape(), dir)
#[inline]
fn col_face_full(s: S, dir: Dir) -> bool { let tb = t(); tb.shapes[tb.col_shape[s as usize] as usize].face_support(dir, 0) }

/// Block.isExceptionForConnection
pub fn is_exception_for_connection(s: S) -> bool {
    kind_of(s) == Kind::LeavesBlock
        || is_blk!(s, "barrier")
        || is_blk!(s, "carved_pumpkin")
        || is_blk!(s, "jack_o_lantern")
        || is_blk!(s, "melon")
        || is_blk!(s, "pumpkin")
        || in_tag!(s, "shulker_boxes")
}

/// BlockState.isRedstoneConductor: `isCollisionShapeFullBlock` unless the block's properties say otherwise
/// (the `isRedstoneConductor(Blocks::never / ::always)` entries of Blocks.java, which copies inherit).
pub fn is_redstone_conductor(s: S) -> bool {
    static CACHE: OnceLock<Vec<u8>> = OnceLock::new();
    let c = CACHE.get_or_init(|| {
        let tb = t();
        tb.blocks
            .iter()
            .map(|b| {
                let n = b.name.as_str();
                if n == "soul_sand" || n == "mud" {
                    1
                } else if b.kind == Kind::LeavesBlock
                    || matches!(n, "glass" | "moving_piston" | "tnt" | "ice" | "glowstone" | "beacon" | "redstone_block" | "sea_lantern" | "chorus_flower" | "frosted_ice" | "observer" | "bamboo" | "scaffolding" | "tinted_glass" | "powder_snow" | "pointed_dripstone" | "sulfur_spike" | "piston" | "sticky_piston")
                    || n.ends_with("stained_glass")
                    || n.ends_with("copper_grate")
                    || n.ends_with("copper_bulb")
                {
                    2
                } else {
                    0
                }
            })
            .collect()
    });
    match c[block_of(s) as usize] { 1 => true, 2 => false, _ => t().full_cube[s as usize] }
}

/// BlockState.shouldChangedStateKeepBlockEntity for the non-redstone blocks (for `rules::keeps_block_entity`):
/// a copper chest or copper golem statue that turns into another one of its family keeps its contents.
pub fn keeps_block_entity(old: S, new: S) -> bool {
    in_tag!(new, "copper_chests") && in_tag!(old, "copper_chests") || in_tag!(new, "copper_golem_statues") && in_tag!(old, "copper_golem_statues")
}

/// MultifaceBlock.canAttachTo(level, directionTowardsNeighbour, neighbourPos, neighbourState)
pub fn can_attach_to(dir: Dir, ns: S) -> bool { sturdy(ns, opposite(dir)) || col_face_full(ns, opposite(dir)) }

/// FallingBlock.isFree
pub fn is_free(s: S) -> bool {
    let tb = t();
    tb.air[s as usize] || in_tag!(s, "fire") || kind_of(s) == Kind::LiquidBlock || tb.replaceable[s as usize]
}

/// AnvilBlock.damage: the next stage of a falling anvil that takes damage on landing (None: it breaks). For the
/// falling-block integrator; the other `Fallable.onLand` overrides change nothing here (concrete powder only
/// hardens next to water).
pub fn anvil_damage(s: S) -> Option<S> {
    let next = if is_blk!(s, "anvil") { blk!("chipped_anvil") } else if is_blk!(s, "chipped_anvil") { blk!("damaged_anvil") } else { return None };
    let d = default_state(next);
    Some(match get(s, P_FACING) { Some(f) => try_with(d, P_FACING, f), None => d })
}

/// Is the rectangle [x0,x1]x[z0,z1] (block-local) inside the DOWN face of the shape (`getFaceShape(DOWN)`)?
/// This is WallBlock.isCovered(aboveShape, testShape) for a test shape that spans the full height.
fn down_face_covers(shape_idx: u32, x0: f64, x1: f64, z0: f64, z1: f64) -> bool {
    let sh = &t().shapes[shape_idx as usize];
    if sh.empty { return false; }
    if sh.full { return true; }
    const EPS: f64 = 1.0e-7;
    let cy = &sh.coords[1];
    // VoxelShape.calculateFace: a shape that is one full-height layer is its own face; else the slice at y = 1e-7
    let yi: usize = if cy.len() == 2 && cy[0].abs() < EPS && (cy[1] - 1.0).abs() < EPS {
        0
    } else {
        let mut n = 0usize;
        for c in cy { if 1.0e-7 < *c { break; } n += 1; }
        if n == 0 || n - 1 >= sh.size[1] { return false; }
        n - 1
    };
    let cell = |a: usize, v: f64| -> Option<usize> {
        let c = &sh.coords[a];
        let mut n = 0usize;
        for x in c { if v < *x { break; } n += 1; }
        if n == 0 || n - 1 >= sh.size[a] { None } else { Some(n - 1) }
    };
    let grid = |a: usize, lo: f64, hi: f64| -> Vec<f64> {
        let mut g: Vec<f64> = sh.coords[a].iter().cloned().filter(|v| *v > lo + EPS && *v < hi - EPS).collect();
        g.push(lo);
        g.push(hi);
        g.sort_by(|p, q| p.partial_cmp(q).unwrap());
        g
    };
    let gx = grid(0, x0, x1);
    let gz = grid(2, z0, z1);
    for ix in 0..gx.len() - 1 {
        let mx = (gx[ix] + gx[ix + 1]) / 2.0;
        for iz in 0..gz.len() - 1 {
            let mz = (gz[iz] + gz[iz + 1]) / 2.0;
            match (cell(0, mx), cell(2, mz)) {
                (Some(cx), Some(cz)) => {
                    if !sh.filled[(cx * sh.size[1] + yi) * sh.size[2] + cz] { return false; }
                }
                _ => return false,
            }
        }
    }
    true
}

// ================================================================================================ fences, bars, walls, gates
/// FenceGateBlock.connectsToDirection
fn gate_connects_to_direction(s: S, dir: Dir) -> bool {
    match facing_opt(s) { Some(f) if is_horizontal(f) && is_horizontal(dir) => axis(f) == axis(clockwise(dir)), _ => false }
}
/// FenceBlock.connectsTo
pub fn fence_connects_to(me: S, ns: S, face_solid: bool, dir: Dir) -> bool {
    let same_fence = in_tag!(ns, "fences") && in_tag!(ns, "wooden_fences") == in_tag!(me, "wooden_fences");
    let gate = kind_of(ns) == Kind::FenceGateBlock && gate_connects_to_direction(ns, dir);
    !is_exception_for_connection(ns) && face_solid || same_fence || gate
}
/// IronBarsBlock.attachsTo
pub fn bars_attach_to(ns: S, face_solid: bool) -> bool {
    !is_exception_for_connection(ns) && face_solid || kind_of(ns) == Kind::IronBarsBlock || in_tag!(ns, "walls")
}
/// WallBlock.connectsTo
pub fn wall_connects_to(ns: S, face_solid: bool, dir: Dir) -> bool {
    let gate = kind_of(ns) == Kind::FenceGateBlock && gate_connects_to_direction(ns, dir);
    in_tag!(ns, "walls") || !is_exception_for_connection(ns) && face_solid || kind_of(ns) == Kind::IronBarsBlock || gate
}
#[inline]
fn wall_side_connected(s: S, d: Dir) -> bool { get(s, dir_prop(d)).map_or(false, |v| v != V_NONE) }

/// WallBlock.updateShape(level, state, topPos, topNeighbour, north, east, south, west): the sides (low/tall by
/// what covers them from above) and the post.
pub fn wall_shape(s: S, top: S, north: bool, east: bool, south: bool, west: bool) -> S {
    let above = t().col_shape[top as usize];
    // WallBlock.TEST_SHAPES_WALL = rotateHorizontal(boxZ(2, 16, 0, 9)); TEST_SHAPE_POST = column(2, 0, 16)
    let (a, b, c) = (7.0 / 16.0, 9.0 / 16.0, 1.0);
    let side = |conn: bool, x0: f64, x1: f64, z0: f64, z1: f64| -> u8 {
        if conn { if down_face_covers(above, x0, x1, z0, z1) { V_TALL } else { V_LOW } } else { V_NONE }
    };
    let n = side(north, a, b, 0.0, b);
    let e = side(east, a, c, a, b);
    let so = side(south, a, b, a, c);
    let we = side(west, 0.0, b, a, b);
    let mut r = s;
    r = with(r, P_NORTH, n);
    r = with(r, P_EAST, e);
    r = with(r, P_SOUTH, so);
    r = with(r, P_WEST, we);
    // shouldRaisePost
    let top_has_post = kind_of(top) == Kind::WallBlock && is_true(top, P_UP);
    let up = if top_has_post {
        true
    } else {
        let (nn, sn, en, wn) = (n == V_NONE, so == V_NONE, e == V_NONE, we == V_NONE);
        let has_corner = nn && sn && wn && en || nn != sn || wn != en;
        if has_corner {
            true
        } else {
            let has_high = n == V_TALL && so == V_TALL || e == V_TALL && we == V_TALL;
            if has_high { false } else { in_tag!(top, "wall_post_override") || down_face_covers(above, a, b, a, b) }
        }
    };
    with_bool(r, P_UP, up)
}

fn wall_update(w: &mut World, s: S, pos: P, dir: Dir, npos: P, ns: S) -> S {
    if dir == DOWN { return s; }
    if dir == UP {
        // topUpdate
        return wall_shape(s, ns, wall_side_connected(s, NORTH), wall_side_connected(s, EAST), wall_side_connected(s, SOUTH), wall_side_connected(s, WEST));
    }
    // sideUpdate
    let opp = opposite(dir);
    let c = |d: Dir| -> bool { if dir == d { wall_connects_to(ns, sturdy(ns, opp), opp) } else { wall_side_connected(s, d) } };
    let (n, e, so, we) = (c(NORTH), c(EAST), c(SOUTH), c(WEST));
    let above = w.get(pos.above());
    wall_shape(s, above, n, e, so, we)
}

// ================================================================================================ stairs
#[inline]
fn is_stairs(s: S) -> bool { kind_of(s) == Kind::StairBlock }
fn stairs_can_take_shape(w: &World, s: S, pos: P, neighbour: Dir) -> bool {
    let n = w.get(pos.rel(neighbour));
    !is_stairs(n) || get(n, P_FACING) != get(s, P_FACING) || get(n, P_HALF) != get(s, P_HALF)
}
/// StairBlock.getStairsShape
pub fn stairs_shape(w: &World, s: S, pos: P) -> u8 {
    let f = facing(s);
    let behind = w.get(pos.rel(f));
    if is_stairs(behind) && get(s, P_HALF) == get(behind, P_HALF) {
        let bf = facing(behind);
        if axis(bf) != axis(f) && stairs_can_take_shape(w, s, pos, opposite(bf)) {
            return if bf == counter_clockwise(f) { V_OUTER_LEFT } else { V_OUTER_RIGHT };
        }
    }
    let front = w.get(pos.rel(opposite(f)));
    if is_stairs(front) && get(s, P_HALF) == get(front, P_HALF) {
        let ff = facing(front);
        if axis(ff) != axis(f) && stairs_can_take_shape(w, s, pos, ff) {
            return if ff == counter_clockwise(f) { V_INNER_LEFT } else { V_INNER_RIGHT };
        }
    }
    V_STRAIGHT
}

// ================================================================================================ chests
#[inline]
fn is_copper_chest(s: S) -> bool { in_tag!(s, "copper_chests") }
/// ChestBlock.chestCanConnectTo (CopperChestBlock: any copper chest)
fn chest_can_connect_to(me: S, ns: S) -> bool {
    if is_copper_chest(me) { is_copper_chest(ns) && has(ns, P_TYPE) } else { same_block(ns, me) }
}
/// ChestBlock.getConnectedDirection
pub fn chest_connected_dir(s: S) -> Dir {
    let f = facing(s);
    if get(s, P_TYPE) == Some(V_LEFT) { clockwise(f) } else { counter_clockwise(f) }
}
fn chest_update(s: S, dir: Dir, ns: S) -> S {
    if chest_can_connect_to(s, ns) && is_horizontal(dir) {
        let nt = val(ns, P_TYPE);
        if val(s, P_TYPE) == V_SINGLE && nt != V_SINGLE && get(s, P_FACING) == get(ns, P_FACING) && chest_connected_dir(ns) == opposite(dir) {
            // ChestType.getOpposite
            return with(s, P_TYPE, if nt == V_LEFT { V_RIGHT } else { V_LEFT });
        }
    } else if chest_connected_dir(s) == dir {
        return with(s, P_TYPE, V_SINGLE);
    }
    s
}
/// ChestBlock.isChestBlockedAt (no cats)
pub fn is_chest_blocked_at(w: &World, pos: P) -> bool { is_redstone_conductor(w.get(pos.above())) }
/// What ChestBlock.getMenuProvider combines (DoubleBlockCombiner.combineWithNeigbour): None when blocked,
/// else the chest positions in container order (first = the `type=right` half).
pub fn chest_menu_parts(w: &World, s: S, pos: P) -> Option<Vec<P>> {
    if is_chest_blocked_at(w, pos) { return None; }
    let ty = val(s, P_TYPE);
    if ty == V_SINGLE { return Some(vec![pos]); }
    let npos = pos.rel(chest_connected_dir(s));
    let ns = w.get(npos);
    if same_block(ns, s) {
        let nt = val(ns, P_TYPE);
        if nt != V_SINGLE && nt != ty && get(ns, P_FACING) == get(s, P_FACING) {
            if is_chest_blocked_at(w, npos) { return None; }
            // BlockType.FIRST is the RIGHT half
            return Some(if ty == V_RIGHT { vec![pos, npos] } else { vec![npos, pos] });
        }
    }
    Some(vec![pos])
}

// ================================================================================================ double blocks
/// DoublePlantBlock.updateShape (super = VegetationBlock.updateShape)
fn double_plant_update(w: &World, s: S, pos: P, dir: Dir, ns: S) -> S {
    let half = val(s, P_HALF);
    if axis(dir) != AXIS_Y || (half == V_LOWER) != (dir == UP) || same_block(ns, s) && get(ns, P_HALF) != Some(half) {
        if half == V_LOWER && dir == DOWN && !survives(w, s, pos) {
            AIR
        } else if !survives(w, s, pos) {
            AIR
        } else {
            s
        }
    } else {
        AIR
    }
}

// ================================================================================================ scaffolding
/// ScaffoldingBlock.getDistance
pub fn scaffolding_distance(w: &World, pos: P) -> u8 {
    let below = w.get(pos.below());
    let mut distance = 7u8;
    if is_blk!(below, "scaffolding") {
        distance = val(below, P_DISTANCE);
    } else if sturdy(below, UP) {
        return 0;
    }
    for d in HORIZONTAL {
        let rs = w.get(pos.rel(d));
        if is_blk!(rs, "scaffolding") {
            distance = distance.min(val(rs, P_DISTANCE) + 1);
            if distance == 1 { break; }
        }
    }
    distance
}
/// ScaffoldingBlock.canSurvive
pub fn scaffolding_can_survive(w: &World, pos: P) -> bool { scaffolding_distance(w, pos) < 7 }

// ================================================================================================ leaves
/// LeavesBlock.getDistanceAt
fn leaves_distance_at(s: S) -> u8 {
    if in_tag!(s, "prevents_nearby_leaf_decay") {
        0
    } else if kind_of(s) == Kind::LeavesBlock {
        // (LeavesBlock.DISTANCE; scaffolding's `distance` is a different property)
        get(s, P_DISTANCE).unwrap_or(7)
    } else {
        7
    }
}
/// LeavesBlock.updateDistance
fn leaves_update_distance(w: &World, s: S, pos: P) -> S {
    let mut nd = 7u8;
    for d in DIRS {
        nd = nd.min(leaves_distance_at(w.get(pos.rel(d))) + 1);
        if nd == 1 { break; }
    }
    try_with(s, P_DISTANCE, nd)
}

// ================================================================================================ multiface, vines, moss carpet
fn multiface_has_any_face(s: S) -> bool { DIRS.iter().any(|d| is_true(s, dir_prop(*d))) }
fn multiface_update(s: S, dir: Dir, ns: S) -> S {
    if !multiface_has_any_face(s) {
        AIR
    } else if is_true(s, dir_prop(dir)) && !can_attach_to(dir, ns) {
        // removeFace
        let r = set_bool(s, dir_prop(dir), false);
        if multiface_has_any_face(r) { r } else { AIR }
    } else {
        s
    }
}

/// VineBlock.isAcceptableNeighbour(level, neighbourPos, directionToNeighbour)
fn vine_acceptable(w: &World, npos: P, dir: Dir) -> bool { can_attach_to(dir, w.get(npos)) }
/// VineBlock.canSupportAtFace
fn vine_can_support_at_face(w: &World, s: S, pos: P, dir: Dir) -> bool {
    if dir == DOWN { return false; }
    if vine_acceptable(w, pos.rel(dir), dir) { return true; }
    if axis(dir) == AXIS_Y { return false; }
    let above = w.get(pos.above());
    same_block(above, s) && is_true(above, dir_prop(dir))
}
/// VineBlock.getUpdatedState
fn vine_updated_state(w: &World, s: S, pos: P) -> S {
    let mut st = s;
    let above_pos = pos.above();
    if is_true(st, P_UP) {
        // (vanilla passes Direction.DOWN here: the top face of the block above is what is tested)
        st = set_bool(st, P_UP, vine_acceptable(w, above_pos, DOWN));
    }
    for d in HORIZONTAL {
        let prop = dir_prop(d);
        if is_true(st, prop) {
            let mut can = vine_can_support_at_face(w, s, pos, d);
            if !can {
                let above = w.get(above_pos);
                can = same_block(above, s) && is_true(above, prop);
            }
            st = set_bool(st, prop, can);
        }
    }
    st
}
fn vine_has_faces(s: S) -> bool { [UP, NORTH, SOUTH, WEST, EAST].iter().any(|d| is_true(s, dir_prop(*d))) }

fn mossy_has_faces(s: S) -> bool {
    is_true(s, P_BOTTOM) || HORIZONTAL.iter().any(|d| get(s, dir_prop(*d)).map_or(false, |v| v != V_NONE))
}
/// MossyCarpetBlock.getUpdatedState
fn mossy_updated_state(w: &World, s: S, pos: P, create_sides: bool) -> S {
    let mut st = s;
    let base = is_true(s, P_BOTTOM);
    let create_sides = create_sides | base;
    let mut above: Option<S> = None;
    let mut below: Option<S> = None;
    for d in HORIZONTAL {
        let prop = dir_prop(d);
        // canSupportAtFace: never upwards; MultifaceBlock.canAttachTo(level, pos, direction)
        let can = can_attach_to(d, w.get(pos.rel(d)));
        let mut side = if can { if create_sides { V_LOW } else { val(st, prop) } } else { V_NONE };
        if side == V_LOW {
            let a = *above.get_or_insert_with(|| w.get(pos.above()));
            if is_blk!(a, "pale_moss_carpet") && get(a, prop) != Some(V_NONE) && !is_true(a, P_BOTTOM) { side = V_TALL; }
            if !is_true(st, P_BOTTOM) {
                let b = *below.get_or_insert_with(|| w.get(pos.below()));
                if is_blk!(b, "pale_moss_carpet") && get(b, prop) == Some(V_NONE) { side = V_NONE; }
            }
        }
        st = with(st, prop, side);
    }
    st
}
/// MossyCarpetBlock.createTopperWithSideChance (each side survives with the given coin flips)
fn mossy_create_topper(w: &mut World, pos: P) -> S {
    let above_pos = pos.above();
    let prev = w.get(above_pos);
    let is_moss_above = is_blk!(prev, "pale_moss_carpet");
    if (!is_moss_above || !is_true(prev, P_BOTTOM)) && (is_moss_above || t().replaceable[prev as usize]) {
        let no_base = with_bool(default_state(blk!("pale_moss_carpet")), P_BOTTOM, false);
        let mut st = mossy_updated_state(w, no_base, above_pos, true);
        for d in HORIZONTAL {
            let prop = dir_prop(d);
            if val(st, prop) != V_NONE && w.rand_u32() & 1 == 0 { st = with(st, prop, V_NONE); }
        }
        if mossy_has_faces(st) && st != prev { st } else { AIR }
    } else {
        AIR
    }
}

// ================================================================================================ growing plants (vines that grow from a head)
struct Growing { dir: Dir, head: u16, body: u16, is_head: bool }
fn growing(s: S) -> Option<Growing> {
    use Kind::*;
    let (dir, head, body, is_head) = match kind_of(s) {
        CaveVinesBlock => (DOWN, blk!("cave_vines"), blk!("cave_vines_plant"), true),
        CaveVinesPlantBlock => (DOWN, blk!("cave_vines"), blk!("cave_vines_plant"), false),
        KelpBlock => (UP, blk!("kelp"), blk!("kelp_plant"), true),
        KelpPlantBlock => (UP, blk!("kelp"), blk!("kelp_plant"), false),
        TwistingVinesBlock => (UP, blk!("twisting_vines"), blk!("twisting_vines_plant"), true),
        TwistingVinesPlantBlock => (UP, blk!("twisting_vines"), blk!("twisting_vines_plant"), false),
        WeepingVinesBlock => (DOWN, blk!("weeping_vines"), blk!("weeping_vines_plant"), true),
        WeepingVinesPlantBlock => (DOWN, blk!("weeping_vines"), blk!("weeping_vines_plant"), false),
        _ => return None,
    };
    Some(Growing { dir, head, body, is_head })
}
fn growing_update(w: &mut World, s: S, pos: P, dir: Dir, ns: S, g: Growing) -> S {
    let nb = block_of(ns);
    // CaveVines keep their berries across the conversion (updateBodyAfterConvertedFromHead and the reverse)
    let copy_berries = |from: S, to: S| -> S { match get(from, P_BERRIES) { Some(v) => try_with(to, P_BERRIES, v), None => to } };
    if g.is_head {
        // GrowingPlantHeadBlock.updateShape
        if dir == opposite(g.dir) {
            if !survives(w, s, pos) {
                sched(w, s, pos, 1);
            } else {
                let n = block_of(w.get(pos.rel(g.dir)));
                if n == g.head || n == g.body { return copy_berries(s, default_state(g.body)); }
            }
        }
        if dir != g.dir || nb != g.head && nb != g.body { s } else { copy_berries(s, default_state(g.body)) }
    } else {
        // GrowingPlantBodyBlock.updateShape
        if dir == opposite(g.dir) && !survives(w, s, pos) { sched(w, s, pos, 1); }
        if dir == g.dir && nb != g.body && nb != g.head {
            // headBlock.getStateForPlacement(random): a random age (the server's own random number is not known here)
            let age = w.rand_int(25) as u8;
            return copy_berries(s, try_with(default_state(g.head), P_AGE, age));
        }
        s
    }
}

// ================================================================================================ speleothems (pointed dripstone, sulfur spike)
fn is_speleothem_with_direction(s: S, tip: Dir) -> bool { in_tag!(s, "speleothems") && get(s, P_VERTICAL_DIRECTION) == Some(dir_val(tip)) }
/// SpeleothemBlock.calculateSpeleothemThickness
fn speleothem_thickness(w: &World, s: S, pos: P, tip: Dir, merge: bool) -> u8 {
    let base = opposite(tip);
    let front = w.get(pos.rel(tip));
    if is_speleothem_with_direction(front, base) && same_block(front, s) {
        if !merge && get(front, P_THICKNESS) != Some(V_TIP_MERGE) { V_TIP } else { V_TIP_MERGE }
    } else if !is_speleothem_with_direction(front, tip) {
        V_TIP
    } else {
        let ft = val(front, P_THICKNESS);
        if ft != V_TIP && ft != V_TIP_MERGE {
            let behind = w.get(pos.rel(base));
            if !is_speleothem_with_direction(behind, tip) { V_BASE } else { V_MIDDLE }
        } else {
            V_FRUSTUM
        }
    }
}
fn speleothem_update(w: &mut World, s: S, pos: P, dir: Dir) -> S {
    if dir != UP && dir != DOWN { return s; }
    let tip = dir_of(val(s, P_VERTICAL_DIRECTION));
    if tip == DOWN && w.has_scheduled_tick(pos, block_of(s)) { return s; }
    if dir == opposite(tip) && !survives(w, s, pos) {
        sched(w, s, pos, if tip == DOWN { 2 } else { 1 });
        s
    } else {
        let merge = val(s, P_THICKNESS) == V_TIP_MERGE;
        with(s, P_THICKNESS, speleothem_thickness(w, s, pos, tip, merge))
    }
}

// ================================================================================================ corals
#[inline]
fn is_dead_coral(s: S) -> bool { name_of(s).starts_with("dead_") }
/// BaseCoralPlantTypeBlock.tryScheduleDieTick / CoralBlock: there is no water, so a living coral always schedules its death
fn coral_schedule_die(w: &mut World, s: S, pos: P) {
    // 60 + random.nextInt(40): the game draws this from Level.random, which is seeded from the clock, so the Java
    // runs themselves differ here. MCSIM_CORAL=a,b,... forces the successive draws (to explore what they change).
    thread_local! { static FORCED: std::cell::RefCell<(Vec<u32>, usize)> = std::cell::RefCell::new((std::env::var("MCSIM_CORAL").ok().map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default(), 0)); }
    let forced = FORCED.with(|f| { let mut f = f.borrow_mut(); if f.0.is_empty() { None } else { let k = f.1 % f.0.len(); f.1 += 1; Some(f.0[k]) } });
    let d = 60 + forced.unwrap_or_else(|| w.rand_int(40)) as i32;
    sched(w, s, pos, d);
}
fn coral_dead_state(s: S) -> Option<S> {
    let b = block_named(&format!("dead_{}", name_of(s)))?;
    let mut d = try_with(default_state(b), P_WATERLOGGED, V_FALSE);
    if let Some(f) = get(s, P_FACING) { d = try_with(d, P_FACING, f); }
    Some(d)
}

// ================================================================================================ updateShape
/// BlockBehaviour.updateShape for everything that is not redstone. `dir` points from `pos` to the neighbour.
pub fn update_shape(w: &mut World, s: S, pos: P, dir: Dir, npos: P, ns: S) -> S {
    use Kind::*;
    match kind_of(s) {
        // ---- connecting blocks
        FenceBlock => {
            if is_horizontal(dir) { with_bool(s, dir_prop(dir), fence_connects_to(s, ns, sturdy(ns, opposite(dir)), opposite(dir))) } else { s }
        }
        IronBarsBlock => {
            if is_horizontal(dir) { with_bool(s, dir_prop(dir), bars_attach_to(ns, sturdy(ns, opposite(dir)))) } else { s }
        }
        WallBlock => wall_update(w, s, pos, dir, npos, ns),
        FenceGateBlock => {
            let f = facing(s);
            if axis(clockwise(f)) != axis(dir) { return s; }
            let in_wall = in_tag!(ns, "walls") || in_tag!(w.get(pos.rel(opposite(dir))), "walls");
            with_bool(s, P_IN_WALL, in_wall)
        }
        StairBlock => {
            if is_horizontal(dir) { with(s, P_SHAPE, stairs_shape(w, s, pos)) } else { s }
        }
        HugeMushroomBlock => {
            if same_block(ns, s) { with_bool(s, dir_prop(dir), false) } else { s }
        }
        ChorusPlantBlock => {
            if !survives(w, s, pos) {
                sched(w, s, pos, 1);
                s
            } else {
                let connect = same_block(ns, s) || is_blk!(ns, "chorus_flower") || dir == DOWN && in_tag!(ns, "supports_chorus_plant");
                with_bool(s, dir_prop(dir), connect)
            }
        }
        ChorusFlowerBlock => {
            if dir != UP && !survives(w, s, pos) { sched(w, s, pos, 1); }
            s
        }
        SnowyBlock => {
            if dir == UP { with_bool(s, P_SNOWY, in_tag!(ns, "snow")) } else { s }
        }
        CampfireBlock => {
            if dir == DOWN { with_bool(s, P_SIGNAL_FIRE, is_blk!(ns, "hay_block")) } else { s }
        }
        // ---- two-block things
        DoorBlock => {
            let half = val(s, P_HALF);
            if axis(dir) != AXIS_Y || (half == V_LOWER) != (dir == UP) {
                if half == V_LOWER && dir == DOWN && !survives(w, s, pos) { AIR } else { s }
            } else if kind_of(ns) == DoorBlock && get(ns, P_HALF) != Some(half) {
                with(ns, P_HALF, half)
            } else {
                AIR
            }
        }
        BedBlock => {
            let part = val(s, P_PART);
            let f = facing(s);
            // getNeighbourDirection
            let nd = if part == V_FOOT { f } else { opposite(f) };
            if dir == nd {
                if same_block(ns, s) && get(ns, P_PART) != Some(part) { with(s, P_OCCUPIED, val(ns, P_OCCUPIED)) } else { AIR }
            } else {
                s
            }
        }
        DoublePlantBlock | TallFlowerBlock | TallSeagrassBlock | SmallDripleafBlock => double_plant_update(w, s, pos, dir, ns),
        PitcherCropBlock => {
            if get(s, P_AGE).unwrap_or(0) >= 3 {
                double_plant_update(w, s, pos, dir, ns)
            } else if survives(w, s, pos) {
                s
            } else {
                AIR
            }
        }
        ChestBlock => {
            let r = chest_update(s, dir, ns);
            if is_copper_chest(s) && chest_can_connect_to(s, ns) {
                // CopperChestBlock: a joined pair takes the neighbour's block (oxidation level)
                if val(r, P_TYPE) != V_SINGLE && chest_connected_dir(r) == dir { return with_props_of(block_of(ns), r); }
            }
            r
        }
        // ---- things that break when their support goes
        TorchBlock | StandingSignBlock | BannerBlock | CakeBlock | CandleCakeBlock | FlowerPotBlock => {
            if dir == DOWN && !survives(w, s, pos) { AIR } else { s }
        }
        CoralPlantBlock | CoralFanBlock => {
            if dir == DOWN && !survives(w, s, pos) { return AIR; }
            if !is_dead_coral(s) { coral_schedule_die(w, s, pos); }
            s
        }
        CoralWallFanBlock => {
            if opposite(dir) == facing(s) && !survives(w, s, pos) { return AIR; }
            if !is_dead_coral(s) { coral_schedule_die(w, s, pos); }
            s
        }
        CoralBlock => {
            coral_schedule_die(w, s, pos);
            s
        }
        WallTorchBlock | LadderBlock | WallSignBlock | WallBannerBlock | AmethystClusterBlock => {
            if opposite(dir) == facing(s) && !survives(w, s, pos) { AIR } else { s }
        }
        CeilingHangingSignBlock | HangingRootsBlock | SporeBlossomBlock => {
            if dir == UP && !survives(w, s, pos) { AIR } else { s }
        }
        WallHangingSignBlock => {
            if axis(dir) == axis(clockwise(facing(s))) && !survives(w, s, pos) { AIR } else { s }
        }
        LanternBlock => {
            // getConnectedDirection(state).getOpposite() == direction
            let connected = if is_true(s, P_HANGING) { DOWN } else { UP };
            if opposite(connected) == dir && !survives(w, s, pos) { AIR } else { s }
        }
        GrindstoneBlock => {
            // FaceAttachedHorizontalDirectionalBlock
            let connected = match get(s, P_FACE) { Some(V_CEILING) => DOWN, Some(V_FLOOR) => UP, _ => facing(s) };
            if opposite(connected) == dir && !survives(w, s, pos) { AIR } else { s }
        }
        CocoaBlock => {
            if dir == facing(s) && !survives(w, s, pos) { AIR } else { s }
        }
        // VegetationBlock and the others that simply ask canSurvive on every update
        BushBlock | FlowerBlock | TallGrassBlock | SaplingBlock | MushroomBlock | NetherFungusBlock | NetherRootsBlock | NetherSproutsBlock
        | NetherWartBlock | CropBlock | TorchflowerCropBlock | StemBlock | AzaleaBlock | CactusFlowerBlock | DryVegetationBlock | FireflyBushBlock
        | FlowerBedBlock | LeafLitterBlock | LilyPadBlock | SweetBerryBushBlock | SeagrassBlock | SeaPickleBlock | MangrovePropaguleBlock
        | CarpetBlock | SnowLayerBlock | FrogspawnBlock => {
            if !survives(w, s, pos) { AIR } else { s }
        }
        AttachedStemBlock => {
            let (fruit, stem) = if is_blk!(s, "attached_melon_stem") { (blk!("melon"), blk!("melon_stem")) } else { (blk!("pumpkin"), blk!("pumpkin_stem")) };
            if block_of(ns) != fruit && dir == facing(s) { return try_with(default_state(stem), P_AGE, 7); }
            if !survives(w, s, pos) { AIR } else { s }
        }
        MossyCarpetBlock => {
            if !survives(w, s, pos) { return AIR; }
            let bs = mossy_updated_state(w, s, pos, false);
            if !mossy_has_faces(bs) { AIR } else { bs }
        }
        FireBlock => {
            // FireBlock.getStateWithAge (which sides burn) is not modelled: the state is kept
            if survives(w, s, pos) { s } else { AIR }
        }
        // ---- things that schedule a tick to break or fall
        SugarCaneBlock | CactusBlock => {
            if !survives(w, s, pos) { sched(w, s, pos, 1); }
            s
        }
        BambooStalkBlock => {
            if !survives(w, s, pos) { sched(w, s, pos, 1); }
            if dir == UP && is_blk!(ns, "bamboo") && val(ns, P_AGE) > val(s, P_AGE) {
                // state.cycle(AGE)
                with(s, P_AGE, if val(s, P_AGE) == 0 { 1 } else { 0 })
            } else {
                s
            }
        }
        BambooSaplingBlock => {
            if !survives(w, s, pos) {
                AIR
            } else if dir == UP && is_blk!(ns, "bamboo") {
                default_state(blk!("bamboo"))
            } else {
                s
            }
        }
        ScaffoldingBlock => {
            sched(w, s, pos, 1);
            s
        }
        // FallingBlock.getDelayAfterPlace: 2, dragon egg 5. Concrete powder: touchesLiquid is never true without water.
        FallingBlock | ConcretePowderBlock | AnvilBlock | BrushableBlock => {
            sched(w, s, pos, 2);
            s
        }
        DragonEggBlock => {
            sched(w, s, pos, 5);
            s
        }
        LeavesBlock => {
            let d = leaves_distance_at(ns) + 1;
            if d != 1 || get(s, P_DISTANCE) != Some(d) { sched(w, s, pos, 1); }
            s
        }
        FarmlandBlock | DirtPathBlock => {
            if dir == UP && !survives(w, s, pos) { sched(w, s, pos, 1); }
            s
        }
        BigDripleafBlock => {
            if dir == DOWN && !survives(w, s, pos) { return AIR; }
            if dir == UP && same_block(ns, s) { with_props_of(blk!("big_dripleaf_stem"), s) } else { s }
        }
        BigDripleafStemBlock => {
            if (dir == DOWN || dir == UP) && !survives(w, s, pos) { sched(w, s, pos, 1); }
            s
        }
        HangingMossBlock => {
            if !hanging_moss_can_stay(w, s, pos) { sched(w, s, pos, 1); }
            with_bool(s, P_TIP, !same_block(w.get(pos.below()), s))
        }
        PointedDripstoneBlock | SulfurSpikeBlock => speleothem_update(w, s, pos, dir),
        CreakingHeartBlock => {
            sched(w, s, pos, 1);
            s
        }
        CaveVinesBlock | CaveVinesPlantBlock | KelpBlock | KelpPlantBlock | TwistingVinesBlock | TwistingVinesPlantBlock | WeepingVinesBlock
        | WeepingVinesPlantBlock => match growing(s) {
            Some(g) => growing_update(w, s, pos, dir, ns, g),
            None => s,
        },
        // ---- faces
        MultifaceBlock | GlowLichenBlock | SculkVeinBlock => multiface_update(s, dir, ns),
        VineBlock => {
            if dir == DOWN { return s; }
            let bs = vine_updated_state(w, s, pos);
            if !vine_has_faces(bs) { AIR } else { bs }
        }
        // PotentSulfurBlock.validBlockState: no water above -> dry
        PotentSulfurBlock => try_with(s, P_POTENT_SULFUR_STATE, V_DRY),
        // NetherPortalBlock (portal frames), BubbleColumnBlock, LiquidBlock: not present in this world
        _ => s,
    }
}

/// HangingMossBlock.canStayAtPosition
fn hanging_moss_can_stay(w: &World, s: S, pos: P) -> bool {
    let above = w.get(pos.above());
    can_attach_to(UP, above) || same_block(above, s)
}

// ================================================================================================ shelves (SideChainPartBlock)
fn shelf_connectable(s: S) -> bool { in_tag!(s, "wooden_shelves") && is_true(s, P_POWERED) }
/// SideChainPartBlock.Neighbors.createNewNeighbor: the part of a connectable neighbour facing the same way
fn shelf_neighbor_part(w: &World, pos: P, f: Dir) -> Option<u8> {
    let n = w.get(pos);
    if shelf_connectable(n) && facing_opt(n) == Some(f) { get(n, P_SIDE_CHAIN) } else { None }
}
fn shelf_set_part(w: &mut World, pos: P, part: u8) {
    let st = w.get(pos);
    if has(st, P_SIDE_CHAIN) && get(st, P_SIDE_CHAIN) != Some(part) { w.set_block(pos, with(st, P_SIDE_CHAIN, part), 3); }
}
fn part_when_connected_right(p: u8) -> u8 { if p == V_UNCONNECTED || p == V_LEFT { V_LEFT } else { V_CENTER } }
fn part_when_connected_left(p: u8) -> u8 { if p == V_UNCONNECTED || p == V_RIGHT { V_RIGHT } else { V_CENTER } }
fn part_when_disconnected_right(p: u8) -> u8 { if p == V_UNCONNECTED || p == V_LEFT { V_UNCONNECTED } else { V_RIGHT } }
fn part_when_disconnected_left(p: u8) -> u8 { if p == V_UNCONNECTED || p == V_RIGHT { V_UNCONNECTED } else { V_LEFT } }
/// SideChainPartBlock.getAllBlocksConnectedTo (max chain length 3)
fn shelf_all_connected(w: &World, pos: P) -> Vec<P> {
    let st = w.get(pos);
    if !shelf_connectable(st) { return vec![]; }
    let f = facing(st);
    let mut res = std::collections::VecDeque::new();
    res.push_back(pos);
    for (d, end, front) in [(clockwise(f), V_LEFT, true), (counter_clockwise(f), V_RIGHT, false)] {
        for steps in 1..3 {
            let np = pos.rel_n(d, steps);
            let part = shelf_neighbor_part(w, np, f);
            // connectsTowards(endPart): CENTER or the end part itself
            if let Some(p) = part { if p == V_CENTER || p == end { if front { res.push_front(np); } else { res.push_back(np); } } }
            // isUnconnectableOrChainEnd
            if part.map_or(true, |p| p != V_CENTER) { break; }
        }
    }
    res.into_iter().collect()
}
/// SideChainPartBlock.updateNeighborsAfterPoweringDown
fn shelf_neighbors_after_powering_down(w: &mut World, pos: P, s: S) {
    let Some(f) = facing_opt(s) else { return };
    let (lp, rp) = (pos.rel(clockwise(f)), pos.rel(counter_clockwise(f)));
    let (l, r) = (shelf_neighbor_part(w, lp, f), shelf_neighbor_part(w, rp, f));
    if let Some(p) = l { shelf_set_part(w, lp, part_when_disconnected_right(p)); }
    if let Some(p) = r { shelf_set_part(w, rp, part_when_disconnected_left(p)); }
}
/// SideChainPartBlock.updateSelfAndNeighborsOnPoweringUp
fn shelf_on_powering_up(w: &mut World, pos: P, s: S, old: S) {
    if !shelf_connectable(s) { return; }
    // isBeingUpdatedByNeighbor
    let getting_connected = get(s, P_SIDE_CHAIN).map_or(false, |p| p != V_UNCONNECTED);
    let was_connected = shelf_connectable(old) && get(old, P_SIDE_CHAIN).map_or(false, |p| p != V_UNCONNECTED);
    if getting_connected || was_connected { return; }
    let f = facing(s);
    let (lp, rp) = (pos.rel(clockwise(f)), pos.rel(counter_clockwise(f)));
    let (l, r) = (shelf_neighbor_part(w, lp, f), shelf_neighbor_part(w, rp, f));
    let mut new_part = V_UNCONNECTED;
    let chain_left = if l.is_some() { shelf_all_connected(w, lp).len() } else { 0 };
    let chain_right = if r.is_some() { shelf_all_connected(w, rp).len() } else { 0 };
    let mut len = 1;
    if chain_left > 0 && len + chain_left <= 3 {
        new_part = part_when_connected_left(new_part);
        shelf_set_part(w, lp, part_when_connected_right(l.unwrap()));
        len += chain_left;
    }
    if chain_right > 0 && len + chain_right <= 3 {
        new_part = part_when_connected_right(new_part);
        shelf_set_part(w, rp, part_when_connected_left(r.unwrap()));
    }
    shelf_set_part(w, pos, new_part);
}

// ================================================================================================ neighborChanged
pub fn neighbor_changed(w: &mut World, s: S, pos: P, from_block: u16, moved: bool) {
    use Kind::*;
    match kind_of(s) {
        BigDripleafBlock => {
            if crate::rules::has_neighbor_signal(w, pos) { dripleaf_reset_tilt(w, s, pos); }
        }
        ShelfBlock => {
            let signal = crate::rules::has_neighbor_signal(w, pos);
            if is_true(s, P_POWERED) != signal {
                let mut ns = with_bool(s, P_POWERED, signal);
                if !signal { ns = try_with(ns, P_SIDE_CHAIN, V_UNCONNECTED); }
                w.set_block(pos, ns, 3);
            }
        }
        SkullBlock | WallSkullBlock => {
            // AbstractSkullBlock
            let signal = crate::rules::has_neighbor_signal(w, pos);
            if has(s, P_POWERED) && signal != is_true(s, P_POWERED) { w.set_block(pos, with_bool(s, P_POWERED, signal), 2); }
        }
        // SpongeBlock (absorbs water), FrostedIceBlock (melts), LiquidBlock: nothing without water
        _ => {}
    }
}

// ================================================================================================ onPlace
/// Called for every block-state write (also same-block changes), as BlockState.onPlace is.
pub fn on_place(w: &mut World, s: S, pos: P, old: S, moved: bool) {
    use Kind::*;
    match kind_of(s) {
        ScaffoldingBlock => sched(w, s, pos, 1),
        FallingBlock | ConcretePowderBlock | AnvilBlock | BrushableBlock => sched(w, s, pos, 2),
        DragonEggBlock => sched(w, s, pos, 5),
        CoralPlantBlock | CoralFanBlock | CoralWallFanBlock => {
            if !is_dead_coral(s) { coral_schedule_die(w, s, pos); }
        }
        ComposterBlock => {
            if get(s, P_LEVEL) == Some(7) { sched(w, s, pos, 20); }
        }
        SnifferEggBlock => {
            let boosted = in_tag!(w.get(pos.below()), "sniffer_egg_hatch_boost");
            let hatch_time = if boosted { 12000 } else { 24000 };
            let d = hatch_time / 3 + w.rand_int(300) as i32;
            sched(w, s, pos, d);
        }
        ShelfBlock => {
            if is_true(s, P_POWERED) { shelf_on_powering_up(w, pos, s, old); } else { shelf_neighbors_after_powering_down(w, pos, s); }
        }
        CarvedPumpkinBlock => {
            if !same_block(old, s) { try_spawn_golem(w, pos); }
        }
        FireBlock => {
            // BaseFireBlock.onPlace (no portal frames here), then FireBlock schedules its first tick
            if !same_block(old, s) && !survives(w, s, pos) {
                w.remove_block(pos, false);
                return;
            }
            if is_blk!(s, "fire") {
                let d = 30 + w.rand_int(10) as i32;
                sched(w, s, pos, d);
            }
        }
        // SpongeBlock.tryAbsorbWater: no water. WetSpongeBlock: dries in the nether only. FrogspawnBlock / TurtleEggBlock:
        // hatching needs mobs. FrostedIceBlock cannot be placed.
        _ => {}
    }
}

fn is_golem_pumpkin(s: S) -> bool { is_blk!(s, "carved_pumpkin") || is_blk!(s, "jack_o_lantern") }
/// CarvedPumpkinBlock.clearPatternBlocks + updatePatternBlocks around the (unmodelled) golem spawn
fn golem_clear(w: &mut World, cells: &[P]) {
    for c in cells { w.set_block(*c, AIR, 2); }
    w.unmodelled += 1; // the golem itself
    for c in cells { w.update_neighbors_at(*c, blk!("air")); }
}
/// CarvedPumpkinBlock.trySpawnGolem. The pattern search is limited to patterns that contain the pumpkin just
/// placed (vanilla's BlockPattern.find would also find a complete pattern next to it), in every orientation.
fn try_spawn_golem(w: &mut World, top: P) {
    if !is_golem_pumpkin(w.get(top)) { return; }
    // snow golem: "^", "#", "#"
    for d in DIRS {
        let (a, b) = (top.rel(d), top.rel_n(d, 2));
        if is_blk!(w.get(a), "snow_block") && is_blk!(w.get(b), "snow_block") {
            golem_clear(w, &[top, a, b]);
            return;
        }
    }
    // iron golem: "~^~", "###", "~#~"
    let air = |w: &World, q: P| t().air[w.get(q) as usize];
    let iron = |w: &World, q: P| is_blk!(w.get(q), "iron_block");
    for d in DIRS {
        for side in DIRS {
            if axis(side) == axis(d) || !is_positive(side) { continue; }
            let c = top.rel(d);
            let leg = top.rel_n(d, 2);
            let (c1, c2) = (c.rel(side), c.rel(opposite(side)));
            if iron(w, c) && iron(w, c1) && iron(w, c2) && iron(w, leg)
                && air(w, top.rel(side)) && air(w, top.rel(opposite(side))) && air(w, leg.rel(side)) && air(w, leg.rel(opposite(side)))
            {
                golem_clear(w, &[top, top.rel(side), top.rel(opposite(side)), c, c1, c2, leg, leg.rel(side), leg.rel(opposite(side))]);
                return;
            }
        }
    }
    // copper golem: "^", "#" (any block of the copper tag); the copper block becomes a copper chest
    for d in DIRS {
        let a = top.rel(d);
        let cs = w.get(a);
        if in_tag!(cs, "copper") {
            let pumpkin = w.get(top);
            golem_clear(w, &[top, a]);
            // CopperChestBlock.getFromCopperBlock (COPPER_TO_COPPER_CHEST_MAPPING: same oxidation and waxing)
            let n = name_of(cs);
            let chest_name = format!("{}_chest", n.strip_suffix("_block").unwrap_or(n));
            let chest = block_named(&chest_name).unwrap_or(blk!("copper_chest"));
            let f = facing_opt(pumpkin).unwrap_or(NORTH);
            let mut st = try_with(default_state(chest), P_FACING, dir_val(f));
            // ChestBlock.getChestType
            let partner = |w: &World, nd: Dir| -> Option<Dir> {
                let n = w.get(a.rel(nd));
                if chest_can_connect_to(st, n) && get(n, P_TYPE) == Some(V_SINGLE) { facing_opt(n) } else { None }
            };
            let ty = if partner(w, clockwise(f)) == Some(f) { V_LEFT } else if partner(w, counter_clockwise(f)) == Some(f) { V_RIGHT } else { V_SINGLE };
            st = with(st, P_TYPE, ty);
            w.set_block(a, st, 2);
            return;
        }
    }
}

// ================================================================================================ affectNeighborsAfterRemoval
pub fn affect_neighbors_after_removal(w: &mut World, old: S, pos: P, moved: bool) {
    use Kind::*;
    match kind_of(old) {
        // Containers.updateNeighboursAfterDestroy
        ChestBlock | BarrelBlock | FurnaceBlock | BrewingStandBlock | ChiseledBookShelfBlock | DecoratedPotBlock | ShulkerBoxBlock | CreakingHeartBlock
        | CopperGolemStatueBlock => crate::rules::update_neighbour_for_output_signal(w, pos, block_of(old)),
        ShelfBlock => {
            crate::rules::update_neighbour_for_output_signal(w, pos, block_of(old));
            shelf_neighbors_after_powering_down(w, pos, old);
        }
        LecternBlock => {
            // updateBelow
            if is_true(old, P_POWERED) { w.update_neighbors_at(pos.below(), block_of(old)); }
        }
        _ => {}
    }
}

// ================================================================================================ scheduled ticks
/// FarmlandBlock.turnToDirt (pushEntitiesUp moves entities only; that is the integrator's business)
fn turn_to_dirt(w: &mut World, pos: P) { w.set_block(pos, default_state(blk!("dirt")), 3); }

fn dripleaf_set_tilt(w: &mut World, s: S, pos: P, tilt: u8) { w.set_block(pos, with(s, P_TILT, tilt), 2); }
/// BigDripleafBlock.setTiltAndScheduleTick (DELAY_UNTIL_NEXT_TILT_STATE: unstable 10, partial 10, full 100)
fn dripleaf_set_tilt_and_schedule(w: &mut World, s: S, pos: P, tilt: u8) {
    dripleaf_set_tilt(w, s, pos, tilt);
    let delay = match tilt { V_UNSTABLE | V_PARTIAL => 10, V_FULL => 100, _ => -1 };
    if delay != -1 { sched(w, s, pos, delay); }
}
fn dripleaf_reset_tilt(w: &mut World, s: S, pos: P) { dripleaf_set_tilt(w, s, pos, V_NONE); }

/// CreakingHeartBlock.hasRequiredLogs
fn creaking_has_logs(w: &World, s: S, pos: P) -> bool {
    let Some(ax) = get(s, P_AXIS) else { return false };
    let dirs = match ax { V_X => [WEST, EAST], V_Y => [DOWN, UP], _ => [NORTH, SOUTH] };
    dirs.iter().all(|d| { let n = w.get(pos.rel(*d)); in_tag!(n, "pale_oak_logs") && get(n, P_AXIS) == Some(ax) })
}

pub fn tick(w: &mut World, s: S, pos: P) {
    use Kind::*;
    match kind_of(s) {
        ScaffoldingBlock => {
            let distance = scaffolding_distance(w, pos);
            // isBottom
            let bottom = distance > 0 && !same_block(w.get(pos.below()), s);
            let ns = with_bool(with(s, P_DISTANCE, distance), P_BOTTOM, bottom);
            if distance == 7 {
                if val(s, P_DISTANCE) == 7 { w.spawn_falling(pos, ns); } else { w.destroy_block(pos, true); }
            } else if s != ns {
                w.set_block(pos, ns, 3);
            }
        }
        FallingBlock | ConcretePowderBlock | AnvilBlock | DragonEggBlock | BrushableBlock => {
            if is_free(w.get(pos.below())) { w.spawn_falling(pos, s); }
        }
        LeavesBlock => {
            let ns = leaves_update_distance(w, s, pos);
            w.set_block(pos, ns, 3);
        }
        SugarCaneBlock | CactusBlock | BambooStalkBlock | ChorusPlantBlock | ChorusFlowerBlock | BigDripleafStemBlock | CaveVinesBlock | CaveVinesPlantBlock
        | KelpBlock | KelpPlantBlock | TwistingVinesBlock | TwistingVinesPlantBlock | WeepingVinesBlock | WeepingVinesPlantBlock => {
            if !survives(w, s, pos) { w.destroy_block(pos, true); }
        }
        HangingMossBlock => {
            if !hanging_moss_can_stay(w, s, pos) { w.destroy_block(pos, true); }
        }
        FarmlandBlock => {
            if !survives(w, s, pos) { turn_to_dirt(w, pos); }
        }
        DirtPathBlock => turn_to_dirt(w, pos),
        BigDripleafBlock => {
            if crate::rules::has_neighbor_signal(w, pos) {
                dripleaf_reset_tilt(w, s, pos);
            } else {
                match val(s, P_TILT) {
                    V_UNSTABLE => dripleaf_set_tilt_and_schedule(w, s, pos, V_PARTIAL),
                    V_PARTIAL => dripleaf_set_tilt_and_schedule(w, s, pos, V_FULL),
                    V_FULL => dripleaf_reset_tilt(w, s, pos),
                    _ => {}
                }
            }
        }
        CoralBlock => {
            // no water next to it: it dies
            if let Some(d) = block_named(&format!("dead_{}", name_of(s))) { w.set_block(pos, default_state(d), 2); }
        }
        CoralPlantBlock | CoralFanBlock | CoralWallFanBlock => {
            if !is_dead_coral(s) { if let Some(d) = coral_dead_state(s) { w.set_block(pos, d, 2); } }
        }
        PointedDripstoneBlock | SulfurSpikeBlock => {
            if is_speleothem_with_direction(s, UP) && !survives(w, s, pos) {
                w.destroy_block(pos, true);
            } else {
                // spawnFallingStalactite
                let mut fp = pos;
                let mut fs = s;
                while is_speleothem_with_direction(fs, DOWN) {
                    w.spawn_falling(fp, try_with(fs, P_WATERLOGGED, V_FALSE));
                    let th = get(fs, P_THICKNESS);
                    if th == Some(V_TIP) || th == Some(V_TIP_MERGE) { break; }
                    fp = fp.below();
                    fs = w.get(fp);
                }
            }
        }
        ComposterBlock => {
            if get(s, P_LEVEL) == Some(7) { w.set_block(pos, with(s, P_LEVEL, 8), 3); }
        }
        LecternBlock => {
            // changePowered(level, pos, state, false)
            w.set_block(pos, with_bool(s, P_POWERED, false), 3);
            w.update_neighbors_at(pos.below(), block_of(s));
        }
        CreakingHeartBlock => {
            // CreakingHeartBlock.updateState; the creaking is not active at noon. The opposite change (logs gone ->
            // uprooted) is made by the block entity's ticker within 20-25 ticks; it is applied here, with this tick.
            let logs = creaking_has_logs(w, s, pos);
            let st = val(s, P_CREAKING_HEART_STATE);
            if logs && st == V_UPROOTED {
                w.set_block(pos, with(s, P_CREAKING_HEART_STATE, V_DORMANT), 3);
            } else if !logs && st != V_UPROOTED {
                w.set_block(pos, with(s, P_CREAKING_HEART_STATE, V_UPROOTED), 3);
            }
        }
        SnifferEggBlock => {
            let h = val(s, P_HATCH);
            if h != 2 {
                w.set_block(pos, with(s, P_HATCH, h + 1), 2);
            } else {
                w.destroy_block(pos, false);
                w.unmodelled += 1; // the sniffer
            }
        }
        DriedGhastBlock => {
            let h = get(s, P_HYDRATION).unwrap_or(0);
            if h > 0 { w.set_block(pos, with(s, P_HYDRATION, h - 1), 2); }
        }
        FireBlock => {
            // fire spreading and burning out is not modelled
            w.unmodelled += 1;
        }
        // ChestBlock / BarrelBlock / EnderChestBlock: recheckOpen (viewer counting) - nothing to do here.
        // FrostedIceBlock, FrogspawnBlock, BubbleColumnBlock, cauldrons under dripstone: not reachable.
        _ => {}
    }
}

// ================================================================================================ block events
/// ShulkerBoxBlockEntity.doNeighborUpdates
fn shulker_neighbor_updates(w: &mut World, s: S, pos: P) {
    for d in UPDATE_SHAPE_ORDER { w.neighbor_shape_changed(opposite(d), pos.rel(d), pos, s, 3, 512); }
    w.update_neighbors_at(pos, block_of(s));
}
pub fn trigger_event(w: &mut World, s: S, pos: P, b0: u8, b1: u8) -> bool {
    use Kind::*;
    match kind_of(s) {
        // ChestBlockEntity / EnderChestBlockEntity.triggerEvent: the lid (event 1 = open count)
        ChestBlock | EnderChestBlock => b0 == 1,
        ShulkerBoxBlock => {
            if b0 == 1 {
                // the lid starts moving; the block entity's ticker updates the neighbours when the animation starts and
                // again when it ends (10 ticks later, the same updates): done once, here
                if b1 == 0 || b1 == 1 { shulker_neighbor_updates(w, s, pos); }
                true
            } else {
                false
            }
        }
        EndGatewayBlock | SpawnerBlock => b0 == 1,
        // PotentSulfurBlock.triggerEvent
        PotentSulfurBlock => true,
        _ => false,
    }
}

// ================================================================================================ setPlacedBy
pub fn set_placed_by(w: &mut World, pos: P, s: S, ctx: &PlaceCtx) {
    use Kind::*;
    match kind_of(s) {
        DoorBlock => {
            w.set_block(pos.above(), with(s, P_HALF, V_UPPER), 3);
        }
        BedBlock => {
            w.set_block(pos.rel(facing(s)), with(s, P_PART, V_HEAD), 3);
        }
        DoublePlantBlock | TallFlowerBlock | TallSeagrassBlock => {
            let up = try_with(default_state(block_of(s)), P_HALF, V_UPPER);
            w.set_block(pos.above(), up, 3);
        }
        SmallDripleafBlock => {
            let mut up = try_with(default_state(block_of(s)), P_HALF, V_UPPER);
            if let Some(f) = get(s, P_FACING) { up = try_with(up, P_FACING, f); }
            w.set_block(pos.above(), up, 3);
        }
        MossyCarpetBlock => {
            let topper = mossy_create_topper(w, pos);
            if !t().air[topper as usize] { w.set_block(pos.above(), topper, 3); }
        }
        CoralBlock => {
            // CoralBlock.getStateForPlacement schedules the death tick when there is no water around (a side effect
            // of placement that `predict_place`, which cannot change the world, has to leave to this hook)
            coral_schedule_die(w, s, pos);
        }
        SkullBlock | WallSkullBlock => {
            // WitherSkullBlock.checkSpawn: three skulls on a T of soul sand summon the wither (unless peaceful)
            if is_blk!(s, "wither_skeleton_skull") || is_blk!(s, "wither_skeleton_wall_skull") {
                if wither_pattern_near(w, pos) { w.unmodelled += 1; }
            }
        }
        // PitcherCropBlock.setPlacedBy is empty
        _ => {}
    }
}
fn wither_pattern_near(w: &World, pos: P) -> bool {
    let skull = |q: P| { let s = w.get(q); is_blk!(s, "wither_skeleton_skull") || is_blk!(s, "wither_skeleton_wall_skull") };
    let base = |q: P| in_tag!(w.get(q), "wither_summon_base_blocks");
    for d in DIRS {
        for side in DIRS {
            if axis(side) == axis(d) || !is_positive(side) { continue; }
            for k in -1..=1 {
                let mid = pos.rel_n(side, k);
                let c = mid.rel(d);
                if skull(mid) && skull(mid.rel(side)) && skull(mid.rel(opposite(side))) && base(c) && base(c.rel(side)) && base(c.rel(opposite(side))) && base(c.rel(d)) {
                    return true;
                }
            }
        }
    }
    false
}

// ================================================================================================ breaking by the player
/// Block.playerWillDestroy for a survival player.
pub fn player_will_destroy(w: &mut World, pos: P, s: S) {
    use Kind::*;
    match kind_of(s) {
        DoorBlock => {
            // `player.preventsBlockDrops() || !player.hasCorrectToolForDrops(state)`: survival, and the bot holds no
            // tools, so this is true exactly for doors that need a tool (in 26.2 no door has requiresCorrectToolForDrops,
            // so in practice the other half goes by its updateShape after the removal)
            if t().requires_tool[s as usize] { prevent_drop_from_bottom_part(w, pos, s); }
        }
        // DoublePlantBlock (survival: drops only), BedBlock (creative only), BeehiveBlock, DecoratedPotBlock (needs a
        // tool in hand), ShulkerBoxBlock: no change to the world
        _ => {}
    }
}
/// DoublePlantBlock.preventDropFromBottomPart
fn prevent_drop_from_bottom_part(w: &mut World, pos: P, s: S) {
    if get(s, P_HALF) == Some(V_UPPER) {
        let bp = pos.below();
        let bs = w.get(bp);
        if same_block(bs, s) && get(bs, P_HALF) == Some(V_LOWER) { w.set_block(bp, AIR, 35); }
    }
}
/// Block.destroy (empty for every block) followed by what Block.playerDestroy changes in the world for a survival player.
pub fn destroyed(w: &mut World, pos: P, s: S) {
    if is_blk!(s, "turtle_egg") {
        // TurtleEggBlock.playerDestroy -> decreaseEggs
        let eggs = get(s, P_EGGS).unwrap_or(1);
        if eggs <= 1 { w.destroy_block(pos, false); } else { w.set_block(pos, with(s, P_EGGS, eggs - 1), 2); }
    } else if is_blk!(s, "ice") {
        // IceBlock.playerDestroy: melts into water when something solid is below - water is not modelled
        let below = w.get(pos.below());
        if !t().no_collision[below as usize] { w.unmodelled += 1; }
    }
}

/// DragonEggBlock.teleport
fn dragon_egg_teleport(w: &mut World, s: S, pos: P) {
    for _ in 0..1000 {
        let dx = w.rand_int(16) as i32 - w.rand_int(16) as i32;
        let dy = w.rand_int(8) as i32 - w.rand_int(8) as i32;
        let dz = w.rand_int(16) as i32 - w.rand_int(16) as i32;
        let tp = pos.off(dx, dy, dz);
        if t().air[w.get(tp) as usize] && !t().air[w.get(tp.below()) as usize] && w.inside(tp) && tp.y <= w.max_y {
            w.set_block(tp, s, 2);
            w.remove_block(pos, false);
            w.unmodelled += 1; // (where it lands depends on the server's random numbers)
            return;
        }
    }
}
/// RedStoneOreBlock.interact
fn redstone_ore_interact(w: &mut World, s: S, pos: P) {
    if get(s, P_LIT) == Some(V_FALSE) { w.set_block(pos, with_bool(s, P_LIT, true), 3); }
}
#[inline]
fn is_redstone_ore(s: S) -> bool { is_blk!(s, "redstone_ore") || is_blk!(s, "deepslate_redstone_ore") }

/// BlockState.attack (left click starts)
pub fn attack(w: &mut World, s: S, pos: P) {
    if kind_of(s) == Kind::DragonEggBlock {
        dragon_egg_teleport(w, s, pos);
    } else if is_redstone_ore(s) {
        redstone_ore_interact(w, s, pos);
    }
}

// ================================================================================================ right clicks
fn potted_by_content() -> &'static HashMap<u16, u16> {
    static M: OnceLock<HashMap<u16, u16>> = OnceLock::new();
    M.get_or_init(|| {
        let tb = t();
        let mut m = HashMap::new();
        for (i, b) in tb.blocks.iter().enumerate() {
            if b.kind != Kind::FlowerPotBlock { continue; }
            let Some(content) = b.name.strip_prefix("potted_") else { continue };
            // (the two azaleas are registered as potted_azalea_bush / potted_flowering_azalea_bush)
            let c = tb.block_by_name.get(content).or_else(|| content.strip_suffix("_bush").and_then(|x| tb.block_by_name.get(x)));
            if let Some(c) = c { m.insert(*c, i as u16); }
        }
        m
    })
}
/// ComposterBlock.COMPOSTABLES (item id -> chance)
fn compostables() -> &'static HashMap<u16, f32> {
    static M: OnceLock<HashMap<u16, f32>> = OnceLock::new();
    M.get_or_init(|| {
        let groups: [(f32, &str); 5] = [
            (0.3, "jungle_leaves oak_leaves spruce_leaves dark_oak_leaves pale_oak_leaves acacia_leaves cherry_leaves birch_leaves azalea_leaves mangrove_leaves oak_sapling spruce_sapling birch_sapling jungle_sapling acacia_sapling cherry_sapling dark_oak_sapling pale_oak_sapling mangrove_propagule beetroot_seeds dried_kelp short_grass kelp melon_seeds pumpkin_seeds seagrass sweet_berries glow_berries wheat_seeds moss_carpet pale_moss_carpet pale_hanging_moss pink_petals wildflowers leaf_litter small_dripleaf hanging_roots mangrove_roots torchflower_seeds pitcher_pod firefly_bush bush cactus_flower dry_short_grass dry_tall_grass"),
            (0.5, "dried_kelp_block tall_grass flowering_azalea_leaves cactus sugar_cane vine nether_sprouts weeping_vines twisting_vines melon_slice glow_lichen"),
            (0.65, "sea_pickle lily_pad pumpkin carved_pumpkin melon apple beetroot carrot cocoa_beans potato wheat brown_mushroom red_mushroom mushroom_stem crimson_fungus warped_fungus nether_wart crimson_roots warped_roots shroomlight dandelion poppy blue_orchid allium azure_bluet red_tulip orange_tulip white_tulip pink_tulip oxeye_daisy cornflower lily_of_the_valley wither_rose open_eyeblossom closed_eyeblossom fern sunflower lilac rose_bush peony large_fern spore_blossom azalea moss_block pale_moss_block big_dripleaf"),
            (0.85, "hay_block brown_mushroom_block red_mushroom_block nether_wart_block warped_wart_block flowering_azalea bread baked_potato cookie torchflower pitcher_plant"),
            (1.0, "cake pumpkin_pie"),
        ];
        let tb = t();
        let mut m = HashMap::new();
        for (ch, names) in groups {
            for n in names.split(' ') {
                if let Some(i) = tb.item_by_name.get(n) { m.insert(*i, ch); }
            }
        }
        m
    })
}
/// The block a BlockItem places (`BlockItem.getBlock`), if the item is one.
fn item_block(item: u16) -> Option<u16> { t().items.get(item as usize).and_then(|i| i.blocks.first().copied()) }
#[inline]
fn is_candle_item(n: &str) -> bool { n == "candle" || n.ends_with("_candle") }
#[inline]
fn is_hanging_sign_item(n: &str) -> bool { n.ends_with("_hanging_sign") }

/// SelectableSlotContainer.getHitSlot
fn hit_slot(pos: P, face: Dir, hit: [f64; 3], block_facing: Dir, rows: i32, columns: i32) -> Option<usize> {
    if block_facing != face { return None; }
    let (rx, ry, rz) = (hit[0] - pos.x as f64, hit[1] - pos.y as f64, hit[2] - pos.z as f64);
    let (x, y) = match face {
        NORTH => ((1.0 - rx) as f32, ry as f32),
        SOUTH => (rx as f32, ry as f32),
        WEST => (rz as f32, ry as f32),
        EAST => ((1.0 - rz) as f32, ry as f32),
        _ => return None,
    };
    let section = |c: f32, n: i32| -> i32 { floor((c * 16.0 / (16.0 / n as f32)) as f64).clamp(0, n - 1) };
    let row = section(1.0 - y, rows);
    let column = section(x, columns);
    Some((column + row * columns) as usize)
}

/// A click context for `BlockPlaceContext.canPlace` questions asked from inside a block interaction.
fn place_ctx_of(w: &World, pos: P, face: Dir, hit: [f64; 3], who: &Clicker) -> PlaceCtx {
    PlaceCtx { item: who.item, hit_pos: pos, face, hit, inside: false, yaw: who.yaw, pitch: who.pitch, sneak: who.sneak, player_pos: who.pos, replace_clicked: false }.resolve(w)
}

/// AbstractCandleBlock.extinguish
fn candle_extinguish(w: &mut World, s: S, pos: P) { w.set_block(pos, with_bool(s, P_LIT, false), 11); }

/// SignBlock.useItemOn
fn sign_use_item_on(w: &mut World, who: &mut Clicker) -> Use {
    let n = item_name(who.item);
    // SignApplicator items: dyes and ink sacs apply to signs with text only (canApplyToSign); honeycomb waxes any sign
    if n == "honeycomb" {
        who.consumed = true;
        w.unmodelled += 1; // (a waxed sign no longer opens its editor; sign contents are not tracked)
        return Use::Consume;
    }
    Use::TryEmptyHand
}

/// BlockState.useItemOn: `who.item` is the held item (0 = empty hand).
pub fn use_item_on(w: &mut World, s: S, pos: P, face: Dir, hit: [f64; 3], who: &mut Clicker) -> Use {
    use Kind::*;
    let item = who.item;
    let iname = item_name(item);
    match kind_of(s) {
        CakeBlock => {
            if is_candle_item(iname) && get(s, P_BITES) == Some(0) {
                if let Some(cake) = block_named(&format!("{iname}_cake")) {
                    who.consumed = true;
                    w.set_block(pos, default_state(cake), 3);
                    return Use::Consume;
                }
            }
            Use::TryEmptyHand
        }
        CandleBlock => {
            if item == 0 && is_true(s, P_LIT) {
                candle_extinguish(w, s, pos);
                Use::Consume
            } else {
                Use::TryEmptyHand
            }
        }
        CandleCakeBlock => {
            if iname == "flint_and_steel" || iname == "fire_charge" {
                Use::Pass
            } else if hit[1] - pos.y as f64 > 0.5 && item == 0 && is_true(s, P_LIT) {
                candle_extinguish(w, s, pos);
                Use::Consume
            } else {
                Use::TryEmptyHand
            }
        }
        FlowerPotBlock => {
            let Some(potted) = item_block(item).and_then(|b| potted_by_content().get(&b).copied()) else { return Use::TryEmptyHand };
            if !is_blk!(s, "flower_pot") { return Use::Consume; }
            w.set_block(pos, default_state(potted), 3);
            who.consumed = true;
            Use::Consume
        }
        ComposterBlock => {
            let level = val(s, P_LEVEL);
            if level < 8 && compostables().contains_key(&item) {
                if level < 7 {
                    // addItem
                    let chance = compostables()[&item];
                    if !((level != 0 || !(chance > 0.0)) && !((w.rand_f32() as f64) < chance as f64)) {
                        let nl = level + 1;
                        w.set_block(pos, with(s, P_LEVEL, nl), 3);
                        if nl == 7 { sched(w, s, pos, 20); }
                    }
                    who.consumed = true;
                }
                Use::Consume
            } else {
                Use::TryEmptyHand
            }
        }
        CampfireBlock => {
            // the campfire cooking recipes' inputs; the block entity holds four items
            if matches!(iname, "beef" | "chicken" | "cod" | "kelp" | "mutton" | "porkchop" | "potato" | "rabbit" | "salmon") {
                let slots = w.containers.entry(pos).or_insert_with(Vec::new);
                while slots.len() < 4 { slots.push(crate::world::Stack::default()); }
                if let Some(free) = slots.iter_mut().find(|x| x.count == 0) {
                    *free = crate::world::Stack { item, count: 1 };
                    who.consumed = true;
                }
                return Use::Consume;
            }
            Use::TryEmptyHand
        }
        RespawnAnchorBlock => {
            let charges = val(s, P_CHARGES);
            if iname == "glowstone" && charges < 4 {
                w.set_block(pos, with(s, P_CHARGES, charges + 1), 3);
                who.consumed = true;
                Use::Consume
            } else {
                Use::TryEmptyHand
            }
        }
        LecternBlock => {
            if is_true(s, P_HAS_BOOK) {
                Use::TryEmptyHand
            } else if iname == "written_book" || iname == "writable_book" {
                // tryPlaceBook -> resetBookState
                w.set_block(pos, with_bool(with_bool(s, P_POWERED, false), P_HAS_BOOK, true), 3);
                w.update_neighbors_at(pos.below(), block_of(s));
                who.consumed = true;
                Use::Consume
            } else if item == 0 {
                Use::Pass
            } else {
                Use::TryEmptyHand
            }
        }
        ChiseledBookShelfBlock => {
            if !matches!(iname, "book" | "written_book" | "enchanted_book" | "writable_book" | "knowledge_book") { return Use::TryEmptyHand; }
            let Some(slot) = hit_slot(pos, face, hit, facing(s), 2, 3) else { return Use::Pass };
            let prop = P_SLOT_0_OCCUPIED + slot as u8;
            if is_true(s, prop) { return Use::TryEmptyHand; }
            // addBook
            who.consumed = true;
            w.set_block(pos, with_bool(s, prop, true), 3);
            Use::Consume
        }
        DecoratedPotBlock => {
            if item == 0 { return Use::TryEmptyHand; }
            let max = t().items[item as usize].stack;
            let slots = w.containers.entry(pos).or_insert_with(Vec::new);
            if slots.is_empty() { slots.push(crate::world::Stack::default()); }
            let pot = &mut slots[0];
            if pot.count == 0 || pot.item == item && pot.count < max {
                if pot.count == 0 { *pot = crate::world::Stack { item, count: 1 }; } else { pot.count += 1; }
                who.consumed = true;
                Use::Consume
            } else {
                Use::TryEmptyHand
            }
        }
        ShelfBlock => {
            let Some(slot) = hit_slot(pos, face, hit, facing(s), 1, 3) else { return Use::Pass };
            if !is_true(s, P_POWERED) {
                // swapSingleItem: the whole held stack goes on the shelf, what was there comes to the hand
                let removed = w.containers.get(&pos).and_then(|v| v.get(slot)).copied().unwrap_or_default();
                if removed.count == 0 && item == 0 { return Use::Pass; }
                let slots = w.containers.entry(pos).or_insert_with(Vec::new);
                while slots.len() < 3 { slots.push(crate::world::Stack::default()); }
                slots[slot] = if item == 0 { crate::world::Stack::default() } else { crate::world::Stack { item, count: 1 } };
                if item != 0 { who.consumed = true; }
                // (the size of the stack that moved, and the stack that came back, cannot be told to the caller)
                w.unmodelled += 1;
                Use::Consume
            } else {
                // swapHotbar: the hotbar is exchanged with the powered shelves
                w.unmodelled += 1;
                Use::Consume
            }
        }
        CopperGolemStatueBlock => {
            if iname.ends_with("_axe") { return Use::Pass; }
            // WeatheringCopperGolemStatueBlock (the unwaxed ones) lets honeycomb through to be used on it
            if iname == "honeycomb" && !name_of(s).starts_with("waxed_") { return Use::Pass; }
            // updatePose: Pose.getNextPose
            let next = match val(s, P_COPPER_GOLEM_POSE) { V_STANDING => V_SITTING, V_SITTING => V_RUNNING, V_RUNNING => V_STAR, _ => V_STANDING };
            w.set_block(pos, with(s, P_COPPER_GOLEM_POSE, next), 3);
            Use::Consume
        }
        BeehiveBlock => {
            if get(s, P_HONEY_LEVEL).unwrap_or(0) >= 5 && (iname == "shears" || iname == "glass_bottle") {
                if iname == "glass_bottle" { who.consumed = true; }
                // resetHoneyLevel (the bees are not modelled)
                w.set_block(pos, with(s, P_HONEY_LEVEL, 0), 3);
                Use::Consume
            } else {
                Use::TryEmptyHand
            }
        }
        CeilingHangingSignBlock => {
            // shouldTryToChainAnotherHangingSign
            if is_hanging_sign_item(iname) && face == DOWN { Use::Pass } else { sign_use_item_on(w, who) }
        }
        WallHangingSignBlock => {
            // ... && !isHittingEditableSide
            if is_hanging_sign_item(iname) && axis(face) != axis(facing(s)) { Use::Pass } else { sign_use_item_on(w, who) }
        }
        StandingSignBlock | WallSignBlock => sign_use_item_on(w, who),
        VaultBlock => {
            // a non-key item on an active vault: tryInsertKey fails, the click is used up
            if item != 0 && get(s, P_VAULT_STATE) == Some(V_ACTIVE) { Use::Consume } else { Use::TryEmptyHand }
        }
        SweetBerryBushBlock => {
            if get(s, P_AGE) != Some(3) && iname == "bone_meal" { Use::Pass } else { Use::TryEmptyHand }
        }
        LayeredCauldronBlock | CauldronBlock | LavaCauldronBlock => {
            // CauldronInteraction: buckets, bottles, and washing dyed things in water - none of which the bot holds
            Use::TryEmptyHand
        }
        Block => {
            if is_redstone_ore(s) {
                redstone_ore_interact(w, s, pos);
                // `itemStack.getItem() instanceof BlockItem && new BlockPlaceContext(...).canPlace()`
                let can_place = item_block(item).is_some() && {
                    let ctx = place_ctx_of(w, pos, face, hit, who);
                    ctx.replace_clicked || super::place::can_be_replaced(w, w.get(ctx.pos()), ctx.pos(), &ctx)
                };
                if can_place { Use::Pass } else { Use::Consume }
            } else if is_blk!(s, "pumpkin") && iname == "shears" {
                let d = if axis(face) == AXIS_Y { opposite(from_yrot(who.yaw as f64)) } else { face };
                w.set_block(pos, try_with(default_state(blk!("carved_pumpkin")), P_FACING, dir_val(d)), 11);
                Use::Consume
            } else {
                Use::TryEmptyHand
            }
        }
        _ => Use::TryEmptyHand,
    }
}

/// ShulkerBoxBlock.canOpen for a closed box: the half block the lid moves into must be free of collision shapes.
fn shulker_can_open(w: &World, s: S, pos: P) -> bool {
    let f = facing_opt(s).unwrap_or(UP);
    let st = STEP[f as usize];
    // Shulker.getProgressDeltaAabb(1, facing, 0, 0.5, bottom centre).deflate(1e-6)
    let mut b = Aabb::new(pos.x as f64, pos.y as f64, pos.z as f64, pos.x as f64 + 1.0, pos.y as f64 + 1.0, pos.z as f64 + 1.0);
    let d = [st.0 as f64, st.1 as f64, st.2 as f64];
    for i in 0..3 {
        if d[i] > 0.0 { b.min[i] += 1.0; b.max[i] += 0.5; } else if d[i] < 0.0 { b.max[i] -= 1.0; b.min[i] -= 0.5; }
    }
    let b = b.deflate(1.0e-6);
    let tb = t();
    for x in floor(b.min[0]) - 1..=floor(b.max[0]) + 1 {
        for y in floor(b.min[1]) - 1..=floor(b.max[1]) + 1 {
            for z in floor(b.min[2]) - 1..=floor(b.max[2]) + 1 {
                let q = p(x, y, z);
                if q == pos { continue; }
                let sh = &tb.shapes[tb.col_shape[w.get(q) as usize] as usize];
                if sh.intersects([x as f64, y as f64, z as f64], &b) { return false; }
            }
        }
    }
    true
}

/// A storage screen opens (what ContainerOpenersCounter does to the block on the first viewer).
fn open_storage(w: &mut World, s: S, pos: P, parts: &[P], slots: u16) {
    w.opened_menu = Some((pos, slots));
    for q in parts {
        let st = w.get(*q);
        match kind_of(st) {
            Kind::BarrelBlock => { w.set_block(*q, with_bool(st, P_OPEN, true), 3); }
            Kind::ChestBlock => {
                // ChestBlockEntity.signalOpenCount; a trapped chest also updates its neighbours (its signal is the
                // number of viewers, which is the caller's to keep)
                w.block_event(*q, block_of(st), 1, 1);
                if is_blk!(st, "trapped_chest") {
                    w.update_neighbors_at(*q, block_of(st));
                    w.update_neighbors_at(q.below(), block_of(st));
                }
            }
            Kind::EnderChestBlock | Kind::ShulkerBoxBlock => w.block_event(*q, block_of(st), 1, 1),
            _ => {}
        }
    }
}
/// For the caller: the player closed the screen that a click on `pos` opened (ContainerOpenersCounter's last viewer left).
pub fn menu_closed(w: &mut World, pos: P) {
    let s = w.get(pos);
    match kind_of(s) {
        Kind::BarrelBlock => { w.set_block(pos, with_bool(s, P_OPEN, false), 3); }
        Kind::ChestBlock => {
            let mut parts = vec![pos];
            if val(s, P_TYPE) != V_SINGLE {
                let np = pos.rel(chest_connected_dir(s));
                if same_block(w.get(np), s) { parts.push(np); }
            }
            for q in parts {
                let st = w.get(q);
                w.block_event(q, block_of(st), 1, 0);
                if is_blk!(st, "trapped_chest") {
                    w.update_neighbors_at(q, block_of(st));
                    w.update_neighbors_at(q.below(), block_of(st));
                }
            }
        }
        Kind::EnderChestBlock | Kind::ShulkerBoxBlock => w.block_event(pos, block_of(s), 1, 0),
        _ => {}
    }
}

/// BlockState.useWithoutItem
pub fn use_without_item(w: &mut World, s: S, pos: P, face: Dir, hit: [f64; 3], who: &mut Clicker) -> Use {
    use Kind::*;
    match kind_of(s) {
        DoorBlock => {
            // BlockSetType.canOpenByHand: everything but iron
            if is_blk!(s, "iron_door") { return Use::Pass; }
            w.set_block(pos, with_bool(s, P_OPEN, !is_true(s, P_OPEN)), 10);
            Use::Consume
        }
        TrapDoorBlock => {
            if is_blk!(s, "iron_trapdoor") { return Use::Pass; }
            w.set_block(pos, with_bool(s, P_OPEN, !is_true(s, P_OPEN)), 2);
            Use::Consume
        }
        FenceGateBlock => {
            if is_true(s, P_OPEN) {
                w.set_block(pos, with_bool(s, P_OPEN, false), 10);
            } else {
                let d = from_yrot(who.yaw as f64);
                let mut st = s;
                if facing(s) == opposite(d) { st = with(st, P_FACING, dir_val(d)); }
                w.set_block(pos, with_bool(st, P_OPEN, true), 10);
            }
            Use::Consume
        }
        // FenceBlock: LeadItem.bindPlayerMobs with no leashed mobs is PASS. CakeBlock / CandleCakeBlock: the player is
        // never hungry (`canEat(false)` fails) -> PASS.
        FlowerPotBlock => {
            if is_blk!(s, "flower_pot") { return Use::Consume; }
            // the plant goes back into the player's inventory, which this hook cannot tell the caller
            w.unmodelled += 1;
            w.set_block(pos, default_state(blk!("flower_pot")), 3);
            Use::Consume
        }
        ComposterBlock => {
            if get(s, P_LEVEL) == Some(8) {
                // extractProduce -> empty (the bone meal drops as an item)
                w.set_block(pos, with(s, P_LEVEL, 0), 3);
                Use::Consume
            } else {
                Use::Pass
            }
        }
        BedBlock => {
            // overworld (no explosion); at noon startSleepInBed fails with "you can only sleep at night"
            if get(s, P_PART) != Some(V_HEAD) {
                let hp = pos.rel(facing(s));
                if !same_block(w.get(hp), s) { return Use::Consume; }
            }
            Use::Consume
        }
        RespawnAnchorBlock => {
            if get(s, P_CHARGES).unwrap_or(0) == 0 { return Use::Pass; }
            // RESPAWN_ANCHOR_WORKS is false in the overworld: it explodes
            w.remove_block(pos, false);
            w.unmodelled += 1;
            Use::Consume
        }
        ChestBlock => {
            if let Some(parts) = chest_menu_parts(w, s, pos) {
                let slots = if parts.len() == 2 { 54 } else { 27 };
                open_storage(w, s, pos, &parts, slots);
            }
            Use::Consume
        }
        EnderChestBlock => {
            if !is_redstone_conductor(w.get(pos.above())) {
                // the player's own ender inventory: not one of the world's containers
                open_storage(w, s, pos, &[pos], 0);
            }
            Use::Consume
        }
        BarrelBlock => {
            open_storage(w, s, pos, &[pos], 27);
            Use::Consume
        }
        ShulkerBoxBlock => {
            if shulker_can_open(w, s, pos) { open_storage(w, s, pos, &[pos], 27); }
            Use::Consume
        }
        FurnaceBlock | BrewingStandBlock | BeaconBlock | CraftingTableBlock | SmithingTableBlock | EnchantingTableBlock | AnvilBlock | GrindstoneBlock
        | LoomBlock | CartographyTableBlock | StonecutterBlock => {
            w.opened_menu = Some((pos, 0));
            Use::Consume
        }
        LecternBlock => {
            if is_true(s, P_HAS_BOOK) { w.opened_menu = Some((pos, 0)); }
            Use::Consume
        }
        ChiseledBookShelfBlock => {
            let Some(slot) = hit_slot(pos, face, hit, facing(s), 2, 3) else { return Use::Pass };
            let prop = P_SLOT_0_OCCUPIED + slot as u8;
            if !is_true(s, prop) { return Use::Consume; }
            // removeBook: the book goes to the player's inventory (not reported to the caller)
            w.unmodelled += 1;
            w.set_block(pos, with_bool(s, prop, false), 3);
            Use::Consume
        }
        // the pot wobbles
        DecoratedPotBlock => Use::Consume,
        SweetBerryBushBlock => {
            if get(s, P_AGE).unwrap_or(0) > 1 {
                w.set_block(pos, with(s, P_AGE, 1), 2);
                Use::Consume
            } else {
                Use::Pass
            }
        }
        CaveVinesBlock | CaveVinesPlantBlock => {
            // CaveVines.use
            if is_true(s, P_BERRIES) {
                w.set_block(pos, with_bool(s, P_BERRIES, false), 2);
                Use::Consume
            } else {
                Use::Pass
            }
        }
        StandingSignBlock | WallSignBlock | CeilingHangingSignBlock | WallHangingSignBlock => {
            // an unwaxed sign with plain text and no one else editing: the edit screen opens (ServerPlayer.openTextEdit).
            // That is a sign-editor packet, not a container screen: the bot's client does not show it, so the click is
            // used up and nothing else happens.
            Use::Consume
        }
        DragonEggBlock => {
            dragon_egg_teleport(w, s, pos);
            Use::Consume
        }
        // LightBlock: only operators cycle the level; the click is still used up
        LightBlock => Use::Consume,
        _ => Use::Pass,
    }
}

// ================================================================================================ the player on / in blocks
/// Block.stepOn for the player. (Whether the player sneaks is not known to this hook: taken as not sneaking.)
pub fn step_on(w: &mut World, s: S, pos: P) {
    if is_redstone_ore(s) {
        redstone_ore_interact(w, s, pos);
    } else if is_blk!(s, "turtle_egg") {
        // TurtleEggBlock.destroyEgg(level, state, pos, entity, 100): one chance in a hundred per tick
        if w.rand_int(100) == 0 {
            let eggs = get(s, P_EGGS).unwrap_or(1);
            if eggs <= 1 { w.destroy_block(pos, false); } else { w.set_block(pos, with(s, P_EGGS, eggs - 1), 2); }
        }
    }
    // MagmaBlock (damage), SlimeBlock (movement), SculkShriekerBlock (warden warnings): no block changes.
}

/// Is the entity resting on the top of some collision box (Entity.onGround, as far as blocks can tell)?
fn rests_on_block(w: &World, e: &crate::world::EntityView) -> bool {
    let tb = t();
    let y = e.bb.min[1];
    let fy = floor(y);
    for cx in floor(e.bb.min[0])..=floor(e.bb.max[0]) {
        for cz in floor(e.bb.min[2])..=floor(e.bb.max[2]) {
            for cy in fy - 2..=fy {
                let sh = &tb.shapes[tb.col_shape[w.get(p(cx, cy, cz)) as usize] as usize];
                for b in &sh.boxes {
                    if (cy as f64 + b[4] - y).abs() < 1.0e-5
                        && e.bb.min[0] < cx as f64 + b[3] - 1.0e-7 && e.bb.max[0] > cx as f64 + b[0] + 1.0e-7
                        && e.bb.min[2] < cz as f64 + b[5] - 1.0e-7 && e.bb.max[2] > cz as f64 + b[2] + 1.0e-7
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// BlockState.entityInside for the player.
pub fn entity_inside(w: &mut World, s: S, pos: P) {
    if kind_of(s) == Kind::BigDripleafBlock {
        if get(s, P_TILT) != Some(V_NONE) { return; }
        // canEntityTilt: entity.onGround() && entity.position().y > pos.getY() + 0.6875
        let Some(e) = w.entities.iter().find(|e| e.is_player).cloned() else { return };
        if e.pos[1] > pos.y as f64 + 0.6875f32 as f64 && rests_on_block(w, &e) && !crate::rules::has_neighbor_signal(w, pos) {
            dripleaf_set_tilt_and_schedule(w, s, pos, V_UNSTABLE);
        }
    }
    // Cobweb, berry bush, powder snow, honey, cactus, fire, campfire: movement and damage only (the player never
    // burns, so powder snow is never melted). Portals do not exist here.
}

/// Fallable.onLand (a falling block has landed and been placed at pos, replacing `replaced`): nothing changes a
/// block here (concrete powder only hardens in water, an anvil is only damaged when it hurts something).
pub fn on_land(_w: &mut World, _pos: P, _s: S, _replaced: S) {}
