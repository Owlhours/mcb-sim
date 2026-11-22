# History

How the measurement side came about, how to read the git log, and what the names in it mean. All dates are
November 2026, in UTC.

## The git history

Three development repositories were filtered down to the files published here and interleaved by author date
into one linear history. Each source's own order is kept, along with the original messages, authors and dates.
The tree was rebuilt at each step from the latest state of each part.

| part | from | commits here | author |
|---|---|---|---|
| `sim-rs/` until 11-13 17:00 | the simulator's own repository (its whole first day, before it joined the bot's tree) | 42 | `Owlhours` (rewritten from the personal identity that session committed under) |
| `sim-rs/` from 11-13 17:13, `harness/`, `scripts/` | the bot's repository | 139 | `mcb <mcb@localhost>` (the development loop) |
| `fastsim/` (`fastsim/mcsim-ref/` was `ref/mcsim-src/` there) | fastsim's repository | 43 | `fastsim <fastsim@localhost>` (the development loop) |
| the last commit | this repository | 1 | `Owlhours` |

How to read it:
- **First-parent only.** The bot's and the simulator's repositories had merged branches. Only each one's
  first-parent line is kept, so a merged branch shows up as its merge commit.
- **Dropped commits.** Commits that changed nothing published here were dropped: status notes, result rows,
  sample lists, the Fabric mod, fastsim's benchmark rows. Many kept messages therefore mention files that aren't
  here. Messages ending `(no change to the bot)` changed only tooling.
- **Moved paths.** Development ran in containers with a fixed absolute root, and the old commits keep those
  absolute paths. In the bot's tree, that root's `src/` is now `harness/src/`. In the simulator's first
  repository, the root was what is now `sim-rs/`. The final commit makes every script run from a checkout.
- **Building old commits.** Before the final commit, nothing builds outside the development containers. The
  state tables, the corpus and the server jar were never in git.
- **Short hashes in messages** (`2d9a193`, `f6b21f4`, `cc43f93` ...) are commits of the bot's development
  repository, not of this one. `d650a66` and `533fdd6` in fastsim's messages are too.
- **Party names in messages.** "A" is the bot's development session (the owner of `mcsim` from 11-13 on). "D" is
  the second, independent Java port that ported and checked each revision in Java. "The host" or "the monitor"
  is the user's own supervising session, and "the owner" is the user. That session's briefs, which messages
  cite by file name, are not published.

