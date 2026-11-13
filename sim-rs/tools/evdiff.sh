#!/bin/bash
# runs one traced build and shows the first difference from the Java event log.
# usage: [REV=<commit>] [EVDIR=<dir with ev-<hash8>.txt>] evdiff.sh <hash8> [context]
# REV defaults to the commit the port is at; the Java logs are looked for in reference/java-<REV>/events
# (reference/java-extra/events for f6b21f4).
h8=$1; ctx=${2:-6}
REV=${REV:-$(cat /work/port/java/COMMIT)}
if [ -z "$EVDIR" ]; then if [ "$REV" = f6b21f4 ]; then EVDIR=/work/reference/java-extra/events; else EVDIR=/work/reference/java-$REV/events; fi; fi
full=$(grep -h "^$h8" /corpus/unique.tsv | head -1 | cut -f1)
ref=$(ls $EVDIR/ev-$h8*.txt 2>/dev/null | head -1)
if [ -z "$ref" ]; then echo "no Java event log for $h8 in $EVDIR"; exit 1; fi
/work/sim/target/release/mcsim run $full /dev/null --rev $REV --events /work/tmp/ev-$h8.txt 2>&1 | tail -1 | cut -c1-200
line=$(cmp <(cat $ref) /work/tmp/ev-$h8.txt 2>/dev/null | awk '{print $NF}')
if [ -z "$line" ]; then echo "event logs identical ($(wc -l < $ref) events)"; exit; fi
echo "first difference at event line $line of $(wc -l < $ref) (ref) / $(wc -l < /work/tmp/ev-$h8.txt) (sim)"
echo "--- java"; sed -n "$((line>ctx?line-ctx:1)),$((line+ctx))p" $ref
echo "--- sim";  sed -n "$((line>ctx?line-ctx:1)),$((line+ctx))p" /work/tmp/ev-$h8.txt
