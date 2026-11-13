//! Shared state and helpers of the builder (the Java version's `BuildState`, and the fields of the classes built on it).
use super::grid::Grid;
use super::nav::Nav;
use super::ray::Ray;
use crate::fastutil::{IntMap, IntSet};
use crate::kinds::Kind;
use crate::oracle::Oracle;
use crate::props::*;
use crate::sim::Sim;
use crate::tables::*;
use crate::fasthash::{FastMap, FastSet};
use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct Stock {
    pub item: u16,
    pub count: i32,
    pub chest_x: i32, pub chest_y: i32, pub chest_z: i32,
    pub slot: usize,
}

#[derive(Default)]
pub struct Spec {
    pub placeable: bool,
    pub item: u16,
    pub first: u16,
    pub table: Option<Rc<Vec<u16>>>,
    pub multi: bool,
    pub content: u16,
    pub chest: bool,
    /// only a steep upward view gives the wanted state (a crafter, dropper or piston pointing down ...)
    pub up_only: bool,
}

#[derive(Clone, Default, Debug)]
pub struct Stance {
    pub x: i32, pub y: i32, pub z: i32,
    pub lean: i32,
    pub moss: bool,
    pub column: i32,
    pub cap: bool,
    pub cost: i32,
}

#[derive(Clone, Default, Debug)]
pub struct Click {
    pub nx: i32, pub ny: i32, pub nz: i32, pub face: i32,
    pub ax: f64, pub ay: f64, pub az: f64,
    pub yaw: f32, pub pitch: f32,
}

