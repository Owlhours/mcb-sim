#!/usr/bin/env python3
"""C lines: at each departure, the best nearby spot's rule-allowed cells (and with kind / layer rule left out),
against the cells the bot then placed at the stand it went to."""
import sys, collections
for p in sys.argv[1:]:
    C, G, P = [], [], []
    for l in open(p):
        f = l.split()
        if not f: continue
        if f[0] == 'C': C.append((int(f[1]), int(f[2]), int(f[3]), int(f[4]), int(f[5]), float(f[6]), int(f[7])))
        elif f[0] == 'G': G.append((int(f[1]), int(f[2])))
        elif f[0] == 'P': P.append(int(f[1]))
    # cells placed between arrival k and departure k+1
    import bisect
    placed_next = []
    for k, g in enumerate(G):
        nxt = G[k + 1][0] if k + 1 < len(G) else 10**12
        placed_next.append(bisect.bisect_right(P, nxt) - bisect.bisect_left(P, g[1]))
    n = len(C)
    s = [sum(c[i] for c in C) / n for i in range(1, 7)]
    pn = sum(placed_next) / len(placed_next)
    h0 = collections.Counter(min(c[2], 10) for c in C)
    print(f"== {p.split('/')[-1]}: {n} departures; spots {s[0]:.1f}; best spot rule-allowed {s[1]:.2f}, without kind rule {s[2]:.2f}, without layer rule {s[3]:.2f}; mean over spots {s[4]:.2f}; best at distance {s[5]:.1f}; placed at the next stand {pn:.2f}")
    print("   best rule-allowed: " + ' '.join(f"{k}:{100*v/n:.0f}%" for k, v in sorted(h0.items())))
