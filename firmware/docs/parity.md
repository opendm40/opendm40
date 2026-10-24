# opendm40 vs stock DM40 V2.6 — parity report

Item-by-item comparison of the Rust replacement firmware (`firmware/src/*.rs`) against
the reverse-engineered stock behaviour (`stock/re/analysis/*_v2_6.h`). Status
per item: **MATCH**, **PARTIAL** (what is missing in parentheses), **MISSING**,
**UNKNOWN** (needs an on-device check). The device in hand is a DM40A (model
byte 0). The stock offers AC+DC and the 50/60 k-count ranges only on DM40B/C;
opendm40 offers both on every model.

Overall: the DMM, scope and generator engines are close ports of the stock
numerics. The real gaps are in reporting/telemetry (BT flags), touch robustness
(single controller, no Goodix fallback, bit-banged I2C), and the entirely new
features (combined views, USBTMC, data logger) which have no stock equivalent to
match. Nothing in the DMM value path looks wrong.

---

## 1. Multimeter (DMM)

Stock: `hy3131_v2_6.h`, `dmm_v2_6.h`. opendm40: `dmm.rs`, `hy3131.rs`,
`hy3131_tables.rs`, `meter.rs`.

| Item | Status | Notes |
|---|---|---|
| All 16 HY3131 config tables f0..f15 | MATCH | `dmm.rs:339`-`358` selects the same rows per mode/range; tables in `hy3131_tables.rs`, f13 r7 + discharge rows in `dmm.rs:48`. |
| Relays PA/PE, PC7 per mode | MATCH | `relay_pa`/`relay_pe`/`pc7` `dmm.rs:157`-`186`; PA pulsed only on change (`pa_now`). |
| DC V/mV formulas, neg-gain, r0 linearity | MATCH | `poll_dcv` `dmm.rs:710`; `DCV_NEGK`, 1e-4 correction, 1000 V clamp all present. |
| DC A/mA/uA, shunt/opgain, 10 A filter | MATCH | `poll_dca` `dmm.rs:757`; 4-20 mA loop % aux on mode 8 `dmm.rs:793`. |
| AC V/mV/A: sqrt(RMS), LPF, AC/DCp, EMA windows | MATCH | `poll_ac` `dmm.rs:801`; overload guard `sq > 262144` matches `> 0x40000`. |
| Autorange thresholds + settle deadlines | MATCH | Per-mode `set(...settle)` values match the deadline table in `dmm_v2_6.h` §"SETTLE DEADLINES". |
| AD1 OSR switching (R22 jump thresholds) | MATCH | `osr()` `dmm.rs:697`; DC V 0x206, DC mV 0xE22-0x960·r, DC A 0x168, cont 0x206. |
| Ohm: ratiometric, q-based autorange, p8set/clr | MATCH | `poll_ohm` `dmm.rs:896`; R22 0x13/0x17 switching, 2500 ms fast-down gate. |
| Continuity | MATCH (beep is hardware) | `poll_cont` `dmm.rs:956`; stock drives the buzzer from the HY3131 CMPO pin, not firmware, so opendm40 correctly does nothing. UNKNOWN whether the comparator-to-buzzer path works without stock init — verify on device. |
| Diode + beeper pattern classes | MATCH | `poll_diode` + `diode_beep` `dmm.rs:964`,`1001`; classes/steps/`beep_n<=6` gating match; aux "ohm" = V·gain/0.765. |
| Capacitance RC (r0..4) + ramp (r5..7) | MATCH | `poll_cap` `dmm.rs:1184`; median-5, 35-sample batch, comparator-flip flush, R21 by slope class, discharge row. |
| Frequency / duty, dual-source in mode 14 | MATCH | `poll_freq_mode`/`freq_secondary` `dmm.rs:1056`,`1081`; reciprocal counter, 350 ms gate. |
| Temperature (3-phase K, cold junction) | MATCH, minor | `poll_temp` `dmm.rs:1113`; **F readout uses 1.8** (`dmm.rs:1144`) vs the stock `1.82` bug (`dmm_v2_6.h:212`, `BT_TEMP_F_FACTOR`). Deliberate deviation; BT reply also differs (see §4). |
| Model counts (DM40A/B/C factor/over) | MATCH | `FACTOR`/`OVER` `dmm.rs:26`; model read from cal byte 0, clamped to 2. |
| HOLD / REL / MIN-MAX / 5 history slots | MATCH | In `meter.rs` (shown/hold/rel, min-max, slots); REL freezes range for modes <11 and 14 as stock. |
| Strip chart 160/320/640 ms/div | MATCH | `app.rs` chart; rate in `meter.rs`. |
| HV warning ≥100 V, main value colour | MATCH (intended red) | Main value drawn red (requirement) instead of stock orange/red-from-30 V split (`dmm_v2_6.h:239`). |
| MF manual-range popup, AUTO/AUTO+ | MATCH | `set_range` `dmm.rs:473`; V/A/ohm items, AUTO+ flags. |
| Calibration read from 0x080C0000 +0x168 | MATCH | `cal()` `dmm.rs:130`, CRC/version sanity check; **read-only** as required. |
| AC+DC sub-mode | DEVIATION | Every model (stock: DM40B/C only). The total is shown only once both the AC and the DC part are measured; the stock first shows a total computed from the DC part alone. `dmm.rs`, `acdcm.rs`. |