pub struct Bot {
    pub c: Sim,
    pub g: Grid,
    pub oracle: Oracle,
    pub ray: Ray,
    // ---- Body
    pub ticks: i64,
    pub max_ticks: i64,
    pub stop_at: i64,
    pub snap_at: i64, pub snap_r: i32,
    pub placed: i64,
    pub broken: i64,
    pub failed_clicks: i64,
    pub verbose: bool,
    pub inventory_open_by_me: bool,
    pub failed_step: i32,
    pub hold_sneak: bool,
    pub hotbar_use: i64,
    pub hotbar_stamp: [i64; 9],
    pub reach: f64,
    pub click: Click,
    // ---- BuildState
    pub stock: Vec<Stock>,
    pub scaffold_item: u16,
    pub scaffold_state: u16,
    pub tower_item: u16,
    pub nav: Nav,
    pub scaffold: IntSet,
    pub scaffold_in_column: Vec<i32>,
    pub skipped: Vec<bool>,
    pub bad_stand: FastSet<i32>,
    pub skipped_cells: i64, pub trips: i64, pub scaffold_placed: i64, pub replans: i64, pub plans: i64, pub plan_fails: i64,
    pub skip_reasons: BTreeMap<String, i32>,
    pub status: String,
    pub restocking: bool,
    pub stray_screens: i64,
    pub specs: Vec<Option<Rc<Spec>>>,
    pub col_next: Vec<i32>,
    pub col_next_set: bool,
    pub blocked: IntMap,
    pub deferred: IntSet,
    pub tries: IntMap,
    pub late_too: bool,
    pub main_done: bool,
    pub carried: FastSet<u16>,
    pub escapes: i64, pub helped: i64,
    pub accept_traps: bool,
    pub traps_avoided: i64, pub trap_checks: i64, pub trap_searches: i64, pub towers_rescued: i64, pub misplaced: i64,
    pub appeared: Vec<i32>,
    pub strays: i64,
    pub plan_top_up: u16,
    pub dbg_stand: i32, pub dbg_click: i32, pub dbg_nav: i32, pub dbg_pillar: i32, pub dbg_pillar_click: i32,
    pub layer_now: i32,
    pub unreachable: FastSet<i32>,
    pub unstuck: i64, pub mined: i64,
    pub force_mine: bool,
    pub shut_in: i32,
    pub push_cells: Option<Vec<bool>>,
    pub after_door: Option<FastMap<i32, i32>>,
    pub door_waiters: FastMap<i32, Vec<i32>>,
    pub door_needs: FastMap<i32, Vec<i32>>,
    pub door_events: Vec<i32>,
    pub changed_again: Vec<i32>,
    pub pushed_into: FastSet<i32>,
    pub machine_live: bool,
    pub broken_out: FastSet<i32>,
    pub have_scaffold: bool,
    pub no_scaffold_nav: bool,
    pub have_tower: bool,
    pub towers_off: bool,
    pub leaning: bool,
    pub rope: Vec<[i32; 8]>,
    pub rope_undone: i64, pub rope_orphaned: i64,
    pub want_held: u16,
    pub gates_passed: i64,
    pub one: [i32; 3],
    pub n_tower_blocks: i64, pub roped: i64,
    pub cleaning: bool,
    pub must_fetch: u16,
    pub stranded: bool,
    pub stranded_tick: i64,
    pub n_self: i64, pub n_natural: i64, pub n_lean: i64, pub n_pillar_stance: i64, pub n_pillar_moves: i64, pub n_bridge_moves: i64, pub n_dig_moves: i64,
    pub build_ticks: i64,
    pub seal_refused: bool,
    pub avoid_seal: u16,
    // ---- Moves
    pub unreachable_from: i32,
    pub unreachable_at: i64,
    pub spares: Option<FastMap<u16, i32>>,
    pub last_unstick: i64,
    pub desperate: bool,
    pub loose_fails: FastMap<i64, i32>,
    pub loose_blocked: FastSet<i64>,
    pub t_path: Vec<i32>,
    pub t_move: Vec<u8>,
    pub t_step: i32,
    pub pending_close: Vec<i32>,
    pub step_off: bool,
    pub behind_x: i32, pub behind_z: i32,
    pub ladders_used: i64,
    // ---- Placing
    pub sweeping: bool,
    // ---- Stances
    pub tower_state: u16,
    pub floating: Vec<[i32; 3]>,
    pub n_floating: usize,
    // ---- Scaffolds
    pub rescuing: bool,
    pub rescue_failed: FastSet<i32>,
    pub comes_back: IntMap,
    // ---- Supplies
    pub scaffold_trips: i32, pub tower_trips: i32,
    // ---- Builder
    pub tiles_x: i32, pub tiles_z: i32,
    pub tile_pending: Vec<i32>,
    pub escape_budget: i32,
    pub progress_mark: i64, pub progress_tick: i64,
    pub no_roofing: bool,
    pub chest_state: u16,
    // ---- added with 2d9a193
    /// Body: damage taken (number of times), and whether it ended the run
    pub hurts: i64, pub died: bool, pub last_health: f32,
    /// The last plain way found from the bot to the chests: the cells stood in, and every cell the body passes through.
    pub exit_nodes: FastSet<i32>, pub exit_cells: FastSet<i32>, pub exit_valid: bool,
    pub from_inside: i64, pub chained: i64, pub from_close: i64, pub pits: i64,
    /// Set by place_helper: the helper that carries the helper (or -1).
    pub helper_base: i32,
    /// Set while a block lent for a click is being taken away again (that one always goes).
    pub must_remove: bool,
    pub any_piston: bool,
    /// Scaffold pieces the clean-up does not go for at the moment (it still breaks them where it comes by), or None.
    pub leave_alone: Option<FastSet<i32>>,
    pub forced_worse: i64, pub tidied: i64,
    /// cells that wait for an up-only block above them (view order)
    pub view_waits: i64,
    /// times the bot did not break the temporary block it stood on because it would have dropped into a trap
    pub kept_underfoot: i64,
    /// Why the cell in hand was last put off (read by do_cell: the last resorts are for "no face to click from anywhere").
    pub defer_why: &'static str,
    pub main_ticks_at: i64, pub put_off_at_main: i64, pub scaffold_at_main: i64,
    pub shut_episodes: i64, pub first_shut: i64, pub exit_samples: [i64; 3],
    /// During the clean-up: the cells about to be taken away (a column from one block up, or one loose block), or None.
    pub going_away: Option<Vec<i32>>,
    /// Set while a search may put temporary blocks into a column's next cell (see scaffold_ok).
    pub relax_next: bool,
    /// The temporary block that lends a face to the placement in hand (see place_helper), or -1.
    pub helper_cell: i32,
    // Builder
    pub last_monitor: i64, pub was_shut: bool, pub retreats: i64, pub try_count: i32,
    // Moves
    /// False while way_out is asked about a world in which the remembered ways may be cut (cells taken away in thought).
    pub witness_usable: bool,
    /// False while the world asked about has cells taken away in thought: a way found there may not exist.
    pub witness_learn: bool,
    pub exit_unknown: i64, pub exit_joined: i64, pub relaxed_routes: i64, pub already_shut: i64,
    pub last_exit_expanded: i32, pub boxed_expanded: i32,
    // Placing
    pub close_failed: FastMap<i32, i32>,
    pub close_x: f64, pub close_z: f64, pub close_sneak: bool, pub close_sx: i32, pub close_sy: i32, pub close_sz: i32, pub close_pit: bool,
    // Scaffolds
    pub kept_for_exit: i64, pub stubs: i64, pub unsafe_spots: i64, pub last_cleanup_restock: i64,
    // Stances
    pub dbg_unsafe: i32,
    /// event log in the format of reference/java-extra/events (placements, scaffold cells, trips, skips)
    pub events: Option<std::io::BufWriter<std::fs::File>>,
    /// MCSIM_WATCH=x,y,z (build coordinates): trace the decisions about one cell
    pub watch: Option<[i32; 3]>,
}

