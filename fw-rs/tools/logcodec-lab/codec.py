"""Candidate successor of the DLG v1 row coder ("DLG2" in the doc).

Same row structure as DLG v1 (time, then OL-bit / escape / value at the
chunk's decimal scale; src/dlog.rs), but
  * time: residual of dt against a backward-selected predictor
    (previous dt = DLG's delta-of-delta / EMA of dt / fixed-rate grid);
  * value: residual against a backward-selected predictor (previous value =
    DLG / two EMA levels / Holt level+trend), selection by the smaller
    running mean |residual| (the decoder runs the same predictors);
  * entropy coding: adaptive binary range coder (LZMA-style) with an
    Exp-Golomb binarization whose bits have adaptive contexts, optionally
    split by an activity class; or adaptive Rice (bit count only).

opts: tpred  set of time predictors  (0 prev dt, 1 EMA, 2 grid)
      vpred  set of value predictors (0 prev, 1 EMA 1/4, 2 EMA 1/16, 3 Holt)
      ncls   activity classes per stream (1 = one context set)
      shift  probability adaptation shift (4 = fast, 5 = LZMA default)
"""
from common import EMPTY, NEGZ, RC, at_scale, zz, rice_len
import dlog

NU = 26                       # Exp-Golomb buckets; the last one = raw 32 bits
MB = 4                        # buckets 1..MB: whole mantissa in a bit tree
TREE = [0, 0]
for _b in range(2, MB + 2):
    TREE.append(TREE[-1] + (1 << (_b - 1)) - 1)
SLOTS = 2 + 2 * NU + TREE[MB + 1]   # zero, sign, unary, top bit, trees
DEF = dict(tpred=(0, 2, 3), vpred=(0, 1, 2, 3), ncls=1, shift=-5, prior=False)


def act_q(a):
    """Chunk-header byte: the activity (Act.a = 128 x mean |residual|) in
    half-octaves."""
    import math
    return min(63, int(round(2 * math.log2(1 + a / 128))))


