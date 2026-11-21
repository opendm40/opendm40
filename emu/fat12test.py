#!/usr/bin/env python3
"""The newer units' FAT12 SPI disk made the DM40 FAT16 one at boot
(firmware/src/fat16conv.rs), in the emulator.

  cd firmware && ./build.sh
  emu/fat12test.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40

The disk: a FAT12 volume as a BY25Q128ES unit's (mkfs.fat: 4096 B sectors,
2 per cluster, 2 FATs of 1 sector, 512 root entries, no hidden sectors),
with a "System Volume Information" directory at cluster 2 and the
CONFIG.BIN and logo.jpg of the MSC dump. Needs mtools and dosfstools.

Run 1 (ends with --save-disk):
  * DBG:FLASH? last field 2 (converted), DBG:BOOT? settings file CREATED;
  * SYST:ALER:ALAR VOLUME, then DBG:BOOT? SAVE,OK (the settings file written);
  * a screenshot (DBG:SWIPE D,3): a .BMP listed by MMEM:CAT?.
The saved disk: FAT16 in the BPB, fsck.fat -n clean, CONFIG.BIN and
logo.jpg byte-identical, the SVI directory gone.
Run 2 on the saved disk: DBG:FLASH? last field 0 (not FAT12, untouched),
settings from the file, SYST:ALER:ALAR? VOLUME.
Exit status 0 = PASS. USB/IP port in 63900-63999 unless --port.
"""
import argparse
import os
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
OUT = os.path.join(HERE, 'out')
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--port', type=int, default=None, help='USB/IP port (default: a free one in 63900-63999)')
ap.add_argument('--run1', type=float, default=60.0, help='virtual seconds of the first run (default 60)')
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
import bootinfo  # noqa: E402
if A.port is None:
    A.port = run.free_port(63900, 63999)
print('USB/IP port %d' % A.port, flush=True)
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

SERIAL = run.emu_uid(A.port).hex().upper()
TAG = 'fat12-%d-%d' % (A.port, os.getpid())
MSC = os.path.join(ROOT, 'stock/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin')
FAIL = []


def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


def sh(*a, **kw):
    return subprocess.run(a, check=True, capture_output=True, **kw).stdout


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

    def up(self):
        return int(self.q('DBG:PING?').split(',')[0])

    def flash_conv(self):
        return int(float(self.q('DBG:FLASH?').split(',')[-1]))

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


def le16(b, o):
    return b[o] | b[o + 1] << 8


os.makedirs(OUT, exist_ok=True)
tmp = tempfile.mkdtemp(prefix=TAG, dir=OUT)
cfg, logo = os.path.join(tmp, 'CONFIG.BIN'), os.path.join(tmp, 'logo.jpg')
sh('mcopy', '-i', MSC, '::/CONFIG.BIN', cfg)
sh('mcopy', '-i', MSC, '::/logo.jpg', logo)
disk0, disk1 = os.path.join(tmp, 'disk0.bin'), os.path.join(tmp, 'disk1.bin')
with open(disk0, 'wb') as f:
    f.truncate(16 << 20)
sh('mkfs.fat', '-F', '12', '-S', '4096', '-s', '2', '-f', '2', '-R', '1', '-r', '512', '-h', '0',
   '-M', '0xF8', '-n', 'ATK-DM40', disk0)
sh('mmd', '-i', disk0, '::/System Volume Information')
sh('mcopy', '-i', disk0, cfg, '::/CONFIG.BIN')
sh('mcopy', '-i', disk0, logo, '::/logo.jpg')
with open(disk0, 'rb') as f:
    b0 = f.read(4096)
print('FAT12 image: %d B/sector, %d per cluster, FAT %d sector(s), %d root entries, %d sectors' %
      (le16(b0, 11), b0[13], le16(b0, 22), le16(b0, 17), le16(b0, 19)), flush=True)
check((le16(b0, 11), b0[13], le16(b0, 14), b0[16], le16(b0, 17), le16(b0, 22), le16(b0, 19))
      == (4096, 2, 1, 2, 512, 1, 4096), 'the image has the BY25Q128ES units\' geometry')

