#!/usr/bin/env python3
"""fw-rs recovery triggers on the native image (src/boot.rs shim, src/recovery.rs)
through the whole boot chain: ROM -> FS-loader -> fw-rs, the real vendor
FS-loader included (run.py chain mode).

  cd fw-rs && ./build.sh
  emu/recoverytest.py --app fw-rs/target/native/thumbv7em-none-eabihf/release/dm40-fw

The emulator models what the shim decides on (emu/peripherals/FWDGT.cs):
the free watchdog resets the machine ~20 s after the last reload, RCU_RSTSCK
keeps BOR|PIN|POR after power-on, FWDGT|PIN after a watchdog reset, SW|PIN
after a SYSRESETREQ, until RSTFC; the backup SRAM and the flash keep their
content over a reset. The power key is PD6 (pressed = 1); "from off" = the
key pressed at t = 0, when the emulated unit powers up. A hang is injected by
setting the PC to fw-rs's `wait_for_watchdog` (what a panic or DBG:RELEASE
leaves: nothing feeds the watchdog any more; the shim sees the same FWDGT
reset). Case `release` sends the real DBG:RELEASE over USBTMC (USB/IP).

Cases (virtual times; fw-rs is entered ~0.69 s after power-on):
  normal      no key: one boot, no recovery, marker valid
  key-short   key pressed at power-on, released at 0.9 s (a normal power-on)
  key-abort   key held to the beep (shim + 4 s), released in the 0.5 s
              confirm window: normal boot
  key-hold    key held from off for 7 s: recovery = loader marker zeroed,
              SYSRESETREQ, ROM -> FS-loader update screen, fw-rs not entered
  wdt-one     one hang: watchdog reset, fw-rs boots again (crash count 1)
  wdt-two     a second hang ~10 s after the first watchdog reboot: recovery
  wdt-two-70  the second watchdog reset 70 s after the first, but fw-rs ran
              only ~50 s in between: still recovery (the rule is 60 s of
              running main loop, not 60 s between the resets)
  wdt-healthy the second hang after > 60 s of main loop: the crash counter
              was cleared at 60 s, so only a reboot (crash count 1 again)
  release     DBG:RELEASE over USB twice in a row: WDT_RETRY, then recovery

Checks per case: entries of the ROM / FS-loader / fw-rs reset handlers and of
recovery::run (cheap per-address hooks), then at the end the loader marker
(0x08008000), the backup-SRAM boot words (BKP_CRASH / BKP_TRIG / BKP_LAST)
and where the CPU is. Exit status 0 = PASS. Per-process names, private disk
copies; cases run in parallel (--jobs).
"""
import argparse
import glob
import os
import re
import shutil
import struct
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
OUT = os.path.join(HERE, 'out')
sys.path.insert(0, HERE)
import run  # noqa: E402

CASES = ['normal', 'key-short', 'key-abort', 'key-hold', 'wdt-one', 'wdt-two', 'wdt-two-70', 'wdt-healthy']

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='fw-rs native ELF (symbols needed)')
ap.add_argument('--cases', default=','.join(CASES), help='comma list (default: all but `release`); add release for '
                                                         'the USB DBG:RELEASE case')
ap.add_argument('--jobs', type=int, default=3, help='emulators at the same time (default 3)')
A = ap.parse_args()

BKP_CRASH, BKP_TRIG, BKP_LAST = 0x40024FF0, 0x40024FF4, 0x40024FFC
NRML, RCVR = 0x4C4D524E, 0x52564352
CRASH0, CRASH1 = 0x43520000, 0x43520001
TRIG_ZEROED = 0x54520010
MARKER = run.MARKER
LOADER_LO, LOADER_HI = 0x080A0000, 0x080C0000
APP_LO = 0x08004000
TAG = 'RCVT'


def nm(elf):
    tools = glob.glob(os.path.join(ROOT, 'toolchain/rustup/toolchains/*/lib/rustlib/*/bin/llvm-nm'))
    if not tools:
        sys.exit('no llvm-nm under toolchain/')
    out = subprocess.check_output([tools[0], '-C', elf], text=True)
    syms = {}
    for l in out.splitlines():
        p = l.split(None, 2)
        if len(p) == 3:
            syms[p[2]] = int(p[0], 16)
    return syms


def word(path, off):
    with open(path, 'rb') as f:
        f.seek(off)
        return struct.unpack('<I', f.read(4))[0]


SYMS = nm(A.app)
HANG = SYMS['dm40_fw::wait_for_watchdog']
RECOVERY = SYMS['dm40_fw::recovery::run']
with open(A.app, 'rb') as f:
    ELF_ENTRY = struct.unpack_from('<I', f.read(0x40), 0x18)[0] & ~1
