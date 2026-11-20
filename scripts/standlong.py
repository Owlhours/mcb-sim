#!/usr/bin/env python3
"""Long trips (Chebyshev 8+ between stands): what the pick before them was (K lines: cost, cost without the kind rule)."""
import sys, collections
for p in sys.argv[1:]:
    lastK = None
    agg = collections.defaultdict(lambda: [0, 0])
    for l in open(p):
        f = l.split()
        if not f: continue
        if f[0] == 'K': lastK = (float(f[5]), float(f[6]))
        elif f[0] == 'G':
            v = list(map(int, f[1:]))
            if not v[2] or v[33]: continue
            d = max(abs(v[3] - v[6]), abs(v[5] - v[8]))
            if d < 8: continue
            c, a = lastK if lastK else (0, 0)
            k = 'pick not carried (restock)' if c >= 400 else 'nearer cell not carried' if a < c - 1 else 'unattached' if c >= 60 else 'nothing nearer allowed'
            agg[k][0] += 1; agg[k][1] += v[1] - v[0]
    tot = sum(a[1] for a in agg.values())
    print(f"== {p.split('/')[-1]}: long trips {sum(a[0] for a in agg.values())}, ticks {tot}")
    for k, a in sorted(agg.items(), key=lambda kv: -kv[1][1]): print(f"   {k:>28}: {a[0]:5d} trips, {a[1]:7d} ticks ({100*a[1]/tot:.0f} %)")
