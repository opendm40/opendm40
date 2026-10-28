#!/bin/bash
# Host test of src/ohm600m.rs (the experimental 600 Mohm range: row,
# phase plan, maths, DBG:OHMX parser). Needs no hardware.
#   tools/ohm600m-test/run.sh
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
rustc --edition 2021 -O main.rs -o "$B/t"
"$B/t"
