#!/usr/bin/env python3
"""DM40 Term: the text fonts drawn from Terminus Font bitmaps, for
tools/mkstockfont.py (src/stockfont_data.rs, src/stockfont_w.rs) and
tools/mklang_x.py (the accented and Cyrillic letters of src/lang_*.rs).

Based on Terminus Font 4.49.1 by Dimitar Zhekov (SIL OFL 1.1,
../fonts/OFL-DM40Term.txt). Modified (our 0, the placement in the stock
cells, the stock Omega), so under its own name: the OFL reserves the name
"Terminus Font".

Fonts (FACE): TEXT22 (menus, settings) ter-u22b, HIST (history values)
ter-u20b, F14 (small labels), SC_F12 and SC_F16 (scope texts) ter-u12b; each
BDF at its native size, never scaled. Every other font stays stock (ASCII26,
the big and 4 bpp fonts, the icons).

* Placement: the glyph's ink box on the stock baseline, raised or lowered
  (`bdf_base`) so the ascenders and descenders fit the line box. Digits share
  one advance. In the cell fonts (F14, HIST) a glyph wider than its stock
  cell stays stock (listed in the log).
* Our edits: the 0 is a plain oval without Terminus's dot (ZERO; at 12 px it
  is the shape of the O); the Omega stays stock (Terminus's is narrow: "600Ω"
  read "6000"), moved by the baseline difference; ℃ / ℉ are Terminus's ring
  and C / F, the ring's top on the caps' top row (no taller than the caps).
* A code the face lacks, or one not text (0x7F), stays stock.

Input: the BDF files of terminus-font-4.49.1.tar.gz with its alternative
Cyrillic и й д в (alt/ij1, alt/dv1: the Russian shapes), not in the repo:
tools/fetch-terminus.sh puts them in target/terminus-font-4.49.1
(DM40_TERM_DIR overrides); each file's SHA-256 is checked here. No time or
path goes into the output.
"""

import hashlib
import os

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
DIR = os.environ.get('DM40_TERM_DIR') or os.path.join(FW, 'target', 'terminus-font-4.49.1')

# BDF file -> SHA-256 (terminus-font-4.49.1.tar.gz with alt/ij1 and alt/dv1,
# tools/fetch-terminus.sh).
FILES = {
    'ter-u12b.bdf': 'd633cae5e16c8eccaf2af9233b81c8ceb7ecbca5685fadf8e7dc5472e483308d',
    'ter-u20b.bdf': 'c3ee3c76111ebdc473679b85eef6677642986d3b1be0ca145c70a9e3f212bd90',
    'ter-u22b.bdf': 'fe4de45b798e7c068ac078ee42780dfc66fd03c90d5a4a490b2b1ae455d2b4f7',
}
# Font -> BDF file. TEXT22: 24 px has the stock cap height (15) but its 12 px
# cells overflow the menus; 22 px (cap 14, 11 px cells) fits.
FACE = {'TEXT22': 'ter-u22b.bdf', 'HIST': 'ter-u20b.bdf', 'F14': 'ter-u12b.bdf',
        'SC_F12': 'ter-u12b.bdf', 'SC_F16': 'ter-u12b.bdf'}
# Fonts in the stock fixed cells (tight boxes): no glyph wider than its cell.
CELL = ('HIST', 'F14')
OHM = 0x3A9
# Drawn as a ring + letter.
TEXTS = {0x2103: [0xB0, 0x43], 0x2109: [0xB0, 0x46]}
DIGITS = [ord(c) for c in '0123456789']

# Our 0 (the full BDF cell): Terminus's 0 has a dot in the middle. 20 / 22 px:
# an oval, rounder at the ends than the O (same stems, same width); 12 px:
# five columns leave no room for a second shape, it is the O.
ZERO = {
    'ter-u12b.bdf': """
......
......
.###..
#...#.
#...#.
#...#.
#...#.
#...#.
#...#.
.###..
......
......
""",
    'ter-u20b.bdf': """
..........
..........
..........
...####...
..##..##..
.##....##.
.##....##.
.##....##.
.##....##.
.##....##.
.##....##.
.##....##.
.##....##.
.##....##.
..##..##..
...####...
..........
..........
..........
..........
""",
    'ter-u22b.bdf': """
...........
...........
...........
....###....
...##.##...
..##...##..
.##.....##.
.##.....##.
.##.....##.
.##.....##.
.##.....##.
.##.....##.
.##.....##.
.##.....##.
..##...##..
...##.##...
....###....
...........
...........
...........
...........
...........
""",
}


