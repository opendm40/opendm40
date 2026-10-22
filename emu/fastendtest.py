#!/usr/bin/env python3
"""fw-rs: the end of a fast-logger disk session (fastlog.rs, LOG:DEST DISK)
while a host reads the USB volume, then MMEM:DATA? of the new file
(device check f3aea2f 6b: the meter hung there and the watchdog reset it).

  cd fw-rs && ./build.sh
  emu/fastendtest.py --app fw-rs/target/native/thumbv7em-none-eabihf/release/dm40-fw

One USB/IP connection with the URBs demultiplexed by sequence number, so
the volume reader (MSC, its own thread) and the SCPI client (USBTMC) run
at the same time, as on a host:
  1. a first AD2 session (--rate, --dur1 s) makes FASTa;
  2. a second session (--dur s) with a thread reading FASTa.CSV over the
     volume in 64-sector READ(10)s all the time; at the UNIT ATTENTION of
     the session's volume refresh the reader does what Linux does
     (REQUEST SENSE, TEST UNIT READY, READ CAPACITY, the boot sector, the
     partition-table re-read, the FAT, the directories) and goes on, then
     switches to the new FASTb.CSV;
  3. right after LOG:DISK? says done: LOG:DEST HOST, LOG:COUNt?, and
     MMEM:DATA? "LOGS/FASTb.CSV" in 6000-byte pieces (--mmem pieces);
  4. every SCPI reply and every SCSI command must come back within
     --timeout s (wall), the uptime (DBG:PING?) must not restart, and the
     CSV pieces must match the volume's bytes of the same file.
Exit status 0 = PASS. Per-process names (port, out/ files) so it can run
next to other emulator tests; the Renode log is capped by run.py.
"""
import argparse
import os
import queue
import shutil
import socket
import struct
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='fw-rs native ELF / image')
ap.add_argument('--port', type=int, default=4827, help='USB/IP port (default 4827)')
ap.add_argument('--rate', type=int, default=19200, help='AD2 S/s (default 19200)')
ap.add_argument('--dur1', type=int, default=3, help='first session, s (device time)')
ap.add_argument('--dur', type=int, default=4, help='second session, s (device time)')
ap.add_argument('--mmem', type=int, default=200, help='MMEM:DATA? pieces after the session (default 200)')
ap.add_argument('--timeout', type=float, default=240, help='one reply / SCSI command at most, s wall')
ap.add_argument('--mips', default='200')
ap.add_argument('--keep', action='store_true', help='leave the emulator running at the end (debug)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, HERE)
import usbip_client as uc  # noqa: E402

OUT = os.path.join(HERE, 'out')
TAG = 'fastend-%d-%d' % (A.port, os.getpid())
FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


class Mux(uc.Dev):
    """uc.Dev with the RET_SUBMITs dispatched by seq (concurrent URBs)."""

    def __init__(self, busid):
        super().__init__(busid)
        self.s.settimeout(None)
        self.lk = threading.Lock()
        self.dirs = {}
        self.ret = {}
        self.cv = threading.Condition()
        self.dead = None
        threading.Thread(target=self._rx, daemon=True).start()

    def _rx(self):
        try:
            while True:
                rh = uc.recvn(self.s, 48)
                cmd, seq, _, _, _, status, actual = struct.unpack('>IIIIIIi', rh[:28])
                if cmd != uc.RET_SUBMIT:
                    raise IOError('unexpected reply cmd 0x%x' % cmd)
                with self.cv:
                    d = self.dirs.pop(seq, uc.DIR_OUT)
                data = uc.recvn(self.s, actual) if d == uc.DIR_IN and actual > 0 else b''
                with self.cv:
                    self.ret[seq] = (status, data)
                    self.cv.notify_all()
        except Exception as e:
            with self.cv:
                self.dead = e
                self.cv.notify_all()

    def send(self, direction, ep, setup=b'\0' * 8, out_data=b'', in_len=0):
        with self.lk:
            self.seq += 1
            seq = self.seq
            with self.cv:
                self.dirs[seq] = direction
            buflen = len(out_data) if direction == uc.DIR_OUT else in_len
            flags = 0x200 if direction == uc.DIR_IN else 0
            hdr = struct.pack('>IIIIIIIiiI', uc.CMD_SUBMIT, seq, self.devid, direction, ep, flags, buflen, 0, 0, 0) + setup
            self.s.sendall(hdr + (out_data if direction == uc.DIR_OUT else b''))
        return seq

    def wait(self, seq, timeout):
        end = time.time() + timeout
        with self.cv:
            while seq not in self.ret:
                if self.dead:
                    raise IOError('USB/IP connection: %r' % self.dead)
                left = end - time.time()
                if left <= 0:
                    raise TimeoutError('URB %d: no answer in %g s' % (seq, timeout))
                self.cv.wait(left)
            return self.ret.pop(seq)

    def submit(self, direction, ep, setup=b'\0' * 8, out_data=b'', in_len=0):
        return self.wait(self.send(direction, ep, setup, out_data, in_len), A.timeout)


class Tmc:
    def __init__(self, d):
        _, self.ep_out, self.ep_in = uc.find_bulk(uc.parse_config(d.config_descriptor()), 0xFE, 3)
        self.d, self.tag = d, 0

    def _tag(self):
        self.tag = self.tag % 255 + 1
        return self.tag

    def write(self, text):
        msg = text.encode() + b'\n'
        t = self._tag()
        hdr = struct.pack('<BBBBIB3x', 1, t, 0xFF ^ t, 0, len(msg), 1)
        self.d.bulk_out(self.ep_out, hdr + msg + b'\0' * ((-len(msg)) % 4))

    def q(self, text, raw=False, maxlen=8192):
        t0 = time.time()
        self.write(text)
        if '?' not in text:
            return None
        t = self._tag()
        self.d.bulk_out(self.ep_out, struct.pack('<BBBBIBB2x', 2, t, 0xFF ^ t, 0, maxlen, 0, 0))
        resp = b''
        while True:
            try:
                st, data = self.d.submit(uc.DIR_IN, self.ep_in & 0x7F, in_len=maxlen + 12)
            except TimeoutError:
                raise TimeoutError('%s: no reply in %.0f s' % (text, time.time() - t0))
            if st != 0:
                raise IOError('%s: bulk_in status %d' % (text, st))
            resp += data
            if len(resp) >= 12 and len(resp) >= 12 + struct.unpack_from('<I', resp, 4)[0]:
                b = resp[12:12 + struct.unpack_from('<I', resp, 4)[0]]
                return b if raw else b.decode(errors='replace').strip()


class ProtoErr(IOError):
    pass


T0 = time.time()


class Vol:
    """The volume as a host reads it: FAT16 layout from the boot sector."""

    def __init__(self, d):
        self.m = uc.Msc(d)
        self.lock = threading.RLock()
        self.n_ua = 0
        self.n_cmd = 0
        self.proto = []

    def cmd(self, cb, n=0):
        """One BOT command (CBW, data in, CSW); a protocol error raises
        ProtoErr with what came back so far."""
        with self.lock:
            self.n_cmd += 1
            m = self.m
            m.tag += 1
            cbw = struct.pack('<4sIIBBB16s', b'USBC', m.tag, n, 0x80, 0, len(cb), cb.ljust(16, b'\0'))
            m.d.bulk_out(m.ep_out, cbw)
            data = b''
            chunks = []
            try:
                while len(data) < n:
                    st, c = m.d.submit(uc.DIR_IN, m.ep_in & 0x7F, in_len=min(4096, n - len(data)))
                    chunks.append((st, len(c)))
                    if st != 0:
                        raise ProtoErr('data phase: URB status %d after %d/%d bytes' % (st - (1 << 32) if st > 1 << 31 else st, len(data), n))
                    if not c:
                        break
                    data += c
                    if len(c) % 64:
                        break
                st, csw = m.d.submit(uc.DIR_IN, m.ep_in & 0x7F, in_len=13)
                if st != 0:
                    raise ProtoErr('CSW: URB status %d (%d data bytes of %d, chunks %s)' % (st - (1 << 32) if st > 1 << 31 else st, len(data), n, chunks[-4:]))
                sig, rtag, residue, status = struct.unpack('<4sIIB', csw)
                if sig != b'USBS' or rtag != m.tag:
                    raise ProtoErr('bad CSW %s (%d data bytes of %d)' % (csw[:16].hex(), len(data), n))
            except ProtoErr as e:
                e.args = ('%s, CDB %s, t %.3f' % (e.args[0], cb[:10].hex(), time.time() - T0),)
                self.proto.append(e.args[0])
                print('  MSC protocol error: %s' % e.args[0], flush=True)
                raise
            return status, data

    def read(self, lba, n):
        st, data = self.cmd(struct.pack('>BBIBHB', 0x28, 0, lba, 0, n, 0), n * 512)
        if st != 0:
            s = self.sense()
            raise IOError('READ(10) %d x %d: sense %s' % (lba, n, s))
        return data

    def sense(self):
        st, d = self.cmd(bytes([0x03, 0, 0, 0, 18, 0]), 18)
        s = '%02X/%02X/%02X' % (d[2] & 15, d[12], d[13]) if len(d) >= 14 else '?'
        if s.startswith('06/'):
            self.n_ua += 1
        return s

    def revalidate(self):
        """Linux after a UNIT ATTENTION 06/28/00: TUR, READ CAPACITY,
        the partition-table read, blkid-style probes, the FAT and dirs."""
        for _ in range(4):
            st, _ = self.cmd(bytes(6))
            if st == 0:
                break
            self.sense()
        st, cap = self.cmd(bytes([0x25]) + bytes(9), 8)
        last = struct.unpack('>I', cap[:4])[0] if st == 0 and len(cap) >= 4 else 0
        self.read(0, 8)
        if last:
            self.read(last - 7, 8)  # the end of the device (GPT backup probe)
        self.layout()

    def layout(self):
        b = self.read(0, 1)
        _, spc, rsvd, nfat, nroot = struct.unpack_from('<HBHBH', b, 11)
        fatsz = struct.unpack_from('<H', b, 22)[0]
        self.spc = spc
        self.fat = rsvd
        self.root = rsvd + nfat * fatsz
        self.data = self.root + nroot * 32 // 512
        self.read(self.fat, 8)

    def lba(self, c):
        return self.data + (c - 2) * self.spc

    def find(self, secs, name):
        for s_ in secs:
            d = self.read(s_, 1)
            for k in range(16):
                e = d[k * 32:k * 32 + 32]
                if e[0] == 0:
                    return None
                if e[:11] == name:
                    return struct.unpack_from('<H', e, 26)[0], struct.unpack_from('<I', e, 28)[0]
        return None

    def file(self, name):
        logs = self.find(range(self.root, self.data), b'LOGS       ')
        return logs and self.find(range(self.lba(logs[0]), self.lba(logs[0]) + self.spc), name)


def run_emu():
    disk = os.path.join(OUT, TAG + '-disk.bin')
    src = os.path.join(OUT, 'disk.bin')
    if not os.path.exists(src):
        shutil.copy(os.path.join(ROOT, 'firmware/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin'), disk)
    else:
        shutil.copy(src, disk)
    script = os.path.join(OUT, TAG + '.script')
    with open(script, 'w') as f:
        f.write('0 monitor sysbus.spi2.spiflash ProgramUs 400\n')
        f.write('0 monitor sysbus.spi2.spiflash Erase4kUs 45000\n')
        f.write('0 monitor sysbus.spi2.spiflash Erase32kUs 120000\n')
        f.write('0 monitor sysbus.spi2.spiflash Erase64kUs 150000\n')
    cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
           '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '5000', '--name', TAG,
           '--disk', disk, '--dmm', 'vsrc:0.2,ac=0.5,f=50', '--script', script]
    return subprocess.Popen(cmd, cwd=ROOT, stdout=open(os.path.join(OUT, TAG + '.out'), 'w'),
                            stderr=subprocess.STDOUT, start_new_session=True), disk, script


