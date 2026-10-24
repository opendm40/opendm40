# DMM+Scope combined view: redesign, scope gesture map, rendering, UI review

It answers the owner's
feedback on the combined view: "fonts of different sizes, everything overlaps;
maybe embed the DMM data into the oscilloscope interface in this mode?" It also
covers two follow-up questions from the owner: the scope gesture map (pinch,
drag, trigger, one thing per drag) and "should we use a graphics library?".

It amends `../docs/history/scope-ui-plan.md` (§2.5, §2.6, §3.1, §3.2, step S8, see §5)
and references `docs/ui-review.md` instead of repeating it.

Mockups are in `docs/mockups/`, drawn at 480x320 with the real stock font data
(`src/stockfont_data.rs`). The renderer in `docs/mockups/tools/` (`sim.py` =
host copy of the `stockfont.rs` drawing code, `render.py`/`mock.py`/`final.py`)
produces them:
`/usr/bin/python3 docs/mockups/tools/final.py docs/mockups [<screenshot dir>]`.
The screenshot dir is only needed to refresh the `cur-*` images; without it
the committed ones are kept.

---

## 0. Recommendation in one paragraph

Make the combined view **a scope page with a meter strip** (Option A, §2):
the scope's own 20 px top bar, then one 56 px DMM strip across the full width,
then the scope graph at 401x225 (28 px per division, 8 divisions), the scope's
5-button right panel and the scope's measurement bar. The DMM value is drawn
in a new **F39** font (the stock F78 digits halved by a 2x2 box filter), so
the main reading is the same typeface as on the DMM page at half size, and it
is still the biggest number on the screen. Units and sub-readings (AC/DC parts,
Hz, duty, cold junction) use LETTERS, as on the DMM page. Status chips use F14,
as in the scope. That gives three sizes in one hierarchy instead of today's
six unrelated ones. Tapping the strip opens the DMM function row as a band
over the bottom of the graph. The side keys keep their DMM meaning, as
scope-ui-plan §2.5 already decided.

| Current (db679a3 / HEAD) | Proposed (Option A) |
|---|---|
| ![](mockups/cur-combined-pwm-acv.png) | ![](mockups/a1-dcv.png) |
| ![](mockups/cur-combined-acdc-head.png) | ![](mockups/a2-acdc-hold-rel.png) |

---

## 1. What is wrong with the current combined view

Seen in the shots 16/32/33, plus the HEAD AC+DC layout (commit 40d9a76,
rendered in `cur-combined-acdc-head.png`):

1. **Six text sizes in 80 px of height.**
   - F21 for the status bar (AUTO RUN 00:12:55);
   - LETTERS 22 px digits for the value;
   - UNIT 29 px caps for the unit, which is bigger than the value's digits;
   - MODE 32 px for the AC/DC glyph;
   - F14 9 px for the AC/DC parts;
   - F14 for the scope labels.

   The unit is visually heavier than the number it qualifies. The AC+DC parts
   are 9 px tall next to a 29 px unit.
2. **Two bars, two languages.** The DMM-style 22 px bar (0x1904, F21) sits
   above an F14 scope label row that floats on the DMM background without a
   bar (y 59). The scope chrome (grey 0x4208 frame, right panel) is missing.
   The graph therefore looks pasted in, with a 66 px empty band on its right.
3. **Things collide.**
   - The AC glyph sits 30 px from the unit (shot 32: "4.8176 V   ~").
   - The scope label row starts 3 px under the value.
   - In AC+DC the part lines (x 354..479) squeeze against the limit badge slot.
4. **The scope cannot be driven** (P18 in scope-ui-plan): no panel, and RUN,
   AUTO and MF go to the DMM.
5. **No REL, HV, aux or limit context** in the compact branch: aux readouts are
   dropped outside AC+DC, the lightning is suppressed, and REL has no indicator
   (ui-review §1).
6. **The graph is 199 px with a non-1:1 scale and a broken grid**
   (`templates_c` uses `r % 35`, so rows are skipped at h ≠ 281; scope-ui-plan
   §3.2 notes).

---

## 2. Option A (recommended): meter strip inside the scope page

### 2.1 Type hierarchy for every scope-family page

| Tier | Font | Size (ink) | Used for | Colour |
|---|---|---|---|---|
| T1 | **F39** (new, §2.6) | digits 39 px, adv 30 | DMM main value | `VALUE_RED` 0xF145 |
| T2 | LETTERS (stock) | caps/digits 22 px | DMM unit, DMM sub-readings, right-panel axis letters use F21 (same family) | 0xF145 |
| T3 | F14 (stock) | caps 9 px | top-bar chips, badges, measurement bar, panel text | per chip |
| (transient) | ASCII26 (stock) | caps 18 px | gesture bubbles only (pinch value, trigger level) | FG / orange |

Rules:
- No other fonts appear on the Scope or combined page.
- No MODE (32 px) or UNIT (31 px) glyphs in the strip: the AC/DC sign is the
  21 px AUX_TAG glyph set, the same glyphs the DMM page uses for its aux rows.
- F14 keeps spacing 1, the others 0.

`fonts-hierarchy.png` shows the four tiers.

### 2.2 Colour tokens used here (RGB565)

| Token | Value | Existing name |
|---|---|---|
| chrome (bars, gutters, panel) | 0x4208 | `S_BAR` |
| chip fill (new) | 0x3186 | — |
| meter tile background | 0x0883 | `BG` (DMM page background) |
| graph fill / grid / frame | 0x2124 / 0x9492 / 0xAD55 | `S_FILL` / `S_GRID` / `S_FRAME` |
| panel button | 0x2966 | `BTN` |
| text | 0xF79E / status 0xF7BE | `FG` / `STATUS_FG` |
| DMM value, unit, sub-readings | 0xF145 | `VALUE_RED` |
| trace / CH1 | 0xFFE0 | `TRACE` |
| trigger | 0xFAA0 | `S_ORANGE` |
| time / trigger mode | 0xA81F | `S_PURPLE` |
| HOLD badge | fg 0x0883 on 0xFC40 | as `hold_label` |
| A-HOLD waiting | fg 0x04FC on chip | as `hold_label` |
| REL badge | fg 0xF79E on 0x0419 | `SEL` |
| PASS / FAIL | white on 0x04A9 / 0xF800 | as `lim_badge` |
| HV badge + ring | white on 0xC9A0 | stock lightning colour |
| gesture bubble | 0x18C3 with a 0xAD55 border | new |

