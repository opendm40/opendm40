#!/usr/bin/env python3
"""SCPI over Bluetooth for a DM40 running fw-rs (docs/bt-scpi.md).

fw-rs answers SCPI text lines on the same Bluetooth serial link as the stock
phone-app protocol: a line starting with a letter, '*' or ':' and ending in
'\\n' goes to the same parser as USBTMC; the reply comes back ending in '\\n'
(a #-block too: #<n><len><data>\\n). DBG:* commands are refused over
Bluetooth (no reply; SYST:ERR? then says "Command protected").

    dm40bt_scpi.py scpi '*IDN?'                 one command / query
    dm40bt_scpi.py scpi 'CONF:VOLT' 'READ?'     several, in order
    dm40bt_scpi.py scpi 'WAV:DATA?' -o w.bin    save a #-block payload
    dm40bt_scpi.py shell                        interactive prompt (default)
    dm40bt_scpi.py bench [-n 20]                READ? / WAV:DATA? timing

Connection: RFCOMM channel 1 on the meter's classic (SPP) address, found like
dist/dm40-memrw-kit/tools/dm40_memrw.py does (bluetoothctl, cached in
~/.config/dm40-memrw/mac), or --mac / DM40_MAC, or --dev /dev/rfcomm0 for a
bound serial device. Needs Python with socket.AF_BLUETOOTH (on Linux:
/usr/bin/python3). Close the phone app first: one link at a time.

For PyVISA / sigrok, bind a serial device instead (see docs/bt-scpi.md):
    sudo rfcomm bind /dev/rfcomm0 <MAC> 1
    pyvisa:  rm.open_resource('ASRL/dev/rfcomm0::INSTR',
                 read_termination='\\n', write_termination='\\n', timeout=5000)
    sigrok:  sigrok-cli -d scpi-dmm:conn=/dev/rfcomm0 --scan
"""
import argparse
import os
import re
import socket
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
KIT_TOOLS = os.path.join(HERE, '..', '..', 'dist', 'dm40-memrw-kit', 'tools')


class Timeout(Exception):
    pass


def is_query(line):
    """True if any command of the message is a query (header ends in '?')."""
    for cmd in re.split(r'[;\r\n]', line):
        head = cmd.strip().split(None, 1)
        if head and head[0].endswith('?'):
            return True
    return False


def is_dbg(line):
    for cmd in re.split(r'[;\r\n]', line):
        root = re.split(r'[:?\s]', cmd.strip().lstrip(':'), 1)[0]
        if root.upper() == 'DBG':
            return True
    return False


