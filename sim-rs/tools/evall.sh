#!/bin/bash
# Compares the simulator's placement logs with the Java side's for every build of the given sets.
# usage: [REV=<commit>] [EVDIR=<dir with the Java ev-<hash8>.txt>] evall.sh [set ...]
# Prints one line per build whose log really differs and the counts by kind of difference (tools/evclass.py).
# SKIPRUN=1: compare the logs already in work/tmp/ev-$REV without running the builds again.
REV=${REV:-$(cat sim-rs/port/java/COMMIT)}
if [ -z "$EVDIR" ]; then if [ "$REV" = f6b21f4 ]; then EVDIR=work/reference/java-extra/events; else EVDIR=work/reference/java-$REV/events; fi; fi
sets=${@:-small strat-small dev2}
out=work/tmp/ev-$REV; mkdir -p $out
pids=()
[ -n "$SKIPRUN" ] && sets_run= || sets_run=$sets
for s in $sets_run; do
  list=work/tmp/$REV-$s.txt; [ -f $list ] || list=work/tmp/$s.txt
  sim-rs/sim/target/release/mcsim run $list work/tmp/sim-$REV-$s.tsv --rev $REV --events-dir $out > work/tmp/log-$REV-$s.txt 2>&1 &
  pids+=($!)
  if [ ${#pids[@]} -ge 2 ]; then wait ${pids[0]}; pids=("${pids[@]:1}"); fi
done
wait
declare -A n
for s in $sets; do
  list=work/tmp/$REV-$s.txt; [ -f $list ] || list=work/tmp/$s.txt
  for h in $(cut -d' ' -f1 $list); do
    h8=${h:0:8}
    if [ ! -f $EVDIR/ev-$h8.txt ]; then n[none]=$((n[none]+1)); continue; fi
    c=$(python3 sim-rs/tools/evclass.py $EVDIR/ev-$h8.txt $out/ev-$h8.txt)
    k=${c%% *}; n[$k]=$((n[$k]+1))
    if [ "$k" = different ]; then echo "$s $h8: first differing tick ${c#* } ($(wc -l < $EVDIR/ev-$h8.txt) events in Java, $(wc -l < $out/ev-$h8.txt) in the simulator)"; fi
  done
done
echo "placement logs: identical ${n[identical]:-0}; same events in another order within a tick ${n[order]:-0}; differing only in random ages ${n[random]:-0}; different ${n[different]:-0}; no Java log ${n[none]:-0}"
