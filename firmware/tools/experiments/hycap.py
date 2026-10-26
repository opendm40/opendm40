#!/usr/bin/python3
"""Host side of DBG:HY:CAP? (src/hyexp.rs): row-byte overrides on the row the
DMM last wrote, an SPI-timed step, then raw AD1 / RMS / AD2 results with
timestamps. Used for docs/hy3131-frontend-2026-10-26.md.

    from hycap import cap
    base, timeouts, pts = cap(kind, n, npre, pre={i: v}, step=[(i, v)])

kind 0 = AD1, 1 = RMS (37 bits unsigned), 2 = AD2, 3 = AD2 + AD3; i = row
byte (R20 + i: R33 is 19, not 13), 100..103 = the R31 square wave (A, B,
half period us lo / hi); pts = [(t_us after the step, value[, ad3])].
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))  # firmware/tools: dm40scpi
import dm40scpi

_l = None


def link():
    global _l
    if _l is None:
        _l = dm40scpi.UsbLink()
    return _l


def cap(kind, n, npre, pre={}, step=[]):
    pairs = list(pre.items()) + [(32 + i, v) for i, v in step]
    q = 'DBG:HY:CAP? ' + ','.join(map(str, [kind, n, npre] + [x for p in pairs for x in p]))
    if len(q) > 64:  # the meter's command-line limit: the pairs as one #H literal
        q = 'DBG:HY:CAP? %d,%d,%d,#H' % (kind, n, npre) + ''.join('%02X%02X' % p for p in pairs)
    assert len(q) <= 64, q
    r = link().query(q)
    f = r.split(';')
    base = bytes.fromhex(f[0])
    _, to = map(int, f[1].split(','))
    pts = [tuple(map(int, x.split(','))) for x in f[2:]]
    return base, to, pts
