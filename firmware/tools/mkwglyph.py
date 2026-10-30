#!/usr/bin/env python3
"""Generate src/stockfont_w.rs: a real 'W' for the stock DMM fonts that lack
one (UNIT, LETTERS, SC_DIGITS = K_FONT, HIST), used by stockfont.rs
glyph_or_twin instead of the two overlapped V of before (owner 2026-10-22),
a lowercase 'k' for the unit fonts (UNIT, LETTERS, F21, K_FONT, HIST; owner
issue #136: "kW", "kΩ", not the stock "K") and an Ω for ASCII26 (the
dialogs).

The stock faces are a Helvetica-style bold (of the installed faces FreeSans
Bold fits their A H K M N V best: mean |alpha| error 0.77..0.98 of 15). The
W is rasterised from that face the way the stock V of the same font maps
onto it: the W outline scaled by the font's V ink box / FreeSans V ink box
(x and y), area-averaged from a 400 px rendering, quantised to the 4 bpp
alpha (0..15); its box height, yoff and the advance-minus-width of the
font's V. HIST (1 bpp pixel font, 2 px strokes) gets its M mirrored top to
bottom, the pixel-font W of the same family. Every k is the face's k
scaled by the font's K (k_from_cap: no taller than the caps, so a "kHz"
keeps the line's height); HIST's k is pixel art
(k_hist). ASCII26 (a regular weight) gets FreeSans' Ω scaled by its O.

    /usr/bin/python3 tools/mkwglyph.py      (from firmware/; needs PIL + the face)
"""
import ast
import os
import re
import sys

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, '..', 'src')
FACE = '/usr/share/fonts/truetype/freefont/FreeSansBold.ttf'
FACE_REG = '/usr/share/fonts/truetype/freefont/FreeSans.ttf'
SS = 400
ENT = re.compile(r'E\((0x[0-9A-F]+),(\d+),(\d+),(\d+),(\d+),(-?\d+),(\d+),(\d+)\)')
sys.path.insert(0, HERE)
from mkstockfont import unrle_glyph, uncrop_glyph  # noqa: E402


def load(path):
    """The fonts of src/stockfont_data.rs (mkstockfont.py's storage: E()
    entries, RLE4 / cropped 1 bpp blobs), each glyph decoded back to its
    full column-major box."""
    s = open(path).read()
    fonts = {}
    for m in re.finditer(r'pub static (\w+)_E: \[Entry; \d+\] = \[(.*?)\];', s, re.S):
        name = m.group(1)
        b = re.search(r'pub static %s_B: \[u8; \d+\] = \*b"(.*?)"\s*;' % name, s, re.S)
        blob = ast.literal_eval('b"' + b.group(1).replace('\\\n', '') + '"')
        fd = re.search(r'pub static %s_FD: FontData = FontData\{line_h:(\d+),descent:(-?\d+),fmt:(\w+)' % name, s)
        four = fd.group(3) == 'RLE4'
        ents = []
        for c, off, w, h, adv, yoff, t, ih in ENT.findall(m.group(2)):
            off, w, h, t, ih = int(off), int(w), int(h), int(t), int(ih)
            if four:
                px = unrle_glyph(blob[off:], w, h) if w and h else []
            else:
                px = uncrop_glyph(blob[off:], w, h, t, ih)
            ents.append(dict(code=int(c, 16), w=w, h=h, adv=int(adv), yoff=int(yoff), px=px))
        fonts[name] = dict(ents=ents, four=four)
    return fonts


def glyph(f, code):
    for e in f['ents']:
        if e['code'] == code:
            return e, [[e['px'][x * e['h'] + y] for x in range(e['w'])] for y in range(e['h'])]
    raise KeyError(code)


