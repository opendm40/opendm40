#!/bin/sh
# Rebuild dm40-fw (flash image) and the RAM stub, return the device to stock if
# our firmware is running, then install/start it over Bluetooth (tools/flash.py).
#   fw-rs/tools/reload.sh [cargo build args...]     e.g. --features keep-screen
#   DM40_MAC overrides the Bluetooth address.
set -e
PY=/usr/bin/python3   # needs AF_BLUETOOTH; the linuxbrew python3 lacks it
cd "$(dirname "$0")/../.."
fw-rs/build.sh "$@" >/dev/null
fw-rs/stub/build.sh >/dev/null
$PY fw-rs/tools/dm40dbg.py reset 2>/dev/null && { echo "reset to stock"; sleep 6; } || true
exec $PY fw-rs/tools/flash.py
