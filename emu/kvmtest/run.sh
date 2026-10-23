#!/bin/bash
# The real Linux USB mass-storage path against the emulator, without root:
# a KVM guest (the host's Ubuntu kernel, downloaded as its .deb because
# /boot/vmlinuz-* is root-only; the host's own modules and usbip tool) that
# attaches the emulated DM40 over USB/IP (vhci-hcd -> usb-storage -> sd ->
# vfat, and usbtmc for SCPI), starts a fast-log disk session at 38400 S/s
# and copies INFO/CALIB.BIN, BOOTDATA.BIN, LOADER.BIN and FIRMWARE.BIN
# while it runs (init: the guest's script; it prints the timings, each
# copy's exit status, cmp against copies made before the session, and the
# kernel's sense / I/O error lines).
#
#   emu/kvmtest/run.sh <app.elf> <usbip port> <LOG:DURation s> <log> ["dm40direct=1 dm40wait=240"]
#
# With the slow-bank mirror (fw-rs vdisk.rs mirror_boot, DBG:VDISK:MIR? in
# the log) the copies finish while the stream runs ("COPIED DURING THE
# STREAM"); dm40wait=<s> starts them that long into the stream. Without it,
# fw-rs vdisk.rs answers those files' slow-bank sectors NOT READY 02/04/01
# while the stream runs: Linux retries for (retries + 1) x timeout = 6 x 30 s
# from the request (scsi_lib.c scsi_cmd_runtime_exceeced); within that the
# copy is only delayed. The emulator runs ~10x slower than real time under
# load: a 6 s session stays inside, 20-25 s ones show the limit
# (dm40direct=1: an O_DIRECT read then fails EIO; a buffered `cat` survived
# because the page cache re-read the failed readahead). Needs: kvm group,
# qemu-system-x86_64, zstd, dpkg-deb, apt-get download, /usr/bin/busybox
# (static), /usr/lib/linux-tools/*/usbip.
set -e
ELF=$(realpath "$1") PORT=$2 DUR=$3 LOG=$(realpath -m "$4") EXTRA=$5
HERE=$(cd "$(dirname "$0")" && pwd)
EMU=$(dirname "$HERE")
K=${KVER:-$(ls /lib/modules | grep -- -generic | sort -V | tail -1)}
W=$EMU/out/kvmtest
mkdir -p $W
if [ ! -f $W/vmlinuz-$K ]; then
    (cd $W && apt-get download linux-image-$K >/dev/null && dpkg-deb -x linux-image-${K}_*.deb img)
    cp $W/img/boot/vmlinuz-$K $W/vmlinuz-$K
fi
R=$W/root
rm -rf $R
mkdir -p $R/{bin,proc,sys,dev,mnt,tmp,lib64,lib/modules,usr/lib/x86_64-linux-gnu,var/run}
cp /usr/bin/busybox $R/bin/busybox
for a in $($R/bin/busybox --list); do [ "$a" = busybox ] || ln -sf busybox $R/bin/$a; done
for m in drivers/usb/usbip/usbip-core drivers/usb/usbip/vhci-hcd drivers/usb/storage/usb-storage \
         drivers/usb/class/usbtmc fs/nls/nls_iso8859-1; do
    zstd -dqf /lib/modules/$K/kernel/$m.ko.zst -o $R/lib/modules/$(basename $m).ko
done
U=$(ls /usr/lib/linux-tools/*/usbip | tail -1)
cp $U $R/bin/usbip-real
cp -L /lib64/ld-linux-x86-64.so.2 $R/lib64/
cp -L /usr/lib/x86_64-linux-gnu/libudev.so.1 /usr/lib/x86_64-linux-gnu/libc.so.6 $R/usr/lib/x86_64-linux-gnu/
cp $HERE/init $R/init
chmod +x $R/init
(cd $R && find . | cpio -o -H newc 2>/dev/null | gzip -1 > $W/initrd.gz)

cd $(dirname $EMU)
setsid python3 emu/run.py fwrs --app $ELF --usbip 0.01 --usbip-port $PORT --run-for 3600 --name kvm-$PORT \
    --dmm 'vsrc:0.2,ac=0.5,f=50' > $LOG.emu 2>&1 &
PID=$!
trap 'kill -- -$PID 2>/dev/null' EXIT
until ss -ltn | grep -q ":$PORT "; do sleep 1; done
timeout 1500 qemu-system-x86_64 -enable-kvm -m 512 -nographic -no-reboot -kernel $W/vmlinuz-$K \
    -initrd $W/initrd.gz -append "console=ttyS0 quiet loglevel=4 dm40port=$PORT dm40dur=$DUR $EXTRA" \
    -netdev user,id=n0 -device virtio-net-pci,netdev=n0 > $LOG 2>&1 || true
echo "EXIT" >> $LOG
grep -E "copy|cat |dd |same|DIFF|sense|Sense|I/O error|timing out" $LOG
