//! (diagnostic, MCSIM_SCAFDIAG=<file>; no bot decision reads anything here) What every piece of scaffolding was put up
//! FOR: the move that put it up (pillar / tower / bridge / cap / helper / escape), the kind of stand the bot was
//! heading for, the target cell being worked on and what that cell had around it, what the piece stands on, and how
//! many target cells were placed from a stand it carried before it came down. Summed by purpose at the end of the run:
//!
//!   P <phase> <move> <stand> <cell> <gap> <foot> | pieces | cells placed from them | pieces that served none | short-lived | standing at the end
//!   C <phase> <how the cell was placed> <cell> <gap> | cells
//!   E <phase> <stand> <cell> <gap> | cells that needed pieces | pieces
use super::state::Bot;
use crate::fasthash::FastMap;
use crate::tables::t;
use std::cell::RefCell;
use std::collections::BTreeMap;

pub fn on() -> bool { static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new(); *V.get_or_init(|| std::env::var_os("MCSIM_SCAFDIAG").is_some()) }

#[derive(Clone)]
struct Piece { key: String, tick: i64, served: u32 }

#[derive(Default)]
struct Diag {
    /// the move about to put a piece up
    mv: &'static str,
    /// the stand last planned ("self", "stand", "lean", "column-tower", "column-cap", "column-loose", "float", "break-...")
    stance: String,
    /// the target cell being worked on (-1: none), its class and the gap under it
    cell: i32, cls: String, gap: &'static str, name: String, up: i32,
    /// pieces put up since the cell was taken up
    ep_pieces: i64,
    standing: FastMap<i32, Piece>,
    pieces: BTreeMap<String, [i64; 5]>,
    cells: BTreeMap<String, i64>,
    episodes: BTreeMap<String, [i64; 2]>,
    comp_key: (i32, i64, usize), comp: Vec<i32>,
}

thread_local! { static D: RefCell<Diag> = RefCell::new(Diag { cell: -1, mv: "?", ..Default::default() }); }

