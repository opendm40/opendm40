#!/usr/bin/env python3
"""fw-rs inrush current capture (src/inrush.rs, src/app_inrush.rs, TODO
"Inrush current", docs/inrush-current.md) in the emulator: stepped currents
into the A jack (the DM40 front-end model's isrc source, switched at fixed
virtual times), driven like a user (DBG:KEY / DBG:TAP) and over SCPI on one
USB/IP connection.

  cd fw-rs && ./build.sh
  emu/hy3131/inrushtest.py --app fw-rs/target/native/thumbv7em-none-eabihf/release/dm40-fw [--shots DIR]

Timeline (virtual seconds; the scheduled input changes):
  0      A-COM 0.05 A DC (idle)
  T1     a lamp-like DC step: 12 A for 20 ms, 6 A for 80 ms, then 3 A:
         Irms(100 ms) = sqrt((144 * 20 + 36 * 80) / 100) = 7.59 A, peak 12 A
  T2     2 A rms 50 Hz running (an AC load already on)
  T3     the AC load steps to 5 A rms: Irms(100 ms) 5 A, |peak| 7.07 A
Checks:
  * MF in DC A -> "Inrush" (Current Settings): the AC A row of the 10 A
    range, the stream ISR runs (DBG:ISR? active), INR:COND? ARMED before T1;
  * T1: INR:FETCh? Irms 7.59 A (+-2 %), peak 12 A (+-1 %), idle 0.05 A;
    INR:FILE? LOGS/FASTnnn.DLG; its CSV (MMEM:DATA?, tools/dlog.py) has
    inrush::LEN rows, 0 lost, the step at t_s ~0.200 s, values = INR:DATA?;
    the stream stopped, the DMM reads again (READ?);
  * INR:STATe OFF: the page closes, FUNC? = CURR (the DC A function back);
  * INR:STATe ON again (SCPI entry) after T2: the running 2 A AC load does
    not trigger a 1 A step (still ARMED just before T3); T3 triggers:
    Irms 5 A (+-3 %), idle 2 A (+-3 %), |peak| 7.07 A (+-3 %);
  * INR:ARM, then a host LOG:CSV ON: refused ("Settings conflict: inrush
    page"), LOG:CSV? off, still ARMED; no region C code and no slow-bank
    data read while it is handled (emu/regionc.py; --no-region-c: skipped);
  * INR:ARM then a host CONF:VOLT:DC: the capture stops (INR:COND? IDLE,
    DBG:ISR? active 0), DC V reads; INR:STATe OFF keeps the host's DC V;
    INR:STATe ON from DC V selects AC A, OFF puts DC V back.
The emulator runs on a private copy of the SPI disk image and writes its own
Renode log (emu/out/inrush-<port>.log, the run.py log cap applies); both
are per process (port + pid). Exit status 0 = all passed.
"""
import argparse
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='fw-rs native ELF (a build with the stream ISR, the default)')
ap.add_argument('--port', type=int, default=3597, help='USB/IP port (default 3597)')
ap.add_argument('--shots', help='directory for PNG screenshots')
ap.add_argument('--mips', default='200')
ap.add_argument('--t1', type=float, default=16.0, help='virtual s of the DC step')
ap.add_argument('--no-region-c', action='store_true', help='without the region C hooks (emu/regionc.py): by default a '
                'host LOG:CSV ON on the armed page (refused: "Settings conflict: inrush page") must run no region C '
                'code and read nothing from the slow flash bank')
A = ap.parse_args()
T1 = A.t1
T2 = T1 + 14.0
T3 = T2 + 8.0
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


OUT = os.path.join(EMU, 'out')
os.makedirs(OUT, exist_ok=True)
tag = 'inrush-%d-%d' % (A.port, os.getpid())
disk = os.path.join(OUT, '%s.disk.bin' % tag)
shutil.copy(os.path.join(ROOT, 'firmware/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin'), disk)
script = os.path.join(OUT, '%s.script' % tag)
sys.path.insert(0, EMU)
import regionc  # noqa: E402
# Not hooked: the save's Prep / FAT writer (hot while not streaming: a
# Python hook per call stretches it; as fastlogtest --region-c).
RC = None if A.no_region_c else regionc.Hooks(A.app, OUT, tag, exclude=('fastlog::prep::', 'dm40_fw::fatren::'))
with open(script, 'w') as f:
    if RC:
        f.write(RC.lines())
    for t, spec in [(T1, 'isrc:12'), (T1 + 0.020, 'isrc:6'), (T1 + 0.100, 'isrc:3'),
                    (T2, 'isrc:0,ac=2,f=50'), (T3, 'isrc:0,ac=5,f=50')]:
        f.write('%.4f dmm %s\n' % (t, spec))
log = os.path.join(OUT, '%s.log' % tag)
cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '3000', '--name', tag, '--log', log,
       '--disk', disk, '--dmm', 'isrc:0.05', '--script', script]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)


class Done(Exception):
    pass


