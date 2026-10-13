#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""DM40 memory read / backup tool (BT command 0x0B of the patched firmware).

This is a BACKUP tool for owners of an Alientek DM40 multimeter running the
patched firmware shipped in this kit (firmware/dm40_v_2_6_memrw.bin). It talks
to the meter over Bluetooth RFCOMM and reads out the internal flash so you can
save a copy of your calibration before experimenting.

READ-ONLY by default. Writing or calling code on the meter can destroy your
calibration or brick the device and is only available behind the explicit
--unsafe flag.

Requirements:
  * Python 3 with Bluetooth socket support (socket.AF_BLUETOOTH). On Linux use
    the system interpreter: /usr/bin/python3. No third-party packages.
  * The meter running the patched firmware, on its MULTIMETER (DMM) screen.
  * The meter's Bluetooth address. On Linux it is found automatically via
    `bluetoothctl` (BlueZ) and cached in ~/.config/dm40-memrw/mac; elsewhere
    (or to override) pass --mac or set the DM40_MAC env var.

Usage examples:
  python3 -I dm40_memrw.py read 0x1FFF7A10 12
  python3 -I dm40_memrw.py read 0x080C0000 0x100 -o cal_head.bin
  python3 -I dm40_memrw.py dump --quick -o backup_quick
  python3 -I dm40_memrw.py dump --full  -o backup_full
  python3 -I dm40_memrw.py --rescan dump --quick -o backup_quick  # ignore cache
  python3 -I dm40_memrw.py --mac XX:XX:XX:XX:XX:XX read 0x1FFF7A10 12
