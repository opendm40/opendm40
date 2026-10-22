#!/usr/bin/env python3
"""fw-rs "Leads in A?" reminder (fw-rs/docs/power-meter-and-misconnect.md
4.9) in the emulator: fw-rs native + the HY3131 model, function changes by
SCPI and by a tap on the DMM page's V button; DBG:LEADs? (count, last
from / to function) must count exactly the changes from a current function
(DC / AC / AC+DC A, the power meter) to a non-current one, none with
SYST:ALER:ENAB bit 3 clear and none for a fuse check started from DC A.
SYST:ALER? stays NONE (the reminder is no alert state). Screenshots: the
DC A page before the tap, the toast over the DC V page with its first
reading, the page after the toast ended by itself.

  emu/hy3131/leadstest.py                        # fw-rs built from HEAD
  emu/hy3131/leadstest.py --app fw-rs/dm40-fw-native.bin --port 3379

Results: emu/out/leads.json, emu/out/leads-*.png. One emulator at a time
per port.
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
ap.add_argument('--app', help='fw-rs native image (default: run.py builds HEAD)')
ap.add_argument('--port', type=int, default=3379, help='USB/IP port (default 3379)')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

# (t, connection or None, SCPI at the start, expected reminder count, expected
# last (from, to) (from None: any current function; DC A autoranges over the
# modes 7..9, AC A over 4..6) or None = any, expected SYST:ALER:ENAB?). A phase is checked
# from t + 2.5 s to the next t - 1 s (fw uptime; SCPI goes out at t + 0.5).
TAP_T = 10.0
# Virtual-time events (connections, the tap) go into the run.py script this
# much later than their phase's t: the fw uptime the windows and the SCPI use
# lags the virtual time by ~1.3-2 s since c2709aa (the tap at virtual 10 s
# counted at uptime 8.7, inside the previous phase's window); 1.5 s puts them
# within -0.5..+1.5 s of t for a lag of 0..2 s.
VLAG = 1.5
PHASES = [
    (0, 'open', ['CONF:CURR:DC'], 0, None, '15'),
    # The DMM page's V button (DBG-free: a real touch, virtual time TAP_T).
    (TAP_T, None, [], 1, (None, 1), '15'),
    (15, None, ['CONF:CURR:AC'], 1, (None, 1), '15'),
    (19, None, ['CONF:VOLT:AC'], 2, (None, 0), '15'),
    # Bit 3 off: an A -> ohm change is silent; the row is not rewritten.
    (23, None, ['SYST:ALER:ENAB 7', 'CONF:CURR:DC'], 2, (None, 0), '7'),
    (27, None, ['CONF:RES'], 2, (None, 0), '7'),
    (31, None, ['SYST:ALER:ENAB ON', 'CONF:POW'], 2, (None, 0), '15'),
    (36, None, ['CONF:VOLT:DC'], 3, (18, 1), '15'),
    # Fuse check from DC A (ohm while it runs, a lead V -> A on purpose) and
    # back to DC A: no reminder.
    (40, 'open;valead', ['CONF:CURR:DC'], 3, (18, 1), '15'),
    (44, None, ['SYST:FUSE:TEST'], 3, (18, 1), '15'),
    (50, 'open', ['CONF:CURR:ACDC'], 3, (18, 1), '15'),
    (54, None, ['CONF:VOLT:ACDC'], 4, (17, 16), '15'),
]
RUN_FOR = 60
# Dmm::func codes of the functions that use the A jack.
A_FUNCS = set(range(4, 10)) | {17, 18}


def main():
    os.makedirs(OUT, exist_ok=True)
    sp = os.path.join(OUT, 'leads.%d.script' % os.getpid())
    with open(sp, 'w') as f:
        for t, conn, _, _, _, _ in PHASES:
            if conn:
                f.write('%g dmm %s\n' % (t + VLAG, conn))
        # V button: btn_x(0) = 4, 64 px wide, BTN_Y = 280.
        f.write('%g tap 36 300\n' % (TAP_T + VLAG))
    shots = ['--shot', os.path.join(OUT, 'leads-before.png@%g' % (TAP_T + VLAG - 0.5)),
             '--shot', os.path.join(OUT, 'leads-toast.png@%g' % (TAP_T + VLAG + 1.3)),
             '--shot', os.path.join(OUT, 'leads-after.png@%g' % (TAP_T + VLAG + 3.5))]
    cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--fast', '--usbip', '3',
           '--usbip-port', str(A.port), '--run-for', str(RUN_FOR), '--script', sp, '--name', 'leads.%d' % os.getpid()] \
        + shots + (['--app', os.path.abspath(A.app)] if A.app else [])
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
    done = set()
    if tm is not None:
        try:
            while p.poll() is None:
                row = {}
                for q in ['DBG:PING?', 'DBG:LEADs?', 'FUNC?', 'SYST:ALER?', 'SYST:ALER:ENAB?', 'STAT:QUES:COND?', 'SYST:FUSE?']:
                    tm.write(q)
                    row[q] = tm.read(timeout=60).decode(errors='replace').strip()
                row['t'] = int(row['DBG:PING?'].split(',')[0]) / 1000.0
                rows.append(row)
                print('%6.2f %-12s LEADS %-10s ALER %-6s ENAB %-3s QUES %s FUSE %s' % (
                    row['t'], row['FUNC?'], row['DBG:LEADs?'], row['SYST:ALER?'], row['SYST:ALER:ENAB?'],
                    row['STAT:QUES:COND?'], row['SYST:FUSE?']), flush=True)
                for i, ph in enumerate(PHASES):
                    if i not in done and row['t'] >= ph[0] + 0.5:
                        done.add(i)
                        for c in ph[2]:
                            tm.write(c)
        except Exception as e:
            print('link ended:', e, flush=True)
    out, _ = p.communicate()
    ok = True
    bad = []
    for i, (t, _, _, n, last, enab) in enumerate(PHASES):
        t0 = t + 2.5
        t1 = (PHASES[i + 1][0] if i + 1 < len(PHASES) else RUN_FOR) - 1.0
        seen = [r for r in rows if t0 <= r['t'] <= t1]
        if not seen:
            ok = False
            bad.append('no samples in %g..%g' % (t0, t1))
        for r in seen:
            try:
                v = [int(x) for x in r['DBG:LEADs?'].split(',')]
            except ValueError:
                ok = False
                bad.append('%.2f DBG:LEADs? %s' % (r['t'], r['DBG:LEADs?']))
                continue
            if v[0] != n or (last is not None and not (
                    (v[1] == last[0] if last[0] is not None else v[1] in A_FUNCS) and v[2] == last[1])):
                ok = False
                bad.append('%.2f DBG:LEADs? %s, want %d,%s' % (r['t'], r['DBG:LEADs?'], n, last))
            if r['SYST:ALER?'] != 'NONE' or r['STAT:QUES:COND?'] != '0':
                ok = False
                bad.append('%.2f SYST:ALER? %s QUES %s' % (r['t'], r['SYST:ALER?'], r['STAT:QUES:COND?']))
            if r['SYST:ALER:ENAB?'] != enab:
                ok = False
                bad.append('%.2f SYST:ALER:ENAB? %s, want %s' % (r['t'], r['SYST:ALER:ENAB?'], enab))
    # The fuse check ran (and came back to DC A) within its phase.
    fz = [r for r in rows if 46 <= r['t'] <= 49]
    if not any(r['SYST:FUSE?'].startswith('OK') and r['FUNC?'].strip('"').startswith('CURR') for r in fz):
        ok = False
        bad.append('fuse check: no OK back in CURR in 46..49 s')
    res = {'ok': ok, 'bad': bad, 'rows': rows, 'rc': p.returncode, 'log': out[-800:] if p.returncode else ''}
    with open(os.path.join(OUT, 'leads.json'), 'w') as f:
        json.dump(res, f, indent=1)
    try:
        os.remove(sp)
    except OSError:
        pass
    print('RESULT leads %s %s' % ('OK' if ok else 'FAIL', '; '.join(bad[:6])), flush=True)
    return ok


if __name__ == '__main__':
    sys.exit(0 if main() else 1)
