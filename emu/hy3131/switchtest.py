#!/usr/bin/env python3
"""fw-rs misconnect sentinel through function / range / coupling switches
(fw-rs/src/sentinel.rs, owner 2026-10-23: "the AC hint and A-JACK / A-PROT
trip falsely when switching from other modes") in the emulator: fw-rs native
+ the HY3131 model with a fixed connection, a sequence of switches over SCPI
over USB/IP (CONF:<f> [range], VOLT:COUP, the MF -> Wave view and back),
each held for DWELL s of fw uptime. After every step the firmware's
assertion log (DBG:SENTinel:LOG?: every alert that came on, with its time,
so a transient between two polls is not missed) and the SYST:ALER? samples
of the step must hold only the alerts the connection allows in that
function.

  emu/hy3131/switchtest.py                        # all scenarios, fw-rs built from HEAD
  emu/hy3131/switchtest.py --app fw-rs/dm40-fw-native.bin --port 44101 open dc5
  emu/hy3131/switchtest.py --full open            # every ordered pair of the functions (long)
  emu/hy3131/switchtest.py --list

The real-* scenarios hold a real condition (a current in A, 24 V across
A-COM, AC or DC on V-COM, 24 V on V in an A function) and check how soon
after every switch into the function its alert comes on (fw uptime from the
switch to the logged assertion), so a blank cannot hide the real thing.
Results: emu/out/switch-<name>.json; the Renode log emu/out/switch-<name>-
<port>-<pid>.log. One emulator per port (a busy port is refused).
"""
import argparse
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
OUT = os.path.join(EMU, 'out')
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('names', nargs='*')
ap.add_argument('--app', help='fw-rs native image (default: run.py builds HEAD)')
ap.add_argument('--port', type=int, default=44100, help='USB/IP port (default 44100)')
ap.add_argument('--dwell', type=float, default=3.0, help='fw-uptime seconds per step (default 3)')
ap.add_argument('--full', action='store_true', help='every ordered pair of the functions (long)')
ap.add_argument('--list', action='store_true')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

# Steps: a name -> the SCPI commands that select it. 'WAVE' = MF -> Wave view
# (V / A functions), 3 s there, MENU back to the DMM.
F = {
    'dcv': ['CONF:VOLT:DC'], 'dcmv': ['CONF:VOLT:DC 0.6'], 'dcv60': ['CONF:VOLT:DC 60'],
    'acv': ['CONF:VOLT:AC'], 'acmv': ['CONF:VOLT:AC 0.6'], 'vacdc': ['CONF:VOLT:ACDC'], 'vauto': ['CONF:VOLT:AUTO'],
    'dca': ['CONF:CURR:DC'], 'dcma': ['CONF:CURR:DC 0.06'], 'dcua': ['CONF:CURR:DC 0.0006'],
    'aca': ['CONF:CURR:AC'], 'acma': ['CONF:CURR:AC 0.06'], 'acua': ['CONF:CURR:AC 0.0006'],
    'aacdc': ['CONF:CURR:ACDC'], 'aauto': ['CONF:CURR:AUTO'],
    'res': ['CONF:RES'], 'cont': ['CONF:CONT'], 'diod': ['CONF:DIOD'], 'cap': ['CONF:CAP'],
    'freq': ['CONF:FREQ'], 'temp': ['CONF:TEMP'], 'pow': ['CONF:POW'],
    'vdcac': ['VOLT:COUP AC'], 'vacdcc': ['VOLT:COUP DC'],
}
WAVE_OK = {'dcv', 'acv', 'acmv', 'dcmv', 'aca', 'acma', 'dca', 'dcma'}
CUR = {'dca', 'dcma', 'dcua', 'aca', 'acma', 'acua', 'aacdc', 'aauto'}
SLOW = {'res', 'temp', 'pow', 'vacdc', 'aacdc', 'vauto', 'aauto'}

