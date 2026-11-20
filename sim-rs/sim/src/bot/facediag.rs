//! (diagnostic, MCSIM_FACELOG=<file>; no bot decision reads anything here) Why the cells of the main pass needed
//! scaffolding and stand trips. Every stance plan of the main pass is classed by what the cell had to be clicked on:
//!
//!   W   a stand that exists already, within reach, sees a face (stages 0-2 of `plan_stance` found it)
//!   2b  such a stand sees a face, but no route to any of the ones tried (the plan went on to a column)
//!   1   no neighbour of the cell is solid in the world yet: nothing to click on
//!   2   a neighbour face, faced by a standable spot within reach, but blocks hide it from every one of them
//!   3   a face is seen from a standable spot nearby (Chebyshev 8, -16..+6 high) only beyond reach
//!   23  the face is faced only from spots beyond reach, and hidden from those too
//!   1f  a neighbour face, but no standable spot nearby faces it at all (or none gives the wanted state)
//!   bad a stand within reach sees a face, but every such stand was marked bad before
//!   K   a plan for breaking (a piece in the way, a block to set right)
//!
//! Lines (coordinates relative to the build's origin; ph: m main pass, a after it, c clean-up):
//!   A <t> <x y z>                     a target cell taken up (do_cell), main pass
//!   E <t> <placed 0/1>                that cell done with
//!   Q <t> <x y z> <stage> <class> <stage-1 stands that see it> [<solid neighbours: build or ground, own pieces>
//!       <block> [<dx,dy,dz,front,eligible,putoff> ...]]
//!       (the stance plan; past W and K the solid neighbours and the target block; for class 1 the neighbours that
//!       are target cells still to place: on the column's front now, its column eligible now, put off)
//!   G <t0> <t1> <ok> <from x y z> <at x y z>   a trip of go_to to a stand, main pass
//!   R <t0> <t1>                       a restock trip, main pass
//!   S <t> <ph> <x y z>                a piece of scaffolding went up (any phase)
//!   P <t> <ph> <x y z> <bot x y z>    a block was placed into a cell (any phase; target cells and others)
use super::state::{ring, Bot, Spec, Stance};
use super::*;
use std::cell::RefCell;
use std::io::Write;

pub fn on() -> bool { static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new(); *V.get_or_init(|| std::env::var_os("MCSIM_FACELOG").is_some()) }

#[derive(Default)]
struct Diag { w: Option<std::io::BufWriter<std::fs::File>>, dep: i64, rs: i64, from: String }

thread_local! { static D: RefCell<Diag> = RefCell::new(Diag::default()); }

fn line(s: String) {
    D.with(|d| {
        let mut d = d.borrow_mut();
        if d.w.is_none() { if let Some(p) = std::env::var_os("MCSIM_FACELOG") { d.w = std::fs::File::create(p).ok().map(std::io::BufWriter::new); } }
        if let Some(w) = d.w.as_mut() { let _ = writeln!(w, "{s}"); }
    });
}

const DYS: [i32; 7] = [0, 1, -1, -2, 2, -3, -4];

impl Bot {
    fn fd_main(&self) -> bool { on() && !self.main_done && !self.cleaning }
    fn fd_ph(&self) -> char { if self.cleaning { 'c' } else if self.main_done { 'a' } else { 'm' } }
    fn fd_rel(&self, x: i32, y: i32, z: i32) -> String { format!("{} {} {}", x - self.g.ox, y - self.g.oy, z - self.g.oz) }

    pub fn fd_cell(&mut self, i: usize, begin: bool, placed: bool) {
        if !self.fd_main() { return; }
        if begin { line(format!("A {} {}", self.ticks, self.fd_rel(self.g.x_of(i), self.g.y_of(i), self.g.z_of(i)))); }
        else { line(format!("E {} {}", self.ticks, placed as i32)); }
    }
    pub fn fd_depart(&mut self) { if self.fd_main() { let (t, p) = (self.ticks, self.fd_rel(self.cx(), self.cy(), self.cz())); D.with(|d| { let mut d = d.borrow_mut(); d.dep = t; d.from = p; }); } }
    pub fn fd_arrive(&mut self, ok: bool) {
        if !self.fd_main() { return; }
        let (t0, from) = D.with(|d| { let d = d.borrow(); (d.dep, d.from.clone()) });
        line(format!("G {t0} {} {} {from} {}", self.ticks, ok as i32, self.fd_rel(self.cx(), self.cy(), self.cz())));
    }
    pub fn fd_restock(&mut self, begin: bool) {
        if !self.fd_main() { return; }
        let t = self.ticks;
        if begin { D.with(|d| d.borrow_mut().rs = t); } else { let t0 = D.with(|d| d.borrow().rs); line(format!("R {t0} {t}")); }
    }
    pub fn fd_put(&mut self, x: i32, y: i32, z: i32) {
        if !on() { return; }
        line(format!("S {} {} {}", self.ticks, self.fd_ph(), self.fd_rel(x, y, z)));
    }
    pub fn fd_placed(&mut self, x: i32, y: i32, z: i32) {
        if !on() { return; }
        line(format!("P {} {} {} {}", self.ticks, self.fd_ph(), self.fd_rel(x, y, z), self.fd_rel(self.cx(), self.cy(), self.cz())));
    }
    pub fn fd_flush(&mut self) { D.with(|d| { if let Some(w) = d.borrow_mut().w.as_mut() { let _ = w.flush(); } }); }

