# Scope scale and position by touch: survey and DM40 proposal

**Implemented (2026-10-14)**, `src/scope_ui.rs` (tap step section,
`Popup::Tb`) and `src/scope_page.rs`; tested by `tools/touch-test`
(`tap_step_tests`) and `tools/ui-render` (section 0a). Owner's choice:
**Corners is the default** (`scope_ui::TAP_DEFAULT`, one const); Halves and
Off are in Settings 2/2 "Scope tap step" (persisted in p[1] bits 28..29 as
the mode XOR the default, so older records restore the default).
Deviations from the text below:
* tabs are 52 px wide, (23,280,52,17) and (358,280,52,17): at 50 px the
  widest labels ("<500ms", 45 px) were 2 px from the ring;
* the list box's first row is y 21 (79,21,132,191): y 20 is the frame line
  in the same colour, so it looks the same and keeps 3 px from the badges;
* Corners hold: the first repeat 500 ms after the press, then every 200 ms
  (owner's numbers); a tap steps on release, a hold that repeated does not
  step again on release;
* a roll-only list entry (orange, beyond 1 s/div) turns Roll on;
* the top-bar badges win over the top 5 rows of the graph area (y 15..19)
  where the T marker's zone reaches into the bar; the tabs and corner zones
  never win over a marker or cursor-line zone.

2026-10-14. Question from the owner: the stock
tap-to-step timebase (tap off a marker: left half slower, right half faster,
`stock-scope-ui.md` §10.3) is not obvious. Would "+/- zones in the corners" be
better? How do other touch scopes and scope apps change the horizontal and
vertical scale, the position and the trigger level?

Source quality tags used below: **manual** = read in the vendor manual or help
PDF (page given), **forum** = owner posts (EEVblog etc.), **review** = a
published review, **marketing** = datasheet or shop text only, **unverified** =
only a search snippet or a secondary copy.

---------------------------------------------------------------------------

## 0. Summary

* **Zone taps are an established pattern.** GW Instek patented them (Good Will,
  US9582185), and three product families use them: GW Instek GDS-200/300,
  the Owon-type "SingleZoom" platform and FNIRSI 1013D. DM40 stock uses them
  too. The difference between the families is the **visible hint**:
  * GW Instek and the Owon type show an overlay (a scale indicator, or a
    "touching control panel" with labelled zones) before or while the taps
    work.
  * FNIRSI 1013D and DM40 stock have invisible halves. The FNIRSI review says
    the timebase taps are "difficult to discover without reading the manual".
  * DM40 owners who did learn the stock taps praise them (user-feedback.md P7)
    and describe them as "tap the top-left/right corner".

  So what decides discoverability is whether a hint is shown, not whether the
  zone is a half or a corner. Corner zones alone will not fix the complaint.
  A hint fixes it with either geometry.
* **The near-universal second path is "tap the scale readout".** It opens a
  list, a spinner, +/- buttons or a keypad. Micsig, Siglent, Tektronix,
  Keysight, Scoppy and Aeroscope all have it. DM40 stock has nothing tappable
  in the top bar. fw-rs already made the run badge (RUN/STOP) and the trigger
  badge (Trigger tab) tappable. The timebase badge and the V/div badge are
  still free.
* **Pinch is standard on bench scopes** (Siglent, Rigol, R&S, Tek, LeCroy,
  GW Instek) and in apps. On a handheld it has a problem: one hand holds the
  probe and the other holds the meter, so most operation is by one thumb.
  fw-rs already has pinch. Keep it as the secondary path.
* **Recommendation** (§4): Design A as the default. It keeps the stock halves,
  which owners have learned, and adds three things:
  * value-preview tabs in the bottom corners, shown while a finger is down and
    for 2 s after a step;
  * a flash of the timebase badge on every step;
  * a timebase spinner popup on a tap of the timebase badge.

  Design B is the owner's corner-only zones. It shares the same tabs and is
  offered as a Settings choice ("Tap step: Halves / Corners / Off"). Its
  geometry is ready (§3.2) if the owner wants corners as the default.

---------------------------------------------------------------------------

## 1. Survey

### 1.1 Touch scopes with a manual read

| Instrument | Scale (time/div, V/div) | Position | Trigger level | Gesture conflicts | Source |
|---|---|---|---|---|---|
| **Micsig TO series** (tablet, touch only) | **On-screen +/- buttons** beside the timebase readout and beside each channel's V/div readout, 1-2-5 steps. **Tap the readout** to open the timebase list ("Time Base Knob": tap a value). **No pinch** for scale in this manual. | 1-finger drag in the graph is coarse. A **"Fine" button** in the lower-left corner nudges the last-adjusted item (position, level, trigger point, cursor). | Slide up/down in a **dedicated trigger-level slide bar** on the right edge. A level line shows while adjusting. Swipe left from the bar opens trigger shortcuts. | Separate areas per object: graph = position, right bar = trigger level, bottom/top edge swipes = menus. Two fingers = cursor linkage. | manual: TO user manual V1.0 2022.06, p.30-31 (items 12, 13, 18, 20), §3.1-3.2 p.51-53, §4.2-4.3 p.75-76, §5 p.99-101 ([PDF](https://www.batronix.com/files/Micsig/New-Micsig-Product-Files-08-2023/(New)-TO-series--Oscilloscope/Micsig-tablet-oscilloscope-TO-series-user-manual.pdf)) |
| Micsig, owner feedback | Owners asked for a pop-up **matrix of timebases** ("more suitable for changing or selecting timebase quickly on a touch-screen"). A complaint: a timebase change zooms about the screen centre and loses the trigger point. Owners found the multi-finger zoom not obvious ("had to read the manual"). They like the scale shown next to its +/- buttons. | | | | forum: [wishlist](https://eevblog.com/forum/testgear/micsig-to1104-wishlist/msg1656878), [TO1104 review](https://eevblog.com/forum/testgear/micsig-to1104-(similar-to-rigol-1104z)/msg1261690), [p.2](https://eevblog.com/forum/testgear/micsig-to1104-(similar-to-rigol-1104z)/25) |
| **Siglent SDS800X HD** (7", knobs + touch) | **Pinch/spread** horizontally = timebase, vertically = V/div ("vertical gain"). **Tap the channel descriptor box**: a quick dialog with **▲/▼** steps, a **coarse/fine** checkbox, and a value field (universal knob or virtual keypad). Touch scaling **switches V/div to fine mode automatically**. Same for the timebase descriptor. | Drag the trace. | Drag the trigger level indicator. | Cursors are grabbed by touching the cursor itself. Their ΔX/ΔY readout region is dragged to move both. "Gestures are quick but not precise; use the knob for fine". Zoom window: double-tap sets the zoom centre. | manual: SDS800X HD user manual EN01A §7.3 p.27, §7.7 p.34, §11.2 p.45, zoom p.67, cursors p.113 ([PDF](https://www.batronix.com/files/Siglent/Oszilloskope/SDS800X-HD/4-Manuals/SDS800X_HD_UserManual.pdf)); forum: horizontal pinch snaps 1-2-5, no vernier ([rf-loop](https://eevblog.com/forum/testgear/sds800x-hd-review-demonstration-thread/msg5363645)) |
| **Tektronix 2 Series MSO** (10.1", 4/5/6 Series similar) | **Single-tap a badge** shows **Scale +/- buttons on the badge**. Double-tap opens its configuration menu. **Pinch** horizontally or vertically on the waveform view. Long-press = right-click menu. | Drag in the waveform view. | Level knob, menu, drag. | Pinch/drag in the zoom view changes only the zoom. Flick a badge off the bottom edge removes it. A **Touch Off** key. | manual: 2 Series MSO help 077-1716-08, Badges p.36, Table 3 p.47-48, zoom UI ([PDF, DE wrapper, EN text](https://www.datatec.eu/media/fe/f9/2c/1777276826/Tektronix-MSO2-Serie-HB-1_DE.pdf)) |
| **Keysight InfiniiVision 3000T** (8.5") | **No pinch.** Touch the scale/offset values of a channel, or the horizontal scale/delay values, to open a **scale dialog** (knob or keypad). | Drag horizontally = delay, vertically = offset. **Flick** for fast browsing. Drag the ground reference icon = that channel's offset. | Touch the level value → dialog. | **Mode switch in the upper-right corner**: drag mode vs rectangle-draw mode (zoom / zone trigger). A [Touch] key turns touch off. | manual: 3000T user's guide p.48-50, 55, 57-59 ([PDF](https://www.batronix.com/files/Keysight/Oszilloskope/3000XT/3000XT-Manual.pdf)) |
| **R&S RTB2000** (10.1"; RTM/RTA similar) | **Spread/pinch** vertically = V/div of the selected trace, horizontally = timebase (span in FFT). | 1-finger drag in the diagram = position; drag the markers for each trace. | Drag the trigger level marker. | Two-finger swipe = history segments. A **TOUCH LOCK** key. | manual: RTB2000 user manual §3.3.4 p.30-31 ([PDF](https://www.batronix.com/pdf/Rohde-Schwarz/RTB2000/RTB_UserManual_en.pdf)) |
| **GW Instek GDS-200/300** (handheld, 5" touch) | **Pinch** vertically = V/div, horizontally = timebase. A **scale indicator pops up**. **While it is visible, tapping the upper/lower half (V/div) or the left/right half (timebase) steps the scale.** | Swipe = position. A popup shows the value. **Snaps to zero at each crossing** (swipe again to pass). Tapping the upper/lower part also nudges the position. | **Swipe in the strip right of the grid.** Tap the trigger indicator to toggle the source. | A **Gesture Control icon** in the graticule corner switches the gestures between waveform and cursor control. | manual: GDS-200/300 user manual 2020-06, p.41-46 ([PDF](https://static.eleshop.nl/mage/media/downloads/GDS-200_300_User_Manual_EN_20200617.pdf)); patent: [US9582185](https://patents.google.com/patent/US9582185) (Good Will) |
| **Owon-type "SingleZoom/DoubleZoom"** (Multicomp Pro MP720105 family; looks Owon-built, not verified) | **DoubleZoom**: pinch H = timebase, V = V/div. **SingleZoom**: **tap anywhere shows a "touching control panel"** with labelled zones: upper-left/lower-left = CH1 V/div up/down, upper-right/lower-right = CH2, **left = slower, right = faster**. | Swipe up/down = position of the selected channel. Swipe left/right = horizontal position. | Swipe near the trigger pointer on the right. | **Double-tap = Run/Stop.** A touch on/off icon at the top right. | manual: "Junior User Guidebook" p.17-23 ([PDF](https://farnell.com/datasheets/4089179.pdf)) |
| **FNIRSI 1013D** (7" tablet, touch only) | **Tap the right half of the graph = faster, left half = slower** (exactly the DM40 stock rule). V/div via the side bar: [CTRL] → V+ / V-. | Tap the channel indicator → its control bar. A fast/slow movement-speed toggle. | Via the trigger control bar. | Touching freezes the display (forum). | manual: [manualslib 3999283](https://www.manualslib.com/manual/3999283/Fnirsi-Fnirsi-1013d.html); review: "changing the time base requires tapping specific sides of the screen ... difficult to discover without reading the manual" ([review](https://fnirsioscilloscopes.store/review-hub/fnirsi-1013d-review-full-hands-on-evaluation/)); forum: [EEVblog thread](https://www.eevblog.com/forum/testgear/fnirsi-1013d-100mhz-tablet-oscilloscope/msg5995451/) |
| **DM40 stock V2.6** (3.5", 480x320) | Tap off a marker zone: **left half slower, right half faster**. Right-panel B1/B3 = V/div +/-. No pinch. Top bar not tappable. | Select a marker by press (32x32 zones), then drag. Per-event axis choice. | Trigger level marker on the left strip, dragged after selection. | Marker zones win over the tap step. A tap that wobbles 1-3 px also nudges the selection. | `stock-scope-ui.md` §10 |

### 1.2 Touch scopes with secondary sources only

| Instrument | What was found | Source |
|---|---|---|
| Rigol DHO800/900/1000, MSO5000, MHO5000 | Tap, drag, **pinch & stretch**: horizontal = timebase, vertical = V/div. Drag the trigger cursor = level. Drag a waveform = position. These models also have knobs. | unverified/secondary: quick guide on [manualslib (DHO800)](https://www.manualslib.com/manual/3194042/Rigol-Dho800-Series.html), [manualslib (MHO5000)](https://www.manualslib.com/manual/3657782/Rigol-Mho5000-Series.html) |
| Teledyne LeCroy WaveSurfer (MAUI OneTouch) | Drag, drop, pinch, flick. Pinch on a trace zooms. Drag the trigger level indicator. Drop a channel descriptor onto the trigger descriptor = trigger source. **Expanded touch area around cursors**. Flick a descriptor off-screen = close. | vendor blog: [the evolving user interface: cursors](https://blog.teledynelecroy.com/2016/05/the-evolving-user-interface-cursors.html) |
| Tek 4 Series B | Double-tap a badge = configuration menu. Drag = position, pinch = scale. | marketing/quick start, see the Tek row above |
| Keysight InfiniiVision 4000X; Tek 4/5/6 Series | Same touch model as the 3000T / 2 Series rows above (same vendor UI families); not read separately. | — |
| Siglent SDS1000X-E | No touch screen on this series (knobs + softkeys); SDS800X HD / SDS2000X Plus are the touch models. | — |
| FNIRSI 1014D, FNIRSI 2C23T, BSIDE, Sainsmart pocket scopes, Owon VDS (PC/USB) | Not surveyed: no touch-scale documentation found in the time spent. | — |
| Hantek 2C/2D, FNIRSI DPOX180H, Owon HDS, Zeeweii DSO3D12 | **No touch screen.** Button pairs for time/div, V/div and level. The DPOX180H has 26 keys, and "knobs are replaced by Up/Down buttons". | review: [Elektor DPOX180H](https://elektormagazine.com/review/fnirsi-dpox180h-2-in-1-digital-phosphor-oscilloscope-review), [Elektor HDS2102S](https://www.elektor.com/blogs/review-blog/owon-hds2102s-handheld-2-channel-100-mhz-oscilloscope-multimeter-signal-generator-review); forum: [portable list](https://eevblog.com/forum/testgear/portable-oscilloscopes-list/msg5654619) |

### 1.3 Mobile and tablet scope apps

| App | Scale | Position / trigger | Source |
|---|---|---|---|
| **Scoppy** (Android, RP2040 front end) | Five ways for time/div: **pinch** horizontally; **+/- buttons** (tap = one step, **hold = repeat**); **tap the TIME/DIV value** = slider or an exact-value dialog; **long-press TIME/DIV = reset to default** (10 ms/div). V/div is the same (pinch vertically, +/-, tap = dialog, long-press = default). | Tap the ground indicator to select a channel. | manual: [Horizontal-Scale](https://oscilloscope.fhdm.xyz/app-help/Horizontal-Scale), [Vertical-Scale](https://oscilloscope.fhdm.xyz/app-help/Vertical-Scale) |
| **Aeroscope** (iPad) | **Pinch** H/V. The **current scales are shown on buttons** on the right that step them. | 1-finger drag H = trigger delay, V = offset. **Trigger level = a slider on the right edge of the plot.** | vendor: [iPad application overview](https://crowdsupply.com/aeroscope-labs/aeroscope-wireless-oscilloscope/updates/ipad-application-overview) |
| SignalScope Pro (iOS) | Pinch **inside the horizontal-axis label region** changes the time scale. A user preferred typing the timebase to pinching. | — | unverified (search snippet of [faberacoustical forum](https://faberacoustical.com/vforum/discussion/comment/414); the page did not load) |
| PicoScope 6/7 Automotive (touch PCs/tablets) | Pinch = zoom, as on phones (touch separated from mouse emulation in an update). | Pan by drag. | forum: [picoauto](https://picoauto.com/support/viewtopic.php?p=42271) |
| LabNation SmartScope | "Pointing, pinching and swiping" for scaling, panning and triggering. Mapping not documented. | — | marketing: [Elektor](https://elektor.com/labnation-smartscope-usb-oscilloscope) |
| Digilent WaveForms Live, Hantek 6022 Android apps, "Oscilloscope Pro" | No gesture documentation found. WaveForms Live uses an instrument panel with fields plus autoscale/centre-on-trigger buttons. | unverified: [App Store](https://apps.apple.com/app/id1244242035) |

### 1.4 Patterns, by frequency

1. **Pinch H = time/div, pinch V = V/div**, snapped to 1-2-5. Used by Siglent,
   Rigol, R&S, Tek, GW Instek, Owon-type, Scoppy and Aeroscope. Siglent
   switches V/div to fine mode under touch. Not used by Micsig TO or Keysight
   3000T.
2. **Tap the scale readout** opens a list, spinner, +/- or keypad. Used by
   Micsig, Siglent, Tek (+/- appear on the badge itself), Keysight and Scoppy.
   Scoppy adds long-press = default value.
3. **Visible +/- buttons next to the readout**: Micsig, Scoppy, Aeroscope, and
   DM40 B1/B3 for V/div. Owners like seeing the value next to its buttons
   (Micsig forum).
4. **Zone taps**: GW Instek (halves, only while the scale indicator is
   visible), Owon-type (zones only after the control panel is shown), FNIRSI
   1013D and DM40 (always-on invisible halves). Both vendors who designed the
   zones on purpose gate them behind a visible overlay. The GW patent's
   argument: pinch distance "is hard to be controlled precisely" for stepped
   scales, and on-screen buttons and track bars are too small, so it uses the
   large waveform area as the target.
5. **Drag = position everywhere.** The trigger level has **its own handle or
   edge strip** in every product: a right-edge bar or slider (Micsig, GW,
   Owon-type, Aeroscope) or a draggable level marker (Siglent, R&S, Rigol,
   LeCroy, DM40 stock).
6. **How conflicts are avoided:**
   * separate areas per object (an edge strip for the level, the graph for
     position);
   * an explicit mode switch (Keysight's corner icon; GW's Gesture Control
     icon for waveform vs cursor);
   * the object under the finger wins (Siglent and LeCroy cursors; DM40 marker
     zones);
   * a touch-off/lock key (Keysight, Tek, R&S);
   * "gestures coarse, knob or Fine button for precision" (Siglent, Micsig).
   * Double-tap is used as a shortcut: Run/Stop (Owon-type), zoom centre
     (Siglent), configuration menu (Tek).

---------------------------------------------------------------------------

## 2. What applies to the DM40

Constraints:
* 3.5" screen, about 0.17 mm/px. The 7 mm target minimum is about 40 px.
* No knobs. Four side keys: MENU / RUN / AUTO / MF.
* One-thumb use while the other hand holds the probe. Which corners a thumb
  reaches depends on the grip, which is not measured; see §5.
* The stock look comes first (`stock-scope-ui.md`).
* The no-overlap rule: every text, icon or shape is at least 3 px from the
  others and at least 3 px inside its container; checked by
  `tools/ui-render`.

**Frame.** The stock-replica page (`src/scope_ui.rs`: `TOP_BAR` (0,0,480,20),
`PANEL` (414,20,66,300), `MEAS_BAR` (0,301,414,19), `INNER` (0,15,414,290))
uses the stock frame **F = (13,20,401,281)** and graph (14,21,399,279). The
401x273 graph at y 28 from combined-view-design §11 belonged to the removed
pre-stock UI (commit 29ba03c). The rects below are absolute for F. Each is
also given relative to the frame's bottom-left/right inner corner, so it
moves with the frame if the frame ever moves (for example in the combined
view, top 76).

Where markers can appear (they constrain any new overlay):
* top graph row y 21..32: the T marker, the X cursor markers and the FFT
  marker. Each can be anywhere along x;
* the FFT label at (214,33,60,17);
* the strip x 1..12: CH1 zero, trigger level, math and the Y cursors. They
  clamp to y 20 (above) or y 288 (below). Their 32x32 press zones reach
  x -9..22.

Consequences:
* **No persistent overlay can go in the top corners** without colliding with
  the T or X-cursor markers when the trigger position or a cursor is near an
  edge. The top-left corner also overlaps the zones of strip markers clamped
  at the top.
* **The bottom corners are free of markers** in every state. Only the trace,
  the grid and the guide/cursor lines (content, drawn under overlays) pass
  there, plus the strip marker zones at x <= 22.

Options considered:

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| Status quo: invisible halves | Largest possible target (Fitts). One thumb. Owners who learned it love it. Zero clutter. | Not discoverable (owner; FNIRSI review). A tap that misses a marker zone by 1 px steps the timebase. | Keep the behaviour, add the hint (A). |
| Halves + transient value-preview tabs + badge flash (**A**) | Fixes discoverability the way GW/Owon do. Keeps muscle memory. Stock look at rest. | Accidental steps remain (they are visible now and undone with one opposite tap). | **Recommended default.** |
| Corner-only zones with persistent tabs (**B**, owner's idea) | Explicit, always visible. Taps elsewhere do nothing, so no accidental steps. Matches scope-ui-plan §2.4. | Smaller target than a half. Two permanent overlays on the trace. Changes a learned behaviour: a tap in the middle no longer works. | **Settings option**, ready to become the default. |
| Tap the timebase badge → spinner popup | The industry convention (§1.4 #2). Big jumps (10 ns → 1 s) in one drag. A list shows the range and the roll threshold. | 17 px-tall target. One more popup. | **Add in both designs.** |
| Tap the V/div badge → CH1 popup (as B2) | Consistency: every badge is tappable. | None (same popup as B2). | Add. Do not invent a second V/div path. |
| Pinch (exists) | Bench-scope convention. Already implemented with a bubble and a scaled preview. | Needs a second hand. Untested multi-touch quality on this panel (combined-view-design §6.5). | Keep as the secondary path. |
| Swipe along an axis edge (strip / measurement bar) | No new pixels. | A swipe on the measurement bar is the view swipe ("Scope swipes" setting). An edge swipe on the strip collides with the marker drags. Invisible again. | Reject. |
| Repurpose the math pill B4-B6 as a time pill while math is off | Obvious 60x40 buttons in the stock pill style. | Changes the stock look. Comes back into conflict when math/FFT lands. Two +/- pills invite the UF-10 direction confusion. | Reject (or keep as a fallback if A/B fail a test). |
| Hold a zone = auto-repeat (Scoppy) | Fast multi-step without a popup. | Long-press on a marker is already "recentre". Off a marker it is free. | Add to B's corner zones only. |

---------------------------------------------------------------------------

## 3. Designs (pixel geometry, Scope page, frame F = (13,20,401,281))

Shared element: the **timebase preview tab**. It shows the value the next tap
on that side will set: the left tab shows the next slower label, the right tab
the next faster one. Showing the value removes the "+ means bigger or
smaller?" ambiguity that owners already hit on the V/div buttons (UF-10).

| Item | Left tab | Right tab |
|---|---|---|
| rect (absolute) | (23, 280, 50, 17) → x 23..72, y 280..296 | (360, 280, 50, 17) → x 360..409, y 280..296 |
| rect (relative to F; F.x+F.w = 414, F.y+F.h = 301) | (F.x+10, F.y+F.h−21, 50, 17) | (F.x+F.w−54, F.y+F.h−21, 50, 17) |
| margins | 9 px to the frame border (x 13), 3 px to it at y 300; starts right after the strip markers' press zones (x <= 22) | 3 px to the frame border (x 413 / y 300); 7 px to the panel's B7 (x 417) |
| text | "<" + `TB_LABELS[tb+1]`, font 0x0804f24c (12 px), align 8, white (e.g. "<500us") | `TB_LABELS[tb−1]` + ">" (e.g. "100us>") |
| style at rest | measurement-cell style: rbox r5 0x4208 + 1 px ring r5, ring in the timebase colour **0xb833** | same |
| style pressed / just stepped | rbox r5 filled 0xb833, text white (the timebase badge's own style) | same |
| at the limit | hidden (TB_MAX, or `TB_YT_MAX` without roll) | hidden (TB_MIN) |

The widest label in F12 is "100ms" / "500us", about 32 px. With the arrow it
is about 40 px, so 50 px leaves about 5 px on each side (>= 3 px rule). The
arrows `<` (0x3c) and `>` (0x3e) exist in the font's own glyph table (dumped
as `tools/stock-ui/assets/glyphs/font_0804f24c_3c.png` / `_3e.png`). The 17 px
height matches every stock badge and cell. Roll: the slower tab shows the roll
entry's label; the record bar already shows "ROLL" once roll is active.

Shared element: the **badge flash**. On every timebase step (tap, picker,
pinch end) the timebase badge (79,1,62,17) is drawn inverted for 150 ms: rbox
r5 white, "H" and the text in 0xb833. One badge repaint, no graph redraw.
(fw-rs already beeps 30 ms on the tap step; stock does not beep.)

Shared element: the **timebase spinner popup** (tap the timebase badge).
* **Hit zone:** (79,0,62,20) only. Do not extend it into the graph: the T
  marker zone can be there. The badge test must run before the `INNER` scope
  touch, because INNER starts at y 15.
* **Box:** (79,20,132,192) → x 79..210, y 20..211, fill 0xad55 (stock popup
  box).
* **Header:** (80,21,130,29) 0x5acb, "Time/div" in 0xffe0, 16 px font
  0x0804f260, centred (as the CH1 popup header).
* **Spinner:** the stock widget at (85,55,120,150): rbox r8 0x4208, 5 rows of
  30 px, centre band (85,115,120,30) 0x059e, item text 0xd679, the in-band
  text white.
* **Items:** `TB_LABELS[TB_MIN..=TB_MAX]`. Entries beyond `TB_YT_MAX` (roll
  only) in 0xfd20.
* **Input:** drag scrolls; a tap on a row selects it. The value applies when
  the spinner settles: one `scope_set` per settled row, not per pixel.
* **Close:** a tap outside the box, MF or MENU (stock popup rules). RUN/AUTO
  are swallowed.
* **Margins:** spinner to box 6 px (x) and 7 px (bottom). The box starts
  under the top bar (y 20) and covers only the left third of the graph
  (x 79..210), so the trace stays partly visible while scrolling.
* **Optional:** a long-press on the badge = default 200 us/div (index 13),
  the Scoppy convention.

The V/div badge (286,0,62,20) tap opens the CH1 popup (same as B2). B1/B3 stay
the V/div steppers.

### 3.1 Design A (recommended default): stock halves, made visible

Behaviour is the stock rule, unchanged: a tap (<= `TAP_TOL` 4 px, one finger,
no marker zone hit) inside `INNER` steps the timebase: x <= 207 slower, else
faster (`scope_page.rs` `sp_release`).

When the tabs show:
1. **On press** off a marker zone inside `INNER`: both tabs at rest style, the
   tab of the half under the finger in pressed style. It previews what a
   release will do. Once the touch becomes a drag (beyond 4 px) or a pinch,
   the tabs hide on the next redraw.
2. **After a step**: both tabs for `GUIDE_MS` (2 s), the stepped side in
   pressed style for 150 ms. This uses the existing guide-line timer: no new
   timer.
3. **On page entry**: both tabs for 2 s on the first 3 entries after power-on
   (a counter in RAM). This teaches the halves without permanent clutter.

At rest the page is pixel-identical to stock.

Mockup (ASCII, the bottom of the graph after a tap on the left half at
200 us/div):

```
x=13                                                              x=413
 |  ... trace / grid ...                                             |
 | +------------+                                    +------------+  |   y 280
 | |   <500us   |  (filled 0xb833, just stepped)     |   100us>   |  |   (ring 0xb833)
 | +------------+                                    +------------+  |   y 296
 +-------------------------------------------------------------------+   y 300 frame
 [ Freq: ...  ][ Vp-p: ... ][ Cycl: ...  ][ Vrms: ... ]                  y 302 cells
```

Pros:
* No relearning; the praised behaviour is kept.
* The hint appears exactly when the user touches the graph.
* The value preview answers "which way is faster".
* Small code: an overlay in the graph flush plus two rect hit tests.

Cons:
* A missed marker grab still steps the timebase. It is visible now (flash +
  tabs) and is undone with one tap on the other side.
* The tabs briefly cover 2 x 50x17 px of the trace's bottom corners.

### 3.2 Design B (Settings option, owner's proposal): corner zones

Setting "Tap step: Halves / Corners / Off" (Display or Settings tab, stored).
With **Corners**:

| Zone | Hit rect (absolute) | Relative to F | Size |
|---|---|---|---|
| left (slower) | (23, 228, 80, 73) → x 23..102, y 228..300 | (F.x+10, F.y+F.h−73, 80, 73) | 80x73 px ≈ 13.6 x 12.4 mm |
| right (faster) | (333, 228, 80, 73) → x 333..412, y 228..300 | (F.x+F.w−81, F.y+F.h−73, 80, 73) | same |

* The left zone starts at x 23, after the strip markers' zones (x <= 22). The
  marker search still runs first (stock precedence).
* A tap elsewhere in the graph does nothing. This matches scope-ui-plan §2.1
  rule 3 ("a single tap in the graph never changes a setting").
* **The tabs are always visible**: the §3 tab rects (each inside its zone:
  the left tab x 23..72 in zone x 23..102), in rest style. They
  switch to pressed style while their zone is pressed and after a step.
* **Hold a zone >= 500 ms**: auto-repeat every 400 ms with a 30 ms beep per
  step, until release or the limit. Off a marker a long-press is unused, so
  there is no conflict with the marker recentre.
* A drag that starts in a zone is a normal drag of the selected item. Pinch
  and 2/3-finger gestures are unchanged.
* Why bottom and not top corners: top corners collide with the T and
  X-cursor markers on row y 21..32 and with the strip marker zones (§2). The
  owners' "top-left/right corner" description fits the stock halves, which
  cover the top too.

Pros:
* Always discoverable.
* No accidental steps.
* Thumb-sized targets in the two corners nearest the hands in a two-handed
  landscape grip.

Cons:
* A permanent 2 x 50x17 px overlay on the trace (the stock look changes at
  rest).
* Owners used to tapping mid-screen lose that.
* The right zone sits 4 px from the right panel (x 413 vs 417). The TRIG
  button (417,280,60,40) is adjacent to the right tab, so a thumb aiming at
  one can hit the other. Both actions are harmless and reversible.

### 3.3 Interaction table after the change (Scope page)

| Touch | Where | Result |
|---|---|---|
| press | marker zone (32x32) | select (stock) |
| tap | off a marker, in INNER | A: halves step + tabs/flash. B: corner zones step; elsewhere nothing |
| hold | B corner zone | auto-repeat steps |
| long-press | marker | recentre that item (fw-rs) |
| drag | anywhere in INNER | stock per-event drag of the selected item |
| pinch | graph | timebase / V/div with bubble (fw-rs), badge flash on release |
| tap | run badge / trigger badge | RUN/STOP / Trigger tab (fw-rs) |
| tap | **timebase badge** | **timebase spinner popup** (new) |
| tap | **V/div badge** | **CH1 popup** (new, = B2) |
| tap | B1/B3, B4/B6, B2/B5, B7 | stock |

---------------------------------------------------------------------------

## 4. Recommendation

1. Implement the **shared pieces**:
   * preview tabs;
   * badge flash;
   * timebase-badge spinner popup;
   * V/div badge → CH1 popup.

   They help in both designs, and the popup is the convention every surveyed
   touch scope follows.
2. Default to **Design A**. It is the stock behaviour, which owners learned
   and praise, plus the visible hint that the surveyed vendors who designed
   zone taps on purpose (GW Instek, Owon type) pair with it.
3. Ship **Design B** as "Tap step: Corners". Its tabs and hit tests are a
   subset of A's code. If the owner prefers corners after trying both on the
   device, flipping the default is a one-line change.
4. Keep the pinch as is. Do not add a fine/vernier mode: the DM40 hardware has
   discrete V/div ranges and timebases.
5. Before merging, the new elements must pass `tools/ui-render`:
   * every timebase label in both tabs;
   * the tabs against the meas cells, the strip and the panel;
   * the popup against the top bar.

Supersedes:
* scope-ui-plan.md §2.4 (60x40 top corners with "‹ ›" hints; the top
  corners collide with the T / X-cursor markers and the strip marker zones,
  and "‹ ›" are not in the stock fonts, while `<` `>` are).
* §2.6's "pinch-zoom is not proposed": pinch is implemented
  (`scope_page.rs`; combined-view-design §6.3).

---------------------------------------------------------------------------

## 5. Open points (measure on the device)

1. Grip: with the probe in one hand, which corners does the other thumb reach?
   Ask 3-5 owners to tap each corner. If the bottom-right is far, move B's
   right zone up, and check the T marker conflict first.
2. Mis-step rate in Design A: log taps that stepped within 300 ms after a
   press that hit no marker zone but landed within 10 px of one
   (`DBG:GEST?`). If it is frequent, add a 6 px dead band around the marker
   zones for the step only.
3. Whether the press preview (tabs on press) is noise during drags. The tabs
   hide as soon as the touch becomes a drag, but the first ~10 ms draw them
   once. If that flickers, show them only after 120 ms of a still press.
