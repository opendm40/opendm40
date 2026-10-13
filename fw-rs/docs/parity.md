# fw-rs vs stock DM40 V2.6 — parity report

Item-by-item comparison of the Rust replacement firmware (`fw-rs/src/*.rs`) against
the reverse-engineered stock behaviour (`firmware/re/analysis/*_v2_6.h`). Status
per item: **MATCH**, **PARTIAL** (what is missing in parentheses), **MISSING**,
**UNKNOWN** (needs an on-device check). The device in hand is a DM40A (model
byte 0). AC+DC and the 50/60 k-count model variants are DM40B/C only and out of
scope for this unit.

Overall: the DMM, scope and generator engines are close ports of the stock
numerics. The real gaps are in reporting/telemetry (BT flags, Device/About page),
a few generator snap edge cases, the generator default, touch robustness
(single controller, no Goodix fallback, bit-banged I2C), and the entirely new
features (combined views, USBTMC, data logger) which have no stock equivalent to
match. Nothing in the DMM value path looks wrong.

---

## 1. Multimeter (DMM)

Stock: `hy3131_v2_6.h`, `dmm_v2_6.h`. fw-rs: `dmm.rs`, `hy3131.rs`,
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
| Continuity | MATCH (beep is hardware) | `poll_cont` `dmm.rs:956`; stock drives the buzzer from the HY3131 CMPO pin, not firmware, so fw-rs correctly does nothing. UNKNOWN whether the comparator-to-buzzer path works without stock init — verify on device. |
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
| AC+DC sub-mode | N/A (DM40A) | Not implemented by design; stock only offers it on B/C. |

DMM verdict: faithful. Only the intentional red main value and the corrected
temperature-F factor differ; both are deliberate.

---

## 2. Oscilloscope

Stock: `scope_gen_v2_6.h`, `scope_gen_ui_v2_6.h`. fw-rs: `scope.rs`, `scope_meas.rs`.

| Item | Status | Notes |
|---|---|---|
| Timebase hardware (TIM1/8 psc/arr, DMA) | MATCH | `HW` table `scope.rs:49` = stock `scope_timebase[31]`. |
| User timebase range | PARTIAL (slow end) | fw-rs `TB_MIN=3`..`TB_MAX=23` = 100 ns/div..500 ms/div (`scope.rs:125`); stock user range is 3..29 = **100 ns..50 s/div** (`SCOPE_TB_USER_MAX 29`). fw-rs stops at 500 ms because it uses one record per frame instead of the stock 65520-sample ping-pong/zoom scheme. 1 s..50 s/div MISSING. |
| V/div 10 mV..10 V, front-end GPIO rows | MATCH | `VDIV`/`GPIO_ROW` `scope.rs:130` = `scope_range_gpio`; PC13 relay 200 ms wait kept. |
| Probe x1/x10/x100 | PARTIAL | `probe` scales readouts/labels only (`scope.rs` Settings.probe), matching stock semantics, but the stock probe index spans x0.1..x1000 (`scope_probe[13]`). fw-rs exposes x1/x10/x100 only. Minor. |
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
| AUTO-set | PARTIAL | fw-rs has an autoset state machine (`app.rs:81`), two-phase. Not verified to reproduce the stock period→timebase table (`FUN_08033388`) exactly. UNKNOWN on device. |
| Zoom in STOP | MISSING | Stock re-renders a stored 65520-sample record at a new s/div while stopped (`scope_gen_ui_v2_6.h` §3 Zoom). fw-rs keeps one screen per frame, so no stop-and-zoom. |
| Math / FFT channel | MISSING | Stock has a math/FFT channel (S+0xd4, purple markers). Not in fw-rs. Likely out of scope. |

---

## 3. Generator

Stock: `scope_gen_v2_6.h` §GENERATOR, `scope_gen_ui_v2_6.h` §5. fw-rs: `gen.rs`.

