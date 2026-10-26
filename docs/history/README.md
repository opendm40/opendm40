# docs/history: archived documents

Plans, reviews and handover notes whose work is done or superseded. They are
kept as a record (the commits they cite, the reasoning behind decisions) and
are not maintained: paths and statements inside them describe the repository
as it was when they were written. Current documentation: the [documentation index](../README.md), with the
[user manual](../manual/README.md). Commit hashes in these files may predate
a rewrite of the repository history and then do not resolve.

| File | What it was | Why archived |
|------|-------------|--------------|
| [`custom-fw-handover.txt`](custom-fw-handover.txt) | First handover of the Rust PoC (DMM only, run from SRAM) | Superseded by [`firmware/README.md`](../../firmware/README.md) and [`firmware/docs/native-boot.md`](../../firmware/docs/native-boot.md) |
| [`TIDY-PLAN.md`](TIDY-PLAN.md) | Repo tidy-up plan of 2026-10-13 (file moves, link map) | Executed (b06da47..369cad7) |
| [`review-2026-10-13.md`](review-2026-10-13.md) | Code review of the 2026-10-13 working-tree changes (bt, gen, scope, ui, stockfont, logger) | Point-in-time review; findings fixed ([`firmware/TODO.md`](../../firmware/TODO.md)) |
| [`scope-ui-plan.md`](scope-ui-plan.md) | The firmware's own scope UI redesign (problems, target design, steps) | Superseded by the stock scope page port; its gesture rules (section 2.6) are still cited by [`gesture.rs`](../../firmware/src/gesture.rs), [`dialog.rs`](../../firmware/src/dialog.rs), [`app.rs`](../../firmware/src/app.rs) |
| [`stock-scope-port-plan.md`](stock-scope-port-plan.md) | Plan to replace the first scope UI with a replica of the stock V2.6 page (S1..S9) | Implemented; the code is in [`scope_ui.rs`](../../firmware/src/scope_ui.rs) / [`scope_page.rs`](../../firmware/src/scope_page.rs) |
| [`scope-lag-fixes.md`](scope-lag-fixes.md) | Static analysis of "the oscilloscope lags" with proposed fixes | Fixes landed (fix A is cited by `scope.rs`); a proposal, not a description of the current code |
| [`stage0-test.md`](stage0-test.md) | Native-boot Stage 0: the FS-loader re-installs a `dm40*.bin` from the SPI disk (2026-10-13) | Ran with the RAM-loaded build, which is gone; the install path is in [`native-boot.md`](../../firmware/docs/native-boot.md) |
| [`hwtrig-integration.md`](hwtrig-integration.md) | The H0..H8 hooks that wire [`hwtrig.rs`](../../firmware/src/hwtrig.rs) into the scope | Integrated (a870070); the engine is described in the module header |
| [`ui-review.md`](ui-review.md) | UI/UX review of the firmware (fonts, layout, gestures) | Point-in-time review; its items are in [`TODO.md`](../../firmware/TODO.md) |
| [`log-format-standards.md`](log-format-standards.md) | Standard log formats (MDF4, HDF5, ...) compared with DLG, and a range-coder DLG2 | Survey finished: DLG stays ([`compact-log-format.md`](../../firmware/docs/compact-log-format.md)) |
| [`decisions-2026-10-22.md`](decisions-2026-10-22.md) | Nine product / engineering decisions of 2026-10-22 (Record page, timestamps, AD2 stream ISR, energy counter, status slot, "Files live", DMA frames, the DMM SPI divider, 76.8 kS/s) | Decision log; code and [`TODO.md`](../../firmware/TODO.md) cite it as `#N` |
| [`switch-latency.md`](switch-latency.md) | DMM function switch latency against the stock, the proposals 1..8 and what landed (branches fastswitch, fastswitch2) | Implemented; code cites it as "proposal N" / "section 6" |
| [`device-check-2026-10-21.md`](device-check-2026-10-21.md) | Device test log of 2026-10-21 .. 2026-10-25, one section per build | A session log, not a reference; current limits are in the manual's [Known limitations](../manual/README.md#known-limitations). Code and docs cite its sections by name |
| [`dm40-feature-request.md`](dm40-feature-request.md) | Feature request and UX gap analysis for an alternative DM40C firmware (Russian), the FR-nn items | The plan the firmware was built from; code, [`TODO.md`](../../firmware/TODO.md) and [hw/README.md](../../hw/README.md) cite its FR-nn ids |
| [`emu-readme-2026-10-25.md`](emu-readme-2026-10-25.md) | The full emulator README before the rewrite: speed measurements, the USB/IP server fixes, kernel attach results, scope-trigger observations, the input-marker offset history | Replaced by the short task guide [emu/README.md](../../emu/README.md) |