pub fn set_move(m: &'static str) { super::hungdiag::set_move(m); if on() { D.with(|d| d.borrow_mut().mv = m); } }
pub fn set_stance(s: String) { if on() { D.with(|d| d.borrow_mut().stance = s); } }

impl Bot {
    fn sd_phase(&self) -> &'static str { if self.cleaning { "cleanup" } else if self.main_done { "after" } else { "main" } }
    fn sd_real(&self, x: i32, y: i32, z: i32) -> u16 {
        if !self.g.inside(x, y, z) { return 0; }
        let i = self.g.idx(x, y, z);
        if self.scaffold.contains(i as i32) { 0 } else { self.g.cur[i] }
    }

    /// A target cell is taken up.
    pub fn sd_cell_begin(&mut self, i: usize) {
        if !on() { return; }
        let tb = t();
        let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
        let under = self.sd_real(x, y - 1, z);
        let beside = (0..4).any(|d| self.sd_real(x + super::DXH[d], y, z + super::DZH[d]) != 0);
        let above = self.sd_real(x, y + 1, z) != 0;
        let thin = !tb.full_cube[self.g.tgt[i] as usize];
        let mut cls = String::new();
        cls.push_str(if under != 0 { if tb.full_cube[under as usize] { "under-full" } else { "under-thin" } } else if y == self.g.oy { "under-ground" } else if beside { "beside" } else if above { "above" } else { "nothing" });
        if under != 0 && beside { cls.push_str("+beside"); }
        cls.push_str(if thin { "/thin" } else { "/cube" });
        // the gap under the cell down to the next block of the build (or the ground)
        let mut g = 0;
        let mut yy = y - 1;
        while yy >= self.g.oy && self.sd_real(x, yy, z) == 0 { g += 1; yy -= 1; }
        let gap = if g == 0 { "gap0" } else if yy < self.g.oy { if g <= 3 { "ground1-3" } else { "ground4+" } } else if g <= 3 { "gap1-3" } else { "gap4+" };
        let name = tb.binfo(self.g.tgt[i]).name.to_string();
        let up = y - self.cy();
        D.with(|d| { let mut d = d.borrow_mut(); d.cell = i as i32; d.cls = cls; d.gap = gap; d.ep_pieces = 0; d.stance = "here".to_string(); d.name = name; d.up = up; });
    }

    /// The cell is done with: `placed` if it went in.
    pub fn sd_cell_end(&mut self, placed: bool) {
        if !on() { return; }
        let ph = self.sd_phase();
        if placed { self.sd_credit(); }
        D.with(|d| {
            let mut d = d.borrow_mut();
            if placed { let k = format!("{ph} {} {} {}", d.stance, d.cls, d.gap); *d.cells.entry(k).or_insert(0) += 1; }
            if d.ep_pieces > 0 { let k = format!("{ph} {} {} {} {}", if placed { "placed" } else { "failed" }, d.stance, d.cls, d.gap); let n = d.ep_pieces; let e = d.episodes.entry(k).or_insert([0; 2]); e[0] += 1; e[1] += n; }
            if d.ep_pieces > 0 && ph == "main" { let up = if d.up <= 1 { "up<=1" } else if d.up <= 4 { "up2-4" } else if d.up <= 8 { "up5-8" } else { "up9+" }; let k = format!("{} {} {} {}", d.stance, d.name, d.cls, up); let n = d.ep_pieces; let e = d.episodes.entry(format!("B {k}")).or_insert([0; 2]); e[0] += 1; e[1] += n; }
            d.cell = -1; d.ep_pieces = 0;
        });
    }

    /// A cell placed in passing, from where the bot stands (sweep_here).
    pub fn sd_swept(&mut self) {
        if !on() { return; }
        let ph = self.sd_phase();
        self.sd_credit();
        D.with(|d| *d.borrow_mut().cells.entry(format!("{ph} swept - -")).or_insert(0) += 1);
    }

    /// One target cell went in: every piece of the scaffolding the bot stands on (joined face to face) served it.
    fn sd_credit(&mut self) {
        let (y, by) = (self.c.y(), self.cy());
        let _ = y;
        let mut feet: Vec<i32> = Vec::new();
        for (ox, oz) in [(0.0, 0.0), (0.3, 0.3), (0.3, -0.3), (-0.3, 0.3), (-0.3, -0.3)] {
            let (fx, fz) = ((self.c.x() + ox).floor() as i32, (self.c.z() + oz).floor() as i32);
            for fy in [by - 1, by] {
                if !self.g.inside(fx, fy, fz) { continue; }
                let k = self.g.idx(fx, fy, fz) as i32;
                if self.scaffold.contains(k) && !feet.contains(&k) { feet.push(k); }
            }
        }
        if feet.is_empty() { return; }
        let key = (feet[0], self.scaffold_placed, self.scaffold.len());
        let layer = self.g.nx * self.g.nz;
        let steps = [1, -1, self.g.nx, -self.g.nx, layer, -layer];
        let total = (self.g.nx * self.g.ny * self.g.nz) as i32;
        D.with(|d| {
            let mut d = d.borrow_mut();
            if d.comp_key != key {
                let mut comp = feet.clone();
                let mut k = 0;
                while k < comp.len() && comp.len() < 400 {
                    let c = comp[k]; k += 1;
                    for s in steps { let n = c + s; if n >= 0 && n < total && self.scaffold.contains(n) && !comp.contains(&n) { comp.push(n); } }
                }
                d.comp = comp; d.comp_key = key;
            }
            let comp = std::mem::take(&mut d.comp);
            for c in &comp { if let Some(p) = d.standing.get_mut(c) { p.served += 1; } }
            d.comp = comp;
        });
    }

    /// A piece went up at (x,y,z).
    pub fn sd_put(&mut self, x: i32, y: i32, z: i32) {
        if !on() { return; }
        let tb = t();
        let ph = self.sd_phase();
        // what it stands on: more scaffolding down to the ground / down to the build, or nothing (hung beside something)
        let mut yy = y - 1;
        let mut h = 0;
        while self.g.inside(x, yy, z) && self.scaffold.contains(self.g.idx(x, yy, z) as i32) { yy -= 1; h += 1; }
        let w = self.g.get(x, yy, z);
        let foot = if w == 0 || tb.replaceable[w as usize] { "hung" } else if yy < self.g.oy { if h >= 3 { "ground-col3+" } else { "ground" } } else if h >= 3 { "build-col3+" } else { "build" };
        let i = self.g.idx(x, y, z) as i32;
        let ticks = self.ticks;
        D.with(|d| {
            let mut d = d.borrow_mut();
            let mv = if d.mv == "pillar" && self.g.is_tower(x, y - 1, z) { "cap" } else { d.mv };
            let key = if d.cell >= 0 { d.ep_pieces += 1; format!("{ph} {mv} {} {} {} {foot}", d.stance, d.cls, d.gap) } else { format!("{ph} {mv} no-cell - - {foot}") };
            d.standing.insert(i, Piece { key, tick: ticks, served: 0 });
            d.mv = "?";
        });
    }

    /// The piece at cell index `i` is gone.
    pub fn sd_gone(&mut self, i: i32) {
        if !on() { return; }
        let ticks = self.ticks;
        D.with(|d| {
            let mut d = d.borrow_mut();
            if let Some(p) = d.standing.remove(&i) {
                let e = d.pieces.entry(p.key).or_insert([0; 5]);
                e[0] += 1; e[1] += p.served as i64; if p.served == 0 { e[2] += 1; } if ticks - p.tick < 400 { e[3] += 1; }
            }
        });
    }

    pub fn sd_dump(&mut self) {
        if !on() { return; }
        let Some(path) = std::env::var_os("MCSIM_SCAFDIAG") else { return };
        D.with(|d| {
            let mut d = d.borrow_mut();
            let st: Vec<Piece> = d.standing.values().cloned().collect();
            for p in st { let e = d.pieces.entry(p.key).or_insert([0; 5]); e[0] += 1; e[1] += p.served as i64; if p.served == 0 { e[2] += 1; } e[4] += 1; }
            let mut s = String::new();
            for (k, v) in &d.pieces { s += &format!("P {k} | {} | {} | {} | {} | {}\n", v[0], v[1], v[2], v[3], v[4]); }
            for (k, v) in &d.cells { s += &format!("C {k} | {v}\n"); }
            for (k, v) in &d.episodes { if k.starts_with("B ") { s += &format!("{k} | {} | {}\n", v[0], v[1]); } else { s += &format!("E {k} | {} | {}\n", v[0], v[1]); } }
            let _ = std::fs::write(&path, s);
        });
    }
}
