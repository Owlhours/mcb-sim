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
    /// worst bad/target over sections
    pub worst_ratio: f64,
    pub bad_by_block: BTreeMap<String, i32>,
}

pub fn decorative(id: usize) -> bool { t().no_collision[id] && !t().air[id] }
fn decorative_wide(id: usize) -> bool {
    let tb = t();
    if tb.air[id] { return false; }
    if tb.no_collision[id] || tb.leaves[id] { return true; }
    let b = tb.binfo(id as u16);
    matches!(b.kind, Kind::CarpetBlock | Kind::LanternBlock | Kind::FlowerPotBlock | Kind::CandleBlock | Kind::LecternBlock | Kind::RepeaterBlock | Kind::ComparatorBlock
        | Kind::SkullBlock | Kind::WallSkullBlock | Kind::DaylightDetectorBlock | Kind::StandingSignBlock | Kind::WallSignBlock | Kind::CeilingHangingSignBlock
        | Kind::WallHangingSignBlock | Kind::BannerBlock | Kind::WallBannerBlock) || b.name.ends_with("carpet")
}
/// Blocks that only an operator in creative mode can place: command_block, chain_command_block,
/// repeating_command_block, structure_block, jigsaw, test_block, test_instance_block. Exempt from the judge like the
/// decorative blocks (the owner's decision of 2026-11-14): their cells are neither missing nor part of what a
/// section's 3 % is taken of, and whatever stands in them is not looked at.
pub fn game_master(id: usize) -> bool {
    let b = t().binfo(id as u16);
    matches!(b.kind, Kind::CommandBlock | Kind::GameMasterBlock | Kind::JigsawBlock) || b.name == "test_block"
}

/// Cells whose target state cannot exist in survival (the owner's rule of 2026-11-19, `FROM-MONITOR-impossible.md`;
/// stated from the game's rules, not from which builds it gains). In the target as it is judged - fluids dropped,
/// waterlogged states dewatered, the build standing on the site's ground - a cell is impossible when
///   "no item"      its state has no block item, so no player can place it (`item` is none);
///   "no item in survival"  its item cannot be had in survival and nothing in survival makes it in place
///                  (`not_in_survival`: chorus plant, budding amethyst, spawner, bedrock, infested blocks, ...);
///   "live coral"   it is live coral (block, plant, fan, wall fan): live coral dies without water next to it, and the
///                  judged target has none;
///   "cannot stay"  the game's own `canSurvive` fails for it in the target world (`rules::can_survive`: a carpet
///                  over air, a torch on nothing, a cactus beside a solid block, scaffolding 7 or more from support,
///                  a door half without its other half, ...), or it is a gravity block with nothing under it.
///                  This includes what stood on a fluid that the judged target drops (lily pads on water): the
///                  bot has no water to put back, so in the target as judged they cannot stay;
/// applied until nothing changes: a cell whose only support is an impossible cell is impossible too ("cannot stay",
/// with the support taken out). A cell that is only hard to reach is never exempt. Returns (cell index, class).
/// Blocks whose item a survival player cannot get, and which nothing in survival makes in place (creative-only items;
/// a chorus plant grows from a flower but cannot be placed): part of the owner's class "no block item".
fn not_in_survival(name: &str) -> bool {
    matches!(name, "chorus_plant" | "budding_amethyst" | "reinforced_deepslate" | "spawner" | "trial_spawner" | "vault" | "frogspawn"
        | "suspicious_sand" | "suspicious_gravel" | "petrified_oak_slab" | "end_portal_frame" | "bedrock" | "barrier" | "light"
        | "structure_void") || name.starts_with("infested_")
}

