#!/usr/bin/env python3
"""opendm40 touch test page (firmware/src/app_touchtest.rs, ui_touchtest.rs):
it opens from Settings 3/3, draws dots for touches of the touch controller
model (emu/peripherals/CST_Touch.cs), lights the touch-key boxes, ignores
the touch keys, clears, and exits with a 2 s hold and with the power key.

  cd firmware && ./build.sh
  emu/touchpagetest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 [--port P] [--keep DIR]

Everything is scripted in emulator time (run.py --script): MENU, the
Settings tile, More, More, the "Touch test" row; then taps at targets, one
of them off by +11,+18 px (the scale error the page is for), a drag, MENU
held, Clear, a 2 s hold (exit), the row again, the power key (exit).
Screenshots at fixed times are checked for the dots (raw orange 5 x 5,
debounced green 3 x 3), the touch-down cross (red), the MENU box lit and
the dots gone after Clear. Over USB/IP, DBG:VIEW? is polled throughout: the
page must be shown from the row tap until the hold exit, the MENU key must
not leave it, and both exits must land on Settings 3/3.

USB/IP port: --port, else a free one in 65200-65300. Exit status 0 = PASS.
"""
import argparse
import os
import shutil
import subprocess
import sys
import time

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
OUT = os.path.join(HERE, 'out')
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--port', type=int, default=None, help='USB/IP port (default: a free one in 65200-65300)')
ap.add_argument('--keep', help='copy the screenshots here')
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
if A.port is None:
    A.port = run.free_port(65200, 65300)
print('USB/IP port %d' % A.port, flush=True)
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

TAG = 'touchpage-%d-%d' % (A.port, os.getpid())
SHOTS = os.path.join(OUT, TAG)
FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


def rgb(c):
    """RGB565 -> the screenshot's RGB (the panel model's expansion, within
    the tolerance of `near`)."""
    return ((c >> 11) << 3, ((c >> 5) & 0x3F) << 2, (c & 0x1F) << 3)


C_BG = rgb(0x10A3)       # ui.rs M_BG
C_TILE = rgb(0x2146)     # M_TILE (a key box, not lit)
C_ACCENT = rgb(0x04FC)   # M_ACCENT (lit)
C_RAW = rgb(0xFC40)      # ui_touchtest.rs C_RAW
C_DEB = rgb(0x07E0)      # C_DEB
C_DOWN = rgb(0xF800)     # C_DOWN


def near(a, b, tol=24):
    return all(abs(x - y) <= tol for x, y in zip(a, b))


def count(im, x0, y0, x1, y1, c):
    """Pixels of colour `c` in the box x0..x1, y0..y1 (inclusive, clipped)."""
    n = 0
    for y in range(max(0, y0), min(319, y1) + 1):
        for x in range(max(0, x0), min(479, x1) + 1):
            if near(im.getpixel((x, y)), c):
                n += 1
    return n


# Settings list rows (ui.rs SET_Y0 44, pitch 30, height 27): row centre.
def row_y(i):
    return 44 + 30 * i + 13


T_OPEN = 10.0
EV = [
    (6.0, 'key menu'),                                       # DMM -> main menu
    (7.0, 'tap 339 219'),                                    # Settings tile
    (8.0, 'tap 240 %d' % row_y(8)),                          # More -> 2/3
    (9.0, 'tap 240 %d' % row_y(8)),                          # More -> 3/3
    (T_OPEN, 'tap 240 %d' % row_y(5)),                       # Touch test
    (12.0, 'tap 240 160'),                                   # the centre target, exact
    (12.4, 'tap 470 317'),                                   # (459,299) at +11,+18
    (12.8, 'tap 25 24'),                                     # (20,20) at +5,+4
    (13.2, 'monitor sysbus.i2c3.touch Drag 300 100 420 140 300'),
    (14.0, 'monitor sysbus.i2c3.touch Touch "300,250;350,250"'),  # two fingers
    (14.1, 'monitor sysbus.i2c3.touch Touch ""'),
    (14.5, 'monitor sysbus.i2c3.touch KeyDown 0'),           # MENU held
    (14.6, 'shot %s' % os.path.join(SHOTS, 'b-menu-held.png')),
    (14.9, 'monitor sysbus.i2c3.touch KeyUp'),
    (15.2, 'shot %s' % os.path.join(SHOTS, 'a-dots.png')),
    (15.5, 'tap 382 259'),                                   # Clear
    (16.2, 'shot %s' % os.path.join(SHOTS, 'c-cleared.png')),
    (17.0, 'monitor sysbus.i2c3.touch Press 300 300'),       # hold: exit at 2 s
    (18.0, 'shot %s' % os.path.join(SHOTS, 'd-holding.png')),
    (20.5, 'monitor sysbus.i2c3.touch Release'),
    (21.0, 'shot %s' % os.path.join(SHOTS, 'e-after-hold.png')),
    (22.0, 'tap 240 %d' % row_y(5)),                         # Touch test again
    (23.0, 'shot %s' % os.path.join(SHOTS, 'f-reopened.png')),
    (24.0, 'key power'),                                     # exit
    (25.0, 'shot %s' % os.path.join(SHOTS, 'g-after-power.png')),
]
END = 26.0

