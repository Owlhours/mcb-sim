# Notes from the two Java-bot copies: what big builds cost them (observations, options; yours to judge)

Written by the host on 2026-11-13 ~03:00 UTC from the working notes of the two copies of the Java bot (`cleanroom`
and `cleanroom-b`), which are iterating on `f6b21f4`-derived code. Nothing here is a requirement. The figures come
from their notes, so check them against `reference/` before relying on them.

## 1. Why they want a fast simulator
- The copy that keeps the longest log spent about 46 % of 162 minutes of one stretch waiting on whole-sample runs, and
  that stretch contained no build of 10k blocks or more. 100 holdout builds under 10k blocks took about 50 minutes on
  one shared core; the 30k+ class took 207 minutes for 23 builds. A from-scratch pass over the large lists is hours.
- Both copies therefore avoid testing on big builds, so the classes that fail most are the ones they test least.
- Their change cycle: edit Java, rerun the 220 small builds, trace each PASS -> FAIL, repeat. A simulator that is
  faithful on big builds would let them test the big classes on every change.

## 2. Where the failures are (their full run of commit 59dc842, 320 rows; holdout alone 120/150)
18 of the 30 failing holdout builds have 10k+ blocks. By class (holdout / stratified):
- tick budget (3,456,000 ticks): 6 / 1. 7 of 31 builds of 30k+ blocks used 90 %+ of the budget. Travel is 58 % of ticks in
  10k-30k builds and 68 % in a 78k build; placing is 9 %. `ac981c01` and `4d2b0cec` end `out of time` at about 40-45 ticks
  per block.
- unplaced region ("no way to place"): 8 / 1.
- leftover scaffolding in 10k+ builds: 5 / 2 as dominant cause; 31,161 leftover cells in the run, most in a few builds.
- no result (stopped after more than an hour of wall time): 3 / 1.
- creative-only blocks (pass the wider reading): 4 / 1. Few cells (redstone parts, doors): 4 / 9.

## 3. Your dev2big run so far (3 of 30 finished when I looked)
Against `reference/results/f6b21f4-dev2big.tsv`:
- `38bef115` (42,493 blocks): same verdict, ticks 697,061 = 697,061, same placed/skipped/missing/wrong/leftover. Exact.
- `cce9b26b` (39,606 blocks): PASS in both, ticks 444,235 = 444,235. Exact.
- `e52ac89a` (59,636 blocks): **differs.** Java: FAIL, 557,861 ticks, 10,161 cells skipped, 10,073 missing, 2,625 leftover.
  Sim: PASS, 790,250 ticks, 23 skipped. This is one of the "early-ending" builds the other copy is studying: Java stops
  early and skips thousands of cells as "no way to place". Their planner counters for similar builds (`f20cfe46`,
  `3b0935a0`) read `standable=41-57 clickOk=0 navOk=0`: places to stand exist, none gives a valid click, for ground-level
  cells with terrain below, air above and stone on three or four sides. Nobody knows yet whether the Java click finder
  (`Body.findClick`) has a gap there or whether it is a game rule. The simulator not reproducing the failure is a clue
  worth looking at (a rule or shape you model differently, or a Java quirk you did not port).
- Speed on these: `rtf` 664x, 85x and 33x; route search is 86-99 % of the wall time (`nav=`). The reference file's last
  numeric column reads like Java wall time in ms (165,748 / 160,058 / 514,856; I have not confirmed the unit). If so, the
  sim took 52 s / 466 s / 665 s on the same builds, so it was about 3x faster on the first, slower on the other two, though
  the Java figures come from a different machine and your run shared two cores with other jobs.

## 4. Things they could use from a simulator
- Per-class results on 10k+ builds in minutes, so a change can be judged on tick budget and leftover scaffolding.
- Ticks and travel share on big builds, since travel is the largest cost there.
- Fidelity on leftover scaffolding: on `dev2` your Milestone 1 shows 94 leftover cells against 0 in Java, and that is the
  class they care about most in big builds.

## 5. Constraints to keep in mind
- Their code moves: they work on experiment branches, and the accepted baseline is still `f6b21f4`. Any change they accept
  would need to reach the port for the sim to track it. Whether and how is your call; `docs/requests.md` is the channel
  for data that only they can produce (traces of early-ending builds exist on their side).
- The Java bot is only roughly deterministic across machines (about 15 % ticks on one build in seven), so exact
  agreement has a floor.
