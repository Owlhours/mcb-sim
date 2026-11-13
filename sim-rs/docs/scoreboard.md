# Scoreboard

Speed and fidelity against the Java reference results (`reference/results/f6b21f4-*.tsv`), by commit.
Produced with `tools/mklists.py`, `sim/target/release/mcsim run /work/tmp/<set>.txt <out.tsv>` and
`tools/compare.py <out.tsv> <set>`.

Columns: `verdict=` share of builds with the same PASS/FAIL (strict reading; the wide reading and the
by-volume reading are reported by the simulator too and agree with Java on the same builds); `exact` builds whose verdict, status, target blocks, ticks, placed, skipped, missing, wrong and
leftover are all identical to the Java run; `tick err` |sim-ref|/ref per build (median, p90); `agg ticks` error of
the summed ticks; `m/w/l` missing/wrong/leftover summed over builds that fail in either run (reference, then
simulator); `RTF` real-time factor per build on one core: game seconds per second of **CPU time** of the build
(site set-up included, the one-time 0.1 s table load excluded): median, p10, p90, and the aggregate (total game time
/ total CPU time, i.e. weighted by time). `agg (wall)` is the same with wall-clock time. All runs were made on the
shared two-core container with a load average of 4-15 (other tenants and two simulator runs at a time), so the speed
columns are pessimistic and vary by about +-30 % between runs; fidelity columns are deterministic.

The Java version on the same sets (its own wall time, its own machine): median 212-326x, aggregate 97-326x
(small 291x / 194x, strat-small 326x / 326x, dev2 279x / 235x, dev3 266x / 112x, dev2big 212x / 97x).

`dev3` is the hold-out set. It was not run before milestone 2; up to its last hold-out comparison (milestone 3) it
was compared three times in full (37, 39, 39 of 40 exact) and once part-way, and only its summary line and the names of the differing
builds were looked at - no dev3 build was traced, and the two that became exact did so through errors found on the
other sets.
`dev2big` (30 builds of 8k-80k blocks) is the stress set.

## `2d9a193` against Java: first reference rows (small, strat-small, dev2)

Rows and placement logs for the three development sets arrived from the Java side
(`reference/results/2d9a193-*.tsv`, `reference/java-2d9a193/events`). The port had been made and reviewed without
them; no change was made to it before this comparison (simulator commit `022960a`; `REV=2d9a193 tools/runsets.sh`,
`REV=2d9a193 tools/evall.sh`).

| set | n | verdict= | exact | metrics= | tick err med | p90 | agg ticks | fail builds m/w/l ref | sim | RTF med | p10 | p90 | agg |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| small | 40 | 100% | 40 | 40/40 | 0.0% | 0% | +0.0% | 5/1/0 | 5/1/0 | 5051x | 3346x | 8798x | 4938x |
| strat-small | 120 | 100% | 119 | 118/120 | 0.0% | 0% | -0.0% | 77/2/1 | 77/2/1 | 6267x | 3733x | 9458x | 5129x |
| dev2 | 60 | 100% | 59 | 59/60 | 0.0% | 0% | +0.0% | 29/2/0 | 29/2/0 | 7202x | 2698x | 9543x | 3030x |
| **all** | 220 | **100%** | **218** | 217/220 | | | | | | | | | |

(Speed measured while the machine was busier than for the table below.) `metrics=`: builds whose 23 counters of
the new `metrics` column all agree as well. Placement logs, event for event: 175 of 220 identical; 41 more have
the same events with another order within a tick (scaffolding columns coming down, in the order of the server's
section packets); 2 differ only in the random age of vines; 2 differ.

Not exact (same verdict; largest tick error 0.6 %):