    /// A face of the cell seen from this eye: `find_click` with the given reach, and (`occl` false) through blocks.
    fn fd_sees(&mut self, x: i32, y: i32, z: i32, sp: &Spec, ex: f64, ey: f64, ez: f64, reach: f64, occl: bool) -> bool {
        let (r0, o0) = (self.reach, self.fd_no_occl);
        self.reach = reach; self.fd_no_occl = !occl;
        let ok = self.find_click(x, y, z, sp.first, sp.table.as_ref().map(|v| v.as_slice()), ex, ey, ez, false, 0x3f);
        self.reach = r0; self.fd_no_occl = o0;
        ok
    }

    /// (from `plan_stance`) One stance plan made: classed as in the header.
    pub fn fd_plan(&mut self, x: i32, y: i32, z: i32, sp: Option<&Spec>, breaking: bool, r: &Option<Stance>, n0: i64, n1: i64, n2: i64) {
        if !self.fd_main() { return; }
        let stage = match r.as_ref() {
            Some(st) if st.column > 0 => if st.cap { "cap" } else if st.moss { "pillar" } else { "tower" },
            Some(_) if self.n_self > n0 => "self",
            Some(_) if self.n_natural > n1 => "stand",
            Some(_) if self.n_lean > n2 => "lean",
            Some(_) => "float",
            None => "none",
        };
        let pos = self.fd_rel(x, y, z);
        let (t0, dbgc) = (self.ticks, self.dbg_click);
        let (Some(sp), false) = (sp, breaking) else { line(format!("Q {t0} {pos} {stage} K {dbgc}")); return; };
        if matches!(stage, "self" | "stand" | "lean") { line(format!("Q {t0} {pos} {stage} W {dbgc}")); return; }
        let tb = crate::tables::t();
        // solid neighbours: of the build or the ground, and the bot's own pieces
        let (mut nsolid, mut nscaf) = (0, 0);
        for d in 0..6 {
            let (nx, ny, nz) = (x + DX[d], y + DY[d], z + DZ[d]);
            let s = self.g.get(nx, ny, nz) as usize;
            if s == 0 || tb.replaceable[s] || tb.out_boxes_f32[s].is_empty() { continue; }
            if self.g.inside(nx, ny, nz) && self.scaffold.contains(self.g.idx(nx, ny, nz) as i32) { nscaf += 1; } else { nsolid += 1; }
        }
        let info = format!("{dbgc} {nsolid} {nscaf} {}", tb.binfo(self.g.tgt[self.g.idx(x, y, z)]).name);
        if dbgc > 0 { line(format!("Q {t0} {pos} {stage} 2b {info}")); return; }
        if nsolid + nscaf == 0 {
            let mut nb = String::new();
            for d in 0..6 {
                let (nx, ny, nz) = (x + DX[d], y + DY[d], z + DZ[d]);
                if !self.g.in_box(nx, ny, nz) { continue; }
                let j = self.g.idx(nx, ny, nz);
                if self.g.tgt[j] == 0 || self.matches(self.g.cur[j], self.g.tgt[j]) { continue; }
                let front = self.col_next[self.col(nx, nz)] == ny;
                nb += &format!(" {},{},{},{},{},{}", DX[d], DY[d], DZ[d], front as i32, self.eligible(nx, nz) as i32, self.deferred.contains(j as i32) as i32);
            }
            line(format!("Q {t0} {pos} {stage} 1 {info}{nb}"));
            return;
        }
        let saved = self.click.clone();
        let eye = |g: &super::grid::Grid, nx: i32, ny: i32, nz: i32| -> Option<(f64, f64, f64)> {
            let h = g.stand16(nx, ny, nz);
            if h < 0 { None } else { Some((nx as f64 + 0.5, ny as f64 + h as f64 / 16.0 + 1.62, nz as f64 + 0.5)) }
        };
        // the stands of stage 1 (within reach): seen through blocks? seen from one marked bad?
        let (mut through, mut bad) = (false, false);
        'a: for o in ring().iter() {
            for dy in DYS {
                let (nx, ny, nz) = (x + o[0], y + dy, z + o[1]);
                if nx == x && nz == z && (ny == y || ny == y - 1) { continue; }
                let Some((ex, ey, ez)) = eye(&self.g, nx, ny, nz) else { continue };
                if !bad && self.bad_stand.contains(&self.g.idx_raw(nx, ny, nz)) && self.fd_sees(x, y, z, sp, ex, ey, ez, 4.15, true) { bad = true; break 'a; }
                if !through && self.fd_sees(x, y, z, sp, ex, ey, ez, 4.15, false) { through = true; }
            }
        }
        let class = if bad { "bad" } else if through { "2" } else {
            // farther: Chebyshev 8 across, 16 down to 6 up, reach 24
            let (mut far, mut far_through) = (false, false);
            'b: for dy in -16..=6 { for dz in -8..=8 { for dx in -8..=8 {
                let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                if nx == x && nz == z && (ny == y || ny == y - 1) { continue; }
                let Some((ex, ey, ez)) = eye(&self.g, nx, ny, nz) else { continue };
                if self.fd_sees(x, y, z, sp, ex, ey, ez, 24.0, true) { far = true; break 'b; }
                if !far_through && self.fd_sees(x, y, z, sp, ex, ey, ez, 24.0, false) { far_through = true; }
            } } }
            if far { "3" } else if far_through { "23" } else { "1f" }
        };
        self.click = saved;
        line(format!("Q {t0} {pos} {stage} {class} {info}"));
    }
}
