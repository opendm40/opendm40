#!/bin/bash
# S9 validation: pixel diff of the replica scope page (src/scope_ui.rs,
# scenes stock::emu_scenes) against the REAL stock V2.6 firmware running in
# the Renode emulator (emu/run.py). The captures are made by
# tools/ui-render/emu-capture.sh into <capture dir>; this renders ours with the
# same state and runs tools/stock-ui/diff.py per scene and region.
#   tools/ui-render/emu-diff.sh <capture dir> [out dir] [scene ...]
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
CAP=$(cd "$1" && pwd)
OUT=${2:-$(mktemp -d)}
shift 2 || shift $#
mkdir -p "$OUT/ours"
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
CARGO_PKG_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' ../../Cargo.toml | head -1) \
    rustc --edition 2021 -O --cfg test --cfg 'feature="embedded-fonts"' -A warnings main.rs -o "$B/r"
"$B/r" "$OUT" --stock "$OUT/ours"
for p in "$OUT"/ours/e*.ppm; do
    n=$(basename "$p" .ppm)
    /usr/bin/python3 -I -c "from PIL import Image; Image.open('$p').save('${p%.ppm}.png')"
    rm "$p"
    if [ $# -gt 0 ]; then
        hit=0; for w in "$@"; do case "$n" in *"$w"*) hit=1 ;; esac; done
        [ $hit = 1 ] || continue
    fi
    [ -f "$CAP/$n.png" ] || { echo "$n: no capture"; continue; }
    echo "== $n"
    /usr/bin/python3 -I ../stock-ui/diff.py "$OUT/ours/$n.png" "$CAP/$n.png" -o "$OUT/diff-$n.png" \
        --regions ../stock-ui/regions.json 2>/dev/null | sed 's/^/  /'
done
rm -f "$OUT"/ours/st*.ppm
echo "renders and diff images in $OUT"
