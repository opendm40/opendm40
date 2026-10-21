# DMM inputs as a scope source (AD2 streaming): feasibility

Date 2026-10-21. Research only; no fw-rs source changed. Evaluates two owner
ideas for the TODO:

* **(A)** a "scope-like" mode on the DMM jacks: switch the HY3131 to its fast
  ADC (AD2), stream raw samples, show them with the stock-scope page, trigger
  in software;
* **(B)** a data-logger source faster than AD1 allows (today 40 .. 640 S/s).

Evidence tags as in `power-meter-and-misconnect.md`: **VERIFIED** = datasheet
text (`docs/DS-HY3131_EN.pdf`, transcribed in `emu/hy3131/ref/registers.md`,
`RESEARCH.md`), the stock row bytes / formulas (`firmware/re/analysis/
hy3131_v2_6.h`, `dmm_v2_6.h`), fw-rs code, the GD32F4xx User Manual (DMA
request table, bus clocks), or a device measurement already in
`acdc-stock-vs-fwrs.md` / a `DBG:PROF?` capture; **INFERRED** = follows from
those but is not stated anywhere; **UNKNOWN** = needs the device. Nothing under
`firmware/re/history/` was used. The sibling doc `power-meter-and-misconnect.md`
(sections 1-3) already establishes the AD2/AD3 mux, OP1/OP2 routing and the
row facts; they are cited, not re-derived.

## 0. Verdict

| idea | verdict | one line |
|---|---|---|
| (A) DMM-input scope | **GO, conditional**, as a *slow* scope: ~5-20 ms/div .. 50 s/div roll, 2.4-9.6 kS/s, bandwidth ~1-2.5 kHz | single AD2 conversions are readable (R03..R05, flag AD2F; ~14 ENOB), but there is no FIFO and fw-rs has no ISRs; the usable rate is set by the main-loop stalls (1-25 ms) and the bit-banged link (~122 us per sample), not by the chip (76.8 kS/s). Gap-free capture needs one small architectural change (section 5.2). Not "1 ms/div". |
| (B) AD2 logger | **GO** (low effort) | adds the 600 .. 9600 S/s rungs above AD1's 640 S/s, ~14-16 bits, gaps marked by the existing seq/timestamp scheme; same readout machinery as (A). Do (B) first: it is the device experiment for (A). |

The decisive UNKNOWN for both: whether the HY3131 latches the 3-byte AD2 data
register for the duration of a read (torn reads) and how many samples the real
loop drops at each rate. Section 6 is a one-session device plan that settles
both before any UI work.

## 1. AD2 / AD3 facts

### 1.1 Rate, resolution, range (VERIFIED, datasheet 11, 13.2; registers.md)

* AD2CLK = SYSCLK/2 (SAD2CLK = 0) or /4 (SAD2CLK = 1); SYSCLK = 4.9152 MHz
  crystal. Output rate = FAD2CLK / OSR2, OSR2 = 32, 64, 128, 256, 512, 1024
  (AD2OSR codes 5, 6, 7 are all 1024). ENCHOPAD2 halves it. AD3 shares the
  clock and OSR. The rate ladder, in samples/s:

  | OSR2 | /2 clock | /4 clock | /4 + chop |
  |---|---|---|---|
  | 32 | 76 800 (13.02 us, the stock AC rows) | 38 400 | 19 200 |
  | 64 | 38 400 | 19 200 | 9 600 |
  | 128 | 19 200 | 9 600 | 4 800 |
  | 256 | 9 600 | 4 800 | 2 400 |
  | 512 | 4 800 | 2 400 | 1 200 |
  | 1024 | 2 400 | 1 200 | 600 |

  Below that the LPF (OSR4 = 128 .. 16384 on the AD2 stream) gives a properly
  decimated stream down to a few S/s, but its real output period on the device
  is half the datasheet formula (RESEARCH.md 8.1: 106.7 ms measured vs 213 ms
  for LPFBW = 111) - any LPF-as-source rate must be measured per LPFBW.
* Data: AD2<18:0> signed 19 bit in R03..R05 (LSB first, bits 23:19 = sign
  copy; max 0x3FFFF, min 0xFC0000). Transfer (datasheet 13.2, RESEARCH 2):
  code = IG * Vin / (RG * VR) * 2^18, IG = 0.5 / 1.0 / 1.5 / 2.0 (AD2IG), RG =
  1 / 0.333 (AD2RG), VR = AD2RH - AD2RL = PB6 - AGND = 1.25 V in every stock row.
  Full scale at the input node: **+-1.25 V / IG**, i.e. +-1.25 V at IG 1.0,
  +-2.5 V at IG 0.5; 4.77 uV / code at IG 1.0.
* Inputs (mux, registers.md): AD2IP = OP1O, OP2O, PB4, PB7; AD2IN = RLU, AGND,
  PB3, PB5. SDO23 = 1 routes AD3's data into the same R03..R05 register
  instead (one data register for both fast ADCs, VERIFIED 13.2 "MUX").
