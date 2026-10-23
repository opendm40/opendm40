# DMM switch latency: fw-rs vs stock (2026-10-23)

Owner: "Why is there a several-second stall on every mode switch now? The
stock seemed to switch instantly." Analysis of rust-fw-poc 69cacb9, the
emulator measurement `emu/hy3131/switchlat.py`, and the coordinator's device
check on ad74e93 (rate FAST, 1 V / 5 V). No firmware was changed for this.

## 1. Short answer

There are three different effects. They add up to what the owner sees:

1. **The readout is blank until the first reading** (`App::func_changed` ->
   `Meter::reset`: `shown = None`, `draw_static` draws no value or unit). The
   stock clears its values and redraws within 50 ms. The new unit is shown at
   once, with `0.0000` or `0L` (emulator screenshots: Ω tapped, `0L MΩ`
   0.05 s later, `0L kΩ` at 1.0 s, `4.7000 kΩ` at 2.0 s). On fw-rs the old
   screen stays for one frame, then it is empty until the number comes (device:
   blank until 2.0-2.3 s, number at 3.0 s for Ω). Even when the first number
   comes only ~0.15 s later than on the stock, fw-rs looks stalled and the
   stock looks instant.
2. **Ω: a 2.5 s dead time that the stock does not have.** fw-rs enters Ω
   with `set(10, 6, 2500)`: a 2500 ms settle deadline, so there is no
   conversion and no range step for 2.5 s. The stock's 2500 at Ω entry is
   **not** a deadline. It is `S+0x70 = now + 2500`, the end of its
   one-range-at-a-time window (app_decompile.c:16135, case 10). S+0x40 only
   gets +300 when the ohm variant changes. The stock steps down from 60 MΩ one
   range per ~150 ms from the start (emulator: the deadline moves in 150 ms
   steps from +0.4 s, r6 -> r2 by 0.9 s) and shows 4.7 kΩ at **1.15 s**.
   fw-rs takes **3.1 s** (emulator) / 2.67 s (device FAST). In fw-rs the
   `entry + 2500` single-step rule in `poll_ohm` is dead code: the deadline
   ends at the same moment.
3. **A ~0.5 s floor on every function change** (the stock: ~0.35 s). fw-rs
   `press` / `select` give each entry a settle: V and AC V 450 ms, A 650 ms.
   The stock uses these numbers only for its own row writes inside a function
   (autorange unit changes: DCV -> DCmV 450, ACA unit 650; AC+DC phases). Its
   button entry writes no deadline (emulator: S+0x40 does not change at
   DC V -> A or A -> DC V). The DMM screen polls once at the change and next
   300 ms later (`FUN_0803cdd8`). So the stock's first value comes at
   **0.35-0.40 s** (300 ms plus one AD1 conversion at OSR 8192). fw-rs:
   450 / 650 ms, plus one more conversion when the vet holds the first one
   (any change of the DC mode or range: Ω -> V, A -> V, V -> A): DC V 0.47-0.54
   s, A 0.68-0.74 s (device: 0.46-0.56 s and 0.75 s).

AC+DC / AUTO coupling are the only places where real multi-second gates
added on 2026-10-23 apply: the AC-part settle, the AC+DC step-down, and the
AUTO coupling's first decision. They act only after a range change or an
input step in AC+DC / AUTO, not on a plain function switch.

## 2. Every gate between a switch and the first displayed reading

