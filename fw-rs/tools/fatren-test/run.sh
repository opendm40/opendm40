#!/bin/bash
# Host test of src/fatren.rs, src/calbackup.rs and src/vdisk.rs on a DM40-geometry FAT16
# image (4096-byte sectors, 2 FATs, 512 root entries, 16 MiB; same cluster
# count as the device, 4087). Update files are real packed images
# (firmware/dm40fw.py pack); the "marker" is the decoded 12-byte header of the
# installed one (what the loader writes to 0x08008000).
# fatren (review 2026-10-13 H1: only the file equal to the marker is renamed):
#   1. old _DM40PRV.BIN + dm40_v_2_6.bin (LFN, installed) + DM40FW.BIN (staged)
#      + others: dry run, then live -> the installed file renamed, old backup
#      deleted and its clusters freed, the staged file untouched (staged=1),
#      fsck clean, contents unchanged;
#   2. same marker again: CLEAN, staged=1, nothing written; no marker: nothing
#      renamed; marker of DM40FW.BIN: it is renamed (replacing the backup);
#      then CLEAN, no block written;
#   3. cold boot with a staged stock file only (H1 scenario): not renamed, the
#      backup survives;
#   4. a long name with leading spaces ("  dm40 lfn only.bin") is renamed.
#   5. recovery trigger: one 0-byte DM40RCVR.BIN root entry, FAT untouched,
#      not written when a dm40*.bin exists; a normal run removes it.
#   (A long name matching while its 8.3 name does not is handled by
#   lfn_matches() but not exercised here: mtools derives the 8.3 name from it.)
# calbackup (write-once safety copy of CALIB.BIN, BOOTDATA.BIN, UID.BIN):
#   6. first boot writes the three files (nothing else: no OPTBYTES / OTP /
#      ROMBOOT / LOADER / BACKUP.TXT / _DM40PRV.BIN); fsck clean, contents equal;
#   7. second boot: PRESENT, no block written (11 block reads);
#   8. calibration changed after the copy: kept (differs=0x001), nothing written;
#      the owner deletes CALIB.BIN: made again; a foreign UID.BIN is kept;
#   9. a disk of an earlier build (all nine per-boot files): nothing written,
#      nothing deleted; a directory named UID.BIN = conflict; fatren on the
#      same volume; a nearly full disk: NOSPACE, nothing written.
# vdisk (src/vdisk.rs, the read-only information volume = USB LUN 1; every
# sector dumped through read_block, twice, from a 1 MiB flash image = the
# device dump when present + the installed firmware at 0x08004000):
#   10. fsck.fat clean, label DM40-INFO, FAT16 / 512-byte sectors / 2 KiB
#      clusters, the 7 region files equal their sources, BACKUP.TXT sizes and
#      CRC-32s match zlib, read-only attributes; write_block refused;
#   11. FIRMWARE.BIN byte-identical to the installed update file (native build,
#      stock dm40_v_2_6.bin, an unpadded 480010-byte body with a stored-as-is
#      tail, a pack --pad image), dm40fw.py info + body CRC ok; no valid loader
#      record or a hazardous short last block -> no FIRMWARE.BIN;
#   12. set_extra hook: 40 generated files after the built-ins (contents =
#      read(arg, offset)), one-shot media change; 2000 x growing files: the
#      prefix that fits is taken, fsck clean.
#   Short last 128-byte block (docs/native-boot.md "Update file format"):
#   pack --pad gives whole blocks; an unpadded body whose last block is
#   hazardous (0x1F < len < 128, key position >= len) is refused by pack, a
#   hand-built such file is flagged by dm40fw.py info/unpack and rejected by
#   dm40dbg.py image.
# screenshot (src/screenshot.rs incremental writer, driven to completion by take_on; synthetic screen):
#   12. S000.BMP taken -> S001.BMP, then S002.BMP; after deleting S000.BMP the
#      next one is S000.BMP again; fsck clean; 307266 bytes, header equal to
#      the stock S000.BMP in the disk dump (when present), every pixel equal
#      to the stub screen with bottom-up rows; the other files untouched.
#      (The stub screen mixes run rows and noise rows, so the RLE capture fills up part way and
#      the rest is read from the "GRAM" while writing: both paths.)
#   12b. power cut / screenshot::abort_on after every slice of a shot on a NOR
#      model of write_step: no FAT / root page ever erased, fsck at worst the
#      shot's lost clusters / FAT 1 ahead; a host-mounted disk refuses the shot.
# Untracked inputs (device dumps, dm40_v_2_6.bin, a native build) are used when
# present, else synthetic stand-ins of the same sizes (noted in the log).
# csvlog (src/csvlog.rs non-blocking CSV recorder, driven pass by pass; rows
# from main.rs row(i): 100 ms apart, values 1e-9..1e4, OL/REL/HOLD/HI/LO):
#   13. long recording, 3000 rows = 76 KB over 19 clusters: LOG000.CSV, fsck
#      clean, every row checked by Python's csv module; NOR model: every data
#      block erased once (appends are program-only), one root erase per commit;
#   14. interleaved with three screenshots (one started while a commit runs):
#      both BMPs pixel-exact, the CSV complete, no commit started during a shot;
#   15. host write in progress (HOST_WRITING) for 120 rows: no block written
#      meanwhile, nothing dropped; for 600 rows: FIFO overflow -> rows dropped,
#      the next row flagged GAP, file consistent;
#   15b. a PC mounts the disk mid-recording (msc::host_mounted, review 3443cea
#      H1): no block written while mounted although the same window unmounted
#      holds commits; 15 s: nothing dropped; 60 s: rows dropped, GAP, file
#      consistent;
#   16. power cut before every block write of a 1500-row recording (and a
#      program-only write cut half way): fsck finds at most a lost cluster, a
#      LOG chain longer than its size (truncated) or FAT 1 ahead of FAT 2;
#      the committed CSV is always a clean prefix; CONFIG.BIN untouched;
#      Also with the chain crossing the FAT 1 block boundary (2047 -> 2048:
#      the new entry in block 1 first, the link in block 0 last);
#   17. nearly full disk (5 free clusters): recording ends Full, file valid;
#   18. gallery: list (names, sizes vs mdir, free bytes), MMEM:CAT? text,
#      delete LOG / BMP / a long-named LOG_WI~1.CSV (LFN entries removed),
#      fsck clean; a delete between two commits of a recording; deleting the
#      file being recorded is refused; a delete while a PC has the disk
#      mounted is refused (Busy, nothing written; review 3443cea M1).
set -e
cd "$(dirname "$0")"
RUSTUP_HOME="$(cd ../../../toolchain/rustup && pwd)"; CARGO_HOME="$(cd ../../../toolchain/cargo && pwd)"; export RUSTUP_HOME CARGO_HOME; PATH="$CARGO_HOME/bin:$PATH"
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
rustc --edition 2021 -O --cfg 'feature="recovery-file"' main.rs -o "$W/t"
export MTOOLS_SKIP_CHECK=1
mkimg() {
    dd if=/dev/zero of=$1 bs=1M count=16 status=none
    mkfs.fat -F 16 -S 4096 -s 1 -f 2 -r 512 -R 1 -n "ATK-DM40" $1 >/dev/null
}
FW=../../../firmware/dm40fw.py
# mkfw <body> <out.bin>: pack a body into an update file (--pad: whole
# 128-byte blocks, as build.sh does), print its marker.
mkfw() {
    python3 -I $FW pack --pad $1 $2 2>/dev/null
    python3 -I $FW decode $2 $W/dec.tmp >/dev/null
    od -An -tx1 -N12 $W/dec.tmp | tr -d ' \n'
}
head -c 480000 /dev/urandom > $W/a.body
head -c 100000 /dev/urandom > $W/b.body
head -c 200000 /dev/urandom > $W/c.body
MA=$(mkfw $W/a.body $W/a.bin)
MB=$(mkfw $W/b.body $W/b.bin)
MC=$(mkfw $W/c.body $W/c.bin)
echo "markers: $MA $MB $MC"
head -c 50000 /dev/urandom > $W/old.bin
head -c 9000 /dev/urandom > $W/keep.bin
IMG=$W/disk.img
mkimg $IMG
mcopy -i $IMG $W/keep.bin ::CONFIG.BIN
mcopy -i $IMG $W/old.bin ::_DM40PRV.BIN
mcopy -i $IMG $W/a.bin ::dm40_v_2_6.bin
mcopy -i $IMG $W/keep.bin ::keep_dm40.bin
mcopy -i $IMG $W/b.bin ::DM40FW.BIN
mmd -i $IMG ::dm40dir.bin
mcopy -i $IMG $W/keep.bin "::A long name with spaces.txt"
echo "--- before"; mdir -i $IMG :: | grep -E "BIN|bin|TXT|txt"
"$W/t" $IMG dry $MA | tee $W/r0
grep -q "status=DryRun" $W/r0 && grep -q "changed blocks: \[\]" $W/r0 && echo "ok: dry run writes nothing" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG live $MA | tee $W/r1
grep -q "status=Renamed" $W/r1 && grep -q "staged=1" $W/r1 && grep -q "others=1" $W/r1 && echo "ok: installed file renamed, staged one counted" || { echo "FAIL at line $LINENO"; exit 1; }
echo "--- after 1"; mdir -i $IMG :: | grep -E "BIN|bin|TXT|txt"
fsck.fat -n $IMG | tail -1
mcopy -n -i $IMG ::_DM40PRV.BIN $W/o && cmp $W/o $W/a.bin && echo "ok: _DM40PRV.BIN == former dm40_v_2_6.bin" || { echo "FAIL at line $LINENO"; exit 1; }
mcopy -n -i $IMG ::DM40FW.BIN $W/o && cmp $W/o $W/b.bin && echo "ok: DM40FW.BIN (staged) untouched" || { echo "FAIL at line $LINENO"; exit 1; }
mcopy -n -i $IMG ::CONFIG.BIN $W/o && cmp $W/o $W/keep.bin && echo "ok: CONFIG.BIN untouched" || { echo "FAIL at line $LINENO"; exit 1; }
if mdir -i $IMG :: | grep -qi "dm40_v_2_6"; then echo "FAIL: long name left"; exit 1; fi
"$W/t" $IMG live $MA | tee $W/r2
grep -q "status=Clean" $W/r2 && grep -q "staged=1" $W/r2 && grep -q "changed blocks: \[\]" $W/r2 && echo "ok: same marker again: staged file left, nothing written" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG live | tee $W/r2b
grep -q "status=Clean" $W/r2b && grep -q "changed blocks: \[\]" $W/r2b && echo "ok: no valid marker: nothing renamed" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG live $MB | tee $W/r2c
grep -q "status=Renamed" $W/r2c && echo "ok: DM40FW.BIN renamed once it is the installed image" || { echo "FAIL at line $LINENO"; exit 1; }
echo "--- after 2"; mdir -i $IMG :: | grep -E "BIN|bin|TXT|txt"
fsck.fat -n $IMG | tail -1
mcopy -n -i $IMG ::_DM40PRV.BIN $W/o && cmp $W/o $W/b.bin && echo "ok: backup is now the former DM40FW.BIN" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG live $MB | tee $W/r3
grep -q "status=Clean" $W/r3 && grep -q "changed blocks: \[\]" $W/r3 && echo "ok: third run clean, nothing written" || { echo "FAIL at line $LINENO"; exit 1; }
echo "--- H1: cold boot with a staged stock file, fw-rs (MB) installed"
mcopy -i $IMG $W/c.bin ::dm40_v_2_6.bin
"$W/t" $IMG live $MB | tee $W/r3b
grep -q "status=Clean" $W/r3b && grep -q "staged=1" $W/r3b && grep -q "changed blocks: \[\]" $W/r3b && echo "ok: staged file not renamed" || { echo "FAIL at line $LINENO"; exit 1; }
mcopy -n -i $IMG ::_DM40PRV.BIN $W/o && cmp $W/o $W/b.bin && echo "ok: backup kept" || { echo "FAIL at line $LINENO"; exit 1; }
mcopy -n -i $IMG ::dm40_v_2_6.bin $W/o && cmp $W/o $W/c.bin && echo "ok: staged file intact" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG live $MC | grep -q "status=Renamed" && echo "ok: renamed after the loader installed it" || { echo "FAIL at line $LINENO"; exit 1; }
fsck.fat -n $IMG | tail -1
IMG2=$W/disk2.img
mkimg $IMG2
mcopy -i $IMG2 $W/b.bin "::  dm40 lfn only.bin"
mdir -i $IMG2 :: | grep -i lfn
"$W/t" $IMG2 live $MB | tee $W/r4
grep -q "status=Renamed" $W/r4 && echo "ok: long-name file renamed" || { echo "FAIL at line $LINENO"; exit 1; }
fsck.fat -n $IMG2 | tail -1
echo "--- recovery trigger"
IMG3=$W/disk3.img
mkimg $IMG3
mcopy -i $IMG3 $W/keep.bin ::CONFIG.BIN
mcopy -i $IMG3 $W/keep.bin ::DEL.TXT
mdel -i $IMG3 ::DEL.TXT                   # leaves a 0xE5 slot before the end
cp $IMG3 $W/pre.img
"$W/t" $IMG3 trigger | tee $W/r5
grep -q "trigger=Created" $W/r5 && grep -q "changed blocks: \[5\]" $W/r5 && echo "ok: trigger created, only root block 5 written" || { echo "FAIL at line $LINENO"; exit 1; }
mdir -i $IMG3 :: | grep -E "BIN"
fsck.fat -n $IMG3 | tail -1
[ "$(mdir -i $IMG3 :: | grep DM40RCVR | awk '{print $3}')" = 0 ] && echo "ok: DM40RCVR.BIN is 0 bytes" || { echo "FAIL at line $LINENO"; exit 1; }
cmp -s <(dd if=$W/pre.img bs=4096 skip=1 count=4 status=none) <(dd if=$IMG3 bs=4096 skip=1 count=4 status=none) && echo "ok: FAT tables untouched" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG3 trigger | grep -q "trigger=AlreadyThere" && echo "ok: second trigger = AlreadyThere" || { echo "FAIL at line $LINENO"; exit 1; }
IMG4=$W/disk4.img
mkimg $IMG4
mcopy -i $IMG4 $W/b.bin ::dm40_staged.bin
"$W/t" $IMG4 trigger | grep -q "trigger=AlreadyThere" && echo "ok: no trigger written when a dm40*.bin is already staged" || { echo "FAIL at line $LINENO"; exit 1; }
mcopy -i $IMG3 $W/b.bin ::dm40_v_2_6.bin
"$W/t" $IMG3 live $MB | tee $W/r6
grep -q "status=Renamed" $W/r6 && grep -q "fake_deleted=true" $W/r6 && echo "ok: normal boot removes DM40RCVR.BIN and renames the image" || { echo "FAIL at line $LINENO"; exit 1; }
mdir -i $IMG3 :: | grep -E "BIN"
fsck.fat -n $IMG3 | tail -1

