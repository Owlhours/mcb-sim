#!/usr/bin/env python3
"""Work done by a simulator run, per set, from counts that do not depend on how busy the machine is.
usage: simcost.py <tag> [<tagB>] [set ...]      (default sets: dev2big small strat-small dev2 dev3)

Reads /work/tmp/sim/<tag>.d/<hash>.log (scripts/simrun.sh) for the builds of each set's list
(/work/tmp/2d9a193-<set>.txt). Per set: builds found, ticks, route-search cells expanded, searches,
cells per tick, and CPU seconds. CPU is printed for reference only: it moves by +-30 % with load
(SMT siblings, other runs). Cells and searches are deterministic for a given tree, seed and switches, so
judge speed by them (and by `perf stat` instructions, see /work/perf/README.md), not by CPU.
With a second tag: the same for B, and the ratio B/A, over the builds present in both runs.
Builds that ran out of the wall limit have no log line and are left out (counted as "missing").
"""
import sys, re, os

SIM = '/work/tmp/sim'
PAT = re.compile(r'ticks=(\d+).*?cpu=(\d+)ms.*?searches=(\d+) expanded=(\d+)')


def load(tag, hashes):
    out = {}
    d = f'{SIM}/{tag}.d'
    for h in hashes:
        p = f'{d}/{h}.log'
        if not os.path.exists(p):
            continue
        with open(p, errors='replace') as f:
            for line in f:
                m = PAT.search(line)
                if m:
                    out[h] = tuple(int(v) for v in m.groups())   # ticks, cpu_ms, searches, expanded
                    break
    return out


def lst(s):
    p = f'/work/tmp/2d9a193-{s}.txt'
    if not os.path.exists(p):
        sys.exit(f'no list {p} (run scripts/simrun.sh once with that set)')
    return [l.split()[0] for l in open(p) if l.strip()]


def tot(rows, keys):
    t = [0, 0, 0, 0]
    for k in keys:
        for i in range(4):
            t[i] += rows[k][i]
    return t


def line(name, n, miss, t):
    ticks, cpu, se, ex = t
    print(f'  {name:14} builds={n:4} missing={miss:3} ticks={ticks/1e6:8.2f}M cells={ex/1e6:9.1f}M '
          f'searches={se/1e6:7.2f}M cells/tick={ex/max(ticks,1):6.1f} cpu={cpu/1000:7.0f}s')


def main():
    args = sys.argv[1:]
    if not args:
        sys.exit(__doc__)
    a = args[0]
    b = args[1] if len(args) > 1 and os.path.isdir(f'{SIM}/{args[1]}.d') else None
    sets = args[2 if b else 1:] or ['dev2big', 'small', 'strat-small', 'dev2', 'dev3']
    ga = [0, 0, 0, 0]; gb = [0, 0, 0, 0]
    for s in sets:
        hs = lst(s)
        ra = load(a, hs)
        print(f'{s}:')
        if not b:
            t = tot(ra, ra)
            line(a, len(ra), len(hs) - len(ra), t)
            ga = [x + y for x, y in zip(ga, t)]
            continue
        rb = load(b, hs)
        both = [h for h in hs if h in ra and h in rb]
        ta, tb = tot(ra, both), tot(rb, both)
        line(a, len(both), len(hs) - len(ra), ta)
        line(b, len(both), len(hs) - len(rb), tb)
        print(f'  {"B/A":14} cells x{tb[3]/max(ta[3],1):.3f}  searches x{tb[2]/max(ta[2],1):.3f}  '
              f'cells/tick x{(tb[3]/max(tb[0],1))/max(ta[3]/max(ta[0],1),1e-9):.3f}  ticks x{tb[0]/max(ta[0],1):.3f}  '
              f'(cpu x{tb[1]/max(ta[1],1):.2f}, noisy)')
        ga = [x + y for x, y in zip(ga, ta)]; gb = [x + y for x, y in zip(gb, tb)]
    print('all:')
    line(a, 0, 0, ga)
    if b:
        line(b, 0, 0, gb)
        print(f'  {"B/A":14} cells x{gb[3]/max(ga[3],1):.3f}  cells/tick x{(gb[3]/max(gb[0],1))/max(ga[3]/max(ga[0],1),1e-9):.3f}  '
              f'ticks x{gb[0]/max(ga[0],1):.3f}  (cpu x{gb[1]/max(ga[1],1):.2f}, noisy)')


main()
