#!/usr/bin/env python3
"""Compare the structure of a translated Markdown file with its English source.
Called by doc-lang-check.sh; prints each difference, exit 1 if any.

Whole file: the same heading levels in the same order, and identical ``` blocks.
Per section (heading to heading, text before the H1 is skipped; it holds the
language line and the translation note):
  - the same images, inline code spans and list items (count); an image of a
    translation is in the language's folder (img/ru/x.png counts as img/x.png)
    and must exist;
  - the same tables: the rows of each and the columns of each row, in order;
  - the same links. An anchor counts as the number of the heading it reaches,
    so a link has to point at the matching section under its translated title.
    README.<lang>.md counts as README.md.
With --markup (languages without spaces between words): ** and * pairs that
CommonMark would not render, such as **LIVE?**后 (GitHub then shows the stars).
Usage: doc-lang-struct.py [--markup] EN.md TRANSLATED.md"""
import os, re, sys, unicodedata, urllib.parse

HEAD = re.compile(r'^(#{1,6})\s+(.*?)\s*#*\s*$')
FENCE = re.compile(r'^\s*(```|~~~)')
IMAGE = re.compile(r'!\[[^\]]*\]\(([^)\s]+)\)')
LINK = re.compile(r'(?<![0-9A-Za-z_\]!])\[(?:[^\]\[]|\[[^\]]*\])*\]\(([^)\s]+)(?:\s+"[^"]*")?\)')
CODE = re.compile(r'`[^`\n]+`')
ITEM = re.compile(r'^\s*(?:[-*+]|\d+\.)\s')
COMMENT = re.compile(r'<!--.*?-->', re.S)
LANG = re.compile(r'(^|/)README\.[a-z]{2}\.md$')
LANGS = ('ru', 'zh')


def slug(h):  # as linkcheck.py and GitHub
    h = re.sub(r'`', '', h.strip().lower())
    h = re.sub(r'\[([^\]]*)\]\([^)]*\)', r'\1', h)
    h = re.sub(r'[^\w\- ]', '', h)
    return h.replace(' ', '-')


def parse(path):
    """Sections [{title, lines}], fenced blocks, and {anchor: heading number}."""
    text = COMMENT.sub('', open(path, encoding='utf-8').read())
    sections, fences, anchors, seen = [], [], {}, {}
    cur, fence = None, None
    for line in text.split('\n'):
        if FENCE.match(line):
            if fence is None:
                fence = []
            else:
                fences.append('\n'.join(fence))
                fence = None
            continue
        if fence is not None:
            fence.append(line.strip())
            continue
        m = HEAD.match(line)
        if m:
            cur = {'level': len(m.group(1)), 'title': m.group(2), 'lines': []}
            sections.append(cur)
            s = slug(m.group(2))
            n = seen.get(s, 0)
            seen[s] = n + 1
            anchors[s if n == 0 else '%s-%d' % (s, n)] = len(sections) - 1
            continue
        if cur is not None:
            cur['lines'].append(line)
        for a in re.findall(r'<a\s+(?:name|id)="([^"]+)"', line):
            anchors[a] = len(sections)  # the heading that follows
    return sections, fences, anchors


def images(text, path, lang, bad, where):
    """The images in the form of the English file; missing files go to `bad`."""
    out = []
    for t in IMAGE.findall(text):
        if re.match(r'^[a-z]+:', t):
            out.append(t)
            continue
        if not os.path.exists(os.path.normpath(os.path.join(os.path.dirname(path), t))):
            bad.append('%s: %s: no such image' % (where, t))
        if lang and '/img/' in '/' + t and '/img/%s/' % lang not in '/' + t:
            bad.append('%s: %s: not in the img/%s/ folder' % (where, t, lang))
        out.append(re.sub(r'(^|/)img/(?:%s)/' % '|'.join(LANGS), r'\1img/', t))
    return out


def has_translation(path):
    base = path[:-len('.md')]
    return any(os.path.exists('%s.%s.md' % (base, l)) for l in LANGS)


def norm_link(t, path, anchors, cache):
    """A link target in a language-independent form."""
    if re.match(r'^[a-z]+:', t) or t.startswith('//'):
        return t
    p, _, frag = t.partition('#')
    frag = urllib.parse.unquote(frag)
    target = os.path.normpath(os.path.join(os.path.dirname(path), urllib.parse.unquote(p))) if p else path
    source = LANG.sub(lambda m: m.group(1) + 'README.md', target)
    if not frag:
        return source
    if target != path and not (LANG.search(target) or has_translation(target)):
        return source + '#' + frag  # an English-only document: the anchor stays
    if target == path:
        known = anchors
    else:
        if target not in cache:
            cache[target] = parse(target)[2] if os.path.exists(target) else {}
        known = cache[target]
    return source + ('#h%d' % known[frag] if frag in known else '#?' + frag)


