# fastsim: a speed-first approximation of the building bot

An event-driven simulator of the same task as `mcsim`, made only for speed, to try algorithms on: build order,
stance and scaffold planning, route search, supply trips, clean-up. It doesn't model Minecraft at all. It models
`mcsim`'s bot on a coarse world, with its constants fitted to `mcsim`'s logs.

- **Speed.** About 1,000,000× real time per core on the median development build: 1.06M× on 140 development
  builds and 1.12M× on 753 triage builds, measured on an idle Ryzen 5 5600X core. `mcsim` does 3,000–7,000× on
  the same builds. All 170 development builds run in under a second.
- **Agreement with `mcsim`** (seed 0; verdicts against `mcsim` at the bot's revision of 11-20, `d650a66` in the
  development repository):
  - **Game time**, summed per set: 0.89 / 0.81 / 0.87 / 0.90 (small / dev2 / dev3 / dev2big).
  - **Verdicts:** 139 / 140 on the development lists, 27 / 30 on dev2big, 90 / 100 on huge builds, but only
    292 / 753 on the triage set of small, hard builds. Two of `mcsim`'s own seeds agree on 99 % there.
- **What it's good for.** It ranks the game-time effects of order and routing changes on ordinary builds: 3 of 4
  such effects carried over to `mcsim`, and the effects correlate 0.77–0.85. It does **not** rank them on large
  builds, and it does **not** predict which builds fail. On the strict test, no fast level beats "nothing
  changes"; only level 2 with the `virtual` screen (about 3,100×) and level 3 (`mcsim`) do.

Treat a win in fastsim as a hypothesis to test in `mcsim`.

## Running

```bash
harness/run.sh tables                       # once: work/reference/mc/states.tsv (and java-extra/)
harness/run.sh prep my-schematics/*.litematic && cp work/corpus/prep-list.txt work/mylist.txt
fastsim/prep.sh work/mylist.txt work/mylist.fsb   # builds, writes work/fastsim/{states.tsv,click.bin}, packs the list
fastsim/target/release/fastsim run work/mylist.fsb
#   395c354eddde9cbecfde  PASS  done  2883  68793  3370  1  1  0  0  8  0  6  480  <sim ns>  <setup ns>  <rtf>  main=55871;...
#   builds=9 median_rtf=335002 p10=300848 p90=544277 aggregate_rtf=328922 cpu_ms=243.8     (stderr)
fastsim/target/release/fastsim run work/mylist.fsb --level 3     # every build in the linked copy of mcsim: mcsim's rows
fastsim/target/release/fastsim run work/mylist.fsb --level 2 --screen virtual   # fast screen, exact rerun of flagged builds
```

**Row columns:** hash, verdict, status, judged blocks, ticks, blocks placed (target plus scaffolding), skipped,
missing, 0, leftover, sections, failed sections, trips, scaffolding placed, simulation ns, set-up ns, real-time
factor, then `key=value` counters. `--trace` prints the event lines (`P` placed, `DF` put off, `M` pass markers,
`SK` stance chosen, and others), and `FS_PHSTAT=1` / `FS_SCSTAT=1` print per-pass and scaffolding statistics on
stderr.

**Knobs.** Every switch is in `src/main.rs`:
- `--cost name=v,...` sets the planner's weights and the tick table (`walk`, `jump`, `tower`, `bridge`, `t_walk`,
  `t_place`, `t_chest_open`, `t_wfail` ...).
- `--seed K` changes the draws.
- `--level 1|2|3` and `--screen bad|virtual` choose how much of `mcsim` to use.
- Switches named after the `mcsim` rule they turn off or bring back in: `--no-orphan`, `--no-close`,
  `--no-chain`, `--strict-helpers`, `--no-fail`, `--sweep-waiting`, `--centre-reach`, `--retry-budget N`, and
  others.

Each STATUS entry of development named the switches that give an earlier binary's rows.

`fastsim kinds <list>` prints the block kinds of each build (used for the failure model), and
`fastsim replay <set.fsb> <dir>` asks fastsim's world the questions `mcsim`'s event logs record.

**Build note.** `.cargo/config.toml` builds with `-C target-cpu=native`. Development used `znver3`, which gives
the same code on that CPU. The binary is not portable between CPUs. `mcsim-ref/` is fastsim's own copy of
`mcsim`, from its development, with fastsim's instrumentation and knobs added (event-log lines, `dump_states`,
`dump_click`, `fsoracle.rs`). It is not the same as `sim-rs/sim`: its newest revision is `thoughtway` (11-17), so
`--level 3` gives `mcsim`'s rows at that revision, not at `covertour`. They match `sim-rs` only on builds that the later
revisions don't touch, such as the timber-frame house: 68,361 ticks in both.

## How it works

- **Clock.** Events, not ticks. A route costs `5 + 4·walk + 12·jump + 12·drop + 14·tower + 10·climb + 9·descend +
  18·bridge` ticks (fitted to 11,144 routes of `mcsim`, R² 0.958). Arriving costs 14 and a placement 1. Chest,
  breaking and failure times were fitted later, in rounds against `mcsim`'s event logs.
