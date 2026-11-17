//! The bot: a port of the Java version's builder (`mcb.bot`), class by class:
//! grid.rs (Grid), ray.rs (Ray), body.rs (Body), nav.rs (Nav), state.rs (BuildState), moves.rs (Moves),
//! placing.rs (Placing), stances.rs (Stances), scaffolds.rs (Scaffolds), supplies.rs (Supplies), builder.rs (Builder).
//! The Java inheritance chain is one struct here; method names follow the original.
#![allow(dead_code, clippy::too_many_arguments)]
pub mod body;
pub mod builder;
pub mod dry;
pub mod grid;
pub mod moves;
pub mod nav;
pub mod placing;
pub mod ray;
pub mod scaffolds;
pub mod stances;
pub mod state;
pub mod supplies;

pub use state::Bot;

/// Thrown (as a panic payload) when the tick limit is reached, like the Java version's `OutOfTime`.
pub struct OutOfTime;

pub const NONE: i32 = i32::MAX;
pub const PLACED: i32 = 0;
pub const DEFER: i32 = 1;
pub const SKIP: i32 = 2;

pub const DX: [i32; 6] = [0, 0, 0, 0, -1, 1];
pub const DY: [i32; 6] = [-1, 1, 0, 0, 0, 0];
pub const DZ: [i32; 6] = [0, 0, -1, 1, 0, 0];
pub const OPP: [usize; 6] = [1, 0, 3, 2, 5, 4];
pub const DXH: [i32; 4] = [1, -1, 0, 0];
pub const DZH: [i32; 4] = [0, 0, 1, -1];

/// Java's Math.round(double)
#[inline]
pub fn jround(v: f64) -> i64 { (v + 0.5).floor() as i64 }
#[inline]
pub fn yaw_to(dx: f64, dz: f64) -> f32 { (-dx).atan2(dz).to_degrees() as f32 }
#[inline]
pub fn pitch_to(dy: f64, hd: f64) -> f32 { (-(dy.atan2(hd).to_degrees())) as f32 }
