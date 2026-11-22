#!/usr/bin/env python3
"""Draws the redstone-heavy lists (samples/redstone-dev1.txt, samples/redstone-ho1.txt; see samples/redstone-dev1.md).

usage: draw_redstone.py order <dir>             shuffled pools, <dir>/order-<k>.txt ("hash blocks"), one per stratum
       draw_redstone.py pick <dir> <counts.txt>  the lists from the counted builds (lines `COUNTS <hash> SXxSYxSZ kind=n ...`)

Pool: corpus/unique.tsv, height <= 380, no hash named in work/samples/**/*.txt or work/tmp/2d9a193-*.txt.
Strata by blocks; each stratum is shuffled with SEED + k. The builds are counted (MCSIM_COUNT='*') in that order,
and a build is redstone-heavy when its pistons, sticky pistons, observers, TNT, droppers, dispensers, hoppers,
repeaters and comparators come to >= 20 cells, or to >= 2 % of its blocks and >= 4 cells. In each stratum the first
QUOTA[k] heavy builds in the shuffled order go to the dev list, the next QUOTA[k] to the decision set.
"""
import random, glob, sys, os
SEED = 2026112001
STRATA = [(50, 300), (300, 1000), (1000, 3000), (3000, 10000), (10000, 30000), (30000, 100000), (100000, 250000)]
QUOTA = [15, 20, 25, 25, 25, 25, 15]
KINDS = {'piston', 'sticky_piston', 'observer', 'tnt', 'dropper', 'dispenser', 'hopper', 'repeater', 'comparator'}
MIN_CELLS, MIN_FRAC, MIN_FLOOR = 20, 0.02, 4

def pools():
    skip = set()
    for p in glob.glob('work/samples/**/*.txt', recursive=True) + glob.glob('work/tmp/2d9a193-*.txt'):
        for l in open(p):
            if l.strip() and not l.startswith('#'): skip.add(l.split()[0])
    out = [[] for _ in STRATA]
    for l in open('work/corpus/unique.tsv'):
        f = l.rstrip('\n').split('\t')
        if len(f) < 5 or not f[4].isdigit(): continue
        h, sy, blocks = f[0], int(f[2]), int(f[4])
        if h in skip or sy > 380: continue
        for k, (lo, hi) in enumerate(STRATA):
            if lo <= blocks < hi: out[k].append((h, blocks))
    for k, pool in enumerate(out):
        pool.sort()
        random.Random(SEED + k).shuffle(pool)
    return out, len(skip)

def heavy(line):
    f = line.split()
    n = 0; total = 0
    for kv in f[3:]:
        k, _, v = kv.partition('=')
        if k == '_blocks': total = int(v)
        elif k in KINDS: n += int(v)
    return n, total, n >= MIN_CELLS or (n >= MIN_FLOOR and n >= MIN_FRAC * max(total, 1))

if sys.argv[1] == 'order':
    d = sys.argv[2]; ps, ns = pools()
    print(ns, 'hashes excluded')
    for k, pool in enumerate(ps):
        with open(os.path.join(d, f'order-{k}.txt'), 'w') as o:
            for h, b in pool: o.write(f'{h} {b}\n')
        print(k, STRATA[k], len(pool), 'in the pool')
else:
    d, cf = sys.argv[2], sys.argv[3]
    counts = {}
    for l in open(cf):
        if l.startswith('COUNTS '): counts[l.split()[1]] = l
    ps, _ = pools()
    dev, ho = [], []
    for k, pool in enumerate(ps):
        got = []; looked = 0; last = None
        for h, b in pool:
            if len(got) == 2 * QUOTA[k]: break
            if h not in counts: last = h; break
            looked += 1
            n, tot, ok = heavy(counts[h])
            if ok: got.append(h)
        if len(got) < 2 * QUOTA[k]: sys.exit(f'stratum {k}: only {len(got)} heavy builds in the first {looked} counted (next uncounted {last})')
        dev += got[:QUOTA[k]]; ho += got[QUOTA[k]:]
        print(k, STRATA[k], f'{looked} counted in order, {len(got)} taken')
    open('samples/redstone-dev1.txt', 'w').write(''.join(h + '\n' for h in dev))
    open('samples/redstone-ho1.txt', 'w').write(''.join(h + '\n' for h in ho))
    print(len(dev), len(ho), len(set(dev) & set(ho)))
