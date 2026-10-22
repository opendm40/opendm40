#!/usr/bin/env python3
"""fw-rs fast logger to the SPI disk (src/fastlog.rs, LOG:DEST DISK) in the
emulator, with the W25Q128 model's busy timing on (page program / erase
times, virtual time): the sustainable rate into the flash per rung.

  cd fw-rs && ./build.sh
  emu/fastlogtest.py --app fw-rs/target/native/thumbv7em-none-eabihf/release/dm40-fw

Per flash profile (--profiles: typ = tPP 0.4 ms, tSE 45 ms, tBE64 150 ms;
max = 3 ms, 400 ms, 2 s; datasheet W25Q128JV) and per session (--sessions:
AD2 rates in AC V, AD1 OSR 0 chop off = 640 S/s in DC V):
  LOG:DEST DISK, LOG:DURation, LOG:STATe ON; LOG:DISK? polled until the
  session ends; the Prep time (DBG:PING? uptime), LOG:COUNt? (overwritten =
  the ring was full: the disk did not keep up), the file over MMEM:DATA?
  decoded by tools/dlog.py: rows, sum of lost_before, bytes per second,
  pages per second. A session "keeps up" when no sample is lost.
Bluetooth (--bt-port passthrough): LOG:STATe ON refused for AD2 ("Rate too
high for Bluetooth") and with LOG:DEST DISK ("Command protected"); an AD1
40 S/s session drains over BT LOG:DATA?.

Caveats: the emulator's SPI bus is instant (the 12.5 MHz byte time, ~0.2 ms
per 256-byte page on the device, is CPU time the model does not charge) and
its main loop runs at 200 MIPS without flash wait states; the chip timing
(busy) is modelled. Exit status 0 = every check passed; the rate table is
printed (docs/compact-log-format.md "Fast stream: rate limits").
"""
import argparse
import os
import socket
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='fw-rs native ELF / image')
ap.add_argument('--port', type=int, default=4817, help='USB/IP port (default 4817)')
ap.add_argument('--bt-port', type=int, default=4818, help='BT passthrough port (default 4818)')
ap.add_argument('--dur', type=int, default=2, help='LOG:DURation per session, s (device time)')
ap.add_argument('--profiles', default='typ,max')
ap.add_argument('--sessions', default='ad1:640,ad2:4800,ad2:9600,ad2:19200,ad2:38400')
ap.add_argument('--no-bt', action='store_true')
A = ap.parse_args()
PROF = {'typ': (400, 45_000, 120_000, 150_000), 'max': (3000, 400_000, 1_600_000, 2_000_000)}
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, HERE)
import usbip_client as uc  # noqa: E402

FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


def run_emu(profile):
    p = PROF[profile]
    script = os.path.join(HERE, 'out', 'fastlog-%d.script' % A.port)
    with open(script, 'w') as f:
        f.write('0 monitor sysbus.spiflash ProgramUs %d\n' % p[0])
        f.write('0 monitor sysbus.spiflash Erase4kUs %d\n' % p[1])
        f.write('0 monitor sysbus.spiflash Erase32kUs %d\n' % p[2])
        f.write('0 monitor sysbus.spiflash Erase64kUs %d\n' % p[3])
    cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', '200', '--app', A.app,
           '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '5000', '--name', 'fastlog-%d' % A.port,
           '--dmm', 'vsrc:0.2,ac=0.5,f=50', '--script', script]
    if not A.no_bt:
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
    sys.exit('fastlogtest: no USB/IP device on port %d' % A.port)


def decode(dlg):
    """tools/dlog.py of the bytes: (rows, sum lost_before, header ok)."""
    path = os.path.join(HERE, 'out', 'fastlog-%d.dlg' % A.port)
    open(path, 'wb').write(dlg)
    r = subprocess.run([sys.executable, '-I', os.path.join(ROOT, 'fw-rs/tools/dlog.py'), path], capture_output=True, text=True)
    if r.returncode:
        return 0, 0, False
    rows = [l for l in r.stdout.splitlines() if l[:1] in ' 0123456789']
    return len(rows), sum(int(l.rsplit(',', 1)[1]) for l in rows), True


TABLE = []


