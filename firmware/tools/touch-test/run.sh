#!/bin/bash
# Host test of the touch input path (main.rs): touch.rs debounce, gesture.rs
# decisions, the DBG:TOUCH? log text, and every page's hit map against what
# src/ui.rs draws (LCD emulator of tools/ui-render/host.rs, draw-call probes
# on). Needs no hardware.
#   tools/touch-test/run.sh [-v]     -v: per-case lines, hit reach per button
# Exit status 1 on any failure.
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
CARGO_PKG_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' ../../Cargo.toml | head -1) \
    rustc --edition 2021 -O --cfg test -A warnings main.rs -o "$B/t"
"$B/t" "$@"
