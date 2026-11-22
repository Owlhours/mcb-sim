#!/bin/bash
# regress.sh [classes-dir] : runs the three development samples (in parallel, one core each) with the classes in
# harness/build/dev (or the given directory), results in work/tmp/, then writes the summaries to work/tmp/reg.done.
cd "$(dirname "$0")/.." || exit 1
export MCB_CLASSES=${1:-harness/build/dev} MCB_SNAP=1 JAVA_OPTS="-Xmx3g -XX:+UseSerialGC"
cd work/tmp && rm -f reg.done results-small-dev.tsv results-strat-small-w.tsv results-dev2b.tsv && cd ../..
harness/run.sh corpus work/tmp/sample-small.txt work/tmp/results-small-dev.tsv 200 86400 300 > work/tmp/reg1.out 2>&1 &
harness/run.sh corpus work/tmp/sample-strat-small.txt work/tmp/results-strat-small-w.tsv 200 86400 400 > work/tmp/reg2.out 2>&1 &
harness/run.sh corpus work/tmp/sample-dev2.txt work/tmp/results-dev2b.tsv 200 86400 900 > work/tmp/reg3.out 2>&1 &
wait
for f in results-small-dev.tsv results-strat-small-w.tsv results-dev2b.tsv; do python3 scripts/summarize.py work/tmp/$f | head -2; done > work/tmp/reg.done
