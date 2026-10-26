#!/usr/bin/env bash
# Developer reload of opendm40 (native image, README.md "Developer workflow").
#
#   tools/reload.sh [options] [build args...]     e.g. --features keep-screen
#
# Default: ./build.sh [build args], check the image (tools/dm40dbg.py
# image: CRCs, size, no short last block the loader mis-decodes), upload it
# over USBTMC into the SPI disk's root as DM40FW.BIN (tools/dm40dbg.py upload
# = DBG:UPLOAD, src/upload.rs: any other dm40*.bin there is removed, the file
# is re-read and its CRC-32 checked on the meter; the opendm40 USB disk is a
# read-only generated volume; a failed upload is retried once), send DBG:UPDATE (the vendor loader installs
# the file and deletes it), wait for opendm40 to come back and check with
# DBG:BOOT? that the loader marker is the header of the new file ("installed
# OK"). A running opendm40 without DBG:UPLOAD (older build): staged on its
# writable USB disk as dm40_opendm40_v<version>.bin instead (mount, copy,
# verify, eject). That is also the name staged on the stock / loader disk
# (--from-stock); <version> as *IDN? shows it (tools/fw-version.sh). Any
# dm40*.bin name installs: the loader takes the first one in the root.
#
# Options:
#   --no-build      do not build, use the existing image
#   --image FILE    install FILE instead of opendm40.bin (implies --no-build)
#   --stock FILE    install a stock / patched stock update file the same way
#                   (e.g. ../stock/patch/build-memrw/dm40_patched.bin, built
#                   by stock/patch/kit/mkkit.sh); does not
#                   wait for opendm40 afterwards
#   --from-stock    the meter runs stock or shows the loader update screen
#                   (USB disk 28e9:028f): stage the image there, then you press
#                   "update" / power-cycle; waits for opendm40
#   --wait S        seconds to wait for opendm40 after the update (default 90)
#   -h, --help      this text
#
# Environment: PYTHON (default /usr/bin/python3).
set -euo pipefail

FW=$(cd "$(dirname "$0")/.." && pwd)
ROOT=$(dirname "$FW")
PY=${PYTHON:-/usr/bin/python3}
DBG=("$PY" -I "$FW/tools/dm40dbg.py")
# opendm40: 1209:d040; builds before it enumerated as the pid.codes test PID 0001.
FWRS_ID='1209:(d040|0001)'
STOCK_ID=28e9:028f

usage() { sed -n '2,/^set -euo/p' "$0" | sed -e '$d' -e 's/^# \{0,1\}//'; }
die() { echo "reload.sh: $*" >&2; exit 1; }

