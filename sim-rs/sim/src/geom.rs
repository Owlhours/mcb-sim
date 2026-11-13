//! Positions, directions, boxes.
#![allow(dead_code)]
use crate::props::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct P {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
#[inline]
pub fn p(x: i32, y: i32, z: i32) -> P { P { x, y, z } }

/// Directions in the game's 3D data order.
pub type Dir = u8;
pub const DOWN: Dir = 0;
pub const UP: Dir = 1;
pub const NORTH: Dir = 2;
pub const SOUTH: Dir = 3;
pub const WEST: Dir = 4;
pub const EAST: Dir = 5;
pub const DIRS: [Dir; 6] = [DOWN, UP, NORTH, SOUTH, WEST, EAST];
/// Direction.Plane.HORIZONTAL iteration order.
pub const HORIZONTAL: [Dir; 4] = [NORTH, EAST, SOUTH, WEST];
pub const STEP: [(i32, i32, i32); 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];
/// BlockBehaviour.UPDATE_SHAPE_ORDER
pub const UPDATE_SHAPE_ORDER: [Dir; 6] = [WEST, EAST, NORTH, SOUTH, DOWN, UP];
/// NeighborUpdater.UPDATE_ORDER
pub const NEIGHBOR_UPDATE_ORDER: [Dir; 6] = [WEST, EAST, DOWN, UP, NORTH, SOUTH];
pub const AXIS_X: u8 = 0;
pub const AXIS_Y: u8 = 1;
pub const AXIS_Z: u8 = 2;

impl P {
    #[inline]
    pub fn rel(self, d: Dir) -> P { let s = STEP[d as usize]; P { x: self.x + s.0, y: self.y + s.1, z: self.z + s.2 } }
    #[inline]
    pub fn rel_n(self, d: Dir, n: i32) -> P { let s = STEP[d as usize]; P { x: self.x + s.0 * n, y: self.y + s.1 * n, z: self.z + s.2 * n } }
    #[inline]
    pub fn below(self) -> P { P { x: self.x, y: self.y - 1, z: self.z } }
    #[inline]
    pub fn above(self) -> P { P { x: self.x, y: self.y + 1, z: self.z } }
    #[inline]
    pub fn off(self, dx: i32, dy: i32, dz: i32) -> P { P { x: self.x + dx, y: self.y + dy, z: self.z + dz } }
}

#[inline]
pub fn opposite(d: Dir) -> Dir { d ^ 1 }
#[inline]
pub fn axis(d: Dir) -> u8 { match d { DOWN | UP => AXIS_Y, NORTH | SOUTH => AXIS_Z, _ => AXIS_X } }
#[inline]
pub fn is_horizontal(d: Dir) -> bool { d >= 2 }
#[inline]
pub fn is_positive(d: Dir) -> bool { d & 1 == 1 }
/// Direction.getClockWise (around Y), horizontal directions only.
pub fn clockwise(d: Dir) -> Dir { match d { NORTH => EAST, EAST => SOUTH, SOUTH => WEST, WEST => NORTH, _ => panic!("clockwise of vertical") } }
pub fn counter_clockwise(d: Dir) -> Dir { match d { NORTH => WEST, WEST => SOUTH, SOUTH => EAST, EAST => NORTH, _ => panic!("ccw of vertical") } }
/// Direction.get2DDataValue: south 0, west 1, north 2, east 3.
pub fn data2d(d: Dir) -> u8 { match d { SOUTH => 0, WEST => 1, NORTH => 2, EAST => 3, _ => 255 } }
pub fn from_2d(i: i32) -> Dir { [SOUTH, WEST, NORTH, EAST][(i.rem_euclid(4)) as usize] }
/// Direction.toYRot
pub fn to_yrot(d: Dir) -> f32 { (data2d(d) & 3) as f32 * 90.0 }
/// Direction.fromYRot
pub fn from_yrot(yrot: f64) -> Dir { from_2d((yrot / 90.0 + 0.5).floor() as i32 & 3) }

/// Value code of a facing-like property -> direction.
pub fn dir_of(v: u8) -> Dir {
    match v { V_DOWN => DOWN, V_UP => UP, V_NORTH => NORTH, V_SOUTH => SOUTH, V_WEST => WEST, V_EAST => EAST, _ => panic!("not a direction value: {v}") }
}
pub fn dir_val(d: Dir) -> u8 { [V_DOWN, V_UP, V_NORTH, V_SOUTH, V_WEST, V_EAST][d as usize] }
/// Value code of an axis property (x/y/z) for an axis number.
pub fn axis_val(a: u8) -> u8 { [V_X, V_Y, V_Z][a as usize] }
pub fn axis_of_val(v: u8) -> u8 { match v { V_X => AXIS_X, V_Y => AXIS_Y, V_Z => AXIS_Z, _ => panic!("not an axis value") } }
/// The boolean property named after a direction (north/east/south/west/up/down).
pub fn dir_prop(d: Dir) -> u8 { [P_DOWN, P_UP, P_NORTH, P_SOUTH, P_WEST, P_EAST][d as usize] }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: [f64; 3],
    pub max: [f64; 3],
}
impl Aabb {
    #[inline]
    pub fn new(x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) -> Aabb { Aabb { min: [x0, y0, z0], max: [x1, y1, z1] } }
    #[inline]
    pub fn moved(&self, dx: f64, dy: f64, dz: f64) -> Aabb { Aabb { min: [self.min[0] + dx, self.min[1] + dy, self.min[2] + dz], max: [self.max[0] + dx, self.max[1] + dy, self.max[2] + dz] } }
    /// AABB.expandTowards
    pub fn expand_towards(&self, dx: f64, dy: f64, dz: f64) -> Aabb {
        let mut r = *self;
        let d = [dx, dy, dz];
        for i in 0..3 { if d[i] < 0.0 { r.min[i] += d[i]; } else if d[i] > 0.0 { r.max[i] += d[i]; } }
        r
    }
    pub fn inflate(&self, a: f64) -> Aabb { Aabb { min: [self.min[0] - a, self.min[1] - a, self.min[2] - a], max: [self.max[0] + a, self.max[1] + a, self.max[2] + a] } }
    pub fn deflate(&self, a: f64) -> Aabb { self.inflate(-a) }
    /// AABB.intersects (strict)
    #[inline]
    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min[0] < o.max[0] && self.max[0] > o.min[0] && self.min[1] < o.max[1] && self.max[1] > o.min[1] && self.min[2] < o.max[2] && self.max[2] > o.min[2]
    }
    #[inline]
    pub fn intersects_cube(&self, x: i32, y: i32, z: i32) -> bool {
        self.min[0] < x as f64 + 1.0 && self.max[0] > x as f64 && self.min[1] < y as f64 + 1.0 && self.max[1] > y as f64 && self.min[2] < z as f64 + 1.0 && self.max[2] > z as f64
    }
}

/// Mth.floor
#[inline]
pub fn floor(v: f64) -> i32 { let i = v as i32; if v < i as f64 { i - 1 } else { i } }
