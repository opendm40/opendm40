# Decisions 2026-10-22 (nine open product / engineering questions)

Taken on the owner's behalf, as delegated. Sources: TODO.md, docs/compact-log-format.md,
docs/log-format-standards.md (section 3, 7), docs/dmm-input-scope-feasibility.md (2.3, 4, 5,
7, 8.5), docs/power-meter-and-misconnect.md (3.11, 3.12, 4.6), src/csvlog.rs, src/meter.rs,
src/logger.rs, src/powermeter.rs, src/persist.rs, src/ui.rs (`hold_label`, `strip_data`),
docs/user-feedback.md, ../docs/dm40-feature-request.md (FR-01, FR-38). Nothing under
firmware/re/history/ was read. No code was changed; this file and the TODO pointers only.

#6..#8 (later the same day, a second delegation): docs/native-boot.md ("Unit data", "Host
writes", "Host OS notes"), src/vwrite.rs (header, `frozen`, `set_read_only`), src/msc.rs
(UNIT ATTENTION / `DBG_EJECT`), src/usb.rs (the SE0 disconnect), the 2026-10-21 UNIT
ATTENTION experiment in TODO.md, docs/dmm-input-scope-feasibility.md 11 (11.3, 11.8, 11.9,
11.10), src/hy3131.rs (`stream::pick_div`, `max_rate_hz`, `set_dma_req`, the DMA0 flag
clears), src/bt.rs / src/gen.rs / src/scope.rs (DMA channels), src/sentinel.rs,
src/hyspi.rs. Same rules: nothing under firmware/re/history/, no code changed.

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

**"Leads in A?" reminder (2026-10-22, misconnect 4.9).** Not part of this
alternation: it is a one-shot toast + beep on leaving a current function, no
alert state, so the slot and the strip never show it and HOLD stays as it is.

**Revisit.** If the device check of the sentinel re-check list (A-JACK false trips) shows the
alert is too noisy to blink over HOLD, go back to A for the slot and keep the beep, rather than
weakening the beep.

---

## #6 USB volume mode: writable, read-only, or read-only with writes on demand

**Question.** The generated USB volume (vdisk.rs) is writable since f4df76f / 96237b8 so the
PC can delete LOGS / SHOTS / SCOPE files and drop a `dm40*.bin` for an update (install
confirmed on the meter). The price (native-boot.md "Host writes", Session): while a host has
it mounted the layout is **frozen**, no media-change UNIT ATTENTION is sent (a UA on a
rw-mounted removable disk makes Linux fail the I/O and later write back dirty buffers that
carry the old FAT / directories: our diff could read a stale FAT sector as "chain freed" of a
new file), so a recording / screenshot made meanwhile appears only at the eject / re-plug.
The read-only volume (`DBG:VDISK:RO 1`) sends the UA at once: the 2026-10-21 experiment
showed Linux listing new files on the mounted volume immediately, Nautilus after F5.

**Options.**
- A. Always writable (today).
- B. Always read-only: live files, no delete / update from the PC.
- C. Read-only by default, writable on demand (a Settings item or a temporary mode entered
  from the update / gallery flow).
- D. Something cleverer: stay writable but recognise stale write-backs after a UA by a layout
  generation and ignore them.

**Decision: C.** The volume is **read-only by default** (`vwrite` `ro` = true at boot, the WP
bit set, the UA at once also while mounted: exactly the `DBG:VDISK:RO 1` volume). A Settings
3/3 row **"USB disk"** with the values **"Files live"** (default) / **"PC can edit"** switches
to the writable volume of today (deletes, `dm40*.bin` + the install dialog, the frozen
layout). Rules:

| item | decision |
|---|---|
| state | **volatile**: not persisted, "Files live" again at every boot. An update reboots, so the update case clears itself; a delete session ends when the meter is switched off or the row is set back |
| when it applies | **at the next USB plug**: a host reads the WP bit when it mounts (MODE SENSE), so a change while a host has the volume cannot be seen by it. Setting the row shows the toast "Re-plug USB to apply" and the row's value carries the pending state. No forced re-enumeration in v1 (usb.rs's SE0 disconnect exists, but it also drops the USBTMC interface and any host SCPI / LOG session; a follow-up may use it when no host session is active) |
| `DBG:VDISK:RO` | the same flag (`vwrite::set_read_only`), `RO?` reports it; nothing else |
| README.TXT | says: files appear while mounted (refresh the folder); to delete files or copy a firmware file from the PC set Settings > USB disk > PC can edit and re-plug; `tools/reload.sh` needs neither |
| Help | one line on the Settings page of the help |
| the Device page / gallery | unchanged; no second entry point (one place to look for the switch) |

