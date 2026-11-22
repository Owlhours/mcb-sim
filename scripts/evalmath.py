#!/usr/bin/env python3
"""evalmath.py : the numbers behind docs/EVALUATION.md, from files that exist (no bot runs).
  1. corpus weights per size class            (corpus/unique.tsv)
  2. hazard model  -ln P(pass) = alpha + kappa*N   fitted to the owner's full run (notes/full-run-59dc842.tsv)
  3. how often verdicts flip between two runs of nearly the same code (paired result files in tmp/)
  4. what follows: false-rejection rate of "no PASS->FAIL", detectable effect, value of each lever
"""
import math, os, sys
sys.path.insert(0, os.path.dirname(__file__))
import results

T = results.TICK_LIMIT
# ---------------------------------------------------------------- 1. corpus weights
sizes = []
for l in open('work/corpus/unique.tsv'):
    f = l.split('\t')
    if len(f) > 4: sizes.append(int(f[4]))
n = len(sizes)
cls = lambda N: next(name for lo, hi, name in results.SIZES if lo <= N < hi)
w = {}
for N in sizes: w[cls(N)] = w.get(cls(N), 0) + 1
print("1. corpus: %d builds; share per size class (w_c), blocks in class / all blocks" % n)
tot_blocks = sum(sizes)
for lo, hi, name in results.SIZES:
    print("   %-8s w=%.3f  blocks share=%.3f" % (name, w.get(name, 0) / n, sum(N for N in sizes if lo <= N < hi) / tot_blocks))
over = [N for N in sizes if N > 100000]
print("   > 100k blocks: %.3f of builds; > 300k: %.3f" % (len(over) / n, sum(1 for N in sizes if N > 300000) / n))

# ---------------------------------------------------------------- 2. hazard model
R = results.load('work/notes/full-run-59dc842.tsv')
rows = [(r.blocks, r.passed, r.wide or r.passed, r.ticks) for r in R.values() if r.blocks > 0]
def fit(data):
    """MLE of P(pass) = exp(-(a + k N)), a >= 0, k >= 0, by coordinate search on the log-likelihood."""
    def ll(a, k):
        s = 0.0
        for N, ok in data:
            h = a + k * N
            if h <= 0: h = 1e-12
            s += -h if ok else math.log(1 - math.exp(-h))
        return s
    best = (-1e18, 0, 0)
    for ai in range(0, 60):
        a = ai * 0.005
        for ki in range(0, 80):
            k = ki * 0.5e-6
            v = ll(a, k)
            if v > best[0]: best = (v, a, k)
    return best
print("\n2. hazard model on the full run (%d builds): P(pass) = exp(-(alpha + kappa*N))" % len(rows))
for label, idx in (("strict", 1), ("wide", 2)):
    v, a, k = fit([(r[0], r[idx]) for r in rows])
    v0, a0, _ = max(((sum(-aa if r[idx] else math.log(1 - math.exp(-aa)) for r in rows), aa, 0) for aa in [i * 0.005 for i in range(1, 100)]))
    print("   %-6s alpha=%.3f (per build)  kappa=%.2e per block (one fatal event per %.0fk blocks);  log-lik %.1f vs %.1f without kappa (LR=%.1f)"
          % (label, a, k, 1e-3 / k if k else float('inf'), v, v0, 2 * (v - v0)))
    for lo, hi, name in results.SIZES:
        rs = [r for r in rows if lo <= r[0] < hi]
        if rs:
            pred = sum(math.exp(-(a + k * r[0])) for r in rs)
            print("      %-8s builds %3d  passed %3d  model expects %.1f" % (name, len(rs), sum(r[idx] for r in rs), pred))
    if label == "strict": A, K = a, k
