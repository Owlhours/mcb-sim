# Method: how the simulators work, how they were validated, and how changes were judged

This page covers four things: what each of the three components computes, how far each one was checked against
the one before it, the evaluation method behind every number, and the rules of the task (judge, world, time
limit). For what to watch out for when reusing the code, read [BUILDING-ON-IT.md](BUILDING-ON-IT.md). For dates
and names, read [HISTORY.md](HISTORY.md).

## 1. The task all three simulate

A player builds a schematic on flat ground in survival.
- **Moving.** It walks, jumps, sprints, sneaks and climbs, and it may fall a long way. It never flies or
  airplaces.
- **Reach.** It places and breaks blocks only within survival reach, by keyboard and mouse.
- **Inventory.** It carries a normal 36-slot inventory and restocks from supply chests north of the site.
- **Scaffolding.** It may put up temporary blocks (scaffolding columns, loose bridging blocks), and it must take
  them down again.
- **Time limit.** 48 hours of game time at 20 ticks per second, which is 3,456,000 ticks per build. Read
  literally, "48 Minecraft-clock hours" would be 48,000 ticks, which no build of more than a few thousand blocks
  could meet.

**World rules.** In every measured run:
- survival on Peaceful, no mob spawning;
- fall damage off, random ticks off (no growth, decay or spread), time and weather frozen;
- block drops off: broken blocks vanish;
- no tools: every block is broken by hand.

The supply chests hold everything the target needs plus a 5 % reserve, scaffolding and loose blocks. They are set
up by `mcb.build.Site`, and the world rules by `mcb.env.Env.applyWorldRules`.

**The judge** (`mcb.build.Judge` in Java, `build.rs` in `mcsim`).
- **Sections.** The build is divided into 16×16×16 sections counted from the schematic's origin. In each section,
  bad cells = wrong + missing + scaffolding left inside the box.
- **Pass.** A section passes at 3 % or fewer bad cells of its judged target cells. A build passes when every
  section passes.
- **What counts as right.** A cell is right when its block and its "intent" properties match. Properties that
  come from neighbours or change by themselves are ignored: connections, stair shape, power, age, waterlogging
  and so on (`StateInfo.IGNORED`).
- **Exempt cells** are neither missing nor counted: decoration (no collision box), and the blocks only an operator
  can place.
- **Impossible cells** (from 11-19 and 11-20) count as standing right, whatever stands there. These are targets
  that can't exist in survival, plus cells that hang only from such cells.
- **Target normalisation.** Fluids, floating gravity blocks and technical blocks are dropped from the target.

The judge was frozen on 11-14. It changed later only by those explicitly decided rules, each recorded in the
commit that made it.

## 2. The harness: the bot in the real game

`harness/src/mcb/env/Env.java` boots Mojang's 26.2 server classes in-process. It uses the headless game-test
server flavour, with a flat world and no network, and ticks it on the calling thread.
- **The body.** The bot's body is an ordinary server-side player entity, driven by key states produced exactly
  as the vanilla client's `LocalPlayer` produces them.
- **Its actions.** Its only actions are view, movement keys, clicks, hotbar and inventory slots.
- **Rules enforced by the game.** Clicks go through the server's real packet handlers, so reach, placement
  rules, break times and container rules are all the game's.
- **The rules oracle.** `mcb.env.Oracle` runs the game's own placement code without placing anything, to
  tabulate and double-check clicks.

**The fast part.** `Env.tick` skips the full server tick while the world is idle (nothing scheduled, no block
events, no block entities ticking, no other entities) and ticks only the player. Every other tick is the
server's own `tickServer`. `mcsim` always runs the full order and gets the same result, which shows that the
skip changes nothing observable.

**One build** is `mcb.run.BuildRun.run`, in five steps:
1. normalise the target;
2. set up the site and fill the chests;
3. join the bot just outside the build, on the chests' side;
4. run the builder until it finishes or the limit is reached;
5. judge with the level's actual blocks, then wipe the site for the next build.

