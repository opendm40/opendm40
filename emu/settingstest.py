#!/usr/bin/env python3
"""opendm40 settings across a power-off (firmware/src/setfile.rs: the copy in
_SETTING.BIN on the SPI disk; the backup SRAM is not powered while the meter
is off, device 2026-10-24).

  cd firmware && ./build.sh
  emu/settingstest.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

The power cycle: two emulator runs. The first one's SPI flash is saved at
its end (run.py --save-disk) and is the second one's disk; the second run
is a new machine, so its backup SRAM starts zeroed and RCU_RSTSCK holds the
power-on flags, as after a real power-off.

Cases (each on a fresh copy of the MSC dump: no _SETTING.BIN yet):
  timed  DBG:BOOT? SETFILE,CREATED and SETTINGS,DEFAULTS on the first boot;
         SYST:ALER:ALAR VOLUME, then DBG:BOOT? SAVE,OK within ~10 s (5 s
         debounce after the 2 s backup SRAM save); the run ends without a
         power-off (a battery pulled). Second boot: SETTINGS,FILE,
         SETFILE,PRESENT, SYST:ALER:ALAR? VOLUME.
  key    SYST:ALER:ALAR OFF at uptime ~29 s, the power key held at virtual
         30..32 s (run.py power-hold: off at the release), so no timed save
         can come first (DBG:BOOT? just before: no save yet). Second boot:
         SETTINGS,FILE, SYST:ALER:ALAR? OFF.
Exit status 0 = PASS. Per-process names, private disk copies; USB/IP port in
63900-63999 unless --port.
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
ap.add_argument('--cases', default='timed,key')
ap.add_argument('--run1', type=float, default=45.0, help='virtual seconds of the first run (default 45)')
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
if A.port is None:
    A.port = run.free_port(63900, 63999)
print('USB/IP port %d' % A.port, flush=True)
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

SERIAL = run.emu_uid(A.port).hex().upper()
TAG = 'settings-%d-%d' % (A.port, os.getpid())
FAIL = []
KEY_AT = 30.0


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


def settings(boot):
    """The SETTINGS.. fields of a DBG:BOOT? reply as a list."""
    p = boot.split(',')
    i = p.index('SETTINGS')
    return p[i:]


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


def case(name):
    print(name, flush=True)
    src = os.path.join(ROOT, 'stock/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin')
    disk0 = os.path.join(OUT, '%s-%s-disk0.bin' % (TAG, name))
    disk1 = os.path.join(OUT, '%s-%s-disk1.bin' % (TAG, name))
    shutil.copy(src, disk0)
    want = {'timed': 'VOLUME', 'key': 'OFF'}[name]
    extra = ['--run-for', str(A.run1), '--save-disk', disk1]
    if name == 'key':
        extra += ['--key', 'power-hold@%g' % KEY_AT]
    emu = start('%s-%s-1' % (TAG, name), disk0, extra)
    try:
        l = Link()
        b = l.q('DBG:BOOT?')
        s = settings(b)
        print('  boot 1: %s' % ','.join(s), flush=True)
        check(s[1] == 'DEFAULTS' and 'CREATED' in s, 'first boot: defaults, the file made')
        before = l.q('SYST:ALER:ALAR?')
        check(before != want, 'setting before: %s' % before)
        if name == 'timed':
            l.w('SYST:ALER:ALAR %s' % want)
            t0 = time.time()
            ok = False
            for _ in range(120):
                s = settings(l.q('DBG:BOOT?'))
                if s[s.index('SAVE') + 1] == 'OK':
                    ok = True
                    break
                time.sleep(1)
            up = l.q('DBG:PING?').split(',')[0]
            check(ok, 'DBG:BOOT? SAVE,OK after the change (%s; %.0f s wall, uptime %s ms)' % (','.join(s), time.time() - t0, up))
        else:
            while int(l.q('DBG:PING?').split(',')[0]) < (KEY_AT - 1.0) * 1000:
                time.sleep(0.1)
            l.w('SYST:ALER:ALAR %s' % want)
            s = settings(l.q('DBG:BOOT?'))
            up = l.q('DBG:PING?').split(',')[0]
            check(s[s.index('SAVE') + 2] == '0', 'no save yet right before the key (%s, uptime %s ms)' % (','.join(s), up))
        check(l.q('SYST:ALER:ALAR?') == want, 'setting now %s' % want)
        l.close()
        # The run's end saves the SPI flash (--save-disk).
        emu.wait(timeout=3600)
    except Exception as e:
        check(False, '%s run 1: %r' % (name, e))
        stop(emu)
        return
    check(os.path.exists(disk1), 'disk saved at the end of run 1')
    if not os.path.exists(disk1):
        return
    emu = start('%s-%s-2' % (TAG, name), disk1, ['--run-for', '3000'])
    try:
        l = Link()
        b = l.q('DBG:BOOT?')
        s = settings(b)
        print('  boot 2: %s' % ','.join(s), flush=True)
        check(s[1] == 'FILE' and 'PRESENT' in s, 'second boot: settings from the file')
        got = l.q('SYST:ALER:ALAR?')
        check(got == want, 'second boot: SYST:ALER:ALAR? %s (want %s)' % (got, want))
        l.close()
    except Exception as e:
        check(False, '%s run 2: %r' % (name, e))
    finally:
        stop(emu)
    for f in (disk0, disk1):
        try:
            os.remove(f)
        except OSError:
            pass


os.makedirs(OUT, exist_ok=True)
for c in A.cases.split(','):
    case(c)
print('FAIL: %d checks' % len(FAIL) if FAIL else 'PASS')
sys.exit(1 if FAIL else 0)
