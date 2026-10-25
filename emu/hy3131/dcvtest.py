#!/usr/bin/env python3
"""opendm40 DC-coupled V row on AD2 (feasibility 1.3 last row / E7, src/ad2exp.rs
`dcv_row` / `dc_enter`) in the emulator: LOG:SOURce AD2 in DC V.

  cd firmware && ./build.sh
  emu/hy3131/dcvtest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

Input: 2 V DC + 0.3 V rms 50 Hz on V/COM, the model's OP1 offset (op1vos,
0.37 mV x 10 at OP1O in 6 V) and an AD2 gain deviation (ad2dcg 1.01) that
the entry self-cal must take out. Checks:
  * DBG:AD2:DCV? 0,36,4: the device test 6.5 command answers (phases A..D,
    the zero D within 20 codes of the model's offset);
  * a host session (LOG:SOURce AD2, 4800 S/s, DC V 6 V): LOG:PREamble?
    mode 1, coupling 0 (DC); LOG:SCALe? b = the model's per-code (nominal
    x the per-range factor GR / 1.01) within 0.3 %; the records' mean (a + b raw) = the DC V reading
    within 3 mV, their peak-to-peak = 2 x 0.3 x sqrt 2 within 5 %, no loss;
    after LOG:STATe OFF the DMM reads 2 V again;
  * the same as a fast-logger disk session (LOG:DEST DISK, 2 s): the file's
    values (tools/dlog.py, the header's scale) have that mean too;
  * 25 V DC (the 60 V range, AUTO): the session's scale per-code ~10x, mean
    within 30 mV.
Exit status 0 = all passed.
"""
import argparse
import os
import struct
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF')
ap.add_argument('--port', type=int, default=3611, help='USB/IP port (default 3611)')
A = ap.parse_args()
NOM = 10 * 1.25 / 262144.0
DCG = 1.01
# The model's per-range per_code factor of this row (HY3131.cs Ad2DcGainR, 6 V / 60 V: the device's
# self-cal per_code against nominal, test 6.5 / acdcmeth e5; 8bb87e9): the node reads 1 / GR, on top
# of DCG. Without it LOG:SCALe? reads ~1 % (6 V) / ~1.9 % (60 V) off the expectation.
GR = (1.0096, 1.0188)
T25 = 150  # virtual s: the input becomes 25 V DC
SCRIPT = os.path.join(EMU, 'out', 'dcvtest-%d.script' % A.port)
os.makedirs(os.path.dirname(SCRIPT), exist_ok=True)
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402
import emusync  # noqa: E402

FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01',
       '--usbip-port', str(A.port), '--run-for', '3000', '--name', 'dcvtest-%d' % A.port,
       '--dmm', 'vsrc:2.0,ac=0.3,f=50;ad2dcg:%g' % DCG, '--dmm', 'vsrc:25.0@%d' % T25]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
