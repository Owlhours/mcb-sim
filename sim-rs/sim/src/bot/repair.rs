//! The repair pass (revision "repair"; from the owner's monitor, `FROM-MONITOR-repair.md`): when the bot's
//! programme is over and sections still fail by its own picture, it takes them up again with its own routines.
//! Up to three rounds: if more than six sections fail and more than 150,000 ticks are left the whole build goes back
//! into the main programme (the bot had stopped early); otherwise the failing sections with the fewest bad cells, up
//! to 24, sections with machine parts left alone. Their bad cells are forgotten as "given up" and built again by the
//! main loop, then tried with the helper rules and with the trap rules; what is then still bad and shut in by standing
//! blocks is reached by a short shaft (revision "excavate"); then the whole end game runs again. A round that does not
//! lower the bad cells is the last.
use super::state::Bot;
use super::*;
use crate::kinds::Kind;
use crate::tables::*;

pub struct Sec { pub bad: i64, pub missing_wrong: i64, pub machine: bool, pub b: [i32; 6] }

impl Bot {
    /// What the judge would say of a cell, by the bot's picture: 0 right or not judged, 1 missing, 2 wrong, 3 left over.
    pub fn bad_kind(&self, i: usize) -> u8 {
        let tb = t();
        let a = self.g.tgt[i] as usize;
        let mut w = self.g.cur[i] as usize;
        if tb.air[w] { w = 0; }
        if a != 0 && (crate::build::decorative(a) || crate::build::game_master(a)) { return 0; }
        if a == 0 { return if w != 0 && !tb.fluid[w] && !tb.technical[w] && !tb.secondary[w] { 3 } else { 0 }; }
        if w == 0 { return 1; }
        if a == w || tb.canon[a] == tb.canon[w] { 0 } else { 2 }
    }

    /// The sections that fail (more than 3 % of their judged target cells bad), and the bad cells of the whole build.
    pub fn failing_sections(&self) -> (Vec<Sec>, i64) {
        let tb = t();
        let g = &self.g;
        let (nx, ny, nz) = ((g.sx + 15) >> 4, (g.sy + 15) >> 4, (g.sz + 15) >> 4);
        let mut out = Vec::new();
        let mut total = 0i64;
        for cy in 0..ny { for cz in 0..nz { for cx in 0..nx {
            let b = [g.ox + (cx << 4), g.oy + (cy << 4), g.oz + (cz << 4), g.ox + g.sx.min((cx + 1) << 4) - 1, g.oy + g.sy.min((cy + 1) << 4) - 1, g.oz + g.sz.min((cz + 1) << 4) - 1];
            let (mut tgt, mut bad, mut mw, mut machine) = (0i64, 0i64, 0i64, false);
            for y in b[1]..=b[4] { for z in b[2]..=b[5] { for x in b[0]..=b[3] {
                let i = g.idx(x, y, z);
                let a = g.tgt[i] as usize;
                if a != 0 && !crate::build::decorative(a) && !crate::build::game_master(a) { tgt += 1; }
                if a != 0 && (matches!(tb.kind(a as u16), Kind::PistonBaseBlock | Kind::PistonHeadBlock | Kind::ObserverBlock) || crate::is_blk!(a as u16, "slime_block") || crate::is_blk!(a as u16, "honey_block")) { machine = true; }
                let k = self.bad_kind(i);
                if k != 0 { bad += 1; if k != 3 { mw += 1; } }
            } } }
            total += bad;
            let fails = if tgt == 0 { bad > 0 } else { bad as f64 / tgt as f64 > 0.03 };
            if fails { out.push(Sec { bad, missing_wrong: mw, machine, b }); }
        } } }
        (out, total)
    }

    /// Puts the bad target cells of a box back into the ordinary programme: no longer given up, put off or blocked,
    /// and the columns' next cell set back to the lowest of them.
    fn requeue(&mut self, b: [i32; 6]) -> i64 {
        let mut n = 0;
        for y in b[1]..=b[4] { for z in b[2]..=b[5] { for x in b[0]..=b[3] {
            let i = self.g.idx(x, y, z);
            let k = self.bad_kind(i);
            if k != 1 && k != 2 { continue; }
            if self.skipped[i] { self.skipped[i] = false; self.skipped_cells -= 1; }
            if !self.needs(i) { continue; }
            self.deferred.remove(i as i32);
            self.tries.remove(i as i32);
            self.blocked.remove(i as i32);
            n += 1;
        } } }
        for z in b[2]..=b[5] { for x in b[0]..=b[3] {
            let y = self.scan_column(x, b[1], z);
            if y != NONE && y <= b[4] && y < self.col_next[self.col(x, z)] { self.set_next(x, z, y); }
        } }
        n
    }

    fn in_build(&self, x: i32, y: i32, z: i32) -> bool {
        let g = &self.g;
        x >= g.ox && x < g.ox + g.sx && y >= g.oy && y < g.oy + g.sy && z >= g.oz && z < g.oz + g.sz
    }

