//! Checks of the placement rules (sim/src/rules/place.rs):
//!  * face_sturdy(s, UP, FULL) against the game's own `sturdyTop` column, and against shapes.rs' face_support,
//!  * which blocks the bot can place by hand (the Java version's `BuildState.spec(...).placeable`),
//!  * spot checks of single clicks that can be reasoned out by hand.
use mcsim::geom::*;
use mcsim::kinds::Kind;
use mcsim::oracle::{Oracle, PROBE_POS};
use mcsim::props::*;
use mcsim::rules::{self, place, PlaceCtx};
use mcsim::tables::*;
use mcsim::world::World;
use std::collections::BTreeSet;

/// The blocks real runs of the Java bot reported as "cannot be placed by hand".
const JAVA_UNPLACEABLE: &[&str] = &[
    "jigsaw", "fire", "lily_pad", "vine", "sugar_cane", "command_block", "soul_fire", "glow_lichen", "brown_mushroom", "red_mushroom", "cave_vines_plant",
    "structure_block", "repeating_command_block", "weeping_vines_plant", "twisting_vines_plant", "small_dripleaf", "sculk_vein",
];

/// A few of the ordinary blocks that must be placeable.
const MUST_PLACE: &[&str] = &[
    "stone", "oak_stairs", "oak_slab", "oak_door", "iron_door", "torch", "wall_torch", "redstone_torch", "redstone_wall_torch", "oak_sign", "oak_wall_sign",
    "oak_hanging_sign", "oak_wall_hanging_sign", "ladder", "rail", "powered_rail", "detector_rail", "redstone_wire", "repeater", "comparator", "lever",
    "stone_button", "oak_button", "oak_trapdoor", "oak_fence", "oak_fence_gate", "cobblestone_wall", "glass_pane", "iron_bars", "chest", "trapped_chest",
    "white_bed", "dandelion", "poppy", "oak_sapling", "wheat", "carrots", "potatoes", "beetroots", "cactus", "nether_wart", "cocoa", "lantern", "soul_lantern",
    "iron_chain", "white_carpet", "snow", "white_banner", "white_wall_banner", "skeleton_skull", "skeleton_wall_skull", "player_head", "hopper", "piston",
    "sticky_piston", "observer", "dispenser", "dropper", "furnace", "oak_log", "oak_leaves", "anvil", "bell", "campfire", "end_rod", "lightning_rod", "candle",
    "sea_pickle", "turtle_egg", "pink_petals", "leaf_litter", "scaffolding", "bamboo", "tall_grass", "sunflower", "short_grass", "fern", "dead_bush",
    "sweet_berry_bush", "melon_stem", "pumpkin_stem", "chorus_plant", "chorus_flower", "pointed_dripstone", "amethyst_cluster", "tripwire", "tripwire_hook",
    "stone_pressure_plate", "light_weighted_pressure_plate", "farmland", "dirt_path", "grass_block", "white_glazed_terracotta", "shulker_box", "decorated_pot",
    "crafter", "heavy_core", "hanging_roots", "pale_hanging_moss", "pale_moss_carpet", "moss_carpet", "weeping_vines", "twisting_vines", "cave_vines",
    "big_dripleaf", "azalea", "spore_blossom", "flower_pot", "potted_poppy", "cauldron", "cake", "note_block", "redstone_lamp", "target", "daylight_detector",
    "brewing_stand", "enchanting_table", "ender_chest", "barrel", "lectern", "grindstone", "stonecutter", "loom", "composter", "beehive", "jukebox", "tnt",
    "sand", "gravel", "white_concrete_powder", "tube_coral_block", "dead_tube_coral", "dead_tube_coral_fan", "dead_tube_coral_wall_fan", "tube_coral",
    "tube_coral_fan", "tube_coral_wall_fan", "crimson_fungus", "warped_roots", "nether_sprouts", "torchflower", "wither_rose", "copper_bulb", "oak_shelf",
    "copper_chest", "copper_golem_statue", "calibrated_sculk_sensor", "sculk_sensor", "sculk_shrieker", "mangrove_propagule", "mangrove_roots", "mud",
    "soul_sand", "ice", "glass", "powder_snow", "cobweb", "sponge", "end_portal_frame", "dragon_egg", "conduit", "beacon", "respawn_anchor", "smoker",
    "blast_furnace", "chiseled_bookshelf", "copper_torch", "copper_wall_torch", "soul_torch", "soul_wall_torch", "resin_brick_wall", "creaking_heart",
    "dried_ghast", "cactus_flower", "firefly_bush", "bush", "wildflowers", "torchflower_crop", "pitcher_crop", "pitcher_plant", "large_fern", "rose_bush",
];

fn ctx(w: &World, item: &str, hit_pos: P, face: Dir, a: f64, b: f64, yaw: f32, pitch: f32, sneak: bool) -> PlaceCtx {
    let item = *t().item_by_name.get(item).unwrap_or_else(|| panic!("no item {item}"));
    let (x, y, z) = (hit_pos.x as f64, hit_pos.y as f64, hit_pos.z as f64);
    // (a, b): position on the face; b is the height on side faces
    let hit = match face {
        DOWN => [x + a, y, z + b],
        UP => [x + a, y + 1.0, z + b],
        NORTH => [x + a, y + b, z],
        SOUTH => [x + a, y + b, z + 1.0],
        WEST => [x, y + b, z + a],
        _ => [x + 1.0, y + b, z + a],
    };
    PlaceCtx { item, hit_pos, face, hit, inside: false, yaw, pitch, sneak, player_pos: PROBE_POS, replace_clicked: true }.resolve(w)
}

fn click(w: &World, item: &str, hit_pos: P, face: Dir, a: f64, b: f64, yaw: f32, pitch: f32, sneak: bool) -> String {
    let c = ctx(w, item, hit_pos, face, a, b, yaw, pitch, sneak);
    match rules::predict_place(w, &c) {
        Some((pos, st)) => format!("{} @({},{},{})", t().name(st), pos.x - hit_pos.x, pos.y - hit_pos.y, pos.z - hit_pos.z),
        None => "nothing".to_string(),
    }
}

struct Checks {
    fails: usize,
    n: usize,
}
impl Checks {
    fn expect(&mut self, what: &str, got: String, want: &str) {
        self.n += 1;
        let ok = got == want;
        if !ok { self.fails += 1; }
        println!("  [{}] {what}: {got}{}", if ok { "ok" } else { "FAIL" }, if ok { String::new() } else { format!("   (expected {want})") });
    }
}