def connect():
    for _ in range(150):
        try:
            return Mux('1-0')
        except Exception:
            time.sleep(2)
    sys.exit('fastendtest: no USB/IP device on port %d' % A.port)


def session(t, dur, during=None):
    t.q('LOG:STATe OFF')
    t.q('CONF:VOLT:AC')
    t.q('LOG:SOURce AD2')
    t.q('LOG:SRATe %d' % A.rate)
    time.sleep(2)
    t.q('LOG:DEST DISK')
    t.q('LOG:DURation %d' % dur)
    t.q('*CLS')
    t.q('LOG:STATe ON')
    e = t.q('SYST:ERR?')
    check(e.startswith('0,'), 'LOG:STATe ON accepted (%s)' % e)
    st = []
    while True:
        st = [int(x) for x in t.q('LOG:DISK?').split(',')]
        if not st or st[0] != getattr(session, 'last', None):
            print('  t %.3f LOG:DISK? %s' % (time.time() - T0, st), flush=True)
            session.last = st[0]
        if during and st[0] == 3:
            during()
            during = None
        if st[0] >= 5:
            return st
        time.sleep(0.2)


emu, disk, script = run_emu()
reader_err = []
try:
    d = connect()
    t = Tmc(d)
    d.set_configuration(1)
    print('*IDN?', t.q('*IDN?'), flush=True)
    up0 = int(t.q('DBG:PING?').split(',')[0])

    st1 = session(t, A.dur1)
    t.q('LOG:DEST HOST')
    check(st1[0] == 5, 'first session done (LOG:DISK? %s)' % st1)
    na = st1[1]
    v = Vol(d)
    # The volume refresh after the first session (1 s quiet).
    fa = None
    for _ in range(60):
        time.sleep(1)
        try:
            v.revalidate()
            fa = v.file(b'FAST%03d CSV' % na)
        except IOError:
            continue
        if fa:
            break
    check(bool(fa), 'FAST%03d.CSV on the volume (%s)' % (na, fa))

    # The reader thread: FASTa.CSV round and round; after a UA the host's
    # revalidation, then on (FASTb once it is listed).
    stop = threading.Event()
    target = {'name': b'FAST%03d CSV' % na}
    stats = {'bytes': 0, 'ua': 0, 'files': set()}

    def reader():
        cur = fa
        name = target['name']
        off = 0
        while not stop.is_set():
            try:
                if target['name'] != name:
                    f = v.file(target['name'])
                    if f:
                        name, cur, off = target['name'], f, 0
                        stats['files'].add(name)
                start, size = cur
                if off >= size:
                    off = 0
                k = min(64, (size - off + 511) // 512)
                b = v.read(v.lba(start) + off // 512, k)
                stats['bytes'] += len(b)
                off += k * 512
            except TimeoutError as e:
                reader_err.append('volume: %s' % e)
                return
            except ProtoErr as e:
                reader_err.append('volume: %s' % e)
                return
            except IOError as e:
                if 'sense 06/' in str(e):
                    stats['ua'] += 1
                    try:
                        v.revalidate()
                        f = v.file(name)
                        if f:
                            cur = f
                        else:
                            off = 0
                    except TimeoutError as e2:
                        reader_err.append('volume (revalidate): %s' % e2)
                        return
                    except IOError:
                        pass
                    continue
                reader_err.append('volume: %s' % e)
                return

    th = threading.Thread(target=reader, daemon=True)
    st2 = session(t, A.dur, during=th.start)
    # Exactly what dm40scpi.py fast does next.
    t.q('LOG:DEST HOST')
    cnt = t.q('LOG:COUNt?')
    nb = st2[1]
    check(st2[0] == 5, 'second session done (LOG:DISK? %s, LOG:COUNt? %s)' % (st2, cnt))
    target['name'] = b'FAST%03d CSV' % nb
    name = 'LOGS/FAST%03d.CSV' % nb
    got = b''
    t0 = time.time()
    for i in range(A.mmem):
        b = t.q('MMEM:DATA? "%s",%d' % (name, len(got)), raw=True)
        n = int(b[2:6])
        got += bytes(b[6:6 + n])
        if n < 6000:
            break
    print('  MMEM:DATA? %s: %d bytes in %.1f s wall' % (name, len(got), time.time() - t0), flush=True)
    check(len(got) > 0, 'MMEM:DATA? of the new file answered (%d bytes)' % len(got))
    # The same bytes off the volume.
    fb = None
    for _ in range(30):
        try:
            with v.lock:
                fb = v.file(b'FAST%03d CSV' % nb)
        except IOError:
            pass
        if fb:
            break
        time.sleep(1)
    stop.set()
    th.join(A.timeout)
    check(not reader_err, 'the volume reader never waited > %g s (%s)' % (A.timeout, '; '.join(reader_err) or
                                                                          '%d bytes, %d UNIT ATTENTION' % (stats['bytes'], stats['ua'])))
    check(stats['ua'] >= 1, 'the session end raised a UNIT ATTENTION on the reader (%d)' % stats['ua'])
    if fb:
        vb = v.read(v.lba(fb[0]), (len(got) + 511) // 512)[:len(got)]
        check(vb == got, 'MMEM:DATA? bytes == the volume\'s FAST%03d.CSV (%d bytes)' % (nb, len(got)))
    else:
        check(False, 'FAST%03d.CSV on the volume' % nb)
    up1 = int(t.q('DBG:PING?').split(',')[0])
    boot = t.q('DBG:BOOT?')
    check(up1 > up0, 'no reboot (uptime %d -> %d ms; DBG:BOOT? %s)' % (up0, up1, boot))
    isr = t.q('DBG:ISR?')
    print('  DBG:ISR? %s' % isr, flush=True)
except Exception as e:
    check(False, 'the meter answers: %r' % e)
finally:
    if not A.keep:
        try:
            os.killpg(emu.pid, 15)
        except Exception:
            pass
        emu.wait()
        for p in (disk, script):
            try:
                os.remove(p)
            except OSError:
                pass

print('FAIL: %d checks' % len(FAIL) if FAIL else 'PASS')
sys.exit(1 if FAIL else 0)
