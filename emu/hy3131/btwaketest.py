#!/usr/bin/env python3
"""fw-rs DMM rail wake (PE2) in the emulator: device check 2026-10-21,
"Bluetooth (0667175)" 1g / 1h. Needs the HY3131 model's PE2 power gate
(HY3131.RailModel, on by default: with PE2 low the chip is unpowered, a PE2
rise is a power-on reset and the chip ignores the bus for PowerUpMs).

  emu/hy3131/btwaketest.py --app fw-rs/target/native/thumbv7em-none-eabihf/release/dm40-fw [-- run.py args]

Timed by the run.py script (virtual s): DC 1 V; MENU -> Oscilloscope (the
DMM rail goes off); a BT host (run.py --bt-port, wall clock, gated on the
script's marker screenshots) links on the Scope page (1g: the DMM must wake);
2 V; FETC? / READ? must read 2 V; link closed and 3 more connect / close
cycles; MENU -> Multimeter, 3 V: FETC? 3 V (1h); then a chip power-on reset
the firmware did not cause (`hy3131 BrownOut`) and 4 V: FETC? 4 V (the stall
watchdog's row read-back). SYST:RAIL:STATe? (dmm,scope,wakes,lost,silent) is
printed at each step. 3bae462 fails 1g and 1h here (READ? kept host_dmm on,
so the rail never went off and the page switch did not restart the row).
Exit status 0 = passed.
"""
import argparse
import os
import socket
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
EMU = os.path.dirname(HERE)
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='fw-rs native ELF')
ap.add_argument('--port', type=int, default=4833, help='BT passthrough TCP port (default 4833)')
ap.add_argument('rest', nargs=argparse.REMAINDER, help='-- further emu/run.py arguments (--base, --disk, ...)')
A = ap.parse_args()
rest = A.rest[1:] if A.rest[:1] == ['--'] else A.rest

mk = tempfile.mkdtemp(prefix='btwake-')
shot = lambda t, n: '%s shot %s/%s.png' % (t, mk, n)
script = ['0 dmm dcv:1.0', '3.0 key menu', '3.6 tap 339 105', shot(4.5, 'A'),
          '6.0 dmm dcv:2.0', shot(6.0, 'B'), shot(9.0, 'C'),
          '12.0 key menu', '12.6 tap 141 105', '13.0 dmm dcv:3.0', shot(16.0, 'D'),
          '16.5 monitor sysbus.hy3131 BrownOut', '16.5 dmm dcv:4.0', shot(21.0, 'E')]
sp = os.path.join(mk, 'script.txt')
open(sp, 'w').write('\n'.join(script) + '\n')
cmd = [sys.executable, os.path.join(EMU, 'run.py'), 'fwrs', '--app', A.app, '--script', sp, '--run-for', '21.5',
       '--bt-port', str(A.port), '--name', 'btwake'] + rest
p = subprocess.Popen(cmd, stdout=open(os.path.join(mk, 'run.out'), 'w'), stderr=subprocess.STDOUT)


def wait(n):
    while not os.path.exists(os.path.join(mk, n + '.png')):
        if p.poll() is not None:
            sys.exit('emulator ended before marker %s (%s)' % (n, os.path.join(mk, 'run.out')))
        time.sleep(0.2)


class Link:
    def __init__(self):
        for _ in range(100):
            try:
                self.s = socket.create_connection(('127.0.0.1', A.port), timeout=60)
                break
            except OSError:
                time.sleep(0.2)
        self.buf = b''

    def q(self, c):
        self.s.sendall(c.encode() + b'\n')
        while b'\n' not in self.buf:
            d = self.s.recv(4096)
            if not d:
                return None
            self.buf += d
        line, _, self.buf = self.buf.partition(b'\n')
        return line.decode().strip()

    def close(self):
        self.s.close()
        time.sleep(1.0)


res = {}
wait('A')
link = Link()
print('Scope page, link up: RAIL', link.q('SYST:RAIL:STAT?'), 'FETC', link.q('FETC?'), flush=True)
wait('C')
res['1g'] = link.q('FETC?')
print('1g 1 -> 2 V on Scope: FETC', res['1g'], 'READ', link.q('READ?'), 'RAIL', link.q('SYST:RAIL:STAT?'), flush=True)
link.close()
for i in range(3):
    link = Link()
    print('cycle', i, 'RAIL', link.q('SYST:RAIL:STAT?'), flush=True)
    link.close()
wait('D')
link = Link()
res['1h'] = link.q('FETC?')
print('1h DMM page, 2 -> 3 V: FETC', res['1h'], 'RAIL', link.q('SYST:RAIL:STAT?'), flush=True)
link.close()
wait('E')
link = Link()
res['por'] = link.q('FETC?')
print('chip POR on the DMM page, 3 -> 4 V: FETC', res['por'], 'RAIL', link.q('SYST:RAIL:STAT?'), flush=True)
link.close()
p.wait()
ok = all(res[k] and abs(float(res[k]) - v) < 0.01 for k, v in (('1g', 2), ('1h', 3), ('por', 4)))
print('PASS' if ok else 'FAIL', res)
sys.exit(0 if ok else 1)
