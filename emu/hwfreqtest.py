#!/usr/bin/env python3
"""Comparator frequency counter (src/hwtrig.rs FreqCounter: TIM2 captures the
PA15 edges, TIM5 counts them; scope_meas.rs Freq / Cycl) in the emulator,
and the comparator trigger after its post count moved from TIM2 to TIM4.

  cd firmware && ./build.sh --dev
  emu/hwfreqtest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

One emulator run; the input changes at fixed virtual times (run.py
--scope SPEC@T). Per case, after the change has settled:
  * DBG:HWTRIG:FREQ? (the counter) within 20 ppm of the input frequency;
  * SCOP:MEAS:FREQ? (the Freq cell) the same value, also on a timebase
    with less than one period on screen (the window search gives none);
  * the trigger level above the signal: no counter result, Freq falls back
    to the window search;
  * Normal sweep: the comparator still triggers (DBG:HWTRIG:STAT? hits
    grow, no misses).
Exit status 0 = all passed.
"""
import argparse
import math
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image (dev build)')
ap.add_argument('--port', type=int, default=24190, help='USB/IP port (default 24190)')
ap.add_argument('--mips', default='200')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, HERE)
import usbip_client as uc  # noqa: E402

FAIL = []
# (virtual start s, wave spec, Hz, TIM:SCAL, what): each case runs from its
# start to the next one's.
# The first case's wave is also the input from boot (cases start after it).
CASES = [
    (20, 'sine,1000,1.0', 1000.0, 200e-6, 'sine 1 kHz, 200 us/div (several periods on screen)'),
    (34, 'sine,1000,1.0', 1000.0, 10e-6, 'sine 1 kHz, 10 us/div (a seventh of a period on screen)'),
    (40, 'square,37.5,1.0', 37.5, 1e-3, 'square 37.5 Hz, 1 ms/div (half a period on screen)'),
    (48, 'sine,123456.7,1.0', 123456.7, 1e-3, 'sine 123.4567 kHz, 1 ms/div'),
    (54, 'triangle,2500,1.0', 2500.0, 500e-6, 'triangle 2.5 kHz, 500 us/div'),
]
END = 60


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


class T:
    def __init__(self, u):
        self.u = u

    def w(self, line):
        self.u.write(line)

    def q(self, line):
        self.u.write(line)
        return self.u.read(maxlen=4096, timeout=30).decode(errors='replace').strip()

    def vtime(self):
        return int(self.q('DBG:PING?').split(',')[0]) / 1000.0

    def until(self, v):
        while self.vtime() < v:
            time.sleep(0.2)

    def freq(self):
        f = self.q('DBG:HWTRIG:FREQ?').split(',')
        v = float(f[0])
        # SCPI NaN is 9.91E37.
        return math.nan if v > 9e37 else v

    def meas(self):
        return float(self.q('SCOP:MEAS:FREQ?'))


def connect():
    for _ in range(150):
        try:
            return T(uc.Usbtmc(uc.Dev('1-0')))
        except Exception:
            time.sleep(2)
    sys.exit('hwfreqtest: no USB/IP device on port %d' % A.port)


def near(v, hz, ppm=20.0):
    return math.isfinite(v) and abs(v / hz - 1.0) <= ppm * 1e-6


def main():
    if uc.port_busy(A.port):
        sys.exit('hwfreqtest: port %d is in use (another emulator?)' % A.port)
    tag = 'hwfreq-%d' % A.port
    cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
           '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '3000',
           '--name', tag, '--log', os.path.join(HERE, 'out', tag + '.log')]
    cmd += ['--scope', CASES[0][1]]
    for t0, spec, _, _, _ in CASES:
        cmd += ['--scope', '%s@%g' % (spec, t0)]
    emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
    try:
        t = connect()
        print('*IDN?', t.q('*IDN?'), flush=True)
        for c in ('*CLS', 'CHAN1:COUP DC', 'CHAN1:SCAL 0.2', 'CHAN1:OFFS 0', 'TRIG:SWE AUTO', 'TRIG:SLOP POS',
                  'TRIG:LEV 0', 'TRIG:SOUR CHAN1', 'RUN'):
            t.w(c)
        e = t.q('SYST:ERR?')
        check(e.startswith('0,'), 'set-up accepted (%s)' % e)
        v = t.vtime()
        if v > CASES[0][0] - 2:
            sys.exit('hwfreqtest: booted at %.1f s virtual, after the first case (%g s)' % (v, CASES[0][0]))
        for i, (t0, spec, hz, tb, what) in enumerate(CASES):
            t1 = CASES[i + 1][0] if i + 1 < len(CASES) else END
            print('== %s (%s, %g..%g s)' % (what, spec, t0, t1), flush=True)
            t.w('TIM:SCAL %g' % tb)
            # The wave changes at t0 (the first gate after it may span both).
            t.until(t0 + 2.0)
            f = t.freq()
            check(near(f, hz), 'counter %.6f Hz (input %g Hz)' % (f, hz))
            m = t.meas()
            check(near(m, hz), 'Freq cell %.6f Hz' % m)
            if i == 0:
                # Comparator trigger (TIM4 post count) still works.
                t.w('DBG:HWTRIG:STAT:RESET')
                t.w('TRIG:SWE NORM')
                v = t.vtime()
                t.until(v + 3.0)
                s = t.q('DBG:HWTRIG:STAT?').split(',')
                hits, miss = int(s[0]), int(s[1])
                check(hits >= 5 and miss == 0, 'Normal: comparator triggers (hits %d, misses %d)' % (hits, miss))
                hw = t.q('DBG:HWTRIG?').split(',')
                check(hw[4:6] == ['1', '1'], 'comparator enabled and available (DBG:HWTRIG? %s)' % ','.join(hw[4:6]))
                st = t.q('TRIG:STAT?')
                check(st == 'TD', 'trigger status %s' % st)
                t.w('TRIG:SWE AUTO')
                # Level above the signal (0.5 V peak): no edges, no result;
                # Freq from the window search (several periods on screen).
                t.w('TRIG:LEV 0.8')
                v = t.vtime()
                t.until(v + 2.5)
                f = t.freq()
                check(math.isnan(f), 'level above the signal: no counter result (%s)' % f)
                m = t.meas()
                check(math.isfinite(m) and abs(m / hz - 1) < 0.02, 'Freq cell from the window: %.3f Hz' % m)
                t.w('TRIG:LEV 0')
        e = t.q('SYST:ERR?')
        check(e.startswith('0,'), 'no SCPI error (%s)' % e)
    finally:
        try:
            os.killpg(emu.pid, 15)
        except ProcessLookupError:
            pass
        emu.wait(timeout=30)
    print('FAILED: %d' % len(FAIL) if FAIL else 'all passed')
    sys.exit(1 if FAIL else 0)


main()
