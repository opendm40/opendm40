#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Offline tests for dm40_memrw.py. No hardware, no Bluetooth needed.

Run:  python3 -I tools/tests/test_dm40_memrw.py
"""
import importlib.util
import json
import os
import struct
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(HERE, '..', 'dm40_memrw.py')

spec = importlib.util.spec_from_file_location('dm40_memrw', TOOL)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


# --- 1. packet encoding matches the original client byte-for-byte ----------
def _orig_pkt(sub, addr, n=0, payload=b''):
    """Verbatim copy of the ORIGINAL client's crc()/pkt() (stock/patch)."""
    ID = bytes([0x05, 0x03])

    def crc(d):
        return d + struct.pack('B', (0x100 - sum(d)) & 0xFF)

    data = struct.pack('<IBB', addr, sub, n) + payload
    return crc(bytes([0xAF]) + ID + bytes([0x0B, len(data)]) + data)


def test_packet_encoding():
    cases = [
        (0, 0x08000000, 0x10, b''),
        (0, 0x1FFF7A10, 0x0C, b''),
        (0, 0x080C0000, 0xFF, b''),
        (0, 0x080A4000, 0x01, b''),
        (1, 0x20001000, 4, b'\xde\xad\xbe\xef'),
        (2, 0x08006444, 0, struct.pack('<4I', 1, 2, 3, 4)),
    ]
    for sub, addr, n, payload in cases:
        got = m.pkt(sub, addr, n, payload)
        exp = _orig_pkt(sub, addr, n, payload)
        assert got == exp, 'mismatch sub=%d addr=%08x: %s != %s' % (
            sub, addr, got.hex(' '), exp.hex(' '))
    # concrete golden vector: read 16 B at 0x08000000
    golden = bytes.fromhex('af 05 03 0b 06 00 00 00 08 00 10 20'.replace(' ', ''))
    assert m.pkt(0, 0x08000000, 0x10) == golden, m.pkt(0, 0x08000000, 0x10).hex(' ')
    print('ok  packet encoding matches original client')


def test_crc():
    # crc makes the whole packet sum to 0 mod 256
    pk = m.pkt(0, 0x08004000, 0x80)
    assert sum(pk) & 0xFF == 0
    print('ok  crc')


# --- 2. mock device: dump + manifest logic ---------------------------------
class FakeMemory:
    """A flat fake address space returning deterministic bytes."""

    def byte(self, addr):
        return (addr * 7 + 0x5A) & 0xFF

    def read(self, addr, n):
        return bytes(self.byte(addr + i) for i in range(n))


class FakeDev:
    """Stand-in for Dev: same read()/version()/close() surface, no sockets."""

    def __init__(self, mem, flaky=0):
        self.mem = mem
        self.flaky = flaky          # fail this many reads then recover
        self.reads = 0

    def read(self, addr, n):
        self.reads += 1
        if self.flaky > 0:
            self.flaky -= 1
            raise m.DeviceError('simulated transient read error')
        return self.mem.read(addr, n)

    def version(self):
        return '2.6'

    def close(self):
        pass


def _make_backup(dev):
    """Build a Backup that uses a pre-made FakeDev and never really sleeps."""
    b = m.Backup.__new__(m.Backup)
    b.mac = 'AA:BB:CC:DD:EE:FF'
    b.timeout = 5.0
    b.reconnect_wait = 0.0
    b.connect_tries = 5
    b.read_tries = 8
    b.verify_tries = 5
    b.dev = dev
    b._reconnect = lambda: None      # no real reconnect in the test
    return b


def test_read_verified_and_retry():
    mem = FakeMemory()
    dev = FakeDev(mem, flaky=2)      # first two reads throw, then succeed
    b = _make_backup(dev)
    data = b.read_verified(0x08000000, 64)
    assert data == mem.read(0x08000000, 64)
    print('ok  read_verified recovers from transient errors')


def test_dump_region_and_hashes():
    import hashlib
    mem = FakeMemory()
    dev = FakeDev(mem)
    b = _make_backup(dev)
    with tempfile.TemporaryDirectory() as d:
        path = os.path.join(d, 'blk.bin')
        data = b.dump_region(path, 0x080C0000, 0x8000, 4096, 'test')
        assert data == mem.read(0x080C0000, 0x8000)
        with open(path, 'rb') as f:
            assert f.read() == data
        assert hashlib.sha256(data).hexdigest() == hashlib.sha256(
            mem.read(0x080C0000, 0x8000)).hexdigest()
    print('ok  dump_region writes the exact bytes and hashes match')


