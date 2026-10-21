#!/usr/bin/env python3
"""DM40 emulator driver (Renode).

    emu/run.py stock|fwrs|chain [options]

Modes
  stock   pristine V2.6 application at 0x08004000 (from dm40_v_2_6.dec.bin),
          ROM/FS-loader/calibration from the flash dump, start at the app.
  fwrs    fw-rs native image at 0x08004000 (--app <dm40-fw-native.bin|body.bin|ELF>,
          default: build from a clean worktree of HEAD), start at the app.
  chain   start at the ROM bootloader (0x08000000): ROM -> FS-loader -> app.
          The app is the stock one unless --app is given; --app dump keeps the
          flash dump's own application.

Images are composed in emu/out/ and never written back to the dumps.
Events (--tap/--key/--shot/--vin/--script) are scheduled in virtual time.
"""
import argparse
import glob
import os
import re
import signal
import shutil
import struct
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
OUT = os.path.join(HERE, 'out')
sys.path.insert(0, os.path.join(ROOT, 'firmware'))
import dm40fw  # noqa: E402

DUMP = os.path.join(ROOT, 'firmware/re/dumps/full-20261013/flash_08000000_1MB.bin')
OTP = os.path.join(ROOT, 'firmware/re/dumps/full-20261013/otp_1FFF7800.bin')
UID = os.path.join(ROOT, 'private/memrw-kit-test/full/uid_1FFF7A10.bin')
STOCK_DEC = os.path.join(ROOT, 'dm40_v_2_6.dec.bin')
DISK_SRC = os.path.join(ROOT, 'firmware/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin')
RENODE = os.path.join(HERE, '.renode/current/renode')

APP_BASE = 0x08004000
LOADER_BASE = 0x080A0000
MARKER = 0x08008000


def rel(p):
    return os.path.relpath(p, os.getcwd())


def read(p):
    with open(p, 'rb') as f:
        return f.read()


def app_from_file(path):
    """-> (header12, body) from a .dec.bin, a packed update file, a raw body or an ELF."""
    d = read(path)
    if d[:4] == b'\x7fELF':
        body = elf_body(path)
        return dm40fw.split(dm40fw.decode(dm40fw.build(body, pad=True)))[:3], body
    if len(d) > 12 and struct.unpack_from('<I', d, 0)[0] == 0x2002F7F8:
        body = d  # raw body (word 0 = stock SP)
        w0, ln, w2, _ = dm40fw.split(dm40fw.decode(dm40fw.build(body, pad=True)))
        return (w0, ln, w2), body
    try:
        w0, ln, w2, body = dm40fw.split(d)  # already decoded (12-byte header)
        if ln == len(body) and dm40fw.header_word0(ln, w2) == w0:
            return (w0, ln, w2), body
    except Exception:
        pass
    w0, ln, w2, body = dm40fw.split(dm40fw.decode(d))
    return (w0, ln, w2), body


def elf_body(path):
    objcopy = shutil.which('arm-none-eabi-objcopy') or shutil.which('llvm-objcopy')
    if not objcopy:
        r = subprocess.run(['sh', '-c', 'find %s/toolchain/rustup -name llvm-objcopy | head -1' % ROOT],
                           capture_output=True, text=True)
        objcopy = r.stdout.strip()
    tmp = os.path.join(OUT, 'app-from-elf.body.bin')
    subprocess.check_call([objcopy, '-O', 'binary', '--gap-fill=0xff', path, tmp])
    return read(tmp)


def compose_flash(mode, app, out_path, base=DUMP, zero_marker=False):
    flash = bytearray(read(base))
    assert len(flash) == 0x100000, 'flash dump must be 1 MiB'
    if app == 'dump':
        pass
    else:
        (w0, ln, w2), body = app
        assert len(body) <= LOADER_BASE - APP_BASE, 'app body reaches the FS-loader'
        off = APP_BASE - 0x08000000
        flash[off:LOADER_BASE - 0x08000000] = b'\xff' * (LOADER_BASE - APP_BASE)
        flash[off:off + len(body)] = body
        # The 12-byte marker the ROM/loader manage = the update header (type 1,
        # length, body CRC); the ROM launches the app only if it matches.
        flash[MARKER - 0x08000000:MARKER - 0x08000000 + 12] = struct.pack('<3I', w0, ln, w2)
    if zero_marker:
        flash[MARKER - 0x08000000:MARKER - 0x08000000 + 12] = b'\0' * 12
    with open(out_path, 'wb') as f:
        f.write(flash)
    return out_path


