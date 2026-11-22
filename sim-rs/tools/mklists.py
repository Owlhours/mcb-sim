#!/usr/bin/env python3
"""Writes work/tmp/<set>.txt for every reference results file: one build hash per line. A build that the Java run
gave up on at its wall-clock limit (status "wall limit") gets the tick at which that happened as a second column;
the simulator then stops at the same tick (`--stop-at`), because how long the Java version takes in wall time is
not something it can predict."""
import glob, os, sys
REF = 'work/reference/results'
# usage: mklists.py [commit]   (default f6b21f4 -> work/tmp/<set>.txt; another commit -> work/tmp/<commit>-<set>.txt)
rev = sys.argv[1] if len(sys.argv) > 1 else 'f6b21f4'
for p in sorted(glob.glob(f'{REF}/{rev}-*.tsv')):
    name = os.path.basename(p)[len(rev) + 1:-4]
    out = []
    for l in open(p):
        f = l.rstrip('\n').split('\t')
        if len(f) < 17 or len(f[0]) != 20: continue
        out.append(f[0] + (f' {f[4]}' if f[2] == 'wall limit' else ''))
    path = f'work/tmp/{name}.txt' if rev == 'f6b21f4' else f'work/tmp/{rev}-{name}.txt'
    open(path, 'w').write('\n'.join(out) + '\n')
    print(path, len(out))
