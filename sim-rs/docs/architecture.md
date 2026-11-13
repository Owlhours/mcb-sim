# Architecture decision record: a Java-free simulator of the schematic-building bot

Status: accepted (phase 1); sections 1-7 are the record as written before the build, section 8 is what the build
showed. Numbers marked *measured* come from this container; current numbers are in `scoreboard.md`.

## 1. What has to be predicted, and what that depends on

For one schematic the Java version reports a verdict (per 16³ section, ≤ 3 % bad), a tick count, and the missing /
wrong / leftover cell counts. Reading its source (`reference/java-bot/src/mcb`, ~7,300 lines) shows how those
numbers come about:

* **Ticks are produced one at a time by closed loops over the player's exact state.** `Body.follow` steers by the
  player's position, speed and `onGround` flag each tick (thresholds such as `dist < 0.38`, `hspeed() < 0.03`,
  coasting `hspeed()*2.2`); `pillar`, `tower`, `lean`, `climb`, `descend`, `breakBlock` are the same kind of loop.
  A walk of N cells does not cost "N × some constant": it depends on turns, sprint windows, step-ups, the sneak
  approach at the end. So tick counts need the real per-tick player physics, or a model fitted to it.
* **What gets built is decided by the game's placement rules.** `Oracle` runs `BlockItem.place` up to the write for
  every candidate click, and tabulates each item over 6 faces × 4 quadrants × 32 yaws × 9 pitches × sneak. Which
  cells end up "cannot be placed by hand", "no way to place", wrong or missing follows from those rules plus
  `canSurvive`, shape updates, falling blocks, scaffolding stability and, in many schematics, live redstone.
