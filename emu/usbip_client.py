#!/usr/bin/env python3
"""Userspace USB/IP client for the emulated DM40 (no kernel vhci, no root).

Talks the USB/IP protocol to Renode's server (emu/run.py ... --usbip T, TCP
3240) and drives the device over EP0 and bulk endpoints:

    emu/usbip_client.py list
    emu/usbip_client.py desc   1-0              # device descriptor
    emu/usbip_client.py config 1-0              # interfaces and endpoints
    emu/usbip_client.py usbtmc 1-0 "*IDN?"      # fw-rs: SCPI over USBTMC
    emu/usbip_client.py msc    1-0 [LBA [LUN]]  # INQUIRY, READ CAPACITY, READ(10)
    emu/usbip_client.py luns   1-0              # GET MAX LUN + per-LUN checks (INQUIRY,
                                                # TEST UNIT READY, READ CAPACITY, MODE
                                                # SENSE 6/10 WP bit, WRITE(10) -> sense,
                                                # PREVENT/ALLOW)
    emu/usbip_client.py mscdump 1-0 LUN OUT     # FAT volume of LUN -> OUT: boot sector,
                                                # FATs, root, allocated clusters (the rest
                                                # zero), for fsck.fat / mtools
    emu/usbip_client.py upload 1-0 FILE         # fw-rs DBG:UPLOAD: FILE -> DM40FW.BIN in the
                                                # SPI disk's root (as dm40dbg.py upload)
    emu/usbip_client.py read   1-0 0x1FFF7A10 12   # usbrw vendor EP0 read
    emu/usbip_client.py write  1-0 0x20028FC0 02   # usbrw vendor EP0 write
    emu/usbip_client.py scpi-tcp 1-0 5025       # line-based SCPI over TCP <-> USBTMC
                                                # (libsigrok conn=tcp-raw/127.0.0.1/5025)

Renode exports a registered device as busid 1-0. Protocol framing after the
ep0-memrw session's emu-usbtest/usbip/usbip_client.py.
"""
import os
import socket
import struct
import sys

HOST, PORT = '127.0.0.1', int(os.environ.get('USBIP_PORT', 3240))  # USBIP_PORT: a server on another port
VERSION = 0x0111
OP_REQ_DEVLIST = 0x8005
OP_REQ_IMPORT = 0x8003
CMD_SUBMIT = 1
RET_SUBMIT = 3
DIR_OUT, DIR_IN = 0, 1
HANDSHAKE_S = float(os.environ.get('USBIP_TIMEOUT', 5))  # DEVLIST / IMPORT answer deadline, s
# Renode's DEVLIST / IMPORT read the device descriptor over EP0 and wait for the firmware to answer.
# A request that timed out does not wedge the server: once the firmware answers, the next one works
# (checked with fw-rs paused / resumed). The stock firmware never answers in the emulator, not even after
# Settings > Other > USB Connect (its USB does not enumerate here; fw-rs enables USB at boot).
NO_ANSWER = ('no answer from the USB/IP server on %s:%d within %g s: the firmware has not enabled USB yet - '
             'the stock firmware needs Settings > Other > USB Connect (and even then does not enumerate in the '
             'emulator; fw-rs enables USB at boot); restart the emulator if the server stays stuck')


class NoAnswer(IOError):
    pass


def port_busy(port, host=HOST):
    """True when a TCP port is already taken (another emulator's USB/IP or BT
    server): a test that started its emulator there would talk to the other
    run's device, or find none ("no USB/IP device"). Parallel runs need
    distinct ports."""
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        s.bind((host, port))
        return False
    except OSError:
        return True
    finally:
        s.close()


def recvn(s, n):
    b = b''
    while len(b) < n:
        c = s.recv(n - len(b))
        if not c:
            raise IOError('connection closed (%d/%d bytes)' % (len(b), n))
        b += c
    return b


def op_header(code):
    return struct.pack('>HHI', VERSION, code, 0)


def handshake_reply(s):
    """The 8-byte op reply header, or NoAnswer after HANDSHAKE_S."""
    s.settimeout(HANDSHAKE_S)
    try:
        h = recvn(s, 8)
    except (TimeoutError, socket.timeout):
        s.close()
        raise NoAnswer(NO_ANSWER % (HOST, PORT, HANDSHAKE_S))
    s.settimeout(20)
    return h


