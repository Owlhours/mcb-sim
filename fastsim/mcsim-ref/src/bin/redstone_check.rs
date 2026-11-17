//! Hand-derived scenarios for `rules/redstone.rs`. Each builds a small world, plays game ticks and compares with
//! what the vanilla source says must happen. Prints PASS/FAIL per scenario with the tick counts seen.
use mcsim::geom::*;
use mcsim::props::*;
use mcsim::rules::redstone as rs;
use mcsim::rules::{self, Clicker, PlaceCtx, Use};
use mcsim::tables::*;
use mcsim::world::*;

fn new_world() -> World {
    rs::reset();
    World::new(0, 0, 0, 24, 16, 24, 0, 319)
}

/// One game tick: scheduled ticks, block events, block entities.
fn step(w: &mut World) {
    w.game_time += 1;
    w.run_scheduled();
    w.run_block_events();
    rs::tick_block_entities(w);
}
fn steps(w: &mut World, n: usize) { for _ in 0..n { step(w); } }

fn st(name: &str, props: &[(u8, u8)]) -> S {
    let mut s = t().default_of(name);
    for (k, v) in props { s = with(s, *k, *v); }
    s
}
fn ctx(pos: P) -> PlaceCtx {
    PlaceCtx { item: 0, hit_pos: pos.below(), face: UP, hit: [pos.x as f64 + 0.5, pos.y as f64, pos.z as f64 + 0.5], inside: false, yaw: 0.0, pitch: 0.0, sneak: false, player_pos: [0.0, 1.0, 0.0], replace_clicked: false }
}
/// What BlockItem.place does with the state once it is chosen: the block write and setPlacedBy.
fn place(w: &mut World, pos: P, s: S) {
    w.set_block(pos, s, F_ALL);
    let cur = w.get(pos);
    if same_block(cur, s) { rules::set_placed_by(w, pos, cur, &ctx(pos)); }
}
fn place_wire(w: &mut World, pos: P) {
    let cross = st("redstone_wire", &[(P_NORTH, V_SIDE), (P_EAST, V_SIDE), (P_SOUTH, V_SIDE), (P_WEST, V_SIDE), (P_POWER, 0)]);
    let s = rs::wire_connection_state(w, cross, pos);
    place(w, pos, s);
}
fn click(w: &mut World, pos: P) -> Use {
    let mut who = Clicker { pos: [0.0, 1.0, 0.0], yaw: 0.0, pitch: 0.0, sneak: false, item: 0, consumed: false };
    let s = w.get(pos);
    rules::use_without_item(w, s, pos, UP, [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5], &mut who)
}
fn name(w: &World, pos: P) -> String { t().name(w.get(pos)) }
fn is(w: &World, pos: P, block: &str) -> bool { t().binfo(w.get(pos)).name == block }
fn pw(w: &World, pos: P) -> i32 { get(w.get(pos), P_POWER).map(|v| v as i32).unwrap_or(-1) }
fn lit(w: &World, pos: P) -> bool { is_true(w.get(pos), P_LIT) }
fn floor_lever() -> S { st("lever", &[(P_FACE, V_FLOOR), (P_FACING, V_NORTH), (P_POWERED, V_FALSE)]) }
fn lamp() -> S { st("redstone_lamp", &[(P_LIT, V_FALSE)]) }
fn stone() -> S { t().default_of("stone") }
fn rblock() -> S { t().default_of("redstone_block") }
fn piston(name: &str, f: u8) -> S { st(name, &[(P_FACING, f), (P_EXTENDED, V_FALSE)]) }

struct Report {
    pass: usize,
    fail: usize,
}
impl Report {
    fn check(&mut self, scenario: &str, ok: bool, detail: String) {
        if ok { self.pass += 1; } else { self.fail += 1; }
        println!("{} {:<34} {}", if ok { "PASS" } else { "FAIL" }, scenario, detail);
    }
}

/// Plays ticks until `f` holds; the number of ticks played (None if it never did within `max`).
fn until(w: &mut World, max: usize, f: impl Fn(&World) -> bool) -> Option<usize> {
    for i in 0..=max {
        if f(w) { return Some(i); }
        if i < max { step(w); }
    }
    None
}

