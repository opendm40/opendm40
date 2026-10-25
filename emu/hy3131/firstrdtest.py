#!/usr/bin/env python3
"""The first readings after a DMM function / range change (device check of
v0.1-pre1, 2026-10-24, firmware/docs/device-check-2026-10-21.md): no final
reading (DBG:READout? kind F, FETCh?) may come from a conversion the
switch spoiled, and a manual mA range must stay. The model's opt-in knobs
replay what the device showed (emu/hy3131/HY3131.cs, INFERRED shapes):

  r22part / r22off / r22n / r22win   an R22 (AD1 OSR) change (r22win: within that
                     many ms of the row write) ends the conversion in progress
                     as one partial result (frac x input + an offset of
                     alternating sign): DC V at MED read ~40 % once after
                     the 0x15 -> 0x17 step; the DC mA entry from DC V 6 V at
                     MED / SLOW read +-44..56 mA / over range and OVERLOAD
                     moved CONF:CURR:DC 0.06 to the 10 A range
  acastep + acasteptau  AC A 10 A -> 60 mA: the RMS block's switch step and
                     its tail (first 60 mA results 29-60 mA on an open jack)

Scenarios (each: REPS switches, the gap before each one a little longer so
the switch meets the old conversion at other phases):

  dcma-med     MED, CONF:VOLT:AC, CONF:VOLT:DC 6, CONF:CURR:DC 0.06, open
               jack: every F below 1 mA, CONF? stays CURR 0.06
  dcma-slow    the same at SLOW
  dcv-acv-med  MED, 1 V: CONF:VOLT:AC -> CONF:VOLT:DC 6: every F 1 V +-0.1 %
  dcv-res-slow SLOW, 1 V: CONF:RES -> CONF:VOLT:DC 6: the same
  res-med      MED, 390 ohm (r22n 2 at 0.32): CONF:VOLT:DC -> CONF:RES: every F
               within 0.1 % of the reading at +6 s
  fuse-open    MED, open jacks (r22n 2, r22off ~280 ohm): SYST:FUSE:TEST from DC V ->
               SYST:FUSE? OPEN
  aca-step     FAST, open jack, 1 V on V: CONF:CURR:AC 10, 2 s, CONF:CURR:AC
               0.06: every F below 0.1 mA, CONF? stays CURR:AC 0.06
  acv-auto-dn  MED, 1 V AC: CONF:VOLT:AC 60, VOLT:AC:RANG:AUTO ON (-> 6 V): every
               F 1 V +-0.5 % (7ab5c0e: a 0 V F from the stepping result, then
               0.99 V from the EMA's step from 0)
  acv-auto-up  the same up: 8 V AC on CONF:VOLT:AC 6 -> 60 V
  aca-auto-dn  0.8 A AC: CONF:CURR:AC 10, CURR:AC:RANG:AUTO ON (-> 6 A)
  acv-entry    1 V AC: CONF:VOLT:DC 6 -> CONF:VOLT:AC: every F 1 V +-0.5 %
  acmv-acv-zero  0 V: CONF:VOLT:AC 0.6 -> CONF:VOLT:AC 6: every F below 0.05 V
               (device 185c066: 0.018-0.036 V first)
  acmv-acv-sq  the 0 / 9.5 V square, mvclamp: CONF:VOLT:AC 0.6 -> CONF:VOLT:AC (AUTO):
               CONF? never on 60 V (device 185c066: 6 V only)
  acv-entry-sq the same on the device's 0 / 9.5 V 50 Hz square (AC part 4.776 V) with
               the model's acarrive 160 ms (the last RMS block before the 300 ms
               settle half over the AC's arrival): every F 4.776 V +-0.5 % (5a4dbce:
               the block latched during the settle seeded the filter, 0.707 x; device
               185c066 3.39-3.41 V for 4.81 V)
  acv-entry-arr  the same on 1 V 50 Hz (sine)

dcv-acv-med, dcv-res-slow and res-med also check the time between two new
readings after the first one (GAP: at most 0.6 s; 25b6ec3 dropped two 400 ms
conversions after the early OSR step, 0.85-0.9 s). The model's r22win 1000
gives partials only for an R22 change within 1 s of the row write (INFERRED:
the device showed none for the same OSR steps later in a function), so these
scenarios show that the firmware makes no slow-OSR step that early, not where
the device's boundary lies.

  emu/hy3131/firstrdtest.py --app firmware/opendm40.bin --port 47700 [names]
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
BOOT_OFS = 0.906
T0 = 8.0
REPS = 4
W = 44.0
OFF = 'r22part:-1;r22off:0;r22n:1;r22win:0;acastep:0;acasteptau:0;acarrive:0'
R22 = 'r22part:0.4;r22off:2400000;r22n:1;r22win:1000'

# name, input, rate, (pre command, s)..., switch command, check (kind, limit), conf prefix that must stay
# ('!prefix': a CONF? prefix that must never show)
SCEN = [
    ('dcma-med', 'open;vsrc:1;' + R22, 'MED', [('CONF:VOLT:AC', 1.5), ('CONF:VOLT:DC 6', 3.0)], 'CONF:CURR:DC 0.06',
     ('abs', 1e-3), 'CURR +6.0'),
    ('dcma-slow', 'open;vsrc:1;' + R22, 'SLOW', [('CONF:VOLT:AC', 1.5), ('CONF:VOLT:DC 6', 3.0)], 'CONF:CURR:DC 0.06',
     ('abs', 1e-3), 'CURR +6.0'),
    ('dcv-acv-med', 'dcv:1.0;r22part:0.4;r22off:0;r22n:2;r22win:1000', 'MED', [('CONF:VOLT:AC', 2.0)], 'CONF:VOLT:DC 6',
     ('rel', 1.0, 0.001), 'VOLT +6.0'),
    ('dcv-res-slow', 'dcv:1.0;r22part:0.4;r22off:0;r22n:2;r22win:1000', 'SLOW', [('CONF:RES', 2.0)], 'CONF:VOLT:DC 6',
     ('rel', 1.0, 0.001), 'VOLT +6.0'),
    ('res-med', 'ohm:390;r22part:0.32;r22off:0;r22n:2;r22win:1000', 'MED', [('CONF:VOLT:DC', 2.0)], 'CONF:RES',
     ('final', 0.001), 'RES'),
    ('fuse-open', 'open;r22part:0;r22off:2100000;r22n:2;r22win:1000', 'MED', [('CONF:VOLT:DC', 2.0)], 'SYST:FUSE:TEST',
     ('fuse', 'OPEN'), ''),
    ('aca-step', 'open;vsrc:1;acastep:200000;acastepms:600;acasteptau:25', 'FAST', [('CONF:CURR:AC 10', 2.0)],
     'CONF:CURR:AC 0.06', ('abs', 1e-4), 'CURR:AC +6.0'),
    # Plain AC: an autorange step (no input change) and the entry. The result that steps the
    # range must not publish the filter the step reset (0) as a final reading, and the first
    # result after the reset seeds the filter (the EMA's 0.99 step from 0 read 1 % low).
    ('acv-auto-dn', 'open;vsrc:0,ac=1,f=50', 'MED', [('CONF:VOLT:AC 60', 3.0)], 'VOLT:AC:RANG:AUTO ON',
     ('rel', 1.0, 0.005), 'VOLT:AC +6.0'),
    ('acv-auto-up', 'open;vsrc:0,ac=8,f=50', 'MED', [('CONF:VOLT:AC 6', 3.0)], 'VOLT:AC:RANG:AUTO ON',
     ('rel', 8.0, 0.005), 'VOLT:AC +6.0'),
    ('aca-auto-dn', 'open;isrc:0,ac=0.8,f=50', 'MED', [('CONF:CURR:AC 10', 3.0)], 'CURR:AC:RANG:AUTO ON',
     ('rel', 0.8, 0.005), 'CURR:AC +'),
    ('acv-entry', 'open;vsrc:0,ac=1,f=50', 'MED', [('CONF:VOLT:DC 6', 3.0)], 'CONF:VOLT:AC',
     ('rel', 1.0, 0.005), 'VOLT:AC'),
    # AC mV -> AC V (device 185c066 step 3): at 0 V the first finals read 0.018-0.036 V, then
    # 3 mV at 1 s; under the 0 / 9.5 V square (608 mV clipped on 600 mV) AUTO goes to 6 V
    # only. The model once read 2.78-3.9 V at 0 V and visited 60 V (its coupling capacitor's
    # level while OP1 is off, HY3131.cs Op1OffLevel).
    ('acmv-acv-zero', 'open;vsrc:0', 'MED', [('CONF:VOLT:AC 0.6', 3.0)], 'CONF:VOLT:AC 6',
     ('abs', 0.05), 'VOLT:AC +6.000000E+00'),
    ('acmv-acv-sq', 'open;vsrc:4.75,ac=4.75,f=40,shape=square;mvclamp:1.212', 'MED', [('CONF:VOLT:AC 0.6', 3.0)],
     'CONF:VOLT:AC', ('any',), '!VOLT:AC +6.000000E+01'),
    # Plain AC entry: the RMS block the chip latched during the settle (its window partly before
    # the AC reached the node, the model's acarrive) must not be a reading (device 185c066).
    ('acv-entry-sq', 'open;acarrive:160;vsrc:4.776,ac=4.776,f=50,shape=square', 'MED', [('CONF:VOLT:DC 6', 3.0)],
     'CONF:VOLT:AC', ('rel', 4.776, 0.005), 'VOLT:AC'),
    ('acv-entry-arr', 'open;acarrive:160;vsrc:0,ac=1,f=50', 'MED', [('CONF:VOLT:DC 6', 3.0)], 'CONF:VOLT:AC',
     ('rel', 1.0, 0.005), 'VOLT:AC'),
]

# Longest wait between two new readings (grey or final) after the first one,
# s (R22_LATE: no 400 ms conversions dropped after an early R22 change at
# MED / SLOW; 25b6ec3: 1.2 s on DC V, 0.85 s on RES).
# Counted for waits that begin within GAP_UNTIL s of the switch: the step to
# the slow OSR ~2 s after the row write restarts the converter (one 400 ms
# conversion), by design.
GAP = {'dcv-acv-med': 0.6, 'dcv-res-slow': 0.6, 'res-med': 0.6}
GAP_UNTIL = 1.8

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native image')
ap.add_argument('--port', type=int, default=47700, help='USB/IP port')
ap.add_argument('names', nargs='*', help='scenarios (default all)')
A = ap.parse_args()
SCEN = [sc for sc in SCEN if not A.names or sc[0] in A.names]
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402
import emusync  # noqa: E402

os.makedirs(OUT, exist_ok=True)
tag = 'firstrd-%d-%d' % (A.port, os.getpid())
script = os.path.join(OUT, tag + '.script')
with open(script, 'w') as f:
    for k, sc in enumerate(SCEN):
        f.write('%.3f dmm %s;%s\n' % (T0 + k * W + BOOT_OFS, OFF, sc[1]))
end = T0 + len(SCEN) * W + 2 + BOOT_OFS
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
        sys.exit('firstrdtest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60):
        t.write(line)
        return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000.0

    def wait_until(s):
        while up() < s:
            time.sleep(0.05)

    if mk.wait_clock() is None:
        sys.exit('firstrdtest: no clock marker (dwt: CYCCNT write) in %s' % mk.log)

    def num(r):
        try:
            return float(r.split(',')[0])
        except ValueError:
            return float('nan')

    def wrong(v, chk):
        if abs(v) >= 9e37 or v != v:
            return False  # +9.91E37 / dashes: no reading, not a wrong one
        if chk[0] in ('final', 'fuse', 'any'):
            return False  # checked at the end against the settled reading
        if chk[0] == 'abs':
            return abs(v) > chk[1]
        return abs(v - chk[1]) > chk[2] * chk[1]

    for k, (name, _, rate, pre, sw, chk, conf) in enumerate(SCEN):
        w0 = mk.at(T0 + k * W + BOOT_OFS)
        wait_until(w0 + 0.5)
        q('SENS:RATE %s;:SYST:ERR?' % rate)
        why = []
        firsts = []
        gaps = []
        for rep in range(REPS):
            for c, d in pre:
                t.write(c)
                wait_until(up() + d + 0.13 * rep * (c == pre[-1][0]))
            t.write(sw)
            t0 = up()
            if chk[0] == 'fuse':
                while True:
                    r = q('SYST:FUSE?')
                    if r not in ('BUSY', 'IDLE') or up() > t0 + 12:
                        break
                if r != chk[1]:
                    why.append('rep %d: SYST:FUSE? %s at +%.2f s' % (rep, r, up() - t0))
                firsts.append(r)
                wait_until(up() + 1.5)
                continue
            first = None
            fs = []
            seen = []  # (s, kind, value) at each new reading (G or F)
            dur = 6.0 if chk[0] == 'final' else 3.0
            while up() < t0 + dur:
                ro = q('DBG:READout?').split(',')
                cf = q('CONF?')
                if ro[0] in ('F', 'G') and len(ro) > 2 and (not seen or seen[-1][1:] != (ro[0], ro[2])):
                    seen.append((up() - t0, ro[0], ro[2]))
                if ro[0] == 'F':
                    v = num(ro[2])
                    fs.append((round(up() - t0, 2), v))
                    if first is None:
                        first = (round(up() - t0, 2), ro[2])
                    if wrong(v, chk):
                        why.append('rep %d: F %s at +%.2f s' % (rep, ro[2], up() - t0))
                if (cf.strip('"').startswith(conf[1:]) if conf.startswith('!') else not cf.strip('"').startswith(conf)):
                    why.append('rep %d: CONF? %s at +%.2f s' % (rep, cf, up() - t0))
                    break
            gap = max([b[0] - a[0] for a, b in zip(seen, seen[1:]) if a[0] < GAP_UNTIL] or [0.0])
            gaps.append(round(gap, 2))
            if name in GAP and gap > GAP[name]:
                why.append('rep %d: %.2f s without a new reading after the first one (limit %.2f)' % (rep, gap, GAP[name]))
            f = q('FETC?')
            if chk[0] == 'final':
                fin = num(f)
                why += ['rep %d: F %g at +%.2f s (settled %g)' % (rep, v, tt, fin) for tt, v in fs
                        if abs(v - fin) > chk[1] * abs(fin)][:2]
            elif wrong(num(f), chk):
                why.append('rep %d: FETC? %s at +3 s' % (rep, f))
            firsts.append(first)
        err = q('SYST:ERR?')
        if err.split(',')[0].strip() not in ('0', '+0'):
            why.append('SYST:ERR? %s' % err)
        print('  %s %-12s first F (s, value) %s, gaps %s%s' % ('ok ' if not why else 'BAD', name, firsts, gaps,
                                                     '' if not why else '  ' + '; '.join(why[:4])), flush=True)
        if why:
            bad.append('%s: %s' % (name, '; '.join(why[:4])))
finally:
    t = None
    emu.wait()
    for x in (script, os.path.join(OUT, 'flash-%s.bin' % tag), os.path.join(OUT, tag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
print('firstrdtest: %s' % ('PASS' if not bad else 'FAIL ' + '; '.join(bad)))
sys.exit(1 if bad else 0)
