# Mockup: four channel blocks in the scope strip (not for merge)

Owner requests of 2026-11-02, revision 2. The renders are in [v2/](v2/).
Contact sheets:

- [today / previous / new](v2/strip4-sheet-today-prev-new.png)
- [every variant](v2/strip4-sheet-all-variants.png)

`firmware/tools/ui-render/run.sh OUT --strip4`
([strip4.rs](../../../tools/ui-render/strip4.rs)) draws 8 states × 4
variants, 71 cases, 0 layout violations. The rules: the replica rule on the
whole page, the full rules on the strip against every item on the screen,
and a 3 px gap between each left-edge marker and every graph item. The
first mockup's renders are kept beside this note (`*-b-new.png`).

How the mockup is drawn: the page comes from the firmware's `scope_ui`
(release/0.4.0 with #243 and #244). The narrower graph is simulated on the
host: columns are dropped (never a grid or tick column), and markers, tabs
and FFT boxes are moved whole. The blocks use the firmware's primitives and
the stock magnifier shape (`scope_ui::magnifier`, UF-10).

## Variants

| variant | graph frame | division | strip | title | magnifier buttons |
|---|---|---|---|---|---|
| today | x 13..413 | 40 px | 66 px | - | 60 × 40 |
| c: 38 px | x 13..393 | 38 px | 86 px | 79 × 26 | **38 × 40** (under 40) |
| d: 37 px | x 13..383 | 37 px | 96 px | 89 × 26 | 43 × 40 |
| **e: left edge 0, 39 px** | x 0..390 | 39 px | 89 px | 82 × 26 | **39 × 40** (1 px under 40) |
| f: left edge 0, 38 px | x 0..380 | 38 px | 99 px | 92 × 26 | 44 × 40 |

Left edge 0 with 40 px divisions would leave a 79 px strip and 34 px
buttons.

**Recommended: e.** With the left marker column gone, the graph keeps 39
of today's 40 px per division (390 of 400 px). That is wider than either
38 or 37 with the column kept. The strip grows 35 % and the buttons are 39
px, 1 px under the target. If 40 px buttons are a must, f gives 44 px with
38 px divisions. Of the two variants that keep the column, c (38 px) gives
a wider graph but 38 px buttons, and d gives 43 px buttons but a 370 px
graph.

## Block

- The title shows the name and the scale in the 16 px font. When that does
  not fit, the scale is in the 12 px font. When that still does not fit,
  mdB values switch to the shorter unit ("FFT 100mdB" becomes "FFT 0.1dB").
  State 8 renders the widest titles in every variant.
- Below the title: the stock magnifier + (zoom in = smaller V/div) on the
  left and the magnifier − (zoom out) on the right. This applies to all
  four blocks.
- An off channel is drawn in its colour at 30 %, with grey ink.
- **MATH:** the firmware has two operations, FFT (mode 5) and ABS (mode 6,
  |CH1|). The operator table also holds "+", "-", "*" and "/", but no menu
  reaches them. With only one analog channel there is no A+B. Titles:
  "ABS 1V", "FFT 20dB", "FFT 0.1dB".
- **+ = zoom in everywhere.** Today, math "+" and DMM "+" make the V/div
  larger (`sp_panel` k 3, `ch2_step(up)`). Both flip to match CH1.

## GEN V/div (proposal)

- **New state:** `gen_vdiv`, an index into the scope's V/div list.
  `sp_gen_frame` maps volts with its own `k = 141 / (4 × V/div)` instead of
  CH1's `row_of_volts`, as `ch2_rows` already does for the DMM. GEN keeps 0
  V on the centre row and has no offset marker, so it has no separate
  offset either.
- **Default:** CH1's V/div when the GEN trace is first turned on (or after
  a reset), so the trace looks as it does today. After that it is
  independent.
- **Stored:** yes, with the other scope settings (the persist word next to
  the `gen_trace` bit, scope_page.rs ~2927/2955). It needs one byte.
- **SCPI (proposal):** `GENerator:SCALe <V/div>[?]` and
  `GENerator:DISPlay ON|OFF[?]`. GENerator is already the trigger source's
  keyword. `SOURce` stays for the generator output.

## The left marker column moves into the graph (variants e, f)

- The frame starts at x 0. The level markers (A for CH1, M for math, the Y
  cursors 1 and 2, the runt T) stay at x 1..12, now inside the graph and
  pointing right. This is the same style as the trigger-level T inside the
  right edge. They cover 12 px of the trace at the very left, as the T does
  at the right.
- The bottom-left timebase tab moves from x 23 to x 16, which leaves 3 px
  to the markers. State 7 renders CH1 at the bottom next to the tab, with
  the Y cursors and the runt level (checked).
- **Drag zones (#244):**
  - `grab_rect(EDGE_LEFT)` keeps its absolute rect: x 0 to the glyph + 30
    px, 48 px tall. It now lies inside the graph, so a press there within
    x 0..42 next to a marker grabs the marker. Today that is the strip plus
    29 px of graph.
  - `EDGE_RIGHT` ends at the new right frame instead of x 414.
  - `ZONE_L` (Corners) starts at x 16 instead of 23. `INNER` already
    starts at x 0.
  - The touch tests that use the left zones' x need the new numbers.

## What moves where (unchanged from the first mockup)

- **CH1 block:** replaces the magnifiers and the CHA tile. Tapping the
  title opens the CH1 popup.
- **MATH block:** replaces the grey group. Tapping the title turns math on
  and opens the Math popup.
- **DMM block:** + / − step the CH2 V/div. Tapping the title switches DMM
  on or off.
- **GEN block:** + / − step the new GEN V/div. Tapping the title switches
  the GEN trace on or off.
- **TRIG:** removed. The trigger settings stay one tap away on the trigger
  badge in the top bar, and in MF > Trigger.
- **Wave view:** the CH1 block keeps today's branch (`sp_panel`, k <= 2).
- **Lost:** the TRIG shortcut, 2.5 % of the graph width (e), the stock
  pixel-exact scope layout, and the stock-diff replica scenes of the scope
  page.

## The time axis

There are still 10 divisions, and the timebase list does not change. With
`scope::COLS` set to 10 × division (390 for e), each division gets 39
samples and a screen 2.5 % fewer, at the same rate. A cursor step is tb /
39.

## Implementation plan (after approval, branch from fresh release/0.4.0)

- **One graph rectangle.** `scope_ui::GRAPH`: frame x0, division width and
  divisions. Everything else becomes `const` derived from it: the frame,
  the right edge (`RIGHT_X`), `COLS`, the column arrays, `col_classes`
  (division and the 5 ticks from a table), `PANEL`, `MEAS_BAR` and its
  cells, `TAB_L/R`, `ZONE_L/R`, `CUR_TAB`, the FFT boxes, `grab_rect`'s
  edges and `INNER`. That covers about 130 sites in scope_ui.rs,
  scope_page.rs and scope.rs, plus app_wave, scope_ch2 and app_ch2 where
  they use 399 / 400.
- **First step:** introduce the rectangle with today's numbers. The output
  must be byte-identical, checked with UIR_HASH and the stock-diff check.
  The second step changes the numbers.
- **Strip:** `B_RECT` and `panel_button` become the blocks. `panel_hit`
  returns (channel, title / + / −). `sp_panel` dispatches by channel.
- **Size (estimate):** about +2–3 KiB of flash in region B (~20 KiB free).
  Region C is not needed. SRAM is about 0.2 KiB less (shorter column
  arrays).
- **Effort:** about 2 days, including the touch tests (#244 zones), the
  ui-render matrix, the manual screenshots, and an emulator and meter check.
