# Keeping the simulator in step with a changed Java bot

The simulator predicts commit `f6b21f4`. If the Java bot changes, the change has to be made in the port as well;
nothing reads the Java source at run time. The port follows the Java classes one to one, so a Java diff maps to a
Rust diff of about the same size.

## Where each Java class lives

| Java (`reference/java-bot/src/mcb`) | Rust (`sim/src`) | notes |
|---|---|---|
| `bot/Grid.java` | `bot/grid.rs` | `put` is the only writer of `cur`. `stand16_compute` is the literal port; `nav_bits()` / `stand_info()` are tables derived from the same per-state facts for the fast route search - a change to `stand16`, `passable`, `rung`, `hang`, door edges or `isTower` must be made in both |
| `bot/Ray.java` | `bot/ray.rs` | |
| `bot/Body.java` | `bot/body.rs` | `find_click`, `find_break`, `aim`, `follow`, `settle` ... |
| `bot/Nav.java` | `bot/nav.rs` | **two copies**: `nav_search_ref` + `relax` (non-fast branch) is the literal port; `nav_search_impl` is the fast one over the padded mirror. Change both, then run with `MCSIM_NAVCHECK=1`, which runs both on every search and stops at the first difference |
| `bot/BuildState.java` | `bot/state.rs` | the `Bot` struct holds the fields of the whole class hierarchy |
| `bot/Moves.java` | `bot/moves.rs` | `way_out` sets `nav.touch_limit` (an exact early exit that relies on the caller reading only "found or > 96 expanded") |
| `bot/Placing.java`, `Stances.java`, `Scaffolds.java`, `Supplies.java` | same names | |
| `bot/Builder.java` | `bot/builder.rs` | `deferred_pass` picks the nearest unvisited cell from a sorted list while the bot stands still (same picks as the Java rescan); `surroundings` has a fast path for interior cells |
| `env/Client.java`, `env/Env.java` | `sim.rs` | tick order, packet handlers, server tick |
| `env/BotPlayer.java` (+ vanilla `LocalPlayer`, `Player`, `LivingEntity`, `Entity`) | `player.rs`, `physics.rs` | |
| `env/Oracle.java` | `oracle.rs` | placement rules themselves: `rules/place.rs` |
| `build/StateInfo.java` | `tables.rs` (columns from `reference/mc/states.tsv`) | a new `StateInfo` column needs a new dump of `states.tsv` or a derivation in `tools/gen_tables.py` |
| `build/Target.java`, `Site.java`, `Judge.java`, `run/BuildRun.java` | `build.rs` | |

## Rules of the port

* Statement by statement, same order of side effects. Ticks pass inside helper calls (`self.tick()`), so moving a
  call across a tick changes results.
* Java arithmetic exactly: `int + float` is float arithmetic, `(int)` truncates, `Math.round` is
  `floor(x + 0.5)`, `float` values stay `f32` until Java widens them, `Mth.sin/cos` are table lookups
  (`physics::msin/mcos`).
* Iteration order is behaviour: fastutil `IntOpenHashSet` / `Int2IntOpenHashMap` are reproduced in
  `fastutil.rs` (`IntSet`, `IntMap`) - use them wherever the Java code iterates one; `FastSet`/`FastMap`
  (`fasthash.rs`) only where it never iterates.
* Counters that the Java code reads back (`nav.expanded`, `placed`, `trips` ...) are part of the behaviour;
  pure statistics are not.

## Checking a change without Java

1. `tools/runsets.sh` - the three development sets against the reference rows of the *old* commit: every build the
   change should not affect must stay exact (today 217 of 220).
2. For builds the change is meant to affect, ask the Java side for the new result rows and, for the first ones
   that differ, a placement log (`docs/requests.md` item 1 has the format; the simulator writes the same with
   `--events`). `tools/evdiff.sh <hash8>` shows the first differing event; `MCSIM_WATCH=x,y,z` and
   `MCSIM_BODY=from,to` show what the simulated bot saw and did around that tick.
3. `MCSIM_NAVCHECK=1` on a handful of builds after any change to `nav.rs` or `grid.rs`.
4. `target/release/ref_check` after any change to the tables or placement rules.

One tick of difference is a real difference: in `e52ac89a` a pressure plate one tick late changed the verdict of
a 59,636-block build. Do not accept "close".
