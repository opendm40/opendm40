# DMM reading rate, integration time and filtering: survey and options for opendm40

Date: 2026-10-24. Research only: no firmware change, no device run. opendm40
reference: `firmware/src/dmm.rs` "FR-05 conversion rate" (`Rate`, `avg_cfg`,
`Avg`) and `firmware/src/meter.rs` (`Smooth`) at 92ef9f6. Sources are listed at
the end (§9). "PDF p." means the page number in the PDF file where the printed
page number was not checked. A source marked *secondary* is a review, a
retailer or a forum post, not the manufacturer.

## 0. Summary

**What good meters do.** Every bench and system DMM surveyed separates two
things:

1. **Integration time**, the window over which one A/D conversion integrates
   the input. It is set as NPLC or aperture, or through a named rate (Keithley
   2000 FAST / MED / SLOW = 0.1 / 1 / 10 PLC) or a digits key (34401A). It sets
   the noise, the usable digits and the line-frequency rejection. Every maker
   states the same rule: power-line noise is rejected only at **1 PLC or an
   integer number of PLCs**, and below 1 PLC the rejection is **0 dB**
   (34401A, Truevolt, 3458A, 8846A, SDM3065X, DM3068). The 3458A manual
   generalises the rule: to reject any periodic interferer, set the aperture to
   an integer multiple of its period.
