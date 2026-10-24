#!/usr/bin/env python3
"""opendm40 on-device screenshot (src/screenshot.rs, 3-finger swipe down) in the
emulator: the BMP the firmware writes to the SPI disk must be the panel's
frame at the gesture, pixel for pixel, read back through the ILI9488 model's
RAMRD (emu/peripherals/ILI9488.cs: dummy word, RGB666 packed 2 pixels per 3
words, as the real panel; the device itself must still be checked).

  cd firmware && ./build.sh
  emu/shottest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 [--out DIR]

Per screen (DMM DC V with HOLD on; the Settings menu; the combined DMM +
scope view, scope STOPped):
  * the panel read over DBG:LCD? before the gesture (RGB666 -> RGB565 as
    lcd.rs rgb666_to_565) is the reference (HOLD / a menu / a stopped
    scope keep it still until the gesture); it is read again after the
    shot (how much the screen changed afterwards, for information);
  * DBG:SWIPE D,3, then MMEM:CAT? until the next SHOTS/Snnn.BMP is listed;
    the file is read back with MMEM:DATA? (6000-byte pieces);
  * the BMP: the stock 66-byte header (480x320, 16 bpp BI_BITFIELDS
    F800/07E0/001F, bottom-up rows), 307266 bytes, its pixels equal the
    reference;
  * the first shot's name with another directory's prefix (LOGS/Snnn.BMP):
    MMEM:DATA? and MMEM:DELete answer -256 "File name not found" and the
    shot stays listed (device 46370df: the prefix was dropped, not checked);
  * --out: the BMPs and the reference frames are kept there (<name>.bmp,
    <name>.lcd565), so two builds can be compared byte for byte
    (cmp DIR1/x.bmp DIR2/x.bmp).
The SPI flash has the W25Q128JV's timing (emu/peripherals/W25Q128.cs busy
model; default tPP 0.7 ms per page program, tSE 45 ms per 4 KiB erase,
--tpp / --tse), so the screen-frozen time of a shot is realistic. Per shot
the firmware's own figures are printed (DBG:FLASH? fields 4..7,
screenshot.rs `stats`): frozen ms, total ms, slices while frozen, data blocks
that needed an erase; --max-freeze MS fails a shot frozen longer.
Builds with the idle-time preparation (screenshot.rs `prep`, DBG:FLASH?
fields 8..) are first waited for (at most 60 s virtual) until the next
shot's clusters are erased, unless --no-prep-wait; the screen "again" is a
second shot of the last screen right after the previous one (the erased
count shows how much of the preparation had happened by then; with this
test's ~10 s of verify wait and panel read-back, usually all of it).
The emulator runs on a private copy of the SPI disk image (per process:
port + pid). Exit status 0 = all passed.
"""
import argparse
import os
import shutil
import struct
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF')
ap.add_argument('--port', type=int, default=17310, help='USB/IP port (default 17310)')
ap.add_argument('--out', help='keep the BMPs and reference frames here')
ap.add_argument('--mips', default='200')
ap.add_argument('--screens', default='dmm,menu,scope', help='dmm, menu, scope, again (comma-separated)')
ap.add_argument('--tpp', type=int, default=700, help='flash page program time, us (default 700)')
ap.add_argument('--tse', type=int, default=45000, help='flash 4 KiB erase time, us (default 45000)')
ap.add_argument('--max-freeze', type=int, default=0, metavar='MS', help='fail a shot frozen longer (0: report only)')
ap.add_argument('--no-prep-wait', action='store_true', help='do not wait for the idle-time preparation')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, HERE)
import usbip_client as uc  # noqa: E402

FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


