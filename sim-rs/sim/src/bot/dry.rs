//! Dry pockets (MCSIM_DRYMEMO): a route search that runs dry has looked at every place the bot can get to under its
//! rules. While nothing it read has changed, a later question from a place in that set (same rules, goal and stop
//! cells outside it) has the same answer - "no, and every reachable place was looked at" - without searching.
//!
//! Only the plain rule set is remembered (no temporary blocks, no breaking, no new columns: `plain_way_out` and the
//! first question of `way_out`). There an expansion reads nothing but the picture (block states and the N_SCAF mark),
//! so a pocket depends on the cells its expansions read, not on the whole world.
//!
//! A pocket is a set of states - (cell, entered by walking or the like) and (cell, entered by digging or clearing
//! own blocks): the search expands the two differently - that is CLOSED: every move out of a state of the set leads
//! to a state of the set. A search that starts in a closed set can never leave it, so if neither the goal nor a stop
//! cell is in it the search ends without a way. The set is made closed, not assumed to be: the search notes in
//! `PCell::pad` how it tried to enter each cell and how it expanded it, and the states it tried but never expanded
//! (a cheaper way in was found first) are expanded afterwards by the search's own loop in `probe` mode.
//!  * `dep`: every cell an expansion of a state reads (the state's own column and the four beside it, from three
//!    above to two below, and on down through whatever the bot could fall or sink through, plus two);
//!  * every write to the picture (`Grid::put`, the N_SCAF mark) that lands in `dep` is noted with the value the cell
//!    had before; a write that puts that value back takes the note away again (a block removed in thought and put
//!    back). A pocket with notes does not answer;
//!  * but it can be mended: only the states in the five columns around a noted cell can have read it. They are
//!    expanded again in the picture as it is now (probe mode); where they lead to states that are not in the set,
//!    those are added and expanded in turn (up to a limit). If that ends, the set is closed in the present picture,
//!    the notes are dropped, and the pocket answers again. (It may now hold places that can no longer be reached:
//!    a closed set that is too large proves the same thing.)
//! What a pocket proves is "no way". Not known without searching: how many expansions the search would have made (it
//! depends on the start: the weighted heuristic re-expands cells), and with that whether it would have run dry or run
//! out of budget inside the set. So a pocket answers only for callers that read nothing but found / not found
//! (`nav.only_found`: `plain_way_out`, and the first question of `way_out`, whose `certain_no` comes from its second
//! search). `nav.expanded` gets the count of the search that made the pocket and `nav.exhausted` is set (neither is
//! read); `nav.total_expanded` (the summary line's expanded=, the checkpoint fingerprint) counts nothing for an answer
//! from a pocket, so it is smaller than without the switch.
//! Blocked edges: the set is closed as if none were blocked (the states a blocked move starts from are expanded again
//! in probe mode, which ignores them), so it holds whatever is blocked later.
use super::grid::*;
use super::nav;
use super::state::Bot;

/// On unless MCSIM_NO_DRYMEMO is set (from 2026-11-17; the rows are the same either way, only the computing differs).
pub fn on() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCSIM_NO_DRYMEMO").is_none() || check())
}
/// MCSIM_DRYCHECK: the real search runs as well; panic if it does not say what the pocket said.
pub fn check() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MCSIM_DRYCHECK").is_some())
}
fn envnum(k: &str, d: i64) -> i64 { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }
/// (measurement) MCSIM_DRYSLOTS pockets kept (16); MCSIM_DRYMEND=0: pockets with notes are not mended;
/// MCSIM_DRYSTAT: counts on stderr.
fn cfg() -> &'static (usize, bool, bool) {
    static V: std::sync::OnceLock<(usize, bool, bool)> = std::sync::OnceLock::new();
    V.get_or_init(|| (envnum("MCSIM_DRYSLOTS", 16).max(1) as usize, std::env::var_os("MCSIM_DRYSTAT").is_some(), envnum("MCSIM_DRYMEND", 1) != 0))
}
pub fn stat_on() -> bool { cfg().1 }

