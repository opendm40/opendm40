#!/usr/bin/env python3
"""LEGACY: install a RAM/sector-8 dm40-fw image into internal flash sector 8
and start it, over Bluetooth (stub in fw-rs/stub + dm40_loader.py).

Only for the T1/T2 RAM test images (./build.sh test-initcore | test-recovery)
and the old RAM build. Needs the STOCK firmware (or the patched stock) running
on the meter, not native fw-rs. The normal developer flow is the native image:
fw-rs/tools/reload.sh (see fw-rs/README.md).

    /usr/bin/python3 fw-rs/tools/flash.py [--mac MAC] [--image fw-rs/dm40-fw.bin]

Each pass loads the RAM stub (fw-rs/stub) with one chunk of the image appended
through dm40_loader.py. The stub checks the flash first: if the whole image is
already there it just starts it (so a normal start is a single short load);
otherwise it erases the sector (first chunk), programs and verifies the chunk,
and either resets back to the stock firmware for the next chunk or starts the
image after the last one. Run with the stock firmware on the multimeter screen.
"""
import argparse
import os
import struct
import subprocess
import sys
import time
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
FW = os.path.dirname(HERE)
ROOT = os.path.dirname(FW)
sys.path.insert(0, HERE)
sys.path.insert(0, ROOT)

STUB_SPAN = 0x4000          # stub code area; payload at 0x2000C000
PAYLOAD_MAX = 0x10000       # up to 0x2001C000
CHUNK = 0xF000
MAGIC = 0x46344D44


def app_up(timeout):
    import dm40dbg
    t = time.time() + timeout
    while time.time() < t:
        try:
            return dm40dbg.Link().ping()
        except (SystemExit, Exception):
            time.sleep(0.5)
    return None


def bt_load(mac, path):
    for attempt in range(6):
        r = subprocess.run([sys.executable, os.path.join(ROOT, 'dm40_loader.py'), mac, path],
                           capture_output=True, text=True, timeout=120)
        if r.returncode == 0:
            return True
        print('  BT load attempt %d failed: %s' % (attempt + 1, (r.stderr or r.stdout).strip().splitlines()[-1:]))
        time.sleep(4)
    return False


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--mac', default=os.environ.get('DM40_MAC', 'XX:XX:XX:XX:XX:XX'))
    ap.add_argument('--image', default=os.path.join(FW, 'dm40-fw.bin'),
                    help='RAM/sector-8 image (NOT dm40-fw-native.bin)')
    ap.add_argument('--stub', default=os.path.join(FW, 'stub', 'dm40-stub.bin'),
                    help='LEGACY RAM stub (fw-rs/stub/build.sh)')
    a = ap.parse_args()

    img = open(a.image, 'rb').read()
    img += b'\xff' * (-len(img) % 4)
    stub = open(a.stub, 'rb').read()
    assert len(stub) <= STUB_SPAN, 'stub too big'
    assert len(img) <= 128 * 1024, 'image does not fit sector 8'
    icrc = zlib.crc32(img) & 0xFFFFFFFF
    chunks = [(o, img[o:o + CHUNK]) for o in range(0, len(img), CHUNK)]
    print('image %d bytes, crc %08x, %d chunk(s)' % (len(img), icrc, len(chunks)))

    tmp = os.path.join(FW, 'target', 'stub_payload.bin')

    # Probe: stub + header only. Starts the image if flash already holds it.
    hdr = struct.pack('<6I', MAGIC, 0, 0, 0, len(img), icrc)
    open(tmp, 'wb').write(stub + b'\0' * (STUB_SPAN - len(stub)) + hdr)
    print('probe')
    if not bt_load(a.mac, tmp):
        sys.exit('BT load failed')
    p = app_up(15)
    if p:
        print('already installed, dm40-fw running:', p)
        return
    time.sleep(3)

    for i, (off, data) in enumerate(chunks):
        hdr = struct.pack('<6I', MAGIC, off, len(data), zlib.crc32(data) & 0xFFFFFFFF, len(img), icrc)
        blob = stub + b'\0' * (STUB_SPAN - len(stub)) + hdr + data
        open(tmp, 'wb').write(blob)
        print('chunk %d/%d: offset 0x%05x, %d bytes' % (i + 1, len(chunks), off, len(data)))
        if not bt_load(a.mac, tmp):
            sys.exit('BT load failed')
        last = i == len(chunks) - 1
        p = app_up(25 if last else 10)
        if p:
            print('dm40-fw running:', p)
            return
        if last:
            sys.exit('image programmed but the firmware did not show up on USB')
        time.sleep(3)  # stub resets to stock; let it boot to the meter screen


if __name__ == '__main__':
    main()
