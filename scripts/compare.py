#!/usr/bin/env python3
"""compare.py A.tsv B.tsv : over the builds both files have - verdict changes, per size class the sums that a pass
count hides, and the tests the acceptance rule uses (docs/EVALUATION.md): exact McNemar on verdicts, Wilcoxon
signed-rank on bad cells per build, and how likely the net loss is if nothing really changed."""
import sys, os
sys.path.insert(0, os.path.dirname(__file__))
import results
a, b = results.load(sys.argv[1]), results.load(sys.argv[2])
common = [h for h in b if h in a]
def line(name, hs):
    A = [a[h] for h in hs]; B = [b[h] for h in hs]
    s = lambda rs, f: sum(f(r) for r in rs)
    print(f"{name:8s} {len(hs):4d} | pass {s(A, lambda r: r.passed):3d} -> {s(B, lambda r: r.passed):3d} | wide {s(A, lambda r: r.wide or r.passed):3d} -> {s(B, lambda r: r.wide or r.passed):3d}"
          f" | missing {s(A, lambda r: r.missing):6d} -> {s(B, lambda r: r.missing):6d} | wrong {s(A, lambda r: r.wrong):5d} -> {s(B, lambda r: r.wrong):5d}"
          f" | leftover {s(A, lambda r: r.leftover):6d} -> {s(B, lambda r: r.leftover):6d} | failed sections {s(A, lambda r: r.failed):4d} -> {s(B, lambda r: r.failed):4d}"
          f" | ticks {s(A, lambda r: r.ticks):9d} -> {s(B, lambda r: r.ticks):9d} | ticks changed in {sum(a[h].ticks != b[h].ticks for h in hs)}")
for lo, hi, name in results.SIZES:
    hs = [h for h in common if b[h].size() == name]
    if hs: line(name, hs)
line('all', common)
for h in common:
    if a[h].passed != b[h].passed or a[h].wide != b[h].wide:
        print('  ', h[:8], 'PASS' if a[h].passed else 'FAIL', '->', 'PASS' if b[h].passed else 'FAIL', 'wide' if b[h].wide else '', 'blocks', b[h].blocks,
              f"missing {a[h].missing}->{b[h].missing} leftover {a[h].leftover}->{b[h].leftover} | {b[h].cls()} | {b[h].bad[:110]}")
# ---- the tests
n01 = sum(b[h].passed and not a[h].passed for h in common); n10 = sum(a[h].passed and not b[h].passed for h in common)
pert = sum(a[h].ticks != b[h].ticks for h in common)
lam = results.FLIP_RATE * pert / 2
print(f"tests: FAIL->PASS {n01}, PASS->FAIL {n10}: McNemar exact one-sided p(B better) = {results.mcnemar(n01, n10):.3f}")
n, w, p = results.signed_rank([a[h].bad_cells() - b[h].bad_cells() for h in common])
better = sum(a[h].bad_cells() > b[h].bad_cells() for h in common); worse = sum(a[h].bad_cells() < b[h].bad_cells() for h in common)
print(f"       bad cells per build: fewer in {better}, more in {worse}: signed-rank one-sided p(B better) = {p:.4f}")
m, pl = results.paired_log_test([a[h].bad_cells() for h in common], [b[h].bad_cells() for h in common])
cat_up = [h for h in common if b[h].bad_cells() >= 10 * (a[h].bad_cells() + 1) and b[h].bad_cells() >= 50]
cat_dn = [h for h in common if a[h].bad_cells() >= 10 * (b[h].bad_cells() + 1) and a[h].bad_cells() >= 50]
print(f"       log10(1 + bad cells): mean change {-m:+.3f} per build, sign-flip test one-sided p(B better) = {pl:.4f};"
      f" collapses (10x more bad cells, 50+): {len(cat_up)} {[h[:8] for h in cat_up]}, recoveries: {len(cat_dn)} {[h[:8] for h in cat_dn]}")
print(f"       guard: course changed in {pert} builds; if nothing really changed about {lam:.1f} PASS->FAIL are expected anyway,"
      f" and a net loss of {max(0, n10 - n01)} or more has probability {results.skellam_ge(max(0, n10 - n01), lam):.2f}")
keys = ['died', 'shutEpisodes', 'hurt', 'retreats', 'stubs', 'putOff', 'unstuck', 'mined', 'fromInside', 'chained', 'fromClose', 'pits', 'forcedWorse', 'tidied']
ma = [a[h].metrics for h in common if a[h].metrics]; mb = [b[h].metrics for h in common if b[h].metrics]
for label, ms in (('A', ma), ('B', mb)):
    if ms: print(f"       counters {label} ({len(ms)} builds): " + ', '.join(f"{k}={sum(int(m.get(k, 0)) for m in ms)}" for k in keys)
                 + f", builds with a shut-in episode={sum(1 for m in ms if int(m.get('shutEpisodes', 0)) > 0)}")