os.makedirs(SHOTS, exist_ok=True)
script = os.path.join(OUT, TAG + '.script')
with open(script, 'w') as f:
    for t, e in EV:
        f.write('%.3f %s\n' % (t, e))
cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01', '--usbip-port', str(A.port),
       '--run-for', '%.1f' % END, '--name', TAG, '--script', script]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=open(os.path.join(OUT, TAG + '.out'), 'w'), stderr=subprocess.STDOUT,
                       start_new_session=True)
views = []  # (uptime ms, DBG:VIEW? reply), on change
try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('touchpagetest: no USB/IP device on port %d' % A.port)

    def q(s, timeout=60):
        t.write(s)
        return t.read(timeout=timeout).decode(errors='replace').strip()

    print('*IDN?', q('*IDN?'), flush=True)
    # Poll the view until the run ends (the device goes away).
    while emu.poll() is None:
        try:
            up = int(q('DBG:PING?').split(',')[0])
            v = q('DBG:VIEW?')
        except Exception:
            break
        if not views or views[-1][1] != v:
            views.append((up, v))
            print('  view %6d ms: %s' % (up, v), flush=True)
    emu.wait(timeout=600)
except Exception as e:
    check(False, 'USB/IP link: %r' % e)
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    emu.wait()

names = [v.split(',')[0] for _, v in views]
# The page: opened from Settings 3/3, closed to Settings 3/3 (hold), opened
# again, closed to Settings 3/3 (power key); MENU held in between never left it.
seq = []
for v in (v for _, v in views):
    k = 'T' if v.startswith('Touch test') else ('S3' if v.startswith('Settings,3') else v.split(',')[0])
    if not seq or seq[-1] != k:
        seq.append(k)
print('  views: %s' % ' -> '.join(seq), flush=True)
i = seq.index('T') if 'T' in seq else -1
check(i > 0 and seq[i - 1] == 'S3', 'the page opens from Settings 3/3 (the "Touch test" row)')
check(seq[i:i + 4] == ['T', 'S3', 'T', 'S3'] if i >= 0 else False,
      'hold 2 s -> Settings 3/3, the row again -> the page, power key -> Settings 3/3 (%s)' % seq[i:])
check('Main menu' not in seq[i:] if i >= 0 else False, 'MENU on the page does not leave it')


def shot(name):
    p = os.path.join(SHOTS, name)
    if not os.path.exists(p):
        check(False, '%s taken' % name)
        return None
    return Image.open(p).convert('RGB')


im = shot('a-dots.png')
if im:
    # Centre target (240,160) exact; (470,317); (25,24); the drag's middle.
    for (x, y), what in [((240, 160), 'the centre target'), ((470, 317), '(470,317), the +11,+18 tap'),
                         ((25, 24), '(25,24)')]:
        r = count(im, x - 3, y - 3, x + 3, y + 3, C_RAW)
        d = count(im, x - 3, y - 3, x + 3, y + 3, C_DEB)
        c = count(im, x - 6, y - 6, x + 6, y + 6, C_DOWN)
        check(r > 0 and d > 0 and c >= 8, 'dots at %s: raw %d px, debounced %d px, cross %d px' % (what, r, d, c))
    r = count(im, 330, 100, 400, 140, C_RAW) + count(im, 330, 100, 400, 140, C_DEB)
    check(r >= 20, 'the drag left a trail (%d dot px in 330..400 x 100..140)' % r)
    r = count(im, 296, 246, 354, 254, C_RAW)
    check(r > 0, 'the two-finger touch: a raw dot at its first finger (%d px)' % r)
    check(count(im, 50, 50, 120, 60, C_TILE) > 200, 'MENU box not lit after its release')
im = shot('b-menu-held.png')
if im:
    check(count(im, 50, 50, 120, 60, C_ACCENT) > 200, 'MENU box lit while the key is held')
    check(count(im, 50, 230, 120, 240, C_TILE) > 200, 'MF box not lit')
im = shot('c-cleared.png')
if im:
    n = count(im, 465, 312, 475, 319, C_RAW) + count(im, 235, 155, 245, 165, C_DEB)
    check(n == 0, 'Clear: the dots are gone (%d px left)' % n)
    check(count(im, 0, 0, 479, 319, C_BG) > 100000, 'Clear: the page is drawn again')
im = shot('d-holding.png')
if im:
    check(count(im, 150, 230, 320, 280, C_DOWN) > 50, 'holding: the hint says "Keep holding" (red)')

if A.keep:
    os.makedirs(A.keep, exist_ok=True)
    for f in os.listdir(SHOTS):
        shutil.copy(os.path.join(SHOTS, f), A.keep)
shutil.rmtree(SHOTS, ignore_errors=True)
print('FAIL: %d checks' % len(FAIL) if FAIL else 'PASS')
sys.exit(1 if FAIL else 0)
