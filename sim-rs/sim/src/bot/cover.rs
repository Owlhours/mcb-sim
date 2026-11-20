//! (candidate `covertour`) Stands chosen by the cells they serve, in the main pass of a build behind its pace.
//! Measured on three huge builds out of time (MCSIM_STANDLOG, MCSIM_COVERLOG): two thirds to four fifths of the stands
//! are on the build's own finished blocks already, but each serves 3.7-5.6 cells, while the best spot within four
//! blocks of a departure would serve 8-11 at once under the bot's own rules (the layer rule, kinds carried, put-off
//! cells). So: before the next cell is picked, a walk (no scaffolding) to the spot nearby that serves the most cells
//! now, and `sweep_here` there - stand by stand over what the bot stands on, the build itself.
//! (Screened out beside it: the same score for the choice among the spots that see the cell in hand, and for the
//! choice of a column - each a few per cent at best.)
use super::state::Bot;
use super::*;
use crate::tables::t;

impl Bot {
    /// Is the candidate `name` on here: the main pass of a build behind its pace.
    pub fn cover_on(&mut self, name: &str) -> bool {
        (if name == "covertour" { crate::rev::rev().cover_tour } else { crate::cand::on(name) }) && crate::ckpt::armed(self.ticks) && !self.main_done && !self.cleaning && self.col_next_set && self.behind_pace()
    }

    /// The cells other than `skip` that a click from the eye of one standing at (sx, sy, sz) would place now under the
    /// bot's own rules: the next cell of its column, the layer rule kept, a kind it carries, not put off, a falling
    /// block only on something that holds it, in reach and with a face to click (what `sweep_here` places from there).
    /// Stops counting at `cap`.
    pub fn cover_at(&mut self, sx: i32, sy: i32, sz: i32, skip: usize, cap: i64) -> i64 {
        let h = self.g.stand16(sx, sy, sz);
        if h < 0 { return 0; }
        let tb = t();
        let saved = self.click.clone();
        let (ex, ey, ez) = (sx as f64 + 0.5, sy as f64 + h as f64 / 16.0 + 1.62, sz as f64 + 0.5);
        let r2 = (self.reach + 0.9) * (self.reach + 0.9);
        let mut n = 0;
        'all: for z in sz - 4..=sz + 4 { for x in sx - 4..=sx + 4 {
            if !self.g.in_box(x, self.g.oy, z) { continue; }
            let y = self.col_next[self.col(x, z)];
            if y == NONE || y > sy + 5 || y < sy - 4 { continue; }
            if x == sx && z == sz && (y == sy || y == sy + 1) { continue; }
            let i = self.g.idx(x, y, z);
            if i == skip || self.g.cur[i] != 0 || self.deferred.contains(i as i32) { continue; }
            let (cx, cy, cz) = (x as f64 + 0.5 - ex, y as f64 + 0.5 - ey, z as f64 + 0.5 - ez);
            if cx * cx + cy * cy + cz * cz > r2 { continue; }
            let tg = self.g.tgt[i];
            let sp = self.spec(tg);
            if !sp.placeable || self.count(sp.item) == 0 || !self.eligible(x, z) { continue; }
            if tb.gravity[tg as usize] { let b = self.g.get(x, y - 1, z) as usize; if b == 0 || tb.walk_through[b] || tb.replaceable[b] { continue; } }
            if self.blocked.contains(i as i32) && self.blocked.get(i as i32) == self.surroundings(i) { continue; }
            if self.scaffold_in_column[self.col(x, z)] != 0 && self.loose_below(x, y, z) { continue; }
            if self.find_click(x, y, z, sp.first, sp.table.as_ref().map(|v| v.as_slice()), ex, ey, ez, false, 63) { n += 1; if n >= cap { break 'all; } }
        } }
        self.click = saved;
        n
    }

    /// (candidate `covertour`) One step of the tour: the spot within four blocks (MCSIM_COVR) (three up or down) that serves the most
    /// cells now (MCSIM_COVW quarters of a block a cell, 8), less its distance, if it serves MCSIM_COVK (3) or more; walked to without scaffolding and swept.
    /// True when cells were placed there.
    pub fn cover_tour(&mut self) -> bool {
        if !self.cover_on("covertour") { return false; }
        if crate::rev::rev().a1 { self.guard_health(); }
        // what is allowed from here goes first (the step must not walk off from cells in reach)
        self.sweep_here();
        if !self.c.on_ground() { return false; }
        let wq = crate::cand::num("MCSIM_COVW", 8) as i32;
        let kmin = crate::cand::num("MCSIM_COVK", 3);
        let rad = crate::cand::num("MCSIM_COVR", 4) as i32;
        let dyr = crate::cand::num("MCSIM_COVDY", 3) as i32;
        let (bx, by, bz) = (self.cx(), self.cy(), self.cz());
        let now = self.ticks;
        self.cover_dud.retain(|_, t| *t > now);
        let mut cands: Vec<(i32, i32, i32, i32)> = Vec::new();
        for sy in by - dyr..=by + dyr { for sz in bz - rad..=bz + rad { for sx in bx - rad..=bx + rad {
            if (sx, sy, sz) == (bx, by, bz) || !self.g.inside(sx, sy, sz) || self.g.stand16(sx, sy, sz) < 0 { continue; }
            let k = self.g.idx_raw(sx, sy, sz);
            if self.bad_stand.contains(&k) || self.cover_dud.contains_key(&k) { continue; }
            // no stand on a piece of the bot's own: the tour goes over the build and the ground
            if self.g.inside(sx, sy - 1, sz) && self.scaffold.contains(self.g.idx(sx, sy - 1, sz) as i32) { continue; }
            let e = (sx - bx).abs().max((sz - bz).abs()) + 2 * 0.max(sy - by) + 0.max(by - sy);
            let n = self.cover_at(sx, sy, sz, usize::MAX, 64);
            if n < kmin { continue; }
            let score = wq * n as i32 - 4 * e;
            if cands.len() >= 3 && score <= cands[2].3 { continue; }
            cands.push((sx, sy, sz, score));
            cands.sort_by(|a, b| b.3.cmp(&a.3));
            cands.truncate(3);
        } } }
        for &(sx, sy, sz, _) in cands.iter() {
            // (the way there, and the route `go_to` makes again, with nothing new put up: no column, pillar or bridge)
            self.no_scaffold_nav = true;
            let ok = self.route(sx, sy, sz, 4000);
            if !ok { self.no_scaffold_nav = false; continue; }
            self.cover_moves += 1;
            let st = super::state::Stance { x: sx, y: sy, z: sz, lean: -1, cost: self.nav.goal_cost, ..Default::default() };
            let gone = self.go_to(&st);
            self.no_scaffold_nav = false;
            let dud = self.g.idx_raw(sx, sy, sz);
            if !gone { self.cover_fails += 1; self.cover_dud.insert(dud, now + 20000); return false; }
            // (counted: target cells the sweep placed, not blocks put down on the way)
            let s0 = self.cover_swept;
            self.sweep_here();
            let got = self.cover_swept - s0;
            self.cover_placed += got;
            if got == 0 { self.cover_fails += 1; self.cover_dud.insert(dud, now + 20000); return false; }
            return true;
        }
        false
    }
}
