#!/usr/bin/env python3
"""Bookkeeping for porting the Java bot to Rust.

The Java source the Rust port corresponds to is kept in port/java (a snapshot, with its commit in port/java/COMMIT).
port/map.tsv says where each Java class lives in Rust and lists the exceptions to the naming rule
(Java `Class.camelCase(...)` is Rust `fn camel_case` in the class's file).

  port.py check                    every Java method of the snapshot has a Rust function or a stated reason not to
  port.py status NEWSRC            what a new Java tree changes against the snapshot: one line per member, by Rust file
  port.py diff NEWSRC [PATTERN]    the same with the Java diff of every member (PATTERN: substring of Class.member)
  port.py done NEWSRC MEMBER...    mark members as ported (they leave `status`; a member that changes again comes back)
  port.py finish NEWSRC COMMIT     the port is complete: NEWSRC becomes the snapshot
  port.py where PATTERN            Rust location of Java members
  port.py map                      the whole table, Java member -> Rust location

NEWSRC is a directory that contains `mcb/` (e.g. reference/java-a/asrc/src).
"""
import difflib, hashlib, os, re, shutil, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SNAP = os.path.join(ROOT, 'port', 'java')
MAP = os.path.join(ROOT, 'port', 'map.tsv')
PROGRESS = os.path.join(ROOT, 'port', 'progress.tsv')
RUST = os.path.join(ROOT, 'sim', 'src')


# ------------------------------------------------------------------ Java source -> members

def masks(src):
    """Two copies of the text with the same offsets: `code` has comments blanked, `bare` has string and char
    literals blanked as well (for finding braces and semicolons)."""
    code, bare = list(src), list(src)
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == '/' and i + 1 < n and src[i + 1] == '/':
            j = src.find('\n', i)
            j = n if j < 0 else j
            for k in range(i, j): code[k] = bare[k] = ' '
            i = j
        elif c == '/' and i + 1 < n and src[i + 1] == '*':
            j = src.find('*/', i + 2)
            j = n if j < 0 else j + 2
            for k in range(i, j):
                if src[k] != '\n': code[k] = bare[k] = ' '
            i = j
        elif c == '"' and src.startswith('"""', i):
            j = src.find('"""', i + 3)
            j = n if j < 0 else j + 3
            for k in range(i + 1, j - 1):
                if src[k] != '\n': bare[k] = ' '
            i = j
        elif c == '"' or c == "'":
            j = i + 1
            while j < n and src[j] != c:
                j += 2 if src[j] == '\\' else 1
            for k in range(i + 1, min(j, n)): bare[k] = ' '
            i = j + 1
        else:
            i += 1
    return ''.join(code), ''.join(bare)


TYPE_RE = re.compile(r'\b(class|interface|enum|record)\s+(\w+)')
ANNOT_RE = re.compile(r'@\w+(\s*\([^()]*\))?')


class Member:
    __slots__ = ('key', 'kind', 'text', 'norm', 'file', 'line')

    def __init__(self, key, kind, text, norm, file, line):
        self.key, self.kind, self.text, self.norm, self.file, self.line = key, kind, text, norm, file, line


def param_types(args):
    out, depth, cur = [], 0, ''
    for ch in args + ',':
        if ch in '<([': depth += 1
        elif ch in '>)]': depth -= 1
        if ch == ',' and depth == 0:
            cur = ANNOT_RE.sub('', cur).strip()
            if cur:
                toks = cur.replace('final ', '').split()
                ty = ' '.join(toks[:-1]) if len(toks) > 1 else toks[0]
                out.append(re.sub(r'<.*>', '', ty).split('.')[-1])
            cur = ''
        else:
            cur += ch
    return out


