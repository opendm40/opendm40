#!/bin/bash
# Host test of src/dlog.rs (compact binary DMM logs, DLG) and src/fastenc.rs
# (the fast logger's kind-3 coder + pager, tools/dlog-test/fast.rs): CSV round trip
# bit-exact vs the pre-DLG csvlog writer, random access, power cut at every
# byte, bit flips, bytes per row; then tools/dlog.py on the same file.
#   tools/dlog-test/run.sh
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
rustc --edition 2021 -O main.rs -o "$B/t"
"$B/t" "$B"
if [ -f ../dlog.py ]; then
    for i in 0 1 2 3 4 5 6 7 8; do
        python3 -I ../dlog.py "$B/t$i.dlg" -o "$B/py.csv"
        cmp "$B/py.csv" "$B/t$i.csv" || { echo "FAIL: dlog.py set $i"; exit 1; }
    done
    echo "ok: tools/dlog.py output == reference CSV (9 sets)"
    for i in 0 1 2 3 4 5 6 7; do
        python3 -I ../dlog.py "$B/s$i.dlg" -o "$B/py.csv"
        cmp "$B/py.csv" "$B/s$i.csv" || { echo "FAIL: dlog.py scope record $i"; exit 1; }
    done
    echo "ok: tools/dlog.py scope records == reference CSV (8 records)"
    for i in 0 1 2; do
        python3 -I ../dlog.py "$B/xy$i.dlg" -o "$B/py.csv"
        cmp "$B/py.csv" "$B/xy$i.csv" || { echo "FAIL: dlog.py X-Y record $i"; exit 1; }
    done
    echo "ok: tools/dlog.py X-Y records (kind 5) == reference CSV (3 records)"
    for i in 0 1 2 3 4 5; do
        python3 -I ../dlog.py "$B/f$i.dlg" -o "$B/py.csv"
        cmp "$B/py.csv" "$B/f$i.csv" || { echo "FAIL: dlog.py fast log $i"; exit 1; }
    done
    echo "ok: tools/dlog.py fast logs (kind 3) == the generated CSV (6 sets)"
    head -c $(( $(stat -c%s "$B/t6.dlg") / 2 )) "$B/t6.dlg" > "$B/torn.dlg"
    python3 -I ../dlog.py --check "$B/torn.dlg"
fi
