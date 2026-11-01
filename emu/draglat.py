#!/usr/bin/env python3
"""opendm40 scope: level-marker drag latency on the screen (emulator time).

  cd firmware && ./build.sh
  emu/draglat.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 [--port N]
      [--item trig|ch1] [--tb 0.005] [--mips 110] [--touchlog] [--keep | --shots DIR]

The scope page at --tb s/div (default 5 ms/div) on a 90 Hz square. One
real touch-controller drag (emu/peripherals/CST_Touch.cs Press / Move /
Release in emulator time, a report every 20 ms): the press on the level
marker (--item trig: the trigger level, x 406 at the right edge; ch1: mid
plot on CH1's row, an up / down drag off the markers moves the selected
level item, CH1 by default), --still reports at the press
point (a landing finger), then --steps reports 5 px down each, release.
Screenshots in emulator time (every --dt ms from 49 ms before the press to
--after s past the release, 10 ms apart before that) give the marker's row
(its glyph found on the screen). Per move report: the time to the marker's
next move and to the marker on the finger's row; how far it is behind the
finger before the next report. After the release DBG:GEST? must show the
level moved by the whole travel (the setting applied on release, the next
acquisition uses it).

Fails (exit 1) when the median report -> marker time exceeds --max-ms,
the marker misses the finger, ends off the release row or the setting
did not move by the travel. --mips 110 is the device model (emu/peripherals/
CpuCost.cs). There, at 5 ms/div, report -> marker (median): trigger level
15 ms before the fix of 2026-10-25 (the guide line repainted one LCD
window per graph column, the trigger badge drawn before the graph), 7 ms
after; CH1 3 ms both (its marker is in the strip, painted before the
graph). The --max-ms default (9) fails the former.

--still 0: the first move report's travel is not applied (the touch-down
debounce, src/touch.rs Debounce, accepts the second pressed frame and
reports its point, so the drag starts there); the marker then stays that
report's 5 px behind the finger. --touchlog prints DBG:TOUCH? and
DBG:PROF? (reset before the drag). --shots DIR analyses kept screenshots
(--keep) again, without a run.
"""
import argparse
import os
import shutil
import statistics
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
ap.add_argument('--tb', default='0.005', help='TIM:SCAL (s/div)')
ap.add_argument('--mode', default='AUTO', help='TRIG:SWE (AUTO / NORM)')
ap.add_argument('--t0', type=float, default=20.0, help='emulator s of the press (the page must be up)')
ap.add_argument('--steps', type=int, default=16, help='move reports (5 px each)')
ap.add_argument('--pre', type=float, default=0.3, help='screenshots before the press (every 10 ms), s')
ap.add_argument('--item', choices=['trig', 'ch1'], default='trig', help='the level marker dragged')
ap.add_argument('--still', type=int, default=1, help='still reports after the press (a finger landing)')
ap.add_argument('--dt', type=float, default=2.0, help='screenshot period, ms')
ap.add_argument('--after', type=float, default=0.6, help='screenshots after the release, s')
ap.add_argument('--max-ms', type=float, default=9.0, help='fail above this median report -> marker time')
ap.add_argument('--touchlog', action='store_true', help='print DBG:TOUCH? and DBG:PROF? after the drag')
ap.add_argument('--shots', help='analyse this kept screenshot directory, no run')
ap.add_argument('--keep', action='store_true', help='keep the screenshots')
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
if A.port is None:
    A.port = run.free_port()
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

