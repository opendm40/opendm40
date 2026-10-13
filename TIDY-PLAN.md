# Repo tidy-up plan

**Executed on 2026-10-13** (commits `26ba0e5`..`HEAD` on `rust-fw-poc`), after
the repo moved to the private `wirenboard/opendm40`. Done: protocol rewrite in
`docs/bt-protocol.md`, BT clients in `tools/`, new front-page `README.md`,
`DM40C_feature_request.md` -> `docs/dm40-feature-request.md`, handover ->
`docs/history/`, `flash.py` + `dm40_loader.py` -> `fw-rs/tools/legacy/`, kit
sources -> `firmware/patch/kit/` (+ `mkkit.sh`; the old
`firmware/patch/dm40_memrw.py` is retired), firmware/patch, firmware/re
docs/tools, Ghidra scripts and `web/` tracked; `dist/` and `private/`
ignored; SPI disk backups, `l2bittest/` and the kit test output (UID,
calibration, OTP) moved to `private/`. Deviations: vendor binaries stay at the
repo root (ignored; the plan's alternative, so `firmware/vendor/` was not
created and no path changed); `fw-rs/stub/` stays; the fw-rs review docs were
not moved to `fw-rs/docs/reviews/` (other agents are editing fw-rs). Left for
the owners of those files: the stale names in `fw-rs/TODO.md`,
`fw-rs/src/meter.rs`, `genuine.rs`, `screenshot.rs` (prose only). The text
below is the original plan.

State as of 2026-10-13 (branch `rust-fw-poc`, after commit `efb7e50`). Nothing
has been moved or deleted yet. Already done: `.gitignore` for personal dumps,
SPI disk backups and the Ghidra database (`3aedb6f`), and stale README
statements (`efb7e50`). `firmware/re/history/` was not opened (off-limits); it
is only listed here by name.

## 1. Inventory

T = tracked, U = untracked, I = ignored.

### Top level

| Path | Git | Class | Notes |
|---|---|---|---|
| `README.md` | T | docs | BT protocol of DM40 + EL15. Has a large uncommitted rewrite of the DM40 protocol section (commands 00..09, mode/format bytes, update file format) from an earlier session, still not committed |
| `LICENSE` | T | docs | MIT |
| `dm40.py` | T | active tool | DM40 BT client (RFCOMM); uncommitted rework goes with the README rewrite |
| `el15.py` | T | active tool | EL15 BT client |
| `dm40_loader.py` | U | legacy | Loads a raw image into SRAM over BT (cmds 0x0B/0x0A). **Imported by the tracked `fw-rs/tools/flash.py`** (via `ROOT`), so a fresh clone has a broken `flash.py` |
| `DM40C_feature_request.md` | U | docs | Feature request / UX gap analysis (Russian). Referenced by `fw-rs/docs/dac-adc-paths.md` (4x) and `fw-rs/docs/user-feedback.md` |
| `dm40_v_2_6.bin` | I | vendor binary | Official update file. Default input of `firmware/patch/Makefile`, `fw-rs/README` (`--stock`) and many docs |
| `dm40_v_2_6.dec.bin` | I | generated | Decoded copy (`dm40fw.py decode`); default input of `fw-rs/tools/mkstockfont.py` |
| `atk-xtool.apk` (106 MB) | I | vendor binary | Vendor Android app |
| `DM40 3-in-1 Multimeter.zip`, `DM40_Multimeter_User_Manual_V1.8_RU.zip` | I | vendor docs | |
| `l2bittest/` | I (now) | scratch | 16 generated update images from the checksum brute force |
| `toolchain/` (900 MB) | I | build env | Pinned rustup + cargo; `fw-rs/build.sh` and the test scripts use `../toolchain` |
| `__pycache__/` | I | generated | |
| `.agents/`, `.aws/`, `.codex/` | - | empty dirs | Created by tools, nothing inside |
| `dist/` | U | build output + kit sources | See below |
| `web/` | U | active tool | Web BLE/Serial client for the stock BT protocol (Russian README, tests, `build_standalone.py` -> `dm40.html`) |

### `firmware/`

