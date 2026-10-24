#!/bin/bash
# Host test of LOG:FORMat PACKed / DLG (src/logger.rs "Compact LOG:DATA?"):
# the logger's own round-trip tests (tools/meter-test, `fmt_*`), then the
# replies of three streams (76.8 kS/s with gaps and full-scale codes, 38.4
# kS/s across the u32 stamp wrap, AD1 640 S/s) decoded by tools/dm40scpi.py
# `log_reply` (the DLG chunks by tools/dlog.py) against the rows fed in.
#   tools/logfmt-test/run.sh
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
B=$(mktemp -d)
trap 'rm -rf "$B"' EXIT
rustc --edition 2021 --test -A warnings ../meter-test/main.rs -o "$B/t"
"$B/t" -q fmt_
LOGFMT_OUT="$B" "$B/t" -q fmt_dump
python3 -I check.py "$B"
