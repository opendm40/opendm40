#!/usr/bin/env python3
"""FR-05 rates in DC V (dmm.rs FR-05 table, fastvos.rs): FAST = AD1 OSR
16384 unchopped (R22 0x06, 100 ms) with the converter offset taken out in
firmware, MED = 0x16 (200 ms), SLOW = 0x17 (400 ms). The model's opt-in
knobs (emu/hy3131/HY3131.cs `UnchopTerm`): ad1vos (VOS in codes by
AD1CHOP, here 15 000, not fastvos VOS_DEFAULT 13 270, so the readings
before the first refresh are ~2.9 mV off and the refresh has to measure
it), ad1ug (the unchopped gain, +95 ppm), the polarity switch inside the
running window (ad1chopjit, default +-5 ms).

Checks, DC V 6 V manual, 1 V on V-COM:
  * MED: a reading every ~200 ms, the reference value (mean);
  * SLOW: every ~400 ms, the same value;
  * FAST: every ~100 ms; from 4 s after the rate change on (the first
    refresh starts 2 s after it and takes 14 conversions), every reading
    within 3 counts (0.3 mV: ~5 sd of the model's noise, ~35 codes per
    unchopped conversion) and their mean within 0.05 mV of the reference;
    DBG:FAST? counts that refresh and the later ones, none rejected, VOS =
    the model's +-40 codes (~4 sd of one refresh); at most 2 conversions lost per
    refresh;
  * FAST again after MED (the path's VOS is known): every reading from the
    third one on within 3 counts;
  * AUTO with 10 V on the input: the 60 V range (its path not measured
    yet: the last VOS), every reading within 3 counts (3 mV; the model's
    noise there is ~0.6 count sd) and the mean within 0.5 count of 10 V x
    the reference's scale.

  emu/hy3131/fastratetest.py --app firmware/opendm40.bin --port 47740
"""
import argparse
import os
import signal
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
OUT = os.path.join(EMU, 'out')

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native image')
ap.add_argument('--port', type=int, default=47740, help='USB/IP port')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402
import emusync  # noqa: E402

MODEL = 'ad1vos:15000;ad1ug:95'
T_10V = 120.0  # model time of the 10 V step (after the 1 V part)
os.makedirs(OUT, exist_ok=True)
tag = 'fastrate-%d-%d' % (A.port, os.getpid())
script = os.path.join(OUT, tag + '.script')
with open(script, 'w') as f:
    f.write('0 dmm dcv:1.0;%s\n' % MODEL)
    f.write('%.1f dmm dcv:10.0;%s\n' % (T_10V, MODEL))
end = T_10V + 25
cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01', '--usbip-port', str(A.port),
       '--run-for', '%.1f' % end, '--name', tag, '--script', script]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
