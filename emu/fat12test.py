#!/usr/bin/env python3
"""opendm40 on the later units' FAT12 SPI disk (BoyaMicro BY25Q128ES: a
Windows-made FAT12 volume, 2 blocks per cluster; fatren.rs `Vol::fat12`),
in the emulator.

  cd firmware && ./build.sh
  emu/fat12test.py --app firmware/target/native/thumbv7em-none-eabihf/release/opendm40 \\
      [--image firmware/dm40_opendm40_v<version>.bin]

The disk: mkfs.fat -F 12 -S 4096 -s 2 -f 2 -r 512 -R 1, a "System Volume
Information" directory and the CONFIG.BIN and logo.jpg of the MSC dump
(the files the units come with). Needs mtools and dosfstools.

Run 1 (fwrs, ends with --save-disk):
  * DBG:BOOT? settings file CREATED; SYST:ALER:ALAR VOLUME, then SAVE,OK;
  * a screenshot (DBG:SWIPE D,3): listed by MMEM:CAT?, its 307266 bytes
    read back with MMEM:DATA?;
  * a CSV recording (LOG:CSV) of a few seconds: listed, read back;
  * --image: DBG:UPLOAD of an update file: DM40FW.BIN, END? size and CRC.
The saved disk: still FAT12 (boot sector byte-identical), fsck.fat -n
clean, CONFIG.BIN and logo.jpg identical, _SETTING.BIN, SHOTS/, SCOPE/ /
LOGS/ there; DM40FW.BIN identical to the image.
Run 2 (fwrs) on it: settings from the file, SYST:ALER:ALAR? VOLUME.
Run 3 (--image; chain, the loader marker zeroed, 15 s): the vendor FS-loader
installs DM40FW.BIN from the FAT12 disk, deletes it and resets, opendm40 starts;
the saved disk is fsck clean without DM40FW.BIN.
Exit status 0 = PASS. USB/IP port in 63900-63999 unless --port.
"""
import argparse
import os
import subprocess
import sys
import tempfile
import time
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
OUT = os.path.join(HERE, 'out')
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF / image')
ap.add_argument('--image', help='update file for the upload / loader install runs')
ap.add_argument('--port', type=int, default=None, help='USB/IP port (default: a free one in 63900-63999)')
ap.add_argument('--run1', type=float, default=90.0, help='virtual seconds of the first run (default 90)')
A = ap.parse_args()
A.app = os.path.abspath(A.app)
if A.image:
    A.image = os.path.abspath(A.image)
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


def sh(*a):
    return subprocess.run(a, check=True, capture_output=True, env=dict(os.environ, MTOOLS_SKIP_CHECK='1')).stdout


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

    def q(self, s, timeout=30, raw=False):
        self.t.write(s)
        b = self.t.read(maxlen=8192, timeout=timeout)
        return b if raw else b.decode(errors='replace').strip()

    def w(self, s):
        self.t.write(s)

    def up(self):
        return int(self.q('DBG:PING?').split(',')[0])

    def wait_ms(self, ms):
        t0 = self.up()
        while self.up() - t0 < ms:
            time.sleep(0.2)

    def files(self):
        cat = self.q('MMEM:CAT?')
        return {x.split(',')[0]: int(x.split(',')[2]) for x in cat.split('"')[1::2] if x.count(',') >= 2}

    def fetch(self, name, size):
        out = bytearray()
        while len(out) < size:
            b = self.q('MMEM:DATA? "%s",%d,6000' % (name, len(out)), raw=True)
            nd = int(b[1:2])
            d = b[2 + nd:2 + nd + int(b[2:2 + nd])]
            if not d:
                break
            out += d
        return bytes(out)

    def close(self):
        try:
            self.d.close()
        except Exception:
            pass


def start(name, disk, extra, mode='fwrs'):
    log = open(os.path.join(OUT, name + '.out'), 'w')
    return subprocess.Popen([sys.executable, os.path.join(HERE, 'run.py'), mode, '--app', A.app, '--usbip', '0.01',
                             '--usbip-port', str(A.port), '--uid', SERIAL, '--name', name, '--disk', disk] + extra,
                            cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)