echo "=== calbackup (write-once safety copy: CALIB.BIN, BOOTDATA.BIN, UID.BIN)"
# fsckok <img>: fsck.fat -n exit 0 and no "lost"/"differ" message.
fsckok() {
    fsck.fat -n $1 > $W/fsck.out 2>&1 || { cat $W/fsck.out; echo "FAIL: fsck"; exit 1; }
    if grep -qiE "free cluster|differ|lost|orphan|reclaim" $W/fsck.out; then cat $W/fsck.out; echo "FAIL: fsck"; exit 1; fi
    tail -1 $W/fsck.out
}
# same3 <img> <dir>: the three safety-copy files on <img> equal <dir>'s.
same3() {
    for f in CALIB.BIN BOOTDATA.BIN UID.BIN; do
        mcopy -n -i $1 ::$f $W/o && cmp $W/o $2/$f || { echo "FAIL: $f differs from $2"; exit 1; }
    done
    echo "ok: CALIB.BIN, BOOTDATA.BIN, UID.BIN equal $(basename $2)"
}
# Old files (earlier builds' per-boot backup) and anything else: not written.
noold() {
    for f in OPTBYTES.BIN OTP.BIN ROMBOOT.BIN LOADER.BIN BACKUP.TXT _DM40PRV.BIN; do
        mdir -i $1 ::$f >/dev/null 2>&1 && { echo "FAIL: $f written"; exit 1; }
    done
    echo "ok: no OPTBYTES / OTP / ROMBOOT / LOADER / BACKUP.TXT / _DM40PRV.BIN written"
}
mkdir $W/src
# Sources: slices of the (untracked) device dumps when present, else
# synthetic stand-ins of the same sizes (review 258c833 L5: the suite must
# run in a clean checkout). Synthetic data is seeded random; the bytes the
# tests below change (CALIB 400 / 500, LOADER 0x1F000) are forced to 0 so
# every change is one.
DUMPS=../../../firmware/re/dumps
DUMP=$DUMPS/full-20261013/flash_08000000_1MB.bin
python3 -I - $W/src <<'PY'
import random, sys
r = random.Random(258)
for name, n in [('CALIB.BIN', 4096), ('BOOTDATA.BIN', 16384), ('OPTBYTES.BIN', 16), ('OTP.BIN', 528),
                ('ROMBOOT.BIN', 16384), ('LOADER.BIN', 131072)]:
    d = bytearray(r.randbytes(n))
    for o in {'CALIB.BIN': [400, 500], 'LOADER.BIN': [0x1F000]}.get(name, []):
        d[o] = 0
    open(sys.argv[1] + '/' + name, 'wb').write(d)
