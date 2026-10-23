#!/usr/bin/python3
"""Device test 6.5 (docs/dmm-input-scope-feasibility.md): the DC-coupled V
row on AD2 (feasibility 1.3 last row, E7) against AD1, per DC V tap.

For every DC V range (0..3 = 6 / 60 / 600 / 1000 V) and DC level, the
calibrator (tools/mr9270.py, V/COM) is set, then per row variant:

  DBG:AD2:DCV? range,r26,n,flags   (fw-rs src/ad2exp.rs `dcv_test`)
      A  AD1 alone in the row (SENSE - PB5, ENAD2 off)
      B  AD1 + AD2: the AD2 mean over the same AD1 conversions
      C  AD2 alone (AD1 off: what the stream sees)
      D  C with OP1+ on AGND: the zero of OP1 + AD2
  READ?                            the DMM's DC V reading at that range
                                   (the DC V row, calibrated): the reference

flags: bit 0 SAD2IN = RLU (else AGND), bits 2:1 OP1CHOP (0 off, 2 = 2 kHz,
the stock AC V row), bit 3 OP2 x1 instead of OP1 (the power meter's path).

Per range and variant it fits V_ref = a + b * code (code = the phase C
mean) and reports: b vs the nominal K * 1.25 / 2^18 (gain error), a (the
offset at the jack), the largest residual (linearity, % of range), the
zero D against a, the noise (sd of C) in codes / jack volts / ENOB, the
AD2 mean with AD1 on (B) vs off (C), AD1 with AD2 on (B) vs off (A), and
AD1-in-the-row volts vs READ? (is AD1 on SENSE - PB5 a usable self-cal
reference). Raw rows go to a CSV.

    /usr/bin/python3 tools/ad2dcv.py -o dcv.csv            # all ranges
    /usr/bin/python3 tools/ad2dcv.py --ranges 0 --points 0,1,2,5,10
    /usr/bin/python3 tools/ad2dcv.py --no-cal --points 1   # calibrator left alone
    tools/ad2dcv.py --usbip 1-0 --no-cal --points 1        # the emulator

The meter must be on fw-rs, USB connected (USBTMC), leads on V / COM. The
range stays manual during the run; at the end CONF:VOLT:DC (AUTO) and the
calibrator back to 1 V DC, output on. Levels are capped at 24 V (the
calibrator's real top level, tools/mr9270.py VOLTAGE_MAX).
"""
import argparse
import csv
import math
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

K = [10.0, 100.0, 1000.0, 10000.0]
FULL = [6.0, 60.0, 600.0, 1000.0]
VMAX = 24.0  # the calibrator's top DC level (mr9270.VOLTAGE_MAX; a 30 V request gives 23.99 V)
POINTS = {0: [0, 0.5, 1, 2, 5, 10], 1: [0, 1, 5, 10, 20, 24], 2: [0, 10, 20, 24], 3: [0, 10, 20, 24]}
VARIANTS = [0, 1, 4, 8]  # AGND chop off, RLU chop off, AGND 2 kHz chopper, OP2


def flags_name(f):
    s = ('OP2' if f & 8 else 'OP1') + ' ' + ('RLU' if f & 1 else 'AGND')
    return s + ' chop%d' % ((f >> 1) & 3)


