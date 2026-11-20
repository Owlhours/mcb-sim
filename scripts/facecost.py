#!/usr/bin/env python3
"""From an MCSIM_FACELOG file (bot/facediag.rs): the main pass's scaffolding pieces and stand-trip ticks by the reason
the cell they served needed them, and for the cells with no face at all, how often a neighbour that is a target cell
goes in later (the "order for faces" lever). usage: facecost.py <file.fl> ... [--region R] [--blocks]

A piece or a trip is credited to the last stance plan (Q line) of the cell being worked on (A..E); a plan for another
cell inside it (a helper block) keeps the cell's class. Restock ticks are taken out of the trips they fall in and
counted on their own. Plan classes (in this order of test):
  W   a stand that exists, within reach, sees a face: 0 if the trip there puts up no piece, else
  2w  ... the way to it needs new blocks (tower, pillar, bridge pieces on the way)
  2b  a stand in reach sees a face, no route to any of those tried (the plan went on to a column)
  1   no neighbour solid yet                   1s  only the bot's own pieces beside it, and no stand in reach sees one
  bad only stands marked bad see it            2   face in reach, hidden by blocks from every stand in reach
  3   face seen from a stand nearby (Chebyshev 8, 16 down, 6 up), only beyond reach / height
  23  faced only from beyond reach, and hidden 1f  a face, but no stand nearby faces it (or none gives the wanted state)
  4*  other: a breaking plan, a restock trip, no cell taken up, a piece put up at a plain stand
Groups: 1 no face (1, 1s); 2 hidden by blocks (2, 23); 2' a stand in reach sees it, the bot cannot walk there without
new blocks (2w, 2b, bad); 3 reach / height (3, 1f); 4 other; 0 plain walk (trips only).
"""
import sys, bisect, collections

GROUP = {'1': '1 no face', '1s': '1 no face', '2': '2 hidden', '23': '2 hidden', '2b': "2' no walk", '2w': "2' no walk",
         'bad': "2' no walk", '1f': '3 reach/height', '3': '3 reach/height'}
GROUPS = ['1 no face', '2 hidden', "2' no walk", '3 reach/height', '4 other', '0 plain walk']
ORDER = ['1', '1s', '2', '23', '2w', '2b', 'bad', '3', '1f', '4 break plan', '4 restock', '4 no cell', '4 before plan',
         '4 plain stand', '0 plain walk']
NAME = {'1': 'no supporting face', '1s': 'only own pieces beside', '2': 'face hidden (in reach)', '23': 'hidden, beyond reach',
        '1f': 'face, no stand faces it', '2b': 'seen in reach, no route', '2w': 'seen in reach, way needs blocks',
        'bad': 'seen only from bad stands', '3': 'seen, beyond reach/height', '4 break plan': 'breaking plan',
        '4 restock': 'restock trip', '4 no cell': 'no cell', '4 before plan': 'before any plan', '4 plain stand': 'piece at plain stand',
        '0 plain walk': 'plain walk'}

def group(k):
    if k in GROUP: return GROUP[k]
    return '4 other' if k.startswith('4') else '0 plain walk'

def within(iv, t):
    j = bisect.bisect_right(iv, (t, 10**18)) - 1
    return j >= 0 and iv[j][0] <= t <= iv[j][1]

def overlap(iv, a, b):
    s = 0
    j = max(0, bisect.bisect_right(iv, (a, 10**18)) - 1)
    while j < len(iv) and iv[j][0] <= b:
        s += max(0, min(b, iv[j][1]) - max(a, iv[j][0])); j += 1
    return s

def qclass(f):
    c = f[6]
    if c in ('2', '23', '3', '1f') and len(f) > 9 and f[8] == '0': return '1s'
    return c

