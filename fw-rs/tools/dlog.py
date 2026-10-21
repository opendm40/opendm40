#!/usr/bin/env python3
"""Decode a DM40 fw-rs binary log (LOGnnn.DLG / SCOPnnn.DLG) to CSV.

Standalone (stdlib only). The output is byte-identical to the CSV the device
serves for the same file (virtual LOGnnn.CSV on the "DM40 info" USB volume,
SCPI MMEM:DATA? "LOGnnn.CSV"), i.e. to what the pre-DLG firmware wrote.
Format: fw-rs/docs/compact-log-format.md.

    dlog.py LOG000.DLG                 CSV to stdout
    dlog.py LOG000.DLG -o LOG000.CSV
    dlog.py --check LOG000.DLG         chunks, rows, torn tail, ratio
A torn tail (power cut, disk full) is reported and skipped: every whole
chunk before it is decoded.
"""
import argparse
import struct
import sys
import zlib

FHDR, CHDR, QMAX = 24, 28, 24
FLAGS = [(1, 'OL'), (2, 'REL'), (4, 'HOLD'), (8, 'HI'), (16, 'LO'), (32, 'GAP')]


class Ad:
    def __init__(self, a):
        self.a, self.n = a, 1

    def k(self):
        k = 0
        while k < 30 and (self.n << k) < self.a:
            k += 1
        return k

    def upd(self, x):
        self.a = min(self.a + min(x, 1 << 24), 0xFFFFFFFF)
        self.n += 1
        if self.n >= 32:
            self.a = (self.a + 1) >> 1
            self.n >>= 1


class Bits:
    def __init__(self, d, pos, end):
        self.d, self.pos, self.end = d, pos * 8, end * 8

    def get(self, n):
        if self.pos + n > self.end:
            raise EOFError
        v = 0
        for _ in range(n):
            v = (v << 1) | ((self.d[self.pos >> 3] >> (7 - (self.pos & 7))) & 1)
            self.pos += 1
        return v

    def rice(self, k):
        q = 0
        while q < QMAX and self.get(1):
            q += 1
        if q == QMAX:
            return self.get(32)
        return (q << k) | self.get(k)


def unzz(u):
    return (u >> 1) ^ -(u & 1)


def fmt_val(neg, m, e):
    s = '-' if neg else ''
    if m == 0:
        return s + '0'
    dg = '%06d' % m
    if -3 <= e < 6:
        if e < 0:
            return s + '0.' + '0' * (-e - 1) + dg
        ip = e + 1
        return s + dg[:ip] + ('.' + dg[ip:] if ip < 6 else '')
    return s + dg[0] + '.' + dg[1:] + 'E' + ('-' if e < 0 else '+') + '%02d' % abs(e)


def from_scale(n, k):
    if n == 0:
        return fmt_val(False, 0, 0)
    a = abs(n)
    d = len(str(a))
    e = k + d - 1
    a = a // 10 ** (d - 6) if d > 6 else a * 10 ** (6 - d)
    return fmt_val(n < 0, a, e)


