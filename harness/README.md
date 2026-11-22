# harness: the bot in the real game, headless

The Java bot (MCB Builder's `core/` planner and `adapter/` Minecraft adapter) running in
Mojang's own 26.2 server code. The server is booted in-process, as the game-test flavour: a flat world with no
network and no window, ticked on the calling thread, with the idle world skipped between player ticks. This is
the ground truth that `mcsim` was checked against.

## Running

Needs a JDK 25, `bash`, `curl` and `sha1sum` (`unzip` if present, otherwise the JDK's `jar`), or only Docker.

```bash
harness/run.sh build my-schematics/house.litematic quiet      # one build, start to verdict
#   BUILD 395c354eddde9cbecfde house.litematic 22x20x29 blocks=3381 format=sponge
#   RESULT PASS | done | blocks=2883 ticks=68361 (23.7 t/block) placed=3755 ... | PASS sections=8 failed=0 ...
#   VERDICT PASS done ticks=68361 gameHours=0.95 blocks=2883 missing=0 wrong=0 leftover=0 sections=8 failedSections=0 wall=11.4s
harness/run.sh build house.litematic                          # the same with the bot's full log (phases, skips, clean-up)
harness/run.sh build house.litematic quiet 600                # with a wall-clock limit of 600 s
harness/run.sh prep a.litematic b.schem ...                   # normalise into work/corpus (what the Rust sims read);
                                                              # the hashes also go to work/corpus/prep-list.txt
harness/run.sh tables                                         # work/reference/mc/states.tsv and java-extra/* for the sims
harness/run.sh one 395c354e                                   # a build from work/corpus by hash prefix
harness/run.sh corpus list.txt rows.tsv                       # a list of hashes, one result row each
harness/run.sh java mcb.test.EnvSmoke                         # any main class
```

The first call runs `build.sh`. It downloads the vanilla 26.2 server jar from Mojang into `harness/.deps/`
(about 60 MB, checked against SHA-1 `823e2250d24b3ddac457a60c92a6a941943fcd6a`), unpacks its inner server jar
and libraries, and compiles `src/` against them. Nothing from Mojang is committed.

Environment:
- `MCB_WORK` is the data directory (default `<repo>/work`). The game's log goes to `$MCB_WORK/logs` and scratch
  worlds to `$MCB_WORK/tmp`.
- `JAVA_OPTS` sets the JVM options (default `-Xmx4g -XX:+UseSerialGC`).
- `MCB_SEED=k` starts the bot k blocks further along the build's front, which gives the same build on a
  different course.
- `MCB_NO_<REVISION>` switches off one revision (glossary in [../docs/HISTORY.md](../docs/HISTORY.md)).
- `MCB_SNAP=1` runs from a copy of the classes, so a rebuild doesn't disturb a long batch.

**Output.** The `RESULT` line has the judge's counts: wrong, missing, scaffolding left, exempt cells, the worst
section's ratio, and two alternative readings (`byVolume`, `wide`). `BAD` lists bad cells by block. `NAV` holds
the bot's counters: ticks by activity, searches, stances, routes. `RunFile` and `Prep` print their own lines plainly on
stdout. The bot's log and the server's messages go through the game's logger, so those lines start with
`[time] [main/INFO]:`.

## Docker

```bash
docker build -f harness/Dockerfile -t mcb-harness .           # from the repository root
docker run --rm -v "$PWD/my-schematics:/in:ro" mcb-harness build /in/house.litematic quiet
mkdir -p work                                                 # before mounting it: else Docker makes it root-owned
docker run --rm -u "$(id -u):$(id -g)" -v "$PWD/work:/repo/work" mcb-harness tables
docker run --rm -u "$(id -u):$(id -g)" -v "$PWD/my-schematics:/in:ro" -v "$PWD/work:/repo/work" mcb-harness prep /in/house.litematic
```

The image is `eclipse-temurin:25-jdk` with nothing installed on top. Docker downloads the server jar from Mojang
while the image builds, and `build.sh` checks its SHA-1. On Windows, Docker Desktop runs the same commands; `run-docker.bat <schematic>` wraps the first one for
a file in the current folder. The `.bat` was not tested.

## What is here

| | |
|---|---|
| (MCB Builder) `core/src/mcb/bot` | the planner: `Builder` → `Supplies` → `Scaffolds` → `Stances` → `Placing` → `Moves` → `BuildState`, plus `Nav` (route search), `Body`, `Grid`, `Ray` |
| (MCB Builder) `core/src/mcb/build`, `schem` | `Target` (normalisation), `Site`, `Judge`, `StateInfo` (per-state tables); readers for `.litematic`, `.schem` (Sponge v1–3), `.nbt` and legacy `.schematic`; `Schem.save` / `load` for the normalised `.mcb` format |
| (MCB Builder) `adapter/server/src/mcb/env` | `Env` (the server in-process, world rules, idle-world fast ticking, site pinning), `BotPlayer` (a server-side player moved by vanilla physics from client-faithful key states), `BotClient` (the bot's only interface: view, keys, clicks, slots, block-change packets), `ServerOracle` (the game's placement code, run without placing), `Depot` (the supply chests) |
| (MCB Builder) `adapter/src/mcb/mc` | `BlockFacts` (fills `StateInfo` from the game's own block definitions), `GameStates` (schematic states through the game's data fixers), `GameSurvival` |
| (MCB Builder) `adapter/headless/src/mcb/headless` | `BuildRun` (one build: site, join, run, judge, wipe), `Gate` (MCB Builder's regression gate), `ClientRun` (the client mod's settings, headless) |
| `src/mcb/run` | `RunFile`, `RunOne`, `RunCorpus`, `Prep`, `DumpStates`, `DumpRef`, `Work` (the data directory) |
| `src/mcb/corpus` | `BuildIndex`: indexes a list of schematic files into `corpus/cache` and `index.tsv` (how the 74,515-build corpus was made) |
| `src/mcb/test` | smoke tests and small scenario mains from the first days (`EnvSmoke`, `FirstBuild`, `TowerTest` ...) |

**The `.mcb` format.** A zlib stream of big-endian values: int magic `0x4d434231`, int sx, sy, sz, long unknown
cells, int data version, a UTF format name, then pairs of (varint run length, 16-bit global state id) until
sx·sy·sz cells are filled. The cell index is `(y*sz + z)*sx + x`, and id 0 is air. The hash is of the normalised
content, so two files holding the same build get one hash. State ids are those of 26.2.

**The tables for the Rust simulators.** `DumpStates` writes `states.tsv`: one row per block state, with the
Java bot's `StateInfo` columns, friction and speed factors, destroy time, light and the collision and outline
boxes. Its `hazard` column reproduces the bot's definition at the time the file was first made (commit
`f6b21f4`, which also counted magma, berry bushes and powder snow). With that, the output is byte-identical to the
file the simulators were developed and validated with (SHA-256
`479e318bf211cf9ec52179a1de7b330c0eed972dc6a247aed52aed87e680117b`). `DumpRef` writes `states-extra.tsv`
(byte-identical too), `floats.bin.gz`, and the placement tables. The tables differ from development's only for
vines and glow berries, which get a random age.
