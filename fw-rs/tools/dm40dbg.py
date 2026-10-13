#!/usr/bin/env python3
"""Host side of the dm40-fw debug commands (USBTMC, VID:PID 1209:0001).

    dm40dbg.py ping
    dm40dbg.py hold [seconds]            keep the watchdog fed while running
    dm40dbg.py scpi 'CMD' ...            send SCPI, print replies to queries
    dm40dbg.py lcdread x y w h nwords    raw GRAM read-back (dummy word first)
    dm40dbg.py shot out.png [--format rgb565|rgb666]
    dm40dbg.py diag                      diagnostic block (hex)
    dm40dbg.py tap x y | left | right    inject a touch tap / view swipe
    dm40dbg.py drag x0 y0 x1 y1 [--ms N] [--hold MS]
                                         one-finger drag through the gesture
                                         tracker (DBG:DRAG): hold MS still at
                                         x0,y0, move to x1,y1 in N ms (300)
    dm40dbg.py key 0..3 [--hold MS]      side key click (0 MENU, 1 RUN, 2 AUTO,
                                         3 MF); --hold: release after MS ms
                                         (long press; RUN >= 1000 ms in a DMM
                                         view = AutoHold on/off)
    dm40dbg.py boot [--expect FILE] [--raw]
                                         DBG:BOOT? decoded (native); --expect
                                         compares the loader marker with FILE's
                                         update header ("installed OK")
    dm40dbg.py update [--wait S] [--expect FILE]
                                         NATIVE: DBG:UPDATE = zero the loader
                                         marker + reset; the vendor loader then
                                         installs the one dm40*.bin on the disk.
                                         --wait: wait for fw-rs to come back and
                                         print `boot`. tools/reload.sh does the
                                         whole flow (stage file, eject, update)
    dm40dbg.py genuine                   DBG:GENUINE? (device-authenticity check,
                                         only in builds that have it)
    dm40dbg.py image FILE                offline: validate a dm40*.bin update
                                         file (header CRC, body CRC, size)
    dm40dbg.py release                   stop feeding the watchdog. RAM build:
                                         back to stock in ~20 s. NATIVE: a WDT
                                         reset = crash (2nd in a row -> recovery,
                                         which ZEROES the app marker)
    dm40dbg.py reset                     RAM: reset to stock. NATIVE: reboot fw-rs
    dm40dbg.py recover                   RAM: plain reset (Stage 0). NATIVE:
                                         DESTRUCTIVE, = DBG:UPDATE without the
                                         checks/wait of `update` (`recover -h`)

Debug commands are SCPI DBG:... (see fw-rs/src/scpi.rs); needs read/write
access to /dev/usbtmcN (udev rule). Native boot: fw-rs/docs/native-boot.md.
"""
import argparse
import glob
import os
import struct
import sys
import time

VID, PID = 0x1209, 0x0001
STOCK_VID, STOCK_PID = 0x28e9, 0x028f   # stock "USB Connect" disk = loader update disk
W, H = 480, 320
FIRMWARE = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
                        'firmware')
APP_BASE, APP_END = 0x08004000, 0x080A0000

