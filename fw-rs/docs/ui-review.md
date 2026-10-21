# fw-rs UI/UX review

Review of the fw-rs replacement firmware's user interface on the Alientek DM40
(480x320 landscape colour LCD, capacitive touch with a resistive feel, 4 side
keys MENU / RUN / AUTO / MF, plus gestures). All three instruments (DMM, scope,
generator) run at once; only the displayed view changes.

**This is a review only — no source was changed.** Citations are `file:line
(fn)`; the working tree was dirty when this was written, so trust the function
name over the number if they drift.

Sources read: `src/ui.rs`, `src/app.rs`, `src/scope.rs`, `src/gen.rs`,
`src/text.rs`, `src/stockfont.rs`, `src/gesture.rs`, `src/touch.rs`,
`docs/parity.md`, and the stock reference `firmware/re/analysis/gui_v2_6.h`,
`gui_render_v2_6.h`, `scope_gen_ui_v2_6.h`.

Context the user supplied: fw-rs no longer needs to match stock 1:1 —
improvements are welcome. The DMM+scope combined view is being reworked to give
the scope more room (smaller DMM text + top bar). A fast-USB data logger and a
CSV-to-disk record mode (started from the DMM screen by a swipe) are planned.
The main DMM value is drawn red by requirement.

---

## 0. The font palette you have to work with

Every live screen is drawn with the stock bitmap fonts re-read from flash
(`stockfont.rs:222`-`237`; `gui_v2_6.h` §5 font table). The usable glyph heights
are roughly:

| px | font const | role |
|----|-----------|------|
| 12 | `Font(0x08065558)` / icons | tiny ASCII, chevrons |
| 14 | `F14` (1 bpp) | scope/chart small labels |
| 15 | `HIST` | DMM history boxes |
| 20 | `K_FONT (0x0805c5dc)` | keypad digits |
| 21 | `F21` | DMM status bar, gen RUN/STOP |
| 22 | `TEXT22` | menu / settings text |
| 26 | `ASCII26` | scope/generator body |
| 30 | `LETTERS` | DMM button letters, aux readouts |
| 31 | `UNIT` | DMM main unit |
| 32 | `MODE` | DC/AC/diode glyphs |
| 78 | `F78` | DMM main readout |

**There is nothing between 32 px and 78 px.** The reworked compact DMM reading
therefore has three honest options, in order of effort:

- **S** — reuse `LETTERS` (30 px); crisp, already the aux-readout font, but
  small for a primary value.
- **M** — box-filter the F78 4 bpp data 2x2 at build time into a ~39 px font
  (the glyph format is column-major 4 bpp, `stockfont.rs` `Glyph::alpha`).
- **M** — generate a fresh ~44-48 px numeric font with `tools/mkstockfont.py` /
  `mkfont.py`.

Note also a second, *vestigial* font system: the anti-aliased `fonts.rs`
(`VALUE` ~124 px, `UNIT`, `LABEL`, `SMALL`) is only referenced by `ui::bar`,
`ui::button_row` and `DMM_BUTTONS` (`ui.rs:58`-`85`), none of which are called
by `app.rs`. Dead weight; worth deleting or wiring up, but not a UX issue.

A render-model plus worth keeping: fw-rs already diffs the main readout per
digit (`stockfont.rs` `cells_diff`, `ui.rs:249`/`256`) and per scope column
(`ScopeView::frame`, `ui.rs:742`), where stock blindly redraws the whole rect
every 50 ms (`gui_render_v2_6.h` §2). The UI is flicker-free and cheap; none of
the proposals below should regress that.

---

## 1. Information hierarchy and legibility, per screen

### DMM (full, `View::Dmm`)

What reads well: the 78 px red main value dominates correctly; unit (401,114)
and DC/AC glyph (437,76) sit beside it; the strip chart (80,176,321,97) with
hi/mid/lo labels and the MAX/REL/MIN right column are a clean secondary block.
Hierarchy is: value > chart > history > buttons. Good.

Cramped / wasteful:

