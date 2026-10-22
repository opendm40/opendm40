#!/usr/bin/env python3
"""fw-rs DMM wave view (scope page, source DMM: the HY3131 AD2 stream,
src/app_wave.rs, TODO (A)) in the emulator, driven like a user (DBG:KEY /
DBG:TAP) and checked over SCPI on one USB/IP connection.

  cd fw-rs && ./build.sh --features ad2-isr
  emu/hy3131/wavetest.py --app fw-rs/target/native/thumbv7em-none-eabihf/release/dm40-fw [--shots DIR]

Input: --dmm (default a 1 V rms 50 Hz sine on V/COM; the DMM model's AD2
path is AC-coupled in AC V like the meter). Checks:
  * MF -> "Wave" from AC V: the AC V row, the stream ISR runs (DBG:ISR?
    active), CHAN1:SCAL? = the DMM range's V/div;
  * per timebase 5 ms .. 100 ms/div (4 rate bands: 9600 / 4800 / 2400 S/s):
    SCOP:MEAS:FREQ? 50 Hz, VRMS? 1.0 V, VPP? 2.83 V (+-3 %), the rate
    restarts on a band change;
  * 2 ms/div (38400 S/s) and 1 ms/div (76800 S/s, the hybrid frames): the
    stream's rate / divider, locked, no late / timeout / overrun / slip /
    bad sample and no ring overflow over 4 s, the trace updates, 2 ms/div
    1 V rms / 2.83 Vpp over its one period;
  * trigger: NORM at +0.5 V rising: WAV:DATA? frames cross the level at the
    trigger column (200) rising, the same on two frames; falling too;
  * roll 500 ms/div (WAV:DATA? fills from the right), 5 s/div;
  * V/div + (panel B3): the next DMM range, the trace the same in volts;
  * MENU: the view closes, the ISR stops, READ? = 1.0 V AC again (DMM
    resumed), no SYST:ERR?;
  * a host CONF mid-wave: the view closes (forced stop), DC V then reads ~0;
  * region C (flash above 0x08080000, not zero-wait: ~20 us ISR entry
    stalls on the device, fw-rs/link-native.x) while the stream ISR owns the
    bus: a Renode hook on every function of C logs the first call made
    while hy3131::HANDED is set. STOP + MF Settings "Save CSV" in the wave
    view (the DLG write pauses the stream, the USB volume refresh waits
    until the stream stops; device f3aea2f: max_lat 21.7 us in vdisk::ours),
    then a host reading SCOPE/SCOPnnn.CSV and LOGS/LOGnnn.CSV off the USB
    volume and MMEM:DATA? of their DLGs while the wave view streams: no C
    function but the Save CSV tap's own handler; the stream runs again
    after the save. --no-region-c: without the hooks.
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
ap.add_argument('--no-region-c', action='store_true', help='without the region C hooks')
ap.add_argument('--case', choices=['volt', 'current', 'dcv'], default='volt',
                help='current: --dmm defaults to a 10 mA DC + 40 Hz square on A-COM; the AC A row is '
                     'DC-coupled: FREQ? 40 Hz, VAVG? = the DC part, values in A. dcv: 0.2 V DC + 1 V rms '
                     '50 Hz; MF -> Wave from DC V AUTO (DC mV, the AC mV row) clips and autoranges to 6 V '
                     '(the DC-coupled V row, feasibility E7): VAVG? = the DC part; the coupling key '
                     'switches to AC V (VAVG? ~0) and back; MENU: DC V AUTO again')
A = ap.parse_args()
if A.case == 'current' and A.dmm == ap.get_default('dmm'):
    A.dmm = 'isrc:0.010,ac=0.02,f=40,shape=square'
if A.case == 'dcv' and A.dmm == ap.get_default('dmm'):
    A.dmm = 'vsrc:0.2,ac=1.0,f=50'
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


# Region C hooks (see the docstring): every function symbol in
# 0x08080000..0x080A0000 of the ELF; a call while the ISR owns the bus
# (hy3131::HANDED != 0) is counted, the count written at 1, 2, 4, ... to a
# per-process file.
OUT = os.path.join(EMU, 'out')
os.makedirs(OUT, exist_ok=True)
TAG = 'wavetest-%d-%d' % (A.port, os.getpid())
CHITS = os.path.join(OUT, TAG + '.chits')
CSYMS = {}


def c_hooks():
    import glob
    nm = sorted(glob.glob(os.path.join(ROOT, 'toolchain/rustup/toolchains/*/lib/rustlib/*/bin/llvm-nm')))
    if not nm:
        return None
    r = subprocess.run([nm[0], '-C', A.app], capture_output=True, text=True)
    handed = None
    for l in r.stdout.splitlines():
        p = l.split(None, 2)
        if len(p) < 3:
            continue
        a = int(p[0], 16)
        if p[1] in 'tT' and 0x08080000 <= a < 0x080A0000:
            CSYMS[a & ~1] = p[2]
        elif p[2].startswith('dm40_fw::hy3131::HANDED'):
            handed = a
    if handed is None or not CSYMS:
        return None
    # The first call while handed is logged and the hook removed (a Python
    # hook per call of a busy function slows the emulator to a crawl).
    code = ("(machine.SystemBus.ReadByte(0x%x)!=0) and (open('%s','a').write('%%x 1'%%pc+chr(10)), self.RemoveHooksAt(pc))"
            % (handed, CHITS))
    path = os.path.join(OUT, TAG + '.script')
    with open(path, 'w') as f:
        for a in sorted(CSYMS):
            f.write('1 monitor sysbus.cpu AddHook 0x%08x "%s"\n' % (a, code))
    return path


HOOKS = None if A.no_region_c else c_hooks()
# A private copy of the SPI disk (the test records a CSV and saves a scope
# record): parallel runs share nothing (port + pid in every name).
DISK = os.path.join(OUT, TAG + '.disk.bin')
import shutil  # noqa: E402
shutil.copy(os.path.join(ROOT, 'firmware/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin'), DISK)
cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '3000', '--name', TAG,
       '--log', os.path.join(OUT, TAG + '.log'), '--disk', DISK, '--dmm', A.dmm] + (['--script', HOOKS] if HOOKS else [])
if not HOOKS:
    print('(no llvm-nm / symbols: the region C checks are skipped)')
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

    def c_reset():
        open(CHITS, 'w').close()

    def c_calls():
        """{function of region C: calls (at least)} while the ISR owned the bus."""
        got = {}
        if os.path.exists(CHITS):
            for l in open(CHITS):
                p = l.strip('\0').split()
                if len(p) == 2:
                    n = CSYMS.get(int(p[0], 16), p[0])
                    got[n] = max(got.get(n, 0), int(p[1]))
        return got

    def cat_names():
        return [x.strip('"').split(',')[0] for x in q('MMEM:CAT?').split(',"')[1:]]

    def get_file(name):
        data = b''
        while True:
            r = q('MMEM:DATA? "%s",%d' % (name, len(data)), raw=True)
            nd = int(r[1:2])
            n = int(r[2:2 + nd])
            data += r[2 + nd:2 + nd + n]
            if n < 6000:
                return data

    def vol_csv(dirname, dlg):
        """<dirname>/<dlg stem>.CSV read off the USB volume (MSC READ(10),
        64 sectors each, as a host) == tools/dlog.py of the DLG (MMEM:DATA?)."""
        import struct
        ref = os.path.join(OUT, '%s-%s' % (TAG, os.path.basename(dlg)))
        open(ref, 'wb').write(get_file(dlg))
        r = subprocess.run([sys.executable, '-I', os.path.join(ROOT, 'fw-rs/tools/dlog.py'), ref, '-o', ref + '.csv'],
                           capture_output=True)
        want = open(ref + '.csv', 'rb').read() if r.returncode == 0 else None
        m = uc.Msc(t.d)
        stem = os.path.basename(dlg).split('.')[0].upper().encode()
        name = stem + b' ' * (8 - len(stem)) + b'CSV'
        for _ in range(3):
            try:
                m.cmd(0, bytes(6))  # TEST UNIT READY (a media change: UNIT ATTENTION once)
            except IOError:
                pass
        b = m.read(0, 0, 1, 512)
        spc = b[13]
        rsvd, nfat, nroot = struct.unpack_from('<HBH', b, 14)
        fatsz = struct.unpack_from('<H', b, 22)[0]
        root = rsvd + nfat * fatsz
        data = root + nroot * 32 // 512
        lba = lambda c: data + (c - 2) * spc

        def find(secs, nm_):
            for s_ in secs:
                d = m.read(0, s_, 1, 512)
                for k in range(16):
                    e = d[k * 32:k * 32 + 32]
                    if e[0] == 0:
                        return None
                    if e[:11] == nm_:
                        return struct.unpack_from('<H', e, 26)[0], struct.unpack_from('<I', e, 28)[0]
            return None
        dirent = find(range(root, data), dirname.encode().ljust(11))
        fe = dirent and find(range(lba(dirent[0]), lba(dirent[0]) + spc), name)
        if not fe:
            return None, want
        start, size = fe
        got = b''
        while len(got) < size:
            k = min(64, (size - len(got) + 511) // 512)
            got += m.read(0, lba(start) + len(got) // 512, k, 512, retries=2)
        return got[:size], want

    def c_check(what, allowed=()):
        if not HOOKS:
            return
        got = c_calls()
        bad = {k: v for k, v in got.items() if not any(a in k for a in allowed)}
        print('  region C functions called while streaming: %s' % (', '.join(sorted(got)) or 'none'))
        check(not bad, '%s: no region C code while the stream ISR owns the bus%s'
              % (what, '' if not bad else ' (%s)' % ', '.join(sorted(bad))))

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

    def dcv_case():
        q('CONF:VOLT:DC')
        # The AC sine on DC V raises the sentinel's "AC voltage here: use
        # AC V?" hint toast (~1.5 s after the switch, shown 1.5 s,
        # sentinel.rs); a toast takes the next key: MF only after it went.
        wait_ms(4500)
        print('  DC V AUTO before: FUNC? %s READ? %s CONF? %s' % (q('FUNC?'), q('READ?'), q('CONF?')))
        q('DBG:KEY 3')
        wait_ms(600)
        q('DBG:TAP 237,259')
        wait_ms(6000)  # entry, the clip, the range step (a self-cal each)
        shot('d01-wave-dcv')
        check(ints(q('DBG:ISR?'))[1] == 1, 'the stream runs (DC V)')
        check(q('FUNC?').strip('"').upper() == 'VOLT', 'the DMM stays in DC V (%s)' % q('FUNC?'))
        vd = f('CHAN1:SCAL?')
        check(vd == 2.0, 'autorange on the clip: the 6 V range, 2 V/div (%g)' % vd)
        q('TIM:SCAL 0.02')
        wait_ms(2500)
        fr, avg, rms = f('SCOP:MEAS:FREQ?'), f('SCOP:MEAS:VAVG?'), f('SCOP:MEAS:VRMS?')
        print('  DC V wave: FREQ %.3f AVG %.4f RMS %.4f WAV:PRE? %s' % (fr, avg, rms, q('WAV:PRE?')))
        check(near(fr, 50.0, 0.03), 'DC row: 50 Hz (%.3f)' % fr)
        check(abs(avg - 0.2) < 0.02, 'DC-coupled: the mean = the 0.2 V DC (%.4f)' % avg)
        check(near(rms, (0.04 + 1.0) ** 0.5, 0.03), 'RMS of DC + AC (%.4f)' % rms)
        q('DBG:TAP 447,81')  # CH1 / coupling key: AC
        wait_ms(4000)
        shot('d02-wave-acv')
        avg = f('SCOP:MEAS:VAVG?')
        check(q('FUNC?').strip('"').upper() == 'VOLT:AC' and abs(avg) < 0.05,
              'coupling key: AC V, the mean ~0 (%s, %.4f)' % (q('FUNC?'), avg))
        check(f('CHAN1:SCAL?') == 2.0, 'AC at the same range (%g V/div)' % f('CHAN1:SCAL?'))
        q('DBG:TAP 447,81')  # back to DC
        wait_ms(4000)
        avg = f('SCOP:MEAS:VAVG?')
        check(q('FUNC?').strip('"').upper() == 'VOLT' and abs(avg - 0.2) < 0.02,
              'coupling key again: DC V, the mean 0.2 V (%s, %.4f)' % (q('FUNC?'), avg))
        q('DBG:KEY 0')
        wait_ms(3000)
        check(ints(q('DBG:ISR?'))[1] == 0, 'MENU: the stream stopped')
        v = f('READ?')
        conf = q('CONF?')
        print('  after: READ? %.4f CONF? %s' % (v, conf))
        check(abs(v - 0.2) < 0.01, 'DMM resumed in DC V: READ? %.4f' % v)

    print('*IDN?', q('*IDN?'))
    isr0 = ints(q('DBG:ISR?'))
    check(isr0[0] == 1, 'DBG:ISR? feature = 1 (an ad2-isr build)')
    if A.case == 'current':
        current_case()
        raise Done
    if A.case == 'dcv':
        dcv_case()
        raise Done
    q('CONF:VOLT:AC')
    wait_ms(4500)
    shot('w00-dmm')
    # A short DMM recording: LOGS/LOGnnn.DLG for the volume read below.
    q('LOG:CSV ON')
    wait_ms(4000)
    q('LOG:CSV OFF')
    wait_ms(2500)
    logs = sorted(n for n in cat_names() if n.upper().startswith('LOG') and n.upper().endswith('.DLG'))
    check(bool(logs), 'a DMM recording to read back later (%s)' % (logs[-1:] or 'none'))
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

    print('fast bands: 2 ms/div = 38400 S/s (DMA /16), 1 ms/div = 76800 S/s (hybrid frames, feasibility 11.12)')
    for tb, name, rate in [(0.002, '2ms', 38400), (0.001, '1ms', 76800)]:
        q('TIM:SCAL %g' % tb)
        wait_ms(2500)
        i0 = ints(q('DBG:ISR?'))
        wait_ms(4000)
        i1 = ints(q('DBG:ISR?'))
        fr, rms, vpp = meas()
        w1 = wave()
        wait_ms(300)
        w2 = wave()
        print('  %s/div: FREQ %.3f Hz, VRMS %.4f V, VPP %.4f V; DBG:ISR? %s' % (name, fr, rms, vpp, i1))
        check(abs(f('TIM:SCAL?') - tb) < tb * 0.01, 'TIM:SCAL? %s' % name)
        check(i1[1] == 1 and i1[14] == 1 and abs(i1[12] / 1000 - rate) < 5 and i1[19] == (1008 if rate > 38400 else 16) and i1[22] == 1,
              '%s/div: the stream at %d S/s, locked, div %d (DMA %d)' % (name, rate, i1[19], i1[22]))
        check(i1[4] == i0[4] and i1[8] == i0[8] and i1[9] == i0[9] and i1[16] == i0[16] and i1[7] == i0[7],
              '%s/div: no late entry, frame timeout, overrun, slip or bad sample in 4 s' % name)
        check(i1[6] == i0[6], '%s/div: the view drained the ring in time (overwritten %d -> %d)' % (name, i0[6], i1[6]))
        check(w1 != w2, '%s/div: the trace updates' % name)
        if tb == 0.002:
            # 20 ms on screen = one period of the 50 Hz sine.
            check(near(rms, 1.0, 0.03) and near(vpp, 2.828, 0.04), '%s/div: 1 V rms, 2.83 Vpp over one period' % name)
        else:
            check(0.5 < vpp < 2.95, '%s/div: half a period on screen, VPP %.3f V <= 2.83' % (name, vpp))
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
    # CONF? = the streamed row's range, not the reading from before the view
    # (device f3aea2f: "VOLT:AC +6" on 200 mV/div).
    c = q('CONF?')
    check(c == '"VOLT:AC +6.000000E+01,+1.000000E-03"', 'CONF? on 20 V/div = AC V 60 V (%s)' % c)
    shot('w07-range-up')
    q('DBG:TAP 447,40')
    wait_ms(2500)
    check(f('CHAN1:SCAL?') == vd0, 'V/div - back (%g)' % f('CHAN1:SCAL?'))
    q('DBG:TAP 447,40')
    wait_ms(2500)
    c = q('CONF?')
    check(f('CHAN1:SCAL?') == 0.2 and c == '"VOLT:AC +6.000000E-01,+1.000000E-05"',
          'V/div - again: 200 mV/div, CONF? = AC mV 600 mV (%s)' % c)
    q('DBG:TAP 447,122')
    wait_ms(2500)
    c = q('CONF?')
    check(f('CHAN1:SCAL?') == vd0 and c == '"VOLT:AC +6.000000E+00,+1.000000E-04"',
          'V/div + back: CONF? = AC V 6 V (%s)' % c)

    print('STOP / RUN')
    q('STOP')
    wait_ms(800)
    shot('w08-stopped')
    q('RUN')
    wait_ms(1500)
    fr, rms, vpp = meas()
    check(near(fr, 50.0, 0.03), 'RUN again: 50 Hz (%.3f)' % fr)

    print('Save CSV in the wave view (region C while the stream ISR runs)')
    q('STOP')
    wait_ms(800)
    before = set(cat_names())
    c_reset()
    q('DBG:KEY 3')  # MF
    wait_ms(800)
    q('DBG:TAP 367,40')  # Settings
    wait_ms(800)
    shot('w08b-mf-settings')
    q('DBG:TAP 154,93')  # Save CSV
    new = []
    for _ in range(30):
        wait_ms(1000)
        new = sorted(n for n in set(cat_names()) - before if n.upper().startswith('SCOP') and n.upper().endswith('.DLG'))
        if new:
            break
    check(bool(new), 'Save CSV wrote %s' % (new or 'nothing'))
    wait_ms(3000)  # the volume refresh would run 1 s after the disk went quiet
    check(ints(q('DBG:ISR?'))[1] == 1, 'the stream still runs after the save')
    # Only the tap's own one-shot (the Save CSV handler, scope_csv::set, the
    # MF popup's press feedback): the DLG write (screenshot.rs job) runs with
    # the stream paused, the volume refresh after the stream stops (main.rs).
    c_check('STOP + Save CSV', ('sp_save_csv', 'scope_csv::', 'press_track', 'sp_popup'))
    q('RUN')
    wait_ms(1500)

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
    print('host reads CSVs off the USB volume while the wave view streams')
    for dirname, dlg in [('SCOPE', new[-1] if new else None), ('LOGS', logs[-1] if logs else None)]:
        if not dlg:
            continue
        c_reset()
        got, want = vol_csv(dirname, dlg)
        stem = os.path.basename(dlg).split('.')[0]
        check(got is not None and got == want, '%s/%s.CSV off the volume == tools/dlog.py (%s bytes)'
              % (dirname, stem, len(got) if got else None))
        check(ints(q('DBG:ISR?'))[1] == 1, 'the stream runs during the read')
        c_check('a host reading %s/%s.CSV' % (dirname, stem))
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
    for p in (DISK, HOOKS, CHITS):
        if p and os.path.exists(p):
            os.remove(p)

print('wavetest: %s' % ('PASS' if not FAIL else 'FAIL: ' + '; '.join(FAIL)))
sys.exit(1 if FAIL else 0)