RECOVER_HELP = """\
NATIVE image (fw-rs flashed at 0x08004000, docs/native-boot.md): DBG:RECOVER is
an alias of DBG:UPDATE and is DESTRUCTIVE (a one-way door into the vendor
updater). It programs the 12-byte app marker at 0x08008000 to zero and resets.
The ROM and the FS-loader then refuse to launch the app on every boot and show
the loader's update screen with its own USB disk (28e9:028f) until a valid
dm40*.bin is installed through it: copy one (e.g. dm40_fwrs.bin or
dm40_v_2_6.bin) onto that disk, press "update" (or power-cycle). If a
dm40*.bin was already on the disk (copied, synced and ejected before the
command), the loader installs it straight away. "Flash error" = the marker did
not read back as zero; no reset was done.

LEGACY RAM build (sector 8, BT-loaded) -- the text below: a plain reset (Stage 0).

Prove the native-boot recovery path (Stage 0) -- see fw-rs/docs/stage0-test.md.

`recover` sends DBG:RECOVER, which does a plain SYSRESETREQ (same mechanism as
`reset`). On reset the stock ROM bootloader always runs the FS-loader at
0x080a0000, which scans the SPI-flash FAT for dm40*.bin and, if one is present,
CRC-checks it and re-installs its body to the app region at 0x08004000 before
launching the app. fw-rs cannot write the FAT itself (it only exposes the flash
as a USB disk; the host OS owns the filesystem), so YOU stage the file first.

Stage 0 test (reflashes STOCK over the intact stock app -- non-destructive and
reversible; current fw-rs is only RAM-loaded):

  1. With fw-rs running, it enumerates a USB mass-storage disk. Mount it.
  2. Copy the official stock update file onto that disk's root:
         cp dm40_v_2_6.bin  /run/media/<you>/<DM40 disk>/
     The filename already matches the FS-loader's dm40*.bin pattern -- do NOT
     rename it. Keep EXACTLY ONE dm40*.bin on the disk (two give an
     order-dependent install). Use the obfuscated official file, not *.dec.bin.
  3. Confirm it is really there (ls the mounted disk) and flush/unmount so the
     write reaches the flash.
  4. Run:  dm40dbg.py recover
  5. The device reboots. Expect the stock splash/UI to come up (the FS-loader
     reflashed stock). "update ok" vs "update error" is decided by CRC inside
     the FS-loader; a bad file is rejected and the old app is kept.
  6. The stock app DELETES dm40_v_2_6.bin from the FAT after its first good
     boot (the FS-loader itself does not delete it). Re-mount and verify it is
     gone; if it somehow remains, delete it so the next boot does not reinstall
     in a loop.
  7. Back to fw-rs: native: fw-rs/tools/reload.sh --from-stock (stages the
     image on the stock disk). LEGACY RAM build: fw-rs/tools/reload.sh --ram
     (Bluetooth RAM loader, needs stock running; does not touch the FAT).
"""


def usb_present(vid, pid):
    """True if a USB device vid:pid is enumerated (any interface)."""
    for f in glob.glob('/sys/bus/usb/devices/*/idVendor'):
        base = os.path.dirname(f)
        try:
            v = int(open(f).read(), 16)
            p = int(open(os.path.join(base, 'idProduct')).read(), 16)
        except (OSError, ValueError):
            continue
        if (v, p) == (vid, pid):
            return True
    return False


def find_dev(required=True):
    for d in sorted(glob.glob('/sys/class/usbmisc/usbtmc*')):
        usb = os.path.realpath(os.path.join(d, 'device', '..'))
        try:
            v = int(open(os.path.join(usb, 'idVendor')).read(), 16)
            p = int(open(os.path.join(usb, 'idProduct')).read(), 16)
        except OSError:
            continue
        if (v, p) == (VID, PID):
            return '/dev/' + os.path.basename(d)
    if not required:
        return None
    sys.exit('dm40-fw USBTMC device not found (is the firmware running and the cable in?)')