* **The order of work depends on everything above**: the nearest eligible cell from where the bot stands, what
  it carries, which stances are reachable (A* over the bot's own grid), what a stray block change did.

Three properties make a faithful port possible at all: the bot is single-threaded and deterministic apart from two
hash-order effects (below); it only touches the game through `Client` (a narrow interface); and its world is tiny
and static apart from what it does itself (flat ground, no fluids, no mobs, no random ticks, no drops, frozen
time).

## 2. Where the Java version's time goes (given) and what that implies

`reference/profile/java-profile.md`: median 169 µs per tick, 295× real time; in the two profiled builds 70–88 % of
the time is the bot's route search, 6–16 % vanilla code called from the bot (collision, shapes, state queries),
1.5–4 % the server tick, the rest small. 1000× needs ≤ 50 µs per tick all-in.

So removing the game alone gives at most ~1.4×. The route search has to become several times cheaper *with the same
results*, because its results (which stance is reachable, the path walked, even the number of cells expanded:
`nav.expanded <= 12`, `> 96`) feed the bot's decisions.

## 3. Options considered

| | Option | Speed potential | Fidelity | Verdict |
|---|---|---|---|---|
| A | **Statistical model** (regress ticks and failure from schematic features, fitted to the 290 results) | ~10⁶× | Verdict: always-PASS already scores 88 %; ticks: the per-build spread at equal block count is large (7–57 ticks/block p10–p90) and driven by geometry; counts of bad cells are not predictable from aggregates. No way to explain or improve a miss. | Rejected as the product; kept as a sanity baseline. |
| B | **Port the bot's decisions, replace per-tick execution by a cost model** ("walking this path costs f(path)", "a pillar costs 14 ticks") | 10⁴–10⁵× | The decision logic reads the body's exact state after every primitive (where the bot came to rest decides the next nearest cell, whether a click is within the 4.5 reach, whether `sealsIn` looks from this cell or the next). A cost model would have to return an end state too; errors compound over thousands of blocks, and every primitive has failure paths (stuck, fell, missed click) that the bot's recovery logic reacts to. Calibrating it needs per-primitive traces we do not have. | Rejected for now. It is the fallback if exact physics were too slow - it is not (see 5). |
| C | **Port everything the bot can observe: bot logic verbatim, plus a compact re-implementation of the parts of the game it touches** (player physics, collision, ray casts, placement rules, block updates, scaffolding, falling blocks, redstone, containers) | 10³–10⁴× | Tick-exact where the port is exact; deviations are bugs that can be found by diffing against the reference results build by build. | **Chosen.** |
| D | C, but running the decompiled Java through a hand-written interpreter or transpiler | – | – | Rejected: forbidden in spirit (a Java interpreter), and slower than the JVM. |

The deciding measurement for C over B: the per-tick physics is cheap. *Measured* (first complete port, before any
tuning): the player tick including collision costs well under 1 µs in open space; whole builds of 54–69 blocks run at
2,200–6,000× real time, and one of them (`eec0f83d`, an 8×1×8 floor) reproduces the Java run exactly: 372 ticks,
65 blocks placed, 1 scaffold block, 1 supply trip. There is no speed reason left to abstract the execution.

## 4. Chosen design

Language: **Rust** (std only; no JVM anywhere). One process, one core per build, tables shared read-only.
Rust over Go/C++: no GC pauses or bounds-check-free footguns, value types for the hot arrays, and `panic=unwind`
gives the Java version's `OutOfTime` exception semantics for free.

```
tools/gen_tables.py     reference data -> data/{blocks,tags,items}.tsv + sim/src/{kinds,props}.rs
sim/src/tables.rs       per-state tables (the Java StateInfo columns from states.tsv, properties, tags, shapes)
        shapes.rs       voxel shapes: collide / overlap / face support, as VoxelShape + SupportType
        world.rs        block grid of the site + Level.setBlock semantics, CollectingNeighborUpdater,
                        scheduled ticks, block events, change list for the client
        rules/          block behaviour ported from the decompiled 26.2 classes:
                        place.rs (BlockItem.place, getStateForPlacement, canSurvive, canBeReplaced),
                        basic.rs (updateShape, ticks, use, setPlacedBy ...), redstone.rs (signals, wire, pistons ...)
        physics.rs      BlockCollisions, Entity.collide (step-up), supporting block, clip (ray cast)
        player.rs       the BotPlayer tick: LocalPlayer input handling + Player/LivingEntity/Entity movement
        sim.rs          Client.tick (pick -> clicks -> movement) + packet handlers (use item on, block breaking,
                        container clicks) + Env.tick (scheduled ticks, broadcast, block events, falling blocks)
        oracle.rs       Oracle (predict, per-item tables, floats)
        bot/            the bot, class by class (grid, ray, body, nav, state, moves, placing, stances,
                        scaffolds, supplies, builder); fastutil.rs reproduces fastutil's hash-set iteration order
        build.rs        Target, Site, Judge, BuildRun; main.rs: CLI writing the Java results TSV format
```

### What is modelled exactly, and what is not

Exactly (ported statement by statement from the decompiled source):

* Player movement as the Java `BotPlayer` runs it: key handling and the sprint/crouch state machine of the client,
  `LivingEntity.aiStep/travelInAir`, `Entity.move` with `maybeBackOffFromEdge`, `collide` with step-up candidates,
  supporting-block lookup, block friction/speed/jump factors, climbing, poses and their bounding boxes, `Mth.sin`
  table lookups, float/double mixing. One peculiarity matters and is kept: the bot's player is a *server-side*
  player, for which `isLocalInstanceAuthoritative()` is false, so fall distance is never tracked and `fallOn`
  never runs (no farmland trampling, no fall damage path).
* The client tick order of `Client.tick` and the server handlers behind it (use-item-on with the sneaking rule,
  survival block breaking with client progress and the server's 0.7 rule and delayed destroy, hotbar selection,
  container quick-move/swap with vanilla's slot order).
* The server tick order that the bot can observe: scheduled block ticks, then the block-change broadcast, then
  block events, entities, block entities. The Java `Env.tick` skips full ticks while the world is idle; the
  simulator always runs the full order, which is equivalent because nothing is pending in a skipped tick.
* Which block changes the client is told about, including the two unconditional updates after every use-item-on
  (the bot's bookkeeping reacts to them even when nothing changed).
* Block shapes as voxel shapes (the game's collision resolution works on voxels, not boxes; the difference shows
  when the body is inside a shape), context-dependent collision (scaffolding, powder snow), random per-position
  offsets of plant shapes, ray casts over outlines with the interaction-shape override.
* The bot itself, including int/float/double arithmetic, Java `Math.round`, and fastutil iteration order (the
  bot iterates `IntOpenHashSet`s where order breaks ties: deferred cells, scaffold blocks at clean-up).

Abstracted or left out, with reasons:

* Chunks, lighting, networking, entity tracking, sounds, particles, advancements, statistics: not observable by
  the bot. Light matters in one place only (mushrooms need darkness); the open-sky value is used.
* Fluids: the target has none (normalised away) and nothing creates any (ice is never mined).
* Item entities: block drops are off. Two rare paths still drop items in the real game (a falling block that
  cannot land; `ENTITY_DROPS`); the simulator counts them (`items_lost`) instead of simulating pickup.
* Explosions (TNT), entity pushing by pistons, damage/health: counted as `unmodelled` events per build so that
  affected builds can be identified; modelled only if the scoreboard shows they matter.
* Vanilla voxel grids are finer than the box list in `states.tsv` reveals; the simulator's grids use the box
  coordinates. This only differs for a body *inside* a multi-box shape.
* The Java run's identity-hash orders (`ReferenceOpenHashSet` of chunk holders, `HashSet<Item>`): unknowable; the
  simulator uses insertion order. This and the client/server timing of a real JVM are the floor on agreement (the
  reference notes 1 build in 7 differing by 15 % in ticks between two Java set-ups).

### Route search

Ported first as-is (same expansion order: binary heap on `cost+heuristic` with the node index as tie-break,
same budgets), because its outcome includes the path, `goalCost`, `expanded`, `scaffoldMoves`, `towerMoves`.
Speed-ups are restricted to those that cannot change any of these:

1. data layout: one 12-byte node record (stamp, cost, parent, move) instead of four parallel arrays; state tables
   as flat arrays; no allocation in the loop (*done in the first port*);
2. per-cell caches of `stand16`/`passable` invalidated on writes to the grid (pure functions of four cells);
3. memoising whole searches on a hash of everything a search reads (grid, scaffold set, blocked edges, rule
   flags, start/goal/budget): the Java version repeats identical failing searches many times between world
   changes (`searches=143357 ... plans=33632 planFails=33579` in 4,778 ticks in one build);
4. only if still needed: exact early termination of searches whose reachable set is provably exhausted.

2–4 are applied in that order and only as far as the scoreboard requires.

## 5. Validation

No Java can be run, so validation is by reading and by comparison with the reference results:

* **Table self-checks against the game's own data** (`mcsim selfcheck`, rule check binaries): the shape code must
  reproduce `sturdyTop`, `fullCube`, `noCollision` for all 32,366 states; target normalisation reproduces the
  reference `target blocks`, `sections`, `exempt cells` for all 290 builds (*measured: 290/290*).
* **Rule scenarios** written from the vanilla source (connections, stairs shapes, doors, scaffolding collapse,
  redstone timings, pistons): `sim/src/bin/*_check.rs`.
* **Placeability**: the set of blocks the Java runs reported as "cannot be placed by hand" must be exactly the
  simulator's unplaceable set among the blocks that occur.
* **Whole builds**: `tools/compare.py` diffs simulator output against `reference/results/f6b21f4-*.tsv`:
  verdict, ticks (ratio), placed, skipped and skip reasons, missing/wrong/leftover, trips, scaffold placed/left.
  Small builds that match tick for tick pin the physics and bot port; the first one does.
* **Hold-out**: `f6b21f4-dev3.tsv` (40 builds) is not looked at during development; it is compared at milestones
  only. Development uses `small`, `strat-small`, `dev2`; `dev2big` is the stress set for speed.

## 6. Expected results and measures

* **Speed** (per core, wall time per build including site set-up, excluding the one-time table load of ~0.1 s,
  like the Java figures exclude server boot): median ≥ 1000× is the floor; expected 2,000–5,000× median after the
  route-search work, with the long builds (dominated by failing searches in the Java version) the ones to watch.
  Reported as median, p10, p90 and wall-time-weighted aggregate per set.
* **Fidelity**: same verdict on ≥ 95 % of builds; median tick error ≤ 5 %, p90 ≤ 25 %; aggregate ticks within
  5 %; missing/wrong/leftover within the per-build noise of the Java version on failing builds. Proposed additional
  measure: the share of builds reproduced *exactly* (ticks and all counts identical), because exact matches are
  what a faithful port produces and partial credit hides compensating errors.

## 7. Risks

| Risk | Mitigation |
|---|---|
| The long tail of block classes (≈ 200 kinds): each rule is a chance for a silent difference | Rule ports follow the vanilla source one-to-one; census of the reference builds orders the work; per-block bad-cell diffs against the results files point at the offender |
| Redstone machines running while half built (pistons moving blocks, observers) | Ported as real mechanics (wire, torches, repeaters, comparators, pistons with moving blocks, observers); unmodelled events are counted per build |
| Chaotic sensitivity: one different tick early changes the order of thousands of later decisions | Expected and accepted for *exactness*; the aggregate measures are robust to it. Exact-match builds guard against systematic drift |
| Route-search memoisation changing behaviour | Keys cover every input; a debug mode re-runs the search and asserts equality |
| Reference data cannot be extended from here | Requests for traces go to `docs/requests.md` |

## 8. What the build showed (phase 2)

**Result** (`scoreboard.md`, milestone 3): the verdict agrees on all 290 reference builds, and 287 of them are
reproduced exactly - tick count and every cell count - including 29 of the 30 builds of 8k-80k blocks and, as a
clean hold-out figure, 39 of 40 (40 after its one failure was looked at). The largest tick error among the three
others is 1.9 %. Speed per core: median 3,300-4,300x real time on the ordinary sets, 1,900x on the hold-out,
1,200x on the big builds; time-weighted aggregates 430-3,200x; all 290 builds in 43 CPU-minutes against 300 minutes
for the Java runs.

**Option C was the right call, for a reason that is stronger than expected.** The bot is chaotic in the strict
sense: in `e52ac89a` (59,636 blocks) one pressure plate releasing one tick late decided whether a door was open
when the bot came back, the two runs then drifted apart, and 350,000 ticks later one had finished the build and the
other had given up with 10,161 cells unplaced. Any model that is not tick-exact is on the wrong side of such a
fork about half the time, and the forks are where the interesting outcomes (early endings, leftover scaffolding)
come from. The same holds for the speed/fidelity trade-off: cutting the route-search budgets in half leaves 27 of
60 builds exact and buys nothing; a quarter buys 1.4x and turns 30 missing cells into 1,348 (table in
`scoreboard.md`). So there is one mode, the faithful one.

**What found the errors.** Reading the source got the first port to 167 of 220 development builds exact. The rest
came from differential data, in this order of value:

1. *Placement logs of the Java run* (`reference/java-extra/events`, one line per placed block with its tick):
   the first differing line names the tick and the cell; from there `MCSIM_WATCH` and `MCSIM_BODY` show what the
   simulated bot saw. Every traced build led to a modelling error within the hour: the player's box that entity
   queries saw was a tick old (pressure plates, `e52ac89a`); sky light under a roof (inverted daylight detectors);
   pistons pushing the player and moving blocks colliding; the order of block updates within a chunk section
   (`ShortOpenHashSet`); ray casts using interaction shapes (hopper, cauldron, composter); cobweb sweeps.
2. *The game's own tables* (`placement-tables`, `floats`, `states-extra`): `ref_check` compares all 1,054 item
   placement tables (identical but for three random vine ages), all states' support faces and flags.
3. *Self-consistency*: the fast route search is checked against the first, literal port on every search
   (`MCSIM_NAVCHECK`); this caught the one optimisation bug that changed results (a packed field losing a sign).
4. *The simulator's own trace against one Java number.* The last verdict difference (`87f8e5fd`: Java gives up
   with 12,603 cells unplaced, the simulator finished the build) had no Java log. The run log showed the
   simulated bot standing in a lit soul campfire for 328 ticks, and at that moment it had placed 8,345 blocks -
   exactly the Java run's final count. The Java bot had died there. With health, food and saturation,
   regeneration, damage and the dead player's state ported, the build is reproduced to the tick (1,093,163).
   The same habit - ask what the *player entity* can undergo that the port ignores - then found freezing in
   powder snow (it slows walking) and the sneaking exemptions of turtle eggs and redstone ore.

**The floor on agreement** - what cannot be reproduced even in principle from here:

* the order in which chunks send their block updates within one tick (identity hash order in the JVM; a fixed order
  is used; no reference build is currently known to depend on it, but two traced event logs differ in exactly
  this order);
* everything drawn from `Level.random`, which the game seeds from the clock, so the Java runs differ among
  themselves here: the velocity of an item dropped by a falling block that breaks (the bot may pick the item up;
  it happens in two reference builds: one is reproduced exactly anyway, in the other handing the simulated bot
  the item changes nothing), the delay of
  60-99 ticks before a coral out of water dies (three reference builds; `MCSIM_CORAL` forces the draws), the
  1-in-100 chance per tick that a trampled turtle egg breaks (one build; `MCSIM_EGG`);
* the Java harness's wall-clock limit (one build): the simulator has to be told the tick (`--stop-at`);
* not modelled so far: explosions, item entities, sculk shriekers, a few rare block entities. `unmodelled` and
  `lost` in the run log flag every build that touches one of these, `random=` counts the draws from the
  clock-seeded source, `DIED` marks a build in which the bot dies.

For draws from the clock-seeded source the simulator takes the likeliest single outcome where there is one (a
trampled turtle egg does not break: 99 % per tick) and its own fixed pseudo-random stream otherwise.

**Where the time goes now, and the search work that was and was not worth it.** On the median build half the time
is route search and a sixth is collision queries; on the slow tail (p10: 280-400x) route search is 80-98 %, at up
to 1,700 expanded cells per tick - the Java bot's own cost profile, since every search result feeds a decision.
Exact speed-ups that were kept: flat state tables and node records, a padded column-major mirror of the grid with
all per-cell facts in one word next to the search node (no bounds tests, one cache line per cell), a `stand16`
cache invalidated on writes, the way-out test stopping when its answer is certain (97 cells reached), the deferred
pass picking from a sorted list while the bot stands still (the Java version rescans: quadratic in early-ending
builds), huge pages. Measured and dropped, because the hit rate was too low to pay for the bookkeeping:

| idea | measurement |
|---|---|
| memoise whole searches on a hash of their inputs (ADR item 3) | 4-16 % of searches repeat with nothing changed |
| prove a failing search unreachable by a flood backwards from the goal (item 4) | 0 % of budget failures provable |
| remember the region an exhausted search covered and answer later searches from inside it | 0.3 % of failing searches (the world changes between them) |

What is left is constant-factor work on the expansion loop (perhaps 1.2-1.5x) and the collision queries
(about 1.1x on the median build); neither changes the picture. The tail is inherent in the bot: a build where it
spends 600 million cell expansions costs that many, in any language.

**Expectations of section 6 against the outcome.** Speed: expected 2,000-5,000x median, measured 3,300-4,300x
(1,200x on big builds, which section 6 named as the ones to watch). Fidelity: expected same verdict on >= 95 %,
median tick error <= 5 %; measured 100 % and 0.0 % (p90 0 %, maximum 1.9 %), because exactness turned out to be
reachable for almost every build rather than a few. The proposed extra measure, the share of exact builds, became
the main one: 99 %.

**Section 4 corrected.** "Explosions, entity pushing by pistons, damage/health: counted as unmodelled" no longer
holds: piston pushing, damage, health and death are modelled (a death decided the last verdict difference), and so are sculk sensor
vibrations. Light is flooded for daylight detectors. The idle-tick shortcut of the Java `Env` is ported as far as
it moves the game time the sensors read.

## 9. Following a moving Java bot (2026-11-13)

The Java bot is not frozen: a's master moved 27 commits past the port base in a day (890 added lines in 12
classes), and will keep moving. A port that is right for one commit and laborious to bring to the next is of
little use, so the second requirement became: **the cost of a port must be proportional to the Java diff, and the
port must be checkable before and after Java reference rows exist.** Decisions:

1. **The Java source the port corresponds to lives in the repository** (`port/java`, with its commit). A port
   commit holds the Java delta and the Rust delta together. Rejected: pointing at `reference/` (not in git, and
   replaced by the host between kits - the base of the next diff would be gone).
2. **The Java-to-Rust mapping is data** (`port/map.tsv`) **with a naming rule as its default** (`Class.camelCase`
   is `fn camel_case` in the class's file), and `tools/port.py` resolves it against the Rust source as it is: 339
   Java methods, 42 exceptions written down, each a line saying where the method went or why it is not ported. `port.py status <new tree>` turns a new
   Java tree into a list of changed members with the Rust function for each (124 for this port), `done` ticks
   them off, `check` fails if a Java method has neither a Rust function nor a reason. Rejected: marker comments in
   the Rust source naming the Java method (they rot silently; the rule plus a check cannot).
3. **Old revisions stay runnable** (`--rev`, `sim/src/rev.rs`): changes are guarded by the Java version's own
   switch where it has one, by the revision otherwise. This costs a branch per change and buys the only regression
   test available without Java: the previous commit's reference rows must stay exact after the port (they did:
   40 / 119 / 59). It also makes a's switched experiments (`MCB_NO_NEXT` ...) predictable under the same names.
   Rejected: a build feature per revision (two binaries to keep building, and no way to compare revisions in
   one run).
4. **One route search.** The literal copy and the fast copy of `Nav.search` are one generic function now; the
   non-fast instantiation recomputes every cell fact and is the cross-check of the fast one's mirror
   (`MCSIM_NAVCHECK`). A Java change to the search - there were five in this diff - is made once. What was given
   up: the literal copy was an independent second reading of the Java text; after 290 builds of agreement that
   reading had done its work.
5. **Review replaces reference rows until they arrive.** Each port is read against the Java diff by reviewers
   who did not write it, per group of files, asked for discrepancies only.
6. **The Java side is asked for a routine, not for favours** (`docs/requests.md`, "port kit"): source, rows for
   the three development sets and placement logs for all their builds per commit, with the event hooks kept in
   the Java tree behind a variable. The `metrics` column a added to the result rows is ported too: it says which
   mechanism went differently before any log is read.

What the first use showed: the port of 2d9a193 took one session including the tooling; the new revision runs
faster than the old one in the simulator (median about 7,600x against 5,000-6,600x on the same machine state),
because the remembered way out replaces most of the trap-check searches - a's change to the bot's own cost
profile carries over, as it should in a port.

Outcome, once the Java rows arrived (scoreboard, milestone 4): with no change to the port, 290 of 290 verdicts and
288 of 290 exact rows, the two inexact builds being ones that were inexact before the port; 286 placement logs
with the same events; the one build in which the new health guard acts, predicted beforehand to die at tick
26,834 after 12 retreats, does exactly that, health value for health value. So a port of this size (124 members)
can be made right from the diff alone with this procedure - mapping tool, guards with the old revision as the
regression test, one search, independent review. What the Java data added was not corrections but confidence,
and two sharper instruments for next time: the `metrics` column and the verbose `NAV` counters (searches, cells
expanded) show a difference that leaves the result row untouched - they found one (`0cbfffb5`, one trap check).
