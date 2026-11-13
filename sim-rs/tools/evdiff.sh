#!/bin/bash
# runs one traced build and shows the first difference from the Java event log. usage: evdiff.sh <hash8> [context]
h8=$1; ctx=${2:-6}
full=$(grep -h "^$h8" /corpus/unique.tsv | head -1 | cut -f1)
ref=/work/reference/java-extra/events/ev-$h8.txt
/work/sim/target/release/mcsim run $full /dev/null --events /work/tmp/ev-$h8.txt 2>&1 | tail -1 | cut -c1-200
line=$(cmp <(cat $ref) /work/tmp/ev-$h8.txt 2>/dev/null | awk '{print $NF}')
if [ -z "$line" ]; then echo "event logs identical ($(wc -l < $ref) events)"; exit; fi
echo "first difference at event line $line of $(wc -l < $ref) (ref) / $(wc -l < /work/tmp/ev-$h8.txt) (sim)"
echo "--- java"; sed -n "$((line>ctx?line-ctx:1)),$((line+ctx))p" $ref
echo "--- sim";  sed -n "$((line>ctx?line-ctx:1)),$((line+ctx))p" /work/tmp/ev-$h8.txt
