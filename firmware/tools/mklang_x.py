"""Glyphs for the alphabetic languages of tools/mklang.py (ru, it, fr).

Latin accents: the stock base letter of each font plus an accent (ACC), so the
face stays the stock one. Cyrillic: letters shaped like a Latin one are that
stock glyph (the text holds the ASCII byte); the others are rendered from
Liberation Sans (SIL OFL 1.1, i18n/OFL-Liberation.txt; the font file is not in
the repo, Debian: fonts-liberation) at the size that best matches the stock
face (fit_size). Per font only the characters its strings use: i18n/font-use.tsv
(from ui-render with LANG_LOG); a translation whose glyph is missing in its
font stays English at run time. 1 bpp glyphs are BIT1, 4 bpp HUF with the stock
fonts' shared tables (tools/mkstockfont.py).

One byte per letter, in valid UTF-8: the most frequent non-ASCII letters take
the control codes (CTRL), which no text font has, so a translation stays a &str
and the draw layer needs no decoder. The others stay two bytes.
"""
import collections
import os
import re
import struct

import mkstockfont as msf

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
ROOT = os.path.dirname(FW)
BASE = 0x08000000
VT_A, VT_B, VT_BP = 0x08023509, 0x080236F5, 0x08023425
LIB = '/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf'

# Font key -> the stock object its ASCII comes from, name.
FONTS = {
    0x0806B13C: (0x0806B13C, 'TEXT22'),
    0x0804F260: (0x080534C4, 'SC_F16'),
    0x08054148: (0x08054148, 'SC_F20'),
    0x0805B138: (0x0805B138, 'F14'),
    0x0804F24C: (0x0805471C, 'SC_F12'),
    0x080592F0: (0x080592F0, 'ASCII26'),
}
# Cyrillic letters drawn with the Latin glyph.
LOOKALIKE = dict(zip('АВЕКМНОРСТХаеорсух', 'ABEKMHOPCTXaeopcyx'))

# Accent shapes: (small fonts line_h <= 16, large fonts).
ACC = {
    'grave': (['#..', '.#.'], ['##...', '.##..', '..##.']),
    'acute': (['..#', '.#.'], ['...##', '..##.', '.##..']),
    'circ': (['.#.', '#.#'], ['..#..', '.###.', '##.##']),
    'diaer': (['#.#'], ['##.##', '##.##']),
    'ced': (['.#.', '.##'], ['..##.', '...##', '.###.']),
}
COMPOSE = {
    'à': ('a', 'grave'), 'â': ('a', 'circ'), 'ç': ('c', 'ced'), 'è': ('e', 'grave'), 'é': ('e', 'acute'),
    'ê': ('e', 'circ'), 'ë': ('e', 'diaer'), 'ì': ('i', 'grave'), 'î': ('i', 'circ'), 'ï': ('i', 'diaer'),
    'ò': ('o', 'grave'), 'ô': ('o', 'circ'), 'ù': ('u', 'grave'), 'û': ('u', 'circ'), 'ü': ('u', 'diaer'),
    'À': ('A', 'grave'), 'É': ('E', 'acute'), 'È': ('E', 'grave'), 'Ê': ('E', 'circ'), 'Ç': ('C', 'ced'),
}


# Tab, line feed and carriage return are left out. 0x7F is a glyph in the fonts.
CTRL = [c for c in range(1, 0x20) if c not in (9, 10, 13)]


def stock_image():
    d = open(os.path.join(ROOT, 'dm40_v_2_6.dec.bin'), 'rb').read()
    return b'\xff' * 0x4000 + d[12:]


