//! (diagnostic, MCSIM_STANDLOG=<file>; no bot decision reads anything here) The stands of the main pass, one line each
//! way: every trip `go_to` makes to a stand (when it left, when it got there, from where, to where, the route's moves
//! by kind and its lowest and highest feet), and every target cell placed (where the bot stood, and how: `d` a cell
//! the loop took up, `s` one placed in passing by `sweep_here`). `scripts/standlog.py` groups them by stand.
//!
//!   G <t0> <t1> <ok> <from x y z> <at x y z> <stand x y z> <cell x y z> <len> <W J D P B dig clear mine climb descend tower gate ladder> <miny> <maxy> <column> <cap> <trips>
//!     <under: what the feet rest on at arrival - 0 climbing column, 1 another piece of the bot's own, 2 a finished cell
//!     of the build, 3 the ground, 4 other> (`scripts/standunder.py`)
//!   C <t> ... (with MCSIM_COVERLOG: the best spot nearby by cells it would serve, see `sl_cover`; `scripts/standcover.py`)
//!   P <t> <d|s> <cell x y z> <bot x y z>
//!   R <t> <bot x y z> <placeable now, by why the sweep left it: total notcarried deferred late above below layer eligible> <needed in reach, no face>
//!     <layer-kept, by how much lower the lowest neighbour column is: 1 2 3+> <and what keeps that column's next cell: blocked notcarried out-of-reach noface-here placeable-here>
//!     (at a departure, after `sweep_here`: the target cells a click from this eye would place now - in reach, a face
//!     to click, held up if they fall - and which rule kept the bot from them)
use super::state::Bot;
use std::cell::RefCell;
use std::io::Write;

pub fn on() -> bool { static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new(); *V.get_or_init(|| std::env::var_os("MCSIM_STANDLOG").is_some()) }

#[derive(Default)]
struct Diag { w: Option<std::io::BufWriter<std::fs::File>>, cell: (i32, i32, i32), dep: (i64, i32, i32, i32, i64) }

thread_local! { static D: RefCell<Diag> = RefCell::new(Diag::default()); }

fn line(s: String) {
    D.with(|d| {
        let mut d = d.borrow_mut();
        if d.w.is_none() { if let Some(p) = std::env::var_os("MCSIM_STANDLOG") { d.w = std::fs::File::create(p).ok().map(std::io::BufWriter::new); } }
        if let Some(w) = d.w.as_mut() { let _ = writeln!(w, "{s}"); }
    });
}

