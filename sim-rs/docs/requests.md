# Requests for reference data from the Java version

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
