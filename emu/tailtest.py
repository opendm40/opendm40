#!/usr/bin/env python3
"""The scope record's tail (docs/ram-pools-audit.md candidate B; pool.rs
TAIL, scope.rs "record tail"): the record runs on into the CH2 store, the
logger ring's slots and the CSV row FIFO while they are idle, and a lender
starting cuts it.

  emu/tailtest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

Scope page, a 1 kHz sine, 1 ms/div (a linear record of the whole ring),
DC V on the DMM (AD1 for LOG:STATe, a DC V row for CH2). The record
lengths are relative to the first one (CAP + the whole tail: CAP + 16 656):
  1. DIG: CAP + 16 656 points (CH2 off, nothing streams); its WORD record
     is the input sine;
  2. WAV:DATA? of it streamed over USB, LOG:STATe ON over Bluetooth after
     the first round: no reply to the Bluetooth message (it waits; checked
     for 0.7 s, under the stalled-stream timeout), the USB
     record comes whole and equal to (1); then the session runs (LOG:STATe?
     1 over Bluetooth); the stopped record is cut: CAP + 4 368 points, a
     window of (1);
  3. LOG:DATA? records consecutive (seq; the scope pauses during a host
     session); LOG:STATe OFF with records
     unread: DIG still CAP + 4 368 (they stay readable); drained: DIG
     CAP + 16 656 again;
  4. LOG:CSV ON with a long stopped record: CAP + 12 560 points, a window
     of the record before; DIG while recording: CAP + 12 560; LOG:CSV OFF:
     DIG CAP + 16 656;
  5. CH2 on (MF > Display) with a long stopped record: CAP points, a window
     of the record before; DIG with CH2: CAP; CH2 off: DIG CAP + 16 656.
Every cut record and every new one is checked against the sine.
Exit status 0 = all passed.
"""
import argparse
import os
import shutil
import socket
import struct
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF')
ap.add_argument('--port', type=int, default=None, help='USB/IP port (default: run.free_port); BT = port + 1')
ap.add_argument('--mips', default='200')
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
if A.port is None:
    A.port = run.free_port()
BT_PORT = A.port + 1
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

# The tail's segments (pool.rs SEGS, samples).
SEG = (4368, 8192, 4096)
FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


for p in (A.port, BT_PORT):
    if uc.port_busy(p):
        sys.exit('tailtest: port %d is in use (another emulator?)' % p)
OUT = os.path.join(HERE, 'out')
os.makedirs(OUT, exist_ok=True)
tag = 'tail-%d-%d' % (A.port, os.getpid())
disk = os.path.join(OUT, tag + '.disk.bin')
shutil.copy(os.path.join(ROOT, 'stock/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin'), disk)
cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--bt-port', str(BT_PORT), '--run-for', '4000',
       '--name', tag, '--log', os.path.join(OUT, tag + '.log'), '--disk', disk,
       '--dmm', 'vsrc:0.2,ac=0.5,f=50', '--scope', 'sine,1000,1.0']
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)


def block(b):
    assert b[:1] == b'#', b[:16]
    d = int(b[1:2])
    n = int(b[2:2 + d]) if d else 0
    data = b[2 + d:]
    assert len(data) == n + 1 and data[-1:] == b'\n', (len(data), n, data[-4:])
    return data[:n]


