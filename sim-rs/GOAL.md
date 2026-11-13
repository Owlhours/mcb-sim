# Goal: a fast simulator of the schematic-building bot, without Java

You own this project and its code, and nothing created here is planned for public release.

## Background

A bot that builds Minecraft schematics the way a survival player would (it walks, jumps, places and breaks blocks within
reach, carries a limited inventory, takes blocks from supply chests, uses temporary scaffolding) exists as a Java project.
It runs inside the real vanilla 26.2 server and is judged per 16x16x16 section (a section passes at 3 % bad cells or
less; a build passes when all sections do). Its source is in `reference/java-bot/src`; start with
`reference/java-bot/README.md`. The results it produced on several build sets are in `reference/results/`.

That version is slow to evaluate: about 300x real time on one core for the median build, about 110x when weighted by wall
time (see `reference/profile/java-profile.md`). In the two builds that were profiled, most of the time went to the bot's own
route search, not to the game.

## The task

Build a simulator that takes a schematic and predicts what the Java version would have done with it, at 1000x real time
or faster per core (game seconds simulated per wall second, 20 ticks per game second). The simulator does not need to
simulate a full world or draw anything. Its results should be close to the Java version's: the verdict (pass or fail), the
tick count, and the missing, wrong and leftover cell counts that the judge reports.

You choose the architecture. Whether the simulator ports the bot's decision logic and replaces the per-tick execution
with something cheaper, ports everything, or does something else is a decision for the research phase, made on
measurements.

## Rules

- **No Java.** Do not run a JVM, a JDK, GraalVM, a Java interpreter or the jar, and do not obtain one. The image has none, and
  the network allows only the model API, so nothing can be downloaded. Reading the jar, the decompiled sources, the data
  dumps and the Java project, and porting their logic and code, is expected.
- Two cores, 16 GB of RAM, a 100 GB disk at `/work`. Toolchains in the image: Rust (rustc, cargo), Go, clang and gcc, cmake,
  Python 3 with numpy. There are no external libraries beyond the standard ones, so write what you need.
- Use the same judging rules as the Java version (`mcb/build/Judge.java`; strict reading is the headline, the wider
  reading is reported next to it).
- The Java version's environment allowances apply (`mcb/env/Env.java`, `applyWorldRules`: fall protection, no block drops,
  no mobs, and so on).

## What is provided (all under `/work/reference`, see its `README.md`)

The Java source, readable source of the vanilla packages it uses, a per-block-state table (shapes, flags), the game's data
reports, results of the Java version on 290 builds at one commit (the comparison set), the build lists, and a read-only
corpus of 74,515 normalised schematics at `/corpus` (format described in the README).

## Measures

These are provisional; propose different ones, with reasons, if the research suggests better ones.

- **Speed:** median real-time factor over a reference set, per core, with the distribution (p10, p90) and the aggregate
  weighted by wall time. The floor is 1000x on the median build. If the research shows that much more is possible, aim higher
  and report the trade-off between speed and fidelity.
- **Fidelity:** per set in `reference/results/f6b21f4-*.tsv`: share of builds with the same verdict; relative error of the
  tick count (median and p90); error of the aggregate tick count; agreement of missing, wrong and leftover cell counts for
  failing builds. The Java version itself is not exactly deterministic (a repeat on another set-up differed by about 15 %
  in ticks on one build of seven), so agreement has a floor.

## Phases

1. **Research and test.** Decide the architecture with evidence before building it. Questions the decision depends on:
   where the Java version's time goes and what each part is for; which parts of the game must be modelled exactly for the
   bot's decisions to be the same (movement and collision, reach and ray casts, the placement rules, break times, falling
   blocks, scaffolding, doors, hazards) and which can be abstracted or skipped (chunks, lighting, entities, networking,
   rendering); the language and data layout, with micro-benchmarks; the route-search design (the profile points here);
   how to validate (differential tests of ported pieces against the decompiled source; whole-build comparison against the
   results files). Write the result as an architecture decision record in `/work/docs/architecture.md`: options considered,
   measurements, the chosen design, expected speed and fidelity, risks. A feasibility study with numbers is a complete
   deliverable for this phase.
2. **Build and measure.** Implement, compare against the reference sets after each step, and keep a scoreboard (speed and
   fidelity per set, by commit) in `/work/docs`. Keep the work in a git repository.

There is no deadline. The reference sets are for development; keep some builds out of the loop (for example one of the
`dev3` or `dev2big` sets) and compare on them only at milestones.

If you need more reference data from the Java version than `reference/results` holds (per-build event traces, for example),
write what you need in `/work/docs/requests.md`; it cannot be produced from inside this container.
