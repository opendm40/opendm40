#!/usr/bin/env python3
"""Host test of the one USB disk mode (feature vdisk-one: src/vdisk.rs,
src/vwrite.rs "one mode"):
    vone_test.py <t1> <flash1m> <srcdir> <spi.img> <fw.bin> <work>
<t1> = the fatren-test binary built with vdisk-one (vserve), <spi.img> = a
SPI disk with logs, scope records and screenshots, <fw.bin> = an update
file.

As vwrite_test.py: a "host" is a copy of the volume dump changed by mtools,
its sector diff written through vserve. The meter adds files while the host
holds the volume (`meter_add`: a file written into the SPI image from its
last free clusters, reloaded, refreshed), so many host writes carry a stale
copy of a sector. Every scenario checks: no write refused, no file of ours
deleted but the ones the host deleted, the firmware file accepted, the
host's other files gone after the eject / re-plug, fsck of the SPI disk and
of the volume clean.
Scenarios: the layout (host region free, the rest bad clusters, fsck);
firmware (Linux-like orders, an Allow mid-copy, cp + eject at once,
Windows-like lazy write-back with the entry first and the FAT last, a meter
file added in the middle); deletes of 1 and 3 files with meter files added
before the host's write-back (stale directory / FAT sectors), in several
orders and with the FAT seconds after the entry; a host file over the slot
of a meter file it has not seen, then deleted; a delete on the meter while
connected (tombstone) and a new file of the same name; host files / folders
(root, SHOTS/, System Volume Information) dropped at the eject; a Linux
trash (deleted at the eject, kept when restored); the bus reset (compacted,
tombstones gone).
"""
import os, random, shutil, struct, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vserve import VServe, diff, sectors, Geo, SECTOR, W_OK

T, FLASH, SRC, SPI0, FW1, WORK = sys.argv[1:7]
os.environ['MTOOLS_SKIP_CHECK'] = '1'
os.makedirs(WORK, exist_ok=True)
NOK = 0
HOST_CL = 128


def ok(msg):
    global NOK
    NOK += 1
    print('ok:', msg, flush=True)


def fail(msg):
    print('FAIL:', msg, flush=True)
    sys.exit(1)


def run(*a, check=True):
    r = subprocess.run(a, capture_output=True)
    if check and r.returncode:
        fail('%s: %s' % (' '.join(a), r.stderr.decode()))
    return r.stdout


def mtools(img, *ops):
    for op in ops:
        run(*op[:1], '-i', img, *op[1:])


def fsck_clean(img, what):
    r = subprocess.run(['fsck.fat', '-n', img], capture_output=True, text=True)
    out = r.stdout + r.stderr
    if r.returncode or any(k in out.lower() for k in ('free cluster', 'differ', 'lost', 'orphan', 'reclaim', 'shared')):
        print(out)
        fail('fsck %s' % what)


def ls(img, d='::'):
    out = run('mdir', '-a', '-i', img, d).decode(errors='replace')
    res = {}
    for l in out.splitlines():
        p = l.split()
        if len(p) >= 3 and p[2].isdigit() and not l.startswith(' '):
            res[(p[0] + '.' + p[1]) if len(p) >= 4 and not p[1].isdigit() else p[0]] = int(p[2])
    return res


def names(img):
    """Every file path of a FAT image."""
    out = run('mdir', '-a', '-/', '-b', '-i', img, '::').decode(errors='replace').splitlines()
    return {p for p in out if not p.endswith('/')}


def ours(img):
    """Our files on the SPI disk (numbered DLG / BMP), as paths."""
    return {p for p in names(img) if p.upper().endswith(('.DLG', '.BMP')) and 'SNAP' not in p.upper()}


def check_spi(v, want, what):
    sp = v.save()
    fsck_clean(sp, what)
    have = ours(sp)
    if have != want:
        fail('%s: SPI files %s, expected %s (missing %s, extra %s)' % (what, sorted(have), sorted(want), sorted(want - have), sorted(have - want)))
    return sp


def cleanup():
    for f in os.listdir(WORK):
        if any(k in f for k in ('.vol', '.host', '.spi', '.in')):
            os.remove(os.path.join(WORK, f))


def fresh(name):
    """A new session: the meter on, plugged (bus reset), the host reads
    the whole volume (its cache = `host`)."""
    cleanup()
    spi = os.path.join(WORK, name + '.spi')
    shutil.copy(SPI0, spi)
    v = VServe(T, spi, FLASH, SRC)
    v.tick(100)
    v.bus_reset()
    v.tick(200)
    base = v.dump(0)
    host = os.path.join(WORK, name + '.host')
    run('cp', '--sparse=always', base, host)
    return v, spi, base, host


