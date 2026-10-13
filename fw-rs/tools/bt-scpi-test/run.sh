#!/bin/bash
# Host test of src/bt_scpi.rs (Bluetooth RX dispatcher: stock frames + SCPI
# lines on one stream; the untrusted-transport DBG:* filter; the TX ring):
#   1. main.rs on the host (mixed streams at every split point, byte by byte,
#      random chunking; timeouts; overrun; refusal cases; ring order);
#   2. a thumbv7em-none-eabihf lib build of src/bt_scpi.rs, so the file that
#      passed on the host is known to compile for the target too.
# Does not touch hardware and does not modify any fw-rs source.
# main-hooks.patch (next to this script) is the main.rs integration; check it
# still applies with: git apply --check fw-rs/tools/bt-scpi-test/main-hooks.patch
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
echo "=== 1. host test"
rustc --edition 2021 -O main.rs -o "$W/t"
"$W/t"
echo "=== 2. target build (thumbv7em-none-eabihf, lib, no_std)"
printf '#![no_std]\n#[path = "%s"]\n#[allow(dead_code)]\npub mod bt_scpi;\n' "$(cd ../../src && pwd)/bt_scpi.rs" > "$W/bt_scpi_nostd.rs"
rustc --edition 2021 --target thumbv7em-none-eabihf -C target-cpu=cortex-m4 -C panic=abort -O \
    --crate-type lib --crate-name bt_scpi -o "$W/libbt_scpi.rlib" "$W/bt_scpi_nostd.rs"
echo "ok: target build"
echo PASS
