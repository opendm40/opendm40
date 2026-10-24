#!/usr/bin/env python3
"""fw-rs: the DMM Wave view's ring drops (DBG:ISR? field 6 `dropped`: the
logger ring was full, the newest stream samples dropped; device 612c45c M3:
~50/s at 1 ms/div). The view is entered as on the device (DC V, MF > Wave:
DBG:KEY 3, DBG:TAP 237,259), set to --tb s/div, then watched for --dur s
of device time: DBG:ISR? (dropped, late, overruns, max_lat) and DBG:PROF?
(the main loop's longest pass and section maxima) every --step s. Optional
--host-read: a host reads INFO/LOADER.BIN (all of it from the SPI-disk
mirror) back to back meanwhile.

  emu/wavedroptest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 --mips 110 --isr-stall-ns 700

Exit status 0 = no sample dropped after the settle time (--settle s after
TIM:SCAL), else 1.
"""
import argparse
import os
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True)
ap.add_argument('--port', type=int, default=None, help='USB/IP port (default: run.free_port, DM40_EMU_PORTS)')
ap.add_argument('--mips', type=int, default=110)
ap.add_argument('--isr-stall-ns', type=int, default=700)
ap.add_argument('--tb', default='0.001', help='TIM:SCAL, s/div')
ap.add_argument('--dur', type=float, default=30, help='device seconds watched')
ap.add_argument('--step', type=float, default=5, help='device seconds between the polls')
ap.add_argument('--settle', type=float, default=3, help='device seconds after TIM:SCAL before the watch')
ap.add_argument('--host-read', action='store_true', help='INFO/LOADER.BIN read back to back during the watch')
ap.add_argument('--dmm', default='vsrc:0.2,ac=0.5,f=50')
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
if A.port is None:
    A.port = run.free_port()
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402
import infovol  # noqa: E402

script = os.path.join(HERE, 'out', 'wavedrop-%d.script' % A.port)
with open(script, 'w') as f:
    if A.isr_stall_ns:
        f.write('0 monitor sysbus.cpucost IsrStallNs %d\n' % A.isr_stall_ns)
    f.write('0 monitor sysbus.spi2.spiflash ProgramUs 400\n')
    f.write('0 monitor sysbus.spi2.spiflash Erase4kUs 45000\n')
emu = subprocess.Popen([sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', str(A.mips), '--app', A.app,
                        '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '5000', '--name', 'wavedrop-%d' % A.port,
                        '--dmm', A.dmm, '--script', script],
                       cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
rc = 1
try:
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    else:
        sys.exit('wavedroptest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=120):
        t.write(line)
        if '?' not in line:
            return None
        return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000

    def wait_dev(s):
        t0 = up()
        while up() - t0 < s:
            time.sleep(0.2)

    def isr():
        r = q('DBG:ISR?').split(',')
        return dict(active=int(r[1]), dropped=int(r[6]), late=int(r[4]), over=int(r[9]), max_lat=int(r[11]))

    def prof():
        p = q('DBG:PROF?')
        m = re.search(r'max (\d+) max1s', p)
        return p, int(m.group(1)) if m else -1

    print('*IDN?', q('*IDN?'), flush=True)
    q('CONF:VOLT:DC')
    wait_dev(2)
    for cmd, want in (('DBG:KEY 3', 'Range'), ('DBG:TAP 237,259', 'Oscilloscope')):
        q(cmd)
        wait_dev(1.5)
        v = q('DBG:VIEW?')
        print(cmd, '->', v, flush=True)
        if not v.startswith(want):
            sys.exit('unexpected view %r' % v)
    q('TIM:SCAL %s' % A.tb)
    wait_dev(A.settle)
    print('TIM:SCAL?', q('TIM:SCAL?'), 'ACQ:SRAT?', q('ACQ:SRAT?'), flush=True)
    m = uc.Msc(uc.Dev('1-0')) if A.host_read else None
    lba = size = 0
    if m:
        lba, size = infovol.vol_dir(m, 'INFO')['LOADER.BIN']
    q('DBG:PROF:RESET')
    a = isr()
    t0 = up()
    rows = []
    last = t0
    off = 0
    nread = 0
    while True:
        if m:
            n = min(240, (size - off + 511) // 512)
            infovol.read(m, lba + off // 512, n)
            nread += n * 512
            off = 0 if off + n * 512 >= size else off + n * 512
        now = up()
        if now - last >= A.step or now - t0 >= A.dur:
            last = now
            b = isr()
            p, mx = prof()
            rows.append((now - t0, b['dropped'] - a['dropped'], b['late'], b['over'], b['max_lat'], mx))
            print('t %5.1f s  dropped +%d  late %d  overruns %d  max_lat %d ns  loop max %d us  active %d  host %d B'
                  % (rows[-1] + (b['active'], nread)), flush=True)
            if now - t0 >= A.dur:
                break
        if not m:
            time.sleep(0.3)
    print(p, flush=True)
    d = rows[-1][1]
    el = rows[-1][0]
    print('dropped +%d in %.1f s (%.1f/s)' % (d, el, d / max(el, 0.1)), flush=True)
    rc = 0 if d == 0 else 1
    q('TIM:SCAL 0.01')
finally:
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    emu.wait()
print('PASS' if rc == 0 else 'FAIL')
sys.exit(rc)
