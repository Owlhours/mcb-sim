#!/usr/bin/env python3
"""Draws the second goal sample (owner's decision, 2026-11-21: FROM-MONITOR-newgoal.md).

usage: draw_goal2.py pool <pool.txt>                      fresh builds (height <= 380, on no list) -> pool.txt
       draw_goal2.py draw <seed> <kinds.txt> <out.txt>    filter and draw (kinds.txt: `mcsim kinds pool.txt` output)
       draw_goal2.py sub <seed> <kinds.txt> <out.txt> <lo>:<hi>:<n> ...   a dev list or decision set with the same
                                                          filters: n builds of lo <= blocks < hi each, from the builds
                                                          of kinds.txt still fresh now (none on any list or run since)

Fresh: on no list and never run. Lists: every .txt in a samples/ folder anywhere under /work (the helpers' trees
and worktrees too) and every tmp/2d9a193-*.txt whose lines all begin with a build hash (8 or 20 hex characters) -
dev lists, decision sets read or not, the old goal sample. Run: every hash that begins a row of any .tsv (and every
<hash>.row) under samples/, tmp/, helpers/, the worktrees and mega-jobs/. (Files of counts made only to draw lists,
e.g. tmp/hp/*count.txt, are not lists: those builds were never built.)
Filters: (1) no cell of any kind in REDSTONE below, anywhere in the schematic; (2) no cell that
build::impossible_cells marks (both parts of the rule, all classes): `imp=0`; (3) the schematic loads.
Size mix: the same number of builds in each size bucket as in samples/goal-corpus1000.txt (blocks: column 5 of
corpus/unique.tsv, the measure that defines "large"), drawn uniformly at random within the bucket.
"""
import os, random, re, sys

# The owner's list, exactly: redstone dust, torches, blocks, repeaters, comparators; pistons and sticky pistons,
# observers, droppers, dispensers, hoppers, TNT; levers, buttons, pressure plates, tripwire hooks and string,
# daylight detectors, note blocks, redstone lamps, targets; powered, detector and activator rails, sculk sensors,
# trapped chests, crafters. (Doors, trapdoors and fence gates are not redstone parts on their own.)
REDSTONE_EXACT = {
    'redstone_wire', 'redstone_torch', 'redstone_wall_torch', 'redstone_block', 'repeater', 'comparator',
    'piston', 'sticky_piston', 'piston_head', 'moving_piston', 'observer', 'dropper', 'dispenser', 'hopper', 'tnt',
    'lever', 'tripwire_hook', 'tripwire', 'daylight_detector', 'note_block', 'redstone_lamp', 'target',
    'powered_rail', 'detector_rail', 'activator_rail', 'sculk_sensor', 'calibrated_sculk_sensor', 'trapped_chest',
    'crafter',
}
REDSTONE_SUFFIX = ('_button', '_pressure_plate')   # every wood / stone / weighted kind


def is_redstone(kind):
    return kind in REDSTONE_EXACT or kind.endswith(REDSTONE_SUFFIX)


BUCKETS = [0, 50, 300, 1000, 3000, 6000, 10000, 30000, 100000, 300000, 10 ** 12]
HEX = re.compile(r'^[0-9a-f]{8}$|^[0-9a-f]{20}$')


def bucket(b):
    for k in range(len(BUCKETS) - 1):
        if BUCKETS[k] <= b < BUCKETS[k + 1]: return k


def name(k):
    lo, hi = BUCKETS[k], BUCKETS[k + 1]
    f = lambda v: f'{v // 1000}k' if v >= 1000 else str(v)
    return f'{f(lo)}+' if hi == 10 ** 12 else f'{f(lo)}-{f(hi)}'


def corpus():
    c = {}
    for l in open('/work/corpus/unique.tsv'):
        f = l.rstrip('\n').split('\t')
        if len(f) < 5 or not f[4].isdigit(): continue
        c[f[0]] = (int(f[2]), int(f[4]))
    return c


H20 = re.compile(r'^[0-9a-f]{20}')


