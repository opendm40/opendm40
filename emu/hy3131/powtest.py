#!/usr/bin/env python3
"""fw-rs power meter (fw-rs/src/powermeter.rs) in the emulator: fw-rs native +
the HY3131 model's Power role, a timed connection script per scenario, SCPI
over USB/IP (setup commands, then DBG:PING? uptime, FETCh:POWer:ALL?,
POW:VOLT:RANG?, POW:CURR:RANG? in a loop); the last samples of every phase
must match its expectation.

  emu/hy3131/powtest.py                          # all scenarios, fw-rs built from HEAD
  emu/hy3131/powtest.py --app fw-rs/dm40-fw-native.bin --port 3381 mains
  emu/hy3131/powtest.py --list

FETC:POW:ALL? fields: P,S,Q,PF,Vrms,Irms,F,E_Wh,seconds,Vdc,Idc,VAh,burden_V.
Results: emu/out/pow-<name>.json (+ .png of the end). One emulator per port.
"""
import argparse
import json
import math
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
ap.add_argument('--port', type=int, default=3381, help='USB/IP port (default 3381)')
ap.add_argument('--list', action='store_true')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

F = ['P', 'S', 'Q', 'PF', 'V', 'I', 'F', 'Wh', 'secs', 'Vdc', 'Idc', 'VAh', 'Vb']


def near(x, want, rel, ab=0.0):
    return x is not None and not math.isnan(x) and abs(x - want) <= max(abs(want) * rel, ab)


# Expectation helpers: d = the FETC:POW:ALL? fields of a sample.
def pw(p, pf, v, i, rel=0.01, f=None):
    def chk(d):
        ok = near(d['P'], p, rel, 0.002 * abs(v * i)) and near(d['PF'], pf, 0.0, 0.01) and near(d['V'], v, rel) and near(d['I'], i, rel)
        return ok and (f is None or near(d['F'], f, 0.01))
    return chk


def reactive(v, i):
    return lambda d: near(d['V'], v, 0.01) and near(d['I'], i, 0.01) and abs(d['PF']) < 0.02 and near(d['Q'], v * i, 0.02)


def alt(v, i):
    # Fallback: S only; P / Q / PF are NaN (9.91E37 on the wire).
    return lambda d: near(d['S'], v * i, 0.015) and near(d['V'], v, 0.01) and near(d['I'], i, 0.01) and math.isnan(d['PF'])


def ranges(vr, ir):
    return lambda d: d['VR'].endswith(vr) and d['IR'].endswith(ir)


def both(*fs):
    return lambda d: all(f(d) for f in fs)


