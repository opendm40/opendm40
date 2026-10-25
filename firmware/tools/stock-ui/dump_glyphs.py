#!/usr/bin/env python3
"""Dump the glyphs of the stock DM40 V2.6 fonts used by the scope page as
individual PNGs (read-only; nothing in the firmware is changed).

    /usr/bin/python3 -I tools/stock-ui/dump_glyphs.py [--all] [dm40_v_2_6.dec.bin] [outdir]

Default source: ../../../dm40_v_2_6.dec.bin (12-byte header + body loaded at
0x08004000); default outdir: assets/glyphs next to this script.
Writes font_<addr>_<code>.png (grey, 0 = transparent, 255 = full ink, 4 bpp
fonts keep their 16 levels * 17), an index glyphs.txt with the metrics, and a
contact sheet per font (font_<addr>.png) with the hex code under each glyph.
--all dumps every font object found in the image instead of the scope list.

Font format (stock/re/analysis/gui_v2_6.h sec. 5, extract_fonts.py): font
object F = { D*, vtable*, u16 line_h, i16 descent }; D = { u8 *bits, entry
*tab, u32 n, u8 fmt }; class A entry (12 B) = { u32 code, u32 off, i8 yoff,
u8 w, u8 h, u8 adv }; class B entry (8 B) = { u32 code, u32 off<<8|w }, h =
line_h, adv = w, yoff = 0; fmt 4 = 4 bpp else 1 bpp; bitmap = column-major
MSB-first bit stream.
"""
import os
import struct
import sys

from PIL import Image, ImageDraw

BASE = 0x08000000
VT_A, VT_B, VT_BPLUS = 0x08023509, 0x080236F5, 0x08023425

# Fonts the scope page and its popups draw with (stock/re/stock-ui-notes/layout.md,
# graph.md, popups.md); name -> font object address.
SCOPE_FONTS = {
    0x0805B138: 'F14 small ASCII 1 bpp (labels)',
    0x080592F0: 'ASCII 26 px 4 bpp (scope/generator)',
    0x0805C5DC: 'digits + units 20 px',
    0x08065558: 'small icons / ASCII 12 px (marker glyphs?)',
    0x0805471C: 'ASCII 12 px (class B+ primary)',
    0x0804F24C: 'CJK 12 px (B+, ASCII from 0x0805471c)',
    0x08055014: 'trigger edge rising / falling 15 px',
    0x08059428: 'up / down chevrons 12 px',
    0x08054FC0: 'symbols 17 px',
    0x0804FA4C: 'battery 0x22..0x27',
    0x0806BD44: 'battery 1 bpp variant',
    0x0806B13C: 'settings / menu text 22 px',
    0x08054148: 'ASCII 20 px',
    0x080534C4: 'ASCII 16 px',
    0x08052BA0: 'CJK 16 px',
    0x0806B24C: 'arrows < >',
    0x0806B300: 'back arrow',
    0x0806C524: 'close X',
    0x0806C0A8: 'save (floppy) 26 px',
    0x0805B2E8: 'save / Bluetooth 21 px',
    0x0806BF24: 'padlock',
    0x080667F8: 'F21 status bar',
    0x0805DAB4: 'letters 30 px',
}


def load_image(path):
    body = open(path, 'rb').read()[12:]
    img = b'\xff' * 0x4000 + body
    return img + b'\xff' * (0x100000 - len(img))


def fonts_in(img):
    def w32(a):
        return struct.unpack_from('<I', img, a - BASE)[0]

    def in_app(a):
        return 0x08004000 <= a < 0x08080000

    out = {}
    for a in range(0x08040000, 0x0807D000, 4):
        D, vt = w32(a), w32(a + 4)
        if not (in_app(D) and in_app(vt)):
            continue
        f0 = w32(vt)
        if f0 not in (VT_A, VT_B, VT_BPLUS):
            continue
        bits, tab, n, fmt = w32(D), w32(D + 4), w32(D + 8), img[D + 12 - BASE]
        if not (in_app(bits) and in_app(tab) and 0 < n < 5000):
            continue
        line_h, desc = struct.unpack_from('<Hh', img, a + 8 - BASE)
        glyphs = []
        for i in range(n):
            if f0 == VT_A:
                code, off = w32(tab + 12 * i), w32(tab + 12 * i + 4)
                yoff, gw, gh, adv = struct.unpack_from('<bBBB', img, tab + 12 * i + 8 - BASE)
            else:
                code, v = w32(tab + 8 * i), w32(tab + 8 * i + 4)
                off, gw, gh, adv, yoff = v >> 8, v & 0xFF, line_h, v & 0xFF, 0
            glyphs.append((code, bits + off, gw, gh, adv, yoff))
        out[a] = ({VT_A: 'A', VT_B: 'B', VT_BPLUS: 'B+'}[f0], 4 if fmt == 4 else 1, line_h, desc, glyphs)
    return out


def glyph_image(img, addr, gw, gh, bpp):
    im = Image.new('L', (max(gw, 1), max(gh, 1)), 0)
    px = im.load()
    for p in range(gw * gh):
        bit = p * bpp
        byte = img[addr - BASE + bit // 8]
        v = (byte >> (8 - bpp - bit % 8)) & ((1 << bpp) - 1)
        px[p // gh, p % gh] = v * 255 // ((1 << bpp) - 1)
    return im


def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    allf = '--all' in sys.argv
    here = os.path.dirname(os.path.abspath(__file__))
    src = args[0] if args else os.path.join(here, '..', '..', '..', 'dm40_v_2_6.dec.bin')
    out = args[1] if len(args) > 1 else os.path.join(here, 'assets', 'glyphs')
    os.makedirs(out, exist_ok=True)
    img = load_image(src)
    fonts = fonts_in(img)
    want = fonts if allf else {a: fonts[a] for a in SCOPE_FONTS if a in fonts}
    idx = open(os.path.join(out, 'glyphs.txt'), 'w')
    for a, (cls, bpp, line_h, desc, glyphs) in sorted(want.items()):
        idx.write('font %08x class %-2s bpp %d line_h %d descent %d glyphs %d  %s\n' % (
            a, cls, bpp, line_h, desc, len(glyphs), SCOPE_FONTS.get(a, '')))
        cw = max(max(g[2] for g in glyphs), 20) + 6
        ch = max(g[3] for g in glyphs) + 16
        cols = max(1, min(len(glyphs), 1200 // cw))
        rows = (len(glyphs) + cols - 1) // cols
        sheet = Image.new('L', (cols * cw, rows * ch), 40)
        dr = ImageDraw.Draw(sheet)
        for k, (code, addr, gw, gh, adv, yoff) in enumerate(glyphs):
            gi = glyph_image(img, addr, gw, gh, bpp)
            gi.save(os.path.join(out, 'font_%08x_%02x.png' % (a, code)))
            idx.write('  %02x w %d h %d adv %d yoff %d\n' % (code, gw, gh, adv, yoff))
            x0, y0 = (k % cols) * cw, (k // cols) * ch
            dr.text((x0 + 1, y0), '%x' % code, fill=160)
            sheet.paste(gi, (x0 + 3, y0 + 13))
        sheet.save(os.path.join(out, 'font_%08x.png' % a))
    idx.close()
    print('%d fonts -> %s' % (len(want), out))


if __name__ == '__main__':
    main()
