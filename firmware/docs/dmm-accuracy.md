# DMM accuracy label

The DMM page shows the specified uncertainty of the reading, ±(x % of the
reading + N d), as an absolute value in the reading's unit, small and grey
under the unit (owner request 2026-10-26). Code:
[`src/accuracy.rs`](../src/accuracy.rs) (table and arithmetic),
[`src/ui.rs`](../src/ui.rs) `acc_ulps` / `DmmView::acc_update` /
`strip_acc`; host test [`tools/accuracy-test`](../tools/accuracy-test/main.rs).

## 1. The manual's table

Source: *DM40 Multimeter User Manual V1.5* (Alientek), "Multimeter Technical
Parameters", PDF page 4 (printed page 2). The accuracy column is the same for
the three models; only the range names differ.

| Function | DM40A | DM40B | DM40C | Accuracy |
|---|---|---|---|---|
| DC voltage | 400mV / 4V / 40V / 400V / 1000V | 500mV / 5V / 50V / 500V / 1000V | 600mV / 6V / 60V / 600V / 1000V | ±(0.03%+5d) |
| AC voltage | 400mV / 4V / 40V / 400V / 750V | 500mV / 5V / 50V / 500V / 750V | 600mV / 6V / 60V / 600V / 750V | ±(0.3%+15d) |
| DC current | 400uA / 4000uA / 40mA / 400mA | 500uA / 5000uA / 50mA / 500mA | 600uA / 6000uA / 60mA / 600mA | ±(0.15%+5d) |
| DC current | 4A / 10A | 5A / 10A | 6A / 10A | ±(0.2%+5d) |
| AC current | 400uA / 4000uA / 40mA / 400mA | 500uA / 5000uA / 50mA / 500mA | 600uA / 6000uA / 60mA / 600mA | ±(0.5%+15d) |
| AC current | 4A / 10A | 5A / 10A | 6A / 10A | ±(0.75%+15d) |
| Resistance | 400Ω / 4KΩ / 40KΩ / 400KΩ / 4MΩ | 500Ω / 5KΩ / 50KΩ / 500KΩ / 5MΩ | 600Ω / 6KΩ / 60KΩ / 600KΩ / 6MΩ | ±(0.1%+5d) |
| Resistance | 40MΩ | 50MΩ | 60MΩ | ±(0.2%+10d) |
| Capacitance | 4nF / 40nF / 400nF / 4uF / 40uF | 5nF / 50nF / 500nF / 5uF / 50uF | 6nF / 60nF / 600nF / 6uF / 60uF | ±(2.5%+30d) |
| Capacitance | 400uF / 4mF / 40mF | 500uF / 5mF / 50mF | 600uF / 6mF / 60mF | ±(3.5%+30d) |
| Frequency | 10Hz~40MHz | 10Hz~50MHz | 10Hz~60MHz | ±(0.01%+5d) |
| Temperature | -40℃~1000℃ (standard probe range -20℃~300℃) | | | ±(1.0%+5℃) |
| Temperature | -40℉~1832℉ | | | ±(1.5%+5℉) |
| Continuity | 0Ω~1KΩ | | | ±(0.2%+5d) |
| Diode | 0V~3.0V, 0Ω~1KΩ | | | ±(1%+5d) |
| AC response | 10KHz | | | |

The manual gives no conditions (temperature, time since calibration,
frequency band of the AC rows, input range of the frequency row): unknown.

## 2. What "d" is on opendm40

opendm40 uses the DM40C counts (60000) on every model
([model-variants.md](model-variants.md) section 4). The three stock
formats have the same decimals per decade (400.00 / 500.00 / 600.00 mV), so
on its own range one d is the last digit of the 60000-count readout,
`Val::dec`, on every model.

A DM40A / B reading above its stock range (a DM40A at 4.8 V: 48 000 counts
on opendm40's 6 V range, above the stock 4 V range) is one the stock would
show on the next range up, with one decimal less. There one d is 10 x the
displayed digit (4.8176 V on a DM40A: ±0.0065 V, against ±0.0020 V on a
DM40C). The step is taken where the stock steps up: above factor x
overrange x decade (DM40A 42 000, DM40B 52 000, DM40C 61 800 counts;
capacitance 4 200 / 5 200 / 6 180 of its 1000-count decade). INFERRED: the
manual does not say which range such a reading belongs to, nor that the
stock capacitance ranging steps at the same factors; the stock's down-step
hysteresis is not modelled.

The extra (+1) digit of the averaged DC readings is below the specified
resolution: it does not change d.

## 3. The label

- **Value:** (x % x |reading| + N d), rounded up to the readout's last
  digit and written with its decimals and unit: "±0.0020V" under 4.8176 V.
  Temperature: x % of the reading in its unit (°C or °F) + 5 °C / 5 °F. The
  spec form "±(0.03%+5d)" is not shown: the absolute value says what the
  digits are worth without arithmetic.
- **Where:** the DMM page, right end of the bargraph's label row (x 415..475,
  y 161..172, F14, the grid grey of the scale labels), under the unit; the
  "±" is drawn (no stock font has U+00B1). The combined view: a free sub
  row of the meter strip (the lower one first), grey. AC V / A there have
  both sub rows taken (Hz, duty): no label in the strip.
- **None:** with the History row instead of the bargraph (Settings 2/3 "DMM
  row"), in the power meter, for OL, dashes and provisional (grey) values,
  under REL (the manual specifies a reading, not the difference of two), and
  where the manual has no row: AC+DC, the 60 mV ranges, temperature with
  PT100 / PT1000 / NTC sensors (the manual's row is for the K thermocouple
  probe). The AUTO coupling's shown part takes the row of its DC or AC
  function.
- **Setting:** Settings 3/3 "Accuracy" ON (default) / OFF, persisted (p[38]
  bit 18 = OFF, so every older record reads ON).
- **Model:** `Dmm::model()`, the calibration record's byte.

## 4. Memory

The label's code is in region A2 (zero-wait, `.text.a2.acc`); the DMM
readout runs while an AD2 stream does. To keep region B above its 16 KiB
warning, `ui::help_page` went back to `.cold` (Help pages only: no stream
runs there; [native-boot.md, Region C audit #2](native-boot.md#region-c-audit-2-2026-10-25)).
With the label, against its parent (e9bdb80d): A2 free 2 948 -> 1 844 B
(below the 2 KiB warning), B 16 500 -> 16 484 B, C 7 780 -> 7 152 B; static
stack worst path unchanged (headroom 20 540 -> 20 524 B).