impl Bot {
    fn sl_main(&self) -> bool { on() && !self.main_done && !self.cleaning }
    pub fn sl_cell(&mut self, i: usize) {
        if !self.sl_main() { return; }
        let c = (self.g.x_of(i) - self.g.ox, self.g.y_of(i) - self.g.oy, self.g.z_of(i) - self.g.oz);
        D.with(|d| d.borrow_mut().cell = c);
    }
    pub fn sl_placed(&mut self, i: usize, how: char) {
        if !self.sl_main() { return; }
        let (x, y, z) = (self.g.x_of(i) - self.g.ox, self.g.y_of(i) - self.g.oy, self.g.z_of(i) - self.g.oz);
        line(format!("P {} {how} {x} {y} {z} {} {} {}", self.ticks, self.cx() - self.g.ox, self.cy() - self.g.oy, self.cz() - self.g.oz));
    }
    pub fn sl_depart(&mut self) {
        if !self.sl_main() { return; }
        let d = (self.ticks, self.cx() - self.g.ox, self.cy() - self.g.oy, self.cz() - self.g.oz, self.trips);
        D.with(|s| s.borrow_mut().dep = d);
    }
    pub fn sl_arrive(&mut self, st: &super::state::Stance, ok: bool) {
        if !self.sl_main() { return; }
        let (dep, cell) = D.with(|s| { let s = s.borrow(); (s.dep, s.cell) });
        let mut k = [0i64; 13];
        let n = self.t_move.len();
        let (mut miny, mut maxy) = (i32::MAX, i32::MIN);
        for j in 0..n {
            if j > 0 { let m = self.t_move[j] as usize; if m < 13 { k[m] += 1; } }
            let y = self.t_path[j * 3 + 1] - self.g.oy;
            miny = miny.min(y); maxy = maxy.max(y);
        }
        if n == 0 { miny = dep.2; maxy = dep.2; }
        let ks: Vec<String> = k.iter().map(|v| v.to_string()).collect();
        // what the feet rest on at arrival: 0 the bot's climbing column, 1 another piece of its own (loose block, bridge,
        // pillar), 2 a finished cell of the build, 3 the ground (below the build's box or natural), 4 anything else
        let (fx, fy, fz) = (self.cx(), self.cy() - 1, self.cz());
        let under = if !self.g.inside(fx, fy, fz) { 3 } else {
            let i = self.g.idx(fx, fy, fz);
            if self.scaffold.contains(i as i32) { if self.g.is_tower(fx, fy, fz) { 0 } else { 1 } }
            else if self.g.tgt[i] != 0 && self.g.cur[i] != 0 { 2 }
            else if fy < self.g.oy { 3 } else { 4 }
        };
        line(format!("G {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}", dep.0, self.ticks, ok as i32, dep.1, dep.2, dep.3,
            self.cx() - self.g.ox, self.cy() - self.g.oy, self.cz() - self.g.oz, st.x - self.g.ox, st.y - self.g.oy, st.z - self.g.oz,
            cell.0, cell.1, cell.2, n, ks.join(" "), miny, maxy, st.column, st.cap as i32, self.trips - dep.4, under));
    }
    pub fn sl_pick(&mut self, i: i32, cost: f64, any: f64) {
        if !self.sl_main() || i < 0 { return; }
        let i = i as usize;
        line(format!("K {} {} {} {} {:.1} {:.1} {} {} {}", self.ticks, self.g.x_of(i) - self.g.ox, self.g.y_of(i) - self.g.oy, self.g.z_of(i) - self.g.oz, cost, any, self.cx() - self.g.ox, self.cy() - self.g.oy, self.cz() - self.g.oz));
    }
    pub fn sl_census(&mut self) {
        if !self.sl_main() { return; }
        let tb = crate::tables::t();
        let saved = self.click.clone();
        let e = self.c.eye();
        let sneak = self.c.sneaking();
        let (bx, by, bz) = (self.cx(), self.cy(), self.cz());
        let mut k = [0i64; 8];
        let mut noface = 0i64;
        let mut b = [0i64; 8];
        for y in by - 4..=by + 6 { for z in bz - 5..=bz + 5 { for x in bx - 5..=bx + 5 {
            if !self.g.inside(x, y, z) { continue; }
            let i = self.g.idx(x, y, z);
            if !self.needs(i) || self.g.cur[i] != 0 { continue; }
            let (cx, cy, cz) = (x as f64 + 0.5 - e[0], y as f64 + 0.5 - e[1], z as f64 + 0.5 - e[2]);
            if (cx * cx + cy * cy + cz * cz).sqrt() > self.reach + 0.9 { continue; }
            let tg = self.g.tgt[i];
            let sp = self.spec(tg);
            if !sp.placeable || self.body_in(x, y, z) { continue; }
            if tb.gravity[tg as usize] { let b = self.g.get(x, y - 1, z) as usize; if b == 0 || tb.walk_through[b] || tb.replaceable[b] { continue; } }
            if !self.find_click(x, y, z, sp.first, sp.table.as_ref().map(|v| v.as_slice()), e[0], e[1], e[2], sneak, 63) { noface += 1; continue; }
            let cn = self.col_next[self.col(x, z)];
            let why = if self.count(sp.item) == 0 { 1 }
                else if self.deferred.contains(i as i32) { 2 }
                else if tb.late[tg as usize] || self.pushed_into.contains(&(i as i32)) { 3 }
                else if cn != super::NONE && y > cn { 4 }
                else if cn == super::NONE || y < cn { 5 }
                else if !self.eligible(x, z) { 6 }
                else { 7 };
            k[0] += 1; k[why] += 1;
            if why == 6 {
                // the lowest neighbour column that holds it back: how much lower, and what keeps its next cell
                let mut low = (super::NONE, 0, 0);
                for dz in -1..=1 { for dx in -1..=1 {
                    if dx == 0 && dz == 0 { continue; }
                    let (nx, nz) = (x + dx, z + dz);
                    if !self.g.in_box(nx, self.g.oy, nz) { continue; }
                    let v = self.col_next[self.col(nx, nz)];
                    if v < low.0 { low = (v, nx, nz); }
                } }
                let (ly, lx, lz) = low;
                let dy = (y - ly).min(3) as usize;
                b[dy - 1] += 1;
                let j = self.g.idx(lx, ly, lz);
                let spj = self.spec(self.g.tgt[j]);
                let c = if self.blocked.contains(j as i32) { 0 }
                    else if self.count(spj.item) == 0 { 1 }
                    else {
                        let (qx, qy, qz) = (lx as f64 + 0.5 - e[0], ly as f64 + 0.5 - e[1], lz as f64 + 0.5 - e[2]);
                        if (qx * qx + qy * qy + qz * qz).sqrt() > self.reach + 0.9 { 2 }
                        else if self.g.cur[j] == 0 && !self.body_in(lx, ly, lz) && self.find_click(lx, ly, lz, spj.first, spj.table.as_ref().map(|v| v.as_slice()), e[0], e[1], e[2], sneak, 63) { 4 }
                        else { 3 }
                    };
                b[3 + c] += 1;
            }
        } } }
        self.click = saved;
        line(format!("R {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}", self.ticks, bx - self.g.ox, by - self.g.oy, bz - self.g.oz, k[0], k[1], k[2], k[3], k[4], k[5], k[6], k[7], noface,
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]));
    }
    /// (MCSIM_STANDLOG + MCSIM_COVERLOG) At a departure: over the spots one could stand on nearby (Chebyshev 4, -3..+3
    /// in height), the cells a click from each would place under the bot's rules (eligible, carried, not put off), and
    /// the same with the kind rule or the layer rule left out. Line:
    ///   C <t> <spots> <best rules> <best no-carry> <best no-layer> <mean rules> <dist of best>
    pub fn sl_cover(&mut self) {
        if !self.sl_main() || std::env::var_os("MCSIM_COVERLOG").is_none() { return; }
        let saved = self.click.clone();
        let (bx, by, bz) = (self.cx(), self.cy(), self.cz());
        let (mut spots, mut b0, mut b1, mut b2, mut sum, mut bd) = (0i64, 0i64, 0i64, 0i64, 0i64, 0i32);
        for sy in by - 3..=by + 3 { for sz in bz - 4..=bz + 4 { for sx in bx - 4..=bx + 4 {
            if !self.g.inside(sx, sy, sz) { continue; }
            let h = self.g.stand16(sx, sy, sz);
            if h < 0 { continue; }
            spots += 1;
            let (ex, ey, ez) = (sx as f64 + 0.5, sy as f64 + h as f64 / 16.0 + 1.62, sz as f64 + 0.5);
            let (mut n0, mut n1, mut n2) = (0i64, 0i64, 0i64);
            for z in sz - 4..=sz + 4 { for x in sx - 4..=sx + 4 {
                if !self.g.in_box(x, self.g.oy, z) { continue; }
                let y = self.col_next[self.col(x, z)];
                if y == super::NONE || y > sy + 5 || y < sy - 5 { continue; }
                if x == sx && z == sz && (y == sy || y == sy + 1) { continue; }
                let i = self.g.idx(x, y, z);
                if self.g.cur[i] != 0 || self.deferred.contains(i as i32) { continue; }
                let (cx, cy, cz) = (x as f64 + 0.5 - ex, y as f64 + 0.5 - ey, z as f64 + 0.5 - ez);
                if (cx * cx + cy * cy + cz * cz).sqrt() > self.reach + 0.9 { continue; }
                let tg = self.g.tgt[i];
                let sp = self.spec(tg);
                if !sp.placeable { continue; }
                let el = self.eligible(x, z);
                let car = self.count(sp.item) > 0;
                if !el && !car { continue; }
                if !self.find_click(x, y, z, sp.first, sp.table.as_ref().map(|v| v.as_slice()), ex, ey, ez, false, 63) { continue; }
                if el && car { n0 += 1; }
                if el { n1 += 1; }
                if car { n2 += 1; }
            } }
            sum += n0;
            if n0 > b0 { b0 = n0; bd = (sx - bx).abs().max((sz - bz).abs()) + (sy - by).abs(); }
            b1 = b1.max(n1); b2 = b2.max(n2);
        } } }
        self.click = saved;
        line(format!("C {} {spots} {b0} {b1} {b2} {:.2} {bd}", self.ticks, sum as f64 / spots.max(1) as f64));
    }
    pub fn sl_flush(&mut self) { D.with(|d| { if let Some(w) = d.borrow_mut().w.as_mut() { let _ = w.flush(); } }); }
}
