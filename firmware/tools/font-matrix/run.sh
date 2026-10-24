#!/bin/bash
# Glyph-matrix identity check of the embedded fonts (main.rs): renders every
# glyph through every stockfont drawing path on the host and prints one
# screen hash per case to <out>. With a git revision, also renders that
# revision's fonts (a temporary worktree) and compares: any differing case
# is listed, exit 1. About 40 s per tree.
#   tools/font-matrix/run.sh <out.txt> [REV]
# e.g. after a font storage change: tools/font-matrix/run.sh /tmp/fm.txt HEAD~1
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
OUT=$1
REV=$2
B=$(mktemp -d)
trap 'rm -rf "$B"; [ -n "$WT" ] && git worktree remove --force "$WT" 2>/dev/null; true' EXIT
rustc --edition 2021 -O --cfg test -A warnings main.rs -o "$B/m"
"$B/m" > "$OUT"
echo "$(wc -l < "$OUT") cases -> $OUT"
if [ -n "$REV" ]; then
    WT="$B/wt"
    git worktree add --detach "$WT" "$REV" > /dev/null
    # The harness of this tree against that tree's fonts (fw-rs/ before the
    # firmware/ layout).
    D=firmware; [ -f "$WT/firmware/Cargo.toml" ] || D=fw-rs
    mkdir -p "$WT/$D/tools/font-matrix"; cp main.rs "$WT/$D/tools/font-matrix/main.rs"
    (cd "$WT/$D/tools/font-matrix" && rustc --edition 2021 -O --cfg test -A warnings main.rs -o "$B/r")
    "$B/r" > "$OUT.$REV"
    if cmp -s "$OUT" "$OUT.$REV"; then
        echo "identical to $REV ($(wc -l < "$OUT") cases)"
    else
        diff "$OUT.$REV" "$OUT" | head -20
        echo "DIFFERENT from $REV"
        exit 1
    fi
fi