def test_full_dump_manifest():
    """Drive do_dump(--quick) end to end with a FakeDev, check the manifest."""
    mem = FakeMemory()

    # monkeypatch Backup so do_dump gets our FakeDev
    orig_init = m.Backup.__init__

    def fake_init(self, mac, **kw):
        self.mac = mac
        self.reconnect_wait = 0.0
        self.connect_tries = 5
        self.read_tries = 8
        self.verify_tries = 5
        self.dev = FakeDev(mem)
        self._reconnect = lambda: None

    m.Backup.__init__ = fake_init
    try:
        with tempfile.TemporaryDirectory() as d:
            class A:
                quick = True
                full = False
                out = d
                timeout = 5.0
            m.do_dump(A(), 'AA:BB:CC:DD:EE:FF')
            with open(os.path.join(d, 'manifest.json')) as f:
                man = json.load(f)
            assert man['kind'] == 'quick'
            assert man['device_mac'] == 'AA:BB:CC:DD:EE:FF'
            assert man['firmware_version'] == '2.6'
            names = {e['file'] for e in man['files']}
            assert names == {q[0] for q in m.QUICK_REGIONS}, names
            # every file on disk, size + hash consistent
            import hashlib
            for e in man['files']:
                p = os.path.join(d, e['file'])
                raw = open(p, 'rb').read()
                assert len(raw) == e['size']
                assert hashlib.sha256(raw).hexdigest() == e['sha256']
            # SHA256SUMS present and consistent
            sums = open(os.path.join(d, 'SHA256SUMS')).read()
            for e in man['files']:
                assert e['sha256'] in sums and e['file'] in sums
    finally:
        m.Backup.__init__ = orig_init
    print('ok  quick dump produces a correct manifest + SHA256SUMS')


# --- 3. set-model check logic ----------------------------------------------
class RegionDev:
    """Mock device backed by an addr->bytes map; supports read/write/version."""

    def __init__(self, regions):
        self.mem = bytearray(0)
        self.map = {}          # addr -> bytearray
        for addr, data in regions.items():
            self.map[addr] = bytearray(data)

    def _region(self, addr):
        for base, buf in self.map.items():
            if base <= addr < base + len(buf):
                return base, buf
        raise m.DeviceError('unmapped read @0x%08x' % addr)

    def read(self, addr, n):
        base, buf = self._region(addr)
        off = addr - base
        assert off + n <= len(buf), 'read past region @0x%08x' % addr
        return bytes(buf[off:off + n])

    def write(self, addr, data):
        base, buf = self._region(addr)
        off = addr - base
        buf[off:off + len(data)] = data

    def version(self):
        return '2.6'

    def close(self):
        pass


def _good_model_device(model_byte=0):
    settings = bytearray(0x200)
    settings[0:16] = bytes.fromhex('0123456701000000000080400000c03f')
    settings[m.MODEL_OFFSET] = model_byte
    return RegionDev({
        m.APP_VECTORS_ADDR: m.APP_VECTORS_V26,
        m.SETTINGS_FLASH: bytes(settings),            # flash record
        m.SETTINGS_RAM: bytes(settings),              # identical RAM copy
        m.DMM_PAGE_MODEL_RAM: bytes([model_byte]),
    })


def test_model_checks_pass():
    dev = _good_model_device(model_byte=0)   # DM40A
    m.check_v26_image(dev)                   # must not raise
    flash_model = m.check_settings_mirror(dev)
    assert flash_model == 0
    print('ok  set-model checks pass on a consistent device (flash model A)')


def test_model_check_wrong_image():
    dev = RegionDev({m.APP_VECTORS_ADDR: b'\x00' * 8})
    try:
        m.check_v26_image(dev)
    except m.ModelCheckError:
        print('ok  set-model refuses a non-V2.6 image')
        return
    assert False, 'expected ModelCheckError for bad vectors'


def test_model_check_ram_flash_mismatch():
    settings = bytearray(0x200)
    settings[0:16] = bytes.fromhex('0123456701000000000080400000c03f')
    ram = bytearray(settings)
    ram[0] ^= 0xFF                           # RAM copy differs from flash
    dev = RegionDev({
        m.APP_VECTORS_ADDR: m.APP_VECTORS_V26,
        m.SETTINGS_FLASH: bytes(settings),
        m.SETTINGS_RAM: bytes(ram),
    })
    m.check_v26_image(dev)
    try:
        m.check_settings_mirror(dev)
    except m.ModelCheckError:
        print('ok  set-model refuses when RAM copy != flash record')
        return
    assert False, 'expected ModelCheckError for RAM/flash mismatch'