* Gain drift 10 (typ) / 30 ppm/C (AD2/AD3, VERIFIED table 6) vs 2.5 ppm/C for
  AD1; no AD2 noise, ENOB, linearity or bandwidth figure anywhere in the
  datasheet (searched: the only bandwidth entries are OP1/OP2 2 MHz and the
  comparator 6 MHz).

### 1.2 Resolution actually available (INFERRED)

The device floor in the stock AC V 4 V row is 31..33 `sqrt(RMS)` counts with a
quiet input (acdc doc 6a, 6b). RMS = sum(X^2)/N of the high-passed AD2 stream,
so that is the per-sample rms noise: **~32 codes against the 2^19 full span
at 76.8 kS/s = 14.0 ENOB** by Hycon's own definition (APD-DMM001 eq. 3: ENOB =
log2(FSR / rms noise), FSR = the full code span; the same convention is used
for AD1 below); ~11.3 noise-free bits (pp ~ 6.6 sd). It includes the divider / OP1 front-end noise,
so the converter alone is at least that good. At higher OSR2 the comb filter
averages the modulator noise: white-noise scaling predicts ~16 ENOB at OSR2 =
512 (4.8 kS/s), but the 1/f part of the front end does not scale, so treat
14 .. 16 bits as the range until the shorted-input test (section 6, step 4).
For a 280-row display that is 5-7 bits more than needed; for the logger it
sits between AD1's 24-bit DC path and the scope's 10 bits.