def stop(emu):
    try:
        os.killpg(emu.pid, 15)
    except Exception:
        pass
    emu.wait()


def fsck(disk, what):
    r = subprocess.run(['fsck.fat', '-n', disk], capture_output=True, text=True)
    check(r.returncode == 0 and 'Truncating' not in r.stdout and 'Free cluster summary wrong' not in r.stdout,
          '%s: fsck.fat -n clean (%s)' % (what, r.stdout.strip().splitlines()[-1:]))
    if r.returncode or 'Truncating' in r.stdout:
        print(r.stdout + r.stderr, flush=True)


def mfile(disk, path, out):
    try:
        sh('mcopy', '-n', '-i', disk, '::/' + path, out)
        return open(out, 'rb').read()
    except subprocess.CalledProcessError:
        return None


os.makedirs(OUT, exist_ok=True)
tmp = tempfile.mkdtemp(prefix=TAG, dir=OUT)
cfg, logo = os.path.join(tmp, 'CONFIG.BIN'), os.path.join(tmp, 'logo.jpg')
sh('mcopy', '-i', MSC, '::/CONFIG.BIN', cfg)
sh('mcopy', '-i', MSC, '::/logo.jpg', logo)
disk0, disk1, disk2 = (os.path.join(tmp, 'disk%d.bin' % i) for i in range(3))
with open(disk0, 'wb') as f:
    f.truncate(16 << 20)
sh('mkfs.fat', '-F', '12', '-S', '4096', '-s', '2', '-f', '2', '-R', '1', '-r', '512', '-h', '0',
   '-M', '0xF8', '-n', 'ATK-DM40', disk0)
sh('mmd', '-i', disk0, '::/System Volume Information')
sh('mcopy', '-i', disk0, cfg, '::/CONFIG.BIN')
sh('mcopy', '-i', disk0, logo, '::/logo.jpg')
b0 = open(disk0, 'rb').read(4096)
le16 = lambda b, o: b[o] | b[o + 1] << 8  # noqa: E731
check((le16(b0, 11), b0[13], le16(b0, 14), b0[16], le16(b0, 17), le16(b0, 22), le16(b0, 19))
      == (4096, 2, 1, 2, 512, 1, 4096), 'the image has the BY25Q128ES units\' geometry (FAT12, 2 blocks per cluster)')

print('run 1: the FAT12 disk', flush=True)
emu = start(TAG + '-1', disk0, ['--run-for', str(A.run1), '--save-disk', disk1, '--dmm', 'dcv:1.234'])
try:
    l = Link()
    s = bootinfo.settings(l.q('DBG:BOOT?'))
    print('  boot 1: %s' % ','.join(s), flush=True)
    check('CREATED' in s, 'the settings file made')
    l.w('SYST:ALER:ALAR VOLUME')
    ok = False
    for _ in range(120):
        s = bootinfo.settings(l.q('DBG:BOOT?'))
        if s[s.index('SAVE') + 1] == 'OK':
            ok = True
            break
        time.sleep(1)
    check(ok, 'settings saved (DBG:BOOT? %s)' % ','.join(s))

    n0 = l.files()
    l.w('DBG:SWIPE D,3')
    shot = {}
    for _ in range(120):
        shot = {k: v for k, v in l.files().items() if k.upper().endswith('.BMP') and k not in n0}
        if shot:
            break
        time.sleep(1)
    check(len(shot) == 1, 'a screenshot written: %s' % shot)
    for name, size in shot.items():
        d = l.fetch(name, size)
        check(size == 307266 and len(d) == size and d[:2] == b'BM', '%s read back (%d bytes)' % (name, len(d)))

    l.w('LOG:CSV ON')
    l.wait_ms(6000)
    l.w('LOG:CSV OFF')
    csv = {}
    for _ in range(60):
        csv = {k: v for k, v in l.files().items() if k.upper().endswith(('.CSV', '.DLG')) and k not in n0 and v > 0}
        if csv:
            break
        time.sleep(1)
    check(len(csv) == 1, 'a recording written: %s' % csv)
    for name, size in csv.items():
        d = l.fetch(name, size)
        check(len(d) == size, '%s read back (%d bytes)' % (name, len(d)))

    if A.image:
        r = False
        for _ in range(20):  # the recording's last commit may still hold the disk
            try:
                r = uc.upload(l.d, A.image)
                break
            except SystemExit as e:
                print('  upload: %s, again' % e, flush=True)
                l.wait_ms(2000)
        check(bool(r), 'DBG:UPLOAD of %s' % os.path.basename(A.image))
    l.close()
    emu.wait(timeout=3600)
