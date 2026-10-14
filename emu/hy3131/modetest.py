#!/usr/bin/env python3
"""Run the stock firmware and fw-rs through every DMM mode in the emulator with
scripted HY3131 inputs and compare what they report with the input.

Stock: the Bluetooth measurement protocol (bt_meas_v2_6.h, cmd 06 sets the
mode, cmd 09 returns the shown values); fw-rs: SCPI over the same UART
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
DUMP = os.path.join(ROOT, 'firmware/re/dumps/full-20261013/flash_08000000_1MB.bin')
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
    ('cap-big', 'cap:100e-6',               0x03, 'CAP',       100e-6,  0.03, 1e-6, 18, {}),
    ('hz',     'hz:1000,duty=0.25,amp=1.0', 0x05, 'FREQ',      1000.0,  0.001, 0.1, 9, {'duty': 25.0}),
    ('temp',   'temp:100,cj=25',            0x45, 'TEMP',      100.0,   0.0, 1.5, 12, {'cj': 25.0}),
]

STOCK_SETTINGS = 0x080C0000
POLL_WALL = 1.5  # seconds of wall time between requests (~0.15 s virtual)


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
    tag = 'hy-%s-%s' % (fw, name)
    script = os.path.join(OUT, tag + '.script')
    scenario_script(fw, sc, script)
    col = Client(port, fw, mode_byte, scpi)
    col.start()
    cmd = [sys.executable, RUNNER, fw, '--name', tag, '--run-for', str(run_for), '--script', script,
           '--bt-port', str(port), '--shot', os.path.join(OUT, tag + '.png')]
    if fast:
        cmd.append('--fast')
    t0 = time.time()
    p = subprocess.run(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    wall = time.time() - t0
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
    if res['last'] is not None:
        err = abs(res['last'] - want)
        res['ok'] = err <= max(atol, abs(want) * rtol)
        res['err'] = err
    if p.returncode != 0:
        res['log'] = p.stdout[-800:]
    return res


def fmt_v(v):
    if v is None:
        return '-'
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
    a = ap.parse_args()
    scs = [s for s in SCENARIOS if not a.names or s[0] in a.names]
    fws = ['stock', 'fwrs'] if a.fw == 'both' else [a.fw]
    jobs = [(fw, sc) for fw in fws for sc in scs]
    os.makedirs(OUT, exist_ok=True)
    results = []
    with ThreadPoolExecutor(max_workers=a.jobs) as ex:
        futs = [ex.submit(run_one, fw, sc, a.port_base + i, a.fast) for i, (fw, sc) in enumerate(jobs)]
        for f in futs:
            r = f.result()
            results.append(r)
            print('%-5s %-8s %-28s want %-10s got %-10s %s  (%d readings, %.0f s wall)%s' % (
                r['fw'], r['name'], r['input'], fmt_v(r['want']), fmt_v(r['last']), 'ok ' if r['ok'] else 'BAD',
                len(r['readings']), r['wall'], ' ' + json.dumps(r['extra']) if r['extra'] else ''))
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
    bad = [r for r in results if not r['ok']]
    print('%d scenarios, %d failed' % (len(results), len(bad)))
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