def build_fwrs(worktree=None, rev='HEAD'):
    """Build fw-rs native at `rev` of the main repo (default: its HEAD) in a
    separate worktree; skip the build when the ELF already matches the fw-rs
    tree of that commit. A lock file serialises parallel runs (the worktree
    checkout is not reentrant)."""
    import fcntl
    wt = worktree or os.path.join(OUT, 'fwrs-worktree')
    commit = subprocess.check_output(['git', 'rev-parse', rev], cwd=ROOT, text=True).strip()
    tree = subprocess.check_output(['git', 'rev-parse', commit + ':fw-rs'], cwd=ROOT, text=True).strip()
    elf = os.path.join(wt, 'fw-rs/target/native/thumbv7em-none-eabihf/release/dm40-fw')
    stamp = os.path.join(wt, 'fw-rs/target/native/built-tree')
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, 'fwrs-worktree.lock'), 'w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        built = read(stamp).decode().strip() if os.path.exists(stamp) else ''
        if built == tree and os.path.exists(elf):
            print('fw-rs: tree %s of %s already built (%s)' % (tree[:12], commit[:12], rel(elf)))
            return elf
        if not os.path.isdir(os.path.join(wt, 'fw-rs')):
            subprocess.check_call(['git', 'worktree', 'add', '--detach', wt, commit], cwd=ROOT)
        else:
            subprocess.check_call(['git', 'checkout', '--quiet', '--detach', commit], cwd=wt)
        tc = os.path.join(wt, 'toolchain')
        if not os.path.exists(tc):
            os.symlink(os.path.join(ROOT, 'toolchain'), tc)
        print('fw-rs: building %s (%s)' % (commit[:12], subprocess.check_output(
            ['git', 'log', '-1', '--format=%s', commit], cwd=ROOT, text=True).strip()))
        subprocess.check_call(['./build.sh', 'native'], cwd=os.path.join(wt, 'fw-rs'))
        with open(stamp, 'w') as f:
            f.write(tree + '\n')
    return elf


def merge_models():
    """All emu/peripherals/*.cs as ONE compile unit (Renode compiles each
    include separately: 15 files took 3.4 s, one file 1 s)."""
    usings, bodies = [], []
    for f in sorted(glob.glob(os.path.join(HERE, 'peripherals', '*.cs'))):
        body = []
        for l in read(f).decode().splitlines():
            if re.match(r'^using [A-Za-z0-9_.]+;\s*$', l):
                if l not in usings:
                    usings.append(l)
            else:
                body.append(l)
        bodies.append('// ---- %s ----\n%s' % (os.path.relpath(f, HERE), '\n'.join(body)))
    out = os.path.join(OUT, 'dm40-models.cs')
    text = '\n'.join(usings) + '\n' + '\n'.join(bodies) + '\n'
    if not os.path.exists(out) or read(out).decode() != text:
        with open(out, 'w') as f:
            f.write(text)
    return out


def q(path):
    return '@' + os.path.abspath(path)


