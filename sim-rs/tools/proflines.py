#!/usr/bin/env python3
"""Hottest source lines from an MCSIM_PROF sample file. usage: proflines.py <samples> [top] [binary]"""
import sys, subprocess, collections
binary = sys.argv[3] if len(sys.argv) > 3 else 'sim-rs/sim/target/release/mcsim'
top = int(sys.argv[2]) if len(sys.argv) > 2 else 40
addrs = [l.strip() for l in open(sys.argv[1])]
cnt = collections.Counter(addrs)
uniq = list(cnt)
out = subprocess.run(['addr2line', '-e', binary] + ['0x' + a for a in uniq], capture_output=True, text=True).stdout.splitlines()
lines = collections.Counter()
for a, loc in zip(uniq, out):
    loc = loc.split(' ')[0].replace('sim-rs/sim/', '')
    lines[loc] += cnt[a]
n = len(addrs)
for loc, c in lines.most_common(top): print(f'{100*c/n:6.2f}%  {loc}')