def parse(r):
    """DCV? reply -> dict."""
    p = r.split(';')
    h = [int(x) for x in p[0].split(',')]
    a = [int(x) for x in p[1].split(',')]
    b = [int(x) for x in p[2].split(',')]
    c = [int(x) for x in p[3].split(',')]
    d = [int(x) for x in p[4].split(',')]
    to = int(p[5])
    uv = [int(x) for x in p[6].split(',')] if len(p) > 6 else [0, 0]
    return {
        'range': h[0], 'r26': h[1], 'flags': h[2], 'n': h[3],
        'a_n': a[0], 'a_ad1': a[1] / 1000, 'a_ad1_sd': a[2] / 1000,
        'b_n': b[0], 'b_ad1': b[1] / 1000, 'b_ad1_sd': b[2] / 1000,
        'b_ad2_n': b[3], 'b_ad2': b[4] / 1000, 'b_ad2_sd': b[5] / 1000, 'b_min': b[6], 'b_max': b[7],
        'c_n': c[0], 'c_ad2': c[1] / 1000, 'c_sd': c[2] / 1000, 'c_min': c[3], 'c_max': c[4],
        'd_n': d[0], 'd_ad2': d[1] / 1000, 'd_sd': d[2] / 1000, 'd_min': d[3], 'd_max': d[4],
        'timeouts': to, 'a_v': uv[0] / 1e6, 'b_v': uv[1] / 1e6,
    }


def fit(xs, ys):
    """Least squares y = a + b x; (a, b, residuals)."""
    n = len(xs)
    if n < 2:
        return (ys[0] if ys else 0.0), 0.0, [0.0] * n
    mx, my = sum(xs) / n, sum(ys) / n
    sxx = sum((x - mx) ** 2 for x in xs)
    if sxx == 0:
        return my, 0.0, [0.0] * n
    b = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / sxx
    a = my - b * mx
    return a, b, [y - (a + b * x) for x, y in zip(xs, ys)]


def query_long(link, line):
    """A slow query (DCV? blocks the meter for ~1-3 s; the emulator runs it
    slower than real time): the emulator link's 20 s reply timeout raised."""
    if hasattr(link, 't'):
        link.t.write(line)
        return link.t.read(maxlen=8192, timeout=600).decode(errors='replace').strip()
    return link.query(line)


def reading(link, n=2):
    v = None
    for _ in range(n):
        v = float(query_long(link, 'READ?'))
    return v


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter, epilog=__doc__)
    ap.add_argument('--usbip', metavar='BUSID', help="the emulator's USB/IP server (emu/run.py --usbip)")
    ap.add_argument('--cal', default='/dev/ttyACM1', help='MR9270 calibrator port (default /dev/ttyACM1)')
    ap.add_argument('--no-cal', action='store_true', help='do not drive the calibrator (one level: --points)')
    ap.add_argument('--ranges', default='0,1,2,3')
    ap.add_argument('--points', help='DC levels (V, comma separated) for every range (default per range)')
    ap.add_argument('--variants', default=','.join(str(v) for v in VARIANTS), help='flags values')
    ap.add_argument('--r26', type=lambda x: int(x, 0), default=0x24, help='AD2 rung (default 0x24: 4800 S/s chopped)')
    ap.add_argument('-n', type=int, default=8, help='AD1 conversions per phase (1..16)')
    ap.add_argument('--settle', type=float, default=2.0, help='seconds after a calibrator step')
    ap.add_argument('-o', '--out', default='ad2dcv.csv')
    ap.add_argument('--analyze', metavar='CSV', help='only print the analysis of an earlier run\'s CSV')
    args = ap.parse_args()
    if args.analyze:
        with open(args.analyze) as fh:
            rows = [{k: float(v) for k, v in d.items()} for d in csv.DictReader(fh)]
        for d in rows:
            for k in ('range', 'flags', 'c_n'):
                d[k] = int(d[k])
        analyze(rows, sorted({d['range'] for d in rows}), sorted({d['flags'] for d in rows}))
        return 0

    import dm40scpi
    link = dm40scpi.UsbipLink(args.usbip) if args.usbip else dm40scpi.UsbLink()
    cal = None
    if not args.no_cal:
        import mr9270
        cal = mr9270.MR9270(args.cal).connect()
    ranges = [int(x) for x in args.ranges.split(',')]
    variants = [int(x, 0) for x in args.variants.split(',')]
    rows = []
    try:
        link.query('CONF:VOLT:DC')
        time.sleep(1.0)
        dm40scpi.check_err(link, 'CONF:VOLT:DC')
        for r in ranges:
            pts = [float(x) for x in args.points.split(',')] if args.points else POINTS[r]
            for v in pts:
                if abs(v) > VMAX:
                    sys.exit('ad2dcv: %g V is above the %g V cap' % (v, VMAX))
                if cal:
                    cal.set_output('voltage', v, enable=True)
                    time.sleep(args.settle)
                for f in variants:
                    q = 'DBG:AD2:DCV? %d,%d,%d,%d' % (r, args.r26, args.n, f)
                    t0 = time.time()
                    rep = query_long(link, q)
                    dt = time.time() - t0
                    dm40scpi.check_err(link, q)
                    d = parse(rep)
                    d['ref'] = reading(link)
                    d['set'] = v if cal else float('nan')
                    d['secs'] = dt
                    rows.append(d)
                    print('r%d %6.2f V %-18s ref %+.5f V | A %.0f B %.0f (AD1 %+.5f V) | B AD2 %.1f sd %.1f n %d | '
                          'C %.1f sd %.1f n %d | D %.1f sd %.1f | to %d %.1f s' % (
                              r, v, flags_name(f), d['ref'], d['a_ad1'], d['b_ad1'], d['b_v'], d['b_ad2'], d['b_ad2_sd'],
                              d['b_ad2_n'], d['c_ad2'], d['c_sd'], d['c_n'], d['d_ad2'], d['d_sd'], d['timeouts'], dt),
                          flush=True)
    finally:
        try:  # the range back to AUTO: CONF of the function in force is a no-op
            link.query('CONF:VOLT:AC')
            link.query('CONF:VOLT:DC')
        except Exception:
            pass
        if cal:
            try:
                cal.safe_shutdown()
            finally:
                cal.close()
        link.close()
        if rows:
            with open(args.out, 'w', newline='') as fh:
                w = csv.DictWriter(fh, fieldnames=list(rows[0].keys()))
                w.writeheader()
                w.writerows(rows)
            print('raw rows: %s' % args.out)

    analyze(rows, ranges, variants)
    return 0


