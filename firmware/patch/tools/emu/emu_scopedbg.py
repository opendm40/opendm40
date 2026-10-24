#!/usr/bin/env python3
"""Static emulation of the scope-page hook (src/scopetramp.S + src/scopedbg.c).

Checks, on the COMPILED scope image (build-dbgscope/patch.elf, i.e. the code in
build-dbg/dm40_dbgscope.bin):

  A. flag==0 (disarmed) the trampoline at 0x080282a0 is BIT-IDENTICAL in effect
     to the stock thunk: same scope poll method reached, same r0..r3, same sp,
     same lr, and no writes anywhere (compared against the stock image).
  B. flag==1 (armed) with a queued tap, touch_push lands the correct 22-byte
     entries in the stock touch queue (0x2002561c) and advances the write index.
  C. the parser-pump path: when a BT frame is 'ready', scope_work runs the
     0x0B handler and emits a correct screenshot reply over UART.
  D. patch_init() leaves the hook disarmed (arm word == 0).

No hardware. Requires unicorn + capstone. Build first: make dbg-scope.
Exit 0 = all checks passed.
"""
import os
import struct
import subprocess
import sys

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.arm_const import (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2,
                               UC_ARM_REG_R3, UC_ARM_REG_SP, UC_ARM_REG_LR,
                               UC_ARM_REG_PC)

HERE = os.path.dirname(os.path.abspath(__file__))
PATCH = os.path.normpath(os.path.join(HERE, '..', '..'))
ROOT = os.path.normpath(os.path.join(PATCH, '..', '..'))
sys.path.insert(0, os.path.join(PATCH, '..'))
import dm40fw  # noqa: E402

FLASH_BASE, FLASH_SIZE = 0x08000000, 0x00100000
RAM_BASE, RAM_SIZE = 0x20000000, 0x40000
STACK_TOP = 0x2002f000

# stock entry points (stubbed)
FW_PARSE = 0x0800d2fa
FW_UART_PUT = 0x080082a0
FW_UART_READY = 0x0800849c
FW_LCD_READ_PIXEL = 0x0800a6aa
FW_FRAME_READY = 0x0800d5f6

THUNK = 0x080282a0
SCOPE_STATE = 0x2000f100
ARM_MAGIC = 0x5C09EDB6
TP_QUEUE = 0x2002561c

# scope-GUI pointer chain the thunk walks
MGR_PTR = 0x20021000
SCOPEOBJ = 0x20021200
VTABLE = 0x20021400
POLL = 0x08099998          # sentinel scope poll method (even; +1 as pointer)


def sym(name):
    out = subprocess.check_output(['arm-none-eabi-nm',
                                   os.path.join(PATCH, 'build-dbgscope', 'patch.elf')],
                                  text=True)
    for line in out.splitlines():
        p = (line.split() + [''])[:3]
        if p[2] == name:
            return int(p[0], 16)
    raise SystemExit('symbol %s not found; run `make dbg-scope`' % name)


def body(path):
    return dm40fw.split(dm40fw.decode(open(path, 'rb').read()))[3]


STOCK = body(os.path.join(ROOT, 'vendor', 'dm40_v_2_6.bin'))
SCOPE = body(os.path.join(PATCH, 'build-dbg', 'dm40_dbgscope.bin'))


def fb_pixel(x, y):
    return 0x07E0 if y == 0 else ((x * 2654435761) & 0xFFFF)


