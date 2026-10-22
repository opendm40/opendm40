"""Client of `main <spi.img> vserve ...` (tools/fatren-test/main.rs): the
generated USB volume (src/vdisk.rs + src/vwrite.rs) over a SPI image, one
request at a time on the child's stdin / stdout. Shared by vwrite_test.py
(sector-diff scenarios, power cuts, replays) and usbip_msc.py (a USB/IP
mass-storage device for a real kernel)."""
import os, struct, subprocess

SECTOR = 512
# vwrite.rs last_error + 1 (0 = accepted).
W_OK, W_IO, W_REFUSED, W_BUSY = 0, 1, 2, 3


class VServe:
    def __init__(self, t, spi, flash, src, cut=None):
        args = [t, spi, 'vserve', flash, src + '/UID.BIN', src + '/OTP.BIN', src + '/OPTBYTES.BIN']
        if cut is not None:
            args += ['cut', str(cut)]
        self.spi = spi
        self.p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        self.now = 0

    def _op(self, c, arg=0, data=b''):
        self.p.stdin.write(c + struct.pack('<I', arg & 0xFFFFFFFF) + data)
        self.p.stdin.flush()

    def _r(self, n):
        d = self.p.stdout.read(n)
        if len(d) != n:
            raise EOFError('vserve died (rc %s)' % self.p.poll())
        return d

    def _1(self):
        return self._r(1)[0]

    def read(self, lba):
        self._op(b'R', lba)
        st = self._1()
        d = self._r(SECTOR)
        assert st == 0, 'read %d failed' % lba
        return d

    def write(self, lba, data):
        assert len(data) == SECTOR
        self._op(b'W', lba, data)
        return self._1()

    def tick(self, ms=None):
        """The main loop at `ms` (default: 10 ms later)."""
        self.now = self.now + 10 if ms is None else ms
        self._op(b'T', self.now)
        self._1()

    def idle(self, ms, step=200):
        """Main-loop passes for `ms`."""
        end = self.now + ms
        while self.now < end:
            self.tick(min(end, self.now + step))

    def lock(self, on):
        self._op(b'L', 1 if on else 0)
        self._1()

    def bus_reset(self):
        self._op(b'B')
        self._1()

    def ua(self):
        self._op(b'U')
        return self._1() == 1

    def wp(self):
        self._op(b'P')
        return self._1() == 1

    def dialog(self):
        self._op(b'D')
        return self._1()

    def answer(self, yes):
        self._op(b'A', 1 if yes else 0)
        self._1()

    def install(self):
        self._op(b'I')
        return self._1() == 1

    def save(self):
        self._op(b'S')
        self._1()
        return self.spi + '.out'

    def dump(self, n=0):
        self._op(b'V', n)
        self._1()
        return '%s.vol%d' % (self.spi, n)

    def stats(self):
        self._op(b'Q')
        n = struct.unpack('<H', self._r(2))[0]
        t = self._r(n).decode()
        return {k: v for k, v in (x.split('=') for x in t.split())}

    def debug(self):
        self._op(b'Z')
        n = struct.unpack('<H', self._r(2))[0]
        return self._r(n).decode()

    def gallery_delete(self, name83):
        assert len(name83) == 11
        self._op(b'G', 0, name83)
        return self._1() == 0

    def close(self):
        try:
            self._op(b'X')
        except BrokenPipeError:
            pass
        self.p.wait()


def extents(path):
    """Data extents (start, end) of a sparse file, in bytes."""
    out = []
    with open(path, 'rb') as f:
        fd = f.fileno()
        size = os.fstat(fd).st_size
        off = 0
        while off < size:
            try:
                s = os.lseek(fd, off, os.SEEK_DATA)
            except OSError:
                break
            e = os.lseek(fd, s, os.SEEK_HOLE)
            out.append((s, e))
            off = e
    return out


def sectors(path):
    """{lba: data} of every non-zero sector of a sparse image."""
    res = {}
    with open(path, 'rb') as f:
        for s, e in extents(path):
            s -= s % SECTOR
            f.seek(s)
            d = f.read(e - s)
            for i in range(0, len(d), SECTOR):
                x = d[i:i + SECTOR]
                if len(x) == SECTOR and any(x):
                    res[(s + i) // SECTOR] = x
    return res


def diff(a, b):
    """Sectors of image b that differ from image a: [(lba, data)]."""
    sa, sb = sectors(a), sectors(b)
    z = bytes(SECTOR)
    out = []
    for lba in sorted(set(sa) | set(sb)):
        x, y = sa.get(lba, z), sb.get(lba, z)
        if x != y:
            out.append((lba, y))
    return out


class Geo:
    """FAT16 geometry of a volume image."""
    def __init__(self, path):
        with open(path, 'rb') as f:
            b = f.read(512)
        bps, spc, rsvd, nf, rootn, tot16, media, fsz = struct.unpack_from('<HBHBHHBH', b, 0x0B)
        self.spc, self.rsvd, self.nf, self.fsz = spc, rsvd, nf, fsz
        self.root = rsvd + nf * fsz
        self.data = self.root + rootn * 32 // bps
        self.path = path

    def kind(self, lba):
        if lba == 0:
            return 'boot'
        if lba < self.root:
            return 'fat'
        if lba < self.data:
            return 'root'
        return 'data'

    def cluster(self, lba):
        return (lba - self.data) // self.spc + 2

    def fat(self):
        with open(self.path, 'rb') as f:
            f.seek(self.rsvd * 512)
            d = f.read(self.fsz * 512)
        return struct.unpack('<%dH' % (len(d) // 2), d)
