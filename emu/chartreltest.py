#!/usr/bin/env python3
"""opendm40 DMM trend chart on REL on / off (#98): the chart starts over, so
the REL deviations are not squashed under the absolute points (and back).

  cd firmware && ./build.sh
  emu/chartreltest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 [--port P] [--shots DIR]

3.3 V DC on V-COM, the chart full of 3.3 V points (8 s; it holds 6.4 s):
  * tap REL: the axis labels (DBG:LCD?, x 0..76, rows 172..281) 2 s later
    equal those 9 s later, when every point is a REL one (before the fix: 2 s
    later the scale still spanned 0 .. 3.3 V);
  * tap REL again: the same for the absolute points.
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
ap.add_argument('--shots', help='directory for PNG screenshots of each step')
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


# The chart's axis labels: rows 172..281, x 0..76.
LABELS = (172, 110, 0, 77)

tag = 'chartrel-%d-%d' % (A.port, os.getpid())
cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--app', A.app, '--fast',
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '3000', '--name', tag,
       '--dmm', 'vsrc:3.3']
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
        sys.exit('chartreltest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60, raw=False):
        t.write(line)
        if '?' not in line:
            return None
        b = t.read(maxlen=65536, timeout=timeout)
        return b if raw else b.decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0])

    def wait_ms(ms):
        t0 = up()
        while up() - t0 < ms:
            time.sleep(0.1)

    def block(b):
        nd = int(b[1:2])
        return b[2 + nd:2 + nd + int(b[2:2 + nd])]

    def lcd(y0, h, x0=0, w=480):
        """Panel rows as RGB tuples (lcd.rs read_span's decode, as shottest.py)."""
        px = []
        for y in range(y0, y0 + h, 4):
            rows = min(4, y0 + h - y)
            n = (w * rows * 3 + 1) // 2 + 1
            d = block(q('DBG:LCD? %d,%d,%d,%d,%d' % (x0, y, w, rows, n), raw=True))
            wd = [d[i] | d[i + 1] << 8 for i in range(2, len(d), 2)]
            bs = bytearray()
            for v in wd:
                bs += bytes([v >> 8, v & 0xFF])
            for i in range(w * rows):
                px.append((bs[3 * i] & 0xF8, bs[3 * i + 1] & 0xFC, bs[3 * i + 2] & 0xF8))
        return px

    def shot(name):
        if not A.shots:
            return
        from PIL import Image
        os.makedirs(A.shots, exist_ok=True)
        im = Image.new('RGB', (480, 320))
        im.putdata(lcd(0, 320))
        im.save(os.path.join(A.shots, name + '.png'))

    print('*IDN?', q('*IDN?'), flush=True)
    wait_ms(8000)
    lab0 = lcd(*LABELS)
    shot('0-abs')
    for k, what in ((1, 'REL on'), (2, 'REL off')):
        q('DBG:TAP 440,224')  # the REL box
        wait_ms(2000)
        lab1 = lcd(*LABELS)
        shot('%d-rel-2s' % k)
        wait_ms(9000)
        lab2 = lcd(*LABELS)
        shot('%d-rel-11s' % k)
        if k == 1:
            check(sum(a != b for a, b in zip(lab0, lab2)) > 50, 'REL on: the labels change from the 3.3 V scale')
        diff = sum(a != b for a, b in zip(lab1, lab2))
        check(diff == 0, '%s: the labels 2 s after = 11 s after (%d pixels differ)' % (what, diff))
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    try:
        emu.wait(timeout=30)
    except Exception:
        os.killpg(emu.pid, 9)

print('%s: %d failure(s)' % ('chartreltest', len(FAIL)), flush=True)
sys.exit(1 if FAIL else 0)
