#!/usr/bin/env python3
"""Capacitance AUTO steps down from the ramp ranges (dmm.rs poll_cap, issue #74): a small
capacitor after a large one ends on its own range with its value. Before the fix the 600 uF
range (r5, the lowest ramp range) had no step down: 100 nF after 1 mF stayed on 600 uF and
read 3 .. 44 uF as final values.

Each window (fw uptime, from the window input's marker: emu/emusync.py): the large capacitor
at its start, CONF:CAP at +1 s, the small one at +SWAP s; from the swap `CONF?;:FETC?` every
0.2 s for HOLD s. PASS when the last polls show the small capacitor's range and a value
within TOL of it.

  emu/hy3131/capdowntest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 --port 47750
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
sys.path.insert(0, EMU)
import emusync  # noqa: E402

BOOT_OFS = emusync.BOOT_S
T0 = 8.0
SWAP = 14.0
HOLD = 20.0
W = SWAP + HOLD + 3.0
TOL = 0.03
# name, large, small, the small one's range (F), its value (F)
SCEN = [
    ('1mF-100nF', 'c:1000e-6', 'c:100e-9', 6e-7, 100e-9),
    ('470uF-22uF', 'c:470e-6', 'c:22e-6', 6e-5, 22e-6),
    ('4.7mF-330uF', 'c:4.7e-3', 'c:330e-6', 6e-4, 330e-6),
]

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--port', type=int, default=47750, help='USB/IP port')
ap.add_argument('-v', '--verbose', action='store_true', help='print every poll')
ap.add_argument('names', nargs='*', help='scenarios (default all)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

scen = [s for s in SCEN if not A.names or s[0] in A.names]
os.makedirs(OUT, exist_ok=True)
tag = 'capdown-%d-%d' % (A.port, os.getpid())
script = os.path.join(OUT, tag + '.script')
starts = [T0 + W * k for k in range(len(scen))]
with open(script, 'w') as f:
    for k, sc in enumerate(scen):
        f.write('%.3f dmm open;%s\n' % (starts[k] + BOOT_OFS, sc[1]))
        f.write('%.3f dmm open;%s\n' % (starts[k] + SWAP + BOOT_OFS, sc[2]))
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
        sys.exit('capdowntest: no USB/IP device on port %d' % A.port)
    mk = emusync.Markers(emusync.log_path(OUT, tag))
    if mk.wait_clock() is None:
        sys.exit('capdowntest: no clock marker (dwt: CYCCNT write) in %s' % mk.log)

    def q(line, timeout=60):
        t.write(line)
        return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000.0

    def wait_until(s):
        while up() < s:
            time.sleep(0.2)

    for k, (name, big, small, rng, want) in enumerate(scen):
        w0 = mk.at(starts[k] + BOOT_OFS)
        wait_until(w0 + 1)
        q('CONF:CAP;*OPC?')
        ws = mk.at(starts[k] + SWAP + BOOT_OFS)
        wait_until(ws)
        rows = []
        while True:
            conf, v = q('CONF?;:FETC?').split(';', 1)
            s = up() - ws
            r = num(conf.strip('"').split()[1].split(',')[0]) if ' ' in conf else float('nan')
            rows.append((s, r, num(v)))
            if A.verbose:
                print('       %6.2f range %-10g %s' % (s, r, v))
            if s > HOLD:
                break
            time.sleep(0.2)
        last = rows[-5:]
        why = []
        if any(abs(r - rng) > rng * 1e-3 for _, r, _ in last):
            why.append('range %s at the end (want %g)' % (' '.join('%g' % r for _, r, _ in last), rng))
        if any(not abs(v - want) <= want * TOL for _, _, v in last):
            why.append('values %s at the end (want %g +- %g %%)' % (' '.join('%g' % v for _, _, v in last), want, 100 * TOL))
        done = [s for s, r, _ in rows if abs(r - rng) <= rng * 1e-3]
        print('%s %s: %s -> %s, on its range from +%s s%s' % (
            'ok  ' if not why else 'FAIL', name, big, small, '%.1f' % done[0] if done else '-',
            '' if not why else ': ' + '; '.join(why)), flush=True)
        if why:
            bad.append(name)
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    emu.wait()
print('FAIL: %s' % ' '.join(bad) if bad else 'PASS')
sys.exit(1 if bad else 0)