/// More notes than this and the pocket is not mended.
const MAX_NOTES: usize = 12;
/// More notes than this and the pocket is given up (MCSIM_DRYKEEP). Between the two it is kept but neither answers nor
/// is mended: pieces taken away in thought (a whole column, in the clean-up) are put back a moment later, and the
/// notes go with them.
fn max_mend() -> usize {
    static V: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *V.get_or_init(|| envnum("MCSIM_DRYMENDMAX", 32) as usize)
}
fn max_kept() -> usize {
    static V: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *V.get_or_init(|| envnum("MCSIM_DRYKEEP", 400).max(MAX_NOTES as i64) as usize)
}
const COLS: [(i32, i32); 5] = [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)];

#[derive(Default)]
pub struct Pocket {
    pub live: bool,
    x0: i32, y0: i32, z0: i32, wx: i32, wy: i32, wz: i32,
    /// per cell of the box: read by an expansion of a state of the set
    dep: Vec<u64>,
    /// per cell of the box: the state (cell, way in) is in the set; [0] walking or the like, [1] digging / clearing
    m: [Vec<u64>; 2],
    /// the search that made the pocket: start (grid index), what it took the start's height for (-2: the start stood
    /// like any other place), and how many expansions it made
    start: i32, start_h: i32, count: i32,
    /// states in the set
    size: i32,
    /// nothing has been mended: the pocket is as the search left it
    pristine: bool,
    /// the search that made it ran with blocked edges (the set is closed as if they were open)
    had_blocked: bool,
    /// cells of `dep` that differ from the picture the set is closed in: (grid index, value there)
    notes: Vec<(u32, u32)>,
    /// the notes with which mending failed last (not tried again while they are the same)
    bad: Vec<(u32, u32)>,
    /// `Dry::exit_gen` for which no remembered way-out place lies in the pocket
    exit_gen: u64,
    used: u64,
}

impl Pocket {
    #[inline]
    fn bit(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let (dx, dy, dz) = (x - self.x0, y - self.y0, z - self.z0);
        if dx < 0 || dy < 0 || dz < 0 || dx >= self.wx || dy >= self.wy || dz >= self.wz { return None; }
        Some(((dx * self.wz + dz) * self.wy + dy) as usize)
    }
    #[inline]
    fn has(v: &[u64], b: usize) -> bool { v[b >> 6] >> (b & 63) & 1 != 0 }
    #[inline]
    fn member(&self, b: usize) -> bool { (self.m[0][b >> 6] | self.m[1][b >> 6]) >> (b & 63) & 1 != 0 }
}

#[derive(Default)]
pub struct Dry {
    pub pockets: Vec<Pocket>,
    pub nlive: usize,
    /// the search in hand lists the cells it reaches (`Grid::pk` keys)
    pub rec: bool,
    pub touched: Vec<i32>,
    pub exit_gen: u64,
    clock: u64,
    /// probe mode: the pocket being closed, whether it failed, the states added (way in, bit), how many more may be added
    cur: usize, fail: bool, added: Vec<(u8, u32)>, room: i32,
    /// see `dump`
    pub st: [u64; 32],
}

impl Dry {
    /// (candidate `nostrand2`) The notes of every pocket and which pockets are live, kept around blocks taken away in
    /// thought and put back (a note can change places on the way, or a pocket be given up for too many).
    pub fn notes_keep(&self) -> Vec<(bool, Vec<(u32, u32)>)> {
        if self.nlive == 0 { return Vec::new(); }
        self.pockets.iter().map(|p| (p.live, p.notes.clone())).collect()
    }
    pub fn notes_put_back(&mut self, k: Vec<(bool, Vec<(u32, u32)>)>) {
        if k.is_empty() { return; }
        debug_assert!(k.len() == self.pockets.len());
        for (p, (live, notes)) in self.pockets.iter_mut().zip(k) { p.live = live; p.notes = notes; }
        self.nlive = self.pockets.iter().filter(|p| p.live).count();
    }
}

