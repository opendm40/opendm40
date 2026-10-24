# Inrush current (2026-10-22)

TODO "Inrush current" (owner 2026-10-22, as the Fluke 376 / 323 clamps):
arm in the 10 A range, trigger when the current steps up past a threshold,
show the RMS over the first 100 ms plus the peak, keep the trace with a
pre-trigger part and save it like the FAST*.DLG records. Everything below
is emulator evidence; the device check is section 9.

Code: `src/inrush.rs` (the detector, the trace, the results, the saved
records: pure, host-tested), `src/app_inrush.rs` (open / arm / close, the
end of a capture, the save glue; region C), `src/app_inrush_live.rs` (the
ring drain, the live state line, the SCPI queries; region B),
`src/ui_inrush.rs` (the page), main.rs (the stream start / stop, the save
session), `emu/hy3131/inrushtest.py` (the emulator scenario).

## 1. Data path

- **Source**: the HY3131 AD2 stream of the stock AC A row in the **10 A
  range**: OP1 x1 across the 10 mohm shunt, DC-coupled, +-125 A at the AD2
  rails (dmm-input-scope-feasibility.md 1.3). The same row as the wave
  view's A trace; AC and DC loads alike (the RMS of the raw samples covers
  both, offset by the DMM's AD2 calibration, `dmm::ad2_scale`).
- **Stream**: a wave-style logger session (`Logger::start_wave(4, 1,
  inrush::RATE_HZ)`), i.e. the stream ISR of section 11 (phase-locked, DMA
  frames by default) at **9600 S/s**: 960 samples in the 100 ms window, a
  104 us grid for the peak; ~4 % CPU (11.11). The ring belongs to the page
  (LOG:DATA? empty, LOG:STATe ON refused "Settings conflict", as for the
  wave view); the DMM reading pauses while armed (main.rs `busy`, as for
  the wave view) and resumes when the capture is complete.
- **Ring and trace**: every pass the main loop drains the logger ring
  (`logger::wave::drain`, 2048 samples = 213 ms of slack at 9600 S/s) into
  the detector and a ring of 16-bit codes `(raw + 4) >> 3` (3.8 mA per code;
  the AD2 noise is ~32 raw codes = 4 of these, so the shift loses nothing
  visible) in the **scope's record buffer** (`scope::BUF`, 26 624 slots,
  52 KB of SRAM0). `Scope::lend_buf` stops the MCX acquisition (its DMA
  writes BUF) and drops the stored scope record for as long as the page is
  open (a scope CSV in progress refuses the page); `return_buf` gives it
  back. No new SRAM: SRAM free is unchanged (11 872 B).
- **Trace length**: 200 ms before the trigger + 1.8 s after = **2.0 s**
  (19 200 samples), 100 ms per chart division. Longer would not fit BUF at
  9600 S/s with the 16-bit codes (2.77 s max); a lamp's inrush is over in
  ~0.1-0.3 s, a small motor's start in ~0.5-1.5 s.

## 2. Trigger rule

`inrush::Cap::push`: the **mean of |I| over the last ~1 ms (10 samples)
exceeds the largest such mean of the previous ~100 ms by the step
threshold** (default 1 A; the page's stepper 0.2 / 0.5 / 1 / 2 / 5 / 10 A,
INR:THR 0.1..50 A). The "envelope" is the maximum over 10 blocks of 10 ms;
the trigger compares against completed blocks only.

- Nothing flowing: an absolute threshold (|I| > step), either polarity
  (|I|: a reversed DC lead triggers too).
- A load already running: only a **rise by the step** triggers. The
  envelope holds the idle current's own peaks: an AC load of 2 A rms has
  1 ms means up to its peak (2.83 A), and that is in the envelope, so it
  does not trigger a 1 A step (host test, and the emulator: 6 s of a
  running 2 A rms 50 Hz load, then a step to 5 A rms triggers).
- **Why not an absolute level on the raw sample** (the wave view's ISR
  edge trigger): on an AC load an absolute level is crossed every half
  period; a step from the idle level needs an idle estimate, and the ISR's
  10-bit trigger code (0.24 A per code in this row) is too coarse for
  sub-ampere steps. The detector runs in the main loop on every drained
  sample (no dead time: the ring is drained in order; a hole is section 3).
- **Armed** once 200 ms + 1 ms of history are in (`Phase::Arming` until
  then; ~0.2 s after the arm or after a hole).
- **t = 0** is the first sample of the ~1 ms window that crossed: at most
  ~1 ms before the step reached the threshold. The 100 ms window starts
  there, so it can hold up to ~1 ms of the idle level (host test: within
  0.3 % of the exact window for a 12 A / 6 A step).