impl Bot {
    pub fn new(c: Sim, g: Grid, stock: Vec<Stock>, scaffold_item: u16, scaffold_state: u16, oracle: Oracle) -> Bot {
        let tb = t();
        let ncell = g.cur.len();
        let ncol = (g.nx * g.nz) as usize;
        let nav = Nav::new(ncell);
        let chest = tb.default_of("chest");
        let mut b = Bot {
            c, g, oracle, ray: Ray::default(),
            ticks: 0, max_ticks: i64::MAX, stop_at: 0, snap_at: std::env::var("MCSIM_SNAP").ok().and_then(|v| v.split(',').next().and_then(|x| x.parse().ok())).unwrap_or(0), snap_r: std::env::var("MCSIM_SNAP").ok().and_then(|v| v.split(',').nth(1).and_then(|x| x.parse().ok())).unwrap_or(5), placed: 0, broken: 0, failed_clicks: 0, verbose: false, inventory_open_by_me: false, failed_step: -1, hold_sneak: false,
            hotbar_use: 0, hotbar_stamp: [0; 9], reach: 4.5, click: Click::default(),
            stock, scaffold_item, scaffold_state, tower_item: *tb.item_by_name.get("scaffolding").unwrap(), nav, scaffold: IntSet::new(),
            scaffold_in_column: vec![0; ncol], skipped: vec![false; ncell], bad_stand: FastSet::default(),
            skipped_cells: 0, trips: 0, scaffold_placed: 0, replans: 0, plans: 0, plan_fails: 0, skip_reasons: BTreeMap::new(), status: String::new(),
            restocking: false, stray_screens: 0, specs: vec![None; tb.n], col_next: Vec::new(), col_next_set: false, blocked: IntMap::new(), deferred: IntSet::new(),
            tries: IntMap::new(), late_too: true, main_done: false, carried: FastSet::default(), escapes: 0, helped: 0, accept_traps: false,
            traps_avoided: 0, trap_checks: 0, trap_searches: 0, towers_rescued: 0, misplaced: 0, appeared: Vec::new(), strays: 0, plan_top_up: 0,
            dbg_stand: 0, dbg_click: 0, dbg_nav: 0, dbg_pillar: 0, dbg_pillar_click: 0, layer_now: 0, unreachable: FastSet::default(), unstuck: 0, mined: 0,
            force_mine: false, shut_in: 0, push_cells: None, after_door: None, door_waiters: FastMap::default(), door_needs: FastMap::default(), door_events: Vec::new(),
            changed_again: Vec::new(), pushed_into: FastSet::default(), machine_live: false, broken_out: FastSet::default(), have_scaffold: true, no_scaffold_nav: false,
            have_tower: true, towers_off: false, leaning: false, rope: Vec::new(), rope_undone: 0, rope_orphaned: 0, want_held: 0, gates_passed: 0, one: [0; 3],
            n_tower_blocks: 0, roped: 0, cleaning: false, must_fetch: 0, stranded: false, stranded_tick: 0,
            n_self: 0, n_natural: 0, n_lean: 0, n_pillar_stance: 0, n_pillar_moves: 0, n_bridge_moves: 0, n_dig_moves: 0, build_ticks: 0, seal_refused: false, avoid_seal: 0,
            unreachable_from: -1, unreachable_at: -1, spares: None, last_unstick: -1000, desperate: false, loose_fails: FastMap::default(), loose_blocked: FastSet::default(),
            t_path: Vec::new(), t_move: Vec::new(), t_step: -1, pending_close: Vec::new(), step_off: false, behind_x: 0, behind_z: 0, ladders_used: 0,
            sweeping: false, tower_state: tb.default_of("scaffolding"), floating: vec![[0; 3]; 24], n_floating: 0,
            rescuing: false, rescue_failed: FastSet::default(), comes_back: IntMap::new(), scaffold_trips: 0, tower_trips: 0,
            tiles_x: 0, tiles_z: 0, tile_pending: Vec::new(), escape_budget: 40, progress_mark: -1, progress_tick: 0, no_roofing: true, chest_state: chest,
            hurts: 0, died: false, last_health: -1.0, exit_nodes: FastSet::default(), exit_cells: FastSet::default(), exit_valid: false,
            from_inside: 0, chained: 0, from_close: 0, pits: 0, helper_base: -1, must_remove: false, any_piston: false, leave_alone: None, forced_worse: 0, tidied: 0, view_waits: 0, kept_underfoot: 0,
            defer_why: "?", main_ticks_at: 0, put_off_at_main: 0, scaffold_at_main: 0, shut_episodes: 0, first_shut: 0, exit_samples: [0; 3], going_away: None,
            relax_next: false, helper_cell: -1, last_monitor: -100000, was_shut: false, retreats: 0, try_count: 0, witness_usable: true, witness_learn: true,
            exit_unknown: 0, exit_joined: 0, relaxed_routes: 0, already_shut: 0, last_exit_expanded: 0, boxed_expanded: 0,
            close_failed: FastMap::default(), close_x: 0.0, close_z: 0.0, close_sneak: false, close_sx: 0, close_sy: 0, close_sz: 0, close_pit: false,
            kept_for_exit: 0, stubs: 0, unsafe_spots: 0, last_cleanup_restock: -100000, dbg_unsafe: 0,
            events: None, watch: std::env::var("MCSIM_WATCH").ok().map(|v| { let p: Vec<i32> = v.split(',').map(|x| x.parse().unwrap()).collect(); [p[0], p[1], p[2]] }),
        };
        for k in 0..b.stock.len() {
            let (x, y, z) = (b.stock[k].chest_x, b.stock[k].chest_y, b.stock[k].chest_z);
            b.g.set(x, y, z, chest);
            b.g.set(x + 1, y, z, chest);
        }
        // cells where temporary blocks may always go (the static part of scaffold_ok / tower_ok)
        let (gx0, gy0, gz0) = (b.g.x0, b.g.y0, b.g.z0);
        b.piston_faces(gx0, gy0, gz0);
        for i in 0..b.g.cur.len() {
            let open = b.g.z_of(i) >= b.g.oz - 1 && b.g.tgt[i] == 0 && !b.push_cells.as_ref().unwrap()[i];
            if open { b.g.set_nav_flag(i, super::grid::N_OPEN, true); }
        }
        b
    }