# name: (setup SCPI, run_for, [(t, spec, check, label)]); a phase is checked on
# its samples from the next phase's start - 4.5 s to that start - 1.5 s (the
# fw uptime lags the virtual time by ~0.5 s or more).
SC = {
    'mains': (['CONF:POW', 'POW:BURD:STAT OFF'], 58, [
        (0, 'open;pow:230@50,i=1,pf=1', both(pw(230, 1, 230, 1, f=50), ranges('6.000000E+02', '1.000000E+01')), 'resistive 230 V 1 A: PF 1, 50 Hz, 600 V / 10 A'),
        (14, 'open;pow:230@50,i=1,pf=0.5', pw(115, 0.5, 230, 1), 'PF 0.5'),
        (28, 'open;pow:230@50,i=1,ph=90', reactive(230, 1), 'reactive (90 deg): PF 0'),
        (43, 'open;pow:230@50,i=1,pf=-1', pw(-230, -1, 230, 1), 'reversed current: P < 0'),
    ]),
    'dc': (['CONF:POW', 'POW:BURD:STAT OFF'], 50, [
        (0, 'open;vsrc:12;isrc:2', both(pw(24, 1, 12, 2), lambda d: near(d['Vdc'], 12, 0.01) and near(d['Idc'], 2, 0.01)), 'DC 12 V 2 A: 24 W (LPFs)'),
        (16, 'open;vsrc:12;isrc:-2', pw(-24, -1, 12, 2), 'DC source: -24 W'),
        (32, 'open;vsrc:5;isrc:0.4', both(pw(2.0, 1, 5, 0.4), ranges('6.000000E+00', '6.000000E-01')), 'DC 5 V 0.4 A: autorange 6 V / 600 mA'),
    ]),
    'small': (['CONF:POW', 'POW:BURD:STAT OFF'], 44, [
        (0, 'open;pow:5@50,i=0.01,pf=1', both(pw(0.05, 1, 5, 0.01), ranges('6.000000E+00', '6.000000E-02')), '5 V 10 mA: 6 V / 60 mA'),
        (22, 'open;pow:5@50,i=0.002,pf=0.8', both(pw(0.008, 0.8, 5, 0.002, rel=0.015), ranges('6.000000E+00', '6.000000E-03')), '5 V 2 mA PF 0.8: 6 mA (uA shunt)'),
    ]),
    'burden': (['CONF:POW', 'POW:BURD 0.5'], 40, [
        # 5 V, 0.4 A on the mA shunt; the model's V jack is ideal, so with
        # R_path 0.5 ohm the load values are P = 2 - 0.08, V = sqrt(25 - 2 + 0.04).
        (0, 'open;vsrc:5;isrc:0.4', both(pw(1.92, 1, math.sqrt(25 - 2 * 0.5 * 2 + 0.04), 0.4, rel=0.01), lambda d: near(d['Vb'], 0.2, 0.01)), 'burden compensation 0.5 ohm'),
    ]),
    'manual': (['CONF:POW 60,6', 'POW:BURD:STAT OFF'], 24, [
        (0, 'open;pow:5@50,i=0.1,pf=1', both(pw(0.5, 1, 5, 0.1, rel=0.02), ranges('6.000000E+01', '6.000000E+00')), 'manual 60 V / 6 A held'),
    ]),
    'alt': (['CONF:POW', 'POW:METH ALT'], 40, [
        (0, 'open;pow:230@50,i=1,pf=0.5', alt(230, 1), 'fallback: S 230 VA, PF n/a'),
    ]),
    'phasecal': (['CONF:POW', 'POW:BURD:STAT OFF', 'POW:PHAS 40'], 60, [
        # 1 kHz, current lagging 90 deg: SPHACAL 40 makes P visible (6 deg);
        # POW:PHAS:CAL at 20 s nulls it.
        (0, 'open;pow:10@1000,i=0.1,ph=90', lambda d: abs(d['PF']) > 0.05, 'SPHACAL 40 at 1 kHz: PF off zero'),
        (20, None, lambda d: abs(d['PF']) < 0.02 and abs(d['PH']) <= 3, 'after POW:PHAS:CAL: PF ~0, SPHACAL ~0'),
    ]),
    'energy': (['CONF:POW', 'POW:BURD:STAT OFF', 'POW:ENER:RES'], 40, [
        (0, 'open;vsrc:10;isrc:1', lambda d: d['secs'] > 5 and near(d['Wh'] * 3600 / d['secs'], 10, 0.05) and near(d['VAh'] * 3600 / d['secs'], 10, 0.05), 'energy: 10 W x t'),
    ]),
    # The reset confirm answered 13 s after the long press on the energy box
    # (device check 5f14d38: the 10 s auto-cancel ate that Yes).
    'ereset': (['CONF:POW', 'POW:BURD:STAT OFF', 'POW:ENER:RES'], 40, [
        (0, 'open;vsrc:10;isrc:1', lambda d: d['secs'] > 8, 'energy runs, the confirm up from 8 s'),
        (22, None, lambda d: d['secs'] < 20, 'Yes at 21 s (13 s after the long press) reset the energy'),
    ]),
}

# Device-test hooks (doc 3.11, T1..T3): the raw ring with R34 frozen at YY
# (T1: V on OP2 / AD3 alone) and with no discards (T3: every result logged).
def ring(stats, disc):
    def chk(d):
        r = d.get('RAW', '').split(';')[1:]
        e = [x.split(',') for x in r]
        return len(e) >= 8 and all(x[0] in stats for x in e) and all(x[1] == disc for x in e) and (len(stats) == 1 or len({x[0] for x in e}) == len(stats))
    return chk