| Item | Status | Notes |
|---|---|---|
| Waveforms sine/square/triangle/ramp | MATCH | `dac()` `gen.rs:168`; same sample formulas incl. sine (c,w)=(50,4045)/(200,4095) split, DAC buffer on for square only. |
| DAC path n / TIM6 PSC | MATCH | `gen.rs:170`: f>5000 PSC=0 n=10e6/f; else k=round(5000/f). |
| PWM square >50 kHz | MATCH | `pwm()` `gen.rs:147`; PWM_HZ/PSC/PER match `gen_pwm_*`. |
| Vpp 0.5..3.0, duty 0..10000 | MATCH | Clamped in `apply()` `gen.rs:130`. |
| Runs while other pages shown (DMA/timers) | MATCH | Output is DMA/timer driven; state shown on the scope bar. |
| **Default wave/freq** | PARTIAL | fw-rs default = **Sine, 1000 Hz** (`gen.rs:60`); stock default = **Square, 2 MHz** (`GEN_DEF_WAVE 1`, `GEN_DEF_FREQ_HZ 2000000`, corrected in `scope_gen_ui_v2_6.h:291`). |
| Freq snap 50k..100k | PARTIAL | Stock snaps square 50k–100k **down to 50 kHz** (`scope_gen_ui_v2_6.h:294`). fw-rs `snap_freq` rounds **up to 100 kHz** (`gen.rs:70`, rev-find returns PWM_HZ[0]). |
| Non-square >50 kHz | PARTIAL | Stock sets freq to **0 Hz (silent, on-flag kept)**. fw-rs clamps to **50 kHz and keeps outputting** (`gen.rs:78`). |
| Keypads (freq H/K/M, numeric Vpp/duty) | MATCH | `View::Keypad`, stock layouts (commit 2464fca). |
| RUN toggles output | MATCH | In `app.rs` RUN handling. |

---

## 4. Bluetooth phone-app protocol

Stock: `bt_meas_v2_6.h`. fw-rs: `bt.rs` (+ `power.rs` for the status it should report).

| Item | Status | Notes |
|---|---|---|
| Frame format, CRC, id 0503/FFFF, len≤0xC2 | MATCH | `poll()` `bt.rs:92`; broadcast detection, resync on bad id/len. |
| Transport: UART4, 115200 8N1, DMA RX | MATCH (DMA vs ISR) | `bt.rs:54`; stock uses RBNE interrupt, fw-rs uses circular DMA (IRQs are masked). Functionally equivalent. |
| cmd 00 discover (model letter) | MATCH | `bt.rs:229`. |
| cmd 01 nop, 02 version (boot data) | MATCH | `bt.rs:231`; version bytes from 0x080A4000. |
| cmd 03 reset, 04 hold, 05 rel | MATCH | `bt.rs:239`; REL gating (not HOLD/not OL, modes <11 or 14). |
| cmd 06 mode byte (fwd + reverse) | MATCH | `mode_byte`/`apply_mode_byte` `dmm.rs:518`,`558`; full group/range/sub decode. |
| cmd 07 set name / 08 get name (AT) | MATCH | `bt.rs:259`; AT+SBN / AT+CBN, "OK"/"CBN+" parsing. |
| cmd 09 measurement reply (fmt + values) | PARTIAL | Encoding in `encode()` `bt.rs:183` matches format-byte rules incl. continuity 0xFFF0 bit 7 and diode value-3. BUT see flags row. |
| **Flags byte (bit0-2 battery, bit3 charge, bit6 lock)** | PARTIAL | `bt.rs:285` hardcodes `... | 5` (battery always level 5) and never sets charging (bit 3) or key-lock (bit 6). `power.rs` computes real `bars`/`charging()` and `app.rs` tracks `locked`, but none is passed into `Bt::serve`. Reports HOLD (bit7) and REL (bit5) correctly. |
| **Startup AT init (SBNDM40A/SLED0/RESET)** | MISSING | Stock configures the module name/LED on first BT-on (`bt_meas_v2_6.h` §"AT commands sent without a request"). fw-rs only handles set/get-name requests; it assumes the module is already configured. |
| PC14 "connected" / PC0 power control | PARTIAL | `power.rs:115` reads PC14; stock also pulses PC0 (module power/reset). fw-rs does not manage PC0. |
| Temperature F in reply = 1.82 (stock bug) | DEVIATION | fw-rs uses 1.8 for the LCD; if the BT path reuses that, the phone app will read a slightly different F than stock. Confirm the BT value matches whatever the app expects. |

---

## 5. GUI / menus

Stock: `gui_v2_6.h`, `gui_render_v2_6.h`, `lcd_buzzer_v2_6.h`. fw-rs: `ui.rs`,
`app.rs`, `text.rs`, `stockfont.rs`, `fonts.rs`, `lcd.rs`.

