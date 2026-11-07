#!/usr/bin/env python3
"""Every string literal with a word in the UI sources is in the language tables
or in i18n/english.txt; the tables have the same keys; no key is stale; every
key has a `tr!` site (src/lang.rs: else no image shows its translation) and
every `tr!` argument is a key.
    check.py [--write-english]   rewrites i18n/english.txt from the sources"""
import glob
import os
import re
import sys

UI = 'accuracy autooff dmm app app_ch2 app_decode app_inrush app_log app_power app_touchtest app_wave dialog gallery meter powermeter scope_meas scope_page scope_ui splash trigsoft sentinel gen genplan app_inrush_live scope_ch2 logger ui ui_decode ui_inrush ui_log ui_power ui_touchtest danger'.split()
LANGS = 'zh ru it fr'.split()
TR = re.compile(r'(?<![\w])tr!\("((?:[^"\\\n]|\\.)*)"\)')
LIT = re.compile(r'''(?<![br'])"((?:[^"\\\n]|\\.)*)"''')


def literals():
    out = {}
    for name in UI:
        t = open('src/%s.rs' % name, encoding='utf-8').read()
        t = t.split('#[cfg(test)]\nmod ')[0]
        t = '\n'.join(l for l in (re.sub(r'(^|\s)//.*', '', l) for l in t.split('\n')) if not l.lstrip().startswith('#['))
        for m in LIT.finditer(t):
            s = m.group(1)
            if re.search(r'[A-Za-z]{2}', s) and not s.endswith('.rs') and s[0] not in '.#\\':
                out.setdefault(s, name)
    return out


def tr_args():
    out = {}
    for p in sorted(glob.glob('src/*.rs')):
        if os.path.basename(p).startswith('tr_'):
            continue
        for m in TR.finditer(open(p, encoding='utf-8').read()):
            out.setdefault(m.group(1), p)
    return out


def translated():
    """The keys with a translation in some language."""
    out = set()
    for lang in LANGS:
        for l in open('i18n/%s.tsv' % lang, encoding='utf-8'):
            if l.strip() and l[0] != '#':
                k, z, _ = l.rstrip('\n').split('\t')
                if k != z:
                    out.add(k)
    return out


def keys(lang):
    return {l.split('\t')[0] for l in open('i18n/%s.tsv' % lang, encoding='utf-8') if l.strip() and l[0] != '#'}


def main():
    lits = literals()
    ks = [keys(l) for l in LANGS]
    p = 'i18n/english.txt'
    if '--write-english' in sys.argv:
        with open(p, 'w', encoding='utf-8') as o:
            o.write('# UI strings that stay English on purpose (units, signal names, diagnostics, rare errors).\n')
            o.writelines(s + '\n' for s in sorted(set(lits) - ks[0]))
        return
    eng = {l.rstrip('\n') for l in open(p, encoding='utf-8') if l[0] != '#'}
    bad = 0
    for l, k in zip(LANGS, ks):
        if k != ks[0]:
            print('%s.tsv keys differ from zh.tsv: %s' % (l, sorted(k ^ ks[0])))
            bad = 1
    for s, f in sorted(lits.items()):
        if s not in ks[0] and s not in eng:
            print('new UI string in src/%s.rs: %r: add it to i18n/*.tsv or i18n/english.txt' % (f, s))
            bad = 1
    for s in sorted(ks[0] - set(lits)):
        print('i18n key not in the sources: %r' % s)
        bad = 1
    trs = tr_args()
    for s in sorted(translated() - set(trs)):
        print('i18n key without a tr! site: %r' % s)
        bad = 1
    for s, f in sorted(trs.items()):
        if s not in ks[0]:
            print('tr!(%r) in %s: not in i18n/*.tsv' % (s, f))
            bad = 1
    if bad:
        sys.exit(1)
    print('lang-check: %d strings translated, %d English on purpose' % (len(ks[0]), len(eng)))


main()
