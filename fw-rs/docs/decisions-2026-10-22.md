# Decisions 2026-10-22 (five open product / engineering questions)

Taken on the owner's behalf, as delegated. Sources: TODO.md, docs/compact-log-format.md,
docs/log-format-standards.md (section 3, 7), docs/dmm-input-scope-feasibility.md (2.3, 4, 5,
7, 8.5), docs/power-meter-and-misconnect.md (3.11, 3.12, 4.6), src/csvlog.rs, src/meter.rs,
src/logger.rs, src/powermeter.rs, src/persist.rs, src/ui.rs (`hold_label`, `strip_data`),
docs/user-feedback.md, ../docs/dm40-feature-request.md (FR-01, FR-38). Nothing under
firmware/re/history/ was read. No code was changed; this file and the TODO pointers only.

Design principles applied throughout: the stock DM40 UI and touch model stay the baseline and
new features use its visual language; zero overlapping UI (ui-render check before any screen);
robustness over cleverness; the main loop is ISR-free unless a decision below says otherwise.

---

## #1 On-device "Logger" page (interval recording to the disk)

**Question.** Add a page in the MF menu with interval (every reading / 1 s / 10 s / 1 min,
mean or min/max per interval), duration or until stopped, Start/Stop and a remaining-memory
estimate? If yes, what exactly and with which defaults?

**Options.**
- A. Nothing: keep the swipe-down REC (one row per reading, until stopped).
- B. A full "Logger" page: intervals with mean + min + max per row, durations, estimate.
- C. A small "Record" page: intervals with the mean per row (format unchanged), optional
  duration, estimate; the swipe stays the quick path and uses the page's settings.

**Decision: C.** Add it, named **"Record"**, not "Logger": "Logger" / "DATA LOGGING" is the
FR-38 fast stream (src/logger.rs, `LOG:STATe`) and the two must not be confused on screen or
in SCPI. Scope and defaults:

| item | decision |
|---|---|
| where | one row "Record" on the DMM **Hold & Limits** page (View::Limits, reached from the MF range page's Hold/Limits button and directly where the function has no range page), opening a settings-style page (rows with -/+ steppers like Settings 1, a Start / Stop button, the stock settings look). Not a new MF menu: the DMM MF key already has a home for DMM-bound tools |
| interval | Every reading (default) / 1 s / 10 s / 1 min. Every reading = exactly today's behaviour, so the swipe-down REC changes nothing for existing users |
| per-interval value | **the mean** of the unsmoothed readings (`meter::Update::raw`, REL applied, as today), one row per interval, time = k x interval from the start (regular by construction). Min / max are **not** written as extra columns: three values per row would change the DLG row, the LUN-1 CSV generator, `MMEM:DATA?` and tools/dlog.py for a feature nobody has asked for yet. Who needs the spread records "every reading" (13 MB holds 325 h at 10/s in DLG) |
| flags in an interval row | OL if any reading in the interval was OL (the value is the mean of the non-OL readings; empty when there was none); HOLD / REL / HI / LO ORed over the interval; GAP as today |
| duration | Until stopped (default) / 10 min / 1 h / 8 h / 24 h; at expiry the recording stops with the normal "saved" toast. Function change still ends it |
| estimate | "Free: ~N h at this interval" on page entry: free clusters (the csvlog.rs free-space scan: a few block reads, synchronous, page entry only, never while a commit runs) x a conservative 2 B/row (measured 1.1..1.9), divided by the row rate; capped by the 49.7-day u32-ms limit of one recording |
| Start / Stop | the same csvlog session as the swipe (`csvlog::start` / `stop`), red REC badge, toasts as today; Start from the page returns to the DMM page |
| SCPI | `LOG:CSV:INTerval <s>` / `?` (0 = every reading), `LOG:CSV:DURation <s>` / `?` (0 = until stopped), `LOG:CSV:FREE?` (bytes); `LOG:CSV ON|OFF` unchanged |
| persistence | interval and duration persisted with the DMM settings (two small fields in the persist record; 0 = defaults, so an old record loads unchanged) |

**Why.** The community asks for 24 h autonomous logging (UF: "car battery drain", FR-04 /
FR-38 notes); what blocks it today is not disk space (DLG) but a 864 000-row CSV that no
spreadsheet opens. A per-interval mean solves that with the file format, the FAT writer, the
export path and the power-cut analysis untouched. Mean, not min/max: a mean per interval is
what every bench logger offers first and it keeps the one-value row; the spread is still in
"every reading" mode. Not a separate MF menu entry: the stock MF key opens the function's
settings page, and Hold & Limits is already fw-rs's home for DMM-bound extras (AutoHold,
limits), so one more row keeps the stock navigation model. Small page, stock look, no new
widgets.