| # | gate (where) | applies to | waits for | worst case | why (origin) |
|---|---|---|---|---|---|
| G1 | settle deadline `set(.., settle)` (`press`, `select`, `set_sub`, `set_range`, autorange) | every row write | time | entry: V / AC V 450, A 650, **Ω 2500**, cap 200, diode 100, cont 200, Hz 1000, temp 500; manual V range 450, manual A 650, Ω range 150; autorange DC 250 (V<->mV 350/450), AC 450, ACA 550/650 | Copied from the stock's S+0x40 table (dmm_v2_6.h "SETTLE DEADLINES"). The table mixes the in-function row writes with entry times: the stock's button entry has **no** deadline (only the 300 ms screen poll gap), Ω entry has none (2500 = S+0x70 single-step window), a manual range change is 250 (S+7 != m+1 path, 0xfa) |
| G2 | `vet_ad1` (VET_N 2, AC+DC VET_N_ACDC 8, VET_HELD_MAX 4, VET_DIV 0.5 %, VET_FLOOR 4000) | DC V / mV / A and the AC+DC DC phase, after any change of (DC mode, range): range steps **and** function changes such as Ω -> V, A -> V | the first conversion is always held; then one that agrees with the one before | +1 conversion (100 ms at OSR 8192) normally; a moving input: up to 5 conversions (~0.5 s); AC+DC: 8 vetted results across two DC phases | f4925f5 (2026-10-23). Device bab5dd4: after a 2-step autorange (600 -> 60 -> 6 V, the second row at the same R22) AD1's filter still held the 60 V tap: 0.767-0.790 V for 1 V in 5 of 35 (the "0.78 V" bug); an R22 write in the middle of a conversion gave 25-51 % (8087c7d); the AC+DC DC part at 53 % ("0.52 V", root cause fixed separately in c8c2f02, ACDC_STOCK_OSR) |
| G3 | readout blank (`Meter::reset`, `shown = None`) | every function / coupling change | the first reading | = G1 + G2 + the first conversion | fw-rs design: no value from the old function under the new unit. The stock shows its cleared values (0 / 0L) with the new unit at once |
| G4 | AC+DC first reading needs both parts (`acdc_seen == 3`) | AC+DC entry | DC phase (450 + 2 vetted DC results), switch to AC (800), first RMS result | 1.3 s (emulator and device 1.32-1.37 s) | fw-rs: never show \|DC\| or \|AC\| alone (the stock shows hypot(0, DC) first). The stock emulator showed 1.1179 V (= hypot(1.0, 0.5), correct) at 0.15 s: its AC+DC went to the DC phase at 0.1 s and back to AC at 0.2 s, apparently on a result already pending from the AC row and the leftover phase state (S+10); not analysed further |
| G5 | AC-part settle `ac_settle_start` / `ac_settle_step` (ACS_STEP 0.1 %, ACS_REM, ACS_FLAT_N 2, ACS_FLOOR, ACS_MAX_MS 5000) | AC+DC and AUTO, after a range or unit change of the same quantity, and after an input step (ACS_DC_STEP 0.5 %, ACS_JUMP 1.5x) | 2 "flat" AC results in a row | 5 s + one DC phase (< 6 s); 10 s when the limit runs out on a jump (ACS_JUMP restarts it, f42a09e) | c4a0b09 / f4925f5 / c8c2f02 / f42a09e (2026-10-23). The AC-coupled AD2 node recharges to the new tap's DC level, and the step reads as AC: device 8087c7d 0.35 V AC part for 2-4 s, READ? +6 %; AUTO switched to AC on it; an input step moved AC+DC autorange 6 -> 60 V and back after 6 s (bab5dd4) |
| G6 | `acdc_down` ACDC_DOWN_MS 400 | AC+DC / AUTO step down | DC results asking for it for 400 ms, i.e. the next DC phase | +~1 s; together with G5 the legitimate 60 -> 6 V step-down took 4-5.5 s on the device | c08b0d5: the calibrator's level-change dip 7.0 -> 6.3 V stepped 60 -> 6 -> 60 V, ~4 s without a reading |
| G7 | AUTO coupling's first decision (AUTO_SETTLE_K 4, AUTO_TIE 1 %, AUTO_TIE_MS 4000; AUTO_VOTES 2 later) | AUTO coupling entry | AC part settled on one side of \|DC\| | shows the DC part until then, up to 4 s (emulator: 0.5 V DC part at 1.3 s, the 1.0 V AC part at 3.4 s); a later change of the part needs 2 cycles (2-4 s) | 2481c4d: the first AC part overshoots (+3..7 %, 2563fe1 / a0985e4) and decided a 40 Hz square's tie |
| G8 | `reading_pending` (READ? / FETCh? +9.91E37) | SCPI only | first reading of the new range | not on the screen | f4925f5: READ? held a 3.39 V transient for 5 s (bab5dd4) |
| G9 | AD1 code window (FR-05 `Avg`) and OSR switch | DC V / mV / A, Ω at MED / SLOW | nothing (a partial mean is shown at once); +1 digit after AVG_FULL = 8 slow conversions (3.2 s) | none for the first number | FR-05 |
| G10 | display smoothing (`Smooth`) | if enabled (off by default) | restarts on a step | none | FR-05 / P3 |
| G11 | rail wake RAIL_SETTLE_MS 50 + `wake_settle` (DC 450, AC / AC+DC / power 2000) | coming back to the DMM page from Scope / Gen (DMM rail off) | time | 2.05 s for AC | 036cc55 / ea29bc4: a row written into a chip still powering up; the AC node starting from a powered-down front end (+8 % first AC reading) |
| G12 | AC EMA (FUN_080160c6 port: thr window, a 0.25) | AC V / A | the first result jumps 99 %, then a = 0.25 | ~0.5 s more to 0.1 % in the emulator; device DC -> AC 2.5-2.9 s (the RMS of the DC input's step decays from 15-38 mV) | stock-identical |

Not gates: the settings save (backup SRAM, no flash erase); the SCPI MEAS?
full-resolution wait (host only).

## 3. Measurements

### 3.1 Emulator (`emu/hy3131/switchlat.py`, model C, 2026-10-23)

Seconds from the switch to the first reading / to the reading after which
every reading stays within 0.1 % of the final value. fw-rs: function changes
are DBG:TAP on the function bar (the touch path), ranges are SCPI
`<f>:RANG` (`Dmm::set_range`, as the MF page), input steps are
`hy3131 SetInput` at a fixed virtual time. Stock: function-bar taps and
input steps, the DMM struct sampled every 50 ms. `[first x]` = the first
value was not yet within 0.1 %. n/m = not measured: the stock MF-page taps
in the script did not change the range (the stock rows for 6>60, 60>6 and
AC+DC 60>6 show no switch), so those stock cells are missing.

| switch | stock | fw-rs MED | fw-rs SLOW | fw-rs FAST |
|---|---|---|---|---|
| DC V -> AC V (1 V 50 Hz) | 0.35 / 1.00 [0.99] | 0.48 / 0.55 [0.99] | | |
| AC V -> AC+DC (1 V AC + 0.5 V DC) | 0.15 / 0.15 (see G4) | 1.30 / 1.44 [1.109] | | |
| AC+DC -> AUTO coupling | (no AUTO) | 1.30 shows DC 0.5 V; AC 1.0 V at 3.44 | | |
| AUTO / AC+DC -> DC V | 0.35 | 0.47 | | |
| DC V -> Ω (4.7 kΩ) | **1.15** | **3.08** | **3.09** | **2.83** |
| Ω -> DC V | 0.95 | 0.54 | 0.55 | 0.54 |
| DC V -> DC A (50 mA) | 0.35 | 0.74 | 0.73 | 0.68 |
| DC A -> DC V | 0.35 | 0.53 | 0.53 | 0.50 |
| DC V 6 -> 60 V manual (1 V) | n/m | 0.51 | 0.51 | 0.48 / (1 V on 60 V FAST: noise > 0.1 %) |
| DC V 60 -> 6 V manual | n/m | 0.53 | 0.51 | 0.46 |
| DC V AUTO up, 1 -> 30 V step | 0.45 | 0.40 | 0.42 | 0.36 |
| DC V AUTO down, 30 -> 1 V step | 0.45 | 0.40 | 0.02 * | 0.07 * |
| AC V AUTO up, 1 -> 30 V | 0.55 / 0.70 | 0.47 / 0.56 | | |
| AC V AUTO down, 30 -> 1 V | 0.65 / 0.85 | 0.10 / 0.64 | | |
| AC+DC AUTO down, 7 -> 4 V DC (+0.5 V AC) | 1.90 | 1.62 | | |
| AC+DC 60 -> 6 V manual | n/m | 1.79 | | |

\* the conversion running at the step already reads the new input on the
old range (the model has no AD1 filter memory by default, see 3.3); the
range step after it is hidden behind that reading.

### 3.2 Device (coordinator, ad74e93, FAST, 1 V / 5 V)

DC V -> AC V first 458-463 ms, within 0.1 % at 2.46-2.89 s, screen 0.5-0.8 s;
AC V -> DC V 457-462 ms; **DC V -> Ω 2.67-2.68 s, screen blank until
2.0-2.3 s, number at 3.0 s**; Ω -> DC V 516-556 ms, screen 1.0 s; DC V -> A
752-762 ms (0.000 A at 1.0 s); DC V -> mA 556-561 ms; AC V -> AC+DC
1.32-1.37 s, screen 1.5 s; DC V 6 -> 60 V 552-561 ms (the old reading
stays). These match the emulator within ~0.1 s, except for the AC settle of
a DC input, which the emulator does not model (3.3).

The "screen" times are 0.1-0.5 s later than the first FETCh?. Probable
causes, not verified: the readout is drawn at most every 80 ms (SHOW_MIN_MS),
and a function change repaints the whole static page first.

### 3.3 What the emulator's HY3131 model does and does not show

- AD1 is evaluated lazily with the input at the end of the conversion. By
  default there is no sinc^3 memory of the previous tap (the `ad1mix:1`
  option models it), no relay bounce or settling, no input or probe
  capacitance charging (Ω), and no dielectric absorption. So the vet (G2) never
  fires in these runs, and the DC and Ω numbers are best cases for the
  timing. They are still close to the device numbers, because the gates are
  timers.
- The AC-coupled AD2 node is a single tau 0.3 s RC, charged only in the AC
  phases. The device's tail is longer (2-4 s, then 18 -> 1 mV over ~10 s,
  acdc-stock-vs-fwrs.md 12), so G5 / G6 end sooner in the emulator (AC+DC
  60 -> 6 V 1.8 s; device 4-5.5 s for the autorange step-down).
