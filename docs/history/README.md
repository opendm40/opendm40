# docs/history: archived documents

Plans, reviews and handover notes whose work is done or superseded. They are
kept as a record (the commits they cite, the reasoning behind decisions) and
are not maintained: paths and statements inside them describe the repository
as it was when they were written. Current documentation is in the root
[`README.md`](../../README.md), [`fw-rs/README.md`](../../fw-rs/README.md), [`fw-rs/docs/`](../../fw-rs/docs) and [`emu/README.md`](../../emu/README.md).

| File | What it was | Why archived |
|------|-------------|--------------|
| [`custom-fw-handover.txt`](custom-fw-handover.txt) | First handover of the Rust PoC (DMM only, run from SRAM) | Superseded by [`fw-rs/README.md`](../../fw-rs/README.md) and [`fw-rs/docs/native-boot.md`](../../fw-rs/docs/native-boot.md) |
| [`TIDY-PLAN.md`](TIDY-PLAN.md) | Repo tidy-up plan of 2026-10-13 (file moves, link map) | Executed (26ba0e5..5d3d005) |
| [`native-boot-review.md`](native-boot-review.md) | Review of the first [`native-boot.md`](../../fw-rs/docs/native-boot.md) (recovery-first boot) against the RE sources | Its findings were folded into [`fw-rs/docs/native-boot.md`](../../fw-rs/docs/native-boot.md); the RAM / T1 / T2 paths it reviews are gone |
| [`native-review-2026-10-13.md`](native-review-2026-10-13.md) | Pre-flash safety review of the first native image and shim | One-time gate before the first flash; fixes are in [`native-boot.md`](../../fw-rs/docs/native-boot.md) and the code |
| [`review-2026-10-13.md`](review-2026-10-13.md) | Code review of the 2026-10-13 working-tree changes (bt, gen, scope, ui, stockfont, logger) | Point-in-time review; findings fixed (fw-rs/TODO.md) |
| [`scope-ui-plan.md`](scope-ui-plan.md) | fw-rs's own scope UI redesign (problems, target design, steps) | Superseded by the stock scope page port; its gesture rules (section 2.6) are still cited by [`gesture.rs`](../../fw-rs/src/gesture.rs), [`dialog.rs`](../../fw-rs/src/dialog.rs), [`app.rs`](../../fw-rs/src/app.rs) |
| [`stock-scope-port-plan.md`](stock-scope-port-plan.md) | Plan to replace the fw-rs scope UI with a replica of the stock V2.6 page (S1..S9) | Implemented; the spec stays in [`fw-rs/docs/stock-scope-ui.md`](../../fw-rs/docs/stock-scope-ui.md), the code in [`scope_ui.rs`](../../fw-rs/src/scope_ui.rs) / [`scope_page.rs`](../../fw-rs/src/scope_page.rs) |
| [`scope-lag-fixes.md`](scope-lag-fixes.md) | Static analysis of "the oscilloscope lags" with proposed fixes | Fixes landed (fix A is cited by `scope.rs`); a proposal, not a description of the current code |