PY
if [ -f $DUMPS/dump_settings.bin ]; then head -c 4096 $DUMPS/dump_settings.bin > $W/src/CALIB.BIN; else echo "note: no $DUMPS/dump_settings.bin, synthetic CALIB.BIN"; fi
if [ -f $DUMP ]; then
    dd if=$DUMP of=$W/src/ROMBOOT.BIN bs=4096 skip=0 count=4 status=none
    dd if=$DUMP of=$W/src/LOADER.BIN bs=4096 skip=$((0xA0000 / 4096)) count=32 status=none
    dd if=$DUMP of=$W/src/BOOTDATA.BIN bs=4096 skip=$((0xA4000 / 4096)) count=4 status=none
else
    echo "note: no $DUMP, synthetic ROMBOOT.BIN / LOADER.BIN / BOOTDATA.BIN"
fi
[ $(stat -c %s $W/src/ROMBOOT.BIN) = 16384 ] && [ $(stat -c %s $W/src/LOADER.BIN) = 131072 ] || { echo "FAIL: dump slices"; exit 1; }
head -c 12 /dev/urandom > $W/src/UID.BIN
for f in option_bytes_1FFFC000.bin:OPTBYTES.BIN otp_1FFF7800.bin:OTP.BIN; do
    if [ -f $DUMPS/full-20261013/${f%%:*} ]; then cp $DUMPS/full-20261013/${f%%:*} $W/src/${f##*:}; else echo "note: no ${f%%:*}, synthetic ${f##*:}"; fi
done
IMG5=$W/disk5.img
mkimg $IMG5
mcopy -i $IMG5 $W/keep.bin ::CONFIG.BIN
mcopy -i $IMG5 $W/keep.bin ::DEL.TXT
mdel -i $IMG5 ::DEL.TXT
"$W/t" $IMG5 backup $W/src | tee $W/b1
grep -q "backup=Written written=0x007 conflict=0x000 differs=0x000" $W/b1 && echo "ok: first boot writes the three files" || { echo "FAIL at line $LINENO"; exit 1; }
mdir -i $IMG5 ::
fsckok $IMG5
same3 $IMG5 $W/src
noold $IMG5
"$W/t" $IMG5 backup $W/src | tee $W/b2
grep -q "backup=Present written=0x000 conflict=0x000 differs=0x000" $W/b2 && grep -q "changed blocks: \[\]" $W/b2 && echo "ok: second boot: SAFECOPY_PRESENT, nothing written" || { echo "FAIL at line $LINENO"; exit 1; }
# Boot sector, root, the 1 + 4 + 1 data blocks of the three files and FAT 1
# once per file (each chain walk starts with an empty FAT cache).
grep -qE "reads=11 data_reads=\[[0-9]+, [0-9]+, [0-9]+, [0-9]+, [0-9]+, [0-9]+\]$" $W/b2 && echo "ok: present: 11 block reads" || { echo "FAIL at line $LINENO"; exit 1; }
echo "--- calibration changed after the first copy: never overwritten, reported"
cp -r $W/src $W/src0
printf '\x42' | dd of=$W/src/CALIB.BIN bs=1 seek=400 conv=notrunc status=none
"$W/t" $IMG5 backup $W/src | tee $W/b3
grep -q "backup=Present written=0x000 conflict=0x000 differs=0x001" $W/b3 && grep -q "changed blocks: \[\]" $W/b3 && echo "ok: CALIB.BIN kept (differs=0x001), nothing written" || { echo "FAIL at line $LINENO"; exit 1; }
same3 $IMG5 $W/src0
echo "--- the owner deletes CALIB.BIN: made again from the flash as it is now"
mdel -i $IMG5 ::CALIB.BIN
"$W/t" $IMG5 backup $W/src | tee $W/b4
grep -q "backup=Written written=0x001 conflict=0x000 differs=0x000" $W/b4 && echo "ok: missing CALIB.BIN written" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG5
same3 $IMG5 $W/src
[ $(mdir -i $IMG5 :: | grep -c "CALIB") = 1 ] && echo "ok: one CALIB.BIN entry" || { echo "FAIL at line $LINENO"; exit 1; }
echo "--- a foreign / short copy is kept as it is (write-once)"
head -c 100000 /dev/urandom > $W/junk
mcopy -o -i $IMG5 $W/junk ::UID.BIN
"$W/t" $IMG5 backup $W/src | tee $W/b5
grep -q "backup=Present written=0x000 conflict=0x000 differs=0x004" $W/b5 && grep -q "changed blocks: \[\]" $W/b5 && echo "ok: foreign UID.BIN kept, differs=0x004" || { echo "FAIL at line $LINENO"; exit 1; }
mcopy -n -i $IMG5 ::UID.BIN $W/o && cmp $W/o $W/junk && echo "ok: UID.BIN untouched" || { echo "FAIL at line $LINENO"; exit 1; }
echo "--- disk of an earlier build (all nine per-boot files): nothing written, nothing deleted"
IMG6=$W/disk6.img
mkimg $IMG6
for f in CALIB.BIN BOOTDATA.BIN UID.BIN OPTBYTES.BIN OTP.BIN ROMBOOT.BIN LOADER.BIN; do mcopy -i $IMG6 $W/src0/$f ::$f; done
echo "old text" > $W/oldtxt; mcopy -i $IMG6 $W/oldtxt ::BACKUP.TXT; mcopy -i $IMG6 $W/old.bin ::_DM40PRV.BIN
mdir -i $IMG6 :: > $W/before6
"$W/t" $IMG6 backup $W/src | tee $W/b6
grep -q "backup=Present written=0x000 conflict=0x000 differs=0x001" $W/b6 && grep -q "changed blocks: \[\]" $W/b6 && echo "ok: old disk: PRESENT, nothing written" || { echo "FAIL at line $LINENO"; exit 1; }
mdir -i $IMG6 :: | diff - $W/before6 && echo "ok: directory unchanged (old files left)" || { echo "FAIL at line $LINENO"; exit 1; }
echo "--- a directory named UID.BIN = conflict; fatren on the same volume"
IMG7=$W/disk7.img
mkimg $IMG7
mmd -i $IMG7 ::UID.BIN
mcopy -i $IMG7 $W/a.bin ::dm40_v_2_6.bin
mcopy -i $IMG7 $W/old.bin ::_DM40PRV.BIN
"$W/t" $IMG7 live $MA | grep -q "status=Renamed" && echo "ok: fatren on the backup volume" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG7 backup $W/src | tee $W/b7
grep -q "backup=Written written=0x003 conflict=0x004" $W/b7 && echo "ok: directory UID.BIN = conflict, CALIB + BOOTDATA written" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG7
"$W/t" $IMG7 backup $W/src | grep -q "backup=Present written=0x000 conflict=0x004" && echo "ok: then present" || { echo "FAIL at line $LINENO"; exit 1; }
mcopy -n -i $IMG7 ::_DM40PRV.BIN $W/o && cmp $W/o $W/a.bin && echo "ok: fatren's _DM40PRV.BIN untouched" || { echo "FAIL at line $LINENO"; exit 1; }
echo "--- nearly full disk (3 free clusters < 6 needed): NOSPACE, nothing written"
IMG9=$W/disk9.img
mkimg $IMG9
FREE=$(python3 -I - $IMG9 <<'PY'
import struct, sys
d = open(sys.argv[1], 'rb').read()
bps, spc, rsvd, nf, rootn, tot16, fsz, tot32 = (struct.unpack_from('<H', d, 0x0B)[0], d[0x0D],
    struct.unpack_from('<H', d, 0x0E)[0], d[0x10], struct.unpack_from('<H', d, 0x11)[0],
    struct.unpack_from('<H', d, 0x13)[0], struct.unpack_from('<H', d, 0x16)[0], struct.unpack_from('<I', d, 0x20)[0])
data = rsvd + nf * fsz + rootn * 32 // bps
clusters = ((tot16 or tot32) - data) // spc
print(sum(1 for c in range(2, clusters + 2) if struct.unpack_from('<H', d, rsvd * bps + 2 * c)[0] == 0))
PY
)
head -c $(( (FREE - 3) * 4096 )) /dev/zero > $W/fill
mcopy -i $IMG9 $W/fill ::FILL.DAT
"$W/t" $IMG9 backup $W/src | tee $W/b9
grep -q "backup=NoSpace written=0x000" $W/b9 && grep -q "changed blocks: \[\]" $W/b9 && echo "ok: NOSPACE, nothing written" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG9
mdel -i $IMG9 ::FILL.DAT
"$W/t" $IMG9 backup $W/src | grep -q "backup=Written written=0x007" && echo "ok: written once there is space" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG9

echo "=== vdisk: information volume (USB LUN 1), generated from the MCU memory"
# mkflash <fw.bin> <flash.out>: what the installed image looks like in flash
# from 0x08004000: the decoded body, the loader's 12-byte marker (= the file's
# decoded header) at +0x4000.
mkflash() {
    python3 -I - $FW $1 $2 <<'PY'
import importlib.util, sys
spec = importlib.util.spec_from_file_location('dm40fw', sys.argv[1]); f = importlib.util.module_from_spec(spec); spec.loader.exec_module(f)
dec = f.decode(open(sys.argv[2], 'rb').read())
w0, length, w2, body = f.split(dec)
body = bytearray(body)
body[f.MARKER_OFFSET:f.MARKER_OFFSET + 12] = dec[:12]
open(sys.argv[3], 'wb').write(body)
PY
}
# fwok <file> <orig.bin>: <file> is byte-identical to <orig.bin>, and
# dm40fw.py reads it as a valid update file (header words, body CRC).
fwok() {
    cmp $1 $2 || { echo "FAIL: $1 != $2"; exit 1; }
    python3 -I $FW info $1
    python3 -I - $FW $1 <<'PY' || { echo "FAIL: dm40fw.py check"; exit 1; }
import importlib.util, sys
spec = importlib.util.spec_from_file_location('dm40fw', sys.argv[1]); f = importlib.util.module_from_spec(spec); spec.loader.exec_module(f)
w0, length, w2, body = f.split(f.decode(open(sys.argv[2], 'rb').read()))
assert w0 == f.header_word0(length, w2), 'word0'
assert w2 >> 16 == 1 and w2 & 0xffff == f.body_checksum(body), 'word2 / body CRC'
print('header words and body CRC ok')
PY
    echo "ok: FIRMWARE.BIN byte-identical to $(basename $2), valid update file"
}
STOCK=../../../dm40_v_2_6.bin
NATIVE=../../dm40-fw-native.bin
# Untracked inputs (L5): stand-ins when absent.
# mkbody <len> <out>: random body with the 12 marker-gap bytes at 0x4000 0xFF.
mkbody() {
    head -c $1 /dev/urandom > $2
    printf '\377\377\377\377\377\377\377\377\377\377\377\377' | dd of=$2 bs=1 seek=$((0x4000)) conv=notrunc status=none
}
if [ ! -f $STOCK ]; then
    echo "note: no $STOCK, synthetic stand-in (body 495128 bytes)"
    mkbody 495128 $W/stock.body && python3 -I $FW pack --pad $W/stock.body $W/stock.bin 2>/dev/null && STOCK=$W/stock.bin
fi
if [ ! -f $NATIVE ]; then
    echo "note: no $NATIVE (./build.sh native), synthetic stand-in"
    mkbody 289000 $W/native.body && python3 -I $FW pack --pad $W/native.body $W/native.bin 2>/dev/null && NATIVE=$W/native.bin
fi
# t: packed WITHOUT --pad: (12 + 480010) % 128 = 22 <= 0x1F, a stored-as-is
# tail the FS-loader also leaves alone (not a hazard), reproduced as is.
head -c 480010 /dev/urandom > $W/t.body
# The 12 marker-gap bytes (body 0x4000..0x400B) are 0xFF in every real build
# (build.sh --gap-fill, the vendor's dm40_v_2_6.bin); the regenerated file
# always has 0xFF there (a file with other bytes differs only there and is
# still valid: the loader's body CRC excludes them).
printf '\377\377\377\377\377\377\377\377\377\377\377\377' | dd of=$W/t.body bs=1 seek=$((0x4000)) conv=notrunc status=none
python3 -I $FW pack $W/t.body $W/t.bin >/dev/null
for f in $STOCK $NATIVE; do [ -f $f ] || { echo "FAIL: $f missing"; exit 1; }; done
mkflash $STOCK $W/flash_stock
mkflash $NATIVE $W/flash_native
mkflash $W/t.bin $W/flash_t
# mkflash1m <fwflash|none> <out>: a 1 MiB internal-flash image: the device
# dump when present (else ROMBOOT / LOADER / CALIB of $W/src at their
# addresses), the installed firmware (mkflash output) at 0x08004000 (none:
# 0xFF there, no valid loader record).
mkflash1m() {
    python3 -I - $DUMP $W/src $1 $2 <<'PY'
import os, sys
dump, src, fw, out = sys.argv[1:]
if os.path.exists(dump):
    f = bytearray(open(dump, 'rb').read())
else:
    f = bytearray(b'\xff' * (1 << 20))
    for name, a in [('ROMBOOT.BIN', 0), ('LOADER.BIN', 0xA0000), ('CALIB.BIN', 0xC0000)]:
        d = open(src + '/' + name, 'rb').read(); f[a:a + len(d)] = d
assert len(f) == 1 << 20
f[0x4000:0xA0000] = b'\xff' * (0xA0000 - 0x4000)
if fw != 'none':
    b = open(fw, 'rb').read(); f[0x4000:0x4000 + len(b)] = b
open(out, 'wb').write(f)
PY
}
# vdisk <flash1m> <img> [extra n size]: the whole volume through read_block.
vdisk() {
    local fl=$1 img=$2; shift 2
    "$W/t" $img vdisk $fl $W/src/UID.BIN $W/src/OTP.BIN $W/src/OPTBYTES.BIN "$@" | tee $W/vd.out
    grep -q "present_before_init=false" $W/vd.out && grep -q "media_changed_after_init=false" $W/vd.out || { echo "FAIL: init"; exit 1; }
    grep -q "read_only=true write_block=false read_past_end=false" $W/vd.out && echo "ok: read-only (write_block refused, nothing past the end)" || { echo "FAIL: read-only"; exit 1; }
}
# vdok <img> <flash1m>: fsck clean, label, geometry, every region file equal
# to its source, BACKUP.TXT CRC-32s, read-only attributes.
vdok() {
    fsckok $1
    mdir -i $1 :: | tee $W/vdir | head -3 | grep -q "Volume in drive : is DM40-INFO" && echo "ok: label DM40-INFO" || { echo "FAIL: label"; exit 1; }
    python3 -I - $1 <<'PY' || { echo "FAIL: geometry"; exit 1; }
import struct, sys
d = open(sys.argv[1], 'rb').read(512)
bps, spc, rsvd, nf, rootn, tot16, media, fsz, tot32 = struct.unpack_from('<HBHBHHBH', d, 0x0B) + struct.unpack_from('<I', d, 0x20)
data = rsvd + nf * fsz + rootn * 32 // bps
clusters = ((tot16 or tot32) - data) // spc
assert (bps, spc, nf, media) == (512, 4, 2, 0xF8) and 4085 <= clusters < 65525 and d[0x36:0x3E] == b'FAT16   ', (bps, spc, clusters)
assert fsz * bps // 2 >= clusters + 2 and d[510:512] == b'\x55\xaa' and d[0x26] == 0x29
print('ok: FAT16, 512-byte sectors, %d clusters of %d bytes, %.1f MiB' % (clusters, spc * bps, (tot16 or tot32) * bps / 1048576))
PY
    python3 -I - $2 $W/src $1 <<'PY' || { echo "FAIL: region contents"; exit 1; }
import subprocess, sys, zlib
flash, src, img = sys.argv[1:]
f = open(flash, 'rb').read()
regions = {'CALIB.BIN': f[0xC0000:0xC1000], 'BOOTDATA.BIN': f[0xA4000:0xA8000], 'ROMBOOT.BIN': f[:0x4000],
           'LOADER.BIN': f[0xA0000:0xC0000], 'UID.BIN': open(src + '/UID.BIN', 'rb').read(),
           'OTP.BIN': open(src + '/OTP.BIN', 'rb').read(), 'OPTBYTES.BIN': open(src + '/OPTBYTES.BIN', 'rb').read()}
def get(n):
    return subprocess.run(['mtype', '-i', img, '::' + n], capture_output=True, check=True).stdout
txt = get('BACKUP.TXT').decode()
lines = txt.splitlines()
h = lines.index('file         address     bytes  crc32')
rows = {l.split()[0]: l.split() for l in lines[h + 1:lines.index('', h)]}
for n, want in regions.items():
    got = get(n)
    assert got == want, n
    r = rows[n]
    assert int(r[2]) == len(got) and int(r[3], 16) == zlib.crc32(got), (n, r)
if 'FIRMWARE.BIN' in rows:
    got = get('FIRMWARE.BIN'); r = rows['FIRMWARE.BIN']
    assert int(r[2]) == len(got) and int(r[3], 16) == zlib.crc32(got), r
print('ok: 7 region files equal their sources, BACKUP.TXT sizes + CRC-32s match (%d lines)' % len(rows))
PY
    mattrib -i $1 ::CALIB.BIN | grep -qE "^ *A? *R|  R " && echo "ok: read-only attribute" || { mattrib -i $1 ::CALIB.BIN; echo "FAIL: attribute"; exit 1; }
}
for f in $STOCK $NATIVE; do [ -f $f ] || { echo "FAIL: $f missing"; exit 1; }; done
mkflash $STOCK $W/flash_stock
mkflash $NATIVE $W/flash_native
mkflash $W/t.bin $W/flash_t
for k in stock native t; do mkflash1m $W/flash_$k $W/f1m_$k; done
VD=$W/vd.img
vdisk $W/f1m_native $VD
vdok $VD $W/f1m_native
mcopy -n -i $VD ::FIRMWARE.BIN $W/fw.bin && fwok $W/fw.bin $NATIVE
mtype -i $VD ::BACKUP.TXT
[ $(grep -c " BIN\| TXT" $W/vdir) = 9 ] && echo "ok: 9 files" || { echo "FAIL at line $LINENO"; exit 1; }
vdisk $W/f1m_stock $VD
vdok $VD $W/f1m_stock
mcopy -n -i $VD ::FIRMWARE.BIN $W/fw.bin && fwok $W/fw.bin $STOCK
vdisk $W/f1m_t $VD
vdok $VD $W/f1m_t
mcopy -n -i $VD ::FIRMWARE.BIN $W/fw.bin && fwok $W/fw.bin $W/t.bin
echo "--- no valid loader record: no FIRMWARE.BIN"
mkflash1m none $W/f1m_none
vdisk $W/f1m_none $VD
vdok $VD $W/f1m_none
mdir -i $VD ::FIRMWARE.BIN >/dev/null 2>&1 && { echo "FAIL at line $LINENO: FIRMWARE.BIN"; exit 1; }
mtype -i $VD ::BACKUP.TXT | grep -q "no FIRMWARE.BIN (no valid loader record" && echo "ok: none, BACKUP.TXT says why" || { echo "FAIL at line $LINENO"; exit 1; }
echo "--- set_extra hook: 40 generated files (LOG000..039.CSV, 3000 * (i + 1) bytes) after the built-ins"
vdisk $W/f1m_native $VD extra 40 3000
grep -q "set_extra taken=40" $W/vd.out && grep -q "media_changed=true again=false" $W/vd.out && echo "ok: taken, one-shot media change" || { echo "FAIL at line $LINENO"; exit 1; }
vdok $VD $W/f1m_native
python3 -I - $VD <<'PY' || { echo "FAIL: extra files"; exit 1; }
import subprocess, sys
for i in range(40):
    got = subprocess.run(['mtype', '-i', sys.argv[1], '::LOG%03d.CSV' % i], capture_output=True, check=True).stdout
    want = bytes(b'0123456789,\n'[(i * 5 + o) % 12] for o in range(3000 * (i + 1)))
    assert got == want, i
print('ok: 40 extra files, contents = read(arg, offset)')
PY
mcopy -n -i $VD ::FIRMWARE.BIN $W/fw.bin && fwok $W/fw.bin $NATIVE
echo "--- set_extra: more than fits (2000 x 64 KiB): a prefix is taken, the volume stays consistent"
vdisk $W/f1m_native $VD extra 2000 65536
# File i takes 32 * (i + 1) clusters: 44 files = 31680 of the 32768, the
# 45th (+ 1440) no longer fits after the ~300 clusters of the built-ins.
grep -q "set_extra taken=44$" $W/vd.out && echo "ok: taken=44 of 2000 (64 MiB data area)" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $VD

echo "--- short last 128-byte block: padded images give whole blocks; a hazardous one gives no FIRMWARE.BIN"
# p: a body whose unpadded file would end in a short block; pack refuses it
# when that block is hazardous, --pad always gives whole blocks, and the
# regenerated FIRMWARE.BIN is the same whole-block file (decodes to the flash
# body incl. the 0xFF padding: fwok = byte-identical + header/CRC check).
head -c 393752 /dev/urandom > $W/p.body       # (12 + 393752) % 128 = 36
printf '\377\377\377\377\377\377\377\377\377\377\377\377' | dd of=$W/p.body bs=1 seek=$((0x4000)) conv=notrunc status=none
python3 -I $FW pack --pad $W/p.body $W/p.bin 2>/dev/null
[ $(( $(stat -c %s $W/p.bin) % 128 )) -eq 0 ] && echo "ok: pack --pad: $(stat -c %s $W/p.bin) bytes = whole 128-byte blocks" || { echo "FAIL at line $LINENO"; exit 1; }
for f in $W/t.bin $W/a.bin $W/b.bin $W/c.bin $NATIVE; do
    python3 -I $FW info $f 2>&1 | grep -q "warning: the last 128-byte block" && { echo "FAIL at line $LINENO: $f hazardous"; exit 1; }
done
echo "ok: no packed test image has a hazardous last block"
# haz: an OLD-STYLE unpadded file (built by hand: pack refuses it) whose last
# block is 84 bytes with [0x1F] = 0xBF -> key position 0x5F >= 84. Installed
# (flash + marker) it must not be regenerated (FwFile::from_flash -> None),
# and the tools must flag it.
python3 -I - $FW $W/haz.body $W/haz.bin <<'PY'
import importlib.util, os, struct, sys
spec = importlib.util.spec_from_file_location('dm40fw', sys.argv[1]); f = importlib.util.module_from_spec(spec); spec.loader.exec_module(f)
n = 128 * 3100 + 84 - 12
body = bytearray(os.urandom(n)); body[f.MARKER_OFFSET:f.MARKER_OFFSET + 12] = b'\xff' * 12
body[128 * 3100 + 0x1F - 12] = 0xBF
w2 = 0x10000 | f.body_checksum(bytes(body))
dec = struct.pack('<3I', f.header_word0(n, w2), n, w2) + bytes(body)
assert f.tail_hazard(dec) == (84, 0xBF, 0x5F)
open(sys.argv[2], 'wb').write(body)
open(sys.argv[3], 'wb').write(f.encode(dec))
PY
python3 -I $FW pack $W/haz.body $W/o 2>&1 | grep -q "refusing to pack" && echo "ok: pack refuses the hazardous body without --pad" || { echo "FAIL at line $LINENO"; exit 1; }
python3 -I $FW info $W/haz.bin 2>&1 | grep -q "warning: the last 128-byte block is short (84 bytes)" && echo "ok: dm40fw.py info warns" || { echo "FAIL at line $LINENO"; exit 1; }
python3 -I $FW unpack $W/haz.bin $W/o 2>&1 | grep -q "warning: the last 128-byte block" && echo "ok: dm40fw.py unpack warns" || { echo "FAIL at line $LINENO"; exit 1; }
python3 -I ../dm40dbg.py image $W/haz.bin > $W/h2 && { echo "FAIL at line $LINENO: dm40dbg image accepted it"; exit 1; }
grep -q "BAD: short last block (84 bytes)" $W/h2 && echo "ok: dm40dbg.py image rejects it (reload.sh refuses to stage)" || { echo "FAIL at line $LINENO"; exit 1; }
python3 -I ../dm40dbg.py image $W/p.bin | grep -q "last 128-byte block: 128 bytes (whole blocks)" && echo "ok: dm40dbg.py image: padded file = whole blocks" || { echo "FAIL at line $LINENO"; exit 1; }
mkflash $W/p.bin $W/flash_p
mkflash1m $W/flash_p $W/f1m_p
vdisk $W/f1m_p $VD
mcopy -n -i $VD ::FIRMWARE.BIN $W/fw.bin && fwok $W/fw.bin $W/p.bin
[ $(( $(stat -c %s $W/fw.bin) % 128 )) -eq 0 ] && echo "ok: FIRMWARE.BIN of a padded image is whole 128-byte blocks" || { echo "FAIL at line $LINENO"; exit 1; }
mkflash $W/haz.bin $W/flash_haz
mkflash1m $W/flash_haz $W/f1m_haz
vdisk $W/f1m_haz $VD
vdok $VD $W/f1m_haz
mdir -i $VD ::FIRMWARE.BIN >/dev/null 2>&1 && { echo "FAIL at line $LINENO: FIRMWARE.BIN of a hazardous image"; exit 1; }
echo "ok: hazardous installed image: no FIRMWARE.BIN"

echo "=== screenshot"
# bmpok <img> <name>: size, header (vs the stock dump), pixels vs lcd::pattern.
bmpok() {
    mcopy -n -i $1 ::$2 $W/shot.bmp
    python3 -I - $W/shot.bmp ../../../firmware/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin <<'PY' || { echo "FAIL: $2"; exit 1; }
import os, struct, sys
d = open(sys.argv[1], 'rb').read()
assert len(d) == 307266, len(d)
if os.path.exists(sys.argv[2]):
    stock = open(sys.argv[2], 'rb').read()[0xD000:0xD000 + 66]
    assert stock[:2] == b'BM' and d[:66] == stock, 'header differs from the stock S000.BMP'
    print('header == stock S000.BMP header')
off, w, h, bpp, comp = struct.unpack_from('<I', d, 10)[0], *struct.unpack_from('<iiHHI', d, 18)[:2], *struct.unpack_from('<HI', d, 28)
assert (off, w, h, bpp, comp) == (66, 480, 320, 16, 3), (off, w, h, bpp, comp)
for r in range(320):
    y = 319 - r                          # bottom-up
    row = struct.unpack_from('<480H', d, 66 + 960 * r)
    for x in range(480):
        want = [((x // 13) * 0x1234 + y), ((x // 3) * 0x0F0F + y), ((x // 2) * 0x3333 + y),
                ((x * 0x9E37 + y * 0x79B9) ^ (y << 11))][y % 4] & 0xFFFF
        assert row[x] == want, (x, y, row[x], want)
print('pixels == stub screen, bottom-up')
PY
}
IMG7=$W/disk7.img
mkimg $IMG7
mcopy -i $IMG7 $W/keep.bin ::CONFIG.BIN
mcopy -i $IMG7 $W/keep.bin ::DEL.TXT
mdel -i $IMG7 ::DEL.TXT
mcopy -i $IMG7 $W/old.bin ::S000.BMP
"$W/t" $IMG7 shot | tee $W/s1
grep -q "shot=Saved(1) medium_changed=true" $W/s1 && echo "ok: S000.BMP taken -> S001.BMP" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG7
bmpok $IMG7 S001.BMP
"$W/t" $IMG7 shot | grep -q "shot=Saved(2)" && echo "ok: then S002.BMP" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG7
bmpok $IMG7 S002.BMP
mdel -i $IMG7 ::S000.BMP
"$W/t" $IMG7 shot | grep -q "shot=Saved(0)" && echo "ok: first free number reused: S000.BMP" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG7
bmpok $IMG7 S000.BMP
mcopy -n -i $IMG7 ::CONFIG.BIN $W/o && cmp $W/o $W/keep.bin && echo "ok: CONFIG.BIN untouched" || { echo "FAIL at line $LINENO"; exit 1; }
mdir -i $IMG7 ::

echo "--- screenshot: power cut / abort between every two slices (NOR model of write_step)"
# Review 258c833 H1 / L4: the shot driven slice by slice (screenshot::poll_on)
# on a NOR model (write_step: erase in the first call, 4 pages programmed per
# call), cut after every slice k: the image as it is then (incl. a half
# programmed data block), and the same k stopped by screenshot::abort_on
# (power_off / DBG:RESET path). Required for every k: no FAT / root page left
# all 0xFF (erased / half programmed metadata), and fsck finds at most the
# shot's 76 lost clusters and FAT 1 ahead of FAT 2. Then the shot completes
# from a clean start (fsck clean). Finally a host-mounted disk refuses the shot.
cp $IMG7 $W/cutbase.img
NS=$("$W/t" $W/cutbase.img shotcut | tee $W/sc0 | grep -oE "slices=[0-9]+" | cut -d= -f2)
cp $IMG7 $W/cutbase.img
cat $W/sc0
grep -q "result=Some(Saved(" $W/sc0 && grep -q "erased_meta_pages=\[\]" $W/sc0 || { echo "FAIL at line $LINENO"; exit 1; }
[ "$(grep -o 'metadata writes.*' $W/sc0 | grep -o '"' | wc -l)" = 6 ] && echo "ok: uncut shot: $NS slices, 3 metadata block writes (FAT 1, FAT 2, root), each in one slice" || { echo "FAIL at line $LINENO"; exit 1; }
# shotcutok <img> <k>: the only fsck findings a cut / abort may leave.
shotcutok() {
    local rc=0
    fsck.fat -n $1 > $W/fsck.out 2>&1 || rc=$?
    [ $rc -le 1 ] || { cat $W/fsck.out; echo "FAIL: fsck rc=$rc ($2)"; exit 1; }
    if grep -vE "^fsck.fat|files, .*clusters|^$|Leaving filesystem unchanged|FATs differ but appear to be intact|Using first FAT|Reclaimed 76 unused clusters \(311296 bytes\)" $W/fsck.out | grep -q .; then
        cat $W/fsck.out; echo "FAIL: unexpected fsck finding ($2)"; exit 1
    fi
}
nlost=0
for k in $(seq 0 $NS); do
    for mode in cut abort; do
        cp $W/cutbase.img $W/cut.img
        "$W/t" $W/cut.img shotcut $k $mode > $W/sc
        grep -q "erased_meta_pages=\[\]" $W/sc || { cat $W/sc; echo "FAIL: $mode after slice $k: erased FAT / root page"; exit 1; }
        [ $mode = abort ] && { grep -qE "busy=false" $W/sc || { cat $W/sc; echo "FAIL: abort $k left the shot busy"; exit 1; }; }
        shotcutok $W/cut.img "$mode $k"
        grep -q "Reclaimed 76" $W/fsck.out && nlost=$((nlost + 1))
    done
done
echo "ok: $((NS + 1)) cut points x (cut, abort): no FAT / root page ever erased, fsck at worst 76 lost clusters / FAT 1 ahead ($nlost cases)"
cp $W/cutbase.img $W/cut.img
"$W/t" $W/cut.img shotcut $((NS - 1)) abort > /dev/null
"$W/t" $W/cut.img shot | grep -q "shot=Saved(" && echo "ok: the next shot after an abort succeeds" || { echo "FAIL at line $LINENO"; exit 1; }
cp $W/cutbase.img $W/cut.img
"$W/t" $W/cut.img shotmounted | tee $W/sm
grep -q "start=false busy=false changed blocks: \[\]" $W/sm && echo "ok: host has the disk mounted: shot refused, nothing written" || { echo "FAIL at line $LINENO"; exit 1; }

echo "=== csvlog"
# csvok <img> <name> <rows> <full|prefix|gap>: header, columns, every row
# recomputed (row(i) in main.rs) through Python's csv module.
csvok() {
    mcopy -n -i $1 ::$2 $W/log.csv || { echo "FAIL: no $2"; exit 1; }
    python3 -I - $W/log.csv $3 $4 <<'PY' || { echo "FAIL: $2 content"; exit 1; }
import csv, re, sys
path, n, mode = sys.argv[1], int(sys.argv[2]), sys.argv[3]
raw = open(path, 'rb').read()
assert raw.isascii() and raw.endswith(b'\r\n'), 'ASCII, CRLF, ends with a whole row'
lines = raw.decode().split('\r\n')
assert lines[0].startswith('# Alientek DM40A, fw-rs ') and ', function DC V, range AUTO 6 V, started uptime 5.000 s' in lines[0], lines[0]
assert lines[1].startswith('# time_s = seconds since the start'), lines[1]
rd = csv.reader(open(path, newline=''))
rows = [r for r in rd if r and not r[0].startswith('#')]
assert rows[0] == ['time_s', 'value', 'unit', 'flags'], rows[0]
P = [1e-9, 1e-8, 1e-7, 1e-6, 1e-5, 1e-4, 1e-3, 1e-2, 1e-1, 1e0, 1e1, 1e2, 1e3]
num = re.compile(r'^-?(\d+(\.\d+)?|\d\.\d{5}E[+-]\d\d)$')
prev, skipped = -1, 0
for r in rows[1:]:
    assert len(r) == 4, r
    t, v, u, fl = r
    assert re.fullmatch(r'\d+\.\d{3}', t), t
    i = round(float(t) * 10) - 1
    gap = i != prev + 1
    if mode != 'gap':
        assert not gap, (prev, r)
    assert i > prev, (prev, r)
    skipped += i - prev - 1
    prev = i
    m = (i * 7919) % 20001 - 10000
    want = m * P[i % 13]
    f = fl.split()
    exp = [x for x, c in [('OL', i % 97 == 3), ('REL', i % 5 == 0), ('HOLD', i % 7 == 0), ('HI', i % 11 == 0), ('LO', i % 13 == 1)] if c]
    if gap:
        exp.append('GAP')
    assert f == exp, (r, exp)
    assert u == 'Ohm', u
    if 'OL' in f:
        assert v == '', r
    else:
        assert num.match(v), v
        assert abs(float(v) - want) <= 2e-5 * abs(want) + 1e-30, (r, want)
got = prev + 1
if mode == 'full':
    assert got == n and len(rows) - 1 == n, (got, n)
else:
    assert got <= n
print('csv ok: %d rows (last index %d, %d skipped)' % (len(rows) - 1, prev, skipped))
PY
}
# cutok <img>: fsck may only report what a cut between the FAT and the
# directory leaves (FAT 1 ahead, a lost cluster, a LOG chain longer than its size).
cutok() {
    local rc=0
    fsck.fat -n $1 > $W/fsck.out 2>&1 || rc=$?
    [ $rc -le 1 ] || { cat $W/fsck.out; echo "FAIL: fsck rc=$rc"; exit 1; }   # 1 = findings, checked below
    if grep -vE "^fsck.fat|files, .*clusters|^$|Leaving filesystem unchanged|FATs differ but appear to be intact|Using first FAT|Reclaimed [0-9]+ unused clusters?|^/LOG[0-9]{3}\.CSV$|File size is [0-9]+ bytes, cluster chain length is > [0-9]+ bytes|Truncating file to [0-9]+ bytes" $W/fsck.out | grep -q .; then
        cat $W/fsck.out; echo "FAIL: unexpected fsck finding ${CUTK:-}"; exit 1
    fi
}
IMGC=$W/diskc.img
mkimg $IMGC
mcopy -i $IMGC $W/keep.bin ::CONFIG.BIN
cp $IMGC $W/csvbase.img
"$W/t" $IMGC csv rec 3000 | tee $W/c1
grep -q "csv=Done file=Some(0) rows=3000 bytes=76028 committed=76028 dropped=0 medium_changed=true result=true" $W/c1 && echo "ok: 3000 rows recorded and committed as LOG000.CSV" || { echo "FAIL at line $LINENO"; exit 1; }
grep -q "LOG:CSV? -> 3,0,3000,76028,76028,0" $W/c1 && echo "ok: LOG:CSV? reply" || { echo "FAIL at line $LINENO"; exit 1; }
grep -q "session_flag=false" $W/c1 && grep -q "data_max_per_block=1 " $W/c1 && echo "ok: host writes allowed again; no data block erased twice (tail rewrites program-only)" || { echo "FAIL at line $LINENO"; exit 1; }
grep -q "erases: fat=38 root=20 data=19" $W/c1 && echo "ok: 20 commits = 20 root erases, 19 clusters = 2 x 19 FAT erases" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGC
csvok $IMGC LOG000.CSV 3000 full
[ "$(mdir -i $IMGC :: | grep LOG000 | awk '{print $3}')" = 76028 ] && echo "ok: directory size 76028" || { echo "FAIL at line $LINENO"; exit 1; }
mcopy -n -i $IMGC ::CONFIG.BIN $W/o && cmp $W/o $W/keep.bin && echo "ok: CONFIG.BIN untouched" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMGC csv rec 50 | grep -q "csv=Done file=Some(1) rows=50" && echo "ok: next recording is LOG001.CSV" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGC
csvok $IMGC LOG001.CSV 50 full
csvok $IMGC LOG000.CSV 3000 full

echo "--- interleaved with screenshots"
IMGS=$W/disks.img
cp $W/csvbase.img $IMGS
"$W/t" $IMGS csv rec 3000 shot 10 shotw 1000 shot 2400 | tee $W/c2
grep -q "csv=Done file=Some(0) rows=3000 bytes=76028 committed=76028 dropped=0" $W/c2 && grep -q "shots=\[Saved(0), Saved(1), Saved(2)\]" $W/c2 && grep -q "lock_ok=true" $W/c2 && echo "ok: three screenshots during the recording, no commit started during a shot" || { echo "FAIL at line $LINENO"; exit 1; }
grep -q "during a commit: true" $W/c2 && grep -qE "shot_waits=[1-9]" $W/c2 && echo "ok: a shot started during a commit waited for it before its FAT scan" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGS
csvok $IMGS LOG000.CSV 3000 full
bmpok $IMGS S000.BMP
bmpok $IMGS S001.BMP
bmpok $IMGS S002.BMP

echo "--- host write in progress"
IMGH=$W/diskh.img
cp $W/csvbase.img $IMGH
"$W/t" $IMGH csv rec 2000 host 300 120 | tee $W/c3
grep -q "csv=Done file=Some(0) rows=2000 .* dropped=0" $W/c3 && grep -q "host_writes=0 " $W/c3 && echo "ok: 12 s host write: no block written meanwhile, nothing dropped" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGH
csvok $IMGH LOG000.CSV 2000 full
cp $W/csvbase.img $IMGH
"$W/t" $IMGH csv rec 2000 host 300 600 | tee $W/c4
grep -q "csv=Done file=Some(0) .* dropped=[1-9]" $W/c4 && grep -q "host_writes=0 " $W/c4 && echo "ok: 60 s host write: FIFO overflow, rows dropped and counted, nothing written meanwhile" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGH
csvok $IMGH LOG000.CSV 2000 gap | tee $W/c4p
grep -qE "[1-9][0-9]* skipped" $W/c4p && echo "ok: the row after the gap carries GAP" || { echo "FAIL at line $LINENO"; exit 1; }

echo "--- PC mounts the disk mid-recording (commits deferred)"
cp $W/csvbase.img $IMGH
"$W/t" $IMGH csv rec 2000 mountwin 300 150 | tee $W/c7
grep -qE "mount_writes=[1-9]" $W/c7 && echo "ok: baseline: the window holds commits when not mounted" || { echo "FAIL at line $LINENO"; exit 1; }
cp $W/csvbase.img $IMGH
"$W/t" $IMGH csv rec 2000 mount 300 150 | tee $W/c8
grep -q "csv=Done file=Some(0) rows=2000 .* dropped=0" $W/c8 && grep -q "mount_writes=0 " $W/c8 && echo "ok: mounted for 15 s: no block written meanwhile, nothing dropped" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGH
csvok $IMGH LOG000.CSV 2000 full
cp $W/csvbase.img $IMGH
"$W/t" $IMGH csv rec 2000 mount 300 600 | tee $W/c9
grep -q "csv=Done file=Some(0) .* dropped=[1-9]" $W/c9 && grep -q "mount_writes=0 " $W/c9 && echo "ok: mounted for 60 s: FIFO overflow, rows dropped and counted, nothing written meanwhile" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGH
csvok $IMGH LOG000.CSV 2000 gap | tee $W/c9p
grep -qE "[1-9][0-9]* skipped" $W/c9p && echo "ok: the row after the gap carries GAP" || { echo "FAIL at line $LINENO"; exit 1; }

echo "--- chain across the FAT 1 block boundary (clusters 2045..2048: link in block 0 written last)"
IMGB=$W/diskb.img
cp $W/csvbase.img $IMGB
head -c $(( 2040 * 4096 )) /dev/zero > $W/fill
mcopy -i $IMGB $W/fill ::FILL.DAT
cp $IMGB $W/fatbase.img
"$W/t" $IMGB csv rec 500 | tee $W/c6
grep -q "csv=Done file=Some(0) rows=500 " $W/c6 && echo "ok: recorded across the FAT block boundary" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGB
csvok $IMGB LOG000.CSV 500 full
python3 -I - $IMGB <<'PY' || { echo "FAIL at line $LINENO: chain"; exit 1; }
import struct, sys
d = open(sys.argv[1], 'rb').read()
rsvd, nf, fsz, rootn = struct.unpack_from('<H', d, 0x0E)[0], d[0x10], struct.unpack_from('<H', d, 0x16)[0], struct.unpack_from('<H', d, 0x11)[0]
root = (rsvd + nf * fsz) * 4096
for i in range(rootn):
    e = d[root + 32 * i: root + 32 * i + 32]
    if e[:11] == b'LOG000  CSV':
        c, chain = struct.unpack_from('<H', e, 0x1A)[0], []
        while c < 0xFFF8:
            chain.append(c); c = struct.unpack_from('<H', d, rsvd * 4096 + 2 * c)[0]
        print('chain', chain)
        assert chain[0] < 2048 <= chain[-1], chain
        break
else:
    sys.exit(1)
PY

# cutsweep <base> <rows>: power cut before every block write (and a
# program-only write cut half way) of a recording on a copy of <base>.
cutsweep() {
    cp $1 $W/cut0.img
    NW=$("$W/t" $W/cut0.img csv rec $2 | grep -oE "^  writes=[0-9]+" | cut -d= -f2)
    echo "uncut: $NW block writes"
    prevc=0
    for k in $(seq 0 $NW); do
        CUTK="(cut $k)"
        cp $1 $W/cut.img
        "$W/t" $W/cut.img csv rec $2 cut $k > $W/cr
        cutok $W/cut.img
        if mdir -i $W/cut.img :: | grep -q LOG000; then
            csvok $W/cut.img LOG000.CSV $2 prefix > $W/cp || { cat $W/cp; echo "FAIL: cut $k"; exit 1; }
            c=$(mdir -i $W/cut.img :: | grep LOG000 | awk '{print $3}')
        else
            c=0
        fi
        [ $c -ge $prevc ] || { echo "FAIL: cut $k: committed size went down"; exit 1; }
        prevc=$c
        mcopy -n -i $W/cut.img ::CONFIG.BIN $W/o && cmp -s $W/o $W/keep.bin || { echo "FAIL: cut $k: CONFIG.BIN"; exit 1; }
        cp $1 $W/cut.img
        "$W/t" $W/cut.img csv rec $2 cut $k half > $W/cr
        cutok $W/cut.img
        if mdir -i $W/cut.img :: | grep -q LOG000; then
            csvok $W/cut.img LOG000.CSV $2 prefix > $W/cp || { cat $W/cp; echo "FAIL: cut $k half"; exit 1; }
        fi
    done
    CUTK=
    echo "ok: $((NW + 1)) cut points x 2: fsck only lost / excess clusters or FAT 1 ahead, committed CSV always a clean prefix, never shrinking"
}
echo "--- power cut before every block write"
cutsweep $W/csvbase.img 1500
echo "--- power cut before every block write, chain across the FAT block boundary"
cutsweep $W/fatbase.img 500

echo "--- nearly full disk"
IMGF=$W/diskf.img
cp $W/csvbase.img $IMGF
FREE=$(fsck.fat -n $IMGF | grep -o "[0-9]*/[0-9]* clusters" | awk -F'[/ ]' '{print $2 - $1}')
head -c $(( (FREE - 5) * 4096 )) /dev/zero > $W/fill
mcopy -i $IMGF $W/fill ::FILL.DAT
"$W/t" $IMGF csv rec 3000 | tee $W/c5
grep -q "csv=Full file=Some(0)" $W/c5 && echo "ok: disk full ends the recording (Full)" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGF
csvok $IMGF LOG000.CSV 3000 prefix
SZ=$(mdir -i $IMGF :: | grep LOG000 | awk '{print $3}')
[ $SZ -gt $((20480 - 96)) ] && [ $SZ -le 20480 ] && echo "ok: the 5 free clusters filled, size cut back to the last whole row ($SZ bytes)" || { echo "FAIL at line $LINENO"; exit 1; }

echo "--- gallery"
mcopy -i $IMGS $W/keep.bin "::Log_with_a_long_name.csv"
mdir -i $IMGS ::
"$W/t" $IMGS csv list | tee $W/g1
grep -q "list n=5 total=5" $W/g1 && grep -q "  LOG000.CSV 76028 Csv" $W/g1 && grep -q "  S001.BMP 307266 Bmp" $W/g1 && grep -q "  LOG_WI~1.CSV 9000 Csv" $W/g1 && grep -q "changed blocks: \[\]" $W/g1 && echo "ok: list: 3 BMP + 2 CSV with sizes, CONFIG.BIN not listed, nothing written" || { echo "FAIL at line $LINENO"; exit 1; }
F0=$(grep -o "free=[0-9]*" $W/g1 | cut -d= -f2)
"$W/t" $IMGS csv cat | tee $W/g0
grep -qx "cat=$((16740352 - F0)),$F0,\"LOG000.CSV,ASC,76028\",\"S000.BMP,BIN,307266\",\"S001.BMP,BIN,307266\",\"S002.BMP,BIN,307266\",\"LOG_WI~1.CSV,ASC,9000\"" $W/g0 && echo "ok: MMEM:CAT? text" || { echo "FAIL at line $LINENO"; exit 1; }
[ $F0 = $(( $(fsck.fat -n $IMGS | grep -o "[0-9]*/[0-9]* clusters" | awk -F'[/ ]' '{print $2 - $1}') * 4096 )) ] && echo "ok: free bytes = fsck free clusters" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMGS csv del LOG000.CSV | tee $W/g2
grep -q "del=Ok(())" $W/g2 && grep -q "free=$((F0 + 19 * 4096)) " $W/g2 && echo "ok: LOG000.CSV deleted, its 19 clusters freed" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGS
"$W/t" $IMGS csv del log_wi~1.csv mounted | tee $W/g2m
grep -q "del=Err(Busy)" $W/g2m && grep -q "changed blocks: \[\]" $W/g2m && echo "ok: delete refused while a PC has the disk mounted, nothing written" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMGS csv del log_wi~1.csv | grep -q "del=Ok(())" && echo "ok: long-named file deleted" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGS
mdir -i $IMGS :: | grep -qi "long_name" && { echo "FAIL at line $LINENO: long name left"; exit 1; }
"$W/t" $IMGS csv del S001.BMP | grep -q "del=Ok(())" && echo "ok: S001.BMP deleted" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMGS csv del S001.BMP | grep -q "del=Err(NotFound)" && echo "ok: second delete = NotFound" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMGS csv del CONFIG.BIN | grep -q "del=Err(NotFound)" && echo "ok: CONFIG.BIN is not a gallery file" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGS
bmpok $IMGS S000.BMP
bmpok $IMGS S002.BMP
mcopy -n -i $IMGS ::CONFIG.BIN $W/o && cmp $W/o $W/keep.bin && echo "ok: CONFIG.BIN untouched" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMGS csv rec 1200 del 300 S002.BMP | tee $W/g4
grep -q "gallery delete at row 3[0-9][0-9]: Ok(())" $W/g4 && grep -q "csv=Done file=Some(0) rows=1200 " $W/g4 && echo "ok: S002.BMP deleted from the gallery between two commits of a recording" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGS
csvok $IMGS LOG000.CSV 1200 full
mdir -i $IMGS :: | grep -q "^S002" && { echo "FAIL at line $LINENO"; exit 1; }
bmpok $IMGS S000.BMP
mdel -i $IMGS ::LOG000.CSV
"$W/t" $IMGS csv busydel | tee $W/g3
grep -q "busydel LOG000.CSV -> Err(Busy)" $W/g3 && grep -q "then del -> Ok(())" $W/g3 && echo "ok: the file being recorded cannot be deleted; after stop it can" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMGS
mdir -i $IMGS ::
echo PASS