DMM verdict: faithful. Only the intentional red main value and the corrected
temperature-F factor differ; both are deliberate.

---

## 2. Oscilloscope

Stock: `scope_gen_v2_6.h`, `scope_gen_ui_v2_6.h`. opendm40: `scope.rs`, `scope_meas.rs`.

| Item | Status | Notes |
|---|---|---|
| Timebase hardware (TIM1/8 psc/arr, DMA) | MATCH | `HW` table `scope.rs:49` = stock `scope_timebase[31]`. |
| User timebase range | MATCH | `TB_MIN=3`..`TB_MAX=29` = 100 ns/div..50 s/div, as the stock `SCOPE_TB_USER_MIN`/`MAX`; YT (non-roll) up to `TB_YT_MAX=24` = 1 s/div, 200 ms..50 s/div in roll mode (`scope.rs`). |
| V/div 10 mV..10 V, front-end GPIO rows | MATCH | `VDIV`/`GPIO_ROW` `scope.rs:130` = `scope_range_gpio`; PC13 relay 200 ms wait kept. |
| Probe x0.1..x1000 | MATCH | `probe` scales readouts/labels only (`scope.rs` Settings.probe), matching stock semantics; the full stock list `scope_probe[13]` (`scope::PROBES`) in the scope settings window, Settings tab (UF-12). |
| Gain/offset calibration from record | MATCH | `cal_pair` `scope.rs:184` reads +0x08 (gain) / +0xA8 (offset) with defaults; CCR3 offset PWM formula matches `FUN_080302d6`. |
| DC/AC coupling (PE3) | MATCH | `apply()` `scope.rs:372`. |
| Trigger: level→code→DAC, hysteresis ±2, edge | MATCH | `trig_code`/`find_trigger` `scope.rs:349`,`547`; software refinement with arm band ±2 as `FUN_080123ce`. |
| Trigger modes Auto/Normal/Single | MATCH | `poll()` `scope.rs:457`; Auto untriggered-timeout table matches `scope_auto_timeout_ms`. |
| Roll mode from 200 ms/div | MATCH | `roll_poll` `scope.rs:512`, circular DMA, per-column min/max, 100 ms. |
| 20 measurement items, 4 shown, defaults | MATCH | `ITEMS` + `DEFAULT_SEL` `scope_meas.rs:9`,`33`; Freq/Vpp/Cycl/Vrms default. |
| Measurement formulas (Vpp/rms/top/base, pulse) | MATCH | `compute()` `scope_meas.rs:61`; histogram top/base 30 % band, 10/50/90 % edge machine. |
| Cursors X2-1 / Xf / Y2-1 | MATCH | Items 7/8/17 present; formulas in `app.rs`/`scope_meas.rs`. |
| Persistence | MATCH | Trace persistence in `app.rs` (commit fb8f7db). |
| Settings window (MF) | MATCH | `View::ScopeMenu` tabs. |
| AUTO-set | PARTIAL | opendm40 has an autoset state machine (`app.rs:81`), two-phase. Not verified to reproduce the stock period→timebase table (`FUN_08033388`) exactly. UNKNOWN on device. |
| Zoom in STOP | PARTIAL (shorter record) | While stopped, a timebase or V/div change re-renders the stored record, and a drag pans it (`scope.rs` "Stop-and-zoom"). The record is 26 624 samples; the stock keeps 65 520. |
| Math / FFT channel | MATCH, extended | `fft.rs`: 2048 points, six windows (Rectangular, Blackman, Hanning, Hamming, Triangular, Flat-top), dB or Vrms; ABS math mode. opendm40 adds the dB/div, centre-frequency, Hz/div and peak readouts. |

