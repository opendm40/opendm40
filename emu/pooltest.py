#!/usr/bin/env python3
"""The page pool (src/pool.rs PAGE, docs/ram-pools-audit.md): the scope
graph, the gallery and the generator preview share its bytes; a view that
takes them evicts the others, and the scope page draws its stopped record
again after it got them back.

  emu/pooltest.py --app <opendm40 ELF built with --features dbg-pool> [--shots DIR]

Scope page, a 1 kHz sine, 5 ms/div, STOP; the graph read back (A). Then:
  1. MENU -> Signal (the Gen page): DBG:POOL? has GenPreview held, ScopeGraph
     free with one more loss; the Gen page shows its preview;
  2. MENU -> Oscilloscope: ScopeGraph held, the graph equals A (the stopped record
     rendered again, not an empty graph);
  3. MENU -> Gallery: Gallery held; MENU -> Oscilloscope: the graph equals
     A again;
  4. RUN, Gen page, back: the running scope draws a trace (not the empty
     graph of fresh bytes).
A CSV recording runs through all of it (rows keep coming, none dropped);
WAV:DATA? reads the stored record on the Gen page.

Exit status 0 = all passed.
"""
import argparse
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native ELF')
ap.add_argument('--port', type=int, default=None, help='USB/IP port (default: run.free_port)')
ap.add_argument('--shots', help='directory for PNG screenshots of each step')
ap.add_argument('--mips', default='200')
A = ap.parse_args()
sys.path.insert(0, HERE)
import run  # noqa: E402
if A.port is None:
    A.port = run.free_port()
os.environ['USBIP_PORT'] = str(A.port)
import usbip_client as uc  # noqa: E402

FAIL = []

def check(ok, what):
    print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
    if not ok:
        FAIL.append(what)


OUT = os.path.join(HERE, 'out')
os.makedirs(OUT, exist_ok=True)
tag = 'pool-%d-%d' % (A.port, os.getpid())
log = os.path.join(OUT, '%s.log' % tag)
disk = os.path.join(OUT, tag + '.disk.bin')
shutil.copy(os.path.join(ROOT, 'stock/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin'), disk)
cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--mips', A.mips, '--app', A.app,
       '--usbip', '0.01', '--usbip-port', str(A.port), '--run-for', '3000', '--name', tag, '--log', log,
       '--disk', disk, '--scope', 'sine,1000,2.0']
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)

