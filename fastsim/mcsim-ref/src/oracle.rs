//! The bot's knowledge of the placement rules (the Java version's `Oracle`): predictions for concrete clicks and
//! the per-item table of what each (face, part of face, view direction, sneaking) produces on a lone support block.
use crate::geom::*;
use crate::rules::{self, PlaceCtx};
use crate::tables::*;
use crate::world::World;
use std::collections::HashMap;

pub const NYAW: usize = 32;
pub const NPITCH: usize = 9;
pub const TABLE_SIZE: usize = 6 * 4 * NYAW * NPITCH * 2;

pub fn yaw_class(yaw: f32) -> usize {
    let mut y = yaw % 360f32;
    if y < -180f32 { y += 360f32; } else if y >= 180f32 { y -= 360f32; }
    let k = ((y + 180f32) / 11.25f32).floor() as i32;
    k.clamp(0, NYAW as i32 - 1) as usize
}
pub fn yaw_centre(k: usize) -> f32 { -180f32 + 5.625f32 + 11.25f32 * k as f32 }
/// Java's Math.round(float): floor(x + 0.5)
fn jround(x: f32) -> i32 { (x + 0.5f32).floor() as i32 }
pub fn pitch_class(pitch: f32) -> usize { jround((pitch + 80f32) / 20f32).clamp(0, NPITCH as i32 - 1) as usize }
pub fn pitch_centre(k: usize) -> f32 { -80f32 + 20f32 * k as f32 }
pub fn index(face: usize, quad: usize, yaw_class: usize, pitch_class: usize, sneak: bool) -> usize {
    (((face * 4 + quad) * NYAW + yaw_class) * NPITCH + pitch_class) * 2 + sneak as usize
}

/// Where the probe player (never part of the world) stands; only its view direction and sneak flag are set per question.
pub const PROBE_POS: [f64; 3] = [0.5, -60.0, 0.5];

pub struct Oracle {
    tables: HashMap<u16, std::rc::Rc<Vec<u16>>>,
    floats: Vec<u8>,
    scratch: World,
    pub predictions: u64,
    pub placed_at: P,
}

const SC: P = P { x: 2, y: 2, z: 2 };

impl Oracle {
    pub fn new() -> Oracle {
        Oracle { tables: HashMap::new(), floats: vec![0; t().n], scratch: World::new(0, 0, 0, 5, 5, 5, -1000, 100000), predictions: 0, placed_at: p(0, 0, 0) }
    }

    /// The state that would appear, and where (`placed_at`), if the item were used on the given hit; 0 if nothing would be placed.
    pub fn predict(&mut self, w: &World, item: u16, hit_pos: P, face: Dir, hit: [f64; 3], inside: bool, yaw: f32, pitch: f32, sneak: bool) -> S {
        self.predictions += 1;
        if item == 0 { return 0; }
        let ctx = PlaceCtx { item, hit_pos, face, hit, inside, yaw, pitch, sneak, player_pos: PROBE_POS, replace_clicked: true }.resolve(w);
        match rules::predict_place(w, &ctx) {
            Some((pos, st)) => { self.placed_at = pos; st }
            None => 0,
        }
    }

    /// Can this block state exist with nothing around it?
    pub fn floats(&mut self, state: S) -> bool {
        if self.floats[state as usize] == 0 {
            for c in self.scratch.cells.iter_mut() { *c = 0; }
            let ok = rules::can_survive(&self.scratch, state, SC);
            self.floats[state as usize] = if ok { 1 } else { 2 };
        }
        self.floats[state as usize] == 1
    }

    /// Would the block at one position still stand if the block at another (next to it) were taken away?
    pub fn survives_without(&mut self, w: &World, at: P, gone: P) -> bool {
        for c in self.scratch.cells.iter_mut() { *c = 0; }
        for dy in -2..=2 { for dz in -2..=2 { for dx in -2..=2 {
            let q = p(at.x + dx, at.y + dy, at.z + dz);
            let s = if q == gone { 0 } else { w.get(q) };
            self.scratch.put_raw(p(SC.x + dx, SC.y + dy, SC.z + dz), s);
        } } }
        let ok = rules::can_survive(&self.scratch, w.get(at), SC);
        for c in self.scratch.cells.iter_mut() { *c = 0; }
        ok
    }

    pub fn table(&mut self, item: u16) -> std::rc::Rc<Vec<u16>> {
        if let Some(tb) = self.tables.get(&item) { return tb.clone(); }
        let mut tbl = vec![0u16; TABLE_SIZE];
        if item != 0 {
            let bases = ["stone", "dirt", "farmland", "sand", "soul_sand", "end_stone", "jungle_log"];
            let mut scratch = std::mem::replace(&mut self.scratch, World::new(0, 0, 0, 1, 1, 1, -1000, 0));
            for base in bases {
                for c in scratch.cells.iter_mut() { *c = 0; }
                scratch.put_raw(SC, t().default_of(base));
                let mut any = false;
                for face in 0..6usize {
                    for quad in 0..4usize {
                        let a = if quad & 1 != 0 { 0.75 } else { 0.25 };
                        let b = if quad & 2 != 0 { 0.75 } else { 0.25 };
                        let (sx, sy, sz) = (SC.x as f64, SC.y as f64, SC.z as f64);
                        let loc = match face {
                            0 => [sx + a, sy, sz + b],
                            1 => [sx + a, sy + 1.0, sz + b],
                            2 => [sx + a, sy + b, sz],
                            3 => [sx + a, sy + b, sz + 1.0],
                            4 => [sx, sy + b, sz + a],
                            _ => [sx + 1.0, sy + b, sz + a],
                        };
                        for yc in 0..NYAW { for pc in 0..NPITCH { for sn in 0..2 {
                            let st = self.predict(&scratch, item, SC, face as Dir, loc, false, yaw_centre(yc), pitch_centre(pc), sn == 1);
                            if st != 0 { any = true; }
                            tbl[index(face, quad, yc, pc, sn == 1)] = st;
                        } } }
                    }
                }
                if any { break; }
            }
            for c in scratch.cells.iter_mut() { *c = 0; }
            self.scratch = scratch;
        }
        let rc = std::rc::Rc::new(tbl);
        self.tables.insert(item, rc.clone());
        rc
    }

    /// The Java version's `Spec.placeable` for a target state's first-click state: does any table entry give it (loosely)?
    pub fn placeable(&mut self, item: u16, first: S) -> bool {
        let tb = self.table(item);
        let want = t().loose[first as usize];
        tb.iter().any(|&e| e != 0 && t().loose[e as usize] == want)
    }
}
