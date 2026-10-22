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
    dm40scpi.py log out.csv --source ad2 [--rate 2400] [--chop on|off] -d 10
                                              FR-38 fast stream (LOG:* SCPI, USB
                                              only): every conversion of the
                                              HY3131 AD1 (DC rows, --osr) or AD2
                                              (AC mV / AC A rows DC-coupled, AC V
                                              AC-coupled; 600..9600 S/s) with
                                              seq, device time, raw code, the
                                              scaled value (AD2) and the
                                              conversions lost before each row;
                                              requested vs measured rate and the
                                              loss statistics in the trailer
    dm40scpi.py ad2 torn|noise [-n 2000] [--sync] [--rungs ...]
                                              AD2 device experiments (fw-rs
                                              DBG:AD2:TORN? / NOISE?, meter in an
                                              AC mV / A row): torn-read
                                              histogram / noise and ENOB per rung
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

Emulator: --usbip BUSID (e.g. 1-0; USBIP_PORT=3293 for a server on another
port) talks USBTMC to emu/run.py ... --usbip through emu/usbip_client.py.

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


class UsbipLink:
    """USBTMC over the emulator's USB/IP server (emu/usbip_client.py)."""

    def __init__(self, busid):
        sys.path.insert(0, os.path.join(HERE, '..', '..', 'emu'))
        import usbip_client
        self.t = usbip_client.Usbtmc(usbip_client.Dev(busid))

    def query(self, line):
        self.t.write(line)
        if not any(c.strip().split(None, 1)[0].endswith('?') for c in line.split(';') if c.strip()):
            return None
        return self.t.read(maxlen=8192, timeout=20).decode(errors='replace').strip()

    def query_raw(self, line):
        self.t.write(line)
        return self.t.read(maxlen=8192, timeout=20)

    def close(self):
        pass


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


# ---------------------------------------------------------- FR-38 fast stream
# AD2 scaling (dmm.rs poll_ac, docs/dmm-input-scope-feasibility.md 3.3): code =
# IG * Vin / VR * 2^18 with IG = 1.0 and VR = 1.25 V in every stock AC row.
AD2_V = 1.25 / 262144.0
K_ACV = [10.0, 100.0, 1000.0, 10000.0]   # AC V divider per range (AC-coupled)
MV_DIV = [10.0, 1.0]                      # OP gain per range (mV, A rows)
SHUNT = {4: 0.01, 5: 0.5, 6: 47.5}        # ohm: A, mA, uA


def ad2_scale(mode, rng):
    """(SI per code, unit) of an AD2 code in DMM mode / range; calibration
    gain / offset not applied (the stock LPF cal would be the DC scale)."""
    if mode == 0:
        return AD2_V * K_ACV[min(rng, 3)], 'V'
    if mode == 2:
        return AD2_V / MV_DIV[min(rng, 1)], 'V'
    if mode in SHUNT:
        return AD2_V / (SHUNT[mode] * MV_DIV[min(rng, 1)]), 'A'
    return None, ''


def check_err(link, what):
    e = link.query('SYST:ERR?') or ''
    if e and not e.startswith('0') and not e.startswith('+0'):
        raise IOError('%s: %s' % (what, e))


def log_records(raw):
    """(seq, ts_us, code) of a LOG:DATA? #4nnnn block."""
    import struct
    if not raw.startswith(b'#4'):
        raise IOError('LOG:DATA? answered %r' % bytes(raw[:20]))
    n = int(raw[2:6])
    d = raw[6:6 + n]
    return [struct.unpack_from('<IIi', d, o) for o in range(0, n - n % 12, 12)]


