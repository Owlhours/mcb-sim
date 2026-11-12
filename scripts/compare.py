#!/usr/bin/env python3
"""compare.py A.tsv B.tsv : over the builds both files have - verdict changes, and per size class the sums that a
pass count hides (missing, wrong, leftover cells, failed sections, ticks)."""
import sys, os
sys.path.insert(0, os.path.dirname(__file__))
import results
a, b = results.load(sys.argv[1]), results.load(sys.argv[2])
common = [h for h in b if h in a]
def line(name, hs):
    A = [a[h] for h in hs]; B = [b[h] for h in hs]
    s = lambda rs, f: sum(f(r) for r in rs)
    print(f"{name:8s} {len(hs):4d} | pass {s(A, lambda r: r.passed):3d} -> {s(B, lambda r: r.passed):3d} | wide {s(A, lambda r: r.wide or r.passed):3d} -> {s(B, lambda r: r.wide or r.passed):3d}"
          f" | missing {s(A, lambda r: r.missing):6d} -> {s(B, lambda r: r.missing):6d} | wrong {s(A, lambda r: r.wrong):5d} -> {s(B, lambda r: r.wrong):5d}"
          f" | leftover {s(A, lambda r: r.leftover):6d} -> {s(B, lambda r: r.leftover):6d} | failed sections {s(A, lambda r: r.failed):4d} -> {s(B, lambda r: r.failed):4d}"
          f" | ticks {s(A, lambda r: r.ticks):9d} -> {s(B, lambda r: r.ticks):9d} | ticks changed in {sum(a[h].ticks != b[h].ticks for h in hs)}")
for lo, hi, name in results.SIZES:
    hs = [h for h in common if b[h].size() == name]
    if hs: line(name, hs)
line('all', common)
for h in common:
    if a[h].passed != b[h].passed or a[h].wide != b[h].wide:
        print('  ', h[:8], 'PASS' if a[h].passed else 'FAIL', '->', 'PASS' if b[h].passed else 'FAIL', 'wide' if b[h].wide else '', 'blocks', b[h].blocks,
              f"missing {a[h].missing}->{b[h].missing} leftover {a[h].leftover}->{b[h].leftover} | {b[h].cls()} | {b[h].bad[:110]}")
