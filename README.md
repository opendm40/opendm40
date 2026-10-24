# opendm40

Open firmware and tooling for the **Alientek DM40** (DM40A / DM40B / DM40C)
handheld multimeter + oscilloscope + signal generator (GD32F427, Cortex-M4F).

What is here:

- **fw-rs**: a replacement firmware written in Rust. Multimeter, scope and
  generator run at the same time; stock-like GUI, menus, touch, the stock
  Bluetooth app protocol and USBTMC/SCPI. It is installed like a stock update
  file and keeps the vendor bootloader, FS-loader and calibration.
- **Stock firmware tooling**: decode / encode / pack the vendor update files
  (`dm40_v_X_Y.bin`), a patch kit that adds code to the stock firmware, and
  the memrw kit that backs up calibration and the whole flash over Bluetooth.
- **Bluetooth protocol** of the stock DM40 firmware (fw-rs keeps it).
- **Reverse-engineering notes** on the stock firmware V2.6, the bootloader and
  the update format, which fw-rs is based on.

## Status

Work in progress, developed and tested on one DM40 unit. fw-rs runs natively
from flash, installed through the vendor FS-loader like a stock update; see
[fw-rs/README.md](fw-rs/README.md) for the feature state and
[fw-rs/docs/parity.md](fw-rs/docs/parity.md) for parity with the stock firmware.
The stock-firmware patches and the memrw kit were used on the device; the BT
protocol notes are for stock V2.6.

## Safety

### Back up with fw-rs

- **Installing fw-rs first is safe.** The vendor FS-loader rewrites only the
  application area from `0x08004000`. The Alientek bootloader (`0x08000000`),
  the FS-loader (`0x080A0000`), its UID-bound boot data (`0x080A4000`) and the
  calibration record (`0x080C0000`) are not touched
  ([fw-rs/docs/native-boot.md](fw-rs/docs/native-boot.md)). fw-rs reads the
  calibration in place and never writes it.
