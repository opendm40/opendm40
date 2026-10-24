#!/bin/bash
# Host test of src/fft.rs (scope FFT, UF-11): FFT vs a direct DFT, dBV scale,
# interpolated peak per window, decimation, sliced stepping. Needs no hardware.
#   tools/fft-test/run.sh
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
rustc --edition 2021 -O main.rs -o "$B/t"
"$B/t"