BUILD=1 IMAGE= STOCK= FROM_STOCK=0 WAIT=90 ARGS=() UPLOAD=0
while (($#)); do
    case "$1" in
        -h|--help) usage; exit 0 ;;
        --no-build) BUILD=0 ;;
        --image) (($# > 1)) || die "--image needs a file"; IMAGE=$2; BUILD=0; shift ;;
        --stock) (($# > 1)) || die "--stock needs a file"; STOCK=$2; BUILD=0; shift ;;
        --from-stock) FROM_STOCK=1 ;;
        --wait) (($# > 1)) || die "--wait needs seconds"; WAIT=$2; shift ;;
        *) ARGS+=("$1") ;;
    esac
    shift
done

# ---- USB / disk helpers (sysfs, no root) ------------------------------------

# usb_present VID:PID
usb_present() {
    local f d
    for f in /sys/bus/usb/devices/*/idVendor; do
        [ -e "$f" ] || continue
        d=$(dirname "$f")
        [[ "$(cat "$f"):$(cat "$d/idProduct" 2>/dev/null)" =~ ^($1)$ ]] && return 0
    done
    return 1
}

# find_disk VID:PID -> /dev/sdX of the USB mass-storage device with that id
find_disk() {
    local b d
    for b in /sys/class/block/sd*; do
        [ -e "$b/device" ] || continue
        [ -e "$b/partition" ] && continue
        d=$(readlink -f "$b/device")
        while [ "$d" != / ] && [ ! -f "$d/idVendor" ]; do d=$(dirname "$d"); done
        if [ -f "$d/idVendor" ] && [[ "$(cat "$d/idVendor"):$(cat "$d/idProduct")" =~ ^($1)$ ]]; then
            echo "/dev/$(basename "$b")"
            return 0
        fi
    done
    return 1
}

# fs_dev /dev/sdX -> first partition if the disk is partitioned, else the disk
fs_dev() {
    local n p
    n=$(basename "$1")
    for p in /sys/class/block/"$n"/"$n"*; do
        [ -e "$p/partition" ] && { echo "/dev/$(basename "$p")"; return; }
    done
    echo "$1"
}

# mount_fs DEV -> mount point (mounts with udisksctl if needed)
mount_fs() {
    local mp out
    mp=$(findmnt -n -o TARGET --source "$1" | head -n1 || true)
    if [ -z "$mp" ]; then
        out=$(udisksctl mount -b "$1" --no-user-interaction 2>&1) || {
            echo "$out" >&2
            die "cannot mount $1 with udisksctl (no sudo is used)"
        }
        mp=$(findmnt -n -o TARGET --source "$1" | head -n1 || true)
    fi
    [ -n "$mp" ] && [ -d "$mp" ] || die "$1: no mount point found"
    echo "$mp"
}

# wait_disk VID:PID SECONDS -> /dev/sdX (block device may lag the USB enumeration)
wait_disk() {
    local i
    for ((i = 0; i < $2 * 2; i++)); do
        find_disk "$1" && return 0
        sleep 0.5
    done
    return 1
}

# stage MOUNTPOINT IMAGE NAME: one dm40*.bin on the disk = IMAGE as NAME, verified
stage() {
    local mnt=$1 img=$2 name=$3 pages old=()
    mapfile -t old < <(find "$mnt" -maxdepth 1 -type f -iname 'dm40*.bin')
    if ((${#old[@]})); then
        echo "removing update file(s) already on the disk (the loader needs exactly one):"
        ls -l -- "${old[@]}"
        rm -f -- "${old[@]}"
    fi
    echo "copying $img -> $mnt/$name"
    cp -- "$img" "$mnt/$name"
    sync -f "$mnt/$name"
    sync
    # Drop the file's (now clean) pages from the page cache, so the compare
    # re-reads what the device returns, not the copy still in host memory.
    # (O_DIRECT reads fail with EINVAL on vfat here.)
    dd if="$mnt/$name" iflag=nocache count=0 status=none
    pages=$(fincore -n -o PAGES -- "$mnt/$name" 2>/dev/null || echo '?')
    cmp -- "$mnt/$name" "$img" || die "verify FAILED: $mnt/$name differs from $img"
    if [ "${pages// /}" = 0 ]; then
        echo "verified: re-read from the device equals $(basename "$img")"
    else
        echo "verified (page cache drop not confirmed: $pages pages cached)"
    fi
    mapfile -t old < <(find "$mnt" -maxdepth 1 -type f -iname 'dm40*.bin')
    ((${#old[@]} == 1)) || die "expected exactly one dm40*.bin on the disk, found: ${old[*]}"
}

# eject DEV MOUNTPOINT: unmount with retries; if busy, show who holds it
eject() {
    local dev=$1 mnt=$2 i err
    sync
    for i in 1 2 3 4 5; do
        if err=$(udisksctl unmount -b "$dev" --no-user-interaction 2>&1); then
            echo "unmounted $dev"
            return 0
        fi
        sleep 1
    done
    echo "WARNING: could not unmount $dev: $err" >&2
    if grep -qi busy <<<"$err"; then
        echo "processes using $mnt:" >&2
        fuser -vm "$mnt" >&2 || true
    fi
    sync
    cat >&2 <<EOF
Continuing anyway: the data is flushed (sync), so the staged file is on the
device. The risk: the meter resets under a still-mounted filesystem; if one of
the programs above writes to it later (file manager, indexer, .Trash), the
host's stale FAT view can corrupt the SPI disk. Close them, and unmount the
disk (or re-plug) once the meter is back before using the disk again.
EOF
}

# check_image FILE: the vendor format, CRCs, size, and no short last 128-byte
# block that the FS-loader mis-decodes (key position outside it, dm40fw.py
# tail_hazard) -- a bad file parks the meter on the loader screen with the app
# erased. Staging is refused for all of these (opendm40 and --stock images).
check_image() {
    "${DBG[@]}" image "$1" || die "$1 is not a valid update file (see BAD above); not staged"
    # Belt and braces for the last-block rule (independent of dm40dbg.py).
    "$PY" -I - "$ROOT/stock" "$1" <<'PY' || die "$1: short last block the FS-loader would corrupt; not staged"
import sys
sys.path.insert(0, sys.argv[1])
import dm40fw
w = dm40fw.tail_warning(open(sys.argv[2], 'rb').read())
if w:
    print(w, file=sys.stderr)
    sys.exit(1)
PY
}

# ---- native ----------------------------------------------------------------------

if [ -n "$STOCK" ]; then
    IMG=$STOCK
    NAME=$(basename "$STOCK")
    shopt -s nocasematch
    [[ $NAME == dm40*.bin ]] || NAME=dm40_stock.bin
    shopt -u nocasematch
else
    if ((BUILD)); then
        "$FW/build.sh" "${ARGS[@]}"
    elif ((${#ARGS[@]})); then
        die "build arguments given with --no-build/--image: ${ARGS[*]}"
    fi
    IMG=${IMAGE:-$FW/opendm40.bin}
    NAME=dm40_opendm40_v$("$FW/tools/fw-version.sh").bin
fi
[ -f "$IMG" ] || die "$IMG not found"
check_image "$IMG"

if usb_present "$FWRS_ID"; then
    "${DBG[@]}" boot >/dev/null 2>&1 ||
        die "the running opendm40 does not answer DBG:BOOT?: no DBG:UPDATE there."
    ((FROM_STOCK)) && echo "(--from-stock ignored: opendm40 is running, using DBG:UPDATE)"
    FROM_STOCK=0
    if R=$("${DBG[@]}" scpi 'DBG:UPLOAD?' 2>/dev/null) && [ -n "$R" ]; then
        UPLOAD=1
    else
        echo "this opendm40 has no DBG:UPLOAD (older build): staging on its USB disk"
        DISK=$(wait_disk "$FWRS_ID" 10) || die "opendm40 is running but its USB disk did not appear"
        echo "opendm40 disk: $DISK ($FWRS_ID)"
    fi
elif usb_present "$STOCK_ID"; then
    if [ -n "$STOCK" ]; then
        die "the attached DM40 is $STOCK_ID (stock \"USB Connect\" or the loader update
screen). To install $(basename "$STOCK") there, copy it onto that disk (the only
dm40*.bin), eject, press \"update\" / power-cycle."
    fi
    if ((!FROM_STOCK)); then
        die "the attached DM40 is $STOCK_ID: the STOCK firmware's \"USB Connect\" disk or
the vendor loader's update screen, not opendm40. To get opendm40 running:
  tools/reload.sh --from-stock [--no-build]
(stages $NAME on that disk; then press \"update\" on the loader screen, or
power-cycle the meter running stock). Or copy $(basename "$IMG") as $NAME
onto that disk yourself, eject, press \"update\" / power-cycle."
    fi
    DISK=$(wait_disk "$STOCK_ID" 10) || die "$STOCK_ID is present but its USB disk did not appear"
    echo "stock/loader disk: $DISK ($STOCK_ID)"
else
    die "no DM40 on USB (neither opendm40 $FWRS_ID nor stock/loader $STOCK_ID).
Plug in the cable; on stock choose \"USB Connect\"."
fi

if ((UPLOAD)); then
    # dm40dbg.py upload starts with a device clear + *CLS (Link.fresh) and
    # prints the meter's SCPI error if a step fails. One retry: an upload
    # that failed left nothing installed, and BEGIN starts over.
    if ! "${DBG[@]}" upload "$IMG"; then
        echo "reload.sh: upload attempt 1 failed (the error is above); retrying once" >&2
        sleep 1
        "${DBG[@]}" upload "$IMG" ||
            die "upload failed twice (the meter's error is above; nothing installed, the meter runs on unchanged)"
    fi
else
    DEV=$(fs_dev "$DISK")
    MNT=$(mount_fs "$DEV")
    echo "mounted $DEV at $MNT"
    stage "$MNT" "$IMG" "$NAME"
    eject "$DEV" "$MNT"
fi

if ((FROM_STOCK)); then
    cat <<EOF

Staged $NAME on the $STOCK_ID disk. Now on the meter:
  - loader update screen: press "update";
  - stock firmware: unplug USB, power the meter off and on (the stock update
    procedure); plug USB back in.
Waiting up to 300 s for opendm40 ($FWRS_ID); Ctrl-C to stop (the file stays staged).
EOF
    for ((i = 0; i < 600; i++)); do
        usb_present "$FWRS_ID" && ls /dev/usbtmc* >/dev/null 2>&1 && break
        sleep 0.5
    done
    usb_present "$FWRS_ID" || die "opendm40 did not appear within 300 s"
    sleep 2
    exec "${DBG[@]}" boot --expect "$IMG"
fi

# Device-side check: the SPI flash reports no block write error.
FLASH=$("${DBG[@]}" scpi 'DBG:FLASH?')
echo "DBG:FLASH? -> $FLASH (JEDEC id, blocks, write error, then the screenshot prep fields)"
[ "$(echo "$FLASH" | cut -d, -f3)" = 0 ] || die "the meter reports an SPI flash write error: the staged file may be
corrupt. Do not update; reset the meter (tools/dm40dbg.py reset) and run again."

if [ -n "$STOCK" ]; then
    "${DBG[@]}" update
    cat <<EOF

The loader installs $(basename "$STOCK") and starts it; the stock firmware does
not enumerate $FWRS_ID, so there is nothing to wait for here. Verify:
  - the meter shows the stock splash, then the stock UI (version in its
    settings/about page);
  - patched stock: $PY -I $ROOT/stock/patch/kit/tools/dm40_memrw.py \
      --mac <mac> read 0x08008000 12
    must show the loader marker = the update header (little-endian words):
EOF
    "${DBG[@]}" image "$STOCK" | sed -n 's/^  header.*: /      /p'
    echo "Back to opendm40: tools/reload.sh --from-stock"
    exit 0
fi

exec "${DBG[@]}" update --wait "$WAIT" --expect "$IMG"