class Link:
    """Byte stream to the meter: an RFCOMM socket or a tty (/dev/rfcomm0)."""

    def __init__(self, mac=None, dev=None, timeout=5.0):
        self.timeout = timeout
        self.buf = b''
        self.s = None
        self.fd = None
        if dev:
            import termios
            import tty
            self.fd = os.open(dev, os.O_RDWR | os.O_NOCTTY)
            tty.setraw(self.fd)
            attr = termios.tcgetattr(self.fd)
            attr[4] = attr[5] = termios.B115200    # ignored by rfcomm, fine
            termios.tcsetattr(self.fd, termios.TCSANOW, attr)
        else:
            if not hasattr(socket, 'AF_BLUETOOTH'):
                sys.exit('This Python has no Bluetooth socket support '
                         '(socket.AF_BLUETOOTH). On Linux run /usr/bin/python3, '
                         'or bind /dev/rfcomm0 and pass --dev.')
            s = socket.socket(socket.AF_BLUETOOTH, socket.SOCK_STREAM,
                              socket.BTPROTO_RFCOMM)
            s.settimeout(timeout)
            s.connect((mac, 1))
            self.s = s

    def close(self):
        if self.s is not None:
            self.s.close()
        if self.fd is not None:
            os.close(self.fd)

    def write(self, data):
        if self.s is not None:
            self.s.sendall(data)
        else:
            while data:
                data = data[os.write(self.fd, data):]

    def _recv(self, deadline):
        left = deadline - time.monotonic()
        if left <= 0:
            raise Timeout()
        if self.s is not None:
            self.s.settimeout(left)
            try:
                d = self.s.recv(4096)
            except socket.timeout:
                raise Timeout()
        else:
            import select
            r, _, _ = select.select([self.fd], [], [], left)
            if not r:
                raise Timeout()
            d = os.read(self.fd, 4096)
        if not d:
            raise ConnectionError('link closed by the meter')
        self.buf += d

    def drain(self, quiet=0.2):
        """Drop anything pending (a late reply, stock frames)."""
        end = time.monotonic() + quiet
        try:
            while True:
                self._recv(end)
        except Timeout:
            pass
        self.buf = b''

    def _take(self, n, deadline):
        while len(self.buf) < n:
            self._recv(deadline)
        d, self.buf = self.buf[:n], self.buf[n:]
        return d

    def read_reply(self, timeout=None):
        """One reply: a text line (without '\\n') or a #-block payload
        (returned as bytes with is_block = True)."""
        deadline = time.monotonic() + (timeout or self.timeout)
        # Skip stray stock frames / blank bytes before the reply.
        while True:
            while not self.buf:
                self._recv(deadline)
            if self.buf[0] in b'\r\n\x00':
                self.buf = self.buf[1:]
                continue
            break
        if self.buf[:1] == b'#':
            hdr = self._take(2, deadline)
            nd = hdr[1] - 0x30
            if not 1 <= nd <= 9:
                raise ValueError('bad block header %r' % hdr)
            n = int(self._take(nd, deadline))
            data = self._take(n, deadline)
            # the terminator (and anything else up to it)
            while b'\n' not in self.buf:
                self._recv(deadline)
            self.buf = self.buf[self.buf.index(b'\n') + 1:]
            return data, True
        while b'\n' not in self.buf:
            self._recv(deadline)
        i = self.buf.index(b'\n')
        line, self.buf = self.buf[:i], self.buf[i + 1:]
        return line.rstrip(b'\r'), False

    def query(self, line, timeout=None):
        """Send one message; return the reply (or None for a command)."""
        self.write(line.encode() + b'\n')
        if not is_query(line):
            return None
        return self.read_reply(timeout)


def resolve_mac(args):
    if args.mac:
        return args.mac
    sys.path.insert(0, os.path.abspath(KIT_TOOLS))
    try:
        from dm40_memrw import BtFinder, DiscoveryError
    except ImportError:
        sys.exit('auto-discovery needs %s/dm40_memrw.py; pass --mac '
                 'AA:BB:CC:DD:EE:FF (or set DM40_MAC) or --dev /dev/rfcomm0'
                 % os.path.normpath(KIT_TOOLS))
    try:
        return BtFinder().find(rescan=args.rescan)
    except DiscoveryError as e:
        sys.exit('dm40bt_scpi: %s' % e)


def connect(args):
    if args.dev:
        return Link(dev=args.dev, timeout=args.timeout)
    mac = resolve_mac(args)
    try:
        return Link(mac=mac, timeout=args.timeout)
    except OSError as e:
        sys.exit('dm40bt_scpi: connect %s channel 1: %s (meter on, Bluetooth '
                 'enabled in its settings, phone app closed?)' % (mac, e))


def show(reply, out=None):
    data, block = reply
    if block:
        if out:
            with open(out, 'wb') as f:
                f.write(data)
            print('#block %d bytes -> %s' % (len(data), out))
        else:
            print('#block %d bytes: %s%s' % (len(data), data[:32].hex(' '),
                                             ' ...' if len(data) > 32 else ''))
    else:
        print(data.decode('ascii', 'replace'))


def warn_dbg(line):
    if is_dbg(line):
        print('note: DBG:* is refused over Bluetooth (no reply; '
              'SYST:ERR? -> "Command protected"). Use USBTMC.',
              file=sys.stderr)
        return True
    return False