class Emu:
    def __init__(self, img, record_writes=False):
        self.uc = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
        self.uc.mem_map(FLASH_BASE, FLASH_SIZE)
        self.uc.mem_map(RAM_BASE, RAM_SIZE)
        self.uc.mem_write(0x08004000, bytes(img))
        self.uart = bytearray()
        self.writes = []
        self.frame_ready = 0
        self.poll_state = None
        self.uc.hook_add(UC_HOOK_CODE, self._code)
        if record_writes:
            self.uc.hook_add(UC_HOOK_MEM_WRITE, self._wr)
        # scope GUI pointer chain
        self.uc.mem_write(0x20020548, struct.pack('<I', MGR_PTR))
        self.uc.mem_write(MGR_PTR + 0x12c, struct.pack('<I', SCOPEOBJ))
        self.uc.mem_write(SCOPEOBJ, struct.pack('<I', VTABLE))
        self.uc.mem_write(VTABLE + 0x54, struct.pack('<I', POLL | 1))

    def _wr(self, uc, access, addr, size, value, ud):
        # ignore our own stack pushes
        sp = uc.reg_read(UC_ARM_REG_SP)
        if not (sp - 0x40 <= addr <= STACK_TOP):
            self.writes.append((addr, size, value))

    def _code(self, uc, address, size, user):
        a = address & ~1
        if a == POLL:
            self.poll_state = dict(
                r0=uc.reg_read(UC_ARM_REG_R0), r1=uc.reg_read(UC_ARM_REG_R1),
                r2=uc.reg_read(UC_ARM_REG_R2), r3=uc.reg_read(UC_ARM_REG_R3),
                sp=uc.reg_read(UC_ARM_REG_SP), lr=uc.reg_read(UC_ARM_REG_LR))
            uc.emu_stop()
            return
        if a == FW_FRAME_READY:
            uc.reg_write(UC_ARM_REG_R0, self.frame_ready)
        elif a == FW_UART_READY:
            uc.reg_write(UC_ARM_REG_R0, 1)
        elif a == FW_UART_PUT:
            self.uart.append(uc.reg_read(UC_ARM_REG_R1) & 0xFF)
        elif a == FW_LCD_READ_PIXEL:
            x = uc.reg_read(UC_ARM_REG_R0)
            y = uc.reg_read(UC_ARM_REG_R1)
            uc.reg_write(UC_ARM_REG_R0, fb_pixel(x, y))
        elif a == FW_PARSE:
            self._parse_stub(uc)
        else:
            return
        uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))

    def _parse_stub(self, uc):
        # populate G (0x20025a20): cmd 0x0B, data ptr -> request bytes
        g = bytearray(16)
        g[3] = 0x0B
        data_addr = 0x20026000
        struct.pack_into('<I', g, 5, data_addr)
        uc.mem_write(0x20025a20, bytes(g))
        uc.mem_write(data_addr, self.req)

    def run_thunk(self, arm=0):
        self.uc.mem_write(SCOPE_STATE, struct.pack('<I', arm))
        for r, v in ((UC_ARM_REG_R0, 0xA0A0A0A0), (UC_ARM_REG_R1, 0xB1B1B1B1),
                     (UC_ARM_REG_R2, 0xC2C2C2C2), (UC_ARM_REG_R3, 0xD3D3D3D3)):
            self.uc.reg_write(r, v)
        self.uc.reg_write(UC_ARM_REG_SP, STACK_TOP)
        self.uc.reg_write(UC_ARM_REG_LR, 0x08011111)   # dispatcher return sentinel
        self.poll_state = None
        self.uc.emu_start(THUNK | 1, 0, count=2000)
        return self.poll_state


def test_A_bit_identity():
    a = Emu(STOCK, record_writes=True)
    orig = a.run_thunk()
    ow = list(a.writes)
    b = Emu(SCOPE, record_writes=True)
    tramp = b.run_thunk(arm=0)          # disarmed
    tw = list(b.writes)
    assert orig is not None and tramp is not None, 'poll not reached'
    assert orig == tramp, 'state differs:\n orig=%s\n tramp=%s' % (orig, tramp)
    assert orig['r0'] == SCOPEOBJ and (orig['r1'] & ~1) == POLL
    assert ow == tw == [], 'unexpected memory writes: orig=%s tramp=%s' % (ow, tw)
    print('ok  A: flag==0 trampoline bit-identical to stock thunk '
          '(same r0..r3/sp/lr, same poll, no writes)')


