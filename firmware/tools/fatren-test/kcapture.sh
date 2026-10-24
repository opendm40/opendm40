#!/bin/bash
# Capture what a real Linux kernel writes on the fw-rs USB volume
# (tools/fatren-test/captures/*.vcap.gz, replayed by run.sh "vwrite"):
# usbip_msc.py serves the volume code (vserve) on 127.0.0.1:3240, the kernel
# attaches it (vhci_hcd), GNOME / udisks mounts it as the desktop does
# (rw, flush, shortname=mixed), the scenario runs, then a natural write-back
# wait, udisksctl unmount, detach.
#   kcapture.sh <work dir of run.sh KEEP=...> <scenario>...
# Scenarios: rm, trash-cp, cp, trash-empty. Needs passwordless
# `sudo -n usbip attach|detach|port`; TCP 3240 must be free.
set -e
cd "$(dirname "$0")"
K=$(cd "$1" && pwd); shift
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
T=$K/t
[ -x $T ] || { echo "no $T: build it (run.sh KEEP=...)"; exit 1; }
# The UID of the captures (the volume serial in the boot sector is its
# CRC-32: a replay must see the same boot sector).
mkdir -p $W/src
cp $K/src/OTP.BIN $K/src/OPTBYTES.BIN $W/src/
cp captures/uid.dat $W/src/UID.BIN
FW=$W/fw500.bin
python3 -I -c "import sys; sys.path.insert(0, '.'); import vreplay; vreplay.mkfw500('$FW')"
ss -ltn | grep -q ":3240 " && { echo "TCP 3240 in use"; exit 1; }
blk() { # the block device of the vhci-attached disk
    for b in /sys/block/sd*; do
        if readlink -f $b/device | grep -q vhci_hcd; then basename $b; fi
    done
    true
}
for sc in "$@"; do
    cp $K/vs1.img $W/$sc.spi
    python3 -I usbip_msc.py $T $W/$sc.spi $K/f1m_none $W/src --capture captures/$sc.vcap.gz --fw $FW --log $W/$sc.log > $W/$sc.out 2>&1 &
    SRV=$!
    sleep 1
    sudo -n usbip attach -r 127.0.0.1 -b 1-1
    for i in $(seq 30); do D=$(blk); [ -n "$D" ] && break; sleep 0.5; done
    [ -n "$D" ] || { echo "no block device"; exit 1; }
    M=
    for i in $(seq 30); do M=$(findmnt -n -o TARGET -S /dev/$D || true); [ -n "$M" ] && break; sleep 0.5; done
    [ -n "$M" ] || { udisksctl mount -b /dev/$D >/dev/null; M=$(findmnt -n -o TARGET -S /dev/$D); }
    echo "--- $sc: /dev/$D at $M ($(findmnt -n -o OPTIONS -S /dev/$D))"
    case $sc in
    rm) rm $M/LOGS/LOG001.CSV $M/SHOTS/S001.BMP ;;
    cp) cp $FW $M/dm40fw.bin; sleep 2; cmp $FW $M/dm40fw.bin && echo "read back ok" ;;
    trash-cp) gio trash $M/SHOTS/S001.BMP; sleep 35; cp $FW $M/dm40fw.bin; sleep 2; cmp $FW $M/dm40fw.bin && echo "read back ok"; ls -la $M ;;
    # "Empty the trash" as gvfsd-trash does it (the CLI's `gio trash --empty`
    # needs the gvfs trash backend, missing here): both files removed.
    trash-empty) gio trash $M/LOGS/LOG001.CSV; sleep 35; ls -la $M/.Trash-1000/files $M/.Trash-1000/info
                 rm -f $M/.Trash-1000/files/LOG001.CSV $M/.Trash-1000/info/LOG001.CSV.trashinfo; sleep 2; ls -la $M/LOGS ;;
    esac
    sleep 40   # the kernel's own write-back (dirty_expire 30 s)
    kill -USR1 $SRV; sleep 0.5
    udisksctl unmount -b /dev/$D
    sleep 3
    M2=$(findmnt -n -o TARGET -S /dev/$D || true)
    [ -n "$M2" ] && { echo "re-mounted at $M2 after the UNIT ATTENTION:"; ls $M2 $M2/LOGS $M2/SHOTS; udisksctl unmount -b /dev/$D; sleep 2; }
    P=$(sudo -n usbip port | grep -B0 -oE "^Port [0-9]+" | grep -oE "[0-9]+" | head -1)
    sudo -n usbip detach -p $P
    sleep 1
    kill -INT $SRV; wait $SRV || true
    grep -E "STATS|WRITE.*refused|UA" $W/$sc.log | cut -c1-200
    cp $W/$sc.log captures/$sc.log
    sudo -n dmesg | tail -40 | grep -E "sd[a-z]|FAT|Buffer" | grep -v "Attached\|logical blocks\|Mode Sense\|Write Protect\|Caching\|cache" | tail -8 || true
done
