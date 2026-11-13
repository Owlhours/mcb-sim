//! Non-redstone block behaviour (stub).
#![allow(dead_code, unused_variables)]
use super::{Clicker, PlaceCtx, Use};
use crate::geom::*;
use crate::tables::*;
use crate::world::World;

pub fn update_shape(w: &mut World, s: S, pos: P, dir: Dir, npos: P, ns: S) -> S { s }
pub fn neighbor_changed(w: &mut World, s: S, pos: P, from_block: u16, moved: bool) {}
pub fn on_place(w: &mut World, s: S, pos: P, old: S, moved: bool) {}
pub fn affect_neighbors_after_removal(w: &mut World, old: S, pos: P, moved: bool) {}
pub fn tick(w: &mut World, s: S, pos: P) {}
pub fn trigger_event(w: &mut World, s: S, pos: P, b0: u8, b1: u8) -> bool { false }
pub fn set_placed_by(w: &mut World, pos: P, s: S, ctx: &PlaceCtx) {}
pub fn player_will_destroy(w: &mut World, pos: P, s: S) {}
pub fn destroyed(w: &mut World, pos: P, s: S) {}
pub fn attack(w: &mut World, s: S, pos: P) {}
pub fn use_item_on(w: &mut World, s: S, pos: P, face: Dir, hit: [f64; 3], who: &mut Clicker) -> Use { Use::TryEmptyHand }
pub fn use_without_item(w: &mut World, s: S, pos: P, face: Dir, hit: [f64; 3], who: &mut Clicker) -> Use { Use::Pass }
pub fn step_on(w: &mut World, s: S, pos: P) {}
pub fn entity_inside(w: &mut World, s: S, pos: P) {}
