#!/usr/bin/env python3
"""Map the HY3131 ohm source (opendm40 DBG:SRC, src/dbgsrc.rs): sweep every
OP3 VREF tap x path, read the nodes around the reference resistor, write a
CSV and print the open-circuit voltage per tap and the current per path.

    srcmap.py --load open  -o open.csv          V/ohm - COM open
    srcmap.py --load short -o short.csv         V/ohm - COM shorted
    srcmap.py --load 1000  -o r1k.csv           a known resistor (ohm): I = V_in / R
    srcmap.py --load open --usbip 1-0           the emulator (emu/run.py --usbip)

Options: --vrefs / --paths (names or indexes, comma separated; default all,
PB7 only with --include-pb7), --sense PB0|OP1O|SENSE|RLU (the input node,
default PB0), --settle s (after each row write, default 0.3), --cycles n
(complete AD1 cycles to wait for after the settle, default 1; one cycle is
10 nodes x 2 conversions of ~50 ms). The current-source paths (CS = the
stock 60 M row, DIODE) take only the stock taps VDS17 / VDS16 / VDS15; the
others are skipped ("refused") unless --force-cs (passes FORCE: how much
current those rows give for another tap is not known).

--ext v|ma reads the MR9270S+ calibrator's own measurement channel
(tools/mr9270.py, --cal-port, default /dev/ttyACM1) at every point as an
independent reference: v = the open-circuit voltage at V/ohm - COM (leads
on the calibrator's voltage input), ma = the current through its mA input
(leads on the mA input; that is the short). Its output must be OFF: the tool
reads REG_OUT_ENABLE at the start and refuses to run when it is on; it never
writes the output enable. The measurement function / mode codes for DC V and
DC mA are NOT known (mr9270.py has only the output table): the defaults
below mirror the output table (V: func 1 mode 0, mA: func 0 mode 0, i32 in
mV / uA) and are *inferred*. Check them first with --ext-probe (reads REG_MEAS_FUNC /
MODE / VALUE only, no write) after setting the measurement on the
calibrator's panel; override with --ext-func / --ext-mode / --ext-scale.
--ext-set writes the function / mode registers (only those) when they
differ; without it the tool refuses on a mismatch.

DBG:SRC? reply (scpi.rs): 1,cycles,vref,path,sense,V_in,V_far,I,V_vref,
V_drop,V_com,VDDA-AGND,AGND-VSS;10 AD1 codes;R20..R35 hex. Volts are
relative to AGND; V_in is PB0 x the path's divider (10 in DIODE). The CSV's
V_jack = V_in - V_com is the voltage V/ohm - COM (what --ext v measures).
"""
import argparse
import csv
import math
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

VREFS = ['VDS17', 'VDS16', 'VDS15', 'AGNDP9', 'AGNDP8', 'AGNDP7', 'AGNDP6', 'AGNDP0',
         'VDS1', 'VDS2', 'VDS3', 'AGNDN9', 'AGNDN8', 'AGNDN7', 'AGNDN6', 'PB7']
PATHS = ['R1K', 'R10K', 'R100K', 'R1M', 'R10M', 'CS', 'DIODE']
RREF = {'R1K': 1e3, 'R10K': 1e4, 'R100K': 1e5, 'R1M': 1e6, 'R10M': 1e7}
CS_PATHS = ('CS', 'DIODE')
CS_SAFE = ('VDS17', 'VDS16', 'VDS15')
SENSES = ['PB0', 'OP1O', 'SENSE', 'RLU']
NAN = float('nan')
FIELDS = ['V_input', 'V_far', 'I', 'V_vref', 'V_drop', 'V_com', 'VDDA_AGND', 'AGND_VSS']
# --ext defaults (INFERRED: mirror mr9270.SCALE's output codes): func, mode, i32 per unit
EXT = {'v': (1, 0, 1000.0, 'V'), 'ma': (0, 0, 1000.0, 'mA')}


def names(arg, table):
    if not arg:
        return list(table)
    out = []
    for t in arg.split(','):
        t = t.strip().upper()
        if t.isdigit():
            t = table[int(t)]
        if t.startswith('VDSC'):
            t = 'VDS' + t[4:]
        if t not in table:
            sys.exit('srcmap: unknown %r (one of %s)' % (t, ', '.join(table)))
        out.append(t)
    return out


def num(s):
    v = float(s)
    return NAN if abs(v) > 9e37 else v


def parse_reply(r):
    """DBG:SRC? -> (cycles, vals dict, codes, row hex) or None when off."""
    parts = r.strip().split(';')
    head = parts[0].split(',')
    if head[0] != '1' or len(parts) < 3 or len(head) < 13:
        return None
    vals = {k: num(x) for k, x in zip(FIELDS, head[5:13])}
    codes = [int(c) for c in parts[1].split(',')]
    return int(head[1]), vals, codes, parts[2].strip()


def link_of(a):
    import dm40scpi
    return dm40scpi.UsbipLink(a.usbip) if a.usbip else dm40scpi.UsbLink()


