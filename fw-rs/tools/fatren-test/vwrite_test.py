#!/usr/bin/env python3
"""Host test of src/vwrite.rs (host writes on the generated USB volume):
    vwrite_test.py <t> <flash1m> <flash1m-nofw> <srcdir> <spi.img> <fw.bin> <fw2.bin> <work> [captures...]
<t> = the fatren-test binary (vserve), <spi.img> = a SPI disk with logs,
scope records and screenshots, <fw.bin> / <fw2.bin> = two update files.

Every "host" operation is made by mtools on a copy of the volume dump (what
a host would write: the sector diff), its sectors are written through
vserve in several orders, then: what vserve accepts / refuses, the volume
dumped again (= the host's image except free clusters' data), the SPI image
(fsck.fat clean, the right files gone / added, everything else intact),
then the session's end (Allow / bus reset) and the regenerated volume.
Scenarios:
  delete    LOGS / SHOTS / SCOPE files (mdel): accepted, SPI files deleted
            after both marks, nothing else touched; FAT-first / dir-first /
            random orders; the FAT2 mirror; one mark only (dir 0xE5 = a
            move) deletes nothing; a delete while frozen + a gallery delete;
  firmware  mcopy of an update file as dm40fw.bin / DM40FW.BIN / a long
            name, sector orders data-first / metadata-first / random; the
            dialog; No deletes DM40FW.BIN, Yes asks to install; a delete then
            a copy into the freed clusters (non-contiguous chain); an
            overwrite with a second file; a DM40*.BIN that is no update file;
  refusals  a new file / directory, writing into README.TXT / INFO, a
            rename, an INFO delete, LFN-only junk: refused, nothing on the
            SPI disk changes, the volume after a bus reset = the original;
  lockless  no PREVENT: frozen while writing, released QUIET_MS later;
  cuts      a power cut before every SPI write of a firmware commit: fsck at
            worst lost clusters / FAT 1 ahead, DM40FW.BIN absent or whole;
  replay    captured kernel sequences (usbip_msc.py --capture): the same
            accept / refuse answers, the final checks of the capture.
"""
import os, random, shutil, struct, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vserve import VServe, diff, sectors, Geo, SECTOR, W_OK, W_REFUSED, W_BUSY

T, FLASH, FLASH0, SRC, SPI0, FW1, FW2, WORK = sys.argv[1:9]
CAPS = sys.argv[9:]
os.environ['MTOOLS_SKIP_CHECK'] = '1'
os.makedirs(WORK, exist_ok=True)
NOK = 0


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


def spi_copy(name):
    p = os.path.join(WORK, name)
    shutil.copy(SPI0, p)
    return p


def fsck_clean(img, what):
    r = subprocess.run(['fsck.fat', '-n', img], capture_output=True, text=True)
    out = r.stdout + r.stderr
    if r.returncode or any(k in out.lower() for k in ('free cluster', 'differ', 'lost', 'orphan', 'reclaim')):
        print(out)
        fail('fsck %s' % what)


def fsck_cut(img, what):
    """A power cut: fsck may report lost clusters / FAT 1 ahead only."""
    r = subprocess.run(['fsck.fat', '-n', img], capture_output=True, text=True)
    if r.returncode > 1:
        fail('fsck rc=%d %s' % (r.returncode, what))
    for l in (r.stdout + r.stderr).splitlines():
        if not l or l.startswith('fsck.fat') or 'files, ' in l or 'Leaving filesystem unchanged' in l \
                or 'FATs differ but appear to be intact' in l or 'Using first FAT' in l \
                or (l.startswith('Reclaimed ') and 'unused cluster' in l):
            continue
        print(r.stdout)
        fail('unexpected fsck finding (%s): %s' % (what, l))


def ls(img, d='::'):
    """{name: size} of a directory (mdir -b + sizes via mdir)."""
    out = run('mdir', '-a', '-i', img, d).decode(errors='replace')
    res = {}
    for l in out.splitlines():
        p = l.split()
        if len(p) >= 3 and p[2].isdigit() and not l.startswith(' '):
            res[(p[0] + '.' + p[1]) if len(p) >= 4 and not p[1].isdigit() else p[0]] = int(p[2])
    return res


