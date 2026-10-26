#!/usr/bin/env python3
"""Ohm readings below 0 (Settings 3/3 "Negative ohm", SCPI RES:NEG; dmm.rs
`poll_ohm` / `ohm_neg`) in the emulator: opendm40 native + the HY3131 model,
shorted leads whose node sits 10 uV below the lead offset the model applies
(NEG: `vsrc:-1e-5,rs=0.001`, as a thermal EMF; the model's plain short reads
+0.000003 ohm), RES AUTO, SCPI over USB/IP.

Phases (each polled for HOLD s after its command):
  off   RES:NEG? 0 (the default), READ? never below 0;
  on    RES:NEG ON: RES:NEG? 1, READ? -1 .. <0 ohm, not OL (+9.9E37), and
        RES:RANG? stays 60 ohm (a value below 0 must not step the
        autorange up);
  r10   r:10 with RES:NEG still ON: READ? 10 ohm within 1 %;
  back  RES:NEG OFF on NEG again: READ? never below 0.
Exit status 0 = PASS. Results: emu/out/ohmneg.json.

  emu/hy3131/ohmnegtest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 --port 64751
"""
import argparse
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
OUT = os.path.join(EMU, 'out')
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', help='opendm40 native image (default: run.py builds HEAD)')
ap.add_argument('--port', type=int, default=64751, help='USB/IP port (default 64751)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

OL = 9.9e37
HOLD = 6.0
RUN_FOR = 80
NEG = 'open;vsrc:-1e-5,rs=0.001;noise:0'
# Emulator inputs (virtual s): NEG from the start, 10 ohm at R10_AT, NEG
# again at SHORT_AT. The off / on phases run from the attach on
# (before R10_AT); r10 / back wait for their input to show.
R10_AT, SHORT_AT = 40, 56
# name, SCPI, wait-for(reading) or None, check(reading, RES:NEG?, RES:RANG?) -> error or None
PHASES = [
    ('off', ['CONF:RES'], lambda r: r < 1, lambda r, n, g: None if n == '0' and 0 <= r < 1 else 'want 0 .. 1 ohm, NEG 0'),
    ('on', ['RES:NEG ON'], None,
     lambda r, n, g: None if n == '1' and -1 < r < 0 and abs(g - 60) < 1e-3 else 'want -1 .. <0 ohm on 60 ohm, NEG 1'),
    ('r10', [], lambda r: abs(r - 10) < 0.1, lambda r, n, g: None if n == '1' and abs(r - 10) < 0.1 else 'want 10 ohm'),
    ('back', ['RES:NEG OFF'], lambda r: r < 1, lambda r, n, g: None if n == '0' and 0 <= r < 1 else 'want 0 .. 1 ohm, NEG 0'),
]


def main():
    os.makedirs(OUT, exist_ok=True)
    name = 'ohmneg.%d' % os.getpid()
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--fast', '--usbip', '3',
           '--usbip-port', str(A.port), '--run-for', str(RUN_FOR), '--dmm', NEG, '--dmm', 'open;r:10;noise:0@%d' % R10_AT,
           '--dmm', '%s@%d' % (NEG, SHORT_AT), '--name', name] \
        + (['--app', os.path.abspath(A.app)] if A.app else [])
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    tm = None
    t_wall = time.time()
    while p.poll() is None and tm is None and time.time() - t_wall < 300:
        try:
            tm = uc.Usbtmc(uc.Dev('1-0'))
        except Exception as e:
            print('attach:', type(e).__name__, e, flush=True)
            time.sleep(1)
    bad, rows = [], []

    def q(s):
        tm.write(s)
        return tm.read(timeout=60).decode(errors='replace').strip()

    if tm is None:
        bad.append('no USB/IP attach')
    else:
        try:
            for ph, scpi, wait, chk in PHASES:
                for c in scpi:
                    tm.write(c)
                if wait:
                    t1 = time.time()
                    while not wait(float(q('READ?'))):
                        if time.time() - t1 > 120:
                            raise RuntimeError('%s: its input never showed' % ph)
                t0 = int(q('DBG:PING?').split(',')[0]) / 1000.0
                seen = 0
                while True:
                    t = int(q('DBG:PING?').split(',')[0]) / 1000.0
                    if t - t0 > HOLD:
                        break
                    rd, neg, rng = q('READ?'), q('RES:NEG?'), q('RES:RANG?')
                    row = {'ph': ph, 't': t, 'READ?': rd, 'RES:NEG?': neg, 'RES:RANG?': rng}
                    rows.append(row)
                    print('%-5s %6.2f READ %-16s NEG %s RANG %s' % (ph, t, rd, neg, rng), flush=True)
                    if t - t0 < 2.0:
                        continue  # settle (autorange, the averaging)
                    seen += 1
                    e = chk(float(rd), neg, float(rng))
                    if e:
                        bad.append('%s %.2f READ? %s NEG %s RANG %s: %s' % (ph, t, rd, neg, rng, e))
                if not seen:
                    bad.append('%s: no samples' % ph)
        except Exception as e:
            bad.append('link ended: %s' % e)
    p.kill()
    out, _ = p.communicate()
    ok = not bad
    with open(os.path.join(OUT, 'ohmneg.json'), 'w') as f:
        json.dump({'ok': ok, 'bad': bad, 'rows': rows}, f, indent=1)
    print('RESULT ohmneg %s %s' % ('OK' if ok else 'FAIL', '; '.join(bad[:6])), flush=True)
    return ok


if __name__ == '__main__':
    sys.exit(0 if main() else 1)