| build | Java | simulator | what is known |
|---|---|---|---|
| `95a77a6aeceb` | 7,960 ticks, 276 placed | 7,910, 274 | inexact at `f6b21f4` too. First difference t=486: a sticky piston pushes the bot off a repeater, and the two bots then choose different places to stand |
| `5f74a6076f31` | 51,516 ticks, 1,732 placed | 51,790, 1,741 | inexact at `f6b21f4` too. First difference t=3590/3593: three ticks in a jump and three drops |
| `0cbfffb5c7d0` | exact row and log; `exitJoined` 3,720 | 3,719 | one trap check differs: probably a block change of the build's sculk / observer machine a tick apart |

Traces that will locate each are asked for in `docs/requests.md`, section B. dev3 and dev2big rows are still
to come; the predictions for them are in the next section.

## Port of the Java bot at `2d9a193` (simulator commit `0d7bbd6` and after) - predictions, not yet compared

a's master of 2026-11-13 (27 commits after `f6b21f4`: remembered ways out, hazards and a health guard, last-resort
placements, a new end game). **No Java results exist for this commit yet**, so this section has no fidelity
columns: it records what the simulator predicts, to be confirmed or refuted by the rows asked for in
`docs/requests.md` (section A). What is established without Java:

* `--rev f6b21f4` with the same binary gives the table of milestone 3 unchanged (40 / 119 / 59 exact on small /
  strat-small / dev2 re-run after the port; the route search is one function now instead of two).
* `MCSIM_NAVCHECK=1` on `small` at both revisions: no mismatch.
* Three independent reviews of the port against the Java diff: no behavioural discrepancy.
* All 290 reference builds and 400 unseen ones run at the new revision without a crash.

Predicted change from `f6b21f4` to `2d9a193` (`tools/revdiff.py reference/results/f6b21f4-<set>.tsv
tmp/sim-2d9a193-<set>.tsv`), and the simulator's speed at the new revision (CPU time, as below; the machine was
quieter than for milestone 3 - the old revision measured the same hour: medians 4,943x / 6,654x / 5,978x on the
first three sets):

| set | n | rows unchanged | PASS before -> after | FAIL->PASS | PASS->FAIL | sum of ticks | RTF med | p10 | p90 | agg |
|---|---|---|---|---|---|---|---|---|---|---|
| small | 40 | 23 | 38 -> 39 | 1 | 0 | +0.2% | 7599x | 4669x | 9102x | 6691x |
| strat-small | 120 | 60 | 113 -> 114 | 2 | 1 | -7.6% | 7907x | 5345x | 10696x | 6682x |
| dev2 | 60 | 28 | 55 -> 56 | 2 | 1 | +3.5% | 7680x | 3367x | 9879x | 4133x |
| dev3 | 40 | 2 | 33 -> 36 | 5 | 2 | -7.1% | 5269x | 1297x | 8351x | 2515x |
| dev2big | 30 | 1 | 15 -> 25 | 10 | 0 | +1.1% | 3070x | 802x | 7256x | 1592x |
| **all** | 290 | 114 | 254 -> 270 | 20 | 4 | -0.8% | 7347x | 2766x | 9858x | 1938x |

Leftover cells over all builds 8,678 -> 519, missing 113,318 -> 42,602. All 290 builds take 17 CPU-minutes at
the new revision (43 at the old one, on a busier machine): the remembered way out answers most trap checks
without a search, and that saving of the Java bot carries over to its port.