    /// Whatever stands in the cell is from now on a block to take away (as the bot treats strays).
    fn mark_stray(&mut self, i: usize) -> bool {
        let tb = t();
        let w = self.g.cur[i] as usize;
        if w == 0 || self.scaffold.contains(i as i32) { return w != 0; }
        let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
        if y < self.g.oy || self.is_supply_chest(x, y, z) || tb.technical[w] || tb.secondary[w] { return false; }
        self.scaffold.add(i as i32);
        self.g.set_nav_flag(i, super::grid::N_SCAF, true);
        let c = self.col(x, z);
        self.scaffold_in_column[c] += 1;
        true
    }
    fn unmark_stray(&mut self, i: usize) {
        if self.scaffold.remove(i as i32) {
            self.g.set_nav_flag(i, super::grid::N_SCAF, false);
            let c = self.col(self.g.x_of(i), self.g.z_of(i));
            self.scaffold_in_column[c] -= 1;
        }
    }
    /// Breaks whatever stands in a cell; true when the cell is empty afterwards.
    fn take_out(&mut self, i: usize) -> bool {
        if self.g.cur[i] == 0 { return true; }
        let had = self.scaffold.contains(i as i32);
        if !self.mark_stray(i) { return false; }
        self.must_remove = true;
        let gone = self.remove_scaffold(i);
        self.must_remove = false;
        if self.g.cur[i] != 0 { if !had { self.unmark_stray(i); } return false; }
        gone
    }

    /// One more try at a cell with the bot's ordinary routine; true when it stands right afterwards.
    fn try_cell(&mut self, i: usize) -> bool {
        if self.g.tgt[i] == 0 { return false; }
        if self.skipped[i] { self.skipped[i] = false; self.skipped_cells -= 1; }
        if self.bad_kind(i) == 2 && !self.take_out(i) { self.defer_why = "the wrong block there could not be taken out"; return false; }
        if !self.needs(i) { return self.bad_kind(i) == 0; }
        let r = self.do_cell(i);
        self.reconcile();
        if r == PLACED && !self.needs(i) { self.deferred.remove(i as i32); return true; }
        if r != SKIP { self.deferred.add(i as i32); }
        false
    }

    /// Straight shafts to a cell from open space, fewest blocks first (up before sideways): the cells to break,
    /// outermost first. Open space is two free cells in a row beyond the last block, or the edge of the build.
    fn shafts_for(&self, i: usize, max_depth: usize) -> Vec<Vec<usize>> {
        let tb = t();
        let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
        const D: [(i32, i32, i32); 5] = [(0, 1, 0), (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)];
        let mut found: Vec<(usize, Vec<usize>)> = Vec::new();
        for (k, d) in D.iter().enumerate() {
            let mut solid: Vec<usize> = Vec::new();
            let (mut air, mut open) = (0, false);
            for step in 1..=(max_depth as i32 + 8) {
                let (nx, ny, nz) = (x + d.0 * step, y + d.1 * step, z + d.2 * step);
                if !self.g.inside(nx, ny, nz) { open = air >= 1 || solid.is_empty(); break; }
                let n = self.g.idx(nx, ny, nz);
                let w = self.g.cur[n] as usize;
                let a = self.g.tgt[n] as usize;
                // another hole on the way: nothing to break, but not open space either
                if w == 0 && self.in_build(nx, ny, nz) && a != 0 && !crate::build::decorative(a) && !crate::build::game_master(a) { continue; }
                if w == 0 || tb.walk_through[w] {
                    air += 1;
                    if air >= 2 || !self.in_build(nx, ny, nz) { open = true; break; }
                } else {
                    if self.is_supply_chest(nx, ny, nz) || ny < self.g.oy || tb.technical[w] || tb.secondary[w] { break; }
                    air = 0;
                    solid.push(n);
                    if solid.len() > max_depth { break; }
                }
            }
            if !open || solid.is_empty() || solid.len() > max_depth { continue; }
            solid.reverse();
            found.push((solid.len() * 2 + if k == 0 { 0 } else { 1 }, solid));
        }
        found.sort_by_key(|f| f.0);
        found.into_iter().map(|f| f.1).collect()
    }

