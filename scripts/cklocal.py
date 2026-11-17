#!/usr/bin/env python3
"""Local tests of a fix from checkpoints just before each failure (the inner loop for large-build fixes).

  cklocal.py points <causes dir> <dir> [--max-per-build 3] [--min-cells 3]
      From the MCSIM_CAUSES files of failing builds (scripts/causes.py): the places where things go wrong. A point is
      a group of bad cells of one cause in failed sections of one build whose first trouble lies close together in
      time: T = 2,400 ticks before the first of them, STOP = 6,000 ticks after the last (clean-up causes: the end of
      the build). Writes <dir>/points.tsv and <dir>/cells/<id>.txt. Builds that ran out of time or stopped placing
      for lack of time get no point (their trouble is pace, not a place).
  cklocal.py save <dir> [-j 6] [BIN=...]
      The base: runs each point's build to STOP with a checkpoint at T (<dir>/ck/<id>.ckpt) and notes which of the
      point's cells are bad at STOP (<dir>/base/). One run per point; do this once per tree.
  cklocal.py run <dir> <tag> [-j 6] [BIN=...] [VAR=value ...]
      A candidate: resumes every checkpoint with the variables set on top (switches gated on crate::ckpt::cand(...) or
      ckpt::armed(...) act from T on; the state at T is checked against the checkpoint's fingerprint) and notes the
      same at STOP (<dir>/<tag>/).
  cklocal.py report <dir> <tag> [<base tag, default base>]
      Per point and in all: the point's cells bad at STOP, base against candidate; cells standing right in the whole
      build at STOP (so that a local win bought elsewhere shows); fingerprint failures.

A candidate that wins here goes to the full gate (docs/STATUS.md, "How changes are judged"); note there how the local
result compared with the full run.
"""
import sys, os, glob, collections, subprocess, re
from concurrent.futures import ThreadPoolExecutor

BIN_DEFAULT = '/work/sim-rs/sim/target/release/mcsim'
CLEANUP = ('clean-up', 'left standing', 'scaffolding stands')
NOPOINT = ('out of time', 'wall limit', 'died', 'crash', 'time ran short')

def read_causes(path):
    """-> (head, [(section, kind, x, y, z, cause, tick)])"""
    head = ''; sec = None; out = []
    for l in open(path):
        l = l.rstrip('\n')
        if l.startswith('B '): head = l[2:]
        elif l.startswith('S '): sec = ' '.join(l.split()[1:4])
        elif l.startswith('C '):
            f = l.split(' | ')
            p = f[0].split()
            t = int(f[3][2:]) if len(f) > 3 and f[3].startswith('t=') else -1
            out.append((sec, p[1], int(p[2]), int(p[3]), int(p[4]), f[1].strip(), t))
    return head, out