class Ink:
    """A glyph: rows of coverage 0 / 15, first column `left` from the pen,
    first row `top` above the baseline, advance `adv`."""

    def __init__(self, rows, left, top, adv):
        self.rows, self.left, self.top, self.adv = rows, left, top, adv
        self.w = len(rows[0]) if rows else 0
        self.h = len(rows)


class Bdf:
    """A BDF bitmap font (one size), with ZERO applied."""

    def __init__(self, name):
        path = os.path.join(DIR, name)
        if not os.path.exists(path):
            raise SystemExit('%s missing: tools/fetch-terminus.sh (or DM40_TERM_DIR)' % path)
        raw = open(path, 'rb').read()
        sha = hashlib.sha256(raw).hexdigest()
        if sha != FILES[name]:
            raise SystemExit('%s: SHA-256 %s, expected %s' % (path, sha, FILES[name]))
        self.name = name
        self.g = {}
        lines = iter(raw.decode('latin-1').split('\n'))
        cur = None
        for ln in lines:
            p = ln.split()
            if not p:
                continue
            if p[0] == 'STARTCHAR':
                cur = {}
            elif p[0] == 'ENCODING':
                cur['enc'] = int(p[-1])
            elif p[0] == 'DWIDTH':
                cur['adv'] = int(p[1])
            elif p[0] == 'BBX':
                cur['bbx'] = tuple(int(v) for v in p[1:5])
            elif p[0] == 'BITMAP':
                w, h = cur['bbx'][:2]
                nb = (w + 7) // 8 * 8
                rows = []
                for _ in range(h):
                    v = int(next(lines).strip() or '0', 16)
                    rows.append([15 if (v >> (nb - 1 - x)) & 1 else 0 for x in range(w)])
                cur['rows'] = rows
            elif p[0] == 'ENDCHAR':
                if cur.get('enc', -1) >= 0 and 'rows' in cur:
                    self.g[cur['enc']] = cur
                cur = None
        z = self.g[ord('0')]
        rows = [[15 if c == '#' else 0 for c in r] for r in ZERO[name].split()]
        assert len(rows) == z['bbx'][1] and all(len(r) == z['bbx'][0] for r in rows), name
        z['rows'] = rows

    def has(self, code):
        return code in self.g

    def glyph(self, code):
        """The ink box of `code`."""
        c = self.g[code]
        w, h, xo, yo = c['bbx']
        rows = [list(r) for r in c['rows']]
        top = yo + h
        while rows and not any(rows[0]):
            rows.pop(0)
            top -= 1
        while rows and not any(rows[-1]):
            rows.pop()
        left = xo
        while rows and not any(r[0] for r in rows):
            rows = [r[1:] for r in rows]
            left += 1
        while rows and not any(r[-1] for r in rows):
            rows = [r[:-1] for r in rows]
        return Ink(rows, left, top, c['adv'])


def unpack(data, w, h, four):
    """Stock bitmap (column-major) -> rows [h][w] of coverage 0..15."""
    rows = [[0] * w for _ in range(h)]
    for x in range(w):
        for y in range(h):
            p = x * h + y
            if four:
                v = data[p >> 1]
                rows[y][x] = (v >> 4) if p % 2 == 0 else (v & 15)
            else:
                rows[y][x] = 15 if data[p >> 3] & (0x80 >> (p & 7)) else 0
    return rows


