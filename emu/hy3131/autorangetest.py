#!/usr/bin/env python3
"""fw-rs DC V autorange after a multi-step range change, and READ? across a
range change (device bab5dd4, device-check doc "bab5dd4" 1 / 3).

  cd firmware && ./build.sh
  emu/hy3131/autorangetest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

Input: 1 V DC on V/COM with the model's AD1 decimation memory on
(`ad1mix:1`, HY3131.cs Ad1Mix): a row write that does not restart AD1 (the
second of two autorange steps, both rows at OSR 8192) leaves the next
conversions a sinc^3 mix of the old divider tap and the new one. On the
device that gave one READ? of 0.767-0.790 V in 5 of 35 `VOLT:RANG:AUTO ON`
from 600 / 1000 V (two steps down to 6 V), never from 60 V (one step).
Checks, per trial (manual 600 / 1000 / 60 V, a varied wait, then
`VOLT:RANG:AUTO ON` and READ? for 2.5 s):
  * every READ? that answers a number is 1 V within 5 % (+9.91E37 = no
    reading is allowed while the range moves); the first one, the 600 /
    1000 V conversion that makes the first step, within 15 % (1 V is 0.1 %
    of that range: its noise is a few %, as on the device);
  * `CALC:AVER:MIN?` (the chart / statistics minimum, cleared before the
    command) stays above 0.85 V (the model's mixed readings: 0.55 V);
  * the meter ends on the 6 V range.
Then AC+DC on the same 1 V: after `VOLT:ACDC:RANG 60` / `6` READ?
answers +9.91E37 while the AC part settles, never the reading from before
the change, and the first fresh reading is 1 V within 0.2 %, at most 6.5 s
after the command (emulator time; poll granularity included).
First, the range queries in every function and coupling (device d330877:
`VOLT:RANG:AUTO?` in AC V answered +9.91E37): a bare VOLTage / CURRent
names the quantity in any coupling (DC, AC, AC+DC, AUTO), so `<q>:RANG:AUTO?`
answers 1 / 0 (0 after `<q>:RANG:AUTO OFF`), `<q>:RANG?` and `<q>:RES?` a
number, with no error; the explicit present coupling the same; in the power
meter `VOLT:RANG?` / `CURR:RANG?` / `:AUTO?` answer its V / I channel
(as POW:VOLT:RANG?). A query never raises "Settings conflict": another
function's (another quantity, another explicit coupling, any in TEMP)
`:RANG:AUTO?` answers 1, its `:RANG?` / `:RES?` +9.91E37, no error; a set
naming it is "Settings conflict"; TEMP:RANG? is "Undefined header" (no
number).
  --queries-only: that part only (~1 min).
Exit status 0 = all passed.
"""
import argparse
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='fw-rs native ELF')
ap.add_argument('--port', type=int, default=30410, help='USB/IP port (default 30410)')
ap.add_argument('--trials', type=int, default=12, help='AUTO ON trials (default 12)')
ap.add_argument('--mix', type=int, default=1, help='ad1mix model option (default 1)')
ap.add_argument('--queries-only', action='store_true', help='only the range queries in every function / coupling')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

