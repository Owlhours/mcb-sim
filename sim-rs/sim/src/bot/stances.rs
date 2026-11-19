//! Choosing where to stand for a cell (the Java version's `Stances`).
use super::nav;
use super::state::{ring, Bot, Spec, Stance};
use super::*;
use crate::tables::*;

const DYS: [i32; 7] = [0, 1, -1, -2, 2, -3, -4];
const PLAN_REACH: f64 = 4.15;

/// (candidate `hang`) Is the whole top of the block at full height - whatever boxes it is made of (an upside-down
/// stair is two)? Every one of 4 x 4 points of the top lies in a box that reaches the top.
fn whole_top(state: u16) -> bool {
    let tb = t();
    let s = state as usize;
    if tb.full_cube[s] { return true; }
    if tb.interactive[s] { return false; }
    let b = &tb.out_boxes_f32[s];
    for i in 0..4 { for j in 0..4 {
        let (px, pz) = (0.125 + 0.25 * i as f32, 0.125 + 0.25 * j as f32);
        let mut k = 0;
        let mut hit = false;
        while k < b.len() && !hit { hit = b[k + 4] == 1.0 && b[k] <= px && px <= b[k + 3] && b[k + 2] <= pz && pz <= b[k + 5]; k += 6; }
        if !hit { return false; }
    } }
    true
}

impl Bot {
    pub fn plan_stance(&mut self, x: i32, y: i32, z: i32, sp: Option<&Spec>, breaking: bool) -> Option<Stance> { let _tg = super::nav::tag("plan_stance");
        self.plans += 1;
        self.n_floating = 0;
        self.dbg_stand = 0; self.dbg_click = 0; self.dbg_nav = 0; self.dbg_pillar = 0; self.dbg_pillar_click = 0; self.dbg_unsafe = 0;
        self.dbg_cut = 0; self.dbg_dry = 0;
        self.thin_floor.clear();
        self.reach = PLAN_REACH;
        let (n0, n1, n2) = (self.n_self, self.n_natural, self.n_lean);
        let r = self.plan_stance_impl(x, y, z, sp, breaking);
        if super::scafdiag::on() { if let Some(st) = r.as_ref() {
            let k = if st.column > 0 { if st.cap { "column-cap" } else if st.moss { "column-loose" } else { "column-tower" } } else if self.n_self > n0 { "self" } else if self.n_natural > n1 { "stand" } else if self.n_lean > n2 { "lean" } else { "float" };
            super::scafdiag::set_stance(format!("{}{k}", if breaking { "break-" } else { "" }));
        } }
        self.reach = 4.5;
        r
    }

    /// (candidate `hdfar`) A column for the plan's cell could not stand at (x, z) because the floor there is not flat.
    /// If what lies there is thin (a carpet, a pressure plate: at most two sixteenths high), is the build's own, stands
    /// on a flat top, and one of its kind is in reserve to put it back, it is noted: `attempt_stances` may
    /// take it out. Only in the repair pass, only for placing.
    fn note_thin_floor(&mut self, x: i32, base: i32, z: i32, breaking: bool) {
        if breaking || !self.in_repair || self.mend_only || !(crate::cand::on("hdfar") && crate::ckpt::armed(self.ticks)) || self.thin_floor.len() >= 8 { return; }
        if !self.g.in_box(x, base, z) || !self.g.inside(x, base - 1, z) { return; }
        let tb = t();
        let i = self.g.idx(x, base, z);
        let w = self.g.cur[i];
        if w == 0 || self.g.tgt[i] != w || self.scaffold.contains(i as i32) || tb.any_top[w as usize] > 2 || tb.interactive[w as usize] { return; }
        let bt = self.break_ticks(w);
        if bt < 0 || bt > 200 || !nav::flat_top(self.g.get(x, base - 1, z)) { return; }
        let it = tb.item[w as usize];
        if self.spare(it) <= 0 || self.thin_floor.contains(&(i as i32)) { return; }
        self.thin_floor.push(i as i32);
    }

    /// (diagnostics) a route of this plan failed: cut short at its budget, or every reachable place was looked at
    fn note_route_fail(&mut self) { if self.route_dry { self.dbg_dry += 1; } else { self.dbg_cut += 1; } }

