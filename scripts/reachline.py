#!/usr/bin/env python3
"""The goal measure's "out of reach by time" line (owner's reporting rule, 2026-11-20).
usage: reachline.py <rows.tsv> [--tpb N]   (rows of goal-corpus1000; N = minimum ticks a block, default 1)

A build is out of reach by time when its judged target cells (row column 4) times the minimum ticks a block exceed the
limit, 3,456,000 ticks (48 in-game hours). The minimum is 1 tick a block: the sim and the client place on a click
(`sim.rs` start_use_item from `use_click`, no delay check, as vanilla's consumeClick loop; `Client.clickUse`); 4 ticks
is the delay of a held use key. The pass count itself does not change; the line reports the reachable builds apart.
"""
import sys
rows = [l.rstrip('\n').split('\t') for l in open(sys.argv[1]) if l.strip()]
tpb = int(sys.argv[sys.argv.index('--tpb') + 1]) if '--tpb' in sys.argv else 1
size = {}
for l in open('work/corpus/unique.tsv'):
    f = l.split('\t')
    if len(f) > 4 and f[4].isdigit(): size[f[0]] = int(f[4])
LIMIT = 3456000
out = [r for r in rows if int(r[3]) * tpb > LIMIT]
large = [r for r in rows if size.get(r[0], 0) >= 10000]
out_l = [r for r in out if size.get(r[0], 0) >= 10000]
p = sum(r[1] == 'PASS' for r in rows); pl = sum(r[1] == 'PASS' for r in large)
po = sum(r[1] == 'PASS' for r in out); plo = sum(r[1] == 'PASS' for r in out_l)
print(f"{len(out)} out of reach by time (at {tpb} tick(s) a block: more than {LIMIT // tpb:,} judged cells); "
      f"the goal measure over the reachable builds is {p - po} / {len(rows) - len(out)}, large {pl - plo} / {len(large) - len(out_l)}")
for r in out: print(f"  {r[0][:8]} {int(r[3]):,} cells {r[1]}")