2. **A digital filter**, an average of several conversions with a separate
   switch, type, count and step window. It is shown on screen (Keithley,
   DMM7510: FILT; 34465A: a filter icon with "priming / settled / constantly
   resetting" states). Its defaults differ by maker: Keithley 2000 ships it ON
   (moving, 10 readings at 1 PLC, about 0.2 s), DMM7510 OFF (repeat, 10,
   window 0.1 % of range), and on the 34465A / 34470A it is an optional Math
   function. The 3458A offers an exponential low-pass instead
   (`MATH FILTER`), and it builds its long NPLC settings by averaging 10-PLC
   readings in firmware.

Handheld meters do not expose integration time. They show digits at a fixed
**4-5 readings/s** (Fluke 87V 4/s, U1282A 5/s, BM869s 5/s, DT4282 about 5/s)
and add a **fast bargraph** at 30-60/s. A **slower "hi-res / stable" mode**
gives one more digit at a lower rate (BM869s: 500,000 counts at 1.25/s), and an
**optional smoothing** mode (87V "Smooth", Hioki "SLOW", 287/289 "Smoothing")
is off by default. Keysight specifies the U1280's accuracy and NMRR at its slow
5/s rate.

**What this means for the HY3131 (the key finding).** The AD1 comb filter
notches multiples of its own output rate, FAD1CLK / OSR1 = 163.84 kHz / OSR1
(DS-HY3131 §13.1). The chopper (AD1CHOP = 1x, used by every stock DC row) then
halves the reading rate. So:

| R22 | OSR | comb window | notches at multiples of | reading period (chopped) | notches 50 Hz | notches 60 Hz |
|---|---|---|---|---|---|---|
| 0x13 | 2048 | 12.5 ms | 80 Hz | 25 ms (40/s) | no | no |
| 0x14 | 4096 | 25 ms | 40 Hz | 50 ms (20/s) | no | no |
| 0x15 | 8192 | 50 ms | 20 Hz | 100 ms (10/s) | **no** | yes |
| 0x16 | 16384 | 100 ms | 10 Hz | 200 ms (5/s) | **yes** | **yes** |
| 0x17 | 32768 | 200 ms | 5 Hz | 400 ms (2.5/s) | yes | yes |

Only **0x16 and 0x17** reject both mains frequencies. The datasheet specifies
NMR only for those two (120 dB at 5 sps, 75 dB at 10 sps: OSR 32768 / 16384
unchopped, DS p.13). For a 50 Hz owner, 0x15 is the HY3131 equivalent of a
sub-PLC setting. The stock rule ("0x15 while |ΔAD1| > 518 codes, else 0x17"),
and opendm40 MEDIUM, which copies it, pick the code that does not reject 50 Hz
**exactly when the input has AC or periodic content**. Bench meters do the
opposite: their default is 1 or 10 PLC, and sub-PLC rates are an opt-in.

This model explains the owner's three observations (§1.3):

- **The rate setting looks inert on a periodic input.** The input is always
  "moving", so FAST, MED and SLOW all run 0x15. MED and SLOW then differ only
  by a window that keeps resetting.
- **A 10 Hz square aliases into a slow ±2.4 V swing.** 10 Hz is not a notch of
  the 50 ms comb, and it equals the 10/s reading rate, so the leak folds to
  near DC and beats. A second-order comb (sinc², what the datasheet block
  diagram seems to show) predicts exactly ±2.4 V for a 0..9.6 V square (assuming the owner used the calibrator's 0..9.6 V level from device-check-2026-10-21).
  At 0x16 or 0x17, 10 Hz is a notch.
- **Small changes take 3.2-6.4 s.** That is the default 8 / 16-conversion
  window of MEDIUM / SLOW. User feedback P3 asks for no smoothing by default.

**Recommendation (option B, §7-8).** Use fixed integration presets with no OSR
switching on motion: FAST = 0x15 (10/s, labelled "60 Hz only"),
**MED = 0x16 (5/s, the default)**, SLOW = 0x17 (2.5/s). Add **one**
Keithley-style digital filter (`[SENS:]<func>:AVERage:STATe / TCONtrol /
COUNt / WINDow`, FILT annunciator), **off by default**, which replaces both the
dmm.rs window and the meter.rs smoothing. The +1 digit appears only when the
filter is on with enough slow conversions in it. A one-touch "HiRes" key
(SLOW + filter 8) behaves like the BM869s stable mode or the 87V HiRes.
Before committing, measure four things on the device: H(f) at 0x15 / 0x16 /
0x17, noise per OSR, scale equality across OSR, and settling after a step
(§8.3).

## 1. opendm40 today, and the HY3131 facts behind it

### 1.1 What opendm40 does (dmm.rs FR-05, meter.rs)

| setting | AD1 OSR | averaging (dmm.rs `Avg`) | readings |
|---|---|---|---|
| FAST | 0x15 (8192) always | none | 10/s |
| MEDIUM (default) | 0x15 while moving, 0x17 once stable (the stock rule) | 8 conversions on V r0..r2 / mV r1 (3.2 s at 0x17), 16 on ohm r0..r3; reset on a step of > ~2 counts | 10/s moving, 2.5/s steady |
| SLOW | as MEDIUM | 16 everywhere (6.4 s), 32 on ohm r0..r3 | as MEDIUM |

On top of that, `meter.rs` `Smooth` (`CALC:SMOothing`, OFF / LOW / HIGH, off by
default) runs a second moving average over the displayed readings, with its
own step reset (3 counts or 0.05 %). The +1 digit (`Val::ext`) appears in
MEDIUM / SLOW once the dmm.rs window holds 8 slow conversions.

The stock DM40 has no user setting. It runs 0x17 when |ΔAD1| <= 518 codes
(~8.6 counts) and 0x15 otherwise, shows every conversion, and does not average
(`docs/history/acdc-stock-vs-opendm40.md` §4).

### 1.2 HY3131 AD1: what the datasheet and app notes say

- **Rate.** "AD1 data output rate = FAD1CLK / OSR1", FAD1CLK = FSYSCLK / 30;
  OSR1 = 256..32768 (DS-HY3131 §13.1 items 10-11, p.31). With AD1CHOP = 1x,
  "ADC output rate will be one time slower", i.e. halved (§13.1 item 2, p.30).
  The 4.9152 MHz crystal gives 163.84 kHz, so the formula yields the table in
  §0 (it matches `hy3131::ad1_period_us`, and the logger measured 38/s at 0x13
  on the device). The feature headline "50,000 counts @ 5 Hz output rate"
  (p.4) is OSR 32768 unchopped.
- **Filter.** The AD1 block diagram (§13.1, p.30) labels the decimator
  "Comb Filter" with a "2" next to it. That suggests a second-order sinc
  (sinc²), but the prose does not state the order. A sinc^k decimator by M
  has its zeros at multiples of fs / M = the unchopped output rate; that is
  where the "notches at multiples of" column in §0 comes from.
- **Normal-mode rejection.** "Offset = 500 mV, AC 50 mV, 50 Hz / 60 Hz ±1 Hz:
  output rate = 5 sps 120 dB; output rate = 10 sps 75 dB" (DS p.13). Nothing
  is specified at 20 sps (0x15 unchopped) or at chopped rates.
- **Pre-filter.** AD1 can take its input through an on-chip RC pre-filter
  (SFT1: 100 k / 10 k / 0 / open, with a capacitor between the FTP and FTN
  pins), which "will help to filter noise and stabilize signal" (DS §12.4,
  p.27). APD-DMM003 §1 (p.6) advises "When OPA measures DC, it is advised to
  open ADC1 Pre-Filter" and suggests a 10 MΩ input load on mV against 50/60 Hz
  pickup. Whether the DM40 board fits an FTP-FTN capacitor, and what SFT1 the
  stock DC rows set, is not known (a hardware check, §8.3).
- **Display averaging.** HYCON's EVA tool (APD-DMM001 §1, p.3; §4 item 12,
  p.17) computes RMS noise and ENOB from at least 1024 conversions and offers
  a software "Avr. Times" averaging of the displayed value. That is a test
  tool, not a recommendation for a product; the app notes do not recommend
  any display averaging. The demo-code zips in `docs/` contain only an
  "under construction" readme.
- **Other HY3131 products.** The OWON B35 runs AD1 at OSR 16384 / 32768
  **unchopped**, i.e. 100 / 200 ms, both mains-notching (essentialscrap.com
  OWON fix, as summarised in `emu/hy3131/RESEARCH.md` §1). The stock DM40
  runs 0x15 / 0x17 chopped (the same RESEARCH.md §3 table). Both stock
  designs stay at the datasheet's 5-10 sps for DC; neither publishes a rate
  setting.

### 1.3 The owner's observations, explained

**"The setting doesn't change the speed on a periodic input."** Any periodic
input moves AD1 by more than 2 counts (opendm40) or 518 codes (stock) between
conversions. MEDIUM and SLOW therefore stay at 0x15 and keep resetting their
windows, which makes them identical to FAST.

**"A 10 Hz square on DC V aliases into a slow ±2.4 V swing."** At 0x15 the
comb window is Tc = 50 ms and its notches are at 20, 40, 60 Hz. A 10 Hz input
sits at the first sidelobe. For sinc², |H(10 Hz)| = sinc²(0.5) = 0.405. The
square's 6.1 V fundamental (0..9.6 V, ±4.8 V) gives 0.405 × 6.1 ≈ 2.5 V. Worked
in the time domain: the sinc² kernel is a 100 ms triangle, so the reading is
¾·4.8 − ¼·4.8 = **±2.4 V** at the worst phase (assuming the 0..9.6 V calibrator square). One reading comes every 100 ms
(chopped), the same period as the signal, so the leak lands on a near-DC beat
whose rate is the frequency mismatch. That is the "slow swing". The chop pair
evidently does **not** form a contiguous 100 ms boxcar: a boxcar would null
10 Hz exactly. That is consistent with the earlier 40 Hz test (40 Hz square
0..9.6 V read 4.77-4.81 V in DC V, `device-check-2026-10-21.md`): 40 Hz is a
notch of every code from 0x14 up. At **0x16 (notches every 10 Hz) and 0x17
(every 5 Hz), 10 Hz is nulled.** This is a model fitted to two data points;
§8.3 test 1 checks it.

**"The sliding window slows the response to small changes to 3.2-6.4 s."** A
change smaller than the reset threshold (2 counts) crawls through the 8- or
16-conversion window at 0.4 s per conversion. That is the intended trade-off of
a moving average with a step window (Keithley, DMM7510 and 34465A all do the
same), but opendm40 applies it **by default** in MEDIUM. The users' P3 praise
("settles instantly") and the decision to keep smoothing off by default
(`user-feedback.md` P3, FR-05 row) argue against that.

**Where the blind spots are, per code.** A chopped reading every 2·Tc
aliases any input near k / (2·Tc) to near DC. Those inputs that are not comb
notches (the odd k) beat with amplitude ≈ sinc²(f·Tc):

| R22 | beats near DC (not notched) | leak at those frequencies |
|---|---|---|
| 0x15 | 10, 30, 50, 70 Hz | 0.41, 0.045, 0.016, 0.008 |
| 0x16 | 5, 15, 25, 35 Hz | 0.41, 0.045, 0.016, 0.008 |
| 0x17 | 2.5, 7.5, 12.5 Hz | 0.41, 0.045, 0.016 |

0x15 has **50 Hz mains** in its list (−36 dB); 0x16 and 0x17 do not. No code
can reject a signal near the reading rate itself. On a bench meter that is the
user's job: choose an aperture that covers whole periods (3458A).

## 2. Bench and system DMMs

### 2.1 Keysight / HP 34401A

- **Speed vs resolution.** NPLC 0.02 / 0.2 / 1 / 10 / 100. From the front
  panel, the DIGITS keys (4½ / 5½ / 6½, each "fast" or "slow") select the NPLC;
  the default is 5½ slow = 10 PLC. Readings/s at 60 (50) Hz: 100 PLC 0.6
  (0.5), 10 PLC 6 (5), 1 PLC 60 (50), 0.2 PLC 300, 0.02 PLC 1000 (6½, 6½, 5½,
  5½, 4½ digits) [34401A datasheet p.4]. The digits mapping and default come
  from secondary excerpts of the User's Guide (see §9); the guide itself was
  not retrieved.
- **NMR.** "100 plc, 10 plc, 1 plc: 60 dB; < 1 plc: 0 dB", for line frequency
  ±0.1 %; ±1 % → 40 dB, ±3 % → 30 dB [datasheet p.4 note 2-3].
- **Autozero.** The fastest rate needs autozero, autorange and the display off
  [datasheet p.4 note 6].
- **Filters.** The DC functions have **no digital filter**. AC has a "slow /
  medium / fast" filter (3 / 20 / 200 Hz) at 0.15 / 1 / 10 readings/s [datasheet
  p.4].
- **Periodic content on DC.** Integer NPLC only; no other mechanism.

### 2.2 Keysight Truevolt 34460A / 34461A / 34465A / 34470A

- **Speed vs resolution.** NPLC 100 / 10 / 1 / 0.2 / 0.02 on all models; the
  34465A / 70A add 0.06 / 0.006 / 0.002 / 0.001 PLC and aperture mode.
  34465A / 70A noise adders vs NPLC: 10 PLC and up 0; 1 PLC 0.0001 % + 0.5 µV;
  0.2 PLC 0.0005 % + 3 µV; 0.02 PLC 0.0020 % + 3 µV. Readings/s: 0.6 / 6 / 60
  / 333 / 3,333 up to 50,000 [Truevolt datasheet 5991-1983EN p.23]. The
  accuracy specs assume 10 or 100 PLC with autozero on [p.12 note 1].
- **NMR.** ≥ 1 PLC: 60 dB; < 1 PLC: 0 dB; ±1 % line → 40 dB [p.22].
- **Autozero.** ON / OFF / ONCE. Autozero OFF adds 0.0002 % of range + 5 µV
  [p.22]. All the reading rates are quoted with autozero off [p.23 note 1].