    /// The visibility test of a candidate eye position: a click that places (or tops up), or a point to break at.
    fn sees(&mut self, x: i32, y: i32, z: i32, sp: Option<&Spec>, breaking: bool, ex: f64, ey: f64, ez: f64) -> bool {
        if breaking {
            if self.plan_top_up != 0 { let pt = self.plan_top_up; self.top_up_visible(x, y, z, pt, sp.unwrap(), ex, ey, ez) } else { self.find_break(x, y, z, ex, ey, ez) }
        } else {
            let s = sp.unwrap();
            self.find_click(x, y, z, s.first, s.table.as_ref().map(|v| v.as_slice()), ex, ey, ez, false, 0x3f)
        }
    }
    fn sees_sneaking(&mut self, x: i32, y: i32, z: i32, sp: &Spec, ex: f64, ey: f64, ez: f64, mask: i32) -> bool {
        self.find_click(x, y, z, sp.first, sp.table.as_ref().map(|v| v.as_slice()), ex, ey, ez, true, mask)
    }

    fn plan_stance_impl(&mut self, x: i32, y: i32, z: i32, sp: Option<&Spec>, breaking: bool) -> Option<Stance> {
        let tb = t();
        // (candidate `hang`: does the cell hang free - no cell of the build on any of its six sides, so that no
        // neighbour will ever lend it a face - or was it put off before for want of a face, and everything else is done? Only then the two column rules below.
        // All parts of the candidate but this first case act only on a cell that was put off once already for want of
        // a face (hang_retry): a build without such cells keeps its course.)
        self.hang_free = (self.main_done && self.hang_retry) || (0..6).all(|d| { let (nx, ny, nz) = (x + DX[d], y + DY[d], z + DZ[d]); !self.g.inside(nx, ny, nz) || self.g.tgt[self.g.idx(nx, ny, nz)] == 0 });
        // 0. on top of the block below the cell itself, placing underfoot with a jump
        if !breaking && self.self_placeable(x, y, z, sp.unwrap()) && !self.bad_stand.contains(&self.g.idx_raw(x, y, z)) && (self.cx() - x).abs() + (self.cz() - z).abs() <= 3 && self.cy() >= y - 1
            && { let _tg = super::nav::tag("st0_self"); self.route(x, y, z, 600) } {
            self.n_self += 1;
            return Some(Stance { x, y, z, lean: -1, cost: self.nav.goal_cost, ..Default::default() });
        }
        // 1. somewhere one can already stand
        let mut cand: Vec<[i32; 4]> = Vec::new();
        let (bx, by, bz) = (self.cx(), self.cy(), self.cz());
        let mut touches_interactive = false;
        for d in 0..6 { if touches_interactive { break; } touches_interactive = tb.interactive[self.g.get(x + DX[d], y + DY[d], z + DZ[d]) as usize]; }
        for o in ring().iter() {
            let (nx, nz) = (x + o[0], z + o[1]);
            for dy in DYS {
                let ny = y + dy;
                if nx == x && nz == z && (ny == y || ny == y - 1) { continue; }
                let h = self.g.stand16(nx, ny, nz);
                if h < 0 || self.bad_stand.contains(&self.g.idx_raw(nx, ny, nz)) { continue; }
                self.dbg_stand += 1;
                let (ex, mut ey, ez) = (nx as f64 + 0.5, ny as f64 + h as f64 / 16.0 + 1.62, nz as f64 + 0.5);
                // (candidate `nostand2`) in the middle of a hopper the bot stands in the bowl: the eye is that much lower
                if super::grid::bowl_floor() && crate::ckpt::armed(self.ticks) && tb.kind(self.g.get(nx, ny - 1, nz)) == crate::kinds::Kind::HopperBlock { ey -= super::grid::BOWL; }
                let mut ok = self.sees(x, y, z, sp, breaking, ex, ey, ez);
                if !ok && !breaking && touches_interactive && !self.g.is_tower(nx, ny - 1, nz) { ok = self.sees_sneaking(x, y, z, sp.unwrap(), ex, ey - 0.35, ez, 0x3f); }
                if !ok { continue; }
                self.dbg_click += 1;
                if cand.len() < 64 { cand.push([nx, ny, nz, (nx - bx).abs() + (nz - bz).abs() + 3 * 0.max(ny - by)]); }
            }
        }
        let nc = cand.len() as i32;
        let mut best: Option<Stance> = None;
        let mut tries = 0i32;
        while tries < 4 && tries < nc && best.is_none() {
            let mut k: i32 = -1;
            for j in 0..cand.len() { if cand[j][3] >= 0 && (k < 0 || cand[j][3] < cand[k as usize][3]) { k = j as i32; } }
            if k < 0 { break; }
            let ku = k as usize;
            cand[ku][3] = -1;
            let (kx, ky, kz) = (cand[ku][0], cand[ku][1], cand[ku][2]);
            if self.avoid_seal != 0 && !breaking && !self.body_in(x, y, z) {
                let ti = self.g.idx(x, y, z);
                let was = self.g.cur[ti];
                self.g.put(ti, self.avoid_seal);
                let usable_was = self.witness_usable;
                self.witness_usable = self.exit_valid && !self.exit_cells.contains(&(ti as i32));
                let open = self.way_out(kx, ky, kz, false);
                self.witness_usable = usable_was;
                self.g.put(ti, was);
                if !open { self.dbg_nav += 1; continue; }   // (tries-- then tries++ in the original)
            }
            // (clean-up: not from a spot that what is being taken away is the only way out of)
            if breaking && self.going_away.is_some() && !self.safe_after_removal(kx, ky, kz) {
                let n = self.dbg_unsafe;
                self.dbg_unsafe += 1;
                if n >= 8 { tries += 1; }   // (`if (dbgUnsafe++ < 8) tries--;` before the loop's tries++)
                continue;
            }
            if !{ let _tg = super::nav::tag("st1_stand"); self.route(kx, ky, kz, 6000) } { self.note_route_fail(); tries += 1; continue; }
            self.dbg_nav += 1;
            best = Some(Stance { x: kx, y: ky, z: kz, lean: -1, cost: self.nav.goal_cost, ..Default::default() });
            tries += 1;
        }
        if best.is_some() { self.n_natural += 1; return best; }
        // 2. on top of a block beside the cell, leaning out over it
        if !breaking {
            let s = sp.unwrap();
            for d in 0..4usize {
                let (nx, nz) = (x + DXH[d], z + DZH[d]);
                // (candidate `hang`: not only on a full block - on anything with a whole flat top at full height: an
                // upside-down stair, a top slab. The edge of a hanging floor is made of those, and the next cell
                // out is clicked onto their side from above, leaning out, as on a full block.)
                let top_ok = self.g.solid_cube(nx, y, nz) || (self.hang(4) && self.hang_retry && { let w = self.g.get(nx, y, nz); w != 0 && whole_top(w) && !self.scaffold.contains(self.g.idx(nx, y, nz) as i32) });
                if !top_ok || self.g.stand16(nx, y + 1, nz) != 0 { if self.watch.is_some() && self.watching(x, y, z) && self.g.get(nx, y, nz) != 0 { self.wlog(&format!("lean from {}: not a spot (on {}, flat top {}, stand {})", self.rel(nx, y + 1, nz), tb.name(self.g.get(nx, y, nz)), top_ok, self.g.stand16(nx, y + 1, nz))); } continue; }
                if self.bad_stand.contains(&(self.g.idx_raw(nx, y + 1, nz) ^ (((d as i32 ^ 1) + 1) << 28))) { continue; }
                let (ex, ez, ey) = (nx as f64 + 0.5 - DXH[d] as f64 * 0.75, nz as f64 + 0.5 - DZH[d] as f64 * 0.75, y as f64 + 1.0 + 1.27);
                if !self.sees_sneaking(x, y, z, s, ex, ey, ez, 0x3c) { if self.watch.is_some() && self.watching(x, y, z) { self.wlog(&format!("lean from {}: no click", self.rel(nx, y + 1, nz))); } continue; }
                if !{ let _tg = super::nav::tag("st2_lean"); self.route(nx, y + 1, nz, 6000) } { if self.watch.is_some() && self.watching(x, y, z) { self.wlog(&format!("lean from {}: no way there", self.rel(nx, y + 1, nz))); } continue; }
                if best.is_none() || self.nav.goal_cost < best.as_ref().unwrap().cost {
                    best = Some(Stance { x: nx, y: y + 1, z: nz, lean: (d ^ 1) as i32, cost: self.nav.goal_cost, ..Default::default() });
                }
            }
            if best.is_some() { self.n_lean += 1; return best; }
        }
        // 3. a temporary column near the cell: scaffolding to climb if possible, else a pillar of loose blocks
        for o in ring().iter() {
            if o[0] == 0 && o[1] == 0 { continue; }
            if o[0] * o[0] + o[1] * o[1] > 8 { break; }
            let (qx, qz) = (x + o[0], z + o[1]);
            if !self.g.inside(qx, y, qz) { continue; }
            let adjacent = o[0].abs() + o[1].abs() == 1;
            let mut top = if adjacent { y } else { y - 1 };
            while top >= y - 2 {
                let tp = top;
                top -= 1;
                if !self.g.passable(qx, tp + 1, qz) || !self.g.passable(qx, tp + 2, qz) { continue; }
                if self.bad_stand.contains(&self.g.idx_raw(qx, tp + 1, qz)) { continue; }
                let mut found: Option<Stance> = None;
                for variant in (if self.have_tower { 0 } else { 1 })..2 {
                    let tw = variant == 0;
                    // (candidate `hang`, below: a climbing column that stands in the column already - what is left of
                    // the one a neighbouring cell was placed from - is a floor for the capped column too: climb on
                    // up it, put a loose block on top, lean out from that. Its top is not a "flat top", and without
                    // this only the bare climbing column was looked at, which nothing can be leant out from.)
                    // find the base of the column
                    let mut base = tp;
                    while base >= self.g.y0 && self.g.nv(self.g.pidx(qx, base, qz)) & super::grid::N_AIR != 0 && (if tw { self.tower_ok(qx, base, qz) } else { self.scaffold_ok(qx, base, qz) }) { base -= 1; }
                    if base == tp || base < self.g.y0 { if self.watch.is_some() && self.watching(x, y, z) { self.wlog(&format!("column at {} top {}: no room (base {}) tower={tw}", self.rel(qx, tp, qz), tp - self.g.oy, base - self.g.oy)); } continue; }
                    let floor = self.g.get(qx, base, qz) as usize;
                    let floor_scaf = self.scaffold.contains(self.g.idx(qx, base, qz) as i32);
                    if tw {
                        if !tb.scaffolding[floor] && (!tb.sturdy_top[floor] || floor_scaf) { continue; }
                    } else if !nav::pillar_floor(floor as u16) && !(tb.scaffolding[floor] && self.have_tower && self.hang(2) && self.hang_free) { self.note_thin_floor(qx, base, qz, breaking); if self.watch.is_some() && self.watching(x, y, z) { self.wlog(&format!("column at {} top {}: floor {} not flat", self.rel(qx, tp, qz), tp - self.g.oy, tb.name(floor as u16))); } continue; }
                    let mut cap = !tw && self.have_tower && self.scaffold_ok(qx, tp, qz) && (tb.scaffolding[floor] || (tp - base >= 2 && tb.sturdy_top[floor] && !floor_scaf));
                    if cap { let mut yy = base + 1; while yy < tp && cap { cap = self.tower_ok(qx, yy, qz); yy += 1; } }
                    if !tw && !cap && tb.scaffolding[floor] { continue; }
                    self.dbg_pillar += 1;
                    if self.watch.is_some() && self.watching(x, y, z) { self.wlog(&format!("column at {} top {} base {} floor {} tower={tw} cap={cap}", self.rel(qx, tp, qz), tp - self.g.oy, base - self.g.oy, tb.name(floor as u16))); }
                    // pretend the column is there
                    for yy in base + 1..=tp { self.g.put_at(qx, yy, qz, if tw || (cap && yy < tp) { self.tower_state } else { self.scaffold_state }); }
                    let mut lean = -1;
                    let (mut ex, mut ey, mut ez) = (qx as f64 + 0.5, tp as f64 + 1.0 + 1.62, qz as f64 + 0.5);
                    let mut ok = self.sees(x, y, z, sp, breaking, ex, ey, ez);
                    if !ok && !tw && !breaking && touches_interactive { ok = self.sees_sneaking(x, y, z, sp.unwrap(), ex, ey - 0.35, ez, 0x3f); }
                    if !ok && !tw && !breaking && adjacent && tp == y {
                        let d: usize = if o[0] == 1 { 0 } else if o[0] == -1 { 1 } else if o[1] == 1 { 2 } else { 3 };
                        ex = qx as f64 + 0.5 - DXH[d] as f64 * 0.75; ez = qz as f64 + 0.5 - DZH[d] as f64 * 0.75; ey = tp as f64 + 1.0 + 1.27;
                        ok = self.sees_sneaking(x, y, z, sp.unwrap(), ex, ey, ez, 0x3c);
                        lean = (d ^ 1) as i32;
                    }
                    for yy in base + 1..=tp { self.g.put_at(qx, yy, qz, 0); }
                    if !ok { continue; }
                    self.dbg_pillar_click += 1;
                    // (clean-up: a new column only where its foot can be walked away from afterwards)
                    if breaking && self.going_away.is_some() && !self.safe_after_removal(qx, base + 1, qz) { self.dbg_unsafe += 1; continue; }
                    self.towers_off = !tw && !cap;
                    self.nav.extra_effort = 400 * (tp - base);
                    let reach2 = { let _tg = super::nav::tag("st3_column"); if cap { self.route_plan(qx, tp, qz, 20000) } else { self.route_plan(qx, tp + 1, qz, 20000) } };
                    self.towers_off = false;
                    self.nav.extra_effort = 0;
                    if !reach2 { self.note_route_fail(); continue; }
                    self.n_pillar_stance += 1;
                    found = Some(Stance { x: qx, y: tp + 1, z: qz, lean, cost: self.nav.goal_cost, moss: !tw && !cap, cap, column: tp - base });
                    break;
                }
                if found.is_some() { return found; }
                let k = self.n_floating % self.floating.len();
                self.floating[k] = [qx, tp, qz];
                self.n_floating += 1;
            }
        }
        // 4. a loose block hung in the air beside the cell, reached by bridging out from wherever a column can stand
        // From `fewfloat` on, in the careful clean-up: at most two of them, each with a search of 6,000 cells and no
        // more. The careful rule turns down most columns, each of them is then offered here as a block hung in the
        // air, and the routes to those - up to twelve a plan, up to 40,000 cells each, nearly all in vain - were
        // nine tenths of the clean-up's computing in one build.
        let few = breaking && self.going_away.is_some() && crate::rev::rev().few_float;
        let mut k = 0;
        while k < self.n_floating.min(self.floating.len()) && k < (if few { 2 } else { 12 }) {
            let [qx, top, qz] = self.floating[k];
            k += 1;
            if !self.g.inside(qx, top, qz) || !self.g.is_air(qx, top, qz) || !self.scaffold_ok(qx, top, qz) || self.g.stand16(qx, top + 1, qz) >= 0 { continue; }
            if !self.g.passable(qx, top + 1, qz) || !self.g.passable(qx, top + 2, qz) || self.bad_stand.contains(&self.g.idx_raw(qx, top + 1, qz)) { continue; }
            let gi = self.g.idx(qx, top, qz);
            self.g.put(gi, self.scaffold_state);
            let (ex, ey, ez) = (qx as f64 + 0.5, top as f64 + 1.0 + 1.62, qz as f64 + 0.5);
            let ok = self.sees(x, y, z, sp, breaking, ex, ey, ez);
            self.g.put(gi, 0);
            if !ok { continue; }
            self.nav.extra_effort = if few { 0 } else { 6000 };
            let reached = { let _tg = super::nav::tag("st4_float"); self.route_plan(qx, top + 1, qz, if few { 6000 } else { 40000 }) };
            self.nav.extra_effort = 0;
            if !reached { self.note_route_fail(); continue; }
            self.n_pillar_stance += 1;
            return Some(Stance { x: qx, y: top + 1, z: qz, lean: -1, cost: self.nav.goal_cost, ..Default::default() });
        }
        None
    }
}
