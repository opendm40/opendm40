"""Replay a kernel write sequence captured by usbip_msc.py --capture
(captures/*.vcap.gz) through vserve on the same starting SPI image, and
check it: every WRITE gets the answer it got from the real kernel run, then
the capture's expectations (captures/<name>.expect, `key value` lines):
    deleted <n>          SPI files deleted by host deletes
    fw_ok <n>            firmware files committed
    gone <path>          SPI file that must be gone at the end
    new <path> <file>    SPI file that must exist, equal to <file>
                         (relative to the capture: FW = the --fw file)
    refused <n>          WRITEs refused
After the replay the session ends (Allow + main-loop passes), the SPI
image must pass fsck.fat cleanly and keep every other file unchanged.
The captured sectors hold the volume's layout of the capture (directory
entries with their sizes, cluster numbers): a change of what the volume
generates (the CSV text of a DLG, the INFO files, README.TXT beyond its
fixed 2048 bytes) makes a replay fail on its first such sector: capture
again (kcapture.sh; the UID is captures/uid.dat, the flash has no
firmware: no INFO/FIRMWARE.BIN)."""
import gzip, os, shutil, struct, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vserve import VServe, SECTOR


HERE = os.path.dirname(os.path.abspath(__file__))


def mkfw500(out):
    """The update file of the captures: a deterministic 500000-byte body
    (marker gap 0xFF) packed with stock/dm40fw.py pack --pad."""
    import random
    body = bytearray(random.Random(500).randbytes(500000))
    body[0x4000:0x400C] = b'\xff' * 12
    open(out + '.body', 'wb').write(body)
    subprocess.run([sys.executable, '-I', os.path.join(HERE, '../../../stock/dm40fw.py'), 'pack', '--pad', out + '.body', out],
                   check=True, capture_output=True)
    return out


def records(path, fw):
    d = gzip.open(path, 'rb').read()
    assert d[:5] == b'VCAP1', 'not a capture'
    o = 5
    fwd = open(fw, 'rb').read() if fw else b''
    while o < len(d):
        ms, op, arg = struct.unpack_from('<IcI', d, o)
        o += 9
        data, ans = None, None
        if op == b'W':
            k = d[o]
            o += 1
            if k == 0:
                data = d[o:o + SECTOR]
                o += SECTOR
            elif k == 1:
                data = bytes(SECTOR)
            else:
                off = struct.unpack_from('<I', d, o)[0]
                o += 4
                data = fwd[off:off + SECTOR]
            ans = d[o]
            o += 1
        yield ms, op, arg, data, ans


def fail(msg):
    print('FAIL:', msg, flush=True)
    sys.exit(1)


def fsck_clean(img, what):
    r = subprocess.run(['fsck.fat', '-n', img], capture_output=True, text=True)
    out = r.stdout + r.stderr
    if r.returncode or any(k in out.lower() for k in ('free cluster', 'differ', 'lost', 'orphan', 'reclaim')):
        print(out)
        fail('fsck %s' % what)


def tree(img):
    import hashlib
    out = subprocess.run(['mdir', '-a', '-/', '-b', '-i', img, '::'], capture_output=True).stdout.decode(errors='replace').split()
    res = {}
    for p in out:
        if not p.endswith('/'):
            r = subprocess.run(['mcopy', '-n', '-i', img, p, '-'], capture_output=True)
            if r.returncode == 0:
                res[p] = hashlib.md5(r.stdout).hexdigest()
    return res


def replay(cap, t, flash, src, spi0, work, fw=None):
    """`flash` must be the 1 MiB image WITHOUT an installed firmware
    (no INFO/FIRMWARE.BIN: the volume's layout must not depend on a build)."""
    name = os.path.basename(cap).split('.')[0]
    exp = {}
    ef = cap.replace('.vcap.gz', '.expect')
    for l in open(ef):
        p = l.split()
        if p and not p[0].startswith('#'):
            exp.setdefault(p[0], []).append(p[1:])
    os.makedirs(work, exist_ok=True)
    fw = os.path.join(work, 'fw500.bin')
    if not os.path.exists(fw):
        mkfw500(fw)
    # The volume serial in the boot sector is the CRC-32 of the UID: the
    # one of the capture (captures/uid.dat), the rest of <src> as given.
    rsrc = os.path.join(work, 'src-replay')
    os.makedirs(rsrc, exist_ok=True)
    for f in ('OTP.BIN', 'OPTBYTES.BIN'):
        shutil.copy(os.path.join(src, f), rsrc)
    shutil.copy(os.path.join(os.path.dirname(cap), 'uid.dat'), os.path.join(rsrc, 'UID.BIN'))
    src = rsrc
    spi = os.path.join(work, 'replay-%s.spi' % name)
    shutil.copy(spi0, spi)
    v = VServe(t, spi, flash, src)
    n = bad = 0
    for ms, op, arg, data, ans in records(cap, fw):
        while v.now + 100 < ms:
            v.tick(v.now + 100)
        v.tick(max(ms, v.now))
        if op == b'W':
            r = v.write(arg, data)
            n += 1
            if r != ans:
                fail('%s: WRITE %d at %d ms: %d, the kernel run got %d' % (name, arg, ms, r, ans))
            bad += r != 0
        elif op == b'L':
            v.lock(arg)
        elif op == b'B':
            v.bus_reset()
    st = v.stats()
    v.lock(False)
    v.idle(8000)
    st2 = v.stats()
    sp = v.save()
    v.close()
    fsck_clean(sp, 'replay ' + name)
    t0, t1 = tree(spi0), tree(sp)
    for k, vals in exp.items():
        for x in vals:
            if k in ('deleted', 'fw_ok', 'refused'):
                got = {'deleted': st2['deleted'], 'fw_ok': st2['fw_ok'], 'refused': str(bad)}[k]
                if got != x[0]:
                    fail('%s: %s = %s, expected %s' % (name, k, got, x[0]))
            elif k == 'gone':
                if x[0] in t1:
                    fail('%s: %s not deleted' % (name, x[0]))
                t0.pop(x[0], None)
            elif k == 'new':
                f = fw if x[1] == 'FW' else os.path.join(os.path.dirname(cap), x[1])
                got = subprocess.run(['mcopy', '-n', '-i', sp, x[0], '-'], capture_output=True).stdout
                if got != open(f, 'rb').read():
                    fail('%s: %s differs from %s' % (name, x[0], f))
                t1.pop(x[0], None)
    if t0 != t1:
        fail('%s: other SPI files changed: %s' % (name, sorted(set(t0.items()) ^ set(t1.items()))[:6]))
    print('  %s: %d WRITEs replayed with the kernel run\'s answers (%d refused), %s' % (name, n, bad, ' '.join('%s=%s' % (k, st2[k]) for k in ('deleted', 'fw_ok', 'fw_bad'))))