**Why.**
- The composite USB device is always present: a meter on the bench is plugged in for
  charging and SCPI, GNOME / Windows automount the volume, and with (A) every recording and
  screenshot of the session is invisible until the cable is pulled. That is the frequent
  case. A delete from the PC or a drag-and-drop update is the rare case, and both have
  other paths that keep working in read-only mode: the gallery / `MMEM:DEL` / dm40scpi.py
  for deletes, `tools/reload.sh` (`DBG:UPLOAD`) and the loader's own disk for updates.
- (B) throws away a working, host-tested feature (vwrite.rs, the install dialog) that is the
  only update path for an owner without Python.
- (D) is the cleverness the file's principles rule out: it adds a new way for the one path
  that must never lose data (a stale FAT sector misread as a delete) to go wrong, for the
  benefit of not pressing one Settings row.
- Volatile, not persisted: a persisted "PC can edit" is a mode the user forgets, and its
  symptom ("my new files do not show up") is exactly what this decision removes; the row
  shows the state while it lasts. No persist record change.
- "At the next plug" rather than a forced re-enumeration: no new USB state machine, the
  behaviour is the one already captured and host-tested for both volumes; the re-plug is
  the same gesture the writable volume already needs to show new files.

**Consequences / implement.**
- vwrite.rs: `ro` default true (one line); the frozen / UA rules need no change (`frozen()`
  is already false while `ro`).
- ui.rs / app.rs: the row on Settings 3/3 (2 rows today: room), value text "Files live" /
  "PC can edit"; the toast; Help text; README.TXT text (the fatren-test captures pad it to
  2048 bytes: re-check the padding, and a changed README needs a new `kcapture.sh` capture
  for the vwrite scenarios, which run in "PC can edit").
- tools/fatren-test: the vwrite scenarios set RO 0 first; add a read-only scenario (a WRITE
  fails 07/27/00, the UA is sent while locked).
- Device checklist (the two open items in TODO under the USB storage redesign, re-ordered):
  default "Files live" on Linux: mount, swipe REC / screenshot, the file is listed at once
  (`ls`), Nautilus after F5; Windows Explorer and macOS Finder with the UA while mounted
  (open since 2026-10-21; if Explorer does not refresh, try a volume serial that changes per
  refresh, TODO note). Then "PC can edit" + re-plug: the 2026-10-22 vwrite checklist (rm /
  trash / cp dm40fw.bin Yes + No / `DBG:VDISK:WR?` / fsck of SPIFLASH.BIN / a recording during
  a cp). Reboot -> `DBG:VDISK:RO?` = 1.

**Revisit.** After the Windows / macOS device checks. If Windows never sends PREVENT (the
"Host OS notes" expectation), the writable volume's cost there is only a 60 s quiet window
after its last write, and a persisted "PC can edit" for Windows-only users becomes
reasonable: then persist the row (a spare bit of an existing persist word if one is free,
else a version bump) and keep "Files live" the factory default. If users ask for live files
*and* deletes at once, the forced re-enumeration (usb.rs SE0, refused during a host session)
is the next step, not (D).

**Implemented (2026-10-22).** As above, with these points fixed in the code (vwrite.rs header):
- The mode has two values: **in force** (`read_only`, MODE SENSE WP, WRITE refused) and **asked
  for** (`read_only_pending`, the Settings row / `DBG:VDISK:RO`). The one asked for is copied at
  the USB bus reset (`vwrite::on_bus_reset`, from `vdisk::on_bus_reset` after the session's
  `release_all`), so the mode never changes under a host, in either direction: asking for "Files
  live" during a writable session keeps it frozen and writable until the re-plug (no host write is
  cut off by a sudden WP), then its records are dropped as at any session end (fatren-test vwrite
  "rw-ro"). The state lives in .sram_bss (zeroed at start-up), stored as "writable" so that zero =
  read-only at every boot.
- The row shows the mode asked for, with " (re-plug)" while it differs from the one in force
  ("PC can edit (re-plug)" fits the value column); the toast "Re-plug USB to apply" comes only
  when it differs (setting the row back before a re-plug cancels silently). The page repaints
  when a bus reset applies the mode while it is shown.
- `DBG:VDISK:RO?` answers `<in force>,<asked for>` (`1,1` after a boot).
- Help: there is no "Settings" help page; a sixth page "USB disk" (8 rows: the two modes, refresh,
  where, when it applies, power-on, delete on the meter) instead of one line.
