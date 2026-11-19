#!/usr/bin/env python3
"""impossible_rescore.py <causes dir> <exempt file from `mcsim impossible`> [--by-fluid-too | --no-by-fluid]
Re-scores stored runs under the owner's rule (docs/IMPOSSIBLE.md) without simulating: for every failed section in a
build's MCSIM_CAUSES file (S tgt bad, then its C lines with the bad cells), the exempt cells of that section leave
its target count, and the bad ones among them leave its bad count; the section passes if bad/target <= 3 % (a section
with no target cells passes only with nothing bad). A build passes when no section fails. Passing sections are not in
the file; removing exempt cells from them can only make them fail if exempt cells stood right there, which is
counted for the failed sections ("exempt but right") as the check of that assumption.
"""
import sys, glob, os, collections
d, ef = sys.argv[1], sys.argv[2]
nofluid = '--no-by-fluid' in sys.argv
ex = collections.defaultdict(dict)          # hash -> {(x,y,z): (class, block)}
for l in open(ef):
    p = l.split()
    if p[0] != 'E': continue
    if nofluid and len(p) > 7 and p[7] == 'by_fluid': continue
    ex[p[1]][(int(p[2]), int(p[3]), int(p[4]))] = (p[5], p[6].split('[')[0])
res = []; cls = collections.Counter(); right = 0; files = 0; rightk = collections.Counter(); risk = 0; riskb = set()
for f in sorted(glob.glob(d + '/*.causes')):
    h = os.path.basename(f)[:-7]; files += 1
    E = ex.get(h, {})
    secs = []; cur = None
    for l in open(f):
        if l.startswith('S '):
            a = l.split(); cur = {'c': tuple(int(v) for v in a[1:4]), 'tgt': int(a[4]), 'bad': int(a[5]), 'cells': set()}; secs.append(cur)
        elif l.startswith('C ') and cur is not None:
            a = l.split(' | ')[0].split()
            if a[1] in ('missing', 'wrong'): cur['cells'].add((int(a[2]), int(a[3]), int(a[4])))
    # passing sections with exempt cells: in the worst case (3 % bad and every exempt cell standing right) the section
    # would fail once they leave its count - an upper bound on what the rule could cost, without the world
    failed = {s['c'] for s in secs}
    by = collections.Counter((q[0] >> 4, q[1] >> 4, q[2] >> 4) for q in E)
    for c, n in by.items():
        if c not in failed and n > 0: risk += 1; riskb.add(h)
    if not secs: res.append((h, 'PASS', 'PASS', [])); continue
    flips = []; still = 0
    for s in secs:
        cx, cy, cz = s['c']
        ein = [q for q in E if q[0] >> 4 == cx and q[1] >> 4 == cy and q[2] >> 4 == cz]
        ebad = [q for q in ein if q in s['cells']]
        right += len(ein) - len(ebad)
        for q in ein:
            if q not in s['cells']: rightk[E[q]] += 1
        t2 = s['tgt'] - len(ein); b2 = s['bad'] - len(ebad)
        ok = (b2 == 0) if t2 <= 0 else b2 / t2 <= 0.03
        for q in ebad: cls[E[q]] += 1
        if ok: flips.append((s['c'], s['tgt'], s['bad'], t2, b2, collections.Counter(E[q] for q in ebad).most_common(3)))
        else: still += 1
    res.append((h, 'FAIL', 'PASS' if still == 0 else 'FAIL', flips if still == 0 else []))
before = sum(r[1] == 'PASS' for r in res); after = sum(r[2] == 'PASS' for r in res)
print(f'{d}: {files} cause files; pass {before} -> {after}; exempt cells that were bad in failed sections: {sum(cls.values())}; exempt cells standing right in failed sections: {right}')
for (c, b), n in cls.most_common(12): print(f'   {n:6}  {c:22} {b}')
print('  exempt but standing right in failed sections, by block: ' + ', '.join(f'{b} {c} x{n}' for (c, b), n in rightk.most_common(8)))
print(f'  passing sections holding exempt cells (could only fail if those cells stood right there): {risk} in {len(riskb)} builds')
for h, b, a, fl in res:
    if b != a:
        print(f'  FLIP {h[:8]}: ' + '; '.join(f'section {c} {t}->{t2} cells, bad {bb}->{b2} ({", ".join(f"{k[1]} {k[0]} x{v}" for k, v in top)})' for c, t, bb, t2, b2, top in fl))