def snapshot(host, tag):
    p = host + '.' + tag
    run('cp', '--sparse=always', host, p)
    return p


# --- the meter adds a file: written into the SPI image from its last free
# clusters (the staging takes the first ones), as the screenshot writer would.

def spi_geo(img):
    with open(img, 'rb') as f:
        b = f.read(512)
    bps, spc, rsvd, nf, rootn, tot16, media, fsz = struct.unpack_from('<HBHBHHBH', b, 0x0B)
    tot = tot16 or struct.unpack_from('<I', b, 0x20)[0]
    root = rsvd + nf * fsz
    data = root + rootn * 32 // bps
    return dict(bps=bps, spc=spc, rsvd=rsvd, nf=nf, fsz=fsz, root=root, rootn=rootn, data=data, ncl=(tot - data) // spc + 2)


def inject(img, dirname, name83, payload):
    g = spi_geo(img)
    bps, cb = g['bps'], g['bps'] * g['spc']
    with open(img, 'r+b') as f:
        def rd(sec, n=1):
            f.seek(sec * bps)
            return bytearray(f.read(n * bps))

        def wr(sec, d):
            f.seek(sec * bps)
            f.write(d)
        fat = rd(g['rsvd'], g['fsz'])
        nent = min(g['ncl'], len(fat) // 2)
        get = lambda c: struct.unpack_from('<H', fat, c * 2)[0]
        root = rd(g['root'], g['rootn'] * 32 // bps)
        dc = None
        for i in range(0, len(root), 32):
            if root[i:i + 11] == dirname.ljust(11).encode() and root[i + 11] & 0x10:
                dc = struct.unpack_from('<H', root, i + 26)[0]
        assert dc, dirname
        need = (len(payload) + cb - 1) // cb
        free = [c for c in range(nent - 1, 1, -1) if get(c) == 0][:need]
        assert len(free) == need
        free.sort()
        for k, c in enumerate(free):
            struct.pack_into('<H', fat, c * 2, free[k + 1] if k + 1 < need else 0xFFFF)
            wr(g['data'] + (c - 2) * g['spc'], payload[k * cb:(k + 1) * cb].ljust(cb, b'\0'))
        for n in range(g['nf']):
            wr(g['rsvd'] + n * g['fsz'], fat)
        c = dc
        while True:
            sec = g['data'] + (c - 2) * g['spc']
            d = rd(sec, g['spc'])
            for i in range(0, len(d), 32):
                if d[i] in (0, 0xE5):
                    e = name83.ljust(11).encode() if isinstance(name83, str) else name83
                    d[i:i + 32] = e + bytes([0x20]) + bytes(14) + struct.pack('<HI', free[0], len(payload))
                    wr(sec, d)
                    return
            c = get(c)
            assert c < 0xFFF8, 'directory full'


def meter_add(v, spi, dirname, name83, payload, wait=1500):
    """The meter wrote a new file: SPI image + volume refresh."""
    src = v.save()
    shutil.copy(src, spi + '.in')
    inject(spi + '.in', dirname, name83, payload)
    v._op(b'J')
    v._1()
    v.idle(wait)


def meter_delete(v, name83):
    if not v.gallery_delete(name83):
        fail('gallery delete %s' % name83)
    v.idle(1500)


def eject(v):
    v._op(b'E')
    v._1()
    v.idle(7000)


def writes(before, after):
    """after's sector diff vs before, as a host writes it: with the zero
    sectors of the clusters it allocated (as vwrite_test.apply)."""
    ws = diff(before, after)
    g = Geo(before)
    fa, fb = g.fat(), Geo(after).fat()
    have = {l for l, _ in ws}
    z = bytes(SECTOR)
    zl = []
    for c in range(2, len(fb)):
        if fb[c] != 0 and fb[c] != 0xFFF7 and fa[c] == 0:
            for q in range(g.spc):
                lba = g.data + (c - 2) * g.spc + q
                if lba not in have:
                    ws.append((lba, z))
                    zl.append(lba)
    if zl:
        with open(after, 'r+b') as f:
            for lba in zl:
                f.seek(lba * SECTOR)
                f.write(z)
    ws.sort()
    return ws


def apply(v, before, after, order='asc', seed=1, fat_after_ms=0, split=None):
    """Write after's sector diff (vs before) through vserve (`writes`).
    `fat_after_ms`: the FAT sectors that long after the rest (a lazy
    write-back); `split` = (k, fn): fn() after the k-th write. Every
    write must be accepted."""
    ws = writes(before, after)
    g = Geo(before)
    kind = lambda w: g.kind(w[0])
    if order == 'desc':
        ws = ws[::-1]
    elif order == 'random':
        random.Random(seed).shuffle(ws)
    elif order == 'data-first':
        ws = [w for w in ws if kind(w) == 'data'] + [w for w in ws if kind(w) != 'data']
    elif order == 'meta-first':
        ws = [w for w in ws if kind(w) != 'data'] + [w for w in ws if kind(w) == 'data']
    if fat_after_ms:
        ws = [w for w in ws if kind(w) != 'fat'] + [None] + [w for w in ws if kind(w) == 'fat']
    n = 0
    for w in ws:
        if w is None:
            v.idle(fat_after_ms)
            continue
        lba, x = w
        r = v.write(lba, x)
        if r != W_OK:
            fail('write %d (%s): %d' % (lba, g.kind(lba), r))
        n += 1
        if split and n == split[0]:
            split[1]()
        v.tick()
    return len(ws)


def fat_counts(vol):
    fat = Geo(vol).fat()[2:2 + 65000]
    return sum(1 for x in fat if x == 0), sum(1 for x in fat if x == 0xFFF7)


ALL = ours(SPI0)
BMP = open(FW1, 'rb').read()[:307266].ljust(307266, b'\x42')
print('=== vone: the one USB disk mode (vdisk-one), %d files of ours on the SPI disk' % len(ALL))

print('--- layout: writable, the host region free, the rest bad clusters')
v, spi, base, host = fresh('layout')
if v.wp():
    fail('write-protected')
free, bad = fat_counts(base)
if free != HOST_CL or bad < 60000:
    fail('free %d bad %d' % (free, bad))
fsck_clean(base, 'fresh volume')
v.lock(True)
v.mode(True)  # Settings has no switch: a read-only request changes nothing
v.bus_reset()
if v.wp():
    fail('read-only after a mode request')
v.close()
ok('no WP bit; %d free clusters (%d KiB) at the end, %d bad; fsck clean' % (free, free * 32, bad))

print('--- firmware: Linux-like copies, an Allow mid-copy, cp + eject at once')
for tag, order, allow_at in (('asc', 'asc', None), ('data-first', 'data-first', 40), ('meta-first', 'meta-first', 3), ('random', 'random', 100)):
    v, spi, base, host = fresh('fw-' + tag)
    v.lock(True)
    mtools(host, ('mcopy', FW1, '::dm40fw.bin'))
    sp = (allow_at, lambda: v.lock(False)) if allow_at else None
    apply(v, base, host, order, split=sp)
    v.idle(1000)
    st = v.stats()
    if st['fw_ok'] != '1' or v.dialog() != 1:
        fail('fw %s: %s' % (tag, st))
    if run('mcopy', '-n', '-i', v.dump(1), '::dm40fw.bin', '-') != open(FW1, 'rb').read():
        fail('fw %s: read back differs' % tag)
    v.ua()
    v.answer(False)
    v.idle(1000)
    check_spi(v, ALL, 'fw %s declined' % tag)
    vol = v.dump(2)
    if 'DM40FW.BIN' in {k.upper() for k in ls(vol)} or not v.ua():
        fail('fw %s: the declined file still shown / no media change' % tag)
    fsck_clean(vol, 'fw %s declined volume' % tag)
    v.close()
    ok('firmware, %s order%s: checked, dialog; No -> DM40FW.BIN deleted' % (tag, ', Allow after write %d' % allow_at if allow_at else ''))

v, spi, base, host = fresh('fw-eject')
v.lock(True)
mtools(host, ('mcopy', FW1, '::dm40fw.bin'))
apply(v, base, host, 'asc')
v.lock(False)
v._op(b'E')
v._1()  # no main-loop pass since the last write
v.idle(3000)
if v.stats()['fw_ok'] != '1' or v.dialog() != 1:
    fail('cp + eject: %s' % v.stats())
v.answer(False)
v.idle(1000)
check_spi(v, ALL, 'cp + eject')
v.close()
ok('cp + eject at once: the check runs before the session ends, dialog')

print('--- firmware: Windows-like lazy write-back (entry size 0, data, FAT seconds later, the size last), an Allow between')
v, spi, base, host = fresh('fw-win')
h0 = snapshot(host, 'h0')
mtools(host, ('mcopy', '/dev/null', '::DM40FW.BIN'))  # the entry, no cluster yet
h1 = snapshot(host, 'h1')
mtools(host, ('mdel', '::DM40FW.BIN'), ('mcopy', FW1, '::DM40FW.BIN'))
apply(v, h0, h1)          # the entry
v.idle(2000)
v.lock(False)             # FASTFAT's Allow after its first metadata flush (inferred)
g = Geo(base)
d = writes(h1, host)
data = [w for w in d if g.kind(w[0]) == 'data']
fat = [w for w in d if g.kind(w[0]) == 'fat']
root = [w for w in d if g.kind(w[0]) == 'root']
for k, (lba, x) in enumerate(data):
    if v.write(lba, x) != W_OK:
        fail('win data')
    v.tick()
v.idle(4000)              # the lazy writer
for lba, x in fat:
    if v.write(lba, x) != W_OK:
        fail('win fat')
v.idle(3000)
if v.dialog() != 0:
    fail('win: dialog before the size')
for lba, x in root:
    if v.write(lba, x) != W_OK:
        fail('win root')
v.idle(1000)
if v.stats()['fw_ok'] != '1' or v.dialog() != 1:
    fail('win: %s' % v.stats())
v.answer(False)
v.idle(1000)
check_spi(v, ALL, 'win fw declined')
v.close()
ok('firmware, Windows-like lazy write-back with an Allow after the first flush: checked, dialog')

print('--- firmware while the meter adds a screenshot in the middle of the copy')
v, spi, base, host = fresh('fw-meter')
v.lock(True)
mtools(host, ('mcopy', FW1, '::dm40fw.bin'))
apply(v, base, host, 'data-first', split=(30, lambda: meter_add(v, spi, 'SHOTS', 'S050    BMP', BMP)))
v.idle(1000)
if v.stats()['fw_ok'] != '1' or v.dialog() != 1:
    fail('fw-meter: %s' % v.stats())
vol = v.dump(1)
if 'S050.BMP' not in ls(vol, '::SHOTS') or run('mcopy', '-n', '-i', vol, '::SHOTS/S050.BMP', '-') != BMP:
    fail('fw-meter: the new screenshot is not on the volume')
if not v.ua():
    fail('fw-meter: no media change raised')
fsck_clean(vol, 'fw-meter volume')
v.answer(False)
v.idle(1000)
check_spi(v, ALL | {'::/SHOTS/S050.BMP'}, 'fw-meter')
v.close()
ok('firmware copy with a meter screenshot in the middle: both there, fsck clean')

print('--- deletes with the meter adding files before the host writes back (stale sectors)')
cases = [
    ('one', ['::SHOTS/S001.BMP'], ['::/SHOTS/S001.BMP'], 'asc', 0),
    ('three', ['::SHOTS/S000.BMP', '::LOGS/LOG001.CSV', '::SCOPE/SCOP000.CSV'], ['::/SHOTS/S000.BMP', '::/LOGS/LOG001.DLG', '::/SCOPE/SCOP000.DLG'], 'random', 0),
    ('three-meta', ['::SHOTS/S002.BMP', '::SHOTS/S000.BMP', '::LOGS/LOG000.CSV'], ['::/SHOTS/S002.BMP', '::/SHOTS/S000.BMP', '::/LOGS/LOG000.DLG'], 'meta-first', 0),
    ('lazy-fat', ['::SHOTS/S001.BMP', '::LOGS/LOG000.CSV'], ['::/SHOTS/S001.BMP', '::/LOGS/LOG000.DLG'], 'asc', 5000),
]
for tag, dels, gone, order, lazy in cases:
    v, spi, base, host = fresh('del-' + tag)
    v.lock(True)
    # The meter adds a screenshot and a log after the host read the volume:
    # the host's copies of SHOTS/ and LOGS/ and of the FAT are stale.
    meter_add(v, spi, 'SHOTS', 'S060    BMP', BMP)
    lg = open(os.path.join(WORK, 'l.dlg'), 'wb')
    lg.write(run('mcopy', '-n', '-i', SPI0, '::LOGS/LOG001.DLG', '-'))
    lg.close()
    meter_add(v, spi, 'LOGS', 'LOG061  DLG', open(os.path.join(WORK, 'l.dlg'), 'rb').read())
    new = {'::/SHOTS/S060.BMP', '::/LOGS/LOG061.DLG'}
    mtools(host, *[('mdel', p) for p in dels])
    apply(v, base, host, order, fat_after_ms=lazy)
    v.idle(2000)
    st = v.stats()
    if st['deleted'] != str(len(dels)) or st['refused'] != '0':
        fail('%s: %s' % (tag, st))
    check_spi(v, (ALL - set(gone)) | new, 'delete ' + tag)
    vol = v.dump(1)
    sh = ls(vol, '::SHOTS')
    if 'S060.BMP' not in sh or 'LOG061.CSV' not in ls(vol, '::LOGS'):
        fail('%s: the new files are not shown: %s' % (tag, sh))
    fsck_clean(vol, 'delete %s volume' % tag)
    v.close()
    ok('delete %s (%s%s): %d deleted, the 2 meter files added meanwhile kept and shown, st dropped=%s' % (tag, order, ', FAT %d ms later' % lazy if lazy else '', len(dels), st['dropped']))

print('--- a host file over the slot of a meter file the host has not seen, then deleted')
v, spi, base, host = fresh('slot')
v.lock(True)
meter_add(v, spi, 'SHOTS', 'S070    BMP', BMP)   # slot 5, after S000..S002 (and S007)
junk = os.path.join(WORK, 'junk.txt')
open(junk, 'wb').write(b'host file\n' * 300)
h0 = snapshot(host, 'h0')
mtools(host, ('mcopy', junk, '::SHOTS/JUNK.TXT'), ('mcopy', junk, '::SHOTS/A long host name.txt'))
apply(v, h0, host)
h1 = snapshot(host, 'h1')
mtools(host, ('mdel', '::SHOTS/JUNK.TXT'), ('mdel', '::SHOTS/A long host name.txt'), ('mdel', '::SHOTS/S002.BMP'))
apply(v, h1, host, 'random', seed=7)
v.idle(7000)  # the staging of the host files owns the SPI disk until it is parked (PARK_MS)
st = v.stats()
if st['deleted'] != '1':
    fail('slot: %s' % st)
check_spi(v, (ALL - {'::/SHOTS/S002.BMP'}) | {'::/SHOTS/S070.BMP'}, 'slot')
v.idle(500)
meter_add(v, spi, 'SHOTS', 'S071    BMP', BMP)
vol = v.dump(1)
sh = ls(vol, '::SHOTS')
if 'S070.BMP' not in sh or 'S071.BMP' not in sh or 'JUNK.TXT' in sh:
    fail('slot: %s' % sh)
fsck_clean(vol, 'slot volume')
v.close()
ok('host files written over an unseen meter file\'s slot and deleted again: the meter file kept, S002 deleted, the next meter file after the host\'s slots')

print('--- a delete on the meter while connected (tombstone), then a new file of the same name')
v, spi, base, host = fresh('tomb')
v.lock(True)
meter_delete(v, b'S001    BMP')
vol = v.dump(1)
if 'S001.BMP' in ls(vol, '::SHOTS'):
    fail('tomb: S001 still shown')
free, bad = fat_counts(vol)
if free != HOST_CL:
    fail('tomb: its clusters are free (%d)' % free)
fsck_clean(vol, 'tomb volume')
# The host deletes S001 from its stale copy, and S000: only S000 goes.
mtools(host, ('mdel', '::SHOTS/S001.BMP'), ('mdel', '::SHOTS/S000.BMP'))
h1 = snapshot(host, 'h1')
apply(v, base, host)
v.idle(2000)
check_spi(v, ALL - {'::/SHOTS/S001.BMP', '::/SHOTS/S000.BMP'}, 'tomb')
meter_add(v, spi, 'SHOTS', 'S001    BMP', BMP)
vol = v.dump(2)
if 'S001.BMP' not in ls(vol, '::SHOTS'):
    fail('tomb: the new S001 not shown')
# The host's stale copy deletes "S001" once more: the new one stays.
mtools(host, ('mcopy', junk, '::SHOTS/X.TXT'), ('mdel', '::SHOTS/X.TXT'))
apply(v, h1, host)
v.idle(2000)
check_spi(v, (ALL - {'::/SHOTS/S000.BMP'}), 'tomb new S001')
fsck_clean(v.dump(3), 'tomb volume 2')
v.close()
ok('meter delete while connected: shown deleted, clusters stay used; the host\'s stale delete of it changes nothing; a new S001 gets a new slot and survives')

print('--- the host\'s own files and folders: kept for the session, gone after the eject')
v, spi, base, host = fresh('junk')
v.lock(True)
mtools(host, ('mcopy', junk, '::NOTES.TXT'), ('mmd', '::New folder'), ('mcopy', junk, '::New folder/a.txt'),
       ('mmd', '::System Volume Information'), ('mcopy', junk, '::System Volume Information/IndexerVolumeGuid'),
       ('mcopy', junk, '::SHOTS/desktop.ini'), ('mcopy', junk, '::INFO/x.txt'), ('mdel', '::README.TXT'))
apply(v, base, host, 'random', seed=11)
v.idle(7000)
st = v.stats()
if st['refused'] != '0' or st['deleted'] != '0':
    fail('junk: %s' % st)
vol = v.dump(1)
nm = names(vol)
if '::/NOTES.TXT' not in nm or '::/System Volume Information/IndexerVolumeGuid' not in nm:
    fail('junk: the host\'s root files not kept for the session: %s' % sorted(nm)[:20])
if run('mcopy', '-n', '-i', vol, '::NOTES.TXT', '-') != open(junk, 'rb').read():
    fail('junk: NOTES.TXT reads back wrong')
check_spi(v, ALL, 'junk session')
eject(v)
v.bus_reset()
v.idle(2000)
vol = v.dump(2)
nm = names(vol)
extra = {p for p in nm if 'NOTES' in p or 'folder' in p or 'System Volume' in p or 'desktop' in p.lower() or 'x.txt' in p.lower()}
if extra or '::/README.TXT' not in nm:
    fail('junk survived: %s' % extra)
fsck_clean(vol, 'junk after')
check_spi(v, ALL, 'junk after')
if any('_VHOST' in p for p in names(v.save())):
    fail('junk: temp files left')
v.close()
ok('host files in the root / a new folder / System Volume Information / SHOTS / INFO and a README delete: no refusal, root ones read back in the session, all gone after eject + re-plug, SPI unchanged')

print('--- Linux trash: deleted at the eject; restored: kept')
for tag, restore in (('trash', False), ('trash-restored', True)):
    v, spi, base, host = fresh(tag)
    v.lock(True)
    mtools(host, ('mmd', '::.Trash-1000'), ('mmd', '::.Trash-1000/files'), ('mmd', '::.Trash-1000/info'),
           ('mcopy', junk, '::.Trash-1000/info/S001.BMP.trashinfo'),
           ('mmove', '::SHOTS/S001.BMP', '::.Trash-1000/files/S001.BMP'))
    apply(v, base, host, 'random', seed=3)
    meter_add(v, spi, 'SHOTS', 'S080    BMP', BMP)
    if restore:
        h1 = snapshot(host, 'h1')
        mtools(host, ('mmove', '::.Trash-1000/files/S001.BMP', '::SHOTS/S001.BMP'), ('mdel', '::.Trash-1000/info/S001.BMP.trashinfo'))
        apply(v, h1, host)
    v.idle(7000)
    if v.stats()['deleted'] != '0':
        fail('%s: deleted before the eject' % tag)
    eject(v)
    want = ALL | {'::/SHOTS/S080.BMP'}
    if not restore:
        want -= {'::/SHOTS/S001.BMP'}
    check_spi(v, want, tag)
    v.bus_reset()
    v.idle(2000)
    vol = v.dump(2)
    if any('trash' in p.lower() for p in names(vol)):
        fail('%s: the trash survived' % tag)
    if ('S001.BMP' in ls(vol, '::SHOTS')) != restore:
        fail('%s: S001 shown %s' % (tag, not restore))
    fsck_clean(vol, tag + ' after')
    v.close()
    ok('%s: %s' % (tag, 'restored before the eject: kept' if restore else 'moved to .Trash-1000: deleted at the eject, nothing else'))

print('--- bus reset: renumbered, tombstones gone')
v, spi, base, host = fresh('reset')
meter_delete(v, b'S000    BMP')
meter_add(v, spi, 'SHOTS', 'S090    BMP', BMP)
v.bus_reset()
v.idle(2000)
vol = v.dump(1)
fsck_clean(vol, 'reset volume')
sh = ls(vol, '::SHOTS')
if 'S000.BMP' in sh or 'S090.BMP' not in sh:
    fail('reset: %s' % sh)
raw = open(vol, 'rb')
g = Geo(vol)
st = v.stats()
v.close()
ok('bus reset: compacted (fsck clean), the deleted file gone, the new one there (%s files)' % st['files'])

cleanup()
print('vone: %d checks PASS' % NOK)
