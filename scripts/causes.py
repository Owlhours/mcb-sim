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
--ticks: what the ticks went to instead (phase, and what the bot was doing), for passing builds, builds failing on
time and other failing builds.
--builds lists every failing build with its causes. A cell with no face to click that touches a cell bad for another
reason is counted under that reason (--no-roots: under its own).
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

NOFACE = 'no way to place: no face to click from anywhere'
def load(path, roots=True):
    secs = []; head = ''; cells = {}
    for l in open(path):
        l = l.rstrip('\n')
        if l.startswith('B '): head = l[2:]
        elif l.startswith('S '):
            p = l.split(); secs.append({'tgt': int(p[4]), 'bad': int(p[5]), 'c': collections.Counter()})
        elif l.startswith('C ') and secs:
            f = l.split(' | '); a = f[0].split()
            cells[(int(a[2]), int(a[3]), int(a[4]))] = [len(secs) - 1, norm(f[1]), a[1]]
    if roots:
        # A cell with no face to click beside a cell that is bad for another reason has no face BECAUSE of it (what it
        # would be clicked against is not there): it is counted under that cell's cause, and so on through the cells
        # with no face that touch it. What is left as "no face" has no such neighbour.
        N = ((1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1))
        todo = [k for k, v in cells.items() if v[1] != NOFACE and v[2] != 'leftover']
        while todo:
            x, y, z = todo.pop(); c = cells[(x, y, z)][1]
            for dx, dy, dz in N:
                n = (x + dx, y + dy, z + dz)
                if n in cells and cells[n][1] == NOFACE:
                    cells[n][1] = c if c.endswith(' (+ no face beside it)') else c + ' (+ no face beside it)'
                    todo.append(n)
    for sec, c, _ in cells.values():
        secs[sec]['c'][c.replace(' (+ no face beside it)', '') if roots else c] += 1
    return head, secs

def ticks_table(d):
    """--ticks: what the bot's ticks went to (the T lines), per build class: passing / failing on time / failing otherwise"""
    import re
    cls = collections.defaultdict(lambda: collections.Counter()); nb = collections.Counter()
    for f in sorted(glob.glob(d + '/*.causes')):
        L = open(f).read().split('\n')
        head = L[0]; fail = any(l.startswith('S ') for l in L)
        timed = 'status=out of time' in head or 'time_cut=true' in head
        k = 'fails on time' if fail and timed else 'fails otherwise' if fail else 'passes'
        nb[k] += 1
        for l in L:
            if l.startswith('T '):
                a, b, n = l[2:].split(' | ')
                # (the outermost activity is the phase; a walk outside clean-up, put-off cells and restock is the main pass)
                phase = a if a in ('clean-up', 'put-off cells', 'restock trip') else 'main pass'
                cls[k][(phase, b)] += int(n)
    for k in ('passes', 'fails on time', 'fails otherwise'):
        if not nb[k]: continue
        tot = sum(cls[k].values())
        print(f'{k}: {nb[k]} builds, {tot / 1e6:.2f}M ticks')
        ph = collections.Counter()
        for (a, b), n in cls[k].items(): ph[a] += n
        for a, n in ph.most_common():
            print(f'  {a:14} {100 * n / tot:5.1f} %   ' + ', '.join(f'{b} {100 * m / tot:.1f}' for (aa, b), m in sorted(cls[k].items(), key=lambda kv: -kv[1]) if aa == a and m / tot >= 0.01))

def passes(tgt, bad):
    return (bad == 0) if tgt == 0 else bad / tgt <= 0.03

def main():
    a = [x for x in sys.argv[1:] if not x.startswith('--')]
    if not a: print(__doc__); return
    if '--ticks' in sys.argv: ticks_table(a[0]); return
    per_build = '--builds' in sys.argv
    top_n = int(sys.argv[sys.argv.index('--top') + 1]) if '--top' in sys.argv else 30
    T = collections.defaultdict(lambda: collections.Counter())
    nb = nf = ns = 0; lines = []
    for f in sorted(glob.glob(a[0] + '/*.causes')):
        head, secs = load(f, '--no-roots' not in sys.argv); nb += 1
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
