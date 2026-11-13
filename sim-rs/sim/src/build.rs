//! Target normalisation, the build site with its supply chests, the judge, and one build from start to verdict
//! (the Java version's `Target`, `Site`, `Judge`, `BuildRun`).
use crate::bot::grid::Grid;
use crate::bot::state::{content_item, Bot, Stock};
use crate::geom::*;
use crate::kinds::Kind;
use crate::oracle::Oracle;
use crate::props::*;
use crate::schem::Schem;
use crate::sim::Sim;
use crate::tables::*;
use crate::world::{Stack, World};
use std::collections::BTreeMap;

pub const TICK_LIMIT: i64 = 48 * 3600 * 20;
pub const ROW_GAP: i32 = 5;
pub const ROW_PITCH: i32 = 3;
pub const STACK_HEIGHT: i32 = 3;
pub const GROUND_TOP: i32 = -61;
pub const MAX_Y: i32 = 319;

pub struct Target {
    pub sx: i32, pub sy: i32, pub sz: i32,
    pub ids: Vec<u16>,
    pub blocks: i64,
}

impl Target {
    pub fn new(s: &Schem) -> Target {
        let tb = t();
        let mut ids = vec![0u16; s.ids.len()];
        let layer = (s.sx * s.sz) as usize;
        let mut blocks = 0;
        for i in 0..ids.len() {
            let mut id = s.ids[i] as usize;
            if id >= tb.n { continue; }
            if id == 0 || tb.air[id] { continue; }
            if tb.fluid[id] { continue; }
            if tb.technical[id] { continue; }
            id = tb.dewater[id] as usize;
            if tb.gravity[id] && i >= layer {
                let below = ids[i - layer] as usize;
                if below == 0 || tb.replaceable[below] { continue; }
            }
            ids[i] = id as u16;
            blocks += 1;
        }
        Target { sx: s.sx, sy: s.sy, sz: s.sz, ids, blocks }
    }
}

// ------------------------------------------------------------------------------------------------ judge
#[derive(Default, Debug)]
pub struct Judged {
    pub sections: i32, pub failed_sections: i32,
    pub target_cells: i64, pub wrong: i64, pub missing: i64, pub leftover: i64, pub exempt_cells: i64,
    pub pass: bool,
    pub failed_by_volume: i32, pub pass_by_volume: bool,
    pub pass_wide: bool, pub failed_wide: i32,
    pub bad_by_block: BTreeMap<String, i32>,
}

fn decorative(id: usize) -> bool { t().no_collision[id] && !t().air[id] }
fn decorative_wide(id: usize) -> bool {
    let tb = t();
    if tb.air[id] { return false; }
    if tb.no_collision[id] || tb.leaves[id] { return true; }
    let b = tb.binfo(id as u16);
    matches!(b.kind, Kind::CarpetBlock | Kind::LanternBlock | Kind::FlowerPotBlock | Kind::CandleBlock | Kind::LecternBlock | Kind::RepeaterBlock | Kind::ComparatorBlock
        | Kind::SkullBlock | Kind::WallSkullBlock | Kind::DaylightDetectorBlock | Kind::StandingSignBlock | Kind::WallSignBlock | Kind::CeilingHangingSignBlock
        | Kind::WallHangingSignBlock | Kind::BannerBlock | Kind::WallBannerBlock) || b.name.ends_with("carpet")
}
fn game_master(id: usize) -> bool {
    let b = t().binfo(id as u16);
    matches!(b.kind, Kind::CommandBlock | Kind::GameMasterBlock | Kind::JigsawBlock) || b.name == "test_block"
}