def load(img, addr):
    """{code: (w, h, adv, yoff, coverage[y][x])}, line_h, descent, four"""
    w32 = lambda a: struct.unpack_from('<I', img, a - BASE)[0]
    D, vt = w32(addr), w32(addr + 4)
    f0 = w32(vt)
    bits, tab, n, four = w32(D), w32(D + 4), w32(D + 8), img[D + 12 - BASE] == 4
    lh, de = struct.unpack_from('<Hh', img, addr + 8 - BASE)
    out = {}
    for i in range(n):
        if f0 == VT_A:
            code, off = w32(tab + 12 * i), w32(tab + 12 * i + 4)
            yo, w, h, adv = struct.unpack_from('<bBBB', img, tab + 12 * i + 8 - BASE)
        else:
            code, v = w32(tab + 8 * i), w32(tab + 8 * i + 4)
            off, w, h, adv, yo = v >> 8, v & 255, lh, v & 255, 0
        if code >= 0x80:
            continue
        cov = [[0] * w for _ in range(h)]
        base = bits - BASE + off
        for p in range(w * h):
            if four:
                b = img[base + (p >> 1)]
                a = (b >> 4) if p % 2 == 0 else b & 15
            else:
                a = 15 if img[base + (p >> 3)] & (0x80 >> (p & 7)) else 0
            cov[p % h][p // h] = a
        out[code] = (w, h, adv, yo, cov)
    return out, lh, de, four


def ink_rows(cov):
    r = [y for y, row in enumerate(cov) if any(row)]
    return (r[0], r[-1]) if r else (0, -1)


def line_rows(g, lh, de):
    """A stock glyph as a line_h tall matrix (top = lh - de - h - yoff)."""
    w, h, adv, yo, cov = g
    top = lh - de - h - yo
    m = [[0] * w for _ in range(lh + 4)]
    for y in range(h):
        if 0 <= y + top < lh + 4:
            m[y + top] = list(cov[y])
    return m


def compose(ch, f, lh, de):
    bc, acc = COMPOSE[ch]
    m = line_rows(f[ord(bc)], lh, de)
    w = f[ord(bc)][0]
    if bc == 'i':  # drop the dot: the ink above the gap
        t, b = ink_rows(m)
        y = t
        while any(m[y]):
            m[y] = [0] * w
            y += 1
    t, b = ink_rows(m)
    xs = [x for row in m for x, a in enumerate(row) if a]
    cx = (min(xs) + max(xs)) // 2
    shape = ACC[acc][0 if lh <= 16 else 1]
    ah, aw = len(shape), len(shape[0])
    ay = b + 1 if acc == 'ced' else t - ah - 1
    if ay < 0:
        ay = 0
    x0 = max(0, cx - aw // 2)
    w = max(w, x0 + aw)
    m = [r + [0] * (w - len(r)) for r in m]
    deep = max(ink_rows(line_rows(f[ord(c)], lh, de))[1] for c in 'gpy' if ord(c) in f)
    for y, row in enumerate(shape):
        for x, c in enumerate(row):
            if c == '#' and ay + y <= deep:
                m[ay + y][x0 + x] = 15
    return m, w, f[ord(bc)][2]


def fit_size(f, lh, de, four):
    """The size where Liberation's a e o p c matches the stock glyphs best."""
    pairs = 'aeopcxH'
    best = None
    for s in range(8, 40):
        err = 0
        for ch in pairs:
            m, w, _ = render(ch, s, f, lh, de, four, 0)
            if ord(ch) not in f:
                continue
            sm = line_rows(f[ord(ch)], lh, de)
            for y in range(lh):
                for x in range(max(w, f[ord(ch)][0])):
                    a = m[y][x] if x < len(m[y]) else 0
                    b = sm[y][x] if x < len(sm[y]) else 0
                    err += abs(a - b)
        if best is None or err < best[0]:
            best = (err, s)
    return best[1]


def baseline(f, lh, de):
    t, b = ink_rows(line_rows(f[ord('n')], lh, de))
    return b


def render(ch, size, f, lh, de, four, _):
    from PIL import Image, ImageDraw, ImageFont
    font = ImageFont.truetype(LIB, size)
    b = baseline(f, lh, de)
    w = int(font.getlength(ch) + 0.5) + 1
    mode = 'L' if four else '1'
    im = Image.new(mode, (w, lh + 4), 0)
    d = ImageDraw.Draw(im)
    if not four:
        d.fontmode = '1'
    d.text((0, b + 1), ch, font=font, fill=255 if four else 1, anchor='ls')
    px = im.load()
    m = [[(px[x, y] * 15 + 127) // 255 if four else (15 if px[x, y] else 0) for x in range(w)] for y in range(lh + 4)]
    while w > 1 and not any(row[w - 1] for row in m):
        w -= 1
    # No deeper than the stock descender (the page rows keep their margins).
    deep = max(ink_rows(line_rows(f[ord(c)], lh, de))[1] for c in 'gpy' if ord(c) in f)
    for y in range(deep + 1, lh + 4):
        m[y] = [0] * len(m[y])
    # A blank first column: the stock glyphs have one (titles draw with spacing -1).
    if any(row[0] for row in m):
        m = [[0] + row for row in m]
        w += 1
    return [r[:w] for r in m], w, w + 1


def huf_lens():
    t = open(os.path.join(FW, 'src', 'stockfont_data.rs'), encoding='utf-8').read()
    cnt = [[int(v) for v in r.split(',') if v.strip()] for r in re.findall(r'\[([0-9, ]+)\],', t.split('pub static HUF_CNT')[1].split(' = [')[1].split('];')[0] + ',')]
    syms = [int(v, 16) for v in re.findall(r'0x([0-9A-F]{2})', t.split('pub static HUF_SYM')[1].split(' = [')[1].split('];')[0])]
    lens, i = [], 0
    for c in cnt:
        d = {}
        for l, n in enumerate(c):
            for _ in range(n):
                d[syms[i]] = l + 1
                i += 1
        lens.append(d)
    return lens


LENS = None


def emit_glyph(m, w, adv, lh, de, four):
    t, b = ink_rows(m)
    if b < t:
        t, b = 0, 0
    h = b - t + 1
    yoff = lh - de - h - t
    bits = []
    data = bytearray()
    if four:
        global LENS
        LENS = LENS or huf_lens()
        cost = lambda cx, s: LENS[cx].get(s, 99)
        rows = [m[y][:w] for y in range(t, b + 1)]
        toks = [tk for y, r in enumerate(rows) for tk in msf.huf_parse_row(r, rows[y - 1] if y else None, cost)]
        data, _ = msf.huf_encode(toks, [msf.huf_canon(c) for c in LENS])
    else:
        bits = [1 if m[y][x] else 0 for x in range(w) for y in range(t, b + 1)]
        bits += [0] * (-len(bits) % 8)
        for i in range(0, len(bits), 8):
            v = 0
            for k in bits[i:i + 8]:
                v = v << 1 | k
            data.append(v)
    return w, h, adv, yoff, bytes(data)


def font_use():
    use = {}
    p = os.path.join(FW, 'i18n', 'font-use.tsv')
    for l in open(p, encoding='utf-8'):
        l = l.rstrip('\n')
        if l and l[0] != '#':
            a, k = l.split('\t', 1)
            use.setdefault(k, set()).add(int(a, 16))
    return use


def build(lang, rows, check, rs_bytes):
    """rows: {English as in the sources: translation} -> ({English: encoded
    translation}, the glyph tables, stats)."""
    enc = lambda s: ''.join(LOOKALIKE.get(c, c) for c in s) if lang == 'ru' else s
    enc_rows = {k: enc(z) for k, z in rows.items()}
    freq = collections.Counter(c for z in enc_rows.values() for c in z if ord(c) >= 0x80 and c != 'Ω')
    code = {c: CTRL[i] for i, (c, _) in enumerate(sorted(freq.items(), key=lambda e: (-e[1], e[0]))[:len(CTRL)])}
    one = lambda s: ''.join(chr(code[c]) if c in code else c for c in enc(s))
    out_rows = {k: one(z) for k, z in rows.items()}
    if check:
        return out_rows, None, None
    img = stock_image()
    use = font_use()
    need = {a: set() for a in FONTS}
    for k, z in enc_rows.items():
        for a in use.get(re.sub(r'\\u\{([0-9a-fA-F]+)\}', lambda m: chr(int(m.group(1), 16)), k), {0x0806B13C}):
            if a in need:
                # Omega: the stock fonts' own (stockfont_w.rs).
                need[a] |= {c for c in z if ord(c) >= 0x80 and c != 'Ω'}
    s, total, sizes = '', 0, []
    extra = []
    for a in sorted(need):
        if not need[a]:
            continue
        src, nm = FONTS[a]
        f, lh, de, four = load(img, src)
        size = fit_size(f, lh, de, four) if lang == 'ru' else 0
        es, blob = [], bytearray()
        for ch in sorted(need[a], key=lambda c: code.get(c, ord(c))):
            if ch in COMPOSE:
                m, w, adv = compose(ch, f, lh, de)
            else:
                assert lang == 'ru', ch
                m, w, adv = render(ch, size, f, lh, de, four, 0)
            gw, gh, gadv, yoff, data = emit_glyph(m, w, adv, lh, de, four)
            es.append('E(%d,%d,%d,%d,%d,%d,0,%d)' % (code.get(ch, ord(ch)), len(blob), gw, gh, gadv, yoff, gh))
            blob += data
        total += len(blob) + 10 * len(es)
        sizes.append('%s %d glyphs %d B%s' % (nm, len(es), len(blob), ' (%d px)' % size if size else ''))
        s += '#[link_section = ".rodata.fonts.LANG_%s_E"]\nstatic X_%s_E: [Entry; %d] = [%s];\n' % (nm, nm, len(es), ', '.join(es))
        s += '#[link_section = ".rodata.fonts.LANG_%s_B"]\nstatic X_%s_B: [u8; %d] = *b"\\\n%s\n";\n' % (nm, nm, len(blob), rs_bytes(bytes(blob)))
        extra.append('(0x%08X, FontData { line_h: %d, descent: %d, fmt: %s, entries: &X_%s_E, blob: &X_%s_B })' % (a, lh, de, 'HUF' if four else 'BIT1', nm, nm))
    s += 'pub const N: usize = 0;\npub const SIZE: u8 = 0;\npub const GB: usize = 1;\npub static G: [u8; 0] = [];\n'
    s += 'pub static EXTRA: [(u32, FontData); %d] = [%s];\n' % (len(extra), ', '.join(extra))
    ntxt = sum(len(z.encode()) for z in out_rows.values())
    return out_rows, s, 'strings %d, text %d B, glyphs %d B: %s' % (len(out_rows), ntxt, total, '; '.join(sizes))
