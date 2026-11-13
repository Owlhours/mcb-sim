# Porting notes for the block rules (shared by everyone working on `sim/src/rules`)

The simulator replaces the vanilla 26.2 server for one purpose: predicting what a schematic-building bot does. The
block rules are ports of the vanilla block classes. The readable vanilla source is in
`/work/reference/mc/decompiled/net/minecraft` (blocks: `world/level/block/*.java`, `world/level/block/piston`,
items: `world/item`, redstone helpers: `world/level/redstone`, `world/level/Level.java`, `SignalGetter.java`).
The Java bot that drives all this is in `/work/reference/java-bot/src/mcb` (read `env/Oracle.java`, `env/Client.java`
and `build/StateInfo.java` to see how the rules are used). Never run Java; none is installed.

## Ground rules
* **Port, do not invent.** Follow the vanilla method bodies statement by statement (order of checks, order of
  neighbour visits, which flags are passed to `setBlock`). Where the simulator cannot do what vanilla does (light
  levels, fluids, entities other than the player, random ticks, explosions), choose the behaviour that matches the
  bot's environment and say so in a comment: random ticks are off, there is no water or lava anywhere (waterlogged
  is always false; `getFluidState` is always empty), nothing ever drops as an item, time is frozen at noon with clear
  weather, sky light is 15 everywhere in the open (treat light as 15), the only living entity is the player.
* Blocks whose behaviour needs something unmodelled (TNT exploding, dragon egg teleporting, ...) should do the
  nearest harmless thing and bump `w.unmodelled`.
* The rules must never panic on odd input (states of unexpected blocks, positions outside the box: `w.get` returns
  air/ground there and `w.set_block` returns false). Use `get`/`has`/`try_with` when a property may be missing.
* Only edit the files assigned to you. Other files (`tables.rs`, `world.rs`, `geom.rs`, `shapes.rs`, `rules/mod.rs`,
  `oracle.rs`, `lib.rs`, `main.rs`) belong to the integrator; if you need something from them, work around it
  inside your own file and list the request in your final report. You may add test binaries under `sim/src/bin/`
  with names starting with your module name.
* Build with `cd sim && cargo build --release` (no external crates are available; std only). Keep the crate
  warning-free in your files where practical.
* Commit your work in your worktree with a clear message when done (git is configured). Do not push anywhere.

## The API you build on (read the files; this is a map)
* `tables.rs`: `S` = block state id (u16, the vanilla global state id; 0 = air). `t()` gives the tables:
  per-state flags copied from the Java version's `StateInfo` (`air`, `replaceable`, `full_cube`, `no_collision`,
  `sturdy_top`, `interactive`, ...), `block[s]` = block index, `blocks[b]` = `BlockInfo { name, kind, first, n,
  default, props, item, push, offset }`, shapes (`col_shape[s]`, `out_shape[s]` index `shapes`), items
  (`items[i].name/stack/blocks`, `item[s]`), tags (`in_tag!(state, "fences")`, tag names as in
  `data/tags.tsv`). Property access by generated constants in `props.rs`: `get(s, P_FACING) -> Option<u8>`,
  `val(s, P_HALF)`, `is_true(s, P_OPEN)`, `with(s, P_FACING, V_NORTH)`, `with_bool`, `try_with`, `has`. Values are
  codes: integers are themselves (0..=31), named values are `V_*` constants. `kind_of(s)` gives the vanilla block
  class as `kinds::Kind` (subclasses are folded into their base where they behave alike - see `ALIAS` in
  `tools/gen_tables.py`; use `is_blk!(s, "name")` / `blk!("name")` for individual blocks).
  `t().default_of("name")`, `t().blocks[b].default` = default state.
* `geom.rs`: `P` (block position) with `rel(dir)`, `above()`, `below()`; directions `DOWN, UP, NORTH, SOUTH, WEST,
  EAST` (= vanilla `get3DDataValue`), `HORIZONTAL` (vanilla `Direction.Plane.HORIZONTAL` order), `opposite`,
  `clockwise`, `counter_clockwise`, `axis`, `dir_of(value code)`, `dir_val(dir)`, `dir_prop(dir)`, `from_yrot`,
  `Aabb`, `floor`.
* `world.rs`: `World` with `get(pos)`, `set_block(pos, state, flags)` (vanilla `Level.setBlock` semantics, flags
  `F_NEIGHBORS=1, F_CLIENTS=2, F_KNOWN_SHAPE=16, ...`; `F_ALL = 3`), `destroy_block`, `remove_block`,
  `schedule_tick(pos, block_index, delay)` / `schedule_tick_prio`, `has_scheduled_tick`, `block_event`,
  `update_neighbors_at(pos, block)`, `update_neighbors_at_except`, `neighbor_changed(pos, block)`,
  `is_face_sturdy(pos, dir)`, `can_support_center(pos, dir)`, `can_support_rigid(pos)`, `is_unobstructed(state,
  pos)`, `spawn_falling(pos, state)`, `entities` (bounding boxes; the player is `is_player`), `containers`,
  `opened_menu`, `rand_*`, `game_time`, `unmodelled`.
* `rules/mod.rs`: the hook signatures (`PlaceCtx`, `Clicker`, `Use`) and how the three rule files are chained.
  The function names and signatures in the stub files are the contract; keep them.

## How the rules are checked
There is no vanilla to compare against, so correctness comes from careful reading. Useful cross-checks:
* `reference/mc/states.tsv` holds, per state, flags computed by the real game (`sturdyTop`, `replaceable`,
  `interactive`, collision/outline boxes ...).
* `reference/results/*.tsv` are real runs of the Java bot (columns in `reference/README.md`): column 19 lists bad
  cells by block, column 20 the skip reasons (e.g. `cannot be placed by hand: lily_pad`).