def row_txt(t, val, unit, flags):
    s = '%d.%03d,' % (t // 1000, t % 1000)
    if not flags & 1:
        s += val
    return s + ',' + unit + ',' + ' '.join(n for b, n in FLAGS if flags & b) + '\r\n'


def chunk_rows(d, off, pl, t0, nrows, flags, unit):
    b = Bits(d, off + CHDR, off + CHDR + pl)
    at, av = Ad(4), Ad(16)
    tp, dtp, k, pv = t0, 0, 0, 0
    out = []
    for _ in range(nrows):
        x = b.rice(at.k())
        at.upd(x)
        dt = (dtp + unzz(x)) & 0xFFFFFFFF
        t = (tp + dt) & 0xFFFFFFFF
        tp, dtp = t, dt
        while True:
            if flags & 1:
                if b.get(1):
                    flags = b.get(6)
                    continue
                val = ''
                break
            u = b.rice(av.k())
            if u:
                av.upd(u)
                pv = pv + unzz(u - 1)
                val = from_scale(pv, k)
                break
            esc = b.get(2)
            if esc == 0:
                val = ''
                break
            if esc == 1:
                flags = b.get(6)
                continue
            if esc == 2:
                k = struct.unpack('b', bytes([b.get(8)]))[0]
                neg = b.get(1)
                pv = -b.get(24) if neg else b.get(24)
                val = from_scale(pv, k)
                break
            val = '-0'
            break
        out.append(row_txt(t, val, unit, flags))
    return out


def sci(v, dig):
    """scope_csv.rs sci(): '+d.dd..de+xx' from an f32 computation."""
    f = lambda x: struct.unpack('<f', struct.pack('<f', x))[0]
    v = f(v) if v == v and abs(v) != float('inf') else 0.0
    s = '-' if v < 0 else '+'
    v = abs(v)
    e = 0
    if v != 0:
        while v >= 10:
            v = f(v * f(0.1))
            e += 1
        while v < 1:
            v = f(v * 10)
            e -= 1
    scale = 10 ** dig
    m = int(f(f(v * scale) + 0.5))
    if m >= 10 * scale:
        m //= 10
        e += 1
    ip, fp = divmod(m, scale)
    return '%s%d.%0*de%s%02d' % (s, ip, dig, fp, '-' if e < 0 else '+', min(abs(e), 99))


def decode(d, errors):
    """CSV text of DLG bytes `d`; torn / bad chunks reported in `errors`."""
    if len(d) < FHDR or d[:4] != b'DLG1':
        raise ValueError('not a DLG file')
    kind, ul, hl = d[4], d[5], struct.unpack_from('<H', d, 6)[0]
    if len(d) < FHDR + hl or (zlib.crc32(d[12:FHDR + hl]) != struct.unpack_from('<I', d, 8)[0]):
        raise ValueError('bad file header')
    unit = d[12:12 + ul].decode('ascii')
    out = [d[FHDR:FHDR + hl].decode('ascii')]
    off = FHDR + hl
    if kind == 2:
        a, b, dt = struct.unpack_from('<fff', d, off)
        t0, n, crc = struct.unpack_from('<III', d, off + 12)
        if zlib.crc32(d[off:off + 20]) != crc:
            raise ValueError('bad scope parameters')
        f = lambda x: struct.unpack('<f', struct.pack('<f', x))[0]
        p = off + 24
        for i in range(n):
            if p + 2 > len(d):
                errors.append('torn at sample %d' % i)
                break
            c = struct.unpack_from('<H', d, p)[0] & 0x3FF
            p += 2
            t = f(f(i - t0 if i >= t0 else -(t0 - i)) * dt)
            out.append(sci(t, 5) + ',' + sci(f(a + f(b * c)), 4) + '\r\n')
        return ''.join(out), {'rows': n, 'chunks': 0}
    rows = chunks = 0
    csv = hl
    while off < len(d):
        if off + CHDR > len(d) or d[off:off + 2] != b'DC':
            errors.append('torn / unknown bytes at %d (%d bytes)' % (off, len(d) - off))
            break
        pl, crc, row0, base, clen, t0, nr, fl0 = struct.unpack_from('<HIIIIIHB', d, off + 2)
        end = off + CHDR + pl
        if end > len(d) or zlib.crc32(d[off + 8:end]) != crc:
            errors.append('chunk at %d: %s' % (off, 'torn' if end > len(d) else 'CRC error'))
            break
        if base != csv or row0 != rows:
            errors.append('chunk at %d: out of sequence' % off)
            break
        r = chunk_rows(d, off, pl, t0, nr, fl0, unit)
        out += r
        csv += clen
        rows += nr
        chunks += 1
        off = end
    return ''.join(out), {'rows': rows, 'chunks': chunks}


def main():
    p = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    p.add_argument('file')
    p.add_argument('-o', '--out', help='CSV output file (default stdout)')
    p.add_argument('--check', action='store_true', help='report chunks / rows / errors only')
    a = p.parse_args()
    d = open(a.file, 'rb').read()
    errors = []
    txt, st = decode(d, errors)
    if a.check:
        print('%s: %d rows in %d chunks, %d bytes -> CSV %d bytes (%.1fx)' %
              (a.file, st['rows'], st['chunks'], len(d), len(txt), len(txt) / max(1, len(d))))
        for e in errors:
            print('  ' + e)
        return 0
    for e in errors:
        print('dlog.py: %s: %s' % (a.file, e), file=sys.stderr)
    if a.out:
        open(a.out, 'w', newline='').write(txt)
    else:
        sys.stdout.write(txt)
    return 0


if __name__ == '__main__':
    sys.exit(main())
