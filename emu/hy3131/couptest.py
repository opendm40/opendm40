#!/usr/bin/env python3
"""fw-rs AUTO coupling of V / A (fw-rs/src/dmm.rs AUTO_*, docs/acdc-stock-vs-fwrs.md
section 11) in the emulator: fw-rs native + the HY3131 model, a timed
connection script per scenario (DC -> AC -> mixed signals, both ways through
the hysteresis band), SCPI over USB/IP (setup, then DBG:PING? uptime,
VOLT:COUP? / CURR:COUP?, FETCh?, FUNCtion? in a loop); the last samples of
every phase must match its expectation.

  emu/hy3131/couptest.py                          # all scenarios, fw-rs built from HEAD
  emu/hy3131/couptest.py --app fw-rs/dm40-fw-native.bin --port 3391 v
  emu/hy3131/couptest.py --list

Results: emu/out/coup-<name>.json (+ .png of the end); the Renode log of that
run: emu/out/coup-<name>-<port>-<pid>.log (capped by DM40_EMU_LOG_MAX_MB).
One emulator per port; a busy port is refused.
"""
import argparse
import json
import math
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
ap.add_argument('--port', type=int, default=3391, help='USB/IP port (default 3391)')
ap.add_argument('--list', action='store_true')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402


def num(s):
    try:
        v = float(s)
    except ValueError:
        return float('nan')
    return float('nan') if abs(v) > 1e37 else v


def near(x, want, rel, ab=0.0):
    return not math.isnan(x) and abs(x - want) <= max(abs(want) * rel, ab)


def shows(coup, value, rel=0.01, ab=0.002):
    """VOLT:COUP? / CURR:COUP? answers `coup` and FETCh? is `value`."""
    return lambda d: d['C'] == coup and near(d['V'], value, rel, ab)


def both(*fs):
    return lambda d: all(f(d) for f in fs)


def func(name):
    return lambda d: d['F'] == '"%s"' % name


# name: (setup SCPI, coupling query, run_for, [(t, spec, check, label)], {t: command});
# a phase is checked on its samples from the next phase's start - 4.5 s to
# that start - 1.5 s (the fw uptime lags the virtual time by ~0.5 s or more).
# AUTO rule (dmm.rs): to AC when AC > 1.5 |DC| (and above the floor), to DC
# when AC < 0.75 |DC|; in between the part shown stays (AC = |DC| below).
SC = {
    'v': (['VOLT:COUP AUTO'], 'VOLT:COUP?', 72, [
        (0, 'open;dcv:5', both(shows('AUTO,DC', 5.0), func('VOLT:AUTO')), 'DC 5 V: the DC part, FUNC VOLT:AUTO'),
        (12, 'open;acv:2@50', shows('AUTO,AC', 2.0), 'AC 2 V: -> the AC part'),
        (24, 'open;acv:1@50,dc=1', shows('AUTO,AC', 1.0), 'mixed AC 1 + DC 1 (band): stays AC'),
        (36, 'open;acv:0.2@50,dc=1', shows('AUTO,DC', 1.0), 'mixed AC 0.2 + DC 1: -> DC'),
        (48, 'open;acv:1@50,dc=1', shows('AUTO,DC', 1.0), 'mixed AC 1 + DC 1 (band): stays DC'),
        (60, 'open;acv:1@50,dc=-0.3', shows('AUTO,AC', 1.0), 'mixed AC 1 + DC -0.3: -> AC'),
    ], {}),
    'i': (['CURR:COUP AUTO'], 'CURR:COUP?', 50, [
        (0, 'open;dca:0.3', both(shows('AUTO,DC', 0.3, ab=0.0005), func('CURR:AUTO')), 'DC 300 mA: the DC part, FUNC CURR:AUTO'),
        (12, 'open;aca:0.2@60', shows('AUTO,AC', 0.2, ab=0.0005), 'AC 200 mA: -> AC'),
        (24, 'open;aca:0.02@60,dc=0.2', shows('AUTO,DC', 0.2, ab=0.0005), 'mixed AC 20 + DC 200 mA: -> DC'),
        (37, 'open;aca:0.2@60,dc=-0.05', shows('AUTO,AC', 0.2, ab=0.0005), 'mixed AC 200 + DC -50 mA: -> AC'),
    ], {}),
    # Touch: three taps on the active V button (DC -> AC -> AC+DC -> AUTO;
    # from 5 s: a tap at 3 s fell into the boot), the fourth back to DC;
    # then VOLT:COUP ACDC and FUNC "VOLT:AUTO".
    'tap': ([], 'VOLT:COUP?', 52, [
        (0, 'open;acv:2@50', both(shows('AUTO,AC', 2.0), func('VOLT:AUTO')), '3 taps on V: AUTO, AC 2 V'),
        (18, None, both(shows('DC', 0.0, ab=0.01), func('VOLT')), '4th tap: DC'),
        (28, None, both(shows('ACDC', 2.0), func('VOLT:ACDC')), 'VOLT:COUP ACDC'),
        (40, None, both(lambda d: d['C'] == 'AUTO,AC', func('VOLT:AUTO'), lambda d: d['X'].get('CURR:COUP?') == 'DC'), 'FUNC "VOLT:AUTO"; CURR:COUP? outside A = DC'),
    ], {5: 'DBG:TAP 36,297', 7: 'DBG:TAP 36,297', 9: 'DBG:TAP 36,297', 18: 'DBG:TAP 36,297', 28: 'VOLT:COUP ACDC', 40: 'FUNC "VOLT:AUTO"'}),
}
EXTRA = {'tap': ['CURR:COUP?']}