- **Status bar dead space.** `status_right` (`ui.rs:384`) places uptime at x=205,
  lock at 335, BT at 390, battery at 435, while the range/HOLD labels end near
  x=186 (`ui.rs:287`-`288`). The gaps 186-205 and 230-335 are empty. That space
  is exactly where a **REC / LOG** indicator and a mode name belong (see §2).
- **REL has no status indicator.** `reading()` takes `rel` and throws it away
  (`ui.rs:289 let _ = rel;`). The only REL cue is the chart-side "REL:" box
  (`ui.rs:531`), drawn only in `View::Dmm`. A user in `DmmScope` reading a
  relative value gets no cue at all.
- History row is a thin 19 px strip (y 153-172); fine to read, thin to hit (§3).

Colour: red value on `BG 0x0883` is strong and legible. **The stock ">=30 V
value turns red" cue is now gone** (the value is always red by requirement). The
>=100 V lightning (`hv_warning`, `ui.rs:379`) survives but is Dmm-only
(`app.rs:758`). Recommend keeping an HV cue that still works: lightning on
`Dmm` + `DmmScope`, and/or a red tint on the status bar above 30 V.

### DMM + Scope combined (`View::DmmScope`)

This is the view being reworked, and today it is half-done:

- The DMM value is **still full 78 px** (`ui.rs:249`, compact branch draws F78 at
  rect(40,36,360,78)), so it occupies y 36-114. The scope is squeezed into
  `SCOPE_COMPACT = {y0:134, h:143}` (`ui.rs:560`) — only 143 px tall, roughly
  half the full scope. The "smaller DMM text + top bar" rework is the fix (§5).
- The compact branch **drops the aux readouts and the HV warning** (`app.rs:758`
  limits `hv` to `View::Dmm`), so AC Hz/duty, 4-20 mA %, diode ohm, temp CJ are
  invisible here even though they are measured.
- **The embedded scope is display-only and has conflicting controls.** In
  `DmmScope`, taps hit only the 6 DMM bottom buttons (`app.rs:1148`-`1161`); the
  scope drag/trig/cursor handlers are `View::Scope`-gated (`app.rs:860`, `873`).
  RUN/AUTO/MF are DMM-bound here (`app.rs:903`-`940`): RUN = DMM HOLD, AUTO = DMM
  autorange, MF = Range page. So the on-screen scope's run/stop, timebase and
  V/div can't be changed without swiping to the full Scope view. The rework must
  decide the control story (tap the scope area to jump to `Scope`, or add a mini
  +/- strip) or the scope half is pure decoration.

### Scope (full, `View::Scope`)

Reads well: yellow trace on dark fill, 10x8 grid, a packed but legible 14 px top
bar (status / timebase / V/div / trigger / gen-state), 4 measurement cells along
the bottom.

Problems:

- **The right-panel buttons lie about what they do.** `SCOPE_RIGHT`
  (`ui.rs:597`) + `labels()` (`ui.rs:663`-`674`) draw a yellow group
  `+ / <V/div value> / -` and a purple group `+ / "Time" / -`, plus `TRIG`. But
  the tap handler (`app.rs:1165`-`1182`) maps the yellow **middle button
  (showing the V/div value) to toggle AC/DC coupling**, and the purple middle
  button (**labelled "Time"**) to **cycle the trigger sweep mode**
  (Auto/Normal/Single). So "the button that shows 500mV" changes coupling, and
  "Time" changes the trigger mode. These are inherited from the stock
  CH1/math/TRIG layout but repurposed without relabelling. High-confusion, easy
  fix (relabel the middle buttons `DC`/`AC` and `Auto`/`Norm`/`Sing`).
- **The trigger level jumps on any graph tap.** `app.rs:1187`-`1189`: a tap
  anywhere in y 20-301 sets `trig_row`. Combined with the 8 px cursor grab
  (§3), a user who taps to inspect, or misses a cursor by >8 px, silently moves
  the trigger. There are also **no on-screen zero or trigger markers** (stock
  draws 12x12 glyphs on the 13 px left strip); the only trigger cue is a 6-column
  orange tick at the left edge (`ui.rs:718`).
- The 13 px left strip (`app.rs:1185`, sets `pos`) is an invisible control: no
  label, no handle.

### Generator (`View::Gen`)

