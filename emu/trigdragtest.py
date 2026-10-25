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
  * right after the trigger-level drag, a slanted sideways drag (80 x 20
    px) that starts on the trace next to the shown trigger line moves only
    the trigger position, by 80 px (one item, one axis per gesture; the
    guide lines are not zones);
  * a sideways drag along CH1's guide line right after, and one off the
    markers, still move the trigger position.

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

    # Right after (the trigger-level guide line shows, the trigger level
    # selected): a slanted sideways drag (80 x 20 px) that starts on the
    # trace 6 px under that line pans: only the trigger position, by the
    # horizontal travel, and the level stays (owner 2026-10-25).
    y = top + r1 + 6
    q('DBG:DRAG 150,%d,230,%d,400,0,0' % (y, y + 20))
    wait_ms(300)
    hs, rs, ps = pos()
    print('  slanted pan next to the trigger line: hpos %d, trig_row %d, pos %d' % (hs, rs, ps), flush=True)
    check(rs == r1, 'a pan starting next to the shown trigger line keeps the trigger level (%d -> %d)' % (r1, rs))
    check(hs == h1 + 80 and ps == p1, 'it moves only the trigger position, by the horizontal travel (%d -> %d)' % (h1, hs))
    h1 = hs

    y = top + 140 + p1
    # 300 ms: the first 8 px of travel must be mostly vertical (the axis
    # is decided by the first motion; at 600 ms the 8 px jitter would come
    # first and decide horizontal, which a level marker ignores).
    q('DBG:DRAG 6,%d,6,%d,300,0,%d' % (y, y - 40, j))
    wait_ms(800)
    h2, r2, p2 = pos()
    print('  CH1 drag: hpos %d, trig_row %d, pos %d' % (h2, r2, p2), flush=True)
    check(p2 != p1, 'CH1 drag moved CH1 (%d -> %d)' % (p1, p2))
    check(h2 == h1, 'CH1 drag with %d px sideways wobble kept the trigger position (%d -> %d)' % (j, h1, h2))

    # Right after (the guide lines show): a sideways drag along CH1's
    # zero line (a line zone: selects CH1, the drag decides its axis).
    y = top + 140 + p2
    q('DBG:DRAG 150,%d,210,%d,400,0,0' % (y + 4, y + 4))
    wait_ms(300)
    hl, rl, pl = pos()
    print('  sideways drag along the CH1 guide line: hpos %d, trig_row %d, pos %d' % (hl, rl, pl), flush=True)
    check(hl == h2 + 60 and pl == p2, 'a sideways drag along the shown CH1 guide line still pans (%d -> %d)' % (h2, hl))
    h2 = hl
    wait_ms(2500)  # the guide lines gone
    q('DBG:DRAG 150,160,210,160,400,0,0')
    wait_ms(800)
    h3, r3, p3 = pos()
    print('  plain sideways drag: hpos %d, trig_row %d, pos %d' % (h3, r3, p3), flush=True)
    check(h3 != h2, 'a sideways drag off the markers still moves the trigger position (%d -> %d)' % (h2, h3))
    check(p3 == p2 and r3 == r2, 'a sideways drag off the markers keeps the levels')

    # A vertical drag mid-plot, 3 px off the trigger row, with CH1 selected
    # (the last drag above was CH1's... and two pans): the nearest level
    # marker moves, the trigger level, not the selection (owner 2026-10-25).
    h4, r4, p4 = pos()
    y = top + r4 + 3
    if abs(y - (top + 140 + p4)) > 20:
        q('DBG:DRAG 200,%d,200,%d,300,0,0' % (y, y + 30))
        wait_ms(300)
        h5, r5, p5 = pos()
        print('  vertical drag near the trigger row mid-plot: hpos %d, trig_row %d, pos %d' % (h5, r5, p5), flush=True)
        check(r5 != r4 and p5 == p4 and h5 == h4, 'a vertical drag mid-plot moves the nearest level (the trigger level), not CH1 (%d -> %d)' % (r4, r5))

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
