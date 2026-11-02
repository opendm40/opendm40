#!/usr/bin/env python3
"""mklfndisk.py BASE OUT: the base FAT16 image + DM40 files with long names; two with deliberately broken LFN entries."""
import os, shutil, struct, subprocess, sys, tempfile
base, out = sys.argv[1:3]
shutil.copy(base, out)
env = dict(os.environ, MTOOLS_SKIP_CHECK='1', LC_ALL='C.UTF-8')
names = ['dm40_opendm40_v0.4.0.bin',
         'dm40_a_very_long_file_name_that_does_not_fit_on_the_screen_v1.bin',
         'dm40_тест_ß.bin',
         'dm40_badsum_long_name.bin',
         'dm40_broken_sequence_name.bin',
         'DM40_UPPER_Long_Mixed~(1).bin'] + ['dm40_filler_number_%02d.bin' % i for i in range(11)]
with tempfile.NamedTemporaryFile(dir=os.environ.get('TMPDIR')) as f:
    f.write(b'x' * 100); f.flush()
    for n in names:
        subprocess.check_call(['mcopy', '-o', '-i', out, f.name, '::' + n], env=env)
d = bytearray(open(out, 'rb').read())
bps, = struct.unpack_from('<H', d, 11)
rsv, = struct.unpack_from('<H', d, 14)
fsz, = struct.unpack_from('<H', d, 22)
root = (rsv + d[16] * fsz) * bps
AT = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30]
def part(e):
    return bytes(e[o] for o in AT[:10:2]).decode('latin1')
for i in range(512):
    o = root + 32 * i
    e = d[o:o + 32]
    if e[0] == 0: break
    if e[11] != 0x0F or e[0] & 0x40 == 0: continue
    n = e[0] & 0x1F
    txt = ''.join(''.join(chr(struct.unpack_from('<H', d, root + 32 * (i + k) + a)[0]) for a in AT if struct.unpack_from('<H', d, root + 32 * (i + k) + a)[0] not in (0, 0xffff)) for k in range(n - 1, -1, -1))
    if txt == 'dm40_badsum_long_name.bin':
        d[o + 13] ^= 0x55
        for k in range(n): d[o + 32 * k + 13] = d[o + 13]   # consistent, but not the short entry's
        print('wrong checksum:', txt)
    if txt == 'dm40_broken_sequence_name.bin':
        d[o + 32] = (d[o + 32] & 0xE0) | ((d[o + 32] & 0x1F) + 5 & 0x1F)
        print('broken sequence:', txt)
open(out, 'wb').write(d)