pub fn impossible_cells(tg: &Target) -> Vec<(usize, &'static str)> {
    let tb = t();
    let (sx, sy, sz) = (tg.sx, tg.sy, tg.sz);
    let oy = GROUND_TOP + 1;
    let mut w = World::new(0, oy, 0, sx, sy, sz, GROUND_TOP, MAX_Y);
    let pos = |i: usize| -> P { let i = i as i32; p(i % sx, oy + i / (sx * sz), (i / sx) % sz) };
    for (i, &a) in tg.ids.iter().enumerate() { if a != 0 { w.put_raw(pos(i), a); } }
    let mut class: Vec<Option<&'static str>> = vec![None; tg.ids.len()];
    for (i, &a) in tg.ids.iter().enumerate() {
        let a = a as usize;
        if a == 0 { continue; }
        let name = tb.binfo(a as u16).name.as_str();
        if tb.item[a] == 0 { class[i] = Some("no item"); }
        else if not_in_survival(name) { class[i] = Some("no item in survival"); }
        else if name.contains("coral") && !name.starts_with("dead_") { class[i] = Some("live coral"); }
    }
    for (i, c) in class.iter().enumerate() { if c.is_some() { w.put_raw(pos(i), 0); } }
    loop {
        let mut gone = Vec::new();
        for (i, &a) in tg.ids.iter().enumerate() {
            if a == 0 || class[i].is_some() { continue; }
            let q = pos(i);
            let falls = tb.gravity[a as usize] && { let b = w.get(q.below()) as usize; b == 0 || tb.air[b] || tb.replaceable[b] };
            if falls || !crate::rules::can_survive(&w, a, q) { gone.push(i); }
        }
        if gone.is_empty() { break; }
        for i in gone { class[i] = Some("cannot stay"); w.put_raw(pos(i), 0); }
    }
    class.into_iter().enumerate().filter_map(|(i, c)| c.map(|c| (i, c))).filter(|&(i, _)| { let a = tg.ids[i] as usize; !decorative(a) && !game_master(a) }).collect()
}

/// The judge counts the cells of `impossible_cells` as standing right, whatever stands there (part of their section's
/// count, never bad - the owner's decision of 2026-11-20, second form; the first form left them out of the count and
/// could make a section fail). ON by default; `MCSIM_IMPOSSIBLE=0` turns it off, for comparing with rows judged before.
fn impossible_judge() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("MCSIM_IMPOSSIBLE").map(|v| v != "0").unwrap_or(true))
}

/// The judge before 2026-11-14 counted the blocks only an operator can place (`game_master`) like any other block.
/// `MCSIM_OLD_JUDGE=1` brings that back, for comparing with result rows written before the change.
fn old_judge() -> bool { std::env::var_os("MCSIM_OLD_JUDGE").is_some() }

