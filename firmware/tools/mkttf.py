#!/usr/bin/env python3
"""EXPERIMENT (exp/font-storage): the text fonts re-rasterised from an
outline font (Noto Sans, SIL OFL 1.1) for tools/mkstockfont.py, to measure
the flash cost of anti-aliased / subpixel text fonts. Not for release.

DM40_FONT_SCENARIO (environment) picks the storage; empty = off (the
current DM40 Term fonts):
  term    Terminus 1 bpp in every text place (tools/mkterm.py, more sizes)
  g4      gray 4 bpp HUF (TEXT22 stays DM40 Term)   g4n: TEXT22 in Noto too
  g2h     gray 2 bpp (coverage 0 / 5 / 10 / 15) in HUF, tables retrained
  g2r     gray 2 bpp RAW2 (column-major 2 bpp, ink rows; random access)
  s3h     small places subpixel: 3 channels interleaved per pixel (3w wide
          rows) in HUF; big places gray 4 bpp HUF
  s3v     small places subpixel: 3x vertical subrows (gray, 3h rows) in HUF,
          the draw filters 3 subrows into each of R / G / B; big gray 4 bpp
  s3b     as s3v, subrows in HUF variant B (contexts by the pixel above,
          partial copies; own tables)
  s3v2    as s3v with 2 bpp subrows, big places gray 2 bpp (g2h)

Rasterisation (as the approved sheets, tmp/aafonts spec.py / subpix.py):
FreeType, FT_LOAD_TARGET_LIGHT | FT_LOAD_FORCE_AUTOHINT, the em at which
'H' is `cap` px tall, coverage (a * L + 127) // 255. Subpixel: the panel's
stripes are vertical, R-G-B from the top: FT_RENDER_MODE_LCD_V (Harmony:
channel k of row y = mean of the 3x subrows 3y+k-1 .. 3y+k+1). Whole-pixel
advances, no kerning, digits share the widest digit's advance in the places
that show live numbers.
"""
import ctypes
import os

import mkterm

TTF_DIR = os.environ.get('DM40_TTF_DIR', '/home/boger/work/board/alientek-dm40/tmp/aafonts/dl')
NOTO = 'NotoSans[wdth,wght].ttf'
SCEN = os.environ.get('DM40_FONT_SCENARIO', '')
# One face for TEXT22 and ASCII26 (g2h otherwise): v14t = Terminus ter-u22b
# (ASCII26 takes TEXT22's glyphs), v14 / v15 / v16 = Noto Sans Medium at
# that cap, gray 2 bpp, one glyph set (place U).
UNI = {'v14t': None, 'v14': 14, 'v15': 15, 'v16': 16}
INFO = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'target', 'mkttf-%s.json' % SCEN)


def lang_glyph(fname, code):
    """An extra (UI language) glyph of a TTF-faced font: (w, h, adv, yoff,
    rows on 0..15) in the place's face, on its baseline; None if `fname`
    is not faced."""
    import json
    if not SCEN or SCEN == 'term' or not os.path.exists(INFO):
        return None
    info = json.load(open(INFO))
    if fname not in info:
        return None
    place, base, fmt, lh = info[fname]
    assert fmt in ('HUF', 'HUF2', 'S3V', 'S3V2'), (fname, fmt)
    _, wght, wdth, cap, small, tab = PLACES[place]
    key = (wght, wdth, cap)
    if key not in _TT:
        _TT[key] = TTF(wght, wdth, cap)
    L = 3 if fmt in ('HUF2', 'S3V2') else 15
    g = _TT[key].subrows(code, L) if fmt in ('S3V', 'S3V2') else _TT[key].gray(code, L)
    return g, base, fmt, lh


_TT = {}

