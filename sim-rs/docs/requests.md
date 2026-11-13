# Requests for reference data from the Java version

**Status after part 2 of the delivery for `2d9a193`:** everything asked for in section A has arrived and is
compared - thank you. Confirmed with my tools (`REV=2d9a193 tools/runsets.sh`, `tools/evall.sh`,
`tools/navdiff.py`):

| | builds | same verdict | row exact | `metrics` equal | log identical | same events, other order within a tick | random ages only | log differs |
|---|---|---|---|---|---|---|---|---|
| small | 40 | 40 | 40 | 40 | | | | |
| strat-small | 120 | 120 | 119 | 118 | | | | |
| dev2 | 60 | 60 | 59 | 59 | | | | |
| dev3 | 40 | 40 | 40 | 40 | | | | |
| dev2big | 30 | 30 | 30 | 29 | | | | |
| all | 290 | 290 | 288 | 286 | 192 | 94 | 2 | 2 |

* `3aa40858` with `--stop-at 1662464` is exact (its one metrics difference, `exitJoined` 974,730 / 974,731, is the
  wall limit striking inside a 64-tick window of the simulator's check).
* `2642e9f3` (the build that dies): row, metrics and all 6,574 log lines identical, including each of the 1,469
  health values and the 12 retreats.
* The 26 verbose logs: in 20 every counter of the `NAV` line that the port keeps (35 of them, among them
  `searches`, `expanded`, trap checks, plans) is equal; 3 more differ only by the three searches the Java verbose
  run makes for its `END` line when scaffolding is left; in 4 (`6cf99326`, `8ec7c0f0`, `b410f5ab`, `e44f73e7`)
  `predictions` differs by 6-34 of 200,000-490,000 with everything else equal (a statistics counter of the
  oracle; cause not looked for).

**Still open, if you can spare them: the three small traces of section B** (the only builds of 290 that are
not exact, and one counter). Nothing else is needed for `2d9a193`. **For the next commit: the kit of A3 item 4**
(`src/`, rows and `MCB_EVENTS` logs for small / strat-small / dev2); dev3 / dev2big rows and a handful of verbose
logs only when the commit matters enough.

---

# B. Open: three builds, three small traces

All three need more than placement events to locate, and all three are cheap: one more hook each, behind
variables so that the normal logs stay as they are. The simulator writes the same lines (`MCSIM_BODY=from,to`,
`MCSIM_EVMORE=1` with `--events`), and `tools/bodydiff.py` / `tools/evclass.py` compare them.

**B1. Body trace** - `MCB_BODY=<from>,<to>`: in `Body.tick()`, after `c.tick(); ticks++`, for ticks in the range,
one event line (doubles and floats as Java prints them; coordinates build-relative like `rel()`):

```
<tick> B <x> <y> <z> <vx> <vy> <vz> <onGround> <yaw> <pitch> <sneaking>
   = ev("B " + (c.x()-g.ox) + " " + (c.y()-g.oy) + " " + (c.z()-g.oz) + " " + v.x + " " + v.y + " " + v.z
        + " " + c.onGround() + " " + c.yaw() + " " + c.pitch() + " " + c.sneaking())      // v = c.velocity()
```

| build | ticks | what happens there |
|---|---|---|
| `5f74a6076f31` (dev2; 51,516 ticks in Java, 51,790 in the simulator) | 3505 - 3600 | logs identical up to the hopper at 8,7,0 (t=3512). The bot then follows the route `8,7,1 J>7,8,1 D>6,7,1 W>6,7,0 D>5,5,0 W>4,5,0 D>3,0,0 W>2,0,0 W>1,0,0 W>0,0,0`; the scaffolding at 0,0,0 goes in at t=3593 in Java, 3590 in the simulator. Same three ticks as at `f6b21f4`. Freezing is not it (the simulated bot has thawed by 3396) |
| `95a77a6aeceb` (strat-small; 7,960 vs 7,910) | 455 - 535 | t=466: the bot, standing on the repeater at 3,1,5, places the sticky piston at 2,2,5, which extends into the bot's upper half and pushes it east off the repeater. The simulator's bot lands at 4,0,5, walks to 4,0,6 and places 4,1,5 from there at t=486 (sneaking, clicking the repeater's side). Java instead puts a column at 4,0,4 / 4,1,4 (t=486, 509) and places 4,1,5 at t=530. So the two bots stand differently after the push (the push arithmetic itself I have checked against `PistonMovingBlockEntity`). The verbose log with `MCB_WATCH=4,1,5` for the same ticks would help too |

**B2. Trap checks and block changes** - `MCB_EVENTS_MORE=1`: two more kinds in the event log,

```
<tick> W <x>,<y>,<z> <ok> <join> <witnessLearn>   Moves.wayOut, just before `return ok`: the cell asked about,
                                                  the answer, the local `join`, the field witnessLearn
<tick> C <x>,<y>,<z> <state name>                 BuildState.onBlockChange, first line (every change the bot is
                                                  told of, inside the grid or not)
```

for `0cbfffb5c7d0` (strat-small, 139 blocks, 4,877 ticks; about 4,000 `W` and 900 `C` lines). Its row and its
placement log are identical, and so is every counter of its verbose log - 4,414 trap checks, 4,223 searches,
13,964 cells expanded - but `exitJoined` is 3,720 in the Java row and 3,719 in the simulator. With the searches
equal to the cell, the one differing check must be one where joining costs nothing (or the Java counter is not
the same in every run of this build: two plain runs would tell, and would be the cheaper thing to try first).
If it is real, it is a block change that the simulator reports and Java does not (or the reverse) - invisible in
any result so far, and the kind of thing that decides a verdict in some other build.

---

# A. For the port of `2d9a193` (and every later commit): the "port kit"

(All of A3 is delivered and compared; see the status above. Kept as the record of what was asked and why.)

## A1. What I decided

* The port follows a's master commit by commit. The Java source it corresponds to is in the repository
  (`port/java`, with the commit), `tools/port.py status <new src>` lists every changed Java member with the Rust
  function it lands in, and the previous revision stays runnable behind `--rev` as the regression test of the port
  itself. Procedure: `docs/tracking-the-java-bot.md`.
* The route search exists once now (it was a literal copy plus a fast copy); `MCSIM_NAVCHECK=1` checks the fast
  form's cell mirror against values computed afresh.
* a's switches (`MCB_NO_EXIT`, `MCB_NO_NEXT`, `MCB_NO_HAZ`, `MCB_CAREFUL`, `MCB_SEED` ...) work in the simulator
  under the same names, so a switched Java run has its simulator counterpart. Not ported: `MCB_PARANOID`,
  `MCB_TRACE`, `MCB_WATCH` output.
* The result rows carry the two new columns (`worstRatio`, `metrics`) for `2d9a193`, then the simulator's CPU time.

## A2. What the simulator predicts for `2d9a193` (to be confirmed or refuted by A3)

Against the `f6b21f4` reference rows, on all five sets (290 builds): 114 rows unchanged to the tick, 176 changed;
PASS 254 -> 270 (20 FAIL -> PASS, 10 of them in `dev2big`: 15 -> 25; 4 PASS -> FAIL: `e083d65be2f9`,
`f66dfbed0b6b`, `5de09e3d9d94`, `081651c277e2`); leftover cells 8,678 -> 519. Table by set: `docs/scoreboard.md`;
per build: `python3 tools/revdiff.py reference/results/f6b21f4-<set>.tsv tmp/sim-2d9a193-<set>.tsv --list`.
The simulator runs this revision at a median of 7,350x; all 290 builds take 17 CPU-minutes.

## A3. What I need, in order of value

**1. Result rows for `2d9a193`** in the usual format (the 24 columns `RunCorpus` writes now), as
`reference/results/2d9a193-<set>.tsv`:
`small`, `strat-small`, `dev2` first (220 builds, the ones the port was developed against at the old commit);
then `dev3` (hold-out) and `dev2big`. Run as the reference rows were: no `MCB_*` variables set, not verbose.
If a run hits the wall-clock limit, the row's tick count is what I need (as before).

**2. Placement logs for all builds of `small`, `strat-small` and `dev2`** (`ev-<hash>.txt`, the format of
`reference/java-extra/events`: `P`, `S+`, `S-`, `T`, `X`), from the same runs if the hooks cost nothing, so that
rows and logs are one run. With 109 builds predicted to change I cannot say in advance which will differ; having
every log means a differing build is diagnosed the same hour instead of after another round trip. The new code
needs the `P` hook at its two new `placed++` sites (`Placing.placeFromInside`, `Placing.placeClose`). Two more
kinds would help with the new parts (one print each):

```
<tick> H <health>                Body.tick: each time hurts++ (health after the hit)
<tick> K <x>,<y>,<z>             Scaffolds.removeScaffoldImpl: keptForExit++ (the piece kept as the way out)
```

If all 220 logs are too much, this panel first (chosen so that every new mechanism is at work in at least two
builds, by the simulator's own metrics; small builds preferred):

| mechanism (metrics key) | builds |
|---|---|
| helper on a helper (`chained`) | `964a55548e3a`, `9fa5261ee4f2`, `3f54860552a7` |
| from inside the cell (`fromInside`) | `7ab4972a5a18`, `b410f5abe0e9`, `dae063ea0a69` |
| from close beside it, pit (`fromClose`, `pits`) | `0cbfffb5c7d0`, `260ecaaaee57`, `ac13fcd7aa07`, `c49c1a9afcea` |
| relaxed next-cell routes (`relaxedRoutes`) | `ed529613681a`, `d1fd403a4c25` |
| already shut in / unstick (`alreadyShut`, `unstuck`, `shutEpisodes`) | `e44f73e70a27`, `581529a395a2`, `9ebaf2bf08d9` |
| hurt (`hurt`) | `957d5fe620ab`, `896c4378cdfd`, `54f7b9d533cf` |
| retreats and death (`retreats`, `died`; no reference build does either - this one is from the corpus at large: the simulator predicts 12 retreats and status `died` at tick 26,834) | `2642e9f3b1088bb4c2ed` (row and log, please) |
| end game (`forcedWorse`, `tidied`) | `a565f7fe3fa8`, `f3671d58d67c`, `8ec7c0f07776` |
| predicted PASS -> FAIL | `e083d65be2f9`, `f66dfbed0b6b` |
| controls (predicted unchanged from `f6b21f4`) | `81749220581f`, `6cf99326cfdd` |

**3. The verbose log (`RunOne <hash> v`) for the panel builds**, as in `java-extra` - the `PHASE`, `cleanup`,
`end game:` and `unstuck:` lines carry ticks.

**4. For later commits, the same as a routine ("port kit")**, one directory per commit a wants predicted,
`reference/java-<commit>/`:

```
src/                      the full source at <commit>
from                      the commit of the previous kit (the port goes kit to kit)
log                       git log --oneline from..commit
results/<commit>-small.tsv, -strat-small.tsv, -dev2.tsv        (rows; dev3 / dev2big when convenient)
events/ev-<hash>.txt      placement logs of those runs
```

`diff` is not needed (`tools/port.py diff` produces it per member). The cheapest way to make this routine on the
Java side: keep the event hooks in a's master behind an environment variable (`MCB_EVENTS=<dir>`, off by default,
print-only), so that any commit can produce rows and logs in one `RunCorpus` pass. The three development sets
took the Java version about an hour of wall time at `f6b21f4`.

**5. Tell me which switches matter.** If a evaluates candidates with switches (`MCB_NO_NEXT=1` etc.) and wants
those predicted too, rows for one switched configuration on `small` would validate the switch handling.

## A4. What the reviews and checks say (no Java involved)

* `--rev f6b21f4` after the port: 40 / 119 / 59 exact on small / strat-small / dev2, as before the port.
* `MCSIM_NAVCHECK=1` at both revisions on `small`: no mismatch.
* Three independent reviews of the Rust commit against the Java diff (Moves/Nav/Grid, Placing/Stances,
  Builder/Scaffolds/BuildState/Body/BuildRun), each asked for discrepancies only: no behavioural discrepancy
  found; one in the output (`%.3f` of the worst ratio: Java rounds the shortest decimal form half up, so 0.0075
  prints as 0.008 - fixed).
* What reading cannot settle, and the rows will: whether the simulator's model of what hurts (campfire, magma,
  cactus, freezing; `hurt=` in the metrics) is complete now that a hurt bot behaves differently and a dead one
  ends the run; and the parts of vanilla the new code reaches for the first time (clicks from inside a cell and
  from a pit beside it, sneaking slowly to an off-centre spot).

---

# Earlier requests (for `f6b21f4`)

**Status 2026-11-13:** items 1-3 below were delivered (`reference/java-extra/`), thank you - they found five
modelling errors within the hour (see `docs/scoreboard.md`). Open requests are in the section "Open" at the end.

Things that cannot be produced in this container (no JVM). Ordered by how much they would help. All of them are for
commit `f6b21f4`; none needs code changes beyond a print statement.

## 1. Placement log for builds the simulator does not reproduce (highest value)

One line per event, written from `Body`/`BuildState` (or filtered from an `MCB_TRACE` run):

```
<tick> P <x> <y> <z> <state name>      a target or temporary block was placed (every `placed++`)
<tick> S+ <x> <y> <z>                  addScaffold
<tick> S- <x> <y> <z>                  a scaffold cell went (onBlockChange removal)
<tick> T <trip number>                 restock0Impl entered
<tick> X <reason> <x> <y> <z>          skip(...)
```

(build-relative coordinates). With this the first divergent decision can be found by diffing against the simulator's
own log; without it only the final counts can be compared. Builds, most useful first:

| build | why |
|---|---|
| `e52ac89a880c4078a997` (dev2big, 118x24x59) | Java ends early: 10,115 "no way to place", 46 "stranded without material", 2,766 scaffold blocks left; the simulator builds it (23 skipped). Also wanted for this one: the plain verbose log (`RunOne <hash> v`), in particular the first `cannot reach chest`, `FAIL`, `no stance for` and `PHASE` lines with their ticks. |
| `b1dbcdda1c48` (small, 23x9x103, slime/observer machine) | Java 3 x "out of stock", simulator 46: a piston machine consumes blocks differently |
| `896c4378cdfd`, `5f74a6076f31`, `c49c1a9afcea`, `3b87ace388a2` (dev2 / strat-small) | tick differences of 7-17 % with near-identical outcomes |
| `6cf99326cfdd`, `f66dfbed0b6b` | differ by 1 and 8 ticks out of 53,272 / 116,890: a single small timing difference each |

## 2. The placement tables

`Oracle.table(item)` for every block item (1,054 items x 13,824 entries of 16 bits; ~29 MB raw, a few MB deflated),
plus `Oracle.floats(state)` for all 32,366 states (one byte each). The simulator's placement rules are ports of the
vanilla block classes; this would test all of them against the game for the lone-support-block case in one go.

## 3. `states.tsv` additions (cheap, removes hand-derived tables)

Per state: `isRedstoneConductor`, `isSolid`, `hasBlockEntity`, `pushReaction`, `isSuffocating`, the note block
`instrument`, `isFaceSturdy` for all six faces and the three support types, and the default state id of each block.
These are currently re-derived from `Blocks.java` by text analysis.

## 4. Nice to have

* The seed-independent facts of the run environment that the code does not show: where the probe player of
  `Oracle` stands (its position enters `CollisionContext.placementContext`), and the build's world `oy`
  (assumed -60 from the flat generator defaults).
* For one build with redstone (e.g. `e4e384f223e9`): every block change the client is told about
  (`MCB_WATCH`-style, all cells), with ticks - to check update order and timing of the redstone port.

---

# Open

**Status after the delivery of items 1-3:** with the placement logs, five of the eight traced builds are now
reproduced event for event (`e52ac89a`, `c49c1a9a`, `3b87ace3`, `896c4378`, `f66dfbed`); `b1dbcdda` and `6cf99326`
have the same events and differ only in the order of some within one tick (all three give the reference result row). `5f74a607` is the one left (below). The logs were worth far more
than their size: every one of them led to a modelling error within minutes (stale entity box for pressure plates,
sky light for daylight detectors, piston pushing, the order of block updates within a section, interaction shapes).

## 5. Per-tick body trace for one short window

One line per tick from `Body.tick()` (after `c.tick()`):

```
<tick> <x> <y> <z> <vx> <vy> <vz> <onGround> <yaw> <pitch> <kForward><kJump><kSneak><kSprint> <selected slot> <sneaking()>
```

with x/y/z/v as full-precision doubles (`Double.toString`), coordinates absolute or build-relative (say which).
The simulator prints the same with `MCSIM_BODY=<from>,<to>`.

| build | ticks | what differs |
|---|---|---|
| `5f74a6076f31` | 3505 - 3600 | (the earlier difference at t=3404 is solved: the bot was freezing in powder snow, which slows it.) After the hopper at 8,7,0 (t=3512, identical) the bot jumps to 7,8,1 and drops in three stages to the ground at 3,0,0, then walks to 0,0,0; the scaffolding there goes in at t=3593 in Java, 3590 in the simulator. `route:` lines of the window would help too |

## 6. Light data per state (for daylight detectors and crops)

`3b87ace388a2` showed that sky light matters: its inverted daylight detectors stand under a roof, give a signal
there and open the fence gates next to them. The simulator now floods sky light with opacities derived from the
shapes, which reproduces that build; the exact inputs would be: per state `getLightBlock()` (opacity),
`propagatesSkylightDown()`, `useShapeForLightOcclusion()` and, where that is true, the occlusion shape's full faces
(six bits). Also the frozen `dayTime` of the test world (assumed 0).

## 7. More placement logs (same format as item 1)

(`87f8e5fdb864`, asked for here earlier, is solved without a log: the bot dies in a soul campfire. The simulator
now models health, and vibrations for the two builds with sculk sensors.)

For the builds that still differ (current list in `docs/scoreboard.md`, "not exact"). Most wanted, in this order:

| build | set | why |
|---|---|---|
| `81f49223e942` | dev2big | +1.9 % ticks, 132 more scaffold blocks. At t=322,797 (simulator) a falling sand block lands on a piston head and breaks into an item; items are not modelled (their initial velocity is random). Handing the simulated bot that item changes nothing, so the first difference is probably earlier and elsewhere (pistons moving sand); the log will show where |
| `2f7467b7ea64` | dev3 (hold-out) | 61 ticks and 65 placements apart out of 229,386 / 10,940; same outcome otherwise |
| `95a77a6aeceb` | strat-small | slime/piston machine: 50 ticks and 2 placements apart |
