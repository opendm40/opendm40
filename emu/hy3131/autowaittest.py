#!/usr/bin/env python3
"""READ? / MEASure? in the AUTO coupling (fw-rs main.rs AUTO_WAIT_MS,
docs/switch-latency.md 6): AUTO's values before its first decision are
provisional (never to SCPI), so a READ? / MEAS:VOLT:AUTO? right after the
host's change waits for the decision, at most until AUTO_WAIT_MS (3.9 s)
after the coupling was entered (a later query: at most 0.85 s), and then
answers the part AUTO shows: never +9.91E37 / "Data stale"; libsigrok's
READ? 3 s after CONF inside its 1 s timeout. FETCh? never blocks: +9.91E37
at once until the decision (ac-fetch / dc-fetch). RUN (HOLD) pressed before
the decision shows HOLD at once and holds the first decided value (ac-hold,
DBG:READout?). The placeholder after CONF:CURR:DC 10 has the A unit from
the start (a10-unit). Each window: input at its start, VOLT:DC at +3 s, the
query at +8 s (fw uptime from the window's input marker, emu/emusync.py),
the reply time from DBG:PING? before and after.

  emu/hy3131/autowaittest.py --app firmware/target/native/thumbv7em-none-eabihf/release/dm40-fw --port 47600
"""
import argparse
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
OUT = os.path.join(EMU, 'out')
BOOT_OFS = 0.906  # the inputs' shift in virtual time (switchlat.py); the windows key on their markers
T0 = 8.0
W = 20.0

# name, input, query, expected value, rel tolerance, max reply s
SCEN = [
    ('dc', 'dcv:5.0', 'MEAS:VOLT:AUTO?', 5.0, 0.01, 4.0),
    ('ac', 'acv:1.0@50,dc=0.2', 'MEAS:VOLT:AUTO?', 1.0, 0.02, 4.0),
    ('ac-read', 'acv:1.0@50,dc=0.2', 'CONF:VOLT:AUTO;:READ?', 1.0, 0.02, 4.0),
    ('tie', 'acv:4.776@40,dc=4.776,shape=square', 'MEAS:VOLT:AUTO?', 4.776, 0.01, 4.0),
    ('tie-read', 'acv:4.776@40,dc=4.776,shape=square', 'CONF:VOLT:AUTO;:READ?', 4.776, 0.01, 4.0),
    # libsigrok's DM40 profile: CONF, 3 s, READ? with a hard 1 s USBTMC timeout.
    ('ac-sigrok', 'acv:1.0@50,dc=0.2', 'CONF:VOLT:AUTO|3|READ?', 1.0, 0.02, 0.95),
    ('tie-sigrok', 'acv:4.776@40,dc=4.776,shape=square', 'CONF:VOLT:AUTO|3|READ?', 4.776, 0.01, 0.95),
    ('tie-sigrok2', 'acv:4.776@40,dc=4.776,shape=square', 'CONF:VOLT:AUTO|5|READ?', 4.776, 0.01, 0.95),
    # FETCh? while AUTO decides (device ab7184c new bug 2): +9.91E37 at once, every time
    # (FETCHPOLL:<s,..>: CONF:VOLT:AUTO, FETC? at those times after it, each within 0.25 s;
    # FETC? at +7 s is the decided value)
    # (dc-fetch: the DC decision comes at ~1.3 s, so only the polls before it)
    ('ac-fetch', 'acv:1.0@50,dc=0.2', 'FETCHPOLL:0.3,0.8,1.5,2.5', 1.0, 0.02, 0.25),
    ('dc-fetch', 'dcv:5.0', 'FETCHPOLL:0.3,0.8', 5.0, 0.01, 0.25),
    # RUN (HOLD) while AUTO decides (device ab7184c new bug 3): HOLD shown from the press,
    # the first decided value held when it comes (HOLDPOLL: CONF:VOLT:AUTO, DBG:KEY 1 at +1 s,
    # DBG:READout? to +8 s)
    ('ac-hold', 'acv:1.0@50,dc=0.2', 'HOLDPOLL', 1.0, 0.02, 8.0),
    # The placeholder after a host's CONF with a range (device ab7184c new bug 5: CONF:CURR:DC 10
    # showed the mA unit for 0.8 s): DBG:READout? in the same message and for 1.2 s after it
    # names the target unit (UNIT:<command>:<unit>; want / rel unused)
    ('a10-unit', 'dcv:1.0', 'UNIT:CONF:CURR:DC 10:A', 0, 0, 1.2),
    ('ma-unit', 'dcv:1.0', 'UNIT:CONF:CURR:DC 0.06:mA', 0, 0, 1.2),
]

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='fw-rs native ELF')
ap.add_argument('--port', type=int, default=47600, help='USB/IP port')
ap.add_argument('names', nargs='*', help='scenarios (default all)')
A = ap.parse_args()
SCEN = [sc for sc in SCEN if not A.names or sc[0] in A.names]
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402
import emusync  # noqa: E402

