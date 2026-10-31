# Metrix MTX3293 (ASYC IV) firmware v1.02: decode and HY3131 configuration

Source: Chauvin Arnoux "Logiciel embarqué ASYC IV", loader v1.09, `loader_asyc4_v1.09.zip`
(15,100,003 B, SHA-256 `f5ac2ce3e984879ce630f4a48e3722d7a8d5a492d550fcc67952399b6d057567`), from the support page
[logiciel-embarque-asyc-iv](https://www.chauvin-arnoux.com/fr/ressources-et-support/support-faq/logiciels/logiciel-embarque-asyc-iv)
(found through the [MTX3293 downloads](https://www.chauvin-arnoux.com/fr/support/telechargement/results/mtx3293)).
Static analysis only. The images, the Ghidra project and the listings are not in the repository (Chauvin Arnoux's
code); the steps below reproduce them from the official package.

## Package and image format

1. `loader_asyc4_v1.09.zip` → `Loader ASYC4 v1.09.exe`, a self-extracting setup; `7z x` gives `Loader ASYC4.msi`
   and `DistFile.cab`; `cabextract DistFile.cab` gives the LabWindows/CVI loader `LOADERASYC4.EXE`, the CP210x
   driver and seven images:

| file | size | meter |
|---|---|---|
| `X04877C01_P_CODE_V1_02_ASYC4B_GB_FR.BIN` (C03 DE, C04 IT, C05 ES) | 796,616 B (FR) | MTX3292B / MTX3293B, CA 5292 / 5293; two-language packs |
| `X04140J00_P_CODE_V1_08_ASYC4_100KPTS.BIN` | 773,544 B | earlier ASYC IV, v1.08 |
| `X04487C00_…_V1_08_ASYC4_MARINE_100KPTS.BIN`, `X04687B00_…_V1_08_ASYC4_AUTO_100KPTS.BIN` | 773,384 / 781,700 B | v1.08 variants |

2. Each `.BIN` is a plain Cortex-M image: no container, no scrambling, no encryption; the loader is not needed for
   the decode. It is linked at **0x00008000**: of 629 NUL-terminated strings, 537 are referenced by a 32-bit word
   equal to their file offset + 0x8000, and no other base comes close. The first two words are SP = 0x20010000
   and reset = 0x00061E99; the rest of the vector table is zero in the file. 0x0000..0x7FFF is not in the
   package (a resident bootloader, *inferred*).
3. All five image types carry the same 1,600-byte HY3131 row block (64 rows of 25 bytes, followed by the
   string `hy3131`); the analysis below uses the v1.02 FR image. Addresses are flash addresses
   (file offset + 0x8000).

Reproduce: Ghidra `BinaryLoader`, base 0x8000, `ARM:LE:32:Cortex`, then disassemble from the code pointers.

## MCU

NXP/Freescale **Kinetis K70** (`MK70FN1M0`, *inferred* exact part): SRAM_L at 0x1FFFxxxx and SRAM_U up to
0x20010000 (128 KB), SIM at 0x40047000 / 0x40048000 (SCGC1..7), PORTA..PORTF at 0x40049000.., GPIO PTA..PTF
at 0x400FF000..0x400FF140, the graphic LCD controller at 0x400B6000 (a K70 peripheral), 12-bit DAC
(0x400CC000 / 0x400CD000), I2C0/1, UART0/1/2/4, RTC. The image ends at 0xCA7C8, beyond 512 KB, so 1 MB
flash. RTOS with device drivers (`/dev/hy3131`; MQX, *inferred*) and a Nano-X GUI (`Couldn't connect to the
Nano-X server`).

## HY3131 driver

Bit-banged SPI on port F (no SPI peripheral is used), init at 0x58608:

- PTF24 = CS (low active), PTF23 = MOSI, PTF25 = SCK, PTF27 = MISO (input); MSB first, data set while SCK is
  low, then an SCK pulse. The first byte is `addr<<1` (write, 0x58AF4) or `addr<<1 | 1` (read, 0x589C0).
- During each transfer the PORTE PCR23 interrupt (IRQC, 0x4004E05C) is disabled; the HY3131 interrupt is on
  PTE23 (*inferred*: the ISR clears PORTE ISFR and reads INTF).
- Writes: one 25-byte burst **R1E..R36** per configuration (INTF = 0, INTE from the row, R20..R36);
  R37 = 0x60 (reset) only when the device is opened (ioctl 0, 0x58DB0) and at 0x586E8; single-register
  read-modify-writes of R20, R2A, R2B, R2F, R30; R1C..R1D for the counter.
- Interrupt handler 0x5A17C: reads INTF; CTF → R1B..R1D, R18..R1A, R15..R17 (CTA, CTB, CTC) and R14; AD1F →
  R00..R02 (24-bit, sign-extended); RMSF → R09..R0D (40 bits) and a floating-point conversion. No row enables the RMS
  block, so that path runs only after a register write from outside the tables.

- The handler pushes each result as a 5-byte record {int32 value, type} into a 10-record ring that the
  measurement task `read()`s: type 0 = AD1 code (or the RMS result), 10..13 = CTA, CTB, CTC, R14. No
  scaling, range or autorange logic sits in the driver.

### Row selection (ioctl handler 0x58DB0)

The request is {mode, range, mask, settings}; the 25-byte row is sent when `mask & 0xFFFDDB60`. Each mode has
its own task; the names come from the task table at 0xB6C9C (`mvolt`, `mamp`, …). The range values come from
the range table at 0xACA44: per mode a list of {counts, exponent, decimals}, value = counts · 10^(exponent −
decimals).

| mode | task | table (flash) | selection | ranges (r0, r1, …) |
|---|---|---|---|---|
| 0 | `mvolt` 0x3B150 | 0xB4C64 (5), 0xB4CE1 (2), 0xB4D13 (5), 0xB4D90 (5) | coupling byte (settings +6): 1 = DC → 0xB4C64 + 25·range, or 0xB4CE1 when range < 2 and the Impedance setting is 1; 0 = AC → 0xB4D13; 2 = AC+DC → 0xB4D90 (= the AC rows) | 100.000 mV, 1.00000 V, 10.0000 V, 100.000 V, 1000.00 V |
| 1 | `mamp` 0x3A3F8 | 0xB4E0D (6), 0xB4EA3 (6) | coupling 1 → 0xB4E0D, else 0xB4EA3 | 1000.00 µA, 10.0000 mA, 100.000 mA, 1000.00 mA, 10.0000 A, 100.000 A (r5: the r4 row and calibration, value ÷ 10) |
| 2 | `mohm` 0x3C068 | 0xB4F39 (7; row 6 all zero) | range | 1000.00 Ω, 10.0000 kΩ, 100.000 kΩ, 1.00000 MΩ, 10.0000 MΩ, 100.000 MΩ |
| 3 | `mhz` 0x3C65C | 0xB504C (5), 0xB50C9 (5) | index = global 0x200024C0 (sensitivity); config +0xAC selects the table | 10.0000 Hz … 10.0000 MHz (7 decades) |
| 4 | `mcont` 0x3D5B0 | 0xB5001 | — | 1000.0 Ω |
| 5 | `mdiode` 0x3DF60 | 0xB501A | — | 10.0000 V |
| 6 | `m50ohm` 0x3EED0 | 0xB4FE8 | — | 100.00 Ω (the "100 Ohm function" of the help text) |
| 7 | `mcapa` 0x3DC8C | 0xB5146 (8) | range | 1000 pF, 10.00 nF, 100.0 nF, 1000 nF, 10.00 µF, 100.0 µF, 1000 µF, 10.00 mF |
| 8 | `mtemp` 0x3CBD0 | 0xB4F39 / 0xB4F52 / 0xB4C64 | sensor field (config +0x28 bits 3..2): 0 Pt100 → Ω r0, 1 Pt1000 → Ω r1, 2 J / 3 K → V DC r0 plus relays | 10000.0 ° (0.1° resolution) |
| 9 | `mvlowz` 0x3E158 | 0xB5227 (3) | range (= V AC rows 2..4) | 10.0000 V, 100.000 V, 1000.00 V |
| 10 | `mdiodez` 0x3F070 | 0xB5033 | — | 20.000 V, OL above 26.000 V (Zener diode test, *inferred* from the name) |

Every range is 100 000 counts except mode 4 / 6 (10 000), mode 7 (1000) and mode 10 (20 000). The readings
are clipped to ± full scale and shown as OL beyond it (0x37398).

64 row slots, 60 non-zero, every one selected by the code above; all rows are listed in the
[comparison appendix](../../hy3131-firmware-comparison.md#appendix-raw-rows).

Key fields:

- **AD2, LPF, RMS and peak-hold are off in every row** (R26 = R29 = 0), and R34..R36 = 0. AC V, AC A, low-Z
  and Hz read AD1 on PB1 − PB5 (OSR 16384, AD1CHOP 01) with the comparator on PB3 and the counter running.
- DC V / DC A: AD1 on OP1O − PB5, reference PB6 − PB5, OP1 chopped (1 kHz; 2 kHz on A r4/r5), OSR 8192
  with AD1CHOP 10 (A r4/r5: OSR 16384, AD1CHOP 01). DC V divider taps PS3 / PS8 / PS7 / PS6 on r1..r4, P9
  in every V and A row.
- AC V r2..r4 close the same divider tap and connect the ACC array (SMODE5) with OP1 buffering SENSE; AC V
  r0 / r1 use OP1 on FB with F5 and no ACC array.
- AD1IG 0.9 almost everywhere (1.8 on V DC r1, 2.7 on A DC r4/r5). SFT1 100 k in every row except cap r5..r7
  (open). The die temperature sensor is never selected (SAD1I = 0 everywhere).
- Ω r0..r3: ratiometric (FB − RLU), Rref on PA5 / PA7 / PA8 / PA3 (DF), OP1 buffering PB0 (chop 2 kHz),
  SMODE 0x14; r4/r5: current source (SMODE 06), PB2 − PB5 against REFO with RG 1/3.
- Capacitance r0..r3: RC relaxation (SMODE 0x1A, comparator OP1O against VDSC10 / VDSC8, INTE = CTIE, AD1
  off); r4..r7: AD1 on OP1O − PB5 against VDDA − AGND with the current source (SMODE 06).

## Board switching (ioctl 1, 0x58DB0)

Before the row, the driver sets MCU GPIOs (descriptor bits 31..27 = pin, 22..20 = port; 0x5AB5C sets,
0x5ABA4 clears). Pin roles are *inferred* from the modes that switch them.

- **Four latching relays K0..K3**: coil polarity on PTD11 / PTD10, pulse on PTD15 / PTD14 / PTD13 / PTD12,
  contact read-back on PTB16 / PTB10 / — / PTB11 (0x5A50C on, 0x5A614 off; a pulse is skipped when the
  read-back already shows the wanted state). K0: on in V DC and AC+DC, off in V AC and low-Z. K1: on in
  every mode except low-Z (off) and A (untouched), the only relay that tells low-Z from V AC, so it
  switches the low-Z load (*inferred*). K2: on in V r0 and Hz sensitivity 0. K3: on only in mode 6 and
  mode 10.
- **A shunt select**: one of five active-low lines, PTC17 / PTC18 / PTC19 / PTB23 for A r0..r3, PTB22 for
  r4 / r5 and all other modes (0x5A42C); none in mode 10. OP1 takes PB8 in every A row, so PB8 carries
  the shunt voltage (*inferred*).
- **Coupling** (mask 0x4000): DC: PTC16 set, K0 on, PTD2 set; AC+DC: PTC16 set, K0 on, PTD2 clear; AC:
  PTC16 clear, K0 off, PTD2 clear. A switches PTC16 and PTD2 the same way.
- **AC path**: PTD0 and PTB21 set in AC and AC+DC, clear in DC. PTC13 set in V DC and on V r0, clear in V AC.
  PTC9 set only on V AC r1.
- **Filter setting**: PTD3 follows it in every mode except Ω, continuity and capacitance.
- **AC trim word** (16 bits, 0x5A7BC): bits 0..6 → R30 ACC (SREFO kept); bit 9 → R2A bit 3 (P0), bit 8 →
  R2A bit 7 (P1), bit 7 → R2B bit 3 (P2), the power-bus switches of PA0..PA2; bits 10 / 11 / 12 → PTC12 /
  PTC11 / PTC10. V AC and low-Z load it from calibration record 0x11 for the range (low-Z: range + 2); V DC
  writes 0. Hz writes 0x1C00 (sensitivity index 3) or 0x1000, then the record 0x11 word for the index.
- **Offset DAC** (DAC0, 12-bit, clamped to 0xFFF): on every V and A range change, in DC and AC alike, the word
  from record 0x12 (V) / 0x13 (A), or 0x1E / 0x1F while the Filter setting is on.
- ioctl 2 restarts the counter: R20 ENCTR = 0, R1C..R1D = 00 F1, read INTF, ENCTR = 1. ioctl 6 sets or
  clears SMODE4 + SMODE5 (R2F bits 4..5).

The AC value reaches PB1 from a circuit outside the HY3131, and the firmware treats the PB1 code linearly
(gain, offset, a small correction below; no square, no square root, no table). The circuit is therefore a
converter whose output is proportional to the RMS value (*inferred*); its part is unknown. AC+DC selects the
DC input coupling (PTC16, K0) and keeps the AC path (PTD2 clear, PTD0 / PTB21 set) with its own calibration
set, so the converter itself measures AC+DC (*inferred*); the firmware does not combine two readings. The ACC
array sits on the divider in the AC V r2..r4 rows, upstream of OP1 and PB1, so the R30 trim acts as the
divider's frequency compensation ahead of the converter (*inferred*).

## Measurement tasks

Each task opens `/dev/hy3131`, reads the 5-byte records, applies the calibration record of its mode and
passes the value to 0x37398 (clip, OL, autorange, display).

- **V** (0x3B150), with g / c the record 0x00 fields of the range and key:
  - DC: v = (code − c) · g₊ for code ≥ 0, (code − c) · g₋ below.
  - AC, AC+DC: x = code · g − c, clamped at 0, then v = x − k · p(x) (r1: x − k · p(x) / 2), with k from
    record 0x18 (default 1.0) and p(x) = 3.49353e−20·x⁴ − 8.506821e−15·x³ + 7.7694757e−11·x² +
    4.062786e−5·x − 0.362539822 (−0.36 at 0, +1.02 at 52 000, −0.54 at 100 000 counts).
  - On r0 (any coupling), a reading of MCU ADC1 channel 16 below 1600 replaces the value with OL.
- **Low-Z** (0x3E158): the V AC formula with the record 0x00 AC set of V r2..r4 (sub = range + 2), the gain
  multiplied by record 0x01.
- **A** (0x3A3F8): DC as V DC. AC / AC+DC: x = code · g − c, clamped at 0; then (not on r5)
  x − k · x / 50 000 below 50 000 counts and x + k · x / 50 000 − 2k above, a triangle of −k counts at mid
  scale; k from record 0x1B (r0..r3, default 0) or 0x19 (r4/r5, default 1.0). r5 divides by 10.
- **Ω** (0x3C068): x = (code − c) · g, then x − s · q(x) with s the record 0x14 integer of the range (default 0)
  and q a fixed cubic without constant term (one coefficient set for r5, one for the others), clamped at 0.
- **Temperature** (0x3CBD0): RTD (code − c) · g / 100 − 100 through a fixed 4th-order polynomial (one set below
  0 °, one above). Thermocouple: (code − c) · g / 1000 mV plus the cold-junction EMF, through the NIST
  ITS-90 inverse polynomials (J: 9 coefficients, segments split at 0 and 42.919 mV; K: 10 coefficients,
  split at 0 and 20.644 mV). °F = 1.8 · t + 32, K = t + 273.15.
- **Cold junction**: `/dev/TempE` returns a 12-bit two's-complement word, 0.0625 ° per LSB (a digital
  temperature sensor, part unknown), plus the offset in record 0x0A. Its EMF is (50.4493·T + 0.02496·T²) /
  1000 mV for J and (39.508·T + 0.01888·T²) / 1000 mV for K. It is read when the sensor is chosen, when the
  calibration changes and then every 100 readings; the 3 readings after each read are discarded.
- **Frequency** (secondary reading in the V task): gate = (CTA + 0xF0000) / 4.9152 MHz, f = CTB / gate, duty = CTC / (CTA +
  0xF0000); valid from 0.8 Hz to below 900 kHz, shown with 5 digits.
- **Zener** (0x3F070): (code − c) · g with record 0x1D, clamped at 0, OL from 26 000 counts.
- **Peak** (`/dev/peak`, 0x33A24 / 0x33914): records 0x0E (V) / 0x0F (A), ±2900 counts (A r5: 3000).
- **Filter** (0x36230): a running mean of up to N readings (N = 16; capacitance 5), restarted when a reading
  differs from the mean by ≥ A counts, or by ≥ P % while |reading| ≥ P % of the display resolution (10 …
  100 000 by the `digits` setting). V: P = 1, A = 90 on r0 and 20 on the other ranges; A DC r0..r3: P = 8,
  A r4/r5 and AC: P = 20. Overload readings reset it.
- No speed setting: R22 comes only from the rows (AD1 period 100 ms, 200 ms in mode 10).

## Autorange and settling (0x37398)

- **AUTO** (range −1): up one range when the reading is OL (≥ 100 000 counts, i.e. ≥ 100 % of full scale);
  down one range when |reading| < 9 % of full scale (9000 counts). V never autoranges into r0 (100 mV); the
  other modes go down to r0. Hz is excluded (its own sensitivity index).
- **AUTO PEAK** (range −2; the `AUTO Pk` entry, *inferred*): up only, when the reading or a stored peak is OL; never down.
- **Settling**: after an autorange step, and after a manual V / A range change, the next 2 readings are
  discarded (capacitance: 1 reading below r4, none above). With the 100 ms rows that is 200 ms.
- A, manual range: 26 consecutive OL readings switch back to AUTO.
- The V, A and low-Z tasks contain a second autorange with per-range thresholds (0xAD204 / 0xAD22C, e.g. V AC r0 850 / 12 250)
  on a value from 0x5A4C0; its enable byte 0x1FFFFCDC is in zeroed RAM and never written, so that path is dead.
- Warnings: V ≥ 60 V DC or ≥ 25 V AC / AC+DC sets the hazard flag and beeper pattern.

## Calibration store

`/dev/hd/hdc` holds 32 records (ids 0x00..0x1F), 1050 bytes, laid out in id order. Record id → layout
table at 0xABD68: per record a list of u16 words {key, field ids…} with separators 0xF001 (next key), 0xF002
(next range), 0xF000 (restart the range count) and 0xF00F (end). Field ids 0..2 = float, 3 = int32, 4 / 5 = u16
(0xABE2C / 0xABE32). The reader 0x3262C(id, key, range) returns {g = f0 (default 1.0), g₊ = f1, g₋ = f2, c = f3,
trim = f4, DAC = f5}.

Bank 0xF000 maps register 0xF000 + n to byte 2n of the same store (handlers 0x40B88..0x40CDC), so each record
field has a fixed register:

| id | use | registers (defaults in the image, see below) |
|---|---|---|
| 0x00 | V | AC r0..r4: f0, f3 at 0xF000 + 8·r (Filter off, key 0x81) and 0xF004 + 8·r (Filter on, key 0x101); DC (f1, f2, f3): r0 0xF028, r0 Impedance 1 0xF02E, r1 0xF034, r1 Impedance 1 0xF03A, r2 0xF040, r3 0xF046, r4 0xF04C; AC+DC: r0 0xF052 / 0xF056, r1 0xF062 / 0xF066, r2..r4 0xF072..0xF088 (Filter off / on; the key 8 slots 0xF05A, 0xF05E, 0xF06A, 0xF06E are never read) |
| 0x01 | low-Z gain factor | 0xF08A (f0), 0xF08C (f3) |
| 0x02 | A | AC r0..r4 0xF08E..0xF0B4 (Filter off / on); DC r0..r4 (f1, f2, f3) 0xF0B6..0xF0D2; AC+DC r0..r4 0xF0D4..0xF0FA |
| 0x03 | Ω r0..r5 | 0xF0FC + 4·r |
| 0x04 / 0x05 / 0x06 | mode 6 (100 Ω) / continuity / diode | 0xF114 / 0xF118 / 0xF11C |
| 0x07 | capacitance r0..r7 | 0xF120 + 4·r |
| 0x08 | temperature Pt100 / Pt1000 / J / K | 0xF140 / 0xF144 / 0xF148 / 0xF14C |
| 0x09 | loaded after `/dev/TempI` opens; use not traced | 0xF150 |
| 0x0A | cold-junction offset (°) | 0xF154 |
| 0x0B, 0x0C, 0x0D | *unknown* | 0xF158, 0xF15C, 0xF160 |
| 0x0E / 0x0F | V / A peak, r0..r4 | 0xF164..0xF19A / 0xF19C..0xF1C2 |
| 0x10 | *unknown* | 0xF1C4 |
| 0x11 | AC trim words: r0 and r1 in two slots (keys 4, 8), r2..r4 | 0xF1C8..0xF1CE |
| 0x12 / 0x13 | offset DAC words V r0..r4 / A r0..r4, Filter off | 0xF1CF..0xF1D5 / 0xF1D6..0xF1DA |
| 0x14 | Ω correction integer s, r0..r5 | 0xF1DB + 2·r |
| 0x15 / 0x16 | A peak gain, r0..r3 / r4 | 0xF1E7 / 0xF1E9 |
| 0x17 | *unknown* | 0xF1EB |
| 0x18 | V AC / low-Z correction factor k | 0xF1ED |
| 0x19 / 0x1B | A AC correction k, r4/r5 / r0..r3 | 0xF1EF / 0xF1F5 |
| 0x1A, 0x1C | f3 of 0x1A feeds only the dead autorange path; f0 of 0x1C is passed to 0x30A0C in modes 2, 4, 5, 10 | 0xF1F1, 0xF1F9 |
| 0x1D | Zener | 0xF1FD |
| 0x1E / 0x1F | offset DAC words V / A, Filter on | 0xF201..0xF207 / 0xF208..0xF20C |

- **Keys**: bit 0 = AC path, bit 1 = DC path (DC 2, AC 1, AC+DC 3); 0x80 / 0x100 = Filter off / on; 4 / 8 =
  V r0 / r1 (8: Impedance 1 in DC, or sub-function 6); 0x10..0x40 = temperature sensor; 0x8000 = single set.
- **Wire format**: the Modbus code at 0x4057C parses ASCII frames (`:` and hex digits). A 4-byte entry goes
  out as one big-endian item (0x3FE80 with element size 4): the lower register carries the high 16 bits.
- **Defaults**: a 1052-byte image of the store at flash 0xAF2C0 (factory values of one unit, e.g. V AC r0
  g = 0.050094, c = −991; V DC r0 g₊ = 0.016852, c = −9589). A write of 1 to register 0xE022 (0x45C7C), or the
  UI handler at 0x50E18 after a string check, copies it over the store (0x32B88). The reader takes bytes from this image instead of
  the store while the RAM flag 0x1FFFFC12 is set.
- The EEVblog [teardown thread](https://www.eevblog.com/forum/testgear/metrix-mtx3293-quick-teardown/)
  (posts #50..#53) labels 0xF0E8..0xF0FA as temperature pairs; the layout table assigns them to record 0x02
  (A AC+DC r2..r4). Its other labels match the table.
- The register access to the HY3131 itself: 0xE016 selects R00..R37 (else exception 3), 0xE017 reads or
  writes it.

Not determined: the AC converter part and the role of the three trim GPIOs; what MCU ADC1 channel 16 measures;
PTC14, PTC15 and PTE4; the use of records 0x09..0x0D, 0x10, 0x17, 0x1C; what sets the default-image flag;
the RTOS tick period (relay pulse = 10 ticks); the physical meaning of the Impedance setting.
