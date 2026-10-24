#!/usr/bin/env python3
"""A USB/IP mass-storage device for a real kernel, backed by the fw-rs USB
volume code (vdisk.rs + vwrite.rs through `main ... vserve`, vserve.py):
the kernel attaches it with `sudo usbip attach -r 127.0.0.1 -b 1-1`, mounts
it (udisks / GNOME), and whatever Linux writes for rm / mv to the trash /
cp / umount reaches vwrite.rs exactly as from the meter's USB port, at host
speed (no emulator). The SCSI answers follow src/msc.rs (INQUIRY removable,
MODE SENSE WP bit, READ CAPACITY / FORMAT CAPACITIES, PREVENT tracked,
UNIT ATTENTION 06/28/00 at the start of a command, a refused WRITE drains
its data phase and fails with the device's sense). The product is
"DM40 vtest" and the USB id 1209:db40, so it is never mistaken for the
real meter (1209:0001).

    usbip_msc.py <t> <spi.img> <flash1m> <srcdir> [--port 3240]
                 [--capture out.vcap.gz] [--fw fw.bin]

--capture: every state-changing request (WRITE sectors with the answer,
PREVENT / ALLOW, bus reset, main-loop ticks between them) with its time,
for vreplay.py; WRITE data equal to a 512-byte-aligned piece of --fw is
stored as a reference, zero sectors as a flag (small captures).
SIGINT / SIGTERM: the capture is closed, the SPI image saved (<spi>.out).
"""
import argparse, gzip, os, signal, socket, struct, sys, threading, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vserve import VServe, SECTOR

ap = argparse.ArgumentParser()
ap.add_argument('t')
ap.add_argument('spi')
ap.add_argument('flash')
ap.add_argument('src')
ap.add_argument('--port', type=int, default=3240)
ap.add_argument('--capture')
ap.add_argument('--fw')
ap.add_argument('--log', default=None)
ap.add_argument('--ro', action='store_true', help='the read-only volume (the firmware default, "Files live"); default: writable ("PC can edit", the write captures)')
args = ap.parse_args()

BUSID = b'1-1'
VID, PID = 0x1209, 0xDB40
DEV_DESC = bytes([18, 1, 0x00, 0x02, 0, 0, 0, 64]) + struct.pack('<HHH', VID, PID, 0x0100) + bytes([1, 2, 3, 1])
CFG_DESC = (bytes([9, 2]) + struct.pack('<H', 32) + bytes([1, 1, 0, 0x80, 50])
            + bytes([9, 4, 0, 0, 2, 8, 6, 0x50, 0])
            + bytes([7, 5, 0x81, 2]) + struct.pack('<H', 512) + bytes([0])
            + bytes([7, 5, 0x02, 2]) + struct.pack('<H', 512) + bytes([0]))
STRINGS = {1: 'ALIENTEK', 2: 'DM40 vtest', 3: 'VTEST0001'}
EPIPE, ECONNRESET = -32, -104

lock = threading.RLock()
vs = VServe(args.t, args.spi, args.flash, args.src, ro=args.ro)
T0 = time.time()
logf = open(args.log, 'a') if args.log else None


def now_ms():
    return int((time.time() - T0) * 1000)


def log(*a):
    if logf:
        print('%9.3f' % (time.time() - T0), *a, file=logf, flush=True)


class Capture:
    """vreplay.py format: b'VCAP1', then records: '<IcI' (ms, op, arg) +
    for 'W': a kind byte (0 raw + 512 bytes, 1 zeros, 2 fw piece + u32
    offset) and the answer byte."""
    def __init__(self, path, fw):
        self.f = gzip.open(path, 'wb') if path else None
        self.fw = {}
        if fw:
            d = open(fw, 'rb').read()
            for o in range(0, len(d) - SECTOR + 1, SECTOR):
                self.fw.setdefault(d[o:o + SECTOR], o)
        if self.f:
            self.f.write(b'VCAP1')

    def rec(self, op, arg, data=None, ans=None):
        if not self.f:
            return
        self.f.write(struct.pack('<IcI', now_ms(), op, arg))
        if op == b'W':
            if not any(data):
                self.f.write(b'\x01')
            elif data in self.fw:
                self.f.write(b'\x02' + struct.pack('<I', self.fw[data]))
            else:
                self.f.write(b'\x00' + data)
            self.f.write(bytes([ans]))

    def close(self):
        if self.f:
            self.f.close()
            self.f = None


cap = Capture(args.capture, args.fw)


def ticker():
    while True:
        time.sleep(0.05)
        with lock:
            vs.tick(now_ms())
        if stop.is_set():
            return


stop = threading.Event()


