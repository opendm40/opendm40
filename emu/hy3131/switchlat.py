#!/usr/bin/env python3
"""DMM switch latency: time from a function / coupling / range switch (or an
input step under AUTO range) to the first reading and to the reading within
0.1 % of its final value, for fw-rs and for the stock firmware, on one
scripted timeline per firmware (firmware/docs/switch-latency.md).

  cd firmware && ./build.sh
  emu/hy3131/switchlat.py fwrs --app firmware/target/native/thumbv7em-none-eabihf/release/dm40-fw \
      --port 45010 --rate MED --json emu/out/swl-fwrs-med.json
  emu/hy3131/switchlat.py stock --json emu/out/swl-stock.json
  emu/hy3131/switchlat.py report emu/out/swl-*.json

Timeline: window k starts at T0 + k * W; the window's input is set at its
start, its preparation (taps, ranges) comes at +0.5 / +1.5 s, the switch
(or the input step) at +SW, and readings are sampled until the window ends.
Switches are what a user does: function bar taps (V again cycles the
coupling), the MF range page (stock) / SCPI <f>:RANGe (fw-rs; the same
Dmm::set_range), input steps by the HY3131 model's SetInput.

fw-rs: the run is a usbip run of emu/run.py with the input steps in a
--script (virtual time); a USB/IP client follows the same timeline on the
fw-rs uptime (DBG:PING?, DWT CYCCNT = virtual time, emu/peripherals/DWT.cs)
and polls `DBG:PING?;CALC:AVER:COUNt?;FETCh?` back to back. A reading is
fresh when CALC:AVER:COUNt? moved (the statistics restart at a function
change; the script clears them at every switch). Function changes are
DBG:TAP on the function bar (a meter-side change: FETCh? never waits).

stock: no host channel; the --script samples the stock's DMM struct m
(TCM 0x10001c04, stock/re/analysis/dmm_v2_6.h: +1 range, +2 mode, value
fields +0xc0 DC V/mV, +0xc4 AC V/mV, +0xcc DC A, +0xd8 AC+DC, +0xdc ohm
kohm) and the settle deadline S+0x40 (0x200200bc) every 50 ms of virtual
time with `sysbus ReadDoubleWord`; the readout redraws every 50 ms from m,
and a mode change clears the values (0 until the first reading).

Ports: --port (fw-rs USB/IP), 45000-45999 for agent runs.
"""
import argparse
import json
import os
import struct
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
OUT = os.path.join(EMU, 'out')
NAN = 9.9e37

T0 = 8.0      # first window (s, virtual); fw-rs boots into DC V AUTO+ by ~4 s
W = 14.0      # window length
SW = 7.0      # switch / step time inside the window
SAMPLE = 0.05  # stock sampling period
# The fw-rs script's input events are shifted by BOOT_OFS (virtual s). The client's
# timeline is keyed on the input markers (emu/emusync.py, README "Input markers"):
# window k starts at the fw uptime of its input change, an input step's switch time
# is the step's marker. (BOOT_OFS was the boot, 0.906 s, once; now 2.85 s on a fresh
# disk: the windows only start later in virtual time than they did.)
BOOT_OFS = 0.906

# stock m struct (TCM) and the poll state S
M = 0x10001C04
S_DEADLINE = 0x2002007C + 0x40
F_DCV, F_ACV, F_DCA, F_ACDC, F_OHM = 0xC0, 0xC4, 0xCC, 0xD8, 0xDC

# function bar (x, y): stock 6 buttons of 80 px, fw-rs 7 of ~68 px
BAR = {
    'stock': {'V': (40, 300), 'A': (120, 300), 'OHM': (200, 300)},
    'fwrs': {'V': (35, 300), 'A': (103, 300), 'OHM': (172, 300)},
}
# stock MF "Voltage Settings" page items (model C labels)
STOCK_MF = {'6': (237, 66), '60': (395, 66), 'AUTO+': (79, 155), 'BACK': (14, 18)}

HYP = (0.5 ** 2 + 4.0 ** 2) ** 0.5

