#!/usr/bin/env python3
"""fw-rs AC+DC methods A / B / C (fw-rs/src/acdcm.rs, DBG:ACDC:METH) in the
emulator: one run per method, the same timed signals (V then A: DC, AC,
mixed, DC with 50 Hz ripple, DC with off-notch 45 Hz ripple, V DC again
on the 6 V range after the 60 V one: a range visited again), SCPI over
USB/IP (DBG:PING?, FETCh?, DBG:ACDC:STAT? in a loop). Per phase it reports
the error of the shown total against hypot(AC, DC) of the source (mean and
peak-to-peak over the phase's 6th to 1.5th last second), the reading rate
(DBG:ACDC:STAT? n_rd: AC+DC readings / s) and the settle time (first sample
from which the total stays within 0.1 % + 2 digits of the truth; nan:
never in the phase).

The HY3131 model's note 7 (AD1 shifted while AD2 / AD3 convert, the device's
per-range offsets, feasibility 13.2) is on in every run; `B~` adds a
non-repeating AD1 gain shift of +-0.2 % per AD1 enable (n7g, the device's
60 V gain did not repeat), which B's entry zero cannot remove.

  emu/hy3131/acdcmethtest.py --app fw-rs/dm40-fw-native.bin             # A B C B~ in parallel
  emu/hy3131/acdcmethtest.py --app X --meths C --port 18120
Results: emu/out/acdcmeth-<meth>.json, the table on stdout. Exit 1 when a
method misses its accuracy bound (CHECK below) in a phase.
"""
import argparse
import json
import math
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
OUT = os.path.join(EMU, 'out')
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', help='fw-rs native image')
ap.add_argument('--port', type=int, default=18110, help='first USB/IP port (one per method)')
ap.add_argument('--meths', default='A,B,C,B~')
ap.add_argument('--child', action='store_true', help=argparse.SUPPRESS)
ap.add_argument('--analyse', action='store_true', help='only the table of the last runs (emu/out/acdcmeth-*.json)')
A = ap.parse_args()
if A.child:
    os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

PH = 12.0  # s per phase
# (signal spec, dc, ac rms, unit 'V' / 'A', label)
SIG = [
    ('open;dcv:5', 5.0, 0.0, 'V', 'V DC 5'),
    ('open;acv:2@50', 0.0, 2.0, 'V', 'V AC 2 @50'),
    ('open;acv:1@50,dc=1', 1.0, 1.0, 'V', 'V AC 1 + DC 1'),
    ('open;acv:0.5@50,dc=12', 12.0, 0.5, 'V', 'V DC 12 + 0.5 @50'),
    ('open;acv:0.5@45,dc=12', 12.0, 0.5, 'V', 'V DC 12 + 0.5 @45'),
    ('open;dcv:5', 5.0, 0.0, 'V', 'V DC 5 again (r1 -> r0)'),
    ('open;dca:0.3', 0.3, 0.0, 'A', 'A DC 0.3'),
    ('open;aca:0.2@50', 0.0, 0.2, 'A', 'A AC 0.2 @50'),
    ('open;aca:0.2@50,dc=0.2', 0.2, 0.2, 'A', 'A AC 0.2 + DC 0.2'),
    ('open;aca:0.05@45,dc=0.3', 0.3, 0.05, 'A', 'A DC 0.3 + 0.05 @45'),
]
# The firmware's uptime (DBG:PING?) lags the virtual time of the event
# script by the boot: 1.34 s (B / C runs: their first reading after the
# 5 V -> 2 V AC step at 12 s came at uptime 10.66 / 10.68 s; the boot is
# deterministic in virtual time).
LAG = 1.34
T_A = 6 * PH  # the A phases start here (CURR:COUP ACDC)
RUN_FOR = len(SIG) * PH + 2
# Accuracy bound per phase (|mean error| %, p-p %) every method must meet.
CHECK = (0.3, 1.0)


def num(s):
    try:
        v = float(s)
    except ValueError:
        return float('nan')
    return float('nan') if abs(v) > 1e37 else v


