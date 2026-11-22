#!/bin/bash
# shard.sh <list> <tag> [workers] [classes-dir] [heap] [wallSecondsPerBuild]
# Runs a sample from scratch on several cores: the list is dealt out by size over the workers, each worker runs
# batch.sh on its share from a frozen copy of the classes, and the rows are gathered in work/tmp/results-<tag>.tsv
# (per-worker logs in work/tmp/<tag>.s*.log). Prints the summary at the end.
LIST=$1; TAG=$2; N=${3:-4}; CLASSES=${4:-harness/build/classes}; HEAP=${5:-3g}; WALL=${6:-900}
cd "$(dirname "$0")/.." || exit 1
mkdir -p work/tmp
FROZEN=work/tmp/frozen-$TAG
rm -rf "$FROZEN"; cp -r "$CLASSES" "$FROZEN"
rm -f work/tmp/$TAG.s*.txt work/tmp/$TAG.s*.tsv work/tmp/$TAG.s*.log work/tmp/results-$TAG.tsv
python3 - "$LIST" "$TAG" "$N" <<'PY'
import sys
sys.path.insert(0, 'scripts')
import results
hs = [l.split()[0] for l in open(sys.argv[1]) if l.strip()]
hs.sort(key=lambda h: -results.blocks_of(h))
n = int(sys.argv[3])
for i in range(n):
    open(f'work/tmp/{sys.argv[2]}.s{i}.txt', 'w').write(''.join(h + '\n' for h in hs[i::n]))
PY
for i in $(seq 0 $((N - 1))); do
  MCB_CLASSES=$FROZEN HEAP=$HEAP scripts/batch.sh work/tmp/$TAG.s$i.txt work/tmp/$TAG.s$i.tsv "$WALL" 60 > /dev/null 2>&1 &
done
wait
cat work/tmp/$TAG.s*.tsv > work/tmp/results-$TAG.tsv
rm -rf "$FROZEN"
python3 scripts/summarize.py work/tmp/results-$TAG.tsv > work/tmp/results-$TAG.summary
echo done >> work/tmp/results-$TAG.summary
