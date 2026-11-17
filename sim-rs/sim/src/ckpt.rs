//! Tooling for experiments on failing builds (no bot decision reads anything here):
//! * a timeline of health signals sampled every `EVERY` ticks (`--timeline FILE`),
//! * a derailment detector over it: explicit rules, the first one to fire gives the tick T where the build starts
//!   to go badly wrong (`--derail`),
//! * checkpoints: `mcsim run <hash> --checkpoint-at derail|<tick> --save <file>` and `mcsim resume <file>`.
//!
//! A checkpoint is a deterministic-replay checkpoint, not a memory image. The bot is a straight port of the Java
//! bot's imperative code: at any tick the "where am I in the plan" state is the Rust call stack (Builder -> pass ->
//! do_cell -> route walk -> tick), which cannot be written to a file without rewriting the bot as a resumable state
//! machine (and that rewrite would be a decision risk). The simulator is deterministic (fixed-seed level random,
//! no clocks in decisions), so the file holds what is needed to reach tick T again exactly: hash, revision, tick
//! limit, stop tick, seed, the tick T, and a fingerprint of the state at T (bot counters, body, inventory, the
//! bot's picture of every target cell, level random draws). `resume` replays to T, checks the fingerprint, and
//! from T on `armed()` is true: experimental switches gated on `crate::ckpt::armed(ticks)` act only from T, so the
//! run before T is the base run. MCSIM_FROM_TICK=T does the same without a file.
use crate::bot::state::Bot;
use crate::tables::t;
use std::cell::RefCell;

pub const EVERY: i64 = 200;

/// The tick from which experimental switches are armed (MCSIM_FROM_TICK, or set by `resume`); 0 = always armed.
pub fn from_tick() -> i64 {
    FROM.with(|f| {
        let mut f = f.borrow_mut();
        if f.is_none() { *f = Some(std::env::var("MCSIM_FROM_TICK").ok().and_then(|v| v.parse().ok()).unwrap_or(0)); }
        f.unwrap()
    })
}
pub fn set_from_tick(t: i64) { FROM.with(|f| *f.borrow_mut() = Some(t)); }
/// For experimental switches: true from the checkpoint tick on (always true when no checkpoint is in use).
#[inline]
pub fn armed(ticks: i64) -> bool { ticks >= from_tick() }
/// A candidate's switch for local tests from a checkpoint: on when the variable is set AND the checkpoint tick is
/// reached (`mcsim resume F` with the variable set: the run up to the checkpoint is the base run, checked by its
/// fingerprint). Without a checkpoint it is simply "the variable is set". The value is read each time (it is not
/// hot: use it where the candidate decides, not per cell of a search).
pub fn cand(name: &str, ticks: i64) -> bool { armed(ticks) && std::env::var_os(name).is_some() }

thread_local! {
    static FROM: RefCell<Option<i64>> = RefCell::new(None);
    /// Set by the command line before a build: whether the next build keeps a timeline.
    pub static ENABLE: RefCell<bool> = RefCell::new(false);
}
pub fn enabled() -> bool { ENABLE.with(|e| *e.borrow()) }
pub fn set_enabled(v: bool) { ENABLE.with(|e| *e.borrow_mut() = v); }

#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub tick: i64,
    /// 'M' main pass, 'D' passes over put-off cells after it, 'C' scaffold clean-up
    pub phase: char,
    pub placed: i64,
    /// target cells that stand as the target wants them (in the bot's picture), the most so far, and skipped cells
    pub done: i64, pub done_max: i64, pub skipped: i64,
    /// put-off cells (all; and those that still need a block)
    pub putoff: i64, pub putoff_need: i64,
    pub scaffold: i64,
    /// exit monitor: currently shut in, episodes, first shut tick, "already shut" route refusals
    pub shut: bool, pub shut_episodes: i64, pub first_shut: i64, pub already_shut: i64,
    pub stranded: bool, pub time_short: bool,
    pub rope_undone: i64, pub trips: i64, pub broken: i64, pub broken_out: i64, pub mined: i64, pub unstuck: i64,
    pub fp: u64,
}

pub struct Timeline {
    pub samples: Vec<Sample>,
    tgt_cells: Vec<u32>,
    done_max: i64,
    pub target: i64,
    pub max_ticks: i64,
}

#[inline]
fn mix(h: u64, v: u64) -> u64 { (h ^ v).wrapping_mul(0x100000001b3).rotate_left(29) ^ (v >> 7) }

impl Timeline {
    pub fn new(b: &Bot) -> Timeline {
        let tgt_cells: Vec<u32> = (0..b.g.tgt.len()).filter(|&i| b.g.tgt[i] != 0 && !t().secondary[b.g.tgt[i] as usize]).map(|i| i as u32).collect();
        Timeline { samples: Vec::new(), target: tgt_cells.len() as i64, tgt_cells, done_max: 0, max_ticks: b.max_ticks }
    }

