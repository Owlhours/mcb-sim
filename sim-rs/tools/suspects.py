#!/usr/bin/env python3
"""Which blocks occur disproportionately in builds the simulator does not reproduce exactly?
usage: suspects.py <sim.tsv> [<sim2.tsv> ...]"""
import sys, zlib, struct, glob, collections
names = []
for l in open('work/reference/mc/states.tsv'):
    f = l.split('\t', 3)
    if f[0] != 'id': names.append(f[1])
def blocks_of(h):
    d = zlib.decompress(open(f'work/corpus/cache/{h[:2]}/{h}.mcb', 'rb').read())
    sx, sy, sz = struct.unpack('>iii', d[4:16]); n = struct.unpack('>H', d[28:30])[0]; p = 30 + n
    out = set(); tot = sx * sy * sz; filled = 0
    while filled < tot:
        run = 0; sh = 0
        while True:
            b = d[p]; p += 1; run |= (b & 127) << sh; sh += 7
            if b < 128: break
        sid = (d[p] << 8) | d[p + 1]; p += 2
        if sid: out.add(names[sid])
        filled += run
    return out
ref = {}
for f in glob.glob('work/reference/results/f6b21f4-*.tsv'):
    for l in open(f):
        r = l.rstrip('\n').split('\t'); ref[r[0]] = r
tot = collections.Counter(); bad = collections.Counter(); nbad = 0; n = 0
for path in sys.argv[1:]:
    for l in open(path):
        q = l.rstrip('\n').split('\t')
        if q[0] not in ref: continue
        r = ref[q[0]]; n += 1
        ex = r[1:10] == q[1:10]
        nbad += not ex
        for b in blocks_of(q[0]):
            tot[b] += 1
            if not ex: bad[b] += 1
print(f'{n} builds, {nbad} not exact ({100*nbad/n:.0f}%)')
rows = [(bad[b] / tot[b], bad[b], tot[b], b) for b in tot if tot[b] >= 3]
rows.sort(reverse=True)
for fr, b_, t_, b in rows[:45]: print(f'{100*fr:4.0f}%  {b_:3d}/{t_:<3d} {b}')
