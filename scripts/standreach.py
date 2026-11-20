#!/usr/bin/env python3
"""From an MCSIM_STANDLOG file: at each departure of the main pass (after sweep_here), the target cells a click from the
bot's eye would place right then (R lines), by the rule that kept the bot from them, against the cells it placed from
that stand. usage: standreach.py <file.sl> ... [--tmax T]"""
import sys, collections

WHY = ['notcarried', 'deferred', 'late', 'above', 'below', 'layer', 'eligible']

def main(paths, tmax):
    for path in paths:
        R, P, G = [], [], []
        for l in open(path):
            f = l.split()
            if not f: continue
            if f[0] == 'R': R.append(list(map(int, f[1:])))
            elif f[0] == 'P': P.append(int(f[1]))
            elif f[0] == 'G': G.append((int(f[1]), int(f[2])))
        if tmax: R = [r for r in R if r[0] <= tmax]; P = [p for p in P if p <= tmax]; G = [g for g in G if g[1] <= tmax]
        # cells placed at the stand left by each departure: those after the previous arrival up to this departure
        arr = [g[1] for g in G]
        pi = 0; ai = 0; last_arr = 0
        tot = [0] * 17; placed = 0
        hist = collections.Counter(); nonzero = 0
        for r in R:
            t = r[0]
            while ai < len(arr) and arr[ai] <= t: last_arr = arr[ai]; ai += 1
            c = 0
            while pi < len(P) and P[pi] <= t:
                if P[pi] >= last_arr: c += 1
                pi += 1
            placed += c
            for k in range(17): tot[k] += r[4 + k] if 4 + k < len(r) else 0
            hist[min(r[4], 10)] += 1
            if r[4]: nonzero += 1
        n = len(R)
        print(f"== {path.split('/')[-1]}: {n} departures, {len(P)} cells placed ({placed / max(n,1):.2f} a stand)")
        print(f"  placeable now at departure: {tot[0]} ({tot[0] / max(n,1):.2f} a departure; {nonzero} departures with any)")
        print("    kept by: " + ', '.join(f"{WHY[k]} {tot[k + 1]} ({100 * tot[k + 1] / max(tot[0], 1):.0f} %)" for k in range(7)))
        print(f"  needed cells in reach with no face from here: {tot[8]} ({tot[8] / max(n,1):.2f} a departure)")
        L = max(tot[6], 1)
        print(f"  layer-kept: the lowest neighbour column 1 / 2 / 3+ lower: {tot[9]} / {tot[10]} / {tot[11]} ({100*tot[9]/L:.0f} / {100*tot[10]/L:.0f} / {100*tot[11]/L:.0f} %)")
        print("    its next cell: " + ', '.join(f"{w} {tot[12 + k]} ({100 * tot[12 + k] / L:.0f} %)" for k, w in enumerate(['blocked', 'notcarried', 'out of reach', 'no face from here', 'placeable here'])))
        print("  placeable-now count per departure: " + ', '.join(f"{k if k < 10 else '10+'}: {hist[k]}" for k in sorted(hist)))

if __name__ == '__main__':
    a = sys.argv[1:]; tmax = None
    if '--tmax' in a: i = a.index('--tmax'); tmax = int(a[i + 1]); del a[i:i + 2]
    main(a, tmax)
