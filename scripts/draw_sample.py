#!/usr/bin/env python3
"""Draws a stratified random sample of builds from corpus/unique.tsv.

usage: draw_sample.py <seed> <per-stratum> <out.txt> [exclude-list ...]
Strata by number of blocks: 50-300, 300-1k, 1k-3k, 3k-10k, 10k-30k, 30k-100k. Builds taller than the world allows
(380 above ground) and builds named in the exclude lists (earlier samples) are left out.
Output: one "hash blocks" line per build, small strata first.
"""
import random, sys
seed, per, out = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
skip = set()
for p in sys.argv[4:]:
    skip.update(l.split()[0] for l in open(p) if l.strip())
strata = [(50, 300), (300, 1000), (1000, 3000), (3000, 10000), (10000, 30000), (30000, 100000)]
pools = [[] for _ in strata]
for l in open('/work/corpus/unique.tsv'):
    f = l.rstrip('\n').split('\t')
    if len(f) < 5 or not f[4].isdigit(): continue
    h, sy, blocks = f[0], int(f[2]), int(f[4])
    if h in skip or sy > 380: continue
    for k, (lo, hi) in enumerate(strata):
        if lo <= blocks < hi: pools[k].append((h, blocks))
rng = random.Random(seed)
with open(out, 'w') as o:
    for k, pool in enumerate(pools):
        pool.sort()
        for h, b in rng.sample(pool, min(per, len(pool))): o.write(f'{h} {b}\n')
        print(strata[k], len(pool), 'candidates')
