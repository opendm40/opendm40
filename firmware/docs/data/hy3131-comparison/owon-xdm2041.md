# OWON XDM2041 stock firmware: how it configures the HY3131

Sources: SPI captures ([`owon-xdm2041-info`](https://github.com/PetteriAimonen/owon-xdm2041-info) `spi_captures/all_modes.txt`, `boot.txt`), the stock image
`bin/owon-xdm2041/XDM2041.bin` (V1.7.2, load address 0x08002800), and Ghidra 12.1.2 headless
decompilation plus `arm-none-eabi-objdump` of that image. Nothing was executed. The decompiled code is not in the repository.

Notation: **[cap]** means read from an SPI capture. **[code]** means read from the disassembly. **[inf]** means
inferred (not read directly). Register field names follow `emu/hy3131/ref/registers.md`.

## 0. Architecture (code)

- **SPI:** bit-banged on GPIOB (0x40010C00). PB12 is HY3131 CS, PB13 SCK, PB14 MISO, PB15 MOSI, and PB11 latches the
  74HC595 relay register. The bus runs at about 2.6 kHz.
- **Register shadow:** a 0x38-byte RAM shadow `regs[0x00..0x37]` lives at 0x200037DC. Mode code edits only the
  shadow. FUN_080072e4 then writes **all 56 bytes in one burst** (command 0x00, R00..R37; the
  read-only data registers get written with junk too). A diff writer (FUN_08007244) exists but has
  no callers. The firmware never writes individual configuration registers, apart from the cases listed in section 3.
- **Mode switch:** `FUN_0800841c(mode, range, no_write)` runs these steps:
  1. memcpy of the shadow to 0x20003812 (previous copy)
  2. FUN_08007618(0): R20 read-modify-write that clears ENCMP. The capture shows this as `41 FF` / `40 48`.
  3. **FUN_08007b48(mode, range)**: switch network (R2A–R2F), SMODE, SFUVR, R30 ACC, R20/R21 comparator, OP base and relay byte
  4. **FUN_08008398(mode, range)**: ADC path (AD1 or AD2/RMS)
  5. FUN_08008356(mode): R31 SAGND
  6. bulk write
  7. relay byte to the 74HC595 (FUN_0800732e, sent bit-reversed with bit 7 inverted)
- **Mode numbers:** 0 DCV, 1 ACV, 2 DCI, 3 ACI, 4 Ω 2W, 5 Ω 4W, 6 continuity, 7 diode, 8 capacitance,
  9 frequency, 10 temperature (range 0 = thermocouple, run as DCV 500 mV; range ≠0 = RTD, run as Ω 500).
  The range full-scale table is at RAM 0x2000157C; the float copy is at 0x20001608.
- **AD1 composer FUN_080074c8:** takes 4 packed words (r0..r3), one byte per field. It sets ENAD1=1 and ENAD2=0, and forces R27=0x15. [code]
  - r0: b0 → R22 AD1CHOP<4:3>, b1 → R23 AD1RG, b2 → R25 AD1IG<5:4>, b3 → R22 AD1OS<7:5>
  - r1: b0 → R22 AD1OSR<2:0>, b1 → AD1IPBUF, b2 → AD1INBUF, b3 → AD1RHBUF
  - r2: b0 → AD1RLBUF, b1 → R33 SAD1I, b2 → R28 SAD1RH, b3 → R28 SAD1RL
  - r3: b0 → R24 SAD1FP, b1 → R24 SAD1FN, b2 → R33 SFT1
- **AD2/RMS composer FUN_08007636:** sets ENAD1=0 and ENAD2=1. It also forces R24 = SAD1FP 4 / SAD1FN 5 (0x45, plus SDIO kept), R28=0x31, and R33 SFT1=0 / SAD1I=0 (AD1 is unused in AC). [code]
  - r0: b0 → ENCHOPAD2, b1 → AD2RG, b2 → AD2IG, b3 → AD2OSR
  - r1: R27 SAD2IP/IN/RH/RL
  - r2: b0 → SAD2CLK, b1 → ENLPF, b2 → LPFBW, b3 → ENRMS
  - r3: b0 → SVXI, b1 → SDO23
- **Helpers [code]:**
  - FUN_08007464(sop1p, op1chop, ops1): sets R32 ENOP1=1 with SOP1P, R33 OP1CHOP, and R25 OPS1
  - FUN_08007414: R31 SFUVR
  - FUN_0800748c(rh, rl, scmpi): R21 SCMPRH/SCMPRL and R20 SCMPI with ENCMP=1
  - FUN_08007422: ENBIAS
  - FUN_080073b4: SREFO and ENREFO
  - FUN_080073f8: SACM
  - FUN_08007430: ENOSC=1, ENXI=0 (internal oscillator)
  - FUN_080073ea: ENVS
- **Base applied by FUN_08007b48 to every mode [code]:**
  - R2A..R2E = 0. R2F keeps ENVS and SMODE<6:4> (the low nibble is cleared). R30 = SREFO only (ACC = 0).
  - SDIO = 0, SFUVR = 7, ENPCMPO = 0.
  - FUN_08007464(3,2,1): OP1 on AGND, OP1CHOP = 2, OPS1 = 1. R32 is then rewritten to 0x33, so OP1 and OP2 are off with inputs on AGND, but **OP1CHOP = 2 and OPS1 = 1 stay set**.
  - R20 SCMPI = 6, ENCMP = 0.
- **AD1OSR by speed:** a table at RAM 0x2000154C = {4, 3, 3, 2} (4096, 2048, 2048, 1024), indexed by the speed
  setting (`state+0x2A`, 0..2).
  - Index [0] is forced in temperature mode. Index [3] is used in Ω/DCI/ACI-LPF while `state+0x28` (the "range just changed" flag set by autorange) is set. [code]
  - Continuity and diode always use index [2] (2048).
  - AC LPFBW is 7/6/5 for speed 0/1/2, and 5 after a range change.
  - Every capture shows speed 0 (OSR 4096).

## 1. Mode/range → R20..R36

### 1a. Captured bulk writes [cap], all confirmed by the code path [code]

Every row below was reproduced by hand from the composer arguments in the functions listed in section 5.
For example, 50 mV DCV is FUN_08007c08 case 0: SAD1FP 3 (OP1O), SAD1FN 5 (PB3), SFT1 2, AD1IG 1, AD1CHOP 2, OSR [speed], and FUN_08007464(4,2,1). That gives R24=0x35, R25=0x11, R32=0x3C, R33=0xA8. Source for every row is therefore **both**.

- The relay column is the 74HC595 byte as seen on the wire. Internally it is `0x3B8` = 08 (DCV ≤500 mV, ACV 500 mV), 0C (V ≥5 V), 8C/CC/1C (I: µA,mA / mA / A ranges), 28 (Ω, continuity, diode, capacitance) and 20 (frequency).
- R34..R36 are always 00. AD3 and SPHACAL are unused.

| SCPI | relay | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27 | 28 | 29 | 2A | 2B | 2C | 2D | 2E | 2F | 30 | 31 | 32 | 33 | 34 | 35 | 36 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `CONF:VOLT:DC 0.05` | 11 | C2 | 21 | 14 | 8B | 35 | 11 | 08 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 08 | 81 | 80 | C7 | 3C | A8 | 00 | 00 | 00 |
| `CONF:VOLT:DC 0.5` | 11 | C2 | 21 | 14 | 8B | 85 | 11 | 08 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 08 | 81 | 80 | C7 | 33 | A8 | 00 | 00 | 00 |
| `CONF:VOLT:DC 5` | 31 | C2 | 21 | 14 | 8B | 01 | 11 | 08 | 15 | 31 | F8 | 20 | 00 | 00 | 90 | 28 | A0 | 80 | C7 | 33 | A8 | 00 | 00 | 00 |
| `CONF:VOLT:DC 50` | 31 | C2 | 21 | 14 | 8B | 01 | 11 | 08 | 15 | 31 | F8 | 20 | 00 | 00 | 09 | 28 | A0 | 80 | C7 | 33 | A8 | 00 | 00 | 00 |
| `CONF:VOLT:DC 500` | 31 | C2 | 21 | 14 | 8B | 01 | 11 | 08 | 15 | 31 | F8 | 20 | 00 | 90 | 00 | 28 | A0 | 80 | C7 | 33 | A8 | 00 | 00 | 00 |
| `CONF:VOLT:DC 1000` | 31 | C2 | 21 | 14 | 8B | 09 | 01 | 08 | 15 | 31 | F8 | 20 | 00 | 90 | 00 | 28 | A0 | 80 | C7 | 33 | A8 | 00 | 00 | 00 |
| `CONF:VOLT:AC 0.5` | 11 | C2 | 21 | 00 | 00 | 45 | 00 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 08 | 91 | 80 | C7 | 3C | A0 | 00 | 00 | 00 |
| `CONF:VOLT:AC 5` | 31 | C2 | 21 | 00 | 00 | 4D | 00 | 88 | 15 | 31 | F8 | 22 | 00 | 00 | D0 | 88 | A0 | E9 | C7 | 38 | 20 | 00 | 00 | 00 |
| `CONF:VOLT:AC 50` | 31 | C2 | 21 | 00 | 00 | 4D | 00 | 88 | 15 | 31 | F8 | 22 | 00 | 00 | 09 | 28 | A0 | FF | C7 | 38 | 20 | 00 | 00 | 00 |
| `CONF:VOLT:AC 500` | 31 | C2 | 21 | 00 | 00 | 4D | 00 | 88 | 15 | 31 | F8 | 22 | 00 | 90 | 00 | 28 | A0 | 80 | C7 | 38 | 20 | 00 | 00 | 00 |
| `CONF:VOLT:AC 750` | 31 | C2 | 21 | 00 | 00 | 4D | 00 | 88 | 15 | 31 | F8 | 22 | 00 | 90 | 00 | 28 | A0 | 80 | C7 | 38 | 20 | 00 | 00 | 00 |
| `CONF:CURR:DC 0.0005` | 30 | C2 | 21 | 14 | 8B | 35 | 11 | 08 | 15 | 11 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | AC | 00 | 00 | 00 |
| `CONF:CURR:DC 0.005` | 30 | C2 | 21 | 14 | 8B | 95 | 11 | 08 | 15 | 11 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 33 | AC | 00 | 00 | 00 |
| `CONF:CURR:DC 0.05` | 32 | C2 | 21 | 14 | 8B | 35 | 11 | 08 | 15 | 11 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | AC | 00 | 00 | 00 |
| `CONF:CURR:DC 0.5` | 32 | C2 | 21 | 14 | 8B | 95 | 11 | 08 | 15 | 11 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 33 | AC | 00 | 00 | 00 |
| `CONF:CURR:DC 5` | 39 | C2 | 21 | 14 | 8B | 35 | 11 | 08 | 15 | 11 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | AC | 00 | 00 | 00 |
| `CONF:CURR:DC 10` | 39 | C2 | 21 | 14 | 8B | 35 | 01 | 08 | 15 | 11 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | AC | 00 | 00 | 00 |
| `CONF:CURR:AC 0.0005` | 30 | C2 | 21 | 12 | 00 | 45 | 01 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | 20 | 00 | 00 | 00 |
| `CONF:CURR:AC 0.005` | 30 | C2 | 21 | 12 | 00 | 45 | 00 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | A0 | 00 | 00 | 00 |
| `CONF:CURR:AC 0.05` | 32 | C2 | 21 | 12 | 00 | 45 | 01 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | 20 | 00 | 00 | 00 |
| `CONF:CURR:AC 0.5` | 32 | C2 | 21 | 12 | 00 | 45 | 00 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | A0 | 00 | 00 | 00 |
| `CONF:CURR:AC 5` | 39 | C2 | 21 | 12 | 00 | 45 | 01 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | 20 | 00 | 00 | 00 |
| `CONF:CURR:AC 10` | 39 | C2 | 21 | 12 | 00 | 45 | 00 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | A0 | 00 | 00 | 00 |
| `CONF:RES 500` | 15 | C2 | 21 | 14 | 83 | 85 | 01 | 08 | 15 | 00 | F8 | 00 | 00 | 06 | 00 | 00 | 94 | 80 | D2 | 3F | AC | 00 | 00 | 00 |
| `CONF:RES 5E3` | 15 | C2 | 21 | 14 | 83 | 85 | 01 | 08 | 15 | 00 | F8 | 00 | 00 | 60 | 00 | 00 | 94 | 80 | D3 | 3F | AC | 00 | 00 | 00 |
| `CONF:RES 50E3` | 15 | C2 | 21 | 14 | 83 | 85 | 01 | 08 | 15 | 00 | F8 | 00 | 00 | 00 | 06 | 00 | 94 | 80 | D3 | 3F | AC | 00 | 00 | 00 |
| `CONF:RES 500E3` | 15 | C2 | 21 | 14 | 83 | 85 | 01 | 08 | 15 | 00 | F8 | 00 | 00 | 00 | 60 | 00 | 94 | 80 | D3 | 3F | AC | 00 | 00 | 00 |
| `CONF:RES 5E6` | 15 | C2 | 21 | 14 | 93 | 85 | 01 | 08 | 15 | 55 | F8 | 00 | 00 | 00 | 80 | 00 | 86 | 80 | D1 | 3F | AC | 00 | 00 | 00 |
| `CONF:RES 50E6` | 15 | C2 | 21 | 14 | 93 | 85 | 01 | 08 | 15 | 55 | F8 | 00 | 08 | 00 | 00 | 00 | 86 | 80 | D1 | 3F | AC | 00 | 00 | 00 |
| `CONF:RES 500` | 15 | C2 | 21 | 14 | 83 | 85 | 01 | 08 | 15 | 00 | F8 | 00 | 00 | 06 | 00 | 00 | 94 | 80 | D2 | 3F | AC | 00 | 00 | 00 |
| `CONF:FRES` | 15 | C2 | 21 | 14 | 83 | A7 | 01 | 08 | 15 | 00 | F8 | 00 | 00 | 06 | 00 | 00 | 94 | 80 | D2 | 3F | AC | 00 | 00 | 00 |
| `CONF:RES 5E3` | 15 | C2 | 21 | 14 | 83 | 85 | 01 | 08 | 15 | 00 | F8 | 00 | 00 | 60 | 00 | 00 | 94 | 80 | D3 | 3F | AC | 00 | 00 | 00 |
| `CONF:FRES` | 15 | C2 | 21 | 14 | 83 | A7 | 01 | 08 | 15 | 00 | F8 | 00 | 00 | 60 | 00 | 00 | 94 | 80 | D3 | 3F | AC | 00 | 00 | 00 |
| `CONF:RES 50E3` | 15 | C2 | 21 | 14 | 83 | 85 | 01 | 08 | 15 | 00 | F8 | 00 | 00 | 00 | 06 | 00 | 94 | 80 | D3 | 3F | AC | 00 | 00 | 00 |
| `CONF:FRES` | 15 | C2 | 21 | 14 | 83 | A7 | 01 | 08 | 15 | 00 | F8 | 00 | 00 | 00 | 06 | 00 | 94 | 80 | D3 | 3F | AC | 00 | 00 | 00 |
| `CONF:CONT` | 15 | 76 | 62 | 13 | 83 | 85 | 01 | 08 | 15 | 00 | F8 | 00 | 40 | 06 | 00 | 00 | 94 | 80 | D2 | 3F | AC | 00 | 00 | 00 |
| `CONF:DIOD` | 15 | 12 | 62 | 13 | 8B | 8D | 11 | 08 | 15 | 11 | F8 | 00 | 00 | 08 | 00 | 00 | 86 | 80 | E2 | 33 | AC | 00 | 00 | 00 |
| `CONF:CAP 50E-9` | 15 | 72 | 21 | 14 | 8B | 01 | 11 | 08 | 15 | 31 | F8 | 00 | 00 | 00 | 08 | 00 | 9A | 80 | D7 | 33 | A8 | 00 | 00 | 00 |
| `CONF:CAP 5E-3` | 15 | C0 | 21 | 07 | 93 | 85 | 11 | 08 | 15 | 55 | F8 | 80 | 00 | 08 | 00 | 00 | 86 | 80 | D1 | 33 | AC | 00 | 00 | 00 |
| `CONF:CAP 50E-3` | 15 | 70 | DE | 07 | 93 | 85 | 11 | 08 | 15 | 55 | F8 | 80 | 00 | 08 | 00 | 00 | 8F | 80 | DA | 33 | AC | 00 | 00 | 00 |
| `CONF:FREQ` | 05 | CA | DE | 07 | 93 | 85 | 11 | 08 | 15 | 55 | F8 | 00 | 00 | 00 | 00 | 00 | 80 | 80 | D7 | 33 | AC | 00 | 00 | 00 |
| `CONF:TEMP:RTD KITS90` | 11 | C2 | DE | 14 | 8B | 85 | 11 | 08 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 08 | 81 | 80 | C7 | 33 | A8 | 00 | 00 | 00 |

Capture artefacts. These are not part of the mode's own configuration:

- **AC rows:** R22/R23 come from an AD1 composer call whose arguments are uninitialised registers (r9 in ACV, giving 00 00; sl in ACI, giving 12 00). AD1 is then disabled by FUN_08007636, so these bytes are inert.
- **`CAP 50E-9` and `FREQ` rows:** AD1 fields (R22–R28, R33 low bits) are left over from the previous mode, because capacitance ranges 0–4 and frequency do not call the AD1 composer.
- **`TEMP:RTD KITS90` row:** this is the thermocouple path. It is DCV 500 mV with OSR index 0. R21=DE is left over from FREQ.
- **Missing captures:** `CAP 500E-9 … 500E-6` produced no bulk write in the capture (only counter restarts), so those rows come from code (section 1b). `TEMP:RTD PT100` has no capture.

### 1b. Rows built from code only [code]

Only the fields that differ from the base, or that matter, are listed.

| mode / range | R20 | R21 | R22 | R23 | R24 | R25 | R28 | R2A–R2E | R2F | R31 | R32 | R33 | other |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| DCV, any range, speed 1/2 | | | 13 | | | | | | | | | | OSR 2048 |
| CAP 50 nF / 500 nF (r0,1) | 72 | 21 | (unchanged) | | | | | 2D=08 | 9A | D7 | 33 | (unchanged) | CTA←BF0000. Matches the `CAP 50E-9` capture |
| CAP 5 µF (r2) | 72 | 21 | (unchanged) | | | | | 2C=80 | 8E | D1 | 33 | | CTA←130000 |
| CAP 50 µF (r3) | 72 | 21 | (unchanged) | | | | | 2C=08 | 8E | D1 | 33 | | CTA←BF0000 |
| CAP 500 µF (r4) | 72 | 61 | (unchanged) | | | | | 2C=08 | 8E | D1 | 33 | | CTA←BF0000 |
| CAP 5/50 mF (r5,6), phase A | C0 | (unch.) | 07 | 93 | 85 | 11 | 55 | 2A=80 2C=08 | 86 | D1 | 33 | AC | Matches the `CAP 5E-3` capture |
| CAP 5/50 mF (r5,6), phase B | 70 | DE | 07 | 93 | 85 | 11 | 55 | 2A=80 2C=08 | 8F | DA | 33 | AC | Matches the `CAP 50E-3` capture |
| Ω 4W 500 k/5 M/50 M (r3–5) | | | | 93 | A7 | 01 | 55 | as 2W r3–5 | | | 3F | AC | Unreachable: the 4W range count is 3 (table at 0x20001608−0xA0) |
| Ω 2W range 6 | | | | | | | | | | | | | No case; the range count is 6, so unreachable |
| RTD (mode 10, r≠0) | identical to Ω 500 | | 14 | 83 | 85 | 01 | 00 | 2C=06 | 94 | D2 | 3F | AC | OSR forced to index 0 |
| Chip-temperature sensor (FUN_08008888) | as DCV 500 mV | | 14 | 8B | 85 | 11 | 31 | | | | | **AA**, then **AB** | SAD1I = 2 (TS1P/TS2N), then 3 (TS1N/TS2P). The boot capture's 2nd bulk write has R33=AA [cap] |
| ACV / ACI with "dual/frequency" flag (`state+4`) | R20 SCMPI 7 (PB4), ENCMP, ENCTR | DD | | | | | | | | | | | Comparator on PB4 (FUN_0800748c(0xD,0xD,7)) for the secondary frequency reading. CTA←F00000 |
| ACV 5 V / 50 V R30 | | | | | | | | | | | | | R30 = 0x80 + **calibration byte** at cal+0x568 / +0x56A. E9/FF in the capture are one unit's ACC trims |

## 2. Decoded fields per captured row

The SFT1 column uses these labels:

| SFT1 code | Datasheet label | Meaning |
|---|---|---|
| 0 | 100 k | 100 kΩ series resistor |
| 1 | 10 k | 10 kΩ series resistor |
| 2 | "0" (shown as 0R) | Multiplexer tied straight to FTP/FTN. C7 (27 nF on this board, FTP–FTN) loads the node directly |
| 3 | "nil" | [inf] Filter bypassed. Ω and DCI read correctly with it, so it cannot be an open path |

Other columns:

- **AD1RG:** 1 or 1/3. **AD1CHOP / OSR:** code / decimation.
- **Buffers:** H/L/P/N = AD1RHBUF / RLBUF / IPBUF / INBUF.
- **OPS1:** [inf] 1 = OP1 negative input on OP1N (external gain network), 0 = follower. This is inferred from OPS1 = 1 wherever OP1 provides gain (50 mV, 500 µA/50 mA/5 A) and OPS1 = 0 on the unity-gain AC ranges.

| SCPI | SFT1 | OP1CHOP | AD1 | AD1IG | AD1RG | AD1CHOP / OSR | AD1FP→FN, SAD1I | ref H/L | AD1 buffers | OP1 | AD2 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `CONF:VOLT:DC 0.05` | 0R | 2k | ON | x1.8 | 1 | 2/4096 | OP1O→PB3, I=0 | PB6/AGND | buf H-PN | OP1 on PB0 OPS1=1 | AD2 off |
| `CONF:VOLT:DC 0.5` | 0R | 2k | ON | x1.8 | 1 | 2/4096 | PB0→PB3, I=0 | PB6/AGND | buf H-PN | OP1 off AGND OPS1=1 | AD2 off |
| `CONF:VOLT:DC 5` | 0R | 2k | ON | x1.8 | 1 | 2/4096 | SENSE→RLU, I=0 | PB6/AGND | buf H-PN | OP1 off AGND OPS1=1 | AD2 off |
| `CONF:VOLT:DC 50` | 0R | 2k | ON | x1.8 | 1 | 2/4096 | SENSE→RLU, I=0 | PB6/AGND | buf H-PN | OP1 off AGND OPS1=1 | AD2 off |
| `CONF:VOLT:DC 500` | 0R | 2k | ON | x1.8 | 1 | 2/4096 | SENSE→RLU, I=0 | PB6/AGND | buf H-PN | OP1 off AGND OPS1=1 | AD2 off |
| `CONF:VOLT:DC 1000` | 0R | 2k | ON | x0.9 | 1 | 2/4096 | SENSE→RLU, I=0 | PB6/AGND | buf H-PN | OP1 off AGND OPS1=1 | AD2 off |
| `CONF:VOLT:AC 0.5` | 100k | 2k | off | x0.9 | 1 | 0/256 | OP2O→PB3, I=0 | PB6/AGND | buf ---- | OP1 on PB0 OPS1=0 | AD2 on |
| `CONF:VOLT:AC 5` | 100k | off | off | x0.9 | 1 | 0/256 | OP2O→PB3, I=0 | PB6/AGND | buf ---- | OP1 on SENSE OPS1=0 | AD2 on |
| `CONF:VOLT:AC 50` | 100k | off | off | x0.9 | 1 | 0/256 | OP2O→PB3, I=0 | PB6/AGND | buf ---- | OP1 on SENSE OPS1=0 | AD2 on |
| `CONF:VOLT:AC 500` | 100k | off | off | x0.9 | 1 | 0/256 | OP2O→PB3, I=0 | PB6/AGND | buf ---- | OP1 on SENSE OPS1=0 | AD2 on |
| `CONF:VOLT:AC 750` | 100k | off | off | x0.9 | 1 | 0/256 | OP2O→PB3, I=0 | PB6/AGND | buf ---- | OP1 on SENSE OPS1=0 | AD2 on |
| `CONF:CURR:DC 0.0005` | nil | 2k | ON | x1.8 | 1 | 2/4096 | OP1O→PB3, I=0 | REFO/AGND | buf H-PN | OP1 on PB1 OPS1=1 | AD2 off |
| `CONF:CURR:DC 0.005` | nil | 2k | ON | x1.8 | 1 | 2/4096 | PB1→PB3, I=0 | REFO/AGND | buf H-PN | OP1 off AGND OPS1=1 | AD2 off |
| `CONF:CURR:DC 0.05` | nil | 2k | ON | x1.8 | 1 | 2/4096 | OP1O→PB3, I=0 | REFO/AGND | buf H-PN | OP1 on PB1 OPS1=1 | AD2 off |
| `CONF:CURR:DC 0.5` | nil | 2k | ON | x1.8 | 1 | 2/4096 | PB1→PB3, I=0 | REFO/AGND | buf H-PN | OP1 off AGND OPS1=1 | AD2 off |
| `CONF:CURR:DC 5` | nil | 2k | ON | x1.8 | 1 | 2/4096 | OP1O→PB3, I=0 | REFO/AGND | buf H-PN | OP1 on PB1 OPS1=1 | AD2 off |
| `CONF:CURR:DC 10` | nil | 2k | ON | x0.9 | 1 | 2/4096 | OP1O→PB3, I=0 | REFO/AGND | buf H-PN | OP1 on PB1 OPS1=1 | AD2 off |
| `CONF:CURR:AC 0.0005` | 100k | off | off | x0.9 | 1 | 2/1024 | OP2O→PB3, I=0 | PB6/AGND | buf ---- | OP1 on PB1 OPS1=1 | AD2 on |
| `CONF:CURR:AC 0.005` | 100k | 2k | off | x0.9 | 1 | 2/1024 | OP2O→PB3, I=0 | PB6/AGND | buf ---- | OP1 on PB1 OPS1=0 | AD2 on |
| `CONF:CURR:AC 0.05` | 100k | off | off | x0.9 | 1 | 2/1024 | OP2O→PB3, I=0 | PB6/AGND | buf ---- | OP1 on PB1 OPS1=1 | AD2 on |
| `CONF:CURR:AC 0.5` | 100k | 2k | off | x0.9 | 1 | 2/1024 | OP2O→PB3, I=0 | PB6/AGND | buf ---- | OP1 on PB1 OPS1=0 | AD2 on |
| `CONF:CURR:AC 5` | 100k | off | off | x0.9 | 1 | 2/1024 | OP2O→PB3, I=0 | PB6/AGND | buf ---- | OP1 on PB1 OPS1=1 | AD2 on |
| `CONF:CURR:AC 10` | 100k | 2k | off | x0.9 | 1 | 2/1024 | OP2O→PB3, I=0 | PB6/AGND | buf ---- | OP1 on PB1 OPS1=0 | AD2 on |
| `CONF:RES 500` | nil | 2k | ON | x0.9 | 1 | 2/4096 | PB0→PB3, I=0 | FB/RLU | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:RES 5E3` | nil | 2k | ON | x0.9 | 1 | 2/4096 | PB0→PB3, I=0 | FB/RLU | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:RES 50E3` | nil | 2k | ON | x0.9 | 1 | 2/4096 | PB0→PB3, I=0 | FB/RLU | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:RES 500E3` | nil | 2k | ON | x0.9 | 1 | 2/4096 | PB0→PB3, I=0 | FB/RLU | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:RES 5E6` | nil | 2k | ON | x0.9 | 1/3 | 2/4096 | PB0→PB3, I=0 | VDDA/VSSA | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:RES 50E6` | nil | 2k | ON | x0.9 | 1/3 | 2/4096 | PB0→PB3, I=0 | VDDA/VSSA | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:RES 500` | nil | 2k | ON | x0.9 | 1 | 2/4096 | PB0→PB3, I=0 | FB/RLU | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:FRES` | nil | 2k | ON | x0.9 | 1 | 2/4096 | PB2→PB5, I=0 | FB/RLU | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:RES 5E3` | nil | 2k | ON | x0.9 | 1 | 2/4096 | PB0→PB3, I=0 | FB/RLU | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:FRES` | nil | 2k | ON | x0.9 | 1 | 2/4096 | PB2→PB5, I=0 | FB/RLU | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:RES 50E3` | nil | 2k | ON | x0.9 | 1 | 2/4096 | PB0→PB3, I=0 | FB/RLU | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:FRES` | nil | 2k | ON | x0.9 | 1 | 2/4096 | PB2→PB5, I=0 | FB/RLU | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:CONT` | nil | 2k | ON | x0.9 | 1 | 2/2048 | PB0→PB3, I=0 | FB/RLU | buf --PN | OP1 on PB8 OPS1=1 | AD2 off |
| `CONF:DIOD` | nil | 2k | ON | x1.8 | 1 | 2/2048 | PB0→PB3, I=0 | REFO/AGND | buf H-PN | OP1 off AGND OPS1=1 | AD2 off |
| `CONF:CAP 50E-9` | 0R | 2k | ON | x1.8 | 1 | 2/4096 | SENSE→RLU, I=0 | PB6/AGND | buf H-PN | OP1 off AGND OPS1=1 | AD2 off |
| `CONF:CAP 5E-3` | nil | 2k | ON | x1.8 | 1/3 | 0/32768 | PB0→PB3, I=0 | VDDA/VSSA | buf --PN | OP1 off AGND OPS1=1 | AD2 off |
| `CONF:CAP 50E-3` | nil | 2k | ON | x1.8 | 1/3 | 0/32768 | PB0→PB3, I=0 | VDDA/VSSA | buf --PN | OP1 off AGND OPS1=1 | AD2 off |
| `CONF:FREQ` | nil | 2k | ON | x1.8 | 1/3 | 0/32768 | PB0→PB3, I=0 | VDDA/VSSA | buf --PN | OP1 off AGND OPS1=1 | AD2 off |
| `CONF:TEMP:RTD KITS90` | 0R | 2k | ON | x1.8 | 1 | 2/4096 | PB0→PB3, I=0 | PB6/AGND | buf H-PN | OP1 off AGND OPS1=1 | AD2 off |

### Observations (pre-filter and gain)

- **DCV, all ranges:** SFT1 = 2 (no series R, 27 nF on the node, so the RC is formed with the divider/source impedance). AD1CHOP = 2 (internal input chopping), OSR 4096, ref PB6 (external 1.25 V) / AGND. AD1IG ×1.8 everywhere except **1000 V, which is ×0.9**. R33 OP1CHOP = 2 in every DC row.
  - **50 mV:** OP1 on PB0 with OPS1 = 1 (external ×10 network, 2 kHz chopper). AD1 reads OP1O vs PB3.
  - **500 mV:** OP1 off. AD1 reads PB0 vs PB3 directly.
  - **≥5 V:** SENSE vs RLU (10 M / 1.111 M divider, SMODE 0x20).
- **DCI:** SFT1 = 3 (nil), ×1.8 (10 A: ×0.9), ref REFO / AGND, OSR 4096.
  - 500 µA / 50 mA / 5 A / 10 A: the shunt on PB1 goes through OP1 (gain, chopped 2 kHz).
  - 5 mA / 500 mA: PB1 direct.
- **Ω and continuity:** SFT1 = 3, AD1IG ×0.9, AD1RHBUF off, OP1 on PB8 (OPS1 = 1, chop 2 kHz; presumably the ohms source buffer [inf]).
  - **≤500 kΩ:** ratiometric, ref FB / RLU, AD1RG 1.
  - **5 / 50 MΩ:** ref VDDA / VSSA with AD1RG 1/3.
  - **4W:** senses PB2 / PB5 instead of PB0 / PB3.
  - **Continuity:** the same as Ω 500 plus DS3, OSR 2048, and the comparator (R20=76, R21=62).
- **Diode:** SFT1 = 3, ×1.8, ref REFO / AGND, OSR 2048, SMODE 6, SAGND 0.1.
- **AC:** AD1 off, AD2 + RMS + LPF (R26 = 88: AD2OSR 32, SAD2CLK; R29 = F8: ENRMS, ENLPF, LPFBW 7). AD2IG ×0.5, AD2 on OP1O vs AGND, ref REFO / AGND. **OP1CHOP = 0 whenever OP1 has gain or is a buffer in AC**:
  - ACV ≥5 V: OP1 follower on SENSE, chop off.
  - ACV 500 mV: OP1 follower on PB0, OP1CHOP = 2 (left on).
  - ACI µA / mA / A ranges: OP1 gain, OPS1 = 1, chop **off**.
  - ACI 5 mA / 500 mA / 10 A: follower, chop 2 kHz.
  - SDIO = 1 on ACV ≥5 V.

## 3. Runtime sequences

- **No software zero/offset cycle.** Between mode changes the captures show only `Read INTF` (0x3D) and `Read AD1` (0x01) per sample, plus the 74HC595 relay byte, which is re-sent before every reading (FUN_08009a98 → FUN_0800732e) [cap][code].
  - No caller of the single-register writers (FUN_0800719e / FUN_0800730a / FUN_0800716c) touches R22..R33 from the measurement loop. The one exception is the speed key below [code].
  - DC offset is left to the HY3131's AD1CHOP = 2 and OP1CHOP = 2. Per-range calibration (gain at cal+0x1F8, offset at +0x3B0) is applied in software (FUN_080086be).
  - The "Zero" strings in `measure.c` (FUN_08008562) belong to the user REL/averaging buffer (200 samples), not to a hardware zero [code].
- **Speed key** (FUN_0800efae): increments `state+0x2A` mod 3 and does a **single R22 read-modify-write of AD1OSR only**: `FUN_0800730a(0xE, mask 7, table[speed])` [code].
- **Before every reconfiguration:** `Read R20` and `Write R20 = 0x48` (FUN_08007618(0): ENCMP off, written immediately), then the full 56-byte burst [cap][code].
- **Counter restarts:**
  - Capacitance (FUN_080076dc): `R20←70, CTA←00 00 BF, R20←72` after each reading [cap][code].
  - Frequency / AC secondary frequency (FUN_08007588 / FUN_080075c4): `R20←00, CTA←F00000, R20←F2` or `←0A`. The period is CTB/CTC [code].
- **Large capacitors (5 / 50 mF)** (FUN_0800841c, mode 8 range > 4): each cycle, in this order:
  1. Write R2A=00 and R2C=00 directly (switches open) [cap].
  2. Poll INTF until AD1F is set.
  3. Read AD1 into `state+0x48` or `+0x4C`, depending on the phase.
  4. Bulk-write the other phase (A / B toggle in `state+0x42`; see table 1b).

  So C is measured from the AD1 voltage slope in alternating phases, with switches opened during the AD1 sample [code; the charge/discharge naming of A/B is inf].
- **Boot** (FUN_0800891e):
  1. Write INTF = 0.
  2. Set ENBIAS, SREFO / ENREFO, SACM = 0, ENOSC = 1 / ENXI = 0.
  3. Configure DCV 5 V (ENVS still 0, so the boot bulk shows R2F = 20).
  4. Calibration-record checks.
  5. FUN_08008888: two chip-temperature readings with SAD1I = 2 and 3, about 2 s each. The difference goes to `state+0x3F4/0x3F8` (cold-junction / IC temperature) [inf: purpose].
  6. ENVS = 1, bulk write, DCV 5 V.
- **Autorange** (FUN_080099dc / FUN_08009866):
  - Up one range when |x| > 1.1 × the range nominal (the double at 0x08009DCC). DCV/ACV have special steps around their top ranges.
  - Down: jumps straight to the smallest range whose nominal (float table at 0x20001608, scaled by 10^exp from 0x20001720) exceeds |x|.
  - A range change sets `state+0x28`, which selects the faster OSR (index 3 → 1024) or LPFBW 5 for the first conversion in Ω/DCI/ACI, then does a full bulk reconfiguration [code].
- **Hard-coded per-range SFUVR (R31 low nibble)** [code]:
  - SFUVR 2: Ω 500, continuity, diode.
  - SFUVR 3: Ω 5 k / 50 k / 500 k.
  - SFUVR 1: Ω 5 M / 50 M, CAP 5 µF and up.
  - SFUVR 7: everything else.
  - SFUVR 10: CAP phase B.
- **SAGND by mode** (FUN_08008356): 0 (×0.5) for V/I, 1 (×0.3) for Ω/continuity/capacitance/temperature, 2 (×0.1) for diode [code].

## 4. Differences vs Project Phoenix (`owon-project-phoenix/Core/Src/dmm.c`, column 1 = INTE, then R20..R33)

Phoenix's DC, Ω, diode and continuity tables copy the captures byte for byte, apart from the differences listed here.

- **R20 ENCTR:** stock has bit 1 set in most rows (C2 vs Phoenix C0; FREQ CA vs C8; diode 12 vs 10; continuity 76 vs 60, where Phoenix also drops ENPCMPO). Phoenix manages the counter itself (DMM_ReloadCounters).
- **AC rows:** Phoenix R20/R21 = 10 / DD (comparator on SENSE, always on) vs stock C2 / 21. Stock enables SCMPI 7, R21=DD only with the dual-frequency flag.
- **ACV R30 (ACC):** Phoenix hard-codes E9 (5 V) and FF (50 V) from one unit's capture. **Stock loads ACC per unit from calibration** (cal+0x568, +0x56A).
- **50 Ω range:** exists only in Phoenix. It is the stock continuity configuration (R20=60, R21=62, R22=13, R2B=40) used as a range.
- **Capacitance:**
  - Phoenix 50 nF–5 µF all use R2D=08 / R2F=9A / R31=D7. Stock uses that only for 50/500 nF; 5 µF is R2C=80, R2F=8E, R31=D1, CTA 0x130000.
  - Phoenix 50 µF uses R2C=80 (stock: R2C=08, R2F=8E, R31=D1).
  - Phoenix 500 µF / 5 mF / "10 mF" are Phoenix's own AD1 configurations (R22=04/00, R23=8A/00, R24=A5/00, R28=55/50, R2F=8E/9A, R31=C2, SFT1 3 / OP1CHOP 0). Stock has no 10 mF range: it has 5 mF and 50 mF, using the two-phase AD1 scheme with SFT1=3, R22=07, R23=93, R28=55, and 500 µF is still counter-based.
- **Frequency:** Phoenix copies the stock bulk, including the AD1 leftovers from the previous mode.
- **Speed:** Phoenix fixes OSR 4096 (R22=14) and does not use the stock speed table {4096, 2048, 2048} or the 1024 setting after a range change. Phoenix has no RTD or chip-temperature configurations.

## 5. Function addresses (XDM2041 V1.7.2, Thumb)

| addr | role |
|---|---|
| 080070b4 / 080070cc / 080070f8 / 08007110 | SCK / CS / MOSI / MISO pin helpers (GPIOB 13 / 12 / 15 / 14) |
| 0800711a / 08007146 | bit-bang byte TX / RX |
| 0800716c / 080071aa | HY3131 write / read (logical register index → address via the table at 0x0801B884) |
| 0800719e / 080071ee / 0800730a | write one byte / read one byte / read-modify-write one register |
| 08007244 | diff write (unused) |
| 080072e4 | bulk write of the 0x38-byte shadow at 0x200037DC |
| 0800732e | 74HC595 relay latch (PB11) |
| 08007464 | OP1: SOP1P, ENOP1, OP1CHOP, OPS1 |
| 0800748c | comparator R21 / R20 |
| 080074c8 | **AD1 path composer** |
| 08007636 | **AD2 / RMS / LPF composer** |
| 0800756a / 08007618 | R20 ENCTR / ENCMP read-modify-write (immediate) |
| 08007588 / 080075c4 / 080076dc | counter restarts (frequency / AC frequency / capacitance) |
| 080077a8 | ACV switch network and ACC |
| 08007b48 | per-mode switch network, SMODE, SFUVR, comparator, relay byte |
| 08007c08 | DCV |
| 08007cde | ACV |
| 08008012 | DCI |
| 080080c4 | ACI |
| 08007d76 | Ω 2W |
| 08007e3e | Ω 4W |
| 08007ef2 | capacitance |
| 08007fa8 | diode |
| 080081b4 | continuity |
| 08008398 | ADC dispatcher |
| 08008356 | SAGND |
| 0800841c | set mode / range |
| 08008234 | large-capacitor AD1 sample |
| 080086be | DC AD1 read and calibration |
| 08008888 | chip-temperature measurement |
| 0800891e | HY3131 init |
| 08009a98 | periodic measurement dispatch |
| 080099dc / 08009866 | autorange |
| 0800efae | key handler (speed → R22 OSR) |
| 0801219e | Keil RW decompressor (region table at 0x0801CA88 → 0x20000000, 0x1928 bytes) |

The register dumps at file offsets 0x1C794 / 0x1C7CA mentioned in the earlier triage lie inside the compressed .data image (0x0801CAA8+). They are therefore initial RAM contents, not configuration tables [inf].