- README.TXT rewritten (both modes, "In any mode": gallery / MMEM:DEL, tools/reload.sh), still 2048
  bytes: the kernel captures replay unchanged (they never write README.TXT), no new kcapture.sh run;
  `usbip_msc.py --ro` serves the read-only volume for a future capture.
- Without the `vdisk` feature (the writable SPI disk) the row shows "PC can edit" and does nothing.

---

## #7 AD2 stream frames: CPU frames at /8 + CKPH or DMA frames at /16 as the default

**Question.** Both frame kinds stream with 0 loss on the device (a0cdb4e, feasibility 11.10):
CPU frames at /8 + CKPH = 1 (the default, `DBG:AD2:DMA 0`) cost 26 % CPU at 19.2 kS/s and
51 % at 38.4 kS/s; DMA frames at /16 (opt-in, `DBG:AD2:DMA 1`) cost 8 % and 17.5 %. Make DMA
the default? Robustness (the frame-shift bug fixed in a0cdb4e, bounded waits), the Wave
view, DMA sharing with BT / generator / scope.

**Options.**
- A. Keep CPU frames (/8 + CKPH) as the default; DMA opt-in.
- B. DMA frames (/16) as the default; CPU frames as the fallback (`DBG:AD2:DMA 0`).
- C. DMA by rate: CPU frames up to 9.6 kS/s, DMA above.

**Decision: B, gated on one more device run with the default flipped.** `set_dma_req`
default 1; `DBG:AD2:DMA 0` and `DBG:AD2:DIV` stay as the fallback and the probe. The rate cap
is unchanged (38 400 S/s either way: `max_rate_hz`). Not (C): two defaults are two
behaviours to verify and explain; the Wave view's bands (9600 / 4800 / 2400 / 1200 S/s) all
sit below the point where the kinds differ in CPU, and above it DMA wins outright.

**Why.**
- CPU: 26 -> 8 % at 19.2 kS/s and 51 -> 17.5 % at 38.4 kS/s. The CPU frames' 51 % leaves the
  main loop (repaint, USB, the Wave view's drain + render) half the machine at the top rung;
  the DMA frames leave it 80 %. The Wave view runs the stream *and* the scope renderer in
  the same pass budget, so it gains the most.
- max_isr: 10.8 us (DMA) against 16.8 us (CPU /8 + CKPH): the ISR holds the bus and the CPU
  for less at every entry.
- Correctness is equal on the device: a0cdb4e, 60 s each, lost 0 / bad 0 / timeouts 0 for
  DMA at 19200 (DRAW 2) and 38400, the same for CPU frames at 4800 (DRAW 2) / 9600 / 38400.
  The DMA path's one bug (an aborted frame leaving a byte in flight, f5f8e96) is fixed with a
  bounded wait + a drained receive register, and a shifted frame would be caught by the
  sign-copy check (`bad`) rather than pass silently.
- DMA sessions take samples only once locked, so they skip the one or two R26-transient
  samples the CPU-frame sessions deliver first (11.10): a cleaner first screen for the Wave
  view and a cleaner first row for the logger.
- Sharing: DMA0 channel 0 (SPI2 RX) and 7 (SPI2 TX) are used by nothing else; bt.rs holds
  channels 2 / 4 (UART4), gen.rs 5 (DAC), the scope is on DMA1. The flag clears are
  per-channel in every module (hy3131.rs INTC0 bits 0..5 / INTC1 bits 22..27, bt.rs `0x3D
  << 16` / `0x3D`, gen.rs HIFCR `0x3D << 6`), so no module clears another's flags. One
  controller is shared, so bus arbitration is the only interaction; a 5-byte SPI frame at
  3.125 MHz is 13 us of 1-byte-per-2.5-us requests against the DAC's up-to-10 MS/s circular
  table: that is the untested part, hence the gate below.
- Decision #3's invariants hold: DMA frames add no NVIC line (the ISR polls the channel
  count / flags at the next entry), the ISR still touches only its ring and counters, and
  the exclusive bus hand-off is unchanged.
- Against: the DMA path has one passing device run (a0cdb4e) where the CPU frames have
  three (4b98128 /16, b81abf2 incl. a 15-min 35.8 M-sample soak, a0cdb4e); and every device
  run so far streamed with the generator off and BT idle. That is a reason for the gate, not
  for keeping a default that costs 3x the CPU.

