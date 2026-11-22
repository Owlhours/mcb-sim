# mcsim: a Java-free simulator of the schematic-building bot

Predicts what the Java bot does with a schematic by running a statement-by-statement port of the bot against a
compact re-implementation of the parts of Minecraft 26.2 the bot touches. It predicts the verdict, the ticks,
and the missing, wrong and leftover cells. No JVM is involved anywhere. The Java source the port corresponds to
is in `port/java` (commit in `port/java/COMMIT`); `port/map.tsv` maps each Java member to its Rust function.

**State at the end of the project (2026-11-21).**
- **Default revision.** The default is `covertour`, the bot's final revision. `--rev` reaches every earlier one
  (glossary in [../docs/HISTORY.md](../docs/HISTORY.md)).
- **Checks against the Java bot.**
  - At `f6b21f4` the verdict agreed on all 290 reference builds, and 287 were reproduced exactly (ticks and every
    count).
  - At `2d9a193`: 290 / 290 verdicts, 288 exact.
  - At the final revision, the Java bot (every revision ported by a second, independent Java port) played exactly
    this simulator's course on 39 of 40 development builds.
- **Rebuilt from this repository,** it reproduced the Java harness tick for tick on 9 of 9 test builds
  ([../docs/METHOD.md](../docs/METHOD.md) §6).
- **Speed.** 2,000–7,500× real time per core.

Original brief: `GOAL.md`, the task as given to the session that wrote the port. It forbade running Java, so
everything was ported from source and checked against stored Java rows. Design, reasons and what was learned:
`docs/architecture.md`. Numbers by commit and the builds that differ: `docs/scoreboard.md`. What was asked of
the Java side: `docs/requests.md`. How the port follows the Java bot: `docs/tracking-the-java-bot.md`. These are
development records, written as the work went, and their paths are the development container's.

## Running

```bash
harness/run.sh tables                       # once: work/reference/mc/states.tsv + java-extra/ from the 26.2 server jar
harness/run.sh prep my-schematics/*.litematic   # work/corpus/cache/<2 hex>/<hash>.mcb + work/corpus/unique.tsv; prints hashes
cd sim-rs/sim && cargo build --release      # Rust 1.93+, no dependencies

target/release/mcsim run <hash>                         # one build, result row on stdout, run log on stderr
target/release/mcsim run path/to/build.mcb              # ... or a normalised file directly
target/release/mcsim run <hash> --rev f6b21f4           # ... as the Java bot of that revision would
target/release/mcsim run <list-file> out.tsv            # a list of hashes (optional second column: tick to stop at)
target/release/mcsim run <hash> --verbose               # PHASE / skip / clean-up log lines like the Java version's
target/release/mcsim run <hash> --events ev.txt         # placement log (P, S+, S-, T, X ... lines)
MCSIM_WATCH=x,y,z target/release/mcsim run <hash>       # every decision about one cell, every block change, every route
MCB_SEED=2 target/release/mcsim run <hash>              # the same build on another course (start shifted 2 blocks)
target/release/mcsim selfcheck                          # table self-check
target/release/place_check; target/release/basic_check; target/release/redstone_check   # rule scenario tests
```

Paths, all optional:
- `MCB_WORK` is the data directory (default `<repo>/work`, found from where the crate was built).
- `MCSIM_REF` (default `$MCB_WORK/reference`), `MCSIM_CORPUS` (default `$MCB_WORK/corpus`) and `MCSIM_DATA`
  (default `sim-rs/data`) override the parts.

The tools below are the development tools, kept as they were used. Run them from the repository root. Many need
Java reference rows for the development sets (`work/reference/results/`), which are not published, because they
are results on harvested builds.

```bash
python3 sim-rs/tools/compare.py out.tsv [set ...] [--list N] [--ref COMMIT]   # fidelity and speed against reference rows
python3 sim-rs/tools/revdiff.py old.tsv new.tsv [--list]   # what a revision changes, build by build
python3 sim-rs/tools/diffs.py out.tsv ...                  # the builds that are not reproduced exactly
sim-rs/tools/evdiff.sh <hash8>                             # first difference from a Java placement log
python3 sim-rs/tools/bodydiff.py java-ev.txt sim-ev.txt    # first tick at which two per-tick body traces differ
python3 sim-rs/tools/port.py status <java src dir>         # what a new Java tree changes against port/java
python3 sim-rs/tools/gen_tables.py                         # regenerate data/*.tsv and kinds.rs/props.rs (needs decompiled sources)
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
paths.rs      where the data that is not in git is found (MCB_WORK, MCSIM_REF, MCSIM_CORPUS, MCSIM_DATA)
(not in git)  work/reference = states.tsv and java-extra/ (harness/run.sh tables); work/corpus = .mcb builds
```