- **World.** For each column and each 64-cell word, eight 64-bit planes sit together in one cache line: solid,
  scaffold, occupied, needed, target, loose, and others. Standing, falls and "the column's next cell" are
  single-word bit operations. The target is one palette entry per cell, giving class bits, item, stack size and
  items per block.
- **Route search.** A* over standing cells with walk, jump, drop, tower, climb, bridge and break moves. It is
  bounded (24 + 4 per cell of distance, weighted estimate 1.5) and uses a stamped open-addressed node table, so
  the working set doesn't grow with the world. A route it doesn't find is charged from the cost table.
- **Policy.** `mcsim`'s bot, coarsely: columns bottom-up; a column eligible when no neighbour column has
  something lower to do; nearest eligible cell first, with `mcsim`'s penalties; stances near the cell or a
  scaffold column; trips loaded for the next cells; put-off passes; clean-up. Most of `mcsim`'s later rules came
  in as defaults, each with a switch back.
- **Clicks.** A cell can be clicked when a neighbour face looks toward the eye within reach (4.15 planning, 4.5
  clicking), and one of five points on it has a clear line from the eye. What a block needs in order to come out
  right is a 48-bit mask per state (6 faces × 8 view octants) taken from `mcsim`'s placement tables
  (`click.bin`).

## Simplifications and their likely bias

Each item is a place where an algorithm comparison in fastsim could come out differently in `mcsim`.

1. **No physics, no ticks.** Moves cost fixed ticks by kind. An algorithm that trades many short routes for few
   long ones is judged by the fitted per-route constant.
2. **Blocks are full cubes or nothing.** Slabs, stairs, fences and panes can all be stood on at full height, and
   doors and gates are never in the way. fastsim finds stands and ways that the bot doesn't have, so algorithms
   that depend on tight spaces are flattered.
3. **Clicking is geometry-light.** Part of the face, pitch and sneaking are ignored in the masks.
4. **Unfound routes come from the cost table.** The bot can't be shut in and never fails to get anywhere. Mazes,
   sealed rooms and long detours are too cheap, and the bot's trap checks, break-outs and repairs have no
   counterpart.
5. **Helpers.** A cell with no usable face gets a loose block lent to it. Where even that has nothing to stand
   against, a pair is charged without asking where the second goes.
6. **The clean-up never leaves scaffolding.** Unreachable pieces get a stand mined beside them and are charged.
   A clean-up strategy can lose only time here, never a build.
7. **Loose blocks stay until the clean-up.** The bot takes its bridges away before it walks on. This changed
   on 11-20: `--keep-bridges` gives the old way.
8. **Trips.** The load is planned as the bot plans it, but the scan stops 16 rings after the slots are full, and
   unwanted items go back at no cost.
9. **Second halves** of two-cell blocks count as placed from the start.
10. **Waiting cells** are retried when one of their 26 neighbours changes, as in the bot, and put off after 12
    tries.
11. **No machines.** Moving redstone, falling blocks after placement, fluids and growth don't exist.
12. **The judge** counts a target cell as right or missing. "Wrong block" doesn't occur, and leaves are judged
    strictly.

## What was found with it

- **The cheat ladder** turns one limitation off at a time.
  - **Clicking is the bottleneck.** With the cheats on, the failing large list climbs from 30 to 47 of 57 passes,
    and huge33 from 11 to 27 of 33.
  - **The +11 passes split** (Shapley) as air 4.9 and sight 3.2, with reach, teleport, no-clean-up and stock at
    0.3–1.2 each.
  - **Ten of 57 still fail** with every cheat on: five hold cells that can't exist, and five are the bot's own
    doing (pistons firing while half built, sand on tripwire, and three others).
- **Oracle studies.**
  - **The bot fails, not the targets.** A checker calls 22 of 27 failures on the failing list buildable, and 55
    of 61 on huge-dev100.
  - **Oracles per decision gain little,** only +2 to +4 passes, and most "search cut short" labels were wrong.
  - **Three targets were handed back to the bot.** Put off a powered part while its power disagrees with its
    target state. Let a gravity block stand on a walk-through block. Fix the "cut short" label.
- **Fix prototypes for the bot** (`--retry-budget`, the rope's feed, clean-up order) were measured in fastsim
  and in patched copies of `mcsim`. Their write-ups were not published with this repository; the commit
  messages carry their numbers.

## Open when the project closed (11-21)

1. **Huge builds' main pass** takes ×1.09 of `mcsim`'s time where both finish, although fastsim lays only part of
   `mcsim`'s rope. `--retry-budget 400` lays it, but loses verdicts.
2. **fastsim's straight walks lay half of `mcsim`'s rope blocks.** Its loops lay 0.68 blocks each, against
   `mcsim`'s 1.16.
3. **`mcsim`'s column stances in the clean-up are missing.** On one build, fastsim's clean-up takes 62k ticks
   against `mcsim`'s 411k.
4. **A verdict screen cheaper than `virtual`** was never found.
5. **A bucket queue for the search** (`FS_BUCKETQ=1`, off) saves 2–3 % of cycles.
