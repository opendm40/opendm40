#!/usr/bin/env python3
"""Settings > Other > Reset settings (firmware/src/app.rs, persist::reset_record).

  cd firmware && ./build.sh
  emu/settingsresettest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

One emulator run, driven like a user (DBG:KEY / DBG:TAP): settings changed over
SCPI and saved; No keeps them; Yes restarts the firmware; afterwards the
settings are the defaults and the function DC V, the thermocouple offset and
the NTC values too; a setting changed then
survives a further DBG:RESET, so the request is gone from the record.
Exit status 0 = PASS. Private disk copy; USB/IP port in 63900-63999 unless
--port.
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
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
if A.port is None:
    A.port = run.free_port(63900, 63999)
print('USB/IP port %d' % A.port, flush=True)
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

SERIAL = run.emu_uid(A.port).hex().upper()
TAG = 'setreset-%d-%d' % (A.port, os.getpid())
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
                self.t = uc.Usbtmc(self.d)
                self.t.write('*IDN?')
                self.t.read(timeout=20)
                return
            except Exception:
                time.sleep(2)
        raise RuntimeError('no USB/IP device on port %d' % A.port)

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


def state(l):
    return {
        'vol': l.q('SYST:BEEP:VOL?'),
        'alarm': l.q('SYST:ALER:ALAR?'),
        'func': l.q('FUNC?'),
        'toffs': l.q('TEMP:OFFS?'),
        'ntc': l.q('TEMP:NTC?'),
    }


def open_row(l):
    """Settings > Other, the Reset settings row: the confirm."""
    for cmd, want in [('DBG:KEY 0', None), ('DBG:TAP 341,221', 'Settings,1'), ('DBG:TAP 60,291', 'Settings,6')]:
        if want is None:
            for _ in range(4):
                if l.q('DBG:VIEW?').startswith('Main menu'):
                    break
                l.w(cmd)
                time.sleep(1)
            continue
        l.w(cmd)
        time.sleep(1)
        check(l.q('DBG:VIEW?').startswith(want), 'view %s' % want)
    l.w('DBG:TAP 300,291')
    time.sleep(1)


def main():
    src = os.path.join(ROOT, 'stock/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin')
    disk = os.path.join(OUT, TAG + '-disk.bin')
    os.makedirs(OUT, exist_ok=True)
    shutil.copy(src, disk)
    log = open(os.path.join(OUT, TAG + '.out'), 'w')
    emu = subprocess.Popen([sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--app', A.app, '--fast', '--usbip', '0.01', '--run-for', '3000',
                            '--usbip-port', str(A.port), '--uid', SERIAL, '--name', TAG, '--disk', disk],
                           cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    try:
        l = Link()
        d0 = state(l)
        print('  defaults', d0, flush=True)
        l.w('TEMP:OFFS 1.5')
        l.w('TEMP:NTC 12345,3456')
        l.w('SYST:BEEP:VOL 30')
        l.w('SYST:ALER:ALAR VOLUME')
        l.w('FUNC "VOLT:AC"')
        time.sleep(3)
        s1 = state(l)
        check(s1 != d0, 'settings changed %s' % s1)
        open_row(l)
        l.w('DBG:TAP 170,204')  # No
        time.sleep(3)
        check(state(l) == s1, 'No keeps them')
        open_row(l)
        l.w('DBG:TAP 310,204')  # Yes
        l.close()
        time.sleep(5)
        l = Link()
        s2 = state(l)
        check(s2 == d0, 'after Yes the defaults %s' % s2)
        check(l.q('DBG:VIEW?').startswith('Multimeter'), 'on the DMM page')
        l.w('SYST:BEEP:VOL 40')
        time.sleep(4)
        l.w('DBG:RESET')
        l.close()
        time.sleep(5)
        l = Link()
        check(l.q('SYST:BEEP:VOL?') == '40', 'the request is gone: a later change survives a reset')
        l.close()
    finally:
        try:
            os.killpg(emu.pid, 15)
        except Exception:
            pass
        emu.wait()
    print('PASS' if not FAIL else 'FAIL: ' + '; '.join(FAIL), flush=True)
    return 1 if FAIL else 0


sys.exit(main())
