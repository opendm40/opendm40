"""The USB volume's INFO/ files and the slow flash bank (fw-rs vdisk.rs
`read_busy`, device b6909d6): while the AD2 stream ISR runs, a READ(10) of a
sector served from internal flash >= 0x08080000 answers CHECK CONDITION, NOT
READY, BECOMING READY (02/04/01) and the host retries. Used by
fastlogtest.py and hy3131/wavetest.py.
"""
import struct

# The INFO/ files with slow-bank sectors and the file offset of the first:
# CALIB (0x080C0000), BOOTDATA (0x080A4000), LOADER (0x080A0000) whole;
# FIRMWARE.BIN (body from 0x08004000, 12-byte header) from the sector holding
# file offset 12 + 0x7C000 (flash 0x08080000).
SLOW_FILES = {'CALIB.BIN': 0, 'BOOTDATA.BIN': 0, 'LOADER.BIN': 0, 'FIRMWARE.BIN': (12 + 0x7C000) // 512 * 512}
NOT_READY = '02/04/01'


def vol_dir(m, dirname):
    """{NAME.EXT: (first LBA, size)} of the files in a root directory of the
    generated volume (contiguous files, vdisk.rs)."""
    m.cmd(0, bytes(6))  # TEST UNIT READY
    b = m.read(0, 0, 1, 512)
    spc, rsvd, nfat, nroot = struct.unpack_from('<BHBH', b, 13)
    fatsz = struct.unpack_from('<H', b, 22)[0]
    root = rsvd + nfat * fatsz
    data = root + nroot * 32 // 512
    lba = lambda c: data + (c - 2) * spc

    def entries(secs):
        for s_ in secs:
            d = m.read(0, s_, 1, 512)
            for k in range(16):
                e = d[k * 32:k * 32 + 32]
                if e[0] == 0:
                    return
                if e[0] != 0xE5 and e[11] != 0x0F:
                    yield e
    for e in entries(range(root, data)):
        if e[:11] == dirname.ljust(11).encode():
            c = struct.unpack_from('<H', e, 26)[0]
            out = {}
            for f in entries(range(lba(c), lba(c) + spc)):
                if f[11] & 0x10 == 0 and f[0] != ord('.'):
                    name = (f[:8].decode().strip() + '.' + f[8:11].decode().strip()).rstrip('.')
                    out[name] = (lba(struct.unpack_from('<H', f, 26)[0]), struct.unpack_from('<I', f, 28)[0])
            return out
    return {}


def read(m, lba, n):
    """READ(10) of n sectors: bytes, or None when refused NOT READY 02/04/01
    (any other error raises)."""
    try:
        return m.read(0, lba, n, 512, retries=2)
    except IOError as e:
        if NOT_READY in str(e):
            return None
        raise


def read_info(m):
    """Every INFO/ file in 64-sector READs, up to its first refusal:
    ({name: file offset of the first refused sector}, [names read whole],
    [other errors])."""
    refused, whole, other = {}, [], []
    for name, (lba, size) in sorted(vol_dir(m, 'INFO').items()):
        off = 0
        while off < size:
            n = min(64, (size - off + 511) // 512)
            try:
                b = read(m, lba + off // 512, n)
            except IOError as e:
                other.append('%s @%#x: %s' % (name, off, e))
                break
            if b is None:
                for i in range(n):
                    if read(m, lba + off // 512 + i, 1) is None:
                        refused[name] = off + i * 512
                        break
                break
            off += n * 512
        else:
            whole.append(name)
    return refused, whole, other
