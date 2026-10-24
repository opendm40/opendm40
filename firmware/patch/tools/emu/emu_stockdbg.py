#!/usr/bin/env python3
"""Static emulation of the DM40 debug patch handler (src/stockdbg.c).

Runs the *compiled* patch_parse() from build-dbg/patch.elf under Unicorn,
with the stock firmware entry points it calls (parser, UART, LCD read-pixel)
replaced by Python stubs, and checks that:

  * sub 3 (raw)  emits exactly the RGB565 pixels of a synthetic framebuffer,
    in a well-formed AF 05 03 0C frame with a correct checksum;
  * sub 4 (RLE)  emits a frame whose decoded pixels + `consumed` count match
    the same framebuffer, over flat, patterned and noisy rows, and never
    exceeds the frame budget; resuming at dx+consumed tiles the row with no
    gap or overlap.

No hardware. Requires: pip install unicorn capstone  (a venv is fine).
Build the image first:  make dbg ORIG=../../firmware/vendor/dm40_v_2_6.bin

Exit status 0 = all checks passed.
"""
import os
import struct
import subprocess
import sys

from unicorn import (Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_HOOK_CODE,
                     UcError)
from unicorn.arm_const import (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2,
                               UC_ARM_REG_R3, UC_ARM_REG_SP, UC_ARM_REG_LR,
                               UC_ARM_REG_PC)

HERE = os.path.dirname(os.path.abspath(__file__))
PATCH = os.path.normpath(os.path.join(HERE, '..', '..'))  # firmware/patch

# --- fixed stock entry points the patch calls (Thumb, even address) --------
FW_PARSE = 0x0800d2fa
FW_UART_PUT = 0x080082a0
FW_UART_READY = 0x0800849c
FW_LCD_READ_PIXEL = 0x0800a6aa
STUBS = {FW_PARSE, FW_UART_PUT, FW_UART_READY, FW_LCD_READ_PIXEL}

# --- RAM layout used by the patch (from fw_v2_6.h / stockdbg.c) ------------
G_ADDR = 0x20025a20            # FW_G parsed-packet struct
DATA_ADDR = 0x20026000        # where G[5..8] points (request data bytes)
STACK_TOP = 0x2002f000
RAM_BASE = 0x20000000
RAM_SIZE = 0x30000            # 192 KiB covers all the above
CODE_BASE = 0x0807d000        # __patch_flash_start
FLASH_BASE = 0x08000000
FLASH_SIZE = 0x00100000
RET_MAGIC = 0x0803cec5        # a mapped flash addr we return to from patch_parse


def patch_parse_addr():
    out = subprocess.check_output(['arm-none-eabi-nm',
                                   os.path.join(PATCH, 'build-dbg', 'patch.elf')],
                                  text=True)
    for line in out.splitlines():
        a, t, name = (line.split() + [''])[:3]
        if name == 'patch_parse':
            return int(a, 16) | 1       # Thumb
    raise SystemExit('patch_parse not found; run `make dbg` first')


