#!/bin/sh
# ./build.sh [core]  default: -Zbuild-std with immediate-abort panics (the firmware's flags);
# `core` = plain precompiled core; `nolfn` = without long names (8.3 list, 48-glyph font).
set -e
cd "$(dirname "$0")"
WT=$(cd .. && pwd)
export RUSTUP_HOME=$(cd $WT/toolchain/rustup && pwd) CARGO_HOME=$(cd $WT/toolchain/cargo && pwd)
export PATH="$CARGO_HOME/bin:$PATH" TMPDIR=$PWD/tmp
BASE="-C link-arg=-Tlink.x -C target-cpu=cortex-m4"
if [ "$1" = core ]; then
    RUSTFLAGS="$BASE" CARGO_TARGET_DIR=target-core cargo build --release --offline --target thumbv7em-none-eabihf
    ELF=target-core/thumbv7em-none-eabihf/release/miniloader
else
    OUTD=target; FEAT=
    if [ "$1" = hook ]; then OUTD=target-hook; FEAT="--features hook"; BASE="-C link-arg=-Tlink-hook.x -C target-cpu=cortex-m4"; fi
    if [ "$1" = nolfn ]; then OUTD=target-nolfn; FEAT="--no-default-features --features touch_ui,labels,verify,installed,idle,format"; fi
    # ./build.sh without=FEATURE: everything but FEATURE (size accounting), no .bin kept.
    case "$1" in without=*) OUTD=target-nolfn; ALL="lfn touch_ui labels verify installed idle format"; L=; for f in $ALL; do [ "$f" = "${1#without=}" ] || L="$L,$f"; done; FEAT="--no-default-features --features ${L#,}";; esac
    RUSTC_BOOTSTRAP=1 RUSTFLAGS="-Zlocation-detail=none -Zunstable-options -Cpanic=immediate-abort $BASE" \
      CARGO_TARGET_DIR=$OUTD cargo build --release --offline --target thumbv7em-none-eabihf -Zbuild-std=core $FEAT
    ELF=$OUTD/thumbv7em-none-eabihf/release/miniloader
fi
arm-none-eabi-size -A "$ELF" | egrep '^\.(vectors|text|rodata|data|bss)'
arm-none-eabi-size "$ELF"
LLVMBIN=$(echo "$(rustc --print sysroot)"/lib/rustlib/*/bin)
BIN=miniloader.bin; case "$1" in nolfn|without=*) BIN=miniloader-nolfn.bin;; hook) BIN=miniloader-hook.bin;; esac
$LLVMBIN/llvm-objcopy -O binary "$ELF" $BIN
python3 - <<P
d=open('$BIN','rb').read()
print('bin bytes',len(d),'last non-FF offset',max(i for i,b in enumerate(d) if b!=0xff))
P
