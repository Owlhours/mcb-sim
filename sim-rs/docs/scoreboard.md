# Scoreboard

Speed and fidelity against the Java reference results (`reference/results/f6b21f4-*.tsv`), by commit.
Produced with `sim/target/release/mcsim run <list> <out.tsv>` and `tools/compare.py <out.tsv> <set>`.

Columns: `verdict=` share of builds with the same PASS/FAIL; `exact` builds whose verdict, status, target blocks,
ticks, placed, skipped, missing, wrong and leftover are all identical to the Java run; `tick err` |sim−ref|/ref per
build (median, p90); `agg ticks` error of the summed ticks; `m/w/l` missing/wrong/leftover summed over builds that
fail in either run (reference, then simulator); `RTF` real-time factor per build on one core (game seconds per wall
second, wall time including site set-up, excluding the one-time 0.1 s table load): median, p10, p90, and the
aggregate weighted by wall time. The Java version: median 295×, aggregate 112×.

`dev3` is the hold-out set: compared at milestones only.

## Milestone 1: first complete port (all rules merged), no tuning

| set | n | verdict= | exact | tick err med | p90 | agg ticks | fail builds m/w/l ref | sim | RTF med | p10 | p90 | agg |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| small | 40 | 98% | 30 | 0.0% | 2% | -1.3% | 14/2/0 | 48/1/0 | 2056x | 686x | 3477x | 1183x |
| strat-small | 120 | 99% | 92 | 0.0% | 4% | +0.1% | 82/3/2 | 87/2/2 | 2868x | 1182x | 5676x | 1528x |
| dev2 | 60 | 95% | 45 | 0.0% | 8% | -3.2% | 55/6/0 | 50/2/94 | 1951x | 479x | 4353x | 560x |