OUT = os.path.join(HERE, 'out')
tag = 'draglat-%d-%d' % (A.port, os.getpid())
shots = os.path.join(OUT, tag)
os.makedirs(shots, exist_ok=True)
script = os.path.join(OUT, '%s.script' % tag)
# Trigger level: press on its marker (x 406, row 105 -> y 125); CH1: mid-plot
# at its row (pos 0 -> y 160), off the markers: the selected item, CH1.
X, Y0, STEP = (406, 125, 5) if A.item == 'trig' else (300, 160, 5)
# The marker glyph: the column with its full height, its tip's columns.
RUN_X, TIP_X = (412, range(400, 403)) if A.item == 'trig' else (1, range(11, 13))
T1 = A.t0 + 0.020 * A.still  # the first move report; still ones before it
moves = [(T1 + 0.020 * k, Y0 + STEP * k) for k in range(1, A.steps + 1)]
t_rel = moves[-1][0] + 0.060
t_end = t_rel + A.after
shot_t = []
with open(script, 'w') as f:
    f.write('%.4f monitor sysbus.i2c3.touch Press %d %d\n' % (A.t0, X, Y0))
    for k in range(1, A.still + 1):
        f.write('%.4f monitor sysbus.i2c3.touch Move %d %d\n' % (A.t0 + 0.020 * k, X, Y0))
    for t, y in moves:
        f.write('%.4f monitor sysbus.i2c3.touch Move %d %d\n' % (t, X, y))
    f.write('%.4f monitor sysbus.i2c3.touch Release\n' % t_rel)
    t = A.t0 - A.pre
    while t < A.t0 - 0.05:
        shot_t.append(t)
        f.write('%.4f shot %s\n' % (t, os.path.join(shots, '%08.4f.png' % t)))
        t += 0.010
    t = A.t0 - 0.049  # 1 ms off the report grid
    while t < t_end:
        shot_t.append(t)
        f.write('%.4f shot %s\n' % (t, os.path.join(shots, '%08.4f.png' % t)))
        t += A.dt / 1000.0
cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '%.3f' % (t_end + 0.2), '--name', tag,
       '--log', os.path.join(OUT, tag + '.log'), '--scope', 'square,90,3.0', '--script', script]
emu = None if A.shots else subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
rc = 0
if A.shots:
    shots = A.shots
    shot_t = sorted(float(f[:-4]) for f in os.listdir(shots) if f.endswith('.png'))
try:
    t = None
    if A.shots:
        raise StopIteration
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('draglat: no USB/IP device on port %d' % A.port)

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
    q('DBG:KEY 0')
    until(up() + 500)
    q('DBG:TAP 341,107')  # the Oscilloscope tile
    until(up() + 1500)
    print('view', q('DBG:VIEW?'), flush=True)
    q('CHAN1:SCAL 1')
    q('TIM:SCAL %s' % A.tb)
    q('TRIG:SWE %s' % A.mode)
    until(up() + 300)
    g = q('DBG:GEST?').split('|POS ')[1].split('|')[0]
    hpos, row, pos = (int(v) for v in g.split(','))
    q('TRIG:LEV %.4f' % ((140 + pos - 105) * 4.0 / 141.0))
    until(up() + 300)
    print('TIM:SCAL?', q('TIM:SCAL?'), 'TRIG:SWE?', q('TRIG:SWE?'), flush=True)
    g = q('DBG:GEST?')
    pos0 = [int(v) for v in g.split('|POS ')[1].split('|')[0].split(',')]
    print('positions (hpos, trigger row, CH1 pos):', pos0, flush=True)
    # The press is in emulator time; the uptime trails it by the boot
    # (~3 s): the setup must be done before.
    if up() > (A.t0 - 3.5) * 1000:
        print('FAIL setup done after the press (raise --t0)', flush=True)
        rc = 1
    n0 = int(g.split(';')[0].split('|')[0])
    q('DBG:PROF:RESET')
    while int(q('DBG:GEST?').split(';')[0].split('|')[0]) == n0:
        time.sleep(0.2)
    g = q('DBG:GEST?')
    pos1 = [int(v) for v in g.split('|POS ')[1].split('|')[0].split(',')]
    print('after the release:', pos1, flush=True)
    travel = STEP * A.steps
    k = 1 if A.item == 'trig' else 2
    if pos1[k] - pos0[k] != travel:
        print('FAIL the %s moved by %d, the finger by %d' % ('trigger row' if k == 1 else 'CH1 position', pos1[k] - pos0[k], travel), flush=True)
        rc = 1
    if A.touchlog:
        print(q('DBG:TOUCH?'), flush=True)
        print(g, flush=True)
        print(q('DBG:PROF?'), flush=True)
    emu.wait(timeout=1800)
except StopIteration:
    pass
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    try:
        emu.wait(timeout=30)
    except Exception:
        pass


