#!/usr/bin/env python3
"""jfrtop.py <jfr print output> [root-frame-substring]: self and inclusive time per method from ExecutionSample events,
optionally only for samples whose stack contains the given frame."""
import sys, re, collections
txt = open(sys.argv[1]).read()
root = sys.argv[2] if len(sys.argv) > 2 else None
top = collections.Counter(); incl = collections.Counter(); n = 0
for ev in txt.split('jdk.ExecutionSample {')[1:]:
    m = re.search(r'stackTrace = \[(.*?)\n  \]', ev, re.S)
    if not m or 'sampledThread = "main"' not in ev: continue
    frames = [re.sub(r'\(.*', '', l.strip()) for l in m.group(1).strip().split('\n') if l.strip() and l.strip() != '...']
    if root:
        idx = [i for i, f in enumerate(frames) if root in f]
        if not idx: continue
        frames = frames[:idx[-1] + 1]
    n += 1
    top[frames[0]] += 1
    for f in set(frames): incl[f] += 1
print('samples', n)
print('--- self');   [print(f'{100*v/n:5.1f}% {k}') for k, v in top.most_common(18)]
print('--- inclusive'); [print(f'{100*v/n:5.1f}% {k}') for k, v in incl.most_common(int(sys.argv[3]) if len(sys.argv) > 3 else 45)]
