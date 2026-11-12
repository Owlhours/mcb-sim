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
        p = '/work/corpus/unique.tsv'
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