def nat(ch, face=None):
    f = ImageFont.truetype(face or FACE, SS)
    im = Image.new('L', (SS * 2, SS * 2), 0)
    ImageDraw.Draw(im).text((SS // 2, SS // 2), ch, font=f, fill=255)
    return im.crop(im.getbbox())


def from_face(f, ch, ref):
    """`ch` from FreeSans Bold, scaled by the stock `ref` glyph's ink box /
    FreeSans `ref` ink box (x and y); yoff of `ref`, adv = w + 1."""
    e, _ = glyph(f, ord(ref))
    r0, g = nat(ref), nat(ch)
    sx, sy = e['w'] / r0.size[0], e['h'] / r0.size[1]
    width = max(1, round(g.size[0] * sx))
    height = max(1, round(g.size[1] * sy))
    r = g.resize((width, height), Image.BOX)
    px = [[min(15, (r.getpixel((x, y)) + 8) // 17) for x in range(width)] for y in range(height)]
    return dict(w=width, h=height, adv=width + 1, yoff=e['yoff'], px=px)


def w_from_face(f):
    # adv = w + 1: a 1 px gap after the W at spacing 0 (the stock V has adv
    # w - 1, which overhangs; a W is followed by 'h' in the energy units).
    return from_face(f, 'W', 'V')


def k_from_cap(f, face=FACE):
    """'k' (owner #136: kW, not KW): the face's k scaled uniformly by the font's K ink height /
    the face's K ink height, standing on K's baseline, adv = w + 2 (its
    arm runs to the box edge: 1 px of air before the next glyph). HIST (1 bpp)
    font is drawn by hand instead (k_hist)."""
    e, _ = glyph(f, ord('K'))
    r0, g = nat('K', face), nat('k', face)
    s = e['h'] / r0.size[1]
    width, height = max(1, round(g.size[0] * s)), max(1, round(g.size[1] * s))
    r = g.resize((width, height), Image.BOX)
    px = [[min(15, (r.getpixel((x, y)) + 8) // 17) for x in range(width)] for y in range(height)]
    return dict(w=width, h=height, adv=width + 2, yoff=e['yoff'] + e['h'] - height, px=px)


# HIST (1 bpp pixel font, 2 px stems): its k drawn by hand on the K's 9 x 15
# box, the stem and diagonals of its K and n.
HIST_K = """
.........
##.......
##.......
##.......
##.......
##...##..
##..##...
##.##....
####.....
#####....
##.##....
##..##...
##...##..
##....##.
.........
"""


def k_hist(f):
    e, _ = glyph(f, ord('K'))
    rows = HIST_K.split()
    px = [[15 if c == '#' else 0 for c in r] for r in rows]
    return dict(w=len(rows[0]), h=len(rows), adv=e['adv'], yoff=e['yoff'], px=px)


def ohm_from_o(f):
    """Ω for ASCII26 (the dialogs; owner #136: "kΩ", not "ohm"): FreeSans
    (regular, the dialog font's weight) scaled by the font's O ink box,
    on O's baseline."""
    e, _ = glyph(f, ord('O'))
    r0, g = nat('O', FACE_REG), nat('\u03a9', FACE_REG)
    sx, sy = e['w'] / r0.size[0], e['h'] / r0.size[1]
    width, height = max(1, round(g.size[0] * sx)), max(1, round(g.size[1] * sy))
    r = g.resize((width, height), Image.BOX)
    px = [[min(15, (r.getpixel((x, y)) + 8) // 17) for x in range(width)] for y in range(height)]
    return dict(w=width, h=height, adv=e['adv'], yoff=e['yoff'] + e['h'] - height, px=px)


def w_from_m(f):
    e, px = glyph(f, ord('M'))
    # rows with ink, mirrored in place (the blank top / bottom rows stay)
    ink = [y for y in range(e['h']) if any(px[y])]
    a, b = ink[0], ink[-1]
    out = [row[:] for row in px]
    for y in range(a, b + 1):
        out[y] = px[a + b - y][:]
    return dict(w=e['w'], h=e['h'], adv=e['adv'], yoff=e['yoff'], px=out)


def scaled(f, code, w, h):
    """Stock glyph `code` of `f` area-averaged to w x h (4 bpp alpha)."""
    e, px = glyph(f, code)
    im = Image.new('L', (e['w'], e['h']))
    im.putdata([v * 17 for row in px for v in row])
    return im.resize((w, h), Image.BOX)


def acdc_from_btn(f):
    """The AUTO coupling's "AC/DC" mark for the V / A button: the stock
    BTN_GLYPH AC (0x05, 24 x 12) and DC (0x04, 24 x 8) glyphs area-averaged
    to 15 x 8 / 14 x 6, AC top left, DC bottom right, a 2 px slash between
    (34 x 16; yoff of the AC+DC glyph 0x06)."""
    W, H, K = 34, 16, 8
    im = Image.new('L', (W * K, H * K), 0)
    im.paste(scaled(f, 5, 15, 8).resize((15 * K, 8 * K), Image.NEAREST), (0, 0))
    im.paste(scaled(f, 4, 14, 6).resize((14 * K, 6 * K), Image.NEAREST), (20 * K, 10 * K))
    ImageDraw.Draw(im).line([(20 * K, 0), (13 * K, H * K)], fill=255, width=2 * K)
    r = im.resize((W, H), Image.BOX)
    px = [[min(15, (r.getpixel((x, y)) + 8) // 17) for x in range(W)] for y in range(H)]
    e, _ = glyph(f, 6)
    return dict(w=W, h=H, adv=W, yoff=e['yoff'] + 2, px=px)


def pack(g, four):
    bits = []
    for x in range(g['w']):
        for y in range(g['h']):
            bits.append(g['px'][y][x])
    out = bytearray()
    if four:
        for i in range(0, len(bits), 2):
            hi = bits[i]
            lo = bits[i + 1] if i + 1 < len(bits) else 0
            out.append(hi << 4 | lo)
    else:
        for i in range(0, len(bits), 8):
            b = 0
            for k, v in enumerate(bits[i:i + 8]):
                if v:
                    b |= 0x80 >> k
            out.append(b)
    return bytes(out)


OWN = [('UNIT', 0x0805ECB8, 'W', w_from_face), ('LETTERS', 0x0805DAB4, 'W', w_from_face),
       ('K_FONT', 0x0805C5DC, 'W', w_from_face), ('HIST', 0x0806C3E8, 'W', w_from_m),
       ('LETTERS', 0x0805DAB4, 'k', k_from_cap), ('BTN_GLYPH', 0x0805A9FC, '\x09', acdc_from_btn),
       ('UNIT', 0x0805ECB8, 'k', k_from_cap), ('F21', 0x080667F8, 'k', k_from_cap),
       ('K_FONT', 0x0805C5DC, 'k', k_from_cap), ('HIST', 0x0806C3E8, 'k', k_hist),
       ('ASCII26', 0x080592F0, '\u03a9', ohm_from_o)]


def main():
    fonts = load(os.path.join(SRC, 'stockfont_data.rs'))
    lines = ['// @generated by tools/mkwglyph.py - do not edit by hand.',
             '// Glyphs of our own for the stock DMM fonts without them (stockfont.rs glyph_or_twin):',
             '// (font object address, char, 4 bpp, w, h, adv, yoff, column-major bitmap).', '',
             'pub static OWN_GLYPHS: [(u32, char, bool, u8, u8, u8, i8, &[u8]); %d] = [' % len(OWN)]
    for name, addr, ch, how in OWN:
        f = fonts[name]
        g = how(f)
        b = pack(g, f['four'])
        cl = ch if ch.isprintable() else '\\u{%x}' % ord(ch)
        lines.append('    // %s %s' % (name, cl))
        for row in g['px']:
            lines.append('    //   ' + ''.join('.:-=+*#%@@'[min(9, v * 10 // 16)] for v in row))
        lines.append("    (0x%08X, '%s', %s, %d, %d, %d, %d, b\"%s\")," % (addr, cl, 'true' if f['four'] else 'false', g['w'], g['h'], g['adv'], g['yoff'],
                                                                   ''.join('\\x%02x' % c for c in b)))
    lines.append('];')
    open(os.path.join(SRC, 'stockfont_w.rs'), 'w').write('\n'.join(lines) + '\n')
    print('src/stockfont_w.rs written')


if __name__ == '__main__':
    sys.exit(main())