    /// One sample (reads the bot only).
    pub fn sample(&mut self, b: &Bot) {
        let mut done = 0i64;
        let mut h: u64 = 0xcbf29ce484222325;
        for &i in &self.tgt_cells {
            let i = i as usize;
            let c = b.g.cur[i];
            h = mix(h, c as u64);
            if b.matches(c, b.g.tgt[i]) { done += 1; }
        }
        if done > self.done_max { self.done_max = done; }
        let putoff_need = b.deferred.keys().iter().filter(|&&c| b.needs(c as usize)).count() as i64;
        let pl = &b.c.pl;
        for v in [b.ticks as u64, b.placed as u64, b.broken as u64, b.skipped_cells as u64, b.trips as u64, b.scaffold.len() as u64, b.deferred.len() as u64,
                  b.scaffold_placed as u64, b.nav.searches as u64, b.nav.total_expanded as u64, b.c.w.rand_draws, b.c.w.n_set, b.c.w.level_time as u64,
                  pl.pos[0].to_bits(), pl.pos[1].to_bits(), pl.pos[2].to_bits(), pl.vel[0].to_bits(), pl.vel[1].to_bits(), pl.vel[2].to_bits(),
                  b.c.yaw().to_bits() as u64, b.c.pitch().to_bits() as u64, pl.health.to_bits() as u64, pl.on_ground as u64] { h = mix(h, v); }
        for s in pl.inv.iter() { h = mix(h, ((s.item as u64) << 8) | s.count as u64); }
        for s in b.stock.iter() { h = mix(h, s.count as u64); }
        self.samples.push(Sample {
            tick: b.ticks,
            phase: if b.cleaning { 'C' } else if b.main_done { 'D' } else { 'M' },
            placed: b.placed, done, done_max: self.done_max, skipped: b.skipped_cells,
            putoff: b.deferred.len() as i64, putoff_need,
            scaffold: b.scaffold.len() as i64,
            shut: b.was_shut, shut_episodes: b.shut_episodes, first_shut: b.first_shut, already_shut: b.already_shut,
            stranded: b.stranded, time_short: b.time_short(),
            rope_undone: b.rope_undone, trips: b.trips, broken: b.broken, broken_out: b.broken_out.len() as i64, mined: b.mined, unstuck: b.unstuck,
            fp: h,
        });
    }

    pub fn fp_at(&self, tick: i64) -> Option<u64> { self.samples.iter().find(|s| s.tick == tick).map(|s| s.fp) }

    /// Reads what `tsv` wrote (to try rules on stored timelines: `mcsim detect FILE...`).
    pub fn from_tsv(text: &str) -> Timeline {
        let mut tl = Timeline { samples: Vec::new(), tgt_cells: Vec::new(), done_max: 0, target: 0, max_ticks: crate::build::TICK_LIMIT };
        for l in text.lines() {
            if let Some(h) = l.strip_prefix("# ") {
                for kv in h.split_whitespace() { match kv.split_once('=') { Some(("target", v)) => tl.target = v.parse().unwrap(), Some(("max_ticks", v)) => tl.max_ticks = v.parse().unwrap(), _ => {} } }
                continue;
            }
            if l.starts_with("tick") { continue; }
            let f: Vec<&str> = l.split('\t').collect();
            let g = |i: usize| f[i].parse::<i64>().unwrap();
            tl.samples.push(Sample { tick: g(0), phase: f[1].chars().next().unwrap(), placed: g(2), done: g(3), done_max: g(4), skipped: g(5), putoff: g(6), putoff_need: g(7), scaffold: g(8),
                shut: g(9) != 0, shut_episodes: g(10), first_shut: g(11), already_shut: g(12), stranded: g(13) != 0, time_short: g(14) != 0, rope_undone: g(15), trips: g(16), broken: g(17),
                broken_out: g(18), mined: g(19), unstuck: g(20), fp: u64::from_str_radix(f[21], 16).unwrap() });
        }
        tl
    }

    pub fn tsv(&self) -> String {
        let mut o = format!("# target={} max_ticks={}\n", self.target, self.max_ticks);
        o += &String::from("tick\tphase\tplaced\tdone\tdoneMax\tskipped\tputOff\tputOffNeed\tscaffold\tshut\tshutEpisodes\tfirstShut\talreadyShut\tstranded\ttimeShort\tropeUndone\ttrips\tbroken\tbrokenOut\tmined\tunstuck\tfp\n");
        for s in &self.samples {
            o += &format!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:016x}\n", s.tick, s.phase, s.placed, s.done, s.done_max, s.skipped, s.putoff, s.putoff_need, s.scaffold,
                s.shut as i32, s.shut_episodes, s.first_shut, s.already_shut, s.stranded as i32, s.time_short as i32, s.rope_undone, s.trips, s.broken, s.broken_out, s.mined, s.unstuck, s.fp);
        }
        o
    }
}