SC.update({
    't1-yy': (['CONF:POW', 'DBG:POW:STAT YY'], 20, [
        (0, 'open;pow:230@50,i=1,pf=1', lambda d: ring({'YY'}, '0')(d) and d['ROW'].split()[20] == '8C', 'T1 hook: R34 frozen at YY, ring all YY'),
    ]),
    't3-cycle': (['CONF:POW', 'DBG:POW:DISC 0'], 20, [
        # d = 0: every result after an R34 switch is taken; with the model's
        # mid-block mixing (powmix) the values are then off, which is what
        # the discards are for (on the device T3 measures the real cost).
        (0, 'open;pow:230@50,i=1,pf=1', ring({'XY', 'YY', 'XX'}, '0'), 'T3 hook: d = 0, XY / YY / XX every result'),
    ]),
})
# Follow-ups (doc 3.11 "Follow-ups"): LPF zeros (powz:1 = the device's
# zeros) and POW:ZERO, negative DC, the V self-cal (powvg: a Y gain error the
# factory calibration does not explain), the counter's window / PB4, the DC
# (AD1) method and the AUTO coupling.
def x(q, want):
    return lambda d: d['X'].get(q) == want


def zeros(want):
    # POW:ZERO? = result,SET,<Y>,<X 600 uA .. 10 A> within 3 codes (noise),
    # <the DC row's AD1 zeros>: powz's 300 + the folded factory DC A offsets
    # (-80..+40 codes) + AD1 noise.
    def chk(d):
        z = d['X'].get('POW:ZERO?', '').split(',')
        return len(z) == 2 + len(want) and z[:2] == ['OK', 'SET'] and all(abs(int(a) - b) <= tol for a, b, tol in zip(z[2:], want, [3] * 7 + [130] * 6))
    return chk


