# DM40A / B / C: how the firmware tells the model, and what changes

Owner question (2026-10-22): "does the stock firmware think it's a DM40A?"
**Yes.** This unit's calibration record says model 0 = DM40A, in the
2026-10-13 flash dump and on the device today (section 3), and the stock
V2.6 firmware shows the DM40A ranges for it.

Sources: `stock/re/analysis/hy3131_v2_6.h` (calibration record, model byte),
`dmm_v2_6.h` (limits per model), `bt_meas_v2_6.h` (BT name / cmd 00),
`gui_v2_6.h` (About page), the flash dump, and emulator runs of the stock
binary with the byte patched (section 5). Manual: Table 2.3.1 (A = 4000-count
style 400 mV / 4 V / 40 V / 400 V / 1000 V, 400 uA .. 4 A / 10 A; B = 5000;
C = 6000; AC+DC on B / C only).

## 1. Where the model comes from

One byte: **calibration struct byte 0 = internal flash `0x080C0168`**
(settings / calibration record at `0x080C0000`, struct at record `+0x168`).

| What | Value |
|---|---|
| Record | `0x080C0000`, 800 bytes: `u16 crc`, `u16 ~crc`, `u32 version = 1`, data |
| CRC | CRC-16 poly 0x8005, init 0xFFFF, MSB-first, no final XOR (`stock/dm40fw.py crc16`), over record bytes 4..0x31F, stored little-endian at +0 with the complement at +2. This unit: `c7 8a 38 75` = 0x8AC7 / 0x7538 (checked) |
| Model | byte `+0x168`: 0 DM40A, 1 DM40B, 2 DM40C; BT cmd 00 clamps it to 2 (bt_meas_v2_6.h); a clamp for the FACTOR / OVER tables is not traced |
| Bad CRC / version | the stock uses the calibration defaults (flash `0x0804C45C`, byte 0 = `00`) -> DM40A. Emulator check: byte set to 2 without fixing the CRC -> the stock shows the DM40A ranges |
| Writer | only the factory menu "import factor.bin" (whole 800-byte record from the USB disk, CRC and version checked). Nothing else in the firmware writes it |

Not involved (checked):

* **OTP** (`0x1FFF7800`, `otp_1FFF7800.bin`): all 0xFF.
* **Option bytes**: no model use found (the RE headers trace none; the model
  byte alone explains the observed A / B / C behaviour, section 5).
* **UID-bound loader block** `0x080A4000` (docs/genuine-check.md): the genuine
  check, a serial-like word, HW version 4 and bootloader version 1.1. No model.
* **FS-loader** string `"ATK-DM-40"` and the update file: one image for all
  three models. The loader does not check a model.
* **BT name**: an output, not an input (section 2).

So the hardware is the same and the factory sets the model in the calibration
record. The stock HY3131 setup (config rows, relays, OSR) is the same on all
three models.

## 2. What the stock V2.6 changes per model

| Area | A (N=0) | B (N=1) | C (N=2) | Source |
|---|---|---|---|---|
| Range limit factor (limit = factor x decade) | 4 | 5 | 6 | `0x0804A45C`, dmm_v2_6.h:297 |
| Overrange factor (OL above, autorange up) | 1.05 | 1.04 | 1.03 | `0x0804A468` |
| Range labels (MF range pages) | 400mV, 4V .. 400V, 1000V; 400uA .. 4A, 10A; 400 ohm .. 40M ohm; 4nF .. | 500mV, 5V ..; 500uA ..; 500 ohm ..; 5nF .. | 600mV, 6V ..; 600uA ..; 600 ohm ..; 6nF .. | strings `0x08078704..0x080788A3`, 3 per range |
| AC+DC sub-mode (V and A) | not offered (DC -> AC -> DC, no AC+DC button) | offered | offered | dmm_v2_6.h:12, 255 |
| Capacitance ramp comparator R21 | value - 1 | value - 1 | `{DC, CC, BB}` | dmm_v2_6.h:164, 326 (an analog threshold, not a counts limit) |
| Frequency auto range (x = factor x over) | 4.2 kHz step | 5.2 | 6.18 | dmm_v2_6.h:192 |
| About page "Model" | `%s:DM40A` | `%s:DM40B` | `%s:DM40C` | `0x080406CC..` |
| BT module name (AT+SBN, on configuring the module) | DM40A | DM40B | DM40C | `0x0800D206..`, FUN_0800ceaa |
| BT cmd 00 (discovery) reply | `05 03 'A'` | `'B'` | `'C'` | FUN_0800cf32 |

