#!/usr/bin/env python3
"""Scope record (26624 10-bit ADC codes): bytes per sample per coder.

Synthetic records (seeded): DC + noise, sine, square with edges, mains with
harmonics, a burst. Coders: u16 (DLG kind 2 today), 10-bit packed,
FLAC-style fixed polynomial predictor (order 0..3 chosen per 4096-sample
block, forward: the record is in RAM) + adaptive Rice, the same predictor +
the DLG2 range coder (real encode, decoded back and compared), FLAC (libFLAC
via soundfile, 16-bit mono, compression level 8) and zlib with a 1 KiB window
as references.

    scope.py            (needs numpy; soundfile optional)
"""
import math
import os
import random
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import RC, rice_len, zz          # noqa: E402
import codec                                  # noqa: E402
import dlog                                   # noqa: E402

N = 26624
BLK = 4096


def records():
    r = random.Random(7)
    g = lambda s: r.gauss(0, s)
    cl = lambda x: max(0, min(1023, int(round(x))))
    out = {}
    out['DC + 1 LSB noise'] = [cl(612 + g(1.0)) for i in range(N)]
    out['sine 13 periods, 0.7 LSB noise'] = [cl(512 + 400 * math.sin(2 * math.pi * 13 * i / N) + g(0.7)) for i in range(N)]
    sq = []
    for i in range(N):
        ph = (i * 20 // N) % 2
        sq.append(cl((812 if ph else 212) + g(0.8)))
    out['square 10 periods'] = sq
    out['mains 50 Hz + harmonics, 2 LSB noise'] = [cl(512 + 300 * math.sin(2 * math.pi * 40 * i / N)
                                                       + 40 * math.sin(2 * math.pi * 120 * i / N + 1)
                                                       + 15 * math.sin(2 * math.pi * 200 * i / N) + g(2)) for i in range(N)]
    out['sine 400 periods (fast)'] = [cl(512 + 450 * math.sin(2 * math.pi * 400 * i / N) + g(0.7)) for i in range(N)]
    out['noise burst on DC'] = [cl(512 + (g(60) if 10000 <= i < 14000 else g(0.8))) for i in range(N)]
    return out


def fixed_res(x, order, i):
    if order == 0 or i < order:
        p = x[i - 1] if i else 512
        return x[i] - p if order else x[i] - 512
    if order == 1:
        return x[i] - x[i - 1]
    if order == 2:
        return x[i] - 2 * x[i - 1] + x[i - 2]
    return x[i] - 3 * x[i - 1] + 3 * x[i - 2] - x[i - 3]


def block_orders(x):
    """FLAC-like: per block the fixed order with the least sum |residual|."""
    orders = []
    for b0 in range(0, len(x), BLK):
        best = min(range(4), key=lambda o: sum(abs(fixed_res(x, o, i)) for i in range(b0, min(len(x), b0 + BLK))))
        orders.append(best)
    return orders


def rice_bytes(x, orders):
    bits = 0
    ad = dlog.Ad(16)
    for i in range(len(x)):
        u = zz(fixed_res(x, orders[i // BLK], i))
        bits += rice_len(u, ad.k())
        ad.upd(u)
    return bits / 8 + len(orders) / 4


def rc_bytes(x, orders):
    rc = RC(shift=-5)
    P = [2048] * (2 * codec.SLOTS)
    for i in range(len(x)):
        codec.sint(rc, P, 0, fixed_res(x, orders[i // BLK], i))
    data = rc.finish()
    # decode back
    rc = RC(data, shift=-5)
    P = [2048] * (2 * codec.SLOTS)
    y = []
    for i in range(len(x)):
        e = codec.sint(rc, P, 0)
        o = orders[i // BLK]
        y.append(0)
        # invert: x[i] = e + prediction from y
        y[i] = e + (x_pred(y, o, i))
    assert y == x, 'scope round trip failed'
    return len(data) + len(orders) / 4


def x_pred(x, order, i):
    return x[i] - fixed_res(x + [0] * 0, order, i) if False else _pred(x, order, i)


def _pred(x, order, i):
    if order == 0 or i < order:
        return (x[i - 1] if i else 512) if order else 512
    if order == 1:
        return x[i - 1]
    if order == 2:
        return 2 * x[i - 1] - x[i - 2]
    return 3 * x[i - 1] - 3 * x[i - 2] + x[i - 3]


def flac_bytes(x):
    try:
        import numpy as np
        import soundfile as sf
        import io
    except ImportError:
        return None
    b = io.BytesIO()
    sf.write(b, (np.array(x, dtype=np.int16) - 512) * 1, 48000, format='FLAC', subtype='PCM_16',
             compression_level=1.0)
    return len(b.getvalue())


def main():
    print('| record | u16 (DLG kind 2) | 10-bit packed | zlib 1 KiB window, u16 | fixed LPC + Rice | fixed LPC + DLG2 RC | FLAC -8 (file) |')
    print('|---|---|---|---|---|---|---|')
    for name, x in records().items():
        raw = b''.join(v.to_bytes(2, 'little') for v in x)
        orders = block_orders(x)
        zl = zlib.compressobj(6, zlib.DEFLATED, -10)
        z = len(zl.compress(raw) + zl.flush())
        fl = flac_bytes(x)
        print('| %s | 2.000 | 1.250 | %.3f | %.3f | %.3f | %s |' % (
            name, z / N, rice_bytes(x, orders) / N, rc_bytes(x, orders) / N,
            '%.3f' % (fl / N) if fl else '-'))


if __name__ == '__main__':
    main()