Reads well: three clearly-labelled value boxes (Frequency / Vpp / Duty), a large
waveform preview, RUN/STOP top-left.

Problems:

- **No status at all.** The 1 s status timer updates only `Dmm`/`DmmScope`/`Scope`
  (`app.rs:446`-`461`); the generator page draws no battery, uptime, BT or lock.
  Stock shows uptime + battery here. On a battery handheld, the one page with no
  charge indicator is a real gap.
- The preview (`gen_preview`, `ui.rs:978`) is ~239x231 px and purely
  illustrative — it ignores actual amplitude, duty and frequency. That is a lot
  of prime real estate for a fixed cartoon; shrinking it would let the value
  boxes grow or make room for a status strip.
- The wave box has a drop-down chevron but a tap just **cycles** the four waves
  (`app.rs:1199`); there is no list. Minor, but the affordance misleads.
- Duty greys out for non-square / >50 kHz (`ui.rs:1053`) with no explanation;
  a tap there silently does nothing (`app.rs:1208`-`1212`).

### Menus (Menu / Settings / Range / ScopeMenu / Device / Keypad)

- **Main menu** (`main_menu`, `ui.rs:1143`): four 190x106 tiles + a 5th row
  "Combined view: DMM+Scope". Clear. The combined-view row is the only
  menu-reachable entry to `DmmScope`.
- **Settings** (`ui.rs:1182`): nine rows at 30 px pitch, 27 px tall — tight but
  legible. Values cycle on tap; **backlight and volume step +10 % per tap and
  wrap** (`app.rs:1235`-`1238`), so turning backlight down is up to 9 taps. A
  slider or +/- pair would be far better on a touch screen.
- **Three different top bars.** DMM bar is 34 px / `0x1904` (`ui.rs:26`), scope
  bar 20 px / `0x4208` (`S_BAR`, `ui.rs:544`), generator bar 36 px / `0x2146`
  (`G_BAR`, `ui.rs:964`). Swiping between views makes the top bar jump in height
  and colour, and each carries a different (often smaller) subset of
  battery/lock/time. Because the instruments run concurrently, the global status
  (battery, lock, REC/LOG) should be identical and always present — this is the
  backbone fix (§2, §4).

---

## 2. Navigation model

### What exists

- **Swipe left/right** cycles `View::CYCLE = [Dmm, DmmScope, Scope, Gen]`
  (`ui.rs:55`, handled `app.rs:845`-`850`), but only when already in one of those
  four; swipes are ignored in menus/popups.
- **MENU key** (side key 0): from a main view opens `Menu`; from a menu/popup
  returns to `back_view` (`app.rs:899`-`902`).
- **RUN key** (1): Gen → output on/off, Scope → run/stop, else → DMM HOLD
  (`app.rs:903`-`919`).
- **AUTO key** (2): Scope → auto-set, else → DMM autorange + clear REL/HOLD
  (`app.rs:920`-`928`).
- **MF key** (3): DMM/DmmScope with V/A/Ω/Cap → Range page; Scope → ScopeMenu;
  and the inverse to go back (`app.rs:929`-`940`).
- **Per-view bottom controls**: DMM 6 function buttons; Scope right panel; Gen
  value boxes; DmmScope borrows the DMM buttons.

### Where a user gets lost

1. **Swipe is invisible.** There is no page indicator, arrow or edge hint. A
   user who never swipes never discovers `DmmScope`, `Scope` or `Gen` except via
   the menu, and the cycle order isn't shown anywhere.
2. **Side keys are overloaded and silent.** RUN means HOLD, STOP or output-toggle
   depending on view; AUTO means autorange or auto-set; MF means Range or
   ScopeMenu. Nothing on screen says what the key under the user's thumb will do
   right now.
3. **Drag is misread as a swipe (bug).** `gesture.rs:77` classifies *any* release
   after >=60 px of travel in <800 ms as a swipe, even if `Drag` events were
   already delivered and consumed. So dragging an X-cursor or the trigger level
   60+ px quickly on the scope **switches you to another view mid-drag**
   (`app.rs:845`). This also blocks a clean swipe-to-record until it's fixed.