SC.update({
    'zero': (['CONF:POW', 'POW:BURD:STAT OFF'], 66, [
        (0, 'open;powz:1', lambda d: d['V'] > 0.04 and d['I'] > 5e-6, 'device zeros, no POW:ZERO: phantom V / I'),
        (16, None, lambda d: d['V'] < 0.003 and d['I'] < 1e-6 and zeros([1080, 888, 155, 888, 155, 888, 155] + [300] * 6)(d), 'after POW:ZERO: V, I ~0'),
        (34, 'open;powz:1;vsrc:5;isrc:0.4', both(pw(2.0, 1, 5, 0.4), lambda d: near(d['Vdc'], 5, 0.002)), 'DC 5 V 0.4 A with the zeros subtracted'),
        (50, 'open;powz:1;vsrc:-5;isrc:0.4', both(pw(-2.0, -1, 5, 0.4), lambda d: near(d['Vdc'], -5, 0.002) and near(d['Idc'], 0.4, 0.002)), 'negative DC: LPF polarity, P < 0'),
    ]),
    'vcal': (['CONF:POW', 'POW:BURD:STAT OFF', 'POW:VOLT:RANG 60'], 64, [
        (0, 'open;powvg:0.99;pow:30@50,i=0.1', lambda d: near(d['V'], 29.7, 0.002), 'Y gain 1 % low: V 29.7'),
        (12, None, lambda d: near(d['V'], 30, 0.002) and d['X']['POW:VOLT:CAL?'].startswith('OK,+6.0'), 'after POW:VOLT:CAL (AC V reference): V 30.00'),
        (26, 'open;powvg:0.98;vsrc:20', lambda d: near(d['Vdc'], 20 * 0.98 / 0.99, 0.002), 'DC, Y gain 2 % low (the AC cal of 1 % kept)'),
        (36, None, lambda d: near(d['Vdc'], 20, 0.002) and d['X']['POW:VOLT:CAL?'].startswith('OK'), 'after POW:VOLT:CAL (DC V reference): 20.00 V'),
        (50, 'open;vsrc:0.2', lambda d: d['X']['POW:VOLT:CAL?'].startswith('SMALL'), 'refused: 0.2 V on 60 V'),
    ]),
    # Device f3aea2f: a 0..9.6 V 40 Hz square on 6 V was calibrated by its 4.8 V DC part (DC kind)
    # though its peak clips the range; the peak guard refuses it (BIG), the gain stays.
    'vcal-clip': (['CONF:POW', 'POW:BURD:STAT OFF', 'POW:VOLT:RANG 6'], 30, [
        (0, 'open;vsrc:4.8,ac=4.8,f=40,shape=square', lambda d: True, 'square 0..9.6 V on 6 V'),
        (16, None, lambda d: d['X']['POW:VOLT:CAL?'].startswith('BIG'), 'POW:VOLT:CAL refused: BIG (the peak clips 6 V)'),
    ]),
    'freq': (['CONF:POW', 'POW:BURD:STAT OFF'], 36, [
        (0, 'open;vsrc:4.8,ac=4.8,f=40,shape=square', lambda d: near(d['F'], 40, 0.01), 'unipolar 40 Hz square (T1): f 40 Hz'),
        (18, 'open;vsrc:12;isrc:0,ac=3,f=50', lambda d: near(d['F'], 50, 0.01), 'DC voltage, AC current: f of the current (PB4)'),
    ]),
    'dcad1': (['CONF:POW', 'POW:BURD:STAT OFF', 'POW:COUP DC'], 64, [
        (0, 'open;vsrc:12;isrc:2', both(pw(24, 1, 12, 2, rel=0.001), lambda d: d['F'] == 0 and x('POW:COUP?', 'DC')(d)), 'AD1 method: 12 V 2 A = 24.00 W'),
        (16, 'open;vsrc:12;isrc:-2', pw(-24, -1, 12, 2, rel=0.001), 'reversed: -24.00 W'),
        (32, 'open;vsrc:5;isrc:0.004', both(pw(0.02, 1, 5, 0.004, rel=0.002), ranges('6.000000E+00', '6.000000E-03')), '5 V 4 mA on 6 mA'),
        (48, 'open;vsrc:5;isrc:200e-6', both(pw(1e-3, 1, 5, 200e-6, rel=0.003), ranges('6.000000E+00', '6.000000E-04')), '5 V 200 uA on 600 uA'),
    ]),
    'dcburden': (['CONF:POW', 'POW:BURD 0.5', 'POW:COUP DC'], 30, [
        (0, 'open;vsrc:5;isrc:0.4', both(pw(1.92, 1, math.sqrt(25 - 2 * 0.5 * 2 + 0.04), 0.4, rel=0.002), lambda d: near(d['Vb'], 0.2, 0.01)), 'AD1 method: burden compensation 0.5 ohm'),
    ]),
    # The former AUTO coupling: POW:COUP AUTO is accepted as AC+DC (doc 3.14).
    'auto': (['CONF:POW', 'POW:BURD:STAT OFF', 'POW:COUP AUTO'], 36, [
        (0, 'open;vsrc:12;isrc:2', both(pw(24, 1, 12, 2, rel=0.005), x('POW:COUP?', 'ACDC')), 'AUTO -> AC+DC: DC 24 W'),
        (18, 'open;pow:230@50,i=1,pf=1', both(pw(230, 1, 230, 1, f=50), x('POW:COUP?', 'ACDC')), 'AC+DC: mains 230 W'),
    ]),
    # AC coupling (doc 3.14): the AC parts only. Mains as AC+DC; a DC source
    # reads ~0 (the DC parts and their zeros are not used); POW:COUP ACDC at
    # 46 s: the same DC source 24 W.
    'ac': (['CONF:POW', 'POW:BURD:STAT OFF', 'POW:COUP AC'], 62, [
        (0, 'open;pow:230@50,i=0.5,pf=0.5', both(pw(57.5, 0.5, 230, 0.5, f=50), x('POW:COUP?', 'AC'), lambda d: math.isnan(d['Vdc']) and math.isnan(d['Idc'])), 'AC: mains PF 0.5, Vdc / Idc n/a'),
        (16, 'open;vsrc:12;isrc:0.4', lambda d: abs(d['P']) < 0.05 and d['V'] < 0.2 and d['I'] < 0.01, 'AC: DC 12 V 0.4 A reads ~0'),
        (32, 'open;vsrc:12,ac=2,f=50;isrc:0.4', lambda d: near(d['V'], 2, 0.03) and abs(d['P']) < 0.1, 'AC: the ripple only (DC current: P ~0)'),
        (46, None, both(pw(4.8, 4.8 / (math.hypot(12, 2) * 0.4), math.hypot(12, 2), 0.4, rel=0.01), x('POW:COUP?', 'ACDC')), 'POW:COUP ACDC: the total'),
    ]),
    # Owner: ~3 A through A showed OVLD. I AUTO walks up across the shunts
    # with no alert (SYST:ALER? NONE in every checked sample).
    'istep': (['CONF:POW', 'POW:BURD:STAT OFF'], 64, [
        (0, 'open;pow:230@50,i=0.01,pf=1', both(pw(2.3, 1, 230, 0.01), ranges('6.000000E+02', '6.000000E-02'), x('SYST:ALER?', 'NONE')), '10 mA: 60 mA range'),
        (16, 'open;pow:230@50,i=0.1,pf=1', both(pw(23, 1, 230, 0.1), ranges('6.000000E+02', '6.000000E-01'), x('SYST:ALER?', 'NONE')), '0.1 A: 600 mA'),
        (32, 'open;pow:230@50,i=1,pf=1', both(pw(230, 1, 230, 1), ranges('6.000000E+02', '6.000000E+00'), x('SYST:ALER?', 'NONE')), '1 A: 6 A (mA shunt -> A shunt, no OVLD)'),
        (48, 'open;pow:230@50,i=3,pf=1', both(pw(690, 1, 230, 3), ranges('6.000000E+02', '6.000000E+00'), x('SYST:ALER?', 'NONE')), '3 A: 6 A, no OVLD'),
    ]),
    # From the uA shunt straight to 3 A: one hop to the A shunt, no alert.
    'istep-ua': (['CONF:POW', 'POW:BURD:STAT OFF'], 40, [
        (0, 'open;pow:230@50,i=0.0002,pf=1', both(pw(0.046, 1, 230, 0.0002, rel=0.02), ranges('6.000000E+02', '6.000000E-04'), x('SYST:ALER?', 'NONE')), '200 uA: 600 uA range'),
        (22, 'open;pow:230@50,i=3,pf=1', both(pw(690, 1, 230, 3), ranges('6.000000E+02', '6.000000E+00'), x('SYST:ALER?', 'NONE')), '3 A: 6 A, no OVLD'),
    ]),
    # Manual 600 mA: 3 A is an OVERLOAD (red, latched on 10 A).
    'iman-ovl': (['CONF:POW AUTO,0.6', 'POW:BURD:STAT OFF'], 34, [
        (0, 'open;pow:230@50,i=0.1,pf=1', both(pw(23, 1, 230, 0.1), x('SYST:ALER?', 'NONE')), 'manual 600 mA: 0.1 A'),
        (16, 'open;pow:230@50,i=3,pf=1', both(pw(690, 1, 230, 3), ranges('6.000000E+02', '1.000000E+01'), x('SYST:ALER?', 'OVERLOAD')), 'manual: 3 A -> OVERLOAD, 10 A'),
    ]),
})
SC['scpi'] = (['MEAS:POW?', 'POW:LIM 2.5,12', 'POW:BURD:STAT OFF'], 24, [
    (0, 'open;pow:100@60,i=0.5,pf=0.6', lambda d: near(num(d['X']['MEAS:POW:PFAC?']), 0.6, 0, 0.01) and near(num(d['X']['READ?']), 30, 0.01)
     and d['X']['CONF?'] == '"POW AUTO,AUTO"' and d['X']['POW:LIM?'] == '+2.500000E+00,12' and d['X']['POW:CYC?'] == '2,642,1284'
     and d['X']['FUNC?'] == '"POW"', 'MEAS:POW:PFAC?, READ?, CONF?, POW:LIM?, POW:CYC?, FUNC?'),
])
# W limits (CALC:LIM in the power meter, the slot of its own; the fail
# beep / badge as the other functions), the phase cal's refusals.
SC['limits'] = (['CONF:POW', 'POW:BURD:STAT OFF', 'CALC:LIM:UPP 100', 'CALC:LIM:LOW 10'], 34, [
    (0, 'open;pow:230@50,i=1,pf=1', lambda d: d['X']['CALC:LIM:FAIL?'] == '1' and d['X']['CALC:LIM:STAT?'] == '1' and near(num(d['X']['CALC:LIM:UPP?']), 100, 1e-6), 'P 230 W > 100 W: FAIL 1'),
    (16, 'open;pow:50@50,i=1,pf=1', lambda d: d['X']['CALC:LIM:FAIL?'] == '0' and near(d['P'], 50, 0.01), 'P 50 W in 10..100 W: FAIL 0'),
])
SC['phasecal-ref'] = (['CONF:POW', 'POW:BURD:STAT OFF'], 50, [
    # RESistive: refused with the reason whatever the load; REACtive on a
    # resistive load: NOTREACTIVE, SPHACAL kept.
    (0, 'open;pow:230@50,i=1,pf=1', lambda d: 'flat' in d['X']['POW:PHAS:CAL RES|SYST:ERR?'] and d['X']['POW:PHAS:CAL?'] in ('NONE,0', 'RUN,0'), 'RES refused with the reason'),
    (12, None, lambda d: d['X']['POW:PHAS:CAL?'] == 'NOTREACTIVE,0' and d['PH'] == 0, 'REAC on a resistive load: NOTREACTIVE, SPHACAL 0'),
    (26, 'open;pow:10@1000,i=0.1,ph=90', lambda d: d['X']['POW:PHAS:CAL?'].startswith('OK,') and abs(d['PF']) < 0.02, 'REAC on a reactive load: OK'),
])
# Bluetooth (run.py --bt-port): the stock phone protocol (cmd 09 = the DC V
# byte with P as a V-group value, cmd 06 echoing it keeps the power meter)
# and BT SCPI (FETC:POW:ALL?, POW:VOLT:RANG?) next to USBTMC.
SC['bt'] = (['CONF:POW', 'POW:BURD:STAT OFF'], 40, [
    (0, 'open;pow:230@50,i=1,pf=0.5', lambda d: bt_ok(d, 115, 0x28) and d['X']['FUNC?'] == '"POW"', 'BT cmd 09: DC V byte 0x28, P 115 (as V); BT SCPI'),
    (18, 'open;vsrc:12;isrc:-2', lambda d: bt_ok(d, -24, 0x28) and d['X']['FUNC?'] == '"POW"', 'after cmd 06 0x28 (echo): still POW, P -24'),
])
EXTRA = {'t1-yy': ['DBG:POW:RAW?', 'DBG:POW:ROW?'], 't3-cycle': ['DBG:POW:RAW?'],
         'zero': ['POW:ZERO?'], 'vcal': ['POW:VOLT:CAL?'], 'vcal-clip': ['POW:VOLT:CAL?'], 'dcad1': ['POW:COUP?'], 'auto': ['POW:COUP?'], 'ac': ['POW:COUP?'],
         'istep': ['SYST:ALER?'], 'istep-ua': ['SYST:ALER?'], 'iman-ovl': ['SYST:ALER?'],
         'scpi': ['MEAS:POW:PFAC?', 'READ?', 'CONF?', 'POW:LIM?', 'POW:CYC?', 'FUNC?'],
         'limits': ['CALC:LIM:FAIL?', 'CALC:LIM:STAT?', 'CALC:LIM:UPP?'],
         'phasecal-ref': ['POW:PHAS:CAL RES|SYST:ERR?', 'POW:PHAS:CAL?'], 'bt': ['FUNC?']}
