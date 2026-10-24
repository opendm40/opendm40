#!/usr/bin/env python3
"""tools/logfmt-test: decode the dumped LOG:DATA? replies (logger.rs test
`fmt_dump_for_the_host_decoder`) with tools/dm40scpi.py `log_reply` and
compare them with the rows fed to the logger: seq, value and lost_before
exact; the stamp exact for DLG, within half a period (+1 us of rounding)
for PACKed.

    check.py DIR
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, '..'))
import dm40scpi  # noqa: E402

d = sys.argv[1]
fail = 0
for name in ('a768', 'wrap384', 'ad1'):
    lines = open(os.path.join(d, name + '.txt')).read().split('\n')
    p_ns = int(lines[0])
    ref = [tuple(int(x) for x in ln.split()) for ln in lines[1:] if ln]
    for fmt in ('packed', 'dlg'):
        b = open(os.path.join(d, '%s-%s.bin' % (name, fmt)), 'rb').read()
        rows, o, replies, nbytes = [], 0, 0, 0
        while o < len(b):
            n = struct.unpack_from('<I', b, o)[0]
            r, info = dm40scpi.log_reply(b[o + 4:o + 4 + n])
            if r:
                assert info['format'] == fmt and abs(info['a'] - 0.25) < 1e-7, info
                replies += 1
                nbytes += n
            rows += r
            o += 4 + n
        bad = []
        if len(rows) != len(ref):
            bad.append('%d rows, %d fed' % (len(rows), len(ref)))
        for got, want in zip(rows, ref):
            seq, ts, code, lost = got
            wseq, wts, wcode, wlost = want
            dt = (ts - wts + (1 << 31)) % (1 << 32) - (1 << 31)
            tol = 0 if fmt == 'dlg' else (p_ns // 2 + 999) // 1000 + 1
            if (seq, code, lost) != (wseq, wcode, wlost) or abs(dt) > tol:
                bad.append('row %d: %r, want %r' % (wseq, got, want))
                break
        print('%s %-7s %-6s %d rows in %d replies, %.2f B/sample%s'
              % ('FAIL' if bad else 'ok  ', name, fmt, len(rows), replies, nbytes / max(len(rows), 1),
                 ': ' + '; '.join(bad) if bad else ''))
        fail += bool(bad)
sys.exit(1 if fail else 0)
