#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Offline tests for the Bluetooth auto-discovery in dm40_memrw.py.

Everything is mocked: bluetoothctl is never started, no Bluetooth is used.

Run:  python3 -I -B tools/tests/test_bt_discovery.py
"""
import importlib.util
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(HERE, '..', 'dm40_memrw.py')

spec = importlib.util.spec_from_file_location('dm40_memrw', TOOL)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

CLASSIC = 'AB:00:00:00:00:01'
BLE = 'AA:00:00:00:00:01'
SPP = m.SPP_UUID

# --- realistic bluetoothctl output (BlueZ 5.x), with colour codes ----------
DEVICES_TWO = (
    'Agent registered\n'
    '\x1b[0;94m[bluetooth]\x1b[0m# \r'
    'Device AA:BB:CC:00:11:22 Some Headphones\n'
    'Device %s C-1-ATK-DM40-CALI\n'
    'Device %s DM40A\n' % (CLASSIC, BLE))

INFO_CLASSIC = (
    'Device %s (public)\n'
    '\tName: C-1-ATK-DM40-CALI\n'
    '\tAlias: C-1-ATK-DM40-CALI\n'
    '\tClass: 0x00001f00 (7936)\n'
    '\tPaired: yes\n'
    '\tBonded: yes\n'
    '\tTrusted: no\n'
    '\tConnected: no\n'
    '\tUUID: Serial Port               (%s)\n' % (CLASSIC, SPP))

INFO_BLE = (
    'Device %s (public)\n'
    '\tName: DM40A\n'
    '\tAlias: DM40A\n'
    '\tPaired: no\n'
    '\tUUID: Generic Access Profile    (00001800-0000-1000-8000-00805f9b34fb)\n'
    '\tUUID: Vendor specific           (0000ffe0-0000-1000-8000-00805f9b34fb)\n'
    '\tManufacturerData.Key: 0x0000 (0)\n'
    '\tRSSI: 0xffffffc4 (-60)\n' % BLE)


def info_spp(mac, name):
    return ('Device %s (public)\n\tName: %s\n\tAlias: %s\n\tPaired: yes\n'
            '\tUUID: Serial Port (%s)\n' % (mac, name, name, SPP))


def not_available(mac):
    return 'Device %s not available\n' % mac


class FakeCtl:
    """Recording stand-in for bluetoothctl.

    responses: {('devices',): text or [text, text...], ('info', MAC): text}.
    A list is consumed one item per call (last item repeats)."""

    def __init__(self, responses, scan=(0, 'Discovery started\n')):
        self.responses = dict(responses)
        self.scan_result = scan
        self.calls = []

    def __call__(self, args, timeout):
        assert timeout and timeout > 0, 'every call needs a timeout'
        args = tuple(args)
        self.calls.append(args)
        if 'scan' in args:
            return self.scan_result
        key = args
        if key not in self.responses:
            if args[0] == 'info':
                return 1, not_available(args[1])
            return 0, ''
        r = self.responses[key]
        if isinstance(r, list):
            r = r.pop(0) if len(r) > 1 else r[0]
        rc = 1 if 'not available' in r else 0
        return rc, r

    def count(self, first):
        return sum(1 for c in self.calls if c[0] == first or first in c)


def finder(ctl, cache_dir, tty=False, answers=(), scan_popen=None):
    logs = []
    answers = list(answers)
    f = m.BtFinder(runner=ctl,
                   scan_popen=scan_popen or (lambda s, sl: ''),
                   sleep=lambda s: None,
                   cache_file=os.path.join(cache_dir, 'dm40-memrw', 'mac'),
                   isatty=lambda: tty,
                   ask=lambda prompt: answers.pop(0),
                   log=logs.append)
    f.logs = logs
    return f


def read_cache(d):
    with open(os.path.join(d, 'dm40-memrw', 'mac')) as fh:
        return fh.read().strip()


def expect_discovery_error(fn, *needles):
    try:
        fn()
    except m.DiscoveryError as e:
        for n in needles:
            assert n in str(e), 'missing %r in: %s' % (n, e)
        return str(e)
    raise AssertionError('expected DiscoveryError')


# --- parsing -------------------------------------------------------------------
def test_parse_robust():
    text = ('[\x1b[0;92mNEW\x1b[0m] Device %s DM40A\n'
            '[\x1b[0;93mCHG\x1b[0m] Device %s RSSI: 0xffffffc4 (-60)\n'
            '[CHG] Device 11:22:33:44:55:66 ManufacturerData.Key: 0x0102\n'
            '[CHG] Device %s Name: C-1-ATK-DM40-CALI\n'
            '[DEL] Device 99:99:99:99:99:99 DM40B\n'
            '\x01\x1b[0;94m\x02[bluetooth]\x01\x1b[0m\x02# '
            'Device 0a:0b:0c:0d:0e:0f 0A-0B-0C-0D-0E-0F\n'
            '\xff garbage \x00 line\n' % (BLE, BLE, CLASSIC))
    devs = dict(m.parse_devices(text))
    assert devs[BLE] == 'DM40A', devs
    assert devs[CLASSIC] == 'C-1-ATK-DM40-CALI', devs
    assert devs['0A:0B:0C:0D:0E:0F'] == '', devs          # placeholder name
    assert devs['11:22:33:44:55:66'] == '', devs
    assert '99:99:99:99:99:99' not in devs                 # [DEL] ignored

    inf = m.parse_info('\x1b[0;94m' + INFO_CLASSIC + '\x1b[0m')
    assert inf['spp'] and inf['paired'] and inf['name'] == 'C-1-ATK-DM40-CALI'
    assert m.parse_info(INFO_BLE)['spp'] is False
    assert m.parse_info(not_available(CLASSIC)) is None
    assert m.parse_info('') is None
    assert m.parse_info('\x00\xff random text without structure') is None
    assert m.derived_classic_mac('AA:00:00:00:00:01') == CLASSIC
    assert m.derived_classic_mac('ff:00:00:00:00:01') == '00:00:00:00:00:01'
    print('ok  bluetoothctl output parsing (ANSI, [NEW]/[CHG]/[DEL], junk)')


# --- selection -----------------------------------------------------------------
def test_two_entries_picks_classic():
    """The real two-entry case from the dev host -> the SPP (classic) one."""
    ctl = FakeCtl({('devices',): DEVICES_TWO,
                   ('info', CLASSIC): INFO_CLASSIC,
                   ('info', BLE): INFO_BLE})
    with tempfile.TemporaryDirectory() as d:
        f = finder(ctl, d)
        assert f.find() == CLASSIC
        assert read_cache(d) == CLASSIC
        assert ctl.count('scan') == 0
        assert ('info', 'AA:BB:CC:00:11:22') not in ctl.calls   # non-DM40
        assert any(l == 'Found DM40: C-1-ATK-DM40-CALI %s' % CLASSIC
                   for l in f.logs), f.logs
    print('ok  two entries (classic + BLE "DM40A") -> picks %s' % CLASSIC)


def test_ble_only_derived_address():
    """Only the BLE "DM40A" is listed by name; classic address = +1 on the
    first byte, accepted because its info shows the Serial Port UUID."""
    ctl = FakeCtl({('devices',): 'Device %s DM40A\n' % BLE,
                   ('info', BLE): INFO_BLE,
                   ('info', CLASSIC): ('Device %s (public)\n\tPaired: yes\n'
                                       '\tUUID: Serial Port (%s)\n'
                                       % (CLASSIC, SPP))})
    with tempfile.TemporaryDirectory() as d:
        assert finder(ctl, d).find() == CLASSIC
    # ...and NOT accepted when the derived address is unknown/unrelated
    ctl = FakeCtl({('devices',): 'Device %s DM40A\n' % BLE,
                   ('info', BLE): INFO_BLE})
    with tempfile.TemporaryDirectory() as d:
        expect_discovery_error(finder(ctl, d).find, 'no DM40 found',
                               '--mac', BLE)
        assert ctl.count('scan') == 1
    print('ok  BLE-only entry -> derived classic address (only if verified)')


def test_multiple_meters():
    m2 = '63:AA:BB:CC:DD:01'
    resp = {('devices',): 'Device %s C-1-ATK-DM40-CALI\nDevice %s DM40C-CLASSIC\n'
                          % (CLASSIC, m2),
            ('info', CLASSIC): INFO_CLASSIC,
            ('info', m2): info_spp(m2, 'DM40C-CLASSIC')}
    with tempfile.TemporaryDirectory() as d:
        f = finder(FakeCtl(resp), d, tty=False)
        expect_discovery_error(f.find, 'several DM40', CLASSIC, m2, '--mac')
        assert not os.path.exists(os.path.join(d, 'dm40-memrw', 'mac'))
    with tempfile.TemporaryDirectory() as d:
        f = finder(FakeCtl(resp), d, tty=True, answers=['x', '7', '2'])
        assert f.find() == m2
        assert read_cache(d) == m2
    print('ok  several meters: non-TTY -> error with list, TTY -> user choice')


def test_none_then_scan_finds():
    ctl = FakeCtl({('devices',): ['', DEVICES_TWO],   # empty before the scan
                   ('info', CLASSIC): INFO_CLASSIC,
                   ('info', BLE): INFO_BLE})
    with tempfile.TemporaryDirectory() as d:
        f = finder(ctl, d)
        assert f.find() == CLASSIC
        scans = [c for c in ctl.calls if 'scan' in c]
        assert scans == [('--timeout', '10', 'scan', 'on')], scans
        assert ctl.calls.index(scans[0]) < len(ctl.calls) - 1
    print('ok  nothing known -> bluetoothctl --timeout 10 scan on -> found')


def test_scan_fallback_old_bluez():
    """bluetoothctl without --timeout -> Popen fallback; the device is only
    seen in the scan output (BlueZ already dropped the temporary entry)."""
    popen_calls = []

    def fake_popen(seconds, sleep):
        popen_calls.append(seconds)
        return ('[NEW] Device %s C-1-ATK-DM40-CALI\n'
                '[CHG] Device %s RSSI: -50\n' % (CLASSIC, CLASSIC))

    ctl = FakeCtl({('info', CLASSIC): INFO_CLASSIC},
                  scan=(1, "bluetoothctl: unrecognized option '--timeout'\n"))
    with tempfile.TemporaryDirectory() as d:
        assert finder(ctl, d, scan_popen=fake_popen).find() == CLASSIC
    assert popen_calls == [10], popen_calls
    print('ok  old BlueZ: scan via Popen fallback, devices from scan output')


def test_none_found_message_and_adapter_off():
    with tempfile.TemporaryDirectory() as d:
        ctl = FakeCtl({})
        expect_discovery_error(finder(ctl, d).find, 'no DM40 found',
                               'Bluetooth on', 'MULTIMETER', '--mac')
        assert ctl.count('scan') == 1
    with tempfile.TemporaryDirectory() as d:
        ctl = FakeCtl({}, scan=(1, 'Failed to start discovery: '
                                   'org.bluez.Error.NotReady\n'))
        expect_discovery_error(finder(ctl, d).find, 'power on')
    with tempfile.TemporaryDirectory() as d:
        ctl = FakeCtl({('devices',): 'No default controller available\n'})
        expect_discovery_error(finder(ctl, d).find, 'no Bluetooth adapter')
    print('ok  none found -> clear error; adapter off / no adapter hints')


# --- cache -----------------------------------------------------------------------
def _write_cache(d, mac):
    os.makedirs(os.path.join(d, 'dm40-memrw'))
    with open(os.path.join(d, 'dm40-memrw', 'mac'), 'w') as fh:
        fh.write(mac + '\n')


def test_cache_hit():
    ctl = FakeCtl({('info', CLASSIC): INFO_CLASSIC})
    with tempfile.TemporaryDirectory() as d:
        _write_cache(d, CLASSIC.lower())
        f = finder(ctl, d)
        assert f.find() == CLASSIC
        assert ctl.calls == [('info', CLASSIC)], ctl.calls   # no devices/scan
        assert 'cached' in f.logs[0]
    print('ok  cache hit: verified with one "info", no devices/scan')


def test_cache_miss_and_rescan():
    stale = '11:22:33:44:55:66'
    resp = {('devices',): DEVICES_TWO,
            ('info', CLASSIC): INFO_CLASSIC,
            ('info', BLE): INFO_BLE}
    with tempfile.TemporaryDirectory() as d:
        _write_cache(d, stale)                      # unknown to BlueZ now
        ctl = FakeCtl(resp)
        assert finder(ctl, d).find() == CLASSIC
        assert ctl.calls[0] == ('info', stale)
        assert read_cache(d) == CLASSIC             # rewritten
    with tempfile.TemporaryDirectory() as d:
        _write_cache(d, 'not a mac')                # corrupt cache ignored
        assert finder(FakeCtl(resp), d).find() == CLASSIC
    with tempfile.TemporaryDirectory() as d:
        _write_cache(d, CLASSIC)                    # valid, but --rescan
        ctl = FakeCtl(resp)
        assert finder(ctl, d).find(rescan=True) == CLASSIC
        assert ('devices',) in ctl.calls
    print('ok  cache miss -> rediscover + rewrite; corrupt cache; --rescan')


# --- CLI / platform / subprocess plumbing -----------------------------------------
class Args:
    def __init__(self, mac=None, rescan=False):
        self.mac = mac
        self.rescan = rescan


def test_resolve_mac_overrides_and_platform():
    orig_finder, orig_which, orig_plat = m.BtFinder, m.shutil.which, m.sys.platform

    class Boom:
        def __init__(self, *a, **k):
            raise AssertionError('discovery must not run when --mac is given')

    try:
        m.BtFinder = Boom
        assert m.resolve_mac(Args(mac='AA:BB:CC:DD:EE:FF')) == 'AA:BB:CC:DD:EE:FF'

        m.shutil.which = lambda name: None          # no bluetoothctl
        try:
            m.resolve_mac(Args())
        except SystemExit as e:
            assert '--mac' in str(e.code) and 'unavailable' in str(e.code)
        else:
            raise AssertionError('expected SystemExit')

        m.shutil.which = lambda name: '/usr/bin/' + name
        m.sys.platform = 'win32'
        try:
            m.resolve_mac(Args())
        except SystemExit as e:
            assert 'Device Manager' in str(e.code) and '--mac' in str(e.code)
        else:
            raise AssertionError('expected SystemExit')

        m.sys.platform = 'linux'
        seen = {}

        class FakeFinder:
            def find(self, rescan=False):
                seen['rescan'] = rescan
                return CLASSIC
        m.BtFinder = FakeFinder
        assert m.resolve_mac(Args(rescan=True)) == CLASSIC and seen['rescan']

        class FailFinder:
            def find(self, rescan=False):
                raise m.DiscoveryError('no DM40 found ...')
        m.BtFinder = FailFinder
        try:
            m.resolve_mac(Args())
        except SystemExit as e:
            assert 'no DM40 found' in str(e.code)
        else:
            raise AssertionError('expected SystemExit')
    finally:
        m.BtFinder, m.shutil.which, m.sys.platform = (orig_finder, orig_which,
                                                      orig_plat)
    print('ok  --mac skips discovery; no bluetoothctl / Windows -> --mac hint')


def test_main_uses_resolved_mac():
    got = {}
    orig_read, orig_resolve = m.do_read, m.resolve_mac
    try:
        m.do_read = lambda args, mac: got.setdefault('mac', mac)
        m.resolve_mac = lambda args: CLASSIC
        m.main(['read', '0x08000000', '4'])
        assert got['mac'] == CLASSIC
    finally:
        m.do_read, m.resolve_mac = orig_read, orig_resolve
    print('ok  main() passes the auto-resolved address to the command')


def test_default_runner_timeouts():
    orig = m.subprocess.run
    seen = {}

    def fake_run(cmd, **kw):
        seen.update(kw, cmd=cmd)
        raise subprocess.TimeoutExpired(cmd, kw['timeout'])
    try:
        m.subprocess.run = fake_run
        rc, out = m._default_runner(['devices'], 8.0)
        assert rc is None and out == ''
        assert seen['cmd'] == ['bluetoothctl', 'devices']
        assert seen['timeout'] == 8.0
        assert seen['stdin'] is subprocess.DEVNULL
        with tempfile.TemporaryDirectory() as d:
            f = m.BtFinder(runner=m._default_runner, cache_file=os.path.join(
                d, 'mac'), log=lambda s: None)
            expect_discovery_error(f.find, 'did not answer')
    finally:
        m.subprocess.run = orig
    print('ok  subprocess timeouts are enforced and reported')


def test_default_scan_popen_stops_scan():
    events = []

    class FakeProc:
        def __init__(self, cmd, **kw):
            events.append(('start', tuple(cmd)))
            self.stdin = self

        def write(self, b):
            events.append(('stdin', b))

        def flush(self):
            pass

        def terminate(self):
            events.append(('terminate',))

        def kill(self):
            events.append(('kill',))

        def communicate(self, timeout=None):
            if timeout is not None:
                raise subprocess.TimeoutExpired('bluetoothctl', timeout)
            return b'[NEW] Device AB:00:00:00:00:01 C-1-ATK-DM40-CALI\n', b''

    orig = m.subprocess.Popen
    try:
        m.subprocess.Popen = FakeProc
        out = m._default_scan_popen(10, lambda s: events.append(('sleep', s)))
    finally:
        m.subprocess.Popen = orig
    assert events[0] == ('start', ('bluetoothctl', 'scan', 'on'))
    assert ('sleep', 10) in events and ('terminate',) in events
    assert ('kill',) in events                 # hung process is killed
    assert dict(m.parse_devices(out))[CLASSIC] == 'C-1-ATK-DM40-CALI'
    print('ok  Popen scan fallback: sleeps, stops, kills if stuck')


if __name__ == '__main__':
    test_parse_robust()
    test_two_entries_picks_classic()
    test_ble_only_derived_address()
    test_multiple_meters()
    test_none_then_scan_finds()
    test_scan_fallback_old_bluez()
    test_none_found_message_and_adapter_off()
    test_cache_hit()
    test_cache_miss_and_rescan()
    test_resolve_mac_overrides_and_platform()
    test_main_uses_resolved_mac()
    test_default_runner_timeouts()
    test_default_scan_popen_stops_scan()
    print('\nALL DISCOVERY TESTS PASSED')
