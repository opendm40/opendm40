#!/usr/bin/env python3
"""opendm40 scope: the CH1 popup's open / close redraw and the tap beep
(emulator time; owner report 2026-10-26 "the longer the beep, the longer the
redraw").

  cd firmware && ./build.sh
  emu/popupbeep.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40
      [--port N] [--mips 110] [--cycles 2] [--max-ms 80]

The scope page, the key / tap volume at 50 %. Real touch-controller taps
(emu/peripherals/CST_Touch.cs) on the CH1 panel button (447, 80): open,
close, --cycles times, 1 s apart. The TIM14 log (run.py --buzzer-log) gives
each tap beep's on -> off time. A tap beep (30 ms) is switched off by the
main loop (App::tick), so it lasts until the pass that drew the tap's
screen has ended: the beep is the redraw's length. Before the fix of
2026-10-26 a popup close repainted the whole scope page: 262 ms on the
device (DBG:PROF? tick max), a 293 ms beep here; after it, the graph
under the popup: 64 ms here.

Fails (exit 1) when a tap beep is longer than --max-ms, a tap has no beep,
the popup did not open / close or the CH1 panel label is not grey with
it open / back to normal after (a screenshot after each tap). --pcs MS
samples the PC (Renode `cpu PC`) for a profile of the close tap's redraw
(map the addresses with llvm-nm). --mips 110 is the device model
(emu/peripherals/CpuCost.cs).
"""
import argparse
import os
import subprocess
import sys
import time

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True)
ap.add_argument('--port', type=int, default=None)
ap.add_argument('--mips', default='110', help='CPU MIPS (110: the device model)')
ap.add_argument('--t0', type=float, default=14.0, help='emulator s of the first tap (the page must be up)')
ap.add_argument('--cycles', type=int, default=2, help='open + close pairs')
ap.add_argument('--max-ms', type=float, default=80.0, help='fail above this beep length')
ap.add_argument('--keep', action='store_true', help='keep the screenshots and the buzzer log')
ap.add_argument('--pcs', type=float, default=0, metavar='MS',
                help='profile: sample the PC every 0.2 ms for MS ms after the first close tap (emu.out, kept)')
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
if A.port is None:
    A.port = run.free_port()
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

OUT = os.path.join(HERE, 'out')
tag = 'popupbeep-%d-%d' % (A.port, os.getpid())
work = os.path.join(OUT, tag)
os.makedirs(work, exist_ok=True)
script = os.path.join(work, 'script')
blog = os.path.join(work, 'buzzer.log')
X, Y = 447, 80
taps = [A.t0 + k for k in range(2 * A.cycles)]
t_end = taps[-1] + 0.9
with open(script, 'w') as f:
    for k, t in enumerate(taps):
        f.write('%.4f monitor sysbus.i2c3.touch Press %d %d\n' % (t, X, Y))
        f.write('%.4f monitor sysbus.i2c3.touch Release\n' % (t + 0.06))
        f.write('%.4f shot %s\n' % (t + 0.8, os.path.join(work, 'tap%d.png' % k)))
    for k in range(int(A.pcs * 5)):
        f.write('%.5f monitor sysbus.cpu PC\n' % (taps[1] + 0.0002 * k))
if A.pcs:
    A.keep = True
cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '%.3f' % (t_end + 0.2), '--name', tag,
       '--log', os.path.join(work, 'emu.log'), '--buzzer-log', blog, '--script', script]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=open(os.path.join(work, 'emu.out'), 'w'), stderr=subprocess.STDOUT, start_new_session=True)
rc = 0
try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('popupbeep: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60):
        t.write(line)
        if '?' not in line:
            return None
        return t.read(maxlen=16384, timeout=timeout).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0])

    def until(ms):
        while up() < ms:
            time.sleep(0.05)

    print('*IDN?', q('*IDN?'), flush=True)
    until(3000)
    q('SYST:BEEP:VOL 50')
    q('DBG:KEY 0')
    until(up() + 500)
    q('DBG:TAP 341,107')  # the Oscilloscope tile
    until(up() + 1500)
    print('view', q('DBG:VIEW?'), flush=True)
    # The taps are in emulator time; the uptime trails it by the boot.
    if up() > (A.t0 - 3.5) * 1000:
        print('FAIL setup done after the first tap (raise --t0)', flush=True)
        rc = 1
    emu.wait(timeout=1800)
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    try:
        emu.wait(timeout=30)
    except Exception:
        pass

# Beeps: CCR1 0 -> non-zero -> 0 (virtual ms).
beeps = []
on = None
try:
    lines = open(blog).read().split('\n')
except OSError:
    lines = []
for ln in lines:
    p = ln.split()
    if len(p) != 3 or p[1] != 'CCR1':
        continue
    ms, v = float(p[0]), int(p[2], 0)
    if v and on is None:
        on = ms
    elif not v and on is not None:
        beeps.append((on, ms - on))
        on = None


def popup_open(path):
    """The popup's grey page area (293..412, 50..299) is up, and the CH1
    panel label is grey (its fill differs from the V/div button's below),
    as (popup, label) or None."""
    try:
        px = Image.open(path).convert('RGB').load()
    except Exception:
        return None
    grey = sum(1 for x in range(300, 410, 10) for y in range(120, 290, 10) if min(px[x, y]) > 150)
    return grey > 100, px[420, 70] != px[420, 110]


for k, tt in enumerate(taps):
    b = [d for s, d in beeps if tt * 1000 <= s < (tt + 0.5) * 1000]
    st = popup_open(os.path.join(work, 'tap%d.png' % k))
    what = 'open' if k % 2 == 0 else 'close'
    print('tap %d (%s): beep %s ms, popup %s, CH1 label %s' % (k, what, '/'.join('%.0f' % d for d in b) or '-',
          '?' if st is None else 'open' if st[0] else 'closed', '?' if st is None else 'grey' if st[1] else 'normal'), flush=True)
    if not b:
        print('FAIL tap %d: no beep' % k, flush=True)
        rc = 1
    elif max(b) > A.max_ms:
        print('FAIL tap %d: beep %.0f ms > %.0f ms (the redraw holds the buzzer on)' % (k, max(b), A.max_ms), flush=True)
        rc = 1
    if st != (k % 2 == 0, k % 2 == 0):
        print('FAIL tap %d: the popup and the grey CH1 label should be %s' % (k, 'up' if k % 2 == 0 else 'gone'), flush=True)
        rc = 1
if not A.keep:
    for f in os.listdir(work):
        os.remove(os.path.join(work, f))
    os.rmdir(work)
    for f in (os.path.join(OUT, tag + '.resc'), os.path.join(OUT, 'flash-%s.bin' % tag)):
        if os.path.exists(f):
            os.remove(f)
else:
    print('kept', work)
print('PASS' if rc == 0 else 'FAIL', flush=True)
sys.exit(rc)
