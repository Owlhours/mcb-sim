#!/usr/bin/env python3
"""Which block kinds (or simple features) of a build's target mark failing builds, beyond what the build's size explains.

usage: scripts/blockclasses.py <counts.txt> <rows.tsv> [rows.tsv ...] [--min 8] [--top 40] [--bands 10000,100000] [--detail 8]

counts.txt (or .gz): one line per build `COUNTS <hash> SXxSYxSZ kind=n ... _blocks=n _nonfull=n _gravity=n _nocollision=n _fluid=n _waterlogged=n`,
  from the simulator's diagnostic:   MCSIM_COUNT='*' mcsim run <hash> /dev/null --stop-at 1 | grep ^COUNTS   (the hash is put in by the caller:
  for h in $(cat hashes); do r=$(MCSIM_COUNT='*' mcsim run $h /dev/null --stop-at 1 2>/dev/null | grep ^COUNTS); echo "COUNTS $h ${r#COUNTS }"; done)
rows: simulator rows (1 hash, 2 PASS/FAIL, 3 status, 4 blocks, 8 missing, 9 wrong, 19 largest kinds of bad cells, 20 skip reasons).
  A build that is in several files counts once: the first file that has it wins (so give the rows of the current base first).

Control for size: builds are compared within size bands (blocks < 10k, 10k-100k, 100k+). For a kind, "expected" is the number of
failures its builds would have at the failure rate of the builds WITHOUT the kind in the same band; excess = failures - expected.
z = excess / sd (hypergeometric, summed over bands). Many kinds are tested: a z of 2 alone means little, look at counts.
"exc.fine" is the same excess under a finer control (six size bands x the sparse / dense half of each): what is left of the
kind once the build's size and density are accounted for. Kinds come in families (redstone parts, water plants): their
excesses overlap and must not be added up - see the GROUPS table and the table of what remains without the redstone builds.
Features (height, footprint, ...) are cut at the band's own median: "high" = above the median of its size band.
"""
import sys, re, math, collections

args = sys.argv[1:]
def opt(name, default):
    if name in args:
        i = args.index(name); v = args[i + 1]; del args[i:i + 2]; return v
    return default
MIN = int(opt('--min', 8)); TOP = int(opt('--top', 40)); DETAIL = int(opt('--detail', 8))
BANDS = [int(x) for x in opt('--bands', '10000,100000').split(',')]
if len(args) < 2: sys.exit(__doc__)

def band(n):
    return sum(n >= b for b in BANDS)
