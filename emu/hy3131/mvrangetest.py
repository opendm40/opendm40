#!/usr/bin/env python3
"""opendm40 60 mV range (the DC / AC mV r0 row, OP1 x10): the mV-first AUTO
steps into it and out of it, the range page and SCPI select it.

  cd firmware && ./build.sh
  emu/hy3131/mvrangetest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 [--port P]

Autorange thresholds (dmm.rs poll_dcv / poll_ac, the stock rule of the
other ranges): 600 mV -> 60 mV below 0.1 x 600 mV / 1.03 = 58.3 mV,
60 mV -> 600 mV above 1.03 x 60 mV = 61.8 mV. Input timeline (emulator
seconds, run.py --dmm): 20 mV DC, 300 mV DC from 40 s, 20 mV DC from 65 s,
20 mV AC 1 kHz from 95 s, 300 mV AC from 125 s, 30 mV DC from 150 s.
The volt autorange is set to AUTO (mV-first; AUTO+, the default, never
enters mV) on the range page (DBG:KEY 3, DBG:TAP on the AUTO item).
Checks (VOLT:RANG? / READ? over USBTMC, fw uptime from DBG:PING?):
  * DC, AUTO: 20 mV -> range 0.06 and READ? 20 mV within 1 %;
    300 mV -> 0.6 (300 mV); 20 mV again -> 0.06;
  * AC, AUTO: 20 mV -> 0.06 (20 mV rms within 3 %); 300 mV -> 0.6;
  * manual: `VOLT:RANG 0.05` (the smallest range that holds 50 mV) ->
    0.06, RANG:AUTO? 0; `VOLT:RANG MIN` -> 0.06; the MF range page's
    first item ("60mV", tapped with DBG:TAP) -> 0.06 manual (from 600 mV).
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
ap.add_argument('--port', type=int, default=63120, help='USB/IP port (default 63120)')
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


DMM = ['vsrc:0.02', 'vsrc:0.3@40', 'vsrc:0.02@65', 'vsrc:0,ac=0.02,f=1000@95', 'vsrc:0,ac=0.3,f=1000@125', 'vsrc:0.03@150']
cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01',
       '--usbip-port', str(A.port), '--run-for', '4000', '--name', 'mvrangetest-%d' % A.port]
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
        sys.exit('mvrangetest: no USB/IP device on port %d' % A.port)

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

    def settle(rng, want, tol, until_ms, what):
        """Poll VOLT:RANG? / READ? until the range is `rng` and READ? is
        `want` within `tol` (relative), at most until fw uptime `until_ms`."""
        last = None
        while up() < until_ms:
            r = num(q('VOLT:RANG?'))
            v = num(q('READ?'))
            last = (r, v)
            if r is not None and abs(r - rng) < rng * 1e-3 and v is not None and v < NAN and abs(abs(v) - want) <= want * tol:
                check(True, '%s: range %g, READ? %.6g (at %.1f s)' % (what, r, v, up() / 1000))
                return
            time.sleep(0.3)
        check(False, '%s: want range %g and %g within %g %%, last %s' % (what, rng, want, tol * 100, last))

    # The fw clock starts ~0.9 s after the emulator's (switchlat.py BOOT_OFS).
    wait_until_up(3000)
    q('CONF:VOLT:DC')
    time.sleep(1)
    q('DBG:KEY 3')          # MF: the Voltage Settings page
    time.sleep(1.5)
    q('DBG:TAP 415,116')    # ui::range_items(Volt)[7] = "AUTO" (mV-first)
    time.sleep(1)
    q('DBG:KEY 0')          # back to the DMM page
    time.sleep(1)
    check(q('VOLT:RANG:AUTO?') == '1', 'DC V in AUTO')
    settle(0.06, 0.02, 0.01, 38000, 'DC 20 mV, AUTO: down to 60 mV')
    wait_until_up(40000)
    settle(0.6, 0.3, 0.01, 63000, 'DC 300 mV: up to 600 mV')
    wait_until_up(65000)
    settle(0.06, 0.02, 0.01, 85000, 'DC 20 mV again: down to 60 mV')
    # Manual: SCPI.
    q('*CLS')
    q('VOLT:RANG 0.05')
    r, a, e = num(q('VOLT:RANG?')), q('VOLT:RANG:AUTO?'), q('SYST:ERR?')
    check(r == 0.06 and a == '0' and e.startswith('0,'), 'VOLT:RANG 0.05 -> range %s, AUTO? %s (%s)' % (r, a, e))
    q('VOLT:RANG 6')
    q('VOLT:RANG MIN')
    r = num(q('VOLT:RANG?'))
    check(r == 0.06, 'VOLT:RANG MIN -> %s' % r)
    q('VOLT:RANG:AUTO ON')
    wait_until_up(95000)
    q('CONF:VOLT:AC')
    q('VOLT:RANG:AUTO ON')
    settle(0.06, 0.02, 0.03, 123000, 'AC 20 mV, AUTO: down to 60 mV')
    wait_until_up(125000)
    settle(0.6, 0.3, 0.03, 148000, 'AC 300 mV: up to 600 mV')
    # The range page: from 600 mV manual, tap the first item ("60mV").
    wait_until_up(150000)
    q('CONF:VOLT:DC')
    q('VOLT:RANG 0.6')
    time.sleep(1)
    q('DBG:KEY 3')          # MF: the Voltage Settings page
    time.sleep(1.5)
    view = q('DBG:VIEW?')
    q('DBG:TAP 64,70')      # range_rect(0): "60mV"
    time.sleep(1.5)
    r, a = num(q('VOLT:RANG?')), q('VOLT:RANG:AUTO?')
    check(r == 0.06 and a == '0', 'range page "60mV" (view %s) -> range %s, AUTO? %s' % (view, r, a))
    settle(0.06, 0.03, 0.01, up() + 15000, 'DC 30 mV on the 60 mV range (manual)')
    q('DBG:KEY 0')
except SystemExit as e:
    if e.code not in (0, None):
        raise
finally:
    os.killpg(emu.pid, 15)

print('PASS' if not FAIL else 'FAIL (%d): %s' % (len(FAIL), FAIL))
sys.exit(1 if FAIL else 0)
