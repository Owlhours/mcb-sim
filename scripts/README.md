# scripts: the evaluation tooling

These are the scripts every decision in the project went through, kept as they were used except for their paths.
Run them from the repository root. They read and write under `work/` (`MCB_WORK`):
- `work/corpus/unique.tsv` and the `.mcb` cache (`harness/run.sh prep` makes both for your own files);
- lists of hashes in `work/samples/sample-<set>.txt` or `work/tmp/2d9a193-<set>.txt`;
- simulator rows in `work/tmp/sim/`.

The method behind them is in [../docs/METHOD.md](../docs/METHOD.md) §7.

## Running sets in the simulator

| script | what it does |
|---|---|
| `simrun.sh <tag> [set ...]` | runs sets in `mcsim`, one build per process, `J` at a time (default 8), largest first; rows to `work/tmp/sim/<tag>-<set>.tsv`, logs to `work/tmp/sim/<tag>.d/`; passes `REV`, `MCB_SEED` and candidate switches through |
| `simseeds.sh <tag> "<seeds>" [set ...]` | the same under several seeds (`<tag>-s<k>`) |
| `simcmp.py <tagA> <tagB> [set ...] [--seeds 0,1,2] [--list]` | two runs build by build: verdict changes, ticks, bad cells |
| `seedgate.py <baseTag> <candTag> ...` | a candidate against the base over several seeds, measured against the base's own seed noise; prints the interval of the summed difference |
| `simcost.py <tag> [<tagB>]` | work done by a run (search cells, expansions), independent of how busy the machine is |
| `countrun.sh <bin> <hashes> <out>` | block-kind counts per build (`MCSIM_COUNT`) |
| `causes.py <dir>` | failed sections by cause, from `MCSIM_CAUSES=<dir>` runs |
| `cklocal.py` | local tests of a fix from checkpoints just before each failure (`mcsim resume`) |
| `blockclasses.py`, `facecost.py`, `stand*.py`, `reachline.py` | diagnostics over `mcsim` side logs (`MCSIM_STANDLOG`, `MCSIM_FACELOG`, cause files) and rows |

Example with your own builds:

```bash
harness/run.sh prep my-schematics/* && mkdir -p work/samples && cp work/corpus/prep-list.txt work/samples/sample-mine.txt
(cd sim-rs/sim && cargo build --release)
scripts/simrun.sh base mine                  # mine: N rows, P pass
MCB_NO_COVERTOUR=1 scripts/simrun.sh nocover mine
python3 scripts/simcmp.py base nocover mine
```

## Running sets in Java

| script | what it does |
|---|---|
| `batch.sh <list> <rows.tsv> [wall-s] [per-process]` | `harness/run.sh corpus` over a list until done, restarting the server process now and then and after a crash |
| `shard.sh <list> <tag> [workers] ...` | a list dealt out by size over several workers from a frozen copy of the classes; rows in `work/tmp/results-<tag>.tsv` |
| `regress.sh [classes]` | the three small development sets in parallel (needs those lists) |
| `summarize.py <rows.tsv>` | pass counts by size class and status for a `RunCorpus` results file |
| `jfrtop.py` | self and inclusive time per method from a Java Flight Recorder dump |

## Statistics and samples

| script | what it does |
|---|---|
| `evalmath.py` | the numbers behind the evaluation method: corpus weights per size class, the failure model `-ln P(pass) = α + κN`, chance per perturbed build, Skellam tables |
| `compare.py A.tsv B.tsv` | two result files: verdict changes, per-size-class sums, exact McNemar, the sign-flip test on log10(1 + bad cells), the Skellam guard, collapses |
| `rejudge_cmp.py`, `impossible_rescore.py` | stored runs re-judged under a judge rule, against the originals |
| `results.py` | shared helpers: size classes, block counts, row parsing |
| `draw_sample.py <seed> <per-stratum> <out>` | a stratified random sample from `work/corpus/unique.tsv`, excluding given lists |
| `draw_goal2.py pool / draw / sub` | the second goal sample's filters (no redstone kinds, no impossible cells, fresh) and decision sets drawn with them |
| `draw_redstone.py` | the redstone-heavy lists |
| `make_unique.py` | `corpus/index.tsv` (one line per file) → `corpus/unique.tsv` (one line per distinct build) |
| `split_members.py` | a refactoring helper: lists the top-level members of a Java class |