# name, input at the window start, prep [(t, action)], switch action at SW,
# step input at SW (or None), stock field, stock scale to SI, expected SI,
# firmware filter (None both), rate-dependent (DC paths)
# actions: ('tap', 'V'|'A'|'OHM'), ('rng', fwrs_scpi, stock_items [..]), ('in', spec)
SCEN = [
    ('dcv>acv', 'acv:1.0@50', [], ('tap', 'V'), F_ACV, 1.0, 1.0, None, False),
    ('acv>acdc', 'acv:1.0@50,dc=0.5', [], ('tap', 'V'), F_ACDC, 1.0, 1.118034, None, False),
    ('acdc>autocpl', 'acv:1.0@50,dc=0.5', [], ('tap', 'V'), None, 1.0, 1.0, 'fwrs', False),
    ('autocpl>dcv', 'dcv:1.0', [], ('tap', 'V'), F_DCV, 1.0, 1.0, 'fwrs', False),
    ('acdc>dcv', 'dcv:1.0', [], ('tap', 'V'), F_DCV, 1.0, 1.0, 'stock', False),
    ('dcv>ohm', 'ohm:4700', [], ('tap', 'OHM'), F_OHM, 1000.0, 4700.0, None, True),
    ('ohm>dcv', 'dcv:1.0', [], ('tap', 'V'), F_DCV, 1.0, 1.0, None, True),
    ('dcv>dca', 'dca:0.05', [], ('tap', 'A'), F_DCA, 1e-3, 0.05, None, True),
    ('dca>dcv', 'dcv:1.0', [], ('tap', 'V'), F_DCV, 1.0, 1.0, None, True),
    ('dcv 6>60', 'dcv:1.0', [(1.5, ('rng', 'VOLT:RANG 6', ['MF', '6']))],
     ('rng', 'VOLT:RANG 60', ['60']), F_DCV, 1.0, 1.0, None, True),
    ('dcv 60>6', 'dcv:1.0', [], ('rng', 'VOLT:RANG 6', ['6']), F_DCV, 1.0, 1.0, None, True),
    ('dcv AUTO up 1>30V', 'dcv:1.0', [(1.5, ('rng', 'VOLT:RANG:AUTO ON', ['AUTO+', 'BACK']))],
     ('in', 'dcv:30'), F_DCV, 1.0, 30.0, None, True),
    ('dcv AUTO down 30>1V', 'dcv:30', [], ('in', 'dcv:1.0'), F_DCV, 1.0, 1.0, None, True),
    ('acv AUTO up 1>30V', 'acv:1.0@50', [(0.5, ('tap', 'V'))], ('in', 'acv:30@50'), F_ACV, 1.0, 30.0, None, False),
    ('acv AUTO down 30>1V', 'acv:30@50', [], ('in', 'acv:1.0@50'), F_ACV, 1.0, 1.0, None, False),
    ('acdc AUTO down 7>4V', 'acv:0.5@50,dc=7', [(0.5, ('tap', 'V'))], ('in', 'acv:0.5@50,dc=4'),
     F_ACDC, 1.0, HYP, None, False),
    ('acdc 60>6', 'acv:1.0@50,dc=0.5', [(1.5, ('rng', 'VOLT:ACDC:RANG 60', ['MF', '60']))],
     ('rng', 'VOLT:ACDC:RANG 6', ['6']), F_ACDC, 1.0, 1.118034, None, False),
    ('acdc>dcv(2)', 'dcv:1.0', [(1.5, ('rng', 'VOLT:ACDC:RANG:AUTO ON', ['AUTO+', 'BACK']))],
     ('tap', 'V'), F_DCV, 1.0, 1.0, 'stock', False),
    ('acdc>dcv(2)', 'dcv:1.0', [(1.5, ('rng', 'VOLT:ACDC:RANG:AUTO ON', ['AUTO+', 'BACK'])), (2.5, ('tap', 'V'))],
     ('tap', 'V'), F_DCV, 1.0, 1.0, 'fwrs', False),
]


def scen_for(fw, dc_only=False):
    out = []
    for s in SCEN:
        if s[7] not in (None, fw):
            continue
        if dc_only and not s[8]:
            continue
        out.append(s)
    return out


# ------------------------------------------------------------------ stock

