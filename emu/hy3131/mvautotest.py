#!/usr/bin/env python3
"""opendm40 V autorange around the mV ranges (device check 46370df, new bugs
2 and 4): AUTO -> AUTO+ on 60 mV leaves mV at once, and AC V AUTO
(mV-first) leaves 600 mV under a large square that the mV input clamps.

  cd firmware && ./build.sh
  emu/hy3131/mvautotest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 [--port P]

Model options (HY3131.cs): `mvclamp:1.212` clips the jack on the mV path
to +-1.212 V (the device read 0.606 V AC on 600 mV with the 40 Hz 0 / 9.5 V
square: a 0 .. 1.21 V node swing, under the 1.03 x 600 mV up-range
threshold); `mvpb1:10` puts the clamp's current on PB1 (the device's
AJACK,AD1PK_PB1 alert at the same time, A jack empty).
Input timeline (emulator seconds, run.py --dmm; the fw clock runs ~0.9 s
behind): 20 mV DC, 0 V from 45 s, the 40 Hz square 0 / 9.5 V
(vsrc:4.75,ac=4.75,shape=square) from 100 s, a 400 mV 50 Hz sine from
125 s, 0 V from 145 s, a 40 Hz square 0.6 / 1.7 V (vsrc:1.15,ac=0.55) from
165 s, 0 V from 190 s. The range page is driven with DBG:KEY 3 / DBG:TAP.
Checks (VOLT:RANG? / READ? over USBTMC, fw uptime from DBG:PING?):
  * DC, AUTO: 20 mV -> 60 mV; the AUTO+ item -> 6 V within 3 s
    (RANG:AUTO? 1) and no mV for the 5 s after; AUTO again -> 60 mV;
  * AC, AUTO: 0 V -> 60 mV; AUTO+ -> 6 V within 3 s, no mV for 5 s;
    AUTO again -> 60 mV;
  * the square: AC 60 mV -> 6 V, READ? 4.75 V within 3 %, within 12 s, no
    60 V range on the way (device 489f956: 6 V only)
    (whether A-JACK asserted on the way is printed: the range must not
    depend on it);
  * the 400 mV sine right after it: back to 600 mV (0.4 V within 3 %)
    within 20 s (the 6 V hold after a clip ends when the input changes);
    0 V -> 60 mV;
  * the 0.6 / 1.7 V square (clipped at 1.21 V in AC mV: |LPF| + 1.414 RMS
    over 2.2 x 600 mV; 0.55 V on 6 V, under its 0.583 V down threshold):
    6 V within 15 s and no mV range for 12 s after (no hunting);
    0 V -> 60 mV within 20 s.
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
ap.add_argument('--app', required=True, help='opendm40 native ELF')
ap.add_argument('--port', type=int, default=63130, help='USB/IP port (default 63130)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

FAIL = []
NAN = 9.9e37
SQ_ON, SINE, ZERO, SQ2, ZERO2 = 100000, 125000, 145000, 165000, 190000
FW = 900   # the fw clock behind the emulator's, ms


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


DMM = ['vsrc:0.02;mvclamp:1.212;mvpb1:10', 'vsrc:0@45',
       'vsrc:4.75,ac=4.75,f=40,shape=square@%d' % (SQ_ON // 1000), 'vsrc:0,ac=0.4,f=50@%d' % (SINE // 1000),
       'vsrc:0@%d' % (ZERO // 1000), 'vsrc:1.15,ac=0.55,f=40,shape=square@%d' % (SQ2 // 1000), 'vsrc:0@%d' % (ZERO2 // 1000)]
cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01',
       '--usbip-port', str(A.port), '--run-for', '4000', '--name', 'mvautotest-%d' % A.port]
for d in DMM:
    cmd += ['--dmm', d]
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
        sys.exit('mvautotest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=600):
        t.write(line)
        if '?' not in line:
            return None
        return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0])

    def num(r):
        try:
            return float(r)
        except (TypeError, ValueError):
            return None

    def wait_until_up(ms):
        while up() < ms:
            time.sleep(0.3)

    def rng():
        return num(q('VOLT:RANG?'))

    def settle(want_r, want, tol, until_ms, what):
        """Poll VOLT:RANG? / READ? until the range is `want_r` (and READ?
        `want` within `tol` relative, when given), at most until `until_ms`."""
        last = None
        while up() < until_ms:
            r, v = rng(), num(q('READ?'))
            last = (r, v)
            if r is not None and abs(r - want_r) < want_r * 1e-3 and (want is None or (v is not None and v < NAN and abs(abs(v) - want) <= want * tol)):
                check(True, '%s: range %g, READ? %s (at %.1f s)' % (what, r, v, up() / 1000))
                return True
            time.sleep(0.3)
        check(False, '%s: want range %g%s, last %s' % (what, want_r, '' if want is None else ' and %g within %g %%' % (want, tol * 100), last))
        return False

    def tap_range(x, y):
        q('DBG:KEY 3')          # MF: the Voltage Settings page
        time.sleep(1.5)
        q('DBG:TAP %d,%d' % (x, y))
        time.sleep(0.5)
        q('DBG:KEY 0')          # back to the DMM page
        time.sleep(0.5)

    AUTO = (415, 116)           # ui::range_items(Volt)[7] = "AUTO" (mV-first)
    AUTOP = (298, 116)          # [6] = "AUTO+"

    def auto_plus_leaves_mv(what):
        t0 = up()
        tap_range(*AUTOP)
        check(q('VOLT:RANG:AUTO?') == '1', '%s: RANG:AUTO? 1 after AUTO+' % what)
        if settle(6.0, None, 0, t0 + 3000 + 2500, '%s: AUTO+ on 60 mV -> 6 V' % what):
            mv = []
            t1 = up()
            while up() < t1 + 5000:
                r = rng()
                if r is not None and r < 1:
                    mv.append(r)
                time.sleep(0.3)
            check(not mv, '%s: AUTO+ stays out of mV for 5 s (%s)' % (what, mv[:5]))

    # The fw clock starts ~0.9 s after the emulator's (switchlat.py BOOT_OFS).
    wait_until_up(3000)
    q('CONF:VOLT:DC')
    time.sleep(1)
    tap_range(*AUTO)
    settle(0.06, 0.02, 0.01, 25000, 'DC 20 mV, AUTO: down to 60 mV')
    auto_plus_leaves_mv('DC')
    tap_range(*AUTO)
    settle(0.06, 0.02, 0.01, up() + 15000, 'DC, AUTO again: down to 60 mV')
    wait_until_up(46000)
    q('CONF:VOLT:AC')
    q('VOLT:RANG:AUTO ON')
    settle(0.06, None, 0, 70000, 'AC 0 V, AUTO: down to 60 mV')
    auto_plus_leaves_mv('AC')
    tap_range(*AUTO)
    settle(0.06, None, 0, up() + 20000, 'AC, AUTO again: down to 60 mV')
    if up() > SQ_ON - FW - 3000:
        check(False, 'the AC part ended at %.1f s, after the square came (%.0f s)' % (up() / 1000, SQ_ON / 1000))
    wait_until_up(SQ_ON - FW - 3000)
    check(rng() == 0.06, 'AC on 60 mV before the square: %s' % rng())
    q('DBG:SENT:LOG?')          # drain the alert log
    seen, alerts = [], set()
    t_end = SQ_ON + 12000
    ok = False
    while up() < t_end:
        r, v, a = rng(), num(q('READ?')), q('SYST:ALER?')
        alerts.add(a)
        if not seen or seen[-1][1] != r:
            seen.append((up() / 1000, r))
        if r == 6.0 and v is not None and v < NAN and abs(v - 4.75) <= 4.75 * 0.03:
            ok = True
            break
        time.sleep(0.3)
    check(ok, 'square 0 / 9.5 V: AC AUTO -> 6 V, READ? 4.75 V within 3 %% by %.0f s (ranges %s, last READ? %s)' % (t_end / 1000, seen, v))
    # Device 489f956: AC mV -> AC V under this square goes to 6 V only (the model's coupling
    # capacitor level while OP1 is off, HY3131.cs Op1OffLevel, once made a 6 -> 60 -> 6 V step).
    check(all(r_ != 60.0 for _, r_ in seen), 'square: no 60 V range on the way up (ranges %s)' % seen)
    print('  info: SYST:ALER? seen during the square: %s; DBG:SENT:LOG? %s' % (sorted(alerts), q('DBG:SENT:LOG?')), flush=True)
    wait_until_up(SQ_ON + 15000)
    r = rng()
    check(r == 6.0, 'square: still on 6 V at %.0f s (%s)' % ((SQ_ON + 15000) / 1000, r))
    wait_until_up(SINE - FW)
    settle(0.6, 0.4, 0.03, SINE + 18000, 'square -> 400 mV sine: back to 600 mV')
    wait_until_up(ZERO - FW)
    settle(0.06, None, 0, ZERO + 18000, '0 V: back to 60 mV')
    wait_until_up(SQ2 - FW)
    if settle(6.0, 0.55, 0.03, SQ2 + 15000, 'square 0.6 / 1.7 V: up to 6 V'):
        seen = []
        t1 = up()
        while up() < min(t1 + 12000, ZERO2 - FW - 500):
            r = rng()
            if r is not None and r < 1:
                seen.append((up() / 1000, r))
            time.sleep(0.3)
        check(not seen, 'square 0.6 / 1.7 V: no mV range for %.0f s (%s)' % ((up() - t1) / 1000, seen[:5]))
    wait_until_up(ZERO2 - FW)
    settle(0.06, None, 0, ZERO2 + 20000, '0 V: back to 60 mV')
except SystemExit as e:
    if e.code not in (0, None):
        raise
finally:
    os.killpg(emu.pid, 15)

print('PASS' if not FAIL else 'FAIL (%d): %s' % (len(FAIL), FAIL))
sys.exit(1 if FAIL else 0)