Predicted to fail where `f6b21f4` passed: `e083d65be2f9`, `f66dfbed0b6b`, `5de09e3d9d94`, `081651c277e2`.
Predicted to pass where it failed, among others: `e52ac89a880c` (the 59,636-block build that ended early),
`f20cfe460b34` and `02dc970ddf0a` (both named in a's own comments as what the change was for), `87f8e5fdb864`
(the bot no longer walks into the soul campfire). `3aa40858` is cut off at the tick of the *old* commit's wall
limit - where the new one hits it, only the Java row can say.

Mechanisms at work (builds of 290, by the `metrics` column): remembered way out joined 290, traps avoided 136,
helper on a helper 38, already shut in 23, unstick 23, end-game tidy 20, placed from close by 15 (4 with a pit),
from inside 9, relaxed routes 9, hurt 9, way out unknown 6. No reference build makes the bot retreat to recover;
of the 400 unseen builds one does (`2642e9f3b108`: 12 retreats, 1,469 hits, and it still dies at tick 26,834 -
status `died`). Unseen builds: 400 run, 360 PASS (347 at `f6b21f4`), median 7,350x, p10 3,335x.

## Milestone 3: commit `ded12c6` - vibrations, health and death, freezing

| set | n | verdict= | exact | tick err med | p90 | agg ticks | fail builds m/w/l ref | sim | RTF med | p10 | p90 | agg | agg (wall) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| small | 40 | 100% | 40 | 0.0% | 0% | +0.0% | 14/2/0 | 14/2/0 | 3347x | 1766x | 4844x | 2669x | 2639x |
| strat-small | 120 | 100% | 119 | 0.0% | 0% | -0.0% | 82/2/2 | 82/2/2 | 4336x | 2521x | 6751x | 3221x | 3129x |
| dev2 | 60 | 100% | 59 | 0.0% | 0% | +0.0% | 30/0/0 | 30/0/0 | 3540x | 1512x | 5189x | 1632x | 1626x |
| dev3 (hold-out until this milestone, see below) | 40 | 100% | 40 | 0.0% | 0% | +0.0% | 11388/12/350 | 11388/12/350 | 1855x | 224x | 4138x | 431x | 425x |
| dev2big | 30 | 100% | 29 | 0.0% | 0% | +0.0% | 100720/284/8131 | 100720/284/8131 | 1235x | 378x | 4027x | 828x | 820x |
| **all** | 290 | **100%** | **287** | 0.0% | 0% | +0.02% | | | 3550x | 879x | 5994x | 770x | |

All 290 verdicts agree (strict, wide and by-volume reading alike); 287 builds are identical in every count and in
the tick count; the largest tick error of the other three is 1.9 %; the missing / wrong / leftover totals of the
failing builds are identical. All 290 builds take 43 minutes of CPU time on one core; the Java reference runs took
300 minutes of wall time.

**The hold-out.** At commit `00e3f70` (health and vibrations in, nothing else changed since) dev3 was compared as a
hold-out for the last time: 39 of 40 exact, 40 of 40 verdicts. Only then was its one differing build
(`2f7467b7`) examined. It has a turtle egg, on which the game draws a 1-in-100 chance per tick from a
clock-seeded random source; the simulator's own pseudo-random stream broke an egg, the Java run did not. The
simulator now takes the likeliest outcome of such a draw (no egg breaks), which reproduces that build exactly.
The 40/40 in the table therefore includes one build fixed by looking at it; 39/40 is the clean figure.

### What changed since milestone 2

| build | was | cause | now |
|---|---|---|---|
| `87f8e5fdb864` (dev2big) | the one verdict difference: Java FAIL with 12,603 cells skipped, simulator PASS | **the bot dies.** It stands in a lit soul campfire; health, food and saturation, regeneration, damage and the state of a dead player (inventory gone, packets ignored, body removed after 20 ticks) were not modelled. Found without Java data: the simulated bot had placed exactly Java's final 8,345 blocks when it stepped into the fire | exact (1,093,163 ticks) |
| `0cbfffb5c7d0` (strat-small) | 24 ticks | sculk sensor vibrations (game events, listeners, selector, travel time) | exact |
| `2f7467b7ea64` (dev3) | 61 ticks, 65 placements | random turtle-egg trampling (above) | exact |
| `5f74a6076f31` (dev2) | 297 ticks | freezing in powder snow slows the player: first difference moved from t=3404 to t=3590 | still 271 ticks (0.5 %) off |

### Not exact (3)

| build | set | Java | simulator | what is known |
|---|---|---|---|---|
| `81f49223e942` 92x64x93 | dev2big | FAIL, 373,334 ticks, 34 missing, 3,767 scaffold | FAIL, 380,245 ticks, 34 missing, 3,899 scaffold | +1.9 % ticks. A piston machine with sand. A falling sand block breaks into an item at t=322,797 (items are not modelled), but handing the simulated bot that item changes nothing. Placement log requested |
| `5f74a6076f31` 23x28x2 | dev2 | PASS, 52,226 ticks | PASS, 52,497 ticks | identical up to t=3512; then a jump and three drops to the ground take 3 ticks less in the simulator. Body trace requested |
| `95a77a6aeceb` 13x7x8 | strat-small | PASS, 7,960 ticks, 276 placed | PASS, 7,910 ticks, 274 placed | slime/piston machine running while being built; not the coral fans' random death delay (all tried draws give the same run). Placement log requested |

### What the run log flags

`unmodelled=` (events the simulator knows it does not model), `lost=` (blocks that broke into items),
`random=` (draws the Java run takes from its clock-seeded random source: its own repeats may differ) and `DIED`.
In the 290 reference builds: 2 builds with unmodelled events (one exact, one is `81f49223`), 4 with random draws
(three exact; `95a77a6a` has 6 coral draws that do not matter), 1 death.

### Builds outside the reference sets (robustness, no ground truth)

400 corpus builds of 20-6,000 blocks drawn at random from the 46,765 that are not in any reference set
(`data/sets/rand400.txt`): all 400 run to `done`, none crashes; 347 PASS (87 %, the reference sets have 88 %); in
one the bot dies (`2642e9f3`, 4,692 cells missing); one reports unmodelled events, three random draws. RTF median
3,571x, p10 1,360x, p90 5,692x, aggregate 1,737x. The slowest (`5ef10f34`, a 25x92x22 tower that ends early with
3,944 cells unplaceable) runs at about 100x: its time goes into the bot's stance planning, which pretends a
scaffolding column up to 90 cells tall for every candidate.

## Milestone 2: commit `4aaa496` (+ `--stop-at`), after the placement logs from the Java side

| set | n | verdict= | exact | tick err med | p90 | agg ticks | fail builds m/w/l ref | sim | RTF med | p10 | p90 | agg | agg (wall) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| small | 40 | 100% | 40 | 0.0% | 0% | +0.0% | 14/2/0 | 14/2/0 | 3565x | 1651x | 6226x | 2706x | 2077x |
| strat-small | 120 | 100% | 118 | 0.0% | 0% | -0.0% | 82/2/2 | 82/2/2 | 4240x | 2425x | 7212x | 3217x | 1716x |
| dev2 | 60 | 100% | 59 | 0.0% | 0% | +0.0% | 30/0/0 | 30/0/0 | 3558x | 1483x | 6735x | 1628x | 1469x |
| **dev3 (hold-out)** | 40 | 100% | 39 | 0.0% | 0% | -0.0% | 11388/12/350 | 11388/12/350 | 2004x | 281x | 4122x | 511x | 391x |
| dev2big | 30 | 97% | 28 | 0.0% | 0% | -0.6% | 100720/284/8131 | 88205/285/6952 | 1106x | 398x | 3485x | 870x | 664x |
| all | 290 | 99.7% | 284 | 0.0% | 0% | | | | | | | | |

284 of 290 builds are reproduced exactly - every count and the tick count, up to 2.87 million ticks. The verdict
differs on one build. (`3aa40858` in dev2big was cut off by the Java harness's wall-clock limit after 2,868,032
ticks; the simulator is told that tick - `tools/mklists.py`, `--stop-at` - and is identical there. Left alone it
finishes the build at 3,321,546 ticks.)

### Not exact (6)

| build | set | Java | simulator | what is known |
|---|---|---|---|---|
| `87f8e5fdb864` 74x29x62, 19,388 blocks | dev2big | FAIL, 1,093,163 ticks, 12,603 skipped, 12,538 missing, 1,179 leftover | PASS, 920,750 ticks, 23 skipped | **the one verdict difference.** Java ends early ("no way to place" x 12,597, "stranded without material" x 6). The build has 24 sculk sensors; vibrations are not modelled (1,048 steps on sensors counted as `unmodelled`). Placement log requested |
| `81f49223e942` 92x64x93 | dev2big | FAIL, 373,334 ticks, 34 missing | FAIL, 380,245 ticks, 34 missing | +1.9 % ticks, 132 more scaffold blocks. A falling sand block breaks into an item at t=322,797 and item entities are not modelled, but handing the simulated bot that item (at five different ticks) changes nothing, so the cause is probably elsewhere in this piston machine. Placement log requested |
| `5f74a6076f31` 23x28x2 | dev2 | PASS, 52,226 ticks | PASS, 52,523 ticks | first difference at t=3404: the simulator starts extending a scaffolding column two ticks later; body trace requested |
| `0cbfffb5c7d0` 6x7x6 | strat-small | FAIL, 5,167 ticks | FAIL, 5,191 ticks | calibrated sculk sensors (as above) |
| `95a77a6aeceb` 13x7x8 | strat-small | PASS, 7,960 ticks, 276 placed | PASS, 7,910 ticks, 274 placed | slime/piston machine running while being built; not the coral fans' random death delay (all 16 tried combinations give the same run). Placement log requested |
| `2f7467b7ea64` 29x27x38 | dev3 | PASS, 229,386 ticks, 10,940 placed | PASS, 229,325 ticks, 10,875 placed | hold-out: not investigated |

Simulator-side flags predict three of the six: every build with `unmodelled` > 0 in the run log (four builds in
all 290) is in this table except one (`6704725e`, two lost items, exact anyway).

### Speed/fidelity trade-off (measured, not shipped as a mode)

`MCSIM_LOSSY=k` makes every route search give up after 1/k of the budget the bot would give it - the cheapest way
to buy speed, since 50-90 % of the time is route search. dev2, same machine:

| | verdict= | exact | tick err med | p90 | agg ticks | fail builds m/w/l (ref 30/0/0 at k=1) | RTF med | agg |
|---|---|---|---|---|---|---|---|---|
| faithful (k=1) | 100% | 59 | 0.0% | 0% | +0.0% | 30/0/0 | 3558x | 1628x |
| k=2 | 98% | 27 | 0.3% | 13% | -2.4% | 35/2/2 | 3607x | 1611x |
| k=4 | 97% | 19 | 2.2% | 13% | -6.3% | 1348/6/3055 | 5422x | 2307x |
| k=16 | 92% | 14 | 4.8% | 20% | -12.0% | 129/5/129 | 4580x | 2491x |

At most 1.5x for a clear loss: the bot's decisions hang on which searches fail, so a cheaper search is a different
bot. The speed that was gained (4-12x the Java version per core) all comes from changes that cannot alter a
result; see `architecture.md` section 8 for the list, including the ones that were measured and dropped.

## Milestone 1: first complete port (all rules merged), no tuning

| set | n | verdict= | exact | tick err med | p90 | agg ticks | fail builds m/w/l ref | sim | RTF med | p10 | p90 | agg |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| small | 40 | 98% | 30 | 0.0% | 2% | -1.3% | 14/2/0 | 48/1/0 | 2056x | 686x | 3477x | 1183x |
| strat-small | 120 | 99% | 92 | 0.0% | 4% | +0.1% | 82/3/2 | 87/2/2 | 2868x | 1182x | 5676x | 1528x |
| dev2 | 60 | 95% | 45 | 0.0% | 8% | -3.2% | 55/6/0 | 50/2/94 | 1951x | 479x | 4353x | 560x |
