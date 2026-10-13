# Plan: replace the fw-rs scope UI with a replica of the stock V2.6 scope page

2026-10-13. Spec: `docs/stock-scope-ui.md` (+ `tools/stock-ui/notes/`).
Tooling: `tools/stock-ui/` (renderer of the spec, diff tool, glyph dump).
Owner verdict: the stock scope UI is "two heads better" than ours, and it is
"responsive and intuitive, everything can be dragged". Decision: reproduce
the stock page near pixel-exactly first (look AND interaction), then re-add
our functional improvements inside its visual language.

## 0. What changes, in one picture

```
                 today (aa51ae5)                          target
top bar          28 px, our chips + logo + lock/BT/batt   20 px stock bar: 5 badges + battery (sec. 2 of the spec)
graph            401x273 at y 28, pitch 34, our grid      401x281 at y 20, stock grid rule (7/8 px dots, ticked cross)
markers          6-col orange tick, dashed level line     stock 12x12 glyph markers (A, T, 1, 2, M), guide lines for 2 s
right panel      7 x 35 px buttons "V+ DC V- T+ AUTO T- /" stock 3+3+1 pills: + CHA/1V - | + --/1V - | TRIG
measurement bar  4 cells, F14, 1 px glyph spacing         stock 98x17 ringed cells, 12 px font, "Vp-p:3.00V"
menus            View::ScopeMenu (our list pages)         stock MF popup (4 tabs), CH1 popup, Math popup
touch            zone table + axis lock + hold-to-grab    stock model: 32x32 marker zones, select on press, per-event axis,
                                                          tap = timebase step; our axis lock kept as an option
combined view    our strip + scaled graph                 stock page + a 56 px strip in the stock language (mockup mk01)
```

## 1. Our modules touched

| module | today | change |
|---|---|---|
| `src/ui.rs` `ScopeView` (`draw_static`, `labels`, `templates_c`, `column_c`, `frame`, `scope_panel`, `panel_hit`, `scope_measurements`, `gen_chip`, `scope_menu*`) | our design | replaced by a new `src/scope_ui.rs` (port of `tools/stock-ui/scope.rs`); `ScopeView` keeps only the column renderer (`frame`/`column_c`) with stock templates |
| `src/stockfont.rs` + `tools/mkstockfont.py` | 19 fonts, blended text | + 8 scope fonts (sec. 3), a threshold text path, class C and the 16 px wrapper, `rbox` with the stock corner tables, `rborder`, dotted lines |
| `src/gesture.rs` (`target`, zone table, axis lock, hold-to-grab) | our model | stock model behind a mode switch (sec. 5); `Tracker` unchanged (Down/Drag/Up/Tap/multi-finger) |
| `src/app.rs` (`gesture`, `scope_drag`, `tap`, `key`, `scope_set`, `scope_poll`, `panel_press`, `cursors_from_selection`, `View::ScopeMenu` handling) | | drag -> stock selection/zone logic; tap -> timebase step; panel taps -> stock actions; popups; no trace blank on drags |
| `src/scope.rs` (engine) | | unchanged except: `Settings::hpos` is the stock trigger position (already), re-render of the last record on position/level changes in every mode (sec. 5.3) |
| `src/touch.rs` / `src/main.rs` touch loop | 10 ms poll + EXTI | unchanged; add the timing probes of sec. 6 |
| `src/dialog.rs` | toast / power-off | toast restyled to the stock (purple r8, 22 px) |
| `tools/ui-render/main.rs` | renders our scenes + layout rules | + "diff vs stock-ui" step (same scene state rendered by both, `diff.py` score must be 0 outside the trace) |

## 2. Steps, order, effort

