#!/bin/bash
# Host test of src/accuracy.rs (the DMM accuracy label, docs/dmm-accuracy.md):
# the manual's row per function / range, the uncertainty per model in
# digits (the x10 digit above a DM40A / B stock range), deg C / F, rounding.
# Needs no hardware.
#   tools/accuracy-test/run.sh
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
rustc --edition 2021 -O main.rs -o "$B/t"
"$B/t"
