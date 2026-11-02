#!/usr/bin/env python3
"""ctest2.py read DISK: first sectors over USB equal DISK. ctest2.py copy BEFORE AFTER: write the sectors that
differ, directory and FAT first (a host that is about to fill the file), pause > 3 s halfway, then the rest."""
import os, struct, sys, time
os.environ['USBIP_PORT'] = os.environ.get('CPORT', '46050')
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '../emu'))
import usbip_client as uc
BS = 4096
for _ in range(60):
    try:
        d = uc.Dev('1-0'); break
    except Exception as e:
        time.sleep(2)
else:
    sys.exit('no device')
m = uc.Msc(d)
if sys.argv[1] == 'read':
    disk = open(sys.argv[2], 'rb').read()
    for lba, n in ((0, 2), (5, 1), (32, 1)):
        print('READ lba', lba, 'x', n, m.read(0, lba, n, BS) == disk[lba * BS:(lba + n) * BS], flush=True)
    sys.exit(0)
a, b = open(sys.argv[2], 'rb').read(), open(sys.argv[3], 'rb').read()
ch = [i for i in range(4096) if a[i * BS:(i + 1) * BS] != b[i * BS:(i + 1) * BS]]
meta = [i for i in ch if i < 9]; data = [i for i in ch if i >= 9]
print('sectors: meta', meta, 'data', len(data), flush=True)
def put(i):
    m.ok(0, struct.pack('>BBIBHB', 0x2A, 0, i, 0, 1, 0), BS, uc.DIR_OUT, b[i * BS:(i + 1) * BS])
t0 = time.time()
for i in meta: put(i)
for i in data[:len(data) // 2]: put(i)
print('half written %.1fs: pause' % (time.time() - t0), flush=True)
if os.environ.get('STOP_HALF'):
    sys.exit(0)
time.sleep(float(os.environ.get('PAUSE', 9)))
print('rest %.1fs' % (time.time() - t0), flush=True)
for i in data[len(data) // 2:]: put(i)
print('all written %.1fs' % (time.time() - t0), flush=True)