OUT = os.path.join(HERE, 'out')
os.makedirs(OUT, exist_ok=True)
tag = 'shot-%d-%d' % (A.port, os.getpid())
disk = os.path.join(OUT, '%s.disk.bin' % tag)
shutil.copy(os.path.join(ROOT, 'stock/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin'), disk)
log = os.path.join(OUT, '%s.log' % tag)
script = os.path.join(OUT, '%s.script' % tag)
with open(script, 'w') as f:
    f.write('0 monitor sysbus.spi2.spiflash ProgramUs %d\n' % A.tpp)
    f.write('0 monitor sysbus.spi2.spiflash Erase4kUs %d\n' % A.tse)
    f.write('0 monitor sysbus.spi2.spiflash Erase32kUs %d\n' % (A.tse * 120 // 45))
    f.write('0 monitor sysbus.spi2.spiflash Erase64kUs %d\n' % (A.tse * 150 // 45))
cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '3000', '--name', tag, '--log', log,
       '--disk', disk, '--script', script]
if A.out:
    os.makedirs(A.out, exist_ok=True)
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)

HDR = bytes([
    0x42, 0x4D, 0x42, 0xB0, 0x04, 0x00, 0, 0, 0, 0, 0x42, 0, 0, 0,
    0x28, 0, 0, 0, 0xE0, 0x01, 0, 0, 0x40, 0x01, 0, 0, 0x01, 0, 0x10, 0,
    0x03, 0, 0, 0, 0x00, 0xB0, 0x04, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0x00, 0xF8, 0, 0, 0xE0, 0x07, 0, 0, 0x1F, 0, 0, 0])
assert len(HDR) == 66

try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('shottest: no USB/IP device on port %d' % A.port)

    def q(line, raw=False, timeout=60):
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
            time.sleep(0.2)

    def block(b):
        nd = int(b[1:2])
        return b[2 + nd:2 + nd + int(b[2:2 + nd])]

    def lcd565():
        """The panel as RGB565 rows (top first), lcd.rs read_span's decode."""
        rows = 4
        px = []
        for y in range(0, 320, rows):
            n = 480 * rows * 3 // 2 + 1
            d = block(q('DBG:LCD? 0,%d,480,%d,%d' % (y, rows, n), raw=True))
            w = [d[i] | d[i + 1] << 8 for i in range(2, len(d), 2)]  # word 0 = the dummy
            bs = bytearray()
            for v in w:
                bs += bytes([v >> 8, v & 0xFF])
            for i in range(480 * rows):
                r, g, b = bs[3 * i], bs[3 * i + 1], bs[3 * i + 2]
                px.append((r & 0xF8) << 8 | (g & 0xFC) << 3 | b >> 3)
        return px

    def shots():
        cat = q('MMEM:CAT?', timeout=30)
        return {f.split(',')[0].strip('"') for f in cat.split('"')[1::2] if '.BMP' in f.upper()}

    def fetch(name, size):
        out = bytearray()
        while len(out) < size:
            d = block(q('MMEM:DATA? "%s",%d,6000' % (name, len(out)), raw=True, timeout=30))
            if not d:
                break
            out += d
        return bytes(out)

    print('*IDN?', q('*IDN?'), flush=True)
    wait_ms(2500)

    def setup(name):
        if name == 'dmm':
            q('CONF:VOLT:DC')
            wait_ms(2500)
            q('DBG:KEY 1')  # RUN = HOLD in a DMM view
            wait_ms(1500)
        elif name == 'menu':
            q('DBG:KEY 1')  # HOLD off
            wait_ms(800)
            q('DBG:KEY 0')  # MENU
            wait_ms(1500)
        elif name == 'scope':
            q('DBG:KEY 0')  # MENU closed
            wait_ms(800)
            q('DBG:SWIPE L')  # the combined DMM + scope view
            wait_ms(1500)
            q('STOP')  # the scope trace stops (the DMM HOLD is still on)
            wait_ms(1500)

    def flash():
        return [int(float(x)) for x in q('DBG:FLASH?').split(',')[1:]]

    def prep_wait():
        f = flash()
        if len(f) < 8 or A.no_prep_wait:
            return
        t0 = up()
        while up() - t0 < 60000:
            f = flash()
            if f[6] == 2:
                break
            time.sleep(0.5)
        print('  prep: state %d, %d clusters ready, %d erases so far (waited %.1f s virtual)'
              % (f[6], f[7], f[8], (up() - t0) / 1000), flush=True)

    print('flash timing: tPP %d us, tSE %d us' % (A.tpp, A.tse), flush=True)
    FREEZE = []
    DIRS_DONE = []
    for name in A.screens.split(','):
        print('--- %s' % name, flush=True)
        if name != 'again':
            setup(name)
            prep_wait()
        before = shots()
        ref0 = lcd565()
        t0 = up()
        q('DBG:SWIPE D,3')
        new = set()
        while up() - t0 < 60000:
            time.sleep(0.5)
            new = shots() - before
            if new:
                break
        check(len(new) == 1, 'a new shot listed (%s, %.1f s virtual)' % (sorted(new), (up() - t0) / 1000))
        if not new:
            continue
        fn = new.pop()
        wait_ms(1500)  # the verify pass
        f = flash()
        print('  frozen %d ms, shot %d ms, %d slices frozen, %d of 76 data blocks erased' % tuple(f[2:6]), flush=True)
        FREEZE.append((name, f[2], f[5]))
        if A.max_freeze:
            check(f[2] <= A.max_freeze, '%s: frozen %d ms <= %d ms' % (name, f[2], A.max_freeze))
        if not DIRS_DONE:
            DIRS_DONE.append(fn)
            other = 'LOGS/' + fn.split('/')[-1]
            q('*CLS')  # MMEM:CAT? polls during the shot queue -250 "Mass storage busy"
            t.write('MMEM:DATA? "%s",0,100' % other)
            try:
                r = t.read(maxlen=8192, timeout=10)
            except Exception:
                r = b''
            e = q('SYST:ERR?')
            check(not r.startswith(b'#') and e.startswith('-256,'), 'MMEM:DATA? "%s": no data (%r), %s' % (other, r[:16], e))
            q('MMEM:DEL "%s"' % other)
            e = q('SYST:ERR?')
            check(e.startswith('-256,') and fn in shots(), 'MMEM:DEL "%s": %s, %s still listed' % (other, e, fn))
        ref1 = lcd565()
        bmp = fetch(fn, 307266)
        check(len(bmp) == 307266 and bmp[:66] == HDR, '%s: 307266 bytes, the stock header' % fn)
        pix = []
        for r in range(319, -1, -1):  # bottom-up rows
            o = 66 + r * 960
            pix += list(struct.unpack('<480H', bmp[o:o + 960]))
        d1 = sum(a != b for a, b in zip(ref0, ref1))
        diff = sum(a != b for a, b in zip(pix, ref0))
        check(diff == 0, '%s: pixels = the panel read before the gesture (%d differ; the panel changed by %d px after the shot)'
              % (fn, diff, d1))
        if A.out and name != 'again':
            open(os.path.join(A.out, name + '.bmp'), 'wb').write(bmp)
            open(os.path.join(A.out, name + '.lcd565'), 'wb').write(struct.pack('<%dH' % len(ref0), *ref0))
    print('freeze: ' + ', '.join('%s %d ms (%d erased)' % x for x in FREEZE), flush=True)
    err = q('SYST:ERR?')
    print('  SYST:ERR? %s' % err, flush=True)
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    emu.wait()
    try:
        os.remove(disk)
    except Exception:
        pass

print('FAIL: %d' % len(FAIL) if FAIL else 'PASS')
sys.exit(1 if FAIL else 0)
