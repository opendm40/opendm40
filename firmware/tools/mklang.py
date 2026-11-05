#!/usr/bin/env python3
"""Generate src/lang_<lang>.rs (the localized images, src/lang.rs) from
i18n/<lang>.tsv.

    /usr/bin/python3 -I tools/mklang.py <lang> [--check]

Chinese: FreeType's mono rendering of Noto Sans CJK SC Regular at 14 px, only
the characters used; each hanzi is U+0400 + its glyph index (two UTF-8 bytes).
Licence: i18n/OFL-NotoSansCJK.txt; the font file is not in the repo (Debian:
fonts-noto-cjk). --check compares the text tables with the tsv, no font needed.
"""
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
NOTO_CJK = '/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc'
ZH_SIZE = 14
BASE = 0x400

# Native name, "Language" in that language, the line height floor.
LANGS = {
    'zh': ('中文', '语言', '语言设置', 15),
    'ru': ('Русский', 'Язык', 'Язык', 12),
    'it': ('Italiano', 'Lingua', 'Lingua', 12),
    'fr': ('Français', 'Langue', 'Langue', 12),
}


def fnv(b):
    h = 0x811C9DC5
    for c in b:
        h = ((h ^ c) * 0x01000193) & 0xFFFFFFFF
    return h


def unesc(s):
    return re.sub(r'\\u\{([0-9a-fA-F]+)\}', lambda m: chr(int(m.group(1), 16)), s)


def read_tsv(lang):
    rows = {}
    for n, l in enumerate(open(os.path.join(FW, 'i18n', lang + '.tsv'), encoding='utf-8'), 1):
        l = l.rstrip('\n')
        if not l or l.startswith('#'):
            continue
        k, z, src = l.split('\t')
        assert (src == 'E') == (k == z) or src == 'M', 'line %d: E = the English on purpose' % n
        assert src in ('S', 'R', 'M', 'E'), 'line %d: source' % n
        k, z = unesc(k), unesc(z)
        assert k not in rows, 'line %d: %r twice' % (n, k)
        if k != z:  # the same in both: no table entry
            rows[k] = z
    return rows


def rs_bytes(b):
    return '\n'.join('    ' + ''.join('\\x%02x' % x for x in b[i:i + 32]) + '\\' for i in range(0, len(b), 32))


def rs_str(s):
    return ''.join(c if 32 <= ord(c) < 127 and c not in '"\\' else '\\u{%x}' % ord(c) for c in s)


def text_tables(ent):
    """ent: (hash, encoded translation) sorted; the H / O / T statics."""
    txt, off = b'', [0]
    for _, z in ent:
        txt += z.encode()
        off.append(len(txt))
    assert len(txt) < 65536 and len({h for h, _ in ent}) == len(ent)
    s = '#[link_section = ".rodata.fonts.LANG_H"]\npub static H: [u32; %d] = [\n' % len(ent)
    for i in range(0, len(ent), 8):
        s += '    ' + ', '.join('0x%08x' % e[0] for e in ent[i:i + 8]) + ',\n'
    s += '];\n#[link_section = ".rodata.fonts.LANG_O"]\npub static O: [u16; %d] = [\n' % len(off)
    for i in range(0, len(off), 16):
        s += '    ' + ', '.join(str(v) for v in off[i:i + 16]) + ',\n'
    s += '];\n#[link_section = ".rodata.fonts.LANG_T"]\npub static T: [u8; %d] = *b"\\\n%s\n";\n' % (len(txt), rs_bytes(txt))
    return s, len(txt)


def is_han(c):
    return ord(c) >= 0x2E80


def zh_bits(chars):
    from PIL import Image, ImageDraw, ImageFont
    font = ImageFont.truetype(NOTO_CJK, ZH_SIZE, index=2)
    assert 'SC' in font.getname()[0], font.getname()
    out = bytearray()
    for c in chars:
        im = Image.new('1', (ZH_SIZE, ZH_SIZE), 0)
        d = ImageDraw.Draw(im)
        d.fontmode = '1'
        d.text((ZH_SIZE // 2, ZH_SIZE // 2), c, font=font, fill=1, anchor='mm')
        bits = [1 if im.getpixel((x, y)) else 0 for x in range(ZH_SIZE) for y in range(ZH_SIZE)]
        bits += [0] * (-len(bits) % 8)
        assert any(bits), c
        for i in range(0, len(bits), 8):
            v = 0
            for b in bits[i:i + 8]:
                v = v << 1 | b
            out.append(v)
    return bytes(out)


def build_zh(rows, check):
    name, row, tr, minl = LANGS['zh']
    chars = sorted({c for z in rows.values() for c in z if is_han(c)} | {c for c in name + row + tr})
    idx = {c: i for i, c in enumerate(chars)}
    enc = lambda s: ''.join(chr(BASE + idx[c]) if is_han(c) else c for c in s)
    lang_row = 'Language ' + enc(row)
    ent = sorted([(fnv(k.encode()), enc(z)) for k, z in rows.items()] + [(fnv(lang_row.encode()), enc(tr))])
    tb, ntxt = text_tables(ent)
    head = 'pub const NAME: &str = "%s";\npub const LANG_ROW: &str = "%s";\npub const MIN_LINE: u16 = %d;\n' % (rs_str(enc(name)), rs_str(lang_row), minl)
    if check:
        return head + tb, None
    bits = zh_bits(chars)
    gb = len(bits) // len(chars)
    s = '// Characters: %s\n' % ''.join(chars)
    s += 'pub const N: usize = %d;\npub const SIZE: u8 = %d;\npub const GB: usize = %d;\n' % (len(chars), ZH_SIZE, gb)
    s += '#[link_section = ".rodata.fonts.LANG_G"]\npub static G: [u8; %d] = *b"\\\n%s\n";\n' % (len(bits), rs_bytes(bits))
    s += 'pub static EXTRA: [(u32, FontData); 0] = [];\n'
    return head + tb, s, 'strings %d, glyphs %d (%d B), text %d B, tables %d B' % (len(ent), len(chars), len(bits), ntxt, 6 * len(ent) + 2)


def main():
    lang = sys.argv[1]
    check = '--check' in sys.argv
    rows = read_tsv(lang)
    out = os.path.join(FW, 'src', 'lang_%s.rs' % lang)
    if lang == 'zh':
        r = build_zh(rows, check)
    else:
        sys.path.insert(0, HERE)
        import mklang_x
        r = mklang_x.build(lang, rows, check, LANGS[lang], text_tables, fnv, rs_str, rs_bytes)
    if check:
        if r[0] not in open(out, encoding='utf-8').read():
            sys.exit('%s is out of date: run tools/mklang.py %s' % (out, lang))
        print('%s matches its tsv' % os.path.basename(out))
        return
    head, gl, stats = r
    notice = '// Generated by tools/mklang.py from i18n/%s.tsv. Do not edit.\n' % lang
    open(out, 'w', encoding='utf-8').write(notice + gl + head)
    print(lang, stats)


main()
