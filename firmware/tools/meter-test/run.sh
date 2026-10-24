#!/bin/bash
# Host unit tests of the FR-05 display smoothing / conversion rate and the
# UF-21 persisted-flag decoding: the #[cfg(test)] modules of src/meter.rs and
# src/dmm.rs, built with rustc --test on the host (main.rs pulls in the
# modules by path). Does not touch hardware and does not modify any source.
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
rustc --edition 2021 --test -A warnings main.rs -o "$W/t"
"$W/t" "$@"
