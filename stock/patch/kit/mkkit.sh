#!/bin/sh
# Build the memrw kit (patched stock firmware + host tool) into dist/.
#
#   stock/patch/kit/mkkit.sh [path/to/dm40_v_2_6.bin]
#
# Output: dist/dm40-memrw-kit/ and dist/dm40-memrw-kit.zip at the repo root
# (both ignored). The vendor file defaults to dm40_v_2_6.bin at the repo root;
# it is not in this repo (see the top-level README for where to get it).
# Needs arm-none-eabi-gcc, make, python3, zip.
set -e
KIT=$(cd "$(dirname "$0")" && pwd)
PATCH=$(dirname "$KIT")
ROOT=$(cd "$PATCH/../.." && pwd)
ORIG=$(realpath "${1:-$ROOT/stock/vendor/dm40_v_2_6.bin}")
OUT=$ROOT/dist/dm40-memrw-kit

[ -f "$ORIG" ] || { echo "missing vendor file $ORIG" >&2; exit 1; }

make -C "$PATCH" BUILD=build-memrw SRCS="src/startup.c src/memrw.c" ORIG="$ORIG"

rm -rf "$OUT" "$OUT.zip"
mkdir -p "$OUT/firmware" "$OUT/tools/tests"
cp "$KIT/README.md" "$OUT/"
cp "$KIT/tools/dm40_memrw.py" "$OUT/tools/"
cp "$KIT/tools/tests/"test_*.py "$OUT/tools/tests/"
cp "$ORIG" "$OUT/firmware/dm40_v_2_6.bin"
cp "$PATCH/build-memrw/dm40_patched.bin" "$OUT/firmware/dm40_v_2_6_memrw.bin"
(cd "$OUT/firmware" && sha256sum dm40_v_2_6_memrw.bin dm40_v_2_6.bin > SHA256SUMS && cat SHA256SUMS)
(cd "$ROOT/dist" && zip -qrX dm40-memrw-kit.zip dm40-memrw-kit)
echo "$OUT.zip"
