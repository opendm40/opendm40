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
# Untracked inputs (L5): stand-ins when absent. The stock file's body is
# 495128 bytes (the freed= counts below depend on it); the native one only
# has to be larger than the 80 KiB left free in the M1 case.
# mkbody <len> <out>: random body with the 12 marker-gap bytes at 0x4000 0xFF.
mkbody() {
    head -c $1 /dev/urandom > $2
    printf '\377\377\377\377\377\377\377\377\377\377\377\377' | dd of=$2 bs=1 seek=$((0x4000)) conv=notrunc status=none
}
if [ ! -f $STOCK ]; then
    echo "note: no $STOCK, synthetic stand-in (body 495128 bytes)"
    mkbody 495128 $W/stock.body && python3 -I $FW pack $W/stock.body $W/stock.bin >/dev/null && STOCK=$W/stock.bin
fi
if [ ! -f $NATIVE ]; then
    echo "note: no $NATIVE (./build.sh native), synthetic stand-in"
    mkbody 289000 $W/native.body && python3 -I $FW pack $W/native.body $W/native.bin >/dev/null && NATIVE=$W/native.bin
fi
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
