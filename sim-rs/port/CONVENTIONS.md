# Java idiom -> Rust idiom in this port

For whoever ports a Java change, and for whoever reviews the port. (Where each class lives: `map.tsv`; the
procedure: `docs/tracking-the-java-bot.md`.)

| Java | Rust |
|---|---|
| the class hierarchy `Body` .. `Builder` | one struct `Bot` (`sim/src/bot/state.rs`); each class's methods are an `impl Bot` block in its own file |
| `g.cur[i] = v` (also for "what if" questions) | `self.g.put(i, v)` / `put_at(x, y, z, v)` - never write `cur` directly (the search's cell mirror and the `stand16` cache hang on it) |
| `g.in(x, y, z)` | `self.g.inside(x, y, z)` |
| `g.idx(x, y, z)` on a cell that may be outside (used as a set key) | `self.g.idx_raw(x, y, z)` |
| `nav.search(..., null)` / `nav.search(..., this::scaffoldOk)` | `self.nav_search(..., false)` / `(..., true)` |
| `nav.mine = this::mineTicks` / `null` | `self.nav.mine = true` / `false` |
| `nav.stopAt = exitNodes` / `null` | `self.nav.stop_at = true` / `false` (the search consults `self.exit_nodes`) |
| `int[] path = Arrays.copyOf(nav.path, ...)` ... `System.arraycopy` back | `let kept = self.keep_route(); ... self.restore_route(kept);` |
| `IntOpenHashSet` / `Int2IntOpenHashMap` that is iterated | `IntSet` / `IntMap` (`fastutil.rs`: same iteration order); `.keys()` is a snapshot in that order |
| the same, only add / contains / remove / clear | `FastSet<i32>` / `FastMap<i32, i32>` |
| `StateInfo.x[state]` | `t().x[state as usize]` (`tables.rs`); `hazard(s)`, `hot(s)` are functions |
| `Ray.outline(s)` / `Ray.hasOutline(s)` | `t().out_boxes_f32[s]` (f32, six per box) / `!...is_empty()` |
| `ray.cast(g, ...)`, results `ray.hx ..` | `self.ray.cast(&self.g, ...)`, `self.ray.hx ..` |
| `oracle.pick(c, yaw, pitch)` | `self.c.pick(yaw, pitch)` -> `Option<Hit { pos, face, loc, inside }>` |
| `oracle.predict(item, hit, yaw, pitch, sneak)` | `self.oracle.predict(&self.c.w, item, hit.pos, hit.face, hit.loc, hit.inside, yaw, pitch, sneak)`; a constructed `BlockHitResult(Vec3 a, Direction f, BlockPos n, false)` is `geom::p(nx, ny, nz), f as u8, [ax, ay, az], false`; `oracle.placedAt` is `self.oracle.placed_at` |
| `yawTo(dx, dz)`, `(float) -Math.toDegrees(Math.atan2(dy, hd))` | `yaw_to(dx, dz)`, `pitch_to(dy, hd)` (`bot/mod.rs`) |
| `c.keys(forward, back, left, right, jump, sneak, sprint)`, `c.eye()` | the same; `eye()` is `[f64; 3]` |
| `c.player.getHealth()`, `isDeadOrDying()` | `self.c.pl.health`, `health <= 0.0` |
| `throw new OutOfTime()` | `std::panic::panic_any(OutOfTime)` (caught in `Bot::run`) |
| `try { A } finally { restore }` | `A; restore;` - and where `A` has an early `return`, split it into a function so that the restore runs (`remove_scaffold` / `remove_scaffold_tries`) |
| `for (init; a && b; step) { ... continue; ... }` | a `while` with the step written out before every `continue` - the usual place for an off-by-one |
| `if (n++ < k)` | `{ let v = n; n += 1; v < k }` - only evaluated where Java evaluates it (short-circuit) |
| `placed++` | `self.placed += 1; self.ev_placed(x, y, z);` (the event log's `P` line) |
| `static final boolean X_FOO = System.getenv("MCB_NO_FOO") == null` | a flag in `rev.rs` reading the same variable; used as `crate::rev::rev().x_foo` |
| a change with no Java switch | `crate::rev::rev().<revision flag>` (`a1` = at or after 2d9a193) |
| `log(...)`, `if (verbose) ...`, `TRACE`, `WATCH`, `PARANOID` | not ported, unless the code inside changes state (then only that part) |
| `String.format("%.3f", x)` in a result row | `build::fmt3(x)` |
| `float` arithmetic, `(int)`, `Math.round`, `Mth.sin` | `f32` until Java widens, `as i32`, `jround`, `physics::msin` |

## Brief for a review of a port

Give each reviewer one group of classes (about 300 changed Java lines each worked well) and this, filled in:

> You are reviewing a Java-to-Rust port for behavioural fidelity. Read-only: do not edit files or build. The Rust
> simulator in work/sim must reproduce, tick for tick, what the Java bot does. The Java source went from commit
> OLD to NEW; the Rust port is git commit C (`git show C -- <file>`; `git show C^:<file>` is the old Rust).
> `python3 tools/port.py diff <NEW src> <Class.>` prints the Java diff member by member with the Rust location of
> each (before `port.py finish`; afterwards `git show C -- port/java`). Conventions: port/CONVENTIONS.md.
> Your scope: classes ... / files ...
> Check, statement by statement: same conditions, loop orders and bounds; same order of side effects (ticks pass
> inside tick / settle / travel / centre / break_block; searches; counters that are read back; fields set and
> restored); Java arithmetic and evaluation order. Check the guards: with the old revision every flag is false and
> the code must behave as the old Rust did; with the new one as the new Java. An unguarded change must be a
> provable no-op in the old revision. Not ported on purpose (do not report): ...
> Report a numbered list of concrete discrepancies (Java file:line, Rust file:line, what differs, when behaviour
> would differ), then what you checked and found equivalent, then anything uncertain. No style remarks.