### 2.3 Screen geometry (480x320)

```
 x 0      13                                          413 414          479
 y0  ┌────┬──────┬──────┬────────┬──────────┬───────────┬──────────────────┐
     │    │ AUTO │ 10ms │ 5V DC  │ / 2.50V  │ ••••      │ 00:21:13 🔓 BT ▮▮▮│ 0..19 top bar
 y20 ├────┴──────┴──────┴────────┴──────────┴───────────┴──────────────────┤
     │╭────────────────────────────────────────────────────────────────────╮│
     │[AUTO] ┊                    ┊ ≅            ~  53.63mV               ││ 20..75
     │[HOLD] ┊   -600.00 (F39)    ┊ mV           ⎓ -501.67mV  (LETTERS)   ││ meter strip
     │[REL ] ┊                    ┊                                       ││
     │╰────────────────────────────────────────────────────────────────────╯│
 y76 ├────┬──────────────────────── ▼T ───────────────────────────┬─────────┤
     │    │                                                       │ [V ⊕]   │ 76..116
     │ 1▶ │      graph 401 x 225, 8 x 28 px divisions             │ [V ⊖]   │ 122..162
     │    │ - - - - - - - - - - - - - - - - - - - - - - - - - - ◀ │ [T ⊕]   │ 168..208
     │    │                                                       │ [T ⊖]   │ 214..254
     │    │                                                       │[AUTOSET]│ 260..300
 y301├────┴───────────────────────────────────────────────────────┼─────────┤
     │ Freq 40.0Hz │ Vpp 10.4V │ Duty 50.0% │ Vrms 4.82V          │G 1.00kHz│ 301..319
     └─────────────────────────────────────────────────────────────┴─────────┘
```

| Element | Rect (x, y, w, h) | Drawing |
|---|---|---|
| Top bar | (0,0,480,20) | fill 0x4208 |
| Chip RUN state | (13,2,50,16) | `rbox` r4 chip, F14 align 8: AUTO/WAIT/ARM/STOP (colours as `ScopeView::labels`) |
| Chip timebase | (65,2,58,16) | F14 FG |
| Chip V/div + coupling | (125,2,66,16) | F14 TRACE: "5V DC" / "5V AC" |
| Chip trigger | (193,2,84,16) | F14 FG: edge "/" or "\" plus the level |
| Record bar (S5) | (13,18,401,2) | 2 px band on the bar's bottom edge: visible part of the record in FG, rest 0x6B4D |
| Page dots | centres (300+9k, 10), k = 0..3, r 2 | current view FG, others 0x6B4D |
| Uptime | (334,0,52,20) | F14 FG "hh:mm:ss" |
| Lock | (388,0,24,20) | LOCK 0x20/0x21 align 8 |
| BT | (414,0,18,20) | SAVE_BT 0x21 |
| Battery | (436,2,40,16) | BATTERY 0x22..0x27 (green 0x04A9 while charging) |
| Strip | (0,20,480,56) | fill 0x4208 |
| Meter tile | (2,21,476,54) | `rbox` r6, 0x0883, outside 0x4208 |
| HV ring | (0,20,480,56) r8 | 0xC9A0 behind the tile when the ≥ 100 V ring applies (§2.4) |
| Badges | (6, 24 + 17k, 46, 15), k = 0..2 | `rbox` r3 + F14 align 8 (§2.4) |
| Main value | (56,26,174,46) | F39 `cells_diff` with `CELLS39` (§2.6); digits on rows 29..67 |
| Mode glyph | (234,25,64,21) | AUX_TAG 0x04 DC / 0x05 AC / 0x06 AC+DC, align 0, rows 23..43 |
| Unit | (234,44,64,30) | LETTERS align 0 (caps on rows 46..67, same baseline as the digits) |
| Sub-reading row k | right edge x 474, y 22 + 26k, h 26 | LETTERS align 4 (right); tag AUX_TAG at (x_text − 27, y + 2, 24, 21) |
| Graph | `ScopeView { x0: 13, y0: 76, h: 225 }` | grid rows at y0 + 28j (j = 1..7), vertical as today |
| Left gutter | (0,76,13,225) | 0x4208, offset marker "1▶" |
| Right panel | (414,76,66,225) | 0x4208, 5 buttons (417, 76 + 46k, 60, 41), `rbox` r6 0x2966 |
| Measurement bar | (0,301,414,19) | `scope_measurements` unchanged |
| Generator chip | (414,301,66,19) | F14 green 0x04A9 "G 1.00kHz" (on the full Scope page too) |

Right-panel buttons:
- Icons are drawn from primitives: an F21 letter "V" (TRACE) or "T" (S_PURPLE)
  at (419, y, 24, 41) align 8, plus a 2 px ring of radius 10 centred at
  (457, y + 20) with an 11x2 bar, and a 2x11 bar for ⊕.
- From the top: **V⊕** (smaller V/div), **V⊖**, **T⊕** (faster), **T⊖**, and
  **AUTOSET** (F14, purple; the scope auto-set).
- Coupling, trigger mode and cursors stay on the full Scope page and in the
  chip popups (S6).
- The 5 x 41 + 4 x 5 = 225 pitch gives 41 px targets. scope-ui-plan's 38 px
  draft is superseded.

Graph scale: 225 px is not 1:1 with the 280-row model.
- `row_px` already scales, by (h − 3)/278.
- `templates_c` must draw grid rows from the pitch `(h − 1) / 8` (= 28) in
  pixel space, not from `r % 35` (scope-ui-plan §3.2 note).

### 2.4 Meter strip content rules

**Main value.** It uses `fmt7` without the extra digit, as the compact branch
does today. OL is "0L" (F39 has `0`, `L`, `O`). It is redrawn digit by digit
with `cells_diff`.

**Unit and sign.**
- The unit text comes from `unit_text` (k → K).
- The mode glyph sits above the unit, as on the DMM page:
  - AC+DC: 0x06; AC: 0x05; DC: 0x04;
  - continuity and diode: none (their function shows in the unit "Ω" / "V"
    and in the function row).
- The pair is redrawn only when `(unit, mode, acdc)` changes.

**Sub-readings** (two rows, right-aligned to x 474):

