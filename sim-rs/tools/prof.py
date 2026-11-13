#!/usr/bin/env python3
"""Per-function table from an MCSIM_PROF sample file. usage: prof.py <samples> [binary] [top]"""
import sys, subprocess, bisect, collections, re
binary = sys.argv[2] if len(sys.argv) > 2 else '/work/sim/target/release/mcsim'
top = int(sys.argv[3]) if len(sys.argv) > 3 else 40
syms = []
for l in subprocess.run(['nm', '-C', '--defined-only', binary], capture_output=True, text=True).stdout.splitlines():
    p = l.split(' ', 2)
    if len(p) == 3 and p[1] in 'tTwW': syms.append((int(p[0], 16), p[2]))
syms.sort()
addrs = [a for a, _ in syms]
cnt = collections.Counter(); n = 0
for l in open(sys.argv[1]):
    a = int(l, 16); n += 1
    k = bisect.bisect_right(addrs, a) - 1
    name = syms[k][1] if k >= 0 else '?'
    name = re.sub(r'::h[0-9a-f]{16}$', '', name)
    cnt[name] += 1
print(f'{n} samples')
for name, c in cnt.most_common(top): print(f'{100*c/n:6.2f}%  {name[:150]}')
