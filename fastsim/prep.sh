#!/usr/bin/env bash
# fastsim/prep.sh <list> <out.fsb>: everything fastsim needs for a list of builds.
#   1. builds fastsim and its copy of mcsim (cargo build --release; no dependencies, works offline)
#   2. once: $MCB_WORK/fastsim/states.tsv and click.bin, from mcsim's tables (dump_states, dump_click)
#   3. packs the builds of <list> (one hash per line, from $MCB_WORK/corpus) into <out.fsb>
# Needs $MCB_WORK/reference/mc/states.tsv (harness/run.sh tables) and the builds in $MCB_WORK/corpus
# (harness/run.sh prep <files>).
set -euo pipefail
F="$(cd "$(dirname "$0")" && pwd)"
export MCB_WORK="${MCB_WORK:-$(dirname "$F")/work}"
[ $# -eq 2 ] || { echo "usage: fastsim/prep.sh <list> <out.fsb>" >&2; exit 2; }
(cd "$F" && cargo build --release -q) && (cd "$F/mcsim-ref" && cargo build --release -q --bins)
mkdir -p "$MCB_WORK/fastsim"
if [ ! -s "$MCB_WORK/fastsim/states.tsv" ] || [ ! -s "$MCB_WORK/fastsim/click.bin" ]; then
    "$F/mcsim-ref/target/release/dump_states" > "$MCB_WORK/fastsim/states.tsv"
    "$F/mcsim-ref/target/release/dump_click" "$MCB_WORK/fastsim/click.bin"
fi
"$F/target/release/fastsim" prep "$1" "$2"
