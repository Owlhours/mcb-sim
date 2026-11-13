//! Redstone (stub).
#![allow(dead_code, unused_variables)]
use super::{Clicker, PlaceCtx, Use};
use crate::geom::*;
use crate::tables::*;
use crate::world::World;

pub fn update_shape(w: &mut World, s: S, pos: P, dir: Dir, npos: P, ns: S) -> Option<S> { None }
pub fn neighbor_changed(w: &mut World, s: S, pos: P, from_block: u16, moved: bool) -> bool { false }
pub fn on_place(w: &mut World, s: S, pos: P, old: S, moved: bool) -> bool { false }
pub fn affect_neighbors_after_removal(w: &mut World, old: S, pos: P, moved: bool) -> bool { false }
pub fn update_indirect_neighbour_shapes(w: &mut World, s: S, pos: P, flags: u32, limit: i32) {}
pub fn tick(w: &mut World, s: S, pos: P) -> bool { false }
pub fn trigger_event(w: &mut World, s: S, pos: P, b0: u8, b1: u8) -> Option<bool> { None }
pub fn set_placed_by(w: &mut World, pos: P, s: S, ctx: &PlaceCtx) -> bool { false }
pub fn attack(w: &mut World, s: S, pos: P) -> bool { false }
pub fn use_item_on(w: &mut World, s: S, pos: P, face: Dir, hit: [f64; 3], who: &mut Clicker) -> Option<Use> { None }
pub fn use_without_item(w: &mut World, s: S, pos: P, face: Dir, hit: [f64; 3], who: &mut Clicker) -> Option<Use> { None }
pub fn step_on(w: &mut World, s: S, pos: P) -> bool { false }
pub fn entity_inside(w: &mut World, s: S, pos: P) -> bool { false }
pub fn has_analog_output(s: S) -> bool { false }
pub fn update_neighbour_for_output_signal(w: &mut World, pos: P, block: u16) {}
pub fn has_neighbor_signal(w: &World, pos: P) -> bool { false }
