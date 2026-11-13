# Requests for reference data from the Java version

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
| `5f74a6076f31` | 3350 - 3410 | after the two dispensers (t=3350/3351) the bot drops to the ground, walks to the scaffolding column at 9,*,2, sinks through its lowest block and extends the column; the first scaffolding goes in at t=3404 in Java, 3406 in the simulator (which lands at t=3404 and clicks two ticks later) |

## 6. Light data per state (for daylight detectors and crops)

`3b87ace388a2` showed that sky light matters: its inverted daylight detectors stand under a roof, give a signal
there and open the fence gates next to them. The simulator now floods sky light with opacities derived from the
shapes, which reproduces that build; the exact inputs would be: per state `getLightBlock()` (opacity),
`propagatesSkylightDown()`, `useShapeForLightOcclusion()` and, where that is true, the occlusion shape's full faces
(six bits). Also the frozen `dayTime` of the test world (assumed 0).

## 7. More placement logs (same format as item 1)

For the builds that still differ (current list in `docs/scoreboard.md`, "not exact"). Most wanted, in this order:

| build | set | why |
|---|---|---|
| `87f8e5fdb864` | dev2big | **verdict differs**: Java FAIL, 1,093,163 ticks, 12,603 skipped ("no way to place" 12,597, "stranded without material" 6); simulator PASS at 920,750 ticks. 24 sculk sensors (vibrations are being ported now) - the log would show whether the first difference is at a sensor. The verbose log's first `no stance for` / `stranded` lines with ticks would help as well |
| `81f49223e942` | dev2big | +1.9 % ticks, 132 more scaffold blocks. At t=322,797 (simulator) a falling sand block lands on a piston head and breaks into an item; items are not modelled (their initial velocity is random). If the Java log agrees up to about that tick, the item is the cause: then please add one line per item pick-up (`<tick> I <item> <count>`) and where the item entity came to rest |
| `2f7467b7ea64` | dev3 (hold-out) | 61 ticks and 65 placements apart out of 229,386 / 10,940; same outcome otherwise |
| `0cbfffb5c7d0` | strat-small | calibrated sculk sensors (vibrations are not modelled): 24 ticks apart |
| `95a77a6aeceb` | strat-small | slime/piston machine: 50 ticks and 2 placements apart |
