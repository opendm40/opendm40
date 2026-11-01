# Bode plot (frequency response)

The generator sweeps a sine and CH1 is read at each frequency, as the
frequency response analysis of the Keysight 1000X-G scopes
([datasheet](https://www.farnell.com/datasheets/2815508.pdf)). User side:
[the manual, Bode plot](../../docs/manual/README.md#bode-plot); SCPI:
[scpi-reference.md, Bode plot](../../docs/manual/scpi-reference.md#bode-plot).

## Sweep ([app_bode.rs](../src/app_bode.rs))

| | |
|---|---|
| grid | 10 Hz .. 50 kHz, 5 / 10 / 20 points per decade, the last point 50 kHz ([bode.rs](../src/bode.rs) `grid_n`, `grid_f`) |
| generator | sine, 2 Vpp around 1.5 V, the grid frequency in whole Hz; the point is stored at `Gen::actual_hz` |
| scope | AC, Auto, trigger source GEN at 1.5 V, the fastest timebase whose screen holds 4 periods (`tb_for`, at most 50 ms/div: roll records have no generator phase) |
| settling | 3 periods, at least 50 ms; then the acquisition restarts |
| V/div | first point 0.5 V/div, then the previous point's; a record clipped (a code at 0..1 or 1022..1023) or under a fifth of the codes is taken again at the V/div `range_retry` gives, at most 4 times |
| no phase | no record with the generator's phase within 3 records + 1.5 s: the point is set up again once, then skipped (gain and phase NaN) |
| end | the user's generator and scope settings back (also on Stop and on leaving the page) |

The low end is 10 Hz because 50 ms/div is the slowest timebase without roll
(5 periods of 10 Hz in a 0.53 s record); the high end is the DAC path's sine
limit (genplan.rs `DAC_WAVE_MAX`).

## Measurement ([bode.rs](../src/bode.rs) `lockin`)

The scope's GEN channel ([scope-gen-channel.md](scope-gen-channel.md)) gives
the generator's phase at every sample of the stored record
(`Scope::gen_sync`): periods(n) = (t0 + n s) k / q. The record's codes,
minus their mean, are projected on sin / cos of 2 pi periods(n) over the
longest whole number of periods from sample 0: amplitude
2 |sum| / N and phase atan2(sum x cos, sum x sin). Phase 0 is the
generator's mid-rising crossing; a CH1 signal that lags reads negative. The
phasor is rotated in f32 per sample and re-seeded from the exact f64 phase
every 256 samples. Codes become volts with the scope's map at that V/div
(`Scope::volts`).

Host test ([tools/bode-test](../tools/bode-test/main.rs)): synthetic records
on the generator's real table timing (genplan.rs) at every point's
timebase: amplitude within 0.03 %, phase within 0.02 degrees with
noise, a DC offset and a 2nd harmonic; an RC low-pass against a Thru
(-3 dB within 0.3 % of 1 / (2 pi R C)).

## Reference

- **Thru**: a sweep with the probe on the generator's output, kept in RAM
  for its points per decade. Later sweeps on the same grid are divided by
  it point by point: the generator's output stage, the AC coupling, the
  front end and the path lag (`scope::GEN_LAG_DEFAULT`, measured on the
  PWM path only) cancel.
- **No Thru**: the gain is against the generator's set amplitude (1 V
  peak) and the phase is raw (the DAC path's own delay is *unknown*).

## Files

`SCOPE/BODEnnn.CSV` on the SPI disk, served as it is on the USB volume
(vdisk.rs: kind 1 with the numbers 1000 + n, since the 2-bit kind holds no
fifth kind; so BODE000 .. BODE023 only). Written by the screenshot job
(screenshot.rs `start_bode`, one 4 KiB block). Not in the gallery or
`MMEMory:CATalog?`.

## Memory

The points (2 x 80 x 12 B) and the CSV text (4 KiB) are in SRAM
(`.sram_bss`). Code in region C, except the view check in
`App::scope_poll` and the `BODE` header in the SCPI parser.
