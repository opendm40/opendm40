#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""DM40 stock remote-debug tool (debug firmware variant dm40_dbg.bin).

Talks to a DM40 running the DEBUG patch (firmware/patch/build-dbg/dm40_dbg.bin)
over Bluetooth RFCOMM. It is a strict superset of dm40_memrw.py: the same
read/write/dump, plus LCD screenshot read-back (BT cmd 0x0B sub 3/4) and
memory-browsing helpers. Connection and auto-discovery are reused from
dm40_memrw.py (same directory).

Because the stock firmware pumps the Bluetooth parser ONLY from the multimeter
page loop, every command here - memory reads AND screenshots - is answered
only while the meter shows the MULTIMETER (DMM) page. `shot` therefore
captures the DMM page. Capturing the scope/generator pages needs the
experimental scope-page build (see firmware/patch/README).

Screenshots read the ILI9488 GRAM in the panel's native 320x480 DRIVER space;
the stock GUI draws the 480x320 landscape view onto it column-major, so the
default output transposes driver->landscape. If the orientation looks wrong on
your unit, use --driver to see the raw driver image and --rot/--flipx/--flipy
to correct it (it is a one-time, per-firmware choice).

READ-ONLY by default (write/call need dm40_memrw.py --unsafe). Screenshots and
memory reads never modify the device.

Usage:
  python3 -I dm40_stockdbg.py shot dmm.png
  python3 -I dm40_stockdbg.py shot area.png --rect 400 0 80 320 --raw
  python3 -I dm40_stockdbg.py read 0x20027C74 11
  python3 -I dm40_stockdbg.py hexdump 0x20025A20 32
  python3 -I dm40_stockdbg.py peek-struct meas
  python3 -I dm40_stockdbg.py peek-struct names
  python3 -I dm40_stockdbg.py tap 240 160            # needs scope-page build
