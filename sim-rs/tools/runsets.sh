#!/bin/bash
# runs the development sets (two at a time) and prints the comparison; usage: [REV=f6b21f4] [MCSIM=binary] runsets.sh [sets...]
# REV: the Java commit to predict and to compare with (reference/results/$REV-<set>.tsv); output in /work/tmp/sim-$REV-<set>.tsv
REV=${REV:-$(cat /work/port/java/COMMIT)}
cd /work/sim
sets=${@:-small strat-small dev2}
pids=()
for s in $sets; do
  list=/work/tmp/$REV-$s.txt; [ -f $list ] || list=/work/tmp/$s.txt   # (tools/mklists.py <commit>: the commit's own list, with its wall-limit ticks)
  ${MCSIM:-./target/release/mcsim} run $list /work/tmp/sim-$REV-$s.tsv --rev $REV > /work/tmp/log-$REV-$s.txt 2>&1 &
  pids+=($!)
  if [ ${#pids[@]} -ge 2 ]; then wait ${pids[0]}; pids=("${pids[@]:1}"); fi
done
wait
for s in $sets; do
  if [ -f /work/reference/results/$REV-$s.tsv ]; then python3 /work/tools/compare.py /work/tmp/sim-$REV-$s.tsv $s --ref $REV | tail -1
  else echo "$s: no reference rows for $REV yet; against f6b21f4:"; python3 /work/tools/revdiff.py /work/reference/results/f6b21f4-$s.tsv /work/tmp/sim-$REV-$s.tsv | head -1; fi
done
