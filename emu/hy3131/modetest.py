#!/usr/bin/env python3
"""Run the stock firmware and opendm40 through every DMM mode in the emulator with
scripted HY3131 inputs and compare what they report with the input.

Stock: the Bluetooth measurement protocol (bt_meas_v2_6.h, cmd 06 sets the
mode, cmd 09 returns the shown values); opendm40: SCPI over the same UART
(CONF:<f>, READ?). Both go through run.py's --bt-port passthrough: a TCP
client polls (wall clock, the emulation runs ~10x slower than real time),
switches the function once the firmware answers, and keeps polling until the
run ends. (Injecting with `sysbus.btmodule SendHex` from the --script monitor
lines hangs Renode while the machine is paused between RunFor segments.)

  emu/hy3131/modetest.py                 # all scenarios, both firmwares
  emu/hy3131/modetest.py -f stock dcv acv
  emu/hy3131/modetest.py -j 6 --md emu/out/hy3131-modes.md
"""
import argparse
import json
import os
import socket
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
OUT = os.path.join(EMU, 'out')
DUMP = os.path.join(ROOT, 'stock/re/dumps/full-20261013/flash_08000000_1MB.bin')
RUNNER = os.path.join(HERE, 'runemu.py')

# name, input spec, stock mode byte (cmd 06), SCPI function, expected SI value,
# relative tolerance, absolute tolerance, virtual seconds to run, extra expectations
SCENARIOS = [
    ('dcv',    'dcv:1.234',                 0x28, 'VOLT:DC',   1.234,   0.0005, 0.0005, 8, {}),
    ('dcv-neg', 'dcv:-12.5',                0x28, 'VOLT:DC',   -12.5,   0.0005, 0.002, 8, {}),
    ('dcv-hv', 'dcv:230',                   0x28, 'VOLT:DC',   230.0,   0.001, 0.1, 9, {}),
    ('dcmv',   'dcv:0.2345',                0x28, 'VOLT:DC',   0.2345,  0.001, 0.0001, 9, {}),
    ('acv',    'acv:1.0@50',                0x68, 'VOLT:AC',   1.0,     0.01, 0.002, 9, {'freq': 50.0}),
    ('acv-hv', 'acv:230@50',                0x68, 'VOLT:AC',   230.0,   0.01, 0.3, 9, {'freq': 50.0}),
    ('acdcv',  'acv:1.0@50,dc=0.5',         0xA8, 'VOLT:ACDC', 1.118034, 0.01, 0.003, 12, {}),
    ('dca',    'dca:0.5',                   0x31, 'CURR:DC',   0.5,     0.005, 0.001, 9, {}),
    ('dcma',   'dca:0.0123',                0x39, 'CURR:DC',   0.0123,  0.005, 2e-5, 9, {}),
    ('aca',    'aca:0.1@60',                0x71, 'CURR:AC',   0.1,     0.01, 0.0005, 9, {}),
    ('ohm',    'ohm:4700',                  0x32, 'RES',       4700.0,  0.005, 1.0, 11, {}),
    ('ohm-1m', 'ohm:1e6',                   0x32, 'RES',       1e6,     0.005, 500.0, 11, {}),
    ('cont',   'ohm:10',                    0x44, 'CONT',      10.0,    0.02, 0.3, 8, {}),
    ('diode',  'diode:0.65',                0x04, 'DIOD',      0.65,    0.005, 0.002, 8, {}),
    ('cap',    'cap:100e-9',                0x03, 'CAP',       100e-9,  0.03, 1e-9, 12, {}),
    # stock: 104 uF, its own arithmetic (stock/re/analysis/dmm_v2_6.h section 4, emu/hy3131/README.md under the results table): C = I * dt / median5(dV),
    # dt = 260 ms (a read 255 ms after the last, on the 10 ms poll), dV = 5 AD1 results of 50 ms = 250 ms of ramp
    ('cap-big', 'cap:100e-6',               0x03, 'CAP',       100e-6,  0.03, 1e-6, 18, {'stock_want': 100e-6 * 260 / 250}),
    ('hz',     'hz:1000,duty=0.25,amp=1.0', 0x05, 'FREQ',      1000.0,  0.001, 0.1, 9, {'duty': 25.0}),
    ('temp',   'temp:100,cj=25',            0x45, 'TEMP',      100.0,   0.0, 1.5, 12, {'cj': 25.0}),
]