try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('pooltest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60, raw=False):
        t.write(line)
        if '?' not in line:
            return None
        b = t.read(maxlen=65536, timeout=timeout)
        return b if raw else b.decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0])

    def wait_ms(ms):
        t0 = up()
        while up() - t0 < ms:
            time.sleep(0.1)

    def block(b):
        nd = int(b[1:2])
        return b[2 + nd:2 + nd + int(b[2:2 + nd])]

    def lcd(y0, h, x0=0, w=480):
        """Panel rows as RGB565 (lcd.rs read_span's decode, as shottest.py)."""
        px = []
        for y in range(y0, y0 + h, 4):
            rows = min(4, y0 + h - y)
            n = (w * rows * 3 + 1) // 2 + 1
            d = block(q('DBG:LCD? %d,%d,%d,%d,%d' % (x0, y, w, rows, n), raw=True))
            wd = [d[i] | d[i + 1] << 8 for i in range(2, len(d), 2)]
            bs = bytearray()
            for v in wd:
                bs += bytes([v >> 8, v & 0xFF])
            for i in range(w * rows):
                r, g, b = bs[3 * i], bs[3 * i + 1], bs[3 * i + 2]
                px.append((r & 0xF8) << 8 | (g & 0xFC) << 3 | b >> 3)
        return px

    def shot(name):
        if not A.shots:
            return
        from PIL import Image
        os.makedirs(A.shots, exist_ok=True)
        px = lcd(0, 320)
        im = Image.new('RGB', (480, 320))
        im.putdata([((v >> 11) << 3, ((v >> 5) & 63) << 2, (v & 31) << 3) for v in px])
        im.save(os.path.join(A.shots, name + '.png'))

    def graph():
        """The graph's trace rows (without the top bar, the tabs and the
        measurement row)."""
        return lcd(40, 220, 20, 380)

    print('*IDN?', q('*IDN?'), flush=True)

    def pool():
        """DBG:POOL? -> {tenant: (held, gen)}."""
        r = q('DBG:POOL?')
        out = {}
        for part in r.split(';')[1:]:
            name, rest = part.split(':', 1)
            f = rest.split(',')
            out[name] = (f[2] == 'held', int(f[3]))
        return out

    def same(a, b, what, tol=40):
        diff = sum(x != y for x, y in zip(a, b))
        check(diff <= tol, '%s (%d pixels differ)' % (what, diff))

    def ink(px):
        """Pixels of the trace colour family (not the graph background / grid)."""
        bg = max(set(px), key=px.count)
        return sum(p != bg for p in px)

    wait_ms(2500)
    # A CSV recording in the background through every page change below.
    q('LOG:CSV:INT 1')
    q('LOG:CSV ON')
    wait_ms(1500)
    st = q('LOG:CSV?')
    check(st.split(',')[0] != '0', 'CSV recording running (LOG:CSV? %s)' % st)
    q('DBG:KEY 0')  # main menu
    wait_ms(500)
    q('DBG:TAP 341,107')  # the Oscilloscope tile
    wait_ms(1500)
    v = q('DBG:VIEW?')
    check(v.startswith('Oscilloscope'), 'on the scope page (%s)' % v)
    q('TRIG:SWE AUTO')
    q('TRIG:LEV 0')
    q('CHAN1:SCAL 1')
    q('TIM:SCAL 5e-3')
    q('RUN')
    wait_ms(2500)
    q('STOP')
    wait_ms(1500)
    check(q('TRIG:STAT?') == 'STOP', 'stopped')
    p = pool()
    check(p['ScopeGraph'][0] and not p['Gallery'][0] and not p['GenPreview'][0], 'scope page: ScopeGraph holds the pool (%s)' % p)
    g0 = p['ScopeGraph'][1]
    shot('pool-0-scope')
    a = graph()
    check(ink(a) > 300, 'the stopped trace is drawn (%d pixels)' % ink(a))

    # 1. The Gen page takes the bytes.
    q('DBG:KEY 0')
    wait_ms(800)
    q('DBG:TAP 141,219')  # the Signal tile
    wait_ms(2000)
    v = q('DBG:VIEW?')
    check(v.startswith('Signal'), 'on the generator page (%s)' % v)
    # The stored record over SCPI with the scope page closed.
    q('WAV:POIN MAX')
    q('WAV:FORM WORD')
    pre = q('WAV:PRE?').split(',')
    d = block(q('WAV:DATA?', raw=True, timeout=120))
    check(len(d) == 2 * int(pre[2]) and int(pre[2]) > 1000, 'WAV:DATA? of the stored record on the Gen page: %d points, %d bytes' % (int(pre[2]), len(d)))
    p = pool()
    check(p['GenPreview'][0] and not p['ScopeGraph'][0] and p['ScopeGraph'][1] == g0 + 1, 'Gen page: GenPreview holds, ScopeGraph lost it once (%s)' % p)
    shot('pool-1-gen')
    # 2. Back: the stored record drawn again.
    q('DBG:KEY 0')
    wait_ms(800)
    q('DBG:TAP 341,107')
    wait_ms(2500)
    v = q('DBG:VIEW?')
    check(v.startswith('Oscilloscope'), 'back on the scope page (%s)' % v)
    p = pool()
    check(p['ScopeGraph'][0] and not p['GenPreview'][0] and p['GenPreview'][1] >= 1, 'scope page again: ScopeGraph holds (%s)' % p)
    shot('pool-2-scope')
    same(a, graph(), 'after the Gen page: the stopped record as before')
    # 3. The gallery.
    q('DBG:KEY 0')
    wait_ms(800)
    q('DBG:TAP 240,27')
    wait_ms(4000)
    v = q('DBG:VIEW?')
    check('allery' in v, 'gallery shown (%s)' % v)
    p = pool()
    check(p['Gallery'][0] and not p['ScopeGraph'][0], 'gallery: Gallery holds (%s)' % p)
    shot('pool-3-gallery')
    q('DBG:KEY 0')
    wait_ms(800)
    q('DBG:TAP 341,107')
    wait_ms(2500)
    p = pool()
    check(p['ScopeGraph'][0] and not p['Gallery'][0], 'scope page after the gallery: ScopeGraph holds (%s)' % p)
    shot('pool-4-scope')
    same(a, graph(), 'after the gallery: the stopped record as before')
    # 4. Running: a trace after the round trip.
    q('RUN')
    wait_ms(1500)
    q('DBG:KEY 0')
    wait_ms(800)
    q('DBG:TAP 141,219')
    wait_ms(1500)
    check(not pool()['ScopeGraph'][0], 'Gen page again: ScopeGraph evicted')
    q('DBG:KEY 0')
    wait_ms(800)
    q('DBG:TAP 341,107')
    wait_ms(2500)
    b = graph()
    shot('pool-5-run')
    check(ink(b) > 300, 'running after the Gen page: a trace is drawn (%d pixels)' % ink(b))
    st = q('LOG:CSV?').split(',')
    check(st[0] != '0' and int(st[2]) >= 20 and st[5] == '0', 'CSV recording ran through the page changes: %s' % ','.join(st))
    q('LOG:CSV OFF')
finally:
    try:
        os.killpg(emu.pid, 15)
    except ProcessLookupError:
        pass
    try:
        emu.wait(10)
    except subprocess.TimeoutExpired:
        os.killpg(emu.pid, 9)
    for p in (disk,):
        try:
            os.unlink(p)
        except OSError:
            pass

print('FAIL: %d' % len(FAIL) if FAIL else 'PASS', flush=True)
sys.exit(1 if FAIL else 0)