def tables(lines):
    """The column count of each row, per table in the section."""
    out, rows = [], []
    for line in lines + ['']:
        t = CODE.sub('C', line.strip()).replace('\\|', 'P')
        if t.startswith('|'):
            rows.append(t.strip('|').count('|') + 1)
        elif rows:
            out.append(rows)
            rows = []
    return out


def counted(items):
    d = {}
    for i in items:
        d[i] = d.get(i, 0) + 1
    return d


def diff(a, b):
    ca, cb = counted(a), counted(b)
    return [(k, ca.get(k, 0), cb.get(k, 0)) for k in sorted(set(ca) | set(cb)) if ca.get(k, 0) != cb.get(k, 0)]


def is_punct(c):
    return unicodedata.category(c)[0] in 'PS'


def markup_problems(path):
    """Emphasis CommonMark would not close or open (flanking rules, spec 6.2)."""
    out, fence = [], False
    text = COMMENT.sub(lambda m: '\n' * m.group(0).count('\n'), open(path, encoding='utf-8').read())
    for no, line in enumerate(text.split('\n'), 1):
        if FENCE.match(line):
            fence = not fence
            continue
        if fence:
            continue
        s = CODE.sub('C', line)
        marks = [m for m in re.finditer(r'\*\*', s)]
        for i, m in enumerate(marks):
            before = s[m.start() - 1] if m.start() else ' '
            after = s[m.end()] if m.end() < len(s) else ' '
            opening = i % 2 == 0
            inner, outer = (after, before) if opening else (before, after)
            if inner.isspace():
                out.append('%s:%d: space inside ** at column %d' % (path, no, m.start() + 1))
            elif is_punct(inner) and not (outer.isspace() or is_punct(outer)):
                out.append('%s:%d: ** at column %d will not render (%r next to %r): add a space' %
                           (path, no, m.start() + 1, inner, outer))
        if len(marks) % 2:
            out.append('%s:%d: unpaired **' % (path, no))
    return out


def main():
    args = sys.argv[1:]
    markup = '--markup' in args
    args = [a for a in args if a != '--markup']
    en, tr = args
    m = LANG.search(tr)
    lang = m.group(0).split('.')[1] if m else None
    se, fe, ae = parse(en)
    st, ft, at = parse(tr)
    bad = []
    le, lt = [s['level'] for s in se], [s['level'] for s in st]
    if le != lt:
        n = next((i for i, (x, y) in enumerate(zip(le, lt)) if x != y), min(len(le), len(lt)))
        tit = lambda ss: ss[n]['title'] if n < len(ss) else '(end of file)'
        bad.append('headings differ at number %d: "%s" / "%s" (%d / %d headings)' %
                   (n + 1, tit(se), tit(st), len(le), len(lt)))
    if fe != ft:
        n = next((i for i, (x, y) in enumerate(zip(fe, ft)) if x != y), min(len(fe), len(ft)))
        bad.append('``` block number %d differs (%d / %d blocks); code stays as in English' % (n + 1, len(fe), len(ft)))
    cache = {}
    if le == lt:
        for a, b in zip(se, st):
            where = '"%s" / "%s"' % (a['title'], b['title'])
            ta, tb = '\n'.join(a['lines']), '\n'.join(b['lines'])
            for name, x, y in (
                ('image', images(ta, en, None, bad, where), images(tb, tr, lang, bad, where)),
                ('code', CODE.findall(ta), CODE.findall(tb)),
                ('link', [norm_link(t, en, ae, cache) for t in LINK.findall(CODE.sub('C', ta))],
                         [norm_link(t, tr, at, cache) for t in LINK.findall(CODE.sub('C', tb))]),
            ):
                for k, ca, cb in diff(x, y):
                    bad.append('%s: %s %s: %d / %d' % (where, name, k, ca, cb))
            ia = sum(1 for l in a['lines'] if ITEM.match(l))
            ib = sum(1 for l in b['lines'] if ITEM.match(l))
            if ia != ib:
                bad.append('%s: list items: %d / %d' % (where, ia, ib))
            if tables(a['lines']) != tables(b['lines']):
                bad.append('%s: tables (columns per row): %s / %s' % (where, tables(a['lines']), tables(b['lines'])))
    if markup:
        bad += markup_problems(tr)
    for b in bad:
        print('  ' + b, file=sys.stderr)
    return 1 if bad else 0


sys.exit(main())