pub fn judge(tg: &Target, world: &[u16]) -> Judged {
    let tb = t();
    let mut r = Judged::default();
    let (nx, ny, nz) = ((tg.sx + 15) >> 4, (tg.sy + 15) >> 4, (tg.sz + 15) >> 4);
    let strip = |s: String| -> String {
        // drop the [properties] of state names
        let mut out = String::new();
        let mut depth = 0;
        for ch in s.chars() { if ch == '[' { depth += 1; } else if ch == ']' { depth -= 1; } else if depth == 0 { out.push(ch); } }
        out
    };
    let mut note = |r: &mut Judged, what: String| {
        let k = strip(what);
        if r.bad_by_block.len() < 400 || r.bad_by_block.contains_key(&k) { *r.bad_by_block.entry(k).or_insert(0) += 1; }
    };
    for cy in 0..ny { for cz in 0..nz { for cx in 0..nx {
        let (mut tgt, mut bad, mut vol, mut tgt_w, mut bad_w) = (0i32, 0i32, 0i32, 0i32, 0i32);
        let (x1, y1, z1) = (tg.sx.min((cx + 1) << 4), tg.sy.min((cy + 1) << 4), tg.sz.min((cz + 1) << 4));
        for y in (cy << 4)..y1 { for z in (cz << 4)..z1 { for x in (cx << 4)..x1 {
            let i = ((y * tg.sz + z) * tg.sx + x) as usize;
            vol += 1;
            let a = tg.ids[i] as usize;
            let mut w = world[i] as usize;
            if tb.air[w] { w = 0; }
            if a == 0 { if w != 0 && !tb.fluid[w] && !tb.technical[w] && !tb.secondary[w] { bad_w += 1; } }
            else if !game_master(a) {
                tgt_w += 1;
                if !decorative_wide(a) && (w == 0 || !(a == w || tb.canon[a] == tb.canon[w])) { bad_w += 1; }
            }
            if a != 0 && decorative(a) { r.exempt_cells += 1; continue; }
            if a == 0 {
                if w != 0 && !tb.fluid[w] && !tb.technical[w] && !tb.secondary[w] { bad += 1; r.leftover += 1; note(&mut r, format!("leftover {}", tb.name(w as u16))); }
                continue;
            }
            tgt += 1;
            if w == 0 { bad += 1; r.missing += 1; note(&mut r, format!("missing {}", tb.name(a as u16))); }
            else if !(a == w || tb.canon[a] == tb.canon[w]) { bad += 1; r.wrong += 1; note(&mut r, format!("wrong {} <- {}", tb.name(a as u16), tb.name(w as u16))); }
        } } }
        r.sections += 1;
        r.target_cells += tgt as i64;
        let ratio = if tgt == 0 { if bad == 0 { 0.0 } else { 1.0 } } else { bad as f64 / tgt as f64 };
        if ratio > 0.03 { r.failed_sections += 1; }
        let ratio_w = if tgt_w == 0 { if bad_w == 0 { 0.0 } else { 1.0 } } else { bad_w as f64 / tgt_w as f64 };
        if ratio_w > 0.03 { r.failed_wide += 1; }
        if bad as f64 / vol as f64 > 0.03 { r.failed_by_volume += 1; }
    } } }
    r.pass = r.failed_sections == 0;
    r.pass_by_volume = r.failed_by_volume == 0;
    r.pass_wide = r.failed_wide == 0;
    r
}

// ------------------------------------------------------------------------------------------------ one build
#[derive(Default)]
pub struct Outcome {
    pub judge: Judged,
    pub status: String,
    pub ticks: i64, pub placed: i64, pub broken: i64, pub skipped: i64, pub trips: i64, pub scaffold_left: i64, pub scaffold_placed: i64, pub replans: i64,
    pub target_blocks: i64,
    pub skip_reasons: String,
    pub wall_ms: f64,
    pub dims: String,
    pub searches: i64, pub expanded: i64, pub unmodelled: u64, pub items_lost: u64, pub predictions: u64, pub stray_screens: i64,
}

fn add_stacks(out: &mut Vec<Stack>, item: u16, mut count: i64) {
    let max = t().items[item as usize].stack as i64;
    while count > 0 { let n = max.min(count); out.push(Stack { item, count: n as u8 }); count -= n; }
}

