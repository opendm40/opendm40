#!/usr/bin/env python3
"""Log-codec lab: how far below DLG v1 can a DMM log go on the MCU?

Input: the reference CSV + DLG files of tools/dlog-test (t0..t8 and the
10 h run), written by a copy of tools/dlog-test/main.rs that also dumps the
long run (see fw-rs/docs/log-format-standards.md, "Reproduce").

    lab.py DATA_DIR            tables for the doc (prints markdown)

Every DLG2 (range coder) number is a real encode, decoded back and compared
with the reference CSV byte for byte, chunk by chunk (chunk independence:
each chunk gets only its t0 / row count / flags / dt hint from its header).
"""
import lzma
import math
import struct
import sys
import os
import sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from common import EMPTY, NEGZ, load, chunks, csv_of, dlg_bits, zz, rice_len
import codec

NAMES = ['noisy DC V 10/s', 'steady ohm 2.5/s', 'ramp 2.5/s', 'AC V 230 10/s',
         'Hz 3/s', 'ohm + OL bursts', 'MEDIUM + autorange + flags',
         'decade sweep (stress)', 'special values (stress)']
HDR1, HDR2 = 28, 30     # chunk header bytes: DLG v1, DLG2 (+ u16 dt hint)


# ------------------------------------------------------------ floors

def hq(sig):
    """Entropy (bits) of a Gaussian of sigma `sig` counts, rounded to an
    integer (averaged over the sub-count offset of the mean)."""
    if sig > 8:
        return 0.5 * math.log2(2 * math.pi * math.e * sig * sig)
    tot = 0.0
    offs = [o / 16 for o in range(16)]
    for o in offs:
        h = 0.0
        for k in range(-int(10 * sig) - 2, int(10 * sig) + 3):
            p = 0.5 * (math.erf((k + 0.5 - o) / (sig * math.sqrt(2))) -
                       math.erf((k - 0.5 - o) / (sig * math.sqrt(2))))
            if p > 0:
                h -= p * math.log2(p)
        tot += h
    return tot / len(offs)


def h(*ps):
    return -sum(p * math.log2(p) for p in ps if p > 0)


def res(v):
    """Resolution (one count) of a printed value."""
    neg, m, e = v
    return 10.0 ** (e - 5)