ACTIONS = {'ac': {46: 'POW:COUP ACDC'}, 'ereset': {8: 'DBG:DRAG 58,160,58,160,20,1500', 21: 'DBG:TAP 310,204'}, 'phasecal': {20: 'POW:PHAS:CAL REAC'}, 'phasecal-ref': {8: 'POW:PHAS:CAL', 32: 'POW:PHAS:CAL'}, 'zero': {16: 'POW:ZERO'}, 'vcal': {12: 'POW:VOLT:CAL', 36: 'POW:VOLT:CAL', 54: 'POW:VOLT:CAL'},
           'vcal-clip': {6: 'POW:VOLT:CAL'}}


def bt_frame(cmd, data=b''):
    d = bytes([0xAF, 0x05, 0x03, cmd, len(data)]) + bytes(data)
    return d + bytes([(0x100 - sum(d)) & 0xFF])


def bt_meas(buf):
    """The last cmd 09 reply in buf: (mode byte, main value in the V group's
    base units scaled to V, raw fmt) or None."""
    out = None
    i = 0
    while i + 17 <= len(buf):
        if buf[i:i + 5] == b'\xdf\x05\x03\x09\x0b' and sum(buf[i:i + 17]) & 0xFF == 0:
            d = buf[i + 5:i + 16]
            raw = d[9] | d[10] << 8
            fmt = d[4]
            v = None if raw == 0xFFFF else raw * 10.0 ** (3 * ((fmt >> 4) & 3) - ((fmt >> 1) & 7)) * 1e-3
            out = [d[0], None if v is None else (-v if fmt & 1 else v), fmt, bytes(d[5:9]).hex()]
            i += 17
            continue
        i += 1
    return out


