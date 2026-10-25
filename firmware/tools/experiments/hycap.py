#!/usr/bin/python3
"""Host side of DBG:HY:CAP? (src/hyexp.rs): row-byte overrides on the row the
DMM last wrote, an SPI-timed step, then raw AD1 / RMS / AD2 results with
timestamps. Used for docs/hy3131-frontend-2026-10-26.md.

    from hycap import cap
    base, timeouts, pts = cap(kind, n, npre, pre={i: v}, step=[(i, v)])

kind 0 = AD1, 1 = RMS (37 bits unsigned), 2 = AD2; i = row byte (R20 + i:
R33 is 19, not 13); pts = [(t_us after the step, value)].
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
    args = [kind, n, npre]
    for i, v in pre.items():
        args += [i, v]
    for i, v in step:
        args += [32 + i, v]
    r = link().query('DBG:HY:CAP? ' + ','.join(map(str, args)))
    f = r.split(';')
    base = bytes.fromhex(f[0])
    _, to = map(int, f[1].split(','))
    pts = [tuple(map(int, x.split(','))) for x in f[2:]]
    return base, to, pts
