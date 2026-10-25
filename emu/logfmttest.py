#!/usr/bin/env python3
"""opendm40 LOG:FORMat in the emulator: a host drains LOG:DATA? of an AD2
session in each format (ROWS 12 B per sample, PACKed ~3 B, DLG ~1-3 B;
src/logger.rs "Compact LOG:DATA?") for --dur seconds of device time and
the replies are decoded afterwards by tools/dm40scpi.py `log_reply`.

  cd firmware && ./build.sh
  emu/logfmttest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

Per rate (--rates, AD2 in AC V, the model's 0.5 V 50 Hz sine) and format
(--formats): LOG:FORMat, LOG:STATe ON, LOG:DATA? as fast as the replies
come (the host stores them; the bit decoding of DLG is too slow in Python
to run inline at 76.8 kS/s), LOG:STATe OFF after --dur s of DBG:PING?
uptime, the rest drained. Lossless = the seqs 1..N consecutive, every
lost_before 0 (ROWS: no stamp gap over 1.5 periods), N = LOG:COUNt?
captured, overwritten / missed 0. Printed: rows/s of device time and of
host time, bytes per sample, replies per second. A compact format must be
lossless (exit status 1 otherwise); ROWS is reported only (above ~38 k
rows/s its 12 B records are past what the drain carries). At 76 800 the
compact formats are lossless with the logger panel on (the default
--draw): a panel part is drawn only while the ring has room for it
(dialog.rs `log_panel_room`; before it, the panel's first draw overwrote a
block after the first 2048 samples, device 2f56ff9).

The device's speed: --mips 110 --isr-stall-ns 700 (the defaults here; as
emu/fastlogtest.py: peripherals/CpuCost.cs). The emulator's USB/IP link is
not the device's USB: the host-side rates are the emulator's, not a
projection for the meter.
"""
import argparse
import os
import socket
import struct
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--port', type=int, default=63610, help='USB/IP port (default 63610)')
ap.add_argument('--rates', default='38400,76800')
ap.add_argument('--formats', default='rows,packed,dlg')
ap.add_argument('--dur', type=float, default=30.0, help='drain time per session, s of device time (default 30)')
ap.add_argument('--mips', type=int, default=110)
ap.add_argument('--isr-stall-ns', type=int, default=700)
ap.add_argument('--bt-port', type=int, default=0, help='also over Bluetooth (the BT passthrough on this port): an AD2 session '
                'refused in PACKed, then an AD1 640 S/s session (OSR 0 unchopped, the stream ISR) drained over BT in each format '
                'for --bt-dur s of device time, lossless')
ap.add_argument('--bt-dur', type=float, default=10.0)
ap.add_argument('--draw', type=int, default=None, help='DBG:AD2:DRAW level first (0: no logger panel; default: as booted, 1)')
ap.add_argument('--keep', action='store_true', help='keep the replies in emu/out/logfmt-<rate>-<fmt>.bin')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(ROOT, 'firmware', 'tools'))
import usbip_client as uc  # noqa: E402
import dm40scpi  # noqa: E402

FAIL = []
TABLE = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


def run_emu():
    os.makedirs(os.path.join(HERE, 'out'), exist_ok=True)
    script = os.path.join(HERE, 'out', 'logfmt-%d.script' % A.port)
    with open(script, 'w') as f:
        if A.isr_stall_ns:
            f.write('0 monitor sysbus.cpucost IsrStallNs %d\n' % A.isr_stall_ns)
    cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', str(A.mips), '--app', A.app,
           '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '100000', '--name', 'logfmt-%d' % A.port,
           '--dmm', 'vsrc:0.2,ac=0.5,f=50', '--script', script]
    if A.bt_port:
        cmd += ['--bt-port', str(A.bt_port)]
    return subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)


def stop_emu(emu):
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    emu.wait()


def connect():
    for _ in range(150):
        try:
            return uc.Usbtmc(uc.Dev('1-0'))
        except Exception:
            time.sleep(2)
    sys.exit('logfmttest: no USB/IP device on port %d' % A.port)


def count(raw, fmt):
    """Samples in a reply from its headers only (no bit decoding)."""
    n = int(raw[2:6])
    d = raw[6:6 + n]
    if fmt == 'rows' or n == 0:
        return n // 12, n < 500 * 12
    left = struct.unpack_from('<I', d, 16)[0]
    if d[:2] == b'LD':
        return struct.unpack_from('<H', d, 28)[0], left == 0
    o, k = 20, 0
    while o < n:
        cnt, w = struct.unpack_from('<HB', d, o + 8)
        k += cnt
        o += 16 + cnt * w
    return k, left == 0


