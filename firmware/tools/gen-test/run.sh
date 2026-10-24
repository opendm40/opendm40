#!/bin/bash
# Host test of src/genplan.rs (generator frequency search, per-wave limits,
# levels/offset, table contents). Builds main.rs with the repo toolchain and
# runs it; exits non-zero on the first failed check.
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
rustc --edition 2021 -O main.rs -o "$W/t"
"$W/t" "$@"