# The default sequence: every function entered from several others (V <-> A
# <-> ohm / temperature / diode / capacitance / frequency / power, the
# coupling commands, the Wave view), ~60 steps.
SEQ = ['dcv', 'acv', 'dcv', 'vacdc', 'dcv', 'vauto', 'acv', 'res', 'dcv', 'temp', 'dcv', 'cont', 'dcv', 'diod',
       'dcv', 'cap', 'dcv', 'freq', 'dcv', 'dca', 'dcv', 'aca', 'acv', 'aacdc', 'acv', 'dcma', 'res', 'acua', 'temp',
       'acmv', 'dcmv', 'acmv', 'vacdc', 'res', 'vauto', 'cont', 'acv', 'WAVE', 'dcv', 'WAVE', 'aca', 'WAVE',
       'dcv', 'pow', 'dcv', 'dcv60', 'dcv', 'vdcac', 'vacdcc', 'diod', 'acv', 'cap', 'acv', 'freq', 'acv', 'temp',
       'aauto', 'dcv', 'dcua', 'dcv', 'res', 'acv', 'dcv']
FULL = ['dcv', 'dcmv', 'acv', 'acmv', 'vacdc', 'vauto', 'dca', 'aca', 'aacdc', 'aauto', 'res', 'cont', 'diod', 'cap',
        'freq', 'temp', 'pow']


def full_seq():
    """An Eulerian circuit of the complete digraph on FULL: every ordered
    pair (a -> b) once."""
    n = len(FULL)
    left = {a: [b for b in range(n) if b != a] for a in range(n)}
    stack, path = [0], []
    while stack:
        v = stack[-1]
        if left[v]:
            stack.append(left[v].pop())
        else:
            path.append(stack.pop())
    return [FULL[i] for i in reversed(path)]


def allowed_for(inp, step):
    """Alerts the connection `inp` may raise in `step` (besides NONE)."""
    a = set()
    if inp == 'ajack':
        a |= {'AJACK'}
    if inp == 'aprot':
        a |= {'AJACK', 'APROT'}
    if inp in ('dc5', 'v24'):
        if step in ('acv', 'vdcac'):
            a |= {'DC'}
        if step in CUR:
            a |= {'VJACK'}  # AC A's peak (also an AC+DC / AUTO AC phase): 2 V threshold
        if step in ('res', 'cont', 'diod'):
            a |= {'LIVE'}
        if step == 'pow':
            a |= {'DC', 'AC'}
    if inp == 'ac2':
        if step in ('dcv', 'vacdcc'):
            a |= {'AC'}
        if step in CUR:
            a |= {'VJACK'}
        if step in ('res', 'cont', 'diod'):
            a |= {'LIVE'}
        if step == 'pow':
            a |= {'DC', 'AC'}
    return a