def bt_ok(d, p, mode):
    m = d.get('BT09')
    f = d['X'].get('BT FETC:POW:ALL?', '').split(',')
    try:
        bp = num(f[0])
    except (ValueError, IndexError):
        return False
    # value 2 / 3 "no value" as in a stock DC V frame.
    return m is not None and m[0] == mode and m[1] is not None and near(m[1], p, 0.01, 0.05) and m[3] == 'ffffffff' and near(bp, p, 0.01, 0.05) \
        and d['X'].get('BT POW:VOLT:RANG?', '').startswith('AUTO')


def num(s):
    v = float(s)
    return float('nan') if abs(v) > 1e37 else v


def run(name):
    setup, run_for, phases = SC[name]
    script = [(t, spec) for t, spec, _, _ in phases if spec]
    tag = 'pow-' + name
    os.makedirs(OUT, exist_ok=True)
    sp = os.path.join(OUT, tag + '.script')
    with open(sp, 'w') as f:
        for t, spec in script:
            f.write('%g dmm %s\n' % (t, spec))
    bt_port = A.port + 1000 if name == 'bt' else 0
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--fast', '--usbip', '3',
           '--usbip-port', str(A.port), '--run-for', str(run_for), '--script', sp, '--name', tag,
           '--shot', os.path.join(OUT, tag + '.png')] + (['--app', os.path.abspath(A.app)] if A.app else []) \
        + (['--bt-port', str(bt_port)] if bt_port else [])
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    rows = []
    t_wall = time.time()
    tm = None
    while p.poll() is None and tm is None and time.time() - t_wall < 300:
        try:
            tm = uc.Usbtmc(uc.Dev('1-0'))
        except Exception as e:
            print('attach:', type(e).__name__, e, flush=True)
            time.sleep(1)
    acts = dict(ACTIONS.get(name, {}))
    bt = None
    if bt_port:
        import socket
        for _ in range(300):
            try:
                bt = socket.create_connection(('127.0.0.1', bt_port), timeout=1)
                break
            except OSError:
                time.sleep(0.2)

    def bt_xfer(msg, want, timeout=8.0):
        """Send msg on the BT link, collect until want(buf) or the timeout."""
        bt.sendall(msg)
        buf = b''
        t_end = time.time() + timeout
        bt.settimeout(0.2)
        while time.time() < t_end and not want(buf):
            try:
                c = bt.recv(4096)
                if not c:
                    break
                buf += c
            except OSError:
                pass
        return buf
    bt_acts = {14: bt_frame(0x06, b'\x28')} if bt else {}
    if tm is not None:
        try:
            for c in setup:
                tm.write(c)
                if c.endswith('?'):
                    print(name, c, tm.read(timeout=60).decode(errors='replace').strip(), flush=True)
            while p.poll() is None:
                row = {}
                for q in ['DBG:PING?', 'FETC:POW:ALL?', 'POW:VOLT:RANG?', 'POW:CURR:RANG?', 'POW:PHAS?'] + EXTRA.get(name, []):
                    # 'command|query': the command written first (no reply).
                    for c in q.split('|')[:-1]:
                        tm.write(c)
                    tm.write(q.split('|')[-1])
                    row[q] = tm.read(timeout=60).decode(errors='replace').strip()
                t = int(row['DBG:PING?'].split(',')[0]) / 1000.0
                bt09 = None
                if bt is not None:
                    bt09 = bt_meas(bt_xfer(bt_frame(0x09), lambda b: bt_meas(b) is not None))
                    for q in ['FETC:POW:ALL?', 'POW:VOLT:RANG?']:
                        row['BT ' + q] = bt_xfer((q + '\n').encode(), lambda b: b.endswith(b'\n')).decode(errors='replace').strip()
                    for ta in sorted(bt_acts):
                        if t >= ta:
                            bt_xfer(bt_acts.pop(ta), lambda b: len(b) >= 7, 2.0)
                d = {'t': t, 'BT09': bt09, 'VR': row['POW:VOLT:RANG?'], 'IR': row['POW:CURR:RANG?'], 'PH': int(row['POW:PHAS?'] or 0),
                     'RAW': row.get('DBG:POW:RAW?', ''), 'ROW': row.get('DBG:POW:ROW?', ''), 'X': row}
                try:
                    d.update(zip(F, [num(x) for x in row['FETC:POW:ALL?'].split(',')]))
                except ValueError:
                    pass
                rows.append(d)
                print('%s %6.2f %s %s %s ph %d' % (name, t, row['FETC:POW:ALL?'], d['VR'], d['IR'], d['PH']), flush=True)
                for ta in sorted(acts):
                    if t >= ta:
                        tm.write(acts.pop(ta))
        except Exception as e:
            print(name, 'link ended:', e, flush=True)
    out, _ = p.communicate()
    ok = True
    bad = []
    for i, (t, _, chk, label) in enumerate(phases):
        t1 = phases[i + 1][0] if i + 1 < len(phases) else run_for
        seen = [r for r in rows if t1 - 4.5 <= r['t'] <= t1 - 1.5 and ('P' in r or name.startswith('t1'))]
        if not seen:
            ok = False
            bad.append('%s: no samples' % label)
            continue
        fails = [r for r in seen if not chk(r)]
        if fails:
            ok = False
            r = fails[-1]
            bad.append('%s: %.2f %s %s %s %s' % (label, r['t'], ' '.join('%s=%g' % (k, r[k]) for k in F if k in r), r['VR'], r['IR'], r['RAW'][:120]))
        else:
            r = seen[-1]
            print('  ok  %-50s %s' % (label, ' '.join('%s=%.6g' % (k, r[k]) for k in F[:8] if k in r) or r['RAW'][:100]), flush=True)
    res = {'name': name, 'ok': ok, 'bad': bad, 'rows': rows, 'rc': p.returncode, 'log': out[-600:] if p.returncode else ''}
    with open(os.path.join(OUT, tag + '.json'), 'w') as f:
        json.dump(res, f, indent=1)
    print('RESULT %s %s %s' % (name, 'OK' if ok else 'FAIL', '; '.join(bad[:5])), flush=True)
    return ok


if __name__ == '__main__':
    if A.list:
        for n, (su, rf, ph) in SC.items():
            print('%-9s %s | %s' % (n, ' '.join(su), ' | '.join('%g %s: %s' % (t, s, l) for t, s, _, l in ph)))
        sys.exit(0)
    allok = True
    for n in A.names or list(SC):
        allok &= run(n)
    sys.exit(0 if allok else 1)
