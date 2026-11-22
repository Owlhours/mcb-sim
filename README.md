# MCB Sim

The measurement side of [MCB Builder](https://github.com/Owlhours/mcb-builder), a Minecraft 26.2 bot that builds
schematics the way a survival player would. This repository holds the simulators and the harness that produced
every number quoted for that bot, including 963 / 1,000 on its second goal sample.

| | what it is | speed (game time per CPU time, one core) | how close to the game |
|---|---|---|---|
| [**harness/**](harness/README.md) | The bot itself, running in Mojang's own 26.2 server code: in-process, headless, no network, no window. | about 100–300x (wall time; a few seconds of start-up per process) | It *is* the game. This is the ground truth. |
| [**sim-rs/**](sim-rs/README.md) (`mcsim`) | A statement-by-statement port of the bot to Rust, on a re-implementation of the parts of the game the bot touches. No JVM anywhere. | 2,000–7,500x | At one checked revision: the Java verdict on 290 / 290 builds, the exact row (ticks and every count) on 288. At the final revision: the Java bot played exactly the simulator's course on 39 of 40 builds. |
| [**fastsim/**](fastsim/README.md) | An event-driven approximation: a cost table in game ticks instead of physics, a bit per cell instead of block states, a short bounded route search. | about 1,000,000x on a median development build | Game time summed per build set 0.81–0.90 of `mcsim`'s; verdicts agree on 139 of 140 development builds but only on 37–39 % of a hard set. Good for ranking order and routing ideas on ordinary builds, not for predicting failures. |

The three were built in that order, each one checked against the one before it:

```
harness (the game)  --  validates -->  sim-rs  --  validates -->  fastsim
   Java bot in the real 26.2 server       Rust port, the inner loop     coarse model, for trying algorithms
   minutes per large build                seconds                       milliseconds
```

During development the bot was changed and judged in `mcsim` first. A second, independent Java port then
carried each change into the Java bot and confirmed that it played the same course. The Java bot is the shipped
one. Its code lives in MCB Builder (`core/` and `adapter/`); the harness here compiles it from there, at the commit
pinned in `harness/mcb-builder.ref`, so there is one copy of the bot.

## A check you can run in five minutes

With Docker, Rust 1.93 or newer, and a schematic of your own (`.litematic`, `.schem`, `.nbt` or `.schematic`):

```bash
docker build -f harness/Dockerfile -t mcb-harness .        # JDK 25 + the 26.2 server jar from Mojang (SHA-1 pinned)
docker run --rm -v "$PWD/my-schematics:/in:ro" mcb-harness build /in/house.litematic quiet
#   ... VERDICT PASS done ticks=68361 gameHours=0.95 blocks=2883 missing=0 wrong=0 leftover=0 ...

mkdir -p work                                               # (before mounting it, or Docker creates it root-owned)
docker run --rm -u "$(id -u):$(id -g)" -v "$PWD/work:/repo/work" mcb-harness tables              # state tables for the sims
docker run --rm -u "$(id -u):$(id -g)" -v "$PWD/my-schematics:/in:ro" -v "$PWD/work:/repo/work" \
       mcb-harness prep /in/house.litematic                 # prints the build's hash; writes work/corpus/cache/<2 hex>/<hash>.mcb
(cd sim-rs/sim && cargo build --release)
sim-rs/sim/target/release/mcsim run <hash>                  # 395c354e 22x20x29 PASS | done | blocks=2883 ticks=68361 ...
```

When this repository was put together, nine schematics from 337 to 33,714 blocks went through both. The Java
harness and `mcsim` gave the same tick count, the same verdict and the same missing and wrong counts on all nine
(for example 68,361 ticks for a 2,883-block timber-frame house and 737,919 for a 33,714-block floating island).
`fastsim` passed all nine as well, at 0.88–1.45 of the exact tick count. Details are in
[docs/METHOD.md](docs/METHOD.md) §6.

Without Docker, the harness needs only a JDK 25 and `curl`: `harness/run.sh build <file>`.

## What you're walking into

Read [docs/BUILDING-ON-IT.md](docs/BUILDING-ON-IT.md) before you reuse any of this. In short:
- **It was written by Claude agents** in an automated loop, against a fixed benchmark, over ten days in
  November 2026. A person set the goals and made the calls. The commit history is the real development history,
  and the terminology in code comments and commit messages is the loop's own: `A's simulator`, `the Java side`,
  `requests`, named revisions. [docs/HISTORY.md](docs/HISTORY.md) has a glossary.
- **Everything is tied to Minecraft 26.2.** The state tables are dumped from the 26.2 server, and the Rust
  rules are ports of 26.2 block classes. Another version means re-dumping the tables and re-checking the rules.
- **The simulators model only what the bot touches.** That means a flat world, survival on Peaceful, no mobs, fall
  damage off, random ticks off, block drops off, and time and weather frozen. Fluids barely exist. Redstone is
  modelled far enough for the bot's own builds, not as a general engine.
- **`mcsim` tells you when not to trust it.** Its run log counts `unmodelled` and `lost` events. A build with either
  above 0 may differ from the Java run.
- **The corpus is not included.** The 74,515 builds the numbers were measured on are other people's work, harvested
  from public sharing sites under unknown licenses. The evaluation scripts expect such a corpus (a list of hashes
  plus `work/corpus/`); you bring your own.
- **No Mojang jars, assets or decompiled source are in this repository, with two things to know about.** The
  scripts download the 26.2 server jar from Mojang, check it against a pinned SHA-1, and use it only on your
  machine. The two state tables the simulators read are generated from that jar there (`harness/run.sh tables`);
  the generated `states.tsv` was checked byte for byte against the one used in development. But:
  - `sim-rs/data/{blocks,items,tags}.tsv` are tables of block, item and tag names and properties taken from the
    game's data reports;
  - the Rust rule and physics code (`sim-rs/sim/src/rules/`, `physics.rs`) follows vanilla methods statement by
    statement, re-expressed in Rust. See [docs/BUILDING-ON-IT.md](docs/BUILDING-ON-IT.md).

## Layout

| path | |
|---|---|
| `harness/` | the headless runners (`src/mcb/run`), `build.sh` (compiles the bot from an MCB Builder checkout, see `mcb-builder.sh`), `run.sh`, `Dockerfile` |
| `sim-rs/` | the Rust simulator `mcsim` (`sim/`), its block tables (`data/`), the Java source it is a port of (`port/java/`) with the member-to-function map, design records (`docs/`) and analysis tools (`tools/`) |
| `fastsim/` | the fast simulator (`src/`) and the copy of `mcsim` it links for its exact levels (`mcsim-ref/`) |
| `scripts/` | the evaluation tooling: sharded runs, seed sweeps, paired comparisons and the statistics behind every decision ([scripts/README.md](scripts/README.md)) |
| `docs/` | [METHOD.md](docs/METHOD.md) (how the simulators work and how they were validated, the evaluation method, the judge and world rules), [HISTORY.md](docs/HISTORY.md) (timeline, git history, glossary), [BUILDING-ON-IT.md](docs/BUILDING-ON-IT.md); and [prompts.md](prompts.md) at the top level: what the owner asked for, in their own words (shared with mcb-builder) |
| `work/` | not in git: downloaded and generated data, your corpus, scratch worlds (`MCB_WORK`) |

## Status

The project ended on 2026-11-21. Nothing here is maintained. The final bot revision is `covertour`, which is
`mcsim`'s default `--rev`. `fastsim` was closed the same day, with its open questions recorded in
[fastsim/README.md](fastsim/README.md).

## License

MIT, for this project's original work; see [LICENSE](LICENSE). The bot itself, packaged as Fabric mods, is in
[mcb-builder](https://github.com/Owlhours/mcb-builder).

NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT. Minecraft is a trademark
of Mojang Synergies AB; its code and assets belong to Mojang and Microsoft under the
[Minecraft EULA](https://www.minecraft.net/en-us/eula).