def do_stream(link, args):
    if args.bt:
        print('dm40scpi: warning: Bluetooth carries ~1 kS/s at best; the ring overflows (dropped: timestamp gaps, LOG:COUNt? overwritten)', file=sys.stderr)
    if args.conf:
        switch(link, args.conf)
    ad2 = args.source == 'ad2'
    link.query('LOG:SOUR %s' % ('AD2' if ad2 else 'AD1'))
    if args.chop:
        link.query('LOG:CHOP %s' % args.chop.upper())
    if ad2:
        link.query('LOG:SRAT %d' % args.rate)
    elif args.osr is not None:
        link.query('LOG:OSR %d' % args.osr)
    check_err(link, 'LOG settings')
    link.query('LOG:STAT ON')
    check_err(link, 'LOG:STAT ON (AD2: AC mV / AC A / AC V; AD1: DC V / mV / A)')
    pre = [int(x) for x in link.query('LOG:PRE?').split(',')]
    mode, rng, osr, nom_us, _, chop = pre[:6]
    req, nom_mhz, r26, coupling = (pre[7], pre[8], pre[9], pre[10]) if len(pre) > 10 else (0, 0, 0, 0)
    period_us = 1e9 / nom_mhz if nom_mhz else float(nom_us)
    scale, unit = ad2_scale(mode, rng) if ad2 else (None, '')
    rows, last_seq, last_ts, wrap = 0, None, None, 0
    t_start = time.monotonic()
    with open(args.out, 'w', newline='') as f:
        f.write('# source,%s\n# mode,%d\n# range,%d\n# chop,%d\n' % (args.source.upper(), mode, rng, chop))
        if ad2:
            f.write('# requested_sps,%d\n# nominal_sps,%.3f\n# r26,0x%02X\n# coupling,%s\n'
                    % (req, nom_mhz / 1000.0, r26, 'AC' if coupling else 'DC'))
            f.write('# scale,%.9g %s/code (IG 1.0, VR 1.25 V, stock range factor; calibration NOT applied)\n' % (scale or 0, unit))
        else:
            f.write('# osr,%d\n# nominal_period_us,%d\n# scale,raw AD1 counts (1.389 V / 8388607 * range decade, uncalibrated)\n' % (osr, nom_us))
        f.write('# start,%s\n' % iso_now())
        f.write('seq,t_s,raw,%s,lost_before\n' % ('value_' + unit if ad2 else 'value'))

        def drain():
            nonlocal rows, last_seq, last_ts, wrap
            got = 0
            while True:
                recs = log_records(link.query_raw('LOG:DATA?'))
                for seq, ts, code in recs:
                    if last_ts is not None and ts < last_ts:
                        wrap += 1 << 32
                    t = (ts + wrap) / 1e6
                    lost = 0
                    if last_seq is not None:
                        # conversions between two kept rows: from the time (stalls) or the seq (ring overflow)
                        lost = max(round((ts + wrap - last_t_us[0]) / period_us) - 1, seq - last_seq - 1, 0)
                    last_t_us[0] = ts + wrap
                    f.write('%d,%.6f,%d,%s,%d\n' % (seq, t, code, '%.7g' % (code * scale) if scale else '', lost))
                    last_seq, last_ts = seq, ts
                    rows += 1
                got += len(recs)
                if len(recs) < 500:
                    return got

        last_t_us = [0]
        try:
            while True:
                if args.duration and time.monotonic() - t_start >= args.duration:
                    break
                if args.n and rows >= args.n:
                    break
                if drain() == 0:
                    time.sleep(0.005)
                if not args.quiet:
                    print('\r%d samples   ' % rows, end='', flush=True)
        except KeyboardInterrupt:
            pass
        link.query('LOG:STAT OFF')
        drain()
        cnt = [int(x) for x in link.query('LOG:COUN?').split(',')]
        gaps = (link.query('LOG:GAP?') or '').strip()
        hz = int(link.query('LOG:RATE?') or 0)
        f.write('# end,%s\n# measured_sps,%d\n# count,buffered %d captured %d overwritten %d missed %d lost %d\n'
                % (iso_now(), hz, *(cnt + [0] * 5)[:5]))
        f.write('# gaps,bad max_gap_us g2 g3_4 g5_16 g17_128 g129+: %s\n' % gaps.replace(',', ' '))
    if not args.quiet:
        print()
    lost = cnt[4] if len(cnt) > 4 else 0
    due = cnt[1] + lost
    print('%s: %d samples; requested %s S/s, chip %.1f S/s, measured %d S/s; lost %d of %d (%.2f %%), '
          'ring overwritten %d; gaps (bad,max_us,2,3-4,5-16,17-128,>128) %s'
          % (args.out, rows, req if ad2 else '-', (nom_mhz / 1000.0) if nom_mhz else 1e6 / max(nom_us, 1), hz,
             lost, due, 100.0 * lost / max(due, 1), cnt[2], gaps))
    return 0


