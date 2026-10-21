#!/bin/bash
# Host test of src/gallery.rs (screenshot + log gallery, FR-16): builds the
# real gallery / csvlog / FAT / font / LCD code for the host (--cfg test:
# draw-call probes on) against the LCD emulator of tools/ui-render/host.rs,
# makes DM40-geometry FAT16 images (mkfs.fat / mtools, as tools/fatren-test)
# from gen.py's BMP / CSV files, then
#   1. decode: every BMP format full screen + thumbnail vs. its reference,
#      unsupported ones refused, CSV statistics vs. the generator's, on a
#      plain and on a fragmented disk (+ the stock RLE4 factory screenshots
#      from private/spi-disk-backup when present, reference by an
#      independent Python RLE4 decode);
#   2. lock: no disk read while busy, resume, delete refused while mounted,
#      delete (fsck clean after);
#   3. ui: the worst-case layout matrix (0 / 1 / 6 / 7 / 99 files, long
#      names, CSV + BMP mixed, unsupported BMP, disk busy, no disk, delete
#      confirm, info boxes): zero violations; PNG renders (2x) + contact sheet.
#   tools/gallery-test/run.sh [out dir]   (default: a temp dir, kept)
# Exit status 1 on any failure. Needs no hardware.
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
OUT=${1:-$(mktemp -d)}
mkdir -p "$OUT"
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
CARGO_PKG_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' ../../Cargo.toml | head -1) \
    rustc --edition 2021 -O --cfg test --cfg 'feature="embedded-fonts"' -A warnings main.rs -o "$W/t"
/usr/bin/python3 -I gen.py "$W/d" >/dev/null
export MTOOLS_SKIP_CHECK=1
mkimg() {
    dd if=/dev/zero of=$1 bs=1M count=16 status=none
    mkfs.fat -F 16 -S 4096 -s 1 -f 2 -r 512 -R 1 -n "ATK-DM40" $1 >/dev/null
}
mkdir -p "$W/img"
for s in e0 e1 e6 e7 e99 fmt; do
    I="$W/img/$s.img"
    mkimg $I
    echo "dummy" > $W/cfg.bin
    mcopy -i $I $W/cfg.bin ::CONFIG.BIN          # not a gallery file
    while read -r f; do
        [ -n "$f" ] && mcopy -i $I "$W/d/$f" "::$f"
    done < "$W/d/set-$s.txt"
done
# Stock factory screenshots (4 bpp RLE4) when this checkout has them.
STOCK=../../../private/spi-disk-backup/20261013-1401
if [ -f $STOCK/S000.BMP ]; then
    # Reference: an independent BI_RLE4 decode (Pillow 12 mis-decodes these
    # files: streaks; checked by eye against the panel).
    /usr/bin/python3 -I - "$STOCK" "$W/d" <<'PY'
