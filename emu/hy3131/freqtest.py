#!/usr/bin/env python3
"""fw-rs DMM frequency: the first readings after an entry (device check
2026-10-24, fw-rs/docs/device-check-2026-10-21.md "Crosstalk and cold-junction
self-heating (f0d21eb)": on a steady 40 Hz square the first 1-2 READ? after
DC V -> CONF:FREQ read 19.94 / 0.00 / 0.99 / 0.94 / 17.04 Hz, then 40.0018).

The cause is a stale counter gate: the Hz row (R20 = 0xF2) re-enables the
HY3131 counter without a CTA preset (CTA<23:8> keeps the last gate's final
count, ~0: a 3.4 s gate; after a rail wake the POR leaves 0 too), and the
first poll after the settle read whatever that gate had counted so far.
The HY3131 model holds CTA<23:8> across ENCTR = 0 (datasheet 14, CtRetain),
returns the counts so far for a read during an open gate, and with
"ctsettle:MS" (HYPOTHETICAL shape) misses every other input edge for MS
after a row change, as the device's half / third first results suggest.

  emu/hy3131/freqtest.py --port 55100                       # all scenarios, fw-rs built from HEAD
  emu/hy3131/freqtest.py --app fw-rs/dm40-fw-native.bin --port 55100 entry wake

Scenarios (each its own emulator; input: a 40 Hz 1 V square on V-COM):
  entry        DC V (3 s) -> CONF:FREQ, READ? x4; REPS times
  entry-settle the same with ctsettle:400
  wake         in Hz: MENU -> Oscilloscope (the DMM rail off) -> MENU ->
               Multimeter (rail wake, chip POR, the row rewritten; checked with
               SYST:RAIL:STAT? wakes), READ? x4;
               with ctsettle:400; REPS times
  acv          AC V (3 s) -> CONF:FREQ (0xF2 -> 0xF2: the counter is not
               re-enabled at all), READ? x4; with ctsettle:400 (passes on 74aff99
               too: the AC V gate is clean)
Every READ? must be 40 Hz within TOL (default 0.05 %). The first READ?'s fw
uptime from the CONF (or the return to the DMM page) is printed. Results:
emu/out/freq-<name>.json. Exit status 0 = passed. 74aff99 fails entry, entry-settle
and wake (first READ? 34-45 / 19.9-30.2 / 18.1-19.6 Hz).
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
ap.add_argument('names', nargs='*')
ap.add_argument('--app', help='fw-rs native image (default: run.py builds HEAD)')
ap.add_argument('--port', type=int, default=55100, help='USB/IP port (default 55100)')
ap.add_argument('--reps', type=int, default=4, help='entries per scenario (default 4)')
ap.add_argument('--tol', type=float, default=0.0005, help='relative tolerance (default 0.0005)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

F0 = 40.0
SIG = 'hz:40,amp=1'
# name: (connection spec, kind, the function before)
SC = {
    'entry': (SIG, 'conf', 'CONF:VOLT:DC'),
    'entry-settle': (SIG + ';ctsettle:400', 'conf', 'CONF:VOLT:DC'),
    'wake': (SIG + ';ctsettle:400', 'wake', None),
    'acv': (SIG + ';ctsettle:400', 'conf', 'CONF:VOLT:AC'),
}


def run(name):
    spec, kind, before = SC[name]
    if uc.port_busy(A.port):
        print('RESULT %s FAIL port %d in use (another emulator?): pass a free --port' % (name, A.port), flush=True)
        return False
    tag = 'freq-' + name
    rtag = '%s-%d-%d' % (tag, A.port, os.getpid())
    os.makedirs(OUT, exist_ok=True)
    sp = os.path.join(OUT, rtag + '.script')
    with open(sp, 'w') as f:
        f.write('0 dmm %s\n' % spec)
    run_for = 40 + A.reps * 14
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--fast', '--usbip', '3',
           '--usbip-port', str(A.port), '--run-for', str(int(run_for)), '--script', sp, '--name', rtag] \
        + (['--app', os.path.abspath(A.app)] if A.app else [])
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    t_wall = time.time()
    tm = None
    while p.poll() is None and tm is None and time.time() - t_wall < 300:
        try:
            tm = uc.Usbtmc(uc.Dev('1-0'))
        except Exception as e:
            print('attach:', type(e).__name__, e, flush=True)
            time.sleep(1)
    bad, reps = [], []

    def q(line):
        tm.write(line)
        return tm.read(timeout=60).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000.0

    def wait_up(t):
        while up() < t and p.poll() is None:
            time.sleep(0.2)

    if tm is not None:
        try:
            q('*IDN?')
            wait_up(6.0)
            if kind == 'wake':
                tm.write('CONF:FREQ')
                wait_up(up() + 3.0)
            for k in range(A.reps):
                if kind == 'conf':
                    tm.write(before)
                    wait_up(up() + 3.0)
                    ts = up()
                    tm.write('CONF:FREQ')
                else:
                    w0 = int(q('SYST:RAIL:STAT?').split(',')[2])
                    tm.write('DBG:KEY 0')        # MENU
                    wait_up(up() + 0.6)
                    tm.write('DBG:TAP 341,107')  # Oscilloscope: the DMM rail goes off
                    wait_up(up() + 2.5)
                    off = q('SYST:RAIL:STAT?')
                    tm.write('DBG:KEY 0')
                    wait_up(up() + 0.6)
                    ts = up()
                    tm.write('DBG:TAP 141,105')  # Multimeter: the rail wake
                vals = []
                for i in range(4):
                    v = q('READ?')
                    t = up()
                    try:
                        fv = float(v)
                    except ValueError:
                        fv = None
                    vals.append((fv, round(t - ts, 3)))
                if kind == 'wake':
                    w1 = int(q('SYST:RAIL:STAT?').split(',')[2])
                    if w1 <= w0 or not off.startswith('0,'):
                        bad.append('rep %d: no rail wake (Scope page rails %s, wakes %d -> %d)' % (k, off, w0, w1))
                wrong = [x for x in vals if x[0] is None or abs(x[0] - F0) > A.tol * F0]
                reps.append({'k': k, 'ts': ts, 'vals': vals, 'rail': q('SYST:RAIL:STAT?')})
                print('%s %d first %s at +%.2f s; then %s%s' % (
                    name, k, vals[0][0], vals[0][1], ' '.join('%s@+%.2f' % x for x in vals[1:]),
                    '  BAD' if wrong else ''), flush=True)
                if wrong:
                    bad.append('rep %d: %s' % (k, ' '.join('%s@+%.2f' % x for x in wrong)))
        except Exception as e:
            print(name, 'link ended:', type(e).__name__, e, flush=True)
            bad.append('link ended: %s' % e)
    p.terminate()
    try:
        out, _ = p.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        p.kill()
        out, _ = p.communicate()
    if not reps:
        print(out[-3000:], flush=True)
    for x in (sp, os.path.join(OUT, 'flash-%s.bin' % rtag), os.path.join(OUT, rtag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
    ok = not bad and len(reps) == A.reps
    first = [r['vals'][0][1] for r in reps]
    with open(os.path.join(OUT, tag + '.json'), 'w') as f:
        json.dump({'name': name, 'ok': ok, 'bad': bad, 'reps': reps}, f, indent=1)
    print('RESULT %s %s first-reading +%s s %s' % (name, 'OK' if ok else 'FAIL', '/'.join('%.2f' % t for t in first),
                                                  '; '.join(bad[:6])), flush=True)
    return ok


if __name__ == '__main__':
    allok = True
    for n in A.names or list(SC):
        allok &= run(n)
    sys.exit(0 if allok else 1)