- **Not detected**: a rise slower than the step per ~100 ms (the envelope
  follows it). A soft starter's ramp is not an inrush.

## 3. Holes in the stream

`logger::wave::dropped()` (ring full + ISR misses; 0 in every device run
of section 11 at 9600 S/s) is checked every pass:
- while waiting: the history starts again (`Phase::Arming`, ~0.2 s);
- after the trigger: counted (`Res::lost`, INR:FETCh? 4th field, the page's
  state line); the saved file's stamps skip the lost stretch at the first
  hole, so the CSV's `lost_before` shows it. The trace on the page closes
  the gap (the samples are contiguous in BUF).

## 4. Results

Over the linear trace after the capture (`Cap::results`, region C: the
stream has stopped):

| value | definition |
|---|---|
| Irms 100 ms | sqrt(mean(I^2)) of the 960 samples from t = 0, DC-coupled (AC + DC rms) |
| Peak | the sample of largest abs(I) after t = 0, signed; **OL** when a sample reached the AD2 rail (abs(code) >= 32 700 of 32 767, ~125 A) |
| Idle | rms of the pre-trigger part (200 ms - 1 ms) |
| Lost | samples lost after the trigger (section 3) |

The zero is the DMM's AD2 calibration offset of the row (exact in the
results, rounded to a code in the detector).

## 5. The saved file

