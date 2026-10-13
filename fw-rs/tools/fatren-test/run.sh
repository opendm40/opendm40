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
#   6. first run writes all eight files (incl. ROMBOOT.BIN 16 KiB and the
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
assert len(rows) == 7, rows
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
grep -q "backup=Written written=0xff conflict=0x00 freed=0" $W/b1 && echo "ok: first run writes all eight files" || { echo "FAIL at line $LINENO"; exit 1; }
mdir -i $IMG5 ::
fsckok $IMG5
same $IMG5
mtype -i $IMG5 ::BACKUP.TXT
"$W/t" $IMG5 backup $W/src | tee $W/b2
grep -q "backup=Unchanged" $W/b2 && grep -q "changed blocks: \[\]" $W/b2 && echo "ok: second run writes nothing" || { echo "FAIL at line $LINENO"; exit 1; }
printf '\x42' | dd of=$W/src/CALIB.BIN bs=1 seek=400 conv=notrunc status=none
"$W/t" $IMG5 backup $W/src | tee $W/b3
grep -q "backup=Written written=0x81 conflict=0x00 freed=2" $W/b3 && echo "ok: changed calibration: CALIB.BIN + BACKUP.TXT rewritten, old clusters freed" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG5
same $IMG5
"$W/t" $IMG5 backup $W/src | grep -q "backup=Unchanged" && echo "ok: unchanged after the rewrite" || { echo "FAIL at line $LINENO"; exit 1; }
[ $(mdir -i $IMG5 :: | grep -c "CALIB") = 1 ] && echo "ok: one CALIB.BIN entry" || { echo "FAIL at line $LINENO"; exit 1; }
head -c 100000 /dev/urandom > $W/junk
mcopy -o -i $IMG5 $W/junk ::UID.BIN
"$W/t" $IMG5 backup $W/src | tee $W/b4
grep -q "backup=Written written=0x04 conflict=0x00 freed=25" $W/b4 && echo "ok: foreign UID.BIN (100000 B) replaced, its 25 clusters freed" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG5
same $IMG5
IMG6=$W/disk6.img
mkimg $IMG6
mmd -i $IMG6 ::OTP.BIN
mcopy -i $IMG6 $W/a.bin ::dm40_v_2_6.bin
mcopy -i $IMG6 $W/old.bin ::_DM40PRV.BIN
"$W/t" $IMG6 live $MA | grep -q "status=Renamed" && echo "ok: fatren on the backup volume" || { echo "FAIL at line $LINENO"; exit 1; }
"$W/t" $IMG6 backup $W/src | tee $W/b5
grep -q "backup=Written written=0xef conflict=0x10" $W/b5 && echo "ok: directory OTP.BIN = conflict, the rest written" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG6
"$W/t" $IMG6 backup $W/src | grep -q "backup=Unchanged" && echo "ok: then unchanged" || { echo "FAIL at line $LINENO"; exit 1; }
mdir -i $IMG6 ::
echo "--- changed loader sector"
printf '\x5a' | dd of=$W/src/LOADER.BIN bs=1 seek=$((0x1F000)) conv=notrunc status=none
"$W/t" $IMG5 backup $W/src | tee $W/b6
grep -q "backup=Written written=0xc0 conflict=0x00 freed=33" $W/b6 && echo "ok: changed LOADER.BIN: LOADER.BIN + BACKUP.TXT rewritten, 33 old clusters freed" || { echo "FAIL at line $LINENO"; exit 1; }
fsckok $IMG5
same $IMG5
"$W/t" $IMG5 backup $W/src | grep -q "backup=Unchanged" && echo "ok: unchanged after the loader rewrite" || { echo "FAIL at line $LINENO"; exit 1; }
mtype -i $IMG5 ::BACKUP.TXT
echo PASS