def pack(rows, w, h, four):
    out = bytearray((w * h + 1) // 2 if four else (w * h + 7) // 8)
    for x in range(w):
        for y in range(h):
            p = x * h + y
            v = rows[y][x]
            if four:
                out[p >> 1] |= (v << 4) if p % 2 == 0 else v
            elif v:
                out[p >> 3] |= 0x80 >> (p & 7)
    return bytes(out)


def join(parts):
    """Glyphs side by side on one baseline (a ring and a C)."""
    pen, cells = 0, {}
    for i in parts:
        for y, row in enumerate(i.rows):
            for x, v in enumerate(row):
                if v:
                    cells[(pen + i.left + x, i.top - y)] = v
        pen += i.adv
    xs = [k[0] for k in cells]
    ys = [k[1] for k in cells]
    x0, x1, y1, y0 = min(xs), max(xs), max(ys), min(ys)
    rows = [[cells.get((x, y), 0) for x in range(x0, x1 + 1)] for y in range(y1, y0 - 1, -1)]
    return Ink(rows, x0, y1, pen)


def stock_base(gl, four, line_h, descent):
    """(row of the baseline in the line box, stock 'H' height, stock digit advance)."""
    by = {g[0]: g for g in gl}
    code, w, h, adv, yoff, data = by.get(ord('H')) or by.get(ord('0'))
    r = [y for y, row in enumerate(unpack(data, w, h, four)) if any(row)]
    top = line_h - descent - h - yoff
    return top + r[-1] + 1, r[-1] - r[0] + 1, by[ord('0')][3]


def bdf_base(r, sbase, cap, line_h, codes):
    """Baseline row of the face: the stock one, raised until the descenders
    and lowered until the ascenders fit, caps centred within 1.5 px of the
    stock's when possible."""
    h = r.glyph(ord('H')).h
    best = None
    for base in sorted(range(sbase - 4, sbase + 2), key=lambda b: abs(b - sbase)):
        over = 0
        for c in 'gjpqyd|()[]bfhkl' + ''.join(map(chr, range(0x410, 0x450))):
            if ord(c) in codes and r.has(ord(c)):
                i = r.glyph(ord(c))
                over += max(0, i.top - base) + max(0, base - i.top + i.h - line_h)
        off = abs((2 * base - h) - (2 * sbase - cap))
        key = (over, off > 3, off)
        if best is None or key < best[0]:
            best = (key, base)
    return best[1]


def fit(i, four, line_h, descent, cls, base, adv_fix=None, gap=1):
    """A glyph -> stock entry fields (w, h, adv, yoff, bytes)."""
    # Taller than the line box (brackets, a bar): moved in, then cut.
    y0 = base - i.top
    if i.h and (y0 < 0 or y0 + i.h > line_h):
        rows = i.rows[:line_h]
        top = base - max(0, min(y0, line_h - len(rows)))
        i = Ink(rows, i.left, top, i.adv)
    left = i.left
    adv = i.adv
    if adv_fix is not None:
        left += (adv_fix - adv) // 2
        adv = adv_fix
    if left < 0:
        # No x offset in an entry: the pen moves on by what the ink needs.
        adv -= left
        left = 0
    w = left + i.w
    adv = max(adv, w + gap) if i.w else adv
    if not i.h:
        w = adv
        if cls == 'A':
            return (w, 0, adv, 0, b'')
        return (w, line_h, adv, 0, pack([[0] * w for _ in range(line_h)], w, line_h, four))
    if cls == 'A':
        rows = [[0] * left + r for r in i.rows]
        top_row = base - i.top
        return (w, i.h, adv, line_h - descent - i.h - top_row, pack(rows, w, i.h, four))
    rows = [[0] * w for _ in range(line_h)]
    for y, r in enumerate(i.rows):
        ry = base - i.top + y
        if 0 <= ry < line_h:
            rows[ry][left:left + i.w] = r
    return (w, line_h, adv, 0, pack(rows, w, line_h, four))


def shift_stock(g, dy, line_h, descent, four, cls):
    """A stock glyph moved `dy` rows down (the face's baseline is `dy` lower)."""
    code, w, h, adv, yoff, data = g
    if not dy or not h:
        return g
    if cls == 'A':
        return (code, w, h, adv, yoff - dy, data)
    rows = unpack(data, w, h, four)
    rows = ([[0] * w] * dy + rows[:h - dy]) if dy > 0 else (rows[-dy:] + [[0] * w] * -dy)
    return (code, w, h, adv, yoff, pack(rows, w, h, four))


class Face:
    """The faced fonts of one generator run; `info[font]` their placement."""

    def __init__(self):
        self.bdf = {}
        self.info = {}
        self.stock = {}

    def font(self, fname):
        f = FACE[fname]
        if f not in self.bdf:
            self.bdf[f] = Bdf(f)
        return self.bdf[f]

    def text(self, code):
        return 0x20 <= code < 0x7F or code in TEXTS

    def render(self, fname, code):
        s = self.info[fname]
        r = s['r']
        if code not in TEXTS:
            return r.glyph(code)
        ring, letter = (r.glyph(c) for c in TEXTS[code])
        # The ring's top on the caps' top row.
        cap = r.glyph(ord('H')).top
        # 2 px between the ring and the letter's ink (Terminus's ° is a
        # full cell wide).
        ring = Ink(ring.rows, ring.left, min(ring.top, cap), ring.left + ring.w + 2 - letter.left)
        return join([ring, letter])

    def restyle(self, fname, line_h, descent, four, gl, cls):
        """Glyph tuples (code, w, h, adv, yoff, bytes) of `fname` in the face."""
        assert not four, fname
        r = self.font(fname)
        sbase, cap, sdig = stock_base(gl, four, line_h, descent)
        codes = {g[0] for g in gl}
        base = bdf_base(r, sbase, cap, line_h, codes)
        s = self.info[fname] = dict(r=r, base=base, sbase=sbase, line_h=line_h, descent=descent, cls=cls, cap=cap,
                                    stock=[], four=four)
        cell = fname in CELL
        s['dadv'] = sdig if cell else max(r.glyph(c).adv for c in DIGITS)
        return [self.glyph(fname, g) for g in gl]

    def glyph(self, fname, g):
        """One stock glyph tuple -> the face's (or the stock one, moved to the
        face's baseline)."""
        s = self.info[fname]
        code, gw, gh, sadv, syoff, data = g
        keep = lambda: shift_stock(g, s['base'] - s['sbase'], s['line_h'], s['descent'], s['four'], s['cls'])
        if code == OHM:
            return keep()
        if not self.text(code) or not all(s['r'].has(c) for c in TEXTS.get(code, [code])):
            return g
        i = self.render(fname, code)
        args = (s['four'], s['line_h'], s['descent'], s['cls'], s['base'])
        if code in DIGITS:
            return (code,) + fit(i, *args, s['dadv'], 0 if fname in CELL else 1)
        if fname not in CELL:
            return (code,) + fit(i, *args)
        # Cell font: centred in the stock advance when wider; a glyph that
        # does not fit the stock cell stays stock.
        if not i.w:
            return g
        t = fit(i, *args, sadv if i.adv > sadv else None, 0)
        if t[2] > sadv + 1 or (fname == 'HIST' and i.w >= sadv):
            s['stock'].append(chr(code))
            return keep()
        return (code,) + t

    def log(self, fname):
        s = self.info[fname]
        H = s['r'].glyph(ord('H'))
        msg = 'DM40 Term %-7s %s  H %d (stock %d)  base %d (stock %d)  digit adv %d' % (
            fname, s['r'].name, H.h, s['cap'], s['base'], s['sbase'], s['dadv'])
        if s['stock']:
            msg += '  stock (wider than the cell): ' + ''.join(s['stock'])
        return msg

    def line(self, fname, code):
        """A face glyph (Cyrillic) for mklang_x: (rows [line_h + 4][w], w, adv),
        the ink on the face's baseline."""
        s = self.info[fname]
        i = s['r'].glyph(code)
        w, h, adv, yoff, data = fit(i, False, s['line_h'], s['descent'], 'B', s['base'])
        rows = unpack(data, w, h, False)
        return rows + [[0] * w for _ in range(4)], w, adv


def restyle(loaded):
    """mkstockfont.py `loaded` with the FACE fonts in DM40 Term; and the Face."""
    face = Face()
    res = []
    for fname, key, (line_h, descent, four, gl, cls) in loaded:
        if fname in FACE:
            gl = face.restyle(fname, line_h, descent, four, gl, cls)
            face.info[fname]['key'] = key
            print(face.log(fname))
        res.append((fname, key, (line_h, descent, four, gl, cls)))
    return res, face


# stockfont_w.rs entries (tools/mkwglyph.py) of the faced fonts: W and k of
# HIST, ℃ / ℉ of TEXT22 in the face; the others stay.
OWN_TEXT = {'W': ord('W'), 'k': ord('k'), '℃': 0x2103, '℉': 0x2109}


def own_glyphs(face, path):
    import re
    own = re.compile(r"    \((0x[0-9A-F]+), '(.*?)', (true|false), (\d+), (\d+), (\d+), (-?\d+), b\"(.*?)\"\),")
    keys = {s['key']: n for n, s in face.info.items()}
    out = []
    for ln in open(path, encoding='utf-8').read().split('\n'):
        m = own.match(ln)
        name = keys.get(int(m.group(1), 16)) if m else None
        code = OWN_TEXT.get(m.group(2)) if m else None
        if not name or not code:
            out.append(ln)
            continue
        assert m.group(3) == 'false'
        s = face.info[name]
        g = (code, int(m.group(4)), int(m.group(5)), int(m.group(6)), int(m.group(7)), b'\0' * 64)
        code, w, h, adv, yoff, b = face.glyph(name, g)
        if chr(code) in s['stock']:
            out.append(ln)
            continue
        while out and out[-1].startswith('    //   '):
            out.pop()
        out.extend('    //   ' + ''.join('@' if v else '.' for v in r) for r in unpack(b, w, h, False))
        out.append("    (%s, '%s', false, %d, %d, %d, %d, b\"%s\")," % (
            m.group(1), m.group(2), w, h, adv, yoff, ''.join('\\x%02x' % v for v in b)))
    open(path, 'w', encoding='utf-8').write('\n'.join(out))