Each complete capture is saved as **LOGS/FASTnnn.DLG** (kind 3, the fast
logger's format, compact-log-format.md "Fast stream"), served as
LOGS/FASTnnn.CSV on the USB volume and by MMEM:DATA?, decoded by
tools/dlog.py, listed by the gallery as every FAST file. It goes through
the fast logger itself (fastlog.rs): `FastReq::Inrush` -> `fastlog::begin`
(19 200 x 3 B + 8 KiB planned) -> Prep (allocate / erase, region C, the
stream has stopped) -> `Ev::Start` -> main.rs gives the header
(`App::inrush_head`) instead of starting a logger session -> `poll(false,
drain)` with `drain` = the trace's records (`Cap::records`): the run ends at
once (no logger) and the Drain phase writes the whole trace, then Stop
trims the file. The GUI stays live during the save (main.rs `logging`
leaves the inrush save out: no logger panel, no frozen screen).

Why the FAST prefix and not a new "INRnnn": the prefix is what fatdir,
vdisk, dlog, csvlog's catalogue and the gallery know; a new one touches all
of them. The header says what it is:

    # Alientek DM40A, fw-rs 0.0.1, inrush capture, AD2, AC A row 10 A (10 mohm, OP1 x1, DC-coupled), nominal 9599.969 S/s
    # trigger: 1 ms mean |I| above the idle peak + 1.000 A; t0 at t_s 0.200000; Irms(100 ms) 7.5672 A, peak 12.016 A, idle 0.0506 A rms, lost 0
    # t_s from the first sample (nominal rate); raw = AD2 code / 8; value = the DMM's AD2 calibration; lost_before = conversions missing before the row
    t_s,raw,value_A,lost_before

- `raw` = the 16-bit code, `value_A` = a + b x raw (b = the row's A per
  AD2 code x 8, a = minus the calibration offset), as INR:PREamble?.
- `t_s` from the first trace sample at the nominal rate (the ISR samples
  on the chip's conversion grid, section 11.2; the locked rate is within
  ~1 ppm of it on the device), t0 = 0.200000 s.
- Emulator: 19 200 rows, 883 634-byte CSV; the DLG itself ~40 KB.

A save that cannot start (a CSV recording, an upload, no disk) or fails
shows "Captured, not saved: ..." on the page; the result stays on the page
and over SCPI. Arm is refused while the save runs (~1 s; the Prep erase
can take a few s on a dirty disk). Closing the page during the save
finishes the save first, then returns BUF (a toast says where it went).

## 6. UI

- **Entry**: MF in the A function -> Current Settings -> **Inrush** (the
  note row, right column, beside Fuse check; builds with the stream ISR,
  the default). The A function is a sub-function here as on the Fluke
  clamps; the Record page is for recordings of readings (decision #1), and
  an inrush capture is a triggered single event of the A input.
- **The page** (View::Inrush, range-page look: 0x2966 title bar, BG body,
  0x2966 boxes, SEL accent; the DMM page's strip chart geometry):

      Inrush Current                                  (title bar)
      Trigger      [ - ]     +1.0 A     [ + ]
      [ Irms 100 ms   7.567 A ] [ Peak   12.02 A ]
      Saved LOGS/FAST000.DLG                          (state line)
      12.62A |  trace, 2 s, 100 ms/div, the 100 ms   | IDLE
      100ms/div  window shaded, t = 0 a blue line     |  0.051
      ...                                             | FILE
      -0.601A|                                        |  FAST000
      [ Back ]  [ Arm / Cancel / Saving ... ]

  Opening selects AC A, 10 A (manual) and arms at once. State line:
  "Arming ...", "Armed: 0.050 A now" (the stream's 100 ms rms, 4 updates
  a second), "Triggered: recording 2 s ...", "Captured: saving ...",
  "Saved LOGS/FASTnnn.DLG", "Captured, not saved: disk full", "Stopped:
  DMM changed", "Cancelled: press Arm". Results "---" until a capture.
  The trace is a min / max envelope per column (60 samples): an AC load
  shows as a band.
- **Keys**: RUN = Arm / Cancel; AUTO refused (beep: the range is held);
  MENU / MF / Back close the page. Closing (or any view change) stops the
  stream, puts the A function's sub-mode (DC / AC / AC+DC) and autorange
  (AUTO / AUTO+ or the manual range) back, and returns BUF to the scope.
- Refused (toast): the power meter, a CSV / fast recording running, a
  scope CSV in progress, a save still running.
- ui-render scenes i01..i07 (armed, DC lamp, AC motor, OL with the longest
  texts, stopped, idle, triggered) with the overlap check, and each live
  update drawn over a page == a fresh page; tools/ui-render: 0 violations.

## 7. SCPI

| command | |
|---|---|
| `INRush[:STATe] ON\|OFF` / `?` | open the page (A, AC row, 10 A) and arm / close it (the A function's settings back) |
| `INRush:ARM` | (re)arm a new capture (opens the page; while armed: starts over) |
| `INRush:THReshold <A>` / `?` | the step, 0.1 .. 50 A (default 1); while armed it applies at once, the history kept |
| `INRush:CONDition?` | OFF, IDLE, ARMING, ARMED, TRIGGERED, SAVING, DONE |
| `INRush:FETCh?` | `Irms100,Ipeak,Iidle,lost` (A; peak signed, +9.9E37 = OL); all +9.9E37 and "Data stale" without a capture |
| `INRush:FILE?` | `"LOGS/FASTnnn.DLG"` of the capture or `""` |
| `INRush:PREamble?` | `rate_hz,samples,trigger_index,a,b` (A = a + b x code) |
| `INRush:DATA? [<start>[,<count>]]` | `#4nnnn` block of the trace's codes, little-endian i16, count <= 3000 |

Over Bluetooth the setters are refused ("Command protected": a capture
writes to the disk, as LOG:CSV ON); the queries work.

While a capture runs, poll **INR:CONDition?** (or INR:STATe?): those two
are in region B. INR:FETCh?, :DATA?, :FILE?, :PREamble? and the setters
are region C (slow flash): each costs one stall of the stream ISR's entry
(~10-20 us against ~47 us of margin at 9600 S/s, section 11.4), which
should not lose a sample but is not measured on the device (section 9).
The page's own full repaint at the arm (region C) runs while the stream is
still in `Arming`, where a hole only restarts the 0.2 s history.

## 8. Emulator (plumbing only, not the device)

`emu/hy3131/inrushtest.py` (200 MIPS; the front-end model's isrc source
switched at fixed virtual times; a private disk copy and its own log per
process): MF -> Inrush from DC A, then

| case | input | result |
|---|---|---|
| DC "lamp" | 0.05 A, then 12 A for 20 ms, 6 A for 80 ms, 3 A | Irms(100 ms) 7.567 A (exact window 7.589; t = 0 up to 1 ms early), peak 12.016 A, idle 0.0506 A, lost 0; LOGS/FAST000.DLG: 19 200 rows, t0 at 0.200000 s, raw = INR:DATA? |
| AC "motor" | 2 A rms 50 Hz running, then 5 A rms | no trigger by the running load; Irms 5.105 A, abs(peak) 7.229 A, idle 2.047 A |
| host CONF:VOLT:DC while armed | | capture stopped ("Stopped: DMM changed"), stream off, DC V reads |

INR:STAT OFF puts the DC A function back (FUNC? CURR); the DMM's AC A
reading of the 5 A rms load after the capture: 5.000 A.

The AC case reads 2 % high, the DC case does not: an emulator artefact. The
model's raw AD2 sine in this row is 2.2 % above its DC scale (a host LOG
session on AD2 in the same row, no firmware scaling: +-15 150 codes peak for
5 A rms, 14 830 expected; 5 A DC gives 10 486 codes, exact), while the
DMM's own AC reading comes from the model's RMS registers. The device run
(section 9, last rows) decides the real AD2 gain on AC.

## 9. Device check (25 minutes)

Image: the one named in the hand-over (built from the committed tree).
Meter on USB (`tools/dm40scpi.py`), the load on A-COM through the 10 A
jack, a second meter (clamp or DMM in its 10 A range) or the scope with a
shunt for comparison. **The 10 A fuse's rating / type is UNKNOWN**: keep
the inrush peak well under ~10x the fuse rating for a few ms and the run
current under 10 A; a motor start can be 5-10x its run current.

| min | step | how | pass |
|---|---|---|---|
| 0-2 | entry | A function, MF -> Inrush | AC A 10 A, "Armed: 0.000 A now" after ~0.2 s; `scpi "INR:COND?" "DBG:ISR?"` ARMED, active 1, late / slips 0 |
| 2-5 | lamp (incandescent, 12 V car bulb on a bench supply, or 230 V 40-100 W with care) | switch it on | "Triggered", 2 s later the result + "Saved LOGS/FASTnnn.DLG"; Peak ~5-10x the run current; Irms 100 ms between the peak and the run current; the trace a decay over ~50-200 ms |
| 5-8 | second meter | a clamp meter with inrush (Fluke 376 / 323 / UT21x) or the scope + shunt on the same wire | Irms 100 ms within ~5 % of the clamp's inrush value (clamps differ in their window start); peak vs the scope's |
| 8-11 | motor (drill, fan, small pump) | switch it on | trigger; the trace shows the start-up band; Irms vs the clamp; `INR:FETC?` 4th field (lost) 0 |
| 11-13 | running load, no false trigger | motor running, INR:ARM, wait 30 s | stays ARMED; then a second load on the same wire triggers |
| 13-15 | file | `scpi 'MMEM:DATA? "LOGS/FASTnnn.CSV"'` or the USB volume | 19 200 rows, t0 at 0.2 s, values in A; `tools/dlog.py` decodes the DLG |
| 15-17 | DMM after | Back; DC A / AC A on a steady load | the reading as before; FUNC? = the function before the page |
| 17-18 | fuse | after the motor runs: MF -> Fuse check | FUSE OK (the inrush did not weaken it visibly; note the fuse's marking if readable) |
| 18-20 | offset / bandwidth | nothing connected: INR:ARM, Idle ~0 (the AD2 offset at x1); a 50 / 60 Hz load: Irms 100 ms vs the DMM's AC A | Idle < ~20 mA; Irms within ~1 % of AC A (AD2 bandwidth and offset at x1 vs the DMM: UNKNOWN until this run) |

| 20-22 | host polling | INR:ARM, then a loop of `INR:FETC?` + `INR:DATA? 0,3000` + `DBG:ISR?` every 0.2 s through a lamp capture | INR:FETC? 4th field (lost) 0, DBG:ISR? late 0 (region C SCPI during a capture) |

Also note `DBG:PROF?` loop max during a capture and whether any toast /
redraw while armed caused a hole (`lost`). The AC-case RMS of section 8 is
not verified (an emulator artefact): rows 5-8 and 18-20 decide the AD2 gain
on AC at x1.

## 10. Budgets (build.sh, on 6dfffec)

| | before | with the inrush capture |
|---|---|---|
| region B free | 19 376 B | 13 924 B (the drain + detector, the live state line, INR:STAT? / :COND?, the SCPI parse, main.rs glue) |
| SRAM free | 10 904 B | 10 904 B (the trace lives in the scope's BUF) |
| stack headroom | 21 404 B | 21 204 B (the session state, ~200 B, is a TCM static) |

Region C: app_inrush.rs, the page and trace drawing, `Cap::results` /
`linearize` / `records`, the SCPI fetch / data / setters.

## 11. Limits / not done

- 9600 S/s only (19 200 / 38 400 would halve / quarter the trace in BUF;
  the 104 us grid and the ~2.4 kHz AD2 band are enough for mains-frequency
  inrush; a sub-ms spike's height is under-read).
- One capture at a time; re-arm replaces it on the page (the file stays).
- The threshold is not persisted (RAM, default 1 A).
- The 6 A range (x10, +-12.5 A) is not offered: an inrush past 12.5 A
  would clip.
- The trace is not zoomable; the CSV has every sample.
