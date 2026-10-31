#!/usr/bin/env python3
"""opendm40 DMM Math (firmware/src/math.rs) in the emulator: opendm40 native
+ the HY3131 model, a DC voltage source on V-COM, SCPI over USB/IP. Each
phase sets the math function and checks READ? (the result), CALC:FUNC? and
DBG:READout? (the readout's unit): dBm into 600 ohm at 2.24 V (9.22 dBm),
dBV at 1 V (0.00 dBV), mX+b with m = 1000, b = -100, unit "A" at 0.5 V
(400 A), then NONE (0.5 V again). Screenshots of the DMM page in the dBm
and mX+b phases.

  emu/hy3131/mathtest.py                       # opendm40 built from HEAD
  emu/hy3131/mathtest.py --app firmware/opendm40.bin --port 3391

Results: emu/out/math.json, emu/out/math-<phase>.png. One emulator at a
time per port.
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
ap.add_argument('--port', type=int, default=3391, help='USB/IP port (default 3391)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402
import emusync  # noqa: E402

# (t, source, SCPI at the start, READ? expected, tolerance, CALC:FUNC?,
# the readout's unit (DBG:READ? field 2), screenshot name).
PHASES = [
    (0, 'open;vsrc:2.24', ['CONF:VOLT:DC', 'CALC:DBM:REF 600', 'CALC:FUNC DBM'], 9.2246, 0.01, 'DBM', 'dBm', 'dbm'),
    (14, 'vsrc:1', ['CALC:FUNC DBV'], 0.0, 0.01, 'DBV', 'dBV', None),
    (26, 'vsrc:0.5', ['CALC:SCAL:GAIN 1000', 'CALC:SCAL:OFFS -100', 'CALC:SCAL:UNIT "A"', 'CALC:FUNC SCAL'], 400.0, 0.5, 'SCAL', 'A', 'scal'),
    (40, 'vsrc:0.5', ['CALC:FUNC NONE'], 0.5, 0.0005, 'NONE', 'V', None),
]
RUN_FOR = 52
SETTLE = 5.0


def main():
    os.makedirs(OUT, exist_ok=True)
    sp = os.path.join(OUT, 'math.%d.script' % os.getpid())
    shots = []
    with open(sp, 'w') as f:
        for i, ph in enumerate(PHASES):
            f.write('%g dmm %s\n' % (ph[0], ph[1]))
            if ph[7]:
                t1 = PHASES[i + 1][0] if i + 1 < len(PHASES) else RUN_FOR
                shots += ['--shot', os.path.join(OUT, 'math-%s.png@%g' % (ph[7], t1 - 0.5))]
    name = 'math.%d' % os.getpid()
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--fast', '--usbip', '3',
           '--usbip-port', str(A.port), '--run-for', str(RUN_FOR), '--script', sp, '--name', name] \
        + shots + (['--app', os.path.abspath(A.app)] if A.app else [])
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    mk = emusync.Markers(emusync.log_path(OUT, name))
    tl = emusync.Timeline([(ph[0], ph[1]) for ph in PHASES], RUN_FOR, mk)
    rows = []
    t_wall = time.time()
    tm = None
    while p.poll() is None and tm is None and time.time() - t_wall < 300:
        try:
            tm = uc.Usbtmc(uc.Dev('1-0'))
        except Exception as e:
            print('attach:', type(e).__name__, e, flush=True)
            time.sleep(1)
    done = set()
    if tm is not None and mk.wait_clock() is None:
        print('no clock marker (dwt: CYCCNT write) in the log', flush=True)
    if tm is not None:
        try:
            while p.poll() is None:
                row = {}
                for q in ['DBG:PING?', 'READ?', 'CALC:FUNC?', 'DBG:READ?', 'CALC:SCAL:UNIT?']:
                    tm.write(q)
                    row[q] = tm.read(timeout=60).decode(errors='replace').strip()
                row['t'] = int(row['DBG:PING?'].split(',')[0]) / 1000.0
                rows.append(row)
                print('%6.2f READ %-16s FUNC %-5s readout %s unit %s' % (
                    row['t'], row['READ?'], row['CALC:FUNC?'], row['DBG:READ?'], row['CALC:SCAL:UNIT?']), flush=True)
                for i, ph in enumerate(PHASES):
                    b = tl.start(i, predict=True)
                    if i not in done and b is not None and row['t'] >= b + 0.5:
                        done.add(i)
                        for c in ph[2]:
                            tm.write(c)
        except Exception as e:
            print('link ended:', e, flush=True)
    out, _ = p.communicate()
    bad = emusync.check_sync(mk, tl, 'math')
    ok = not bad
    for i, (t, _, _, want, tol, func, unit, _) in enumerate(PHASES):
        if tl.start(i) is None or tl.stop(i) is None:
            continue  # a marker is missing: check_sync failed the run
        t0 = tl.start(i) + SETTLE
        t1 = tl.stop(i) - 1.0
        seen = [r for r in rows if t0 <= r['t'] <= t1]
        if not seen:
            ok = False
            bad.append('no samples in %g..%g' % (t0, t1))
        for r in seen:
            try:
                rd = float(r['READ?'])
            except ValueError:
                ok = False
                bad.append('%.2f unparsable READ? %s' % (r['t'], r['READ?']))
                continue
            if abs(rd - want) > tol:
                ok = False
                bad.append('%.2f READ? %g, want %g +- %g' % (r['t'], rd, want, tol))
            if r['CALC:FUNC?'] != func:
                ok = False
                bad.append('%.2f CALC:FUNC? %s, want %s' % (r['t'], r['CALC:FUNC?'], func))
            f = r['DBG:READ?'].split(',')
            if len(f) < 2 or f[1] != unit:
                ok = False
                bad.append('%.2f DBG:READ? %s, want unit %s' % (r['t'], r['DBG:READ?'], unit))
    res = {'ok': ok, 'bad': bad, 'rows': rows, 'rc': p.returncode, 'log': out[-800:] if p.returncode else '',
           'sync': emusync.summary(mk, tl)}
    with open(os.path.join(OUT, 'math.json'), 'w') as f:
        json.dump(res, f, indent=1)
    try:
        os.remove(sp)
    except OSError:
        pass
    print('RESULT math %s %s' % ('OK' if ok else 'FAIL', '; '.join(bad[:6])), flush=True)
    return ok


if __name__ == '__main__':
    sys.exit(0 if main() else 1)
