#!/bin/sh
# Build opendm40: the native update file -> opendm40.bin, and the same file
# under its install name dm40_opendm40_v<version>.bin.
#   ./build.sh [native] [cargo args]       e.g. ./build.sh --features keep-screen
# (`native` is accepted as a no-op first argument for older callers, e.g.
# emu/run.py.) Body = 0x08004000..end with the 12-byte loader marker at
# 0x08008000 as 0xFF, linked with link-native.x, packed by
# ../stock/dm40fw.py pack --pad (0xFF-padded to whole 128-byte blocks,
# checked below) into the stock update format (renamed to dm40*.bin only when
# staged on the device). tools/reload.sh passes its arguments here.
# DM40_TDIR / DM40_OUT (environment) build elsewhere than target/native /
# opendm40.bin, with every check (tools/build-variants/run.sh builds
# the recovery-usb / recovery-file / no-ad2-isr variants this way, so a test
# build never replaces the image reload.sh installs).
set -e
cd "$(dirname "$0")"
# Toolchain: the repo's pinned one in ../toolchain (README "Toolchain") when
# it is there; else (CI, a checkout without it) the rustup install that
# RUSTUP_HOME / CARGO_HOME name (default ~/.rustup, ~/.cargo) with cargo on
# PATH. Either needs the thumbv7em-none-eabihf target and llvm-tools.
if [ -d ../toolchain/rustup ] && [ -d ../toolchain/cargo ]; then
    RUSTUP_HOME="$(cd ../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../toolchain/cargo && pwd)"
else
    RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"; CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