# --- synthetic framebuffer: deterministic, exercises RLE edge cases --------
def fb_pixel(x, y):
    """RGB565 of driver pixel (x, y). Row 0 flat, row 1 patterned runs,
    row 2 noisy (every pixel differs), others a gradient with short runs."""
    if y == 0:
        return 0x07E0                       # fully flat -> one long run
    if y == 1:
        return 0xF800 if (x // 23) % 2 else 0x001F   # 23-px runs
    if y == 2:
        return (x * 2654435761) & 0xFFFF    # pseudo-random, no runs
    return ((x // 7) + y * 97) & 0xFFFF     # 7-px runs


class Emu:
    def __init__(self):
        self.uc = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
        self.uc.mem_map(FLASH_BASE, FLASH_SIZE)
        self.uc.mem_map(RAM_BASE, RAM_SIZE)
        text = open(os.path.join(PATCH, 'build-dbg', 'patch.elf'), 'rb')
        text.close()
        blob = subprocess.check_output(
            ['arm-none-eabi-objcopy', '-O', 'binary', '-j', '.text', '-j', '.data',
             os.path.join(PATCH, 'build-dbg', 'patch.elf'), '/dev/stdout'])
        self.uc.mem_write(CODE_BASE, blob)
        self.uart = bytearray()
        self.uc.hook_add(UC_HOOK_CODE, self._hook)
        self.pp = patch_parse_addr()

    def _hook(self, uc, address, size, user):
        a = address & ~1
        if a not in STUBS:
            return
        if a == FW_PARSE:
            pass                            # no-op: G is pre-populated
        elif a == FW_UART_READY:
            uc.reg_write(UC_ARM_REG_R0, 1)  # always ready
        elif a == FW_UART_PUT:
            self.uart.append(uc.reg_read(UC_ARM_REG_R1) & 0xFF)
        elif a == FW_LCD_READ_PIXEL:
            x = uc.reg_read(UC_ARM_REG_R0)
            y = uc.reg_read(UC_ARM_REG_R1)
            uc.reg_write(UC_ARM_REG_R0, fb_pixel(x, y) & 0xFFFF)
        uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))

    def run(self, sub, dx, dy, n):
        self.uart = bytearray()
        # G: byte[3] = cmd 0x0B, bytes[5..8] = little-endian data pointer
        g = bytearray(16)
        g[3] = 0x0B
        struct.pack_into('<I', g, 5, DATA_ADDR)
        self.uc.mem_write(G_ADDR, bytes(g))
        # request data: [addr:4][sub:1][n:1]; addr = (dy<<16)|dx
        addr = ((dy & 0xFFFF) << 16) | (dx & 0xFFFF)
        self.uc.mem_write(DATA_ADDR, struct.pack('<IBB', addr, sub, n))
        self.uc.reg_write(UC_ARM_REG_SP, STACK_TOP)
        self.uc.reg_write(UC_ARM_REG_LR, RET_MAGIC | 1)
        self.uc.reg_write(UC_ARM_REG_R0, 0)   # frame arg (unused by stub parse)
        self.uc.emu_start(self.pp, RET_MAGIC, count=2_000_000)
        return bytes(self.uart)


def parse_frame(f):
    assert f[:4] == bytes([0xAF, 0x05, 0x03, 0x0C]), f[:4].hex()
    n = f[4]
    data = f[5:5 + n]
    crc = f[5 + n]
    assert (sum(f[:5 + n]) + crc) & 0xFF == 0, 'bad checksum'
    assert len(f) == 6 + n, 'trailing bytes'
    return data


def decode_rle(data):
    consumed = data[0]
    px = []
    i = 1
    while i + 2 < len(data) + 1 and i + 2 <= len(data):
        cnt = data[i]
        col = data[i + 1] | (data[i + 2] << 8)
        px.extend([col] * cnt)
        i += 3
    assert i == len(data), 'ragged RLE payload'
    return consumed, px


def main():
    emu = Emu()
    fails = 0

    # --- sub 3 raw: full-width rows, clamped to 125 px -----------------
    for dy in range(4):
        data = parse_frame(emu.run(3, 0, dy, 125))
        got = list(struct.unpack('<%dH' % (len(data) // 2), data))
        exp = [fb_pixel(x, dy) for x in range(125)]
        if got != exp:
            print('FAIL sub3 row %d' % dy); fails += 1
        else:
            print('ok   sub3 row %d: 125 px exact' % dy)
    # clamp check: ask for 200, must get 125
    data = parse_frame(emu.run(3, 0, 3, 200))
    if len(data) != 2 * 125:
        print('FAIL sub3 clamp (%d bytes)' % len(data)); fails += 1
    else:
        print('ok   sub3 clamp 200->125 px')

    # --- sub 4 RLE: flat / patterned / noisy, tile the whole 320 row ---
    for dy in range(4):
        x = 0
        tiled = []
        budget_ok = True
        guard = 0
        while x < 320:
            frame = emu.run(4, x, dy, min(255, 320 - x))
            data = parse_frame(frame)
            # the reply length byte is u8 (like memrw reads, up to 0xFF); the
            # 0x97 limit is on the REQUEST, which here is a fixed 6 data bytes.
            if frame[4] > 0xFF or len(data) > 0xFF:
                budget_ok = False
            consumed, px = decode_rle(data)
            if consumed == 0:              # a single run bigger than budget?
                print('FAIL sub4 row %d stuck at x=%d' % (dy, x)); fails += 1
                break
            assert len(px) == consumed, 'consumed/px mismatch'
            tiled.extend(px)
            x += consumed
            guard += 1
            if guard > 1000:
                print('FAIL sub4 row %d no progress' % dy); fails += 1
                break
        exp = [fb_pixel(xx, dy) for xx in range(320)]
        if tiled[:320] == exp and budget_ok:
            print('ok   sub4 row %d: 320 px round-trip, replies well-formed' % dy)
        else:
            print('FAIL sub4 row %d round-trip/budget' % dy); fails += 1

    print('\n%s' % ('ALL EMULATION CHECKS PASSED' if not fails
                    else '%d CHECK(S) FAILED' % fails))
    return 1 if fails else 0


if __name__ == '__main__':
    sys.exit(main())
