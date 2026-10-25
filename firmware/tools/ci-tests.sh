#!/bin/bash
# The host test suites CI runs (.github/workflows/ci.yml), one after the
# other; every suite runs, the failures are listed at the end (exit 1).
#   tools/ci-tests.sh [suite ...]      default: all of the list below
# Needs the toolchain (README "Toolchain"; the suites take ../../../toolchain,
# CI links toolchain/rustup and toolchain/cargo to its rustup install),
# python3 with Pillow, mtools and dosfstools (fatren-test, gallery-test).
# Not in the list: genuine-test (needs this unit's flash and UID dumps, not
# in git), build-variants (a separate CI step), the emulator tests (emu/,
# Renode).
cd "$(dirname "$0")"
ROOT=$(cd ../.. && pwd)
SUITES="bargraph-test bt-scpi-test dbgsrc-test dlog-test fft-test gen-test
logfmt-test meter-test scpi-test scpi-msg-test sinc-test touch-test trig-test
ui-render gallery-test fatren-test font-matrix stock-ui"
[ $# -gt 0 ] && SUITES="$*"
OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT
# stock-ui maps the decoded stock image (repo root, untracked); decode it from
# the vendor update file in git when missing.
case " $SUITES " in *" stock-ui "*)
    [ -f "$ROOT/dm40_v_2_6.dec.bin" ] ||
        python3 -I "$ROOT/stock/dm40fw.py" decode "$ROOT/stock/vendor/dm40_v_2_6.bin" "$ROOT/dm40_v_2_6.dec.bin" ;;
esac
failed=
for s in $SUITES; do
    echo "::group::$s"
    t0=$(date +%s)
    case $s in
    font-matrix) args="$OUT/font-matrix.txt" ;;
    ui-render|bargraph-test|gallery-test|stock-ui) args="$OUT/$s" ;;
    *) args= ;;
    esac
    if "./$s/run.sh" $args; then r=ok; else r=FAIL; failed="$failed $s"; fi
    echo "::endgroup::"
    echo "$s: $r ($(( $(date +%s) - t0 )) s)"
done
if [ -n "$failed" ]; then
    echo "FAILED:$failed"
    exit 1
fi
echo "all passed: $(echo $SUITES | wc -w) suites"