def stock_script(scens, path):
    lines = []
    st = BAR['stock']

    def at(t, s):
        lines.append('%.3f %s' % (t, s))

    def do(t, act):
        if act[0] == 'tap':
            x, y = st[act[1]]
            at(t, 'tap %d %d' % (x, y))
        elif act[0] == 'in':
            at(t, 'dmm %s' % act[1])
        elif act[0] == 'rng':
            k = 0.0
            for it in act[2]:
                if it == 'MF':
                    at(t + k, 'key mf')
                else:
                    x, y = STOCK_MF[it]
                    at(t + k, 'tap %d %d' % (x, y))
                k += 0.4

    at(0, 'monitor sysbus.hy3131 SetInput "dcv:1.0"')
    at(2.5, 'tap 141 105')  # main menu: Multimeter
    marks = []
    for k, sc in enumerate(scens):
        name, inp, prep, sw, field = sc[:5]
        w0 = T0 + k * W
        at(w0, 'dmm %s' % inp)
        for t, a in prep:
            do(w0 + t, a)
        do(w0 + SW, sw)
        # each monitor time is one Renode pause (~3 s of wall time): 50 ms
        # steps for the first 3.2 s after the switch, then 250 ms
        ts = []
        t = -0.3
        while t < W - SW - 0.05:
            ts.append(round(t, 3))
            t += SAMPLE if t < 3.2 else 0.25
        for t in ts:
            for addr in (M, M + (field or F_DCV), S_DEADLINE):
                at(w0 + SW + t, 'monitor sysbus ReadDoubleWord 0x%08X' % addr)
        n = len(ts)
        marks.append((name, w0, ts))
    with open(path, 'w') as f:
        f.write('\n'.join(lines) + '\n')
    return marks


