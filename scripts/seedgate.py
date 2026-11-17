#!/usr/bin/env python3
"""The seed gate: a candidate against the base over several seeds, measured against the base's own seed noise.
usage: seedgate.py <baseTag> [<candTag>] <set> [set ...] --seeds 0,1,2,3 [--list]

Rows are /work/tmp/sim/<tag>[-s<k>]-<set>.tsv (seed 0 has no suffix), as scripts/simrun.sh and scripts/simseeds.sh
write them (MCB_SEED=k: the bot starts k blocks further along the front; nothing else changes).

Without a candidate: the base's noise on these builds - passes per seed, how far two seeds of the same tree lie
apart, and the builds whose verdict depends on the seed. Measure it once per tree.
With a candidate, per build the paired effect averaged over the seeds, and over the builds:
  passes       sum over seeds of (candidate - base), with a 95 % interval (bootstrap over builds; a build keeps all its
               seeds together, since a build's seeds are not independent)
  bad cells    mean over builds of the seed-averaged difference of log10(1 + bad), same interval
and the reading: a GAIN or a REGRESSION only if the interval excludes 0 AND the pass difference is larger than the
largest difference between two seeds of the base; otherwise "within noise".
--list: the builds with an effect, most affected first.
"""
import sys, os, math, random, itertools
D = '/work/tmp/sim'
def load(tag, seed, sets):
    out = {}
    for s in sets:
        p = f'{D}/{tag}{"" if seed == 0 else f"-s{seed}"}-{s}.tsv'
        if not os.path.exists(p): return None
        for l in open(p):
            f = l.rstrip('\n').split('\t')
            if len(f) >= 17: out[f[0]] = (f[1] == 'PASS', int(f[7]) + int(f[8]) + int(f[9]))
    return out
def boot(vals, n=4000, seed=1):
    """95 % interval of the SUM of vals under resampling of the builds"""
    if not vals: return (0.0, 0.0)
    r = random.Random(seed); m = len(vals); sums = []
    for _ in range(n): sums.append(sum(vals[r.randrange(m)] for _ in range(m)))
    sums.sort(); return (sums[int(0.025 * n)], sums[int(0.975 * n) - 1])
def main():
    a = [x for x in sys.argv[1:] if not x.startswith('--')]
    if '--seeds' not in sys.argv or len(a) < 2: print(__doc__); return
    v = sys.argv[sys.argv.index('--seeds') + 1]; seeds = [int(x) for x in v.split(',')]; a = [x for x in a if x != v]
    known = lambda s: os.path.exists(f'{D}/{a[0]}-{s}.tsv') or any(os.path.exists(f'{D}/{a[0]}-s{k}-{s}.tsv') for k in seeds)
    cand = None if known(a[1]) else a[1]
    sets = a[1:] if cand is None else a[2:]
    B = {k: load(a[0], k, sets) for k in seeds}
    miss = [k for k in seeds if B[k] is None]
    if miss: sys.exit(f'no rows of {a[0]} for seed(s) {miss} on {sets}')
    hs = sorted(set.intersection(*[set(B[k]) for k in seeds]))
    n = len(hs); S = len(seeds)
    pc = [sum(B[k][h][0] for h in hs) for k in seeds]
    mean = sum(pc) / S; sd = math.sqrt(sum((x - mean) ** 2 for x in pc) / max(S - 1, 1))
    pair = [abs(x - y) for x, y in itertools.combinations(pc, 2)] or [0]
    dep = [h for h in hs if 0 < sum(B[k][h][0] for k in seeds) < S]
    flips = [sum(B[x][h][0] != B[y][h][0] for h in hs) for x, y in itertools.combinations(seeds, 2)] or [0]
    print(f'{a[0]} on {" ".join(sets)}: {n} builds, seeds {v}')
    print(f'  base passes per seed: {" ".join(str(x) for x in pc)}   mean {mean:.1f}, sd {sd:.2f}')
    print(f'  two seeds of the base differ by {sum(pair) / len(pair):.1f} passes on average, {max(pair)} at most; '
          f'{sum(flips) / len(flips):.1f} builds change verdict between two seeds on average ({max(flips)} at most)')
    print(f'  builds whose verdict depends on the seed: {len(dep)} of {n}; always pass {sum(all(B[k][h][0] for k in seeds) for h in hs)}, never {sum(not any(B[k][h][0] for k in seeds) for h in hs)}')
    if cand is None: return
    C = {k: load(cand, k, sets) for k in seeds}
    miss = [k for k in seeds if C[k] is None]
    if miss: sys.exit(f'no rows of {cand} for seed(s) {miss}')
    hs = [h for h in hs if all(h in C[k] for k in seeds)]
    dp = [sum(C[k][h][0] - B[k][h][0] for k in seeds) for h in hs]                       # passes, summed over seeds
    db = [sum(math.log10(1 + C[k][h][1]) - math.log10(1 + B[k][h][1]) for k in seeds) / S for h in hs]
    lo, hi = boot(dp); blo, bhi = boot(db)
    per = [sum(C[k][h][0] - B[k][h][0] for h in hs) for k in seeds]
    gained = sum(1 for k in seeds for h in hs if C[k][h][0] and not B[k][h][0]); lost = sum(1 for k in seeds for h in hs if B[k][h][0] and not C[k][h][0])
    changed = sum(1 for h in hs if any(C[k][h] != B[k][h] for k in seeds))
    tot = sum(dp)
    print(f'{cand} against it ({len(hs)} builds x {S} seeds):')
    print(f'  passes {sum(pc)} -> {sum(pc) + tot} over the seeds ({tot:+d}: +{gained} / -{lost}); per seed {" ".join(f"{x:+d}" for x in per)}; '
          f'95 % interval of the sum {lo:+.0f} .. {hi:+.0f}; per seed {tot / S:+.2f}')
    print(f'  bad cells: mean difference of log10(1 + bad) {sum(db) / len(db):+.4f} a build (interval {blo / len(db):+.4f} .. {bhi / len(db):+.4f}); '
          f'builds with any difference {changed} of {len(hs)}')
    outside = (lo > 0 or hi < 0) and abs(tot) / S > max(pair)
    word = 'within noise' if not outside else ('GAIN' if tot > 0 else 'REGRESSION')
    bw = 'bad cells fewer' if bhi < 0 else 'bad cells more' if blo > 0 else 'bad cells: no clear difference'
    print(f'  reading: passes {word} (the base moves by up to {max(pair)} between two seeds); {bw}')
    if '--list' in sys.argv:
        for h, x, y in sorted(zip(hs, dp, db), key=lambda t: (-abs(t[1]), -abs(t[2])))[:40]:
            if x == 0 and abs(y) < 1e-9: break
            print(f'    {h[:8]} passes {x:+d} of {S}   log10(1+bad) {y:+.3f}   base pass in {sum(B[k][h][0] for k in seeds)} of {S} seeds')
main()