**Consequences / implement.**
- csvlog.rs (or a thin aggregator in app.rs `reading()`): accumulate sum / count / flags per
  interval, push one row at each boundary; "every reading" bypasses it.
- A long recording holds off the idle power-off (app.rs `csvlog::active()` already counts as
  activity); the backlight dim still applies, which a 24 h battery log needs. Say so in Help.
- ui.rs: the row on Hold & Limits (LIMIT_ROWS has 8 rows today: if a ninth does not fit at the
  settings row pitch, page Hold & Limits as Settings 1/2/3 does, or put Record first), the
  Record page (ui-render overlap check + contact sheet before showing it), the estimate line.
- Emulator / host tests: dlog-test unchanged (format unchanged); a csvlog test for the
  interval aggregation (boundaries, OL mix, GAP); a USB/IP run `LOG:CSV:INT 1` 60 s -> 60 rows.
- Device verify: 1 min interval for 10 min, the file opens, the estimate is within 2x.

**Implemented (2026-10-22).** As above, with these points fixed in the code (csvlog.rs header):
- time_s of an interval row = the interval's **end**, k x I (the value is known only then;
  "when the firmware saw it"); the header says so in a third `#` line (interval mode only).
- An interval with an OL reading is an **OL row without a value**: the DLG coder / CSV
  generator print no value on an OL row (dlog.rs `fmt_row`, the OL row coding), so "OL + the
  mean of the non-OL readings" cannot be stored without the format change this decision
  rules out. The mean of a partly overloaded interval would also understate it.
- An interval without readings writes no row (a 2.2 s capacitance reading at 1 s; an empty
  value would read as OL); the partial last interval at a stop is written at its nominal end.
- The settings apply to the next recording; changing them while one runs is refused (page:
  beep + toast; SCPI: "Settings conflict"). Persisted as persist v8 word W_REC.
- The idle auto power-off did NOT count a recording as activity (the premise above was
  wrong: `csvlog::active()` only kept the DMM rail on); it is now held off while a
  recording runs (app.rs tick / off_warn), the backlight dim still applies.
- The estimate reads "about N h left" (the `~` glyph of TEXT22 touches the row box),
  "(limit)" at the 49.7-day cap, "-- (disk busy)" / "-- (no disk)".
Device checklist: Hold & Limits shows the ninth row; Record page steppers / Start returns to
the DMM page with REC; 1 min interval for 10 min -> 10 rows, the CSV opens, the estimate within
2x; 10 min duration stops itself with "Saved LOGnnn.DLG"; swipe REC uses the chosen interval;
settings survive a reboot; `LOG:CSV:INT 1`, `LOG:CSV ON`, 60 s, `LOG:CSV OFF` -> ~60 rows.

**Revisit.** If users ask for min / max per interval: add them as a new DLG chunk kind with
three value columns (mean, min, max), bump tools/dlog.py and the CSV generator together; the
UI then gets a "Mean / Min-Max / All" option. Not before.

---

## #2 DMM log timestamps: converter grid vs real main-loop time

**Question.** Snap each row's time to the HY3131 conversion grid (regular CSV time column, DLG
1.3..1.5x smaller) or keep the main-loop time of the pass that handled the reading (jitter
+-3 ms, measured in the emulator, unmeasured on the device)?

**Options.**
- A. Keep the real time (today).
- B. Snap to the grid with re-sync + GAP on misses and rate changes.
- C. Snap only when the loop time is within a tolerance of the predicted grid point.

**Decision: A.** Keep the real main-loop time for the DMM record (csvlog.rs rows).

**Why.**
- There is no single grid to snap to: MEDIUM switches 10/s <-> 2.5/s on its own (moving /
  steady), autorange and function changes restart the converter, the FR-05 rate can change
  mid-recording. B needs a re-sync rule for each case and a GAP flag whenever it fails: new
  failure modes in the one path that must never lose data, for a saving of 1.3..1.5x on a
  format that is already 11..26x smaller than CSV (13 MB = 325 h at 10/s; a 24 h log at
  10/s is ~1 MB).