For comparison AD1 at its fastest (OSR 256 unchopped, 640 S/s): 25 counts sd
at OSR 32768 chopped (6c) scaled by 1/sqrt(OSR) (the emulator's model, INFERRED)
gives ~280 counts of 2^24, ~15.9 ENOB. So AD2 at 2.4-9.6 kS/s and AD1 at 640
S/s have similar effective resolution (14-16 vs 15.9); AD2 buys rate, not bits.

### 1.3 Coupling: which jack reaches AD2, and how (VERIFIED rows + device)

| path | stock row | AD2 input | coupling | evidence |
|---|---|---|---|---|
| V (4 .. 1000 V) | f0 AC V r0..r3: R27 0x9d, R32 0x38 | PB4 - AGND, PB4 fed from OP1O (OP1+ = SENSE tap) through an external capacitor | **AC**, tau ~0.3 s (fc ~0.5 Hz) | acdc 5 / 6a transient; RESEARCH 7; power doc 2 |
| mV (40 / 400 mV) | f2 AC mV: R27 0x5d, R32 0xaa | OP2O - AGND, OP1+ = OP2+ = RLU | **DC** (the LPF reports the full DC: 6a "LPF = 495 mV") | acdc 6a |
| A / mA / uA | f4, f5_6 AC A: R27 0x2d, R32 0x3e / 0x3d | OP1O - PB3, OP1+ = PB2 (10 mohm) / PB1 (0.5 ohm, 47.5 ohm), x10 / x1 | **DC** (6b: LPF = 8.8 uA constant offset on the open uA range) | acdc 6b |
| V, DC-coupled | none in the stock | **register-only new row**: a DC V / AC V row's switch bytes (tap PA7 /10, PA6 /100, PA5 /1000, PA4+PA5 /10000) + ENOP1, OP1+ = SENSE (R32 0x38, which the stock itself writes in the AC+DC DC phase) + SAD2IP = 00 (OP1O) instead of PB4, SAD2IN = AGND or RLU, IG 1.0 | DC | mux table VERIFIED; OP1 on SENSE at those taps VERIFIED (f0 r1..r3, AC+DC); that AD2 reads OP1O cleanly there: INFERRED, test 6.5 (power doc 3.1 proposes the same node for AD3) |

So current waveforms (through the A shunt, DC included) and mV waveforms need
no new row: the stock AC A / AC mV rows already put the DC-coupled signal on
AD2; only the readout changes. Voltage waveforms are AC-coupled with the stock
row (fine for mains / audio, DC lost) and DC-coupled with the new row.

Enable rules that matter for a streaming mode (VERIFIED datasheet notes):
* 13.1 note 7: do not switch AD2/AD3 on or off while AD1 converts (AD1 offset
  shifts). Enable AD2 *with* the row and leave it; for the AC rows it already is.
* Turning ENAD2 off clears R03..R05 (13.2 (1)).
* AD2 and AD3 together: enable AD3 first (hy3131.rs `configure`, the beat fix,
  acdc 10). AD3 is not needed for a single-channel waveform; a streaming row
  should run AD2 alone with RMS/LPF off (or SDO23 = 0 and both on: the stock
  row, if the DMM AC reading is to continue in parallel, see 3.4).

### 1.4 Readability of single conversions (VERIFIED where stated)

* Register: R03..R05 (3 bytes, auto-increment read in one CS frame).
  **Width** 19 bit signed. **Flag**: INTF.1 AD2F set on every AD2 (and AD3)
  conversion; INTF is cleared by *reading it* (all flags at once, AD1F / RMSF /
  LPFF included) or by writing 0s; flags accumulate until read (datasheet 10).
  Optional **IRQ pin**: with INTE.AD2IE = 1, DO (= MISO, PC11) is high while CS
  is high and a flag is pending (datasheet 7.2 (4)) - a hardware data-ready
  line on an existing pin, unused by the stock (INTE stays 0) and by fw-rs.
* **No FIFO, no buffer.** The datasheet lists one data register per converter
  and no depth; the emulator and the RE headers model one register. An unread
  conversion is overwritten by the next (INFERRED from the register set; no
  text says otherwise).
* **Latch semantics UNKNOWN.** Nothing says whether R03..R05 are frozen during
  a CS-low read. At OSR2 = 32 the register changes every 13 us while a
  bit-banged 3-byte read takes ~77 us, so a read straddles 5-6 conversions. The
  stock never uses AD2 raw (it stores it at m+0xac and ignores it) and reads
  RMS/LPF right after RMSF, so no stock behaviour tells. The two "garbage"
  `sqrt(RMS)` values the stock dropped (acdc 6b, 370726 > 0x40000) are
  consistent with a torn 5-byte read but not proof. **This is device test 6.2
  and it bounds the usable OSR2 for any readout scheme: the read must finish
  well inside one conversion period, and must start right after AD2F.**

## 2. Readout bottleneck

### 2.1 What a sample costs on today's bit-banged bus (VERIFIED, hy3131.rs)

`HALF` = 40 spins ~ 1 us (the logger.rs numbers 45 / 77 us pin it down). `tx`
is 3 half-periods per bit (MOSI, spin, clock = 2 spins) = 24 us/byte, `rx` 2
per bit = 16 us/byte, begin 1, end 2, the read-period clock 2:

| frame | half-periods | time |
|---|---|---|
| `read1(R1E)` flag check: cmd + read period + 1 byte | 1 + 24 + 2 + 16 + 2 = 45 | 45 us |
| `read(R03, 3)` AD2 sample | 1 + 24 + 2 + 48 + 2 = 77 | 77 us |
| flag check + sample | | **122 us** (as AD1 today) |
| `read(R00, 6)` AD1 + AD2 in one frame | 1 + 24 + 2 + 96 + 2 = 125 | 125 us |
| `read(R00, 20)` the stock AC poll | 349 | 349 us |

INTF (R1E) and AD2 (R03) are not adjacent, so the flag cannot be fetched in
the same frame as the data (R03..R1E would be 28 bytes = 475 us). Link-only
ceiling: ~8 kS/s blind, ~4 kS/s with the flag check, at 100 % CPU in the
bit-bang loop. Realistic from the main loop: see 2.3.

### 2.2 Hardware SPI on the same pins

* **Pins.** PC10 / PC11 / PC12 are SPI3 SCK / MISO / MOSI on alternate
  function 6 in the STM32F4 pin map that the GD32F4xx follows (GD32 naming:
  **SPI2**, base 0x4000_3C00, APB1, VERIFIED in the UM memory map; the AF
  assignment itself is in the GD32F427 datasheet, not in the UM at hand ->
  INFERRED, confirmed by test 6.1). CS = PD2 stays a GPIO (no hardware-NSS
  framing is possible there; the UM's hardware NSS only toggles per transfer
  burst or per word in TI mode, neither matches a multi-byte HY3131 frame).
  SPI mode 0 (CK idle low, DO changes on the falling edge: datasheet 7.2
  (7)/(8); the MCU samples on the rising edge), MSB first.
* **Clock.** APB1 = 50 MHz (RCU_CFG0 0x940A read back on the device: APB1 /4,
  clock.rs). SPI prescalers give 25 / 12.5 / 6.25 / 3.125 MHz. HY3131 limits
  (VERIFIED table 6): TCKHI >= 20 ns, TCKLO >= 60 ns, DO ready 60 ns after
  the falling edge -> a symmetric clock may not exceed 6.25 MHz (80 ns low),
  and the 60 ns DO delay leaves 20 ns of margin at that speed *before* the
  isolator: the SPI crosses a CA-IS3741 digital isolator (BOM, INFERRED in
  `dm40-feature-request.md` 1.2; ~10-15 ns per direction) -> ~85-100 ns round
  trip, so **3.125 MHz** (160 ns half period) is the safe setting and 6.25 MHz
  must be proven by read-back (test 6.1).
* **The 9th clock.** A read is 8 command bits, one blank "read period" clock,
  then 8n data bits (7.2 (2)). A byte-oriented SPI cannot emit one clock, so
  clock cmd + (n + 1) bytes and shift the received bit stream left by one: RX
  byte k (k >= 1) holds [1 blank or carry bit | 7 MSBs of data byte k], the
  next byte supplies the LSB. For the 3-byte AD2 read that is 5 bytes = 40
  clocks (the extra 7 clocks read into R06, harmless; "readout stops when CS
  goes high"). Writes need no shift (7.2 (3)).
* **Cost.** At 3.125 MHz: flag check 3 bytes = 7.7 us, AD2 read 5 bytes =
  12.8 us, plus ~1-2 us of CS handling: **~22 us per sample with the flag
  check, ~14 us blind**; at 6.25 MHz half of that. The 20-byte DMM AC poll
  drops from 349 us to ~60 us. Link ceiling ~45 kS/s (flag-checked) - the
  link stops being the limit; the MCU's attention does.
* **DMA.** SPI2_RX -> DMA0 channel 0 (or 2, busy: UART4_RX), SPI2_TX -> DMA0
  channel 7 (or 5, busy: DAC0) (VERIFIED UM table 10-5; channel use from
  bt.rs / gen.rs; the scope's two streams are on DMA1). A fully autonomous
  timer-paced capture (timer CC -> DMA writes GPIOD_BOP for CS, DMA writes 5
  bytes into SPI_DATA, RX DMA into a ring) needs 4-6 timer events per sample
  spread over 3-4 DMA channels (TIMER3_UP/CH1/CH2 on channels 6/3/7 collide
  with the SPI channels) and still cannot honour AD2F, so it reads on its own
  grid and tears or duplicates once per beat of the two crystals. It is
  high-effort and uncertain; **not recommended**. The simple SPI-with-polling
  path (per-frame CPU work ~15-22 us) is enough if something runs it
  regularly, which is the real question:

### 2.3 Who runs the read: the main loop vs an interrupt

fw-rs runs with interrupts masked (`cpsid i`, shim.rs / startup.rs; logger.rs
header: "there are NO ISRs"). Everything is polled from one main loop whose
period is tiny most of the time but not always (two `DBG:PROF?` captures of the
device, 104-120 s each, DMM page):

| metric | capture 1 | capture 2 |
|---|---|---|
| passes / s | 22 247 | 33 523 |
| loop avg / ema | 44 / 27 us | 29 / 37 us |
| loop max, max in the last 1 s | **24.9 ms, 9.5 ms** | **16.0 ms, 4.5 ms** |
| passes >= 5 ms | 5 289 (~50 / s) | 48 |
| `d.read` (DMM poll, bit-bang) max | - | 15.5 ms |
| `d.chart` (strip chart redraw) | - | 0.9 .. 3.2 ms |

(VERIFIED numbers from `DBG:PROF?` captures found in the session scratchpad,
`prof-before.txt` / `prof-after.txt`, taken earlier on 2026-10-21 on the DMM
page, 104-120 s each; raw files not committed, this table is the record - as
acdc doc 9 does for its /tmp captures. The sections are the ones prof.rs
names. No device was used for this study.) The 1-25 ms
stalls are LCD redraws, the 20-byte bit-banged DMM read, the 10 ms relay pulse
and SPI-flash writes (45-400 ms per sector erase, logger.rs header).

Consequences for a polled AD2 reader with no FIFO:

| rate (OSR2) | period | samples lost per 5 ms stall | per 16 ms stall | gap-free without ISR? |
|---|---|---|---|---|
| 600 S/s | 1.67 ms | 3 | 10 | no (loses on every redraw) |
| 2 400 | 417 us | 12 | 38 | no |
| 9 600 | 104 us | 48 | 154 | no |
| 40 S/s (AD1 today) | 25 ms | 0 | 0 | yes (the logger's 38 Hz measured) |

Gap-free capture at any kS/s rate therefore needs either (i) the display and
flash traffic frozen while capturing (a "capture then show" single-shot
scope, no live redraw: the stalls that remain are the DMM poll and the relay
pulse, which the streaming mode controls), or (ii) **one interrupt**: a timer
or EXTI (the HY3131 IRQ on PC11 = EXTI line 11, free; touch uses line 5)
handler that owns the HY3131 bus, performs the 2-frame read in ~15-22 us and
pushes the sample into a ring the main loop drains. Cost per rate at 3.125
MHz: 5 % of the CPU at 2.4 kS/s, 10 % at 4.8 kS/s, 21 % at 9.6 kS/s, 42 % at
19.2 kS/s (the practical top). The ISR preempts bit-banged timing loops
(DWT-based delays, touch I2C fallback) harmlessly - all tolerate stretched
timing - and the scope's own DMA capture is unaffected. Shared state is one
SPSC ring (ISR writes, main reads) plus the INTF shadow of 3.4.

Jitter: with either scheme the *sample instants* are the chip's regular
conversion grid; the read instant only has to fall inside the same period
(timer at ~1.05x the AD2 rate, or the IRQ edge). What the MCU adds is the
ISR latency (< 1 us) - irrelevant - or, polled, nothing until a stall, which
then costs whole samples (detectable: the flag-check-then-read scheme sees
AD2F already set with a new conversion having overwritten the old one only if
the timestamp gap says so, as logger.rs's `missed` does today).

Sustainable rates, summary (3.125 MHz hardware SPI, flag-checked reads, torn
reads excluded by reading right after AD2F):

| scheme | gap-free rate | best-effort rate | effort |
|---|---|---|---|
| main loop, bit-bang (today's driver) | ~40 S/s | 600-2 400 S/s with gaps on every redraw | none |
| main loop, display frozen during capture, bit-bang | 600-1 200 S/s (DMM poll suspended) | 2 400 | small |
| ISR + bit-bang | 2 400 S/s (ISR 29 % busy) | 4 800 (58 %) | small, policy change |
| ISR + SPI2 at 3.125 MHz | 4 800-9 600 S/s | 19 200 | medium |
| ISR + SPI2 at 6.25 MHz (if the isolator allows) | 9 600-19 200 S/s | 38 400 | medium |

The AD2F-synchronised read bounds OSR2 from below as well: INTF frame + data
frame must fit the conversion period with margin (22 us vs 52 us at OSR2 = 128
fits; 26 us at OSR2 = 64 does not) -> **19.2 kS/s is the ceiling** of any
flag-synchronised scheme on this bus, 9.6 kS/s the comfortable one.

## 3. What that gives as a scope

### 3.1 Timebases

The stock-scope screen is 400 columns / 10 divisions (scope.rs COLS). One
sample per column needs 40 / (s/div) samples per second:

| s/div | samples/s for 1 per column | source |
|---|---|---|
| 1 ms | 40 000 | out of reach (section 2) |
| 2 ms | 20 000 | 19.2 kS/s at 42 % CPU: marginal |
| 5 ms | 8 000 | 9.6 kS/s |
| 10 ms | 4 000 | 4.8 kS/s (mains: 5 cycles per screen) |
| 20 .. 100 ms | 2 000 .. 400 | 2.4 kS/s with 1.2 .. 6 samples per column |
| 200 ms .. 50 s | roll | 600 S/s .. LPF rates, min/max per column |

Practical range: **5 (or 10) ms/div .. 50 s/div**, roll from 200 ms/div as the
scope does (TB_ROLL). Below 5 ms/div the display would interpolate 1-2
samples per column (sinc.rs, UF-41) with nothing new to show: the AD2 band
ends there anyway (3.2). Decimation for the slow timebases must come from the
chip (OSR2 / LPF ladder) or per-column min/max of the full stream, never from
dropping samples (aliasing).

### 3.2 Bandwidth and anti-aliasing (INFERRED)

The AD2 decimator is a comb (sinc) filter of order 3 (the block diagram label
"Comb Filter ³", datasheet 13.2). For decimation N from the 2.4576 MHz
modulator rate, |H| = (sin(pi f N / fm) / (N sin(pi f / fm)))^3: -3 dB at
~0.26 of the output rate, -12 dB at half the output rate, nulls at the output
rate and its multiples, i.e. the folding bands are attenuated (> 50 dB at an
offset of 10 % of fs from the null). So the useful band is **~0.25 x output
rate**: 2.5 kHz at 9.6 kS/s, 1.2 kHz at 4.8 kS/s, 20 kHz at 76.8 kS/s (the
stock AC rows). Above the AD2 band the DMM front end limits anyway: the manual
specifies "AC response 10 kHz" (VERIFIED, manual table 1.1), the divider has
the ACC compensation array for AC V, and the 0.3 s coupling time constant sets
the low end of the AC V path at ~0.5 Hz. The real scope channel: 10 MHz, 50
MS/s, 10 bit, 1 Mohm (manual 1.2). The DMM scope is three to four decades
slower and 5-9 bits finer.

### 3.3 Vertical ranges, scaling, accuracy

Full scale at the AD2 node is +-1.25 V / IG (1.1). With the stock rows (IG 1.0,
R25 0x40; r0 rows 0x43 = IG 1.0 with OPS1/OPS2 x10):

| DMM function / range | node gain | AD2 window at the jack | code size at the jack |
|---|---|---|---|
| V 4 V (tap /10) | 1/10 | +-12.5 V peak | 47.7 uV |
| V 40 / 400 / 1000 V | 1/100 .. 1/10000 | +-125 / +-1250 / (+-12 500) V | 0.48 / 4.8 / 48 mV |
| mV 40 / 400 mV | x10 / x1 | +-125 mV / +-1.25 V | 0.48 / 4.8 uV |
| A 4 A / 10 A (10 mohm, x10 / x1) | 0.1 / 0.01 V/A | +-12.5 A / +-125 A (fuse 10 A) | 48 / 480 uA |
| mA 40 / 400 mA (0.5 ohm) | 5 / 0.5 V/A | +-250 mA / +-2.5 A | 0.95 / 9.5 uA |
| uA 400 / 4000 uA (47.5 ohm) | 475 / 47.5 V/A | +-2.6 / +-26 mA | 10 / 100 nA |

Headroom above the DMM range is ~3x (peak), enough for crest factors of
mains / PWM waveforms; the DMM autorange (which watches the RMS) can keep
working in parallel (3.4) and the scope view follows the range.

Scaling: `V = code * 1.25 / 2^18 / IG * k` with the stock range factor k,
exactly the LPF formula of dmm_v2_6.h; the stock calibration record holds a
**gain and offset for the LPF (DC mean of AD2) of every AC row** (0x64/0x74 V,
0x84/0x8c mV, 0xf4/0x10c A, VERIFIED hy3131_v2_6.h), i.e. a calibrated DC
scale of the AD2 path for the DC-coupled rows (mV, A) - apply it to the raw
samples. For the AC-coupled V row the RMS gain (0x34/0x44) is the AC scale.
The new DC-coupled V row has no stock calibration: self-calibrate against AD1
(both converters can see the same tap; the mean of AD2 over an AD1 conversion
must equal the AD1 reading; one-shot at mode entry) - UNKNOWN until 6.5.
Expected accuracy: +-0.5 % of the DMM range after that (AD2 gain drift 10-30
ppm/C, OP1 offset 2 mV without chopper = 20 mV at the 4 V range: use OP1CHOP
as the stock AC rows do, or subtract the zero), vs the scope channel's 10-bit
+-3 %.

### 3.4 Isolation (the plus) and conflicts

* The DMM side is the isolated domain: HY3131, reference, divider, shunts and
  the banana jacks sit behind a CA-IS3741 digital isolator and VPS8504B
  isolated supplies (BOM in `dm40-feature-request.md` 1.1/1.2, INFERRED there;
  the MCU, USB, scope ADC and MCX share the battery ground). Hence a
  **floating, 1000 V / 10 A waveform channel**: mains-side voltage waveforms,
  current waveforms through the A shunt (half-wave rectifier draw, PWM motor
  current, inrush) with the USB cable connected - none of which the MCX scope
  input (40 Vrms CAT II at x1, shared ground) can do. Current waveforms are the
  strongest case: the scope has no current input at all.
* DMM readings: in the **DC rows they need not stop** - AD1 converts
  independently of a raw AD2 read. In the **AC rows only partly**: the RMS/LPF
  chain is clocked by the same OSR2, so streaming at OSR2 = 128 .. 1024 slows
  the RMS/LPF cadence 4-32x (106 ms -> 0.4-3.4 s) and moves the comb response
  the stock calibration was taken at; keeping OSR2 = 32 preserves the AC
  reading but the readout cannot follow 76.8 kS/s. So in the Wave view either
  pause the AC reading (the first cut below) or compute RMS / mean from the
  record in software. Note also that AD2F is set by AD3 conversions too (13.2
  "in common with AD2"): with both converters on, the flag / IRQ paces at
  both. Rules: enable AD2 with the row (note 7),
  never toggle it during the mode; INTF is cleared by every read, so the
  streaming reader must keep a **shadow of the flags it clears** (AD1F, RMSF,
  LPFF, CTF) for dmm.rs's poll, and dmm.rs's own frames must go through the
  same bus owner (queued and executed between two sample reads: a 20-byte AC
  poll is ~60 us on hardware SPI, i.e. one 104 us period at 9.6 kS/s has room
  for it only at 6.25 MHz; at 3.125 MHz accept one dropped sample per DMM
  poll or run the DMM poll at the 2-frame granularity). Simplest first cut:
  pause dmm.rs while the waveform view is live and show the last reading.
* Row / relay switching: every range or function change is the stock sequence
  (quiesce R26/R34/R36, 10 ms relay pulse, 22-byte row, settle 300-900 ms;
  dmm.rs `write_config`) - a visible gap of ~0.5-1 s in the waveform, like the
  DMM's own blanking. Autorange while streaming should be off or confirmed by
  a tap (the scope-page V/div buttons map to the DMM range).
* Functions with no waveform: ohm, continuity, diode, capacitance, frequency,
  temperature. The mode is offered for V, mV, A (DC or AC sub-mode).
* AC V row: the AC coupling means the trace has no DC and a 0.3 s settling
  after any switch (acdc 6a); label the channel "AC" and offer the DC row.

## 4. Data logger via AD2

| | AD1 today (logger.rs) | AD2 |
|---|---|---|
| rates | 40 / 80 / 160 / 320 / 640 S/s nominal (chopped 2048 .. unchopped 256); 38 / ~80 / 318 / 634 measured | 600, 1 200, 2 400, 4 800, 9 600 (19 200) S/s, section 2.3 limits |
| resolution | 24 bit, ~16 ENOB at 640 S/s, chopper-cancelled offset, 2.5 ppm/C | 19 bit, 14-16 ENOB (1.2), offset via ENCHOPAD2 (halves the rate) or OP1CHOP, 10-30 ppm/C |
| functions | DC V / mV / A (AD1 rows) | mV, A directly (DC-coupled AC rows), V with the new DC row; AC-coupled V with the stock row |
| gaps | none below 640 S/s (main loop 30 us typical) when nothing stalls | every stall drops samples until the ISR variant; marked by the seq / timestamp scheme (logger.rs `missed`, `overwritten`), as the FR-38 design already requires |
| ring | 2048 x 12 B = 24 KiB = 51 s at 40 S/s | 213 ms at 9.6 kS/s: size per rate; 16 K x 4 B (24-bit sample + seq byte) = 64 KiB = 1.7 s at 9.6 kS/s covers a 400 ms sector erase |
| USBTMC | 480 B/s | 115 kB/s at 9.6 kS/s with 12 B records, 38 kB/s packed 4 B: fine for full-speed bulk (`LOG:DATA?` blocks of 511 records) |
| BT 115200 | fine | 11.5 kB/s -> ~1 kS/s packed, 2.4 kS/s with 16-bit deltas: BT is the limit, not the chip |
| CSV to the SPI disk | works at 40 S/s (csvlog.rs slices) | text at ~20 B/sample x 9.6 kS/s = 190 kB/s exceeds the flash's erase-limited ~90 kB/s and the erases stall the loop: disk logging only at <= 1 kS/s, binary, with the big ring |

Noise trade: at 2.4 kS/s AD2 gives ~15-16 bits per sample; averaging 4 samples
on the host matches AD1's 640 S/s figure, so (B) only pays off for the rates
AD1 cannot do (> 640 S/s) and for the AC-coupled / current paths. Accuracy:
the stock LPF calibration applies to mV and A; the V DC row is uncalibrated
until the AD1 self-cal (3.3). Plumbing: logger.rs already has OSR / chop /
preamble / ring / drain; it needs a source enum (AD1 | AD2), per-function row
selection (AD2 rows), the 3-byte vs 4-byte sample, and the `LOG:SOURce`
command plus `dm40scpi.py` scaling. Low effort once the readout (section 5.2)
exists; without it, a main-loop AD2 logger at 600-2 400 S/s with gaps is a
one-day change and is the natural first device experiment.

## 5. Recommendation

### 5.1 Go / no-go

* **(B) GO.** Build it first, on the existing logger: it is the cheapest way to
  measure everything (A) depends on (torn reads, loss per rate, noise per OSR,
  the DC V row) and is useful on its own (current / mV logging at kS/s to the
  host).
* **(A) GO as a slow floating scope**, scoped honestly: 5-10 ms/div .. 50 s/div,
  ~1-2.5 kHz bandwidth, 14+ bits, V / mV / A on the isolated jacks, with the
  stock-scope rendering, trigger, roll, stop-and-zoom and cursors; **not** a
  replacement for the MCX channel and not "1 ms/div". Condition: the gap-free
  readout of 5.2 (an ISR), because a waveform display with a hole at every
  redraw is worse than no waveform. Decide on the ISR policy before UI work.

### 5.2 Architecture

```
HY3131 ----SPI2 (PC10/11/12, 3.125 MHz) + CS PD2----> hy3131 bus owner (ISR or main loop)
   |  AD2F / IRQ on DO (EXTI11) or TIMER at ~1.05 x rate                 |
   |                                                                     v
   |   INTF shadow (AD1F RMSF LPFF CTF) <--- dmm.rs poll via request queue
   |                                                                     |
   +--> sample ring (i32 or u16, 8-16 K) --> wavesrc (DMM) --+--> scope_page / scope_ui
                                                            |      (Frame.data u16, fs, trig)
   3PA1030 DMA ring (unchanged) ------------------------> scope.rs ----+
                                                            +--> logger.rs (LOG:SOURce AD2)
```

1. **hy3131.rs**: a `Bus` with two back ends (bit-bang as today, SPI2 with the
   one-bit shift), a single owner, a request queue for dmm.rs's frames, and
   the INTF shadow. The DMM path keeps its semantics (`read1(R_INTF)` returns
   shadow | live).
2. **One ISR** (TIMER or EXTI11 on the HY3131 IRQ): flag check, 5-byte AD2
   read, push to the ring, run one queued DMM frame if the period allows.
   Policy change for fw-rs ("no ISRs"): keep it the only one, document the
   shared state (SPSC ring, shadow byte, queue), keep `cpsid` everywhere else.
3. **Source abstraction for the scope**: `Frame.data: &[u16]` is what
   scope_ui renders; AD2 19-bit -> u16 (>> 3, offset binary) loses nothing
   visible (280 rows). Add a `WaveSource` behind `Scope::poll` with its own
   `fs`, ring (not the scope's BUF: DMM and scope stay simultaneous),
   `volts(code)` (3.3 scaling) and timebase table (3.1). The trigger search
   (`stream_poll` / find_trigger with hysteresis) is written for a DMA ring
   position; generalise its ring accessor. Roll mode and stop-and-zoom reuse
   the existing code once the record is a `[u16]`.
4. **UI**: in the DMM page a **"Wave" view** (swipe / MF menu entry next to
   the strip chart) that opens the stock-scope page with the DMM source: the
   top bar shows the DMM function and range instead of CH1 V/div (V/div +/-
   step the DMM range; the "AC/DC" coupling pill selects the AC V row vs the
   DC row), the trigger, timebase, run/stop, cursors and measurements as the
   scope (RMS / mean / freq from the record at 14 bits are better than the
   MCX channel's). In the scope page a **source badge "MCX | DMM"** opens the
   same view, so the owner's "DMM input source in the scope page" and the
   "Wave view in the DMM" are one screen reached from both sides. The DMM
   reading (last value) stays in the top strip as the combined view does.
   Overlap check and contact sheet before any screen (memory rule).
5. **Logger**: `LOG:SOURce AD1|AD2`, row selection, 4-byte records option,
   `dm40scpi.py` scaling from the preamble.

### 5.3 Effort

| step | effort | depends on |
|---|---|---|
| E0 device experiment plan (section 6), through `DBG:` commands and the logger | 1 day + device | - |
| E1 main-loop AD2 logger (bit-bang, gaps marked), `LOG:SOURce` | 1 day | E0 |
| E2 SPI2 back end with the shift trick, read-back self-test, speed probe | 1-2 days | E0 |
| E3 bus owner + INTF shadow + request queue; dmm.rs on it | 2 days | E2 |
| E4 the ISR (policy decision), ring, loss statistics | 1-2 days | E3 |
| E5 `WaveSource` + scope.rs source abstraction + trigger generalisation | 3 days | E4 |
| E6 UI: Wave view / source badge, range-as-V/div, coupling pill, mockups + overlap check | 3 days | E5 |
| E7 DC-coupled V row + AD1 self-cal + autorange handling | 1-2 days + device | E0 (6.5) |
| total | ~2.5-3 weeks for (A) incl. (B); (B) alone E0-E1 2 days (E2-E4 upgrade it later) | |

### 5.4 Risks

1. Torn reads (1.4) unresolvable at the wanted OSR -> cap the rate at the OSR
   where the AD2F-synchronised read fits (probably 9.6 kS/s). Low risk for
   the scoped feature, fatal only for the "1 ms/div" ambition, already excluded.
2. The ISR policy: fw-rs's single-threaded design is a deliberate simplicity
   and safety property (prof.rs, review docs). Mitigation: one ISR, one SPSC
   ring, no other shared state; the whole streaming path can be compiled out.
3. SPI2 across the isolator at 3.125 MHz fails read-back -> 1.56 MHz
   (frames 2x longer, ceiling 4.8-9.6 kS/s). Still a GO.
4. AD2 on OP1O in the DC V row (E7) shows an offset / noise that the AD1
   self-cal cannot remove (OP1 common-mode, note 7 interactions) -> V stays
   AC-coupled (stock row), DC V waveforms limited to the 4 V range through
   r0's OP1 path (already OP1O - RLU on AD1; AD2 on OP1O - RLU is the same
   node) - UNKNOWN until 6.5.
5. CPU share at 9.6 kS/s (~20 %) lengthens the LCD redraw stalls the owner
   already complains about (scope drag lag): keep the default at 4.8 kS/s and
   let the timebase pick the OSR (slower timebases -> higher OSR -> less
   CPU and more bits).
6. The DMM poll competing for the bus during streaming: first cut pauses the
   DMM reading in the Wave view (the stock scope page has no DMM either);
   simultaneous reading is E3's queue.

## 6. Device experiment plan (one session, before any UI work)

All through `DBG:` SCPI on fw-rs; a calibrator / signal generator on the
jacks; log with `dm40scpi.py`.

1. **SPI2 pins and speed** (E2 prototype as a DBG command): configure
   PC10/11/12 AF6, read R20..R35 back at 1.56 / 3.125 / 6.25 MHz and compare
   with the row just written bit-banged; 1000 frames each, count mismatches.
   Settles: AF mapping (INFERRED -> VERIFIED), the isolator-limited clock.
2. **Torn reads**: DC-coupled row (AC mA r1 or AC mV r1, DC source so that the
   code sits near a byte boundary, e.g. 0x00FF80 +- noise, and a slow ramp),
   AD2 alone (R34 = 0, R29 = 0), OSR2 = 1024 .. 32: 10 000 blind reads per
   OSR, histogram of |x[n] - x[n-1]| -> outliers of 256 / 65536 codes are
   tears; repeat flag-synchronised (INTF then data). Settles 1.4 and the
   OSR floor of every scheme.
3. **Loss per rate**: logger session at 600 / 2 400 / 9 600 S/s with the
   DMM page redrawing, then with the display frozen: `missed`, `overwritten`,
   measured rate vs nominal (the logger already reports them). Settles the
   section 2.3 table on the real loop.
4. **Noise / ENOB**: input shorted (mV) and 0 A, 1 s per OSR2 at 76.8 kS/s ..
   600 S/s: sd of the samples in codes -> ENOB per rung (1.2). Also with
   ENCHOPAD2 and with OP1CHOP on/off.
5. **DC-coupled V row**: AC V r1 bytes + R32 0x38 + R27 SAD2IP = OP1O (two
   variants: AD2IN = AGND, = RLU), 10 V DC and 5 V 50 Hz: mean of AD2 vs the
   DC V reading (AD1 in the same row, r1 = SENSE - PB5), ripple, and the AD1
   reading with AD2 on vs off (note 7). Settles 1.3 last row and E7.
6. **Waveform sanity**: AC A r0 with a 40 Hz PWM load (the calibrator used in
   acdc doc 10): record 1 s at 4.8 kS/s to the host, plot: edges, duty,
   amplitude vs the DMM AC A reading.

Total: a half day with the DBG commands of E0/E1 in place. Outcomes map
directly to the rate table of 2.3 and to the GO conditions of 5.1.
