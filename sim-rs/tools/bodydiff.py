#!/usr/bin/env python3
"""First tick at which two per-tick body traces differ. usage: bodydiff.py <java ev file> <sim ev file> [context]
Both files hold event-log lines `<tick> B <x> <y> <z> <vx> <vy> <vz> <onGround> <yaw> <pitch> <sneaking>` (the
simulator writes them with MCSIM_BODY=from,to and --events; the Java side with MCB_BODY=from,to). Numbers are
compared as numbers (the two sides print doubles differently)."""
import sys
def load(p):
    d = {}
    for l in open(p):
        f = l.split()
        if len(f) > 2 and f[1] == 'B': d[int(f[0])] = f[2:]
    return d
def same(a, b):
    if len(a) != len(b): return False
    for x, y in zip(a, b):
        try:
            if float(x) != float(y): return False
        except ValueError:
            if x.lower() != y.lower(): return False
    return True
a, b = load(sys.argv[1]), load(sys.argv[2]); ctx = int(sys.argv[3]) if len(sys.argv) > 3 else 3
ticks = sorted(set(a) & set(b))
if not ticks: sys.exit('no common ticks with B lines')
bad = [t for t in ticks if not same(a[t], b[t])]
if not bad: print(f'identical over ticks {ticks[0]}-{ticks[-1]}'); sys.exit()
t0 = bad[0]
print(f'first difference at tick {t0} (common ticks {ticks[0]}-{ticks[-1]}, {len(bad)} differ)')
for t in ticks:
    if t0 - ctx <= t <= t0 + ctx: print(f'{t} java {" ".join(a[t])}\n{t} sim  {" ".join(b[t])}')