def tree(img):
    """{path: md5} of every file of a FAT image (mcopy -s / mdir -/ -b)."""
    out = run('mdir', '-a', '-/', '-b', '-i', img, '::').decode(errors='replace').split()
    res = {}
    for p in out:
        if p.endswith('/'):
            continue
        r = subprocess.run(['mcopy', '-n', '-i', img, p, '-'], capture_output=True)
        if r.returncode == 0:
            import hashlib
            res[p] = hashlib.md5(r.stdout).hexdigest()
    return res


def same_tree(a, b, what, gone=(), new=()):
    ta, tb = tree(a), tree(b)
    for p in gone:
        if p in tb:
            fail('%s: %s still there' % (what, p))
        ta.pop(p, None)
    for p in new:
        if p not in tb:
            fail('%s: %s missing' % (what, p))
        tb.pop(p, None)
    if ta != tb:
        fail('%s: other files differ: %s' % (what, sorted(set(ta.items()) ^ set(tb.items()))[:6]))


def spiflash_range(img):
    """Clusters of DEBUG/SPIFLASH.BIN (raw SPI disk: changes under the host
    by design, docs/native-boot.md)."""
    g = Geo(img)
    with open(img, 'rb') as f:
        f.seek(g.root * 512)
        r = f.read(512 * 32)
        for i in range(0, len(r), 32):
            if r[i:i + 11] == b'DEBUG      ':
                c = struct.unpack_from('<H', r, i + 26)[0]
                f.seek((g.data + (c - 2) * g.spc) * 512)
                d = f.read(512)
                for k in range(0, 512, 32):
                    if d[k:k + 11] == b'SPIFLASHBIN':
                        s0 = struct.unpack_from('<H', d, k + 26)[0]
                        n = struct.unpack_from('<I', d, k + 28)[0]
                        return range(s0, s0 + (n + g.spc * 512 - 1) // (g.spc * 512))
    fail('no SPIFLASH.BIN')


def host_view_ok(host, vol, what):
    """The volume dump equals the host's image except data sectors of
    clusters free in the host's FAT (a host never reads them) and
    DEBUG/SPIFLASH.BIN."""
    g = Geo(host)
    fat = g.fat()
    skip = spiflash_range(host)
    bad = []
    for lba, x in diff(host, vol):
        if g.kind(lba) == 'data':
            c = g.cluster(lba)
            if fat[c] == 0 or c in skip:
                continue
        bad.append((lba, g.kind(lba)))
    if bad:
        fail('%s: the volume differs from the host image at %s' % (what, bad[:8]))


def apply(v, host_before, host_after, order='asc', expect=W_OK, seed=1):
    """Write host_after's sector diff through vserve in `order`; every
    answer must be `expect` (None: any). Returns the answers."""
    ws = diff(host_before, host_after)
    g = Geo(host_before)
    # A host writes every sector of the clusters it allocates (zeros too,
    # which a sector diff of the images does not show).
    fa, fb = g.fat(), Geo(host_after).fat()
    have = {l for l, _ in ws}
    z = bytes(SECTOR)
    for c in range(2, len(fb)):
        if fb[c] != 0 and fa[c] != fb[c] and (fa[c] == 0 or True) and fa[c] == 0:
            for q in range(g.spc):
                lba = g.data + (c - 2) * g.spc + q
                if lba not in have:
                    ws.append((lba, z))
    ws.sort()
    if order == 'desc':
        ws = ws[::-1]
    elif order == 'random':
        random.Random(seed).shuffle(ws)
    elif order == 'data-first':
        ws = [w for w in ws if g.kind(w[0]) == 'data'] + [w for w in ws if g.kind(w[0]) != 'data']
    elif order == 'meta-first':
        ws = [w for w in ws if g.kind(w[0]) != 'data'] + [w for w in ws if g.kind(w[0]) == 'data']
    res = []
    for lba, x in ws:
        r = v.write(lba, x)
        res.append((lba, g.kind(lba), r))
        if expect is not None and r != expect:
            fail('write %d (%s): %d, expected %d' % (lba, g.kind(lba), r, expect))
        v.tick()
    return res


def cleanup():
    """The previous scenario's images (each volume dump holds ~20 MB of
    data: SPIFLASH.BIN)."""
    for f in os.listdir(WORK):
        if any(k in f for k in ('.vol', '.host', '.spi', '.boot')):
            os.remove(os.path.join(WORK, f))


def fresh(name, cut=None):
    cleanup()
    spi = spi_copy(name + '.spi')
    v = VServe(T, spi, FLASH, SRC, cut=cut)
    v.tick(100)
    base = v.dump(0)
    host = os.path.join(WORK, name + '.host')
    run('cp', '--sparse=always', base, host)
    return v, spi, base, host


def settle(v, ms=2000):
    v.idle(ms)


def release(v, how='allow'):
    if how == 'allow':
        v.lock(False)
    else:
        v.bus_reset()
    settle(v)


print('=== vwrite: host writes on the USB volume')
v, spi, base, host = fresh('wp')
if v.wp():
    fail('volume write-protected')
ok('volume writable (no WP bit), %s' % v.stats()['files'] + ' generated files')
v.close()

print('--- mode (decision 2026-10-22 #6): read-only "Files live" by default, "PC can edit" at the next bus reset')
cleanup()
spi = spi_copy('ro.spi')
v = VServe(T, spi, FLASH, SRC, ro=True)
v.tick(100)
base = v.dump(0)
host = os.path.join(WORK, 'ro.host')
run('cp', '--sparse=always', base, host)
if not v.wp():
    fail('read-only: no WP bit')
v.lock(True)
mtools(host, ('mdel', '::LOGS/LOG001.CSV'))
ws = diff(base, host)
res = [v.write(lba, x) for lba, x in ws]
if not ws or any(r != W_REFUSED for r in res):
    fail('read-only: writes not all refused (07/27/00): %s' % res)
settle(v)
st = v.stats()
if st['frozen'] != 'false' or st['deleted'] != '0' or st['rec'] != '0' or st['pairs'] != '0':
    fail('read-only: %s' % st)
same_tree(SPI0, v.save(), 'read-only writes')
# A delete on the meter while the host holds the volume locked: the volume
# changes and the UNIT ATTENTION goes out at once (not at the Allow).
if not v.gallery_delete(b'LOG001  DLG'):
    fail('read-only: gallery delete')
v.idle(1500)
if not v.ua() or 'LOG001.CSV' in ls(v.dump(1), '::LOGS'):
    fail('read-only: no UNIT ATTENTION / LOG001 still listed while locked')
# Asking for "PC can edit" while mounted changes nothing until the bus reset.
v.mode(False)
if not v.wp():
    fail('mode changed under the host')
v.bus_reset()
settle(v)
if v.wp():
    fail('writable mode not in force after the bus reset')
v.mode(True)
v.bus_reset()
if not v.wp():
    fail('read-only mode not in force after the bus reset')
v.close()
ok('read-only: WP bit, %d writes of a delete refused, nothing staged, SPI unchanged; a meter-side delete while locked: UNIT ATTENTION at once; a mode asked for applies at the next bus reset only' % len(ws))

# A writable session with a delete half done, then "Files live" asked for:
# the session stays frozen and writable until the bus reset, then the
# volume is read-only and regenerated (nothing of the session survives).
v, spi, base, host = fresh('rw-ro')
v.lock(True)
mtools(host, ('mdel', '::LOGS/LOG001.CSV'))
g = Geo(base)
for lba, x in diff(base, host):
    if g.kind(lba) == 'data':
        if v.write(lba, x) != W_OK:
            fail('rw-ro: dir write')
v.mode(True)
settle(v)
if v.wp() or v.stats()['frozen'] != 'true' or v.ua():
    fail('rw-ro: the session changed before the bus reset: %s' % v.stats())
v.bus_reset()
settle(v)
if not v.wp() or v.stats()['rec'] != '0' or 'LOG001.CSV' not in ls(v.dump(1), '::LOGS'):
    fail('rw-ro: after the bus reset: %s' % v.stats())
sp = v.save()
fsck_clean(sp, 'rw-ro')
same_tree(SPI0, sp, 'rw-ro')
v.close()
ok('writable session + read-only asked: frozen and writable until the bus reset, then read-only, the half delete dropped, SPI unchanged')

print('--- delete (mdel): LOGS / SHOTS / SCOPE files, every order')
for order in ('asc', 'desc', 'random', 'data-first', 'meta-first'):
    v, spi, base, host = fresh('del-' + order)
    v.lock(True)
    mtools(host, ('mdel', '::LOGS/LOG001.CSV'), ('mdel', '::SHOTS/S001.BMP'), ('mdel', '::SCOPE/SCOP000.CSV'))
    apply(v, base, host, order)
    settle(v)
    st = v.stats()
    if st['deleted'] != '3' or st['refused'] != '0':
        fail('%s: %s' % (order, st))
    vol = v.dump(1)
    host_view_ok(host, vol, 'delete ' + order)
    sp = v.save()
    fsck_clean(sp, 'delete ' + order)
    same_tree(SPI0, sp, 'delete ' + order, gone=['::/LOGS/LOG001.DLG', '::/SHOTS/S001.BMP', '::/SCOPE/SCOP000.DLG'])
    if v.ua():
        fail('a UNIT ATTENTION while the host holds the volume')
    release(v)
    if not v.ua():
        fail('no UNIT ATTENTION after the Allow')
    vol2 = v.dump(2)
    lg = ls(vol2, '::LOGS')
    if 'LOG001.CSV' in lg or 'LOG000.CSV' not in lg or 'S001.BMP' in ls(vol2, '::SHOTS'):
        fail('regenerated volume: %s' % lg)
    fsck_clean(vol2, 'regenerated volume')
    v.close()
    ok('delete, order %s: 3 SPI files deleted (and only them), SPI fsck clean, the volume = the host image until the Allow, then regenerated' % order)

print('--- the root legacy files (LOGS/LOG005.CSV from the root LOG005.DLG, SHOTS/S007.BMP)')
v, spi, base, host = fresh('del-legacy')
v.lock(True)
mtools(host, ('mdel', '::LOGS/LOG005.CSV'), ('mdel', '::SHOTS/S007.BMP'))
apply(v, base, host, 'random', seed=5)
settle(v)
sp = v.save()
fsck_clean(sp, 'legacy delete')
same_tree(SPI0, sp, 'legacy delete', gone=['::/LOG005.DLG', '::/S007.BMP'])
v.close()
ok('root files of earlier builds deleted the same way')

print('--- only one mark: the entry 0xE5 alone (a move to a trash directory) / the chain alone')
v, spi, base, host = fresh('onemark')
v.lock(True)
mtools(host, ('mdel', '::LOGS/LOG001.CSV'))
g = Geo(base)
ws = diff(base, host)
for lba, x in ws:
    if g.kind(lba) == 'data':
        if v.write(lba, x) != W_OK:
            fail('dir write')
settle(v)
if v.stats()['deleted'] != '0':
    fail('deleted on the 0xE5 alone')
release(v)
vol2 = v.dump(2)
if 'LOG001.CSV' not in ls(vol2, '::LOGS'):
    fail('LOG001 not back after the session')
sp = v.save()
same_tree(SPI0, sp, 'one mark')
v.close()
v, spi, base, host = fresh('onemark2')
v.lock(True)
for lba, x in ws:
    if g.kind(lba) == 'fat' and v.write(lba, x) != W_OK:
        fail('fat write')
settle(v)
if v.stats()['deleted'] != '0':
    fail('deleted on the FAT alone')
release(v, 'reset')
sp = v.save()
same_tree(SPI0, sp, 'one mark (FAT)')
if 'LOG001.CSV' not in ls(v.dump(3), '::LOGS'):
    fail('LOG001 not back after the bus reset')
v.close()
ok('one mark only deletes nothing; the file is back after the session')

print('--- frozen: a delete on the meter while the host holds the volume, a host delete of it')
v, spi, base, host = fresh('frozen')
v.lock(True)
if not v.gallery_delete(b'LOG001  DLG'):
    fail('gallery delete')
settle(v)
if v.ua():
    fail('UNIT ATTENTION while frozen')
vol = v.dump(1)
lg = ls(vol, '::LOGS')
if 'LOG001.CSV' not in lg:
    fail('LOG001 vanished under the host')
if any(run('mcopy', '-n', '-i', vol, '::LOGS/LOG001.CSV', '-')):
    fail('LOG001 data not zeros after the delete on the meter')
for lba, x in diff(base, vol):
    if Geo(base).kind(lba) != 'data':
        fail('metadata changed under the host at %d' % lba)
mtools(host, ('mdel', '::LOGS/LOG001.CSV'))
apply(v, base, host)
settle(v)
if v.stats()['deleted'] != '0':
    fail('frozen: deleted twice')
sp = v.save()
fsck_clean(sp, 'frozen')
same_tree(SPI0, sp, 'frozen', gone=['::/LOGS/LOG001.DLG'])
release(v)
if not v.ua() or 'LOG001.CSV' in ls(v.dump(2), '::LOGS'):
    fail('frozen: not regenerated')
v.close()
ok('a delete on the meter while frozen: the entry stays (data zeros), no UA; the host deletes it too; regenerated at the Allow')

print('--- firmware: mcopy of an update file, names and orders')
for name, order in (('dm40fw.bin', 'asc'), ('DM40FW.BIN', 'data-first'), ('dm40_v_2_7.bin', 'meta-first'), ('dm40fw.bin', 'random'), ('Dm40-Update.BIN', 'desc')):
    tag = 'fw-%s-%s' % (name.replace('.', '_'), order)
    v, spi, base, host = fresh(tag)
    v.lock(True)
    mtools(host, ('mcopy', FW1, '::' + name))
    apply(v, base, host, order)
    settle(v)
    st = v.stats()
    if st['fw_ok'] != '1' or st['refused'] != '0':
        fail('%s: %s' % (tag, st))
    if v.dialog() != 1:
        fail('%s: no dialog' % tag)
    vol = v.dump(1)
    host_view_ok(host, vol, tag)
    if run('mcopy', '-n', '-i', vol, '::' + name, '-') != open(FW1, 'rb').read():
        fail('%s: the host reads back another file' % tag)
    sp = v.save()
    fsck_clean(sp, tag)
    if run('mcopy', '-n', '-i', sp, '::DM40FW.BIN', '-') != open(FW1, 'rb').read():
        fail('%s: DM40FW.BIN on the SPI disk differs' % tag)
    same_tree(SPI0, sp, tag, new=['::/DM40FW.BIN'])
    if order == 'random':
        v.answer(True)
        if not v.install():
            fail('Yes: no install')
        ok('%s: Yes -> install (DBG:UPDATE)' % tag)
    else:
        v.answer(False)
        settle(v)
        sp = v.save()
        fsck_clean(sp, tag + ' declined')
        same_tree(SPI0, sp, tag + ' declined')
    release(v)
    fsck_clean(v.dump(2), tag + ' regenerated')
    v.close()
    ok('firmware %s (%s): staged, checked, DM40FW.BIN == the file on the SPI disk, dialog; host view exact' % (name, order))

print('--- a delete, then a copy into the freed clusters (non-contiguous chain)')
v, spi, base, host = fresh('fw-holes')
v.lock(True)
mtools(host, ('mdel', '::SHOTS/S000.BMP'), ('mdel', '::LOGS/LOG000.CSV'))
apply(v, base, host, 'asc')
settle(v)
h2 = host + '2'
run('cp', '--sparse=always', host, h2)
mtools(h2, ('mcopy', FW1, '::dm40fw.bin'))
apply(v, host, h2, 'random', seed=7)
settle(v)
st = v.stats()
if st['fw_ok'] != '1' or st['deleted'] != '2':
    fail('holes: %s' % st)
sp = v.save()
fsck_clean(sp, 'holes')
same_tree(SPI0, sp, 'holes', gone=['::/SHOTS/S000.BMP', '::/LOGS/LOG000.DLG'], new=['::/DM40FW.BIN'])
host_view_ok(h2, v.dump(1), 'holes')
fat = Geo(h2).fat()
ch = []
with open(h2, 'rb') as f:
    f.seek(Geo(h2).root * 512)
    r = f.read(512 * 32)
c = [struct.unpack_from('<H', r, i + 26)[0] for i in range(0, len(r), 32) if r[i:i + 8] == b'DM40FW  '][0]
while c < 0xFFF8:
    ch.append(c)
    c = fat[c]
jumps = sum(1 for a, b in zip(ch, ch[1:]) if b != a + 1)
if jumps == 0:
    fail('holes: the chain is contiguous (%s)' % ch)
ok('delete + copy into the freed clusters (chain %s, %d jumps): both deletes done, DM40FW.BIN whole' % (ch[:4] + ['...'], jumps))
print('--- overwrite with a second update file (mcopy -o), then No')
h3 = host + '3'
run('cp', '--sparse=always', h2, h3)
run('mcopy', '-o', '-i', h3, FW2, '::dm40fw.bin')
v.dialog()
apply(v, h2, h3, 'asc', expect=None)
settle(v)
st = v.stats()
if st['fw_ok'] != '2':
    fail('overwrite: %s' % st)
sp = v.save()
fsck_clean(sp, 'overwrite')
if run('mcopy', '-n', '-i', sp, '::DM40FW.BIN', '-') != open(FW2, 'rb').read():
    fail('overwrite: DM40FW.BIN is not the second file')
v.close()
ok('overwrite: the second file committed as DM40FW.BIN')

print('--- a DM40*.BIN that is no update file')
v, spi, base, host = fresh('fw-bad')
v.lock(True)
junk = os.path.join(WORK, 'junk.bin')
open(junk, 'wb').write(random.Random(3).randbytes(200000))
mtools(host, ('mcopy', junk, '::dm40junk.bin'))
apply(v, base, host, 'asc')
settle(v)
st = v.stats()
if st['fw_bad'] != '1' or st['fw_ok'] != '0' or v.dialog() != 2:
    fail('junk: %s' % st)
sp = v.save()
fsck_clean(sp, 'junk')
same_tree(SPI0, sp, 'junk')
v.close()
ok('a DM40*.BIN with no valid update header: rejected (toast), nothing on the SPI disk')

print('--- refusals')
for tag, ops in (
        ('newfile', [('mcopy', junk, '::NOTES.TXT')]),
        ('newdir', [('mmd', '::NEWDIR')]),
        ('mkdir-logs', [('mmd', '::LOGS/SUB')]),
        ('file-in-logs', [('mcopy', junk, '::LOGS/X.CSV')]),
        ('readme', [('mcopy', '-o', junk, '::README.TXT')]),
        ('info-del', [('mdel', '::INFO/CALIB.BIN')]),
        ('rename', [('mren', '::LOGS/LOG000.CSV', '::LOGS/RENAMED.CSV')]),
        ('label', [('mlabel', '::NEWLABEL')])):
    v, spi, base, host = fresh('ref-' + tag)
    v.lock(True)
    for op in ops:
        run(op[0], '-i', host, *op[1:])
    res = apply(v, base, host, 'asc', expect=None)
    if not any(r == W_REFUSED for _, _, r in res):
        fail('%s: nothing refused: %s' % (tag, res))
    v.idle(7000)  # the staging is parked (a temp file at most)
    sp = v.save()
    fsck_clean(sp, tag + ' parked')
    t = tree(sp)
    extra = [p for p in t if p.startswith('::/_VHOST')]
    for p in extra:
        t.pop(p)
    if t != tree(SPI0):
        fail('%s: SPI files changed' % tag)
    if v.stats()['staging'] != 'false':
        fail('%s: staging still active' % tag)
    release(v, 'reset')
    sp = v.save()
    fsck_clean(sp, tag)
    same_tree(SPI0, sp, tag)
    after = v.dump(2)
    g = Geo(base)
    skip = spiflash_range(base)
    d = [l for l, _ in diff(base, after) if not (g.kind(l) == 'data' and g.cluster(l) in skip)]
    if d:
        fail('%s: volume after the bus reset differs from the original at %s' % (tag, d[:5]))
    v.close()
    ok('%s refused (%d of %d writes), SPI files unchanged (%d temp file(s) until the session ends), volume regenerated as before' % (tag, sum(r == W_REFUSED for _, _, r in res), len(res), len(extra)))

print('--- the hosts\' own directories: a trash (accepted for the session, parked, gone after it)')
def trash_case(tag, ops, gone, expect_deleted_now):
    v, spi, base, host = fresh(tag)
    v.lock(True)
    for op in ops:
        run(op[0], '-i', host, *op[1:])
    apply(v, base, host, 'random', seed=3)
    v.idle(7000)
    st = v.stats()
    if st['refused'] != '0':
        fail('%s: refused: %s' % (tag, st))
    vol = v.dump(1)
    host_view_ok(host, vol, tag)
    sp = v.save()
    fsck_clean(sp, tag + ' parked')
    if st['deleted'] != expect_deleted_now:
        fail('%s: deleted %s during the session' % (tag, st['deleted']))
    release(v)
    sp = v.save()
    fsck_clean(sp, tag + ' released')
    same_tree(SPI0, sp, tag, gone=gone)
    vol2 = v.dump(2)
    fsck_clean(vol2, tag + ' regenerated')
    if any(n.lower().startswith('.trash') for n in ls(vol2)):
        fail('%s: the trash survived the session' % tag)
    v.close()
    return st

trash_case('trash-move', [('mmd', '::.Trash-1000'), ('mmd', '::.Trash-1000/files'), ('mmd', '::.Trash-1000/info'),
                          ('mcopy', junk, '::.Trash-1000/info/S001.BMP.trashinfo'),
                          ('mmove', '::SHOTS/S001.BMP', '::.Trash-1000/files/S001.BMP')], ['::/SHOTS/S001.BMP'], '0')
ok('a move to .Trash-1000: the trash kept for the session (temp file), S001 deleted from the SPI disk at the Allow, nothing else')
trash_case('trash-empty', [('mmd', '::.Trash-1000'), ('mmd', '::.Trash-1000/files'),
                           ('mmove', '::LOGS/LOG001.CSV', '::.Trash-1000/files/LOG001.CSV'), ('mdel', '::.Trash-1000/files/LOG001.CSV')], ['::/LOGS/LOG001.DLG'], '1')
ok('a move to the trash, then the trash emptied: deleted at once')
trash_case('trash-unrelated', [('mmd', '::.Trash-1000'), ('mmd', '::.Trash-1000/files'), ('mcopy', junk, '::.Trash-1000/files/OTHER.BIN')], [], '0')
ok('a trash with an unrelated file: nothing of ours deleted')
v, spi, base, host = fresh('trash-restored')
v.lock(True)
mtools(host, ('mmd', '::.Trash-1000'), ('mmd', '::.Trash-1000/files'), ('mmove', '::LOGS/LOG001.CSV', '::.Trash-1000/files/LOG001.CSV'))
apply(v, base, host)
h2 = host + '2'
run('cp', '--sparse=always', host, h2)
mtools(h2, ('mdel', '::.Trash-1000/files/LOG001.CSV'))
# Restored by the host = its entry back in LOGS (refused) and gone from the
# trash: only the trash part of it reaches us here.
g = Geo(base)
for lba, x in diff(host, h2):
    if g.kind(lba) == 'data':
        v.write(lba, x)
v.idle(7000)
release(v)
sp = v.save()
same_tree(SPI0, sp, 'trash-restored')
v.close()
ok('a file taken out of the trash again (the entry in files/ deleted, chain kept): not deleted')
v, spi, base, host = fresh('boot-purge')
v.close()
mtools(spi, ('mcopy', junk, '::_VHOST05.TMP'))
v = VServe(T, spi, FLASH, SRC)
v.idle(500)
sp = v.save()
fsck_clean(sp, 'boot purge')
same_tree(SPI0, sp, 'boot purge')
v.close()
ok('a temp file left by an earlier session (power cut) deleted at the first main-loop pass')

print('--- a lockless host (no PREVENT): frozen while it writes, released QUIET_MS later')
v, spi, base, host = fresh('lockless')
mtools(host, ('mdel', '::LOGS/LOG001.CSV'))
apply(v, base, host)
settle(v)
if v.stats()['frozen'] != 'true' or v.ua():
    fail('lockless: not frozen')
v.idle(61000, step=1000)
if v.stats()['frozen'] != 'false' or not v.ua():
    fail('lockless: not released: %s' % v.stats())
if 'LOG001.CSV' in ls(v.dump(1), '::LOGS'):
    fail('lockless: LOG001 still shown')
v.close()
ok('lockless host: delete done, released 60 s after its last write with a UNIT ATTENTION')

print('--- power cuts: before every SPI write of a firmware copy (staging + commit)')
v, spi, base, host = fresh('cut-probe')
v.lock(True)
mtools(host, ('mcopy', FW1, '::dm40fw.bin'))
apply(v, base, host, 'random', seed=11)
settle(v)
nw = int(v.stats()['spi_writes'])
v.close()
ws = diff(base, host)
# Every cut of the commit (the last SPI writes: the zeroed tail, the old
# update files' delete, the FAT chain, the root entry), every 25th of the
# staging before it.
pts = sorted(set(range(0, nw, 25)) | set(range(max(0, nw - 40), nw)))
for k in pts:
    vv = VServe(T, spi_copy('cut.spi'), FLASH, SRC, cut=k)
    vv.lock(True)
    rnd = list(ws)
    random.Random(11).shuffle(rnd)
    for lba, x in rnd:
        vv.write(lba, x)
        vv.tick()
    vv.idle(2000)
    sp = vv.save()
    fsck_cut(sp, 'cut %d' % k)
    t = tree(sp)
    if '::/DM40FW.BIN' in t:
        import hashlib
        if t['::/DM40FW.BIN'] != hashlib.md5(open(FW1, 'rb').read()).hexdigest():
            fail('cut %d: a partial DM40FW.BIN' % k)
    t.pop('::/DM40FW.BIN', None)
    t0 = tree(SPI0)
    if t != t0:
        fail('cut %d: other files changed' % k)
    vv.close()
ok('%d of %d cut points: fsck at worst lost clusters / FAT 1 ahead, DM40FW.BIN absent or whole, every other file intact' % (len(pts), nw))

print('--- power cuts: a trash session (staging, park, purge, the delete at the Allow), then a boot')
tops = [('mmd', '::.Trash-1000'), ('mmd', '::.Trash-1000/files'), ('mcopy', junk, '::.Trash-1000/files/x.trashinfo'),
        ('mmove', '::SHOTS/S001.BMP', '::.Trash-1000/files/S001.BMP')]
v, spi, base, host = fresh('tcut-probe')
v.close()
for op in tops:
    run(op[0], '-i', host, *op[1:])
tw = diff(base, host)
def trash_session(cut):
    vv = VServe(T, spi_copy('tcut.spi'), FLASH, SRC, cut=cut)
    vv.lock(True)
    vv.idle(300)
    for lba, x in tw:
        vv.write(lba, x)
        vv.tick()
    vv.idle(7000)
    vv.lock(False)
    vv.idle(3000)
    n = int(vv.stats()['spi_writes'])
    sp = vv.save()
    vv.close()
    return sp, n
sp, nt = trash_session(None)
same_tree(SPI0, sp, 'trash session', gone=['::/SHOTS/S001.BMP'])
for k in range(nt):
    sp, _ = trash_session(k)
    fsck_cut(sp, 'trash cut %d' % k)
    shutil.copy(sp, sp + '.boot')
    vb = VServe(T, sp + '.boot', FLASH, SRC)
    vb.idle(1000)
    sb = vb.save()
    vb.close()
    fsck_cut(sb, 'trash cut %d + boot' % k)
    t, t0 = tree(sb), tree(SPI0)
    if any(p.startswith('::/_VHOST') for p in t):
        fail('trash cut %d: a temp file survived the next boot' % k)
    t.pop('::/SHOTS/S001.BMP', None)
    t0.pop('::/SHOTS/S001.BMP', None)
    if t != t0:
        fail('trash cut %d: other files changed' % k)
ok('%d cut points of a trash session: fsck at worst lost clusters / FAT 1 ahead; after the next boot no temp file is left, S001 deleted or whole, nothing else changed' % nt)

for cap in CAPS:
    print('--- replay', cap)
    import vreplay
    vreplay.replay(cap, T, FLASH0, SRC, SPI0, WORK, FW1)
    ok('replay %s' % os.path.basename(cap))

print('vwrite: %d checks PASS' % NOK)
