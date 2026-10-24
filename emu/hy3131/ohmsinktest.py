#!/usr/bin/env python3
"""RES AUTO on an input the 6 Mohm and 600 kohm ranges disagree on (device
a2e9766, firmware/docs/device-check-2026-10-21.md "a2e9766: bug-fix recheck"
note 1: the calibrator's disabled output read 0.104 Mohm on 6 Mohm and OL on
600 kohm, AUTO stepped between them for good, DBG:READout? `P,MΩ,NAN,0`, and
8 of 8 FETCh? got no reply within the driver's 5 s).

Model: `isink:<A>`, a current-limited path from V to COM (INFERRED as the
cause): it holds the node at 0 V while that takes at most <A>, else it takes
<A>. Set just under the model's 6 Mohm test current (~0.139 uA; the device's
0.215 uA, docs/data/short.csv), it takes nearly all of it: 6 Mohm reads ~0.1
Mohm. The 600 kohm range drives ten times that current: OL.

Each window: input at its start, CONF:VOLT:DC at +3 s, at +8 s (fw uptime):
  fetch  CONF:RES, then FETCh? back to back for HOLD s, each reply timed:
         at most FETCH_MAX s (+9.91E37 while the range moves).
  poll   as fetch, with RES:RANG? / DBG:READout? after each FETCh?. AUTO
         must settle: no range change in the last QUIET s, and a final
         (amber, kind F) readout in them.
  read   CONF:RES;:READ?: an answer within READ_MAX s (under 5 s).
  leave  as poll; then the input becomes a 4.7 kohm resistor (at +HOLD s):
         4.7 kohm final within 3.5 s (the hunt rule must let go of 6 Mohm).

  emu/hy3131/ohmsinktest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 --port 64810
"""
import argparse
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
OUT = os.path.join(EMU, 'out')
BOOT_OFS = 0.906  # virtual time - fw uptime (switchlat.py)
T0 = 8.0
W = 24.0
HOLD = 9.0
QUIET = 4.0
FETCH_MAX = 2.5
READ_MAX = 4.8
SINK = 'isink:1.377e-7'

# name, input, rate, how
SCEN = [
    ('sink-fetch', SINK, 'MED', 'fetch'),
]

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--port', type=int, default=64810, help='USB/IP port')
ap.add_argument('-v', '--verbose', action='store_true', help='print every poll')
ap.add_argument('--no-fetch', action='store_true', help='poll without FETCh? (to watch the hunt)')
ap.add_argument('names', nargs='*', help='scenarios (default all)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

scen = [s for s in SCEN if not A.names or s[0] in A.names]
os.makedirs(OUT, exist_ok=True)
tag = 'ohmsink-%d-%d' % (A.port, os.getpid())
script = os.path.join(OUT, tag + '.script')
with open(script, 'w') as f:
    for k, sc in enumerate(scen):
        f.write('%.3f dmm %s\n' % (T0 + k * W + BOOT_OFS, 'open;' + sc[1]))
        if sc[3] == 'leave':
            f.write('%.3f dmm %s\n' % (T0 + k * W + 8 + HOLD + BOOT_OFS, 'open;ohm:4700'))
end = T0 + len(scen) * W + 2 + BOOT_OFS
cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01', '--usbip-port', str(A.port),
       '--run-for', '%.1f' % end, '--name', tag, '--script', script]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
bad = []


def num(r):
    try:
        return float(r.split(',')[0])
    except ValueError:
        return float('nan')