pub fn judge(tg: &Target, world: &[u16]) -> Judged {
    let tb = t();
    let old_judge = old_judge();
    let mut skip_imp = vec![false; if impossible_judge() { tg.ids.len() } else { 0 }];
    if impossible_judge() { for (i, _) in impossible_cells(tg) { skip_imp[i] = true; } }
    let mut r = Judged::default();
    let (nx, ny, nz) = ((tg.sx + 15) >> 4, (tg.sy + 15) >> 4, (tg.sz + 15) >> 4);
    let strip = |s: String| -> String {
        // drop the [properties] of state names
        let mut out = String::new();
        let mut depth = 0;
        for ch in s.chars() { if ch == '[' { depth += 1; } else if ch == ']' { depth -= 1; } else if depth == 0 { out.push(ch); } }
        out
    };
    let note = |r: &mut Judged, what: String| {
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
            if a != 0 && (decorative(a) || (!old_judge && game_master(a))) { r.exempt_cells += 1; continue; }
            // (impossible cells, the owner's decision of 2026-11-20: counted as standing right whatever stands there -
            // part of the section's count, never bad - so the rule can only help a section)
            if a != 0 && !skip_imp.is_empty() && skip_imp[i] { r.exempt_cells += 1; tgt += 1; continue; }
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
        if ratio > r.worst_ratio { r.worst_ratio = ratio; }
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

/// The bad cells of the strict reading, one line each: `missing x y z <target>`, `wrong x y z <target> <- <world>`,
/// `leftover x y z <world>` (a diagnostic; the counts are `judge`'s).
pub fn bad_cells(tg: &Target, world: &[u16]) -> Vec<String> {
    let tb = t();
    let mut out = Vec::new();
    let mut skip_imp = vec![false; if impossible_judge() { tg.ids.len() } else { 0 }];
    if impossible_judge() { for (i, _) in impossible_cells(tg) { skip_imp[i] = true; } }
    for y in 0..tg.sy { for z in 0..tg.sz { for x in 0..tg.sx {
        let i = ((y * tg.sz + z) * tg.sx + x) as usize;
        let a = tg.ids[i] as usize;
        let mut w = world[i] as usize;
        if tb.air[w] { w = 0; }
        if a != 0 && (decorative(a) || (!old_judge() && game_master(a)) || (!skip_imp.is_empty() && skip_imp[i])) { continue; }
        if a == 0 {
            if w != 0 && !tb.fluid[w] && !tb.technical[w] && !tb.secondary[w] { out.push(format!("leftover {x} {y} {z} {}", tb.name(w as u16))); }
        } else if w == 0 { out.push(format!("missing {x} {y} {z} {}", tb.name(a as u16))); }
        else if !(a == w || tb.canon[a] == tb.canon[w]) { out.push(format!("wrong {x} {y} {z} {} <- {}", tb.name(a as u16), tb.name(w as u16))); }
    } } }
    out
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
    /// MCSIM_CAUSES: see `cause_lines`
    pub causes: Vec<String>,
    /// CPU time of the thread for this build (what "per core" speed is measured with; wall time on a shared machine is noise)
    pub cpu_ms: f64,
    pub dims: String,
    pub searches: i64, pub expanded: i64, pub search_ms: f64, pub unmodelled: u64, pub items_lost: u64, pub died: bool, pub rand_draws: u64, pub predictions: u64, pub stray_screens: i64,
    /// the Java version's `metrics` column (from 2d9a193 on): what happened to the bot, apart from the verdict
    pub metrics: String,
    /// counters of candidate behaviour (not in the Java rows): for the run log only
    pub extra: String,
    /// tooling: the health timeline, when `ckpt::enabled()` (not in the row)
    pub timeline: Option<Box<crate::ckpt::Timeline>>,
}

fn add_stacks(out: &mut Vec<Stack>, item: u16, mut count: i64) {
    let max = t().items[item as usize].stack as i64;
    while count > 0 { let n = max.min(count); out.push(Stack { item, count: n as u8 }); count -= n; }
}

#[repr(C)]
struct Timespec { sec: i64, nsec: i64 }
extern "C" { fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32; }
/// CPU time used by this thread so far (CLOCK_THREAD_CPUTIME_ID).
pub fn thread_cpu_ms() -> f64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    unsafe { clock_gettime(3, &mut ts); }
    ts.sec as f64 * 1000.0 + ts.nsec as f64 / 1e6
}

/// Runs one build. The oracle (with its cached placement tables) is passed in and handed back.
pub fn run(schem: &Schem, tick_limit: i64, verbose: bool, oracle: Oracle, events: Option<&str>) -> (Outcome, Oracle) { run_until(schem, tick_limit, 0, verbose, oracle, events) }

/// `stop_at`: if not 0, the tick at which the Java run was cut off by its wall-clock limit (status "wall limit").
pub fn run_until(schem: &Schem, tick_limit: i64, stop_at: i64, verbose: bool, oracle: Oracle, events: Option<&str>) -> (Outcome, Oracle) {
    let t0 = std::time::Instant::now();
    let c0 = thread_cpu_ms();
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
    // (diagnostic MCSIM_COUNT=<block>: how many cells of the target are that block - for drawing lists by content)
    // (MCSIM_COUNT=*: one line `COUNTS SXxSYxSZ kind=n ...` with every block kind of the target, plus the sums _blocks, _nonfull
    //  (not a full cube), _gravity, _nocollision over its states, _fluid, _waterlogged - the input of scripts/blockclasses.py)
    if std::env::var("MCSIM_COUNT").as_deref() == Ok("*") {
        let mut line = format!("COUNTS {}x{}x{}", target.sx, target.sy, target.sz);
        let (mut all, mut nonfull, mut grav, mut nocol) = (0i64, 0i64, 0i64, 0i64);
        for b in &tb.blocks {
            let mut n = 0i64;
            for s in b.first as usize..b.first as usize + b.n as usize {
                let c = per_state[s]; if c == 0 || tb.air[s] { continue; }
                n += c; all += c;
                if !tb.full_cube[s] { nonfull += c; } if tb.gravity[s] { grav += c; } if tb.no_collision[s] { nocol += c; }
            }
            if n > 0 { line += &format!(" {}={}", b.name, n); }
        }
        // (fluids and waterlogging are not part of the target: counted in the schematic)
        let (mut fluid, mut logged) = (0i64, 0i64);
        for &c in &schem.ids { let c = c as usize; if c == 0 || c >= tb.n { continue; } if tb.fluid[c] { fluid += 1; } else if tb.dewater[c] as usize != c { logged += 1; } }
        println!("{line} _blocks={all} _nonfull={nonfull} _gravity={grav} _nocollision={nocol} _fluid={fluid} _waterlogged={logged}");
    } else if let Ok(name) = std::env::var("MCSIM_COUNT") { let b = &tb.blocks[tb.bid(&name) as usize]; let n: i64 = (b.first..b.first + b.n).map(|s| per_state[s as usize]).sum(); println!("COUNT {name} {n}"); }
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
    crate::rules::vibrations::reset();
    // MCB_SEED=k (from 2d9a193 on): the bot starts k blocks further along the front of the build. Nothing else changes,
    // but the whole run takes another course: the way to measure how much of a verdict is the build and how much is chance.
    let seed: i32 = if crate::rev::rev().a1 { std::env::var("MCB_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0) } else { 0 };
    let mut sim = Sim::new(w, ox as f64 + 0.5 + seed.rem_euclid(1.max(target.sx.min(12))) as f64, oy as f64, (oz - 2) as f64 + 0.5);
    // Env.pinSite(ox - marginX0, oz - depotDepth0, ox + sx + marginX0, oz + sz + 6, ox, oz - 2)
    {
        let (min_x, min_z, max_x, max_z, vx, vz) = (ox - margin_x, oz - depot_depth, ox + target.sx + margin_x, oz + target.sz + 6, ox >> 4, (oz - 2) >> 4);
        let (cx0, cz0, cx1, cz1) = ((min_x >> 4) - 1, (min_z >> 4) - 1, (max_x >> 4) + 1, (max_z >> 4) + 1);
        let radius = (vx - cx0).max(cx1 - vx).max((vz - cz0).max(cz1 - vz)) + 1;
        let count = (cx1 - cx0 + 1) as i64 * (cz1 - cz0 + 1) as i64;
        sim.env = crate::sim::EnvState::new(!(radius > 32 || count > 2500));
    }
    for _ in 0..3 { sim.tick(); }
    sim.changes.clear();
    let mut b = Bot::new(sim, grid, stock, scaffold_item, scaffold_state, oracle);
    b.max_ticks = tick_limit;
    b.stop_at = stop_at;
    b.verbose = verbose;
    if let Some(path) = events { b.events = std::fs::File::create(path).ok().map(std::io::BufWriter::new); }
    if crate::ckpt::enabled() { b.timeline = Some(Box::new(crate::ckpt::Timeline::new(&b))); }
    b.run();
    b.sd_dump();
    b.hd_dump();
    out.timeline = b.timeline.take();
    if let Some(mut w) = b.events.take() { use std::io::Write; let _ = w.flush(); }
    out.status = b.status.clone();
    out.metrics = b.metrics();
    out.extra = format!("mended={} orphans={} chained={} fromClose={} helpedClose={}", b.mended, b.orphans, b.chained, b.from_close, b.helped_close);
    out.skip_reasons = format!("{{{}}}", b.skip_reasons.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", "));
    out.ticks = b.ticks; out.placed = b.placed; out.broken = b.broken; out.skipped = b.skipped_cells; out.trips = b.trips;
    out.scaffold_left = b.scaffold_left() as i64; out.scaffold_placed = b.scaffold_placed; out.replans = b.replans;
    if crate::bot::dry::stat_on() { crate::bot::dry::dump(&b); }
    out.searches = b.nav.searches; out.search_ms = b.nav.search_ns as f64 / 1e6; out.expanded = b.nav.total_expanded; out.stray_screens = b.stray_screens;
    // (the Java version lets the server run five more ticks before the snapshot)
    b.c.release_keys();
    for _ in 0..5 { b.c.tick(); }
    // sensors on a site too big to pin: whether their chunks tick (VibrationSystem.Ticker, the block entity
    // tickers) depends on where the player is, which is not modelled
    if !b.c.env.pinned && crate::rules::vibrations::made() > 0 { b.c.w.unmodelled += 1; }
    // item entities keep the Java world from idling (Env.worldIdle), which moves the game time the sensors see
    if b.c.items_lost > 0 && crate::rules::vibrations::made() > 0 { b.c.w.unmodelled += 1; }
    out.unmodelled = b.c.w.unmodelled; out.items_lost = b.c.items_lost; out.died = b.c.w.player_died; out.rand_draws = b.c.w.rand_draws; out.predictions = b.oracle.predictions;
    let mut snap = vec![0u16; target.ids.len()];
    let mut i = 0;
    for y in 0..target.sy { for z in 0..target.sz { for x in 0..target.sx { snap[i] = b.c.w.get(p(ox + x, oy + y, oz + z)); i += 1; } } }
    out.judge = judge(&target, &snap);
    // MCSIM_BAD=<file>: every bad cell of the strict reading, one per line (build coordinates), as the Java run's "bad:" lines
    if let Ok(path) = std::env::var("MCSIM_BAD") { let _ = std::fs::write(path, bad_cells(&target, &snap).join("\n") + "\n"); }
    // MCSIM_CAUSES=<dir>: <dir>/<hash>.causes, the bad cells of the failed sections with what left each of them bad
    if let Ok(dir) = std::env::var("MCSIM_CAUSES") {
        let _ = std::fs::create_dir_all(&dir);
        out.causes = cause_lines(&target, &snap, &b, ox, oy, oz);
    }
    out.wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    out.cpu_ms = thread_cpu_ms() - c0;
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
            if self.judge.pass_wide { "widePASS" } else { "wideFAIL" }.to_string()].into_iter()
            // (from 2d9a193 on the Java rows carry the worst section ratio and the metrics)
            .chain(if crate::rev::rev().a1 { vec![fmt3(self.judge.worst_ratio), self.metrics.clone()] } else { vec![] })
            .chain([format!("{:.0}", self.cpu_ms)]).collect::<Vec<_>>().join("\t")
    }
}