import struct, sys
src, out = sys.argv[1], sys.argv[2]
for n in range(4):
    d = open('%s/S00%d.BMP' % (src, n), 'rb').read()
    off, hs = struct.unpack_from('<II', d, 10)
    w, h = struct.unpack_from('<ii', d, 18)
    pal = [d[14 + hs + 4 * i:18 + hs + 4 * i] for i in range(16)]
    pal = [((q[2] & 0xF8) << 8) | ((q[1] & 0xFC) << 3) | (q[0] >> 3) for q in pal]
    img = [[0] * w for _ in range(h)]
    i, x, k = off, 0, 0
    def put(x, k, v):
        if x < w and k < h:
            img[h - 1 - k][x] = pal[v]
    while i < len(d) - 1:
        c = d[i]; i += 1
        if c == 0:
            e = d[i]; i += 1
            if e == 0: x = 0; k += 1
            elif e == 1: break
            elif e == 2: x += d[i]; k += d[i + 1]; i += 2
            else:
                for j in range(e):
                    b = d[i + j // 2]; put(x, k, b >> 4 if j % 2 == 0 else b & 15); x += 1
                i += ((e + 1) // 2 + 1) & ~1
        else:
            v = d[i]; i += 1
            for j in range(c):
                put(x, k, v >> 4 if j % 2 == 0 else v & 15); x += 1
    open('%s/STK%d.ref' % (out, n), 'wb').write(struct.pack('<II', w, h) + b''.join(struct.pack('<%dH' % w, *r) for r in img))
PY
    I="$W/img/stock.img"
    mkimg $I
    for n in 0 1 2 3; do mcopy -i $I $STOCK/S00$n.BMP ::STK$n.BMP; done
    # The e6 grid shows a real stock screenshot.
    mcopy -o -i $W/img/e6.img $STOCK/S001.BMP ::S003.BMP
fi
# Compact binary logs (LOGnnn.DLG, dlog.rs): CSVs re-coded, on the format
# disk next to the CSVs, same expectations (every row read: exact counts).
for n in 000 001 002 005 006; do
    "$W/t" dlg $W/d/LOG$n.CSV $W/d/LOG2${n:1}.DLG
    cp $W/d/LOG$n.exp $W/d/LOG2${n:1}.exp
    mcopy -i $W/img/fmt.img $W/d/LOG2${n:1}.DLG ::LOG2${n:1}.DLG
done
# Fragmented disk: S000.BMP over every other freed 8 KiB hole.
I="$W/img/frag.img"
mkimg $I
for k in $(seq 10 49); do head -c 8192 /dev/urandom > $W/f.bin; mcopy -i $I $W/f.bin ::F$k.BIN; done
for k in $(seq 10 2 48); do mdel -i $I ::F$k.BIN; done
mcopy -i $I $W/d/S000.BMP ::S000.BMP
mcopy -i $I $W/d/LOG001.CSV ::LOG001.CSV
mcopy -i $I $W/d/S005.BMP ::S005.BMP

RC=0
echo "=== decode"
"$W/t" decode $W/img/fmt.img $W/d || RC=1
echo "=== decode, fragmented chains"
"$W/t" decode $W/img/frag.img $W/d || RC=1
if [ -f $W/img/stock.img ]; then
    echo "=== decode, stock RLE4 screenshots"
    "$W/t" decode $W/img/stock.img $W/d || RC=1
fi
echo "=== lock + delete"
cp $W/img/e6.img $W/lock.img
"$W/t" lock $W/lock.img $W/d || RC=1
fsck.fat -n $W/lock.img | tail -1
fsck.fat -n $W/lock.img >/dev/null 2>&1 && echo "ok: fsck clean after the delete" || { echo "FAIL: fsck"; RC=1; }
echo "=== ui matrix"
"$W/t" ui "$OUT" $W/d $W/img | tee "$OUT/check.txt"
[ "${PIPESTATUS[0]}" = 0 ] || RC=1
/usr/bin/python3 -I - "$OUT" <<'PY'
import glob, os, sys
from PIL import Image
out = sys.argv[1]
ims = []
for p in sorted(glob.glob(os.path.join(out, '*.ppm'))):
    im = Image.open(p)
    png = p[:-4] + '.png'
    im.save(png)
    os.remove(p)
    ims.append((os.path.basename(png)[:-4], im))
if ims:
    cols = 2
    w, h = ims[0][1].size
    rows = (len(ims) + cols - 1) // cols
    sheet = Image.new('RGB', (cols * (w + 8), rows * (h + 8)), (255, 255, 255))
    for i, (n, im) in enumerate(ims):
        sheet.paste(im, ((i % cols) * (w + 8), (i // cols) * (h + 8)))
    sheet.save(os.path.join(out, 'contact-sheet.png'))
    print('%d renders + contact-sheet.png in %s' % (len(ims), out))
PY
[ $RC = 0 ] && echo "ALL OK" || echo "FAILURES"
exit $RC
