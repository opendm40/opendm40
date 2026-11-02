#!/bin/bash
# Half copy, then whole copy over USB, with enough wall time.
cd "$(dirname "$0")"; R=$PWD; O=$R/t2; FW=${FW:?path of an opendm40 update file}
export TMPDIR=$R/tmp DM40_EMU_PORTS=46100-46199 DM40_EMU_LOG_MAX_MB=200 MTOOLS_SKIP_CHECK=1 LC_ALL=C.UTF-8
B=../emu/out/disk.bin
python3 mkimg.py $O/a.bin
cp $B $O/d.orig; cp $B $O/d.after; mcopy -o -i $O/d.after $FW ::dm40_usb_copy.bin
cp $B $O/d1.bin; cp $B $O/d2.bin
timeout 560 python3 ../emu/run.py chain --uid unit --trace-boot --flash $O/a.bin --pa2 -1 --fast --usbip 0.01 --usbip-port 46050 --run-for 12 --name s9b --disk $O/d1.bin --shot $O/s9b-half.png --save-flash $O/s9b-flash.bin > $O/s9b.out 2>&1 &
sleep 8; CPORT=46050 STOP_HALF=1 timeout 120 python3 ctest2.py copy $O/d.orig $O/d.after > $O/s9b-usb.txt 2>&1
wait
python3 mkimg.py $O/b.bin --zero-marker
timeout 560 python3 ../emu/run.py chain --uid unit --trace-boot --flash $O/a.bin --pa2 -1 --fast --usbip 0.01 --usbip-port 46051 --run-for 16 --name s9 --disk $O/d2.bin --shot $O/s9-end.png --save-flash $O/s9-flash.bin > $O/s9.out 2>&1 &
sleep 8; CPORT=46051 timeout 200 python3 ctest2.py copy $O/d.orig $O/d.after > $O/s9-usb.txt 2>&1
wait
python3 - <<P > $O/s9-checks.txt
import sys; sys.path.insert(0,'../stock')
import dm40fw as f
dec=f.decode(open('$FW','rb').read()); w0,l,w2,body=f.split(dec)
o=open('$O/a.bin','rb').read()
print('s9b flash identical to the start image',open('$O/s9b-flash.bin','rb').read()==o)
d=open('$O/s9-flash.bin','rb').read(); a=d[0x4000:0x4000+l]
print('s9 marker==hdr',d[0x8000:0x800c]==dec[:12],'body diffs',len([1 for x,y in zip(a,body) if x!=y]))
P
rm -f $O/d.orig $O/d.after $O/d1.bin $O/d2.bin $O/*-flash.bin $O/*.bin
echo done