def do_scpi(link, args):
    rc = 0
    for line in args.commands:
        if warn_dbg(line):
            rc = 1
            continue
        try:
            r = link.query(line)
        except Timeout:
            print('timeout (%.1f s) waiting for the reply to %r'
                  % (args.timeout, line), file=sys.stderr)
            rc = 1
            link.drain()
            continue
        if r is not None:
            show(r, args.out)
    return rc


def do_shell(link, args):
    try:
        import readline  # noqa: F401  (line editing / history)
    except ImportError:
        pass
    print('DM40 SCPI over Bluetooth. Queries end in "?"; empty line or '
          'Ctrl-D quits.')
    while True:
        try:
            line = input('dm40> ').strip()
        except EOFError:
            print()
            break
        if not line:
            break
        if warn_dbg(line):
            continue
        t0 = time.monotonic()
        try:
            r = link.query(line)
        except Timeout:
            print('timeout (%.1f s)' % args.timeout)
            link.drain()
            continue
        if r is not None:
            show(r)
            if args.verbose:
                print('  (%.0f ms)' % ((time.monotonic() - t0) * 1000))
    return 0


def do_bench(link, args):
    print(link.query('*IDN?')[0].decode('ascii', 'replace'))
    for cmd in ('READ?', 'WAV:DATA?'):
        times, size = [], 0
        for _ in range(args.n):
            t0 = time.monotonic()
            data, _ = link.query(cmd)
            times.append(time.monotonic() - t0)
            size = len(data)
        times.sort()
        mid = times[len(times) // 2]
        print('%-10s n=%d  reply %d B  median %.0f ms  min %.0f  max %.0f'
              '  (%.1f kB/s at the median)'
              % (cmd, args.n, size, mid * 1000, times[0] * 1000,
                 times[-1] * 1000, size / mid / 1000 if mid else 0))
    print('SYST:ERR? ->', link.query('SYST:ERR?')[0].decode('ascii', 'replace'))
    return 0


def main():
    p = argparse.ArgumentParser(
        description='SCPI over Bluetooth for a DM40 running fw-rs.',
        formatter_class=argparse.RawDescriptionHelpFormatter, epilog=__doc__)
    p.add_argument('--mac', default=os.environ.get('DM40_MAC'),
                   help='meter classic Bluetooth MAC (or DM40_MAC); found via '
                        'bluetoothctl when omitted')
    p.add_argument('--dev', help='use a bound serial device (/dev/rfcomm0) '
                                 'instead of an RFCOMM socket')
    p.add_argument('--rescan', action='store_true',
                   help='ignore the cached address and search again')
    p.add_argument('--timeout', type=float, default=5.0,
                   help='reply timeout in seconds (default 5: MEAS:<f>? after '
                        'a function change takes ~3 s)')
    p.add_argument('-v', '--verbose', action='store_true',
                   help='shell: print the round-trip time')
    sub = p.add_subparsers(dest='cmd')
    s = sub.add_parser('scpi', help='send commands / queries in order')
    s.add_argument('commands', nargs='+')
    s.add_argument('-o', '--out', help='write a #-block payload to this file')
    s.set_defaults(func=do_scpi)
    s = sub.add_parser('shell', help='interactive prompt')
    s.set_defaults(func=do_shell)
    s = sub.add_parser('bench', help='READ? / WAV:DATA? round-trip timing')
    s.add_argument('-n', type=int, default=20)
    s.set_defaults(func=do_bench)
    args = p.parse_args()
    if not args.cmd:
        args.func = do_shell
    link = connect(args)
    try:
        link.drain(0.3)          # stock frames / a late reply from before
        return args.func(link, args)
    except KeyboardInterrupt:
        return 130
    except (ConnectionError, OSError) as e:
        print('dm40bt_scpi: %s' % e, file=sys.stderr)
        return 1
    finally:
        link.close()


if __name__ == '__main__':
    sys.exit(main())