const YAW_S: f32 = 0.0;
const YAW_W: f32 = 90.0;
const YAW_N: f32 = 180.0;
const YAW_E: f32 = -90.0;

fn main() {
    let t0 = std::time::Instant::now();
    init("/a/reference", "/a/data");
    let tb = t();

    // ------------------------------------------------------------------ 1. sturdy top
    let mut bad = 0;
    let mut shown = BTreeSet::new();
    for s in 0..tb.n {
        let st = rules::face_sturdy(s as S, UP, 0);
        if st != tb.sturdy_top[s] {
            bad += 1;
            shown.insert(tb.blocks[tb.block[s] as usize].name.clone());
        }
    }
    println!("== sturdyTop: {bad} states where face_sturdy(s, UP, FULL) differs from the game's table {:?}", shown.iter().take(20).collect::<Vec<_>>());
    // the same test through shapes.rs (collision shapes only, i.e. without the support shape overrides)
    let (mut diff_full, mut diff_center, mut diff_rigid) = (0, 0, 0);
    let mut diff_names = BTreeSet::new();
    for s in 0..tb.n {
        if !matches!(
            kind_of(s as S),
            Kind::LeavesBlock | Kind::MudBlock | Kind::SoulSandBlock | Kind::ShulkerBoxBlock | Kind::SnowLayerBlock | Kind::ChorusFlowerBlock | Kind::FenceGateBlock
                | Kind::CeilingHangingSignBlock | Kind::WallHangingSignBlock
        ) {
            let sh = &tb.shapes[tb.col_shape[s] as usize];
            for d in DIRS {
                for k in 0..3u8 {
                    if sh.face_support(d, k) != place::face_sturdy(s as S, d, k) {
                        match k { 0 => diff_full += 1, 1 => diff_center += 1, _ => diff_rigid += 1 }
                        diff_names.insert(format!("{}:{}:{}", tb.blocks[tb.block[s] as usize].name, d, k));
                    }
                }
            }
        }
    }
    println!(
        "== place::face_sturdy vs shapes.rs VoxShape::face_support on plain collision shapes: FULL {diff_full}, CENTER {diff_center}, RIGID {diff_rigid} differences {:?}",
        diff_names.iter().take(12).collect::<Vec<_>>()
    );
    let solid = (0..tb.n).filter(|s| place::is_solid(*s as S)).count();
    println!("== is_solid: {solid} of {} states", tb.n);
    // BlockState.isSolid of default states, as the rule in BlockStateBase.calculateSolid gives it by hand
    let solid_want: &[(&str, bool)] = &[
        ("stone", true), ("glass", true), ("oak_leaves", true), ("oak_slab", true), ("white_carpet", false), ("snow", false), ("oak_sign", true), ("torch", false),
        ("ladder", false), ("chest", true), ("hopper", true), ("cake", true), ("oak_fence_gate", true), ("oak_door", true), ("oak_trapdoor", true), ("lantern", true),
        ("iron_chain", true), ("end_rod", false), ("cobweb", true), ("bamboo", true), ("scaffolding", false), ("powder_snow", false), ("flower_pot", false),
        ("anvil", true), ("redstone_wire", false), ("repeater", false), ("daylight_detector", true), ("cactus", true), ("azalea", false), ("big_dripleaf", false),
        ("pointed_dripstone", true), ("turtle_egg", true), ("sea_pickle", false), ("candle", false), ("skeleton_skull", false), ("white_bed", true),
        ("farmland", true), ("soul_sand", true), ("lily_pad", false), ("white_banner", true), ("stone_pressure_plate", true), ("stone_button", false),
        ("lever", false), ("rail", false), ("vine", false), ("glow_lichen", false), ("sculk_vein", true), ("chorus_plant", false), ("shulker_box", true),
        ("dead_tube_coral_fan", true), ("tube_coral_fan", false), ("oak_fence", true), ("cobblestone_wall", true), ("bell", true), ("amethyst_cluster", true),
    ];
    let solid_bad: Vec<&str> = solid_want.iter().filter(|(n, v)| place::is_solid(tb.default_of(n)) != *v).map(|x| x.0).collect();
    println!("== is_solid samples: {} of {} differ from the hand-derived values {:?}", solid_bad.len(), solid_want.len(), solid_bad);
    let cond_want: &[(&str, bool)] = &[("stone", true), ("glass", false), ("oak_leaves", false), ("redstone_block", false), ("glowstone", false), ("piston", false),
        ("soul_sand", true), ("mud", true), ("oak_slab", false), ("observer", false), ("tnt", false), ("ice", false), ("packed_ice", true), ("dirt", true), ("copper_bulb", false)];
    let cond_bad: Vec<&str> = cond_want.iter().filter(|(n, v)| place::is_redstone_conductor(tb.default_of(n)) != *v).map(|x| x.0).collect();
    println!("== is_redstone_conductor samples: {} of {} differ {:?}", cond_bad.len(), cond_want.len(), cond_bad);

    // ------------------------------------------------------------------ 2. spot checks
    println!("== spot checks");
    let mut ck = Checks { fails: 0, n: 0 };
    let c = p(4, 4, 4);
    let fresh = || World::new(0, 0, 0, 9, 9, 9, -1000, 100000);
    let stone = tb.default_of("stone");
    {
        let mut w = fresh();
        w.put_raw(c, stone);
        // stairs: side face, upper half, facing north
        ck.expect("stair on top half of a side face, facing north", click(&w, "oak_stairs", c, SOUTH, 0.5, 0.75, YAW_N, 0.0, false), "oak_stairs[facing=north,half=top,shape=straight,waterlogged=false] @(0,0,1)");
        ck.expect("stair on lower half of a side face, facing east", click(&w, "oak_stairs", c, WEST, 0.5, 0.25, YAW_E, 0.0, false), "oak_stairs[facing=east,half=bottom,shape=straight,waterlogged=false] @(-1,0,0)");
        ck.expect("stair under a block", click(&w, "oak_stairs", c, DOWN, 0.5, 0.5, YAW_W, -60.0, false), "oak_stairs[facing=west,half=top,shape=straight,waterlogged=false] @(0,-1,0)");
        ck.expect("slab on upper half of a side face", click(&w, "oak_slab", c, NORTH, 0.5, 0.75, YAW_S, 0.0, false), "oak_slab[type=top,waterlogged=false] @(0,0,-1)");
        ck.expect("slab on lower half of a side face", click(&w, "oak_slab", c, NORTH, 0.5, 0.25, YAW_S, 0.0, false), "oak_slab[type=bottom,waterlogged=false] @(0,0,-1)");
        ck.expect("slab on top", click(&w, "oak_slab", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "oak_slab[type=bottom,waterlogged=false] @(0,1,0)");
        ck.expect("wall torch on a north face", click(&w, "torch", c, NORTH, 0.5, 0.5, YAW_S, 0.0, false), "wall_torch[facing=north] @(0,0,-1)");
        ck.expect("torch on top", click(&w, "torch", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "torch @(0,1,0)");
        ck.expect("torch under a block (no support)", click(&w, "torch", c, DOWN, 0.5, 0.5, YAW_S, -60.0, false), "nothing");
        ck.expect("redstone wall torch on an east face", click(&w, "redstone_torch", c, EAST, 0.5, 0.5, YAW_W, 0.0, false), "redstone_wall_torch[facing=east,lit=true] @(1,0,0)");
        ck.expect("log on an east face", click(&w, "oak_log", c, EAST, 0.5, 0.5, YAW_W, 0.0, false), "oak_log[axis=x] @(1,0,0)");
        ck.expect("log on a north face", click(&w, "oak_log", c, NORTH, 0.5, 0.5, YAW_S, 0.0, false), "oak_log[axis=z] @(0,0,-1)");
        ck.expect("log on top", click(&w, "oak_log", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "oak_log[axis=y] @(0,1,0)");
        ck.expect("ladder on a west face", click(&w, "ladder", c, WEST, 0.5, 0.5, YAW_E, 0.0, false), "ladder[facing=west,waterlogged=false] @(-1,0,0)");
        ck.expect("lever on the floor, facing north", click(&w, "lever", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "lever[face=floor,facing=north,powered=false] @(0,1,0)");
        ck.expect("button on a south face", click(&w, "stone_button", c, SOUTH, 0.5, 0.5, YAW_N, 0.0, false), "stone_button[face=wall,facing=south,powered=false] @(0,0,1)");
        ck.expect("button on the ceiling, facing east", click(&w, "stone_button", c, DOWN, 0.5, 0.5, YAW_E, -60.0, false), "stone_button[face=ceiling,facing=east,powered=false] @(0,-1,0)");
        ck.expect("trapdoor on upper half of a side face", click(&w, "oak_trapdoor", c, NORTH, 0.5, 0.75, YAW_S, 0.0, false), "oak_trapdoor[facing=north,half=top,open=false,powered=false,waterlogged=false] @(0,0,-1)");
        ck.expect("trapdoor on top, facing south", click(&w, "oak_trapdoor", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "oak_trapdoor[facing=north,half=bottom,open=false,powered=false,waterlogged=false] @(0,1,0)");
        ck.expect("repeater facing north", click(&w, "repeater", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "repeater[delay=1,facing=south,locked=false,powered=false] @(0,1,0)");
        ck.expect("lone redstone dust", click(&w, "redstone", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "redstone_wire[east=side,north=side,power=0,south=side,west=side] @(0,1,0)");
        ck.expect("rail facing east", click(&w, "rail", c, UP, 0.5, 0.5, YAW_E, 60.0, false), "rail[shape=east_west,waterlogged=false] @(0,1,0)");
        ck.expect("door facing north, clicked left part", click(&w, "oak_door", c, UP, 0.25, 0.5, YAW_N, 60.0, false), "oak_door[facing=north,half=lower,hinge=left,open=false,powered=false] @(0,1,0)");
        ck.expect("door facing north, clicked right part", click(&w, "oak_door", c, UP, 0.75, 0.5, YAW_N, 60.0, false), "oak_door[facing=north,half=lower,hinge=right,open=false,powered=false] @(0,1,0)");
        ck.expect("chest facing the player", click(&w, "chest", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "chest[facing=south,type=single,waterlogged=false] @(0,1,0)");
        ck.expect("furnace facing the player", click(&w, "furnace", c, UP, 0.5, 0.5, YAW_E, 60.0, false), "furnace[facing=west,lit=false] @(0,1,0)");
        ck.expect("piston looking down", click(&w, "piston", c, UP, 0.5, 0.5, YAW_E, 80.0, false), "piston[extended=false,facing=up] @(0,1,0)");
        ck.expect("observer looking north", click(&w, "observer", c, UP, 0.5, 0.5, YAW_N, 0.0, false), "observer[facing=north,powered=false] @(0,1,0)");
        ck.expect("hopper on a side face", click(&w, "hopper", c, WEST, 0.5, 0.5, YAW_E, 0.0, false), "hopper[enabled=true,facing=east] @(-1,0,0)");
        ck.expect("hopper on top", click(&w, "hopper", c, UP, 0.5, 0.5, YAW_E, 60.0, false), "hopper[enabled=true,facing=down] @(0,1,0)");
        ck.expect("lantern on top", click(&w, "lantern", c, UP, 0.5, 0.5, YAW_E, 60.0, false), "lantern[hanging=false,waterlogged=false] @(0,1,0)");
        ck.expect("lantern under a block", click(&w, "lantern", c, DOWN, 0.5, 0.5, YAW_E, -60.0, false), "lantern[hanging=true,waterlogged=false] @(0,-1,0)");
        ck.expect("chain on a side face", click(&w, "iron_chain", c, SOUTH, 0.5, 0.5, YAW_N, 0.0, false), "iron_chain[axis=z,waterlogged=false] @(0,0,1)");
        ck.expect("sign on top, facing north", click(&w, "oak_sign", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "oak_sign[rotation=0,waterlogged=false] @(0,1,0)");
        ck.expect("sign on top, facing east", click(&w, "oak_sign", c, UP, 0.5, 0.5, YAW_E, 60.0, false), "oak_sign[rotation=4,waterlogged=false] @(0,1,0)");
        ck.expect("wall sign on a west face", click(&w, "oak_sign", c, WEST, 0.5, 0.5, YAW_E, 0.0, false), "oak_wall_sign[facing=west,waterlogged=false] @(-1,0,0)");
        ck.expect("banner on top, facing south", click(&w, "white_banner", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "white_banner[rotation=8] @(0,1,0)");
        ck.expect("skull on top, facing south", click(&w, "skeleton_skull", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "skeleton_skull[powered=false,rotation=0] @(0,1,0)");
        ck.expect("wall skull on a north face", click(&w, "skeleton_skull", c, NORTH, 0.5, 0.5, YAW_S, 0.0, false), "skeleton_wall_skull[facing=north,powered=false] @(0,0,-1)");
        ck.expect("hanging sign under a block, facing north", click(&w, "oak_hanging_sign", c, DOWN, 0.5, 0.5, YAW_N, -60.0, false), "oak_hanging_sign[attached=false,rotation=0,waterlogged=false] @(0,-1,0)");
        ck.expect("wall hanging sign on an east face, facing north", click(&w, "oak_hanging_sign", c, EAST, 0.5, 0.5, YAW_N, 0.0, false), "oak_wall_hanging_sign[facing=south,waterlogged=false] @(1,0,0)");
        ck.expect("bed facing north", click(&w, "white_bed", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "white_bed[facing=north,occupied=false,part=foot] @(0,1,0)");
        ck.expect("carpet", click(&w, "white_carpet", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "white_carpet @(0,1,0)");
        ck.expect("snow", click(&w, "snow", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "snow[layers=1] @(0,1,0)");
        ck.expect("fence, no neighbours but the block below", click(&w, "oak_fence", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "oak_fence[east=false,north=false,south=false,waterlogged=false,west=false] @(0,1,0)");
        ck.expect("fence next to a full block", click(&w, "oak_fence", c, EAST, 0.5, 0.5, YAW_W, 0.0, false), "oak_fence[east=false,north=false,south=false,waterlogged=false,west=true] @(1,0,0)");
        ck.expect("wall next to a full block", click(&w, "cobblestone_wall", c, EAST, 0.5, 0.5, YAW_W, 0.0, false), "cobblestone_wall[east=none,north=none,south=none,up=true,waterlogged=false,west=low] @(1,0,0)");
        ck.expect("pane next to a full block", click(&w, "glass_pane", c, NORTH, 0.5, 0.5, YAW_S, 0.0, false), "glass_pane[east=false,north=false,south=true,waterlogged=false,west=false] @(0,0,-1)");
        ck.expect("fence gate facing east", click(&w, "oak_fence_gate", c, UP, 0.5, 0.5, YAW_E, 60.0, false), "oak_fence_gate[facing=east,in_wall=false,open=false,powered=false] @(0,1,0)");
        ck.expect("scaffolding on top", click(&w, "scaffolding", c, UP, 0.5, 0.5, YAW_E, 60.0, false), "scaffolding[bottom=false,distance=0,waterlogged=false] @(0,1,0)");
        ck.expect("scaffolding on a side (distance 7: refused)", click(&w, "scaffolding", c, EAST, 0.5, 0.5, YAW_W, 0.0, false), "nothing");
        ck.expect("end rod on a south face", click(&w, "end_rod", c, SOUTH, 0.5, 0.5, YAW_N, 0.0, false), "end_rod[facing=south] @(0,0,1)");
        ck.expect("anvil facing north", click(&w, "anvil", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "anvil[facing=east] @(0,1,0)");
        ck.expect("bell on top", click(&w, "bell", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "bell[attachment=floor,facing=north,powered=false] @(0,1,0)");
        ck.expect("bell on a side", click(&w, "bell", c, EAST, 0.5, 0.5, YAW_W, 0.0, false), "bell[attachment=single_wall,facing=west,powered=false] @(1,0,0)");
        ck.expect("glow lichen on a north face", click(&w, "glow_lichen", c, NORTH, 0.5, 0.5, YAW_S, 0.0, false), "glow_lichen[down=false,east=false,north=false,south=true,up=false,waterlogged=false,west=false] @(0,0,-1)");
        ck.expect("vine on a north face", click(&w, "vine", c, NORTH, 0.5, 0.5, YAW_S, 0.0, false), "vine[east=false,north=false,south=true,up=false,west=false] @(0,0,-1)");
        ck.expect("flower on stone", click(&w, "poppy", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "nothing");
        ck.expect("mushroom on stone (light 15)", click(&w, "red_mushroom", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "nothing");
        ck.expect("lily pad on stone", click(&w, "lily_pad", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "nothing");
        ck.expect("command block (not an operator)", click(&w, "command_block", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "nothing");
        ck.expect("pointed dripstone under a block", click(&w, "pointed_dripstone", c, DOWN, 0.5, 0.5, YAW_S, -60.0, false), "pointed_dripstone[thickness=tip,vertical_direction=down,waterlogged=false] @(0,-1,0)");
        ck.expect("crafter looking down, facing north", click(&w, "crafter", c, UP, 0.5, 0.5, YAW_N, 80.0, false), "crafter[crafting=false,orientation=up_north,triggered=false] @(0,1,0)");
        ck.expect("leaves next to nothing", click(&w, "oak_leaves", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "oak_leaves[distance=7,persistent=true,waterlogged=false] @(0,1,0)");
        ck.expect("campfire", click(&w, "campfire", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "campfire[facing=north,lit=true,signal_fire=false,waterlogged=false] @(0,1,0)");
        ck.expect("candle", click(&w, "candle", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "candle[candles=1,lit=false,waterlogged=false] @(0,1,0)");
        ck.expect("amethyst cluster on a west face", click(&w, "amethyst_cluster", c, WEST, 0.5, 0.5, YAW_E, 0.0, false), "amethyst_cluster[facing=west,waterlogged=false] @(-1,0,0)");
        ck.expect("tripwire hook on a north face", click(&w, "tripwire_hook", c, NORTH, 0.5, 0.5, YAW_S, 0.0, false), "tripwire_hook[attached=false,facing=north,powered=false] @(0,0,-1)");
    }
    {
        // things that depend on what is already there
        let mut w = fresh();
        w.put_raw(c, stone);
        w.put_raw(c.above(), with(tb.default_of("oak_slab"), P_TYPE, V_BOTTOM));
        ck.expect("second slab onto a bottom slab", click(&w, "oak_slab", c.above(), UP, 0.5, 0.5, YAW_S, 60.0, false), "oak_slab[type=double,waterlogged=false] @(0,0,0)");
        ck.expect("other slab onto a bottom slab goes above", click(&w, "stone_slab", c.above(), UP, 0.5, 0.5, YAW_S, 60.0, false), "stone_slab[type=bottom,waterlogged=false] @(0,1,0)");
        w.put_raw(c.above(), tb.default_of("snow"));
        ck.expect("snow onto snow", click(&w, "snow", c.above(), UP, 0.5, 0.5, YAW_S, 60.0, false), "snow[layers=2] @(0,0,0)");
        ck.expect("stone into one snow layer", click(&w, "stone", c.above(), UP, 0.5, 0.5, YAW_S, 60.0, false), "stone @(0,0,0)");
        w.put_raw(c.above(), tb.default_of("candle"));
        ck.expect("candle onto a candle", click(&w, "candle", c.above(), UP, 0.5, 0.5, YAW_S, 60.0, false), "candle[candles=2,lit=false,waterlogged=false] @(0,0,0)");
        w.put_raw(c.above(), tb.default_of("short_grass"));
        ck.expect("stone into grass", click(&w, "stone", c.above(), UP, 0.5, 0.5, YAW_S, 60.0, false), "stone @(0,0,0)");
        // a chest next to a chest
        w.put_raw(c.above(), with(tb.default_of("chest"), P_FACING, V_SOUTH));
        w.put_raw(c.rel(EAST), stone);
        ck.expect("chest next to a chest joins it", click(&w, "chest", c.rel(EAST), UP, 0.5, 0.5, YAW_N, 60.0, false), "chest[facing=south,type=left,waterlogged=false] @(0,1,0)");
        ck.expect("chest next to a chest, sneaking, stays single", click(&w, "chest", c.rel(EAST), UP, 0.5, 0.5, YAW_N, 60.0, true), "chest[facing=south,type=single,waterlogged=false] @(0,1,0)");
        // stairs forming a corner
        w.put_raw(c.above(), with(tb.default_of("oak_stairs"), P_FACING, V_EAST));
        ck.expect("stair in front of a turned stair makes an outer corner", click(&w, "oak_stairs", c.above(), SOUTH, 0.5, 0.25, YAW_N, 0.0, false), "oak_stairs[facing=north,half=bottom,shape=outer_right,waterlogged=false] @(0,0,1)");
        // scaffolding on scaffolding
        w.put_raw(c.above(), with(with_bool(tb.default_of("scaffolding"), P_BOTTOM, false), P_DISTANCE, 0));
        ck.expect("scaffolding clicked on scaffolding goes on top", click(&w, "scaffolding", c.above(), NORTH, 0.5, 0.5, YAW_S, 0.0, false), "scaffolding[bottom=false,distance=0,waterlogged=false] @(0,1,0)");
        ck.expect("scaffolding clicked on top of scaffolding goes sideways", click(&w, "scaffolding", c.above(), UP, 0.5, 0.5, YAW_W, 60.0, false), "scaffolding[bottom=true,distance=1,waterlogged=false] @(-1,0,0)");
        // redstone dust next to dust
        w.put_raw(c.above(), tb.default_of("redstone_wire"));
        ck.expect("dust next to dust", click(&w, "redstone", c.rel(EAST), UP, 0.5, 0.5, YAW_N, 60.0, false), "redstone_wire[east=side,north=none,power=0,south=none,west=side] @(0,1,0)");
        // door next to a door: hinge on the other side
        w.put_raw(c.above(), with(tb.default_of("oak_door"), P_FACING, V_NORTH));
        ck.expect("door right of a door mirrors its hinge", click(&w, "oak_door", c.rel(EAST), UP, 0.25, 0.5, YAW_N, 60.0, false), "oak_door[facing=north,half=lower,hinge=right,open=false,powered=false] @(0,1,0)");
    }
    {
        // plants on their soil
        let soils = [("dirt", "oak_sapling", "oak_sapling[stage=0]"), ("dirt", "poppy", "poppy"), ("farmland", "wheat_seeds", "wheat[age=0]"),
            ("farmland", "carrot", "carrots[age=0]"), ("sand", "cactus", "cactus[age=0]"), ("soul_sand", "nether_wart", "nether_wart[age=0]"),
            ("dirt", "bamboo", "bamboo_sapling"), ("dirt", "sweet_berries", "sweet_berry_bush[age=0]"), ("sand", "dead_bush", "dead_bush"),
            ("end_stone", "chorus_flower", "chorus_flower[age=0]"), ("mycelium", "red_mushroom", "red_mushroom"), ("dirt", "sugar_cane", "nothing"),
            ("clay", "small_dripleaf", "small_dripleaf[facing=north,half=lower,waterlogged=false]"), ("ice", "lily_pad", "nothing"),
            ("dirt", "sunflower", "sunflower[half=lower]"), ("farmland", "melon_seeds", "melon_stem[age=0]")];
        for (soil, item, want) in soils {
            let mut w = fresh();
            w.put_raw(c, tb.default_of(soil));
            let got = click(&w, item, c, UP, 0.5, 0.5, YAW_S, 60.0, false);
            let want = if want == "nothing" { want.to_string() } else { format!("{want} @(0,1,0)") };
            ck.expect(&format!("{item} on {soil}"), got, &want);
        }
        let mut w = fresh();
        w.put_raw(c, tb.default_of("jungle_log"));
        ck.expect("cocoa on a jungle log", click(&w, "cocoa_beans", c, NORTH, 0.5, 0.5, YAW_S, 0.0, false), "cocoa[age=0,facing=south] @(0,0,-1)");
        w.put_raw(c, tb.default_of("ice"));
        let ray = ctx(&w, "lily_pad", c, UP, 0.5, 0.5, YAW_S, 60.0, false);
        let got = match place::predict_place_on_water(&w, &ray) { Some((q, st)) => format!("{} @({},{},{})", tb.name(st), q.x - c.x, q.y - c.y, q.z - c.z), None => "nothing".into() };
        ck.expect("lily pad through Item.use on ice", got, "lily_pad @(0,1,0)");
    }
    {
        // neighbours
        let mut w = fresh();
        w.put_raw(c, stone);
        w.put_raw(c.off(1, 1, 0), stone);
        ck.expect("wall under a block is tall", click(&w, "cobblestone_wall", c, EAST, 0.5, 0.5, YAW_W, 0.0, false), "cobblestone_wall[east=none,north=none,south=none,up=true,waterlogged=false,west=tall] @(1,0,0)");
        w.put_raw(c.off(1, 1, 0), 0);
        w.put_raw(c.off(2, 0, 0), stone);
        ck.expect("wall between two blocks has no post", click(&w, "cobblestone_wall", c, EAST, 0.5, 0.5, YAW_W, 0.0, false), "cobblestone_wall[east=low,north=none,south=none,up=false,waterlogged=false,west=low] @(1,0,0)");
        ck.expect("bell between two blocks", click(&w, "bell", c, EAST, 0.5, 0.5, YAW_W, 0.0, false), "bell[attachment=double_wall,facing=west,powered=false] @(1,0,0)");
        w.put_raw(c.off(-1, 1, 0), tb.default_of("cobblestone_wall"));
        ck.expect("fence gate next to a wall", click(&w, "oak_fence_gate", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "oak_fence_gate[facing=north,in_wall=true,open=false,powered=false] @(0,1,0)");
        ck.expect("iron bars next to a wall", click(&w, "iron_bars", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "iron_bars[east=false,north=false,south=false,waterlogged=false,west=true] @(0,1,0)");
        w.put_raw(c.off(-1, 1, 0), 0);
        w.put_raw(c.above(), with(tb.default_of("chest"), P_FACING, V_SOUTH));
        ck.expect("chest sneak-clicked onto a chest's side joins it", click(&w, "chest", c.above(), EAST, 0.5, 0.5, YAW_E, 0.0, true), "chest[facing=south,type=left,waterlogged=false] @(1,0,0)");
        w.put_raw(c.above(), with(tb.default_of("exposed_copper_chest"), P_FACING, V_SOUTH));
        w.put_raw(c.off(1, 0, 0), stone);
        ck.expect("waxed copper chest joining an exposed one", click(&w, "waxed_copper_chest", c.off(1, 0, 0), UP, 0.5, 0.5, YAW_N, 60.0, false), "copper_chest[facing=south,type=left,waterlogged=false] @(0,1,0)");
        w.put_raw(c.above(), with(tb.default_of("oak_stairs"), P_FACING, V_EAST));
        ck.expect("stair behind a turned stair makes an inner corner", click(&w, "oak_stairs", c.above(), NORTH, 0.5, 0.25, YAW_N, 0.0, false), "oak_stairs[facing=north,half=bottom,shape=inner_right,waterlogged=false] @(0,0,-1)");
        w.put_raw(c.above(), 0);
        w.put_raw(c.off(0, 0, -1), with(tb.default_of("ladder"), P_FACING, V_NORTH));
        ck.expect("ladder onto the front of a ladder", click(&w, "ladder", c.off(0, 0, -1), NORTH, 0.5, 0.5, YAW_S, 0.0, false), "nothing");
        w.put_raw(c.off(0, 0, -1), 0);
        w.put_raw(c.off(-1, 1, 0), with(with_bool(tb.default_of("repeater"), P_POWERED, true), P_FACING, V_WEST));
        ck.expect("repeater next to a powered repeater pointing at it", click(&w, "repeater", c, UP, 0.5, 0.5, YAW_N, 60.0, false), "repeater[delay=1,facing=south,locked=true,powered=false] @(0,1,0)");
        w.put_raw(c.off(-1, 1, 0), 0);
        w.put_raw(c.off(-1, -1, 0), stone);
        w.put_raw(c.above(), tb.default_of("redstone_wire"));
        ck.expect("dust below dust on a block climbs", click(&w, "redstone", c.off(-1, -1, 0), UP, 0.5, 0.5, YAW_N, 60.0, false), "redstone_wire[east=up,north=none,power=0,south=none,west=side] @(0,1,0)");
        w.put_raw(c.above(), stone);
        ck.expect("door next to a block hinges on it", click(&w, "oak_door", c.off(1, 0, 0), UP, 0.75, 0.5, YAW_N, 60.0, false), "oak_door[facing=north,half=lower,hinge=left,open=false,powered=false] @(0,1,0)");
        ck.expect("bed with a block where its head goes", click(&w, "white_bed", c.off(1, 0, 0), UP, 0.5, 0.5, YAW_W, 60.0, false), "nothing");
        w.put_raw(c.off(1, 2, 0), stone);
        ck.expect("door with a block in its upper half", click(&w, "oak_door", c.off(1, 0, 0), UP, 0.5, 0.5, YAW_N, 60.0, false), "nothing");
        ck.expect("note block on stone", click(&w, "note_block", c.above(), UP, 0.5, 0.5, YAW_W, 60.0, false), "note_block[instrument=basedrum,note=0,powered=false] @(0,1,0)");
        w.put_raw(c.above(), tb.default_of("oak_planks"));
        ck.expect("note block on planks", click(&w, "note_block", c.above(), UP, 0.5, 0.5, YAW_W, 60.0, false), "note_block[instrument=bass,note=0,powered=false] @(0,1,0)");
        w.put_raw(c.off(0, 3, 0), tb.default_of("zombie_head"));
        ck.expect("note block under a zombie head", click(&w, "note_block", c.above(), UP, 0.5, 0.5, YAW_W, 60.0, false), "note_block[instrument=zombie,note=0,powered=false] @(0,1,0)");
    }
    {
        let mut w = fresh();
        w.put_raw(c, stone);
        let q = c.off(2, 0, 2);
        w.put_raw(q, tb.default_of("iron_chain"));
        ck.expect("hanging sign under a chain", click(&w, "oak_hanging_sign", q, DOWN, 0.5, 0.5, YAW_N, -60.0, false), "oak_hanging_sign[attached=true,rotation=0,waterlogged=false] @(0,-1,0)");
        ck.expect("hanging sign under a chain, turned 45 degrees", click(&w, "oak_hanging_sign", q, DOWN, 0.5, 0.5, 45.0, -60.0, false), "oak_hanging_sign[attached=true,rotation=10,waterlogged=false] @(0,-1,0)");
        w.put_raw(c.above(), with(tb.default_of("pointed_dripstone"), P_VERTICAL_DIRECTION, V_UP));
        w.put_raw(c.off(0, 3, 0), stone);
        ck.expect("stalactite onto a stalagmite's tip merges", click(&w, "pointed_dripstone", c.off(0, 3, 0), DOWN, 0.5, 0.5, YAW_N, -60.0, false), "pointed_dripstone[thickness=tip_merge,vertical_direction=down,waterlogged=false] @(0,-1,0)");
        ck.expect("... but not while sneaking", click(&w, "pointed_dripstone", c.off(0, 3, 0), DOWN, 0.5, 0.5, YAW_N, -60.0, true), "pointed_dripstone[thickness=tip,vertical_direction=down,waterlogged=false] @(0,-1,0)");
        w.put_raw(c.above(), 0);
        w.put_raw(c.off(0, 3, 0), 0);
        w.put_raw(c.off(-1, 0, -1), stone);
        w.put_raw(c.off(0, 0, -1), with_bool(tb.default_of("vine"), P_SOUTH, true));
        ck.expect("second face onto a vine", click(&w, "vine", c.off(0, 0, -1), NORTH, 0.5, 0.5, YAW_W, 0.0, false), "vine[east=false,north=false,south=true,up=false,west=true] @(0,0,0)");
        ck.expect("stone into a vine", click(&w, "stone", c.off(0, 0, -1), NORTH, 0.5, 0.5, YAW_W, 0.0, false), "stone @(0,0,0)");
        w.put_raw(c.off(0, 0, -1), with_bool(tb.default_of("glow_lichen"), P_SOUTH, true));
        ck.expect("second face onto glow lichen", click(&w, "glow_lichen", c.off(0, 0, -1), NORTH, 0.5, 0.5, YAW_W, 0.0, false), "glow_lichen[down=false,east=false,north=false,south=true,up=false,waterlogged=false,west=true] @(0,0,0)");
        w.put_raw(c.off(0, 0, -1), 0);
        w.put_raw(c.off(1, -1, 0), stone);
        ck.expect("pale moss carpet next to a block", click(&w, "pale_moss_carpet", c, EAST, 0.5, 0.5, YAW_W, 0.0, false), "pale_moss_carpet[bottom=true,east=none,north=none,south=none,west=low] @(1,0,0)");
        w.put_raw(c.off(1, 1, 0), tb.default_of("snow"));
        ck.expect("grass block under snow", click(&w, "grass_block", c, EAST, 0.5, 0.5, YAW_W, 0.0, false), "grass_block[snowy=true] @(1,0,0)");
        w.put_raw(c.off(1, 1, 0), stone);
        ck.expect("farmland under a block", click(&w, "farmland", c, EAST, 0.5, 0.5, YAW_W, 0.0, false), "dirt @(1,0,0)");
        w.put_raw(c.off(1, 1, 0), with(tb.default_of("tripwire_hook"), P_FACING, V_WEST));
        ck.expect("string next to a hook", click(&w, "string", c, UP, 0.5, 0.5, YAW_W, 60.0, false), "tripwire[attached=false,disarmed=false,east=true,north=false,powered=false,south=false,west=false] @(0,1,0)");
        ck.expect("sea pickle", click(&w, "sea_pickle", c, UP, 0.5, 0.5, YAW_W, 60.0, false), "sea_pickle[pickles=1,waterlogged=false] @(0,1,0)");
        w.put_raw(c.above(), with_bool(tb.default_of("sea_pickle"), P_WATERLOGGED, false));
        ck.expect("sea pickle onto a sea pickle", click(&w, "sea_pickle", c.above(), UP, 0.5, 0.5, YAW_W, 60.0, false), "sea_pickle[pickles=2,waterlogged=false] @(0,0,0)");
        w.put_raw(c.above(), 0);
        w.put_raw(c.off(0, -2, 0), tb.default_of("weeping_vines"));
        ck.expect("weeping vines above weeping vines are the plant block", click(&w, "weeping_vines", c, DOWN, 0.5, 0.5, YAW_W, -60.0, false), "weeping_vines_plant @(0,-1,0)");
        w.put_raw(c.off(0, -2, 0), 0);
        let got = click(&w, "glow_berries", c, DOWN, 0.5, 0.5, YAW_W, -60.0, false);
        ck.expect("cave vines under a block (age is random)", got.split("age=").next().unwrap_or("").to_string() + &got[got.find(',').unwrap_or(0)..], "cave_vines[,berries=false] @(0,-1,0)");
        let got = click(&w, "twisting_vines", c, UP, 0.5, 0.5, YAW_W, 60.0, false);
        ck.expect("twisting vines on a block (age is random)", got.split("age=").next().unwrap_or("").to_string() + &got[got.find(']').unwrap_or(0)..], "twisting_vines[] @(0,1,0)");
    }
    {
        let mut w = fresh();
        w.put_raw(c, tb.default_of("dirt"));
        ck.expect("petals", click(&w, "pink_petals", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "pink_petals[facing=north,flower_amount=1] @(0,1,0)");
        w.put_raw(c.above(), with(tb.default_of("pink_petals"), P_FACING, V_EAST));
        ck.expect("petals onto petals", click(&w, "pink_petals", c.above(), UP, 0.5, 0.5, YAW_S, 60.0, false), "pink_petals[facing=east,flower_amount=2] @(0,0,0)");
        w.put_raw(c.above(), tb.default_of("bamboo_sapling"));
        ck.expect("bamboo onto a bamboo shoot", click(&w, "bamboo", c.above(), UP, 0.5, 0.5, YAW_S, 60.0, false), "bamboo[age=0,leaves=none,stage=0] @(0,1,0)");
        w.put_raw(c.above(), with(tb.default_of("big_dripleaf"), P_FACING, V_EAST));
        ck.expect("big dripleaf onto a big dripleaf", click(&w, "big_dripleaf", c.above(), UP, 0.5, 0.5, YAW_S, 60.0, false), "big_dripleaf[facing=east,tilt=none,waterlogged=false] @(0,1,0)");
        w.put_raw(c.above(), 0);
        w.put_raw(c.off(0, 2, 0), stone);
        ck.expect("tall flower with a block in its upper half", click(&w, "sunflower", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "nothing");
        w.put_raw(c, tb.default_of("oak_log"));
        ck.expect("leaves on a log", click(&w, "oak_leaves", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "oak_leaves[distance=1,persistent=true,waterlogged=false] @(0,1,0)");
        w.put_raw(c, tb.default_of("red_mushroom_block"));
        ck.expect("mushroom block on a mushroom block", click(&w, "red_mushroom_block", c, UP, 0.5, 0.5, YAW_S, 60.0, false), "red_mushroom_block[down=false,east=true,north=true,south=true,up=true,west=true] @(0,1,0)");
    }
    println!("== spot checks: {} of {} failed", ck.fails, ck.n);

    // ------------------------------------------------------------------ 3. placeable by hand
    let mut oracle = Oracle::new();
    let mut unplaceable: Vec<(String, &str)> = Vec::new();
    let mut n_item = 0;
    let mut n_placeable = 0;
    let pot = tb.default_of("flower_pot");
    for b in &tb.blocks {
        let st = b.default;
        let item = tb.item[st as usize];
        if item == 0 || tb.item_count[st as usize] == 0 {
            unplaceable.push((b.name.clone(), "no item"));
            continue;
        }
        n_item += 1;
        // BuildState.spec: the first click's state
        let mut first = st;
        if tb.item_count[st as usize] > 1 {
            first = try_with(first, P_TYPE, V_BOTTOM);
            for p in [P_LAYERS, P_CANDLES, P_PICKLES, P_EGGS, P_FLOWER_AMOUNT, P_SEGMENT_AMOUNT] { first = try_with(first, p, 1); }
        }
        if b.kind == Kind::FlowerPotBlock && b.name != "flower_pot" { first = pot; }
        let mut ok = oracle.placeable(item, first);
        if b.name == "bamboo" { ok = true; } // (BuildState.spec: bamboo goes in as a shoot)
        if ok { n_placeable += 1; } else { unplaceable.push((b.name.clone(), "table")); }
    }
    println!("== placeable by hand: {n_placeable} of {n_item} blocks with an item ({} blocks in all); oracle predictions {}", tb.blocks.len(), oracle.predictions);
    let no_item: Vec<&str> = unplaceable.iter().filter(|x| x.1 == "no item").map(|x| x.0.as_str()).collect();
    let by_table: Vec<&str> = unplaceable.iter().filter(|x| x.1 == "table").map(|x| x.0.as_str()).collect();
    println!("unplaceable, no item ({}): {}", no_item.len(), no_item.join(" "));
    println!("unplaceable, has an item but no table entry gives it ({}): {}", by_table.len(), by_table.join(" "));
    let all_unplaceable: BTreeSet<&str> = unplaceable.iter().map(|x| x.0.as_str()).collect();
    let missing: Vec<&&str> = JAVA_UNPLACEABLE.iter().filter(|n| !all_unplaceable.contains(**n)).collect();
    println!("blocks the Java runs could not place that come out placeable here ({}): {:?}", missing.len(), missing);
    let extra: Vec<&str> = by_table.iter().copied().filter(|n| !JAVA_UNPLACEABLE.contains(n)).collect();
    println!("blocks with an item that are unplaceable here and not in the Java list ({}): {}", extra.len(), extra.join(" "));
    let must_fail: Vec<&&str> = MUST_PLACE.iter().filter(|n| all_unplaceable.contains(**n) || !tb.block_by_name.contains_key(**n)).collect();
    println!("ordinary blocks that must be placeable but are not ({}): {:?}", must_fail.len(), must_fail);
    // ------------------------------------------------------------------ 4. orientation properties that no click changes
    // (a block with a facing / axis / rotation / orientation property whose placement never sets it is either a block
    // that vanilla places in its default state, or a getStateForPlacement that was not ported)
    let mut fixed: Vec<String> = Vec::new();
    for (i, it) in tb.items.iter().enumerate() {
        if i == 0 || it.blocks.is_empty() { continue; }
        let table = oracle.table(i as u16);
        for &bl in &it.blocks {
            let b = &tb.blocks[bl as usize];
            for pr in [P_FACING, P_AXIS, P_ROTATION, P_ORIENTATION] {
                if !has(b.default, pr) { continue; }
                let vals: BTreeSet<u8> = table.iter().filter(|e| **e != 0 && tb.block[**e as usize] == bl).map(|e| val(*e, pr)).collect();
                if vals.len() == 1 { fixed.push(format!("{}.{}", b.name, PROP_NAMES[pr as usize])); }
            }
        }
    }
    println!("== orientation properties that the item's clicks never vary ({}): {}", fixed.len(), fixed.join(" "));
    // ------------------------------------------------------------------ 5. nothing panics on odd input
    // random states around the click (any state id, sensible or not), every item, random clicks
    let mut seed: u64 = 0x9E3779B97F4A7C15;
    let mut rnd = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let mut w = World::new(0, 0, 0, 5, 5, 5, -1000, 100000);
    let (mut placed, mut survive) = (0u64, 0u64);
    let rounds = 300_000;
    for round in 0..rounds {
        if round % 16 == 0 {
            let fill = rnd(4);
            for i in 0..w.cells.len() { w.cells[i] = if rnd(4) < fill { rnd(tb.n as u64) as S } else { 0 }; }
        }
        let item = 1 + rnd(tb.items.len() as u64 - 1) as u16;
        let hp = p(1 + rnd(3) as i32, 1 + rnd(3) as i32, 1 + rnd(3) as i32);
        let face = rnd(6) as Dir;
        let hit = [hp.x as f64 + rnd(17) as f64 / 16.0, hp.y as f64 + rnd(17) as f64 / 16.0, hp.z as f64 + rnd(17) as f64 / 16.0];
        let c = PlaceCtx { item, hit_pos: hp, face, hit, inside: rnd(8) == 0, yaw: rnd(7200) as f32 / 10.0 - 360.0, pitch: rnd(1801) as f32 / 10.0 - 90.0, sneak: rnd(2) == 0, player_pos: PROBE_POS, replace_clicked: true }
            .resolve(&w);
        if let Some((pos, st)) = rules::predict_place(&w, &c) {
            placed += 1;
            let mut w2 = World::new(0, 0, 0, 5, 5, 5, -1000, 100000);
            w2.cells.copy_from_slice(&w.cells);
            if round % 64 == 0 { place::item_place_block(&mut w2, &c, pos, st); }
        }
        let _ = place::predict_place_on_water(&w, &c);
        let st = rnd(tb.n as u64) as S;
        if rules::can_survive(&w, st, hp) { survive += 1; }
        let _ = place::can_be_replaced(&w, st, hp, &c);
        let _ = rules::placement_collision_shape(st);
    }
    println!("== fuzz: {rounds} random clicks in random surroundings, {placed} placed something, {survive} random states could survive; no panic");
    println!("== done in {:?}", t0.elapsed());
}
