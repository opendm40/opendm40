# DMM inputs as a scope source (AD2 streaming): feasibility

Date 2026-10-21. Sections 0-6: research (no fw-rs source changed then).
Section 7: (B) implemented on the main loop (E0 + E1), emulator results and
the 30-minute device plan. Evaluates two owner ideas for the TODO:

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
  assignment VERIFIED in the GD32F450xx datasheet Rev1.1 table 5 (p. 29:
  PC10 SPI2_SCK, PC11 SPI2_MISO, PC12 SPI2_MOSI, all AF6; same GD32F4xx pin
  map as the F427), SPI2EN = RCU_APB1EN bit 15, SPI_CTL0 / SPI_STAT layout
  per GD32F4xx UM V2.2 21.11.1 / 21.11.3; implemented, section 8). CS = PD2 stays a GPIO (no hardware-NSS
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
  goes high"). Writes need no shift (7.2 (3)). **Implemented instead**
  (section 8): the 9th clock as one GPIO pulse on PC10 between the SPI
  bytes (the exact datasheet frame, no extra clocks).
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

## 7. (B) implemented: main-loop AD2 logger + the E0 hooks (2026-10-21)

Status: E0 (the DBG hooks of section 6, steps 2-4) and E1 (main-loop AD2
logger, bit-banged, gaps marked) are in fw-rs. No ISR, no SPI2 (E2-E4 wait for
the owner's ISR decision and the device numbers below). Nothing of it has run
on the device yet; every number in 7.2 is from the Renode emulator.

### 7.1 What exists

| piece | where | what |
|---|---|---|
| rate ladder | `hy3131::ad2_r26_for`, `ad2_period_ns` | requested S/s -> R26 low bits. Chop ON (default, `LOG:CHOP`): 600 = /4 OSR 1024, 1200 / 2400 / 4800 / 9600 = /2 OSR 1024 / 512 / 256 / 128, all with ENCHOPAD2. Chop OFF: 1200 = /4 OSR 1024, 2400..9600 = /2 OSR 1024..256 (600 needs the chopper). Nearest rung in log scale; the achieved rung is reported, never silently another rate. Tested against the 1.1 table (meter-test). |
| stream setup | `hy3131::set_ad2_stream` | after the full stock row (dmm.start): ENAD1 cleared if set (the misconnect sentinel runs AD1 in AC V / mV; off first, so note 7 cannot apply), R34 = 0 (AD3 off: the row starts it before AD2, so with both on AD2F would come twice per period on two skewed grids), R29 = 0 (RMS / LPF off), R26 = ENAD2 \| rung, INTF cleared. ENAD2 never toggled. LOG:STATe OFF rewrites the row, sentinel included (unchanged path). |
| rows | `Logger::mode_ok` | AD2: AC mV (2) and AC A / mA / uA (4..6) **DC-coupled** (OP2O / OP1O, stock rows unchanged), AC V (0) **AC-coupled** (PB4, ~0.5 Hz; preamble `coupling` = 1). AC+DC (16/17) refused (alternating rows). The DC-coupled V row (1.3 last row) is **not implemented: needs device test 6.5**. |
| sampling | `Logger::poll` | AD2F-gated (INTF read, then R03..R05), P/2 gate from the last sample, timestamp at the flag observation. Since one sample costs ~122 us bit-banged and the period is 104..1667 us, a call keeps sampling while the next result is due within `AD2_BURST_US` = 500 us of the call. A read whose bits 23:19 are not a copy of bit 18 is dropped and counted (`bad`, a torn read for sure). |
| honesty | LOG:COUNt?, LOG:GAPs?, LOG:PREamble?, panel | `lost` = round(span / nominal period) + 1 - captured (counts the steady "slightly late" losses of a rate the link cannot sustain, which the per-gap `missed` cannot); gap histogram in periods (2, 3-4, 5-16, 17-128, >128) + max gap; preamble `...,source,req_hz,nominal_mhz,r26,coupling` (requested vs chip rate; measured_hz in field 5); the on-screen panel shows "AD2 measured/nominal Sa/s", samples and "lost N (x.x%)" (ui-render checked, no overlaps). |
| ring | `logger.rs` | unchanged: 2048 records of 8 B (seq implicit, d957027), 16 KiB of SRAM (+40 B of stats). Only 213 ms at 9600/s: the host must drain `LOG:DATA?` (511 records per block) at least ~5x/s, else `overwritten` counts the loss; a bigger ring does not fit (SRAM free ~4.5 KB). Wire format unchanged (12-byte {seq, ts_us, raw}). |
| hooks (E0) | `src/ad2exp.rs`, `DBG:AD2:*` | `TORN? r26,n[,SYNC]`: n back-to-back pairs of R03..R05 reads, INTF after each pair; same / differ-with-new-conversion / differ-without / signbad + a histogram of bit_length(\|a-b\|) (tears show as a bump at 9 or 17 above the noise). `NOISE? r26,n[,op1chop]`: n flag-synced samples: mean, sd (1/1000 code), min, max, readout us; ENOB = log2(2^19/sd) on the host. `DRAW 0\|1\|2`: during a log session no panel / the panel / panel + a full DMM-view repaint every 100 ms (the DMM page's redraw load). Bounded (n <= 2000 per call, watchdog kicked), refused while logging or outside an AC row (no reply: `dm40scpi.py ad2` checks FUNC? first), the row is rewritten afterwards. |
| host | `tools/dm40scpi.py` | `log out.csv --source ad2 --rate N [--chop off] -d S`: CSV `seq,t_s,raw,value_<unit>,lost_before` (scaled with IG 1.0, VR 1.25 V and the stock range factor, calibration NOT applied, said in the header; lost_before from the timestamps and the seq), trailer with measured rate, counts and gaps; `ad2 torn\|noise [--sync] [--op1chop k] [--rungs ...]` sweeps the rungs. `--usbip BUSID` talks to the emulator. |
| emulator | `emu/hy3131/HY3131.cs` (+ peripherals copy), tests | AD2 on its own grid (one AD2F and one fresh result per period; before: the flag was set and the code resampled on every bus access after the first period), per-result noise model, the logger's row variant still recognised. `emu/hy3131/tests`: `Ad2Stream` (960 conversions in 100 ms at 9600/s, one flag each, code of 12.3 mA in AC mA r0, noise model). |

### 7.2 Emulator results (plumbing only, not the device)

`emu/run.py fwrs --fast` (50 MIPS: a quarter of the device's 200 MHz, so the
bit-banged readout costs ~4x the device's) with `isrc:0.5,ac=0.3,f=50`, AC A
(the meter picked mA r1), 15-20 s per run, USB/IP draining LOG:DATA?:

| requested | chip | measured | lost | gaps (2 / 3-4 / 5-16 / 17-128 / >128), max |
|---|---|---|---|---|
| 600 | 600.0 | 593 | 1.1 % | 0 / 51 / 0 / 0 / 0, 6.5 ms |
| 2400 | 2400.0 | 2030 | 15.4 % | 5616 / 7 / 96 / 0 / 0, 6.1 ms |
| 9600, panel (DRAW 1) | 9600.0 | 2011 | 79 % | 0 / 10913 / 19660 / 48 / 0, 6.0 ms |
| 9600, no panel (DRAW 0) | 9600.0 | 2055 | 79 % | 0 / 11514 / 19493 / 0 / 0, 1.1 ms |
| 9600, DMM-view repaint every 100 ms (DRAW 2) | 9600.0 | 685 | 93 % | 0 / 2182 / 3095 / 333 / 83, 47 ms |

`ad2 noise` (DC 50 mA, the emulator's noise model): sd 31.5 codes at 76.8
kS/s (ENOB 14.0) .. 8.3 at 600/s (16.0); `ad2 torn --sync`: signbad 0, no
difference without a conversion (the model latches by construction). Readout
426 us per flag-synced sample at 50 MIPS. What this shows: the AD2 path,
the ladder, the counters and the host CSV work end to end, the counters
expose loss honestly, and the repaint costs whole tens of ms. The rates are
the emulator's, not the device's: at 200 MHz expect ~4x the readout speed
(~8 kS/s link ceiling, section 2.1).

### 7.3 Device test plan (30 minutes)

Meter on fw-rs with this build, USB to the PC (`tools/dm40scpi.py`, USBTMC);
a DC source and a signal generator; write the outputs into a new section of
this doc. Step 1 is the go/no-go for every higher rate.

| min | step | commands | record / pass |
|---|---|---|---|
| 0-3 | setup | AC mV on the meter (V key -> mV, AC; `dm40scpi.py info`: FUNC? VOLT:AC); DC 0.312 V into V-COM (AD2 code ~0x00FF80, a byte boundary at 400 mV x1) | - |
| 3-8 | torn reads (6.2) | `dm40scpi.py ad2 torn -n 2000` then `... --sync` | per rung: signbad, diff(no conv), the bits histogram. Pass = signbad 0 and no bump at 9 / 17 beyond the noise. A bump at a rung = the rate cap of every flag-synced scheme (1.4) |
| 8-18 | loss per rate (6.3) | for r in 600 2400 4800 9600, for d in 1 0 2: `dm40scpi.py scpi "DBG:AD2:DRAW $d"; dm40scpi.py log l$r-$d.csv --source ad2 --rate $r -d 20` (then DRAW 1) | the trailer line: measured S/s, lost %, gaps. Fills the 2.3 table with the real loop: expected loss-free at 600-2400 with DRAW 0 / 1, the link ceiling (~8 kS/s) at 9600 |
| 18-23 | noise / ENOB (6.4) | V-COM shorted (mV): `dm40scpi.py ad2 noise -n 2000`; AC A, jacks open: `ad2 noise -n 2000 --op1chop 0` and `--op1chop 3` | sd / ENOB per rung (compare 1.2: 14-16 bits; the emulator's model is 14.0 .. 16.0) |
| 23-28 | waveform (6.6) | AC mV, generator 50 Hz 100 mV rms + 50 mV DC: `log w.csv --source ad2 --rate 4800 -d 2`; AC V, 5 V 50 Hz: `log v.csv --source ad2 --rate 2400 -d 2` | plot value vs t_s: amplitude and mean vs the DMM's AC mV / DC mV readings (calibration not applied: expect < 1 %); AC V: no DC (coupling AC in the header) |
| 28-30 | profile | during one 9600 session: `dm40scpi.py scpi "DBG:PROF?"` | the longest main-loop passes while logging (what still stalls) |

Not covered by this plan: the DC-coupled V row (6.5, E7), which needs code
that is not written yet. SPI2 (6.1, E2): section 8.

## 8. E2 implemented: the HY3131 on the hardware SPI2 (2026-10-22)

Owner decision: no runtime switch between bit-bang and SPI2; if the hardware
bus is reliable it is simply the bus. fw-rs now talks to the HY3131 over SPI2
only; the bit-banged bus survived as a compile-time fallback (`--features
hyspi-bitbang`) until the device check passed (8.6, 7889e6c); the feature and
the bit-banged read path are gone since (bit-banged writes stay, 8.6).

### 8.1 What exists

| piece | where | what |
|---|---|---|
| bus | `src/hy3131.rs` (`init_pins`, `spi::read` / `write`) | SPI2 @ 0x4000_3C00, PCLK1 50 MHz / 16 = **3.125 MHz**, master, mode 0 (CKPL = CKPH = 0), MSB first, 8 bit, software NSS; PC10 / PC11 / PC12 AF6 (VERIFIED, 2.2), OSPEED 25 MHz on SCK / MOSI; CS = PD2 GPIO. `read` / `write` keep their signatures, so the DMM, the AD2 logger, ad2exp and the sentinel all use it unchanged. Polled byte exchange (TBE, DATA, RBNE), TRANS idle before CS goes high. No buffers, no ISR. |
| read period | `spi::read` | command byte on SPI2; wait TRANS = 0 (SCK low); PC10 MODER AF -> output (ODR bit = 0, the SPI idle level: no edge), one pulse (~100 ns high, ~100 ns low), back to AF; then the n data bytes (MOSI = 1 as on the bit-banged bus). The datasheet frame exactly (8 + 1 + 8n clocks), so the register after the frame sees what it saw before (its MSB fetched at the last falling edge). The "shift" alternative (cmd + n + 1 bytes, RX shifted one bit) is not used: it clocks 6 more bits of reg + n, which after the counter frame (R15..R1D) is R1E = INTF, clear-on-read. |
| pin hand-over | `init_pins` | CS high first, SCK low as GPIO, SPI2 configured and enabled (drives SCK low), then AFRH, then MODER: SCK never moves while CS is high, CS never glitches (and the chip ignores CK while CS is high, DS 7.2 (6)). Only at init; no switching back at run time. |
| CS timing | `spi::{begin,end}` | CS low -> first edge ~200 ns, last edge -> CS high ~100 ns, CS high >= ~500 ns between frames (the bit-banged bus had ~1 us; the datasheet gives no minimum). |
| self-test | `src/hyspi.rs`, `DBG:HYSPI:SELF? n[,div]` | in the current row (run it in an AC row: AD2 on): reference = first read of R1F..R35 (INTE + the row: the HY3131 has no ID / version register); n x {read1(R20), read(R00,6) AD2 sign copy, read(R1F,23) = ref, write(R36, pattern) + read1(R36) = pattern}; reply `div,n,bad_r1,bad_r23,bad_w1,signbad,us_r1,us_r6,us_r23,us_w1`. `div` 8 / 16 / 32 runs that call at 6.25 / 3.125 / 1.5625 MHz (the margin probe), then back to /16. The write-pattern check is absolute (a consistently shifted bus fails it even though the reference would shift too). DMM paused and its row rewritten afterwards, refused while logging, n <= 2000. In the fallback build the same test runs on the bit-banged bus (div 0). |
| fallback | Cargo feature `hyspi-bitbang` (removed after 8.6) | the old bit-banged bus, nothing else changed. |
| emulator | `emu/hy3131/HY3131.cs` (= `emu/peripherals/HY3131.cs`), `emu/dm40.repl` | `HY3131SpiPort` (ISPIPeripheral) on Renode's STM32SPI `spi3` @ 0x40003C00 hands each byte to `HY3131.SpiByte`, which runs the existing bit engine for 8 mode-0 clocks (sample DO, rising, falling); the GPIO pulse on PC10 lands in the same engine. Host tests (`emu/hy3131/tests`, "Hardware SPI byte path"): R1F..R35, R36 and live R00..R05 equal on bit-bang / SPI-pulse / SPI-shift, 256 SPI writes read back bit-banged, a 22-byte row written over SPI, INTF side effects of the frame tails, a byte with CS high is ignored. |

### 8.2 Timing budget (why /16)

Mode 0: the master changes MOSI and the chip changes DO after the falling
edge; both sample at the rising edge, half a period later. The SPI crosses
a CA-IS3741 isolator (~10-15 ns per direction, INFERRED, not from its
datasheet).

| | /8 = 6.25 MHz (80 ns half) | **/16 = 3.125 MHz (160 ns)** | /32 = 1.5625 MHz (320 ns) |
|---|---|---|---|
| DO path: SCK out through the isolator + TDODL <= 60 ns + back = ~80-90 ns, vs the half period | **-0 .. -10 ns** (fails or marginal) | **+70 ns** | +230 ns |
| DI setup TDISET >= 60 ns (+ channel skew, few ns) | +15 ns | +95 ns | +255 ns |
| TCKLO >= 60 / TCKHI >= 20 ns | ok | ok | ok |
| byte | 1.28 us | 2.56 us | 5.12 us |

So /16 is the bus; /8 is only probed (SELF? n,8) to see the real margin;
/32 is the step back if /16 shows any error on the device.

### 8.3 Emulator results (plumbing only, not the device)

`emu/run.py fwrs --fast` (50 MIPS), DC V 1.2 V / AC V 0.5 V 50 Hz, both
images built from this tree:

| | SPI2 build | `hyspi-bitbang` build |
|---|---|---|
| READ? DC V / AC V | 1.19999 / 0.49990 | 1.20000 / 0.49995 |
| SELF? 200 (AC V) at 16 / 8 / 32: bad_r1, bad_r23, bad_w1, signbad | 0 on all | 0 |
| us per frame r1 / r6 / r23 / w1 | 5 / 7 / 11 / 6 (Renode quantum noise) | 156 / 435 / 1381 / 178 |
| AD2 logger 2400 S/s, 10 s: measured, lost | 2364 S/s, 1.5 % | 2017 S/s, 15.9 % |
| AD2 logger 9600 S/s, 10 s: measured, lost | 8806 S/s, 8.3 % | 2010 S/s, 79 % |

The SPI bytes are instantaneous in Renode, so the SPI2 us are CPU overhead
under the emulator's quantum, not bus time; the bit-banged numbers scale ~4x
down on the device (200 MHz). Device expectation from 2.2: flag check ~8 us,
3-byte AD2 read ~13 us, the 20-byte AC poll ~60 us at 3.125 MHz, i.e. SELF?
us_r1 ~ 6-8, us_r6 ~ 19-21, us_r23 ~ 62-66, us_w1 ~ 6-7.

### 8.4 Device check (10 minutes)

One build: the SPI2 image (this tree, `./build.sh native`). Meter on USB,
`tools/dm40scpi.py`; a DC source (1-10 V) and a 50 Hz sine (a few V) for V-COM.

| min | step | commands | pass |
|---|---|---|---|
| 0-1 | boots, DMM reads | DC V, 5 V in: `dm40scpi.py scpi "READ?"` x3 | the source's value (compare the last bit-banged session's reading of the same source) |
| 1-4 | bus self-test | AC V, sine in: `scpi "DBG:HYSPI:SELF? 2000"`, then `"DBG:HYSPI:SELF? 2000,32"`, then `"DBG:HYSPI:SELF? 2000,8"` | /16 and /32: every bad_* and signbad = 0, us_r1 ~6-8, us_r6 ~19-21 (8.3). /8: record the counts (errors there are the expected margin answer, not a failure) |
| 4-6 | DMM functions | DC V, AC V (sine), ohm (a resistor), FREQ (the sine: the 9-byte counter frame) - `READ?` each | readings as on the bit-bang firmware, no flicker / no stuck reading |
| 6-9 | logger | AC mV or AC V: `dm40scpi.py log s9.csv --source ad2 --rate 9600 -d 10` and `--rate 2400` | lost well below the bit-bang emulator figures; signbad (`bad`) 0 |
| 9-10 | profile | `scpi "DBG:PROF?"` on the DMM page | the DMM poll no longer near the top |

If step 1 or 2 fails at /16 and /32 (bad_w1 > 0, nonsense readings), flash
the fallback build `./build.sh native --features hyspi-bitbang` (identical
otherwise) to keep the meter usable, and report the SELF? lines: bad_w1 with
good bad_r23 points at the read-period pulse (PULSE_LO / CS_SETUP in
hy3131.rs), all bad at every divider at the pin mux / AF, errors only at /16
at the isolator margin (then make /32 the bus). On a pass: remove the
fallback feature and the bit-banged code.


### 8.5 Device result of 8e5efa6 and the DIAG image (2026-10-22)

Calibrator 1.000 V DC on V/COM. SPI2 build: DC V READ? = -0.0002 V (wrong);
`SELF? 2000` -> `16,2000,0,1024,508,0,7.71,21.38,68.33,7.26`; `,32` ->
`32,2000,0,965,505,0,12.57,...`; `,8` -> `8,2000,65,1745,626,0,5.31,...`.
Bit-bang build of the same commit: DC V 0.99978 V; `SELF? 500` ->
`0,500,0,257,0,0,51.87,144.00,457.66,58.72`.

Reading it:
- r1 (1-byte read of a constant row byte, pulse framing) is clean at /16 and
  /32: mode 0, AF6, the GPIO read-period pulse and the data-phase sampling
  work. /8: 65 errors = the DO-path margin of 8.2, as predicted.
- us_r1 7.71 at /16 = 16 clocks x 320 ns + ~2.6 us, 12.57 at /32 = 16 x 640
  + ~2.3 us: APB1 is 50 MHz and the PSC coding is right.
- w1 (write R36, read back) fails ~25 % at /16 AND /32: independent of the
  bit period -> a fixed-time quantity at a frame boundary, not SPI edge
  timing. Candidates: CS hold after the last clock (~100 ns), CS high
  between frames (~500 ns), CS setup (~200 ns); the bit-banged bus had ~1 us
  for each. The HY3131 runs on a 4.9152 MHz SYSCLK (203 ns): a write commit
  or CS sampling synchronised to it and lost on a per-frame phase basis
  gives exactly a divider-independent fraction. DC V wrong fits: single-byte
  config writes (R22, R23, INTE, R26 / R34 enables) lost one time in four.
- r23 is unusable as evidence: 51 % "bad" on the bit-banged bus too (the
  reference was one read; bytes of the row change between reads). SELF? now
  takes 8 bit-banged reads and masks live bytes (reported as `..`).

The DIAG image (this commit; REMOVE the DIAG pieces before the final bus):
every bus parameter is runtime (`DBG:HYSPI:DIAG div,cpol,cpha,frame,setup,
hold,idle,phi,plo,ospeed`, `DIAG?` also prints DWT cycles of spin(1000)),
`DBG:HYSPI:RAW?` (256 values into R36 three ways: A SPI2 write / bit-bang
read, B bit-bang write / SPI2 read, C SPI2 both; each mismatch with the
previous value written, so `got = previous` = a lost write), SELF? on the
current parameters with the masked reference. Every scratch write is
followed by an empty write frame to R00 so a missed CS high could only put
0x00 into R37 (test mode). Defaults = 8e5efa6's constants (the first run
reproduces); frame 2 = bit-bang at run time for comparison.

First device runs (in this order): `DIAG?` (spin calibration); `RAW?` with
the defaults (classify: A failures = write side, B = read side, got =
previous = write lost); `DIAG ,,,,40,40,40` (~1 us setup / hold / idle like
bit-bang) then `READ?` on the 1 V calibrator + `SELF? 2000` + `RAW?`; if
that is clean, bring hold and idle down separately (20, 10, 5) to find the
cliff, and set the final constants at >= 2x it. The emulator cannot show
any of this (SPI bytes are instantaneous, the model has no SYSCLK phase).

### 8.6 Device bring-up with the DIAG image fca31fd, and the fix (2026-10-22)

Run over USBTMC on the device, 1.000 V DC calibrator on V/COM.
`DIAG?`: spin(1000) = 5017 cycles, i.e. one spin = 25 ns at 200 MHz.

`RAW?` with 8e5efa6's parameters: `A=064,B=000,C=064`. Every mismatch is
`App>(pp|1)`: exactly the 64 values with bit 7 = 1 and bit 0 = 0, written
over SPI2, arrive with bit 0 = 1 (A: SPI2 write / bit-bang read; C: SPI2
both); B (bit-bang write, SPI2 read) is clean for all 256 values. The same
64 values, unchanged, with: CS setup / hold / idle 8/40/40, 40/4/20 and
40/200/200 spins (0.2 .. 5 us); pin speed OSPEED 0 (2 MHz) and 3 (200 MHz);
/32; the shift framing. CPHA = 1 wrote nothing (`A=255`, READ? overload).
SELF? (masked reference, 23 stable bytes): reads clean at /8, /16 and /32
(bad_r1 = bad_r23 = signbad = 0 in 2000 iterations; us_r1 5.6 / 8.0 / 12.8,
us_r6 13.4 / 21.8 / 38.6, us_r23 40.4 / 69.2 / 126.8), w1 501/2000 bad at
every divider (= the 25 % of patterns with bit 7 = 1, bit 0 = 0). The
SPI2-written row read back as 0091..9128A181.. against the bit-banged
0090..9028A080..: interior bytes of a multi-byte write are hit too (every
transfer ends after its byte, the next one is loaded on RBNE).

Conclusion: the chip takes a data byte's LSB after the GD32 has stopped
holding it on MOSI (the peripheral drives the byte's MSB level there once the
byte is out; a 1 wins). Command bytes always have bit 7 = 0, so reads are
not affected. Not a timing margin: no divider, CS time or edge rate changes
it.

Fix (this commit): reads on SPI2 (/16, pulse framing, the constants of
8e5efa6), writes bit-banged with the three pins switched to GPIO for the
frame (SCK low on both sides of the switch, CS high, SPI2 left enabled and
idle). DIAG / RAW removed. Writes are rare (mode changes, counter restarts,
the power meter's R34 phases), ~25 us per byte. A pure-SPI2 write (keep
TX continuous so no byte ends early, plus a dummy byte with MSB = the last
LSB while CS rises mid-byte) is possible but timing-delicate and untested.

Emulator (plumbing): DC V 0.99999, AC V 0.50000, SELF? 200 clean at /16 and
/32, AD2 logger 9600 S/s 7.1 % lost (bit-bang 79 %).

Device verification of this image (5 min, the meter on USB, 1 V DC on V/COM):
1. `READ?` x3 in DC V: 1.0000 V (the 8e5efa6 SPI2 build read -0.0002).
2. `DBG:HYSPI:SELF? 2000`, `... 2000,32`: every bad_* and signbad = 0
   (w1 included now: writes are bit-banged), 23 stable bytes, us_r1 ~8.
3. AC V and FREQ on a 50 Hz sine (or the calibrator's AC): readings as on the
   bit-bang build; ohm on a resistor.
4. `dm40scpi.py log s.csv --source ad2 --rate 9600 -d 10` in AC V / mV: lost
   well below the bit-banged ~6.5 kS/s ceiling; bad 0.
Fail -> the fallback build `./build.sh native --features hyspi-bitbang`.
(Passed on the device, TODO 2026-10-22; the fallback feature is removed.)

## 9. E3/E4 implemented: the AD2 stream on one timer ISR (2026-10-22)

Decision #3 of docs/decisions-2026-10-22.md, behind the Cargo feature
`ad2-isr` (**off by default** until the device acceptance in 9.5: invariant 1,
the default build has no interrupt and behaves as before, except the ring
policy noted in 9.1). Image for the device: `./build.sh --features ad2-isr`.
Everything below is emulator plumbing; the device numbers are 9.5's job.

### 9.1 What exists

| piece | where | what |
|---|---|---|
| vector | `src/shim.rs` | the table runs to IRQ 55 (72 words, region A still 0x0800460c); IRQ 55 = `ad2_stream_irq` with the feature, `default_handler` without (cfg'd in the asm, no weak symbol: LTO rejects a weak + strong pair in one module). |
| timer | `hy3131::stream` | TIMER6 (STM32 TIM7, 0x4000_1400, APB1EN bit 5; free: TIM1/2/3/5/8 scope + hwtrig, TIM6/12 generator, TIM13/14 backlight / buzzer). 200 MHz kernel, PSC 3 = 50 MHz count, ARR for 4 x the AD2 rate (4800 .. 38400 Hz), URS so UG does not raise UIF, UIE. |
| hand-off | `hy3131::stream::start` / `stop` | start (main loop, after the session's last bus access `set_ad2_stream`): TIM7 set up, SysTick off + PENDSTCLR / PENDSVCLR, ICER and ICPR on all 8 NVIC words (whatever the FS-loader left), IPR[55] = 0, `HANDED` = true, ISER bit 55, `cpsie i`, CEN. stop (main loop): CEN / UIE off, ICER bit 55, `cpsid i`, SR and ICPR bit 55 cleared, `HANDED` = false. PRIMASK is cleared nowhere else. "Wait for a running ISR": nothing to wait for on one core (the main loop runs only between ISR entries, and the ISR always ends its frame: CS high, SPI2 idle). |
| guard | `hy3131::read` / `write` / `set_div` | every main-loop bus call checks `HANDED` and, if set, stops the stream BEFORE its frame: a main-loop frame can never interleave with the ISR's. The decision asked for a debug assertion; one main-loop path legitimately gets here (a function / range change while logging: touch, key, CONF -> dmm.start), which the logger's auto-stop used to catch only after the fact, so it is a runtime guard in every build. `forced_stops` counts (DBG:ISR? last field), `Logger::poll` ends the session and main.rs reports "Settings conflict". |
| ring | `logger::RING` | the same 2048 x 8 B (+ counters, 16468 B in .dmabuf; `Logger` itself is 72 B now). Single producer (the ISR, or the main loop for AD1 / the main-loop AD2 sampler), single consumer (LOG:DATA?): `head` / `tail` u32 atomics, Release / Acquire; every counter has one writer; the 64-bit span is two words read hi-lo-hi. Full ring: the NEWEST sample is dropped (the producer must not move the consumer's tail), `overwritten` counts it, the seqs stay consecutive and the host sees a timestamp gap (before: oldest overwritten, a seq gap). This is the one behaviour change of the default build (AD1 and the main-loop sampler use the same ring). |
| ISR | `logger::isr::ad2_stream_irq` | clear UIF (and read CNT = entry latency); inside the read gate (7/8 P after the last seen conversion) nothing else; else INTF (1-byte SPI2 frame), on AD2F R03..R05 (3 bytes), sign-copy check, `Producer::push` (stats + ring). Status waits capped (`spi::wait`, 2000 polls ~100 us; `timeouts`). No register write, no dmm.rs, no LCD / flash / watchdog, no float, no panic path (checked in the disassembly: callees `spi::read`, `spin`, `Producer::push`, `__aeabi_uldivmod`). |
| counters | `DBG:ISR?` | `feature,active,ticks,captured,empty,gated,dropped,bad,timeouts,overruns,max_isr_ns,max_lat_ns,tick_mhz,forced`: ticks = captured + empty + gated + bad; overruns = a tick already pending at exit; max_isr_ns from DWT; max_lat_ns = TIM7 CNT at entry x 20 ns; timeouts = HY3131 frames abandoned since boot. LOG:COUNt? / GAPs? / PREamble? unchanged. |
| default rate | `logger::DEFAULT_AD2_HZ` | 4800 S/s with the feature (2400 without). |
| stack | `tools/stack-depth/stackdepth.py` | the ISR runs on the MSP on top of any main-loop frame: worst = main path + ISR path (176 B) + exception frame (26 words with the FPU's lazy-stacking space + 4 alignment = 108 B). The tool also follows IT-block calls (`bllo`) and conditional tail calls now, which it missed before: the main path grew by 336 B (a pre-existing undercount). Feature build: 30632 of 48088 B, headroom 17456 (>= 16384 checked by build.sh). |
| SRAM | | +32 B in SRAM (0x2002f250, 3504 B free below 0x20030000), +72 B TCM .bss (the ISR's producer state). |
| emulator | `emu/dm40.repl`, `emu/hy3131/ad2streamtest.py` | timer7 is a `GD32_Timer` (IRQ -> nvic@55) instead of Renode's STM32_Timer; the ISR's SPI2 frames go through `HY3131SpiPort` as the main loop's. The test boots the feature build at 200 MIPS and checks 9.4's sessions plus a function change mid-stream. |

### 9.2 Tick rate and stamps (deviation from "~1.05 x")

The record stamp is the observation time (decision #2: "when the firmware
saw it"). With a tick period T the stamp is late by d in [0, T).

- **1.05 x** (T = 0.95 P), measured first: d walks down by P - T per sample
  and wraps every ~21 samples (one empty tick), so consecutive stamps differ
  by T normally and by 2T = 1.9 P at each wrap. A genuine single miss gives
  ~2 P: no rule on two stamps can tell them apart. Emulator 4800 S/s, 10 s:
  LOG:COUNt? lost 0, but 1622 of 32464 records (5 %) in the "2 periods" gap
  bucket and 1622 false `lost_before` in the host CSV. The latency margin is
  only P - T (10 us at 4.8 kS/s, 5 us at 9.6).
- **2.1 x + P/2 gate**: d < T = 0.476 P, but d + entry latency can pass P/2:
  190 false gaps in 139493 at 9.6 kS/s.
- **4 x + 7/8 P gate** (implemented): after a read that saw conversion k at
  tick t, ticks t + T .. t + 3T are gated (bus-free) and t + 4T = t + P reads,
  i.e. one period after the previous read, so it sees conversion k + 1 with the
  same d: the stamps are as regular as the two crystals plus the entry
  latency. The timer / chip mismatch (ARR rounding, at most ~65 ppm, plus the
  crystals) walks d slowly to 0; then one read comes just before the
  conversion (`empty`: 9 in 27345 samples at 4800) and the next tick takes it,
  a stamp step of at most T + latency < P/2 for latency < P/4: round(gap / P)
  is exact. A conversion is lost only when an entry is delayed by more than
  P - d > 3/4 P (156 us at 4.8 kS/s, 78 us at 9.6). Cost per sample: one hit
  (~21.5 us on the device: SELF? r1 7.65 + r6-per-3-bytes ~13.5) + 3 gated
  entries (~0.5 us each): ~11 % CPU at 4.8 kS/s, ~22 % at 9.6, as
  invariant 5 budgets. Revert = `TICKS` / `GATE_EIGHTHS` in logger.rs `isr`.

### 9.3 Preemption and shared state

- **One interrupt.** NVIC: every line ICER'd and its pending bit cleared at
  each start, only IRQ 55 enabled; SysTick (SYST_CSR = 0) and PendSV can not
  fire; faults go to `default_handler` (`b .`) as before: the FWDGT resets.
  The ISR never kicks the watchdog: a main loop that stops kicking still
  resets, an ISR hang (impossible with the capped waits) would too.
- **What masks the ISR.** Only `critical_section::with` in synopsys-usb-otg
  (endpoint buffer copies, a 64-byte packet: ~1 us) and the ~20 instructions
  of `stream::stop`. fw-rs has no other `cpsid`. Measured entry latency in
  the emulator at 200 MIPS: max 3.5 us in most sessions, 10 .. 27 us in some
  4800 S/s DRAW 2 ones (most likely Renode's execution quantum: nothing in
  the firmware masks that long); the device number is DBG:ISR? max_lat_ns.
- **What the ISR preempts.** The main loop anywhere: LCD (FSMC), USB polling,
  spin-delays (stretched by <= 22 us, all tolerate it), BT UART polling. DMA
  users (BT UART4 TX, generator DAC / TIM6 and PWM / TIM12, scope DMA2 +
  TIM1/5/8) share no register with SPI2 / TIM7 / PC10..12 and keep running;
  the scope is not polled while logging anyway (main.rs).
- **GPIOC MODER.** The ISR's read-period pulse is a read-modify-write of
  GPIOC MODER (PC10 AF -> output -> AF). If it lands inside a main-loop RMW of
  the same register, the main loop writes back the value it read, which has
  PC10 = AF: the ISR restored exactly that, so nothing is lost. The main loop
  never changes PC10..12 while handed off (only `hy3131::write` does, and the
  guard stops the stream first).
- **INTF.** Reading INTF clears AD1F / RMSF / LPFF too: harmless, the DMM
  poll is paused while logging (as before) and restarts the row after.
- **Shared state** = `RING` (atomics + slots published by `head`) and
  `HANDED`; the ISR's clock / stats state (`isr::PROD`) is written by the
  main loop only before the hand-off.

### 9.4 Emulator results (plumbing only, not the device)

`emu/run.py fwrs --mips 200` (the device's instruction rate; Renode's SPI is
instantaneous, so the ISR's own time is CPU only), AC V, 0.5 V 50 Hz,
host draining LOG:DATA? continuously on one USB/IP connection, *IDN? every
second:

| build | rate, DBG:AD2:DRAW | captured | lost | stamp gaps | DBG:ISR? empty / overruns / max_lat | *IDN? max |
|---|---|---|---|---|---|---|
| main-loop sampler (feature off) | 4800, 2 | 26339 | 7855 (23 %) | 361 (max 19 ms) | - | 23 ms |
| main-loop sampler | 4800, 1 | 29907 | 110 (0.4 %) | 24 | - | 4 ms |
| main-loop sampler | 9600, 1 | 57301 | 697 (1.2 %) | 2799 | - | 4 ms |
| ad2-isr, 1.05 x (first try, 50 MIPS) | 4800, 1 | 32464 | 0 | 1622 (false) | 1622 / 0 / 68 us | - |
| **ad2-isr, 4 x** | **4800, 2** | 27345 | **0** | **0** | 9 / 0 / 9.9 us (27 us in a rerun) | 15 ms |
| **ad2-isr, 4 x** | **9600, 1** | 49928 | **0** | **0** | 13 / 0 / 3.6 us | < 1 ms |
| **ad2-isr, 4 x** | **1200, 2** | 3588 | **0** | **0** | 3 / 0 / 3.5 us | 6 ms |

ticks = 4.00 x captured in every session; dropped / bad / timeouts /
forced 0; READ? 0.5000 V AC after each stream; a CONF:VOLT:DC mid-stream
stopped the stream first (forced 0 -> 1, "Settings conflict", LOG:STATe? 0),
DC V then ~0, AC V 0.5 V again, DBG:HYSPI:SELF? 200 clean. DBG:PROF? at
9600 S/s with DRAW 2: loop max 22 ms (the repaint), USB section max 43 us.
At 50 MIPS (`--fast`) 9600 S/s with DRAW 2 starves the main loop (the
repaint alone exceeds its 100 ms period there): an emulator-speed artefact.

### 9.5 Device check (10 minutes)

Image: `./build.sh --features ad2-isr` (the path is in the hand-over). Meter
on USB, `tools/dm40scpi.py`; AC V with a 50 Hz sine of a few V on V/COM (or
AC mV). Record every reply in this section.

| min | step | commands | pass |
|---|---|---|---|
| 0-1 | build | `scpi "DBG:ISR?"`, `scpi "READ?"` | first field 1; the reading as before |
| 1-3 | 4800, full DMM repaint every 100 ms (the decision's acceptance) | `scpi "DBG:AD2:DRAW 2"`; `log a48.csv --source ad2 --rate 4800 -d 60`; `scpi "DBG:ISR?"` | trailer lost 0 (0.00 %), gaps all 0, bad 0, overwritten 0; DBG:ISR? overruns 0, timeouts 0, forced 0, max_isr_ns ~22000-26000, max_lat_ns < ~5000 (larger: something masks interrupts, report it) |
| 3-4 | 9600 | `log a96.csv --source ad2 --rate 9600 -d 30`; `scpi "DBG:ISR?"` | lost 0 or a few (7889e6c main loop: 11 %) |
| 4-6 | profile while streaming, USB alive | `scpi "LOG:SOURce AD2" "LOG:SRATe 9600" "DBG:PROF:RESET" "LOG:STATe ON"`, wait 10 s, `scpi "*IDN?" "DBG:PROF?" "DBG:ISR?" "LOG:COUNt?"`, `scpi "LOG:STATe OFF"` | *IDN? answers at once; PROF loop max ~ the repaint (no ISR-made stalls); COUNt? lost 0 (overwritten > 0 is expected: nobody drained) |
| 6-8 | DMM after a stream | `scpi "DBG:AD2:DRAW 1" "READ?" "READ?"`, DC V on a 1 V source: `scpi "CONF:VOLT:DC" "READ?"`, `scpi "DBG:HYSPI:SELF? 2000"` | readings as before the streams; SELF? all error counts 0 |
| 8-10 | function change mid-stream | AC V again; `scpi "LOG:SOURce AD2" "LOG:STATe ON"`, then press the V key (or `scpi "CONF:VOLT:DC"`); `scpi "LOG:STATe?" "SYST:ERR?" "DBG:ISR?" "READ?"` | STATe? 0, "Settings conflict", forced + 1, the reading of the new function |

On a pass: `ad2-isr` becomes a default feature (TODO), then the Wave view
(E5 / E6). A hang or a reset during a stream: report DBG:BOOT? (the shim's
crash counter) and the last DBG:ISR?.