def prior_probs(q):
    """Initial P(bit = 0) (12-bit) of one context set for a two-sided
    geometric residual of mean |e| = 2^(q/2) - 1."""
    mu = max(2 ** (q / 2) - 1, 0.02)
    lo, hi = 1e-9, 1 - 1e-9          # theta: mean |e| = 2 th / (1 - th^2)
    for _ in range(60):
        th = (lo + hi) / 2
        if 2 * th / (1 - th * th) < mu:
            lo = th
        else:
            hi = th
    p0 = (1 - th) / (1 + th)
    # P(|e| = a + 1) for a >= 0, as a function: (1 - p0) * (1 - th) * th^a
    def pa(a):
        return (1 - th) * th ** a

    def pr(lo_, hi_):                 # P(lo_ <= a < hi_) given e != 0
        return th ** lo_ - th ** hi_ if hi_ is not None else th ** lo_
    P = [2048] * SLOTS
    clip = lambda x: max(31, min(4065, int(round(x * 4096))))
    P[0] = clip(p0)
    P[1] = 2048
    for j in range(NU):              # bucket b = j covers a + 1 in [2^j, 2^(j+1))
        ge = pr((1 << j) - 1, None)
        eq = pr((1 << j) - 1, (1 << (j + 1)) - 1)
        P[2 + j] = clip(eq / ge) if ge > 0 else 4065
    for b in range(1, MB + 1):
        for node in range(1, 1 << b):
            # node at depth d covers a contiguous range of r values
            d = node.bit_length() - 1
            pre = node - (1 << d)
            span = 1 << (b - d)
            r0 = pre * span
            base_a = (1 << b) - 1 + r0
            tot = pr(base_a, base_a + span)
            zero = pr(base_a, base_a + span // 2)
            P[2 + 2 * NU + TREE[b] + node - 1] = clip(zero / tot) if tot > 0 else 2048
    return P


def bitlen(x):
    return x.bit_length()


class Sel:
    """Backward predictor selection: running mean |residual| per candidate."""

    def __init__(self, cands):
        self.c = list(cands)
        self.cost = {j: 0 for j in self.c}

    def best(self):
        return min(self.c, key=lambda j: (self.cost[j], j))

    def upd(self, errs):
        for j in self.c:
            self.cost[j] += abs(errs[j]) * 16 - (self.cost[j] >> 4)


F = 8                         # fractional bits of the model states


def rs(x, s):
    """x / 2^s rounded (an arithmetic shift with rounding: no dead zone,
    so an EMA converges to the mean instead of drifting by 2^s units)."""
    return (x + (1 << (s - 1))) >> s


class TimeModel:
    def __init__(self, t0, hint, cands):
        self.t = t0
        self.dtp = hint
        self.ema = self.per = self.gper = hint << F
        self.grid = t0 << F
        self.sel = Sel(cands)

    def preds(self):
        return {0: self.dtp, 1: rs(self.ema, F),
                2: rs(self.grid + self.gper, F) - self.t,
                3: rs(self.per, F)}

    def upd(self, dt):
        p = self.preds()
        self.sel.upd({j: dt - p[j] for j in self.sel.c})
        D = dt << F
        # grid: a second-order loop (phase and period from the phase error)
        g = self.grid + self.gper
        t = self.t + dt
        pe = (t << F) - g
        if abs(pe) > (self.gper >> 2) + (8 << F):
            self.grid, self.gper = t << F, D      # rate change / gap: re-lock
        else:
            self.grid = g + rs(pe, 3)
            self.gper += rs(pe, 7)
        self.per += rs(D - self.per, 3 if abs(D - self.per) > (self.per >> 2) else 6)
        self.ema += rs(D - self.ema, 3)
        self.dtp = dt
        self.t = t


class ValModel:
    def __init__(self, cands):
        self.sel = Sel(cands)
        self.reset(0)

    def reset(self, n):
        self.prev = n
        self.l1 = self.l2 = self.hl = n << F
        self.ht = 0

    def preds(self):
        return {0: self.prev, 1: rs(self.l1, F), 2: rs(self.l2, F),
                3: rs(self.hl + self.ht, F)}

    def upd(self, n):
        p = self.preds()
        self.sel.upd({j: n - p[j] for j in self.sel.c})
        N = n << F
        self.l1 += rs(N - self.l1, 2)
        self.l2 += rs(N - self.l2, 4)
        pr = self.hl + self.ht
        e = N - pr
        self.hl = pr + rs(e, 3)
        self.ht += rs(e, 6)
        self.prev = n


class Act:
    """Activity class: log2 of the running mean |residual|."""

    def __init__(self, ncls):
        self.n, self.a = ncls, 0

    def cls(self):
        if self.n == 1:
            return 0
        return min(self.n - 1, bitlen(self.a >> 4) // 2)

    def upd(self, e):
        self.a += abs(e) * 16 - (self.a >> 3)


TREEON = True


def sint(rc, P, base, e=None):
    """Signed integer, Exp-Golomb binarization, adaptive bits."""
    enc = not rc.dec
    if not rc.bit(P, base, int(e != 0) if enc else None):
        return 0
    s = rc.bit(P, base + 1, int(e < 0) if enc else None)
    if enc:
        a = abs(e) - 1
        b = bitlen(a + 1) - 1
        b = min(b, NU - 1)
        for j in range(b):
            rc.bit(P, base + 2 + j, 1)
        if b < NU - 1:
            rc.bit(P, base + 2 + b, 0)
        else:
            rc.raw(32, a)
            return e
        if b:
            r = a + 1 - (1 << b)
            if b <= MB and TREEON:
                node = 1
                for j in range(b - 1, -1, -1):
                    bit = (r >> j) & 1
                    rc.bit(P, base + 2 + 2 * NU + TREE[b] + node - 1, bit)
                    node = node * 2 + bit
                return e
            rc.bit(P, base + 2 + NU + b, r >> (b - 1))
            if b > 1:
                rc.raw(b - 1, r & ((1 << (b - 1)) - 1))
        return e
    b = 0
    while b < NU - 1 and rc.bit(P, base + 2 + b):
        b += 1
    if b == NU - 1:
        a = rc.raw(32)
    else:
        r = 0
        if b and b <= MB and TREEON:
            node = 1
            for j in range(b):
                node = node * 2 + rc.bit(P, base + 2 + 2 * NU + TREE[b] + node - 1)
            r = node - (1 << b)
        elif b:
            r = rc.bit(P, base + 2 + NU + b) << (b - 1)
            if b > 1:
                r |= rc.raw(b - 1)
        a = (1 << b) + r - 1
    return -(a + 1) if s else a + 1


class Coder:
    """One chunk, AC version. P layout: time classes, value classes, misc."""

    def __init__(self, o, t0, hint, flags0, rc, pq=None):
        self.o, self.rc = o, rc
        nc = o['ncls']
        np_ = 2 * nc * SLOTS + 32
        self.P = [1 << 11] * np_ + ([0] * np_ if o['shift'] < 0 else [])
        if o['prior'] and pq is not None:
            assert nc == 1
            self.P[0:SLOTS] = prior_probs(pq[0])
            self.P[SLOTS:2 * SLOTS] = prior_probs(pq[1])
            if o['shift'] < 0:
                for i in range(2 * SLOTS):
                    self.P[np_ + i] = o.get('c0', 6)
        self.misc = 2 * nc * SLOTS
        self.tm = TimeModel(t0, hint, o['tpred'])
        self.vm = ValModel(o['vpred'])
        self.ta, self.va = Act(nc), Act(nc)
        self.flags = flags0
        self.has_k, self.k = False, 0
        self.first = True

    def _flags6(self, f=None):
        r = 0
        for j in range(6):
            b = self.rc.bit(self.P, self.misc + 8 + j, ((f >> j) & 1) if f is not None else None)
            r |= b << j
        return r

    def row(self, t=None, v=None, fl=None):
        rc, P, o = self.rc, self.P, self.o
        enc = not rc.dec
        nc = o['ncls']
        rc.cat = 'time'
        if self.first:
            self.first = False
            dt = 0
            if not enc:
                t = self.tm.t
        else:
            pr = self.tm.preds()[self.tm.sel.best()]
            base = self.ta.cls() * SLOTS
            r = sint(rc, P, base, (t - self.tm.t) - pr if enc else None)
            self.ta.upd(r)
            dt = r + pr
            self.tm.upd(dt)
            t = self.tm.t
        rc.cat = 'value'
        m = self.misc
        while True:
            if self.flags & 1:
                ch = rc.bit(P, m + 0, int(fl != self.flags) if enc else None)
                if not ch:
                    v = EMPTY
                    break
                self.flags = self._flags6(fl if enc else None)
                continue
            if enc:
                if fl != self.flags:
                    typ = 1
                elif v == EMPTY:
                    typ = 0
                elif v == NEGZ:
                    typ = 3
                else:
                    neg, mm, e = v
                    n = at_scale(mm, e, neg, self.k) if self.has_k else None
                    typ = 2 if n is None else -1
            esc = rc.bit(P, m + 1, int(typ >= 0) if enc else None)
            if esc:
                t1 = rc.bit(P, m + 2, (typ >> 1) if enc else None)
                t0 = rc.bit(P, m + 3 + t1, (typ & 1) if enc else None)
                typ = t1 * 2 + t0
                if typ == 1:
                    self.flags = self._flags6(fl if enc else None)
                    continue
                if typ == 0:
                    v = EMPTY
                elif typ == 3:
                    v = NEGZ
                else:
                    if enc:
                        neg, mm, e = v
                        k = 0 if mm == 0 else e - 5
                        n = at_scale(mm, e, neg, k)
                        rc.raw(8, k & 0xFF)
                        rc.raw(1, int(n < 0))
                        rc.raw(24, abs(n))
                    else:
                        k = rc.raw(8)
                        k = k - 256 if k > 127 else k
                        sg = rc.raw(1)
                        n = rc.raw(24)
                        n = -n if sg else n
                    self.has_k, self.k = True, k
                    self.vm.reset(n)
                    v = self._val(n)
                break
            pr = self.vm.preds()[self.vm.sel.best()]
            base = (nc + self.va.cls()) * SLOTS
            r = sint(rc, P, base, (n - pr) if enc else None)
            self.va.upd(r)
            n = pr + r
            self.vm.upd(n)
            v = self._val(n)
            break
        return t, v, self.flags

    def _val(self, n):
        if n == 0:
            return (False, 0, 0)
        a = abs(n)
        d = len(str(a))
        e = self.k + d - 1
        mm = a // 10 ** (d - 6) if d > 6 else a * 10 ** (6 - d)
        return (n < 0, mm, e)


def encode_chunk(ch, hint, o=DEF, acc=None, pq=None, out=None):
    """Coded bytes of a chunk; `out` (a dict) gets the activity bytes the
    next chunk's header carries."""
    rc = RC(shift=o['shift'])
    rc.acc = acc
    c = Coder(o, ch[0][0], hint, ch[0][2], rc, pq)
    for t, v, fl in ch:
        c.row(t, v, fl)
    if out is not None:
        out['pq'] = (act_q(c.ta.a), act_q(c.va.a))
    return rc.finish()


def decode_chunk(data, t0, nrows, hint, flags0, o=DEF, pq=None):
    rc = RC(data, shift=o['shift'])
    c = Coder(o, t0, hint, flags0, rc, pq)
    return [c.row() for _ in range(nrows)]


# ------------------------------------------------------------ Rice variant

def rice_bits(ch, hint, o=DEF):
    """Payload bits: DLG v1's adaptive Rice + escapes, new predictors."""
    at, av = dlog.Ad(4), dlog.Ad(16)
    tm = TimeModel(ch[0][0], hint, o['tpred'])
    vm = ValModel(o['vpred'])
    flags = ch[0][2]
    has_k, k = False, 0
    bits = 0
    for i, (t, v, fl) in enumerate(ch):
        if i:
            dt = t - tm.t
            r = dt - tm.preds()[tm.sel.best()]
            x = zz(r)
            bits += rice_len(x, at.k())
            at.upd(x)
            tm.upd(dt)
        while True:
            if flags & 1:
                bits += 1 if fl == flags else 7
                if fl != flags:
                    flags = fl
                    continue
                break
            kv = av.k()
            if fl != flags:
                bits += rice_len(0, kv) + 8
                flags = fl
                continue
            if v in (EMPTY, NEGZ):
                bits += rice_len(0, kv) + 2
                break
            neg, m, e = v
            n = at_scale(m, e, neg, k) if has_k else None
            if n is None:
                k = 0 if m == 0 else e - 5
                n = at_scale(m, e, neg, k)
                has_k = True
                vm.reset(n)
                bits += rice_len(0, kv) + 35
                break
            u = zz(n - vm.preds()[vm.sel.best()]) + 1
            bits += rice_len(u, kv)
            av.upd(u)
            vm.upd(n)
            break
    return bits
