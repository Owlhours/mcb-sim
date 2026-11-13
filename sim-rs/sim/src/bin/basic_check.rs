//! Scenario checks for `rules/basic.rs` (reasoned out from the vanilla source; there is no game to compare with).
use mcsim::geom::*;
use mcsim::props::*;
use mcsim::rules::{self, basic, Clicker, PlaceCtx, Use};
use mcsim::tables::*;
use mcsim::world::*;

struct Tally { pass: u32, fail: u32, pending: u32, stub: bool }
impl Tally {
    fn check(&mut self, name: &str, ok: bool) {
        if ok { self.pass += 1; println!("PASS  {name}"); } else { self.fail += 1; println!("FAIL  {name}"); }
    }
    /// A scenario that needs the real `can_survive` (place.rs): reported as pending while that is still the permissive stub.
    fn check_survive(&mut self, name: &str, ok: bool) {
        if ok {
            self.pass += 1;
            println!("PASS  {name}");
        } else if self.stub {
            self.pending += 1;
            println!("PEND  {name}  (rules::can_survive is still the permissive stub)");
        } else {
            self.fail += 1;
            println!("FAIL  {name}");
        }
    }
}

fn st(name: &str) -> S { t().default_of(name) }
fn world() -> World { World::new(0, 0, 0, 16, 16, 16, 0, 319) }
fn ticks(w: &mut World, n: i32) { for _ in 0..n { w.game_time += 1; w.run_scheduled(); w.run_block_events(); } }
fn clicker(yaw: f32, item: &str) -> Clicker {
    Clicker { pos: [0.5, 1.0, 0.5], yaw, pitch: 0.0, sneak: false, item: *t().item_by_name.get(item).unwrap_or(&0), consumed: false }
}
fn ctx(pos: P) -> PlaceCtx {
    PlaceCtx { item: 0, hit_pos: pos.below(), face: UP, hit: [pos.x as f64 + 0.5, pos.y as f64, pos.z as f64 + 0.5], inside: false, yaw: 0.0, pitch: 0.0, sneak: false, player_pos: [0.5, 1.0, 0.5], replace_clicked: false }
}
fn name(w: &World, q: P) -> String { t().name(w.get(q)) }
fn click(w: &mut World, q: P, face: Dir, who: &mut Clicker) -> Use { let s = w.get(q); rules::use_without_item(w, s, q, face, centre(q), who) }
fn click_item(w: &mut World, q: P, face: Dir, who: &mut Clicker) -> Use { let s = w.get(q); rules::use_item_on(w, s, q, face, centre(q), who) }
fn centre(q: P) -> [f64; 3] { [q.x as f64 + 0.5, q.y as f64 + 0.5, q.z as f64 + 0.5] }

