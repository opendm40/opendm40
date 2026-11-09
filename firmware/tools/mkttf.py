#!/usr/bin/env python3
"""The faced text fonts re-rasterised from an outline font for
tools/mkstockfont.py (src/stockfont_data.rs) and tools/mklang_x.py (the
language glyphs of src/lang_*.rs).

Face: Noto Sans (variable, SIL OFL 1.1), NotoSans[wdth,wght].ttf from
DM40_TTF_DIR (default target/noto, tools/fetch-fonts.sh). Each place
(PLACES) is one weight / width at a cap height (the 'H' `cap` px tall) and
replaces its stock fonts' glyphs: U = TEXT22 + ASCII26 (Medium, cap 14),
HIST, A = SC_F16 + SC_F12, SC_C, B = F21 + K_FONT, LETTERS. A font after the
first of a place draws with the first's glyphs (stockfont.rs font_base).
F14 is not faced here (X11 6x13, tools/mkterm.py).

Storage: 2 bpp gray (coverage 0 / 5 / 10 / 15) in HUF with its own table set
('HUF2', tools/mkstockfont.py). Rasterisation: FreeType, FT_LOAD_TARGET_LIGHT
| FT_LOAD_FORCE_AUTOHINT at the em where 'H' is `cap` px tall; if no hinted
em gives that cap, unhinted at the em where the outline's 'H' is exactly
`cap` px. Coverage (a * 3 + 127) // 255. Whole-pixel advances, no kerning,
digits share the widest digit's advance in the places that show live
numbers; a glyph whose solid ink would touch its neighbour's advances one
more column.
"""
import json
import os

import mkterm

FW = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TTF_DIR = os.environ.get('DM40_TTF_DIR') or os.path.join(FW, 'target', 'noto')
NOTO = 'NotoSans[wdth,wght].ttf'
# The faced fonts' placement, for tools/mklang_x.py.
INFO = os.path.join(FW, 'target', 'mkttf.json')
# The fonts drawn with another's glyphs, for tools/mklang_x.py
# (tools/mkstockfont.py writes it).
SHARE_JSON = os.path.join(FW, 'target', 'mkttf-share.json')


def lang_glyph(fname, code):
    """An extra (UI language) glyph of a faced font: (Glyph, baseline row,
    format, line_h) in the place's face; None if `fname` is not faced."""
    if not os.path.exists(INFO):
        return None
    info = json.load(open(INFO))
    if fname not in info:
        return None
    place, base, fmt, lh = info[fname]
    assert fmt == 'HUF2', (fname, fmt)
    _, wght, wdth, cap, tab = PLACES[place]
    key = (wght, wdth, cap)
    if key not in _TT:
        _TT[key] = TTF(wght, wdth, cap)
    return _TT[key].gray(code), base, fmt, lh


_TT = {}

# place -> (fonts: the first holds the glyphs, the others share them;
# wght, wdth, cap, tabular digits). TEXT22, ASCII26 and F14 are not faced
# on their own (fmt_of): U holds TEXT22's and ASCII26's glyphs.
PLACES = {
    'TEXT22': (['TEXT22'], 500, 100, 15, False),
    'HIST': (['HIST'], 500, 87.5, 13, True),
    'F14': (['F14'], 700, 100, 9, True),
    'A': (['SC_F16', 'SC_F12'], 600, 100, 10, True),
    'SC_C': (['SC_C'], 700, 100, 12, False),
    'ASCII26': (['ASCII26'], 500, 100, 16, False),
    'B': (['F21', 'K_FONT'], 500, 100, 20, True),
    'LETTERS': (['LETTERS'], 500, 100, 22, True),
    'U': (['TEXT22', 'ASCII26'], 500, 100, 14, False),
}
# Glyphs a place takes over from stockfont_w.rs / HAND_GLYPHS (font name ->
# chars): drawn in the place's face instead.
OWN = {'HIST': 'Wk', 'TEXT22': '℃℉', 'ASCII26': 'Ω', 'F21': 'Wk', 'K_FONT': 'Wk',
       'LETTERS': 'Wk', 'F14': 'σ', 'SC_F12': 'Ω°'}
DIGITS = '0123456789'
# Fonts that go to region A2 as well (tools/mkstockfont.py).
A2_EXTRA = ('TEXT22',)


def fmt_of(place):
    """Storage of a place: 'HUF2' (2 bpp gray in HUF); None = not faced
    (TEXT22 / ASCII26: in U; F14: tools/mkterm.py)."""
    return None if place in ('TEXT22', 'ASCII26', 'F14') else 'HUF2'