"""
import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import socket
import struct
import subprocess
import sys
import time

# ---------------------------------------------------------------------------
# Wire protocol (identical to the original client: AF 05 03 0B ...)
# ---------------------------------------------------------------------------
ID = bytes([0x05, 0x03])
CMD = 0x0B
RESP_CMD = 0x0C
MAXR = 0xFF          # max bytes read per request
MAXW = 0x91          # max payload bytes written per request

# --- model-switch (set-model) constants, all V2.6-specific -----------------
# The app expects vectors SP=0x2002F7F8, Reset=0x080041C9 at 0x08004000; this
# identifies the V2.6 app layout (the same for stock and patched V2.6 — the
# only firmware whose BT memory command answers at all is the patched one).
APP_VECTORS_ADDR = 0x08004000
APP_VECTORS_V26 = bytes.fromhex('f8f70220c9410008')
# Settings/calibration record: in flash at 0x080C0000, its live RAM copy
# (returned by FUN_08022a62) at 0x20028E58. The DMM model byte is at record
# offset 0x168; byte 0 of that = model (0=DM40A, 1=DM40B, 2=DM40C).
SETTINGS_FLASH = 0x080C0000
SETTINGS_RAM = 0x20028E58
MODEL_OFFSET = 0x168
MODEL_FLASH = SETTINGS_FLASH + MODEL_OFFSET    # 0x080C0168
MODEL_RAM = SETTINGS_RAM + MODEL_OFFSET         # 0x20028FC0
# The DMM page rebuilds a secondary copy at 0x20026B0C from the record on every
# entry to the multimeter page; we write it too so an already-open page is
# consistent, but the authoritative byte is MODEL_RAM.
DMM_PAGE_MODEL_RAM = 0x20026B0C
MODEL_CODES = {'A': 0, 'B': 1, 'C': 2}
MODEL_NAMES = {0: 'A', 1: 'B', 2: 'C'}


def crc(d):
    """Append the vendor 1-byte checksum (two's complement of the byte sum)."""
    return d + struct.pack('B', (0x100 - sum(d)) & 0xFF)


def pkt(sub, addr, n=0, payload=b''):
    """Build a request packet, byte-for-byte as the original client does."""
    data = struct.pack('<IBB', addr, sub, n) + payload
    return crc(bytes([0xAF]) + ID + bytes([CMD, len(data)]) + data)


# --- stock firmware BT protocol (commands 00-09, same framing) -------------
STOCK_REPLY_HDR = 0xDF
STOCK_HOLD, STOCK_REL, STOCK_MODE, STOCK_MEAS = 0x04, 0x05, 0x06, 0x09


def stock_frame(cmd, data=b''):
    return crc(bytes([0xAF]) + ID + bytes([cmd, len(data)]) + bytes(data))


# Named measurement modes -> mode byte (bits0-2 group, 3-5 range, 6-7 submode;
# submode 0 DC, 1 AC, 2 AC+DC). Voltage/current modes are auto-ranging.
MODE_BYTES = {
    'dcv': 0x28, 'acv': 0x68, 'acdcv': 0xA8, 'dcmv': 0x00, 'acmv': 0x40,
    'dca': 0x31, 'aca': 0x71, 'acdca': 0xB1, 'dcma': 0x39, 'acma': 0x79,
    'ohm': 0x32, 'cap': 0x03, 'diode': 0x04, 'cont': 0x44,
    'freq': 0x05, 'temp': 0x45,
}

# Base unit per mode group / special mode (value is in this unit before prefix).
_GROUP_BASE = {0: 'mV', 1: 'uA', 2: 'ohm'}
_SPECIAL_BASE = {0x03: 'nF', 0x04: 'V', 0x44: 'ohm', 0x05: 'Hz', 0x45: 'C'}
_PREFIX = {0: '', 1: 'k', 2: 'M'}      # for display scaling of the base unit


def _base_unit(mode):
    if mode in _SPECIAL_BASE:
        return _SPECIAL_BASE[mode]
    return _GROUP_BASE.get(mode & 7, '?')


def decode_value(mode, fmt, raw):
    """Decode one (fmt, raw u16) field. Returns (number_in_base_units, unit)
    or (None, unit) for over-range / no value (raw == 0xFFFF)."""
    base = _base_unit(mode)
    if raw == 0xFFFF:
        return None, base
    neg = fmt & 1
    dec = (fmt >> 1) & 7
    pfx = (fmt >> 4) & 3
    # value in base units = raw * 10^(3*prefix - dec)
    val = raw * (10 ** (3 * pfx - dec))
    if neg:
        val = -val
    return val, base


# ---------------------------------------------------------------------------
# Device connection
# ---------------------------------------------------------------------------
class DeviceError(IOError):
    pass


class Dev:
    """RFCOMM connection to the meter (channel 1), speaking BT command 0x0B."""

    def __init__(self, mac, timeout=5.0):
        if not hasattr(socket, 'AF_BLUETOOTH'):
            sys.exit('This Python build has no Bluetooth socket support '
                     '(socket.AF_BLUETOOTH). On Linux run /usr/bin/python3.')
        self.mac = mac
        self.timeout = timeout
        self.s = None
        self.connect()

    def connect(self):
        self.close()
        s = socket.socket(socket.AF_BLUETOOTH, socket.SOCK_STREAM,
                          socket.BTPROTO_RFCOMM)
        s.settimeout(self.timeout)
        s.connect((self.mac, 1))
        self.s = s

    def close(self):
        if self.s is not None:
            try:
                self.s.close()
            except Exception:
                pass
            self.s = None

    # -- low level ---------------------------------------------------------
    def _resp(self, n):
        want = 5 + n + 1                # AF 05 03 0C n <data> CRC
        buf = b''
        while len(buf) < want:
            r = self.s.recv(want - len(buf))
            if not r:
                raise DeviceError('connection closed by device')
            buf += r
        if (buf[0] != 0xAF or buf[1] != ID[0] or buf[2] != ID[1]
                or buf[3] != RESP_CMD or buf[4] != n):
            raise DeviceError('bad response: %s' % buf.hex(' '))
        return buf

    def _read_once(self, addr, n):
        self.s.send(pkt(0, addr, n))
        return self._resp(n)[5:5 + n]

    # -- public ------------------------------------------------------------
    # -- stock protocol (unmodified firmware commands) ---------------------
    def stock(self, cmd, data=b''):
        """Send a stock request and return the reply payload (no --unsafe).

        Request AF 05 03 cmd len data crc; reply DF 05 03 cmd len data crc."""
        self.s.send(stock_frame(cmd, data))
        hdr = b''
        while len(hdr) < 5:
            r = self.s.recv(5 - len(hdr))
            if not r:
                raise DeviceError('connection closed')
            hdr += r
        if hdr[0] != STOCK_REPLY_HDR or hdr[3] != cmd:
            raise DeviceError('bad stock reply: %s' % hdr.hex(' '))
        n = hdr[4]
        body = b''
        while len(body) < n + 1:
            r = self.s.recv(n + 1 - len(body))
            if not r:
                raise DeviceError('connection closed')
            body += r
        return body[:n]

    def read(self, addr, n):
        """Read n bytes, chunked at MAXR per request. Raises on any error."""
        out = b''
        while n:
            k = min(n, MAXR)
            out += self._read_once(addr, k)
            addr += k
            n -= k
        return out

    def write(self, addr, data):
        """UNSAFE: write bytes, chunked at MAXW per request."""
        while data:
            k = min(len(data), MAXW)
            self.s.send(pkt(1, addr, k, data[:k]))
            self._resp(0)
            addr += k
            data = data[k:]

    def call(self, addr, r0=0, r1=0, r2=0, r3=0):
        """UNSAFE and KNOWN BROKEN: execute code at addr with r0..r3.

        The shipped firmware's call path hangs the meter until the watchdog
        resets it. Provided only for experiments; do not use on a device you
        care about."""
        self.s.send(pkt(2, addr, 0, struct.pack('<4I', r0, r1, r2, r3)))
        return struct.unpack('<I', self._resp(4)[5:9])[0]

    def version(self):
        """Best effort firmware identification using memory reads only.

        The memrw command answering at all proves the patched firmware; the
        app vector words at 0x08004000 identify the V2.6 app layout. Returns
        a description string, or None if not recognised. Never raises."""
        try:
            v = self.read(APP_VECTORS_ADDR, 8)
        except Exception:
            return None
        if v == APP_VECTORS_V26:
            return 'V2.6 + memrw patch (app vectors match V2.6)'
        return None


# ---------------------------------------------------------------------------
# Robust backup session: reconnect / retry / double-read verification
# ---------------------------------------------------------------------------
class Backup:
    def __init__(self, mac, timeout=5.0, reconnect_wait=4.0,
                 connect_tries=20, read_tries=8, verify_tries=5):
        self.mac = mac
        self.timeout = timeout
        self.reconnect_wait = reconnect_wait
        self.connect_tries = connect_tries
        self.read_tries = read_tries
        self.verify_tries = verify_tries
        self.dev = None
        self._connect()

    def _connect(self):
        last = None
        for i in range(self.connect_tries):
            try:
                self.dev = Dev(self.mac, self.timeout)
                return
            except Exception as e:                      # noqa: BLE001
                last = e
                sys.stderr.write('connect retry %d: %s\n' % (i, e))
                sys.stderr.flush()
                time.sleep(self.reconnect_wait)
        sys.exit('cannot connect to %s: %s' % (self.mac, last))

    def _reconnect(self):
        if self.dev:
            self.dev.close()
        time.sleep(self.reconnect_wait)
        self._connect()

    def read(self, addr, n):
        """Read with retry + reconnect on error."""
        last = None
        for i in range(self.read_tries):
            try:
                return self.dev.read(addr, n)
            except Exception as e:                      # noqa: BLE001
                last = e
                sys.stderr.write('read error @0x%08x: %s, reconnecting\n'
                                 % (addr, e))
                sys.stderr.flush()
                self._reconnect()
        sys.exit('read failed @0x%08x after %d tries: %s'
                 % (addr, self.read_tries, last))

    def read_verified(self, addr, n):
        """Read twice and compare; retry the block on mismatch."""
        for _ in range(self.verify_tries):
            x = self.read(addr, n)
            y = self.read(addr, n)
            if x == y:
                return x
            sys.stderr.write('double-read mismatch @0x%08x, retrying\n' % addr)
            sys.stderr.flush()
        sys.exit('unstable read @0x%08x (double-read never matched)' % addr)

    def dump_region(self, out_path, base, size, blk=4096, label=''):
        out = bytearray()
        t0 = time.time()
        a = base
        while a < base + size:
            n = min(blk, base + size - a)
            out += self.read_verified(a, n)
            done = a - base + n
            if done % 0x10000 == 0 or done == size:
                pct = 100.0 * done / size
                sys.stdout.write('\r  %-28s 0x%08x  %5.1f%%  %4.0fs'
                                 % (label, a, pct, time.time() - t0))
                sys.stdout.flush()
            a += n
        sys.stdout.write('\n')
        sys.stdout.flush()
        with open(out_path, 'wb') as f:
            f.write(out)
        return bytes(out)

    def close(self):
        if self.dev:
            self.dev.close()


# ---------------------------------------------------------------------------
# Region tables
# ---------------------------------------------------------------------------
# (name, addr, size, block, description)
QUICK_REGIONS = [
    ('uid_1FFF7A10.bin',           0x1FFF7A10, 0x0C,    16,   '96-bit chip UID'),
    ('option_bytes_1FFFC000.bin',  0x1FFFC000, 0x10,    16,   'flash option bytes'),
    ('otp_1FFF7800.bin',           0x1FFF7800, 0x210,   528,  'OTP area'),
    ('bootdata_080A4000_16K.bin',  0x080A4000, 0x4000,  4096, 'UID-bound boot data'),
    ('calibration_080C0000_32K.bin', 0x080C0000, 0x8000, 4096,
     'settings + DMM/scope CALIBRATION record'),
]

# Full = small internal regions + the whole 1 MiB internal flash
FULL_SMALL = [
    ('uid_1FFF7A10.bin',          0x1FFF7A10, 0x0C,  16,  '96-bit chip UID'),
    ('option_bytes_1FFFC000.bin', 0x1FFFC000, 0x10,  16,  'flash option bytes'),
    ('otp_1FFF7800.bin',          0x1FFF7800, 0x210, 528, 'OTP area'),
]
FULL_FLASH = ('flash_08000000_1MB.bin', 0x08000000, 0x100000, 4096,
              'whole 1 MiB internal flash (bootloader + app + loader + '
              'boot data + calibration)')


# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------
def do_read(args, mac):
    dev = Dev(mac, args.timeout)
    try:
        data = dev.read(args.addr, args.length)
    finally:
        dev.close()
    if args.out:
        with open(args.out, 'wb') as f:
            f.write(data)
        print('wrote %d bytes to %s (sha256 %s)'
              % (len(data), args.out, hashlib.sha256(data).hexdigest()))
    else:
        print(data.hex(' '))


def do_dump(args, mac):
    if args.full == args.quick:   # both or neither
        sys.exit('choose exactly one of --quick or --full')
    out_dir = args.out
    os.makedirs(out_dir, exist_ok=True)
    kind = 'full' if args.full else 'quick'

    b = Backup(mac, timeout=args.timeout)
    try:
        fw_version = b.dev.version()
        if fw_version:
            print('firmware version reported: %s' % fw_version)
        else:
            print('firmware version: not readable')

        entries = []

        def record(name, addr, size, data):
            h = hashlib.sha256(data).hexdigest()
            entries.append({
                'file': name,
                'address': '0x%08X' % addr,
                'size': len(data),
                'sha256': h,
            })
            print('  saved %-30s 0x%08X  %7d B  %s' % (name, addr, len(data), h))

        if args.quick:
            for name, addr, size, blk, desc in QUICK_REGIONS:
                data = b.dump_region(os.path.join(out_dir, name), addr, size,
                                     blk, desc)
                record(name, addr, size, data)
        else:
            for name, addr, size, blk, desc in FULL_SMALL:
                data = b.dump_region(os.path.join(out_dir, name), addr, size,
                                     blk, desc)
                record(name, addr, size, data)
            name, addr, size, blk, desc = FULL_FLASH
            data = b.dump_region(os.path.join(out_dir, name), addr, size,
                                 blk, desc)
            record(name, addr, size, data)
    finally:
        b.close()

    manifest = {
        'tool': 'dm40_memrw.py',
        'kind': kind,
        'date_utc': datetime.datetime.now(datetime.timezone.utc)
                     .strftime('%Y-%m-%dT%H:%M:%SZ'),
        'device_mac': mac,
        'firmware_version': fw_version,
        'files': entries,
    }
    mpath = os.path.join(out_dir, 'manifest.json')
    with open(mpath, 'w') as f:
        json.dump(manifest, f, indent=2)
        f.write('\n')

    # also a human-readable SHA256SUMS and a text manifest
    with open(os.path.join(out_dir, 'SHA256SUMS'), 'w') as f:
        for e in entries:
            f.write('%s  %s\n' % (e['sha256'], e['file']))
    with open(os.path.join(out_dir, 'manifest.txt'), 'w') as f:
        f.write('DM40 %s backup\n' % kind)
        f.write('date (UTC): %s\n' % manifest['date_utc'])
        f.write('device MAC: %s\n' % mac)
        f.write('firmware:   %s\n' % (fw_version or 'unknown'))
        f.write('\n%-30s %-12s %8s  %s\n'
                % ('file', 'address', 'size', 'sha256'))
        for e in entries:
            f.write('%-30s %-12s %8d  %s\n'
                    % (e['file'], e['address'], e['size'], e['sha256']))

    print('\nbackup complete: %s' % out_dir)
    print('manifest: %s' % mpath)


UNSAFE_BANNER = """
*********************************  WARNING  *********************************
  You are about to %s the meter's memory over Bluetooth.
  WRITE can DESTROY your calibration or BRICK the meter.
  CALL can HANG the meter (the shipped firmware's call path is known to hang
  the unit until the watchdog resets it).
  Make a FULL backup first (dump --full) and be sure you know the address.
****************************************************************************
"""


def _require_unsafe(args, what):
    if not args.unsafe:
        sys.exit('refusing to %s without --unsafe (this is a backup tool; '
                 'reads are safe, writes/calls are not)' % what)
    sys.stderr.write(UNSAFE_BANNER % what.upper())
    sys.stderr.flush()
    if not args.yes:
        ans = input('Type "yes" to proceed: ').strip().lower()
        if ans != 'yes':
            sys.exit('aborted')


def do_write(args, mac):
    _require_unsafe(args, 'write')
    data = bytes.fromhex(args.hexbytes)
    dev = Dev(mac, args.timeout)
    try:
        dev.write(args.addr, data)
    finally:
        dev.close()
    print('wrote %d bytes to 0x%08X' % (len(data), args.addr))


def do_call(args, mac):
    _require_unsafe(args, 'call')
    regs = [int(x, 0) for x in (args.regs or [])][:4]
    regs += [0] * (4 - len(regs))
    dev = Dev(mac, args.timeout)
    try:
        r0 = dev.call(args.addr, *regs)
    finally:
        dev.close()
    print('r0 = 0x%08X' % r0)


# ---------------------------------------------------------------------------
# set-model: temporarily override the meter's model type in RAM (no flash write)
# ---------------------------------------------------------------------------
class ModelCheckError(Exception):
    pass


def check_v26_image(dev):
    """Confirm the V2.6 app layout (the set-model addresses are V2.6-only).

    Raises ModelCheckError on mismatch."""
    v = dev.read(APP_VECTORS_ADDR, 8)
    if v != APP_VECTORS_V26:
        raise ModelCheckError(
            'app vectors at 0x%08X = %s, expected V2.6 %s -- this is not the '
            'expected patched V2.6 image; refusing'
            % (APP_VECTORS_ADDR, v.hex(' '), APP_VECTORS_V26.hex(' ')))


def check_settings_mirror(dev):
    """Confirm the RAM settings copy really mirrors flash 0x080C0000, so the
    model byte is where we think. Returns the flash model byte.

    Raises ModelCheckError on mismatch."""
    ram16 = dev.read(SETTINGS_RAM, 16)
    fl16 = dev.read(SETTINGS_FLASH, 16)
    if ram16 != fl16:
        raise ModelCheckError(
            'settings RAM copy 0x%08X (%s) != flash 0x%08X (%s); the model-byte '
            'location cannot be trusted; refusing'
            % (SETTINGS_RAM, ram16.hex(' '), SETTINGS_FLASH, fl16.hex(' ')))
    ram8 = dev.read(MODEL_RAM, 8)
    fl8 = dev.read(MODEL_FLASH, 8)
    if ram8 != fl8:
        raise ModelCheckError(
            'model block RAM 0x%08X (%s) != flash 0x%08X (%s); refusing'
            % (MODEL_RAM, ram8.hex(' '), MODEL_FLASH, fl8.hex(' ')))
    return fl8[0]


def do_set_model(args, mac):
    target = args.model.upper()
    dev = Dev(mac, args.timeout)
    try:
        try:
            check_v26_image(dev)
            flash_model = check_settings_mirror(dev)
        except ModelCheckError as e:
            sys.exit('set-model: %s' % e)

        if target == 'RESTORE':
            val = flash_model
            label = 'flash default (%s)' % MODEL_NAMES.get(val, '?')
        else:
            val = MODEL_CODES[target]
            label = 'DM40%s' % target

        # authoritative byte + the DMM page's secondary copy, then read back
        dev.write(MODEL_RAM, bytes([val]))
        dev.write(DMM_PAGE_MODEL_RAM, bytes([val]))
        rb = dev.read(MODEL_RAM, 1)[0]
        if rb != val:
            sys.exit('readback failed: wrote 0x%02X at 0x%08X, read 0x%02X'
                     % (val, MODEL_RAM, rb))
    finally:
        dev.close()
    print('model set to %s (was flash default DM40%s)'
          % (label, MODEL_NAMES.get(flash_model, '?')))
    print('Switch to another page (scope/menu) and back to the multimeter for '
          'the change to take effect. It is RAM-only and reverts on reboot.')


# ---------------------------------------------------------------------------
# Stock-protocol commands (safe; only work on the multimeter page)
# ---------------------------------------------------------------------------
def do_mode(args, mac):
    code = MODE_BYTES[args.mode.lower()]
    dev = Dev(mac, args.timeout)
    try:
        dev.stock(STOCK_MODE, bytes([code]))
    finally:
        dev.close()
    print('mode set: %s (0x%02X)' % (args.mode.lower(), code))


def do_hold(args, mac):
    on = args.state.lower() in ('on', '1', 'true')
    dev = Dev(mac, args.timeout)
    try:
        dev.stock(STOCK_HOLD, bytes([1 if on else 0]))
    finally:
        dev.close()
    print('HOLD %s' % ('on' if on else 'off'))


def do_rel(args, mac):
    on = args.state.lower() in ('on', '1', 'true')
    dev = Dev(mac, args.timeout)
    try:
        dev.stock(STOCK_REL, bytes([1 if on else 0]))
    finally:
        dev.close()
    print('REL %s' % ('on' if on else 'off'))


def _flags_str(f):
    parts = []
    if f & 0x80:
        parts.append('HOLD')
    if f & 0x20:
        parts.append('REL')
    parts.append('batt=%d' % (f & 0x07))
    return ' '.join(parts)


def _fmt_field(mode, val, unit):
    if val is None:
        return 'OL/--'
    return '%g %s' % (val, unit)


def do_measure(args, mac):
    dev = Dev(mac, args.timeout)
    try:
        p = dev.stock(STOCK_MEAS)
    finally:
        dev.close()
    if len(p) < 11:
        sys.exit('short measurement reply: %s' % p.hex(' '))
    mode, flags, fmt2, fmt3, fmtm = p[0], p[1], p[2], p[3], p[4]
    v2 = p[5] | (p[6] << 8)
    v3 = p[7] | (p[8] << 8)
    vm = p[9] | (p[10] << 8)
    main = decode_value(mode, fmtm, vm)
    sec2 = decode_value(mode, fmt2, v2)
    sec3 = decode_value(mode, fmt3, v3)
    out = {
        'mode_byte': '0x%02X' % mode,
        'flags': '0x%02X (%s)' % (flags, _flags_str(flags)),
        'main': _fmt_field(mode, *main),
        'value2': _fmt_field(mode, *sec2),
        'value3': _fmt_field(mode, *sec3),
        'raw': p.hex(' '),
    }
    if args.json:
        print(json.dumps({
            'mode_byte': mode, 'flags': flags,
            'main': main[0], 'main_unit': main[1],
            'value2': sec2[0], 'value2_unit': sec2[1],
            'value3': sec3[0], 'value3_unit': sec3[1],
            'raw': p.hex(' '),
        }))
    else:
        print('main   : %s' % out['main'])
        print('value2 : %s' % out['value2'])
        print('value3 : %s' % out['value3'])
        print('flags  : %s' % out['flags'])
        print('mode   : %s' % out['mode_byte'])


# ---------------------------------------------------------------------------
# Bluetooth address auto-discovery (Linux / BlueZ `bluetoothctl`, stdlib only)
# ---------------------------------------------------------------------------
# The meter shows up twice in BlueZ: a classic BR/EDR device (name like
# "C-1-ATK-DM40-CALI", Serial Port UUID, RFCOMM channel 1 -- the one we need)
# and a BLE device for the phone app (name "DM40A/B/C", address with the first
# byte one lower; not usable for RFCOMM).
SPP_UUID = '00001101-0000-1000-8000-00805f9b34fb'
SCAN_SECONDS = 10
BTCTL_TIMEOUT = 8.0          # seconds for `devices` / `info`
_BT_MAC_RE = r'[0-9A-Fa-f]{2}(?::[0-9A-Fa-f]{2}){5}'
_BT_DEVICE_LINE = re.compile(r'Device\s+(' + _BT_MAC_RE + r')(?:[ \t]+(.*))?$')
_BT_ANSI = re.compile(r'\x1b\[[0-9;?]*[A-Za-z]|\x1b[()][0-9A-Za-z]|[\x01\x02]')
_BT_TAGS = re.compile(r'^\s*(?:\[[^\]]*\][#>]?\s*)+')   # [NEW] [CHG] [bluetooth]#


class DiscoveryError(Exception):
    pass


def clean_lines(text):
    """bluetoothctl output -> plain lines without colours/prompts/[CHG]."""
    out = []
    for line in _BT_ANSI.sub('', text or '').replace('\r', '\n').split('\n'):
        line = _BT_TAGS.sub('', line).strip()
        if line:
            out.append(line)
    return out


def parse_devices(text):
    """`bluetoothctl devices` (or scan output) -> [(MAC, name)], in order.
    Unnamed entries get name ''; "[DEL]" lines are ignored."""
    seen = {}
    for raw in _BT_ANSI.sub('', text or '').replace('\r', '\n').split('\n'):
        if '[DEL]' in raw:
            continue
        line = _BT_TAGS.sub('', raw).strip()
        mo = _BT_DEVICE_LINE.search(line)
        if not mo:
            continue
        mac = mo.group(1).upper()
        name = (mo.group(2) or '').strip()
        prop = re.match(r'^([A-Za-z][\w.]*):(?:\s|$)', name)
        if prop:                         # scan "[CHG] Device X RSSI: -60" etc.
            key = prop.group(1).lower()
            name = name[prop.end():].strip() if key in ('name', 'alias') else ''
        elif name.startswith('('):       # "Device X (public)" header
            name = ''
        if name == mac.replace(':', '-'):
            name = ''                    # BlueZ placeholder for "no name"
        if name or mac not in seen:
            seen[mac] = name or seen.get(mac, '')
    return list(seen.items())


def parse_info(text):
    """`bluetoothctl info <mac>` -> dict, or None if the device is unknown."""
    lines = clean_lines(text)
    if not lines or any('not available' in l for l in lines):
        return None
    info = {'name': '', 'alias': '', 'class': None, 'paired': False,
            'uuids': [], 'spp': False}
    for line in lines:
        key, sep, val = line.partition(':')
        if not sep:
            continue
        key, val = key.strip().lower(), val.strip()
        if key == 'name':
            info['name'] = val
        elif key == 'alias':
            info['alias'] = val
        elif key == 'class':
            info['class'] = val
        elif key == 'paired':
            info['paired'] = val.lower() == 'yes'
        elif key == 'uuid':
            info['uuids'].append(val)
    info['spp'] = SPP_UUID in ' '.join(info['uuids']).lower()
    if not (info['name'] or info['alias'] or info['uuids']
            or info['class'] or any(l.startswith('Device ') for l in lines)):
        return None
    return info


def is_dm40_name(name):
    return 'dm40' in (name or '').lower()


def derived_classic_mac(mac):
    """BLE "DM40x" address -> likely classic address (first byte + 1)."""
    parts = mac.upper().split(':')
    parts[0] = '%02X' % ((int(parts[0], 16) + 1) & 0xFF)
    return ':'.join(parts)


def config_path():
    base = os.environ.get('XDG_CONFIG_HOME') or os.path.join(
        os.path.expanduser('~'), '.config')
    return os.path.join(base, 'dm40-memrw', 'mac')


def _default_runner(args, timeout):
    """Run bluetoothctl; return (returncode, output text). rc None = timeout."""
    env = dict(os.environ, LC_ALL='C', LANG='C', NO_COLOR='1')
    try:
        r = subprocess.run(['bluetoothctl'] + list(args),
                           stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT, timeout=timeout, env=env)
    except subprocess.TimeoutExpired as e:
        out = e.output or b''
        return None, out.decode('utf-8', 'replace') if isinstance(
            out, bytes) else out
    except OSError as e:
        return 127, str(e)
    return r.returncode, r.stdout.decode('utf-8', 'replace')


def _default_scan_popen(seconds, sleep):
    """Fallback scan for bluetoothctl without --timeout: run `scan on` for a
    while, then stop it. Returns its output text."""
    env = dict(os.environ, LC_ALL='C', LANG='C', NO_COLOR='1')
    try:
        p = subprocess.Popen(['bluetoothctl', 'scan', 'on'],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, env=env)
    except OSError as e:
        return str(e)
    sleep(seconds)
    try:
        p.stdin.write(b'scan off\nquit\n')
        p.stdin.flush()
    except (OSError, ValueError):
        pass
    p.terminate()
    try:
        out, _ = p.communicate(timeout=5)
    except subprocess.TimeoutExpired:
        p.kill()
        out, _ = p.communicate()
    return (out or b'').decode('utf-8', 'replace')


class BtFinder:
    """Find the meter's classic (RFCOMM) address via bluetoothctl.

    All side effects are injectable for tests: runner(args, timeout) ->
    (rc, text), scan_popen(seconds, sleep) -> text, sleep, cache_file,
    isatty(), ask(prompt) and log(msg)."""

    def __init__(self, runner=None, scan_popen=None, sleep=time.sleep,
                 cache_file=None, isatty=None, ask=input, log=None,
                 scan_seconds=SCAN_SECONDS):
        self.runner = runner or _default_runner
        self.scan_popen = scan_popen or _default_scan_popen
        self.sleep = sleep
        self.cache_file = cache_file or config_path()
        self.isatty = isatty or (
            lambda: bool(sys.stdin) and sys.stdin.isatty())
        self.ask = ask
        self.log = log or (lambda msg: (sys.stderr.write(msg + '\n'),
                                        sys.stderr.flush()))
        self.scan_seconds = scan_seconds
        self._info_cache = {}
        self.seen_dm40 = []          # DM40-named entries without SPP (for hints)

    # -- bluetoothctl wrappers ---------------------------------------------
    def _run(self, args, timeout=BTCTL_TIMEOUT):
        rc, out = self.runner(args, timeout)
        if rc is None and not out:
            raise DiscoveryError(
                'bluetoothctl did not answer within %.0f s (is the bluetooth '
                'service running? try: systemctl status bluetooth)' % timeout)
        if 'No default controller available' in (out or ''):
            raise DiscoveryError('no Bluetooth adapter found by BlueZ '
                                 '(bluetoothctl: No default controller '
                                 'available)')
        return rc, out or ''

    def devices(self):
        return parse_devices(self._run(['devices'])[1])

    def info(self, mac):
        mac = mac.upper()
        if mac not in self._info_cache:
            self._info_cache[mac] = parse_info(self._run(['info', mac])[1])
        return self._info_cache[mac]

    def scan(self):
        """Short discovery; returns the scan output text."""
        self.log('No DM40 known to BlueZ yet, scanning for %d s '
                 '(meter on, Bluetooth enabled, multimeter screen)...'
                 % self.scan_seconds)
        rc, out = self._run(['--timeout', str(self.scan_seconds), 'scan', 'on'],
                            timeout=self.scan_seconds + 15)
        low = out.lower()
        if rc not in (0, None) and ('invalid option' in low
                                    or 'unrecognized option' in low
                                    or 'unknown option' in low
                                    or 'usage' in low):
            # old BlueZ without --timeout: run `scan on` and stop it ourselves
            out = self.scan_popen(self.scan_seconds, self.sleep) or ''
            low = out.lower()
        if 'notready' in low or 'not powered' in low:
            raise DiscoveryError('the Bluetooth adapter is off: run '
                                 '"bluetoothctl power on" (or enable '
                                 'Bluetooth in the system settings)')
        self._info_cache.clear()
        return out

    # -- candidate selection -------------------------------------------------
    def _display_name(self, mac, listed_name):
        inf = self.info(mac)
        if inf and (inf['name'] or inf['alias']):
            return inf['name'] or inf['alias']
        return listed_name

    def candidates(self, extra=()):
        """Usable meters [(MAC, name)]: DM40-named devices with the Serial
        Port UUID; failing that, the BLE->classic address heuristic."""
        listed = list(self.devices())
        known = {mac for mac, _ in listed}
        for mac, name in extra:
            if mac not in known:
                listed.append((mac, name))
                known.add(mac)
        dm40 = []
        for mac, name in listed:
            if not is_dm40_name(name):   # unnamed / other devices: skip
                continue
            inf = self.info(mac)
            full = (inf and (inf['name'] or inf['alias'])) or name
            dm40.append((mac, full, inf))
        spp = [(mac, full) for mac, full, inf in dm40 if inf and inf['spp']]
        self.seen_dm40 = [(mac, full) for mac, full, inf in dm40
                          if not (inf and inf['spp'])]
        if spp:
            return spp
        # last resort: BLE "DM40x" entry -> classic address = first byte + 1
        out = []
        for mac, full in self.seen_dm40:
            alt = derived_classic_mac(mac)
            if alt in {m for m, _ in out}:
                continue
            inf = self.info(alt)
            if inf and (inf['spp'] or is_dm40_name(inf['name'])
                        or is_dm40_name(inf['alias'])):
                out.append((alt, inf['name'] or inf['alias'] or full))
        return out

    # -- cache -----------------------------------------------------------------
    def read_cache(self):
        try:
            with open(self.cache_file) as f:
                mac = f.read().strip().upper()
        except (OSError, UnicodeDecodeError):
            return None
        return mac if re.fullmatch(_BT_MAC_RE, mac) else None

    def write_cache(self, mac):
        try:
            os.makedirs(os.path.dirname(self.cache_file), exist_ok=True)
            with open(self.cache_file, 'w') as f:
                f.write(mac + '\n')
        except OSError:
            pass

    def cached_ok(self, mac):
        inf = self.info(mac)
        if inf and (inf['spp'] or is_dm40_name(inf['name'])
                    or is_dm40_name(inf['alias'])):
            return inf['name'] or inf['alias'] or mac
        return None

    # -- main entry ------------------------------------------------------------
    def choose(self, cands):
        if len(cands) == 1:
            return cands[0]
        listing = '\n'.join('  %d) %s  %s' % (i + 1, mac, name)
                            for i, (mac, name) in enumerate(cands))
        if not self.isatty():
            raise DiscoveryError(
                'several DM40 meters found:\n%s\npass the right one with '
                '--mac AA:BB:CC:DD:EE:FF (or set DM40_MAC)' % listing)
        self.log('Several DM40 meters found:\n' + listing)
        while True:
            try:
                ans = self.ask('Choose 1-%d: ' % len(cands)).strip()
            except EOFError:
                raise DiscoveryError('no choice made; pass --mac')
            if ans.isdigit() and 1 <= int(ans) <= len(cands):
                return cands[int(ans) - 1]

    def find(self, rescan=False):
        """Return the meter's classic MAC or raise DiscoveryError."""
        if not rescan:
            cached = self.read_cache()
            if cached:
                name = self.cached_ok(cached)
                if name:
                    self.log('Using cached DM40: %s %s (--rescan to search '
                             'again)' % (name, cached))
                    return cached
        cands = self.candidates()
        if not cands:
            scan_out = self.scan()
            cands = self.candidates(extra=parse_devices(scan_out))
        if not cands:
            hint = ''
            if self.seen_dm40:
                hint = ('\nDM40-named devices without the Serial Port service '
                        '(BLE side, not usable): ' + ', '.join(
                            '%s %s' % (m, n) for m, n in self.seen_dm40))
            raise DiscoveryError(
                'no DM40 found over Bluetooth. Switch the meter\'s Bluetooth on '
                '(Settings), keep it on the MULTIMETER screen, close the phone '
                'app, and retry; or pass --mac AA:BB:CC:DD:EE:FF (the address '
                'whose "bluetoothctl info" shows "Serial Port").' + hint)
        mac, name = self.choose(cands)
        self.log('Found DM40: %s %s' % (name, mac))
        self.write_cache(mac)
        return mac


def resolve_mac(args):
    """--mac / DM40_MAC win; otherwise auto-discover via bluetoothctl."""
    if args.mac:
        return args.mac
    if not sys.platform.startswith('linux') or shutil.which('bluetoothctl') is None:
        hint = ''
        if sys.platform.startswith('win'):
            hint = (' On Windows: Settings -> Bluetooth & devices -> the meter '
                    '-> Properties, or Device Manager -> Bluetooth -> the '
                    'meter -> Details -> "Bluetooth device address".')
        sys.exit('Bluetooth auto-discovery is unavailable here (needs Linux '
                 'with BlueZ "bluetoothctl"); pass --mac AA:BB:CC:DD:EE:FF '
                 'or set DM40_MAC.' + hint)
    try:
        return BtFinder().find(rescan=args.rescan)
    except DiscoveryError as e:
        sys.exit('dm40_memrw: %s' % e)


def auto_int(x):
    return int(x, 0)


def build_parser():
    p = argparse.ArgumentParser(
        description='DM40 memory read / backup tool (read-only by default).',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__)
    p.add_argument('--mac', default=os.environ.get('DM40_MAC'),
                   help='meter Bluetooth MAC (or set DM40_MAC); optional on '
                        'Linux, where it is found automatically via '
                        'bluetoothctl and cached in ~/.config/dm40-memrw/mac')
    p.add_argument('--rescan', action='store_true',
                   help='ignore the cached address and search again')
    p.add_argument('--timeout', type=float, default=5.0,
                   help='socket timeout in seconds (default 5)')
    p.add_argument('--unsafe', action='store_true',
                   help='enable the dangerous write/call commands')
    p.add_argument('--yes', action='store_true',
                   help='skip the interactive confirmation for --unsafe')
    sub = p.add_subparsers(dest='cmd', required=True)

    s = sub.add_parser('read', help='read memory (safe)')
    s.add_argument('addr', type=auto_int)
    s.add_argument('length', type=auto_int)
    s.add_argument('-o', '--out', help='write raw bytes to this file')
    s.set_defaults(func=do_read)

    s = sub.add_parser('dump', help='back up memory regions (safe)')
    g = s.add_mutually_exclusive_group()
    g.add_argument('--quick', action='store_true',
                   help='UID + option bytes + OTP + boot data + calibration')
    g.add_argument('--full', action='store_true',
                   help='quick regions + the whole 1 MiB internal flash')
    s.add_argument('-o', '--out', required=True, help='output directory')
    s.set_defaults(func=do_dump)

    s = sub.add_parser('mode', help='set measurement function (stock cmd 06)')
    s.add_argument('mode', choices=sorted(MODE_BYTES.keys()),
                   help='measurement function name')
    s.set_defaults(func=do_mode)

    s = sub.add_parser('hold', help='HOLD on/off (stock cmd 04)')
    s.add_argument('state', choices=['on', 'off'])
    s.set_defaults(func=do_hold)

    s = sub.add_parser('rel', help='REL/relative on/off (stock cmd 05)')
    s.add_argument('state', choices=['on', 'off'])
    s.set_defaults(func=do_rel)

    s = sub.add_parser('measure', help='read one measurement (stock cmd 09)')
    s.add_argument('--json', action='store_true', help='print as JSON')
    s.set_defaults(func=do_measure)

    s = sub.add_parser(
        'set-model',
        help='temporarily switch the meter model type in RAM (A/B/C/restore)')
    s.add_argument('model', choices=['A', 'B', 'C', 'a', 'b', 'c',
                                     'restore', 'RESTORE'],
                   help='A, B, C, or restore (write back the flash default)')
    s.set_defaults(func=do_set_model)

    s = sub.add_parser('write', help='UNSAFE: write memory (needs --unsafe)')
    s.add_argument('addr', type=auto_int)
    s.add_argument('hexbytes', help='payload as hex, e.g. deadbeef')
    s.set_defaults(func=do_write)

    s = sub.add_parser('call', help='UNSAFE and BROKEN: execute code (needs --unsafe)')
    s.add_argument('addr', type=auto_int)
    s.add_argument('regs', nargs='*', help='up to 4 integer args r0..r3')
    s.set_defaults(func=do_call)
    return p


def main(argv=None):
    args = build_parser().parse_args(argv)
    args.mac = resolve_mac(args)
    args.func(args, args.mac)


if __name__ == '__main__':
    main()
