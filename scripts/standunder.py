#!/usr/bin/env python3
"""Share of main-pass stands by what the feet rest on (G line field 35), weighted by stands, trip ticks, cells placed."""
import sys, collections
NM = ['climbing column', 'other own piece', 'build cell', 'ground', 'other']
for p in sys.argv[1:]:
    G, P = [], []
    for l in open(p):
        f = l.split()
        if not f: continue
        if f[0] == 'G': G.append(list(map(int, f[1:])))
        elif f[0] == 'P': P.append((int(f[1]), f[2]))
    end = P[-1][0] if P else 0
    pi = 0
    agg = collections.defaultdict(lambda: [0, 0, 0, 0])
    hy = collections.defaultdict(lambda: [0, 0])
    for k, g in enumerate(G):
        t0, t1, ok = g[0], g[1], g[2]
        nxt = G[k + 1][0] if k + 1 < len(G) else end + 1
        while pi < len(P) and P[pi][0] < t1: pi += 1
        c = 0; j = pi
        while j < len(P) and P[j][0] <= nxt: c += 1; j += 1
        u = g[34] if len(g) > 34 else 4
        a = agg[u]; a[0] += 1; a[1] += t1 - t0; a[2] += c; a[3] += nxt - t1
        hy[(u, min(g[7] // 10, 9))][0] += 1; hy[(u, min(g[7] // 10, 9))][1] += c
    S = [sum(a[i] for a in agg.values()) for i in range(4)]
    print(f"== {p.split('/')[-1]}: {S[0]} stands, trip ticks {S[1]}, cells {S[2]}")
    for u in sorted(agg):
        a = agg[u]
        print(f"  {NM[u]:>16}: stands {a[0]:6d} ({100*a[0]/S[0]:5.1f} %)  trip ticks {100*a[1]/S[1]:5.1f} %  cells {100*a[2]/S[2]:5.1f} %  cells/stand {a[2]/a[0]:5.2f}  ticks/trip {a[1]/a[0]:6.1f}")