def devlist():
    s = socket.create_connection((HOST, PORT), timeout=20)
    s.sendall(op_header(OP_REQ_DEVLIST))
    _, _, status = struct.unpack('>HHI', handshake_reply(s))
    if status != 0:
        raise IOError('DEVLIST status %d' % status)
    (ndev,) = struct.unpack('>I', recvn(s, 4))
    devs = []
    for _ in range(ndev):
        d = recvn(s, 0x138)
        busid = d[256:288].split(b'\0')[0].decode('latin1')
        (busnum, devnum, speed, idv, idp, bcd) = struct.unpack('>IIIHHH', d[288:306])
        (dcl, dscl, dproto, cfgv, ncfg, nif) = struct.unpack('>BBBBBB', d[306:312])
        ifs = [struct.unpack('>BBBB', recvn(s, 4))[:3] for _ in range(nif)]
        devs.append(dict(busid=busid, idVendor=idv, idProduct=idp, cls=dcl, ifs=ifs))
    s.close()
    return devs


class Dev:
    def __init__(self, busid):
        self.busid = busid
        self.s = socket.create_connection((HOST, PORT), timeout=20)
        self.s.sendall(op_header(OP_REQ_IMPORT) + busid.encode('latin1').ljust(32, b'\0'))
        _, _, status = struct.unpack('>HHI', handshake_reply(self.s))
        if status != 0:
            raise IOError('IMPORT refused, status %d' % status)
        d = recvn(self.s, 0x138)
        busnum, devnum = struct.unpack('>II', d[288:296])
        self.devid = (busnum << 16) | devnum
        self.seq = 0
        self.idVendor, self.idProduct = struct.unpack('>HH', d[296:300])

    def submit(self, direction, ep, setup=b'\0' * 8, out_data=b'', in_len=0):
        self.send_submit(direction, ep, setup, out_data, in_len)
        return self.recv_ret(direction)

    def send_submit(self, direction, ep, setup=b'\0' * 8, out_data=b'', in_len=0):
        self.seq += 1
        buflen = len(out_data) if direction == DIR_OUT else in_len
        flags = 0x200 if direction == DIR_IN else 0  # URB_DIR_IN: Renode reads the direction from transfer_flags
        hdr = struct.pack('>IIIIIIIiiI', CMD_SUBMIT, self.seq, self.devid, direction, ep,
                          flags, buflen, 0, 0, 0) + setup
        self.s.sendall(hdr + (out_data if direction == DIR_OUT else b''))

    def recv_ret(self, direction):
        rh = recvn(self.s, 48)
        cmd, seq, devid, dirn, rep, status, actual = struct.unpack('>IIIIIIi', rh[:28])
        if cmd != RET_SUBMIT:
            raise IOError('unexpected reply cmd 0x%x' % cmd)
        data = b''
        if direction == DIR_IN and actual > 0:
            data = recvn(self.s, actual)
        return status, data

    # ---- EP0 ----
    def control_in(self, bmReqType, bReq, wValue, wIndex, length):
        st, data = self.submit(DIR_IN, 0, struct.pack('<BBHHH', bmReqType, bReq, wValue, wIndex, length), in_len=length)
        if st != 0:
            raise IOError('control_in status %d' % st)
        return data

    def control_out(self, bmReqType, bReq, wValue, wIndex, data=b''):
        st, _ = self.submit(DIR_OUT, 0, struct.pack('<BBHHH', bmReqType, bReq, wValue, wIndex, len(data)), out_data=data)
        if st != 0:
            raise IOError('control_out status %d' % st)

    def device_descriptor(self):
        return self.control_in(0x80, 6, 0x0100, 0, 18)

    def config_descriptor(self):
        head = self.control_in(0x80, 6, 0x0200, 0, 9)
        total = struct.unpack_from('<H', head, 2)[0]
        return self.control_in(0x80, 6, 0x0200, 0, total)

    def set_configuration(self, n=1):
        self.control_out(0x00, 9, n, 0)

    # ---- bulk ----
    def bulk_out(self, ep, data):
        st, _ = self.submit(DIR_OUT, ep & 0x7F, out_data=data)
        if st != 0:
            raise IOError('bulk_out ep%d status %d' % (ep, st))

    def bulk_in(self, ep, length):
        st, data = self.submit(DIR_IN, ep & 0x7F, in_len=length)
        if st != 0:
            raise IOError('bulk_in ep%d status %d' % (ep, st))
        return data

    def close(self):
        self.s.close()


