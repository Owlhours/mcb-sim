# Before you build on this

Possible uses include companion bots, autonomous bots, scripted multiplayer, roleplay NPCs, and research on
build planners or on simulator design. This page says what you're getting and where it will bite.

## How it was written

- **By Claude agents, in an automated loop, against a fixed benchmark.** Each simulator was its own long-running
  session with its own goal, writing its own notes and committing its own code. A person set the goals,
  answered questions and made the decisions marked "the owner's" in comments. Nobody reviewed the code line by
  line.
- **Optimised for one benchmark.** The target was the pass rate on the goal samples under the judge in METHOD.md
  §1. Anything that didn't move that number got no attention: code structure, API design, error handling for
  inputs the benchmark never produced.
- **Terminology is internal.** The code says "A's simulator" (`mcsim`), "the Java side", "requests", "the lead",
  "the owner's rule", and names revisions after what they change (`ropefar`, `nostand`). HISTORY.md has a
  glossary. Comments are often notes to the next session ("ported from ...", "D confirmed ...") rather than
  documentation.
- **The comments are dense but accurate.** The habit in all three sessions was to record *why* next to the code,
  with the build hash that showed it. Expect long comments and very long lines.

## What it is tied to

- **Minecraft 26.2, exactly.** The harness compiles against the 26.2 server classes, which ship unobfuscated, so
  there is no remapping layer to update. `mcsim`'s block rules are ports of 26.2 classes, and its tables are
  dumped from 26.2. A different version means:
  1. re-dumping `states.tsv` and `states-extra.tsv` (`harness/run.sh tables`);
  2. regenerating `sim-rs/data/*.tsv` and `kinds.rs` / `props.rs` (`sim-rs/tools/gen_tables.py`, which also
     reads decompiled `Blocks.java` and the game's data reports: you decompile and extract those yourself);
  3. re-checking every rule port (`place_check`, `basic_check`, `redstone_check`, `ref_check`) and the physics
     against Java runs.
- **The bot's own world.** Flat ground, a fixed site layout, supply chests the harness fills, Peaceful, no mobs,
  fall damage off, random ticks off, drops off, no tools. `mcsim` models only that world. It has no general
  entity system, no fluids to speak of, no lighting, no chunks, and only the redstone the builds needed. It is
  not a general Minecraft simulator, and growing it into one would mean starting most of it again.
- **One player.** Both simulators and the harness run one bot per world. Several bots were an idea, never built.

## Where the simulators can't be trusted

- **`mcsim` counts its blind spots.** The log line of every build ends with `unmodelled=`, `lost=` and `random=`:
  - `unmodelled` counts events it doesn't model, such as explosions, entities pushed by pistons and some damage;
  - `lost` counts items the real game would have dropped;
  - `random` counts draws the Java bot takes from a clock-seeded random source.

  A build with `unmodelled` or `lost` above 0 may differ from the Java run. One with `random` above 0 may differ
  between two Java runs.
- **Parity is per revision, not forever.** The numbers were measured at given revisions (METHOD.md §5). A change
  to the Java bot must be ported to `mcsim`, or the other way round. `sim-rs/port/` and
  `sim-rs/docs/tracking-the-java-bot.md` describe how that was done. An unported change means the two simply disagree,
  silently.
- **Identity-hash order and JVM timing** put a floor under agreement. Early on, two Java set-ups disagreed on 1
  build in 7, by about 15 % in ticks. The harness is deterministic for given classes, but it's not guaranteed to
  match a different JVM or machine tick for tick.
- **fastsim is a different, coarser bot.** It agrees with `mcsim` on summed time and on ranking order and routing
  changes on ordinary builds. It doesn't predict which builds fail, and it doesn't rank changes on large builds.
  Every simplification is listed with its likely bias in [fastsim/README.md](../fastsim/README.md). Treat a fastsim
  win as a hypothesis for `mcsim`, never as a result.
- **The placement tables have random parts.** Vines and glow berries get a random `age` when placed, so their
  tables differ from dump to dump (`ref_check` shows 3 items differing). The judge ignores `age`.

## Technical debts and assumptions

- **Absolute paths are gone at HEAD, but nowhere else.** Development ran in containers, with the repository at a
  fixed absolute path. The final commit replaced those paths with `MCB_WORK` (default `<repo>/work`) and repo-relative
  paths. Scripts expect to be started from the repository root. Old commits won't run outside those containers.
- **`sim-rs/port/java` is a frozen copy** of the Java source the port was last brought to. It still contains
  the container's absolute paths, on purpose: `tools/port.py` diffs a Java tree against it.
- **Many tools need what isn't published.** `sim-rs/tools/compare.py`, `mklists.py`, `evall.sh`, `scripts/simrun.sh`
  with set names, `draw_goal2.py` and others read the corpus index, sample lists, and Java reference rows for
  harvested builds (`work/reference/results/`). They are included because they *are* the method, but you have to
  bring a corpus. `harness/run.sh prep` builds a corpus from your own files in the same layout
  (`work/corpus/cache/<2 hex>/<hash>.mcb` plus `unique.tsv`).
- **The Rust crates have no dependencies,** so `cargo build` works offline. fastsim compiles with
  `-C target-cpu=native` (development used `znver3`). Its binary is not portable between CPUs.
- **The Rust rule code is a port of decompiled game code** in its structure and constants. The harness compiles
  against the game's classes but contains none of them. `sim-rs/sim/src/rules/` and `physics.rs` follow vanilla
  methods statement by statement, re-expressed in Rust. Judge for yourself whether that matters for your use.
- **`basic_check` reports 65 passed, 1 failed** (the sign test: a click with a block on a sign falls through to
  the empty hand). That was its state at the end of development. The builds don't depend on it.

## Seams for your own project

- **A different planner on the same body:** MCB Builder's `core/src/mcb/bot` is the planner. `mcb.game.Client` is the whole
  interface to the game (view, keys, clicks, slots, what the client was told). Replace `Builder` and keep the
  rest, and the harness and judge still work.
- **Your own scoring:** `mcb.build.Judge` (Java) and `build.rs` (Rust) are self-contained. `StateInfo.canon` / `loose` (filled from the
  game by MCB Builder's `mcb.mc.BlockFacts`, whose `IGNORED` list is the core of it) define what "right" means.
- **A faster inner loop for your own bot:** `mcsim` shows that a statement-by-statement port with exact per-tick
  physics can run at thousands of times real time. Its architecture record (`sim-rs/docs/architecture.md`)
  explains the decisions and their measurements. fastsim shows the next step down, along with what it costs
  in fidelity.
- **Evaluation:** `scripts/evalmath.py`, `seedgate.py`, `compare.py` and METHOD.md §7 apply to any planner whose
  runs are deterministic but chaotic.
- **Live use** (a real player, a real server) is MCB Builder's client mod, not this repository.