---

## 3. Generator

Stock: `scope_gen_v2_6.h` §GENERATOR, `scope_gen_ui_v2_6.h` §5. opendm40: `gen.rs`.

| Item | Status | Notes |
|---|---|---|
| Waveforms sine/square/triangle/ramp | MATCH | `dac()` `gen.rs:168`; same sample formulas incl. sine (c,w)=(50,4045)/(200,4095) split, DAC buffer on for square only. |
| DAC path n / TIM6 PSC | MATCH | `gen.rs:170`: f>5000 PSC=0 n=10e6/f; else k=round(5000/f). |
| PWM square >50 kHz | MATCH | `pwm()` `gen.rs:147`; PWM_HZ/PSC/PER match `gen_pwm_*`. |
| Vpp, duty 0..10000 | DEVIATION | Vpp 0..3.0 (stock 0.5..3.0), so that an offset with 0 Vpp is a DC output; adjustable offset (stock: fixed 0..Vpp). `genplan.rs` `AMP_MIN`. |
| Runs while other pages shown (DMA/timers) | MATCH | Output is DMA/timer driven; state shown on the scope bar. |
| Default wave/freq | MATCH | Square, 2 MHz, 3 Vpp, output off (`genplan.rs` `Settings::DEFAULT`), as the stock. |
| Freq snap 50k..100k | DEVIATION | Stock snaps a square of 50k–100k down to 50 kHz (`scope_gen_ui_v2_6.h:294`). opendm40 runs it on the PWM path at the nearest frequency 200 MHz / D can make (`genplan.rs` `plan_pwm`), and shows that real frequency. |
| Non-square >50 kHz | DEVIATION | Stock sets the frequency to 0 Hz (silent, on-flag kept). opendm40 runs at 50 kHz and shows "max 50 kHz"; the set value is kept (`genplan.rs` `target_hz`, `max_note`). |
| Keypads (freq H/K/M, numeric Vpp/duty) | MATCH | `View::Keypad`, stock layouts (commit 2464fca). |
| RUN toggles output | MATCH | In `app.rs` RUN handling. |

---

## 4. Bluetooth phone-app protocol

Stock: `bt_meas_v2_6.h`. opendm40: `bt.rs` (+ `power.rs` for the status it should report).

| Item | Status | Notes |
|---|---|---|
| Frame format, CRC, id 0503/FFFF, len≤0xC2 | MATCH | `poll()` `bt.rs:92`; broadcast detection, resync on bad id/len. |
| Transport: UART4, 115200 8N1, DMA RX | MATCH (DMA vs ISR) | `bt.rs:54`; stock uses RBNE interrupt, opendm40 uses circular DMA (IRQs are masked). Functionally equivalent. |
| cmd 00 discover (model letter) | MATCH | `bt.rs:229`. |
| cmd 01 nop, 02 version (boot data) | MATCH | `bt.rs:231`; version bytes from 0x080A4000. |
| cmd 03 reset, 04 hold, 05 rel | MATCH | `bt.rs:239`; REL gating (not HOLD/not OL, modes <11 or 14). |
| cmd 06 mode byte (fwd + reverse) | MATCH | `mode_byte`/`apply_mode_byte` `dmm.rs:518`,`558`; full group/range/sub decode. |
| cmd 07 set name / 08 get name (AT) | MATCH | `bt.rs:259`; AT+SBN / AT+CBN, "OK"/"CBN+" parsing. |
| cmd 09 measurement reply (fmt + values) | PARTIAL | Encoding in `encode()` `bt.rs:183` matches format-byte rules incl. continuity 0xFFF0 bit 7 and diode value-3. BUT see flags row. |
| **Flags byte (bit0-2 battery, bit3 charge, bit6 lock)** | PARTIAL | `bt.rs:285` hardcodes `... | 5` (battery always level 5) and never sets charging (bit 3) or key-lock (bit 6). `power.rs` computes real `bars`/`charging()` and `app.rs` tracks `locked`, but none is passed into `Bt::serve`. Reports HOLD (bit7) and REL (bit5) correctly. |
| **Startup AT init (SBNDM40A/SLED0/RESET)** | MISSING | Stock configures the module name/LED on first BT-on (`bt_meas_v2_6.h` §"AT commands sent without a request"). opendm40 only handles set/get-name requests; it assumes the module is already configured. |
| PC14 "connected" / PC0 power control | MATCH | `power.rs` reads PC14; `Bt::power` drives PC0 low (module on) while Bluetooth is ON and high when OFF, plus the stock's 100 ms high pulse after a successful cmd 07. Until 2026-10-22 native opendm40 left PC0 high from `board::init` (the stock boot level): the module stayed off and the meter was unreachable over BT. Deviation: stock powers the module only on its meter page, opendm40 on every view (UF-36). |
| Temperature F in reply = 1.82 (stock bug) | DEVIATION | opendm40 uses 1.8 for the LCD; if the BT path reuses that, the phone app will read a slightly different F than stock. Confirm the BT value matches whatever the app expects. |