def members_of(src, file):
    code, bare = masks(src)
    out = []

    def body(start, end, prefix):
        i = start
        mstart = start
        depth = paren = 0
        saw_eq = False
        block = False
        counters = {}
        while i < end:
            ch = bare[i]
            done = False
            if ch == '(': paren += 1
            elif ch == ')': paren -= 1
            elif ch == '=' and depth == 0 and paren == 0: saw_eq = True
            elif ch == '{':
                if depth == 0 and paren == 0 and not saw_eq: block = True
                depth += 1
            elif ch == '}':
                depth -= 1
                if depth == 0 and block: done = True
            elif ch == ';' and depth == 0 and paren == 0: done = True
            i += 1
            if not done: continue
            emit(mstart, i, prefix, counters)
            mstart = i
            saw_eq = block = False

    def emit(a, b, prefix, counters):
        # leading blank space and comments belong to the member, but the header is read from the code
        cs = a
        while cs < b and bare[cs].isspace(): cs += 1
        if cs >= b: return
        seg = bare[cs:b]
        # the header: up to the first `{`, `;` or `=` outside parentheses (annotations may hold any of them)
        def first(stops):
            d = 0
            for k, ch in enumerate(seg):
                if ch == '(': d += 1
                elif ch == ')': d -= 1
                elif d == 0 and ch in stops: return k
            return len(seg)
        hend, bend = first('{;='), first('{;')
        blank = lambda m: ' ' * len(m.group(0))
        seg = ANNOT_RE.sub(blank, seg[:bend]) + seg[bend:]
        header, full_header = seg[:hend], seg[:bend]
        has_block = bend < len(seg) and seg[bend] == '{'
        text = src[a:b].strip('\n')
        norm = ' '.join(code[cs:b].split())
        line = src.count('\n', 0, cs) + 1
        if re.match(r'\s*(package|import)\b', header): return
        tm = TYPE_RE.search(full_header)
        is_method = '(' in header and (has_block or '=' not in full_header)
        if tm and not ('(' in full_header[:tm.start()]):
            name = tm.group(2)
            open_ = cs + bend
            # the type's own header counts as a member (extends / implements changes)
            out.append(Member(prefix + name + '.<type>', 'type', src[cs:open_].strip(), ' '.join(code[cs:open_].split()), file, line))
            body(open_ + 1, b - 1, prefix + name + '.')
        elif is_method:
            p = header.find('(')
            name = re.findall(r'\w+', header[:p])[-1]
            q, d = p + 1, 1
            while q < len(seg) and d > 0:
                d += seg[q] == '('
                d -= seg[q] == ')'
                q += 1
            key = prefix + name + '(' + ','.join(param_types(seg[p + 1:q - 1])) + ')'
            out.append(Member(key, 'method' if has_block else 'decl', text, norm, file, line))
        elif header.strip() in ('', 'static'):
            k = '<clinit>' if header.strip() else '<init>'
            counters[k] = counters.get(k, 0) + 1
            out.append(Member(prefix + k + str(counters[k]), 'init', text, norm, file, line))
        else:
            names = re.findall(r'\w+', re.sub(r'<[^<>]*>|\[[^\]]*\]', ' ', header))
            if not names: return
            out.append(Member(prefix + names[-1], 'field', text, norm, file, line))

    body(0, len(src), '')
    return out


def tree(src_dir):
    """All members of all Java files under src_dir/mcb, keyed by Class.member."""
    out = {}
    base = os.path.join(src_dir, 'mcb')
    if not os.path.isdir(base): sys.exit(f'{src_dir}: no mcb/ directory')
    for d, _, files in sorted(os.walk(base)):
        for f in sorted(files):
            if not f.endswith('.java'): continue
            path = os.path.join(d, f)
            rel = os.path.relpath(path, src_dir)
            with open(path, encoding='utf-8') as fh: src = fh.read()
            for m in members_of(src, rel):
                k = m.key
                n = 2
                while k in out:      # (a second field declaration that ends in the same name, say)
                    k = f'{m.key}#{n}'; n += 1
                m.key = k
                out[k] = m
    return out


# ------------------------------------------------------------------ the map and the Rust side

class Map:
    def __init__(self):
        self.files = {}      # Java class -> [rust files]
        self.ignore = []     # path prefixes (under src) that are not part of the port
        self.loose = set()   # classes that are not ported function by function
        self.entry = {}      # Class.member -> (rust or '-', note)
        if not os.path.exists(MAP): return
        for ln in open(MAP, encoding='utf-8'):
            ln = ln.rstrip('\n')
            if not ln.strip() or ln.startswith('#'): continue
            p = ln.split('\t')
            if p[0] == '@class': self.files[p[1]] = p[2].split(',')
            elif p[0] == '@ignore': self.ignore.append(p[1])
            elif p[0] == '@loose': self.loose.add(p[1])
            else: self.entry[p[0]] = (p[1] if len(p) > 1 else '', p[2] if len(p) > 2 else '')

    def ignored(self, m):
        return any(m.file.startswith(p) for p in self.ignore)