`RunFile` (one file), `RunOne` (one hash from the corpus) and `RunCorpus` (a list, one result row each) wrap it.

## 3. `mcsim`: a statement-by-statement port

**Why a port, not a model.** The bot's decisions read the body's exact state after every primitive: where it
came to rest decides the next nearest cell, and whether a click is within reach. So tick counts depend on
per-tick physics, and errors in a cost model would compound over thousands of blocks. The deciding measurement
was that exact per-tick physics is cheap: well under 1 µs per player tick in open space. The first complete
port ran small builds at 2,200–6,000× real time and reproduced one Java run exactly. (The options considered
and why: `sim-rs/docs/architecture.md` §3.)

**What is ported exactly.** Each part follows the decompiled 26.2 classes or the bot's own Java source,
statement by statement:
- **Player movement.** The client's key and sprint/crouch handling, `LivingEntity` travel, `Entity.move` with
  step-up and edge back-off, supporting blocks, friction and speed factors, climbing, poses. It keeps the
  float/double mixing and `Mth.sin` table lookups, and one quirk: the bot is a server-side player, so fall
  distance is never tracked.
- **Tick order.** The client tick order and the server handlers behind it: use-item-on with the sneaking rule,
  survival breaking with the server's 0.7 rule, hotbar, container clicks in vanilla's slot order.
- **What the bot can observe of the server tick.** Scheduled block ticks, the block-change broadcast, block
  events, entities, block entities, and the two unconditional block updates after every use-item-on.
- **Shapes and ray casts.** Block shapes as voxel shapes, context-dependent collision (scaffolding, powder snow),
  per-position plant offsets, ray casts over outlines.
- **Block rules.** Placement (`getStateForPlacement`, `canSurvive`, `canBeReplaced`), shape updates, scheduled
  ticks, falling blocks, scaffolding collapse, and redstone as far as the bot's builds need it: signals, wire,
  repeaters, comparators, pistons, observers, bulbs.
- **The bot itself.** Including Java integer and float arithmetic, `Math.round`, and fastutil's hash-set
  iteration order where it breaks ties.

**What is left out.**
- **Not observable by the bot:** chunks, lighting (open sky is assumed), networking, sounds, advancements.
- **Fluids.** The target has none and nothing creates any.
- **Item entities.** Drops are off. The two rare paths that still drop items are counted as `lost`.
- **Explosions, piston-pushed entities, some damage paths.** These are counted as `unmodelled` events per build.
- **The JVM's identity-hash orders.** These can't be known, so insertion order is used. Together with JVM timing,
  this is the floor on agreement: a build that draws from a clock-seeded random source is flagged `random=`.

The run log prints `unmodelled=`, `lost=` and `random=` for every build. **A build with `unmodelled` or `lost`
above 0 may differ from the Java run.** In development, such builds were not trusted until checked in Java.

**Tables.** The per-state facts come from the game itself, not from hand-written lists:
- `work/reference/mc/states.tsv` holds one row per state (32,366) with the Java bot's `StateInfo` columns,
  friction, destroy time and the collision and outline boxes. `harness/run.sh tables` writes it.
- `java-extra/states-extra.tsv` adds redstone conduction, push reaction, face sturdiness and default states.
- `sim-rs/data/{blocks,items,tags}.tsv` hold block classes, item stack sizes and tags. They were generated once
  from the game's data by `tools/gen_tables.py`.

`mcsim selfcheck` and the `*_check` binaries test the shape and rule code against these. `ref_check` compares
the simulator's own placement tables with the ones the Java oracle writes.

**Route search** was ported as-is first: the same expansion order, tie-breaks and budgets, because the bot reads
the path, the cost and the expansion counts. Speed-ups were restricted to ones that can't change any of these:
- flat node records;
- per-cell caches that are invalidated on writes;
- memoised identical searches.

`MCSIM_NAVCHECK=1` runs the reference search beside the fast one and stops at the first difference.

