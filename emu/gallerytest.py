#!/usr/bin/env python3
"""fw-rs gallery (src/gallery.rs, region C) end to end in the emulator:
open, page scroll, the full-screen viewer, file steps, the Delete dialog
(Cancel, then Delete), back, driven like a user (DBG:KEY / DBG:TAP /
DBG:SWIPE) and checked over SCPI (MMEM:CAT?, DBG:LCD?).

  cd firmware && ./build.sh
  emu/gallerytest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

The disk is a private copy (per process: port + pid) of the 2026-10-13 MSC
dump (S000..S002.BMP, 480 x 320 16 bpp) with S003..S007.BMP added (copies
of S000..S002): 8 files = two grid pages. Checks:
  1. MENU -> the Gallery row: the grid's title bar, a thumbnail in tile 0
     (not the empty thumbnail background) on page 1;
  2. swipe left: page 2 holds 2 tiles (tile 2 is the bare page background),
     swipe right: page 1 again;
  3. tile 0 -> viewer: the whole screen equals one of the BMP files (RGB565
     vs the LCD's RGB666 read-back, top bits), swipe left: the next file,
     equal to a BMP too;
  4. tap = overlay bar, Delete -> dialog -> Cancel: still 8 files; Delete ->
     dialog -> Delete: 7 files (MMEM:CAT?), the viewer shows the next BMP
     (below the overlay bar, which stays);
  5. MENU: the grid; MENU: the gallery is left (*IDN? answers, SYST:ERR? 0).
--shots DIR saves PNG screenshots. Exit status 0 = all passed.
"""
import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='fw-rs native ELF')
ap.add_argument('--port', type=int, default=19310, help='USB/IP port (default 19310)')
ap.add_argument('--shots', help='directory for PNG screenshots')
ap.add_argument('--mips', default='200')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, HERE)
import usbip_client as uc  # noqa: E402

if uc.port_busy(A.port):
    sys.exit('gallerytest: port %d is busy' % A.port)

FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


OUT = os.path.join(HERE, 'out')
os.makedirs(OUT, exist_ok=True)
tag = 'gallery-%d-%d' % (A.port, os.getpid())
disk = os.path.join(OUT, '%s.disk.bin' % tag)
shutil.copy(os.path.join(ROOT, 'stock/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin'), disk)
tmp = tempfile.mkdtemp(prefix=tag)
env = dict(os.environ, MTOOLS_SKIP_CHECK='1')
BMPS = {}
for i in range(3):
    p = os.path.join(tmp, 'S%03d.BMP' % i)
    subprocess.run(['mcopy', '-i', disk, '::S%03d.BMP' % i, p], check=True, env=env)
    BMPS[i] = open(p, 'rb').read()
for i in range(3, 8):
    subprocess.run(['mcopy', '-i', disk, os.path.join(tmp, 'S%03d.BMP' % (i % 3)), '::S%03d.BMP' % i],
                   check=True, env=env)


def bmp565(b):
    """Top-down rows of RGB565 words of a 480 x 320 16 bpp BMP (565 bitfields)."""
    off = int.from_bytes(b[10:14], 'little')
    w, h = int.from_bytes(b[18:22], 'little'), int.from_bytes(b[22:26], 'little', signed=True)
    bpp, comp = int.from_bytes(b[28:30], 'little'), int.from_bytes(b[30:34], 'little')
    assert (w, abs(h), bpp, comp) == (480, 320, 16, 3), (w, h, bpp, comp)
    stride = (w * 2 + 3) & ~3
    rows = []
    for y in range(320):
        r = y if h < 0 else 319 - y
        o = off + r * stride
        rows.append([b[o + 2 * x] | b[o + 2 * x + 1] << 8 for x in range(w)])
    return rows


REF = [bmp565(BMPS[i]) for i in range(3)]

log = os.path.join(OUT, '%s.log' % tag)
cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '3000', '--name', tag, '--log', log,
       '--disk', disk]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)