- The real jitter on the device is unmeasured (log-format-standards.md 3, 7 E: "measure
  first"). Deciding a format-visible change on emulator jitter is premature.
- The time column means the same thing everywhere: csvlog's `time_s` and the FR-38 logger's
  microsecond stamp are both "when the firmware saw the reading" (logger.rs header). A snapped
  stamp would be a third convention, true to the chip but not to the file.
- Decision #1 makes the point moot for long logs: interval rows sit on t = k x interval by
  construction, so the regular column exists where people want it.

**Consequences.** Nothing to implement. Accept the +-few ms in "every reading" mode and its
1.3..1.5x DLG cost. FR-38 stamps untouched.

**Revisit.** After the device measurement planned in log-format-standards.md 7 (10 min of a
stable DC input, `tools/logcodec-lab` on the CSV): if the stamps are "grid + loop latency" with
a stable latency, reconsider B together with DLG2 (where the gain is 2x), still as a new chunk
version, never a silent change of the existing one.

---

## #3 One interrupt for gap-free AD2 capture vs staying ISR-free

**Question.** Introduce one ISR (timer or EXTI on the HY3131 data-ready) so the AD2 logger is
gap-free above ~2 kS/s and the "slow floating scope" on the DMM inputs becomes possible, or
keep the firmware ISR-free (every AD2 sample then competes with LCD redraws: 26 ms gaps per
100 ms with the DMM page repainting, feasibility doc 7 / device-check)?

**Options.**
- A. Stay ISR-free: AD2 logger best-effort with honest loss counters; the floating scope only as
  "capture then show" with the display frozen (600..1200 S/s).
- B. One ISR, HY3131 bus shared with the main loop through a request queue (feasibility 5.2 E3).
- C. One ISR with **exclusive** bus hand-off: while a stream runs the ISR owns the HY3131, the
  main loop only reads the ring; no queue.

**Decision: C, gated.** Allow exactly one interrupt, a **TIMER**, not EXTI, and only after the
SPI2 bus is proven on the device. The rule becomes: "no ISRs, except the HY3131 stream ISR,
which touches nothing but its ring".

Invariants (these are the decision, not advice):
1. Exactly one interrupt enabled in the NVIC, ever. `cpsid` stays everywhere else. The whole
   path is behind a Cargo feature; the default build without it is bit-identical in behaviour
   to today.
2. Source: a general-purpose timer not used today (src uses TIM1/2/3/5/8 for hwtrig, TIM6/TIM12
   for the generator, TIM13/TIM14 for backlight / buzzer; TIM7 appears only in a board.rs
   comment about the stock key init and is free; grep before choosing), at ~1.05 x the AD2 rate; the ISR reads INTF, and on AD2F the 3-byte
   result (one or two SPI2 frames, 15..22 us at 3.125 MHz). EXTI11 is rejected: PC11 is the
   HY3131 data-ready line **and** SPI2 MISO (AF6), so an edge interrupt on it would fire during
   every bus frame and needs masking around each transfer - exactly the shared-state subtlety
   this decision avoids. A timer also runs in Renode as-is.
3. Exclusive ownership: `stream_start` (main loop) writes the AD2 row, hands the bus to the
   ISR and from then on **nothing in the main loop touches the HY3131** (dmm.rs poll, the
   sentinel's CTSTA read, the power meter, BT `serve`, DBG:HYSPI); `stream_stop` disables the
   timer, waits for a running ISR to finish, takes the bus back and rewrites the DMM row. The
   precedent exists: an FR-38 session already owns the HY3131 and pauses BT (TODO, UF-36 note).
   In the Wave view the DMM readout therefore shows the last value and a "WAVE" cue (the stock
   scope page has no DMM either); simultaneous DMM + stream (the request queue) is not v1.
4. Shared state: one SPSC ring (ISR writes, main reads, index words atomic) plus loss / overrun
   counters. Nothing else. The ISR never kicks the watchdog, never allocates, never writes
   flash or LCD, never calls into dmm.rs.
5. CPU budget: default stream 4.8 kS/s (~10 % at 3.125 MHz), 9.6 kS/s opt-in, 19.2 kS/s never
   (feasibility 2.3). The static stack check (build.sh, tools/stack-depth) gains the ISR frame
   on the worst path; the 16 KiB headroom rule stays.
6. Gate: the SPI2 bring-up (feasibility 8.5: the ~25 % lost writes at the frame boundary) must
   be fixed and the DIAG knobs removed first. **If SPI2 cannot be made clean, the ISR does not
   go in with bit-bang** (122 us per sample in an ISR = 29 % CPU at 2.4 kS/s for a rate the
   AD1 logger nearly reaches without any of this): the floating scope is then parked as A
   ("capture then show") and the AD2 logger stays best-effort with its loss counters.

**Why.** The AD2 logger above ~2 kS/s and the floating scope both need gap-free capture and the
device numbers say the main loop cannot give it (26 ms holes per 100 ms when the DMM page
repaints; the link ceiling, not the chip, at 6.5 kS/s bit-banged). A waveform with a hole at
every redraw is worse than no waveform (feasibility 5.1). The safety value of "ISR-free" is
that no shared state exists; C keeps that value by making ownership exclusive in time instead
of guarded in code. A timer is the dumbest possible source. Gating on SPI2 stops the ISR from
being built on a bus whose frame timing is still being debugged (fca31fd DIAG image).

**Consequences / implement (order).** SPI2 fix + DIAG removal (E2) -> bus hand-off API in
hy3131.rs with a debug assertion that the main loop never touches the bus while handed off
(E3, reduced: no queue) -> timer ISR + ring + counters behind the feature (E4) -> `LOG:SOURce
AD2` uses the ring when the feature is on -> Wave view (E5/E6) only after the device
acceptance. Acceptance on the device: `LOG:COUNt?` lost = 0 at 4800 S/s for 60 s with
`DBG:AD2:DRAW 2` (full DMM repaint every 100 ms), `DBG:PROF?` loop max unchanged, `DBG:ISR?`
(count, max DWT cycles in the handler, overruns) added for that. Emulator: the same run over
USB/IP; host test for the ring.

**Status (2026-10-22).** Implemented behind the Cargo feature `ad2-isr` (off by default until
the device acceptance), docs/dmm-input-scope-feasibility.md section 9. Two deviations, both
measured: the tick runs at 4 x the AD2 rate with a 7/8-period read gate instead of ~1.05 x
(at 1.05 x the observation stamps jitter by ~P and the host saw 5 % false gaps; 9.2), and
the "debug assertion" is a runtime guard in every build that stops the stream before any
main-loop bus frame (a function change while logging legitimately reaches the bus; 9.1).

**Amendment (2026-10-22, coordinator-directed: the Wave view's trigger).** The Wave view
(feasibility 10, TODO (A); behind the same feature) runs its trigger detector in the ISR,
per sample (edge / pulse width / runt, integers only, a few compares: no dead time between
frames). Invariant 4's shared state grows by: the trigger configuration words (written by the
main loop, generation word last with Release; the ISR copies them on a new generation) and
an 8-slot event ring plus a "first event after index N" pair (written by the ISR, Release on
the count / generation). Still one writer per word, no lock, nothing else; the ISR still
writes no register and touches no other module. The HY3131 comparator was evaluated and not
used (fixed ladder thresholds, no timestamp, owned by the frequency mode / sentinel; 10.2).

**Revisit.** A second ISR, or sharing the bus while streaming, needs a new decision with its
own shared-state analysis; "it would be convenient" is not a reason. If the exclusive hand-off
turns out to hurt (users want the DMM digits live under the waveform), the request queue (B)
is the documented next step, not a second interrupt.

---

## #4 Power-meter energy counter across power-off

**Question.** Keep the Wh / VAh total (and resume integrating) across a power-off, or reset on
boot (today: `pw_enter` resets)?

**Options.**
- A. Reset on boot (today).
- B. Persist E / VAh / elapsed in the backup-SRAM settings record; on boot in the power meter
  show and continue from it.
- C. Persist, but show as a separate "Last run" value with a manual Resume.

**Decision: A.** The energy total is a per-session quantity; it resets on boot.

**Why.**
- No RTC: a resumed total hides an unknowable gap and presents a wrong number as a right one.
  A power meter that shows 3.2 Wh after the load ran unobserved for a night is worse than one
  that shows 0.
- persist.rs is a settings record: magic / version / CRC, and a failed check drops the whole
  record to defaults. Measurement data does not belong there, and whether the backup domain
  survives the DM40's power-off (power hold low, PE2 / PC4) is unverified on the device.
- The loss case is already narrow: the idle auto power-off is held while the energy integrates
  with I > 50 uA (powermeter.rs `integrating`), so the only way to lose a run is the user's own
  power-off or a flat battery, and a run longer than the battery needs USB power anyway.
- Cross-session energy has a correct tool: Record (decision #1) on the power function logs P
  per reading / per interval; the host integrates with real times from the file.

**Consequences.** Nothing to implement; note it in the power meter's Help text ("Wh counts from
entry or Reset Wh; a power-off clears it"). Optionally (cheap, no new state): the power-off
confirm dialog, when `integrating()`, adds one line "Energy total will be cleared".

**Revisit.** When a real-time clock or a dated log exists, or when users ask: then C (a "Last
run" value with its elapsed time, shown as history, never silently merged), stored in its own
CRC-protected words, not in the settings record.

---

## #5 Misconnect A-JACK badge vs HOLD / AutoHold in the status slot

**Question.** The DMM status slot (x 119..190, alert box to 201) carries RUN / HOLD / A-HOLD,
or, with no hold, A-JACK / V-JACK? / LIVE?. Today HOLD wins and hides the alert; the toast
and the beep (3 x 50 ms every 2 s for A-JACK) still fire. Should the alert outrank HOLD?

**Options.**
- A. HOLD wins (today).
- B. Alert wins, HOLD invisible while it is on.
- C. Alternate the two in the slot at the 1 s `alt` cadence the combined-view strip already
  uses for REL / limit verdict; both states visible, neither hidden.
- D. A second place for the alert (a red band, the range label area).

**Decision: C for A-JACK and LIVE?, A for V-JACK?.** Priority in the slot: A-JACK / LIVE?
alternating with HOLD / A-HOLD (1 s each) > HOLD / A-HOLD > V-JACK? > RUN. The combined-view
strip badge gets the same rule (today HOLD > alert > HV > REC > REL).

**Why.** A-JACK ("current leads in the A jack while measuring volts": the next probe on a live
circuit shorts it through the shunt and fuse) and LIVE? ("voltage on the circuit in ohm /
continuity / diode") are safety warnings; HOLD is a display convenience. A safety cue that a
frozen display hides half of its surface is wrong, but hiding HOLD is also wrong: a user who
cannot see HOLD does not know the digits are stale. The slot has no room for both (71 px,
uptime at 205) and option D invents a new place against the stock layout; alternating in the
same box is the pattern fw-rs already uses in the strip, costs no pixels, and the ongoing beep
carries the alert in the off phase. V-JACK? is a silent, question-marked hint and does not earn
a blink over HOLD.

**Consequences / implement.**
- ui.rs `hold_label` / `last_top`: the redraw key includes the alternation phase; each phase
  clears the full 119..201 box (the other states already clear 190..201); the strip badge
  priority in `strip_data` gets the same alternation.
- tools/ui-render: the alert x hold x alt x verdict x rate-chip matrix re-run, both phases, no
  overlap; contact sheet before showing the owner.
- sentinel.rs unchanged: it already evaluates on the live hardware while HOLD freezes the
  display; the toast per assertion and the beep pattern stay.
- Help / Settings 3/3 text: "alerts alternate with HOLD".

**Implemented (2026-10-22).** ui.rs `slot_alert` / `alert_alternates`; the phase is the app's
1 s `strip_alt`, toggled in every view; the DMM page repaints the slot each second only while it
alternates. While HOLD / AutoHold freeze the readout the slot and the strip draw the **live**
alert (meter.live), so a probe swap after HOLD shows. Help page 5 says "alerts alternate with
HOLD" (Settings 3/3 keeps its value text: no room in the row). Device check: HOLD in DC V, then
current through A -> HOLD / A-JACK alternate 1 s each, beep continues; V-JACK? under HOLD shows
HOLD only.

**Extended (2026-10-22, misconnect 4.7).** OVERLOAD (red, the protective
10 A switch) joins the alternating set (A-JACK / LIVE? / OVLD); the grey hints
DC? / AC? / FUSE? take V-JACK?'s place below HOLD (never alternating).

**Revisit.** If the device check of the sentinel re-check list (A-JACK false trips) shows the
alert is too noisy to blink over HOLD, go back to A for the slot and keep the beep, rather than
weakening the beep.

---

## Summary

1. Record page (not "Logger"): one row on Hold & Limits; interval every reading / 1 s / 10 s /
   1 min with the mean per row, until stopped or 10 min..24 h, free-time estimate; format
   unchanged.
2. Timestamps stay real main-loop time; no grid snap until measured on the device; interval
   rows are regular anyway.
3. One TIMER ISR for AD2 streaming with exclusive bus hand-off, behind a feature, only after
   SPI2 is clean; no ISR with bit-bang.
4. Energy resets on boot; cross-session energy = Record P and integrate on the host.
5. A-JACK / LIVE? alternate with HOLD / A-HOLD in the status slot (1 s); V-JACK? stays below
   HOLD.