def cmd_points(a):
    src, dst = a[0], a[1]
    maxpb = int(a[a.index('--max-per-build') + 1]) if '--max-per-build' in a else 3
    minc = int(a[a.index('--min-cells') + 1]) if '--min-cells' in a else 3
    os.makedirs(dst + '/cells', exist_ok=True)
    rows = []; skipped = collections.Counter()
    for f in sorted(glob.glob(src + '/*.causes')):
        h = os.path.basename(f)[:-7]
        head, cells = read_causes(f)
        if not cells: continue
        m = re.search(r'ticks=(\d+)', head); end = int(m.group(1)) if m else 0
        by = collections.defaultdict(list)
        for c in cells:
            if c[5].startswith(NOPOINT) or 'before its turn' in c[5]: skipped[c[5].split(':')[0]] += 1; continue
            if c[6] < 0: skipped['no tick (never tried)'] += 1; continue
            by[c[5]].append(c)
        pts = []
        for cause, cs in by.items():
            cs.sort(key=lambda c: c[6])
            grp = [cs[0]]
            for c in cs[1:] + [None]:
                if c is not None and c[6] - grp[-1][6] <= 20000: grp.append(c); continue
                if len(grp) >= minc: pts.append((cause, grp))
                grp = [c]
        pts.sort(key=lambda p: -len(p[1]))
        for cause, grp in pts[:maxpb]:
            t = max(200, (grp[0][6] - 2400) // 200 * 200)
            stop = 0 if cause.startswith(CLEANUP) else min(end, grp[-1][6] + 6000)
            pid = f'{h[:8]}.{t}'
            n = 2
            while any(r[0] == pid for r in rows): pid = f'{h[:8]}.{t}.{n}'; n += 1
            open(f'{dst}/cells/{pid}.txt', 'w').write(''.join(f'{c[2]} {c[3]} {c[4]} {c[1]}\n' for c in grp))
            rows.append((pid, h, t, stop, len(grp), cause))
    with open(dst + '/points.tsv', 'w') as o:
        o.write('id\thash\tT\tstop\tcells\tcause\n')
        for r in rows: o.write('\t'.join(str(x) for x in r) + '\n')
    print(f'{len(rows)} points in {len(set(r[1] for r in rows))} builds, {sum(r[4] for r in rows)} cells -> {dst}/points.tsv')
    by = collections.Counter(); byc = collections.Counter()
    for r in rows: by[r[5]] += 1; byc[r[5]] += r[4]
    for c, n in by.most_common(): print(f'  {n:3} points {byc[c]:6} cells  {c}')
    if skipped: print('  no point for: ' + ', '.join(f'{k} ({v} cells)' for k, v in skipped.most_common()))

def points(d):
    rows = [l.rstrip('\n').split('\t') for l in open(d + '/points.tsv')][1:]
    return [dict(id=r[0], hash=r[1], T=int(r[2]), stop=int(r[3]), cells=int(r[4]), cause=r[5]) for r in rows]

def split_args(a):
    j = 6; binp = BIN_DEFAULT; env = {}; rest = []
    i = 0
    while i < len(a):
        if a[i] == '-j': j = int(a[i + 1]); i += 2; continue
        if '=' in a[i] and not a[i].startswith('/'):
            k, v = a[i].split('=', 1)
            if k == 'BIN': binp = v
            else: env[k] = v
        else: rest.append(a[i])
        i += 1
    return j, binp, env, rest

def one(cmd, env, log):
    e = dict(os.environ); e.update(env)
    with open(log, 'w') as lf: return subprocess.run(cmd, env=e, stdout=lf, stderr=subprocess.STDOUT).returncode

def cmd_save(a):
    j, binp, env, rest = split_args(a); d = rest[0]
    os.makedirs(d + '/ck', exist_ok=True); os.makedirs(d + '/base', exist_ok=True)
    mybin = d + '/mcsim-base'; subprocess.run(['cp', binp, mybin], check=True)
    def go(p):
        cd = f'{d}/base/{p["id"]}'; os.makedirs(cd, exist_ok=True)
        cmd = [mybin, 'run', p['hash'], f'{cd}/row.tsv', '--checkpoint-at', str(p['T']), '--save', f'{d}/ck/{p["id"]}.ckpt']
        if p['stop'] > 0: cmd += ['--stop-at', str(p['stop'])]
        return p['id'], one(cmd, dict(env, MCSIM_CAUSES=cd), f'{cd}/log')
    with ThreadPoolExecutor(j) as ex: res = list(ex.map(go, points(d)))
    ok = sum(os.path.exists(f'{d}/ck/{i}.ckpt') for i, _ in res)
    print(f'{ok} of {len(res)} checkpoints saved in {d}/ck')

def cmd_run(a):
    j, binp, env, rest = split_args(a); d, tag = rest[0], rest[1]
    os.makedirs(f'{d}/{tag}', exist_ok=True)
    mybin = f'{d}/{tag}/mcsim'; subprocess.run(['cp', binp, mybin], check=True)
    open(f'{d}/{tag}/env.txt', 'w').write(' '.join(f'{k}={v}' for k, v in env.items()) + '\n')
    def go(p):
        ck = f'{d}/ck/{p["id"]}.ckpt'
        if not os.path.exists(ck): return p['id'], -1
        cd = f'{d}/{tag}/{p["id"]}'; os.makedirs(cd, exist_ok=True)
        cmd = [mybin, 'resume', ck, f'{cd}/row.tsv']
        return p['id'], one(cmd, dict(env, MCSIM_CAUSES=cd), f'{cd}/log')
    with ThreadPoolExecutor(j) as ex: res = list(ex.map(go, points(d)))
    bad = [i for i, rc in res if rc != 0]
    print(f'{len(res) - len(bad)} of {len(res)} points ran' + (f'; FAILED (state at the checkpoint differs, or no checkpoint): {" ".join(bad)}' if bad else ''))

def local(d, tag, p):
    """-> (the point's cells still bad, cells standing right in the whole build [row column 'placed right' unknown: bad cells], ok)"""
    cd = f'{d}/{tag}/{p["id"]}'
    fs = glob.glob(cd + '/*.causes')
    if not fs: return None
    _, cells = read_causes(fs[0])
    badset = {(c[2], c[3], c[4]) for c in cells}
    want = [tuple(int(v) for v in l.split()[:3]) for l in open(f'{d}/cells/{p["id"]}.txt')]
    row = open(cd + '/row.tsv').read().split('\t') if os.path.exists(cd + '/row.tsv') else []
    # row: 7 missing, 8 wrong, 9 leftover (the whole build at STOP)
    tot = sum(int(row[k]) for k in (7, 8, 9)) if len(row) > 9 else -1
    log = open(cd + '/log').read() if os.path.exists(cd + '/log') else ''
    return sum(1 for w in want if w in badset), tot, 'DIFFERS from the checkpoint' not in log

def cmd_report(a):
    d, tag = a[0], a[1]; base = a[2] if len(a) > 2 else 'base'
    S = collections.defaultdict(lambda: [0, 0, 0, 0, 0, 0, 0])   # points, cells, bad base, bad cand, better, worse, all-bad delta
    print(f'{"point":22} {"cells":>6} {"bad " + base:>10} {"bad " + tag:>10} {"all bad":>16}  cause')
    for p in points(d):
        A = local(d, base, p); B = local(d, tag, p)
        if A is None or B is None: print(f'{p["id"]:22} (missing)'); continue
        flag = '' if B[2] else '  STATE DIFFERS AT T'
        print(f'{p["id"]:22} {p["cells"]:6} {A[0]:10} {B[0]:10} {A[1]:7} -> {B[1]:6}  {p["cause"]}{flag}')
        for k in (p['cause'], 'ALL'):
            s = S[k]; s[0] += 1; s[1] += p['cells']; s[2] += A[0]; s[3] += B[0]; s[4] += B[0] < A[0]; s[5] += B[0] > A[0]; s[6] += B[1] - A[1]
    print()
    print(f'{"cause":58} {"points":>6} {"cells":>6} {"bad " + base:>10} {"bad " + tag:>10} {"better":>6} {"worse":>5} {"all bad +/-":>11}')
    for k, s in sorted(S.items(), key=lambda kv: (kv[0] == 'ALL', -kv[1][1])):
        print(f'{k[:58]:58} {s[0]:6} {s[1]:6} {s[2]:10} {s[3]:10} {s[4]:6} {s[5]:5} {s[6]:+11}')

a = sys.argv[1:]
if not a or a[0] not in ('points', 'save', 'run', 'report'): print(__doc__); sys.exit(0)
{'points': cmd_points, 'save': cmd_save, 'run': cmd_run, 'report': cmd_report}[a[0]](a[1:])