**Following a moving Java bot.** From `mend` on, the simulator led: a change was made and judged in `mcsim`
first, behind a revision name (`--rev`) and an `MCB_NO_*` switch. A second, independent Java port then carried
it into the Java bot and checked that both played the same course. `sim-rs/port/java` is the Java source the
port was last brought to, and `port/map.tsv` maps each Java member to its Rust function.
`tools/port.py status <java src>` lists what a newer Java tree changes against it. The procedure is in
`sim-rs/docs/tracking-the-java-bot.md`.

## 4. `fastsim`: an approximation for trying algorithms

`fastsim` simulates the same task coarsely, made only for speed. It was fitted to logs of `mcsim` runs, not to
the game. The design:
- **Clock.** An event-driven clock jumps over whole actions using a cost table in game ticks. The table was fitted
  to 11,144 routes of `mcsim` (R² 0.958): per route 4.9 + 3.9 per walk step, + 11.7 per jump, + 12.3 per drop,
  + 14.5 per tower block, + 10.4 per climb, + 9.3 per descent, + 18.3 per bridge block. Chest and breaking times
  were fitted later.
- **World.** Column-major bit planes (solid, scaffold, occupied, still needed, target, loose), one cache line per
  64-cell column word. Vertical questions (can I stand here, where does a fall end, what is the column's next
  cell) are single-word bit operations. The target is one palette index per cell.
- **Route search.** A* over standing cells, bounded (24 cells plus 4 per cell of distance, weighted estimate
  1.5), with a small stamped node table. Routes it doesn't find are charged from the cost table, so the bot is
  never shut in.
- **Policy.** The bot's policy, coarsely: columns bottom-up, nearest eligible cell, stances, scaffold columns,
  load-planned trips, put-off cells, clean-up. Many of the bot's rules came in later as switches.
- **Click tables.** `mcsim`'s placement tables become 48-bit masks per state (face × view octant), produced by
  `mcsim-ref`'s `dump_click`.

Its twelve simplifications and the way each could bias an algorithm comparison are listed in
[fastsim/README.md](../fastsim/README.md). The main ones:
- blocks are full cubes or nothing;
- clicking is geometry-light;
- unsearched routes are charged from the table;
- the clean-up never leaves scaffolding;
- no machines.

**Levels.** `--level 1` uses the exact click tables. `--level 2` screens each build in fastsim and reruns the
flagged ones in the linked copy of `mcsim`. `--level 3` is that copy itself. It gives `mcsim`'s rows exactly, at
the copy's newest revision, `thoughtway` (11-17).

## 5. How far each was validated

| check | result | where it was measured |
|---|---|---|
| `mcsim` against the Java bot at `f6b21f4` | verdict on 290 / 290 reference builds, exact row on 287 | development, 11-13 (`sim-rs/docs/scoreboard.md`) |
| `mcsim` against the Java bot at `2d9a193` | verdict on 290 / 290, exact row (ticks and every count) on 288; 286 of 290 placement logs with the same events | development, 11-13 |
| Java bot (all revisions ported) against `mcsim` at the final revision | **39 of 40** development builds played exactly the simulator's course; the 40th differed by a known 12-tick Java run-to-run variation | the second Java port's final check, 11-21 |
| the released singleplayer mod against `mcsim` | `timber-frame-house` 68,361 ticks, a 9,975-block build 537,714 ticks, the same judge counts | 11-21 |
| `fastsim` (default level) against `mcsim` | game time summed per set 0.89 / 0.81 / 0.87 / 0.90 (small / dev2 / dev3 / dev2big); verdicts 139 / 140 on development lists, 27 / 30 on dev2big, 90 / 100 on huge builds, 292 / 753 on the triage set | fastsim's final state, 11-21 |
| `fastsim` as a ranking tool | 3 of 4 order and routing effects carried over to `mcsim`; the effects correlate 0.77–0.85 | same |
| `fastsim` as a failure predictor | on the strict test, no fast level beats "everything is the same"; only level 2 with the `virtual` screen (3,100×) and level 3 do | same |
| **this repository, rebuilt from scratch** | 9 of 9 builds: Java harness and `mcsim` gave identical ticks, verdict, missing and wrong (§6); generated `states.tsv` byte-identical to development's | 2026-11-21, while staging |

