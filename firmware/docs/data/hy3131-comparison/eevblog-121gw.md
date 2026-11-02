# EEVblog 121GW firmware v2.05: decode and HY3131 configuration

Source: the official update images
[`EEVBlog2_05.zip`](https://www.eevblog.com/wp-content/uploads/2017/11/EEVBlog2_05.zip) (78,131 B,
SHA-256 `86c16b56d70c9bf743dd805e3241307cd055394ab764968884deeec461f4746e`) and, for the comparison with the
community tables, [`EEVBlog1_02.zip`](https://www.eevblog.com/wp-content/uploads/2017/11/EEVBlog1_02.zip)
(66,413 B, SHA-256 `927fc58070ea9abf12f0e6a3c10731dda617775c5107becf4f29cbb9d2ded7d4`). Board wiring from the
official circuit diagram (`121GW EEVBlog Circuit diagram.pdf`, also in
[tpwrules/121gw-re](https://github.com/tpwrules/121gw-re) `pdfs/`). Static analysis only. The images, the Ghidra
project and the listings are not in the repository (EEVblog's code); the steps below reproduce them.

Starting point: Thomas Watson's (tpwrules) [121gw-re](https://github.com/tpwrules/121gw-re), an IDA database of
v1.02 with named functions, the row tables `tables/hy_regtbl.h`, the AC frequency-response and 50 MΩ tables and
`routines/cal_decode.py`, and [121gw-88mph](https://github.com/tpwrules/121gw-88mph) (open firmware; its linker
script gives the memory map). The v1.02 names were used to find the same functions in v2.05; every value below is
read from the v2.05 image unless it says v1.02.

## Image format

- `EEVBlog2_05.zip` holds one file, `EEVBlog2_05.bin` (144,616 B, SHA-256
  `8f885f12f567c2cccd9862d9fe26f127e2a674c9afbce9f7a38d441b3f396722`); v1.02: `EEVBlog1_02.bin` (117,848 B,
  identical to `binaries/EEVBlog-102.bin` in 121gw-re). The meter takes it from the SD card as `EEVBlog.bin`.
- Plain Cortex-M3 image, no header, no scrambling, no encryption: the file starts with the vector table
  (SP 0x20001240, reset 0x08029175). It is linked at **0x08006000**, behind the 24 KB bootloader
  (0x08000000..0x08005FFF, not in the package): `main` (0x08024B40) first relocates the vector table to
  0x08000000 + 0x6000, and the v1.02 IDA address of `hy_regtbl_ohms` (0x08022010) is file offset 0x1C010 + 0x08006000.
- Reproduce: Ghidra `BinaryLoader`, base 0x08006000, `ARM:LE:32:Cortex`, disassemble from the vector table and the
  odd code pointers; mark the image block read-only so literal-pool pointers resolve.

## MCU and memory

STM32L152ZD (part from the schematic, U4): flash from 0x08000000, SRAM 0x20000000 (48 KB in the 88mph linker
script), data EEPROM 0x08080000 (calibration and settings, below). GPIO F carries the HY3131 bus.

Clocks: `SystemInit` (0x08025F70) starts PLL = HSE × 12 / 3 (HSE crystal X2 4 MHz → 16 MHz), but the board
init (0x0801BF1A, argument 0) then switches SYSCLK to MSI range 6 (**4.194304 MHz**) and stops the HSE. HSE
(4 MHz, no PLL) is selected only around SD-card access, then MSI again. AHB and APB1 prescalers stay at 1, so TIM2 / TIM3 run at SYSCLK.

- **TIM2** (0x0801C486): PSC 260, ARR 160, update interrupt (vector 44) → tick = 261 × 161 / 4.194304 MHz =
  **10.019 ms** (10.505 ms while the HSE runs). The "ticks" below are these. The ISR sets a tick flag (main
  loop: counter-gate check and other periodic work), a flag every 10 ticks (100.2 ms; battery and thermistor
  ADC, alternating), and every 50 ticks (501 ms) starts a Bluetooth packet.
- **TIM3** (0x0801C4D4): PSC 260, ARR 15 → 0.996 ms.

## HY3131 driver

Bit-banged SPI on GPIOF, the same code as v1.02 (`send_data_to_hy3131` 0x0800C03C, `read_registers_from_hy3131`
0x0800C0C0): PF1 = CS, PF2 = MOSI, PF0 = SCK, PF3 = MISO; PF3 is also polled as the interrupt line
(GPIOF IDR bit 3 high → read). First byte `addr<<1` (write) or `addr<<1 | 1` (read).

Every SPI access in the image goes through those two functions; their callers are:

| caller | access |
|---|---|
| 0x0800614E (main loop, when PF3 is high) | read R00..R33 (52 bytes), write R1E = 0 (INTF), then dispatch AD1 / RMS / peak-hold / CT |
| 0x0800B994 (row writer) | R20..R33, 20 bytes from the row table |
| 0x0800BEAC | R1E..R1F = 00, INTE: 0x04 DC, 0x05 AC (AD1 + CT), 0x11 1 ms peak (RMS + CT), 0x01 Hz / ms / duty / capacitance |
| 0x0800BF8C (counter restart) | R20 with ENCTR = 0, busy-wait, R1B..R1D = 00 + CTA preset, read back R1B..R1D, R20 with ENCTR = 1 |
| 0x0800A7FC (1 ms peak) | R29 with ENPKH = 0, wait, R29 back (re-arms the peak hold after each RMS interrupt) |

So R22 (OSR), R30 (ACC) and every other analog field come only from the rows. Nothing writes R34..R37, and
no command path reaches the HY3131 except through these callers.

### Row selection (0x0800B994)

Mode numbers follow the 121gw-re enum (0 Low-Z, 1 DCV, 2 ACV, 3 DCmV, 4 ACmV, 5 temperature, 6..8 Hz / ms /
duty, 9 Ω, 10 continuity, 11 diode, 12 capacitance, 13..15 AC VA, 16..21 AC / DC µA, mA, A, 22..24 DC VA).

| mode | table (flash, rows) | selection |
|---|---|---|
| Low-Z | DCV row 3, or ACV row 3 with R2A \|= 0x20 and R2E = (R2E & 0x7F) \| 0x20 when Low-Z measures AC | AC / DC flag |
| DCV | 0x080288A0 (4) | range |
| ACV | 0x080289C8 (4); 1 ms peak 0x08028904 (4); DCV rows in the DC phase of AC+DC | LPF flag → AC rows; else 1 ms peak flag |
| DCmV, temperature | 0x08028F1C (2); temperature uses row 0 (50 mV) | range |
| ACmV | 0x08028F44 (2); 1 ms peak 0x08028F6C (2) | as ACV |
| Hz, ms, duty | 0x08028FBC | — |
| Ω | 0x080285BC (7) | range; 50 MΩ: R2A bit 3 (PA0 P) cleared while EEPROM byte 0x26 ≠ 0xA5 |
| continuity | 0x08029150 | — |
| diode | 0x08028F94 | range (both rows identical) |
| capacitance | 0x08028748 (6), or 0x080286D0 (6) when PF8 reads low at boot | range |
| AC A | 0x08028D1C (3) | range |
| DC A | 0x08028D94 (3) | range |
| AC / DC VA, burden | A: 0x08028D1C / 0x08028D58 (3); V: ACV / DCV rows | V or A phase flag, separate V and A range |

**PF8 strap.** At boot (0x0800C28C) PF8 is made an input with the internal pull-up and sampled 20 times; fewer
than 10 high samples set a board-variant flag, then PF8 goes back to analog. In the circuit diagram PF8 (pin 20)
is the net BURDEN_VOLT, which reaches rotary switch T1 pin 35 only through R92, marked "x" (not fitted). T1
connects pin 35 to A_OUT in the µA position and to the R33 / R43 junction in the mA / A position (both tied to
AGND through the shunts), and to nothing in the others. With R92 absent PF8 floats, reads high through the
pull-up, and the firmware uses the default tables; with R92 fitted, PF8 would see the shunt node only when the
meter is switched on in µA or mA / A. No code reads PF8 after boot, so the net never measures a burden voltage (that
feature uses the V input, below). The PF8-low variant changes:

- capacitance: the second row table (100 nF and 100 µF comparator thresholds), a gain divisor of 10⁴ instead
  of 10⁵ on 100 µF, and a 1.5 s instead of 2.0 s counter timeout on 100 nF;
- VA modes (0x08008ED0, AC and DC): each range has two sets of correction tables for the reading-dependent
  correction, e.g. AC µVA range 0 at 0x080260E4 / 0x08026204, and the flag picks the second; on DC VA range 2
  the correction is subtracted for negative values instead of added;
- DC VA: no fixed sign-dependent offsets (below, EEPROM byte 0x26);
- high-voltage diode: +500 counts above 14000 counts.

v2.04 has the same PF8 test, read by the same seven functions; v1.02 has none. Which production boards pull
PF8 low is a hardware question the image cannot answer: the published diagram has R92 not fitted, and neither
the manual's firmware table, the EEVblog forum threads, nor tpwrules' repositories (121gw-88mph does not use
PF8) name a board revision other than the range-switch shim and indent ring and the PCB thickness change.

### v1.02 → v2.05 row changes

The other 32 table rows are byte-identical. Decoded with the [comparison](../../hy3131-firmware-comparison.md) decoder:

| row | v1.02 → v2.05 |
|---|---|
| Ω 50 / 500 Ω | R22 0x15 → 0x13: OSR 8192 → 2048 |
| Ω 5 kΩ | R22 0x13 → 0x11: OSR 2048 → 512 |
| Ω 50 / 500 kΩ | R22 0x14 → 0x12: OSR 4096 → 1024 |
| Ω 5 MΩ | R22 0x14 → 0x17: OSR 4096 → 32768; R2E 0x80 → 0x00: P9 opened |
| Ω 50 MΩ | R22 0x15 → 0x14: OSR 8192 → 4096 |
| DC A (all three; not the VA table) | R33 0x2C → 0x20: SFT1 open → 100 k. v1.02 had one DC A table for A and VA |
| ACV 5 V | R32 0x08 → 0x38: OP2 + input SENSE → AGND (OP2 stays off) |
| capacitance, PF8 variant (new) | 100 nF: R21 VDSC11 / VDSC5 → VDSC9 / VDSC8; 100 µF: → VDSC10 / VDSC8 |
| Low-Z AC (new run-time edit) | PA9 P → F, PA1 F added |

## Runtime logic

- **Mode / range change** (0x0800D2D0): clear the averaging state; settle count; max counts; then the 20-byte
  row, INTE, counter restart; calibration load (offset / gain, the stored zero of current range 0, the
  HY3131 clock, the AC coefficients); then GPIO: the AC path muxes and the range relays (0x0800FBE8). No
  read-back.
- **Reading** (0x08006342): one reading is the mean of a block of AD1 results: 16 in every mode except Ω
  (8; 5 MΩ: 1). With the AD1 period of each row (OSR, chop) that gives:

  | rows | AD1 period | reading |
  |---|---|---|
  | DCV, Low-Z DC, ACmV | 25 ms | 400 ms |
  | DCmV 50 mV, temperature | 50 ms | 800 ms |
  | DCmV 500 mV, continuity | 3.1 ms | 50 ms |
  | ACV 5..500 V | 6.25 ms | 100 ms |
  | ACV 600 V, Low-Z AC | 1.56 ms | 25 ms |
  | AC / DC µA, mA, A, diode | 12.5 ms | 200 ms |
  | Ω 50 / 500 Ω; 5 kΩ; 50 / 500 kΩ; 5 MΩ; 50 MΩ | 25; 6.25; 12.5; 400; 50 ms | 200; 50; 100; 400; 400 ms |

  Hz / ms / duty and capacitance readings come from the counter interrupt instead.
- **Settle** (0x0800DB48): after a change the autorange is held for N readings: DCV 2 (0.8 s), ACV 3
  (0.3 s; 600 V 75 ms), ACmV 4 (1.6 s), DCmV 1 (0.8 s / 50 ms), Ω 1 (50..200 ms; 5 MΩ and 50 MΩ: 2, 0.8 s),
  continuity 0, Hz 0, capacitance 1 counter result (v1.02: 2 on 10 / 100 nF), Low-Z DC 5 (2.0 s), AC+DC 1,
  1 ms peak 5, otherwise 3 (A: 0.6 s); current ranges with burden display 3 (A phase) / 0.
- **Autorange** (0x0800DE24, once per new reading): up when |reading| > 1.1 · full scale (1.083 · full scale
  while the overrange flag of the previous reading is set; diode: > full scale), down when < 0.1 · full scale
  (capacitance 10 mF range: 0.01; 10 A range: 0.5); never in manual range or calibration mode. Ω: after an
  up-range a down-range is blocked for 100 ticks (1.0 s). Capacitance (0x0800EC28, 0x0800B540): no counter
  result within 200 ticks (2.0 s; 100 nF on PF8-low boards 1.5 s; 10 mF and manual 10 / 100 nF 3.0 s) →
  jump from 10 / 100 nF to 1 µF and from 1..100 µF to 10 mF.
- **Ranges** (count per mode, initialised data at 0x20000000): Low-Z 1, DCV 4 (5 / 50 / 500 / 600 V), ACV 4,
  DCmV 2 (50 / 500 mV), ACmV 2, temperature 1, Hz 5, ms 3, duty 1, Ω 7 (50 Ω..50 MΩ), continuity 1, diode 1
  (the 15 V diode test is range 1, chosen by key, not autorange), capacitance 6 (10 nF..10 mF), each VA mode 4,
  µA 2 (50 / 500 µA), mA 2 (5 / 50 mA), A 3 (500 mA / 5 A / 10 A).
- **Full scale** (flash table 0x08025A3C, 26 modes × 7 ranges, counts): 50000 on most ranges; 6000 on
  DCV / ACV 1000 V (displayed as 600.0 V) and Low-Z; 10000 on capacitance, temperature r0 and 10 A; 25000 on most VA
  ranges; diode 30000, high-voltage diode 15000; Hz 99999.
- **Software averaging** (0x0800B754): moving average of the last N readings, restarted when a reading
  differs from the average by more than W counts. DCV and diode N 2 / W 2; DCmV 10 / 5; AC 10 / 20; AC+DC 10 / 20;
  Ω: 50 Ω 10 / 100, 500 Ω and 500 kΩ 10 / 20, 5 kΩ 10 / 10, 50 kΩ 10 / 20, 5 MΩ 5 / 10, 50 MΩ 20 / 100;
  capacitance 10 / max(20, value / 50) (10 mF: value / 10), and readings below 21 counts on 1..100 µF shown as
  0. DC A: a three-reading average restarted beyond ±10 counts (0x0800B888).
- **Offset / zero**: no auto-zero row. Reading − calibrated offset, divided by the calibrated gain; on range 0
  of each current mode a stored zero (EEPROM, flag 0xA5 + 4 bytes at 0x4D0 / 0x4E0 / 0x4F0 / 0x4F8 …) is
  subtracted too.
- **AC**: the normal AC rows read the external AD8436 true-RMS converter on PB7 with AD1 (see Board wiring).
  The HY3131 RMS block runs only in the 1 ms peak rows, and its result is not used: the RMS interrupt
  handlers are empty. The LPF key switches the analog path, not a row.
- **1 ms peak** (0x0800A7FC): the peak-hold block follows AD2 (PKHSEL 00), OSR2 1024 at SYSCLK / 2 =
  2400 samples/s (0.417 ms per sample) on the V rows, OSR2 64 = 38400 samples/s (26 µs) on the mV rows.
  INTE 0x11 enables the RMS and counter interrupts; the RMS interrupt comes every OSR4 AD2 samples (datasheet:
  LPF rate = AD2 rate / OSR4): 5 V (OSR4 128) 53.3 ms, 50..600 V (4096) 1.707 s, mV (4096) 106.7 ms; on the DM40
  the LPF period measured half the formula ([scope feasibility](../../dmm-input-scope-feasibility.md)). The
  counter interrupt (gate 0.427 s, below) also arrives. On every HY3131 interrupt the firmware reads PKHMAX
  (R11..R13), clears ENPKH, waits, sets it again (re-arm), calibrates the value and shows the largest since the
  mode was entered; one interrupt counts as one reading (settle: 5).
- **AC frequency-response correction** (0x080124F4, below): applied to AC V, AC mV and AC A (µA, mA) for input
  ≥ 51 Hz and < 55000 counts, from the measured frequency and four per-range calibration coefficients.
- **Counter gate** (0x0800ED64, every tick): in AC V / mV / A, when no counter interrupt arrives for 101 ticks
  (1.01 s; Low-Z AC 2001 ticks, 20.05 s; v1.02 500), the CTA preset is swapped between 0x8000 and 0xE000 and
  the whole row set re-sent. Each counter interrupt restarts the count.
- **Frequency timeout** (TIM2 ISR): with no counter interrupt for 2001 ticks (20.0 s) in AC V / mV / A, or 501
  ticks (5.0 s) in Low-Z AC, the frequency used by the AC correction is cleared; a reading below 1000 counts
  (600 V: 100) clears it at the next tick (0x08006C7A). Hz / ms / duty show 0 after 301 ticks (3.0 s) without
  a result.
- **AC+DC** (ACV with the AC+DC flag): 5 readings on the ACV row, then 3 on the DCV row of the same range
  (row and calibration reloaded at every switch); displayed value √(AC² + DC²) of the last reading of each,
  overload when either exceeds 60000 counts.
- **Burden voltage / VA**: burden voltage is a sub-display of the current ranges (string "Burden" in the
  image). The firmware alternates the A rows with the DCV / ACV 5 V rows (4 readings per phase on DC A with
  burden), reloading row and calibration at each switch; the V reading comes through the normal V divider,
  which is why the [manual](https://www.eevblog.com/files/EEVblog-121GW-Manual.pdf) (pp. 42–43) asks for an
  extra lead from the active current terminal to the V input.
  No code reads PF8 (net BURDEN_VOLT) after boot: it is analog afterwards, and the ADC converts only
  channels 4 and 5.
- **Capacitance** (0x0800B33C): every range is counter-based. The rows make a relaxation oscillator: OP3
  drives the power bus to VDS15 (32/36 VDDA) or VDS3 (4/36) by the comparator state, through PA7 + PA2
  (R13 1.11 MΩ ∥ R29 10 MΩ) on 10 / 100 nF, PA5 + PA2 (R21 10 kΩ ∥ R29) on 1 µF, PA4 + PA2 (R26 1 kΩ ∥ R29) on
  10 / 100 µF; the comparator watches PB0 against VDSC11 / VDSC5 (22/36 and 10/36 VDDA; PF8-low rows: 100 nF
  VDSC9 / VDSC8, 100 µF VDSC10 / VDSC8). The 10 mF row drives the bus directly (SMODE 0x1A, PA4 only) and
  compares OP1O. The counter input is the comparator; CTC counts SYSCLK while it is high, CTB counts cycles.
  Reading D (counts) = (CTC / CTB · m − offset) / g, with m = 100 on 1..100 µF, else 1; offset (10 / 100 nF
  only) and g (stored / 10⁵; 100 µF on PF8-low boards / 10⁴) are per-unit calibration. AD1 is configured in the
  10 / 100 µF rows, but the value comes only from the counter. Timing: after each result the counter restarts
  (R20 ENCTR off, a busy-wait of 10000 loop passes, 14..19 ms at 4.19 MHz, CTA = 0xE00000, ENCTR on); the gate
  is (2²⁴ − 0xE00000) / 4.9152 MHz = **427 ms**, and the counter then runs to the end of the current
  oscillator cycle. So one reading takes 0.44 s plus up to one oscillator period, and at least one period
  when that is longer. The high time per cycle is t_high = (g · D / m + offset) / 4.9152 MHz; with the one
  published calibration (121gw-re `binaries/CAL - Iagash.BIN`) that is 3.0 ms per nF on 10 / 100 nF (31.6 ms
  at 10 nF, 303 ms at 100 nF), 30 ms at 1 µF, 35 ms at 10 µF, 349 ms at 100 µF and 2.66 s at 10 mF.
- **Temperature**: K thermocouple on the DCmV 50 mV row; flash table 0x08025CF8 (µV every 10 °C from
  −210 °C, 0 µV at index 21); valid −5891..54139 µV (−200..1350 °C), outside it the open / over flag. In
  temperature mode PH2 (TEMP_CTL) is high (0x0800FDB0) and feeds the V input from VDD through 20 MΩ (U16
  switch Z). There is no separate open-lead code: an open input is pulled far above 54139 µV and takes the
  over-range branch of the µV→°C conversion (0x08013C40), so it shows the same overload as > 1350 °C; below
  −5891 µV the negative overload.
- **Cold junction** (0x0801381E, every 200 ms): NTC1 10 kΩ to VSS with R38 10 kΩ to VDD, net THERMISTOR on
  PA4 = ADC1 channel 4 (12 bit, 384-cycle sample; PA5 / channel 5 is LOW_BAT, read on the other 100 ms
  slot). Raw values below 51 are ignored; a moving average of the last 10 is looked up in the table at
  0x08028DD0 (descending ADC counts at −30, −25, … 85 °C: `3758, 3672, 3570, 3451, 3315, 3163, 2997, 2819, 2631,
  2437, 2242, 2048, 1859, 1678, 1508, 1349, 1203, 1070, 950, 842, 746, 661, 585, 518`): with t[k] ≤ adc <
  t[k−1], T = (5 (k − 1) − 30) °C + 5 °C · (t[k−1] − adc) / (t[k−1] − t[k]), in 0.01 °C; accepted 518..3672
  (−25..85 °C), otherwise the reading is flagged invalid. The calibration factor at EEPROM 0x2E0 (0.01 °C, ±1000)
  is added; the thermistor step of calibration stores 23.00 °C − computed. The result (0.1 °C, −20.0..100.0 °C)
  becomes µV through the thermocouple table (0x08013CF8). Same table and formula as v1.02 (121gw-re
  `calc_internal_temp_core`).
- **Ω non-linearity** (only while EEPROM byte 0x26 = 0xA5, see EEPROM markers): 5 MΩ: a fixed staircase, 1 count per 1000 up to
  15 counts at 15000..36000, back to 0 at 50000; 50 MΩ: the 121gw-re `cal_ohms_50M_factor_tbl` (input 0..60 k
  counts in 5 k steps → 0, 37, 65, 85, 100, 100, 100, 87, 70, 40, 0, −37, −65 %) times the per-unit
  factor at EEPROM 0x178 (accepted −3000..3000). Not applied in calibration mode.

## AC frequency-response correction

Input frequency tables (Hz): ACmV `50, 400, 1000, 10000, 30000, 50000, 70000, 90000, 100000`; ACV 500 V / 600 V,
µA, mA `50, 400, 1000, 3000, 5000, 7000, 10000`. Row 0 is used at ≤ 5000 counts, row 1 above (percent of the
calibration coefficient, linear between points):

| range | row 0 | row 1 |
|---|---|---|
| ACmV 50 mV | 0, 3, 0, −2, 1, 18, 47, 81, 100 | 0, −5, −1, 7, 40, 70, 90, 98, 100 |
| ACmV 500 mV | 0, 1, −1, −2, −1, 15, 45, 82, 100 | 0, −2, −1, 3, 19, 40, 65, 88, 100 |
| ACV 500 V | 0, 3, 10, 52, 77, 90, 100 | 0, 2, 9, 52, 77, 90, 100 |
| ACV 600 V, Low-Z AC | 0, 2, 9, 51, 77, 90, 100 | 0, 2, 9, 51, 77, 90, 100 |
| AC µA | 0, −10, 3, 20, 35, 55, 100 | 0, −20, −8, 2, 16, 43, 100 |
| AC mA | 0, −10, 3, 20, 40, 55, 100 | 0, −15, −4, 7, 20, 47, 100 |

The ACmV, µA and mA rows equal the v1.02 tables in 121gw-re `tables/mcafdoc_tbl.h`; the ACV 500 V and 600 V
rows differ (v1.02: 0, 2, 22, 78, 98, 99, 100 and 0, 2, 22, 85, 95, 99, 100). AC A (5 / 10 A ranges) has no
correction. The interpolated percentage scales the "decade" coefficient (applied at 5000 counts; 600 V: 600) and
the "full-scale" one (45000; 600 V: 5400); the correction is interpolated between the two points by reading
and subtracted.

ACV 5 V and 50 V (0x080129EE) use two bands: ≤ 20 kHz on `50, 400, 1000, 3000, 5000, 7000, 10000, 15000, 20000`
with the 20 kHz coefficients, and 20..100 kHz on `20000, 30000, …, 100000` interpolating between the 20 kHz and
100 kHz coefficients:

| range | ≤ 20 kHz decade / full | 20..100 kHz decade / full |
|---|---|---|
| 5 V, EEPROM 0x1F = 0xA5 | 0, −10, 14, 80, 110, 120, 125, 115, 100 / 0, −3, 12, 60, 83, 92, 98, 100, 100 | 0, 5, 13, 23, 36, 50, 66, 83, 100 / 0, 5, 12, 22, 33, 47, 63, 80, 100 |
| 5 V, otherwise | 0, 1, 18, 80, 95, 105, 109, 105, 100 / 0, −1, 14, 60, 83, 92, 97, 99, 100 | 0, 5, 13, 23, 36, 50, 66, 83, 100 / 0, 4, 10, 18, 29, 43, 60, 78, 100 |
| 50 V | 0, −5, 15, 90, 100, 110, 107, 105, 100 / 0, −5, 10, 57, 80, 89, 96, 99, 100 | 0, 5, 13, 23, 36, 50, 66, 83, 100 / 0, 4, 8, 16, 27, 41, 58, 77, 100 |

While EEPROM byte 0x1F ≠ 0xA5 (written by the ACV 5 V calibration step), ACV 5 V also subtracts a fixed
correction (0x080070CC) at frequencies `50, 400, 1000, 2000, 3000, 5000, 7000, 10000, 20000, …, 100000` (17
points): below 5000 counts `0, 7, 36, 92, 152, 171, 190, 206, 199, 199, 199, 195, 231, 237, 237, 264, 269`
counts, above it toward `0, 41, 294, 680, 1126, 1552, 1716, 1788, 1867, 1884, 1912, 1931, 1949, 1962, 1970, 1981,
1979` counts at full scale.

## Calibration storage

Calibration lives in the MCU data EEPROM (0x08080000 + offset), big-endian, written byte by byte with up to 10
retries. The layout matches 121gw-re `routines/cal_decode.py` (a 1200-byte SD-card copy of the same block,
`cal_save_to_sd` / `cal_load_from_sd` in v1.02). Per-mode base offsets (flash table 0x08028EE8, 16 bytes per
range: offset int32, gain uint32 / 10⁶ (capacitance / 10⁵), then the four 16-bit AC coefficients at +8..+0xE):

| mode | base | mode | base |
|---|---|---|---|
| Low-Z (DC: DCV r3, AC: ACV r3) | 0x70 + 0x30 / 0x30 + 0x30 | Ω | 0x110 |
| DCV | 0x70 | continuity | 0x110 + 0x10 (= Ω 500 Ω) |
| ACV | 0x30 | diode | 0x180 |
| DCmV | 0xB0 | capacitance | 0x190 |
| ACmV | 0xD0 | AC µA / DC µA | 0x1F0 / 0x210 |
| temperature | 0xF0 | AC mA / DC mA | 0x230 / 0x250 |
| Hz (HY3131 clock, 0.1 Hz, at 0x104) | 0x100 | AC A / DC A | 0x270 / 0x2A0 |
| 1 ms peak | 0x2D0 | AC VA (µ, m, —) | 0x300 / 0x340 / 0x380 |
| internal thermistor | 0x2E0 (16-bit, ±1000) | DC VA (µ, m, —) | 0x3C0 / 0x400 / 0x440 |

Defaults in code: HY3131 clock 49,152,050 (4.915205 MHz) when the stored value is outside 40,000,000..60,000,000;
thermistor factor 0 outside ±1000; 50 MΩ factor 0 outside −3000..3000; AC coefficients 0 outside ±20000
(20 kHz pair) / ±10000 (100 kHz pair). The per-range offset / gain loader has no default.
### EEPROM markers 0x1F and 0x26

Both are read once at power-up (0x08024DE0) into a flag "byte ≠ 0xA5", and each is written (0xA5, up to 10
tries) only by one calibration step: 0x1F by ACV 5 V (0x080104D4), 0x26 by Ω 50 MΩ (0x08010F28). Nothing else
writes them; v2.04 has the same code, v1.02 neither marker. What they gate:

| marker | = 0xA5 | ≠ 0xA5 |
|---|---|---|
| 0x1F | ACV 5 V frequency-response table "0x1F = 0xA5" | the other 5 V table, plus the fixed 17-point correction (0x080070CC) |
| 0x26 | 50 MΩ row with PA0 P closed; 5 MΩ / 50 MΩ non-linearity corrections | 50 MΩ row with PA0 P open (R2A bit 3 cleared); no Ω correction; on PF8-high boards, fixed DC VA offsets by range and by the signs of the two readings (e.g. DC µVA range 0: ±200 counts) when the reading is outside ±2500 counts (0x080087F8) |

After the 50 MΩ step the RAM flag is forced to "≠ 0xA5" until the next power-up, so the rest of that
calibration session runs with PA0 P open and without corrections. So a marker is 0xA5 exactly when the unit's
ACV 5 V / 50 MΩ calibration step was run by a firmware that writes it (v2.04 and v2.05 do, v1.02 does not; other
versions not examined); otherwise the firmware falls back to the fixed corrections
and the PA0-open row. The firmware table in the
[manual](https://www.eevblog.com/files/EEVblog-121GW-Manual.pdf) mentions neither marker. The code never
derives either marker from a GPIO.

## Board wiring (circuit diagram)

Names: VIN = the V/Ω jack after PTC3 1.2 kΩ + R16 1 kΩ; RBUS = the node shared by the PA resistors and RLU,
which reaches rotary switch T1 pins 14 / 16 through bead FB4; CBUS = the node of the compensation capacitors,
joined to net COM (T1 pin 8) by R14 0 Ω. The COM jack is net GND = AGND. There are no relays: ranges are the
HY3131 switches, T1 and the analog switches below.

| HY3131 pin | connection |
|---|---|
| PA9 | R9 30 kΩ + C13 6 pF in series to VIN (AC compensation) |
| PA8 | R11 10 MΩ to VIN (divider top) |
| PA7 / PA6 / PA5 / PA4 | R13 1.11 MΩ / R15 101 kΩ / R21 10 kΩ / R26 1 kΩ to RBUS; to CBUS: C16, C17 (not fitted) / C19 36 pF + C20 560 pF / C22 750 pF + C23 5.6 nF / C24 10 nF + C26 56 nF |
| PA3 | C27 100 nF to PB2 |
| PA2 / PA1 / PA0 | R29 10 MΩ / R32 1 kΩ / C30 27 nF to RBUS |
| RLU | RBUS |
| RLD | R79 + R42 49.9 Ω each to net RLD; Q3 + Q5 (2SC3876 as diodes, anti-series) from RLD to AGND |
| PB0 | R18 + R19 + R20 (3 × 300 kΩ) to the sense node P9; U16 Y adds C45 1.5 nF to AGND |
| PB1 | A_IN (current amplifier output, below) through R30 0 Ω |
| PB2 | C27 to PA3; R41 to AGND not fitted |
| PB3 | R37 10 kΩ to AGND |
| PB4 | C51 1 µF from OP1O, R78 1 MΩ to AGND (0.16 Hz high-pass) |
| PB5 | not connected |
| PB6 | ZD1 ADR3412 1.2 V reference, C61 100 nF |
| PB7 | AD8436 output (EXT_AC_OUT) through R44 + R134 0 Ω |
| PB8 | R27 47 kΩ to AGND |
| OP1N / OP1O | R24 90 kΩ from OP1O, R25 10 kΩ to AGND (×10); OP1O = net AC |
| OP2N / OP2O | tied together; OP2O drives C38 |
| FTP / FTN | C21 27 nF |
| CNT | C32 2.2 µF from RLD1 (U9 X1), R45 1 MΩ to AGND |
| REFO / ACM | C33 100 nF to AGND / C28 100 nF to AVSS |
| XIN / XOUT | X1 4.9152 MHz, C12 / C14 15 pF, R8 1 MΩ |
| CS / CK / DI / DO | MCU PF1 / PF0 / PF2 / PF3 |

**Rotary switch T1** (contact pairs per position; positions named by the mode line each one pulls low):

| position | pairs |
|---|---|
| off | 19–23 (RESET to VSS) |
| Low-Z | 1–7: PTC1 + PTC2 (2 × 1.5 kΩ) from the jack to AGND as the load; 11–14: RBUS to AGND; 17–18 (all positions on): battery |
| V | 2–8: CBUS to AGND; 11–14: RBUS to AGND |
| mV / temperature | 3–9: net R (jack via PTC4 1.2 kΩ + R17 1 kΩ) to P9; 12–15: RLD to VIN |
| Hz / duty, Ω / continuity / diode / capacitance | 3–9; 36–37: VIN to R_RLD (→ R82 100 Ω → U9 Y → RLD) |
| VA | 4–10: µA/mA jack (after F1) to junction J; 13–16: RBUS to AGND |
| µA | 5–10: jack to A_OUT; 33–35: A_OUT to R92 (not fitted) |
| mA / A | 6–10: jack to J; 34–35: J to R92 (not fitted) → BURDEN_VOLT (PF8) |
| end | 22–32 (RESET to VSS) |

**Current inputs.** µA/mA jack → F1 440 mA / 1000 V → T1 pin 10, clamped to AGND by BD1 (DF10S bridge with
± shorted). Shunt chain A_OUT – R33 100 Ω – J – R43 1 Ω – SHUNT 0.01 Ω – AGND; 10 A jack → F2 10 A / 1000 V →
SHUNT. Burden without fuse: µA 101.01 Ω, mA and VA 1.01 Ω, A 0.01 Ω. A_OUT → R126 100 Ω → U8 MAX4238 ×10
(R106 90 kΩ / R46 10 kΩ; shutdown SHDN_A = PE12) → R104 100 Ω; U11 74HC4053 picks ×10 or direct onto A_IN
(control A_CTL = PE5 through inverter Q16). Fuse check: U5 TLC272 compares each jack (before its fuse; R54 +
R55 / R61 + R62 9.4 MΩ, 30 MΩ pull-up to VDD, 10 nF) with REFO (R59 100 kΩ) → FUSE_mA (PE10), FUSE_A (PE11).

**Protection.** V/Ω jack: PTC3 + R16 to VIN, PTC4 + R17 to the sense net R, PTC1 + PTC2 to the Low-Z load;
MOV1 VIN – X, MOV3 X – AGND, MOV2 X – R (S05K575 each); D13 SM6T22CA from P9 to AGND; P9 also has R35 1 MΩ to
TEMP_OL. Diode test (U9 HEF4053, 15 V): RT9271 boost U10 (enable ENB = PF6) or VDD via D9 powers U9; D7 / D8
1N4007 clamp U9's Y node; U9 selects RLD or RLD1 (→ C32 → CNT), and RLD or the boosted rail through R39
1 kΩ (control FRE_CTL = PA12 and LED_CK = PE15 through inverters Q12 / Q11). U14 with Q14 / Q15 (DCmV_CTL
PD7, VA_CTL PA11) is not fitted.

**Analog switches.**
- U15 HEF4052 (A = CTL_D PF4, B = CTL_E PF5): X → RMS_IN from X0 TP7, X1 AC, X2 1kHz (X3 open); Y → LPF from
  Y2 AC. So AC: X1; LPF key: X2 with Y2.
- U16 HEF4053 (A = CTL_A PD0, B = CTL_B PC11, C = TEMP_CTL PH2 through inverter Q13): X: PB7 via R105 (not
  fitted); Y: C45 1.5 nF on PB0; Z: R113 + R114 (20 MΩ) from VDD to TEMP_OL.

**AC path.** OP1O (net AC) → U15 → RMS_IN → C85 1 µF (R132 10 MΩ to IGND, 0.016 Hz) → U17 AD8436 (C_AVG C79
10 µF ∥ C78 470 pF, CCF C80 100 nF, output C86 4.7 µF, output buffer as follower, VCC +4.0 V) → EXT_AC_OUT →
PB7. LPF network (only with the LPF key): AC → R28 4.02 kΩ → N2 (C50 100 nF to AGND) → R23 1.21 kΩ → N1 (C38
470 nF to OP2O) → R22 2.49 kΩ → net 1kHz (C25 100 nF to AGND) → RMS_IN. Single sections: 396, 280, 639 Hz.
The whole ladder: −3 dB at 47.5 Hz (−31.8 dB at 1 kHz) if OP2O holds AGND, 152 Hz (−18.9 dB at 1 kHz) if
OP2O is high-impedance. OP2 is off in every row, and the datasheet does not state a disabled OP2's output,
so which of the two applies is unknown.

**Other.** NTC1 / R38 on MCU PA4, LOW_BAT on PA5; PB6 reference ADR3412 from B+ (R94 0 Ω).

## Bluetooth

USART2 at 19200 baud to the BLE module. While Bluetooth is on, the TIM2 ISR fills a 19-byte packet at tick 40
of every 50 and sends it at tick 50 (one packet per 501 ms), byte by byte from the TX interrupt. The layout is
the one in libsigrok [`eev121gw.c`](https://github.com/sigrokproject/libsigrok/blob/master/src/dmm/eev121gw.c):
start byte 0xF2, serial number, main mode / range / 16-bit value, sub-display mode / range / value, bar graph,
three icon bytes and a checksum. Received frames (USART2 RX interrupt, 0x080247D8; handler 0x0800C332):
`F4` + a hex key code sent twice (5 bytes) presses a key — 1 RANGE, 2 HOLD, 3 REL, 4 PEAK, 5 MODE, 6 MIN/MAX,
7 MEM, 8 SETUP, +0x80 for the long press; `F8` + six hex bytes and a hex XOR (15 bytes) sets the RTC. Nothing
else is accepted: measurement settings change only through the emulated keys, and no frame reaches the HY3131.

## Not determined

- Hardware: which production boards pull PF8 low (the published diagram has R92 not fitted; no source names a
  board revision).
- Hardware: whether a disabled OP2 drives OP2O, which sets the LPF corner (47 or 152 Hz, above); the
  datasheet does not say.