def analyze(rows, ranges, variants):
    """Per range and variant (levels where READ? was over range dropped)."""
    print('\nrange variant          pts gain fit/nom-1  0 V at code  zero D code  maxres %FS  sd codes  sd @jack  ENOB  '
          'B-C codes  AD1 on-off cts  AD1-in-row - READ?')
    for r in ranges:
        for f in variants:
            s = [d for d in rows if d['range'] == r and d['flags'] == f and d['c_n'] > 0 and abs(d['ref']) < 1e30]
            if not s:
                continue
            nomf = (K[0] if (f & 8 and r == 0) else K[r]) * 1.25 / 262144.0
            xs = [d['c_ad2'] for d in s]
            ys = [d['ref'] for d in s]
            a, b, res = fit(xs, ys)
            if b == 0:  # one level: the nominal gain, the offset from it
                b = nomf
                a = ys[0] - b * xs[0]
            zero = sum(d['d_ad2'] for d in s) / len(s)
            sd = sum(d['c_sd'] for d in s) / len(s)
            enob = math.log2(2 ** 19 / sd) if sd > 0 else float('inf')
            bc = max((d['b_ad2'] - d['c_ad2'] for d in s), key=abs)
            ab = max((d['b_ad1'] - d['a_ad1'] for d in s), key=abs)
            rowref = max((d['b_v'] - d['ref'] for d in s), key=abs)
            print('r%d    %-18s %3d  %+8.4f %%   %+10.1f   %+10.1f   %8.4f   %7.2f  %8.5f  %5.1f  %+8.1f  %+12.1f  %+10.5f V' % (
                r, flags_name(f), len(s), (b / nomf - 1) * 100, -a / b, zero, max(abs(x) for x in res) / FULL[r] * 100,
                sd, sd * b, enob, bc, ab, rowref))


if __name__ == '__main__':
    sys.exit(main())
