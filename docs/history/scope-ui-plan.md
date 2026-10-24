# fw-rs oscilloscope UI: problems, target design, implementation plan

This is a research and design document only. No source was changed. It covers
the scope UI of fw-rs on the DM40: a 480x320 landscape capacitive panel, 4 touch
side keys (MENU / RUN / AUTO / MF) and one physical power key. The scope is
single-channel, 10-bit, 50 MSa/s. The DMM, scope and generator all run at once.
The views are DMM, DMM+Scope, Scope and Generator.

The user has said fw-rs does not have to copy the stock scope UI 1:1, so this
plan sets out to improve on it. It must not lose the one scope feature owners
praise: **finger control of the scope** (dragging the trigger, zero and FFT,
quick timebase taps).

Inputs read:
- `docs/user-feedback.md` (§2 P7/P8/P9/P13/P18/P20, §6, §8, §9);
- `docs/ui-review.md`, `docs/parity.md` §2, `TODO.md`;
- `../firmware/re/analysis/scope_gen_ui_v2_6.h`, `scope_gen_v2_6.h`,
  `touch_v2_6.h`, `gui_v2_6.h` (stock behaviour);
- source: `src/scope.rs`, `src/scope_meas.rs`, `src/ui.rs` (`ScopeView`,
  `scope_menu`), `src/app.rs` (`gesture`, `tap`, `key`, `autoset_*`,
  `scope_poll`, `show`), `src/gesture.rs`, `src/touch.rs`, `src/main.rs`
  (touch poll loop), `src/persist.rs`.

The working tree is dirty, so citations name functions rather than line numbers.

---

## 1. Problem list (merged, deduplicated)

Severity:
- **H** = wrong behaviour, an accidental change of a setting, or a feature that
  cannot be reached.
- **M** = frequent friction.
- **L** = polish.

Sources:
- `UF` = user-feedback item (Telegram message ids `#nnnnn`).
- `R#` = ui-review punch-list item or section.
- `N` = new finding from this review (verified in the source).

### 1.1 Controls that move the wrong thing, or move things by accident

| # | Problem | Sources | Sev |
|---|---|---|---|
| P1 | **Dragging is misread as a swipe.** A drag of 60 px or more that ends within 800 ms is classified as a swipe on release (`gesture.rs` `Tracker::event`), even though `Drag` events were already consumed. A quick cursor or trigger drag on the Scope page therefore **switches to another view**. | R §2.3, R#1; related to UF-05 #44587 | H |
| P2 | **The trigger level jumps on any tap or drag in the graph.** `app.rs` `tap()` sets `trig_row = y` for a tap anywhere in y 20..300. `gesture()` `Drag` with no cursor grabbed sets `trig_row = p.y` as an absolute value, so the level jumps to the finger, and horizontal drags move it as well. | R §1 Scope, R#8, R §3 | H |
| P3 | **Trace offset and trigger level share one control spot.** "тыкать несколько раз и присматриваться что ты выбрал это боль". A rival scope puts the two controls on opposite sides. In stock, the graph drag moves whichever item is "selected", so it is ambiguous. In fw-rs the offset has only an invisible 13 px tap strip. | #58939, #59147; R#8; stock `scope_gen_ui_v2_6.h` §6 "Touch on the graph" | H |
| P4 | **A horizontal drag also nudges the vertical value** ("двигается немного и верх-низ"). There is no axis lock. | UF-05 #44587 | H |
| P5 | **N: tapping the left strip sets a mirrored offset.** `tap()` does `st.pos = 140 - (y-20)`. `Scope::row_of_volts` puts 0 V at row `140 + pos`, so a tap near the top puts 0 V near the **bottom**. Verify on the device; the code arithmetic is unambiguous. | N (`app.rs` `tap`, `scope.rs` `row_of_volts`) | H |
| P6 | **N: changing the offset changes the trigger voltage.** `Scope::apply` keeps `trig_row` (a screen row) when `pos` changes, and `trig_volts()` depends on `pos`. Moving the trace up or down can therefore lose the trigger. Stock moves the level row by the same delta, so the trigger voltage is kept (`FUN_080225c2`, header §1 "Position"). | N | M |
| P7 | **A wake tap acts.** After idle-dim, the first touch both un-dims the screen and runs the gesture. On the scope page this can move the trigger. | R §2.5, R#3 | M |
| P8 | **N: every drag event restarts the acquisition and redraws the whole graph.** `scope_set` → `Scope::apply` → `halt()` + `scope_invalidate()` runs on every 10 ms `Drag`. At timebases with records of 100 ms or more, no frame completes until the finger lifts. Each event also costs a full 401-column repaint. | N (`app.rs` `scope_set`, `scope.rs` `apply`) | M |

### 1.2 Things that cannot be seen or reached

| # | Problem | Sources | Sev |
|---|---|---|---|
| P9 | **No zero (offset) marker and no trigger-level marker.** The only cue is a 6-column orange tick at the left edge (`ScopeView::column_t`). Stock draws 12x12 markers on the left strip. | R §1, R#8; stock §6 "Markers" | H |
| P10 | **No trigger-position setting, and no "centre X/Y".** fw-rs fixes the trigger at mid-screen (`Scope::finish`: `half = screen/2`). Stock has `S+0x68` (drag, "T" marker on top, record bar). | UF-09 #44586; TODO "horizontal pan not done"; stock §2/§6 | H |
| P11 | **Cursors are buried.** In stock they sit deep in a menu. In fw-rs they only turn on as a side effect of selecting the measurement items X2-1 / Xf / Y2-1 (`cursors_from_selection`). The grab zone is ±8 px with no handles or labels. | UF-08 #17520, #17517, #58939; R §3, R#8; #48875/#54966 (undiscoverable) | H |
| P12 | **The right-panel buttons lie.** The yellow middle button shows the V/div value but toggles AC/DC. The purple middle button reads "Time" but cycles the trigger mode. TRIG toggles the slope. | R §1 Scope, R#4 | H |
| P13 | **V/div "+" at the top looks inverted.** In stock, "+" raises V/div, so the trace gets smaller. fw-rs maps "+" to a smaller V/div, the opposite of stock, but the button still shows a bare "+". The bare +/- glyphs are ambiguous for both axes. | UF-10 #26398, #26405; R §3 | M |
| P14 | **The timebase is hard to find, and stepping is round-robin through a tiny box.** Owners praise the stock-style quick taps (tap the top-left/right corner to step the timebase). fw-rs has neither corner taps nor a list picker. | UF-07 #26399, #26921; praise P7 #15791, #16558, #44584 | M |
| P15 | **Long-press list pickers for timebase and V/div are missing.** Only ±1 steps exist (30 timebases, 10 ranges). | UF-07 | L |
| P16 | **FFT is missing.** On stock, FFT has no numeric axis, the peak frequency is found only by dragging it to the centre, the drag is over-sensitive, and the display blanks while adjusting. Owners praise FFT window choice and FFT cursors. | UF-11 #15794, #15948, #51067, #16554, #44584; parity §2 "Math / FFT MISSING" | M |
| P17 | **The probe list has only x1/x10/x100.** Stock has x0.1..x1000 (13 values), and owners value that range. | UF-12 #44584; parity §2 | L |
| P18 | **The scope half of DMM+Scope is display-only.** Its taps only reach the 6 DMM buttons, and RUN/AUTO/MF are bound to the DMM. Without `analog_both`, `show()` powers only the DMM side (`scope.power(false)`) in `View::DmmScope`, so the scope front end is unpowered there. | R §1 DMM+Scope, R#7; TODO "both rails on automatically in the DMM+Scope combined view"; #29158/#29139/#29174 (ripple p-p in DC V) | H |
| P19 | **Swiping between views is invisible.** There are no page dots, and the side keys mean different things per view without saying so. | R §2.1-2, R#5 | M |
| P20 | **Three different top bars.** Lock, BT and battery are not visible on the Scope page. | R §1, R#2 | M |
| P21 | **No pressed-state feedback.** `Gesture::Down` is only used for the cursor grab. | R §3, R#11; #26950, #26952 (buttons too small, want feedback) | L |

