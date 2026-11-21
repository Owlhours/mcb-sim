#!/bin/bash
# countrun.sh <bin> <hashes.txt> <out.txt>   [J=6]
# One `COUNTS <hash> SXxSYxSZ kind=n ...` line per build (MCSIM_COUNT='*', the target loaded and counted, --stop-at 1),
# appended to <out.txt>; builds already in it are skipped.
bin=$1; list=$2; out=$3
touch "$out"
one() { r=$(MCSIM_COUNT='*' timeout 120 "$0" run "$1" /dev/null --stop-at 1 2>/dev/null | grep -m1 ^COUNTS); [ -n "$r" ] && echo "COUNTS $1 ${r#COUNTS }" || echo "NOCOUNT $1"; }
export -f one
cut -d' ' -f1 "$list" | grep -v -x -F -f <(awk '{print $2}' "$out") | xargs -P ${J:-6} -I{} bash -c 'one {}' "$bin" >> "$out"
