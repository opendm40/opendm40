#!/usr/bin/env python3
"""READ? / MEASure? in the AUTO coupling (fw-rs main.rs AUTO_WAIT_MS,
docs/switch-latency.md 6): AUTO's values before its first decision are
provisional (never to SCPI), so a READ? / MEAS:VOLT:AUTO? right after the
host's change waits for the decision, at most until AUTO_WAIT_MS (3.9 s)
after the coupling was entered (a later query: at most 0.85 s), and then
answers the part AUTO shows: never +9.91E37 / "Data stale"; libsigrok's
READ? 3 s after CONF inside its 1 s timeout. FETCh? stays non-blocking-ish
(+9.91E37 until the decision). Each window: input at its start, VOLT:DC at
+3 s, the query at +8 s (fw uptime), the reply time from DBG:PING? before
and after.

  emu/hy3131/autowaittest.py --app fw-rs/target/native/thumbv7em-none-eabihf/release/dm40-fw --port 47600
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
BOOT_OFS = 0.906  # virtual time - fw uptime (switchlat.py)
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
]

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='fw-rs native ELF')
ap.add_argument('--port', type=int, default=47600, help='USB/IP port')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

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

    for k, (name, _, query, want, rel, tmax) in enumerate(SCEN):
        w0 = T0 + k * W
        wait_until(w0 + 3)
        q('CONF:VOLT:DC;:SYST:ERR?')
        wait_until(w0 + 8)
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
