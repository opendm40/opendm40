#!/usr/bin/env python3
"""fw-rs burden voltage readout (fw-rs/docs/power-meter-and-misconnect.md
3.15) in the emulator: fw-rs native + the HY3131 model, a current source in
the A jack through the DC / AC current functions, SCPI over USB/IP. Each
phase checks FETC:BURD? against READ? x the R_path of the shunt in use
(default the bare shunt: 10 mohm / 0.5 ohm / 47.5 ohm; POW:BURD sets it),
DISP:BURD on / off, and +9.9E37 outside the A functions. Screenshots of the
DMM page with the readout at the end of the DC / AC phases.

  emu/hy3131/burdentest.py                       # fw-rs built from HEAD
  emu/hy3131/burdentest.py --app fw-rs/dm40-fw-native.bin --port 3378

Results: emu/out/burden.json, emu/out/burden-<phase>.png. One emulator at a
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
ap.add_argument('--app', help='fw-rs native image (default: run.py builds HEAD)')
ap.add_argument('--port', type=int, default=3378, help='USB/IP port (default 3378)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

OL = 9.9e37
# (t, connection, SCPI at the start, expected FETC:BURD? / READ? ratio (ohm)
# or 'OL' = +9.9E37, expected DISP:BURD?, screenshot name). A phase is
# checked from t + 5 s (settle, autorange) to the next t - 1 s.
PHASES = [
    (0, 'open;isrc:0.05', ['CONF:CURR:DC', 'DISP:BURD ON'], 0.5, '1', 'dc-ma'),
    # AUTO moves -2 mA to the uA shunt (6000 uA range).
    (14, 'isrc:-0.002', [], 47.5, '1', 'dc-ua'),
    (26, 'isrc:3', [], 0.01, '1', 'dc-a'),
    (40, 'isrc:0,ac=0.1,f=50', ['CONF:CURR:AC'], 0.5, '1', 'ac-ma'),
    (54, 'isrc:0,ac=0.1,f=50', ['POW:BURD 0.6'], 0.6, '1', None),
    # The readout off: FETC:BURD? still answers.
    (64, 'isrc:0,ac=0.1,f=50', ['POW:BURD DEF', 'DISP:BURD OFF'], 0.5, '0', 'ac-off'),
    (74, 'open;vsrc:1', ['CONF:VOLT:DC'], 'OL', '0', None),
]
RUN_FOR = 86


def main():
    os.makedirs(OUT, exist_ok=True)
    sp = os.path.join(OUT, 'burden.%d.script' % os.getpid())
    shots = []
    with open(sp, 'w') as f:
        for i, ph in enumerate(PHASES):
            f.write('%g dmm %s\n' % (ph[0], ph[1]))
            if ph[5]:
                t1 = PHASES[i + 1][0] if i + 1 < len(PHASES) else RUN_FOR
                shots += ['--shot', os.path.join(OUT, 'burden-%s.png@%g' % (ph[5], t1 - 0.5))]
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--fast', '--usbip', '3',
           '--usbip-port', str(A.port), '--run-for', str(RUN_FOR), '--script', sp, '--name', 'burden.%d' % os.getpid()] \
        + shots + (['--app', os.path.abspath(A.app)] if A.app else [])
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
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
    if tm is not None:
        try:
            while p.poll() is None:
                row = {}
                for q in ['DBG:PING?', 'READ?', 'FETC:BURD?', 'DISP:BURD?', 'FUNC?', 'POW:BURD?']:
                    tm.write(q)
                    row[q] = tm.read(timeout=60).decode(errors='replace').strip()
                row['t'] = int(row['DBG:PING?'].split(',')[0]) / 1000.0
                rows.append(row)
                print('%6.2f %-8s READ %-16s BURD %-16s disp %s POW:BURD %s' % (
                    row['t'], row['FUNC?'], row['READ?'], row['FETC:BURD?'], row['DISP:BURD?'], row['POW:BURD?']), flush=True)
                for i, ph in enumerate(PHASES):
                    if i not in done and row['t'] >= ph[0] + 0.5:
                        done.add(i)
                        for c in ph[2]:
                            tm.write(c)
        except Exception as e:
            print('link ended:', e, flush=True)
    out, _ = p.communicate()
    ok = True
    bad = []
    for i, (t, _, _, want, disp, _) in enumerate(PHASES):
        t0 = t + 5.0
        t1 = (PHASES[i + 1][0] if i + 1 < len(PHASES) else RUN_FOR) - 1.0
        seen = [r for r in rows if t0 <= r['t'] <= t1]
        if not seen:
            ok = False
            bad.append('no samples in %g..%g' % (t0, t1))
        for r in seen:
            try:
                rd, bd = float(r['READ?']), float(r['FETC:BURD?'])
            except ValueError:
                ok = False
                bad.append('%.2f unparsable %s / %s' % (r['t'], r['READ?'], r['FETC:BURD?']))
                continue
            if r['DISP:BURD?'] != disp:
                ok = False
                bad.append('%.2f DISP:BURD? %s, want %s' % (r['t'], r['DISP:BURD?'], disp))
            if want == 'OL':
                if bd != OL:
                    ok = False
                    bad.append('%.2f FETC:BURD? %s, want +9.9E37' % (r['t'], r['FETC:BURD?']))
                continue
            # The same reading: FETC:BURD? is the latest raw reading, READ? the
            # next one; a steady source gives the same value within 0.2 %.
            if rd == OL or abs(bd - rd * want) > 0.002 * abs(rd * want) + 1e-6:
                ok = False
                bad.append('%.2f FETC:BURD? %g != READ? %g x %g' % (r['t'], bd, rd, want))
    res = {'ok': ok, 'bad': bad, 'rows': rows, 'rc': p.returncode, 'log': out[-800:] if p.returncode else ''}
    with open(os.path.join(OUT, 'burden.json'), 'w') as f:
        json.dump(res, f, indent=1)
    try:
        os.remove(sp)
    except OSError:
        pass
    print('RESULT burden %s %s' % ('OK' if ok else 'FAIL', '; '.join(bad[:6])), flush=True)
    return ok


if __name__ == '__main__':
    sys.exit(0 if main() else 1)
