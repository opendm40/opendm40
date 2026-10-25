# opendm40 firmware: developer README

Sources, tools and docs of opendm40. Features, install, backup and recovery
are in the [root README](../README.md). Paths below are relative to
`firmware/`.

## Build

```
./build.sh                    # -> opendm40.bin + dm40_opendm40_v<version>.bin
tools/reload.sh               # build, upload over USBTMC, install, check
tools/ui-render/run.sh OUT    # render every screen to PNG + layout check
tools/scpi-doc-check/run.sh   # SCPI parser vs docs/manual/scpi-reference.md
tools/linkcheck.py ..         # every relative link and #anchor in the *.md files
../emu/run.py fwrs --run-for 4 --shot out.png   # run opendm40 in the emulator
```

`build.sh` links with [`link-native.x`](link-native.x), checks the stack
([`tools/stack-depth/stackdepth.py`](tools/stack-depth/stackdepth.py)) and
the flash / SRAM budgets, and packs the body into the stock update format with
[`../stock/dm40fw.py`](../stock/dm40fw.py). The emulator
([emu/README.md](../emu/README.md)) needs a flash dump of your own meter (not
in the repo).

The version is the package version in [`Cargo.toml`](Cargo.toml) without a
`.0` patch (0.1.0 -> `0.1`, [`src/ident.rs`](src/ident.rs),
[`tools/fw-version.sh`](tools/fw-version.sh)): `*IDN?`, Settings > Device,
the USB disk texts, the CSV headers and the install file name
`dm40_opendm40_v<version>.bin` take it (a release asset:
`dm40_opendm40_v<version>[-preN].bin`, from a version such as 0.1.0-pre1). Any `dm40*.bin` name installs: the
FS-loader takes the first `dm40*.bin` in the root, and a long name's 8.3
alias (`DM40_O~1.BIN`) starts with `DM40` too.

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

