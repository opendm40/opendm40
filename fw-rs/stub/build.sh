#!/bin/sh
# Build the RAM stub -> dm40-stub.bin (load at 0x20008000, payload at 0x2000C000).
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
cargo build --release
OBJCOPY=$(find ../../toolchain/rustup -name llvm-objcopy | head -1)
"$OBJCOPY" -O binary target/thumbv7em-none-eabihf/release/dm40-stub dm40-stub.bin
echo "dm40-stub.bin: $(wc -c < dm40-stub.bin) bytes"
