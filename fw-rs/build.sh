#!/bin/sh
# Build the RAM image and emit dm40-fw.bin (bare binary, load at 0x20008000).
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
cargo build --release "$@"
OBJCOPY=$(find ../toolchain/rustup -name llvm-objcopy | head -1)
ELF=target/thumbv7em-none-eabihf/release/dm40-fw
"$OBJCOPY" -O binary "$ELF" dm40-fw.bin
echo "dm40-fw.bin: $(wc -c < dm40-fw.bin) bytes, flash at 0x08080000"
python3 - <<'PY'
import struct
d = open('dm40-fw.bin', 'rb').read()
sp, entry = struct.unpack_from('<II', d)
print("  initial SP   = 0x%08x" % sp)
print("  reset entry  = 0x%08x" % entry)
PY
