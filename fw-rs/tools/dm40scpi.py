#!/usr/bin/env python3
"""DM40 (fw-rs) SCPI host client: readings, CSV logging, function switching
(UF-38, docs/user-feedback.md section 10). Over USBTMC (default; the
transport of dm40dbg.py, /dev/usbtmcN of VID:PID 1209:0001) or Bluetooth
(--bt; the transport of dm40bt_scpi.py: RFCOMM channel 1, or --dev
/dev/rfcomm0; needs /usr/bin/python3 for socket.AF_BLUETOOTH).

    dm40scpi.py info                          *IDN?, function, CONFigure?
    dm40scpi.py conf dcv|acv|acdcv|dci|aci|acdci|res|cont|diode|cap|freq|temp
                                              switch the function (or a raw
                                              SCPI function: conf VOLT:AC)
    dm40scpi.py read [-n 10] [-i 0.5] [--conf FUNC]
                                              print readings (READ? = the next
                                              fresh reading)
    dm40scpi.py log out.csv [-i 1] [-n N | -d SECONDS] [--conf FUNC]
                                              CSV: '.' decimals, ',' separator,
                                              ISO-8601 local time with offset,
                                              '#' header lines (model, fw,
                                              function, CONFigure?); Ctrl-C
                                              ends it cleanly
    dm40scpi.py get LOG000.CSV [-o f.csv]     copy a disk file (MMEM:DATA?): a
                                              LOGnnn.CSV is generated from the
                                              binary LOGnnn.DLG on the meter
                                              (or copy the .DLG and decode it
                                              with tools/dlog.py)
    dm40scpi.py scpi 'CMD' ...                send commands / queries in order
    dm40scpi.py --bt btpoll [-p 50] [-d 10]   UF-36 check: poll the stock
                                              phone-app measurement frame (cmd
                                              09) every P ms like the app, print
                                              replies/s, latency and how often
                                              the value changed

Bluetooth options (with --bt): --mac AA:BB:.. (or DM40_MAC; found via
bluetoothctl when omitted), --dev /dev/rfcomm0, --rescan. Close the phone app
first (one link at a time). SCPI reference: fw-rs/src/scpi.rs header.
"""
import argparse
import datetime
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

FUNCS = {
    'dcv': 'VOLT:DC', 'acv': 'VOLT:AC', 'acdcv': 'VOLT:ACDC',
    'dci': 'CURR:DC', 'aci': 'CURR:AC', 'acdci': 'CURR:ACDC',
    'res': 'RES', 'cont': 'CONT', 'diode': 'DIOD', 'cap': 'CAP',
    'freq': 'FREQ', 'temp': 'TEMP',
}
# FUNCtion? answer -> unit of READ? (SI, as scpi.rs replies).
UNITS = {'VOLT': 'V', 'CURR': 'A', 'RES': 'Ohm', 'CONT': 'Ohm', 'DIOD': 'V',
         'CAP': 'F', 'FREQ': 'Hz', 'TEMP': 'C'}
OVERLOAD = 9.9e37


class UsbLink:
    """USBTMC through dm40dbg.Link (one message per write, reply per read)."""

    def __init__(self):
        import dm40dbg
        self.l = dm40dbg.Link()

    def query(self, line):
        self.l.write(line)
        if not any(c.strip().split(None, 1)[0].endswith('?') for c in line.split(';') if c.strip()):
            return None
        return self.l.read().decode(errors='replace').strip()

    def query_raw(self, line):
        self.l.write(line)
        return self.l.read()

    def close(self):
        os.close(self.l.fd)


class BtLink:
    """Bluetooth through dm40bt_scpi.Link (SCPI lines on the stock serial)."""

    def __init__(self, args):
        import dm40bt_scpi
        self.mod = dm40bt_scpi
        self.l = dm40bt_scpi.connect(args)
        self.l.drain(0.3)

    def query(self, line):
        r = self.l.query(line)
        if r is None:
            return None
        data, block = r
        return data if block else data.decode(errors='replace').strip()

    def query_raw(self, line):
        r = self.l.query(line)
        if r is None:
            return b''
        data, block = r
        return b'#4%04d' % len(data) + data if block else data

    def close(self):
        self.l.close()


def scpi_func(name):
    return FUNCS.get(name.lower(), name)


def unit_of(func_answer):
    f = func_answer.strip().strip('"').upper()
    return UNITS.get(f.split(':')[0], '')


def iso_now():
    return datetime.datetime.now().astimezone().isoformat(timespec='milliseconds')


def switch(link, func):
    link.query('CONF:%s' % scpi_func(func))
    f = link.query('FUNC?')
    err = link.query('SYST:ERR?')
    if err and not err.startswith('0') and not err.startswith('+0'):
        sys.exit('dm40scpi: CONF:%s: %s' % (scpi_func(func), err))
    return f


