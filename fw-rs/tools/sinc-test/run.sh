#!/bin/bash
# Host test of src/sinc.rs (UF-41 scope sin(x)/x interpolation): sin/cos
# polynomial, kernel weights, DC / sample pass-through, sine reconstruction
# vs linear, edges. Needs no hardware.
#   tools/sinc-test/run.sh
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
rustc --edition 2021 -O main.rs -o "$B/t"
"$B/t"
