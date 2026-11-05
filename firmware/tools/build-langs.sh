#!/bin/sh
# The localized images: one UI language each (src/lang.rs), with every check
# of build.sh (stack depth, regions A2 / B / C, SRAM). Each is one more release
# link of the crate; the dependencies are shared.
#   tools/build-langs.sh [cargo args]    -> dm40_opendm40_v<version>-<lang>.bin
set -e
cd "$(dirname "$0")/.."
V=$(tools/fw-version.sh)
for l in zh ru it fr; do
    DM40_TDIR=target/lang DM40_OUT=target/lang/opendm40-$l.bin ./build.sh "$@" --features "$l" > "target/lang-$l.log" 2>&1 ||
        { cat "target/lang-$l.log" >&2; echo "build-langs: $l failed" >&2; exit 1; }
    cp "target/lang/opendm40-$l.bin" "dm40_opendm40_v$V-$l.bin"
    echo "$l: $(grep -m1 '^stack:' "target/lang-$l.log" | sed 's/.*headroom/stack headroom/'); $(grep -E 'region (A2|B|C) free|SRAM free' "target/lang-$l.log" | sed 's/ *(.*//; s/^ *//; s/  *= */ /; s/ bytes//' | tr '\n' ';')"
done