def test_model_write_roundtrip():
    """Writing MODEL_RAM to 2 (DM40C) and reading back works on the mock."""
    dev = _good_model_device(model_byte=0)
    dev.write(m.MODEL_RAM, bytes([2]))
    dev.write(m.DMM_PAGE_MODEL_RAM, bytes([2]))
    assert dev.read(m.MODEL_RAM, 1)[0] == 2
    assert dev.read(m.DMM_PAGE_MODEL_RAM, 1)[0] == 2
    # flash record is untouched (still A)
    assert dev.read(m.MODEL_FLASH, 1)[0] == 0
    print('ok  set-model RAM write round-trips, flash untouched')


# --- 4. stock BT protocol framing + measurement decode ----------------------
def test_stock_frame():
    # cmd 06 (mode) with one data byte: AF 05 03 06 01 <byte> crc, sums to 0
    f = m.stock_frame(m.STOCK_MODE, bytes([0x28]))
    assert f[:5] == bytes([0xAF, 0x05, 0x03, 0x06, 0x01])
    assert f[5] == 0x28
    assert sum(f) & 0xFF == 0
    # measurement request has no data: AF 05 03 09 00 crc
    f = m.stock_frame(m.STOCK_MEAS)
    assert f[:5] == bytes([0xAF, 0x05, 0x03, 0x09, 0x00])
    assert sum(f) & 0xFF == 0
    print('ok  stock_frame framing + checksum')


class StockDev:
    """Mock meter answering stock commands; records what was sent."""

    def __init__(self, meas_payload):
        self.meas = meas_payload
        self.sent = []

    def stock(self, cmd, data=b''):
        self.sent.append((cmd, bytes(data)))
        if cmd == m.STOCK_MEAS:
            return self.meas
        return b'\x00'          # status byte for 04/05/06

    def close(self):
        pass


def test_stock_commands_send_right_bytes():
    # drive do_mode / do_hold / do_rel through a patched Dev
    orig = m.Dev
    captured = {}

    def fake_dev(mac, timeout):
        d = StockDev(b'')
        captured['dev'] = d
        return d
    m.Dev = fake_dev
    try:
        class A:
            timeout = 5.0
            mode = 'acdcv'
        m.do_mode(A(), 'MAC')
        assert captured['dev'].sent[-1] == (m.STOCK_MODE, bytes([0xA8]))

        class H:
            timeout = 5.0
            state = 'on'
        m.do_hold(H(), 'MAC')
        assert captured['dev'].sent[-1] == (m.STOCK_HOLD, b'\x01')

        class R:
            timeout = 5.0
            state = 'off'
        m.do_rel(R(), 'MAC')
        assert captured['dev'].sent[-1] == (m.STOCK_REL, b'\x00')
    finally:
        m.Dev = orig
    print('ok  mode/hold/rel send the correct stock command bytes')


def test_measure_decode():
    # Reference frame payload from bt_meas_v2_6.h example:
    # 28 05 00 00 04 FF FF FF FF FD 56  => 222.69 mV (V auto, DC)
    payload = bytes.fromhex('2805000004fffffffffd56')
    mode, fmtm = payload[0], payload[4]
    vm = payload[9] | (payload[10] << 8)
    val, unit = m.decode_value(mode, fmtm, vm)
    assert unit == 'mV'
    assert abs(val - 222.69) < 1e-6, val
    # value2/value3 are 0xFFFF -> None (OL/no value)
    assert m.decode_value(mode, payload[2], payload[5] | (payload[6] << 8))[0] is None
    # flags 0x05 = battery level 5, no HOLD/REL
    assert m._flags_str(0x05) == 'batt=5'
    assert 'HOLD' in m._flags_str(0x80 | 0x05)
    print('ok  measurement decode matches the documented reference frame')


def test_version_from_vectors():
    """Dev.version() identifies V2.6 from memory reads only (no stock cmds)."""
    d = m.Dev.__new__(m.Dev)
    d.read = lambda addr, n: (m.APP_VECTORS_V26 if addr == m.APP_VECTORS_ADDR
                              else b'\x00' * n)
    assert d.version().startswith('V2.6')
    d.read = lambda addr, n: b'\x00' * n
    assert d.version() is None

    def boom(addr, n):
        raise m.DeviceError('x')
    d.read = boom
    assert d.version() is None
    print('ok  version() from app vectors, None when unrecognised/error')


if __name__ == '__main__':
    test_stock_frame()
    test_stock_commands_send_right_bytes()
    test_measure_decode()
    test_version_from_vectors()
    test_packet_encoding()
    test_crc()
    test_read_verified_and_retry()
    test_dump_region_and_hashes()
    test_full_dump_manifest()
    test_model_checks_pass()
    test_model_check_wrong_image()
    test_model_check_ram_flash_mismatch()
    test_model_write_roundtrip()
    print('\nALL TESTS PASSED')