def band_name(i):
    k = lambda v: '%dk' % (v // 1000)
    return '<' + k(BANDS[0]) if i == 0 else k(BANDS[-1]) + '+' if i == len(BANDS) else k(BANDS[i - 1]) + '-' + k(BANDS[i])

counts = {}
import gzip
for l in (gzip.open(args[0], 'rt') if args[0].endswith('.gz') else open(args[0])):
    f = l.split()
    if len(f) < 3 or f[0] != 'COUNTS': continue
    d = {}
    for kv in f[3:]:
        k, v = kv.rsplit('=', 1); d[k] = int(v)
    d['_dims'] = tuple(int(x) for x in f[2].split('x'))
    counts[f[1]] = d

builds = {}   # hash -> row
src = collections.Counter()
for path in args[1:]:
    for l in open(path):
        c = l.rstrip('\n').split('\t')
        if len(c) < 20 or c[1] not in ('PASS', 'FAIL') or c[0] in builds or c[0] not in counts: continue
        builds[c[0]] = c; src[path] += 1
H = sorted(builds)
fail = {h: builds[h][1] == 'FAIL' for h in H}
bnd = {h: band(int(builds[h][3])) for h in H}
nb = len(BANDS) + 1

print('builds: %d (%d fail)' % (len(H), sum(fail.values())))
for p, n in src.items(): print('  %4d from %s' % (n, p))
for i in range(nb):
    hs = [h for h in H if bnd[h] == i]
    print('  band %-9s %4d builds, %3d fail, pass rate %s' % (band_name(i), len(hs), sum(fail[h] for h in hs), '%.0f%%' % (100 - 100 * sum(fail[h] for h in hs) / len(hs)) if hs else '-'))

def stat(withset, strata=None):
    """builds with, fails with, expected fails (at the rate of the builds without, in the same stratum), sd, per-band text"""
    strata = strata or bnd
    n = f = 0; e = var = 0.0; txt = []
    groups = collections.defaultdict(list)
    for h in H: groups[strata[h]].append(h)
    for i in sorted(groups):
        hs = groups[i]
        w = [h for h in hs if h in withset]; o = [h for h in hs if h not in withset]
        fw = sum(fail[h] for h in w); fo = sum(fail[h] for h in o)
        n += len(w); f += fw
        txt.append('%d/%d|%d/%d' % (len(w) - fw, len(w), len(o) - fo, len(o)))
        if not w: continue
        if not o: e += fw; continue          # nothing to compare with in this stratum: no excess counted
        e += len(w) * fo / len(o)
        N = len(hs); K = fw + fo
        if N > 1: var += len(w) * (K / N) * (1 - K / N) * (N - len(w)) / (N - 1)
    return n, f, e, math.sqrt(var), txt

# (a finer control, as a check on the first: six size bands x the sparse / dense half of each - sparse builds fail much more)
def fine_strata():
    cuts = [3000, 10000, 30000, 100000, 300000]
    sb = {h: sum(int(builds[h][3]) >= c for c in cuts) for h in H}
    out = {}
    for i in set(sb.values()):
        hs = [h for h in H if sb[h] == i]
        v = sorted(dens(h) for h in hs); med = v[len(v) // 2]
        for h in hs: out[h] = (i, dens(h) >= med)
    return out
def dens(h):
    d = counts[h]; sx, sy, sz = d['_dims']; return d['_blocks'] / (sx * sy * sz)
FINE = fine_strata()

def line(name, withset):
    n, f, e, sd, txt = stat(withset)
    if n < MIN or n > len(H) - MIN: return None
    n2, f2, e2, sd2, _ = stat(withset, FINE)
    return (f - e, name, n, f, e, (f - e) / sd if sd > 0 else 0.0, txt, f2 - e2, (f2 - e2) / sd2 if sd2 > 0 else 0.0)

def show(rows, title):
    print('\n' + title)
    print('%-34s %5s %9s %9s %7s %5s %7s %5s   %s' % ('kind / feature', 'with', 'pass with', 'expected', 'excess', 'z', 'exc.fine', 'z', 'pass/n with|without per band (' + ', '.join(band_name(i) for i in range(nb)) + ')'))
    for x, name, n, f, e, z, txt, x2, z2 in rows:
        print('%-34s %5d %4d %3.0f%% %8.0f%% %+7.1f %+5.1f %+7.1f %+5.1f   %s' % (name, n, n - f, 100 * (n - f) / n, 100 * (n - e) / n, x, z, x2, z2, '  '.join(txt)))

kinds = sorted({k for h in H for k in counts[h] if not k.startswith('_')})
res1, res10 = [], []
for k in kinds:
    r = line(k, {h for h in H if counts[h].get(k, 0) > 0})
    if r: res1.append(r)
    r = line(k + ' >=10', {h for h in H if counts[h].get(k, 0) >= 10})
    if r: res10.append(r)
res1.sort(reverse=True); res10.sort(reverse=True)
show(res1[:TOP], 'BLOCK KINDS, present at all (in at least %d builds), by excess failures' % MIN)
show(res10[:TOP], 'BLOCK KINDS, at least 10 cells')
show(sorted(res1)[:10], 'the other end: kinds whose builds fail LESS than their size band')

# ---- families of kinds (their members' excesses overlap)
GROUPS = {
    'redstone parts (any)': ['piston', 'sticky_piston', 'repeater', 'comparator', 'observer', 'dispenser', 'dropper', 'hopper', 'redstone_wire'],
    'pistons': ['piston', 'sticky_piston'],
    'containers': ['chest', 'trapped_chest', 'barrel', 'hopper', 'dispenser', 'dropper', 'furnace', 'shulker_box'],
    'plants on water / in water': ['lily_pad', 'kelp', 'kelp_plant', 'seagrass', 'tall_seagrass', 'sea_pickle', 'sugar_cane'],
    'vines': ['vine', 'cave_vines', 'cave_vines_plant', 'weeping_vines', 'weeping_vines_plant', 'twisting_vines', 'twisting_vines_plant', 'glow_lichen'],
    'not placeable by hand': ['fire', 'soul_fire', 'jigsaw', 'command_block', 'repeating_command_block', 'chain_command_block', 'structure_block', 'brown_mushroom', 'red_mushroom'],
    'stained glass (any colour)': [k for k in kinds if k.endswith('_stained_glass')],
    'trapdoors': [k for k in kinds if k.endswith('_trapdoor')],
    'gravity blocks': ['sand', 'red_sand', 'gravel'] + [k for k in kinds if k.endswith('_concrete_powder')],
    'rails': [k for k in kinds if k == 'rail' or k.endswith('_rail')],
}
def gcount(h, ks): return sum(counts[h].get(k, 0) for k in ks)
gr = []
for g, ks in GROUPS.items():
    for lim in (1, 10, 100):
        r = line('%s >=%d' % (g, lim), {h for h in H if gcount(h, ks) >= lim})
        if r: gr.append(r)
gr.sort(reverse=True)
show(gr, 'GROUPS of kinds (cells of any member)')

# ---- what remains of the kinds outside the largest family
red = {h for h in H if gcount(h, GROUPS['redstone parts (any)']) >= 10}
H_all = H
H = [h for h in H_all if h not in red]
FINE_all = FINE; FINE = fine_strata()
rest = []
for k in kinds:
    r = line(k, {h for h in H if counts[h].get(k, 0) > 0})
    if r: rest.append(r)
rest.sort(reverse=True)
show(rest[:25], 'BLOCK KINDS among the %d builds (%d fail) WITHOUT ten or more redstone parts' % (len(H), sum(fail[h] for h in H)))
H = H_all; FINE = FINE_all

# ---- kinds whose own cells are what is bad: the direct defects
# (column 19 names only the eight largest kinds of bad cells of a build, so where the bot gave up or ran out of time the small kinds are
#  hidden behind the bulk: the table counts the builds that came to an end - status done - with under NEAR of their cells bad, where
#  the list is nearly complete and a kind in it was left bad selectively, not as part of an unbuilt region)
NEAR = 0.03
def bad_cells(row):
    """[(what, target kind, found kind or None, n)] from column 19"""
    out = []
    for m in re.finditer(r'(missing|wrong|leftover) (\S+?)(?: <- (\S+?))?=(\d+);', row[18]):
        out.append((m.group(1), m.group(2), m.group(3), int(m.group(4))))
    return out
def bad_share(h): return (int(builds[h][7]) + int(builds[h][8])) / max(1, counts[h]['_blocks'])
own = collections.defaultdict(lambda: [0, 0, 0, 0, 0, 0, 0])   # builds listed, of them failing, missing, wrong, target cells of the kind, builds with the kind, builds where it is the largest bad kind
near = [h for h in H if builds[h][2] == 'done' and bad_share(h) < NEAR]
for h in near:
    per = collections.Counter()
    bc = [c for c in bad_cells(builds[h]) if c[0] != 'leftover']
    for what, k, found, n in bc: per[k, what] += n
    for k in {k for k, _ in per}:
        o = own[k]; o[0] += 1; o[1] += fail[h]; o[2] += per[k, 'missing']; o[3] += per[k, 'wrong']
    if bc: own[bc[0][1]][6] += 1
    for k, n in counts[h].items():
        if not k.startswith('_'): own[k][4] += n; own[k][5] += 1
tot_bad = sum(int(builds[h][7]) + int(builds[h][8]) for h in near); tot_cells = sum(counts[h]['_blocks'] for h in near)
print('\nKINDS WHOSE OWN CELLS ARE BAD (direct defects), in the %d builds (%d FAIL) that came to an end with under %.0f%% of their cells bad' % (len(near), sum(fail[h] for h in near), 100 * NEAR))
print('  (all kinds together: %d bad of %d cells = %.3f%%; "bad share" = the kind\'s bad cells / all its cells in these builds)' % (tot_bad, tot_cells, 100.0 * tot_bad / tot_cells))
hdr = '%-34s %11s %8s %8s %9s %9s %11s %7s' % ('kind', 'builds bad', 'largest', 'of them', 'missing', 'wrong', 'cells of it', 'bad')
hdr2 = '%-34s %11s %8s %8s %9s %9s %11s %7s' % ('', '/ with it', 'bad kind', 'FAIL', '', '', '', 'share')
fmt = lambda k, o: '%-34s %5d /%4d %8d %8d %9d %9d %11d %6.1f%%' % (k, o[0], o[5], o[6], o[1], o[2], o[3], o[4], 100.0 * (o[2] + o[3]) / max(1, o[4]))
print(hdr); print(hdr2)
rows = [(o[0], k, o) for k, o in own.items() if o[0] >= 3]
rows.sort(key=lambda r: (-r[0], r[1]))
for _, k, o in rows[:TOP]: print(fmt(k, o))
print('  -- the same, by the share of the kind\'s cells that are bad (kinds bad in at least 3 builds)')
rows = [((o[2] + o[3]) / max(1, o[4]), k, o) for k, o in own.items() if o[0] >= 3]
rows.sort(reverse=True)
for _, k, o in rows[:30]: print(fmt(k, o))
print('  -- kinds the bot says it cannot place by hand (column 20), all builds: builds PASS / FAIL, cells')
cant = collections.defaultdict(lambda: [0, 0, 0])
for h in H:
    for m in re.finditer(r'cannot be placed by hand: (\w+)=(\d+)', builds[h][19]):
        c = cant[m.group(1)]; c[fail[h]] += 1; c[2] += int(m.group(2))
for k, c in sorted(cant.items(), key=lambda kv: -(kv[1][0] + kv[1][1])):
    print('%-34s %5d / %-5d %9d' % (k, c[0], c[1], c[2]))
print('  -- the other skip reasons: builds PASS / FAIL with the reason, cells')
why = collections.defaultdict(lambda: [0, 0, 0])
for h in H:
    for m in re.finditer(r'([^{},=:]+)=(\d+)', builds[h][19]):
        if 'cannot be placed by hand' in builds[h][19][max(0, m.start() - 27):m.start()]: continue
        c = why[m.group(1).strip()]; c[fail[h]] += 1; c[2] += int(m.group(2))
for k, c in sorted(why.items(), key=lambda kv: -(kv[1][0] + kv[1][1])):
    print('%-34s %5d / %-5d %9d' % (k, c[0], c[1], c[2]))

# ---- features
def feats(h):
    d = counts[h]; sx, sy, sz = d['_dims']; n = max(1, d['_blocks'])
    return {'height': sy, 'footprint (sx*sz)': sx * sz, 'density (blocks/volume)': n / (sx * sy * sz), 'share non-full-cube': d['_nonfull'] / n,
            'share gravity blocks': d['_gravity'] / n, 'share no-collision blocks': d['_nocollision'] / n, 'fluid cells in schematic': d['_fluid'],
            'waterlogged cells': d['_waterlogged'], 'distinct kinds': sum(1 for k in d if not k.startswith('_')), 'flat (height <= 3)': float(sy <= 3)}
F = {h: feats(h) for h in H}
fr = []
for name in F[H[0]]:
    hi = set()
    for i in range(nb):
        hs = [h for h in H if bnd[h] == i]
        if not hs: continue
        v = sorted(F[h][name] for h in hs); med = v[len(v) // 2]
        if med == v[0]: hi |= {h for h in hs if F[h][name] > med}      # mostly zero: "has any"
        else: hi |= {h for h in hs if F[h][name] >= med}
    r = line('high ' + name, hi)
    if r: fr.append(r)
for g in ('redstone parts (any)', 'containers', 'trapdoors'):
    for pct in (1, 5):
        r = line('%s >= %d%% of the blocks' % (g, pct), {h for h in H if gcount(h, GROUPS[g]) * 100 >= pct * max(1, counts[h]['_blocks'])})
        if r: fr.append(r)
for name, test in (('gravity blocks >= 100', lambda d: d['_gravity'] >= 100), ('fluid >= 100', lambda d: d['_fluid'] >= 100), ('fluid >= 1', lambda d: d['_fluid'] >= 1),
                   ('waterlogged >= 1', lambda d: d['_waterlogged'] >= 1), ('non-full share >= 50%', lambda d: d['_nonfull'] * 2 >= max(1, d['_blocks'])),
                   ('height >= 100', lambda d: d['_dims'][1] >= 100), ('height <= 10', lambda d: d['_dims'][1] <= 10)):
    r = line(name, {h for h in H if test(counts[h])})
    if r: fr.append(r)
fr.sort(reverse=True)
show(fr, 'FEATURES ("high" = at or above the median of the build\'s own size band; where the median is the minimum: above it)')

# ---- how the bot fares in the builds of a class: its skip reasons and counters (column 24, where the rows have it), builds of 10k+ blocks
def flags(h):
    r = builds[h]
    d = dict(kv.split('=')[:2] for kv in r[23].split(';') if '=' in kv) if len(r) > 23 else {}
    for m in re.finditer(r'([^{},=]+?)=(\d+)', r[19]):
        k = 'skip: ' + m.group(1).strip().split(':')[0]; d[k] = int(d.get(k, 0)) + int(m.group(2))
    d['out of time'] = int(r[2] == 'out of time'); d['done, but 3%+ of the cells bad (gave up)'] = int(r[2] == 'done' and bad_share(h) >= NEAR)
    d['FAIL at under 3% bad'] = int(fail[h] and r[2] == 'done' and bad_share(h) < NEAR)
    return d
BIG = [h for h in H if int(builds[h][3]) >= 10000]
FL = {h: flags(h) for h in BIG}
CLASSES = [('trapdoors', 10), ('redstone parts (any)', 100), ('containers', 10), ('vines', 1), ('stained glass (any colour)', 100)]
KEYS = [('out of time', 1), ('done, but 3%+ of the cells bad (gave up)', 1), ('FAIL at under 3% bad', 1), ('died', 1), ('unstuck', 1), ('shutEpisodes', 1), ('trapsAvoided', 50), ('stubs', 10), ('mined', 200),
        ('skip: stuck', 1), ('skip: stranded without material', 1), ('skip: occupied by another block', 1), ('skip: could not be set right', 10), ('skip: scaffold in the way', 10),
        ('skip: no way to place', 100), ('skip: out of stock', 1), ('skip: cannot be placed by hand', 1), ('skip: pot could not be filled', 1)]
print('\nHOW THE BOT FARES BY CLASS: share of the builds (10k+ blocks: %d, %d fail) with the flag, among builds WITH / WITHOUT the class' % (len(BIG), sum(fail[h] for h in BIG)))
print('%-46s %5s %9s  ' % ('flag (count at least ..)', 'n', 'fail') + ' '.join('%-13s' % ('%s>=%d' % (g.split(' ')[0], l)) for g, l in CLASSES))
for k, t in KEYS:
    fl = {h for h in BIG if float(FL[h].get(k, 0) or 0) >= t}
    if not fl: continue
    out = '%-46s %5d %4d %3.0f%%  ' % ('%s >= %d' % (k, t), len(fl), sum(fail[h] for h in fl), 100 * sum(fail[h] for h in fl) / len(fl))
    for g, l in CLASSES:
        w = [h for h in BIG if gcount(h, GROUPS[g]) >= l]; o = [h for h in BIG if gcount(h, GROUPS[g]) < l]
        out += ' %3.0f%% / %3.0f%% ' % (100 * sum(h in fl for h in w) / max(1, len(w)), 100 * sum(h in fl for h in o) / max(1, len(o)))
    print(out)

# ---- what the failing builds with the top kinds die of
print('\nWHAT THE FAILING BUILDS DIE OF (top %d kinds by excess; "own" = bad cells whose target is the kind itself, from column 19, which lists only the largest kinds)' % DETAIL)
seen = []
for x, name, n, f, e, z, txt, x2, z2 in res1:
    if len(seen) >= DETAIL: break
    seen.append(name)
    fb = [h for h in H if fail[h] and counts[h].get(name, 0) > 0]
    status = collections.Counter(builds[h][2] for h in fb)
    own_builds = 0; own_cells = 0; cells = 0; bad_total = 0; own_top = 0; reasons = collections.Counter(); named = 0; share = []
    for h in fb:
        r = builds[h]; bc = bad_cells(r)
        o = sum(c[3] for c in bc if c[0] != 'leftover' and c[1] == name)
        tot = int(r[7]) + int(r[8])
        own_cells += o; bad_total += tot; cells += counts[h][name]
        if o: own_builds += 1
        if bc and bc[0][1] == name and bc[0][0] != 'leftover': own_top += 1
        if tot: share.append(o / tot)
        if re.search(r'\b%s=' % re.escape(name), r[19]): named += 1
        for m in re.finditer(r'([^{},=]+?)(?:: [a-z_0-9]+)?=(\d+)', r[19]): reasons[m.group(1).strip()] += 1
    share.sort()
    print('\n%s: %d builds with it, %d fail (excess %+.1f, z %+.1f)' % (name, n, f, x, z))
    others = [h for h in H if fail[h] and h not in fb]
    print('  status of the failing: ' + ', '.join('%s %d' % kv for kv in status.most_common()) + '   (the %d failing builds without it: %s)' % (len(others), ', '.join('%s %d' % kv for kv in collections.Counter(builds[h][2] for h in others).most_common())))
    print('  own cells among the bad: in %d of %d failing builds (the largest bad kind in %d); %d own bad cells of %d bad cells in all (%.1f%%); median share per build %.1f%%; the kind is %.2f%% of these builds\' blocks'
          % (own_builds, len(fb), own_top, own_cells, bad_total, 100 * own_cells / max(1, bad_total), 100 * share[len(share) // 2] if share else 0, 100 * cells / max(1, sum(counts[h]['_blocks'] for h in fb))))
    print('  named in a skip reason (cannot be placed by hand ...) in %d failing builds; skip reasons by number of failing builds: %s' % (named, ', '.join('%s %d' % kv for kv in reasons.most_common(6))))
    for h in sorted(fb, key=lambda h: -int(builds[h][3]))[:4]:
        r = builds[h]
        print('    %s %s blocks=%s missing=%s wrong=%s %s=%d | %s | %s' % (h[:12], r[2], r[3], r[7], r[8], name, counts[h][name], r[18][:150], r[19][:110]))

# ---- a second opinion on what is a class of its own and what only stands for size / sparseness / detail: a logistic model
# of FAIL on (log size, its square, log density, share of non-full-cube blocks, log fluid cells), then each kind or group
# added alone. LR = twice the gain in log-likelihood (about z squared; 3.8 would be p = 0.05 for ONE test - hundreds are made).
try:
    import numpy as np
except ImportError:
    np = None
if np is not None:
    y = np.array([fail[h] for h in H], float)
    def base(h):
        d = counts[h]; sx, sy, sz = d['_dims']; n = max(1, d['_blocks']); ls = math.log10(n)
        return [1, ls, ls * ls, math.log10(n / (sx * sy * sz)), d['_nonfull'] / n, math.log10(1 + d['_fluid'])]
    X0 = np.array([base(h) for h in H])
    def fit(X):
        w = np.zeros(X.shape[1]); ridge = 1e-6 * np.eye(X.shape[1])
        for it in range(60):
            p = 1 / (1 + np.exp(-np.clip(X @ w, -30, 30)))
            w = w + np.linalg.solve(X.T @ (X * (p * (1 - p))[:, None]) + ridge, X.T @ (y - p))
        p = 1 / (1 + np.exp(-np.clip(X @ w, -30, 30)))
        return w, float(np.sum(y * np.log(p + 1e-12) + (1 - y) * np.log(1 - p + 1e-12)))
    w0, ll0 = fit(X0)
    print('\nLOGISTIC CHECK: FAIL ~ size, density, non-full share, fluid (coefficients %s), then each kind / group added alone' % ' '.join('%+.2f' % v for v in w0[3:]))
    print('%-40s %5s %5s %7s %7s' % ('kind / group', 'with', 'fail', 'coef', 'LR'))
    lr = []
    cands = [('%s >=%d' % (g, lim), {h for h in H if gcount(h, ks) >= lim}) for g, ks in GROUPS.items() for lim in (1, 10, 100)]
    cands += [('%s >=%d' % (k, lim), {h for h in H if counts[h].get(k, 0) >= lim}) for k in kinds for lim in (1, 10)]
    for name, ws in cands:
        if len(ws) < MIN or len(ws) > len(H) - MIN: continue
        x = np.array([h in ws for h in H], float)
        w, ll = fit(np.column_stack([X0, x]))
        lr.append((2 * (ll - ll0) * (1 if w[-1] > 0 else -1), name, len(ws), sum(fail[h] for h in ws), w[-1]))
    lr.sort(reverse=True)
    for v, name, n, f, c in lr[:TOP]:
        print('%-40s %5d %5d %+7.2f %+7.1f%s' % (name, n, f, c, v, '   (all fail or none: no estimate)' if abs(c) > 8 else ''))
