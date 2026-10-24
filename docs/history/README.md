# docs/history: archived documents

Plans, reviews and handover notes whose work is done or superseded. They are
kept as a record (the commits they cite, the reasoning behind decisions) and
are not maintained: paths and statements inside them describe the repository
as it was when they were written. Current documentation is in the root
[`README.md`](../../README.md), [`firmware/README.md`](../../firmware/README.md) (the index of the developer docs) and [`emu/README.md`](../../emu/README.md).

| File | What it was | Why archived |
|------|-------------|--------------|
| [`custom-fw-handover.txt`](custom-fw-handover.txt) | First handover of the Rust PoC (DMM only, run from SRAM) | Superseded by [`firmware/README.md`](../../firmware/README.md) and [`firmware/docs/native-boot.md`](../../firmware/docs/native-boot.md) |
| [`TIDY-PLAN.md`](TIDY-PLAN.md) | Repo tidy-up plan of 2026-10-13 (file moves, link map) | Executed (26ba0e5..5d3d005) |
| [`native-boot-review.md`](native-boot-review.md) | Review of the first [`native-boot.md`](../../firmware/docs/native-boot.md) (recovery-first boot) against the RE sources | Its findings were folded into [`firmware/docs/native-boot.md`](../../firmware/docs/native-boot.md); the RAM / T1 / T2 paths it reviews are gone |
| [`native-review-2026-10-13.md`](native-review-2026-10-13.md) | Pre-flash safety review of the first native image and shim | One-time gate before the first flash; fixes are in [`native-boot.md`](../../firmware/docs/native-boot.md) and the code |
| [`review-2026-10-13.md`](review-2026-10-13.md) | Code review of the 2026-10-13 working-tree changes (bt, gen, scope, ui, stockfont, logger) | Point-in-time review; findings fixed ([`firmware/TODO.md`](../../firmware/TODO.md)) |
| [`scope-ui-plan.md`](scope-ui-plan.md) | The firmware's own scope UI redesign (problems, target design, steps) | Superseded by the stock scope page port; its gesture rules (section 2.6) are still cited by [`gesture.rs`](../../firmware/src/gesture.rs), [`dialog.rs`](../../firmware/src/dialog.rs), [`app.rs`](../../firmware/src/app.rs) |
| [`stock-scope-port-plan.md`](stock-scope-port-plan.md) | Plan to replace the first scope UI with a replica of the stock V2.6 page (S1..S9) | Implemented; the spec stays in [`firmware/docs/stock-scope-ui.md`](../../firmware/docs/stock-scope-ui.md), the code in [`scope_ui.rs`](../../firmware/src/scope_ui.rs) / [`scope_page.rs`](../../firmware/src/scope_page.rs) |
| [`scope-lag-fixes.md`](scope-lag-fixes.md) | Static analysis of "the oscilloscope lags" with proposed fixes | Fixes landed (fix A is cited by `scope.rs`); a proposal, not a description of the current code |
| [`stage0-test.md`](stage0-test.md) | Native-boot Stage 0: the FS-loader re-installs a `dm40*.bin` from the SPI disk (2026-10-13) | Ran with the RAM-loaded build, which is gone; the install path is in [`native-boot.md`](../../firmware/docs/native-boot.md) |
| [`integration-queue.md`](integration-queue.md) | Queue of hooks into shared files, with the commits that integrated them | Empty since 2026-10-13 |
| [`hwtrig-integration.md`](hwtrig-integration.md) | The H0..H8 hooks that wire [`hwtrig.rs`](../../firmware/src/hwtrig.rs) into the scope | Integrated (4046e8f); the engine is described in the module header |
| [`ui-review.md`](ui-review.md) | UI/UX review of the firmware (fonts, layout, gestures) | Point-in-time review; its items are in [`TODO.md`](../../firmware/TODO.md) |
| [`acdc-stock-vs-opendm40.md`](acdc-stock-vs-opendm40.md) | AC+DC and AUTO coupling of the stock V2.6, measured live, against the firmware at 9421fd5 and later branches | Investigation finished; the result is in [`dmm.rs`](../../firmware/src/dmm.rs) and [`acdc-approaches.md`](../../firmware/docs/acdc-approaches.md) |
| [`log-format-standards.md`](log-format-standards.md) | Standard log formats (MDF4, HDF5, ...) compared with DLG, and a range-coder DLG2 | Survey finished: DLG stays ([`compact-log-format.md`](../../firmware/docs/compact-log-format.md)) |
| [`decisions-2026-10-22.md`](decisions-2026-10-22.md) | Nine product / engineering decisions of 2026-10-22 (Record page, timestamps, AD2 stream ISR, energy counter, status slot, "Files live", DMA frames, the DMM SPI divider, 76.8 kS/s) | Decision log; code and [`TODO.md`](../../firmware/TODO.md) cite it as `#N` |
| [`switch-latency.md`](switch-latency.md) | DMM function switch latency against the stock, the proposals 1..8 and what landed (branches fastswitch, fastswitch2) | Implemented; code cites it as "proposal N" / "section 6" |
