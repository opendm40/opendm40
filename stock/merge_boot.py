#!/usr/bin/env python3
"""Merge bootloader dump pieces into one 16 KB image (0x08000000-0x08003FFF).

A piece's flash address is taken from the last hex number in its filename:
  - an 8-digit value like 08001400 (or 0x08001400) is an absolute address;
  - a short value <= 0x4000 (e.g. 1400) is an offset from 0x08000000.
Names with no number must be listed in EXTRA_MAP. Gaps are filled with 0xFF.

  python3 merge_boot.py [dir] [out.bin]
"""
import glob
import os
import re
import sys

BASE = 0x08000000
END = 0x08004000
EXTRA_MAP = {'boot_rest.bin': 0x08000100, 'dm40_boot.bin': 0x08000000}


def addr_from_name(bn):
    if bn in EXTRA_MAP:
        return EXTRA_MAP[bn]
    nums = re.findall(r'(?:0x)?([0-9a-fA-F]{3,8})', bn.replace('.bin', ''))
    if not nums:
        return None
    v = int(nums[-1], 16)
    if v >= BASE:
        return v
    if v < (END - BASE):
        return BASE + v
    return None


def main():
    d = sys.argv[1] if len(sys.argv) > 1 else '.'
    out = sys.argv[2] if len(sys.argv) > 2 else 'bootloader.bin'
    img = bytearray(b'\xff' * (END - BASE))
    covered = bytearray(END - BASE)
    pieces = []
    files = set(glob.glob(os.path.join(d, '*boot*.bin')))
    for f in sorted(files):
        bn = os.path.basename(f)
        addr = addr_from_name(bn)
        if addr is None:
            print('skip %s: no address in name' % bn)
            continue
        data = open(f, 'rb').read()
        off = addr - BASE
        if off < 0 or off + len(data) > len(img):
            print('skip %s @ 0x%08x: outside region' % (bn, addr))
            continue
        img[off:off + len(data)] = data
        for i in range(off, off + len(data)):
            covered[i] = 1
        pieces.append((addr, len(data), bn))
    open(out, 'wb').write(img)
    print('pieces: %d -> %s' % (len(pieces), out))
    for a, n, bn in sorted(pieces):
        print('  0x%08x  %5d bytes  %s' % (a, n, bn))
    cov = sum(covered)
    print('coverage: %d / %d bytes (%.1f%%)' % (cov, len(img), 100 * cov / len(img)))
    gaps = []
    i = 0
    while i < len(covered):
        if not covered[i]:
            j = i
            while j < len(covered) and not covered[j]:
                j += 1
            gaps.append((BASE + i, BASE + j))
            i = j
        else:
            i += 1
    if gaps:
        print('missing:')
        for s, e in gaps[:20]:
            print('  0x%08x - 0x%08x (%d)' % (s, e, e - s))
    else:
        print('FULL 16 KB captured')


if __name__ == '__main__':
    main()