| Item | Status | Notes |
|---|---|---|
| DMM screen layout, stock fonts, pixel layout | MATCH | Rendered in place from the stock app fonts; main value red. |
| Main menu tiles | PARTIAL (by design) | Stock: 4 tiles + "正点原子" logo (`gui_v2_6.h:53`). fw-rs: `ui::main_menu` with no Alientek logo (requirement). Tiles present via view selection. |
| Settings: dim, shutdown, backlight, volume, BT, start page | MATCH | `View::Settings` `app.rs:288`; dim indices 5/10/30/never, shutdown 10/30/60/never. |
| Language selection (zh-CN/zh-TW/en) | MISSING (by design) | fw-rs is English only; stock has 3 languages (`CFG_LANGUAGE`). Acceptable unless CJK is wanted. |
| Device / About page (Vendor/Web/Model/version) | MISSING | Stock §6 Device page. fw-rs has none. Minor. |
| Per-function DMM settings pages (MF: Voltage/Current/Resistor/Cap Settings) | PARTIAL | Stock opens dedicated per-function settings screens from MF (`gui_v2_6.h:65`). fw-rs uses a `View::Range` popup + `ScopeMenu`; fewer per-function options. |
| "Other Settings" toggles (Anti-drop, Mode Remember, BT) | MATCH-ish | Anti-drop = `reassert_relay`; Mode Remember / persist via backup SRAM; BT toggle present. Stored in backup SRAM instead of config.bin. |
| Idle dim + auto power-off | MATCH | `tick()` `app.rs:344`; dims backlight, power-off dialog/action. |
| Combined view (menu row "combo") | NEW | fw-rs adds a DMM+Scope combined view (no stock equivalent). |

---

## 6. Touch

Stock: `touch_v2_6.h`. fw-rs: `touch.rs`, `gesture.rs`.

| Item | Status | Notes |
|---|---|---|
| Hynitron 0x5A protocol (D1 01 / D2 04 / D1 09, D0 00 frame, 0xAB, ack) | MATCH | `touch.rs:236`,`259`; chip-type 0xA8 check, extra points read+dropped, D0 00 AB ack. |
| Coordinate decode X/Y, landscape | MATCH | `decode()` `touch.rs:198`. |
| Side keys MENU/RUN/AUTO/MF from key-report X | MATCH | `touch.rs:285`; thresholds 500/750/1000 (measured 372/628/884/1140). |
| Debounce / long-press filtering | MATCH (fw-rs adds more) | `DEBOUNCE_MS` + two-identical-frames; long-press-as-key suppression `touch.rs:279`. Stock relies on controller filtering only. |
| Gestures (tap/long-press/swipe/drag) | NEW | `gesture.rs`; stock has no gestures. Used for view switching. |
| Power key PD6 (hold 1.5 s off, double lock) | MATCH | `app.rs:354`-`362`. |
| **I2C transport** | PARTIAL | fw-rs bit-bangs I2C in the main loop (`touch.rs:131`); stock uses the I2C2 peripheral + DMA + EXTI interrupt. TODO in the header to move to the peripheral. Works but blocks the loop during reads. |
| **Goodix GT1158 fallback (0x14)** | MISSING | Stock probes GT1158 first, falls back to 0x5A. fw-rs supports 0x5A only, so a unit with the Goodix panel would have no touch. UNKNOWN which panels ship. |
| Multi-touch (up to 5 points) | PARTIAL | fw-rs reports point 0 + count only; stock stores up to 5. Single-finger UI, so fine. |

---

## 7. USB

Stock: mass-storage disk only (`spiflash_v2_6.h`) + the BT phone protocol. There
is **no USBTMC/SCPI in stock** — it is a fw-rs addition, so "parity" here means
the mass-storage disk must stay compatible and USBTMC is a superset.

fw-rs: `usb.rs`, `usbtmc.rs`, `scpi.rs`, `msc.rs`, `spiflash.rs`.

| Item | Status | Notes |
|---|---|---|
| Mass storage = stock FAT16 on W25Q128, 4096 B blocks | MATCH | `spiflash.rs` BlockDevice, `msc.rs`; `DBG:FLASH?` returns JEDEC EF4018 (README). LBA == sector == flash/4096 as stock. |
| SPI access read-only vs read/write | UNKNOWN | fw-rs implements writes (erase+program); a block write stalls the loop ~50 ms (up to 400 ms). Mount/write/read round-trip **not logged on device** (TODO.md). |
| Disk always attached (like stock disk mode) | MATCH | Composite always present. |
| USBTMC/USB488 + SCPI (DMM) | NEW | `scpi.rs`: `*IDN? *RST *CLS *OPC? SYST:ERR?`, `CONF/MEAS/READ/FETC`, `FUNC`. Superset of stock. |
| SCPI scope/generator | NEW | `SOUR:*`, `TIM/CHAN/TRIG`, `RUN/STOP/SING`, `SCOP:MEAS?`, `WAV:DATA?/PRE?`. |
| Debug over USBTMC (`DBG:*`) | NEW | Ping/Tap/Key/Swipe/Lcd/Diag/Flash/Release/Reset. |
| On-device USBTMC verification | UNKNOWN | `*IDN?`, screenshot, tap/swipe, disk mount all listed as untested in TODO.md. |

