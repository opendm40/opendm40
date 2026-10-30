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

### Row selection (ioctl handler 0x58DB0)

The request is {mode, range, mask, settings}; the 25-byte row is sent when `mask & 0xFFFDDB60`. Function
names are *inferred* from the row contents, the calibration record each mode uses and the menus; the
numbers are from the code.

| mode | table (flash) | selection | function (*inferred*) |
|---|---|---|---|
| 0 | 0xB4C64 (5), 0xB4CE1 (2), 0xB4D13 (5), 0xB4D90 (5) | coupling byte (settings +6): 1 → 0xB4C64 + 25·range, or 0xB4CE1 when range < 2 and config +0x18 bit 1; 0 → 0xB4D13; 2 → 0xB4D90 (= the 0 rows) | V: DC / AC / AC+DC |
| 1 | 0xB4E0D (6), 0xB4EA3 (6) | coupling 1 → 0xB4E0D, else 0xB4EA3 | A: DC / AC |
| 2 | 0xB4F39 (7; row 6 all zero) | range | Ω |
| 3 | 0xB504C (5), 0xB50C9 (5) | index = global 0x200024C0 (sensitivity); config +0xAC selects the table | Hz |
| 4 | 0xB5001 | — | continuity |
| 5 | 0xB501A | — | diode |
| 6 | 0xB4FE8 | — | low-Ω (10 mA) |
| 7 | 0xB5146 (8) | range | capacitance |
| 8 | 0xB4F39 / 0xB4F52 / 0xB4C64 | sensor field (config +0x28 bits 3..2): 0 → Ω r0, 1 → Ω r1, 2 / 3 → V DC r0 plus relays | temperature: Pt100, Pt1000, J, K |
| 9 | 0xB5227 (3) | range (= V AC rows 2..4) | V low-Z |
| 10 | 0xB5033 | — | unknown |

64 row slots, 60 non-zero, every one selected by the code above; all rows are listed in the
[comparison appendix](../../hy3131-firmware-comparison.md#appendix-raw-rows).

Key fields:

- **AD2, LPF, RMS and peak-hold are off in every row** (R26 = R29 = 0), and R34..R36 = 0. AC V, AC A and Hz
  read AD1 on PB1 − PB5 (OSR 16384, AD1CHOP 01) with the comparator on PB3 and the counter running; the AC
  value comes from a circuit outside the HY3131 that drives PB1 (*inferred*).
- DC V / DC A: AD1 on OP1O − PB5, reference PB6 − PB5, OP1 chopped (1 kHz; 2 kHz on A r4/r5), OSR 8192
  with AD1CHOP 10. DC V divider taps PS3 / PS8 / PS7 / PS6 on r1..r4, P9 in every V and A row.
- AD1IG 0.9 almost everywhere (1.8 on V DC r1, 2.7 on A DC r4/r5). SFT1 100 k in every row except cap r5..r7
  (open). The die temperature sensor is never selected (SAD1I = 0 everywhere).
- Ω r0..r3: ratiometric (FB − RLU), Rref on PA5 / PA7 / PA8 / PA3 (DF), OP1 buffering PB0 (chop 2 kHz),
  SMODE 0x14; r4/r5: current source (SMODE 06), PB2 − PB5 against REFO with RG 1/3.
- Capacitance r0..r3: RC relaxation (SMODE 0x1A, comparator OP1O against VDSC10 / VDSC8, INTE = CTIE, AD1
  off); r4..r7: AD1 on OP1O − PB5 against VDDA − AGND with the current source (SMODE 06).

## Runtime logic

- **Mode / range change**: relays and analog switches first (GPIO descriptors 0xXXYY0100 via 0x5AB5C /
  0x5ABA4, plus 0x5A42C / 0x5A50C / 0x5A614), then the 25-byte burst, then the trims below. No read-back.
- **AC trim** (0x5A7BC, a 16-bit word): bits 0..6 → R30 ACC (SREFO kept), bit 9 → R2A bit 3 (P0), bit 8 →
  R2A bit 7 (P1), bit 7 → R2B bit 3 (P2): the power-bus switches of PA0..PA2; bits 10..12 → three MCU GPIOs. V AC (mode 0, coupling ≠ 1) and
  mode 9 load it from calibration record 0x11 for the range, Hz for the sensitivity index (after writing 0x1C00
  for index 3, 0x1000 otherwise); V DC writes 0.
- **Offset DAC**: on V and A range changes the firmware loads calibration record 0x12 / 0x1E (V) or
  0x13 / 0x1F (A) and writes it to the 12-bit DAC (clamped to 0xFFF); the record pair is chosen by config
  +0x18 bit 0. That the DAC trims the input offset is *inferred*.
- **Counter restart** (ioctl 2): R20 ENCTR = 0, R1C..R1D = 00 F1, read INTF, ENCTR = 1.
- ioctl 6: sets or clears SMODE4 + SMODE5 (R2F bits 4..5); caller not traced.
- **OSR / speed**: R22 comes only from the rows; the driver has no other R22 write.
- **Register access over Modbus**: holding register 0xE016 selects a HY3131 register (0..0x37, else exception 3),
  0xE017 reads or writes it.

## Constants and calibration

The Modbus map is a bank table at 0xAF218 ({descriptor array, base, count}: banks 0x0000..0x6000, 0x7000,
0x8000, 0x9000, 0xA000, 0xE000, 0xF000). Each descriptor is 16 bytes {offset, read handler, write handler,
type, size}. Bank 0xF000 (array at 0xAE028) is the calibration store: the handlers 0x40B88..0x40CDC copy
the entry's bytes at offset `(reg − 0xF000) · 2` from / into it. Types 0x80 and 0x82: 4 bytes (2 registers),
with identical handlers; 0x81: 2 bytes. 0x80 = float and 0x82 = signed 32-bit (SL) follow from the EEVblog
labels (*inferred*).

| registers | layout |
|---|---|
| 0xF000..0xF027 | pairs float, SL |
| 0xF028..0xF051 | triples float, float, SL |
| 0xF052..0xF0B5 | pairs float, SL |
| 0xF0B6..0xF0D3 | triples float, float, SL |
| 0xF0D4..0xF1C7 | pairs float, SL |
| 0xF1C8..0xF1DA | 19 × 16-bit |
| 0xF1DB..0xF200 | SL and float |
| 0xF201..0xF20C | 12 × 16-bit |
| 0xF20D..0xF218 | types 0x01 / 0x02 |

This agrees with the EEVblog [teardown thread](https://www.eevblog.com/forum/testgear/metrix-mtx3293-quick-teardown/)
(posts #50..#53): offsets at 0xF02C, 0xF038, 0xF0D2, 0xF0EE, 0xF0FE, 0xF112 are SL entries; the temperature
slope / offset pairs 0xF0E8..0xF0FA and the J / K slopes 0xF148 / 0xF14C are float entries. Which record the
mode code fetches (ids 0x00..0x1F through 0x3262C, 0x324E0) maps to which registers, the word order on the
wire, the default values, and the use of the 16-bit block (19 entries, the AC trim words, *inferred*) are
*unknown*.

Not determined: autorange thresholds and settle times, the range values behind each index (V r0 is taken
as the mV range), the cold-junction source, how AC+DC combines readings, mode 10, and the board wiring
of PB1, PB8 and the relays.
