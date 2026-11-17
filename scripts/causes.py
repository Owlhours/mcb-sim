#!/usr/bin/env python3
"""Failed sections by cause, from the simulator's MCSIM_CAUSES files.
usage: causes.py <dir> [--builds] [--top N]

Make the files with   MCSIM_CAUSES=<dir> scripts/simrun.sh <tag> <set>   (one <hash>.causes per build: the failed
sections of the judge, and for every bad cell in them the bot's own last word on it - why the cell was skipped, why
it was last put off, or why the clean-up gave the scaffolding piece up; `build::cause_lines`).

Per cause:
  cells      bad cells in failed sections
  sections   failed sections in which it is the largest cause
  alone      failed sections that would pass if only this cause's cells were right (the others staying bad)
  builds     failing builds in which it has any cell in a failed section
  top        failing builds in which it has more bad cells than any other cause
  wins       failing builds that would PASS if this cause's cells were right and nothing else changed
--builds lists every failing build with its causes.
"""
import sys, os, glob, collections

def norm(c):
    c = c.strip()
    # (the end states of a build that did not finish: one cause each, whatever the cell's own last word was)
    for st in ('out of time', 'wall limit', 'died', 'crash'):
        if c.startswith(st + ':') or c.startswith('clean-up: ' + st): return st
    # (the bot stopped placing to have time for the clean-up: one cause, whatever the cell's own last word was)
    if c.startswith('time ran short'): return 'time ran short (stopped placing to clean up)'
    return c

def load(path):
    secs = []; head = ''
    for l in open(path):
        l = l.rstrip('\n')
        if l.startswith('B '): head = l[2:]
        elif l.startswith('S '):
            p = l.split(); secs.append({'tgt': int(p[4]), 'bad': int(p[5]), 'c': collections.Counter()})
        elif l.startswith('C ') and secs:
            f = l.split(' | ')
            secs[-1]['c'][norm(f[1])] += 1
    return head, secs

def passes(tgt, bad):
    return (bad == 0) if tgt == 0 else bad / tgt <= 0.03

def main():
    a = [x for x in sys.argv[1:] if not x.startswith('--')]
    if not a: print(__doc__); return
    per_build = '--builds' in sys.argv
    top_n = int(sys.argv[sys.argv.index('--top') + 1]) if '--top' in sys.argv else 30
    T = collections.defaultdict(lambda: collections.Counter())
    nb = nf = ns = 0; lines = []
    for f in sorted(glob.glob(a[0] + '/*.causes')):
        head, secs = load(f); nb += 1
        if not secs: continue
        nf += 1; ns += len(secs)
        tot = collections.Counter()
        for s in secs:
            tot.update(s['c'])
            big = s['c'].most_common(1)[0][0]
            T[big]['sections'] += 1
            for c, n in s['c'].items():
                T[c]['cells'] += n
                if passes(s['tgt'], s['bad'] - n): T[c]['alone'] += 1
        for c in tot:
            T[c]['builds'] += 1
            if all(passes(s['tgt'], s['bad'] - s['c'][c]) for s in secs): T[c]['wins'] += 1
        T[tot.most_common(1)[0][0]]['top'] += 1
        lines.append(f"{os.path.basename(f)[:8]} sections={len(secs)} bad={sum(tot.values())} [{head.split(' ticks')[0]}] " + '; '.join(f'{c}={n}' for c, n in tot.most_common(4)))
    print(f'{nb} builds read, {nf} failing, {ns} failed sections, {sum(v["cells"] for v in T.values())} bad cells in them')
    print(f'{"cause":58} {"cells":>7} {"sections":>8} {"alone":>6} {"builds":>6} {"top":>4} {"wins":>5}')
    for c, v in sorted(T.items(), key=lambda kv: -kv[1]['cells'])[:top_n]:
        print(f'{c[:58]:58} {v["cells"]:7} {v["sections"]:8} {v["alone"]:6} {v["builds"]:6} {v["top"]:4} {v["wins"]:5}')
    if per_build:
        print(); print('\n'.join(lines))

main()
