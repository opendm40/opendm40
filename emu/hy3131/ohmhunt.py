#!/usr/bin/env python3
"""Ohm range hunt: no unvetted value as final (opendm40 dmm.rs poll_ohm,
device check ab7184c new bug 1, docs/history/switch-latency.md 6.2). DC V -> RES on
a resistor with the HY3131 model's ohm node lag (`ohmtau:ms`, opt-in: the
node relaxes to its value after every ohm row change, so the first
conversions after a range step read low, as the device's 219 / 57 ohm on
the right range). Every FETCh? that is not +9.91E37, every READ? after
CONF:RES and every amber (final, DBG:READout? kind F) readout value must be
within TOL of the final value; values off by more may only be grey
(kind G). Each window: input at its start, CONF:VOLT:DC at +3 s, the switch
at +8 s (fw uptime), then FETCh? (+ DBG:READout?) back to back for 5 s.

  emu/hy3131/ohmhunt.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 --port 58610
  emu/hy3131/ohmhunt.py --app ... --fetch-only   # a firmware without DBG:READout? (before the fix)
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
W = 16.0
TOL = 0.005  # 0.5 % of the final value
LAG = 'ohmtau:150'

# name, input, rate, how ('poll': CONF:RES then FETCh? polling; 'read': CONF:RES;:READ?),
# nominal ohm, max s to the first final value
SCEN = [
    ('r390-fast', 'ohm:390;' + LAG, 'FAST', 'poll', 390.0, 3.0),
    ('r390-med', 'ohm:390;' + LAG, 'MED', 'poll', 390.0, 3.5),
    ('r100-fast', 'ohm:100;' + LAG, 'FAST', 'poll', 100.0, 3.0),
    ('r100-med', 'ohm:100;' + LAG, 'MED', 'poll', 100.0, 3.5),
    ('r4k7-med', 'ohm:4700;' + LAG, 'MED', 'poll', 4700.0, 3.5),
    ('r10M-med', 'ohm:10e6;' + LAG, 'MED', 'poll', 10e6, 3.5),
    ('r390-read-fast', 'ohm:390;' + LAG, 'FAST', 'read', 390.0, 3.9),
    ('r390-read-med', 'ohm:390;' + LAG, 'MED', 'read', 390.0, 3.9),
    ('r4k7-nolag', 'ohm:4700;ohmtau:0', 'MED', 'poll', 4700.0, 2.5),  # (knobs persist: off again)
    # Noise above the vet's 0.1 % (60 Mohm range at FAST, x400 the model's AD1 noise, ~1-2 % rms): the
    # conversions rarely agree; the vet must still end (OHM_VET_MAX_MS), not stay grey, and show no
    # amber value off by more than the noise (7th: TOL). (Seeded noise: two still agree by chance
    # within ~1 s, so this does not fail without the escape.)
    ('r10M-fast-noisy', 'ohm:10e6;ohmtau:0;noise:400', 'FAST', 'poll', 10e6, 3.5, 0.08),
]

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--port', type=int, default=58610, help='USB/IP port')
ap.add_argument('--fetch-only', action='store_true', help='no DBG:READout? (firmware without it)')
ap.add_argument('names', nargs='*', help='scenarios (default all)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

scen = [s for s in SCEN if not A.names or s[0] in A.names]
os.makedirs(OUT, exist_ok=True)
tag = 'ohmhunt-%d-%d' % (A.port, os.getpid())
script = os.path.join(OUT, tag + '.script')
with open(script, 'w') as f:
    for k, sc in enumerate(scen):
        f.write('%.3f dmm %s\n' % (T0 + k * W + BOOT_OFS, sc[1]))
end = T0 + len(scen) * W + 2 + BOOT_OFS
cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01', '--usbip-port', str(A.port),
       '--run-for', '%.1f' % end, '--name', tag, '--script', script]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
bad = []
NAN = 9.91e37


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
        sys.exit('ohmhunt: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60):
        t.write(line)
        return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000.0

    def wait_until(s):
        while up() < s:
            time.sleep(0.2)

    for k, sc in enumerate(scen):
        name, _, rate, how, nom, tmax = sc[:6]
        tol = sc[6] if len(sc) > 6 else TOL
        w0 = T0 + k * W
        wait_until(w0 + 3)
        q('CONF:VOLT:DC;:RATE %s;:SYST:ERR?' % rate)
        wait_until(w0 + 8)
        why = []
        rows = []  # (s after the switch, FETC? value, readout kind, readout value)
        if how == 'read':
            t1 = up()
            r = q('CONF:RES;:READ?')
            dt = up() - t1
            err = q('SYST:ERR?')
            v = num(r)
            rows.append((dt, v, '', float('nan')))
            if not abs(v - nom) <= 0.01 * nom:
                why.append('READ? %s in %.2f s (%s)' % (r, dt, err))
            elif dt > tmax:
                why.append('READ? in %.2f s' % dt)
            wait_until(t1 + 5)
            final = num(q('FETC?'))
        else:
            t.write('CONF:RES')
            t1 = up()
            amber = False
            while True:
                # (the time after the reply: a FETCh? before the first reading waits for it.
                # With DBG:READout?: the readout alone until it turns amber, so the grey
                # hunt is sampled; then FETCh? with it)
                if A.fetch_only:
                    f, ro = q('FETC?'), 'N,,nan,0'
                elif not amber:
                    f, ro = 'nan', q('DBG:READ?')
                    amber = ro.startswith('F,')
                else:
                    f, ro = q('FETC?;:DBG:READ?').split(';', 1)
                s = up()
                ro = ro.split(',')
                rows.append((s - t1, num(f), ro[0], num(ro[2]) if len(ro) > 2 else float('nan')))
                if s - t1 > 5.0:
                    break
                time.sleep(0.01)
            final = rows[-1][1]
        if not abs(final - nom) <= max(0.01, tol) * nom:
            why.append('final %g (nominal %g)' % (final, nom))
        first = None
        for (s, f, kind, rv) in rows:
            if f < 9e37 and f == f:
                if first is None:
                    first = s
                if abs(f - final) > tol * abs(final):
                    why.append('FETC? %g at +%.2f s (final %g)' % (f, s, final))
            if kind == 'F' and abs(rv - final) > tol * abs(final):
                why.append('amber readout %g at +%.2f s (final %g)' % (rv, s, final))
        if how == 'poll' and (first is None or first > tmax):
            why.append('first final FETC? at %s s (max %g)' % (first, tmax))
        grey = sum(1 for r in rows if r[2] == 'G')
        print('  %s %-15s %s %s: first final %s s, final %g, %d grey samples%s' % (
            'ok ' if not why else 'BAD', name, rate, how, '-' if first is None else '%.2f' % first, final, grey,
            '' if not why else ': ' + '; '.join(why[:4])), flush=True)
        if why:
            bad.append('%s: %s' % (name, '; '.join(why[:3])))
finally:
    t = None
    emu.wait()  # run.py ends the emulator at --run-for
    for x in (script, os.path.join(OUT, 'flash-%s.bin' % tag), os.path.join(OUT, tag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
print('ohmhunt: %s' % ('PASS' if not bad else 'FAIL ' + '; '.join(bad)))
sys.exit(1 if bad else 0)