---

## 8. Combined / simultaneous operation (fw-rs only)

Stock switches instruments; fw-rs runs DMM + scope + generator at once and only
switches the view. Constraints and how fw-rs handles them:

- **Analog power rails PC4 (scope) / PE2 (DMM)**: stock powers one side per page
  (`scope.rs:280` `power()`); fw-rs adds `power_both()` (`scope.rs:309`) to keep
  both up. This is **off by default** and the stock never does it, so accuracy of
  simultaneous DMM+scope is UNKNOWN (crosstalk between the two analog front ends
  on the shared supply is untested). The view switcher calls one-side power for
  DMM-only / scope-only views and both only for the combined view (`app.rs:244`).
- **Generator independence**: DAC+DMA / PWM, no CPU involvement, so it genuinely
  runs during any view (MATCH to the stock "generator keeps running while scope
  shown" behaviour, extended to the DMM view).
- **Shared HY3131 vs scope ADC**: separate chips (HY3131 bit-banged SPI vs 3PA1030
  on GPIOB + TIM/DMA), no bus contention. SPI flash is a third SPI peripheral.
- **Risk**: a block write to the SPI disk stalls the main loop up to ~400 ms, which
  would pause DMM/scope polling and touch (bit-banged) during that window.

---

## 9. Data logger (FR-38) — new feature, not a parity gap

Stock has no logger mode. fw-rs has none yet (no module). Relevant HY3131 facts
for FR-38, from `hy3131_v2_6.h`:

- **Fastest conversion** is set by R22 `AD1OSR`, `OSR = 256 << (R22 & 7)`. The
  stock already drops to `R22 = 0x13` (OSR 2048) for fast open-circuit ohm updates
  and `0x14`/`0x15` (8192/… ) elsewhere; OSR 2048 is the lowest it uses.
- **SYSCLK** to the chip is a 4.9152 MHz crystal, so sample interval ≈ OSR/SYSCLK
  (OSR 2048 ≈ 0.42 ms raw, before SPI read overhead); trading bits for speed means
  the low-OSR path. Exact achievable sample rate over the bit-banged SPI link is
  UNKNOWN and must be measured, per the FR-38 note.
- FR-38 wants per-sample device timestamps (µs/ms counter), a monotonic sequence
  number, constant interval, and drop detection; none exist today. The DMM poll is
  a 10 ms cooperative loop, which is not constant-interval — a logger would need a
  timer-driven HY3131 read path independent of the GUI loop.

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
3. **Generator defaults and snap edge cases** — `gen.rs:60` default should be
   Square/2 MHz to match stock; `snap_freq` (`gen.rs:70`) should snap square
   50k–100k **down to 50 kHz** and set non-square >50 kHz to **0 Hz/silent** rather
   than clamping to 50 kHz.
4. **Touch robustness** — add the Goodix GT1158 fallback (`touch.rs`, stock
   `touch_v2_6.h` §Goodix) for units without the 0x5A panel, and move the
   bit-banged I2C to the I2C2 peripheral + EXTI so a disk write / long loop does
   not drop touch frames.
5. **Scope slow timebases + stop-zoom** — extend `TB_MAX` past 500 ms/div toward
   50 s/div and add the stored-record re-render in STOP (`scope.rs`,
   `scope_gen_ui_v2_6.h` §3), using the 65520-sample record instead of one screen.
6. **Confirm simultaneous DMM+scope accuracy** — characterise crosstalk with both
   analog rails on (`scope.rs:309`); decide whether the combined view needs
   recalibration or a warning.
7. **Device/About page** — add the stock §6 Device info (vendor/model/HW/FW) to
   the menu; low effort, low priority.
8. **BT module startup AT init** — send `AT+SBNDM40x` / `AT+SLED0` / `AT+RESET`
   once on BT-on so a factory module is named/LED-configured like stock
   (`bt.rs`, `bt_meas_v2_6.h`). Only matters on a fresh module.
9. **Data logger (FR-38)** — new feature; needs a timer-driven, constant-interval
   HY3131 read path with timestamps + sequence numbers, separate from the GUI loop.

Deliberate deviations (not gaps): red main value, no Alientek logo, English-only
UI, temperature F factor 1.8 (stock bug was 1.82), combined views, USBTMC, touch
gestures.
