#!/usr/bin/env python3
"""Writes /work/tmp/<set>.txt for every reference results file: one build hash per line. A build that the Java run
gave up on at its wall-clock limit (status "wall limit") gets the tick at which that happened as a second column;
the simulator then stops at the same tick (`--stop-at`), because how long the Java version takes in wall time is
not something it can predict."""
import glob, os
REF = '/work/reference/results'
for p in sorted(glob.glob(f'{REF}/f6b21f4-*.tsv')):
    name = os.path.basename(p)[8:-4]
    out = []
    for l in open(p):
        f = l.rstrip('\n').split('\t')
        if len(f) < 17 or len(f[0]) != 20: continue
        out.append(f[0] + (f' {f[4]}' if f[2] == 'wall limit' else ''))
    open(f'/work/tmp/{name}.txt', 'w').write('\n'.join(out) + '\n')
    print(name, len(out))
