//! Block shapes as the game's voxel shapes: coordinate lists per axis plus a set of filled cells. Collision
//! resolution, overlap tests and face coverage follow VoxelShape / Shapes / SupportType.
use crate::geom::*;

#[derive(Clone, Debug)]
pub struct VoxShape {
    pub boxes: Vec<[f64; 6]>,
    pub coords: [Vec<f64>; 3],
    pub filled: Vec<bool>,
    pub size: [usize; 3],
    pub empty: bool,
    pub full: bool,
    pub min: [f64; 3],
    pub max: [f64; 3],
    /// extends beyond its own cell (Cache.largeCollisionShape)
    pub large: bool,
}

const EPS: f64 = 1.0e-7;

impl VoxShape {
    pub fn from_boxes(boxes: &[[f64; 6]]) -> VoxShape {
        let mut coords: [Vec<f64>; 3] = [vec![], vec![], vec![]];
        for b in boxes {
            for a in 0..3 { coords[a].push(b[a]); coords[a].push(b[a + 3]); }
        }
        for a in 0..3 {
            coords[a].sort_by(|x, y| x.partial_cmp(y).unwrap());
            coords[a].dedup_by(|x, y| (*x - *y).abs() < EPS);
        }
        let empty = boxes.is_empty();
        let size = if empty { [0, 0, 0] } else { [coords[0].len() - 1, coords[1].len() - 1, coords[2].len() - 1] };
        let mut filled = vec![false; size[0] * size[1] * size[2]];
        for b in boxes {
            for x in 0..size[0] {
                let mx = (coords[0][x] + coords[0][x + 1]) / 2.0;
                if mx < b[0] || mx > b[3] { continue; }
                for y in 0..size[1] {
                    let my = (coords[1][y] + coords[1][y + 1]) / 2.0;
                    if my < b[1] || my > b[4] { continue; }
                    for z in 0..size[2] {
                        let mz = (coords[2][z] + coords[2][z + 1]) / 2.0;
                        if mz < b[2] || mz > b[5] { continue; }
                        filled[(x * size[1] + y) * size[2] + z] = true;
                    }
                }
            }
        }
        let mut min = [f64::MAX; 3];
        let mut max = [-f64::MAX; 3];
        for b in boxes { for a in 0..3 { min[a] = min[a].min(b[a]); max[a] = max[a].max(b[a + 3]); } }
        let full = boxes.len() == 1 && boxes[0] == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let large = !empty && (0..3).any(|a| min[a] < 0.0 || max[a] > 1.0);
        VoxShape { boxes: boxes.to_vec(), coords, filled, size, empty, full, min, max, large }
    }

    #[inline]
    fn is_full(&self, i: [isize; 3]) -> bool {
        if i[0] < 0 || i[1] < 0 || i[2] < 0 { return false; }
        let (x, y, z) = (i[0] as usize, i[1] as usize, i[2] as usize);
        if x >= self.size[0] || y >= self.size[1] || z >= self.size[2] { return false; }
        self.filled[(x * self.size[1] + y) * self.size[2] + z]
    }

    /// VoxelShape.findIndex for the shape moved by `off`: index of the last coordinate <= v (-1 .. size).
    #[inline]
    fn find_index(&self, a: usize, v: f64, off: f64) -> isize {
        let c = &self.coords[a];
        let mut n = 0isize;
        for x in c { if v < *x + off { break; } n += 1; }
        n - 1
    }

    /// VoxelShape.collide: how far `moving` can travel along `axis` (signed distance) before meeting this shape placed at `off`.
    pub fn collide(&self, axis: usize, moving: &Aabb, off: [f64; 3], mut distance: f64) -> f64 {
        if self.empty { return distance; }
        if distance.abs() < EPS { return 0.0; }
        let a = axis;
        let b = (axis + 1) % 3;
        let c = (axis + 2) % 3;
        let max_a = moving.max[a];
        let min_a = moving.min[a];
        let a_min = self.find_index(a, min_a + EPS, off[a]);
        let a_max = self.find_index(a, max_a - EPS, off[a]);
        let b_min = self.find_index(b, moving.min[b] + EPS, off[b]).max(0);
        let b_max = (self.find_index(b, moving.max[b] - EPS, off[b]) + 1).min(self.size[b] as isize);
        let c_min = self.find_index(c, moving.min[c] + EPS, off[c]).max(0);
        let c_max = (self.find_index(c, moving.max[c] - EPS, off[c]) + 1).min(self.size[c] as isize);
        let a_size = self.size[a] as isize;
        let mut idx = [0isize; 3];
        if distance > 0.0 {
            let mut ai = a_max + 1;
            while ai < a_size {
                for bi in b_min..b_max {
                    for ci in c_min..c_max {
                        idx[a] = ai; idx[b] = bi; idx[c] = ci;
                        if self.is_full(idx) {
                            let nd = self.coords[a][ai as usize] + off[a] - max_a;
                            if nd >= -EPS { distance = distance.min(nd); }
                            return distance;
                        }
                    }
                }
                ai += 1;
            }
        } else if distance < 0.0 {
            let mut ai = a_min - 1;
            while ai >= 0 {
                for bi in b_min..b_max {
                    for ci in c_min..c_max {
                        idx[a] = ai; idx[b] = bi; idx[c] = ci;
                        if self.is_full(idx) {
                            let nd = self.coords[a][ai as usize + 1] + off[a] - min_a;
                            if nd <= EPS { distance = distance.max(nd); }
                            return distance;
                        }
                    }
                }
                ai -= 1;
            }
        }
        distance
    }