def run(meth, port, res):
    tag = 'acdcmeth-' + meth.replace('~', 'g')
    if uc.port_busy(port):
        res[meth] = {'err': 'port %d busy' % port}
        return
    rtag = '%s-%d-%d' % (tag, port, os.getpid())
    os.makedirs(OUT, exist_ok=True)
    sp = os.path.join(OUT, rtag + '.script')
    extra = 'n7g:0.002;' if meth == 'B~' else ''
    with open(sp, 'w') as f:
        for i, (spec, *_x) in enumerate(SIG):
            f.write('%g dmm %s%s\n' % (i * PH, extra, spec))
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--fast', '--usbip', '3', '--usbip-port', str(port),
           '--run-for', str(RUN_FOR), '--script', sp, '--name', rtag, '--app', os.path.abspath(A.app)]
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    rows = []
    tm = None
    t_wall = time.time()
    while p.poll() is None and tm is None and time.time() - t_wall < 300:
        try:
            tm = uc.Usbtmc(uc.Dev('1-0'))
        except Exception:
            tm = None
            time.sleep(1)
    switched = False
    if tm is not None:
        try:
            m = meth[0]
            tm.write('DBG:ACDC:METH %s' % m)
            tm.write('VOLT:COUP ACDC')
            while p.poll() is None:
                row = {}
                for q in ['DBG:PING?', 'FETC?', 'DBG:ACDC:STAT?', 'FUNC?']:
                    tm.write(q)
                    row[q] = tm.read(timeout=60).decode(errors='replace').strip()
                t = int(row['DBG:PING?'].split(',')[0]) / 1000.0
                st = row['DBG:ACDC:STAT?'].split(',')
                rows.append({'t': t, 'v': num(row['FETC?']), 'st': st, 'f': row['FUNC?']})
                if t + LAG >= T_A + 0.3 and not switched:
                    tm.write('CURR:COUP ACDC')
                    switched = True
        except Exception as e:
            print(meth, 'link ended:', e, flush=True)
    out, _ = p.communicate()
    for x in (sp, os.path.join(OUT, 'flash-%s.bin' % rtag), os.path.join(OUT, rtag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
    res[meth] = {'rows': rows, 'rc': p.returncode, 'log': out[-800:] if p.returncode else ''}
    with open(os.path.join(OUT, tag + '.json'), 'w') as f:
        json.dump(res[meth], f)


def analyse(meth, d):
    """Per phase: (label, mean err %, p-p %, rate /s, settle s, n samples)."""
    rows = d.get('rows', [])
    lag = LAG
    out = []
    for i, (_spec, dc, ac, unit, label) in enumerate(SIG):
        t0, t1 = i * PH, (i + 1) * PH
        truth = math.hypot(dc, ac)
        ph = [r for r in rows if t0 <= r['t'] + lag < t1]
        win = [r for r in ph if t1 - 6.0 <= r['t'] + lag <= t1 - 1.5 and not math.isnan(r['v'])]
        if not win:
            out.append((label, float('nan'), float('nan'), float('nan'), float('nan'), 0))
            continue
        e = [(r['v'] - truth) / truth * 100 for r in win]
        mean = sum(e) / len(e)
        pp = max(e) - min(e)
        # reading rate: DBG:ACDC:STAT? n_rd (AC+DC readings, every method)
        n = [int(r['st'][8]) for r in win]
        rate = (n[-1] - n[0]) / max(1e-3, win[-1]['t'] - win[0]['t'])
        tol = 0.001 * truth + 0.002 * (1 if unit == 'V' else 0.1)
        settle = float('nan')
        for k, r in enumerate(ph):
            if all(not math.isnan(x['v']) and abs(x['v'] - truth) <= tol for x in ph[k:]):
                settle = r['t'] + lag - t0
                break
        out.append((label, mean, pp, rate, settle, len(win)))
    return out


if __name__ == '__main__':
    meths = A.meths.split(',')
    res = {}
    if A.child:
        run(meths[0], A.port, res)
        sys.exit(0)
    ps = None if A.analyse else []
    for i, m in enumerate(meths if ps is not None else []):
        ps.append(subprocess.Popen([sys.executable, __file__, '--child', '--app', A.app, '--meths', m, '--port', str(A.port + i)]))
        time.sleep(2)
    for p in ps or []:
        p.wait()
    for m in meths:
        try:
            with open(os.path.join(OUT, 'acdcmeth-' + m.replace('~', 'g') + '.json')) as f:
                res[m] = json.load(f)
        except OSError as e:
            res[m] = {'err': str(e)}
    ok = True
    print('\n%-22s' % 'phase' + ''.join('| %-34s' % ('method ' + m) for m in meths))
    print('%-22s' % '' + ''.join('| %-34s' % 'err% mean  p-p   rd/s settle_s' for _ in meths))
    tabs = {m: analyse(m, res.get(m, {})) for m in meths}
    for i in range(len(SIG)):
        line = '%-22s' % SIG[i][4]
        for m in meths:
            lab, mean, pp, rate, settle, n = tabs[m][i]
            line += '| %+7.3f %6.3f %5.1f %6.1f     ' % (mean, pp, rate, settle)
            if n == 0 or abs(mean) > CHECK[0] or pp > CHECK[1]:
                if m != 'B~':
                    ok = False
                line += '*'
        print(line)
    for m in meths:
        if res.get(m, {}).get('err') or res.get(m, {}).get('rc'):
            print(m, 'run problem:', res[m].get('err', ''), res[m].get('log', '')[-300:])
            ok = False
    print('RESULT', 'OK' if ok else 'FAIL')
    sys.exit(0 if ok else 1)