| Path | Git | Class | Notes |
|---|---|---|---|
| `dm40fw.py` | T | active tool | Update file decode/encode/pack incl. both header CRCs; used by `fw-rs/build.sh` |
| `README.md` | U | docs | Tools + patch kit. Two stale statements fixed in the working tree (header CRC "not understood", "never tested on a device"); still untracked |
| `merge_boot.py` | U | RE tool | Merges bootloader dump pieces |
| `custom-fw-handover.txt` | U | docs (historical) | First handover for the Rust PoC (DMM only, run from SRAM): outdated. Referenced by `fw-rs/docs/native-boot.md` (3x) |
| `patch/` sources (`Makefile`, `*.ld`, `fw_v2_6.{S,h}`, `hooks.h`, `patcher.py`, `dm40_memrw.py`, `AGENTS.md`, `src/*.c,*.S`) | U | active tool | Patch kit for the stock firmware (memrw BT command, USB stream). `fw-rs/tools/reload.sh` and docs point at `firmware/patch/build/dm40_patched.bin` and `firmware/patch/dm40_memrw.py` |
| `patch/build/`, `patch/build-usb/` | I | generated | |
| `re/analysis/*.h`, `*.py`, `*.md`, `app_decompile.c`, `usbcore.dis` | T | RE docs | Stock firmware V2.6 notes used by fw-rs. `app_decompile.c` is a full decompile of the vendor app |
| `re/analysis/gui_render_v2_6.h` | U | RE docs | Same kind as the tracked headers |
| `re/README.md`, `re/ANALYSIS.md` | U | RE docs | `ANALYSIS.md` is referenced by `fw-rs/link-test.x` and five fw-rs docs |
| `re/tools/*.py` | U | RE tools | Capstone helpers |
| `re/ghidra/Setup.java`, `Dump.java` | U | RE tools | |
| `re/ghidra/gproj/` | I (now) | generated | Ghidra DB, embeds the vendor image |
| `re/bootloader*.bin`, `re/loader_0x080a0000.bin`, `re/flash_full_1mb.bin` | I | dumps | Full flash includes calibration + UID-bound boot data: personal |
| `re/dumps/` | I (now) | personal data | Flash/OTP/option-byte dumps, boot data with UID, settings, SPI disk image (`CONFIG.BIN` and `SHA256SUMS` were not covered by `*.bin`) |
| `re/history/` | I (now) | not inspected | Off-limits; ignored as a whole so nothing gets added by accident |
| `spi-disk-backup/` | I (now) | personal data | `CONFIG.BIN`, `S00x.BMP` screenshots from the meter |

### `fw-rs/`

| Path | Git | Class | Notes |
|---|---|---|---|
| `src/`, `Cargo.*`, `.cargo/`, `link-native.x`, `build.sh` (native mode) | T | active | |
| `link.x`, `link-test.x`, `build.sh` RAM / T1 / T2 modes | T | legacy | RAM / sector-8 images loaded over BT |
| `stub/` | T | legacy | RAM stub that installs sector 8 over BT (Stage 2 retired it) |
| `tools/flash.py` | T | legacy | Needs `stub/` + root `dm40_loader.py` + stock firmware |
| `tools/reload.sh`, `dm40dbg.py` | T | active | `reload.sh --ram / test-initcore / test-recovery` are the legacy paths; `test-recovery` is still the documented way from stock to the loader screen |
| `tools/mkfont.py`, `mkstockfont.py`, `mktables.py`, `mklogo.py` (U, new) | T | active (generators) | Outputs `src/fonts.rs`, `src/stockfont_data.rs`, `src/hy3131_tables.rs` are tracked on purpose (mkstockfont needs the vendor binary) |
| `tools/fatren-test/`, `tools/genuine-test/` | T | active (host tests) | Default inputs are personal dumps: `firmware/re/dumps/...`, `dist/dm40-memrw-kit/tools/full/uid_1FFF7A10.bin` (already overridable via `$1`/`$2`) |
| `tools/mr9270.py` | T | active | Calibrator driver |
| `tools/ui-render/` | U (new) | active? | Being added by another agent |
| `docs/*.md` | T | docs | Design docs, reviews (`*review*.md`), user feedback |
| `docs/mockups/*.png`, `docs/mockups/tools/` | T | docs + generators | |
| `fonts/` (Roboto + OFL), `fonts/wb-logo-round.*` (U, new) | T | vendor asset | |
| `ref/stock_dcmv.png` | T | docs | Stock screenshot |
| `dm40-fw.bin`, `dm40-fw-native.bin`, `stub/dm40-stub.bin`, `target/`, `stub/target/` | I | generated | |

