#!/bin/bash
# S9 reference captures: run the REAL stock V2.6 firmware in the Renode
# emulator (emu/run.py, see emu/README.md) through the scripted touch / key
# sequences of tools/ui-render/emu-seq/run*.txt (DC 0 V scope input) and
# save its scope page in the states of tools/ui-render/stock.rs emu_scenes()
# (e01..e23) as PNGs in <capture dir>; then run emu-diff.sh on it.
#   tools/ui-render/emu-capture.sh <capture dir> [run1 run2 run3]
# Each run takes about a minute of wall time (15-20 s virtual).
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
OUT=$(mkdir -p "$1" && cd "$1" && pwd)
shift
RUNS=${*:-run1 run2 run3}
for r in $RUNS; do
    S=$(mktemp)
    sed "s|@OUT@|$OUT|g" "$HERE/emu-seq/$r.txt" > "$S"
    T=$(awk 'END { print $1 + 0.3 }' "$S")
    # Per-process emulator name (emu/out/<name>.resc / .log, flash-<name>.bin):
    # parallel captures share no file.
    N="s9-$r-$$"
    (cd "$ROOT" && /usr/bin/python3 emu/run.py stock --name "$N" --run-for "$T" --script "$S" | tail -1)
    rm -f "$S" "$ROOT/emu/out/$N.resc" "$ROOT/emu/out/flash-$N.bin"
done
ls "$OUT"