/// Java `String.format("%.3f", x)`: `Formatter` takes the shortest decimal form of the double (the digits of
/// `Double.toString`) and rounds that half up at the third decimal - so 0.0075 (stored as 0.00749999...) prints as
/// 0.008, where rounding the exact binary value would give 0.007.
pub fn fmt3(x: f64) -> String {
    let s = format!("{}", x);   // (shortest digits that read back as x, plain notation)
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let mut digits: Vec<u8> = frac.bytes().collect();
    digits.resize(digits.len().max(4), b'0');
    let mut n: u64 = int.parse::<u64>().unwrap_or(0) * 1000 + digits[..3].iter().fold(0, |a, &d| a * 10 + (d - b'0') as u64);
    if digits[3] >= b'5' { n += 1; }
    format!("{}.{:03}", n / 1000, n % 1000)
}

#[cfg(test)]
mod tests {
    #[test]
    fn fmt3_rounds_like_java() {
        for (x, want) in [(0.0, "0.000"), (1.0, "1.000"), (0.0625, "0.063"), (0.0075, "0.008"), (0.075, "0.075"), (3.0 / 40.0, "0.075"), (3.0 / 400.0, "0.008"), (0.0224, "0.022"), (1.0 / 3.0, "0.333"), (2.0 / 3.0, "0.667"), (0.9996, "1.000")] {
            assert_eq!(super::fmt3(x), want, "{x}");
        }
    }
}

