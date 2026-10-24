#!/usr/bin/env python3
"""DM40 (opendm40) SCPI host client: readings, CSV logging, function switching
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
                                              loss statistics in the trailer;
                                              -d / -n end it after the block in
                                              flight, Ctrl-C too (LOG:STAT OFF,
                                              the rest drained, the trailer; a
                                              second Ctrl-C abandons the drain)
                                              --format packed|dlg: LOG:FORMat of
                                              LOG:DATA? (3 B / ~1-3 B per sample
                                              instead of 12; the same CSV)
    dm40scpi.py ad2 torn|noise [-n 2000] [--sync] [--rungs ...]
                                              AD2 device experiments (opendm40
                                              DBG:AD2:TORN? / NOISE?, meter in an
                                              AC mV / A row): torn-read
                                              histogram / noise and ENOB per rung
    dm40scpi.py get LOG000.CSV [-o f.csv]     copy a disk file (MMEM:DATA?): a
                                              LOGnnn.CSV is generated from the
                                              binary LOGnnn.DLG on the meter
                                              (or copy the .DLG and decode it
                                              with tools/dlog.py)
    dm40scpi.py fast out.csv --source ad2 --rate 9600 --dur 30
                                              the fast logger to the meter's
                                              disk (LOG:DEST DISK; opendm40
                                              fastlog.rs): LOGS/FASTnnn.DLG,
                                              then its generated CSV copied
                                              (t_s,raw,value,lost_before);
                                              USB only
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
first (one link at a time). SCPI reference: firmware/src/scpi.rs header.
"""
import argparse
import datetime
import os
import signal
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
        if not any(c.strip().split(None, 1)[0].endswith('?') for c in line.split(';') if c.strip()):
            self.l.write(line)
            return None
        # dm40dbg.Link.query: SIGINT held during the read, a desync (empty
        # reply) repaired by a USBTMC clear
        return self.l.query(line).decode(errors='replace').strip()

    def query_raw(self, line):
        return self.l.query(line)

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
    if mode in (0, 1):  # AC V; DC V = the DC-coupled V row (E7), same taps
        return AD2_V * K_ACV[min(rng, 3)], 'V'
    if mode in (2, 3):  # AC mV; DC mV streams the AC mV row
        return AD2_V / MV_DIV[min(rng, 1)], 'V'
    if mode in SHUNT:
        return AD2_V / (SHUNT[mode] * MV_DIV[min(rng, 1)]), 'A'
    return None, ''


def check_err(link, what):
    e = link.query('SYST:ERR?') or ''
    if e and not e.startswith('0') and not e.startswith('+0'):
        raise IOError('%s: %s' % (what, e))


def log_records(raw):
    """(seq, ts_us, code) of a LOG:DATA? #4nnnn block (LOG:FORMat ROWS)."""
    import struct
    if not raw.startswith(b'#4'):
        raise IOError('LOG:DATA? answered %r' % bytes(raw[:20]))
    n = int(raw[2:6])
    d = raw[6:6 + n]
    return [struct.unpack_from('<IIi', d, o) for o in range(0, n - n % 12, 12)]


# LOG:FORMat PACKed / DLG replies (opendm40 logger.rs "Compact LOG:DATA?"):
# a 20-byte header {'LP' | 'LD', u8 version 1, u8 source (0 AD1, 1 AD2), u32
# rate mHz, f32 a, f32 b, u32 records left}, then 16-byte blocks {u32 seq, u32
# t0_us, u16 n, u8 width (3 / 4; 0 = DLG chunks follow), u8 flags (1 session
# start, 2 gap), u32 lost_before} with n samples each (PACKed) or one block
# and kind-3 chunks of all n rows (DLG, docs/compact-log-format.md).
FMT_HDR, FMT_BLK = 20, 16
LOG_FORMATS = {'rows': 'ROWS', 'packed': 'PACK', 'dlg': 'DLG'}