4. **Two "back" idioms.** MENU key returns to `back_view`; Settings also has a
   "Back" row that goes to Menu (`app.rs:1254`); Device dismisses on *any* tap
   (`app.rs:971`). Inconsistent.
5. **Wake tap acts.** After idle dim, the first tap un-dims *and falls through*
   to the gesture handler (`app.rs:836`-`839`), so a "wake it up" tap can change
   the DMM function or move the trigger.

### Proposed view map

```
                         ┌───────────── MENU key ─────────────┐
                         ▼                                     │
   ┌──────┐  swipe  ┌──────────┐  swipe  ┌───────┐  swipe  ┌──────┐
   │ DMM  │ ◀────▶  │ DMM+Scope│ ◀────▶  │ Scope │ ◀────▶  │ Gen  │   (wraps)
   └──┬───┘         └────┬─────┘         └───┬───┘         └──┬───┘
      │ swipe↓/↑         │ (tap scope→Scope) │ MF              │
      ▼                  │                   ▼                 │
  CSV record         (same page              ScopeMenu         Keypad
  start/stop          dot row)               (tabs)            (freq/Vpp/duty)
      │                                       MENU→back
   MF (V/A/Ω/Cap)
      ▼
   Range page

   MENU (from any main view) ──▶ Main menu ──▶ { DMM, Scope, Gen,
                                                 Settings ──▶ Device,
                                                 Combined view row }
```

Concrete changes to make it discoverable and consistent:

- **Page dots.** A row of 4 dots (current view filled) in the shared top bar,
  always visible on the four cycle views. One small widget; teaches the swipe.
- **Side-key legend.** The four keys are physical and down the right edge. Draw a
  tiny label next to each (`MENU | RUN/HOLD/OUT | AUTO/AUTOSET | MF/RANGE`) that
  updates per view. This removes the single biggest "what does this do here"
  problem for near-zero pixels.
- **Unified top bar.** One height, one colour, carrying battery + charge + BT +
  lock + REC/LOG + page dots on *every* cycle view. The per-instrument info
  (timebase, V/div, DMM mode) sits *below* it. This is what lets REC/LOG/lock be
  visible regardless of which instrument's view is on top — which matters
  precisely because all instruments run at once.

### Where the logger and CSV-record modes live

- **Fast-USB data logger**: not a view. It streams over USBTMC/USB, so it is a
  *mode* toggled from the host (a `DBG:`/SCPI command) and/or a Settings toggle.
  On screen it is just a **`LOG` badge** in the unified bar (plus optional sample
  count), so the user can see the device is streaming no matter which view is up.
- **CSV-to-disk record**: the user wants this started from the DMM screen by a
  swipe. Vertical swipes are currently free (`app.rs:845` handles only
  Left/Right; `SwipeUp`/`SwipeDown` fall through), so **SwipeDown on `Dmm` /
  `DmmScope` = start/stop CSV recording**, with a red `● REC hh:mm:ss` badge in
  the bar and a sample/line counter. Put the file list / stop-all / interval in a
  Settings or menu entry. (A SwipeUp could be reserved for a future quick
  "screenshot to disk".)

  Design constraint from the drag/swipe bug: the record swipe must use a
  **dominance ratio** (`|dy| > 2|dx|`) and must be suppressed once a `Drag` was
  consumed, or it will fire during chart/REL interactions. Fix the gesture
  classifier (§4 item 1) first.

---

## 3. Touch / gesture ergonomics

Tap targets (handheld, ~1 px ≈ 0.17 mm at this DPI; aim for >=40 px / ~7 mm):

| Control | Size | Verdict |
|---|---|---|
| DMM function buttons (`dmm_button`, `ui.rs:173`) | 76x40 | good |
| Range-page buttons (`range_rect`, `ui.rs:314`) | 112x40 | good |
| Keypad keys (`keypad_keys`, `ui.rs:1068`) | 60x40 (H/K/M 56) | good |
| Scope right panel (`SCOPE_RIGHT`, `ui.rs:597`) | 60x40 | ok width, but +/- are single glyphs and the middle buttons mislead (§1) |
| ScopeMenu tabs / items (`scope_menu`, `ui.rs:900`) | 95x28 / 75x34 | ok |
| DMM REL box (`app.rs:947`) | 76x38, far corner | ok |
| History row tap (`app.rs:959`) | full width x **19 px** | thin; a mis-tap hits buttons/chart |
| **Scope cursor grab** (`app.rs:865`-`870`) | **±8 px** | too tight for a finger |
| Scope left strip (`app.rs:1185`) | 13 px, unlabelled | hard to find, easy to hit by accident |

