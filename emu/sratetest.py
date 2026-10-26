#!/usr/bin/env python3
"""opendm40 scope sample rates in the timebase list (each entry's rate beside
its time, src/scope_ui.rs draw_tb / tb_rate) in the emulator, against the
host renders of tools/ui-render (scenes s07-srate-*).

  cd firmware && ./build.sh && tools/ui-render/run.sh /tmp/ur
  emu/sratetest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 --refs /tmp/ur [--shots DIR]

The list's rows (DBG:LCD?) equal the host render with the same centre
entry, and ACQ:SRAT? the centre entry's rate:
  * scope page (MCX, 1 kHz sine): the list opened at 200 us, 50 ms, 5 s/div
    (roll); opened at 2 ms/div, then 1 ms and 200 us picked in the open
    list (the rates follow the list);
  * the Wave view (the source badge, DMM V input): 1 ms/div (76.8 kS/s),
    1 s/div (roll, 1.2 kS/s).

Exit status 0 = all passed.
"""
import argparse
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF')
ap.add_argument('--refs', required=True, help='tools/ui-render/run.sh output directory (s07-srate-*.png)')
ap.add_argument('--port', type=int, default=None, help='USB/IP port (default: run.free_port)')
ap.add_argument('--shots', help='directory for PNG screenshots of each step')
ap.add_argument('--mips', default='200')
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
if A.port is None:
    A.port = run.free_port()
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402
from PIL import Image  # noqa: E402

FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


# The timebase list's rows (scope_ui.rs TB_SPIN).
ROWS = (85, 55, 120, 150)


def ref(name, box):
    """The box of a host render (2x PNG) as RGB565 values."""
    im = Image.open(os.path.join(A.refs, name + '.png')).convert('RGB')
    x0, y0, w, h = box
    out = []
    for y in range(y0, y0 + h):
        for x in range(x0, x0 + w):
            r, g, b = im.getpixel((2 * x, 2 * y))
            out.append((r >> 3) << 11 | (g >> 2) << 5 | b >> 3)
    return out


OUT = os.path.join(HERE, 'out')
os.makedirs(OUT, exist_ok=True)
tag = 'srate-%d-%d' % (A.port, os.getpid())
log = os.path.join(OUT, '%s.log' % tag)
disk = os.path.join(OUT, tag + '.disk.bin')
shutil.copy(os.path.join(ROOT, 'stock/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin'), disk)
cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '3000', '--name', tag, '--log', log,
       '--disk', disk, '--scope', 'sine,1000,3.0', '--dmm', 'vsrc:0,ac=1.0,f=50']
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
        sys.exit('sratetest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60, raw=False):
        t.write(line)
        if '?' not in line:
            return None
        b = t.read(maxlen=8192, timeout=timeout)
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
        """Panel rows as RGB565 (lcd.rs read_span's decode, as shottest.py)."""
        px = []
        for y in range(y0, y0 + h, 4):
            rows = min(4, y0 + h - y)
            n = (w * rows * 3 + 1) // 2 + 1  # 16-bit words of RGB666 bytes, + the dummy
            d = block(q('DBG:LCD? %d,%d,%d,%d,%d' % (x0, y, w, rows, n), raw=True))
            wd = [d[i] | d[i + 1] << 8 for i in range(2, len(d), 2)]
            bs = bytearray()
            for v in wd:
                bs += bytes([v >> 8, v & 0xFF])
            for i in range(w * rows):
                r, g, b = bs[3 * i], bs[3 * i + 1], bs[3 * i + 2]
                px.append((r & 0xF8) << 8 | (g & 0xFC) << 3 | b >> 3)
        return px

    def shot(name):
        if not A.shots:
            return
        os.makedirs(A.shots, exist_ok=True)
        px = lcd(0, 320)
        im = Image.new('RGB', (480, 320))
        im.putdata([((v >> 11) << 3, ((v >> 5) & 63) << 2, (v & 31) << 3) for v in px])
        im.save(os.path.join(A.shots, name + '.png'))

    def rows():
        x0, y0, w, h = ROWS
        return lcd(y0, h, x0, w)

    def step(what, cmd, refname, fs):
        if cmd:
            q(cmd)
        wait_ms(1500)
        got = rows()
        want = ref(refname, ROWS)
        diff = sum(a != b for a, b in zip(got, want))
        sr = float(q('ACQ:SRAT?'))
        print('  %s: ACQ:SRAT? %g, %d of %d pixels differ from %s' % (what, sr, diff, len(want), refname), flush=True)
        shot(refname.replace('s07-srate', 'emu'))
        check(abs(sr - fs) < 0.5, '%s: ACQ:SRAT? %g (want %g)' % (what, sr, fs))
        check(diff == 0, '%s: the timebase list = the host render %s' % (what, refname))

    def tb_list(scale, what, refname, fs):
        """TIM:SCAL with the list closed, then the list (the timebase
        badge); checked; closed again (KEY 3)."""
        q('TIM:SCAL %s' % scale)
        wait_ms(1500)
        q('DBG:TAP 110,10')  # the timebase badge
        step(what, None, refname, fs)
        q('DBG:KEY 3')
        wait_ms(800)

    print('*IDN?', q('*IDN?'), flush=True)
    wait_ms(2500)
    q('DBG:KEY 0')  # main menu
    wait_ms(500)
    q('DBG:TAP 341,107')  # the Oscilloscope tile
    wait_ms(1500)
    v = q('DBG:VIEW?')
    check(v.startswith('Oscilloscope'), 'on the scope page (%s)' % v)
    q('TRIG:SWE AUTO')

    print('scope page (MCX):', flush=True)
    tb_list('2e-4', '200 us/div', 's07-srate-tb13', 12.5e6)
    tb_list('5e-2', '50 ms/div', 's07-srate-tb20', 50e3)
    tb_list('5', '5 s/div (roll)', 's07-srate-roll', 500)
    # Picks in the open list: it re-centres on the pick, the rates with it.
    q('TIM:SCAL 2e-3')
    wait_ms(1500)
    q('DBG:TAP 110,10')
    step('2 ms/div, list open', None, 's07-srate-tb16', 1.25e6)
    q('DBG:TAP 145,100')  # the row above the band: 1 ms/div
    wait_ms(800)
    q('DBG:TAP 145,70')  # two rows above: 200 us/div
    step('picked 1 ms, then 200 us in the list', None, 's07-srate-tb13', 12.5e6)
    q('DBG:KEY 3')
    wait_ms(800)

    print('Wave view (DMM):', flush=True)
    q('DBG:TAP 177,10')  # the source badge: MCX -> DMM
    wait_ms(3000)
    isr = q('DBG:ISR?')
    check(isr.split(',')[1].strip() == '1', 'the Wave view streams (DBG:ISR? %s)' % isr[:40])
    tb_list('1e-3', '1 ms/div', 's07-srate-wave', 76800)
    tb_list('1', '1 s/div (roll)', 's07-srate-wave-roll', 1200)
    q('TIM:SCAL 1e-3')
    wait_ms(1000)
    q('DBG:TAP 177,10')  # back to MCX
    wait_ms(3000)
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    try:
        emu.wait(timeout=30)
    except Exception:
        os.killpg(emu.pid, 9)
    for f in (disk,):
        try:
            os.remove(f)
        except OSError:
            pass

print('%s: %d failure(s)' % ('sratetest', len(FAIL)), flush=True)
sys.exit(1 if FAIL else 0)
