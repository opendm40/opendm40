# Mockup: four channel blocks in the scope strip (not for merge)

Owner request 2026-11-02. Renders: `*-a-current.png` (today's page) and
`*-b-new.png` (the mockup), side by side in
[strip4-contact-sheet.png](strip4-contact-sheet.png). Made by
`firmware/tools/ui-render/run.sh OUT --strip4`
([strip4.rs](../../../tools/ui-render/strip4.rs)): 18 cases, 0 layout
violations (replica rule on the whole page, the full rules on the new strip
against everything on the screen).

How the mockup is drawn: the page is the firmware's own `scope_ui` draw; the
narrower graph is then simulated on the host (3 of every 40 columns
dropped, never a grid or tick column; markers, tabs and FFT boxes moved
whole). The blocks and the measurement cells are drawn with the firmware's
primitives (stockfont `paint`, `Ink`, `in_rbox`, the stock fonts and
colours).

## Geometry

| | today | mockup |
|---|---|---|
| graph frame | x 13..413, 10 x 40 px | x 13..383, 10 x 37 px |
| strip | x 414..479, 66 px, buttons 60 px | x 384..479, 96 px (+45 %), blocks 89 px (+48 %) |
| block | - | title 89 x 26, then + and − 43 x 40 each, 3 px apart; blocks 74 px apart from y 24 |
| measurement cells | 4 x 98 px | 4 x 90 px |

Why 37 px/div: the division width must stay an integer and the strip must
grow 30–50 %. 38 px gives a 86 px strip (+30 %) and + / − buttons of 39 px
(under 40). 37 px gives 96 px and 43 x 40 buttons. 36 px would be +61 %.
The 5 minor ticks per division (every 8 px today) become 7–8 px apart
(37 / 5 is not an integer).

The title uses the 16 px font. The widest titles ("MATH 500mV",
"FFT 100mdB") are 80 px wide in an 89 px box.

## What moves where

- Magnifier + / CHA / magnifier −: these become the CH1 block (title "CH1 1V").
  Tapping the title opens the CH1 popup, as CHA does today. The V/div badge
  in the top bar still opens it too.
- Grey + / operator+scale / − (math): this becomes the MATH block. The title
  is "MATH 2V", or "FFT 20dB" in FFT mode. The operator (+, −, ×, ÷, ABS)
  only shows in the Math popup now. Tapping the title works like the math
  label today: it turns math on and opens the Math popup. Off: dimmed.
- DMM block (new): its + / − button does the MF > Display "CH2 V/div − / +" step
  (`ch2_step`, exists). Suggested title tap: on / off (today only in MF >
  Display).
- GEN block (new): the GEN trace is drawn on CH1's scale today
  (`sp_gen_frame`, `row_of_volts`). The mockup title shows CH1's V/div. For
  its + / − to work, GEN needs a V/div of its own (new state plus a persisted
  setting). Suggested title tap: on / off (today: MF > Display "GEN trace").
- TRIG button: removed. The Trigger tab is still reachable by tapping the
  trigger badge in the top bar (`top_badge`, the `Some(_)` arm) and through
  MF > Trigger.
- An off channel: its colour at 30 % over the strip, with grey text. CH1
  has no off state.
- Wave view (DMM source): today the V/div buttons step the wave range and
  CHA shows the coupling note (`sp_panel`, k <= 2). The CH1 block must keep
  that branch.
- The graph's "CH2 2V" label repeats the DMM block. It could go.

What is lost: the TRIG shortcut (it now takes one tap on the badge), the
operator on the math button, about 7.5 % of the graph width, and the
pixel-exact stock layout of the scope page. The
stock-diff replica check (`tools/ui-render/stock-diff.sh`) would no longer
match on the scope scenes.

## The graph width and the time axis

Time per division and the timebase list do not change: there are still 10
divisions. Each division drops from 40 to 37 px. The engine's `scope::COLS`
(400) sets the record length per screen (`scope.rs:273`). One option is
COLS = 370, which gives 37 points per division instead of 40, 7.5 % fewer
samples per screen and an unchanged sample rate. The other is to keep 400
and map 400 points onto 369 columns (min/max per column). A cursor step
becomes tb / 37 per pixel. The trigger position centre moves from x 213 to
x 198. The left-strip markers are unchanged. The trigger-level marker sits
at x 371..382, inside the frame and clear of the strip (checked).

## Size and effort (estimate)

- Graph width: about 130 hard-coded geometry values (87 in scope_ui.rs,
  28 in scope_page.rs, 13 in scope.rs), the 399/400-column arrays and the
  grid's `col_classes` (40 → 37, a tick table). Code size stays about the
  same. SRAM: about 30 columns × 5 trace arrays × 4 B, roughly 0.6 KiB less.
- Strip: the 7-arm `panel_button` and `panel_hit` become 4 blocks with 3
  rects each. `sp_panel` gets 12 targets. About +0.5–1 KiB.
- GEN V/div: about +0.5 KiB. DMM on/off from the title: about +0.2 KiB.
- Total: about +1.5–2.5 KiB of flash. The panel code is hot (region B, link-native.x
  keeps `panel_button` / `panel_hit` there). This fits region B (~20 KiB
  free). Region C (~7.7 KiB free, floor 4 KiB) is not needed.
- Effort: about 1.5–2 days, including the ui-render matrix, the regenerated manual
  scope screenshots, the emulator, and a device check.