os.makedirs(OUT, exist_ok=True)
tag = 'autowait-%d-%d' % (A.port, os.getpid())
script = os.path.join(OUT, tag + '.script')
with open(script, 'w') as f:
    for k, sc in enumerate(SCEN):
        f.write('%.3f dmm %s\n' % (T0 + k * W + BOOT_OFS, sc[1]))
end = T0 + len(SCEN) * W + 2 + BOOT_OFS
cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--app', A.app, '--usbip', '0.01', '--usbip-port', str(A.port),
       '--run-for', '%.1f' % end, '--name', tag, '--script', script]
emu = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, start_new_session=True)
mk = emusync.Markers(emusync.log_path(OUT, tag))
bad = []
try:
    t = None
    for _ in range(150):
        try:
            t = uc.Usbtmc(uc.Dev('1-0'))
            break
        except Exception:
            time.sleep(2)
    if t is None:
        sys.exit('autowaittest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60):
        t.write(line)
        return t.read(maxlen=8192, timeout=timeout).decode(errors='replace').strip()

    def up():
        return int(q('DBG:PING?').split(',')[0]) / 1000.0

    def wait_until(s):
        while up() < s:
            time.sleep(0.2)

    if mk.wait_clock() is None:
        sys.exit('autowaittest: no clock marker (dwt: CYCCNT write) in %s' % mk.log)
    def num(r):
        try:
            return float(r.split(',')[0])
        except ValueError:
            return float('nan')

    def fetchpoll(name, at, want, rel, tmax):
        t.write('CONF:VOLT:AUTO')
        t0 = up()
        why = []
        for d in at:
            wait_until(t0 + d)
            t1 = up()
            r = q('FETC?')
            dt = up() - t1
            print('      FETC? at +%.2f s: %s in %.2f s' % (t1 - t0, r, dt), flush=True)
            if dt > tmax or num(r) < 9e37:
                why.append('FETC? +%.1f s %s in %.2f s' % (d, r, dt))
        wait_until(t0 + 7)
        r = q('FETC?')
        err = q('SYST:ERR?')
        if not abs(num(r) - want) <= rel * want:
            why.append('FETC? +7 s %s' % r)
        if err.split(',')[0].strip() not in ('0', '+0'):
            why.append('SYST:ERR? %s' % err)
        print('  %s %-9s FETC? +7 s %s, SYST:ERR? %s' % ('ok ' if not why else 'BAD', name, r, err), flush=True)
        if why:
            bad.append('%s: %s' % (name, '; '.join(why)))

    def holdpoll(name, want, rel, tmax):
        t.write('CONF:VOLT:AUTO')
        t0 = up()
        wait_until(t0 + 1.0)
        t.write('DBG:KEY 1')
        wait_until(t0 + 1.3)
        why, seen = [], []
        while up() < t0 + tmax:
            ro = q('DBG:READout?').split(',')
            seen.append(ro)
            if ro[-1] != '1':
                why.append('hold state %s at +%.1f s' % (ro[-1], up() - t0))
                break
            time.sleep(0.25)
        last = seen[-1] if seen else ['N', '', 'nan', '0']
        coup = q('VOLT:COUP?')
        fetch = q('FETC?')
        kinds = ''.join(x[0] for x in seen)
        if last[0] != 'F' or not abs(num(last[2]) - want) <= rel * want:
            why.append('readout %s at +%.0f s (kinds %s)' % (','.join(last), tmax, kinds))
        if not abs(num(fetch) - want) <= rel * want:
            why.append('FETC? %s' % fetch)
        if 'AUTO,AC' not in coup:
            why.append('VOLT:COUP? %s' % coup)
        # a held value stays: the input is unchanged, so the readout keeps it
        finals = [x for x in seen if x[0] == 'F']
        if len(set(x[2] for x in finals)) > 1:
            why.append('held value moved: %s' % sorted(set(x[2] for x in finals)))
        t.write('DBG:KEY 1')
        print('  %s %-9s readout kinds %s, last %s, COUP? %s, FETC? %s' % (
            'ok ' if not why else 'BAD', name, kinds, ','.join(last), coup, fetch), flush=True)
        if why:
            bad.append('%s: %s' % (name, '; '.join(why)))

    def unitpoll(name, cmd, unit, tmax):
        ro = q(cmd + ';:DBG:READ?')
        t0 = up()
        seen = [ro]
        while up() < t0 + tmax:
            seen.append(q('DBG:READ?'))
        units = [x.split(',')[1] if ',' in x else x for x in seen]
        why = ['%s at #%d' % (x, i) for i, x in enumerate(seen) if x.split(',')[1:2] != [unit]][:3]
        print('  %s %-9s %s: readout units %s' % ('ok ' if not why else 'BAD', name, cmd, ' '.join(sorted(set(units)))), flush=True)
        if why:
            bad.append('%s: %s' % (name, '; '.join(why)))

    for k, (name, _, query, want, rel, tmax) in enumerate(SCEN):
        w0 = mk.at(T0 + k * W + BOOT_OFS)  # fw uptime of the window's input change
        wait_until(w0 + 3)
        q('CONF:VOLT:DC;:SYST:ERR?')
        wait_until(w0 + 8)
        if query.startswith('FETCHPOLL'):
            fetchpoll(name, [float(x) for x in query.split(':')[1].split(',')], want, rel, tmax)
            continue
        if query == 'HOLDPOLL':
            holdpoll(name, want, rel, tmax)
            continue
        if query.startswith('UNIT:'):
            c, u = query[len('UNIT:'):].rsplit(':', 1)
            unitpoll(name, c, u, tmax)
            continue
        parts = query.split('|')
        for i in range(0, len(parts) - 1, 2):
            t.write(parts[i])
            wait_until(up() + float(parts[i + 1]))
        t1 = up()
        r = q(parts[-1])
        t2 = up()
        err = q('SYST:ERR?')
        coup = q('VOLT:COUP?')
        fetch = q('FETC?')
        try:
            v = float(r.split(',')[0])
        except ValueError:
            v = float('nan')
        dt = t2 - t1
        ok = abs(v - want) <= rel * want and dt <= tmax and err.split(',')[0].strip() in ('0', '+0')
        print('  %s %-9s %-24s reply %-16s in %.2f s  SYST:ERR? %s  COUP? %s  FETC? then %s' % (
            'ok ' if ok else 'BAD', name, query, r, dt, err, coup, fetch), flush=True)
        if not ok:
            bad.append('%s: %s in %.2f s, %s' % (name, r, dt, err))
finally:
    t = None
    emu.wait()  # run.py ends the emulator at --run-for
    for x in (script, os.path.join(OUT, 'flash-%s.bin' % tag), os.path.join(OUT, tag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
print('autowaittest: %s' % ('PASS' if not bad else 'FAIL ' + '; '.join(bad)))
sys.exit(1 if bad else 0)