fn main() {
    init("/a/reference", "/a/data");
    let mut r = Report { pass: 0, fail: 0 };

    // ---------------------------------------------------------------- 1 lever + dust line, lamps
    {
        let mut w = new_world();
        // line A: 16 dust (x=2..17) and a lamp behind it; line B: 15 dust (x=2..16) and a lamp
        place(&mut w, p(1, 1, 5), floor_lever());
        for x in 2..=17 { place_wire(&mut w, p(x, 1, 5)); }
        place(&mut w, p(18, 1, 5), lamp());
        place(&mut w, p(1, 1, 9), floor_lever());
        for x in 2..=16 { place_wire(&mut w, p(x, 1, 9)); }
        place(&mut w, p(17, 1, 9), lamp());
        click(&mut w, p(1, 1, 5));
        click(&mut w, p(1, 1, 9));
        let powers: Vec<i32> = (2..=17).map(|x| pw(&w, p(x, 1, 5))).collect();
        let want: Vec<i32> = (0..16).map(|i| 15 - i).collect();
        r.check("dust line 16 long", powers == want && !lit(&w, p(18, 1, 5)), format!("powers {:?}, lamp behind the 16th dust lit={}", powers, lit(&w, p(18, 1, 5))));
        let powers_b: Vec<i32> = (2..=16).map(|x| pw(&w, p(x, 1, 9))).collect();
        r.check("dust line 15 long lights lamp", powers_b[14] == 1 && lit(&w, p(17, 1, 9)), format!("last dust {}, lamp lit={} (at once)", powers_b[14], lit(&w, p(17, 1, 9))));
        // lever off: dust dies at once, the lamp 4 ticks later
        click(&mut w, p(1, 1, 9));
        let dead = (2..=16).all(|x| pw(&w, p(x, 1, 9)) == 0);
        let still = lit(&w, p(17, 1, 9));
        let n = until(&mut w, 10, |w| !lit(w, p(17, 1, 9)));
        r.check("lamp turns off after 4 ticks", dead && still && n == Some(4), format!("dust dead at once={}, lamp off after {:?} ticks", dead, n));
    }

    // ---------------------------------------------------------------- 2 dust shapes
    {
        let mut w = new_world();
        place_wire(&mut w, p(5, 1, 3));
        let lone = name(&w, p(5, 1, 3));
        let ok_cross = lone == "redstone_wire[east=side,north=side,power=0,south=side,west=side]";
        let used = click(&mut w, p(5, 1, 3));
        let dot = name(&w, p(5, 1, 3));
        let ok_dot = dot == "redstone_wire[east=none,north=none,power=0,south=none,west=none]" && used == Use::Consume;
        click(&mut w, p(5, 1, 3));
        let back = name(&w, p(5, 1, 3)) == lone;
        r.check("lone dust: cross, click: dot", ok_cross && ok_dot && back, format!("{} -> {} -> back to cross={}", lone, dot, back));
        // two in a row: a straight line
        place_wire(&mut w, p(5, 1, 7));
        place_wire(&mut w, p(6, 1, 7));
        let a = name(&w, p(5, 1, 7));
        let b = name(&w, p(6, 1, 7));
        let line = "redstone_wire[east=side,north=none,power=0,south=none,west=side]";
        r.check("two dust: straight line", a == line && b == line, format!("{} | {}", a, b));
        // a corner
        place_wire(&mut w, p(6, 1, 8));
        let c1 = name(&w, p(6, 1, 7));
        let c2 = name(&w, p(6, 1, 8));
        r.check("three dust: corner", c1 == "redstone_wire[east=none,north=none,power=0,south=side,west=side]" && c2 == "redstone_wire[east=none,north=side,power=0,south=side,west=none]", format!("{} | {}", c1, c2));
        // up a block: the lower dust climbs, the upper one comes down as a side
        place_wire(&mut w, p(5, 1, 12));
        place(&mut w, p(6, 1, 12), stone());
        place_wire(&mut w, p(6, 2, 12));
        let lo = name(&w, p(5, 1, 12));
        let hi = name(&w, p(6, 2, 12));
        let ok = lo == "redstone_wire[east=up,north=none,power=0,south=none,west=side]" && hi == "redstone_wire[east=side,north=none,power=0,south=none,west=side]";
        // a block over the lower dust cuts the climb
        place(&mut w, p(5, 2, 12), stone());
        let cut = name(&w, p(5, 1, 12));
        let hi2 = name(&w, p(6, 2, 12));
        r.check("dust up a block, cut by a block", ok && cut == "redstone_wire[east=side,north=side,power=0,south=side,west=side]" && hi2 == "redstone_wire[east=side,north=side,power=0,south=side,west=side]", format!("{} | {} ; cut: {} | {}", lo, hi, cut, hi2));
    }

    // ---------------------------------------------------------------- 3 torches
    {
        let mut w = new_world();
        // stone with a lever on its west side, a torch on top and a wall torch on its east side
        place(&mut w, p(5, 1, 5), stone());
        place(&mut w, p(4, 1, 5), st("lever", &[(P_FACE, V_WALL), (P_FACING, V_WEST), (P_POWERED, V_FALSE)]));
        place(&mut w, p(5, 2, 5), st("redstone_torch", &[(P_LIT, V_TRUE)]));
        place(&mut w, p(7, 1, 5), lamp());
        place(&mut w, p(6, 1, 5), st("redstone_wall_torch", &[(P_FACING, V_EAST), (P_LIT, V_TRUE)]));
        steps(&mut w, 3);
        let before = lit(&w, p(5, 2, 5)) && lit(&w, p(6, 1, 5)) && lit(&w, p(7, 1, 5));
        click(&mut w, p(4, 1, 5));
        let n_off = until(&mut w, 10, |w| !lit(w, p(5, 2, 5)) && !lit(w, p(6, 1, 5)));
        click(&mut w, p(4, 1, 5));
        let n_on = until(&mut w, 10, |w| lit(w, p(5, 2, 5)) && lit(w, p(6, 1, 5)));
        r.check("torch inversion (top and side)", before && n_off == Some(2) && n_on == Some(2), format!("lit before={}, off after {:?} ticks, on again after {:?}", before, n_off, n_on));
        // burnout: wall torch -> block above it -> dust on the block the torch hangs on
        let mut w = new_world();
        place(&mut w, p(5, 1, 5), stone());
        place(&mut w, p(6, 2, 5), stone());
        place_wire(&mut w, p(5, 2, 5));
        place(&mut w, p(6, 1, 5), st("redstone_wall_torch", &[(P_FACING, V_EAST), (P_LIT, V_TRUE)]));
        let mut offs = 0;
        let mut prev = true;
        let mut last_off = 0;
        let mut relit_at = None;
        for i in 1..=260 {
            step(&mut w);
            let l = lit(&w, p(6, 1, 5));
            if prev && !l { offs += 1; last_off = i; }
            if !prev && l && offs >= 8 && relit_at.is_none() { relit_at = Some(i); }
            prev = l;
        }
        // (the torch's own feedback had already asked for a tick 2 later when the 160-tick restart is scheduled, so
        // that one is dropped as a duplicate: the torch stays dark until something updates it)
        place(&mut w, p(7, 1, 5), stone());
        let n_relit = until(&mut w, 10, |w| lit(w, p(6, 1, 5)));
        r.check("torch burnout after 8 toggles", offs == 8 && last_off == 30 && relit_at.is_none() && n_relit == Some(2),
            format!("turned off {} times, the 8th at tick {}, dark until tick 260 (relit: {:?}), relit {:?} ticks after a block update", offs, last_off, relit_at, n_relit));
    }

    // ---------------------------------------------------------------- 4 repeater delay and lock
    {
        let mut w = new_world();
        place(&mut w, p(2, 1, 8), floor_lever());
        place(&mut w, p(3, 1, 8), st("repeater", &[(P_FACING, V_WEST), (P_DELAY, 2), (P_LOCKED, V_FALSE), (P_POWERED, V_FALSE)]));
        place(&mut w, p(4, 1, 8), lamp());
        click(&mut w, p(2, 1, 8));
        let n = until(&mut w, 12, |w| lit(w, p(4, 1, 8)));
        r.check("repeater delay 2 = 4 ticks", n == Some(4), format!("lamp lit {:?} ticks after the lever", n));
        click(&mut w, p(2, 1, 8));
        let n_off = until(&mut w, 12, |w| !is_true(w.get(p(3, 1, 8)), P_POWERED));
        steps(&mut w, 6);
        // side repeater (from the north) locks the main one
        place(&mut w, p(3, 1, 6), floor_lever());
        place(&mut w, p(3, 1, 7), st("repeater", &[(P_FACING, V_NORTH), (P_DELAY, 1), (P_LOCKED, V_FALSE), (P_POWERED, V_FALSE)]));
        click(&mut w, p(3, 1, 6));
        let n_lock = until(&mut w, 12, |w| is_true(w.get(p(3, 1, 8)), P_LOCKED));
        click(&mut w, p(2, 1, 8));
        steps(&mut w, 10);
        let held = !is_true(w.get(p(3, 1, 8)), P_POWERED) && !lit(&w, p(4, 1, 8));
        click(&mut w, p(3, 1, 6));
        let n_unlock = until(&mut w, 12, |w| !is_true(w.get(p(3, 1, 8)), P_LOCKED));
        let n_on = until(&mut w, 12, |w| lit(w, p(4, 1, 8)));
        r.check("repeater locked by side repeater", n_off == Some(4) && n_lock == Some(2) && held && n_unlock == Some(2) && n_on == Some(4),
            format!("off after {:?}, locked after {:?} ticks, held off while locked={}, unlocked after {:?}, then on after {:?}", n_off, n_lock, held, n_unlock, n_on));
    }

    // ---------------------------------------------------------------- 5 comparator
    {
        let mut w = new_world();
        // input from the west: lever, 3 dust (15, 14, 13); side input from the south: dust with power 5
        place(&mut w, p(1, 1, 5), floor_lever());
        for x in 2..=4 { place_wire(&mut w, p(x, 1, 5)); }
        place(&mut w, p(5, 1, 5), st("comparator", &[(P_FACING, V_WEST), (P_MODE, V_COMPARE), (P_POWERED, V_FALSE)]));
        place_wire(&mut w, p(6, 1, 5));
        place_wire(&mut w, p(7, 1, 5));
        place(&mut w, p(5, 1, 17), floor_lever());
        for z in (6..=16).rev() { place_wire(&mut w, p(5, 1, z)); }
        click(&mut w, p(5, 1, 17));
        let side = pw(&w, p(5, 1, 6));
        click(&mut w, p(1, 1, 5));
        let input = pw(&w, p(4, 1, 5));
        let n = until(&mut w, 8, |w| pw(w, p(6, 1, 5)) > 0);
        let cmp = (pw(&w, p(6, 1, 5)), pw(&w, p(7, 1, 5)));
        click(&mut w, p(5, 1, 5));
        steps(&mut w, 4);
        let sub = (pw(&w, p(6, 1, 5)), pw(&w, p(7, 1, 5)));
        let mode = name(&w, p(5, 1, 5));
        r.check("comparator compare / subtract", side == 5 && input == 13 && n == Some(2) && cmp == (13, 12) && sub == (8, 7),
            format!("input {}, side {}, output after {:?} ticks: compare {:?}, subtract {:?} ({})", input, side, n, cmp, sub, mode));
        // side stronger than the input: off
        let mut w = new_world();
        place(&mut w, p(4, 1, 5), rblock());
        place(&mut w, p(5, 1, 5), st("comparator", &[(P_FACING, V_WEST), (P_MODE, V_COMPARE), (P_POWERED, V_FALSE)]));
        place_wire(&mut w, p(6, 1, 5));
        steps(&mut w, 3);
        let on = pw(&w, p(6, 1, 5));
        // reading a composter through a solid block
        let mut w2 = new_world();
        place(&mut w2, p(3, 1, 5), st("composter", &[(P_LEVEL, 6)]));
        place(&mut w2, p(4, 1, 5), stone());
        place(&mut w2, p(5, 1, 5), st("comparator", &[(P_FACING, V_WEST), (P_MODE, V_COMPARE), (P_POWERED, V_FALSE)]));
        place_wire(&mut w2, p(6, 1, 5));
        steps(&mut w2, 3);
        let through = pw(&w2, p(6, 1, 5));
        r.check("comparator sources", on == 15 && through == 6, format!("redstone block in: {}, composter level 6 through stone: {}", on, through));
    }

    // ---------------------------------------------------------------- 6 pistons
    {
        // 12 blocks move, 13 do not
        let mut w = new_world();
        place(&mut w, p(2, 1, 5), piston("piston", V_EAST));
        for x in 3..=14 { place(&mut w, p(x, 1, 5), stone()); }
        place(&mut w, p(2, 1, 9), piston("piston", V_EAST));
        for x in 3..=15 { place(&mut w, p(x, 1, 9), stone()); }
        place(&mut w, p(2, 1, 4), rblock());
        place(&mut w, p(2, 1, 8), rblock());
        let n = until(&mut w, 10, |w| is(w, p(15, 1, 5), "stone"));
        let moved = is(&w, p(3, 1, 5), "piston_head") && (4..=15).all(|x| is(&w, p(x, 1, 5), "stone")) && is_true(w.get(p(2, 1, 5)), P_EXTENDED);
        let stuck = !is_true(w.get(p(2, 1, 9)), P_EXTENDED) && (3..=15).all(|x| is(&w, p(x, 1, 9), "stone")) && is(&w, p(16, 1, 9), "air");
        r.check("piston pushes 12, not 13", n == Some(3) && moved && stuck, format!("12 blocks arrived after {:?} ticks, row intact={}, 13-row unmoved={}", n, moved, stuck));

        // sticky piston pulls its block back
        let mut w = new_world();
        place(&mut w, p(2, 1, 5), piston("sticky_piston", V_EAST));
        place(&mut w, p(3, 1, 5), stone());
        place(&mut w, p(2, 1, 4), rblock());
        let n_out = until(&mut w, 10, |w| is(w, p(4, 1, 5), "stone"));
        let head = name(&w, p(3, 1, 5));
        w.set_block(p(2, 1, 4), 0, F_ALL);
        let n_back = until(&mut w, 10, |w| is(w, p(3, 1, 5), "stone") && is(w, p(2, 1, 5), "sticky_piston"));
        let clean = is(&w, p(4, 1, 5), "air") && !is_true(w.get(p(2, 1, 5)), P_EXTENDED);
        r.check("sticky piston pulls back", n_out == Some(3) && head == "piston_head[facing=east,short=false,type=sticky]" && n_back == Some(3) && clean,
            format!("out after {:?} ticks ({}), back after {:?}, clean={}", n_out, head, n_back, clean));

        // torch in front is destroyed, obsidian blocks
        let mut w = new_world();
        place(&mut w, p(2, 1, 5), piston("piston", V_EAST));
        place(&mut w, p(3, 1, 5), t().default_of("torch"));
        place(&mut w, p(2, 1, 4), rblock());
        place(&mut w, p(2, 1, 9), piston("piston", V_EAST));
        place(&mut w, p(3, 1, 9), t().default_of("obsidian"));
        place(&mut w, p(2, 1, 8), rblock());
        // a stone with a torch behind it: the stone moves, the torch pops
        place(&mut w, p(2, 1, 13), piston("piston", V_EAST));
        place(&mut w, p(3, 1, 13), stone());
        place(&mut w, p(4, 1, 13), t().default_of("torch"));
        place(&mut w, p(2, 1, 12), rblock());
        steps(&mut w, 4);
        let torch_gone = is(&w, p(3, 1, 5), "piston_head") && is(&w, p(4, 1, 5), "air") && is_true(w.get(p(2, 1, 5)), P_EXTENDED);
        let blocked = !is_true(w.get(p(2, 1, 9)), P_EXTENDED) && is(&w, p(3, 1, 9), "obsidian");
        let popped = is(&w, p(4, 1, 13), "stone") && is(&w, p(5, 1, 13), "air");
        r.check("piston: torch destroyed, obsidian", torch_gone && blocked && popped, format!("torch destroyed={}, obsidian holds={}, torch behind a pushed block destroyed={}", torch_gone, blocked, popped));

        // slime block drags its neighbours (in the air: on the ground it would stick to the ground too)
        let mut w = new_world();
        place(&mut w, p(2, 3, 5), piston("sticky_piston", V_EAST));
        place(&mut w, p(3, 3, 5), t().default_of("slime_block"));
        place(&mut w, p(3, 3, 6), stone());
        place(&mut w, p(3, 4, 5), stone());
        place(&mut w, p(2, 3, 4), rblock());
        let n_out = until(&mut w, 10, |w| is(w, p(4, 3, 5), "slime_block"));
        let out = is(&w, p(4, 3, 6), "stone") && is(&w, p(4, 4, 5), "stone") && is(&w, p(3, 3, 6), "air") && is(&w, p(3, 4, 5), "air");
        w.set_block(p(2, 3, 4), 0, F_ALL);
        let n_back = until(&mut w, 10, |w| is(w, p(3, 3, 5), "slime_block"));
        let back = is(&w, p(3, 3, 6), "stone") && is(&w, p(3, 4, 5), "stone") && is(&w, p(4, 3, 6), "air") && is(&w, p(4, 4, 5), "air");
        // on the ground the slime block holds on to the ground row: more than 12 blocks, nothing moves
        let mut w = new_world();
        place(&mut w, p(2, 1, 5), piston("piston", V_EAST));
        place(&mut w, p(3, 1, 5), t().default_of("slime_block"));
        place(&mut w, p(2, 1, 4), rblock());
        steps(&mut w, 4);
        let grounded = !is_true(w.get(p(2, 1, 5)), P_EXTENDED);
        r.check("slime block drags neighbours", n_out == Some(3) && out && n_back == Some(3) && back && grounded,
            format!("out after {:?} with both stones={}, back after {:?} with both={}, on the ground it holds={}", n_out, out, n_back, back, grounded));

        // quasi-connectivity
        let mut w = new_world();
        place(&mut w, p(5, 1, 5), piston("piston", V_EAST));
        place(&mut w, p(4, 2, 5), rblock());
        steps(&mut w, 4);
        let dormant = !is_true(w.get(p(5, 1, 5)), P_EXTENDED);
        place(&mut w, p(5, 1, 6), stone());
        let n = until(&mut w, 10, |w| is(w, p(6, 1, 5), "piston_head"));
        r.check("quasi-connectivity", dormant && n == Some(3) && is_true(w.get(p(5, 1, 5)), P_EXTENDED), format!("power diagonally above alone: extended={}, after a block update: head out after {:?} ticks", !dormant, n));

        // a 1-tick pulse makes a sticky piston leave its block behind
        let mut w = new_world();
        place(&mut w, p(2, 1, 5), piston("sticky_piston", V_EAST));
        place(&mut w, p(3, 1, 5), stone());
        place(&mut w, p(2, 1, 4), rblock());
        step(&mut w);
        w.set_block(p(2, 1, 4), 0, F_ALL);
        steps(&mut w, 6);
        r.check("sticky piston drops on short pulse", is(&w, p(4, 1, 5), "stone") && is(&w, p(3, 1, 5), "air") && is(&w, p(2, 1, 5), "sticky_piston") && !is_true(w.get(p(2, 1, 5)), P_EXTENDED),
            format!("{} | {} | {}", name(&w, p(2, 1, 5)), name(&w, p(3, 1, 5)), name(&w, p(4, 1, 5))));
    }

    // ---------------------------------------------------------------- 7 observers
    {
        let mut w = new_world();
        place(&mut w, p(5, 1, 5), st("observer", &[(P_FACING, V_EAST), (P_POWERED, V_FALSE)]));
        place_wire(&mut w, p(4, 1, 5));
        steps(&mut w, 6);
        let quiet = !is_true(w.get(p(5, 1, 5)), P_POWERED);
        place(&mut w, p(6, 1, 5), stone());
        let mut trace = Vec::new();
        for _ in 0..7 {
            step(&mut w);
            trace.push((is_true(w.get(p(5, 1, 5)), P_POWERED) as i32, pw(&w, p(4, 1, 5))));
        }
        let want = vec![(0, 0), (1, 15), (1, 15), (0, 0), (0, 0), (0, 0), (0, 0)];
        r.check("observer 2-tick pulse", quiet && trace == want, format!("(powered, dust behind) per tick after the change: {:?}", trace));

        // two observers looking at each other clock; the piston behind one of them keeps moving
        let mut w = new_world();
        place(&mut w, p(4, 1, 5), piston("piston", V_UP));
        place(&mut w, p(5, 1, 5), st("observer", &[(P_FACING, V_EAST), (P_POWERED, V_FALSE)]));
        place(&mut w, p(6, 1, 5), st("observer", &[(P_FACING, V_WEST), (P_POWERED, V_FALSE)]));
        let mut ext_changes = 0;
        let mut obs_rises = 0;
        let mut pe = false;
        let mut po = false;
        let mut first_rises = Vec::new();
        for i in 1..=60 {
            step(&mut w);
            let e = is_true(w.get(p(4, 1, 5)), P_EXTENDED) || is(&w, p(4, 1, 5), "moving_piston");
            let o = is_true(w.get(p(5, 1, 5)), P_POWERED);
            if e != pe { ext_changes += 1; }
            if o && !po {
                obs_rises += 1;
                if first_rises.len() < 4 { first_rises.push(i); }
            }
            pe = e;
            po = o;
        }
        // by the tick order of the source the pair settles into a 6-tick period: A on at 2, 8, 14 ... (B's tick, asked
        // for during A's state change, runs before A's own at the same game tick)
        r.check("observer clock drives piston", obs_rises == 10 && first_rises == vec![2, 8, 14, 20] && ext_changes == 20, format!("60 ticks: observer pulses {} (first at ticks {:?}), piston state changes {}", obs_rises, first_rises, ext_changes));
    }

    // ---------------------------------------------------------------- 8 button, doors
    {
        let mut w = new_world();
        place(&mut w, p(5, 1, 5), st("oak_button", &[(P_FACE, V_FLOOR), (P_FACING, V_NORTH), (P_POWERED, V_FALSE)]));
        place(&mut w, p(6, 1, 5), st("oak_door", &[(P_FACING, V_NORTH), (P_HALF, V_LOWER), (P_HINGE, V_LEFT), (P_OPEN, V_FALSE), (P_POWERED, V_FALSE)]));
        place(&mut w, p(5, 1, 9), st("stone_button", &[(P_FACE, V_FLOOR), (P_FACING, V_NORTH), (P_POWERED, V_FALSE)]));
        place(&mut w, p(6, 1, 9), lamp());
        click(&mut w, p(5, 1, 5));
        click(&mut w, p(5, 1, 9));
        let open_now = is_true(w.get(p(6, 1, 5)), P_OPEN) && is_true(w.get(p(6, 1, 5)), P_POWERED);
        let mut wood = None;
        let mut stone_b = None;
        for i in 1..=40 {
            step(&mut w);
            if wood.is_none() && !is_true(w.get(p(6, 1, 5)), P_OPEN) { wood = Some(i); }
            if stone_b.is_none() && !is_true(w.get(p(5, 1, 9)), P_POWERED) { stone_b = Some(i); }
        }
        r.check("wooden button: door open 30 ticks", open_now && wood == Some(30) && stone_b == Some(20), format!("door open at once={}, closed after {:?} ticks; stone button out after {:?}", open_now, wood, stone_b));

        let mut w = new_world();
        place(&mut w, p(5, 1, 5), floor_lever());
        place(&mut w, p(6, 1, 5), st("iron_door", &[(P_FACING, V_NORTH), (P_HALF, V_LOWER), (P_HINGE, V_LEFT), (P_OPEN, V_FALSE), (P_POWERED, V_FALSE)]));
        place(&mut w, p(5, 1, 6), st("iron_trapdoor", &[(P_FACING, V_NORTH), (P_HALF, V_BOTTOM), (P_OPEN, V_FALSE), (P_POWERED, V_FALSE)]));
        place(&mut w, p(4, 1, 5), st("oak_fence_gate", &[(P_FACING, V_NORTH), (P_OPEN, V_FALSE), (P_POWERED, V_FALSE)]));
        click(&mut w, p(5, 1, 5));
        let open = [p(6, 1, 5), p(5, 1, 6), p(4, 1, 5)].iter().all(|q| is_true(w.get(*q), P_OPEN) && is_true(w.get(*q), P_POWERED));
        click(&mut w, p(5, 1, 5));
        let closed = [p(6, 1, 5), p(5, 1, 6), p(4, 1, 5)].iter().all(|q| !is_true(w.get(*q), P_OPEN) && !is_true(w.get(*q), P_POWERED));
        r.check("lever: iron door, trapdoor, gate", open && closed, format!("all open with the lever={}, all closed without={}", open, closed));
    }

    // ---------------------------------------------------------------- 9 rails
    {
        let mut w = new_world();
        for x in 3..=13 { place(&mut w, p(x, 1, 5), st("powered_rail", &[(P_SHAPE, V_EAST_WEST), (P_POWERED, V_FALSE)])); }
        place(&mut w, p(2, 1, 5), rblock());
        let on: Vec<i32> = (3..=13).map(|x| is_true(w.get(p(x, 1, 5)), P_POWERED) as i32).collect();
        w.set_block(p(2, 1, 5), 0, F_ALL);
        let off = (3..=13).all(|x| !is_true(w.get(p(x, 1, 5)), P_POWERED));
        r.check("powered rail line", on == vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0] && off, format!("powered {:?} (source rail + 8), all off without the source={}", on, off));
        // plain rails: a turn and a slope
        let mut w = new_world();
        let rail = st("rail", &[(P_SHAPE, V_NORTH_SOUTH)]);
        place(&mut w, p(5, 1, 5), rail);
        place(&mut w, p(5, 1, 6), rail);
        place(&mut w, p(6, 1, 6), rail);
        let corner = name(&w, p(5, 1, 6));
        let end = name(&w, p(6, 1, 6));
        place(&mut w, p(8, 1, 6), stone());
        place(&mut w, p(7, 1, 6), rail);
        place(&mut w, p(8, 2, 6), rail);
        let slope = name(&w, p(7, 1, 6));
        r.check("rail shapes", corner == "rail[shape=north_east,waterlogged=false]" && end == "rail[shape=east_west,waterlogged=false]" && slope == "rail[shape=ascending_east,waterlogged=false]",
            format!("corner {}, end {}, slope {}", corner, end, slope));
    }

    // ---------------------------------------------------------------- 10 TNT
    {
        let mut w = new_world();
        place(&mut w, p(5, 1, 5), st("tnt", &[(P_UNSTABLE, V_FALSE)]));
        place(&mut w, p(6, 1, 5), rblock());
        let gone = is(&w, p(5, 1, 5), "air");
        // placed next to power
        place(&mut w, p(7, 1, 5), st("tnt", &[(P_UNSTABLE, V_FALSE)]));
        let gone2 = is(&w, p(7, 1, 5), "air");
        r.check("TNT next to power is primed", gone && gone2 && w.unmodelled == 2 && rs::has_neighbor_signal(&w, p(5, 1, 5)), format!("removed={}, placed next to power removed={}, unmodelled={}", gone, gone2, w.unmodelled));
    }

    // ---------------------------------------------------------------- 11 pressure plate
    {
        let mut w = new_world();
        let plate = p(5, 1, 5);
        place(&mut w, plate, st("stone_pressure_plate", &[(P_POWERED, V_FALSE)]));
        place(&mut w, p(6, 1, 5), lamp());
        steps(&mut w, 3);
        w.entities.push(EntityView { bb: Aabb::new(5.2, 1.0, 5.2, 5.8, 2.8, 5.8), pos: [5.5, 1.0, 5.5], is_player: true });
        let s = w.get(plate);
        rules::entity_inside(&mut w, s, plate);
        let on = is_true(w.get(plate), P_POWERED) && lit(&w, p(6, 1, 5));
        steps(&mut w, 45);
        let held = is_true(w.get(plate), P_POWERED);
        w.entities.clear();
        let n = until(&mut w, 40, |w| !is_true(w.get(plate), P_POWERED));
        let n_lamp = until(&mut w, 10, |w| !lit(w, p(6, 1, 5)));
        r.check("pressure plate under the player", on && held && n.map_or(false, |x| x >= 1 && x <= 20) && n_lamp == Some(4),
            format!("on at once={}, held while standing={}, released {:?} ticks after leaving (next 20-tick check), lamp off {:?} later", on, held, n, n_lamp));
    }

    // ---------------------------------------------------------------- 12 tripwire
    {
        let mut w = new_world();
        place(&mut w, p(2, 1, 5), stone());
        place(&mut w, p(8, 1, 5), stone());
        let hook = |f: u8| st("tripwire_hook", &[(P_FACING, f), (P_ATTACHED, V_FALSE), (P_POWERED, V_FALSE)]);
        place(&mut w, p(3, 1, 5), hook(V_EAST));
        place(&mut w, p(7, 1, 5), hook(V_WEST));
        place(&mut w, p(7, 1, 6), lamp());
        let wire = st("tripwire", &[(P_ATTACHED, V_FALSE), (P_DISARMED, V_FALSE), (P_POWERED, V_FALSE), (P_NORTH, V_FALSE), (P_SOUTH, V_FALSE), (P_EAST, V_TRUE), (P_WEST, V_TRUE)]);
        place(&mut w, p(4, 1, 5), wire);
        place(&mut w, p(5, 1, 5), wire);
        let early = is_true(w.get(p(3, 1, 5)), P_ATTACHED);
        place(&mut w, p(6, 1, 5), wire);
        let attached = [3, 4, 5, 6, 7].iter().all(|x| is_true(w.get(p(*x, 1, 5)), P_ATTACHED));
        w.entities.push(EntityView { bb: Aabb::new(4.7, 1.0, 5.2, 5.3, 2.8, 5.8), pos: [5.0, 1.0, 5.5], is_player: true });
        let s = w.get(p(5, 1, 5));
        rules::entity_inside(&mut w, s, p(5, 1, 5));
        let tripped = is_true(w.get(p(3, 1, 5)), P_POWERED) && is_true(w.get(p(7, 1, 5)), P_POWERED) && lit(&w, p(7, 1, 6));
        w.entities.clear();
        let n = until(&mut w, 30, |w| !is_true(w.get(p(7, 1, 5)), P_POWERED));
        r.check("tripwire and hooks", !early && attached && tripped && n.map_or(false, |x| x <= 11), format!("attached only when complete={}, tripped by the player={}, released after {:?} ticks", !early && attached, tripped, n));
    }

    // ---------------------------------------------------------------- 13 the small ones
    {
        let mut w = new_world();
        // dispenser: triggered follows the power (also from above its head: quasi-connectivity), hopper: enabled
        place(&mut w, p(3, 1, 3), st("dispenser", &[(P_FACING, V_NORTH), (P_TRIGGERED, V_FALSE)]));
        place(&mut w, p(3, 1, 6), st("hopper", &[(P_FACING, V_DOWN), (P_ENABLED, V_TRUE)]));
        place(&mut w, p(4, 1, 3), rblock());
        place(&mut w, p(4, 1, 6), rblock());
        let d1 = is_true(w.get(p(3, 1, 3)), P_TRIGGERED) && !is_true(w.get(p(3, 1, 6)), P_ENABLED);
        w.set_block(p(4, 1, 3), 0, F_ALL);
        w.set_block(p(4, 1, 6), 0, F_ALL);
        let d2 = !is_true(w.get(p(3, 1, 3)), P_TRIGGERED) && is_true(w.get(p(3, 1, 6)), P_ENABLED);
        let menu = click(&mut w, p(3, 1, 3)) == Use::Consume && w.opened_menu == Some((p(3, 1, 3), 0));
        // note block: instrument from below, powered
        place(&mut w, p(8, 1, 3), st("note_block", &[(P_POWERED, V_FALSE)]));
        let grass_note = name(&w, p(8, 1, 3));
        place(&mut w, p(10, 1, 3), t().default_of("gold_block"));
        let nb = rules::update_shape(&mut w, st("note_block", &[(P_POWERED, V_FALSE)]), p(10, 2, 3), DOWN, p(10, 1, 3), t().default_of("gold_block"));
        place(&mut w, p(10, 2, 3), nb);
        let gold_note = name(&w, p(10, 2, 3));
        click(&mut w, p(10, 2, 3));
        let tuned = get(w.get(p(10, 2, 3)), P_NOTE) == Some(1);
        place(&mut w, p(11, 2, 3), rblock());
        let note_pow = is_true(w.get(p(10, 2, 3)), P_POWERED);
        // copper bulb toggles on each rising edge
        place(&mut w, p(8, 1, 8), st("copper_bulb", &[(P_LIT, V_FALSE), (P_POWERED, V_FALSE)]));
        place(&mut w, p(9, 1, 8), floor_lever());
        click(&mut w, p(9, 1, 8));
        let b1 = lit(&w, p(8, 1, 8));
        click(&mut w, p(9, 1, 8));
        let b2 = lit(&w, p(8, 1, 8));
        click(&mut w, p(9, 1, 8));
        let b3 = lit(&w, p(8, 1, 8));
        // daylight detector: 15 at noon from the next multiple of 20 ticks, 0 when inverted
        place(&mut w, p(14, 1, 8), st("daylight_detector", &[(P_INVERTED, V_FALSE), (P_POWER, 0)]));
        let n_day = until(&mut w, 25, |w| pw(w, p(14, 1, 8)) == 15);
        let at = w.game_time;
        click(&mut w, p(14, 1, 8));
        let inv = pw(&w, p(14, 1, 8));
        r.check("dispenser, hopper, note, bulb, daylight", d1 && d2 && menu && grass_note.contains("instrument=harp") && gold_note.contains("instrument=bell") && tuned && note_pow && b1 && b2 && !b3 && n_day.is_some() && at % 20 == 0 && inv == 0,
            format!("triggered/enabled follow power={}, menu={}, {} / {}, bulb lit {} {} {}, detector 15 at game time {} then {} inverted", d1 && d2, menu, grass_note, gold_note, b1, b2, b3, at, inv));
    }

    // ---------------------------------------------------------------- 14 odd input: random states, clicks and ticks
    {
        let names = ["redstone_wire", "redstone_torch", "redstone_wall_torch", "repeater", "comparator", "redstone_lamp", "redstone_block", "piston", "sticky_piston",
            "piston_head", "moving_piston", "observer", "dispenser", "dropper", "hopper", "note_block", "lever", "stone_button", "oak_button", "stone_pressure_plate",
            "light_weighted_pressure_plate", "tripwire", "tripwire_hook", "target", "daylight_detector", "tnt", "powered_rail", "activator_rail", "detector_rail", "rail",
            "copper_bulb", "crafter", "lightning_rod", "sculk_sensor", "calibrated_sculk_sensor", "bell", "jukebox", "command_block", "oak_door", "iron_trapdoor",
            "oak_fence_gate", "trapped_chest", "lectern", "slime_block", "honey_block", "stone", "stone", "stone", "glass", "obsidian", "white_glazed_terracotta",
            "sand", "torch", "oak_leaves", "composter", "cake", "chest"];
        let args: Vec<String> = std::env::args().collect();
        let worlds: u64 = args.get(1).and_then(|x| x.parse().ok()).unwrap_or(300);
        let mut seed: u64 = 0x9E3779B97F4A7C15;
        let mut rnd = move |n: u64| -> u64 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) % n.max(1)
        };
        let (mut sets, mut ticks, mut unmodelled) = (0u64, 0u64, 0u64);
        for _ in 0..worlds {
            let mut w = new_world();
            w.entities.push(EntityView { bb: Aabb::new(6.2, 1.0, 6.2, 6.8, 2.8, 6.8), pos: [6.5, 1.0, 6.5], is_player: true });
            for _ in 0..80 {
                let q = p(3 + rnd(8) as i32, 1 + rnd(3) as i32, 3 + rnd(8) as i32);
                match rnd(10) {
                    0..=5 => {
                        let b = &t().blocks[t().bid(names[rnd(names.len() as u64) as usize]) as usize];
                        let s = b.first + rnd(b.n as u64) as u16;
                        place(&mut w, q, s);
                    }
                    6 => { w.set_block(q, 0, F_ALL); }
                    7 => { click(&mut w, q); }
                    8 => {
                        let s = w.get(q);
                        rules::entity_inside(&mut w, s, q);
                        rules::step_on(&mut w, s, q, false);
                        rules::attack(&mut w, s, q);
                    }
                    _ => {
                        let n = 1 + rnd(4) as usize;
                        steps(&mut w, n);
                        ticks += n as u64;
                    }
                }
            }
            steps(&mut w, 40);
            ticks += 40;
            sets += w.n_set;
            unmodelled += w.unmodelled;
        }
        r.check("random states, clicks, ticks", true, format!("{} worlds, {} ticks, {} block writes, {} unmodelled events, no panic", worlds, ticks, sets, unmodelled));
    }

    println!("---- {} passed, {} failed", r.pass, r.fail);
    if r.fail > 0 { std::process::exit(1); }
}
