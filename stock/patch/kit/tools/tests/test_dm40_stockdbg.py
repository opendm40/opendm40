#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Offline tests for dm40_stockdbg.py. No hardware, no Bluetooth needed.

Run:  python3 -I tools/tests/test_dm40_stockdbg.py
A fake socket models the debug firmware's BT replies (memory read + LCD
raw/RLE), so the host read-back, RLE decode, orientation and PNG writer are
all exercised end to end.
"""
import importlib.util
import os
import struct
import sys
import tempfile
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(HERE, '..', 'dm40_stockdbg.py')

spec = importlib.util.spec_from_file_location('dm40_stockdbg', TOOL)
sd = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sd)
rw = sd.rw


# --- synthetic framebuffer (same shape as the emulator's) ------------------
def fb_pixel(x, y):
    if y == 0:
        return 0x07E0
    if y == 1:
        return 0xF800 if (x // 23) % 2 else 0x001F
    if y == 2:
        return (x * 2654435761) & 0xFFFF
    return ((x // 7) + y * 97) & 0xFFFF


RLE_OUT_MAX = 250            # must match stockdbg.c


def _device_rle(dx, dy, n):
    """Byte-for-byte model of stockdbg.c sub 4."""
    out = bytearray([0])
    committed = 0
    cur = 0
    run = 0
    i = 0
    while i < n:
        c = fb_pixel(dx + i, dy)
        if run and (c != cur or run >= 255):
            if len(out) + 3 > 1 + RLE_OUT_MAX:
                break
            out += bytes((run, cur & 0xFF, cur >> 8))
            committed += run
            run = 0
        if run == 0:
            cur, run = c, 1
        else:
            run += 1
        i += 1
    if run and len(out) + 3 <= 1 + RLE_OUT_MAX:
        out += bytes((run, cur & 0xFF, cur >> 8))
        committed += run
    out[0] = committed
    return bytes(out)


class FakeSock:
    """Implements just enough of a socket for DbgDev: memory + LCD replies."""

    def __init__(self, mem=None):
        self.mem = mem or {}
        self._tx = b''
        self._rx = b''

    def settimeout(self, t):
        pass

    def close(self):
        pass

    def send(self, data):
        # parse one request AF 05 03 0B LEN [addr:4][sub][n][payload]
        assert data[:4] == bytes([0xAF, 0x05, 0x03, 0x0B]), data[:4].hex()
        ln = data[4]
        body = data[5:5 + ln]
        addr, sub, n = struct.unpack_from('<IBB', body, 0)
        if sub == 0:                         # memory read
            payload = bytes(self.mem.get(addr + i, 0) for i in range(n))
            self._rx += self._frame(payload)
        elif sub == sd.SUB_LCD_RAW:
            dx, dy = addr & 0xFFFF, addr >> 16
            k = min(n, sd.RAW_MAX)
            px = [fb_pixel(dx + i, dy) for i in range(k)]
            self._rx += self._frame(struct.pack('<%dH' % k, *px))
        elif sub == sd.SUB_LCD_RLE:
            dx, dy = addr & 0xFFFF, addr >> 16
            self._rx += self._frame(_device_rle(dx, dy, n))
        elif sub == sd.SUB_INJECT:           # program touch injection
            count = body[6]
            self.last_inject = []
            for k in range(count):
                fl, x, y = struct.unpack_from('<Hhh', body, 7 + k * 6)
                self.last_inject.append((fl, x, y))
            self._rx += self._frame(bytes([count]))
        elif sub == sd.SUB_ARM:              # arm / disarm
            self.armed = bool(body[6])
            self._rx += self._frame(bytes([1 if self.armed else 0]))
        else:
            raise AssertionError('unexpected sub %d' % sub)

    def recv(self, n):
        if not self._rx:
            raise sd.rw.DeviceError('no data queued')
        out, self._rx = self._rx[:n], self._rx[n:]
        return out

    @staticmethod
    def _frame(payload):
        hdr = bytes([0xAF, 0x05, 0x03, 0x0C, len(payload)])
        crc = (0x100 - (sum(hdr) + sum(payload))) & 0xFF
        return hdr + payload + bytes((crc,))


def fake_dev(mem=None):
    dev = sd.DbgDev.__new__(sd.DbgDev)       # bypass connect()
    dev.s = FakeSock(mem)
    dev.timeout = 1.0
    return dev


# --- tests -----------------------------------------------------------------
def test_lcd_req_golden():
    # sub 3, dx=10 dy=2, n=125  -> addr = 2<<16 | 10 = 0x0002000A
    got = sd.lcd_req(sd.SUB_LCD_RAW, 10, 2, 125)
    exp = rw.pkt(3, 0x0002000A, 125)
    assert got == exp, got.hex(' ')
    assert sum(got) & 0xFF == 0
    print('ok  lcd_req encoding + crc')


def test_raw_span():
    dev = fake_dev()
    for dy in (0, 1, 2, 3):
        px = dev.lcd_span_raw(0, dy, 125)
        assert px == [fb_pixel(x, dy) for x in range(125)], 'row %d' % dy
    # clamp at 125
    assert len(dev.lcd_span_raw(0, 3, 200)) == 125
    print('ok  raw span read + clamp')


def test_rle_row():
    dev = fake_dev()
    for dy in (0, 1, 2, 3):
        rows = dev.lcd_region(0, dy, 320, 1, raw=False)
        assert rows[0] == [fb_pixel(x, dy) for x in range(320)], 'row %d' % dy
    print('ok  rle region round-trip (flat/patterned/noisy/gradient)')


def test_consumed_tiling():
    # noisy row: every run is 1 px -> multiple frames, must tile with no gap
    dev = fake_dev()
    x = 0
    seen = []
    while x < 320:
        consumed, px = dev.lcd_row_rle(x, 2, 320 - x)
        assert consumed > 0 and consumed == len(px)
        seen.extend(px[:consumed])
        x += consumed
    assert seen[:320] == [fb_pixel(i, 2) for i in range(320)]
    print('ok  rle consumed-count tiling on a non-compressible row')


def test_orient_transpose():
    # driver 2 wide x 3 tall -> landscape transpose 3 wide x 2 tall
    drv = [[0x11, 0x12], [0x21, 0x22], [0x31, 0x32]]

    class A:
        driver = False; rot = 0; flipx = False; flipy = False
    img = sd.orient(drv, A)
    assert img == [[0x11, 0x21, 0x31], [0x12, 0x22, 0x32]], img

    class B(A):
        driver = True
    assert sd.orient(drv, B) == drv
    print('ok  orientation transpose + --driver passthrough')


def test_png_roundtrip():
    rows = [[(255, 0, 0), (0, 255, 0)], [(0, 0, 255), (8, 16, 24)]]
    with tempfile.NamedTemporaryFile(suffix='.png', delete=False) as f:
        path = f.name
    sd.write_png(path, rows)
    blob = open(path, 'rb').read()
    os.unlink(path)
    assert blob[:8] == b'\x89PNG\r\n\x1a\n'
    assert blob[12:16] == b'IHDR'
    w, h, depth, ctype = struct.unpack('>IIBB', blob[16:26])
    assert (w, h, depth, ctype) == (2, 2, 8, 2), (w, h, depth, ctype)
    # IDAT present and inflatable
    assert b'IDAT' in blob and b'IEND' in blob
    print('ok  png writer (signature, IHDR, IDAT)')


def test_rgb565_corners():
    assert sd.rgb565_to_888(0x0000) == (0, 0, 0)
    assert sd.rgb565_to_888(0xFFFF) == (255, 255, 255)
    assert sd.rgb565_to_888(0xF800) == (255, 0, 0)
    assert sd.rgb565_to_888(0x07E0) == (0, 255, 0)
    assert sd.rgb565_to_888(0x001F) == (0, 0, 255)
    print('ok  rgb565 -> 888')


def test_memory_read_and_peek():
    mem = {0x20027C74 + i: v for i, v in enumerate(
        bytes.fromhex('28 80 00 00 00 20 03 00 ff ff ff'.replace(' ', '')))}
    dev = fake_dev(mem)
    data = dev.read(0x20027C74, 11)
    assert data == bytes.fromhex('28800000002003 00 ffffff'.replace(' ', ''))
    assert sd.SYMBOLS['meas'][0] == 0x20027C74
    assert '20027c74' in sd.hexdump(data, 0x20027C74)
    print('ok  memory read + peek-struct symbol + hexdump')


def test_arm_and_inject_encoding():
    dev = fake_dev()

    class Ns:
        pass
    # enable-scope -> sub 6 with on=1, device reports armed
    sd.do_enable_scope(dev, Ns())
    assert dev.s.armed is True
    # tap -> 2 entries: press 0xC1 then release 0x0000, same coords
    a = Ns(); a.x, a.y = 240, 160
    sd.do_tap(dev, a)
    assert dev.s.last_inject == [(sd.PRESS_FLAGS, 240, 160),
                                 (sd.RELEASE_FLAGS, 240, 160)], dev.s.last_inject
    # drag -> starts pressed at (x0,y0), ends with a release at (x1,y1)
    a = Ns(); a.x0, a.y0, a.x1, a.y1, a.steps = 100, 160, 300, 160, 4
    sd.do_drag(dev, a)
    seq = dev.s.last_inject
    assert seq[0] == (sd.PRESS_FLAGS, 100, 160)
    assert seq[-1] == (sd.RELEASE_FLAGS, 300, 160)
    assert all(f == sd.PRESS_FLAGS for f, _, _ in seq[:-1])
    assert len(seq) <= sd.INJ_MAX
    # key 1 (RUN) -> down with bit5 + code, then release
    a = Ns(); a.code, a.hold = 1, 0
    sd.do_key(dev, a)
    assert dev.s.last_inject[0] == (0x20 | (1 << 3), 0, 0)
    assert dev.s.last_inject[-1] == (sd.RELEASE_FLAGS, 0, 0)
    # disable-scope
    sd.do_disable_scope(dev, Ns())
    assert dev.s.armed is False
    print('ok  arm + tap/drag/key injection encoding')


def main():
    for name, fn in sorted(globals().items()):
        if name.startswith('test_') and callable(fn):
            fn()
    print('\nall dm40_stockdbg tests passed')


if __name__ == '__main__':
    main()
