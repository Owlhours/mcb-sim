#!/usr/bin/env python3
"""What a new revision changes: compares two result files build by build (simulator output of two revisions, or
simulator output against Java reference rows of another commit).
usage: revdiff.py <old.tsv> <new.tsv> [--list]
Prints how many builds are unchanged (verdict .. leftover), verdict flips, tick change, and - where the new file
has the metrics column - in how many builds each mechanism was at work."""
import sys, statistics as st
def load(path):
    out = {}
    for l in open(path):
        f = l.rstrip('\n').split('\t')
        if len(f) >= 17: out[f[0]] = f
    return out
def metrics(f):
    for c in f[22:]:
        if c.startswith('died='): return dict(kv.split('=', 1) for kv in c.split(';'))
    return {}
def main():
    a, b = load(sys.argv[1]), load(sys.argv[2])
    common = [h for h in b if h in a]
    same = [h for h in common if a[h][1:10] == b[h][1:10]]
    flips = [(h, a[h][1], b[h][1]) for h in common if a[h][1] != b[h][1]]
    up = sum(1 for _, x, y in flips if y == 'PASS'); down = len(flips) - up
    rel = [(int(b[h][4]) - int(a[h][4])) / max(1, int(a[h][4])) for h in common]
    ta, tb = sum(int(a[h][4]) for h in common), sum(int(b[h][4]) for h in common)
    print(f'{len(common)} builds: {len(same)} unchanged, {len(common) - len(same)} changed; verdict FAIL->PASS {up}, PASS->FAIL {down}; '
          f'PASS {sum(1 for h in common if a[h][1] == "PASS")} -> {sum(1 for h in common if b[h][1] == "PASS")}; ticks {100 * (tb - ta) / ta:+.1f}% in sum, median change {100 * st.median(rel):+.2f}%')
    left = lambda r: int(r[9])
    print(f'leftover cells {sum(left(a[h]) for h in common)} -> {sum(left(b[h]) for h in common)}; missing {sum(int(a[h][7]) for h in common)} -> {sum(int(b[h][7]) for h in common)}; statuses: '
          + ', '.join(f'{s}={sum(1 for h in common if b[h][2] == s)}' for s in sorted({b[h][2] for h in common})))
    keys = {}
    for h in common:
        for k, v in metrics(b[h]).items():
            on = v not in ('0', '0/0/0') and k not in ('mainTicks', 'exitSamples', 'putOff', 'scaffoldAtMain')
            if k == 'exitSamples': on = v.split('/')[2] != '0'; k = 'exitSamples: no way'
            if on: keys.setdefault(k, []).append(h)
    if keys:
        print('builds in which each was at work: ' + ', '.join(f'{k} {len(v)}' for k, v in sorted(keys.items(), key=lambda kv: -len(kv[1]))))
    if '--list' in sys.argv:
        for h in common:
            if a[h][1:10] == b[h][1:10]: continue
            m = metrics(b[h])
            act = ' '.join(f'{k}={v}' for k, v in m.items() if v not in ('0', '0/0/0') and k in ('died', 'hurt', 'retreats', 'shutEpisodes', 'alreadyShut', 'exitUnknown', 'relaxedRoutes', 'unstuck', 'trapsAvoided', 'fromInside', 'chained', 'fromClose', 'pits', 'forcedWorse', 'tidied'))
            print(f'{h[:12]} blk={a[h][3]:>6s} | {a[h][1]} {a[h][2]:11s} {a[h][4]:>8s} {a[h][7]}/{a[h][8]}/{a[h][9]} | {b[h][1]} {b[h][2]:11s} {b[h][4]:>8s} {b[h][7]}/{b[h][8]}/{b[h][9]} | {act}')
main()
