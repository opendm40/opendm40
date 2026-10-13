#!/usr/bin/env python3
"""Pixel diff of two 480x320 screen images (ours vs a stock reference).

    /usr/bin/python3 -I diff.py ours.png ref.bmp [-o diff.png] [--regions regions.json]
                                [--tol N] [--dx DX --dy DY] [--align] [--json out.json]

Inputs: PNG / any PIL format, or BMP straight from the device:
  * the stock V2.6 screenshot (s%03d.bmp): 480x320, BI_BITFIELDS RGB565,
    307266 bytes (spiflash_v2_6.h), parsed here without PIL (PIL rejects some
    565 masks);
  * the 2020 vendor disk images S000..S003.BMP: 4 bpp RLE4 (PIL handles them);
  * 24-bit BMP, PNG, PPM (our host renders are 2x: scaled back to 480x320).
Comparison is done in RGB565 space: every pixel is quantised to 565 first,
then two pixels are "different" when any 565 channel differs by more than
--tol (default 0; use 1..2 against real captures, the ILI9488 read-back path
rounds). Output: diff.png (left: ours, middle: ref, right: differing pixels in
red on a dimmed ref), the overall score and, with --regions, a per-region
score. regions.json: {"name": [x, y, w, h], ...} (regions.json next to this
script = the stock scope layout, shared with the renderer).
--align tries --dx/--dy shifts in -4..4 and reports the best (for captures of
a layout that differs by a constant offset, e.g. the 2020 images); --dx/--dy
apply a fixed shift of the reference.
Exit status 0; the score is printed as "diff: P% (N px)" plus one line per
region. --json writes the numbers.
"""
import json
import os
import struct
import sys

from PIL import Image

W, H = 480, 320


def load_bmp565(path):
    d = open(path, 'rb').read()
    if d[:2] != b'BM':
        return None
    off = struct.unpack_from('<I', d, 10)[0]
    hsz, w, h, planes, bpp, comp = struct.unpack_from('<IiiHHI', d, 14)
    if bpp != 16:
        return None
    im = Image.new('RGB', (abs(w), abs(h)))
    px = im.load()
    stride = (abs(w) * 2 + 3) & ~3
    masks = struct.unpack_from('<III', d, 14 + 40) if comp == 3 and hsz >= 40 else (0xF800, 0x07E0, 0x001F)
    rm, gm, bm = masks

    def ch(v, m):
        if m == 0:
            return 0
        s = (m & -m).bit_length() - 1
        bits = bin(m >> s).count('1')
        return ((v & m) >> s) * 255 // ((1 << bits) - 1)

    for y in range(abs(h)):
        row = off + (abs(h) - 1 - y if h > 0 else y) * stride
        for x in range(abs(w)):
            v = struct.unpack_from('<H', d, row + 2 * x)[0]
            px[x, y] = (ch(v, rm), ch(v, gm), ch(v, bm))
    return im


def load(path):
    im = load_bmp565(path) if path.lower().endswith('.bmp') else None
    if im is None:
        im = Image.open(path).convert('RGB')
    if im.size != (W, H):
        if im.size[0] % W == 0 and im.size[1] % H == 0 and im.size[0] // W == im.size[1] // H:
            k = im.size[0] // W
            im = im.resize((W, H), Image.NEAREST) if k == 1 else im.reduce(k) if False else im.resize((W, H), Image.BOX)
        else:
            im = im.resize((W, H), Image.NEAREST)
    return im


def to565(im):
    """List of (r5, g6, b5) per pixel."""
    out = []
    for r, g, b in im.getdata():
        out.append((r >> 3, g >> 2, b >> 3))
    return out


def shifted(p, dx, dy):
    """Reference shifted by (dx, dy): pixel (x, y) of the result = ref(x - dx, y - dy)."""
    if dx == 0 and dy == 0:
        return p
    out = [None] * (W * H)
    for y in range(H):
        sy = y - dy
        for x in range(W):
            sx = x - dx
            out[y * W + x] = p[sy * W + sx] if 0 <= sx < W and 0 <= sy < H else None
    return out