"""
import argparse
import importlib.util
import os
import struct
import sys
import time
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location(
    'dm40_memrw', os.path.join(HERE, 'dm40_memrw.py'))
rw = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(rw)

# Debug sub-commands (requests use the same AF 05 03 0B framing as memrw).
SUB_LCD_RAW = 3
SUB_LCD_RLE = 4
SUB_INJECT = 5          # scope-page build: program a touch-injection sequence
SUB_ARM = 6             # scope-page build: arm/disarm the scope hook

# Panel driver geometry (ILI9488 as the stock configures it: portrait).
DRV_W = 320
DRV_H = 480
RAW_MAX = 125           # pixels per sub-3 reply (2*125 = 250 data bytes)


def lcd_req(sub, dx, dy, n):
    """A sub-3/4 request: addr carries (dy<<16)|dx, n = pixel count."""
    addr = ((dy & 0xFFFF) << 16) | (dx & 0xFFFF)
    return rw.pkt(sub, addr, n)


class DbgDev(rw.Dev):
    """memrw Dev + LCD read-back and variable-length replies."""

    def _resp_var(self):
        """Read an AF 05 03 0C n <data> CRC reply of unknown length."""
        hdr = b''
        while len(hdr) < 5:
            r = self.s.recv(5 - len(hdr))
            if not r:
                raise rw.DeviceError('connection closed by device')
            hdr += r
        if (hdr[0] != 0xAF or hdr[1] != rw.ID[0] or hdr[2] != rw.ID[1]
                or hdr[3] != rw.RESP_CMD):
            raise rw.DeviceError('bad reply header: %s' % hdr.hex(' '))
        n = hdr[4]
        body = b''
        while len(body) < n + 1:
            r = self.s.recv(n + 1 - len(body))
            if not r:
                raise rw.DeviceError('connection closed by device')
            body += r
        data, crc = body[:n], body[n]
        if (sum(hdr) + sum(data) + crc) & 0xFF != 0:
            raise rw.DeviceError('bad reply checksum')
        return data

    def lcd_span_raw(self, dx, dy, n):
        """Read n (<=125) pixels -> list of RGB565 ints."""
        self.s.send(lcd_req(SUB_LCD_RAW, dx, dy, min(n, RAW_MAX)))
        data = self._resp_var()
        return list(struct.unpack('<%dH' % (len(data) // 2), data))

    def lcd_row_rle(self, dx, dy, n):
        """Read n pixels with on-device RLE. Returns (consumed, [RGB565])."""
        self.s.send(lcd_req(SUB_LCD_RLE, dx, dy, min(n, 255)))
        data = self._resp_var()
        consumed = data[0]
        px = []
        i = 1
        while i + 2 < len(data) + 1 and i + 2 <= len(data):
            cnt = data[i]
            col = data[i + 1] | (data[i + 2] << 8)
            px.extend([col] * cnt)
            i += 3
        return consumed, px

    def lcd_region(self, dx0, dy0, w, h, raw=False, progress=None):
        """Read a DRIVER-space rectangle -> list of rows, each w RGB565 ints."""
        rows = []
        for dy in range(dy0, dy0 + h):
            row = []
            x = dx0
            while x < dx0 + w:
                if raw:
                    row.extend(self.lcd_span_raw(x, dy, dx0 + w - x))
                    x += min(dx0 + w - x, RAW_MAX)
                else:
                    consumed, px = self.lcd_row_rle(x, dy, dx0 + w - x)
                    if consumed == 0:            # safety: avoid an infinite loop
                        raise rw.DeviceError('device made no progress at '
                                             '(%d,%d)' % (x, dy))
                    row.extend(px[:consumed])
                    x += consumed
            rows.append(row[:w])
            if progress:
                progress(dy - dy0 + 1, h)
        return rows


# --- RGB565 -> 8-bit and PNG (stdlib only) ---------------------------------
def rgb565_to_888(c):
    r = (c >> 11) & 0x1F
    g = (c >> 5) & 0x3F
    b = c & 0x1F
    return (r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)


def write_png(path, rows888):
    h = len(rows888)
    w = len(rows888[0]) if h else 0
    raw = bytearray()
    for row in rows888:
        raw.append(0)                       # filter type 0
        for (r, g, b) in row:
            raw += bytes((r, g, b))

    def chunk(tag, data):
        c = struct.pack('>I', len(data)) + tag + data
        return c + struct.pack('>I', zlib.crc32(tag + data) & 0xFFFFFFFF)

    ihdr = struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)   # 8-bit RGB
    with open(path, 'wb') as f:
        f.write(b'\x89PNG\r\n\x1a\n')
        f.write(chunk(b'IHDR', ihdr))
        f.write(chunk(b'IDAT', zlib.compress(bytes(raw), 9)))
        f.write(chunk(b'IEND', b''))


def orient(driver_rows, args):
    """driver_rows[dy][dx] (320 wide x 480 tall) -> output pixel rows."""
    h = len(driver_rows)
    w = len(driver_rows[0]) if h else 0
    if args.driver:
        img = driver_rows                    # raw 320x480, no transpose
    else:
        # landscape: out[gy][gx] = driver[gx][gy]  (column-major mapping)
        img = [[driver_rows[gx][gy] for gx in range(h)] for gy in range(w)]
    for _ in range(args.rot // 90 % 4):
        img = [list(r) for r in zip(*img[::-1])]   # rotate 90 CW
    if args.flipx:
        img = [r[::-1] for r in img]
    if args.flipy:
        img = img[::-1]
    return img


# --- named RAM symbols (from firmware/re/analysis/*.h) ----------------------
SYMBOLS = {
    'meas':      (0x20027C74, 11, 'DMM measurement snapshot (BT cmd 9 packet)'),
    'touch':     (0x2002561C, 32, 'touch driver state block (queue ring base)'),
    'lcd':       (0x200255F0, 16, 'LCD driver state: width,height,orient,cmds'),
    'scope_ptr': (0x2002036C, 8,  'scope engine S pointer at +4'),
    'settings':  (0x20028E58, 32, 'settings/calibration RAM copy'),
    'model':     (0x20028FC0, 1,  'DMM model byte (0=A,1=B,2=C)'),
    'bthandler': (0x2002004C, 16, 'BT command handler table (RAM)'),
}


def hexdump(data, base):
    out = []
    for i in range(0, len(data), 16):
        chunk = data[i:i + 16]
        hexs = ' '.join('%02x' % b for b in chunk)
        asci = ''.join(chr(b) if 32 <= b < 127 else '.' for b in chunk)
        out.append('%08x  %-47s  %s' % (base + i, hexs, asci))
    return '\n'.join(out)


# --- commands --------------------------------------------------------------
def do_shot(dev, args):
    if args.rect:
        dx, dy, w, h = args.rect
    else:
        dx, dy, w, h = 0, 0, DRV_W, DRV_H
    t0 = time.time()
    nbytes = [0]

    def prog(done, total):
        if done % 40 == 0 or done == total:
            sys.stderr.write('\r  row %d/%d' % (done, total))
            sys.stderr.flush()

    rows = dev.lcd_region(dx, dy, w, h, raw=args.raw, progress=prog)
    sys.stderr.write('\n')
    img = orient(rows, args)
    img888 = [[rgb565_to_888(c) for c in row] for row in img]
    write_png(args.out, img888)
    dt = time.time() - t0
    print('wrote %s  (%dx%d, %.1fs, %s)'
          % (args.out, len(img888[0]) if img888 else 0, len(img888), dt,
             'raw' if args.raw else 'RLE'))


def do_read(dev, args):
    data = dev.read(args.addr, args.length)
    if args.out:
        open(args.out, 'wb').write(data)
        print('wrote %d bytes to %s' % (len(data), args.out))
    else:
        sys.stdout.buffer.write(data)


def do_hexdump(dev, args):
    data = dev.read(args.addr, args.length)
    print(hexdump(data, args.addr))


def do_peek(dev, args):
    if args.name in ('names', 'list'):
        for k, (a, n, desc) in sorted(SYMBOLS.items()):
            print('%-10s 0x%08x  %3d B  %s' % (k, a, n, desc))
        return
    if args.name not in SYMBOLS:
        sys.exit('unknown symbol %r; try: peek-struct names' % args.name)
    a, n, desc = SYMBOLS[args.name]
    data = dev.read(a, n)
    print('%s @ 0x%08x  (%s)' % (args.name, a, desc))
    print(hexdump(data, a))
    if args.name == 'meas' and len(data) >= 11:
        mode, flags = data[0], data[1]
        vals = struct.unpack('<3H', data[5:11])
        print('  mode=0x%02x flags=0x%02x (HOLD=%d REL=%d) values=%s'
              % (mode, flags, bool(flags & 0x80), bool(flags & 0x20),
                 ['0x%04x' % v for v in vals]))
    if args.name == 'model' and data:
        print('  model = %s' % {0: 'A', 1: 'B', 2: 'C'}.get(data[0], '?'))


# Touch-queue entry flags (firmware/re/analysis/touch_v2_6.h):
#   bit0..2 n_pressed, bit3..4 key code, bit5 key report, bit6 any pressed,
#   bit7..9 n_reported. A screen point down = n_pressed 1, bit6, n_reported 1.
PRESS_FLAGS = 0x01 | 0x40 | 0x80            # 0xC1
RELEASE_FLAGS = 0x0000
INJ_MAX = 20                                # keep the request <= 0x97 bytes


def _program(dev, entries):
    """Program a touch-injection sequence (cmd 0x0B sub 5): the scope hook
    plays one entry per ~100 ms poll while armed. entries = [(flags,x,y)]."""
    entries = entries[:INJ_MAX]
    payload = bytes([len(entries)])
    for fl, x, y in entries:
        payload += struct.pack('<Hhh', fl & 0xFFFF, int(x), int(y))
    dev.s.send(rw.pkt(SUB_INJECT, 0, len(payload), payload))
    return dev._resp_var()                  # reply: 1 byte = accepted count


def _arm(dev, on):
    dev.s.send(rw.pkt(SUB_ARM, 0, 1, bytes([1 if on else 0])))
    return dev._resp_var()[0]               # 1 = armed, 0 = disarmed


def do_enable_scope(dev, args):
    st = _arm(dev, True)
    print('scope debug %s (send this on the DMM page)'
          % ('ARMED' if st else 'FAILED (plain image ignores it)'))


def do_disable_scope(dev, args):
    _arm(dev, False)
    print('scope debug disarmed')


def do_tap(dev, args):
    n = _program(dev, [(PRESS_FLAGS, args.x, args.y),
                       (RELEASE_FLAGS, args.x, args.y)])
    print('tap (%d,%d) programmed (%d entries; plays while scope page is open)'
          % (args.x, args.y, n[0] if n else 0))


def do_drag(dev, args):
    steps = max(1, args.steps)
    pts = [(PRESS_FLAGS, args.x0, args.y0)]
    for k in range(1, steps):
        t = k / float(steps)
        pts.append((PRESS_FLAGS, round(args.x0 + (args.x1 - args.x0) * t),
                    round(args.y0 + (args.y1 - args.y0) * t)))
    pts.append((PRESS_FLAGS, args.x1, args.y1))
    pts.append((RELEASE_FLAGS, args.x1, args.y1))
    n = _program(dev, pts)
    print('drag (%d,%d)->(%d,%d) programmed (%d entries)'
          % (args.x0, args.y0, args.x1, args.y1, n[0] if n else 0))


def do_key(dev, args):
    # side key as a key report: bit5 + code in bits3..4; down then up.
    down = 0x20 | ((args.code & 3) << 3)
    holds = max(1, (args.hold + 99) // 100)          # ~100 ms per played entry
    pts = [(down, 0, 0)] * holds + [(RELEASE_FLAGS, 0, 0)]
    n = _program(dev, pts)
    print('key %d programmed (%d entries)' % (args.code, n[0] if n else 0))


def do_swipe(dev, args):
    # a single-finger drag across the 480x320 landscape screen
    routes = {'left': (380, 160, 100, 160), 'right': (100, 160, 380, 160),
              'down': (240, 60, 240, 260)}
    x0, y0, x1, y1 = routes[args.dir]
    args.x0, args.y0, args.x1, args.y1, args.steps = x0, y0, x1, y1, 8
    do_drag(dev, args)


def build_parser():
    p = argparse.ArgumentParser(
        description='DM40 stock remote-debug (screenshots + memory browse).',
        formatter_class=argparse.RawDescriptionHelpFormatter, epilog=__doc__)
    p.add_argument('--mac', default=os.environ.get('DM40_MAC'),
                   help='meter Bluetooth MAC (auto on Linux via bluetoothctl)')
    p.add_argument('--rescan', action='store_true', help='ignore cached MAC')
    p.add_argument('--timeout', type=float, default=5.0)
    sub = p.add_subparsers(dest='cmd', required=True)

    s = sub.add_parser('shot', help='screenshot the DMM page to PNG')
    s.add_argument('out')
    s.add_argument('--raw', action='store_true', help='no RLE (for noisy areas)')
    s.add_argument('--rect', type=int, nargs=4, metavar=('DX', 'DY', 'W', 'H'),
                   help='DRIVER-space rectangle (default full 320x480)')
    s.add_argument('--driver', action='store_true',
                   help='save the raw 320x480 driver image (no transpose)')
    s.add_argument('--rot', type=int, default=0, choices=[0, 90, 180, 270])
    s.add_argument('--flipx', action='store_true')
    s.add_argument('--flipy', action='store_true')
    s.set_defaults(func=do_shot)

    s = sub.add_parser('read', help='read memory (safe)')
    s.add_argument('addr', type=lambda x: int(x, 0))
    s.add_argument('length', type=lambda x: int(x, 0))
    s.add_argument('-o', '--out')
    s.set_defaults(func=do_read)

    s = sub.add_parser('hexdump', help='read + hexdump memory (safe)')
    s.add_argument('addr', type=lambda x: int(x, 0))
    s.add_argument('length', type=lambda x: int(x, 0))
    s.set_defaults(func=do_hexdump)

    s = sub.add_parser('peek-struct', help='read a named stock RAM structure')
    s.add_argument('name', help='symbol name, or "names" to list them')
    s.set_defaults(func=do_peek)

    sub.add_parser('enable-scope',
                   help='arm the scope-page hook (send on the DMM page)'
                   ).set_defaults(func=do_enable_scope)
    sub.add_parser('disable-scope', help='disarm the scope-page hook'
                   ).set_defaults(func=do_disable_scope)

    s = sub.add_parser('tap', help='tap at landscape (x,y); scope page, armed')
    s.add_argument('x', type=int); s.add_argument('y', type=int)
    s.set_defaults(func=do_tap)

    s = sub.add_parser('drag', help='drag (x0,y0)->(x1,y1); scope page, armed')
    for k in 'x0 y0 x1 y1'.split():
        s.add_argument(k, type=int)
    s.add_argument('--steps', type=int, default=6, help='intermediate points')
    s.set_defaults(func=do_drag)

    s = sub.add_parser('key'); s.add_argument('code', type=int,
                                              help='0 MENU 1 RUN 2 AUTO 3 MF')
    s.add_argument('--hold', type=int, default=0)
    s.set_defaults(func=do_key)

    s = sub.add_parser('swipe'); s.add_argument('dir',
                                               choices=['left', 'right', 'down'])
    s.add_argument('--fingers', type=int, default=1)
    s.set_defaults(func=do_swipe)
    return p


def main(argv=None):
    args = build_parser().parse_args(argv)
    args.mac = rw.resolve_mac(args)
    dev = DbgDev(args.mac, args.timeout)
    try:
        args.func(dev, args)
    finally:
        dev.close()


if __name__ == '__main__':
    main()