def parse_config(cfg):
    """-> list of interfaces {num, cls, sub, proto, eps: [(addr, attr, mps)]}"""
    ifs, i = [], 0
    while i + 1 < len(cfg):
        ln, typ = cfg[i], cfg[i + 1]
        if ln == 0:
            break
        if typ == 4:
            ifs.append(dict(num=cfg[i + 2], alt=cfg[i + 3], cls=cfg[i + 5], sub=cfg[i + 6], proto=cfg[i + 7], eps=[]))
        elif typ == 5 and ifs:
            ifs[-1]['eps'].append((cfg[i + 2], cfg[i + 3], struct.unpack_from('<H', cfg, i + 4)[0]))
        i += ln
    return ifs


def find_bulk(ifs, cls, sub=None):
    for f in ifs:
        if f['cls'] == cls and (sub is None or f['sub'] == sub):
            bo = [e[0] for e in f['eps'] if e[1] & 3 == 2 and not e[0] & 0x80]
            bi = [e[0] for e in f['eps'] if e[1] & 3 == 2 and e[0] & 0x80]
            if bo and bi:
                return f['num'], bo[0], bi[0]
    return None


def usbtmc_query(d, text, timeout_reads=4):
    ifs = parse_config(d.config_descriptor())
    found = find_bulk(ifs, 0xFE, 3)
    if not found:
        raise IOError('no USBTMC interface (interfaces: %s)' % ifs)
    _, ep_out, ep_in = found
    d.set_configuration(1)
    msg = text.encode() + b'\n'
    tag = 1
    hdr = struct.pack('<BBBBIB3x', 1, tag, 0xFF ^ tag, 0, len(msg), 1)  # DEV_DEP_MSG_OUT, EOM
    pad = (-len(msg)) % 4
    d.bulk_out(ep_out, hdr + msg + b'\0' * pad)
    if not text.strip().endswith('?'):
        return ''
    tag = 2
    req = struct.pack('<BBBBIBB2x', 2, tag, 0xFF ^ tag, 0, 4096, 0, 0)  # REQUEST_DEV_DEP_MSG_IN
    d.bulk_out(ep_out, req)
    resp = b''
    for _ in range(timeout_reads):
        resp += d.bulk_in(ep_in, 4096)
        if len(resp) >= 12:
            size = struct.unpack_from('<I', resp, 4)[0]
            if len(resp) >= 12 + size:
                return resp[12:12 + size].decode(errors='replace')
    return resp.decode(errors='replace')


class Usbtmc:
    """USBTMC message layer over a Dev (bulk OUT / IN, bTag sequence)."""

    def __init__(self, d):
        found = find_bulk(parse_config(d.config_descriptor()), 0xFE, 3)
        if not found:
            raise IOError('no USBTMC interface')
        _, self.ep_out, self.ep_in = found
        d.set_configuration(1)
        self.d, self.tag = d, 0
        self.in_pending = False  # a Bulk-IN URB of an unanswered query still queued

    def _next_tag(self):
        self.tag = self.tag % 255 + 1
        return self.tag

    def write(self, text):
        self.write_raw(text.encode() + b'\n')

    def write_raw(self, msg):
        t = self._next_tag()
        hdr = struct.pack('<BBBBIB3x', 1, t, 0xFF ^ t, 0, len(msg), 1)  # DEV_DEP_MSG_OUT, EOM
        self.d.bulk_out(self.ep_out, hdr + msg + b'\0' * ((-len(msg)) % 4))

    def read(self, maxlen=4096, timeout=5.0):
        """One reply; OSError (socket.timeout) after `timeout` s without one."""
        t = self._next_tag()
        self.d.bulk_out(self.ep_out, struct.pack('<BBBBIBB2x', 2, t, 0xFF ^ t, 0, maxlen, 0, 0))
        resp = b''
        while True:
            # A URB left over from a query the device never answered takes
            # this reply (USB/IP has no cheap cancel in Renode).
            if not self.in_pending:
                self.d.send_submit(DIR_IN, self.ep_in & 0x7F, in_len=maxlen + 12)
                self.in_pending = True
            self.d.s.settimeout(timeout)
            try:
                st, data = self.d.recv_ret(DIR_IN)
            finally:
                self.d.s.settimeout(20)
            self.in_pending = False
            if st != 0:
                raise IOError('bulk_in status %d' % st)
            resp += data
            if len(resp) >= 12 and len(resp) >= 12 + struct.unpack_from('<I', resp, 4)[0]:
                return resp[12:12 + struct.unpack_from('<I', resp, 4)[0]]