/// Runs one build. The oracle (with its cached placement tables) is passed in and handed back.
pub fn run(schem: &Schem, tick_limit: i64, verbose: bool, oracle: Oracle) -> (Outcome, Oracle) {
    let t0 = std::time::Instant::now();
    let tb = t();
    let mut out = Outcome::default();
    let target = Target::new(schem);
    out.target_blocks = target.blocks;
    out.dims = format!("{}x{}x{}", schem.sx, schem.sy, schem.sz);
    let (ox, oz) = (64, 64);
    let oy = GROUND_TOP + 1;
    if oy + target.sy + 4 > MAX_Y {
        out.status = "too tall for the world".into();
        out.judge = judge(&target, &vec![0u16; target.ids.len()]);
        return (out, oracle);
    }
    // ---- Site: scaffold material, requirements, chests
    let mut used = vec![false; tb.n];
    for &c in &target.ids { used[c as usize] = true; }
    let cands = ["moss_block", "pale_moss_block", "dried_kelp_block", "hay_block", "dirt", "cobblestone"];
    let mut chosen = cands[0];
    for c in cands {
        let b = &tb.blocks[tb.bid(c) as usize];
        if !(b.first..b.first + b.n).any(|s| used[s as usize]) { chosen = c; break; }
    }
    let scaffold_state = tb.default_of(chosen);
    let scaffold_item = tb.item[scaffold_state as usize];
    let tower_item = *tb.item_by_name.get("scaffolding").unwrap();
    let mut per_state = vec![0i64; tb.n];
    for &c in &target.ids { per_state[c as usize] += 1; }
    let mut need: Vec<(u16, i64)> = Vec::new();
    let merge = |need: &mut Vec<(u16, i64)>, it: u16, n: i64| { if let Some(e) = need.iter_mut().find(|e| e.0 == it) { e.1 += n; } else { need.push((it, n)); } };
    for id in 1..tb.n {
        if per_state[id] == 0 || tb.item_count[id] == 0 { continue; }
        let it = tb.item[id];
        if it == 0 { continue; }
        merge(&mut need, it, per_state[id] * tb.item_count[id] as i64);
        let extra = content_item(id as u16);
        if extra != 0 { merge(&mut need, extra, per_state[id]); }
    }
    let scaffold_count = (64i64 * 54 * 20).min(64 * 27 + target.blocks * 3 + 64 * target.sy as i64 * 2);
    let tower_count = (64i64 * 54 * 20).min(64 * 27 + target.blocks * 3 + 64 * target.sy as i64 * 8);
    let mut stacks: Vec<Stack> = Vec::new();
    add_stacks(&mut stacks, scaffold_item, scaffold_count);
    add_stacks(&mut stacks, tower_item, tower_count);
    for &(it, n) in &need { add_stacks(&mut stacks, it, n + 4.max(n / 20)); }
    let per_chest = 45usize;
    let per_stack = per_chest * STACK_HEIGHT as usize;
    let chest_stacks = 1.max((stacks.len() + per_stack - 1) / per_stack) as i32;
    let depot_cols = 3.max(24.min((target.sx + 1) / 2));
    let depot_rows = (chest_stacks + depot_cols - 1) / depot_cols;
    let depot_depth = ROW_GAP + depot_rows * ROW_PITCH + 2;
    let margin_x = 6.max(depot_cols * 2 - target.sx + 4);
    let top = 8.min(MAX_Y - (oy + target.sy));
    let ground = tb.default_of("grass_block");
    let grid = Grid::new(ox, oy, oz, target.sx, target.sy, target.sz, &target.ids, margin_x, depot_depth, 6, top, ground);
    let pad = 4;
    let mut w = World::new(grid.x0 - pad, grid.y0, grid.z0 - pad, grid.nx + 2 * pad, grid.ny + pad, grid.nz + 2 * pad, GROUND_TOP, MAX_Y);
    let chest_base = with(tb.default_of("chest"), P_FACING, V_SOUTH);
    let mut stock: Vec<Stock> = Vec::new();
    let mut si = 0usize;
    for k in 0..chest_stacks {
        let (row, col) = (k / depot_cols, k % depot_cols);
        let (cx, cz) = (ox + col * 2, oz - ROW_GAP - row * ROW_PITCH);
        let mut h = 0;
        while h < STACK_HEIGHT && si < stacks.len() {
            let cy = oy + h;
            let (west, east) = (p(cx, cy, cz), p(cx + 1, cy, cz));
            w.put_raw(west, with(chest_base, P_TYPE, V_RIGHT));
            w.put_raw(east, with(chest_base, P_TYPE, V_LEFT));
            let mut bw = vec![Stack::default(); 27];
            let mut be = vec![Stack::default(); 27];
            let mut s = 0;
            while s < per_chest && si < stacks.len() {
                let st = stacks[si];
                if s < 27 { bw[s] = st; } else { be[s - 27] = st; }
                stock.push(Stock { item: st.item, count: st.count as i32, chest_x: cx, chest_y: cy, chest_z: cz, slot: s });
                s += 1; si += 1;
            }
            w.containers.insert(west, bw);
            w.containers.insert(east, be);
            h += 1;
        }
    }
    crate::rules::redstone::reset();
    let mut sim = Sim::new(w, ox as f64 + 0.5, oy as f64, (oz - 2) as f64 + 0.5);
    for _ in 0..3 { sim.tick(); }
    sim.changes.clear();
    let mut b = Bot::new(sim, grid, stock, scaffold_item, scaffold_state, oracle);
    b.max_ticks = tick_limit;
    b.verbose = verbose;
    b.run();
    out.status = b.status.clone();
    out.skip_reasons = format!("{{{}}}", b.skip_reasons.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", "));
    out.ticks = b.ticks; out.placed = b.placed; out.broken = b.broken; out.skipped = b.skipped_cells; out.trips = b.trips;
    out.scaffold_left = b.scaffold_left() as i64; out.scaffold_placed = b.scaffold_placed; out.replans = b.replans;
    out.searches = b.nav.searches; out.expanded = b.nav.total_expanded; out.stray_screens = b.stray_screens;
    // (the Java version lets the server run five more ticks before the snapshot)
    b.c.release_keys();
    for _ in 0..5 { b.c.tick(); }
    out.unmodelled = b.c.w.unmodelled; out.items_lost = b.c.items_lost; out.predictions = b.oracle.predictions;
    let mut snap = vec![0u16; target.ids.len()];
    let mut i = 0;
    for y in 0..target.sy { for z in 0..target.sz { for x in 0..target.sx { snap[i] = b.c.w.get(p(ox + x, oy + y, oz + z)); i += 1; } } }
    out.judge = judge(&target, &snap);
    out.wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    (out, b.oracle)
}

