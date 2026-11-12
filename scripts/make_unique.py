#!/usr/bin/env python3
"""corpus/index.tsv (one line per file) -> corpus/unique.tsv (one line per distinct build):
hash, sx, sy, sz, blocks, unknownCells, format, copies, firstPath"""
import collections
rows = collections.OrderedDict(); copies = collections.Counter()
for l in open('/work/corpus/index.tsv'):
    f = l.rstrip('\n').split('\t')
    if f[1] != 'ok': continue
    h = f[2]; copies[h] += 1
    if h not in rows: rows[h] = f
with open('/work/corpus/unique.tsv', 'w') as o:
    for h, f in rows.items():
        o.write('\t'.join([h, f[3], f[4], f[5], f[6], f[7], f[8], str(copies[h]), f[0]]) + '\n')
print(len(rows), 'unique builds')
