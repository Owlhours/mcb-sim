//! Collision queries and ray casts against the world, following Level/CollisionGetter/BlockCollisions/BlockGetter.
#![allow(dead_code)]
use crate::geom::*;
use crate::kinds::Kind;
use crate::props::*;
use crate::shapes::VoxShape;
use crate::tables::*;
use crate::world::World;
use std::sync::OnceLock;

/// Mth.sin / Mth.cos (table lookups, as the game does them).
fn sin_table() -> &'static Vec<f32> {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| (0..65536).map(|i| (i as f64 / 10430.378350470453).sin() as f32).collect())
}
#[inline]
pub fn msin(v: f64) -> f32 { sin_table()[((v * 10430.378350470453) as i64 & 65535) as usize] }
#[inline]
pub fn mcos(v: f64) -> f32 { sin_table()[((v * 10430.378350470453 + 16384.0) as i64 & 65535) as usize] }

/// Who is asking for a collision shape (EntityCollisionContext).
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub descending: bool,
    pub bottom: f64,
    /// false for CollisionContext.empty(): "is above" questions take their default
    pub entity: bool,
    pub falling_block: bool,
    pub holding_scaffolding: bool,
    /// PistonMovingBlockEntity.NOCLIP: while a piston moves the entity, blocks moving the same way do not collide
    pub noclip: Option<Dir>,
}
pub const CTX_EMPTY: Ctx = Ctx { descending: false, bottom: -f64::MAX, entity: false, falling_block: false, holding_scaffolding: false, noclip: None };

struct Extra {
    scaffold_unstable_bottom: VoxShape,
    full: VoxShape,
    empty: VoxShape,
    cauldron_inside: VoxShape,
    /// hopper interaction shapes by facing (down, -, north, south, west, east)
    hopper: [VoxShape; 6],
}
fn extra() -> &'static Extra {
    static E: OnceLock<Extra> = OnceLock::new();
    E.get_or_init(|| Extra {
        scaffold_unstable_bottom: VoxShape::from_boxes(&[[0.0, 0.0, 0.0, 1.0, 0.125, 1.0]]),
        full: VoxShape::from_boxes(&[[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]),
        empty: VoxShape::from_boxes(&[]),
        cauldron_inside: VoxShape::from_boxes(&[[0.125, 0.25, 0.125, 0.875, 1.0, 0.875]]),
        hopper: {
            let inside = [0.125, 0.6875, 0.125, 0.875, 1.0, 0.875];
            let sp = |b: [f64; 6]| VoxShape::from_boxes(&[inside, b]);
            [VoxShape::from_boxes(&[inside]), VoxShape::from_boxes(&[inside]),
             sp([0.375, 0.5, 0.0, 0.625, 0.625, 0.25]), sp([0.375, 0.5, 0.75, 0.625, 0.625, 1.0]),
             sp([0.0, 0.5, 0.375, 0.25, 0.625, 0.625]), sp([0.75, 0.5, 0.375, 1.0, 0.625, 0.625])]
        },
    })
}

/// Mth.getSeed
fn seed(x: i32, y: i32, z: i32) -> i64 {
    let mut s = (x.wrapping_mul(3129871) as i64) ^ (z as i64).wrapping_mul(116129781) ^ (y as i64);
    s = s.wrapping_mul(s).wrapping_mul(42317861).wrapping_add(s.wrapping_mul(11));
    s >> 16
}
fn raw_offset(b: &BlockInfo, x: i32, z: i32) -> [f64; 3] {
    let sd = seed(x, 0, z);
    let maxh: f32 = if matches!(b.kind, Kind::PointedDripstoneBlock | Kind::SulfurSpikeBlock) { 0.125 } else { 0.25 };
    let maxv: f32 = if b.kind == Kind::SmallDripleafBlock { 0.1 } else { 0.2 };
    let ox = ((((sd & 15) as f32 / 15.0f32) as f64 - 0.5) * 0.5).clamp(-maxh as f64, maxh as f64);
    let oz = ((((sd >> 8 & 15) as f32 / 15.0f32) as f64 - 0.5) * 0.5).clamp(-maxh as f64, maxh as f64);
    let oy = if b.offset == 2 { (((sd >> 4 & 15) as f32 / 15.0f32) as f64 - 1.0) * maxv as f64 } else { 0.0 };
    [ox, oy, oz]
}
/// Offset of a block's shapes at a position relative to the table's shapes (which were taken at BlockPos.ZERO).
#[inline]
fn shape_off(s: S, q: P) -> [f64; 3] {
    let b = t().binfo(s);
    let base = [q.x as f64, q.y as f64, q.z as f64];
    // (only these classes move their shapes by the offset; for the other offset blocks it is visual only)
    if b.offset == 0 || !matches!(b.kind, Kind::FlowerBlock | Kind::BambooSaplingBlock | Kind::BambooStalkBlock | Kind::MangrovePropaguleBlock | Kind::PointedDripstoneBlock | Kind::SulfurSpikeBlock) { return base; }
    let here = raw_offset(b, q.x, q.z);
    let zero = raw_offset(b, 0, 0);
    [base[0] + here[0] - zero[0], base[1] + here[1] - zero[1], base[2] + here[2] - zero[2]]
}

