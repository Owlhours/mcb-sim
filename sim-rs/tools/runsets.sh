#!/bin/bash
# runs the development sets (two at a time) and prints the comparison; usage: runsets.sh [sets...]
cd /work/sim
sets=${@:-small strat-small dev2}
pids=()
for s in $sets; do
  ${MCSIM:-./target/release/mcsim} run /work/tmp/$s.txt /work/tmp/sim-$s.tsv > /work/tmp/log-$s.txt 2>&1 &
  pids+=($!)
  if [ ${#pids[@]} -ge 2 ]; then wait ${pids[0]}; pids=("${pids[@]:1}"); fi
done
wait
for s in $sets; do python3 /work/tools/compare.py /work/tmp/sim-$s.tsv $s | tail -1; done
