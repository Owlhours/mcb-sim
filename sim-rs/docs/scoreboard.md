# Scoreboard

Speed and fidelity against the Java reference results (`reference/results/f6b21f4-*.tsv`), by commit.
Produced with `tools/mklists.py`, `sim/target/release/mcsim run /work/tmp/<set>.txt <out.tsv>` and
`tools/compare.py <out.tsv> <set>`.

Columns: `verdict=` share of builds with the same PASS/FAIL (strict reading; the wide reading and the
by-volume reading are reported by the simulator too and agree with Java on the same 289 builds); `exact` builds whose verdict, status, target blocks, ticks, placed, skipped, missing, wrong and
leftover are all identical to the Java run; `tick err` |sim-ref|/ref per build (median, p90); `agg ticks` error of
the summed ticks; `m/w/l` missing/wrong/leftover summed over builds that fail in either run (reference, then
simulator); `RTF` real-time factor per build on one core: game seconds per second of **CPU time** of the build
(site set-up included, the one-time 0.1 s table load excluded): median, p10, p90, and the aggregate (total game time
/ total CPU time, i.e. weighted by time). `agg (wall)` is the same with wall-clock time. All runs were made on the
shared two-core container with a load average of 5-9 (other tenants and two simulator runs at a time), so the speed
columns are pessimistic and vary by about +-30 % between runs; fidelity columns are deterministic.

The Java version on the same sets (its own wall time, its own machine): median 212-326x, aggregate 97-326x
(small 291x / 194x, strat-small 326x / 326x, dev2 279x / 235x, dev3 266x / 112x, dev2big 212x / 97x).

`dev3` is the hold-out set. It was not run before milestone 2; during milestone 2 it was compared twice (37,
then 39 of 40 exact) and only its summary line and the names of the differing builds were looked at - no dev3 build
was traced, and the two that became exact did so through errors found on the other sets.
`dev2big` (30 builds of 8k-80k blocks) is the stress set.

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
| `81f49223e942` 92x64x93 | dev2big | FAIL, 373,334 ticks, 34 missing | FAIL, 380,245 ticks, 34 missing | +1.9 % ticks. A falling sand block breaks into an item at t=322,797; item entities are not modelled (their start velocity is random in the game) |
| `5f74a6076f31` 23x28x2 | dev2 | PASS, 52,226 ticks | PASS, 52,523 ticks | first difference at t=3404: the simulator starts extending a scaffolding column two ticks later; body trace requested |
| `0cbfffb5c7d0` 6x7x6 | strat-small | FAIL, 5,167 ticks | FAIL, 5,191 ticks | calibrated sculk sensors (as above) |
| `95a77a6aeceb` 13x7x8 | strat-small | PASS, 7,960 ticks, 276 placed | PASS, 7,910 ticks, 274 placed | slime/piston machine running while being built; not the coral fans' random death delay (all 16 tried combinations give the same run). Placement log requested |
| `2f7467b7ea64` 29x27x38 | dev3 | PASS, 229,386 ticks, 10,940 placed | PASS, 229,325 ticks, 10,875 placed | hold-out: not investigated |

Simulator-side flags predict three of the six: every build with `unmodelled` > 0 in the run log (four builds in
all 290) is in this table except one (`6704725e`, two lost items, exact anyway).

### Builds outside the reference sets (robustness, no ground truth)

400 corpus builds of 20-6,000 blocks drawn at random from the 46,765 that are not in any reference set
(`/work/tmp/rand400.txt`, seed 7): all 400 run to `done`, none crashes; 348 PASS (87 %, the reference sets have
88 %); RTF median 3,901x, p10 1,549x, p90 7,803x, aggregate 1,980x; two builds report unmodelled events.
The slowest (`5ef10f34`, a 25x92x22 tower that ends early with 3,944 cells unplaceable) runs at 137x: its time
goes into the bot's stance planning, which pretends a scaffolding column up to 90 cells tall for every candidate.

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