def log_reply(raw, fmt):
    """(rows, info) of one LOG:DATA? reply in LOG:FORMat `fmt` ('rows',
    'packed', 'dlg': the host set it; a ROWS reply can start with the
    compact formats' magic bytes, so it is not guessed). rows: (seq,
    ts_us, code, lost_before) with ts_us the low 32 bits of the device time
    (PACKed: t0 + round(i / rate), the grid) and lost_before None for ROWS
    (the caller derives it from the stamps); info: {'format', 'left',
    'blocks', 'rate_mhz', 'a', 'b', 'source'} (only 'format' for ROWS)."""
    import struct
    if not raw.startswith(b'#4'):
        raise IOError('LOG:DATA? answered %r' % bytes(raw[:20]))
    n = int(raw[2:6])
    d = bytes(raw[6:6 + n])
    if len(d) != n:
        raise IOError('LOG:DATA? block short: %d of %d bytes' % (len(d), n))
    if fmt == 'rows':
        return [(s, t, c, None) for s, t, c in log_records(raw)], {'format': 'rows'}
    magic = d[:2]
    if n == 0:
        return [], {'format': fmt, 'left': 0, 'blocks': 0}
    if n < FMT_HDR or magic != {'packed': b'LP', 'dlg': b'LD'}[fmt]:
        raise IOError('LOG:DATA? is not a %s reply: %r' % (fmt, d[:8]))
    ver, src, rate, a, b, left = struct.unpack_from('<BBIffI', d, 2)
    if ver != 1:
        raise IOError('LOG:DATA? reply version %d (this tool reads 1)' % ver)
    info = {'format': 'packed' if magic == b'LP' else 'dlg', 'left': left, 'rate_mhz': rate,
            'a': a, 'b': b, 'source': src, 'blocks': 0}
    rows = []
    o = FMT_HDR
    if magic == b'LP':
        while o < n:
            seq, t0, cnt, w, _fl, lost = struct.unpack_from('<IIHBBI', d, o)
            o += FMT_BLK
            if w not in (3, 4) or o + cnt * w > n:
                raise IOError('LOG:DATA? PACKed block at %d: width %d, %d samples' % (o - FMT_BLK, w, cnt))
            for i in range(cnt):
                code = int.from_bytes(d[o:o + w], 'little', signed=True)
                t = (t0 + (i * 1000000000 + 500 * rate // 1000) // rate) & 0xFFFFFFFF
                rows.append((seq + i, t, code, lost if i == 0 else 0))
                o += w
            info['blocks'] += 1
        return rows, info
    import dlog
    seq0, _t0, cnt, _w, _fl, lost0 = struct.unpack_from('<IIHBBI', d, o)
    o += FMT_BLK
    while o < n:
        if not dlog.chunk_ok(d, o):
            raise IOError('LOG:DATA? DLG chunk at %d: torn or CRC error' % o)
        pl, _, row0 = struct.unpack_from('<HII', d, o + 2)
        for j, (t, code, dt) in enumerate(dlog.fast_chunk(d, o, rate)):
            if not rows:
                lost = lost0
            else:
                lost = max(dlog.round_half_up(dt, rate) - 1, 0)
            rows.append((row0 + 1 + j, t & 0xFFFFFFFF, code, lost))
        o += dlog.CHDR + pl
        info['blocks'] += 1
    if len(rows) != cnt or (rows and rows[0][0] != seq0):
        raise IOError('LOG:DATA? DLG reply: %d rows for a count of %d' % (len(rows), cnt))
    return rows, info


def do_stream(link, args):
    if args.bt:
        print('dm40scpi: warning: Bluetooth carries ~870 records/s at best; the meter refuses AD2 and > 800 S/s over BT ("Rate too high for Bluetooth")', file=sys.stderr)
    if args.conf:
        switch(link, args.conf)
    ad2 = args.source == 'ad2'
    # An error another client left in the queue (e.g. "Data stale") would
    # fail the check below: clear it first.
    link.query('*CLS')
    link.query('LOG:SOUR %s' % ('AD2' if ad2 else 'AD1'))
    if args.chop:
        link.query('LOG:CHOP %s' % args.chop.upper())
    if ad2:
        link.query('LOG:SRAT %d' % args.rate)
    elif args.osr is not None:
        link.query('LOG:OSR %d' % args.osr)
    check_err(link, 'LOG settings')
    # LOG:FORMat is kept by the meter (another client may have left PACKed):
    # always set it. Firmware before LOG:FORMat answers "Undefined header"
    # and sends ROWS, which is fine for --format rows.
    link.query('LOG:FORM %s' % LOG_FORMATS[args.format])
    e = link.query('SYST:ERR?') or ''
    if e and not e.startswith('0') and not e.startswith('+0'):
        if args.format != 'rows' or 'Undefined header' not in e:
            raise IOError('LOG:FORMat %s: %s (needs opendm40 with compact LOG:DATA?)' % (LOG_FORMATS[args.format], e))
    t_start = time.monotonic()
    link.query('LOG:STAT ON')
    # From here on the logger runs: LOG:STAT OFF on every way out (Ctrl-C
    # twice, an error), not only the normal end.
    try:
        return _stream_session(link, args, ad2, t_start)
    finally:
        try:
            link.query('LOG:STAT OFF')
        except (IOError, OSError, KeyboardInterrupt):
            pass


def _stream_session(link, args, ad2, t_start):
    check_err(link, 'LOG:STAT ON (AD2: V / mV / AC A; AD1: DC V / mV / A)')
    pre = [int(x) for x in link.query('LOG:PRE?').split(',')]
    mode, rng, osr, nom_us, _, chop = pre[:6]
    req, nom_mhz, r26, coupling = (pre[7], pre[8], pre[9], pre[10]) if len(pre) > 10 else (0, 0, 0, 0)
    period_us = 1e9 / nom_mhz if nom_mhz else float(nom_us)
    scale, unit = ad2_scale(mode, rng) if ad2 else (None, '')
    # The meter's own scale (LOG:SCALe?, opendm40 E7): the stock calibration or,
    # in DC V, the entry self-cal; older firmware: an error, nominal scale.
    off, cal = 0.0, False
    if ad2:
        try:
            a, b = [float(x) for x in (link.query('LOG:SCAL?') or '').split(',')]
            if b != 0:
                off, scale, cal = a, b, True
        except ValueError:
            pass
        link.query('*CLS')
    rows, last_seq, last_ts, wrap = 0, None, None, 0
    # The session ends at -d seconds after LOG:STAT ON (host clock) or after
    # -n rows, checked after every LOG:DATA? block (a drain that only ended
    # on a short block ran on for ever above the host's drain rate), or on
    # Ctrl-C: the first one lets the block in flight finish (dm40dbg.Link
    # holds SIGINT during a read: a cut read leaves the rest of the reply on
    # the meter and every later reply one query late), then LOG:STAT OFF,
    # the rest drained and the trailer written; a second one abandons the
    # drain (LOG:STAT OFF still sent, no trailer).
    t_end = t_start + args.duration if args.duration else None
    stop = []

    def on_sigint(*_):
        if stop and stop[0] == 'ctrl-c':
            raise KeyboardInterrupt
        stop[:] = ['ctrl-c']
        print('\ndm40scpi: stopping (Ctrl-C again: abandon the drain)', file=sys.stderr, flush=True)

    def done():
        if not stop:
            if args.n and rows >= args.n:
                stop.append('count')
            elif t_end is not None and time.monotonic() >= t_end:
                stop.append('duration')
        return bool(stop)

    with open(args.out, 'w', newline='') as f:
        f.write('# source,%s\n# mode,%d\n# range,%d\n# chop,%d\n' % (args.source.upper(), mode, rng, chop))
        if ad2:
            f.write('# requested_sps,%d\n# nominal_sps,%.3f\n# r26,0x%02X\n# coupling,%s\n'
                    % (req, nom_mhz / 1000.0, r26, 'AC' if coupling else 'DC'))
            if cal:
                f.write('# scale,%.9g %s/code, offset %.9g %s (the meter\'s, LOG:SCALe?: value = offset + scale * raw)\n'
                        % (scale, unit, off, unit))
            else:
                f.write('# scale,%.9g %s/code (IG 1.0, VR 1.25 V, stock range factor; calibration NOT applied)\n' % (scale or 0, unit))
        else:
            f.write('# osr,%d\n# nominal_period_us,%d\n# scale,raw AD1 counts (1.389 V / 8388607 * range decade, uncalibrated)\n' % (osr, nom_us))
        if args.format != 'rows':
            f.write('# format,%s (t_s: %s)\n' % (args.format, 'the time grid t0 + i / rate of each gap-free block'
                                                  if args.format == 'packed' else 'the device stamps'))
        f.write('# start,%s\n' % iso_now())
        f.write('seq,t_s,raw,%s,lost_before\n' % ('value_' + unit if ad2 else 'value'))

        def drain(final=False):
            """LOG:DATA? blocks until a short one (the ring empty), or, unless
            `final`, until the session is done."""
            nonlocal rows, last_seq, last_ts, wrap
            got = 0
            while True:
                recs, info = log_reply(link.query_raw('LOG:DATA?'), args.format)
                # ROWS: 511 records fill a reply; the compact ones say how many are left
                short = len(recs) < 500 if info['format'] == 'rows' else info['left'] == 0
                if args.n:
                    recs = recs[:max(0, args.n - rows)]
                for seq, ts, code, dev_lost in recs:
                    if last_ts is not None and ts < last_ts:
                        wrap += 1 << 32
                    t = (ts + wrap) / 1e6
                    lost = 0
                    if dev_lost is not None:
                        # the meter's count (PACKed: the grid's stamps; DLG: from the exact stamps)
                        lost = dev_lost if last_seq is not None else 0
                    elif last_seq is not None:
                        # conversions between two kept rows: from the time (stalls) or the seq (ring overflow)
                        lost = max(round((ts + wrap - last_t_us[0]) / period_us) - 1, seq - last_seq - 1, 0)
                    if last_seq is not None and seq != last_seq + 1:
                        lost = max(lost, seq - last_seq - 1)
                    last_t_us[0] = ts + wrap
                    f.write('%d,%.6f,%d,%s,%d\n' % (seq, t, code, '%.7g' % (off + code * scale) if scale else '', lost))
                    last_seq, last_ts = seq, ts
                    rows += 1
                got += len(recs)
                if short or (args.n and rows >= args.n) or (not final and done()):
                    return got

        last_t_us = [0]
        old_sigint = signal.signal(signal.SIGINT, on_sigint)
        try:
            while not done():
                if drain() == 0:
                    time.sleep(0.005)
                if not args.quiet:
                    print('\r%d samples   ' % rows, end='', flush=True)
            link.query('LOG:STAT OFF')
            if not (args.n and rows >= args.n):
                drain(final=True)
        finally:
            signal.signal(signal.SIGINT, old_sigint)
        cnt = [int(x) for x in link.query('LOG:COUN?').split(',')]
        gaps = (link.query('LOG:GAP?') or '').strip()
        hz = int(link.query('LOG:RATE?') or 0)
        f.write('# end,%s\n# stopped_by,%s (after %.3f s)\n# measured_sps,%d\n'
                '# count,buffered %d captured %d overwritten %d missed %d lost %d\n'
                % (iso_now(), stop[0], time.monotonic() - t_start, hz, *(cnt + [0] * 5)[:5]))
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


def do_fast(link, args):
    """A LOG:DEST DISK session (fastlog.rs): prepare, run --dur seconds
    (device time), copy LOGS/FASTnnn.CSV."""
    if args.conf:
        switch(link, args.conf)
    ad2 = args.source == 'ad2'
    link.query('LOG:STAT OFF')
    # An error another client left in the queue ("Data stale" of a READ?)
    # would fail the check below (as in do_stream): clear it first.
    link.query('*CLS')
    link.query('LOG:SOUR %s' % ('AD2' if ad2 else 'AD1'))
    if args.chop:
        link.query('LOG:CHOP %s' % args.chop.upper())
    if ad2:
        link.query('LOG:SRAT %d' % args.rate)
    elif args.osr is not None:
        link.query('LOG:OSR %d' % args.osr)
    link.query('LOG:DEST DISK')
    link.query('LOG:DUR %d' % args.dur)
    check_err(link, 'LOG settings')
    t0 = time.monotonic()
    link.query('LOG:STAT ON')
    check_err(link, 'LOG:STAT ON (AD2: V / mV / AC A; AD1: DC V / mV / A; no CSV recording / disk job running)')
    names = ['idle', 'preparing', 'ready', 'running', 'stopping', 'done', 'full', 'no disk', 'error']
    st = []
    try:
        while True:
            st = [int(x) for x in link.query('LOG:DISK?').split(',')]
            if not args.quiet:
                print('\r%-9s %3d %%  %d samples  %d bytes   ' % (names[st[0]], st[5], st[4], st[3]), end='', flush=True)
            if st[0] >= 5:
                break
            time.sleep(0.5)
    except KeyboardInterrupt:
        link.query('LOG:STAT OFF')
        # (Ctrl-C before the first LOG:DISK? answer: st was still empty)
        st = st or [int(x) for x in link.query('LOG:DISK?').split(',')]
        while st[0] < 5:
            time.sleep(0.5)
            st = [int(x) for x in link.query('LOG:DISK?').split(',')]
    if not args.quiet:
        print()
    link.query('LOG:DEST HOST')
    cnt = (link.query('LOG:COUN?') or '').strip()
    if st[0] not in (5, 6) or st[1] < 0:
        raise IOError('session ended: %s (LOG:DISK? %s)' % (names[st[0]], st))
    name = 'LOGS/FAST%03d.CSV' % st[1]
    d = get_file(link, name, args.out)
    rows = [l for l in d.decode('ascii', 'replace').splitlines() if l[:1] in ' 0123456789']
    lost = sum(int(l.rsplit(',', 1)[1]) for l in rows)
    print('%s (%s): %d samples, %d lost (lost_before), %d bytes on the disk, %.0f s; LOG:COUNt? %s'
          % (args.out, name, len(rows), lost, st[3], time.monotonic() - t0, cnt))
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
    p = argparse.ArgumentParser(description='DM40 opendm40 SCPI client (USBTMC or Bluetooth).',
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
    s.add_argument('--rate', type=int, default=2400, help='(ad2) requested S/s: 1200, 2400, 4800, 9600, 19200, 38400 (nearest rung; 19200 / 38400 with the ad2-isr stream ISR)')
    s.add_argument('--chop', choices=['on', 'off'], help='(stream) converter chopper (default on; ad2: same rungs either way)')
    s.add_argument('--osr', type=int, help='(ad1) AD1OSR code 0..7')
    s.add_argument('--format', choices=sorted(LOG_FORMATS), default='rows',
                   help='(stream) LOG:FORMat of LOG:DATA?: rows (12 B per sample, default), packed (3 B per sample on '
                   'the time grid, a new block at every gap) or dlg (the disk fast stream\'s coding, ~1-3 B per sample, '
                   'exact stamps); the CSV is the same')
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
    s = sub.add_parser('fast', help='the fast logger to the meter\'s disk (LOG:DEST DISK), then its CSV')
    s.add_argument('out')
    s.add_argument('--source', choices=['ad1', 'ad2'], default='ad2')
    s.add_argument('--rate', type=int, default=4800, help='(ad2) requested S/s')
    s.add_argument('--chop', choices=['on', 'off'])
    s.add_argument('--osr', type=int, help='(ad1) AD1OSR code 0..7')
    s.add_argument('--dur', type=int, default=60, help='seconds of device time (0 = until the disk is full / Ctrl-C)')
    s.add_argument('--conf', help='switch to this function first')
    s.add_argument('-q', '--quiet', action='store_true')
    s.set_defaults(func_=do_fast)
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
