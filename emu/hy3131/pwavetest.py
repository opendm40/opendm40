#!/usr/bin/env python3
"""opendm40 Power Wave (the power meter's U and I waveforms, src/app_pwave.rs,
src/pwave.rs; firmware/docs/power-meter-and-misconnect.md 3.18) in the
emulator: opendm40 native + the HY3131 model's Power role, driven like a user
(DBG:KEY / DBG:TAP) and checked over SCPI (POW:WAVE?, FETC:POW:ALL?).

  emu/hy3131/pwavetest.py [--app ELF] [--port N] [--shots DIR] [case ...]

Cases (default all):
  mains  230 V 50 Hz, 1 A, PF 0.5 (60 deg lag): MF -> Power Settings ->
         "Wave" opens the page (DBG:VIEW?); POW:WAVE? Urms 230, Irms 1,
         P 115 (the U x I pairing across the alternate reads keeps the
         phase), f 50, no unreadable sample, >= 100 samples per channel;
         "+ ms/div": more samples per channel; RUN key: STOP (no new frames),
         RUN again; MENU: the page closes and the power meter reads again
         (FETC:POW:ALL? V 230, I 1 within 2 %); POW:WAVE ON / OFF over SCPI.
  dc     12 V, 2 A DC: Urms 12, Irms 2, P 24, f none (9.91E37).
  alt    POW:METH ALT: POW:WAVE ON refused (SYST:ERR? Settings conflict).
The model latches R03..R05 at each conversion from the converter SDO23
selects then, one conversion grid for both: whether the device does so
(and how far AD3's grid sits from AD2's) is the device check's question
(doc 3.18); the firmware waits one conversion after each R34 write.
Exit status 0 = all passed. Results: emu/out/pwave-<case>.json.
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
sys.path.insert(0, EMU)
import run  # noqa: E402

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('cases', nargs='*')
ap.add_argument('--app', help='opendm40 native image (default: run.py builds HEAD)')
ap.add_argument('--port', type=int, default=None, help='USB/IP port (default: run.free_port)')
ap.add_argument('--shots', help='directory for PNG screenshots (DBG:LCD?)')
A = ap.parse_args()
A.port = A.port or run.free_port()
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

CASES = {
    'mains': 'open;pow:230@50,i=1,pf=0.5',
    'dc': 'open;vsrc:12;isrc:2',
    'alt': 'open;pow:230@50,i=1,pf=1',
}
# The Wave button of Power Settings (ui_power.rs ps_row4(3)) and the page's
# buttons (ui_pwave.rs BTN_R).
WAVE_BTN = (414, 201)
PLUS_BTN = (296, 298)


def num(s):
    try:
        v = float(s)
    except ValueError:
        return float('nan')
    return float('nan') if v > 9e37 else v


def near(x, want, rel):
    return x == x and abs(x - want) <= abs(want) * rel


def run_case(name):
    fails = []
    res = {'name': name}

    def check(ok, what):
        print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
        if not ok:
            fails.append(what)

    tag = 'pwave-%s-%d-%d' % (name, A.port, os.getpid())
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--usbip', '0.01', '--usbip-port', str(A.port),
           '--run-for', '600', '--name', tag, '--log', os.path.join(OUT, tag + '.log'), '--dmm', CASES[name] + '@0'] \
        + (['--app', os.path.abspath(A.app)] if A.app else [])
    emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
    try:
        t = None
        for _ in range(150):
            try:
                t = uc.Usbtmc(uc.Dev('1-0'))
                break
            except Exception:
                time.sleep(2)
        if t is None:
            check(False, 'no USB/IP device on port %d' % A.port)
            return False

        def q(line, raw=False, timeout=60):
            t.write(line)
            if '?' not in line:
                return None
            b = t.read(maxlen=8192, timeout=timeout)
            return b if raw else b.decode(errors='replace').strip()

        def up():
            return int(q('DBG:PING?').split(',')[0])

        def wait_ms(ms):
            t0 = up()
            while up() - t0 < ms:
                time.sleep(0.2)

        def shot(n):
            if not A.shots:
                return
            from PIL import Image
            os.makedirs(A.shots, exist_ok=True)
            rows = 4
            px = bytearray()
            for y in range(0, 320, rows):
                b = q('DBG:LCD? 0,%d,480,%d,%d' % (y, rows, 480 * rows * 3 // 2 + 1), raw=True)
                nd = int(b[1:2])
                d = b[2 + nd:2 + nd + int(b[2:2 + nd])]
                w = [d[i] | d[i + 1] << 8 for i in range(2, len(d), 2)]
                bs = bytearray()
                for v in w:
                    bs += bytes([v >> 8, v & 0xFF])
                px += bs[:480 * rows * 3]
            Image.frombytes('RGB', (480, 320), bytes(px)).save(os.path.join(A.shots, n + '.png'))
            wait_ms(400)

        def pw_all():
            return [num(x) for x in q('FETC:POW:ALL?').split(',')]

        def wave():
            f = q('POW:WAVE?').split(',')
            return {'on': f[0] == '1', 'frames': int(f[1]), 'U': num(f[2]), 'I': num(f[3]), 'P': num(f[4]),
                    'f': num(f[5]), 'n': int(f[6]), 'retries': int(f[7]), 'bad': int(f[8])}

        def wait_frames(k, limit_ms=20000):
            w0 = wave()
            t0 = up()
            while True:
                w = wave()
                if w['frames'] >= w0['frames'] + k or up() - t0 > limit_ms:
                    return w
                time.sleep(0.3)

        def view():
            return q('DBG:VIEW?')

        for c in ['CONF:POW', 'POW:COUP ACDC', 'POW:BURD:STAT OFF'] + (['POW:METH ALT'] if name == 'alt' else []):
            q(c)
        # The power meter settles on its ranges (V 600, I autoranging down).
        t0 = up()
        while up() - t0 < 40000:
            p = pw_all()
            if len(p) > 5 and p[4] == p[4] and p[5] == p[5] and p[4] > 1 and p[5] > 0.1:
                break
            time.sleep(1)
        wait_ms(4000)
        p0 = pw_all()
        res['before'] = p0
        print(name, 'FETC:POW:ALL?', p0, flush=True)
        if name == 'alt':
            q('*CLS')
            q('POW:WAVE ON')
            wait_ms(500)
            e = q('SYST:ERR?')
            check('Settings conflict' in e and not wave()['on'], 'POW:METH ALT: POW:WAVE ON refused (%s)' % e)
            return not fails
        # MF -> Power Settings -> Wave.
        q('DBG:KEY 3')
        wait_ms(800)
        v1 = view()
        q('DBG:TAP %d,%d' % WAVE_BTN)
        wait_ms(800)
        v2 = view()
        check(v2.startswith('Power Wave'), 'MF (%s) -> Wave: the page opens (%s)' % (v1.split(',')[0], v2))
        w = wait_frames(4)
        res['wave'] = w
        print(name, 'POW:WAVE?', w, flush=True)
        shot('pwave-' + name)
        check(w['on'] and w['frames'] >= 4, 'frames run (%d)' % w['frames'])
        check(w['bad'] == 0, 'no unreadable sample (bad %d, repeated reads %d)' % (w['bad'], w['retries']))
        check(w['n'] >= 100, '%d samples per channel' % w['n'])
        if name == 'mains':
            check(near(w['U'], 230, 0.01), 'Urms %.2f ~ 230' % w['U'])
            check(near(w['I'], 1.0, 0.01), 'Irms %.4f ~ 1' % w['I'])
            check(near(w['P'], 115, 0.015), 'P %.2f ~ 115 (PF 0.5: the U / I pairing keeps the phase)' % w['P'])
            check(near(w['f'], 50, 0.01), 'f %.3f ~ 50' % w['f'])
            # A slower timebase: more samples per channel.
            q('DBG:TAP %d,%d' % PLUS_BTN)
            w2 = wait_frames(3)
            check(w2['n'] > w['n'] * 1.5, '+ ms/div: %d -> %d samples per channel' % (w['n'], w2['n']))
            check(near(w2['P'], 115, 0.015), 'P %.2f at 10 ms/div' % w2['P'])
            shot('pwave-mains-10ms')
            # RUN key: STOP, then RUN.
            q('DBG:KEY 1')
            wait_ms(600)
            a = wave()['frames']
            wait_ms(2000)
            b = wave()['frames']
            check(a == b, 'STOP: no new frames (%d, %d)' % (a, b))
            q('DBG:KEY 1')
            c = wait_frames(2)['frames']
            check(c > b, 'RUN: frames again (%d)' % c)
            # MENU: back to the power meter.
            q('DBG:KEY 0')
            wait_ms(800)
            v3 = view()
            check(not wave()['on'] and not v3.startswith('Power Wave'), 'MENU closes the page (%s)' % v3)
            wait_ms(6000)
            p1 = pw_all()
            res['after'] = p1
            check(near(p1[4], 230, 0.02) and near(p1[5], 1.0, 0.02) and near(p1[0], 115, 0.04),
                  'the power meter reads again: V %.2f I %.4f P %.2f' % (p1[4], p1[5], p1[0]))
            # SCPI open / close.
            q('POW:WAVE ON')
            wait_ms(1500)
            check(wave()['on'] and view().startswith('Power Wave'), 'POW:WAVE ON opens the page')
            q('POW:WAVE OFF')
            wait_ms(800)
            check(not wave()['on'] and not view().startswith('Power Wave'), 'POW:WAVE OFF closes it')
            e = q('SYST:ERR?')
            check(e.startswith('0') or e.startswith('+0'), 'no SCPI error (%s)' % e)
        else:
            check(near(w['U'], 12, 0.01), 'Urms %.3f ~ 12' % w['U'])
            check(near(w['I'], 2, 0.01), 'Irms %.4f ~ 2' % w['I'])
            check(near(w['P'], 24, 0.02), 'P %.3f ~ 24' % w['P'])
            check(w['f'] != w['f'], 'f none (%s)' % w['f'])
        return not fails
    finally:
        emu.terminate()
        try:
            emu.wait(timeout=20)
        except subprocess.TimeoutExpired:
            emu.kill()
        res['fails'] = fails
        os.makedirs(OUT, exist_ok=True)
        with open(os.path.join(OUT, 'pwave-%s.json' % name), 'w') as f:
            json.dump(res, f, indent=1)
        print('RESULT %s %s %s' % (name, 'OK' if not fails else 'FAIL', '; '.join(fails[:4])), flush=True)


if __name__ == '__main__':
    ok = True
    for n in A.cases or list(CASES):
        ok &= run_case(n)
    sys.exit(0 if ok else 1)