def run_stock(a):
    scens = scen_for('stock')
    if a.part == 1:
        scens = scens[:7]
    elif a.part == 2:
        scens = scens[7:]  # 'dcv 6>60' onwards: from the boot state (DC V AUTO+)
    tag = 'swl-stock-%d' % os.getpid()
    script = os.path.join(OUT, tag + '.script')
    marks = stock_script(scens, script)
    end = T0 + len(scens) * W + 0.2
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'stock', '--model', a.model, '--name', tag,
           '--script', script, '--run-for', '%.2f' % end]
    print('stock: %d windows, %.0f s virtual' % (len(scens), end), flush=True)
    t0 = time.time()
    r = subprocess.run(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    log = os.path.join(OUT, tag + '.log')
    words = []
    # the monitor's replies go to run.py's stdout (not the Renode log)
    for line in r.stdout.splitlines():
        s = line.strip()
        if s.startswith('0x') and len(s) == 10:
            try:
                words.append(int(s, 16))
            except ValueError:
                pass
    print('stock: rc %d, %.0f s wall, %d words' % (r.returncode, time.time() - t0, len(words)), flush=True)
    res = []
    i = 0
    for k, (name, w0, ts) in enumerate(marks):
        sc = scens[k]
        scale, want = sc[5], sc[6]
        ser = []
        for j, t in enumerate(ts):
            if i + 3 > len(words):
                break
            m0, val, dl = words[i:i + 3]
            i += 3
            v = struct.unpack('<f', struct.pack('<I', val))[0] * scale
            ser.append({'t': t, 'range': (m0 >> 8) & 0xFF, 'mode': (m0 >> 16) & 0xFF, 'v': v, 'deadline': dl})
        res.append({'name': name, 'want': want, 'series': ser})
    for x in (script, os.path.join(OUT, 'flash-%s.bin' % tag), os.path.join(OUT, tag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
    return {'fw': 'stock', 'model': a.model, 'windows': res, 'log': os.path.relpath(log, ROOT)}


# ------------------------------------------------------------------ fw-rs

def run_fwrs(a):
    os.environ['USBIP_PORT'] = str(a.port)
    sys.path.insert(0, EMU)
    import usbip_client as uc
    import emusync
    scens = scen_for('fwrs', a.dc_only)
    tag = 'swl-fwrs-%s-%d' % (a.rate, a.port)
    script = os.path.join(OUT, tag + '.script')
    lines = ['0 monitor sysbus.hy3131 SetInput "dcv:1.0"']
    for k, sc in enumerate(scens):
        w0 = T0 + k * W + BOOT_OFS
        lines.append('%.3f dmm %s' % (w0, sc[1]))
        for t, act in sc[2]:
            if act[0] == 'in':
                lines.append('%.3f dmm %s' % (w0 + t, act[1]))
        if sc[3][0] == 'in':
            lines.append('%.3f dmm %s' % (w0 + SW, sc[3][1]))
    with open(script, 'w') as f:
        f.write('\n'.join(lines) + '\n')
    end = T0 + len(scens) * W + 1 + BOOT_OFS
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--app', a.app, '--model', a.model,
           '--usbip', '0.01', '--usbip-port', str(a.port), '--run-for', '%.1f' % (end + 30),
           '--name', tag, '--script', script]
    emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
    mk = emusync.Markers(emusync.log_path(OUT, tag))
    res = []
    try:
        t = None
        for _ in range(150):
            try:
                t = uc.Usbtmc(uc.Dev('1-0'))
                break
            except Exception:
                time.sleep(2)
        if t is None:
            sys.exit('switchlat: no USB/IP device on port %d' % a.port)

        def q(line, timeout=60):
            t.write(line)
            return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

        def cmdw(line):
            t.write(line)

        def up():
            return int(q('DBG:PING?').split(',')[0])

        def wait_until(ms):
            while up() < ms:
                time.sleep(0.2)

        def sample():
            r = q('DBG:PING?;CALC:AVER:COUN?;FETC?').split(';')
            try:
                return int(r[0].split(',')[0]), int(float(r[1])), float(r[2].split(',')[0])
            except (ValueError, IndexError):
                return int(r[0].split(',')[0]), -1, float('nan')

        def act(a_):
            if a_[0] == 'tap':
                x, y = BAR['fwrs'][a_[1]]
                return 'DBG:TAP %d,%d' % (x, y)
            if a_[0] == 'rng':
                return a_[1]
            return None

        print('fwrs %s: uptime %d ms at connect' % (a.rate, up()), flush=True)
        if mk.wait_clock() is None:
            sys.exit('switchlat: no clock marker (dwt: CYCCNT write) in %s' % mk.log)
        print('fwrs %s: fw clock zero at %.4f s virtual' % (a.rate, mk.zero), flush=True)
        wait_until(int(mk.at(T0 + BOOT_OFS) * 1000) - 3000)
        cmdw('RATE %s' % a.rate)
        q('SYST:ERR?')
        for k, sc in enumerate(scens):
            name, inp, prep, sw = sc[:4]
            v0 = T0 + k * W + BOOT_OFS  # the window's input change (virtual)
            w0 = int(round(mk.at(v0) * 1000))
            for tt, a_ in prep:
                c = act(a_)
                if c:
                    wait_until(w0 + int(tt * 1000))
                    cmdw(c)
            wait_until(w0 + int(SW * 1000) - 400)
            ser = []
            ts = w0 + int(SW * 1000)
            sent = False
            while True:
                u, c, v = sample()
                if not sent and u >= ts - 30:
                    c_ = act(sw)
                    if c_:
                        u0 = int(q('%s;CALC:AVER:CLE;DBG:PING?' % c_).split(',')[0])
                    else:
                        u0 = int(q('CALC:AVER:CLE;DBG:PING?').split(',')[0])
                    sent = True
                    sw_up = u0 if c_ else int(round(mk.at(v0 + SW) * 1000))  # input steps: the step's marker
                    continue
                ser.append({'u': u, 'n': c, 'v': v})
                if u >= w0 + int(W * 1000) - 150:
                    break
                # back-to-back polls starve the emulator (~0.2 ms of virtual
                # time per poll); ~10-20 ms of virtual time between samples
                time.sleep(a.poll_wall)
            for s_ in ser:
                s_['t'] = round((s_['u'] - sw_up) / 1000.0, 3)
            res.append({'name': name, 'want': sc[6], 'sw_up': sw_up, 'series': ser})
            print('  %-22s %d samples' % (name, len(ser)), flush=True)
        q('SYST:ERR?')
    finally:
        os.killpg(emu.pid, 15)
        emu.wait()
        for x in (script, os.path.join(OUT, 'flash-%s.bin' % tag), os.path.join(OUT, tag + '.resc')):
            if os.path.exists(x):
                os.remove(x)
    return {'fw': 'fwrs', 'rate': a.rate, 'model': a.model, 'windows': res}


# ------------------------------------------------------------------ analysis

def analyse(fw, win):
    """-> (t_first, v_first, t_01, v_final): the first reading after the
    switch, the time from which every reading stays within 0.1 % of the final
    (the median of the last 1.5 s), seconds after the switch."""
    want = win['want']
    ser = [s for s in win['series'] if s['t'] >= 0]
    if fw == 'stock':
        pre = [s for s in win['series'] if s['t'] < 0]
        v_pre = pre[-1]['v'] if pre else None
        rd = []
        last = v_pre
        for s in ser:
            v = s['v']
            fresh = v != last
            last = v
            if v != 0 and v == v and abs(v) < 1e30:
                rd.append((s['t'], v, fresh))
        # first reading: first fresh nonzero value
        firsts = [r for r in rd if r[2]]
    else:
        rd = []
        n_prev = 0
        for s in ser:
            v = s['v']
            fresh = s['n'] > n_prev or (s['n'] >= 0 and s['n'] < n_prev)
            n_prev = s['n']
            if v < NAN and s['n'] > 0:
                rd.append((s['t'], v, fresh))
        firsts = [r for r in rd if r[2]] or rd
    if not rd:
        return None, None, None, None
    tail = [v for t, v, _ in rd if t >= rd[-1][0] - 1.5]
    tail.sort()
    fin = tail[len(tail) // 2]
    tol = 0.001 * abs(fin)
    t01 = None
    for i, (t, v, _) in enumerate(rd):
        if all(abs(x - fin) <= tol for _, x, _ in rd[i:]):
            t01 = t
            break
    # an input step: the first fresh reading of the new value (closer to the
    # final than to the reading before the step), not a conversion that was
    # already running at the step
    pre = [s for s in win['series'] if s['t'] < 0 and (fw == 'stock' or (s['v'] < NAN and s['n'] > 0))]
    v_pre = pre[-1]['v'] if pre else None
    if v_pre is not None and abs(fin - v_pre) > 0.01 * abs(fin):
        firsts = [r for r in firsts if abs(r[1] - fin) < abs(r[1] - v_pre)]
    tf, vf = (firsts[0][0], firsts[0][1]) if firsts else (None, None)
    return tf, vf, t01, fin


def report(paths):
    rows = {}
    cols = []
    for p in paths:
        with open(p) as f:
            d = json.load(f)
        col = d['fw'] + ('' if d['fw'] == 'stock' else ' ' + d['rate'])
        cols.append(col)
        for w in d['windows']:
            tf, vf, t01, fin = analyse(d['fw'], w)
            rows.setdefault(w['name'], {})[col] = (tf, vf, t01, fin, w['want'])
    print('| switch | ' + ' | '.join(cols) + ' |')
    print('|---|' + '---|' * len(cols))

    def cell(x):
        if x is None:
            return '-'
        tf, vf, t01, fin, want = x
        if tf is None:
            return 'no reading'
        s = '%.2f / %s' % (tf, '%.2f' % t01 if t01 is not None else 'never')
        if want and abs(fin - want) > 0.01 * abs(want):
            s += ' (final %.4g)' % fin
        if want and abs(vf - want) > 0.001 * abs(want):
            s += ' [first %.4g]' % vf
        return s

    seen = set()
    for name in [s[0] for s in SCEN]:
        if name in rows and name not in seen:
            seen.add(name)
            print('| %s | ' % name + ' | '.join(cell(rows[name].get(c)) for c in cols) + ' |')


def main():
    import signal
    signal.signal(signal.SIGTERM, lambda *_: sys.exit('switchlat: terminated'))
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('fw', choices=['fwrs', 'stock', 'report'])
    ap.add_argument('files', nargs='*', help='report: JSON files')
    ap.add_argument('--app', help='fw-rs native ELF')
    ap.add_argument('--port', type=int, default=45010)
    ap.add_argument('--rate', default='MED', choices=['SLOW', 'MED', 'FAST'])
    ap.add_argument('--dc-only', action='store_true', help='fw-rs: only the DC scenarios (the rate applies)')
    ap.add_argument('--model', default='C')
    ap.add_argument('--part', type=int, default=0, help='stock: 1 = windows 0-6, 2 = the rest (parallel runs)')
    ap.add_argument('--poll-wall', type=float, default=0.1, help='fw-rs: wall seconds between polls')
    ap.add_argument('--json', help='write the raw series here')
    a = ap.parse_args()
    if a.fw == 'report':
        return report(a.files)
    d = run_stock(a) if a.fw == 'stock' else run_fwrs(a)
    if a.json:
        with open(a.json, 'w') as f:
            json.dump(d, f)
    for w in d['windows']:
        tf, vf, t01, fin = analyse(d['fw'], w)
        print('  %-22s first %s (%s)  0.1%% %s  final %s  want %.4g' % (
            w['name'], tf, vf, t01, fin, w['want']), flush=True)


if __name__ == '__main__':
    main()
