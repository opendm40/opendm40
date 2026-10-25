#!/usr/bin/env python3
"""RES AUTO set from a manual RES range (device 41a33a1, firmware/docs/device-check-2026-10-21.md
"41a33a1: the RES jack fix" note 2): `CONF:RES 6E6`, then `CONF:RES` + `RES:RANG:AUTO ON` left the
status bar on MANU and the readout on the manual range's final value (DBG:READout? `F`) while AUTO
stepped; from DC V the same commands showed AUTO and dashes.

Each window: input at its start, CONF:VOLT:DC at +3 s, the manual range (`CONF:RES <r>`) at +5 s, at
+9 s (fw uptime) the AUTO command, then `RES:RANG?;:DBG:READout?` every 0.05 s for HOLD s:
  - the status bar (DBG:READout? field 5) reads AUTO from the reply in the AUTO command's own
    message on; no final (kind F) readout with MANU after it;
  - a final AUTO readout within FINAL_MAX s, its value within 1 % of `want` (resistor rows).
The `dcv` row is the reference (from DC V, no manual range); `key` uses the AUTO key (DBG:KEY 2);
`sink` is ohmsinktest.py's input (6 Mohm ~0.10 Mohm, 600 kohm OL), `open` an open input (OL).
Before the fix (3f84f81) every row but `dcv` showed MANU and the manual final for 0.25-1.1 s.

  emu/hy3131/resautotest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 --port 64731
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
W = 22.0
HOLD = 8.0
SINK = 'isink:1.377e-7'  # ohmsinktest.py: 6 Mohm reads ~0.10 Mohm, 600 kohm OL
AUTO = 'CONF:RES;:RES:RANG:AUTO ON'

# name, input, manual range command (None: from DC V), AUTO command, want (ohm, None: any), final max s
SCEN = [
    ('r100k-6M', 'ohm:100000', 'CONF:RES 6E6', AUTO, 1e5, 3.0),
    ('r100k-600', 'ohm:100000', 'CONF:RES 600', AUTO, 1e5, 3.0),
    ('r100k-key', 'ohm:100000', 'CONF:RES 6E6', 'DBG:KEY 2', 1e5, 3.0),
    ('sink-6M', SINK, 'CONF:RES 6E6', AUTO, None, 6.0),
    ('sink-600', SINK, 'CONF:RES 600', AUTO, None, 6.0),
    ('sink-dcv', SINK, None, AUTO, None, 6.0),
    ('open-600', 'open', 'CONF:RES 600', AUTO, None, 6.0),
]

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--port', type=int, default=64731, help='USB/IP port')
ap.add_argument('-v', '--verbose', action='store_true', help='print every poll')
ap.add_argument('names', nargs='*', help='scenarios (default all)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

scen = [s for s in SCEN if not A.names or s[0] in A.names]
os.makedirs(OUT, exist_ok=True)
tag = 'resauto-%d-%d' % (A.port, os.getpid())
script = os.path.join(OUT, tag + '.script')
with open(script, 'w') as f:
    for k, sc in enumerate(scen):
        f.write('%.3f dmm %s\n' % (T0 + k * W + BOOT_OFS, 'open;' + sc[1]))
end = T0 + len(scen) * W + 2 + BOOT_OFS
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
        sys.exit('resautotest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60):
        t.write(line)
        return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000.0

    def wait_until(s):
        while up() < s:
            time.sleep(0.2)

    for k, (name, _, manual, auto, want, final_max) in enumerate(scen):
        w0 = T0 + k * W
        wait_until(w0 + 3)
        q('CONF:VOLT:DC;:SYST:ERR?')
        if manual:
            wait_until(w0 + 5)
            q(manual + ';:SYST:ERR?')
        wait_until(w0 + 9)
        why = []
        before = q('DBG:READ?')
        t1 = up()
        first = q(auto + ';:DBG:READ?').split(',')
        rows = [(0.0, float('nan'), first)]  # (s, range, readout fields)
        while True:
            rng, ro = q('RES:RANG?;:DBG:READ?').split(';', 1)
            s = up() - t1
            rows.append((s, num(rng), ro.split(',')))
            if s > HOLD:
                break
            time.sleep(0.05)
        err = q('SYST:ERR?')
        if A.verbose:
            print('       before %s' % before)
            for s, rng, ro in rows:
                print('       %6.2f range %-10g %s' % (s, rng, ','.join(ro)))
        if not err.startswith('0') and not err.startswith('+0'):
            why.append('SYST:ERR? %s' % err)
        lab = [r for r in rows if len(r[2]) < 5 or r[2][4] != 'AUTO']
        if lab:
            why.append('status bar not AUTO on %d of %d polls (first +%.2f s: %s)' % (len(lab), len(rows), lab[0][0], ','.join(lab[0][2])))
        stale = [r for r in rows if r[2][0] == 'F' and len(r[2]) > 4 and r[2][4] == 'MANU']
        if stale:
            why.append('%d final readouts with MANU after AUTO (last +%.2f s)' % (len(stale), stale[-1][0]))
        fin = [r for r in rows if r[2][0] == 'F' and len(r[2]) > 4 and r[2][4] == 'AUTO']
        if not fin:
            why.append('no final AUTO readout in %g s' % HOLD)
        else:
            s, _, ro = fin[0]
            v = num(ro[2])
            if s > final_max:
                why.append('first final AUTO readout at +%.2f s (max %g)' % (s, final_max))
            if want is not None and abs(v - want) > want * 0.01:
                why.append('first final AUTO readout %g (want %g)' % (v, want))
        print('  %s %-10s before %s; first final %s' % (
            'ok ' if not why else 'BAD', name, before, '+%.2f s %s' % (fin[0][0], ','.join(fin[0][2])) if fin else '-'), flush=True)
        if why:
            print('     ' + '; '.join(why))
            bad.append('%s: %s' % (name, '; '.join(why)))
finally:
    t = None
    emu.wait()  # run.py ends the emulator at --run-for
    for x in (script, os.path.join(OUT, 'flash-%s.bin' % tag), os.path.join(OUT, tag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
print('resautotest: %s' % ('PASS' if not bad else 'FAIL ' + '; '.join(bad)))
sys.exit(1 if bad else 0)