impl Outcome {
    /// One line in the format of the Java version's results files.
    pub fn tsv(&self, hash: &str) -> String {
        let mut bad: Vec<(&String, &i32)> = self.judge.bad_by_block.iter().collect();
        bad.sort_by(|a, b| b.1.cmp(a.1));
        let bad_s: String = bad.iter().take(8).map(|(k, v)| format!("{k}={v}; ")).collect();
        [hash.to_string(), if self.judge.pass { "PASS" } else { "FAIL" }.to_string(), self.status.clone(), self.target_blocks.to_string(), self.ticks.to_string(), self.placed.to_string(),
            self.skipped.to_string(), self.judge.missing.to_string(), self.judge.wrong.to_string(), self.judge.leftover.to_string(), self.judge.sections.to_string(),
            self.judge.failed_sections.to_string(), self.judge.exempt_cells.to_string(), self.scaffold_placed.to_string(), self.scaffold_left.to_string(), self.trips.to_string(),
            format!("{:.0}", self.wall_ms), if self.judge.pass_by_volume { "volPASS" } else { "volFAIL" }.to_string(), bad_s, self.skip_reasons.clone(), self.dims.clone(),
            if self.judge.pass_wide { "widePASS" } else { "wideFAIL" }.to_string()].join("\t")
    }
}