/// BlockState.getCollisionShape(level, pos, context)
pub fn col_shape(s: S, q: P, ctx: &Ctx) -> (&'static VoxShape, [f64; 3]) {
    let tb = t();
    if tb.scaffolding[s as usize] {
        let base = [q.x as f64, q.y as f64, q.z as f64];
        let above_block = if ctx.entity { ctx.bottom > q.y as f64 + 1.0 - 1.0e-5f32 as f64 } else { true };
        if above_block && !ctx.descending { return (&tb.shapes[tb.col_shape[s as usize] as usize], base); }
        let above_below = if ctx.entity { ctx.bottom > q.y as f64 + 0.0 - 1.0e-5f32 as f64 } else { true };
        if val(s, P_DISTANCE) != 0 && is_true(s, P_BOTTOM) && above_below { return (&extra().scaffold_unstable_bottom, base); }
        return (&extra().empty, base);
    }
    if ctx.falling_block && tb.kind(s) == Kind::PowderSnowBlock {
        return (&extra().full, [q.x as f64, q.y as f64, q.z as f64]);
    }
    (&tb.shapes[tb.col_shape[s as usize] as usize], shape_off(s, q))
}

/// BlockState.getShape(level, pos, context): the outline.
pub fn out_shape(s: S, q: P, ctx: &Ctx) -> (&'static VoxShape, [f64; 3]) {
    let tb = t();
    if tb.scaffolding[s as usize] && ctx.holding_scaffolding { return (&extra().full, [q.x as f64, q.y as f64, q.z as f64]); }
    (&tb.shapes[tb.out_shape[s as usize] as usize], shape_off(s, q))
}