try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('ohmsinktest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60):
        t.write(line)
        return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000.0

    def wait_until(s):
        while up() < s:
            time.sleep(0.2)

    for k, sc in enumerate(scen):
        name, _, rate, how = sc
        w0 = T0 + k * W
        wait_until(w0 + 3)
        q('CONF:VOLT:DC;:RATE %s;:SYST:ERR?' % rate)
        wait_until(w0 + 8)
        why = []
        if how == 'read':
            t1 = up()
            r = q('CONF:RES;:READ?')
            dt = up() - t1
            err = q('SYST:ERR?')
            print('     READ? %s in %.2f s (%s)' % (r, dt, err))
            if dt > READ_MAX:
                why.append('READ? in %.2f s (max %g)' % (dt, READ_MAX))
        else:
            t.write('CONF:RES')
            t1 = up()
            rows = []  # (s, FETC? value, FETC? reply s, range, readout kind, unit)
            while True:
                a = up()
                f = '+9.91E37' if A.no_fetch else q('FETC?')
                b = up()
                if how == 'fetch':
                    rng, ro = 'nan', ['', '']
                else:
                    rng, ro = q('RES:RANG?;:DBG:READ?').split(';', 1)
                    ro = ro.split(',')
                rows.append((b - t1, num(f), b - a, num(rng), ro[0], ro[1] if len(ro) > 1 else ''))
                if A.verbose:
                    print('       %6.2f FETC? %-16s %.2f s  range %-10s %s' % (b - t1, f, b - a, rng, ','.join(ro)))
                if b - t1 > HOLD:
                    break
                time.sleep(0.05)
            slow = [r for r in rows if r[2] > FETCH_MAX]
            if slow:
                why.append('%d of %d FETC? slower than %g s (max %.2f s)' % (len(slow), len(rows), FETCH_MAX, max(r[2] for r in slow)))
            print('     %d FETC?, max %.2f s, last %g' % (len(rows), max(r[2] for r in rows), rows[-1][1]))
            if how != 'fetch':
                ranges = [r[3] for r in rows]
                steps = [i for i in range(1, len(rows)) if ranges[i] != ranges[i - 1]]
                late = [i for i in steps if rows[i][0] > HOLD - QUIET]
                if late:
                    why.append('range still moving after %.0f s (%d changes, e.g. %g -> %g at +%.2f s)' % (
                        HOLD - QUIET, len(late), ranges[late[-1] - 1], ranges[late[-1]], rows[late[-1]][0]))
                tail = [r for r in rows if r[0] > HOLD - QUIET]
                if not any(r[4] == 'F' for r in tail):
                    why.append('no final readout in the last %g s (kinds %s)' % (QUIET, ''.join(sorted(set(r[4] for r in tail)))))
                print('     %d polls, %d range changes, last range %g, last FETC? %g, max FETC? %.2f s' % (
                    len(rows), len(steps), ranges[-1], rows[-1][1], max(r[2] for r in rows)))
            if how == 'leave':
                t2 = w0 + 8 + HOLD
                wait_until(t2)
                ok = None
                while up() < t2 + 6:
                    ro = q('DBG:READ?').split(',')
                    if ro[0] == 'F' and len(ro) > 2 and ro[1] == 'kΩ' and abs(num(ro[2]) - 4700) < 30:
                        ok = up() - t2
                        break
                    time.sleep(0.05)
                print('     after the swap to 4.7 kohm: %s' % ('final at +%.2f s' % ok if ok is not None else 'not final in 6 s (%s)' % ','.join(ro)))
                if ok is None or ok > 3.5:
                    why.append('4.7 kohm after the sink: %s' % ('%.2f s' % ok if ok is not None else 'never'))
        print('  %s %-11s %s %s%s' % ('ok ' if not why else 'BAD', name, rate, how, '' if not why else ': ' + '; '.join(why)), flush=True)
        if why:
            bad.append('%s: %s' % (name, '; '.join(why)))
finally:
    t = None
    emu.wait()  # run.py ends the emulator at --run-for
    for x in (script, os.path.join(OUT, 'flash-%s.bin' % tag), os.path.join(OUT, tag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
print('ohmsinktest: %s' % ('PASS' if not bad else 'FAIL ' + '; '.join(bad)))
sys.exit(1 if bad else 0)
