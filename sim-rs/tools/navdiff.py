#!/usr/bin/env python3
"""Compares the counters of a Java verbose log (`RunOne <hash> v`: the RESULT and NAV lines) with the simulator's
(`mcsim run <hash> --verbose`: its NAV line). usage: navdiff.py <java verbose dir> [hash8 ...] [--rev COMMIT]
Runs the simulator for every log-<hash8>.txt in the directory (or the given ones) and prints the counters that
differ. The searches / expanded pair is the sharpest test there is: equal means every route search of the run was
the same search. (Where scaffolding is left at the end, the Java verbose run makes three more searches for its END
line; those builds are marked.)"""
import glob, os, re, subprocess, sys
args = [a for a in sys.argv[1:] if not a.startswith('--')]
rev = sys.argv[sys.argv.index('--rev') + 1] if '--rev' in sys.argv else open('/work/port/java/COMMIT').read().strip()
args = [a for a in args if a != rev]
d = args[0]
want = args[1:]
uniq = [l.split('\t')[0] for l in open('/corpus/unique.tsv')]
KEYS = ['alreadyShut', 'hurt', 'retreats', 'stubs', 'unsafeSpots', 'helped', 'fromInside', 'chained', 'fromClose', 'pits', 'escapes', 'mined', 'strays', 'misplaced',
        'traps avoided/checks/searches', 'towerBlocks', 'roped', 'rescued', 'gates', 'rope undone/orphaned', 'stances self/natural/lean/pillar', 'moves pillar/bridge/dig',
        'buildTicks', 'searches', 'expanded', 'plans', 'planFails', 'predictions']
def counters(text):
    out = {}
    nav = [l for l in text.split('\n') if 'NAV ' in l]
    if not nav: return out
    line = nav[-1][nav[-1].index('NAV '):]
    line = line.split(' END ')[-1] if ' END ' in line else line      # (the MAIN part repeats some names)
    for k in KEYS:
        m = re.search(r'(?:^| )' + re.escape(k) + r'=(\S+)', line)
        if m: out[k] = m.group(1)
    m = re.search(r'NAV .*? alreadyShut=(\S+) hurt=(\S+) retreats=(\S+)', nav[-1])
    if m: out['alreadyShut'], out['hurt'], out['retreats'] = m.group(1), m.group(2), m.group(3)
    res = [l for l in text.split('\n') if 'RESULT ' in l]
    if res:
        for k in ['ticks', 'placed', 'broken', 'replans', 'left']:
            m = re.search(r' ' + k + r'=(\d+)', res[-1])
            if m: out[k] = m.group(1)
    m = re.search(r' placed=(\d+) broken=(\d+) replans=(\d+)$', nav[-1].strip())
    if m and not res: out['placed'], out['broken'], out['replans'] = m.group(1), m.group(2), m.group(3)
    return out
same = 0; n = 0
for p in sorted(glob.glob(os.path.join(d, 'log-*.txt'))):
    h8 = os.path.basename(p)[4:12]
    if want and h8 not in want: continue
    h = next(u for u in uniq if u.startswith(h8))
    java = counters(open(p, errors='replace').read())
    r = subprocess.run(['/work/sim/target/release/mcsim', 'run', h, '--verbose', '--rev', rev], capture_output=True, text=True)
    sim = counters(r.stdout)
    row = r.stdout.strip().split('\n')[-1].split('\t')
    if len(row) > 5: sim['ticks'] = row[4]
    diff = {k: (java[k], sim.get(k)) for k in java if k in sim and java[k] != sim[k]}
    n += 1
    left = java.get('left', '0') != '0'
    if not diff: same += 1
    else: print(h8, '(scaffolding left: Java verbose adds END searches)' if left else '', ' '.join(f'{k}: {a} / {b}' for k, (a, b) in diff.items()))
print(f'{same} of {n} builds: all counters equal (Java / simulator shown where not)')