try:
    t = None
    for _ in range(120):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('gallerytest: no USB/IP device on port %d' % A.port)

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

    def screen():
        """The panel as 320 rows of 480 (r, g, b) RGB666 read-back bytes."""
        rows = 4
        out = []
        for y in range(0, 320, rows):
            n = 480 * rows * 3 // 2 + 1
            b = q('DBG:LCD? 0,%d,480,%d,%d' % (y, rows, n), raw=True)
            nd = int(b[1:2])
            d = b[2 + nd:2 + nd + int(b[2:2 + nd])]
            w = [d[i] | d[i + 1] << 8 for i in range(2, len(d), 2)]
            bs = bytearray()
            for v in w:
                bs += bytes([v >> 8, v & 0xFF])
            for r in range(rows):
                o = r * 480 * 3
                out.append([tuple(bs[o + 3 * x:o + 3 * x + 3]) for x in range(480)])
        return out

    def shot(name, s=None):
        if not A.shots:
            return
        from PIL import Image
        s = s or screen()
        os.makedirs(A.shots, exist_ok=True)
        im = Image.new('RGB', (480, 320))
        im.putdata([p for row in s for p in row])
        im.save(os.path.join(A.shots, name + '.png'))

    def is565(px, c):
        return (px[0] >> 3, px[1] >> 2, px[2] >> 3) == (c >> 11, c >> 5 & 63, c & 31)

    def which_bmp(s, y0=0):
        """Index of the reference BMP the screen (rows y0..319) equals (None: none)."""
        for i, ref in enumerate(REF):
            if all(is565(s[y][x], ref[y][x]) for y in range(y0, 320, 3) for x in range(0, 480, 3)):
                if all(is565(s[y][x], ref[y][x]) for y in range(y0, 320) for x in range(480)):
                    return i
        return None

    def n_files():
        c = q('MMEM:CAT?')
        return c.count('.BMP')

    def wait_settled(ms=6000):
        """Until two screen reads 1 s apart are equal (thumbnails / image drawn)."""
        prev = None
        t0 = up()
        while up() - t0 < ms:
            s = screen()
            if s == prev:
                return s
            prev = s
            wait_ms(1000)
        return prev

    BAR, M_BG, IMG_BG = 0x2966, 0x10A3, 0x0000
    print('*IDN?', q('*IDN?'), flush=True)
    wait_ms(2000)
    n0 = n_files()
    check(n0 == 8, 'MMEM:CAT?: 8 BMP files (%d)' % n0)

    # 1. MENU -> Gallery row.
    q('DBG:KEY 0')
    wait_ms(800)
    shot('g00-menu')
    q('DBG:TAP 240,27')
    s = wait_settled()
    shot('g01-grid-p1', s)
    check(is565(s[20][300], BAR), 'grid: title bar colour')
    th = [s[y][x] for y in range(50, 50 + 93, 4) for x in range(14, 14 + 140, 4)]
    check(any(not is565(p, IMG_BG) for p in th), 'grid page 1: tile 0 has a thumbnail')
    check(not is565(s[168 + 58][322 + 74], M_BG), 'grid page 1: tile 5 is drawn (8 files: 6 on page 1)')

    # 2. Page 2 and back.
    q('DBG:SWIPE L')
    s = wait_settled()
    shot('g02-grid-p2', s)
    check(any(not is565(p, IMG_BG) for p in [s[y][x] for y in range(50, 143, 4) for x in range(170, 310, 4)]),
          'grid page 2: tile 1 has a thumbnail')
    check(is565(s[46 + 58][322 + 74], M_BG), 'grid page 2: tile 2 empty (2 files on page 2)')
    q('DBG:SWIPE R')
    s = wait_settled()
    check(not is565(s[168 + 58][322 + 74], M_BG), 'swipe right: page 1 again (tile 5 drawn)')

    # 3. Viewer.
    q('DBG:TAP 84,100')
    s = wait_settled(12000)
    shot('g03-view', s)
    k0 = which_bmp(s)
    check(k0 is not None, 'viewer: the screen equals a BMP file (S%s)' % k0)
    q('DBG:SWIPE L')
    s = wait_settled(12000)
    shot('g04-view-next', s)
    k1 = which_bmp(s)
    check(k1 is not None, 'viewer, swipe left: the next file equals a BMP (S%s)' % k1)

    # 4. Overlay, Delete -> Cancel, Delete -> Delete.
    q('DBG:TAP 240,200')
    wait_ms(1500)
    s = screen()
    shot('g05-overlay', s)
    check(is565(s[20][300], BAR), 'tap: overlay bar shown')
    q('DBG:TAP 420,20')
    wait_ms(1500)
    shot('g06-dialog')
    q('DBG:TAP 170,204')
    wait_ms(2000)
    check(n_files() == 8, 'Delete -> Cancel: still 8 files')
    q('DBG:TAP 240,200')
    wait_ms(1500)
    s = screen()
    if not is565(s[20][300], BAR):  # the overlay toggled off by the tap: once more
        q('DBG:TAP 240,200')
        wait_ms(1500)
    q('DBG:TAP 420,20')
    wait_ms(1500)
    q('DBG:TAP 310,204')
    wait_ms(3000)
    n1 = n_files()
    check(n1 == 7, 'Delete -> Delete: 7 files (%d)' % n1)
    s = wait_settled(12000)
    shot('g07-after-delete', s)
    check(which_bmp(s, 40) is not None, 'after the delete the viewer shows a BMP again (below the overlay bar, which stays)')

    # 5. Back out.
    q('DBG:KEY 0')
    s = wait_settled()
    shot('g08-grid-back', s)
    check(is565(s[20][300], BAR) and is565(s[168 + 58][322 + 74], M_BG) is False,
          'MENU: back to the grid (6 tiles on page 1)')
    q('DBG:KEY 0')
    wait_ms(1500)
    shot('g09-left')
    check(q('*IDN?').startswith('dm40') or ',' in q('*IDN?'), 'MENU again: gallery left, *IDN? answers')
    err = q('SYST:ERR?')
    check(err.startswith('0'), 'SYST:ERR? %s' % err)
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    emu.wait()
    shutil.rmtree(tmp, ignore_errors=True)
    try:
        os.remove(disk)
    except OSError:
        pass

print('gallerytest: %s' % ('PASS' if not FAIL else 'FAIL (%d)' % len(FAIL)))
sys.exit(1 if FAIL else 0)
