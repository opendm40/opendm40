#!/bin/bash
# Pixel diff of the replica scope page (src/scope_ui.rs, scenes in
# tools/ui-render/stock.rs) against the stock reference renderer
# (tools/stock-ui, same scene names): renders both, runs
# tools/stock-ui/diff.py per scene with the stock regions and prints the
# per-region scores (graph_inner excluded from the total: the 1 px trace /
# marker offsets are reproduced, so it is expected to be 0 too).
# Since the four channel blocks (2026-11-02: scope_ui::GX 0, DIV_W 39, the
# 89 px panel) the scope page no longer has the stock geometry: only the
# top bar regions still compare; the graph, the markers, the panel and the
# measurement cells differ by design. The last commit with the stock
# geometry and panel is 044a4c1d ("derive the graph ... from one graph
# rectangle"): check it out for a pixel diff of the replica page.
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
    rustc --edition 2021 -O --cfg test -A warnings main.rs -o "$B/r"
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