class Ext:
    """MR9270S+ measurement channel; never touches the output enable."""

    def __init__(self, a):
        import mr9270
        self.m = mr9270
        self.cal = mr9270.MR9270(a.cal_port).connect()
        func, mode, scale, unit = EXT[a.ext]
        self.func = a.ext_func if a.ext_func is not None else func
        self.mode = a.ext_mode if a.ext_mode is not None else mode
        self.scale = a.ext_scale if a.ext_scale is not None else scale
        self.unit = unit
        en = self.cal.read(mr9270.REG_OUT_ENABLE, 1)[0]
        if en != 0:
            self.cal.close()
            sys.exit('srcmap: the calibrator output is ON (REG_OUT_ENABLE = %d): turn it off on the calibrator first' % en)
        f, m = self.cal.read(mr9270.REG_MEAS_FUNC, 2)
        if (f, m) != (self.func, self.mode):
            if not a.ext_set:
                self.cal.close()
                sys.exit('srcmap: calibrator measurement func %d mode %d, want %d / %d (--ext-set writes them, '
                         '--ext-func / --ext-mode change the wanted codes)' % (f, m, self.func, self.mode))
            self.cal.write(mr9270.REG_MEAS_FUNC, [self.func, self.mode])
            f, m = self.cal.read(mr9270.REG_MEAS_FUNC, 2)
            if (f, m) != (self.func, self.mode):
                self.cal.close()
                sys.exit('srcmap: calibrator kept measurement func %d mode %d' % (f, m))

    def check_off(self):
        en = self.cal.read(self.m.REG_OUT_ENABLE, 1)[0]
        if en != 0:
            raise SystemExit('srcmap: the calibrator output went ON (REG_OUT_ENABLE = %d): stopped' % en)

    def value(self):
        self.check_off()
        return self.cal.read_measurement() / self.scale

    def close(self):
        self.cal.close()


def ext_probe(a):
    import mr9270
    with mr9270.MR9270(a.cal_port) as cal:
        en = cal.read(mr9270.REG_OUT_ENABLE, 1)[0]
        f, m = cal.read(mr9270.REG_MEAS_FUNC, 2)
        print('output enable %d, measurement func %d mode %d, value i32 %d' % (en, f, m, cal.read_measurement()))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--load', help='open | short | <ohms> across V/ohm - COM')
    ap.add_argument('-o', '--out', default='srcmap.csv')
    ap.add_argument('--vrefs')
    ap.add_argument('--paths')
    ap.add_argument('--sense', default='PB0', choices=SENSES)
    ap.add_argument('--settle', type=float, default=0.3)
    ap.add_argument('--cycles', type=int, default=1)
    ap.add_argument('--timeout', type=float, default=8.0, help='s per point for the cycles')
    ap.add_argument('--force-cs', action='store_true')
    ap.add_argument('--include-pb7', action='store_true', help='PB7 as VREF (its board wiring is not known)')
    ap.add_argument('--usbip', help='emulator USB/IP bus id (e.g. 1-0)')
    ap.add_argument('--ext', choices=sorted(EXT))
    ap.add_argument('--ext-func', type=int)
    ap.add_argument('--ext-mode', type=int)
    ap.add_argument('--ext-scale', type=float, help='i32 counts per V (v) / per mA (ma)')
    ap.add_argument('--ext-set', action='store_true')
    ap.add_argument('--ext-probe', action='store_true', help='read the calibrator registers, no write, then exit')
    ap.add_argument('--cal-port', default='/dev/ttyACM1')
    ap.add_argument('-y', '--yes', action='store_true', help='no prompt')
    a = ap.parse_args()
    if a.ext_probe:
        return ext_probe(a)
    if not a.load:
        ap.error('--load open|short|<ohms> is required')
    load = a.load.lower()
    r_load = None
    if load not in ('open', 'short'):
        try:
            r_load = float(load)
        except ValueError:
            ap.error('--load: open, short or a resistance in ohm')
        load = '%g ohm' % r_load
    vrefs = names(a.vrefs, VREFS)
    if not a.vrefs and not a.include_pb7:
        vrefs.remove('PB7')
    paths = names(a.paths, PATHS)

    ext = None
    if a.ext:
        print('--ext %s: move the leads to the calibrator\'s %s input (V/ohm -> +, COM -> -); its output stays off.'
              % (a.ext, 'voltage' if a.ext == 'v' else 'mA'))
    print('load: %s across V/ohm - COM; nothing else may drive the jacks (the misconnect sentinel is off during DBG:SRC).' % load)
    if not a.yes and sys.stdin.isatty():
        input('Enter to start, Ctrl-C to abort: ')
    if a.ext:
        ext = Ext(a)

    link = link_of(a)
    rows = []
    try:
        print('IDN: %s' % link.query('*IDN?'))
        link.query('*CLS')
        for p in paths:
            for v in vrefs:
                rec = {'vref': v, 'path': p, 'sense': a.sense, 'load': load}
                if p in CS_PATHS and v not in CS_SAFE and not a.force_cs:
                    rec['status'] = 'refused'
                    rows.append(rec)
                    continue
                cmd = 'DBG:SRC %s,%s,%s%s' % (v, p, a.sense, ',FORCE' if p in CS_PATHS and a.force_cs else '')
                link.query(cmd)
                err = link.query('SYST:ERR?')
                if not err.startswith('0') and not err.startswith('+0'):
                    rec['status'] = 'error: ' + err
                    rows.append(rec)
                    print('%-6s %-6s %s' % (p, v, err), flush=True)
                    continue
                time.sleep(a.settle)
                got = parse_reply(link.query('DBG:SRC?'))
                n0 = got[0] if got else 0
                t_end = time.time() + a.timeout
                while time.time() < t_end:
                    got = parse_reply(link.query('DBG:SRC?'))
                    if got is None:
                        break
                    if got[0] >= n0 + a.cycles + 1:
                        break
                    time.sleep(0.1)
                if got is None or got[0] < n0 + a.cycles + 1:
                    rec['status'] = 'timeout' if got else 'ended'
                    rows.append(rec)
                    print('%-6s %-6s %s' % (p, v, rec['status']), flush=True)
                    continue
                _, vals, codes, rowhex = got
                rec.update(vals)
                # the jack voltage V/ohm - COM (V_in and V_com are both against AGND)
                rec['V_jack'] = vals['V_input'] - vals['V_com']
                rec['I_load'] = rec['V_jack'] / r_load if r_load else NAN
                rec['ext'] = ext.value() if ext else NAN
                rec['codes'] = ' '.join(str(c) for c in codes)
                rec['row'] = rowhex
                rec['status'] = 'ok'
                rows.append(rec)
                print('%-6s %-6s V_in %+8.4f  V_far %+8.4f  I %s  V_vref %+8.4f%s' % (
                    p, v, vals['V_input'], vals['V_far'],
                    '%+.4e A' % vals['I'] if vals['I'] == vals['I'] else '-',
                    vals['V_vref'], '  ext %+.5g %s' % (rec['ext'], ext.unit) if ext else ''), flush=True)
    finally:
        try:
            link.query('DBG:SRC OFF')
        finally:
            link.close()
            if ext:
                ext.close()

    cols = ['vref', 'path', 'sense', 'load'] + FIELDS + ['V_jack', 'I_load', 'ext', 'codes', 'row', 'status']
    with open(a.out, 'w', newline='') as f:
        w = csv.DictWriter(f, fieldnames=cols, restval='')
        w.writeheader()
        for r in rows:
            w.writerow({k: ('' if isinstance(x, float) and math.isnan(x) else x) for k, x in r.items()})
    print('wrote %s (%d points)' % (a.out, len(rows)))
    summary(rows, vrefs, paths, load, ext)


