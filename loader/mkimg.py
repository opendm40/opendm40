#!/usr/bin/env python3
"""mkimg.py OUT [--zero-marker]: copy of the 1 MiB dump, first 16 KiB = miniloader.bin (0xFF padded)."""
import sys
d = bytearray(open('../stock/re/dumps/full-20261013/flash_08000000_1MB.bin', 'rb').read())
b = open('miniloader-nolfn.bin' if '--nolfn' in sys.argv else 'miniloader.bin', 'rb').read()
assert len(b) <= 16384
d[:16384] = b + b'\xff' * (16384 - len(b))
if '--zero-marker' in sys.argv:
    d[0x8000:0x800c] = bytes(12)
open(sys.argv[1], 'wb').write(d)
