#!/usr/bin/env python3
"""Block census of builds (by hash prefix). usage: blocks.py <hashprefix>..."""
import sys, zlib, struct, collections
names = [l.split('\t', 3)[1] for l in open('work/reference/mc/states.tsv')][1:]
uniq = [l.split('\t')[0] for l in open('work/corpus/unique.tsv')]
for pre in sys.argv[1:]:
    h = next(u for u in uniq if u.startswith(pre))
    d = zlib.decompress(open(f'work/corpus/cache/{h[:2]}/{h}.mcb', 'rb').read())
    sx, sy, sz = struct.unpack('>iii', d[4:16]); n = struct.unpack('>H', d[28:30])[0]; p = 30 + n
    c = collections.Counter(); tot = sx * sy * sz; filled = 0
    while filled < tot:
        run = 0; sh = 0
        while True:
            b = d[p]; p += 1; run |= (b & 127) << sh; sh += 7
            if b < 128: break
        sid = (d[p] << 8) | d[p + 1]; p += 2
        if sid: c[names[sid]] += run
        filled += run
    print(h[:12], f'{sx}x{sy}x{sz}', ' '.join(f'{k}:{v}' for k, v in c.most_common()))