Gesture issues:

- **Cursor grab is ±8 px** — under 1.5 mm. Cursors have no visible handles and no
  zero/trigger markers accompany them. Widen to ~20 px and draw grab handles.
- **Trigger moves on any graph tap** (`app.rs:1187`). Restrict trigger-set to a
  dedicated draggable handle (left edge or a marker), not the whole graph.
- **Only RUN/AUTO/MF/Tap give feedback; press-down doesn't.** `Gesture::Down`
  exists (`gesture.rs:17`) but `app.rs` uses it only for cursor grab
  (`app.rs:860`). A pressed-state highlight on buttons/tiles is a quick win.
- **Long-press** is used once (DMM history clear, `app.rs:855`). Fine; room to
  add e.g. long-press a function button → its Range page.
- **Generator keypad traps**: it opens empty with `kp_mult = b'K'`
  (`app.rs:529`) regardless of the field's current unit, so typing "500" for a
  2 MHz field yields 500 kHz; and changing the wave re-snaps silently (square
  2 MHz → sine = 0 Hz, `app.rs:1216` → `gen.rs:86`). Pre-fill the keypad with the
  current value and unit, and warn/round visibly on a snap to 0.
- The RUN/STOP label on the generator isn't tappable (no y<36 case in
  `app.rs:1192`-`1215`); only the RUN key toggles output. Make the label a
  button.

REL / HOLD / range controls specifically:

- HOLD, autorange and REL are **keys only** on DMM; REL is also a corner touch
  box. HOLD has a clear "HOLD" label in the bar; **REL has no bar indicator at
  all** (§1). Add a `REL` badge to the unified bar.
- The Range page is good (big buttons, DC/AC, OK). The quibble is reaching it:
  MF key or... there is no on-screen hint that MF opens it.

---

## 4. Prioritised punch-list

Each: what / why / effort (S/M/L) / stock deviation. Quick wins marked ★.

1. **Fix drag-misread-as-swipe** — `gesture.rs:77` fires a swipe after any >=60 px
   drag; dragging a scope cursor/trigger switches view mid-gesture. Suppress
   swipe once a `Drag` was consumed (app returns "handled"), and require a
   dominance ratio. *Why:* correctness bug and a prerequisite for swipe-to-record.
   **S. Fixes fw-rs behaviour (gestures are fw-rs-only).**
2. ★ **Unified top bar on all four cycle views** — one height/colour carrying
   battery, charge, BT, **lock**, REC/LOG, page dots. Today lock is invisible on
   Scope/Gen and Gen shows no status, so a locked device on the scope page looks
   broken (`app.rs:446`-`461`, `ui.rs:384`/`403`). *Why:* concurrent instruments
   need one global status; biggest consistency win. **M. Deviates (simplifies
   the three stock bars).**
3. ★ **Swallow the wake tap** — after idle-dim, the first input should only
   un-dim, not act (`app.rs:836`-`839`). *Why:* prevents accidental function/
   trigger changes. **S. fw-rs behaviour.**
4. ★ **Relabel the scope middle buttons** — show `DC`/`AC` on the yellow middle
   button (it toggles coupling) and `Auto`/`Norm`/`Sing` on the purple one (it
   cycles trigger mode), instead of the V/div value and "Time"
   (`ui.rs:663`-`674` vs `app.rs:1165`-`1182`). *Why:* the labels currently lie.
   **S. Deviates (clearer than stock).**
5. ★ **Page dots + side-key legend** — teach the swipe cycle and show what
   RUN/AUTO/MF do in the current view. *Why:* the two biggest discoverability
   gaps. **S-M. New (stock had neither).**
