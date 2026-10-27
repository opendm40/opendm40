#!/usr/bin/env python3
"""opendm40 protocol decoder (src/decode.rs, src/app_decode.rs; DECode:...) in
the emulator: UART and Modbus RTU bit patterns on the scope input
(peripherals/ScopeFeeder.cs SetBitsFile, a watched file the test rewrites),
decoded while the scope streams, read back over USBTMC with DECode:FRAMes?.

  cd firmware && ./build.sh
  emu/decodetest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

Cases (each: write the pattern, set the decoder over SCPI, DECode:CLEar,
let it run, read the frames):
  * UART 115200 8N1, 3.3 V logic, auto-baud and auto threshold: DECode:RATE?
    115200, the text intact in every frame, no framing / parity error, no
    stream lap (Lost), DECode:LEVel? near 1.65 V;
  * the same with the main loop busy: the decoder view drawn (rows) while a
    host reads frames, DBG:DIAG? loop maximum, no lap;
  * RS-232 levels (+-6 V, inverted) at 9600 7E1, the rate set: text intact;
  * baud mismatch (signal 9600, decoder 19200): framing errors, no text;
  * Modbus RTU 19200 8E1 (auto-baud): 03 request / reply, 06 write / echo,
    an exception and a frame with a bad CRC: kinds, pairing (req=), CRC
    counter, the reply time;
  * DMM alongside: READ? answers while the decoder runs;
  * region C (emu/regionc.py, gated on scope::DEC_LIVE): no .cold function
    runs while the decoder stream does.
DEC:LEV? reads ~1.44 V for 3.3 V logic here, not 1.65 V: the feeder's
counts per volt and the dump's scope calibration differ (the scope page's
volts differ the same way), hence the wide window.
--shots DIR: emulator screenshots of the decoder view (DBG:LCD?).
--mips: CPU speed (default 110 = the device's speed per instruction).
Exit status 0 = all passed.
"""
import argparse
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
import run as emurun  # noqa: E402

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--port', type=int, default=0, help='USB/IP port (default: a free one)')
ap.add_argument('--mips', default='110')
ap.add_argument('--shots', help='save screenshots of the decoder view here')
ap.add_argument('--no-region-c', action='store_true', help='without the region C hooks')
A = ap.parse_args()
if not A.port:
    A.port = emurun.free_port()
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402
import regionc  # noqa: E402

FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


OUT = os.path.join(HERE, 'out')
os.makedirs(OUT, exist_ok=True)
TAG = 'decode-%d-%d' % (A.port, os.getpid())
BITS = os.path.join(OUT, TAG + '.bits')


# ---- bit patterns ----

def uart_bits(data, fmt='8N1'):
    nb = 7 if fmt[0] == '7' else 8
    par = fmt[1]
    s = ''
    for b in data:
        s += '0'
        ones = 0
        for i in range(nb):
            bit = b >> i & 1
            ones += bit
            s += str(bit)
        if par == 'E':
            s += str(ones & 1)
        elif par == 'O':
            s += str(1 - (ones & 1))
        s += '1'
    return s


def crc16(d):
    c = 0xFFFF
    for b in d:
        c ^= b
        for _ in range(8):
            c = (c >> 1) ^ 0xA001 if c & 1 else c >> 1
    return c


def mb(d):
    c = crc16(d)
    return bytes(d) + bytes([c & 0xFF, c >> 8])


def put_bits(bits, baud, vlo, vhi, repeat):
    """Write the pattern (the feeder reloads it within 50 ms of virtual time)."""
    tmp = BITS + '.tmp'
    with open(tmp, 'w') as f:
        f.write('%.12g %g %g %g\n' % (1.0 / baud, vlo, vhi, repeat))
        f.write(bits)
    os.replace(tmp, BITS)


put_bits('1' * 10, 9600, 0.0, 3.3, 0)
RC = None if A.no_region_c else regionc.Hooks(A.app, OUT, TAG, gate='opendm40::scope::DEC_LIVE')
SCRIPT = os.path.join(OUT, TAG + '.script')
with open(SCRIPT, 'w') as f:
    f.write('0.2 monitor sysbus.scopefeed SetBitsFile "%s"\n' % BITS)
    if RC and RC.script:
        f.write(RC.lines())
if RC and not RC.script:
    print('(no llvm-nm / symbols: the region C checks are skipped)')
    RC = None

cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '3000', '--name', TAG,
       '--log', os.path.join(OUT, TAG + '.log'), '--script', SCRIPT]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)


def block(b):
    assert b[:1] == b'#', b[:16]
    d = int(b[1:2])
    n = int(b[2:2 + d]) if d else 0
    return b[2 + d:2 + d + n]


try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('decodetest: no USB/IP device on port %d' % A.port)

    def q(line, raw=False, timeout=60):
        t.write(line)
        if '?' not in line:
            return None
        b = t.read(maxlen=8192, timeout=timeout)
        return b if raw else b.decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0])

    def wait_ms(ms):
        t0 = up()
        while up() - t0 < ms:
            time.sleep(0.2)

    def err():
        return q('SYST:ERR?')

    def frames():
        """Every unread frame line (DECode:FRAMes? until empty)."""
        out = []
        for _ in range(12):
            d = block(q('DEC:FRAM?', raw=True)).decode()
            if not d:
                break
            out += [l.split(',') for l in d.splitlines()]
        return out

    def count():
        return [int(x) for x in q('DEC:COUN?').split(',')]

    def shot(name):
        if not A.shots:
            return
        from PIL import Image
        os.makedirs(A.shots, exist_ok=True)
        rows = 4
        px = bytearray()
        for y in range(0, 320, rows):
            n = 480 * rows * 3 // 2 + 1
            b = q('DBG:LCD? 0,%d,480,%d,%d' % (y, rows, n), raw=True)
            nd = int(b[1:2])
            d = b[2 + nd:2 + nd + int(b[2:2 + nd])]
            w = [d[i] | d[i + 1] << 8 for i in range(2, len(d), 2)]
            bs = bytearray()
            for v in w:
                bs += bytes([v >> 8, v & 0xFF])
            px += bs[:480 * rows * 3]
        Image.frombytes('RGB', (480, 320), bytes(px)).save(os.path.join(A.shots, name + '.png'))
        wait_ms(400)

    def loop_max():
        b = block(q('DBG:DIAG?', raw=True))
        return b[97] | b[98] << 8

    # The test's own queries: DBG:DIAG? (touch_diag) and the DMM READ?
    # (stale_read, once per READ?) are region C by design.
    ALLOWED = ('touch_diag', 'stale_read')

    def c_check(what, every=False):
        if not RC:
            return
        got = RC.calls(every)
        bad = sorted(k for k in got if not any(a in k for a in ALLOWED))
        print('  region C functions called while decoding: %s' % (', '.join(sorted(got)) or 'none'))
        check(not bad, '%s: no region C code while the decoder stream runs%s' % (what, '' if not bad else ' (%s)' % ', '.join(bad)))

    print('decodetest: %s' % q('*IDN?'), flush=True)
    wait_ms(1500)
    q('DBG:KEY 0')  # main menu -> the decoder opens over whatever view
    wait_ms(500)

    # ---- UART 115200 8N1, auto ----
    text = b'The quick brown fox 0123456789\r\n'
    put_bits('1' * 20 + uart_bits(text), 115200, 0.0, 3.3, 0.05)
    q('DEC:PROT UART;DEC:BAUD AUTO;DEC:FORM 8N1;DEC:POL NORM;DEC:THR AUTO')
    q('DEC:STAT ON')
    check(err().startswith('0,'), 'DEC:STAT ON accepted')
    wait_ms(400)
    check(q('DEC:STAT?') == '1', 'DEC:STAT? 1')
    check(q('DEC:RATE?') == '115200', 'auto-baud: DEC:RATE? 115200 (%s)' % q('DEC:RATE?'))
    lev = float(q('DEC:LEV?'))
    check(1.2 < lev < 2.1, 'auto threshold: DEC:LEV? %.3f V (3.3 V logic)' % lev)
    q('DEC:CLE')
    frames()
    if RC:
        RC.reset()
    q('DBG:PROF:RESET')
    wait_ms(1000)
    print(q('DBG:PROF?', raw=True).decode(errors='replace'), flush=True)
    fr = frames()
    data = b''.join(bytes.fromhex(f[4]) for f in fr)
    n = data.count(text)
    c = count()
    print('    %d frames, %d bytes, %d lines; counters %s' % (len(fr), len(data), n, c), flush=True)
    check(n >= 15, 'UART 115200: >= 15 intact lines (%d; 20 per s sent)' % n)
    check(c[2] == 0 and c[3] == 0, 'UART 115200: no framing / parity error (%d / %d)' % (c[2], c[3]))
    check(c[5] == 0, 'UART 115200: no stream lap (Lost %d)' % c[5])
    check(all(f[2] == 'UART' and f[3] == 'OK' for f in fr), 'UART frames: all UART,OK')
    ts = [float(f[1]) for f in fr]
    check(all(b > a for a, b in zip(ts, ts[1:])), 'frame times increase')
    seqs = [int(f[0]) for f in fr]
    check(seqs == list(range(seqs[0], seqs[0] + len(seqs))) if seqs else False, 'FIFO: consecutive sequence numbers, none lost')
    lm = loop_max()
    print('    main-loop pass max %d us' % lm, flush=True)
    c_check('UART 115200')
    shot('decode-uart')

    # ---- DMM alongside ----
    v = q('READ?', timeout=20)
    try:
        ok = abs(float(v)) < 1e30
    except ValueError:
        ok = False
    check(ok, 'DMM READ? answers while decoding (%s)' % v)
    c = count()
    check(c[5] == 0, 'no lap during the DMM read (Lost %d)' % c[5])

    # ---- the view busy: page / scroll / setup while frames arrive ----
    steps = [('DBG:TAP 447,40', 'page up'), ('DBG:TAP 447,121', 'page down'), ('DBG:TAP 447,81', 'LIVE'),
             ('DBG:TAP 447,170', 'SETUP'), (None, 'screenshot'), ('DBG:KEY 0', 'MENU closes the popup'),
             ('DBG:TAP 447,252', 'STOP'), ('DBG:TAP 447,252', 'RUN')]
    def prof_max():
        """(main-loop pass max, tick max) since DBG:PROF:RESET, us."""
        t = q('DBG:PROF?', raw=True).decode(errors='replace')
        lm = next((l for l in t.splitlines() if l.startswith('loop us:')), '')
        tk = next((l for l in t.splitlines() if l.startswith('tick ')), '')
        return (int(lm.split('max')[1].split()[0]) if 'max' in lm else -1, int(tk.split()[4]) if tk else -1)

    for cmd, what in steps:
        q('DBG:PROF:RESET')
        if cmd:
            q(cmd)
        else:
            shot('decode-setup')
        wait_ms(400)
        print('    %-22s Lost %d, pass max %d us, tick max %d us' % ((what, count()[5]) + prof_max()), flush=True)
    c = count()
    check(c[5] == 0, 'taps / popup while decoding: no lap (Lost %d)' % c[5])
    c_check('the view in use')

    # ---- RS-232: +-6 V inverted, 9600 7E1, rate set ----
    line = b'RS-232 line 7E1\r\n'
    put_bits('1' * 30 + uart_bits(line, '7E1'), 9600, 6.0, -6.0, 0.05)
    q('DEC:BAUD 9600;DEC:FORM 7E1;DEC:POL INV')
    wait_ms(300)
    q('DEC:CLE')
    frames()
    wait_ms(1200)
    fr = frames()
    data = b''.join(bytes.fromhex(f[4]) for f in fr)
    c = count()
    n = data.count(line)
    check(n >= 18, 'RS-232 inverted 9600 7E1: >= 18 intact lines in 1.2 s (%d; 24 sent)' % n)
    check(c[2] == 0 and c[3] == 0, 'RS-232: no framing / parity error (%d / %d)' % (c[2], c[3]))
    check(q('DEC:POL?') == 'INV' and q('DEC:FORM?') == '7E1' and q('DEC:BAUD?') == '9600', 'DEC:POL? / FORM? / BAUD? read back')
    shot('decode-rs232')

    # ---- baud mismatch ----
    put_bits('1' * 30 + uart_bits(text), 9600, 0.0, 5.0, 0.05)
    q('DEC:BAUD 19200;DEC:FORM 8N1;DEC:POL NORM')
    wait_ms(300)
    q('DEC:CLE')
    frames()
    wait_ms(1000)
    fr = frames()
    data = b''.join(bytes.fromhex(f[4]) for f in fr)
    c = count()
    check(c[2] > 50 and text not in data, 'signal 9600, decoder 19200: framing errors (%d), no text' % c[2])

    # ---- Modbus RTU 19200 8E1, auto ----
    baud = 19200
    gap = '1' * 60   # > 3.5 characters (38.5 bits)
    req = mb([0x01, 0x03, 0x00, 0x00, 0x00, 0x02])
    rsp = mb([0x01, 0x03, 0x04, 0x12, 0x34, 0x56, 0x78])
    w6 = mb([0x11, 0x06, 0x00, 0x05, 0xAB, 0xCD])
    exr = mb([0x01, 0x04, 0x00, 0x10, 0x00, 0x01])
    exc = mb([0x01, 0x84, 0x02])
    bad = bytearray(req)
    bad[6] ^= 0xFF
    seq = [req, rsp, w6, w6, exr, exc, bytes(bad)]
    bits = '1' * 40
    for fr_ in seq:
        bits += uart_bits(fr_, '8E1') + gap
    put_bits(bits, baud, 0.0, 3.3, 0.2)
    q('DEC:PROT MODB;DEC:BAUD AUTO;DEC:FORM 8E1')
    wait_ms(500)
    check(q('DEC:RATE?') == '19200', 'Modbus: auto-baud 19200 (%s)' % q('DEC:RATE?'))
    q('DEC:CLE')
    frames()
    wait_ms(1000)
    fr = frames()
    c = count()
    print('    %d frames; counters %s' % (len(fr), c), flush=True)
    for f in fr[:7]:
        print('    ' + ','.join(f), flush=True)
    # Align on the first request of a cycle.
    k = next((i for i, f in enumerate(fr) if f[4] == req.hex().upper()), None)
    cyc = fr[k:k + 7] if k is not None else []
    ok = len(cyc) == 7
    check(ok, 'Modbus: a full cycle of 7 frames found')
    if ok:
        check(cyc[0][5] == 'REQ 01 03 @0000 n=2', 'Modbus: request fields (%s)' % cyc[0][5])
        check(cyc[1][5].startswith('RSP 01 03 4B req=%s ' % cyc[0][0]), 'Modbus: reply paired (%s)' % cyc[1][5])
        dt = int(cyc[1][5].split('dt=')[1].rstrip('us'))
        want = 60 / baud * 1e6
        check(abs(dt - want) < 3 * 11 / baud * 1e6, 'Modbus: reply time %d us (gap %.0f us)' % (dt, want))
        check(cyc[2][5] == 'REQ 11 06 @0005 =ABCD' and cyc[3][5].startswith('RSP 11 06 @0005 =ABCD req='), 'Modbus: 06 write / echo')
        check(cyc[5][5].startswith('EXC 01 84 E02 req='), 'Modbus: exception paired (%s)' % cyc[5][5])
        check(cyc[6][3] == 'CRC' and all(f[3] == 'OK' for f in cyc[:6]), 'Modbus: CRC bad on the bad frame only')
    ncyc = c[0] / 7
    check(abs(c[4] - ncyc) <= 1.5, 'Modbus: CRC counter %d = frames / 7 (%.1f)' % (c[4], ncyc))
    check(c[2] == 0 and c[3] == 0 and c[5] == 0, 'Modbus: no framing / parity error, no lap %s' % c)
    shot('decode-modbus')
    c_check('Modbus')

    # ---- close ----
    q('DEC:STAT OFF')
    wait_ms(300)
    check(q('DEC:STAT?') == '0', 'DEC:STAT OFF closes the decoder')
    q('RUN')
    wait_ms(1500)
    st = q('TRIG:STAT?')
    check(st in ('AUTO', 'TD', 'WAIT', 'RUN'), 'the scope runs again (%s)' % st)
    check(err().startswith('0,'), 'no SCPI error left')
    # The stack high-water of the whole run (the decoder view's deep draw
    # paths with the decoder polled inside) against build.sh's static path.
    b = q('DBG:BOOT?')
    f = b.split(',')
    if 'STACK' in f:
        i = f.index('STACK')
        print('    stack high-water %s of %s bytes' % (f[i + 1], f[i + 2]), flush=True)
finally:
    if RC:
        try:
            c_check('the whole run', True)
        except Exception as e:
            print('region C check: %s' % e)
    os.killpg(emu.pid, 15)
    emu.wait()
    if RC:
        RC.cleanup()
    for p in (BITS, SCRIPT):
        if os.path.exists(p):
            os.remove(p)

print('decodetest: %s' % ('PASS' if not FAIL else 'FAIL: ' + '; '.join(FAIL)))
sys.exit(1 if FAIL else 0)
