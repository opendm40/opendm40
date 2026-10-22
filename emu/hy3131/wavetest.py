#!/usr/bin/env python3
"""fw-rs DMM wave view (scope page, source DMM: the HY3131 AD2 stream,
src/app_wave.rs, TODO (A)) in the emulator, driven like a user (DBG:KEY /
DBG:TAP) and checked over SCPI on one USB/IP connection.

  cd fw-rs && ./build.sh --features ad2-isr
  emu/hy3131/wavetest.py --app fw-rs/target/native/thumbv7em-none-eabihf/release/dm40-fw [--shots DIR]

Input: --dmm (default a 1 V rms 50 Hz sine on V/COM; the DMM model's AD2
path is AC-coupled in AC V like the meter). Checks:
  * MF -> "Wave" from DC V: the AC V row (AC-coupled note), the stream ISR
    runs (DBG:ISR? active), CHAN1:SCAL? = the DMM range's V/div;
  * per timebase 5 ms .. 100 ms/div (4 rate bands: 9600 / 4800 / 2400 S/s):
    SCOP:MEAS:FREQ? 50 Hz, VRMS? 1.0 V, VPP? 2.83 V (+-3 %), the rate
    restarts on a band change;
  * trigger: NORM at +0.5 V rising: WAV:DATA? frames cross the level at the
    trigger column (200) rising, the same on two frames; falling too;
  * roll 500 ms/div (WAV:DATA? fills from the right), 5 s/div;
  * V/div + (panel B3): the next DMM range, the trace the same in volts;
  * MENU: the view closes, the ISR stops, READ? = 1.0 V AC again (DMM
    resumed), no SYST:ERR?;
  * a host CONF mid-wave: the view closes (forced stop), DC V then reads ~0.
--shots DIR: PNG screenshots (DBG:LCD?) of each stage for a look.
Exit status 0 = all passed.
"""
import argparse
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='fw-rs native ELF built with --features ad2-isr')
ap.add_argument('--port', type=int, default=3591, help='USB/IP port (default 3591)')
ap.add_argument('--dmm', default='vsrc:0,ac=1.0,f=50', help='DMM jack input (emu/run.py --dmm)')
ap.add_argument('--shots', help='directory for PNG screenshots')
ap.add_argument('--mips', default='200')
ap.add_argument('--case', choices=['volt', 'current'], default='volt',
                help='current: --dmm defaults to a 10 mA DC + 40 Hz square on A-COM; the AC A row is '
                     'DC-coupled: FREQ? 40 Hz, VAVG? = the DC part, values in A')
A = ap.parse_args()
if A.case == 'current' and A.dmm == ap.get_default('dmm'):
    A.dmm = 'isrc:0.010,ac=0.02,f=40,shape=square'
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

FAIL = []


class Done(Exception):
    pass


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what))
    if not ok:
        FAIL.append(what)


cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '3000', '--name', 'wavetest',
       '--dmm', A.dmm]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