def upload(d, path, chunk=232):
    """fw-rs DBG:UPLOAD (as tools/dm40dbg.py upload): FILE -> DM40FW.BIN in
    the SPI disk's root."""
    import time
    import zlib
    data = open(path, 'rb').read()
    crc = zlib.crc32(data)
    u = Usbtmc(d)

    def err():
        u.write('SYST:ERR?')
        return u.read().decode(errors='replace').strip()
    u.write('DBG:UPLOAD:BEGIN %d,%d' % (len(data), crc))
    e = err()
    if not e.startswith('0,'):
        sys.exit('BEGIN: %s' % e)
    t0 = time.time()
    for i, off in enumerate(range(0, len(data), chunk)):
        u.write_raw(b'DBG:UPLOAD:DATA %d,' % off + data[off:off + chunk])
        if i % 256 == 255:
            e = err()
            if not e.startswith('0,'):
                sys.exit('DATA at %d: %s' % (off, e))
            print('  %d / %d bytes, %.1f s' % (off + chunk, len(data), time.time() - t0), flush=True)
    u.write('DBG:UPLOAD:END?')
    try:
        r = u.read(timeout=30).decode(errors='replace').strip()
    except OSError:
        r = 'no reply; SYST:ERR? -> %s' % err()
    print('END? -> %s (want %d,%d), %.1f s' % (r, len(data), crc, time.time() - t0))
    return r == '%d,%d' % (len(data), crc)


def scpi_tcp(busid, port):
    """Serve SCPI lines on TCP `port` (one client at a time) and forward each
    line as one USBTMC message; a line with a '?' gets the device's reply
    plus '\n'. Empty lines are dropped. Every exchange is logged with its wall
    time; a reply later than 1 s (libsigrok's hard USBTMC Bulk-IN timeout) is
    flagged with '!!'. Virtual time runs at or below wall speed (0.7x with
    --fast on a loaded host, 0.9x with --mips 25), so the flag is
    conservative; fw-rs DBG:PING? (uptime ms) around a query gives the
    virtual latency."""
    import time
    while True:  # the USB/IP server appears only at run.py --usbip T
        try:
            if any(x['busid'] == busid for x in devlist()):
                break
        except OSError:
            pass
        time.sleep(0.5)
    tmc = Usbtmc(Dev(busid))
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(('127.0.0.1', port))
    srv.listen(1)
    print('scpi-tcp: %s on tcp-raw/127.0.0.1/%d' % (busid, port), flush=True)
    t_start = time.monotonic()
    while True:
        c, _ = srv.accept()
        buf = b''
        while True:
            data = c.recv(4096)
            if not data:
                break
            buf += data
            while b'\n' in buf:
                line, buf = buf.split(b'\n', 1)
                cmd = line.decode(errors='replace').strip()
                if not cmd:
                    continue
                t0 = time.monotonic()
                print('%8.3f << %s' % (t0 - t_start, cmd), flush=True)
                tmc.write(cmd)
                if '?' in cmd:
                    try:
                        rep = tmc.read().decode(errors='replace').strip()
                    except socket.timeout:  # no reply (unknown query): nothing to the client
                        print('         !! no reply within 5 s', flush=True)
                        continue
                    dt = time.monotonic() - t0
                    print('         >> %s (%.0f ms)%s' % (rep, dt * 1e3, '  !! > 1 s' if dt > 1.0 else ''), flush=True)
                    c.sendall(rep.encode() + b'\n')
        c.close()


