#!/bin/bash
# batch.sh <list> <results.tsv> [wallSecondsPerBuild] [buildsPerProcess]
# Runs RunCorpus over a list until everything is done, restarting the server process now and then (and after a crash).
# Environment: MCB_CLASSES (classes directory, default harness/build/classes), HEAP (default 6g).
LIST=$1; OUT=$2; WALL=${3:-600}; PER=${4:-60}
cd "$(dirname "$0")/.." || exit 1
for round in $(seq 1 400); do
  MCB_SNAP=1 JAVA_OPTS="-Xmx${HEAP:-6g} -XX:+UseSerialGC" harness/run.sh corpus "$LIST" "$OUT" "$PER" 86400 "$WALL" >> "${OUT%.tsv}.log" 2>&1
  code=$?
  if [ $code -eq 3 ]; then echo "all done"; break; fi
  # a crash inside one build: make sure that build is not retried forever
  if [ $code -ne 0 ] && [ $code -ne 2 ]; then   # (2 = out of memory, row already written)
    next=$(python3 - "$LIST" "$OUT" <<'PY'
import sys
done=set(l.split('\t')[0] for l in open(sys.argv[2])) if __import__('os').path.exists(sys.argv[2]) else set()
for l in open(sys.argv[1]):
    h=l.split()[0] if l.strip() else ''
    if h and h not in done: print(h); break
PY
)
    [ -n "$next" ] && printf '%s\tFAIL\tcrash: process exit %s\n' "$next" "$code" >> "$OUT"
  fi
done
