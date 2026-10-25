#!/usr/bin/env python3
"""DBG:AD2:CHOPSEQ smoke test (emulator; branch exp/ad1-chopseq): an
unchopped AD1 logger session (LOG:CHOP OFF, LOG:OSR 6, 100 ms) with each
polarity sequence mode. Checks that every record is tagged (bit 30), that
the polarity in R22 (bit 28) follows the flips announced by bit 29, that
mode 1 flips every `param` conversions, that DBG:AD2:CHOPSEQ? counts the
writes, and that CHOPSEQ 0 gives untagged records again. The emulator does
not model the 00 / 01 offsets: only the firmware's bookkeeping is tested.
"""
import argparse
import os
import signal
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ROOT = os.path.dirname(EMU)
OUT = os.path.join(EMU, 'out')
sys.path.insert(0, os.path.join(ROOT, 'firmware', 'tools'))
import dm40scpi  # noqa: E402

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native image')
ap.add_argument('--port', type=int, default=47730, help='USB/IP port')
A = ap.parse_args()
os.environ['USBIP_PORT'] = str(A.port)
sys.path.insert(0, EMU)
import usbip_client as uc  # noqa: E402

os.makedirs(OUT, exist_ok=True)
tag = 'chopseq-%d-%d' % (A.port, os.getpid())
script = os.path.join(OUT, tag + '.script')
with open(script, 'w') as f:
    f.write('0 dmm dcv:1.0\n')
end = 150.0
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
        sys.exit('chopseqtest: no USB/IP device on port %d' % A.port)

    def q(line, timeout=60):
        t.write(line)
        return t.read(maxlen=1 << 16, timeout=timeout)

    def qs(line):
        return q(line).decode(errors='replace').strip()

    def up():
        return int(qs('DBG:PING?').split(',')[0]) / 1000.0

    def wait_dev(s):
        t0 = up()
        while up() < t0 + s:
            time.sleep(0.05)

    def session(osr, chop, dur):
        t.write('LOG:CHOP %s;:LOG:OSR %d;:LOG:STAT ON' % (chop, osr))
        recs = []
        t0 = up()
        while up() < t0 + dur:
            recs += dm40scpi.log_records(q('LOG:DATA?'))
        t.write('LOG:STAT OFF')
        recs += dm40scpi.log_records(q('LOG:DATA?'))
        return recs

    def untag(raw):
        u = raw & 0xFFFFFFFF
        return u >> 30 & 1, u >> 28 & 1, u >> 29 & 1

    def session(seq, dur):
        t.write('DBG:AD2:CHOPSEQ %d,%d,%d' % seq)
        t.write('LOG:CHOP OFF;:LOG:OSR 6;:LOG:STAT ON')
        recs = []
        t0 = up()
        while up() < t0 + dur:
            recs += dm40scpi.log_records(q('LOG:DATA?'))
        cs = qs('DBG:AD2:CHOPSEQ?')
        t.write('LOG:STAT OFF')
        recs += dm40scpi.log_records(q('LOG:DATA?'))
        return recs, cs

    wait_dev(3.0)
    for c in ['CONF:VOLT:DC 6', 'VOLT:RANG 6', 'LOG:DEST HOST', 'LOG:SOUR AD1', 'LOG:DUR 60', 'DBG:AD2:DRAW 0']:
        t.write(c)
    wait_dev(2.0)
    for seq in [(1, 4, 0), (1, 1, 0), (2, 0, 0), (3, 6, 0), (1, 3, 30), (0, 0, 0)]:
        recs, cs = session(seq, 8.0)
        tg = [untag(r[2]) for r in recs]
        why = []
        if len(recs) < 50:
            why.append('%d records' % len(recs))
        if seq[0] == 0:
            if any(x[0] for x in tg):
                why.append('tagged records with CHOPSEQ 0')
        else:
            if not all(x[0] for x in tg):
                why.append('untagged records')
            # pol of record i+1 = pol of i, flipped when i announced a write
            # (the write lands before the next read: delay < P).
            for i, (a, b) in enumerate(zip(tg, tg[1:])):
                if b[1] != a[1] ^ a[2]:
                    why.append('pol sequence broken at %d' % i)
                    break
            flips = sum(x[2] for x in tg)
            w = int(cs.split(',')[4])
            if abs(w - flips) > 1:
                why.append('writes %d vs flips %d' % (w, flips))
            if seq[0] == 1:
                runs, n = [], 1
                for a, b in zip(tg, tg[1:]):
                    if b[1] == a[1]:
                        n += 1
                    else:
                        runs.append(n)
                        n = 1
                if any(r != seq[1] for r in runs[1:]):
                    why.append('run lengths %s' % sorted(set(runs[1:])))
            if seq[0] == 3 and not any(x[1] for x in tg):
                why.append('no 01 conversion')
        e = qs('SYST:ERR?')
        if not e.startswith('0'):
            why.append(e)
        print('  %s CHOPSEQ %s: %d records, cs %s%s' % ('ok ' if not why else 'BAD', seq, len(recs), cs, (' ' + '; '.join(why)) if why else ''), flush=True)
        if why:
            bad.append('%s: %s' % (seq, '; '.join(why)))
finally:
    t = None
    if emu.poll() is None:
        os.killpg(emu.pid, signal.SIGTERM)
    emu.wait()
    for x in (script, os.path.join(OUT, 'flash-%s.bin' % tag), os.path.join(OUT, tag + '.resc')):
        if os.path.exists(x):
            os.remove(x)
print('chopseqtest: %s' % ('PASS' if not bad else 'FAIL ' + '; '.join(bad)))
sys.exit(1 if bad else 0)
