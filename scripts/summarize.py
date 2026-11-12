#!/usr/bin/env python3
"""Summarises a results TSV written by RunCorpus."""
import sys, collections, re
rows = [l.rstrip('\n').split('\t') for l in open(sys.argv[1])]
n = len(rows); p = sum(1 for r in rows if r[1] == 'PASS')
print(f'{n} builds, {p} pass ({100*p/max(1,n):.1f}%)')
wide = [r for r in rows if len(r) > 21]
if wide:
    pw = sum(1 for r in wide if r[21] == 'widePASS')
    print(f'wider reading of the exemptions (owner\'s phase-1 rules): {pw} of {len(wide)} pass ({100*pw/len(wide):.1f}%)')
status = collections.Counter(r[2].split(':')[0] for r in rows); print('status', dict(status))
ok = [r for r in rows if len(r) > 16]
if ok:
    blocks = sum(int(r[3]) for r in ok); ticks = sum(int(r[4]) for r in ok); wall = sum(int(r[16]) for r in ok)
    print(f'blocks {blocks}, ticks {ticks} ({ticks/max(1,blocks):.1f}/block), wall {wall/1000:.0f}s ({wall/max(1,blocks):.2f} ms/block)')
    print('missing', sum(int(r[7]) for r in ok), 'wrong', sum(int(r[8]) for r in ok), 'leftover', sum(int(r[9]) for r in ok), 'skipped', sum(int(r[6]) for r in ok))
    bad = collections.Counter(); why = collections.Counter(); kind = collections.Counter()
    for r in ok:
        for part in r[18].split('; '):
            if '=' in part:
                k, v = part.rsplit('=', 1); bad[k] += int(v); kind[k.split(' ')[0]] += int(v)
        if len(r) > 19:
            for m in re.finditer(r'([^,{}=]+)=(\d+)', r[19]): why[m.group(1).strip()] += int(m.group(2))
    print('top bad:', bad.most_common(25))
    print('skip reasons:', why.most_common(25))
    fails = collections.Counter()
    for r in ok:
        if r[1] != 'PASS':
            m, w, l = int(r[7]), int(r[8]), int(r[9])
            fails['missing' if m >= max(w, l) else 'wrong' if w >= l else 'leftover'] += 1
    print('dominant failure:', dict(fails))

# by size class and by dominant cause (see results.py for the rules)
sys.path.insert(0, __import__('os').path.dirname(__file__))
import results
R = results.load(sys.argv[1])
print('size      builds pass wide | missing  wrong leftover | failed/sections | t/block | >=90% ticks')
for lo, hi, name in results.SIZES:
    rs = [r for r in R.values() if r.size() == name]
    if not rs: continue
    b = sum(r.blocks for r in rs if r.full) or 1
    print(f"{name:9s} {len(rs):6d} {sum(r.passed for r in rs):4d} {sum(r.wide or r.passed for r in rs):4d} | {sum(r.missing for r in rs):7d} {sum(r.wrong for r in rs):6d} {sum(r.leftover for r in rs):8d} |"
          f" {sum(r.failed for r in rs):6d}/{sum(r.sections for r in rs):<8d} | {sum(r.ticks for r in rs)/b:7.1f} | {sum(r.ticks >= 0.9*results.TICK_LIMIT for r in rs)}")
cl = collections.Counter(r.cls() for r in R.values() if not r.passed)
print('failing builds by class:', dict(cl.most_common()))