/// MCSIM_CAUSES: one line per failed section (`S cx cy cz target bad`) followed by one per bad cell in it
/// (`C kind x y z | cause | block`). The cause is the bot's own last word on the cell: why it was skipped, why it was
/// last put off, or why the clean-up gave the scaffolding piece up. Sections and bad cells are the judge's.
pub fn cause_lines(tg: &Target, world: &[u16], b: &Bot, ox: i32, oy: i32, oz: i32) -> Vec<String> {
    let tb = t();
    let Some(cs) = b.causes.as_ref() else { return Vec::new() };
    let mut out = vec![format!("B status={} ticks={} max={} scaffold_left={} time_cut={} build_ticks={} at={} on_ground={} placed={} scaffold_placed={} rope_undone={} trips={}", b.status, b.ticks, b.max_ticks, b.scaffold.len(), cs.time_cut, b.build_ticks, b.rel(b.cx(), b.cy(), b.cz()), b.c.on_ground(), b.placed, b.scaffold_placed, b.rope_undone, b.trips)];
    let ended = b.status != "done";
    let (nx, ny, nz) = ((tg.sx + 15) >> 4, (tg.sy + 15) >> 4, (tg.sz + 15) >> 4);
    out.extend(crate::bot::nav::act_lines());
    // (diagnostic, MCSIM_WHERE) where each leftover piece stands: the air it is in, as the bot knows the world -
    // `open` joined to the outside, `room=N` a closed space of N cells (`+bot` if the bot is in it too), and
    // `behind` if only other scaffolding lies between it and the outside
    let labels: Option<(Vec<i32>, Vec<i32>, Vec<i32>, Vec<i32>)> = if std::env::var_os("MCSIM_WHERE").is_some() {
        let g = &b.g;
        let n = (g.nx * g.ny * g.nz) as usize;
        let lab = |scaf_open: bool| -> (Vec<i32>, Vec<i32>) {
            let mut l = vec![-1i32; n];
            let mut sizes: Vec<i32> = Vec::new();
            let free = |i: usize| -> bool { let sc = b.scaffold.contains(i as i32); if sc { scaf_open } else { tb.walk_through[g.cur[i] as usize] } };
            let mut stack: Vec<usize> = Vec::new();
            for s in 0..n {
                if l[s] >= 0 || !free(s) { continue; }
                let id = sizes.len() as i32;
                let mut cnt = 0;
                l[s] = id; stack.push(s);
                while let Some(i) = stack.pop() {
                    cnt += 1;
                    let (x, y, z) = (g.x_of(i), g.y_of(i), g.z_of(i));
                    for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
                        if !g.inside(x + dx, y + dy, z + dz) { continue; }
                        let j = g.idx(x + dx, y + dy, z + dz);
                        if l[j] < 0 && free(j) { l[j] = id; stack.push(j); }
                    }
                }
                sizes.push(cnt);
            }
            (l, sizes)
        };
        let (l1, s1) = lab(true);
        let (l2, s2) = lab(false);
        Some((l1, s1, l2, s2))
    } else { None };
    let where_of = |gi: i32| -> String {
        let Some((l1, s1, l2, s2)) = labels.as_ref() else { return String::new() };
        if gi < 0 { return String::new(); }
        let g = &b.g;
        let big = |s: &Vec<i32>| -> i32 { (0..s.len()).max_by_key(|&k| s[k]).map(|k| k as i32).unwrap_or(-1) };
        let (out1, out2) = (big(s1), big(s2));
        let bi = g.idx(b.cx(), b.cy(), b.cz());
        let c1 = l1[gi as usize];
        let (x, y, z) = (g.x_of(gi as usize), g.y_of(gi as usize), g.z_of(gi as usize));
        let under = if !g.inside(x, y - 1, z) { "edge" } else { let j = g.idx(x, y - 1, z); if b.scaffold.contains(j as i32) { "scaffold" } else if g.cur[j] == 0 { "air" } else { "block" } };
        let botw = if l1[bi] == out1 { "bot-outside" } else { "bot-shut" };
        if c1 != out1 { return format!(" | where=room={}{} under={under} {botw}", s1[c1 as usize], if l1[bi] == c1 { "+bot" } else { "" }); }
        let mut touch = false;
        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
            if !g.inside(x + dx, y + dy, z + dz) { continue; }
            if l2[g.idx(x + dx, y + dy, z + dz)] == out2 { touch = true; }
        }
        format!(" | where={} under={under} {botw}", if touch { "open" } else { "behind" })
    };
    let mut skip_imp = vec![false; if impossible_judge() { tg.ids.len() } else { 0 }];
    if impossible_judge() { for (i, _) in impossible_cells(tg) { skip_imp[i] = true; } }
    for cy in 0..ny { for cz in 0..nz { for cx in 0..nx {
        let (x1, y1, z1) = (tg.sx.min((cx + 1) << 4), tg.sy.min((cy + 1) << 4), tg.sz.min((cz + 1) << 4));
        let (mut tgt, mut bad) = (0, 0);
        let mut lines = Vec::new();
        for y in (cy << 4)..y1 { for z in (cz << 4)..z1 { for x in (cx << 4)..x1 {
            let i = ((y * tg.sz + z) * tg.sx + x) as usize;
            let a = tg.ids[i] as usize;
            let mut w = world[i] as usize;
            if tb.air[w] { w = 0; }
            if a != 0 && (decorative(a) || (!old_judge() && game_master(a))) { continue; }
            if a != 0 && !skip_imp.is_empty() && skip_imp[i] { tgt += 1; continue; }
            let gi = if b.g.inside(ox + x, oy + y, oz + z) { b.g.idx(ox + x, oy + y, oz + z) as i32 } else { -1 };
            if a == 0 {
                if w != 0 && !tb.fluid[w] && !tb.technical[w] && !tb.secondary[w] {
                    bad += 1;
                    let cause = if gi >= 0 && b.scaffold.contains(gi) {
                        // (the piece itself, or the nearest piece below / above it in its column that the clean-up tried)
                        let layer = b.g.nx * b.g.nz;
                        let mut why = cs.scaf.get(&gi).copied();
                        let mut k = 1;
                        while why.is_none() && k < 64 { why = cs.scaf.get(&(gi - k * layer)).or_else(|| cs.scaf.get(&(gi + k * layer))).copied(); k += 1; }
                        match why { Some(wh) => wh.to_string(), None => if ended { format!("clean-up: {} before its turn", b.status) } else if b.time_short() { "clean-up: time short".to_string() } else if !cs.cleanup_end.is_empty() { format!("{}, piece not tried", cs.cleanup_end) } else { "clean-up: piece not tried".to_string() } }
                    } else { "left standing, not known as scaffolding".to_string() };
                    let t1 = { let layer = b.g.nx * b.g.nz; let mut tt = cs.first.get(&gi).copied(); let mut k = 1; while tt.is_none() && k < 64 && gi >= 0 { tt = cs.first.get(&(gi - k * layer)).or_else(|| cs.first.get(&(gi + k * layer))).copied(); k += 1; } tt.unwrap_or(-1) };
                    let put = cs.put.get(&gi).copied().unwrap_or(-1);
                    let lastf = cs.last.get(&gi).copied().unwrap_or(-1);
                    lines.push(format!("C leftover {x} {y} {z} | {cause} | {} | t={t1} | put={put} last={lastf}{}", tb.name(w as u16), where_of(gi)));
                }
                continue;
            }
            tgt += 1;
            let missing = w == 0;
            if !missing && (a == w || tb.canon[a] == tb.canon[w]) { continue; }
            bad += 1;
            let was_placed = cs.placed.contains(&gi);
            let last = cs.defer.get(&gi).copied();
            let cause = if let Some(sk) = cs.skip.get(&gi) {
                if sk == "no way to place" { format!("no way to place: {}", last.unwrap_or("never tried")) } else { sk.split(':').next().unwrap_or("").to_string() }
            } else if !missing && gi >= 0 && b.scaffold.contains(gi) { "scaffolding stands in the cell".to_string()
            } else if was_placed { if missing { "placed, gone again".to_string() } else { "placed, stands wrong".to_string() }
            } else if ended { format!("{}: {}", b.status, last.unwrap_or("not reached"))
            } else if cs.time_cut && last.is_none() { "time ran short before its turn".to_string()
            } else if cs.time_cut { format!("time ran short; put off: {}", last.unwrap())
            } else if let Some(l) = last { format!("left put off: {l}") } else if missing { "never tried".to_string() } else { "something else stands there".to_string() };
            lines.push(format!("C {} {x} {y} {z} | {cause} | {} | t={}", if missing { "missing" } else { "wrong" }, tb.name(a as u16), cs.first.get(&gi).copied().unwrap_or(-1)));
        } } }
        let ratio = if tgt == 0 { if bad == 0 { 0.0 } else { 1.0 } } else { bad as f64 / tgt as f64 };
        if ratio > 0.03 { out.push(format!("S {cx} {cy} {cz} {tgt} {bad}")); out.extend(lines); }
    } } }
    out
}