mk = emusync.Markers(emusync.log_path(OUT, tag))
bad = []
try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('fastratetest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60):
        t.write(line)
        return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000.0

    def wait_until(s):
        while up() < s:
            time.sleep(0.05)

    if mk.wait_clock() is None:
        sys.exit('fastratetest: no clock marker (dwt: CYCCNT write) in %s' % mk.log)

    def readings(dur):
        """New final readings (s from the start, value) for `dur` s of device time."""
        t0 = up()
        seen = []
        last = None
        while True:
            now = up()
            if now >= t0 + dur:
                return seen
            ro = q('DBG:READout?').split(',')
            if ro[0] == 'F' and len(ro) > 2 and ro[1:3] != last:
                last = ro[1:3]
                try:
                    seen.append((now - t0, float(ro[2])))
                except ValueError:
                    pass

    def med_dt(rs):
        d = sorted(b[0] - a[0] for a, b in zip(rs, rs[1:]))
        return d[len(d) // 2] if d else 0.0

    def check(name, cond, msg):
        print('  %s %-14s %s' % ('ok ' if cond else 'BAD', name, msg), flush=True)
        if not cond:
            bad.append('%s: %s' % (name, msg))

    wait_until(mk.at(3.0))
    for c in ['CONF:VOLT:DC 6', 'SENS:RATE MED']:
        t.write(c)
    wait_until(up() + 3)
    rs = readings(8)
    vals = [v for s, v in rs if s > 1.0]
    ref = sum(vals) / len(vals)
    check('med', abs(med_dt(rs) - 0.2) < 0.04 and abs(ref - 1.0) < 0.002, 'interval %.3f s, mean %.5f V (%d readings)' % (med_dt(rs), ref, len(rs)))
    t.write('SENS:RATE SLOW')
    rs = readings(8)
    vals = [v for s, v in rs if s > 2.0]
    m = sum(vals) / len(vals)
    check('slow', abs(med_dt(rs) - 0.4) < 0.06 and abs(m - ref) < 0.0002, 'interval %.3f s, mean %.5f V (ref %.5f)' % (med_dt(rs), m, ref))
    t.write('SENS:RATE FAST')
    rs = readings(40)
    early = [v for s, v in rs if s < 1.8]
    late = [(s, v) for s, v in rs if s > 4.0]  # the first refresh: 2 s + 1.4 s (14 conversions)
    off = [x for x in late if abs(x[1] - ref) > 0.0003]
    lm = sum(v for _, v in late) / max(1, len(late))
    fi = q('DBG:FAST?').split(',')
    used, rej, vos = int(fi[0]), int(fi[1]), int(fi[2])
    span = rs[-1][0] - rs[0][0]
    lost = round(span / 0.1) + 1 - len(rs)
    check('fast-rate', abs(med_dt(rs) - 0.1) < 0.02, 'interval %.3f s, %d readings in %.1f s (%d not shown)' % (med_dt(rs), len(rs), span, lost))
    check('fast-default', bool(early) and abs(sum(early) / len(early) - ref) > 0.002,
          'before the first refresh (default VOS): mean %.5f V vs ref %.5f' % (sum(early) / max(1, len(early)), ref))
    check('fast-value', not off and abs(lm - ref) < 0.00005,
          'mean %.6f V (ref %.6f); %d of %d readings off by > 0.3 mV%s' % (lm, ref, len(off), len(late), (' e.g. %s' % off[:3]) if off else ''))
    check('fast-refresh', used >= 2 and rej == 0 and abs(vos - 15000) <= 40, 'DBG:FAST? %s' % ','.join(fi))
    check('fast-lost', lost <= 2 * used + 2, '%d conversions not shown for %d refreshes' % (lost, used))
    t.write('SENS:RATE MED')
    wait_until(up() + 3)
    t.write('SENS:RATE FAST')
    rs = readings(5)
    off = [x for x in rs[2:] if abs(x[1] - ref) > 0.0003]
    check('fast-again', len(rs) > 30 and not off, '%d readings, %d off%s' % (len(rs), len(off), (' e.g. %s' % off[:3]) if off else ''))
    t.write('VOLT:RANG:AUTO ON')
    wait_until(mk.at(T_10V) + 4)
    conf = q('CONF?')
    rs = readings(6)
    want = 10.0 * ref
    off = [x for x in rs if abs(x[1] - want) > 0.003]
    m = sum(v for _, v in rs) / max(1, len(rs))
    check('fast-auto-60v', '+6.000000E+01' in conf and len(rs) > 40 and not off and abs(m - want) < 0.0005,
          'CONF? %s, %d readings, mean %.5f, %d off from %.4f by > 3 counts%s' % (conf, len(rs), m, len(off), want, (' e.g. %s' % off[:3]) if off else ''))
    err = q('SYST:ERR?')
    check('errors', err.split(',')[0].strip() in ('0', '+0'), err)
finally:
    t = None
    if emu.poll() is None:
        os.killpg(emu.pid, signal.SIGTERM)
    emu.wait()
    for x in (script, os.path.join(OUT, 'flash-%s.bin' % tag), os.path.join(OUT, tag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
print('fastratetest: %s' % ('PASS' if not bad else 'FAIL ' + '; '.join(bad)))
sys.exit(1 if bad else 0)
