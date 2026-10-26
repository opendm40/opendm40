#!/usr/bin/env python3
"""opendm40 DMM frequency: a step of the input frequency while Hz reads.

Device 2026-10-26 (MR9270 square on V/COM, DBG:READout? every 4 s): after
the input went from 1 Hz to 0.5 Hz the readout showed F,Hz,1.402781 (final)
for ~8 s, then F,Hz,0.000, the correct 0.500022 only from ~12 s; the same
going to 0.7 Hz. A gate across the step counted an input period cut short
(the source restarts its waveform on a change: the HY3131 model's
SignalHzChanged, inferred), and the slower gates after it were given up as
"no signal" (firmware/src/dmm.rs `Freq::take`, `gate_limit_ms`).

Input: a 1 V square on V/COM, its frequency stepped at fixed virtual times
(STEPS, each a few gate phases apart). DC V -> CONF:FREQ at the start, then
DBG:READout? every ~0.2 s to the end. Checks:
  - every final (F) reading is within TOL (1 %) of the frequency before or
    after the latest step (never 0.000 Hz, never a mixed value);
  - after each step the new frequency reads final within SETTLE_S;
  - 0.5 Hz reads (the low end of the range).
Provisional (G) readings are listed, not checked.

  emu/hy3131/freqsteptest.py --port 48110
  emu/hy3131/freqsteptest.py --app firmware/opendm40.bin --port 48110

Result: emu/out/freqstep.json. Exit status 0 = passed.
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
ap.add_argument('--port', type=int, default=48110, help='USB/IP port (default 48110)')
ap.add_argument('--tol', type=float, default=0.01, help='relative tolerance of a final reading (default 0.01)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import emusync  # noqa: E402
import usbip_client as uc  # noqa: E402

F0 = 1.0
# (virtual s, Hz): offsets within the gates differ from step to step.
STEPS = [(20.0, 0.5), (36.0, 1.0), (47.3, 0.5), (63.7, 0.7), (79.1, 1.0), (88.6, 0.7), (101.2, 0.5)]
END_S = 118.0
SETTLE_S = 14.0


def main():
    if uc.port_busy(A.port):
        print('RESULT freqstep FAIL port %d in use' % A.port, flush=True)
        return False
    name = 'freqstep-%d-%d' % (A.port, os.getpid())
    os.makedirs(OUT, exist_ok=True)
    sp = os.path.join(OUT, name + '.script')
    with open(sp, 'w') as f:
        f.write('0 dmm hz:%g,amp=1\n' % F0)
        for t, hz in STEPS:
            f.write('%g dmm hz:%g,amp=1\n' % (t, hz))
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--fast', '--usbip', '3',
           '--usbip-port', str(A.port), '--run-for', str(int(END_S + 10)), '--script', sp, '--name', name] \
        + (['--app', os.path.abspath(A.app)] if A.app else [])
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    mk = emusync.Markers(emusync.log_path(OUT, name))
    t_wall = time.time()
    tm = None
    while p.poll() is None and tm is None and time.time() - t_wall < 300:
        try:
            tm = uc.Usbtmc(uc.Dev('1-0'))
        except Exception as e:
            print('attach:', type(e).__name__, e, flush=True)
            time.sleep(1)
    rows, bad = [], []

    def q(line):
        tm.write(line)
        return tm.read(timeout=60).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000.0

    if tm is not None:
        try:
            q('*IDN?')
            while up() < 4.0:
                time.sleep(0.2)
            tm.write('CONF:VOLT:DC')
            time.sleep(1.0)
            tm.write('CONF:FREQ')
            mk.poll()
            last = None
            while p.poll() is None:
                r = q('DBG:READout?')
                t = up()
                mk.poll()
                if mk.zero is not None and t + mk.zero > END_S:
                    break
                if r != last:
                    print('  +%.2f %s' % (t, r), flush=True)
                    last = r
                rows.append((t, r))
                time.sleep(0.15)
        except Exception as e:
            print('link ended:', type(e).__name__, e, flush=True)
            bad.append('link ended: %s' % e)
    p.terminate()
    try:
        out, _ = p.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        p.kill()
        out, _ = p.communicate()
    mk.poll()
    # fw uptime of each step (its input marker)
    steps = []
    for v, hz in STEPS:
        tf = mk.input_fw(v)
        if tf is None:
            bad.append('no input marker for the step at %g s' % v)
        steps.append((tf, hz))
    if not rows:
        print(out[-3000:], flush=True)
        bad.append('no readings')

    def segment(t):
        """(old Hz, new Hz, step fw s) at fw uptime t."""
        old, new, ts = F0, F0, 0.0
        for tf, hz in steps:
            if tf is not None and t >= tf:
                old, new, ts = new, hz, tf
        return old, new, ts

    finals, provs = 0, 0
    settled = {}
    for t, r in rows:
        f = r.split(',')
        if len(f) < 3 or f[0] not in ('F', 'G'):
            continue
        try:
            v = float(f[2])
        except ValueError:
            continue
        old, new, ts = segment(t)
        if f[0] == 'G':
            provs += 1
            continue
        finals += 1
        # A reading polled just after a step may still be the one before it:
        # the old value is allowed in its segment.
        if not any(abs(v - x) <= A.tol * x for x in (old, new)):
            bad.append('final %s at +%.2f (step %g -> %g Hz at +%.2f)' % (v, t, old, new, ts))
        if abs(v - new) <= A.tol * new and ts not in settled:
            settled[ts] = t - ts
    for tf, hz in steps:
        if tf is None:
            continue
        if tf + SETTLE_S > rows[-1][0] if rows else True:
            continue
        if tf not in settled:
            bad.append('%g Hz not final within %g s of its step at +%.2f' % (hz, SETTLE_S, tf))
        elif settled[tf] > SETTLE_S:
            bad.append('%g Hz final only %.2f s after its step at +%.2f' % (hz, settled[tf], tf))
    for x in (sp,):
        if os.path.exists(x):
            os.remove(x)
    ok = not bad
    with open(os.path.join(OUT, 'freqstep.json'), 'w') as f:
        json.dump({'ok': ok, 'bad': bad, 'steps': steps, 'settle': {'%.2f' % k: v for k, v in settled.items()},
                   'rows': rows}, f, indent=1)
    print('RESULT freqstep %s finals %d provisional %d; new value final after %s s; %s' % (
        'OK' if ok else 'FAIL', finals, provs,
        ' / '.join('%.1f' % settled[tf] if tf in settled else '-' for tf, _ in steps if tf is not None),
        '; '.join(bad[:8])), flush=True)
    return ok


if __name__ == '__main__':
    sys.exit(0 if main() else 1)
