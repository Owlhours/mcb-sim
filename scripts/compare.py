#!/usr/bin/env python3
"""compare.py A.tsv B.tsv : builds whose verdict changed between two result files, and the totals over the builds both have."""
import csv, sys
def load(p):
    try: return {r[0]: r for r in csv.reader(open(p), delimiter='\t') if len(r) > 10 and r[1] in ('PASS', 'FAIL')}
    except FileNotFoundError: return {}
a, b = load(sys.argv[1]), load(sys.argv[2])
common = [h for h in b if h in a]
wide = lambda r: len(r) > 21 and r[21] == 'widePASS'
print(f"{len(common)} in both | strict {sum(a[h][1] == 'PASS' for h in common)} -> {sum(b[h][1] == 'PASS' for h in common)}"
      f" | wide {sum(wide(a[h]) for h in common)} -> {sum(wide(b[h]) for h in common)}"
      f" | ticks {sum(int(a[h][4]) for h in common)} -> {sum(int(b[h][4]) for h in common)}"
      f" | leftover {sum(int(a[h][9]) for h in common)} -> {sum(int(b[h][9]) for h in common)}")
for h in common:
    if a[h][1] != b[h][1] or wide(a[h]) != wide(b[h]):
        print('  ', h[:8], a[h][1], '->', b[h][1], 'wide' if wide(b[h]) else '', 'blocks', b[h][3], '|', b[h][18][:160])