- **Then copy to the PC**, from the fw-rs USB drive:
  - the whole `INFO/` folder (read-only, generated from the MCU memory):

    | File | What |
    |---|---|
    | `CALIB.BIN` | settings + calibration record, `0x080C0000`, 4 KiB. **Unit-specific** |
    | `BOOTDATA.BIN` | UID-bound FS-loader data, `0x080A4000`, 16 KiB. **Unit-specific** |
    | `UID.BIN` | the chip's 96-bit unique ID; says which unit the boot data belongs to. **Unit-specific** |
    | `ROMBOOT.BIN` | the Alientek bootloader, `0x08000000`, 16 KiB |
    | `LOADER.BIN` | the FS-loader incl. the boot data, `0x080A0000`, 128 KiB |
    | `OPTBYTES.BIN`, `OTP.BIN` | option bytes, OTP area |
    | `FIRMWARE.BIN` | the running firmware as a vendor update file |
    | `BACKUP.TXT` | addresses, sizes, CRC-32s and the restore notes |

  - `DEBUG/SPIFLASH.BIN`: the raw 16 MiB SPI flash (the meter's own disk).

  `CALIB.BIN`, `BOOTDATA.BIN` and `UID.BIN` cannot be rebuilt: no update
  file contains them. fw-rs also writes one copy of these three files, once
  and never overwritten, to the root of its internal SPI-flash disk
  ([`fw-rs/src/calbackup.rs`](fw-rs/src/calbackup.rs)), because the SPI flash
  survives an SWD mass erase of the MCU.
- **Full restore after an SWD mass erase** (from `BACKUP.TXT`): write
  `ROMBOOT.BIN` at `0x08000000`, then `LOADER.BIN` at `0x080A0000` and
  `CALIB.BIN` at `0x080C0000`, then install a firmware. The loader data only
  works on the unit whose `UID.BIN` it was made with.
- Keep backups off the meter. Dumps contain personal data (calibration, chip
  UID, boot data): keep them in `private/` or `firmware/re/dumps/` (both
  ignored by git); never commit them.

### Enter the loader / return to stock

The vendor loader's update screen (with its own USB disk) is reachable from a
running fw-rs (`dm40dbg.py update`, "Install it now?", key hold, double hang)
or, whatever the firmware does, by shorting the debug header's TX pad to GND
at power-on. There, copy a fw-rs image or the official
[stock file](firmware/vendor/dm40_v_2_6.bin) as the only `dm40*.bin` and press
"update". Steps: [fw-rs/README.md, Recovery](fw-rs/README.md#recovery);
the short: [hw/README.md, Hardware recovery](hw/README.md#hardware-recovery).

## Layout

| Path | What |
|---|---|
| [fw-rs/](fw-rs/README.md) | Replacement firmware: build, install, reload, recovery, debug link ([`tools/dm40dbg.py`](fw-rs/tools/dm40dbg.py)), host tests, design docs in [`fw-rs/docs/`](fw-rs/docs) |
| [firmware/](firmware/README.md) | [`dm40fw.py`](firmware/dm40fw.py) (update file decode / encode / pack incl. header CRCs), [`merge_boot.py`](firmware/merge_boot.py) |
| [firmware/patch/](firmware/patch/AGENTS.md) | Patch kit for the stock firmware (BT memory read/write, USB live stream) |
| [firmware/patch/kit/](firmware/patch/kit/README.md) | memrw backup kit: [`dm40_memrw.py`](firmware/patch/kit/tools/dm40_memrw.py), tests, [`mkkit.sh`](firmware/patch/kit/mkkit.sh) (builds `dist/`) |
| [firmware/re/](firmware/re/README.md) | Reverse-engineering notes ([ANALYSIS.md](firmware/re/ANALYSIS.md), `analysis/*.h`), capstone helpers, Ghidra scripts |
| [hw/](hw/README.md) | Hardware: chips, pins, front ends, power; teardown photos ([`hw/photos/`](hw/photos)) and datasheets ([`hw/datasheets/`](hw/datasheets)) |
| [docs/bt-protocol.md](docs/bt-protocol.md) | Bluetooth protocol of the stock DM40 |
| [docs/dm40-feature-request.md](docs/dm40-feature-request.md) | DM40C feature request and UX gap analysis (Russian) |
| [docs/history/](docs/history/README.md) | Archived plans, reviews and handover notes (done or superseded), kept for reference |

## Vendor files

[firmware/vendor/](firmware/vendor/README.md) holds the latest official stock firmware,
[`dm40_v_2_6.bin`](firmware/vendor/dm40_v_2_6.bin) (Alientek's, unmodified, not under the MIT licence; see
its README). It returns a meter to stock and feeds the emulator and the
stock-firmware tools. Older versions live in `wirenboard/alientek-dm40-tools`
(`vendor/`). The Android app and the manuals are not in this repo.
`emu/run.py stock` and [`fw-rs/tools/mkstockfont.py`](fw-rs/tools/mkstockfont.py) need the decoded body at
the repo root (ignored by git):

```
python3 firmware/dm40fw.py decode firmware/vendor/dm40_v_2_6.bin dm40_v_2_6.dec.bin
```

## Toolchain

- fw-rs: a pinned Rust toolchain in `toolchain/` at the repo root (ignored,
  local). [`fw-rs/build.sh`](fw-rs/build.sh) and the host tests use `../toolchain`. Create it
  once (tested with stable 1.99):

  ```
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
  RUSTUP_HOME=$PWD/toolchain/rustup CARGO_HOME=$PWD/toolchain/cargo \
      sh /tmp/rustup-init.sh -y --no-modify-path --profile minimal \
      --default-toolchain stable --target thumbv7em-none-eabihf --component llvm-tools
  cd fw-rs && ./build.sh             # -> dm40-fw-native.bin
  ```

- Patch kit: `gcc-arm-none-eabi`, `make`, `zip`.
- Host tests: `mtools` ([`fw-rs/tools/fatren-test`](fw-rs/tools/fatren-test)).
- Bluetooth tools need a Python with `AF_BLUETOOTH` sockets
  (`/usr/bin/python3` on Linux; the linuxbrew one has none).

## License

MIT, see [LICENSE](LICENSE). [`firmware/re/`](firmware/re) and [`firmware/patch/`](firmware/patch) contain
addresses, symbols and decompiled code of the vendor firmware. The vendor
update file in [`firmware/vendor/`](firmware/vendor/README.md) is unmodified
and under its own notice. [`fw-rs/fonts/`](fw-rs/fonts) carries its own font
license.

[`hw/photos/`](hw/photos) and [`hw/datasheets/`](hw/datasheets) are third-party material under their
owners' copyright (the review author, the chip manufacturers), kept for
reference and not covered by the MIT license; see [hw/photos/README.md](hw/photos/README.md),
[hw/datasheets/README.md](hw/datasheets/README.md) and [firmware/vendor/README.md](firmware/vendor/README.md).
