#!/usr/bin/env python3
"""rejudge_cmp.py <stored rows glob> <re-judged rows glob> [<name>]: passes overall / <10k / 10k-100k / 100k+ / large,
whether the bot's course is the same (status, ticks, placed), and every build whose verdict changes."""
import sys, glob, collections
blk = {l.split('\t')[0][:20]: int(l.split('\t')[4]) for l in open('/work/corpus/unique.tsv') if l.split('\t')[4].isdigit()}
def rd(g):
    d = {}
    for f in glob.glob(g):
        for l in open(f):
            p = l.rstrip('\n').split('\t'); d[p[0]] = p
    return d
o, a = rd(sys.argv[1]), rd(sys.argv[2])
band = lambda h: '<10k' if blk[h] < 10000 else '10k-100k' if blk[h] < 100000 else '100k+'
same = sum(1 for h in o if h in a and (o[h][2], o[h][4], o[h][5]) == (a[h][2], a[h][4], a[h][5]))
print(f'{len(o)} stored, {len(a)} re-judged; course identical (status, ticks, placed) in {same}')
for name, d in (('stored', o), (sys.argv[3] if len(sys.argv) > 3 else 're-judged', a)):
    c = collections.Counter(); t = collections.Counter()
    for h, r in d.items(): t[band(h)] += 1; c[band(h)] += r[1] == 'PASS'
    L = c['10k-100k'] + c['100k+']; LT = t['10k-100k'] + t['100k+']
    print(f"  {name:26} all {sum(c.values())}/{sum(t.values())}  <10k {c['<10k']}/{t['<10k']}  10k-100k {c['10k-100k']}/{t['10k-100k']}  100k+ {c['100k+']}/{t['100k+']}  large {L}/{LT} = {100 * L / LT:.1f} %")
fl = [h for h in o if h in a and o[h][1] != a[h][1]]
print(f'  flips: {len(fl)}')
for h in fl: print(f'    {h[:8]} {band(h):9} {o[h][1]} -> {a[h][1]}  missing {o[h][7]} -> {a[h][7]}, wrong {o[h][8]} -> {a[h][8]}, failed sections {o[h][11]} -> {a[h][11]} | {o[h][18][:80]}')