# place -> (fonts: the first holds the glyphs, the others share them;
# wght, wdth, cap, small (subpixel), tabular digits)
PLACES = {
    'TEXT22': (['TEXT22'], 500, 100, 15, False, False),
    'HIST': (['HIST'], 500, 87.5, 13, True, True),
    'F14': (['F14'], 700, 100, 9, True, True),
    'A': (['SC_F16', 'SC_F12'], 600, 100, 10, True, True),
    'SC_C': (['SC_C'], 700, 100, 12, True, False),
    'ASCII26': (['ASCII26'], 500, 100, 16, False, False),
    'B': (['F21', 'K_FONT'], 500, 100, 20, False, True),
    'LETTERS': (['LETTERS'], 500, 100, 22, False, True),
    'U': (['TEXT22', 'ASCII26'], 500, 100, UNI.get(SCEN) or 15, False, False),
}
# Terminus-everywhere: font -> BDF (the fonts mkterm.py already faces keep
# theirs); SC_F12 shares SC_F16's u16b glyphs (TERM_A14=1: its own u14b).
TERM_FACE = {'SC_C': 'ter-u18b.bdf', 'ASCII26': 'ter-u24b.bdf', 'F21': 'ter-u32b.bdf',
             'K_FONT': 'ter-u32b.bdf', 'LETTERS': 'ter-u32b.bdf'}
TERM_SHARE = {'K_FONT': 'F21'} if os.environ.get('TERM_A14') else {'SC_F12': 'SC_F16', 'K_FONT': 'F21'}
# Glyphs a place takes over from stockfont_w.rs / HAND_GLYPHS (font name ->
# chars): drawn in the place's face instead.
OWN = {'HIST': 'Wk', 'TEXT22': '℃℉', 'ASCII26': 'Ω', 'F21': 'Wk', 'K_FONT': 'Wk',
       'LETTERS': 'Wk', 'F14': 'σ', 'SC_F12': 'Ω°'}
DIGITS = '0123456789'
# Fonts that go to region A2 as well in a scenario (tools/mkstockfont.py).
A2_EXTRA = {'g2h': ('TEXT22',), 's3v2': ('TEXT22',), 'v14t': ('TEXT22',), 'v14': ('TEXT22',),
             'v15': ('TEXT22',), 'v16': ('TEXT22',)}


def fmt_of(place):
    """Storage of a place in the scenario: 'HUF' (4 bpp gray), 'HUF2' (2 bpp
    gray in HUF), 'RAW2', 'S3H', 'S3V', 'S3V2', 'S3B'; None = not faced."""
    small = PLACES[place][4]
    if place == 'U':
        return 'HUF2' if UNI.get(SCEN) else None
    if SCEN in UNI:
        # v14: F14 in X11 6x13 (tools/mkterm.py), not Noto.
        return None if place in ('TEXT22', 'ASCII26') or (place == 'F14' and SCEN == 'v14') else 'HUF2'
    if SCEN in ('g4', 'g4n'):
        return None if place == 'TEXT22' and SCEN == 'g4' else 'HUF'
    if SCEN == 'g2h':
        return None if place == 'TEXT22' else 'HUF2'
    if SCEN == 'g2r':
        return None if place == 'TEXT22' else 'RAW2'
    if SCEN in ('s3h', 's3v', 's3b'):
        if place == 'TEXT22':
            return None
        return {'s3h': 'S3H', 's3v': 'S3V', 's3b': 'S3B'}[SCEN] if small else 'HUF'
    if SCEN == 's3v2':
        if place == 'TEXT22':
            return None
        return 'S3V2' if small else 'HUF2'
    return None


