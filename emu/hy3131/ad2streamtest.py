#!/usr/bin/env python3
"""fw-rs AD2 stream ISR (feature ad2-isr, docs/decisions-2026-10-22.md #3) in
the emulator: TIMER6 (TIM7, GD32_Timer in dm40.repl) -> IRQ 55 -> the ISR's
SPI2 reads through HY3131SpiPort; SCPI over USB/IP on one connection.

  cd fw-rs && ./build.sh --features ad2-isr
  emu/hy3131/ad2streamtest.py --app fw-rs/target/native/thumbv7em-none-eabihf/release/dm40-fw

Checks (AC V, 0.5 V 50 Hz on V/COM; 200 MIPS = the device's instruction
rate, so DBG:AD2:DRAW 2 costs the main loop what it does on the meter):
  * DBG:ISR? feature = 1;
  * per session (--sessions, default 4800 S/s DRAW 2, 9600 S/s DRAW 1, 1200
    S/s DRAW 2): every record drained, seqs consecutive, no stamp gap
    (round(dt / P) == 1), LOG:COUNt? overwritten = missed = lost = 0,
    LOG:GAPs? all 0, DBG:ISR? overruns = timeouts = forced = late = slips =
    0, locked, ~1 probe in 4 periods, the locked rate = the chip's (--ppm:
    the HY3131 crystal offset of the model; --race-us its INTF race),
    *IDN? answered within 0.5 s while streaming, no SYST:ERR?, READ? back to
    0.5 V after LOG:STATe OFF;
  * a function change mid-stream (CONF:VOLT:DC): the stream stops first
    (DBG:ISR? forced + 1, LOG:STATe? 0, "Settings conflict"), DC V reads ~0,
    AC V 0.5 V again, DBG:HYSPI:SELF? 200 clean.
Exit status 0 = all passed. The emulator's SPI is instantaneous: the ISR
times and the CPU share are the device's job (fw-rs TODO device check).
"""
import argparse
import os
import struct
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='fw-rs native ELF / image built with --features ad2-isr')
ap.add_argument('--port', type=int, default=3378, help='USB/IP port (default 3378)')
ap.add_argument('--secs', type=float, default=1.0, help='scale of the session lengths (wall clock)')
ap.add_argument('--ppm', type=float, default=0.0, help='HY3131 crystal offset vs the MCU, ppm (model ClockPpm)')
ap.add_argument('--extra-script', metavar='FILE', help='event-script lines (run.py --script) appended to the generated one, e.g. Renode hooks (fw-rs region C audit)')
ap.add_argument('--race-us', type=float, default=None, help='INTF read-clears-flag race window, us (model default 0.7)')
ap.add_argument('--sessions', default='4800:2:20,9600:1:15,1200:2:10',
                help='rate:draw:secs[:dma],... (secs scaled by --secs; dma = DBG:AD2:DMA 0 / 1 sent before the '
                     'session; without it nothing is sent: the firmware default (DMA frames) or the last one sent)')
A = ap.parse_args()
SCRIPT = os.path.join(EMU, 'out', 'ad2stream-%d.script' % A.port)
with open(SCRIPT, 'w') as f:
    f.write('0 monitor sysbus.hy3131 ClockPpm %g\n' % A.ppm)
    for k in range(1, 40):
        f.write('%d monitor sysbus.hy3131 IntfPhaseHist\n' % (k * 3))
    if A.race_us is not None:
        f.write('0 monitor sysbus.hy3131 IntfRaceUs %g\n' % A.race_us)
    if A.extra_script:
        f.write(open(A.extra_script).read())
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what))
    if not ok:
        FAIL.append(what)


cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--mips', '200', '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '3000', '--name', 'ad2stream',
       '--dmm', 'vsrc:0,ac=0.5,f=50', '--script', SCRIPT]
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
        sys.exit('ad2streamtest: no USB/IP device on port %d' % A.port)

    def q(line, raw=False, timeout=60):
        t.write(line)
        if '?' not in line:
            return None
        b = t.read(maxlen=8192, timeout=timeout)
        return b if raw else b.decode(errors='replace').strip()

    def ints(s):
        return [int(x, 0) for x in s.split(',')]

    def read_v(n=3):
        v = None
        for _ in range(n):
            v = float(q('READ?'))
        return v

    print('*IDN?', q('*IDN?'))
    isr0 = ints(q('DBG:ISR?'))
    check(isr0[0] == 1, 'DBG:ISR? feature = 1 (an ad2-isr build)')
    q('CONF:VOLT:AC')
    t_ac = time.time()
    v = read_v()
    while abs(v - 0.5) >= 0.005 and time.time() - t_ac < 20:  # the AC node settles (tau 0.3 s virtual)
        time.sleep(1)
        v = read_v()
    check(abs(v - 0.5) < 0.005, 'AC V reads 0.5 V before streaming (%.5f)' % v)

    dma_cur = 1  # the firmware default (decision 2026-10-22 #7: DMA frames)
    for rate, draw, secs, dma in [tuple(float(x) if i == 2 else int(x) for i, x in enumerate((v + ':-1').split(':')[:4]))
                                  for v in A.sessions.split(',')]:
        if dma >= 0:
            q('DBG:AD2:DMA %d' % dma)
            dma_cur = dma
        print('session %d S/s, DBG:AD2:DRAW %d, DBG:AD2:DMA %d%s, %g s'
              % (rate, draw, dma_cur, '' if dma >= 0 else ' (not sent)', secs * A.secs))
        dma = dma_cur
        q('LOG:STATe OFF')
        q('*CLS')
        q('DBG:AD2:DRAW %d' % draw)
        q('LOG:SOURce AD2')
        q('LOG:SRATe %d' % rate)
        q('LOG:STATe ON')
        pre = q('LOG:PREamble?').split(',')
        period = 1e9 / int(pre[8])  # us
        recs, last, seqgap, tsp, stampgap, idn = 0, 0, 0, None, 0, []
        t0 = time.time()
        nidn = t0 + 1

        def drain():
            global recs, last, seqgap, tsp, stampgap
            b = q('LOG:DATA?', raw=True)
            n = int(b[2:6])
            for o in range(6, 6 + n, 12):
                seq, ts, _ = struct.unpack_from('<IIi', b, o)
                seqgap += last != 0 and seq != last + 1
                last = seq
                if tsp is not None and round(((ts - tsp) & 0xFFFFFFFF) / period) != 1:
                    stampgap += 1
                tsp = ts
                recs += 1
            return n

        while time.time() - t0 < secs * A.secs:
            drain()
            if time.time() >= nidn:
                a = time.time()
                q('*IDN?')
                idn.append(time.time() - a)
                nidn = time.time() + 1
        q('LOG:STATe OFF')
        while drain():
            pass
        cnt = ints(q('LOG:COUNt?'))
        gaps = ints(q('LOG:GAPs?'))
        isr = ints(q('DBG:ISR?'))
        err = q('SYST:ERR?')
        print('  records %d, LOG:COUNt? %s, LOG:GAPs? %s, DBG:ISR? %s, *IDN? max %.3f s'
              % (recs, cnt, gaps, isr, max(idn or [0])))
        check(recs == cnt[1] and recs > 1000, 'every captured record drained (%d)' % recs)
        check(seqgap == 0 and stampgap == 0, 'seqs consecutive, no stamp gap (%d / %d)' % (seqgap, stampgap))
        check(cnt[2:] == [0, 0, 0], 'overwritten / missed / lost = 0')
        check(gaps[0] == 0 and gaps[2:] == [0] * 5, 'LOG:GAPs? bad and every bucket 0')
        check(isr[1] == 0 and isr[8:10] == [0, 0] and isr[13] == isr0[13], 'ISR stopped, timeouts / overruns 0, no forced stop')
        check(isr[14] == 1, 'phase lock reached (locked %d, max run %d, edges after B %d of %d probes)'
              % (isr[14], isr[17], isr[18], isr[5]))
        check(isr[4] == 0 and isr[16] == 0, 'no late entry, no slip (late %d, slips %d)' % (isr[4], isr[16]))
        bents = isr[2] - isr[3] - isr[4] - isr[7]
        if isr[22]:
            # DMA frames: B and C per probe, 1 in 8 periods; acquisition
            # periods (no sample) are A entries too.
            check(isr[5] > isr[3] / 40, 'DMA frames: probes ~ 1 in 8 periods (%d probes, %d samples)' % (isr[5], isr[3]))
        else:
            check(isr[5] > isr[3] / 8 and 0 <= bents - isr[5] <= 2 + isr[5] // 20,
                  'probes ~ 1 in 4 periods, entries = A + B (%d probes, %d B entries)' % (isr[5], bents))
        check(isr[22] == (1 if dma == 1 else 0) and isr[19] == (16 if dma == 1 else 1008),
              'DMA frames %d (DBG:AD2:DMA %d), div %d (1008 = /8 + CKPH data phase)' % (isr[22], dma, isr[19]))
        check(abs(isr[15] + 10 * A.ppm) <= 250, 'locked period vs nominal %+.1f ppm (chip %+g ppm)' % (isr[15] / 10, A.ppm))
        check(isr[12] > 0 and abs(isr[12] / 1000 / rate - 1 - A.ppm * 1e-6) < 2.5e-5, 'locked rate %.3f S/s' % (isr[12] / 1000))
        check(max(idn or [9]) < 0.5, '*IDN? answered while streaming (max %.3f s)' % max(idn or [9]))
        check(err.startswith('0,'), 'SYST:ERR? empty (%s)' % err)
        time.sleep(1.5)
        v = read_v()
        check(abs(v - 0.5) < 0.005, 'DMM reads 0.5 V AC again after the stream (%.5f)' % v)

    print('function change mid-stream')
    q('DBG:AD2:DRAW 1')
    q('LOG:SRATe 4800')
    q('LOG:STATe ON')
    time.sleep(1)
    f0 = ints(q('DBG:ISR?'))[13]
    q('CONF:VOLT:DC')
    st = q('LOG:STATe?')
    err = q('SYST:ERR?')
    isr = ints(q('DBG:ISR?'))
    check(st == '0' and 'Settings conflict' in err and isr[13] == f0 + 1 and isr[1] == 0,
          'stream stopped before the bus access (STATe? %s, %s, forced %d -> %d)' % (st, err, f0, isr[13]))
    time.sleep(2)
    check(abs(read_v()) < 0.01, 'DC V reads ~0 (the 50 Hz sine)')
    q('CONF:VOLT:AC')
    time.sleep(2)
    check(abs(read_v() - 0.5) < 0.005, 'AC V reads 0.5 V again')
    self_ = q('DBG:HYSPI:SELF? 200').split(',')
    check(self_[2:6] == ['0', '0', '0', '0'], 'DBG:HYSPI:SELF? 200 clean (%s)' % ','.join(self_[:6]))
    for mode in range(4):
        wr = q('DBG:HYSPI:WR? %d' % mode).split(',')
        check(len(wr) >= 7 and wr[2:4] == ['0', '0'] and wr[5] == wr[6],
              'DBG:HYSPI:WR? %d (the model takes every write path; the device tells them apart): %s' % (mode, ','.join(wr[:8])))
finally:
    os.killpg(emu.pid, 15)
    emu.wait()

print('ad2streamtest: %s' % ('PASS' if not FAIL else 'FAIL: ' + '; '.join(FAIL)))
sys.exit(1 if FAIL else 0)
