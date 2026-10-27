# Scope extra channels: GEN (computed) and DMM CH2

Owner request 2026-10-27: the generator as a scope channel, mainly as a
trigger source ("it need not be captured and digitised: compute when the
trigger fires and draw accordingly"), and the DMM's wave stream as an
optional second channel on the MCX scope page.

## 1. GEN: the generator as a computed channel

### 1.1 Why it can be computed

Both the generator and the MCX acquisition are timer-driven from the same
200 MHz clock (PLL from the 25 MHz crystal; HCLK = APB timer clocks =
DWT, [clock.rs](../src/clock.rs)):

| what | clock | position readable |
|---|---|---|
| generator, DAC path | TIM6 update = one DAC sample, M = (PSC+1)(ARR+1) ticks; DMA0 S5 plays a circular table of L samples holding K periods | S5 NDTR (table position), TIM6 CNT (ticks into the sample) |
| generator, PWM path (square > 50 kHz) | TIM12, D = (PSC+1)(ARR+1) ticks per period | TIM12 CNT |
| MCX scope | TIM1 CC4 = one sample, (PSC+1)(ARR+1) ticks; sample n at CC4 of period n after the enable | the enable instant (DWT) |

So one reading of the generator at the scope's start fixes the
generator's phase at every sample of that record, with no drift (one
crystal). Nothing is digitised; the GEN channel needs no wire.

### 1.2 The model ([genplan.rs](../src/genplan.rs) `Shape`, `Sync`; host test tools/gen-test)

* Phase 0 = table sample 0: sine mid-rising, square rising edge, triangle
  and ramp at the bottom; PWM: count 0 (its rising edge).
* `gen::now()` ([gen.rs](../src/gen.rs)): the generator's time in ticks
  into its table loop (q = M L ticks for K periods; PWM q = D, K = 1) and
  the DWT count of that instant. DAC: the output (DOR) holds table sample
  L - NDTR - 2 (each TIM6 update copies DHR to DOR, then the DMA loads the
  next sample into DHR), plus TIM6 CNT (PSC+1) ticks into it. Retried when
  NDTR moved during the reads, CNT is within 8 ticks of an update (the DMA
  may lag), or an interrupt stretched the reads (> 64 cycles).
* `Scope::start` reads the generator right before the TIM1 enable and the
  DWT right after it: `Sync { t0, s, q, k, shape }` with
  t0 = g + (DWT_enable - DWT_g) + ccr4 (PSC+1) - lag, s = ticks per sample.
  Sample n is at generator time t0 + n s, phase frac((t0 + n s) k / q).
  More than 400 cycles between the two reads (the AD2 stream ISR, say):
  no phase for that record (no GEN trigger, no GEN trace).