except Exception as e:
    check(False, 'run 1: %r' % (e,))
    stop(emu)

if not os.path.exists(disk1):
    check(False, 'disk saved at the end of run 1')
else:
    check(open(disk1, 'rb').read(4096) == b0, 'still the FAT12 volume: boot sector unchanged')
    fsck(disk1, 'run 1')
    for name, src in (('CONFIG.BIN', cfg), ('logo.jpg', logo)):
        check(mfile(disk1, name, os.path.join(tmp, 'b-' + name)) == open(src, 'rb').read(), '%s unchanged' % name)
    ls = sh('mdir', '-a', '-i', disk1, '::/').decode(errors='replace')
    print('  ' + '\n  '.join(x for x in ls.splitlines() if x.strip()), flush=True)
    check('_SETTING' in ls and 'SHOTS' in ls, '_SETTING.BIN and SHOTS/ in the root')
    if A.image:
        fw = mfile(disk1, 'DM40FW.BIN', os.path.join(tmp, 'b-fw'))
        check(fw == open(A.image, 'rb').read(), 'DM40FW.BIN identical to the image (CRC %08x)' % zlib.crc32(fw or b''))

    print('run 2: settings over the power cycle', flush=True)
    emu = start(TAG + '-2', disk1, ['--run-for', '3000'])
    try:
        l = Link()
        s = bootinfo.settings(l.q('DBG:BOOT?'))
        print('  boot 2: %s' % ','.join(s), flush=True)
        check(s[1] == 'FILE', 'settings from the file')
        got = l.q('SYST:ALER:ALAR?')
        check(got == 'VOLUME', 'SYST:ALER:ALAR? %s' % got)
        l.close()
    except Exception as e:
        check(False, 'run 2: %r' % (e,))
    finally:
        stop(emu)

    if A.image:
        print('run 3: the vendor loader installs DM40FW.BIN from the FAT12 disk', flush=True)
        emu = start(TAG + '-3', disk1, ['--zero-marker', '--trace-boot', '--run-for', '15', '--log', os.path.join(OUT, TAG + '-3.log'), '--save-disk', disk2], mode='chain')
        try:
            l = Link()
            idn = l.q('*IDN?')
            check('opendm40' in idn.lower() or 'DM40' in idn, 'opendm40 runs after the install: %s' % idn)
            l.close()
            emu.wait(timeout=3600)
        except Exception as e:
            check(False, 'run 3: %r' % (e,))
            stop(emu)
        # The loader resets only after a successful install (stock/re/docs/
        # fsloader-install-trigger.md); a failure leaves it on its update screen.
        import re
        ev = re.findall(r'boot: entered (ROM|FS-loader|app)', open(os.path.join(OUT, TAG + '-3.log'), errors='replace').read())
        check(ev[:4] == ['ROM', 'FS-loader', 'ROM', 'app'], 'ROM -> FS-loader (install) -> reset -> ROM -> opendm40 (%s)' % ' -> '.join(ev))
        if os.path.exists(disk2):
            fsck(disk2, 'run 3')
            check(mfile(disk2, 'DM40FW.BIN', os.path.join(tmp, 'b-fw2')) is None, 'the loader deleted DM40FW.BIN')
        else:
            check(False, 'disk saved at the end of run 3')

if not FAIL:
    for f in os.listdir(tmp):
        os.remove(os.path.join(tmp, f))
    os.rmdir(tmp)
print('FAIL: %d checks' % len(FAIL) if FAIL else 'PASS')
sys.exit(1 if FAIL else 0)
