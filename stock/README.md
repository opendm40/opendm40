# DM40 Firmware Tools

Tools to unpack DM40 firmware update files (`dm40_v_X_Y.bin`) and to build
modified ones. The original firmware is not rebuilt from source: the patched
image is the original binary plus new code compiled from C and linked after
the end of the original image, with branches from the original code into it.

Requirements: Python 3, `gcc-arm-none-eabi`, `make`.

The latest official update file is in [vendor/](vendor/README.md)
([`dm40_v_2_6.bin`](vendor/dm40_v_2_6.bin), Alientek's, not under the MIT licence); older versions are
kept in `wirenboard/alientek-dm40-tools` (`vendor/`). Reverse-engineering notes on the stock firmware, bootloader and
update format: [re/README.md](re/README.md), [re/ANALYSIS.md](re/ANALYSIS.md).

## dm40fw.py

```
python3 dm40fw.py info   dm40_v_2_6.bin
python3 dm40fw.py decode dm40_v_2_6.bin dm40_v_2_6.dec.bin   # whole file, header included
python3 dm40fw.py encode dm40_v_2_6.dec.bin dm40_v_2_6.bin
python3 dm40fw.py unpack dm40_v_2_6.bin body.bin             # image loaded at 0x08004000
python3 dm40fw.py pack   body.bin out.bin           # as is (vendor round trip)
python3 dm40fw.py pack   --pad body.bin out.bin     # new images: whole 128-byte blocks
```

`unpack` + `pack` reproduce all official files from V1.6 to V2.6 byte for byte.

**Short last block: always `--pad` new images.** The file is obfuscated in
128-byte blocks (header included); each block's key position comes from its
byte `0x1F`. If the file is not a multiple of 128 and the short last block is
longer than `0x1F` bytes with its key position at or past its end, this tool
stores that block as is, but the vendor FS-loader decodes it with a stale key
byte, corrupts the tail, fails the body CRC ("update error") and leaves the app
erased. `pack --pad` (`build(pad=True)`) pads the body with 0xFF so header +
body is a multiple of 128 (word 1 and the body CRC include the padding);
`pack` without `--pad` refuses such a body; `info` / `unpack` warn about such
a file (`tail_hazard`). Vendor files are safe (V2.6: 36-byte last block, key
position 8). opendm40 [`build.sh`](../firmware/build.sh) and [`patch/patcher.py`](patch/patcher.py) always pad.

**Header checksum.** `pack` computes both header check words: word 0 is a
CRC-16 of words 1 and 2 with its complement in the high half, word 2 is
`0x00010000` | CRC-16 of the body with its 12 bytes at offset `0x4000` (the
bootloader's marker at flash `0x08008000`) left out. The bootloader checks
both. `--like` is no longer needed. Details: [re/README.md](re/README.md).

## Patches (`patch/`)

```
cd patch
make ORIG=../../dm40_v_2_6.bin          # -> build/dm40_patched.bin
make SRCS="src/startup.c src/my.c"      # choose patch sources
```

- [`dm40.ld`](patch/dm40.ld) places patch code at `0x0807d000` (after the V2.6 image, which
  ends at `0x0807ce18`) and patch variables at `0x2002f800`. Flash above
  `0x080a0000` must not be used: `0x080a4000` holds bootloader data that the
  firmware checks against the chip UID, `0x080c0000` holds settings.
  The patch RAM area is a placeholder; it is not yet verified to be unused.
- [`fw_v2_6.S`](patch/fw_v2_6.S) / [`fw_v2_6.h`](patch/fw_v2_6.h) declare functions of the original firmware that
  patches can call. Addresses are valid for V2.6 only.
- Hooks are declared in C next to the code:

  ```c
  HOOK_CALL(addr, fn, expect);     /* replace the 4-byte bl at addr with bl fn */
  HOOK_JUMP(addr, fn, expect);     /* write b.w fn at addr */
  HOOK_POINTER(addr, fn, expect);  /* write the address of fn at addr */
  ```

  `expect` is the original 32-bit word at `addr` (0 = do not check), so a patch
  built for a different firmware version fails instead of corrupting code.
- [`src/startup.c`](patch/src/startup.c) is always needed: it redirects the call to the C++
  constructors in the reset handler (`0x080041b4`) to `patch_startup()`, which
  initializes patch `.data`/`.bss`, calls `patch_init()` if a patch defines it,
  and then continues with the original startup.
- [`src/demo_version.c`](patch/src/demo_version.c) is a test patch: the About page and the Bluetooth
  version command (02) report firmware version 9.9.
- [`patcher.py`](patch/patcher.py) applies the linked patch to the original update file.

Calling convention: patches are compiled with `-mfloat-abi=hard`. Calls into
the firmware with float arguments are not verified to use the same ABI.

The memrw patch ([`src/memrw.c`](patch/src/memrw.c), Bluetooth memory read/write) has been used on
a device; [patch/AGENTS.md](patch/AGENTS.md) lists what works (including the
64-byte pad rule) and what is still broken.

## memrw backup kit (`patch/kit/`)

Stock V2.6 + the memrw patch, with a host tool that backs up calibration, boot
data, UID and the whole flash over Bluetooth, and switches the model
(A/B/C). User guide (Russian, English summary at the end):
[patch/kit/README.md](patch/kit/README.md).

```
patch/kit/mkkit.sh                         # -> dist/dm40-memrw-kit/ and .zip
python3 -I patch/kit/tools/tests/test_dm40_memrw.py     # offline tests
python3 -I patch/kit/tools/tests/test_bt_discovery.py
/usr/bin/python3 -I patch/kit/tools/dm40_memrw.py dump --quick -o backup_quick
```

## merge_boot.py

Merges bootloader dump pieces into one 16 KB image, see
[re/README.md](re/README.md).
