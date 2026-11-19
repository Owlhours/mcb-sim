//! (diagnostic, MCSIM_HUNGDIAG=<file>; no bot decision reads anything here) What put up every temporary block, and
//! what the clean-up spends on getting at it. Every piece is tagged when it goes up: the phase (main / after / cleanup)
//! and the move (pillar, bridge or cap - the bot's own way, the "rope" -, helper, escape, tower, other), and for a
//! piece of the rope how it left the rope if it was not taken down with it: `far` (given up by "ropefar": 6+ blocks
//! behind the bot in a build behind its pace) or `fail` (the walk back or the break failed). Written at the end:
//!
//!   S <when> <tag> <hung|stands> | pieces standing    (when: main = at the end of the main pass, end = at the end)
//!   T <tag of the piece tried> | tries | taken | pieces the clean-up put up for those tries | of them still standing at the end
//!   G <tag> | pieces put up | given up from the rope
//!   X key=value   (counters)
use super::state::Bot;
use crate::fasthash::FastMap;
use crate::tables::t;
use std::cell::RefCell;
use std::collections::BTreeMap;

pub fn on() -> bool { static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new(); *V.get_or_init(|| std::env::var_os("MCSIM_HUNGDIAG").is_some()) }

#[derive(Clone, Copy)]
struct Tag { phase: u8, mv: &'static str, fate: u8, tower: bool }

impl Tag {
    fn label(&self) -> String {
        let ph = ["main", "after", "cleanup"][self.phase as usize];
        if self.tower { return format!("{ph} tower"); }
        let f = ["", "/far", "/fail"][self.fate as usize];
        format!("{ph} {}{f}", self.mv)
    }
}

#[derive(Default)]
struct Hd {
    mv: &'static str,
    standing: FastMap<i32, Tag>,
    snap: BTreeMap<String, i64>,
    tries: BTreeMap<String, [i64; 3]>,
    put: BTreeMap<String, [i64; 2]>,
    /// pieces put up by clean-up tries, by the tag of the piece tried (to count what of it still stands at the end)
    try_own: FastMap<i32, String>,
    x: BTreeMap<&'static str, i64>,
    /// what is breaking pieces now ("sweep", "target", "undo", "other"), and what took each piece that is gone
    ctx: &'static str,
    gone_by: FastMap<i32, &'static str>,
    /// (reach of a big try) pieces gone since the try began, and the try's pending row
    in_try: bool,
    try_gone: Vec<i32>,
    reach_row: Option<(String, Vec<i32>, Vec<i32>)>,
    rows: Vec<String>,
    /// per big try: pieces of its own still standing, pieces left in reach taken later while some of its own stood
    recs: Vec<[i64; 2]>,
    own_of: FastMap<i32, usize>,
    q_of: FastMap<i32, Vec<usize>>,
}

thread_local! { static H: RefCell<Hd> = RefCell::new(Hd { mv: "other", ..Default::default() }); }

pub fn set_move(m: &'static str) { if on() { H.with(|h| h.borrow_mut().mv = m); } }
pub fn set_ctx(c: &'static str) -> &'static str { if on() { H.with(|h| std::mem::replace(&mut h.borrow_mut().ctx, c)) } else { "" } }
pub fn gone_by(i: i32) -> &'static str { if on() { H.with(|h| h.borrow().gone_by.get(&i).copied().unwrap_or("never")) } else { "" } }
pub fn count(k: &'static str, n: i64) { if on() { H.with(|h| *h.borrow_mut().x.entry(k).or_insert(0) += n); } }

impl Bot {
    fn hd_phase(&self) -> u8 { if self.cleaning { 2 } else if self.main_done { 1 } else { 0 } }

    pub fn hd_put(&mut self, x: i32, y: i32, z: i32) {
        if !on() { return; }
        let i = self.g.idx(x, y, z) as i32;
        let tower = t().scaffolding[self.g.cur[i as usize] as usize];
        let phase = self.hd_phase();
        let on_tower = self.g.is_tower(x, y - 1, z);
        H.with(|h| {
            let mut h = h.borrow_mut();
            let mut mv = h.mv;
            if mv == "pillar" && on_tower { mv = "cap"; }
            let tag = Tag { phase, mv, fate: 0, tower };
            h.put.entry(tag.label()).or_insert([0; 2])[0] += 1;
            h.standing.insert(i, tag);
            h.mv = "other";
        });
    }

    pub fn hd_gone(&mut self, i: i32) {
        if !on() { return; }
        H.with(|h| { let mut h = h.borrow_mut(); h.standing.remove(&i); h.try_own.remove(&i); if h.in_try { h.try_gone.push(i); }
            if let Some(r) = h.own_of.remove(&i) { h.recs[r][0] -= 1; }
            if let Some(v) = h.q_of.remove(&i) { for r in v { if h.recs[r][0] > 0 { h.recs[r][1] += 1; } } }
            let c = if h.ctx.is_empty() { "other" } else { h.ctx }; h.gone_by.insert(i, c); });
    }

    /// The rope is given up: its pieces that still stand go with the clean-up (fate 1 "ropefar", 2 a failure).
    pub fn hd_rope_lost(&mut self, fate: u8) {
        if !on() { return; }
        let cells: Vec<i32> = self.rope.iter().map(|e| e[7]).filter(|&c| self.scaffold.contains(c)).collect();
        H.with(|h| {
            let mut h = h.borrow_mut();
            for c in cells {
                let Some(tg) = h.standing.get_mut(&c) else { continue };
                if tg.tower || tg.fate != 0 { continue; }
                tg.fate = fate;
                let l = tg.label();
                h.put.entry(l).or_insert([0; 2])[1] += 1;
            }
        });
    }

    /// Counts what stands now, by tag and by whether it hangs (nothing but scaffolding under it down to the air).
    pub fn hd_snap(&mut self, when: &str) {
        if !on() { return; }
        let tb = t();
        let mut rows: Vec<String> = Vec::new();
        H.with(|h| {
            let h = h.borrow();
            for (&i, tg) in h.standing.iter() {
                let iu = i as usize;
                let (x, mut y, z) = (self.g.x_of(iu), self.g.y_of(iu), self.g.z_of(iu));
                y -= 1;
                while self.g.inside(x, y, z) && self.scaffold.contains(self.g.idx(x, y, z) as i32) { y -= 1; }
                let hung = if y < self.g.oy { false } else { let w = self.g.get(x, y, z); w == 0 || tb.replaceable[w as usize] };
                rows.push(format!("{when} {} {}", tg.label(), if hung { "hung" } else { "stands" }));
            }
        });
        H.with(|h| { let mut h = h.borrow_mut(); for r in rows { *h.snap.entry(r).or_insert(0) += 1; } });
        count(if when == "main" { "behind_at_main" } else { "behind_at_end" }, self.behind as i64);
    }

    pub fn hd_key(&self, i: i32) -> String {
        if !on() { return String::new(); }
        H.with(|h| h.borrow().standing.get(&i).map(|t| t.label()).unwrap_or_else(|| "untagged".to_string()))
    }

    /// A clean-up try begins (for the reach rows).
    pub fn hd_try_begin(&mut self) { if !on() { return; } H.with(|h| { let mut h = h.borrow_mut(); h.in_try = true; h.try_gone.clear(); h.reach_row = None; }); }

    /// Before the rope of a try comes down: if the try put up 6+ pieces, the other pieces within reach (sweep box:
    /// 4 aside, 4 below, 5 above the feet) of every stand it reached (the top of every piece it put up, and where it
    /// stands now), and how many of those it took during the try.
    pub fn hd_try_reach(&mut self, best: i32, from: usize) {
        if !on() { return; }
        let own: Vec<i32> = self.scaf_log[from..].to_vec();
        if own.len() < 6 { H.with(|h| h.borrow_mut().in_try = false); return; }
        let ownset: crate::fasthash::FastSet<i32> = own.iter().copied().collect();
        let mut stands: Vec<(i32, i32, i32)> = vec![(self.cx(), self.cy(), self.cz())];
        for &c in &own { let cu = c as usize; let (x, y, z) = (self.g.x_of(cu), self.g.y_of(cu), self.g.z_of(cu));
            if self.scaffold.contains(c) && t().scaffolding[self.g.cur[cu] as usize] { stands.push((x, y, z)); }
            if !self.g.inside(x, y + 1, z) || !ownset.contains(&(self.g.idx(x, y + 1, z) as i32)) { stands.push((x, y + 1, z)); } }
        let towers = own.iter().filter(|&&c| self.scaffold.contains(c) && t().scaffolding[self.g.cur[c as usize] as usize]).count();
        let gone: crate::fasthash::FastSet<i32> = H.with(|h| h.borrow().try_gone.iter().copied().collect());
        let (mut seen, mut left, mut taken, mut left_now, mut taken_now) = (crate::fasthash::FastSet::default(), 0, 0, 0, 0);
        let mut lq: Vec<i32> = Vec::new();
        let (sx, sy, sz) = stands[0];
        for &(x, y, z) in &stands { for dy in -4..=5 { for dz in -4..=4 { for dx in -4..=4 {
            let (qx, qy, qz) = (x + dx, y + dy, z + dz);
            if !self.g.inside(qx, qy, qz) { continue; }
            let q = self.g.idx(qx, qy, qz) as i32;
            if q == best || ownset.contains(&q) || !seen.insert(q) { continue; }
            let near_now = (qx - sx).abs() <= 4 && (qz - sz).abs() <= 4 && qy - sy >= -4 && qy - sy <= 5;
            if self.scaffold.contains(q) { left += 1; lq.push(q); if near_now { left_now += 1; } } else if gone.contains(&q) { taken += 1; if near_now { taken_now += 1; } }
        } } } }
        let row = format!("t={} put={} towers={} stands={} reach={} taken={} reach_end={} taken_end={} force={} y={}", self.ticks, own.len(), towers, stands.len(), left + taken, taken, left_now + taken_now, taken_now, self.force_mine as u8, sy - self.g.oy);
        H.with(|h| { let mut h = h.borrow_mut(); h.reach_row = Some((row, own, lq)); h.in_try = false; });
    }

    /// A clean-up try for a piece tagged `key` is over; the pieces logged since `from` in `scaf_log` were put up for it.
    pub fn hd_try(&mut self, key: String, from: usize, taken: bool) {
        if !on() { return; }
        let own: Vec<i32> = self.scaf_log[from..].to_vec();
        let key = if self.own_down { format!("owndown {key}") } else { key };
        if self.verbose && !own.is_empty() { let towers = own.iter().filter(|&&c| t().scaffolding[self.g.cur[c as usize] as usize]).count(); self.log(&format!("HD try [{key}] put {} ({towers} towers) taken={taken} standing={}", own.len(), self.scaffold.len())); }
        H.with(|h| {
            let mut h = h.borrow_mut();
            let e = h.tries.entry(key.clone()).or_insert([0; 3]);
            e[0] += 1; if taken { e[1] += 1; } e[2] += own.len() as i64;
            for c in own { if h.standing.contains_key(&c) { h.try_own.insert(c, key.clone()); } }
            h.in_try = false;
        });
        let rr = H.with(|h| h.borrow_mut().reach_row.take());
        if let Some((row, own, lq)) = rr {
            let still: Vec<i32> = own.iter().copied().filter(|&c| self.scaffold.contains(c)).collect();
            let lq: Vec<i32> = lq.into_iter().filter(|&c| self.scaffold.contains(c)).collect();
            let od = self.own_down as u8;
            H.with(|h| { let mut h = h.borrow_mut();
                let r = h.recs.len();
                h.recs.push([still.len() as i64, 0]);
                for c in &still { h.own_of.insert(*c, r); }
                for c in &lq { h.q_of.entry(*c).or_default().push(r); }
                h.rows.push(format!("R {row} own_after={} left_after={} taken_try={taken} owndown={od}", still.len(), lq.len())); });
        }
    }

    pub fn hd_dump(&mut self) {
        if !on() { return; }
        self.hd_snap("end");
        let Some(path) = std::env::var_os("MCSIM_HUNGDIAG") else { return };
        H.with(|h| {
            let h = h.borrow();
            let mut left: BTreeMap<&String, i64> = BTreeMap::new();
            for k in h.try_own.values() { *left.entry(k).or_insert(0) += 1; }
            let mut s = String::new();
            for (k, v) in &h.snap { s += &format!("S {k} | {v}\n"); }
            for (k, v) in &h.tries { s += &format!("T {k} | {} | {} | {} | {}\n", v[0], v[1], v[2], left.get(k).copied().unwrap_or(0)); }
            for (k, v) in &h.put { s += &format!("G {k} | {} | {}\n", v[0], v[1]); }
            for (k, v) in &h.x { s += &format!("X {k}={v}\n"); }
            for (k, r) in h.rows.iter().enumerate() { s += &format!("{r} later={} own_end={}\n", h.recs[k][1], h.recs[k][0]); }
            s += &format!("X ticks={} main_ticks={} scaffold_placed={} rope_orphaned={} rope_undone={}\n", self.ticks, self.main_ticks_at, self.scaffold_placed, self.rope_orphaned, self.rope_undone);
            let _ = std::fs::write(&path, s);
        });
    }
}
