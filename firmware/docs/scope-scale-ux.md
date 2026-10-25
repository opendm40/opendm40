# Scope timebase and V/div by touch

How the opendm40 scope page steps the timebase and V/div by touch (the tap
step, the preview tabs, the timebase list, the tappable badges), followed by
the survey of other touch scopes and scope apps that the design came from.
Code: [`src/scope_ui.rs`](../src/scope_ui.rs) (tap-step section, `Popup::Tb`)
and [`src/scope_page.rs`](../src/scope_page.rs). Host tests:
[`tools/touch-test`](../tools/touch-test/main.rs) (`tap_step_tests`) and
[`tools/ui-render`](../tools/ui-render/main.rs) (section 0a). Implemented in
831678f (UF-07 in [user-feedback.md](user-feedback.md)); untested on the
device. The stock behaviour it extends: `stock/re/docs/stock-scope-ui.md` (in the private analysis repo)
§10.3.

---------------------------------------------------------------------------

## 1. Current behaviour (Scope page and DMM+Scope)

All rects are absolute. Both views share the frame's bottom edge (y 300), so
the tabs and zones are the same in the combined view. Frame F = (13,20,401,281),
graph (14,21,399,279), `INNER` (0,15,414,290).

### 1.1 Tap-step modes

Settings page 2 "Scope tap step" cycles Corners -> Halves (stock) -> Off.
**Corners is the default** (`scope_ui::TAP_DEFAULT`). The mode is persisted
in p[1] bits 28..29 as the mode XOR the default, so a record written before
the setting existed restores the default.

| Mode | What a tap off a marker does |
|---|---|
| **Corners** (default) | Only the two bottom-corner zones step: `ZONE_L` (23,228,80,73) slower, `ZONE_R` (333,228,80,73) faster (80 x 73 px, about 13.6 x 12.4 mm). `ZONE_L` starts after the strip markers' press zones (x <= 22). A still hold in a zone repeats: the first repeat 500 ms after the press (`REPEAT_FIRST_MS`), then every 200 ms (`REPEAT_MS`). A tap steps on release; a hold that repeated does not step again on release. A tap elsewhere in the graph does nothing. |
| **Halves** (stock) | A tap (<= 4 px, `TAP_TOL`, one finger) anywhere in `INNER`: x <= 207 slower, else faster. |
| **Off** | No tap step. The timebase changes by the list popup, pinch or AUTO. |

Precedence (`scope_ui::graph_press`): the object under the finger wins. A
marker, cursor line or shown guide line zone is picked first, then the cursor
tab (UF-08), and only then the tap-step zone. The right-edge trigger-level
marker's zone wins over `ZONE_R` where they meet. A drag that starts in a zone
is a normal drag of the selected item; pinch and the 2/3-finger gestures are
unchanged.

Each step beeps 30 ms. At a limit nothing happens and that side's tab is
hidden. Where a tap does nothing (the graph middle in Corners, anywhere with
Off), a double tap centres CH1 and the trigger position (UF-09,
`sp_centre`).

### 1.2 Preview tabs

`TAB_L` (23,280,52,17) and `TAB_R` (338,280,52,17), in the bottom graph
corners (`scope_ui::tb_tab`). Each shows the value the next step on its side
sets: the left tab "<" + the next slower label (e.g. "<500us"), the right tab
the next faster label + ">" (e.g. "100us>"). Font 0x0804f24c (12 px), white.
The arrows `<` / `>` are in the stock font's glyph table. The tabs are 52 px
wide: the widest label "<500ms" is 45 px. `TAB_R` ends before the right-edge
marker column (x 401..412). The cursor tab `CUR_TAB` (108,280,54,17) sits
between them in the same style.

* At rest: rbox r5 0x4208 with a 1 px r5 ring in the timebase magenta 0xb833
  (the measurement-cell style).
* Pressed (the side armed by the finger) or just stepped (150 ms,
  `FLASH_MS`): filled 0xb833.
* When they show (`scope_ui::tabs_of`): Corners always; Halves while a press
  is armed, for 2 s after a step (`GUIDE_MS`) and for 2 s on the first 3 page
  entries after power-on (`INTRO_ENTRIES`); Off never.
* A tab whose step is at the limit is hidden (slower at the highest timebase
  the scope accepts, `tb_top`: `TB_MAX` with roll, else `TB_YT_MAX`; faster at
  `TB_MIN`).