**Consequences / implement.**
- hy3131.rs `set_dma_req` default 1 (the "one line" of 11.10); `pick_div` then selects /16
  mode 0 for the default; docs (feasibility 11.3 "Defaults", 11.8 "Selection", bt-scpi.md
  `DBG:AD2:DMA`) and `DBG:ISR?` div = 16 by default.
- Emulator: ad2streamtest with the default (no `:dma` suffix needed) at -100 / +100 ppm, 4800
  DRAW 2 / 9600 / 19200 / 38400; wavetest.py PASS (the Wave view now streams DMA frames).
- **Acceptance on the device (the gate; CPU frames stay the default until it passes):**
  1. 60 s each, raw `LOG:DATA?` drain: 4800 DRAW 2, 9600, 19200 DRAW 2, 38400: lost 0, bad 0,
     late / slips / overruns / timeouts 0, CPU within 2 % of 11.10.
  2. **Sharing matrix**, 38400 DMA for >= 15 min (the soak the CPU frames have): generator ON
     in DAC mode at its top rate (a sine on the MCX output, checked on the scope page before
     and after) **and** BT connected to a phone (idle: BT serve is paused while streaming,
     but the UART4 RX DMA keeps running); lost 0, bad 0, timeouts 0, the generator's output
     unchanged, BT reconnects after the stream.
  3. Wave view: every band 5 ms .. 1 s/div on the 50 Hz 1 V rms sine (feasibility 10's
     table): FREQ / VRMS / VPP as before, `DBG:ISR?` bad 0, no "Wave stopped"; note the time
     from a band change to the first drawn sample (DMA waits for the lock) and compare with
     CPU frames (`DBG:AD2:DMA 0`); it must stay under one screen at the band's rate.
  4. After the streams: DC V on the 1 V calibrator within 0.05 %, `DBG:HYSPI:SELF? 2000`
     all counts 0 (the DMM's /16 mode 0 restored by `stop`).

**Gate result (a239aa6, feasibility 11.11).** Items 1, 3 and 4 pass. Item 2 passes for the
generator DAC: 900 s at 38400 with a 10 kHz sine at 10 MS/s, lost 0 of 34.56 M, 17.6 % CPU. The
BT part was not run because the host could not reach the meter's SPP address (TODO). The default
stays flipped.

**Revisit.** If the gate fails on the sharing matrix (bad > 0 only with the generator or BT
active), the fix is DMA channel priorities (SPI2 RX above the DAC) or keeping CPU frames
while the generator runs, decided then on the numbers; do not revert to (A) silently. 76.8
kS/s (a DMA data phase after a CPU command byte + CKPH switch, 11.8) is a separate step and
needs its own acceptance; it does not change this default.

---

## #8 DMM reads at /8 + CKPH = 1 (2.6x faster bus) or /16 mode 0

**Question.** The stream reads at /8 + CKPH = 1 are clean on the device (11.3). Move all DMM
traffic (dmm.rs polls, the sentinel's CTSTA read, the power meter's phase rows, BT `serve`,
row writes) to /8 + CKPH too? Benefits: shorter main-loop passes, tighter sentinel / power
meter phase timing. Cost: a full per-function device check.

**Options.**
- A. Keep the DMM at /16 mode 0 (today; `stream::stop` restores it).
- B. /8 + CKPH for every DMM frame, after a per-function device check.
- C. /8 + CKPH for reads only, writes stay /16.

**Decision: A.** The DMM bus stays at /16 mode 0. Not now, and not for the reasons listed
("loop time", "phase timing"); only decision #3's revisit (a request queue: DMM frames
between ISR frames) can make a faster DMM bus necessary, and that is when this is re-opened.

**Why.**
- The gain is microseconds the loop cannot see. Per pass the DMM reads one byte (INTF or
  CTSTA: ~8 us at /16, ~6 us at /8 + CKPH) and, only when a conversion completed, one frame
  of 6..20 bytes (~20 / ~60 us at /16 against ~14 / ~23 us: SELF? us_r6 13.9 vs 21.6). Row
  writes are ~11 us per register at /16 and happen at a function / range / phase change.
  The main loop's passes are 2..25 ms (repaints, the scope page); the sentinel windows and
  the power meter's phases are 350 ms. Nothing user-visible changes by 40 us.