# name: (connection spec, input class for allowed_for, monitor lines, sequence or None
# (= SEQ / --full), {step: (alert, max s from the switch)}: a real condition that must
# come on that fast after every switch into that step - a blank must not hide it).
SC = {
    'open': ('open', 'open', [], None, {}),
    'dc5': ('open;vsrc:5', 'dc5', [], None, {}),
    'ac2': ('open;vsrc:0,ac=2,f=50', 'ac2', [], None, {}),
    # model knobs: the AD1 decimation memory (a mux switch without a restart mixes the old
    # input into the next 3 conversions) and AD2's start-up results 3000 codes off (f3aea2f)
    'open-mix': ('open;ad1mix:1', 'open', ['sysbus.hy3131 Ad2StartGlitch 3000'], None, {}),
    'dc5-mix': ('open;vsrc:5;ad1mix:1', 'dc5', ['sysbus.hy3131 Ad2StartGlitch 3000'], None, {}),
    # the model's switch transient (HY3131.cs SwTrMv, a HYPOTHETICAL ring of the sentinel nodes
    # after every row change, until device numbers come): 300 mV, tau 50 ms, 25 Hz
    'open-swtr': ('open;swtr:300;swtau:50', 'open', [], None, {}),
    'dc5-swtr': ('open;vsrc:5;swtr:300;swtau:50', 'dc5', [], None, {}),
    # device ad74e93 (nothing connected): RES -> A / mA, CMP_SENSE CTSTA 0x70 for ~3.5 s then 0x50
    # to ~5 s -> a false V-JACK? from 1.5-1.95 s; the model's ohmexit: replays that CTSTA
    'ohmexit': ('open;ohmexit:3.5;ohmexitlo:1.2', 'open', [],
                ['res', 'dca', 'res', 'dcma', 'res', 'dcua', 'res', 'aauto', 'res', 'aacdc', 'res', 'aca', 'cont', 'dca',
                 'temp', 'dcma', 'diod', 'dca', 'cap', 'dcma', 'res', 'dcv', 'res', 'dca'], {}),
    'real-vjack-ohmexit': ('open;vsrc:24;ohmexit:3.5;ohmexitlo:1.2', 'v24', [], ['dcv', 'dca', 'res', 'dca', 'res', 'dcma'],
                           {'dca': ('VJACK', 6.5), 'dcma': ('VJACK', 6.5)}),
    'real-ajack-swtr': ('open;isrc:0.001;swtr:300;swtau:50', 'ajack', [],
                        ['acv', 'dcv', 'res', 'dcv', 'temp', 'dcv', 'vacdc', 'dcv', 'acv', 'cont', 'acv'],
                        {'dcv': ('AJACK', 1.0), 'acv': ('AJACK', 1.6), 'cont': ('AJACK', 1.0), 'vacdc': ('AJACK', 2.5)}),
    # real conditions: how fast after a switch in (settle + the detector)
    'real-ajack': ('open;isrc:0.001', 'ajack', [], ['acv', 'dcv', 'res', 'dcv', 'temp', 'dcv', 'aca', 'dcv', 'acv', 'cont', 'acv'],
                   {'dcv': ('AJACK', 1.0), 'acv': ('AJACK', 1.6), 'cont': ('AJACK', 1.0)}),
    'real-aprot': ('open;avsrc:24', 'aprot', [], ['res', 'dcv'], {'dcv': ('APROT', 1.2)}),
    'real-achint': ('open;vsrc:0,ac=2,f=50', 'ac2', [], ['acv', 'dcv', 'res', 'dcv', 'temp', 'dcv'], {'dcv': ('AC', 4.0)}),
    'real-dchint': ('open;vsrc:5', 'dc5', [], ['dcv', 'acv', 'res', 'acv', 'temp', 'acv'], {'acv': ('DC', 6.0)}),
    'real-vjack': ('open;vsrc:24', 'v24', [], ['dcv', 'dca', 'res', 'dca', 'aca', 'dca'], {'dca': ('VJACK', 4.0)}),
}


def fmt(ents, ts):
    return ' '.join('%s/%s@+%.2f' % (e[0], e[1], e[2] - ts) for e in ents)


