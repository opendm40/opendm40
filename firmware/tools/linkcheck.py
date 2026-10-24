#!/usr/bin/env python3
"""Check every relative Markdown link and #anchor in the tracked *.md files,
and that firmware/README.md links every firmware/docs/*.md.
Usage: firmware/tools/linkcheck.py [REPO_ROOT]  (default: the current directory)."""
import os, re, subprocess, sys, urllib.parse
root = sys.argv[1] if len(sys.argv) > 1 else '.'
os.chdir(root)
files = [f for f in subprocess.check_output(['git', 'ls-files', '*.md'], text=True).split('\n') if f and not f.startswith('firmware/vendor/')]
LINK = re.compile(r'(?<![\w\]!])\[(?:[^\]\[]|\[[^\]]*\])*\]\(([^)\s]+)(?:\s+"[^"]*")?\)|!\[[^\]]*\]\(([^)\s]+)\)')
def slug(h):
    h = re.sub(r'`', '', h.strip().lower())
    h = re.sub(r'\[([^\]]*)\]\([^)]*\)', r'\1', h)
    h = re.sub(r'[^\w\- ]', '', h)
    return h.replace(' ', '-')
anchors = {}
def get_anchors(p):
    if p not in anchors:
        a = set(); seen = {}; fence = False
        for l in open(p, encoding='utf-8'):
            if l.lstrip().startswith('```'): fence = not fence; continue
            if fence: continue
            m = re.match(r'^#{1,6}\s+(.*?)\s*#*\s*$', l)
            if m:
                s = slug(m.group(1)); n = seen.get(s, 0); seen[s] = n + 1
                a.add(s if n == 0 else '%s-%d' % (s, n))
            for x in re.findall(r'<a\s+(?:name|id)="([^"]+)"', l): a.add(x)
        anchors[p] = a
    return anchors[p]
bad = 0
for f in files:
    text = open(f, encoding='utf-8').read()
    text = re.sub(r'```.*?```', '', text, flags=re.S)
    text = re.sub(r'`[^`\n]*`', lambda m: '' if '](' not in m.group(0) else m.group(0), text)
    for m in LINK.finditer(text):
        t = m.group(1) or m.group(2)
        if re.match(r'^[a-z]+:', t) or t.startswith('//'): continue
        path, _, frag = t.partition('#')
        path = urllib.parse.unquote(path)
        tgt = os.path.normpath(os.path.join(os.path.dirname(f), path)) if path else f
        if not os.path.exists(tgt):
            print('%s: missing %s' % (f, t)); bad += 1; continue
        if frag and tgt.endswith('.md') and os.path.isfile(tgt):
            if frag not in get_anchors(tgt):
                print('%s: missing anchor %s' % (f, t)); bad += 1
# docs index
idx = open('firmware/README.md', encoding='utf-8').read()
linked = {os.path.normpath(os.path.join('firmware', (m.group(1) or '').partition('#')[0])) for m in LINK.finditer(idx) if m.group(1)}
for d in sorted(x for x in files if re.match(r'firmware/docs/[^/]+\.md$', x)):
    if d not in linked:
        print('firmware/README.md: does not link %s' % d); bad += 1
print('%d problems in %d files' % (bad, len(files)))
sys.exit(1 if bad else 0)