impl Grid {
    /// A cell of the picture changes (state, or the N_SCAF mark): `old` -> `new` as `state | scaf << 16`.
    #[inline(never)]
    pub fn dry_edit(&mut self, i: usize, old: u32, new: u32) {
        let (x, y, z) = (self.x_of(i), self.y_of(i), self.z_of(i));
        let d = &mut self.dry;
        for p in d.pockets.iter_mut() {
            if !p.live { continue; }
            match p.bit(x, y, z) { Some(b) if Pocket::has(&p.dep, b) => {}, _ => continue }
            if let Some(k) = p.notes.iter().position(|n| n.0 == i as u32) {
                if p.notes[k].1 == new { p.notes.swap_remove(k); }
            } else {
                p.notes.push((i as u32, old));
                d.st[15] += 1;
                if p.notes.len() > max_kept() { p.live = false; d.nlive -= 1; d.st[16] += 1; }
            }
        }
    }
}

impl Bot {
    /// Probe mode, a move into (x,y,z): the state must be in the pocket, or is added to it and expanded in its turn.
    #[inline(never)]
    pub(super) fn dry_probe(&mut self, x: i32, y: i32, z: i32, how: u8) {
        let k = self.g.pk(x, y, z);
        let d = &mut self.g.dry;
        if d.fail { return; }
        let p = &mut d.pockets[d.cur];
        let c = match how { 1 => 0usize, 2 => 1, _ => { d.fail = true; return; } };
        let Some(b) = p.bit(x, y, z) else { d.fail = true; return; };
        if Pocket::has(&p.m[c], b) { return; }
        d.room -= 1;
        if d.room < 0 { d.fail = true; return; }
        p.m[c][b >> 6] |= 1 << (b & 63);
        d.added.push((c as u8, b as u32));
        self.nav.push(if c == 0 { nav::WALK } else { nav::DIG } as i64, k);
    }

    /// Probe mode, a state is about to be expanded: the cells that reads go into `dep`. False: it does not fit (stop).
    #[inline(never)]
    pub(super) fn dry_probe_cell(&mut self, x: i32, y: i32, z: i32) -> bool {
        if self.g.dry.fail { return false; }
        self.g.dry.st[23] += 1;
        let k = self.g.dry.cur;
        let mut p = std::mem::take(&mut self.g.dry.pockets[k]);
        let ok = self.dry_dep(&mut p, x, y, z);
        self.g.dry.pockets[k] = p;
        if !ok { self.g.dry.fail = true; }
        ok
    }

    /// Marks in `dep` what an expansion at (x,y,z) reads. False: part of it lies outside the pocket's box.
    fn dry_dep(&self, p: &mut Pocket, x: i32, y: i32, z: i32) -> bool {
        let top = (y + 3).min(self.g.y0 + self.g.ny - 1);
        for (dx, dz) in COLS {
            let (cx, cz) = (x + dx, z + dz);
            if !self.g.inside(cx, y, cz) { continue; }
            let low = self.dry_low(cx, y, cz);
            let (Some(b0), Some(b1)) = (p.bit(cx, low, cz), p.bit(cx, top, cz)) else { return false };
            for b in b0..=b1 { p.dep[b >> 6] |= 1 << (b & 63); }
        }
        true
    }

    /// The lowest cell of column (x, z) that an expansion at height y (there or in the column beside it) reads: two
    /// below, then on down through everything a fall, a step down or a sinking could pass, and two below where it stops.
    #[inline]
    fn dry_low(&self, x: i32, y: i32, z: i32) -> i32 {
        let y0 = self.g.y0;
        let mut yy = y - 2;
        if yy <= y0 { return y0; }
        let mut pi = self.g.pidx(x, yy, z);
        loop {
            let c = self.g.nv(pi);
            if c & N_PASS == 0 && c & N_COLNZ != 0 { return (yy - 2).max(y0); }
            if yy <= y0 { return y0; }
            yy -= 1;
            pi -= self.g.pys;
        }
    }

