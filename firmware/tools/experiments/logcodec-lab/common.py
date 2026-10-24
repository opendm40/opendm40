"""Shared pieces of the log-codec lab (docs/history/log-format-standards.md).

* load(csv, dlg)   rows of a reference CSV (+ the chunk cuts of its DLG file)
* dlg_bits()       bit-exact cost model of the DLG v1 coder (src/dlog.rs)
* RC               LZMA-style binary range coder (encoder and decoder)
"""
import os
import struct
import sys
from decimal import Decimal

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..'))
import dlog  # noqa: E402  (tools/dlog.py: from_scale, row_txt, Ad)

FLAGBIT = {'OL': 1, 'REL': 2, 'HOLD': 4, 'HI': 8, 'LO': 16, 'GAP': 32}
EMPTY, NEGZ = 'E', 'Z'


def parse_val(s):
    """CSV value text -> EMPTY / NEGZ / (neg, m, e) as src/dlog.rs dec6()."""
    if s == '':
        return EMPTY
    if s == '-0':
        return NEGZ
    d = Decimal(s)
    if d == 0:
        return (False, 0, 0)
    neg = d < 0
    d = abs(d)
    e = d.adjusted()
    m = int(d.scaleb(5 - e))
    assert 100000 <= m <= 999999, s
    return (neg, m, e)


def load(csv_path, dlg_path=None):
    """(header text, unit, rows [(t_ms, val, flags6)], chunk sizes)."""
    raw = open(csv_path, 'rb').read().decode('ascii')
    lines = raw.split('\r\n')
    i = 0
    while not lines[i].startswith('time_s,'):
        i += 1
    hdr = '\r\n'.join(lines[:i + 1]) + '\r\n'
    rows = []
    unit = None
    for ln in lines[i + 1:]:
        if not ln:
            continue
        ts, v, u, fl = ln.split(',')
        unit = u
        a, b = ts.split('.')
        f = 0
        for tok in fl.split():
            f |= FLAGBIT[tok]
        rows.append((int(a) * 1000 + int(b), parse_val(v), f))
    cuts = None
    if dlg_path:
        d = open(dlg_path, 'rb').read()
        hl = struct.unpack_from('<H', d, 6)[0]
        off = 24 + hl
        cuts = []
        while off < len(d):
            pl = struct.unpack_from('<H', d, off + 2)[0]
            nr = struct.unpack_from('<H', d, off + 24)[0]
            cuts.append(nr)
            off += 28 + pl
    return hdr, unit, rows, cuts


def at_scale(m, e, neg, k):
    if m == 0:
        return 0
    s = e - 5 - k
    if s == 0:
        n = m
    elif s == 1:
        n = m * 10
    else:
        return None
    return -n if neg else n


def csv_of(hdr, unit, rows):
    """Reference CSV text of decoded rows (tools/dlog.py formatter)."""
    out = [hdr]
    for t, v, f in rows:
        if v == EMPTY or f & 1:
            vs = ''
        elif v == NEGZ:
            vs = '-0'
        else:
            neg, m, e = v
            vs = dlog.from_scale((-m if neg else m), e - 5) if m else '0'
        out.append(dlog.row_txt(t, vs, unit, f))
    return ''.join(out)


def chunks(rows, cuts):
    i = 0
    for n in cuts:
        yield rows[i:i + n]
        i += n


def zz(x):
    return (x << 1) if x >= 0 else ((-x << 1) - 1)


def rice_len(x, k):
    q = x >> k
    return 24 + 32 if q >= 24 else q + 1 + k


# ----------------------------------------------------------- DLG v1 model

def dlg_bits(ch):
    """Payload bits of one DLG v1 chunk (mirror of src/dlog.rs Enc::row)."""
    at, av = dlog.Ad(4), dlog.Ad(16)
    bits = 0
    t_prev, dt_prev = ch[0][0], 0
    flags = ch[0][2]
    has_k, k, pv = False, 0, 0
    for t, v, fl in ch:
        dt = t - t_prev
        x = zz(dt - dt_prev)
        bits += rice_len(x, at.k())
        at.upd(x)
        t_prev, dt_prev = t, dt
        while True:
            if flags & 1:
                if fl == flags:
                    bits += 1
                    break
                bits += 7
                flags = fl
                continue
            kv = av.k()
            if fl != flags:
                bits += rice_len(0, kv) + 8
                flags = fl
                continue
            if v in (EMPTY, NEGZ):
                bits += rice_len(0, kv) + 2
            else:
                neg, m, e = v
                n = at_scale(m, e, neg, k) if has_k else None
                if n is not None:
                    u = zz(n - pv) + 1
                    bits += rice_len(u, kv)
                    av.upd(u)
                    pv = n
                else:
                    k = 0 if m == 0 else e - 5
                    pv = at_scale(m, e, neg, k)
                    has_k = True
                    bits += rice_len(0, kv) + 2 + 8 + 25
            break
    return bits