// ------------------------------------------------------------------------------------------------ detector
#[derive(Clone, Debug)]
pub struct Derail { pub rule: &'static str, pub tick: i64, pub why: String }

/// The rules, in the order they are tried at each sample; the first sample at which any fires is the derailment.
/// Every rule looks only at samples up to the one it fires at, except where it asks for persistence (then it fires at
/// the onset, once the condition has held for the stated time).
pub const RULES: &[(&str, &str)] = &[
    ("shutin", "the exit monitor says no way back and stays shut for 6000 ticks, while the target gains < 0.5 % in them, with >= 2 % of the target missing (not in the clean-up, whose stale monitor stays shut)"),
    ("lost", "target cells that stood correctly are gone again: done fell 1 % of the target (and >= 20 cells) below its maximum"),
    ("putoff", "after the main pass (its end hands every blocked cell over: not counted), put-off cells that still need a block jumped by 3 % of the target (>= 50 cells) within 6000 ticks"),
    ("skips", "skipped (given-up) cells jumped by 3 % of the target (>= 50 cells) within 6000 ticks"),
    ("stranded", "a restock failed (stranded) with >= 2 % of the target missing, and it stays stranded 150000 ticks or to the end"),
    ("stall", "no placement from some tick to the end of the run (>= 24000 ticks) with >= 2 % of the target missing"),
    ("pace", "main pass, from t100000: at the rate of the last 200000 ticks the target is not done before time_short() would stop it, and that holds for 100000 ticks"),
    ("timeshort", "time_short() with more than 1 % of the target missing"),
];

pub fn detect(tl: &Timeline) -> Option<Derail> {
    let s = &tl.samples;
    let n = s.len();
    if n == 0 { return None; }
    let tgt = tl.target.max(1);
    let w = |ticks: i64| ((ticks + EVERY - 1) / EVERY) as usize;
    // the sample from which nothing more is placed (stall)
    let mut flat_from = n - 1;
    while flat_from > 0 && s[flat_from - 1].placed == s[n - 1].placed { flat_from -= 1; }
    for k in 0..n {
        let c = &s[k];
        // missing: not standing as the target wants it (skipped cells included)
        let missing = tgt - c.done;
        if c.shut && (k == 0 || !s[k - 1].shut) && c.phase != 'C' && missing * 50 >= tgt {
            let end = (k + w(6000)).min(n - 1);
            if (k..=end).all(|j| s[j].shut) && (end > k || k == n - 1) && (s[end].done - c.done) * 200 < tgt {
                return Some(Derail { rule: "shutin", tick: c.tick, why: format!("shut in from t{} (episode {}, phase {}), still shut at t{}, done {} -> {}", c.tick, c.shut_episodes, c.phase, s[end].tick, c.done, s[end].done) });
            }
        }
        let lost = c.done_max - c.done;
        if lost >= 20 && lost * 100 >= tgt { return Some(Derail { rule: "lost", tick: c.tick, why: format!("done {} < max {} (phase {})", c.done, c.done_max, c.phase) }); }
        let back = k.saturating_sub(w(6000));
        let base = s[back..=k].iter().map(|x| x.putoff_need).min().unwrap();
        let jump = c.putoff_need - base;
        if c.phase != 'M' && s[back].phase != 'M' && jump >= 50 && jump * 100 >= 3 * tgt { return Some(Derail { rule: "putoff", tick: c.tick, why: format!("put-off needing a block {} -> {} (phase {})", base, c.putoff_need, c.phase) }); }
        let jump = c.skipped - s[back].skipped;
        if jump >= 50 && jump * 100 >= 3 * tgt { return Some(Derail { rule: "skips", tick: c.tick, why: format!("skipped {} -> {} (phase {})", s[back].skipped, c.skipped, c.phase) }); }
        if c.stranded && (k == 0 || !s[k - 1].stranded) && missing * 50 >= tgt {
            let mut e = k;
            while e + 1 < n && s[e + 1].stranded { e += 1; }
            if e == n - 1 || s[e].tick - c.tick >= 150_000 { return Some(Derail { rule: "stranded", tick: c.tick, why: format!("stranded t{}..t{} with {missing} of {tgt} missing", c.tick, s[e].tick) }); }
        }
        if k == flat_from && s[n - 1].tick - c.tick >= 24000 && missing * 50 >= tgt {
            return Some(Derail { rule: "stall", tick: c.tick, why: format!("no placement t{}..t{} (phase {}, {missing} missing)", c.tick, s[n - 1].tick, c.phase) });
        }
        if c.phase == 'M' && c.tick >= 100_000 && pace_short(tl, k) {
            let end = (k + w(100_000)).min(n - 1);
            if (k..=end).all(|j| s[j].phase != 'M' || pace_short(tl, j)) && (end > k || k == n - 1) {
                return Some(Derail { rule: "pace", tick: c.tick, why: format!("{missing} of {tgt} missing at {:.4} cells/tick", rate(tl, k)) });
            }
        }
        if c.time_short && missing * 100 > tgt { return Some(Derail { rule: "timeshort", tick: c.tick, why: format!("time short with {missing} of {tgt} missing") }); }
    }
    None
}

fn rate(tl: &Timeline, k: usize) -> f64 {
    let s = &tl.samples;
    let back = k.saturating_sub((200_000 / EVERY) as usize);
    (s[k].done - s[back].done) as f64 / (s[k].tick - s[back].tick).max(1) as f64
}
/// At the recent rate, would the main pass still be short of the target when time_short() strikes?
fn pace_short(tl: &Timeline, k: usize) -> bool {
    let c = &tl.samples[k];
    let missing = (tl.target - c.done).max(0) as f64;
    let stop = (tl.max_ticks - 45 * c.scaffold - 40000) as f64;
    let r = rate(tl, k);
    missing * 100.0 > tl.target as f64 && (r <= 0.0 || c.tick as f64 + missing / r > stop)
}

// ------------------------------------------------------------------------------------------------ checkpoint files
#[derive(Clone, Debug, Default)]
pub struct Checkpoint {
    pub hash: String, pub rev: String, pub tick: i64, pub ticks_limit: i64, pub stop_at: i64, pub fp: u64,
    /// the MCB_* / MCSIM_* environment of the run (revision switches, seed): set again by `resume`
    pub env: Vec<(String, String)>,
    pub rule: String, pub derail_tick: i64, pub row: String,
}

impl Checkpoint {
    pub fn write(&self, path: &str) -> std::io::Result<()> {
        std::fs::write(path, format!("mcsim-checkpoint 1 (deterministic replay: see sim/src/ckpt.rs)\nhash\t{}\nrev\t{}\ntick\t{}\nticks_limit\t{}\nstop_at\t{}\n{}fingerprint\t{:016x}\nrule\t{}\nderail_tick\t{}\nrow\t{}\n",
            self.hash, self.rev, self.tick, self.ticks_limit, self.stop_at, self.env.iter().map(|(k, v)| format!("env\t{k}={v}\n")).collect::<String>(), self.fp, self.rule, self.derail_tick, self.row))
    }
    pub fn read(path: &str) -> Result<Checkpoint, String> {
        let s = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let mut c = Checkpoint::default();
        let mut lines = s.lines();
        if !lines.next().unwrap_or("").starts_with("mcsim-checkpoint 1") { return Err(format!("{path}: not a checkpoint file")); }
        for l in lines {
            let (k, v) = l.split_once('\t').unwrap_or((l, ""));
            match k {
                "hash" => c.hash = v.into(), "rev" => c.rev = v.into(), "tick" => c.tick = v.parse().unwrap_or(0),
                "ticks_limit" => c.ticks_limit = v.parse().unwrap_or(0), "stop_at" => c.stop_at = v.parse().unwrap_or(0),
                "env" => { let (a, b) = v.split_once('=').unwrap_or((v, "")); c.env.push((a.into(), b.into())); }
                "fingerprint" => c.fp = u64::from_str_radix(v, 16).unwrap_or(0),
                "rule" => c.rule = v.into(), "derail_tick" => c.derail_tick = v.parse().unwrap_or(0), "row" => c.row = v.into(),
                _ => {}
            }
        }
        Ok(c)
    }
}

/// The environment that decides a run: MCB_* and MCSIM_* variables, without the diagnostics that only write files.
pub fn run_env() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::env::vars().filter(|(k, _)| (k.starts_with("MCB_") || k.starts_with("MCSIM_")) && !matches!(k.as_str(), "MCSIM_FROM_TICK" | "MCSIM_PROF" | "MCSIM_BAD" | "MCSIM_CAUSES")).collect();
    v.sort();
    v
}

/// A row without its wall-time and CPU-time columns (17th and last), for comparing runs.
pub fn row_key(row: &str) -> String {
    let mut f: Vec<&str> = row.split('\t').collect();
    if f.len() > 17 { f[16] = ""; let n = f.len(); f[n - 1] = ""; }
    f.join("\t")
}
