#!/usr/bin/env python3
"""Generate src/stockfont_w.rs: a real 'W' for the stock DMM fonts that lack
one (UNIT, LETTERS, SC_DIGITS = K_FONT, HIST), used by stockfont.rs
glyph_or_twin instead of the two overlapped V of before (owner 2026-10-22),
and a lowercase 'k' for LETTERS (the energy units "kWh" / "kVAh" of the
power page; stockfont.rs unit_text keeps 'k' there, "KΩ" stays as stock).

The stock faces are a Helvetica-style bold (of the installed faces FreeSans
Bold fits their A H K M N V best: mean |alpha| error 0.77..0.98 of 15). The
W is rasterised from that face the way the stock V of the same font maps
onto it: the W outline scaled by the font's V ink box / FreeSans V ink box
(x and y), area-averaged from a 400 px rendering, quantised to the 4 bpp
alpha (0..15); its box height, yoff and the advance-minus-width of the
font's V. The k likewise from the font's h (same ascender and stem: scale
by the h ink box, h's yoff), adv = w + 1 as the W (the font's h / K
advance-minus-width, 4 / 2, left a wider k-W gap than W-h in "kWh"). HIST (1 bpp pixel font, 2 px strokes) gets its M mirrored top to
bottom, the pixel-font W of the same family.

    /usr/bin/python3 tools/mkwglyph.py      (from fw-rs/; needs PIL + the face)
"""
import ast
import os
import re
import sys

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, '..', 'src')
FACE = '/usr/share/fonts/truetype/freefont/FreeSansBold.ttf'
SS = 400
ENT = re.compile(r'Entry\{code:0x([0-9A-F]+),off:(\d+),w:(\d+),h:(\d+),adv:(\d+),yoff:(-?\d+)\}')


def load(path):
    s = open(path).read()
    fonts = {}
    for m in re.finditer(r'pub static (\w+)_E: \[Entry; \d+\] = \[(.*?)\];', s, re.S):
        name = m.group(1)
        ents = [dict(code=int(a, 16), off=int(b), w=int(c), h=int(d), adv=int(e), yoff=int(f)) for a, b, c, d, e, f in ENT.findall(m.group(2))]
        key = 'pub static %s_B: &[u8] = b"' % name
        i = start = s.index(key) + len(key)
        while s[i] != '"':
            i += 2 if s[i] == '\\' else 1
        blob = ast.literal_eval('b"' + s[start:i] + '"')
        fd = re.search(r'pub static %s_FD: FontData = FontData\{line_h:(\d+),descent:(-?\d+),four:(\w+)' % name, s)
        fonts[name] = dict(ents=ents, blob=blob, four=fd.group(3) == 'true')
    return fonts


def glyph(f, code):
    for e in f['ents']:
        if e['code'] == code:
            px = [[0] * e['w'] for _ in range(e['h'])]
            for x in range(e['w']):
                for y in range(e['h']):
                    p = x * e['h'] + y
                    if f['four']:
                        b = f['blob'][e['off'] + p // 2]
                        px[y][x] = (b >> 4) if p % 2 == 0 else (b & 15)
                    else:
                        b = f['blob'][e['off'] + p // 8]
                        px[y][x] = 15 if b & (0x80 >> (p & 7)) else 0
            return e, px
    raise KeyError(code)


def nat(ch):
    f = ImageFont.truetype(FACE, SS)
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


def k_from_face(f):
    return from_face(f, 'k', 'h')


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


def main():
    fonts = load(os.path.join(SRC, 'stockfont_data.rs'))
    lines = ['// @generated by tools/mkwglyph.py - do not edit by hand.',
             '// Glyphs of our own for the stock DMM fonts without them (stockfont.rs glyph_or_twin):',
             '// (font object address, char, 4 bpp, w, h, adv, yoff, column-major bitmap).', '',
             'pub static OWN_GLYPHS: [(u32, char, bool, u8, u8, u8, i8, &[u8]); 6] = [']
    for name, addr, ch, how in [('UNIT', 0x0805ECB8, 'W', w_from_face), ('LETTERS', 0x0805DAB4, 'W', w_from_face),
                                ('K_FONT', 0x0805C5DC, 'W', w_from_face), ('HIST', 0x0806C3E8, 'W', w_from_m),
                                ('LETTERS', 0x0805DAB4, 'k', k_from_face), ('BTN_GLYPH', 0x0805A9FC, '\x09', acdc_from_btn)]:
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