FAIL = []
NAN = 9.9e37


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01',
       '--usbip-port', str(A.port), '--run-for', '4000', '--name', 'autorangetest-%d' % A.port,
       '--dmm', 'dcv:1.0;ad1mix:%d' % A.mix]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('autorangetest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=600):
        t.write(line)
        if '?' not in line:
            return None
        return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0])

    def wait_ms(ms):
        t0 = up()
        while up() - t0 < ms:
            time.sleep(0.2)

    def num(r):
        try:
            return float(r)
        except (TypeError, ValueError):
            return None

    def qerr(line):
        """`line`'s reply and the error it left (SYST:ERR?)."""
        q('*CLS')
        r = q(line)
        return r, q('SYST:ERR?')

    def want_ok(fn, line, ok, what):
        r, e = qerr(line)
        check(ok(r) and e.startswith('0,'), '%s: %s -> %s (%s)%s' % (fn, line, r, e, ' ' + what if what else ''))

    def is_bool(b):
        return lambda r: r == b

    def finite(r):
        v = num(r)
        return v is not None and 0 < v < NAN

    def want_nan(fn, line):
        """Another function's query: AUTO? 1, else +9.91E37; no error."""
        r, e = qerr(line)
        if line.endswith(':AUTO?'):
            check(r == '1' and e.startswith('0,'), '%s: %s -> %s (%s), want 1 and no error' % (fn, line, r, e))
        else:
            check(num(r) is not None and num(r) >= NAN and e.startswith('0,'), '%s: %s -> %s (%s), want +9.91E37 and no error' % (fn, line, r, e))

    # Range queries in every function and coupling.
    for conf, bare, expl, other in [('CONF:VOLT:DC', 'VOLT', 'VOLT:DC', 'CURR'), ('CONF:VOLT:AC', 'VOLT', 'VOLT:AC', 'CURR'),
                                    ('CONF:VOLT:ACDC', 'VOLT', 'VOLT:ACDC', 'CURR'), ('CONF:VOLT:AUTO', 'VOLT', 'VOLT:AUTO', 'CURR'),
                                    ('CONF:CURR:DC', 'CURR', 'CURR:DC', 'VOLT'), ('CONF:CURR:AC', 'CURR', 'CURR:AC', 'VOLT'),
                                    ('CONF:CURR:ACDC', 'CURR', 'CURR:ACDC', 'VOLT'), ('CONF:CURR:AUTO', 'CURR', 'CURR:AUTO', 'VOLT'),
                                    ('CONF:RES', 'RES', 'FRES', 'VOLT'), ('CONF:CAP', 'CAP', 'CAP', 'VOLT'),
                                    ('CONF:FREQ', 'FREQ', 'FREQ', 'VOLT')]:
        q(conf)
        wait_ms(1500)
        for h in dict.fromkeys([bare, expl, 'SENS:' + bare]):
            want_ok(conf, h + ':RANG:AUTO?', is_bool('1'), '(want 1)')
            want_ok(conf, h + ':RANG?', finite, '')
        want_ok(conf, bare + ':RES?', finite, '')
        q(bare + ':RANG:AUTO OFF')
        want_ok(conf, bare + ':RANG:AUTO?', is_bool('0'), '(want 0 after RANG:AUTO OFF)')
        want_ok(conf, expl + ':RANG:AUTO?', is_bool('0'), '(want 0 after RANG:AUTO OFF)')
        q(bare + ':RANG:AUTO ON')
        want_ok(conf, bare + ':RANG:AUTO?', is_bool('1'), '(want 1 after RANG:AUTO ON)')
        want_nan(conf, other + ':RANG:AUTO?')
        want_nan(conf, other + ':RANG?')
        r, e = qerr(other + ':RANG:AUTO OFF')
        check('Settings conflict' in e, '%s: %s:RANG:AUTO OFF refused (%s)' % (conf, other, e))
        if bare in ('VOLT', 'CURR'):
            # another explicit coupling stays strict
            alt = bare + (':AC' if expl.endswith(':DC') else ':DC')
            want_nan(conf, alt + ':RANG:AUTO?')
            want_nan(conf, alt + ':RANG?')
    q('CONF:TEMP')
    wait_ms(1500)
    for h in ['VOLT', 'CURR', 'RES']:
        want_nan('CONF:TEMP', h + ':RANG:AUTO?')
    r, e = qerr('TEMP:RANG?')
    check(num(r) is None and 'Undefined header' in e, 'CONF:TEMP: TEMP:RANG? -> %r (%s), want no number and "Undefined header"' % (r, e))
    # The power meter: the V / I channel's (POW:VOLT:RANG? / POW:CURR:RANG?).
    q('CONF:POW')
    wait_ms(2500)
    want_ok('CONF:POW', 'VOLT:RANG:AUTO?', is_bool('1'), '(want 1)')
    want_ok('CONF:POW', 'CURR:RANG:AUTO?', is_bool('1'), '(want 1)')
    pv = q('POW:VOLT:RANG?')
    want_ok('CONF:POW', 'VOLT:RANG?', lambda r: num(r) is not None and abs(num(r) - float(pv.split(',')[1])) < 1e-6, '(POW:VOLT:RANG? %s)' % pv)
    pi = q('POW:CURR:RANG?')
    want_ok('CONF:POW', 'CURR:RANG?', lambda r: num(r) is not None and abs(num(r) - float(pi.split(',')[1])) < 1e-9, '(POW:CURR:RANG? %s)' % pi)
    q('POW:VOLT:RANG 60')
    wait_ms(500)
    want_ok('CONF:POW', 'VOLT:RANG:AUTO?', is_bool('0'), '(want 0 after POW:VOLT:RANG 60)')
    want_ok('CONF:POW', 'VOLT:RANG?', lambda r: num(r) == 60.0, '(want 60)')
    want_ok('CONF:POW', 'CURR:RANG:AUTO?', is_bool('1'), '(want 1)')
    want_nan('CONF:POW', 'VOLT:RES?')
    want_nan('CONF:POW', 'VOLT:AC:RANG:AUTO?')
    r, e = qerr('VOLT:RANG 6')
    check('Settings conflict' in e, 'CONF:POW: VOLT:RANG 6 refused (%s)' % e)
    q('POW:VOLT:RANG AUTO')
    if A.queries_only:
        raise SystemExit(0)

    q('CONF:VOLT:DC')
    wait_ms(4000)
    v0 = float(q('READ?'))
    check(abs(v0 - 1.0) < 0.002, 'DC V reads 1 V on entry (%.5f)' % v0)

    pres = [600, 1000, 60]
    lows = {p: 0 for p in pres}
    runs = {p: 0 for p in pres}
    for k in range(A.trials):
        pre = pres[k % len(pres)]
        q('VOLT:RANG %d' % pre)
        # a varied wait: the phase of the manual range's 400 ms conversions
        # at the AUTO ON command moves from trial to trial
        wait_ms(2500 + 37 * k)
        q('CALC:AVER:CLE')
        t0 = up()
        q('VOLT:RANG:AUTO ON')
        vals = []
        while True:
            r = q('READ?')
            now = up() - t0
            vals.append((now, float(r)))
            if now > 2500:
                break
        mn = q('CALC:AVER:MIN?')
        rng = q('VOLT:RANG?')
        nums = [v for _, v in vals if v < NAN]
        # the first number answered is the 600 / 1000 V conversion that made
        # the first step: 1 V is 0.1-0.2 % of that range, its noise a few %
        nums_i = [i for i, (_, v) in enumerate(vals) if v < NAN]
        bad = [(tt, v) for i, (tt, v) in enumerate(vals)
               if v < NAN and abs(v - 1.0) > (0.15 if nums_i and i == nums_i[0] and pre > 60 else 0.05)]
        runs[pre] += 1
        if bad:
            lows[pre] += 1
        print('  trial %2d from %4d V: %s  MIN %s  range %s' % (k, pre, ' '.join(
            '%d:%s' % (tt, 'nan' if v >= NAN else '%.4f' % v) for tt, v in vals[:8]), mn, rng), flush=True)
        check(not bad, 'from %d V: every READ? 1 V within 5 %% (the first 15 %%; bad %s)' % (pre, bad[:3]))
        check(len(nums) > 0, 'from %d V: a reading within 2.5 s' % pre)
        try:
            check(float(mn) > 0.85, 'from %d V: CALC:AVER:MIN? %s > 0.85' % (pre, mn))
        except ValueError:
            check(False, 'from %d V: CALC:AVER:MIN? answers (%s)' % (pre, mn))
        check(rng.startswith('+6.0'), 'from %d V: ends on 6 V (%s)' % (pre, rng))
    print('  low readings per start range: %s of %s' % (lows, runs))

    # READ? across an AC+DC range change: +9.91E37 until the first reading
    # of the new range (the AC part settles for seconds), never the reading
    # from before the change. A number while CALC:AVER:COUNt? (cleared at
    # the change) is still 0 is the old reading.
    q('CONF:VOLT:ACDC')
    wait_ms(6000)
    for k, rg in enumerate([60, 6, 60]):
        q('VOLT:ACDC:RANG %d' % rg)
        q('CALC:AVER:CLE')
        t0 = up()
        seen = []
        while up() - t0 < 8000:
            v = float(q('READ?'))
            c = int(float(q('CALC:AVER:COUNt?')))
            seen.append((up() - t0, v, c))
            if v < NAN and c > 0:
                break
        stale = [(tt, v) for tt, v, c in seen if v < NAN and c == 0]
        first = [(tt, v) for tt, v, c in seen if v < NAN and c > 0][:1]
        print('  AC+DC -> %d V: %s' % (rg, ' '.join('%d:%s/%d' % (tt, 'nan' if v >= NAN else '%.4f' % v, c) for tt, v, c in seen)), flush=True)
        check(not stale, 'AC+DC -> %d V: no READ? with the pre-change reading (%s)' % (rg, stale[:2]))
        check(bool(first) and abs(first[0][1] - 1.0) < 0.002, 'AC+DC -> %d V: first fresh reading 1 V within 0.2 %% (%s)' % (rg, first))
        check(bool(first) and first[0][0] <= 6500, 'AC+DC -> %d V: first fresh reading within 6.5 s (%s)' % (rg, first))
        q('SYST:ERR?')
except SystemExit as e:
    if e.code not in (0, None):
        raise
finally:
    os.killpg(emu.pid, 15)
    emu.wait()

print('autorangetest: %s' % ('PASS' if not FAIL else 'FAIL: ' + '; '.join(FAIL)))
sys.exit(1 if FAIL else 0)
