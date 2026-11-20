#!/usr/bin/env python3
"""Trip causes: for each go_to trip of the main pass, the pick that led to it (K line before it), its cost and the cost
of the nearest pick regardless of material; plus restock trips from stderr TRIP lines."""
import sys, bisect, collections, re
sys.path.insert(0, __import__('os').path.dirname(__file__))
from standlog import load

def picks(path):
    K = []
    for l in open(path):
        if l.startswith('K '):
            f = l.split()
            K.append((int(f[1]), (int(f[2]), int(f[3]), int(f[4])), float(f[5]), float(f[6]) if len(f) > 9 else -1, (int(f[-3]), int(f[-2]), int(f[-1]))))
    return K

def cheb(a, b): return max(abs(a[0] - b[0]), abs(a[2] - b[2]))

for path in (sys.argv[1:] if __name__ == '__main__' else []):
    G, P = load(path)
    K = picks(path)
    kt = [k[0] for k in K]
    end = P[-1][0]
    trips = [g for g in G if g['ok']]
    tot = sum(g['t1'] - g['t0'] for g in G)
    cls = collections.OrderedDict()
    def add(k, g):
        e = cls.setdefault(k, [0, 0]); e[0] += 1; e[1] += g['t1'] - g['t0']
    for g in G:
        j = bisect.bisect_right(kt, g['t0']) - 1
        if j < 0: add('no pick', g); continue
        k = K[j]
        cost, anyc = k[2], k[3]
        d = cheb(g['fr'], g['at'])
        far = 'far(16+)' if d >= 16 else 'mid(4-15)' if d >= 4 else 'near(0-3)'
        why = 'not carried' if cost >= 400 else 'unattached' if cost >= 60 else 'plain'
        mat = ' material-skipped' if anyc >= 0 and cost - anyc > 1 and cost < 400 else ''
        if g['trips'] > 0: why += ' +restock'
        add(f"{far:9} {why}{mat}", g)
    print(f"== {path.split('/')[-1]}: main pass {end} ticks, go_to ticks {tot}")
    for k, v in sorted(cls.items(), key=lambda kv: -kv[1][1]):
        print(f"   {k:45} {v[0]:7d} trips {v[1]:9d} ticks ({100*v[1]/tot:5.1f} % of go_to)")
