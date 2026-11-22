#!/usr/bin/env python3
"""Lists the cells of a build holding blocks whose name contains a pattern. usage: cells.py <hashprefix> <pattern>"""
import sys, zlib, struct
rows = [l.rstrip('\n').split('\t') for l in open('work/reference/mc/states.tsv')][1:]
uniq = [l.split('\t')[0] for l in open('work/corpus/unique.tsv')]
h = next(u for u in uniq if u.startswith(sys.argv[1]))
d = zlib.decompress(open(f'work/corpus/cache/{h[:2]}/{h}.mcb', 'rb').read())
sx, sy, sz = struct.unpack('>iii', d[4:16]); n = struct.unpack('>H', d[28:30])[0]; p = 30 + n
i = 0; tot = sx * sy * sz
while i < tot:
    run = 0; sh = 0
    while True:
        b = d[p]; p += 1; run |= (b & 127) << sh; sh += 7
        if b < 128: break
    sid = (d[p] << 8) | d[p + 1]; p += 2
    if sid and sys.argv[2] in rows[sid][1]:
        for k in range(i, i + run):
            x = k % sx; z = (k // sx) % sz; y = k // (sx * sz)
            print(f'{x},{y},{z} {rows[sid][1]}[{rows[sid][2]}]')
    i += run