### 1.3 Badge flash

Every timebase change (tap step, list, pinch, AUTO, SCPI) draws the timebase
badge (79,1,62,17) inverted for 150 ms: white box, magenta text. Entering a
scope page does not flash.

### 1.4 Tappable top bar

The stock top bar is not tappable; opendm40 makes the badges tappable
(`scope_ui::top_badge`, tested before `INNER`, which starts at y 15, so a
badge wins over the top 5 rows of a marker zone there):

| Badge | Tap |
|---|---|
| run (13,1,62,17) | RUN/STOP |
| timebase (79,1,62,17) | timebase list popup |
| V/div (286,1,62,17) | CH1 popup (same as B2) |
| trigger (352,1,62,17) | settings popup, Trigger tab |

B1/B3 on the right panel stay the V/div steppers.

### 1.5 Timebase list popup

`Popup::Tb`, drawn by `scope_ui::draw_tb`:
* box `TB_BOX` (79,21,132,191) in 0xad55 (stock popup box). The first row is
  y 21: y 20 is the frame line in the same colour, so it looks the same and
  keeps 3 px from the badges;
* header (80,22,130,29) 0x5acb, "Time/div" in 0xffe0, 16 px font;
* the stock spinner widget `TB_SPIN` (85,55,120,150): 5 rows of 30 px,
  centre band 0x059e;
* items: every timebase from the list's floor (`TB_MIN`, or `WAVE_TB_MIN`
  while the DMM wave view runs) to `TB_MAX`. Roll-only entries (beyond 1 s/div,
  `TB_YT_MAX`) are orange 0xfd20. Picking one turns Roll on, except in Single
  mode, where the scope takes the YT maximum and Roll is left alone
  (`scope_ui::tb_pick`);
* input: a drag scrolls one row per 30 px (`TB_ROW_PX`); a still tap picks its
  row. The pick applies on release, one `scope_set` per pick, with a 30 ms
  beep;
* a tap outside the box closes it.

The box covers only the left third of the graph, so the trace stays partly
visible while scrolling.

### 1.6 Pinch

Horizontal = timebase, vertical = V/div, 1-2-5 steps, a value bubble and a
scaled preview of the shown frame; applied on release (module doc of
[`src/scope_page.rs`](../src/scope_page.rs)). There is no fine/vernier mode:
the hardware has discrete V/div ranges and timebases.

### 1.7 Why this shape

