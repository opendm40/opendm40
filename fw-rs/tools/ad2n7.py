#!/usr/bin/python3
"""Datasheet 13.1 note 7 in the stock DC V / DC mV rows (fw-rs DBG:AD2:N7?,
src/ad2exp.rs `n7_test`): does AD2 (and AD3 + RMS) left on for the whole
row, enabled once with it and never toggled, shift AD1's DC reading against
the stock row (AD2 / AD3 off)? The question behind it: a DC V "AC?" hint
from AD2 + RMS instead of the comparator time-sharing PB1 / SENSE.

Per DC range (MF items: 0 = 600 mV DC mV, 1..4 = DC V 6 / 60 / 600 /
1000 V) at 0 V and one higher level (calibrator, V/COM), the AD1 means of
n conversions (the row's OSR, chopped) in states 0 (stock), 1 (AD2 on
OP1O, 76.8 kS/s), 2 (1 + AD3 + RMS / LPF), then 0 again (drift). Prints the
shift against state 0 in counts and ppm of the level's code, the offset
shift (0 V) and the gain shift ((d_level - d_0) / code_level), and the
noise (sd). The supply current is not measured (no readout on the meter).

    /usr/bin/python3 tools/ad2n7.py [-n 20] [--items 0,1,2,3,4] [-o n7.csv]
"""
import argparse
import csv
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
LEVELS = {0: 0.3, 1: 3.0, 2: 24.0, 3: 24.0, 4: 24.0}
NAMES = ['600 mV', '6 V', '60 V', '600 V', '1000 V']


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0], epilog=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--cal', default='/dev/ttyACM1')
    ap.add_argument('--items', default='0,1,2,3,4')
    ap.add_argument('-n', type=int, default=20)
    ap.add_argument('--settle', type=float, default=2.0)
    ap.add_argument('-o', '--out', default='ad2n7.csv')
    args = ap.parse_args()
    import dm40scpi
    import mr9270
    link = dm40scpi.UsbLink()
    cal = mr9270.MR9270(args.cal).connect()
    rows = []
    try:
        link.query('CONF:VOLT:DC')
        time.sleep(1.5)
        for item in [int(x) for x in args.items.split(',')]:
            for v in (0.0, LEVELS[item]):
                cal.set_output('voltage', v, enable=True)
                time.sleep(args.settle)
                for k, st in enumerate((0, 1, 2, 0)):
                    r = link.query('DBG:AD2:N7? %d,%d,%d' % (item, st, args.n))
                    dm40scpi.check_err(link, 'DBG:AD2:N7?')
                    f = [int(x) for x in r.split(',')]
                    d = {'item': item, 'level': v, 'state': st, 'pass': k, 'n': f[3], 'mean': f[4] / 1000,
                         'sd': f[5] / 1000, 'min': f[6], 'max': f[7], 'timeout': f[8]}
                    rows.append(d)
                    print('%-7s %5.2f V state %d: n %d mean %12.1f sd %7.1f%s' % (
                        NAMES[item], v, st, d['n'], d['mean'], d['sd'], ' TIMEOUT' if d['timeout'] else ''), flush=True)
    finally:
        try:
            link.query('CONF:VOLT:DC')
        except Exception:
            pass
        cal.safe_shutdown()
        cal.close()
        link.close()
        if rows:
            with open(args.out, 'w', newline='') as fh:
                w = csv.DictWriter(fh, fieldnames=list(rows[0].keys()))
                w.writeheader()
                w.writerows(rows)
    print('\nrange    level   AD2 on (1)          AD2+AD3+RMS (2)     drift 0->0  sd    | offset shift 1 / 2   gain shift 1 / 2')
    for item in sorted({d['item'] for d in rows}):
        res = {}
        for v in sorted({d['level'] for d in rows if d['item'] == item}):
            s = [d for d in rows if d['item'] == item and d['level'] == v]
            base = [d['mean'] for d in s if d['state'] == 0]
            b = sum(base) / len(base)
            d1 = next(d['mean'] for d in s if d['state'] == 1) - b
            d2 = next(d['mean'] for d in s if d['state'] == 2) - b
            drift = base[-1] - base[0]
            sd = max(d['sd'] for d in s)
            ppm = lambda x: x / b * 1e6 if abs(b) > 1000 else float('nan')
            res[v] = (b, d1, d2)
            print('%-7s %6.2f  %+8.1f (%+7.0f ppm) %+8.1f (%+7.0f ppm) %+8.1f  %6.1f' % (
                NAMES[item], v, d1, ppm(d1), d2, ppm(d2), drift, sd))
        if len(res) == 2:
            (b0, a1, a2), (bl, l1, l2) = res[min(res)], res[max(res)]
            print('%-7s         offset %+.1f / %+.1f counts, gain %+.0f / %+.0f ppm' % (
                NAMES[item], a1, a2, (l1 - a1) / (bl - b0) * 1e6, (l2 - a2) / (bl - b0) * 1e6))
    return 0


if __name__ == '__main__':
    sys.exit(main())