### `dist/`

| Path | Class | Notes |
|---|---|---|
| `dm40-memrw-kit.zip` | build output (I) | Checked: 11 files, no personal data (README, tool, tests, two vendor/patched images, SHA256SUMS) |
| `dm40-memrw-kit/README.md` (35 KB, Russian), `tools/dm40_memrw.py` (41 KB), `tools/tests/` | **only copy of the kit sources** | `firmware/patch/dm40_memrw.py` is an older 2.6 KB version. The README uses the user's BT MAC as the example address |
| `dm40-memrw-kit/firmware/*.bin` | vendor / build output (I) | |
| `dm40-memrw-kit/tools/quick.img/`, `tools/full/` | personal data (I, now) | UID, calibration, boot data, OTP, option bytes, manifests from the user's meter: output of a test run that landed inside the kit tree |

## 2. Proposed layout

```
README.md                 short: what is here, links (see section 7)
LICENSE
docs/
  bt-protocol.md          DM40 + EL15 protocol (today's README body)
  dm40-feature-request.md (DM40C_feature_request.md)
  history/custom-fw-handover.txt
tools/
  dm40.py  el15.py        BT clients
web/                      web client (unchanged)
firmware/
  README.md
  dm40fw.py  merge_boot.py
  patch/                  patch kit (sources only; build*/ ignored)
    kit/                  memrw kit sources: README, dm40_memrw.py, tests/, mkkit.sh
  re/
    README.md  ANALYSIS.md
    analysis/  tools/  ghidra/{Setup,Dump}.java
    dumps/  history/  ghidra/gproj/      ignored, stay local
  vendor/                 ignored: dm40_v_*.bin, .dec.bin, apk, manuals
fw-rs/
  ...                     unchanged, plus:
  tools/legacy/           flash.py, dm40_loader.py, (stub/ moved here or kept as fw-rs/stub)
  docs/reviews/           review-*.md, native-review-*.md, native-boot-review.md, ui-review.md
private/                  ignored: spi-disk-backup/, l2bittest/, test dumps (UID, calibration)
dist/                     ignored as a whole: built kits and zips
```

## 3. Moves

| From | To | Note |
|---|---|---|
| README.md protocol sections | `docs/bt-protocol.md` | After the pending protocol rewrite is committed |
| `dm40.py`, `el15.py` | `tools/` | Nothing imports them |
| `DM40C_feature_request.md` | `docs/dm40-feature-request.md` | Update 2 fw-rs docs |
| `firmware/custom-fw-handover.txt` | `docs/history/` | Update `fw-rs/docs/native-boot.md` |
| `dm40_loader.py` | `fw-rs/tools/legacy/` | Fix `flash.py` (`os.path.join(ROOT, 'dm40_loader.py')`) |
| `fw-rs/tools/flash.py` | `fw-rs/tools/legacy/` | Fix `HERE`/`FW`/`ROOT` and `reload.sh` line `exec "$PY" "$FW/tools/flash.py"` |
| `fw-rs/stub/` | keep, or `fw-rs/tools/legacy/stub/` | Moving needs `stub/build.sh` (`../../toolchain`), `flash.py` (`--stub` default), `reload.sh` (`$FW/stub/build.sh`) updates; low value, suggest keep |
| `dist/dm40-memrw-kit/{README.md,tools/dm40_memrw.py,tools/tests/}` | `firmware/patch/kit/` | Then a small script rebuilds `dist/` from it; replace the old `firmware/patch/dm40_memrw.py` with the kit version (check `reload.sh` hint and `native-boot.md` which name it) |
| `dm40_v_2_6.bin`, `.dec.bin`, apk, vendor zips | `firmware/vendor/` | Many references, see section 8; alternative: keep them at the root (already ignored) |
| `firmware/spi-disk-backup/`, `l2bittest/` | `private/` | |
| `dist/.../tools/full/uid_1FFF7A10.bin` and the dumps the host tests use | `private/` | Update the defaults of `genuine-test/run.sh`, `fatren-test/run.sh` |
| fw-rs review docs | `fw-rs/docs/reviews/` | Optional; do it when no agent is editing fw-rs/docs |