class Glyph:
    """A rendered glyph: rows of stored values (gray 0..15, or (R, G, B), or
    for S3V the 3x subrows: 3 * h rows), first column `left` from the pen,
    first pixel row `top` above the baseline, h pixel rows, advance."""

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
        self.ft = freetype
        self.f = freetype.Face(os.path.join(TTF_DIR, NOTO))
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
        if self.em is None and SCEN in UNI:
            # No hinted em gives that cap (Noto Medium: 14 -> 16): unhinted,
            # the em at which the outline's H is exactly `cap` px tall (its
            # baseline and top on pixel edges, rounds and x-height soft).
            self.light = freetype.FT_LOAD_TARGET_LIGHT | freetype.FT_LOAD_NO_HINTING
            self.f.load_char('H', freetype.FT_LOAD_NO_SCALE)
            bb = self.f.glyph.outline.get_bbox()
            self.em = cap * self.f.units_per_EM / float(bb.yMax - bb.yMin)
        if self.em is None:
            # No em gives that cap: spec.py's fallback.
            self.em = cap / 0.7
        self.f.set_char_size(int(round(self.em * 64)))
        self.f.load_char('H', freetype.FT_LOAD_RENDER | self.light)
        self.cap = self.f.glyph.bitmap.rows

    def has(self, code):
        return self.f.get_char_index(code) != 0

    def _adv(self):
        return (self.f.glyph.advance.x + 32) >> 6

    def gray(self, code, L):
        """Coverage quantised to L + 1 levels, scaled to 0..15 (HUF) or kept
        0..L (RAW2: caller)."""
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

    def lcd(self, code):
        """The approved subpixel rendering: LCD_V, channel k = subrow 3y + k
        (R top), 4 bits each."""
        f = self.f
        f.load_char(chr(code), self.light)
        f.glyph.render(self.ft.FT_RENDER_MODE_LCD_V)
        g, b = f.glyph, f.glyph.bitmap
        adv = self._adv()
        h = b.rows // 3
        q = lambda v: (v * 15 + 127) // 255
        rows = [[tuple(q(b.buffer[(3 * r + k) * b.pitch + x]) for k in range(3)) for x in range(b.width)] for r in range(h)]
        c = _crop(rows, (0, 0, 0))
        if c is None:
            return Glyph([], 0, 0, 0, 0, adv)
        rows, dx, dy = c
        return Glyph(rows, g.bitmap_left + dx, g.bitmap_top - dy, len(rows), len(rows[0]), adv)

    def subrows(self, code, L=15):
        """The 3x subrows (the light-hinted outline scaled 3x vertically,
        gray), quantised to L + 1 levels on 0..15, on the pixel grid: rows
        3 p0 .. 3 p1 + 2 where p0 .. p1 are the pixel rows whose filtered
        channels can be non-zero (subrows outside are zero)."""
        ft, f = self.ft, self.f
        f.load_char(chr(code), self.light)
        adv = self._adv()
        m = ft.FT_Matrix(0x10000, 0, 0, 3 * 0x10000)
        ft.raw.FT_Outline_Transform(ctypes.byref(f.glyph._FT_GlyphSlot.contents.outline), ctypes.byref(m))
        f.glyph.render(ft.FT_RENDER_MODE_NORMAL)
        g, b = f.glyph, f.glyph.bitmap
        sub = [[(b.buffer[r * b.pitch + x] * L + 127) // 255 * (15 // L) for x in range(b.width)] for r in range(b.rows)]
        nz = [r for r in range(len(sub)) if any(sub[r])]
        if not nz:
            return Glyph([], 0, 0, 0, 0, adv)
        cols = [x for x in range(b.width) if any(sub[r][x] for r in nz)]
        sub = [row[cols[0]:cols[-1] + 1] for row in sub]
        w = len(sub[0])
        # Subrow r lies at 1/3 px heights t3 - r - 1 .. t3 - r above the
        # baseline; pixel row 0 of the grid is the one at T - 1 .. T px.
        t3 = g.bitmap_top
        T = -(-t3 // 3) + 1
        off = 3 * T - t3          # grid subrow of sub[0]
        j0, j1 = nz[0] + off, nz[-1] + off
        p0 = -(-(j0 - 3) // 3)
        p1 = (j1 + 1) // 3
        grid = []
        for j in range(3 * p0, 3 * p1 + 3):
            r = j - off
            grid.append(list(sub[r]) if 0 <= r < len(sub) else [0] * w)
        return Glyph(grid, g.bitmap_left + cols[0], T - p0, p1 - p0 + 1, w, adv)


def s3_channels(sub, h, w):
    """Firmware S3V filter: channel k of row y = round((s[3y+k-1] + s[3y+k]
    + s[3y+k+1]) / 3), subrows outside zero. -> rows of (R, G, B)."""
    S = lambda j: sub[j] if 0 <= j < len(sub) else [0] * w
    out = []
    for y in range(h):
        out.append([tuple((sum(S(3 * y + k + d)[x] for d in (-1, 0, 1)) * 2 + 3) // 6 for k in range(3)) for x in range(w)])
    return out


class Adapter:
    """mkterm.bdf_base's view of a face: glyph(code) -> mkterm.Ink."""

    def __init__(self, glyphs):
        self.g = glyphs

    def has(self, code):
        return code in self.g

    def glyph(self, code):
        return self.g[code]


def stock_rows(data, w, h, four):
    return mkterm.unpack(data, w, h, four)


def zero_of(fmt):
    return (0, 0, 0) if fmt == 'S3H' else 0


def strong_cols(g, fmt, thr=12):
    """First / last box column with solid ink (coverage >= thr of 15 in
    some channel) of an entry (code, w, h, adv, yoff, data), or None."""
    code, w, h, adv, yoff, d = g
    if not h or not isinstance(d, tuple):
        return None
    rows = d[2]
    # Subpixel: the mean of the channels (as ui-render's ink).
    if fmt in ('S3V', 'S3V2', 'S3B'):
        rows = [[(sum(px) + 1) // 3 for px in r] for r in s3_channels(rows, h, w)]
    elif fmt == 'S3H':
        rows = [[(sum(px) + 1) // 3 for px in r] for r in rows]
    cols = [x for x in range(w) if any(len(r) > x and r[x] >= thr for r in rows)]
    return (cols[0], cols[-1]) if cols else None


def solid_px(g, fmt, thr=15):
    """Solid ink pixels {(x, y)} of an entry, y from the glyph's top (its
    rows are placed by yoff: y + top in the line box)."""
    code, w, h, adv, yoff, d = g
    if not h or not isinstance(d, tuple):
        return set()
    rows = d[2]
    if fmt in ('S3V', 'S3V2', 'S3B'):
        rows = [[(sum(px) + 1) // 3 for px in r] for r in s3_channels(rows, h, w)]
    elif fmt == 'S3H':
        rows = [[(sum(px) + 1) // 3 for px in r] for r in rows]
    return {(x, y) for y, r in enumerate(rows) for x, v in enumerate(r) if v >= thr}


def gap_fix(out, fmt):
    """No solid ink (coverage >= 12 of 15) of two glyphs side by side may
    touch, diagonals included (ui-render: glyphs of a text do not touch; the
    fainter fringe may): a glyph that touches some glyph placed after it
    advances one more. Digits keep one advance."""
    px = {}
    for g in out:
        c, w, h, adv, yoff, d = g
        top = -(h + yoff)        # rows above the line bottom (same for all glyphs of the font)
        px[c] = {(x, y + top) for x, y in solid_px(g, fmt)}
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
    z = zero_of(fmt)
    rows = [[z] * left + list(r) for r in gl.rows]
    top_row = base - gl.top
    yoff = lh - desc - gl.h - top_row
    return (w, gl.h, adv, yoff, ('rows', fmt, rows))


def stock_glyph(g, fmt, four, lh, desc, cls):
    """A stock glyph kept in a faced font (a code the face lacks, 0x7F):
    its coverage in the place's storage (1 bpp: 0 / 15)."""
    code, w, h, adv, yoff, data = g
    rows = stock_rows(data, w, h, four)
    if cls != 'A':
        # Class B / C: h = line_h; as class A on its ink rows.
        r = [y for y, row in enumerate(rows) if any(row)]
        if not r:
            return (code, w, 0, adv, 0, ('rows', fmt, []))
        rows = rows[r[0]:r[-1] + 1]
        top = r[0]
        h = len(rows)
        yoff = lh - desc - h - top
    if fmt in ('HUF2', 'RAW2', 'S3V2'):
        rows = [[(v * 3 + 7) // 15 * 5 for v in row] for row in rows]
    if fmt == 'S3H':
        rows = [[(v, v, v) for v in row] for row in rows]
    if fmt in ('S3V', 'S3B', 'S3V2'):
        rows = [list(row) for row in rows for _ in range(3)]
    return (code, w, h, adv, yoff, ('rows', fmt, rows))


def restyle(loaded):
    """mkstockfont.py `loaded` with the scenario's places re-rasterised:
    (loaded, share {font: (base, yoff delta)}, own {font: chars})."""
    by = {n: (k, v) for n, k, v in loaded}
    share, own = {}, {}
    new = {}
    info = {}
    for place, (fonts, wght, wdth, cap, small, tab) in PLACES.items():
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
        L = 3 if fmt in ('HUF2', 'RAW2', 'S3V2') else 15
        gray = {c: tt.gray(c, L) for c in sorted(codes) if tt.has(c) and c != 0x7F}
        ink = {c: mkterm.Ink(g.rows, g.left, g.top, g.adv) for c, g in gray.items()}
        if fmt == 'S3H':
            ren = {c: tt.lcd(c) for c in gray}
        elif fmt in ('S3V', 'S3B', 'S3V2'):
            ren = {c: tt.subrows(c, 3 if fmt == 'S3V2' else 15) for c in gray}
        else:
            ren = gray
        dadv = max(ren[ord(c)].adv for c in DIGITS) if tab else None
        # The stored glyph boxes (subpixel: with the fringe rows) for the baseline choice.
        boxes = {c: mkterm.Ink([[0]] * g.h, g.left, g.top, g.adv) for c, g in ren.items()}
        bases, sb = {}, {}
        for n in fonts:
            nlh, nde, nfour, ngl, ncls = by[n][1]
            sbase, scap, _ = mkterm.stock_base(ngl, nfour, nlh, nde)
            sb[n] = (sbase, scap)
            bases[n] = mkterm.bdf_base(Adapter(boxes), sbase, scap, nlh, codes)
        # A line box too small for the face's ascenders / descenders (with
        # the subpixel fringe rows) grows to hold them: no glyph is cut.
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
        out = gap_fix(out, fmt)
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
    import json
    os.makedirs(os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'target'), exist_ok=True)
    json.dump(info, open(INFO, 'w'))
    return res, share, own


def term_restyle(loaded):
    """Terminus in every text place: the stock 4 bpp fonts become 1 bpp
    (a stock glyph the face lacks thresholded at 8), then mkterm.py.
    -> (loaded, share, own, face)."""
    for n, f in TERM_FACE.items():
        mkterm.FACE[n] = f
    mkterm.FACE['SC_F12'] = 'ter-u14b.bdf' if os.environ.get('TERM_A14') else 'ter-u16b.bdf'
    # W / k of the newly faced unit fonts from the face (their 4 bpp rows in
    # stockfont_w.rs are dropped); the fonts faced before keep mkterm's rows.
    own = {'LETTERS': 'Wk', 'F21': 'Wk', 'K_FONT': 'Wk'}
    res = []
    allc = {n: {g[0] for g in v[3]} for n, _, v in loaded}
    for n, b in TERM_SHARE.items():
        own.setdefault(b, '')
        own[b] += ''.join(chr(c) for c in sorted(allc[n] - allc[b]) if 0x20 <= c < 0x7F)
    for n, k, (lh, de, four, gl, cls) in loaded:
        if n in mkterm.FACE and four:
            ngl = []
            for (code, w, h, adv, yoff, data) in gl:
                rows = mkterm.unpack(data, w, h, True)
                rows = [[15 if v >= 8 else 0 for v in r] for r in rows]
                ngl.append((code, w, h, adv, yoff, mkterm.pack(rows, w, h, False)))
            gl, four = ngl, False
        if n in own:
            have = {g[0] for g in gl}
            for ch in own[n]:
                if ord(ch) not in have:
                    gl = sorted(gl + [(ord(ch), 1, 1, 1, 0, b'\0')])
        res.append((n, k, (lh, de, four, gl, cls)))
    loaded2, face = mkterm.restyle(res)
    share = {}
    for n, b in TERM_SHARE.items():
        s, t = face.info[n], face.info[b]
        share[n] = (b, (s['line_h'] - s['descent'] - s['base']) - (t['line_h'] - t['descent'] - t['base']))
    return loaded2, share, own, face


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


def term_share(loaded, face):
    """v14t: ASCII26 drawn with TEXT22's DM40 Term glyphs: {font: (base,
    yoff delta)}, the baselines on the stock ones."""
    if SCEN != 'v14t':
        return {}
    by = {n: v for n, _, v in loaded}
    lh, de, four, gl, cls = by['ASCII26']
    sbase = mkterm.stock_base(gl, four, lh, de)[0]
    t = face.info['TEXT22']
    return {'ASCII26': ('TEXT22', (lh - de - sbase) - (t['line_h'] - t['descent'] - t['base']))}