6. **DMM swipe-to-record (CSV) + REC badge** — SwipeDown on Dmm/DmmScope toggles
   CSV recording to the SPI disk; `● REC` badge + counter in the bar. *Why:* the
   requested feature; vertical swipes are free. **M (UI part S; needs a logger
   module). New.**
7. **Rework DMM+Scope** — compact DMM reading (30 px `LETTERS` or a half-scale
   F78) in a one-line strip, scope spanning ~y40-277; and give the embedded scope
   real controls (tap-to-Scope or a mini +/- strip) and restore aux/HV
   (`ui.rs:249`/`560`, `app.rs:758`/`1148`). *Why:* the stated rework; today the
   scope gets half the screen and can't be driven. **M. Deviates (intended).**
8. **Trigger handle + markers, wider cursor grab** — stop trigger-on-any-tap
   (`app.rs:1187`), draw zero/trigger markers on the left strip, widen cursor grab
   to ~20 px with visible handles (`app.rs:865`). *Why:* error-prone precision on
   a finger screen. **M. Deviates (stock had markers; fw-rs dropped them).**
9. ★ **Generator status + tappable RUN/STOP** — draw battery/uptime/lock on the
   gen page; make the RUN/STOP label a button (`app.rs:1192`, `ui.rs:1034`).
   *Why:* only page with no charge indicator; output toggle hidden behind a key.
   **S. Restores stock status; tappable label is new.**
10. **Generator keypad prefill + snap feedback** — open the keypad with the
    current value/unit (`app.rs:529`), and show when a wave change or entry snaps
    to 0 Hz (`gen.rs:86`). *Why:* silent unit traps. **S. Clearer than stock.**
11. ★ **Pressed-state feedback** — highlight buttons/tiles on `Gesture::Down`
    (`gesture.rs:17` already emits it). *Why:* taps feel dead today. **S. New.**
12. **Settings: slider / +/- for backlight & volume** — stop the wrap-around
    +10 %/tap (`app.rs:1235`-`1238`). *Why:* 9 taps to dim. **S-M. Deviates.**
13. **HV cue that survives the always-red value** — keep the >=100 V lightning on
    Dmm *and* DmmScope, and consider a >=30 V bar tint, since the stock
    red-from-30 V value cue is gone (`app.rs:758`). *Why:* safety cue lost. **S.**
14. ★ **Shrink the generator preview** — free space for status/values; it's a
    fixed cartoon today (`ui.rs:978`). *Why:* wasted prime real estate. **S.**
15. **Housekeeping (not UX, but it bites implementers):** `app.rs:1050`-`1123`
    has duplicate, unreachable `View::Keypad` and `View::ScopeMenu` match arms
    (identical to `:976`-`1049`) — edit both or delete the dead copy; and
    `ui::bar`/`button_row`/`DMM_BUTTONS` + the `fonts.rs` VALUE/UNIT/LABEL/SMALL
    set are dead. **S.**

Suggested order to ship: 1, 3, 4, 2, 5, 11 (all quick, high-impact) → 9, 10, 13,
14 → 6, 7, 8 → 12, 15.

**Status 2026-10-21** (fw-rs/TODO.md has the commits): done 1, 2, 3, 5, 6, 9, 10, 12, 13;
4 / 7 / 8 superseded by the stock scope port; 11 keypads only; 14 won't do (stock
geometry); 15 duplicate arms gone, dead `ui::bar` / `button_row` / `DMM_BUTTONS` /
fonts.rs deleted 2026-10-21 (with text.rs, mkfont.py, Roboto). The line numbers above are from the review date and stale.

---

## 5. Layout sketches

### 5a. Reworked DMM + Scope combined

Goal: DMM value in a compact top strip, scope filling the rest, both drivable.