class Msc:
    """SCSI over Bulk-Only Transport on the device's MSC interface."""

    def __init__(self, d):
        ifs = parse_config(d.config_descriptor())
        found = find_bulk(ifs, 8)
        if not found:
            raise IOError('no MSC interface (interfaces: %s)' % ifs)
        self.iface, self.ep_out, self.ep_in = found
        self.d = d
        d.set_configuration(1)
        self.tag = 0

    def max_lun(self):
        """Class request GET MAX LUN (0xFE)."""
        return self.d.control_in(0xA1, 0xFE, 0, self.iface, 1)[0]

    def cmd(self, lun, cb, data_len=0, direction=DIR_IN, out=b''):
        """-> (CSW status, residue, data in)."""
        self.tag += 1
        cbw = struct.pack('<4sIIBBB16s', b'USBC', self.tag, data_len, 0x80 if direction == DIR_IN else 0, lun,
                          len(cb), cb.ljust(16, b'\0'))
        self.d.bulk_out(self.ep_out, cbw)
        data = b''
        if direction == DIR_IN and data_len:
            while len(data) < data_len:
                chunk = self.d.bulk_in(self.ep_in, min(4096, data_len - len(data)))
                if not chunk:
                    break
                data += chunk
                if len(chunk) % 64:
                    break  # short packet: the device sent less
        elif direction == DIR_OUT and data_len:
            # One URB per 64-byte packet: Renode's DWC2 OUT endpoint keeps at
            # most one packet while the device NAKs, so a longer URB loses data.
            out = out.ljust(data_len, b'\0')
            for o in range(0, data_len, 64):
                self.d.bulk_out(self.ep_out, out[o:o + 64])
        csw = self.d.bulk_in(self.ep_in, 13)
        sig, rtag, residue, status = struct.unpack('<4sIIB', csw)
        if sig != b'USBS' or rtag != self.tag:
            raise IOError('bad CSW %s for %s' % (csw.hex(), cb.hex()))
        return status, residue, data

    def ok(self, lun, cb, data_len=0, direction=DIR_IN, out=b''):
        st, _, data = self.cmd(lun, cb, data_len, direction, out)
        if st != 0:
            raise IOError('CSW status %d for %s on LUN %d (sense %s)' % (st, cb.hex(), lun, self.sense(lun)))
        return data

    def sense(self, lun):
        d = self.ok(lun, bytes([0x03, 0, 0, 0, 18, 0]), 18)
        return '%02X/%02X/%02X' % (d[2] & 15, d[12], d[13])

    def inquiry(self, lun):
        return self.ok(lun, bytes([0x12, 0, 0, 0, 36, 0]), 36)

    def capacity(self, lun):
        return struct.unpack('>II', self.ok(lun, bytes([0x25]) + b'\0' * 9, 8))

    def read(self, lun, lba, n, bs, retries=0):
        """READ(10). retries > 0: on a stuck transfer (the emulated DWC2
        sometimes stops answering a bulk URB, also with one LUN) reconnect,
        Bulk-Only Mass Storage Reset + clear both halts, and try again."""
        for k in range(retries + 1):
            try:
                return self.ok(lun, struct.pack('>BBIBHB', 0x28, 0, lba, 0, n, 0), n * bs)
            except (TimeoutError, socket.timeout):
                if k == retries:
                    raise
                print('  READ LBA %d x %d stuck: BOT reset recovery, retry' % (lba, n), flush=True)
                self.recover()

    def recover(self):
        busid = self.d.busid
        self.d.close()
        self.d = Dev(busid)
        self.d.control_out(0x21, 0xFF, 0, self.iface)          # Bulk-Only Mass Storage Reset
        for ep in (self.ep_in, self.ep_out):
            self.d.control_out(0x02, 0x01, 0, ep)              # CLEAR_FEATURE(ENDPOINT_HALT)


def msc_read(d, lba=0, lun=0):
    m = Msc(d)
    inq = m.inquiry(lun)
    print('INQUIRY: %r' % inq[8:36].decode(errors='replace'))
    last, bsz = m.capacity(lun)
    print('READ CAPACITY: %d blocks of %d bytes (%.1f MiB)' % (last + 1, bsz, (last + 1) * bsz / 1048576))
    blk = m.read(lun, lba, 1, bsz)
    print('READ(10) LBA %d: %d bytes, head %s, sig %s' % (lba, len(blk), blk[:16].hex(), blk[510:512].hex()))
    return blk