# Fixed connections x functions ("cross" table): connection spec, expectations per function.
# An expectation is a number (value +- max(atol, rtol * value) with the per-function tolerance),
# 'zero' (|x| < zero floor), 'ol' (no valid reading / over range), or None (record only).
CROSS_CONNECTIONS = [
    # cont with a source / diode / capacitor: the stock clamps the negative code to 0 ohm, opendm40 shows the
    # negative value; diode mode on a stiff 1.234 V source reads the source (below the 3 V full scale)
    ('V 1.234', 'vsrc:1.234', {'dcv': 1.234, 'acv': 'zero', 'dca': 'zero', 'aca': 'zero', 'ohm': 'ol', 'cont': None,
                               'diode': 1.234, 'cap': None, 'hz': None, 'temp': None}),
    ('V 1.234 + I 12.3 mA', 'vsrc:1.234;isrc:0.0123', {'dcv': 1.234, 'acv': 'zero', 'dca': 0.0123, 'aca': 'zero',
                                                        'ohm': 'ol', 'cont': None, 'diode': 1.234, 'cap': None, 'hz': None, 'temp': None}),
    ('I 12.3 mA', 'isrc:0.0123', {'dcv': 'zero', 'acv': 'zero', 'dca': 0.0123, 'aca': 'zero', 'ohm': 'ol', 'cont': 'ol',  # open jack
                                  'diode': 'ol', 'cap': None, 'hz': None, 'temp': None}),
    ('R 4.7 k', 'r:4700', {'dcv': 'zero', 'acv': 'zero', 'dca': 'zero', 'aca': 'zero', 'ohm': 4700.0, 'cont': 'ol',
                           'diode': 'ol', 'cap': None, 'hz': 'zero', 'temp': None}),
    ('0.1 V behind 4.7 k', 'vsrc:0.1,rs=4700', {'dcv': 0.1, 'acv': 'zero', 'dca': 'zero', 'aca': 'zero', 'ohm': None,
                                                'cont': None, 'diode': None, 'cap': None, 'hz': None, 'temp': None}),
    ('diode 0.65', 'diode:0.65', {'dcv': 'zero', 'acv': 'zero', 'dca': 'zero', 'aca': 'zero', 'ohm': None, 'cont': None,
                                  'diode': 0.65, 'cap': None, 'hz': None, 'temp': None}),
    ('C 100 nF', 'c:100e-9', {'dcv': 'zero', 'acv': 'zero', 'dca': 'zero', 'aca': 'zero', 'ohm': None, 'cont': None,
                              'diode': 'ol', 'cap': 100e-9, 'hz': None, 'temp': None}),
    ('TC 100 C (cj 25, die 25)', 'tc:100;cj:25', {'dcv': 0.003096, 'acv': 'zero', 'dca': 'zero', 'aca': 'zero', 'ohm': None,
                                                  'cont': None, 'diode': None, 'cap': None, 'hz': None, 'temp': 100.0}),
    ('TC 100 C, die 35', 'tc:100;cj:25;die:35', {'temp': 110.0, 'dcv': 0.003096}),
    # 0.5 V across the A jack: 0.5 / (10 mohm + 50 mohm fuse) = 8.3 A on the auto-selected A range (a real fuse would blow)
    ('0.5 V in the A jack', 'avsrc:0.5', {'dcv': 'zero', 'dca': 8.2, 'aca': None, 'ohm': 'ol'}),
]
CROSS_FUNCTIONS = {  # name: (stock mode byte, SCPI function, tolerance rel, abs, zero floor, run_for)
    'dcv': (0x28, 'VOLT:DC', 0.001, 0.0002, 0.0005, 8),
    'acv': (0x68, 'VOLT:AC', 0.01, 0.002, 0.02, 9),  # zero floor 20 mV: opendm40 keeps ~18 mV of the entry transient on a DC source
    'dca': (0x31, 'CURR:DC', 0.02, 2e-5, 5e-5, 9),
    'aca': (0x71, 'CURR:AC', 0.01, 5e-5, 1e-4, 9),
    'ohm': (0x32, 'RES', 0.005, 1.0, 1.0, 11),
    'cont': (0x44, 'CONT', 0.02, 0.3, 1.0, 8),
    'diode': (0x04, 'DIOD', 0.005, 0.002, 0.01, 8),
    'cap': (0x03, 'CAP', 0.03, 1e-9, 1e-9, 12),
    'hz': (0x05, 'FREQ', 0.001, 0.1, 0.5, 9),
    'temp': (0x45, 'TEMP', 0.0, 1.5, 1.0, 12),
}


