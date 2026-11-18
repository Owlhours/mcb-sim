//! The repair pass (revision "repair"; from the owner's monitor, `FROM-MONITOR-repair.md`): when the bot's
//! programme is over and sections still fail by its own picture, it takes them up again with its own routines.
//! Up to three rounds: if more than six sections fail and more than 150,000 ticks are left the whole build goes back
//! into the main programme (the bot had stopped early); otherwise the failing sections with the fewest bad cells, up
//! to 24, sections with machine parts left alone. Their bad cells are forgotten as "given up" and built again by the
//! main loop, then tried with the helper rules and with the trap rules; then the whole end game runs again. A round
//! that does not lower the bad cells is the last.
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