def summary(rows, vrefs, paths, load, ext):
    ok = {(r['vref'], r['path']): r for r in rows if r.get('status') == 'ok'}

    def cell(v, p, key, fmt):
        r = ok.get((v, p))
        if not r or r.get(key) != r.get(key):
            return '-'
        return fmt % r[key]

    print()
    if load == 'open':
        print('Open-circuit voltage V/ohm - COM (V_jack = V_in - V_com, V) per VREF tap x path; V_vref = the tap vs AGND:')
        print('%-7s %8s ' % ('vref', 'V_vref') + ' '.join('%8s' % p for p in paths) + ('  %8s' % 'ext' if ext else ''))
        for v in vrefs:
            vv = next((ok[(v, p)]['V_vref'] for p in paths if (v, p) in ok), NAN)
            line = '%-7s %8s ' % (v, '-' if vv != vv else '%+.4f' % vv) + ' '.join('%8s' % cell(v, p, 'V_jack', '%+.4f') for p in paths)
            if ext:
                e = [ok[(v, p)]['ext'] for p in paths if (v, p) in ok]
                line += '  %8s' % ('%+.4f' % e[0] if e else '-')
            print(line)
    else:
        key = 'I' if load == 'short' else 'I_load'
        print('%s current (%s, mA) per path x VREF tap:' % ('Short-circuit' if load == 'short' else 'Load', key))
        print('%-7s ' % 'path' + ' '.join('%8s' % v for v in vrefs))
        for p in paths:
            print('%-7s ' % p + ' '.join('%8s' % ('-' if (v, p) not in ok or ok[(v, p)][key] != ok[(v, p)][key]
                                               else '%+.4f' % (ok[(v, p)][key] * 1e3)) for v in vrefs))
        if ext:
            print('ext (%s) per path x VREF tap:' % ext.unit)
            for p in paths:
                print('%-7s ' % p + ' '.join('%8s' % cell(v, p, 'ext', '%+.4f') for v in vrefs))
    bad = [r for r in rows if r.get('status') != 'ok']
    if bad:
        print('%d points not measured (%s)' % (len(bad), ', '.join(sorted({r['status'].split(':')[0] for r in bad}))))


if __name__ == '__main__':
    main()
