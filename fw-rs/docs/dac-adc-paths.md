# DAC -> ADC paths on the DM40 (loopback / self-test survey)

Question: can the generator output (or any MCU DAC channel) reach the scope ADC
or the HY3131 meter ADC without an external wire, so the firmware can self-test
or calibrate? Short answer: **only at the MCU pin level (DAC pin read back by
the MCU's own ADC) and through the trigger comparator; never into the scope
ADC or the HY3131.** Everything else needs a cable.

Tags: **VERIFIED** = read in the stock disassembly / fw-rs driver that runs on the
device, or exercised on the device; **INFERRED** = interpretation (board wiring
is not traced; no schematic in the repo). Sources: `fw-rs/src/*.rs`,
`firmware/re/analysis/*.h`, `firmware/re/analysis/app_decompile.c`,
`DM40C_feature_request.md` (BOM from photos; its "[Вывод]" = inference).

## 1. DAC channels

| Channel / pin | Drives | Status | Evidence |
|---|---|---|---|
| DAC ch1 = PA4 (`DAC_OUT0`) | Generator waveform: TIM6 TRGO at 10 MHz/(PSC+1), DMA0 stream 5 ch7 from the wave table, 12-bit unipolar 0..amp (3.0 V ref assumed) | VERIFIED | `fw-rs/src/gen.rs:3-7,232-252`; `scope_gen_v2_6.h:105-131` (FUN_080166a4) |
| DAC ch2 = PA5 (`DAC_OUT1`) | **Scope trigger level** for a hardware comparator: `DHR12R2` = 1780 + (code - t0)(2401 - 1780)/(t1 - t0); written at every acquisition start; comparator output -> PA15 AF1 TIM2_CH1 | VERIFIED (stock programs DAC2/PA5 analog and PA15/TIM2: FUN_08012260 `app_decompile.c:13615`, FUN_0801241a); comparator chip itself INFERRED ("guess" in the header) | `scope_gen_v2_6.h:50-59`; `scope_gen_ui_v2_6.h:119-125,471-472` (`SCOPE_TRIG_DAC_HI/LO`) |
| (not a DAC) PC8 TIM3_CH3 PWM, ARR 20000 | Scope vertical offset (CCR3 8800..11636 maps to ADC code ~280..730) | VERIFIED | `scope.rs:260-271,432-439`; `scope_gen_v2_6.h:89-91` |
| PB12 GPIO | Generator output-path select: 0 = DAC (PA4), 1 = PWM (PB14 TIM12_CH1, square > 50 kHz) | VERIFIED (pin use); the switch is very likely the SGM3157 SPDT a user burned at 12 V | `gen.rs:6,232-235`; `scope_gen_v2_6.h:130-131`; `fw-rs/docs/user-feedback.md:250,307` (INFERRED) |
| External DAC on SPI/I2C | none: SPI peripherals are W25Q128 flash and the bit-banged HY3131; no DAC in the BOM | INFERRED (negative) | `hy3131_v2_6.h:4-6`; `DM40C_feature_request.md:30-50` |

fw-rs never enables DAC ch2, PA15 or TIM2: the trigger is found in software in
the record (`scope.rs:8-9,777-800`; no `TIM2`/`PA15` in `fw-rs/src`). The
generator DAC output buffer is off (BOFF1 = 1) for sine/triangle/ramp, which
implies an external buffer/op-amp stage on PA4 (INFERRED, `scope_gen_v2_6.h:126-127`).

## 2. MCU ADC channels

Only **ADC0** is used, by both stock and fw-rs:

| ADC0 channel / pin | Source | Status | Evidence |
|---|---|---|---|
| ch15 = PC5 (`ADC012_IN15`) | Battery, Vbat = raw * 3.0/4096 * 2 | VERIFIED | `power.rs:2,50-59`; `dmm_v2_6.h:278,369`; stock configures exactly one channel (FUN_0800458e(adc, 0, 0xf, 6), single call site `app_decompile.c:6360`) |
| ch16 (internal temperature sensor, TSVREN) | MCU die temperature | VERIFIED (fw-rs only) | `power.rs:6-7,54-56` |
| ch4 = PA4 (`ADC012_IN4`) | **Generator DAC pin read back** (`DIAG:DAC?`): 512 samples / ~21 ms, min/max/mid-crossings | VERIFIED on device (levels and frequencies of all 4 waves matched the settings) | `power.rs:87-125`, `app.rs:2269-2279`, `scpi.rs:164-165`; commit `0e930c9` |

Nothing from the scope front end reaches an MCU ADC pin: the scope ADC is the
external 10-bit **3PA1030** whose parallel data bus is PB0..PB9 (digital inputs,
`scope.rs:4,257-259`; `scope_gen_v2_6.h:11-21`), PC4 is the scope-rail enable
output, and the stock writes no other ADC/analog pin (all GPIO writes of the
stock were enumerated, `scope_gen_ui_v2_6.h:653-654`). VERIFIED for firmware
use; the absence of an untraced board wire is INFERRED.

## 3. Analog switches, muxes, relays; connectors

Scope front end (all VERIFIED pin use; meaning INFERRED from gain steps,
`scope_gen_v2_6.h:60-88`, `scope.rs:137-148,427-431`):

- PC13 = input attenuator **relay** (1 = direct, 0 = /50), 200 ms settle.
- PB15 / PD12 / PD13 = **HCF4051** select code -> amplifier gain x0.5..x20
  (stock table `[11][4]`; code `100` unused, `000` = stock row 10: 20 V/div x0.5 /
  channel off, not used by fw-rs). The 4051 sits in
  the gain/feedback path, not an input selector; no row routes anything but the MCX
  signal (INFERRED).
- PE3 = coupling (1 = DC, 0 = AC). PC4 = scope/generator analog rail; PE2 = DMM
  rail (`scope.rs:10-12,308-340`; `scope_gen_ui_v2_6.h:640-660`).
- **No GPIO connects the generator output to the scope input** (stock GPIO list,
  `scope_gen_ui_v2_6.h:653`). VERIFIED for GPIO; board traces INFERRED.

Generator output stage: only PB12 (DAC/PWM select, above). No attenuator or
relay: amplitude is set in the DAC codes (`gen.rs:211-230`).

DMM side (VERIFIED, `dmm.rs:343-372`, `hy3131_v2_6.h:14-18`): PA9/PA10 latching
relay pair (V/A vs the rest), PE0/PE1 shunt select, PC7 = 1 only in the frequency
mode. Everything else is inside the **HY3131**, driven over bit-banged SPI
(SCK PC10, MOSI PC12, MISO PC11, CS PD2). Note the naming collision: the
"PA0..PA9 / PB0..PB8" in `hy3131_v2_6.h:70-115` are the HY3131's own analog
switch pins, not MCU GPIO.

HY3131 internal routing the stock already uses (register rows VERIFIED, node
meaning from the datasheet field names, `hy3131_v2_6.h:56-68`):

- AD1 input mux (R24 SAD1FP/SAD1FN, R33 SAD1I): shorted inputs **PB3-PB3** and
  **PB5-PB5** (zero reading, rows `0804a08c`, `0804a076`, `hy3131_v2_6.h:89-90`),
  **VDDA-VSSA** (row `0804a060`, `:88`), internal temperature sensor **TS1N/TS2P**
  (mode 15, `:142,185-187`).
- AD1 reference (R28 SAD1RH/RL): external **PB6** (= the on-board MAX6190 1.25 V,
  INFERRED from the BOM) in most modes, internal **REFO** in others, **VDDA** in
  one ohm row (`:88-90,105-106,155`).
- These give zero / reference-ratio / supply / temperature self-checks of the
  meter chain **without the MCU DAC**. There is no HY3131 pin that could accept
  the generator signal: the meter is on an isolated domain (CA-IS3741 digital
  isolator + VPS8504B supplies, `DM40C_feature_request.md:38-39,55-70`), so only
  SPI crosses the barrier. Isolation INFERRED (marked "[Вывод]" there), consistent
  with the repo's ground-check instruction (`:85`).

Connectors (`DM40C_feature_request.md:46,55-70`): banana V / COM / A = DMM only
(isolated domain); MCX = scope input and generator output. The repo does not say
whether these are one or two physical MCX jacks (the diagram draws two); UNKNOWN.
No jack is shared between the meter and the scope/generator.

## 4. MCU-internal paths

- GD32F4 has no internal DAC -> ADC channel (internal ADC channels are temperature,
  Vrefint, Vbat only; INFERRED from the datasheet, not from the repo); the path is
  the **shared pin**: PA4 =
  `DAC_OUT0` / `ADC012_IN4`, PA5 = `DAC_OUT1` / `ADC012_IN5` (same pin table as
  STM32F4). PA4 -> ADC0 ch4 is VERIFIED on device (commit `0e930c9`); PA5 -> ch5 is
  INFERRED (same pattern, never exercised).
- The pin read sees the DAC + DMA + TIM6 chain only, **not** the external buffer,
  SGM3157 switch or MCX jack (INFERRED stage, section 1).
- Scope ADC = external 3PA1030, fed only by the analog front end (section 2):
  an internal DAC -> scope-ADC path is **impossible**.
- Trigger comparator: DAC2 (PA5) vs the front-end signal -> PA15 / TIM2_CH1 input
  capture, polarity inverted, IC filter 3 (`scope_gen_v2_6.h:50-59`,
  `scope_gen_ui_v2_6.h:126-129`). The comparator sees the signal **after** the
  offset stage: the stock maps the DAC level from an ADC-code-domain level
  (`scope_gen_ui_v2_6.h:116-122`), which only works if offset and gain are already
  applied (INFERRED).

## 5. What is actually possible

Wire-free, already implemented (VERIFIED on device):

- `DIAG:DAC?`: generator DAC waveform read back on PA4 by ADC0 ch4 (levels,
  frequency via mid crossings). `DIAG:PWM?`: PB14 edge count + PB12 level
  (`app.rs:2269-2300`).

Wire-free, stock procedure (VERIFIED code path, input condition INFERRED):

- **Offset self-cal** FUN_08022ed8 (menu FUN_0802307a): DC coupling, tb 12
  (100 us/div), for r = 9..0 set CCR3 = 8800 then 11636, mean of a 200 ms-settled
  flat record (max-min < 0x33) -> offset pairs; accepted in 180<o1<380, 630<o0<830
  (`scope_gen_ui_v2_6.h:96-100`). It never touches the generator (no FUN_080166a4
  call in `app_decompile.c` FUN_08022ed8). Needs 0 V at the MCX = open input
  (input termination INFERRED).

Wire-free, new code, INFERRED (not done by the stock):

- **Comparator / trigger-DAC check**: with an open input, sweep DAC2 and record
  where the static PA15 level flips -> the DAC code of "0 V + offset"; move CCR3
  (8800 / 11636) for a
  second point -> a two-point trigger map like record+0x148 without any signal.
  Requires enabling DAC ch2, PA5 analog, PA15 AF1 (default JTDI) and TIM2 or EXTI;
  relies on the comparator being after the offset stage (section 4).
- **PA5 read back**: ADC0 ch5 can confirm DAC2 is alive (pin-level only).
- **HY3131 self-checks**: zero (PB3-PB3 / PB5-PB5), REFO vs MAX6190 ratio, VDDA,
  TS1N/TS2P, using the stock rows as templates (section 3). No DAC involved.

Needs an external cable:

- GEN -> scope: MCX-to-MCX cable (the only full-chain test of amplifier, SGM3157,
  jack and front end). Needs PC4 raised: the generator output stage is on the
  scope/generator rail (`scope.rs:10-11`), whereas `DIAG:DAC?` reads the MCU pin
  and works with PC4 off.
- GEN -> meter: MCX-to-banana cable, crossing the isolation barrier (fine for a
  cable; 0..3 V DC/AC checks of V and mV ranges).
- **Stock trigger self-cal** FUN_080230a2 (`app_decompile.c:32019-32095`): range 6
  (1 V/div, ~108 counts/V, code 512 = 0 V), pos 0, DC coupling, tb 12, trigger row
  0x8c; per edge it sets DAC2 = 2401 / 1780 and takes the mean of a *triggered
  but flat* record (FUN_0801165a: trigger within ~500 ms, max-min < 0x33,
  `app_decompile.c:12833-12858`). The defaults t0/t1 = codes 300 / 780 are about
  -2.0 V / +2.5 V at the jack, so it expects a **slow bipolar signal** spanning both
  thresholds (a ramp of a few V/s; 1 ms screen stays flat). It does not switch the
  generator on, and the DM40's own 0..3 V unipolar output cannot reach the -2 V
  threshold, so this is a fixture/external-source calibration (INFERRED from the
  numbers).

Impossible on this hardware (VERIFIED chain, INFERRED absence of hidden traces):

- MCU ADC reading the scope signal, or any HY3131 node.
- Generator signal into the scope ADC or the HY3131 without a cable.