try:
    t = None
    for _ in range(120):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('wavetest: no USB/IP device on port %d' % A.port)

    def q(line, raw=False, timeout=60):
        t.write(line)
        if '?' not in line:
            return None
        b = t.read(maxlen=8192, timeout=timeout)
        return b if raw else b.decode(errors='replace').strip()

    def ints(s):
        return [int(x, 0) for x in s.split(",")]

    def f(line):
        return float(q(line))

    def up():
        return int(q('DBG:PING?').split(',')[0])

    def wait_ms(ms):
        """Virtual (device) milliseconds."""
        t0 = up()
        while up() - t0 < ms:
            time.sleep(0.2)

    def shot(name):
        if not A.shots:
            return
        from PIL import Image
        os.makedirs(A.shots, exist_ok=True)
        rows = 4
        px = bytearray()
        for y in range(0, 320, rows):
            n = 480 * rows * 3 // 2 + 1
            b = q('DBG:LCD? 0,%d,480,%d,%d' % (y, rows, n), raw=True)
            nd = int(b[1:2])
            d = b[2 + nd:2 + nd + int(b[2:2 + nd])]
            w = [d[i] | d[i + 1] << 8 for i in range(2, len(d), 2)]  # dummy word dropped
            bs = bytearray()
            for v in w:
                bs += bytes([v >> 8, v & 0xFF])
            px += bs[:480 * rows * 3]
        Image.frombytes('RGB', (480, 320), bytes(px)).save(os.path.join(A.shots, name + '.png'))
        wait_ms(400)  # DBG:LCD? holds drawing for 300 ms

    def wave():
        b = q('WAV:DATA?', raw=True)
        return list(b[5:405])

    def meas():
        return f('SCOP:MEAS:FREQ?'), f('SCOP:MEAS:VRMS?'), f('SCOP:MEAS:VPP?')

    def near(v, want, tol):
        return abs(v - want) <= tol * abs(want)

    def current_case():
        q('CONF:CURR:DC')
        wait_ms(2500)
        dc = f('READ?')
        q('DBG:KEY 3')
        wait_ms(600)
        q('DBG:TAP 237,259')
        wait_ms(3000)
        shot('c01-wave-current')
        check(ints(q('DBG:ISR?'))[1] == 1, 'the stream runs (A function)')
        check(q('FUNC?').strip('"').upper().startswith('CURR:AC'), 'the DMM is in the AC A row (%s)' % q('FUNC?'))
        q('TIM:SCAL 0.02')
        wait_ms(2500)
        fr, avg, pp = f('SCOP:MEAS:FREQ?'), f('SCOP:MEAS:VAVG?'), f('SCOP:MEAS:VPP?')
        print('  DC A READ? before %.5f A; wave: FREQ %.3f Hz, AVG %.5f A, PP %.5f A, CHAN1:SCAL? %s, WAV:PRE? %s'
              % (dc, fr, avg, pp, q('CHAN1:SCAL?'), q('WAV:PRE?')))
        check(near(fr, 40.0, 0.03), 'square 40 Hz on the current row')
        check(near(avg, dc, 0.05), 'DC-coupled: the mean = the DC A reading (%.5f vs %.5f)' % (avg, dc))
        shot('c02-wave-current-20ms')
        q('DBG:KEY 0')
        wait_ms(2500)
        check(ints(q('DBG:ISR?'))[1] == 0, 'MENU: the stream stopped')

    print('*IDN?', q('*IDN?'))
    isr0 = ints(q('DBG:ISR?'))
    check(isr0[0] == 1, 'DBG:ISR? feature = 1 (an ad2-isr build)')
    if A.case == 'current':
        current_case()
        raise Done
    q('CONF:VOLT:DC')
    # The AC sine on DC V raises the sentinel's "AC voltage here: use AC V?"
    # hint toast (~1.5 s after the switch, shown 1.5 s, sentinel.rs) and a
    # toast takes the next key (app.rs overlay): MF only after it went.
    wait_ms(4500)
    shot('w00-dmm')
    # MF -> the V range page -> Wave.
    q('DBG:KEY 3')
    wait_ms(600)
    shot('w01-range-page')
    q('DBG:TAP 237,259')
    wait_ms(2500)
    shot('w02-wave-10ms')
    isr = ints(q('DBG:ISR?'))
    check(isr[1] == 1, 'the stream ISR runs in the wave view (DBG:ISR? %s)' % isr[:4])
    check(q('FUNC?').strip('"').upper().startswith('VOLT:AC'), 'the DMM is in AC V (%s)' % q('FUNC?'))
    vd = f('CHAN1:SCAL?')
    check(vd in (2.0, 20.0, 200.0, 500.0), 'CHAN1:SCAL? = the DMM range V/div (%g)' % vd)
    print('  WAV:PRE?', q('WAV:PRE?'))

    for tb, name in [(0.005, '5ms'), (0.01, '10ms'), (0.02, '20ms'), (0.05, '50ms'), (0.1, '100ms')]:
        q('TIM:SCAL %g' % tb)
        wait_ms(1500 + int(tb * 10 * 2000))
        fr, rms, vpp = meas()
        print('  %s/div: FREQ %.3f Hz, VRMS %.4f V, VPP %.4f V' % (name, fr, rms, vpp))
        check(abs(f('TIM:SCAL?') - tb) < tb * 0.01, 'TIM:SCAL? %s' % name)
        check(near(fr, 50.0, 0.03) and near(rms, 1.0, 0.03) and near(vpp, 2.828, 0.04),
              '%s/div: 50 Hz, 1 V rms, 2.83 Vpp' % name)
        if tb in (0.005, 0.05):
            shot('w03-wave-%s' % name)

    print('trigger')
    q('TIM:SCAL 0.01')
    q('TRIG:SWE NORM')
    for lev, slope in [(0.5, 'POS'), (-0.8, 'NEG')]:
        q('TRIG:SLOP %s' % slope)
        q('TRIG:LEV %g' % lev)
        wait_ms(1500)
        pre = [float(x) for x in q('WAV:PRE?').split(',')]
        k, z = pre[2], pre[3]
        ok = True
        for _ in range(2):
            w = wave()
            v = [c * k + z for c in w]
            a, b = v[196], v[204]
            rise = b > a if slope == 'POS' else b < a
            hit = min(a, b) - 0.15 <= lev <= max(a, b) + 0.15
            ok = ok and rise and hit
            print('    level %g %s: v[196] %.3f v[200] %.3f v[204] %.3f' % (lev, slope, a, v[200], b))
            wait_ms(300)
        check(ok, 'NORM %s %g V: the frame crosses the level at the trigger column' % (slope, lev))
    shot('w04-trig-falling')
    q('TRIG:SWE SING')
    wait_ms(1500)
    w1 = wave()
    wait_ms(600)
    check(w1 == wave(), 'SINGle: one frame, then stopped (WAV:DATA? unchanged)')
    shot('w04b-single')
    q('TRIG:SWE AUTO')
    q('RUN')
    q('TRIG:SLOP POS')
    q('TRIG:LEV 0')

    print('roll')
    q('TIM:SCAL 0.5')
    wait_ms(3000)
    w = wave()
    shot('w05-roll-500ms')
    check(w[399] > 0 and any(x != w[399] for x in w[300:400]), 'roll 500 ms/div: data at the right edge')
    q('TIM:SCAL 5')
    wait_ms(4000)
    shot('w06-roll-5s')
    # Roll speed (WAV:DATA? keeps the last triggered frame in roll: read
    # the screen). 5 s/div = 8 px/s; before the fix the roll took `screen`
    # (clamped to the record) and moved as at 2 s/div (20 px/s).
    def roll_left():
        b = q('DBG:LCD? 0,160,480,1,721', raw=True)
        nd = int(b[1:2])
        d = b[2 + nd:2 + nd + int(b[2:2 + nd])]
        w = [d[i] | d[i + 1] << 8 for i in range(2, len(d), 2)]
        bs = bytearray()
        for v in w:
            bs += bytes([v >> 8, v & 0xFF])
        xs = [x for x in range(14, 412) if bs[3 * x] > 200 and bs[3 * x + 1] > 200 and bs[3 * x + 2] < 100]
        return min(xs) if xs else 412
    x1 = roll_left()
    t1 = up()
    wait_ms(4000)
    x2 = roll_left()
    v = (x1 - x2) * 1000.0 / (up() - t1)
    print('  roll 5 s/div: left edge %d -> %d, %.1f px/s' % (x1, x2, v))
    check(5 <= v <= 12, 'roll 5 s/div moves ~8 px/s (got %.1f)' % v)
    q('TIM:SCAL 0.01')
    wait_ms(1500)

    print('V/div + (the next DMM range)')
    vd0 = f('CHAN1:SCAL?')
    q('DBG:TAP 447,122')
    wait_ms(2500)
    vd1 = f('CHAN1:SCAL?')
    fr, rms, vpp = meas()
    print('  V/div %g -> %g, FREQ %.3f VRMS %.4f VPP %.4f' % (vd0, vd1, fr, rms, vpp))
    check(vd1 > vd0, 'V/div + steps the DMM range up (%g -> %g)' % (vd0, vd1))
    check(near(rms, 1.0, 0.05) and near(fr, 50.0, 0.03), 'the same signal in volts on the next range')
    shot('w07-range-up')
    q('DBG:TAP 447,40')
    wait_ms(2500)
    check(f('CHAN1:SCAL?') == vd0, 'V/div - back (%g)' % f('CHAN1:SCAL?'))

    print('STOP / RUN')
    q('STOP')
    wait_ms(800)
    shot('w08-stopped')
    q('RUN')
    wait_ms(1500)
    fr, rms, vpp = meas()
    check(near(fr, 50.0, 0.03), 'RUN again: 50 Hz (%.3f)' % fr)

    print('exit (MENU)')
    q('DBG:KEY 0')
    wait_ms(2500)
    isr = ints(q('DBG:ISR?'))
    check(isr[1] == 0, 'the ISR stopped with the view (%s)' % isr[:4])
    check(isr[13] == isr0[13], 'no forced stop (%d)' % isr[13])
    v = [f('READ?') for _ in range(2)][-1]
    check(near(v, 1.0, 0.01), 'DMM resumed: READ? %.4f V AC' % v)
    err = q('SYST:ERR?')
    check(err.startswith('0,') or err.startswith('+0,'), 'SYST:ERR? empty (%s)' % err)

    print('source badge, host CONF mid-wave')
    q('DBG:KEY 0')  # the menu steps back to the (MCX) scope page
    wait_ms(1000)
    shot('w09-mcx-badge')
    q('DBG:TAP 177,10')  # the source badge: MCX -> DMM
    wait_ms(2500)
    check(ints(q('DBG:ISR?'))[1] == 1, 'the source badge opens the wave view again')
    q('TIM:SCAL 0.01')
    wait_ms(2000)
    fr, rms, vpp = meas()
    print('  after the badge: FREQ %.3f VRMS %.4f VPP %.4f' % (fr, rms, vpp))
    check(near(fr, 50.0, 0.03) and near(rms, 1.0, 0.03), 'the badge path shows the signal (50 Hz, 1 V rms)')
    shot('w09b-badge-wave')
    f0 = ints(q('DBG:ISR?'))[13]
    q('CONF:VOLT:DC')
    wait_ms(1500)
    isr = ints(q('DBG:ISR?'))
    check(isr[1] == 0 and isr[13] == f0 + 1, 'CONF stopped the stream before its bus access (forced %d -> %d)' % (f0, isr[13]))
    shot('w10-lost')
    wait_ms(1500)
    v = f('READ?')
    check(abs(v) < 0.02, 'DC V reads ~0 after (%.4f)' % v)
except Done:
    pass
finally:
    os.killpg(emu.pid, 15)
    emu.wait()

print('wavetest: %s' % ('PASS' if not FAIL else 'FAIL: ' + '; '.join(FAIL)))
sys.exit(1 if FAIL else 0)