`mcsim`'s rows are reproducible: the same binary, revision, build and seed give the same row. The Java bot is
deterministic for a given set of classes too, apart from the rare clock-seeded draws above. In one early
comparison of two Java set-ups, 6 of 7 builds matched tick for tick and one differed by about 15 %.

## 6. The check from a clean checkout

Done while preparing this repository, on nine schematics of 337 to 33,714 blocks. Each went through
`harness/run.sh build` (Docker, 2 CPUs), `mcsim run` and `fastsim run`:

| build | blocks judged | Java ticks | `mcsim` ticks | Java wall | `mcsim` CPU | `fastsim` ticks | verdict (all three) |
|---|---|---|---|---|---|---|---|
| small spruce cottage | 337 | 5,874 | 5,874 | 3.8 s | 0.10 s | 7,711 | PASS |
| small stone tower | 346 | 11,043 | 11,043 | 4.3 s | 0.08 s | 11,288 | PASS |
| birch house with garden | 935 | 7,306 | 7,306 | 4.3 s | 0.05 s | 10,051 | PASS |
| oak barn | 1,526 | 12,403 | 12,403 | 5.1 s | 0.08 s | 17,500 | PASS |
| timber-frame house | 2,883 | 68,361 | 68,361 | 11.4 s | 0.55 s | 68,793 | PASS |
| twin-spire church | 9,407 | 302,162 | 302,162 | 27.4 s | 2.8 s | 265,321 | PASS (3 missing, 1 wrong, 5 scaffolding left, in Java and `mcsim` alike) |
| road with median | 16,557 | 71,763 | 71,763 | 13.2 s | 0.56 s | 103,711 | PASS |
| brick city block | 28,652 | 252,400 | 252,400 | 28.7 s | 2.4 s | 331,254 | PASS |
| floating islands | 33,714 | 737,919 | 737,919 | 153 s | 18.2 s | 788,377 | PASS (2 missing in Java and `mcsim`) |

Java wall time includes 2–3 s of server start-up per process. `fastsim` ran all nine in 0.24 s of CPU (median
335,000× real time on this shared machine). `fastsim --level 3` (its copy of `mcsim`, at `thoughtway`) gives the same row as `mcsim` here (checked on the
timber-frame house: 68,361).

## 7. How changes were judged

This is the part most worth copying if you continue the work.

**Runs are deterministic, but courses are chaotic.** The same code on the same build gives the same ticks. Any
change that alters one decision sends a build down a different course, and builds near a threshold then flip
for reasons unrelated to the change. Over five paired passes on 220 small builds, 20 verdict flips came from 543
perturbed builds (`q` = 0.037 per perturbed build). No verdict ever changed in a build whose ticks didn't.

**Chance was measured directly.** The same version was run under seeds: `MCB_SEED=k` starts the bot k blocks
further along the front of the build, and nothing else changes. On 220 small builds, only 2 changed verdict
with the seed (`q0` = 0.006). So most flips between two versions are real, small, unintended effects of the
change on particular builds, not noise that averaging removes.

**"No build may go from PASS to FAIL" was dropped.** For a harmless change that perturbs `m` builds,
PASS→FAIL and FAIL→PASS are two Poisson counts with mean `q·m/2`, and the net is Skellam. At `m` = 200 the old
rule rejects a harmless change 97 times in 100. Answering each apparent regression with another change is a
branching process with mean offspring 1.1–2.2, which is why the work stalled before the rule changed
(`scripts/evalmath.py`).

**The procedure** (`scripts/compare.py`, `scripts/seedgate.py`, `scripts/simcmp.py`):
- **Pass counts:** exact McNemar on the discordant pairs.
- **Bad cells:** they run from 0 to 40,000 per build, so the comparison is on `log10(1 + bad cells)`, with a
  sign-flip permutation test.