## 4. .gitignore

Done (`3aedb6f`): `firmware/re/dumps/`, `firmware/re/history/`,
`firmware/spi-disk-backup/`, `dist/*/tools/quick.img/`, `dist/*/tools/full/`,
`firmware/re/ghidra/gproj/`, `l2bittest/`. Verified: no tracked file is
ignored (`git ls-files -ci --exclude-standard` is empty), and only the dump,
backup, kit test-output and Ghidra DB entries left `git status`.

After the moves add: `dist/` (whole), `private/`, `firmware/vendor/`.
`dist/` is not ignored yet because it holds the only copy of the kit sources.

The blanket `*.bin` rule also hides any future `.bin` that should be tracked;
such a file needs a `!path` exception.

## 5. Untracked files that should be tracked

Source and docs that tracked files or docs already depend on:

- `firmware/README.md`, `firmware/merge_boot.py`
- `firmware/patch/` sources (not `build*/`, not `__pycache__/`)
- `firmware/re/README.md`, `firmware/re/ANALYSIS.md`,
  `firmware/re/analysis/gui_render_v2_6.h`, `firmware/re/tools/*.py`,
  `firmware/re/ghidra/{Setup,Dump}.java`
- `web/` (all of it, `dm40.html` included: it is the double-click build)
- `DM40C_feature_request.md`
- `dm40_loader.py` (or move to `fw-rs/tools/legacy/` first), since tracked `flash.py` needs it
- the kit sources from `dist/` (after the move)

User decision first: the repo has a GitHub remote. `firmware/patch/` embeds
addresses and stubs of the vendor firmware, `re/` holds vendor RE notes, and
`app_decompile.c` (already tracked) is a full decompile of the vendor app.

## 6. Untracked but kept locally

`toolchain/`, `firmware/re/dumps/`, `firmware/re/history/`,
`firmware/re/*.bin`, `firmware/re/ghidra/gproj/`, `firmware/spi-disk-backup/`,
vendor binaries and manuals, `dist/` build outputs, `private/`.

## 7. Delete candidates (not deleted)

- `l2bittest/` (scratch output, regenerable)
- `.agents/`, `.aws/`, `.codex/` (empty)
- every `__pycache__/`, `firmware/patch/build*/`, `fw-rs/target/`, `fw-rs/stub/target/` (regenerable)
- `dm40_v_2_6.dec.bin` (regenerable with `dm40fw.py decode`; but it is the default input of `mkstockfont.py`)
- `dist/dm40-memrw-kit/tools/quick.img/`, `tools/full/` once the host tests no longer read them (copy to `private/` first)
- `firmware/patch/dm40_memrw.py` once the kit version replaces it

## 8. README restructure

Top level (`README.md`), short:

1. What this is: DM40 work (replacement firmware, patch kit, BT protocol) + EL15 protocol.
2. Links: `fw-rs/README.md` (build / install / recovery), `firmware/README.md`
   (update file tool, patch kit, memrw backup kit), `docs/bt-protocol.md`,
   `web/README.md`, `firmware/re/README.md`.
3. Safety note: back up calibration before flashing anything (memrw kit),
   recovery via the vendor FS-loader.
4. Toolchain note: `toolchain/` is local and ignored; how to create it.

Done now (`efb7e50`): a short pointer list to fw-rs, `dm40fw.py` and the RE
notes in the intro (links only to tracked paths), and the stale "Not
implemented: screenshots ..., AC+DC (DM40B/C only)" line in `fw-rs/README.md`.
In the working tree only (inside uncommitted text of others): the update file
layout in `README.md` now describes both header CRCs instead of "Unknown";
`firmware/README.md` (untracked) no longer says the header CRC is unknown and
the patches were never tested.

## 9. References the moves would break

