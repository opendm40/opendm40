# DAC -> ADC paths for self-test

Which signals the firmware can check without an external wire, using the
MCU's DAC, its own ADC and the scope's trigger comparator. Short answer: the
DAC pins can be read back by the MCU's ADC, and the trigger comparator can be
calibrated against the scope's offset level; nothing reaches the scope ADC or
the HY3131 without a cable. The hardware itself (pins, chips, front ends) is
described in [hw/README.md](../../hw/README.md); this doc covers what the
firmware does with it.

Tags: **INFERRED** = interpretation (the board wiring is not traced; no
schematic in the repo). Sources: [`firmware/src`](../src),
`stock/re/analysis/*.h`, `stock/re/analysis/app_decompile.c`.

## 1. DAC channels

The hardware is in [hw/README.md Generator](../../hw/README.md#generator)
(DAC channel 1 on PA4, PB12 path select, PWM on PB14) and
[hw/README.md Oscilloscope](../../hw/README.md#oscilloscope) (DAC channel 2
on PA5 as the trigger level, comparator output on PA15, offset PWM on PC8).
Firmware use:

| DAC channel / pin | opendm40 | Code |
|---|---|---|
| ch1 = PA4 | generator waveform (TIM6 TRGO, DMA0 stream 5) | [`src/gen.rs`](../src/gen.rs) `Gen::dac` |
| ch2 = PA5 | scope hardware trigger level, on by default since ef43768 | [`src/hwtrig.rs`](../src/hwtrig.rs) `set_level`, `set_dac` (bits 16..31 of the DAC registers; `gen.rs` owns bits 0..15) |

No external DAC: the SPI peripherals are the W25Q128 flash and the HY3131
([hw/README.md Main parts](../../hw/README.md#main-parts)).

## 2. MCU ADC channels

Only ADC0 is used ([`src/power.rs`](../src/power.rs)):

| ADC0 channel | Source | Use |
|---|---|---|
| ch15 = PC5 | battery ([hw/README.md Power and battery](../../hw/README.md#power-and-battery)) | every 2 s |
| ch16 | MCU die temperature (internal sensor) | every 2 s, after the battery |
| ch4 = PA4 | the generator DAC pin read back | `DIAG?` / `DIAG:DAC?`: `Power::sample`, 512 samples in ~21 ms, min / max / mid-crossings |

`DIAG?` on the device: the levels and frequencies of all 4 waveforms matched
the settings (commit `2bbcaf5`).

Nothing from the scope front end reaches an MCU ADC pin: the scope ADC is the
external 3PA1030 on the PB0..PB9 bus
([hw/README.md Oscilloscope](../../hw/README.md#oscilloscope)), and the stock
writes no other analog pin.

## 3. Switches and connectors

Scope front end (attenuator relay PC13, 4051 gain code, coupling PE3), the
generator path switch and the jacks:
[hw/README.md Oscilloscope](../../hw/README.md#oscilloscope),
[hw/README.md Generator](../../hw/README.md#generator),
[hw/README.md Main parts](../../hw/README.md#main-parts). For self-test this
means:

- No GPIO connects the generator output to the scope input (the stock GPIO
  list). The 4051 sits in the gain feedback, not in front of the input
  (INFERRED from the gain steps).
- The meter is on the isolated domain; only SPI crosses
  ([hw/README.md Isolation boundary](../../hw/README.md#isolation-boundary)).
  No HY3131 pin can take the generator signal.
- No jack is shared between the meter and the scope / generator.

The HY3131 can check its own chain without the MCU DAC, using rows the stock
already has (`stock/re/analysis/hy3131_v2_6.h`, in the private analysis repo): AD1
inputs shorted (PB3-PB3, PB5-PB5) for the zero, VDDA-VSSA, the die sensor
TS1N/TS2P (the temperature mode), and the AD1 reference switched between PB6
(the MAX6190) and the internal REFO. opendm40 does not run these as a
self-test.

## 4. MCU-internal paths

- The GD32F4 has no internal DAC -> ADC channel (INFERRED from the
  datasheet: the internal channels are temperature, Vrefint, Vbat). The path
  is the shared pin: PA4 = DAC_OUT0 / ADC012_IN4, PA5 = DAC_OUT1 /
  ADC012_IN5. PA5 -> ch5 is INFERRED (same pattern, never run).
- The pin read sees the DAC + DMA + TIM6 chain only, not the external
  buffer, the SGM3157 switch or the jack.
- The trigger comparator compares DAC channel 2 with the signal **after**
  the offset and gain stages: the stock maps the DAC level from an
  ADC-code-domain level with a two-point map, so 1024 ADC codes span about
  621 DAC codes = 0.45 V (INFERRED; [`src/hwtrig.rs`](../src/hwtrig.rs)
  module header). Its output on PA15 is inverted and ~2 us late.

## 5. What the firmware can test without a wire

Implemented:

- **`DIAG?` / `DIAG:DAC?`**: the generator DAC waveform read back on PA4 by
  ADC0 ch4 (levels, frequency from the mid crossings). **`DIAG:PWM?`**: the
  PB14 edge count and the PB12 level (`App` `DiagDac` / `DiagPwm` in
  [`src/app.rs`](../src/app.rs)). Both USB only
  ([scpi-reference.md Debug commands](../../docs/manual/scpi-reference.md#debug-commands)).
- **Comparator calibration, `DBG:HWTRIG:CAL?`** (`hwtrig::calibrate`, via
  `Scope::hw_calibrate`): with nothing on the MCX, the comparator sees the
  offset level, i.e. the ADC code of 0 V at the current CCR3. For CCR3 =
  `CCR3_LO` 8800 and `CCR3_HI` 11636 the codes are the stock offset pair of
  the range (settings record +0xA8, `REC_OFFSET`). At each point DAC2 is swept
  up and down and the PA15 flip is recorded; the two sweeps give one map per
  edge (the comparator's hysteresis), like the stock pairs at record +0x148
  (`REC_TRIG`). Range 6 (1 V/div), DC coupling, scope rail on, ~0.5 s. The
  result stays in RAM. A device run of `DBG:HWTRIG:CAL?` is not recorded
  (untested). The trigger itself uses the stock map from the settings record.

Stock procedures:

- **Offset self-cal** (stock `FUN_08022ed8`): DC coupling, 100 us/div; for
  each range it sets CCR3 = 8800 then 11636 and takes the mean of a flat
  record -> the offset pairs at record +0xA8 (accepted in 180 < o1 < 380,
  630 < o0 < 830). Needs 0 V at the MCX, i.e. an open input (input
  termination INFERRED). opendm40 reads its result; it does not run it.
- **Trigger self-cal** (stock `FUN_080230a2`): range 6, DC, 100 us/div; per
  edge DAC2 = 2401 / 1780 and the mean of a triggered but flat record. The
  default codes 300 / 780 are about -2.0 V / +2.5 V at the jack, so it needs
  a slow bipolar source spanning both (INFERRED from the numbers). The
  DM40's own generator (0..3 V, unipolar) cannot reach -2 V: this is a
  fixture calibration, and `hwtrig::calibrate` replaces it without a source.

Possible, not built:

- PA5 read back on ADC0 ch5 (DAC2 alive, pin level only).
- HY3131 self-checks from the stock rows (section 3).

Needs a cable:

- GEN -> CHA (MCX to MCX): the only full-chain test of the generator stage,
  the switch, the jacks and the scope front end. Needs the scope /
  generator rail (PC4) on; `DIAG?` works with it off.
- GEN -> V/COM (MCX to banana): 0..3 V DC / AC checks of the V and mV ranges
  across the isolation barrier.

Not possible on this hardware (no hidden traces assumed, INFERRED): the MCU
ADC reading the scope signal or any HY3131 node; the generator into the
scope ADC or the HY3131 without a cable.