class Bot:
    """Bulk-only transport + SCSI, as src/msc.rs."""
    def __init__(self):
        self.sense = (0, 0, 0)
        self.inq = []        # pending IN URBs: (seq, length)
        self.inbuf = []      # IN transfers to give: bytes objects (data phase, CSW)
        self.out_left = 0    # data-out bytes still expected
        self.out = bytearray()
        self.cbw = None
        self.lost = 0

    def reset(self):
        self.inbuf = []
        self.out_left = 0
        self.out = bytearray()
        self.cbw = None

    def csw(self, tag, residue, status):
        return b'USBS' + struct.pack('<IIB', tag, residue, status)

    def fail(self, sense):
        self.sense = sense

    def command(self, cbw):
        tag, dlen, flags, lun, cblen = struct.unpack_from('<IIBBB', cbw, 4)
        cb = cbw[15:15 + cblen]
        op = cb[0]
        data, ok = b'', True
        with lock:
            if op not in (0x12, 0x03) and vs.ua():
                self.sense = (0x06, 0x28, 0x00)
                log('UA on op %#x' % op)
                ok = False
            elif op == 0x00:  # TEST UNIT READY
                pass
            elif op == 0x03:  # REQUEST SENSE
                s = self.sense
                data = bytes([0x70, 0, s[0], 0, 0, 0, 0, 10, 0, 0, 0, 0, s[1], s[2], 0, 0, 0, 0])
                self.sense = (0, 0, 0)
            elif op == 0x12:  # INQUIRY
                data = bytes([0, 0x80, 0x04, 0x02, 31, 0, 0, 0]) + b'ALIENTEK' + b'DM40 vtest      ' + b'0.2 '
            elif op == 0x1A:
                data = bytes([3, 0, 0x80 if vs.wp() else 0, 0])
            elif op == 0x5A:
                data = bytes([0, 6, 0, 0x80 if vs.wp() else 0, 0, 0, 0, 0])
            elif op == 0x25:
                data = struct.pack('>II', 4160541 - 1, 512)
            elif op == 0x23:
                data = bytes([0, 0, 0, 8]) + struct.pack('>I', 4160541) + bytes([2, 0, 2, 0])
            elif op == 0x28:  # READ(10)
                lba, n = struct.unpack_from('>I', cb, 2)[0], struct.unpack_from('>H', cb, 7)[0]
                data = b''.join(vs.read(lba + i) for i in range(n))
            elif op == 0x2A:  # WRITE(10): data first
                lba, n = struct.unpack_from('>I', cb, 2)[0], struct.unpack_from('>H', cb, 7)[0]
                self.cbw = (tag, dlen, lba, n)
                self.out_left = dlen
                self.out = bytearray()
                if dlen == 0:
                    self.finish_write()
                return
            elif op == 0x1E:  # PREVENT ALLOW MEDIUM REMOVAL
                vs.lock(cb[4] & 1)
                cap.rec(b'L', cb[4] & 1)
                log('PREVENT' if cb[4] & 1 else 'ALLOW')
            elif op == 0x1B:
                log('START STOP UNIT', cb.hex())
            else:
                self.sense = (0x05, 0x20, 0x00)
                ok = False
        if flags & 0x80 and dlen:
            data = data[:dlen]
            self.inbuf.append(bytes(data))
            self.inbuf.append(self.csw(tag, dlen - len(data), 0 if ok else 1))
        else:
            self.inbuf.append(self.csw(tag, 0, 0 if ok else 1))
        log('op %#x %s ok=%d' % (op, cb.hex(), ok))

    def finish_write(self):
        tag, dlen, lba, n = self.cbw
        d = bytes(self.out)
        st = 0
        with lock:
            for i in range(n):
                x = d[i * SECTOR:(i + 1) * SECTOR]
                r = vs.write(lba + i, x)
                cap.rec(b'W', lba + i, x, r)
                if r:
                    st = r
                    break
            if st:
                self.sense = {1: (0x03, 0x0C, 0x00), 2: (0x07, 0x27, 0x00), 3: (0x02, 0x04, 0x01)}[st]
        log('WRITE lba=%d n=%d -> %s' % (lba, n, 'ok' if not st else 'refused %d' % st))
        self.cbw = None
        self.inbuf.append(self.csw(tag, 0, 1 if st else 0))

    def bulk_out(self, data):
        if self.out_left:
            self.out += data
            self.out_left -= len(data)
            if self.out_left <= 0:
                self.out_left = 0
                self.finish_write()
            return
        if len(data) == 31 and data[:4] == b'USBC':
            self.command(data)
        else:
            log('stray OUT', len(data))


def recvn(s, n):
    b = b''
    while len(b) < n:
        d = s.recv(n - len(b))
        if not d:
            raise EOFError
        b += d
    return b


def dev_struct():
    path = b'/sys/devices/vtest/usb1/1-1'.ljust(256, b'\0')
    return (path + BUSID.ljust(32, b'\0') + struct.pack('>III', 1, 2, 3)
            + struct.pack('>HHH', VID, PID, 0x0100) + bytes([0, 0, 0, 1, 1, 1]))


