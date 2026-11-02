#!/bin/bash
# Owner requirements 1-5, emulator only. Results in t2/.
cd "$(dirname "$0")"; R=$PWD; O=$R/t2; FW=${FW:?path of an opendm40 update file}
export TMPDIR=$R/tmp DM40_EMU_PORTS=46100-46199 DM40_EMU_LOG_MAX_MB=200 MTOOLS_SKIP_CHECK=1 LC_ALL=C.UTF-8
RUN="timeout 280 python3 ../emu/run.py chain --uid unit --trace-boot"
B=../emu/out/disk.bin
python3 mkimg.py $O/a.bin; python3 mkimg.py $O/b.bin --zero-marker
python3 - <<P
import sys; sys.path.insert(0,'../stock')
import dm40fw as f
d=f.decode(open('$FW','rb').read())
b=bytearray(d); b[12+1000]^=1; open('$O/crcbad.bin','wb').write(f.encode(bytes(b)))
e=open('$FW','rb').read(); open('$O/trunc.bin','wb').write(e[:len(e)//2])
P
# S1 touch + keys + scroll, 17 files, valid app with PA2 low
python3 mklfndisk.py $B $O/disk-s1.bin >/dev/null
$RUN --flash $O/a.bin --pa2 -1 --run-for 3.2 --usbip-port 46060 --name s1 --disk $O/disk-s1.bin --shot $O/s1-a-list.png@1.25 \
  --tap 300,84@1.3 --shot $O/s1-b-tap-row2.png@1.5 --tap 300,228@1.55 --shot $O/s1-c-tap-last-scrolls.png@1.75 --key mf@1.8 --shot $O/s1-d-mf-prev.png@2.05 --key auto@2.1 --key auto@2.35 --shot $O/s1-e-auto-auto.png@2.6 --tap 300,290@2.65 --shot $O/s1-f-button-bad.png@2.85 --key menu@2.9 --shot $O/s1-g-menu.png@3.15 > $O/s1.out 2>&1 &
# S2 touch install of the second of two files
cp $B $O/disk-s2.bin; mcopy -o -i $O/disk-s2.bin $O/crcbad.bin ::dm40_first_bad_crc.bin; mcopy -o -i $O/disk-s2.bin $FW ::dm40_opendm40_v0.4.0.bin
$RUN --flash $O/b.bin --pa2 0 --run-for 6 --usbip-port 46061 --name s2 --disk $O/disk-s2.bin --shot $O/s2-a-list.png@0.6 --tap 300,60@0.7 --shot $O/s2-b-sel.png@0.9 --tap 300,290@1.0 --shot $O/s2-c-busy.png@1.1 --shot $O/s2-d-app.png --save-flash $O/s2-flash.bin > $O/s2.out 2>&1 &
# S3 one file, zeroed marker: silent install
cp $B $O/disk-s3.bin; mcopy -o -i $O/disk-s3.bin $FW ::dm40_opendm40_v0.4.0.bin; cp $O/disk-s3.bin $O/disk-s4.bin
$RUN --flash $O/b.bin --pa2 0 --run-for 5 --usbip-port 46062 --name s3 --disk $O/disk-s3.bin --shot $O/s3-a-busy.png@0.5 --shot $O/s3-b-app.png --save-flash $O/s3-flash.bin > $O/s3.out 2>&1 &
# S5 one bad file: error, no retry, flash untouched
cp $B $O/disk-s5.bin; mcopy -o -i $O/disk-s5.bin $O/crcbad.bin ::dm40_bad.bin
$RUN --flash $O/a.bin --pa2 -1 --run-for 3 --usbip-port 46063 --name s5 --disk $O/disk-s5.bin --shot $O/s5-crc.png --save-flash $O/s5-flash.bin > $O/s5.out 2>&1 &
cp $B $O/disk-s6.bin; mcopy -o -i $O/disk-s6.bin $O/trunc.bin ::dm40_half.bin
$RUN --flash $O/a.bin --pa2 -1 --run-for 3 --usbip-port 46064 --name s6 --disk $O/disk-s6.bin --shot $O/s6-trunc.png --save-flash $O/s6-flash.bin > $O/s6.out 2>&1 &
wait
# S4 already installed: the flash of S3, one file, PA2 low: list, marked, no flashing; button says so
$RUN --flash $O/s3-flash.bin --pa2 -1 --run-for 3 --usbip-port 46065 --name s4 --disk $O/disk-s4.bin --shot $O/s4-a-marked.png@1.25 --tap 300,290@1.3 --shot $O/s4-b-button.png@1.5 --key run@1.6 --shot $O/s4-c-key.png --save-flash $O/s4-flash.bin > $O/s4.out 2>&1 &
# S7 blank disk: format
python3 -c "open('$O/disk-s7.bin','wb').write(b'\xff'*(16<<20))"
$RUN --flash $O/a.bin --pa2 -1 --run-for 2 --usbip-port 46066 --name s7 --disk $O/disk-s7.bin --shot $O/s7-formatted.png --save-disk $O/s7-saved.bin > $O/s7.out 2>&1 &
# S8 first sector not blank, no FAT: unchanged, message, USB served
python3 -c "
d=bytearray(b'\xff'*(16<<20)); d[100:104]=b'CAL!'; d[0x20000:0x20010]=bytes(range(16)); open('$O/disk-s8.bin','wb').write(d)"
cp $O/disk-s8.bin $O/disk-s8.orig
echo "60 monitor sysbus.spi2.spiflash Save @$O/s8-saved.bin" > $O/s8.script
timeout 280 python3 ../emu/run.py chain --uid unit --flash $O/a.bin --pa2 -1 --fast --usbip 0.01 --usbip-port 46050 --run-for 25 --name s8 --disk $O/disk-s8.bin --shot $O/s8-nofs.png@5 --save-disk $O/s8-saved.bin > $O/s8.out 2>&1 &
sleep 8; CPORT=46050 timeout 40 python3 ctest2.py read $O/disk-s8.orig > $O/s8-usb.txt 2>&1
wait
python3 - <<P > $O/flash-checks.txt
import sys; sys.path.insert(0,'../stock')
import dm40fw as f
dec=f.decode(open('$FW','rb').read()); w0,l,w2,body=f.split(dec)
o=open('$O/a.bin','rb').read()
def chk(n):
    d=open('$O/'+n,'rb').read()
    a=d[0x4000:0x4000+l]
    print(n,'marker==hdr',d[0x8000:0x800c]==dec[:12],'body diffs',len([1 for x,y in zip(a,body) if x!=y]),'rest FF',set(d[0x4000+l:0xA0000])=={255},'s9-11 same',d[0xA0000:]==o[0xA0000:],'s0 same',d[:16384]==o[:16384])
chk('s2-flash.bin'); chk('s3-flash.bin')
for n in ('s5-flash.bin','s6-flash.bin'):
    print(n,'identical to the start image',open('$O/'+n,'rb').read()==o)
print('s4 identical to s3',open('$O/s4-flash.bin','rb').read()==open('$O/s3-flash.bin','rb').read())
print('s8 disk unchanged',open('$O/s8-saved.bin','rb').read()==open('$O/disk-s8.orig','rb').read())
P
mdir -i $O/s7-saved.bin :: > $O/s7-mdir.txt 2>&1; fsck.fat -n $O/s7-saved.bin > $O/s7-fsck.txt 2>&1; echo "fsck exit $?" >> $O/s7-fsck.txt
cmp -n 36864 $O/s7-saved.bin $O/s7-saved.bin; python3 - <<P >> $O/flash-checks.txt
d=open('$O/s7-saved.bin','rb').read(); print('s7 data area still FF',set(d[0x9000:])=={255})
P
# S7b the formatted volume: USB read/write, then a file staged on it installs
cp $O/s7-saved.bin $O/disk-s7b.orig; cp $O/s7-saved.bin $O/disk-s7b.bin
timeout 200 python3 ../emu/run.py chain --uid unit --flash $O/a.bin --pa2 -1 --fast --usbip 0.01 --usbip-port 46050 --run-for 20 --name s7b --disk $O/disk-s7b.bin > $O/s7b.out 2>&1 &
sleep 8; CPORT=46050 timeout 40 python3 ctest2.py read $O/disk-s7b.orig > $O/s7b-usb.txt 2>&1
wait
cp $O/s7-saved.bin $O/disk-s7c.bin; mcopy -o -i $O/disk-s7c.bin $FW ::dm40_opendm40_v0.4.0.bin; python3 mkimg.py $O/b.bin --zero-marker
$RUN --flash $O/b.bin --pa2 0 --run-for 5 --usbip-port 46067 --name s7c --disk $O/disk-s7c.bin --shot $O/s7c-app.png --save-flash $O/s7c-flash.bin > $O/s7c.out 2>&1
# S9 USB copy, file first half-written, >3 s pause: error and flash untouched; then the rest: installed by itself
cp $B $O/disk-s9.bin; cp $B $O/disk-s9.orig; cp $O/disk-s9.orig $O/disk-s9.after; mcopy -o -i $O/disk-s9.after $FW ::dm40_usb_copy.bin
S=; for t in 10 15 20 25 30 35 40 45 50 55; do S="$S --shot $O/s9-$t.png@$t"; done
timeout 280 python3 ../emu/run.py chain --uid unit --trace-boot --flash $O/a.bin --pa2 -1 --fast --usbip 0.01 --usbip-port 46050 --run-for 60 --name s9 --disk $O/disk-s9.bin $S --save-flash $O/s9-flash.bin > $O/s9.out 2>&1 &
sleep 8; CPORT=46050 timeout 200 python3 ctest2.py copy $O/disk-s9.orig $O/disk-s9.after > $O/s9-usb.txt 2>&1
wait
# S9b the same copy stopped halfway (metadata + half the data): error on the screen, flash untouched
cp $O/disk-s9.orig $O/disk-s9b.bin
timeout 200 python3 ../emu/run.py chain --uid unit --trace-boot --flash $O/a.bin --pa2 -1 --fast --usbip 0.01 --usbip-port 46050 --run-for 25 --name s9b --disk $O/disk-s9b.bin --shot $O/s9b-half.png --save-flash $O/s9b-flash.bin > $O/s9b.out 2>&1 &
sleep 8; CPORT=46050 STOP_HALF=1 timeout 120 python3 ctest2.py copy $O/disk-s9.orig $O/disk-s9.after > $O/s9b-usb.txt 2>&1
wait
# Stack high-water: format; then verify + USB read/write + idle rescan + list with the installed check
python3 -c "open('$O/disk-r1.bin','wb').write(b'\xff'*(16<<20))"
echo "1.9 monitor sysbus.memtool Save @$O/ram-format.bin 0x20000000 0x1000" > $O/r1.script
$RUN --flash $O/a.bin --pa2 -1 --run-for 2 --usbip-port 46068 --name r1 --disk $O/disk-r1.bin --script $O/r1.script > $O/r1.out 2>&1 &
python3 mklfndisk.py $B $O/disk-r2.orig >/dev/null; mcopy -o -i $O/disk-r2.orig $O/crcbad.bin ::dm40_crc_bad_file.bin; cp $O/disk-r2.orig $O/disk-r2.bin
echo "45 monitor sysbus.memtool Save @$O/ram-use.bin 0x20000000 0x1000" > $O/r2.script
timeout 200 python3 ../emu/run.py chain --uid unit --flash $O/a.bin --pa2 -1 --fast --usbip 0.01 --usbip-port 46050 --run-for 46 --name r2 --disk $O/disk-r2.bin --script $O/r2.script --key auto@3 --key auto@3.5 --key run@4 > $O/r2.out 2>&1 &
sleep 8; CPORT=46050 timeout 60 python3 ctest.py $O/disk-r2.orig > $O/r2-usb.txt 2>&1
wait
python3 - <<P >> $O/flash-checks.txt
for n in ('ram-format','ram-use'):
    r=open('$O/%s.bin'%n,'rb').read(); print(n,'stack high-water',4096-next(i for i,x in enumerate(r) if x))
print('s9b flash identical to the start image',open('$O/s9b-flash.bin','rb').read()==open('$O/a.bin','rb').read())
P
python3 - <<P >> $O/flash-checks.txt
import sys; sys.path.insert(0,'../stock')
import dm40fw as f
dec=f.decode(open('$FW','rb').read()); w0,l,w2,body=f.split(dec)
for n in ('s7c-flash.bin','s9-flash.bin'):
    d=open('$O/'+n,'rb').read(); a=d[0x4000:0x4000+l]
    print(n,'marker==hdr',d[0x8000:0x800c]==dec[:12],'body diffs',len([1 for x,y in zip(a,body) if x!=y]))
P
rm -f $O/disk-*.bin $O/disk-*.orig $O/disk-*.after $O/*-saved.bin $O/ram-*.bin
echo done
