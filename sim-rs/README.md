# mcsim: a Java-free simulator of the schematic-building bot

Predicts what the Java bot (`reference/java-bot`, commit `f6b21f4`) does with a schematic - verdict, ticks, missing /
wrong / leftover cells - by running a port of the bot against a compact re-implementation of the parts of
Minecraft 26.2 it touches. No JVM is involved anywhere. Goal: `GOAL.md`; design and reasons:
`docs/architecture.md`; numbers by commit: `docs/scoreboard.md`; what was asked of the Java side:
`docs/requests.md`.

## Running

```bash
python3 tools/gen_tables.py                 # once: data/*.tsv and sim/src/{kinds,props}.rs from the reference material
cd sim && cargo build --release

target/release/mcsim run <hash>                         # one build (hash from /corpus/unique.tsv), result line on stdout
target/release/mcsim run <list-file> out.tsv            # a list of hashes; out.tsv has the Java results format
target/release/mcsim run <hash> --verbose               # PHASE / skip / clean-up log lines like the Java version's
target/release/mcsim run <hash> --events ev.txt         # placement log in the format of reference/java-extra/events
MCSIM_WATCH=x,y,z target/release/mcsim run <hash>       # every decision about one cell, every block change, every route
target/release/mcsim selfcheck                          # table self-check

python3 tools/compare.py out.tsv [set ...] [--list N]   # fidelity and speed against reference/results/f6b21f4-*.tsv
python3 tools/diffs.py out.tsv ...                      # the builds that are not reproduced exactly
tools/evdiff.sh <hash8>                                 # first difference from a Java placement log
tools/runsets.sh [small strat-small dev2]               # run development sets and compare
```

Diagnostics: `MCSIM_NAVCHECK=1` runs the reference route search next to the fast one and stops on any difference;
`MCSIM_NAVSTAT=1` prints a histogram of search sizes by outcome; `MCSIM_PROF=file` samples the program counter
(`tools/prof.py`, `tools/proflines.py`); `target/release/ref_check` compares placement tables, `floats` and state
facts with the Java dumps in `reference/java-extra`; `place_check`, `basic_check`, `redstone_check` are rule
scenario tests.

## Layout

```
tools/        table generator, comparison and analysis scripts
data/         generated block / tag / item tables
sim/src/      tables, shapes, world (block update engine), rules/ (place, basic, redstone), physics, player,
              sim (client + server tick), oracle, bot/ (the port of mcb.bot), build (target, site, judge, run)
docs/         architecture decision record, scoreboard, requests, porting notes
reference/    (not in git) the Java version, vanilla sources and data, results, Java-side dumps and traces
```