def compare(a, b, tol):
    bad = [False] * (W * H)
    n = 0
    for i in range(W * H):
        q = b[i]
        if q is None:
            continue
        p = a[i]
        if abs(p[0] - q[0]) > tol or abs(p[1] - q[1]) > tol or abs(p[2] - q[2]) > tol:
            bad[i] = True
            n += 1
    return bad, n


def region_score(bad, r):
    x, y, w, h = r
    x0, y0 = max(0, x), max(0, y)
    x1, y1 = min(W, x + w), min(H, y + h)
    tot = max(1, (x1 - x0) * (y1 - y0))
    n = sum(1 for yy in range(y0, y1) for xx in range(x0, x1) if bad[yy * W + xx])
    return n, tot


def main():
    args = sys.argv[1:]
    out = 'diff.png'
    regions = None
    tol = 0
    dx = dy = 0
    align = False
    jpath = None
    pos = []
    i = 0
    while i < len(args):
        a = args[i]
        if a == '-o':
            out = args[i + 1]; i += 2
        elif a == '--regions':
            regions = json.load(open(args[i + 1])); i += 2
        elif a == '--tol':
            tol = int(args[i + 1]); i += 2
        elif a == '--dx':
            dx = int(args[i + 1]); i += 2
        elif a == '--dy':
            dy = int(args[i + 1]); i += 2
        elif a == '--align':
            align = True; i += 1
        elif a == '--json':
            jpath = args[i + 1]; i += 2
        else:
            pos.append(a); i += 1
    if len(pos) != 2:
        print(__doc__)
        sys.exit(2)
    ours, ref = load(pos[0]), load(pos[1])
    if regions is None:
        here = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'regions.json')
        if os.path.exists(here):
            regions = json.load(open(here))
    pa, pb = to565(ours), to565(ref)
    best = None
    cands = [(dx, dy)] if not align else [(x, y) for x in range(-4, 5) for y in range(-4, 5)]
    for cx, cy in cands:
        bad, n = compare(pa, shifted(pb, cx, cy), tol)
        if best is None or n < best[0]:
            best = (n, cx, cy, bad)
    n, dx, dy, bad = best
    pct = 100.0 * n / (W * H)
    print('diff: %.2f%% (%d px) tol %d shift (%d,%d)' % (pct, n, tol, dx, dy))
    res = {'percent': pct, 'pixels': n, 'tol': tol, 'dx': dx, 'dy': dy, 'regions': {}}
    if regions:
        for name, r in regions.items():
            rn, tot = region_score(bad, r)
            res['regions'][name] = {'percent': 100.0 * rn / tot, 'pixels': rn, 'total': tot}
            print('  %-24s %6.2f%% (%d/%d)' % (name, 100.0 * rn / tot, rn, tot))
    # diff image: ours | ref | diff marks
    sheet = Image.new('RGB', (3 * W + 8, H), (255, 255, 255))
    sheet.paste(ours, (0, 0))
    refs = Image.new('RGB', (W, H))
    rp = refs.load()
    sb = shifted(pb, dx, dy)
    for y in range(H):
        for x in range(W):
            q = sb[y * W + x]
            rp[x, y] = (q[0] << 3, q[1] << 2, q[2] << 3) if q else (255, 0, 255)
    sheet.paste(refs, (W + 4, 0))
    dim = refs.point(lambda v: v // 3)
    dp = dim.load()
    for y in range(H):
        for x in range(W):
            if bad[y * W + x]:
                dp[x, y] = (255, 0, 0)
    sheet.paste(dim, (2 * W + 8, 0))
    sheet.save(out)
    if jpath:
        json.dump(res, open(jpath, 'w'), indent=1)


if __name__ == '__main__':
    main()
