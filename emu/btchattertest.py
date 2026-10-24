#!/usr/bin/env python3
"""opendm40: the Bluetooth module's own text is not SCPI (device 46370df,
device-check doc "46370df" new bug 5: the first BT client of the session
found -113, -102, -113 in its error queue).

  emu/btchattertest.py --app firmware/opendm40.bin --port 64530

With no host linked (PC14 low) the model's BT module sends what a module
prints on its own: AT answers and an echo ("OK", "AT+SBNDM40A" = -102 with
its '+', "OK" = -113, the error sequence of the device), and a line over
256 bytes (an overrun). Then a host links: SYST:ERR:COUN? must be 0 and
SYST:ERR? "No error"; a command it sends then works (*IDN?), and a bad one
of its own is queued (-113), so the queue itself is live.
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
ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('--app', required=True, help='opendm40 native image / ELF')
ap.add_argument('--port', type=int, default=4834, help='BT passthrough TCP port (default 4834)')
A = ap.parse_args()
sys.path.insert(0, HERE)
import usbip_client as uc  # noqa: E402
if uc.port_busy(A.port):
    sys.exit('btchattertest: port %d is in use: pass a free --port' % A.port)

mk = tempfile.mkdtemp(prefix='btchat-')
CHAT = os.environ.get('BTCHAT', '1') == '1'
TAG = 'btchat-%d-%d' % (A.port, os.getpid())
text = b'OK\r\nAT+SBNDM40A\r\nOK\r\n' + b'X' * 300 + b'\r\n'
script = [] if not CHAT else ['9.0 monitor sysbus.btmodule SendHex "%s"' % text.hex(),
          ]
script.append('10.0 shot %s/A.png' % mk)
sp = os.path.join(mk, 'script.txt')
open(sp, 'w').write('\n'.join(script) + '\n')
cmd = [sys.executable, os.path.join(HERE, 'run.py'), 'fwrs', '--app', A.app, '--script', sp, '--run-for', '60',
       '--bt-port', str(A.port), '--name', TAG, '--log', os.path.join(mk, 'renode.log')]
p = subprocess.Popen(cmd, stdout=open(os.path.join(mk, 'run.out'), 'w'), stderr=subprocess.STDOUT,
                     start_new_session=True)
bad = []
try:
    while not os.path.exists(os.path.join(mk, 'A.png')):
        if p.poll() is not None:
            sys.exit('btchattertest: emulator ended before the marker (%s)' % os.path.join(mk, 'run.out'))
        time.sleep(0.2)
    s = None
    for _ in range(100):
        try:
            s = socket.create_connection(('127.0.0.1', A.port), timeout=300)
            break
        except OSError:
            time.sleep(0.2)
    if s is None:
        sys.exit('btchattertest: no BT link on port %d' % A.port)
    buf = b''

    def q(c):
        global buf
        s.sendall(c.encode() + b'\n')
        while b'\n' not in buf:
            d = s.recv(4096)
            if not d:
                return None
            buf += d
        line, _, buf = buf.partition(b'\n')
        return line.decode(errors='replace').strip()

    def check(ok, what):
        print('  %s %s' % ('ok  ' if ok else 'FAIL', what), flush=True)
        if not ok:
            bad.append(what)

    n = q('SYST:ERR:COUN?')
    check(n is not None and n.lstrip('+') == '0', 'SYST:ERR:COUN? after the module text with no link: %s (want 0)' % n)
    e = q('SYST:ERR?')
    check(e is not None and e.startswith('0'), 'SYST:ERR? %s (want 0,"No error")' % e)
    i = q('*IDN?')
    check(i is not None and 'opendm40' in i, '*IDN? %s' % i)
    s.sendall(b'NOSUCH:CMD\n')
    e = q('SYST:ERR?')
    check(e is not None and e.startswith('-113'), 'the host\'s own bad command: SYST:ERR? %s (want -113)' % e)
    s.close()
finally:
    p.terminate()
    try:
        p.wait(30)
    except subprocess.TimeoutExpired:
        pass
    subprocess.run(['pkill', '-9', '-f', TAG])
print('btchattertest: %s' % ('PASS' if not bad else 'FAIL ' + '; '.join(bad)))
sys.exit(1 if bad else 0)