class Glyph:
    """A rendered glyph: rows of coverage 0..15, first column `left` from
    the pen, first pixel row `top` above the baseline, h rows, advance."""

    def __init__(self, rows, left, top, h, w, adv):
        self.rows, self.left, self.top, self.h, self.w, self.adv = rows, left, top, h, w, adv


def _crop(rows, zero):
    """Rows / columns of `zero` at the edges off: (rows, dx, dy) or None."""
    if not rows or not any(v != zero for r in rows for v in r):
        return None
    y0 = 0
    while all(v == zero for v in rows[y0]):
        y0 += 1
    y1 = len(rows)
    while all(v == zero for v in rows[y1 - 1]):
        y1 -= 1
    rows = rows[y0:y1]
    x0 = 0
    while all(r[x0] == zero for r in rows):
        x0 += 1
    x1 = len(rows[0])
    while all(r[x1 - 1] == zero for r in rows):
        x1 -= 1
    return [r[x0:x1] for r in rows], x0, y0


class TTF:
    def __init__(self, wght, wdth, cap):
        import freetype
        path = os.path.join(TTF_DIR, NOTO)
        if not os.path.exists(path):
            raise SystemExit('%s missing: tools/fetch-fonts.sh (or DM40_TTF_DIR)' % path)
        self.ft = freetype
        self.f = freetype.Face(path)
        self.f.set_var_design_coords([wght if a.tag == 'wght' else wdth for a in self.f.get_variation_info().axes])
        self.light = freetype.FT_LOAD_TARGET_LIGHT | freetype.FT_LOAD_FORCE_AUTOHINT
        self.em = None
        for q in range(32, 160):
            em = q / 4
            self.f.set_char_size(int(em * 64))
            self.f.load_char('H', freetype.FT_LOAD_RENDER | self.light)
            if self.f.glyph.bitmap.rows == cap:
                self.em = em
                break
        if self.em is None:
            # No hinted em gives that cap (Noto Medium: 14 -> 16): unhinted,
            # the em at which the outline's H is exactly `cap` px tall (its
            # baseline and top on pixel edges, rounds and x-height soft).
            self.light = freetype.FT_LOAD_TARGET_LIGHT | freetype.FT_LOAD_NO_HINTING
            self.f.load_char('H', freetype.FT_LOAD_NO_SCALE)
            bb = self.f.glyph.outline.get_bbox()
            self.em = cap * self.f.units_per_EM / float(bb.yMax - bb.yMin)
        self.f.set_char_size(int(round(self.em * 64)))
        self.f.load_char('H', freetype.FT_LOAD_RENDER | self.light)
        self.cap = self.f.glyph.bitmap.rows

    def has(self, code):
        return self.f.get_char_index(code) != 0

    def _adv(self):
        return (self.f.glyph.advance.x + 32) >> 6

    def gray(self, code, L=3):
        """Coverage quantised to L + 1 levels, scaled to 0..15."""
        f = self.f
        f.load_char(chr(code), self.ft.FT_LOAD_RENDER | self.light)
        g, b = f.glyph, f.glyph.bitmap
        adv = self._adv()
        rows = [[(b.buffer[r * b.pitch + x] * L + 127) // 255 * (15 // L) for x in range(b.width)] for r in range(b.rows)]
        c = _crop(rows, 0)
        if c is None:
            return Glyph([], 0, 0, 0, 0, adv)
        rows, dx, dy = c
        return Glyph(rows, g.bitmap_left + dx, g.bitmap_top - dy, len(rows), len(rows[0]), adv)


class Adapter:
    """mkterm.bdf_base's view of a face: glyph(code) -> mkterm.Ink."""

    def __init__(self, glyphs):
        self.g = glyphs

    def has(self, code):
        return code in self.g

    def glyph(self, code):
        return self.g[code]


def solid_px(g, thr=15):
    """Solid ink pixels {(x, y)} of an entry, y from the glyph's top (its
    rows are placed by yoff: y + top in the line box)."""
    code, w, h, adv, yoff, d = g
    if not h or not isinstance(d, tuple):
        return set()
    return {(x, y) for y, r in enumerate(d[2]) for x, v in enumerate(r) if v >= thr}


def gap_fix(out):
    """No solid ink (coverage 15) of two glyphs side by side may touch,
    diagonals included (ui-render: glyphs of a text do not touch; the
    fainter fringe may): a glyph that touches some glyph placed after it
    advances one more. Digits keep one advance."""
    px = {}
    for g in out:
        c, w, h, adv, yoff, d = g
        top = -(h + yoff)        # rows above the line bottom (same for all glyphs of the font)
        px[c] = {(x, y + top) for x, y in solid_px(g)}
    adv = {g[0]: g[3] for g in out}
    # left / right solid columns per row, for a fast test
    def touches(a, b):
        pa, pb = px[a], px[b]
        if not pa or not pb:
            return False
        sh = {(x + adv[a], y) for x, y in pb}
        return any((x + dx, y + dy) in sh for x, y in pa for dx in (0, 1) for dy in (-1, 0, 1) if x >= adv[a] - 2)
    codes = [g[0] for g in out]
    need = {a for a in codes if any(touches(a, b) for b in codes)}
    dig = {ord(c) for c in DIGITS} & set(codes)
    if need & dig:
        need |= dig
    if need:
        print('    one more column after: %s' % ''.join(chr(c) for c in sorted(need)))
    return [(c, w, h, a + (1 if c in need else 0), yoff, d) for (c, w, h, a, yoff, d) in out]


def place_glyph(gl, fmt, base, lh, desc, dadv):
    """A Glyph -> (w, h, adv, yoff, ('rows', fmt, stored rows))."""
    left, adv = gl.left, gl.adv
    if dadv is not None:
        left += (dadv - adv) // 2
        adv = dadv
    if left < 0:
        adv -= left
        left = 0
    if not gl.h:
        return (adv, 0, adv, 0, ('rows', fmt, []))
    w = left + gl.w
    adv = max(adv, w)
    rows = [[0] * left + list(r) for r in gl.rows]
    top_row = base - gl.top
    yoff = lh - desc - gl.h - top_row
    return (w, gl.h, adv, yoff, ('rows', fmt, rows))


def stock_glyph(g, fmt, four, lh, desc, cls):
    """A stock glyph kept in a faced font (a code the face lacks, 0x7F):
    its coverage in the place's storage (1 bpp: 0 / 15)."""
    code, w, h, adv, yoff, data = g
    rows = mkterm.unpack(data, w, h, four)
    if cls != 'A':
        # Class B / C: h = line_h; as class A on its ink rows.
        r = [y for y, row in enumerate(rows) if any(row)]
        if not r:
            return (code, w, 0, adv, 0, ('rows', fmt, []))
        rows = rows[r[0]:r[-1] + 1]
        top = r[0]
        h = len(rows)
        yoff = lh - desc - h - top
    rows = [[(v * 3 + 7) // 15 * 5 for v in row] for row in rows]
    return (code, w, h, adv, yoff, ('rows', fmt, rows))


def restyle(loaded):
    """mkstockfont.py `loaded` with the faced places re-rasterised:
    (loaded, share {font: (base, yoff delta)}, own {font: chars})."""
    by = {n: (k, v) for n, k, v in loaded}
    share, own = {}, {}
    new = {}
    info = {}
    for place, (fonts, wght, wdth, cap, tab) in PLACES.items():
        fmt = fmt_of(place)
        if fmt is None:
            continue
        tt = TTF(wght, wdth, cap)
        # The place's codes: every font's own (no CJK) and the glyphs it takes over.
        codes = set()
        stock = {}
        for n in fonts:
            lh, de, four, gl, cls = by[n][1]
            for g in gl:
                if g[0] < 0x2E80:
                    codes.add(g[0])
                    stock.setdefault(g[0], (g, n))
            codes |= {ord(c) for c in OWN.get(n, '')}
        if place == 'A':
            # SC_F12 took most glyphs from F14 (SHARE): the union.
            lh, de, four, gl, cls = by['F14'][1]
            codes |= {g[0] for g in gl}
        name0 = fonts[0]
        lh, de, four, gl, cls = by[name0][1]
        ren = {c: tt.gray(c) for c in sorted(codes) if tt.has(c) and c != 0x7F}
        dadv = max(ren[ord(c)].adv for c in DIGITS) if tab else None
        # The glyph boxes for the baseline choice.
        boxes = {c: mkterm.Ink([[0]] * g.h, g.left, g.top, g.adv) for c, g in ren.items()}
        bases, sb = {}, {}
        for n in fonts:
            nlh, nde, nfour, ngl, ncls = by[n][1]
            sbase, scap, _ = mkterm.stock_base(ngl, nfour, nlh, nde)
            sb[n] = (sbase, scap)
            bases[n] = mkterm.bdf_base(Adapter(boxes), sbase, scap, nlh, codes)
        # A line box too small for the face's ascenders / descenders grows
        # to hold them: no glyph is cut.
        mt = max(g.top for g in ren.values() if g.h)
        md = max(g.h - g.top for g in ren.values() if g.h)
        nl, grow = {}, {}
        for n in fonts:
            nlh = by[n][1][0]
            ot, ob = max(0, mt - bases[n]), max(0, bases[n] + md - nlh)
            if ot or ob:
                # Grown so that the caps' centre keeps its place relative to
                # the box's centre (a text centred in its rect stays where
                # the stock font put it), the face's ascenders and
                # descenders inside: the smallest such box.
                sbase, scap = sb[n]
                c0 = sbase - scap / 2.0 - nlh / 2.0
                for L in range(nlh + 1, nlh + 16):
                    b = int(round(L / 2.0 + c0 + tt.cap / 2.0))
                    if b >= mt and L - b >= md:
                        break
                ot, ob = b - bases[n], (L - b) - (nlh - bases[n])
                nl[n], bases[n] = L, b
            else:
                nl[n] = nlh
            grow[n] = ob
            if ot or ob:
                print('    %s: line box %d -> %d px (+%d above, +%d below)' % (n, nlh, nl[n], ot, ob))
        lh = nl[name0]
        # descent = rows below the baseline: lh - descent is the baseline
        # (as in the stock class A fonts; ui-render marks descenders by it).
        de = lh - bases[name0]
        out = []
        for c in sorted(codes):
            if c in ren:
                g = ren[c]
                d = dadv if chr(c) in DIGITS else None
                out.append((c,) + place_glyph(g, fmt, bases[name0], lh, de, d))
            elif c in stock:
                sg, sn = stock[c]
                slh, sde, sfour, _, scls = by[sn][1]
                c2, w2, h2, a2, y2, d2 = stock_glyph(sg, fmt, sfour, slh, sde, scls)
                # Its rows relative to the stock baseline, on the face's.
                sb = mkterm.stock_base(by[sn][1][3], sfour, slh, sde)[0]
                top = slh - sde - h2 - y2 - sb + bases[name0]
                out.append((c2, w2, h2, a2, lh - de - h2 - top, d2))
        out = gap_fix(out)
        # A word space at least lh / 3 + 1 (stockfont's layout gives a space
        # lh / 3; the face's letter gaps reach 3 px: ui-render's word gap).
        out = [(c, w, h, max(adv, lh // 3 + 1) if c == 0x20 else adv, yoff, d) for (c, w, h, adv, yoff, d) in out]
        new[name0] = (lh, de, fmt, out, 'A')
        for n in fonts:
            info[n] = (place, bases[n], fmt, nl[n])
        over = []
        for (c, w, h, adv, yoff, d) in out:
            top = lh - de - h - yoff
            if h and (top < 0 or top + h > lh):
                over.append(chr(c))
        print('TTF %-8s %-5s Noto wght %d wdth %s cap %d (H %d) em %.2f base %s digit adv %s; %d glyphs; outside the %d px line: %s'
              % (place, fmt, wght, wdth, cap, tt.cap, tt.em, bases, dadv, len(out), lh, ''.join(over)))
        for n in fonts[1:]:
            nlh = nl[n]
            nde = nlh - bases[n]
            delta = (nlh - nde - bases[n]) - (lh - de - bases[name0])
            share[n] = (name0, delta)
            over = [chr(c) for (c, w, h, adv, yoff, d) in out
                     if h and (nlh - nde - h - (yoff + delta) < 0 or nlh - nde - (yoff + delta) > nlh)]
            print('    %s shares, yoff %+d; outside its %d px line: %s' % (n, delta, nlh, ''.join(over)))
            new[n] = (nlh, nde, fmt, [], 'A')
        for n in fonts:
            own[n] = OWN.get(n, '')
    res = [(n, k, new.get(n, v)) for n, k, v in loaded]
    # For tools/mklang_x.py (the UI language's extra glyphs in the same face).
    os.makedirs(os.path.dirname(INFO), exist_ok=True)
    json.dump(info, open(INFO, 'w'))
    return res, share, own


def drop_own(path, fonts, own, addr):
    """stockfont_w.rs without the rows of (font, char) that the faced font now has."""
    import re
    rx = re.compile(r"    \((0x[0-9A-F]+), '(.*?)', ")
    keys = {}
    for n, chars in own.items():
        for ch in chars:
            keys[(addr[n], ch)] = True
    src = open(path, encoding='utf-8').read().split('\n')
    out, pend, n = [], [], 0
    for ln in src:
        if ln.startswith('    //'):
            pend.append(ln)
            continue
        m = rx.match(ln)
        if m:
            g2 = m.group(2)
            ch = chr(int(g2[3:-1], 16)) if g2.startswith('\\u{') else g2
            if (int(m.group(1), 16), ch) in keys:
                pend = []
                n += 1
                continue
        out += pend + [ln]
        pend = []
    out += pend
    txt = '\n'.join(out)
    m = re.search(r'OWN_GLYPHS: \[\(u32, char, bool, u8, u8, u8, i8, &\[u8\]\); (\d+)\]', txt)
    txt = txt.replace(m.group(0), m.group(0).replace('; %s]' % m.group(1), '; %d]' % (int(m.group(1)) - n)))
    open(path, 'w', encoding='utf-8').write(txt)
    return n
