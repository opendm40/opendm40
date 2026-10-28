#!/usr/bin/env python3
"""opendm40 battery charging blink and the "USB" host badge in the DMM status
bar (firmware/src/power.rs shown_bars / pins, src/ui.rs status_right).

  cd firmware && ./build.sh
  emu/chargetest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

The charge status pins are driven from the Renode monitor (run.py --script):
PB10 high from 1 s (PB11 is left to opendm40's pull-up, so it reads high:
the stock "charging" condition PB10 = PB11 = 1), PB10 low from --t-off s
(default 30 s virtual). Checks, over USB/IP:
  * DBG:DIAG? byte 61: PB10 = PB11 = 1 and USB configured while charging,
    PB10 = 0 after --t-off;
  * the battery (DBG:LCD? of 435..474 x 9..24) is green while charging and
    its bar count alternates (two different frames of 5 reads 1.3 s apart), not
    green after --t-off;
  * "USB" (DBG:LCD? of 409..431 x 10..24) has ink while the host is
    attached.
Which TP4056 pins PB10 / PB11 really are is unknown (hw/README.md);
this checks the firmware's handling of the stock condition only.
Exit status 0 = PASS.
"""
import argparse
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--port', type=int, default=None, help='USB/IP port (default: a free one, run.free_port)')
ap.add_argument('--t-off', type=float, default=30.0, help='virtual s at which PB10 goes low (default 30)')
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402  (emu/run.py: free_port)
A.port = A.port or run.free_port()
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

OUT = os.path.join(HERE, 'out')
TAG = 'charge-%d-%d' % (A.port, os.getpid())
FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


script = os.path.join(OUT, TAG + '.script')
os.makedirs(OUT, exist_ok=True)
with open(script, 'w') as f:
    f.write('1.0 monitor sysbus.gpioPortB OnGPIO 10 true\n')
    f.write('%.1f monitor sysbus.gpioPortB OnGPIO 10 false\n' % A.t_off)

emu = subprocess.Popen([sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--app', A.app, '--fast', '--usbip', '0.01',
                        '--usbip-port', str(A.port), '--run-for', '3000', '--name', TAG, '--script', script],
                       cwd=ROOT, stdout=open(os.path.join(OUT, TAG + '.out'), 'w'), stderr=subprocess.STDOUT,
                       start_new_session=True)
try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('chargetest: no USB/IP device on port %d' % A.port)

    def q(line, raw=False, timeout=60):
        t.write(line)
        b = t.read(maxlen=8192, timeout=timeout)
        return b if raw else b.decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0])

    def wait_until(ms):
        while up() < ms:
            time.sleep(0.3)

    def block(b):
        nd = int(b[1:2])
        return b[2 + nd:2 + nd + int(b[2:2 + nd])]

    def lcd(x, y, w, h):
        """RGB565 pixels of (x, y, w, h), rows top first (lcd.rs read_span)."""
        n = w * h * 3 // 2 + 2
        d = block(q('DBG:LCD? %d,%d,%d,%d,%d' % (x, y, w, h, n), raw=True))
        wd = [d[i] | d[i + 1] << 8 for i in range(2, len(d) - 1, 2)]  # word 0 = the dummy
        bs = bytearray()
        for v in wd:
            bs += bytes([v >> 8, v & 0xFF])
        return [(bs[3 * i] & 0xF8) << 8 | (bs[3 * i + 1] & 0xFC) << 3 | bs[3 * i + 2] >> 3 for i in range(w * h)]

    def diag61():
        return block(q('DBG:DIAG?', raw=True))[61]

    def green(px):
        # 0x04A9-ish: G well above R and B.
        return sum(1 for p in px if (p >> 5 & 0x3F) > 24 and (p >> 11) < 6 and (p & 0x1F) < 16)

    def white(px):
        return sum(1 for p in px if (p >> 11) > 24 and (p >> 5 & 0x3F) > 48 and (p & 0x1F) > 24)

    print('*IDN?', q('*IDN?', timeout=30), flush=True)
    wait_until(4000)
    d = diag61()
    check(d & 7 == 7, 'charging: DIAG byte 61 = %#x (PB10, PB11, USB configured all 1)' % d)
    usb = lcd(409, 10, 23, 15)
    check(white(usb) >= 20, '"USB" shown while the host is attached (%d white px)' % white(usb))
    # A DBG:LCD? holds the drawing for 300 ms (main.rs draw_hold_until):
    # one read per 1.3 s of virtual time, so the 1 s status redraw runs.
    frames = set()
    g = []
    for _ in range(5):
        wait_until(up() + 1300)
        px = lcd(435, 9, 40, 16)
        g.append(green(px))
        frames.add(tuple(px))
    check(min(g) >= 40, 'battery green while charging (green px per frame %d..%d)' % (min(g), max(g)))
    check(len(frames) >= 2, 'battery level blinks while charging (%d different frames of 5, 1.3 s apart)' % len(frames))
    wait_until(int(A.t_off * 1000) + 2500)
    d = diag61()
    check(d & 7 == 6, 'not charging: DIAG byte 61 = %#x (PB10 0, PB11 1, USB 1)' % d)
    px = lcd(435, 9, 40, 16)
    check(green(px) == 0 and white(px) >= 40, 'battery white after PB10 low (green %d, white %d px)' % (green(px), white(px)))
except Exception as e:
    check(False, 'the meter answers: %r' % e)
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    emu.wait()
print('FAIL: %d checks' % len(FAIL) if FAIL else 'PASS')
sys.exit(1 if FAIL else 0)