### 1.3 Behaviour bugs and functional gaps behind the UI

| # | Problem | Sources | Sev |
|---|---|---|---|
| P22 | **AUTO switches AC coupling back to DC.** fw-rs copies this: `autoset_start` sets `st.ac = false`, and the success path keeps it. | UF-04 #44566, #44570, #44573, #44575 | H |
| P23 | **N: the trigger is software-only, with an 800-sample search window.** fw-rs never ports the stock hardware comparator trigger (DAC2 on PA5 → comparator → PA15 TIM2_CH1, TIM5 pre-count; `scope_gen_v2_6.h` "Trigger"). It searches `WIN = 800` samples after `screen/2` in each record. From 100 µs/div a screen is 25 000 samples, so a slow periodic signal often has no edge in the window. The likely results are untriggered Auto frames and a sticky WAIT in Normal. This needs measuring (see S9). `parity.md` lists the trigger as MATCH, which covers the software refinement only. | N (`scope.rs` `finish`, `find_trigger`) | M |
| P24 | **N: the Single status label is wrong.** While armed and waiting it shows "T'D" (`ScopeView::labels`: `Single => "T'D"`). It should show WAIT/ARM. | N | L |
| P25 | Persistence offers only OFF and INF; stock has OFF/MIN/1S/INF. | N; stock §6 | L |
| P26 | Smooth trace at fast timebases: the fw-rs trace is linear, stock uses sinc. | UF-41 #59901, #59909 (praise P20) | L |
| P27 | Saving a waveform as CSV, and advanced triggers (pulse width, runt). | UF-13 #50392, #50395 | L |
| P28 | **Screenshot gesture (3-finger swipe down) and 2-finger power-off are missing.** Both are praised stock gestures, and both must coexist with the scope's drags. | UF-17 #16839, #17435, #19950, #50265; UF-18 #15928, #16743, #44653 | M |
| P29 | Touch is "fiddly" and the buttons small for some owners. Others praise one-handed light taps. A passive stylus barely registers. | #15801, #38175, #44652, #26950, #54968, #54975, #59921; UF-31 #50846, #54986 | M |

### 1.4 Context items (no UI action; kept so nothing is lost)

