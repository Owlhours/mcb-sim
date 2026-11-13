#!/bin/bash
# Runs sample sets in the Rust simulator (sim-rs), one build per process, N at a time.
# usage: [REV=..] [J=8] [BIN=..] scripts/simrun.sh <tag> [set ...]     sets: small strat-small dev2 dev3 dev2big holdout wide
# (holdout = samples/sample-holdout.txt: counts by class only; wide = samples/sample-wide.txt, 300 builds drawn 2026-11-13
#  for the simulator, never run in Java. A build that takes more than TMO seconds of wall time has no row.)
# Lists are /work/tmp/2d9a193-<set>.txt (sim-rs/tools/mklists.py 2d9a193; a second column is the tick to stop at).
# Output: /work/tmp/sim/<tag>-<set>.tsv (rows in list order), stderr of each build in /work/tmp/sim/<tag>.d/<hash>.log
# Extra environment (MCB_SEED, MCSIM_... candidate switches) is passed through to the simulator.
# Compare two runs with scripts/simcmp.py <tagA> <tagB> [set ...] [--seeds 0,1,2] [--list].
tag=$1; shift
sets=${@:-dev2big small strat-small dev2 dev3}
BIN=${BIN:-/work/sim-rs/sim/target/release/mcsim}
out=/work/tmp/sim; d=$out/$tag.d; mkdir -p $d
# (the run keeps its own copy of the binary: the simulator can be rebuilt while it is under way)
cp $BIN $d/mcsim; BIN=$d/mcsim
one() {
  h=$1; stop=$2
  timeout ${TMO:-1500} $BIN run $h ${REV:+--rev $REV} ${stop:+--stop-at $stop} > $d/$h.row 2> $d/$h.log
}
export -f one; export BIN REV d TMO
# the largest builds first, so the tail of the run is not one long build on one core
for s in $sets; do cat /work/tmp/2d9a193-$s.txt; done | awk 'NF' | sort -u > $d/all.txt
python3 - $d/all.txt > $d/order.txt <<'EOF'
import sys
size = {}
for l in open('/work/corpus/unique.tsv'):
    f = l.split('\t')
    try: size[f[0]] = int(f[4])
    except Exception: pass
rows = [l.split() for l in open(sys.argv[1])]
rows.sort(key=lambda r: -size.get(r[0], 0))
for r in rows: print(' '.join(r))
EOF
xargs -P ${J:-8} -L 1 bash -c 'one $0 $1' < $d/order.txt
for s in $sets; do
  while read h stop; do [ -n "$h" ] && grep -h "^$h" $d/$h.row; done < /work/tmp/2d9a193-$s.txt > $out/$tag-$s.tsv
  echo "$s: $(wc -l < $out/$tag-$s.tsv) rows, $(cut -f2 $out/$tag-$s.tsv | grep -c '^PASS') pass"
done