mk = emusync.Markers(emusync.log_path(os.path.join(EMU, 'out'), 'dcvtest-%d' % A.port))
try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('dcvtest: no USB/IP device on port %d' % A.port)

    def q(line, raw=False, timeout=600):
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

    def read_v(n=3):
        v = None
        for _ in range(n):
            v = float(q('READ?'))
        return v

    def no_err(what):
        e = q('SYST:ERR?')
        check(e.startswith('0,') or e.startswith('+0,'), '%s: SYST:ERR? %s' % (what, e))

    def host_session(want, per_code, tol_v, secs=2.0):
        q('LOG:DEST HOST')
        q('LOG:SOURce AD2')
        q('LOG:SRATe 4800')
        q('LOG:STATe ON')
        no_err('LOG:STATe ON in DC V')
        pre = q('LOG:PREamble?').split(',')
        check(pre[0] == '1' and pre[6] == '1' and pre[10] == '0', 'LOG:PREamble? mode 1 (DC V), AD2, coupling DC (%s)' % ','.join(pre))
        a, b = [float(x) for x in q('LOG:SCALe?').split(',')]
        check(abs(b / per_code - 1) < 0.003, 'LOG:SCALe? per-code %.6g = the model\'s %.6g (+-0.3 %%)' % (b, per_code))
        vals = []
        t0 = up()
        while up() - t0 < secs * 1000:
            raw = q('LOG:DATA?', raw=True)
            n = int(raw[2:6])
            vals += [a + b * struct.unpack_from('<i', raw, 6 + o + 8)[0] for o in range(0, n - n % 12, 12)]
        q('LOG:STATe OFF')
        raw = q('LOG:DATA?', raw=True)
        cnt = [int(x) for x in q('LOG:COUNt?').split(',')]
        mean = sum(vals) / max(len(vals), 1)
        pp = max(vals) - min(vals) if vals else 0
        print('  %d records, mean %.5f V, p-p %.4f V, LOG:COUNt? %s' % (len(vals), mean, pp, cnt))
        check(len(vals) > 1000, 'records drained (%d)' % len(vals))
        check(abs(mean - want) < tol_v, 'mean %.5f V = the DC V reading %.5f V (+-%g)' % (mean, want, tol_v))
        check(cnt[2] == 0 and cnt[4] == 0, 'no loss (overwritten %d, lost %d)' % (cnt[2], cnt[4]))
        return a, b, pp

    print('*IDN?', q('*IDN?'))
    q('CONF:VOLT:DC')
    wait_ms(3000)
    ref = read_v()
    print('  DC V READ? %.5f' % ref)
    check(abs(ref - 2.0) < 0.002, 'DC V reads 2 V (%.5f)' % ref)

    r = q('DBG:AD2:DCV? 0,36,4')
    print('  DBG:AD2:DCV? 0,36,4 ->', r)
    p = r.split(';')
    d = [int(x) for x in p[4].split(',')]
    zero = 0.37e-3 * 10 * DCG / GR[0] / 1.25 * 262144
    check(len(p) == 7 and d[0] > 0 and abs(d[1] / 1000 - zero) < 20, 'DCV? phases answer, zero D %.1f ~ %.1f codes' % (d[1] / 1000, zero))
    q('CONF:VOLT:AC')  # DCV? left the range manual: AUTO again (CONF of the function in force is a no-op)
    q('CONF:VOLT:DC')
    wait_ms(3000)
    ref = read_v()

    a, b, pp = host_session(ref, NOM * GR[0] / DCG, 0.003)
    check(abs(pp - 2 * 0.3 * 2 ** 0.5) < 0.05 * 0.85 + 0.01, 'p-p %.4f V = the 0.3 V rms sine (DC-coupled, both)' % pp)
    wait_ms(1500)
    v = read_v()
    check(abs(v - 2.0) < 0.002, 'DMM back after the session: %.5f V' % v)

    # The fast logger to the disk, DC V on AD2.
    q('LOG:DEST DISK')
    q('LOG:DURation 2')
    q('LOG:STATe ON')
    no_err('LOG:DEST DISK session in DC V')
    st = []
    for _ in range(2000):
        st = [int(x) for x in q('LOG:DISK?').split(',')]
        if st[0] >= 5:
            break
        time.sleep(0.5)
    check(st[0] in (5, 6), 'disk session ended (LOG:DISK? %s)' % st)
    name = 'LOGS/FAST%03d.DLG' % st[1]
    dlg = b''
    while True:
        bb = q('MMEM:DATA? "%s",%d' % (name, len(dlg)), raw=True)
        n = int(bb[2:6])
        dlg += bytes(bb[6:6 + n])
        if n < 6000:
            break
    path = os.path.join(EMU, 'out', 'dcvtest-%d.dlg' % A.port)
    open(path, 'wb').write(dlg)
    out = subprocess.run([sys.executable, '-I', os.path.join(ROOT, 'firmware/tools/dlog.py'), path], capture_output=True, text=True).stdout
    rows = [[x.strip() for x in l.split(',')] for l in out.splitlines() if l.strip()[:1] in '0123456789']
    vals = [float(x[2]) for x in rows if len(x) > 3 and x[2]]
    mean = sum(vals) / max(len(vals), 1)
    print('  %s: %d rows, mean %.5f V' % (name, len(vals), mean))
    check(len(vals) > 1000 and abs(mean - ref) < 0.003, 'the file\'s values (its header scale) mean %.5f = %.5f V' % (mean, ref))
    q('LOG:DEST HOST')

    # 25 V DC from virtual T25 s on (run.py --dmm SPEC@T): the 60 V range. Its fw
    # uptime is its input marker (emu/emusync.py; predicted by the clock marker).
    if mk.wait_clock() is None:
        sys.exit('dcvtest: no clock marker (dwt: CYCCNT write) in %s' % mk.log)
    f25 = mk.at(T25)
    check(up() < f25 * 1000, 'the 2 V part done before the 25 V input (fw %.2f s, now %.2f s)' % (f25, up() / 1000))
    while up() < f25 * 1000 + 500:
        time.sleep(0.5)
    q('CONF:VOLT:DC')
    wait_ms(4000)
    ref = read_v()
    print('  DC V READ? %.4f (25 V)' % ref)
    check(abs(ref - 25.0) < 0.02, 'DC V reads 25 V (%.4f)' % ref)
    a, b, pp = host_session(ref, NOM * 10 * GR[1] / DCG, 0.03)
except SystemExit:
    raise
finally:
    os.killpg(emu.pid, 15)
    emu.wait()

print('dcvtest: %s' % ('PASS' if not FAIL else 'FAIL: ' + '; '.join(FAIL)))
sys.exit(1 if FAIL else 0)
