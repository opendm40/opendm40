#!/bin/bash
# Host test of src/bargraph.rs (UF-30 DMM fast bargraph) on the LCD emulator
# of tools/ui-render/host.rs: every incremental update equals a fresh draw,
# scale / sign / overload, the per-update bus cost; renders (2x PPM -> PNG)
# to <out dir> (default: a temp dir). Needs no hardware.
#   tools/bargraph-test/run.sh [out dir]
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
OUT=${1:-$(mktemp -d)}
mkdir -p "$OUT"
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
rustc --edition 2021 -O -A warnings main.rs -o "$B/t"
"$B/t" "$OUT"
/usr/bin/python3 -I -c '
import glob, os, sys
from PIL import Image
for p in glob.glob(os.path.join(sys.argv[1], "*.ppm")):
    Image.open(p).save(p[:-4] + ".png"); os.remove(p)
print("renders in", sys.argv[1])' "$OUT"