    /// Event log line "<tick> P x,y,z state": a block was placed (the state now believed at the cell).
    pub fn ev_placed(&mut self, x: i32, y: i32, z: i32) {
        if self.events.is_none() { return; }
        let line = format!("{} P {} {}\n", self.ticks, self.rel(x, y, z), t().name(self.g.get(x, y, z)));
        self.ev_write(&line);
    }
    #[inline]
    pub fn watching(&self, x: i32, y: i32, z: i32) -> bool {
        match self.watch { Some(w) => x - self.g.ox == w[0] && y - self.g.oy == w[1] && z - self.g.oz == w[2], None => false }
    }
    pub fn wlog(&self, m: &str) {
        println!("[t{} @{},{},{} pos {:.3},{:.3},{:.3} yaw {:.1} sneak {}] WATCH {}", self.ticks, self.cx() - self.g.ox, self.cy() - self.g.oy, self.cz() - self.g.oz,
            self.c.x() - self.g.ox as f64, self.c.y() - self.g.oy as f64, self.c.z() - self.g.oz as f64, self.c.yaw(), self.c.sneaking(), m);
    }
    pub fn ev_write(&mut self, line: &str) {
        use std::io::Write;
        if let Some(w) = self.events.as_mut() { let _ = w.write_all(line.as_bytes()); }
    }

    #[inline]
    pub fn col(&self, x: i32, z: i32) -> usize { ((z - self.g.z0) * self.g.nx + (x - self.g.x0)) as usize }

    /// BuildState.tick: one tick, then close any screen a stray click opened.
    pub fn tick(&mut self) {
        self.body_tick();
        if !self.restocking && self.c.screen_open() && !self.inventory_open_by_me {
            self.stray_screens += 1;
            if self.verbose && self.stray_screens < 10 { self.log("UNEXPECTED SCREEN"); }
            self.c.close_screen();
            self.body_tick();
        }
        if self.snap_at > 0 && self.ticks >= self.snap_at { self.snap_at = 0; self.snapshot(); }
        // the Java run's wall-clock limit, given as the tick at which it struck (see `--stop-at`)
        if self.stop_at > 0 && (self.ticks & 63) == 0 && self.ticks >= self.stop_at { std::panic::panic_any(super::OutOfTime); }
    }

    /// MCSIM_SNAP=tick[,radius]: the bot's picture of the world around it at that tick, one map per layer (a
    /// diagnostic; x to the right, z downwards). `.` air, `_` air where the target has a block, `s` the bot's own
    /// temporary blocks, otherwise the first letter of the block's name (upper case: as the target wants it);
    /// the bot's column in brackets.
    fn snapshot(&self) {
        let r = self.snap_r;
        let (bx, by, bz) = (self.cx(), self.cy(), self.cz());
        println!("SNAP t{} bot at {} pos {:.3},{:.3},{:.3} on_ground={} health={}", self.ticks, self.rel(bx, by, bz), self.c.pl.pos[0] - self.g.ox as f64, self.c.pl.pos[1] - self.g.oy as f64, self.c.pl.pos[2] - self.g.oz as f64, self.c.on_ground(), self.c.pl.health);
        let mut names: std::collections::BTreeMap<String, i32> = Default::default();
        for y in (by - 2..=by + 3).rev() {
            println!("SNAP y={} (x {}..{}, z {}..{})", y - self.g.oy, bx - r - self.g.ox, bx + r - self.g.ox, bz - r - self.g.oz, bz + r - self.g.oz);
            for z in bz - r..=bz + r {
                let mut line = String::new();
                for x in bx - r..=bx + r {
                    let (w, tg, sc) = if self.g.inside(x, y, z) { let i = self.g.idx(x, y, z); (self.g.cur[i], self.g.tgt[i], self.scaffold.contains(i as i32)) } else { (0, 0, false) };
                    let ch = if sc { 's' } else if w == 0 { if tg != 0 { '_' } else { '.' } } else {
                        let n = t().name(w);
                        *names.entry(n.clone()).or_insert(0) += 1;
                        let c = n.chars().next().unwrap();
                        if tg != 0 && self.matches(w, tg) { c.to_ascii_uppercase() } else { c }
                    };
                    if x == bx && z == bz { line.push('['); line.push(ch); line.push(']'); } else { line.push(' '); line.push(ch); line.push(' '); }
                }
                println!("SNAP   {line}");
            }
        }
        println!("SNAP blocks: {}", names.iter().map(|(k, v)| format!("{k} x{v}")).collect::<Vec<_>>().join("; "));
        println!("SNAP inventory: {}", (0..36).filter(|&k| self.c.pl.inv[k].count > 0).map(|k| format!("{}x{}", t().items[self.c.pl.inv[k].item as usize].name, self.c.pl.inv[k].count)).collect::<Vec<_>>().join(" "));
    }

