#!/bin/bash
# Pixel diff of the replica scope page (src/scope_ui.rs, scenes in
# tools/ui-render/stock.rs) against the stock reference renderer
# (tools/stock-ui, same scene names): renders both, runs
# tools/stock-ui/diff.py per scene with the stock regions and prints the
# per-region scores (graph_inner excluded from the total: the 1 px trace /
# marker offsets are reproduced, so it is expected to be 0 too).
#   tools/ui-render/stock-diff.sh [out dir] [scene ...]
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
OUT=${1:-$(mktemp -d)}
shift || true
mkdir -p "$OUT/ref" "$OUT/ours"
../stock-ui/run.sh "$OUT/ref" "$@" > /dev/null
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
CARGO_PKG_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' ../../Cargo.toml | head -1) \
    rustc --edition 2021 -O --cfg test --cfg 'feature="embedded-fonts"' -A warnings main.rs -o "$B/r"
"$B/r" "$OUT" --stock "$OUT/ours"
for p in "$OUT"/ours/*.ppm; do
    n=$(basename "$p" .ppm)
    /usr/bin/python3 -I -c "from PIL import Image; Image.open('$p').save('${p%.ppm}.png')"
    rm "$p"
    if [ $# -gt 0 ]; then
        hit=0; for w in "$@"; do case "$n" in *"$w"*) hit=1 ;; esac; done
        [ $hit = 1 ] || continue
    fi
    [ -f "$OUT/ref/$n.png" ] || { echo "$n: no reference"; continue; }
    echo "== $n"
    /usr/bin/python3 -I ../stock-ui/diff.py "$OUT/ours/$n.png" "$OUT/ref/$n.png" -o "$OUT/diff-$n.png" \
        --regions ../stock-ui/regions.json | sed 's/^/  /'
done
echo "renders and diff images in $OUT"
