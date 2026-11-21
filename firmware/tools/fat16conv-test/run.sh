#!/bin/bash
# Host test of src/fat16conv.rs (the later units' FAT12 disk made the DM40
# FAT16 volume in place). Needs no hardware and no mtools: the FAT12 image
# is built in main.rs like the one a BY25Q128ES unit came with.
#   tools/fat16conv-test/run.sh [SPIFLASH.BIN]   # optional: a real dump too
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
rustc --edition 2021 -O main.rs -o "$B/t"
"$B/t" "$@"
