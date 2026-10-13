#!/bin/bash
# Host test of src/fatren.rs and src/calbackup.rs on a DM40-geometry FAT16
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
# calbackup:
#   6. first run writes all eight files (no flash image given: no _DM40PRV.BIN) (incl. ROMBOOT.BIN 16 KiB and the
#      32-cluster LOADER.BIN, real data from the 1 MiB dump); fsck clean;
#      contents equal the sources; BACKUP.TXT CRCs match;
#   7. second run: UNCHANGED, no block written;
#   8. changed calibration: CALIB.BIN + BACKUP.TXT rewritten (copy-on-write,
#      old clusters freed), fsck clean, contents equal; then unchanged again;
#   9. a corrupted / foreign copy (different size, 25 clusters) is replaced and
#      its chain freed; a directory named OTP.BIN is a conflict (skipped);
#   10. fatren + calbackup on the same volume, fsck clean;
#   11. one changed byte deep in LOADER.BIN: LOADER.BIN + BACKUP.TXT rewritten
#      (multi-block 32-cluster chain, copy-on-write), 33 old clusters freed,
#      fsck clean, contents equal; then unchanged again.
#   CRC-first: an unchanged run reads 4 blocks (boot, root, FAT 1, BACKUP.TXT).
# firmware file _DM40PRV.BIN (calbackup::FwFile from a flash image = decoded
# body with the decoded header at +0x4000):
#   stock dm40_v_2_6.bin (empty disk: all 9 files), dm40-fw-native.bin and a
#   random 480010-byte body (22-byte stored-as-is tail) regenerated
#   byte-identical, dm40fw.py info + body CRC ok; unchanged = 4 reads; firmware
#   change rewrites it + BACKUP.TXT; a host re-copy is only re-stamped (root
#   block); an in-place data change is missed (documented) until BACKUP.TXT is
#   deleted; fatren renaming the installed file over it -> re-stamp only;
#   review M1: a nearly full disk (20 free clusters) with a changed firmware
#   and calibration -> NoSpace retried without the firmware file: CALIB.BIN +
#   BACKUP.TXT written (fw_skipped), old _DM40PRV.BIN untouched, then
#   UNCHANGED while full; after freeing space the firmware file is written.
# screenshot (src/screenshot.rs incremental writer, driven to completion by take_on; synthetic screen):
#   12. S000.BMP taken -> S001.BMP, then S002.BMP; after deleting S000.BMP the
#      next one is S000.BMP again; fsck clean; 307266 bytes, header equal to
#      the stock S000.BMP in the disk dump (when present), every pixel equal
#      to the stub screen with bottom-up rows; the other files untouched.
#      (The stub screen mixes run rows and noise rows, so the RLE capture fills up part way and
#      the rest is read from the "GRAM" while writing: both paths.)
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
# mkfw <body> <out.bin>: pack a body into an update file, print its marker.
mkfw() {
    python3 -I $FW pack $1 $2 >/dev/null
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

echo "=== calbackup"
# fsckok <img>: fsck.fat -n exit 0 and no "lost"/"differ" message.
fsckok() {
    fsck.fat -n $1 > $W/fsck.out 2>&1 || { cat $W/fsck.out; echo "FAIL: fsck"; exit 1; }
    if grep -qiE "free cluster|differ|lost|orphan|reclaim" $W/fsck.out; then cat $W/fsck.out; echo "FAIL: fsck"; exit 1; fi
    tail -1 $W/fsck.out
}
# same <img>: every backup file on the image equals its source; BACKUP.TXT CRCs.
same() {
    for f in CALIB.BIN BOOTDATA.BIN UID.BIN OPTBYTES.BIN OTP.BIN ROMBOOT.BIN LOADER.BIN; do
        mcopy -n -i $1 ::$f $W/o && cmp $W/o $W/src/$f || { echo "FAIL: $f differs"; exit 1; }
    done
    mcopy -n -i $1 ::BACKUP.TXT $W/txt
    python3 -I - $W/src $W/txt <<'PY' || { echo "FAIL: BACKUP.TXT"; exit 1; }
import sys, zlib
src, txt = sys.argv[1], open(sys.argv[2], 'rb').read().decode()
lines = txt.splitlines()[3:]
rows = {l.split()[0]: l.split() for l in lines[:lines.index('')]}
assert len(rows) in (7, 8), rows
for f in ['CALIB.BIN', 'BOOTDATA.BIN', 'UID.BIN', 'OPTBYTES.BIN', 'OTP.BIN', 'ROMBOOT.BIN', 'LOADER.BIN']:
    d = open(src + '/' + f, 'rb').read()
    r = rows[f]
    assert int(r[2]) == len(d) and int(r[3], 16) == zlib.crc32(d), (f, r)
PY
    echo "ok: all backup files equal their sources, BACKUP.TXT CRCs match"
}
mkdir $W/src
cp ../../../firmware/re/dumps/dump_settings.bin $W/s32 && head -c 4096 $W/s32 > $W/src/CALIB.BIN
DUMP=../../../firmware/re/dumps/full-20261013/flash_08000000_1MB.bin
dd if=$DUMP of=$W/src/ROMBOOT.BIN bs=4096 skip=0 count=4 status=none
dd if=$DUMP of=$W/src/LOADER.BIN bs=4096 skip=$((0xA0000 / 4096)) count=32 status=none
dd if=$DUMP of=$W/src/BOOTDATA.BIN bs=4096 skip=$((0xA4000 / 4096)) count=4 status=none
[ $(stat -c %s $W/src/ROMBOOT.BIN) = 16384 ] && [ $(stat -c %s $W/src/LOADER.BIN) = 131072 ] || { echo "FAIL: dump slices"; exit 1; }
head -c 12 /dev/urandom > $W/src/UID.BIN
cp ../../../firmware/re/dumps/full-20261013/option_bytes_1FFFC000.bin $W/src/OPTBYTES.BIN
cp ../../../firmware/re/dumps/full-20261013/otp_1FFF7800.bin $W/src/OTP.BIN
IMG5=$W/disk5.img
mkimg $IMG5
mcopy -i $IMG5 $W/keep.bin ::CONFIG.BIN
mcopy -i $IMG5 $W/keep.bin ::DEL.TXT
mdel -i $IMG5 ::DEL.TXT
"$W/t" $IMG5 backup $W/src | tee $W/b1
grep -q "backup=Written written=0x0ff conflict=0x000 freed=0" $W/b1 && echo "ok: first run writes all eight files" || { echo "FAIL at line $LINENO"; exit 1; }
mdir -i $IMG5 ::
fsckok $IMG5
same $IMG5
mtype -i $IMG5 ::BACKUP.TXT
"$W/t" $IMG5 backup $W/src | tee $W/b2
grep -q "backup=Unchanged" $W/b2 && grep -q "changed blocks: \[\]" $W/b2 && echo "ok: second run writes nothing" || { echo "FAIL at line $LINENO"; exit 1; }
# CRC-first: an unchanged run reads boot sector, root, FAT 1 and BACKUP.TXT only.
grep -qE "reads=4 data_reads=\[[0-9]+\]$" $W/b2 && echo "ok: unchanged run: 4 block reads, the only data block is BACKUP.TXT" || { echo "FAIL at line $LINENO"; exit 1; }
printf '\x42' | dd of=$W/src/CALIB.BIN bs=1 seek=400 conv=notrunc status=none
"$W/t" $IMG5 backup $W/src | tee $W/b3
grep -q "backup=Written written=0x081 conflict=0x000 freed=2" $W/b3 && echo "ok: changed calibration: CALIB.BIN + BACKUP.TXT rewritten, old clusters freed" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG5
same $IMG5
"$W/t" $IMG5 backup $W/src | grep -q "backup=Unchanged" && echo "ok: unchanged after the rewrite" || { echo "FAIL at line $LINENO"; exit 1; }
[ $(mdir -i $IMG5 :: | grep -c "CALIB") = 1 ] && echo "ok: one CALIB.BIN entry" || { echo "FAIL at line $LINENO"; exit 1; }
head -c 100000 /dev/urandom > $W/junk
mcopy -o -i $IMG5 $W/junk ::UID.BIN
"$W/t" $IMG5 backup $W/src | tee $W/b4
grep -q "backup=Written written=0x004 conflict=0x000 freed=25" $W/b4 && echo "ok: foreign UID.BIN (100000 B) replaced, its 25 clusters freed" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG5
same $IMG5
IMG6=$W/disk6.img
mkimg $IMG6
mmd -i $IMG6 ::OTP.BIN
mcopy -i $IMG6 $W/a.bin ::dm40_v_2_6.bin
mcopy -i $IMG6 $W/old.bin ::_DM40PRV.BIN
"$W/t" $IMG6 live $MA | grep -q "status=Renamed" && echo "ok: fatren on the backup volume" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG6 backup $W/src | tee $W/b5
grep -q "backup=Written written=0x0ef conflict=0x010" $W/b5 && echo "ok: directory OTP.BIN = conflict, the rest written" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG6
"$W/t" $IMG6 backup $W/src | grep -q "backup=Unchanged" && echo "ok: then unchanged" || { echo "FAIL at line $LINENO"; exit 1; }
mdir -i $IMG6 ::
echo "--- changed loader sector"
printf '\x5a' | dd of=$W/src/LOADER.BIN bs=1 seek=$((0x1F000)) conv=notrunc status=none
"$W/t" $IMG5 backup $W/src | tee $W/b6
grep -q "backup=Written written=0x0c0 conflict=0x000 freed=33" $W/b6 && echo "ok: changed LOADER.BIN: LOADER.BIN + BACKUP.TXT rewritten, 33 old clusters freed" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG5
same $IMG5
"$W/t" $IMG5 backup $W/src | grep -q "backup=Unchanged" && echo "ok: unchanged after the loader rewrite" || { echo "FAIL at line $LINENO"; exit 1; }
mtype -i $IMG5 ::BACKUP.TXT

echo "=== running firmware's update file (_DM40PRV.BIN) + CRC-first"
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
# fwok <img> <orig.bin>: _DM40PRV.BIN on <img> is byte-identical to <orig.bin>,
# and dm40fw.py reads it as a valid update file (header words, body CRC).
fwok() {
    mcopy -n -i $1 ::_DM40PRV.BIN $W/prv.bin
    cmp $W/prv.bin $2 || { echo "FAIL: _DM40PRV.BIN != $2"; exit 1; }
    python3 -I $FW info $W/prv.bin
    python3 -I - $FW $W/prv.bin <<'PY' || { echo "FAIL: dm40fw.py check"; exit 1; }
import importlib.util, sys
spec = importlib.util.spec_from_file_location('dm40fw', sys.argv[1]); f = importlib.util.module_from_spec(spec); spec.loader.exec_module(f)
w0, length, w2, body = f.split(f.decode(open(sys.argv[2], 'rb').read()))
assert w0 == f.header_word0(length, w2), 'word0'
assert w2 >> 16 == 1 and w2 & 0xffff == f.body_checksum(body), 'word2 / body CRC'
print('header words and body CRC ok')
PY
    echo "ok: _DM40PRV.BIN byte-identical to $(basename $2), valid update file"
}
STOCK=../../../dm40_v_2_6.bin
NATIVE=../../dm40-fw-native.bin
head -c 480010 /dev/urandom > $W/t.body       # (12 + 480010) % 128 = 22: store-as-is tail
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
IMG8=$W/disk8.img
mkimg $IMG8
"$W/t" $IMG8 backup $W/src $W/flash_stock | tee $W/f1
grep -q "backup=Written written=0x1ff conflict=0x000 freed=0" $W/f1 && echo "ok: empty disk: 8 backup files + stock firmware file (121 clusters) written" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG8
same $IMG8
fwok $IMG8 $STOCK
mtype -i $IMG8 ::BACKUP.TXT | grep -q "_DM40PRV.BIN 08004000" && echo "ok: BACKUP.TXT lists _DM40PRV.BIN" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG8 backup $W/src $W/flash_stock | tee $W/f2
grep -q "backup=Unchanged" $W/f2 && grep -q "changed blocks: \[\]" $W/f2 && grep -qE "reads=4 data_reads=\[[0-9]+\]$" $W/f2 && echo "ok: unchanged with the firmware file: 4 reads, no file data read" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG8 backup $W/src $W/flash_native | tee $W/f3
grep -q "backup=Written written=0x180 conflict=0x000 freed=$(( (495128 + 12 + 4095) / 4096 + 1 ))" $W/f3 && echo "ok: firmware changed (native): _DM40PRV.BIN + BACKUP.TXT rewritten, old chains freed" || { echo "FAIL at line $LINENO"; exit 1; }
grep -qE "data_reads=\[[0-9]+, [0-9]+" $W/f3 || { echo "FAIL at line $LINENO: expected the full compare of the changed file"; exit 1; }
fsckok $IMG8
same $IMG8
fwok $IMG8 $NATIVE
"$W/t" $IMG8 backup $W/src $W/flash_native | grep -q "backup=Unchanged" && echo "ok: then unchanged" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG8 backup $W/src $W/flash_t | tee $W/f4
grep -q "backup=Written written=0x180" $W/f4 && echo "ok: 480010-byte body (22-byte stored-as-is tail)" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG8
fwok $IMG8 $W/t.bin
echo "--- host stamp on an equal file -> re-stamped once (root block only)"
mcopy -n -i $IMG8 ::_DM40PRV.BIN $W/o && mcopy -o -i $IMG8 $W/o ::_DM40PRV.BIN
"$W/t" $IMG8 backup $W/src $W/flash_t | tee $W/f5
grep -q "backup=Written written=0x100 conflict=0x000 freed=0" $W/f5 && echo "ok: equal file with a foreign stamp: written=0x100 (stamp only)" || { echo "FAIL at line $LINENO"; exit 1; }
[ "$(grep -o 'changed blocks: .*' $W/f5)" = "changed blocks: [5]" ] && echo "ok: only root block 5 written" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG8 backup $W/src $W/flash_t | tee $W/f6
grep -q "backup=Unchanged" $W/f6 && grep -qE "reads=4 data_reads=\[[0-9]+\]$" $W/f6 && echo "ok: then trusted again without data reads" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG8
fwok $IMG8 $W/t.bin
echo "--- in-place corruption keeping size / chain / stamp: not detected (documented); deleting BACKUP.TXT forces the full compare"
L=$(python3 -I - $IMG8 <<'PY'
import struct, sys
d = open(sys.argv[1], 'rb').read()
rsvd, nf, fsz, rootn = struct.unpack_from('<H', d, 0x0E)[0], d[0x10], struct.unpack_from('<H', d, 0x16)[0], struct.unpack_from('<H', d, 0x11)[0]
root = rsvd + nf * fsz; data = root + rootn * 32 // 4096
for i in range(rootn):
    e = d[root * 4096 + 32 * i: root * 4096 + 32 * i + 32]
    if e[:11] == b'LOADER  BIN':
        print(data + struct.unpack_from('<H', e, 0x1A)[0] - 2 + 3); break
PY
)
printf '\xa5' | dd of=$IMG8 bs=1 seek=$(( L * 4096 + 100 )) conv=notrunc status=none
"$W/t" $IMG8 backup $W/src $W/flash_t | grep -q "backup=Unchanged" && echo "ok: (expected) in-place change of a LOADER.BIN data block not seen by the quick check" || { echo "FAIL at line $LINENO"; exit 1; }
mdel -i $IMG8 ::BACKUP.TXT
"$W/t" $IMG8 backup $W/src $W/flash_t | tee $W/f7
grep -q "backup=Written written=0x0c0" $W/f7 && echo "ok: without BACKUP.TXT every file is compared: LOADER.BIN + BACKUP.TXT rewritten" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG8
same $IMG8
fwok $IMG8 $W/t.bin
echo "--- fatren renames the installed file over _DM40PRV.BIN: no fight"
MT=$(od -An -tx1 -N12 <(python3 -I $FW decode $W/t.bin /dev/stdout) | tr -d ' \n')
mcopy -i $IMG8 $W/t.bin ::dm40_fwrs.bin
"$W/t" $IMG8 live $MT | tee $W/f8
grep -q "status=Renamed" $W/f8 && grep -q "old_deleted=true" $W/f8 && echo "ok: fatren replaced _DM40PRV.BIN by the installed (identical) file" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG8 backup $W/src $W/flash_t | tee $W/f9
grep -q "backup=Written written=0x100 conflict=0x000 freed=0" $W/f9 && echo "ok: calbackup only re-stamps it" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG8 backup $W/src $W/flash_t | grep -q "backup=Unchanged" && echo "ok: then unchanged" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG8 live $MT | grep -q "status=Clean" && echo "ok: fatren then clean" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG8
fwok $IMG8 $W/t.bin

echo "--- M1: nearly full disk, no room for _DM40PRV.BIN -> backup retried without it"
IMG9=$W/disk9.img
mkimg $IMG9
"$W/t" $IMG9 backup $W/src $W/flash_stock | grep -q "backup=Written written=0x1ff conflict=0x000 freed=0 fw_skipped=false" && echo "ok: all 9 files written" || { echo "FAIL at line $LINENO"; exit 1; }
# Fill the volume up to 20 free clusters (80 KiB < the ~260 KiB new firmware file).
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
head -c $(( (FREE - 20) * 4096 )) /dev/zero > $W/fill
mcopy -i $IMG9 $W/fill ::FILL.DAT
echo "free clusters before the fill: $FREE, after: 20"
printf '\x17' | dd of=$W/src/CALIB.BIN bs=1 seek=500 conv=notrunc status=none
"$W/t" $IMG9 backup $W/src $W/flash_native | tee $W/m1
grep -q "backup=Written written=0x081 conflict=0x000 freed=2 fw_skipped=true" $W/m1 && echo "ok: firmware + calibration changed, disk nearly full: CALIB.BIN + BACKUP.TXT written, _DM40PRV.BIN skipped" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG9
same $IMG9
fwok $IMG9 $STOCK
mtype -i $IMG9 ::BACKUP.TXT | grep -q "_DM40PRV" && { echo "FAIL at line $LINENO: BACKUP.TXT still lists _DM40PRV.BIN"; exit 1; }
echo "ok: old _DM40PRV.BIN left as it was, BACKUP.TXT without the firmware lines"
"$W/t" $IMG9 backup $W/src $W/flash_native | tee $W/m2
grep -q "backup=Unchanged written=0x000 conflict=0x000 freed=0 fw_skipped=true" $W/m2 && grep -q "changed blocks: \[\]" $W/m2 && echo "ok: still full: UNCHANGED (fw skipped), nothing written" || { echo "FAIL at line $LINENO"; exit 1; }
mdel -i $IMG9 ::FILL.DAT
"$W/t" $IMG9 backup $W/src $W/flash_native | tee $W/m3
grep -q "backup=Written written=0x180 conflict=0x000 freed=$(( (495128 + 12 + 4095) / 4096 + 1 )) fw_skipped=false" $W/m3 && echo "ok: space freed: _DM40PRV.BIN + BACKUP.TXT written, old chains freed" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG9
same $IMG9
fwok $IMG9 $NATIVE
"$W/t" $IMG9 backup $W/src $W/flash_native | grep -q "backup=Unchanged written=0x000 conflict=0x000 freed=0 fw_skipped=false" && echo "ok: then unchanged" || { echo "FAIL at line $LINENO"; exit 1; }

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
echo PASS
