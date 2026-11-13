# mcsim: a Java-free simulator of the schematic-building bot

Predicts what the Java bot does with a schematic - verdict, ticks, missing / wrong / leftover cells - by running a
port of the bot against a compact re-implementation of the parts of Minecraft 26.2 it touches. No JVM is involved
anywhere. The Java source the port corresponds to is in `port/java` (commit in `port/java/COMMIT`).

State: two Java commits can be predicted.
* `f6b21f4` (`--rev f6b21f4`): the verdict agrees on all 290 reference builds and 287 are reproduced exactly
  (ticks and all counts); median speed 1,200x (big builds) to 4,300x real time per core.
* `2d9a193` (the default; a's master of 2026-11-13): the verdict agrees on all 290 reference builds and 288 are
  reproduced exactly; 192 placement logs are identical line for line and 94 more hold the same events; median
  speed 3,800x (big builds) to 7,800x. The port was made and reviewed before any Java result for this commit
  existed, and was not changed afterwards.

Goal: `GOAL.md`; design, reasons and what was learned: `docs/architecture.md`; numbers by commit and the builds
that differ: `docs/scoreboard.md`; what was asked of the Java side: `docs/requests.md`; how the port follows the
Java bot from commit to commit: `docs/tracking-the-java-bot.md`.

## Running

```bash
python3 tools/gen_tables.py                 # once: data/*.tsv and sim/src/{kinds,props}.rs from the reference material
cd sim && cargo build --release

target/release/mcsim run <hash>                         # one build (hash from /corpus/unique.tsv), result line on stdout
target/release/mcsim run <hash> --rev f6b21f4           # ... as the Java bot of that commit would (default: port/java/COMMIT)
target/release/mcsim run <list-file> out.tsv            # a list of hashes; out.tsv has the Java results format
target/release/mcsim run <hash> --verbose               # PHASE / skip / clean-up log lines like the Java version's
target/release/mcsim run <hash> --events ev.txt         # placement log in the format of reference/java-extra/events
MCSIM_WATCH=x,y,z target/release/mcsim run <hash>       # every decision about one cell, every block change, every route
target/release/mcsim selfcheck                          # table self-check

python3 tools/compare.py out.tsv [set ...] [--list N] [--ref COMMIT]   # fidelity and speed against reference/results/<commit>-*.tsv
python3 tools/revdiff.py old.tsv new.tsv [--list]       # what a revision changes, build by build, and which new mechanisms were at work
python3 tools/diffs.py out.tsv ...                      # the builds that are not reproduced exactly
tools/evdiff.sh <hash8>                                 # first difference from a Java placement log
[REV=..] tools/evall.sh [set ...]                       # placement logs of whole sets against the Java side's, by kind of difference
python3 tools/bodydiff.py java-ev.txt sim-ev.txt        # first tick at which two per-tick body traces (B lines) differ
python3 tools/navdiff.py <java verbose dir> [hash8 ..]   # the counters of Java verbose logs (searches, cells expanded, trap checks ...) against the simulator's
python3 tools/mklists.py                                # /work/tmp/<set>.txt from the reference result files
[REV=f6b21f4] tools/runsets.sh [small strat-small dev2] # run development sets at a revision and compare with its reference rows
python3 tools/port.py status <java src dir>             # what a new Java tree changes against port/java, with the Rust function for each member
```

The result line has the columns of the Java results files (from `2d9a193` on with the worst section ratio and
the `metrics` counters) plus, last, the CPU time of the build in ms; the run log on
stderr adds `rtf=` (game seconds per CPU second), route-search share, and `unmodelled=` / `lost=` (events the
simulator does not model - a build with either above 0 may differ from the Java run), `random=` (draws the Java
run takes from a clock-seeded random source: its own repeats may differ) and `DIED` (the bot died). `--stop-at TICK` (or a
second column in a list file) ends a build at the tick where the Java harness hit its wall-clock limit.

Diagnostics: `MCSIM_BODY=from,to` prints the body's position, velocity and view per tick (and writes `B` lines
into the `--events` log); `MCSIM_EVMORE=1` adds every trap check (`W`) and every block change the bot is told
of (`C`) to that log; `--events-dir DIR` writes one log per build of a list; `MCSIM_NAVCHECK=1` runs
the reference route search next to the fast one and stops on any difference;
`MCSIM_NAVSTAT=1` prints a histogram of search sizes by outcome; `MCSIM_PROF=file` samples the program counter
(`tools/prof.py`, `tools/proflines.py`); `target/release/ref_check` compares placement tables, `floats` and state
facts with the Java dumps in `reference/java-extra`; `place_check`, `basic_check`, `redstone_check` are rule
scenario tests.

## Layout

```
tools/        table generator, comparison and analysis scripts, port bookkeeping (port.py)
port/         java/ = the Java source the Rust port corresponds to; map.tsv = Java member -> Rust function
data/         generated block / tag / item tables
sim/src/      tables, shapes, world (block update engine), rules/ (place, basic, redstone), physics, player,
              sim (client + server tick), oracle, bot/ (the port of mcb.bot), build (target, site, judge, run)
docs/         architecture decision record, scoreboard, requests, porting notes
reference/    (not in git) the Java version, vanilla sources and data, results, Java-side dumps and traces
```
