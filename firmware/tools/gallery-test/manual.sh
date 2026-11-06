#!/bin/bash
# The manual's gallery pictures: the real gallery code on the host (as
# run.sh) over a DM40-geometry disk with made-up files: eight screenshots
# made from the manual's own screen renders (manual.py; DC V, ohms, combined
# view, scope, generator, Settings, capacitance, temperature) in SHOTS/ and
# two recordings in LOGS/. Writes OUT/gallery.png (the grid) and
# OUT/gallery-select.png (select mode, three ticks), 960 x 640, and fails on
# a layout violation. tools/ui-render/manual-img.py takes them from its
# gallery directory (OUT4).
#   tools/gallery-test/manual.sh OUT [screen dir]
# screen dir: where the screen renders are (default docs/manual/img; a
# language's folder for its pictures).
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
OUT=$(mkdir -p "$1" && cd "$1" && pwd)
SRC=$(cd "${2:-../../../docs/manual/img}" && pwd)
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
CARGO_PKG_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' ../../Cargo.toml | head -1) \
    rustc --edition 2021 -O --cfg test -A warnings main.rs -o "$W/t"
/usr/bin/python3 -I manual.py "$SRC" "$W/d"
export MTOOLS_SKIP_CHECK=1
I="$W/man.img"
dd if=/dev/zero of=$I bs=1M count=16 status=none
mkfs.fat -F 16 -S 4096 -s 1 -f 2 -r 512 -R 1 -n "ATK-DM40" $I >/dev/null
echo "dummy" > $W/cfg.bin
mcopy -i $I $W/cfg.bin ::CONFIG.BIN
mmd -i $I ::LOGS ::SHOTS
for f in $W/d/LOG*.CSV; do
    "$W/t" dlg $f ${f%.CSV}.DLG >/dev/null
    mcopy -i $I ${f%.CSV}.DLG ::LOGS/
done
for f in $W/d/S*.BMP; do mcopy -i $I $f ::SHOTS/; done
"$W/t" manual "$W" $I
/usr/bin/python3 -I -c '
import sys
from PIL import Image
for n in ("gallery", "gallery-select"):
    Image.open("%s/%s.ppm" % (sys.argv[1], n)).save("%s/%s.png" % (sys.argv[2], n))
' "$W" "$OUT"
echo "$OUT/gallery.png $OUT/gallery-select.png"
