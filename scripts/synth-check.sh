#!/usr/bin/env bash
# mcsim against the Java bot on MCB Builder's generated test builds (its regression gate), with nothing private:
# exports the gate's 31 test builds as .mcb files (MCB Builder's mcb.headless.Gate export, in the headless game),
# runs mcsim on each, and compares verdict, status, blocks, ticks, placed, skipped, missing, wrong, leftover,
# sections and failed sections with the Java rows recorded in MCB Builder's gate/expected/26.2.tsv.
#
#   scripts/synth-check.sh             (needs: the harness built (harness/build.sh), Rust 1.93+, python3)
#   J=4 scripts/synth-check.sh         parallel mcsim runs (default 4)
#
# Writes into work/synth/ (not in git). mcsim also needs the state tables (harness/run.sh tables) in work/reference.
set -euo pipefail
cd "$(dirname "$0")/.."
R=$PWD
MB=$(harness/mcb-builder.sh)
W=$R/work/synth; rm -rf "$W"; mkdir -p "$W"
[ -f work/reference/mc/states.tsv ] || harness/run.sh tables
(cd sim-rs/sim && cargo build --release -q)
CP="$R/harness/build/classes:$(harness/mc-classpath.sh)"
[ -f harness/build/classes/mcb/headless/Gate.class ] || harness/build.sh
(cd "$W" && java -cp "$CP" mcb.headless.Gate export "$W/corpus" 2> /dev/null | grep -v '^\[' > "$W/names.tsv")
cut -f2 "$W/names.tsv" | xargs -P "${J:-4}" -I{} sh -c "MCB_WORK=$R/work MCSIM_CORPUS=$W/corpus sim-rs/sim/target/release/mcsim run {} > $W/{}.row 2> $W/{}.log"
python3 - "$W" "$MB/gate/expected/26.2.tsv" <<'PY'
import sys, os
w, exp = sys.argv[1], sys.argv[2]
names = dict(l.rstrip('\n').split('\t')[:2] for l in open(os.path.join(w, 'names.tsv')))
want = {f[0]: f for f in (l.rstrip('\n').split('\t') for l in open(exp)) if f[0] and not f[0].startswith('#')}
ok = 0
for name, h in sorted(names.items()):
    j = want[name]
    s = open(os.path.join(w, h + '.row')).read().rstrip('\n').split('\t')
    jv = [j[2], j[3], j[4], j[5], j[6], j[7], j[8], j[9], j[10], j[11], j[12]]   # verdict .. failed sections
    sv = s[1:12]
    if jv == sv: ok += 1
    else: print('differs', name, 'java', jv, 'mcsim', sv)
print(f'mcsim matches the Java bot on {ok} of {len(names)} test builds')
sys.exit(0 if ok == len(names) else 1)
PY
