#!/usr/bin/env python3
"""opendm40 scope marker drags with a wobbling finger, in the emulator.

  cd firmware && ./build.sh
  emu/trigdragtest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 [--port N]

The scope page (MENU, the Oscilloscope tile) runs on a 1 kHz sine. Each
drag is DBG:DRAG with a sideways sawtooth on the path (jitter: +j, +j/2, 0
per 3 points, so every third move report is mostly sideways: the stock
per-event split sent it to the trigger position and dropped the vertical
part); DBG:GEST?'s "|POS hpos,trig_row,pos" is read before and after.

  * the trigger-level marker (right edge, x 406), dragged 60 px down with
    the wobble ending where it started: the trigger row moves, the trigger
    position is where it was (the 2-D trigger drag nets the sideways
    wobble to 0; the stock split would have moved it +j px per 3 points);
  * the CH1 marker (left strip, x 6), the same: CH1 moves, the trigger
    position does not (axis lock of a level marker);
  * a sideways drag off the markers (plain pan of the selected time item,
    the trigger position) still moves the trigger position.

Exit status 0 = all passed.
"""
import argparse
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF')
ap.add_argument('--port', type=int, default=None, help='USB/IP port (default: run.free_port)')
ap.add_argument('--mips', default='200')
ap.add_argument('--jitter', type=int, default=8)
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
if A.port is None:
    A.port = run.free_port()
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


OUT = os.path.join(HERE, 'out')
os.makedirs(OUT, exist_ok=True)
tag = 'trigdrag-%d-%d' % (A.port, os.getpid())
log = os.path.join(OUT, '%s.log' % tag)
cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '3000', '--name', tag, '--log', log,
       '--scope', 'sine,1000,3.0']
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)

try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('trigdragtest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60):
        t.write(line)
        if '?' not in line:
            return None
        return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

    def wait_ms(ms):
        t0 = int(q('DBG:PING?').split(',')[0])
        while int(q('DBG:PING?').split(',')[0]) - t0 < ms:
            time.sleep(0.1)

    def pos():
        g = q('DBG:GEST?')
        p = g.split('|POS ')[1].split('|')[0]
        return tuple(int(v) for v in p.split(','))

    print('*IDN?', q('*IDN?'), flush=True)
    wait_ms(2500)
    q('DBG:KEY 0')  # main menu
    wait_ms(500)
    q('DBG:TAP 341,107')  # the Oscilloscope tile
    wait_ms(1500)
    v = q('DBG:VIEW?')
    check(v.startswith('Oscilloscope'), 'on the scope page (%s)' % v)
    top = 20
    j = A.jitter

    h0, r0, p0 = pos()
    print('  before: hpos %d, trig_row %d, pos %d' % (h0, r0, p0), flush=True)
    y = top + r0
    q('DBG:DRAG 406,%d,406,%d,600,0,%d' % (y, y + 60, j))
    wait_ms(800)
    h1, r1, p1 = pos()
    print('  trigger-level drag: hpos %d, trig_row %d, pos %d' % (h1, r1, p1), flush=True)
    check(r1 != r0, 'trigger-level drag moved the trigger level (%d -> %d)' % (r0, r1))
    check(h1 == h0, 'trigger-level drag with %d px sideways wobble kept the trigger position (%d -> %d)' % (j, h0, h1))
    check(p1 == p0, 'trigger-level drag kept CH1 (%d -> %d)' % (p0, p1))

    y = top + 140 + p1
    q('DBG:DRAG 6,%d,6,%d,600,0,%d' % (y, y - 40, j))
    wait_ms(800)
    h2, r2, p2 = pos()
    print('  CH1 drag: hpos %d, trig_row %d, pos %d' % (h2, r2, p2), flush=True)
    check(p2 != p1, 'CH1 drag moved CH1 (%d -> %d)' % (p1, p2))
    check(h2 == h1, 'CH1 drag with %d px sideways wobble kept the trigger position (%d -> %d)' % (j, h1, h2))

    q('DBG:DRAG 150,160,210,160,400,0,0')
    wait_ms(800)
    h3, r3, p3 = pos()
    print('  plain sideways drag: hpos %d, trig_row %d, pos %d' % (h3, r3, p3), flush=True)
    check(h3 != h2, 'a sideways drag off the markers still moves the trigger position (%d -> %d)' % (h2, h3))
    check(p3 == p2 and r3 == r2, 'a sideways drag off the markers keeps the levels')
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    emu.wait(timeout=30)

print('%s: %d failure(s)' % ('trigdragtest', len(FAIL)), flush=True)
sys.exit(1 if FAIL else 0)