    /// Runs the search's loop over the states in the heap (probe mode) for pocket k. True: the set is closed.
    fn dry_close(&mut self, k: usize, room: i32) -> bool {
        let d = &mut self.g.dry;
        d.cur = k; d.fail = false; d.room = room; d.added.clear();
        let rec = d.rec;
        d.rec = false;
        self.nav.probe = true;
        self.nav_search_g::<true>(0, 0, 0, 0, 0, 0, i32::MAX, false);
        self.nav.probe = false;
        self.nav.heap.clear();
        let d = &mut self.g.dry;
        d.rec = rec;
        let p = &mut d.pockets[k];
        if d.fail {
            for &(c, b) in d.added.iter() { p.m[c as usize][(b >> 6) as usize] &= !(1 << (b & 63)); }
            return false;
        }
        if !d.added.is_empty() { p.size += d.added.len() as i32; p.exit_gen = u64::MAX; }
        true
    }

    /// A pocket with notes: expand again every state that can have read a noted cell. True: closed again, notes dropped.
    fn dry_mend(&mut self, k: usize) -> bool {
        let p = std::mem::take(&mut self.g.dry.pockets[k]);
        self.nav.heap.clear();
        let mut n = 0;
        for &(i, _) in p.notes.iter() {
            let i = i as usize;
            let (kx, ky, kz) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
            for (dx, dz) in COLS {
                let (cx, cz) = (kx + dx, kz + dz);
                let Some(b0) = p.bit(cx, (ky - 3).max(p.y0), cz) else { continue };
                let y_lo = (ky - 3).max(p.y0);
                for dy in 0..(p.y0 + p.wy - y_lo) as usize {
                    let b = b0 + dy;
                    if Pocket::has(&p.m[0], b) { self.nav.push(nav::WALK as i64, self.g.pk(cx, y_lo + dy as i32, cz)); n += 1; }
                    if Pocket::has(&p.m[1], b) { self.nav.push(nav::DIG as i64, self.g.pk(cx, y_lo + dy as i32, cz)); n += 1; }
                }
            }
        }
        let room = 64 + p.size / 4;
        self.g.dry.pockets[k] = p;
        self.g.dry.st[24] += 1; self.g.dry.st[25] += n;
        let ok = self.dry_close(k, room);
        let d = &mut self.g.dry;
        let p = &mut d.pockets[k];
        if ok { p.notes.clear(); p.bad.clear(); p.pristine = false; d.st[26] += 1; d.st[27] += d.added.len() as u64; }
        else { p.bad.clear(); p.bad.extend_from_slice(&p.notes); p.bad.sort_unstable(); }
        ok
    }

