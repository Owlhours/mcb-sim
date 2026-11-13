#!/usr/bin/env python3
"""How two placement logs differ. usage: evclass.py <java ev file> <sim ev file>
Prints one word: identical | order (same events, another order within a tick: blocks that change in the same tick
are reported in the order of the server's chunk-section packets) | random (as order, apart from the age of vines and
the like, which the game draws from its clock-seeded random source when they are placed) | different <first tick>."""
import re, sys
from collections import defaultdict
def grp(path):
    d = defaultdict(list)
    for l in open(path):
        l = l.rstrip('\n')
        if l: d[int(l.split()[0])].append(l)
    return d
def norm(l): return re.sub(r'\b(age|stage)=\d+', r'\1=*', l)
a, b = grp(sys.argv[1]), grp(sys.argv[2])
if open(sys.argv[1]).read() == open(sys.argv[2]).read(): print('identical'); sys.exit()
ticks = sorted(set(a) | set(b))
if all(sorted(a.get(t, [])) == sorted(b.get(t, [])) for t in ticks): print('order'); sys.exit()
bad = [t for t in ticks if sorted(map(norm, a.get(t, []))) != sorted(map(norm, b.get(t, [])))]
print('random' if not bad else f'different {bad[0]}')