fi
export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
# llvm-objcopy / llvm-nm of the active toolchain (component llvm-tools).
LLVMBIN=$(echo "$(rustc --print sysroot)"/lib/rustlib/*/bin)
OBJCOPY=$LLVMBIN/llvm-objcopy
[ -x "$OBJCOPY" ] || { echo "build.sh: no llvm-objcopy in $LLVMBIN (rustup component add llvm-tools)" >&2; exit 1; }
export DM40_LLVMBIN="$LLVMBIN"

if [ "$1" = native ]; then shift; fi
SCRIPT=link-native.x; TDIR=${DM40_TDIR:-target/native}; OUT=${DM40_OUT:-opendm40.bin}
mkdir -p "$TDIR"
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
# The registry sources must be unpacked before the remap list is made (a
# fresh CARGO_HOME, e.g. in CI, has none yet): cargo fetch, with the
# lock-file flags of this build so a stale Cargo.lock still fails --locked.
FETCH=
for a in "$@"; do case $a in --locked|--frozen|--offline) FETCH="$FETCH $a" ;; esac; done
cargo fetch -q $FETCH
for d in "$CARGO_HOME"/registry/src/*/; do
    [ -d "$d" ] && remap "$d" crates
done
# -Zlocation-detail=none: panic sites carry no file / line / column (the
# panic handler, main.rs `panic`, never reads them; ~32 KB of region B,
# docs/native-boot.md "Region B audit #6"). A nightly flag: RUSTC_BOOTSTRAP=1
# lets the pinned stable rustc take it, for this firmware build only (the
# host test builds do not use build.sh and keep their panic locations).
RUSTC_BOOTSTRAP=1 \
RUSTFLAGS="-Zlocation-detail=none -C link-arg=-T$SCRIPT -C target-cpu=cortex-m4$REMAP" CARGO_TARGET_DIR=$TDIR \
    cargo build --release "$@"
ELF=$TDIR/thumbv7em-none-eabihf/release/opendm40

# Static worst-case stack path vs the TCM stack (>= 20 KiB headroom; the
# SRAM-free, region-A2, region-B and region-C checks are link-native.x asserts).
if ! python3 tools/stack-depth/stackdepth.py "$ELF" --check 20480 --top 0 > $TDIR/stackdepth.txt; then
    cat $TDIR/stackdepth.txt >&2
    exit 1
fi
head -1 $TDIR/stackdepth.txt

BODY=$TDIR/opendm40.body.bin
rm -f $TDIR/regiona2.warn $TDIR/regionb.warn $TDIR/regionc.warn
"$OBJCOPY" -O binary --gap-fill=0xff "$ELF" "$BODY"
python3 ../stock/dm40fw.py pack --pad "$BODY" "$OUT"
python3 - "$BODY" "$ELF" "$TDIR" <<'PY'
import os, struct, subprocess, sys
body = open(sys.argv[1], 'rb').read()
BASE, MARK, END = 0x08004000, 0x08008000, 0x080A0000
end = BASE + len(body)
sp, entry = struct.unpack_from('<II', body)
nm = os.environ['DM40_LLVMBIN'] + '/llvm-nm'
syms = {}
for line in subprocess.run([nm, sys.argv[2]], capture_output=True, text=True).stdout.split('\n'):
    p = line.split()
    if len(p) == 3:
        syms[p[2]] = int(p[0], 16)
gap = body[MARK - BASE:MARK - BASE + 16]
pad = body[syms['__a2_end'] - BASE:MARK + 16 - BASE]
print("opendm40.bin: body %d bytes, 0x%08x..0x%08x (limit 0x%08x, %d bytes free)"
      % (len(body), BASE, end, END, END - end))
# Region A2 (zero-wait, after the shim, below the loader marker): the link
# fails when it reaches the marker (0 bytes free); warn below 2 KiB.
a2free = MARK - syms['__a2_end']
print("  region A2 free   = %d bytes (0x%08x..0x%08x used, zero-wait below the marker; link fails below 0)"
      % (a2free, syms['__a2_start'], syms['__a2_end']))
if a2free < 2048:
    open(sys.argv[3] + '/regiona2.warn', 'w').write('%d\n' % a2free)
bfree = 0x08080000 - syms['__b_end']
print("  region B free    = %d bytes (zero-wait flash below 0x08080000; link fails below 8 KiB)" % bfree)
if bfree < 16384:
    # Repeated as a banner at the very end of build.sh (target/native/regionb.warn).
    open(sys.argv[3] + '/regionb.warn', 'w').write('%d\n' % bfree)
# Region C (.cold, slow flash; the .data load image is in B) ends at the FS-loader:
# the update file cannot reach above 0x080A0000 (docs/native-boot.md "Memory budget").
cfree = END - syms['__image_end']
print("  region C free    = %d bytes (.cold below 0x080A0000; link fails below 4 KiB)" % cfree)
if cfree < 8192:
    open(sys.argv[3] + '/regionc.warn', 'w').write('%d\n' % cfree)
print("  SRAM free        = %d bytes (above .sram_bss; >= 8 KiB)" % (0x20030000 - syms['__sram_bss_end']))
print("  word0 initial SP = 0x%08x (loader only; reset sets 0x10010000)" % sp)
print("  word1 reset      = 0x%08x" % entry)
print("  region A end     = 0x%08x (shim + vectors, limit 0x%08x)" % (syms['__region_a_end'], MARK))
print("  .text start      = 0x%08x" % syms['__stext'])
print("  marker 0x08008000..0x0800800F = %s" % gap.hex())
print("  0x%08x..0x08008010 (%d bytes after region A2) all 0xFF: %s"
      % (syms['__a2_end'], len(pad), pad == b'\xff' * len(pad)))
assert end < END, 'image reaches 0x080A0000'
assert syms['__region_a_end'] <= MARK and syms['__stext'] >= MARK + 12
assert syms['__a2_start'] == syms['__region_a_end'] and syms['__a2_end'] <= MARK, 'region A2 overlaps the marker'
assert gap == b'\xff' * 16, 'marker gap is not 0xFF'
assert pad == b'\xff' * len(pad), 'padding between region A2 and region B is not 0xFF'
assert body[:4] == struct.pack('<I', 0x2002F7F8)
assert BASE <= entry - 1 < MARK and entry & 1, 'reset vector not in region A (thumb)'
PY
python3 ../stock/dm40fw.py info "$OUT"
# The FS-loader mis-decodes a short last 128-byte block whose key position is
# outside it (docs/native-boot.md "Update file format"); dm40fw.py pack --pad pads
# the body so the file is whole blocks. Fail the build if it is not.
n=$(wc -c < "$OUT")
if [ $((n % 128)) -ne 0 ]; then
    echo "build.sh: $OUT is $n bytes, not a multiple of 128 (short last block)" >&2
    rm -f "$OUT"
    exit 1
fi
if [ $((0x08004000 + n - 12)) -gt $((0x080A0000)) ]; then
    echo "build.sh: the padded body ends past 0x080A0000 (FS-loader)" >&2
    rm -f "$OUT"
    exit 1
fi
# The install name, dm40_opendm40_v<version>.bin: a copy next to
# opendm40.bin (the file tools/reload.sh and the emulator take). <version> is
# the one *IDN? and Settings > Device show (src/ident.rs VERSION): Cargo.toml's
# version as Cargo.toml has it (0.1.0, 0.1.0-pre2).
# Not for DM40_OUT builds.
if [ -z "${DM40_OUT:-}" ]; then
    VER=$(tools/fw-version.sh)
    cp "$OUT" "dm40_opendm40_v$VER.bin"
    echo "install file: dm40_opendm40_v$VER.bin"
fi
# Region A2 (zero-wait, 0x0800460c..0x08008000) below 2 KiB free: a banner
# (the link fails when A2 reaches the loader marker at 0x08008000).
if [ -f $TDIR/regiona2.warn ]; then
    a=$(cat $TDIR/regiona2.warn)
    {
        echo
        echo "################################################################################"
        echo "## build.sh: WARNING: region A2 (zero-wait, below the loader marker) has only"
        echo "## $a bytes free (< 2 KiB). The link fails when A2 reaches 0x08008000. Take a"
        echo "## font out of A2_FONTS (tools/mkstockfont.py) or a .rodata.a2 / .text.a2 item."
        echo "################################################################################"
        echo
    } >&2
fi
# Region B (zero-wait flash) below 16 KiB free: a loud banner as the last
# thing printed (the link itself fails below 8 KiB, link-native.x).
if [ -f $TDIR/regionb.warn ]; then
    b=$(cat $TDIR/regionb.warn)
    {
        echo
        echo "################################################################################"
        echo "## build.sh: WARNING: region B (zero-wait flash) has only $b bytes free (< 16 KiB)"
        echo "## The link fails below 8 KiB. Move rarely run code (start / stop, taps, pages,"
        echo "## debug replies) to .cold (link-native.x; #[inline(never)] where it is inlined"
        echo "## into a hot caller), never per-pass or ISR / stream code (docs/native-boot.md)."
        echo "################################################################################"
        echo
    } >&2
fi
# Region C (.cold, 0x08080000..0x080A0000) below 8 KiB free: the same kind of
# banner (the link fails below 4 KiB). The FS-loader writes the update body
# only to 0x08004000..0x080A0000; above are the FS-loader, boot data and
# calibration (docs/native-boot.md "Flash outside the image").
if [ -f $TDIR/regionc.warn ]; then
    c=$(cat $TDIR/regionc.warn)
    {
        echo
        echo "################################################################################"
        echo "## build.sh: WARNING: region C (.cold, slow flash) has only $c bytes free (< 8 KiB)"
        echo "## The link fails below 4 KiB. The image may not reach 0x080A0000 (FS-loader)."
        echo "## Cut code, or move .cold code back to region B while B has room (only what"
        echo "## B's budget allows); see docs/native-boot.md \"Memory budget\"."
        echo "################################################################################"
        echo
    } >&2
fi