```
 x:0                                                              479
  ┌───────────────────────────────────────────────────────────────┐ y0
  │ [bat⚡][BT][🔒][●REC]      ● ○ ○ ○            12:34:56           │ unified bar (22px)
  ├───────────────────────────────────────────────────────────────┤ 22
  │  12.345 V≈  DC   Hz 1.0k   AUTO  HOLD           REL  ▲100V      │ compact DMM line
  │  └30px LETTERS┘  └aux (restored)┘  └status┘                     │  (28px, tap→full DMM)
  ├───────────────────────────────────────────────────────────────┤ 52
  │▕                                                             ▏+ │  scope: y≈56..300
  │▕                 ╱╲      ╱╲      ╱╲                          ▏DC │  (≈244px, was 143)
  │▕ ───────────╳──────────────────────────────── trig ─────────▏- │  right strip = same
  │▕              ╲╱      ╲╱      ╲╱                             ▏+ │  controls as full
  │▕                                                             ▏Au │  scope, 60x40
  │▕  AUTO  200us  1V  /0.00V                                    ▏- │
  │▕_____________________________________________________________▏T │
  ├───────────────────────────────────────────────────────────────┤ 300
  │   V  │  A  │  Ω  │ Cap │Diode│Hz/°C     (DMM function buttons)   │ 280/40
  └───────────────────────────────────────────────────────────────┘ 319
```

Notes: tapping the compact DMM line jumps to the full DMM view; tapping the
scope area (or its right strip) drives the scope; RUN/AUTO/MF act on whichever
half last had focus, shown by the side-key legend. Aux readouts and the HV arrow
return to the compact line.

### 5b. DMM screen with a record indicator

Only the bar changes; the body is today's DMM. SwipeDown toggles REC.

```
  ┌───────────────────────────────────────────────────────────────┐
  │ AUTO  HOLD   [bat⚡][BT][🔒]   ● REC 00:04:17 · 2571     12:34:56│ ← REC badge
  ├───────────────────────────────────────────────────────────────┤   (red dot, elapsed,
  │                                                                 │    line count) lives
  │                 1 2 . 3 4 5   mV                                │    in the 186-335 gap
  │                               DC                                │
  │  ┌──┐┌──┐┌──┐┌──┐┌──┐  (history)                                │
  │ hi│  chart ·····························  │ MAX:                 │
  │ md│            ╱╲____                     │ ┌REL:┐              │
  │ lo│  ·········/······\·················   │ └────┘ MIN:         │
  │   │[320mS/div]                            │                     │
  ├───────────────────────────────────────────────────────────────┤
  │   V  │  A  │  Ω  │ Cap │Diode│Hz/°C                             │
  └───────────────────────────────────────────────────────────────┘
       ▲ swipe ⟷ cycles views · swipe ↓ start/stop CSV record ·
         swipe (none) while a drag/REL touch is in progress
```

When not recording the badge is absent (the gap stays empty). When the fast-USB
logger is streaming, a separate `LOG` badge sits in the same region on every
view via the unified bar.

---

## Summary — top recommendations

1. Fix the drag-misread-as-swipe bug (`gesture.rs:77`): a scope cursor/trigger
   drag currently switches views; it also blocks a clean swipe-to-record.
2. Give every cycle view one unified top bar carrying battery, charge, BT, lock,
   REC/LOG and page dots — because all instruments run at once, global status
   must not depend on which view is up (lock is invisible on Scope/Gen today).
3. Swallow the wake tap after idle-dim so it can't change the DMM function or
   trigger (`app.rs:836`).
4. Relabel the scope right-panel middle buttons: they actually toggle coupling
   and cycle the trigger mode, not what their "500mV"/"Time" labels say.
5. Add page dots + a per-view side-key legend; the swipe cycle and the
   RUN/AUTO/MF overloading are undiscoverable today.
6. Put CSV record on SwipeDown from the DMM/DmmScope screen with a red REC badge
   in the bar; keep the fast-USB logger as a host-toggled mode with a LOG badge.
7. Finish the DMM+Scope rework: a compact DMM strip (30 px LETTERS or half-scale
   F78 — nothing exists between 32 and 78 px) frees ~100 px for the scope, which
   also needs real controls and the aux/HV readouts restored.
8. Make the scope safer to touch: a dedicated trigger handle (not trigger-on-any-
   tap), zero/trigger markers, and ~20 px cursor grab with handles.
9. Give the generator page status (battery/uptime/lock), a tappable RUN/STOP
   label, and a keypad pre-filled with the current value/unit.
10. Quick wins: pressed-state button feedback, a REL badge, keep the HV cue alive
    now the value is always red, slider/+/- for backlight & volume, and shrink the
    decorative generator preview.