def sine_ok(words, xinc, what):
    """The WORD record is the 1 kHz, 1 Vpp input sine (rising mid crossings)."""
    yinc = PRE['yinc']
    v = [c * yinc for c in struct.unpack('<%dH' % (len(words) // 2), words)]
    lo, hi = min(v), max(v)
    mid, hy = (lo + hi) / 2, (hi - lo) * 0.1
    ups, armed = [], False
    for i, x in enumerate(v):
        if x < mid - hy:
            armed = True
        elif armed and x > mid:
            ups.append(i)
            armed = False
    f = (len(ups) - 1) / ((ups[-1] - ups[0]) * xinc) if len(ups) >= 2 else 0.0
    # Every period the same length: a corrupted stretch (another tenant's
    # bytes in the record) breaks it.
    per = [b - a for a, b in zip(ups, ups[1:])]
    even = per and max(per) - min(per) <= 0.02 * (sum(per) / len(per)) + 2
    check(abs(f - 1000) < 20 and abs((hi - lo) - 1.0) < 0.15 and even,
          '%s: %.1f Hz, %.3f Vpp, periods %s..%s samples' % (what, f, hi - lo, min(per) if per else '-', max(per) if per else '-'))


PRE = {}

try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('tailtest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60, raw=False, maxlen=4096):
        t.write(line)
        if '?' not in line:
            return None
        b = t.read(maxlen=maxlen, timeout=timeout)
        return b if raw else b.decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000.0

    def wait_dev(s):
        t0 = up()
        while up() - t0 < s:
            time.sleep(0.1)

    def wait_stop(s=15, wall=240):
        t0, v0 = time.time(), up()
        while time.time() - t0 < wall and up() - v0 < s:
            if q('TRIG:STAT?') == 'STOP':
                return True
            time.sleep(0.2)
        return False

    def points():
        f = q('WAV:PRE?').split(',')
        PRE.update(n=int(f[2]), xinc=float(f[4]), yinc=float(f[7]))
        return PRE['n']

    def record():
        return block(q('WAV:DATA?', raw=True, maxlen=65536, timeout=240))

    def dig():
        q('DIG')
        ok = wait_stop()
        return points() if ok else -1

    def window(cut, full, what):
        """`cut` is a contiguous window of `full` (whole samples)."""
        i = full.find(cut)
        while i >= 0 and i % 2:
            i = full.find(cut, i + 1)
        check(i >= 0, '%s: the cut record (%d points) is samples %d.. of the one before' % (what, len(cut) // 2, i // 2))

    print('*IDN?', q('*IDN?'), flush=True)
    q('CONF:VOLT:DC')
    wait_dev(1.5)
    # Main menu > Oscilloscope tile.
    for c in ('DBG:KEY 0', 'DBG:TAP 341,107'):
        q(c)
        wait_dev(1.5)
    v = q('DBG:VIEW?')
    check(v.startswith('Oscilloscope'), 'scope page (%s)' % v)
    for c in ('*CLS', 'DBG:HWTRIG:ENABLE 0', 'CHAN1:COUP DC', 'CHAN1:SCAL 0.2', 'CHAN1:OFFS 0', 'TIM:SCAL 1e-3',
              'TRIG:SWE AUTO', 'TRIG:SLOP POS', 'TRIG:LEV 0', 'WAV:POIN MAX', 'WAV:FORM WORD', 'LOG:SOUR AD1', 'RUN'):
        q(c)
    e = q('SYST:ERR?')
    check(e.startswith('0,'), 'set-up accepted (%s)' % e)
    wait_dev(1)

    # 1. The whole tail.
    n0 = dig()
    cap = n0 - sum(SEG)
    full = (n0, cap + SEG[0], cap + SEG[0] + SEG[1], cap)
    print('  CAP %d, record %d' % (cap, n0), flush=True)
    check(cap in (42 * 1024, 43 * 1024, 44 * 1024), '1. DIG, nothing streams: CAP + the whole tail (%d = %d + %d)' % (n0, cap, sum(SEG)))
    r0 = record()
    check(len(r0) == 2 * n0, '1. WAV:DATA? %d bytes' % len(r0))
    sine_ok(r0, PRE['xinc'], '1. the long record')

    # 2. LOG:STATe ON over Bluetooth during the streamed WAV:DATA?.
    bt = socket.create_connection(('127.0.0.1', BT_PORT), timeout=10)
    bbuf = b''

    def bt_line(cmd, reply=True, timeout=30):
        global bbuf
        bt.sendall(cmd.encode() + b'\n')
        if not reply:
            return None
        bt.settimeout(timeout)
        while b'\n' not in bbuf:
            d = bt.recv(65536)
            if not d:
                return None
            bbuf += d
        l, _, bbuf = bbuf.partition(b'\n')
        return l.decode(errors='replace')

    idn = None
    for _ in range(6):
        try:
            idn = bt_line('*IDN?', timeout=10)
            break
        except socket.timeout:
            bbuf = b''
    check(bool(idn) and idn.startswith('Alientek'), 'BT: *IDN? %r' % idn)
    t.write('WAV:DATA?')
    parts = []
    part, eom = t._read1(4096, 60)
    parts.append(part)
    bt.sendall(b'LOG:STAT ON;LOG:STAT?\n')
    # Well under the 2 s of device time after which a host that stopped
    # reading loses its stream (main.rs STREAM_STALL_MS).
    bt.settimeout(0.7)
    try:
        early = bt.recv(4096)
    except socket.timeout:
        early = b''
    check(early == b'', '2. LOG:STATe ON during the transfer: no reply yet (%r)' % early[:40])
    while not eom:
        part, eom = t._read1(4096, 60)
        parts.append(part)
    r = block(b''.join(parts))
    check(r == r0, '2. the transfer came whole and unchanged (%d bytes in %d rounds)' % (len(r), len(parts)))
    bt.settimeout(60)
    while b'\n' not in bbuf:
        bbuf += bt.recv(4096)
    l, _, bbuf = bbuf.partition(b'\n')
    check(l.strip() == b'1', '2. then the session runs: LOG:STATe? %r' % l)
    e = bt_line('SYST:ERR?')
    check(e.startswith('0,'), '2. no error (%s)' % e)
    n = points()
    check(n == full[1], '2. the stopped record cut to CAP + the CH2 store: %d (want %d)' % (n, full[1]))
    r1 = record()
    window(r1, r0, '2.')
    sine_ok(r1, PRE['xinc'], '2. the cut record')

    # 3. A host session running (the GUI and the scope pause meanwhile).
    seq, gaps, recs = None, 0, 0
    for _ in range(6):
        d = block(q('LOG:DATA?', raw=True, maxlen=65536))
        for k in range(0, len(d) - 11, 12):
            s = struct.unpack_from('<I', d, k)[0]
            if seq is not None and s != seq + 1:
                gaps += 1
            seq = s
            recs += 1
        wait_dev(0.5)
    check(recs > 20 and gaps == 0, '3. LOG:DATA? of the session: %d records, %d seq breaks' % (recs, gaps))
    q('LOG:STAT OFF')
    wait_dev(0.5)
    n = dig()
    check(n == full[1], '3. LOG:STATe OFF with records unread: DIG %d (want %d, they stay readable)' % (n, full[1]))
    for _ in range(4):
        d = block(q('LOG:DATA?', raw=True, maxlen=65536))
        if not d:
            break
    q('RUN')
    wait_dev(0.5)
    n = dig()
    check(n == full[0], '3. drained: DIG %d (want %d)' % (n, full[0]))
    rb = record()
    sine_ok(rb, PRE['xinc'], '3. the long record again')

    # 4. A CSV recording.
    q('LOG:CSV ON')
    e = q('SYST:ERR?')
    check(e.startswith('0,'), '4. LOG:CSV ON accepted (%s)' % e)
    n = points()
    check(n == full[2], '4. the stopped record cut to CAP + store + slots: %d (want %d)' % (n, full[2]))
    r2 = record()
    window(r2, rb, '4.')
    sine_ok(r2, PRE['xinc'], '4. the cut record')
    q('RUN')
    wait_dev(1)
    n = dig()
    check(n == full[2], '4. DIG while recording: %d (want %d)' % (n, full[2]))
    sine_ok(record(), PRE['xinc'], '4. its record')
    q('LOG:CSV OFF')
    for _ in range(40):
        if q('LOG:CSV?').split(',')[0] not in ('1', '2'):
            break
        wait_dev(0.5)
    st = q('LOG:CSV?')
    check(st.split(',')[0] == '3', '4. the recording ended (LOG:CSV? %s)' % st)
    q('RUN')
    wait_dev(0.5)
    n = dig()
    check(n == full[0], '4. after the recording: DIG %d (want %d)' % (n, full[0]))
    rc = record()

    # 5. CH2 (MF > Display > CH2 On, MF closed).
    def ch2_set(on):
        # The Display tab's "CH2 (DMM)" On / Off list (scope_ui.rs C2_LIST).
        for c in ('DBG:KEY 3', 'DBG:TAP 165,40', 'DBG:TAP %d,245' % (40 if on else 90), 'DBG:KEY 3'):
            q(c)
            wait_dev(1.5)

    ch2_set(True)
    wait_dev(1)
    v = q('DBG:VIEW?')
    n = points()
    check(n == full[3], '5. CH2 on with a long stopped record: cut to CAP: %d (want %d; view %s)' % (n, full[3], v))
    r3 = record()
    window(r3, rc, '5.')
    sine_ok(r3, PRE['xinc'], '5. the cut record')
    q('RUN')
    wait_dev(1)
    n = dig()
    check(n == full[3], '5. DIG with CH2: %d (want %d)' % (n, full[3]))
    sine_ok(record(), PRE['xinc'], '5. its record')
    q('RUN')
    ch2_set(False)
    wait_dev(1)
    n = dig()
    check(n == full[0], '5. CH2 off: DIG %d (want %d)' % (n, full[0]))
    e = q('SYST:ERR?')
    check(e.startswith('0,'), 'no error at the end (%s)' % e)
    # A --features dbg-pool image: the holders at the end (else -113).
    print('  DBG:POOL?', q('DBG:POOL?'), flush=True)
    q('*CLS')
except Exception as x:
    import traceback
    traceback.print_exc()
    check(False, 'exception %r' % x)
finally:
    os.killpg(emu.pid, 9)
    emu.wait()
    try:
        os.remove(disk)
    except OSError:
        pass

print('tailtest: %s' % ('PASS' if not FAIL else 'FAIL (%d)' % len(FAIL)))
sys.exit(1 if FAIL else 0)
