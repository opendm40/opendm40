#!/usr/bin/env python3
"""Load a raw binary image into DM40 SRAM over the Bluetooth serial port
and start it.

Transport: the RFCOMM service on channel 1 speaks the DM40 packet protocol
(`AF 05 03 <cmd> <len> <data> <crc>`); commands `AF 05 03 0B ..` (load /
verify) and `AF 05 03 0A ..` (start) are provided by the device firmware.

Image format: word0 = initial SP, word1 = entry point (Thumb). The loader
copies the image to `--addr` (default 0x20008000) and starts it at word1.
"""
import argparse
import socket
import struct
import sys
import time

ID = bytes([0x05, 0x03])
CHUNK = 0x7D


def crc(d):
    return d + struct.pack('B', (0x100 - sum(d)) & 0xFF)


def pkt(data):
    return crc(bytes([0xAF]) + ID + bytes([0x0B, len(data)]) + data)


class Dev:
    def __init__(self, mac, timeout=4):
        last = None
        for _ in range(8):
            try:
                self.s = socket.socket(socket.AF_BLUETOOTH, socket.SOCK_STREAM,
                                       socket.BTPROTO_RFCOMM)
                self.s.settimeout(timeout)
                self.s.connect((mac, 1))
                return
            except OSError as e:
                last = e
                time.sleep(5)
        raise SystemExit('cannot connect: %s' % last)

    def xfer(self, data, want):
        self.s.send(pkt(data))
        buf = b''
        try:
            while len(buf) < want:
                r = self.s.recv(want - len(buf))
                if not r:
                    break
                buf += r
        except socket.timeout:
            pass
        return buf

    def rd(self, addr, n):
        out = b''
        while n:
            k = min(n, 0xFF)
            b = self.xfer(struct.pack('<IBB', addr, 0, k), 5 + k + 1)
            if b[:5] != bytes([0xAF, 0x05, 0x03, 0x0C, k]):
                raise IOError('read 0x%08x: %s' % (addr, b.hex(' ')))
            out += b[5:5 + k]
            addr += k
            n -= k
        return out

    def wr(self, addr, data):
        off = 0
        while off < len(data):
            chunk = data[off:off + CHUNK]
            b = self.xfer(struct.pack('<IBB', addr + off, 1, len(chunk)) + chunk, 6)
            if b[:5] != bytes([0xAF, 0x05, 0x03, 0x0C, 0x00]):
                raise IOError('write 0x%08x: %s' % (addr + off, b.hex(' ')))
            off += len(chunk)

    def start(self, entry):
        """Command 0x0A: begin execution at `entry` (Thumb)."""
        data = bytearray(0xC2)
        data[155:159] = struct.pack('<I', entry)
        self.s.send(crc(bytes([0xAF]) + ID + bytes([0x0A, 0xC2]) + bytes(data)))


def main():
    p = argparse.ArgumentParser()
    p.add_argument('mac')
    p.add_argument('image')
    p.add_argument('--addr', type=lambda x: int(x, 0), default=0x20008000)
    p.add_argument('--entry', type=lambda x: int(x, 0), default=None,
                   help='default: word1 of the image')
    p.add_argument('--verify-only', action='store_true')
    p.add_argument('--no-verify', action='store_true')
    a = p.parse_args()

    fw = open(a.image, 'rb').read()
    sp, w1 = struct.unpack_from('<2I', fw, 0)
    entry = a.entry if a.entry is not None else w1
    print('image %s: %d bytes, SP=0x%08x, entry=0x%08x -> load at 0x%08x'
          % (a.image, len(fw), sp, entry, a.addr))

    d = Dev(a.mac)
    print('connected')
    d.wr(a.addr, fw)
    print('written %d bytes (chunks of 0x%x)' % (len(fw), CHUNK))
    if not a.no_verify:
        if d.rd(a.addr, len(fw)) != fw:
            print('verify failed', file=sys.stderr)
            sys.exit(1)
        print('verify OK')
    if a.verify_only:
        return
    d.start(entry)
    print('started at 0x%08x' % entry)


if __name__ == '__main__':
    main()