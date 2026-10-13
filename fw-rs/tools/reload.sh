#!/bin/sh
# Rebuild, return the device to stock (if our RAM firmware is running), load the
# new image over Bluetooth and wait for the USB debug port.
#   fw-rs/tools/reload.sh [bt-mac]
set -e
MAC=${1:-XX:XX:XX:XX:XX:XX}
PY=/usr/bin/python3   # needs AF_BLUETOOTH; the linuxbrew python3 lacks it
cd "$(dirname "$0")/../.."
fw-rs/build.sh >/dev/null
$PY fw-rs/tools/dm40dbg.py reset 2>/dev/null && echo "reset to stock" || true
for i in 1 2 3 4 5 6; do
    sleep 4
    if timeout 90 $PY dm40_loader.py "$MAC" fw-rs/dm40-fw.bin; then
        for j in $(seq 1 20); do
            sleep 0.5
            $PY fw-rs/tools/dm40dbg.py ping 2>/dev/null && exit 0
        done
        echo "loaded, but no USB debug port" >&2
        exit 1
    fi
    echo "BT load attempt $i failed, retrying" >&2
done
exit 1