- During a stream the DMM never touches the bus (exclusive hand-off, decision #3), so a
  faster DMM frame buys nothing for the Wave view or the logger either.
- The DMM path has **no integrity check**: the stream's sign-copy test catches a torn
  AD2 read, but a 24-bit AD1 result, a counter frame or CTSTA read wrong at /8 becomes a
  wrong reading, a false A-JACK or a missed one, silently. The /8 margin was measured on
  one unit at room temperature (11.3: ~80 ns of DO-path delay against a half period of 80
  ns in mode 0; CKPH = 1 gives a full period). CKPH = 1 was proven for 1-, 3- and 6-byte
  frames in the stream; the DMM's 20-byte AC poll and the 23 / 28-byte counter frames (FREQ,
  the frequency sentinel) have not run at /8 + CKPH at all.
- Region B (zero-wait flash) has 28 bytes left (TODO). The CKPH switch is per-frame hot code
  (SPE off / CKPH / SPE on between the read-period pulse and the data) in `hy3131::read`,
  which runs in every pass; it cannot go to `.cold`.
- Writes at /8: `WR? 1` gave 0 / 0 at /8 on b81abf2, but the matched-dummy trick exists
  because the chip takes each LSB late; halving the bit period halves that margin too, on
  the one register path (rows, ranges, calibration-dependent mux bits) where a bad bit is a
  wrong range or a wrong relay. (C) avoids that but keeps every other cost.