    pub fn fail(&mut self, why: &str) -> bool {
        if self.verbose { self.log(&format!("FAIL {why}")); }
        false
    }
    pub fn log(&self, m: &str) {
        if self.verbose { println!("[t{} @{},{},{}] {}", self.ticks, self.cx() - self.g.ox, self.cy() - self.g.oy, self.cz() - self.g.oz, m); }
    }
    pub fn rel(&self, x: i32, y: i32, z: i32) -> String { format!("{},{},{}", x - self.g.ox, y - self.g.oy, z - self.g.oz) }

    pub fn on_block_change(&mut self, x: i32, y: i32, z: i32, state: u16) {
        if self.watch.is_some() && self.g.get(x, y, z) != (if t().air[state as usize] { 0 } else { state }) { self.wlog(&format!("change {} : {} -> {}", self.rel(x, y, z), t().name(self.g.get(x, y, z)), t().name(state))); }
        if self.events.is_some() && super::moves::ev_more() { let l = format!("{} C {} {}\n", self.ticks, self.rel(x, y, z), t().name(state)); self.ev_write(&l); }
        self.g.set(x, y, z, state);
        if !self.g.inside(x, y, z) { return; }
        let tb = t();
        let i = self.g.idx(x, y, z);
        if tb.technical[state as usize] && self.piston_faces(x, y, z) { self.machine_live = true; if self.g.tgt[i] != 0 { self.pushed_into.insert(i as i32); } }
        if !tb.air[state as usize] && !self.main_done && self.door_waiters.contains_key(&(i as i32)) { self.door_events.push(i as i32); }
        // the remembered way out (see way_out): gone when anything was taken away, or something now stands in it
        if self.exit_valid && (tb.air[state as usize] || self.exit_cells.contains(&(i as i32))) { self.exit_valid = false; }
        if tb.air[state as usize] {
            if self.scaffold.remove(i as i32) { let c = self.col(x, z); self.scaffold_in_column[c] -= 1; self.g.set_nav_flag(i, super::grid::N_SCAF, false); if self.events.is_some() { let l = format!("{} S- {}\n", self.ticks, self.rel(x, y, z)); self.ev_write(&l); } }
        } else if self.appeared.len() < 100000 { self.appeared.push(i as i32); }
        if self.g.tgt[i] != 0 && self.col_next_set && self.g.in_box(x, y, z) && self.col_next[self.col(x, z)] > y && self.changed_again.len() < 100000 { self.changed_again.push(i as i32); }
    }

    pub fn add_scaffold(&mut self, x: i32, y: i32, z: i32) {
        let i = self.g.idx(x, y, z);
        if self.scaffold.add(i as i32) { let c = self.col(x, z); self.scaffold_in_column[c] += 1; self.scaffold_placed += 1; self.g.set_nav_flag(i, super::grid::N_SCAF, true); if self.events.is_some() { let l = format!("{} S+ {}\n", self.ticks, self.rel(x, y, z)); self.ev_write(&l); } }
    }

    pub fn break_ticks(&self, state: u16) -> i32 {
        let tb = t();
        // (Block.defaultDestroyTime: the block's hardness)
        let h = tb.destroy_time[tb.binfo(state).default as usize];
        if h < 0.0 { return -1; }
        let k: f32 = if tb.requires_tool[state as usize] { 100.0 } else { 30.0 };
        ((20.0f32 + h * k * 1.2f32).min(6000.0f32)) as i32
    }

    pub fn spec(&mut self, state: u16) -> Rc<Spec> {
        if let Some(s) = &self.specs[state as usize] { return s.clone(); }
        let tb = t();
        let mut s = Spec { item: tb.item[state as usize], ..Default::default() };
        if s.item != 0 && tb.item_count[state as usize] != 0 {
            let mut first = state;
            if tb.item_count[state as usize] > 1 {
                s.multi = true;
                if has(state, P_TYPE) && kind_of(state) == Kind::SlabBlock { first = with(state, P_TYPE, V_BOTTOM); }
                for pr in [P_LAYERS, P_CANDLES, P_PICKLES, P_EGGS, P_FLOWER_AMOUNT, P_SEGMENT_AMOUNT] { if has(state, pr) { first = with(first, pr, 1); } }
            }
            s.content = content_item(state);
            if s.content != 0 { first = tb.default_of("flower_pot"); }
            s.first = first;
            s.chest = kind_of(state) == Kind::ChestBlock;
            let table = self.oracle.table(s.item);
            let want = tb.loose[s.first as usize];
            let (mut any, mut all) = (false, true);
            for &e in table.iter() { if e != 0 && tb.loose[e as usize] == want { any = true; } else { all = false; } }
            s.placeable = any;
            if any && !all {
                // (the pitch classes of the table are 20 degrees wide, centred on -80, -60, -40 ... 80; at -40 "up" is
                // still the nearest direction of a view along a diagonal)
                s.up_only = table.iter().enumerate().all(|(k, &e)| e == 0 || tb.loose[e as usize] != want || (k / 2) % crate::oracle::NPITCH <= 2);
            }
            s.table = if all { None } else { Some(table) };
            if crate::is_blk!(state, "bamboo") { s.placeable = true; s.table = None; }
        }
        let rc = Rc::new(s);
        self.specs[state as usize] = Some(rc.clone());
        rc
    }

