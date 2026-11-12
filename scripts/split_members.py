"""Helper for refactoring: lists the top-level members of a Java class (name, kind, lines)."""
import re, sys

def scan(src):
    """Yields (index, char, depth) skipping comments and literals; depth counts braces."""
    i, n, depth = 0, len(src), 0
    while i < n:
        c = src[i]
        if src.startswith('//', i):
            i = src.index('\n', i) if '\n' in src[i:] else n; continue
        if src.startswith('/*', i):
            i = src.index('*/', i) + 2; continue
        if c == '"':
            if src.startswith('"""', i):
                i = src.index('"""', i + 3) + 3; continue
            i += 1
            while src[i] != '"':
                i += 2 if src[i] == '\\' else 1
            i += 1; continue
        if c == "'":
            i += 1
            while src[i] != "'":
                i += 2 if src[i] == '\\' else 1
            i += 1; continue
        yield i, c, depth
        if c == '{': depth += 1
        elif c == '}': depth -= 1
        i += 1

def strip_comments(t):
    out = []
    last = 0
    for i, c, d in scan(t + '\n'):
        out.append(c)
    return ''.join(out)

def eol_comment(src, i):
    """End of a member: a // comment on the rest of the same line belongs to it."""
    m = re.compile(r'[ \t]*//[^\n]*').match(src, i)
    return m.end() if m else i

def members(src):
    """src = whole file. Returns (head, [member dicts], tail) for the first top-level class."""
    m = re.search(r'public final class Builder extends Body \{', src)
    body_start = m.end()
    res = []
    start = body_start
    par = 0
    state = None
    hdr_end = None
    for i, c, d in scan(src[body_start:]):
        gi = body_start + i
        if d == 0 and c == '}':
            class_end = gi
            break
        if d == 0:
            if c == '(': par += 1
            elif c == ')': par -= 1
            elif c in '{;=' and par == 0 and hdr_end is None:
                hdr_end = gi; state = c
            if c == ';' and state in (';', '='):
                e = eol_comment(src, gi + 1)
                res.append((start, e, state, hdr_end)); start = e; hdr_end = None; state = None
        elif d == 1 and c == '}' and state == '{':
            e = eol_comment(src, gi + 1)
            res.append((start, e, state, hdr_end)); start = e; hdr_end = None; state = None
    out = []
    for s, e, st, he in res:
        text = src[s:e]
        hdr = strip_comments(src[s:he]).strip()
        hdr = re.sub(r'@\w+(\([^)]*\))?\s*', '', hdr).strip()
        if st == '{':
            if re.search(r'\b(class|interface|enum|record)\b', hdr): kind, name = 'type', re.search(r'\b(?:class|interface|enum|record)\s+(\w+)', hdr).group(1)
            elif hdr == 'static': kind, name = 'init', '<static>'
            else:
                kind = 'method'; name = re.search(r'(\w+)\s*\(', hdr).group(1)
        else:
            kind = 'field'; name = re.findall(r'(\w+)\s*(?:\[\])*\s*$', hdr.split(',')[0].strip())
            name = name[0] if name else hdr
        out.append(dict(text=text, hdr=hdr, kind=kind, name=name, override='@Override' in src[s:he], static=bool(re.search(r'\bstatic\b', hdr))))
    return src[:body_start], out, src[class_end:]

if __name__ == '__main__':
    head, ms, tail = members(open(sys.argv[1]).read())
    for m in ms:
        print(f"{m['kind']:7} {m['name']:28} {m['text'].count(chr(10)):4}  {m['hdr'][:90]}")
