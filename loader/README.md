# Minimal loader (prototype)

A replacement for both the vendor primary bootloader and the FS-loader, small
enough for the free tail of flash sector 0. It has run only in the emulator.
Nothing here installs it: no tool writes sector 0.

Goal: flash sectors 9, 10 and 11 become firmware space, so region C grows from
128 KiB to 512 KiB (`0x08080000..0x08100000`). All of it is slow flash.

## What it does

- Boot: PA2 poll, marker and body CRC check, launch of the application. Hook
  build: the first poll is the vendor routine at `0x080005F4`, the second one
  stands in for the FS-loader's.
- Loader mode: file list with long names, touch and the side keys (their
  actions are labelled at the left edge), install, USB disk (`28e9:028f`).
- One `dm40*.bin` installs without asking; a file that is already installed
  does not install again. Every file is checked end to end before any erase.
- A blank SPI flash gets an empty FAT16. A first sector that is not blank and
  has no valid FAT is left alone.

## Layout in sector 0

| Address | Size | Content |
|---|---|---|
| `0x08000000` | 6 500 B | vendor bootloader; only the word at `0x08000660` changes |
| `0x08001974` | up to 9 036 B | this loader |
| `0x08003CC0` | 800 B | copy of the calibration record (`0x080C0000`) |
| `0x08003FE0` | 24 B | copy of the unit's block (`0x080A4000`) |
| `0x08003FF8` | 4 B | CRC-16 and its complement over both |

The word at `0x08000660` goes from `F7FF FFC8` (`BL` to the PA2 poll) to
`F001 F988` (`BL 0x08001974`). Only 1 -> 0 bits change, so no erase is needed.
The vendor bootloader is identical on the units in `stock/re/other-units`.

The loader copies the calibration and the block to the tail at boot. It erases
sectors 9..11 only for an image that needs them, and only with a valid tail. An
image that ends below `0x080A0000` gets both back at their old addresses, for
the stock application. The FS-loader is not restored.

## Build and test

`./build.sh hook` builds for the tail, `./build.sh` for `0x08000000`, and
`./build.sh core` with the precompiled `core`. Needs `toolchain/` at the top of
the repository.

`FW_REF=<release image> FW_MOD=<image with firmware/src/caltail.rs> ./tests4.py`
runs the emulator checks of the tail. It needs the flash dump in `stock/re/dumps`.

Do not `usbip attach` the emulated disk while anyone installs to a real meter:
`firmware/tools/reload.sh` looks for `28e9:028f` in `lsusb`.

## Open

- It has never run on hardware. Not checked: FMC error bits, the patched `BL`,
  USB with a host beyond listing and reading, touch orientation, key positions,
  other LCD controllers (hardware version 03 units).
- There is no way back. With sector 9 overwritten and SWD locked, a bug in the
  loader leaves only a mass erase, and the loader cannot be updated.
- The tail freezes the calibration: a later one needs its own storage.
- The firmware still links region C to `0x080A0000`; `vdisk.rs`, `FW_MAX_BODY`
  and `upload.rs` do not know the new layout yet.