The bot's Java source has its own published history in
[MCB Builder](https://github.com/Owlhours/mcb-builder). `harness/src/mcb/{bot,build,env,schem}` is the same code
as its `builder/src/mcb` at the release, apart from the mod-only `ModView` class, which the harness doesn't have.

## Timeline

| date | what happened | measure |
|---|---|---|
| 11-12 | The Java bot and its environment: vanilla 26.2 in-process, idle-world fast ticking, schematic loaders, corpus index, judge, site, the first working builder; the same day, placement tables and the rules oracle, scaffolding, doors, restocking, clean-up. The first full run of one commit (`59dc842`, 320 builds) became the reference for the failure model. | — |
| 11-13 01:22 | The Rust simulator was started in a separate session with no Java allowed, as a port of the Java bot at `f6b21f4`. It read the bot's source, decompiled vanilla classes and Java result rows. | — |
| 11-13 02:07 | Milestone 1: a complete port. | 167 of 220 development builds exact |
| 11-13 06:08 | Milestone 2: route search over a padded per-cell mirror (4× fewer ns per expansion). | 284 / 290 exact |
| 11-13 08:14 | Milestone 3: health and damage, sculk vibrations. | 290 / 290 verdicts, 287 exact (`f6b21f4`) |
| 11-13 17:00 | Milestone 4: the port brought to the bot's new master. | `2d9a193`: 290 / 290 verdicts, 288 exact |
| 11-13 17:13 | The simulator moved into the bot's tree and became the inner loop: all five development sets (290 builds) in 4.5 minutes, against an hour or more for the large set alone in Java. The evaluation method was rewritten after chance was measured directly (METHOD.md §7). | — |
| 11-13–14 | Revisions `mend`, `orphan`, `close`, `chain`, `careful`, then `fewfloat` and `escape`, each made in the simulator first and ported to Java by the second Java port. The judge was frozen, with operator-only blocks exempt. Timelines, derailment detection and checkpoints (`mcsim resume`). | first goal sample: 896 / 1,000 |
| 11-16–17 | Route search made cheaper without changing results: `askonce`, `plankeep`, `thoughtway` (search cells ×0.58). Decisions moved to 4–6 seeds. | — |
| 11-17 20:01 | fastsim started in its own session: "a speed-first simulator of the same task" with a target of 10⁶× real time. Copies of `mcsim` were linked in for reference. | — |
| 11-17–18 | fastsim's fidelity ladder: click masks from `mcsim`'s placement tables, tick times fitted to `mcsim`'s routes, `mcsim`'s helper, orphan, close and chain rules as switches, then levels 1–3. | 1,066,000× median (development) |
| 11-18 | The bot's large-build work: `repair`, `ropefar`, `excavate`, `minefence`, `lastbreak`, `samefail`, `weblate`. A candidates queue was built (`MCSIM_CAND`, one binary for many screened candidates). fastsim's scaffolding, trips and put-off cells were brought closer to `mcsim`. | 918 / 1,000 |
| 11-19 | The impossible-cell judge rule. fastsim gained failing walks and clicks (measured in `mcsim`'s event logs), PGO, a packed palette, and the cheat ladder ("clicking is the bottleneck"). | fastsim 1.06M× (development), 1.12M× (triage) |
| 11-19–20 | Revisions `nearclean`, `hungtower`, `magmasneak`, `nostrand`, `dearbreak`, `nostand`, `walkway`. fastsim's oracle studies: "the bot fails, not the targets". | 927–930 / 1,000 |
| 11-21 | `covertour`. The first goal sample closed at 929. The second sample was drawn and measured once. The second Java port's final check: the Java bot played the simulator's course on 39 of 40 builds. Both projects closed. | **963 / 1,000**, 10k+ 265 / 297 |

## Glossary of `--rev` names

`mcsim run <build> --rev NAME` predicts the bot as it was at that revision (default `covertour`; `MCSIM_REV`
does the same). Each revision includes all earlier ones. Most can be switched off one by one with an
environment variable, in `mcsim` and in the Java bot alike, so that a change in results can be blamed on one of
them. The table is from `sim-rs/sim/src/rev.rs`.

| revision | date | what it does | switch |
|---|---|---|---|
| `f6b21f4` (`base`) | 12 | The Java bot of 11-12 that the port started from. | — |
| `2d9a193` (`a1`) | 13 | The bot's master of 11-13: remembered ways out, hazards, last-resort placements, end game. | — |
| `mend` | 13 | After the last clean-up rounds broke through the build to reach scaffolding, put back what was broken (nothing new put up, no breaking out). | — |
| `orphan` | 13 | A log (or similar) whose column starts in mid-air gets a helper block on a second helper, instead of being put off for ever. | — |
| `close` | 13 | Last resort for cells with no usable face (observers, hoppers, pistons): a helper block on each side in turn, with a fine search of spots off the middle of blocks. | — |
| `chain` | 13 | A helper resting on a second helper, allowed in builds with pistons once everything else stands. | — |
| `careful` | 14 | The clean-up takes a piece away only from a place it can still walk back to the chests from. | `MCB_NO_CAREFUL` |
| `fewfloat` | 14 | In the careful clean-up, at most two stands hung in the air per piece, each with a capped search. A speed-up. | `MCB_FLOAT_CAREFUL=1` restores the old way |
| `escape` | 14 | An enclosed bot may, on its last try, break a block unless a neighbour would really come off with it. | `MCB_NO_ESCAPE` |
| `askonce` | 16 | Way-out searches that don't settle quickly switch to a search that visits each place once. Tunnelling first asks once for a way to any place next to the cell. | `MCB_NO_PFHEUR`, `MCB_NO_PFTUN` |
| `plankeep` | 17 | The stance planner keeps its search between questions about the same world. | `MCB_NO_PLANKEEP` |
| `thoughtway` | 17 | A way out found in a world-in-thought is kept while the same world is asked about. | `MCB_NO_THOUGHTWAY` |
| `repair` | 18 | After the end game, sections that still fail by the bot's own picture are built again, then the end game runs again, while bad cells keep falling. | `MCB_NO_REPAIR` |
| `ropefar` | 18 | Behind pace, the bot doesn't walk back to take down pieces of its own way that lie 6 or more blocks behind; the clean-up gets them. | `MCB_NO_ROPEFAR` |
| `excavate` | 18 | In the repair pass, a cell shut in by standing blocks is reached through a short shaft, which is closed again after. | `MCB_NO_EXCAVATE` |
| `minefence` | 18 | A bot fenced in can break out. | `MCB_NO_MINEFENCE` |
| `lastbreak` | 18 | An enclosed bot whose last-resort search failed makes one more, in which it may break a block whatever comes off with it. | `MCB_NO_LASTBREAK` |
| `samefail` | 18 | A put-off cell that failed is not tried again under the same rules until something within two cells of it changes, or its block comes into the inventory. | `MCB_NO_SAMEFAIL` |
| `weblate` | 18 | Cobwebs wait until the first clean-up is over, because the bot can't move in them. | `MCB_NO_WEBLATE` |
| `nearclean` | 19 | Behind pace or past half its time, the clean-up takes the cheapest piece to walk to (with a penalty per level below the highest) instead of the highest. | `MCB_NO_NEARCLEAN` |
| `hungtower` | 20 | **Hung back:** when the way back to the top of its own way is lost, the bot climbs back to it instead of leaving it hanging. **Tower serve:** after putting up a long way for one piece, it takes every other piece reachable from up there first. | `MCB_NO_HUNG`, `MCB_NO_HUNGTOWER` |
| `magmasneak` | 20 | On a hot floor (magma) with no safe spot reached, the bot crouches, and the clean-up steps aside when hurt. | `MCB_NO_MAGMASNEAK` |
| `nostrand` | 20 | Before taking away the temporary block it stands on, the bot checks it wouldn't come down somewhere with no way back. | `MCB_NO_NOSTRAND` |
| `dearbreak` | 20 | In last-resort searches, breaking a finished block with no spare in the chests costs far more. After breaking an opening, the bot tries again to walk in without breaking more. | `MCB_NO_DEARBREAK` |
| `nostand` | 20 | The bot keeps the block under its feet when it would come down where it can't stand (a floor of hoppers or cauldrons, say). | `MCB_NO_NOSTAND` |
| `walkway` | 20 | Behind pace, scaffolding put out sideways from a column top makes a walkway, up to 6 pieces hanging from one column. The clean-up leaves hanging pieces to last. | `MCB_NO_WALKWAY` |
| `covertour` | 21 | Behind pace, before picking the next cell, the bot walks (putting up nothing) to the spot within four blocks that serves the most cells now, and places them. | `MCB_NO_COVERTOUR` |

**Other switches** read by `mcsim` (and, under the same names, by the Java bot):
- `MCB_SEED=k` shifts the start position by k blocks. This is how seeds are made.
- `MCB_NO_EXIT`, `MCB_NO_UNSTICK`, `MCB_NO_HAZ`, `MCB_NO_MONITOR` and the other `MCB_NO_*` in `rev.rs` switch off
  older parts.
- `MCB_OWNDOWN`, `MCB_HOPPERFLOOR` and `MCSIM_CAND=<names>` (screened candidates in `sim/src/cand.rs`, such as
  `walkstandbp` and `odbp`) are things that were tried and not taken. They are off by default.

**Judge decisions** were made separately from the bot:
- **11-14:** operator-only blocks exempt.
- **11-19:** cells that can't exist in survival exempt.
- **11-20:** cells hanging only from such cells exempt, and a bamboo shoot counts as bamboo.

## Names you will meet in the code and messages

| name | meaning |
|---|---|
| development sets `small`, `strat-small`, `dev2`, `dev3`, `dev2big` | the five lists of the first days (290 builds; 220 small, 40 in `dev3`, 30 large in `dev2big`) with Java rows; `dev3` was the hold-out of the port |
| `wide`, `nwtps`, `triage` | 300 builds drawn for the simulator only; 753 small builds labelled "no way to place"; 47,941 builds up to 6,000 blocks classified by cause at `2d9a193` |
| `lgfail`, `hugedev`, `huge33`, `nrdev1` | dev lists of failing large builds, debugged on, never decided on |
| `big-ho*`, `huge-ho*`, `mag-ho1`, `huge-ho9` ... | fresh held-out decision sets, one per decision, read once |
| `goal-corpus1000`, `goal2-corpus1000` | the two goal samples (METHOD.md §8) |
| the rope | the ordered list of loose blocks the bot put up as bridges, taken away again before it moves on |
| witness, way out | the one concrete path from the bot back to the chests that the bot keeps, with the cells it depends on |
| put off, end game, repair pass | cells retried after the main pass; the clean-up rounds; `repair` |
| `unmodelled`, `lost`, `random` | `mcsim`'s counters of events it does not model (§3 of METHOD.md) |
| fastsim levels 0–3, "the ladder" | how much of `mcsim` fastsim borrows; level 3 is `mcsim` |
| cheat ladder, oracles | fastsim studies that turn one limitation off at a time (reach, sight, stock ...) to see what the bot loses to it |
