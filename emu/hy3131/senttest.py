#!/usr/bin/env python3
"""fw-rs misconnect sentinel (fw-rs/src/sentinel.rs) in the emulator: fw-rs
native + the HY3131 model, a timed connection script per scenario, SCPI over
USB/IP (CONF:<f>, then DBG:PING? uptime, SYST:ALER?, DBG:SENT?, READ? in a
loop); each phase of the script must show only its expected alerts.

  emu/hy3131/senttest.py                         # all scenarios, fw-rs built from HEAD
  emu/hy3131/senttest.py --app fw-rs/dm40-fw-native.bin --port 3377 dcv-adc acv
  emu/hy3131/senttest.py --list

Results: emu/out/sent-<name>.json (+ .png of the end). One emulator at a
time per port; run several invocations on different ports in parallel.
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
ap.add_argument('--port', type=int, default=3377, help='USB/IP port (default 3377)')
ap.add_argument('--list', action='store_true')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

# name: (SCPI function, run_for, [(t, spec)], [(t0, t1, allowed alerts)])
# name: (SCPI function, run_for, [(t, spec, allowed alerts in this phase)]); a phase is checked
# from its start + 2 s to the next start - 1.5 s (fw uptime lags the virtual time by ~0.5 s;
# A-JACK clears 1 s after the last hit, V-JACK? needs 1 s + the settle).
SC = {
    'dcv-adc':  ('VOLT:DC', 26, [(0, 'open;vsrc:1.234', {'NONE'}), (10, 'isrc:0.001', {'AJACK'}), (17, 'isrc:none', {'NONE'})]),
    'dcv-neg':  ('VOLT:DC', 18, [(0, 'open;vsrc:1.234', {'NONE'}), (10, 'isrc:-0.001', {'AJACK'})]),
    'dcv-aac':  ('VOLT:DC', 20, [(0, 'open;vsrc:1.234', {'NONE'}), (10, 'isrc:0,ac=0.005,f=50', {'AJACK'})]),
    'dcv-avs':  ('VOLT:DC', 18, [(0, 'open;vsrc:1.234', {'NONE'}), (10, 'avsrc:5', {'AJACK'})]),
    'dcv-small': ('VOLT:DC', 18, [(0, 'open;vsrc:1.234;isrc:0.0002', {'NONE'})]),
    'dcmv':     ('VOLT:DC', 20, [(0, 'open;vsrc:0.1', {'NONE'}), (10, 'isrc:0.002', {'AJACK'})]),
    'ohm':      ('RES', 26, [(0, 'open;r:4700', {'NONE'}), (12, 'isrc:0.002', {'AJACK'})]),
    'temp':     ('TEMP', 26, [(0, 'open;tc:100;cj:25', {'NONE'}), (12, 'isrc:0.005', {'AJACK'})]),
    'acv':      ('VOLT:AC', 26, [(0, 'open;vsrc:0,ac=1.0,f=50', {'NONE'}), (10, 'isrc:0,ac=0.01,f=50', {'AJACK'}), (18, 'isrc:none', {'NONE'})]),
    'acv-dc':   ('VOLT:AC', 20, [(0, 'open;vsrc:0,ac=1.0,f=50', {'NONE'}), (10, 'isrc:0.001', {'AJACK'})]),
    'acmv':     ('VOLT:AC', 20, [(0, 'open;vsrc:0,ac=0.05,f=50', {'NONE'}), (10, 'isrc:0,ac=0.005,f=60', {'AJACK'})]),
    'dca-v':    ('CURR:DC', 28, [(0, 'open;vsrc:5', {'NONE'}), (10, 'vsrc:24', {'VJACK'}), (19, 'vsrc:none', {'NONE'})]),
    'dca-vac':  ('CURR:DC', 20, [(0, 'open', {'NONE'}), (10, 'vsrc:0,ac=230,f=50', {'VJACK'})]),
    'dca-vi':   ('CURR:DC', 16, [(0, 'open;vsrc:24;isrc:0.0123', {'NONE'})]),
    'ohm-live': ('RES', 22, [(0, 'open;r:4700', {'NONE'}), (10, 'vsrc:-1.0,rs=100', {'LIVE'})]),
    'cont-live': ('CONT', 18, [(0, 'open;r:10', {'NONE'}), (8, 'open;vsrc:1.234', {'LIVE'})]),
    'diode-live': ('DIOD', 18, [(0, 'open;diode:0.65', {'NONE'}), (8, 'open;vsrc:-1.0,rs=100', {'LIVE'})]),
    'masked':   ('VOLT:DC', 18, [(0, 'open;vsrc:1.234;isrc:0.001', {'NONE'})]),
}


def run(name):
    func, run_for, phases = SC[name]
    script = [(t, spec) for t, spec, _ in phases]
    expect = []
    for i, (t, _, allowed) in enumerate(phases):
        t1 = phases[i + 1][0] - 1.5 if i + 1 < len(phases) else run_for
        expect.append((t + 2.0 if t > 0 else 5.0, t1, allowed))
    tag = 'sent-' + name
    os.makedirs(OUT, exist_ok=True)
    sp = os.path.join(OUT, tag + '.script')
    with open(sp, 'w') as f:
        for t, spec in script:
            f.write('%g dmm %s\n' % (t, spec))
    # USB/IP from 3 s: an attach before fw-rs enumerated can leave the server stuck
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--fast', '--usbip', '3',
           '--usbip-port', str(A.port), '--run-for', str(run_for), '--script', sp, '--name', tag,
           '--shot', os.path.join(OUT, tag + '.png')] + (['--app', os.path.abspath(A.app)] if A.app else [])
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    rows = []
    t_wall = time.time()
    tm = None
    while p.poll() is None and tm is None and time.time() - t_wall < 300:
        try:
            d = uc.Dev('1-0')
            tm = uc.Usbtmc(d)
        except Exception as e:
            print('attach:', type(e).__name__, e, flush=True)
            time.sleep(1)
    if tm is not None:
        try:
            if name == 'masked':
                tm.write('SYST:ALER:ENAB 6')
            tm.write('CONF:' + func)
            while p.poll() is None:
                row = {}
                for q in ['DBG:PING?', 'SYST:ALER?', 'DBG:SENT?', 'READ?']:
                    tm.write(q)
                    row[q] = tm.read(timeout=60).decode(errors='replace').strip()
                row['t'] = int(row['DBG:PING?'].split(',')[0]) / 1000.0
                rows.append(row)
                print('%s %6.2f %-6s %-40s %s' % (name, row['t'], row['SYST:ALER?'], row['DBG:SENT?'], row['READ?']), flush=True)
        except Exception as e:
            print(name, 'link ended:', e, flush=True)
    out, _ = p.communicate()
    ok = True
    bad = []
    for t0, t1, allowed in expect:
        seen = [r for r in rows if t0 <= r['t'] <= t1]
        if not seen:
            ok = False
            bad.append('no samples in %g..%g' % (t0, t1))
        for r in seen:
            if r['SYST:ALER?'] not in allowed:
                ok = False
                bad.append('%.2f %s not in %s' % (r['t'], r['SYST:ALER?'], sorted(allowed)))
    res = {'name': name, 'ok': ok, 'bad': bad, 'rows': rows, 'rc': p.returncode, 'log': out[-600:] if p.returncode else ''}
    with open(os.path.join(OUT, tag + '.json'), 'w') as f:
        json.dump(res, f, indent=1)
    print('RESULT %s %s %s' % (name, 'OK' if ok else 'FAIL', '; '.join(bad[:5])), flush=True)
    return ok


if __name__ == '__main__':
    if A.list:
        for n, (f, rf, ph) in SC.items():
            print('%-11s %-8s %s' % (n, f, ' | '.join('%g %s -> %s' % (t, sp, '/'.join(sorted(al))) for t, sp, al in ph)))
        sys.exit(0)
    allok = True
    for n in A.names or list(SC):
        allok &= run(n)
    sys.exit(0 if allok else 1)
