#!/bin/bash
# Host test of src/scpi_msg.rs (compound SCPI messages: one command at a
# time, replies joined in order with ';', one response per message, a
# reading wait pausing the rest, block replies alone only), then a
# thumbv7em-none-eabihf lib build of the same file. Needs no hardware.
#   tools/scpi-msg-test/run.sh
# End to end over USBTMC: emu/usbtmc_stress.py --cases compound.
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
rustc --edition 2021 -O main.rs -o "$W/t"
"$W/t"
printf '#![no_std]\n#[path = "%s"]\n#[allow(dead_code)]\npub mod scpi_msg;\n' "$(cd ../../src && pwd)/scpi_msg.rs" > "$W/nostd.rs"
rustc --edition 2021 --target thumbv7em-none-eabihf -C target-cpu=cortex-m4 -C panic=abort -O \
    --crate-type lib --crate-name scpi_msg -o "$W/lib.rlib" "$W/nostd.rs"
echo "ok: target build"
