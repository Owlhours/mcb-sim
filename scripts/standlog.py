#!/usr/bin/env python3
"""Groups an MCSIM_STANDLOG file by stand: per trip of go_to the ticks, distance, height change, moves; per stand the
cells placed. Prints a summary."""
import sys, collections

def load(path):
    G, P = [], []
    for l in open(path):
        f = l.split()
        if not f: continue
        if f[0] == 'G':
            v = list(map(int, f[1:]))
            g = dict(t0=v[0], t1=v[1], ok=v[2], fr=tuple(v[3:6]), at=tuple(v[6:9]), st=tuple(v[9:12]), cell=tuple(v[12:15]), n=v[15],
                     mv=v[16:29], miny=v[29], maxy=v[30], column=v[31], cap=v[32], trips=v[33])
            G.append(g)
        elif f[0] == 'P':
            P.append((int(f[1]), f[2], tuple(map(int, f[3:6])), tuple(map(int, f[6:9]))))
    return G, P

MV = 'W J D P B dig clear mine climb descend tower gate ladder'.split()

def summary(path, tmax=None):
    G, P = load(path)
    if tmax: G = [g for g in G if g['t1'] <= tmax]; P = [p for p in P if p[0] <= tmax]
    end = P[-1][0] if P else 0
    # cells placed after each arrival up to the next departure
    pi = 0
    stands = []
    for k, g in enumerate(G):
        nxt = G[k + 1]['t0'] if k + 1 < len(G) else end + 1
        while pi < len(P) and P[pi][0] < g['t1']: pi += 1
        c = 0; cs = []
        j = pi
        while j < len(P) and P[j][0] <= nxt: c += 1; cs.append(P[j]); j += 1
        stands.append((g, c, cs, nxt - g['t1']))
    trip_ticks = sum(g['t1'] - g['t0'] for g in G)
    print(f"== {path.split('/')[-1]}: main pass to t={end}, {len(P)} cells placed, {len(G)} trips, trip ticks {trip_ticks} ({100*trip_ticks/end:.1f} %), at-stand ticks {sum(s[3] for s in stands)}")
    fails = [g for g in G if not g['ok']]
    print(f"  failed trips {len(fails)}, their ticks {sum(g['t1']-g['t0'] for g in fails)}")
    # trips with a restock in them
    rs = [g for g in G if g['trips'] > 0]
    print(f"  trips with a restock in them {len(rs)}, ticks {sum(g['t1']-g['t0'] for g in rs)}")
    ok = [s for s in stands if s[0]['ok'] and s[0]['trips'] == 0]
    # by horizontal distance
    def hd(g): return max(abs(g['fr'][0] - g['at'][0]), abs(g['fr'][2] - g['at'][2]))
    def md(g): return abs(g['fr'][0] - g['at'][0]) + abs(g['fr'][2] - g['at'][2])
    buckets = collections.OrderedDict((k, [0, 0, 0]) for k in ['0', '1', '2-3', '4-7', '8-15', '16-31', '32+'])
    def bk(d): return '0' if d == 0 else '1' if d == 1 else '2-3' if d < 4 else '4-7' if d < 8 else '8-15' if d < 16 else '16-31' if d < 32 else '32+'
    for s in ok:
        b = buckets[bk(hd(s[0]))]; b[0] += 1; b[1] += s[0]['t1'] - s[0]['t0']; b[2] += s[1]
    tt = sum(b[1] for b in buckets.values())
    print("  by horizontal (Chebyshev) distance between stands: trips | ticks (share) | ticks/trip | cells placed from the stand reached / trip")
    for k, b in buckets.items():
        if b[0]: print(f"    {k:>6}: {b[0]:7d} | {b[1]:9d} ({100*b[1]/tt:5.1f} %) | {b[1]/b[0]:6.1f} | {b[2]/b[0]:5.2f}")
    # height: dy and down-up
    hb = collections.OrderedDict()
    for s in ok:
        g = s[0]
        dy = g['at'][1] - g['fr'][1]
        dip = min(g['fr'][1], g['at'][1]) - g['miny']
        k = ('up' if dy > 1 else 'down' if dy < -1 else 'level') + (' via a dip of 2+' if dip >= 2 else '')
        b = hb.setdefault(k, [0, 0, 0, 0]); b[0] += 1; b[1] += g['t1'] - g['t0']; b[2] += s[1]; b[3] += abs(dy)
    print("  by height change: trips | ticks (share) | ticks/trip | cells/stand | mean |dy|")
    for k, b in sorted(hb.items(), key=lambda kv: -kv[1][1]):
        print(f"    {k:>22}: {b[0]:7d} | {b[1]:9d} ({100*b[1]/tt:5.1f} %) | {b[1]/b[0]:6.1f} | {b[2]/b[0]:5.2f} | {b[3]/b[0]:5.1f}")
    # moves summed
    ms = [0] * 13
    for s in ok:
        for i, v in enumerate(s[0]['mv']): ms[i] += v
    print("  moves on those routes: " + ', '.join(f"{MV[i]} {ms[i]}" for i in range(13) if ms[i]))
    # cells per stand
    cb = collections.Counter()
    tk = collections.Counter()
    for s in ok:
        c = s[1]; k = '0' if c == 0 else '1' if c == 1 else '2-3' if c < 4 else '4-7' if c < 8 else '8-15' if c < 16 else '16+'
        cb[k] += 1; tk[k] += s[0]['t1'] - s[0]['t0']
    print("  cells placed per stand reached: " + ', '.join(f"{k}: {cb[k]} stands / {tk[k]} trip ticks" for k in ['0', '1', '2-3', '4-7', '8-15', '16+']))
    return G, P, stands

if __name__ == '__main__':
    for p in sys.argv[1:]: summary(p)