* Zone taps are a known pattern (§2.4 #4). Vendors who designed them on
  purpose (GW Instek, the Owon type) show a visible hint; FNIRSI 1013D and the
  DM40 stock have invisible halves, and the FNIRSI review calls them
  "difficult to discover". The preview tabs are that hint, and their values
  answer "which way is faster" (the UF-10 confusion on the V/div buttons).
* Corners avoid accidental steps: a tap that misses a marker zone does
  nothing. The stock halves stay available for owners who learned them
  (user-feedback.md P7).
* Bottom, not top, corners: the top graph row y 21..32 holds the T marker,
  the X-cursor markers and the FFT marker anywhere along x, and the top-left
  corner overlaps the zones of strip markers clamped at the top. The bottom
  corners hold no markers in any state.
* Tapping the scale readout is the near-universal second path (§2.4 #2):
  Micsig, Siglent, Tektronix, Keysight, Scoppy and Aeroscope have it.
* Pinch is standard but needs a second hand; on a handheld most use is by
  one thumb, so it is the secondary path.

Rejected: a swipe along the strip or the measurement bar (collides with the
marker drags and the "Scope swipes" view swipe), and reusing the math pill
B4-B6 as a time pill (changes the stock look, conflicts with math/FFT).
The earlier proposal in
[scope-ui-plan.md](../../docs/history/scope-ui-plan.md) §2.4 (60x40 top
corners with "‹ ›" hints) is superseded: the top corners collide with the
markers, and "‹ ›" are not in the stock fonts.

### 1.8 Open points (measure on the device)

1. Grip: with the probe in one hand, which corners does the other thumb
   reach? If the bottom-right is far, move `ZONE_R` up, and check the T marker
   conflict first.
2. Halves mis-step rate: taps that stepped after a press that hit no marker
   zone but landed within 10 px of one (`DBG:GEST?`). If frequent, add a 6 px
   dead band around the marker zones for the step only.
3. Halves press preview: the tabs show on press and hide once the touch
   becomes a drag. If that flickers, show them only after 120 ms of a still
   press.

---------------------------------------------------------------------------

## 2. Survey of other scopes and apps

Background for §1 (2026-10-14). Source tags: **manual** = the vendor manual
or help PDF (page given), **forum** = owner posts (EEVblog etc.),
**review** = a published review, **marketing** = datasheet or shop text only,
**secondary** = only a search snippet or a secondary copy.

### 2.1 Touch scopes with a manual read

| Instrument | Scale (time/div, V/div) | Position | Trigger level | Gesture conflicts | Source |
|---|---|---|---|---|---|
| **Micsig TO series** (tablet, touch only) | **On-screen +/- buttons** beside the timebase readout and beside each channel's V/div readout, 1-2-5 steps. **Tap the readout** to open the timebase list ("Time Base Knob": tap a value). **No pinch** for scale in this manual. | 1-finger drag in the graph is coarse. A **"Fine" button** in the lower-left corner nudges the last-adjusted item (position, level, trigger point, cursor). | Slide up/down in a **dedicated trigger-level slide bar** on the right edge. A level line shows while adjusting. Swipe left from the bar opens trigger shortcuts. | Separate areas per object: graph = position, right bar = trigger level, bottom/top edge swipes = menus. Two fingers = cursor linkage. | manual: TO user manual V1.0 2022.06, p.30-31 (items 12, 13, 18, 20), §3.1-3.2 p.51-53, §4.2-4.3 p.75-76, §5 p.99-101 ([PDF](https://www.batronix.com/files/Micsig/New-Micsig-Product-Files-08-2023/(New)-TO-series--Oscilloscope/Micsig-tablet-oscilloscope-TO-series-user-manual.pdf)) |
| Micsig, owner feedback | Owners asked for a pop-up **matrix of timebases** ("more suitable for changing or selecting timebase quickly on a touch-screen"). A complaint: a timebase change zooms about the screen centre and loses the trigger point. Owners found the multi-finger zoom not obvious ("had to read the manual"). They like the scale shown next to its +/- buttons. | | | | forum: [wishlist](https://eevblog.com/forum/testgear/micsig-to1104-wishlist/msg1656878), [TO1104 review](https://eevblog.com/forum/testgear/micsig-to1104-(similar-to-rigol-1104z)/msg1261690), [p.2](https://eevblog.com/forum/testgear/micsig-to1104-(similar-to-rigol-1104z)/25) |
| **Siglent SDS800X HD** (7", knobs + touch) | **Pinch/spread** horizontally = timebase, vertically = V/div ("vertical gain"). **Tap the channel descriptor box**: a quick dialog with **▲/▼** steps, a **coarse/fine** checkbox, and a value field (universal knob or virtual keypad). Touch scaling **switches V/div to fine mode automatically**. Same for the timebase descriptor. | Drag the trace. | Drag the trigger level indicator. | Cursors are grabbed by touching the cursor itself. Their ΔX/ΔY readout region is dragged to move both. "Gestures are quick but not precise; use the knob for fine". Zoom window: double-tap sets the zoom centre. | manual: SDS800X HD user manual EN01A §7.3 p.27, §7.7 p.34, §11.2 p.45, zoom p.67, cursors p.113 ([PDF](https://www.batronix.com/files/Siglent/Oszilloskope/SDS800X-HD/4-Manuals/SDS800X_HD_UserManual.pdf)); forum: horizontal pinch snaps 1-2-5, no vernier ([rf-loop](https://eevblog.com/forum/testgear/sds800x-hd-review-demonstration-thread/msg5363645)) |
| **Tektronix 2 Series MSO** (10.1", 4/5/6 Series similar) | **Single-tap a badge** shows **Scale +/- buttons on the badge**. Double-tap opens its configuration menu. **Pinch** horizontally or vertically on the waveform view. Long-press = right-click menu. | Drag in the waveform view. | Level knob, menu, drag. | Pinch/drag in the zoom view changes only the zoom. Flick a badge off the bottom edge removes it. A **Touch Off** key. | manual: 2 Series MSO help 077-1716-08, Badges p.36, Table 3 p.47-48, zoom UI ([PDF, DE wrapper, EN text](https://www.datatec.eu/media/fe/f9/2c/1777276826/Tektronix-MSO2-Serie-HB-1_DE.pdf)) |
| **Keysight InfiniiVision 3000T** (8.5") | **No pinch.** Touch the scale/offset values of a channel, or the horizontal scale/delay values, to open a **scale dialog** (knob or keypad). | Drag horizontally = delay, vertically = offset. **Flick** for fast browsing. Drag the ground reference icon = that channel's offset. | Touch the level value → dialog. | **Mode switch in the upper-right corner**: drag mode vs rectangle-draw mode (zoom / zone trigger). A [Touch] key turns touch off. | manual: 3000T user's guide p.48-50, 55, 57-59 ([PDF](https://www.batronix.com/files/Keysight/Oszilloskope/3000XT/3000XT-Manual.pdf)) |
| **R&S RTB2000** (10.1"; RTM/RTA similar) | **Spread/pinch** vertically = V/div of the selected trace, horizontally = timebase (span in FFT). | 1-finger drag in the diagram = position; drag the markers for each trace. | Drag the trigger level marker. | Two-finger swipe = history segments. A **TOUCH LOCK** key. | manual: RTB2000 user manual §3.3.4 p.30-31 ([PDF](https://www.batronix.com/pdf/Rohde-Schwarz/RTB2000/RTB_UserManual_en.pdf)) |
| **GW Instek GDS-200/300** (handheld, 5" touch) | **Pinch** vertically = V/div, horizontally = timebase. A **scale indicator pops up**. **While it is visible, tapping the upper/lower half (V/div) or the left/right half (timebase) steps the scale.** | Swipe = position. A popup shows the value. **Snaps to zero at each crossing** (swipe again to pass). Tapping the upper/lower part also nudges the position. | **Swipe in the strip right of the grid.** Tap the trigger indicator to toggle the source. | A **Gesture Control icon** in the graticule corner switches the gestures between waveform and cursor control. | manual: GDS-200/300 user manual 2020-06, p.41-46 ([PDF](https://static.eleshop.nl/mage/media/downloads/GDS-200_300_User_Manual_EN_20200617.pdf)); patent: [US9582185](https://patents.google.com/patent/US9582185) (Good Will) |
| **Owon-type "SingleZoom/DoubleZoom"** (Multicomp Pro MP720105 family; Owon-built: inferred) | **DoubleZoom**: pinch H = timebase, V = V/div. **SingleZoom**: **tap anywhere shows a "touching control panel"** with labelled zones: upper-left/lower-left = CH1 V/div up/down, upper-right/lower-right = CH2, **left = slower, right = faster**. | Swipe up/down = position of the selected channel. Swipe left/right = horizontal position. | Swipe near the trigger pointer on the right. | **Double-tap = Run/Stop.** A touch on/off icon at the top right. | manual: "Junior User Guidebook" p.17-23 ([PDF](https://farnell.com/datasheets/4089179.pdf)) |
| **FNIRSI 1013D** (7" tablet, touch only) | **Tap the right half of the graph = faster, left half = slower** (exactly the DM40 stock rule). V/div via the side bar: [CTRL] → V+ / V-. | Tap the channel indicator → its control bar. A fast/slow movement-speed toggle. | Via the trigger control bar. | Touching freezes the display (forum). | manual: [manualslib 3999283](https://www.manualslib.com/manual/3999283/Fnirsi-Fnirsi-1013d.html); review: "changing the time base requires tapping specific sides of the screen ... difficult to discover without reading the manual" ([review](https://fnirsioscilloscopes.store/review-hub/fnirsi-1013d-review-full-hands-on-evaluation/)); forum: [EEVblog thread](https://www.eevblog.com/forum/testgear/fnirsi-1013d-100mhz-tablet-oscilloscope/msg5995451/) |
| **DM40 stock V2.6** (3.5", 480x320) | Tap off a marker zone: **left half slower, right half faster**. Right-panel B1/B3 = V/div +/-. No pinch. Top bar not tappable. | Select a marker by press (32x32 zones), then drag. Per-event axis choice. | Trigger level marker on the left strip, dragged after selection. | Marker zones win over the tap step. A tap that wobbles 1-3 px also nudges the selection. | `stock/re/docs/stock-scope-ui.md` (in the private analysis repo) §10 |

### 2.2 Touch scopes with secondary sources only

| Instrument | What was found | Source |
|---|---|---|
| Rigol DHO800/900/1000, MSO5000, MHO5000 | Tap, drag, **pinch & stretch**: horizontal = timebase, vertical = V/div. Drag the trigger cursor = level. Drag a waveform = position. These models also have knobs. | secondary: quick guide on [manualslib (DHO800)](https://www.manualslib.com/manual/3194042/Rigol-Dho800-Series.html), [manualslib (MHO5000)](https://www.manualslib.com/manual/3657782/Rigol-Mho5000-Series.html) |
| Teledyne LeCroy WaveSurfer (MAUI OneTouch) | Drag, drop, pinch, flick. Pinch on a trace zooms. Drag the trigger level indicator. Drop a channel descriptor onto the trigger descriptor = trigger source. **Expanded touch area around cursors**. Flick a descriptor off-screen = close. | vendor blog: [the evolving user interface: cursors](https://blog.teledynelecroy.com/2016/05/the-evolving-user-interface-cursors.html) |
| Tek 4 Series B | Double-tap a badge = configuration menu. Drag = position, pinch = scale. | marketing/quick start, see the Tek row above |
| Keysight InfiniiVision 4000X; Tek 4/5/6 Series | Same touch model as the 3000T / 2 Series rows above (same vendor UI families); not read separately. | — |
| Siglent SDS1000X-E | No touch screen on this series (knobs + softkeys); SDS800X HD / SDS2000X Plus are the touch models. | — |
| FNIRSI 1014D, FNIRSI 2C23T, BSIDE, Sainsmart pocket scopes, Owon VDS (PC/USB) | Not surveyed: no touch-scale documentation found in the time spent. | — |
| Hantek 2C/2D, FNIRSI DPOX180H, Owon HDS, Zeeweii DSO3D12 | **No touch screen.** Button pairs for time/div, V/div and level. The DPOX180H has 26 keys, and "knobs are replaced by Up/Down buttons". | review: [Elektor DPOX180H](https://elektormagazine.com/review/fnirsi-dpox180h-2-in-1-digital-phosphor-oscilloscope-review), [Elektor HDS2102S](https://www.elektor.com/blogs/review-blog/owon-hds2102s-handheld-2-channel-100-mhz-oscilloscope-multimeter-signal-generator-review); forum: [portable list](https://eevblog.com/forum/testgear/portable-oscilloscopes-list/msg5654619) |

### 2.3 Mobile and tablet scope apps

| App | Scale | Position / trigger | Source |
|---|---|---|---|
| **Scoppy** (Android, RP2040 front end) | Five ways for time/div: **pinch** horizontally; **+/- buttons** (tap = one step, **hold = repeat**); **tap the TIME/DIV value** = slider or an exact-value dialog; **long-press TIME/DIV = reset to default** (10 ms/div). V/div is the same (pinch vertically, +/-, tap = dialog, long-press = default). | Tap the ground indicator to select a channel. | manual: [Horizontal-Scale](https://oscilloscope.fhdm.xyz/app-help/Horizontal-Scale), [Vertical-Scale](https://oscilloscope.fhdm.xyz/app-help/Vertical-Scale) |
| **Aeroscope** (iPad) | **Pinch** H/V. The **current scales are shown on buttons** on the right that step them. | 1-finger drag H = trigger delay, V = offset. **Trigger level = a slider on the right edge of the plot.** | vendor: [iPad application overview](https://crowdsupply.com/aeroscope-labs/aeroscope-wireless-oscilloscope/updates/ipad-application-overview) |
| SignalScope Pro (iOS) | Pinch **inside the horizontal-axis label region** changes the time scale. A user preferred typing the timebase to pinching. | — | secondary (search snippet of [faberacoustical forum](https://faberacoustical.com/vforum/discussion/comment/414); the page did not load) |
| PicoScope 6/7 Automotive (touch PCs/tablets) | Pinch = zoom, as on phones (touch separated from mouse emulation in an update). | Pan by drag. | forum: [picoauto](https://picoauto.com/support/viewtopic.php?p=42271) |
| LabNation SmartScope | "Pointing, pinching and swiping" for scaling, panning and triggering. Mapping not documented. | — | marketing: [Elektor](https://elektor.com/labnation-smartscope-usb-oscilloscope) |
| Digilent WaveForms Live, Hantek 6022 Android apps, "Oscilloscope Pro" | No gesture documentation found. WaveForms Live uses an instrument panel with fields plus autoscale/centre-on-trigger buttons. | secondary: [App Store](https://apps.apple.com/app/id1244242035) |

### 2.4 Patterns, by frequency

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
