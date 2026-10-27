# DMM accuracy label

The DMM page shows the specified uncertainty of the reading, ±(x % of the
reading + N d), as an absolute value in the reading's unit, small and grey
under the unit (owner request 2026-10-26). Code:
[`src/accuracy.rs`](../src/accuracy.rs) (table and arithmetic),
[`src/ui.rs`](../src/ui.rs) `acc_ulps` / `DmmView::acc_update` /
`strip_acc`; host test [`tools/accuracy-test`](../tools/accuracy-test/main.rs).
The same uncertainty greys the reading's digits that are below it
(section 5).

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
  digit, then to one significant digit (two when the first is 1), half
  up, and written without trailing zeros, with the unit: "±0.002V" under
  4.8176 V, "±0.0015V", "±0.2V" for 0.201 V.
  Temperature: x % of the reading in its unit (°C or °F) + 5 °C / 5 °F. The
  spec form "±(0.03%+5d)" is not shown: the absolute value says what the
  digits are worth without arithmetic.
- **Where:** the DMM page, right end of the bargraph's label row (x 415..476,
  y 161..172, F14, the grid grey of the scale labels), under the unit; the
  "±" is drawn (no stock font has U+00B1). Too wide with the unit: the
  number alone ("±0.0060" under MHz). The combined view: a free sub row of
  the meter strip (the lower one first), grey; none when it does not fit
  beside the GEN column. AC V / A there have both sub rows taken (Hz,
  duty): no label in the strip.
- **Rounding:** the significant-digit step rounds to nearest, so the
  label is not a strict bound (0.201 V reads ±0.2 V); the grey digits
  (section 5) follow the rounded value.
- **None:** with the History row instead of the bargraph (Settings 2/3 "DMM
  row"), in the power meter, for OL, dashes and provisional (grey) values,
  under REL (the manual specifies a reading, not the difference of two), and
  where the manual has no row: AC+DC, the 60 mV ranges, 60 Ω, temperature with
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

With the grey digits (section 5), against 2ee15299: A2 free 1 844 ->
1 720 B, B 16 780 -> 16 596 B, C 5 756 -> 5 720 B; stack headroom
unchanged, 20 492 B (the DmmView's last split is kept in the spare byte
of its 8-byte readout text, so the TCM statics do not grow).

## 5. Grey digits

The main readout's digits below the uncertainty are drawn grey, the
significant ones in the normal colour (orange, the >= 30 V red)
(owner request 2026-10-26). Code: `acc::grey_from`
([`src/accuracy.rs`](../src/accuracy.rs)), `DmmView::reading`,
`stockfont::cells_diff`, `scope_ui::st_text_grey`.

- **p:** the place of the uncertainty's most significant digit; the
  digits with a place value below p are grey. ±0.0020 V: p = 10^-3,
  4.817 orange, the 6 grey. ±1.49 µF under 47.30 µF: p = 10^0, "47"
  orange, ".30" grey.
- **The point** takes the colour of the digit after it.
- **No leading-digit exception:** where the uncertainty is larger than
  the reading, its digits are grey too (0.0009 V AC, ±0.0016 V: "0.000"
  orange, the 9 grey).
- **The extra (averaged) digit** is below p whenever the label applies
  (the uncertainty is >= 5 d): grey.
- **When:** whenever the label's value conditions hold (section 3,
  "None": Accuracy ON, not OL, dashes, provisional, REL, the power meter,
  or a function without a row), also where the label has no room (the
  History row; the combined view's strip with both sub rows taken, AC V /
  A). Accuracy OFF: unchanged.
- **Colour:** `ui::VALUE_LOW` (104, 104, 104), darker than the
  provisional grey `VALUE_PROV` (144, 152, 152, the whole reading), so
  the two stay apart. The combined view's strip greys its value (orange,
  no extra digit) the same way in the same colour.
- **Redraw:** a cell whose colour changes is repainted with the digits
  that change; the result equals a full draw (ui-render ag15 / ag16).