- **Scope UI judged "сделали на отвали" / "софт сырой".** Owners judge the scope
  by its UI, not its 10 MHz hardware. (#44663, #59147, #38124)
- **Feels sluggish on a newer hardware revision.** Keep the fw-rs frame rate at
  or above stock. (#49740)
- **A colour choice recolours the whole UI.** fw-rs has a fixed palette, so the
  problem does not arise. An optional trace-colour setting is P2, not filed.
  (#44588, #44597)
- **Cursor timing agrees with a DHO814, but voltage reads high vs a DHO802.**
  This is the probe/front end, not the UI; keep the stock cal. (#17512, #16009,
  #38222)
- **V1.7–V2.1 fixed the trigger position at some timebases.** fw-rs follows V2.6.
  (#15892, #18850)
- **The generator runs independently and the scope can watch it.** Keep this:
  the generator state chip stays in the scope bar. (#50397, #61569, #59900)
- **Hardware limits** (§6.3: −3 dB at 11.5–17 MHz, single channel, 30–35 pF
  input). (#15789, #16738, #38120, #15900, #44582, #50216, #30339, #43276,
  #28400, #15801)
- **Battery: the scope front end should be powered on demand.** This interacts
  with P18. (UF-19 #16619, #25610, #54285, #61298, #61311)
- **Touch near a strong HF source stops responding.** The touch code must not
  latch a stuck state. (#29263)
- **The stock disk fills after about 28 screenshots.** Write compact BMPs.
  (UF-17 note #16839, #50880)

---

## 2. Target interaction model

### 2.1 Principles

1. **Every draggable value has a visible marker and its own region of the
   graph.**
   - Vertical drags on the **left and middle** of the graph move the trace
     offset.
   - Vertical drags on the **right edge** move the trigger level.
   - Horizontal drags anywhere move the trigger position (or pan the stored
     record when stopped).

   This answers #58939/#59147 ("opposite sides"), as many small touch scopes
   do: zero marker on the left edge, trigger handle on the right edge,
   trigger-position marker on top.
2. **Drags are relative and axis-locked.** A drag adds `Δ` to the value it
   grabbed, so a touch never makes a value jump to the finger. After 12 px of
   travel the gesture locks to the dominant axis for the rest of the drag
   (UF-05).
3. **A single tap in the graph never changes a setting.** It is the safest
   default and kills P2 and P7. The few tap actions in the graph are
   **double-taps** (reset to a sensible value) and the optional **corner
   taps** (§2.4).
4. **The owner of a touch is decided at touch-down.** A touch that starts in the
   graph belongs to the scope, and the gesture tracker is told so (`capture`).
   Such a touch can never become a view swipe. View swipes start outside the
   graph (§2.6).
5. **Every control shows its state, and the label says what a tap does.**
   Coupling shows "DC"/"AC", the trigger button shows its mode and edge, and
   CUR shows "X"/"Y"/"XY". No button's label may disagree with its action.
6. **Popups instead of deep menus for frequent settings.** Each top-bar readout
   is a "chip": tap it to open a small popup with 40 px rows. MF still opens
   the full settings window.

### 2.2 Per-element controls (Scope view)

| Element | Primary | Secondary | Notes |
|---|---|---|---|
| **V/div** | Right-panel **V⊕ / V⊖** (magnifier icons: ⊕ = zoom in = smaller V/div = bigger trace). | Tap the V/div chip → CH1 popup: a 1-2-5 V/div list, DC/AC, probe x0.1..x1000. Long-press V⊕/V⊖ → same popup. | UF-10, UF-12, UF-07. Changing V/div keeps the trigger **voltage** and clamps it to the screen (option; stock keeps the row). |
| **Time/div** | Right-panel **t⊕ / t⊖** (⊕ = faster = more detail). | Tap the timebase chip → list picker (100 ns..50 s, roll range marked). Corner taps (§2.4). | UF-07, P14/P15. While stopped, this zooms the stored record (existing stop-and-zoom). |
| **Horizontal / trigger position** | **Horizontal drag anywhere in the graph** (axis-locked). Running: moves the trigger point 0..100 % of the screen (pre-trigger). Stopped: pans over the stored record. | **Double-tap the graph body** → trigger at 50 % (centre). Trigger popup: "Position: centre". | UF-09, P10. The "T▼" marker on the graph's top edge shows the position; it becomes an arrow at the edge when the trigger is off-screen in a stopped pan. A record bar in the top bar shows the visible part of the record (as stock). |
| **Vertical offset** | **Vertical drag in the left band (x < 53) or the graph body.** Shown by a yellow "1▶" marker in the left gutter at the 0 V row. | **Double-tap the left band** → offset 0 (0 V at centre). | P3, P5, P9. The trigger row moves with the offset so the trigger voltage is kept (P6). |
| **Trigger level** | **Vertical drag that starts in the right band** (the last 40 graph columns, x 373..413). Shown by an orange "◀" marker on the right edge at the level row. A dashed level line and a live "T 0.52 V" readout appear while dragging and stay 1 s after release. | **Double-tap the right band** → level at 50 % of the last frame's (Vmax+Vmin)/2. Trigger popup: "Level 50 %". | P2, P3, P9. The marker becomes an arrow at the top or bottom when the level is off-screen. |
| **Coupling** | Right-panel **DC/AC** button (label = current state, tap toggles). | CH1 popup. | P12, UF-04. AUTO keeps it (§4.1). |
| **Trigger mode / slope** | Right-panel **TRIG** button, labelled with the current state ("Auto ⟋"). Tap → trigger popup: Auto / Normal / Single, Rising / Falling, Level 50 %, Position centre. | The trigger chip in the top bar opens the same popup. RUN long-press = Single. | P12, P24. |
| **Run / Stop / Single** | Side key **RUN**: short = Run/Stop. | **RUN long-press (≥ 700 ms) = arm Single.** Tap the RUN-state chip = Run/Stop. | The status chip reads RUN / WAIT / ARM / T'D / STOP / ROLL (P24). |
| **Auto-set** | Side key **AUTO**: short = auto-set (keeps coupling, probe and slope; trigger position to 50 %). | **AUTO long-press = undo the last auto-set** (restore the saved settings). | UF-04, P22. |
| **Cursors** | Right-panel **CUR** button cycles Off → X (time) → Y (volts) → XY. The label shows the state. | Long-press CUR → cursor popup (track mode, reset positions, readout choice). Cursors are independent of the measurement selection. | UF-08, P11. Grab zone ±20 px; labelled tabs "a"/"b" (§2.3). |
| **Measurements** | Bottom bar: 4 cells. **Tap a cell → item picker for that slot.** | MF → Measure tab (as now). | Fewer steps than MF → tab. With cursors on, the bar shows the cursor readouts instead (§4.6). |
| **FFT** | Graph long-press quick menu → **FFT on/off**. MF → "FFT" tab (window, dB/div, ref level, span). | The mode chip in the top bar ("YT"/"FFT"/"ROLL") toggles YT ↔ FFT. | UF-11, P16. In FFT mode, horizontal drag moves the FFT marker and vertical drag moves the reference level (§4.7). |
| **Persistence** | Long-press quick menu → Persist OFF / 0.5 s / 1 s / INF. | MF → Display tab. | P25. Cleared on STOP and off in roll (stock), and also on V/div, time and offset changes (new). |
| **Roll** | Automatic from 200 ms/div with Roll on (existing). | MF → Display tab. The mode chip shows "ROLL". | The trigger handle and the T marker are hidden in roll mode. |
| **Quick menu** | **Long-press (700 ms, no movement) on the graph body**: FFT, Persist, Cursors, Centre all (T-pos 50 %, offset 0, level 50 %), Save CSV, Screenshot, Help. | — | A discoverable home for everything else. The 700 ms threshold already exists (`LONG_MS`). |

### 2.3 Cursor handling

- **Cursors are drawn with a labelled tab at the graph edge.**
  - X cursors: a dotted vertical line with a tab "a"/"b" on the graph's
    **bottom** edge.
  - Y cursors: a dotted horizontal line with a tab "a"/"b" on the graph's
    **left inner** edge, just right of the offset gutter.
- **Grab rule:** at touch-down, every cursor line within ±20 px along its own
  axis is a *candidate*. When the axis lock resolves after 12 px:
  - horizontal lock + X-cursor candidate → move that cursor (nearest wins);
  - vertical lock + Y-cursor candidate → move that cursor;
  - otherwise the normal body or band action applies.

  A finger placed near an X cursor can therefore still drag the offset
  vertically, and vice versa. This replaces the ±8 px absolute grab (P11).
- **Starting positions:** first switch-on places X at 25 % / 75 % of the screen
  and Y at ±2 div. The "reset positions" item restores them.
- **Track mode (optional):** "a" and "b" move together, keeping Δ.
- **In FFT mode:** X cursors read in Hz and Y cursors in dBV.

### 2.4 Optional stock-style corner taps (praise P7, UF-07)

- **The zones:** the graph's top-left and top-right 60x40 px corners. A
  **single tap** there steps the timebase slower (left) or faster (right),
  exactly like the praised stock behaviour.
- **Hint:** a faint "‹ ›" in the corners shows the zones exist.
- **Exceptions to other gestures:** the corners are excluded from double-tap,
  and a drag that starts in a corner is a normal horizontal or vertical drag.
- **Setting:** "Corner taps: ON/OFF" in Settings, default ON. This is the one
  exception to "taps in the graph never change a setting". It changes a
  visible, labelled value, and a step can be undone with the opposite corner.

### 2.5 Side keys on the scope page

| Key | Short | Long (≥ 700 ms) |
|---|---|---|
| MENU | Main menu. If a popup is open: close it (back). | — (reserved) |
| RUN | Run / Stop | Arm **Single** |
| AUTO | Auto-set (keeps coupling, probe, slope) | **Undo** the last auto-set |
| MF | Scope settings window (tabs Measure / Display / Trigger / Channel / FFT); MF again closes it | Help page for this view (UF-29) |

Keys with a long action must fire the short action on **KeyUp** when the key
was held for less than 700 ms. This adds a few tens of ms to RUN, AUTO and MF.
Keys without a long action keep acting on KeyDown. The 50 ms key beep stays on
KeyDown.

**Combined view (DMM+Scope).** The side keys keep their DMM meanings (RUN =
HOLD, AUTO = autorange, MF = Range page). The DMM is the primary instrument
there, and a hidden "focus" mode would bring back the "what does this key do"
problem (R §2.2). The scope half gets its own on-screen controls (§3.2). The
ui-review side-key legend shows the current meaning on every view.

### 2.6 Coexistence with view swipes and the global gestures

| Gesture | Starts where | Result |
|---|---|---|
| 1-finger tap | button / chip / meas cell | that action. On Down: pressed highlight. Fires on release. |
| 1-finger tap | graph body / bands | nothing (corner zones: timebase step) |
| 1-finger double-tap (≤ 350 ms, ≤ 24 px apart) | left band / body / right band | offset 0 / trigger position 50 % / level 50 % |
| 1-finger long-press | graph body | quick menu |
| 1-finger long-press | V⊕/V⊖, t⊕/t⊖, chips, CUR | list picker or popup |
| 1-finger drag | inside the graph | **captured by the scope**: offset / level / position / cursor, axis-locked. **Never a swipe.** |
| 1-finger swipe left/right (≥ 60 px, ≤ 800 ms, \|dx\| > 2\|dy\|) | **outside the graph**: top bar, measurement bar, right panel (a Down on a button that then moves > 15 px cancels the button) | previous / next view |
| 1-finger swipe down | DMM area of Dmm / DmmScope only | CSV record start/stop (ui-review §2, planned) |
| **2-finger swipe down** (≥ 60 px, vertical-dominant) | anywhere | **power-off confirm** dialog ("Power off? [Cancel] [Off]", auto-cancel 5 s). Never an instant off. (UF-18) |
| **3-finger swipe down** (≥ 30 px, the stock threshold) | anywhere | **screenshot** to the disk + toast (UF-17) |
| 2-finger swipe left/right (optional, setting) | anywhere, incl. the graph | previous / next view (an escape hatch, since 1-finger swipes do not work inside the graph) |

**How multi-touch must be handled.** The fingers of a 2- or 3-finger gesture
rarely land in the same 10 ms frame. The first finger is first seen as a
1-finger Down and may already have produced `Drag` events. The rules:

1. At Down, the scope saves a **snapshot** of the settings it may change
   (`Settings`, cursors).
2. When `points ≥ 2` appears during a touch, the tracker emits `Cancel`. The
   app **restores the snapshot**, drops any pending tap, long-press or swipe,
   and ignores the rest of the touch except for multi-finger classification.
3. On release, the tracker classifies the gesture by the **maximum** point count
   seen and the travel of the centroid (or of point 0). It requires
   vertical-down dominance. Stock checks that all three points moved down by
   at least 30 px and more than sideways (`gui_v2_6.h` "Screenshot").

A 2-finger gesture therefore never leaves the trigger or offset moved.

**What the touch controllers report:**
- **Hynitron CST2xx at 0x5A (this unit).** `rx[5] & 7` is the point count. Up
  to 5 slots: record 0 plus 4 extra 5-byte records at D0 07 / 0C / 11 / 16
  (`touch_v2_6.h`; stock queue entry holds 5 points).
- **fw-rs `touch.rs read_hyn()`** already reads the extra records but drops
  them. `Event` carries only `points` (the count) and point 0.
- **Goodix GT1158 path (untested here).** Bits 3..0 of status 0x814E hold the
  count. Points are 8 bytes apart from 0x8150.

So 2- and 3-finger detection is possible with today's data: the count plus
point 0. Passing all points (or a centroid) is better, because point 0 can
change identity when a finger lifts. Multi-touch tracking quality on this panel
is **unverified**.

**Pinch-zoom is not proposed.** Reasons:
- the panel's multi-point tracking is untested;
- polling is 10 ms over bit-banged I2C;
- 2-finger motion is reserved for power-off;
- the scope already has ⊕/⊖ buttons and pickers.

If it is ever tried, do it behind a setting, only while STOPPED, and only with
horizontal-dominant motion.

### 2.7 Touch-target sizes

The scale is about 0.17 mm per px; the aim is at least 40 px (≈ 7 mm).

| Target | Visual | Hit zone |
|---|---|---|
| Right-panel buttons | 60x40 (now 60x40, keep) | 66x41 incl. gaps |
| Chips (top bar) | 20 px tall bar | bar + 8 px into the graph top (y 0..27), tap only |
| Measurement cells | 98x19 | 100x22 (y 298..319), tap only |
| Offset marker | 12x12 in the 13 px gutter | band x 0..52, full graph height |
| Trigger marker | 8x12 on the right graph edge | band x 373..413, full graph height |
| T-position marker | 12x8 on the top graph edge | whole graph (horizontal drag) |
| Cursor lines | 1 px dotted + 12x12 tab | ±20 px along the axis |
| Corner timebase zones | faint "‹ ›" | 60x40 |
| Popup rows | 40 px tall, ≥ 120 px wide | same |
| Long-press | 700 ms, ≤ 15 px slop (existing `LONG_MS`/`SLOP`) | — |

---

## 3. Screen layouts (480x320)

### 3.1 Scope view

The graph keeps the stock 400x281 px at (13,20) with 1:1 rows (the 280-row
model, 10x8 divisions). This means no new trace scaling and no loss of vertical
resolution. Gestures and popups add controls without taking graph area.

```
 x: 0  13                                                          413 417        479
 y0 ┌──┬──────┬───────┬────────┬───────────┬──────────┬──────────────┬─────────────┐
    │  │ RUN  │ 200µs │ 1V  AC │ ⟋ 0.52V A │ [══▮▮══] │ GEN Sq 1kHz  │●○●○ 🔒BT ▮▮▯│ 0..19 bar
    │  │ chip │ chip  │ chip   │ chip      │ rec bar  │ (gen state)  │dots status  │ (chips tap,
 y20├──┼──────┴───────┴──── ▼T ┴───────────┴──────────┴────────┬─────┼─────────────┤  zone to y27)
    │  │‹  ·   ·   ·   ·   ┊·   ·   ·   ·   ·   ·   ·   ·    ›│     │  ┌───────┐  │ 20..59
    │  │                   ┊                                   │     │  │ V  ⊕  │  │ V/div smaller
    │  │        ╭─╮        ┊        ╭─╮           ╭─╮          │     │  └───────┘  │
    │  │       ╱   ╲       ┊       ╱   ╲         ╱   ╲         │     │  ┌───────┐  │ 61..100
    │  │ - - -╱- - -╲- - - ┼ - - -╱- - -╲- - - -╱- - - - - - -◀│     │  │  AC   │  │ coupling
    │1▶│─────╱───────╲─────┼─────╱───────╲──────╱──────────────│     │  │  CH1  │  │ (label=state)
    │  │    ╱         ╲    ┊    ╱         ╲    ╱               │     │  └───────┘  │
    │  │ ──╯           ╰───┊───╯           ╰──╯                │     │  ┌───────┐  │ 102..141
    │  │                   ┊                                   │     │  │ V  ⊖  │  │
    │  │ a                 ┊          b                        │     │  └───────┘  │
    │  │ ┊                 ┊          ┊                        │     │  ┌───────┐  │ 143..182
    │  │ ┊                 ┊          ┊                        │     │  │ t  ⊕  │  │ faster
    │  │◁a┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┊┄┄┄┄┄┄┄┄┄┄┊┄┄┄┄┄ (Y cursor a) ┄┄┄┄┄┄│     │  └───────┘  │
    │  │ ┊                 ┊          ┊                        │     │  ┌───────┐  │ 184..223
    │  │ ┊                 ┊          ┊                        │     │  │ t  ⊖  │  │ slower
    │  │[a]               ┊         [b]                       │     │  └───────┘  │
 y300├──┼─────────────────────────────────────────────────────────┤     │  ┌───────┐  │ 225..264
    │  │ Freq 1.000kHz│ Vpp 3.30V    │ Cycl 1.000ms │ Vrms 1.17V │     │  │TRIG   │  │ "Auto ⟋"
    └──┴──────────────┴──────────────┴──────────────┴────────────┘     │  └───────┘  │
       301..319 measurement bar (tap cell = picker; cursor readouts     │  ┌───────┐  │ 266..305
       replace it while CUR is on: Δt 2.00ms  1/Δt 500Hz  ΔV 1.40V)    │  │ CUR XY│  │
                                                                       │  └───────┘  │
                                                                       └─────────────┘
 Markers:
 - 1▶ (yellow, left gutter x 0..12) = CH1 0 V row; an arrow at the top or bottom
   when off-screen.
 - ◀ (orange, graph columns 392..399) = trigger level row. The dashed level line
   is drawn across the graph only while dragging and for 1 s after.
 - ▼T (orange, graph rows 0..7) = trigger position; an arrow at the edge when
   off-screen.
 - ‹ › = corner timebase tap hints (optional).
 - [a] [b] = X-cursor tabs on the bottom edge; ◁a = Y-cursor tab on the left
   inner edge.
```

Notes:
- **Right panel.** It stays at x 414..479 with seven 60x40 buttons, the same
  pitch as today: `SCOPE_RIGHT` y values 20/61/102/150/191/232/280 are re-spread
  to 20/61/102/143/184/225/266. Icons ⊕/⊖ are drawn from primitives: a 1 px
  circle via `fill_rect` runs plus the glyph "+"/"−" in F14.
- **Top bar.** It follows the ui-review "unified bar" (lock, BT, battery, page
  dots on every view), but on the scope page the instrument readouts are tap
  chips. Widths: RUN 50, timebase 62, V/div+coupling 70, trigger 84, record
  bar 64, gen 110, status 40. Page dots go into the status block when the
  unified bar lands.
- **Trigger marker placement.** It sits inside the graph's right edge (columns
  392..399, drawn by the column renderer as an overlay, like today's left
  tick). It does not get its own gutter, which keeps the 400 px graph and the
  66 px panel unchanged.

### 3.2 DMM + Scope combined view

The goal is a compact DMM reading on top and a drivable scope below. The side
keys stay with the DMM.

```
 x: 0  13                                                          413 414        479
 y0 ┌───────────────────────────────────────────────────────────────────────────────┐
    │ AUTO+  RUN  REL      ○●○○                          ⚡ BT 🔒 ▮▮▯   01:23:45    │ 0..21 unified bar
 y22├───────────────────────────────────────────────────────────────────────────────┤
    │  12.3456  V  DC      50.00 Hz            ▲HV                                  │ 22..55 DMM row
    │  └ LETTERS 30 px ┘   └ aux (restored) ┘                                       │ (tap = full DMM)
 y56├──────┬───────┬────────┬───────────┬─────────────────────────────────────────┤
    │ RUN  │ 200µs │ 1V  AC │ ⟋ 0.52V A │  Vpp 3.30V   Freq 1.000kHz        ⤢    │ 56..75 scope chips
 y76├──┬───┴───────┴────────┴───────────┴────────────────────────┬────────────────┤ (⤢ = full Scope)
    │  │‹  ·   ·   ·   ·   ┊·   ·   ·   ·   ·   ·   ·   ·      ›│   ┌────────┐   │ 76..115
    │  │                   ┊                                     │   │  V ⊕   │   │
    │  │       ╭─╮         ┊       ╭─╮          ╭─╮              │   └────────┘   │
    │1▶│──────╱───╲────────┼──────╱───╲────────╱───╲────────────◀│   ┌────────┐   │ 116..155
    │  │     ╱     ╲       ┊     ╱     ╲      ╱     ╲            │   │  V ⊖   │   │
    │  │ ───╯       ╰──────┊────╯       ╰────╯       ╰───        │   └────────┘   │
    │  │                   ┊                                     │   ┌────────┐   │ 156..195
    │  │                   ┊                                     │   │  t ⊕   │   │
    │  │                   ┊                                     │   └────────┘   │
    │  │                   ┊                                     │   ┌────────┐   │ 196..235
    │  │                   ┊                                     │   │  t ⊖   │   │
    │  │                   ┊                                     │   └────────┘   │
    │  │                   ┊                                     │   ┌────────┐   │ 236..275
    │  │                   ┊                                     │   │ RUN/STP│   │
 y277├──┴──────────────────────────────────────────────────────────┴───┴────────┴───┤
    │    V   │   A   │   Ω   │  Cap  │ Diode │ Hz/°C        (DMM function buttons)  │ 280..319
    └───────────────────────────────────────────────────────────────────────────────┘
 Graph: x 13..413, y 76..277 (h = 201 = 8 div x 25 px + 1). The same bands and
 markers as the full view. Taps on the scope chips: RUN chip = run/stop; the
 other chips open the same popups as on the full Scope page. ⤢ jumps to the
 Scope view.
```

Notes:
- **Space.** The compact DMM row already exists (`DmmView::reading(compact)`,
  `BAR_H_C` = 22, LETTERS at y 24). This layout moves the scope's label row to
  y 56..75 (currently `SCOPE_COMPACT.y0 - 19` = 59) and grows the graph from
  199 to 201 px. The right side x 414..479 is unused today and becomes the mini
  panel: five 60x38 buttons (V⊕, V⊖, t⊕, t⊖, RUN/STOP). The ui-review asked
  for 40 px; 38 px is the most that fits in 201 px.
- **Grid at non-1:1 heights.** `templates_c` derives grid rows from
  `r % 35 == 0` after scaling. At h = 201 (and any h ≠ 281) some row values are
  skipped, so grid lines can go missing. Compute the grid from `y` with the
  per-division pitch, `(h - 1) / 8`, instead.
- **Analog rails.** The combined view needs **both rails on**, which is the user
  decision queued in TODO. Until then `show()` must call `power_both` for
  `View::DmmScope`, or the scope half shows an unpowered front end (P18).

### 3.3 FFT mode (inside the Scope view)

```
 y0 ┌──┬──────┬──────────┬─────────────┬───────────────┬────────────────────┬──────────┐
    │  │ FFT  │ Hann     │ 0..250 kHz  │ 10dB/ 0dBV ref│ Pk 1.000kHz −3.2dBV│●○●○ ▮▮▯ │
 y20├──┼──────┴──────────┴─────────────┴───────────────┴──────────┬─────────┼──────────┤
    │  │ |                                                         │         │ (panel as │
    │  │ |▼ Pk                                                     │         │  YT; V⊕/⊖ │
    │ ▶│─||──────────────────────────────────────── ref ──────────  │         │  = dB/div,│
    │  │ ||   |                                                    │         │  t⊕/⊖ =   │
    │  │ ||   |    |                                               │         │  span)    │
    │  │_||_|_|_|__|_.__.___.._____________________________________│         │           │
 y300├──┼───────────────────────────────────────────────────────────┤
    │  │0   25k  50k  75k  100k 125k 150k 175k 200k 225k 250k      │ frequency axis labels
    └──┴───────────────────────────────────────────────────────────┘ (replace the meas bar)
```

The frequency axis is labelled at each of the 10 divisions in Hz/kHz/MHz, and
the top bar shows window, span, dB/div + reference, and the automatic peak.
This addresses "no numeric axis" (#15794, #51067). The last spectrum stays on
screen while settings change, so the display does not blank (#16554).

---

## 4. Specific behaviour fixes

1. **AUTO keeps the coupling (UF-04, P22).** Changes in `autoset_start`:
   - Stop forcing `ac = false`.
   - Run phase 1 with the user's coupling.
   - If AC is selected and every candidate range gives a swing of 19 codes or
     less, retry once in DC. On success, keep DC and show the toast
     "AUTO: DC (no AC signal)". Otherwise restore the saved settings (as today).
   - Also keep the probe and slope; set trigger position = 50 %.
   - Save the pre-auto-set `Settings` for AUTO long-press undo.
2. **Separate the offset from the trigger level (P2, P3, P5, P6).**
   - Remove `tap()`'s graph `trig_row = y` and the left-strip `pos = 140 - …`
     branch.
   - Replace `gesture()`'s absolute `Drag` → `trig_row` with the band model of
     §2.2, using relative deltas from the snapshot taken at Down.
   - On an offset change, add the same row delta to `trig_row` so the trigger
     voltage holds (stock `FUN_080225c2`).
   - Apply the stock pos clamp (±192 rows for range 4 and 9) in `Scope::apply`.
3. **Axis-locked drags (UF-05, P4).** In `gesture.rs`:
   - After 12 px of travel, lock to x if |dx| ≥ 2|dy|, or to y if |dy| ≥ 2|dx|.
     If neither holds by 20 px, lock to the larger axis.
   - Emit `Drag { p0, p, axis }`. Consumers use only the locked component.
4. **Drag ≠ swipe (P1).**
   - The tracker gets `capture()`: the app calls it from the `Down` handler when
     the point is inside a scope graph or a draggable control.
   - A captured touch never produces a swipe, and a touch that sent any
     `Drag` the app consumed never does either.
   - Swipes need |dx| > 2|dy| (horizontal) or the reverse.
5. **Trigger position (UF-09, P10).**
   - Add `Settings.tpos` in columns 0..400, default 200.
   - `Scope::finish` uses `pre = screen * tpos / 400` instead of `screen/2`. The
     search window starts at `pre`, and `start = trig - pre`. The record length
     is unchanged, because `start ≤ win` holds for any `pre` in 0..=screen, so
     there is **no RAM cost**.
   - On a timebase change, keep `tpos` in pixels, as stock does (`FUN_08020c7c`).
   - While stopped, a horizontal drag moves `stored_anchor` (pan) instead.
   - Draw the T marker and the record bar.
   - Persist `tpos` in a free `persist` word (11..15 are unused).
   - SCPI: `:TIMebase:POSition` / `:TRIGger:POSition` (new).
6. **Cursor quick access (UF-08, P11).**
   - The CUR button sets `cur.x_on/y_on` directly. `cursors_from_selection` no
     longer drives them; the measurement items 7/8/17 still read them.
   - Grab per §2.3, labelled tabs, readouts in the measurement bar while on
     (Δt, 1/Δt, ΔV; in FFT: Δf, ΔdB).
7. **FFT with labelled axes (UF-11, P16).** This is a display mode of the Scope
   view.
   - **Input:** the visible window (`Frame.data`, `screen` samples). It is
     reduced to N = 1024 points by an integer block average (anti-alias), or
     zero-padded when `screen` < 1024.
   - **Window and transform:** Hann (default), Flat-top or Rect, then a real
     radix-2 FFT in f32 on the M4F.
   - **Output:** magnitude in dBV RMS with window gain correction. 512 bins are
     mapped to 400 columns as per-column max (peak detect), drawn through the
     same column renderer as the trace.
   - **Span and resolution:** span = f_s,eff / 2, where f_s,eff = fs × N /
     `screen`. Resolution = 1/T_screen.
   - **Controls:** t⊕/t⊖ change the span (through the timebase), and V⊕/V⊖
     change the dB/div (5/10/20). A vertical drag moves the reference level and
     a horizontal drag moves the marker.
   - **Peak:** automatic, the largest bin above DC with parabolic
     interpolation for f, shown as "Pk f, dBV" in the bar.
   - **Rate:** recompute at most every 200 ms. Keep the last spectrum on screen
     while settings change.
8. **Honest right-panel labels (P12, P13, UF-10).** The panel becomes V⊕, DC/AC
   (shows the state), V⊖, t⊕, t⊖, TRIG ("Auto ⟋"), CUR ("X/Y/XY"). This is a
   new layout, not a relabel of the old one.
9. **Swallow the wake tap (P7).** After a dim, the first Down only un-dims the
   screen. The tracker is told to ignore that touch until release.
10. **Drag performance (P8).**
    - **Coalesce:** the app keeps the latest `Drag` per main-loop pass.
    - **Trigger-level drags** only move the marker and level line and update
      `trig_code` for `find_trigger`. With the software trigger they need no
      `halt()`/restart; with a future hardware trigger, write DAC2 only.
    - **Offset drags** write TIM3 CCR3 at most every 50 ms. Between writes the
      drawn trace is shifted digitally (row + Δ), which redraws only the changed
      columns. The real acquisition restarts on release or at the 50 ms tick.
    - **Never full-invalidate** the graph for marker moves: redraw only the old
      and new marker columns or rows.
11. **Single status (P24).** Armed and waiting shows "ARM" (or "WAIT"), and the
    triggered-then-stopped state shows "STOP" in orange, as stock.
12. **Trigger hit rate (P23). Measure first.**
    - Add `DBG:SCOP?` counters for records searched, triggered and shown
      untriggered.
    - If the hit rate is poor at 100 µs..50 ms/div, widen the software search
      window in Auto/Normal to `min(CAP - screen, max(WIN, screen))`. This
      costs nothing at fast timebases. At slow ones the capture time grows, but
      at most to the 26 K record.
    - Long term, port the stock comparator trigger: DAC2 level on PA5,
      comparator → PA15 TIM2_CH1, TIM5 pre-trigger count. `parity.md` should
      then say "software refinement MATCH, hardware trigger MISSING".
13. **Probe list x0.1..x1000 (UF-12, P17)** in the CH1 popup. This only relabels
    (stock semantics: the hardware range is unchanged).
14. **Persistence levels (P25):** OFF / 0.5 s / 1 s / INF, cleared on
    settings changes.
15. **Optional:** sinc interpolation at ≤ 200 ns/div (UF-41, P26), using the
    stock FIR tables at 0x0806ddf8 / 0x0806f6ec / 0x08071c48 (L 2/4/8). Save a
    waveform as CSV from the quick menu (UF-13, P27) after the FAT writer
    exists.

---

## 5. Implementation plan

Each step is independently mergeable, builds on its own, and leaves the device
usable.

- **Size:** S < ½ day, M ≈ 1–2 days, L > 2 days.
- **Testing:** after every step, run `./build.sh` (and `./build.sh native` once
  native is in use). Report the `.text` / `.bss` / `.dmabuf` deltas. Verify on
  the device with `DBG:TAP` / `DBG:DRAG` (S0) and `DBG:LCD?` screenshots.

| Step | What | Size | Files | Depends on |
|---|---|---|---|---|
| **S0** | Test hooks. Add `DBG:DRAG x0,y0,x1,y1[,ms]` (synthesised Down/Drag…/Up at 10 ms steps) and `DBG:MT n,x,y,dx,dy` (n-finger swipe) to inject gestures. Extend `DBG:DIAG` with `prof[2]` (frame draw time) and the trigger hit-rate counters. | S | `scpi.rs`, `main.rs`, `scope.rs` (counters) | — |
| **S1** | **Gesture core.** In `gesture.rs`: `capture()`, swipe suppression after a consumed drag, swipe dominance ratio, axis lock (`Drag` carries the axis), `DoubleTap`, `Cancel` on `points ≥ 2`, and track the max point count. In `app.rs`: call `capture()` from the scope `Down`; swallow the wake touch. Fixes P1, P4, P7. | S–M | `gesture.rs`, `app.rs` (`gesture`), `main.rs` (DBG injection path) | S0 (for testing) |
| **S2** | **Honest right panel and side keys.** New `SCOPE_RIGHT` (V⊕, DC/AC, V⊖, t⊕, t⊖, TRIG, CUR), ⊕/⊖ icons, pressed-state highlight on Down. TRIG cycles the mode until S6 adds the popup. RUN long-press = Single and AUTO long-press = undo; short actions move to KeyUp for those keys. Fix the Single status label. Fixes P12, P13, P21, P24. | S | `ui.rs` (`SCOPE_RIGHT`, `labels`, `s_btn`), `app.rs` (`tap` View::Scope, `key`) | — |
| **S3** | **AUTO-set fixes.** Keep the coupling with a DC fallback and toast; keep probe and slope; `tpos` = 50 % once S5 lands; save settings for undo. Fixes P22. | S | `app.rs` (`autoset_start`, `autoset_step`) | — (undo key binding from S2) |
| **S4** | **Graph touch model and markers.** Hit-test bands (left/body/right/corners), relative drags from the Down snapshot, revert on `Cancel`, double-tap actions, remove trigger-on-tap and the mirrored pos tap. Offset moves `trig_row` by the same delta; pos clamp ±192 for range 4/9. Offset and trigger markers, the temporary level line and readout, off-screen arrows. Drag coalescing/throttle and partial marker redraw (§4.10). Put the band and zone logic in a small pure module (`scope_touch.rs`: geometry in, action out) so it can be unit-tested on the host. Fixes P2, P3, P5, P6, P8, P9. | M | new `src/scope_touch.rs`, `app.rs` (`gesture`, `tap`, `scope_set`), `ui.rs` (`ScopeView::column_t`/`frame` overlays, marker draw in the gutter), `scope.rs` (`apply`: trig_row follows pos, clamp) | S1 |
| **S5** | **Trigger position.** `Settings.tpos`; `finish()` uses `pre`; rescale on timebase change; STOP pan via `stored_anchor`; T marker + record bar; horizontal-drag binding; double-tap centre; persist word; SCPI `:TIM:POS`. Fixes P10. | M | `scope.rs` (`Settings`, `finish`, `zoom_frame`, `apply`), `app.rs` (bindings, `persist_words`/`restore`), `ui.rs` (marker, record bar), `scpi.rs` | S4 (bindings); the scope.rs part can merge first |
| **S6** | **Popup framework, chips and pickers.** A small modal popup: rect, 40 px rows, tap outside / MENU closes it, drawn over the graph, and the graph is re-validated on close. Top-bar chips: RUN (toggle), timebase (list), V/div (CH1 popup: list, DC/AC, probe x0.1..x1000), trigger (mode, edge, level 50 %, position centre). Long-press pickers on ⊕/⊖. Graph long-press quick menu (FFT/Persist/Cursors/Centre/Save/Screenshot/Help; items appear as their features land). Optional corner taps with a Settings toggle. Fixes P14, P15, P17. | M–L | `ui.rs` (popup, chip hit-test, `labels`), `app.rs` (popup state; either a `View::ScopePopup` or an overlay flag; side-key routing), `scope.rs` (probe table) | S2, S4 |
| **S7** | **Cursors.** CUR cycles the mode independently of the measurement selection; tabs; ±20 px axis-resolved grab; readout bar while on; track mode; reset. Measurement-cell tap → item picker. Fixes P11. | M | `app.rs` (`cur`, `cursor_values`, `cursors_from_selection`, gesture bindings), `ui.rs` (`Cursors`, `frame`, `scope_measurements`), `scope_meas.rs` (readout formats), `scope_touch.rs` | S4 (S6 for the picker) |
| **S8** | **Combined view controls.** Chips row at y 56..75, graph y 76..277 (h 201, grid from `y`), mini panel (V⊕/V⊖/t⊕/t⊖/RUN), graph bands and markers reused via `ScopeView` geometry, ⤢ → Scope. `show()` powers both rails in `View::DmmScope` (follow the queued "analog rails" decision). Fixes P18. | M | `ui.rs` (`SCOPE_COMPACT`, `templates_c` grid, compact panel), `app.rs` (`tap` DmmScope branch, `show`, `draw_static`), `scope.rs` (`power_both` use) | S4; coordinate with the analog-rails task (`power.rs`/`app.rs`) |
| **S9** | **Trigger hit rate.** Counters (from S0). If they confirm the problem, widen the search window (§4.12). | S | `scope.rs` (`finish`, `apply` rec length) | S0 |
| **S9b** | Hardware comparator trigger port (DAC2/PA5, PA15 TIM2_CH1, TIM5 pre-count, as stock) with software refinement kept. | L | `scope.rs`, `scope_gen_v2_6.h` reference | S9 result |
| **S10** | **FFT.** New `fft.rs`: in-place radix-2 real FFT f32 (N = 1024) with Hann, Flat-top and Rect windows (the window can be computed on the fly with `libm`-free cosine recursion to save 4 KB). FFT display mode in `ScopeView` (columns = bins, dB map). Frequency-axis labels in the measurement bar; peak readout; FFT-aware cursors; FFT tab in MF; mode chip. Keep the last spectrum on screen during changes. Fixes P16. | L | new `src/fft.rs`, `ui.rs` (`ScopeView` FFT path, axis labels, chips), `app.rs` (`scope_poll` FFT branch, bindings), `scope_meas.rs` (dB formats), `scpi.rs` (optional `:FFT:DATA?`) | S6, S7 |
| **S11** | **Multi-finger global gestures.** `touch.rs` passes all points (or centroid + count) in `Event`; 2-finger down → power-off confirm dialog; 3-finger down → screenshot (BMP to the SPI FAT, toast). Optional 2-finger left/right → view switch (setting). Fixes P28. | M (+FAT) | `touch.rs` (`read_hyn`/`read_goodix`, `Event`), `gesture.rs`, `app.rs`, new `screenshot.rs` | S1; FAT writer (CSV-to-disk TODO item) |
| **S12** | Polish: persistence levels (P25), help page entries for scope gestures (UF-29), optional sinc interpolation (P26), waveform CSV save (P27, after the FAT writer). | S each | `ui.rs`, `app.rs`, `scope.rs` | S6 (menu), FAT writer |

**Suggested order:**

S0 → S1 → S2 → S3 → S4 → S5 → S9 → S6 → S7 → S8 → S10 → S11 → S12

- S2, S3 and S9 are independent and can go in parallel with S1.
- S8 needs the analog-rails change.
- S11 needs the FAT writer.

### 5.1 Risks and constraints

- **Code size (RAM-loaded build).**
  - The sector-8 RAM image runs from a 120 KiB `CODE` region and `.text` is
    already 114 896 B (measured on the current `target/.../release/dm40-fw`),
    leaving about 8 KB.
  - S6, S7, S10 and S11 will not fit there. They need the native image (about 600 KB
    flash, `link-native.x` FLASH_B, now the top priority), or code trimmed elsewhere.
  - Each step must report its `.text` delta. Gate the big steps on the native
    build.
- **RAM.**
  - `.dmabuf` is 59 KB of 72 KB in the RAM build (scope `BUF` 52 KB, `gen::WAVE`
    4 KB, `ROLL_COLS` 1.6 KB).
  - `.bss` (TCM) is 26 KB of 64 KB, shared with the stack. `LOGGER` alone is
    24.6 KB.
  - FFT N = 1024 needs about 8 KB (re/im f32) plus 1.6 KB of column dB, plus an
    optional 4 KB window table. Put it in TCM `.bss` or the spare `.dmabuf`.
    N = 2048 (about 16 KB) only in the native build (192 KB SRAM).
  - Trigger position costs nothing (same record length).
  - Popups need a save-under buffer only if they must not trigger a redraw.
    Prefer redrawing the covered graph columns (the column renderer is cheap)
    over a 400x200x2 = 160 KB buffer, which does not fit.
- **CPU and redraw.**
  - A full graph redraw is 401 columns × 281 px ≈ 113 K FSMC writes plus
    per-pixel template selection; measure with `prof[2]`.
  - Drag paths must not full-invalidate (§4.10). Marker moves redraw ≤ 2
    columns/rows. Throttle offset hardware writes to 20 Hz.
  - Measurements on a 25 K-sample window run every 300 ms (`scope_meas::compute`).
    FFT adds about 1–2 ms per 200 ms.
  - The main loop polls touch every 10 ms (`main.rs` `next_touch`). A long
    frame draw delays touch and makes drags feel laggy. Keep any single draw
    under about 15 ms, or split it across loop passes.
- **Touch controller.**
  - The Hynitron reports a count of up to 5 and the coordinates; fw-rs drops
    points 1..4 today.
  - On this panel these are unverified: multi-point tracking, point-0 identity
    across lift-off, and how fast a second finger is reported.
  - The 40 ms state debounce in `touch.rs` delays point-count changes.
  - Screen long-presses that the controller reports as keys are dropped
    (`touch.rs poll`).
  - Validate S11 with real fingers before enabling power-off. Keep the confirm
    dialog mandatory.
  - The Goodix path is untested (no hardware).
- **Retraining users.** Taps in the graph no longer set the trigger, and
  swipes inside the graph no longer switch views. Mitigations:
  - page dots;
  - visible markers;
  - the optional 2-finger horizontal swipe;
  - the Help page (UF-29);
  - a one-time toast on the first Scope entry after an update: "Drag ◀ right
    edge = trigger, drag left = offset, swipe the top bar to change view".
- **Stock deviations to record in `parity.md`:**
  - band-based gestures instead of select-then-drag;
  - corner taps;
  - AUTO keeps the coupling;
  - V⊕ = zoom in;
  - RUN/AUTO long-press actions;
  - FFT as a display mode rather than a math channel;
  - trigger position in pixels;
  - the combined-view scope controls.
- **Conflicts with parallel work.**
  - S1 and S11 touch the same files as the queued gesture task (3-/2-finger)
    and the CSV-record swipe. Do S1 first and build those on its API.
  - S8 overlaps the analog-rails task and the ui-review unified-bar work in
    `ui.rs`/`app.rs`. Sequence them, do not run them in parallel.