ROM_RESET = word(run.DUMP, 4) & ~1
LOADER_RESET = word(run.DUMP, 0xA0004) & ~1
HOOKS = [('rom', ROM_RESET), ('loader', LOADER_RESET), ('app', ELF_ENTRY), ('recovery', RECOVERY)]
PRINT_LOCK = threading.Lock()


def say(*a):
    with PRINT_LOCK:
        print(*a, flush=True)


class Case:
    """One emulator run: timed events, read-outs (`read`, in order) and the
    reset-handler hook log."""

    def __init__(self, name):
        self.name = name
        self.lines = []
        self.reads = []
        self.fails = []
        self.notes = []
        for n, a in HOOKS:
            self.at(0, 'sysbus.cpu AddHook 0x%08X "self.Log(LogLevel.Warning, \'%s %s t=%%s\' %% '
                       'machine.ElapsedVirtualTime.TimeElapsed)"' % (a, TAG, n))

    def at(self, t, cmd):
        self.lines.append('%.3f monitor %s' % (t, cmd))

    def key(self, t, down):
        self.at(t, 'sysbus.gpioPortD OnGPIO 6 %s' % ('true' if down else 'false'))

    def hang(self, t):
        self.at(t, 'sysbus.cpu PC 0x%08X' % HANG)

    def read(self, t, label, addr=None):
        """addr None = the PC."""
        self.at(t, 'sysbus.cpu PC' if addr is None else 'sysbus ReadDoubleWord 0x%08X' % addr)
        self.reads.append(label)

    def end_reads(self, t):
        for i in range(3):
            self.read(t, 'marker%d' % i, MARKER + 4 * i)
        self.read(t, 'crash', BKP_CRASH)
        self.read(t, 'trig', BKP_TRIG)
        self.read(t, 'last', BKP_LAST)
        self.read(t, 'pc')

    def run(self, run_for, extra=()):
        tag = 'rcv-%s-%d' % (self.name, os.getpid())
        self.tag = tag
        script = os.path.join(OUT, tag + '.script')
        with open(script, 'w') as f:
            f.write('\n'.join(self.lines) + '\n')
        disk = os.path.join(OUT, tag + '-disk.bin')
        src = os.path.join(OUT, 'disk.bin')
        shutil.copy(src if os.path.exists(src) else run.DISK_SRC, disk)
        self.log = os.path.join(OUT, tag + '.log')
        self.png = os.path.join(OUT, tag + '.png')
        cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'chain', '--app', A.app, '--name', tag,
               '--script', script, '--disk', disk, '--run-for', '%.3f' % run_for, '--shot', self.png] + list(extra)
        t0 = time.time()
        p = subprocess.run(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        with open(os.path.join(OUT, tag + '.out'), 'w') as f:
            f.write(p.stdout)
        vals = [int(l.split()[0], 16) for l in p.stdout.splitlines() if re.match(r'^0x[0-9A-Fa-f]+\s*$', l)]
        if p.returncode != 0 or len(vals) != len(self.reads):
            self.fail('emulator run: rc %d, %d of %d read-outs (%s)' % (p.returncode, len(vals), len(self.reads),
                                                                     os.path.relpath(self.log, ROOT)))
            self.v = {}
        else:
            self.v = dict(zip(self.reads, vals))
        self.ev = []
        if os.path.exists(self.log):
            for l in open(self.log, errors='replace'):
                m = re.search(TAG + r' (\w+) t=(\d+):(\d+):([\d.]+)', l)
                if m:
                    self.ev.append((m.group(1), int(m.group(2)) * 3600 + int(m.group(3)) * 60 + float(m.group(4))))
        self.wall = time.time() - t0
        os.remove(disk)

    def fail(self, what):
        self.fails.append(what)

    def check(self, ok, what):
        (self.notes if ok else self.fails).append(what)

    def times(self, name):
        return [t for n, t in self.ev if n == name]

    # ---- the end states ----
    def marker_valid(self):
        m = [self.v.get('marker%d' % i) for i in range(3)]
        return None not in m and m != [0, 0, 0] and m != [0xFFFFFFFF] * 3

    def expect_normal(self, boots, crash):
        """fw-rs running after `boots` entries, no recovery."""
        if not self.v:
            return
        self.check(len(self.times('app')) == boots, 'fw-rs entered %d time(s) (%s)' % (boots, self.fmt_ev()))
        self.check(not self.times('recovery'), 'recovery::run not entered')
        self.check(self.marker_valid(), 'loader marker valid')
        self.check(self.v['last'] == NRML, 'BKP_LAST = NRML (0x%08X)' % self.v['last'])
        self.check(self.v['crash'] in crash, 'BKP_CRASH 0x%08X in %s' % (self.v['crash'], ['0x%08X' % c for c in crash]))
        pc = self.v['pc']
        self.check(APP_LO <= pc < LOADER_LO or 0x10000000 <= pc < 0x20030000,
                   'CPU in fw-rs at the end (PC 0x%08X)' % pc)
        scr = self.update_screen()
        if scr is not None:
            self.check(not scr, 'no FS-loader update screen')

    def expect_recovery(self, t_lo, t_hi):
        """recovery::run in [t_lo, t_hi], then ROM -> FS-loader with the
        marker zeroed, fw-rs not entered again."""
        if not self.v:
            return
        rec = self.times('recovery')
        self.check(len(rec) == 1 and t_lo <= rec[0] <= t_hi,
                   'recovery::run once, in %.1f..%.1f s (%s)' % (t_lo, t_hi, self.fmt_ev()))
        if rec:
            after = [(n, t) for n, t in self.ev if t >= rec[0] and n != 'recovery']
            self.check([n for n, _ in after] == ['rom', 'loader'],
                       'after it: ROM -> FS-loader only (%s)' % after)
        self.check([self.v['marker%d' % i] for i in range(3)] == [0, 0, 0], 'loader marker zeroed')
        self.check(self.v['trig'] == TRIG_ZEROED, 'BKP_TRIG = 0x%08X (marker zeroed)' % self.v['trig'])
        self.check(self.v['last'] == RCVR, 'BKP_LAST = RCVR (0x%08X)' % self.v['last'])
        self.check(self.v['crash'] == CRASH0, 'BKP_CRASH cleared on recovery (0x%08X)' % self.v['crash'])
        self.check(LOADER_LO <= self.v['pc'] < LOADER_HI, 'CPU in the FS-loader at the end (PC 0x%08X)' % self.v['pc'])
        scr = self.update_screen()
        if scr is not None:
            self.check(scr, 'FS-loader update screen ("Update" button) in %s' % os.path.relpath(self.png, ROOT))

    def update_screen(self):
        """The FS-loader's update screen in the end screenshot: its blue
        "Update" button (0x0082CE) at 165..315 x 210..270 (None: no PIL / PNG)."""
        try:
            from PIL import Image
            im = Image.open(self.png).convert('RGB')
        except Exception:
            return None
        n = sum(im.getpixel((x, y)) == (0, 130, 206) for x in range(170, 311, 4) for y in range(214, 267, 4))
        return n > 0.9 * len(range(170, 311, 4)) * len(range(214, 267, 4))

    def fmt_ev(self):
        return ' '.join('%s@%.2f' % e for e in self.ev)


def app_time(c, n):
    t = c.times('app')
    return t[n] if len(t) > n else None


def case_normal():
    c = Case('normal')
    c.end_reads(6.0)
    c.run(6.2)
    c.expect_normal(1, (0, CRASH0))
    return c


def case_key_short():
    c = Case('key-short')
    c.key(0.0, True)
    c.key(0.9, False)
    c.end_reads(6.0)
    c.run(6.2)
    c.expect_normal(1, (0, CRASH0))
    return c


def case_key_abort():
    # fw-rs at ~0.69 s; the shim's 4 s window ends ~4.7 s, the beep ~5.0 s,
    # the confirm window ~5.0..5.5 s: release at 5.2.
    c = Case('key-abort')
    c.key(0.0, True)
    c.key(5.2, False)
    c.end_reads(9.0)
    c.run(9.2)
    c.expect_normal(1, (0, CRASH0))
    return c


def case_key_hold():
    c = Case('key-hold')
    c.key(0.0, True)
    c.key(7.0, False)
    c.end_reads(9.0)
    c.run(9.2)
    a = app_time(c, 0) or 0.69
    # 4 s hold + ~0.3 s beep + 0.5 s confirm after the shim started.
    c.expect_recovery(a + 4.7, a + 5.2)
    return c


def case_wdt_one():
    c = Case('wdt-one')
    c.hang(6.0)
    c.end_reads(34.0)
    c.run(34.2)
    c.expect_normal(2, (CRASH1,))
    t = c.times('app')
    if len(t) == 2:
        c.check(25.5 <= t[1] <= 27.5, 'watchdog reboot ~20 s after the hang (fw-rs again at %.2f s)' % t[1])
    return c


def case_wdt_two():
    c = Case('wdt-two')
    c.hang(6.0)          # reset ~26 s, fw-rs ~26.2 s
    c.hang(36.0)         # ~10 s after the reboot -> reset ~56 s
    c.end_reads(62.0)
    c.run(62.2)
    c.expect_recovery(55.5, 57.5)
    return c


def case_wdt_two_70():
    c = Case('wdt-two-70')
    c.hang(6.0)          # reset ~26 s
    c.read(75.5, 'crash_before', BKP_CRASH)
    c.hang(76.0)         # reset ~96 s: 70 s after the first, ~50 s of main loop
    c.end_reads(102.0)
    c.run(102.2)
    if c.v:
        c.check(c.v['crash_before'] == CRASH1, 'crash count still 1 before the 2nd hang (0x%08X)' % c.v['crash_before'])
    c.expect_recovery(95.5, 97.5)
    return c


def case_wdt_healthy():
    c = Case('wdt-healthy')
    c.hang(6.0)          # reset ~26 s; 60 s of main loop at ~88 s
    c.read(92.0, 'crash_before', BKP_CRASH)
    c.hang(92.5)         # reset ~112.5 s
    c.end_reads(118.0)
    c.run(118.2)
    if c.v:
        c.check(c.v['crash_before'] == CRASH0, 'crash count cleared after 60 s of main loop (0x%08X)'
                % c.v['crash_before'])
    c.expect_normal(3, (CRASH1,))
    return c


def case_release():
    """DBG:RELEASE over USBTMC (USB/IP), twice: 1st -> WDT_RETRY, crash 1;
    2nd (within 60 s of main loop) -> recovery, FS-loader."""
    import usbip_client as uc
    c = Case('release')
    port = run.free_port()
    os.environ['USBIP_PORT'] = str(port)
    uc.PORT = port
    end = 150.0
    c.end_reads(end - 0.1)
    res = {}

    def link():
        for _ in range(200):
            try:
                d = uc.Dev('1-0')
                return d, uc.Usbtmc(d)
            except Exception:
                time.sleep(1)
        raise IOError('no USB/IP device on port %d' % port)

    def q(t, s):
        t.write(s)
        return t.read(timeout=60).decode(errors='replace').strip()

    def host():
        try:
            d, t = link()
            res['boot1'] = q(t, 'DBG:BOOT?')
            t.write('DBG:RELEASE')
            d.close()
            time.sleep(5)
            # Re-import after the watchdog reboot: DBG:BOOT? must say WDT_RETRY.
            for _ in range(120):
                d, t = link()
                try:
                    b = q(t, 'DBG:BOOT?')
                except Exception:
                    d.close()
                    time.sleep(2)
                    continue
                if 'WDT_RETRY' in b:
                    res['boot2'] = b
                    break
                d.close()
                time.sleep(2)
            if 'boot2' in res:
                t.write('DBG:RELEASE')
                d.close()
        except Exception as e:
            res['err'] = repr(e)

    th = threading.Thread(target=host, daemon=True)
    th.start()
    c.run(end, ['--usbip', '0.01', '--usbip-port', str(port)])
    th.join(5)
    if 'err' in res:
        c.fail('USB host: %s' % res['err'])
    b1, b2 = res.get('boot1', ''), res.get('boot2', '')
    c.check(',NORMAL,NONE,' in b1, 'first boot NORMAL, NONE (%s)' % b1[:60])
    f2 = b2.split(',')
    c.check(',NORMAL,WDT_RETRY,' in b2 and '0x43520001' in f2, 'after DBG:RELEASE: NORMAL, WDT_RETRY, crash word '
                                                               '0x43520001 (%s)' % b2[:60])
    r = c.times('recovery')
    c.expect_recovery(r[0] if r else 0, r[0] if r else 0)
    return c


FUNCS = {'normal': case_normal, 'key-short': case_key_short, 'key-abort': case_key_abort, 'key-hold': case_key_hold,
         'wdt-one': case_wdt_one, 'wdt-two': case_wdt_two, 'wdt-two-70': case_wdt_two_70,
         'wdt-healthy': case_wdt_healthy, 'release': case_release}


def one(name):
    say('%s: started' % name)
    try:
        c = FUNCS[name]()
    except Exception as e:
        c = Case(name)
        c.fail('exception %r' % e)
        c.ev, c.wall = [], 0
    with PRINT_LOCK:
        print('%s: %s (%.0f s wall; events %s)' % (name, 'PASS' if not c.fails else 'FAIL', c.wall, c.fmt_ev()))
        for n in c.notes:
            print('  ok   ' + n)
        for f in c.fails:
            print('  FAIL ' + f)
        sys.stdout.flush()
    return c


def main():
    os.makedirs(OUT, exist_ok=True)
    names = [n for n in A.cases.split(',') if n]
    for n in names:
        if n not in FUNCS:
            sys.exit('unknown case %s' % n)
    print('app %s: entry 0x%08X, wait_for_watchdog 0x%08X, recovery::run 0x%08X; ROM 0x%08X, FS-loader 0x%08X'
          % (os.path.relpath(A.app), ELF_ENTRY, HANG, RECOVERY, ROM_RESET, LOADER_RESET), flush=True)
    with ThreadPoolExecutor(max_workers=max(1, A.jobs)) as ex:
        res = list(ex.map(one, names))
    bad = [c.name for c in res if c.fails]
    print('recoverytest: %s' % ('PASS (%d cases)' % len(res) if not bad else 'FAIL: ' + ', '.join(bad)))
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