def session(q, src, rate, profile):
    q('LOG:STATe OFF')
    q('*CLS')
    if src == 'ad2':
        q('CONF:VOLT:AC')
        q('LOG:SOURce AD2')
        q('LOG:SRATe %d' % rate)
        q('LOG:CHOP ON')
    else:
        q('CONF:VOLT:DC')
        q('LOG:SOURce AD1')
        q('LOG:OSR 0')
        q('LOG:CHOP %s' % ('OFF' if rate == 640 else 'ON'))
    time.sleep(3)
    q('LOG:DEST DISK')
    q('LOG:DURation %d' % A.dur)
    up0 = int(q('DBG:PING?').split(',')[0])
    q('LOG:STATe ON')
    err = q('SYST:ERR?')
    check(err.startswith('0,'), '%s %d: LOG:STATe ON accepted (%s)' % (src, rate, err))
    t_run = None
    st = []
    for _ in range(2000):
        st = [int(x) for x in q('LOG:DISK?').split(',')]
        if st[0] == 3 and t_run is None:
            t_run = int(q('DBG:PING?').split(',')[0])
        if st[0] >= 5:
            break
        time.sleep(0.5)
    up1 = int(q('DBG:PING?').split(',')[0])
    cnt = [int(x) for x in q('LOG:COUNt?').split(',')]
    pre = q('LOG:PREamble?').split(',')
    nominal = int(pre[8]) / 1000 if len(pre) > 8 else rate
    check(st[0] in (5, 6), '%s %d: the session ended (LOG:DISK? %s)' % (src, rate, st))
    name = 'LOGS/FAST%03d.DLG' % st[1]
    dlg = b''
    while True:
        b = q('MMEM:DATA? "%s",%d' % (name, len(dlg)), raw=True)
        n = int(b[2:6])
        dlg += bytes(b[6:6 + n])
        if n < 6000:
            break
    rows, lost, ok = decode(dlg)
    check(ok and rows == st[4] and len(dlg) == st[3], '%s %d: %s decodes: %d rows, %d bytes' % (src, rate, name, rows, len(dlg)))
    prep_s = ((t_run or up1) - up0) / 1000
    run_s = (up1 - (t_run or up1)) / 1000
    span = rows / nominal if nominal else 0
    bps = len(dlg) / max(span, 1e-9)
    TABLE.append((profile, src, rate, prep_s, rows, lost, cnt[2], bps, bps / 256, len(dlg) / max(rows, 1)))
    print('  %s %s %d: prep %.1f s, rows %d, lost %d (ring overwritten %d), %.0f B/s, %.0f pages/s, %.2f B/sample'
          % (profile, src, rate, prep_s, rows, lost, cnt[2], bps, bps / 256, len(dlg) / max(rows, 1)), flush=True)
    return lost


def bt_checks():
    try:
        s = socket.create_connection(('127.0.0.1', A.bt_port), timeout=5)
    except OSError as e:
        check(False, 'BT passthrough (%s)' % e)
        return
    s.settimeout(20)
    buf = b''

    def line(cmd, reply=True):
        nonlocal buf
        s.sendall(cmd.encode() + b'\n')
        if not reply:
            return None
        while b'\n' not in buf:
            d = s.recv(65536)
            if not d:
                return None
            buf += d
        l, _, buf = buf.partition(b'\n')
        return l

    def block(cmd):
        nonlocal buf
        s.sendall(cmd.encode() + b'\n')
        while len(buf) < 6:
            buf += s.recv(65536)
        n = int(buf[2:6])
        while len(buf) < 6 + n + 1:
            buf += s.recv(65536)
        d, buf = buf[6:6 + n], buf[6 + n + 1:]
        return d

    try:
        line('*IDN?')
        line('LOG:STATe OFF', False)
        line('LOG:DEST HOST', False)
        line('CONF:VOLT:AC', False)
        time.sleep(2)
        line('LOG:SOURce AD2', False)
        line('LOG:SRATe 1200', False)
        line('*CLS', False)
        line('LOG:STATe ON', False)
        e = line('SYST:ERR?').decode()
        st = line('LOG:STATe?').decode()
        check('Rate too high for Bluetooth' in e and st == '0', 'BT: AD2 1200 S/s refused (%s, STATe? %s)' % (e, st))
        line('LOG:DEST DISK', False)
        line('LOG:STATe ON', False)
        e = line('SYST:ERR?').decode()
        check('Command protected' in e, 'BT: LOG:DEST DISK session refused (%s)' % e)
        line('LOG:DEST HOST', False)
        line('CONF:VOLT:DC', False)
        time.sleep(2)
        line('LOG:SOURce AD1', False)
        line('LOG:OSR 3', False)
        line('LOG:CHOP ON', False)
        line('LOG:STATe ON', False)
        e = line('SYST:ERR?').decode()
        n = 0
        t0 = time.time()
        while time.time() - t0 < 20:
            n += len(block('LOG:DATA?')) // 12
            time.sleep(0.5)
        line('LOG:STATe OFF', False)
        check(e.startswith('0,') and n > 20, 'BT: AD1 40 S/s session drained over BT LOG:DATA? (%d records, %s)' % (n, e))
    except Exception as x:
        check(False, 'BT: %r' % x)
    finally:
        s.close()


for profile in A.profiles.split(','):
    print('profile %s: tPP %d us, tSE %d us, tBE64 %d us' % (profile, PROF[profile][0], PROF[profile][1], PROF[profile][3]), flush=True)
    emu = run_emu(profile)
    try:
        t = connect()

        def q(line, raw=False, timeout=120):
            t.write(line)
            if '?' not in line:
                return None
            b = t.read(maxlen=8192, timeout=timeout)
            return b if raw else b.decode(errors='replace').strip()

        print('*IDN?', q('*IDN?'), flush=True)
        for sv in A.sessions.split(','):
            src, rate = sv.split(':')
            lost = session(q, src, int(rate), profile)
            if profile == 'typ' and int(rate) <= 9600:
                check(lost == 0, '%s %s %s: no sample lost' % (profile, src, rate))
        if not A.no_bt and profile == A.profiles.split(',')[0]:
            bt_checks()
    finally:
        stop_emu(emu)

print('\nprofile  src  rate   prep_s   rows   lost  ring_ovw   B/s   pages/s  B/sample')
for r in TABLE:
    print('%-7s %-4s %6d %7.1f %7d %6d %8d %7.0f %7.0f %8.2f' % r)
print('FAIL: %d checks' % len(FAIL) if FAIL else 'PASS')
sys.exit(1 if FAIL else 0)