def snake(name):
    return re.sub(r'(?<=[a-z0-9])([A-Z])', r'_\1', name).lower()


_rust = None


def rust_index():
    """fn name -> [(file, line)], and for every file its text (for field hints)."""
    global _rust
    if _rust is not None: return _rust
    fns, texts = {}, {}
    for d, _, files in sorted(os.walk(RUST)):
        for f in sorted(files):
            if not f.endswith('.rs'): continue
            path = os.path.join(d, f)
            rel = os.path.relpath(path, ROOT)
            lines = open(path, encoding='utf-8').read().split('\n')
            texts[rel] = lines
            for n, ln in enumerate(lines, 1):
                m = re.match(r'\s*(?:pub(?:\([a-z]+\))?\s+)?(?:const\s+)?(?:unsafe\s+)?fn\s+(\w+)', ln)
                if m: fns.setdefault(m.group(1), []).append((rel, n))
    _rust = (fns, texts)
    return _rust


def locate(mp, m):
    """(location text or None, note, explicit?) for a Java member."""
    fns, texts = rust_index()
    cls = m.key.split('.')[0]
    files = mp.files.get(cls, [])
    e = mp.entry.get(m.key) or mp.entry.get(re.sub(r'\(.*', '', m.key))
    note = e[1] if e else ''
    if e and e[0] == '-': return ('-', note, True)
    if e and e[0]:
        path, _, fn = e[0].partition('::')
        if not fn: return (path, note, True)
        for (f, n) in fns.get(fn, []):
            if f == path: return (f'{f}:{n} fn {fn}', note, True)
        return (None, f'map.tsv names {e[0]}, which does not exist', True)
    name = re.sub(r'\(.*', '', m.key.split('.')[-1])
    if m.kind == 'method':
        # (fooImpl is the body of the wrapper foo, which the port has as one function)
        for nm in [snake(name)] + ([snake(name[:-4])] if name.endswith('Impl') else []):
            for (f, n) in fns.get(nm, []):
                if f in files: return (f'{f}:{n} fn {nm}', note, False)
        if cls in mp.loose and files: return (files[0], note, False)
        return (None, note, False)
    # fields, types, initialisers: a hint only - the first line in the class's files that names it
    pat = re.compile(r'\b' + re.escape(snake(name)) + r'\b', re.I)
    for f in files:
        for n, ln in enumerate(texts.get(f, []), 1):
            if pat.search(ln): return (f'{f}:{n}', note, False)
    return (files[0] if files else None, note, False)


# ------------------------------------------------------------------ commands

def load_progress():
    out = {}
    if os.path.exists(PROGRESS):
        for ln in open(PROGRESS, encoding='utf-8'):
            p = ln.rstrip('\n').split('\t')
            if len(p) == 2: out[p[0]] = p[1]
    return out


def digest(m):
    return hashlib.sha1(m.norm.encode()).hexdigest()[:16] if m else 'gone'


def changes(new_dir):
    """[(key, old member or None, new member or None, kind of change)] for everything that differs."""
    mp = Map()
    old, new = tree(SNAP), tree(new_dir)
    out = []
    for k in sorted(set(old) | set(new), key=lambda k: ((new.get(k) or old.get(k)).file, (new.get(k) or old.get(k)).line)):
        a, b = old.get(k), new.get(k)
        if mp.ignored(a or b) or (a or b).kind == 'decl': continue
        if a is None: out.append((k, a, b, 'NEW'))
        elif b is None: out.append((k, a, b, 'GONE'))
        elif a.norm != b.norm: out.append((k, a, b, 'CHANGED'))
        elif a.text != b.text: out.append((k, a, b, 'comment'))
    return mp, out