def reading(link):
    """(value float, overload bool) of READ?."""
    r = link.query('READ?')
    try:
        v = float(r)
    except (TypeError, ValueError):
        raise IOError('READ? answered %r' % r)
    return v, abs(v) >= OVERLOAD


def do_info(link, args):
    print('IDN       %s' % link.query('*IDN?'))
    print('function  %s' % link.query('FUNC?'))
    print('config    %s' % link.query('CONF?'))
    return 0


def do_conf(link, args):
    print(switch(link, args.func))
    return 0


def do_read(link, args):
    if args.conf:
        switch(link, args.conf)
    unit = unit_of(link.query('FUNC?') or '')
    k = 0
    while args.n == 0 or k < args.n:
        t0 = time.monotonic()
        v, ol = reading(link)
        print('%s  %s %s' % (iso_now(), 'OL' if ol else '%.7g' % v, unit), flush=True)
        k += 1
        dt = args.interval - (time.monotonic() - t0)
        if dt > 0:
            time.sleep(dt)
    return 0


def do_log(link, args):
    if args.conf:
        switch(link, args.conf)
    func = (link.query('FUNC?') or '').strip('"')
    unit = unit_of(func)
    idn = link.query('*IDN?') or ''
    parts = [p.strip() for p in idn.split(',')]
    model = parts[1] if len(parts) > 1 else idn
    fw = parts[3] if len(parts) > 3 else ''
    conf = link.query('CONF?') or ''
    n = 0
    t_start = time.monotonic()
    with open(args.out, 'w', newline='') as f:
        f.write('# model,%s\n# fw,%s\n# function,%s\n# config,%s\n# unit,%s\n# start,%s\n'
                % (model, fw, func, conf.replace(',', ';'), unit, iso_now()))
        f.write('time,elapsed_s,value,unit,overload\n')
        try:
            while True:
                if args.n and n >= args.n:
                    break
                if args.duration and time.monotonic() - t_start >= args.duration:
                    break
                t0 = time.monotonic()
                v, ol = reading(link)
                f.write('%s,%.3f,%s,%s,%d\n' % (iso_now(), t0 - t_start, '' if ol else repr(v), unit, ol))
                f.flush()
                n += 1
                if not args.quiet:
                    print('\r%d readings, last %s %s   ' % (n, 'OL' if ol else '%.7g' % v, unit), end='', flush=True)
                dt = args.interval - (time.monotonic() - t0)
                if dt > 0:
                    time.sleep(dt)
        except KeyboardInterrupt:
            pass
    if not args.quiet:
        print()
    print('%s: %d readings' % (args.out, n))
    return 0


def get_file(link, name, out=None, progress=None):
    """A disk file over MMEM:DATA? (6000-byte pieces). "LOGnnn.CSV" of a
    LOGnnn.DLG binary log is the CSV the meter generates from it."""
    data = bytearray()
    while True:
        r = link.query_raw('MMEM:DATA? "%s",%d' % (name, len(data)))
        if not r.startswith(b'#4'):
            err = link.query('SYST:ERR?') or ''
            raise IOError('MMEM:DATA? %s: %r (%s)' % (name, bytes(r[:40]), err))
        n = int(r[2:6])
        data += r[6:6 + n]
        if progress:
            progress(len(data))
        if n < 6000:
            break
    if out:
        open(out, 'wb').write(data)
    return bytes(data)


def do_get(link, args):
    out = args.out or args.name
    t0 = time.monotonic()
    prog = None if args.quiet else (lambda n: print('\r%d bytes' % n, end='', flush=True))
    d = get_file(link, args.name, out, prog)
    if not args.quiet:
        print()
    print('%s: %d bytes in %.1f s' % (out, len(d), time.monotonic() - t0))
    return 0


def do_scpi(link, args):
    for c in args.commands:
        r = link.query(c)
        if r is not None:
            print(r if isinstance(r, str) else '<%d-byte block>' % len(r))
    return 0


def stock_frame(cmd, data=b''):
    """Phone-app request AF 05 03 <cmd> <len> <data> <crc> (byte sum 0)."""
    b = bytes([0xAF, 0x05, 0x03, cmd, len(data)]) + data
    return b + bytes([(-sum(b)) & 0xFF])


