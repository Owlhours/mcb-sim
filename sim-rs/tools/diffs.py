#!/usr/bin/env python3
"""Lists builds that are not reproduced exactly, smallest first. usage: diffs.py <sim.tsv>..."""
import sys, glob
ref = {}
for f in glob.glob('work/reference/results/f6b21f4-*.tsv'):
    for l in open(f):
        r = l.rstrip('\n').split('\t'); ref[r[0]] = r
rows = []
for path in sys.argv[1:]:
    for l in open(path):
        q = l.rstrip('\n').split('\t')
        r = ref.get(q[0])
        if r and r[1:10] != q[1:10]: rows.append((int(r[3]), r, q))
rows.sort(key=lambda x: x[0])
for _, r, q in rows:
    print(f'{r[0][:12]} {r[20]:>10s} blk={r[3]:>5s} | {r[1]} t={r[4]:>7s} pl={r[5]:>5s} sk={r[6]:>3s} {r[7]}/{r[8]}/{r[9]} sc={r[13]}/{r[14]} tr={r[15]} | {q[1]} t={q[4]:>7s} pl={q[5]:>5s} sk={q[6]:>3s} {q[7]}/{q[8]}/{q[9]} sc={q[13]}/{q[14]} tr={q[15]}')
    if r[18] != q[18] or r[19] != q[19]:
        print(f'      ref: {r[18]} {r[19]}\n      sim: {q[18]} {q[19]}')
