#!/usr/bin/env python3
"""Re-rasterise the stock text fonts in an outline face (docs/fonts.md
"Another face"), for tools/mkstockfont.py --face FACE.ttf.

Every glyph keeps its stock horizontal metrics (box width w, advance adv),
so the layout of every screen stays the same; the face is drawn into that
box:

* size: the face's reference glyph ('H', else '0', '8') gets the ink height
  of the stock one (the cap / digit height of the font);
* baseline: the reference glyph's ink bottom on the stock one's row;
* width: the face's ink centred on the stock glyph's ink, condensed when it
  is wider than the box allows (class A: the whole box, the stock boxes
  are tight; class B / C: the stock ink or the box less 1 px a side);
* class A boxes take the new ink rows (h, yoff), clipped to the line box;
  class B / C keep the line-high box;
* 4 bpp: area coverage from an SS x SS supersampled rendering, 0..15;
  1 bpp: covered when at least half the pixel is.

Glyphs below U+0020 (icon slots) and those the face lacks keep the stock
pixels. `restyle` works on mkstockfont.py's loaded tuples and returns them in
the stock bitmap format (column-major, 4 bpp high nibble first or 1 bpp MSB
first), so the encoder and its decode-back asserts run unchanged.

    tools/mkstockfont.py --face /path/Comfortaa-Bold.ttf
"""

import ast
import os
import re

import numpy as np
from fontTools.ttLib import TTFont
from PIL import Image, ImageDraw, ImageFont

SS = 8
# Fonts drawn in the face (the text fonts; icons stay stock).
FACE_FONTS = ('F78', 'UNIT', 'LETTERS', 'F21', 'F14', 'HIST', 'ASCII26', 'TEXT22',
              'K_FONT', 'SC_C', 'SC_F12', 'SC_F20', 'SC_F16')
REFS = 'H08EA'


def unpack(data, w, h, four):
    """Stock bitmap -> coverage array [h][w] (0..15)."""
    a = np.zeros((h, w), dtype=np.int32)
    for x in range(w):
        for y in range(h):
            p = x * h + y
            if four:
                v = data[p >> 1]
                a[y, x] = (v >> 4) if p % 2 == 0 else (v & 15)
            else:
                a[y, x] = 15 if data[p >> 3] & (0x80 >> (p & 7)) else 0
    return a


