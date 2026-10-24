#!/bin/bash
# Host renderer of the STOCK DM40 V2.6 scope UI (tools/stock-ui/main.rs):
# builds the renderer for the host against the ILI9488 emulator of
# tools/ui-render/host.rs with the stock app image (../../dm40_v_2_6.dec.bin)
# mapped at 0x08004000 so every stock font is read live, renders the scene
# list (or the scenes named on the command line) and converts them to PNG
# (1x and 2x) in <out dir> (default: tools/stock-ui/out).
#   tools/stock-ui/run.sh [out dir] [scene ...]
# Needs no hardware.
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
OUT=${1:-out}
shift || true
mkdir -p "$OUT"
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
rustc --edition 2021 -O -A warnings main.rs -o "$B/r"
"$B/r" "$(cd ../../.. && pwd)/dm40_v_2_6.dec.bin" "$OUT" "$@"
/usr/bin/python3 -I - "$OUT" <<'PY'
import glob, os, sys
from PIL import Image
out = sys.argv[1]
n = 0
for p in sorted(glob.glob(os.path.join(out, '*.ppm'))):
    im = Image.open(p)
    im.save(p[:-4] + '.png')
    im.resize((im.size[0] * 2, im.size[1] * 2), Image.NEAREST).save(p[:-4] + '@2x.png')
    os.remove(p)
    n += 1
print('%d renders in %s' % (n, out))
PY
