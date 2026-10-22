#!/usr/bin/env python3
"""fw-rs misconnect sentinel (fw-rs/src/sentinel.rs) in the emulator: fw-rs
native + the HY3131 model, a timed connection script per scenario, SCPI over
USB/IP (CONF:<f>, then DBG:PING? uptime, SYST:ALER?, DBG:SENT?, READ? in a
loop); each phase of the script must show only its expected alerts.

  emu/hy3131/senttest.py                         # all scenarios, fw-rs built from HEAD
  emu/hy3131/senttest.py --app fw-rs/dm40-fw-native.bin --port 3377 dcv-adc acv
  emu/hy3131/senttest.py --list

Results: emu/out/sent-<name>.json (+ .png of the end; Renode log
emu/out/sent-<name>-<port>-<pid>.log). One emulator at a time per port (a
busy port is refused); run several invocations on different ports in
parallel.
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
ap.add_argument('--port', type=int, default=3377, help='USB/IP port (default 3377)')
ap.add_argument('--list', action='store_true')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

# name: (SCPI function, run_for, [(t, spec)], [(t0, t1, allowed alerts)])
# name: (SCPI function, run_for, [(t, spec, allowed alerts in this phase)]); a phase is checked
# from its start + 2 s to the next start - 1.5 s (fw uptime lags the virtual time by ~0.5 s;
# A-JACK clears 1 s after the last hit, V-JACK? needs 1 s + the settle).
SC = {
    'dcv-adc':  ('VOLT:DC', 26, [(0, 'open;vsrc:1.234', {'NONE'}), (10, 'isrc:0.001', {'AJACK'}), (17, 'isrc:none', {'NONE'})]),
    'dcv-neg':  ('VOLT:DC', 18, [(0, 'open;vsrc:1.234', {'NONE'}), (10, 'isrc:-0.001', {'AJACK'})]),
    'dcv-aac':  ('VOLT:DC', 20, [(0, 'open;vsrc:1.234', {'NONE'}), (10, 'isrc:0,ac=0.005,f=50', {'AJACK'})]),
    'dcv-avs':  ('VOLT:DC', 18, [(0, 'open;vsrc:1.234', {'NONE'}), (10, 'avsrc:5', {'AJACK'})]),
    'dcv-small': ('VOLT:DC', 18, [(0, 'open;vsrc:1.234;isrc:0.0002', {'NONE'})]),
    # 9dd1222 device check: A-JACK with 10 V on the 60 V range, A jack open. The model has no
    # such coupling; this checks that the autorange 6 <-> 60 V leaves the r0 sentinel working.
    'dcv-step': ('VOLT:DC', 36, [(0, 'open;vsrc:1', {'NONE'}), (8, 'vsrc:10', {'NONE'}), (14, 'vsrc:1', {'NONE'}),
                                 (20, 'vsrc:10', {'NONE'}), (26, 'vsrc:1;isrc:0.001', {'AJACK'})]),
    'dcmv':     ('VOLT:DC', 20, [(0, 'open;vsrc:0.1', {'NONE'}), (10, 'isrc:0.002', {'AJACK'})]),
    'ohm':      ('RES', 26, [(0, 'open;r:4700', {'NONE'}), (12, 'isrc:0.002', {'AJACK'})]),
    'temp':     ('TEMP', 26, [(0, 'open;tc:100;cj:25', {'NONE'}), (12, 'isrc:0.005', {'AJACK'})]),
    'acv':      ('VOLT:AC', 26, [(0, 'open;vsrc:0,ac=1.0,f=50', {'NONE'}), (10, 'isrc:0,ac=0.01,f=50', {'AJACK'}), (18, 'isrc:none', {'NONE'})]),
    'acv-dc':   ('VOLT:AC', 20, [(0, 'open;vsrc:0,ac=1.0,f=50', {'NONE'}), (10, 'isrc:0.001', {'AJACK'})]),
    'acmv':     ('VOLT:AC', 20, [(0, 'open;vsrc:0,ac=0.05,f=50', {'NONE'}), (10, 'isrc:0,ac=0.005,f=60', {'AJACK'})]),
    'dca-v':    ('CURR:DC', 28, [(0, 'open;vsrc:5', {'NONE'}), (10, 'vsrc:24', {'VJACK'}), (19, 'vsrc:none', {'NONE'})]),
    'dca-vac':  ('CURR:DC', 20, [(0, 'open', {'NONE'}), (10, 'vsrc:0,ac=230,f=50', {'VJACK'})]),
    'dca-vi':   ('CURR:DC', 16, [(0, 'open;vsrc:24;isrc:0.0123', {'NONE'})]),
    'ohm-live': ('RES', 22, [(0, 'open;r:4700', {'NONE'}), (10, 'vsrc:-1.0,rs=100', {'LIVE'})]),
    'cont-live': ('CONT', 18, [(0, 'open;r:10', {'NONE'}), (8, 'open;vsrc:1.234', {'LIVE'})]),
    'diode-live': ('DIOD', 18, [(0, 'open;diode:0.65', {'NONE'}), (8, 'open;vsrc:-1.0,rs=100', {'LIVE'})]),
    'masked':   ('VOLT:DC', 18, [(0, 'open;vsrc:1.234;isrc:0.001', {'NONE'})]),
    # Sentinels added after 9dd1222 (doc 4.7): AC+DC (both phases), AC A (AD1 on the V tap),
    # frequency (AD1 peak on PB1), continuity / diode / cap r0..4 (AD2 + OP2 on PB1); cap r5..7
    # (the ramp) has none: the gap is asserted.
    'acdc-v':   ('VOLT:ACDC', 30, [(0, 'open;vsrc:1.234', {'NONE'}), (10, 'isrc:0.001', {'AJACK'}), (21, 'isrc:none', {'NONE'})]),
    'acdc-vac': ('VOLT:ACDC', 22, [(0, 'open;vsrc:0,ac=1.0,f=50', {'NONE'}), (10, 'isrc:0,ac=0.005,f=50', {'AJACK'})]),
    # AC+DC V above 6 V: both phases without a sentinel (the DC V 60 V+ gap), no flapping
    'acdc-v-hi-gap': ('VOLT:ACDC', 26, [(0, 'open;vsrc:30', {'NONE'}), (10, 'isrc:0.001', {'NONE'})]),
    'acdc-a':   ('CURR:ACDC', 30, [(0, 'open', {'NONE'}), (10, 'vsrc:24', {'VJACK'}), (21, 'vsrc:none', {'NONE'})]),
    'aca-v':    ('CURR:AC', 28, [(0, 'open;vsrc:1', {'NONE'}), (10, 'vsrc:5', {'VJACK'}), (19, 'vsrc:none', {'NONE'})]),
    'aca-vac':  ('CURR:AC', 20, [(0, 'open', {'NONE'}), (10, 'vsrc:0,ac=230,f=50', {'VJACK'})]),
    'aca-vi':   ('CURR:AC', 18, [(0, 'open;vsrc:24;isrc:0,ac=0.01,f=50', {'NONE'})]),
    # Frequency carries no sentinel (device f3aea2f: PB1 - PB3 stands at ~54 mV in its row, a
    # false A-JACK; doc 4.7 gaps): a current in A shows nothing there.
    'freq-gap': ('FREQ', 24, [(0, 'open;vsrc:0,ac=1,f=1000', {'NONE'}), (10, 'isrc:0.001', {'NONE'})]),
    'cont-a':   ('CONT', 24, [(0, 'open;r:10', {'NONE'}), (10, 'isrc:0.002', {'AJACK'}), (17, 'isrc:none', {'NONE'})]),
    'diode-a':  ('DIOD', 24, [(0, 'open;diode:0.65', {'NONE'}), (10, 'isrc:0,ac=0.005,f=50', {'AJACK'}), (17, 'isrc:none', {'NONE'})]),
    'cap-a':    ('CAP', 26, [(0, 'open;c:100e-9', {'NONE'}), (12, 'isrc:0.002', {'AJACK'})]),
    'cap-ramp-gap': ('CAP', 30, [(0, 'open;c:100e-6', {'NONE'}), (14, 'isrc:0.002', {'NONE'})]),
    # Hints (doc 4.7): DC? in AC V (AD1 alternates PB1 / the V tap), AC? in DC V 6 V (the
    # comparator alternates PB1 / SENSE), FUSE? after 60 s at zero in an A function.
    'acv-dchint': ('VOLT:AC', 32, [(0, 'open;vsrc:0,ac=1,f=50', {'NONE'}), (10, 'vsrc:5', {'NONE', 'DC'}),
                                   (16, 'vsrc:5', {'DC'}), (23, 'vsrc:0,ac=1,f=50', {'NONE'})]),
    'acv-dc-ac': ('VOLT:AC', 20, [(0, 'open;vsrc:5,ac=1,f=50', {'NONE'})]),
    'acv-dc-ajack': ('VOLT:AC', 26, [(0, 'open;vsrc:5', {'NONE', 'DC'}), (10, 'isrc:0.001', {'AJACK'}), (18, 'isrc:none', {'NONE', 'DC'})]),
    'dcv-achint': ('VOLT:DC', 32, [(0, 'open;vsrc:1', {'NONE'}), (10, 'vsrc:0,ac=2,f=50', {'NONE', 'AC'}),
                                   (15, 'vsrc:0,ac=2,f=50', {'AC'}), (23, 'vsrc:1', {'NONE'})]),
    'dcv-ac-ajack': ('VOLT:DC', 24, [(0, 'open;vsrc:0,ac=2,f=50', {'NONE', 'AC'}), (10, 'isrc:0.001', {'AJACK'})]),
    'fuse-hint': ('CURR:DC', 76, [(0, 'open;fuseopen', {'NONE'}), (62, 'open;fuseopen', {'NONE', 'FUSE'}),
                                  (66, 'open;fuseopen', {'FUSE'})]),
    # Fuse check (SYST:FUSE:TEST, doc 4.7): a lead V -> A, the fuse intact / blown, a short V-COM.
    'fuse-ok':   ('CURR:DC', 20, [(0, 'open;valead', {'NONE'})], ['SYST:FUSE:TEST'],
                  {'queries': ['SYST:FUSE?', 'FUNC?'], 'fuse': 'OK', 'ohm': (44, 54)}),
    # the same through the UI: MF -> Current Settings -> Fuse check -> Start
    'fuse-ui':   ('CURR:DC', 22, [(0, 'open;valead', {'NONE'})], ['DBG:KEY 3', 'DBG:TAP 237,159', 'DBG:TAP 344,210'],
                  {'queries': ['SYST:FUSE?', 'FUNC?'], 'fuse': 'OK', 'ohm': (44, 54)}),
    'fuse-open': ('CURR:DC', 20, [(0, 'open;valead;fuseopen', {'NONE'})], ['SYST:FUSE:TEST'],
                  {'queries': ['SYST:FUSE?', 'FUNC?'], 'fuse': 'OPEN'}),
    'fuse-odd':  ('CURR:DC', 20, [(0, 'open;r:0.5', {'NONE'})], ['SYST:FUSE:TEST'],
                  {'queries': ['SYST:FUSE?', 'FUNC?'], 'fuse': 'FAIL', 'ohm': (0, 2)}),
    # OVERLOAD (doc 4.7): the uA / mA shunt over range -> the 10 A shunt at once, latched; a
    # phase may carry a READ? window (lo, hi) in A. AUTO: from mA r0 through r1 to 10 A, the
    # floor keeps the mA shunt out until 50 mA < 0.3 A held 5 s, then back to mA.
    'ovl-dcma': ('CURR:DC', 40, [(0, 'open;isrc:0.01', {'NONE'}, (0.0099, 0.0101)),
                                 (10, 'isrc:2', {'OVERLOAD'}, (1.98, 2.02)),
                                 (20, 'isrc:0.05', {'OVERLOAD', 'NONE'}),
                                 (32, 'isrc:0.05', {'NONE'}, (0.0499, 0.0501))]),
    # Manual 600 uA (Current Settings page): 10 mA -> 10 A (manual); 0 A for 5 s -> back to
    # 600 uA manual (100 uA reads with uA resolution again); 1 mA within 30 s of that release
    # trips again and the latch is sticky (0 A no longer releases it).
    'ovl-dcua-man': ('CURR:DC', 56, [(0, 'open;isrc:0.0001', {'NONE'}, (99e-6, 101e-6)),
                                     (12, 'isrc:0.01', {'OVERLOAD'}, (0.008, 0.012)),
                                     (22, 'isrc:none', {'OVERLOAD', 'NONE'}),
                                     (30, 'isrc:0.0001', {'NONE'}, (99.5e-6, 100.5e-6)),
                                     (38, 'isrc:0.001', {'OVERLOAD'}),
                                     (44, 'isrc:none', {'OVERLOAD'})],
                     ['DBG:KEY 3', 'DBG:TAP 64,70', 'DBG:KEY 3']),
    'ovl-acma': ('CURR:AC', 26, [(0, 'open;isrc:0,ac=0.01,f=50', {'NONE'}, (0.0098, 0.0102)),
                                 (10, 'isrc:0,ac=2,f=50', {'OVERLOAD'}, (1.95, 2.05))]),
    'ovl-acdc': ('CURR:ACDC', 30, [(0, 'open;isrc:0.01', {'NONE'}), (12, 'isrc:2', {'OVERLOAD'})]),
    # Power meter, I fixed on 600 mA (CONF:POW AUTO,0.6): 2 A -> ik 5.
    'ovl-pow':  ('POW AUTO,0.6', 30, [(0, 'open;pow:230,i=0.1,f=50', {'NONE'}), (12, 'pow:230,i=2,f=50', {'OVERLOAD'})], ['POW:COUP ACDC']),
    # Power meter, I AUTO (owner: 3 A through A showed OVLD): an over range on the mA / uA
    # shunt is the normal up-range to the next shunt, no alert in any sample (READ? = P).
    'ovl-pow-auto-ma': ('POW', 30, [(0, 'open;pow:230,i=0.01,f=50', {'NONE'}, (2.25, 2.35)),
                                    (12, 'pow:230,i=3,f=50', {'NONE'}, (680, 700))], ['POW:COUP ACDC']),
    'ovl-pow-auto-ua': ('POW', 34, [(0, 'open;pow:230,i=0.0002,f=50', {'NONE'}, (0.044, 0.048)),
                                    (16, 'pow:230,i=3,f=50', {'NONE'}, (680, 700))], ['POW:COUP ACDC']),
    'ovl-pow-auto-dc': ('POW', 30, [(0, 'open;vsrc:12;isrc:0.01', {'NONE'}, (0.118, 0.122)),
                                    (12, 'vsrc:12;isrc:3', {'NONE'}, (35.6, 36.4))], ['POW:COUP DC']),
    # A-jack protection (doc 4.8): a source across A-COM in DC V / ohm / temperature. 5 V (104 mA
    # in the 47.5 ohm chain) is a warning only; 12 V (250 mA) moves the A jack to the 10 mohm
    # shunt 0.5 s after the current started, 24 V (0.5 A) at the first probe window. The latch
    # (APROT) holds until the user acts (CONF:... at `acts`) and the current is gone.
    # 'aprot_ms': the trip latency window (DBG:SENT? field 11, ms from the first hit window).
    'aprot-dcv-5': ('VOLT:DC', 24, [(0, 'open', {'NONE'}), (8, 'avsrc:5', {'AJACK'})]),
    'aprot-dcv-12': ('VOLT:DC', 32, [(0, 'open', {'NONE'}), (8, 'avsrc:12', {'APROT'}), (16, 'open', {'APROT'}),
                                     (22, 'open', {'NONE'})], [], {'acts': {19.5: 'DBG:KEY 2'}, 'aprot_ms': (400, 560)}),
    'aprot-dcv-24': ('VOLT:DC', 22, [(0, 'open', {'NONE'}), (8, 'avsrc:24', {'APROT'})], [], {'aprot_ms': (100, 450)}),
    # Ohm / temperature carry no probe (device 1eb4d7e: there it follows the V input, 5 V on V-COM
    # read 1.7 A and latched A-PROT): a source across A-COM is A-JACK only.
    'aprot-ohm-12': ('RES', 24, [(0, 'open;r:4700', {'NONE'}), (10, 'avsrc:12', {'AJACK'})]),
    'aprot-temp-12': ('TEMP', 26, [(0, 'open;tc:100;cj:25', {'NONE'}), (12, 'avsrc:12', {'AJACK'})]),
    # The user acts (another function) while the source is still there: the latch stays; the current goes later and
    # the latch ends 1 s after that (the act counts).
    'aprot-ack-early': ('VOLT:DC', 34, [(0, 'open', {'NONE'}), (8, 'avsrc:12', {'APROT'}), (16, 'avsrc:12', {'APROT'}),
                                        (22, 'open', {'APROT', 'NONE'}), (26, 'open', {'NONE'})], [], {'acts': {14: 'DBG:KEY 2'}}),
    # Device f3aea2f: A-PROT latched with the A jack open on the probe's first window (AD2 just
    # restarted). The model's AD2 reads 3000 codes (0.72 A) off for its first results after a
    # start: a small current (1 V across A-COM, 21 mA) brings the probe in; A-JACK only.
    'aprot-glitch': ('VOLT:DC', 22, [(0, 'open', {'NONE'}), (8, 'avsrc:1', {'AJACK'})], [],
                     {'monitor': ['sysbus.hy3131 Ad2StartGlitch 3000']}),
    # A current function ends the latch (its own shunt switching takes over).
    'aprot-to-dca': ('VOLT:DC', 30, [(0, 'open', {'NONE'}), (8, 'avsrc:12', {'APROT'}), (18, 'avsrc:12', {'NONE', 'OVERLOAD'})],
                     [], {'acts': {16: 'CONF:CURR:DC'}}),
}


def run(name):
    func, run_for, phases = SC[name][:3]
    setup = SC[name][3] if len(SC[name]) > 3 else []
    xtra = SC[name][4] if len(SC[name]) > 4 else {}
    script = [(ph[0], ph[1]) for ph in phases]
    expect = []
    for i, ph in enumerate(phases):
        t = ph[0]
        t1 = phases[i + 1][0] - 1.5 if i + 1 < len(phases) else run_for
        expect.append((t + 2.0 if t > 0 else 5.0, t1, ph[2], ph[3] if len(ph) > 3 else None))
    if uc.port_busy(A.port):
        print('RESULT %s FAIL port %d in use (another emulator?): pass a free --port' % (name, A.port), flush=True)
        return False
    tag = 'sent-' + name
    # The run's own files (event script, Renode log, flash image, .resc) per
    # process: parallel runs (distinct --port) share none; results under tag.
    rtag = '%s-%d-%d' % (tag, A.port, os.getpid())
    os.makedirs(OUT, exist_ok=True)
    sp = os.path.join(OUT, rtag + '.script')
    with open(sp, 'w') as f:
        for m in xtra.get('monitor', []):
            f.write('0 monitor %s\n' % m)
        for t, spec in script:
            f.write('%g dmm %s\n' % (t, spec))
    # USB/IP from 3 s: an attach before fw-rs enumerated can leave the server stuck
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--fast', '--usbip', '3',
           '--usbip-port', str(A.port), '--run-for', str(run_for), '--script', sp, '--name', rtag,
           '--shot', os.path.join(OUT, tag + '.png')] + (['--app', os.path.abspath(A.app)] if A.app else [])
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    rows = []
    t_wall = time.time()
    tm = None
    while p.poll() is None and tm is None and time.time() - t_wall < 300:
        try:
            d = uc.Dev('1-0')
            tm = uc.Usbtmc(d)
        except Exception as e:
            print('attach:', type(e).__name__, e, flush=True)
            time.sleep(1)
    if tm is not None:
        try:
            if name == 'masked':
                tm.write('SYST:ALER:ENAB 6')
            tm.write('CONF:' + func)
            for c in setup:
                time.sleep(1.0)
                tm.write(c)
            acts = dict(xtra.get('acts', {}))
            while p.poll() is None:
                row = {}
                for q in ['DBG:PING?', 'SYST:ALER?', 'DBG:SENT?', 'STAT:QUES:COND?', 'READ?'] + xtra.get('queries', []):
                    tm.write(q)
                    row[q] = tm.read(timeout=60).decode(errors='replace').strip()
                row['t'] = int(row['DBG:PING?'].split(',')[0]) / 1000.0
                rows.append(row)
                for ta in sorted(acts):
                    if row['t'] >= ta:
                        tm.write(acts.pop(ta))
                print('%s %6.2f %-6s %-40s %s %s' % (name, row['t'], row['SYST:ALER?'], row['DBG:SENT?'], row['READ?'],
                                                   ' '.join(row[q] for q in xtra.get('queries', []))), flush=True)
        except Exception as e:
            print(name, 'link ended:', e, flush=True)
    out, _ = p.communicate()
    for x in (sp, os.path.join(OUT, 'flash-%s.bin' % rtag), os.path.join(OUT, rtag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
    ok = True
    bad = []
    for t0, t1, allowed, win in expect:
        seen = [r for r in rows if t0 <= r['t'] <= t1]
        if not seen:
            ok = False
            bad.append('no samples in %g..%g' % (t0, t1))
        for r in seen:
            if r['SYST:ALER?'] not in allowed:
                ok = False
                bad.append('%.2f %s not in %s' % (r['t'], r['SYST:ALER?'], sorted(allowed)))
            # STAT:QUES:COND? bit 9 = a red alert, bit 1 = OVERLOAD
            want = (512 if r['SYST:ALER?'] in ('AJACK', 'OVERLOAD', 'APROT') else 0) | (2 if r['SYST:ALER?'] == 'OVERLOAD' else 0)
            if len(allowed) == 1 and r.get('STAT:QUES:COND?') != str(want):
                ok = False
                bad.append('%.2f QUES %s for %s' % (r['t'], r.get('STAT:QUES:COND?'), r['SYST:ALER?']))
            if win:
                try:
                    v = float(r['READ?'])
                except ValueError:
                    v = float('nan')
                if not (win[0] <= abs(v) <= win[1]):
                    ok = False
                    bad.append('%.2f READ? %s outside %g..%g' % (r['t'], r['READ?'], win[0], win[1]))
            # DC V 60 V+ carries no comparator sentinel (9dd1222 device check)
            try:
                v = float(r['READ?'])
            except ValueError:
                v = 0.0
            if name == 'dcv-step' and 7.0 < v < 1e6 and not r['DBG:SENT?'].startswith('OFF'):
                ok = False
                bad.append('%.2f %g V: sentinel %s on the 60 V range' % (r['t'], v, r['DBG:SENT?'].split(',')[0]))
    if 'aprot_ms' in xtra:
        # the trip latency the firmware recorded (DBG:SENT? field 11)
        ms = [int(r['DBG:SENT?'].split(',')[10]) for r in rows if r['SYST:ALER?'] == 'APROT' and len(r['DBG:SENT?'].split(',')) > 10]
        lo, hi = xtra['aprot_ms']
        if not ms or not (lo <= ms[-1] <= hi):
            ok = False
            bad.append('A-PROT latency %s ms, want %d..%d' % (ms[-1:] or None, lo, hi))
        else:
            print('  A-PROT latency %d ms' % ms[-1], flush=True)
    if 'fuse' in xtra:
        # the verdict came, the function came back (CURR), the ohm value in its window
        fz = [r['SYST:FUSE?'] for r in rows if 'SYST:FUSE?' in r]
        done = [f for f in fz if f not in ('BUSY', 'IDLE')]
        if not done or not done[-1].startswith(xtra['fuse']):
            ok = False
            bad.append('SYST:FUSE? %s, want %s' % (fz[-6:], xtra['fuse']))
        elif 'ohm' in xtra:
            v = float(done[-1].split(',')[1])
            if not (xtra['ohm'][0] <= v <= xtra['ohm'][1]):
                ok = False
                bad.append('fuse ohm %g outside %s' % (v, xtra['ohm']))
        if 'BUSY' not in fz:
            ok = False
            bad.append('never BUSY')
        if not rows or rows[-1].get('FUNC?') != '"CURR"':
            ok = False
            bad.append('function after the check: %s' % (rows[-1].get('FUNC?') if rows else None))
    res = {'name': name, 'ok': ok, 'bad': bad, 'rows': rows, 'rc': p.returncode, 'log': out[-600:] if p.returncode else ''}
    with open(os.path.join(OUT, tag + '.json'), 'w') as f:
        json.dump(res, f, indent=1)
    print('RESULT %s %s %s' % (name, 'OK' if ok else 'FAIL', '; '.join(bad[:5])), flush=True)
    return ok


if __name__ == '__main__':
    if A.list:
        for n, v in SC.items():
            print('%-11s %-8s %s' % (n, v[0], ' | '.join('%g %s -> %s' % (x[0], x[1], '/'.join(sorted(x[2]))) for x in v[2])))
        sys.exit(0)
    allok = True
    for n in A.names or list(SC):
        allok &= run(n)
    sys.exit(0 if allok else 1)
