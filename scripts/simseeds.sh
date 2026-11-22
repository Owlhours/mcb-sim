#!/bin/bash
# Runs sample sets under several seeds: scripts/simseeds.sh <tag> "<seeds>" [sets...]   e.g. simseeds.sh tw1 "0 1 2 3" big80
# Rows as scripts/simrun.sh writes them, tag <tag> for seed 0 and <tag>-s<k> for seed k (what seedgate.py and simcmp.py read).
# Seeds whose rows are all there already are not run again. Other environment (REV, BIN, J, candidate switches) is passed through.
cd "$(dirname "$0")/.." || exit 1   # (paths below are relative to the repository root)
tag=$1; seeds=$2; shift 2
sets=${@:-dev2big small strat-small dev2 dev3}
for k in $seeds; do
  t=$tag; [ "$k" != 0 ] && t=$tag-s$k
  have=1; for s in $sets; do [ -s work/tmp/sim/$t-$s.tsv ] || have=0; done
  if [ $have = 1 ]; then echo "seed $k: rows of $t are there"; continue; fi
  echo "seed $k:"; MCB_SEED=$k scripts/simrun.sh $t $sets
done
