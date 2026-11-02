#!/usr/bin/env python3
"""(c): enumeration and volume read/write over USB/IP. ctest.py DISK_IMAGE (a copy of what the emulator started with)."""
import os, struct, sys, time
os.environ['USBIP_PORT'] = os.environ.get('CPORT','46050')
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '../emu'))
import usbip_client as uc

disk = open(sys.argv[1], 'rb').read()
BS = 4096
fail = 0
def check(ok, what):
    global fail
    print(('ok   ' if ok else 'FAIL ') + what, flush=True)
    fail += not ok

for _ in range(100):
    try:
        d = uc.Dev('1-0')
        break
    except Exception as e:
        print('wait', e, flush=True); time.sleep(3)
else:
    sys.exit('no device')
dd = d.device_descriptor(); vp = struct.unpack_from('<HH', dd, 8)
check(vp == (0x28e9, 0x028f), 'VID:PID %04x:%04x, device descriptor %s' % (vp[0], vp[1], dd.hex()))
m = uc.Msc(d)
check(m.max_lun() == 0, 'GET MAX LUN 0')
print('inquiry', m.inquiry(0), flush=True)
st, _, _ = m.cmd(0, bytes(6)); check(st == 0, 'TEST UNIT READY')
last, bs = m.capacity(0); check((last, bs) == (4095, 4096), 'READ CAPACITY %d %d' % (last, bs))
check(len(m.ok(0, bytes([0x1A, 0, 0x3F, 0, 4, 0]), 4)) == 4, 'MODE SENSE(6)')
t = time.time()
for lba, n in ((0, 1), (1, 2), (3, 1), (2000, 2)):
    got = m.read(0, lba, n, BS)
    want = disk[lba * BS:(lba + n) * BS]
    if got != want:
        diff = [i for i in range(min(len(got), len(want))) if got[i] != want[i]]
        print('  len', len(got), len(want), 'first diffs', diff[:8], len(diff), got[:32].hex(), want[:32].hex())
    check(got == want, 'READ(10) lba %d x %d' % (lba, n))
print('read wall %.1fs' % (time.time() - t), flush=True)
st, res, _ = m.cmd(0, bytes([0xF7, 0, 0, 0, 0, 0]), 64); check(st == 1 and res == 64, 'unknown op fails, residue %d' % res)
check(m.sense(0) == '05/20/00', 'REQUEST SENSE after it')
pat = bytes((i * 7 + (i >> 8)) & 255 for i in range(BS))
lba = 3000
keep = m.read(0, lba + 1, 1, BS)
wr = struct.pack('>BBIBHB', 0x2A, 0, lba, 0, 1, 0)
m.ok(0, wr, BS, uc.DIR_OUT, pat)
check(m.read(0, lba, 1, BS) == pat, 'WRITE(10) one sector, read back')
check(m.read(0, lba + 1, 1, BS) == keep, 'next sector unchanged')
st, _, _ = m.cmd(0, struct.pack('>BBIBHB', 0x28, 0, 4095, 0, 2, 0), 2 * BS); check(st == 1, 'READ past the end fails')
# BOT reset in the middle of a read data phase, as usb-storage recovery does.
m.tag += 1
m.d.bulk_out(m.ep_out, struct.pack('<4sIIBBB16s', b'USBC', m.tag, 8 * BS, 0x80, 0, 10, struct.pack('>BBIBHB', 0x28, 0, 0, 0, 8, 0).ljust(16, b'\0')))

m.d.bulk_in(m.ep_in, 64)

m.d.control_out(0x21, 0xFF, 0, m.iface)

for ep in (m.ep_in, m.ep_out):
    m.d.control_out(0x02, 0x01, 0, ep)
st, _, _ = m.cmd(0, bytes(6)); check(st == 0, 'BOT reset mid-read: TEST UNIT READY')
check(m.read(0, 0, 1, BS) == disk[:BS], 'BOT reset mid-read: READ(10)')
print('FAILED %d' % fail if fail else 'ALL OK', flush=True)
sys.exit(1 if fail else 0)
