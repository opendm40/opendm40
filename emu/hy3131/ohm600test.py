#!/usr/bin/env python3
"""Manual 600 ohm range: steps into 500 ohm and the over-range boundary, at each rate (TODO "500 ohm
from the MMB3.0 calibrator reads OL on 600 ohm"; device record firmware/docs/data/res-steps-2026-10-25.csv).

On the device a 200 -> 500 ohm step of the MMB3.0 calibrator (an active resistance simulator,
inferred) read OL at every conversion (R22 0x13, 25 ms) for 32 s on manual 600 ohm, until a range
change. This test checks the firmware side with passive resistors: the OL decision on manual
600 ohm is taken afresh from every conversion, so an over-range moment (an open input during the
calibrator's switch, a value above the range) ends at the first conversion that reads in range.

Each window: `CONF:RES 600;:SENS:RATE <rate>` at +1 s (fw uptime), the input `a` from the window
start, the input `b` from `at` s after it (emulator script, virtual time), `RES:RANG?;:DBG:READout?`
every 0.05 s from +3 s to GUARD s before the window end. Checks:
  - manual: the range is 600 ohm on every poll (nothing moves it); AUTO: on the last LAST polls;
  - the last LAST polls are final (F) readouts of `want` ohm within 0.2 % (`tol`), or OL if `want` is None;
  - `rec`: a final in-range readout of `b` within `rec` s of the first poll that shows `b` (or OL)
    after the step, i.e. no OL hold beyond the conversions the step itself spans.
Rows: 200 -> open (0.3 s) -> 500 ohm (the calibrator's switch), 700 (OL) -> 500, open -> 500 at SLOW /
MED / FAST; 590 / 610 ohm in range, 620 ohm OL (the limit 618 ohm = 600 x 1.03, dmm.rs OVER) at each
rate; AUTO 200 -> open -> 500 and open -> 500 ohm land on 600 ohm, also with the node lag `ohmtau:1000`.

  emu/hy3131/ohm600test.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 --port 64210
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
W = 18.0
GUARD = 4.0  # s: polling ends this long before the next window (the script runs ahead of the fw uptime, ~1.6 s 3 min into a run)
LAST = 5
STEP = 6.0  # s after the window start: the step to `b`

# name, rate, a, [(s after STEP, input)], want (ohm; None = OL), rec (s; None = no check)[, opts]
# opts: auto = the AUTO command instead of the manual range; range = the range of the last polls
# (default 600, and 600 on every poll in manual); tol = the last polls' tolerance (default 0.002;
# 0.01 in the node-lag row, whose tail is still in MED's 6.4 s average).
AUTO = 'CONF:RES;:RES:RANG:AUTO ON'
SCEN = []
for rate in ('SLOW', 'MED', 'FAST'):
    SCEN += [
        ('200-open-500-' + rate, rate, 'ohm:200', [(0.0, 'open'), (0.3, 'ohm:500')], 500.0, 3.0),
        ('700-500-' + rate, rate, 'ohm:700', [(0.0, 'ohm:500')], 500.0, 3.0),
        ('open-500-' + rate, rate, 'open', [(0.0, 'ohm:500')], 500.0, 3.0),
    ]
for rate in ('SLOW', 'MED', 'FAST'):
    SCEN += [
        ('590-' + rate, rate, 'ohm:590', [], 590.0, None),
        ('610-' + rate, rate, 'ohm:610', [], 610.0, None),
        ('620-' + rate, rate, 'ohm:620', [], None, None),
    ]
SCEN += [
    # AUTO: 500 ohm lands on 600 ohm (the comparison for the device's AUTO run).
    ('auto-200-open-500-MED', 'MED', 'ohm:200', [(0.0, 'open'), (0.3, 'ohm:500')], 500.0, None, {'auto': True}),
    ('auto-open-500-MED', 'MED', 'open', [(0.0, 'ohm:500')], 500.0, None, {'auto': True}),
    # The emulator's model of a slow node / a simulator catching up after every ohm row change
    # (`ohmtau:ms`, emu/hy3131/README.md), last: the knob stays on for the rest of the run.
    ('auto-open-500-tau1000-MED', 'MED', 'open;ohmtau:1000', [(0.0, 'ohm:500;ohmtau:1000')], 500.0, None, {'auto': True, 'tol': 0.01}),
]

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--port', type=int, default=64210, help='USB/IP port')
ap.add_argument('-v', '--verbose', action='store_true', help='print every poll')
ap.add_argument('names', nargs='*', help='scenarios (default all)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

scen = [s for s in SCEN if not A.names or s[0] in A.names]
os.makedirs(OUT, exist_ok=True)
tag = 'ohm600-%d-%d' % (A.port, os.getpid())
script = os.path.join(OUT, tag + '.script')
starts = [T0 + k * W for k in range(len(scen))]
with open(script, 'w') as f:
    for k, sc in enumerate(scen):
        f.write('%.3f dmm %s\n' % (starts[k] + BOOT_OFS, 'open;' + sc[2]))
        for s, inp in sc[3]:
            f.write('%.3f dmm %s\n' % (starts[k] + STEP + s + BOOT_OFS, 'open;' + inp if inp != 'open' else 'open'))
end = starts[-1] + W + 2 + BOOT_OFS
cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01', '--usbip-port', str(A.port),
       '--run-for', '%.1f' % end, '--name', tag, '--script', script]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
bad = []


def num(r):
    try:
        return float(r)
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
        sys.exit('ohm600test: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60):
        t.write(line)
        return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000.0

    def wait_until(s):
        while up() < s:
            time.sleep(0.05)

    for k, sc in enumerate(scen):
        name, rate, _, steps, want, rec = sc[:6]
        o = sc[6] if len(sc) > 6 else {}
        w0 = starts[k]
        wait_until(w0 + 1)
        q('%s;:SENS:RATE %s;:SYST:ERR?' % (AUTO if o.get('auto') else 'CONF:RES 600', rate))
        wait_until(w0 + 3)
        rows = []  # (uptime s, range, readout fields)
        while True:
            rng, ro = q('RES:RANG?;:DBG:READ?').split(';', 1)
            s = up()
            rows.append((s, num(rng), ro.split(',')))
            if s > w0 + W - GUARD:
                break
            time.sleep(0.05)
        err = q('SYST:ERR?')
        if A.verbose:
            for s, rng, ro in rows:
                print('       %7.2f range %-6g %s' % (s - w0, rng, ','.join(ro)))
        why = []
        if not err.startswith('0') and not err.startswith('+0'):
            why.append('SYST:ERR? %s' % err)
        off = [r for r in (rows[-LAST:] if o.get('auto') else rows) if r[1] != o.get('range', 600)]
        if off:
            why.append('range %g at +%.2f s' % (off[0][1], off[0][0] - w0))

        def ok(r, want):
            if r[2][0] != 'F':
                return False
            v = num(r[2][2])
            return v > 1e37 if want is None else abs(v - want) <= want * o.get('tol', 0.002)

        tail = rows[-LAST:]
        if not all(ok(r, want) for r in tail):
            why.append('last polls %s (want %s)' % (' '.join('%s:%s' % (r[2][0], r[2][2]) for r in tail),
                                                    'OL' if want is None else '%g' % want))
        if rec is not None:
            # The step: the first poll after which nothing reads the old input `a` any more.
            fin = [r for r in rows if r[2][0] == 'F']
            a_val = num(sc[2].split(':')[1]) if ':' in sc[2] else None

            def is_a(r):
                v = num(r[2][2])
                return (v > 1e37 and (a_val is None or a_val > 618)) or (a_val is not None and abs(v - a_val) <= a_val * 0.01)
            last_a = max((i for i, r in enumerate(fin) if is_a(r)), default=-1)
            after = fin[last_a + 1:]
            got = next((r for r in after if ok(r, want)), None)
            if after and got and got[0] - after[0][0] > rec:
                why.append('in range %.2f s after the step (max %g)' % (got[0] - after[0][0], rec))
            ol_after = [r for r in after if num(r[2][2]) > 1e37 and got and r[0] > got[0]]
            if ol_after:
                why.append('OL at +%.2f s after the in-range readout' % (ol_after[0][0] - w0))
        print('  %s %-18s %d polls, last %s' % ('ok ' if not why else 'BAD', name, len(rows), ','.join(rows[-1][2])), flush=True)
        if why:
            print('     ' + '; '.join(why))
            bad.append('%s: %s' % (name, '; '.join(why)))
finally:
    t = None
    emu.wait()  # run.py ends the emulator at --run-for
    for x in (script, os.path.join(OUT, 'flash-%s.bin' % tag), os.path.join(OUT, tag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
print('ohm600test: %s' % ('PASS' if not bad else 'FAIL ' + '; '.join(bad)))
sys.exit(1 if bad else 0)