    pub fn matches(&self, world: u16, target: u16) -> bool {
        if world == 0 { return false; }
        let tb = t();
        if tb.canon[world as usize] == tb.canon[target as usize] { return true; }
        if shoot_for(world, target) { return true; }
        tb.loose[world as usize] == tb.loose[target as usize] && kind_of(target) == Kind::ChestBlock
    }

    /// For a chest cell: the exact state to produce now - joined with its partner if that is there, single otherwise.
    pub fn chest_want(&self, x: i32, y: i32, z: i32, tg: u16) -> u16 {
        let ty = val(tg, P_TYPE);
        if ty == V_SINGLE { return tg; }
        // ChestBlock.getConnectedDirection: LEFT -> clockwise of facing, RIGHT -> counter-clockwise
        let facing = crate::geom::dir_of(val(tg, P_FACING));
        let dir = if ty == V_LEFT { crate::geom::clockwise(facing) } else { crate::geom::counter_clockwise(facing) };
        let st = crate::geom::STEP[dir as usize];
        let pp = self.g.get(x + st.0, y, z + st.2);
        if pp != 0 && same_block(pp, tg) && get(pp, P_FACING) == get(tg, P_FACING) && get(pp, P_TYPE) != Some(ty) { return tg; }
        with(tg, P_TYPE, V_SINGLE)
    }

    #[inline]
    pub fn needs(&self, i: usize) -> bool {
        let tg = self.g.tgt[i];
        tg != 0 && !self.skipped[i] && !self.matches(self.g.cur[i], tg) && !t().secondary[tg as usize]
    }

    pub fn skip(&mut self, i: usize, why: &str) {
        if self.skipped[i] { return; }
        self.skipped[i] = true;
        self.skipped_cells += 1;
        *self.skip_reasons.entry(why.to_string()).or_insert(0) += 1;
        if self.events.is_some() { let l = format!("{} X {} {}\n", self.ticks, why.replace(' ', "_"), self.rel(self.g.x_of(i), self.g.y_of(i), self.g.z_of(i))); self.ev_write(&l); }
        if self.verbose && self.skipped_cells < 60 { self.log(&format!("skip {} {}: {}", self.rel(self.g.x_of(i), self.g.y_of(i), self.g.z_of(i)), t().name(self.g.tgt[i]), why)); }
    }

    pub fn is_supply_chest(&self, x: i32, y: i32, z: i32) -> bool { z < self.g.oz - 2 && kind_of(self.g.get(x, y, z)) == Kind::ChestBlock }

    /// Temporary blocks may go where the target has nothing, or where its turn has not come yet.
    pub fn scaffold_ok(&mut self, x: i32, y: i32, z: i32) -> bool {
        if !self.g.inside(x, y, z) || !self.have_scaffold || self.no_scaffold_nav { return false; }
        // (N_OPEN is the static part of what follows: outside the target, not in front of a piston, not north of the site)
        if self.g.nv(self.g.pidx(x, y, z)) & super::grid::N_OPEN != 0 { return true; }
        if z < self.g.oz - 1 || self.piston_faces(x, y, z) { return false; }
        let i = self.g.idx(x, y, z);
        if self.g.tgt[i] == 0 || (self.broken_out.contains(&(i as i32)) && self.g.cur[i] == 0) { return true; }
        if !self.g.in_box(x, y, z) || !self.col_next_set { return false; }
        // (relax_next: also in the column's next cell itself. Only for ways out and for routes that cannot be had
        // otherwise - the way off a spot may be a pillar there)
        self.layer_now == i32::MAX || y > self.col_next[self.col(x, z)] || (self.relax_next && y == self.col_next[self.col(x, z)])
    }

    pub fn piston_faces(&mut self, x: i32, y: i32, z: i32) -> bool {
        if self.push_cells.is_none() {
            let mut pc = vec![false; self.g.tgt.len()];
            for i in 0..self.g.tgt.len() {
                let st = self.g.tgt[i];
                if st == 0 || kind_of(st) != Kind::PistonBaseBlock { continue; }
                self.any_piston = true;
                let f = crate::geom::STEP[crate::geom::dir_of(val(st, P_FACING)) as usize];
                let (px, py, pz) = (self.g.x_of(i) + f.0, self.g.y_of(i) + f.1, self.g.z_of(i) + f.2);
                if self.g.inside(px, py, pz) { let k = self.g.idx(px, py, pz); pc[k] = true; }
            }
            self.push_cells = Some(pc);
        }
        self.push_cells.as_ref().unwrap()[self.g.idx(x, y, z)]
    }

    /// Does the build contain a piston anywhere? (Then it is a machine, and temporary blocks are used sparingly.)
    pub fn has_pistons(&mut self) -> bool { let (x, y, z) = (self.g.ox, self.g.oy, self.g.oz); self.piston_faces(x, y, z); self.any_piston }