# ----------------------------------------------------------- range coder

PB, SH = 12, 4          # probability bits, adaptation shift
TOP = 1 << 24
HALF = 1 << (PB - 1)


class RC:
    """Binary range coder (LZMA rc, 12-bit probabilities). One object codes
    or decodes; `bit(p, i, b)` codes b (encoder) or returns the bit."""

    def __init__(self, data=None, shift=SH):
        self.dec = data is not None
        self.sh = shift
        self.range = 0xFFFFFFFF
        if self.dec:
            self.d, self.pos, self.code = data, 0, 0
            for _ in range(4):
                self.code = (self.code << 8) | self._byte()
        else:
            self.low, self.cache, self.csize, self.out = 0, 0, 0, bytearray()
            self.first = True

    def _byte(self):
        b = self.d[self.pos] if self.pos < len(self.d) else 0
        self.pos += 1
        return b

    def _shift_low(self):
        if self.low < 0xFF000000 or self.low > 0xFFFFFFFF:
            carry = self.low >> 32
            if not self.first:
                self.out.append((self.cache + carry) & 0xFF)
            self.first = False
            for _ in range(self.csize):
                self.out.append((0xFF + carry) & 0xFF)
            self.csize = 0
            self.cache = (self.low >> 24) & 0xFF
        else:
            self.csize += 1
        self.low = (self.low & 0x00FFFFFF) << 8

    acc = None   # optional {category: ideal bits}; self.cat = current

    def _upd(self, p, i, b):
        """Probability update. shift > 0: fixed rate (LZMA). shift < 0:
        counter-based rate, 2 at the first use up to -shift (the use count
        lives in p[i + len(p) // 2])."""
        sh = self.sh
        if sh < 0:
            j = i + len(p) // 2
            c = p[j]
            sh = min(-sh, 1 + (c + 1).bit_length())
            if c < 255:
                p[j] = c + 1
        pr = p[i]
        if b:
            p[i] = pr - (pr >> sh)
        else:
            p[i] = pr + (((1 << PB) - pr) >> sh)
        # keep it inside (0, 1)
        if p[i] < 31:
            p[i] = 31
        elif p[i] > (1 << PB) - 31:
            p[i] = (1 << PB) - 31

    def bit(self, p, i, b=None):
        pr = p[i]
        if self.acc is not None and not self.dec:
            import math
            q = pr / (1 << PB)
            self.acc[self.cat] = self.acc.get(self.cat, 0) - math.log2(q if not b else 1 - q)
        bound = (self.range >> PB) * pr
        if self.dec:
            b = int(self.code >= bound)
            if b:
                self.code -= bound
                self.range -= bound
            else:
                self.range = bound
            self._upd(p, i, b)
            while self.range < TOP:
                self.range = (self.range << 8) & 0xFFFFFFFF
                self.code = ((self.code << 8) | self._byte()) & 0xFFFFFFFF
            return b
        if b:
            self.low += bound
            self.range -= bound
        else:
            self.range = bound
        self._upd(p, i, b)
        while self.range < TOP:
            self.range = (self.range << 8) & 0xFFFFFFFF
            self._shift_low()
        return b

    def raw(self, nbits, v=None):
        """Equiprobable bits (MSB first)."""
        r = 0
        if self.acc is not None and not self.dec:
            self.acc[self.cat] = self.acc.get(self.cat, 0) + nbits
        for j in range(nbits - 1, -1, -1):
            self.range >>= 1
            if self.dec:
                b = 0
                if self.code >= self.range:
                    self.code -= self.range
                    b = 1
                r = (r << 1) | b
                while self.range < TOP:
                    self.range = (self.range << 8) & 0xFFFFFFFF
                    self.code = ((self.code << 8) | self._byte()) & 0xFFFFFFFF
            else:
                if (v >> j) & 1:
                    self.low += self.range
                while self.range < TOP:
                    self.range = (self.range << 8) & 0xFFFFFFFF
                    self._shift_low()
        return r if self.dec else v

    def finish(self):
        """Flush: enough bytes that the decoder (zero-padding past the end)
        sees a code inside the final interval."""
        # Choose a value in [low, low + range) with as many trailing zero
        # bytes as possible, then emit up to its last non-zero byte.
        for nb in range(1, 5):
            mask = (1 << (32 - 8 * nb)) - 1
            v = (self.low + mask) & ~mask
            if v + 0 < self.low + self.range and v >= self.low:
                self.low = v
                break
        for _ in range(5):
            self._shift_low()
        while self.out and self.out[-1] == 0:
            self.out.pop()
        return bytes(self.out)
