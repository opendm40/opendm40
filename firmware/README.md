# opendm40 firmware: developer README

Sources, tools and docs of opendm40. Features, install, backup and recovery
are in the [root README](../README.md). Paths below are relative to
`firmware/`.

## Build

```
./build.sh                    # -> opendm40.bin (needs ../toolchain)
tools/reload.sh               # build, upload over USBTMC, install, check
tools/ui-render/run.sh OUT    # render every screen to PNG + layout check
tools/scpi-doc-check/run.sh   # SCPI parser vs docs/manual/scpi-reference.md
../emu/run.py fwrs --run-for 4 --shot out.png   # run opendm40 in the emulator
```

`build.sh` links with [`link-native.x`](link-native.x), checks the stack
([`tools/stack-depth/stackdepth.py`](tools/stack-depth/stackdepth.py)) and
the flash / SRAM budgets, and packs the body into the stock update format with
[`../stock/dm40fw.py`](../stock/dm40fw.py). The emulator
([emu/README.md](../emu/README.md)) needs a flash dump of your own meter (not
in the repo).

### Toolchain

A pinned Rust toolchain in `toolchain/` at the repo root (ignored, local).
`build.sh` and the host tests use `../toolchain`. Create it once (tested with
stable 1.99), from the repo root:

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
RUSTUP_HOME=$PWD/toolchain/rustup CARGO_HOME=$PWD/toolchain/cargo \
    sh /tmp/rustup-init.sh -y --no-modify-path --profile minimal \
    --default-toolchain stable --target thumbv7em-none-eabihf --component llvm-tools
```

The host tests also need `mtools` ([`tools/fatren-test`](tools/fatren-test)).

## Layout

| Path | What |
|---|---|
| [`src/`](src) | The firmware (crate `opendm40`) |
| [`tools/`](tools) | Host tools ([`dm40dbg.py`](tools/dm40dbg.py) USB debug link, [`dm40scpi.py`](tools/dm40scpi.py) SCPI client, [`dm40bt_scpi.py`](tools/dm40bt_scpi.py), [`reload.sh`](tools/reload.sh), [`dlog.py`](tools/dlog.py), font and table generators) and the host test suites (`tools/*/run.sh`) |
| [`docs/`](docs) | Firmware docs, see [Documentation](#documentation) |
| [`fonts/`](fonts) | Boot logo source (`wb-logo-round.svg` / `.png`) |
| [`ref/`](ref) | Reference screenshot of the stock firmware |
| [`vendor/usbd-storage/`](vendor/usbd-storage) | usbd-storage 3.0.0 with the DM40 patches ([`Cargo.toml`](Cargo.toml)) |
| [`TODO.md`](TODO.md) | What is done and what is open |

## Documentation

Reference:

- [native-boot.md](docs/native-boot.md): update format, shim, install, recovery, memory budget
- [dev-notes.md](docs/dev-notes.md): USB debug link, SCPI list, developer workflow
- [bt-scpi.md](docs/bt-scpi.md): SCPI over Bluetooth and USBTMC on Windows
- [power-meter-and-misconnect.md](docs/power-meter-and-misconnect.md): power meter and misconnect alerts
- [inrush-current.md](docs/inrush-current.md): inrush current capture
- [compact-log-format.md](docs/compact-log-format.md): DLG log format on the disk, CSV on read
- [log-format-standards.md](docs/log-format-standards.md): standard log formats compared with DLG
- [fonts.md](docs/fonts.md): stock fonts, their cost and storage
- [model-variants.md](docs/model-variants.md): how the firmware tells DM40A / B / C apart
- [parity.md](docs/parity.md): item-by-item parity with the stock firmware V2.6
- [genuine-check.md](docs/genuine-check.md): the stock genuine-device check (UID-bound loader data)
- [fsloader-install-trigger.md](docs/fsloader-install-trigger.md): how the FS-loader installs a staged `dm40*.bin`
- [ram-budget.md](docs/ram-budget.md): RAM users and when each is live

Design notes:

- [acdc-approaches.md](docs/acdc-approaches.md): AC+DC in other meters and DMM chips, what fits the HY3131
- [acdc-stock-vs-opendm40.md](docs/acdc-stock-vs-opendm40.md): AC+DC of the stock V2.6, measured live, vs opendm40
- [switch-latency.md](docs/switch-latency.md): DMM function switch latency, opendm40 vs stock
- [dmm-rate-filter-survey.md](docs/dmm-rate-filter-survey.md): DMM reading rate, integration time and filtering
- [dmm-input-scope-feasibility.md](docs/dmm-input-scope-feasibility.md): DMM inputs as a scope source (AD2 streaming)
- [dac-adc-paths.md](docs/dac-adc-paths.md): DAC to ADC loopback paths for self-test
- [hwtrig-integration.md](docs/hwtrig-integration.md): the hardware trigger comparator
- [combined-view-design.md](docs/combined-view-design.md): DMM + scope combined view, scope gestures
- [scope-scale-ux.md](docs/scope-scale-ux.md): scope scale and position by touch
- [stock-scope-ui.md](docs/stock-scope-ui.md): pixel and interaction spec of the stock scope UI
- [ui-review.md](docs/ui-review.md): UI/UX review
- [user-feedback.md](docs/user-feedback.md): DM40 owners' feedback triaged into items

Logs and history:

- [device-check-2026-10-21.md](docs/device-check-2026-10-21.md): device test log
- [decisions-2026-10-22.md](docs/decisions-2026-10-22.md): product and engineering decisions
- [stage0-test.md](docs/stage0-test.md): native-boot Stage 0 recovery test (historical)
- [integration-queue.md](docs/integration-queue.md): integration queue (empty)