def do_btpoll(link, args):
    """UF-36: the phone app polls cmd 09 (measurement) every 50..1000 ms."""
    if not isinstance(link, BtLink):
        sys.exit('dm40scpi: btpoll needs --bt (it uses the stock Bluetooth frame protocol)')
    l = link.l
    req = stock_frame(0x09)
    period = args.period / 1000.0
    lat, vals, sent, got = [], [], 0, 0
    t_end = time.monotonic() + args.duration
    nxt = time.monotonic()
    while time.monotonic() < t_end:
        t0 = time.monotonic()
        l.write(req)
        sent += 1
        # Reply DF 05 03 09 0B <11 bytes> <crc>; skip anything else.
        deadline = t0 + max(period, 0.5)
        try:
            while True:
                while len(l.buf) < 17:
                    l._recv(deadline)
                i = l.buf.find(b'\xdf\x05\x03\x09\x0b')
                if i < 0:
                    l.buf = l.buf[-4:]
                    continue
                if len(l.buf) < i + 17:
                    l._recv(deadline)
                    continue
                fr, l.buf = l.buf[i:i + 17], l.buf[i + 17:]
                if sum(fr) & 0xFF == 0:
                    got += 1
                    lat.append(time.monotonic() - t0)
                    vals.append(fr[5 + 9:5 + 11])
                    break
        except link.mod.Timeout:
            pass
        nxt += period
        dt = nxt - time.monotonic()
        if dt > 0:
            time.sleep(dt)
        else:
            nxt = time.monotonic()
    changes = sum(1 for a, b in zip(vals, vals[1:]) if a != b)
    span = args.duration
    print('polled every %d ms for %.0f s: %d requests, %d replies (%.1f/s)' % (args.period, span, sent, got, got / span))
    if lat:
        lat.sort()
        print('latency ms: median %.1f, 95%% %.1f, max %.1f' % (lat[len(lat) // 2] * 1e3, lat[int(len(lat) * 0.95)] * 1e3, lat[-1] * 1e3))
    print('main value changed %d times (%.1f/s; the DMM conversion rate bounds this)' % (changes, changes / span))
    return 0 if got >= sent * 0.95 else 1


def main():
    p = argparse.ArgumentParser(description='DM40 fw-rs SCPI client (USBTMC or Bluetooth).',
                                formatter_class=argparse.RawDescriptionHelpFormatter, epilog=__doc__)
    p.add_argument('--bt', action='store_true', help='use Bluetooth instead of USBTMC')
    p.add_argument('--mac', default=os.environ.get('DM40_MAC'), help='(--bt) meter MAC')
    p.add_argument('--dev', help='(--bt) bound serial device, e.g. /dev/rfcomm0')
    p.add_argument('--rescan', action='store_true', help='(--bt) search the address again')
    p.add_argument('--timeout', type=float, default=5.0, help='(--bt) reply timeout, s')
    sub = p.add_subparsers(dest='cmd', required=True)
    s = sub.add_parser('info', help='*IDN?, FUNC?, CONF?')
    s.set_defaults(func_=do_info)
    s = sub.add_parser('conf', help='switch the DMM function')
    s.add_argument('func', help=' | '.join(FUNCS) + ' or a SCPI function')
    s.set_defaults(func_=do_conf)
    s = sub.add_parser('read', help='print readings')
    s.add_argument('-n', type=int, default=10, help='readings (0 = forever)')
    s.add_argument('-i', '--interval', type=float, default=0.0, help='seconds between readings (0 = as fast as they come)')
    s.add_argument('--conf', help='switch to this function first')
    s.set_defaults(func_=do_read)
    s = sub.add_parser('log', help='log readings to a CSV file')
    s.add_argument('out')
    s.add_argument('-i', '--interval', type=float, default=1.0)
    s.add_argument('-n', type=int, default=0, help='stop after N readings')
    s.add_argument('-d', '--duration', type=float, default=0, help='stop after S seconds')
    s.add_argument('--conf', help='switch to this function first')
    s.add_argument('-q', '--quiet', action='store_true')
    s.set_defaults(func_=do_log)
    s = sub.add_parser('get', help='copy a disk file (LOGnnn.CSV = generated from LOGnnn.DLG)')
    s.add_argument('name', help='e.g. LOG000.CSV, LOG000.DLG, S000.BMP (MMEM:CAT? lists them)')
    s.add_argument('-o', '--out', help='output file (default: the name)')
    s.add_argument('-q', '--quiet', action='store_true')
    s.set_defaults(func_=do_get)
    s = sub.add_parser('scpi', help='send commands / queries')
    s.add_argument('commands', nargs='+')
    s.set_defaults(func_=do_scpi)
    s = sub.add_parser('btpoll', help='UF-36: stock cmd-09 poll rate check (--bt)')
    s.add_argument('-p', '--period', type=int, default=50, help='poll period, ms (the app: 50)')
    s.add_argument('-d', '--duration', type=float, default=10.0)
    s.set_defaults(func_=do_btpoll)
    args = p.parse_args()
    link = BtLink(args) if args.bt else UsbLink()
    try:
        return args.func_(link, args)
    except KeyboardInterrupt:
        return 130
    except (IOError, OSError, ConnectionError) as e:
        print('dm40scpi: %s' % e, file=sys.stderr)
        return 1
    finally:
        link.close()


if __name__ == '__main__':
    sys.exit(main())