def marker_row(path):
    """Centre row of the dragged level marker (an arrow glyph, 11 rows,
    trace colour, outline or filled: the trigger level's at x 400..412
    pointing left, CH1's at x 1..12 pointing right) or None."""
    try:
        im = Image.open(path).convert('RGB')
    except Exception:
        return None
    px = im.load()

    def ink(x, y):
        r, g, b = px[x, y]
        return max(r, g, b) > 150 and max(r, g, b) - min(r, g, b) > 80

    best = None
    y = 20
    while y < 302:
        if not ink(RUN_X, y):
            y += 1
            continue
        y1 = y
        while y1 < 302 and ink(RUN_X, y1):
            y1 += 1
        c = (y + y1 - 1) // 2
        # 9..13 rows at RUN_X, the arrow's tip on the centre row only (a
        # trace edge is a plain vertical line).
        if 9 <= y1 - y <= 13 and any(ink(x, c) for x in TIP_X) and not any(ink(x, c - 4) for x in TIP_X):
            best = c
        y = y1
    return best


rows = []
for ts in shot_t:
    f = os.path.join(shots, '%08.4f.png' % ts)
    rows.append((ts, marker_row(f)))
base = next((r for ts, r in rows if ts < A.t0 and r is not None), None)
if base is None:
    print('FAIL no marker before the press', flush=True)
    sys.exit(1)
print('marker row before the press: %d (finger y %d)' % (base, Y0), flush=True)
# Per move report: the time to the marker's next move, the time to the
# marker on the finger's row (the press row + the travel), and how far
# the marker is behind the finger just before the next report.
lat_mv, lat_on, behind = [], [], []
for k, (tm, y) in enumerate(moves):
    t_next = moves[k + 1][0] if k + 1 < len(moves) else t_rel
    before = [r for ts, r in rows if ts < tm and r is not None]
    r0 = before[-1] if before else base
    want = base + (y - Y0)
    mv = next((ts for ts, r in rows if ts >= tm and r is not None and r != r0), None)
    on = next((ts for ts, r in rows if ts >= tm and r is not None and abs(r - want) <= 1), None)
    lat_mv.append(None if mv is None or mv > t_next else (mv - tm) * 1000.0)
    lat_on.append(None if on is None or on > t_next else (on - tm) * 1000.0)
    last = [r for ts, r in rows if ts < t_next and r is not None]
    behind.append(want - last[-1] if last else None)
fmt = lambda v: '-' if v is None else '%.0f' % v
print('report -> marker moves, ms:   %s' % ' '.join(fmt(v) for v in lat_mv), flush=True)
print('report -> marker on finger, ms: %s' % ' '.join(fmt(v) for v in lat_on), flush=True)
print('marker behind the finger, px: %s' % ' '.join(fmt(v) for v in behind), flush=True)
fin = [r for ts, r in rows if ts >= t_end - 0.1 and r is not None]
want_end = base + (moves[-1][1] - Y0)
print('marker row at the end: %s (finger %d)' % (fmt(fin[-1]) if fin else '-', want_end), flush=True)
mv_ok = [v for v in lat_mv if v is not None]
on_ok = [v for v in lat_on if v is not None]
if mv_ok:
    print('report->marker ms: n %d/%d median %.1f max %.1f' % (len(mv_ok), len(lat_mv), statistics.median(mv_ok), max(mv_ok)), flush=True)
print('marker on the finger after %d of %d reports' % (len(on_ok), len(lat_on)), flush=True)
if len(mv_ok) < len(lat_mv) - 1 or not mv_ok or statistics.median(mv_ok) > A.max_ms:
    print('FAIL the marker does not follow the reports', flush=True)
    rc = 1
if len(on_ok) < len(lat_on) - 1:
    print('FAIL the marker is not on the finger', flush=True)
    rc = 1
if not fin or abs(fin[-1] - want_end) > 1:
    print('FAIL the marker is not on the release row', flush=True)
    rc = 1
if not A.keep and not A.shots:
    shutil.rmtree(shots, ignore_errors=True)
else:
    print('shots in', shots)
sys.exit(rc)