def show_status(new_dir, pattern=None, with_diff=False):
    mp, ch = changes(new_dir)
    prog = load_progress()
    by_file = {}
    left = ported = comments = 0
    for (k, a, b, what) in ch:
        if pattern and pattern not in k: continue
        if what == 'comment':
            comments += 1
            if not with_diff or not pattern: continue
        if prog.get(k) == digest(b):
            ported += 1
            if not pattern: continue
        else:
            left += what != 'comment'
        loc, note, _ = locate(mp, b or a)
        if loc is None and what == 'NEW':
            files = mp.files.get(k.split('.')[0], [])
            if files: loc = files[0] + ' (to add)'
        rf = (loc or '?').split(':')[0].split(' ')[0]
        by_file.setdefault(rf, []).append((k, a, b, what, loc, note))
    for rf in sorted(by_file):
        print(f'== {rf}')
        for (k, a, b, what, loc, note) in by_file[rf]:
            m = b or a
            where = 'not ported' if loc == '-' else (loc or 'NO RUST COUNTERPART YET')
            print(f'  {what:8s}{k}   [{m.file}:{m.line}]  ->  {where}' + (f'   ({note})' if note else ''))
            if with_diff:
                al = a.text.split('\n') if a else []
                bl = b.text.split('\n') if b else []
                for ln in difflib.unified_diff(al, bl, 'snapshot', 'new', n=2, lineterm=''):
                    if ln.startswith(('---', '+++')): continue
                    print('      ' + ln)
    print(f'{left} members to port, {ported} marked done, {comments} with changed comments only'
          + ('' if with_diff else ' (port.py diff shows the Java changes)'))
    return left


def main():
    a = sys.argv[1:]
    if not a: sys.exit(__doc__)
    cmd = a[0]
    if cmd == 'check':
        mp = Map()
        bad = 0
        n = 0
        for k, m in tree(SNAP).items():
            if mp.ignored(m) or m.kind != 'method': continue
            n += 1
            loc, note, _ = locate(mp, m)
            if loc is None:
                bad += 1
                print(f'no Rust function for {k}  [{m.file}:{m.line}]' + (f'  ({note})' if note else ''))
        for k in mp.entry:
            if k not in tree(SNAP) and not any(x.startswith(k + '(') for x in tree(SNAP)):
                print(f'map.tsv: {k} is not in the snapshot (stale entry)')
                bad += 1
        print(f'{n} Java methods, {bad} problems')
        sys.exit(1 if bad else 0)
    elif cmd in ('status', 'diff'):
        show_status(a[1], a[2] if len(a) > 2 else None, cmd == 'diff')
    elif cmd == 'done':
        mp, ch = changes(a[1])
        prog = load_progress()
        keys = {k: b for (k, _, b, what) in ch if what != 'comment'}
        for pat in a[2:]:
            hit = [k for k in keys if k == pat or re.sub(r'\(.*', '', k) == pat or (pat.endswith('.') and k.startswith(pat))]
            if not hit: print(f'{pat}: no changed member of that name (Class.member, or `Class.` for all of a class)')
            for k in hit: prog[k] = digest(keys[k])
        with open(PROGRESS, 'w', encoding='utf-8') as fh:
            for k in sorted(prog): fh.write(f'{k}\t{prog[k]}\n')
        show_status(a[1])
    elif cmd == 'finish':
        left = show_status(a[1])
        if left and '--force' not in a: sys.exit('not finished: the members above are not marked done (--force to override)')
        shutil.rmtree(os.path.join(SNAP, 'mcb'), ignore_errors=True)
        shutil.copytree(os.path.join(a[1], 'mcb'), os.path.join(SNAP, 'mcb'))
        with open(os.path.join(SNAP, 'COMMIT'), 'w') as fh: fh.write(a[2] + '\n')
        if os.path.exists(PROGRESS): os.remove(PROGRESS)
        print(f'snapshot is now {a[2]}')
    elif cmd in ('where', 'map'):
        mp = Map()
        pat = a[1] if cmd == 'where' else ''
        for k, m in tree(SNAP).items():
            if mp.ignored(m) or pat.lower() not in k.lower(): continue
            if cmd == 'map' and m.kind != 'method': continue
            loc, note, _ = locate(mp, m)
            print(f'{k}\t{"not ported" if loc == "-" else (loc or "?")}' + (f'\t{note}' if note else ''))
    else:
        sys.exit(__doc__)


if __name__ == '__main__':
    main()
