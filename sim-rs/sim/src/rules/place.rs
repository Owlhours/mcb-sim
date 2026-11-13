//! Placement rules (stub).
#![allow(dead_code, unused_variables)]
use super::PlaceCtx;
use crate::geom::*;
use crate::tables::*;
use crate::world::World;

pub fn predict_place(w: &World, ctx: &PlaceCtx) -> Option<(P, S)> {
    // TEMPORARY smoke-test rule: plain blocks only
    let tb = t();
    let pos = ctx.pos();
    if !(ctx.replace_clicked || tb.replaceable[w.get(pos) as usize]) { return None; }
    let it = &tb.items[ctx.item as usize];
    let b = &tb.blocks[*it.blocks.first()? as usize];
    let st = b.default;
    if !w.is_unobstructed(st, pos) { return None; }
    Some((pos, st))
}
pub fn can_be_replaced(w: &World, s: S, pos: P, ctx: &PlaceCtx) -> bool { t().replaceable[s as usize] }
pub fn can_survive(w: &World, s: S, pos: P) -> bool { true }
pub fn face_sturdy(s: S, dir: Dir, kind: u8) -> bool { let tb = t(); tb.shapes[tb.col_shape[s as usize] as usize].face_support(dir, kind) }
pub fn placement_collision_shape(s: S) -> u32 { t().col_shape[s as usize] }
pub fn item_place_block(w: &mut World, ctx: &PlaceCtx, pos: P, state: S) -> bool { w.set_block(pos, state, 11) }