    /// For bad cells shut in by blocks that stand around them: a short straight shaft from the nearest open side, the
    /// cell put right through it, and the shaft closed again from the inside out - what a player does to mend a hole
    /// inside a wall. Only where every block taken out can be put back from the stock (what is broken drops nothing).
    /// Returns the cells put right.
    fn excavate(&mut self, cells: &[usize]) -> i64 {
        let tb = t();
        self.undo_rope();
        self.unreachable.clear();
        self.bad_stand.clear();
        let mut done = vec![false; cells.len()];
        let mut fixed = 0;
        for _ in 0..cells.len().min(300) {
            if self.time_short() { break; }
            let (bx, by, bz) = (self.cx(), self.cy(), self.cz());
            let mut k = usize::MAX;
            let mut best = i64::MAX;
            for (j, &c) in cells.iter().enumerate() {
                if done[j] { continue; }
                let d = ((self.g.x_of(c) - bx).abs() + (self.g.z_of(c) - bz).abs() + 2 * (self.g.y_of(c) - by).abs()) as i64;
                if d < best { best = d; k = j; }
            }
            if k == usize::MAX { break; }
            done[k] = true;
            let i = cells[k];
            if self.bad_kind(i) == 0 { continue; }
            if self.try_cell(i) { fixed += 1; continue; }
            // a shaft only helps where the trouble is getting at the cell
            let why = self.defer_why;
            if !(why.starts_with("no face") || why.starts_with("no way") || why.starts_with("could not get") || why.starts_with("click failed") || why.starts_with("would shut") || why.starts_with("the wrong block")) { continue; }
            let mut ok = false;
            for shaft in self.shafts_for(i, 4) {
                // only what can be put back
                let mut need: Vec<(u16, i32)> = Vec::new();
                let mut add = |it: u16| { if let Some(e) = need.iter_mut().find(|e| e.0 == it) { e.1 += 1; } else { need.push((it, 1)); } };
                for &v in &shaft { if self.g.cur[v] != 0 && self.g.tgt[v] != 0 { add(tb.item[self.g.tgt[v] as usize]); } }
                if self.g.cur[i] == 0 { add(tb.item[self.g.tgt[i] as usize]); }
                if need.iter().any(|&(it, n)| self.count(it) + self.in_depot(it) < n + 1) { continue; }
                let mut opened = 0;
                for &v in &shaft {
                    if self.g.cur[v] == 0 { opened += 1; continue; }
                    let was_target = self.g.tgt[v] != 0 && self.matches(self.g.cur[v], self.g.tgt[v]);
                    if !self.take_out(v) { break; }
                    if was_target { self.broken_out.insert(v as i32); self.deferred.add(v as i32); }
                    opened += 1;
                }
                if opened == shaft.len() { self.unreachable.clear(); self.bad_stand.clear(); ok = self.try_cell(i); }
                // close the shaft, innermost first
                for q in (0..opened).rev() {
                    let v = shaft[q];
                    if self.g.tgt[v] == 0 || !self.in_build(self.g.x_of(v), self.g.y_of(v), self.g.z_of(v)) { continue; }
                    self.unreachable.clear(); self.bad_stand.clear();
                    self.try_cell(v);
                }
                self.undo_rope();
                if ok { break; }
            }
            if ok { fixed += 1; }
        }
        self.undo_rope();
        self.reconcile();
        fixed
    }

    pub fn repair(&mut self) {
        for round in 0..3 {
            if self.time_short() { break; }
            let (mut secs, before) = self.failing_sections();
            if secs.is_empty() { break; }
            self.undo_rope();
            self.unreachable.clear();
            self.bad_stand.clear();
            let whole = secs.len() > 6 && self.max_ticks - self.ticks > 150000;
            let mut queued = 0;
            if whole {
                let g = &self.g;
                let b = [g.ox, g.oy, g.oz, g.ox + g.sx - 1, g.oy + g.sy - 1, g.oz + g.sz - 1];
                queued = self.requeue(b);
            } else {
                secs.sort_by_key(|s| s.bad);
                for s in secs.iter().take(24) { if s.machine || s.missing_wrong == 0 { continue; } queued += self.requeue(s.b); }
            }
            if self.verbose { self.log(&format!("repair round {round}: {} sections fail, {before} bad cells in the build; {queued} cells taken up again{}", secs.len(), if whole { " (the whole build)" } else { "" })); }
            if queued > 0 {
                let (late_was, layer_was) = (self.late_too, self.layer_now);
                self.main_loop();
                self.late_too = true;
                self.layer_now = i32::MAX;
                self.deferred_pass(false);
                self.undo_rope();
                self.reconcile();
                // the other ways of placing: a helper block that may rest on a second one, then with traps accepted
                self.helper_chain_now = true;
                self.deferred_pass(false);
                self.helper_chain_now = false;
                self.undo_rope();
                self.deferred_pass(true);
                self.undo_rope();
                self.reconcile();
                // (revision "excavate") what is still bad in the sections taken up and shut in by standing blocks
                if crate::rev::rev().excavate && crate::ckpt::armed(self.ticks) {
                    let mut cells: Vec<usize> = Vec::new();
                    if whole { secs.sort_by_key(|s| s.bad); }
                    for s in secs.iter().take(24) {
                        if s.machine || s.missing_wrong == 0 { continue; }
                        let b = s.b;
                        for y in b[1]..=b[4] { for z in b[2]..=b[5] { for x in b[0]..=b[3] { let i = self.g.idx(x, y, z); let k = self.bad_kind(i); if k == 1 || k == 2 { cells.push(i); } } } }
                    }
                    let n = self.excavate(&cells);
                    if self.verbose { self.log(&format!("repair round {round}: {} cells still bad in those sections, {n} put right through a shaft", cells.len())); }
                }
                self.late_too = late_was; self.layer_now = layer_was;
            }
            // the clean-up again in full
            self.end_game();
            let (_, after) = self.failing_sections();
            if self.verbose { self.log(&format!("repair round {round}: bad cells {before} -> {after}")); }
            if after >= before { break; }
        }
    }
}