* The trigger: `Shape::crossing(level, slope)` gives the phase where the
  output crosses the trigger level (V) in that direction (sine by
  bisection; square 0 / duty; triangle u/2 / 1 - u/2; ramp u / 0); a level
  not strictly inside the output's swing never triggers. `Sync::next(n0,
  ph)` = the first sample at or after the next crossing.
* The trace: `Sync::span(a, b)` = the output's min / max over samples
  [a, b) (the column), from the shape (ends plus the extremes / jumps in
  between). The DAC staircase and the output filter are not modelled.
* PWM path: the levels are 0 and 3.3 V (the logic output, not measured
  after the output stage).
* `gen::epoch()` counts output (re)starts: a record read in another epoch
  has no phase (the user changed the generator meanwhile).

### 1.3 How each acquisition path uses it ([scope.rs](../src/scope.rs))

| path | GEN trigger |
|---|---|
| linear record (fs > 1.25 MS/s) | `finish`: the first crossing in [pre, pre + CAP - screen]. That window (65 us at 100 us/div, 32 us at 50 MS/s) can be shorter than a generator period, so `gen_ready` starts the record at the generator phase that puts a crossing in the window's middle: busy-wait up to 1 ms (`GEN_SPIN`), else not this main-loop pass. Auto past its timeout starts at once (untriggered if no crossing falls in). |
| stream (fs <= 1.25 MS/s, circular) | `stream_poll`: the next crossing is computed instead of searched; emitted once its post-trigger part is in, as the CH1 stream path. |
| comparator (hwtrig) | not used: off while the source is GEN or the GEN trace shows (its stop instant is not known, so its records have no phase). |
| roll | no trigger (as CH1); no GEN trace (column frames). |
| wave view (source DMM) | not offered: the HY3131 runs on its own crystal. |

The trigger type (pulse / runt) does not apply to GEN: edge only.

### 1.4 UI, SCPI, settings

* MF > Trigger > **Source: CH1 | GEN**; the trigger badge in the GEN
  colour (cyan) for GEN; a toast when GEN is picked with the generator off.
* MF > Display > **GEN trace: On | Off**: the computed output, cyan, under
  CH1, on CH1's V/div and position (a loopback to CH1 at x1 lies on it).
  Hidden while dragging / pinching (drawn again on release) and in roll /
  zoomed-out column frames.
* SCPI `TRIGger[:EDGE]:SOURce CHANnel1|GENerator` (`CHAN1` / `GEN`);
  `DBG:GENTRIG:LAG <ns>` (not kept) sets the lag below.
* Kept across a restart in the same session with the other trigger
  settings (W_SCOPE_EXT word 3, bits 4 / 5).

### 1.5 Unknowns, device check

The lag between the timers and the voltage at the CH1 input (DAC settling,
the generator's output stage, the scope's front end and the ADC pipeline,
the TIM8 latch below 50 MS/s) is 0 until measured; it matters at the fast
timebases only (40 px per division: a 100 ns lag is 40 px at 100 ns/div,
0.2 px at 20 us/div). Device check, the generator looped back to CH1 (x1):

1. Sine 1 kHz 3 Vpp, 200 us/div, Source GEN, level 1.5 V rising, Normal:
   T'D on every frame, the CH1 crossing on the T marker; GEN trace On: the
   cyan trace on CH1. Repeat with falling, level 0.5 V / 2.5 V, Single.
2. Level 3.2 V (outside the swing): WAIT in Normal, AUTO frames in Auto.
3. Square 2 MHz (PWM path), 100 ns/div: the edge on the T marker; read the
   offset in ns and set `DBG:GENTRIG:LAG`; repeat at 10 ns/div and 1 us/div
   (same offset expected). Then the DAC path: square 10 kHz, 1 us/div.
4. Slow generator against a fast timebase (1 kHz, 1 us/div): triggered
   frames (the timed start), no stall of the page (DBG:PING?).
5. Generator off with Source GEN: Normal waits, Auto runs untriggered.

## 2. DMM CH2 (the AD2 stream on the MCX page)

### 2.1 What it is

The wave view's stream ([dmm-input-scope-feasibility.md](dmm-input-scope-feasibility.md)
sections 9-11: the HY3131 AD2 at 1.2 .. 76.8 kS/s in one timer ISR) drawn
as a second trace (magenta) over the MCX record, time-aligned, from
1 ms/div. The DMM reading pauses while it runs, as in the wave view
(decision #3: the stream owns the HY3131 bus).

### 2.2 Time alignment

* The stream's samples carry the producer's microsecond stamp (`ts_us`,
  logger.rs: the DWT time of the ISR's read since the session began). The
  logger keeps the DWT count of the session's begin, so a stamp maps to the
  DWT clock: DWT = c0 + 200 ts_us (+-1 us).
* The MCX record's sample 0 is at the TIM1 enable (DWT, 1.2); sample n is
  n / fs later. So each MCX column has a DWT interval, and the CH2 samples
  stamped inside it give the column's CH2 min / max.
* The stamp is the read, not the conversion: a conversion integrates over
  the period before it, and the AD2 filter delays it further (a constant
  per rate). That offset is to be measured on the device (a step on both
  inputs) and taken off the stamps; until then CH2 may lag CH1 by about a
  conversion period (13 us at 76.8 kS/s = 0.5 px at 1 ms/div).
* DWT wraps every 21.5 s: the CH2 ring keeps 32-bit DWT stamps, and a
  record spans at most 10 s/div x 10 = 100 s only in roll (column frames),
  where CH2 uses its own roll columns instead.

### 2.3 Plan (not built yet)

1. logger.rs: `begin_cyc` (DWT at the session's begin) and a drain with
   the stamps.
2. A CH2 ring (4096 x (u16 code, u32 DWT), 24 KB? no: codes 8 KB plus a
   stamp per 64 samples, the stream is regular between holes) filled every
   main-loop pass and at the scope pass's slow parts (`wave_keep`).
3. main.rs: the stream started / restarted (rate per timebase,
   `wave_fs_for`) / stopped for CH2 as for the wave view; paused for a disk
   job; restarted on a DMM range / function change (V / A only).
4. The page: CH2 rows per column from the record's DWT span; its V/div
   (the wave view's steps) and position; Display tab "CH2 (DMM): On | Off";
   the strip marker.
5. Roll: CH2 roll columns on the MCX roll's column grid.