Without `../toolchain` (CI, another checkout) `build.sh` takes the rustup
install that `RUSTUP_HOME` / `CARGO_HOME` name (default `~/.rustup`,
`~/.cargo`), with the same target and `llvm-tools`. The image bytes depend
only on the commit and the rustc version: `build.sh` remaps every absolute
path (checkout, rustup, cargo registry) and nothing in the image is
time-stamped, so rustc 1.99.0 gives the same `opendm40.bin` in any
directory and with a fresh `CARGO_HOME`. CI pins 1.99.0; that its image
matches a local build is inferred (compare the artifact's `SHA256SUMS`).

## CI and releases

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs on every push
and pull request to `main`; a push that changes only documentation (Markdown, `docs/`, `firmware/docs/`, `hw/`, images, data) skips the Build and Host tests jobs:

| Job | What |
|---|---|
| Build | `build.sh --locked` (stack, region and update-file checks fail it), [`tools/build-variants`](tools/build-variants/run.sh); the image and `SHA256SUMS` as a workflow artifact |
| Host tests | [`tools/ci-tests.sh`](tools/ci-tests.sh): the host suites listed in the script, `JOBS` at a time (default: the CPU count; each suite's log printed whole after all finish); `stock-ui` decodes [`dm40_v_2_6.bin`](../stock/vendor/dm40_v_2_6.bin) first |
| Doc checks | [`tools/linkcheck.py`](tools/linkcheck.py), [`tools/scpi-doc-check`](tools/scpi-doc-check/run.sh) |
| Version | [`tools/version-check.sh`](tools/version-check.sh): fails when the firmware changed since the last `v*` tag but the version did not |

The emulator tests ([emu/README.md](../emu/README.md)) are not in CI: they
need a flash dump of a meter.

### Releasing

Releases follow [Semantic Versioning](https://semver.org/): the version is
`MAJOR.MINOR.PATCH` in [`Cargo.toml`](Cargo.toml), shown as it is, and the tag
is `v` + the version. While MAJOR is 0:

| Change | Bump | Example |
|---|---|---|
| New features, changed behaviour, or anything incompatible (SCPI, settings file, USB ID) | MINOR | `0.2.0` -> `0.3.0` |
| Fixes only | PATCH | `0.2.0` -> `0.2.1` |

MAJOR becomes 1 when the SCPI command set and the settings file are declared
stable; after that an incompatible change bumps MAJOR. Pre-release suffixes
(`-rc1`) are allowed but not used so far.

1. Set `version` in [`Cargo.toml`](Cargo.toml) and run `./build.sh` once, so
   that `Cargo.lock` follows (CI builds with `--locked` and fails on a stale
   lock).
2. In [`../CHANGELOG.md`](../CHANGELOG.md), rename `## Unreleased` to
   `## v<version> (<date>)` and start a new empty `## Unreleased` above it.
3. Commit both files, push the branch, wait for CI.
4. Tag and push the tag: `git tag -a v0.1.0-pre2 -m "opendm40 v0.1.0-pre2"`,
   `git push origin v0.1.0-pre2`.

The tag starts [`.github/workflows/release.yml`](../.github/workflows/release.yml):
the CI jobs, then a check that the tag is `v` + [`tools/fw-version.sh`](tools/fw-version.sh)
(a mismatch fails the release), a clean `build.sh --locked`, and the GitHub
release with `dm40_opendm40_v<version>.bin` and `SHA256SUMS`, a pre-release
for a `-pre` / `-rc` tag. The notes ([`tools/release-notes.sh`](tools/release-notes.sh))
are the install, backup and recovery steps, then the tag's
`CHANGELOG.md` section, or GitHub's generated notes when there is none.

After a release, the next firmware change needs a new version before it is
pushed (the Version job fails otherwise): bump to the next `-preN` or
release version in the same commit.

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

User documentation: the [user manual](../docs/manual/README.md) and the
[SCPI reference](../docs/manual/scpi-reference.md). Developer documents,
one line each:

### Reference: protocols, formats, SCPI

- [scpi-reference.md](../docs/manual/scpi-reference.md): every SCPI command (checked against the parser by `tools/scpi-doc-check`)
- [bt-scpi.md](docs/bt-scpi.md): SCPI over Bluetooth next to the stock app protocol; USBTMC on Windows
- [bt-protocol.md](../docs/bt-protocol.md): the stock Bluetooth phone-app protocol
- [compact-log-format.md](docs/compact-log-format.md): the DLG log format on the disk, CSV on read
- [native-boot.md](docs/native-boot.md): flash and memory map, update format, shim, recovery, the USB disk, memory budget
- [model-variants.md](docs/model-variants.md): how the firmware tells DM40A / B / C apart
- [parity.md](docs/parity.md): item-by-item parity with the stock firmware V2.6
- [stock/README.md](../stock/README.md): stock update-file tools

### Stock-firmware analysis (private)

The reverse-engineering material on the stock firmware lives in the private
repo `wirenboard/alientek-dm40-fw-analysis`; those with access clone it into
`stock/re/` (ignored by git), so the `stock/re/...` paths in the code and the
docs resolve. The emulator needs the developer's own flash and SPI flash
dumps in `stock/re/dumps/` ([emu/README.md](../emu/README.md)). The build and
the CI suites do not need the clone. In it:

- `stock/re/docs/fsloader-install-trigger.md`: how the stock FS-loader installs a `dm40*.bin`, its USB and error paths
- `stock/re/docs/genuine-check.md`: the stock genuine-device check (UID-bound loader data); opendm40 has no implementation of it (the former `DBG:GENUINE?` and its host test are in the private repo, `opendm40-private/`)
- `stock/re/docs/stock-scope-ui.md`: pixel and interaction spec of the stock scope UI ([renderer](tools/stock-ui/README.md))
- `stock/re/ANALYSIS.md`: reverse-engineering notes on the stock firmware and flash
- `stock/re/docs/history/native-boot-plan.md`: the 2026-10-13 native-image plan and its T1 / T2 / T3 tests
- `stock/re/docs/history/acdc-stock-vs-opendm40.md`: AC+DC and AUTO coupling of the stock, measured live

### Design

- [dev-notes.md](docs/dev-notes.md): the USB debug link and the developer workflow
- [ram-budget.md](docs/ram-budget.md): RAM users and when each is live
- [fonts.md](docs/fonts.md): the stock fonts, their cost and storage
- [power-meter-and-misconnect.md](docs/power-meter-and-misconnect.md): power meter and wrong-connection alerts
- [inrush-current.md](docs/inrush-current.md): inrush current capture
- [dmm-input-scope-feasibility.md](docs/dmm-input-scope-feasibility.md): the HY3131 AD2 stream: wave view, fast logger
- [acdc-approaches.md](docs/acdc-approaches.md): AC+DC in other meters and DMM chips, what fits the HY3131
- [dmm-rate-filter-survey.md](docs/dmm-rate-filter-survey.md): DMM reading rate, integration time and filtering
- [dac-adc-paths.md](docs/dac-adc-paths.md): DAC to ADC paths for self-test
- [combined-view-design.md](docs/combined-view-design.md): the DMM + scope combined view
- [scope-scale-ux.md](docs/scope-scale-ux.md): scope scale and position by touch
- [user-feedback.md](docs/user-feedback.md): DM40 owners' feedback triaged into UF-nn items
- [dm40-feature-request.md](../docs/dm40-feature-request.md): the FR-nn feature list (Russian)

### Hardware

- [hw/README.md](../hw/README.md): the board: block diagram, parts, pins, DMM front end, scope, power; the only place for hardware facts
- [HY3131 model](../emu/hy3131/README.md), [HY3131 research notes](../emu/hy3131/RESEARCH.md), [HY3131 registers](../emu/hy3131/ref/registers.md)

### Testing

- [emu/README.md](../emu/README.md): the emulator (Renode): run, screenshots, scenarios
- Host tests: `tools/*/run.sh` (one suite per directory in [`tools/`](tools), e.g. [`fatren-test`](tools/fatren-test), [`scpi-test`](tools/scpi-test), [`touch-test`](tools/touch-test), [`ui-render`](tools/ui-render)); `tools/linkcheck.py` and `tools/scpi-doc-check` for the docs
- [device-check-2026-10-21.md](docs/device-check-2026-10-21.md): device test log, appended per build

### History

Finished plans, reviews, investigations and decision logs; not maintained.
The full list is [docs/history/README.md](../docs/history/README.md). Archived from `docs/`:

- [stage0-test.md](../docs/history/stage0-test.md): Stage 0, the FS-loader re-installs a staged file
- [decisions-2026-10-22.md](../docs/history/decisions-2026-10-22.md): nine product / engineering decisions, cited by the code as `#N`
- [switch-latency.md](../docs/history/switch-latency.md): DMM switch latency and the proposals cited by the code
- [log-format-standards.md](../docs/history/log-format-standards.md): standard log formats against DLG
- [ui-review.md](../docs/history/ui-review.md): UI/UX review
- [hwtrig-integration.md](../docs/history/hwtrig-integration.md): the hooks that wired the comparator trigger
- [integration-queue.md](../docs/history/integration-queue.md): the queue of hooks into shared files (empty)
