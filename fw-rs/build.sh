#!/bin/sh
# Build fw-rs.
#   ./build.sh [cargo args]                RAM image (sector 8, BT-loaded) -> dm40-fw.bin
#   ./build.sh test-initcore [cargo args]  T1 RAM test image              -> dm40-fw.bin
#   ./build.sh test-recovery [cargo args]  T2 RAM test image              -> dm40-fw.bin
#   ./build.sh native [cargo args]         native update file             -> dm40-fw-native.bin
# (tools/reload.sh passes its arguments here, so `tools/reload.sh test-recovery`
# BT-loads the T2 image.) The default mode is exactly the original build; the
# other modes select their linker script through RUSTFLAGS and build in their
# own target directory. Native: body = 0x08004000..end with the 12-byte loader
# marker at 0x08008000 as 0xFF, packed by ../firmware/dm40fw.py into the stock
# update format (renamed to dm40*.bin only when staged on the device).
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
OBJCOPY=$(find ../toolchain/rustup -name llvm-objcopy | head -1)

MODE=ram
case "$1" in
    native|test-initcore|test-recovery) MODE=$1; shift ;;
esac

if [ "$MODE" = ram ]; then
    cargo build --release "$@"
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
    exit 0
fi

if [ "$MODE" = native ]; then
    SCRIPT=link-native.x; FEAT=native; TDIR=target/native
else
    SCRIPT=link-test.x; FEAT=$MODE; TDIR=target/test-$MODE
fi
RUSTFLAGS="-C link-arg=-T$SCRIPT -C target-cpu=cortex-m4" CARGO_TARGET_DIR=$TDIR \
    cargo build --release --features "$FEAT" "$@"
ELF=$TDIR/thumbv7em-none-eabihf/release/dm40-fw

if [ "$MODE" != native ]; then
    "$OBJCOPY" -O binary "$ELF" dm40-fw.bin
    echo "dm40-fw.bin ($MODE RAM test image): $(wc -c < dm40-fw.bin) bytes, flash at 0x08080000"
    python3 - <<'PY'
import struct
d = open('dm40-fw.bin', 'rb').read()
assert len(d) <= 128 * 1024, 'image larger than flash sector 8'
sp, entry = struct.unpack_from('<II', d)
print("  initial SP   = 0x%08x" % sp)
print("  reset entry  = 0x%08x" % entry)
PY
    exit 0
fi

BODY=target/native/dm40-fw-native.body.bin
"$OBJCOPY" -O binary --gap-fill=0xff "$ELF" "$BODY"
python3 ../firmware/dm40fw.py pack "$BODY" dm40-fw-native.bin
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
