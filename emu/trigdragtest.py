#!/usr/bin/env python3
"""opendm40 scope marker drags in the emulator: only a drag that starts in a
marker's grab zone moves anything (owner 2026-10-30), a wobbling finger
moves the marker along its own axis only.

  cd firmware && ./build.sh
  emu/trigdragtest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 [--port N]

The scope page (MENU, the Oscilloscope tile) runs on a 1 kHz sine. Each
drag is DBG:DRAG, some with a sideways sawtooth on the path (jitter: +j,
+j/2, 0 per 3 points); DBG:GEST?'s "|POS hpos,trig_row,pos" is read before
and after.

  * the trigger-level marker (right edge, x 406), dragged 60 px down with
    the wobble: the trigger row follows the finger (+60), the trigger
    position and CH1 stay;
  * a drag that starts 20 px left of the trigger-level marker (x 386)
    moves it too, by the finger's travel;
  * drags that start on the empty graph (a slanted pan next to the shown
    trigger line, a sideways drag along the CH1 line, a plain sideways drag,
    a vertical drag 3 px off the trigger row mid-plot) change nothing;
  * the CH1 marker (left strip, x 6) and 20 px right of it (x 26): CH1
    follows the finger, the trigger position stays;
  * the trigger-position marker T (top edge) and 20 px below it: the
    trigger position follows the finger sideways.

Last: TRIG:LEV 0.8 then CHAN1:SCAL 1 / 0.5 / 2 / 0.05 / 1: TRIG:LEV? keeps
its volts, the marker row moves (at the top edge, row -10, while the level
is off screen at 50 mV/div).

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

    def drag(what, x0, y0, x1, y1, ms=400, jit=0, want=None):
        """DBG:DRAG, then the positions; `want` (hpos, trig_row, pos) = the
        expected positions after it."""
        before = pos()
        q('DBG:DRAG %d,%d,%d,%d,%d,0,%d' % (x0, y0, x1, y1, ms, jit))
        wait_ms(800)
        after = pos()
        print('  %s: %s -> %s' % (what, before, after), flush=True)
        if want is not None:
            check(after == want, '%s: positions %s, want %s' % (what, after, want))
        return after

    h0, r0, p0 = pos()
    print('  before: hpos %d, trig_row %d, pos %d' % (h0, r0, p0), flush=True)
    y = top + r0
    h, r, p = drag('trigger-level marker 60 px down, %d px sideways wobble' % j, 406, y, 406, y + 60, 600, j, (h0, r0 + 60, p0))
    y = top + r
    h, r, p = drag('20 px left of the trigger-level marker, 40 px up', 386, y, 386, y - 40, 400, 0, (h, r - 40, p))

    # The empty graph (the trigger level just dragged: its guide line shows).
    y = top + r + 6
    drag('slanted pan next to the shown trigger line (empty graph)', 150, y, 230, y + 20, 400, 0, (h, r, p))
    y = top + 140 + p
    drag('sideways drag along the CH1 line (empty graph)', 150, y + 4, 210, y + 4, 400, 0, (h, r, p))
    drag('plain sideways drag mid-plot', 150, 160, 210, 160, 400, 0, (h, r, p))
    y = top + r + 3
    if abs(y - (top + 140 + p)) > 30:
        drag('vertical drag 3 px off the trigger row mid-plot', 200, y, 200, y + 30, 300, 0, (h, r, p))

    # CH1 (the trigger row moves with it: same voltage).
    y = top + 140 + p
    h, r, p = drag('CH1 marker 40 px up, %d px sideways wobble' % j, 6, y, 6, y - 40, 600, j, (h, r - 40, p - 40))
    y = top + 140 + p
    h, r, p = drag('20 px right of the CH1 marker, 30 px down', 26, y, 26, y + 30, 400, 0, (h, r + 30, p + 30))

    # The trigger position (T at the top edge, column hpos).
    x = 14 + h
    h, r, p = drag('T marker 50 px right', x, 26, x + 50, 26, 400, 0, (h + 50, r, p))
    x = 14 + h
    h, r, p = drag('20 px below the T marker, 40 px left', x, 46, x - 40, 50, 400, 0, (h - 40, r, p))

    # The trigger level keeps its volts across V/div changes (owner
    # 2026-10-25): TRIG:LEV 0.8, CH1:SCAL 0.5 / 2 / 0.05 (off screen) / 1;
    # TRIG:LEV? stays, the marker row (DBG:GEST? |POS) moves.
    q('TRIG:LEV 0.8')
    q('CHAN1:SCAL 1')
    wait_ms(500)
    lv = []
    for vd in ['1', '0.5', '2', '0.05', '1']:
        q('CHAN1:SCAL %s' % vd)
        wait_ms(500)
        lv.append((vd, float(q('TRIG:LEV?')), pos()[1]))
    print('  V/div, TRIG:LEV?, row: %s' % lv, flush=True)
    v0 = lv[0][1]
    check(all(abs(v - v0) <= 1e-3 for _, v, _ in lv), 'V/div changes keep the trigger level in volts (%.4f V)' % v0)
    rows = [r for _, _, r in lv]
    check(rows[1] != rows[0] and rows[2] != rows[0] and rows[3] == -10 and rows[4] == rows[0],
          'the marker row follows the scale, at the edge while off screen, back after (%s)' % rows)
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    emu.wait(timeout=30)

print('%s: %d failure(s)' % ('trigdragtest', len(FAIL)), flush=True)
sys.exit(1 if FAIL else 0)