def cross_scenarios(only=None):
    out = []
    for cname, spec, exp in CROSS_CONNECTIONS:
        for fname, (mode_byte, scpi, rtol, atol, zero, run_for) in CROSS_FUNCTIONS.items():
            if fname not in exp:
                continue
            if only and fname not in only and cname not in only:
                continue
            want = exp[fname]
            extra = {'cross': cname, 'fn': fname, 'zero': zero}
            out.append(('%s|%s' % (cname.replace(' ', '_'), fname), spec, mode_byte, scpi, want, rtol, atol, run_for, extra))
    return out


STOCK_SETTINGS = 0x080C0000
POLL_WALL = 1.5  # seconds of wall time between requests (~0.15 s virtual)
START_LOCK = threading.Lock()  # run.py's opendm40 preparation (git checkout in the shared worktree) is not reentrant


def crc16_8005(data, init=0xFFFF):
    c = init
    for b in data:
        c ^= (b << 8)
        c &= 0xFFFF
        for _ in range(8):
            c = ((c << 1) ^ 0x8005) & 0xFFFF if c & 0x8000 else (c << 1) & 0xFFFF
    return c


def stock_settings_patch():
    """Monitor commands that turn the stock's Bluetooth on in the settings record
    (byte 0x115 bit 0; bit 7 = first-start AT setup done) with a valid CRC."""
    with open(DUMP, 'rb') as f:
        f.seek(STOCK_SETTINGS - 0x08000000)
        rec = bytearray(f.read(0x320))
    rec[0x115] = 0x81
    c = crc16_8005(rec[4:0x320])
    words = [c & 0xFF, c >> 8, (c ^ 0xFFFF) & 0xFF, (c ^ 0xFFFF) >> 8]  # u16 crc, u16 ~crc, little endian
    cmds = ['sysbus WriteByte 0x%08X 0x%02X' % (STOCK_SETTINGS + i, w) for i, w in enumerate(words)]
    cmds.append('sysbus WriteByte 0x%08X 0x81' % (STOCK_SETTINGS + 0x115))
    return cmds


def frame(cmd, data=b''):
    d = bytes([0xAF, 0x05, 0x03, cmd, len(data)]) + bytes(data)
    return d + bytes([(0x100 - sum(d)) & 0xFF])


def decode_stock(mode, fmt, raw):
    """(value in base units, base unit) or (None, unit) for 0xFFFF."""
    base = {0x03: 'nF', 0x04: 'V', 0x44: 'ohm', 0x05: 'Hz', 0x45: 'C'}.get(mode)
    if base is None:
        base = {0: 'mV', 1: 'uA', 2: 'ohm'}.get(mode & 7, '?')
    if raw == 0xFFFF:
        return None, base
    if mode == 0x44 and fmt & 0x80:
        raw += 0xFFF0
        fmt &= 0x7F
    val = raw * 10.0 ** (3 * ((fmt >> 4) & 3) - ((fmt >> 1) & 7))
    return (-val if fmt & 1 else val), base