    pub fn in_depot(&self, it: u16) -> i32 { self.stock.iter().filter(|st| st.item == it && st.count > 0).map(|st| st.count).sum() }

    /// Counters for the result file (one column, key=value;...): what happened to the bot, apart from the verdict.
    pub fn metrics(&self) -> String {
        format!("died={};hurt={};retreats={};shutEpisodes={};firstShut={};exitSamples={}/{}/{};stubs={};alreadyShut={};exitUnknown={};exitJoined={};mainTicks={};putOff={};scaffoldAtMain={};relaxedRoutes={};unstuck={};mined={};trapsAvoided={};fromInside={};chained={};fromClose={};pits={};forcedWorse={};tidied={}",
            self.died as i32, self.hurts, self.retreats, self.shut_episodes, self.first_shut, self.exit_samples[2], self.exit_samples[1], self.exit_samples[0], self.stubs, self.already_shut, self.exit_unknown, self.exit_joined,
            self.main_ticks_at, self.put_off_at_main, self.scaffold_at_main, self.relaxed_routes, self.unstuck, self.mined, self.traps_avoided, self.from_inside, self.chained, self.from_close, self.pits, self.forced_worse, self.tidied)
    }

    fn cell_or_minus(&self, x: i32, y: i32, z: i32) -> i32 { if self.g.inside(x, y, z) { self.g.idx(x, y, z) as i32 } else { -1 } }

    fn find_door_order(&mut self) {
        let tb = t();
        let mut after: FastMap<i32, i32> = FastMap::default();
        for i in 0..self.g.tgt.len() {
            let tg = self.g.tgt[i];
            if tg == 0 || kind_of(tg) != Kind::DoorBlock { continue; }
            if val(tg, P_HALF) != V_LOWER { continue; }
            let facing = crate::geom::dir_of(val(tg, P_FACING));
            let want_right = val(tg, P_HINGE) == V_RIGHT;
            let left = crate::geom::STEP[crate::geom::counter_clockwise(facing) as usize];
            let right = crate::geom::STEP[crate::geom::clockwise(facing) as usize];
            let (x, y, z) = (self.g.x_of(i), self.g.y_of(i), self.g.z_of(i));
            let l = [self.cell_or_minus(x + left.0, y, z + left.2), self.cell_or_minus(x + left.0, y + 1, z + left.2)];
            let r = [self.cell_or_minus(x + right.0, y, z + right.2), self.cell_or_minus(x + right.0, y + 1, z + right.2)];
            let mut balance = 0;
            for c2 in l { if c2 >= 0 && tb.full_cube[self.g.tgt[c2 as usize] as usize] { balance -= 1; } }
            for c2 in r { if c2 >= 0 && tb.full_cube[self.g.tgt[c2 as usize] as usize] { balance += 1; } }
            let left_door = l[0] >= 0 && lower_door(self.g.tgt[l[0] as usize]);
            let right_door = r[0] >= 0 && lower_door(self.g.tgt[r[0] as usize]);
            let forced_right = (left_door && !right_door) || balance > 0;
            let forced_left = !forced_right && ((right_door && !left_door) || balance < 0);
            if !(if want_right { forced_left } else { forced_right }) {
                for c2 in [l[0], l[1], r[0], r[1]] {
                    if c2 < 0 || !tb.full_cube[self.g.tgt[c2 as usize] as usize] { continue; }
                    self.door_needs.entry(i as i32).or_default().push(c2);
                    self.door_waiters.entry(c2).or_default().push(i as i32);
                }
                continue;
            }
            let mut wait: Vec<i32> = Vec::new();
            let side = if want_right { l } else { r };
            for c2 in side { if c2 >= 0 && tb.full_cube[self.g.tgt[c2 as usize] as usize] { wait.push(c2); } }
            let other_door = if want_right { r[0] } else { l[0] };
            if other_door >= 0 && lower_door(self.g.tgt[other_door as usize]) && !(if want_right { left_door } else { right_door }) { wait.push(other_door); }
            for &wk in &wait {
                if after.get(&wk).copied().unwrap_or(-1) < 0 && after.get(&(i as i32)).copied().unwrap_or(-1) != wk { after.insert(wk, i as i32); }
            }
            self.door_waiters.insert(i as i32, wait);
        }
        // View order: a block that only goes in with a steep upward view is placed from below, through the cell
        // under it. What the target has in that cell waits until the block above stands (or is given up) - the
        // same waiting as for the doors, and with the same end: once traps are accepted nothing waits any more.
        if crate::rev::rev().view_order {
            let layer = (self.g.nx * self.g.nz) as usize;
            for i in 0..self.g.tgt.len() {
                let tg = self.g.tgt[i];
                if tg == 0 || i < layer { continue; }
                let l = i - layer;
                if self.g.tgt[l] == 0 || self.g.y_of(l) < self.g.oy || tb.secondary[tg as usize] { continue; }
                let sp = self.spec(tg);
                if !sp.placeable || !sp.up_only { continue; }
                if after.get(&(l as i32)).copied().unwrap_or(-1) >= 0 || self.door_waiters.contains_key(&(i as i32)) { continue; }
                after.insert(l as i32, i as i32);
                self.door_waiters.insert(i as i32, vec![l as i32]);
                self.view_waits += 1;
            }
        }
        if self.verbose && self.view_waits > 0 { self.log(&format!("view order: {} cells wait for an up-only block above them", self.view_waits)); }
        self.after_door = Some(after);
    }

