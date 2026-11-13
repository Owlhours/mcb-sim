# Keeping the simulator in step with the Java bot

The simulator is a port: nothing reads the Java source at run time, so every Java change has to be made in Rust as
well. The Java bot keeps moving, so the port is set up to be brought forward commit by commit. This page is the
procedure; the facts it relies on are in files that tools read, not in prose:

| what | where |
|---|---|
| the Java source the Rust port corresponds to (a snapshot, in git) | `port/java/`, commit in `port/java/COMMIT` |
| Java class -> Rust file, the exceptions to the naming rule, what is deliberately not ported and why | `port/map.tsv` |
| what a new Java tree changes, member by member, with the Rust function each change lands in | `tools/port.py` |
| which Java commits the binary can predict, and the Java version's own switches | `sim/src/rev.rs` |
| Java idiom -> Rust idiom, and the brief for reviewers of a port | `port/CONVENTIONS.md` |

History: `f6b21f4` (the first port, 287 of 290 reference builds exact) -> `2d9a193` (2026-11-13, 124 changed
members; ported and reviewed before any Java result existed for it; the first 220 rows then showed 218 exact,
the two others being builds that were inexact before the port).

## Bringing the port to a new Java commit

```bash
python3 tools/port.py status NEWSRC          # NEWSRC = the directory that contains mcb/ of the new commit
python3 tools/port.py diff NEWSRC Moves.     # the Java diff of every changed member of a class (or one member)
```

`status` lists every member (method, field, initialiser) whose code differs from the snapshot - comments and
whitespace do not count - grouped by the Rust file it belongs to, with `file:line fn name` of its counterpart:

```
== sim/src/bot/moves.rs
  CHANGED Moves.wayOut(int,int,int,boolean)   [mcb/bot/Moves.java:86]  ->  sim/src/bot/moves.rs:71 fn way_out
  NEW     Moves.plainWayOut(int,int,int)      [mcb/bot/Moves.java:164] ->  sim/src/bot/moves.rs (to add)
```

Then, per member:

1. Port it (rules below). A new method gets the Java name in snake case in the class's file; if it cannot (it is
   inlined, split, or not ported), add a line to `port/map.tsv` saying where it went or why not.
2. Guard it so that the previous behaviour stays reachable (next section).
3. `python3 tools/port.py done NEWSRC Class.member ...` (or `Class.` for a whole class). A member that changes again
   in a later tree comes back by itself.

When `status` reports `0 members to port`:

```bash
python3 tools/port.py finish NEWSRC <commit>   # NEWSRC becomes the snapshot
python3 tools/port.py check                    # every Java method has a Rust function or a stated reason
git add -A port sim && git commit              # one commit holds the Java delta and the Rust delta
```

so `git show <commit> -- port/java` is the Java change a Rust commit ported, and a reviewer can read the two
side by side.

## Revisions: the old behaviour stays testable

`mcsim run ... --rev <commit>` (or `MCSIM_REV`; default: the newest) selects the Java commit to predict;
`REV=<commit> tools/runsets.sh` runs the development sets at that revision against
`reference/results/<commit>-<set>.tsv`. In the code:

* a change that the Java side made behind one of its switches (`X_EXIT`, `MCB_NO_HAZ` ...) is behind the flag of
  the same name in `rev::rev()` (`rv.x_exit`, `rv.x_haz` ...). The flags read the same environment variables as the
  Java constants, so a Java run with `MCB_NO_NEXT=1` is predicted by the simulator run with `MCB_NO_NEXT=1`;
* a change without a Java switch is behind the revision flag (`rv.a1` = "at or after 2d9a193");
* no guard where the change cannot matter in the old revision (a new field that only new code reads).

The older revision is the regression test of the port itself: after porting, `REV=f6b21f4 tools/runsets.sh` must
give the same exact counts as before (40 / 119 / 59 on small / strat-small / dev2). When a revision has no
reference rows worth keeping any more, its guards can go: grep for the flag, keep the newer branch.

## Rules of the port

* Statement by statement, same order of side effects. Ticks pass inside helper calls (`self.tick()`, `settle`,
  `travel` ...), so moving a call across one changes results.
