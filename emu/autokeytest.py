#!/usr/bin/env python3
"""opendm40 AUTO key in the DMM (firmware/src/app.rs auto_key_end): a short
press in autorange fixes the present range, each further one steps to the
next higher manual range (the MF page's order, 60 mV included; ohm
from 600 ohm, no 60 ohm range),
the highest wrapping to the lowest; held 1 s it goes back to the default
autorange (V: AUTO+). HOLD is released, AutoHold is kept.

  cd firmware && ./build.sh
  emu/autokeytest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

Keys through DBG:KEY 2 (a click) and DBG:KEY 2,1200 (held 1.2 s); ranges
through [SENS:]VOLT|RES:RANG? and RANG:AUTO?. Input: 1 V DC (run.py --vin),
then ohm with the default open input. Exit status 0 = PASS.
"""
import argparse
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
OUT = os.path.join(HERE, 'out')
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--port', type=int, default=None, help='USB/IP port (default: a free one)')
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
A.port = A.port or run.free_port()
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


os.makedirs(OUT, exist_ok=True)
name = 'autokey-%d-%d' % (A.port, os.getpid())
log = open(os.path.join(OUT, name + '.out'), 'w')
emu = subprocess.Popen([sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01',
                        '--usbip-port', str(A.port), '--uid', run.emu_uid(A.port).hex().upper(), '--name', name,
                        '--vin', '1.0@0', '--run-for', '3000'],
                       cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
try:
    for _ in range(150):
        try:
            d = uc.Dev('1-0')
            break
        except Exception:
            time.sleep(2)
    else:
        raise RuntimeError('no USB/IP device on port %d' % A.port)
    t = uc.Usbtmc(d)

    def q(s):
        t.write(s)
        return t.read(timeout=30).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0])

    def wait(ms):
        t0 = up()
        while up() < t0 + ms:
            time.sleep(0.05)

    def rng(f):
        return float(q('%s:RANG?' % f)), q('%s:RANG:AUTO?' % f)

    def key(hold=0):
        q('DBG:KEY 2,%d;*OPC?' % hold) if hold else q('DBG:KEY 2;*OPC?')
        wait(hold + 600)

    q('*RST;*OPC?')
    q('CONF:VOLT:DC;*OPC?')
    wait(2500)
    r0 = rng('VOLT')
    print('DC V, 1 V in: range %g, auto %s' % r0, flush=True)
    check(r0[1] == '1', 'starts in autorange')
    q('CALC:AHOL ON;*OPC?')
    key()
    r = rng('VOLT')
    check(r == (r0[0], '0'), 'short press in autorange: the present range fixed (%g, auto %s)' % r)
    seen = [r[0]]
    for _ in range(6):
        key()
        seen.append(rng('VOLT')[0])
    print('  ranges: %s' % ' '.join('%g' % v for v in seen), flush=True)
    want = [6, 60, 600, 1000, 0.06, 0.6, 6]
    check(seen == want, 'each press the next higher range, wrapping (%s)' % want)
    check(q('CALC:AHOL?') == '1', 'AutoHold kept')
    # kind,unit,value,hold,range (scpi.rs DBG:READout?): a fixed range = MANU.
    rd = q('DBG:READout?').split(',')
    check(len(rd) == 5 and rd[4] == 'MANU', 'DBG:READout? fields: %s' % ','.join(rd))
    key(1200)
    r = rng('VOLT')
    check(r[1] == '1', 'held 1.2 s: autorange (%g, auto %s)' % r)
    check(q('VOLT:RANG:AUTO? ;DBG:VIEW?').startswith('1'), 'still autorange after the release')
    check(q('CALC:AHOL?') == '1', 'AutoHold kept after the long press')
    q('CALC:AHOL OFF;*OPC?')
    # HOLD released by a short press.
    q('DBG:KEY 1;*OPC?')
    wait(300)
    h1 = q('DBG:READout?').split(',')[3]
    key()
    h2 = q('DBG:READout?').split(',')[3]
    check(h1 == '1' and h2 == '0', 'HOLD released by AUTO (hold state %s -> %s)' % (h1, h2))
    # Ohm: the cycle wraps to 600 ohm, no 60 ohm (open input: autorange sits
    # on 60 Mohm).
    q('CONF:RES;*OPC?')
    wait(3000)
    key()
    seen = [rng('RES')[0]]
    for _ in range(2):
        key()
        seen.append(rng('RES')[0])
    print('  ohm ranges: %s' % ' '.join('%g' % v for v in seen), flush=True)
    check(seen[1] == 600 and seen[2] == 6000, 'ohm: the highest wraps to 600 ohm, then 6 kohm')
except Exception as e:
    check(False, 'run: %r' % e)
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    emu.wait()
print('FAIL: %d checks' % len(FAIL) if FAIL else 'PASS')
sys.exit(1 if FAIL else 0)
