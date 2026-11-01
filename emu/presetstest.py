#!/usr/bin/env python3
"""opendm40 presets (firmware/src/presets.rs, app_presets.rs): *SAV / *RCL,
MEMory:STATe:NAME, the Presets page, and the presets across a power-off.

  cd firmware && ./build.sh
  emu/presetstest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

Run 1 (a fresh copy of the MSC dump: no _PRESETS.BIN yet): preset 2 empty;
resistance, RATE FAST, scope 1 ms/div, generator 2 kHz with the output ON,
*SAV 2 and a name; then other settings and *RCL 2: the saved ones back, the
output OFF; *RCL of an empty preset is -221. The Presets page from the main
menu: a tap on preset 2 and on Recall goes back to the multimeter.
Run 2 (the disk of run 1, a new machine): the name and the preset are there.
Exit status 0 = PASS. USB/IP port in 63900-63999 unless --port.
"""
import argparse
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
OUT = os.path.join(HERE, 'out')
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--port', type=int, default=None, help='USB/IP port (default: a free one in 63900-63999)')
ap.add_argument('--run1', type=float, default=40.0, help='virtual seconds of the first run (default 40)')
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
if A.port is None:
    A.port = run.free_port(63900, 63999)
print('USB/IP port %d' % A.port, flush=True)
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

SERIAL = run.emu_uid(A.port).hex().upper()
TAG = 'presets-%d-%d' % (A.port, os.getpid())
FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


class Link:
    def __init__(self):
        for _ in range(150):
            try:
                self.d = uc.Dev('1-0')
                break
            except Exception:
                time.sleep(2)
        else:
            raise RuntimeError('no USB/IP device on port %d' % A.port)
        self.t = uc.Usbtmc(self.d)

    def q(self, s, timeout=30):
        self.t.write(s)
        return self.t.read(timeout=timeout).decode(errors='replace').strip()

    def w(self, s):
        self.t.write(s)

    def close(self):
        try:
            self.d.close()
        except Exception:
            pass


def start(name, disk, extra):
    log = open(os.path.join(OUT, name + '.out'), 'w')
    return subprocess.Popen([sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01',
                             '--usbip-port', str(A.port), '--uid', SERIAL, '--name', name, '--disk', disk] + extra,
                            cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)


def stop(emu):
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    emu.wait()


def view(l):
    return l.q('DBG:VIEW?').split(',')[0]


def wait_view(l, want):
    for _ in range(50):
        if view(l) == want:
            return True
        time.sleep(0.2)
    return False


def state(l):
    return (l.q('FUNC?'), l.q('RATE?'), float(l.q('TIM:SCAL?')), float(l.q('SOUR:FREQ?')), l.q('OUTP?'))


os.makedirs(OUT, exist_ok=True)
src = os.path.join(ROOT, 'stock/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin')
disk0 = os.path.join(OUT, '%s-disk0.bin' % TAG)
disk1 = os.path.join(OUT, '%s-disk1.bin' % TAG)
shutil.copy(src, disk0)
emu = start(TAG + '-1', disk0, ['--run-for', str(A.run1), '--save-disk', disk1])
try:
    l = Link()
    check(l.q('MEM:STAT:VAL? 2') == '0', 'first boot: preset 2 empty')
    check(l.q('MEM:STAT:NAME? 2') == '""', 'first boot: no name')
    for c in ('CONF:RES', 'RATE FAST', 'TIM:SCAL 1E-3', 'SOUR:FREQ 2000', 'OUTP ON', '*SAV 2', 'MEM:STAT:NAME 2,"Bench res"'):
        l.w(c)
    saved = state(l)
    print('  saved: %s' % (saved,), flush=True)
    check(l.q('SYST:ERR?').startswith('0,'), 'no error after *SAV / NAME')
    check(l.q('MEM:STAT:VAL? 2') == '1', 'preset 2 holds a state')
    check(l.q('MEM:STAT:NAME? 2') == '"Bench res"', 'the name, its case kept')
    for c in ('CONF:VOLT:AC', 'RATE SLOW', 'TIM:SCAL 1E-5', 'SOUR:FREQ 500', 'OUTP ON'):
        l.w(c)
    print('  changed: %s' % (state(l),), flush=True)
    l.w('*RCL 2')
    got = state(l)
    print('  recalled: %s' % (got,), flush=True)
    check(got[:4] == saved[:4], 'recall: function, rate, timebase, frequency as saved')
    check(got[4] == '0', 'recall: the generator output OFF')
    check(view(l) == 'Multimeter', 'recall: the page it was saved on')
    check(l.q('SYST:ERR?').startswith('0,'), 'no error after *RCL')
    l.w('*RCL 3')
    check(l.q('SYST:ERR?').startswith('-221,'), '*RCL of an empty preset: -221')
    # The page: MENU, the top row's right half, preset 2, Recall.
    l.w('CONF:VOLT:DC')
    l.w('DBG:KEY 0')
    check(wait_view(l, 'Main menu'), 'MENU: the main menu')
    l.w('DBG:TAP 340,27')
    check(wait_view(l, 'Presets'), 'the top row\'s right half: Presets')
    l.w('DBG:TAP 240,110')
    time.sleep(1)
    l.w('DBG:TAP 180,284')
    check(wait_view(l, 'Multimeter'), 'preset 2, Recall: back on the multimeter')
    check(l.q('FUNC?') == saved[0], 'recalled from the page: %s' % saved[0])
    l.close()
    emu.wait(timeout=3600)
except Exception as e:
    check(False, 'run 1: %r' % (e,))
    stop(emu)
if os.path.exists(disk1):
    emu = start(TAG + '-2', disk1, ['--run-for', '3000'])
    try:
        l = Link()
        check(l.q('MEM:STAT:NAME? 2') == '"Bench res"', 'after a power-off: the name')
        check(l.q('MEM:STAT:VAL? 2') == '1' and l.q('MEM:STAT:VAL? 1') == '0', 'after a power-off: preset 2 only')
        l.w('*RCL 2')
        check(l.q('FUNC?') == saved[0] and l.q('RATE?') == saved[1], 'after a power-off: *RCL 2')
        l.w('MEM:STAT:DEL 2')
        check(l.q('MEM:STAT:VAL? 2') == '0', 'MEM:STAT:DEL 2: empty')
        l.close()
    except Exception as e:
        check(False, 'run 2: %r' % (e,))
    finally:
        stop(emu)
else:
    check(False, 'disk saved at the end of run 1')
for f in (disk0, disk1):
    try:
        os.remove(f)
    except OSError:
        pass
print('FAIL: %d checks' % len(FAIL) if FAIL else 'PASS')
sys.exit(1 if FAIL else 0)
