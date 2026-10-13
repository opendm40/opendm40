#!/usr/bin/env python3
"""Apply a linked patch (ELF) to a DM40 firmware update file.

  patcher.py <orig_fw.bin> <patch.elf> <out_fw.bin>

The patch code is placed at its link address after the end of the original
image, then every hook from the .hooks section is applied to the original
code.
"""
import os
import struct
import subprocess
import sys
import tempfile

PAD = int(os.environ.get('PATCH_PAD', '64'), 0)   # trailing nop pad (see below)

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
import dm40fw  # noqa: E402

OBJCOPY = os.environ.get('OBJCOPY', 'arm-none-eabi-objcopy')
NM = os.environ.get('NM', 'arm-none-eabi-nm')

HOOK_BL, HOOK_BW, HOOK_WORD = 1, 2, 3
HOOK_NAMES = {HOOK_BL: 'bl', HOOK_BW: 'b.w', HOOK_WORD: 'word'}


def objcopy_section(elf, args):
    with tempfile.NamedTemporaryFile(suffix='.bin') as f:
        subprocess.run([OBJCOPY, '-O', 'binary'] + args + [elf, f.name], check=True)
        return open(f.name, 'rb').read()


def symbols(elf):
    out = subprocess.run([NM, elf], check=True, capture_output=True, text=True).stdout
    syms = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 3:
            syms[parts[2]] = int(parts[0], 16)
    return syms


def thumb_branch(src, dst, link):
    """Encode Thumb-2 bl / b.w from src to dst."""
    off = (dst & ~1) - (src + 4)
    if not -(1 << 24) <= off < (1 << 24) or off & 1:
        raise ValueError('branch 0x%08x -> 0x%08x out of range' % (src, dst))
    s = (off >> 24) & 1
    i1 = (off >> 23) & 1
    i2 = (off >> 22) & 1
    j1 = (1 - i1) ^ s
    j2 = (1 - i2) ^ s
    hi = 0xf000 | (s << 10) | ((off >> 12) & 0x3ff)
    lo = (0xd000 if link else 0x9000) | (j1 << 13) | (j2 << 11) | ((off >> 1) & 0x7ff)
    return struct.pack('<HH', hi, lo)


def patch_body(body, elf):
    syms = symbols(elf)
    base = dm40fw.LOAD_ADDR
    image_end = base + len(body)
    flash_start = syms['__patch_flash_start']
    flash_end = syms['__patch_flash_end']
    if flash_start < image_end:
        raise ValueError('patch at 0x%08x overlaps the image (end 0x%08x)' % (flash_start, image_end))

    code = objcopy_section(elf, ['-j', '.text', '-j', '.data'])
    if len(code) != flash_end - flash_start:
        raise ValueError('unexpected patch size 0x%x' % len(code))

    out = bytearray(body)
    out += b'\xff' * (flash_start - image_end)
    out += code
    out += b'\x00\xbf' * (PAD // 2)         # trailing nops: the bootloader
    # rejects update files whose body ends right after the patch literal
    # pool (observed: 308-byte code -> 'update error', same code +64 nops ok)
    # the bootloader flashes the body in 8-byte words and rejects lengths
    # that are not a multiple of 8 (observed: 0x79118 ok, 0x79134 'update error')
    pad = -len(out) % 8
    out += b'\xff' * pad
    print('patch code 0x%08x-0x%08x (%d bytes +%d pad), body 0x%x'
          % (flash_start, flash_end + PAD, len(code), PAD, len(out)))

    hooks = objcopy_section(elf, ['-j', '.hooks', '--set-section-flags', '.hooks=alloc,load'])
    for i in range(0, len(hooks), 16):
        htype, addr, target, expect = struct.unpack_from('<4I', hooks, i)
        off = addr - base
        if not 0 <= off < len(body) - 3:
            raise ValueError('hook address 0x%08x outside the image' % addr)
        orig = struct.unpack_from('<I', out, off)[0]
        if expect and orig != expect:
            raise ValueError('hook at 0x%08x: found 0x%08x, expected 0x%08x' % (addr, orig, expect))
        if htype == HOOK_BL:
            new = thumb_branch(addr, target, True)
        elif htype == HOOK_BW:
            new = thumb_branch(addr, target, False)
        elif htype == HOOK_WORD:
            new = struct.pack('<I', target)
        else:
            raise ValueError('unknown hook type %d' % htype)
        out[off:off + 4] = new
        print('hook %-4s 0x%08x -> 0x%08x' % (HOOK_NAMES[htype], addr, target))
    return bytes(out)


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    orig_path, elf, out_path = sys.argv[1:]
    orig = open(orig_path, 'rb').read()
    _, _, _, body = dm40fw.split(dm40fw.decode(orig))
    new_body = patch_body(body, elf)
    open(out_path, 'wb').write(dm40fw.build(new_body, like=orig))
    print('written %s' % out_path)


if __name__ == '__main__':
    main()