def floor_bits(si, rows):
    """(time, value) bits/row an encoder that knows the generator needs.
    None where the set has no simple model (stress sets)."""
    n = len(rows)
    tj = {4: 11}.get(si, 7)
    t_bits = math.log2(tj) * (n - 1) / n
    if si == 0:
        v = hq(30e-6 / 1e-5)
    elif si == 1:
        v = h(0.9, 0.05, 0.05)
    elif si == 2:
        v = hq(50e-6 / 1e-5)
    elif si == 3:
        q, r = 20.0 ** 2, 10.0 ** 2          # counts of 1 mV: walk, noise
        P = (q + math.sqrt(q * q + 4 * q * r)) / 2
        v = hq(math.sqrt(P + r))
    elif si == 4:
        v = hq(0.0005 / 1e-4)
    elif si == 5:
        ol = sum(1 for r in rows if r[2] & 1) / n
        v = (1 - ol) * hq(0.02 / 1e-4) + h(0.01, 0.99)
    elif si == 6:
        lv = [0.5, 0.0123, 12.5, 1.0, 0.000999, 3.3e-7, 250000.0]
        acc = 0.0
        for k, (t, val, f) in enumerate(rows):
            if isinstance(val, tuple) and val[1]:
                sig = lv[(k // 300) % 7] * 2e-5 / res(val)
                acc += hq(sig)
        # flags: REL/HOLD/HI/LO/GAP change ~ every 100 rows: ~12 bits each
        nch = sum(1 for a, b in zip(rows, rows[1:]) if a[2] != b[2])
        v = (acc + nch * 12) / n
    else:
        return None
    return t_bits, v


# ------------------------------------------------------------ coders

VARIANTS = [
    ('Rice, new predictors', 'rice', dict(codec.DEF)),
    ('RC, DLG predictors', 'rc', dict(codec.DEF, tpred=(0,), vpred=(0,))),
    ('RC, new predictors (DLG2)', 'rc', dict(codec.DEF)),
    ('DLG2 + 4 activity classes', 'rc', dict(codec.DEF, ncls=4)),
    ('DLG2, fixed-rate LZMA probabilities', 'rc', dict(codec.DEF, shift=4)),
]


def hints(rows, cuts):
    """dt hint per chunk: the last interval of the previous chunk."""
    out, i, last = [], 0, 0
    for nn in cuts:
        out.append(last)
        if nn > 1:
            last = rows[i + nn - 1][0] - rows[i + nn - 2][0]
        elif i:
            last = rows[i][0] - rows[i - 1][0]
        i += nn
    return out


def run_rc(hdr, unit, rows, cuts, o, csv, verify=True):
    acc = {}
    tot, out = 0, []
    st = {'pq': (4, 4)}
    for c, hn in zip(chunks(rows, cuts), hints(rows, cuts)):
        pq = st['pq']
        b = codec.encode_chunk(c, hn, o, acc, pq, st)
        tot += len(b) + HDR2 + (2 if o.get('prior') else 0)
        if verify:
            out += codec.decode_chunk(b, c[0][0], len(c), hn, c[0][2], o, pq)
    if verify and csv_of(hdr, unit, out) != csv:
        raise SystemExit('round trip FAILED')
    return tot, acc


def run_rice(rows, cuts, o):
    return sum((codec.rice_bits(c, hn, o) + 7) // 8 + HDR2
               for c, hn in zip(chunks(rows, cuts), hints(rows, cuts)))


def gorilla_bits(rows):
    """Gorilla (Facebook TSDB) float XOR coding of the values as f32 and
    its delta-of-delta timestamps (bits; reference only: an f32 is not the
    printed 6-digit decimal, so this does not reproduce the CSV)."""
    bits = 0
    pv, lead, trail = None, 99, 0
    tp, dp = None, 0
    for t, v, f in rows:
        if tp is None:
            bits += 64
        else:
            d = t - tp
            dd = d - dp
            dp = d
            bits += 1 if dd == 0 else 9 if -63 <= dd <= 64 else 12 if -255 <= dd <= 256 else 16 if -2047 <= dd <= 2048 else 36
        tp = t
        if isinstance(v, tuple):
            neg, m, e = v
            x = struct.unpack('<I', struct.pack('<f', (-m if neg else m) * 10.0 ** (e - 5)))[0]
        else:
            x = 0x7F800000
        if pv is None:
            bits += 32
        else:
            xr = x ^ pv
            if xr == 0:
                bits += 1
            else:
                ld = 32 - xr.bit_length()
                tr = (xr & -xr).bit_length() - 1
                if ld >= lead and tr >= trail:
                    bits += 2 + (32 - lead - trail)
                else:
                    lead, trail = ld, tr
                    bits += 2 + 5 + 6 + (32 - ld - tr)
        pv = x
    return bits


def main():
    D = sys.argv[1]
    print('## per set (bytes per row incl. chunk headers; chunks = the dlog-test cuts)\n')
    cols = ['CSV', 'DLG v1', 'floor (t+v)'] + [v[0] for v in VARIANTS] + ['xz -9 CSV', 'Gorilla f32']
    print('| set | ' + ' | '.join(cols) + ' |')
    print('|---' * (len(cols) + 1) + '|')
    brk = []
    for si in range(9):
        hdr, unit, rows, cuts = load('%s/t%d.csv' % (D, si), '%s/t%d.dlg' % (D, si))
        csv = open('%s/t%d.csv' % (D, si), 'rb').read().decode()
        dl = open('%s/t%d.dlg' % (D, si), 'rb').read()
        n = len(rows)
        body = len(csv) - len(hdr)
        cells = ['%.2f' % (body / n), '%.3f' % ((len(dl) - 24 - len(hdr)) / n)]
        fl = floor_bits(si, rows)
        cells.append('%.3f (%.1f+%.1f b)' % ((fl[0] + fl[1]) / 8, fl[0], fl[1]) if fl else '-')
        for name, kind, o in VARIANTS:
            if kind == 'rice':
                b = run_rice(rows, cuts, o)
            else:
                b, acc = run_rc(hdr, unit, rows, cuts, o, csv)
                if name == 'RC, new predictors (DLG2)':
                    brk.append((NAMES[si], acc.get('time', 0) / n, acc.get('value', 0) / n, fl))
            cells.append('%.3f' % (b / n))
        cells.append('%.3f' % (len(lzma.compress(csv[len(hdr):].encode(), preset=9 | lzma.PRESET_EXTREME)) / n))
        cells.append('%.3f' % (gorilla_bits(rows) / 8 / n))
        print('| %s | %s |' % (NAMES[si], ' | '.join(cells)))
    print('\n## where the bits go (RC, new predictors; ideal bits/row; floor)\n')
    print('| set | time | time floor | value+flags | value floor |')
    print('|---|---|---|---|---|')
    for nm, tb, vb, fl in brk:
        print('| %s | %.2f | %s | %.2f | %s |' % (nm, tb, '%.2f' % fl[0] if fl else '-', vb, '%.2f' % fl[1] if fl else '-'))

    # 10 h run, chunk-size sweep
    hdr, unit, rows, _ = load('%s/long.csv' % D)
    csv = open('%s/long.csv' % D, 'rb').read().decode()
    n = len(rows)
    dl = open('%s/long.dlg' % D, 'rb').read()
    print('\n## 10 h at 10/s (360000 rows): chunk size\n')
    print('| rows/chunk (time) | DLG v1 B/row | DLG2 RC B/row | gain | lost on power cut |')
    print('|---|---|---|---|---|')
    o = dict(codec.DEF)
    for cn in (60, 600, 3000, 6000):
        cuts = [cn] * (n // cn) + ([n % cn] if n % cn else [])
        v1 = sum((dlg_bits(c) + 7) // 8 + HDR1 for c in chunks(rows, cuts))
        b, _ = run_rc(hdr, unit, rows, cuts, o, csv, verify=(cn == 600))
        print('| %d (%s) | %.3f | %.3f | %.2fx | %s |' % (
            cn, '%g s' % (cn / 10) if cn < 600 else '%g min' % (cn / 600), v1 / n, b / n, v1 / b,
            '%d s of readings' % (cn // 10)))
    print('\nDLG v1 file (600-row chunks): %.3f B/row' % ((len(dl) - 24 - len(hdr)) / n))

    # Timestamps from a fixed-rate converter stamped by a jittery loop
    print('\n## timestamp model: t_i = 100 i + loop latency U(0..6) ms (noisy DC values)\n')
    hdr, unit, rows, cuts = load('%s/t0.csv' % D, '%s/t0.dlg' % D)
    import random
    rnd = random.Random(1)
    rows2 = [(100 * i + rnd.randint(0, 6), v, f) for i, (t, v, f) in enumerate(rows)]
    csv2 = csv_of(hdr, unit, rows2)
    v1 = sum((dlg_bits(c) + 7) // 8 + HDR1 for c in chunks(rows2, cuts))
    print('| coder | B/row | time bits/row |')
    print('|---|---|---|')
    print('| DLG v1 (delta-of-delta) | %.3f | |' % (v1 / len(rows2)))
    for nm, tp in (('DLG2 without the grid predictor', (0, 3)), ('DLG2 (prev dt / slow EMA / grid)', (0, 2, 3))):
        b, acc = run_rc(hdr, unit, rows2, cuts, dict(codec.DEF, tpred=tp), csv2)
        print('| %s | %.3f | %.2f |' % (nm, b / len(rows2), acc['time'] / len(rows2)))
    print('| floor: log2(7) time + value | %.3f | 2.81 |' % ((math.log2(7) + hq(3)) / 8))
    # Timestamps on the converter's grid (no loop jitter): t_i = 100 i
    print('\n## timestamps on the conversion grid (t_i = 100 i ms, same values)\n')
    print('| set | DLG v1 B/row | DLG2 B/row | DLG2 value+flags bits/row | DLG2 time bits/row |')
    print('|---|---|---|---|---|')
    for si in (0, 1, 3, 5):
        hdr, unit, rows, cuts = load('%s/t%d.csv' % (D, si), '%s/t%d.dlg' % (D, si))
        per = 400 if si == 1 else 100
        rows3 = [(per * i, v, f) for i, (t, v, f) in enumerate(rows)]
        csv3 = csv_of(hdr, unit, rows3)
        v1 = sum((dlg_bits(c) + 7) // 8 + HDR1 for c in chunks(rows3, cuts))
        b, acc = run_rc(hdr, unit, rows3, cuts, dict(codec.DEF), csv3)
        n = len(rows3)
        print('| %s | %.3f | %.3f | %.2f | %.2f |' % (NAMES[si], v1 / n, b / n, acc['value'] / n, acc['time'] / n))


if __name__ == '__main__':
    main()