/// The collision shapes a box meets (BlockCollisions), as (shape, offset, position).
pub fn colliders(w: &World, ctx: &Ctx, bx: &Aabb, out: &mut Vec<(&'static VoxShape, [f64; 3], P)>) {
    out.clear();
    let x0 = floor(bx.min[0] - 1.0e-7) - 1; let x1 = floor(bx.max[0] + 1.0e-7) + 1;
    let y0 = floor(bx.min[1] - 1.0e-7) - 1; let y1 = floor(bx.max[1] + 1.0e-7) + 1;
    let z0 = floor(bx.min[2] - 1.0e-7) - 1; let z1 = floor(bx.max[2] + 1.0e-7) + 1;
    // Cursor3D order: x fastest, then y, then z
    for z in z0..=z1 { for y in y0..=y1 { for x in x0..=x1 {
        let ft = (x == x0 || x == x1) as u8 + (y == y0 || y == y1) as u8 + (z == z0 || z == z1) as u8;
        if ft == 3 { continue; }
        let q = p(x, y, z);
        let s = w.get(q);
        if s == 0 { continue; }
        if ft == 2 && !crate::is_blk!(s, "moving_piston") { continue; }
        if crate::is_blk!(s, "moving_piston") {
            // PistonMovingBlockEntity.getCollisionShape: the block in transit, where it is now (and the piston's base while it retracts)
            if let Some(m) = crate::rules::redstone::moving_block_at(w, q) {
                let tb = t();
                let base = [x as f64, y as f64, z as f64];
                if !m.extending && m.source && kind_of(m.moved) == Kind::PistonBaseBlock {
                    let sh = &tb.shapes[tb.col_shape[with_bool(m.moved, P_EXTENDED, true) as usize] as usize];
                    if !sh.empty && sh.intersects(base, bx) { out.push((sh, base, q)); }
                }
                let mdir = if m.extending { m.dir } else { opposite(m.dir) };
                if m.progress < 1.0 && ctx.noclip == Some(mdir) { continue; }
                let bs = if m.source {
                    let head = with(tb.default_of("piston_head"), P_FACING, dir_val(m.dir));
                    with_bool(head, P_SHORT, m.extending != (1.0f32 - m.progress < 0.25f32))
                } else { m.moved };
                let ep = (if m.extending { m.progress - 1.0f32 } else { 1.0f32 - m.progress }) as f64;
                let st = STEP[m.dir as usize];
                let off = [base[0] + st.0 as f64 * ep, base[1] + st.1 as f64 * ep, base[2] + st.2 as f64 * ep];
                let sh = &tb.shapes[tb.col_shape[bs as usize] as usize];
                if !sh.empty && sh.intersects(off, bx) { out.push((sh, off, q)); }
            }
            continue;
        }
        let (sh, off) = col_shape(s, q, ctx);
        // (blocks with a dynamic shape have no cached "large" flag in the game and always count as large)
        if ft == 1 && !t().shapes[t().col_shape[s as usize] as usize].large && !matches!(kind_of(s), Kind::BambooStalkBlock | Kind::ScaffoldingBlock | Kind::PowderSnowBlock | Kind::PointedDripstoneBlock | Kind::SulfurSpikeBlock) { continue; }
        if sh.empty { continue; }
        if sh.full {
            if bx.intersects_cube(x, y, z) { out.push((sh, off, q)); }
        } else if sh.intersects(off, bx) {
            out.push((sh, off, q));
        }
    } } }
}

/// CollisionGetter.noCollision(entity, box) (blocks only: no other entity collides).
pub fn no_collision(w: &World, ctx: &Ctx, bx: &Aabb) -> bool {
    SCRATCH.with(|sc| {
        let mut v = sc.borrow_mut();
        colliders(w, ctx, bx, &mut v);
        v.is_empty()
    })
}

thread_local! {
    /// reused buffers for the collision queries (they run a dozen times per tick)
    static SCRATCH: std::cell::RefCell<Vec<(&'static VoxShape, [f64; 3], P)>> = const { std::cell::RefCell::new(Vec::new()) };
    static SCRATCH2: std::cell::RefCell<Vec<(&'static VoxShape, [f64; 3], P)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Shapes.collide over a list
pub fn collide_axis(axis: usize, moving: &Aabb, shapes: &[(&'static VoxShape, [f64; 3], P)], mut dist: f64) -> f64 {
    for (sh, off, _) in shapes {
        if dist.abs() < 1.0e-7 { return 0.0; }
        dist = sh.collide(axis, moving, *off, dist);
    }
    dist
}

/// Entity.collideWithShapes
pub fn collide_with_shapes(mv: [f64; 3], bx: &Aabb, shapes: &[(&'static VoxShape, [f64; 3], P)]) -> [f64; 3] {
    if shapes.is_empty() { return mv; }
    let mut res = [0.0f64; 3];
    let order: [usize; 3] = if mv[0].abs() < mv[2].abs() { [1, 2, 0] } else { [1, 0, 2] };
    for a in order {
        if mv[a] != 0.0 {
            let moved = bx.moved(res[0], res[1], res[2]);
            res[a] = collide_axis(a, &moved, shapes, mv[a]);
        }
    }
    res
}

/// Entity.collide: movement of a box through the world, with stepping up when on the ground.
pub fn collide(w: &World, ctx: &Ctx, bb: &Aabb, mv: [f64; 3], on_ground: bool, max_up_step: f32) -> [f64; 3] {
    SCRATCH.with(|a| SCRATCH2.with(|b| collide_impl(w, ctx, bb, mv, on_ground, max_up_step, &mut a.borrow_mut(), &mut b.borrow_mut())))
}

#[allow(clippy::too_many_arguments)]
fn collide_impl(w: &World, ctx: &Ctx, bb: &Aabb, mv: [f64; 3], on_ground: bool, max_up_step: f32, shapes: &mut Vec<(&'static VoxShape, [f64; 3], P)>, cs: &mut Vec<(&'static VoxShape, [f64; 3], P)>) -> [f64; 3] {
    shapes.clear();
    let step = if mv[0] * mv[0] + mv[1] * mv[1] + mv[2] * mv[2] == 0.0 {
        mv
    } else {
        colliders(w, ctx, &bb.expand_towards(mv[0], mv[1], mv[2]), shapes);
        collide_with_shapes(mv, bb, shapes)
    };
    let xc = mv[0] != step[0];
    let yc = mv[1] != step[1];
    let zc = mv[2] != step[2];
    let ground_after = yc && mv[1] < 0.0;
    if max_up_step > 0.0 && (ground_after || on_ground) && (xc || zc) {
        let grounded = if ground_after { bb.moved(0.0, step[1], 0.0) } else { *bb };
        let mut up = grounded.expand_towards(mv[0], max_up_step as f64, mv[2]);
        if !ground_after { up = up.expand_towards(0.0, -1.0e-5f32 as f64, 0.0); }
        colliders(w, ctx, &up, cs);
        let skip = step[1] as f32;
        // candidate step heights: the y coordinates of the colliders relative to the feet
        let mut cands: Vec<f32> = Vec::new();
        for (sh, off, _) in cs.iter() {
            for c in &sh.coords[1] {
                let rel = (*c + off[1] - grounded.min[1]) as f32;
                if !(rel < 0.0) && rel != skip {
                    if rel > max_up_step { break; }
                    if !cands.contains(&rel) { cands.push(rel); }
                }
            }
        }
        cands.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let base_h = step[0] * step[0] + step[2] * step[2];
        for h in cands {
            let r = collide_with_shapes([mv[0], h as f64, mv[2]], &grounded, cs);
            if r[0] * r[0] + r[2] * r[2] > base_h {
                let dg = bb.min[1] - grounded.min[1];
                return [r[0], r[1] - dg, r[2]];
            }
        }
    }
    step
}

/// CollisionGetter.findSupportingBlock
pub fn find_supporting_block(w: &World, ctx: &Ctx, bx: &Aabb, pos: [f64; 3]) -> Option<P> {
    SCRATCH.with(|sc| {
        let mut v = sc.borrow_mut();
        colliders(w, ctx, bx, &mut v);
        find_supporting_in(&v, pos)
    })
}

fn find_supporting_in(v: &[(&'static VoxShape, [f64; 3], P)], pos: [f64; 3]) -> Option<P> {
    let mut best: Option<P> = None;
    let mut bd = f64::MAX;
    for &(_, _, q) in v {
        let (dx, dy, dz) = (q.x as f64 + 0.5 - pos[0], q.y as f64 + 0.5 - pos[1], q.z as f64 + 0.5 - pos[2]);
        let d = dx * dx + dy * dy + dz * dz;
        let better = d < bd || (d == bd && match best { None => true, Some(b) => (b.y, b.z, b.x) < (q.y, q.z, q.x) });
        if better { best = Some(q); bd = d; }
    }
    best
}

/// BlockState.isSuffocating: by default "blocks motion and the collision shape is a full cube", with the per-block
/// exceptions of Blocks.java (never: glass, leaves, grates, mangrove roots, moving pistons; always: farmland, soul
/// sand, dirt path, mud; pistons only while not extended).
pub fn is_suffocating(s: S) -> bool {
    if !t().suffocating.is_empty() && !matches!(kind_of(s), Kind::PistonBaseBlock | Kind::ShulkerBoxBlock) { return t().suffocating[s as usize]; }
    static V: OnceLock<Vec<u8>> = OnceLock::new();
    let v = V.get_or_init(|| {
        t().blocks.iter().map(|b| {
            let n = b.name.as_str();
            if b.kind == Kind::LeavesBlock || n == "glass" || n.ends_with("stained_glass") || n == "tinted_glass" || n.ends_with("copper_grate") || n == "mangrove_roots" || n == "moving_piston" { 0 }
            else if n == "farmland" || n == "soul_sand" || n == "dirt_path" || n == "mud" { 1 }
            else if n == "piston" || n == "sticky_piston" { 3 }
            else { 2 }
        }).collect()
    });
    match v[block_of(s) as usize] {
        0 => false,
        1 => true,
        3 => !is_true(s, P_EXTENDED),
        _ => t().full_cube[s as usize],
    }
}

/// CollisionGetter.collidesWithSuffocatingBlock
pub fn collides_with_suffocating(w: &World, ctx: &Ctx, bx: &Aabb) -> bool {
    SCRATCH.with(|sc| {
        let mut v = sc.borrow_mut();
        colliders(w, ctx, bx, &mut v);
        v.iter().any(|(_, _, q)| is_suffocating(w.get(*q)))
    })
}

/// Direction.getApproximateNearest
pub fn approximate_nearest(dx: f64, dy: f64, dz: f64) -> Dir {
    let (x, y, z) = (dx as f32, dy as f32, dz as f32);
    let mut best = NORTH;
    let mut hi = f32::MIN_POSITIVE;
    for d in DIRS {
        let n = STEP[d as usize];
        let dot = x * n.0 as f32 + y * n.1 as f32 + z * n.2 as f32;
        if dot > hi { hi = dot; best = d; }
    }
    best
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub pos: P,
    pub face: Dir,
    pub loc: [f64; 3],
    pub inside: bool,
}

fn clip_boxes(boxes: &[[f64; 6]], off: [f64; 3], from: [f64; 3], to: [f64; 3]) -> Option<(f64, Dir)> {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let mut scale = 1.0f64;
    let mut dir: Option<Dir> = None;
    for b in boxes {
        let mn = [b[0] + off[0], b[1] + off[1], b[2] + off[2]];
        let mx = [b[3] + off[0], b[4] + off[1], b[5] + off[2]];
        // per axis a: the faces towards the ray; (a, b, c) cyclic as in AABB.getDirection
        for a in 0..3 {
            let bb = (a + 1) % 3;
            let cc = (a + 2) % 3;
            let (point, nd) = if d[a] > 1.0e-7 { (mn[a], [WEST, DOWN, NORTH][a]) } else if d[a] < -1.0e-7 { (mx[a], [EAST, UP, SOUTH][a]) } else { continue };
            let s = (point - from[a]) / d[a];
            let pb = from[bb] + s * d[bb];
            let pc = from[cc] + s * d[cc];
            if 0.0 < s && s < scale && mn[bb] - 1.0e-7 < pb && pb < mx[bb] + 1.0e-7 && mn[cc] - 1.0e-7 < pc && pc < mx[cc] + 1.0e-7 {
                scale = s;
                dir = Some(nd);
            }
        }
    }
    dir.map(|d| (scale, d))
}

fn shape_clip(sh: &VoxShape, off: [f64; 3], from: [f64; 3], to: [f64; 3], q: P) -> Option<Hit> {
    if sh.empty { return None; }
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < 1.0e-7 { return None; }
    let tp = [from[0] + d[0] * 0.001, from[1] + d[1] * 0.001, from[2] + d[2] * 0.001];
    // inside one of the shape's boxes?
    for b in &sh.boxes {
        // (findIndex semantics: lower bound inclusive, upper exclusive)
        if tp[0] >= b[0] + off[0] && tp[0] < b[3] + off[0] && tp[1] >= b[1] + off[1] && tp[1] < b[4] + off[1] && tp[2] >= b[2] + off[2] && tp[2] < b[5] + off[2] {
            return Some(Hit { pos: q, face: opposite(approximate_nearest(d[0], d[1], d[2])), loc: tp, inside: true });
        }
    }
    clip_boxes(&sh.boxes, off, from, to).map(|(s, dir)| Hit { pos: q, face: dir, loc: [from[0] + s * d[0], from[1] + s * d[1], from[2] + s * d[2]], inside: false })
}

fn lerp(t: f64, a: f64, b: f64) -> f64 { a + t * (b - a) }
fn frac(v: f64) -> f64 { v - (v.floor()) }

/// BlockGetter.clip with ClipContext.Block.OUTLINE and no fluids: the first block outline the segment meets.
pub fn clip(w: &World, ctx: &Ctx, from: [f64; 3], to: [f64; 3]) -> Option<Hit> {
    if from == to { return None; }
    let test = |q: P| -> Option<Hit> {
        let s = w.get(q);
        if s == 0 { return None; }
        let (sh, off) = out_shape(s, q, ctx);
        let r = shape_clip(sh, off, from, to, q)?;
        // getInteractionShape override: where the ray meets the interaction shape first, that decides the face
        let ish: Option<&VoxShape> = match kind_of(s) {
            Kind::ScaffoldingBlock | Kind::ComposterBlock => Some(&extra().full),
            Kind::CauldronBlock | Kind::LavaCauldronBlock | Kind::LayeredCauldronBlock => Some(&extra().cauldron_inside),
            Kind::HopperBlock => Some(&extra().hopper[dir_of(val(s, P_FACING)) as usize]),
            _ => None,
        };
        if let Some(ish) = ish {
            if let Some(o) = shape_clip(ish, [q.x as f64, q.y as f64, q.z as f64], from, to, q) {
                let d1 = (o.loc[0] - from[0]).powi(2) + (o.loc[1] - from[1]).powi(2) + (o.loc[2] - from[2]).powi(2);
                let d0 = (r.loc[0] - from[0]).powi(2) + (r.loc[1] - from[1]).powi(2) + (r.loc[2] - from[2]).powi(2);
                if d1 < d0 { return Some(Hit { face: o.face, ..r }); }
            }
        }
        Some(r)
    };
    let tx = lerp(-1.0e-7, to[0], from[0]); let ty = lerp(-1.0e-7, to[1], from[1]); let tz = lerp(-1.0e-7, to[2], from[2]);
    let fx = lerp(-1.0e-7, from[0], to[0]); let fy = lerp(-1.0e-7, from[1], to[1]); let fz = lerp(-1.0e-7, from[2], to[2]);
    let (mut cx, mut cy, mut cz) = (floor(fx), floor(fy), floor(fz));
    if let Some(h) = test(p(cx, cy, cz)) { return Some(h); }
    let (dx, dy, dz) = (tx - fx, ty - fy, tz - fz);
    let sg = |v: f64| -> i32 { if v == 0.0 { 0 } else if v > 0.0 { 1 } else { -1 } };
    let (sx, sy, sz) = (sg(dx), sg(dy), sg(dz));
    let tdx = if sx == 0 { f64::MAX } else { sx as f64 / dx };
    let tdy = if sy == 0 { f64::MAX } else { sy as f64 / dy };
    let tdz = if sz == 0 { f64::MAX } else { sz as f64 / dz };
    let mut ttx = tdx * if sx > 0 { 1.0 - frac(fx) } else { frac(fx) };
    let mut tty = tdy * if sy > 0 { 1.0 - frac(fy) } else { frac(fy) };
    let mut ttz = tdz * if sz > 0 { 1.0 - frac(fz) } else { frac(fz) };
    while ttx <= 1.0 || tty <= 1.0 || ttz <= 1.0 {
        if ttx < tty {
            if ttx < ttz { cx += sx; ttx += tdx; } else { cz += sz; ttz += tdz; }
        } else if tty < ttz { cy += sy; tty += tdy; } else { cz += sz; ttz += tdz; }
        if let Some(h) = test(p(cx, cy, cz)) { return Some(h); }
    }
    None
}

/// Entity.calculateViewVector
pub fn view_vector(pitch: f32, yaw: f32) -> [f64; 3] {
    let rx = pitch * (std::f64::consts::PI / 180.0) as f32;
    let ry = -yaw * (std::f64::consts::PI / 180.0) as f32;
    let yc = mcos(ry as f64);
    let ys = msin(ry as f64);
    let xc = mcos(rx as f64);
    let xs = msin(rx as f64);
    [(ys * xc) as f64, (-xs) as f64, (yc * xc) as f64]
}

fn between_corners_in_direction(c0: [i32; 3], c1: [i32; 3], dir: [f64; 3], out: &mut Vec<P>, seen: &mut Vec<P>, all: bool) {
    let mn = [c0[0].min(c1[0]), c0[1].min(c1[1]), c0[2].min(c1[2])];
    let mx = [c0[0].max(c1[0]), c0[1].max(c1[1]), c0[2].max(c1[2])];
    let start = [if dir[0] >= 0.0 { mn[0] } else { mx[0] }, if dir[1] >= 0.0 { mn[1] } else { mx[1] }, if dir[2] >= 0.0 { mn[2] } else { mx[2] }];
    let order: [usize; 3] = if dir[0].abs() < dir[2].abs() { [1, 2, 0] } else { [1, 0, 2] };
    let sg = |a: usize| -> i32 { if dir[a] >= 0.0 { 1 } else { -1 } };
    let (a1, a2, a3) = (order[0], order[1], order[2]);
    for i1 in 0..=(mx[a1] - mn[a1]) { for i2 in 0..=(mx[a2] - mn[a2]) { for i3 in 0..=(mx[a3] - mn[a3]) {
        let mut c = start;
        c[a1] += sg(a1) * i1; c[a2] += sg(a2) * i2; c[a3] += sg(a3) * i3;
        let q = p(c[0], c[1], c[2]);
        if all || !seen.contains(&q) { out.push(q); }
        if !seen.contains(&q) { seen.push(q); }
    } } }
}

/// BlockGetter.forEachBlockIntersectedBetween: the cells a box touches while moving from `from` to `to`
/// (`bb` is the box at the target), in the game's visiting order.
pub fn blocks_intersected_between(from: [f64; 3], to: [f64; 3], bb: &Aabb, out: &mut Vec<P>) {
    let tr = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let fl = |b: &Aabb| ([floor(b.min[0]), floor(b.min[1]), floor(b.min[2])], [floor(b.max[0]), floor(b.max[1]), floor(b.max[2])]);
    if tr[0] * tr[0] + tr[1] * tr[1] + tr[2] * tr[2] < (1.0e-5f32 as f64) * (1.0e-5f32 as f64) {
        let (a, b) = fl(bb);
        for x in a[0]..=b[0] { for y in a[1]..=b[1] { for z in a[2]..=b[2] { out.push(p(x, y, z)); } } }
        return;
    }
    let mut seen: Vec<P> = Vec::new();
    let start = bb.moved(-tr[0], -tr[1], -tr[2]);
    let (a, b) = fl(&start);
    between_corners_in_direction(a, b, tr, out, &mut seen, true);
    // addCollisionsAlongTravel
    let size = [bb.max[0] - bb.min[0], bb.max[1] - bb.min[1], bb.max[2] - bb.min[2]];
    let (xd, yd, zd) = (tr[0].abs(), tr[1].abs(), tr[2].abs());
    let (xs, ys, zs) = (if tr[0] >= 0.0 { 1 } else { -1 }, if tr[1] >= 0.0 { 1 } else { -1 }, if tr[2] >= 0.0 { 1 } else { -1 });
    let cd: [i32; 3] = if xd <= yd && xd <= zd { [-xs, -zs, ys] } else if yd <= zd { [zs, -ys, -xs] } else { [-ys, xs, -zs] };
    let ctr = [(bb.min[0] + bb.max[0]) / 2.0, (bb.min[1] + bb.max[1]) / 2.0, (bb.min[2] + bb.max[2]) / 2.0];
    let to_c = [ctr[0] + size[0] * 0.5 * cd[0] as f64, ctr[1] + size[1] * 0.5 * cd[1] as f64, ctr[2] + size[2] * 0.5 * cd[2] as f64];
    let from_c = [to_c[0] - tr[0], to_c[1] - tr[1], to_c[2] - tr[2]];
    let mut cv = [floor(from_c[0]), floor(from_c[1]), floor(from_c[2])];
    let sg = |v: f64| -> i32 { if v == 0.0 { 0 } else if v > 0.0 { 1 } else { -1 } };
    let sgn = [sg(tr[0]), sg(tr[1]), sg(tr[2])];
    let mut td = [0.0f64; 3];
    let mut tt = [0.0f64; 3];
    for a in 0..3 {
        td[a] = if sgn[a] == 0 { f64::MAX } else { sgn[a] as f64 / tr[a] };
        tt[a] = td[a] * if sgn[a] > 0 { 1.0 - frac(from_c[a]) } else { frac(from_c[a]) };
    }
    let mut guard = 0;
    while (tt[0] <= 1.0 || tt[1] <= 1.0 || tt[2] <= 1.0) && guard < 64 {
        guard += 1;
        if tt[0] < tt[1] {
            if tt[0] < tt[2] { cv[0] += sgn[0]; tt[0] += td[0]; } else { cv[2] += sgn[2]; tt[2] += td[2]; }
        } else if tt[1] < tt[2] { cv[1] += sgn[1]; tt[1] += td[1]; } else { cv[2] += sgn[2]; tt[2] += td[2]; }
        let unit = [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];
        if let Some((sc, _)) = clip_boxes(&unit, [cv[0] as f64, cv[1] as f64, cv[2] as f64], from_c, to_c) {
            let hit = [from_c[0] + sc * (to_c[0] - from_c[0]), from_c[1] + sc * (to_c[1] - from_c[1]), from_c[2] + sc * (to_c[2] - from_c[2])];
            let e = 1.0e-5f32 as f64;
            let mut opp = [0i32; 3];
            for a in 0..3 {
                let ch = hit[a].clamp(cv[a] as f64 + e, cv[a] as f64 + 1.0 - e);
                opp[a] = floor(ch - size[a] * cd[a] as f64);
            }
            between_corners_in_direction(cv, opp, tr, out, &mut seen, false);
        }
    }
    let (a, b) = fl(bb);
    between_corners_in_direction(a, b, tr, out, &mut seen, false);
}
