#!/usr/bin/env python3
"""Compares simulator results with the Java reference results.
usage: compare.py <sim.tsv> [set ...] [--list N] [--md]
Sets default to all f6b21f4 files that share builds with the simulator output."""
import sys, glob, os, statistics as st
REF = '/work/reference/results'
def load(path):
    out = {}
    for l in open(path):
        f = l.rstrip('\n').split('\t')
        if len(f) < 17: continue
        out[f[0]] = f
    return out
def pct(v, q):
    v = sorted(v)
    if not v: return float('nan')
    k = (len(v) - 1) * q
    lo = int(k); hi = min(lo + 1, len(v) - 1)
    return v[lo] + (v[hi] - v[lo]) * (k - lo)
def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    nlist = 0
    if '--list' in sys.argv: nlist = int(sys.argv[sys.argv.index('--list') + 1]); args = [a for a in args if a != str(nlist)]
    sim = load(args[0])
    sets = args[1:] or [os.path.basename(p)[8:-4] for p in sorted(glob.glob(f'{REF}/f6b21f4-*.tsv'))]
    md = '--md' in sys.argv
    hdr = ['set', 'n', 'verdict=', 'exact', 'tick err med', 'p90', 'agg ticks', 'placed err med', 'fail builds m/w/l ref', 'sim', 'RTF med', 'p10', 'p90', 'agg']
    rows = []
    worst = []
    for s in sets:
        ref = load(f'{REF}/f6b21f4-{s}.tsv')
        common = [h for h in ref if h in sim]
        if not common: continue
        same = 0; exact = 0; errs = []; perr = []; rt = 0; stt = 0; rtf = []; wall = 0.0; ticks = 0
        fm = [0, 0, 0]; fs = [0, 0, 0]
        for h in common:
            r, q = ref[h], sim[h]
            if r[1] == q[1]: same += 1
            rtk, stk = int(r[4]), int(q[4])
            e = abs(stk - rtk) / max(1, rtk)
            errs.append(e); rt += rtk; stt += stk
            perr.append(abs(int(q[5]) - int(r[5])) / max(1, int(r[5])))
            if r[1:10] == q[1:10]: exact += 1
            if r[1] == 'FAIL' or q[1] == 'FAIL':
                for k in range(3): fm[k] += int(r[7 + k]); fs[k] += int(q[7 + k])
            w = max(float(q[16]), 0.5) / 1000.0
            rtf.append(stk / 20.0 / w); wall += w; ticks += stk
            worst.append((e, s, h, r, q))
        rows.append([s, len(common), f'{100*same/len(common):.0f}%', f'{exact}', f'{100*st.median(errs):.1f}%', f'{100*pct(errs,0.9):.0f}%', f'{100*(stt-rt)/max(1,rt):+.1f}%',
                     f'{100*st.median(perr):.1f}%', '/'.join(map(str, fm)), '/'.join(map(str, fs)), f'{st.median(rtf):.0f}x', f'{pct(rtf,0.1):.0f}x', f'{pct(rtf,0.9):.0f}x', f'{ticks/20/wall:.0f}x'])
    if md:
        print('| ' + ' | '.join(hdr) + ' |'); print('|' + '---|' * len(hdr))
        for r in rows: print('| ' + ' | '.join(map(str, r)) + ' |')
    else:
        w = [max(len(str(x)) for x in [h] + [r[i] for r in rows]) for i, h in enumerate(hdr)]
        print('  '.join(h.ljust(w[i]) for i, h in enumerate(hdr)))
        for r in rows: print('  '.join(str(x).ljust(w[i]) for i, x in enumerate(r)))
    if nlist:
        worst.sort(key=lambda x: -x[0])
        print('\nlargest tick differences (set hash | ref: verdict ticks placed skipped m/w/l trips | sim ...):')
        for e, s, h, r, q in worst[:nlist]:
            print(f'{s:12s} {h[:12]} {r[20] if len(r)>20 else "":>10s} blk={r[3]:>6s} | {r[1]} {r[4]:>8s} {r[5]:>6s} {r[6]:>4s} {r[7]}/{r[8]}/{r[9]} t{r[15]} sc{r[13]} | {q[1]} {q[4]:>8s} {q[5]:>6s} {q[6]:>4s} {q[7]}/{q[8]}/{q[9]} t{q[15]} sc{q[13]}  {100*e:.0f}%')
main()
