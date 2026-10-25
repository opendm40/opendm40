#!/usr/bin/env python3
"""AD1 stream logger: no conversion recorded twice (device check of
ea73ea5, AD1 OSR characterization: right after a 640/s session an OSR 8192
chopped session held each conversion twice, identical codes ~52 ms apart,
121 of 294 records; docs/history/device-check-2026-10-21.md). The main-loop
sampler (firmware/src/logger.rs `sample`) took the session's first interval
- from a flag raised before the start to the first conversion, half a
period after the restart with the chopper on (the model's opt-in
`chophalf`) - as its period, rescued a "lost" flag before every later
conversion and never measured the period again.

Sessions (LOG:DEST HOST, DC V 6 V manual, 1 V with the model's noise), each
right after a 640/s one (LOG:OSR 0, chop OFF, 1.5 s): OSR 8192 chopped
(LOG:OSR 5, ~10/s) and OSR 16384 chopped (LOG:OSR 6, ~5/s), 6 s each. A pair
of records with the same code less than 0.75 period apart is a copy; at
most 1 per session passes (a genuine repeat of the code).

  emu/hy3131/logduptest.py --app firmware/opendm40.bin --port 47710
"""
import argparse
import os
import signal
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
OUT = os.path.join(EMU, 'out')
sys.path.insert(0, os.path.join(ROOT, 'firmware', 'tools'))
import dm40scpi  # noqa: E402

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native image')
ap.add_argument('--port', type=int, default=47710, help='USB/IP port')
ap.add_argument('--skew', type=float, default=22.4, help='model chopskew, ms (0 = the ea73ea5 case alone)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

# (LOG:OSR, chop, nominal period s)
SESS = [(5, 'ON', 0.1008, 0), (6, 'ON', 0.2008, 0), (5, 'ON', 0.1008, 0)]
os.makedirs(OUT, exist_ok=True)
tag = 'logdup-%d-%d' % (A.port, os.getpid())
script = os.path.join(OUT, tag + '.script')
with open(script, 'w') as f:
    f.write('0 dmm dcv:1.0;chophalf:1;chopskew:%g\n' % A.skew)
end = 120.0
cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01', '--usbip-port', str(A.port),
       '--run-for', '%.1f' % end, '--name', tag, '--script', script]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
bad = []
try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('logduptest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60):
        t.write(line)
        return t.read(maxlen=1 << 16, timeout=timeout)

    def qs(line):
        return q(line).decode(errors='replace').strip()

    def up():
        return int(qs('DBG:PING?').split(',')[0]) / 1000.0

    def wait_dev(s):
        t0 = up()
        while up() < t0 + s:
            time.sleep(0.05)

    def session(osr, chop, dur):
        t.write('LOG:CHOP %s;:LOG:OSR %d;:LOG:STAT ON' % (chop, osr))
        recs = []
        t0 = up()
        while up() < t0 + dur:
            recs += dm40scpi.log_records(q('LOG:DATA?'))
        t.write('LOG:STAT OFF')
        recs += dm40scpi.log_records(q('LOG:DATA?'))
        return recs

    wait_dev(3.0)
    for c in ['CONF:VOLT:DC 6', 'LOG:DEST HOST', 'LOG:SOUR AD1', 'LOG:DUR 60']:
        t.write(c)
    wait_dev(2.0)
    for osr, chop, per, _ in SESS:
        session(0, 'OFF', 1.5)
        recs = session(osr, chop, 6.0)
        dup = [(a[0], (b[1] - a[1]) / 1e3) for a, b in zip(recs, recs[1:]) if a[2] == b[2] and b[1] - a[1] < 0.75 * per * 1e6]
        ok = len(recs) >= 0.7 * 6.0 / per and len(dup) <= 1
        print('  %s OSR %d chop %s: %d records, %d copies%s, err %s' % (
            'ok ' if ok else 'BAD', osr, chop, len(recs), len(dup),
            (' (seq, ms after the one before: %s)' % dup[:4]) if dup else '', qs('SYST:ERR?')), flush=True)
        if not ok:
            bad.append('OSR %d: %d copies in %d records' % (osr, len(dup), len(recs)))
finally:
    t = None
    if emu.poll() is None:
        os.killpg(emu.pid, signal.SIGTERM)
    emu.wait()
    for x in (script, os.path.join(OUT, 'flash-%s.bin' % tag), os.path.join(OUT, tag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
print('logduptest: %s' % ('PASS' if not bad else 'FAIL ' + '; '.join(bad)))
sys.exit(1 if bad else 0)
