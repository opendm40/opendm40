# OWON XDM1041 stock firmware V2.1.2: how it configures the HY3131

Image: `XDM1041_OS_V2.1.2_20210525.bin` (V2.1.2, 2021-05-25), from
[TheHWcave/OWON-XDM1041](https://github.com/TheHWcave/OWON-XDM1041/tree/main/OWON%20firmware); its readme says OWON
allowed sharing it. Like the other vendor images, it is not in the repository. Comparison baseline: the
[XDM2041 notes](owon-xdm2041.md). Register field names follow
[`registers.md`](../../../../emu/hy3131/ref/registers.md); decoded fields come from
[`hydec.py`](tools/hydec.py).

## Files

| file | source | size | SHA-256 |
|---|---|---|---|
| `XDM1041_OS_V2.1.2_20210525.bin` | [`OWON firmware/`](https://github.com/TheHWcave/OWON-XDM1041/tree/main/OWON%20firmware) | 119092 | `16e2706a0f3160d5efd742969f1defe2131c2886c779f17a996111568da67224` |
| `update.zip` (chunks `x00`..`x14` of the same folder, concatenated) | same folder | 75667562 | `a1e6627b1edc75c09235320e2c613826ee64de4c82a9a1d84cb97e80ac616a1d` |

`update.zip` holds the same `.bin` (byte-identical), `upgrade stepsXDM1041.pdf`, the DS Wave loader
(`DS_Wave_Patch_1.17.7.v20191126_Setup.exe`, not run) and libusb drivers. The update goes over the meter's serial
port with DS Wave.

**V4.7.0 is not public.** The [kernel-error.de post](https://www.kernel-error.de/2026/01/08/owon-xdm1041-firmware-v4-7-0-20220913-update-dateien-und-vorgehen/)
got it from OWON support by e-mail and did not publish it; its
[upgrade PDF](https://www.kernel-error.de/download/upgrade-stepsXDM10412212508.pdf) names
`OS--XDM1041_OS_V4.7.0_20220913.bin` and `TX-2212508.bin`. What `TX-2212508.bin` runs on is unknown. GitHub code
search and the [OWON download centre](https://www.owon.com.hk/download.asp) gave no newer image (2026-10-31).

## Method

- Ghidra 12.1.2 headless (ARM Cortex, raw binary at 0x08002800) plus `arm-none-eabi-objdump`.
- The rows were produced by emulating the firmware's own mode-setup function in
  [Unicorn](https://www.unicorn-engine.org/) 2.1.4. The decompiled code and the emulation scripts are not in the
  repository; the steps reproduce them:
  1. Load the image at 0x08002800. Run `__main` (0x080028ec) up to `__rt_entry` (0x08004644), so the firmware's own
     scatter-load fills `.data` / `.bss`.
  2. Hook the HY3131 write and read functions (0x0800784a, 0x08007888): log writes; reads return 0xFF, so polls end.
     Skip the delay loops (0x08004a4a, 0x08004a5c); log `GPIO_WriteBit` (0x08004a40).
  3. Run the HY3131 init (0x08008ee0).
  4. Run `set_mode(mode, range, 0)` (0x08008a50) once per mode and range, each time from the post-init RAM image,
     and keep the last 56-byte bulk write.
- Emulation inputs: speed index 0; the dual-frequency, large-capacitor-phase and temperature sub-reading flags
  cleared (the variant rows set them); an all-zero calibration record. Fields that come from calibration are marked
  below. No hardware was involved.
- **Against the XDM2041 captures:** the rows match the XDM2041 V1.7.2 SPI captures byte for byte, except for the
  differences in section 3 (each traced to a different constant in the code) and the history-dependent bytes in
  section 1.
- **By hand:** DCV 50 mV and diode were rebuilt from the composer arguments in the decompile and match the emulated
  bytes. DCV 50 mV is FUN_08007a16(4,2,1) plus composer words r0 = 0x10002, r1 = 0x01010106, r2 = 0x01030000,
  r3 = 0x..010503, giving R22 = 16, R23 = 8B, R24 = 35, R25 = 11, R28 = 31, R32 = 3C, R33 = A4.

## 0. Architecture

- **Image:** plain, unencrypted Thumb code; vector table at file offset 0, load address 0x08002800 (reset vector
  0x08002901, `Reset_Handler` at file offset 0x100: SystemInit, then `__main`). The first 10 KB of flash
  presumably hold a bootloader [*inferred*].
- **MCU:** the only library source name in the image is `stm32f10x_it.c` (ST StdPeriph); whether the part is a GD32
  is not visible in the image.
- **Same code base as XDM2041 V1.7.2.** The HY3131 layer matches function for function at other addresses
  (section 5): the logical-index → address table (0x0801DD5C) is byte-identical; a 0x38-byte register shadow at
  0x2000372C is written as one 56-byte burst (FUN_0800791e); the AD1 composer (FUN_08007a7a), the AD2 / RMS path
  and the helpers have the same layout.
- **SPI:** bit-banged on GPIOB: PB12 CS, PB13 SCK, PB14 MISO, PB15 MOSI.
- **No 74HC595.** The relay / switch drive is two GPIOA pins, PA8 and PA13 (FUN_08007944), set from a code at
  `state+0x3E4` after every bulk write. What the two pins switch is unknown.

  | code | set by | PA8 / PA13 |
  |---|---|---|
  | 0 | DCI 500 µA / 5 mA | 0 / 0 |
  | 1 | DCI 50 / 500 mA | 1 / 0 |
  | 2 | DCI 5 / 10 A | 0 / 0 |
  | 3 | Ω, continuity | 1 / 1 |
  | 4 | capacitance | 1 / 1 |
  | 5 | DCV ≤500 mV, ACV 500 mV | 1 / 0 |
  | 6 | V ≥5 V | 0 / 0 |
  | 7 | frequency | 0 / 1 |
  | 8 | diode | 1 / 1 |

- **State block 0x20003768:** +0x11 mode, +0x2B+mode range, +0x4C speed, +0x04 AC dual / frequency flag, +0x6A
  large-capacitor phase, +0x448 temperature sub-reading.
- **Mode numbers** (the XDM2041 numbering): 0 DCV, 1 ACV, 2 DCI, 3 ACI, 4 Ω 2W, 5 Ω 4W, 6 continuity, 7 diode,
  8 capacitance, 9 frequency, 10 temperature (range 0 on the DCV 500 mV path, range ≥1 on Ω 500). The thermocouple
  / RTD naming of the temperature ranges is *inferred* from the `KITS90` / `PT100` strings.
- **Range count per mode:** the first 11 u16 entries of a table at RAM 0x20001448 are 6, 5, 6, 6, 6, 3, 6, 6, 7, 1,
  2 (modes 0..10) [*inferred* meaning; it matches the full-scale tables]. 4W has 3 ranges, as on the XDM2041.
- **Full-scale float tables** (RAM, from `.data`): 0x200014FC DCV 0.05 … 1000; 0x20001518 ACV 0.5 … 750;
  0x20001534 DCI 0.0005 … 10; 0x20001550 ACI 0.0005 … 10; 0x2000156C Ω 2W 500 … 50 M; 0x20001588 Ω 4W 500 … 50 M;
  0x200015DC capacitance 50 nF … 50 mF.

## 1. Mode/range → R20..R36

R34..R36 are always 00. The last column is PA8 / PA13 after the burst. The rows are in
[`tools/rows.json`](tools/rows.json) (`src` = `owon-xdm1041`).

| row | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27 | 28 | 29 | 2A | 2B | 2C | 2D | 2E | 2F | 30 | 31 | 32 | 33 | 34 | 35 | 36 | PA8/PA13 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `CONF:VOLT:DC 0.05` | C0 | 00 | 16 | 8B | 35 | 11 | 00 | 15 | 31 | 00 | 00 | 00 | 00 | 00 | 08 | 81 | 80 | C7 | 3C | A4 | 00 | 00 | 00 | 1 / 0 |
| `CONF:VOLT:DC 0.5` | C0 | 00 | 16 | 8B | 85 | 11 | 00 | 15 | 31 | 00 | 00 | 00 | 00 | 00 | 08 | 81 | 80 | C7 | 33 | A4 | 00 | 00 | 00 | 1 / 0 |
| `CONF:VOLT:DC 5` | C0 | 00 | 16 | 8B | 01 | 11 | 00 | 15 | 31 | 00 | 20 | 00 | 00 | 90 | 28 | A0 | 80 | C7 | 33 | A4 | 00 | 00 | 00 | 0 / 0 |
| `CONF:VOLT:DC 50` | C0 | 00 | 16 | 8B | 01 | 11 | 00 | 15 | 31 | 00 | 20 | 00 | 00 | 09 | 28 | A0 | 80 | C7 | 33 | A4 | 00 | 00 | 00 | 0 / 0 |
| `CONF:VOLT:DC 500` | C0 | 00 | 16 | 8B | 01 | 11 | 00 | 15 | 31 | 00 | 20 | 00 | 90 | 00 | 28 | A0 | 80 | C7 | 33 | A4 | 00 | 00 | 00 | 0 / 0 |
| `CONF:VOLT:DC 1000` | C0 | 00 | 16 | 8B | 09 | 01 | 00 | 15 | 31 | 00 | 20 | 00 | 90 | 00 | 28 | A0 | 80 | C7 | 33 | A4 | 00 | 00 | 00 | 0 / 0 |
| `CONF:VOLT:AC 0.5` | C0 | 00 | 1B | 16 | 45 | 00 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 08 | 91 | 80 | C7 | 3C | A0 | 00 | 00 | 00 | 1 / 0 |
| `CONF:VOLT:AC 5` | C0 | 00 | 1B | 16 | 4D | 00 | 88 | 15 | 31 | F8 | 22 | 00 | 00 | D0 | 88 | A0 | 80 | C7 | 38 | 20 | 00 | 00 | 00 | 0 / 0 |
| `CONF:VOLT:AC 50` | C0 | 00 | 1B | 16 | 4D | 00 | 88 | 15 | 31 | F8 | 22 | 00 | 00 | 09 | 28 | A0 | 80 | C7 | 38 | 20 | 00 | 00 | 00 | 0 / 0 |
| `CONF:VOLT:AC 500` | C0 | 00 | 1B | 16 | 4D | 00 | 88 | 15 | 31 | F8 | 22 | 00 | 90 | 00 | 28 | A0 | 80 | C7 | 38 | 20 | 00 | 00 | 00 | 0 / 0 |
| `CONF:VOLT:AC 750` | C0 | 00 | 1B | 16 | 4D | 00 | 88 | 15 | 31 | F8 | 22 | 00 | 90 | 00 | 28 | A0 | 80 | C7 | 38 | 20 | 00 | 00 | 00 | 0 / 0 |
| `CONF:CURR:DC 0.0005` | C0 | 00 | 16 | 8B | 35 | 11 | 00 | 15 | 11 | 00 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3F | A8 | 00 | 00 | 00 | 0 / 0 |
| `CONF:CURR:DC 0.005` | C0 | 00 | 16 | 8B | B5 | 11 | 00 | 15 | 11 | 00 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 33 | A8 | 00 | 00 | 00 | 0 / 0 |
| `CONF:CURR:DC 0.05` | C0 | 00 | 16 | 8B | 35 | 11 | 00 | 15 | 11 | 00 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3F | A8 | 00 | 00 | 00 | 1 / 0 |
| `CONF:CURR:DC 0.5` | C0 | 00 | 16 | 8B | B5 | 11 | 00 | 15 | 11 | 00 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 33 | A8 | 00 | 00 | 00 | 1 / 0 |
| `CONF:CURR:DC 5` | C0 | 00 | 16 | 8B | 35 | 11 | 00 | 15 | 11 | 00 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | A8 | 00 | 00 | 00 | 0 / 0 |
| `CONF:CURR:DC 10` | C0 | 00 | 16 | 8B | 35 | 01 | 00 | 15 | 11 | 00 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | A8 | 00 | 00 | 00 | 0 / 0 |
| `CONF:CURR:AC 0.0005` | C0 | 00 | 00 | 00 | 45 | 01 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3F | 20 | 00 | 00 | 00 | 0 / 0 |
| `CONF:CURR:AC 0.005` | C0 | 00 | 00 | 00 | 45 | 00 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3F | A0 | 00 | 00 | 00 | 0 / 0 |
| `CONF:CURR:AC 0.05` | C0 | 00 | 00 | 00 | 45 | 01 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3F | 20 | 00 | 00 | 00 | 1 / 0 |
| `CONF:CURR:AC 0.5` | C0 | 00 | 00 | 00 | 45 | 00 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3F | A0 | 00 | 00 | 00 | 1 / 0 |
| `CONF:CURR:AC 5` | C0 | 00 | 00 | 00 | 45 | 01 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | 20 | 00 | 00 | 00 | 0 / 0 |
| `CONF:CURR:AC 10` | C0 | 00 | 00 | 00 | 45 | 00 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3D | A0 | 00 | 00 | 00 | 0 / 0 |
| `CONF:RES 500` | C0 | 00 | 16 | 83 | 85 | 01 | 00 | 15 | 00 | 00 | 00 | 40 | 06 | 00 | 00 | 94 | 80 | D2 | 3F | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:RES 5E3` | C0 | 00 | 16 | 83 | 85 | 01 | 00 | 15 | 00 | 00 | 00 | 40 | 60 | 00 | 00 | 94 | 80 | D3 | 3F | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:RES 50E3` | C0 | 00 | 16 | 83 | 85 | 01 | 00 | 15 | 00 | 00 | 00 | 40 | 00 | 06 | 00 | 94 | 80 | D3 | 3F | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:RES 500E3` | C0 | 00 | 16 | 83 | 85 | 01 | 00 | 15 | 00 | 00 | 00 | 40 | 00 | 60 | 00 | 94 | 80 | D3 | 3F | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:RES 5E6` | C0 | 00 | 16 | 93 | 85 | 01 | 00 | 15 | 55 | 00 | 00 | 00 | 00 | 80 | 00 | 86 | 80 | D1 | 3F | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:RES 50E6` | C0 | 00 | 16 | 93 | 85 | 01 | 00 | 15 | 55 | 00 | 00 | 08 | 00 | 00 | 00 | 86 | 80 | D1 | 3F | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:FRES 500` | C0 | 00 | 16 | 83 | A7 | 01 | 00 | 15 | 00 | 00 | 00 | 40 | 06 | 00 | 00 | 94 | 80 | D2 | 3F | AC | 00 | 00 | 00 | 1 / 1 |
| `CONF:FRES 5E3` | C0 | 00 | 16 | 83 | A7 | 01 | 00 | 15 | 00 | 00 | 00 | 40 | 60 | 00 | 00 | 94 | 80 | D3 | 3F | AC | 00 | 00 | 00 | 1 / 1 |
| `CONF:FRES 50E3` | C0 | 00 | 16 | 83 | A7 | 01 | 00 | 15 | 00 | 00 | 00 | 40 | 00 | 06 | 00 | 94 | 80 | D3 | 3F | AC | 00 | 00 | 00 | 1 / 1 |
| `CONF:CONT` | 74 | 62 | 10 | 83 | 85 | 01 | 00 | 15 | 00 | 00 | 00 | 40 | 06 | 00 | 00 | 94 | 80 | D2 | 3F | AC | 00 | 00 | 00 | 1 / 1 |
| `CONF:DIOD` | C0 | 00 | 10 | 8B | 8D | 11 | 00 | 15 | 11 | 00 | 00 | 00 | 08 | 00 | 00 | 86 | 80 | E2 | 33 | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:CAP 50E-9` † | 72 | 21 | 16 | 8B | 01 | 11 | 00 | 15 | 31 | 00 | 00 | 00 | 00 | 08 | 00 | 9A | 80 | D7 | 33 | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:CAP 500E-9` † | 72 | 21 | 16 | 8B | 01 | 11 | 00 | 15 | 31 | 00 | 00 | 00 | 00 | 08 | 00 | 9A | 80 | D7 | 33 | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:CAP 5E-6` † | 72 | 21 | 16 | 8B | 01 | 11 | 00 | 15 | 31 | 00 | 00 | 00 | 80 | 00 | 00 | 8E | 80 | D1 | 33 | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:CAP 50E-6` † | 72 | 21 | 16 | 8B | 01 | 11 | 00 | 15 | 31 | 00 | 00 | 00 | 08 | 00 | 00 | 8E | 80 | D1 | 33 | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:CAP 500E-6` † | 72 | 21 | 16 | 8B | 01 | 11 | 00 | 15 | 31 | 00 | 00 | 00 | 08 | 00 | 00 | 8E | 80 | D2 | 33 | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:CAP 5E-3` | C0 | 00 | 07 | 93 | 85 | 11 | 00 | 15 | 55 | 00 | 80 | 00 | 08 | 00 | 00 | 86 | 80 | D1 | 33 | AC | 00 | 00 | 00 | 1 / 1 |
| `CONF:CAP 50E-3` | C0 | 00 | 07 | 93 | 85 | 11 | 00 | 15 | 55 | 00 | 80 | 00 | 08 | 00 | 00 | 86 | 80 | D1 | 33 | AC | 00 | 00 | 00 | 1 / 1 |
| `CONF:FREQ` † | CA | 00 | 16 | 8B | 01 | 11 | 00 | 15 | 31 | 00 | 00 | 00 | 00 | 00 | 00 | 80 | 80 | C7 | 33 | A4 | 00 | 00 | 00 | 0 / 1 |
| `TEMP` range 0 (thermocouple, *inferred*) | C0 | 00 | 16 | 8B | 85 | 11 | 00 | 15 | 31 | 00 | 00 | 00 | 00 | 00 | 08 | 81 | 80 | D7 | 33 | A4 | 00 | 00 | 00 | 1 / 0 |
| `TEMP` range 1 (RTD, *inferred*) | C0 | 00 | 16 | 83 | 85 | 01 | 00 | 15 | 00 | 00 | 00 | 40 | 06 | 00 | 00 | 94 | 80 | D2 | 3F | A4 | 00 | 00 | 00 | 1 / 1 |
| `CONF:CAP 5E-3 / 50E-3` phase B | 70 | DE | 07 | 93 | 85 | 11 | 00 | 15 | 55 | 00 | 80 | 00 | 08 | 00 | 00 | 8F | 80 | DA | 33 | AC | 00 | 00 | 00 | – |
| `TEMP` range 0, `state+0x448` = 1 (die sensor, SAD1I = 2) | C0 | 00 | 16 | 8B | 85 | 11 | 00 | 15 | 31 | 00 | 00 | 00 | 00 | 00 | 08 | 81 | 80 | D7 | 33 | A6 | 00 | 00 | 00 | – |
| `TEMP` range 0, `state+0x448` = 2 (die sensor, SAD1I = 3) | C0 | 00 | 16 | 8B | 85 | 11 | 00 | 15 | 31 | 00 | 00 | 00 | 00 | 00 | 08 | 81 | 80 | D7 | 33 | A7 | 00 | 00 | 00 | – |
| `CONF:VOLT:AC 5`, secondary frequency (`state+4` = 1) | F2 | DD | 1B | 16 | 4D | 00 | 88 | 15 | 31 | F8 | 22 | 00 | 00 | D0 | 88 | A0 | 80 | C7 | 38 | 20 | 00 | 00 | 00 | – |
| `CONF:CURR:AC 0.005`, secondary frequency (`state+4` = 1) | F2 | DD | 00 | 00 | 45 | 00 | 88 | 15 | 31 | F8 | 00 | 00 | 00 | 00 | 00 | A0 | 80 | C7 | 3F | A0 | 00 | 00 | 00 | – |
| `CONF:VOLT:DC 5` speed 1 | C0 | 00 | 14 | 8B | 01 | 11 | 00 | 15 | 31 | 00 | 20 | 00 | 00 | 90 | 28 | A0 | 80 | C7 | 33 | A4 | 00 | 00 | 00 | – |
| `CONF:VOLT:AC 5` speed 1 | C0 | 00 | 1B | 16 | 4D | 00 | 88 | 15 | 31 | F0 | 22 | 00 | 00 | D0 | 88 | A0 | 80 | C7 | 38 | 20 | 00 | 00 | 00 | – |
| `CONF:VOLT:DC 5` speed 2 | C0 | 00 | 10 | 8B | 01 | 11 | 00 | 15 | 31 | 00 | 20 | 00 | 00 | 90 | 28 | A0 | 80 | C7 | 33 | A4 | 00 | 00 | 00 | – |
| `CONF:VOLT:AC 5` speed 2 | C0 | 00 | 1B | 16 | 4D | 00 | 88 | 15 | 31 | E8 | 22 | 00 | 00 | D0 | 88 | A0 | 80 | C7 | 38 | 20 | 00 | 00 | 00 | – |

† AD1 fields (R22–R28, R33 low bits) are left from the previous mode; the emulation starts from DCV 5 V.

- **Start state.** Every row starts from the post-boot shadow, the DCV 5 V configuration after FUN_08008ee0. Some
  fields are not rewritten by every mode, so on a running meter they keep earlier values:
  - **R20 ENCTR and R21:** the base keeps them; after boot both are 0. The base's R20 line,
    `(R20 & 0x1F) - 0x40 & 0xEF`, is identical in XDM2041 FUN_08007b48, so the XDM2041 captures' C2 / 21 are left
    over from earlier modes [*inferred*].
  - **R26 bit 3 (SAD2CLK) and R29:** the AD1 path clears only ENAD2. After boot both are 00; after any AC mode they
    stay 08 / F8, as in the XDM2041 captures.
  - **Capacitance 50 nF–500 µF and frequency** do not call the AD1 composer (as on the XDM2041).
- **ACV / ACI R22–R23** hold whatever the AD1 composer was called with (1B 16 in ACV, 00 00 in ACI); AD1 is off
  (ENAD1 = 0), so they are inert.
- **ACV 5 V / 50 V R30** = 0x80 | (cal[+0x568] & 0x7F) and 0x80 | (cal[+0x56A] & 0x7F) (FUN_08007e52); 80 in the
  rows because the emulated calibration is all zero. The calibration reset writes 0x74 to +0x568.
- **Variant rows** (left out of the comparison tables): large-capacitor phase B (`state+0x6A` = 1); temperature
  with `state+0x448` = 1 / 2, the die sensor via SAD1I = 2 / 3, presumably for the cold junction [*inferred*]; AC
  with the dual-frequency flag `state+4` = 1 (comparator SCMPI 7 = PB4, R21 = DD, ENCTR); speed 1 / 2.

## 2. Per-mode settings (decoded with hydec.py)

| mode | AD1 in / ref | SFT1 | AD1IG / RG | OP1 | switches | notes |
|---|---|---|---|---|---|---|
| DCV 50 mV | OP1O − PB3 / PB6 − AGND | 10k | 1.8 / 1 | on PB0, OPS1 = 1, chop 2 kHz | P8 | |
| DCV 500 mV | PB0 − PB3 / PB6 − AGND | 10k | 1.8 / 1 | off | P8 | |
| DCV 5–1000 V | SENSE − RLU / PB6 − AGND | 10k | 1.8 (1000 V: 0.9) / 1 | off | F9 P8 PS7 / PS6 / PS5 F1 | SDIO on 1000 V |
| ACV | AD2 on OP1O − AGND, ref REFO − AGND, RMS + LPF | 100k | AD2 IG 0.5 | 500 mV: on PB0, chop 2 kHz; ≥5 V: follower on SENSE, chop off | 5 V: P9 P8 PDS7 F1 F0 | |
| DCI 500 µA, 50 mA | OP1O − PB3 / REFO − AGND | 0 | 1.8 / 1 | on **PB8**, OPS1 = 1, chop 2 kHz | none | |
| DCI 5 mA, 500 mA | **PB3 − PB3** / REFO − AGND | 0 | 1.8 / 1 | off | none | SAD1FP = 0xB, SAD1FN = 5; how the shunt is read here is unknown |
| DCI 5 A, 10 A | OP1O − PB3 / REFO − AGND | 0 | 1.8 (10 A: 0.9) / 1 | on PB1, chop 2 kHz | none | |
| ACI | AD2 as ACV | 100k | | µA / mA ranges on **PB8**, A ranges on PB1; chop off when OPS1 = 1, 2 kHz when OPS1 = 0 | none | |
| Ω 500–500 k | PB0 − PB3 / FB − RLU | 10k | 0.9 / 1 | on PB8, chop 2 kHz | DF4 / DF5 / DF6 / DF7 + **D3** | SFUVR 2 (500 Ω), 3 |
| Ω 5 M, 50 M | PB0 − PB3 / VDDA − VSSA | 10k | 0.9 / 1/3 | on PB8 | P7 / P2 | SFUVR 1 |
| Ω 4W 500–50 k | PB2 − PB5 / FB − RLU | open | 0.9 / 1 | on PB8 | as 2W + D3 | |
| continuity | PB0 − PB3 / FB − RLU | open | 0.9 / 1 | on PB8 | DF4 D3 | comparator SCMPI PB0, R21 = 62; OSR 256 |
| diode | PB0 − PB3 / REFO − AGND | 10k | 1.8 / 1 | off | P4 | no comparator; OSR 256; SAGND 0.1 |
| CAP ≤500 µF | counter (CTA ← BF0000) | | | | P6 / P6 / P5 / P4 / P4 | comparator PB0, R21 = 21 |
| CAP 5 / 50 mF | PB0 − PB3 / VDDA − VSSA, OSR 32768, chop 0 | open | 1.8 / 1/3 | off | P4 P1 | two phases, as on the XDM2041 |
| frequency | counter (CTA ← F00000) | | | | none | R20 = CA |

**AD1 OSR by speed:** the table at 0x0801DDA8 = {6, 4, 0}, indexed by `state+0x4C`. The remote rate command
(FUN_080124a8; `RATE` is in the SCPI table) sets it: `S` 0, `M` 1, `F` 2; boot sets 0. How a new rate reaches R22
before the next `set_mode` is not traced (the XDM2041 does a single R22 read-modify-write).

| speed | AD1 OSR | conversion time (hydec.py, 4.9152 MHz) |
|---|---|---|
| 0 (slow) | 16384 | 200 ms |
| 1 (medium) | 4096 | 50 ms |
| 2 (fast) | 256 | 3.1 ms |

- Not set by speed: temperature range 0 always OSR 16384; continuity and diode 256; large capacitors 32768.
- AC: LPFBW 7 / 6 / 5 (OSR4 16384 / 8192 / 4096) for speed 0 / 1 / 2.
- After a range change the DCV, DCI and Ω functions index the table with `state+0x4C` only: no faster first
  conversion, unlike the XDM2041's `state+0x28` flag.

## 3. Differences from the XDM2041 V1.7.2 rows

Code-level differences only; history-dependent bytes are in section 1.

- **AD1 OSR:** speed table {16384, 4096, 256} (XDM2041 {4096, 2048, 2048, 1024}); continuity and diode 256
  (XDM2041 2048); no OSR override after a range change.
- **SFT1:**

  | mode | XDM1041 | XDM2041 |
  |---|---|---|
  | DCV | 1 (10k) | 2 (0) |
  | DCI | 2 (0) | 3 (open) |
  | Ω 2W | 1 | 3 |
  | diode | 1 | 3 |
  | RTD | 1 | 3 |

  The same for 4W, continuity, large capacitors (3) and AC (0).
- **Ω 2W ≤500 kΩ, 4W and RTD:** DS3 closed (R2B = 40). The XDM2041 closes DS3 only in continuity.
- **DCI / ACI low ranges:** OP1 takes its input from PB8 on the µA / mA ranges (SOP1P 7, R32 = 3F; XDM2041 PB1,
  3D). The direct 5 mA / 500 mA DCI ranges select SAD1FP = PB3 (R24 = B5) instead of PB1 (95). Both presumably
  follow the board's shunt wiring [*inferred*].
- **Diode:** no comparator (R20 = C0, R21 = 00; XDM2041 12 / 62).
- **Capacitance:** 5 µF uses CTA BF0000 (XDM2041 130000); 500 µF uses R21 = 21 and SFUVR 2 (XDM2041 R21 = 61,
  SFUVR 1).
- **Relay drive:** two GPIOA pins instead of a 74HC595 byte.
- **The same as on the XDM2041:** the DCV / ACV switch networks; SDIO on DCV 1000 V and ACV ≥5 V; AD1IG 0.9 on DCV
  1000 V and DCI 10 A; the AC AD2 / RMS / LPF settings and R30 ACC from calibration; Ω 5 M / 50 M; the
  large-capacitor phases A / B; the frequency counter and the dual-frequency comparator; SAGND per mode; 3 4W
  ranges.

## 4. Runtime and calibration

- **set_mode** (FUN_08008a50):
  1. FUN_08007be4(0): R20 read-modify-write that clears ENCMP, sent at once (skipped for continuity).
  2. Switch network and base (FUN_080081fe).
  3. ADC path (FUN_080089b0).
  4. SAGND (FUN_0800898c).
  5. Capacitance 5 / 50 mF: write R2A = R2C = 0, poll INTF bit 2, read AD1 (FUN_0800889c).
  6. Bulk write (FUN_0800791e), bracketed by FUN_08003de2(TIM3, 0 / 1), which clears and sets TIM3 CR1.CEN; that
     TIM3 paces the sampling is *inferred*.
  7. Relay pins (FUN_08007944).
- **Counter restarts:** capacitance (FUN_08007cbc) R20 ← 70, CTA ← 00 00 BF, R20 ← 72; frequency (FUN_08007b3a)
  R20 ← 00, CTA ← F00000, R20 ← F2.
- **Boot** (FUN_08008ee0):
  1. Clear the shadow, write INTF (R1E) = 0.
  2. Set ENBIAS, SREFO / ENREFO, SACM = 0, ENOSC = 1 / ENXI = 0, then configure DCV 5 V.
  3. Check the calibration record: if cal+0x56C is out of range it becomes 0x69E, and +0x274 / +0x278 are
     recomputed from +0x24C..+0x27C. The meaning of these values is unknown.
  4. Take the die-temperature readings (FUN_08008dd4; bursts with R33 = A6 and A7, SAD1I = 2 and 3).
  5. Set ENVS, bulk write, configure DCV 5 V.
- **Register dump:** a command handler (case 0x13 in FUN_08011234) reads all 56 registers into 0x20003B4D. The
  string `HYREG` suggests a remote command for it [*inferred*].
- **Storage (internal flash, 2 KB pages),** copied to RAM at boot by FUN_0800457c:

  | flash address | size | RAM copy | content | written by |
  |---|---|---|---|---|
  | 0x0803F000 | 0xA8 | 0x200019A0 | settings | FUN_08006600 |
  | 0x0803F800 | 0x570 | 0x20001A48 | calibration | FUN_080065e8 (erase + word program up to 0x08040000) |
  | 0x0803D800 | 0x1774 | 0x20001FB8 | unknown | |

- **Calibration layout** (the XDM2041 layout): gain floats at +0x1F8 and offset floats at +0x3B0, 0x28 bytes (10
  ranges) per mode, indexed `mode*0x28 + range*4`; ACC bytes at +0x568 / +0x56A. Applied in software in
  FUN_0800971e.
- **Factory calibration** (FUN_0800bb90; literal pool at 0x0800BCD4 holds the calibration base): sends `*IDN?`,
  `FUNC DC;:VOLT %f`, `FUNC SIN;:VOLT %f;:FREQ 60`, `FUNC DC;:CURR %f`, `FUNC SIN;:CURR %f;:FREQ 60`,
  `RES %f;:RES:UUT_I HIGH|LOW`, `OUTP ON/OFF` and `INT:END` on a UART (commands for an external calibrator
  [*inferred*]). It first resets all offsets to 0, all gains to one constant and ACC (+0x568) to 0x74, then runs the
  steps and saves the record after each one. The `ACC cali Read:%0.4f,Diff:%0.4f,ACC:%d` strings and `selftuning.c`
  point to an automatic ACC trim loop [*inferred*].

## 5. Function addresses (XDM1041 V2.1.2, Thumb)

| addr | role | XDM2041 V1.7.2 |
|---|---|---|
| 0800778c / 080077c8 | CS / byte TX | |
| 0800784a / 08007888 | HY3131 write / read (logical index → table 0x0801DD5C) | 0800716c / 080071aa |
| 0800787c / 080078ec | write / read one register | 0800719e / 080071ee |
| 0800791e | bulk write of the shadow 0x2000372C | 080072e4 |
| 08007944 | PA8 / PA13 relay drive | 0800732e (74HC595) |
| 0800798a / 080079be / 080079cc / 080079e8 / 080079f6 / 08007a04 | SREFO+ENREFO / ENVS / SACM / SFUVR / ENBIAS / ENOSC | 080073b4 … 08007430 |
| 08007a16 | OP1: SOP1P, ENOP1, OP1CHOP, OPS1 | 08007464 |
| 08007a3e | comparator R21 / R20 | 0800748c |
| 08007a7a | AD1 composer | 080074c8 |
| 08007b1c / 08007be4 | R20 ENCTR / ENCMP read-modify-write | 0800756a / 08007618 |
| 08007b3a / 08007cbc | counter restart (frequency / capacitance) | 08007588 / 080076dc |
| 08007e52 | ACV switches and ACC | 080077a8 |
| 08007f44 | Ω switches | |
| 08007ff2 | capacitance switches | |
| 080081fe | per-mode switch network and base | 08007b48 |
| 080082be / 08008392 / 0800841a / 080084ca / 0800857e / 08008730 / 08008820 | DCV / ACV / Ω 2W / Ω 4W / capacitance / ACI / continuity ADC paths | |
| 0800889c | large-capacitor AD1 sample | 08008234 |
| 0800898c | SAGND | 08008356 |
| 080089b0 | ADC dispatcher | 08008398 |
| 08008a50 | set mode / range | 0800841c |
| 08008dd4 | die-temperature measurement | 08008888 |
| 08008ee0 | HY3131 init | 0800891e |
| 0800971e | measurement and calibration | |
| 0800457c | load settings / calibration from flash | |
| 080065e8 / 08006600 | save calibration / settings | |
| 08004a4a | delay (ms loop) | |