- **Digital filter ("Smoothing", 34465A / 70A only).** A moving-average boxcar.
  Response FAST / MEDIUM / SLOW = 10 / 50 / 100 readings. It resets on a
  function change or when a reading lands "too far beyond the current
  average". The filter icon shows its state, "solid yellow" when large
  variation keeps resetting it. It is not available in digitize, data logging,
  continuity, diode or probe hold [Truevolt help "Math - Smoothing"; the page
  returned 403 to the fetcher, so the text is from the search index].
  opendm40 already uses this command name (`CALCulate:SMOothing:RESPonse`).
- **AC filter.** 3 / 20 / 200 Hz; ACV 0.4 / 1.6 / 40 readings/s [p.12 note
  6, p.23].

### 2.3 Keysight / HP 3458A

- **Speed vs resolution.** NPLC 0..1000 or APER 500 ns..1 s, in 100 ns steps;
  resolution can also be given directly (`DCV 10, 1E-6`). Power-on NPLC is 10,
  PRESET NORM 1 [3458A User's Guide 03458-90014, Ch. 3 pp. 58-61]. Table
  (60 Hz, autozero off / on): NPLC 0.0001 4.5 digits 100,000 / 4,130 per s;
  0.01 6.5 digits 5,300 / 930; 0.1 6.5 digits 592 / 245; 1 7.5 digits 60 /
  29.4; 10 8.5 digits 6 / 3; 100 36 / 18 per min [App. A p.285].
- **NMR.** "The multimeter achieves NMR … when the integration time is ≥1
  power line cycle"; NPLC < 1: 0 dB, > 1: 60 dB, 1000: 75 dB; ±1 % line → 40 dB
  [pp. 58-59, App. A p.284]. `LFREQ` sets the reference frequency (e.g. 50 Hz
  for a 50 Hz DUT on 60 Hz mains) [p.59].
- **Non-mains periodic signals on DC.** "You can also use the APER command to
  reject a noise signal of a specific frequency … set the integration time
  equal to an integral multiple of the period of the signal to be rejected.
  For example, to reject noise at 100 Hz (period = 10 ms), specify … 10 ms,
  20 ms, 30 ms" [p.60].
- **Long apertures are firmware averaging.** "If you specify 60 PLCs … the
  multimeter averages six 10 PLC readings"; APER integrates one reading for
  up to 1 s [p.60]. This is the precedent for opendm40 building its +1 digit by
  averaging 0x17 conversions.
- **Autozero.** AZERO ON measures a zero after every reading and roughly
  halves the rate (table above); OFF / ONCE takes one zero [p.61].
- **Digital filter.** `MATH FILTER`: "simulates the output of a single pole,
  low pass, RC filter … Result = (Previous Result)×(DEGREE−1)/DEGREE +
  Reading/DEGREE"; DEGREE = number of readings to reach 63 % of a step
  [p.124]. Shown by the MATH annunciator [p.27].

### 2.4 Keithley 2000 (6½ digit)

- **Speed vs resolution.** RATE FAST / MEDium / SLOW = 0.1 / 1 / 10 PLC;
  0.01..10 PLC over the bus. The FAST / MED / SLOW annunciators show the rate
  [2000 User's Manual 2000-900-01 Rev J, §3 "Rate" p.3-6, Table 3-1 p.3-7].
  Digits (3½..6½) are display-only: "The number of displayed digits does not
  affect accuracy or speed. Those parameters are controlled by the RATE
  setting" [p.3-5]. 2000 readings/s at 4½ digits into the buffer, 50
  triggered readings/s at 6½ over GPIB [§1, PDF p.7].
- **Defaults.** DCV / DCI / Ω: rate MEDium (1 PLC), **filter ON, count 10,
  moving average**, autozero ON [Table 2-2 Factory defaults, §2, PDF pp. 24-26].
  At 50 Hz that is 10 × 20 ms ≈ 0.2 s of averaging (more with autozero); a count
  of 10 is not a long span at this rate.
- **Digital filter.** "The displayed, stored, or transmitted reading is
  simply an average of a number of reading conversions (from 1 to 100)." The
  moving filter is a FIFO stack; the repeating filter fills the stack, averages
  it, clears it and starts over ("Choose this filter for scanning"). The FILT
  annunciator is on while the filter is enabled. It is not available for
  frequency, period, continuity and diode [§3 "Filter" p.3-4, Fig. 3-1]. SCPI:
  `[SENSe:]<function>:AVERage:TCONtrol MOVing|REPeat`, `:COUNt 1..100`,
  `:STATe` [§5 SCPI tables]. There is **no window**.
- **AC "bandwidth".** RATE also sets the ACV low-frequency limit: SLOW 3 Hz
  (1200 samples, 1.2 s), MED 30 Hz (120 ms), FAST 300 Hz (1 PLC) [p.3-7].
- **Autozero.** `SYSTem:AZERo:STATe`; "When disabled, speed is increased at
  the expense of accuracy" [§5, PDF p.195].

### 2.5 Keithley 2010 and 2100

The 2010 and 2100 user manuals were **not retrieved** (the mirror returned
HTML). The 2100 is specified at 1 PLC (2100 spec sheet, §9, secondary
mirror). Keithley's application note "Five Ways to Shave Test Time" covers the
same family. It explains the integer-NPLC rule, autozero (three measurements
per reading, ×3 speed when off) and the filters: repeating "is the slowest,
since the stack has to be completely filled for each reading"; moving is
faster "but … slightly less stable" [Keithley 4452, pp. 1-3]. It also
explains why front-panel defaults are slow on purpose: "if … it is reading
2000 samples per second, your eyes would not be able to distinguish the data"
[p.1].

### 2.6 Keithley DMM7510 (and DMM6500)

- **Speed vs resolution.** `[SENSe]:<function>:NPLCycles` 0.0005..15 (60 Hz),
  default 1; an aperture can also be given [DMM7510 Reference Manual
  DMM7510-901-01 Rev B, p.6-85].
- **Autozero.** `:AZERo[:STATe]` default ON: the reference and zero are
  measured with every reading, "in addition to the normal measurement time";
  `AZERo:ONCE` exists [p.6-72].
- **Digital filter.** Repeating or moving average [§3 pp. 3-11..3-12]. SCPI:
  `:AVERage[:STATe]` default **OFF** [p.6-68], `:COUNt` 1..100 default 10
  [p.6-67], `:TCONtrol` default **REP** [p.6-69], `:WINDow` 0..10 % of range
  default **0.1** [p.6-71]. Window semantics: "A reading that falls outside
  the plus or minus noise window fills the filter stack immediately … the
  reading is a single reading conversion, and new averaging starts from this
  point" [p.6-71]. When a moving filter starts, "the first measurement … is
  copied into all the stack locations" [p.3-11]. The status bar shows **FILT**
  ("A filter is applied to the measurement") [§2, PDF p.44].
- **DMM6500.** Its reference manual (DMM6500-901-01) was not downloaded.
  Third-party drivers expose the same `AVERage` set (repeat / moving / hybrid,
  count 1..100) [pymeasure DMM6500 docs]. A Keithley preset for precise DCV
  uses 5 NPLC, autozero and a 20-reading moving filter [element14 blog,
  secondary]. The same command set as the DMM7510 is assumed, not verified.

### 2.7 Fluke 8845A / 8846A (and Tektronix DMM4050)

- **Speed vs resolution.** NPLC 100 / 10 / 1 / 0.2 / 0.02 → 6½ / 6½ / 5½ /
  5½ / 4½ digits at 0.6 (0.5) / 6 (5) / 60 (50) / 270 / 995 readings/s, with
  autozero off [8845A/8846A Users Manual Rev 3, p.1-23]. The front-panel digits
  of the DC functions follow the PLC setting [p.3-9].
- **NMR.** 60 dB at NPLC ≥ 1 with the analog filter off; **100 dB with the
  analog filter on** (line ±0.1 %) [Ch. 1 specifications, PDF pp. 20, 25-26].
- **Digital filter "D FLTR".** "This filter averages readings to reduce
  reading noise … The filter is only available for dc functions at rates
  slower than 1 PLC. The number of readings averaged by the digital filter
  varies with dc function and range." The user sets no count [p.4-4]. It was
  added with OutGuard firmware 2.0 [PDF p.47].
- **Analog filter "A FLTR".** A 3-pole analog filter that "will increase
  stabilization time" [p.4-5]. It is "intended to reduce the presence of
  external ac when making dc measurements … for example a DC power supply
  voltage that has significant AC line ripple". It adds offset, so the range
  must be zeroed first [App. D p.D-1]. This is the only surveyed meter with a
  documented answer to "DC with AC content" other than "use a longer
  aperture".
- **AC filter.** 3 Hz slow / 20 Hz (power-up default) / 200 Hz [p.3-9,
  p.4-6]; 0.47 / 1.64 / 4.5 readings/s [p.1-23].
- **DMM4050.** Widely reported to be the same platform as the 8846A; **not
  independently verified** here.

### 2.8 Fluke 8508A / 8588A (reference multimeters)

The manuals were **not retrieved** (no accessible PDF). The only fact found:
the 8588A's DC specs assume an aperture ≥ 100 µs, and Fluke's product page
claims its digital filters settle faster than analog ones (Fluke 8588A product
page, secondary summary). They are left out of the table.

### 2.9 Rohde & Schwarz HMC8012

- **Speed.** "ADC Rate" SLOW / MEDIUM / FAST = **5 / 10 / 200 readings/s**
  (DC V, DC I; 2-wire Ω FAST 50). "The maximum accuracy in measurements is
  achieved when using the setting SLOW" [HMC8012 User Manual v06, English
  part, PDF pp. 52-54]. 200 ms and 100 ms are integer multiples of both 20 ms
  and 16.7 ms, so SLOW and MEDIUM are mains-notching windows: the same pair as
  HY3131 0x17 / 0x16 unchopped.
- **Autozero.** "If the ADC RATE = FAST is selected, the function AUTO ZERO is
  automatically deactivated" [PDF p.52].
- **AC filter.** SLOW / MEDIUM (default) / FAST; SLOW for < 50 Hz, FAST for
  > 1 kHz. "The AC filter represents a display filter and not a low-pass
  filter" [PDF pp. 52-53].

### 2.10 Siglent SDM3065X, Rigol DM3068

- **Siglent SDM3065X.** NPLC 0.005 / 0.05 / 0.5 / 1 / 10 / 100 (60 Hz:
  0.006 / 0.06 / 0.6 …): resolution 2.7 / 1.6 / 1 / 0.22 / 0.08 / 0.035 ppm of
  range, NMRR 0 / 0 / 0 / 60 / 60 / 60 dB, 10000 / 1000 / 100 / 50 / 5 / 0.5
  readings/s. NMRR is −20 dB at line ±1 %; the rates are with autozero off
  [SDM3065X datasheet, PDF p.6, pp. 11-12]. The manual's DC filter was not
  retrieved; an EEVblog thread reports an SDM3055 DC "filter" that slows
  continuity and may be analog (secondary).
- **Rigol DM3068.** NPLC 0.006 / 0.02 / 0.06 / 0.2 / 1 / 2 / 10 / 100:
  10000 / 2500 / 833 / 250 / 50 / 25 / 5 / 0.5 readings/s (50 Hz); NMRR 60 dB
  from 1 PLC up, 0 below [DM3068 datasheet PDF pp. 2-3, p.15]. Filter
  settings not retrieved.

## 3. Handheld meters

| meter | counts | digits update | bargraph | rate / smoothing control | source |
|---|---|---|---|---|---|
| Fluke 87V | 6000 (HiRes 19,999) | 4/s | 33 seg, 40/s ("10 times faster than the digital display") | **Smooth**: power-up option (hold a key at power-on), icon on the display, "when the input signal changes rapidly, smoothing provides a steadier reading"; HiRes key = 10x resolution; LPF on AC V | 80 Series V Users Manual pp. 38-40, 51 |
| Fluke 287 / 289 | 50,000 | ~4/s (secondary) | ~30/s (secondary) | "Selectable ac filter (Smoothing mode) helps display a steadier reading when the input signal is changing rapidly or noisy"; LPF | Fluke 287/289 datasheet; lygte review |
| Fluke 189 | 50,000 | not retrieved | yes | not retrieved | Fluke 187/189 spec (Transcat) |
| Keysight U1281A / U1282A | 60,000 | **Slow 5/s, Fast 40/s** (cap, Hz: 1/s) | yes | refresh-rate setting; "Specification is based on 5 times/second of data refresh rate", NMRR too; LPF | U1280 datasheet 5992-0847EN pp. 10, 17 |
| Keysight U1272A | 30,000 | not retrieved | yes | not retrieved | — |
| Brymen BM869s (BM860s family) | 50,000 fast / **500,000 stable** (DC V) | **5/s fast; 1.25/s stable** | 41 seg, 60/s | the stable mode is the hi-res, slow mode | BM860s datasheet |
| Hioki DT4282 | 60,000 | ~5/s (secondary) | none | Mem/**Slow** key: hold for an averaging filter; LPF on AC V | lygte review; Hioki manual returned 403 |
| Gossen METRAHIT PM TECH / XTRA | 60,000 | not stated | bargraph / pointer **40/s** | 1 kHz LPF on AC V, "Fil" on display | Operating instructions 3-349-456-03, PDF p.19 |
| Gossen METRAHit 2+ | — | 2/s | 20/s | — | RS listing (secondary) |
| Uni-T UT171A / UT181A | 60,000 | UT171A 5/s (secondary) | yes | not retrieved | EEVblog thread |

Patterns:

- No handheld exposes integration time. The digits run at **2-5/s**. A
  **separate fast path drives the bargraph** (30-60/s; the 87V is "10 times
  faster"). One more digit costs speed (BM869s 5/s → 1.25/s; 87V HiRes).
- **Smoothing / averaging is an option, off by default**, and is shown with an
  icon (87V). Keysight specifies accuracy and NMRR at the slow refresh rate;
  its fast 40/s rate is an extra.
- Handhelds deal with "DC with AC content" by **rate choice** (5/s windows) and
  an AC LPF; none documents more.

## 4. ΣΔ / consumer DMM chips

- **HYCON HY3131** (DM40, OWON B35). See §1.2: no rate recommendation beyond
  "5 Hz output rate" and the 5 / 10 sps NMR figures. Real products run AD1 at
  5-10 readings/s on a mains-notching window (OWON: 100 / 200 ms unchopped),
  or switch to 0x15 while moving (DM40 stock).
- **Fortune FS9922-DMM4** (6000 counts): "Converter Rate: 3 times/sec; Bar
  graph: 30 times/sec", using a separate fast ADC for the bar [FS9922-DMM4
  datasheet Rev 1.3].
- **Cyrustek ES51932** (22,000 counts, a sibling of the ES51922 in the
  UT61E): "Conversion rate: 2 times/s" plus a 46-segment bargraph [ES51932
  datasheet, alldatasheet mirror]. Cyrustek's ES51963 lets the MCU select the
  conversion rate (search summary). The ES51997P datasheet was not found.
- The common pattern: a fixed slow main conversion (2-5/s) on a window that
  rejects mains, plus a fast coarse path for the bargraph. The HY3131 has only
  one high-resolution ADC (AD1). AD2 / AD3 are the fast AC / RMS converters,
  so a bargraph faster than the readout means running AD1 faster
  (option D).

## 5. Comparison table

| instrument | speed control | steps (DC readings/s) | mains rule | autozero | digital filter (default) | window | on-screen | AC filter |
|---|---|---|---|---|---|---|---|---|
| 34401A | NPLC via DIGITS fast / slow | 1000 / 300 / 60 / 6 / 0.6 (60 Hz) | ≥1 PLC 60 dB, <1 0 dB | on; off for max speed | none on DC | — | — | 3 / 20 / 200 Hz |
| 34461A | NPLC | 1000 / 300 / 60 / 6 / 0.6 | same | ON / OFF / ONCE | none | — | — | 3 / 20 / 200 Hz |
| 34465A / 70A | NPLC, aperture | up to 50,000 … 0.6 | same | ON / OFF / ONCE | Smoothing boxcar 10 / 50 / 100 (optional Math) | reset when "too far" from the mean | icon, state colours | 3 / 20 / 200 Hz |
| 3458A | NPLC 0..1000, APER, resolution | 100k … 36/min | ≥1 PLC; APER = n × interferer period | ON halves the rate | MATH FILTER (EMA, DEGREE) | — | MATH | analog / sync / random sampling |
| Keithley 2000 | RATE F / M / S = 0.1 / 1 / 10 PLC | 2000 (4½) … | 10 PLC best | ON | **moving, 10, ON** | none | FILT, FAST / MED / SLOW | rate = BW 300 / 30 / 3 Hz |
| DMM7510 | NPLC 0.0005..15 | — | — | ON / ONCE | repeat, 10, **OFF** | 0.1 % of range | FILT | — |
| Fluke 8846A | NPLC | 995 / 270 / 60 / 6 / 0.6 | 60 dB (100 dB with A FLTR) | off for rates | D FLTR, count per function / range | — | key highlight | 3 / 20 / 200 Hz |
| HMC8012 | ADC rate S / M / F | 5 / 10 / 200 | (windows 200 / 100 ms) | auto-off at FAST | — | — | — | display filter S / M / F |
| SDM3065X | NPLC | 10000 … 0.5 | ≥1 PLC 60 dB | off for rates | not retrieved | — | — | slow / med / fast |
| DM3068 | NPLC | 10000 … 0.5 | ≥1 PLC 60 dB | "Once" for resolution spec | not retrieved | — | — | slow / med / fast |
| Fluke 87V | none | 4 (bar 40) | — | — | Smooth (power-up option) | — | icon | LPF |
| U1282A | Slow / Fast | 5 / 40 | specs at 5/s | — | — | — | — | LPF |
| BM869s | fast / stable | 5 / 1.25 (500k) | — | — | stable mode | — | — | — |
| DT4282 | Slow key | ~5 | — | — | Slow = average | — | — | LPF |
| **DM40 stock** | none (auto) | 10 moving / 2.5 steady | 0x15 does not notch 50 Hz | chop always | none | — | — | RMS EMA |
| **opendm40 now** | RATE S / M / F | 10 / 10→2.5 / 10→2.5 | as stock | chop always | window 8-32 by default (M / S) + CALC:SMO | ~2 counts | +1 digit only | RMS EMA |

## 6. Lessons for opendm40

1. **The rate must not follow the input.** Every bench meter holds its
   integration time fixed; auto-switching exists only in the stock DM40.
   Switching to a short window on motion defeats mains rejection exactly when
   it is needed, and it makes the rate setting invisible on any periodic
   input.
2. **The default integration time rejects mains.** On the HY3131 that means
   0x16 or 0x17. 0x16 gives 5/s, which is the handheld norm (4-5/s), and both
   0x16 and 0x17 notch 50 **and** 60 Hz, so a battery meter that does not know
   its line frequency needs no LFREQ setting for rejection.
3. **Averaging is a separate, visible, optional layer.** Keithley-style
   type / count / window is the common SCPI vocabulary. The window is what
   keeps a filtered meter responsive to steps (DMM7510 default 0.1 % of
   range = 60 counts on a 60000-count scale; opendm40 uses ~2 counts, so its
   window resets on almost any real signal).
4. **Post-averaging cannot remove an alias that the converter window let
   through near the reading rate.** Averaging N contiguous conversions does
   not attenuate inputs at multiples of the reading rate; only the comb (or
   an analog pre-filter) can. Long apertures and filters help with random
   noise and with ripple away from those frequencies.
5. **One more digit costs speed and should say so.** The 3458A averages
   10-PLC readings for longer NPLC; BM869s shows 500k counts at 1.25/s; the 87V
   HiRes is a mode. In opendm40 the +1 digit is a property of "filter on at a
   slow code", not of a default rate.
6. **A fast bargraph is a separate path on handhelds.** On the HY3131 it can
   only come from a faster AD1 code, which conflicts with lesson 2 unless the
   readout is decimated in firmware (option D).

## 7. Options for opendm40

Common numbers: AD1 periods 100 / 200 / 400 ms at 0x15 / 0x16 / 0x17 (chopped).
Noise per conversion is assumed white: σ(0x16) ≈ σ(0x15)/√2,
σ(0x17) ≈ σ(0x15)/2, consistent with the dmm.rs note that one 0x15 conversion
has ~2x the noise of one 0x17 conversion. Not measured yet (§8.3 test 2). A
single 0x17 conversion has 6-7.5 codes per +1-digit step (the dmm.rs +1 digit
table), so the +1 digit needs about 8 slow conversions (3.2 s) either way.
"Step" = a change larger than the filter window; "drift" = a change smaller
than it. Settling after a step is assumed to take 2 conversions (sinc² spans
2·Tc, plus the chop pair); §8.3 test 4 measures it.

### Option A: fixed integration presets only

- **Behaviour.** FAST = 0x15, MED = 0x16 (default), SLOW = 0x17; no OSR
  switching; no averaging; `meter.rs` smoothing kept or dropped.
- **Readings/s.** 10 / 5 / 2.5.
- **Step response.** ~0.2 / 0.4 / 0.8 s. **Drift.** Follows within one
  period.
- **Noise / +1 digit.** σ0, σ0/√2, σ0/2. **No +1 digit** (not enough
  conversions); that is a regression against UF-25 / the current MEDIUM.
- **Periodic input.** MED / SLOW notch 50 / 60 Hz and every 10 Hz (MED) or
  5 Hz (SLOW) multiple, so the 10 Hz square reads its mean. The blind spots
  are near 5 / 15 Hz (MED) and 2.5 / 7.5 Hz (SLOW), §1.3. FAST leaks 50 Hz
  (−36 dB) and 10 Hz; label it "FAST (60 Hz)" or just "fast, no 50 Hz
  rejection".
- **UI.** The existing RATE page; a FAST / MED / SLOW chip as on the Keithley
  2000.
- **SCPI.** `[SENSe:]RATE SLOW|MEDium|FAST` (kept), plus
  `[SENSe:]<func>:APERture 0.1|0.2|0.4` and `:NPLCycles` (rounds up to the
  next code; NPLC reported for `SYSTem:LFRequency` 50|60, default 50, which
  affects only the conversion: 5 / 10 / 20 PLC at 50 Hz, 6 / 12 / 24 at 60 Hz).
- **Effort.** S: delete the moving / stable switch for DC, add 0x16 to the DC
  rows (it is in the table, used by the stock only for temperature).
- **Risk.** Low-medium: 0x16 is untested on DC rows. The stock already swaps
  0x15 / 0x17 under one calibration and the datasheet transfer formula has no
  OSR term, but scale equality must be checked (§8.3 test 3). Loses the +1
  digit.

### Option B: presets plus a Keithley-style filter (**recommended**)

- **Behaviour.** Option A, plus **one** digital filter that replaces the
  dmm.rs `Avg` window and `meter.rs` `Smooth`:
  - state OFF (default) / ON;
  - type MOVing (default) / REPeat;
  - count 2..32 (the `AVG_MAX` 32 buffer; default 8);
  - window 0..10 % of range, 0 = no reset; proposed default 0.02 % = 12
    counts on the 60000-count scale, above the 0x15 noise peak and well
    below the DMM7510's 0.1 %;
  - the window is compared with each new conversion against the stack mean
    (DMM7510 semantics). A moving filter starts with the stack filled with the
    first reading.
  A **HiRes** quick toggle on the readout sets SLOW + MOV 8 + the default
  window (the BM869s stable mode / 87V HiRes pattern). The +1 digit is shown
  only when the filter is on and the stack holds ≥ 8 conversions at 0x17
  (3.2 s) or ≥ 16 at 0x16 (3.2 s). Optionally, a third type `EXPonential`
  (3458A `MATH FILTER`), with no stack edge and therefore no "sticks then
  jumps" on a small drift.
- **Readings/s.** As in A for MOV; rate / count for REP (SLOW + REP 8 = one
  reading per 3.2 s).
- **Step response.** One conversion more than A (the window fills the stack
  with the new value).
- **Drift.** Lag of (count − 1)·T / 2. MOV 8 at SLOW: 1.4 s group delay, 3.2 s
  to full value. MOV 8 at MED: 0.7 / 1.6 s.
- **Noise / +1 digit.** ÷√count: MOV 8 at SLOW ≈ σ0/5.7. The +1 digit is real
  under the white-noise assumption (§8.3 test 2).
- **Periodic input.** The notches of A. The filter also suppresses ripple
  that is not near the reading rate; blind-spot beats are reduced only if
  their period is shorter than the stack. On a large periodic swing the window
  resets on every conversion; the FILT chip shows that (34465A "solid
  yellow") so the user knows to set the window to 0 or pick a slower rate.
- **UI.** A FILT chip in the status row: outlined while the stack fills,
  filled when full, warning colour while it keeps resetting. Settings → DMM:
  Rate (F / M / S), Filter (Off / On), Type, Count, Window; a HiRes toggle on
  the DMM page.
- **SCPI** (Keithley 2000 / DMM7510 names, 34401A / 34465A aliases):
  - `[SENSe:]<func>:AVERage[:STATe] ON|OFF`, `:TCONtrol MOVing|REPeat`,
    `:COUNt <n>|MIN|MAX|DEF`, `:WINDow <%>|MIN|MAX|DEF`, all with queries.
    `<func>` = VOLTage[:DC], CURRent[:DC], RESistance; one global setting
    applied to all AD1 functions unless per-function storage is cheap (the
    Keithley manuals store it per function).
  - `CALCulate:SMOothing[:STATe]` / `:RESPonse SLOW|MEDium|FAST` kept as
    34465A-compatible aliases (STATe = AVER:STAT; RESPonse = COUNt 32 / 16 / 8
    instead of the 34465A's 100 / 50 / 10, documented).
  - `[SENSe:]RATE`, `:APERture`, `:NPLCycles` as in A.
  - READ? / MEAS? with REP wait for a full stack; with MOV they return the
    moving value; with the +1 digit they wait for ≥ 8 slow conversions (the
    existing `AVG_FULL` rule).
- **Effort.** M: merge two averaging layers into one, persist four fields,
  FILT chip, the HiRes toggle, SCPI subtree, host tests for window / reset.
  Most of the machinery exists (`Avg`, `Smoother`, `ext_ok`).
- **Risk.** Low. The main behavioural change is visible: no averaging and no
  +1 digit by default (it matches P3 / FR-05 "default off").

### Option C: stock-like AUTO plus the optional filter

- **Behaviour.** `RATE AUTO` = the stock rule (0x15 if |ΔAD1| > 518 codes,
  else 0x17) as default or as a fourth choice, plus the B filter.
- **Readings/s.** 10 moving / 2.5 steady.
- **Step response.** ~0.1-0.2 s (the stock's "instant" feel, P3).
- **Drift.** A drift faster than ~8.6 counts per conversion keeps it at 0x15.
- **Noise.** σ0 moving, σ0/2 steady.
- **Periodic input.** The worst of the options: any input with AC content
  holds 0x15, which does not reject 50 Hz and aliases 10 / 30 Hz; the rate
  setting stays invisible.
- **Variant C'.** AUTO between 0x16 (moving, 5/s) and 0x17 (steady); both
  notch mains. It keeps "fast while moving" at half the stock's speed without
  the 50 Hz hole. Since 0x16 already updates at 5/s, the gain over a fixed
  0x16 is only the √2 lower noise of 0x17 when steady.
- **UI / SCPI.** As B, plus `RATE AUTO` (opendm40 extension; no bench
  equivalent).
- **Effort.** S (the code exists). **Risk.** Low technically. It keeps the
  behaviour the owner is unhappy with, and the reading rate varies with the
  signal (READ? timing, log cadence).

### Option D: oversampled stream plus firmware integration (the "better idea", with caveats)

- **Behaviour.** Run AD1 at 0x13 (25 ms, 40/s, the logger's stream code,
  `AD1OSR_STREAM`) all the time. The bargraph (UF-30, ≥ 40/s) is fed from the
  raw stream. The readout is a firmware boxcar of 4 / 8 / 16 conversions
  (100 / 200 / 400 ms apertures), the 3458A "NPLC > 10 = averaged readings"
  idea taken down to the chip's shortest code. The stream is uniformly
  sampled, so the sampled-domain argument holds whatever the per-sample
  kernel: 50 Hz aliases to 10 Hz and 60 Hz to 20 Hz, which a boxcar of 4k
  samples nulls; the same holds for 100 Hz and the 10 Hz square's harmonics.
  A step detector on 100 ms block means (already free of 10 Hz-multiple
  ripple) restarts the window; the window then grows back to the target
  (adaptive aperture: 5 digits at 0.1 s, the +1 digit when the window
  reaches the noise target).
- **Readings/s.** Readout 40/s internally (shown at 5/s, as handhelds do);
  bargraph 40/s.
- **Step response.** ~50-100 ms. **Drift.** Lag of half the aperture.
- **Noise.** **Unknown**, and the main risk: at OSR 2048 a ΣΔ modulator may be
  quantisation-noise-limited, and a firmware boxcar is a first-order sinc
  whose notches are shallower than the chip's own comb at OSR 16384. The
  +1 digit may not be reachable in 3.2 s.
- **Periodic input.** Blind spots at multiples of 40 Hz, which alias to DC:
  **120 Hz** (60 Hz full-wave ripple) and **200 Hz** (4th harmonic of 50 Hz).
  They are attenuated only by the 12.5 ms sinc² (−27 dB and −36 dB). This is
  worse than 0x16 for 60 Hz countries.
- **UI / SCPI.** As B, with `APERture` / `NPLCycles` mapped to the firmware
  window (0.1 s steps) and `RATE` mapped to apertures.
- **Effort.** L: a new acquisition path (the logger's 40/s ISR exists),
  calibration consistency at 0x13, settling after range changes, CPU / RAM
  for a 40-sample ring per function.
- **Risk.** Medium-high until §8.3 test 2 shows that 16 × 0x13 matches 1 × 0x17
  in noise.

### Option E (variant of A / B): "autozero off" FAST

The HY3131 chopper is opendm40's autozero. Unchopped, 0x06 (OSR 16384) gives
**10/s with a 100 ms comb that notches 50 and 60 Hz**, which is what the OWON
HY3131 meter runs. A FAST preset at 0x06 would be the 3458A / Truevolt
"autozero OFF doubles the rate" trade-off. Its offset is VOS instead of
nulled: handle it with a periodic chopped reference (`[SENSe:]ZERO:AUTO ONCE`
semantics, 34401A / Truevolt; Keithley `SYSTem:AZERo`) or with firmware
alternation of AD1CHOP 00 / 01. Effort M; risk medium (offset drift with
temperature, calibration done chopped). It is worth a test because it removes
FAST's 50 Hz hole.

### Summary of options

| | A presets | **B presets + filter** | C AUTO + filter | D 40/s stream | E AZ-off FAST |
|---|---|---|---|---|---|
| default readings/s | 5 | **5** | 10 → 2.5 | 40 (shown 5) | (FAST only) 10 |
| step | 0.4 s | **0.4-0.6 s** | 0.1-0.2 s | 0.05-0.1 s | 0.2 s |
| drift (filter on) | 0.2 s | 1.6-3.2 s (count × T) | same | aperture / 2 | — |
| 50 / 60 Hz rejected by default | yes | **yes** | no while moving | mostly (120 / 200 Hz blind) | yes |
| 10 Hz square | mean | **mean** | ±2.4 V beat | mean | mean |
| +1 digit | no | **with filter / HiRes** | with filter | unknown | no |
| effort / risk | S / low-med | **M / low** | S / low | L / med-high | M / med |

## 8. Recommendation

### 8.1 Choice

**Option B**, with C' and E kept as candidates for later:

1. **Fixed integration presets, no OSR switching on motion:** FAST 0x15
   (10/s; "no 50 Hz rejection"), **MED 0x16 (5/s), the default**, SLOW 0x17
   (2.5/s). The default rejects 50 and 60 Hz like every bench meter's
   ≥ 1 PLC default, updates at the handheld rate (4-5/s), and stops the
   periodic-input aliasing the owner saw. It needs no line-frequency
   setting.
2. **One Keithley-style digital filter, off by default**, with a step window
   and a FILT chip. It replaces both averaging layers. The vocabulary and SCPI
   are what users of the Keithley 2000 / DMM7510 expect; the 34465A smoothing
   commands stay as aliases.
3. **+1 digit only with the filter (HiRes toggle = SLOW + MOV 8):** honest,
   like the BM869s stable mode, and in line with P3 (fast and raw by default)
   and UF-25 (extra digit as an opt-in).
4. Why not C: it reintroduces the 50 Hz hole on exactly the inputs that need
   rejection. Why not D yet: noise at OSR 2048 is unmeasured, and 120 Hz
   ripple aliases to DC. D stays the route to a ≥ 40/s bargraph if the noise
   test is favourable.

The cost: a step settles in ~0.4 s at the default instead of the stock's
~0.1-0.2 s. If the owner wants the stock's feel back, C' (0x16 ↔ 0x17) is the
safe AUTO; the stock's 0x15 ↔ 0x17 rule is not.

### 8.2 Migration from the current code

- `dmm.rs`: drop the DC moving / stable OSR switch for the V / mV / A / Ω
  rows; `Rate` → R22 code (0x15 / 0x16 / 0x17); `avg_cfg` becomes the filter
  config (state, type, count, window in counts derived from % of range); the
  per-range window tables go away; `ext_ok` keys on (filter on, code, n).
- `meter.rs`: remove `Smooth` for the AD1 functions (`CALC:SMO` maps onto the
  filter). Keep a display smoothing for AC / capacitance / Hz / temperature
  only if wanted (those have their own converters and are outside FR-05).
- Persisted settings (`ext_word`): rate 2 bits, filter state / type 2 bits,
  count 5 bits, window code. Old words map FAST → FAST, MED → MED (now 0x16),
  SLOW → SLOW, smoothing ON → filter ON.
- SCPI (`scpi.rs` header list): add `AVERage` and `APERture` / `NPLCycles`
  under `[SENSe:]<func>`, `SYSTem:LFRequency`; keep `RATE`, `RATE:PERiod?`,
  `CALC:SMO`.
- UI: the FILT chip and HiRes toggle; the rate chip shows F / M / S.
- AC+DC keeps its own phase logic (its DC phase at 0x15 is a separate
  decision, `acdc-*.md`).

### 8.3 Device tests before committing (no new hardware needed)

1. **H(f) per code.** The DM40 generator or the calibrator into DC V: a
   square or sine at 2.5, 5, 7.5, 10, 15, 20, 30, 40, 50, 60 Hz; log AD1 at
   0x15 / 0x16 / 0x17 for 30 s each. Expected under the sinc² model: leak
   0.41 at the first sidelobe of each code, nulls at the notch multiples,
   50 Hz leaking only at 0x15. This confirms or replaces the §0 table and the
   10 Hz explanation.
2. **Noise per code** (APD-DMM001 method: ≥ 1024 conversions, input shorted,
   V r0..r2, mV, Ω r1..r3): σ at 0x13 / 0x15 / 0x16 / 0x17, chopped and
   unchopped. It decides the +1 digit counts, option D and option E.
3. **Scale equality across OSR.** A fixed source (calibrator 1 V, 5 V; a
   resistor) at 0x15 / 0x16 / 0x17: means must agree within noise under one
   calibration.
4. **Settling.** Count the conversions after a step (generator square at
   0.2 Hz) and after a range change at each code.
5. **AD1 pre-filter.** Read the stock DC rows' R33 (SFT1) and check the board
   for an FTP-FTN capacitor (a possible Fluke-style analog filter, DS §12.4).

**Results, tests 1-4 and option E (device, 10242c3, 2026-10-24;**
`device-check-2026-10-21.md` "AD1 OSR characterization (10242c3)"**).** AD1
streams on DC V 6 V; the calibrator's 0 / 9.59 V square (no sine available),
0 / 1 / 5 V DC and its disabled output (no real short on the bench).

| | 0x15 (FAST) | 0x16 (MED) | 0x17 (SLOW) | 0x06 (E) | 0x07 |
|---|---|---|---|---|---|
| period, ms | 100.78 | 200.79 | 400.78 | 100.00 | 200.00 |
| leaks 0.405 at (beats near DC) | 10 Hz | 5 Hz | 2.5 Hz | 5 Hz | 2.5 Hz |
| 50 Hz / 60 Hz rejection | **36 dB** / >= 91 dB | >= 92 / >= 93 dB | >= 94 / >= 95 dB | >= 89 / >= 90 dB | >= 90 / >= 90 dB |
| noise sd, output off, uV (counts of 100 uV) | 40 (0.40) | **20 (0.20)** | 17 (0.17) | 35 (0.35) | 33 (0.33) |
| noise for equal time, uV x s^0.5 | 12.6 | **8.9** | 10.9 | 11.2 | 14.8 |
| offset vs 0x17 | -3 uV | -2 uV | 0 | **+22.22 mV** | +22.23 mV |
| gain vs 0x17 | within ~50 ppm (the calibrator's drift limits it) | | | | |
| first good conversion after a step or range change | 3rd: 0.2-0.3 s | 3rd: 0.4-0.6 s | 3rd: 0.8-1.2 s | 3rd: 0.2-0.3 s | 3rd: 0.4-0.6 s |

- Test 1: the dc-aliasing.md kernel (chopped = two sinc^2 conversions one
  period apart) predicts the p-p of all 45 streams (9 frequencies x 5 codes)
  within 1 %. Nulled inputs read their mean within +-0.2 mV.
- Test 2: 0x17 is barely quieter than 0x16 (not the white-noise 1/sqrt(2));
  unchopped 0x07 is noisier than chopped 0x16 at the same 5/s (1/f).
- Option E: the +22 mV that dcalias saw is an offset (the same at 0, 1 and 5 V),
  not gain. Over 9.5 min it moved 39 uV (0.4 count); corrected by alternating
  0x16 reference blocks it stays within 6 uV sd / 20 uV range. Untested: the
  other ranges and temperature.
- Verdict: **B** is supported on every count (0x16 as MED: mains and 10 Hz
  nulled, the quietest per unit time, same scale as 0x15 / 0x17, 3
  conversions to settle). **E** is feasible as FAST (0x15's noise, mains
  rejected), but only with a per-range offset subtraction, best a periodic
  0x16 zero.

## 9. Sources

Local (repository `docs/`):

- HYCON DS-HY3131 V10: `docs/DS-HY3131_EN.pdf`: p.4 features (5 Hz output
  rate), p.13 NMR (5 / 10 sps), §12.4 p.27 pre-filter, §13.1 pp. 30-31 AD1
  (chop, AD1OSR, comb-filter diagram).
- HYCON APD-DMM001 (EVA test tool): `docs/APD-DMM001_EN.pdf` §1 p.3 (RMS
  noise / ENOB, ≥ 1024 samples), §4 p.17 (Avr. Times).
- HYCON APD-DMM003 (configurations): `docs/APD-DMM003_EN.pdf` §1 p.6 (DCmV
  50/60 Hz pickup, pre-filter advice).
- HYCON APN-HY3131001 (SPI timing; nothing on rate): `docs/APN-HY3131001_EN.pdf`.
- `emu/hy3131/RESEARCH.md` §1 (OWON HY3131 OSR, after
  http://essentialscrap.com/owon_fix/) and §3 (stock R22 codes).
- `docs/history/acdc-stock-vs-opendm40.md` §4, `firmware/docs/user-feedback.md` (P3,
  UF-25, UF-30), `firmware/docs/device-check-2026-10-21.md` (40 Hz square in DC V).

Bench / system:

- Keysight 34401A data sheet 5968-0162EN, p.4:
  https://xdevs.com/doc/HP_Agilent_Keysight/34401A/34401A%20Multimeter%20-%20Data%20Sheet%205968-0162EN%20c20141113%20%5B6%5D.pdf
- 34401A User's Guide excerpts (digits ↔ PLC mapping, default 10 PLC;
  secondary): https://weble.upc.edu/asig/ie/P5/resolucio.pdf,
  https://pymeasure.readthedocs.io/en/latest/api/instruments/hp/hp34401A.html
- Keysight Truevolt data sheet 5991-1983EN, pp. 12, 22, 23:
  https://www.batronix.com/files/Keysight/DMM/34460-34470/34460-34470-Datasheet.pdf
- Keysight Truevolt help, "Math - Smoothing" (34465A / 70A; read via the
  search index, the page returns 403):
  https://rfmw.em.keysight.com/bihelpfiles/Truevolt/WebHelp/US/Content/__E_Features%20and%20Functions/Math-Smoothing.htm
- Keysight 3458A User's Guide 03458-90014: Ch. 3 pp. 58-61 (NMR, NPLC, APER,
  autozero), Ch. 4 p.124 (FILTER), App. A pp. 284-285 (NMR, reading rates):
  https://xdevs.com/doc/HP_Agilent_Keysight/3458A/03458-90014.pdf
- Keithley 2000 User's Manual 2000-900-01 Rev J: Table 2-2 (PDF pp. 24-26),
  §3 pp. 3-4..3-7, §5 SCPI tables, SYST:AZER (PDF p.195):
  https://download.tek.com/manual/2000-900_J-Aug2010_User.pdf
- Keithley application note 4452, "Five Ways to Shave Test Time":
  https://xdevs.com/doc/Keithley/Appnotes/4452_Five_Ways.pdf
- Keithley 2100 spec sheet (secondary mirror):
  https://docs.ampnuts.ru/eevblog.docs/Keithley/2100/SPEC-2100%20%28C%20-%20Sep%202011%29%28Customer%29.pdf
- Keithley DMM7510 Reference Manual DMM7510-901-01 Rev B: pp. 3-11..3-12,
  6-67..6-72, 6-85; FILT indicator (PDF p.44):
  https://download.tek.com/manual/DMM7510-901-01_B_May_2015_Ref.pdf
- DMM6500: reference manual page
  https://www.tek.com/en/tektronix-and-keithley-digital-multimeter/dmm6500-manual/model-dmm6500-6-1-2-digit-multimeter-3
  (not downloaded); pymeasure driver
  https://pymeasure.readthedocs.io/en/stable/api/instruments/keithley/keithleyDMM6500.html;
  element14 blog (secondary)
  https://community.element14.com/technologies/test-and-measurement/b/blog/posts/dc-voltage-measurement-accuracy-measuring-dc-voltage-with-high-accuracy-on-a-dmm6500
- Fluke 8845A / 8846A Users Manual Rev 3 (3/11): Ch. 1 (PDF pp. 20, 25-26), p.1-23, PDF p.47,
  3-9, 4-4..4-6, App. D:
  https://xdevs.com/doc/Fluke/8846A/884xa___umeng0300.pdf
- Fluke 8588A product page (secondary summary):
  https://fluke.com/de-at/produkt/kalibratoren/elektrische-kalibrierung/tischmultimeter/8588a
- R&S HMC8012 User Manual v06 (English part, PDF pp. 52-54):
  https://scdn.rohde-schwarz.com/ur/pws/dl_downloads/pdm/cl_manuals/user_manual/5800_4505_01/HMC8012_UserManual_de_en_06.pdf
- Siglent SDM3065X data sheet, PDF pp. 6, 11-12: https://siglentna.com/download/38118
- SDM3055 filter discussion (secondary):
  https://www.eevblog.com/forum/testgear/siglent-new-bench-dmm-sdm3055/msg786601/
- Rigol DM3068 specifications, PDF pp. 2-3, 15:
  https://assets.testequity.com/te1/Documents/pdf/rigol/Rigol-DM3068-Specification.pdf

Handheld and chips:

- Fluke 80 Series V Users Manual, pp. 38 (bar graph 40/s), 39 (HiRes),
  40 (Smooth), 51 (display 4/s):
  https://www.manualsdir.com/manuals/743810/fluke-80-series-v-85-v-83-v-87-v.html?page=38
  (and `?page=39`, `?page=40`, `?page=51`)
- Fluke 287 / 289 data sheet: https://www.farnell.com/datasheets/3557939.pdf;
  lygte review (secondary): https://lygte-info.dk/review/DMMFluke%20289%20UK.html
- Fluke 187 / 189 spec (Transcat): https://transcat.com/media/pdf/fluk189.pdf
- Keysight U1280 series data sheet 5992-0847EN, pp. 10, 17:
  https://www.farnell.com/datasheets/2352292.pdf
- Brymen BM860s data sheet:
  https://www.welectron.com/mediafiles/datasheets/brymen/Brymen_BM860s_Datasheet.pdf
- Hioki DT4282: manual https://www.hioki.com/download/30687 (403 to the
  fetcher); lygte review (secondary):
  https://lygte-info.dk/review/DMMHioki%20DT4282%20UK.html
- Gossen METRAHIT PM TECH / PM XTRA operating instructions 3-349-456-03:
  https://astat.pl/pliki/Energetyka_i_miernictwo/Multimetry/instrukcja-metrahit-pmtech-pmxtra-eupro-ba.pdf;
  METRAHit 2+ listing (secondary): https://uk.rs-online.com/web/p/digital-multimeters/6670201/
- Uni-T UT171A (secondary):
  https://www.eevblog.com/forum/testgear/uni-t-ut171a-pictures-and-first-impressions/msg590796/
- Fortune FS9922-DMM4 data sheet Rev 1.3:
  https://cdn.hackaday.io/files/21300911299200/FS9922-DMM4-DS-13_EN.pdf
- Cyrustek ES51932 (alldatasheet mirror):
  https://html.alldatasheetit.com/html-pdf/456303/CYRUSTEK/ES51932/1164/3/ES51932.html

Not retrieved (no accessible copy): Keithley 2010 / 2100 user manuals, Fluke
8508A / 8588A manuals, Fluke 189 manual, Keysight U1272A, Uni-T UT181A
manual, Siglent / Rigol user manuals (filter menus), Tektronix DMM4050 manual,
Cyrustek ES51997P.