# corpus-level expectation and levers (strict)
tpb = sorted(r[3] / r[0] for r in rows if r[0] >= 10000 and r[3] > 0)
tau = tpb[len(tpb) // 2]
def J(a, k, tau_):
    return sum(math.exp(-(a + k * N)) for N in sizes if N * tau_ <= T) / n
base = J(A, K, tau)
print("   corpus-wide expected pass rate under the model (median %.0f ticks/block on 10k+): J = %.3f" % (tau, base))
print("   levers:  kappa halved: %+.3f   alpha halved: %+.3f   twice as fast: %+.3f   all three: %+.3f"
      % (J(A, K / 2, tau) - base, J(A / 2, K, tau) - base, J(A, K, tau / 2) - base, J(A / 2, K / 2, tau / 2) - base))
print("   share of the corpus that cannot finish in time at that speed: %.3f" % (sum(1 for N in sizes if N * tau > T) / n))

# ---------------------------------------------------------------- 3. flips between runs
pairs = [("A->B (4+ changes)", "results-%s-A.tsv", "results-%s-B.tsv"), ("B->C (3 changes)", "results-%s-B.tsv", "results-%s-C.tsv"),
         ("C->E (3 small changes)", "results-%s-C.tsv", "results-%s-BASE.tsv")]
print("\n3. paired runs on the 220 small builds: builds whose ticks changed (perturbed), PASS->FAIL, FAIL->PASS")
tot_p = tot_f = 0
def paired(a, b, label):
    global tot_p, tot_f
    common = [h for h in b if h in a]
    pert = sum(a[h].ticks != b[h].ticks for h in common)
    n10 = sum(a[h].passed and not b[h].passed for h in common); n01 = sum(b[h].passed and not a[h].passed for h in common)
    same = sum(a[h].passed != b[h].passed and a[h].ticks == b[h].ticks for h in common)
    print("   %-26s builds %3d  perturbed %3d  PASS->FAIL %d  FAIL->PASS %d  (flips without a tick change: %d)" % (label, len(common), pert, n10, n01, same))
    tot_p += pert; tot_f += n10 + n01
    return pert, n10, n01
for label, fa, fb in pairs:
    a = {}; b = {}
    for s in ("small-dev", "strat-small-w", "dev2b"):
        a.update(results.load('work/tmp/' + fa % s)); b.update(results.load('work/tmp/' + fb % s))
    if a and b: paired(a, b, label)
base220 = results.load('work/tmp/results-small220-f6b21f4.tsv')
for label, f in (("baseline->v2", "results-small220-v2.tsv"), ("baseline->v3", "results-small220-v3.tsv")):
    b = results.load('work/tmp/' + f)
    if b: paired(base220, b, label)
q = tot_f / max(1, tot_p)
print("   pooled: %d flips in %d perturbed builds: q = %.3f per perturbed build" % (tot_f, tot_p, q))

# ---------------------------------------------------------------- 4. consequences
def pois(k, lam): return math.exp(-lam) * lam ** k / math.factorial(k)
def skellam_ge(d, lam):   # P(X - Y >= d), X, Y ~ Poisson(lam) independent
    return sum(pois(x, lam) * pois(y, lam) for x in range(0, 40) for y in range(0, 40) if x - y >= d)
print("\n4. consequences for a change that perturbs m of the 220 builds and has NO true effect:")
for m in (50, 100, 200):
    lam = q * m / 2
    print("   m=%3d: expected PASS->FAIL = %.1f;  P(rule 'no PASS->FAIL' rejects it) = %.2f;  sd of the net change = %.1f builds;"
          "  P(net loss >= 3) = %.2f, >= 4: %.2f, >= 5: %.2f" % (m, lam, 1 - math.exp(-lam), math.sqrt(2 * lam), skellam_ge(3, lam), skellam_ge(4, lam), skellam_ge(5, lam)))
lam = q * 200 / 2
print("   smallest true gain detectable at 2 sd with one paired pass (m=200): %.1f builds of 220 (%.1f %%)" % (2 * math.sqrt(2 * lam), 100 * 2 * math.sqrt(2 * lam) / 220))
print("   with R seeds per version the sd falls by sqrt(R): R=4 -> %.1f builds" % (2 * math.sqrt(2 * lam / 4)))