def listed():
    full, short, files = set(), set(), 0
    roots = ['/work/samples', '/work/tmp', '/work/helpers', '/work/.claude/worktrees', '/work/mega-jobs']
    for r in roots:
        for d, _, fs in os.walk(r):
            if '/target' in d: continue
            for fn in fs:
                p = os.path.join(d, fn)
                if fn.endswith('.row') and H20.match(fn): full.add(fn[:20]); continue
                try: sz = os.path.getsize(p)
                except OSError: continue
                try:
                    if fn.endswith('.txt') and ('/samples' in d or fn.startswith('2d9a193-')) and sz < 5_000_000:
                        toks = [l.split()[0] for l in open(p, errors='replace') if l.strip()]
                        if not toks or not all(HEX.match(t) for t in toks): continue
                        files += 1
                        for t in toks: (full if len(t) == 20 else short).add(t)
                    elif fn.endswith('.tsv') and sz < 300_000_000:
                        files += 1
                        for l in open(p, errors='replace'):
                            m = H20.match(l)
                            if m and l[20:21] == '\t': full.add(m.group(0))
                except OSError: continue
    return full, short, files


def main():
    if sys.argv[1] == 'pool':
        c = corpus(); full, short, files = listed()
        pool = [h for h, (sy, b) in c.items() if sy <= 380 and h not in full and h[:8] not in short]
        print(f'{len(c)} in the corpus, {files} list and row files with {len(full) + len(short)} hashes, {len(pool)} fresh with height <= 380')
        with open(sys.argv[2], 'w') as o:
            for h in sorted(pool): o.write(h + '\n')
        return
    seed, kinds, out = int(sys.argv[2]), sys.argv[3], sys.argv[4]
    c = corpus()
    if sys.argv[1] == 'sub':
        full, short, _ = listed()
        ok = []
        for l in open(kinds):
            f = l.split()
            if f[0] != 'K' or f[1] in full or f[1][:8] in short or int(f[3].split('=')[1]) > 0: continue
            ks = f[4].split('=', 1)[1].split(',') if len(f) > 4 else []
            if not any(k and is_redstone(k) for k in ks): ok.append(f[1])
        ok.sort(); rng = random.Random(seed)
        with open(out, 'w') as o:
            for spec in sys.argv[5:]:
                lo, hi, n = map(int, spec.split(':'))
                pool = [h for h in ok if lo <= c[h][1] < hi]
                if len(pool) < n: sys.exit(f'{spec}: only {len(pool)}')
                for h in rng.sample(pool, n): o.write(h + '\n')
                print(spec, len(pool), 'pass the filters and are fresh;', n, 'drawn')
        return
    old = [0] * (len(BUCKETS) - 1)
    for l in open('/work/samples/goal-corpus1000.txt'):
        if l.strip(): old[bucket(c[l.split()[0]][1])] += 1
    rows = {}
    for l in open(kinds):
        f = l.split()
        if f[0] == 'X': rows[f[1]] = None; continue
        imp = int(f[3].split('=')[1]); ks = f[4].split('=', 1)[1].split(',') if len(f) > 4 else []
        rows[f[1]] = (imp, [k for k in ks if k and is_redstone(k)])
    tested = [[0] * 5 for _ in old]   # fresh, failed load, redstone, impossible (no redstone), pass
    ok = [[] for _ in old]
    for h, r in rows.items():
        k = bucket(c[h][1]); tested[k][0] += 1
        if r is None: tested[k][1] += 1
        elif r[1]: tested[k][2] += 1
        elif r[0] > 0: tested[k][3] += 1
        else: tested[k][4] += 1; ok[k].append(h)
    print(f'{"blocks":>10} {"fresh":>7} {"noload":>7} {"redstone":>9} {"impossible":>10} {"pass":>7} {"old":>5}')
    for k in range(len(old)):
        t = tested[k]; print(f'{name(k):>10} {t[0]:>7} {t[1]:>7} {t[2]:>9} {t[3]:>10} {t[4]:>7} {old[k]:>5}')
    tot = [sum(t[i] for t in tested) for i in range(5)]
    print(f'{"all":>10} {tot[0]:>7} {tot[1]:>7} {tot[2]:>9} {tot[3]:>10} {tot[4]:>7} {sum(old):>5}')
    rng = random.Random(seed)
    with open(out, 'w') as o:
        for k in range(len(old)):
            pool = sorted(ok[k])
            if len(pool) < old[k]: sys.exit(f'bucket {name(k)}: {len(pool)} pass the filters, {old[k]} needed')
            for h in rng.sample(pool, old[k]): o.write(h + '\n')
    print(f'drawn {sum(old)} with seed {seed} -> {out}')


main()
