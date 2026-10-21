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
- **Bluetooth protocol** of the stock DM40 firmware (and of the Alientek EL15
  electronic load), with Python clients and a web client.
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

- **Back up first.** Each meter has its own calibration record (flash
  `0x080C0000`) and UID-bound boot data (`0x080A4000`). Neither is part of an
  update file and neither can be rebuilt. Before you flash anything, make a
  backup with the memrw kit
  ([firmware/patch/kit/README.md](firmware/patch/kit/README.md),
  `dm40_memrw.py dump --quick`, better also `--full`) and keep it off the meter.
  fw-rs reads calibration in place, never rewrites it, and also writes a copy
  to its USB disk.
- **Recovery.** Updates only rewrite the application from `0x08004000`; the
  bootloader and the vendor FS-loader (`0x080A0000`) stay. On the FS-loader's
  update screen (with fw-rs installed it opens after two watchdog resets in a
  row, or by holding the power key from off) copy one good update file onto its
  USB disk and press "update": the official `dm40_v_2_6.bin` returns to stock.
  Details:
  "Recovery" in [fw-rs/README.md](fw-rs/README.md),
  [fw-rs/docs/native-boot.md](fw-rs/docs/native-boot.md).
- The stock firmware installs any `dm40*.bin` it finds on its USB disk. Do not
  leave files with that name there by accident.
- Dumps contain personal data (calibration, chip UID, boot data). Keep them in
  `private/` or `firmware/re/dumps/` (both ignored); never commit them.

## Layout

| Path | What |
|---|---|
| [fw-rs/](fw-rs/README.md) | Replacement firmware: build, install, reload, recovery, debug link (`tools/dm40dbg.py`), host tests, design docs in `fw-rs/docs/` |
| [firmware/](firmware/README.md) | `dm40fw.py` (update file decode / encode / pack incl. header CRCs), `merge_boot.py` |
| [firmware/patch/](firmware/patch/AGENTS.md) | Patch kit for the stock firmware (BT memory read/write, USB live stream) |
| [firmware/patch/kit/](firmware/patch/kit/README.md) | memrw backup kit: `dm40_memrw.py`, tests, `mkkit.sh` (builds `dist/`) |
| [firmware/re/](firmware/re/README.md) | Reverse-engineering notes ([ANALYSIS.md](firmware/re/ANALYSIS.md), `analysis/*.h`), capstone helpers, Ghidra scripts |
| [docs/bt-protocol.md](docs/bt-protocol.md) | Bluetooth protocol of the stock DM40 and of the EL15 |
| [tools/](tools/) | `dm40.py`, `el15.py`: minimal Python BT clients |
| [web/](web/README.md) | Web Bluetooth / Web Serial client for the stock DM40 protocol |
| [docs/dm40-feature-request.md](docs/dm40-feature-request.md) | DM40C feature request and UX gap analysis (Russian) |
| `docs/history/` | Outdated handover notes, kept for reference |

## Vendor files

The vendor firmware update files, the Android app and the manuals are not in
this repo. Copies of the official update files live in
`wirenboard/alientek-dm40-tools` (`vendor/`). The tools expect the official
`dm40_v_2_6.bin` at the repo root (ignored by git); `fw-rs/tools/mkstockfont.py`
also needs its decoded copy:

```
python3 firmware/dm40fw.py decode dm40_v_2_6.bin dm40_v_2_6.dec.bin
```

## Toolchain

- fw-rs: a pinned Rust toolchain in `toolchain/` at the repo root (ignored,
  local). `fw-rs/build.sh` and the host tests use `../toolchain`. Create it
  once (tested with stable 1.99):

  ```
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
  RUSTUP_HOME=$PWD/toolchain/rustup CARGO_HOME=$PWD/toolchain/cargo \
      sh /tmp/rustup-init.sh -y --no-modify-path --profile minimal \
      --default-toolchain stable --target thumbv7em-none-eabihf --component llvm-tools
  cd fw-rs && ./build.sh             # -> dm40-fw-native.bin
  ```

- Patch kit: `gcc-arm-none-eabi`, `make`, `zip`.
- Host tests: `mtools` (`fw-rs/tools/fatren-test`), `node` (`web/`).
- Bluetooth tools need a Python with `AF_BLUETOOTH` sockets
  (`/usr/bin/python3` on Linux; the linuxbrew one has none).

## License

MIT, see [LICENSE](LICENSE). `firmware/re/` and `firmware/patch/` contain
addresses, symbols and decompiled code of the vendor firmware; the vendor
binaries themselves are not included. `fw-rs/fonts/` carries its own font
license.