SI = {'mV': 1e-3, 'uA': 1e-6, 'ohm': 1.0, 'nF': 1e-9, 'V': 1.0, 'Hz': 1.0, 'C': 1.0}


def parse_stock(buf):
    """All cmd 09 replies in the byte stream -> list of dicts."""
    out = []
    i = 0
    while i + 17 <= len(buf):
        if buf[i] == 0xDF and buf[i + 1] == 0x05 and buf[i + 2] == 0x03 and buf[i + 3] == 0x09 and buf[i + 4] == 0x0B:
            d = buf[i + 5:i + 16]
            if (sum(buf[i:i + 16]) + buf[i + 16]) & 0xFF == 0:
                mode = d[0]
                main, u = decode_stock(mode, d[4], d[9] | d[10] << 8)
                v2, _ = decode_stock(mode, d[2], d[5] | d[6] << 8)
                v3, _ = decode_stock(mode, d[3], d[7] | d[8] << 8)
                out.append({'mode': mode, 'flags': d[1], 'main': None if main is None else main * SI.get(u, 1),
                            'unit': u, 'v2': v2, 'v3': v3, 'fmt': (d[2], d[3], d[4])})
                i += 17
                continue
        i += 1
    return out


def parse_scpi(buf):
    out = []
    for line in buf.decode('latin1').split('\n'):
        line = line.strip()
        if not line:
            continue
        try:
            out.append(float(line.split(',')[0]))
        except ValueError:
            out.append(line)
    return out


class Client(threading.Thread):
    """TCP side of the BT passthrough: polls, switches the function after the
    first answer, records every reply with its wall time."""

    def __init__(self, port, fw, mode_byte, scpi):
        super().__init__(daemon=True)
        self.port, self.fw, self.mode_byte, self.scpi = port, fw, mode_byte, scpi
        self.buf = bytearray()
        self.stop = False
        self.switched = False
        self.connected = threading.Event()
        self.t0 = time.time()

    def poll_msg(self):
        return frame(0x09) if self.fw == 'stock' else b'READ?\n'

    def switch_msg(self):
        return frame(0x06, [self.mode_byte]) if self.fw == 'stock' else ('CONF:%s\n' % self.scpi).encode()

    def run(self):
        s = None
        for _ in range(900):  # wait up to 90 s for Renode to listen
            if self.stop:
                return
            try:
                s = socket.create_connection(('127.0.0.1', self.port), timeout=1)
                break
            except OSError:
                time.sleep(0.1)
        if s is None:
            return
        self.connected.set()
        s.settimeout(0.2)
        last_poll = 0
        seen = 0
        while not self.stop:
            now = time.time()
            if now - last_poll >= POLL_WALL:
                try:
                    if len(self.buf) > seen and not self.switched:
                        s.sendall(self.switch_msg())
                        self.switched = True
                        self.switch_len = len(self.buf)
                        self.switch_time = now - self.t0
                    else:
                        s.sendall(self.poll_msg())
                except OSError:
                    break
                last_poll = now
            try:
                d = s.recv(4096)
                if not d:
                    break
                self.buf += d
            except socket.timeout:
                continue
            except OSError:
                break
        s.close()


def scenario_script(fw, sc, path):
    name, spec, mode_byte, scpi, want, rtol, atol, run_for, extra = sc
    lines = []
    if fw == 'stock':
        for c in stock_settings_patch():
            lines.append('0 monitor %s' % c)
    lines.append('0 monitor sysbus.hy3131 SetInput "%s"' % spec)
    with open(path, 'w') as f:
        f.write('\n'.join(lines) + '\n')