---

## 5. GUI / menus

Stock: `gui_v2_6.h`, `gui_render_v2_6.h`, `lcd_buzzer_v2_6.h`. opendm40: `ui.rs`,
`app.rs`, `text.rs`, `stockfont.rs`, `fonts.rs`, `lcd.rs`.

| Item | Status | Notes |
|---|---|---|
| DMM screen layout, stock fonts, pixel layout | MATCH | Rendered in place from the stock app fonts; main value red. |
| Main menu tiles | PARTIAL (by design) | Stock: 4 tiles + "正点原子" logo (`gui_v2_6.h:53`). opendm40: `ui::main_menu` with no Alientek logo (requirement). Tiles present via view selection. |
| Settings: dim, shutdown, backlight, volume, BT, start page | MATCH | `View::Settings` `app.rs:288`; dim indices 5/10/30/never, shutdown 10/30/60/never. |
| Language selection (zh-CN/zh-TW/en) | MISSING (by design) | opendm40 is English only; stock has 3 languages (`CFG_LANGUAGE`). Acceptable unless CJK is wanted. |
| Device / About page (Vendor/Web/Model/version) | MATCH | Settings > More > More > Device (`app.rs`): vendor, web, model, bootloader, software and hardware versions, plus the board temperature. |
| Per-function DMM settings pages (MF: Voltage/Current/Resistor/Cap Settings) | PARTIAL | Stock opens dedicated per-function settings screens from MF (`gui_v2_6.h:65`). opendm40 uses a `View::Range` popup + `ScopeMenu`; fewer per-function options. |
| "Other Settings" toggles (Anti-drop, Mode Remember, BT) | MATCH-ish | Anti-drop = `reassert_relay`; Mode Remember / persist via backup SRAM; BT toggle present. Stored in backup SRAM instead of config.bin. |
| Idle dim + auto power-off | MATCH | `tick()` `app.rs:344`; dims backlight, power-off dialog/action. |
| Combined view (menu row "combo") | NEW | opendm40 adds a DMM+Scope combined view (no stock equivalent). |

---

## 6. Touch

Stock: `touch_v2_6.h`. opendm40: `touch.rs`, `gesture.rs`.

