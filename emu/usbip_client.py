#!/usr/bin/env python3
"""Userspace USB/IP client for the emulated DM40 (no kernel vhci, no root).

Talks the USB/IP protocol to Renode's server (emu/run.py ... --usbip T, TCP
3240) and drives the device over EP0 and bulk endpoints:

    emu/usbip_client.py list
    emu/usbip_client.py desc   1-0              # device descriptor
    emu/usbip_client.py config 1-0              # interfaces and endpoints
    emu/usbip_client.py usbtmc 1-0 "*IDN?"      # fw-rs: SCPI over USBTMC
    emu/usbip_client.py msc    1-0 [LBA]        # INQUIRY, READ CAPACITY, READ(10)
    emu/usbip_client.py read   1-0 0x1FFF7A10 12   # usbrw vendor EP0 read
    emu/usbip_client.py write  1-0 0x20028FC0 02   # usbrw vendor EP0 write
    emu/usbip_client.py scpi-tcp 1-0 5025       # line-based SCPI over TCP <-> USBTMC
                                                # (libsigrok conn=tcp-raw/127.0.0.1/5025)

Renode exports a registered device as busid 1-0. Protocol framing after the
ep0-memrw session's emu-usbtest/usbip/usbip_client.py.
"""
import socket
import struct
import sys

HOST, PORT = '127.0.0.1', 3240
VERSION = 0x0111
OP_REQ_DEVLIST = 0x8005
OP_REQ_IMPORT = 0x8003
CMD_SUBMIT = 1
RET_SUBMIT = 3
DIR_OUT, DIR_IN = 0, 1


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


def devlist():
    s = socket.create_connection((HOST, PORT), timeout=20)
    s.sendall(op_header(OP_REQ_DEVLIST))
    _, _, status = struct.unpack('>HHI', recvn(s, 8))
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
        self.s = socket.create_connection((HOST, PORT), timeout=20)
        self.s.sendall(op_header(OP_REQ_IMPORT) + busid.encode('latin1').ljust(32, b'\0'))
        _, _, status = struct.unpack('>HHI', recvn(self.s, 8))
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
        msg = text.encode() + b'\n'
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


def msc_read(d, lba=0):
    ifs = parse_config(d.config_descriptor())
    found = find_bulk(ifs, 8)
    if not found:
        raise IOError('no MSC interface (interfaces: %s)' % ifs)
    _, ep_out, ep_in = found
    d.set_configuration(1)
    tag = [0]

    def scsi(cb, data_len, direction):
        tag[0] += 1
        cbw = struct.pack('<4sIIBBB16s', b'USBC', tag[0], data_len, 0x80 if direction == DIR_IN else 0, 0, len(cb), cb.ljust(16, b'\0'))
        d.bulk_out(ep_out, cbw)
        data = b''
        if direction == DIR_IN and data_len:
            while len(data) < data_len:
                chunk = d.bulk_in(ep_in, min(4096, data_len - len(data)))
                if not chunk:
                    break
                data += chunk
        csw = d.bulk_in(ep_in, 13)
        sig, rtag, residue, status = struct.unpack('<4sIIB', csw)
        if sig != b'USBS' or status != 0:
            raise IOError('CSW status %d for %s' % (status, cb.hex()))
        return data

    inq = scsi(bytes([0x12, 0, 0, 0, 36, 0]), 36, DIR_IN)
    print('INQUIRY: %r' % inq[8:36].decode(errors='replace'))
    cap = scsi(bytes([0x25]) + b'\0' * 9, 8, DIR_IN)
    last, bsz = struct.unpack('>II', cap)
    print('READ CAPACITY: %d blocks of %d bytes (%.1f MiB)' % (last + 1, bsz, (last + 1) * bsz / 1048576))
    blk = scsi(struct.pack('>BBIBHB', 0x28, 0, lba, 0, 1, 0), bsz, DIR_IN)
    print('READ(10) LBA %d: %d bytes, head %s, sig %s' % (lba, len(blk), blk[:16].hex(), blk[510:512].hex()))
    return blk


def main():
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
            msc_read(d, int(a[2], 0) if len(a) > 2 else 0)
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
