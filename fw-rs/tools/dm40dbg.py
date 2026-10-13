#!/usr/bin/env python3
"""Host side of the dm40-fw debug link (USB CDC-ACM, VID:PID 1209:0001).

    dm40dbg.py ping
    dm40dbg.py hold [seconds]            keep the watchdog fed while running
    dm40dbg.py fill x y w h rgb565
    dm40dbg.py lcdreg cmd nwords         send an LCD command, read words back
    dm40dbg.py lcdread x y w h nwords    raw GRAM read-back (dummy word first)
    dm40dbg.py shot out.png [--format rgb565|rgb666]
    dm40dbg.py release                   stop feeding -> stock fw in ~20 s
    dm40dbg.py reset                     reset to stock now

Protocol: see fw-rs/src/host.rs.
"""
import argparse
import struct
import sys
import time

import serial
import serial.tools.list_ports

VID, PID = 0x1209, 0x0001
MAGIC = b'\xd4\x0d'
PING, FILL, LCD_READ, LCD_REG, RELEASE = range(5)
W, H = 480, 320


def find_port():
    for p in serial.tools.list_ports.comports():
        if p.vid == VID and p.pid == PID:
            return p.device
    sys.exit('dm40-fw USB port not found (is the RAM firmware running and the cable in?)')


class Link:
    def __init__(self, port=None):
        self.s = serial.Serial(port or find_port(), timeout=3)
        self.s.reset_input_buffer()

    def req(self, cmd, a=0, b=0, c=0):
        self.s.write(MAGIC + bytes([cmd, 0]) + struct.pack('<3I', a, b, c))
        hdr = self.s.read(8)
        if len(hdr) != 8 or hdr[:2] != MAGIC or hdr[2] != cmd:
            raise IOError('bad reply header: %s' % hdr.hex(' '))
        status, n = hdr[3], struct.unpack_from('<I', hdr, 4)[0]
        data = self.s.read(n)
        if len(data) != n:
            raise IOError('short reply: %d of %d bytes' % (len(data), n))
        if status:
            raise IOError('device status %d' % status)
        return data

    def ping(self, keepalive_ms=0):
        up, flags, left, cid = struct.unpack('<4I', self.req(PING, keepalive_ms))
        return dict(uptime_ms=up, pllsai=bool(flags & 1), usb_core=bool(flags & 2),
                    keepalive_left_ms=left, usb_cid='0x%08x' % cid)

    def lcdread(self, x, y, w, h, nwords):
        d = self.req(LCD_READ, x | y << 16, w | h << 16, nwords)
        return list(struct.unpack('<%dH' % nwords, d))


def rect(x, y, w, h):
    return x | y << 16, w | h << 16


def decode(words, fmt, npix):
    """Raw RAMRD words (dummy dropped) -> list of (r, g, b) 8-bit."""
    px = []
    if fmt == 'rgb565':
        for v in words[:npix]:
            px.append(((v >> 8) & 0xF8, (v >> 3) & 0xFC, (v << 3) & 0xF8))
    else:  # rgb666: one byte per component, packed big-endian into the words
        b = b''.join(struct.pack('>H', v) for v in words)
        for i in range(npix):
            r, g, bl = b[3 * i:3 * i + 3]
            px.append((r & 0xFC, g & 0xFC, bl & 0xFC))
    return px


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--port')
    sub = ap.add_subparsers(dest='cmd', required=True)
    sub.add_parser('ping')
    p = sub.add_parser('hold'); p.add_argument('seconds', type=float, nargs='?', default=0)
    p = sub.add_parser('fill')
    for k in 'x y w h color'.split():
        p.add_argument(k, type=lambda v: int(v, 0))
    p = sub.add_parser('lcdreg'); p.add_argument('reg', type=lambda v: int(v, 0)); p.add_argument('n', type=int)
    p = sub.add_parser('lcdread')
    for k in 'x y w h n'.split():
        p.add_argument(k, type=int)
    p = sub.add_parser('shot'); p.add_argument('out')
    p.add_argument('--format', choices=['rgb565', 'rgb666'], default='rgb666')
    sub.add_parser('release')
    sub.add_parser('reset')
    a = ap.parse_args()

    ln = Link(a.port)
    if a.cmd == 'ping':
        print(ln.ping())
    elif a.cmd == 'hold':
        t_end = time.time() + a.seconds if a.seconds else None
        while t_end is None or time.time() < t_end:
            print(ln.ping(), flush=True)
            time.sleep(5)
    elif a.cmd == 'fill':
        ln.req(FILL, *rect(a.x, a.y, a.w, a.h), a.color)
    elif a.cmd == 'lcdreg':
        print(' '.join('%04x' % v for v in struct.unpack('<%dH' % a.n, ln.req(LCD_REG, a.reg, a.n))))
    elif a.cmd == 'lcdread':
        print(' '.join('%04x' % v for v in ln.lcdread(a.x, a.y, a.w, a.h, a.n)))
    elif a.cmd == 'shot':
        from PIL import Image
        img = Image.new('RGB', (W, H))
        per_px = 1 if a.format == 'rgb565' else 1.5
        t0 = time.time()
        for y in range(0, H, 8):  # bands keep each reply short
            n = int(W * 8 * per_px + 0.5)
            words = ln.lcdread(0, y, W, 8, 1 + n)[1:]
            for i, c in enumerate(decode(words, a.format, W * 8)):
                img.putpixel((i % W, y + i // W), c)
        img.save(a.out)
        print('saved %s in %.1f s' % (a.out, time.time() - t0))
    elif a.cmd == 'release':
        ln.req(RELEASE, 0)
    elif a.cmd == 'reset':
        ln.req(RELEASE, 1)


if __name__ == '__main__':
    main()
