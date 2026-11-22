"""Shared reading of RunCorpus result files (TSV) and the failure classes used in the summaries."""
import csv, os
TICK_LIMIT = 3456000
SIZES = [(0, 300, '<300'), (300, 1000, '300-1k'), (1000, 3000, '1k-3k'), (3000, 10000, '3k-10k'), (10000, 30000, '10k-30k'), (30000, 10**9, '30k+')]
_blocks = None

def blocks_of(h):
    """Target size from the corpus index (for rows that have none: crashes, no result)."""
    global _blocks
    if _blocks is None:
        _blocks = {}
        p = 'work/corpus/unique.tsv'
        if os.path.exists(p):
            for l in open(p):
                f = l.split('\t')
                if len(f) > 4: _blocks[f[0]] = int(f[4])
    return _blocks.get(h, 0)

class Row:
    def __init__(self, r):
        self.r = r
        self.hash = r[0]
        self.passed = r[1] == 'PASS'
        self.status = r[2] if len(r) > 2 else '?'
        full = len(r) > 16
        self.full = full
        self.blocks = int(r[3]) if full else blocks_of(r[0])
        self.ticks = int(r[4]) if full else 0
        self.skipped = int(r[6]) if full else 0
        self.missing = int(r[7]) if full else 0
        self.wrong = int(r[8]) if full else 0
        self.leftover = int(r[9]) if full else 0
        self.sections = int(r[10]) if full else 0
        self.failed = int(r[11]) if full else 0
        self.wall = int(r[16]) if full else 0
        self.bad = r[18] if len(r) > 18 else ''
        self.skips = r[19] if len(r) > 19 else ''
        self.wide = len(r) > 21 and r[21] == 'widePASS'
        # (since 9b80ffd) worst section ratio and the bot's counters: died=..;hurt=..;shutEpisodes=..;...
        self.worst = float(r[22]) if len(r) > 22 and r[22].replace('.', '', 1).isdigit() else None
        self.metrics = {}
        if len(r) > 23 and '=' in r[23]:
            for kv in r[23].split(';'):
                k, _, v = kv.partition('=')
                self.metrics[k] = int(v) if v.lstrip('-').isdigit() else v
    def bad_cells(self):
        return self.missing + self.wrong + self.leftover
    def size(self):
        for lo, hi, name in SIZES:
            if lo <= self.blocks < hi: return name
        return '?'
    def cls(self):
        """Dominant cause of a failing row, rules applied in this order."""
        if self.passed: return 'pass'
        if not self.full: return 'no result'
        if self.status.startswith('wall'): return 'wall limit'
        if self.status.startswith('out of time') or self.ticks >= 0.9 * TICK_LIMIT: return 'tick budget'
        if self.leftover > 0 and self.leftover >= self.missing + self.wrong: return 'leftover scaffolding'
        if self.wide: return 'creative-only / exempt (passes wide)'
        if self.missing + self.wrong >= 100: return 'unplaced region'
        return 'few cells'

def load(path):
    if not os.path.exists(path): return {}
    out = {}
    for r in csv.reader(open(path), delimiter='\t', quoting=csv.QUOTE_NONE):
        if len(r) > 1 and r[1] in ('PASS', 'FAIL'): out[r[0]] = Row(r)
    return out


# ------------------------------------------------------------------ small statistics (no libraries)
import math

def binom_tail(k, n, p=0.5):
    """P(X >= k) for X ~ Binomial(n, p)."""
    return sum(math.comb(n, i) * p ** i * (1 - p) ** (n - i) for i in range(k, n + 1))

def mcnemar(n01, n10):
    """Exact one-sided p-value that B is better than A: n01 builds went FAIL->PASS, n10 PASS->FAIL."""
    n = n01 + n10
    return 1.0 if n == 0 else binom_tail(n01, n)

def signed_rank(diffs):
    """Wilcoxon signed-rank, exact, one-sided: p-value that the differences (A - B, positive = B better) lean positive.
    Returns (number of non-zero pairs, W+, p)."""
    d = [x for x in diffs if x != 0]
    n = len(d)
    if n == 0: return 0, 0.0, 1.0
    order = sorted(range(n), key=lambda i: abs(d[i]))
    rank2 = [0] * n                      # twice the rank (ties share the mean rank)
    i = 0
    while i < n:
        j = i
        while j + 1 < n and abs(d[order[j + 1]]) == abs(d[order[i]]): j += 1
        for k in range(i, j + 1): rank2[order[k]] = i + j + 2
        i = j + 1
    w2 = sum(rank2[i] for i in range(n) if d[i] > 0)
    total = sum(rank2)
    if n <= 40:
        ways = {0: 1}                    # distribution of 2*W+ under the null: each rank counted with probability 1/2
        for r in rank2:
            nxt = {}
            for s_, c in ways.items():
                nxt[s_] = nxt.get(s_, 0) + c
                nxt[s_ + r] = nxt.get(s_ + r, 0) + c
            ways = nxt
        p = sum(c for s_, c in ways.items() if s_ >= w2) / 2 ** n
    else:
        mean = total / 2; var = sum(r * r for r in rank2) / 4
        z = (w2 - mean - 1) / math.sqrt(var)
        p = 0.5 * math.erfc(z / math.sqrt(2))
    return n, w2 / 2, p

def skellam_ge(d, lam):
    """P(X - Y >= d) for independent X, Y ~ Poisson(lam): net loss of d builds or more when nothing really changed."""
    po = lambda k: math.exp(-lam) * lam ** k / math.factorial(k)
    return sum(po(x) * po(y) for x in range(0, 60) for y in range(0, 60) if x - y >= d)

FLIP_RATE = 0.037   # verdict flips per build whose course changed, measured on the small samples (scripts/evalmath.py)

def paired_log_test(xa, xb, rounds=20000):
    """Sign-flip permutation test on d = log10(1+a) - log10(1+b) per build (positive = B better): one-sided p-value and
    the mean. Bad cells are heavy-tailed (0 ... 40,000 in one build); on the log scale a collapse and a recovery count
    by their order of magnitude, and a +3 on a build with 20 bad cells counts for little."""
    import random
    d = [math.log10(1 + x) - math.log10(1 + y) for x, y in zip(xa, xb)]
    d = [x for x in d if x != 0]
    if not d: return 0.0, 1.0
    obs = sum(d)
    rnd = random.Random(12345)
    if len(d) <= 16:
        hits = 0
        for mask in range(1 << len(d)):
            if sum(x if mask >> i & 1 else -x for i, x in enumerate(d)) >= obs - 1e-12: hits += 1
        return obs / len(xa), hits / (1 << len(d))
    hits = sum(1 for _ in range(rounds) if sum(x if rnd.random() < 0.5 else -x for x in d) >= obs - 1e-12)
    return obs / len(xa), (hits + 1) / (rounds + 1)