def ret_submit(s, seq, status, data=b'', dirn=0, alen=None):
    if alen is None:
        alen = len(data)
    h = struct.pack('>IIIII', 3, seq, 0, dirn, 0) + struct.pack('>iiiii', status, alen, 0, 0, 0) + bytes(8)
    s.sendall(h + data)


def control(setup, out_data):
    rt, req, val, idx, ln = struct.unpack('<BBHHH', setup)
    if rt == 0x80 and req == 6:
        t, i = val >> 8, val & 0xFF
        if t == 1:
            return DEV_DESC[:ln]
        if t == 2:
            return CFG_DESC[:ln]
        if t == 3:
            if i == 0:
                return bytes([4, 3, 0x09, 0x04])[:ln]
            sd = STRINGS.get(i)
            if sd is None:
                return None
            e = sd.encode('utf-16-le')
            return (bytes([2 + len(e), 3]) + e)[:ln]
        return None
    if rt == 0x80 and req == 0:
        return bytes(2)[:ln]
    if (rt, req) in ((0x00, 9), (0x01, 11), (0x02, 1), (0x00, 1), (0x00, 3)):
        return b''
    if rt == 0xA1 and req == 0xFE:
        return b'\x00'
    if rt == 0x21 and req == 0xFF:
        bot.reset()
        return b''
    return None


bot = Bot()


def serve(s):
    pending = {}
    while True:
        h = recvn(s, 48)
        cmd, seq, devid, dirn, ep = struct.unpack_from('>IIIII', h)
        if cmd == 1:
            flags, tlen = struct.unpack_from('>Ii', h, 20)
            setup = h[40:48]
            data = recvn(s, tlen) if dirn == 0 and tlen > 0 else b''
            if ep == 0:
                r = control(setup, data)
                if r is None:
                    log('stall setup', setup.hex())
                    ret_submit(s, seq, EPIPE, dirn=dirn)
                else:
                    ret_submit(s, seq, 0, r if dirn == 1 else b'', dirn, None if dirn == 1 else len(data))
            elif dirn == 0:
                bot.bulk_out(data)
                ret_submit(s, seq, 0, b'', 0, len(data))
            else:
                bot.inq.append((seq, tlen))
            # Give IN data to waiting IN URBs.
            while bot.inq and bot.inbuf:
                q, ln = bot.inq.pop(0)
                d = bot.inbuf[0]
                part, rest = d[:ln], d[ln:]
                if rest:
                    bot.inbuf[0] = rest
                else:
                    bot.inbuf.pop(0)
                ret_submit(s, q, 0, part, 1)
        elif cmd == 2:
            useq = struct.unpack_from('>I', h, 20)[0]
            found = [x for x in bot.inq if x[0] == useq]
            bot.inq = [x for x in bot.inq if x[0] != useq]
            log('UNLINK', useq, 'found' if found else 'gone')
            st = ECONNRESET if found else 0
            s.sendall(struct.pack('>IIIII', 4, seq, 0, 0, 0) + struct.pack('>i', st) + bytes(24))
        else:
            raise EOFError('bad command %d' % cmd)


def shutdown(*_):
    stop.set()
    with lock:
        cap.close()
        try:
            vs.save()
            vs.close()
        except Exception:
            pass
    os._exit(0)


def dump_state(*_):
    with lock:
        log('STATS', vs.stats())
        log('DEBUG', vs.debug().replace('\n', ' | '))


signal.signal(signal.SIGUSR1, dump_state)
signal.signal(signal.SIGINT, shutdown)
signal.signal(signal.SIGTERM, shutdown)
threading.Thread(target=ticker, daemon=True).start()
ls = socket.socket()
ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
ls.bind(('127.0.0.1', args.port))
ls.listen(1)
print('usbip_msc: listening on', args.port, flush=True)
while True:
    c, _ = ls.accept()
    c.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    try:
        h = recvn(c, 8)
        ver, code, st = struct.unpack('>HHI', h)
        if code == 0x8005:  # DEVLIST
            c.sendall(struct.pack('>HHII', 0x0111, 5, 0, 1) + dev_struct() + bytes([8, 6, 0x50, 0]))
            c.close()
            continue
        if code == 0x8003:  # IMPORT
            busid = recvn(c, 32)
            c.sendall(struct.pack('>HHI', 0x0111, 3, 0) + dev_struct())
            log('imported')
            with lock:
                vs.bus_reset()
                cap.rec(b'B', 0)
            bot.reset()
            serve(c)
    except (EOFError, ConnectionError) as e:
        log('connection closed', e)
        with lock:
            vs.bus_reset()
            cap.rec(b'B', 0)
    finally:
        c.close()
