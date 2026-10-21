#!/bin/sh
# Build fw-rs: the native update file -> dm40-fw-native.bin.
#   ./build.sh [native] [cargo args]       e.g. ./build.sh --features keep-screen
# (`native` is accepted as a no-op first argument for older callers, e.g.
# emu/run.py.) Body = 0x08004000..end with the 12-byte loader marker at
# 0x08008000 as 0xFF, linked with link-native.x, packed by
# ../firmware/dm40fw.py pack --pad (0xFF-padded to whole 128-byte blocks,
# checked below) into the stock update format (renamed to dm40*.bin only when
# staged on the device). tools/reload.sh passes its arguments here.
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
OBJCOPY=$(find ../toolchain/rustup -name llvm-objcopy | head -1)

if [ "$1" = native ]; then shift; fi
SCRIPT=link-native.x; TDIR=target/native
# Reproducible image (review L10): the same commit gives the same bytes in
# any checkout / worktree. Absolute paths reach rodata through panic
# locations of the dependencies (CARGO_HOME/registry/src/..., resolved
# through the worktree's toolchain symlink); remap them to relative ones,
# e.g. crates/usb-device-0.3.2/src/bus.rs (our own sources are already
# src/..., core is /rustc/<hash>/...). rustc applies the LAST matching
# prefix, so the most specific comes last; logical (pwd) and physical
# (pwd -P) spellings both. Paths must not contain spaces (RUSTFLAGS).
REMAP=
remap() { # remap FROM-dir TO
    for d in "$(cd "$1" && pwd)" "$(cd "$1" && pwd -P)"; do
        REMAP="$REMAP --remap-path-prefix=$d=$2"
    done
}
remap .. dm40
remap "$RUSTUP_HOME" rustup
remap "$CARGO_HOME" cargo
for d in "$CARGO_HOME"/registry/src/*/; do
    [ -d "$d" ] && remap "$d" crates
done
RUSTFLAGS="-C link-arg=-T$SCRIPT -C target-cpu=cortex-m4$REMAP" CARGO_TARGET_DIR=$TDIR \
    cargo build --release "$@"
ELF=$TDIR/thumbv7em-none-eabihf/release/dm40-fw

# Static worst-case stack path vs the TCM stack (>= 16 KiB headroom).
if ! python3 tools/stack-depth/stackdepth.py "$ELF" --check 16384 --top 0 > target/native/stackdepth.txt; then
    cat target/native/stackdepth.txt >&2
    exit 1
fi
head -1 target/native/stackdepth.txt

BODY=target/native/dm40-fw-native.body.bin
"$OBJCOPY" -O binary --gap-fill=0xff "$ELF" "$BODY"
python3 ../firmware/dm40fw.py pack --pad "$BODY" dm40-fw-native.bin
python3 - "$BODY" "$ELF" <<'PY'
import struct, subprocess, sys
body = open(sys.argv[1], 'rb').read()
BASE, MARK, END = 0x08004000, 0x08008000, 0x080A0000
end = BASE + len(body)
sp, entry = struct.unpack_from('<II', body)
nm = subprocess.run(['sh', '-c', 'find ../toolchain/rustup -name llvm-nm | head -1'],
                    capture_output=True, text=True).stdout.strip()
syms = {}
for line in subprocess.run([nm, sys.argv[2]], capture_output=True, text=True).stdout.split('\n'):
    p = line.split()
    if len(p) == 3:
        syms[p[2]] = int(p[0], 16)
gap = body[MARK - BASE:MARK - BASE + 16]
pad = body[syms['__region_a_end'] - BASE:MARK + 16 - BASE]
print("dm40-fw-native.bin: body %d bytes, 0x%08x..0x%08x (limit 0x%08x, %d bytes free)"
      % (len(body), BASE, end, END, END - end))
print("  word0 initial SP = 0x%08x (loader only; reset sets 0x10010000)" % sp)
print("  word1 reset      = 0x%08x" % entry)
print("  region A end     = 0x%08x (shim + vectors, limit 0x%08x)" % (syms['__region_a_end'], MARK))
print("  .text start      = 0x%08x" % syms['__stext'])
print("  marker 0x08008000..0x0800800F = %s" % gap.hex())
print("  0x%08x..0x08008010 (%d bytes after region A) all 0xFF: %s"
      % (syms['__region_a_end'], len(pad), pad == b'\xff' * len(pad)))
assert end < END, 'image reaches 0x080A0000'
assert syms['__region_a_end'] <= MARK and syms['__stext'] >= MARK + 12
assert gap == b'\xff' * 16, 'marker gap is not 0xFF'
assert pad == b'\xff' * len(pad), 'padding between region A and region B is not 0xFF'
assert body[:4] == struct.pack('<I', 0x2002F7F8)
assert BASE <= entry - 1 < MARK and entry & 1, 'reset vector not in region A (thumb)'
PY
python3 ../firmware/dm40fw.py info dm40-fw-native.bin
# The FS-loader mis-decodes a short last 128-byte block whose key position is
# outside it (docs/native-boot.md "Update file format"); dm40fw.py pack --pad pads
# the body so the file is whole blocks. Fail the build if it is not.
n=$(wc -c < dm40-fw-native.bin)
if [ $((n % 128)) -ne 0 ]; then
    echo "build.sh: dm40-fw-native.bin is $n bytes, not a multiple of 128 (short last block)" >&2
    rm -f dm40-fw-native.bin
    exit 1
fi
if [ $((0x08004000 + n - 12)) -gt $((0x080A0000)) ]; then
    echo "build.sh: the padded body ends past 0x080A0000 (FS-loader)" >&2
    rm -f dm40-fw-native.bin
    exit 1
fi