class Link:
    def __init__(self, dev=None):
        self.fd = os.open(dev or find_dev(), os.O_RDWR)

    def write(self, cmd):
        os.write(self.fd, cmd.encode() + b'\n')

    def read(self, n=1 << 16):
        return os.read(self.fd, n)

    def query(self, cmd):
        self.write(cmd)
        return self.read()

    def try_query(self, cmd):
        """(reply, None) or (None, error text). An unknown query gets no reply:
        the usbtmc read times out (~5 s); the error is then taken from
        SYST:ERR?."""
        self.write(cmd)
        try:
            r = self.read()
        except OSError as e:
            r, why = b'', str(e)
        else:
            why = 'empty reply'
        if r.strip():
            return r, None
        try:
            err = self.query('SYST:ERR?').decode(errors='replace').strip()
        except OSError as e:
            err = str(e)
        return None, '%s; SYST:ERR? -> %s' % (why, err)

    def block(self, cmd):
        """Query returning an IEEE 488.2 definite block (#Nlen...)."""
        d = self.query(cmd)
        if d[:1] != b'#':
            raise IOError('not a block: %r' % d[:32])
        nd = int(d[1:2])
        n = int(d[2:2 + nd])
        d = d[2 + nd:]
        while len(d) < n:
            d += self.read()
        return d[:n]

    def ping(self, keepalive_ms=0):
        up, flags, left = (int(v) for v in self.query('DBG:PING? %d' % keepalive_ms).split(b','))
        return dict(uptime_ms=up, pllsai=bool(flags & 1), usb_core=bool(flags & 2),
                    keepalive_left_ms=left)

    def lcdread(self, x, y, w, h, nwords):
        d = self.block('DBG:LCD? %d,%d,%d,%d,%d' % (x, y, w, h, nwords))
        return list(struct.unpack('<%dH' % (len(d) // 2), d))


def open_link(timeout):
    """Wait up to `timeout` s for the USBTMC device and open it (udev may
    need a moment to set the permissions after enumeration)."""
    t_end = time.time() + timeout
    while True:
        dev = find_dev(required=False)
        if dev:
            try:
                return Link(dev)
            except OSError:
                pass
        if time.time() > t_end:
            return None
        time.sleep(0.5)


# ---- update files ----------------------------------------------------------

def image_info(path):
    """Validate a dm40*.bin update file (the vendor format, firmware/dm40fw.py).
    Returns dict(header=(w0, length, w2), sp, reset, end, problems=[...]);
    the header is what the loader writes to the marker at 0x08008000."""
    sys.path.insert(0, FIRMWARE)
    import dm40fw
    data = open(path, 'rb').read()
    try:
        w0, length, w2, body = dm40fw.split(dm40fw.decode(data))
    except (ValueError, struct.error) as e:
        return dict(header=None, problems=['not an update file (%s) -- a RAM image '
                                           '(dm40-fw.bin) or a decoded file?' % e])
    probs = []
    if w0 != dm40fw.header_word0(length, w2):
        probs.append('header word0 (CRC of words 1/2) wrong: 0x%08x' % w0)
    if w2 >> 16 != 1 or (w2 & 0xFFFF) != dm40fw.body_checksum(body):
        probs.append('word2 0x%08x != 0x1%04x (body CRC)' % (w2, dm40fw.body_checksum(body)))
    end = APP_BASE + length
    if end > APP_END:
        probs.append('body ends at 0x%08x, past 0x%08x (loader)' % (end, APP_END))
    sp, reset = struct.unpack_from('<2I', body, 0) if len(body) >= 8 else (0, 0)
    if not (reset & 1 and APP_BASE <= reset - 1 < end):
        probs.append('reset vector 0x%08x not a thumb address inside the body' % reset)
    return dict(header=(w0, length, w2), sp=sp, reset=reset, end=end, problems=probs)


def print_image(path):
    i = image_info(path)
    print('%s:' % path)
    if i['header']:
        w0, length, w2 = i['header']
        print('  header (= loader marker after install): 0x%08X 0x%08X 0x%08X' % (w0, length, w2))
        print('  body 0x%08x..0x%08x (%d bytes), SP 0x%08x, reset 0x%08x'
              % (APP_BASE, i['end'], length, i['sp'], i['reset']))
    for p in i['problems']:
        print('  BAD: ' + p)
    print('  OK' if not i['problems'] else '  -> do NOT install this file')
    return not i['problems']


# ---- DBG:BOOT? -------------------------------------------------------------

# Field order of boot::put_info + native::put_extra (docs/native-boot.md
# "Normal native mode"). PD6 / STAGED / CALBK / STACK are in-band labels.
BOOT_POS = ['rstsck', 'rstsck_after', 'mode', 'reason', 'clock', 'last', 'hold_ms',
            'bcr1', 'btr1', 'bwtr1', 'lcd_id',
            'fatren', 'found', 'renamed', 'old_backup', 'old_deleted', 'freed_clusters',
            'others', 'rcvr_deleted', 'last_trigger',
            'marker_w0', 'marker_w1', 'marker_w2', 'marker',
            'crash_word', 'healthy', 'fmc_last']
BOOT_LABELS = {'PD6': ['moder', 'pupd', 'idr'], 'STAGED': ['n'],
               'CALBK': ['status', 'written', 'conflict', 'freed'], 'STACK': ['used', 'size'],
               'CALMS': ['ms'], 'SPICLK': ['div', 'check']}
# spiflash.rs ClockCheck (DBG:BOOT? SPICLK,<div>,<check>).
SPICLK_CHECK = {'OK': '/4 (12.5 MHz) read block 0 + FAT block equal to /16 twice',
                'FORCED': 'spiflash::FORCE_SLOW: stock /16',
                'NOCHIP': 'no known chip (JEDEC ID): /16',
                'NOSIG': 'no 0x55AA in block 0 at /16 (blank/foreign disk): /16, not checked',
                'DIFF1': '1st /4 read DIFFERED from /16: fell back to /16',
                'DIFF2': '2nd /4 read DIFFERED from /16: fell back to /16',
                'NOTRUN': 'no SpiFlash created yet'}
RST_FLAGS = [(31, 'LP'), (30, 'WWDGT'), (29, 'FWDGT'), (28, 'SW'), (27, 'POR'), (26, 'EP'), (25, 'BOR')]
# calbackup written / conflict bits (native.rs put_extra).
CALBK_FILES = ['CALIB', 'BOOTDATA', 'UID', 'OPTBYTES', 'OTP', 'ROMBOOT', 'LOADER', 'BACKUP.TXT',
               '_DM40PRV.BIN']
TRIGGERS = {0x54520010: 'marker zeroed by recovery/update', 0x54520011: 'marker zeroing FAILED',
            0x54520000: 'rcvr file created', 0x54520001: 'rcvr file already there',
            0x54520002: 'rcvr: no medium', 0x54520003: 'rcvr: bad BPB',
            0x54520004: 'rcvr: no space', 0x54520005: 'rcvr: error'}


def parse_boot(text):
    """'a,b,...' -> (ordered dict name -> value, warnings)."""
    t = text.strip().split(',')
    f, warn = {}, []
    n = t.index('PD6') if 'PD6' in t else len(t)
    if n != len(BOOT_POS):
        warn.append('%d fields before PD6, decoder knows %d: names may be off'
                    % (n, len(BOOT_POS)))
    for i, v in enumerate(t[:n]):
        f[BOOT_POS[i] if i < len(BOOT_POS) else 'field%d' % i] = v
    i = n
    while i < len(t):
        lab = t[i]
        names = BOOT_LABELS.get(lab)
        if names is None:  # unknown label: take the values up to the next label
            j = i + 1
            while j < len(t) and not t[j][:1].isalpha():
                j += 1
            f[lab] = ','.join(t[i + 1:j])
            i = j
            continue
        for k, nm in enumerate(names):
            if i + 1 + k < len(t):
                f['%s.%s' % (lab, nm)] = t[i + 1 + k]
        i += 1 + len(names)
    return f, warn


def _int(v):
    try:
        return int(v, 0)
    except (TypeError, ValueError):
        return None


def _note(name, v):
    x = _int(v)
    if name == 'STACK.used':
        _note.used = v
    if name in ('rstsck', 'rstsck_after') and x is not None:
        return ' '.join(n for b, n in RST_FLAGS if x >> b & 1) or 'no flags'
    if name == 'last' and x is not None:
        b = struct.pack('<I', x)
        return b.decode() if all(0x20 <= c < 0x7F for c in b) else ('none' if x == 0 else '')
    if name == 'crash_word' and x is not None:
        return 'count %d' % (x & 0xFF) if x >> 8 == 0x435200 else 'not initialised'
    if name == 'fmc_last' and x is not None:
        return 'clean' if x == 0 else 'FMC error/timeout'
    if name == 'last_trigger' and x is not None:
        return TRIGGERS.get(x, 'none' if x == 0 else '?')
    if name in ('CALBK.written', 'CALBK.conflict') and x is not None:
        return '+'.join(n for i, n in enumerate(CALBK_FILES) if x >> i & 1) or '-'
    if name == 'CALBK.status' and v.endswith('_NOFW'):
        return 'no space for _DM40PRV.BIN: backed up without the firmware file'
    if name == 'SPICLK.div':
        return {'4': '12.5 MHz', '16': '3.125 MHz (stock)'}.get(v, '?')
    if name == 'SPICLK.check':
        return SPICLK_CHECK.get(v, '?')
    if name == 'CALMS.ms':
        return 'boot backup time (DWT at 200 MHz; 12.5x low on IRC16M)'
    if name == 'PD6.moder':
        return 'input' if v == '0' else 'NOT input'
    if name == 'PD6.pupd':
        return 'pull-down' if v == '2' else 'expected 2 (pull-down)'
    if name == 'PD6.idr':
        return 'pressed' if v == '1' else 'released'
    if name == 'STACK.size' and _int(v) and _int(_note.used):
        return '%.0f %% used' % (100.0 * _int(_note.used) / _int(v))
    if name == 'hold_ms':
        return 'power-key hold at power-on'
    if name == 'renamed':
        return ''
    return ''


_note.used = None

BOOT_GROUPS = [
    ('boot', ['mode', 'reason', 'clock', 'hold_ms']),
    ('reset flags', ['rstsck', 'rstsck_after']),
    ('previous boot', ['last', 'last_trigger']),
    ('crash', ['crash_word', 'healthy', 'fmc_last']),
    ('loader marker', ['marker_w0', 'marker_w1', 'marker_w2', 'marker']),
    ('fatren', ['fatren', 'found', 'renamed', 'old_backup', 'old_deleted', 'freed_clusters',
                'others', 'rcvr_deleted', 'STAGED.n']),
    ('calbackup', ['CALBK.status', 'CALBK.written', 'CALBK.conflict', 'CALBK.freed', 'CALMS.ms']),
    ('stack', ['STACK.used', 'STACK.size']),
    ('SPI flash clock', ['SPICLK.div', 'SPICLK.check']),
    ('power key PD6', ['PD6.moder', 'PD6.pupd', 'PD6.idr']),
    ('FSMC/LCD seen', ['bcr1', 'btr1', 'bwtr1', 'lcd_id']),
]


def print_boot(text, expect=None, raw=False):
    """Print DBG:BOOT? decoded. Returns True/False for the --expect check
    (None without --expect)."""
    if raw:
        print(text)
    if text.strip() == 'NONE':
        print('DBG:BOOT? -> NONE (no boot info: not booted through the shim)')
        return None if expect is None else False
    f, warn = parse_boot(text)
    for w in warn:
        print('warning: ' + w)
    shown = set()
    for title, names in BOOT_GROUPS:
        parts = []
        for nm in names:
            if nm not in f:
                continue
            shown.add(nm)
            v = f[nm]
            if nm == 'renamed':
                v = repr(v.strip()) if v.strip() else '-'
            n = _note(nm, f[nm])
            parts.append('%s=%s%s' % (nm.split('.')[-1], v, ' (%s)' % n if n else ''))
        if parts:
            print('  %-15s %s' % (title + ':', '  '.join(parts)))
    rest = ['%s=%s' % (k, v) for k, v in f.items() if k not in shown]
    if rest:
        print('  %-15s %s' % ('other:', '  '.join(rest)))
    if expect is None:
        return None
    info = image_info(expect)
    want = info['header']
    have = tuple(_int(f.get('marker_w%d' % i)) for i in range(3))
    print('  expected marker (header of %s): %s' % (
        os.path.basename(expect), ' '.join('0x%08X' % w for w in want) if want else 'n/a (bad file)'))
    if want and have == want and f.get('marker') == 'VALID':
        print('installed OK: the loader marker is the header of %s' % expect)
        return True
    print('NOT the expected image: marker %s %s' % (
        ' '.join(f.get('marker_w%d' % i, '?') for i in range(3)), f.get('marker', '?')))
    return False


def cmd_update(ln, wait, expect):
    """DBG:UPDATE; True once the device dropped off USB (= marker zeroed, reset)."""
    ln.write('DBG:UPDATE')
    t_end = time.time() + 5
    while time.time() < t_end:
        if not usb_present(VID, PID):
            break
        time.sleep(0.1)
    else:
        r, err = ln.try_query('SYST:ERR?')
        sys.exit('DBG:UPDATE: the device did not reset (SYST:ERR? -> %s).\n'
                 '"Flash error" = the marker did not read back as zero (nothing changed);\n'
                 '"Undefined header" = not a native build (RAM build: no DBG:UPDATE).'
                 % (r.decode(errors='replace').strip() if r else err))
    os.close(ln.fd)
    print('DBG:UPDATE: marker zeroed, device reset; the vendor loader installs the dm40*.bin.')
    if not wait:
        print('Wait for fw-rs to re-enumerate (1209:0001), then: dm40dbg.py boot --expect FILE')
        return 0
    print('waiting up to %d s for fw-rs (first boot after an install is slow and dark: '
          'do not power-cycle) ...' % wait)
    t0 = time.time()
    stock_since = None
    while True:
        if usb_present(VID, PID) and find_dev(required=False):
            break
        el = time.time() - t0
        if usb_present(STOCK_VID, STOCK_PID):
            stock_since = stock_since or time.time()
            if time.time() - stock_since > 20:
                print('28e9:028f (vendor loader / stock disk) has been up for 20 s instead of fw-rs:\n'
                      '  the loader update screen means the install failed or found no file\n'
                      '  (bad/missing dm40*.bin), or a stock image was installed. To retry:\n'
                      '  tools/reload.sh --from-stock --no-build  (stages dm40-fw-native.bin on\n'
                      '  that disk), then press "update" on the meter.')
                return 5
        else:
            stock_since = None
        if el > wait:
            print('timeout: fw-rs did not come back within %d s. Look at the meter: loader\n'
                  'screen ("open file error"/"update error") -> copy dm40-fw-native.bin as\n'
                  'dm40_fwrs.bin onto its disk and press "update".' % wait)
            return 4
        time.sleep(0.5)
    print('fw-rs is back after %.0f s' % (time.time() - t0))
    ln = open_link(15)
    if ln is None:
        print('USBTMC device present but cannot be opened (udev permissions?)')
        return 6
    r, err = ln.try_query('DBG:BOOT?')
    if r is None:
        print('DBG:BOOT? failed: %s' % err)
        return 6
    ok = print_boot(r.decode(errors='replace'), expect)
    return 0 if ok in (None, True) else 3


def decode(words, fmt, npix):
    """Raw RAMRD words (dummy dropped) -> list of (r, g, b) 8-bit."""
    px = []
    if fmt == 'rgb565':
        for v in words[:npix]:
            px.append(((v >> 8) & 0xF8, (v >> 3) & 0xFC, (v << 3) & 0xF8))
    else:  # rgb666: one byte per component, packed big-endian into the words
        b = b''.join(struct.pack('>H', v) for v in words)
        for i in range(npix):
            r, g, bl = b[3 * i:3 * i + 3]
            px.append((r & 0xFC, g & 0xFC, bl & 0xFC))
    return px


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--dev')
    sub = ap.add_subparsers(dest='cmd', required=True)
    sub.add_parser('ping')
    p = sub.add_parser('hold'); p.add_argument('seconds', type=float, nargs='?', default=0)
    p = sub.add_parser('scpi'); p.add_argument('commands', nargs='+')
    p = sub.add_parser('lcdread')
    for k in 'x y w h n'.split():
        p.add_argument(k, type=int)
    p = sub.add_parser('shot'); p.add_argument('out')
    p.add_argument('--format', choices=['rgb565', 'rgb666'], default='rgb666')
    sub.add_parser('diag')
    p = sub.add_parser('tap'); p.add_argument('x', type=int); p.add_argument('y', type=int)
    p = sub.add_parser('drag', help='one-finger drag (DBG:DRAG x0,y0,x1,y1,ms,hold)')
    for k in 'x0 y0 x1 y1'.split():
        p.add_argument(k, type=int)
    p.add_argument('--ms', type=int, default=300, help='move time, ms (default 300)')
    p.add_argument('--hold', type=int, default=0, metavar='MS', help='hold still at x0,y0 first, ms')
    sub.add_parser('left')
    sub.add_parser('right')
    p = sub.add_parser('key', help='side key click (DBG:KEY n[,ms]); --hold MS = long press')
    p.add_argument('code', type=int, help='0 MENU, 1 RUN, 2 AUTO, 3 MF')
    p.add_argument('--hold', type=int, default=0, metavar='MS',
                   help='release MS ms after the press (default 0 = click; max 60000); '
                        'RUN held >= 1000 ms in a DMM view toggles AutoHold')
    p = sub.add_parser('boot', help='DBG:BOOT? decoded (native builds)')
    p.add_argument('--expect', metavar='FILE', help='update file whose header the loader marker must match')
    p.add_argument('--raw', action='store_true', help='also print the raw reply')
    p = sub.add_parser('update', help='NATIVE: DBG:UPDATE (zero the loader marker + reset; the '
                       'loader installs the dm40*.bin staged on the disk)')
    p.add_argument('--wait', type=int, default=0, metavar='S',
                   help='wait up to S s for fw-rs to come back, then print `boot`')
    p.add_argument('--expect', metavar='FILE', help='with --wait: check the marker against FILE')
    sub.add_parser('genuine', help='DBG:GENUINE? device-authenticity check (when the build has it)')
    p = sub.add_parser('image', help='offline: validate a dm40*.bin update file')
    p.add_argument('file')
    sub.add_parser('release')
    sub.add_parser('reset')
    sub.add_parser('recover', description=RECOVER_HELP,
                   formatter_class=argparse.RawDescriptionHelpFormatter,
                   help='RAM: plain reset. NATIVE: zero the app marker + reset '
                        '-> vendor update screen (DESTRUCTIVE)')
    a = ap.parse_args()

    if a.cmd == 'image':
        sys.exit(0 if print_image(a.file) else 1)
    if a.cmd in ('update', 'boot') and a.expect and image_info(a.expect)['problems']:
        print_image(a.expect)
        sys.exit(1)

    if a.cmd == 'boot' and not a.dev:
        # may run right after re-enumeration: udev can lag with the permissions
        ln = open_link(10) or Link(find_dev())
    else:
        ln = Link(a.dev)
    if a.cmd == 'ping':
        print(ln.ping())
    elif a.cmd == 'hold':
        t_end = time.time() + a.seconds if a.seconds else None
        while t_end is None or time.time() < t_end:
            print(ln.ping(15000), flush=True)
            time.sleep(5)
    elif a.cmd == 'scpi':
        for c in a.commands:
            if '?' in c:
                print(ln.query(c).decode(errors='replace').rstrip())
            else:
                ln.write(c)
    elif a.cmd == 'lcdread':
        print(' '.join('%04x' % v for v in ln.lcdread(a.x, a.y, a.w, a.h, a.n)))
    elif a.cmd == 'shot':
        from PIL import Image
        img = Image.new('RGB', (W, H))
        per_px = 1 if a.format == 'rgb565' else 1.5
        rows = 4
        t0 = time.time()
        for y in range(0, H, rows):
            n = int(W * rows * per_px + 0.5)
            words = ln.lcdread(0, y, W, rows, 1 + n)[1:]
            for i, c in enumerate(decode(words, a.format, W * rows)):
                img.putpixel((i % W, y + i // W), c)
        img.save(a.out)
        print('saved %s in %.1f s' % (a.out, time.time() - t0))
    elif a.cmd == 'diag':
        print(ln.block('DBG:DIAG?').hex(' '))
    elif a.cmd == 'tap':
        ln.write('DBG:TAP %d,%d' % (a.x, a.y))
    elif a.cmd == 'drag':
        ln.write('DBG:DRAG %d,%d,%d,%d,%d,%d' % (a.x0, a.y0, a.x1, a.y1, a.ms, a.hold))
    elif a.cmd == 'left':
        ln.write('DBG:SWIPE L')
    elif a.cmd == 'right':
        ln.write('DBG:SWIPE R')
    elif a.cmd == 'key':
        # Older firmware reads only n and sends key-down alone.
        ln.write('DBG:KEY %d,%d' % (a.code, a.hold))
    elif a.cmd == 'boot':
        r, err = ln.try_query('DBG:BOOT?')
        if r is None:
            sys.exit('DBG:BOOT? not available (%s); RAM builds other than T1 have none' % err)
        ok = print_boot(r.decode(errors='replace'), a.expect, a.raw)
        sys.exit(3 if ok is False else 0)
    elif a.cmd == 'update':
        sys.exit(cmd_update(ln, a.wait, a.expect))
    elif a.cmd == 'genuine':
        r, err = ln.try_query('DBG:GENUINE?')
        if r is None:
            sys.exit('DBG:GENUINE? not available in this firmware build (%s)' % err)
        v = r.decode(errors='replace').strip().split(',')
        names = ['uid', 'stored', 'expected', 'result']  # hex x3 (12 bytes), GENUINE|MISMATCH
        if len(v) == len(names):
            for nm, x in zip(names, v):
                print('  %-9s %s' % (nm + ':', x))
        else:
            print(','.join(v))
        sys.exit(0 if v[-1] == 'GENUINE' else 2)
    elif a.cmd == 'release':
        ln.write('DBG:RELEASE')
    elif a.cmd == 'reset':
        ln.write('DBG:RESET')
    elif a.cmd == 'recover':
        ln.write('DBG:RECOVER')
        print('DBG:RECOVER sent. RAM build: plain reset (nothing is reflashed\n'
              'unless the loader runs and finds a dm40*.bin). NATIVE: the app\n'
              'marker was zeroed; the device comes up in the vendor update screen\n'
              'with its USB disk -- copy a dm40*.bin there and press "update".\n'
              'See `dm40dbg.py recover -h`.')


if __name__ == '__main__':
    main()