def analyse(path, region):
    L = [l.split() for l in open(path) if l.strip()]
    R = sorted((int(f[1]), int(f[2])) for f in L if f[0] == 'R')
    G = sorted((int(f[1]), int(f[2])) for f in L if f[0] == 'G')
    Sm = [int(f[1]) for f in L if f[0] == 'S' and f[2] == 'm']
    Sset = set((int(f[1]), tuple(map(int, f[3:6]))) for f in L if f[0] == 'S')
    Pl = collections.defaultdict(list)
    for f in L:
        if f[0] == 'P':
            t, c = int(f[1]), tuple(map(int, f[3:6]))
            if (t, c) not in Sset: Pl[c].append(t)
    track = sorted((int(f[1]), int(f[6]), int(f[7]), int(f[8])) for f in L if f[0] == 'P')
    tk = [x[0] for x in track]
    def stray(c, a, b):
        i, j = bisect.bisect_right(tk, a), bisect.bisect_right(tk, b)
        return max((max(abs(track[k][1] - c[0]), abs(track[k][3] - c[2])) for k in range(i, j)), default=0)
    pieces, ticks, trips = collections.Counter(), collections.Counter(), collections.Counter()
    blocks = collections.defaultdict(collections.Counter)
    climb = collections.Counter()   # 2w trips: pieces, ticks by height climbed
    cell = cls = cname = None
    q1, q1_pieces, nq = [], collections.Counter(), collections.Counter()
    for f in L:
        k = f[0]
        if k == 'A': cell = tuple(map(int, f[2:5])); cls = None
        elif k == 'E': cell = cls = None
        elif k == 'Q':
            qc = qclass(f); nq[qc] += 1
            qcell = tuple(map(int, f[2:5]))
            if cell is not None and qcell != cell and qc != 'K' and cls is not None: continue
            cls = qc; cname = f[10] if len(f) > 10 else '-'
            if cls == '1': q1.append((int(f[1]), qcell, [tuple(map(int, s.split(','))) for s in f[11:]]))
        elif k == 'S' and f[2] == 'm':
            t = int(f[1])
            if within(R, t): c = '4 restock'
            elif cell is None: c = '4 no cell'
            elif cls is None: c = '4 before plan'
            elif cls == 'K': c = '4 break plan'
            elif cls == 'W': c = '2w' if within(G, t) else '4 plain stand'
            else: c = cls
            pieces[c] += 1
            if c not in ('2w',) and not c.startswith('4'): blocks[c][cname] += 1
            if c == '1' and q1: q1_pieces[len(q1) - 1] += 1
        elif k == 'G':
            t0, t1 = int(f[1]), int(f[2])
            d = (t1 - t0) - overlap(R, t0, t1)
            if cell is None: c = '4 no cell'
            elif cls is None: c = '4 before plan'
            elif cls == 'K': c = '4 break plan'
            elif cls == 'W':
                j = bisect.bisect_left(Sm, t0)
                n = bisect.bisect_right(Sm, t1) - j
                c = '2w' if n > 0 else '0 plain walk'
                if n > 0 and len(f) >= 10:
                    up = int(f[8]) - int(f[5]); b = 'up 0-2' if up <= 2 else 'up 3-8' if up <= 8 else 'up 9+'
                    climb[(b, 'pieces')] += n; climb[(b, 'ticks')] += d; climb[(b, 'trips')] += 1
            else: c = cls
            ticks[c] += d; trips[c] += 1
    for a, b in R: ticks['4 restock'] += b - a; trips['4 restock'] += 1
    # order for faces
    o = collections.Counter(); dts = []
    qcells1 = set(q[1] for q in q1)
    for qi, (t, c, nbs) in enumerate(q1):
        o['plans'] += 1; o['pieces'] += q1_pieces[qi]
        if not nbs: o['no target neighbour'] += 1; o['no target neighbour pieces'] += q1_pieces[qi]; continue
        o['target neighbour'] += 1
        if any(nb[3] and nb[4] for nb in nbs): o['neighbour placeable by the rules now'] += 1
        best = None
        for nb in nbs:
            nc = (c[0] + nb[0], c[1] + nb[1], c[2] + nb[2])
            for pt in Pl.get(nc, []):
                if pt > t:
                    if best is None or pt < best[0]: best = (pt, nc)
                    break
        if best is None: continue
        o['later'] += 1; o['later pieces'] += q1_pieces[qi]
        dts.append(best[0] - t)
        s = stray(c, t, best[0])
        if s <= region: o['in region'] += 1; o['in region pieces'] += q1_pieces[qi]
        if s <= 2 * region: o['in 2region'] += 1; o['in 2region pieces'] += q1_pieces[qi]
        if best[1] not in qcells1:
            o['clean'] += 1; o['clean pieces'] += q1_pieces[qi]
            if s <= region: o['clean in region'] += 1; o['clean in region pieces'] += q1_pieces[qi]
    return dict(pieces=pieces, ticks=ticks, trips=trips, nq=nq, blocks=blocks, climb=climb, o=o, dts=sorted(dts))