def do_ad2(link, args):
    """Sweep DBG:AD2:TORN? / NOISE? over R26 rungs (ad2exp.rs)."""
    import math
    # Refused (no reply: the query would time out) outside an AC row or while logging.
    f = (link.query('FUNC?') or '').strip('"').upper()
    if f not in ('VOLT:AC', 'CURR:AC') or (link.query('LOG:STAT?') or '0').strip() != '0':
        sys.exit('dm40scpi: ad2 needs AC V / AC mV / AC A (not AC+DC) and no log session (FUNC? %s)' % f)
    rungs = [int(x, 0) for x in args.rungs.split(',')] if args.rungs else \
        [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x2D]
    for r26 in rungs:
        if args.what == 'torn':
            r = link.query('DBG:AD2:TORN? %d,%d%s' % (r26, args.n, ',SYNC' if args.sync else ''))
            check_err(link, 'DBG:AD2:TORN?')
            head, hist = r.split(';')
            v = [int(x) for x in head.split(',')]
            h = [int(x) for x in hist.split(',')]
            print('r26 0x%02X %7.0f S/s n %d timeouts %d same %d diff(new conv) %d diff(no conv) %d signbad %d | '
                  'bits: %s' % (v[0], 1e9 / v[1], v[2], v[3], v[4], v[5], v[6], v[7],
                                ' '.join('%d:%d' % (k, c) for k, c in enumerate(h) if c)), flush=True)
        else:
            r = link.query('DBG:AD2:NOISE? %d,%d%s' % (r26, args.n, (',%d' % args.op1chop) if args.op1chop is not None else ''))
            check_err(link, 'DBG:AD2:NOISE?')
            v = [int(x) for x in r.split(',')]
            sd = v[6] / 1000.0
            enob = math.log2(2 ** 19 / sd) if sd > 0 else float('inf')
            print('r26 0x%02X %7.0f S/s n %d timeouts %d signbad %d mean %.1f sd %.2f codes min %d max %d '
                  'ENOB %.2f readout %.0f us/sample' % (v[0], 1e9 / v[1], v[2], v[3], v[4], v[5] / 1000.0, sd,
                                                        v[7], v[8], enob, v[9] / max(v[2], 1)), flush=True)
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
    out = args.out or args.name.replace('\\', '/').split('/')[-1]
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
    p.add_argument('--usbip', metavar='BUSID', help='the emulator\'s USB/IP server (emu/run.py --usbip), e.g. 1-0')
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
    s = sub.add_parser('log', help='log readings (or, with --source, the FR-38 fast stream) to a CSV file')
    s.add_argument('out')
    s.add_argument('--source', choices=['ad1', 'ad2'], help='FR-38 fast stream from this converter (LOG:SOURce)')
    s.add_argument('--rate', type=int, default=2400, help='(ad2) requested S/s: 1200, 2400, 4800, 9600 (nearest rung; ~6.5 kS/s link limit)')
    s.add_argument('--chop', choices=['on', 'off'], help='(stream) converter chopper (default on; ad2: same rungs either way)')
    s.add_argument('--osr', type=int, help='(ad1) AD1OSR code 0..7')
    s.add_argument('-i', '--interval', type=float, default=1.0)
    s.add_argument('-n', type=int, default=0, help='stop after N readings')
    s.add_argument('-d', '--duration', type=float, default=0, help='stop after S seconds')
    s.add_argument('--conf', help='switch to this function first')
    s.add_argument('-q', '--quiet', action='store_true')
    s.set_defaults(func_=lambda link, a: (do_stream if a.source else do_log)(link, a))
    s = sub.add_parser('ad2', help='AD2 device experiments (DBG:AD2:TORN? / NOISE?)')
    s.add_argument('what', choices=['torn', 'noise'])
    s.add_argument('-n', type=int, default=2000, help='pairs / samples per rung (max 2000)')
    s.add_argument('--sync', action='store_true', help='(torn) read right after AD2F instead of blind')
    s.add_argument('--op1chop', type=int, choices=[0, 1, 2, 3], help='(noise) R33 OP1CHOP for the run')
    s.add_argument('--rungs', help='R26 low bits, comma separated (default: /2 OSR 32..1024 plain and chopped, /4 OSR 1024 chopped)')
    s.set_defaults(func_=do_ad2)
    s = sub.add_parser('get', help='copy a disk file (LOGnnn.CSV = generated from LOGnnn.DLG)')
    s.add_argument('name', help='e.g. LOG000.CSV, LOGS/LOG000.DLG, SHOTS/S000.BMP (MMEM:CAT? lists them)')
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
    link = BtLink(args) if args.bt else UsbipLink(args.usbip) if args.usbip else UsbLink()
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