| Item | Status | Notes |
|---|---|---|
| Hynitron 0x5A protocol (D1 01 / D2 04 / D1 09, D0 00 frame, 0xAB, ack) | MATCH | `touch.rs:236`,`259`; chip-type 0xA8 check, extra points read+dropped, D0 00 AB ack. |
| Coordinate decode X/Y, landscape | MATCH | `decode()` `touch.rs:198`. |
| Side keys MENU/RUN/AUTO/MF from key-report X | MATCH | `touch.rs:285`; thresholds 500/750/1000 (measured 372/628/884/1140). |
| Debounce / long-press filtering | MATCH (opendm40 adds more) | `DEBOUNCE_MS` + two-identical-frames; long-press-as-key suppression `touch.rs:279`. Stock relies on controller filtering only. |
| Gestures (tap/long-press/swipe/drag) | NEW | `gesture.rs`; stock has no gestures. Used for view switching. |
| Power key PD6 (hold 1.5 s off, double lock) | MATCH | `app.rs:354`-`362`. |
| **I2C transport** | MATCH (no DMA) | Hardware I2C2 (400/100 kHz by rise time, bit-bang fallback) + EXTI5 pending-bit gating; no DMA (`touch.rs` header). Stock uses the I2C2 peripheral + DMA + EXTI interrupt. Raw bus/IRQ counters in DBG:DIAG? bytes 20..59. |
| **Goodix GT1158 fallback (0x14)** | MISSING | Stock probes GT1158 first, falls back to 0x5A. opendm40 supports 0x5A only, so a unit with the Goodix panel would have no touch. UNKNOWN which panels ship. |
| Multi-touch (up to 5 points) | PARTIAL | opendm40 reports point 0 + count only; stock stores up to 5. Single-finger UI, so fine. |

---

## 7. USB

Stock: one mass-storage disk ("USB Connect": the SPI flash's FAT16 as it is,
`spiflash_v2_6.h`) and no other USB function; the phone-app protocol is
Bluetooth only. USBTMC / SCPI is an opendm40 addition.

opendm40 (native build): `usb.rs`, `usbtmc.rs`, `scpi.rs`, `scpi_msg.rs`,
`msc.rs`, `vdisk.rs`, `vwrite.rs`. One composite device, always present:
USBTMC (USB488) plus one mass-storage LUN.

