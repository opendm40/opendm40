#!/usr/bin/env python3
"""Per-module flash bytes from nm (symbols group by Rust module path). Alignment/pool gaps -> 'other'."""
import collections, re, subprocess, sys
elf = sys.argv[1] if len(sys.argv) > 1 else 'target/thumbv7em-none-eabihf/release/miniloader'
g = collections.Counter(); items = collections.defaultdict(list)
for l in subprocess.check_output(['arm-none-eabi-nm', '-S', '--size-sort', '-C', elf], text=True).splitlines():
    p = l.split(None, 3)
    if len(p) < 4: continue
    n, name = int(p[1], 16), p[3]
    m = re.search(r'miniloader::(\w+)', name)
    k = m.group(1) if m and m.group(1) in 'boot lcd touch spi fat install usb ui hw'.split() else ('vectors+fault+reset' if m or name == 'reset' else 'library')
    if name.startswith('OUTLINED'): k = 'outlined'
    if name.endswith('FONT'): k = 'font'
    g[k] += n; items[k].append((n, name))
for k, n in g.most_common(): print('%6d %s' % (n, k))
print('%6d total symbols' % sum(g.values()))
if '-v' in sys.argv:
    for k in items:
        print(k, sorted(items[k], reverse=True)[:12])
