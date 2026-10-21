#!/usr/bin/env python3
"""Transparent TCP proxy logging USB/IP PDUs (SUBMIT / RET_SUBMIT / UNLINK /
RET_UNLINK) between `usbip attach` (TCP 3240) and the emulator on another port:

    emu/run.py fwrs --fast --usbip 0.01 --usbip-port 3241 --run-for 3600 &
    emu/usbip_proxy.py 3240 3241 > usbip.log &
    sudo usbip attach -r 127.0.0.1 -b 1-0

Logs per chunk, so a PDU split across reads is decoded only from its first
chunk."""
import socket, struct, sys, threading, time

LPORT, UPORT = int(sys.argv[1]), int(sys.argv[2])
T0 = time.time()


def log(*a):
    print('%8.3f' % (time.time() - T0), *a, flush=True)


def pump(src, dst, tag, n):
    pfx = '#%d' % n
    while True:
        try:
            d = src.recv(65536)
        except OSError:
            d = b''
        if not d:
            log(pfx, tag, 'EOF')
            try:
                dst.shutdown(socket.SHUT_WR)
            except OSError:
                pass
            return
        log(pfx, tag, len(d), d[:64].hex())
        if tag == 'K->R' and len(d) >= 48:
            off = 0
            while off + 48 <= len(d):
                cmd, seq, devid, dirn, ep = struct.unpack_from('>IIIII', d, off)
                if cmd == 1:
                    flags, tlen = struct.unpack_from('>II', d, off + 20)
                    setup = d[off + 40:off + 48].hex()
                    log('   SUBMIT seq=%d dir=%d ep=%d flags=%#x len=%d setup=%s' % (seq, dirn, ep, flags, tlen, setup))
                    off += 48 + (tlen if dirn == 0 else 0)
                elif cmd == 2:
                    log('   UNLINK seq=%d unlink_seq=%d' % (seq, struct.unpack_from('>I', d, off + 20)[0]))
                    off += 48
                else:
                    break
        elif tag == 'R->K' and len(d) >= 48:
            cmd, seq, devid, dirn, ep, st, alen = struct.unpack_from('>IIIIIiI', d, 0)
            if cmd in (3, 4):
                log('   RET%s seq=%d dir=%d ep=%d status=%d alen=%d' % ('_SUBMIT' if cmd == 3 else '_UNLINK', seq, dirn, ep, st, alen))
        dst.sendall(d)


ls = socket.socket()
ls.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
ls.bind(('127.0.0.1', LPORT))
ls.listen(4)
n = 0
while True:
    c, _ = ls.accept()
    n += 1
    try:
        u = socket.create_connection(("127.0.0.1", UPORT))
    except OSError as e:
        log("upstream", e); c.close(); continue
    log('conn')
    threading.Thread(target=pump, args=(c, u, 'K->R', n), daemon=True).start()
    threading.Thread(target=pump, args=(u, c, 'R->K', n), daemon=True).start()