class Script:
    def __init__(self):
        self.lines = []
        self.events = []  # (time, order, cmd lines)

    def add(self, line):
        self.lines.append(line)

    def at(self, t, *cmds):
        self.events.append((float(t), len(self.events), list(cmds)))

    def render(self, end, quit_at_end):
        out = list(self.lines)
        now = 0.0
        for t, _, cmds in sorted(self.events):
            if t > now:
                out.append('emulation RunFor "%.6f"' % (t - now))
                now = t
            out.extend(cmds)
        if end > now:
            out.append('emulation RunFor "%.6f"' % (end - now))
        if quit_at_end:
            out.append('quit')
        else:
            out.append('start')
        return '\n'.join(out) + '\n'


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('mode', choices=['stock', 'fwrs', 'chain'])
    ap.add_argument('--flash', help='use this 1 MiB flash image as-is (no composition)')
    ap.add_argument('--base', default=DUMP, help='1 MiB flash dump to compose from')
    ap.add_argument('--app', help='application: .dec.bin / packed dm40*.bin / body / ELF, or "dump"')
    ap.add_argument('--rev', default='HEAD', help='fwrs: build fw-rs from this commit of the main repo (default HEAD)')
    ap.add_argument('--disk', help='16 MiB SPI-flash image (default: working copy of the MSC dump in emu/out/)')
    ap.add_argument('--disk-add', action='append', default=[], metavar='FILE[::NAME]',
                    help='copy FILE into the working disk image (mtools), e.g. dm40-fw-native.bin::DM40_FW.BIN')
    ap.add_argument('--zero-marker', action='store_true',
                    help='zero the 12-byte loader marker at 0x08008000 (chain: the FS-loader then installs dm40*.bin)')
    ap.add_argument('--save-disk', help='write the SPI-flash image here at the end')
    ap.add_argument('--save-flash', help='write the internal flash here at the end')
    ap.add_argument('--uid', default=UID, help='12-byte chip UID file (0x1FFF7A10)')
    ap.add_argument('--otp', default=OTP, help='OTP dump (0x1FFF7800)')
    ap.add_argument('--run-for', type=float, default=3.0, help='virtual seconds to run (default 3)')
    ap.add_argument('--shot', action='append', default=[], metavar='PNG[@T]',
                    help='screenshot at the end (or at virtual time T)')
    ap.add_argument('--autoshot', type=int, default=0, metavar='N', help='PNG every N frames into emu/out/frames/')
    ap.add_argument('--tap', action='append', default=[], metavar='X,Y[@T]', help='tap the screen at T (default 2.5 s)')
    ap.add_argument('--drag', action='append', default=[], metavar='X0,Y0,X1,Y1[@T]', help='drag over 0.3 s')
    ap.add_argument('--key', action='append', default=[], metavar='NAME[@T]',
                    help='side key menu|run|auto|mf, or power (PD6), or pa2')
    ap.add_argument('--vin', action='append', default=[], metavar='V[@T]', help='DMM input voltage (DC V on V/COM)')
    ap.add_argument('--dmm', action='append', default=[], metavar='SPEC[@T]',
                    help='what is connected to the DMM jacks (DM40 front-end model, sysbus.hy3131 SetInput): '
                         'vsrc:1.234[,ac=1.0,f=50,shape=square,duty=0.3,rs=4700] | isrc:0.0123 | r:4700 | c:100e-9 | '
                         'diode:0.65[,rev=1] | tc:100 | cj:25 | die:35 | avsrc:0.5 | short | open; several with ";" '
                         '(elements add up); the older dcv:/acv:/dca:/aca:/ohm:/cap:/hz:/temp: keys still work '
                         '(see emu/hy3131/README.md)')
    ap.add_argument('--scope', action='append', default=[], metavar='SPEC[@T]',
                    help='scope input waveform, e.g. sine,1000,1.0 (shape,Hz,Vpp[,offset])')
    ap.add_argument('--script', help='event file: lines "<t> <tap|drag|key|shot|vin|scope|monitor> args"')
    ap.add_argument('--gdb', nargs='?', const=3333, type=int, help='start a GDB server (port)')
    ap.add_argument('--interactive', action='store_true', help='leave the Renode monitor open')
    ap.add_argument('--pause', action='store_true', help='do not start the CPU (use with --gdb)')
    ap.add_argument('--bt-port', type=int, default=0, help='TCP port for the Bluetooth UART')
    ap.add_argument('--usbip', type=float, default=None, metavar='T',
                    help='start a USB/IP server (TCP 3240) at virtual time T and export the USB device as busid 1-0 '
                         '(use with a long --run-for or --interactive)')
    ap.add_argument('--log', help='Renode log file (default emu/out/<name>.log)')
    ap.add_argument('--name', help='base name of the generated .resc/.log (default: the mode)')
    ap.add_argument('--pa2', type=float, default=None, metavar='T',
                    help='chain: release PA2 (power key) at T seconds; default 0.55 = ROM -> FS-loader -> app; '
                         '0 = ROM -> app directly; never (-1) = FS-loader install screen')
    ap.add_argument('--debug', default='', metavar='P1,P2',
                    help='peripherals to log at debug level, e.g. sysbus.i2c3,sysbus.i2c3.touch')
    ap.add_argument('--trace-boot', action='store_true',
                    help='log when the ROM, FS-loader and app reset vectors are entered')
    ap.add_argument('--pc-samples', type=int, default=0, metavar='N',
                    help='debug: print N PC samples (13 ms apart) at the end')
    ap.add_argument('--fast', action='store_true',
                    help='screenshot mode: --mips 50 (the idle GUI loops run 4x fewer iterations; '
                         'UI work takes up to 4x longer in virtual time, so allow ~1 s more)')
    ap.add_argument('--mips', type=float, default=None,
                    help='CPU PerformanceInMips (default 200 = real speed; lower = fewer busy-loop '
                         'iterations per virtual ms, faster wall clock, compute takes longer virtual time)')
    ap.add_argument('--quantum', default=None, help='emulation global quantum in seconds (e.g. 0.0005)')
    ap.add_argument('--stats', action='store_true', help='print virtual time, instructions and wall time')
    ap.add_argument('--live', metavar='FILE', help='export the LCD to a memory-mapped file for emu/live.py')
    ap.add_argument('--dry', action='store_true', help='only write the .resc')
    a = ap.parse_args(argv)

    os.makedirs(OUT, exist_ok=True)
    if not os.path.exists(RENODE):
        sys.exit('Renode missing: run emu/get-renode.sh')

    # ---- flash image ----
    if a.flash:
        flash = a.flash
    else:
        if a.app == 'dump':
            app = 'dump'
        elif a.app:
            app = app_from_file(a.app)
        elif a.mode == 'fwrs':
            app = app_from_file(build_fwrs(rev=a.rev))
        else:
            app = app_from_file(STOCK_DEC)
        flash = compose_flash(a.mode, app, os.path.join(OUT, 'flash-%s.bin' % (a.name or a.mode)), a.base, a.zero_marker)
    print('flash image:', rel(flash))

    # ---- SPI disk ----
    disk = a.disk or os.path.join(OUT, 'disk.bin')
    if not a.disk and not os.path.exists(disk):
        shutil.copy(DISK_SRC, disk)
    if a.disk_add:
        # Work on a private copy so the shared working image is not changed.
        priv = os.path.join(OUT, 'disk-%s.bin' % (a.name or a.mode))
        shutil.copy(disk, priv)
        disk = priv
        for spec in a.disk_add:
            src, _, name = spec.partition('::')
            name = name or os.path.basename(src).upper()
            subprocess.check_call(['mcopy', '-o', '-i', disk, src, '::' + name], env=dict(os.environ, MTOOLS_SKIP_CHECK='1'))
            print('disk: added %s as %s' % (rel(src), name))
    print('disk image: ', rel(disk))

    s = Script()
    name = a.name or a.mode
    log = a.log or os.path.join(OUT, '%s.log' % name)
    s.add('logFile %s' % q(log))
    s.add('include %s' % q(merge_models()))
    s.add('mach create "dm40"')
    s.add('machine LoadPlatformDescription %s' % q(os.path.join(HERE, 'dm40.repl')))
    s.add('sysbus LoadBinary %s 0x08000000' % q(flash))
    if os.path.exists(a.otp):
        s.add('sysbus LoadBinary %s 0x1FFF7800' % q(a.otp))
    if os.path.exists(a.uid):
        s.add('sysbus LoadBinary %s 0x1FFF7A10' % q(a.uid))
    else:
        print('warning: no UID file (%s): the stock genuine check fails -> 16 MHz' % a.uid)
    s.add('sysbus WriteWord 0x1FFF7A22 0x0400')
    s.add('sysbus.spi2.spiflash Load %s' % q(disk))
    s.add('sysbus.gpioPortD OnGPIO 6 false')  # power key released
    if a.mode == 'chain':
        # PA2 low while the power key is held at power-on: the ROM then runs the
        # FS-loader, which re-tests PA2 and launches the app once it is released.
        s.add('sysbus.gpioPortA OnGPIO 2 false')
    if a.fast and not a.mips:
        a.mips = 50
    if a.mips:
        s.add('sysbus.cpu PerformanceInMips %d' % int(a.mips))
    if a.quantum:
        s.add('emulation SetGlobalQuantum "%s"' % a.quantum)
    vtor = 0x08000000 if a.mode == 'chain' else APP_BASE
    s.add('cpu VectorTableOffset 0x%08X' % vtor)
    # SYSRESETREQ (the FS-loader after an install, fw-rs recovery): Renode runs
    # the machine's `reset` macro; the flash keeps its (possibly updated)
    # content, only the vector table base is restored.
    s.add('macro reset\n"""\n    cpu VectorTableOffset 0x%08X\n"""' % vtor)
    s.add('runMacro $reset')
    for p in [x for x in a.debug.split(',') if x]:
        s.add('logLevel -1 %s' % p)
    if a.trace_boot:
        # Log every transition between the ROM, app and FS-loader flash regions
        # (block granularity; the hook slows the run down a little).
        hook = ("global _boot_region\n"
                "r = 'ROM' if pc < 0x08004000 else ('app' if pc < 0x080A0000 else ('FS-loader' if pc < 0x080C0000 else 'other'))\n"
                "if pc >= 0x08000000 and pc < 0x08100000 and r != globals().get('_boot_region'):\n"
                "    _boot_region = r\n"
                "    self.Log(LogLevel.Info, 'boot: entered %s at 0x%08x, t=%s' % (r, pc, machine.ElapsedVirtualTime.TimeElapsed))\n")
        s.add('sysbus.cpu SetHookAtBlockBegin """\n%s"""' % hook)
    if a.live:
        s.add('sysbus.lcd LiveFile %s 30' % q(a.live))
    if a.autoshot:
        s.add('sysbus.lcd AutoShot %s %d' % (q(os.path.join(OUT, 'frames')), a.autoshot))
    if a.bt_port:
        s.add('sysbus.btmodule Listen %d' % a.bt_port)
    if a.gdb:
        s.add('machine StartGdbServer %d' % a.gdb)
    # RunFor starts the emulation itself (it is refused after `start`); an
    # interactive session is left running at the end instead.

    def parse_at(spec, default):
        if '@' in spec:
            v, t = spec.rsplit('@', 1)
            return v, float(t)
        return spec, default

    def tap(t, x, y):
        s.at(t, 'sysbus.i2c3.touch Tap %d %d' % (int(x), int(y)))

    def drag(t, x0, y0, x1, y1):
        s.at(t, 'sysbus.i2c3.touch Drag %d %d %d %d 300' % (int(x0), int(y0), int(x1), int(y1)))

    def key(t, name):
        name = name.lower()
        keys = {'menu': 0, 'run': 1, 'auto': 2, 'mf': 3}
        if name in keys:
            s.at(t, 'sysbus.i2c3.touch Key %d' % keys[name])
        elif name == 'power':
            s.at(t, 'sysbus.gpioPortD OnGPIO 6 true')
            s.at(t + 0.2, 'sysbus.gpioPortD OnGPIO 6 false')
        elif name == 'power-hold':
            s.at(t, 'sysbus.gpioPortD OnGPIO 6 true')
            s.at(t + 2.0, 'sysbus.gpioPortD OnGPIO 6 false')
        elif name == 'pa2':
            s.at(t, 'sysbus.gpioPortA Release 2')  # floats high (pull-up)
        else:
            sys.exit('unknown key ' + name)

    def shot(t, path):
        s.at(t, 'sysbus.lcd Screenshot %s' % q(path))

    def vin(t, v):
        s.at(t, 'sysbus.hy3131 SetVin %s' % v)

    def dmm(t, spec):
        s.at(t, 'sysbus.hy3131 SetInput "%s"' % spec)

    def scope(t, spec):
        parts = spec.split(',')
        shape = parts[0]
        hz = float(parts[1]) if len(parts) > 1 else 1000.0
        vpp = float(parts[2]) if len(parts) > 2 else 1.0
        off = float(parts[3]) if len(parts) > 3 else 0.0
        s.at(t, 'sysbus.scopefeed SetWave "%s" %g %g %g' % (shape, hz, vpp, off))

    for spec in a.tap:
        v, t = parse_at(spec, 2.5)
        tap(t, *v.split(','))
    for spec in a.drag:
        v, t = parse_at(spec, 2.5)
        drag(t, *v.split(','))
    for spec in a.key:
        v, t = parse_at(spec, 2.5)
        key(t, v)
    for spec in a.vin:
        v, t = parse_at(spec, 0.0)
        vin(t, v)
    for spec in a.dmm:
        v, t = parse_at(spec, 0.0)
        dmm(t, v)
    for spec in a.scope:
        v, t = parse_at(spec, 0.0)
        scope(t, v)
    if a.usbip is not None:
        s.at(a.usbip, 'emulation CreateUSBIPServer', 'host.usb Register sysbus.usbfs')
    if a.mode == 'chain':
        pa2 = 0.55 if a.pa2 is None else a.pa2
        if pa2 >= 0:
            key(pa2, 'pa2')
    if a.script:
        with open(a.script) as f:
            for line in f:
                line = line.split('#', 1)[0].strip()
                if not line:
                    continue
                parts = line.split()
                t = float(parts[0])
                cmd, args = parts[1], parts[2:]
                if cmd == 'tap':
                    tap(t, *args[:2])
                elif cmd == 'drag':
                    drag(t, *args[:4])
                elif cmd == 'key':
                    key(t, args[0])
                elif cmd == 'shot':
                    shot(t, args[0])
                elif cmd == 'vin':
                    vin(t, args[0])
                elif cmd == 'dmm':
                    dmm(t, args[0])
                elif cmd == 'scope':
                    scope(t, args[0])
                elif cmd == 'monitor':
                    s.at(t, ' '.join(args))
                else:
                    sys.exit('bad script line: ' + line)
    end = a.run_for
    for spec in a.shot:
        v, t = parse_at(spec, None)
        if t is None:
            s.at(end, 'sysbus.lcd Screenshot %s' % q(v))
        else:
            shot(t, v)
    if a.stats:
        s.at(end, 'sysbus.cpu ExecutedInstructions')
    if a.save_disk:
        s.at(end, 'sysbus.spi2.spiflash Save %s' % q(a.save_disk))
    if a.save_flash:
        s.at(end, 'sysbus.memtool Save %s 0x08000000 0x100000' % q(a.save_flash))

    resc = os.path.join(OUT, '%s.resc' % name)
    with open(resc, 'w') as f:
        f.write(s.render(end, not a.interactive))
    print('script:     ', rel(resc))
    if a.dry:
        return resc
    cmd = [RENODE, '--console', '--disable-xwt', '-e', 'i @' + os.path.abspath(resc)]
    if not a.interactive:
        cmd.insert(1, '--hide-log')
        cmd += ['-e', 'quit']  # also quit when the script aborts on an error
    import time
    t0 = time.time()
    rc = run_renode(cmd, quiet=not a.interactive, log=log)
    if a.stats:
        wall = time.time() - t0
        print('wall %.1f s for %.2f s virtual (%.1fx slower than real time)' % (wall, end, wall / end))
    return rc