    /// Does a pocket settle the question? Returns the number of expansions to report.
    pub fn dry_lookup(&mut self, sx: i32, sy: i32, sz: i32, gx: i32, gy: i32, gz: i32) -> Option<i32> {
        if self.g.dry.nlive == 0 { return None; }
        let s = self.g.idx(sx, sy, sz) as i32;
        let mend = cfg().2;
        let mut why = 10usize;
        for k in 0..self.g.dry.pockets.len() {
            let p = &self.g.dry.pockets[k];
            if !p.live { continue; }
            let Some(b) = p.bit(sx, sy, sz) else { continue };
            // the question the pocket was made by, asked again in the same picture
            let own = p.start == s && p.pristine && !p.had_blocked && (p.start_h == -2 || p.start_h == self.nav.start_h);
            // or a question from a place that is in the set as a standing place (and still is one)
            let inside = Pocket::has(&p.m[0], b);
            if !(own || inside) { continue; }
            if !p.notes.is_empty() {
                let try_it = mend && inside && p.notes.len() <= max_mend() && { let mut v = p.notes.clone(); v.sort_unstable(); v != p.bad };
                if !try_it || !self.dry_mend(k) { why = why.min(7); continue; }
            }
            let p = &self.g.dry.pockets[k];
            let own = own && p.pristine;
            if !own && self.g.stand16(sx, sy, sz) < 0 { continue; }
            if let Some(gb) = p.bit(gx, gy, gz) { if p.member(gb) { continue; } }
            if self.nav.stop_at && p.exit_gen != self.g.dry.exit_gen {
                let mut clear = true;
                for &e in self.exit_nodes.iter() {
                    let e = e as usize;
                    if let Some(eb) = p.bit(self.g.x_of(e), self.g.y_of(e), self.g.z_of(e)) { if p.member(eb) { clear = false; break; } }
                }
                if !clear { why = why.min(8); continue; }
                let gen = self.g.dry.exit_gen;
                self.g.dry.pockets[k].exit_gen = gen;
            }
            let d = &mut self.g.dry;
            d.clock += 1;
            let p = &mut d.pockets[k];
            p.used = d.clock;
            let n = p.count;
            d.st[1] += 1; d.st[2] += own as u64; d.st[3] += n as u64;
            return Some(n);
        }
        self.g.dry.st[why] += 1;
        None
    }