def run(name):
    setup, cq, run_for, phases, acts = SC[name]
    acts = dict(acts)
    script = [(t, spec) for t, spec, _, _ in phases if spec]
    if uc.port_busy(A.port):
        print('RESULT %s FAIL port %d in use (another emulator?): pass a free --port' % (name, A.port), flush=True)
        return False
    tag = 'coup-' + name
    # The run's own files (event script, Renode log, flash image, .resc) per
    # process: parallel runs (distinct --port) share none; results under tag.
    rtag = '%s-%d-%d' % (tag, A.port, os.getpid())
    os.makedirs(OUT, exist_ok=True)
    sp = os.path.join(OUT, rtag + '.script')
    with open(sp, 'w') as f:
        for t, spec in script:
            f.write('%g dmm %s\n' % (t, spec))
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--fast', '--usbip', '3',
           '--usbip-port', str(A.port), '--run-for', str(run_for), '--script', sp, '--name', rtag,
           '--shot', os.path.join(OUT, tag + '.png')] + (['--app', os.path.abspath(A.app)] if A.app else [])
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
    if tm is not None:
        try:
            for c in setup:
                tm.write(c)
            while p.poll() is None:
                row = {}
                for q in ['DBG:PING?', cq, 'FETC?', 'FUNC?'] + EXTRA.get(name, []):
                    tm.write(q)
                    row[q] = tm.read(timeout=60).decode(errors='replace').strip()
                t = int(row['DBG:PING?'].split(',')[0]) / 1000.0
                d = {'t': t, 'C': row[cq], 'V': num(row['FETC?']), 'F': row['FUNC?'], 'X': row}
                rows.append(d)
                print('%s %6.2f %-8s %-14s %s' % (name, t, d['C'], row['FETC?'], d['F']), flush=True)
                for ta in sorted(acts):
                    if t >= ta:
                        tm.write(acts.pop(ta))
        except Exception as e:
            print(name, 'link ended:', e, flush=True)
    out, _ = p.communicate()
    for x in (sp, os.path.join(OUT, 'flash-%s.bin' % rtag), os.path.join(OUT, rtag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
    ok = True
    bad = []
    for i, (t, _, chk, label) in enumerate(phases):
        t1 = phases[i + 1][0] if i + 1 < len(phases) else run_for
        seen = [r for r in rows if t1 - 4.5 <= r['t'] <= t1 - 1.5]
        if not seen:
            ok = False
            bad.append('%s: no samples' % label)
            continue
        fails = [r for r in seen if not chk(r)]
        if fails:
            ok = False
            r = fails[-1]
            bad.append('%s: %.2f %s %g %s' % (label, r['t'], r['C'], r['V'], r['F']))
        else:
            r = seen[-1]
            print('  ok  %-50s %s %g' % (label, r['C'], r['V']), flush=True)
    res = {'name': name, 'ok': ok, 'bad': bad, 'rows': rows, 'rc': p.returncode, 'log': out[-600:] if p.returncode else ''}
    with open(os.path.join(OUT, tag + '.json'), 'w') as f:
        json.dump(res, f, indent=1)
    print('RESULT %s %s %s' % (name, 'OK' if ok else 'FAIL', '; '.join(bad[:5])), flush=True)
    return ok


if __name__ == '__main__':
    if A.list:
        for n, (su, cq, rf, ph, ac) in SC.items():
            print('%-4s %s | %s' % (n, ' '.join(su), ' | '.join('%g %s: %s' % (t, s, l) for t, s, _, l in ph)))
        sys.exit(0)
    allok = True
    for n in A.names or list(SC):
        allok &= run(n)
    sys.exit(0 if allok else 1)