def msc_luns(d):
    """GET MAX LUN and the per-LUN behaviour a host OS relies on."""
    m = Msc(d)
    n = m.max_lun() + 1
    print('GET MAX LUN: %d -> %d LUN(s)' % (n - 1, n))
    res = {}
    for lun in range(n):
        inq = m.inquiry(lun)
        tur = m.cmd(lun, bytes(6))[0]
        last, bs = m.capacity(lun)
        ms6 = m.ok(lun, bytes([0x1A, 0, 0x3F, 0, 192, 0]), 192)
        ms10 = m.ok(lun, bytes([0x5A, 0, 0x3F, 0, 0, 0, 0, 0, 192, 0]), 192)
        wp6, wp10 = bool(ms6[2] & 0x80), bool(ms10[3] & 0x80)
        print('LUN %d: INQUIRY type %02x rmb %d %r; TEST UNIT READY %s; %d x %d B (%.1f MiB); '
              'MODE SENSE(6) WP %d, (10) WP %d' % (lun, inq[0], inq[1] >> 7, inq[8:32].decode(errors='replace'),
                                                    'ok' if tur == 0 else 'FAIL %s' % m.sense(lun),
                                                    last + 1, bs, (last + 1) * bs / 1048576, wp6, wp10))
        res[lun] = dict(inq=inq, wp=wp6, bs=bs, blocks=last + 1)
    for lun in range(n):
        if not res[lun]['wp']:
            continue
        bs = res[lun]['bs']
        before = m.read(lun, 0, 1, bs)
        st, residue, _ = m.cmd(lun, struct.pack('>BBIBHB', 0x2A, 0, 0, 0, 1, 0), bs, DIR_OUT, b'\xA5' * bs)
        sense = m.sense(lun)
        after = m.read(lun, 0, 1, bs)
        print('LUN %d: WRITE(10) LBA 0 -> CSW status %d residue %d, sense %s (want 07/27/00); LBA 0 %s'
              % (lun, st, residue, sense, 'unchanged' if after == before else 'CHANGED'))
        assert st == 1 and sense == '07/27/00' and after == before
        # PREVENT / ALLOW on the read-only LUN: accepted (host_mounted is LUN 0 only).
        print('LUN %d: PREVENT %d, ALLOW %d' % (lun, m.cmd(lun, bytes([0x1E, 0, 0, 0, 1, 0]))[0],
                                                m.cmd(lun, bytes([0x1E, 0, 0, 0, 0, 0]))[0]))
    print('LUN 0 TEST UNIT READY after: %s' % ('ok' if m.cmd(0, bytes(6))[0] == 0 else 'FAIL ' + m.sense(0)))
    return res