fn main() {
    init("/work/reference", "/work/data");
    let tb = t();
    let stub = rules::can_survive(&world(), st("torch"), p(8, 8, 8));
    let mut ty = Tally { pass: 0, fail: 0, pending: 0, stub };
    if stub { println!("note: rules::can_survive is the permissive stub; scenarios that need it are reported as PEND"); }

    // ---- every block and tag name the rules look up exists
    {
        let blocks = ["barrier", "carved_pumpkin", "jack_o_lantern", "melon", "pumpkin", "scaffolding", "pale_moss_carpet", "cave_vines", "cave_vines_plant", "kelp", "kelp_plant",
            "twisting_vines", "twisting_vines_plant", "weeping_vines", "weeping_vines_plant", "chorus_flower", "hay_block", "attached_melon_stem", "melon_stem", "pumpkin_stem",
            "bamboo", "big_dripleaf_stem", "dirt", "snow_block", "iron_block", "air", "copper_chest", "turtle_egg", "ice", "redstone_ore", "deepslate_redstone_ore",
            "wither_skeleton_skull", "wither_skeleton_wall_skull", "flower_pot", "iron_door", "iron_trapdoor", "trapped_chest", "fire"];
        let tags = ["shulker_boxes", "fire", "fences", "wooden_fences", "walls", "wall_post_override", "copper_chests", "prevents_nearby_leaf_decay", "speleothems",
            "supports_chorus_plant", "snow", "wooden_shelves", "sniffer_egg_hatch_boost", "copper", "pale_oak_logs", "wither_summon_base_blocks"];
        let mut missing = Vec::new();
        for b in blocks { if !tb.block_by_name.contains_key(b) { missing.push(b); } }
        for g in tags { if !tb.block_tags.contains_key(g) { missing.push(g); } }
        if !missing.is_empty() { println!("  missing: {missing:?}"); }
        ty.check("names: all blocks and tags used by the rules exist", missing.is_empty());
    }
    // ---- no hook panics on any state (properties that a kind is assumed to have)
    {
        let mut w = world();
        let mut n = 0u32;
        for s in 0..tb.n as u32 {
            let s = s as S;
            let q = p(8, 8, 8);
            w.put_raw(q, s);
            for d in DIRS {
                let _ = basic::update_shape(&mut w, s, q, d, q.rel(d), 0);
                let _ = basic::update_shape(&mut w, s, q, d, q.rel(d), s);
            }
            basic::neighbor_changed(&mut w, s, q, 0, false);
            basic::affect_neighbors_after_removal(&mut w, s, q, false);
            let _ = basic::trigger_event(&mut w, s, q, 1, 1);
            w.put_raw(q, s);
            basic::tick(&mut w, s, q);
            w.put_raw(q, s);
            basic::on_place(&mut w, s, q, 0, false);
            w.put_raw(q, s);
            basic::set_placed_by(&mut w, q, s, &ctx(q));
            w.put_raw(q, s);
            basic::player_will_destroy(&mut w, q, s);
            basic::destroyed(&mut w, q, s);
            w.put_raw(q, s);
            basic::attack(&mut w, s, q);
            for item in ["air", "stone", "glowstone", "candle", "oak_sapling", "oak_hanging_sign", "wheat_seeds"] {
                w.put_raw(q, s);
                let mut who = clicker(0.0, item);
                let _ = basic::use_item_on(&mut w, s, q, NORTH, centre(q), &mut who);
            }
            w.put_raw(q, s);
            let mut who = clicker(0.0, "air");
            let _ = basic::use_without_item(&mut w, s, q, NORTH, centre(q), &mut who);
            w.put_raw(q, s);
            basic::step_on(&mut w, s, q);
            basic::entity_inside(&mut w, s, q);
            basic::menu_closed(&mut w, q);
            // clean the neighbourhood for the next state
            for dx in -2..=2 { for dy in -2..=2 { for dz in -2..=2 { w.put_raw(q.off(dx, dy, dz), 0); } } }
            w.falling.clear();
            n += 1;
        }
        ty.check(&format!("robustness: all hooks ran on all {n} states without a panic"), true);
    }

    // ---- fences
    {
        let mut w = world();
        let (a, b) = (p(5, 1, 5), p(6, 1, 5));
        w.set_block(a, st("oak_fence"), F_ALL);
        w.set_block(b, st("oak_fence"), F_ALL);
        // the block placed second gets its own connections from placement (place.rs); here: by hand
        let sb = { let ns = w.get(a); basic::update_shape(&mut w, st("oak_fence"), b, WEST, a, ns) };
        w.set_block(b, sb, F_ALL);
        println!("  {} | {}", name(&w, a), name(&w, b));
        ty.check("fence: two posts connect both ways", is_true(w.get(a), P_EAST) && is_true(w.get(b), P_WEST) && !is_true(w.get(a), P_WEST) && !is_true(w.get(b), P_EAST));
        w.set_block(p(5, 1, 4), st("stone"), F_ALL);
        w.set_block(p(5, 1, 6), st("nether_brick_fence"), F_ALL);
        w.set_block(p(4, 1, 5), st("oak_leaves"), F_ALL);
        println!("  {}", name(&w, a));
        ty.check("fence: connects to a solid block, not to a nether brick fence or leaves", is_true(w.get(a), P_NORTH) && !is_true(w.get(a), P_SOUTH) && !is_true(w.get(a), P_WEST));
        w.set_block(p(5, 1, 4), 0, F_ALL);
        ty.check("fence: disconnects when the block goes", !is_true(w.get(a), P_NORTH));
        // glass pane next to a wall and a pane
        let mut w = world();
        w.set_block(a, st("glass_pane"), F_ALL);
        w.set_block(b, st("iron_bars"), F_ALL);
        w.set_block(p(4, 1, 5), st("cobblestone_wall"), F_ALL);
        w.set_block(p(5, 1, 4), st("oak_fence"), F_ALL);
        println!("  {}", name(&w, a));
        ty.check("pane: attaches to bars and walls, not to a fence", is_true(w.get(a), P_EAST) && is_true(w.get(a), P_WEST) && !is_true(w.get(a), P_NORTH));
    }
    // ---- walls
    {
        let mut w = world();
        let (a, b, c) = (p(5, 1, 5), p(6, 1, 5), p(7, 1, 5));
        let wall = st("cobblestone_wall");
        w.set_block(a, wall, F_ALL);
        w.set_block(b, wall, F_ALL);
        w.set_block(c, wall, F_ALL);
        // give the later ones their own state as placement would (updateShape from each side)
        for q in [b, c] {
            let mut s = w.get(q);
            for d in HORIZONTAL { let ns = w.get(q.rel(d)); s = basic::update_shape(&mut w, s, q, d, q.rel(d), ns); }
            w.set_block(q, s, F_ALL);
        }
        println!("  {} | {} | {}", name(&w, a), name(&w, b), name(&w, c));
        let sa = w.get(a);
        let sb = w.get(b);
        ty.check("wall: end piece has one low side and a post", val(sa, P_EAST) == V_LOW && val(sa, P_WEST) == V_NONE && is_true(sa, P_UP));
        ty.check("wall: middle piece has two low sides and no post", val(sb, P_EAST) == V_LOW && val(sb, P_WEST) == V_LOW && !is_true(sb, P_UP));
        w.set_block(b.above(), st("stone"), F_ALL);
        let sb = w.get(b);
        println!("  under stone: {}", name(&w, b));
        ty.check("wall: under a full block the sides are tall and there is no post", val(sb, P_EAST) == V_TALL && val(sb, P_WEST) == V_TALL && !is_true(sb, P_UP));
        w.set_block(b.above(), st("torch"), F_ALL);
        let sb = w.get(b);
        println!("  under torch: {}", name(&w, b));
        ty.check("wall: a torch on top raises the post (wall_post_override), sides low", val(sb, P_EAST) == V_LOW && is_true(sb, P_UP));
        w.set_block(b.above(), wall, F_ALL);
        let sb = w.get(b);
        println!("  under wall post: {}", name(&w, b));
        ty.check("wall: under another wall's post: post, sides low (the post does not cover the side strips)", is_true(sb, P_UP) && val(sb, P_EAST) == V_LOW);
        w.set_block(b.above(), st("stone_slab"), F_ALL);
        let sb = w.get(b);
        println!("  under bottom slab: {}", name(&w, b));
        ty.check("wall: under a bottom slab the sides are tall", val(sb, P_EAST) == V_TALL && val(sb, P_WEST) == V_TALL && !is_true(sb, P_UP));
        w.set_block(a, 0, F_ALL);
        let sb = w.get(b);
        println!("  one side gone: {}", name(&w, b));
        ty.check("wall: losing one side gives a corner post", val(sb, P_WEST) == V_NONE && is_true(sb, P_UP));
    }
    // ---- stairs
    {
        let mut w = world();
        let a = p(5, 1, 5);
        let north = with(st("oak_stairs"), P_FACING, V_NORTH);
        let east = with(st("oak_stairs"), P_FACING, V_EAST);
        w.set_block(a, north, F_ALL);
        w.set_block(a.rel(NORTH), east, F_ALL);
        println!("  {}", name(&w, a));
        ty.check("stairs: a turned stair behind makes an outer corner", val(w.get(a), P_SHAPE) == V_OUTER_RIGHT);
        w.set_block(a.rel(NORTH), with(st("oak_stairs"), P_FACING, V_WEST), F_ALL);
        ty.check("stairs: ... to the other side", val(w.get(a), P_SHAPE) == V_OUTER_LEFT);
        w.set_block(a.rel(NORTH), 0, F_ALL);
        ty.check("stairs: straight again when it goes", val(w.get(a), P_SHAPE) == V_STRAIGHT);
        w.set_block(a.rel(SOUTH), east, F_ALL);
        println!("  {}", name(&w, a));
        ty.check("stairs: a turned stair in front makes an inner corner", val(w.get(a), P_SHAPE) == V_INNER_RIGHT);
        w.set_block(a.rel(SOUTH), with(with(st("oak_stairs"), P_FACING, V_EAST), P_HALF, V_TOP), F_ALL);
        ty.check("stairs: not with the other half", val(w.get(a), P_SHAPE) == V_STRAIGHT);
    }
    // ---- doors
    {
        let mut w = world();
        let a = p(5, 1, 5);
        let door = st("oak_door");
        w.set_block(a, door, F_ALL);
        rules::set_placed_by(&mut w, a, door, &ctx(a));
        println!("  {} / {}", name(&w, a), name(&w, a.above()));
        ty.check("door: setPlacedBy adds the upper half", val(w.get(a), P_HALF) == V_LOWER && same_block(w.get(a.above()), door) && val(w.get(a.above()), P_HALF) == V_UPPER);
        let mut who = clicker(0.0, "air");
        let r = click(&mut w, a, NORTH, &mut who);
        ty.check("door: a click opens both halves", r == Use::Consume && is_true(w.get(a), P_OPEN) && is_true(w.get(a.above()), P_OPEN));
        let r = click(&mut w, a.above(), NORTH, &mut who);
        ty.check("door: a click on the upper half closes both", r == Use::Consume && !is_true(w.get(a), P_OPEN) && !is_true(w.get(a.above()), P_OPEN));
        w.set_block(a.below(), 0, F_ALL);
        ty.check_survive("door: removing the block below removes both halves", w.get(a) == 0 && w.get(a.above()) == 0);
        // breaking one half
        let mut w = world();
        w.set_block(a, door, F_ALL);
        rules::set_placed_by(&mut w, a, door, &ctx(a));
        let up = w.get(a.above());
        rules::player_will_destroy(&mut w, a.above(), up);
        w.set_block(a.above(), 0, F_ALL);
        rules::destroyed(&mut w, a.above(), up);
        ty.check("door: breaking the upper half removes the lower", w.get(a) == 0 && w.get(a.above()) == 0);
        // iron
        let mut w = world();
        let iron = st("iron_door");
        w.set_block(a, iron, F_ALL);
        rules::set_placed_by(&mut w, a, iron, &ctx(a));
        let before = (w.get(a), w.get(a.above()));
        let r = click(&mut w, a, NORTH, &mut who);
        ty.check("door: an iron door does not react to a click (PASS)", r == Use::Pass && before == (w.get(a), w.get(a.above())));
        let mut w = world();
        let copper = st("copper_door");
        w.set_block(a, copper, F_ALL);
        rules::set_placed_by(&mut w, a, copper, &ctx(a));
        let r = click(&mut w, a, NORTH, &mut who);
        ty.check("door: a copper door opens by hand", r == Use::Consume && is_true(w.get(a), P_OPEN) && is_true(w.get(a.above()), P_OPEN));
        // trapdoors
        let mut w = world();
        w.set_block(a, st("oak_trapdoor"), F_ALL);
        let r = click(&mut w, a, UP, &mut who);
        let ok1 = r == Use::Consume && is_true(w.get(a), P_OPEN);
        w.set_block(a, st("iron_trapdoor"), F_ALL);
        let r = click(&mut w, a, UP, &mut who);
        ty.check("trapdoor: wood toggles, iron passes", ok1 && r == Use::Pass && !is_true(w.get(a), P_OPEN));
    }
    // ---- torch
    {
        let mut w = world();
        let a = p(5, 1, 5);
        w.set_block(a, st("stone"), F_ALL);
        w.set_block(a.above(), st("torch"), F_ALL);
        w.set_block(a.rel(EAST), with(st("wall_torch"), P_FACING, V_EAST), F_ALL);
        w.set_block(a, 0, F_ALL);
        ty.check_survive("torch: standing and wall torch go when the block is removed", w.get(a.above()) == 0 && w.get(a.rel(EAST)) == 0);
    }
    // ---- sand
    {
        let mut w = world();
        let a = p(5, 5, 5);
        w.set_block(a, st("sand"), F_ALL);
        ticks(&mut w, 1);
        let still = w.get(a) == st("sand") && w.falling.is_empty();
        ticks(&mut w, 1);
        println!("  falling entities: {}", w.falling.len());
        ty.check("sand: in mid-air it becomes a falling block on its tick (2 ticks after placement)", still && w.get(a) == 0 && w.falling.len() == 1 && w.falling[0].state == st("sand"));
        let mut w = world();
        let a = p(5, 1, 5);
        w.set_block(a, st("sand"), F_ALL);
        w.set_block(a.above(), st("red_concrete_powder"), F_ALL);
        w.set_block(a.above().above(), st("anvil"), F_ALL);
        ticks(&mut w, 5);
        let rests = w.falling.is_empty() && w.get(a) == st("sand");
        w.set_block(a.below(), 0, F_ALL);
        ticks(&mut w, 8);
        println!("  falling after the ground went: {}", w.falling.len());
        ty.check("sand: a stack on the ground rests, and falls one by one when the ground goes", rests && w.falling.len() == 3 && w.get(a) == 0 && w.get(a.above()) == 0 && w.get(a.above().above()) == 0);
    }
    // ---- scaffolding
    {
        let sc = st("scaffolding");
        let mut w = world();
        // column of 3 at x=5 and a 4-block arm at the top
        let mut cells = vec![p(5, 1, 5), p(5, 2, 5), p(5, 3, 5)];
        for x in 6..10 { cells.push(p(x, 3, 5)); }
        for q in &cells { w.set_block(*q, sc, F_ALL); ticks(&mut w, 2); }
        let ds: Vec<String> = cells.iter().map(|q| format!("{}{}", val(w.get(*q), P_DISTANCE), if is_true(w.get(*q), P_BOTTOM) { "b" } else { "" })).collect();
        println!("  distances (b = bottom): {}", ds.join(" "));
        ty.check("scaffolding: column 0 0 0, arm 1b 2b 3b 4b", ds.join(" ") == "0 0 0 1b 2b 3b 4b" && w.falling.is_empty());
        w.set_block(p(5, 1, 5), 0, F_ALL);
        let mut n = 0;
        let mut log = Vec::new();
        while n < 400 {
            n += 1;
            ticks(&mut w, 1);
            let left = cells.iter().filter(|q| w.get(**q) == 0).count();
            if log.last().map_or(true, |l: &(i32, usize, usize)| l.1 != left || l.2 != w.falling.len()) { log.push((n, left, w.falling.len())); }
            if cells.iter().all(|q| w.get(*q) == 0) && w.scheduled_count() == 0 { break; }
        }
        println!("  after breaking the bottom block: (tick, cells gone, falling entities) {log:?}");
        println!("  everything gone after {n} ticks; {} became falling blocks, {} broke", w.falling.len(), cells.len() - 1 - w.falling.len());
        ty.check("scaffolding: breaking the bottom block brings the whole thing down", cells.iter().all(|q| w.get(*q) == 0) && n < 400);

        // overhang
        let mut w = world();
        w.set_block(p(2, 1, 5), sc, F_ALL);
        ticks(&mut w, 2);
        w.set_block(p(2, 2, 5), sc, F_ALL);
        ticks(&mut w, 2);
        let mut ok = val(w.get(p(2, 2, 5)), P_DISTANCE) == 0 && !is_true(w.get(p(2, 2, 5)), P_BOTTOM);
        let mut ds = vec![0u8];
        for i in 1..=6 {
            let q = p(2 + i, 2, 5);
            ok &= basic::scaffolding_can_survive(&w, q);
            w.set_block(q, sc, F_ALL);
            ticks(&mut w, 2);
            let s = w.get(q);
            ds.push(get(s, P_DISTANCE).unwrap_or(99));
            ok &= get(s, P_DISTANCE) == Some(i as u8) && is_true(s, P_BOTTOM);
        }
        let eighth = p(9, 2, 5);
        println!("  overhang distances: {ds:?}; distance for the next one: {}", basic::scaffolding_distance(&w, eighth));
        ty.check("scaffolding: overhang distances 0..6 with bottom=true; the 8th cannot survive", ok && !basic::scaffolding_can_survive(&w, eighth));
        w.set_block(eighth, sc, F_ALL);
        ticks(&mut w, 2);
        ty.check("scaffolding: an 8th one forced in falls on its tick", w.get(eighth) == 0 && w.falling.len() == 1);
    }
    // ---- snowy
    {
        let mut w = world();
        let g = p(5, 0, 5);
        w.set_block(g.above(), st("snow"), F_ALL);
        let on = is_true(w.get(g), P_SNOWY);
        w.set_block(g.above(), 0, F_ALL);
        ty.check("snowy: a snow layer on grass sets snowy, removing it clears it", on && !is_true(w.get(g), P_SNOWY));
        w.set_block(g, st("podzol"), F_ALL);
        w.set_block(g.above(), st("snow_block"), F_ALL);
        ty.check("snowy: podzol under a snow block", is_true(w.get(g), P_SNOWY));
    }
    // ---- chests
    {
        let mut w = world();
        let (a, b) = (p(5, 1, 5), p(6, 1, 5));
        let chest = with(st("chest"), P_FACING, V_NORTH);
        w.set_block(a, chest, F_ALL);
        // the second chest comes from placement as the right half (its partner is to its west when facing north)
        w.set_block(b, with(chest, P_TYPE, V_RIGHT), F_ALL);
        println!("  {} | {}", name(&w, a), name(&w, b));
        ty.check("chest: the single chest joins as type=left next to a type=right", val(w.get(a), P_TYPE) == V_LEFT && val(w.get(b), P_TYPE) == V_RIGHT);
        let mut who = clicker(0.0, "air");
        w.opened_menu = None;
        let r = click(&mut w, a, NORTH, &mut who);
        ty.check("chest: a double chest opens with 54 slots", r == Use::Consume && w.opened_menu == Some((a, 54)));
        basic::menu_closed(&mut w, a);
        w.set_block(b.above(), st("stone"), F_ALL);
        w.opened_menu = None;
        let r = click(&mut w, a, NORTH, &mut who);
        ty.check("chest: a solid block over either half blocks it (click used, nothing opens)", r == Use::Consume && w.opened_menu.is_none());
        w.set_block(b.above(), st("glass"), F_ALL);
        let _ = click(&mut w, a, NORTH, &mut who);
        ty.check("chest: glass above does not block", w.opened_menu == Some((a, 54)));
        w.set_block(b, 0, F_ALL);
        w.opened_menu = None;
        let _ = click(&mut w, a, NORTH, &mut who);
        ty.check("chest: single again when the partner goes (27 slots)", val(w.get(a), P_TYPE) == V_SINGLE && w.opened_menu == Some((a, 27)));
        // barrel
        w.set_block(b, st("barrel"), F_ALL);
        w.opened_menu = None;
        let _ = click(&mut w, b, NORTH, &mut who);
        let open = is_true(w.get(b), P_OPEN) && w.opened_menu == Some((b, 27));
        basic::menu_closed(&mut w, b);
        ty.check("barrel: opens (open=true, 27 slots) and closes", open && !is_true(w.get(b), P_OPEN));
        // shulker box under a block
        w.set_block(b, st("shulker_box"), F_ALL);
        w.set_block(b.above(), st("stone"), F_ALL);
        w.opened_menu = None;
        let _ = click(&mut w, b, NORTH, &mut who);
        let blocked = w.opened_menu.is_none();
        w.set_block(b.above(), st("torch"), F_ALL);
        let _ = click(&mut w, b, NORTH, &mut who);
        ty.check("shulker box: does not open under a block, opens under a torch", blocked && w.opened_menu == Some((b, 27)));
        w.opened_menu = None;
        w.set_block(a, st("crafting_table"), F_ALL);
        let r = click(&mut w, a, NORTH, &mut who);
        ty.check("crafting table: opens a screen without slots", r == Use::Consume && w.opened_menu == Some((a, 0)));
    }
    // ---- fence gate
    {
        let mut w = world();
        let a = p(5, 1, 5);
        let gate = with(st("oak_fence_gate"), P_FACING, V_NORTH);
        w.set_block(a, gate, F_ALL);
        w.set_block(a.rel(EAST), st("cobblestone_wall"), F_ALL);
        let one = is_true(w.get(a), P_IN_WALL);
        w.set_block(a.rel(WEST), st("cobblestone_wall"), F_ALL);
        w.set_block(a.rel(EAST), 0, F_ALL);
        let still = is_true(w.get(a), P_IN_WALL);
        w.set_block(a.rel(WEST), 0, F_ALL);
        ty.check("fence gate: in_wall with a wall on either side, cleared when both are gone", one && still && !is_true(w.get(a), P_IN_WALL));
        w.set_block(a.rel(EAST), st("cobblestone_wall"), F_ALL);
        w.set_block(a.rel(WEST), st("cobblestone_wall"), F_ALL);
        // walls placed before the gate: they connect to it when it arrives
        w.set_block(a, 0, F_ALL);
        w.set_block(a, with_bool(gate, P_IN_WALL, true), F_ALL);
        println!("  {} | {} | {}", name(&w, a.rel(WEST)), name(&w, a), name(&w, a.rel(EAST)));
        ty.check("fence gate: between walls in_wall=true and the walls connect to it", is_true(w.get(a), P_IN_WALL) && val(w.get(a.rel(EAST)), P_WEST) == V_LOW && val(w.get(a.rel(WEST)), P_EAST) == V_LOW);
        w.set_block(a.rel(NORTH), st("cobblestone_wall"), F_ALL);
        ty.check("fence gate: a wall in front of it does not connect", val(w.get(a.rel(NORTH)), P_SOUTH) == V_NONE);
        // opening away from the player: the gate faces north; a player looking south (yaw 0) is north of it looking at it
        let mut who = clicker(0.0, "air");
        let r = click(&mut w, a, NORTH, &mut who);
        let s = w.get(a);
        println!("  opened by a player looking south: {}", name(&w, a));
        ty.check("fence gate: opens, turning to the player's direction when it faced the player", r == Use::Consume && is_true(s, P_OPEN) && val(s, P_FACING) == V_SOUTH);
        let _ = click(&mut w, a, NORTH, &mut who);
        ty.check("fence gate: closes again keeping its facing", !is_true(w.get(a), P_OPEN) && val(w.get(a), P_FACING) == V_SOUTH);
    }
    // ---- beds and double plants
    {
        let mut w = world();
        let a = p(5, 1, 5);
        let bed = with(st("red_bed"), P_FACING, V_EAST);
        w.set_block(a, bed, F_ALL);
        rules::set_placed_by(&mut w, a, bed, &ctx(a));
        let head = w.get(a.rel(EAST));
        let made = same_block(head, bed) && val(head, P_PART) == V_HEAD && val(w.get(a), P_PART) == V_FOOT;
        let mut who = clicker(0.0, "air");
        let r = click(&mut w, a, UP, &mut who);
        w.set_block(a.rel(EAST), 0, F_ALL);
        ty.check("bed: setPlacedBy adds the head; a click at noon changes nothing; removing the head removes the foot", made && r == Use::Consume && w.get(a) == 0);
        let sun = st("sunflower");
        w.set_block(a, sun, F_ALL);
        rules::set_placed_by(&mut w, a, sun, &ctx(a));
        let made = same_block(w.get(a.above()), sun) && val(w.get(a.above()), P_HALF) == V_UPPER;
        w.set_block(a, 0, F_ALL);
        ty.check("tall flower: setPlacedBy adds the upper half; removing the lower removes the upper", made && w.get(a.above()) == 0);
        w.set_block(a, sun, F_ALL);
        rules::set_placed_by(&mut w, a, sun, &ctx(a));
        w.set_block(a.above(), 0, F_ALL);
        ty.check("tall flower: removing the upper removes the lower", w.get(a) == 0);
    }
    // ---- leaves
    {
        let mut w = world();
        let a = p(5, 3, 5);
        let leaves = with_bool(st("oak_leaves"), P_PERSISTENT, true);
        for i in 1..=3 { w.set_block(p(5 + i, 3, 5), leaves, F_ALL); }
        w.set_block(a, st("oak_log"), F_ALL);
        ticks(&mut w, 5);
        let ds: Vec<u8> = (1..=3).map(|i| val(w.get(p(5 + i, 3, 5)), P_DISTANCE)).collect();
        println!("  leaf distances from the log: {ds:?}");
        let ok = ds == vec![1, 2, 3];
        w.set_block(a, 0, F_ALL);
        ticks(&mut w, 12);
        let ds2: Vec<u8> = (1..=3).map(|i| val(w.get(p(5 + i, 3, 5)), P_DISTANCE)).collect();
        println!("  after the log went: {ds2:?}");
        ty.check("leaves: distance 1 2 3 from a log, back to 7 (and still there) when the log goes", ok && ds2 == vec![7, 7, 7]);
    }
    // ---- flower pot, cake, composter
    {
        let mut w = world();
        let a = p(5, 1, 5);
        w.set_block(a, st("flower_pot"), F_ALL);
        let mut who = clicker(0.0, "poppy");
        let r = click_item(&mut w, a, UP, &mut who);
        let potted = name(&w, a) == "potted_poppy" && who.consumed && r == Use::Consume;
        let mut who = clicker(0.0, "stone");
        let r2 = click_item(&mut w, a, UP, &mut who);
        let mut who = clicker(0.0, "azalea");
        w.set_block(a, st("flower_pot"), F_ALL);
        let _ = click_item(&mut w, a, UP, &mut who);
        println!("  azalea in a pot: {}", name(&w, a));
        ty.check("flower pot: a poppy goes in (item used); a stone block does not (try empty hand); azalea -> potted_azalea_bush", potted && r2 == Use::TryEmptyHand && name(&w, a) == "potted_azalea_bush");
        w.set_block(a, st("cake"), F_ALL);
        let mut who = clicker(0.0, "red_candle");
        let r = click_item(&mut w, a, UP, &mut who);
        let mut who2 = clicker(0.0, "air");
        let r2 = click(&mut w, a, UP, &mut who2);
        ty.check("cake: a candle makes a candle cake; eating fails (PASS)", r == Use::Consume && who.consumed && name(&w, a).starts_with("red_candle_cake") && r2 == Use::Pass);
        w.set_block(a, st("composter"), F_ALL);
        let mut who = clicker(0.0, "oak_leaves");
        let r = click_item(&mut w, a, UP, &mut who);
        let mut who2 = clicker(0.0, "stone");
        let r2 = click_item(&mut w, a, UP, &mut who2);
        ty.check("composter: the first compostable item always raises the level; stone is not compostable", r == Use::Consume && who.consumed && val(w.get(a), P_LEVEL) == 1 && r2 == Use::TryEmptyHand);
        w.set_block(a, with(st("composter"), P_LEVEL, 7), F_ALL);
        ticks(&mut w, 20);
        let ready = val(w.get(a), P_LEVEL) == 8;
        let mut who = clicker(0.0, "air");
        let r = click(&mut w, a, UP, &mut who);
        ty.check("composter: level 7 becomes 8 after 20 ticks; an empty-hand click empties it", ready && r == Use::Consume && val(w.get(a), P_LEVEL) == 0);
    }
    // ---- signs, candles, respawn anchor
    {
        let mut w = world();
        let a = p(5, 1, 5);
        w.set_block(a, st("oak_sign"), F_ALL);
        let mut who = clicker(0.0, "stone");
        let r = click_item(&mut w, a, NORTH, &mut who);
        let r2 = click(&mut w, a, NORTH, &mut who);
        ty.check("sign: a click with a block falls through to the empty hand, which opens the editor", r == Use::TryEmptyHand && r2 == Use::Consume && w.opened_menu == Some((a, 0)));
        w.set_block(a, with_bool(st("candle"), P_LIT, true), F_ALL);
        let mut who = clicker(0.0, "air");
        let r = click_item(&mut w, a, UP, &mut who);
        ty.check("candle: an empty hand puts it out", r == Use::Consume && !is_true(w.get(a), P_LIT));
        w.set_block(a, st("respawn_anchor"), F_ALL);
        let mut who = clicker(0.0, "air");
        let r0 = click(&mut w, a, UP, &mut who);
        let mut who = clicker(0.0, "glowstone");
        let r = click_item(&mut w, a, UP, &mut who);
        let charged = val(w.get(a), P_CHARGES) == 1 && who.consumed;
        let un = w.unmodelled;
        let mut who = clicker(0.0, "air");
        let r2 = click(&mut w, a, UP, &mut who);
        ty.check("respawn anchor: empty passes; glowstone charges it; using it in the overworld blows it up (unmodelled)", r0 == Use::Pass && r == Use::Consume && charged && r2 == Use::Consume && w.get(a) == 0 && w.unmodelled == un + 1);
    }
    // ---- big dripleaf
    {
        let mut w = world();
        let a = p(5, 1, 5);
        let leaf = st("big_dripleaf");
        w.set_block(a, leaf, F_ALL);
        w.set_block(a.above(), leaf, F_ALL);
        println!("  {} / {}", name(&w, a), name(&w, a.above()));
        let stem = name(&w, a).starts_with("big_dripleaf_stem");
        let top = a.above();
        let y = top.y as f64 + 15.0 / 16.0;
        w.entities.push(EntityView { bb: Aabb::new(5.2, y, 5.2, 5.8, y + 1.8, 5.8), pos: [5.5, y, 5.5], is_player: true });
        { let s = w.get(top); rules::entity_inside(&mut w, s, top); }
        let t0 = val(w.get(top), P_TILT) == V_UNSTABLE;
        w.entities.clear();
        ticks(&mut w, 10);
        let t1 = val(w.get(top), P_TILT) == V_PARTIAL;
        ticks(&mut w, 10);
        let t2 = val(w.get(top), P_TILT) == V_FULL;
        ticks(&mut w, 99);
        let t3 = val(w.get(top), P_TILT) == V_FULL;
        ticks(&mut w, 1);
        ty.check("big dripleaf: a leaf under a leaf becomes a stem; the player tilts it: unstable, partial after 10, full after 20, back after 120", stem && t0 && t1 && t2 && t3 && val(w.get(top), P_TILT) == V_NONE);
    }
    // ---- corals, vines, misc
    {
        let mut w = world();
        let a = p(5, 1, 5);
        w.set_block(a, st("tube_coral_block"), F_ALL);
        rules::set_placed_by(&mut w, a, st("tube_coral_block"), &ctx(a));
        ticks(&mut w, 59);
        let alive = name(&w, a) == "tube_coral_block";
        ticks(&mut w, 41);
        ty.check("coral block: dies 60-100 ticks after being placed out of water", alive && name(&w, a) == "dead_tube_coral_block");
        let mut w = world();
        let a = p(5, 3, 5);
        w.set_block(a, st("stone"), F_ALL);
        let vine = with_bool(st("vine"), P_WEST, true);
        w.set_block(a.rel(EAST), vine, F_ALL);
        w.set_block(a.rel(EAST).below(), vine, F_ALL);
        w.set_block(a, 0, F_ALL);
        ty.check("vine: a vine on a block and the one hanging below it both go with the block", w.get(a.rel(EAST)) == 0 && w.get(a.rel(EAST).below()) == 0);
        let a = p(5, 1, 5);
        let mut w = world();
        w.set_block(a, st("stone"), F_ALL);
        let lichen = with_bool(with_bool(st("glow_lichen"), P_WEST, true), P_DOWN, true);
        w.set_block(a.rel(EAST), lichen, F_ALL);
        w.set_block(a, 0, F_ALL);
        let s = w.get(a.rel(EAST));
        ty.check("glow lichen: loses the face whose block went, keeps the other", !is_true(s, P_WEST) && is_true(s, P_DOWN));
        let mut w = world();
        w.set_block(a, st("redstone_ore"), F_ALL);
        { let s = w.get(a); rules::attack(&mut w, s, a); }
        ty.check("redstone ore: lights up when hit", is_true(w.get(a), P_LIT));
        let mut w = world();
        w.set_block(a, st("farmland"), F_ALL);
        w.set_block(a.above(), st("stone"), F_ALL);
        ticks(&mut w, 2);
        ty.check_survive("farmland: turns to dirt under a solid block", name(&w, a) == "dirt");
        let mut w = world();
        w.set_block(a, st("snow_block"), F_ALL);
        w.set_block(a.above(), st("snow_block"), F_ALL);
        w.set_block(a.above().above(), st("carved_pumpkin"), F_ALL);
        ty.check("carved pumpkin: on two snow blocks the pattern is cleared (golem unmodelled)", w.get(a) == 0 && w.get(a.above()) == 0 && w.get(a.above().above()) == 0 && w.unmodelled == 1);
    }

    println!("basic_check: {} passed, {} failed, {} pending on other modules", ty.pass, ty.fail, ty.pending);
    if ty.fail > 0 { std::process::exit(1); }
}
