#!/usr/bin/env python3
"""The Russian and Chinese manuals and READMEs show the localized screens
(docs/manual/img/<lang>/), so their on-screen labels follow i18n/<lang>.tsv:
**label**, **A > B > C** and table cells that are exactly a label become the
localized text, and image paths move to the language's folder. Idempotent; run
it after the manual changes.
    tools/manual-labels.py [ru zh]"""
import os
import re
import sys

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '../..'))
IMG = re.compile(r'(!\[[^\]]*\]\((?:[^)\s]*/)?img/)(?!(?:ru|zh)/)')


def table(lang):
    t = {}
    for l in open(os.path.join(ROOT, 'firmware/i18n/%s.tsv' % lang), encoding='utf-8'):
        if l.strip() and l[0] != '#':
            k, z, s = l.rstrip('\n').split('\t')
            k, z = (x.replace('\\u{2103}', '℃').replace('\\u{2109}', '℉').replace('\\u{3a9}', 'Ω') for x in (k, z))
            if k != z and s != 'E':
                t[k] = z
    return t


# A label that another font on the screen keeps in English (i18n/README.md).
AFTER = {'zh': [('绿色 **确认**', '绿色 **OK**')]}


def one(t, p):
    if p in t:
        return t[p]
    if p + ':' in t:  # "Roll" for the row "Roll:"
        return t[p + ':'].rstrip(':').rstrip()
    a, sep, b = p.partition(': ')  # "Burden voltage: ON"
    return one(t, a) + sep + one(t, b) if sep else p


def label(t, s):
    return ' > '.join(one(t, p) for p in s.split(' > '))


def convert(path, lang, t):
    text = open(path, encoding='utf-8').read().split('\n')
    fence = skip = False
    for i, l in enumerate(text):
        if l.startswith('<!-- label table'):
            skip = True
        elif not l.strip():
            skip = False
        if skip:
            continue
        if l.lstrip().startswith('```'):
            fence = not fence
        if fence:
            continue
        l = IMG.sub(lambda m: m.group(1) + lang + '/', l)
        l = re.sub(r'\*\*([^*\n]+)\*\*', lambda m: '**%s**' % label(t, m.group(1)), l)
        if l.startswith('|') and '---' not in l and '`' not in l:
            cells = l.split('|')
            l = '|'.join(' %s ' % t[c.strip()] if c.strip() in t else c for c in cells)
        text[i] = l
    text = '\n'.join(text)
    for a, b in AFTER.get(lang, []):
        text = text.replace(a, b)
    open(path, 'w', encoding='utf-8').write(text)


def main():
    for lang in sys.argv[1:] or ['ru', 'zh']:
        t = table(lang)
        for p in ('docs/manual/README.%s.md', 'README.%s.md'):
            convert(os.path.join(ROOT, p % lang), lang, t)


main()