    pub fn waits_for_door(&mut self, cell: usize) -> bool {
        if self.after_door.is_none() { self.find_door_order(); }
        let ad = self.after_door.as_ref().unwrap();
        if (ad.is_empty() && self.door_needs.is_empty()) || self.accept_traps { return false; }
        let door = ad.get(&(cell as i32)).copied().unwrap_or(-1);
        if door >= 0 && self.needs(door as usize) { return true; }
        if let Some(pre) = self.door_needs.get(&(cell as i32)) {
            for &k in pre { if self.needs(k as usize) { return true; } }
        }
        false
    }

    pub fn is_late(&mut self, cell: usize) -> bool {
        let tg = self.g.tgt[cell];
        if tg == 0 { return false; }
        if t().late[tg as usize] || self.pushed_into.contains(&(cell as i32)) { return true; }
        self.machine_live && { let (x, y, z) = (self.g.x_of(cell), self.g.y_of(cell), self.g.z_of(cell)); self.piston_faces(x, y, z) }
    }

    pub fn check_scaffold(&mut self) {
        let mut have = self.count(self.scaffold_item) > 0;
        if !have && !self.stranded { have = self.stock.iter().any(|s| s.item == self.scaffold_item && s.count > 0); }
        if have != self.have_scaffold { self.have_scaffold = have; self.unreachable.clear(); self.log(if have { "scaffolding available again" } else { "out of scaffolding" }); }
        let mut tw = self.count(self.tower_item) > 0;
        if !tw && !self.stranded { tw = self.stock.iter().any(|s| s.item == self.tower_item && s.count > 0); }
        if tw != self.have_tower { self.have_tower = tw; self.unreachable.clear(); }
    }

    /// Where a scaffolding column may stand: wherever the target has nothing.
    pub fn tower_ok(&mut self, x: i32, y: i32, z: i32) -> bool {
        if !self.g.inside(x, y, z) || !self.have_tower || self.no_scaffold_nav || self.towers_off { return false; }
        if self.g.nv(self.g.pidx(x, y, z)) & super::grid::N_OPEN != 0 { return true; }
        if z < self.g.oz - 1 { return false; }
        if self.piston_faces(x, y, z) { return false; }
        let i = self.g.idx(x, y, z);
        self.g.tgt[i] == 0 || (self.broken_out.contains(&(i as i32)) && self.g.cur[i] == 0)
    }

    pub fn short_of_scaffold(&self) -> bool {
        (self.nav.scaffold_moves > 0 && self.count(self.scaffold_item) < self.nav.scaffold_moves) || self.nav.tower_moves > self.count(self.tower_item)
    }

    pub fn scaffold_left(&self) -> usize { self.scaffold.len() }
}

pub fn lower_door(tg: u16) -> bool { tg != 0 && kind_of(tg) == Kind::DoorBlock && val(tg, P_HALF) == V_LOWER }

/// Same block as far as judging goes (a filled cauldron counts as a cauldron ...).
pub fn same_kind(a: u16, b: u16) -> bool { let tb = t(); same_block(tb.canon[a as usize], tb.canon[b as usize]) }

pub fn shoot_for(world: u16, target: u16) -> bool { crate::is_blk!(target, "bamboo") && crate::is_blk!(world, "bamboo_sapling") }

pub fn same_family(a: u16, b: u16) -> bool {
    if shoot_for(a, b) || shoot_for(b, a) { return true; }
    // (description ids: "block.minecraft.<name>")
    let (na, nb) = (&t().binfo(a).name, &t().binfo(b).name);
    na.replace("wall_", "") == nb.replace("wall_", "") || na.replace("_plant", "") == nb.replace("_plant", "")
}

/// For a potted plant: the plant that goes into the pot (item id), else 0.
pub fn content_item(state: u16) -> u16 {
    let tb = t();
    if kind_of(state) != Kind::FlowerPotBlock { return 0; }
    let name = &tb.binfo(state).name;
    let Some(plant) = name.strip_prefix("potted_") else { return 0 };
    // FlowerPotBlock.getPotted().asItem(): the plant block's item (azalea bushes are named differently)
    let pb = match plant { "azalea_bush" => "azalea", "flowering_azalea_bush" => "flowering_azalea", o => o };
    match tb.block_by_name.get(pb) { Some(&b) => tb.blocks[b as usize].item, None => 0 }
}

pub struct RingData { pub ring: Vec<[i32; 2]> }
pub fn ring() -> &'static Vec<[i32; 2]> {
    static R: std::sync::OnceLock<Vec<[i32; 2]>> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        let mut l = Vec::new();
        for dx in -4..=4 { for dz in -4..=4 { if dx * dx + dz * dz <= 17 { l.push([dx, dz]); } } }
        // (List.sort is stable)
        l.sort_by_key(|a: &[i32; 2]| a[0] * a[0] + a[1] * a[1]);
        l
    })
}
