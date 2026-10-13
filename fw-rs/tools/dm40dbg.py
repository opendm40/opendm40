#!/usr/bin/env python3
"""Host side of the dm40-fw debug commands (USBTMC, VID:PID 1209:0001).

    dm40dbg.py ping
    dm40dbg.py hold [seconds]            keep the watchdog fed while running
    dm40dbg.py scpi 'CMD' ...            send SCPI, print replies to queries
    dm40dbg.py lcdread x y w h nwords    raw GRAM read-back (dummy word first)
    dm40dbg.py shot out.png [--format rgb565|rgb666]
    dm40dbg.py diag                      diagnostic block (hex)
    dm40dbg.py tap x y | left | right | key 0..3   inject touch / side key
    dm40dbg.py release                   stop feeding -> stock fw in ~20 s
    dm40dbg.py reset                     reset to stock now

Debug commands are SCPI DBG:... (see fw-rs/src/scpi.rs); needs read/write
access to /dev/usbtmcN (udev rule).
"""
import argparse
import glob
import os
import struct
import sys
import time

VID, PID = 0x1209, 0x0001
W, H = 480, 320


def find_dev():
    for d in sorted(glob.glob('/sys/class/usbmisc/usbtmc*')):
        usb = os.path.realpath(os.path.join(d, 'device', '..'))
        try:
            v = int(open(os.path.join(usb, 'idVendor')).read(), 16)
            p = int(open(os.path.join(usb, 'idProduct')).read(), 16)
        except OSError:
            continue
        if (v, p) == (VID, PID):
            return '/dev/' + os.path.basename(d)
    sys.exit('dm40-fw USBTMC device not found (is the firmware running and the cable in?)')


class Link:
    def __init__(self, dev=None):
        self.fd = os.open(dev or find_dev(), os.O_RDWR)

    def write(self, cmd):
        os.write(self.fd, cmd.encode() + b'\n')

    def read(self, n=1 << 16):
        return os.read(self.fd, n)

    def query(self, cmd):
        self.write(cmd)
        return self.read()

    def block(self, cmd):
        """Query returning an IEEE 488.2 definite block (#Nlen...)."""
        d = self.query(cmd)
        if d[:1] != b'#':
            raise IOError('not a block: %r' % d[:32])
        nd = int(d[1:2])
        n = int(d[2:2 + nd])
        d = d[2 + nd:]
        while len(d) < n:
            d += self.read()
        return d[:n]

    def ping(self, keepalive_ms=0):
        up, flags, left = (int(v) for v in self.query('DBG:PING? %d' % keepalive_ms).split(b','))
        return dict(uptime_ms=up, pllsai=bool(flags & 1), usb_core=bool(flags & 2),
                    keepalive_left_ms=left)

    def lcdread(self, x, y, w, h, nwords):
        d = self.block('DBG:LCD? %d,%d,%d,%d,%d' % (x, y, w, h, nwords))
        return list(struct.unpack('<%dH' % (len(d) // 2), d))


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
    ap.add_argument('--dev')
    sub = ap.add_subparsers(dest='cmd', required=True)
    sub.add_parser('ping')
    p = sub.add_parser('hold'); p.add_argument('seconds', type=float, nargs='?', default=0)
    p = sub.add_parser('scpi'); p.add_argument('commands', nargs='+')
    p = sub.add_parser('lcdread')
    for k in 'x y w h n'.split():
        p.add_argument(k, type=int)
    p = sub.add_parser('shot'); p.add_argument('out')
    p.add_argument('--format', choices=['rgb565', 'rgb666'], default='rgb666')
    sub.add_parser('diag')
    p = sub.add_parser('tap'); p.add_argument('x', type=int); p.add_argument('y', type=int)
    sub.add_parser('left')
    sub.add_parser('right')
    p = sub.add_parser('key'); p.add_argument('code', type=int, help='0 MENU, 1 RUN, 2 AUTO, 3 MF')
    sub.add_parser('release')
    sub.add_parser('reset')
    a = ap.parse_args()

    ln = Link(a.dev)
    if a.cmd == 'ping':
        print(ln.ping())
    elif a.cmd == 'hold':
        t_end = time.time() + a.seconds if a.seconds else None
        while t_end is None or time.time() < t_end:
            print(ln.ping(15000), flush=True)
            time.sleep(5)
    elif a.cmd == 'scpi':
        for c in a.commands:
            if '?' in c:
                print(ln.query(c).decode(errors='replace').rstrip())
            else:
                ln.write(c)
    elif a.cmd == 'lcdread':
        print(' '.join('%04x' % v for v in ln.lcdread(a.x, a.y, a.w, a.h, a.n)))
    elif a.cmd == 'shot':
        from PIL import Image
        img = Image.new('RGB', (W, H))
        per_px = 1 if a.format == 'rgb565' else 1.5
        rows = 4
        t0 = time.time()
        for y in range(0, H, rows):
            n = int(W * rows * per_px + 0.5)
            words = ln.lcdread(0, y, W, rows, 1 + n)[1:]
            for i, c in enumerate(decode(words, a.format, W * rows)):
                img.putpixel((i % W, y + i // W), c)
        img.save(a.out)
        print('saved %s in %.1f s' % (a.out, time.time() - t0))
    elif a.cmd == 'diag':
        print(ln.block('DBG:DIAG?').hex(' '))
    elif a.cmd == 'tap':
        ln.write('DBG:TAP %d,%d' % (a.x, a.y))
    elif a.cmd == 'left':
        ln.write('DBG:SWIPE L')
    elif a.cmd == 'right':
        ln.write('DBG:SWIPE R')
    elif a.cmd == 'key':
        ln.write('DBG:KEY %d' % a.code)
    elif a.cmd == 'release':
        ln.write('DBG:RELEASE')
    elif a.cmd == 'reset':
        ln.write('DBG:RESET')


if __name__ == '__main__':
    main()
