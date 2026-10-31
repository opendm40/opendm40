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
script), data EEPROM 0x08080000 (calibration and settings, below). GPIO F carries the HY3131 bus. The main
loop is driven by flags set in the TIM2 interrupt (vector 44): one tick flag, and a second flag every 10
ticks; the tick period is not decoded (10 ms, *inferred* from the 121gw-re name
`freq_input_timeout_timer_10ms_unit`).

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

PF8 is configured as an input and sampled 20 times at boot (0x0800C28C); fewer than 10 high samples set a
board-variant flag that selects the second capacitance table, a gain divisor of 10⁴ instead of 10⁵ on 100 µF and
other calibration tables in the VA modes. What hardware the strap marks is unknown.

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
- **Settle** (0x0800DB48): after a change the autorange is held for N readings: DCV 2, ACV 3, ACmV 4, DCmV 1,
  Ω 1 (5 MΩ and 50 MΩ: 2), continuity 0, Hz 0, capacitance 1 (v1.02: 2 on 10 / 100 nF), Low-Z DC 5, AC+DC 1,
  1 ms peak 5, otherwise 3; current ranges with burden display 3 (A phase) / 0.
- **Autorange** (0x0800DE24, once per new reading): up when |reading| > 1.1 · full scale (1.083 · full scale
  while the overrange flag of the previous reading is set; diode: > full scale), down when < 0.1 · full scale
  (capacitance 10 mF range: 0.01; 10 A range: 0.5). Ω: after an up-range a down-range is blocked for 100
  timer ticks. Capacitance: no counter result before a timeout → jump from 10 / 100 nF to 1 µF and from
  1..100 µF to 10 mF (v1.02 `meas_process_cap_autorange`, same code in v2.05).
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
  handlers are empty. 1 ms peak: on each RMS interrupt the firmware reads PKHMAX / PKHMIN (R11..R13 / R0E..R10),
  pulses ENPKH and keeps the software maximum. The LPF key switches the analog path, not a row.
- **AC frequency-response correction** (0x080124F4, below): applied to AC V, AC mV and AC A (µA, mA) for input
  ≥ 51 Hz and < 55000 counts, from the measured frequency and four per-range calibration coefficients.
- **Counter gate**: in AC the CTA preset alternates between 0x8000 and 0xE000 every 100 timer ticks (Low-Z
  AC: 2000; v1.02 500), and the whole row set is re-sent each time (0x0800ED64).
- **AC+DC** (ACV with the AC+DC flag): 5 readings on the ACV row, then 3 on the DCV row of the same range
  (row and calibration reloaded at every switch); displayed value √(AC² + DC²) of the last reading of each,
  overload when either exceeds 60000 counts.
- **Burden voltage / VA**: the firmware alternates the A rows with the DCV / ACV rows (4 readings per phase on
  DC A with burden), reloading row and calibration at each switch.
- **Capacitance**: every range is counter-based: C ∝ CTC (comparator high time) / CTB (cycles), ×100 on
  1..100 µF; AD1 is configured in the 10 / 100 µF rows but the value comes from the counter (v1.02
  `meas_process_cap`, same structure in v2.05). Offset subtracted on 10 / 100 nF; gain divisor 10⁵ (10⁴ on
  100 µF with the PF8 variant).
- **Temperature**: K thermocouple on the DCmV 50 mV row; flash table 0x08025CF8 (µV every 10 °C from
  −210 °C, 0 µV at index 21); valid −5891..54139 µV (−200..1350 °C), outside it the open / over flag. Cold
  junction: a temperature in 0.1 °C (−20.0..100.0 °C) converted to µV with the same table
  (0x08013CF8); that it is the NTC on the MCU ADC (`calc_internal_temp` in 121gw-re) is *inferred*. In
  temperature mode TEMP_CTL feeds the V input from VDD through 20 MΩ (U16 switch Z; open-lead detection,
  *inferred* in 121gw-re).
- **Ω non-linearity** (only while EEPROM byte 0x26 = 0xA5): 5 MΩ: a fixed staircase, 1 count per 1000 up to
  15 counts at 15000..36000, back to 0 at 50000; 50 MΩ: the 121gw-re `cal_ohms_50M_factor_tbl` (input 0..60 k
  counts in 5 k steps → 0, 37, 65, 85, 100, 100, 100, 87, 70, 40, 0, −37, −65 %) times the per-unit
  factor at EEPROM 0x178 (accepted −3000..3000). The 50 MΩ calibration step writes 0xA5 to byte 0x26.
  While the byte is not 0xA5, the 50 MΩ row is sent with PA0 P open and neither correction is applied.

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
thermistor factor 0 outside ±1000; 50 MΩ factor 0 outside −3000..3000. Offsets and gains have no default.
EEPROM markers: byte 0x1F (ACV 5 V calibrated, 0xA5), byte 0x26 (50 MΩ, 0xA5).

## Board wiring (circuit diagram)

- **PB7**: output of U17 **AD8436** (true RMS-to-DC, C_AVG 10 µF) via EXT_AC_OUT, U16 (HEF4053) switch X, and
  R44 0 Ω. Its input RMS_IN comes through 1 µF from U15 (HEF4052) switch X: X1 = net AC (HY3131 OP1O), X2 = the
  output of the 1 kHz filter built around OP2 (C38, R22, R23, C25, R28), X0 = TP7. U15 switch Y sends AC to that
  filter (net LPF). MCU: CTL_D = PF4, CTL_E = PF5 (AC: X1, LPF: X2, other modes: X3 open), CTL_A = PD0 (low in AC).
- OP1O → R24 90 k → OP1N → R25 10 k → AGND (the ×10 network); OP1O → C51 1 µF → PB4 with R78 1 M to AGND.
- PB0: U16 switch Y adds C45 1.5 nF to AGND in Ω (CTL_B = PC11). PB6: ADR3412 1.2 V reference (ZD1). HY3131
  clock 4.9152 MHz crystal (X1).

## Not determined

- The hardware the PF8 strap distinguishes, and why the 50 MΩ row and the Ω corrections hinge on EEPROM byte 0x26.
- The TIM2 tick period (so the CTA gate period and the Ω down-range block in ms).
- The BT protocol: not traced; it has no HY3131 register path (all SPI callers are listed above).
- The circuit behind the range relays and PA pins beyond the nets named above.