**Consequences.** Nothing to implement. `stream::stop` keeps restoring /16 mode 0; `DBG:AD2:DIV`
and `DBG:HYSPI:SELF? n,8,1` remain the probes. feasibility 11.3's "moving it to /8 + CKPH is a
separate step (TODO)" becomes "decided against for now (decision #8)".

**Acceptance check, defined now for the revisit** (all on the device, the calibrator and the
generator as in the device-check docs; a /8 + CKPH build with `DBG:HYSPI:SELF?` reading the
same way the DMM does):
1. `SELF? 2000,8,1` in the row of **every** function (DC V, DC mV, AC V, AC mV, AC+DC, DC A /
   mA / uA, AC A, ohm both modes, continuity, diode, capacitance, frequency, temperature TC
   / RTD / NTC, power meter): every count 0, 3 sessions each, including a cold start (meter
   at 0..5 C) and after 1 h on (the DO-path delay drifts with temperature).
2. A long-frame test added to SELF? (read(R00, 20) and the 23 / 28-byte counter frame) with
   a stable reference: 2000 x 0 bad at /8 + CKPH.
3. `WR? 1` at /8: 0 / 0 in 10 runs, then the row writes themselves: every function's row
   read back after the write equals the row table, 100 function changes.
4. Readings vs /16 on the calibrator per function: equal within the last digit (DC V 1 V,
   AC V 1 V 50 Hz, 1 kohm, 10 mA, FREQ 1 kHz, TC type K at room temperature, power meter
   with the 40 Hz square); a 10-min DC V stability run at 1 V: no outlier beyond 2 counts.
5. Sentinel: 1 h in DC V with the A jack shorted and open (no false A-JACK, `DBG:SENT?` hit
   counts as at /16), the LIVE? and OVERLOAD cases of the sentinel checklist once each.
6. Then a 15-min stream (decision #7's matrix item 2) with the DMM switched back and forth
   10 times (`stop` -> `start`): the DMM reads right after each stop.

**Revisit.** When decision #3's request queue (DMM live under the waveform) is on the table,
the DMM frames must fit between ISR frames and the bus speed becomes a real constraint;
then run the acceptance above and decide (B) or (C) on its results. Also if a second unit or
a temperature run shows the /16 mode 0 margin itself to be thin: then the question is
CKPH = 1 at /16, not /8.

---

## #9 76.8 kS/s: frame, probes, limits, where it is offered (implemented, emulator)

**Question.** TODO "76.8 kS/s": /8 + CKPH works for CPU frames, but the 13.02 us period has
no room for a CPU INTF probe (5.8 us at /8) next to a data frame. How to read the chip's top
AD2 rate with 0 lost, what it costs, and where to offer it (LOG:SRATe, the fast logger, the
Wave view) given the SPI flash's write rate. Taken by the implementing agent on the owner's
delegation (the task listed the planned approach); the details below are its choices.

**Decisions.**
1. **Hybrid data frame**: CPU command byte in mode 0 at /8 (waited for, 1.28 us), the
   read-period pulse, CKPH = 1, then DMA for the 3 data bytes. Not: plain /8 DMA shift frames
   (mode 0 data, 61 / 100 sign errors per ~2 M on 4b98128), a CPU frame (no room for the
   probe), a second timer interrupt to avoid the command-byte wait (saves ~0.5 us per sample
   for a second entry type and a tighter schedule: not worth it at ~43 %).
2. **Probe frames at /8 mode 0** (2-byte DMA, shift framing), every 16th period: a marginal
   DO bit reads AD2F as 0, i.e. one wrong-sign probe, which a bang-bang loop with a 20 ns
   phase step averages out (emulator: 50x the default error rate, still locked and lost 0).
   Not CKPH probes (a CPU wait per probe and a longer frame in the tightest window).
3. **Geometry** `Pll::hybrid`: beta = (P + c_off - delta) / 2 from device estimates (A frame
   7.3 us, probe latch 3.2 us); a late C drops its probe (new `c_late`), a late A still reads
   up to 2.3 us. The /16 sessions keep their geometry and behaviour exactly (c_late = MAX).
4. **Emulator honesty first**: SPI bytes timed on both SPI buses, DMA in time, the chip's
   state per byte in R03 frames (TornAd2), the /8 mode-0 DO margin, CKPH 1, CPU-synced virtual
   time, ns timer interrupts; the old firmware re-verified under it before any change. The
   emulator's ISR still runs ~0.57x the device's CPU (no wait states): CPU figures for the
   device are estimated from the device's /16 numbers, not taken from the emulator.
5. **Offered**: LOG:SRATe 76800 (DMA frames only; CPU frames / a forced /16 or /32 divider cap
   the rung, LOG:PREamble? says which); Wave view 1 ms/div = 76800 and 2 ms/div = 38400 (5
   ms/div unchanged), closing with "Wave: rate needs DMA frames" if the rung got capped;
   the **fast logger only after a page-program probe**: a disk session above 38.4 kS/s
   programs and times 4 pages (zeros, the allocation's last cluster, erased again) at the end
   of Prep; mean <= 800 us and max <= 1.5 ms -> 76800, else 38400. Measured (emulator): a
   typical chip (tPP 0.4 ms) keeps up to 196 KB/s with a FIFO high water of 460 of 8192 B; a
   worst-case one (3 ms) loses 28-54 %. Not "always" (silent-ish loss on a slow chip: only
   lost_before rows) and not "never" (a typical part has 2-4x headroom).
6. **Limits accepted, not fixed**: the 2048-sample ring is 26.7 ms at 76.8 kS/s; a full DMM
   repaint (DBG:AD2:DRAW 2) overflows it (7.8-9.3 % overwritten, lost 0); SRAM (10.9 KB free,
   8 KB floor) cannot double it. A USB host drain needs 921 KB/s (12 B per record): above what
   the device's Python drain did (~37 kS/s); `overwritten` shows it.
7. The logger default stays chopped (ENCHOPAD2) at every rung, 76800 included; the chopper at
   OSR 32 is a device-check item, not assumed.

**Consequences.** hy3131.rs (`hybrid_kick`, `choose_div(dma, hz)`, ladder), logger.rs (`isr`
hybrid path, the two-half RXB, the reorder, push without a 64-bit division), ad2pll.rs
(`hybrid`, `lead`, `c_late`, host model), fastlog.rs (Probe phase, `tpp_us`), main.rs (the gate,
the wave guard, LOG:DISK? fields), scope.rs (bands). CPU estimate ~43 % (40..46) at 76.8 kS/s.

**Revisit.** After the device check (TODO item): the CPU share, the chip's measured tPP (if most
units probe fast, the gate stays; if the probe itself misleads, i.e. fast probe and lost_before
rows anyway, lower the thresholds), the chopper at OSR 32, the host drain rate.

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
6. USB volume read-only by default ("Files live": new files appear while mounted, UA at
   once); Settings 3/3 "USB disk" = "PC can edit" switches to the writable volume (deletes,
   drag-and-drop update), volatile, applied at the next USB plug.
7. DMA frames (/16) become the AD2 stream default, gated on one device run with the default
   flipped including a sharing matrix (generator DAC + BT during a 15-min 38.4 kS/s stream)
   and the Wave view bands; CPU frames stay the fallback.
8. The DMM bus stays at /16 mode 0: the gain is microseconds, the DMM path has no integrity
   check, region B is full; acceptance check defined for the revisit (decision #3's request
   queue).
9. 76.8 kS/s with hybrid frames (CPU command + CKPH, DMA data; /8 mode-0 DMA probes), ~43 %
   CPU estimated; LOG:SRATe and the Wave view (1 ms/div) offer it, the fast logger only after a
   page-program probe of the chip (else 38400).