def run(name):
    spec, inp, mon, seq, real = SC[name]
    seq = seq or (full_seq() if A.full else SEQ)
    if uc.port_busy(A.port):
        print('RESULT %s FAIL port %d in use (another emulator?): pass a free --port' % (name, A.port), flush=True)
        return False
    tag = 'switch-' + name
    rtag = '%s-%d-%d' % (tag, A.port, os.getpid())
    os.makedirs(OUT, exist_ok=True)
    sp = os.path.join(OUT, rtag + '.script')
    with open(sp, 'w') as f:
        for m in mon:
            f.write('0 monitor %s\n' % m)
        f.write('0 dmm %s\n' % spec)
    run_for = 30 + len(seq) * (A.dwell + 4) + sum(5 for s in seq if s == 'WAVE')
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--fast', '--usbip', '3',
           '--usbip-port', str(A.port), '--run-for', str(int(run_for)), '--script', sp, '--name', rtag] \
        + (['--app', os.path.abspath(A.app)] if A.app else [])
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    t_wall = time.time()
    tm = None
    while p.poll() is None and tm is None and time.time() - t_wall < 300:
        try:
            tm = uc.Usbtmc(uc.Dev('1-0'))
        except Exception as e:
            print('attach:', type(e).__name__, e, flush=True)
            time.sleep(1)
    bad, steps = [], []

    def q(line):
        tm.write(line)
        return tm.read(timeout=60).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000.0

    def log():
        s = q('DBG:SENT:LOG?').split(';')
        return int(s[0]), [(a, k, int(t) / 1000.0) for a, k, t in (e.split(',') for e in s[1:])]

    if tm is not None:
        try:
            q('*IDN?')
            while up() < 6.0 and p.poll() is None:  # past the boot's own settle
                time.sleep(0.3)
            prev = None
            seen_n, _ = log()
            for i, st in enumerate(seq):
                if st == 'WAVE':
                    if prev not in WAVE_OK:
                        continue
                    tm.write('DBG:KEY 3')
                    time.sleep(0.6)
                    tm.write('DBG:TAP 237,259')
                    w = up() + 3.0
                    while up() < w and p.poll() is None:
                        time.sleep(0.2)
                    ts = up()
                    tm.write('DBG:KEY 0')
                    fn = prev
                else:
                    ts = up()
                    for c in F[st]:
                        tm.write(c)
                    fn = st
                rec = []
                until = ts + max(A.dwell, real[fn][1] + 1.0 if fn in real else 0) + (2.5 if st in SLOW else 0)
                while p.poll() is None:
                    t = up()
                    rec.append((t, q('SYST:ALER?'), q('DBG:SENT?')))
                    if t >= until:
                        break
                n, ents = log()
                new = ents[-(n - seen_n):] if n > seen_n else []
                if n - seen_n > len(ents):
                    bad.append('step %d %s: %d assertions, the log holds %d' % (i, st, n - seen_n, len(ents)))
                seen_n = n
                ok_set = allowed_for(inp, fn) | {'NONE'}
                wrong = [e for e in new if e[0] not in ok_set] + [(r[1], 'poll', r[0]) for r in rec if r[1] not in ok_set]
                func = q('FUNC?')
                lat = None
                if fn in real and st != 'WAVE':
                    want, lim = real[fn]
                    hit = [e for e in new if e[0] == want] + [(r[1], 'poll', r[0]) for r in rec if r[1] == want]
                    lat = min(e[2] for e in hit) - ts if hit else None
                    # an alert that stayed on through the switch (latched A-PROT): held
                    held = bool(rec) and all(r[1] == want for r in rec) and not new
                    if not held and (lat is None or lat > lim):
                        bad.append('step %d %s<-%s: %s after %s s, want <= %g' % (i, st, prev, want, lat, lim))
                steps.append({'i': i, 'step': st, 'from': prev, 'func': func, 'ts': ts, 'new': new, 'rec': rec, 'lat': lat})
                print('%s %3d %-6s <- %-6s %-14s t %7.2f log %s%s %s' % (
                    name, i, st, prev, func, ts, fmt(new, ts) or '-', '' if lat is None else ' lat %.2f' % lat,
                    'BAD' if wrong else ''), flush=True)
                if wrong:
                    bad.append('step %d %s<-%s: %s' % (i, st, prev, fmt(wrong[:3], ts)))
                prev = fn
        except Exception as e:
            print(name, 'link ended:', type(e).__name__, e, flush=True)
            bad.append('link ended: %s' % e)
    p.terminate()
    try:
        out, _ = p.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        p.kill()
        out, _ = p.communicate()
    for x in (sp, os.path.join(OUT, 'flash-%s.bin' % rtag), os.path.join(OUT, rtag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
    ok = not bad and len(steps) >= len([s for s in seq if s != 'WAVE'])
    with open(os.path.join(OUT, tag + '.json'), 'w') as f:
        json.dump({'name': name, 'ok': ok, 'bad': bad, 'steps': steps}, f, indent=1)
    print('RESULT %s %s %s' % (name, 'OK' if ok else 'FAIL', '; '.join(bad[:6])), flush=True)
    return ok


if __name__ == '__main__':
    if A.list:
        for n, v in SC.items():
            print('%-11s %-26s %s' % (n, v[0], ' '.join('%s:%s<=%gs' % (k, a, t) for k, (a, t) in v[4].items())))
        sys.exit(0)
    allok = True
    for n in A.names or list(SC):
        allok &= run(n)
    sys.exit(0 if allok else 1)