def run_one(fw, sc, port, fast):
    name, spec, mode_byte, scpi, want, rtol, atol, run_for, extra = sc
    if fw == 'stock' and 'stock_want' in extra:
        want = extra['stock_want']
    tag = 'hy-%s-%s-%d' % (fw, name, os.getpid())  # parallel invocations share no file
    script = os.path.join(OUT, tag + '.script')
    scenario_script(fw, sc, script)
    col = Client(port, fw, mode_byte, scpi)
    col.start()
    cmd = [sys.executable, RUNNER, fw, '--name', tag, '--run-for', str(run_for), '--script', script,
           '--bt-port', str(port), '--shot', os.path.join(OUT, tag + '.png')]
    if fast:
        cmd.append('--fast')
    t0 = time.time()
    with START_LOCK:
        p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        col.connected.wait(90)  # Renode is listening: run.py's preparation is over
    out, _ = p.communicate()
    wall = time.time() - t0
    for x in (script, os.path.join(OUT, 'flash-%s.bin' % tag), os.path.join(OUT, tag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
    time.sleep(0.5)
    col.stop = True
    col.join(2)
    res = {'fw': fw, 'name': name, 'input': spec, 'want': want, 'rc': p.returncode, 'wall': round(wall, 1),
           'readings': [], 'last': None, 'ok': False, 'extra': {}}
    buf = bytes(col.buf[getattr(col, 'switch_len', 0):]) if col.switched else b''
    res['switched_at_wall'] = round(getattr(col, 'switch_time', -1), 1)
    if fw == 'stock':
        frames = parse_stock(buf)
        res['readings'] = [f['main'] for f in frames]
        valid = [f for f in frames if f['main'] is not None]
        if valid:
            last = valid[-1]
            res['last'] = last['main']
            res['mode_byte'] = last['mode']
            res['unit'] = last['unit']
            if 'freq' in extra and last['v2'] is not None:
                res['extra']['freq'] = last['v2']
            if 'duty' in extra and last['v3'] is not None:
                res['extra']['duty'] = last['v3']
            if 'cj' in extra and last['v2'] is not None:
                res['extra']['cj'] = last['v2']
        res['n_frames'] = len(frames)
    else:
        vals = parse_scpi(buf)
        res['readings'] = vals
        nums = [v for v in vals if isinstance(v, float) and v < 9e37]
        if nums:
            res['last'] = nums[-1]
    if want is None:
        res['ok'] = True  # record only
    elif want == 'ol':
        # no valid reading: the stock sends 0xFFFF (None), opendm40 +9.9E37 (filtered out) -> last stays None
        res['ok'] = res['last'] is None
        if fw == 'fwrs' and res['last'] is None and not any(isinstance(v, float) and v >= 9e37 for v in res['readings']):
            res['ok'] = False  # no reply at all
    elif want == 'zero':
        res['ok'] = res['last'] is not None and abs(res['last']) <= extra.get('zero', 0)
    elif res['last'] is not None:
        err = abs(res['last'] - want)
        res['ok'] = err <= max(atol, abs(want) * rtol)
        res['err'] = err
    if p.returncode != 0:
        res['log'] = out[-800:]
    return res


def free_ports(base, n):
    """n TCP ports from `base` upwards that nothing listens on right now."""
    ports = []
    p = base
    while len(ports) < n:
        try:
            with socket.socket() as s:
                s.bind(('127.0.0.1', p))
            ports.append(p)
        except OSError:
            pass
        p += 1
    return ports


def fmt_v(v):
    if v is None:
        return '-'
    if isinstance(v, str):
        return v
    if isinstance(v, str):
        return v
    return '%.6g' % v


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('names', nargs='*', help='scenario names (default: all)')
    ap.add_argument('-f', '--fw', choices=['stock', 'fwrs', 'both'], default='both')
    ap.add_argument('-j', '--jobs', type=int, default=4)
    ap.add_argument('--port-base', type=int, default=47100)
    ap.add_argument('--fast', action='store_true', help='pass --fast to run.py')
    ap.add_argument('--json', default=os.path.join(OUT, 'hy3131-modes.json'))
    ap.add_argument('--md', help='write a markdown results table here')
    ap.add_argument('--cross', action='store_true', help='fixed connections x functions instead of the per-function scenarios')
    a = ap.parse_args()
    if a.cross:
        scs = cross_scenarios(a.names)
    else:
        scs = [s for s in SCENARIOS if not a.names or s[0] in a.names]
    fws = ['stock', 'fwrs'] if a.fw == 'both' else [a.fw]
    jobs = [(fw, sc) for fw in fws for sc in scs]
    os.makedirs(OUT, exist_ok=True)
    if 'fwrs' in fws:
        # build opendm40 once up front: parallel first runs would race in the shared worktree
        pre_tag = 'hy-prebuild-%d' % os.getpid()
        pre = subprocess.run([sys.executable, RUNNER, 'fwrs', '--name', pre_tag, '--dry'], cwd=ROOT,
                             stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        if not os.path.exists(os.path.join(OUT, pre_tag + '.resc')):  # --dry exits 1 with the .resc path
            sys.exit('opendm40 build failed:\n' + pre.stdout[-2000:])
        for x in (pre_tag + '.resc', 'flash-%s.bin' % pre_tag):
            if os.path.exists(os.path.join(OUT, x)):
                os.remove(os.path.join(OUT, x))
    results = []
    with ThreadPoolExecutor(max_workers=a.jobs) as ex:
        ports = free_ports(a.port_base, len(jobs))
        futs = [ex.submit(run_one, fw, sc, ports[i], a.fast) for i, (fw, sc) in enumerate(jobs)]
        for f in futs:
            r = f.result()
            results.append(r)
            print('%-5s %-30s %-28s want %-10s got %-10s %s  (%d readings, %.0f s wall)%s%s' % (
                r['fw'], r['name'], r['input'], fmt_v(r['want']), fmt_v(r['last']), 'ok ' if r['ok'] else 'BAD',
                len(r['readings']), r['wall'], ' ' + json.dumps({k: v for k, v in r['extra'].items() if k in ('freq', 'duty', 'cj')}) if r['extra'] else '',
                '' if r['rc'] == 0 else '  run.py rc %s: %s' % (r['rc'], r.get('log', '').strip().splitlines()[-1:])))
            sys.stdout.flush()
    with open(a.json, 'w') as f:
        json.dump(results, f, indent=1)
    if a.md:
        with open(a.md, 'w') as f:
            f.write('| firmware | scenario | input | expected | reported | error | result | readings |\n|---|---|---|---|---|---|---|---|\n')
            for r in results:
                f.write('| %s | %s | `%s` | %s | %s | %s | %s | %s |\n' % (
                    r['fw'], r['name'], r['input'], fmt_v(r['want']), fmt_v(r['last']),
                    fmt_v(r.get('err')), 'ok' if r['ok'] else 'FAIL', ' '.join(fmt_v(v) for v in r['readings'][-6:])))
    if a.cross and a.md:
        with open(a.md, 'w') as f:
            fns = list(CROSS_FUNCTIONS)
            f.write('| connection | firmware | ' + ' | '.join(fns) + ' |\n|---|---|' + '---|' * len(fns) + '\n')
            for cname, spec, exp in CROSS_CONNECTIONS:
                for fw in fws:
                    cells = []
                    for fn in fns:
                        r = next((x for x in results if x['fw'] == fw and x['extra'].get('cross') == cname and x['extra'].get('fn') == fn), None)
                        if r is None:
                            cells.append('')
                            continue
                        shown = 'OL' if r['last'] is None else fmt_v(r['last'])
                        cells.append('%s%s' % (shown, '' if r['ok'] else ' **!**'))
                    f.write('| %s `%s` | %s | %s |\n' % (cname, spec, fw, ' | '.join(cells)))
    bad = [r for r in results if not r['ok']]
    print('%d scenarios, %d failed' % (len(results), len(bad)))
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