def session(q, rate, fmt):
    q('LOG:STATe OFF')
    q('*CLS')
    q('CONF:VOLT:AC')
    q('LOG:SOURce AD2')
    q('LOG:SRATe %d' % rate)
    q('LOG:CHOP ON')
    q('LOG:FORMat %s' % dm40scpi.LOG_FORMATS[fmt])
    f = q('LOG:FORMat?')
    check(f == dm40scpi.LOG_FORMATS[fmt], '%d %s: LOG:FORMat? %s' % (rate, fmt, f))
    time.sleep(2)
    up0 = int(q('DBG:PING?').split(',')[0])
    # The first LOG:DATA? right after LOG:STATe ON: the ring holds 26 ms
    # at 76.8 kS/s (the error queue is read at the end).
    q('LOG:STATe ON')
    w0 = time.monotonic()
    replies = []
    got = 0
    up = up0
    slow = []
    while up < up0 + A.dur * 1000:
        for _ in range(50):
            ta = time.monotonic()
            r = q('LOG:DATA?', raw=True)
            replies.append(r)
            got += count(r, fmt)[0]
            slow.append(time.monotonic() - ta)
        up = int(q('DBG:PING?').split(',')[0])
    w1 = time.monotonic()
    q('LOG:STATe OFF')
    err = q('SYST:ERR?')
    check(err.startswith('0,'), '%d %s: LOG:STATe ON accepted, no error (%s)' % (rate, fmt, err))
    slow.sort()
    print('  LOG:DATA? round trip (host): median %.1f ms, 99 %% %.1f ms, max %.1f ms'
          % (1e3 * slow[len(slow) // 2], 1e3 * slow[len(slow) * 99 // 100], 1e3 * slow[-1]), flush=True)
    up1 = int(q('DBG:PING?').split(',')[0])
    while True:
        r = q('LOG:DATA?', raw=True)
        replies.append(r)
        if count(r, fmt)[0] == 0:
            break
    cnt = [int(x) for x in q('LOG:COUNt?').split(',')]
    isr = q('DBG:ISR?')
    pre = q('LOG:PREamble?').split(',')
    nominal = int(pre[8]) / 1000.0
    if A.keep:
        with open(os.path.join(HERE, 'out', 'logfmt-%d-%s.bin' % (rate, fmt)), 'wb') as fo:
            for r in replies:
                fo.write(struct.pack('<I', len(r)) + r)
    # decode (streamed: 2.3 M rows at 76.8 kS/s)
    period_us = 1e6 / nominal
    n, lost, seq_ok, last_ts = 0, 0, True, None
    for r in replies:
        for seq, ts, _code, dl in dm40scpi.log_reply(r, fmt)[0]:
            n += 1
            seq_ok &= seq == n
            if dl is not None:
                lost += dl
            elif last_ts is not None:
                lost += max(round(((ts - last_ts) & 0xFFFFFFFF) / period_us) - 1, 0)
            last_ts = ts
    nbytes = sum(int(r[2:6]) + 6 for r in replies)
    dev_s = (up1 - up0) / 1000.0
    print('  LOG:COUNt? %s  DBG:ISR? %s  nominal %.1f S/s' % (','.join(map(str, cnt)), isr, nominal), flush=True)
    lossless = seq_ok and lost == 0 and n == cnt[1] and cnt[2] == 0 and cnt[3] == 0
    what = '%d %s: %d rows, seqs %s, lost_before sum %d, captured %d, overwritten %d, missed %d' % (
        rate, fmt, n, 'consecutive' if seq_ok else 'NOT consecutive', lost, cnt[1], cnt[2], cnt[3])
    if fmt == 'rows':
        print('  %s %s (reported only)' % ('ok  ' if lossless else 'info', what), flush=True)
    else:
        check(lossless, what)
    TABLE.append((rate, fmt, n, got / max(dev_s, 1e-9), got / max(w1 - w0, 1e-9), nbytes / max(n, 1),
                  len(replies) / max(dev_s, 1e-9), cnt[2], 'yes' if lossless else 'no'))


def bt_session(q):
    """Over Bluetooth: the same LOG:FORMat replies (Bt::respond_with)."""
    try:
        s = socket.create_connection(('127.0.0.1', A.bt_port), timeout=5)
    except OSError as e:
        check(False, 'BT passthrough (%s)' % e)
        return
    s.settimeout(30)
    buf = b''

    def line(cmd, reply=True):
        nonlocal buf
        s.sendall(cmd.encode() + b'\n')
        if not reply:
            return None
        while b'\n' not in buf:
            buf += s.recv(65536)
        ln, _, buf = buf.partition(b'\n')
        return ln.decode(errors='replace').strip()

    def block(cmd):
        nonlocal buf
        s.sendall(cmd.encode() + b'\n')
        while len(buf) < 6:
            buf += s.recv(65536)
        n = int(buf[2:6])
        while len(buf) < 6 + n + 1:
            buf += s.recv(65536)
        d, buf = buf[:6 + n], buf[6 + n + 1:]
        return d

    try:
        line('*IDN?')
        line('LOG:STATe OFF', False)
        line('LOG:DEST HOST', False)
        line('LOG:FORMat PACKed', False)
        line('CONF:VOLT:AC', False)
        time.sleep(2)
        line('LOG:SOURce AD2', False)
        line('LOG:SRATe 1200', False)
        line('*CLS', False)
        line('LOG:STATe ON', False)
        e = line('SYST:ERR?')
        st = line('LOG:STATe?')
        check('Rate too high for Bluetooth' in e and st == '0', 'BT PACKed: AD2 1200 S/s refused (%s, STATe? %s)' % (e, st))
        line('CONF:VOLT:DC', False)
        time.sleep(2)
        line('LOG:SOURce AD1', False)
        line('LOG:OSR 0', False)
        line('LOG:CHOP OFF', False)
        for fmt in ('rows', 'packed', 'dlg'):
            line('*CLS', False)
            line('LOG:FORMat %s' % dm40scpi.LOG_FORMATS[fmt], False)
            f = line('LOG:FORMat?')
            up0 = int(q('DBG:PING?').split(',')[0])  # DBG: over USB (refused over BT)
            line('LOG:STATe ON', False)
            replies = []
            while int(q('DBG:PING?').split(',')[0]) < up0 + A.bt_dur * 1000:
                replies.append(block('LOG:DATA?'))
            line('LOG:STATe OFF', False)
            while True:
                r = block('LOG:DATA?')
                replies.append(r)
                if r == b'#40000':
                    break
            e = line('SYST:ERR?')
            cnt = [int(x) for x in q('LOG:COUNt?').split(',')]
            n, lost, seq_ok, last_ts = 0, 0, True, None
            for r in replies:
                for seq, ts, _c, dl in dm40scpi.log_reply(r, fmt)[0]:
                    n += 1
                    seq_ok &= seq == n
                    if dl is not None:
                        lost += dl
                    elif last_ts is not None:
                        lost += max(round(((ts - last_ts) & 0xFFFFFFFF) / 1562.5) - 1, 0)
                    last_ts = ts
            nb = sum(len(r) for r in replies)
            check(f == dm40scpi.LOG_FORMATS[fmt] and e.startswith('0,') and seq_ok and lost == 0 and n == cnt[1] and cnt[2] == 0,
                  'BT %s: AD1 640 S/s over Bluetooth, %d rows in %d replies (%.2f B/sample), lost %d, captured %d, '
                  'overwritten %d (%s)' % (fmt, n, len(replies), nb / max(n, 1), lost, cnt[1], cnt[2], e))
    except Exception as x:
        check(False, 'BT: %r' % x)
    finally:
        s.close()


emu = run_emu()
try:
    t = connect()

    def q(line, raw=False, timeout=120):
        t.write(line)
        if '?' not in line:
            return None
        b = t.read(maxlen=8192, timeout=timeout)
        return b if raw else b.decode(errors='replace').strip()

    print('*IDN?', q('*IDN?'), flush=True)
    if A.draw is not None:
        q('DBG:AD2:DRAW %d' % A.draw)
    for rate in [int(x) for x in A.rates.split(',')]:
        for fmt in A.formats.split(','):
            session(q, rate, fmt)
    if A.bt_port:
        bt_session(q)
finally:
    stop_emu(emu)

print('\n  rate format   rows    rows/s(dev) rows/s(host) B/sample replies/s(dev) overwritten lossless')
for r in TABLE:
    print('%6d %-7s %8d %11.0f %12.0f %8.2f %14.1f %11d %s' % r)
print('FAIL: %d checks' % len(FAIL) if FAIL else 'PASS')
sys.exit(1 if FAIL else 0)