def show(h, a, region, show_blocks):
    pieces, ticks, trips = a['pieces'], a['ticks'], a['trips']
    tp, tt = sum(pieces.values()), sum(ticks.values())
    print(f"== {h}: main-pass pieces {tp}, stand-trip + restock ticks {tt} ({sum(trips.values())} trips); plans {sum(a['nq'].values())}: " + ' '.join(f"{c}={n}" for c, n in a['nq'].most_common()))
    print(f"   {'reason':34} {'pieces':>8} {'%':>6} {'trip ticks':>11} {'%':>6} {'trips':>7}")
    for c in ORDER:
        if pieces[c] == 0 and ticks[c] == 0: continue
        print(f"   {c + ' ' + NAME[c]:34} {pieces[c]:8d} {100 * pieces[c] / max(tp, 1):6.1f} {ticks[c]:11d} {100 * ticks[c] / max(tt, 1):6.1f} {trips[c]:7d}")
    gp, gt = collections.Counter(), collections.Counter()
    for c in pieces: gp[group(c)] += pieces[c]
    for c in ticks: gt[group(c)] += ticks[c]
    print("   groups (pieces % / ticks %): " + '; '.join(f"{g} {100 * gp[g] / max(tp, 1):.1f} / {100 * gt[g] / max(tt, 1):.1f}" for g in GROUPS))
    cl = a['climb']
    if cl: print("   2w trips by height climbed: " + '; '.join(f"{b}: {cl[(b, 'trips')]} trips, {cl[(b, 'pieces')]} pieces, {cl[(b, 'ticks')]} ticks" for b in ['up 0-2', 'up 3-8', 'up 9+']))
    if show_blocks:
        for c in ['1', '1s', '2', '3', '1f']:
            if a['blocks'][c]: print(f"   blocks {c}: " + ', '.join(f"{n} {v}" for n, v in a['blocks'][c].most_common(5)))
    o, dts = a['o'], a['dts']
    print(f"   order for faces: class-1 plans {o['plans']} ({o['pieces']} pieces); no neighbour that is a target cell {o['no target neighbour']} ({o['no target neighbour pieces']} pieces);"
          f" one still to place {o['target neighbour']} (one of them its column's next cell and eligible right then: {o['neighbour placeable by the rules now']});"
          f" one placed later in the run {o['later']} ({o['later pieces']} pieces): the bot within {region} across until then {o['in region']} ({o['in region pieces']} pieces),"
          f" within {2 * region} {o['in 2region']} ({o['in 2region pieces']} pieces); that neighbour never a no-face plan itself {o['clean']} ({o['clean pieces']} pieces),"
          f" and the bot within {region} too {o['clean in region']} ({o['clean in region pieces']} pieces)")
    if dts:
        q = lambda p: dts[min(len(dts) - 1, int(p * len(dts)))]
        print(f"   ticks until it went in: median {q(0.5)}, quartiles {q(0.25)} / {q(0.75)}, 90 % {q(0.9)};"
              f" within 1,000: {sum(d <= 1000 for d in dts)}, 10,000: {sum(d <= 10000 for d in dts)}, 100,000: {sum(d <= 100000 for d in dts)}")

if __name__ == '__main__':
    a = sys.argv[1:]
    reg = 8
    if '--region' in a: i = a.index('--region'); reg = int(a[i + 1]); del a[i:i + 2]
    sb = '--blocks' in a
    a = [x for x in a if x != '--blocks']
    for p in a: show(p.split('/')[-1].split('.')[0], analyse(p, reg), reg, sb)