try:
    t = None
    for _ in range(120):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('inrushtest: no USB/IP device on port %d' % A.port)

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

    def wait_until(t_s):
        while up() < t_s * 1000:
            time.sleep(0.2)

    def cond():
        return q('INR:COND?')

    def wait_cond(want, max_ms):
        t0 = up()
        c = cond()
        while c not in want and up() - t0 < max_ms:
            time.sleep(0.3)
            c = cond()
        return c

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

    def fetch():
        return [float(x) for x in q('INR:FETCh?').split(',')]

    def near(v, want, tol):
        return abs(v - want) <= tol * abs(want)

    def isr_active():
        return int(q('DBG:ISR?').split(',')[1])

    print('*IDN?', q('*IDN?'), flush=True)
    q('CONF:CURR:DC')
    wait_ms(3000)
    print('  DC A before: READ? %s A' % q('READ?'), flush=True)
    # MF -> Current Settings -> Inrush (the note row's right column).
    q('DBG:KEY 3')
    wait_ms(600)
    shot('i00-range-a')
    q('DBG:TAP 394,159')
    wait_ms(800)
    c = wait_cond(('ARMED',), 3000)
    check(c == 'ARMED', 'MF -> Inrush: armed (INR:COND? %s)' % c)
    check(q('INR:STATe?') == '1' and isr_active() == 1, 'the page is open, the stream ISR runs')
    check(q('FUNC?').strip('"').upper() == 'CURR:AC', 'the AC A row (FUNC? %s)' % q('FUNC?'))
    pre = q('LOG:PREamble?').split(',')
    check(pre[:2] == ['4', '1'], 'the 10 A range of AC A (the stream: LOG:PRE? mode,range %s)' % pre[:2])
    shot('i01-armed')
    now = up()
    check(now < T1 * 1000 - 500, 'armed before the step (up %.1f s, step at %.1f s)' % (now / 1000, T1))
    if now >= T1 * 1000 - 500:
        raise Done
    wait_until(T1 + 0.5)
    c = cond()
    check(c in ('TRIGGERED', 'SAVING', 'DONE'), 'the DC step triggers (%s)' % c)
    shot('i02-triggered')
    c = wait_cond(('DONE',), 12000)
    check(c == 'DONE', 'captured and saved (%s)' % c)
    r = fetch()
    want = (((144 * 20 + 36 * 80) / 100) ** 0.5)
    print('  DC step: INR:FETCh? %s (Irms want %.3f A)' % (r, want), flush=True)
    check(near(r[0], want, 0.02), 'Irms(100 ms) %.3f A ~ %.3f A' % (r[0], want))
    check(near(r[1], 12.0, 0.01), 'peak %.3f A ~ 12 A' % r[1])
    check(abs(r[2] - 0.05) < 0.01, 'idle %.4f A ~ 0.05 A' % r[2])
    check(r[3] == 0, 'no samples lost (%d)' % r[3])
    check(isr_active() == 0, 'the stream stopped after the capture')
    shot('i03-done-dc')
    f = q('INR:FILE?').strip('"')
    check(f.startswith('LOGS/FAST') and f.endswith('.DLG'), 'INR:FILE? %s' % f)
    pre = q('INR:PREamble?').split(',')
    rate, n, ti, a, b = int(pre[0]), int(pre[1]), int(pre[2]), float(pre[3]), float(pre[4])
    check(rate == 9600 and n == 19200 and ti == 1920, 'INR:PRE? %s' % pre)
    codes = []
    while len(codes) < n:
        blk = q('INR:DATA? %d,3000' % len(codes), raw=True)
        k = int(blk[2:6])
        d = blk[6:6 + k]
        if k == 0:
            break
        codes += [int.from_bytes(d[i:i + 2], 'little', signed=True) for i in range(0, k, 2)]
    check(len(codes) == n, 'INR:DATA? gives the whole trace (%d codes)' % len(codes))
    amps = [a + b * c for c in codes]
    check(max(amps[:ti - 10]) < 0.2 and near(max(amps), 12.0, 0.01), 'trace: idle before t0, 12 A peak after')
    # The saved file: the CSV generated from the DLG (MMEM:DATA?).
    csv = b''
    name = f.replace('.DLG', '.CSV')
    while True:
        blk = q('MMEM:DATA? "%s",%d' % (name, len(csv)), raw=True)
        k = int(blk[2:6])
        csv += bytes(blk[6:6 + k])
        if k < 6000:
            break
    rows = [l for l in csv.decode(errors='replace').splitlines() if l[:1] in ' 0123456789']
    hdr = [l for l in csv.decode(errors='replace').splitlines() if l.startswith('#')]
    print('  %s: %d bytes, %d rows; header %s' % (name, len(csv), len(rows), hdr[:2]), flush=True)
    check(len(rows) == n, 'the CSV has the trace (%d rows)' % len(rows))
    if len(rows) == n:
        v = [float(l.split(',')[2]) for l in rows]
        lost = sum(int(l.rsplit(',', 1)[1]) for l in rows)
        t0 = float(rows[ti].split(',')[0])
        raw_ok = all(int(rows[i].split(',')[1]) == codes[i] for i in range(0, n, 97))
        check(lost == 0 and abs(t0 - 0.2) < 0.001 and raw_ok, 'CSV: lost 0, t0 at %.6f s, raw = INR:DATA?' % t0)
        check(near(max(v), 12.0, 0.01), 'CSV: peak %.3f A' % max(v))
    rd = q('READ?')
    print('  DMM after the capture: READ? %s' % rd, flush=True)
    q('INR:STATe OFF')
    wait_ms(1500)
    check(q('INR:STATe?') == '0' and q('FUNC?').strip('"').upper() == 'CURR', 'INR:STAT OFF: page closed, DC A back (FUNC? %s)' % q('FUNC?'))
    shot('i04-closed')

    # The AC load: running from T2, the step at T3 (SCPI entry).
    wait_until(T2 + 1.0)
    q('INR:THReshold 1')
    q('INR:STATe ON')
    c = wait_cond(('ARMED',), 3000)
    check(c == 'ARMED', 'INR:STAT ON with the 2 A AC load running: armed (%s)' % c)
    now = up()
    check(now < T3 * 1000 - 1500, 'armed before the AC step (up %.1f s)' % (now / 1000))
    # (DBG:PING? uptime runs behind the virtual time of the scheduled
    # inputs by the boot chain: a margin of 2 s.)
    wait_until(T3 - 2.0)
    c = cond()
    check(c == 'ARMED', 'a running 2 A rms load does not trigger a 1 A step (%s, %.1f s of it)' % (c, T3 - 2.0 - T2))
    shot('i05-armed-ac')
    wait_until(T3 + 0.5)
    c = wait_cond(('DONE',), 12000)
    r = fetch()
    print('  AC step: INR:FETCh? %s' % r, flush=True)
    check(c == 'DONE', 'the AC step triggers and saves (%s)' % c)
    check(near(r[0], 5.0, 0.03), 'Irms(100 ms) %.3f A ~ 5 A' % r[0])
    check(near(abs(r[1]), 7.071, 0.03), '|peak| %.3f A ~ 7.07 A' % abs(r[1]))
    check(near(r[2], 2.0, 0.03), 'idle %.3f A rms ~ 2 A' % r[2])
    shot('i06-done-ac')
    wait_ms(3000)
    rd = float(q('READ?'))
    print('  the DMM on the 5 A load after the capture: READ? %.4f A AC' % rd, flush=True)
    check(near(r[0], rd, 0.03), 'Irms(100 ms) %.3f A ~ the DMM AC A reading %.3f A' % (r[0], rd))

    # A host CONF while armed: the capture stops, the page says why.
    q('INR:ARM')
    c = wait_cond(('ARMED',), 3000)
    check(c == 'ARMED', 'INR:ARM re-arms (%s)' % c)
    # A CSV recording is refused on the inrush page (the DMM poll is paused
    # while the capture streams: it would record no rows).
    if RC:
        RC.reset()
    q('*CLS')
    q('LOG:CSV ON')
    err = q('SYST:ERR?')
    st = q('LOG:CSV?').split(',')
    c = cond()
    check('inrush page' in err and st[0] != '1' and c == 'ARMED' and isr_active() == 1,
          'LOG:CSV ON on the armed inrush page refused (%s; LOG:CSV? %s; %s)' % (err, ','.join(st), c))
    if RC:
        got = RC.calls()
        print('  region C functions / slow-bank reads while streaming: %s' % (', '.join(sorted(got)) or 'none'), flush=True)
        check(not got, 'LOG:CSV ON on the armed page: no region C code / slow-bank read while streaming%s'
              % ('' if not got else ' (%s)' % ', '.join(sorted(got))))
    q('CONF:VOLT:DC')
    wait_ms(1500)
    c = cond()
    check(c == 'IDLE' and isr_active() == 0, 'a host CONF stops the capture (INR:COND? %s)' % c)
    shot('i07-lost')
    wait_ms(1500)
    rd = q('READ?')
    check(abs(float(rd)) < 0.05, 'DC V reads after it (%s)' % rd)
    q('INR:STATe OFF')
    wait_ms(800)
    check(q('INR:STATe?') == '0' and q('FUNC?').strip('"').upper() == 'VOLT', 'INR:STATe OFF: the host\'s DC V stays (FUNC? %s)' % q('FUNC?'))
    # From DC V: the page selects AC A; closing puts DC V back.
    q('INR:STATe ON')
    c = wait_cond(('ARMED',), 3000)
    check(c == 'ARMED' and q('FUNC?').strip('"').upper() == 'CURR:AC', 'INR:STAT ON from DC V: AC A, armed (%s)' % c)
    q('INR:STATe OFF')
    wait_ms(1500)
    check(q('FUNC?').strip('"').upper() == 'VOLT' and isr_active() == 0, 'INR:STAT OFF: DC V back (FUNC? %s)' % q('FUNC?'))
    err = q('SYST:ERR?')
    print('  SYST:ERR? %s' % err, flush=True)
except Done:
    pass
finally:
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
    if RC:
        RC.cleanup()

print('inrushtest: %s' % ('PASS' if not FAIL else 'FAIL (%d)' % len(FAIL)))
sys.exit(1 if FAIL else 0)