def kill_group(p, grace=2.0):
    """Terminate Renode's whole process group (Renode + its .NET children)."""
    if p.poll() is not None:
        return
    try:
        os.killpg(p.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        p.wait(grace)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        p.wait()


def run_renode(cmd, quiet=True, log=None):
    """Run Renode in its own process group; Ctrl-C / SIGTERM stop it (and every
    child) and exit non-zero. In quiet mode stdin stays an open pipe for the
    whole run: on EOF Renode's console reader spins and kills long runs
    ("semaphore ... maximum count")."""
    if quiet:
        p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, text=True, start_new_session=True)
    else:
        # Interactive: Renode owns the terminal, but in its own session so that
        # Ctrl-C reaches us (and we stop the group) rather than the monitor.
        p = subprocess.Popen(cmd, start_new_session=True)

    def on_signal(signum, _frame):
        raise KeyboardInterrupt
    old = signal.signal(signal.SIGTERM, on_signal)
    old_int = signal.signal(signal.SIGINT, on_signal)  # also when started with SIGINT ignored
    try:
        out = p.stdout.read() if quiet else ''
        rc = p.wait()
    except KeyboardInterrupt:
        print('\ninterrupted: stopping Renode', file=sys.stderr)
        kill_group(p)
        return 130
    finally:
        signal.signal(signal.SIGTERM, old)
        signal.signal(signal.SIGINT, old_int)
        if quiet and p.stdin:
            try:
                p.stdin.close()
            except OSError:
                pass
        kill_group(p, 0.5)  # nothing may outlive us
    if quiet:
        for l in out.splitlines():
            l = re.sub(r'\x1b\[[0-9;]*m', '', l)
            if l.startswith('0x') or 'error' in l.lower() or 'screenshot' in l.lower():
                print(l)
        if log:
            print('log:        ', rel(log))
    return rc


if __name__ == '__main__':
    sys.exit(main())