def pack(a, four):
    """Coverage array [h][w] -> stock bitmap bytes."""
    h, w = a.shape
    if four:
        out = bytearray((w * h + 1) // 2)
        for x in range(w):
            for y in range(h):
                p = x * h + y
                v = int(a[y, x])
                out[p >> 1] |= (v << 4) if p % 2 == 0 else v
    else:
        out = bytearray((w * h + 7) // 8)
        for x in range(w):
            for y in range(h):
                p = x * h + y
                if a[y, x]:
                    out[p >> 3] |= 0x80 >> (p & 7)
    return bytes(out)


def ink_rows(a):
    r = np.nonzero(a.any(axis=1))[0]
    return (int(r[0]), int(r[-1]) + 1) if len(r) else None


def ink_cols(a):
    c = np.nonzero(a.any(axis=0))[0]
    return (int(c[0]), int(c[-1]) + 1) if len(c) else None


class Face:
    def __init__(self, path):
        self.path = path
        self.cmap = TTFont(path).getBestCmap()
        self._f = {}

    def has(self, ch):
        return ord(ch) in self.cmap

    def font(self, px):
        k = round(px * SS, 3)
        if k not in self._f:
            self._f[k] = ImageFont.truetype(self.path, size=k)
        return self._f[k]

    def mask(self, ch, px):
        """Supersampled ink of `ch` at em size `px`: (array [H][W] 0..1,
        origin x, origin y = the baseline) in supersampled pixels."""
        f = self.font(px)
        l, t, r, b = f.getbbox(ch, anchor='ls')
        pad = 2
        W, H = max(1, r - l + 2 * pad), max(1, b - t + 2 * pad)
        im = Image.new('L', (W, H), 0)
        ImageDraw.Draw(im).text((pad - l, pad - t), ch, font=f, fill=255, anchor='ls')
        return np.asarray(im, dtype=np.float64) / 255.0, pad - l, pad - t

    def ink(self, ch, px):
        """Ink box of `ch` at `px` in target pixels relative to the origin /
        baseline: (x0, x1, y0, y1), None when blank."""
        m, ox, oy = self.mask(ch, px)
        r, c = np.nonzero(m > 0.02)
        if not len(r):
            return None
        return ((c.min() - ox) / SS, (c.max() + 1 - ox) / SS, (r.min() - oy) / SS, (r.max() + 1 - oy) / SS)


def rel_rows(h, yoff):
    """Baseline-relative y of box row 0 (stockfont.rs: top = baseline - h - yoff)."""
    return -h - yoff


def font_scale(face, gl):
    """(em size px, baseline shift) of the face for a stock font's glyphs."""
    by = {chr(g[0]): g for g in gl if 0x20 <= g[0] < 0x110000}
    for ref in REFS:
        g = by.get(ref)
        if g and face.has(ref):
            break
    else:
        return None
    code, gw, gh, adv, yoff, data, four = g
    rows = ink_rows(unpack(data, gw, gh, four))
    if rows is None:
        return None
    y0 = rel_rows(gh, yoff)
    top, bot = y0 + rows[0], y0 + rows[1]
    want = bot - top
    # Ink height is linear in the em size: one probe, then one correction.
    px = 100.0
    i = face.ink(ref, px)
    px = px * want / (i[3] - i[2])
    i = face.ink(ref, px)
    px = px * want / (i[3] - i[2])
    i = face.ink(ref, px)
    return px, bot - i[3]


def render(face, ch, px, dy, gw, box_top, box_h, cx, maxw, vlim):
    """Coverage 0..1 [box_h][gw] of `ch`: ink centred on column cx, at most
    maxw wide (condensed), baseline shifted by dy, inside the
    baseline-relative rows vlim (top, bottom; scaled about the baseline),
    box rows from the baseline-relative y box_top."""
    m, ox, oy = face.mask(ch, px)
    r, c = np.nonzero(m > 0.0)
    if not len(r):
        return np.zeros((box_h, gw))
    c0, c1 = c.min(), c.max() + 1
    r0, r1 = r.min(), r.max() + 1
    m = m[r0:r1, c0:c1]
    # Baseline-relative ink rows after the shift; then inside the stock
    # glyph's ink rows `vlim`: scaled down to their height when taller,
    # else moved in (a round bottom's overshoot, a higher hyphen).
    yt, yb = (r0 - oy) / SS + dy, (r1 - oy) / SS + dy
    vt, vb = vlim
    if yb - yt > vb - vt:
        nh = max(1, int(round((vb - vt) * SS)))
        m = np.asarray(Image.fromarray((m * 255).astype(np.uint8)).resize((m.shape[1], nh), Image.BOX), dtype=np.float64) / 255.0
        yt = vt
    elif yt < vt:
        yt = vt
    elif yb > vb:
        yt = vb - (yb - yt)
    # Whole rows too: the ink's top on a pixel edge inside the stock rows
    # (an AA row half above / below would reach a neighbour's margin).
    hh = m.shape[0] / SS
    yt = float(int(round(yt)))
    yt = min(max(yt, vt), max(vt, vb - hh))
    # Exactly the stock ink width (the gaps to the neighbours, and so the
    # word gaps against the letter gaps, stay the stock ones).
    nw = max(1, int(round(maxw * SS)))
    if nw != m.shape[1]:
        m = np.asarray(Image.fromarray((m * 255).astype(np.uint8)).resize((nw, m.shape[0]), Image.BOX), dtype=np.float64) / 255.0
    wpx = nw / SS
    # On whole pixels: the stock ink's first column (the width is the stock
    # one), so the coverage never spills into a neighbour's gap.
    left = float(int(round(cx - wpx / 2)))
    left = min(max(left, 0.0), max(0.0, gw - wpx))
    canvas = np.zeros((box_h * SS, gw * SS))
    x0 = int(round(left * SS))
    # Mask row 0 (cropped, maybe scaled) is at baseline-relative y yt.
    y0 = int(round((yt - box_top) * SS))
    h, w = m.shape
    ya, yb = max(0, y0), min(box_h * SS, y0 + h)
    xa, xb = max(0, x0), min(gw * SS, x0 + w)
    if ya < yb and xa < xb:
        canvas[ya:yb, xa:xb] = m[ya - y0:yb - y0, xa - x0:xb - x0]
    return canvas.reshape(box_h, SS, gw, SS).mean(axis=(1, 3))


def glyph_in_face(face, sc, line_h, descent, four, cls, text, gw, gh, adv, yoff, data, vlim):
    """One glyph box (stock metrics, stock bitmap `data` for the ink
    centre) drawn as `text` in the face: (gw, h, adv, yoff, bytes), None
    to keep the stock one."""
    px, dy = sc
    line_top, line_bot = -(line_h - descent), descent
    a = unpack(data, gw, gh, four)
    # The stock ink's solid columns (coverage >= 1/4): its faint AA edge
    # columns stay gaps, where the face's denser edges would make two
    # neighbours touch (ui-render's glyph-gap check).
    cols = ink_cols(a >= 4) or ink_cols(a)
    if cols is None:
        return None
    sw = cols[1] - cols[0]
    cx = (cols[0] + cols[1]) / 2
    # The stock glyph's ink box: its width, and its rows (baseline-relative)
    # unless `vlim` is given (own glyphs: the font's).
    maxw = sw
    rows = ink_rows(a)
    y0 = rel_rows(gh, yoff)
    if vlim is None:
        vlim = (y0 + rows[0], y0 + rows[1])
    if cls == 'A':
        top, h = line_top, line_bot - line_top
    else:
        top, h = rel_rows(gh, yoff), gh
    cov = render(face, text, px, dy, gw, top, h, cx, maxw, vlim)
    if four:
        q = np.clip(np.rint(cov * 15), 0, 15).astype(np.int32)
    else:
        q = np.where(cov >= 0.5, 15, 0).astype(np.int32)
    if cls == 'A':
        rows = ink_rows(q)
        if rows is None:
            return None
        q = q[rows[0]:rows[1]]
        return (gw, rows[1] - rows[0], adv, -(top + rows[1]), pack(q, four))
    return (gw, gh, adv, yoff, pack(q, four))


def stock_vlim(gl, four):
    """Baseline-relative ink rows (top, bottom) of all the font's stock
    text glyphs (codes from '!')."""
    top, bot = 0, 0
    for (code, gw, gh, adv, yoff, data) in gl:
        if code < 0x21 or not gw:
            continue
        rows = ink_rows(unpack(data, gw, gh, four))
        if rows:
            y0 = rel_rows(gh, yoff)
            top, bot = min(top, y0 + rows[0]), max(bot, y0 + rows[1])
    return top, bot


def restyle_font(face, line_h, descent, four, gl, cls):
    """mkstockfont.py loaded glyph list -> (the same in the face, scale)."""
    tagged = [(c, w, h, a, y, d, four) for (c, w, h, a, y, d) in gl]
    sc = font_scale(face, tagged)
    if sc is None:
        return gl, None
    sc = sc + (stock_vlim(gl, four),)
    out = []
    for (code, gw, gh, adv, yoff, data) in gl:
        ch = chr(code) if code < 0x110000 else ''
        g = None
        if code >= 0x21 and face.has(ch) and gw:
            g = glyph_in_face(face, sc[:2], line_h, descent, four, cls, ch, gw, gh, adv, yoff, data, None)
        out.append((code,) + g if g else (code, gw, gh, adv, yoff, data))
    return out, sc


def restyle(face_path, loaded):
    """mkstockfont.py `loaded` [(name, key, (line_h, descent, four, gl, cls))]
    with FACE_FONTS drawn in the face; and {key: (name, line_h, descent,
    cls, scale)} of the restyled fonts (`own_glyphs`)."""
    face = Face(face_path)
    res, fonts = [], {}
    for name, key, (line_h, descent, four, gl, cls) in loaded:
        if name in FACE_FONTS:
            gl, sc = restyle_font(face, line_h, descent, four, gl, cls)
            if sc:
                fonts[key] = (name, line_h, descent, cls, sc)
        res.append((name, key, (line_h, descent, four, gl, cls)))
    return res, fonts


OWN = re.compile(r"    \((0x[0-9A-F]+), '(.*?)', (true|false), (\d+), (\d+), (\d+), (-?\d+), b\"(.*?)\"\),")
# Own glyphs drawn as these texts (the rest, icons, stay).
OWN_TEXT = {'W': 'W', 'k': 'k', '\u03a9': '\u03a9', '\u2103': '\u00b0C', '\u2109': '\u00b0F'}


def art(q, four):
    ramp = ' .:-=+*#%@'
    h, w = q.shape
    return ['    //   ' + ''.join((ramp[min(9, int(q[y, x]) * 10 // 16)] if four else ('@' if q[y, x] else '.')) for x in range(w)) for y in range(h)]


def own_glyphs(face_path, fonts, path):
    """Rewrite the W / k / Omega / degree glyphs of stockfont_w.rs (mkwglyph.py's
    table, `glyph_or_twin`) in the face, with the stock boxes' widths."""
    face = Face(face_path)
    src = open(path).read()
    lines = src.split('\n')
    out = []
    i = 0
    while i < len(lines):
        ln = lines[i]
        m = OWN.match(ln)
        if not m:
            out.append(ln)
            i += 1
            continue
        key, ch, four, w, h, adv, yoff, esc = m.groups()
        # Rust char literal: 'W', 'Ω' or '\u{9}'.
        ch = chr(int(ch[3:-1], 16)) if ch.startswith('\\u{') else ch
        four = four == 'true'
        w, h, adv, yoff = int(w), int(h), int(adv), int(yoff)
        data = ast.literal_eval('b"' + esc + '"')
        f = fonts.get(int(key, 16))
        text = OWN_TEXT.get(ch)
        g = None
        if f and text and all(face.has(c) for c in text):
            name, line_h, descent, cls, sc = f
            g = glyph_in_face(face, sc[:2], line_h, descent, four, cls, text, w, h, adv, yoff, data, None)
        if g is None:
            out.append(ln)
            i += 1
            continue
        nw, nh, nadv, nyoff, b = g
        # The comment block above (name + art) is replaced too.
        while out and out[-1].startswith('    //   '):
            out.pop()
        out.append(art(unpack(b, nw, nh, four), four) and '\n'.join(art(unpack(b, nw, nh, four), four)))
        out.append("    (%s, '%s', %s, %d, %d, %d, %d, b\"%s\")," % (
            key, m.group(2), 'true' if four else 'false', nw, nh, nadv, nyoff, ''.join('\\x%02x' % v for v in b)))
        i += 1
    s = '\n'.join(out)
    s = s.replace('// @generated by tools/mkwglyph.py', '// @generated by tools/mkwglyph.py (W / k / \u03a9 / \u2103 / \u2109: mkfaceglyph.py %s)' % os.path.basename(face_path), 1)
    open(path, 'w').write(s)