    /// After a search that ran dry (and listed its cells): keep what it reached as a pocket.
    pub fn dry_store(&mut self, sx: i32, sy: i32, sz: i32) {
        let start = self.g.pk(sx, sy, sz);
        self.g.dry.touched.push(start);
        let cells = std::mem::take(&mut self.g.dry.touched);
        // (a start in the air that the search took as standing: that expansion is no state of the set)
        let special = self.g.stand16(sx, sy, sz) < 0 && self.nav.start_h >= 0;
        let (mut lx, mut ly, mut lz, mut hx, mut hy, mut hz) = (i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN);
        for &k in cells.iter() {
            let (x, y, z) = self.g.unpk(k);
            lx = lx.min(x); hx = hx.max(x); lz = lz.min(z); hz = hz.max(z); hy = hy.max(y);
            for (dx, dz) in COLS { if self.g.inside(x + dx, y, z + dz) { ly = ly.min(self.dry_low(x + dx, y, z + dz)); } }
        }
        // (room around it for the states that closing and mending add)
        let g = &self.g;
        let (x0, z0, y0) = ((lx - 3).max(g.x0), (lz - 3).max(g.z0), (ly - 3).max(g.y0));
        let (x1, z1, y1) = ((hx + 3).min(g.x0 + g.nx - 1), (hz + 3).min(g.z0 + g.nz - 1), (hy + 6).min(g.y0 + g.ny - 1));
        let (wx, wy, wz) = (x1 - x0 + 1, y1 - y0 + 1, z1 - z0 + 1);
        let vol = wx as i64 * wy as i64 * wz as i64;
        if vol > (1 << 20).max(512 * cells.len() as i64) { self.g.dry.st[6] += 1; self.g.dry.touched = cells; return; }
        // which slot: a free one, else the one used longest ago
        let slots = cfg().0;
        let d = &mut self.g.dry;
        let mut slot = d.pockets.iter().position(|p| !p.live);
        if slot.is_none() && d.pockets.len() < slots { d.pockets.push(Pocket::default()); slot = Some(d.pockets.len() - 1); }
        let slot = slot.unwrap_or_else(|| { let mut best = 0; for k in 1..d.pockets.len() { if d.pockets[k].used < d.pockets[best].used { best = k; } } best });
        if !d.pockets[slot].live { d.nlive += 1; }
        d.clock += 1;
        let mut p = std::mem::take(&mut d.pockets[slot]);
        let words = (vol as usize + 63) / 64;
        p.dep.clear(); p.dep.resize(words, 0);
        for v in p.m.iter_mut() { v.clear(); v.resize(words, 0); }
        p.live = true; p.x0 = x0; p.y0 = y0; p.z0 = z0; p.wx = wx; p.wy = wy; p.wz = wz;
        p.start = self.g.idx(sx, sy, sz) as i32; p.start_h = if special { self.nav.start_h } else { -2 };
        p.count = self.nav.expanded; p.pristine = true; p.had_blocked = !self.nav.blocked_edges.is_empty();
        p.notes.clear(); p.bad.clear();
        p.exit_gen = if self.nav.stop_at { self.g.dry.exit_gen } else { u64::MAX };
        p.used = self.g.dry.clock;
        // the states: every way the search tried to enter a cell; those it did not expand that way are expanded now
        self.nav.heap.clear();
        let (mut size, mut open, mut fits) = (0, 0u64, true);
        for &k in cells.iter() {
            let (x, y, z) = self.g.unpk(k);
            let pad = self.g.pc[self.g.pidx(x, y, z)].pad;
            let (mut tried, mut done) = (pad & 15, pad >> 4);
            if k == start { if special { done &= !1; } else { tried |= 1; } }
            if tried & !3 != 0 { fits = false; break; }
            let b = p.bit(x, y, z).unwrap();
            for c in 0..2usize {
                if tried >> c & 1 == 0 { continue; }
                p.m[c][b >> 6] |= 1 << (b & 63);
                size += 1;
                if done >> c & 1 == 0 { self.nav.push(if c == 0 { nav::WALK } else { nav::DIG } as i64, k); open += 1; }
            }
            if done != 0 || k == start { fits &= self.dry_dep(&mut p, x, y, z); }
        }
        // (moves that were blocked for this search are followed too: the pocket then holds whatever edges are blocked)
        let mut from: Vec<usize> = self.nav.blocked_edges.iter().map(|&e| (e >> 32) as usize).collect();
        from.sort_unstable();
        from.dedup();
        for i in from {
            if i >= self.g.cur.len() { continue; }
            let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
            let Some(b) = p.bit(x, y, z) else { continue };
            for c in 0..2usize { if Pocket::has(&p.m[c], b) { self.nav.push(if c == 0 { nav::WALK } else { nav::DIG } as i64, self.g.pk(x, y, z)); open += 1; } }
        }
        p.size = size;
        let ncells = cells.len() as u64;
        let d = &mut self.g.dry;
        d.touched = cells;
        d.pockets[slot] = p;
        let closed = fits && (open == 0 || self.dry_close(slot, 64 + size / 4));
        let d = &mut self.g.dry;
        if !closed { d.pockets[slot].live = false; d.nlive -= 1; d.st[5] += 1; self.nav.heap.clear(); return; }
        d.st[4] += 1; d.st[13] += ncells; d.st[14] += d.pockets[slot].dep.iter().map(|w| w.count_ones() as u64).sum::<u64>();
        if open > 0 { d.st[28] += 1; d.st[29] += open; d.st[30] += d.added.len() as u64; }
    }
}

pub fn dump(b: &Bot) {
    let s = &b.g.dry.st;
    eprintln!("drystat eligible={} dry={} hits={} own_start={} cells_saved={} made={} not_closed={} too_wide={} miss_notes={} miss_exit={} miss_none={} count_differs={} real_not_exhausted={} max_real_per_cell_pct={} pocket_cells={} dep_cells={} notes={} given_up={} found_n={}/{} probes={} mends={} mend_states={} mended={} mend_added={} closings={} closing_open={} closing_added={}",
        s[11], s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], s[8], s[10], s[12], s[17], s[18], s[13], s[14], s[15], s[16], s[21], s[22], s[23], s[24], s[25], s[26], s[27], s[28], s[29], s[30]);
}