* Java arithmetic exactly: `int + float` is float arithmetic, `(int)` truncates, `Math.round` is
  `floor(x + 0.5)`, `float` values stay `f32` until Java widens them, `Mth.sin/cos` are table lookups
  (`physics::msin/mcos`), `String.format("%.3f")` rounds ties up (`build::fmt3`).
* Iteration order is behaviour: fastutil `IntOpenHashSet` / `Int2IntOpenHashMap` are reproduced in
  `fastutil.rs` (`IntSet`, `IntMap`) - use them wherever the Java code iterates one; `FastSet`/`FastMap`
  (`fasthash.rs`) only where it never iterates.
* Every write to the bot's picture of the world goes through `Grid::put` / `put_at` (Java writes `g.cur[i]`
  directly, also for "what if this block were here" questions): the per-cell mirror the route search reads
  depends on it.
* Counters the Java code reads back (`nav.expanded`, `placed`, `trips`, `keptForExit` ...) and strings it compares
  (`deferWhy`) are behaviour. Counters that only reach the log are not ported; the ones in the result file's
  `metrics` column are (`Bot::metrics`) - they are the cheapest check there is of which mechanism went differently.
* Java `try { ... } finally { restore }`: an `OutOfTime` ends the run, so only early `return`s need the restore.
* Not ported, by decision (each has its line in `port/map.tsv`): `PARANOID`, `TRACE`, `WATCH` and verbose log
  text, activity names (`enter`), `ROPE_PERCHED` (constant false).

## One route search, one standing test

There used to be two copies of `Nav.search` (the literal port and a fast one). There is one now,
`nav_search_g::<F>` in `bot/nav.rs`, written over per-cell facts (`N_PASS`, `N_AIR`, `N_SCAF`, `N_COL16` ...;
`grid.rs`, `nav_bits`):

* `F = true` (what runs) reads the facts from the grid's padded mirror, `stand16` from its per-cell cache, and
  takes the hoisted shortcuts for `scaffoldOk` / `towerOk`;
* `F = false` computes every fact on the spot from `Grid::get`, the state tables and the scaffold set, and calls
  the rules every time.

A Java change to the search is made once. `MCSIM_NAVCHECK=1` runs both forms on every search and stops at the
first difference in anything a caller can see: that checks the mirror, the cache and the shortcuts, which is where
the port can go wrong on its own (a stale cache after a "what if" write, a flag not kept up).

If the Java search starts to use a state fact that is not a bit yet: for a rare branch read the table through
`self.g.get(x, y, z)` (as the fence pillar does with `colTop`); for a hot one add a bit to `nav_bits`.

`Grid.stand16` is `stand16_compute` (one implementation); the fast path is a cache of its result. It reads its
per-state facts from `stand_info()`; a new fact (as `hazard` was) is one more bit there.

## Checking a port without Java

1. `REV=<previous> tools/runsets.sh` - unchanged exact counts (the guards hold).
2. `MCSIM_NAVCHECK=1` on a set at the new revision after any change to `nav.rs` or `grid.rs`.
3. A review by readers who did not write the port: one per group of files, given the Java diff and the Rust
   commit, asked for discrepancies only. (Done for 2d9a193 with three reviewers: no behavioural discrepancy, one
   in the formatting of an output column.)
4. `target/release/ref_check` after any change to the tables or placement rules.

## Checking it with Java

What to ask the Java side for is in `docs/requests.md` ("Port kit"). With the rows of the new commit:

1. `REV=<commit> tools/runsets.sh` - verdict, ticks, counts, and the `metrics=` column.
   `REV=<commit> tools/evall.sh` - the placement logs of whole sets against the Java side's (`MCB_EVENTS`), sorted
   into identical / same events in another order within a tick / random ages only / different.
2. For a build that differs, `tools/evdiff.sh <hash8>` against its placement log shows the first differing event;
   `MCSIM_WATCH=x,y,z` and `MCSIM_BODY=from,to` show what the simulated bot saw and did around that tick. The
   metrics tell where to look first (a different `exitJoined` or `trapsAvoided`: the trap check; `retreats`: the
   health guard; `fromInside` / `fromClose` / `pits`: the last-resort placements ...).

One tick of difference is a real difference: in `e52ac89a` a pressure plate one tick late changed the verdict of
a 59,636-block build. Do not accept "close".
