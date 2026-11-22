#!/usr/bin/env python3
"""Compares two simulator runs (scripts/simrun.sh tags) build by build, over any number of sets and seeds.
usage: simcmp.py <tagA> <tagB> [set ...] [--seeds 0,1,2] [--list]
Rows are work/tmp/sim/<tag>[-s<k>]-<set>.tsv (seed 0 has no suffix). Prints, per seed and in sum: passes, verdict
changes, bad cells (missing + wrong + leftover), builds whose course changed (ticks differ), collapses (bad cells
tenfold and 50+) and recoveries, and the exact sign-flip test on log10(1 + bad) as docs/EVALUATION.md has it."""
import sys, os, math, random
D = 'work/tmp/sim'
def load(tag, seed, s):
    p = f'{D}/{tag}{"" if seed == 0 else f"-s{seed}"}-{s}.tsv'
    out = {}
    if not os.path.exists(p): return None
    for l in open(p):
        f = l.rstrip('\n').split('\t')
        if len(f) >= 17: out[f[0]] = f
    return out
def bad(r): return int(r[7]) + int(r[8]) + int(r[9])
def signflip(d, n=20000):
    d = [x for x in d if x != 0]
    if not d: return 1.0
    obs = abs(sum(d)); rnd = random.Random(1); hit = 0
    for _ in range(n):
        if abs(sum(x if rnd.random() < 0.5 else -x for x in d)) >= obs - 1e-12: hit += 1
    return hit / n
def main():
    a = [x for x in sys.argv[1:] if not x.startswith('--')]
    seeds = [0]
    if '--seeds' in sys.argv:
        v = sys.argv[sys.argv.index('--seeds') + 1]; seeds = [int(x) for x in v.split(',')]; a = [x for x in a if x != v]
    lst = '--list' in sys.argv
    ta, tb = a[0], a[1]; sets = a[2:] or ['dev2big', 'small', 'strat-small', 'dev2', 'dev3']
    tot = dict(n=0, pa=0, pb=0, fp=0, pf=0, ch=0, less=0, more=0, col=0, rec=0, ba=0, bb=0, ta=0, tb=0); logs = []
    for s in sets:
        for k in seeds:
            A, B = load(ta, k, s), load(tb, k, s)
            if A is None or B is None: print(f'{s} seed {k}: no rows'); continue
            c = dict(n=0, pa=0, pb=0, fp=0, pf=0, ch=0, less=0, more=0, col=0, rec=0, ba=0, bb=0, ta=0, tb=0); lines = []
            for h in A:
                if h not in B: continue
                r, q = A[h], B[h]; x, y = bad(r), bad(q)
                c['n'] += 1; c['pa'] += r[1] == 'PASS'; c['pb'] += q[1] == 'PASS'
                c['fp'] += r[1] != 'PASS' and q[1] == 'PASS'; c['pf'] += r[1] == 'PASS' and q[1] != 'PASS'
                c['ba'] += x; c['bb'] += y; c['ta'] += int(r[4]); c['tb'] += int(q[4])
                if r[1:10] != q[1:10]:
                    c['ch'] += 1; c['less'] += y < x; c['more'] += y > x
                    c['col'] += y >= 50 and y >= 10 * max(1, x); c['rec'] += x >= 50 and x >= 10 * max(1, y)
                    lines.append(f'   {h[:8]} blk={r[3]:>6} | {r[1]} {r[2]:<10} t={r[4]:>8} m/w/l={r[7]}/{r[8]}/{r[9]} failed={r[11]} | {q[1]} {q[2]:<10} t={q[4]:>8} m/w/l={q[7]}/{q[8]}/{q[9]} failed={q[11]}')
                logs.append(math.log10(1 + y) - math.log10(1 + x))
            print(f'{s} seed {k}: n={c["n"]} pass {c["pa"]} -> {c["pb"]} (F->P {c["fp"]}, P->F {c["pf"]}); course changed {c["ch"]} (bad fewer {c["less"]}, more {c["more"]}); bad cells {c["ba"]} -> {c["bb"]}; collapses {c["col"]}, recoveries {c["rec"]}; ticks {100.0 * (c["tb"] - c["ta"]) / max(1, c["ta"]):+.1f}%')
            if lst: print('\n'.join(lines))
            for key in tot: tot[key] += c[key]
    c = tot
    print(f'ALL: n={c["n"]} pass {c["pa"]} -> {c["pb"]} (F->P {c["fp"]}, P->F {c["pf"]}); course changed {c["ch"]} (bad fewer {c["less"]}, more {c["more"]}); bad cells {c["ba"]} -> {c["bb"]}; collapses {c["col"]}, recoveries {c["rec"]}; ticks {100.0 * (c["tb"] - c["ta"]) / max(1, c["ta"]):+.1f}%; sign-flip p on log10(1+bad) = {signflip(logs):.3f}')
main()