| Item | Status | Notes |
|---|---|---|
| Mass storage | CHANGED | Not the raw SPI FAT16: a FAT16 volume generated on the fly (`vdisk.rs`): `LOGS/*.CSV` and `SCOPE/*.CSV` made from the DLG files, `SHOTS/*.BMP`, `INFO/` (calibration, boot data, UID, option bytes, OTP, loader images, `FIRMWARE.BIN`, `BACKUP.TXT`), `DEBUG/SPIFLASH.BIN` (the raw 16 MiB), `README.TXT`. |
| Writes from the PC | CHANGED | "Files live" (default): read-only. "PC can edit" (Settings 3/3, at the next plug): deletes in LOGS / SCOPE / SHOTS and a copied `dm40*.bin` (checked, then "Install it now?") are taken (`vwrite.rs`); other files are not kept. The stock disk keeps whatever the PC writes. |
| Firmware update from the disk | MATCH | Any `dm40*.bin` (stock app or the vendor loader's disk); opendm40 also stages one copied to its own disk, or sent with `DBG:UPLOAD` (`tools/reload.sh`). |
| USBTMC/USB488 + SCPI (DMM) | NEW | IEEE 488.2 common commands, `SYST:ERR?` with an 8-entry error queue and standard codes, `CONF/MEAS/READ/FETC`, `FUNC`, ranges and resolution, `CALC`, temperature, the power meter (`POW`), inrush (`INR`), alerts (`SYST:ALER`, `STAT:QUES`). |
| SCPI scope / generator | NEW | `SOUR:*`, `OUTP`, `TIM/CHAN/TRIG`, `RUN/STOP/SING/DIG`, `ACQ:SRAT?`, `SCOP:MEAS?`, `WAV:DATA?/PRE?/POIN/FORM`. |
| SCPI files and logger | NEW | `MMEM:CAT?/DATA?/DEL`, `LOG:*` (section 9). |
| Debug over USBTMC (`DBG:*`) | NEW | Ping, tap / key / swipe / drag, LCD read-back, boot info, upload / update, profiling, the HY3131 and AD2 experiments. Refused over Bluetooth, as `DIAG`. |
| HP 34401A identity | NEW | `SYST:IDN:EMUL 34401A`: libsigrok / SmuView's 34401A profile drives the meter (emulator: `sigrok-cli -d scpi-dmm` reads). |

The reference for hosts: [docs/manual/scpi-reference.md](../../docs/manual/scpi-reference.md).

---

## 8. Combined / simultaneous operation (opendm40 only)

Stock switches instruments; opendm40 runs DMM + scope + generator at once and only
switches the view. Constraints and how opendm40 handles them:

- **Analog power rails PC4 (scope) / PE2 (DMM)**: stock powers one side per page.
  opendm40's policy is `SYST:RAIL AUTO|BOTH` (default AUTO, persisted): AUTO
  powers the side the view needs (both in the combined view and with a host
  scope command), BOTH keeps both up. Measured with both rails on (device check
  f3aea2f, docs/device-check-2026-10-21.md): DC within 0.04 %; a 40 Hz 0..9.6 V
  square reads +0.19 % more in V (edge-rate dependent coupling of the powered
  scope front end), which is why AUTO keeps the scope rail off on the DMM page.
- **Generator independence**: DAC+DMA / PWM, no CPU involvement, so it runs
  during any view (the stock keeps it running with the scope; opendm40 also with
  the DMM).
- **Shared HY3131 vs scope ADC**: separate chips (HY3131 bit-banged SPI vs 3PA1030
  on GPIOB + TIM/DMA), no bus contention. SPI flash is a third SPI peripheral.
- **SPI-flash writes**: done in bounded slices from the main loop (CSV commits,
  screenshots, the fast logger's pre-erased clusters), so the DMM, USB and touch
  keep running; a screenshot freezes the screen for ~1.5 s
  ([ram-budget.md](ram-budget.md)).

---

## 9. Data logger (FR-38) — new feature, not a parity gap

Stock has no logger. opendm40 (`logger.rs`, `fastlog.rs`, `csvlog.rs`):

- **Record page** (`csvlog.rs`): DMM readings at an interval (every reading,
  1 s, 10 s, 60 s, mean per row) to `LOGS/LOGnnn.DLG`, shown as CSV on the USB
  disk; `LOG:CSV`.
- **Fast logger** (`logger.rs`): raw AD1 samples (40 .. 640 S/s, LOG:OSR /
  LOG:CHOP) or the AD2 stream (1200 .. 76 800 S/s, `ad2-isr` builds), each
  record with a sequence number and a device microsecond timestamp, so a gap
  is visible; streamed to the host (`LOG:DATA?`) or to the SPI disk
  (`LOG:DEST DISK`: `LOGS/FASTnnn.DLG`, `fastlog.rs`).
- Over Bluetooth only AD1 sessions (`Rate too high for Bluetooth`).
- Formats: [compact-log-format.md](compact-log-format.md),
  [log-format-standards.md](log-format-standards.md).

---

## Prioritised punch-list (most important first)

1. **BT flags byte: report real battery / charging / lock** — `bt.rs:285`
   hardcodes battery level 5 and omits charging (bit 3) and key-lock (bit 6). Pass
   `Power` (and `app.locked`) into `Bt::serve` and build the flags from
   `power.bars`, `power.charging()`, and the lock state. The phone app shows a
   permanently-full battery today.
2. **On-device verification of USBTMC + SPI disk** — TODO.md items; mount /
   write / read round-trip and `*IDN?`/screenshot/tap/swipe are unlogged. Fastest
   to close by running `tools/dm40dbg.py` once the native-boot work lands.
3. **Touch robustness** — add the Goodix GT1158 fallback (`touch.rs`, stock
   `touch_v2_6.h` §Goodix) for units without the 0x5A panel, and move the
   bit-banged I2C to the I2C2 peripheral + EXTI so a disk write / long loop does
   not drop touch frames.
4. **Confirm simultaneous DMM+scope accuracy** — characterise crosstalk with both
   analog rails on (`scope.rs:309`); decide whether the combined view needs
   recalibration or a warning.
5. **BT module startup AT init** — send `AT+SBNDM40x` / `AT+SLED0` / `AT+RESET`
   once on BT-on so a factory module is named/LED-configured like stock
   (`bt.rs`, `bt_meas_v2_6.h`). Only matters on a fresh module.
6. **Data logger (FR-38)** — new feature; needs a timer-driven, constant-interval
   HY3131 read path with timestamps + sequence numbers, separate from the GUI loop.

Deliberate deviations (not gaps): red main value, no Alientek logo, English-only
UI, temperature F factor 1.8 (stock bug was 1.82), AC+DC on every model, the
generator's 50 kHz clamp and nearest-frequency square, combined views, USBTMC,
touch gestures.