| Function | Row 0 | Row 1 |
|---|---|---|
| AC+DC (V, A) | AC tag 0x05 + AC part (aux1) | DC tag 0x04 + DC part (aux2) |
| AC V / AC A | aux1 (Hz) | aux2 (duty %) |
| Hz | — | duty % |
| Temperature (TC) | aux1 (CJ ℃) | aux2 (℉) |
| DC mA | — | "4-20mA %" when it applies (UF-26) |
| others | — | — |

Width guarantee:
- The worst AC+DC row is "-501.67mV": 160 px of text plus the 27 px tag =
  187 px. It starts at x ≥ 287, while the widest AC+DC unit ("mV"/"mA",
  48 px) ends at x 282.
- Wide units ("KHz" 55, "MHz" 62) only occur in Hz, where the sub-reading is
  ≤ 83 px wide.
- Sub-reading rects are therefore sized to their text. Each row keeps its last
  left edge and clears `[min(old_x, new_x), new_x)` with 0x0883 when the text
  narrows (a label rect starting at 290 would overpaint "MHz").

**Badges.** Three fixed slots at the left, F14 in 46x15 pills. A slot is
filled with the tile colour when empty.

| Slot | Shows (first match) |
|---|---|
| 0 (y 24) | **HV** (white on 0xC9A0) when a V function reads ≥ 30 V (the stock red-value threshold, ui-review #13). Else the range mode: AUTO / AUTO+ / MANU (FG on chip). In temperature: the sensor short name TC-K / PT100 / PT1000 / NTC10k (both 10k B-values; the B-value stays on the sensor page) / N100k / NTC-U. |
| 1 (y 41) | **HOLD** (0x0883 on 0xFC40); **A-HOLD** waiting (0x04FC on chip) or captured (0x0883 on 0xFC40); empty while running |
| 2 (y 58) | Limit verdict **PASS** / **FAIL** when limits are on; else **REL** (FG on 0x0419) when REL is active. If both apply, it alternates every 1 s (on the existing status timer) |

Badge text rule: at most 41 px of F14 with spacing 1 inside the 46 px pill. `glyphs` does not clip, so any new badge string must be measured. Widths: A-HOLD 41, PT1000 40, NTC10k 41, AUTO+ 34, N100k 34; "NTC100k" (48) and "NTCusr" (42) do not fit, hence the short names above.

At ≥ 100 V (the stock lightning threshold) the tile also gets a 2 px 0xC9A0
ring. The 59 px LIGHTNING glyph does not fit the strip and is not used here
(`a4-acv-hv.png`).

States in the mockups:
- `a1-dcv.png`: DC V;
- `a2-acdc-hold-rel.png`: AC+DC, worst-case widths, HOLD + REL;
- `a3-hz-duty.png`: Hz with duty, AUTO+ / A-HOLD;
- `a4-acv-hv.png`: 230 V AC, HV badge and ring;
- `a5-temp.png`: thermocouple, OL, CJ / ℉.

### 2.5 Touch and keys in the combined view

| Where | Gesture | Action |
|---|---|---|
| Meter strip, value/unit/sub area (x ≥ 54) | tap | open the **function row** (below) |
| Meter strip, badge column (x 2..53) | tap | REL on/off. This is the same action as the DMM page REL box (`app.rs` `tap`, REL branch), including the range hold. |
| Meter strip | long-press (700 ms) | go to the full DMM view |
| Meter strip / top bar / measurement bar / right panel | 1-finger swipe left/right | previous / next view (view swipes start only outside the graph, §6.4) |
| Meter strip | 1-finger swipe down | CSV record start/stop (ui-review #6, when it lands) |
| Top-bar chips | tap | as on the Scope page: RUN chip = run/stop now; other chips open their popups once S6 lands (no-op before) |
| Graph | 1-finger drag / 2-finger pinch / double-tap / long-press | exactly the Scope page map (§6), using the combined `ScopeView` geometry |
| Right panel | tap / long-press | V⊕ V⊖ T⊕ T⊖ step; AUTOSET = scope auto-set; long-press V/T = list pickers (S6) |
| Measurement cell | tap | item picker (S7) |

**Function row** (`a6-function-picker.png`):
- **Layout.** A band (0,274,480,46) in 0x4208 with a 1 px 0xAD55 top line.
  It holds the DMM page's own six buttons: `dmm_button(i, ..)` at
  (2 + 80i, 278, 76, 40), reused unchanged except for the y and the outside
  colour.
- **Taps.** A tap behaves as on the DMM page, including cycling the sub-mode
  on a second tap of the current button.
- **Closing.** It closes 4 s after the last tap, on a tap outside it, or on
  MENU.
- **Redraw on close.** The band covers more than the graph:
  - graph rows 274..300: re-render the columns (cheap);
  - the measurement bar;
  - the left gutter (0,274,13,27), refilled with 0x4208 plus the offset marker
    if it sits there;
  - the AUTOSET button (417,260,60,41) and the panel gap around it;
  - the GEN chip (414,301,66,19). While it is open, the scope keeps
  updating the uncovered graph rows.
- **No S6 dependency.** This is a plain overlay flag, so it does not need the
  S6 popup framework. If S8 lands before it, the interim is: strip tap = go to
  the full DMM view.

**Side keys.** Unchanged, as scope-ui-plan §2.5 decided:

| Key | Action |
|---|---|
| MENU | menu; closes the function row if open |
| RUN short | DMM HOLD |
| RUN long | AutoHold |
| AUTO | DMM autorange (clears REL/HOLD as today) |
| MF | Range or Limits page |

The scope has its own on-screen run/stop (RUN chip) and auto-set (AUTOSET
button). Rationale: the strip is the primary readout here, and a hidden focus
mode would bring back "what does this key do now".

### 2.6 F39: the one new font

F39 is derived from F78, the stock DMM digits, so the combined view uses the
DMM page's typeface at half size. It is not a separate extraction: add a
derivation step at the end of `tools/mkstockfont.py` that emits `F39_FD` into
`stockfont_data.rs`. The `Font` key is a synthetic address, e.g.
`0x0806_5071`, matched in `font_data()`.

F39 does not exist in the stock flash, so its data must be compiled in for
every build:
- `native` (which enables `embedded-fonts`): no extra work.
- The RAM image (feature off, fonts read in place): `Font::glyph` needs one
  branch that serves F39 from the embedded table, costing ~6 KB of the tight
  RAM-image CODE budget. Alternatively, ship C4 only in the native build.

- **Glyphs:** the 16 F78 glyphs `space + - . 0-9 L O`.
- **Format:** 4 bpp, column-major (the `stockfont.rs` layout).
- **Line metrics:** `line_h 39`, `descent 0`.
- **Per glyph:**
  - `w' = ceil(w/2)`, `h' = ceil(h/2)`;
  - `adv' = floor(adv/2)` (digits 30, `.` 15, `-` 20);
  - `top = 78 − 1 − h − yoff` (the F78 glyph top in its line);
  - `top' = floor(top/2)`, `yoff' = 39 − h' − top'`.
- **Pixels:** α'(x, y) = (Σ α(2x+dx, 2y+dy) + 2) / 4 over dx, dy ∈ {0, 1}.
  Pixels outside the source glyph count as 0. Pack into 4 bpp column-major.
- **Size:** about 6 KB of flash. The source F78 blob is ~23 KB.
- **Reference:** the `F39` class in `docs/mockups/tools/render.py` is this
  exact transform. The mockups use it.

Cells (`CELLS39`: sign 18, digit 29, point 10; seven cells = 174 px):

```
const CELLS39: [[u8; 7]; 5] = [
    [18, 29, 10, 29, 29, 29, 29],   // %7.4f
    [18, 29, 29, 10, 29, 29, 29],   // %7.3f
    [18, 29, 29, 29, 10, 29, 29],   // %7.2f
    [18, 29, 29, 29, 29, 10, 29],   // %7.1f
    [18, 26, 26, 26, 29, 29, 20],   // "   0L  "
];
```

The widest F39 digit is 28 px ('4'), so a 29 px cell leaves ≥ 1 px between
digits. F78's 60 px cell leaves 4 px. `CELLS_C` (LETTERS) is deleted with the
old compact branch.

---

## 3. Option B (not recommended): floating meter tile on the full scope

The combined view becomes the full Scope page (1:1 graph 401x281, 7-button
panel) with a framed tile (17,236,276,61) inside the graph's bottom-left
corner. The tile holds the F39 value, unit, sign and one badge
(`b1-floating-tile.png`). The tile could be dragged to another corner.

| | A: meter strip | B: floating tile |
|---|---|---|
| Graph | 401x225 (80 % of rows) | 401x281, but ~15 % of it permanently covered |
| Sub-readings (AC+DC parts, Hz/duty) | fit, 22 px | do not fit; they would need a second tile or the measurement bar |
| Scope code changes | new `ScopeView` geometry + grid-from-pitch fix (needed anyway, S8) | `column_t` must mask a rectangle in every column it crosses, and trace, markers, cursor tabs, the dashed trigger line and the X-cursor tabs on the bottom edge (scope-ui-plan §2.3) all collide with the tile |
| Touch | strip is a clean, large non-graph zone for view swipes, function row, REL | tile sits inside the captured graph area: its taps fight the double-tap and drag bands, and "drag tile" is another drag object |
| Readability | value always in the same place, top | value low on the screen, over the trace |

B saves graph rows but hides trace, which a scope must never do. It also costs
more renderer and gesture special cases than A. A wins on what the owner asked
for: one visual language, nothing overlapping.

---

## 4. Full Scope page: what changes to stay consistent

The full Scope page uses the same top bar (§2.3), including the page dots,
uptime, lock, BT and battery. The record bar becomes the 2 px band at y 18..19
and the generator chip moves to (414,301,66,19). scope-ui-plan §3.1 otherwise
stands: 7 buttons with 41 px pitch, 1:1 graph, markers. The only change from
§3.1's bar widths is that the status cluster takes x 296..479, because §3.1's
40 px status block cannot hold uptime, lock and BT.

---

## 5. Delta against scope-ui-plan

| Plan item | Kept | Changed by this design |
|---|---|---|
| §3.2 layout | graph x 13..413, top edge y 76, mini panel in x 414..479, chips semantics, bands/markers from S4, `power_both` | the DMM function row (y 277..319) is removed; the graph grows to y 300 (h 225, pitch 28); the measurement bar is kept; the 0..21 DMM bar + 22..55 DMM row become the scope top bar + meter strip; panel = V⊕ V⊖ T⊕ T⊖ AUTOSET with 41 px buttons (RUN/STOP moves to the RUN chip) |
| §3.1 top bar | chips and order | status cluster x 296..479 (dots, uptime, lock, BT, battery); record bar → 2 px band at y 18..19; GEN chip → (414,301,66,19) |
| §2.5 side keys | DMM meanings in the combined view | none (confirmed) |
| §2.6 gestures | drag ≠ swipe, capture at Down, double-tap, long-press, multi-finger handling | pinch = scale is added (§6.3; the plan rejected pinch); 2-finger swipe-down is told apart from pinch by spread change |
| S8 | — | depends on F39 (new step C1) and the grid-from-pitch fix; the function row does not need S6 |

---

## 6. Scope gesture map (Scope page and the combined graph)

Owner requirements:
- pinch = scale;
- one-finger drag = position;
- the trigger level is draggable, with a dashed line while dragging and a
  permanent marker;
- view swipes must not conflict;
- "one thing per drag" (a horizontal drag must never move the vertical).

### 6.1 One finger in the graph

The touch is **captured at Down** (the existing `capture()`), so it can never
become a view swipe.

**Axis lock.**
- At touch-down, snapshot `Settings` and the cursors (`touch_snap`, already
  there).
- When the travel first reaches `AXIS_SLOP` = 12 px:
  - lock **X** if |dx| ≥ 2|dy|;
  - lock **Y** if |dy| ≥ 2|dx|;
  - otherwise keep waiting until 24 px, then lock the larger component.
- The lock holds **until release**. The other component is ignored completely.
- Values are always `snapshot + f(Δ along the locked axis)`. Nothing jumps to
  the finger.

**What the drag moves** is a pure function of the start zone and the locked
axis. Put it in `scope_touch.rs` (scope-ui-plan S4) so it can be tested on the
host.

| Start zone (hit-tested at Down) | Y-locked drag | X-locked drag |
|---|---|---|
| Right trigger band: last 40 graph columns (x 373..413), full height | **trigger level** | trigger position |
| Within ±20 px of a horizontal cursor (cursors on) | that cursor | trigger position |
| Within ±20 px of a vertical cursor (cursors on) | vertical position | that cursor |
| Anywhere else in the graph, incl. the left gutter x 0..12 | **vertical position** (offset; the trigger row moves with it so the trigger voltage holds, plan §4.2) | **trigger position / time offset** while running; pans the stored record while stopped |

**Feedback while dragging:**

| Object | Feedback |
|---|---|
| Trigger level | Dashed orange line (4 px on / 4 px off, 0xFAA0) across the graph at the level row while dragging and for 1.5 s after release. Readout bubble "T 3.57 V" (ASCII26 orange on 0x18C3) right-aligned at x 404, 6 px above the line, flipped below the line near the top. Permanent marker: the orange ◀ (6x11) on the graph's right edge, cols 394..399, plus an arrow at the top or bottom edge when off-screen. See `a7-trigger-drag.png`. |
| Vertical position | The yellow "1▶" gutter marker follows; bubble "Pos +1.20 V" |
| Trigger position | The orange ▼ on the top edge follows; bubble "T −2.00 ms" |

Rendering:
- The dashed line and the markers are template overlays in the column
  renderer (§7), so only the columns and rows that change are rewritten.
- A move of the trigger line redraws 2 rows' worth of pixels across 401
  columns. That is a row redraw, cheap enough to do on every `Drag`.

Other one-finger gestures:
- **Double-tap:** left gutter or body → offset 0; right band → level 50 %;
  top 24 px of the graph → trigger position centre.
- **Long-press 700 ms** without movement → quick menu (plan §2.2).
- **Corner taps:** optional (plan §2.4).

### 6.2 Direct drag vs press-and-hold-to-grab

| | Direct drag (axis lock + zones) | Hold-to-grab (~300 ms, then drag) |
|---|---|---|
| Speed | one motion, fast; this is the stock finger control owners praise | +300 ms per adjustment |
| Wrong-object risk | the zone decides; mistakes are possible at the right-band edge | none: the grabbed object is highlighted + beep before anything moves |
| Axis knock-on (owner's complaint) | solved by the lock | solved by the lock too (the lock is needed either way) |
| Conflicts | none new | long-press (700 ms quick menu) must move or be gated; a quick flick does nothing, which feels dead |
| Discoverability | markers + bubbles | needs a hint |

**Recommendation.** Use direct drag with axis lock as the default. Most of the
"vertical knocked off" complaint is the missing axis lock and the absolute
drags, not the lack of a grab step. The trigger has its own zone and handle.

Make hold-to-grab a **runtime** switch so it can be A/B tested on the device
without reflashing:

- **The switch.** `Settings → "Scope drag: Direct / Hold"` and
  `DBG:GRAB <ms>` (0 = direct, e.g. 300 = hold). The existing const
  flag becomes the default value of a `static` the app reads.
- **Hold mode:**
  - the finger stays within 8 px for `GRAB_MS`;
  - then the nearest object within 24 px is grabbed: the trigger line row
    anywhere along it, a cursor line or a marker; otherwise "trace", whose
    axis is then locked;
  - the grab gives a 20 ms beep and the object turns white;
  - movement before the grab does nothing;
  - the quick menu moves to 1200 ms, reached only if nothing was grabbed.
- **A/B protocol.**
  - Five tasks, five runs each, in each mode:
    1. set the trigger to a given level;
    2. move the trace one division up;
    3. shift time by two divisions;
    4. place an X cursor;
    5. a mixed sequence.
  - Add `DBG:GEST?`: a 32-entry ring of (object, axis, Δ, duration) per drag.
  - Compare wrong-object drags and total time.
- **Hybrid fallback**, if the trigger band produces mis-grabs: direct for
  position, hold only for the trigger.

**What carries over from the axis-lock work now in progress:**
- the tracker decides the axis once (`Drag` carries it, or the app latches it
  on the first `Drag`);
- relative deltas from the Down snapshot;
- `Cancel` restores the snapshot;
- the dashed line as a renderer overlay.

This design only adds the zone table, the pinch path and the runtime grab
switch on top.

### 6.3 Two fingers: pinch = scale

Classification for a touch whose point count reaches 2:
- The second finger landing sends `Cancel` (as today): the first finger's drag
  is undone.
- Record both points `a0, b0`, the spread `s0 = (|bx − ax|, |by − ay|)` and the
  centroid `c0`. Identify the fingers frame to frame by nearest neighbour,
  because the controller's slot order can change.
- Once any of |Δs_x|, |Δs_y| or |Δc| reaches 16 px, decide (locked until
  release):

| Decision | Condition |
|---|---|
| **Power-off swipe** (existing `MultiSwipeDown(2)` → confirm dialog) | Δc_y ≥ 16, and Δc_y > 2 · max(\|Δs_x\|, \|Δs_y\|) |
| **Pinch** | otherwise, and only if `c0` is inside the graph |

  Pinch axis:
  - **X (timebase)** if |Δs_x| ≥ 1.5 |Δs_y|;
  - **Y (V/div)** if |Δs_y| ≥ 1.5 |Δs_x|;
  - else the larger component of `s0` (fingers side by side = timebase).

  Outside the graph, two fingers only ever make the power-off swipe.

Scale and snap:
- `k = s_axis / max(s0_axis, 40)`. Fingers apart = zoom in, i.e. a smaller
  V/div or a faster timebase.
- `n = 3 · log10(k)` is the number of 1-2-5 steps. Apply
  `step = round(n)` with ±0.15 hysteresis around the current step. The new
  index is the snapshot index ∓ step, clamped to `SDIV` (30) / `VDIV` (10).

Feedback (`a8-pinch-timebase.png`):
- a bubble in the graph centre (ASCII26 FG on 0x18C3), e.g. "5 ms/div" or
  "500 mV/div";
- the corresponding chip drawn inverted;
- the hardware is applied when the step changes, at most every 150 ms, and
  finally on release.

Edge cases:
- A third finger turns the touch into a screenshot candidate and restores the
  snapshot.
- In FFT mode, X-pinch = span and Y-pinch = dB/div.
- In the stopped state, X-pinch zooms the stored record (the existing
  stop-and-zoom).

### 6.4 Global map and conflicts

| Gesture | Starts | Result |
|---|---|---|
| 1-finger swipe L/R | outside the graph only: top bar, meter strip, measurement bar, right panel (a Down on a button that moves > 15 px cancels the button) | previous / next view |
| 1-finger drag | inside the graph (captured) | §6.1, never a swipe |
| 2-finger pinch | centroid in the graph | §6.3 |
| 2-finger swipe down | anywhere | power-off confirm (unchanged) |
| 3-finger swipe down | anywhere | screenshot (unchanged) |
| 1-finger swipe down | meter strip / DMM page | CSV record (planned) |

On the DMM and generator pages, swipes work anywhere, as today. The optional
2-finger horizontal view swipe from plan §2.6 is dropped: horizontal 2-finger
motion in the graph is now a pinch, and the strip and bars give enough swipe
area.

### 6.5 What the touch driver provides, and what must change

What the driver provides today:
- **Points.** The Hynitron 0x5A path reads all points: `Event.points` plus
  `pts[..points]`, with `MAX_POINTS = 5` (`touch.rs read_hyn`).
- **Multi-finger handling.** `gesture.rs` already emits `Cancel` on the second
  finger and classifies multi-finger swipes by centroid on release.
- **Polling.** The main loop polls only when INT is low, every 10 ms
  (`main.rs next_touch`), so the maximum rate is 100 frames/s.
- **Bus.** The bus is bit-banged I2C at ~125 kHz (T = 4 µs half-bit). One frame
  is the 7-byte read, one 5-byte read per extra point, and the ack: ≈ 1 ms +
  0.7 ms per extra finger. A pinch therefore costs ≈ 2 ms of every 10 ms loop.
- **Debounce.** A new touch is debounced 40 ms. Point-count changes and moves
  of an existing touch are reported at once.
- **Unmeasured:** the controller's own report rate, two-finger tracking
  quality and slot stability on this panel.

What must change:
1. `gesture.rs`:
   - axis-locked `Drag` (in progress elsewhere);
   - live two-finger tracking with nearest-neighbour finger identity;
   - spread and centroid;
   - the §6.3 classifier;
   - new `Gesture::Pinch { axis, k_milli }` and `PinchEnd`;
   - the `MultiSwipeDown(2)` test must reject touches whose spread changed by
     more than 30 %.
2. `app.rs`: pinch handling with the snapshot (`touch_snap`), throttled apply,
   and the bubble; the zone table from `scope_touch.rs`; the runtime
   `GRAB_MS`.
3. Measurement first: `DBG:DIAG` touch events/s, plus a raw point dump with
   two fingers (spread noise, slot swaps, ghosting when the fingers share a
   row or column).
4. Optional: move to the I2C3 peripheral at 400 kHz, which cuts a 2-finger
   frame to ~0.5 ms (`touch.rs` header already notes the move).
5. The wake-tap swallow (ui-review #3), so a wake touch cannot move the trigger.

---

## 7. Graphics library or own renderer? (owner's question)

### 7.1 Numbers

**LCD bus.**
- ILI9488 on 16-bit FSMC bank 1, `BCR1 0x5091` (16-bit, WREN, EXTMOD),
  `BWTR1 0x0FF00201` (mode A write: ADDSET 1, DATAST 2).
- A write takes ADDSET + DATAST + 1 = 4 HCLK = 20 ns at 200 MHz, plus ≥ 1 idle
  HCLK between transactions: **≈ 20–25 ns per pixel, 40–50 Mpx/s**. This is
  the stock timing, so the glass accepts it.
- Reads (`BTR1 0x0F04`, ADDSET 4, DATAST 15) are ~5x slower but only matter
  for screenshots.

**Bus floor:**

| Area | Pixels | Time |
|---|---|---|
| Full screen | 153 600 | 3.1–3.8 ms |
| Full-scope graph (401x281) | 112 681 | 2.3–2.8 ms |
| Combined graph (401x225) | 90 225 | 1.8–2.3 ms |

**CPU.**
- The existing column renderer (`ScopeView::column_t`) does per pixel two
  range compares, a template load and a 16-bit store: ≈ 6–10 cycles, i.e.
  30–50 ns.
- That puts a full graph at ≈ **3.5–6 ms** (estimate; measure with the DWT
  counter as plan S0 `prof[2]`).
- It already redraws only the columns whose trace segment changed, so a
  typical frame costs 1–3 ms.

**Memory** (native build, measured from the current
`target/native/.../opendm40`):

| Resource | Free |
|---|---|
| Flash (FLASH_B, 622 576 B) | 371 252 B (`.text` 251 276 B) |
| SRAM (192 KiB) | 124 952 B (`.dmabuf` 71 656 B) |
| TCM (64 KiB) | 38 988 B (`.bss` 26 500 B, stack included) |

A full RGB565 framebuffer is 307 200 B and the full graph alone is 225 362 B:
**neither fits**. The RAM-loaded build has even less (.dmabuf 59 KB of 72 KB).

**Conclusion: speed is not the problem; RAM is.** No library draws faster than
the bus. Any option has to render in lines, columns or strips.

### 7.2 Options

| | (a) Own compositor (extend `column_t`) | (b) `embedded-graphics` | (c) Slint, MCU software renderer | (d) LVGL via C |
|---|---|---|---|---|
| Model | per column (or 401xN strip): grid template → overlay rows (dashed trigger line, Y cursors) → X-cursor/marker columns → trace segment → sprite layer (bubble text pre-rendered once into a small 4 bpp alpha buffer) → one window write per column | `DrawTarget` over `lcd.rs`; primitives drawn straight to the LCD | line-by-line renderer, no framebuffer, `.slint` UI | widgets + partial draw buffer |
| Flicker | none (each pixel written once) | overdraw where layers overlap unless drawn into a strip buffer | none | none |
| Full graph redraw | ≈ 3.5–6 ms; dirty columns 1–3 ms; DMA mem→FSMC from a column buffer can overlap CPU | similar with `fill_contiguous`; ~10x slower through per-pixel `draw_iter` (a window set per pixel) | estimate 10–40 ms (path/polyline rasterisation of a 400-point trace) | estimate 10–30 ms (`lv_chart` antialiased lines) |
| Flash | +3–6 KB | +8–20 KB (primitives, adapter to the stock 4 bpp fonts) | order of 200–400 KB with fonts, vs 371 KB free | order of 100–250 KB |
| RAM | ~0.6 KB templates + ≤ 4 KB sprite | 0 (or a strip buffer) | needs `alloc` + heap (tens of KB); fw-rs has no allocator today | ≥ 15–30 KB draw buffer + 32–64 KB heap |
| Other costs | none; keeps the stock fonts and the per-digit/per-column diffing (ui-review §0) | stock fonts need an adapter | all screens rewritten; fonts re-converted; embedded use under GPLv3 or a commercial licence | C toolchain + FFI; all screens rewritten; fonts re-converted |

The (c) and (d) sizes are order-of-magnitude estimates from typical MCU
builds, not measured here.

**Recommendation: (a).** Generalise `column_t` into a small layer compositor:

```
Layer list per column:
  background template (grid)
  overlay rows (dashed trigger line, cursor rows)
  column overlays (X cursors, T marker)
  trace
  sprites (bubbles, badges inside the graph)
```

Each layer reports its dirty columns and rows. Gesture overlays then cost a
few rows or columns, not a full redraw.

Optional extensions:
- If overlays grow complex, a **4 bpp indexed graph canvas** (401x281/2 =
  56 KB of SRAM, native build only) with a 16-entry palette is the next step.
  It blits at bus speed.
- `embedded-graphics` is worth adding only as a rasteriser into small
  offscreen sprites (rings, rounded boxes, icons), not as the screen
  pipeline.
- Measure first: add `prof[2]` (full-graph and dirty-frame draw time) before
  any refactor.

---

## 8. General UI review (only items not already in ui-review.md / scope-ui-plan.md)

### P1
1. **Two typeface families on one screen.**
   - The 4 bpp Roboto-like faces (F78, LETTERS, UNIT, F21, K_FONT, ASCII26)
     are mixed with the 1 bpp bitmap faces (TEXT22, F14, HIST).
   - Rule: readouts and page text in the 4 bpp family; F14 only for dense
     scope chrome; TEXT22 and HIST nowhere new.
   - The combined view gets this rule here.
   - Fix the most visible mix: the `A-HOLD` label in TEXT22 sitting in an F21
     bar (`hold_label`). Draw it as a badge pill in F14 (as §2.4) or as F21
     "AHLD".
2. **Status cluster position differs per page.**
   - Uptime, lock, BT and battery are at x 205/335/390/435 on the DMM page.
   - The Scope page shows only the battery; the generator page shows nothing
     (ui-review #2, #9).
   - Concrete fix: one cluster at fixed x for all four cycle views. Use
     §2.3's right-aligned cluster (dots 296..328, uptime 334..385, lock
     388..411, BT 414..431, battery 436..475) on the 20 px scope-family bar.
     On the DMM page's 34 px bar use the same x, vertically centred, with F21
     uptime.
3. **Value hierarchy inversions.** A unit or mode glyph must never be taller
   than the digits it qualifies. Today's compact view (UNIT 29 px next to
   22 px digits) breaks this. The rule also holds on the generator tiles
   ("KHz" in TEXT22 next to bold digits is fine; keep it that way).

### P2
4. **Generator value tiles use three faces**: label TEXT22, value bold 4 bpp,
   unit TEXT22. Use ASCII26 in 0x94D3 for the label and LETTERS for value and
   unit (FG). That matches the meter strip's T2.
5. **Menu, Settings, Device and Range pages are in TEXT22** (1 bpp mono, from
   the stock Chinese UI); the power-off dialog is in a sans. Move page text to
   ASCII26 (full ASCII, 18 px caps). Keep TEXT22 only where Ω or CJK is needed
   (the Range page can use LETTERS for "600mV/6V/..", which has Ω).
6. **Colour tokens.** Three near-identical dark backgrounds (0x0883, 0x10A3,
   0x2124), three tile greys (0x2966, 0x3A08, 0x2146) and two accent blues
   (0x0419, 0x04FC) are spread over `ui.rs`. Collapse them into one token
   table:

   | Token | Value |
   |---|---|
   | bg | 0x0883 |
   | surface | 0x2966 |
   | chrome | 0x4208 |
   | chip | 0x3186 |
   | accent | 0x0419 |
   | accent-text | 0x04FC |
   | value | 0xF145 |
   | trace | 0xFFE0 |
   | trigger | 0xFAA0 |
   | ok | 0x04A9 |
   | warn | 0xFC40 |
   | danger | 0xF800 |
   | hv | 0xC9A0 |

   Use it in every view. Pure refactor; no visible change except the generator
   and menu backgrounds converging.
7. **DMM history row.** Five empty 19 px grey bars look like broken progress
   bars (shots 01/02/34). Draw them only when the slot holds a value, or put a
   dim "—" in empty slots.

### P3
8. **Main menu.** The "Combined view: DMM + Scope" row is a text strip 7 px
   from the bottom edge with different padding from the four tiles above. Make
   it a fifth tile row of equal height (46 px margins), or drop it once the
   page dots exist.
9. **Settings.** The "Device" row has no value and no chevron. Add "›" at
   x 452 like a navigation row.
10. **DMM chart.** The "320mS/div" box overlaps the left y-axis label column
    (x 0..79). Move it under the axis labels, or into the chart's top-left
    inside corner.
11. **Temperature bar label.** "TCK" in F14 inside the 34 px F21 bar reads as
    a glitch (shot 05). Use the same F14 badge pill as §2.4 slot 0 ("TC-K").

---

## 9. Implementation steps (ordered)

| # | Step | Size | Files | Depends on |
|---|---|---|---|---|
| C1 | **F39 font**: derivation in `tools/mkstockfont.py`, `F39_FD` + `stockfont::F39`, `CELLS39`; host render check with `docs/mockups/tools` | S | `tools/mkstockfont.py`, `src/stockfont_data.rs`, `src/stockfont.rs` | — |
| C2 | **Scope-family top bar**: chips as pills, status cluster (dots, uptime, lock, BT, battery) for Scope + combined; GEN chip to (414,301); 1 s status update for the Scope page too | S | `ui.rs` (`ScopeView::labels`, `gen_state`, new `scope_status`), `app.rs` (status timer) | — |
| C3 | **Combined layout**: `SCOPE_COMBINED {13,76,225}`, grid rows from pitch in `templates_c`, gutters, 5-button panel (icons from primitives), measurement bar; delete `SCOPE_COMPACT` and the DMM button row in this view | M | `ui.rs`, `app.rs` (`draw_static`, `tick` frame/labels, `scope_measurements` for DmmScope) | C2 |
| C4 | **Meter strip**: `DmmView::strip()` replaces the `compact` branch (F39 value, unit/sign, two sub rows with tracked left edges, three badge slots incl. HV/limits/REL alternation, ≥ 100 V ring) | M | `ui.rs` (`DmmView`), `app.rs` (hv flag for DmmScope) | C1, C3 |
| C5 | **Combined touch**: strip tap → function row overlay (4 s, MENU closes, graph rows 274..300 redraw), badge column → REL, long-press → DMM view; panel taps (V/T steps, AUTOSET); graph hit-tests use the current `ScopeView` (not hard-coded 20..301) | S–M | `app.rs` (`tap`, `gesture`, overlay flag), `ui.rs` | C3, C4 |
| G1 | **Zone table + runtime grab switch**: `scope_touch.rs` (zone × axis → object), `GRAB_MS` static + `DBG:GRAB`, `DBG:GEST?` ring; builds on the in-progress axis lock | S | new `src/scope_touch.rs`, `app.rs`, `scpi.rs` | axis-lock work |
| G2 | **Markers + trigger feedback**: ◀ trigger marker, 1▶ offset marker, ▼ T marker, dashed level line 1.5 s, bubbles (sprite layer) | M | `ui.rs` (`column_t` overlays, sprite), `app.rs` | G1 |
| G3 | **Pinch**: two-finger tracking + classifier + `Pinch`/`PinchEnd` in `gesture.rs`; apply with throttle + bubble in `app.rs`; reject pinches in the 2-finger power-off test | M | `gesture.rs`, `app.rs`, `ui.rs` | G1; device measurement of 2-finger tracking first |
| G4 | **A/B session** on the device (direct vs hold), decide the default, record it in `parity.md` deviations | S | — | G1–G2 |
| R1 | **Measure redraw** (`prof[2]`); compositor refactor of `column_t` into layers when G2's overlays land | S + M | `main.rs`, `ui.rs` | — |
| U1 | Review P1 items 1–3 (A-HOLD badge, unified status cluster on the DMM page, hierarchy rule) | S | `ui.rs` | C2 |
| U2 | Review P2 items 4–7 (gen tiles, page text → ASCII26, colour tokens, history placeholders) | M | `ui.rs` | — |
| U3 | Review P3 items 8–11 | S | `ui.rs` | — |

Order: C1 → C2 → C3 → C4 → C5 (the combined view is usable after C5). G1 → G2
→ G3 → G4 can run in parallel with C2..C5, since they touch the gesture side.
R1 comes before G2's overlays. U1..U3 any time.

## 10. Mockup index (`docs/mockups/`)

| File | Shows |
|---|---|
| `cur-combined.png`, `cur-combined-pwm-acv.png`, `cur-combined-pwm-freq.png` | device screenshots, build db679a3 (non-AC+DC: HEAD is pixel-equal except the 36 px mode box) |
| `cur-combined-acdc-head.png` | HEAD (40d9a76) AC+DC compact layout rendered over shot 16 |
| `a1-dcv.png` … `a5-temp.png` | Option A states (§2.4) |
| `a6-function-picker.png` | strip tap → function row |
| `a7-trigger-drag.png` | trigger-level drag: dashed line, readout, marker |
| `a8-pinch-timebase.png` | horizontal pinch → timebase bubble, chip highlighted |
| `b1-floating-tile.png` | Option B |
| `fonts-hierarchy.png` | the four type tiers |

## 11. As implemented (C1..C5, G1; 2026-10-13)

The owner's layout rules came after the mockups: every text / icon / shape
ink box >= 3 px from any other and >= 3 px inside its container, >= 4 px from
the screen edges, no touching glyphs. `tools/ui-render/run.sh` builds the
real `ui.rs` / `stockfont.rs` drawing code for the host (LCD emulator, draw
calls probed under `cfg(test)`) and checks these rules over a worst-case
matrix (every function x AC/DC/AC+DC x OL x HOLD/A-HOLD x REL/PASS/FAIL x HV,
sensors, every timebase x V/div x probe x run state x trigger level, status
block combinations, the full Scope page, the DMM / generator / menu bars);
it must report 0 violations. Geometry changed from §2.3 to satisfy them:

| Item | §2.3 | Implemented |
|---|---|---|
| Top bar | 20 px | 27 px (combined; 28 on the Scope page): the 20 px lock / BT icons need 4 px to the screen edge and 3 px to the bar edge. Round Wiren Board logo (20 px, `tools/mklogo.py`) at x 4 on every bar. Chips 17 px pills at x 27 / 67 / 112 (70 wide since the F14 space fix) / 185; REC 251; dots 301..332; uptime ..385; lock ..412; BT 416; battery 436..475 |
| Meter strip | y 20..75 (56) | y 27..83 (57): three 15 px badges need 51 px plus margins |
| Combined graph | 401x225, pitch 28 | 401x217 at y 84, pitch 27 |
| Full Scope graph | 401x281 1:1 | 401x273 at y 28, pitch 34 (rows scale by (h - 3) / 278) |
| Panel | 5 x 41 px | combined 5 x 39 px, Scope page 7 x 35 px (V+, DC/AC, V-, T+, mode, T-, slope, labelled with their state) |
| F39 | baked 6 KB font | F78 at half size at draw time (`stockfont::half_text`, 2x2 box filter as `label_half`), proportional: each glyph advances by its half-size width + 3 px |
| Units | LETTERS | LETTERS, F21 where LETTERS lacks the glyph ("nF", "uF") |
| AC+DC parts | "~ 53.63mV" | "~ 53.63": the parts are in the main unit, shown next to them |
| >= 100 V | 2 px red ring | dark red strip background (a ring cannot keep 3 px to the badges) |
| Measurement bar | one label per cell | name and value on one baseline, 1 px glyph spacing; the frame line at y 302 is gone |
| Function band | (0,274,480,46) over the bar | y 258..300 (under the T- button, above the measurement bar), buttons 76x36 |

Colours: DMM readings use the stock value colour 0xFC40 (`gui_v2_6.h`
GUI_C_VALUE) instead of red.

Touch (C5 / G1): strip tap = function band (4 s from the last tap, MENU or a
tap elsewhere closes it), badge column (x < 54) tap = REL, strip long-press =
DMM page, panel / run chip taps; touches in the graph are captured. Zone
table in `gesture::target` (right 40 columns vertical = level, else vertical
= offset with the trigger row following, horizontal = trigger position
`Settings::hpos` running / pan of the stored record stopped, +-20 px of a
cursor = that cursor; hold mode grabs by proximity, 24 px). Hold-to-grab is
runtime: Settings "Scope drag" (Direct / Hold 300 ms), `DBG:GRAB <ms>` /
`DBG:GRAB?`; `DBG:GEST?` returns the last 32 drags (target, axis, travel,
ms). Not done here: markers / bubbles (G2), pinch (G3), record bar (S5).