print('run 1: the FAT12 disk', flush=True)
emu = start(TAG + '-1', disk0, ['--run-for', str(A.run1), '--save-disk', disk1])
try:
    l = Link()
    c = l.flash_conv()
    check(c == 2, 'DBG:FLASH? conversion: %d (2 = converted)' % c)
    s = bootinfo.settings(l.q('DBG:BOOT?'))
    print('  boot 1: %s' % ','.join(s), flush=True)
    check('CREATED' in s, 'the settings file made on the converted disk')
    l.w('SYST:ALER:ALAR VOLUME')
    ok = False
    for _ in range(120):
        s = bootinfo.settings(l.q('DBG:BOOT?'))
        if s[s.index('SAVE') + 1] == 'OK':
            ok = True
            break
        time.sleep(1)
    check(ok, 'settings saved (DBG:BOOT? %s)' % ','.join(s))

    def bmps():
        cat = l.q('MMEM:CAT?')
        return {x.split(',')[0] for x in cat.split('"')[1::2] if '.BMP' in x.upper()}
    n0 = bmps()
    l.w('DBG:SWIPE D,3')
    got = set()
    for _ in range(120):
        got = bmps() - n0
        if got:
            break
        time.sleep(1)
    check(bool(got), 'a screenshot written: %s' % (sorted(got) or 'none'))
    l.close()
    emu.wait(timeout=3600)
except Exception as e:
    check(False, 'run 1: %r' % (e,))
    stop(emu)

if os.path.exists(disk1):
    with open(disk1, 'rb') as f:
        b1 = f.read(4096)
    check(b1[13] == 1 and le16(b1, 22) == 2 and b1[0x36:0x3E] == b'FAT16   ',
          'saved disk: FAT16 BPB (spc %d, FAT %d blocks, %r)' % (b1[13], le16(b1, 22), b1[0x36:0x3E]))
    r = subprocess.run(['fsck.fat', '-n', disk1], capture_output=True, text=True)
    check(r.returncode == 0, 'fsck.fat -n clean: %s' % r.stdout.strip().splitlines()[-1:])
    if r.returncode:
        print(r.stdout + r.stderr, flush=True)
    for name, src in (('CONFIG.BIN', cfg), ('logo.jpg', logo)):
        back = os.path.join(tmp, 'back-' + name)
        try:
            sh('mcopy', '-n', '-i', disk1, '::/' + name, back)
            same = open(back, 'rb').read() == open(src, 'rb').read()
        except subprocess.CalledProcessError:
            same = False
        check(same, '%s identical after the conversion' % name)
    ls = sh('mdir', '-a', '-i', disk1, '::/').decode(errors='replace')
    print('  ' + '\n  '.join(x for x in ls.splitlines() if x.strip()), flush=True)
    check('System Volume' not in ls and 'SYSTEM~1' not in ls, 'the SVI directory dropped')

    print('run 2: the converted disk', flush=True)
    emu = start(TAG + '-2', disk1, ['--run-for', '3000'])
    try:
        l = Link()
        c = l.flash_conv()
        check(c == 0, 'DBG:FLASH? conversion: %d (0 = not FAT12, untouched)' % c)
        s = bootinfo.settings(l.q('DBG:BOOT?'))
        print('  boot 2: %s' % ','.join(s), flush=True)
        check(s[1] == 'FILE', 'settings from the file')
        got = l.q('SYST:ALER:ALAR?')
        check(got == 'VOLUME', 'SYST:ALER:ALAR? %s after the power cycle' % got)
        l.close()
    except Exception as e:
        check(False, 'run 2: %r' % (e,))
    finally:
        stop(emu)
else:
    check(False, 'disk saved at the end of run 1')

if not FAIL:
    for f in os.listdir(tmp):
        os.remove(os.path.join(tmp, f))
    os.rmdir(tmp)
print('FAIL: %d checks' % len(FAIL) if FAIL else 'PASS')
sys.exit(1 if FAIL else 0)