def msc_dump(d, lun, out):
    """FAT12/16 volume -> image: metadata + allocated clusters (zeros elsewhere).
    Resumable: progress is kept in OUT.part / OUT.done; a stuck transfer (the
    emulated DWC2 sometimes stops answering, also with one LUN and the
    pre-LUN-1 firmware) saves what was read and exits 2: restart the
    emulator and run the same command again."""
    import os
    m = Msc(d)
    last, bs = m.capacity(lun)
    part, prog = out + '.part', out + '.done'
    img = bytearray(open(part, 'rb').read()) if os.path.exists(part) else bytearray((last + 1) * bs)
    done = int(open(prog).read()) if os.path.exists(prog) else -1  # -1: metadata not read yet
    try:
        if done < 0:
            boot = m.read(lun, 0, 1, bs)
            b16 = lambda o: struct.unpack_from('<H', boot, o)[0]
            data = b16(0x0E) + boot[0x10] * b16(0x16) + b16(0x11) * 32 // bs
            for l in range(0, data, 64):
                n = min(64, data - l)
                img[l * bs:(l + n) * bs] = m.read(lun, l, n, bs)
            done = 0
        boot = bytes(img[:bs])
        b16 = lambda o: struct.unpack_from('<H', boot, o)[0]
        spc, rsvd, nf, rootn, fsz = boot[0x0D], b16(0x0E), boot[0x10], b16(0x11), b16(0x16)
        tot = b16(0x13) or struct.unpack_from('<I', boot, 0x20)[0]
        data = rsvd + nf * fsz + rootn * 32 // bs
        fat = img[rsvd * bs:(rsvd + fsz) * bs]
        clusters = (tot - data) // spc
        used = [c for c in range(2, clusters + 2) if struct.unpack_from('<H', fat, 2 * c)[0] != 0]
        print('LUN %d: %d sectors of %d, metadata %d sectors, %d clusters used of %d, %d done'
              % (lun, tot, bs, data, len(used), clusters, done), flush=True)
        for i in range(done, len(used)):
            lba = data + (used[i] - 2) * spc
            img[lba * bs:(lba + spc) * bs] = m.read(lun, lba, spc, bs)
            done = i + 1
            if done % 32 == 0:
                print('  %d / %d clusters' % (done, len(used)), flush=True)
    except (TimeoutError, socket.timeout):
        open(part, 'wb').write(img)
        open(prog, 'w').write(str(done))
        print('stuck after %d clusters: saved, restart the emulator and run again' % max(done, 0))
        sys.exit(2)
    open(out, 'wb').write(img)
    for f in (part, prog):
        if os.path.exists(f):
            os.remove(f)
    print('wrote %s' % out)


def main():
    try:
        main1()
    except NoAnswer as e:
        sys.exit(str(e))
    except ConnectionRefusedError:
        sys.exit('no USB/IP server on %s:%d (live.py --no-usbip? another port: USBIP_PORT=...)' % (HOST, PORT))


def main1():
    a = sys.argv[1:]
    if not a or a[0] == 'list':
        for d in devlist():
            print('busid=%s %04x:%04x class=%d interfaces=%s' % (d['busid'], d['idVendor'], d['idProduct'], d['cls'],
                  ' '.join('%02x/%02x/%02x' % i for i in d['ifs'])))
        return
    cmd, busid = a[0], a[1]
    if cmd == 'scpi-tcp':
        return scpi_tcp(busid, int(a[2]) if len(a) > 2 else 5025)
    d = Dev(busid)
    try:
        if cmd == 'desc':
            dd = d.device_descriptor()
            print('device descriptor:', dd.hex(' '))
            print('  %04x:%04x bcdUSB %x.%02x ep0 %d' % (d.idVendor, d.idProduct, dd[3], dd[2], dd[7]))
        elif cmd == 'config':
            for f in parse_config(d.config_descriptor()):
                print('interface %d class %02x/%02x/%02x eps %s' % (f['num'], f['cls'], f['sub'], f['proto'],
                      ' '.join('%02x(%s,%d)' % (e[0], {2: 'bulk', 3: 'int'}.get(e[1] & 3, e[1]), e[2]) for e in f['eps'])))
        elif cmd == 'usbtmc':
            print(usbtmc_query(d, a[2]).strip())
        elif cmd == 'msc':
            msc_read(d, int(a[2], 0) if len(a) > 2 else 0, int(a[3]) if len(a) > 3 else 0)
        elif cmd == 'luns':
            msc_luns(d)
        elif cmd == 'upload':
            sys.exit(0 if upload(d, a[2]) else 1)
        elif cmd == 'mscdump':
            msc_dump(d, int(a[2]), a[3])
        elif cmd == 'read':
            addr, n = int(a[2], 0), int(a[3], 0)
            data = d.control_in(0xC0, 0x02, addr & 0xFFFF, addr >> 16, n)
            print('read 0x%08x [%d]: %s' % (addr, n, data.hex(' ')))
        elif cmd == 'write':
            addr, data = int(a[2], 0), bytes.fromhex(a[3])
            d.control_out(0x40, 0x03, addr & 0xFFFF, addr >> 16, data)
            back = d.control_in(0xC0, 0x02, addr & 0xFFFF, addr >> 16, len(data))
            print('wrote %s @0x%08x; readback %s %s' % (data.hex(), addr, back.hex(), 'OK' if back == data else 'MISMATCH'))
        else:
            sys.exit(__doc__)
    finally:
        d.close()


if __name__ == '__main__':
    main()