| # | step | what | effort | done when |
|---|---|---|---|---|
| S0 | tooling (done) | `tools/stock-ui` renderer of the spec (28 scenes), `diff.py`, glyph dump, layout-check port | — | `run.sh` renders; this doc |
| S1 | fonts + primitives | `mkstockfont.py`: add 0x08054828 (class C, emit as 13 class-A glyphs), 0x0804f24c (B+; emit the ASCII part from 0x0805471c and the `[ ] ROLL` glyphs it needs from its own table, skip CJK), 0x08054148, 0x08054fc0, 0x08055014, 0x08065558, 0x0806bd44, 0x080534c4 (the 16 px ASCII of the popups; CJK 0x08052ba0 skipped). ~17 KB of flash. `stockfont.rs`: `text_thr` (no blend: v >= 8 -> fg, draws glyph boxes only), `rbox_stock(r, radius, mask, c)` with the INSET tables, `rborder`, `dots_v/h`. Unit test on the host: `tools/stock-ui` and `stockfont` render the same badge pixel-for-pixel | 1 d | ui-render diff of a badge = 0 px |
| S2 | static chrome | `scope_ui.rs`: top bar (5 badges + battery), right panel (3 pills + TRIG), measurement bar, frame + grid as column templates (`templates_c` emits the stock rule per column: x%40/y%7 dots, y%35/x%8 dots, ticks, solid cross; frame rows 20/300, cols 13/413), graph rows 1:1 (281 px, `row_px` identity). Remove the logo, lock/BT chips, uptime from the scope page (the DMM page keeps them) | 1 d | `diff.py` of our `s01-scope` vs `stock-ui st01` = 0 px outside the trace |
| S3 | markers + lines | strip markers (two-glyph draw, families/variants, y rules, off-screen arrows, selected-last order), T marker as a 12x12 overlay on the graph top (repaint its 12 columns from the templates when it moves), X/Y cursor markers, guide lines (dotted step 2 in the trace colour; solid CH1 zero line) via the column renderer (one `line_rows`/`line_cols` input instead of today's `trig_line`), cursor lines 0xad55 | 0.5 d | scenes st06..st10 match |
| S4 | interaction port | sec. 5 | 1.5 d | DBG:GEST? log shows stock-equivalent behaviour on the synthetic drags of `tools/ui-render` (§6 tests updated) |
| S5 | responsiveness | sec. 6 | 1 d | targets of sec. 6 measured with DBG:DRAG? on the device |
| S6 | popups | MF popup (Measure grid + selected list + Clear/Default, Display lists + sliders, Trigger lists, Settings: PriScr / Save CSV — ours: + the fw-rs items that used to live in `View::ScopeMenu`, as a 5th tab "fw-rs" rather than mixing them in), CH1 popup (Coupling / Probe spinner / Color), Math popup (Mode FFT/ABS greyed until we have FFT; Color), toast restyle. Modal: all taps go to the popup; tap outside closes (settings: below the box); MF closes; MENU = main menu | 1.5 d | scenes st20..st43 match |
| S7 | combined view | stock page + 56 px meter strip at y 20..75 (mockup `mk01-combined`): function badge (purple r5, 16 px), AUTO/HOLD/REL badge, reading in the 20 px digit font 0x0805c5dc right-aligned, unit 16 px, two 12 px sub-readings, GEN badge; graph 401x225 at y 76 with 8 x 28 px divisions (same dot rule with div_h 28); right panel / bar unchanged. Replaces `SCOPE_COMBINED` + `strip*` | 1 d | `mk01/mk02` match; DMM strip update stays diff-based (digits by cells) |
| S8 | our extras in the stock language | sec. 4 | 0.5 d | |
| S9 | validation with real captures | owner's V2.6 BMPs (sec. 7) -> `diff.py --regions` per state; fix until every static region is 0 % and the corner tables are confirmed; then the device shots of ours vs stock | 0.5 d + owner time | report of scores per region |

Total ~8 days of agent work. S1-S3 and S6 are independent of S4-S5 and can
run in parallel; S7 after S2/S3; S8 last.

## 3. Fonts: what to embed (sizes from the stock tables)

| font | glyphs | bytes (approx) | note |
|---|---|---|---|
| 0x08054828 class C | 13 | 0.3 KB | emit as 13 class-A entries (w 10/4, h 16, adv = w) |
| 0x0805471c (ASCII 12 px) | 73 | 1.1 KB | the real glyphs behind 0x0804f24c for ASCII |
| 0x0804f24c own table, non-ASCII | `[` `]` + nothing else needed | 0.1 KB | `[`/`]` are not in 0x0805471c |
| 0x08054148 (20 px ASCII) | 96 | 4.8 KB | panel button texts |
| 0x08054fc0 (17 px probe glyphs) | 36 | 1.2 KB | 26 used |
| 0x08055014 (edge) | 2 | 0.1 KB | |
| 0x08065558 (markers) | 42 | 0.7 KB | |
| 0x0806bd44 (battery) | 6 | 0.5 KB | replaces our 0x0804fa4c on this page |
| 0x080534c4 (16 px ASCII) | 96 | 3.1 KB | popups |
| 0x0806b13c (22 px) | already embedded | | tab labels, toasts |

## 4. Our functional improvements, restyled

| ours | stock equivalent / how it is kept |
|---|---|
| `Settings::hpos` (trigger position, pan of the stored record in STOP) | = the stock trigger position: T marker on the graph top, record bar window, horizontal drag of the selected time item; STOP pans the stored record with the limits of the stock (screen/20 margins) |
| trigger level dashed line while dragging | = the stock guide lines (dotted trigger-level line, solid CH1 zero line, dotted trigger-position line) for 2 s after the last drag event / release |
| trace offset marker / level marker | = the stock 'A' / 'T' strip glyphs with the filled-selected variant |
| cursors from the measurement selection | = stock: selecting X2-1 / Xf / Y2-1 in the Measure tab creates the white '1'/'2' markers and 0xad55 lines; ours additionally keeps the +-20 px line grab as a zone alias (sec. 5) |
| generator chip "G 1.00kHz" (measurement bar) | full page: 12 px text in the record bar's dark box, right-aligned (x 275), grey 0xad55 like "ROLL" ("G 1.00kHz" / nothing when off) — the only stock-styled free space on the page; combined view: the green GEN badge of the strip |
| combined DMM strip (reading, badges, sub-readings, HV ring, REC) | sec. 2 S7; HV >= 100 V: the strip ring turns 0xc9a0 (like the trigger badge ring but red); REC: a small orange r5 badge "REC" in place of the range badge |
| axis lock / hold-to-grab (owner A/B) | kept as Settings "Scope drag": Stock (per-event, default) / Axis lock / Hold 300 ms (sec. 5) |
| pinch = scale (G3, not done) | out of scope here; the stock has none |
| lock / BT / uptime / logo on the scope bar | dropped on the scope page (stock has none); they stay on the DMM page and in the combined strip's badge column if wanted |
| swipe between views | the stock has no page swipe; see sec. 5.4 |
| no-overlap layout rules | the stock layout breaks them (sec. 8); the rules are relaxed for the replica pages |

## 5. Interaction port (replaces `gesture::target` and the zone table)

### 5.1 Model (= spec sec. 10)

* `ScopeSel { v: 0..4, h: 0..3 }` in `App`, default 0/0 (CH1 zero, trigger
  position), persisted? no (stock: not saved).
* Zone list rebuilt whenever a marker can move (after every `scope_set`,
  cursor change, mode change): one `Zone { rect: marker rect grown by 10,
  group: V/H, id }` per existing marker. Marker rects from `scope_ui`
  (single source for drawing and hit testing).
* `Gesture::Down(p)` inside the inner rect (0,15,414,290): zone search in
  the stock order (other group after the current index; same group cycling
  from current+1; other group from 0) -> `sel.v/h` updated, marker redrawn
  filled (old one outline), guide lines on (`guide_until = now + 2000`),
  `tap_candidate = !hit`. Outside the inner rect / on the right panel:
  today's handling. Two or more fingers: nothing.
* `Gesture::Drag`: the tracker today sends drags only after SLOP (15 px)
  and with a locked axis. For the stock mode add `Tracker::stock_drag` =
  raw move events (every report, no slop, no axis) carrying `(dx, dy)`
  since the last processed report; the app applies the per-event rule
  `|dx| <= |dy|` -> V item += dy rows, else H item += dx px, with the clamps
  of the spec. Axis-lock mode keeps today's locked drags but applies them
  to the SELECTED item (not the zone of the touch-down); hold mode likewise
  after the grab.
* `Gesture::Tap` inside the inner rect and not on a zone: timebase step,
  x <= 207 slower, else faster (roll entry/exit as `scope.rs` does today).
* `Gesture::Up`: `guide_until = now + 2000`; nothing else.
* Second finger: `Gesture::Cancel` already exists; in stock mode it only
  ends the drag (no undo — the stock has none; keep our undo as an
  axis-lock-mode feature).
* Right panel: B1/B3 V/div, B4/B6 math (no-op until math exists), B2 CH1
  popup, B5 Math popup, B7 settings popup tab 2. No pressed colour.
* Keys: MENU main menu, RUN run/stop, AUTO autoset, MF settings popup tab
  0; inside popups MF closes, MENU main menu, RUN/AUTO ignored.

### 5.2 Where we deliberately differ (and why), switchable

| stock | ours | reason |
|---|---|---|
| trace blanks on every drag event in RUN | re-render the last record with the new map immediately, acquisition restarts in the background (like stock STOP) | the owner wants "traces can be dragged"; stock only does this in STOP |
| no pressed feedback | 1 frame of 0x7bef on the pressed pill (stock's own popup pressed colour) | cheap, owners asked (#26950); switchable |
| per-event axis | default stock; "Axis lock" and "Hold 300 ms" stay as Settings options | owner A/B (#44587) |
| cursor select only by marker zone | + +-20 px grab of the line itself (as today) | the lines are long, the zones small |
| 1 px trace/marker offset | reproduced (replica first); a "fix" flag later | pixel parity with the captures |

### 5.3 Engine hooks

`scope_set` today halts and restarts the acquisition and invalidates the
whole trace on every event (scope-ui-plan P8). For the stock feel: on
position / level / hpos changes call `Scope::refresh()` (re-render the last
frame with the new map, which `poll()` already supports for STOP) and defer
the hardware apply to the next poll (coalesce), so one drag report costs
one column-diff redraw (sec. 6). The level change only needs the DAC and the
marker; the position change needs the offset PWM (CCR3) and the map.

### 5.4 Leaving the page

The stock has no page swipe. Options (owner decides; default A):
A. keep our view swipes but only from the top bar (y < 15) and the right
   panel column (x >= 414), where the stock has no drags; B. drop swipes on
   the scope page, MENU key only (pure stock); C. swipes everywhere with
   the axis-lock mode only. The combined view strip is not a drag area, so
   swipes there stay.

## 6. Responsiveness target and how to measure it

Targets (per drag report, scope page, any timebase, persistence off):

| metric | stock (estimate, spec sec. 11) | fw-rs target |
|---|---|---|
| touch report -> marker/badge pixels on the panel | ~15..25 ms + report period | **<= 12 ms** after the report (one report handled per loop pass, no backlog) |
| redraw cost of one drag report | ~9 ms (full 399x279 graph + glyphs) | **<= 4 ms**: marker (12x12 x2), guide line columns, badge text, and the column-diff of the re-rendered trace (changed columns only) |
| trace follows the finger | STOP only; RUN blanks until the next record | **every mode**, re-render of the last record <= 3 ms |
| drag redraw rate | = controller report rate (one event per pass) | = controller report rate, 0 dropped reports at <= 100 Hz |
| touch report period | not programmed (measure) | same controller; measure; if < 60 Hz consider the controller's rate register (Hynitron CST2xx: measure first) |
| end-to-end finger -> pixel | unknown | <= 1 report period + 12 ms (camera check) |

Measurement on our side (S5):

* `App::prof[]` already keeps DWT max cycle counts for poll / measure / frame.
  Add `prof[3]` = drag handler (Drag event -> model -> `scope_ui` redraw
  done) and `prof[4]` = touch report -> gesture dispatch latency (store the
  DWT stamp in `touch::Event` when the report is read; the EXTI edge time is
  available in the driver's IRQ counters), `prof[5]` = reports per second
  while touching, `prof[6]` = reports dropped (queue newer than processed).
* `DBG:DRAG x0,y0,x1,y1,ms[,hold]` (exists: synthetic one-finger drag) ->
  extend `DBG:DRAG?` to return: events, max/avg handler ms, max/avg redraw
  ms, max latency ms, dropped. `DBG:GEST?` keeps the per-drag log (target,
  axis, travel, ms). `DBG:DIAG?` bytes 20..59 keep the touch bus counters.
* Real-finger numbers: `DBG:DRAG?` after a manual drag (the same counters
  run for real reports); INT period on PE5 with a scope; 240 fps phone
  camera for finger -> pixel (the owner can do both).
* Host: `tools/ui-render` counts the pixels written per drag event
  (emulator write counter) so the "<= 4 ms" budget (~36 Mpx/s flush) is
  checked without the device.

## 7. Stock screenshots the owner should capture (V2.6, 3-finger swipe down -> S0xx.BMP on the USB disk, 307 KB each; the disk holds ~28 — offload after batch A)

Each line: state, how to reach it, what it validates. Default page = power
on into the scope (Settings > Start Page, or main menu > Oscilloscope),
defaults: 200us, 1V, DC, x1, trigger 1 div up, Auto, probe nothing (or the
generator 2 MHz square into CH1 for a trace).

Batch A (static chrome, 12 shots):
1. Fresh scope page, no signal, Auto -> "AUTO" badge, empty graph, default markers (filled 'A' and 'T'). Validates everything static incl. corner pixels.
2. Same with the generator square wave connected -> "T'D", trace, measurement cells with values.
3. RUN key (stop) -> "STOP" orange badge, record bar window.
4. RUN again, then tap the left half of the graph 3 times -> 1ms/div (timebase badge text, record bar), then the right half 5 times -> 50us (tap-to-step direction check).
5. "+" button 3 times -> 5V/div ("5V" texts in badge and CHA pill); "-" 5 times -> 200mV ("200mV" texts).
6. CHA pill -> CH1 popup, Coupling tab (popup geometry, 0x7bef label).
7. In the popup: AC -> close (tap outside) -> badge glyph AC wave + the probe glyph; then Probe tab -> 10X -> close (glyph "X10", "10V" labels).
8. CH1 popup > Color > green -> close (palette recolour of pills, badges, markers, cells, trace).
9. Back to yellow; TRIG button -> settings popup Trigger tab; select Falling -> close (edge glyph).
10. MF key -> Measure tab (grid, selected list); Display tab; Settings tab (3 shots, 9/10/11).
11. (same shot count) Math pill -> Math popup Mode tab; select FFT -> close -> FFT marker, label, purple pill (12).

Batch B (interaction states, 10 shots; use the PriScr button in Settings
when three fingers would disturb a drag, otherwise the gesture):
13. Tap the 'A' strip marker, drag the trace up 2 divisions, lift, screenshot within 2 s -> guide line (solid), filled 'A', badge unchanged.
14. Tap the 'T' strip marker, drag the level down 1 division, screenshot within 2 s -> dotted level line, trigger badge text.
15. Drag the trace off the top (position > 140 rows) -> up-arrow marker variant at y 20; same off the bottom.
16. Drag the T marker to x ~ 30, then to x ~ 5 -> left-pointing variant; to x ~ 395 -> right-pointing.
17. Measure tab: add X2-1 and Xf, close -> X cursor markers + lines + cells; drag one cursor to the right edge.
18. Add Y2-1 -> Y markers (white) + lines; select the Y cursor (tap its marker) -> filled variant.
19. Persist = 1S with a slow signal -> persistence half colour; Persist = INF.
20. Timebase 500ms (Roll on) -> roll page: "RUN", "ROLL" in the record bar, growing trace.
21. Normal mode, no signal -> "WAIT" badge, empty graph; Single -> armed state.
22. AUTO key during acquisition at 1s/div, immediately RUN -> "Sampling" popup (if it can be caught); low battery toast if it appears.

Batch C (optional): Display tab brightness 50 % / grid 100 % (blend check),
the Chinese UI (language = 简体中文) of the MF popup, the generator page.

Also useful: a photo of a drag in progress at 100 ms/div (does the trace
blank? spec sec. 10.2) and a 240 fps clip of a marker drag (latency).

## 8. Layout-rule check of the stock layout (`tools/stock-ui/out/check.txt`)

The owner's rules (>= 3 px between ink boxes, >= 3 px inside a container,
>= 4 px from the screen edge, no touching glyphs) are violated ~50 times per
stock scene (28 scenes, 1447 total). The substantive classes:

* badges at y 1 and the TRIG button / cells at y 319 / 318: 1 px (0 px) from
  the screen edge; the strip markers at x 1;
* 12 px and 16 px text in 17 px badges / cells: 2 px inside their shape;
* the three CH1 pills and the three math pills: 1 px apart (the stock draws
  one pill split by 1 px lines);
* marker glyphs 0 px inside the strip region (they are 12 of its 13 px).

The stock layout should not be adjusted to pass; the recommendation is to
exempt the replica pages from the rules (keep them for the DMM page and the
menus) and keep the check running as a regression tool only for text
collisions (the "glyphs touch" rule is off for the 12 px font anyway).

## 9. Risks and loose ends

* Flash: +17 KB of fonts, +~6 KB of code; the native image budget must be
  rechecked (`native-boot.md`).
* The host corner tables are a port; a wrong row costs <= 6 px per corner
  and is fixed from the first capture.
* Math/FFT: the pills and popup exist in the stock; we have no FFT engine.
  Draw them (grey, "--") and keep the Mode tab inert until FFT exists.
* The combined view's 225 px graph is not 1:1 with the 280-row model
  (`row_px` scaling stays); only the full page is pixel-exact.
* Touch report rate is the controller's; if it is low (< 50 Hz) the "feel"
  ceiling is the hardware, same as stock.
* The RUN-mode trace blank on drags (spec sec. 10.2) is read from the code
  and contradicts the owner's impression; verify it first (100 ms/div, drag
  the trace, phone video). If the stock does keep the trace, the apply
  chain has a path the notes missed and sec. 5.3 is simply "do what stock
  does".
* `tools/stock-ui/regions.json` (23 regions for diff.py) and `scope.rs
  REGIONS` (10 regions for the layout check) are two tables; merge when the
  firmware port needs a third copy.
* `tools/stock-ui/notes/` also holds the agents' scratch (`__pycache__`,
  `_crop*.png`, helper scripts); the four `*.md` plus `rounded_masks.txt`,
  `fontC_08054828.txt`, `glyphs_*.txt` and `tools/rounded_mask.py` are the
  parts worth keeping.
* `diff.py` round-trip checked: a synthetic stock-format RGB565 BMP
  (BI_BITFIELDS, 307266 bytes) and a 24-bit BMP of the same render both
  diff to 0.00 %; 2x renders are scaled down automatically.