def test_B_injection():
    e = Emu(SCOPE)
    e.frame_ready = 0                    # no BT frame -> injection only
    # arm + program a tap at (240,160): down 0xC1 then up 0x00
    st = bytearray(16 + 24 * 8)
    struct.pack_into('<IIII', st, 0, ARM_MAGIC, 2, 0, 0)
    struct.pack_into('<HhhH', st, 16 + 0 * 8, 0x00C1, 240, 160, 0)
    struct.pack_into('<HhhH', st, 16 + 1 * 8, 0x0000, 240, 160, 0)
    e.uc.mem_write(SCOPE_STATE, bytes(st))
    e.uc.mem_write(TP_QUEUE + 0x2d6, b'\x00\x00')     # head=tail=0
    for _ in range(2):
        e.uc.reg_write(UC_ARM_REG_SP, STACK_TOP)
        e.uc.reg_write(UC_ARM_REG_LR, 0x08011111)
        e.poll_state = None
        e.uc.emu_start(THUNK | 1, 0, count=4000)
        assert e.poll_state is not None  # still reached the real poll
    head = e.uc.mem_read(TP_QUEUE + 0x2d6, 1)[0]
    assert head == 2, 'write index = %d' % head
    for k, (fl, x, y) in enumerate([(0xC1, 240, 160), (0x00, 240, 160)]):
        ent = e.uc.mem_read(TP_QUEUE + 0x10 + k * 22, 22)
        ex, ey = struct.unpack_from('<hh', ent, 0)
        flags = struct.unpack_from('<H', ent, 20)[0]
        assert (ex, ey, flags) == (x, y, fl), 'entry %d = %s' % (k, (ex, ey, flags))
    print('ok  B: flag==1 tap injected 2 correct entries, poll still runs')


def test_C_pump():
    e = Emu(SCOPE)
    e.frame_ready = 1
    # request: sub 3 raw, dx=0 dy=0, n=4  -> expect 4 RGB565 pixels of row 0
    e.req = struct.pack('<IBB', 0, 3, 4)
    st = bytearray(16 + 24 * 8)
    struct.pack_into('<IIII', st, 0, ARM_MAGIC, 0, 0, 0)  # armed, no injection
    e.uc.mem_write(SCOPE_STATE, bytes(st))
    e.uc.reg_write(UC_ARM_REG_SP, STACK_TOP)
    e.uc.reg_write(UC_ARM_REG_LR, 0x08011111)
    e.poll_state = None
    e.uc.emu_start(THUNK | 1, 0, count=20000)
    f = bytes(e.uart)
    assert f[:4] == bytes([0xAF, 0x05, 0x03, 0x0C]), f[:8].hex()
    n = f[4]
    assert n == 8, 'reply n=%d' % n
    px = list(struct.unpack('<4H', f[5:13]))
    assert px == [fb_pixel(x, 0) for x in range(4)], px
    assert (sum(f[:5 + n]) + f[5 + n]) & 0xFF == 0, 'bad crc'
    print('ok  C: parser-pump path serves a sub-3 screenshot reply on scope page')


def test_D_patch_init():
    e = Emu(SCOPE)
    e.uc.mem_write(SCOPE_STATE, struct.pack('<I', ARM_MAGIC))   # pretend armed
    e.uc.reg_write(UC_ARM_REG_SP, STACK_TOP)
    e.uc.reg_write(UC_ARM_REG_LR, 0x08011111)
    pi = sym('patch_init')
    # run until return to the LR sentinel

    def stop(uc, address, size, user):
        if (address & ~1) == 0x08011110:
            uc.emu_stop()
    e.uc.hook_add(UC_HOOK_CODE, stop)
    e.uc.emu_start(pi | 1, 0, count=2000)
    arm = struct.unpack('<I', e.uc.mem_read(SCOPE_STATE, 4))[0]
    assert arm == 0, 'patch_init left arm=0x%08x' % arm
    print('ok  D: patch_init disarms the hook at boot')


def main():
    fails = 0
    for fn in (test_A_bit_identity, test_B_injection, test_C_pump, test_D_patch_init):
        try:
            fn()
        except AssertionError as e:
            print('FAIL %s: %s' % (fn.__name__, e)); fails += 1
    print('\n%s' % ('ALL SCOPE EMULATION CHECKS PASSED' if not fails
                    else '%d CHECK(S) FAILED' % fails))
    return 1 if fails else 0


if __name__ == '__main__':
    sys.exit(main())