    /// Shapes.joinIsNotEmpty(shape moved by off, box, AND): do they overlap (by more than the game's tolerance)?
    pub fn intersects(&self, off: [f64; 3], bx: &Aabb) -> bool {
        if self.empty { return false; }
        for b in &self.boxes {
            let mut hit = true;
            for a in 0..3 {
                if !(b[a + 3] + off[a] - bx.min[a] > EPS && bx.max[a] - (b[a] + off[a]) > EPS) { hit = false; break; }
            }
            if hit { return true; }
        }
        false
    }

    /// Is the given region of the face towards `dir` fully covered? kind: 0 FULL, 1 CENTER, 2 RIGID (SupportType).
    pub fn face_support(&self, dir: Dir, kind: u8) -> bool {
        if self.empty { return false; }
        if self.full { return true; }
        let a = axis(dir) as usize;
        let b = (a + 1) % 3;
        let c = (a + 2) % 3;
        // the slice of cells at the far end of the axis (VoxelShape.calculateFace)
        let ai: isize = if self.coords[a].len() == 2 && self.coords[a][0].abs() < EPS && (self.coords[a][1] - 1.0).abs() < EPS {
            0
        } else {
            self.find_index(a, if is_positive(dir) { 0.9999999 } else { 1.0e-7 }, 0.0)
        };
        if ai < 0 || ai >= self.size[a] as isize { return false; }
        // region to be covered, in the (b, c) plane of the unit square, on a grid merged from the shape's coordinates and the region's
        let mut gb: Vec<f64> = self.coords[b].clone();
        let mut gc: Vec<f64> = self.coords[c].clone();
        // region rectangles as [b0,b1,c0,c1] in block-local coordinates
        let col = |ax: usize, lo: f64, hi: f64, ylo: f64, yhi: f64| -> (f64, f64) { if ax == 1 { (ylo, yhi) } else { (lo, hi) } };
        let regions: Vec<[f64; 4]> = match kind {
            0 => vec![[0.0, 1.0, 0.0, 1.0]],
            1 => {
                // Block.column(2, 0, 10): x,z in 7/16..9/16, y in 0..10/16
                let (b0, b1) = col(b, 7.0 / 16.0, 9.0 / 16.0, 0.0, 10.0 / 16.0);
                let (c0, c1) = col(c, 7.0 / 16.0, 9.0 / 16.0, 0.0, 10.0 / 16.0);
                vec![[b0, b1, c0, c1]]
            }
            _ => {
                // the unit cube minus box(2,0,2,14,16,14): seen along Y that is the rim; seen along a horizontal axis it
                // projects onto the whole face
                if a == 1 {
                    let (lo, hi) = (2.0 / 16.0, 14.0 / 16.0);
                    vec![[0.0, 1.0, 0.0, lo], [0.0, 1.0, hi, 1.0], [0.0, lo, lo, hi], [hi, 1.0, lo, hi]]
                } else {
                    vec![[0.0, 1.0, 0.0, 1.0]]
                }
            }
        };
        for r in &regions { gb.push(r[0]); gb.push(r[1]); gc.push(r[2]); gc.push(r[3]); }
        gb.push(0.0); gb.push(1.0); gc.push(0.0); gc.push(1.0);
        gb.sort_by(|x, y| x.partial_cmp(y).unwrap()); gb.dedup_by(|x, y| (*x - *y).abs() < EPS);
        gc.sort_by(|x, y| x.partial_cmp(y).unwrap()); gc.dedup_by(|x, y| (*x - *y).abs() < EPS);
        for ib in 0..gb.len() - 1 {
            let mb = (gb[ib] + gb[ib + 1]) / 2.0;
            for ic in 0..gc.len() - 1 {
                let mc = (gc[ic] + gc[ic + 1]) / 2.0;
                if !regions.iter().any(|r| mb > r[0] && mb < r[1] && mc > r[2] && mc < r[3]) { continue; }
                let bi = self.find_index(b, mb, 0.0);
                let ci = self.find_index(c, mc, 0.0);
                let mut idx = [0isize; 3];
                idx[a] = ai; idx[b] = bi; idx[c] = ci;
                if !self.is_full(idx) { return false; }
            }
        }
        true
    }
}