| Reference | Where | Affected by |
|---|---|---|
| `../toolchain/{rustup,cargo}` | `fw-rs/build.sh`; `../../toolchain` in `fw-rs/stub/build.sh`; `../../../toolchain` in both `tools/*-test/run.sh` | moving `fw-rs/`, `stub/` or `toolchain/` |
| `../firmware/dm40fw.py` | `fw-rs/build.sh` (3x), `fw-rs/tools/fatren-test/run.sh` (`FW=`), `firmware/patch/Makefile` (`../dm40fw.py`), `patcher.py` | moving `dm40fw.py` |
| `ROOT/dm40_loader.py` | `fw-rs/tools/flash.py` | moving the loader or `flash.py` |
| `$FW/tools/flash.py`, `$FW/stub/build.sh` | `fw-rs/tools/reload.sh` | moving legacy tools |
| `$ROOT/firmware/patch/build/dm40_patched.bin`, `$ROOT/firmware/patch/dm40_memrw.py` | `fw-rs/tools/reload.sh` hints, `fw-rs/README.md`, `fw-rs/docs/native-boot.md`, `fsloader-install-trigger.md` | moving the patch kit |
| `../../dm40_v_2_6.bin` | `firmware/patch/Makefile` (`ORIG ?=`), `firmware/patch/AGENTS.md`, `firmware/README.md` | moving vendor binaries |
| `../dm40_v_2_6.bin`, `../dm40_v_2_6.dec.bin` | `fw-rs/README.md` (`--stock`), `fw-rs/tools/mkstockfont.py` default | same |
| `dm40_v_2_6.bin` / `.dec.bin` (prose) | `fw-rs/docs/native-boot.md`, `stage0-test.md`, `native-review-2026-10-13.md`, `fsloader-install-trigger.md`, `review-2026-10-13.md`, `firmware/re/analysis/*.h` headers | same (prose only) |
| `firmware/re/dumps/full-20261013/*.bin`, `firmware/re/dumps/dump_settings.bin` | `fw-rs/tools/fatren-test/run.sh`, `genuine-test/run.sh`, `fw-rs/docs/genuine-check.md`, `fsloader-install-trigger.md`, `native-review-2026-10-13.md` | moving dumps to `private/` |
| `dist/dm40-memrw-kit/tools/full/uid_1FFF7A10.bin` | `fw-rs/tools/genuine-test/run.sh`, `fw-rs/docs/genuine-check.md` | moving / deleting the kit test output |
| `firmware/re/ANALYSIS.md` | `fw-rs/link-test.x`, `fw-rs/docs/native-boot.md`, `native-boot-review.md`, `stage0-test.md`, `fsloader-install-trigger.md` | moving `re/` |
| `firmware/re/analysis/...` | `fw-rs/README.md`, `fw-rs/docs/{parity,ui-review,scope-ui-plan,dac-adc-paths,genuine-check}.md`, `custom-fw-handover.txt` | moving `re/` |
| `../DM40C_feature_request.md`, `DM40C_feature_request.md` | `fw-rs/docs/user-feedback.md`, `fw-rs/docs/dac-adc-paths.md` | moving the feature request |
| `custom-fw-handover.txt` | `fw-rs/docs/native-boot.md` | moving the handover |
| `tools/flash.py`, `stub/` | `fw-rs/README.md` (legacy section), `fw-rs/docs/native-boot.md` | moving legacy tools |
| `dm40.py`, `el15.py` (prose) | `README.md` intro, protocol note on `dm40.py` | moving the BT clients to `tools/` |
| `firmware/re/history/` (prose) | `fw-rs/docs/fsloader-install-trigger.md`, `native-boot-review.md`, `native-review-2026-10-13.md` | moving `history/` |

Already broken (no move needed):

- `firmware/re/README.md` lists `tools/l2.c`, `l2b.c`, `l2c.c`; they are not in `firmware/re/tools/`.
- `fw-rs/tools/flash.py` in a fresh clone (needs the untracked `dm40_loader.py`).
- `fw-rs/README.md`, `reload.sh` and docs refer to `firmware/patch/`, which is untracked.
- Host tests default to personal dumps that are not in the repo (documented as overridable).

## 10. Personal data notes

- The unit's BT MACs are in `README.md` and
  `dm40.py` since the first commit (2025-09-01); the kit README uses the same
  MAC as the example.
- The first 4 bytes of the chip UID appear as an example in the tracked
  `fw-rs/docs/genuine-check.md` (line 121).
- Calibration, boot data, OTP and full UID exist only in ignored files.
