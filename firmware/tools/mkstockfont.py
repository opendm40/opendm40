#!/usr/bin/env python3
"""Generate src/stockfont_data.rs: the stock DM40 bitmap fonts embedded in our
own image, so stockfont.rs no longer reads them from the stock app's absolute
flash addresses (which disappear once opendm40 is flashed over 0x08004000).

    /usr/bin/python3 -I tools/mkstockfont.py [dm40_v_2_6.dec.bin]

Default source: ../dm40_v_2_6.dec.bin (the decoded stock update file: a 12-byte
header + the body that is flashed at 0x08004000). A 1 MB flash view is
synthesised as 0xFF*0x4000 + body so an absolute address A maps to offset
A-0x08000000.

The parse mirrors src/stockfont.rs exactly (it used to read these same
structures live from flash): font object F = { D*, vtable*, u16 line_h,
i16 descent }; D = { u8 *bits, entry *tab, u32 n, u8 fmt }; class A entry (12 B)
= { u32 code, u32 off, i8 yoff, u8 w, u8 h, u8 adv }; class B entry (8 B)
= { u32 code, u32 off<<8|w }, h = line_h, adv = w, yoff = 0; fmt 4 = 4 bpp else
1 bpp; bitmap is a column-major MSB-first bit stream.

Emitted storage (docs/fonts.md; the drawn pixels stay the stock ones, checked
here by decoding every glyph back and by tools/font-matrix): glyphs from
U+2E80 up (CJK, full-width) are dropped; a 4 bpp glyph becomes row-major
nibble runs (rle_glyph), a 1 bpp glyph keeps only its ink rows (crop_glyph);
one blob per font as a named array in section .rodata.fonts.<NAME>_B, 10-byte
entries E(code, off, w, h, adv, yoff, t, ih) in .rodata.fonts.<NAME>_E.
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
ROOT = os.path.dirname(FW)

BASE = 0x08000000
VT_A, VT_B, VT_BPLUS, VT_COMP = 0x08023509, 0x080236F5, 0x08023425, 0x080237A5

# name -> font-object address, as in src/stockfont.rs. Order = emit order.
FONTS = [
    ('F78', 0x08065070),
    ('UNIT', 0x0805ECB8),
    ('LETTERS', 0x0805DAB4),
    ('F21', 0x080667F8),
    ('F14', 0x0805B138),
    ('HIST', 0x0806C3E8),
    ('ASCII26', 0x080592F0),
    ('AUX_TAG', 0x0805965C),
    ('MODE', 0x08059D00),
    ('BTN_GLYPH', 0x0805A9FC),
    ('BATTERY', 0x0804FA4C),
    ('LOCK', 0x0806BF24),
    ('SAVE_BT', 0x0805B2E8),
    ('ON_TAG', 0x0806C724),
    ('LIGHTNING', 0x0805A0E0),
    ('TEXT22', 0x0806B13C),
    # Fonts constructed inline by address in src/ui.rs (kept address-keyed).
    ('G_08059428', 0x08059428),  # 42x42 box glyph
    ('K_FONT', 0x0805C5DC),      # keypad font
    ('ICONS', 0x0805592C),       # menu icons
]


# Fonts whose bitmap blob is a named static in its own section
# `.rodata.a2.<NAME>`, which link-native.x places in region A2 (zero-wait
# flash between the shim and the loader marker, 0x0800460c..0x08008000;
# docs/native-boot.md "Region A2"). The other blobs are named statics in
# `.rodata.fonts.<NAME>_B`, region B. A2 holds ~14.8 KB; build.sh warns below 2 KiB free. ASCII26 is
# the logger panel's font (read per draw while the AD2 stream runs; device
# 5f14d38 showed it in slow flash stalling the ISR), F14 the small texts;
# LETTERS and UNIT since region B audit #4 (2026-10-25: B at 15 KB free, A2
# at 7.9 KB after the font rework; A2 keeps ~4 KB for the shim).
A2_FONTS = ('ASCII26', 'F14', 'LETTERS', 'UNIT')


# Codes from here up (CJK, full-width punctuation) are dropped: opendm40 draws
# English text only (no string in src/ has one; docs/fonts.md).
CJK_FROM = 0x2E80
# Widest glyph (F78 'O' = 64): the row buffer of stockfont.rs (MAXW there).
MAXW = 64


def pixels(data, w, h, four):
    """Coverage 0..15 per pixel of a stock bitmap, column-major (x * h + y)."""
    if four:
        return [(data[p >> 1] >> 4) if p % 2 == 0 else (data[p >> 1] & 15) for p in range(w * h)]
    return [15 if data[p >> 3] & (0x80 >> (p & 7)) else 0 for p in range(w * h)]


def pack_nibbles(nib):
    if len(nib) % 2:
        nib = nib + [0]
    return bytes((nib[i] << 4) | nib[i + 1] for i in range(0, len(nib), 2))


def rle_glyph(px, w, h):
    """4 bpp glyph -> row-major nibble RLE (stockfont.rs `Cur`), MSB nibble
    first, byte-aligned per glyph. Tokens: 1..14 = one pixel of that
    coverage; 0|15 then n: n = 0..13 -> n + 1 pixels of 0|15, n = 14 -> two
    more nibbles L: 15 + L pixels (15..270); 0 then 15, k (at a row start
    only): the previous literal row again k + 1 times. No run crosses a row."""
    rows = [[px[x * h + y] for x in range(w)] for y in range(h)]
    out = []
    prev = None
    y = 0
    while y < h:
        r = rows[y]
        if prev is not None and r == prev:
            k = 0
            while y + k + 1 < h and rows[y + k + 1] == prev and k < 15:
                k += 1
            out += [0, 15, k]
            y += k + 1
            continue
        i = 0
        while i < w:
            v = r[i]
            if v in (0, 15):
                j = i
                while j < w and r[j] == v and j - i < 270:
                    j += 1
                L = j - i
                out += [v, L - 1] if L <= 14 else [v, 14, (L - 15) >> 4, (L - 15) & 15]
                i = j
            else:
                out.append(v)
                i += 1
        prev = r
        y += 1
    return pack_nibbles(out)


def unrle_glyph(b, w, h):
    nib = [(c >> 4, c & 15)[k] for c in b for k in (0, 1)]
    pos = 0
    rows = []
    src = None
    while len(rows) < h:
        if nib[pos] == 0 and nib[pos + 1] == 15:
            rows += [rows[src]] * (nib[pos + 2] + 1)
            pos += 3
            continue
        r = []
        while len(r) < w:
            v = nib[pos]
            if v in (0, 15):
                n = nib[pos + 1]
                if n == 14:
                    L = 15 + (nib[pos + 2] << 4 | nib[pos + 3])
                    pos += 4
                else:
                    L = n + 1
                    pos += 2
                r += [v] * L
            else:
                r.append(v)
                pos += 1
        src = len(rows)
        rows.append(r)
    assert len(rows) == h
    return [rows[y][x] for x in range(w) for y in range(h)]


def crop_glyph(px, w, h):
    """1 bpp glyph -> its ink rows t .. t + ih only (same column-major
    MSB-first bit stream, ih rows per column). An inkless glyph: ih = 0."""
    ink = [y for y in range(h) if any(px[x * h + y] for x in range(w))]
    if not ink:
        return b'', 0, 0
    t, ih = ink[0], ink[-1] - ink[0] + 1
    bits = [px[x * h + y] for x in range(w) for y in range(t, t + ih)]
    out = bytearray((len(bits) + 7) // 8)
    for p, v in enumerate(bits):
        if v:
            out[p >> 3] |= 0x80 >> (p & 7)
    return bytes(out), t, ih


def uncrop_glyph(b, w, h, t, ih):
    return [15 if t <= y < t + ih and b[(x * ih + y - t) >> 3] & (0x80 >> ((x * ih + y - t) & 7)) else 0
            for x in range(w) for y in range(h)]


def main():
    src = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, 'dm40_v_2_6.dec.bin')
    body = open(src, 'rb').read()[12:]
    img = b'\xff' * 0x4000 + body

    def w32(a):
        return struct.unpack_from('<I', img, a - BASE)[0]

    def u16(a):
        return struct.unpack_from('<H', img, a - BASE)[0]

    def i16(a):
        return struct.unpack_from('<h', img, a - BASE)[0]

    def i8(a):
        return struct.unpack_from('<b', img, a - BASE)[0]

    def u8(a):
        return img[a - BASE]

    out = []
    out.append('// @generated by tools/mkstockfont.py from dm40_v_2_6 - do not edit by hand.')
    out.append('// The stock bitmap fonts, embedded so stockfont.rs needs no stock app in flash.')
    out.append('')

    def load(a):
        """Glyphs of the font object at `a`: (line_h, descent, four, [(code, w, h, adv, yoff, bytes)])."""
        D, vt = w32(a), w32(a + 4)
        f0 = w32(vt)
        if f0 not in (VT_A, VT_B, VT_BPLUS):
            raise SystemExit('@%08x: unexpected vtable %08x (composite/unknown not supported)' % (a, f0))
        class_a = (f0 == VT_A)
        bits, tab, n, fmt = w32(D), w32(D + 4), w32(D + 8), u8(D + 12)
        line_h, descent = u16(a + 8), i16(a + 10)
        four = (fmt == 4)
        bpp = 4 if four else 1
        gl = []
        prev_code = None
        for i in range(n):
            if class_a:
                code = w32(tab + 12 * i)
                off = w32(tab + 12 * i + 4)
                yoff = i8(tab + 12 * i + 8)
                gw = u8(tab + 12 * i + 9)
                gh = u8(tab + 12 * i + 10)
                adv = u8(tab + 12 * i + 11)
            else:
                code = w32(tab + 8 * i)
                v = w32(tab + 8 * i + 4)
                off = v >> 8
                gw = v & 0xFF
                gh = line_h
                adv = gw
                yoff = 0
            if prev_code is not None and code <= prev_code:
                raise SystemExit('@%08x: glyph table not strictly ascending at code %x' % (a, code))
            prev_code = code
            span = (gw * gh * bpp + 7) // 8
            abs_bits = bits + off
            data = img[abs_bits - BASE: abs_bits - BASE + span]
            if len(data) != span:
                raise SystemExit('@%08x code %x: bitmap out of image' % (a, code))
            gl.append((code, gw, gh, adv, yoff, bytes(data)))
        return line_h, descent, four, gl, ('A' if class_a else 'B')

    def load_class_c():
        """Class C font 0x08054828 (vtable 0x08054820, FUN_0802362c/FUN_080235bc):
        fixed 1 bpp bitmaps, ' (4x16) at 0x08054728, R U N S T O P W A I D H
        (10x16, 20 bytes each) from 0x08054730; line_h 16, descent 0."""
        gl = []
        for i, ch in enumerate("'RUNSTOPWAIDH"):
            if i == 0:
                a, w = 0x08054728, 4
            else:
                a, w = 0x08054730 + 20 * (i - 1), 10
            gl.append((ord(ch), w, 16, w, 0, img[a - BASE: a - BASE + (w * 16 + 7) // 8]))
        gl.sort()
        return 16, 0, False, gl, 'C'

    def load_merged(first, second):
        """B+ font: every code of `first` (0x0805471c) and, for the codes it
        lacks, the font's own table (`second`), as FUN_08023425 looks them up.
        Only codes < 0x80 of the own table (the CJK part is not used)."""
        lh, de, four, g1, _ = load(first)
        lh2, de2, four2, g2, _ = load(second)
        assert (lh, four) == (lh2, four2)
        have = set(g[0] for g in g1)
        gl = sorted(g1 + [g for g in g2 if g[0] < 0x80 and g[0] not in have])
        return lh, de, four, gl, 'B+'

    total_blob = 0
    total_glyphs = 0
    dropped = [0, 0]
    raw_total = [0]
    summary = []
    fonts = [(name, a, (lambda a=a: load(a))) for name, a in FONTS]
    fonts += [(name, key, src) for name, key, src in [
        # Stock scope page fonts (stock/re/docs/stock-scope-ui.md sec. 0.1), keyed by
        # the font-object address the stock page uses.
        ('SC_C', 0x08054828, load_class_c),                                  # status letters, "H"
        ('SC_F12', 0x0804F24C, lambda: load_merged(0x0805471C, 0x0804F24C)),  # 12 px texts
        ('SC_F20', 0x08054148, lambda: load(0x08054148)),                     # panel buttons
        ('SC_PROBE', 0x08054FC0, lambda: load(0x08054FC0)),                   # coupling / probe glyph
        ('SC_EDGE', 0x08055014, lambda: load(0x08055014)),                    # trigger edge
        ('SC_MARK', 0x08065558, lambda: load(0x08065558)),                    # 12x12 markers
        ('SC_BATT', 0x0806BD44, lambda: load(0x0806BD44)),                    # battery
        ('SC_F16', 0x0804F260, lambda: load(0x080534C4)),                     # 16 px popup ASCII (wrapper)
    ]]
    for name, a, src in fonts:
        line_h, descent, four, gl, cls = src()
        bpp = 4 if four else 1
        blob = bytearray()
        entries = []
        for (code, gw, gh, adv, yoff, data) in gl:
            if code >= CJK_FROM:
                dropped[0] += 1
                dropped[1] += len(data)
                continue
            px = pixels(data, gw, gh, four)
            if four:
                enc = rle_glyph(px, gw, gh)
                assert unrle_glyph(enc, gw, gh) == px, (name, code)
                t, ih = 0, gh
            else:
                enc, t, ih = crop_glyph(px, gw, gh)
                assert uncrop_glyph(enc, gw, gh, t, ih) == px, (name, code)
            assert len(blob) < 0x10000 and code < 0x10000 and gw <= MAXW, (name, code)
            entries.append((code, len(blob), gw, gh, adv, yoff, t, ih))
            blob += enc
            raw_total[0] += len(data)
        n = len(entries)

        total_blob += len(blob)
        total_glyphs += n
        summary.append((name, cls, bpp, line_h, descent, n, len(blob)))

        # Emit the glyph entry table.
        out.append('#[link_section = ".rodata.fonts.%s_E"]' % name)
        out.append('pub static %s_E: [Entry; %d] = [' % (name, n))
        line = '    '
        for (code, noff, gw, gh, adv, yoff, t, ih) in entries:
            cell = 'E(0x%X,%d,%d,%d,%d,%d,%d,%d),' % (code, noff, gw, gh, adv, yoff, t, ih)
            if len(line) + len(cell) > 116:
                out.append(line)
                line = '    '
            line += cell
        if line.strip():
            out.append(line)
        out.append('];')
        # Emit the bitmap blob as a wrapped byte-string literal, a named
        # [u8; N] in its own section: A2 fonts `.rodata.a2.<NAME>`
        # (link-native.x routes it to region A2), the others
        # `.rodata.fonts.<NAME>_B` (region B).
        if name in A2_FONTS and len(blob) > 0:
            out.append('#[link_section = ".rodata.a2.%s"]' % name)
        else:
            out.append('#[link_section = ".rodata.fonts.%s_B"]' % name)
        out.append('pub static %s_B: [u8; %d] = *b"\\' % (name, len(blob)))
        esc = ''.join('\\x%02x' % b for b in blob)
        for j in range(0, len(esc), 128):
            cont = '\\' if j + 128 < len(esc) else '"'
            out.append(esc[j:j + 128] + cont)
        if len(esc) == 0:
            out[-1] = 'pub static %s_B: [u8; 0] = *b"";' % name
        else:
            out.append(';')
        # Emit the FontData.
        out.append('pub static %s_FD: FontData = FontData{line_h:%d,descent:%d,fmt:%s,entries:&%s_E,blob:&%s_B};'
                   % (name, line_h, descent, 'RLE4' if four else 'BIT1', name, name))
        out.append('')

    # Address -> embedded FontData dispatch, so stockfont::Font can stay keyed
    # by the original stock flash address (no call site changes).
    out.append('/// Resolve a stock font-object address to its embedded data.')
    out.append('pub fn font_data(addr: u32) -> Option<&\'static FontData> {')
    out.append('    match addr {')
    for name, a, _ in fonts:
        out.append('        0x%08X => Some(&%s_FD),' % (a, name))
    out.append('        _ => None,')
    out.append('    }')
    out.append('}')
    out.append('')

    open(os.path.join(FW, 'src', 'stockfont_data.rs'), 'w').write('\n'.join(out) + '\n')

    # Self-check report.
    print('fonts: %d  glyphs: %d  blob bytes: %d (raw %d; dropped %d CJK glyphs, %d raw bytes)'
          % (len(fonts), total_glyphs, total_blob, raw_total[0], dropped[0], dropped[1]))
    for (name, cls, bpp, lh, desc, n, nb) in summary:
        print('  %-9s class %-2s %dbpp line_h=%-3d descent=%-2d glyphs=%-4d blob=%d'
              % (name, cls, bpp, lh, desc, n, nb))


if __name__ == '__main__':
    main()
