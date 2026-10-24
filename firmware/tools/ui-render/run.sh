#!/bin/bash
# Host render + layout check of the fw-rs screens (main.rs): builds the real
# drawing code (src/ui.rs, src/stockfont.rs, ...) for the host with
# --cfg test (draw-call probes on) against the LCD emulator in host.rs, runs
# the worst-case matrix and the layout rules, writes PNG renders (2x) and a
# contact sheet to <out dir> (default: a temp dir, kept and printed).
#   tools/ui-render/run.sh [out dir]
#   tools/ui-render/run.sh <out dir> --readme   the README screens only (1x,
#                                               readme.rs; for ../../docs/img/)
# Exit status 1 when any rule is violated. Needs no hardware.
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
OUT=${1:-$(mktemp -d)}
mkdir -p "$OUT"
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
CARGO_PKG_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' ../../Cargo.toml | head -1) \
    rustc --edition 2021 -O --cfg test -A warnings main.rs -o "$B/r"
set +e
"$B/r" "$OUT" "${@:2}" | tee "$OUT/check.txt"
RC=${PIPESTATUS[0]}
set -e
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
exit $RC