- No RMS of a DC step on plain AC V (the device's 2.5-2.9 s to 0.1 % on
  DC V -> AC V with a DC input).
- Touch: a stock tap is a scripted touch-controller event (its latency is
  included); fw-rs DBG:TAP enters at the gesture layer.

## 4. Proposals

The aim: show the new function's frame and a number as fast as the stock,
and keep what each gate protects. Ranked by gain / risk. Times are for the
emulator / device paths above.

| rank | change | gain | expected | risk | keeps |
|---|---|---|---|---|---|
| 1 | **Draw the new function at once**: on `func_changed`, draw the unit / glyphs and a neutral placeholder (dimmed dashes, never `0.0000`, which would be a false reading) instead of a blank readout | the perceived stall on every switch | frame at ~0.05 s (stock: 0.05 s) | none (UI only) | the rule that no old-function value is shown under the new unit |
| 2 | **Ω entry: drop the 2500 ms deadline**. Enter with the stock's timing (no deadline, or 150 ms like a range write) and keep `entry + 2500` as the stock's one-range-per-step window, which then starts to work | 3.1 -> ~1.2 s (device 2.7 -> ~1.2 s) | stock 1.15 s | low: this is the stock's own behaviour; the single-step window still guards against the probe-capacitance / open-lead transient the stock disables fast down-ranging for | the stock range-hunt logic |
| 3 | **Function entry settles to the stock's effective ~300 ms**: V / AC V 450 -> 300, A 650 -> 300 (keep the in-function values: unit changes, AC+DC phases, autorange) | DC V -0.15 s, A -0.35 s | DC V ~0.35-0.4 s, A ~0.4-0.45 s (stock 0.35) | low-medium: the stock does it on the same hardware (relay pulse 10 ms inside write_config); check the A-jack relays (PE0 / PE1 shunt switch) on the device: first A reading against a calibrator | everything else |
| 4 | **Manual V range 450 -> 250** (`set_range`, the stock's range-write deadline 0xfa) | -0.2 s on MF / SCPI range changes | ~0.3 s | low | G2 still vets the first conversion |
| 5 | **Vet only where a stale tap is possible**: no held first conversion when the row write changed the R22 that was running, so AD1 restarts. The quiesce in write_config does not restart it, or bab5dd4 would not have happened. Row R22 (hy3131_tables.rs): the DC V / mV / A rows are all 0x15, the Ω row 0x14 (run at 0x13 / 0x17), the AC rows 0x16, and a DC row in steady MED / SLOW runs at 0x17. So Ω -> V / A, AC -> DC, and V <-> A at MED / SLOW (0x17 -> 0x15) restart AD1. **V <-> A at FAST (0x15 -> 0x15) does not, and keeps the vet.** Keep VET_N for range steps within the same DC mode and in AC+DC | -1 conversion (0.1 s) where R22 changes | Ω -> V ~0.45 s; V -> A at MED ~0.6 s, with 3 ~0.35 s | low-medium: device bab5dd4 had 0 of 10 bad readings after a single step that changed R22; all of the 0.78 V cases were a second row at the same R22. The rule must compare the R22 actually running (`self.r22`), not the mode | the 0.78 V and 25-51 % protections |
| 6 | **Provisional readings** while G2 / G4 / G5 / G7 hold: show the held value dimmed and without the last digit (or two), then turn it normal when the gate releases. Autorange, limits, statistics, CSV, REL and SCPI keep using only settled readings (`reading_pending` unchanged) | AC+DC range change: no number for 1.8 s (emulator) / 2-5.5 s (device) -> a dimmed number at the first result (~0.6-1 s); AC+DC entry 1.3 -> ~0.6 s (dimmed hypot(0, DC) or the DC part alone, labelled); AUTO coupling: a dimmed "deciding" state instead of showing DC as if settled | first number ~0.5-1 s everywhere | medium: a dimmed 0.35 V transient (+6 %) is visible for a moment; the owner must accept dimmed digits as "not final". It must never feed autorange (the excursion bugs) or READ? (bab5dd4) | all gates keep their protective role, only the display changes |
| 7 | **Shorter AC settle limits where the device allows**: ACS_MAX_MS 5000 -> ~3000 and an AC+DC step-down in the same DC phase when the vet passed (instead of the next DC phase) | AC+DC 60 -> 6 V 4-5.5 s -> ~3 s | ~3 s | medium-high: these limits came from device transients (2-4 s node recharge, the 7.0 -> 6.3 V dip); only with a device sweep (calibrator steps, DBG:ACDC:TRACE) | needs device data first |
| 8 | AUTO_TIE_MS 4000 -> 2500 | the AUTO coupling's first decision up to 1.5 s sooner | ~2.5 s | low-medium (a tie is shown as DC sooner) | the overshoot rule (AUTO_SETTLE_K) |

1 + 2 + 3 together give stock-like behaviour for the common switches:
the frame at 0.05 s, DC V ~0.35 s, A ~0.4 s, Ω ~1.2 s, without touching
any protection added on 2026-10-23. 6 is the general answer for AC+DC /
AUTO, where the real multi-second gates are.

Which earlier bug each gate protects, so a change can be checked against
it: the 0.781 V / 0.767-0.790 V readings after a two-step autorange (G2;
`emu/hy3131/autorangetest.py` with `ad1mix:1` reproduces it); the 25-51 %
after a same-range row rewrite (`same_row`, G2); the AC+DC 0.52 V DC part
(c8c2f02 ACDC_STOCK_OSR, G2 as the backstop); READ? holding a 3.39 V
transient for 5 s (G8); autorange excursions 6 -> 60 V on the AC part's
transient and 60 -> 6 -> 60 V on the calibrator dip (G5, G6; acdcmethtest,
couptest); AUTO deciding a tie on the overshoot (G7, couptest).

## 5. Reproduce

```sh
cd fw-rs && ./build.sh && cd ..
emu/hy3131/switchlat.py fwrs --app fw-rs/target/native/thumbv7em-none-eabihf/release/dm40-fw \
    --port 45010 --rate MED --json emu/out/swl-fwrs-med.json          # ~20 min wall
emu/hy3131/switchlat.py fwrs --app ... --port 45011 --rate FAST --dc-only --json emu/out/swl-fwrs-fast.json
emu/hy3131/switchlat.py stock --part 1 --json emu/out/swl-stock1.json  # ~12 min; part 2 in parallel
emu/hy3131/switchlat.py report emu/out/swl-stock.json emu/out/swl-fwrs-*.json
```

fw-rs uptime is virtual time minus 0.906 s (the DWT CYCCNT reset at fw-rs
start; `BOOT_OFS`). Each stock monitor sample is one Renode pause (~1-3 s of
wall time), so the stock sampling is 50 ms only for the first 3.2 s after a
switch and 250 ms after that.

## 6. Implemented (branch fastswitch, 2026-10-24)

Proposals 1-6 as approved by the owner (2026-10-23); 7 and 8 (ACS_MAX_MS,
ACDC_DOWN_MS, AUTO_TIE_MS) wait for a device sweep and are unchanged.

1. **Frame at once** (`Dmm::placeholder`, App `prov`): on a function /
   coupling change the readout shows the new function's unit, glyph, range
   label and (AC+DC / AUTO) its part rows, the value as dashes in
   VALUE_PROV (the stock grid grey 0x94D3), never 0.0000. The first settled
   reading replaces it at once (`Meter::show_now`). Emulator (DC V -> ohm
   tap, LCD screenshots every 50 ms): the new page with "- - Mohm" is up
   0.15-0.20 s after the tap event (touch scan and gesture included).
2. **Ohm entry** `OHM_ENTRY_MS` 150 instead of the 2500 ms deadline;
   `OHM_SINGLE_MS` 2500 is the stock's one-range-at-a-time window again.
3. **V / A entry** `ENTRY_MS` 300 (buttons, `select`, the plain DC / AC
   coupling of `set_sub`); the AC+DC / AUTO entry's DC phase, autorange,
   unit changes, manual A ranges and the power meter -> A path keep theirs.
4. **Manual V range** `RANGE_MS` 250 (AC+DC: 450 as before).
5. **vet_fresh**: no held first conversion on a function / coupling change
   into plain DC V / A when the R22 read from the chip before the row write
   differs from the new one (AD1 restarts). Range and V <-> mV / A unit
   steps, same-R22 changes (V <-> A at FAST) and AC+DC keep the full vet.
   (The task text said "hold only when R22 changed"; its own example and
   proposal 5 say the opposite, which is what is built.)
6. **Provisional readings** (`Reading::prov`): G2 (a held DC conversion,
   raw), G4 (AC+DC before both parts: the missing part's row shows
   dashes), G5 (AC+DC while the AC part settles: its latest result) and G7
   (AUTO before its first decision with an AC part above the floor) are
   drawn in VALUE_PROV on the DMM page (strip: C_DIS). `Dmm::poll` returns
   them before every consumer; main.rs hands them to `App::provisional`,
   which only draws: no meter (statistics, REL, limits, AutoHold, trend,
   CSV, beeps), no `last_raw` (BT), no SCPI (`reading_pending` stays,
   READ? / FETCh? +9.91E37), no autorange. G7 readings are measured values
   that the sentinel / OVERLOAD release / watchdog saw before, so they still
   do (without that couptest iacfill / sqtie failed). HOLD, an AutoHold
   capture and REL keep their readout. Consequence: in AUTO, FETCh? is
   +9.91E37 until the first decision (AC+DC -> AUTO below: 1.3 -> 3.6 s);
   READ? / MEAS? after the host's change wait for it, at most until 3.9 s
   after the coupling was entered (main.rs AUTO_WAIT_MS; a later query at
   most 0.85 s), then the part AUTO shows by then, never "Data stale" (a tie
   of AC and DC can take ~10 s to decide). emu/hy3131/autowaittest.py:
   MEAS? DC 1.28 s, clear AC 3.46 s, tie 3.91 s; libsigrok-style READ? 3 s
   after CONF 0.42 s (AC) / 0.85 s (tie).

Dim colour: VALUE_PROV = DIM (144, 152, 152), chosen over a darkened amber
(0x8220: 128, 68, 0): the grey reads as "not final" and cannot be taken
for the >= 30 V red (VALUE_HV) or a dim screen. Renders: `tools/ui-render`
sw01..sw14 (sw01 = the old blank readout, sw02..sw06 placeholders, sw07 /
sw09 / sw10 / sw12 provisional, sw08 / sw11 settled, sw13 / sw14 the
combined view's strip); ui-render matrix with ph_* / pv_* cases: 0
violations.

### 6.1 Emulator, first settled FETCh? (s; first / within 0.1 %)

switchlat.py, model C; before = falsehint 93d8033 (the same DMM code as
rust-fw-poc f0d21eb), after = fastswitch on f0d21eb.

| switch | MED before | MED after | SLOW before | SLOW after | FAST before | FAST after |
|---|---|---|---|---|---|---|
| DC V -> AC V | 0.48 / 0.55 | 0.32 / 0.43 | | | | |
| AC V -> AC+DC | 1.30 / 1.45 | 1.30 / 1.44 | | | | |
| AC+DC -> AUTO coupling | 1.29 (DC part) / 3.44 | 3.56 / 3.56 (G7) | | | | |
| AUTO -> DC V | 0.48 | 0.32 | | | | |
| DC V -> ohm (4.7 kohm) | **3.08** | **1.19** | **3.08** | **1.19** | **2.83** | **0.93** |
| ohm -> DC V | 0.53 | 0.34 | 0.54 | 0.33 | 0.53 | 0.33 |
| DC V -> DC A (50 mA) | 0.72 | 0.33 | 0.74 | 0.33 | 0.69 | 0.35 |
| DC A -> DC V | 0.52 | 0.32 | 0.53 | 0.32 | 0.48 | 0.34 |
| DC V 6 -> 60 V manual | 0.51 / 0.91 | 0.31 | 0.52 | 0.31 | 0.48 / 6.58 * | 0.36 / 6.64 * |
| DC V 60 -> 6 V manual | 0.53 | 0.30 | 0.51 | 0.31 | 0.47 | 0.33 |
| DC V AUTO up 1 -> 30 V | 0.41 | 0.59 ** | 0.41 | 0.60 ** | 0.35 | 0.31 |
| DC V AUTO down 30 -> 1 V | 0.41 | 0.20 ** | 0.39 | 0.19 ** | 0.06 | 0.01 / 0.32 |
| AC V AUTO up 1 -> 30 V | 0.48 / 0.58 | 0.50 / 0.59 | | | | |
| AC V AUTO down 30 -> 1 V | 0.01 / 0.55 | 0.03 / 0.56 | | | | |
| AC+DC AUTO down 7 -> 4 V | 0.11 | 0.04 | | | | |
| AC+DC 60 -> 6 V manual | 1.69 | 1.50 | | | | |
| AC+DC -> DC V | 0.49 | 0.33 | | | | |

\* 1 V on the 60 V range at FAST: the noise is above 0.1 %.
\*\* No code on this path changed (autorange settles and the vet are as
before): at MED / SLOW the steady OSR is 32768 (400 ms per conversion), and
the input step lands at another point of the conversion running then,
because the earlier windows' shorter settles moved the conversion phase.
Up and down moved by ~0.2 s in opposite directions; FAST (100 ms
conversions) shows no such shift. The same 0.59 / 0.20 appear with
proposals 1-4 alone (bisect build).

Device: the fastswitch build is not measured yet. Baseline (settle.py at
1 V, RATE FAST, f0d21eb; device-check-2026-10-21.md "f0d21eb" item 5, the
same as ad74e93): DC V -> AC V 460-462 ms, AC V -> DC V 457-460 ms, DC V ->
RES 2672 ms, RES -> DC V 554-558 ms, DC V -> A 758 ms, DC V -> mA 557-559
ms, AC V -> AC+DC 1372-1377 ms, DC V 6 -> 60 V 509-557 ms. Retest steps:
TODO.md "Switch latency".
