#!/bin/bash
# regress.sh : runs the three development samples with the classes in build/dev (results in tmp/), then prints the summaries.
cd /work/tmp || exit 1
export MCB_CLASSES=build/dev MCB_SNAP=1 JAVA_OPTS="-Xmx4g -XX:+UseSerialGC"
rm -f reg.done results-small-dev.tsv results-strat-small-w.tsv results-dev2b.tsv
/work/run.sh mcb.run.RunCorpus /work/tmp/sample-small.txt /work/tmp/results-small-dev.tsv 200 86400 300 > /work/tmp/reg1.out 2>&1
/work/run.sh mcb.run.RunCorpus /work/tmp/sample-strat-small.txt /work/tmp/results-strat-small-w.tsv 200 86400 400 > /work/tmp/reg2.out 2>&1
/work/run.sh mcb.run.RunCorpus /work/tmp/sample-dev2.txt /work/tmp/results-dev2b.tsv 200 86400 900 > /work/tmp/reg3.out 2>&1
for f in results-small-dev.tsv results-strat-small-w.tsv results-dev2b.tsv; do python3 /work/scripts/summarize.py $f | head -2; done > reg.done