Everything else (scope, generator, UI layout, calibration use) is the same.

## 3. This unit

* Dump 2026-10-13 (`stock/re/dumps/full-20261013/flash_08000000_1MB.bin`):
  `0x080C0168` = `00`, record CRC valid -> **DM40A**.
* Device, 2026-10-22, read-only: `CALIB.BIN` (the opendm40 safety copy of
  `0x080C0000`, `dm40scpi.py get CALIB.BIN`; DBG:BOOT? shows `CALBK` with no
  difference against the flash) is byte-identical to the dump's 800-byte
  record: model byte `00`. opendm40 on the device answers
  `*IDN?` -> `Alientek,DM40A,0,dm40-fw 0.1`.
* Manual and community: most owners have DM40C (docs/user-feedback.md); this
  repo's unit is one of the few DM40A.

## 4. opendm40

User decisions 2026-10-13 (firmware/TODO.md, "User decisions"): **60000 counts
(DM40C limits) on every model**, **AC+DC on every model** (the DM40A hardware
measures it: same HY3131 rows), **IDN / Device page keep the real model**.
The model is in the calibration record and the hardware is the same, so the
40000 / 50000 limits would only cut the range. opendm40 keeps them:

* `dmm.rs`: `Dmm::model()` reads the stock byte the stock way (byte at
  `+0x168`, clamped to 2; a record without version 1 counts as DM40A, as the
  stock defaults; opendm40 does not check the CRC). `Dmm::model_raw()` = the
  byte as stored. `COUNTS_MODEL = 2` keeps FACTOR / OVER at the DM40C values
  (set it to `self.model` in `factor()` / `over()` for the stock limits).
  The capacitance comparator follows the real model byte, as the stock does.
* `ui.rs range_items`: DM40C labels (600mV / 6V / ...) on all models.
* `*IDN?`: `Alientek,DM40<A|B|C>,<serial>,dm40-fw 0.1` from the model (was a
  constant DM40A until 2026-10-22; serial `0` until 2026-10-23, now the chip
  UID as 24 hex digits, docs/native-boot.md "USB serial number").
  `SYST:IDN:EMUL 34401A` unchanged.
* `SYSTem:MODel?` (USBTMC and BT SCPI; read-only): `DM40B,1,60000` =
  model name, raw calibration byte (decimal), counts opendm40 uses.
* About page "Model", BT name `AT+SBNDM40x` and BT cmd 00 `'A'+model`: as the
  stock.

## 5. Emulator

`emu/run.py --model A|B|C` patches `0x080C0168` and rewrites the record CRC
in the composed flash image (with `--flash IMG`: in a copy); default = the
dump's byte (A). The GTK panel (`emu/live.py`) has a DM40A / B / C dropdown
in the header bar next to stock / fwrs / chain. A change restarts the
emulator and updates the model printed on the panel. Both firmwares read the
patched flash.

Verified 2026-10-22 (stock V2.6, MF key on the DMM page, `emu/run.py stock
--model X --key mf@4 --shot ...@5.5`):

| --model | Voltage Settings page |
|---|---|
| A | 400mV, 4V, 40V, 400V, 1000V, AUTO, AUTO+; AC, DC (no AC+DC) |
| B | 500mV, 5V, 50V, 500V, 1000V, AUTO, AUTO+; AC, DC, AC+DC |
| C | 600mV, 6V, 60V, 600V, 1000V, AUTO, AUTO+; AC, DC, AC+DC |

fw-rs with `--model B`: `*IDN?` -> `Alientek,DM40B,0,dm40-fw 0.1`,
`SYST:MOD?` -> `DM40B,1,60000`; its Voltage page keeps 600mV .. 1000V and
AC+DC (section 4).