- **A guard on net loss,** calibrated by the Skellam distribution: reject when P(net loss ≥ observed) < 0.10.
- **Collapses:** bad cells up tenfold to 50 or more. One counts only if it repeats under a second seed.
- **Seeds.** From 11-16, decisions used 4–6 seeds, against the base version's own seed noise on the same builds.
  The seed gate reports the interval of the summed difference.
- **Held-out decision sets.** Each decision got a freshly drawn set that was read once: `big-ho*`, `huge-ho*`,
  `mag-ho1` and others, drawn with `scripts/draw_sample.py` and later `draw_goal2.py sub`. Development lists were
  debugged on and never used to decide.
- **A side effect is answered by narrowing the trigger** of the part that caused it, which perturbs fewer
  builds. It is never answered by adding a compensating behaviour.
- **Attribution costs one build.** Because runs are deterministic, a build that got worse is rerun once per
  switch to name the responsible part. Every revision has an `MCB_NO_*` switch for this.

**Diagnostics that made this cheap in `mcsim`:**
- `MCSIM_CAUSES=<dir>` writes a cause per failed section (`scripts/causes.py` makes cause tables).
- `--timeline` and `--checkpoint-at` save a build's state before a derailment, and `mcsim resume` replays from it
  with a candidate switched on (`scripts/cklocal.py`).
- `MCSIM_CAND=name` switches on screened candidates without a rebuild (`sim/src/cand.rs`).

**What the failure model said** (`scripts/evalmath.py`, fitted to an early full run of 320 builds). A build
failed for size-independent reasons with probability 8.6 %, and met a fatal event (shut in, wedged, a region
never reached) about once per 167,000 blocks. Under that model:
- halving the fatal events is worth +2.8 points over the corpus;
- the bot twice as fast is worth +0.2.

Speed was never the goal.

## 8. The corpus and the goal samples

**The corpus.**
- **Source.** About 80,000 schematic files harvested from public sharing sites: 79,135 readable, giving 74,515
  distinct builds after normalisation (`mcb.corpus.BuildIndex`; the hash is of the normalised content).
- **Sizes.** The median build is 2.5k blocks, 96 % are at most 100k, and 28.8 % are over 10k.
- **Not included.** The builds are other people's work under unknown licenses.

**The goal samples.** Each holds 1,000 builds drawn at random with the corpus's mix of sizes: 297 over 10k blocks,
43 of them over 100k. Both were measurement only. Neither was used for development or decisions.

| sample | filter | final result | 10k+ | 100k+ |
|---|---|---|---|---|
| first (`goal-corpus1000`) | none | 929 / 1,000 | 238 / 297 (80.1 %) | 20 / 43 |
| second (`goal2-corpus1000`) | no redstone component, no impossible cell, no build on any earlier list or run | **963 / 1,000** | **265 / 297 (89.2 %)** | 24 / 43 |

**How the second sample was drawn** (`scripts/draw_goal2.py`, seed 2026112102):
- **The pool.** 59,586 fresh builds of height up to 380.
- **The screen.** `mcsim kinds` listed every block kind of each schematic and counted its impossible cells.
- **Redstone filter.** It kept builds with no cell of the listed redstone kinds: dust, torches, redstone blocks,
  repeaters, comparators, pistons, observers, TNT, droppers, dispensers, hoppers and similar. Redstone ore, plain
  rails and non-sensor sculk were left in on purpose.
- **Impossible-cell filter.** It kept builds with no impossible cell.
- **The draw.** 31,483 builds had redstone and 4,143 more had impossible cells, which left 23,960. The sample
  was drawn from those with the first sample's count in every size bucket.

**The 37 failures of the second sample**, read from the rows only. They overlap:
- 13 ran out of time;
- 14 of the 16 builds over 300k blocks failed, with most of their cells missing;
- 10 were near-misses on scaffolding the clean-up left behind;
- 9 had a few to a few hundred wrong or missing cells in 1–6 sections.

All goal-sample numbers are `mcsim` rows at revision `covertour`, seed 0.
