# Device check 2026-10-21 (a2f4b01, then 16b3eb9)

The checks ran on the DM40 over USBTMC (`tools/dm40dbg.py`). UI input came from DBG:TAP, DBG:DRAG and DBG:KEY.
Screenshots are DBG:LCD? reads. They are in the session scratchpad (`dev-a2f4b01/`, `dev-16b3eb9/`) and are not in git.

**Builds**
- Part A ran on **a2f4b01**, the image installed at the start.
- The coordinator then flashed **16b3eb9**. Part B ran on that image.
- **24f7080 (HEAD) is not on the device.** The image was built in a clean worktree and passed `dm40dbg.py image`:
  - header `0x7DC78238 0x0006DEF4 0x00013CD5`, 3518 x 128 blocks;
  - body 450 244 B, 188 732 B free.

  The install (`tools/reload.sh --image`) was refused by the session's permission classifier as a "production deploy".
  It was not retried. As a result, these 24f7080 items are **not verified**:
  - DLG logs, MMEM:DATA?;
  - the LUN-1 refresh;
  - READ? <= 850 ms;
  - the ohm mode tag;
  - the temperature page;
  - the resolution tap.

  No TODO items were ticked.

**Test signal.** The scope probe and the V/COM jacks are both on the MR9270S+ output (`/dev/ttyACM1`).
- Calibrator func 2 mode 0 **value 40000** = a 40.00 Hz, 0..10.4 V square wave.
- **Value 4000 = 4 Hz** (i32 = centi-Hz in this mode).
- Mode 5 (the "PWM" row in `mr9270.py`) gave a ~9 kHz, ~0.4 V signal.

**Final calibrator state:** 1 V DC, enabled (func 1 mode 0 value 1000).

## Summary

| # | Check | Result | Evidence |
|---|---|---|---|
| 1 | Scope responsiveness, open input | PASS (see notes) | `prof-idle-1ms.txt`, `prof-idle-100ms.txt`, `prof-*-offset/level/trigpos.txt` |
| 1b | Scope responsiveness, 40 Hz PWM | PASS | `prof-pwm-*.txt` |
| 2 | Corner tap step, tabs, badge flash, limits, Halves/Off | PASS; hold repeat **not testable** by hook | `tapL/R-*`, `step-*`, `20-22-limit-*.png`, `27/31/33-*.png` |
| 3 | TB list, CH1 popup, MF tabs, RUN/STOP, AUTO | PASS | `50-53-tb-list*.png`, `54-ch1-popup.png`, `55/56-mf-*.png`, `57-stopped.png`, `60/61-auto-*.png` |
| 3b | Trigger matrix on the 40 Hz PWM | PASS | `trig-{AUTO,NORM}-{POS,NEG}-*.png` |
| 3c | AUTO from 5 us, 1 ms, 20 ms, 10 mV | PASS | `61-auto-pwm-*.png` |
| 4 | Combined view | partly: popups only, the long press was not run | – |
| 5 | Stack | **a2f4b01: 32 328 / 59 152. 16b3eb9: 37 468 / 46 048 (81 %)** | DBG:BOOT? |
| 6 | DMM on the PWM: DC / AC / AC+DC / Hz | DC, AC, AC+DC PASS; **Hz sub-harmonics after AC+DC** | `dmm-*.png` |
| 7 | 32 ms scope loop passes | explained (see below) | `prof-scope-open-1ms.txt` (16b3eb9) |
| – | Owner settings at the end | volume 0 %, backlight 10 % (the stepper clamps), Dim Never, Shutdown Never, Start page Multimeter | `21-settings-final.png` |

## A. a2f4b01

### 1. Scope responsiveness

DBG:GRAB was 0 before the run and was left at 0. All drags hit the intended item, according to DBG:GEST?. The codes are:
- 3 = CH1 offset (drag the A marker at x 7);
- 1 = trigger level (drag the T marker on the left strip);
- 2 = trigger position (horizontal drag at y 200).

**Idle, open input, AUTO sweep, 1 V/div**

| Timebase | fps | Loop max | Loop histogram | s.frame avg | s.meas | s.mtxt |
|---|---|---|---|---|---|---|
| 1 ms/div | 8.7 | 32.2 ms | 29 passes in 20-50 ms per 10 s | 4.2 ms | 10.9 ms | 10.8 ms |
| 100 ms/div | 0.3-0.4 | 32 ms | 4 passes in 20-50 ms | – | – | – |

**Idle, 40 Hz PWM, triggered (T'D):** 36.6 fps at 1 ms/div, loop max 16-22 ms. At 100 ms/div: 0.9 fps.

**Drags.** Five per target. The scpi handler cost about 12 ms per DBG:DRAG.

| | Offset touch->frame | Level touch->frame | TrigPos touch->frame |
|---|---|---|---|
| open, 1 ms/div | 94-107 ms | 94-108 ms | 95-107 ms |
| open, 100 ms/div | 2006 ms | 2006 ms | 2007 ms |
| PWM, 1 ms/div | 43-49 ms (first 195) | 24-38 ms | 30-39 ms |
| PWM, 100 ms/div | 1055-1075 ms | same | same |

Notes:
- The `gesture` row stays 0 under DBG:DRAG. The hook calls the app directly, so "gesture max" cannot be measured this way.
- With an open input, touch->frame equals the AUTO-sweep frame period (about 100 ms at 1 ms/div). This is expected, but it is slower than the doc's 10-35 ms, which assumes a triggering signal.

### 2. Tap step (Corners default)

- **DBG:TAP does not step.** It injects only `Gesture::Tap`, with no Down, so no zone is armed.
  A zero-travel `DBG:DRAG x,y,x,y,20` steps correctly: (60,265) slower and (370,265) faster.
- **Feedback.** The tab turns pressed (magenta fill) and the badge inverts (white).
  A single LCD read with the PWM present saw the flash at 130 ms and no flash at 180 ms, which is correct (150 ms).
  - **Measurement caveat:** each DBG:LCD? sets `draw_hold_until = now + 300` in main.rs.
  - Polling the LCD every 50-100 ms therefore stretches the flash to 0.8-2.5 s and delays the repaint.
  - The long flash times I first saw were caused by this artifact, not by the firmware.
  - Without polling, step touch->frame was 30-54 ms with the PWM.
- **Hold repeat.** `DBG:DRAG x,y,x,y,1500` (and the hold argument 1500) gives **one step only**.
  The synthetic touch ends in the same loop pass, and the repeat runs on real time in `sp_update`.
  The repeat needs a real finger.
- **Limits**
  - At 100 ns/div the right tab is hidden and a step there does nothing.
  - At 50 s/div the left tab is hidden and a step there does nothing.
  - Stepping from 1 s to 2 s/div entered ROLL (Roll is on in MF → Display).
- **No-step areas.** Taps at (200,100) and (300,240) in Corners mode did not step.
- **Halves.** The tabs showed for about 2 s on page entry. Left- and right-half taps stepped.
- **Off.** No tabs and no steps.
- The setting was restored to **Corners**.

### 3. Popups, RUN/STOP, AUTO

- **Timebase badge (110,9):** the list opened.
  - A 60 px drag up scrolled from 1 ms to 5 ms and applied.
  - A still tap on a row picked it.
  - A tap outside closed the list.
- **V/div badge:** opened the CHA popup (Coupling / Probe / Color).
- **MF popup:** the Measure, Display, Trigger and Settings tabs all draw cleanly.
- **RUN/STOP:** the badge tap and the RUN key both toggle. Stopped = 0 fps; running = 36 fps.
- **AUTO, open input** (calibrator disabled): finished in 2.0 s at 1 us/div, 20 mV/div, triggered on noise (level −49.9 mV). No hang.
- **AUTO on the 40 Hz PWM** from 5 us/div, 1 ms/div, 20 ms/div and 10 mV/div:
  - every start ended at **10 ms/div, 2 V/div, level 4.88 V** (mid-swing), centred and T'D;
  - each run took 1.5-2.0 s, with Freq 40.0 Hz.
  - The DBG:GEST? AUTO log shows one `A5` record per run.
- **Trigger matrix** (Auto/Normal × rising/falling × 100 us, 1 ms, 10 ms, 100 ms/div):
  - 10 ms and 100 ms/div: stable T'D in all eight cases.
  - 100 us and 1 ms/div: Normal mostly shows WAIT, and Auto mostly shows AUTO.
  - This is expected: those windows cover only a small part of the 25 ms period.
- **Observation:** after AUTO at 1 ms/div, 2 V/div, the frame rate was about 9 fps, with s.frame 8-14 ms and s.mtxt up to 12 ms.
  The same timebase ran at 36 fps before AUTO. The level was not the cause (2-7 V gave the same rate).
  This was not investigated further.

### 5. Stack

`used=32328 size=59152` at the start of A.

### Settings touched

- During A, Backlight changed by accident from 55 to 65 % and then 75 % (mis-taps).
- On 16b3eb9 it is now set to the owner's minimum, 10 %.
- No setting change was caused by the firmware.

## B. 16b3eb9

- **Stack.** `used=37468 size=46048` (**81 %**) after these steps:
  - the scope page, the timebase list, the CHA popup and all MF tabs;
  - a corner step and AUTO on the open input.

  The stack is 13 KB smaller than on a2f4b01 (59 152 → 46 048), probably because .bss grew.
  `used` is above the ~36 000 guideline, with only 8.6 KB headroom.
  **Re-check this on 24f7080 before more .bss is added.**
- **DMM on the 40 Hz PWM, 0..10.4 V, 50 %**
  - DC V 4.8132 V and AC V 4.8099 V.
  - AC+DC 6.8030-6.8040 V. **PASS:** the expected range is 6.80-6.81 V. The first READ? after the switch returned 6.994, a settling value.
  - Hz: 40.002 Hz with duty 50.0 %. **PASS once settled.**
- **BUG: Hz shows sub-harmonics after an AC+DC → Hz switch.** The input was the 40 Hz square wave.
  - Repro: `CONF:VOLT:ACDC`, wait 4 s, then `CONF:FREQ` and poll READ? every 0.7 s.
  - For about 11 s the reading alternates: 40.002, **13.334, 26.668** Hz (= f/3 and 2f/3).
  - The screen shows duty **100.0 %**, and the MIN/AVG stats are polluted.
  - After that it stays at 40.002 Hz (43 of 43 readings over 30 s).
  - From `CONF:VOLT:DC` the same switch is clean.
- **Possibly stale READ?.** The first READ? right after `CONF:FREQ` returned the previous session's 40.002 Hz instantly. From DC it returned 0.000.
- **The 32 ms main-loop passes** (scope page, open input, 1 ms/div)
  - There are 29 long passes per 10 s, one for each `s.meas` call.
  - A long pass runs acquisition, frame, measurement and measurement text together:
    - s.acq ≤ 2.2 ms;
    - s.frame 5.0 ms;
    - **s.meas 11.4 ms**;
    - **s.mtxt 12.3 ms**;
    - total about 31-34 ms.
  - The hardware trigger and streaming are not involved: no rows appear, and `scope` max = 33.6 ms = the sum.
  - s.mtxt costs about 40 us when the cells read "--" (triggered PWM before AUTO), but 6-12 ms when the four cells change every update.
  - **Effect on input:**
    - 400 back-to-back `DBG:PING?` round trips: p50 0.6 ms, p99 28.8 ms, max 34 ms, 15 of 400 over 20 ms;
    - so about 3-4 % of touch reports and USB requests wait up to ~33 ms;
    - an active drag is not affected, because acquisition and measurement are frozen while it lasts.
  - **Candidates:** split s.meas and s.mtxt into separate passes, or redraw only the cells whose text changed.
- **Settings at the end**
  - Key/alert volume 0 % (owner).
  - Backlight 10 %: the − stepper clamps at 10.
  - Dim screen Never, Shutdown Never, Start page Multimeter.
  - The device was left on the DMM page, DC V, reading 0.9992 V. Healthy, crash count 0.

## Not done (blocked by the missing 24f7080 install)

- DMM d.read / d.chart and the DMM loop.
- Dim-then-tap.
- Generator keypad prefill and keypad pressed feedback.
- Ohm Normal / Low current.
- FFT and s.fft.
- Save CSV → SCOPnnn.DLG/CSV.
- Pulse / runt trigger, CUR tab, double tap, magnifier, sinc.
- REC → LOGnnn.DLG/CSV and dlog.py; MMEM:DATA?; the gallery .DLG tile; the LUN-1 refresh profile.
- 2 LUNs and write protect.
- dm40scpi.py read -n 200 stress.
- READ? latency after a meter-side change.

## Fixes (emulator-verified, not yet on the device)

The emulator (Renode, 200 MIPS) ran the native image built from each commit. Commands went through USB/IP: DBG:BOOT?, DBG:PROF?, DBG:DRAG and DBG:KEY.
- **Stack walk** (the B steps): scope page, TB list, CHA popup and its tabs, MF and all four tabs, a corner step each way, then AUTO. The input was a 40 Hz 0..10.4 V square.
- **Profiles** ran for 10 s at 1 ms/div. There were two inputs:
  - "noise": open-input-like, 50 mVpp noise, AUTO sweep, Vpp and Vrms cells changing;
  - "square": the 40 Hz square.
- **Emulator vs device timing.** The emulator runs the frame paint about 1.4x faster than the device. The measurement maths ran 3.7x faster (3.1 vs 11.4 ms), so its device-to-emulator ratio is unclear. Treat the pass times below as relative.

### 1. Stack / TCM (d957027)

| | 891f5dd (before) | d957027 + rebase (after) |
|---|---|---|
| TCM .data + .bss | 22 088 B | 17 192 B |
| Stack (TCM above .bss) | 43 448 B | 48 344 B |
| Stack used, walk (DBG:BOOT? STACK) | 38 580 B | 28 532 B |
| Headroom | 4 868 B | **19 812 B** |
| Static worst path (tools/stack-depth) | 38 596 B | 28 548 B |
| main's frame | 29 608 B | 19 440 B |
| SRAM free (.dmabuf + .sram_bss) | 5 464 B | ~4.3 KB |

**What used the TCM and the stack:**
- **main's frame.** It held three things:
  - Usbtmc, 12.7 KB: `rsp` plus a second 6 KB `tx` copy of every reply;
  - Msc, 4.2 KB, with its 4 KB block buffer;
  - App, 5.5 KB.
- **.bss.** The FFT (12.3 KB, from bde5bff) was the bulk of it. The rest was the touch log (1.9 KB), EP_MEMORY (1.3 KB), the dlog vfs table (1.3 KB) and the profiler (1 KB).
- **The worst path** was main → App::tick → sp_update → panel_button → Ink::new / Ink::none, which returns about 600 B by value.

**Changes**
- **USBTMC.** Bulk-IN packets are assembled from the header and `rsp` as they go, so there is no tx copy.
  If a new reply would overwrite `rsp` while the previous transfer still has payload to send, that transfer is ended first. This only happens when the host sends a command in the middle of a read.
- **Logger ring.** Records shrank from 12 to 8 B. The seq is implied, because the ring holds consecutive seqs. The ring went from 24 to 16 KiB of SRAM, still 2048 records.
  LOG:DATA? was checked in the emulator: the seqs are contiguous and the timestamps step 25 ms.
- **`.sram_bss`.** This is a new zeroed NOLOAD SRAM section, zeroed by native.rs `init_ram` and main.rs `rust_start`. It holds EP_MEMORY, MSC_IO, the MSC block buffer (now a static passed to `Msc::new`), touch::LOG and the dlog vfs TAB.
  - All of these are CPU-only, so it does not matter that they are in SRAM.
  - TCM cannot be reached by DMA. Nothing that DMA uses moved into it.
- **Build-time guards**
  - `build.sh native` runs `tools/stack-depth/stackdepth.py --check 16384`. It takes the worst static call path from the disassembly and fails the build below 16 KiB headroom. Its result matched DBG:BOOT? STACK to within 16 B.
  - link-native.x asserts that the stack is at least 28 KiB + 16 KiB, so .data/.bss cannot eat the headroom.
- **The FFT** stays in TCM. Moving it would need 12.3 KB of SRAM, and only about 4 KB is free.
- **Pre-existing failures, unchanged:** the RAM image (`./build.sh`, link.x) and test-initcore already failed to link at 891f5dd. Their CODE and DMARAM regions overflow.

### 2. Scope main-loop passes (a33e3ad, fa3358d)

**Changes**
- **Measurement maths** (`scope_meas::compute_codes`). Each sample now costs only:
  - a histogram increment;
  - the edge state machine on integer thresholds (the same tests);
  - integer moments.

  Min, max, top, base, mean and RMS come from the histogram's exact moments. A comparison over 3000 random records matched the old code exactly, except Vavg, Vrms and Vcrm, which differ by at most 2.2e-4 relative (the old f32 running sums).
- **Measurement pass.** The pass that measures only converts the frame (`sp_frame`). The graph paint runs on the next pass.
- **Cells** follow one changed cell per pass. Unchanged cells were already skipped.
- **Graph flush.** A trace-only flush stops after 4 ms of DWT cycles and resumes at that column on the next pass. Host renders are unchanged: ui-render shows 0 violations.

**Results** (emulator, DBG:PROF?, max per 10 s)

| Section | noise before | noise after | square before | square after |
|---|---|---|---|---|
| loop max | 10.9 ms | 5.7 ms | 21.5 ms | 8.2-10.5 ms |
| s.meas | 3.1 ms | 1.4-1.5 ms | 7.7 ms | 4.6 ms |
| s.frame | 3.6 ms | 3.6 ms | 10.3 ms | 5.9-9.1 ms |
| s.mtxt (one pass) | 2.1 ms (all cells) | 2.2 ms (one cell) | 2.1 ms | 2.2 ms |
| graph flush (x1, instrumented) | – | – | – | 3.8 ms |

### 3. ~9 fps after AUTO (53bdb9d)

**Cause.** At 1 ms/div the record (26 624 samples) is barely longer than the screen (25 000), so a software trigger search only sees 0.65 ms of the 25 ms period. A fast frame rate therefore needs the comparator.
- Three comparator misses in a row disabled the hardware trigger for good (`hwtrig::note_miss`), until DBG:HWTRIG:ENABLE 1 or a reboot.
- AUTO's sweep starts at 5 us/div on fine ranges, where the 10.4 V square overdrives the input. That sweep produces the misses.
- Without the comparator almost every frame is the 100 ms Auto timeout, about 9 fps whatever the level. Before AUTO the comparator fired on every edge, giving 36 fps.

**Fix.** Misses now suspend the comparator. A change of range, timebase, coupling, edge, mode or trigger type resumes it. Level and position drags do not. A stuck TIM1 gate still disables it for good.
- touch-test has 6 new hwtrig cases.
- In the emulator, after AUTO, DBG:HWTRIG? shows enabled=1 and available=1 again, where before it was 0,0 for good.

The emulator cannot show the fps difference. Its comparator path (the ScopeFeeder TIM2 gate timing) misses even before AUTO. That is a separate emulator bug and was not fixed.

### 4. STOP then Save CSV (f09b3b0, coordinator item)

**Problem.** STOP in the middle of an acquisition (Auto with no trigger, the common case) left no stored record, so Save CSV answered "No record to save".

**Fix.** On STOP, the samples captured so far become an untriggered stored record, oldest first. This covers a partial record, the hardware-trigger ring and the stream ring. The screen stays as it was drawn.

**Emulator check:** a never-triggered sine, STOP, then Save CSV saved SCOP000.DLG (4157 B, 200 us/div) and SCOP001.DLG (4312 B, 2 ms/div).

### Device re-check list

1. **Stack.** Run the B walk, then DBG:BOOT? STACK. Expect about 28.5 K used of about 48.3 K, so 19 K or more headroom.
2. **Scope on an open input**, 1 ms/div, cells changing. DBG:PROF? loop max should be 12 ms or less, with no 20-50 ms passes. 400 DBG:PING? round trips: p99 should be below 15 ms.
3. **The 40 Hz square at 1 ms/div**, triggered: check the loop max and s.frame max.
4. **AUTO on the square**, then 1 ms/div: expect about 36 fps and T'D. DBG:HWTRIG? fields 5 and 6 should read 1,1. Drag the level a few times: the scope should stay smooth.
5. **Save CSV after STOP**: try Auto on an open input, STOP mid-acquisition with the square, and Normal while it shows WAIT. Each should give "Saved SCOPnnn.DLG", with an "untriggered" header where no trigger happened.
6. **USB**
   - DBG:LCD? screenshots (multi-round replies).
   - `dm40scpi.py read -n 200`.
   - LOG:STAT ON, then LOG:DATA?: the seqs should be contiguous.
   - MSC copy of a large file. The MSC block buffer is now in SRAM.

## 9dd1222

**Build.** The owner installed **9dd1222** before the run. At the start it was healthy: stack 24 608 / 48 336 B (51 %), crash count 0. No firmware was installed during the run.
Screenshots, profiles and CSVs are in the session scratchpad (`dev-9dd1222/`), not in git.

**The run was cut short.** The coordinator stopped it at about 00:15 on 2026-10-22 so the owner could flash a new build. These were not run:
- the AC mV and thermocouple soak modes;
- item 5 (swipe-REC, LOGnnn.DLG/CSV, MMEM:DATA?);
- item 6 (DBG:BOOT? stack after everything);
- the follow-ups listed under "Not run" below;
- the end-state restore.

**Wiring.** The calibrator (`/dev/ttyACM1`) drives the scope probe and the DMM V/COM jacks in parallel. The A jack is not wired, so AC A, T9 and T10 were skipped.
- The MR9270S+ has **no sine output.** Func 2 modes 0 and 3 give the 40 Hz, 0..10.4 V square. Mode 1 gives about 10 mVpp. Mode 4 gives a waveform that read about 16 V AC; it is undefined and was not used again.
  Every "sine" step ran with the 40 Hz square or a DC level instead.
- The calibrator's "0 V" is a low-impedance 0 V, not a short: DC V reads −17 µV on it.

**The flicker the owner reported** came from the deliberate `DBG:AD2:DRAW 2` load. DRAW 2 repaints the full DMM view every 100 ms. It ran during the loss runs in four 20 s windows:
- 23:21:41-23:22:01
- 23:22:41-23:23:02
- 23:23:42-23:24:02
- 23:24:42-23:25:02

DRAW was set back to the default 1 at 23:25:02. Right after that, DBG:PROF? showed no long passes (dmm max 2.0 ms) and the screen looked normal (`04-after-ad2loss.png`).

### Summary

| # | Check | Result |
|---|---|---|
| 1a | AD2 torn reads, 13 rungs, blind and SYNC | **PASS.** signbad 0, diff(no conv) 0, no bump at 9 / 17 bits |
| 1b | AD2 loss per rate × DRAW 0/1/2 | Numbers below. **The chop-ON rungs run faster than nominal**, so "lost 0 %" at 600-4800 is a clamp artefact. Link ceiling about 6.5 kS/s |
| 1c | AD2 noise / ENOB | Converter floor (AC A, jacks open): ENOB 16.1-17.6 (13.4 at 76.8 k). The calibrator 0 V into AC mV is input-dominated: ENOB 11.8, flat. A true shorted-mV input was not tested |
| 1d | AD2 waveforms | AC V on the square: 40.00 Hz, mean ≈ 0 (AC coupling), rms −1.6 % vs the DMM. AC mV on 100 mV DC: −1.7 % vs DC mV (calibration not applied) |
| 1e | Loop profile while logging at 9600 | loop max 2.5 ms; the logger is inside the `usb` lap (96 % CPU) |
| 2a | Scope page, open input, 1 ms/div | loop max **12.38 ms** (marginal: target ≤ 12). No 20-50 ms passes. Ping p99 0.7 ms |
| 2b | 40 Hz square 1 ms/div after AUTO | **PASS.** 40.0 fps, HWTRIG 1,1, loop max 10.65 ms, ping p99 9.3 ms. Level drags 38 fps |
| 2c | STOP → Save CSV, three cases | **PASS** on the SPI disk: SCOP000-002.DLG, "untriggered" headers, dlog.py == MMEM CSV. **FAIL: SCOPnnn.CSV did not appear on DM40-INFO** after unmount/mount (see below) |
| 3a | AutoHold re-capture | **PASS.** 1.10 → 1.20 → 1.10 → 1.12 captured; 1.11 held at 1.1210 |
| 3b | Bargraph | **PASS.** Visible, segmented, bipolar −6/0/+6 V. d.bar cost 1 µs |
| 3c | Tap value → FAST/MED/SLOW toasts | **PASS** |
| 3d | Hz after AC+DC, 40 Hz square | **PASS.** 2 × 43 reads all 40.0017-40.0018 Hz, duty 50.0 %, MIN/MAX clean |
| 3e | Continuity with 1.234 V on the jacks | **PASS.** 0.00 Ω, LIVE? badge |
| 3f | °F toggle and back | **PASS** (unit, CJ, SYST:TEMP? converted). TC K reads 192-198 °C on a 0 V source (see below) |
| 4a | T7: alerts ON vs OFF | DC V, Ω, AC mV, Hz and the AC V square: equal within noise. **FAIL: AC V 6 V range reads +37.2 mV with alerts ON** on any DC input (0.15-0.35 mV OFF) |
| 4b | No false trips, 5 min per mode, A jack open | DC V 1 V, DC mV, Ω 100, AC V square: 0 hits. **FAIL: DC V autorange 1 ↔ 10 V asserted A-JACK** (+1476 hit windows, streak 96). AC mV and TC not run (stopped) |
| 5 | Swipe-REC LOG / MMEM:DATA? | **not run** (stopped) |
| 6 | DBG:BOOT? stack after everything | **not run** (stopped). 51 % at the start |

### 1. AD2 fast logger (doc dmm-input-scope-feasibility §7.3)

The meter was in AC mV (the 600 mV button on the range page) with 0.312 V DC from the calibrator. The stream is AD2 code ≈ 64 000, i.e. 0.305 V.

**Torn reads** (`ad2 torn -n 2000`, blind and `--sync`)
- On every rung: signbad 0 and diff(no conv) 0.
- The bit-length histograms are smooth noise distributions. They peak at 6-8 bits at 76.8 k / 38.4 k and lower at the slow rungs.
- There is no separate bump at 9 or 17 bits. **No tears at any rate.**
- With SYNC, pairs at 0x04 / 0x05 / 0x24 / 0x25 / 0x2D are 2000 / 2000 identical. Pairs at 0x03 / 0x23 and faster mostly differ: the two-read pair (~140 µs) is longer than the period there.

**Loss per rate** (20 s each; DBG:AD2:DRAW d; `ad2-loss.txt`)

| requested (rung) | DRAW | captured | measured S/s | reported lost | gaps 2 / 3-4 / 5-16 / 17-128 / >128, max |
|---|---|---|---|---|---|
| 600 (0x2D) | 1 | 22 520 | 1125 | 0 % | 0/0/0/0/0, 2.1 ms |
| 600 | 0 | 22 583 | 1128 | 0 % | 0/0/0/0/0, 0.9 ms |
| 600 | 2 | 18 153 | 907 | 0 % | 0/0/200/0/0, 26.2 ms |
| 2400 (0x24) | 1 | 76 318 | 3814 | 0 % | 0/40/20/0/0, 2.0 ms |
| 2400 | 0 | 76 668 | 3832 | 0 % | 0/0/0/0/0, 0.4 ms |
| 2400 | 2 | 59 204 | 2958 | 0 % | 0/538/266/200/0, 25.8 ms |
| 4800 | 1 | 122 308 | 6111 | 0 % | 10/0/60/0/0, 2.0 ms |
| 4800 | 0 | 123 608 | 6179 | 0 % | 10/0/0/0/0, 0.4 ms |
| 4800 | 2 | 94 372 | 4715 | 1.76 % | 24/0/804/200/0, 25.7 ms |
| 9600 | 1 | 129 956 | 6493 | 32.4 % | 29024/3404/2/58/0, 2.1 ms |
| 9600 | 0 | 131 428 | 6568 | 31.6 % | 29380/3461/0/0/0, 0.4 ms |
| 9600 | 2 | 100 080 | 5001 | 47.9 % | 21466/2547/21/785/200, 26.0 ms |

**Finding: the chop-ON rungs deliver a result faster than the ladder's nominal rate.** The logger's spacing is perfectly uniform:
- 886 µs at "600" and 261 µs at "2400";
- 157 µs at "4800" (with 250 µs steps);
- that is P/2 + ~53 µs each time, i.e. AD2F is already set when the P/2 gate opens.

`DBG:AD2:NOISE?` (flag-synced) gives the same picture:
- 0x25 ("1200") and 0x05 ("2400", chop OFF) both take 422 µs per sample;
- 0x24 ("2400") and 0x04 ("4800") both take 232 µs.

So ENCHOPAD2 does not halve the AD2F rate the way `ad2_r26_for` / §7.1 assume. The labels of the chop-ON rungs are 2x low; 0x2D is 1063 µs, ~940/s, which fits neither. Because `lost = span / nominal − captured` goes negative and is clamped, **the 0 % at 600-4800 means nothing**.
- What the timestamps show directly:
  - every result is captured through 0x24 (~3.8 kS/s real, all dt = 261 µs);
  - at "4800" there are already 250 µs steps;
  - the bit-banged link tops out at **~6.2-6.6 kS/s**, not the doc's ~8 k.
- **DRAW 2** (the DMM page repaint) costs a 25.7-26.2 ms gap every 100 ms at every rate. That is ~26 % dead time, which the counter does not show below 9600.
- Not run, the discriminating check: `log --chop off --rate 2400 / 4800` (measured ≈ nominal would confirm that the chop-ON labels are the issue).
- Each stream starts with two rail samples: seq 2 = −262144, seq 3 = +262143. Without them, the stream sd is 62-70 codes (0.3 mV, mostly 50 Hz-like wander).

**Noise / ENOB**

| input | rungs | sd (codes) | ENOB | readout µs/sample |
|---|---|---|---|---|
| AC mV r1, calibrator 0 V (`ad2-noise-0V.txt`) | all 13 | 141-158, flat | 11.7-11.9 | 141 (fast) .. 1063 (0x2D) |
| AC A (600 µA r0), jacks open, op1chop 0 | 0x00 / 0x03 / 0x05 / 0x22 / 0x24 / 0x2D | 47.5 / 4.5 / 2.7 / 7.2 / 3.5 / 3.1 | 13.4 / 16.8 / 17.6 / 16.1 / 17.2 / 17.4 | |
| same, op1chop 3 | same | 47.4 / 4.5 / 2.7 / 7.2 / 3.3 / 3.1 | 13.4 / 16.9 / 17.6 / 16.2 / 17.3 / 17.4 | |

- The flat ~145 codes (0.68 mV rms) in AC mV are the calibrator's output and pickup, not the converter. The DMM's own AC mV reads 0.7 mV on it.
- The open AC A path is the converter floor, and it matches §1.2 (14-16 bits or better).
- OP1 chop moves the offset from +868 to −742 codes.
- A shorted mV input was not available.

**Waveforms**
- AC V, 40 Hz square, 2400 and 4800 (`v.csv`, `v4800.csv`):
  - 40.00 Hz;
  - mean 0.008 / −0.019 V (AC coupled, coupling AC in the header);
  - rms 4.691 / 4.713 V vs DMM AC V 4.768 V (−1.6 % / −1.2 %, uncalibrated);
  - p-p −4.78..+5.10 V.
- AC mV, 100 mV DC (`w.csv`, 4800): mean 97.1 mV vs DMM DC mV 98.75 mV (−1.7 %); rms 0.4 mV vs DMM AC mV 0.47 mV.
- During the session, DMM AC V on the same square read 4.768 V at 23:26 and 4.8076 V at 23:55-00:00 (+0.8 %). The cause is not known: the calibrator, temperature, or something else. It was unrelated to logging (checked before and after a log session).

**Profile while logging 9600** (`prof-ad2-9600.txt`):
- loop avg 615 µs, max 2.49 ms; 30 passes of 2-5 ms per 10 s;
- the logger poll is counted in the `usb` lap (590 µs avg, 95.7 %).

### 2. Scope re-checks

**Test conditions**
- "Open input" = calibrator output disabled.
- Shots 10-13 and the first profile were taken on the **combined view**. There AUTO went to the DMM.
- The pure scope page numbers are below.

**Profiles**

| view / input, 1 ms/div | fps | loop max | loop hist 10-20 ms | s.frame max | s.meas max | s.mtxt max | ping p50 / p99 / max |
|---|---|---|---|---|---|---|---|
| combined view, open (noise-triggered at 20 mV) | 49.8 | 11.37 ms | 31 | 6.5 | 7.0 | 3.5 | **11.8 / 16.5 / 17.5 ms** |
| scope page, open (AUTO sweep, level 4.88 V) | 9.1 | **12.38 ms** | 32 | 6.4 | 6.9 | 3.5 | 0.5 / 0.7 / 8.7 ms |
| scope page, square, 20 mV/div overdriven, before AUTO | 9.5 | 16.7 ms | 40 | 14.5 | 5.5 | 3.3 | 0.5 / 9.3 / 11.5 ms |
| scope page, square, after AUTO + 3 steps to 1 ms/div | **40.0** | 10.65 ms | 33 | 6.3 | 5.4 | 3.1 | 0.5 / 9.3 / 17.6 ms |
| same, during 6 level drags | 38.0 | 12.9 ms | 28 | | | | |

**Results**
- The 32 ms passes are gone. The worst is now 12.4 ms on the open scope page, 0.4 ms over the ≤ 12 target.
- **HWTRIG.** Before AUTO the square overdrove the 20 mV/div input, and the comparator was suspended (fields 5,6 = 1,0; 9.5 fps).
  - AUTO ended at 10 ms/div, 2 V, level 4.88 V, T'D.
  - After that, HWTRIG? read 1,1 and stayed 1,1 through three timebase steps and six level drags.
  - DBG:HWTRIG:STAT? read 723 hits / 25 misses.
  - **The 53bdb9d suspend/resume works.** The ~9 fps after AUTO is fixed.
  - AUTO took more than 4 s (the screenshot at 4 s still showed level 4.14 V; 4.88 V by 7 s).
- **Observation: the combined view's USB latency.** Ping p50 was 11.8 ms there, against 0.5 ms on the scope page.
- **Observation: the side keys.** RUN (key 1) and the RUN badge tap did nothing while the MF popup was open (`30-*.png`, `31*.png`). The MF key toggles the popup.
  - The scripted key sequences got out of step because of this. The scope page also ignores DBG:SWIPE: the "Scope swipe" setting is "Off (stock)". MENU → Multimeter is the way back.

**STOP then Save CSV** (MF → Settings → Save CSV)
- The first try showed "Disk in use by PC - eject first", because the PC had ATK-DM40 mounted. It was then unmounted with `udisksctl unmount -b /dev/sda`, and **left unmounted** when the run was stopped.

| case | file | size | header |
|---|---|---|---|
| Auto, open input, 1 ms/div, STOP | SCOP000.DLG | 11 544 B | mode Auto, untriggered, 2.5 MS/s, 26 624 samples, −0.15..+0.08 V |
| Auto, square, 50 ms/div, STOP during acquisition | SCOP001.DLG | 13 732 B | mode Auto, untriggered, square visible (−0.27..+10.03 V) |
| Normal showing WAIT (calibrator off), STOP | SCOP002.DLG | 12 704 B | mode Normal, untriggered, flat 0 V |

- Each case showed the toast "Saved SCOPnnn.DLG".
- `dlog.py` of each DLG was byte-identical to MMEM:DATA? "SCOPnnn.CSV" (692 550-692 553 B).
- On STOP from WAIT, the screen repainted the captured untriggered samples (flat), not the old frame.
- A real noisy 1 ms/div record is 11.5 KB.
- **FAIL: no SCOPnnn.CSV on DM40-INFO.**
  - Repro: after the saves, `udisksctl unmount -b /dev/sdb; udisksctl mount -b /dev/sdb`. The listing still held only the 9 unit files.
  - Code reading: the Save CSV job (screenshot.rs) never calls `dlog::set_stale()`. Only csvlog's finish, a delete and a host write do (main.rs ~885, csvlog.rs 592 / 1314). So `dlog::refresh` is not run after a scope save.
  - Not verified: that a later REC stop makes the SCOP CSVs appear. Item 5 was not run.

### 3. DMM

- **AutoHold** (RUN held 1.2 s; CALC:AHOL? 1; `47-ah-sheet.png`)
  - Captures: 1.1007 → 1.2005 → 1.1009 → 1.1210 (1.12 V).
  - At 1.11 V it stayed at 1.1210, as meter-test expects.
  - READ? keeps returning the live value.
- **Bargraph.** Segmented, bipolar −6 / 0 / +6 V, orange lit segments (`46-dmm.png`).
- **Resolution step** (tap on the value; `48-rate-sheet.png`)
  - MED → "SLOW 600000 counts / 6.4 s" → "FAST 60000 counts / 10/s" → "MED 600000 counts / 3.2 s".
  - RATE? follows the steps. It was left at MED.
  - The rate chip under the chart updates a moment after the toast.
- **Hz after AC+DC** (40 Hz square; CONF:VOLT:ACDC, 4 s, CONF:FREQ, then 43 READ? at 0.5 s, twice)
  - All 86 reads: 40.0017 / 40.0018 Hz.
  - Duty 50.0 %; MIN 40.0017, MAX 40.0018.
  - The 16b3eb9 bug (13.334 / 26.668 Hz, 100 % duty) is gone.
- **Continuity** with 1.234 V DC: 0.00 Ω (five reads), SYST:ALER? LIVE, amber LIVE? in the status slot.
- **Temperature unit**
  - UNIT:TEMP F: 191.9 C → 377.9 F, CJ 26.38 C → 79.45 F, SYST:TEMP? 97.4 F; the screen showed F.
  - Back to C: 192.8 C. Left at C.
- **TC K on the calibrator.** With 0 V (low impedance) or the output disabled, TC K reads 192-198 °C, independent of Jack alerts.
  - 0 mV should read about the CJ (26 °C).
  - The calibrator is not a thermocouple, and the TC path's jack wiring was not checked, so this is **not assessable here**. Check it with a real thermocouple.

### 4. Misconnect sentinel

**T7** (`t7.py`: CONF, settle, then 10 READ? each with SYST:ALER:ENAB ON / OFF / ON; `t7.txt`)

| function | input | ON | OFF | ON |
|---|---|---|---|---|
| DC V | 0 V | 26.8 µV | −7.6 µV | 153 µV (sd 110) |
| DC V | 1 V | 1.000620 | 1.000476 | 1.000552 |
| DC mV | 0 V | 59 µV | 239 µV | 352 µV (drifting, sd 40-110 µV) |
| DC mV | 0.1 V | 99.497 mV | 99.550 mV | 99.418 mV |
| Ω | open | 418.27 kΩ | 418.74 kΩ | 419.15 kΩ (drifting) |
| Ω | 100 Ω (calibrator) | 99.9546 | 99.9541 | 99.9551 |
| TC K | 0 V | 195.2 | 194.9 | – |
| AC V | 40 Hz square | 4.807473 | 4.808188 | 4.807519 |
| Hz | 40 Hz square | 40.00173 | 40.00173 | 40.00173 |
| AC mV | 0 V | 0.725 mV | 0.723 mV | 0.717 mV |
| AC mV | 0.1 V DC | 0.485 mV | 0.470 mV | 0.476 mV |
| **AC V (6 V)** | **0 / 0.5 / 1 / 2 / 5 V DC** | **37.28 / 37.24 / 37.22 / 37.18 / 37.19 mV** | **0.35 / 0.14 / 0.17 / 0.15 / 0.18 mV** | **37.22 / 37.22 / 37.22 / 37.15 / 37.17 mV** |

**FAIL: AC V on the 6 V range with Jack alerts ON reads a constant ~37.2 mV floor.**
- Repro: `CONF:VOLT:AC` with any DC (or 0 V) on V-COM.
- With alerts OFF it reads 0.15-0.35 mV. The effect follows SYST:ALER:ENAB on every toggle.
- The first screenshot of the run shows it too: AC V 0.0374 V on 1 V DC.
- On the 4.8 V square the difference is −0.7 mV.
- AC mV is unaffected.
- DBG:SENT? in AC V: `AD1PK_PB1,NONE,7,0,~1300,~1368,...`. The peak hold is well below the 17 900 threshold, so this is not a trip.
- This is the case doc §4.5 T9 names ("AD1 running + the R29 ENPKH toggle every ~107 ms must not move the RMS / LPF readouts; if it does, PKH_RESTART = false"). Here it moves the AC V RMS floor by 37 mV.

**No false trips** (`soak.py`: 5 min per mode, DBG:SENT? + SYST:ALER? every 1 s, A jack open; `soak.txt`)

| mode | hit windows | max streak | alerts seen |
|---|---|---|---|
| DC V 1 V (6 V range) | +0 | 0 | NONE |
| **DC V, calibrator 1 V ↔ 10 V every 10 s (autorange 6 ↔ 60 V)** | **+1476** | **96** | **AJACK**, NONE |
| DC mV 0.1 V (600 mV) | +0 | 0 | NONE |
| Ω, 100 Ω | +0 | 0 | NONE |
| AC V, 40 Hz square | +0 | 0 | NONE |
| AC mV, TC K | not run (the run was stopped) | | |

**FAIL: A-JACK false trip in DC V with 10 V on V-COM and nothing in the A jack.**
- A streak of 96 windows means the comparator on PB1 hit for the whole 10 s at 10 V. This is not a range-change transient.
- 1476 windows ≈ 148 s ≈ every 10 V half-period of the 300 s.
- Not isolated (stopped): which of these causes it:
  - 10 V on the 60 V range;
  - or anything above ~6 V, including 10 V on the 6 V range.

  Next step: DC V manual 60 V and AUTO+ at 5 / 7 / 10 V, polling SYST:ALER? and DBG:SENT? ctsta.
- Before the soak, the cumulative hit counter had also risen by 1-2 per function or row change, and by +23 across one DC V → DC mV switch made while the jacks still had 5 V on them (overload on 600 mV). That was not watched live.
- The counter is cumulative since boot and never reset (sentinel.rs `hits`).

### Not run (stopped by the coordinator)

- Soak: AC mV, TC K.
- The DC V / A-JACK isolation above.
- The DC mV overload trip check.
- The AD2 `--chop off` pair.
- Item 5: swipe-REC 2 min (DBG:SWIPE D,1), LOGnnn.DLG, the LUN-1 LOGnnn.CSV vs dlog.py, MMEM:DATA?, and whether the SCOP CSVs then appear.
- Item 6: DBG:BOOT? stack after everything.

### State when stopped

- **Device**
  - DMM page, AC V on the 600 mV manual range (the soak's last setup).
  - Jack alerts 7 (ON), RATE MED, UNIT C, DBG:AD2:DRAW 1.
  - Scope: Auto trigger, 50 ms/div, running.
  - Volume, backlight, Dim and Shutdown were not touched.
- **Calibrator:** 0.1 V DC, enabled. It was **not** set back to 1 V: the run stopped with no further device access.
- **Host:** ATK-DM40 (/dev/sda) **unmounted** by this run. DM40-INFO was remounted.
- Files on the SPI disk added by this run: SCOP000.DLG, SCOP001.DLG, SCOP002.DLG.

### Fixes after 9dd1222 (on 7939217; not on the device yet)

**4a. AC V floor with Jack alerts ON (sentinel.rs).** The AC V and AC mV sentinel rows differ from the stock rows in the same five AD1 fields (R22, R23 ENAD1, R24, R25 AD1IG, R29 PKHSEL) and both get the same R29 rewrite every ~107 ms, yet only AC V moved. So the R29 toggle and the AD1 sampling of PB1 / PB3 are unlikely causes. The difference is the path:
- AC V reads OP1 (SENSE x10 on 6 V -> OP1O -> PB4 -> AD2 / AD3). AC mV reads OP2O, and its OP1 sits idle on RLU.
- Every stock row has R33 = 0xac, which includes OP1CHOP = 10 (a 2 kHz chopper). The stock pairs OP1's chopper with AD1 in DC V 6 V and DC A. In AC V, AD1 is off and the chopper evidently stands still.
- When AD1 is on, OP1's chopper runs. That puts a 2 kHz square of OP1's offset (×10 on 6 V) on PB4. This is in band for the RMS block and shows up as a constant floor whatever the input. 37.2 mV on 6 V equals 3.7 mV at PB4, or 0.37 mV of offset at OP1's input. The datasheet allows up to 2 mV unchopped.
- This is inferred from the rows and the datasheet. The device re-check below settles it.

Fix: the AC V sentinel row also clears OP1CHOP (R33 bits 7:6 = 00, CHOP_CLK held at 0). AC V is AC-coupled, so OP1's static offset does not reach the reading. AC mV is unchanged.

**4b. A-JACK in DC V at 10 V.** The DC V rows for r1..r3 (60 / 600 / 1000 V) are the only comparator-sentinel rows that drive the power bus from OP3 (R2F SMODE 0100, PS6 + PS8 closed). The r0 row (6 V) leaves the bus off and never tripped.
- The likely mechanism: OP3's bus current returns near PB1, so PB1 picks up a level that depends on the input.
- Fix: DC V r1..r3 carry no sentinel (kind OFF, stock row).
- What this gives up: only "leads in A/COM on a manual 60 V+ range". With the leads in A/COM and the function on DC V, V-COM is open, so the autorange sits on 6 V, where the sentinel stays.
- Host test `dcv_high_ranges_have_no_comparator`. Emulator `senttest.py dcv-step`: 1 ↔ 10 V autorange with the A jack open shows NONE and kind OFF at 10 V, then 1 mA at 1 V shows AJACK. The model has no PB1 coupling, so this only checks that the gating does not break the r0 sentinel.

**Device-check hook.** `DBG:SENTinel:VARiant <0..7>` (RAM only) puts back parts of the 9dd1222 rows:
- +1: AC V keeps OP1's chopper;
- +2: no AC V sentinel;
- +4: comparator on DC V 60 V+.

**1. AD2 rate (hy3131.rs, logger.rs).** ENCHOPAD2 does not halve AD2's rate: 0x25 = 0x05 and 0x24 = 0x04 measured the same.
- `ad2_period_ns` now ignores the bit.
- The ladder is 1200 / 2400 / 4800 / 9600 S/s, with or without chop. 600 S/s does not exist: /4 at OSR 1024 is already 1200.
- The "600" sessions ran at 1200/s, so 1125/s captured was a real 6 % loss to the old P/2 gate. The "2400" sessions ran at 4800/s.
- Nominal, `lost` and the P/2 gate now use the real period.
- The bit-banged link (9dd1222) tops out at **~6.5 kS/s**; 9600 showed that as ~32 % lost. The SPI2 bus (E2, landed meanwhile) should lift it; the re-check measures it.
- The emulator model (Ad2Period, noise) and its tests follow.

**2. Combined view ping p50 11.8 ms.** From `prof-scope-open-1ms.txt`, this is not caused by the view itself.
- The trace was noise-triggered at 49.8 fps. Every column changed every frame, so painting cost about 10 ms per frame.
- Each frame took a 6.2 ms frame pass (which included a 4 ms flush slice), plus 2-3 more 4 ms `sp_tick` slices. The scope section used 98 % of the CPU, and 56 % of that was outside the nested sections.
- The scope page at 9 fps (auto sweep), or on a square (columns mostly unchanged), had idle passes in between.

Fix (scope_ui.rs, app.rs):
- The flush slice is now 1.5 ms (was 4 ms).
- A frame's paint starts on the next pass without a frame. Frames in back-to-back passes still paint at once.
- `DBG:PROF?` "s.paint" (formerly the spare x1) times that paint.

The emulator does not reach that frame rate at 1 ms/div, so this is not measured yet.

**3. RUN key and RUN badge while the MF popup is open.** This matches stock and was left as is. docs/stock-scope-ui.md 7.1 / 10.3 / 10.4: the popups are full-screen modal roots, and RUN / AUTO are swallowed while one is open. Scripts must close the popup with MF first.

**5. Save CSV on the USB volume.** Checked in the emulator on e832a86 and again on 7939217 (SPI2 bus):
- Scope page, STOP, MF → Settings → Save CSV: the next LUN 0 read gets UNIT ATTENTION 06/28, then the listing has `SCOPE/SCOP000.CSV`. screenshot.rs `finish()` calls `dlog::set_stale()` since 4a484f7.
- `LOG:CSV ON` / `OFF` (the same csvlog path as swipe-REC) gives `LOGS/LOG000.CSV` the same way.
- No change needed.

**Re-check list (next device session, this build)**
1. T7 AC V, 6 V range, 0 / 1 / 5 V DC: ON == OFF within 0.1 mV. Then `DBG:SENT:VAR 1` should bring back the ~37 mV floor (this confirms the cause), then `VAR 0`. If the floor stays with VAR 0, use `VAR 2` and report: the AC V sentinel then goes off by default.
2. Soak: DC V 1 ↔ 10 V autorange, A jack open, 5 min: no AJACK, `DBG:SENT?` kind OFF at 10 V. Also `VAR 4` at 10 V manual 60 V (expect the trip again), and 10 V on manual 6 V (OL, r0 sentinel): note whether it trips.
3. The AC mV and TC soaks that were not run. T9 with a source in A (1 mA in DC V r0 and in AC V).
4. AD2: `log --rate 1200 / 2400 / 4800`, chop on and off: measured ≈ nominal, lost 0 (DRAW 0/1). 9600: the loss on the SPI2 bus.
5. Combined view, open input, 1 ms/div, noise-triggered: 400 × DBG:PING? p50 ≤ 2 ms, DBG:PROF? s.paint max ≤ 2 ms, fps.
6. Save CSV and swipe-REC on the USB volume (the emulator result above), DBG:BOOT? stack.

## 5f14d38 (+ad2-isr)

**Build.** The owner installed **5f14d38** built with `--features ad2-isr` before the run (2026-10-22 03:39-04:40). At the start it was healthy: `DBG:ISR?` field 1 = 1, stack 26 408 / 48 088 B, crash count 0, DMM page DC V AUTO+ 0.9998 V. No firmware was installed and no code was changed. Two reboots were made on purpose (`DBG:RESET`, items 3 and 5).
Screenshots, CSVs and profiles are in the session scratchpad (`dev-5f14d38/`), not in git.

**Wiring** as in the 9dd1222 run: the calibrator (`/dev/ttyACM1`) drives the scope probe and the DMM V/COM jacks in parallel; the A jack is not wired.
- The 40 Hz square is func 2 mode 0 value 40000: 0..9.6 V, 4.806 V in AC V, 4.77-4.81 V in DC V (the calibrator's level drifts by ~1 % over the session, as seen on 9dd1222).
- **Func 2 mode 1 is not a small square.** It gives **9.585 V DC** (DC V) with ~2.5 mV AC on it; the 9dd1222 note "mode 1 = about 10 mVpp" is wrong. The calibrator has no small AC signal and no negative output (a -1 V request gives 0 V).

### Summary

| # | Check | Result |
|---|---|---|
| 1a | AD2 ISR, 4800 S/s, DRAW 2, 60 s | **FAIL.** lost 4113 of 288 202 (**1.43 %**), max gap 12.6 ms, bad 0, overwritten 0. DBG:ISR? overruns 0, timeouts 0, forced 0, max_isr 22.3 us, **max_lat 21.8 us** (> 5 us) |
| 1b | 9600 S/s, 30 s | lost 2.64 %, overruns 1845, max gap 9.1 ms |
| 1c | Cause of the loss (1a/1b) | **Not the main loop.** Every ~3948 conversions at every rate, the ISR reads INTF for ~11.5 ms and sees no AD2F. The phase of the 4x tick walks onto the conversion edge, and the INTF read-to-clear appears to swallow AD2F there (details below). 1200 S/s: lost 1 |
| 1d | *IDN? while streaming (undrained, 4800 / 9600) | **PASS.** p50 0.34 / 0.37 ms, max 0.90 / 1.03 ms; PROF loop max 3.3 / 3.5 ms |
| 1e | DMM and SELF? 2000 after the streams | **PASS.** AC V 4.8070 (4.8063 before), DC V 1.00043, SELF? all error counts 0 |
| 1f | Function change mid-stream | **PASS.** STATe? 0, "Settings conflict", forced 1, DC V reading at once, no reset |
| 1g | AC mV small square | not possible (no small AC from the calibrator). Mode 1 (9.6 V DC, overload): 1.35 % / 2.37 % lost at 4800 / 9600, the same mechanism |
| 2a | AC V floor with Jack alerts ON, VAR 0 / 1 / 0 | **PASS.** VAR 0: ON - OFF within 0.04 mV (0.13-0.44 mV); VAR 1 brings back **37.15-37.17 mV**: the OP1 chopper was the cause |
| 2b | DC V 1 ↔ 10 V autorange soak, 5 min, A open | **PASS.** hits +0, streak 0, NONE; kind OFF at 10 V. VAR 4 on manual 60 V trips again (AJACK); 10 V on manual 6 V (OL, r0 sentinel) does not trip |
| 2c | Combined view, open input, 1 ms/div | **Marginal.** 56.6 fps, s.paint max 1.68 ms (PASS ≤ 2), loop max 12.3 ms, **ping p50 3.3 ms** (target ≤ 2; 11.8 on 9dd1222), p99 18.1 ms |
| 2d | AD2 rate ladder | **PASS.** captured + lost = nominal at 1200 / 2400 / 4800 / 9600, chop on and off (r26 0x0D / 0x05 / 0x24 / 0x03) |
| 2e | Scope Save CSV on the USB volume (9dd1222 FAIL) | **PASS.** SCOPE/SCOP000-002.CSV are on the mounted volume |
| 3a | Record page via Hold & Limits | **PASS.** 9th row "Record", page with steppers, "Free: about 1193 h (limit)" |
| 3b | 1 s interval, Start / Stop on the page | **PASS.** LOGS/LOG000.CSV on the mounted volume right after the stop, byte-identical to MMEM:DATA?, **132 rows, t = 1, 2, … 132** for a 131.05 s recording |
| 3c | LOG:CSV:INT / DUR / FREE?, settings conflict | **PASS.** INT 10 and DUR 600 accepted, INT 5 / DUR 7 "Illegal parameter value", INT while recording "Settings conflict", FREE? 14 880 768 |
| 3d | 10 s / 10 min stops itself | **PASS.** LOG001.CSV: 60 rows, t = 10 … 600, state 3 after ~605 s |
| 3e | Settings over a reboot | **PASS.** INT 10 / DUR 600 kept after DBG:RESET |
| 3f | Auto power-off held while recording | **not observable**: no DBG field exposes it, and Shutdown is "Never" on this unit (see below) |
| 4 | LIVE? vs HOLD | **PASS in continuity**: LIVE? / HOLD alternate ~1 s, the slot is clean. **In Ω, +1 V gives no LIVE?** (50.2 MΩ, NONE): by design, Ω LIVE? needs a negative result |
| 5a | W button: wiring dialog once, then direct | **PASS** |
| 5b | Tap W cycles the coupling, glyphs | **PASS.** ACDC → AUTO,DC → DC → ACDC; glyph on the button and above the unit; "AUTO" small text above the glyph |
| 5c | POW:ZERO (calibrator disabled) | **PASS.** OK,SET,Y 1072, X 891/160/892/162/875/152, AD1 547/2806/256/-2934/2599/-2763; V phantom 51.8 mV → Vdc -0.1 mV |
| 5d | V vs DC V after the zero | **PASS.** 5 V on 6 V: -0.021 % (DC) / -0.023 % (ACDC); 5 V on 60 V: -0.013 / -0.028 %; 10 V on 60 V: -0.052 / -0.063 % |
| 5e | f on the square, AC+DC | **PASS.** 40.0018 Hz, R21 0xBF; V 6.7369 V vs DMM AC+DC 6.7387 V (-0.03 %) |
| 5f | Energy: tap P → Wh → VAh, long-press reset | **PASS** (the box shows P while the main readout shows energy). The first "Yes" did not reset, the second did (see below) |
| 5g | Reboot returns to W | **PASS.** CONF? POW, coupling AUTO and the zeros kept, energy and time from 0 |
| 6 | DBG:BOOT? stack | **PASS.** High-water **29 996 / 48 088 B (62 %)** after items 1-3 (first reboot); 26 408 after items 3-5 and at the end; crash count 0 |

### 1. AD2 stream ISR (feasibility doc 9.5)

AC V, 6 V range, the 40 Hz square (READ? 4.8063 V). Host `dm40scpi.py log --source ad2`, then `DBG:ISR?` (fields: feature, active, ticks, captured, empty, gated, dropped, bad, timeouts, overruns, max_isr_ns, max_lat_ns, tick_mHz, forced).

| run | captured | lost | gaps 2 / 3-4 / 5-16 / 17-128 / >128, max | DBG:ISR? |
|---|---|---|---|---|
| 4800, DRAW 2, 60 s (`a48.csv`) | 284 089 | 4113 (1.43 %) | 11/16/64/97/0, 12.55 ms | 1,0,1152883,284089,16527,852267,0,0,0,0,22315,21760,19201228,0 |
| 4800, DRAW 1, 30 s | 142 020 | 2035 (1.41 %) | 27/7/63/50/0, 12.34 ms | lat 21 620 |
| 4800, DRAW 0, 30 s | 141 997 | 2016 (1.40 %) | 10/14/99/49/0, 7.76 ms | lat 4620 |
| 4800, chop off / 1 V DC in / calibrator off / alerts OFF, 15 s each | | 1.38 / 1.37 / 1.38 / 1.39 % | max 11.9-12.3 ms | |
| 9600, DRAW 1, 30 s (`a96.csv`) | 280 472 | 7617 (2.64 %) | 33/35/296/191/0, 9.06 ms | 1,0,1152432,280472,30545,841415,0,0,0,1845,22315,21760,38402457,0 |
| 2400, 15 s | 35 780 | 245 (0.68 %) | 0/4/16/5/0, 12.6 ms | lat 13 260 |
| 1200, 15 s | 18 013 | 1 (0.01 %) | 0/0/0/0/0, 0.84 ms | lat 5660 |

**FAIL: lost ≠ 0 at 4800 S/s.** The loss is the same with DRAW 0, 1 and 2, with chop on or off, with any input, and with alerts off.
- The losses come in bursts, **one every ~3948 conversions at every rate**: every 0.822 s at 4800, every 1.645 s at 2400, and every 0.411 s at 9600.
- Each burst lasts **~11.5 ms**: 57 conversions at 4800, 27 at 2400, 109 at 9600.
- The ISR itself ran on time:
  - ticks = the tick rate x the span exactly, so no tick was missed;
  - gated = 3 x captured exactly;
  - **empty ≈ 4 x lost**: through a burst, the ISR read INTF on every tick and saw no AD2F.
- Interrupt masking cannot explain a 12 ms hole with every tick serviced. The bus did not time out (timeouts 0, SELF? clean).
- **Likely mechanism (inference):** the INTF read clears AD2F. If AD2F is set during the INTF frame, after the bit was sampled, the conversion is lost.
  - The tick is exactly 4x the rate, so every 4th read lands at the same phase of the conversion, and it keeps swallowing AD2F until the slow timer / chip drift carries it past.
  - The tick runs **+62.5 .. +64 ppm fast** against the chip at 2400 / 4800 / 9600 (DBG:ISR? tick_mHz 9600614 / 19201228 / 38402457). That drift brings the read phase to the edge once every 1 / (4 x 63 ppm) ≈ 3950 conversions, which is exactly the burst period.
  - It then stays there for w / 63 ppm. With w ≈ 0.73 us (about one INTF frame), that is ~11.5 ms whatever the rate, again as measured.
  - At 1200 S/s the tick is **-32 ppm** (4799846 mHz), so the phase drifts the other way, and the run lost 1 conversion.
  - The emulator cannot show this: its INTF read has no race.
- **Fix candidates (not tried):**
  - after an `empty` read, shift the next read by a fraction of a tick, so the phase cannot stay on the edge;
  - or round ARR so that the tick is always slower than 4x the chip;
  - or detect a swallowed conversion from the data registers instead of INTF.
- **max_lat_ns 18.8-21.8 us whenever DRAW is 1 or 2**, including the undrained 4800 run in which no DMM frame was painted (DBG:PROF? frames 0, dmm max 22 us); 4.6 us at DRAW 0. The doc expects < ~5 us. The source is not isolated. It is not what causes the loss, because the loss is the same with DRAW 0.
- **9600 S/s:** a 26 us tick with a 22.3 us ISR gives overruns (1845 in 30 s, 13 in 10 s undrained). The 2.64 % loss matches the race model alone (11.4 ms every 0.411 s = 2.8 %).

**While streaming, undrained, 10 s** (`prof-stream-4800.txt`, `-9600.txt`): 40 x `*IDN?` gave p50 0.34 / 0.37 ms and max 0.90 / 1.03 ms. DBG:PROF? showed a loop max of 3.26 / 3.55 ms, no pass over 5 ms, and the usb lap max at 43 us. LOG:COUNt? showed `2048,52173,50125,772,772` at 4800 (overwritten > 0 is expected, because nothing drained). The ISR loss is still there.

**After the streams** (`after-stream.txt`):
- DRAW 1; AC V 4.80696 / 4.80695 / 4.80694 V;
- DC V on 1 V: 1.000435 / 1.000426 / 1.000399 V;
- `DBG:HYSPI:SELF? 2000`: `16,2000,0,0,0,0,7.84,21.55,68.65,57.48,23,…`, all error counts 0.

**Function change mid-stream** (`forced.txt`): AC V, `LOG:SOUR AD2`, `LOG:SRAT 4800`, `LOG:STAT ON`, 3 s, then `CONF:VOLT:DC`. The results:
- `LOG:STATe?` 0;
- `SYST:ERR?` -100,"Settings conflict";
- DBG:ISR? forced = 1 (`1,0,58621,14436,878,43307,12388,0,0,0,22315,21560,19201228,1`);
- READ? 4.81089 V (DC V of the square);
- uptime continuous, no reset.

**AC mV.** Calibrator mode 1 is 9.585 V DC, a 16x overload of the 600 mV range. The DC-coupled AD2 stream read code ~249 950 (1.192 V at the stock scale). It lost 1.35 % at 4800 and 2.37 % at 9600 (overruns 1162), the same bursts. A small AC mV signal needs another source.

### 2. Fixes after 9dd1222: the re-check list

**1. AC V floor** (`t7var.txt`: AC V 6 V, 8 READ? per state, alerts 7 / OFF / 7)

| VAR | 0 V DC | 1 V DC | 5 V DC |
|---|---|---|---|
| 0 | 0.418 / 0.411 / 0.405 mV | 0.160 / 0.124 / 0.128 mV | 0.156 / 0.141 / 0.135 mV |
| 1 (9dd1222 row: OP1 chopper kept) | **37.149** / 0.412 / **37.135** mV | **37.172** / 0.164 / **37.148** mV | **37.172** / 0.167 / **37.172** mV |
| 0 again | 0.443 / 0.417 / 0.423 mV | 0.139 / 0.158 / 0.154 mV | 0.163 / 0.150 / 0.153 mV |

The floor is gone, and VAR 1 brings it back. This confirms the inferred cause, OP1's chopper running with AD1. VAR 2 was not needed. VAR was left at 0.

**2. A-JACK in DC V** (`soak-dcv.txt`, `dcv10.txt`)
- **Autorange soak:** 1 ↔ 10 V every 10 s for 302 s gave hits 184 → 184 (+0), max streak 0, alerts NONE. At 10 V the meter sits on the 60 V range with `DBG:SENT?` kind **OFF**.
- **Manual 60 V, 10 V in**, 15 s each:

  | VAR | hits | max streak | alert |
  |---|---|---|---|
  | 0 | +0 | 0 | NONE |
  | 4 (the 9dd1222 comparator row) | +150 | 178 | **AJACK** |

  The mechanism is confirmed.
- **Manual 6 V, 10 V in** (OL, r0 sentinel CMP_PB1, VAR 0): +0, NONE. Nothing above 6 V trips on r0.
- `CONF:VOLT:DC 6` with 10 V on the input selected the 60 V range, not 6 V, so the 6 V range was set on the range page.

**3.** Not run: the AC mV and TC soaks, and T9 with a source in A (the A jack is not wired).

**4. AD2 rate ladder.** Every session's `captured + lost` over its span equals the nominal rate (1200.7, 2402, 4800.0, 9600.0 /s). Chop off uses r26 0x0D / 0x05 / 0x03, chop on 0x24 at 4800. The ladder is right. The only loss is the ISR race in 1.

**5. Combined view** (DMM page, `DBG:SWIPE L`; calibrator off; `TIM:SCAL 0.001`, `CHAN1:SCAL 0.02`, `TRIG:SWE AUTO`, `TRIG:LEV 0`; noise-triggered T'D; `06-combined-1ms-open.png`, `prof-combined-open-1ms.txt`)
- **Profile:** 56.6 fps; loop max 12.27 ms (628 passes 5-10 ms, 33 passes 10-20 ms per 10 s).
- **Sections:**
  - **s.paint max 1.68 ms** (avg 1.27 ms, 63 % CPU);
  - s.frame 2.27 ms;
  - s.meas 6.99 ms;
  - s.mtxt 4.06 ms.
- **400 x DBG:PING?:** **p50 3.3 ms**, p90 7.0, p99 18.1, max 18.1 ms, 4 over 15 ms.

  That is much better than 9dd1222 (p50 11.8 ms), but above the ≤ 2 ms target: almost every pass paints a 1.3 ms slice at ~57 fps. The p99 / max come from the s.meas passes (7 ms) plus a slice.

**6.**
- **Scope CSVs:** SCOPE/SCOP000-002.CSV (692 550-692 553 B) are on the mounted USB volume (`/run/media/$USER/DM40`), which fixes the 9dd1222 FAIL.
- **Recordings:** see 3.
- **Stack:** see 6.

### 3. Record page (decision #1)

Navigation: MF → Hold/Limits (83,260) → "Record" (the 9th row, y 297), with DBG:TAP.
- **Hold & Limits** (`10-hold-limits.png`) shows the 9 rows with no overlap; the last is "Record  Every reading".
- **The Record page** (`11-`, `12-record-1s.png`) has Interval / Duration steppers, "Free  about 1193 h (limit)" and "Start recording".
  - Free is 14.88 MB at 2 B/row and 1 s, i.e. 2067 h. It is capped at the 49.7-day limit, which is consistent.
  - A tap on Interval "+" gave "1 s" (LOG:CSV:INT? 1).

**1 s interval for ~2 min**
- **Start** on the page at 04:06:08.46. It returned to the DMM page with the red REC badge (`13-rec-started.png`).
  - `LOG:CSV?` read `1,0,1,402,369,0`.
  - `LOG:CSV:INT 10` during the recording gave -100,"Settings conflict".
- **Stop** on the page at 04:08:19.51 (131.05 s). It showed the toast "Saved LOG000.DLG" (`16-rec-stopped.png`); `LOG:CSV?` read `3,0,132,750,750,0`. The page stays open after Stop.
- **The CSV:** **LOGS/LOG000.CSV appeared on the mounted volume right after the stop** (3009 B, no remount). It is byte-identical to `MMEM:DATA?` (`dm40scpi.py get`).
  - The header carries the third "# interval 1 s: …" line.
  - **132 rows, time_s 1.000, 2.000 … 132.000** (every step exactly 1 s; the last is the partial interval at its nominal end).
  - Values 0.999218-0.999975 V.
- The LOG000.DLG is 750 B for 132 rows.

**SCPI**
- `LOG:CSV:INT 5` and `LOG:CSV:DUR 7` gave -100,"Illegal parameter value"; the old value was kept.
- `LOG:CSV:INT 10` and `LOG:CSV:DUR 600` were accepted.
- `LOG:CSV:FREE?` 14 880 768.
- The page then showed "10 s" / "10 min" (`17-record-10s-10min.png`).

**Reboot** (DBG:RESET): `LOG:CSV:INT?` 10 and `LOG:CSV:DUR?` 600 survived. The meter came back on DMM DC V AUTO+.

**Duration expiry:** `LOG:CSV ON` at 04:09:47. By 04:19:52 it had ended itself: `LOG:CSV?` `3,1,60,915,915,0`. LOG001.CSV has 60 rows, t = 10 … 600 s, and LUN = MMEM. INT / DUR were then set back to 0 / 0.

**Auto power-off hold:** no DBG field exposes it.
- In the code (app.rs tick / `off_warn`), the idle `power_off()` and the countdown are gated on `!csvlog::active()`, which is `LOG:CSV?` state 1 during both recordings.
- Shutdown is "Never" on this unit, so a missing power-off would prove nothing anyway. **Not verified on the device.**

Not checked:
- swipe REC with the chosen interval;
- the gallery tile of a .DLG;
- Windows;
- the estimate "within 2x" (the page shows the cap).

### 4. Alerts vs HOLD (decision #5)

- **Ω (`CONF:RES`) with +1 V on V/COM:** 50.2 MΩ, `SYST:ALER?` NONE, `DBG:SENT?` `CMP_PB1,NONE,…`.
  - Ω's LIVE? needs a negative result (ohm AD1 code < -20 000, doc §4.6), and the calibrator cannot output a negative voltage (-1 V gives 0 V: 434.6 kΩ, NONE).
  - With HOLD (DBG:KEY 1) the slot shows only HOLD (`20-hold-live-*.png`).
  - **A positive source in Ω is not flagged.** That is a design limit, not a regression.
- **Continuity with +1 V:** 0.00 Ω, LIVE.
  - **HOLD on:** six screenshots ~1.1 s apart alternate exactly, LIVE? (amber) / HOLD / LIVE? / … (status-slot md5 A B A B A B; `22-cont-hold-1.png`, `22-cont-hold-2.png`).
  - The box is cleared fully in each phase, and the readout stays frozen at 0.00 Ω.
- A-JACK under HOLD was not run (A jack not wired).

### 5. Power meter (doc §3.12 re-check)

1. **W button** (444,297) from DC V:
   - The first tap opened "Power meter: wiring" (`30-W-tap1.png`). OK entered the page with V 1.0563 V and "I ranging..." (`31-`); P followed within ~10 s (`32-`).
   - Back on V, a second tap on W entered directly with no dialog (`33-`).
2. **Coupling:** a tap on the active W button cycles ACDC → AUTO,DC → DC → ACDC (`POW:COUP?`; `34-W-cycle-sheet.png`).
   - The glyph above the unit and on the button: DC = the DC glyph; AUTO = the DC glyph with "AUTO" in small text above; AC+DC = the AC+DC glyph.
   - In DC the PF / S / Q / f boxes show "--".
3. **POW:ZERO** with the calibrator output disabled: `OK,SET,1072,891,160,892,162,875,152,547,2806,256,-2934,2599,-2763`. Y and X are as the doc expects (~1080, ~880 / ~155).
   - Before the zero: V 51.8 mV, Vdc 52.2 mV, I 9.2 uA.
   - After it: V 2.5 mV (rms noise), Vdc -0.095 mV, I 0.48 uA.
4. **V vs DC V** (`powv.txt`; 6 x FETC:POW:ALL? field 5 vs the DC V function's mean before / after)

   | input | range | DC | AC+DC |
   |---|---|---|---|
   | 5 V (DC V 4.99811) | 6 V | 4.99705 (-0.021 %) | 4.99697 (-0.023 %) |
   | 5 V | 60 V | 4.99748 (-0.013 %) | 4.99673 (-0.028 %) |
   | 10 V (DC V 9.99721) | 60 V | 9.99199 (-0.052 %) | 9.99093 (-0.063 %) |

   Everything is within 0.07 % before any V cal, against the ~0.2 % target. POW:VOLT:CAL was not run.
5. **The square, AC+DC, 60 V:**
   - f 40.0018 Hz (field 7), `DBG:POW:ROW?` R21 = **0xBF**.
   - V 6.7369 V vs the DMM AC+DC function 6.7387 V (-0.03 %); Vdc 4.7620 vs DC V 4.7668 V (-0.10 %).
   - `POW:VOLT:RANG AUTO` stays on 60 V. `POW:COUP AUTO` gives AUTO,ACDC (the ripple).
6. **Energy** (`37-W-valtap-sheet.png`):
   - **Value tap:** the main readout cycles mW → mWh → VAh → mW, and the E box shows P while the main readout shows energy. READ? stays P.
   - **Long press on the E box** (`DBG:DRAG 58,160,58,160,20,1500`) opens "Reset energy?" No / Yes (`38-`).
     - The first "Yes" tap left the time running (00:00:55 → 00:01:10; `39-`).
     - A second long press + Yes reset it (time 00:00:46 → 00:00:01, MEAS:POW:ENER? -1.66e-9 → -1.27e-10; `41-42-sheet.png`).

     Either the first tap was lost (it came ~0.7 s after a DBG:LCD? read) or the reset did not take. This is not settled; one retry worked.
   - **Cosmetic:** with ~0 current the readouts show "-0.0000 mWh" / "-0.000 mW" (a negative zero).
7. **Reboot** (DBG:RESET) in W: the meter came back in W (`40-`) with CONF? "POW AUTO,AUTO", POW:COUP? AUTO,ACDC, the zeros kept (`POW:ZERO?` `NONE,SET,…`) and E time 00:00:10 (from 0).

Not run: T2-T4 with current through A, the XY / XX switch cost with current, POW:VOLT:CAL, and the MF > Power Settings page.

### 6. Stack

DBG:BOOT? STACK (high-water since boot / size):
- start of the run: 26 408 / 48 088;
- **29 996 / 48 088 (62 %, 18 092 free) before the first reboot**, after items 1, 2 and 3a-3c;
- 26 408 before the second reboot, after items 3d-5;
- 26 408 at the end.

Crash count 0 all the way, no unexpected reset. The build-time worst path is 30 632 B.

### State at the end

- **Device**
  - DMM page, DC V AUTO+, 0.9997 V (`51-final.png`).
  - Jack alerts 7, DBG:SENT:VAR 0, DBG:AD2:DRAW 1, LOG:STATe 0.
  - LOG:CSV:INT 0 and DUR 0 (defaults).
  - Power meter coupling AUTO, the POW:ZERO zeros stored.
  - Volume, backlight, Dim and Shutdown were not touched.
  - Scope left at 1 ms/div, 20 mV/div, Auto, level 0 V (set by SCPI for the combined-view check; it was 50 ms/div, 2 V/div, level 4.88 V).
  - The first READ? after the calibrator's switch from the square to 1 V was OL (the calibrator's output settling; the chart shows ~0.75 V, then OL); it was 0.99969 V a few seconds later.
- **Calibrator:** 1 V DC, enabled (func 1 mode 0 value 1000).
- **Files on the SPI disk added by this run:** LOGS/LOG000.DLG, LOGS/LOG001.DLG.

## Bluetooth (0667175)

**Build.** The device ran **3bae462** (= 0667175 on rust-fw-poc: the BT module powered via PC0) for the whole run (2026-10-22 09:35-10:35). No firmware was installed and no code was changed; no reboot. Host: BlueZ, classic SPP XX:XX:XX:XX:XX:XX (name "C-1-ATK-DM40-CALI"), `/usr/bin/python3`. Wiring as before: the calibrator (`/dev/ttyACM1`) on the DMM V/COM jacks (1 V DC, a 2 V step for the freeze checks); the A jack is not wired. Screenshots, frame logs and scripts are in the session scratchpad (`bt0667/`), not in git.

### Summary

| # | Check | Result |
|---|---|---|
| 1a | Status-bar BT glyph | **PASS.** Off with no host (`01-nohost.png`), on while a host is linked (DMM `02-`, Gen `25-`), off again after the disconnect (`03b-`). The Scope page bar has no BT glyph (stock scope bar layout; `12-scope-conn.png`) |
| 1b | `--bt info`, `--bt read -n 20` | **PASS.** IDN / FUNC? / CONF? answer; 20 readings 0.99950-0.99976 V at ~2.5/s |
| 1c | `--bt btpoll -p 50 -d 20` (UF-36) | **PASS on all four pages: 400 / 400 replies (100 %)** on DMM, DMM+Scope, Scope, Gen; latency median 36.6-37.2 ms, max 43.9-62.9 ms. But see 1g: on Scope / Gen the value can be stale |
| 1d | Reconnect after a disconnect | **PASS.** 5 cycles: connected again 0.7-1.3 s after the previous close (the first try right after a close gets EBUSY from the host, the next one 0.5 s later connects) |
| 1e | Settings > Bluetooth OFF / ON (DBG:TAP 240,178) | **PASS.** OFF: connect times out, a 12 s `bluetoothctl scan` does not see 63:14:… (`31-bt-off.png`). ON: seen in the scan at once (RSSI -47), `--bt info` answers (`32-bt-on.png`). Left ON |
| 1f | DMM kept on while linked (UF-36 rail rule), link opened **before** leaving the DMM | **PASS.** cmd 09 followed a 1 → 2 V step made on the Scope page (999.5 → 1998.5 "mV" frames) |
| 1g | Link opened **while on Scope / Gen** (DMM rail off) | **FAIL.** The DMM does not resume: cmd 09, BT `FETC?` / `READ?` and even USB `READ?` on the Scope page return the last reading taken before the rail went off, at once, and do not follow a 1 → 2 V step (3 of 3 tries) |
| 1h | Several such links, then back to the DMM page | **FAIL (DMM frozen).** After 3 connect / disconnect cycles on the Scope page (and in the first session, several btpoll / cmd 09 links on Scope / Gen), the DMM page stays frozen: `FETC?` +1.998914E+00 at a 1 V input for > 9 min, `DBG:PROF?` d.read 0 in 5 s, an "AC?" hint in the status bar (`23-stuck2.png`). 2 of 2 tries with ≥ 3 cycles; 0 of 2 with one cycle. `DBG:HYSPI:SELF?` (it runs `app.dmm.start`) brings the DMM back at once |
| 2 | Compound SCPI over BT (f52b239) | **PASS.** `POW:PHAS:CAL RES;:SYST:ERR?;*OPC?` → `-100,"Settings conflict; RES: PF 1 optimum is flat, use REACtive (C or L load, PF ~0)";1` in one line; `*IDN?;SYST:ERR?` → `Alientek,DM40A,0,dm40-fw 0.1;0,"No error"`. The same over USB |
| 3a | cmd 09 in the power meter (doc §3.13 item 1, without current) | **PASS.** Mode byte **0x28**, format 0x08, main 0x0000 = "0.0000 mV" (the readout 0.0000 mW, P -1.5e-8 W; no sign bit, no negative zero), value 2 / 3 FF FF. In DC V AUTO+ the byte is 0x30 (format 0x18: 0.9993 V) |
| 3b | cmd 06 0x28 in the power meter | **PASS.** Reply `DF 05 03 06 01 00 12`; FUNC? "POW", CONF? "POW AUTO,AUTO" after it |
| 3c | BT SCPI in the power meter (§3.13 item 4) | **PASS.** `CONF:POW` → FUNC? "POW"; `FETC:POW:ALL?` 13 fields (P -1.5e-8, Vrms 0.99932, PF -1, t 34 s, …); `MEAS:POW?` / `READ?` answer; `CALC:LIM:UPP 100` → UPP? 1.0E+02, STAT? 1, FAIL? 0; `CALC:LIM:UPP -1` → FAIL? 1 and the red **HI** badge (`41-pow-hi.png`); restored with `CALC:LIM:CLE` + `STAT OFF` (UPP? / LOW? 9.9E37, STAT? 0 as before) |
| 4 | Decision #7 BT part: AD2 38 400 S/s, 900 s, DMA frames, BT client polling all the time | **PASS (stream).** 34 560 269 records, seq gaps 0, stamp gaps 0, `LOG:COUNt?` 0,34560269,0,0,0 (lost 0), `LOG:GAPs?` bad 0, max gap 27 us; `DBG:ISR?` late 0, dropped 0, bad 0, timeouts 0, overruns 0, max_isr 5.4 us, max_lat 0.28 us, CPU 17.5 %, dma 1. **BT reply rate during the stream: 0** (by design, TODO UF-36: cmd 09 frames are not served during an FR-38 session); btpoll overall 698 / 2496 (the replies before and after the stream); BT answered again right after (`--bt info`) |
| 5 | Fast logger over BT (`LOG:DATA?` on the BT link) | **Present, partial.** BT SCPI is served during the session: AC V, `LOG:SOUR AD2`, `LOG:SRAT 1200` (600 is taken as 1200), `LOG:STAT ON` over BT, 30 s of `LOG:DATA?` over BT: 27 777 records at **918 /s**, no timeouts, seq gaps 0, lost 0, but **6 593 overwritten** (ring full: BT drains ~918 records/s = ~11 KB/s, under the 1 200 S/s floor). In DC V `LOG:STAT ON` is refused ("Settings conflict", the DC row has no AD2 stream) |

### 1. The DMM freeze (1g / 1h)

Repro (the 1 V / 2 V calibrator on V/COM; no USB `READ?` in between, it would set host_dmm):
1. DMM page, no BT link. MENU → Oscilloscope (the DMM rail goes off).
2. Open a BT link (cmd 09 polls, `dm40scpi.py --bt btpoll` or any SCPI) and change the input: the frames and `FETC?` keep the old value.
3. Close it; repeat 2-3 two more times; MENU → Multimeter.
4. `FETC?` stays at the value of step 1 for minutes; the readout does not move, the bargraph is empty, the status bar shows "AC?".

Observations:
- A link held while leaving the DMM page keeps the DMM measuring (1f). Only a link that **starts** while the rail is off fails to wake it, though `update_rails` runs every pass with `d |= bt_on && bt_connected()`.
- A single connect / disconnect on the Scope page, then back to the DMM page: the DMM resumes normally (2 of 2). With ≥ 3 cycles it stays frozen (2 of 2: the first one recovered after `DBG:HYSPI:SELF?` too, ~5 min in; the second was still frozen after 9 min).
- `DBG:HYSPI:SELF?` (main.rs: `dmm_power`, the bus test, then `app.dmm.start`, `meter.reset`, `last_raw = None`) brings it back at once; the bus test itself reports 0 errors, so the HY3131 bus is fine and the DMM state machine is what is stuck. A suspect: `Dmm::start` from `set_suspended(false)` right after `Scope::rails` switches PE2 on (no settling time), repeated on every BT-driven rail toggle.

Cause and fix (emulator; the device re-check is below when done):
- `Scope::rails` pauses 10 ms only when a rail goes **down** (make-before-break, as the stock FUN_0800b384). A page switch Scope -> DMM drops PC4, so the row (`set_suspended(false)` -> `Dmm::start`) went out 10 ms after the PE2 rise. A wake that keeps PC4 on (a BT link or a host query on Scope / Gen) wrote the row microseconds after the rise, into a chip still powering up: the chip came up with its power-on row (AD1 off: no INTF flag, no reading; the sentinel's comparator pokes then show "AC?") and nothing rewrote it. In the emulator the rail then stayed on (`host_dmm` after a `READ?`, kept until a DMM view), so the DMM page found the rail already on and did not restart the row either: frozen until `DBG:HYSPI:SELF?`. **1h on the device is not explained by this alone:** the device repro avoided `READ?` and the rail toggled with the link, and the page switch has the 10 ms pause; the device has an ingredient the model lacks (PC14 still high after a disconnect, or a power-up longer than 10 ms after a short off). The fix covers both (50 ms settle, the read-back); the `SYST:RAIL:STATe?` counters of the re-check tell which. Emulator, no false restarts: `emu/hy3131/modetest.py` (every function, both firmwares, under the PE2 model) as before, the silent / lost counters 0 in every fw-rs scenario; with the jacks open, 16 s each in Hz, temperature, ohm, diode, continuity, power meter, capacitance, AC A: 0 restarts.
- The emulator's HY3131 model now power-gates the chip with PE2 (`RailModel`, emu/hy3131/README.md). `emu/hy3131/btwaketest.py` (link on the Scope page, 1 -> 2 V; 3 link cycles; DMM page, 2 -> 3 V; a chip POR, 3 -> 4 V): 3bae462 reads 1 V at every step (fails 1g, 1h and the POR), the fix 2 / 3 / 4 V.
- Fix (dmm.rs): after any DMM rail rise the row goes out `RAIL_SETTLE_MS` = 50 ms later from `Dmm::poll` (`wake_at`; the deadline moves at once, so a host `READ?` waits for the first fresh reading instead of answering the stale one within 850 ms). A stall watchdog: no reading for 2 s past the deadline -> R2A..R31 read back (bytes nothing rewrites while a row runs); all 0x00 / 0xFF where the row has others = the row is lost -> restart; no reading for max(10 s, 8 x the reading period) (not in capacitance, whose ramp may wait on a discharge) -> restart anyway. `SYST:RAIL:STATe?` = dmm,scope,wakes,lost,silent.

### State at the end

- **Device:** DMM page, DC V AUTO+, 0.9994 V (`50-after.png`), DMM live. Bluetooth ON. Backlight 10 %, volume 0 % (seen on the Settings page; Bluetooth toggled OFF and back ON, nothing else touched). Power meter coupling AUTO, its limits cleared as before (STAT 0). `DBG:AD2:DMA 1`, `DRAW 1`, `DIV 0,0` (defaults; drain.py sets DMA 0 on exit, set back), `LOG:SOUR AD1`, `LOG:SRAT 4800`, `LOG:STAT 0`. Crash count 0, no reset; stack high-water 23 252 / 46 588 B.
- **Calibrator:** 1 V DC, enabled (func 1 mode 0 value 1000).

## DMM rail wake and fast logger (036cc55)

**Build.** 036cc55 (DMM rail wake fix, stall watchdog, `SYST:RAIL:STATe?` counters, `DBG:AD2:DMA?`; it also contains the fast logger of 5d5dba0), installed by the owner, 2026-10-22 ~11:15-12:10; no reboot during the checks. USBTMC for SCPI and DBG:KEY / TAP, BT classic XX:XX:XX:XX:XX:XX (`/usr/bin/python3`, `dm40bt_scpi.Link`), the calibrator (`/dev/ttyACM1`) on V/COM. Scripts in the session scratchpad (`btwake/dev/chk.py`, `btwake/fast/`).

### Rail wake (the 1g / 1h repro)

`SYST:RAIL:STATe?` = dmm,scope,wakes,lost,silent.

| # | Check | Result |
|---|---|---|
| 1 | Baseline (DMM page) | `1,0,0,0,0`, FETC? 0.99962 V |
| 2 | **1g**: Scope page (`0,1,0,0,0`), BT link opened there, 1 → 2 V | **PASS.** Rail on at the link (`1,1,1,0,0`, wakes +1); BT FETC? 1.99886, BT READ? 1.99883 V |
| 2b | USB READ? on the Scope page (host wake, 2 → 1 V) | **PASS.** 0.99964 V, wakes +1 |
| 3 | Link cycles on the Scope page, no READ? (4 cycles, the input stepped 1 ↔ 2 V while linked) | **PASS.** Each link: rail on, wakes +1 (3, 4, 5, 6), BT FETC? follows the step (1.99898 / 0.99956 / 1.99864 / 0.99957); each close: `0,1` (PC14 drops: the rail follows the link) |
| 4 | **1h**: then MENU → Multimeter, 2 → 3 V | **PASS.** FETC? 2.99790 V, still live 10 s later, 1 V again 0.99976. lost 0, silent 0 (and 0 after 3 cycles that kept the rail on through a BT READ?: 2.99808 V) |
| 5 | Watchdog false restarts: calibrator off, 35 s each in Hz, temperature, ohm, power meter, diode, capacitance | **PASS.** lost 0, silent 0 throughout (FETC? 0 Hz, 212 °C open TC, 443 kΩ, ~0 W, OL, 65 µF open) |
| 6 | `DBG:AD2:DMA?` | **PASS.** `1` |
| 7 | AC V wake (40 Hz 0..10.4 V square, DMM AC V 4.7672 V): Scope page, USB READ? × 8 | **The first answer after the wake is +8 %** (5.145 / 5.187 V), then 4.90, 4.99, 4.92, 4.87, 4.84, 4.80, 4.785 V (1.1 s): the AC node starts over after the rail was off. Fixed in ea29bc4 (2 s settle after a wake in the AC functions); to re-check |

The device 1h froze on 3bae462 with a rail that toggles with the link (seen here: wakes +1 per link, `0,1` after each close), so the lingering-PC14 theory is out. What remains is a row written microseconds after the PE2 rise, which the 50 ms settle now prevents. The lost / silent counters stayed 0 in every check, so the watchdog never had to step in.

### Fast logger (disk sessions, `dm40scpi.py fast`)

| Session | Result |
|---|---|
| AD1, DC V 1 V, OSR 0, chop off, 60 s | 37 302 samples (622/s of the nominal 639.8), **1 122 lost** (`LOG:COUNt?` 0,37302,0,508,1088; `LOG:GAPs?` g2 1088, g3_4 17, max gap 4.7 ms): **FAIL vs "lost_before all 0"**. Prep 0 → 64 → 93 % → running; 99 575 B. `LOGS/FAST000.CSV` on the USB volume == the tool's CSV (cmp) |
| AD2, AC V on the 40 Hz square, 4800 S/s, 30 s | 144 002 samples, 0 lost, 0 overwritten; DBG:ISR? late 0, max_isr 3.66 us, max_lat 1.64 us, CPU 1.9 %. FAST001.CSV on the volume == the tool's CSV |
| AD2, 19 200 S/s, 30 s | 576 002 samples, 0 lost, 0 overwritten; max_lat 12.1 us, CPU 7.8 % |
| AD2, 38 400 S/s, 30 s | 1 152 002 samples, **1 lost** (late 1, one g2 gap), 0 overwritten; max_lat 11.9 us, CPU 17.5 %; 1.23 MB on the disk (3.47 MB allocated) |
| AD2 4800, LOG:DUR 60, stopped by the host (Ctrl-C → LOG:STAT OFF) at 23.8 s | **PASS.** LOG:DISK? 5 (done), 114 032 rows, 248 783 B of 872 448 allocated, LOG:STAT? 0, no error; the CSV decodes to the last row, 0 lost |

- `LOG:DISK?` went 1 (prep %) → 3 → 5 in every session; the AD2 sessions' wall time (61 / 131 / 221 s for 30 s of data) includes the prep (erase of the allocation) and the CSV transfer.
- max_lat while pages program: 1.6-12 us, not the ~0.24 us the plan expected (the SPI1 page writes do delay the stream ISR); no sample was lost to it up to 19 200 S/s.
- FAST002 / FAST003 were not compared over the USB volume (26 / 53 MB CSVs generated on the fly; too slow over MSC); their CSVs came through MMEM:DATA?.
- Not run: the Record page UI (3), LOG:DUR 0 to a full disk (4), the battery pull (5), BT (6), the prep time of 7 MB (7).

### State at the end

- **Device:** DMM page, DC V, 1.0008 V, `SYST:RAIL:STATe?` 1,0,10,0,0, `DBG:AD2:DMA?` 1, LOG:STAT 0, no error. Five new files LOGS/FAST000..004.DLG on the disk (≈ 6 MB). Bluetooth ON.
- **Calibrator:** 1 V DC, enabled.

## DMM rail wake re-check (ea29bc4)

ea29bc4 (on 2828108: AC settle 2 s after a wake, the Wave view's MENU keeps the DMM rail), installed by the owner, 2026-10-22 ~12:30. Same wiring; AC on the calibrator's 40 Hz 0..10.4 V square (DMM AC V 4.7670 V on the DMM page).

| # | Check | Result |
|---|---|---|
| 1 | AC V, Scope page, the READ? that wakes the rail (USB, then BT) | **PASS.** First answer after ~2.0 s: 4.7271 V (USB) / 4.7273 V (BT), -0.8 %; the next ones 4.775 V (+0.17 %). 036cc55 answered +8 % at 0.5 s. wakes +1 each |
| 2 | Wave view in AC V (MF → Wave: ISR active, FUNC VOLT:AC), MENU, READ? | **PASS.** ISR stops, rail stays `1,0` (no wake: counter unchanged), READ? 4.76747 V at once, no error |
| 3 | 1g / 1h in DC V | **PASS.** BT link on Scope, 1 → 2 V: BT FETC? 1.99934 V; 3 link cycles (wakes +1 each, `0,1` after each close); DMM page, 3 V: 2.99860 V. lost / silent 0 |

### State at the end

- **Device:** DMM page, DC V, 1.00028 V, `SYST:RAIL:STATe?` 1,0,8,0,0, no error. Bluetooth ON.
- **Calibrator:** 1 V DC, enabled.

## f3aea2f: open "Device" items without current through A

**Build.** f3aea2f (installed by the owner before the run), 2026-10-22 16:35-, no firmware installed by this run. Same wiring: the calibrator (`/dev/ttyACM1`) on V/COM only (DC, and the 40 Hz square func 2 mode 0 value 40000: 0..9.6 V, AC V 4.767 V); the A jack and the scope probe are not wired (the scope sees an open input). Host Linux, the volume auto-mounted read-only by udisks at `/run/media/$USER/DM40`. Screenshots (`dm40dbg.py shot` GRAM reads and the device's own SHOTS/Snnn.BMP), scripts and logs are in the session scratchpad (`dev-f3a/`), not in git.

### Summary

| # | Check | Result |
|---|---|---|
| 1a | W entry | **PASS** (direct, no dialog: `wired` is persisted, so the first-entry dialog is not re-testable on this unit; the same wiring picture opens from Power Settings > Wiring and has no overlaps). V after ~2 s, "I ranging..." until P (~10 s) |
| 1b | Coupling by a tap on the active W button | **PASS.** DC → AC → AC+DC → DC (`POW:COUP?` DC / AC / ACDC), the DC / AC / AC+DC glyphs on the button and above the unit; PF / S / Q / f "--" in DC. AC on the square: V 4.7672 V vs AC V 4.7667 (+0.01 %), Vdc / Idc 9.91E37 |
| 1c | Power Settings dialogs | **PASS.** At load toggles its help line (ON / OFF texts); Zero → dialog → Start: "Zeroing ... (12 s)" then "Zero stored", `POW:ZERO?` OK,SET,Y 1066, X 901/175/904/172/887/166; V cal → dialog → Cancel: nothing run; Wiring dialog; Limits → Hold & Limits: Power; Reset energy? |
| 1d | V cal (`POW:VOLT:CAL`) | **PASS on 6 V and 60 V (DC reference).** 5 V on 6 V: gain 1.020998 (doc 1.0209; factory 1.020659), Vdc vs DC V -0.022 % → +0.011 %, AC+DC -0.023 → +0.005 %. 10 V on 60 V: 1.016612 (doc 1.0170), -0.034 → +0.028 % (over-corrects by 0.06 %). 5 V on 60 V = SMALL (8 % < the 10 % window: the §3.12 list's "60 V with 5 V" is a doc error). AC reference: **blocked** (the calibrator's square has 4.77 V AC = 8 % of 60 V, SMALL; on 6 V it clips). All V gains set back to factory (`POW:VOLT:CAL DEF` per range; `POW:VOLT:GAIN?` 1.020659, 1.016145, 1.004528, 0.998148 as before) |
| 1e | f on the square | **PASS.** 40.0018 Hz, `DBG:POW:ROW?` R21 0xBF, V 6.7429 V (AC+DC, 60 V) vs √(AC V² + DC V²) 6.7408 V (+0.03 %) |
| 1f | Energy | **PASS.** Tap P → Wh → VAh → P; long press on the E box → "Reset energy?", **Yes 13 s later resets** (t 94 s → 1 s; the 5f14d38 failure is fixed); no "-0" readouts at ~0 W |
| 2a | Help 7/7 "Power meter" and pages 1-6 | **PASS.** Round-4 text (DC > AC > AC+DC, MF: Zero, hold energy box); every label / value ≥ 10 px apart, ≥ 4 px inside the row boxes (measured). Cosmetic: Help 5/7's long title leaves 9 px between "Back" and "Help" (30-50 px on the other pages) |
| 2b | `SYST:MOD?` | **PASS.** `DM40A,0,60000` |
| 2c | Record page | **PASS.** Hold & Limits 9th row "Record" → Interval / Duration steppers (Every reading / Until stopped → 1 s / 10 min), "Free: about 1193 h (limit)", Start recording |
| 3a | "Files live": new files without a re-plug (Linux, udisks ro mount) | **PASS.** A device screenshot (`DBG:SWIPE D,3`) appeared as SHOTS/S008.BMP 8.8-10 s after the gesture (the kernel re-read the volume on the UNIT ATTENTION); a 1 s CSV recording appeared as LOGS/LOG004.CSV within 2 s of its stop, byte-identical to `MMEM:DATA?` (152 rows, t = 1 … 152). A recording in progress is not listed (it appears at the stop: `finish` → `set_stale`) |
| 3b | "PC can edit" after a re-plug | **PASS.** Settings 3/3 row → "PC can edit (re-plug)" + toast "Re-plug USB to apply", `DBG:VDISK:RO?` 1,0. Re-plug = `usbreset 1209:0001` (bus reset: RO? 0,0) + `DBG:VDISK:EJECT 3` (the kernel only re-reads the WP bit at a medium change: "Write Protect is off"), udisks mount rw. `rm` S008.BMP: deleted on the SPI disk at once (WR? deleted 1); `gio trash` S009.BMP (Nautilus): deleted at the eject; `cp dm40fw.bin` (an f3aea2f image, 601 472 B in 29.8 s): "Firmware received / Install it now?" → **No**: no dm40*.bin left on the SPI disk (mdir of DEBUG/SPIFLASH.BIN), no _VHOST files, fsck clean (dirty bit / label notes only). WR? refused 0 throughout. Stack 23 376 / 45 912 after the cp. Restored "Files live" (RO? 1,1, ro mount) |
| 4 | Shot of a big screen, then the FFT | **PASS.** Scope FFT on, W page (AC+DC, f 40 Hz) shot as SHOTS/S009.BMP (correct image), back on the Scope page: `s.fft` calls 5012 → 5866 in 3 s, frames advance |
| 5 | `DBG:PROF?` while recording a CSV with the volume mounted | **PASS (no region C stall).** DMM page, 1 s interval, 152 s: loop max1s 1.1-2.5 ms (the DMM reading) except **one 74 ms pass per commit (60 s) and 84 ms at the start, all in the `csv` section**: the documented blocking FAT / directory `write_block` (sector erase, csvlog.rs header), not region C; `tick` 27.7 ms once at the start (bpb / fatdir). A host read of FAST002.CSV (26.5 MB) meanwhile: 584 KB/s, `usb` max 2.9 ms, `READ?` every 0.40 s (MED 2.5/s) without a gap, values 0.99925-0.99963 V |
| 6a | AD2 19200 to disk with a host volume read (TODO fast logger item 9) | **FAIL: max_lat 8.98 us** (target < 2 us), lat_pc `vdisk::file_read::{closure#0}` = region C (0x0808a8d2): the `near` closure of the two-cursor code got its own symbol, matched by the `.cold` rule. Fixed (below). 0 lost (`LOG:COUNt?` 2,192011,0,0,0) |
| 6b | Device hang at the end of a disk session | **FAIL (once, not reproduced).** `dm40scpi.py fast --source ad2 --rate 19200 --dur 30` with a host reading FAST003.CSV over the volume: the session completed (575 320 samples, FAST005.DLG intact), then the meter stopped answering and the **watchdog reset it** (`DBG:BOOT?` reason WDT_RETRY, FWDGT, crash count 1, healthy again after 60 s). Details below |
| 6c | Wave view + Save CSV | **FAIL (latency).** After a Save CSV in the Wave view (4800 S/s) `DBG:ISR?` max_lat 21.7 us, 15 overruns, lat_pc `vdisk::ours` (0x0808adb4, region C): the volume refresh after the save runs region C while the stream ISR runs. Also lat_pc `prof::report` (4.7 us) for a `DBG:PROF?` during the stream (debug only). Not fixed (below) |
| 6d | DMM page while a host copies (fast logger item 11) | **PASS** (5: `READ?` every 0.40 s during a 45 s copy) |
| 7 | DMM accuracy with the scope running (both rails) | **Partial FAIL.** DC 1 / 5 / 10 V: DMM page vs combined view vs Scope page within 0.04 % (5 / 10 V: ≤ 0.003 %). **The 40 Hz 0..9.6 V square reads +0.19 % higher whenever the scope rail is on** (DC V 4.7670 → 4.7760 V, AC V 4.7682 → 4.7745, AC+DC 6.7390 → 6.7501; alternated DMM / Scope / DMM / combined / DMM / Scope, reproducible to 0.01 %). Combined-view FREQ 40.0017 Hz (the 13.3 Hz bug is gone). AC V of a 0.5 V DC source: 0.3-0.5 mV on all three views (decays to the floor) |
| 8 | Wave view (feasibility 10.6) | **PASS with two bugs, fixed.** Entry from AC V: 2 V/div, T'D, Freq 40.0 Hz, Vrms 4.74 V vs AC V 4.7675 (-0.6 %), `DBG:ISR?` active 1. Trigger Normal ±1 V rising / falling at the T marker, Single → STOP. Range 2 → 20 V/div (60 V) → 2 → 200 mV/div (AC mV, DC-coupled), trace the same in volts. STOP + Save CSV: SCOPE/SCOP004.CSV "source DMM AC V (AD2)", volts, the last 2 s rms 4.780 V. MENU: ISR active 0, `READ?` 4.729 then 4.777 V. `LOG:STAT ON` → "Settings conflict", the view stays; `CONF:VOLT:DC` → "Wave stopped: DMM changed", DC V reads. **Bugs: (1) roll at 5 / 10 / 20 / 50 s/div moves at the 2 s/div speed (~20 px/s measured at 2, 20 and 50 s/div); (2) the entry toast from DC V is clipped ("V: no DC row on AD2, AC-coup")**. Current steps blocked (A jack not wired) |
| 9 | Alerts with the A jack open (seen along the way) | **FAIL.** A-PROT found latched at the end of the run; false A-JACK in Hz (reproducible). See "Alerts" |

### 1. Power meter

- Screens: `13-` .. `25-` (entry, coupling cycle `s-coup.png`, Power Settings `16-` .. `22-`, the square `s-Wsq.png`, reset `s-reset.png`). No overlaps found; the chart's axis labels end 3 px before the frame (also in mWh / VAh).
- V cal transcript (`vcal.txt`, `vcal2.txt`, `sq.txt`, `sq2.txt`): see 1d. The 6 V AC-coupled cal on the square (`POW:COUP AC`, range 6) **returned OK, gain 1.021410**, though the square's 9.6 V peak exceeds the 6 V range: the kind followed the DC part (DC reference, 100 % window), so nothing checks the peak. Flag for the owner (a clipping guard on the DC-kind path, or refuse when the AC part is large). The dialog says "10-70 % of range"; the DC kind accepts up to 100 % (cosmetic). The gains were reset to factory afterwards.
- `POW:ZERO` from the dialog changed the DC-row AD1 zeros of 600 mA / 6 A / 10 A a lot (256 / 2599 / -2763 → 522 / 1168 / 2560 codes) with the A jack open; X / Y moved by ≤ 15 codes. Not judged (needs current through A, T4).

### 3. USB volume

- Re-plug without touching the cable: `usbreset` (plugdev may write `/dev/bus/usb/...`) gives the bus reset the mode needs (vwrite `on_bus_reset`), but the kernel keeps the SCSI disk and its cached WP bit. `DBG:VDISK:EJECT 3` ("no medium" for 3 commands, then a UNIT ATTENTION) makes it revalidate the disk: `Write Protect is off` / `on`. `udisksctl unmount` before, `udisksctl mount` after.
- A host read in flight when the layout changes (a session end, a screenshot) fails with EIO (the UNIT ATTENTION: "medium may have changed"); a reader that reopens the file goes on. The known cost of "Files live".

### 6. Region C and the hang

**6a.** `DBG:ISR?` after the 10 s AD2 19200 disk session (repro below) with a host reading FAST003.CSV through the volume: `1,0,240176,192011,0,24059,0,0,0,0,3705,8980,19200017,0,1,-9,0,7,12029,16,79,0x0808a8d2,1` → max_lat 8 980 ns, lat_pc 0x0808a8d2 = `dm40_fw::vdisk::file_read::{closure#0}` (llvm-nm of the f3aea2f ELF, built reproducibly from the commit). The read path is `.text.vdisk_rd` (region B), but `file_read`'s `near` closure is a separate symbol whose name carries `5vdisk`, so link-native.x's `*7dm40_fw5vdisk*` rule put it in region C. **Fix:** `near` is a fn in `.text.vdisk_rd` (now at 0x08009434, region B).

Still in region C on that path (not fixed, for the next audit): the DMM-log and scope-record CSV generators (`Gen::advance`, `RowDec::get / next / rice`, `SDec::get / next`, `chunk_at`, `Gen::open`): a host reading LOG*.CSV or SCOP*.CSV while a stream runs executes region C per row (FAST*.CSV is fine).

**6b. The hang.** Timeline (`hang-kernel.log`):
1. `dm40scpi.py fast out --source ad2 --rate 19200 --dur 30 --conf acv` started; 8 s later a host reader started on FAST003.CSV (`volread.py`, 64 KiB reads, cache dropped).
2. The session ran to "done" (575 320 samples). At **17:00:51** the volume's UNIT ATTENTION failed the reader's READ(10)s (EIO) and the kernel re-read the partition table; the tool went on to `LOG:DEST HOST`, `LOG:COUN?` and `MMEM:DATA? "LOGS/FAST005.CSV"`.
3. The tool's USBTMC read timed out; at **17:01:24** usb-storage reset the device (its 30 s SCSI timeout: a command issued at ~17:00:54 was never completed), again at 17:01:30; at 17:01:44 the device no longer answered (descriptor read -32, address -71); at 17:01:46 it enumerated again after the **watchdog reset**.

Not reproduced: `MMEM:DATA?` of FAST005.CSV alone (26.5 MB in 50 s), with a volume read of FAST003.CSV meanwhile (75.6 s, the reader at 379 KB/s), and with both reading FAST005.CSV (76.9 s): all complete, the CSV identical. One repro of the session end alone (AD2 19200, `LOG:DUR 10`, the host reading FAST003.CSV, `LOG:DISK?` polled every 0.2 s, no `MMEM:DATA?`): done at 17:12:00.5, UNIT ATTENTION at 17:12:01, the reader got EIO for 2 s then went on, `DBG:PING?` answered 25 s later: no hang. So the hang needs the session end + the volume refresh + the host's revalidation reads + an `MMEM:DATA?` of the new FAST file starting at the same time; not repeated further (a second crash within 60 s of a crash reboot would zero the loader marker). The SPI disk was clean afterwards (fsck of DEBUG/SPIFLASH.BIN: 67 files, dirty bit only).

**6c.** The Wave view streams continuously, so any volume refresh while it runs (Save CSV here; a screenshot or a recording stop likewise) executes `vdisk::refresh` / `ours` / `scan_spi` in region C under the stream ISR: max_lat 21.7 us, 15 overruns (4800 S/s). The `.cold` audit assumed a refresh never overlaps a stream (true for disk sessions, not for the Wave view). Options: defer the refresh while the Wave view streams (it is only needed by a host), or move the refresh path to region B (~3 KB of the 13.6 KB left).

### 7. Both rails

`xtalk.txt`, `xt2.txt`, `xt3.txt` (`SYST:RAIL:STATe?` confirms the rails: `1,0` DMM page, `1,1` Scope / combined). The scope probe is not connected; the shift is the same with the probe input open. Only the square shows it (DC 5 / 10 V: none; 1 V: -0.02..-0.04 %, within the calibrator's drift of the run). The first `READ?`s after a `CONF:` change of function are unsettled on every view (AC V +13 % for ~1 s, AC+DC 9.9E37 then +10 %): a host should wait ~2 s, as the stock.

### 8. Wave view

- Timebase steps need a press (`DBG:DRAG x,y,x,y,10,80` on the corner zones); `DBG:TAP` gives a Tap gesture without the press tracker's arm, so the zones ignore it (a test-tool limit, not a bug).
- 5 ms/div shows "Freq: --" / "Cycl: --" with two periods on screen (10 ms .. 100 ms/div: 40.0 Hz). Vrms 4.74 V up to 20 ms/div, 4.69 V (-1.6 %) from 50 ms/div (1200-2400 S/s, under-sampled edges).
- Roll speed (`rollm.py`, the left edge of the trace every 2.8 s): 2 s/div 20 px/s (right), 20 s/div 20 px/s (should be 2), 50 s/div 20 px/s (should be 0.8). Cause: `w.per = screen / COLS`, and `screen` is clamped to the record (CAP - 2600 = 24 024 samples), reached at 2 s/div with 1200 S/s. **Fix** (scope.rs `wave_start`): samples per column from the timebase (25 000 x fs / TB.0 / COLS), not from `screen`. emu/hy3131/wavetest.py now checks the column count at 5 s/div.
- The DC V entry toast (and the coupling-pill note) were wider than the 400 px toast: "DC V: wave is AC-coupled", "AC-coupled (no DC row)"; the power meter's "Current above the limit: reduce it" was clipped as well → "Current too high: reduce it". tools/ui-render checks all four now (41 941 cases, 0 violations).
- `CONF?` in the Wave view on 200 mV/div (AC mV) still says `VOLT:AC +6`.
- `DBG:ISR?` after the session: dropped 255 488 of 277 801 captured (STOP / Single / the MF menu: the record is frozen and the ring is not drained; by design?).

### Alerts (A jack open all the time)

- **A-PROT latched.** The last DMM-page screenshot (`93-final.png`) had the red A-PROT badge; `SYST:ALER?` APROT, `DBG:SENT?` `CMP_PB1,APROT,7,0,744,72248,0,381,196510,15,105` (field 10: probe 15 mA now; field 11: 105 ms = the ">= 300 mA in the first probe window" rule). No earlier DMM-page shot after the reboot had it, so it tripped between ~17:15 and 17:45 in a probe function (DC V / mV / ohm / temperature). `CONF:VOLT:DC` over SCPI and a tap on V (to AC V / AC+DC) did not release it; DC A did (NONE). A sweep afterwards (DC V with 1 / 5 / 10 V and the square, on the DMM page, the combined view and the Scope page, after AC V / AC+DC / FREQ) did not trip it again (`aprot.txt`, `aprot2.txt`).
- **False A-JACK in Hz**, reproducible: FREQ with 1 V DC or the 40 Hz square on V-COM → `SYST:ALER?` AJACK, sentinel kind AD1PK_PB1, field 7 = 20 (DMM page and combined view; not at 10 V). DC V on the calibrator's 5 V DC showed the "AC" hint (not at 1 / 10 V).

### Fixes (0ec2093 on rust-fw-poc, not on the device yet)

Image built from the committed tree 0ec2093 (it also carries the burden-voltage and inrush work committed since f3aea2f): `dm40-fw-0ec2093.bin` in the session scratchpad, 609 792 B, sha256 6963016c…f428b; region B 13 596 B free (the build's < 16 KiB warning came with the inrush commit b1c6a27).


1. vdisk.rs: `file_read`'s cursor choice as a region B fn (6a).
2. scope.rs: Wave view roll speed from the timebase (8).
3. app_wave.rs, app_power.rs: three toasts that did not fit (8); ui-render checks them.

Host / emulator checks of the fixes: emu/hy3131/wavetest.py all PASS, roll 5 s/div 8.1 px/s (f3aea2f: 19.9 px/s, FAIL with the new check); tools/fatren-test PASS (vdisk / vwrite, 40 vwrite checks); tools/ui-render 41 941 cases, 0 violations (3 with the old toast texts); `near` links at 0x08009434 (region B) in a test build of f3aea2f + the patch.

### State at the end

- **Device:** DMM page, DC V; Settings unchanged (volume 0 %, backlight 10 %, Dim / Shutdown Never, Bluetooth ON, USB disk "Files live", ro mount); scope FFT off again; power meter coupling AC+DC, V gains factory, zeros from this run's `POW:ZERO`; LOG:CSV:INT / DUR back to 0 / 0; LOG:DEST HOST, LOG:DUR 0, LOG:SRAT 4800, LOG:SOUR AD1, LOG:STAT 0 (the repro had left DISK / 10 / 19200 / AD2). One watchdog reset during the run (6b); crash count 0 and HEALTHY afterwards.
- **Files added on the SPI disk:** LOGS/LOG004.DLG, LOGS/FAST005.DLG, LOGS/FAST006.DLG, SCOPE/SCOP004.DLG (scope record); S008 / S009.BMP deleted again (3b).
- **Calibrator:** 1 V DC, enabled.

## f3aea2f follow-up: the watchdog hang, A-PROT / Hz alerts, V cal guard, Wave Freq, both rails (7a00f62, 8124fb5)

Code reading, emulator reproductions and two read-only device measurements (the f3aea2f build, A jack open, calibrator on V-COM; logs `wdt/dev/` in the session scratchpad). Image: see "Device results" below.

### 6b. The watchdog hang: the USB disk, not the logger

**Cause, in two steps** (emulator; 7a00f62):

1. **A spurious zero-length packet on the disk's Bulk-IN.** usbd-storage 3.0.0 unstalls both bulk endpoints at the end of every command (`enter_state_command_transfer`), a read-modify-write of DIEPCTL in synopsys-usb-otg (`set_stalled`, a critical section that does not mask interrupts here). It runs right after the CSW went into the IN endpoint; when the host takes that packet between the read and the write, the core has cleared EPENA and the write sets it again with XFRSIZ 0: the endpoint sends a ZLP in front of the next command's data. An interrupt between the read and the write widens the window: the AD2 stream ISR during a disk session, when a host reading the volume issues hundreds of commands a second. The host's transfer then breaks (Linux: the command never completes; usb-storage's 30 s timeout at 17:01:24 in the kernel log).
2. **Linux's reset recovery then panicked the firmware.** usb-storage sends a Bulk-Only Mass Storage Reset (a class control OUT; the crate looked for it in `control_in`, so usb-device stalled it: review L5 of 2026-10-13) and resets the port. The transport kept the interrupted data in its IO buffer, the next CBW was parsed from those bytes and was invalid (Command Transfer Invalid: both endpoints stalled), and the crate still called our SCSI callback with the previous, finished command. Its `pass` / `fail` hit `set_status`'s state assertion: a panic (the magenta bar), no more feeding, FWDGT ~19 s later (WDT_RETRY, re-enumerated at 17:01:46). The emulator shows the second step directly: 31 bytes that are no CBW halt the meter (PC in `mmio::spin` called from `wait_for_watchdog`: the panic handler) and the watchdog resets it.

Also found on the way: a reply longer than the host's allocation length went out whole (babble), a command with no data phase that we answer with data (REQUEST SENSE of length 0) never got a CSW, the fill of a failed READ (UNIT ATTENTION on a 120 KB READ(10)) went out one packet per main-loop pass, and after a bus reset in a data phase the IN endpoint stayed armed and never sent again.

**Fix** (7a00f62): usbd-storage vendored (`fw-rs/vendor/usbd-storage`, `[patch.crates-io]`, each change marked "dm40 patch"): unstall only after our own stall (no read-modify-write after a CSW), no callback outside the data states, the IO buffer cleared at every reset, the Bulk-Only reset handled on control OUT, never more IN data than asked for, `Scsi::sending` / `in_address`. msc.rs: short replies via `write_data` + `pass(count)`, a data phase the host cut short = phase error, the Bulk-Only reset drops our transfer, the read pump also runs while the transport sends fill or the CSW, `usb::abort_in` (NAK, EPDIS, Tx FIFO flush) at a bus reset and a Bulk-Only reset. Not the logger, the vdisk cursors or the FAT commit: those ran to completion in every repro.

**Tests** (emulator; the DWC2 model now handles EPDIS):

| test | a60f8ce | fix |
|---|---|---|
| `emu/fastendtest.py --dur1 20 --dur 20` (two AD2 19200 disk sessions, a host thread reading FAST000.CSV over the volume all the time and doing Linux's revalidation at the UNIT ATTENTION, then MMEM:DATA? of the new FAST001.CSV while the reader switches to it) | 2 of 6 runs pass; 4 fail: a ZLP as a READ(10)'s first data, the reader's next CSW gets data (-EOVERFLOW), then the meter stops answering | 3 of 3 pass (51-53 MB read over the volume, 1 UNIT ATTENTION, MMEM:DATA? == the volume's bytes, no reboot) |
| `emu/mscresettest.py` invalid-cbw | watchdog reset (no answer to the Bulk-Only reset) | stall, reset recovery, healthy |
| read-bot / read-port / ua-port (a READ cut by a Bulk-Only reset / a port reset / a port reset with a UNIT ATTENTION pending) | Bulk-Only reset stalled; after a port reset no CSW ever | healthy |
| alloc-small (INQUIRY, 5 bytes asked) | 36 bytes sent (-EOVERFLOW) | 5 bytes |
| zero-alloc (REQUEST SENSE, no data phase) | no CSW | CSW |
| ZLP hunt (300 x REQUEST SENSE of length 0 + `*IDN?`, same connection) | (no CSW at all) | 0 short CSWs (5 of 300 with the other fixes but the unguarded unstall) |

### Alerts with the A jack open

- **A-PROT (first-window rule, 105 ms).** Not a stale reading of another function (the comparator rows the probe joins have the peak hold off, so it starts empty with the probe) and not the scale (4 194 codes per A, the probe's own path). The probe's row write restarts AD2; the peak hold catches its first results, and 1 258 codes (0.3 A) is 0.5 % of AD2's full scale. Fix (8124fb5): after every row write with the probe the hold is cleared 30 ms later (`PROBE_SETTLE_MS`) and the first window counts from there; a trip also needs the comparator run still going. What brought the probe in with nothing on A: comparator hit windows in DC V (`DBG:SENT?` counted 498 hits by the start of this follow-up); a likely source is the frequency row's ~54 mV on PB1 (below) still decaying after a switch to DC V, and the run's FREQ checks came before the latch was seen. Emulator: the HY3131 model's `Ad2StartGlitch` (the first 4 AD2 results after a start off by N codes, default 0); senttest `aprot-glitch` (1 V across A-COM, 3 000 codes = 0.72 A): a60f8ce A-PROT, fix A-JACK only; `aprot-dcv-24` 154 ms, `aprot-dcv-12` 454 ms.
- **False A-JACK in Hz.** Measured (`DBG:SENT?` in FREQ, A jack open): PB1 - PB3 stands at +41 100 codes (~54 mV; the threshold is 17 900 = 0.5 mA) with the calibrator at 0 / 0.1 / 0.5 / 1 / 2 / -1 V or disabled, ~3 100 at 5 V, -850 at 10 V; the 40 Hz square swings between 3 900 and 41 200 (peak-to-peak of a DC input: ~50 codes). The frequency row leaves its input divider open (R2C / R2D 0) and PB1 follows the input path: no fixed threshold or offset tells a current from it. Fix (8124fb5): no sentinel in the frequency function (a gap, as DC V 60 V+; doc 4.7); senttest `freq-gap`.

### Smaller items

- **V cal clipping guard** (8124fb5): `POW:VOLT:CAL` refuses with BIG when |DC| + sqrt(2) x AC RMS (>= the peak of a sine or a square) exceeds the range, whatever the kind; powtest `vcal-clip` (the 0..9.6 V square on 6 V).
- **Wave view 5 ms/div "Freq: --"** (8124fb5): 2 periods of 40 Hz in 480 samples, triggered on a rising edge at the centre: the rises at the frame's ends are not whole, one rise is inside, `compute_codes` needed two. It now takes the falls when there are fewer than two rises (frequency, period, duty over whole cycles from them). meter-test `meas` (fails on a60f8ce).
- **+0.19 % on the square with the scope rail on: analog, not firmware.** On the DMM page, same row, only `SYST:RAIL AUTO` <-> `BOTH` toggled (`xr.txt`): the 40 Hz 0..9.6 V square reads 4.76629 V (scope rail off) vs 4.77559 V (on), +9.3 mV = +0.195 %, alternated twice, 2e-6..3e-5 V sd; 400 Hz: +14.1 mV (+0.295 %); 20 Hz on the pages: ~+0.17 %; DC 5 V +0.01 mV, DC 0 V -0.03 mV. The DMM's row, timing and readout are the same in both states; the shift needs edges and grows with the edge rate, so it is the powered scope front end loading or coupling into the V input / COM. Nothing to fix in firmware; the default `SYST:RAIL AUTO` keeps the scope rail off on the DMM page, so only the combined view and the Scope page carry it (help / doc note). `SYST:RAIL` back to AUTO after the test.

### Budgets (fix image)

Stack headroom 21 148 B (>= 20 KiB), region B 11 460 B free (>= 8 KiB), SRAM 10 904 B free (>= 8 KiB). usbd-storage stays in region B (0x08027f20..), `usb::abort_in` too.

### Device results (1eb4d7e, 2026-10-22 19:49-20:10)

Installed by the owner (sha256 968ed018…97ae, crash count 0). Same wiring (calibrator on V-COM, A jack open, the volume mounted read-only). Scripts and logs: `wdt/dev/` in the session scratchpad.

| # | Check | Result |
|---|---|---|
| 6b | The hang repro: `dm40scpi.py fast out.csv --source ad2 --rate 19200 --dur 30 --conf acv` with a host reading FAST003.CSV (52 MB) over the volume in 64 KiB reads (reopening on EIO) | **PASS.** 576 002 samples, 0 lost (`LOG:COUNt?` 0,576014,0,0,0), the session's UNIT ATTENTION failed the reader twice (EIO, kernel "device offline or changed"), it reopened and went on (27.8 MB in 75 s); `MMEM:DATA?` of the new FAST007.CSV (26.5 MB) right after the session complete; no usb-storage reset in the kernel log, `DBG:BOOT?` reason NONE |
| 6b' | A port reset during a volume read (`usbreset 1209:0001` 6 s into a 30 s read of FAST002.CSV) | **PASS.** The reader saw 0 errors (17.5 MB at 578 KB/s), the meter kept running (no reboot, `READ?` answers) |
| Hz | FREQ with 1 V DC and the 40 Hz square, A jack open | **PASS.** `SYST:ALER?` NONE, `DBG:SENT?` kind OFF; FREQ 40.0017 Hz |
| V cal | Power meter, 6 V range, AC coupling, the 0..9.6 V square: `POW:VOLT:CAL` | **PASS.** `BIG,+6.000000E+00,+1.020659E+00,FACT`, the four gains unchanged (factory); coupling AC+DC and V range AUTO restored |
| Wave | Wave view from DC V, `TIM:SCAL 0.005`, NORM rising at 1 V | **PASS.** `SCOP:MEAS:FREQ?` 40.000 Hz at 5 ms/div (was "--"), Vrms 4.767 V; 10 ms/div 40.000 Hz |
| A-PROT | 6 rounds of FREQ -> DC V -> AC V -> DC V -> RES -> TEMP -> DC V, the square then 1 V on V-COM | **FAIL -> fixed after.** A-PROT in TEMP with the square (probe 1 747 mA, 135 ms). Traced: in ohm and temperature the probe follows the V input, ~0.34 A per volt (`ap3.txt`: 5 V DC on V-COM read 1.7 A in TEMP and RES, 10 V 3.1 A, the square up to 37.8 A; DC V with the same inputs: nothing). That, not the AD2 start-up alone, is the likely f3aea2f latch (the run had the calibrator on in ohm / temperature). Fix (next commit): the probe only in DC V / DC mV; ohm / temperature A-JACK only (doc 4.8); senttest aprot-ohm-12 / aprot-temp-12 now expect A-JACK, aprot-dcv-12 / aprot-ack-early act with the AUTO key |
| alerts | Also seen: with 5 / 10 V DC or the square on V-COM, ohm and temperature raise A-JACK (comparator hits on PB1, A jack open) | **Open** (a false A-JACK where LIVE? is the right alert; not changed) |

End state: DMM page, DC V, logging off (`LOG:DEST HOST`, `LOG:STAT 0`), `SYST:RAIL AUTO`, power meter coupling AC+DC / V range AUTO, V gains factory, calibrator 1 V DC; volume 0 %, backlight 10 %, Dim / Shutdown Never untouched. No crash during the checks.

### Device re-check (973df07, 2026-10-22 ~20:25)

Installed by the owner (sha256 2605779d…5a8d, crash count 0). TEMP and RES after DC V, with the square, 5 V, 10 V and 1 V DC on V-COM, 5 s each (`ap3-973.txt`): **no A-PROT**, the probe is not in (field 10 = 0 mA); A-JACK still comes with the square / 5 V / 10 V (the open item above), none at 1 V. Five rounds of FREQ -> DC V -> AC V -> DC V -> RES -> TEMP -> DC V with the square: alerts NONE except A-JACK in TEMP. End: DC V, `SYST:ALER?` NONE, 0.9994 V, logging off, calibrator 1 V DC, `DBG:BOOT?` reason NONE.

## a0985e4: region B audit #3, AUTO coupling, "Leads in A?", burden, E7, the calibrator's 50 Hz (2026-10-22 22:13-23:10)

The image was already installed by the owner. `DBG:BOOT?` matched the header of `dm40-fw-native-a0985e4.bin`: marker 0x0518FAE7 / 0x0009A0F4 / 0x00017E82, crash count 0, HEALTHY, reset reason SW at ~22:00.
- **USB:** at the start, usbtmc was not bound to interface 0, so there was no `/dev/usbtmc*`. A libusb client (sigrok) had detached it. pyusb `dev.attach_kernel_driver(0)` brought it back; `usbreset` alone does not.
- **Wiring:** the calibrator on V/COM, the A jack open, the volume mounted read-only at `/run/media/$USER/DM40`.
- **Input:** UI input came from DBG:TAP / DBG:KEY / DBG:SWIPE; screenshots are DBG:LCD? reads.
- **Files:** scripts and screenshots are in the session scratchpad `devchk-a0985e4/` (not in git).
- **Crashes:** none, two SW resets (the reboot checks).

### Summary

| # | Check | Result | Evidence |
|---|---|---|---|
| 1a | AD2 38400 S/s to disk (`LOG:DEST DISK`, 20 s, AC V), a host reading FAST003.CSV over the volume all the time | **PASS**: 768 003 samples, 0 lost, max_lat 0.28 us, lat_pc 0x08046ef6 (not C); the reader got 2 EIO at the session end (the known UNIT ATTENTION) and went on, 369 KB/s | `fastrun.py`, `f38400b.csv` |
| 1b | DBG:PROF? during that run | **FAIL (debug only)**: `prof::report` is in region C (0x08096000): max_lat 12.2 us, lat_pc 0x08096000, **1 sample lost** at the moment of the query (row t = 2.928 s, `lost_before` 1). Without the query: clean (1a) | `f38400.csv`, `f38400.csv.prof.txt` |
| 1c | Wave view 4800 S/s (20 ms/div) and 38400 S/s (2 ms/div): STOP + Save CSV, RUN, a screenshot (3-finger swipe) while running | **PASS**: overruns 0, max_lat 0.24-0.30 us at both rates; the stream pauses for the disk job (`active` 0 for ~2 s for a screenshot) and restarts by itself; `dropped` stays constant while running (the ring is drained) and grows only while STOPped (by design); SCOP007.CSV (38.4 kS/s, 2 ms/div) and SCOP008 / 009 appeared after leaving the view, S008-S010.BMP | `wave.py`, `stale.py`, `w*.png` |
| 1c' | One stale save | **open, not reproduced**: SCOP006.CSV (saved in a mis-sequenced run at 2 ms/div) has the header "2ms/div ... 4.80kS/s" and the same data as SCOP005 (4800). The same sequence afterwards (`stale.py 0.02 shot save1 tb2 0.002`) saved correct 38.4 kS/s data | SCOP005 / 006.CSV |
| 1d | A host copying SCOP / LOG / FAST CSVs (14 files, 11 MB) while the Wave view streams at 38400 | **FAIL -> fixed**: 0 overruns, but max_lat **21.4 us**, `late` 1, lat_pc 0x08087f50 = `dlog::FPar::parse` (region C), called by `Gen::open` of a FAST CSV. Copy at 96 KB/s, 0 errors. Fix below | `hostcopy.py`, `copy-wave.txt` |
| 1e | Leaving the Wave view (MENU) at 38400 | **minor**: `App::show` (C since audit #3) runs before the stream stops: max_lat 11.6 / 18.0 us, `late` 2 or 1 overrun, lat_pc 0x08099bc0 / 0x08099ae8. The samples belong to a view that is closing; not changed | `DBG:ISR?` after exit |
| 1f | Tap latency on pages whose handlers are in C | **PASS**: loop max1s after a tap (the handler + the page draw): range page 28-38 ms, Hold & Limits 37-51 ms, keypad keys ~1 ms, Record page 23-37 ms, Power Settings 31 ms, menu 37-51 ms, Settings 1/2/3 25-39 ms, generator wave cycle 26 ms, Wave view entry 190 ms (the DC V self-cal) | `taplat.py` |
| – | Inrush page taps while armed | not run (not on the list) | – |
| 2 | AUTO coupling, V side | **PASS** with the notes below; the REL / range-hold bug was fixed | `a*.png` |
| 3 | "Leads in A?" reminder | **PASS**; one cosmetic issue | `l-*.png`, `b1-leads-toast-dcv.png` |
| 4 | Burden readout without current | **PASS** | `c*.png`, `m5-settings3-changed.png` |
| 5 | E7 DC V on AD2 | **PASS**; the clip autorange was seen on the device for the first time. Bug fixed: Wave view `CONF?` in DC V | `e*.png`, `e7-*.csv` |
| 6 | The calibrator's "50 Hz" | **calibrator / tool, not the meter**: `mr9270.py` frequency scale fixed | below |

### 2. AUTO coupling (doc acdc-stock-vs-opendm40.md §11, voltage side)

- **Glyph:** 16f5b0f (the "~/⎓" button) **is** in a0985e4 (`git merge-base --is-ancestor`).
- **Taps:** four taps on V from DC V: FUNC? `VOLT:AC` -> `VOLT:ACDC` -> `VOLT:AUTO` (`AUTO,DC`) -> `VOLT`. The button glyph is DC / AC / AC+DC / the AC/DC mark; "AUTO" (F14) sits left of the glyph above the unit. Zoomed: no overlap with the digits, the glyph or the button edges (`a1-sheet.png`, `a1-zoom.png`).
- **Range page:** the 4-button row DC / AC / AC+DC / AC/DC, with AC/DC selected in AUTO; a tap on each selects it (VOLT:COUP? DC / AC / ACDC / AUTO,DC) (`a2-range.png`).
- **DC 5 V in AUTO:** `AUTO,DC` throughout (14 s), aux AC ~0.
- **40 Hz square (0..9.5 V)**: DC part 4.766 V, AC part 4.763 V, ratio 0.999. That is inside the 0.75..1.5 band, so:
  - **From DC (5 V or 1 V DC) to the square: stays DC** (20 s watched). This is the band, as designed.
  - **Entering AUTO afresh on the square: AC, 5 of 5 times**, decided 3.2 s after `VOLT:COUP AUTO`.
    - The cause is the first evaluation: the first AC-phase results after the entry (and the autorange step 6 V -> 60 V, the total is 6.7 V) are high: 5.93, then 5.11 V, then 4.96, then 4.766 V by ~5 s.
    - The doc's "its AC part can still be settling (-1 %, emulator)" is **+7 %** on the device, so on entry a near-tie up to AC ~ 0.93 x DC picks AC, and the band then keeps it.
    - Harmless for the band's purpose (either part is inside it), but the doc's claim is too optimistic. Possible change: skip the first evaluation after an entry / unit change (the first decision ~1.9 s later). Not changed.
  - **Square (AUTO,AC) -> 1 V DC: DC after 5.4 s and 5.1 s** (two runs; doc 2..6 s). Within the 2..6 s window.
  - An AC-dominant signal (DC -> AC switch) cannot be made with this calibrator (only DC and the 0-based square).
- **REL:** REL on in AUTO,AC (the main value 0.000, REL 4.763), then the source -> 1 V DC: REL went off at the switch (PASS). **But the status bar went from AUTO+ to MANU, and the range stayed on 60 V** (1.000 V shown with 3 decimals) (`a67.png`).
  - Cause: `Meter::update` drops REL on the AUTO switch (`rel_off`), but only `App::rel_toggle` releases the range hold that REL took.
  - **Fixed** (app.rs `reading`): REL ended by a reading releases the hold, as `rel_toggle` does.
- **HOLD:** HOLD on AUTO,AC 4.765 V, the source -> 1 V DC: the held value and its AC glyph stay (PASS); READ? gives the live 0.9998 V. RUN releases it: AUTO,DC 0.9998 V (`a8-10.png`).
  - Cosmetic: during the HOLD, the switch's static redraw changed the bargraph scale to -6..+6 (bipolar, DC) under the held AC value. Not changed.
- **SCPI:** `VOLT:COUP AUTO`; `VOLT:COUP?` AUTO,DC; `FUNC?` "VOLT:AUTO"; `READ?` 0.99954; `CURR:COUP?` DC; `CONF?` "VOLT:AUTO +6,+1E-04". `LOG:STAT ON` is refused (`-100,"Settings conflict"`), `LOG:STAT?` 0. `CONF:VOLT:AUTO`, `FUNC "VOLT:AUTO"` and `MEAS:VOLT:AUTO?` work, with no error.
- **Reboot in AUTO** (`DBG:RESET` 10 s after the change): it comes back in "VOLT:AUTO" `AUTO,DC` (PASS).

### 3. "Leads in A?" (doc power-meter-and-misconnect.md §4.9)

`DBG:LEADs?` gives the count and from/to. The toast was caught with 100-row LCD reads 0.3 s apart (a tight read loop holds the repaint; with the pauses it does not).

| change | path | reminder | toast |
|---|---|---|---|
| DC A -> V | tap on V | **yes** (n+1, 9 -> 1) | 0.9 .. 2.05 s after the tap, gone by itself (~1.5 s) |
| AC A -> ohm | `CONF:RES` | **yes** (5 -> 10) | yes |
| power -> Hz | `CONF:FREQ` | **yes** (18 -> 14) | yes |
| A AC+DC -> V AC+DC | `CONF:VOLT:ACDC` | **yes** (17 -> 16) | yes |
| DC A -> AC A | `CONF:CURR:AC` | no (PASS) | only the V-JACK alert's own toast "Voltage on V jack, no current" (1 V on V/COM in AC A: correct) |
| V -> A, A -> power | tap / `CONF:POW` | no (PASS) | – |
| `SYST:ALER:ENAB 7`, DC A -> V | SCPI | no (PASS) | – |
| `SYST:ALER:ENAB ON` (15) | SCPI | yes again | yes |

- **Settings 3/3 "A-lead reminder" OFF:** `SYST:ALER:ENAB?` 7. "Jack alerts" cycled ON -> A-JACK only -> OFF -> ON gave 1 / 0 / 7: the reminder bit stays off (PASS). DC A -> V with it OFF: no reminder.
- **Persistence:** OFF survived a `DBG:RESET` (7 after the reboot). It is restored to ON (15).
- **Volume:** the key / alert volume is 0 % (Settings 1/1, `m2-settings1.png`), so the toast shows with no beep. The beep itself cannot be heard by the agent; volume 0 = `buzzer(true, 0)` = silent by design.
- **Toast position:** the toast sits over the DC V reading (`b1-leads-toast-dcv.png`), with no overlap with the frame.
- **Cosmetic (open):** while the toast is up, the rest of the new page is not repainted. The chart labels still read "0.50uA / 0.00uA / -0.50uA", the REL box "uA", and MAX / AVG / MIN are empty, under a DC V main value. This lasts ~1.5 s, until the toast goes. The toast is due at the first reading (`leads_tick`: `meter.live().is_some()`), before the chart / stats pass has run. A fix would wait for one full DMM draw after the first reading.

### 4. Burden readout (doc §3.15, the steps without current)

- **Setting:** Settings 3/3 "Burden voltage" OFF (stock) -> ON (A, mA, uA); `DISP:BURD?` 0 -> 1. It survived the reboot (`DISP:BURD?` 1 afterwards). It is restored to OFF.
- **FETC:BURD?:**
  - DC V: `+9.90000000E+37` (PASS).
  - DC A (uA range, 2.95 nA): 1.4026E-07 V = 47.50 ohm x I.
  - AC A: 1.1694E-07 = 47.50 x 2.462 nA.
  - `POW:BURD?` 47.5 (the default R_path).
- **Off:** `DISP:BURD OFF` makes the duty come back in AC A, while `FETC:BURD?` still answers (3.2E-08).
- **Screens:** DC A aux1 "Burden 0.00mV"; AC A aux1 Hz / aux2 "Burden 0.00 mV"; with OFF the duty "0.0 %" is back. No overlap, no stale caption (`c-sheet.png`).

### 5. E7 DC V on AD2 (feasibility §13)

- **Wave view from DC V AUTO, 1 V:** "DC V 2V", 2 V/div, Vrms 1.00 V, `SCOP:MEAS:VAVG?` 1.0022 / 5.0042 / 10.019 V at 1 / 5 / 10 V.
- **Clip autorange (first time on the device):** at 15 V it stepped up to 20 V/div (the 60 V range; `CHAN1:SCAL?` 20): PASS. VAVG at 15 / 20 V was 14.938 / 19.910 V (one pixel = 0.8 V at 20 V/div). This ticks the TODO's "clip autorange on the device (emulator only)" (`e3-wave-dcv-20v.png`).
- **Bug: `CONF?` in the Wave view in DC V answered `"VOLT +6.000000E-04,+1.000000E-08"`.** The streamed row's format match (2b8a124) knew AC V / AC mV / A only, so DC V (mode 1) and DC mV (3) fell into the uA branch. **Fixed**: `0 | 1 => V`, `2 | 3 => mV` (DC V 6 V range -> "VOLT +6,+1E-04").
- **Trigger level:** the trigger level badge reads "0.00uV" at 0 V (`fmt_v`: < 1e-6 -> u); cosmetic, all Wave views.
- **`LOG:SOURce AD2` host sessions, DC V, 4800 S/s, 3 s, 0 lost:**

  | level | mean | READ? | difference |
  |---|---|---|---|
  | 1 V | 0.99906 | 0.99971 | -0.6 mV, sd 0.32 mV |
  | 5 V | 4.99695 | 4.99785 | -0.9 mV, sd 0.44 mV |
  | 20 V (60 V range) | 19.99680 | 19.9983 | -1.5 mV, sd 2.1 mV |

  This is within 0.02 % of the range, as in 13.4.
- **Tool note:** the first session failed in `dm40scpi.py` with `LOG settings: -100,"Data stale"`, an error another query had left in the queue. **Fixed**: `log --source` sends `*CLS` first.

### 6. The calibrator's "50 Hz" and the millivolts in sigrok

`tools/mr9270.py` had `"frequency": (2, 0, 1.0, "Hz")`, so `set_output("frequency", 50)` wrote i32 50. In func 2 mode 0 the unit is **milli-Hz** (40000 = 40.0017 Hz on the DM40 FREQ, as the header of this doc says), so 50 = **0.05 Hz**: a 20 s square.
- **Measured** (`poll.py`, DC V READ? every 0.3 s for 45 s): 9.5175 V (drooping slowly to 9.5166 V), then 7.73 (an edge), then 1.39 mV for 9.6 s, then 9.5172 V again.
- **The first high phase after the value was written lasted ~31 s.** The calibrator starts the period at the write.
- **The meter reads both levels right:**
  - DC 9.5 V from the calibrator reads 9.4961 V.
  - DC 10.48 V reads 10.4744 V.
  - The square's low level is the calibrator's own 1.4 mV.
- So sigrok sampled a low phase ("millivolts"). The **+10.48 V seen later over BT** is not this mode's high level (9.52 V into 10 Mohm here). It is not reproducible from the calibrator's frequency output; my guess is that it came from another calibrator setting at the time.
- **Fixed:** the SCALE entry is 1000.0 (milli-Hz), so `set_output("frequency", 40)` = 40 Hz.

### Fixes (on rust-fw-poc after a0985e4; image below, not on the device)

1. **app.rs `reading`:** REL ended by a reading (the AUTO coupling switch) releases the range hold REL took (`dmm.hold_range(false)`, as `rel_toggle`).
2. **app.rs `CONF?` in the Wave view:** DC V / DC mV (modes 1 / 3) are formatted as V / mV.
3. **dlog.rs `FPar::parse`:** moved to region B (`.text.vdisk_rd`; the closure removed so that no symbol stays in C).
4. **tools:** `mr9270.py` frequency in milli-Hz; `dm40scpi.py log` sends `*CLS` first.

Not changed (described above):
- `prof::report` in C (1b): a DBG reply; a host polling DBG:PROF? during a 38400 session loses samples. Move it to B (~1 KB) or document "no DBG:PROF? during fast sessions".
- `App::show` / `App::key` in C at the Wave view exit (1e).
- The leads toast over the unrepainted page (3).
- The AUTO entry bias (2).
- The bargraph under HOLD (2).
- The unexplained stale SCOP006 (1c').

### State at the end

- **Device:** DMM page, DC V, 1 V.
- **Settings:**
  - unchanged: volume 0 %, backlight 10 %, Dim / Shutdown Never, Bluetooth ON;
  - restored: A-lead reminder ON (`SYST:ALER:ENAB?` 15), Jack alerts ON, Burden voltage OFF;
  - power meter coupling DC (as found).
- **Logging:** `LOG:DEST HOST`, `LOG:STAT 0`. The generator output is off (STOP; it was switched on by mistake for ~20 s at 2 MHz / 3 Vpp with nothing on its jack).
- **Files added on the SPI disk:** LOGS/FAST009 / FAST010.DLG (two 20 s 38400 sessions), SCOPE/SCOP005..009, SHOTS/S008..S010.BMP.
- **Calibrator:** 1 V DC, enabled.

## 5e52761: fonts compacted, region A2, power meter round 5 + V2, UF-26, the a0985e4 fixes (2026-10-22 23:05-23:40)

The owner installed the image. `DBG:BOOT?` matched the header of `dm40-fw-5e52761.bin` (marker 0x1653E9AC / 0x00097874 / 0x00019A78, VALID), crash count 0, reset reason SW.
- **Wiring:** the calibrator on V/COM (1 V DC, the 40 Hz square for two steps), the A jack open, the volume mounted read-only.
- **Input:** DBG:TAP / KEY; screenshots are DBG:LCD? reads (rgb666). Pixel comparisons use the a0985e4 device shots (`devchk-a0985e4/`) or ui-render (2x, downsampled).
- **Files:** scripts and screenshots are in the session scratchpad `devchk-5e52761/` (not in git).
- **Crashes:** none, two SW resets (the reboot checks).

### Summary

| # | Check | Result | Evidence |
|---|---|---|---|
| 1a | Region A2: the install | **PASS**: marker VALID = the image header; `INFO/FIRMWARE.BIN` = the installed file (sha1 5e826c88…, 620 672 B). The file is read back from flash, so the update body's sector 1 (A2) is intact | `dm40dbg.py boot --expect` |
| 1b | Region A2: logger panel text, AD2 38400 to disk (AC V, 20 s) with the panel up | **PASS**: "DATA LOGGING / AD2 38399/38400 Sa/s / 206329 samples / FAST012 215 KB" (ASCII26) correct; max_lat 0.28 us (no screenshot) and 0.30 us (a screenshot mid-run), 768 003 samples, 0 lost | `l1-panel.png`, `f2.log` |
| 2a | Fonts: menu, Settings 1/2/3, Hold & Limits, keypad, Record, Power Settings, generator | **PASS**: pixel-identical to the a0985e4 device shots (0 px; the generator differs only in the uptime) | `pdiff.py` |
| 2b | Fonts: Help 1-7 | **PASS**: pixel-identical to the ui-render renders | `h1..h7.png` |
| 2c | Fonts: DMM (V, A, ohm, cap, diode, continuity, Hz, temperature), power DC / AC / AC+DC, the keypad, gallery + image view, scope, combined view, Wave view (AC V, DC V) | **PASS** (looked at each, zoomed where dense): no glyph damage. Layout findings below (none from the fonts) | `sh*.png` |
| 2d | Draw times | **PASS**: tap -> draw (loop max1s) range page 18 ms (a0985e4: 28-38), Hold & Limits 25 (37-51), Record 29 (23-37), menu 22 (37-51), Settings 25 (25-39), Power Settings 23 (31). Page switch (`tick` max, 2 runs): power -> DMM 21.0 / 21.2 ms, DMM -> power 14.2 / 14.2 ms (emulator 15.7 / 10.5: flash wait states). Steady DMM DC V (10 s): d.read avg 800 / max 5098 us, d.chart avg 256 / max 1388 us | `taplat1.txt`, `prof-*.txt` |
| 3 | Power meter round 5 + V2 (no current) | **PASS** for every step without current; 2 cosmetic notes | below |
| 4a | Re-check: a host copying CSVs during the Wave view at 38400 | **PASS**: 17 files (SCOP000-009, LOG000-004, FAST000, FAST008; 12.0 MB, 95 KB/s, 0 errors): max_lat 0.30 us, overruns 0 (a0985e4: 21.4 us). `dropped` +831 once, near the end of the copy (the view's ring) | `copy1.txt` |
| 4b | Re-check: AUTO + REL, then a switch | **FAIL -> fixed**: REL went off and the range was released (60 V -> 6 V, 4 decimals), but the status bar stayed **MANU** until a static redraw (range page and back: AUTO+). Cause below | `r2-rel-on.png`, `r3-after-switch.png`, `r6-redrawn.png` |
| 4c | Re-check: Wave view `CONF?` in DC V | **PASS**: `"VOLT +6.000000E+00,+1.000000E-04"`; VAVG 1.0021 V, 2 V/div | `w5-wave-dcv.png` |
| 5 | UF-26: DC mA, 60 mA range, A jack open | **PASS**: 0.001 mA, no 4-20 mA percentage. Showing it above 3.6 mA: blocked (no current through A) | `m3-60ma.png` |
| 6 | Combined view, power function | **FAIL (layout, open)**: the strip's unit box clips "mW" to "mV" plus a sliver | `g7-combined.png`, `z-comb.png` |

### 3. Power meter (doc §3.16 / §3.17 device lists, the steps without current)

- **Entry:** V -> W button: `FUNC?` POW, `POW:COUP?` DC, "I ranging..." for the first seconds (the uA hunt with the A jack open), then the DC page.
- **Tap cycle:** DC -> AC -> AC+DC -> DC, glyphs on the button and by the value (`p3..p5`, `p8`).
- **Reboot in AC+DC** (`DBG:RESET`): back in POW, `POW:COUP?` ACDC (PASS).
- **"At load":** `POW:BURD:STAT?` 0 after the update; Power Settings "At load" -> 1, help line "At load ON: V at source, so load sees V - I*R", the page shows "@load" after the V unit and at the main readout's top left (`q2-page-atload.png`). It survived a reboot (1). Off again -> 0, help line "OFF: V, P at the jacks. ON if V leads at source", no "@load" left (diff vs the earlier page: only the digits and the chart). Help 7/7 has "MF: At load (off)" / "@load: V - I*R".
- **AC on 1 V DC:** `FETC:POW:ALL?` P 0, S / Q / PF 9.91E37, V 0, I 0, F 9.91E37; the page shows 0.0000 V / 0.00 uA / 0.0000 mW and "--" with no units (`p4-ac.png`).
- **AC+DC on 1 V DC:** Q 0 (`0.00 mvar`), V = V_dc 1.0001 V, V_ac 0.0000; PF -1.000 (I is the open jack's -0.08 uA, so the sign is noise). "DC?" badge after ~6 s, `SYST:ALER?` DC, toast "No AC: DC is more precise" (`tn-03.png`).
- **DC on the 40 Hz square** (0..9.5 V): V 4.765 V, "AC?" badge, `SYST:ALER?` AC, toast "AC present: use AC+DC" (`tq-02.png`).
- **Sub-lines (V2):** "DC 1.0001 V AC 0.0000 V", "DC -0.13 uA AC 0.00 uA", "DC -0.0001 mW + AC 0.0000 mW" in place, no overlap at 2x zoom (`z-acdc.png`). A tap on the value -> Wh: the P sub-line empties, V / I stay; -> VAh; -> W, the P line back (`sh3.png`).
- **Back to DC:** the DC layout (VAh row, chart from 176) with no leftover pixels: the diff against the first DC shot is only the uptime and the chart trace / chips.
- **Cosmetic:** a coupling switch keeps the old layout and values until the first reading of the new coupling, up to ~4 s. `POW:COUP ACDC` from DC: the DC layout with the DC glyph by the value under an AC+DC W button for ~4 s (the "No AC" toast came inside that time); AC+DC -> DC: one frame of the AC+DC totals without sub-lines.
- **Blocked (A jack not wired):** every step with current (V at 1 / 2 A with "At load", residues against the floors, mains / motor PF, ripple with current, the parts with a load).

### 4b. The status bar after a range release

Sequence: AUTO coupling on the square (`AUTO,AC`, 60 V), REL on (REL 4.765), calibrator -> 1 V DC. After ~5 s `AUTO,DC`, REL 0.0000, `CONF?` "VOLT:AUTO +6,+1E-04" (6 V, so the 5e52761 fix released the hold), but the status bar showed "MANU" for > 20 s.
- **Cause:** `DmmView`'s status key was `(r.auto && !r.auto_plus, hold, r.auto_plus, ...)`. Under a manual range `v_auto` stays 1, so `auto_plus` is set and MANU gives `(false, .., true)`, the same key as AUTO+. MANU -> AUTO+ never repainted the label. Paths that invalidate the view (REL tap, page switch) hid it.
- **Fixed** (ui.rs): the key holds `r.auto`.

### 6. Combined view strip: power units clipped

- **Symptom:** in the power function, the strip reads "-0.0001m" plus a cut "W" that looks like "V" (`z-comb.png`).
- **Cause:** `ST_UNIT` is 40 px and `st_text` clips without a warning. ui-render's `c12-strip-power` render has the same cut, and no rule checks the value / unit boxes.
- **Widths (SC_DIGITS):** "mW" 59 px, "kWh" 63, "mWh" 75; values up to 103 px ("-3.7799").
- **Space:** value + gap + unit = 88..250 (162 px). 103 + 4 + 75 does not fit, so shifting the unit box left is not enough. This needs a layout decision: draw the unit in a smaller font when it does not fit, or move the sub column. A draft ui-render rule found 5 cases (mw, main-wh, main-mwh x2, main-zero). Not changed.

### Other findings (cosmetic, not from the fonts)

- **Help 5/7:** the title "Help 5/7: Record + alerts" runs into "< Back" (one space). Help 2/7 "Power key held 1.5 s" touches its value column. Both are the same in ui-render.
- **Chart axis labels:** in a function that reads OL with no earlier value, the labels keep the previous function's unit: cap "0.050MΩ", diode "MΩ", temperature "Hz".
- **Wave view entry from DC V:** `DBG:ISR?` `dropped` +7087 (~185 ms, the DC V self-cal at the entry), 0 from AC V. Exit at 38400: max_lat 4.0 / 5.8 us, late 1 (the known App::show item).
- **A range switch spike:** a manual 600 uA -> 60 mA switch with the A jack open drew a 0.315 mA spike in the chart (MAX 0.265 mA) for one reading.
- **`CONF?` resolution follows the display:** "+1E-05" while the DMM shows the extra digit, "+1E-04" without it.

### Fix (branch `devchk-5e52761`, on rust-fw-poc d6f5e96; not on the device)

1. ui.rs `DmmView`: the status-bar key holds `r.auto` (4b).
- **Build:** region B 78 192 B free, C 17 420, stack headroom 21 228.
- **Tests:** ui-render 51 495 cases, 0 violations.
- **Device re-check:** 4b (expect AUTO+ right after the switch).

### State at the end

- **Device:** DMM page, DC V, AUTO+, 1 V.
- **Settings:**
  - unchanged: volume 0 %, backlight 10 %, Dim / Shutdown Never, Bluetooth ON.
  - restored: "DMM row" back to Bargraph (I changed it to History by mistake in Settings 2/3).
  - checked: Settings 1/2/3 pixel-identical to the start; `SYST:ALER:ENAB?` 15, `DISP:BURD?` 0.
  - couplings: V DC, A DC with AUTO range, W DC, "At load" OFF.
- **Logging and generator:** `LOG:DEST HOST`, `LOG:STAT 0`; generator `OUTP?` 0 (never switched on).
- **Files added on the SPI disk:** LOGS/FAST011 and FAST012 (two 20 s 38400 sessions).
- **Calibrator:** 1 V DC, enabled.

## ffa16ef: the RAM round (screenshot streamed from the panel, FFT work buffer in the disk arena) (2026-10-22 23:43 - 2026-10-23 00:25)

The owner had installed ffa16ef (it contains 290ebce..f06f046). `DBG:BOOT?` showed marker 0x03A8FC57 / 0x000976F4 / 0x000164D9, crash count 0, HEALTHY; no reset during the run (uptime continuous).
- **Checks:** docs/ram-budget.md "Device checks still open" 1-6, plus the AUTO OVERLOAD item.
- **Wiring:** the calibrator (`/dev/ttyACM1`) on V/COM: 1 V DC, a 1 -> 2 -> 1 V step, and the 40 Hz square (func 2 mode 0, 40000 mHz). The A jack and the MCX probe were not wired.
- **Input:** UI input came from DBG:TAP / DBG:KEY / DBG:SWIPE D,3 (the 3-finger swipe).
- **Panel reads:** these were DBG:LCD? reads, decoded as `dm40dbg.py shot` does (RGB666, dummy word dropped).
- **BMPs:** pulled with `MMEM:DATA? "Snnn.BMP"`.
- **Files:** scripts, BMPs and PNGs are in the session scratchpad `devchk-ram/` (not in git).
- **USB:** at ~23:51 one write got ENODEV. Another agent's PulseView scan had opened the meter over libusb, which re-bound usbtmc. The device did not reset. A 1209:0001 emulator on vhci_hcd was attached at the same time, so the scripts pinned `DM40_USBTMC=real`.

### Summary

| # | Check | Result | Evidence |
|---|---|---|---|
| 1a | Shot of a menu = the frozen panel | **PASS**: 0 px differ (BMP vs the panel read before the gesture, during the freeze and after) | `s1-menu.bmp` |
| 1b | Shot of AC+DC (did not fit the old CAP) | **PASS**: BMP = the panel at the gesture, 0 px | `s2-acdc.bmp` |
| 1c | Shot of a running scope trace (Wave view, 40 Hz square, triggered) | **PASS**: BMP = the panel at the gesture, 0 px | `s3-scope.bmp` |
| 1d | Colour-rich: the scope's CHA colour picker; RGB666 -> 565, the dummy word | **PASS**: 0 px against all three panel reads; the five colour names hold the exact `scope_ui::PALETTE` values | `s4-colour.bmp` |
| 2 | Freeze time on the real panel | **PASS, a bit longer than estimated**: touch works again after **5.3-5.5 s**, toast at **~6.1-6.5 s** (estimate 4-5 s) | `freeze2.py` |
| 3 | READ? / DMM / loop max during a shot | **PASS**: READ? answers all through and follows a 1 -> 2 -> 1 V step; data-phase passes <= 4.5 ms; 3 passes of 50-78 ms = the FAT x2 + directory block writes, unchanged since 0d165e4 | `chk3.py`, `c3-*-prof.txt` |
| 4 | Shot during a fast-log session | **FAIL -> fixed (7409b92)**: no "Disk busy". The request waited for the session's end, then wrote S023 of a half-repainted page | `chk4.py`, `c4-busy.png`, `c4-S023.png` |
| 5 | Scope FFT during CSV commits and during a shot | **PASS**: the FFT stops during the shot and resumes; the peak readout is identical before and after; the commits stall it < 0.2 s; no garbage | `chk5a.py`, `chk5b.py`, `c5a-sheet.png` |
| 6 | calbackup, CALIB / BOOTDATA / UID | **PASS**: `CALBK SAFECOPY_PRESENT` (written 0, conflict 0, differs 0); the three SHA-256 equal before and after (after a re-mount) | `hash-*.txt` |
| - | f06f046's own item: a toast / the auto power-off warning / the firmware-file dialog due during a shot are not drawn | **covered only in part**: the shot's own toast came after the freeze in every run. The power-off warning and the fw-file dialog were not run | - |
| 7 | AUTO OVERLOAD on AC 1 A (TODO 1bffde2, couptest iac repro) | **blocked**: needs current through the A jack, which is not wired | - |

### 1. The BMP vs the panel

`shotcmp.py` reads the panel right before the gesture, sends `DBG:SWIPE D,3`, waits for the job (`LOG:CSV:FREE?` answers "Mass storage busy" while `screenshot::busy()`), reads the panel twice more, then reads it after the job. It compares at 565 with `rgb666_to_565`'s truncation.

All BMPs have the stock layout:
- 307 266 B, 66-byte header;
- BI_BITFIELDS with masks F800 / 07E0 / 001F;
- 480 x 320, bottom-up.

| screen | file | BMP vs before the gesture | vs during the freeze | vs after |
|---|---|---|---|---|
| Menu | S019 | 0 | 0 | 0 |
| AC+DC (DMM page, 1 V DC) | S023 | **0** | 244 (chart, rate, T counter) | 616 (new readings) |
| Scope, Wave view, 40 Hz square | S021 | **0** | 22 427 (the "Saved" toast) | 22 427 |
| CHA colour picker | S022 | 0 | 0 | 0 |

- **"During" read:** with a shot running, an 80-query full-panel read takes 2.9-7 s, longer than the data phase under that load. Its lower rows were read after the freeze had ended: the toast was caught in the scope run, newer DMM readings in the AC+DC run. For a moving screen, the read before the gesture is therefore the reference. It is the frozen screen:
  - DBG:LCD? holds drawing for 300 ms;
  - the shot starts at the first tick that may draw (`shot_req`);
  - the freeze then begins at once.

  The BMP equals that read pixel for pixel on all four screens.
- **Freeze without load:** a separate run (`tstrip.py`) read only two 4-row strips (the T counter and the reading) at 1.4 / 1.9 / 2.9 / 3.4 / 4.4 / 4.9 s. The strips were unchanged while busy and changed at 6.0 s after the job.
- **Colours:** the GRAM's R and B low bits (bit 0 of the 6-bit values) are 0 on every pixel. So the panel stores exactly the 565 that was drawn, and the truncating 666 -> 565 is lossless.
- **Palette:** in the picker, the boxes "red / orange / yellow / green / blue" hold 80 / 168 / 170 / 140 / 113 pixels of exactly F800 / FD20 / FFE0 / 0400 / 001F (two values per box: the ink and the background).
- **Dummy word:** a dummy-word slip would rotate R -> G -> B or shift every pixel. Both checks exclude it.

### 2. Freeze time (`freeze2.py`, AC+DC page, 3 runs)

| event | run 1 | run 2 | run 3 |
|---|---|---|---|
| busy (from `DBG:SWIPE D,3`) | 0.00 s | 0.01 | 0.00 |
| tap on "A" at 2.0 s (during the freeze) | ignored: FUNC? "VOLT:ACDC" | ignored | ignored |
| first tap that took effect (taps every ~0.15 s from 3 s) | 5.47 s | 5.50 | 5.49 |
| job end (`busy()` false) | 6.12 s | 6.15 | 6.07 |
| toast on the panel ("Saved", green) | by 6.47 s | by 6.50 | by 6.42 |

- **Freeze:** ~5.4 s, toast at ~6.1 s. The estimate was 4-5 s, so this is a little longer.
- **Without host traffic:** two shots were busy for 6.01 and 5.98 s.
- **Faster with traffic:** with continuous DBG:LCD? traffic the same job ended in 2.3-4 s. Not investigated.
- **Shot start:** the shot starts 0.2-0.35 s after the gesture when a DBG:LCD? read came just before it (the 300 ms draw hold), else at once.

### 3. DMM and loop during a shot (`chk3.py`, DC V)

| | 10 s without a shot | 10 s with a shot (busy 0-5.1 s) |
|---|---|---|
| READ? every ~0.4 s | 0.99987-0.99994 V | answered every time: 1.0001, then **1.498 -> 1.9992** after the calibrator's 2 V step at 1.8 s, then 1.50 / 0.405 (settling) -> 1.00005 after the 1 V step at 3.7 s |
| DBG:DIAG? loop max1s | 1.2-5.2 ms | 28.5 ms (first second), **4.5 ms** (data phase), >= 65.5 ms (u16 saturated, the end), then 13-25 ms for ~5 s |
| DBG:PROF? loop max | 5.2 ms | 78.5 ms (`tick` 74.5 ms) |
| loop hist | none >= 10 ms | 2-5 ms: 2733; 10-20: 16; 20-50: 2; >= 50: **3** |

- **First second:** the 28.5 ms pass is not attributed. It is probably the Root / Alloc scan, which reads several blocks in one pass.
- **Data phase:** passes stay at <= 4.5 ms (a ~2 ms write slice plus ~1.3 ms panel read).
- **The three >= 50 ms passes:** these are `fat()` x 2 copies and `dir()`, which write their block "whole in one call (H1)": a sector erase plus 16 programs. They are the same as the 74 ms csvlog commit pass of f3aea2f. This code has been unchanged since 0d165e4, so the RAM round did not add it.
- **After the shot:** the 13-25 ms passes are the full repaint after the freeze (measured separately: 22-24 ms for the DMM page and the menu) and the volume refresh.
- **Before vs after:** there is no device number for the old build's shot. The old path ran the same FAT / directory writes, so "about the same as before" holds for the maximum.

### 4. A shot during a fast-log session (`chk4.py`): FAIL, fixed

Steps: `LOG:DEST DISK`, `LOG:DUR 20`, `LOG:STAT ON` (AD1, 38 S/s, FAST013). Once it was running (`LOG:DISK?` state 3), `DBG:SWIPE D,3`.

- **During the session:** no toast. The panel showed the DATA LOGGING panel (`c4-busy.png`), and no BMP was written.
- **After the session:** a shot S023 was written. It is the DMM page right after `draw_static`: no reading, no chart, no statistics, half a status bar (`c4-S023.png`).
- **Cause:**
  - `App::gesture` only sets `shot_req`.
  - `App::tick` serves it only when `can_draw`, and `can_draw` is false for the whole session (`logging`).
  - So `screenshot::start` never ran during the session, and the `Shot::Busy` path of screenshot.rs (`fastlog::holds_disk()` at Bpb) could not be reached.
  - At the session's end, the full repaint (`redraw`) and the shot ran in the same tick. The panel was read before the next passes drew the reading, chart and status.
- **Same after a host session:** the same happens after a host `LOG:STAT ON` session (`DBG:SWIPE D,3` during it, `LOG:STAT OFF`). S027 is a bare page.
- **Not affected:** the scope's MF "PriScr" (S025, a full page). A second swipe while the first shot's toast was up did not start a shot (S026 only).
- **Fix (7409b92, branch `devchk-ram-ffa16ef`; on the device: not yet):**
  - `App::shot_request` answers "Disk busy" at the gesture when `fastlog::holds_disk() || upload::busy()`, for the swipe and for PriScr.
  - A requested shot waits `SHOT_SETTLE_MS` (1 s) after a full repaint (`App::repaint_at`).
  - **Build:** stack headroom 21 228 B, region B 71 748 B free, region C 23 916, SRAM 60 544.
  - **Tests:** touch-test 892 cases, 0 failures.
  - **Image:** `dm40-fw-native-7409b92.bin`, header 0xC3AA3C55 / 0x000962F4 / 0x000120BD. It is built on rust-fw-poc 3028923, so it also carries the commits after ffa16ef.
- **Re-check after the fix:**
  - the swipe during a fast-log session shows "Disk busy" at once, and no S*.BMP appears after the session. The toast draws over the DATA LOGGING panel, which repaints one part per pass, so the panel covers it again within ~1 s: that is expected;
  - the swipe during a host LOG:STAT session: the shot after `LOG:STAT OFF` holds the reading and the chart.
- **Limits of the fix:**
  - it is verified by the build and touch-test only, not on the device or in the emulator;
  - the 1 s settle was not timed against the DMM's first reading after `LOG:STAT OFF`. If the re-check still shows a bare page, the condition should be "a reading drawn since the repaint", not a longer timer.

### 5. FFT in the disk arena (`chk5a.py`, `chk5b.py`)

- **During a shot:** Wave view (DMM source, DC V, 40 Hz square, 10 ms/div), math FFT on, ~7 spectra/s, peak readout "40.0Hz 12.7dBV".
  - `s.fft` calls stayed at 32 463 from the gesture to 5.9 s; the job was busy until 6.7 s.
  - The count was 32 465 at 7.9 s and grew 7 per ~1 s after that.
  - The peak readout box was byte-identical before and after the shot.
  - The spectrum was the same harmonic comb (`c5a-sheet.png`).
- **MMEM:CAT? bursts:** these are single-pass disk users that move `GEN`. During 20 of them in 4.3 s the FFT ran 6.5 /s (7.0 /s after), and the readout stayed identical (`c5a-after-cat.png`).
- **During CSV commits:** MCX source (open input), 2 ms/div, ~33 spectra/s, `LOG:CSV ON` from DC V 1 V.
  - The commits came at 60 s and 120 s (committed 233 -> 497 -> 769 B).
  - In the 0.2 s windows holding the commits the rate was 33.3 and 22.6 /s, against a median of 33.3. The pause is < 0.2 s, and the FFT was not stopped otherwise.
  - The open input's spectrum is a floor (-166 dBV), so "no garbage" here only means that no peaks appeared.
- **Side finding:** in the Wave view, `LOG:CSV ON` was refused once ("Settings conflict", no reading yet) and accepted later (LOG006, state 1). That recording got no rows in 135 s (rows 0, `READ?` 9.9E37): the AD2 stream owns the DMM there. docs/ram-budget.md says csvlog is refused by the Wave view. Either that line or the guard is wrong. Not changed.

### 6. calbackup and the calibration files

- **`DBG:BOOT?`:** `calbackup: status=SAFECOPY_PRESENT written=0 conflict=0 differs=0 ms=45`; crash count 0, HEALTHY.
- **Stack:** the high-water mark after the session was 23 120 / 45 512 B (51 %); it was 20 616 at the start.
- **Hashes:** SHA-256 of INFO/CALIB.BIN 1103b821..., BOOTDATA.BIN b141bc95..., UID.BIN 18b55482... are equal before and after the session. The "after" hash was taken after `udisksctl unmount` + `mount`, so the page cache could not answer.
- **Copies:** the files were hashed on the mount and not copied anywhere.

### State at the end

- **Device:** DMM page, DC V, 1 V.
  - The scope is back to MCX, 2 ms/div, 20 mV/div, DC, math / FFT off, trigger level 0 (AUTO had set 38 mV).
  - CH1 colour unchanged (yellow).
  - The CH1 offset is not restored: AUTO moved it, and the "A" marker sits at about row 228 instead of about 245 at the start (`scope-end.png`).
- **Settings:** not touched (volume 0 %, backlight 10 %, Dim / Shutdown Never as found).
- **Logging and generator:** `LOG:DEST HOST`, `LOG:STAT 0`, no CSV recording. Generator `OUTP?` 0 (never switched on).
- **Files:** my S012-S027, LOG005-LOG008 and FAST013 were deleted (`MMEM:DEL`).
  - `MMEM:CAT?` used / free = 10 829 824 / 5 910 528, the same as at the start.
  - The file list under SHOTS / SCOPE / LOGS on the volume is the same as at the start.
- **Calibrator:** 1 V DC, enabled.

## 7409b92: the shot fix re-check, 76.8 kS/s, AC+DC methods (2026-10-23 00:36 - 02:20)

The owner had installed 7409b92 (it contains rust-fw-poc up to a4dadc7). `DBG:BOOT?` showed marker 0xC3AA3C55 / 0x000962F4 / 0x000120BD, VALID, crash count 0, HEALTHY. No reset during the session: the boot block stayed the same.
- **Wiring:** the calibrator (`/dev/ttyACM1`) on V/COM: 1 V DC, 3 / 5 / 30 V DC and the 40 Hz square (`set_output('frequency', 40.0)`). The A jack and the MCX probe were not wired.
- **Input:** DBG:TAP / DBG:KEY / DBG:SWIPE D,3. Panel reads are DBG:LCD? reads (`devlib.grab`).
- **Files:** scripts, PNGs, BMPs, `.npy` sample dumps and JSON results are in the session scratchpad `devchk-7409/` (not in git).
- **USB:** the scripts pinned `DM40_USBTMC=real`. One run (2.1 below) was killed by a host `timeout` in the middle of a `LOG:DATA?` read. After that every read returned 0 bytes until `USBTMC_IOCTL_CLEAR`. The device did not reset.

### Summary

| # | Check | Result | Evidence |
|---|---|---|---|
| 1a | Swipe during `LOG:DEST DISK` + `LOG:STAT ON` | **PASS**: "Disk busy" (red toast) in the first panel read, 0.00-0.14 s after the gesture, and in the next three reads to 0.6 s. No new S*.BMP after the session | `p1a.py`, `p1a-toast0.png` |
| 1b | Swipe during a host `LOG:STAT` session, then `LOG:STAT OFF` | **PASS** (2 runs): S012 / S013 hold the reading, the bar graph, the chart with its scale, statistics and the status bar. The shot started 1.07 s after `LOG:STAT OFF` (the 1 s settle) | `p1b.py`, `p1b-S012.BMP.png`, `p1b2-S013.BMP.png` |
| 2.1 | 76 800 S/s, AC V, 40 Hz square, raw host drain 60 s | **PASS by the listed criteria**, three findings below: div 1008, dma 1, lost 0, late / slips / overruns / timeouts / bad 0, locked, max_lat 300 ns. **cpu_permille 544** (estimate ~430). Host drain **34.4-35.0 k rows/s**, so ~55 % of the samples were overwritten in the ring | `p2.py`, `a768b.json` |
| 2.2 | The same with `LOG:CHOP OFF` and `ON` | **PASS**: codes in the levels +99.3 k / -99.0 k; no stuck samples (longest run of equal codes 7-8). Only the sample right after a falling edge goes past -107 k (overshoot, to -117.8 k). One mid-level glitch per 2 M samples with CHOP ON, see below | `a768off.json`, `a768on.json` |
| 2.3 | `DBG:AD2:DIV 8,1` at 4800 and 38 400 | **PASS**: lost 0, bad 0, overwritten 0 (host kept up), locked at -1.6 ppm; cpu_permille 28 / 228 | `h4800.json`, `h38400.json` |
| 2.4 | Disk at 76 800, `LOG:DUR 60` | **FAIL**: the rung was 76 800 (tpp 382 / 389 us passed the gate), but the file holds 2 095 713 rows of 4 599 040 conversions. **lost_before sum 2 505 919 (54 %)**. fifo_hw 525 of 8192, 336 pages/s. The coder was CPU-starved; the flash was not the limit | `p4.py`, `disk768.json`, `prof768disk.txt` |
| 2.5 | Wave view 1 ms/div (76 800) and 2 ms/div (38 400), 30 s each | **PASS**: trace and Vp-p 9.94 / 10.0 V right (`wave-1e-3.png`, `wave-2e-3.png`); ISR `dropped` +0 in each 30 s window (22 at the switch to 1 ms/div, before the window), bad / overruns / slips 0, max_lat 280 / 260 ns; cpu_permille 560 / 154 | `p5.py`, `p5.json` |
| 2.6 | After the streams: DC V 1 V, `DBG:HYSPI:SELF? 2000` | **PASS**: 0.999951 V mean of 10 READ? (-0.005 %, range 0.999812-1.000066); SELF? 2000 frames, all error counts 0 | - |
| 3 | AC+DC methods (acdcmeth.py e6 e1 e7 e2 e5, then AUTO with C by hand) | e6 PASS (C fastest) / e1 **B FAIL** (bimodal zero) / e7 (ii) better / e2 AC PASS (C +0.13 / +0.23 % vs A), C 60 V DC +0.28 % / e5 6 V PASS, **60 V FAIL** (+0.09..0.10 %) / AUTO with C: square PASS, **>= 5 V DC FAIL** (range hunting, bogus AC part). The rule for C as default is not met; see "3." | `acdcmeth.jsonl`, `p3.py`, `p3b.py` |

### 1. The shot fix (7409b92)

- **1a:** the session was AD1, 38 S/s, `LOG:DUR 20`. `DBG:SWIPE D,3` went in once `LOG:DISK?` showed state 3. Panel rows 128-192 (the toast rectangle is y 132-188) were read four times in 0.6 s. Each read had 20 837 red pixels and the text "Disk busy". After the session `MMEM:CAT?` listed one new file, FAST013 (and FAST014 in the second run); the S*.BMP list was unchanged. The first run's script failed in its panel read after the swipe, so it gives only the "no BMP" half. At the end of Part 1, after deleting S012 / S013 / FAST013 / FAST014, `MMEM:CAT?` used / free was back to 10 829 824 / 5 910 528, the same as at the start: no BMP was written.
- **1b:** `LOG:DEST HOST`, `LOG:STAT ON` (AD1) on DC V 1 V; `DBG:SWIPE D,3` 2 s later; `LOG:STAT OFF` 3 s after the swipe. Mass storage was busy from 4.07 to ~10 s after the swipe, so the shot started 1.07 s after `LOG:STAT OFF`. The chart in both BMPs has only a few points (the statistics restart after the session: T 00:00:00). This is the page as it is drawn, not a bare page. **The 1 s settle is enough on DC V**; the "a reading drawn since the repaint" variant is not needed for this case.

### 2. 76.8 kS/s (c58ae48, TODO "76.8 kS/s")

Setup: `CONF:VOLT:AC` (6 V range, AD2 r26 0x20, AC-coupled), the 40 Hz square of the calibrator (READ? 4.768 V), `LOG:SOUR AD2`, `LOG:SRAT 76800`. `p2.py` drains `LOG:DATA?` (500 records per block) into numpy for 60 s, then reads `DBG:ISR?`, `LOG:COUN?`, `LOG:GAP?`, `LOG:RATE?`.

**2.1-2.3, the host drain runs**

| run | rows got | drain rows/s | captured | overwritten | lost | cpu_permille | max_lat ns | rate_mhz | ppm_x10 | div |
|---|---|---|---|---|---|---|---|---|---|---|
| 76 800 CHOP default (ON) | 2 062 907 | 34 378 | 4 596 477 | 2 531 522 | 0 | 544 | 300 | 76 723 645 | +9951 | 1008 |
| 76 800 CHOP OFF | 2 098 166 | 34 962 | 4 596 976 | 2 496 762 | 0 | 544 | 300 | 76 723 683 | +9946 | 1008 |
| 76 800 CHOP ON | 2 096 633 | 34 939 | 4 596 698 | 2 498 017 | 0 | 544 | 300 | 76 723 803 | +9931 | 1008 |
| 4800, DIV 8,1 | 191 865 | 4 797 | 191 882 | 0 | 0 | 28 | 240 | 4 800 008 | -16 | 1008 |
| 38 400, DIV 8,1 | 1 536 158 | 38 397 | 1 536 658 | 0 | 0 | 228 | 240 | 38 400 062 | -16 | 1008 |

late, bad, timeouts, overruns and slips were 0 in every run, locked 1, dma 1. `LOG:GAP?` bad 0.

- **Host drain.** The host gets ~35 k rows/s at 76 800 and the full rate at 38 400. The limit is on the meter's side (see 2.4): the main loop gets ~46 % of the CPU and moves ~35 k records/s, by USB or to the disk.
- **CPU.** cpu_permille is 544 at 76 800, not ~430 as estimated. The emulator had 29.5 %. Hybrid frames at 38 400 cost 228 against 148-154 for the /16 DMA frames at the same rate.
- **Codes.** The levels are +99.3 k / -99.0 k with a ~1 k droop over a half period (AC coupling), the same with CHOP ON and OFF. The only codes past the square's +-107 k are the 1-2 samples right after a falling edge (down to -117.8 k, overshoot). They show at 76 800 only: 4800 and 38 400 stay within -106.3 k / +107.5 k. The longest run of equal codes is 7-8 samples (codes are multiples of 8), so nothing is stuck. Half periods are 956-959 samples in the contiguous stretches between ring overwrites (960 expected at 76 800).
- **Finding 1: mid-level glitches.** One isolated sample far from both neighbours, away from any edge, per CHOP ON run: seq 752 877 (-65 616 between -98 272 / -98 272) and seq 133 301 (82 960 between 98 288 / 98 320). There were none with CHOP OFF (2.1 M samples) and none at 4800 / 38 400. `bad` stayed 0. The XOR against the neighbour is not one bit (0x7F90, 0x3BE0), so this is not a torn read of two adjacent samples. It is about one per 2 M samples, too few to tell whether CHOP is the cause.
- **Finding 2: the rate at 76 800.** `rate_mhz` is 76 723.6 S/s, so ppm_x10 is about +9950 (period +995 ppm against the nominal 76 799.016). At 4800 / 38 400 the same chip locks at -1.6 ppm. The record stamps give a third rate: the mean step inside contiguous stretches is 13.0592 us = **76 574.5 S/s**. `LOG:RATE?` gives 76 576 and captured / 60 s 76 600. At 38 400 the stamps give 38 400.07 S/s, matching `rate_mhz`. So at 76 800 the PLL's rate and the stamps disagree by ~1.9 ‰, and both are off the nominal rung. The CSV header's `nominal_sps` is off by 0.3 % against the stamps. Not investigated.
- **Finding 3: the killed run.** The first 76 800 attempt used `dm40scpi.py log ... -d 60`. Its `drain()` returns only when a block has < 500 records, which never happens when the meter produces faster than the host drains. So `-d` was never checked and the run went on until my `timeout` killed it (~150 s). That session's counters show **overruns 1, max_lat 1160 ns**, lat_pc 0x08094a7e. They belong to the end of that session (a killed host mid-read, then `USBTMC_IOCTL_CLEAR`); the three clean runs show 0 / 300 ns. dm40scpi.py needs the duration check inside `drain()` for rates above ~35 k/s (not changed here).

**2.4, disk at 76 800: FAIL**

`LOG:SOUR AD2`, `LOG:SRAT 76800`, `LOG:DEST DISK`, `LOG:DUR 60`, `LOG:STAT ON`. Free space was 5 910 528 B (the whole free space is allocated).
- **Prep:** 21 s (prep_pct 20 / 42 / 64 / 87 % at 5 / 10 / 15 / 20 s).
- **Rung:** `LOG:PRE?` `0,0,0,13,76374,1,1,76800,76799016,32,1` = 76 800. `LOG:DISK?` tpp **382 / 389 us** (mean / max), within the 0.4-0.7 ms expected and under the 800 / 1500 us gate.
- **End:** `5,13,5910528,4827914,2095713,100,525,20155,382,389`: done, FAST013, 4 827 914 B, **2 095 713 rows**, **fifo_hw 525** of 8192, **20 155 programs = 336 pages/s**.
- **DBG:ISR?:** captured 4 599 040, **dropped 2 501 279**, lost 0, late / bad / overruns / slips 0, max_lat 320 ns, cpu_permille 544.
- **The file:** `dlog.py` decodes it: 2 095 713 rows over 60.000 s, **lost_before sum 2 505 919** in 65 385 holes. The holes come **exactly every 32 rows** (`BATCH` = 32), 27 conversions each (median; mean 38, max 544).
- **38 400 for comparison (`LOG:DUR 20`):** rung 38 400, div 16, cpu_permille 148, 768 003 rows, lost_before 0, dropped 0.
- **Profile (`DBG:PROF:RESET` at state 3, `DBG:PROF?` after a 20 s session):** 1272 main-loop passes/s, the `usb` section (the fast-log Run pass runs in it) avg 718 us per pass, 91 % of the loop time. The hole every 32 rows means each Run pass drained one batch only. `RUN_BUDGET_CYC` (300 us of cycles, counted with the ISR's share in it) is used up by one 32-record batch while the ISR takes 54 % of the CPU. Between passes ~70 conversions arrive, so the 2048-record ring overflows.
- **Diagnosis:** the coder is CPU-starved; the flash is not the limit (fifo_hw 525 B, tpp 0.38 ms, 336 of ~767 pages/s needed). The emulator passed because its ISR cost was 29.5 %. Directions (owner's choice, not built):
  - (a) a CPU gate next to the tpp probe: measure cpu_permille (or the ring fill) during Prep's probe and run at 38 400 above a limit;
  - (b) more coder work per pass while the ring is over half full (several batches, budget by records not wall cycles), if the CPU left (~46 %) is enough. The host drain's ~35 k/s suggests the main loop as a whole cannot move 76.8 k records/s, so (b) alone may not be enough;
  - (c) cheaper hybrid frames (the ISR at 544 ‰ against the ~430 estimate).
- **Not run:** the noisy-input repeat (sine + generator noise) of the TODO step: the clean case already fails.

**2.5, Wave view**

From AC V: MF key 3, tap (210, 260) opens the Wave view (10 ms/div, 4800 S/s, 40 Hz square, Freq 40.0 Hz, Vp-p 9.96 V). Then `TIM:SCAL 1e-3` / `2e-3`, 3 s settle, 30 s with `DBG:ISR?` and `LOG:COUN?` every 10 s.
- **1 ms/div:** rate_mhz 76 723 541, div 1008, cpu_permille 560. The ring was overwritten 22 times at the switch to 1 ms/div, then **+0** in 30 s. Vp-p 9.94 V; the edge with its overshoot, Vrms 4.81 V.
- **2 ms/div:** 38 400, div 16, cpu_permille 154, overwritten 0, Vp-p 10.0 V.
- **Both:** bad / overruns / slips / late 0. `LOG:COUN?`'s lost field read 2 in two of the 1 ms/div snapshots and 0 in the others (counts per record, not a ring overflow).
- **Exit (MENU at 10 ms/div):** the 4800 session ended with max_lat 18.6 us (lat_pc 0x0808bdd4). This is the known "Wave view exit: App::show runs before the stream stops" item.

**2.6:** DC V 1 V: 0.999951 V (10 READ?, -0.005 %). `DBG:HYSPI:SELF? 2000`: `16,2000,0,0,0,0,7.92,21.61,69.14,9.89,23,...`: all error counts 0.

Part 2's FAST files were deleted. `MMEM:CAT?` was back to 10 829 824 / 5 910 528.

### 3. AC+DC methods (docs/acdc-approaches.md §10.4)

`DBG:ACDC:METH?` was A at the start (the default) and is A at the end. `DM40_USBTMC=real /usr/bin/python3 tools/acdcmeth.py e6 e1 e7 e2 e5 -o acdcmeth.jsonl` ran 01:06-01:56. It ended by itself with method A, DC V and the calibrator at 1 V DC. The numbers are in `acdcmeth.jsonl`; the detailed results are in acdc-approaches.md §10.5. In short:

| exp | result |
|---|---|
| e6 entry / range | Entry at 3 V: first reading A 1.28 s, B 1.48 s, C 1.02 s (C settled 1.31 s). Readings/s (n_rd): A 3.7, B 9.3, C 9.0. 3 -> "30" -> 3 V: settle A 3.5 / 2.7 s, B 2.5 / 2.2 s, C 1.6 / 2.0 s. **The calibrator gives 23.99 V for 30 V** (DC V READ? 23.986), so every "30 V" point is ~24 V |
| e1 B's zero | **B fails** (rule: <= 2 digits). The zero is bimodal per entry, not a spread around one value: 6 V +12.6 or -30.5 mV; 60 V +17 or -30 mV; 600 V +-0.17..0.24 V; 1000 V +1.9 or -1.5 V. B's DC part after its zero is still off by up to 5.4 / 7.5 / 7.2 / 16.6 digits on 6 / 60 / 600 / 1000 V |
| e7 B's 6 V row | (i) R0 1: gain 0.99985, err -0.22 / +0.44 / -0.82 mV at 1 / 3 / 5 V. (ii) R0 2: gain 1.00003, err -0.09 / -0.11 / +0.03 mV, but its zero grows with the level (0.052 / 0.083 / 0.111 V: a ~1.5 % raw gain that the per-entry zero takes up). AC part on the square: 4.7758 / 4.7765 V |
| e2 square | 6 V AC part: A 4.77609, B 4.77610, **C 4.78217 (+0.127 % vs A)**. 60 V: A 4.77129, B 4.77074, **C 4.78204 (+0.225 %)**. DC part vs DC V (4.7765): 6 V A +0.24 mV, B -0.13, C +0.88; 60 V A -0.70, B -1.04, **C +13.6 mV (+0.28 %)** |
| e5 C over 2 x 20 min | 6 V at 3 V: C's DC part -0.34..+1.38 mV vs READ? (limit 0.05 % + 5 digits = 2.0 mV): **PASS**. 60 V at 24 V: **+20.9..+24.9 mV (+0.087..+0.104 %)**, limit 12 + 5 = 17 mV: **FAIL** in all 21 entries. It is a constant error, not a drift: per_code moved -132..+31 ppm, offset 0.0991..0.1038 V. 6 V per_code scattered -88..+321 ppm between entries, but the DC part stayed within the limit |

**Decision rule (C becomes the default when e2's AC part is within the AC V spec and e5's DC part within 0.05 % + 5 digits): not met.** The AC part is within the 0.5 % class (+0.13 / +0.23 % vs A). The DC part fails on 60 V: +0.09..0.10 % at 24 V in e5, and +0.28 % at 4.78 V in e2. The two points fit an offset of ~+11 mV plus a gain error of ~+0.05 % in C's 60 V self-cal. The default was not changed.

**By hand: AUTO coupling with C (3 taps on V), `p3.py`, `p3b.py`**
- **The square:** AUTO picks AC. AC 4.772 V, DC 4.777 V, 60 V range (`autoC-square.png`). Freq 40.00 Hz in the STAT. PASS.
- **5 V DC: FAIL.** C hunts between the 6 V and 60 V ranges every ~3 s. The AC part reads **2.11 V** on 6 V and **3.83 V** on 60 V with a DC input, and the readings drop to ~0.7/s. In AUTO, after the square, the page stayed on AC with "~4.056 V" (`autoC-5V.png`). Entered fresh at 5 V, AUTO shows the DC part (5.0098 V, +0.2 % on 6 V), but the hunting and the bogus AC part are the same.
- **Not AUTO-specific:** AC+DC (2 taps) with C at 5 V and 5.5 V hunts the same way (total 6.45 V for 5 V DC). At 4.0 V and 4.5 V it is fine: 6 V range, AC part 1.0-1.6 mV, ~9 readings/s.
- **Cause (not investigated):** the AC part (sqrt(RMS)) is wrong from somewhere between 4.5 and 5 V on the 6 V range. The up-range rule (|LPF| + 1.414 x AC > 90 % of AD2's span) then fires, and on 60 V the down-range rule brings it back. Each range visit re-runs the self-cal.
- **Method A as a control:** the same square -> 5 V sequence in AUTO shows AC 4.761 V, then DC 4.998 V after ~6 s, AC part 5-10 mV, 6 V range. PASS.

### State at the end

- **Device:** DMM page, DC V, method A, `LOG:DEST HOST`, `LOG:STAT 0`; LOG source / rate / chop back to AD1 / 4800 / ON. `LOG:DUR` is 20 (it was not read at the start; the previous session's chk4 had set 20). Scope timebase 2 ms/div as found. Generator `OUTP?` 0 (never switched on). Settings not touched (volume 0 %, backlight 10 %, Dim / Shutdown Never).
- **Files:** S012, S013, FAST013, FAST014 deleted (Part 1, then again for Part 2's FAST013 / FAST014 / the profiling session's FAST013). `MMEM:CAT?` used / free 10 829 824 / 5 910 528, the same as at the start.
- **Health:** `DBG:BOOT?` unchanged: crash count 0, HEALTHY, no reset. `DBG:HYSPI:SELF? 2000` all 0 at the end too.
- **Open: 1 V after Part 3.** DC V at the calibrator's 1 V read **0.99897-0.99949 V** after Part 3 (0.99919 V at the very end, -0.08 %). After Part 2 it read 0.999951 V. Over e5 the 3 V reference also moved 2.99796 -> 2.99749 V (-160 ppm). The calibrator had given ~24 V for 25 min before this. I did not find out whether the calibrator or the meter moved; there was no third reference.
- **Calibrator:** 1 V DC, enabled.

## 8087c7d: screenshot pre-erase, SCPI Range / Digits + sigrok, scope full record, the 9abe84f UI fixes (2026-10-23 02:20 - 03:10)

The owner had installed 8087c7d (it contains 36d6952, de01f2f, ad5ad8e, 9abe84f). `DBG:BOOT?` showed marker 0xE29D2342 / 0x000976F4 / 0x00013510, VALID, crash count 0, HEALTHY. No reset during the session (the boot block stayed the same; stack high-water 20 608 -> 22 732 of 45 800 B).
- **Checks:** TODO "Device: screenshot preparation" items 1, 2, 4, 5, 6; "Range and digits" device retest 1-8; "Scope full record" device retest 1-9; the 5e52761 UI fixes; the AC+DC + manual V range transient.
- **Wiring:** the calibrator on V/COM at 1 V DC the whole session. The A jack, the MCX probe and the generator output were not wired. **Calibrator control over `/dev/ttyACM1` was denied by the agent's permission classifier**, so nothing ran on the 40 Hz square this time.
- **Input:** DBG:TAP / DBG:KEY / DBG:SWIPE D,3; panel reads are DBG:LCD? reads decoded as `dm40dbg.py shot` does (`devlib.grab`).
- **Files:** scripts, PNGs, BMPs and JSON are in the session scratchpad `devchk-8087/` (not in git).
- **USB:** every libsigrok-dm40 run left the usbtmc kernel driver detached from interface 0 (`/dev/usbtmc2` gone). `reattach.py` (USBDEVFS_IOCTL + USBDEVFS_CONNECT on `/dev/bus/usb/BBB/DDD`, works as a plugdev user) brings it back; `sr.sh` runs it after each sigrok-cli call. The meter never reset or re-enumerated.

### Summary

| # | Check | Result | Evidence |
|---|---|---|---|
| 1.1 | Prep after boot; shot of DMM (DC V) and AC+DC | **PASS**: `DBG:FLASH?` state 2 / 76 at the start. frozen_ms **1357** (DC V), **1372 / 1380** (AC+DC), blocks_erased **0**, slices 532. BMP = the panel at the gesture, **0 px** (DC V and AC+DC). Touch works again by **1.54 s** after the gesture (external tap probe), job end 2.1 s. Prep back to 2 / 76 ~8 s after the job | `shot.py`, `freeze3.py`, `s2dmm-*`, `s3acdc-*` |
| 1.2 | A shot right after another | **PASS, erase path seen once**: S017 -> S018 at 4.6 s (prep state 0): frozen **2332 ms**, 0 erased (the clusters past the files were blank flash, compare only). A shot onto dirty clusters (S012 deleted, swipe 0.14 s later, prep had 17 ready): frozen **1954 ms**, **14 blocks erased** (~42 ms each; 76 would be ~4.8 s). Both Saved | `b2b.py` |
| 1.4 | Disk full | **PASS**: two AD2 38 400 disk sessions filled the disk (free 0): prep state 2 with **0 ready**; the swipe shows **"Disk full"** (red toast at 0.5-1.2 s), no BMP; `DBG:FLASH?` frozen 78 ms, 0 erased | `full.py`, `full2.png` |
| 1.5a | CSV commits | **PASS**: after each 60 s commit (at 60.4 / 120.4 s) prep 2 -> 0 -> 1 -> 2 within 2.3-2.8 s; after `LOG:CSV OFF` the same | `csvprep.py` |
| 1.5b | Fast-log session | **PASS**: prep state 0 for the whole session (LOG:DISK? state 3), 1 at 2.0 s after the end, 2 / 76 at 2.5 s | `fastprep.py` |
| 1.5c | "PC can edit" + a DM40*.BIN copied: prep pauses, the file installs | **not run** (installs firmware; owner) | - |
| 1.6 | `DBG:DIAG?` loop max while preparing | **PASS (+~1-2 ms)**: a prep with 76 erases (after deleting S012): state-1 windows max **13.9 ms**, median 11.9; idle (AC+DC page, same polling) max 12.0, median 11.2; the state-2 windows after it reached 14.2. The 65 ms windows right after `MMEM:DEL` are the delete's FAT write (state 0) | `diagdel.py`, `diagprep.py` |
| 1.3 | Battery-pull power cut during prep | **skipped** (owner) | - |
| 2.1 | `--show` ranges per function | **PASS**: V (DC, AC+DC, AUTO) Auto, 600mV .. 1000V; A (DC, AC+DC) Auto, 600uA .. 10A; ohm Auto, 60R .. 60MR; cap / freq Auto only; continuity / temperature none (an empty "range:" line); digits 5, 6 | `show.txt` |
| 2.2 | `-c range=` each, DC V / A / ohm | **PASS**: DC V 1 V in: 600mV inf (OL), 6V 999.1 mV, 60V 998 mV, 600V 1.00 V, 1000V 1.0 V, Auto 999.1 mV. A (open jack): every range answers with its CONF? (600 uA 1E-8 .. 10 A 1E-3), no "Leads in A?" alert. Ohm (the calibrator's output, ~50 MOhm): 60R .. 6MR inf, 60MR 50.16 MOhm, Auto 60MR | `rng-dcv.txt`, `rng-a-r.txt` |
| 2.3 | Range page highlight, MANU | **PASS**: after `range=60V --set` the status bar shows MANU and the MF range page highlights 60V | `sr1-60V.png`, `sr2-rangepage.png` |
| 2.4 | `range=Auto` keeps AUTO+ | **PASS**: AUTO+ in the bar and on the range page, `VOLT:RANG:AUTO?` 1 | `sr-auto2.png` |
| 2.5 | digits=5 / 6 | **PASS**: 5 -> chip "FAST 10/s", 4 decimals, CONF? 1E-04, samples 999.1 mV; 6 -> "MED 2.5/s", the small extra digit, CONF? 1E-05, samples 999.12 mV. (SLOW, the owner's setting, reads as digits 6; restored by hand at the end) | `dg.png` |
| 2.6 | SmuView Range / Digits combos | **skipped** (owner) | - |
| 2.7 | AC+DC / AUTO with a manual V range | **transient confirmed** (open item), see 5 | `trans*.py` |
| 2.8 | CONF? on 1000 V / 10 A | **PASS**: `"VOLT +1.000000E+03,+1.000000E-01"`, `"CURR +1.000000E+01,+1.000000E-03"` | - |
| 3.1 | DIG, WAV:POIN MAX + WORD, one read | **PASS**: DIG -> STOP in 0.21-0.22 s; `#553248` + 53 248 B (26 624 points) in **one `read()` call, 66 ms**; loop max1s right after 7.4 ms | `wav1.py` |
| 3.2 | WAV:DATA? while running / RUN during a long read | **PASS / not isolated**: running -> `#10` + `-100,"Settings conflict"`. RUN pressed during a read needs a hand on the key: a DBG:KEY over USB is a new command, which ends the stream | `wav1.py` |
| 3.3 | DIG x100 with the comparator on, 1 kHz loopback | **blocked** (no generator loopback; the output must stay off) | - |
| 3.4 | CHAN1:OFFS 0.5 / -0.5 then DIG | **PASS**: 1 V/div, open input: codes move +59 / -64 (0.5 V = 59.8 codes), the record's volts stay 0.918 / 0.901 / 0.891 / 0.889 V (the floating input drifts ~30 mV, < 4 codes) | `wav4.py` |
| 3.5 | ngscopeclient-dm40 on /dev/usbtmc2 | **FAIL, host side**: the driver hangs at its 6th query. `OUTP?` answers 1 byte, "0" (no newline); `SCPITMCTransport::ReadRawData` asked for len 1, got 1, so `bytes_fetched == bytes_requested` loops into another `read()`: 5 s ETIMEDOUT, `i += -1`, forever. Stream Browser "STOPPED", generator fields 0, no trace; Run / Auto clicks do nothing | `ngs4-bc.png`, `ngs5.strace` |
| 3.6 | Bluetooth | **PASS (dm40bt_scpi.py)**: after DIG, `WAV:POIN MAX` -> `WAV:DATA?` empty block + `-100,"Too much data"`; `WAV:POIN 1902` -> a 1902-byte record. The driver over rfcomm: not run (no rfcomm bind; needs sudo) | - |
| 3.7 | Kill the host mid-read | **PASS (partly)**: a process read 4096 B of the record and exited; the next process's queries answered at once (no USBTMC_IOCTL_CLEAR needed) and RUN by DBG:KEY took effect at 0.01 s. The 2 s stall timeout without host traffic was not isolated (same reason as 3.2) | `kill1.py`, `after2.py` |
| 3.8 | Wave view record (DMM source) | **PASS on 1 V DC only**: DC V Wave view, STOP: SCReen 400 and MAX 26 624 points, all code 554 = **1.0007 V**, the screen Vrms 1.00 V. Not run on an AC signal (calibrator control denied) | `wav8.py`, `wv0.png` |
| 3.9 | 50 MS/s rates on the generator's 1 MHz square | **blocked** (loopback) | - |
| 4a | Combined-view strip units | **PASS**: "0.0000mW" (SC_DIGITS, the W whole), "0.0000mWh" (F20 on the baseline). MHz: not reachable on the device (no source above the calibrator's range, generator off) | `u2z.png`, `u4-comb-wh.png` |
| 4b | Help 2/7, 5/7 | **PASS**: "< Back" -> title ink gap 14 px on every page (was one space on 5/7); topic -> value gap >= 21 px on all seven pages (2/7: 36 px) | `hp1..hp7.png` |
| 4c | Power page after a coupling change | **PASS**, cosmetic note: DC -> AC, AC -> AC+DC, AC+DC -> DC each show the "--" page in the new layout at once (first strip read 0.05-0.32 s after the tap), the new reading at ~1-2 s. AC -> AC+DC: bits of the AC layout's right column (the "VAh" / "AVG" rows) stay next to the new one for ~2.5 s; the pending page's chart labels have no unit | `pc1-sheet.png`, `pc23-sheet.png`, `pc4-strips.png`, `pc2-z.png` |
| 4d | Chart axis after a function change | **PASS**: Hz "0.050Hz", diode (OL) "0.0050V", cap (OL) "5.0uF", ohm "50.206MOhm", temperature (OL) "5.0°C" | `c-sheet.png` |
| 5 | AC+DC + manual V range transient | **confirmed** (open): a wrong AC part for ~2-4 s after each range change, then a tail; details in 5 | `trans.py`, `trans2.py` |
| new | DC V: the present range set again | **FAIL (new, pre-existing)**: 1-2 readings at 25-51 % of the input, see "New findings" | `dcvsw2.py`, `mfsw2.py` |

### 1. Screenshot preparation (36d6952)

`DBG:FLASH?` fields 4-10 = frozen_ms, total_ms, slices_frozen, blocks_erased, prep state, clusters ready, prep erases since boot.

- **1.1:** `shot.py` reads the panel, sends `DBG:SWIPE D,3` at once, polls `LOG:CSV:FREE?` every 0.25 s for the busy window, then reads `DBG:FLASH?` and pulls the BMP with `MMEM:DATA?`. The panel read just before the gesture is the reference: the 300 ms draw hold of DBG:LCD? delays the shot start (busy from 1.25 s when a read ran during 0-0.7 s), so the frozen page is the page at the gesture. Both BMPs are 307 266 B, BI_BITFIELDS 565, 0 px against that read. A first run with a 0.6 s pause before the gesture differed in 652 px (the clock, chart and statistics moved): the reference was just stale.
- **frozen_ms:** 1357 / 1357 (DC V, two shots), 1372 / 1380 / 1370 / 1383 / 1362 (AC+DC); total_ms 1950-1981; slices 532. The emulator gave 1403-1409 ms. The external probe (`freeze3.py`, taps on "A" every 0.1 s from 0.9 s) saw the tap at 0.61 s ignored and the tap at 1.54 s take effect; job end 2.09 s, toast up by 2.44 s. The old freeze was 5.4 s (touch) / 6.1 s (toast).
- **After a shot:** prep goes 0 (2 s quiet) -> 1 -> 2 / 76; the erase count rose by 76 per shot while the shots took the earlier-prepared clusters.
- **1.2:** `b2b.py`: the second swipe is accepted only once the first shot's toast is gone (4.6 s). By then prep was still state 0, so the second shot had no prepared clusters, but its clusters were blank flash past the end of the files: compare only, frozen 2332 ms, 0 erased. To see the erase path, S012 was deleted (its 75 dirty clusters become the first free ones) and the swipe sent 0.14 s later; prep had already re-checked 17 clusters (the delete's quiet window had passed during the toast): frozen 1954 ms, 14 blocks erased. Each unprepared dirty block costs ~42 ms, so a fully unprepared shot would be ~4.8 s (TODO estimate 3-5 s).
- **1.4:** free was 4 034 560 B. FAST014 (AD2 38 400, `LOG:DUR 60`) used 2.46 MB (kind 3 is ~1.07 B per sample, not 3 B, so 60 s did not fill it); FAST015 (`LOG:DUR 120`) ran into Full (LOG:DISK? state 6). Then `MMEM:CAT?` free 0, `DBG:FLASH?` state 2 / **0** ready. The swipe: "Disk full" (red), no new file, the shot's stats 78 / 78 ms, 6 slices.
- **1.5:** `csvprep.py` polled `DBG:FLASH?` every 0.2 s for 135 s of `LOG:CSV ON` (DC V): each commit -> state 0 for ~2 s -> 1 for 0.2-0.8 s -> 2 / 76; no erases (the commit took a prepared cluster). `fastprep.py` (`LOG:DEST DISK`, `LOG:DUR 20`, AD1): prep stayed 0 during the session and resumed 2 s after it.
- **1.6:** `diagdel.py`: 6 s of idle polling, `MMEM:DEL "S012.BMP"`, 20 s of polling (`DBG:FLASH?` + `DBG:DIAG?` loop max1s every 0.2 s). The prep erased 76 blocks (394 -> 470) in ~5.5 s. Loop max per state: idle 12.0 ms (median 11.2), state 0 (the delete) 65.5 ms saturated then 18.9, **state 1 13.9 ms (median 11.9)**, state 2 afterwards 14.2 (median 12.0). An earlier run (`diagprep.py`, right after a shot, no erases) had one 26.4 ms window in state 1 at 2.5 s after the job end, which overlaps the post-shot repaint / volume refresh. The baseline is the AC+DC page, which itself runs at ~11 ms.
- **Not run:** the battery pull (owner), "PC can edit" + a DM40*.BIN (it installs firmware). A host read of the USB volume during prep (to see the 0.5 s pause) failed on the host side (`dd iflag=direct` on vfat: "Invalid input") and was dropped.

### 2. SCPI Range / Digits + sigrok (de01f2f)

sigrok-cli from `libsigrok-dm40/install`, always `-d scpi-dmm:conn=usbtmc/3.55`, no scan. After every call the usbtmc driver had to be re-attached (above).
- **Same-range set:** see "New findings". It also shows in sigrok's own runs: after `range=60V --set` on 60 V the chart's MIN was 0.510 V (`sr1-60V.png`).
- **Rate:** the owner's SLOW 2.5/s reads as digits 6 in `--show`; digits 5 / 6 switch FAST / MED. SLOW was restored with the rate chip on the DMM page. The chip cycles FAST -> SLOW -> MED (`rate_cycle`, the enum order); a tap on the value cycles FAST -> MED -> SLOW (`resolution_cycle`). Not wrong, but the two orders differ.
- **A ranges with the A jack open:** readings 0-1 uA, `SYST:ALER?` NONE on every range. After `range=Auto` the 600 uA range read 2.30 uA for a moment.

### 3. Scope full record (ad5ad8e)

- **3.1 / 3.4:** `wav1.py`, `wav4.py`. The open MCX input floats at ~0.9 V; at 20 mV/div the record was all code 1023 (clipped), so 3.4 ran at 1 V/div. `WAV:PRE?` (MAX, WORD): `1,2,26624,1,8E-07,-0.0113,0,1.6706E-04,-0.0476,0` at 2 ms/div.
- **3.5 in detail:** `strace -f` on ngscopeclient: `*IDN?`, `WAV:SOUR CHAN1`, `WAV:FORM WORD`, `WAV:POIN MAX`, `TRIG:SWE?`, `TRIG:LEV?`, `TRIG:SLOP?`, `CHAN1:OFFS?`, `CHAN1:SCAL?`, `CHAN1:OFFS?`, `ACQ:SRAT?`, `CHAN1:COUP?` (two threads interleave on the one transport), `OUTP?` -> "0", then only `read()` calls that time out every 5.1 s with the buffer pointer moving back one byte each time (`i += bytes_fetched` with -1). Every reply before it was 2+ bytes; fw-rs ends USBTMC replies without a newline (the Bluetooth path adds one), and `ReadReply` reads one byte at a time until '\n'. The emulator tests used `lan`, never this transport. The ngscopeclient `-L` log stays empty when it is killed. Options (owner): (a) fw-rs ends USBTMC replies with '\n' (IEEE 488.2 NL^END; check the host tools that compare raw bytes), (b) fix the loop in the scopehal fork (`bytes_fetched == bytes_requested` with `bytes_requested = len` while `read()` is given the whole staging buffer), (c) both. The meter itself stayed fine: after the hung process was killed, queries answered at once.
- **After 3.5:** the launcher's `TRIG:LEV?` was -1.90 V: the trigger level follows the CH1 offset/scale changes of 3.4 (it was 0 at the start). Restored to 0 with the original offset.
- **3.7:** the killed process had read 4096 of 53 255 bytes. The previous session's killed `LOG:DATA?` needed `USBTMC_IOCTL_CLEAR`; this one did not.

### 4. UI fixes (9abe84f)

- **4a:** power function, A jack open: the strip "0.0000mW" with the full W (`u2z.png`, 3x), and after a tap on the value (Wh) "0.0000mWh" in the smaller font on the digits' baseline. The DMM page shows "mWh" in the same smaller font.
- **4c:** `powc.py` (full panel 0.3-1.0 s after the W tap, then at 2.5 and 8 s) and `powc2.py` (two strips every 0.33 s from 0.05 s). The pending page: "--V", "--uA", "- -" as the value, E / PF / S / Q / f "--", the AC glyph / AC+DC glyph on the W button at once. Leftovers on AC -> AC+DC (`pc2-z.png`, 4x): the top of an "AVG" row above "Vb" and the bottom of a row under the E box; gone by 3-8 s.
- **4d:** `c-sheet.png`; the cap reads OL and the ohm 50.146 MOhm because the calibrator's 1 V output is on V/COM.

### 5. AC+DC + manual V range (re-check)

`trans2.py`: CONF:VOLT:ACDC (method A), 1 V DC, then `VOLT:ACDC:RANG 60`, `6`, `:AUTO ON`, polling `DBG:ACDC:STAT?` (range, ac, dc) and `READ?` back to back. (The first run used `VOLT:RANG 60` in AC+DC: "Settings conflict", as specified: the header must name the current function.)
- **6 -> 60 V:** the first AC part after the change was **0.353 V**, then 0.262 / 0.193 / 0.142 V until ~4 s, then 18 -> 1 mV over ~10 s. READ? **1.0593 V (+6 %)** at 2.2 s, 1.0087-1.0091 to 3.3 s, then within 0.1 %.
- **60 -> 6 V:** AC part **0.146 V** at 1.5 s, 0.080 / 0.058 / 0.042 V to 3.3 s, then 6-10 mV, ~2-6 mV after ~5 s. READ? 1.0098 at 1.3 s, normal from ~2.5 s.
- **MF page path** (`DBG:ACDC:RANG 2` / `1`, `trans.py`): the same, AC part 1.058 V at 1.4 s and 0.577 V to 3 s after 6 -> 60 V; 51 / 27 mV after 60 -> 6 V.
- **AUTO coupling** (`CONF:VOLT:AUTO`, `VOLT:AUTO:RANG 60` / `6`): the same AC-part transient in `DBG:ACDC:STAT?`, but AUTO stays DC (READ? = the DC part, `VOLT:COUP?` AUTO,DC at the end).
- **Verdict:** the emulator's "2-5 s" transient is real on the device: a grossly wrong AC part for ~2-4 s, and on the 60 V range a tail of > 5 digits for ~10 s. In AC+DC it moves the total by up to +6 %.

### New findings

1. **DC V: setting the present range again gives 1-2 bogus readings** (`dcvsw2.py`, `mfsw2.py`; 1 V DC in). `VOLT:RANG 60` while on 60 V: READ? **0.257 / 0.257 V** at 0.46 / 0.66 s, the next time **0.509 V**; `VOLT:RANG 6` while on 6 V (manual or AUTO+): **0.375 V**, **0.403 V**. The MF range page tap on the highlighted item does the same (chart / statistics MIN 0.2506 V and 0.4033 V). It happens on about every other repeat. A real range change (6 <-> 60 <-> 600 V, AUTO ON) gave none in 8 tries; one `VOLT:RANG:AUTO ON` from 600 V gave 0.781 V in the first run. Pointer: `Dmm::set` rewrites the config (`write_config`) and sets a 450 ms deadline, but `changed_mode` is false, so nothing is reset; the first conversion after the deadline looks like a partial integration (25-51 % of the input). SmuView / sigrok users re-applying a range will hit it. Not fixed (pre-existing; owner's call).
2. **libsigrok-dm40 leaves `/dev/usbtmcN` detached** after each sigrok-cli run (host side). Recipe: USBDEVFS_CONNECT on interface 0 (`devchk-8087/reattach.py`).
3. **ngscopeclient over USBTMC hangs on a 1-byte reply** (3.5).
4. **DC V reads -0.08..-0.13 % at the calibrator's 1 V all session** (0.9987-0.9993 V on the page, sigrok 998.9-999.6 mV). The calibrator had sat at 1 V for hours, so "warm after 24 V" (the 7409b92 open item) does not explain it.
5. **Cosmetic:** the rate chip and the value tap cycle the rates in different orders; the power page leftovers on AC -> AC+DC (4c).

### State at the end

- **Device:** DMM page, DC V, AUTO+, rate SLOW (restored after the digits test; `--show` had found it there). Method A, `LOG:DEST HOST`, `LOG:STAT 0`, LOG source / rate / chop AD1 / 4800 / ON, `LOG:DUR` 60 (as found; it was 60 at the start, not the 20 of the 7409b92 section). `SYST:IDN:EMUL` OFF. Scope 2 ms/div, 20 mV/div, CH1 offset -38.0 mV, trigger level 0, sweep AUTO (as found); `WAV:FORM` BYTE, `WAV:POIN` SCR. Generator `OUTP?` 0 (never switched on). Settings not touched (volume 0 %, backlight 10 %, Dim / Shutdown Never: seen on Settings 1/3).
- **Files:** S012-S019 (S012 twice), LOG005, FAST013-FAST015 deleted. `MMEM:CAT?` used / free 10 829 824 / 5 910 528, the same as at the start; prep 2 / 76.
- **Health:** `DBG:BOOT?` unchanged: crash count 0, HEALTHY, no reset.
- **Calibrator:** 1 V DC, enabled (never changed).

## DC V at the calibrator's 1 V: meter or calibrator? (8087c7d, 2026-10-23 03:15-03:42)

This checks the open items "1 V after Part 3" (7409b92) and New finding 4 (8087c7d). DC V had read -0.08..-0.13 % at the calibrator's 1 V since Part 3 of the 7409b92 session; after Part 2 it read 0.999951 V.
- **Build:** 8087c7d, as installed. No firmware was installed and no code was changed.
- **Wiring:** the calibrator (`/dev/ttyACM1`, `mr9270.py`; access worked this time) on V/COM. The A jack, the MCX probe and the generator output were not wired.
- **Files:** in the session scratchpad `dcv/now/` (not in git): `lv-pre.jsonl`, `lv-sweep1.jsonl`, `lv-post.jsonl` (levels x rates), `rng-r1.jsonl` (6 V vs 60 V), `reset.jsonl` (re-set test, 0 V, 5 min hold), `fast1.csv` (raw AD1), `CALIB.BIN`, `s0.png`, `s-end.png`.

### Summary

**Verdict: the calibrator, not the meter, and not a firmware regression.**
- The error depends on the level, in a way that neither a gain nor an offset gives (table 2).
- The 6 V and 60 V ranges read the same input within 0.2-0.4 mV.
- The same per-level pattern was there on 2026-10-22 20:24 with a 4ff95f7-era build, before 7409b92.
- On top of that, the 1 V reading wanders by +-0.4 mV over minutes while nothing is written to the calibrator.
- No fw-rs state changes the DC V math.

| # | Check | Result |
|---|---|---|
| 1 | Persistent state | **None found.** `CONF?` "VOLT +6,+1E-05", rate SLOW, `CALC:SMO?` 0, `CALC:AHOL?` 0, REL 0.0000 on the page (`s0.png`), `DBG:ACDC:METH?` A, `DBG:ACDC:ZERO?` 0,0 / all 0, `DBG:ACDC:R0?` 1, `POW:VOLT:GAIN?` factory. The DC V path (`poll_dcv`) uses only the raw AD1 code and the factory record. It does not use a REL, the AC+DC method, the E7 self-cal (`DC_CAL` is used only by AD2's `Ad2Scale` and AC+DC methods B / C in acdcm.rs) or the power-meter V gain. |
| 2 | Calibration record | **Unchanged.** `CALIB.BIN` from the meter is byte-identical to the 2026-10-13 dump (`private/memrw-kit-test/quick.img/calibration_080C0000_32K.bin`). fw-rs programs internal flash only for the boot marker (`boot.rs`). 6 V range: gain 1.012064, offset 0.12534 mV. 60 V range: gain 1.007588, offset 0.25962 mV. |
| 3 | Raw AD1 code -> volts | **Matches READ?.** `dm40scpi.py fast --source ad1` (OSR 3, chop on, 116 samples at 1 V): mean code 596 203 (sd 85). `code x 10 x (1.25/0.9) / 8388606 x 1.012064 - 0.000125` = 0.998908 V, + 0.05 mV (the r0 term in `poll_dcv`) = 0.998958 V. The READ?s right after gave 0.999033-0.999044 V. The 0.08 mV gap is the OSR 3 vs SLOW difference plus drift. |
| 4 | Rate | **No effect.** 1 V: SLOW / MED / FAST 0.99918 / 0.99913 / 0.99906 V (`lv-pre`). At every level the three rates agree within 0.1-0.25 mV (table 2). |
| 5 | Levels 0..20 V | **Neither gain nor offset** (table 2). |
| 6 | 6 V vs 60 V range, same input | **Agree.** 1 V: -0.98 / -1.24 / -0.88 mV (6 / 60 / 6 V). 3 V: -2.31 / -2.50 / -2.12 mV. 5 V: -2.12 / -2.48 / -2.24 mV (`rng-r1`). The two ranges use different divider taps and calibration entries, and AD1 sees a 10x smaller signal on 60 V. The common error is therefore at the input. The 60 V range reads 0.2-0.36 mV below the 6 V range, which is a separate small offset within spec. |
| 7 | `dm40dbg.py reset` | **Nothing reset-dependent.** After the reboot (uptime 13 s, `DBG:BOOT?` NORMAL, crash count 0) the errors were 1 V -0.29..-0.48 mV, 2 V -0.9, 3 V -2.1, 5 V -1.7 mV (`lv-post`). That is +0.3..0.5 mV against the pre-reboot sweep, the same shift at every level (not a gain), and within the wander in 9. |
| 8 | Same 1 V written again (5 x, 50 s apart) | **No step.** Before / after the re-write: -0.01 / -0.04 / -0.02 / +0.00 / +0.03 mV. Writing the setpoint again does not move the output. |
| 9 | 1 V held 5 min, no calibrator writes (30 windows of 25 READ?) | **Wanders.** Window means 0.998969-0.999747 V (span 0.78 mV, mean 0.999409); sd within a window 0.09 mV. |
| 10 | 0 V | **Meter zero is good.** -0.037 mV (sd 2.6 uV) at 03:35, -0.12..-0.13 mV at 03:17. |

**Table 2: levels** (`lv-sweep1`, before the reboot, SLOW; READ? mean of 15)

| set | range | read | error | error % |
|---|---|---|---|---|
| 0 V | 6 V | -0.000118 | -0.12 mV | - |
| 1 V | 6 V | 0.999100 | -0.90 mV | -0.090 |
| 2 V | 6 V | 1.998599 | -1.40 mV | -0.070 |
| 3 V | 6 V | 2.997531 | -2.47 mV | -0.082 |
| 5 V | 6 V | 4.998050 | -1.95 mV | -0.039 |
| 10 V | 60 V | 9.997050 | -2.95 mV | -0.030 |
| 20 V | 60 V | 19.998745 | -1.26 mV | -0.006 |
| 1 V again | 6 V | 0.999479 | -0.52 mV | -0.052 |

- **Not a meter gain:** the % error is not constant (-0.09 at 1 V, -0.006 at 20 V).
- **Not a meter offset:** the zero is -0.12 mV, and the mV error goes up and down with the level (3 V worse than 5 V).
- **Consistent with the source:** a 24-bit ΣΔ AD1 is not this non-linear (0.05 % of the reading), and the 60 V path shows the same pattern. What is left is the calibrator's own DAC error at each setpoint.

**The same pattern before 7409b92.** On 2026-10-22 ~20:24 a 4ff95f7-era build (device test 6.5, `DBG:AD2:DCV?`, the READ? reference column of `ad2dcv.py`) read the same calibrator as follows:

| set | read | error % |
|---|---|---|
| 0 V | -0.06 mV | - |
| 0.5 V | 0.49924 | -0.15 % |
| 1 V | 0.99914 | -0.086 % |
| 2 V | 1.99879 | -0.06 % |
| 5 V | 4.99726 | -0.055 % |
| 10 V | 9.99695 | -0.03 % |
| 20 V | 19.99788 | -0.011 % |

That is the same shape within ~0.02 %, 31 h and many builds earlier.

**History.** At 1 V the meter has read from 0.9987 to 1.0006 V over two days:
- 1.0005-1.0006 (9dd1222);
- 1.0004 (5f14d38);
- 0.9992-0.9998 (5f14d38 .. 973df07);
- 0.99914 (the run above);
- 0.99906 / 0.99971 (a0985e4);
- 0.999951 (7409b92 Part 2);
- 0.9987-0.9993 (8087c7d);
- 0.9990-0.9997 (today).

The readings go up and down; they do not step with the builds. 9dd1222 / 5f14d38 have no multi-level data, so the only claim is that the 1 V point moves by up to ~0.1 % on its own over hours and minutes. The 0.999951 after Part 2 was a high point of that wander, not the baseline.

**Limits.**
- There is still no third reference. "Calibrator" rests on the exclusions above: no firmware state, the same code -> volts, both ranges agree, a stable zero, and a level-dependent but repeatable signature.
- A meter reference that drifts ~400 ppm in minutes is not ruled out. It would be unusual for the HY3131, and it would scale with the level, which the 0 V / 20 V points do not show.
- The settled answer needs a 6.5-digit DMM or a voltage reference (e.g. a 2.5 / 5 V reference IC) on V/COM.

**The open items are closed:**
- 7409b92 "1 V after Part 3": not caused by the AC+DC session.
- 8087c7d New finding 4: not a regression.

### State at the end

- **Device:** **rebooted once on purpose** (`dm40dbg.py reset`, 03:25). `DBG:BOOT?` NORMAL / HEALTHY, crash count 0, stack high-water reset to 20 608 / 45 800 B. DMM page, DC V, AUTO+ (`VOLT:RANG:AUTO?` 1), rate SLOW, `CONF?` "VOLT +6,+1E-05", method A. `LOG:DEST HOST`, `LOG:DUR 60`, `LOG:OSR 3`, `LOG:CHOP 1`, source AD1, 4800 (as found). Generator `OUTP?` 0. Settings not touched (volume 0 %, backlight 10 %, Dim / Shutdown Never).
- **Files:** the `fast` run's `LOGS/FAST013.DLG` was deleted. `MMEM:CAT?` 10 829 824 / 5 910 528, as at the start.
- **Calibrator:** 1 V DC, enabled (func 1 mode 0 value 1000).

## 2563fe1: AC+DC method C retest (acdc-approaches.md §10.7) (2026-10-23 04:57 - 05:22)

The owner had installed 2563fe1 (install OK). `DBG:BOOT?` showed NORMAL, crash count 0, VALID at the start and at the end. There was no reset during the session.
- **Wiring:** the calibrator (`/dev/ttyACM1`) on V/COM: DC 0..24 V (its "30 V" is 23.99 V) and the 40 Hz square 0..9.55 V. The A jack, the MCX probe and the generator output were not wired.
- **Runs:** `DM40_USBTMC=real /usr/bin/python3 tools/acdcmeth.py e8 e9 e10 e2 e6 -o acdcfix.jsonl` (04:57-05:10), then `e5 --minutes 3` (05:10-05:13). After that three small scripts: `z60.py` (C against READ? at the same moment, 60 V), `hand.py` (AUTO by touch with screenshots) and `par.py` (AUTO on the square, A vs C).
- **Files:** in the session scratchpad `rt/` (not in git): `acdcfix.jsonl`, `run1.log`, `run2.log`, `z60.jsonl`, `hand.log`, `hand-*.png`, `par-A-square.png`, `pre.png`, `end.png`.
- **The calibrator's wander** (section "DC V at the calibrator's 1 V") is up to ~0.8 mV. Every DC check below therefore compares C with the meter's own DC V reading (READ?), not with the set value. In e8 each level's READ? was taken ~0.5-1 min before C's reading (all refs of a range first, then C stepped down, 6 s per level). `z60.py` repeats the two deciding points with READ? taken right after C's reading, and gives the same answer.

### Summary

**Decision rule (§10.4): not met.** Everything the §10.6 fix targeted now passes on the device. The rule still fails at the low end of the 60 V range: C's 60 V gain, taken from the internal zero, is +0.030 % off, so 0 V reads -7 to -8 mV against READ? (limit 5 mV). The default stays A. The owner decides.

| # | Check | Pass criterion | Result |
|---|---|---|---|
| e8 | C's DC part vs READ?, entered at the top level and stepped down | 0.05 % + 5 digits | 6 V: **PASS** at 5 / 3 / 1 / 0 V (+0.07 / +0.10 / +0.44 / +0.33 mV). 60 V: 24 / 12 / 5 V PASS (-0.47 / -4.34 / -6.12 mV), **1 V FAIL (-7.23 mV, limit 5.5), 0 V FAIL (-8.01 mV, limit 5.0)**. e8kept (re-entered at 5 V on 60 V, the gain kept from 24 V, gspan 49 422): -1.24 mV, PASS |
| z60 | as e8 at 0 / 1 V on 60 V, READ? right after C, 2 repeats | same | 0 V **-6.89 / -7.06 mV**, 1 V **-6.39 / -6.58 mV**: **FAIL**. Confirms e8: this is not the calibrator |
| e9 | 4.0 / 4.5 / 5.0 / 5.5 V in AC+DC, AUTO range | one range, >= 6 readings/s, AC part < 30 mV, DC within the rule | **PASS** at all 4: the 6 V range only, 9.18-9.27 readings/s, AC part max 0.27-0.31 mV, DC -0.15..+0.42 mV. `acsrc` all 0: the RMS was usable in every sample (119-120 of 120); no garbage RMS at 5 / 5.5 V |
| e10 | AUTO on the square, then 5.0 / 5.5 / 4.5 V DC | AUTO,DC within the rule, one range | **PASS**: AUTO,DC 4.9981 / 5.4978 / 4.4985 V, -0.21 / -0.28 / -0.28 mV against READ?, the 6 V range only, ~9.0-9.3 readings/s. On the square: AUTO,**DC** 4.766 V (see 3) |
| e2 | the square, C's AC part vs A's | within 0.05 % | **PASS**: 6 V 4.76580 vs 4.76524 V (**+0.012 %**, was +0.127 %); 60 V 4.76049 vs 4.76067 V (**-0.004 %**, was +0.225 %). C's DC part vs DC V: +0.32 / -0.33 mV (was +0.88 / +13.6 mV) |
| e6 | entry / range-change timing | C first reading ~2.5 s, ~9 readings/s | **PASS**: C entry 1.71 s, 9.3 readings/s; 3 -> 24 V settled in 0.59 s, back to 3 V in 1.01 s (A: 1.27 s, 3.7 readings/s, 3.41 / 2.47 s; B: 1.49 s, 9.3 readings/s, 2.56 / 2.24 s) |
| e5 | C re-entered every 60 s, 3 min per level | 0.05 % + 5 digits | **PASS**. 24 V on 60 V: +0.53 / -1.29 / +0.53 mV (limit 17 mV; on 7409b92 it was +20.9..+24.9 mV). 3 V on 6 V: -0.04 / +0.37 / -0.09 mV (limit 2.0 mV). per_code is constant after the first entry (gain kept: gspan 49 422 / 103 946) |
| hand | AUTO (3 taps on V) with C: the square, then 5 V DC | the 7409b92 failure case | **PASS**: 5 V reads AUTO DC **4.9981 V**, the 6 V range, AC part 0.2 mV, 9.4 readings/s, no hunting (`hand-C0-5V.png`). On 7409b92 this was "~4.056 V" AC with hunting |
| R29 | restarting AD2's peak hold does not restart the RMS / LPF | e9 rate and acsrc | **Holds**: 9.2 readings/s (an RMS / LPF result every ~107 ms) and `acsrc` 0 in all 479 e9 samples and in 80 of 80 per e10 level |

### 1. C's 60 V gain (e8, z60)

The 60 V errors of e8 lie on a straight line: err = -7.77 mV + 0.302 mV/V x level. The residuals are -0.24..+0.23 mV, the size of the calibrator's wander. So:
- C's 60 V per_code (4.85309e-4, from the internal zero, span 49 422 codes) is **0.030 % too high**. The data imply ~4.8516e-4. The table value (4.853726e-4) is 0.043 % high, so falling back to the table would be worse.
- Equivalently, the internal zero (OP1+ on AGND) sits **~14-16 codes (~7-8 mV; z60 ~14, e8 ~16)** off the code that a real 0 V input gives on 60 V. The sign follows from the fit (C reads low below the anchor, so per_code is high): the AGND zero's LPF code would be *above* that of SENSE at 0 V. This is an inference from the fit, to check directly before a zero-correction fix. The gain anchored at 24 V inherits that.
- At the anchor the error is ~0 by construction (e5, e8 24 V, e8kept).
- The 6 V range shows nothing similar: across 0-5 V the error is within +0.07..+0.44 mV, a slope of about -0.007 %.

### 2. The >= 5 V fault and the R29 assumption (e9, e10, hand)

The 7409b92 failure did not recur. The 6 V range at 5.0 / 5.5 V gave usable RMS results throughout in steady state (`acsrc` 0), so the RMS-fault up-range was not needed. The peak bound did stand in once, in the transient after the square -> 5 V step (`hand.py`, at 2 s: `acsrc` 1, AC part 12.7 mV, an upper limit, as designed); the RMS was usable again by 5 s (0.27 mV). The garbage RMS of 7409b92 may have depended on its per-visit self-cal / row-write sequence, which the fix removed. This run cannot tell.

The peak-hold restart by R29 does not disturb the RMS / LPF block. The rate stayed at ~9.2 readings/s, and the AC part on a pure DC input stayed at 0.2-0.3 mV.

### 3. AUTO on the square: C picks DC, A picks AC (`hand.py`, `par.py`)

`CONF:VOLT:AUTO` on the 40 Hz square, two entries per method, VOLT:COUP? every 0.5 s for 15 s:
- **A:** DC for ~3.5 s, then AC to the end (both entries).
- **C:** DC throughout (both entries; also in `hand.py`'s AUTO-by-touch run, `hand-C0-square.png`).

The square has AC 4.760 V and DC 4.764 V, practically equal. The AUTO rule (dmm.rs `auto_shows_ac` / `auto_eval`) decides on the first evaluation (AC if AC > |DC|). After that, switching to AC needs AC > 1.5 x |DC|, which this square never gives. So the first evaluation decides:
- A's first AC part overshoots (4.91 V at 2.8 s) and A lands on AC.
- C takes its first AC part as it is (4.760 < 4.764) and lands on DC.
- On 7409b92, C's AC part was 0.13 % high (4.782 > 4.777) and C picked AC.

This is not a C fault: either part is a fair answer for AC ≈ DC. It is, however, a behaviour difference from A on exactly this test signal.

### 4. Other observations

- **e2's 6 V total:** FETCh? returned the overload value (NaN in the log) on the manual 6 V range for A, B and C. hypot(4.765, 4.764) = 6.74 V is over the 6 V range. On 60 V the total was 6.734-6.735 V.
- **In AC+DC (not AUTO)** after the square -> 5 V step, the first C reading on the 6 V range had AC part 1.88 V (total 5.34 V at 1 s). The next one, by 4 s, had 0.35 mV. A in the same step: AC part 0.164 V at 3 s, 5-8 mV after 6 s. This is the RMS block's HPF transient after a large step, gone in < 4 s.
- The calibrator's 24 V read 23.9845-23.9864 V (READ?) over the session.

### State at the end

- **Device:** DMM page, DC V, AUTO+ (`VOLT:RANG:AUTO?` 1), rate SLOW, `CONF?` "VOLT +6,+1E-05", `VOLT:COUP?` DC, **method A** (`DBG:ACDC:METH?` A), as found (`pre.png`, `end.png`). `SYST:ERR?` empty. Generator `OUTP?` 0 (never switched on). Settings not touched (volume 0, backlight 10 %, Dim / Shutdown Never).
- **Files:** none created on the meter (`MMEM:CAT?` identical to the start: 10 829 824 / 5 910 528). Screenshots were `DBG:LCD?` reads.
- **Health:** `DBG:BOOT?` NORMAL, crash count 0, VALID; no reset.
- **Calibrator:** 1 V DC, enabled.

## 4c4b489: the 76.8 kS/s disk retest (TODO "76800 to the disk loses 54 %" 1-7) (2026-10-23 05:30 - 06:00)

The owner had installed 4c4b489 (install OK, crash count 0). It carries the emulator fix of the 7409b92 disk failure: the Run budget counts the pass's own CPU, a faster coder, and the writer trial (`Gate`, LOG:DISK? fields 11..13). `DBG:BOOT?` at the end: NORMAL, crash count 0, HEALTHY, marker VALID. Three deliberate resets (`DBG:RESET`, item 5b: one attempt lost its reconnect and was repeated), no crash.
- **Wiring:** the calibrator (`/dev/ttyACM1`) on V/COM: the 40 Hz square (`set_output('frequency', 40.0)`, 0 / 9.5 V, READ? in AC V 4.764 V), the 100 Hz square, 1 V DC. The A jack was not wired.
- **Files:** scripts, JSON, DLG / CSV and PNGs in the session scratchpad `devchk-4c4b/` (not in git). `disk.py` runs one disk session (LOG:DISK? every 0.5 s, LOG:PRE? / LOG:SRAT? at state 3 and 3 s later, DBG:ISR? / LOG:COUN? at the end, MMEM:DATA? of the file, `dlog.py`, `lost.py` = the sum of lost_before). `lat.py` runs a session with no polling or with a 20 Hz LOG:DISK? poll. `stop.py`, `early.py`, `recfast.py` are items 4, 5b and 7. `p2.py` / `p5.py` are the 7409b92 scripts.
- **Symbols:** the lat_pc values were looked up in the ELF of the 4c4b489 build (`fl768/`, same timestamp as `dm40-fw-4c4b489.bin`).

### Summary

**The 7409b92 failure is fixed on the device.** 76 800 S/s to the disk keeps every sample: lost_before 0 in every file, the trial kept the rung in 8 of 9 sessions, and the ring's high water stayed at 1017-1170 of 2048 (the ~1900 limit was never reached). **New: a region C fetch on every SCPI command** (de01f2f's `range_cmd`, .cold). During a 76 800 stream it pushes DBG:ISR? max_lat to 1.6-20 us with late / overruns / slips, and under a 20 Hz LOG:DISK? poll one trial fell back to 38 400. On 7409b92 the same runs had 0.24-0.32 us. Fixed in bab5dd4 (below); a device check is pending.

| # | Check | Result | Evidence |
|---|---|---|---|
| 1 | AC V, 40 Hz square, AD2, 76 800, DISK, `LOG:DUR 60` | **PASS** (data): gate **1 (kept)**, LOG:PRE? rung 76 800, LOG:SRATe? 76 800. **4 331 399 rows, lost_before 0** (7409b92: 2 095 713 rows, 54 % lost). The session ended **Full** (state 6) after 56.6 s: 60 s at 1.36 B/sample needs ~6.3 MB, and the disk has 5.9 MB free. ISR permille 546 (gate) / 548 (end), **1.478 ISR entries per sample**, ring high water **1017**, fifo_hw 445, 436 pages/s, tpp 382 / 387 us. DBG:ISR? dropped / bad / overruns / slips 0, **late 1, max_lat 4.14 us at `range_cmd`** (criterion < 1 us: FAIL, see "range_cmd"). LOG:COUNt? overwritten 0 | `d768.json`, `d768.DLG` |
| 2 | `DBG:PROF:RESET` at state 3, `DBG:PROF?` at the end (57.7 s window) | **PASS**: **8987 main-loop passes/s** (was 1272), `usb` (the Run pass) **avg 83 us** (was 718), 74.8 % of the loop. Loop max 94 ms / `usb` max 84.5 ms: one pass, also on 7409b92 (83.5 ms). The file has no hole, so that pass was outside the run (probably the Full end) | `d768.prof.txt` |
| 3 | A noisier input | **Not available**: the generator stays off, the calibrator has no sine and stops at 100 Hz (99 999 mHz), and its disabled output is quiet (0.6 mV AC). Ran the **100 Hz square** instead (2.5x the edges, 1.46 B/sample against 1.36), `LOG:DUR 30`: gate 1, 2 297 248 rows = 30.000 s, **lost_before 0**, high water **1135**, 468 pages/s. **overruns 3, slips 1, max_lat 6.86 us at `range_cmd`**. The fallback path at 76 800 (gate 2) showed only in the polled run below | `d768sq100.json` |
| 4 | `LOG:STAT OFF` on the first poll that shows state 3 (0.1 s polls, inside the trial) | **PASS**: state 5, bytes / rows 0, **no FAST013 in MMEM:CAT?**, used / free unchanged (10 829 824 / 5 910 528). The next sessions ran normally. Cosmetic: LOG:DISK? still names file 13 after the file is removed | `stop.py` |
| 5 | 38 400 / 4800 to the disk (`LOG:DUR 20`, gate 0), AD1 640 (DC V, OSR 0, CHOP OFF) | **PASS**: 768 002 / 96 002 / 12 802 rows, **lost_before 0**, overwritten 0. 38 400: cpu 149, 1.251 entries per sample, ppm_x10 -4, **max_lat 0.76 us at `range_cmd`**. 4800: cpu 17, **max_lat 4.66 us at `calbackup::FwFile::fill`** (region C: the info volume serving FIRMWARE.BIN. Presumably something on the host read it during the session; this test did not). AD1 640: max_lat 0.24 us. Cosmetic: LOG:DISK? tpp says 381 / 387 in the sessions without a probe (the last probe's value; the doc says 0 = none) | `d38400.json`, `d4800.json`, `dad1.json` |
| 5a | DC V (1 V) via AD2 at 76 800, `LOG:DUR 20`, and again at 10 s with LOG:DISK? at ~30 Hz | **PASS**: gate 1 both, 1 531 489 / 765 747 rows, **lost_before 0**, high water **109 / 116** (DC: 1.23 B/sample), Prep 11 / 8 s (with the self-cal). value_V 0.99912 against READ? 0.99975 (-0.06 %). max_lat 3.06 us (`range_cmd`) / 1.62 us. **A fallback (gate 2) in DC V was not provoked**, so the self-cal on the 38 400 restart is not checked | `ddc768.json`, `ddcpoll.json` |
| 5b | 76 800 session after `DBG:RESET`, state 3 at uptime 11.68 s and 16.36 s (CYCCNT at 200 MHz wraps every 21.47 s: its upper half is 10.7-21.5 s) | **PASS**: gate **1** both (verdicts at 12.69 / 17.38 s uptime), high water 1082 / 1091, 229 725 / 229 724 rows = 3.000 s, lost_before 0. The second: overruns 3, max_lat 2.22 us (`range_cmd`) | `early.py` |
| 6a | Host `LOG:DATA?` drain, 76 800, 30 s (`p2.py`) | **PASS** (as on 7409b92), faster: **40.5 k rows/s** (7409b92: 34.4-35.0 k), overwritten 1 083 265 (47 %: the host is still slower than 76.8 k), lost 0, bad 0, seq gaps 0, LOG:RATE? 76 576. **late 1, max_lat 5.40 us at `range_cmd`** (7409b92: 300 ns) | `h768.json` |
| 6b | Wave view 1 / 2 ms/div, 30 s each (`p5.py`) | Trace and readout right (Vp-p 9.94 / 9.99 V, `wave-1e-3.png`, `wave-2e-3.png`); dropped +0 in each window (22 at the switch to 1 ms/div, as before); cpu_permille 557 / 155. **1 ms/div: overruns 1, late 1, max_lat 7.08 us; 2 ms/div: late 1, max_lat 15.08 us**, both at `range_cmd` (7409b92: 280 / 260 ns, 0) | `p5.json` |
| 7a | Record page (Hold & Limits -> Record) Interval "Fast", Duration "Until stopped", Start by tap, AC V, LOG:SRAT 76 800 | **PASS**: the page offered "76800/s, 0 min left" (the estimate at ~3 B/sample; under a minute it says 0 min). Prep 8.2 s (the area was blank from the earlier sessions). Gate 1, panel "AD2 76574/76799 Sa/s", high water 1029. It ran to Full: **4 331 292 rows / 56.563 s, lost_before 0**, toast "Saved FAST013.DLG". The panel's "lost 2 (0.0%)" is LOG:COUNt?'s `missed` field (2), as in the 7409b92 Wave view. **The stop by tap could not be tested**: DBG:TAP 240,160 on the panel did not stop it. The fast session's stop reads the touch controller itself (main.rs `fast_touch`), and DBG:TAP injects past that. A finger is needed | `recfast.json`, `rec-fast.png`, `rec-run.png`, `rec-end.png` |
| 7b | Inrush save | **Not run**: it needs a load current on A-COM; the calibrator is wired to V/COM only | - |

**76 800 numbers (the PLL item, for the other agent):** rate_mhz 76 723 392-76 723 803 and ppm_x10 +9937..+9984 in every 76 800 session (the Pll on its +1000 ppm clamp). The record stamps give 76 574.4-76 574.9 rows/s, and LOG:PRE? measured_hz says 76 574. ISR entries per sample 1.478 (7409b92: 1.476). cpu_permille 546-548 (gate) / 548 (end), 557 in the Wave view. At 38 400: 1.251 entries per sample, cpu 149, ppm_x10 -4. At 4800: 1.251, cpu 17.

### range_cmd: a region C fetch on every SCPI command (found here, fixed in bab5dd4)

Every max_lat over 1 us at 76 800 / 38 400 in this session had lat_pc 0x080957d8 = the entry of `scpi::range_cmd`. The two exceptions are 1.6 us at an OUTLINED_FUNCTION in B and the 4800 vdisk read. de01f2f (SCPI ranges) put `range_cmd` in .cold, on the assumption that it runs once per range command. The dispatcher calls it, though, for every multi-part header that gets past the CONFigure arms: LOG:DATA?, LOG:DISK?, LOG:PRE?, DBG:ISR?, MMEM:... (the LOG / DBG / MMEM arms come after it). Region C is not zero-wait, and a fetch there stalls the AD2 stream ISR's entry (link-native.x: ~20 us).

`lat.py` (76 800 / 38 400 to the disk, `LOG:DUR` 10 / 20, no polling during the session vs LOG:DISK? at 20 Hz):

| rate | host during the session | gate | high water | max_lat | lat_pc | overruns / slips |
|---|---|---|---|---|---|---|
| 38 400 | nothing | 0 | - | 280 ns | spiflash::xfer | 0 / 0 |
| 38 400 | 20 Hz LOG:DISK? (~400 polls) | 0 | - | 400 ns | scpi::kw | 0 / 0 |
| 76 800 | nothing | 1 | 1170 | 320 ns | spiflash::xfer | 0 / 0 |
| 76 800 | 20 Hz LOG:DISK? | **2 (fell back to 38 400)** | **2048** | **19.96 us** | **range_cmd** | 1 / 1 |
| 76 800 | 20 Hz LOG:DISK? (repeat) | 1 | 1093 | 1.64 us | OUTLINED_FUNCTION_176 (B) | 0 / 0 |

Without SCPI traffic the stream is as clean as on 7409b92. At 38 400 (div 16, few ISR entries) a stall rarely meets an ISR entry. At 76 800 (an ISR entry every ~9 us) it often does. The fallback run lost nothing to the file (it ran at 38 400, 384 002 rows), but the trial verdict was spoiled. This test does not separate whether the full ring (2048) came from the stalls or from the overrun / slip handling.

**Fix (bab5dd4, branch `fastlog-768-retest`, on rust-fw-poc 848047f):** the dispatcher calls `range_cmd` only when a part of the header after the first is RANGe / RESolution (a `kw` check in region B). range_cmd stays in C. The other .cold SCPI helper on the dispatch path, `wav_cmd`, runs only under the WAVeform arm. Image: `dm40-fw-bab5dd4.bin` in the session scratchpad (621 184 B, `dm40dbg.py image` OK, built from this branch's tree; region B 62 796 B free, C 17 888 B). Not run in the emulator or on the device.

**Re-check on bab5dd4:** item 1 (`disk.py AD2 76800 60`), `lat.py 76800 10 poll` x3, `p2.py h768 76800 30` and `p5.py`: max_lat < 1 us, late / overruns / slips 0, gate 1 under the poll. The range commands still work: `VOLT:RANG?`, `VOLT:RANG 60`, `SENS:VOLT:RANG:AUTO ON`, `VOLT:RES?`, `CURR:DC:RANG?`.

### State at the end

- **Device:** DMM page, DC V, AUTO+ (`VOLT:RANG:AUTO?` 1), rate SLOW, `VOLT:COUP?` DC, method A (`end.png`), as found. LOG restored: AD1 / 4800 / HOST / `LOG:DUR` 60 / CHOP 1 / OSR 3 / `LOG:STAT` 0. The Record page showed "Fast" until put back: Interval "Every reading" by four "-" taps ("+" stops at Fast), Duration "Until stopped" unchanged (`rec-fix2.png`). Generator `OUTP?` 0 throughout. The generator page was opened once by mistake (MF + a tap on the main menu's Signal tile) and left with MENU; nothing was changed on it. Settings not touched (volume 0, backlight 10 %, Dim / Shutdown Never). `SYST:ERR?` empty. `DBG:HYSPI:SELF? 2000`: all error counts 0.
- **Files:** every FAST013 of this session was deleted. `MMEM:CAT?` used / free 10 829 824 / 5 910 528, the same as at the start.
- **Health:** crash count 0, HEALTHY, marker VALID. Three `DBG:RESET`s (item 5b).
- **Calibrator:** 1 V DC, enabled.

## bab5dd4: range_cmd latency fix and the 8087c7d follow-up fixes (2026-10-23 06:07 - 06:52)

The owner had installed bab5dd4 (install OK, crash count 0). It carries the code of rust-fw-poc fa5f603: the range_cmd fix (previous section) and the tmcnl-fix commits up to 848047f (USBTMC '\n', AC+DC range settle, same-range set, rate order, libsigrok close). `DBG:BOOT?` NORMAL / HEALTHY, crash count 0, marker VALID at the start and the end; no reset (uptime 0:59 -> 45:09).
- **Wiring:** the calibrator (`/dev/ttyACM1`) on V/COM: the 40 Hz square (Part A, B5 square), DC 4..7 V (B5 autorange), 1 V DC (the rest). The A jack, the MCX probe and the generator output were not wired.
- **Files:** scripts, JSON, PNGs in the session scratchpad `rt5/` (not in git). `disk.py`, `lat.py`, `p2.py`, `p5.py` are the 4c4b489 scripts; `rng.py` the range commands; `sq5.py`, `ar6.py`, `ar6b.py`, `dc5.py`, `dc5b.py`, `dcpart.py` (B5); `dc4.py`, `dc781.py`, `tap4.py` (B4); `rate6.py` (B6); `tl.py` logs READ? and `DBG:ACDC:STAT?` after a change.
- **Symbols:** lat_pc looked up in the ELF of the bab5dd4 build (`rt768/`, same timestamp as `dm40-fw-bab5dd4.bin`).

### Summary

**Part A: the range_cmd fix works.** max_lat 260-320 ns everywhere, late / overruns / slips 0, the trial kept under the 20 Hz poll. **Part B: the five fixes pass their own checks.** The retest found four AC+DC / autorange issues around them and reproduced the 0.781 V case (TODO items).

| # | Check | Result | Evidence |
|---|---|---|---|
| A1 | `disk.py AD2 76800 60`, AC V, 40 Hz square | **PASS**: gate 1, 4 329 787 rows / 56.5 s (Full: 5.9 MB free), **lost_before 0**, high water 1066, ISR 548 / 550 permille, 1.480 entries per sample, LOG:COUNt? overwritten 0. **max_lat 320 ns** (`spiflash::xfer`), late / overruns / slips **0** (4c4b489: 4.14 us at range_cmd, late 1) | `d768.json` |
| A2 | `lat.py 76800 10 poll` x3 (327 / 327 / 335 LOG:DISK? polls) | **PASS** x3: gate **1** each (kept), 765 728-765 730 rows, high water 75-120, **max_lat 320 ns** (`spiflash::xfer`), late / overruns / slips 0 / 0 / 0. 4c4b489: one fallback (gate 2, ring 2048, 19.96 us) and 1.64 us | console |
| A3 | `p2.py h768 76800 30` (host LOG:DATA? drain) | **PASS**: 38.3 k rows/s (4c4b489 40.5 k), overwritten 1 148 478 (the host is slower than 76.8 k, as before), host seq gaps 0, LOG:RATE? 76 572. **max_lat 280 ns** (`dm40_fw::run`), late 0, overruns 0, slips 0 (4c4b489: 5.40 us at range_cmd) | `h768.json` |
| A4 | `p5.py`, Wave view 1 / 2 ms/div, 30 s each | **PASS**: Vp-p 9.94 V, trace right (`wave0.png`, `wave-1e-3.png`, `wave-2e-3.png`); dropped +0 in each window (111 at the switch, before the window); **max_lat 280 / 260 ns** (`run` / `lcd::set_window_cmd`), late / overruns / slips 0; cpu 561 / 155 permille (4c4b489: 7.08 / 15.08 us at range_cmd) | `p5.json` |
| A5 | Range commands (DC V, from AUTO+) | **PASS**: `VOLT:RANG?` +6, `VOLT:RANG 60` -> `VOLT:RANG?` +60, `VOLT:RANG:AUTO?` 0, CONF? "VOLT +60,+1E-03"; `SENS:VOLT:RANG:AUTO ON` -> AUTO? 1, +6; `VOLT:RES?` +1E-05; `CURR:DC:RANG?` +9.91E37 with "Settings conflict" (documented: one range state, the current function's). All answer in < 1 ms. The first CONF? right after AUTO ON said 1E-04, then 1E-05 (probably the extra digit waiting for 8 averaged conversions) | `rng.py` |
| B1 | ngscopeclient-dm40 `--headless 15 ngs.png /dev/usbtmc2`; `test/dm40cli usbtmc /dev/usbtmc2` | **PASS**: TRIGGERED, a flat trace at 0 mV (open input), generator fields filled (Square, 2 MHz, 50 %, 3 V, 1.5 V, GEN OFF) (`ngs.png`). dm40cli: every protocol check ok (rates, trigger, generator set / read back), 5 records of 26 624 points in 0.84 s; the 3 signal checks FAIL as expected (no loopback: 0.8577 V flat). dm40cli sets the generator to sine 1 kHz 1 Vpp 0.5 V offset; the output stayed off (`OUTP?` 0); restored to Square 2 MHz 3 V 1.5 V by SCPI | `ngs.png` |
| B2 | Host tools over the '\n' terminator | **PASS**: `dm40scpi info`, `read -n 5`, `log -n 6` (readings CSV), `log --source ad1 -d 5` (191 samples, 0 lost), `log --source ad2 --rate 4800 -d 4` (19 196 samples, 0 lost), `get S000.BMP` (17 279 B) and `get S004.BMP` (307 266 B, 0.8 s); `dm40dbg shot` (GRAM read, 0.8 s). pyvisa 1.14.1 / pyvisa-py `USB0::4617::1::0001::0::INSTR`: `query('OUTP?')` -> `'0\n'` in 1 ms, `*IDN?`, `READ?` (248 ms), `VOLT:RANG?`. pyvisa-py detaches the usbtmc driver (host side; `reattach.py`). The install path was proven by the owner's reload.sh | console |
| B3 | sigrok (libsigrok-dm40 install, `-d scpi-dmm:conn=usbtmc/3.71`) | **PASS**: `--show` lists V / A / ohm / ... , range Auto / 600mV .. 1000V, digits 5 / 6; `-c range=6V` x3, `60V` x2, `Auto` with `--samples 5`: rc 0, readings 999.0-999.5 mV, CONF? / AUTO? follow (Auto -> AUTO? 1). **/dev/usbtmc2 still there after every run** | console |
| B4 | DC V on 1 V: the same range set again | **PASS**: `VOLT:RANG 6` x10 and `60` x10 from manual, x10 each from AUTO+ (`dc4.py`); 13 MF-page taps on the highlighted item (6 V manual / from AUTO+, 60 V manual; `tap4.py`, `mf6b.png`); sigrok range= the present range (B3). Lowest READ? / `CALC:AVER:MIN?` 0.99869 V on 6 V, 0.99853 V on 60 V; 0.99789 V in the real 6 -> 60 V changes (60 V range). Nothing below 0.99 V. **0.781 V case: reproduced** (see 1) | `dc4.json` |
| B5 | AC+DC method A on 1 V, AUTO coupling, the square, autorange across 6 V | **PASS with findings**: no fresh reading off by > 0.1 % in 29 of 30 AC+DC changes (SCPI and MF, 6 <-> 60 V); the one exception and one in AUTO coupling are a DC part of 0.53 V (2). Gap to the first fresh reading 3.0-4.3 s (5.7 s once). AC part at that reading: 15-27 mV after 6 -> 60 V, falling to 0.4-2.5 mV by 9-12 s; 2-3 mV (max 7 mV) on 6 V. AUTO coupling: AUTO,DC in every sample. The square: 3 of 4 changes right, 600 -> 60 V +0.55 % (4). Autorange across 6 V: one range step per level at most, no hunting; one excursion after a step down (5). During the gap READ? returns the pre-change reading (3) | `sq5.json`, `ar6.json`, `ar6b.json`, `dc5.json`, `dc5b-ACDC.json`, `dcpart-ACDC.json` |
| B6 | Rate chip and value tap | **PASS**: chip SLOW -> FAST -> MED -> SLOW and FAST -> MED -> SLOW -> FAST -> MED (`RATE?`); value tap 2.5 s apart MED -> SLOW -> FAST -> MED -> SLOW, SLOW -> FAST -> MED -> SLOW -> FAST. A second value tap 1 s after the first changed nothing (SLOW -> FAST -> FAST -> MED); the chip at 1 s did not skip. Cosmetic | `rate6.py`, `chip*.png` |

### 1. DC V: 0.77-0.79 V after AUTO from 600 / 1000 V (the 8087c7d 0.781 V)

`VOLT:RANG:AUTO ON` from a manual 600 V or 1000 V range (1 V in) steps down twice, to 60 V and then 6 V. In 5 of 35 tries one READ? gave 0.767-0.790 V (0.767, 0.790, 0.788, 0.767, 0.786): 600 V 1 of 5 (`dc4.py`) and 3 of 20 (`dc781.py`), 1000 V 1 of 10. From 60 V (one step) it happened 0 of 10 times. The low value was the second reading on the 6 V range, ~0.8 s after the command (`VOLT:RANG?` already +6). The next reading was right (0.9987-0.9990 V). The chart MIN records it. The same-range fix does not cover this, since it is a real range change. The averaging window is not the cause by itself (write_config resets it on each step). The suspects are unchecked: a conversion that straddles a register write (the OSR write in `osr()` after the first 6 V conversion), or the 250 ms autorange settle being shorter than a SLOW conversion. A raw AD1 code trace around the second step would decide.

### 2. AC+DC / AUTO: DC part 0.53 V once after an MF 6 -> 60 V change

In `dc5.py` 2 of 4 MF-page changes 6 -> 60 V gave a DC part of 0.5258 V (AC+DC) and 0.5284 V (AUTO coupling, shown as AUTO,DC) for one DC phase (~1 s, 2.6 s after the tap). READ? answered 0.526 / 0.528 V four times, then 0.999 V. Two repeats gave 0 of 9 (`dc5b.py` with READ? polling, `dcpart.py` with `DBG:ACDC:STAT?` at 25 Hz). SCPI changes gave 0 of 30, and MF 60 -> 6 V gave 0 of 13. The 53 % value matches the 25-51 % partial conversions of the 8087c7d same-range case. The cause is open.

### 3. AC+DC: READ? during the settle answers the pre-change reading

After `VOLT:ACDC:RANG 60` / `6` (and the MF page) READ? did not answer +9.91E37 during the AC part's settle. It answered the last reading from before the change, every 0.85 s (`SCPI_QUICK_MS`), 3-6 times, until the first fresh one. The tmcnl-fix TODO item expected NaN ("READ? NaN after 0.85 s, as at entry"). The scpi.rs header says "the latest one if none comes within SCPI_QUICK_MS" and keeps the settle wait for CONF / MEAS / FUNC only. On a steady input the value is right. After an AC+DC autorange step it is not: with the calibrator going 5.8 -> 6.3 V, READ? held 3.3885 V (a transient reading) for 5 s while the meter settled on 60 V (`ar6.json`). This needs a decision: drop the latest reading on a range change in AC+DC, or keep the rule and fix the TODO's claim.

### 4. AC+DC: the 1 % settle step lets the 600 -> 60 V tail through (40 Hz square)

On the square (AC 4.768 V, DC 4.772 V, total 6.746 V; the 6 V range is OL there) the range changes on the manual range gave (`sq5.json`):
- **AUTO -> 60 V:** right at once.
- **60 -> 600 V:** first fresh reading at 3.9 s, 6.733 V (the 600 V range, -0.2 %, within its resolution).
- **6 -> 60 V:** first fresh reading at 3.5 s, right.
- **600 -> 60 V:** the AC part came back at 1.6 s as 4.821 V, then 4.790 V, and only at 3.3 s as 4.768 V. READ? gave 6.783 / 6.762 V (**+0.55 / +0.24 %**). Each result fell by < 1 %, so `ac_settle_step` took them as flat. This is the case the TODO item warned about. Options are a stricter step (e.g. 0.2 %) or a minimum time after the change.

### 5. AC+DC autorange across 6 V (DC 4..7 V)

Steps 5.0 / 5.8 / 6.3 / 7.0 / 6.3 / 5.8 / 5.5 / 4.0 / 6.5 / 5.0 V, 12 s each (`ar6.py`), then again with `DBG:ACDC:STAT?` at 25 Hz (`ar6b.py`). At most one range step per level, and no hunting. Above 6.0 V the meter moved to 60 V, and below 6.0 V back to 6 V. The settled readings were 3.9971 / 4.9976 / 5.4972 / 5.7979 / 6.2954 / 6.4961 / 6.9965 V.

A DC step itself gives the AC path a large AC part for about a second: up to 3.4 V for a 0.8 V step. The calibrator's own step is clean within 0.3 s (`calstep.py`, DC V on 60 V). hypot then exceeds 6 V, and on 5.8 -> 5.0 V the meter went 6 -> 60 V at 0.25 s and back to 6 V at 5.98 s. That is one excursion per step, not a hunt. During it READ? held old values (3).

### Follow-up: branch settle-fix (not on the device yet)

- **1 (0.781 V), cause:** in `dc781.json` the READ?s at 0.31 / 0.56 s are the 600 V and 60 V conversions that made the two steps (published as each step is taken); the bad one at 0.81 s is the first conversion read on 6 V, and in every bad try the next reading came 10 ms later (0.81 / 0.82 s, good tries 0.81 / 0.91 s). The 600 V row (R22 0x17) -> 60 V row (0x15) restarts AD1; the 6 V row 250 ms later is 0x15 again with ENAD1 kept, so AD1 is not restarted and its decimation filter still holds 60 V-tap samples; the 6 V deadline falls on a conversion end and the poll sometimes reads the older, mixed one. From 60 V (0x17 -> 0x15) AD1 restarts: 0 of 10. Fix: the first conversions on a new range must agree pairwise within 0.5 % (`vet_ad1`). Emulator repro: HY3131 model `ad1mix:1` + `emu/hy3131/autorangetest.py` (base build: 0.55 / 0.85 V readings with the device's 1 ms pair signature; fixed build: none).
- **2 (DC part 53 %):** cause not proven (MF and SCPI call the same `set_range`; the MF path delays the DMM poll); the AC+DC DC phase vets its first 8 results on a new range, so such a result is held.
- **3 (READ?):** decided NaN: READ? / FETCh? answer +9.91E37 from a range / function change to the first reading of the new range (`Dmm::reading_pending`).
- **4 (tail):** flat = fell <= 0.1 % and, while still falling, <= 0.05 % of the decay left; cap 5 s, also in the DC phase.
- **5 (DC step):** the same settle after an input step (DC part moved > 0.5 % of the range, or an AC result > 1.5 x the AC part + 0.5 %), with the AC part held and no range move on the discarded results.

Retest list: TODO items of this section (each has its device check).

### State at the end

- **Device:** DMM page, DC V, AUTO+ (`VOLT:RANG:AUTO?` 1), rate SLOW, `CONF?` "VOLT +6,+1E-05", `VOLT:COUP?` DC, method A (`end.png`), as found. LOG restored: AD1 / 4800 / HOST / `LOG:DUR` 60 / CHOP 1 / OSR 3 / `LOG:STAT` 0 (`disk.py` and `p2.py` had left LOG:SRAT 76800). Generator `OUTP?` 0 throughout; its settings were restored after dm40cli (Square 2 MHz, 3 V, 1.5 V offset, 50 %). The scope's timebase / trigger were left as ngscopeclient / dm40cli set them. The main menu showed for part of the session (MENU from the Wave view); one tap meant for the range page landed on its Multimeter tile, and the next on the DMM page's chart edge: nothing changed (REL off, settings as before). Settings not touched (volume 0, backlight 10 %, Dim / Shutdown Never). `SYST:ERR?` empty.
- **Files:** the four FAST013 files were deleted. `MMEM:CAT?` identical to the start (10 829 824 / 5 910 528). `dm40dbg shot` reads the GRAM and writes nothing on the meter.
- **Health:** crash count 0, HEALTHY, marker VALID, no reset.
- **Calibrator:** 1 V DC, enabled.

## e7ec2cc: the 76.8 kS/s PLL retest (TODO "76800 stream / rate" 1-8) (2026-10-23 07:10 - 07:45)

The owner had installed e7ec2cc (install OK, crash count 0). `DBG:BOOT?` NORMAL / HEALTHY, crash count 0, marker VALID at the start and the end. One deliberate reset (`dm40dbg.py reset`, item 4).
- **Wiring:** the calibrator (`/dev/ttyACM1`) on V/COM: the 40 Hz square (`set_output('frequency', 40.0)`, READ? in AC V 4.764 V) for items 1-7 and 8 (AC), 1 V DC for item 8 (DC). The A jack, the MCX probe and the generator output were not wired.
- **Files:** scripts, JSON, NPY, DLG / CSV and PNGs in the session scratchpad `devchk-e7ec/` (not in git). `p3.py` = `p2.py` plus a `DBG:ISR?` line every 5 s; `disk.py`, `lost.py` as before; `wave6.py` / `wav.py` (item 6); `rate.py`, `spp.py` (samples per calibrator period from a gap-free CSV), `edges6.py` (edges4 with the period fixed to +-30 us around the falling edge: edges4 / edges5 mis-select on CHOP OFF dumps, old ones included); `item8.sh`.
- **Symbols:** lat_pc looked up in the e7ec2cc ELF (`acdcfix/basewt`, its image == `dm40-fw-e7ec2cc.bin`).

### Summary

**The loop fix works. The nominal change is wrong.** In every 76 800 session, including a cold start, the Pll locks within 5 s at **rate_mhz 76 800 0xx, ppm_x10 -29 35x (-2935 ppm)** against the new nominal 76 574.6, at **1.125 ISR entries per sample** (was 1.478) and **cpu 455-457 permille** (was 546-548). late / bad / overruns / slips are 0, and the transition-sample scatter is **0.41-0.62 us rms** (was 3.0-3.45 us). The stream's rate is measured three independent ways: gap-free disk data has **1919.95 samples per 40 Hz calibrator period** (bab5dd4: 1914.27), the Wave view's record has 960 samples per half period, and the stamps step 13.02 us. So with a loop that tracks, the chip converts at the datasheet's 76 800 S/s. The 76 574.6 of 7409b92 / 4c4b489 / bab5dd4 came from the old railed loop. These data do not separate a slower chip under that read pattern from skipped conversions, but the old builds' per-period count scatters far more (sd 0.46 against 0.014 samples). With e7ec2cc's nominal, every 76 800 time axis is 0.29 % long: LOG:PRE? nominal_mhz, the DLG / CSV headers, and the Wave view (a 40 Hz square reads 39.88 Hz). **Fix (ee28b71, below): the nominal goes back to the datasheet's; the loop and MAX_PPM 5000 stay.**

| # | Check | Result | Evidence |
|---|---|---|---|
| 1 | AC V, 40 Hz square, AD2 76 800, host drain 60 s, CHOP default / OFF / ON (`p3.py`) | **Loop PASS, rate premise wrong.** rate_mhz **76 800 017-76 800 071** (expected ~76 574 600), ppm_x10 **-29 350..-29 358** (expected within +-50), locked 1 from the first 5 s snapshot on. Entries per sample **1.1295 at 5 s -> 1.1252-1.1254 at 60 s** (was 1.478); **cpu 455** (expected < 544); probes / captured **0.0626-0.0627**, edges_after_b / probes **0.4996-0.5002**; late / bad / overruns / slips **0 / 0 / 0 / 0**, max_lat 260 ns, max_run 37 / 52 / 51. LOG:PRE? nominal_mhz **76 575 541** (= 1e12 / round(13 059 ns), not 76 574 600: cosmetic); LOG:RATE? 76 799 / 76 800 / 76 800. Drain 46.0-46.5 k rows/s (overwritten ~1.8 M per run, as always: the host is slower), host seq gaps 0 | `a768def.json`, `a768off.json`, `a768on.json` |
| 2 | Sample-to-stamp scatter, falling-edge transition samples (`edges4.py`, `edges5.py`, `edges6.py`) | **PASS**: edges6 rms **0.42 / 0.62 / 0.41 us** (default / OFF / ON), p99 1.0-1.4 us, max 1.2-1.7 us. Same script on the old dumps: 7409b92 3.01 / 3.15 / 3.20 us (on / off / b), bab5dd4 3.45 us (p99 8.6-8.8, max 10.5-14.1 us). edges5 (local scatter) 1.55-1.59 us, against 8.3-8.7 / 5.4 us on the old dumps | `edges6.py` output |
| 3 | A second 76 800 session right after, then 38 400 (30 s) | **PASS**: the OFF / ON runs (sessions 2 and 3, after the disk and Wave view sessions) lock the same (row 1). 38 400: rate_mhz 38 400 011-033, **ppm_x10 -2..-8** (expected ~-16: within 1 ppm), **entries 1.2501-1.2507**, cpu 149, overwritten 0, all error counters 0, edges6 0.55 us | `h384.json` |
| 4 | The first 76 800 session after `dm40dbg.py reset` (uptime 11 s), 60 s | **PASS**: locked at 5 s, ppm_x10 -29 352..-29 355, **entries 1.1295 (5 s) -> 1.1254 (60 s)**, cpu 455-456, error counters 0, max_run 36, edges6 0.57 us. The session's first run (`a768def`) was also the first AD2 session since the install's boot (DBG:ISR? all 0) and is the same. A cold start pulls -2935 ppm in, so REL_HYB is not needed for this | `boot768.json` |
| 5 | Disk, 76 800, `LOG:DUR 60`, square (`disk.py`) | **PASS** (data), **header nominal wrong**: gate **1 (kept)**, ISR permille **457** (expected < 546), **ring high water 52** (4c4b489 / bab5dd4: 1017 / 1066), fifo_hw 389 (436-445). 4 378 193 rows / 57.008 s (Full: 5.9 MB free), **lost_before 0**, LOG:COUNt? overwritten 0, max_lat 280 ns. The `dlog.py` header says **nominal 76575.541 S/s**, but the rows are 76 800.1 / s by their stamps and **76 797.8 S/s by the calibrator** (2279 periods in 4 375 556 rows, 1919.95 +- 0.014 per period; the calibrator is -29 ppm against the MCU by the stamps' 24 999.28 us period, so 76 800 in MCU time). bab5dd4's `d768.csv`: 1914.27 +- 0.46 = 76 570.7 | `d768.json`, `spp.py` |
| 6 | Wave view, AC V, MF -> Wave, 1 / 2 ms/div, 30 s each (`wave6.py`, `wav.py`) | **FAIL (as the nominal predicts)**. `ACQ:SRAT?` 76 575 (as expected) but the stream runs at 76 800. `SCOP:MEAS:FREQ?` answers **0** at 1 and 2 ms/div, and the screen shows "Freq: --": the 10 ms screen holds less than one 25 ms period, so this check cannot work as written. The record (`WAV:POIN MAX`, 26 624 points) has **960 samples per half period** (959 / 961 alternating), and with `WAV:PRE?` xinc 13.059 us that is **39.88 Hz**. Vp-p 9.94 V, trace right (`wave-1e-3.png`). ISR at 1 ms/div: 76 800 0xx locked, entries 1.125, cpu 470. **late 1, max_lat 6.16 us at lat_pc 0x0808badc = `scope_wav::query`** (region C), probably this script's `ACQ:SRAT?` / `SCOP:MEAS:...?` polls (bab5dd4's `p5.py` did not send them: 280 ns). 2 ms/div: 38 400, ppm_x10 -3..-6, 1.250, cpu 156, max_lat 2.12 us at the same PC, late 0 | `wave6.json`, `wav1ms.npy` |
| 7 | `dm40scpi.py log l768.csv --source ad2 --rate 76800 -d 5` | Header **`# nominal_sps,76575.541`** (the build's nominal, wrong as above); stamps 76 799.8 / s. The run ignored `-d 5` (the known drain() TODO) and was stopped by SIGINT at 20 s. **After the SIGINT the USBTMC link answered every query with an empty read** until a USBTMC INITIATE_CLEAR ioctl (new TODO) | `l768.csv` |
| 8 | Disk sanity, `LOG:DUR 20`: AD2 38 400 / 4800 (AC V, square), AD1 640 (DC V, 1 V, OSR 0, CHOP OFF), AD2 76 800 in DC V (1 V) | **PASS**: 768 002 / 96 002 / 12 802 / 1 536 002 rows, **lost_before 0**, overwritten 0 in all four. ppm_x10 **-3** at 38 400 / 4800 / AD1 640 (expected ~-16: the lock is at -0.3 ppm today); entries 1.250 / 1.251 / 1.258 / 1.125; cpu 149 / 17 / 11 / 457; max_lat 240-280 ns. DC V via AD2: gate 1, high water 51, value_V mean 0.99949 (sd 2.3 mV) against READ? 1.00002 V (-0.05 %) | `d38400.json`, `d4800.json`, `dad1.json`, `ddc768.json` |

### Fix: the datasheet's rate as the 76.8 kS/s nominal (ee28b71, not on the device)

Branch `pll768-datasheet-rate` on e7ec2cc. `hy3131.rs` sets `AD2_OSR32_REAL_MHZ = ad2_rate_mhz(0)` (76 799 016 mHz, the other rungs' formula). The function names stay, so LOG:PRE? nominal_mhz, the DLG / CSV headers, the gap accounting and `scope.rs` `wave_real_fs` all go back to the datasheet's rate. The ad2pll.rs loop changes and MAX_PPM 5000 stay; whether 5000 should stay is the owner's call (the device needs only ~15 ppm now). Emulator: HY3131 `Ad2Osr32Ppm` default 0, and `ad2streamtest.py` / `wavetest.py` / `fastlogtest.py` expect the datasheet's rate. Host: `meter-test` 89 passed, `touch-test` 892 cases 0 failures. The emulator suites were not run. Image: `dm40-fw-ee28b71.bin` in the session scratchpad (621 184 B, built from the committed tree). `dm40dbg.py image` was not run on it (the session's permission check refused it); `reload.sh` runs the same check before staging. After the fix ACQ:SRAT? says 76 799 (the datasheet formula's 76 799.016, rounded like 38 400 -> 38 399.5), 13 ppm under the measured 76 800 03x: expected, not a bug. **Device check:** a fresh-boot 76 800 host drain locks at ppm_x10 near 0 (about -2..-16, as 38 400) with entries ~1.125, and LOG:PRE? nominal_mhz is 76 799 016. The Wave view's record at 1 ms/div gives 40.00 Hz from xinc, and ACQ:SRAT? says 76 799. The disk header says nominal 76 799.016.

### Mistake: FAST000-FAST012 deleted from the meter (07:30)

A cleanup helper in this session deleted every `LOGS/FAST*.DLG` on the meter instead of only this session's FAST013: **FAST000-FAST012 (13 files, 7.26 MB), which predate this session.** Within seconds the next disk session's Prep (38 400, stopped at 42 %) and the idle-time preparation started erasing the freed clusters. A copy of the raw flash (`DEBUG/SPIFLASH.BIN` on the USB volume, read at 07:34) gives:
- **Recovered, decode with `dlog.py`:** FAST004-FAST012 (FAST008 has a CRC error in its last chunk: one cluster was reused). They are AD2 fast-log sessions: 4800 AC, 19 200 AC CHOP 0 / 1, 9600 DC, and 38 400 AC (x4). Copies are in the scratchpad's `devchk-e7ec/recovered/`, with the image in `devchk-e7ec/spiflash-after-delete.bin`.
- **Lost:** FAST000-FAST002 (erased to 0xFF), and FAST003 except its tail (63 of 302 clusters erased, the header with them; the tail is kept as `FAST003.partial-tail.raw`).
- The files were not written back: the USB volume is read-only, and restoring them is the owner's decision.
- From then on the helper deleted only files whose (name, size) was not in the pre-session `MMEM:CAT?`. The new FAST000-FAST003 of item 8 reused the old names and were deleted at the end.

### State at the end

- **Device:** DMM page, DC V, AUTO+ (`VOLT:RANG:AUTO?` 1), `CONF?` "VOLT +6,+1E-05", `VOLT:COUP?` DC (`end.png`). LOG restored: AD1 / 4800 / HOST / `LOG:DUR` 60 / CHOP 1 / OSR 3 / `LOG:STAT` 0. The Wave view's timebase was left at 1 ms/div. Generator `OUTP?` 0 throughout and never opened. Settings not touched (volume 0, backlight 10 %, Dim / Shutdown Never). `SYST:ERR?` empty.
- **Files:** every file this session made was deleted (FAST013, FAST000-FAST003). `MMEM:CAT?` used / free **3 534 848 / 13 205 504**, against 10 829 824 / 5 910 528 at the start: the 13 old FAST files are gone (above). No other file changed.
- **Health:** crash count 0, HEALTHY, marker VALID. One `dm40dbg.py reset` (item 4), no crash.
- **Calibrator:** 1 V DC, enabled.

## c6f9670: AC+DC method C retest of §10.9 (acdc-approaches.md §10.10) (2026-10-23 08:59 - 09:40)

The owner had installed c6f9670 and booted it (install OK, crash count 0). `DBG:BOOT?` showed NORMAL, crash count 0, VALID at the start and at the end.
- **Wiring:** the calibrator (`/dev/ttyACM1`) on V/COM: DC 0..24 V and the 40 Hz square. The A jack, the MCX probe and the generator output were not wired.
- **Runs** (`DM40_USBTMC=real /usr/bin/python3 tools/acdcmeth.py ... -o acdc29.jsonl`): e8up at 09:00 (1 min after power-on), then `dm40dbg.py reset` (09:02), e8s, e8, e11, e10 + e9, and `e5 --minutes 3` (09:20-09:31). Helper scripts: `refs.py` (DC V READ? at every level again right after each e8 run, to bracket the refs), `hand29.py` / `hand29b.py` / `tap29.py` (AUTO by touch with screenshots), `tc6.py` (the 6 V two-point step as a time course, 3 repeats, each after a `dm40dbg.py reset`: 09:33 / 09:34 / 09:35), `dctail.py` (DC V after the same step).
- **Files:** in the session scratchpad `rt29/` (not in git): `acdc29.jsonl`, `run-*.log`, `refs.jsonl`, `tc6.jsonl`, `dctail-*.json`, `hand29*.log`, `h29-*.png`, `tap29-*.png`, `pre.png`, `end.png`, `cat0.txt` / `cat1.txt`.
- **References:** each C reading is compared with the meter's own DC V READ? on the same manual range, taken before the C session (the script's `ref`) and again right after it (`refs.py` / `tc6.py`). The two agree within 0.05-0.33 mV, so no verdict below depends on the calibrator's wander.

### Summary

**Decision rule (§10.4 / §10.7): not met, for a new reason.** The 60 V failure of 2563fe1 is fixed, and e8up, e8, e11, e10, e9 and e5 pass. But e8s on the **6 V** range fails at 0 V. There the two-point gain of §10.9, taken over a 5 -> 3 V step (41.6 k codes), replaced the internal zero's gain (104 k codes) and was 143-195 ppm high in all 4 power-on sessions tried. The cause is a slow tail in the AD2 LPF after a step (section 2). The default stays A. The owner decides.

| # | Check | Pass | Result (C's DC part - READ?, mV; pre / post ref where both) |
|---|---|---|---|
| e8up | 60 V, entered at 0 V, then 1 / 5 / 12 / 24 V (4 s per level) | 5.0 / 5.5 / 7.5 / 11 / 17 mV | **PASS**: -2.23 / -1.35 / -0.07 / -2.85 / +1.22 (post: -2.09 / -1.22 / +0.19 / -3.0 / +0.9). gspan 0 -> 10 305 (5 V) -> 39 138 (24 V), as §10.10 expected. 24 V +1.2 mV: the "~+10 mV without the fix" is gone |
| e8s 60 V | after a power cycle: entered at 24 V, then 12 / 5 / 1 / 0 V | 17 / 11 / 7.5 / 5.5 / 5.0 | **PASS**: +2.51 / +1.49 / +1.56 / **+1.82 / +1.90** (post: +2.6 / +1.2 / +1.5 / +1.8 / +1.6). 2563fe1: -0.47 / -4.34 / -6.12 / **-7.23 / -8.01**. gspan 49 425 (internal zero) -> 24 709 (two-point at 12 V), as expected |
| e8s 6 V | entered at 5 V, then 3 / 1 / 0 V | 3.0 / 2.0 / 1.0 / 0.5 | 5 / 3 / 1 V PASS: -0.14 / -0.21 / -0.55; **0 V FAIL: -0.86** (post: -0.23 / -0.37 / -0.81 / **-0.85**). pc 4.80824e-5 (internal zero, 103 937) -> **4.80915e-5** (two-point at 3 V, 41 584): +190 ppm. 2563fe1: +0.07..+0.44 |
| e8kept (e8s) | 5 V on 60 V re-entered, kept gain | 7.5 | PASS +1.39 (gspan 24 709) |
| e8 | 1 s per level, after e8s (gains kept) | as e8s | **PASS** everywhere. 60 V: +3.61 / -0.16 / -0.21 / -0.39 / -0.78 (new two-point at 5 V, 39 131). 6 V: +0.31 / -0.02 / +0.11 / +0.10 (post: +0.09 / -0.12 / +0.06 / +0.11); new two-point at 1 V over 83 158 codes: pc 4.80823e-5, 2 ppm from the internal zero's. e8kept +0.31 |
| tc6 | the e8s 6 V step 5 -> 3 V after a power cycle, 3 repeats; READ? right after each C mean | 0 V within 0.5 | two-point pc **+195 / +146 / +143 ppm** over each session's internal zero (4.80873 / 4.80798 / 4.80851e-5: the internal zero itself scatters +-80 ppm over power cycles). 3 V anchor -0.21 / -0.26 / -0.12; 1 V -0.15 / -0.14 / -0.43; **0 V -0.92 / -0.25 / -0.63**: 2 of 3 FAIL |
| e11 | AUTO on the 40 Hz square, A and C, 3 entries, 15 s | DC, <= 1 change, same part | **PASS**: DC in all 6 entries, 0 changes (A 33, C 38 samples each) |
| hand | 3 taps on V on the square, A then C, screenshots | DC, no AC flash | **PASS**: A twice, C three times: AUTO DC 4.763-4.764 V with both parts shown (~4.760 / =4.763), no AC at any poll (0.3 s) nor in the 2 s / 15 s screenshots (`h29-A-15s.png`, `h29-C-2s.png`, `h29-C-15s.png`). See 3 for a lost first tap |
| e10 | AUTO, square -> 5.0 / 5.5 / 4.5 V | AUTO,DC in rule, one range | **PASS**: square AUTO,DC 4.765 (60 V); then -0.00 / -0.29 / -0.59 mV, 6 V only, 9.2-9.3 rd/s, acsrc 0 in 80 of 80 |
| e9 | 4.0 / 4.5 / 5.0 / 5.5 V, AUTO range | 1 range, >= 6 rd/s, AC < 30 mV, DC in rule | **PASS**: 6 V only, 9.23-9.25 rd/s, AC max 0.28-0.35 mV, DC -0.16 / -0.26 / +0.07 / -0.04, acsrc 0 in 119 of 119 |
| e5 | C re-entered every 60 s, 3 min per level | 17 / 2.0 | **PASS**. 24 V on 60 V (gain from e8, gspan 39 131): +1.51 / -0.22 / -1.93 / -1.85. 3 V on 6 V (gain from e8, gspan 83 158): -0.01 / -0.07 / +0.02 / -0.24 |

### 1. 60 V: fixed

The two-point gain removes the 2563fe1 failure. e8s at 1 / 0 V reads +1.8 / +1.9 mV (was -7.2 / -8.0 mV), and e8up (entered at 0 V on the table's gain) reads +1.2 mV at 24 V. The two-point gains on 60 V over this session are 4.84943e-4 (10.3 k span, e8up 5 V), 4.85152e-4 (39.1 k), 4.85198e-4 (24.7 k), 4.85225e-4 (39.1 k). The 2563fe1 fit implied ~4.8516e-4. The internal zero after the power cycle gave 4.85281e-4 (+0.025 % against the fit; 2563fe1 +0.030 %). The anchor itself is 1-4 mV off READ? on 60 V here (e8s 24 V +2.5, e8 24 V +3.6, e8up 0 V -2.2 mV; 1-4 digits). That is inside the rule, and of the same family as section 2.

### 2. 6 V: the two-point gain over a short span is worse than the internal zero's

On 6 V the internal zero's gain was right (2563fe1: within 0.44 mV from 5 V to 0 V). §10.9 replaces it with the first two-point gain of >= 10 k codes, even when that span is smaller than the internal zero's. In e8s and in the three `tc6` repeats, the 5 -> 3 V step (41.6 k codes) gave a gain 143-195 ppm above that power-on's internal zero, and C then read 0 V 0.25-0.92 mV low (limit 0.5). The 83 k span of e8 (5 -> 1 V) gave the internal zero's gain to 2 ppm.

Mechanism, from `tc6`'s STAT? trace at ~10 Hz: after the 5 -> 3 V step the steady window fills by 1.75 s (step 0: the DC V reference, 3 results), and the run row's ST_MEAS ends at ~3.6 s. At that moment the LPF code is still **+5..+12 codes** above the value it reaches 8 s later. It creeps down by 0.25-0.55 mV over ~8 s. On a step down, an m2 that is still high makes |m2 - m1| too small and per_code too large. The same m2 makes the new anchor's offset read low afterwards (the 3 V anchor reads -0.12..-0.26 mV right after). Both signs match. `dctail.py` runs the same step on DC V (AD1, FETCh? at 10 Hz): it settles within 0.1 mV in ~1 s, so the tail is in the AD2 / LPF path, not the calibrator. 10 codes over 41.6 k is 240 ppm, 10 codes over 83 k 120 ppm, and over 104 k (the internal zero) none, since the zero and the 5 V anchor are both measured at rest.

Candidate fixes (none made; the owner decides): (a) keep a gain whose span is larger (the internal zero's included) unless the new span is >= 1.5 x it, so the 6 V internal zero stands and 60 V still takes the two-point gain. On 60 V a 1.5 x rule would refuse 24 -> 12 V (24.7 k < 74 k), so the AGND bias would come back for e8s. So the rule would need per-range knowledge, which §10.9 rejected; (b) take m2 only once the LPF has stopped drifting (a slope test over the steady window, e.g. < 1 code/s; costs up to ~8 s more per first level change); (c) a larger CAL_GAIN_CODES on 6 V. (b) is the one that removes the cause.

### 3. Other observations

- **The first V tap after `CONF:VOLT:DC` is sometimes lost** on the square: the first tap was lost in 5 of 6 tries with C and 1 of 3 with A (`tap29.py`: FUNC? still "VOLT" after it). It did not reproduce at 1 V DC with A (4 of 4 fine). It is not specific to C. A guess, not checked: the DC V autorange on the square 3 s after the CONF. Not chased.
- `SYST:ERR?` had `-100,"Data stale"` once after `hand29.py` (a READ? right after CONF:VOLT:DC), and `-100,"Illegal parameter value"` from a `RATE F` of mine in `dctail.py` (not a valid parameter; RATE stayed SLOW). Both cleared; the queue was empty at the end.
- The 60 V internal zero after power-on: 4.85281e-4 (2563fe1 4.85309e-4); the 6 V one over 4 power cycles 4.80798-4.80873e-5 (+-80 ppm).

### State at the end

- **Device:** DMM page, DC V, AUTO+ (`VOLT:RANG:AUTO?` 1), rate SLOW, `CONF?` "VOLT +6,+1E-05", `VOLT:COUP?` DC, **method A** (`DBG:ACDC:METH?` A), as found (`pre.png`, `end.png`). `SYST:ERR?` empty. Generator `OUTP?` 0 (never switched on). Settings not touched (volume 0, backlight 10 %, Dim / Shutdown Never).
- **Files:** none created on the meter (screenshots were `DBG:LCD?` reads, so nothing was deleted). `MMEM:CAT?` file list identical to the start (3 534 848 / 13 205 504).
- **Health:** `DBG:BOOT?` NORMAL, crash count 0, VALID, HEALTHY. Four deliberate `dm40dbg.py reset`s (09:02 before e8s; 09:33 / 09:34 / 09:35 before each `tc6` repeat), no crash.
- **Calibrator:** 1 V DC, enabled.

## ceb5ca5: the settle-fix retest (TODO items of "bab5dd4" B4 / B5, acdc-stock-vs-opendm40.md §12) (2026-10-23 09:44 - 10:35)

The owner had installed ceb5ca5 (install OK, crash count 0). It carries the settle-fix branch (f4925f5 `vet_ad1` / `reading_pending` / `acs_flat` / input-step settle, 7e10110 no DC-phase step down during an input-step settle). `DBG:BOOT?` NORMAL, crash count 0, VALID, CLEAN at the start and the end; no reset.
- **Wiring:** the calibrator (`/dev/ttyACM1`) on V/COM: 1 V DC (items 1-3), the 40 Hz square (4, 6), DC 4..7 V (5). The A jack, the MCX probe and the generator output were not wired.
- **Files:** in the session scratchpad `rtsf/` (not in git). The bab5dd4 scripts (`dc781.py` with CALC:AVER:MIN? and the range as a number added, `sq5.py`, `ar6.py`, `ar6b.py`, `tl.py`, `sr.sh`) and new ones: `dcmf.py` (`dcpart.py` with the MF path only), `dcpart.py` (SCPI + MF), `dcph.py` (a range change at 0-0.6 s into a DC phase), `nan3.py` (FETCh? / READ? / SYST:ERR? through the gap), `calstep2.py` (the calibrator's level change on DC V FAST), `tap6.py` / `tap6z.py` / `tapshot.py` / `restl.py` (item 6). JSON / logs with the same names; `pre.png`, `end.png`, `ts-*.png`, `cat0.txt` / `cat1.txt`.

### Summary

**Four of five settle-fix checks pass; the DC part guard fails.** The 0.52 V DC part of bab5dd4 came back in 3 of 116 AC+DC range changes (all 6 -> 60 V). In one of them READ? gave 0.524 V three times as the first fresh readings after the NaN gap. The lost V tap of c6f9670 is the AC hint toast taking the gesture.

| # | Check | Result | Evidence |
|---|---|---|---|
| 1 | `dc781.py`: DC V, 1 V, AUTO ON from 600 V x20, 60 V x10, 1000 V x10 | **PASS**: no READ? below 0.99 V on 6 V (216 readings, lowest 0.99890 V). CALC:AVER:MIN? >= 0.99670 V, except 0.98396 V once (1000 V #0): the 1000 V range's own conversion at 0.31 s (it read 0.984-1.021 V, as on bab5dd4). First 6 V conversion read at 0.88-0.92 s from 600 V (bab5dd4 0.81 s), 0.61 s from 60 V (0.56), 1.22 s from 1000 V (1.06): +0.05..+0.16 s | `dc781.json` |
| 2 | `dcmf.py` MF 6 <-> 60 V x10 in AC+DC and AUTO, `DBG:ACDC:STAT?` at 25 Hz; more AC+DC changes in `dcpart.py`, `dcph.py`, `nan3.py` | **FAIL**: DC part 0.5224 V (MF -> 60 #3) and 0.5212 V (`dcpart.py` SCPI -> 60 #1); READ? 0.5244 / 0.5243 / 0.5240 V (`nan3.py` SCPI -> 60, rep 2). AC+DC: 3 of 116 changes 6 <-> 60 V, all on 6 -> 60 V (MF 1 of 44, SCPI 2 of 24, `dcph.py` 0 of 48); AUTO 0 of 20 | `dcmf-*.json`, `dcpart-ACDC.json`, `dcph-*.json`, `nan3.json` |
| 3 | READ? / FETCh? / SYST:ERR? after `VOLT:ACDC:RANG 60` / `6` and the MF page (`nan3.py`, x2 each); sigrok `-c range=60V` / `6V` / `60V --samples 5` in AC+DC (`sr.sh`) | **PASS**: FETCh? +9.91E37 and READ? +9.91E37 with -100,"Data stale" every 0.85 s until the first fresh reading; never the old reading. sigrok rc 0 each, 999.7-1000 mV, CONF? follows, `/dev/usbtmc2` stays. sigrok leaves "Data stale" in the queue | `nan3.log`, `sigrok.log` |
| 4 | `sq5.py`: 40 Hz square, AC+DC, AUTO -> 60, 60 -> 600, 600 -> 60, 60 -> 6, 6 -> 60 V | **PASS**: the square read 6.7328 V on 60 V this time (AC 4.759 V; 6.746 V on bab5dd4: the calibrator). Every fresh READ? within 0.012 % of the run's level after 600 -> 60 and 6 -> 60 V. On 600 V the spread was 0.075 % (6.7219 V, one 10 mV digit low). Gaps 4.2 / 4.3 / 5.4 s (600 -> 60, 6 -> 60, 60 -> 600); 6 V: no reading in 10 s (OL, expected). The 1 V DC changes (`nan3.py`): gap 3.3-5.0 s after SCPI, 3.4-4.2 s after the tap | `sq5.json` |
| 5 | `ar6.py` / `ar6b.py`: DC 4..7 V, AC+DC AUTO range | **PASS on its checks**: 6.3 -> 5.8 V one range step, 5.8 -> 5.0 V none. Every step across 6.0 V moved the range once (5.8 -> 6.3, 4.0 -> 6.5, 6.5 -> 5.0, 6.3 -> 5.8). On the same-range steps down READ? was on the new level at 0.83 s (6.3 -> 5.8, 5.5 -> 4.0, 6.5 -> 5.0; bab5dd4: 6.737 / 5.679 / 5.957 V there for 2-5 s). **Not clean:** 4.0 -> 5.8 V went 6 -> 60 -> 6 V and 7.0 -> 6.3 V went 60 -> 6 -> 60 V. The calibrator dips for ~0.2 s on every level change (below) | `ar6.json`, `ar6b.json`, `calstep2.json` |
| 6 | First V tap after `CONF:VOLT:DC`, square, method A | **Cause found**: the tap lands on the "AC voltage here: use AC V?" hint toast, which takes it. Lost 14 of 30 with the tap 2.8-4.1 s after the CONF, 0 of 30 at 0.5-2.75 s, 0 of 6 at 5 s | `tap6-sq.json`, `tap6z-sq.json`, `ts-sheet.png` |

### 1. DC V AUTO from 600 / 1000 V

`VOLT:RANG?` read right after each READ? already shows the range the next conversion uses. So in each try the first READ? (0.27-0.31 s) is the start range's conversion that triggered the step, and the 6 V conversions start with the 3rd READ? from 600 V, the 2nd from 60 V and the 4th from 1000 V. The bab5dd4 `dc781.json` kept only 10 characters of `VOLT:RANG?`, which cannot tell 6, 60 and 600 V apart, so the comparison above uses reading times. That is why the TODO's "+0.1-0.3 s" is +0.05 s from 60 V. Nothing low on 6 V. The averaging restarts make the first 6 V readings 0.9990-0.9997 V, then 0.9991-0.9996 V.

### 2. AC+DC: the DC part of 0.52 V after a range change, again

`DBG:ACDC:STAT?` field 6 is `dc_val`, the DC part. `dc_out` sets it only for a vetted result (`vet_ad1` true), so 0.52 V there means a vetted result. Both STAT-traced hits have the same timeline (t from the command; MF: from the page key, the item tap at ~1.3 s):

| | change landed | that DC phase | AC phase | next DC phase | 0.52 V | corrected |
|---|---|---|---|---|---|---|
| `dcpart` SCPI -> 60 #1 | in a DC phase (dc_val kept 0.99963) | ends at 0.48 s, no vetted result | 0.48-1.66 s, no readings | 2.27 s 0.99919 V | 2.57 s 0.52122 V, at the switch to AC; 5 readings on it (n_rd 5244-5248) | 4.32 s |
| `dcmf` MF -> 60 #3 | in a DC phase (dc_val kept 0.99923) | ends at 1.70 s, no vetted result | 1.70-2.77 s | 3.39 s 0.99942 V | 3.63 s 0.52245 V; 4 readings | 5.33 s |

A change that lands in the AC phase goes mode 0 -> 1 in `set()`, so `reset_values` clears `dc_val` to 0 (the `dc 0.0` rows: expected, until the first DC result). That case gave 0 hits in 68 changes (`dcmf` AC+DC and AUTO, `dcpart`). A change that lands in a DC phase leaves the mode alone: no reset, `acdc_seen` keeps bit 2 and `dc_val` keeps the old range's part. If that DC phase's next result is held, the phase ends at once ("a held result ... one more DC result" applies only without bit 2). All three such cases on record show it: the two hits and `dcpart` SCPI -> 60 #7, which then came out right. In `dcph.py`, 48 changes placed 0-0.6 s after an AC -> DC switch each got a vetted result ~0.5 s later and stayed right. So the trigger is narrower than "in a DC phase", probably near its end.

Why the guard lets 0.52 V through: a conversion counts when it agrees with the one before it (0.5 % or 4000 codes), or once VET_HELD_MAX (4) are held. A good result 0.3 s before the bad one cannot agree with it. So either the conversion just before the 0.52 V one was also ~0.52 V (held), or the vetting had ended (5 holds, or 8 vetted). All five hits since bab5dd4 are 0.521-0.528 V (52-53 %). That is a repeatable state lasting at least two conversions, not the 25-51 % scatter of one straddled register write (8087c7d). The fast logger cannot watch it: `LOG:SOUR AD1` reprograms the converter. A DBG view of `vet_left` / `vet_held` / the last DC-phase AD1 codes and R22 would decide. Not fixed.

In `nan3.py` the hit was the run right after an MF -> 6 V change, and READ? answered 0.5244 / 0.5243 / 0.5240 V at 3.28 / 3.38 / 3.50 s. With the NaN rule the bad DC part is now the first value a host gets after a change.

### 3. READ? / FETCh? during the gap

As the TODO item wants. The MF path's first query came 1.33 s after the page key (the script's sleeps), and was already NaN.

### 4. The square

The calibrator's square was 6.7328 V total this session (AUTO -> 60 V steady), against 6.746 V on bab5dd4. The 0.1 % check is against the run's own level.

### 5. AC+DC autorange and the calibrator's level change

`calstep2.py` (DC V, manual 60 V, RATE FAST, FETCh? polled) shows that **every MR9270 level change first dips to 2.2-3.9 V for ~0.2 s**. It is within 1 % of the new level by 0.29-0.36 s. Examples: 5.8 -> 6.3 V: 3.31, 3.44, 6.17, 6.29; 4.0 -> 5.8 V: 2.17, 2.95, 5.67, 5.79; 6.3 -> 5.8 V: 5.06, 3.84, 4.82, 5.79. The bab5dd4 `calstep.py` read at SLOW and missed it. The 2.3-5.3 V READ?s right after the ar6 steps (5.27 / 3.86 on 5.8 -> 6.3, 3.44 / 3.02 on 5.8 -> 5.5, 2.31 on 4.0 -> 6.5, 4.05 on 7.0 -> 6.3) are this real dip, not the meter. On bab5dd4 they were 3.3-3.4 V too.

With the dip, the input step is "down ~2-3 V, then up to the new level". Two steps moved the range twice:
- **4.0 -> 5.8 V (`ar6b`):** the AC phase took 2.4224 and then 2.5284 V as the AC part (STAT ac, n_rd rising: readings published). ACS_JUMP should have discarded them (> 1.5 x 0.006 V + 0.03 V) but did not. hypot(2.53, 5.8) > 6 V, so the range went to 60 V at 1.04 s and back at 6.35 s.
- **7.0 -> 6.3 V (`ar6`):** 60 -> 6 V at ~0.9 s (a DC result in the dip, below the step-down threshold), then back to 60 V (6.3 V is OL on 6 V); first reading at 4.99 s.

One excursion each, no hunting. New TODO item.

### 6. The lost first V tap

`tap6.py` / `tap6z.py`: `CONF:VOLT:DC` (from AC coupling, so the function restarts), wait 0.5-5 s, `DBG:TAP 35,297`, then `VOLT:COUP?` every 0.1 s for 1.5 s. When the tap worked, the coupling had already changed at the first poll. A lost tap never advanced later.

| tap after the CONF | lost |
|---|---|
| 0.5 / 1.0 / 1.5 / 2.0 s | 0 of 6 each |
| 2.5 / 2.7 s | 0 of 3 each |
| 2.8-3.5 s (3 each, 3.0 s 6 more) | 13 of 27 |
| 4.0 s | 1 of 3 |
| 5.0 s | 0 of 6 |

`VOLT:RANG?` was +6 before every tap, so no autorange step coincides. `tapshot.py` screenshots at 1.5 / 2.6 / 3.0 / 3.4 / 4.5 s after the CONF show the grey hint toast "AC voltage here: use AC V?" over the value at 2.6 s and 3.4 s, and not at 1.5 / 3.0 / 4.5 s (`ts-sheet.png`). That is the sentinel's AC? hint (HINT_ASSERT_MS 2 s) shown for TOAST_MS 1.5 s. app.rs gives an overlay every gesture ("a toast just goes"), so a tap during the toast only dismisses it. The window fits 2.6-4.1 s. The hint does not fire at 1 V DC, which is why the tap was never lost there. c6f9670's `tap29.py` tapped at 3 s, in the window, which explains its 5 of 6 (C) and 1 of 3 (A). The CONF? resolution was 1E-04 just before 10 of 11 lost taps (the value redraw without the extra digit while the hint shows), against mostly 1E-05 for taps that worked in the same window. Not fixed: it is a UX decision (stock-like swallow, or let a tap outside the toast reach the page).

### State at the end

- **Device:** DMM page, DC V, AUTO+ (`VOLT:RANG:AUTO?` 1), rate SLOW, `CONF?` "VOLT +6,+1E-05", `VOLT:COUP?` DC, **method A** (`DBG:ACDC:METH?` A), READ? 0.99953 V (`end.png`). LOG unchanged (AD1 / 4800 / HOST / STAT 0). Generator `OUTP?` 0 (never switched on). RATE was FAST only during `calstep2.py`, then set back to SLOW. Settings not touched (volume 0, backlight 10 %, Dim / Shutdown Never). `SYST:ERR?` empty.
- **Files:** nothing created on the meter (screenshots were GRAM reads). `MMEM:CAT?` file list identical to the start (3 534 848 / 13 205 504).
- **Health:** `DBG:BOOT?` NORMAL, crash count 0, VALID, CLEAN; no reset, no crash.
- **Calibrator:** 1 V DC, enabled.

## ad97fd7 / c8c2f02 / f42a09e: the AC+DC 0.52 V DC part traced and fixed, ar6 excursions, pass-through toasts (2026-10-23 11:05 - 12:40)

Three images in a row (owner-installed, each md5 OK, crash count 0): ad97fd7 (the trace `DBG:ACDC:TRACE` only), c8c2f02 (fixes), f42a09e (one more autorange fix, rebased on 8844316). Owner settings untouched (volume 0, backlight 10 %, Dim / Shutdown Never). Calibrator on V / COM (`/dev/ttyACM1`), 1 V DC except in the ar6 / toast runs, back to 1 V DC after each. Generator output never on.
- **Files** (session scratchpad `dcp52/`, not in git): `s/trdec.py` (the trace decoder), `dev/hunt.py` (an AC+DC 6 <-> 60 V change aimed a set delay after the AC -> DC switch, SCPI or MF; trace armed, dumped on a trigger), `dev/ar6t.py` (ar6b with the trace re-armed per level), `dev/toast.py`; results `hunt-<run>.json`, `hunt-s1-<i>.txt` (traces of the hits), `ar6t-<run>-<i>-<level>.txt`, `toast.json`, `fixed-trace.txt`.

### Summary

| # | Check | Image | Result |
|---|---|---|---|
| 1 | Trace of the 0.52 V DC part (`hunt.py s1` SCPI, 40 changes, 0.55-1.05 s into a DC phase) | ad97fd7 | **Cause found.** 5 hits (4 of 7 at 0.75 s on 6 -> 60 V: 0.5223 / 0.5162 / 0.5221 / 0.5194 V; once 60 -> 6 V at 0.65 s: 0.4025 V), all 5 traced, all one mechanism (below) |
| 2 | The same aimed changes after the fix | c8c2f02 | **PASS**: 0 of 80 SCPI (40 of them 6 -> 60 V, 30 at 0.75 s) + 0 of 30 MF. DC part 0.99842..1.00036 V over 6168 STAT samples |
| 2b | Again on the final image | f42a09e | **PASS**: 0 of 80 SCPI (48 at 0.75 s, both directions) + 0 of 30 MF (0.65 / 0.75 / 0.85 s). DC part 0.99850..1.00086 V |
| 3 | ar6 (DC 4..7 V, AC+DC AUTO range) with the trace | ad97fd7, c8c2f02, f42a09e | ad97fd7: 4.0 -> 5.8 V 6 -> 60 -> 6 V again (traced); c8c2f02: one excursion in 29 steps (a second cause, traced on the rerun); **f42a09e: PASS**, 27 steps, every step across 6.0 V moved the range once, no excursion |
| 4 | Toast pass-through (40 Hz square, method A, `CONF:VOLT:AC` -> `CONF:VOLT:DC`, the "AC voltage here: use AC V?" hint) | c8c2f02 | **PASS**: toast up (read from GRAM) at the tap: V tap outside it advanced the coupling 10 of 10 (the toast went); a tap on the toast dismissed it 10 of 10, coupling unchanged. Before (ceb5ca5): 13 of 27 taps lost in that window |

### 1. The cause of the 0.52 V DC part

`DBG:ACDC:TRACE 2` logs every DC conversion of the AC+DC DC phase with the row read back from the chip (R22 / R24 / R32 and a hash of R20..R35), the AC results, sets, phase switches, row writes and R22 writes. Hit `hunt-s1-8` (6 -> 60 V by SCPI 0.75 s into a DC phase; ms):

```
23302 SET  m1 r1 from m1 r0, vet_left 8, phase_n 0       the change lands at the end of a DC phase
23752 DC   r22 0x15  ad1 59906  0.99912 V  held (no conversion before it on 60 V); acdc_osr writes 0x15
23762 PH   -> AC (one held result is enough: acdc_seen still has the DC bit)
24618..24938 AC  1.04 / 0.76 / 0.55 / 0.40 V, all discarded (the range-change settle)
24948 PH   -> DC, settle 600 ms
25548 DC   ad1 59916  0.99929 V  vetted; agrees with 59906 -> acdc_osr writes R22 = 0x17 (once-flag)
25703 DC   ad1 31333  0.52245 V  155 ms later, not 400: held; disagrees -> acdc_osr writes R22 = 0x15
25808 DC   ad1 31322  0.52227 V  105 ms later, the same 52 %: agrees with 31333 -> vetted -> DC part 0.522 V
25818 PH   -> AC with dc_val 0.52227
```

All 5 hits are this sequence (the 60 -> 6 V one at 40 %: 0x17 write, 240321 / 240251 codes for 596385). The first result of the DC phase is read at the settle deadline; the converter ran through the 600 ms settle, so INTF was already set and the result is up to a conversion old. The R22 write right after it lands in the middle of the next conversion. That conversion and the one after the 0x15 write that the disagreement causes both read ~52 %, and they agree with each other, which `vet_ad1` accepts. Why only a change in a DC phase: there the phase ends after one held result, so the once-flag write comes with the first result of the *next* DC phase (read late); a change in the AC phase starts a DC phase that runs on until a vetted result, and the 0x17 write comes after a result read on time. In steady AC+DC the flag stays set and nothing is written (as the stock).

**Fix (c08b0d5):** no R22 write in the AC+DC DC phase (the `acdc_osr` once-flag rule removed; the row's 0x15 stays, as the stock's steady AC+DC). `fixed-trace.txt` (f42a09e): no R22 write, chip R22 0x15 in every DC conversion after a 6 -> 60 V change, the DC part 0.9994..0.9998 V.

### 3. ar6: autorange on the calibrator's level change

ad97fd7, 4.0 -> 5.8 V landing in an AC phase: the AC results rose 0.0795 (a jump: input-step settle starts) -> 1.166 -> 2.384 V as the RMS window filled; the old "rose: flat" rule counted two rises as flat, the settle ended on 2.384 V, and the next DC result stepped up on hypot(2.384, 5.798) = 6.27 V (6 -> 60 V at 0.88 s, back at 5.9 s). Fixed in c08b0d5: a rise is flat only within ACS_STEP (0.1 %).

The 7.0 -> 6.3 V excursion of ceb5ca5 (a DC result in the dip stepped 60 -> 6 V) did not recur in these runs; c08b0d5 also makes AC+DC step down only from the DC phase, once its results asked for it for ACDC_DOWN_MS (400 ms) in a row. Cost, measured: 60 -> 6 V after 6.3 -> 4.0 / 5.8 V and 6.5 -> 5.0 V came at 4.0-5.5 s on f42a09e (ceb5ca5 / ad97fd7: 1.8-3.7 s). Steps up unchanged (0.3-1.8 s).

c8c2f02, one more excursion (4.0 -> 5.8 V, 6 -> 60 V at 1.12 s, back at 8.86 s, `ar6t-f1`); the rerun (`ar6t-f2-03`) traced its cause: the 5.8 V step came ~4.6 s after the 60 -> 6 V step of the 4.0 V level, so that step's range-change settle still ran, and ACS_MAX_MS (5 s) ended it on the step's rising result (0.84 V there; 2.4 V in the excursion). Fixed in f42a09e: a result above ACS_JUMP x the previous one when the limit runs out restarts the settle as an input step. f42a09e `ar6t-g1` (27 steps incl. four 6.3 -> 4.0 -> 5.8 V with the step-down 4.8-5.5 s before the 5.8 V step): the step's 2.40 / 2.00 / 1.05 / 0.79 V AC results discarded, no excursion.

### 4. Toasts

Owner's decision: a toast is not modal (77ae236). In `toast.py` the toast's red box was checked in GRAM right before and 0.3 s after the tap. Toast up at 2.95 / 3.25 s after `CONF:VOLT:DC`: tap on V (35, 297) -> coupling DC -> AC 10 of 10 and the toast gone; tap on the toast (240, 160) -> toast gone, coupling DC 10 of 10. At 2.65 s the toast was not up yet (it came after the tap): the V taps advanced, the taps at (240, 160) hit the page (no button there).

### Emulator / host

On f42a09e: couptest, acdcmethtest, autorangetest, senttest, modetest --fast (fw-rs 18 of 18; c8c2f02's both-firmware run: the stock's cap-big 5 % high, not fw-rs), all `tools/*/run.sh` (ui-render 0 violations, touch-test 892 cases, meter-test 90), emu/hy3131 tests (501) pass. powtest: 27 of 29 in the parallel run (ereset: Yes 13 s after the long press; acdc-floor: Q 5.99), both OK when rerun alone (five emulators plus modetest on the host: timing). Budgets f42a09e: region B 57.8 KB, C 17.0 KB free, stack headroom 21.4 KB, SRAM 53.1 KB free (the trace ring: 5.4 KB).

### 5. Method C 6 V retest of acdc-approaches.md 10.13 (on f42a09e, which contains 8844316)

As 10.13 lists it, `tools/acdcmeth.py` from the worktree, log `dcp52/dev/acdc-lw.jsonl` / `c613.log`: reset, e8up; 4 x (reset, e8s); e8; e5 --minutes 3.
- **e8up** (60 V entered at 0 V): 0 / 1 / 5 / 12 / 24 V: +0.74 / -0.09 / +0.62 / +0.80 / +0.91 mV (gsrc 0, 0, 2, 2, 2). PASS.
- **e8s x 4**, 6 V at 0 V: **-0.25 / +0.18 / -0.59 / +0.21 mV**: 3 of 4 within 0.5 mV. The third: on 6 V the 3 V level took a second reference judged biased (gsrc 3, gdis +8.95 codes), then the 1 V level another (gsrc 2, gdis -4.47), and 1 / 0 V read -0.46 / -0.59 mV. The other three kept the internal zero's gain (gsrc 1) at every 6 V level. 60 V all within 5.5 / 5.0 mV (0 V: +0.03 / +0.06 / +2.78 / -0.75 mV).
- **e8**: 6 V 5 / 3 / 1 / 0 V +0.22 / +0.26 / +0.07 / +0.16 mV (gsrc 1); 60 V 24 V +3.19 mV, 0 V +0.88 mV. PASS.
- **e5** (3 min): 60 V at 24 V -1.23..+1.70 mV, 6 V at 3 V -0.36..+0.24 mV. PASS.
Decision rule of 10.13 (all four e8s) not met: 1 of 4 at -0.59 mV. Not chased here (the method-C agent's item).

### State at the end

- **Device:** f42a09e, DMM page, DC V AUTO, `CONF?` "VOLT +6,+1E-05", method A, `DBG:ACDC:TRACE?` off, READ? 0.99939 V, generator `OUTP?` 0, `SYST:ERR?` empty. `DBG:BOOT?` NORMAL, crash count 0, CLEAN, VALID. Five deliberate resets (`dm40dbg.py reset`, 10.13); no crash. Settings not touched; nothing written to the meter's disk.
- **Calibrator:** 1 V DC, enabled.


## acdc-bias: method C's bias test from device data (f42a09e -> d0baca5 -> b94bfb8, 2026-10-23 12:35 - 13:50)

Images: f42a09e (data before the fix), d0baca5 and b94bfb8 (owner-installed, md5 OK, crash count 0). Owner
settings untouched (volume 0, backlight 10 %, Dim / Shutdown Never). Calibrator on V / COM (`/dev/ttyACM1`), at
1 V DC after every run. Generator output never on. 17 deliberate resets (`dm40dbg.py reset`), no crash.
- **Files** (session scratchpad `cbias/dev/`, not in git):
  - `pre.jsonl` / `pre_e8s.log`: f42a09e, e8up + 6 x e8s, `--trace`
  - `post.jsonl` / `post.log`: d0baca5, the full 10.13 set
  - `conf.jsonl` / `conf.log`: b94bfb8, e8up + 3 x e8s
  - `ana.py` / `ana2.py` / `ana3.py`: per-level tables from the traces
- Analysis and rule: acdc-approaches.md 10.14; the result tables: 10.15.

### 1. f42a09e: gdis and the LPF before / after the reference

Per e8s session (reset before each), gdis in codes (the two-point gain against the gain in use, at the new
point), the window's residual rms (`ana.py`'s offline line fit over the 3 s before the reference), and the 6 V
0 V error:

| run | 60 V 12 V | 60 V 5 V | 6 V 3 V | 6 V 1 V | 6 V 0 V (mV) |
|---|---|---|---|---|---|
| 1 | -2.5 (iz kept) | -2.5 (iz kept) | +7.6 (rms 2.4) | +2.8 | +0.13 |
| 2 | -3.4 (iz kept) | -6.9 -> gsrc 3 | **+13.0 -> gsrc 3** (rms 2.0) | -19.4 vs that gain -> gsrc 2 | +0.04 |
| 3 | -10.1 -> gsrc 3 | +8.2 vs that -> gsrc 2 | +7.6 | +0.7 | +0.02 |
| 4 | -6.5 -> gsrc 3 | | +3.9 | +3.3 | +0.08 |
| 5 | -9.0 -> gsrc 3 | | +5.1 | +3.6 | +0.21 |
| 6 | -9.3 -> gsrc 3 | | +8.0 | +2.0 | +0.06 |

e8up: 0 / 1 / 5 / 12 / 24 V -3.34 / -2.13 / +0.17 / +1.40 / +1.22 mV (rule 5.0 / 5.5 / 7.5 / 11 / 17). The window's
residual rms: 60 V 0.7-1.6 codes, 6 V 1.4-2.9.

`ana2.py`: the 6 V 3 V level's LPF against the code the level's own DC V READ? gives through the internal
zero's line. It is +3..+12 codes for 2-5 s after the step (flat), then lower after the reference. The 1 V level
mostly shows -3..+1 before its reference. The lag is in the 3 V point; why it is smaller at 1 V is open
(acdc-approaches.md 10.14).

### 2. d0baca5 and b94bfb8

Tables in acdc-approaches.md 10.15. **PASS** on both images: 6 V 0 V within 0.5 mV in 9 of 9 e8s sessions
(-0.34..+0.09 mV), gsrc 1 throughout on 6 V; 60 V's biased zero replaced in every session (gsrc 3); e8up, e8,
e5, e11, e10, e9 pass on d0baca5.

### State at the end

- **Device:** b94bfb8, DMM page, DC V AUTO, `CONF?` "VOLT +6,+1E-04", method A (STAT? meth 0), `VOLT:COUP?` DC,
  READ? 1.00052 V, generator `OUTP?` 0, `SYST:ERR?` "No error". `DBG:BOOT?` NORMAL, crash count 0, CLEAN, VALID,
  HEALTHY. Settings not touched; nothing written to the meter's disk.
- **Calibrator:** 1 V DC, enabled.

## d21b588 host tools on b94bfb8: Ctrl-C, -d / -n, the desync repair, the 24 V cap (2026-10-23 15:48 - 16:00)

Host-only retest of ff27c69..d21b588 (TODO "dm40scpi.py `log` stopped by SIGINT", "`log --source ad2 -d N`",
"acdcmeth.py: the calibrator's 30 V"). The meter ran b94bfb8; no flashing. Files: the scratchpad's `hdc/run/`.

| # | check | result |
|---|---|---|
| 1 | `log x.csv --source ad2 --rate 38400 -d 60`, SIGINT (kill -INT) after 5 s, 5 runs | **PASS** 5 / 5: exit 0, `# stopped_by,ctrl-c (after 4.97 s)` and the full trailer, 167-173 k rows, lost 0; `info` right after answers, no "empty USBTMC reply" note, LOG:STAT? 0 |
| 2 | `-d 10` at 38400 | **PASS**: `# stopped_by,duration (after 10.028 s)`, 360 637 rows, lost 0, wall 10.07 s |
| 2 | `-n 5000` at 38400 | **PASS**: exactly 5000 rows, `# stopped_by,count (after 0.758 s)` |
| 3 | desync made with the old tool (0100788), then the new `info` | **PASS** (by the DBG:LCD? route, below): `*IDN?` read b'' (raw os.read on /dev/usbtmc2, 3 queries all empty); new `dm40scpi.py info` printed `dm40: empty USBTMC reply to *IDN?: device cleared, asked again` and the right IDN / FUNC / CONF; afterwards raw `*IDN?`, LOG:STAT? 0, SYST:ERR? "No error", FUNC? all correct |
| 3+ | the same cut against the new dm40dbg.Link | 3 / 3 in step (`*IDN?` and LOG:STAT? right after) |
| 4 | mr9270 `set_output('voltage', 30)` | **PASS**: stderr `mr9270: 30 V requested, the calibrator gives at most 24 V: set 24 V`, 24000 written and read back, the meter read 23.985 V; then 1 V |
| 4 | `acdcmeth.py e6 --no-cal` | **not run**: the session's permission check denied it. Offline instead: `settle_time` on a level without readings returns 4 NaNs (e6 unpacks them); the 0100788 version returns 3 (ValueError) |

Notes:

- **The TODO recipe for check 3 does not work on the real meter.** During `log --source ad2 --rate 38400` the
  wchan never read `usbtmc_generic_read`: 15.7 M polls in 20 s (Python pread on /proc/PID/wchan), a 3 s sample
  of 2.9 M: usb_start_wait_urb 2.2 M, running 0.44 M, usb_wait_anchor_empty_timeout 0.21 M, nothing else in the
  driver. The meter sends the rest of a 6006 B LOG:DATA? block before the read starts waiting, so the
  interruptible wait (the only SIGINT cut point) never sleeps. The old tool killed at usb_start_wait_urb (runs
  above, new tool) or at usb_wait_anchor_empty_timeout (old tool, 3 runs) stayed in step. SIGINT 5 s into a
  run also left the old tool's `log` clean (it caught KeyboardInterrupt).
- **What made the desync:** the old 0100788 `dm40dbg.Link.lcdread(0, 0, 320, 48, 15361)` in a loop (a 6136 B
  DBG:LCD? reply, read with a plain os.read), SIGINT the moment wchan = usbtmc_generic_read (hit after 832
  polls). The process died with KeyboardInterrupt inside `os.read`, and every later `*IDN?` read empty, which
  matches the e7ec2cc state. dmesg is not readable here, so the "wrong MsgID / bTag" lines were not checked.
- Runs 6-10 (the kill held back until wchan = usbtmc_generic_read, which never came) ended by `-d 60`
  (60.028 s) or by a SIGINT at 56-60 s: all clean, `info` and LOG:STAT? 0 right after.
- CONF? reads `VOLT +6,+1E-04` after an AD2 log session and `+1E-05` before / after the repair (resolution of
  the current state; nothing set).

### State at the end

- **Device:** b94bfb8, DC V, `VOLT:COUP?` DC, method A, READ? 0.99995 V, LOG:STAT? 0, LOG:SOUR AD1, LOG:SRAT 4800,
  LOG:DEST HOST (OSR 3, CHOP 1 as found), `OUTP?` 0, `SYST:ERR?` "No error". `DBG:BOOT?` NORMAL, crash count 0,
  CLEAN, VALID, HEALTHY. Settings not touched; nothing written to the meter's disk.
- **Calibrator:** 1 V DC, enabled (24 V for about 3 s during check 4).

## b6909d6: the region C / Wave view / fast logger retest (TODO b6909d6 items 1-7) (2026-10-23 16:02 - 17:15)

The owner had installed b6909d6 (5ca0557's fixes). `dm40dbg.py boot --expect dm40-fw-b6909d6.bin`: marker
0x3259CDA6 / 0x00096774 / 0x00011644 = the image header, VALID, crash count 0, HEALTHY, at the start and the end
(no reset). The run was started by one agent (16:02-16:18) and finished by another. Files are in the scratchpad's
`devchk-b690/`: `disk3.py` (a disk session; `--copy` reads an INFO/ file in a loop from state 3 on),
`fwdirect.py` (O_DIRECT reads of [FW_LO, FW_HI) of FW_PATH), `fwcopy.py` (page-cache reads, the kernel's
readahead is 240 KB), `wv1.py`, `inr6.py`, `rec7.py`, `stop.py`, `wdrop.py`, the JSONs and PNGs.

| # | check | result |
|---|---|---|
| 1 | `DBG:PROF?` polled during a 38400 disk session (`p384`, 20 s, 39 polls in state 3) | **PASS**: max_lat 280 ns (`spiflash::cmd_addr`), late 0, overruns 0, dropped 0; the file has 768 003 rows / 20.000 s, lost_before 0 |
| 2 | Wave view exit with MENU at 2 ms/div (38400), 3 runs (`we1..3`) | **PASS**: after the exit, max_lat 300 ns (`dm40_fw::run`), late 0, overruns 0, timeouts 0; the menu appears and the DMM resumes (`we*-menu.png`, `dmm1.png`). At 1 ms/div (76800, `wv1`): late and overruns did not change across the exit, the menu appeared and then the DMM |
| 3 | Host copies INFO/FIRMWARE.BIN during disk sessions | **FAIL** (cause below). 4800 (`c48`, whole file, page cache): max_lat **21.78 us** at `compiler_builtins::memcpy`, late 0, overruns 70, lost_before 0. 76800 (`c768`): max_lat 8.74 us, late 3778, overruns 31 642, ISR dropped 268 500, the file lost_before 288 677 (8584 holes). At 38400: 21.7 us, late 450 (head 0..0x50000 + readahead) to 3727 (tail) |
| 4 | Wave view at 1 ms/div (76800), ACQ:SRAT?, SCOP:MEAS:FREQ?, VPP?, CONF?, MMEM:CAT? every 5 s for 61 s (`wpoll`) | **PASS** for the ISR: max_lat 260 ns, late 0, overruns 0. FREQ? reads 0 on the 40 Hz square (the open FREQ? item). New: each MMEM:CAT? adds 6813 to `dropped` (below) |
| 5 | Wave view entry from DC V at 2 ms/div, 3 runs (`dcv`) | **PASS**: field 6 is 0 in all 18 snapshots (1 s apart) and after the exit, late 0, max_lat 240-260 ns |
| 6 | `LOG:CSV ON` in the Wave view (`wv1`) and on the inrush page (`inr6`, `INR ON`, ARMED) | **PASS**: -100,"Settings conflict: Wave view" / "... : inrush page", `LOG:CSV?` 0,-1,..., no new file in MMEM:CAT? |
| 7a | 76800 session stopped in its trial (`stop.py`, OFF at state 3) | **PASS**: `LOG:DISK?` 5,**-1**,...; no FAST file, used / free unchanged |
| 7b | tpp after a 76800 session | **PASS**: 4800 after 76800 (`c48` after `c768`; `t48` after the stop.py trial with tpp 388 / 393), AD1 after 76800 (`ad1` after `k768`), and the 38400 sessions: tpp 0,0 from the first poll on |
| 7c | Record page, Interval Fast, AC V, LOG:SRAT 76800 | **PASS**: "76800/s, 57 s left" (`rac-fast.png`; 13.2 MB free / 76 800 / 3 B). In DC V, Fast reads "40/s, 30 h left" (`rec-fast.png`). Set back to "Every reading" (LOG:CSV:INT? 0) |

**Item 3, the cause: data reads from region C flash, not code.** FwFile::fill is in region B now, but it copies
the image bytes, and the image from 0x08080000 up lies in region C (not zero-wait). A load from there stalls the bus,
and the ISR cannot preempt it. The emulator's region C hook watches only instruction fetches, so fastlogtest
could not see this. A split test with O_DIRECT reads at 38400, 10 s each:

| read during the session | max_lat | late | overruns | lat_pc |
|---|---|---|---|---|
| FIRMWARE.BIN 0..0x70000 (`hd384`) | 280 ns | 0 | 0 | spiflash |
| FIRMWARE.BIN 0x70000..0x7C000 (`s70`, flash up to 0x0807FFF4) | 280 ns | 0 | 0 | spiflash |
| FIRMWARE.BIN 0x7C000..0x82000 (`s7c`, flash 0x0807FFF4..) | **21.78 us** | 3651 | 12 859 | memcpy |
| FIRMWARE.BIN 0x7C000..0x96000 (`dtail384`) | 21.78 us | 3727 | 12 941 | memcpy; the file lost_before 10 015 |
| CALIB.BIN (0x080C0000) / LOADER.BIN (0x080A0000) / BOOTDATA.BIN (0x080A4000) | 21.76-21.78 us | 2365-2946 | 8.8-10.8 k | `vdisk::mem_read` |
| ROMBOOT.BIN (0x08000000, zero-wait) (`srom`) | 280 ns | 0 | 0 | spiflash |

So any INFO/ file whose source is internal flash at or above 0x08080000 stalls the AD2 ISR by ~21.8 us per read. This
covers the FIRMWARE.BIN tail, LOADER, BOOTDATA and CALIB. In every stalled run the PLL's estimate is dragged to
rate_mhz ~38 210 xxx (ppm_x10 +4995): this is a symptom of the late entries. No software chunking helps: a single
load stalls. The fix is a design call: hold or refuse the host's reads of those sectors while the stream runs
(fast-log session, Wave view, inrush), or document the limit.

**New: `fast_scpi` (LOG:DEST / LOG:DURation, set and query) is region C.** At the start, the meter had sat in the
Wave view at 1 ms/div with late 5, max_lat 5.78 us at lat_pc 0x0808f1f4 = `dm40_fw::fast_scpi`, from LOG:DEST? /
LOG:DUR? queries. Reproduced in `wv1`: idle 60 s gave late 0 and 260 ns; then LOG:DISK?, LOG:DEST?, LOG:DUR? gave late 1 and
2.8 us at fast_scpi. (LOG:DISK? itself goes through `fast_status`, region B.) Fixed in my worktree, commit
0464566 on branch devchk-b690: fast_scpi left .cold. Region B is 51 420 B free. The image is
`devchk-b690/dm40-fw-0464566.bin` (sha256 af0529b0…ed7e), not flashed and not emulator-tested.

**New: MMEM:CAT? in the Wave view at 76800 overflows the view's ring.** `wdrop.py` at 1 ms/div, each query 3 times:
MMEM:CAT? takes 123 ms and adds +6813 `dropped` (~89 ms of samples) every time. CONF?, SCOP:MEAS:VPP? and ACQ:SRAT? add +0, and
1 s idle adds +0. late 0, max_lat 260 ns: the ISR loses nothing, but the view's record has a gap.

### State at the end

- **Device:** b6909d6, DMM page, DC V, AUTO+, RATE SLOW (it was FAST for a few minutes after the Record page
  sequence: a MENU / tap that landed on the DMM value; set back with `RATE SLOW`), READ? 1.0005 V. LOG: AD1 /
  4800 / HOST / DUR 60 / CHOP 1 / OSR 3 / STAT 0; LOG:CSV:INT 0 (Every reading). `OUTP?` 0 throughout.
  `SYST:ERR?` "No error". Settings not touched (volume 0, backlight 10 %, Dim / Shutdown Never).
- **Disk:** MMEM:CAT? names and used / free (3 534 848 / 13 205 504) are the same as at the start. Each session's
  LOGS/FAST000.DLG was deleted by exact name (`del1.py`, which refuses pre-session names).
- **Calibrator:** 1 V DC (a 40 Hz square during the Wave view checks).

## b6909d6: the "DM40 files" volume on Windows 11 (read only) (2026-10-23 17:26 - 17:38)

The meter ran b6909d6. INFO/BACKUP.TXT names the loader record 3259CDA6 / 00096774 / 00011644, the marker in the
section above. The default "Files live" mode was in force (read-only). The host was a libvirt Windows 11 VM
(`win11`). The meter (bus 3, device 127) was passed through with a USB hostdev; the coordinator attached it and
detached it. The guest was driven only with `virsh send-key` and screenshots: HMP mouse clicks did not register.
No SCPI was sent and nothing was written to the meter. Copies went to the guest's Desktop\dm40copy. The
screenshots are in the scratchpad's `win/shots/` (`10`..`37`).

| # | check | result |
|---|---|---|
| 1 | Drive, label, size | **PASS**: AutoPlay opened Explorer on **E: "DM40"** at the attach. It took the focus, so a PowerShell line typed then went to Explorer and moved it into DEBUG: browsing only, nothing opened. Get-Volume: FAT, Removable, Healthy, 2 129 920 000 B, 2 102 427 648 free, 32 KiB clusters. Get-Disk: ALIENTEK "DM40 files" rev 0.2, serial 0001, MBR, **IsReadOnly True**, 1 partition; the USBSTOR id is `VEN_ALIENTEK&PROD_DM40_FILES&REV_0.2`. fsutil: FAT, serial 0xe523e098, "Is ReadOnly" |
| 2 | Folders | **PASS**: root DEBUG, INFO, LOGS, SCOPE, SHOTS, README.TXT (2048 B). `gci -r`: 41 files, all ReadOnly, all dated 13/10/2026 00:00. INFO: CALIB 4096, BOOTDATA 16384, UID 12, OPTBYTES 16, OTP 528, ROMBOOT 16384, LOADER 131072, BACKUP.TXT 1388, FIRMWARE 616320. DEBUG/SPIFLASH.BIN 16 777 216. LOGS/LOG000-004.CSV, SCOPE/SCOP000-009.CSV (384-693 KB), SHOTS/S000-011.BMP (17 KB and 307 266 B) |
| 3 | Open a CSV and the INFO files | **PASS**: LOG000.CSV (PowerShell) and LOG001.CSV (Notepad: 1531 chars, CRLF, UTF-8): the 3 comment lines, header `time_s,value,unit,flags`, rows. SCOP000.CSV: 4 comment lines (2.50 MS/s, 26624 samples), `time_s,volts`. BACKUP.TXT and README.TXT read as text. Format-Hex of UID.BIN (12 B) and OPTBYTES.BIN (16 B) |
| 4 | Copy to the Desktop (Measure-Command, Copy-Item) | **PASS**, sizes exact. DEBUG/SPIFLASH.BIN 16 777 216 B in **48.26 s (~348 KB/s)**. SCOPE/SCOP000.CSV 692 550 B in **8.92 s (~78 KB/s**, generated from the DLG). INFO/FIRMWARE.BIN 616 320 B in **0.74 s (~834 KB/s)**. The copied CSV has 26 629 lines (5 + 26 624 samples) and a complete last row. FIRMWARE.BIN sha256 C7EBD17C…D9AA |
| 5 | FAT16 in the drive properties | **PASS**: Properties shows "File system: FAT" (Windows' name for FAT16), Type USB Drive, used 27 492 352 B, capacity 2 129 920 000 B. Get-Partition: **Type FAT16**, offset 0. Hardware tab: "ALIENTEK DM40 files USB Device", "This device is working properly". Closed with Cancel |
| 6 | Device Manager: the USBTMC interface | **As expected (no in-box driver)**: "DM40 multimeter (dm40-fw)" under **Other devices** with the warning icon. Get-PnpDevice: MI_00 Status Error, **CM_PROB_FAILED_INSTALL (code 28)**, no service. The hardware ids are `USB\VID_1209&PID_0001&REV_0010&MI_00` and `...&MI_00`; compatible ids start `USB\Class_fe&SubClass_03&Prot_01` (USBTMC USB488). The composite device and MI_01 "USB Mass Storage Device" are OK |
| 7 | Safely remove | **PASS with a note**: first the Explorer window on E:\ was closed. The shell's Eject verb then gave "Safe To Remove Hardware: The 'DM40 (E:)' device can now be safely removed". **Note:** the next queries (Get-PnpDevice, Get-Volume, Get-Disk) showed E: mounted again and the disk Online. In the normal app msc.rs only records START STOP UNIT for recovery mode (`STOP_UNIT`, msc.rs:585) and keeps the medium present, so Windows remounts the volume at the next access. Unplugging after the toast is still safe: the volume is read-only. Explorer users will just see the drive come back if anything touches it |

**Not covered here:** a recording or screenshot made while the volume is mounted (does Explorer show the new file
after F5, with the UA sent at once?), "PC can edit" on Windows (delete, copy dm40fw.bin, System Volume Information),
and the AD2 ISR during the copies (no SCPI was sent; the DMM page was not watched). The INFO/FIRMWARE.BIN copy
reads the region C tail; per the section above, that stalls only an AD2 stream that is running.

**Side issue, not the meter:** at 17:12 the VM was found paused with an io-error ("sda: no space"): the host root
filesystem was full. The VM was left paused until the owner freed space, then resumed with `virsh resume`. Windows
came back without problems.

### State at the end

- **Device:** b6909d6. The coordinator detached it from the VM, and it is back on the host as /dev/usbtmc2.
  Nothing was written to its disk, and its settings and mode (Files live) were not touched.
- **VM:** win11 shut off. The guest keeps Desktop\dm40copy (the three copied files).

## 35fce2a: the slow-bank retest (TODO "Region C DATA reads" device retest 1-6, dmm-bugs 5) (2026-10-23 18:36 - 19:00)

The owner had installed 35fce2a (it contains 7dcf3d4, the dmm-bugs fixes 1152624). INFO/FIRMWARE.BIN read before any
session is byte-equal to the 35fce2a build (sha256 6209c7f4…13a9). `dm40dbg.py boot`: marker VALID, crash count 0,
HEALTHY at the start and the end (no reset). Before the sessions every INFO/*.BIN was copied as a reference. Before each
test the page cache was dropped (`posix_fadvise DONTNEED`, `fincore` 0 pages). The source was AC V with the
calibrator's 40 Hz square, AD2. Files are in the scratchpad's `devchk-35fc/`: `disk4.py` (a disk session; `--cp`
runs a real `cp` of INFO/ files from state 3, `--direct` runs O_DIRECT reads of a range in a loop, `--rfiles` runs
buffered reads, `--poll-q` polls queries; dmesg is diffed; the DLG is fetched, decoded and deleted by exact name),
`wv2.py`, `dmm7.py`, `dmm7b.py`, `bt02.py`, the JSONs and PNGs.

| # | check | result |
|---|---|---|
| 1a | 38400 disk session (20 s), `cp` FIRMWARE.BIN, CALIB, BOOTDATA, LOADER, ROMBOOT, UID, OTP, OPTBYTES from state 3 (`cp384`) | **PASS**: max_lat 280 ns (`spiflash::cmd_addr`), late 0, overruns 0, dropped 0; 768 002 rows / 20.000 s, lost_before 0. The FIRMWARE.BIN `cp` started at 4.06 s and returned at 24.43 s; the session left state 3 at 24.1 s. All 8 copies are byte-equal to the references. dmesg: nothing for sda |
| 1b | The same at 76800 (`cp768`) | **PASS**: max_lat 280 ns, late 0, overruns 0, cpu 464 permille; 1 536 004 rows / 20.000 s, lost_before 0. `cp` ran from 8.67 s to 29.84 s; state 5 came at 29.74 s. All copies are equal. dmesg clean |
| 2 | Wave view at 1 ms/div (ACQ:SRAT? 76 800), `cp` FIRMWARE.BIN + CALIB.BIN, MENU at 40 s (`wv2`) | **PASS**: `cp` blocked from 0 to 40.27 s and returned right after MENU (40.1 s). The copies are equal. While the view was open: late 0, overruns 0, max_lat 300 ns (`dm40_fw::run`); none after the exit either. dmesg: nothing for sda |
| 3 | O_DIRECT splits at 38400, 12 s each (`fwdirect.py`) | **PASS** (after the rerun, below): FIRMWARE.BIN 0x7C000..0x82000 (`s7c`), 0x7C000..0x96000 (`stail`), CALIB (`scal`), LOADER (`sldrb`), BOOTDATA (`sbd`): the first read waits for the whole session (11.9-12.4 s) and returns at the end, then reads at once; max_lat 280 ns, late 0, lost_before 0. 0..0x70000 (`sh0b`): 21 reads of 448 KiB, 0.57 s each; 0x70000..0x7C000 (`s70b`): 160 reads, ~60 ms each; both 280 ns, late 0 |
| 4 | UID / OTP / OPTBYTES.BIN, cache-dropped buffered reads in a loop during a 76800 session (15 s) (`r768`) | **PASS**: 867 reads, all byte-equal, each ≤ 175 ms (never refused); max_lat 280 ns, late 0, lost_before 0 |
| 5 | SCOP:MEAS:VPP?, WAV:PRE?, DBG:HWTRIG?, LOG:SCALe?, LOG:DEST?, LOG:DURation?, DBG:ISR? polled in a 38400 disk session, 98 rounds in 20 s (`p5`) | **PASS**: late 0, max_lat 280 ns, lost_before 0. Worst reply times: VPP? 161 ms, WAV:PRE? 107 ms, the others < 1 ms. |
| 5b | BT 00 / 02 (5 each, `bt02.py`) in the Wave view at 1 ms/div (76 800), after the end of the session runs | **PASS for the ISR**: late 0, max_lat 260 ns before and after (dropped 31 came from the view's entry, before BT). **No BT reply** to any of the 10 frames (2 s timeout each). This matches the BT check of 0667175 item 4 (stock frames are not served during a stream, UF-36). After MENU: 00 / 02 answered at once, with the same bytes as in 6 |
| 5+ | **New:** `OUTP?` (the generator's queries) polled in a 38400 disk session (`outp`) | **FAIL**: late 25, overruns 8, **max_lat 21.1 us at lat_pc `Gen::apply`** (region C), lost_before 29 in 10 s. That is one late entry per query. Cause and fix below |
| 6 | Device page, *IDN?, SYST:MOD?, BT 00 / 02 | **PASS**: Device page: Model DM40A, Bootloader V1.1, Software fw-rs 0.0.1, Hardware V4 (`dev-before.png`). *IDN? `Alientek,DM40A,0,dm40-fw 0.1`; SYST:MOD? `DM40A,0,60000`; BT 00 → `05 03 41` ('A'); BT 02 → `28 1a 0b` = 40 / 26 / 11, which is BOOTDATA +0x10..12 = 04 01 01 (genuine-check.md). At the end *IDN?, SYST:MOD? and BT 00 / 02 were queried again: same. The Device page was not reopened |
| 7a | dmm-bugs 5: TEMP with 1 / 5 / 10 V DC and the square on V-COM, 10 s each (`dmm7`) | **PASS**: SYST:ALER? NONE throughout, DBG:SENT? field 12 = 1 (driven) at 5 / 10 V, 0/1 at 1 V and the square |
| 7b | dmm-bugs 5: RES with 1 / 5 / 10 V DC and the square (AUTO, so the 60 MΩ range) | **FAIL**: **A-JACK** at every level (`res-5.png`: red A-JACK, "0L MΩ"); LIVE? never shows, field 12 stays **0 (free)**. FETC? reads OL (+9.9E37) at 5 / 10 V and 50.2 MΩ at 1 V, so it is never negative. `dmm7b`, 5 V on manual 600 Ω .. 60 MΩ: OL and A-JACK on every range, field 12 = 0. On this device the ohm AD1 code does **not** go negative for a positive source above the ~1.4 V open-circuit voltage, so the V_GATE premise (sentinel.rs, "ohm's AD1 code goes negative") fails here, and the gate takes V-COM as free. Not fixed; it needs the owner's call (an OL in ohm alone cannot tell a driven V-COM from the open leads of the A-JACK case). A negative source was not tried |
| 7c | dmm-bugs 5: the comparator's lowest tripping level in RES, 1.0 / 1.5 / 2 / 2.5 / 3 V DC, 6 s each | **Inconclusive**: field 8 (hits) did not grow at 1.0-3.0 V (367 all through), while 5 V had added hits before. One A-JACK showed at the 1.0 V step, carried over from the square just before it. So the PB1 pickup trips between 3 and 5 V: k ≈ 23.75 mV / 3..5 V ≈ 5-8 mV / V, below the ~17 mV / V of doc 4.8's gap |
| 7d | dmm-bugs 1-4 and 6 (AUTO overload, OVLD, a current in A) | **BLOCKED**: the A jack is not wired (no current source) |

**Contaminated runs and the reruns.** The first `sh0`, `s70` and `sldr` runs had late 3 / 5 / 2 and max_lat 17-18 us,
all at lat_pc `Gen::apply`. My scripts send no generator command. At the time an outside USBTMC client (the owner's
ngscopeclient, whose DM40 driver polls `OUTP?`) was running, so these three runs were repeated after it closed:
`sh0b`, `s70b`, `sldrb` all gave late 0 and 280 ns. `outp` then reproduced the stall on purpose.

**Cause of 5+:** `App::scpi` ends every generator command with `gen_set(g)`, queries included. So `OUTP?`,
`SOUR:FREQ?`, `SOUR:FUNC?`, `SOUR:VOLT?` and the others re-apply the generator: `stop_outputs()` and a restart, which
also glitches a running output. `Gen::apply` and gen::plan are region C (link-native.x .cold), so each query during a
stream stalls the ISR ~21 us. Emulator, base 35fce2a, `fastlogtest --sessions ad2:9600 --region-c --host-read
--poll 'OUTP?;SOUR:FREQ?;SOUR:FUNC?;SOUR:VOLT?'`: FAIL, `Gen::apply` (code + interrupted PCs).

**Fix (branch devchk-35fc, not on the device):**
- `gen_set` only for a setting (`reply.is_none()`, as the scope branch does), so queries no longer re-apply.
- `SOUR:FREQ?` reads a cached `out_hz` (set in `Gen::new` / `apply`) instead of `gen::plan::Timing::hz`. The first
  fix alone still ran `Timing::hz` in region C (emulator).
- `Out::duty_pct` is `#[inline(always)]`.

Setting a generator value during a stream still runs `Gen::apply` in region C; that is left as it is (a rare user
action). Emulator on the fix: the poll run above with SOUR:VOLT:OFFS?, SOUR:FUNC:SQU:DCYC? and *IDN? added: PASS,
no region C code and no slow-bank load while streaming, lost 0; `fastlogtest --sessions
ad1:640,ad2:4800,ad2:38400,ad2:76800 --region-c --host-read`: PASS, lost 0, no region C. Image:
`devchk-35fc/dm40-fw-<commit>.bin` in the scratchpad (the commit that adds this section).

### State at the end

- **Device:** 35fce2a, DMM page, DC V AUTO+, RATE SLOW, `VOLT:COUP?` DC, READ? 0.99999 V (`end.png`). LOG: AD1 / 4800
  / HOST / DUR 60 / CHOP 1 / OSR 3 / STAT 0. `OUTP?` 0 throughout. SYST:ALER? NONE, `SYST:ERR?` "No error". Crash count
  0, no reset.
- **Settings:** volume 0, backlight 10 %, Dim / Shutdown Never (seen on the Settings page). **Two settings were
  changed by mistake and then restored.** After the Device page, MENU returned to Settings 3/3, not to the main menu.
  The "Multimeter" tap (140,105) therefore landed on Settings 3/3's **USB disk** row ("PC can edit (re-plug)", asked
  only, `DBG:VDISK:RO?` 1,0), and a later one landed on Settings 2/3's **DMM row** (Bargraph → History). Both were
  set back: `DBG:VDISK:RO 1` (→ 1,1, "Files live" on the page) and one more tap on DMM row → Bargraph. No bus reset
  came in between, so the volume stayed read-only all along.
- **Disk:** MMEM:CAT? equals the start (3 534 848 / 13 205 504, same names). Each session's LOGS/FAST000/001.DLG was
  deleted by exact name (`del1.py`).
- **Calibrator:** 1 V DC, enabled.
- Side note: another agent's emulator attached as sdb / usbtmc3 over vhci_hcd during this run. Its dmesg lines are
  not the meter's.

## ad74e93: generator queries, per-unit serial, eject (2026-10-23 20:58 - 22:40)

The coordinator had installed ad74e93. `*IDN?` = `Alientek,DM40A,<UID>,dm40-fw 0.1`. `dm40dbg.py boot`: marker VALID,
crash count 0, HEALTHY at the start and the end (no reset). No flashing, no reload.sh / DBG:UPDATE. The meter was found on
the scope page (10 us/div, cursors), DMM DC V AUTO+, RATE FAST. The calibrator was found on a 10 Hz square (func 2,
10 000 mHz), enabled. Files are in the session scratchpad's `chk/`: `sessA.py`, `sentE.py`, `summE.py`, `settle.py`,
`shots.py`, `reattach.py`, `usbreset.py`, and `out/` (JSON, CSV, PNG; `out/E_all_sent.txt` is the raw check E log).

| # | check | result |
|---|---|---|
| A1 | 72f011e: `OUTP?`, `SOUR:FREQ?`, `SOUR:FUNC?`, `SOUR:VOLT?`, `SOUR:VOLT:OFFS?`, `SOUR:FUNC:SQU:DCYC?` polled every 0.25 s in a 38400 AD2 disk session (15 s, 60 rounds = 360 queries, `a384`) | **PASS**: late 0, overruns 0, max_lat 280 ns (`DBG:ISR?` before 240 ns), 576 003 rows / 15.000 s, lost_before 0. Worst reply 4.8 ms (`OUTP?`). The replies did not change: `0`, `+2.000000E+06`, `SQU`, `+3.000000E+00`, `+1.500000E+00`, `+5.000000E+01` |
| A2 | The same at 76800 (`a768`, 64 rounds) | **PASS**: late 0, overruns 0, max_lat 280 ns, 1 152 003 rows / 15.000 s, lost_before 0, worst reply 13.3 ms |
| A3 | A generator that is OFF stays OFF | **PASS**: `OUTP?` 0 before, during (every round) and after both sessions, and at the end. Each session's LOGS/FAST000.DLG was deleted by exact name (only after checking that it was the one new name) |
| A4 | `OUTP?` with the generator ON does not glitch the output | **Not run**: switching the generator on is forbidden for agents (owner only) |
| B1 | `lsusb -v -d 1209:0001` iSerial == `xxd -p -u INFO/UID.BIN` == `*IDN?` field 3 | **PASS**: all three are the same 24 hex digits `<UID>`. iManufacturer "dm40-fw", iProduct "DM40 multimeter (dm40-fw)" |
| B2 | The same over BT (`dm40scpi.py --bt --mac ... scpi '*IDN?'`) | **PASS**: the same `*IDN?` line. `SYST:MOD?` is `DM40A,0,60000` on both links (its second field is not the serial) |
| B3 | udisks / by-id | **PASS**: udisks drive `/org/freedesktop/UDisks2/drives/ALIENTEK_DM40_files_<UID>`, Serial `<UID>`, Vendor ALIENTEK, Model "DM40 files". `/dev/disk/by-id/usb-ALIENTEK_DM40_files_<UID>-0:0` -> sda |
| B4 | sigrok-cli (libsigrok-dm40/install) `-d scpi-dmm:conn=usbtmc/3.10 --scan` | **PASS**: `scpi-dmm:conn=usbtmc/3.10 - Alientek DM40A dm40-fw 0.1 [S/N: <UID>] with 1 channel: P1`. This time `/dev/usbtmc2` stayed attached (USBDEVFS_CONNECT answered EBUSY = driver bound) |
| B5 | Reload-path query `DBG:FLASH?` | **PASS**: `EF4018,4096,0,0,0,0,0,2,76,179`. `SYST:ERR?` "No error" |
| B6 | ngscopeclient connects; Windows 11 VM (new instance, Get-PhysicalDisk SerialNumber); emu/ejecttest.py next to the meter | **Not run** (ngscopeclient is the owner's tool; no virsh; no emulator in this check) |
| C1 | Eject from Linux as a desktop does: `gio mount -e /run/media/$USER/DM40` (gvfs 1.60.0, udisks2 2.10.91, util-linux 2.42.3) | **FAIL (host side, twice)**: gio unmounts, then reports `eject: cannot open /dev/sda: Permission denied` (rc 2). The meter got no START STOP UNIT (`DBG:VDISK:EJECT?` 0,0,0), the medium stayed present. The error text is util-linux `eject` run as the user (not in group `disk`), not by root udisksd. Below |
| C2 | `udisksctl unmount` + udisks `Drive.Eject` over D-Bus (`busctl --system call ... Drive Eject 'a{sv}' 0`) | **PASS**: kernel "sda: detected capacity change from 4160541 to 0"; `DBG:VDISK:EJECT?` 1,2,2 (ejected, two 0x1B commands, last CB[4] = 2 = LoEj, Start 0); lsblk 0B, udisks MediaAvailable false, Size 0; `udisksctl mount` "not a mountable filesystem". USBTMC answered throughout |
| C3 | Stays ejected | **PASS**: sysfs size 0 and EJECT? 1,2,2 at +15 / 30 / 45 / 60 s, no kernel line for sda |
| C4 | Back after a re-plug | **PASS** with a port reset (USBDEVFS_RESET on /dev/bus/usb/003/010, plugdev; no physical re-plug): "reset full-speed USB device", sda 4160541 blocks, auto-mounted read-only at /run/media/$USER/DM40, README.TXT sha256 equal to before, EJECT? 0,2,2, RO? 1,1, /dev/usbtmc2 the same. A host load (LoEj=1 Start=1, `eject -t`) could not be sent: /dev/sda is root:disk |
| C5 | "PC can edit" + cp + Eject at once | **Not run**: it needs `DBG:VDISK:RO 0` (an owner setting), a bus reset and a write to the meter's disk. With the `gio` eject failing on this host, the "at once" desktop case would not even reach the meter |
| D | RES / A-JACK behaviour | Not changed (pending the owner's decision); see E |

**C1, why gio fails (not traced further).** The error is util-linux `eject`'s own text, and it is what an unprivileged
user gets: /dev/sda is `root:disk` 0660, so gvfs evidently ran `eject` itself as the user (it does so for some
mounts) instead of asking udisks. udisksd's own Drive.Eject (C2) runs as root and works.
So on this host, Files / Nautilus "Eject" on the DM40 volume only unmounts it. The emulator's ejecttest.py passed because it
falls back to udisks `Drive.Eject` over D-Bus when `gio mount -e` fails (emu/ejecttest.py `eject`). It is not a fw-rs
bug: the same would happen with any USB stick that gvfs ejects this way. A user who wants the medium gone can use
`udisksctl power-off` (not tried here: forbidden in this check) or unplug after the unmount. The kernel line
"/dev/sda: Can't open blockdev" appears also at each re-mount, so it does not mark the failure.

### E: false alerts after DMM function switches (owner report, 2026-10-23)

Method: `sentE.py` switches with SCPI `CONF:*` (DC V, AC V, A = `CONF:CURR:DC 10`, mA = `CONF:CURR:DC 0.06`, RES,
TEMP, POW), `VOLT:COUP AUTO`, taps on the DMM function bar (V / A (uA-first) / ohm / Hz / W), MENU -> Oscilloscope -> MENU ->
Multimeter, and AC V / DC V -> MF -> Wave -> MENU -> Multimeter. After each switch it polls `DBG:SENT?` without a pause for 2.5 s
(about 1 ms per poll), then every ~50 ms, and `FUNC?` every 8th poll. The watch lasts 5 s, or up to 14 s while an alert is on. "matrix" is an
Eulerian circuit over every ordered pair of {DC V, AC V, A, mA, RES, TEMP, POW} (42 switches). Levels: calibrator output
disabled (V-COM open), 1 V DC and 5 V DC on V-COM. The A jack is open. In all, 397 switches: off 175 (matrix twice,
chain DC V -> AC V -> A -> mA -> RES -> TEMP -> W and back, AUTO, UI, scope, Wave view), 1 V 111, 5 V 111.
DBG:SENT? fields: kind,alert,mask,ctsta,pkmin,pkmax,streak,hits,samples,probe_mA,aprot_ms,vside. CTSTA_CMP = 0x30
(CMPHO | CMPLO).

| # | finding | where | detector values |
|---|---|---|---|
| E1 | **False V-JACK? (amber) after RES -> A or RES -> mA**, V-COM open or 1 V. Every time (11 / 11: SCPI and the A tap). On 1.47-1.97 s after the switch, off at 4.9-5.5 s (shown 3.1-4.0 s) | off: matrix x2, chain, AUTO, UI; 1 V: matrix, AUTO, UI. Not at 5 V (RES -> A / mA there: 0 / 4) | kind CMP_SENSE from the first poll. ctsta 0 for 0.46-0.73 s (samples frozen: the row change), then **0x70** in every window (streak +1 per 100 ms window, hits +1). V-JACK at streak 10-11 (= first hit + ~1 s, VJACK_ASSERT_MS). From ~4.3 s ctsta 0x50, then 0 at ~4.5 s, streak 0; badge off ~0.5 s later. pkmin / pkmax are stale (the previous kind's). No V-JACK after DC V / AC V / TEMP / POW -> A, nor A <-> mA |
| E2 | **False V-JACK? after scope page -> DMM in A** (MENU, Oscilloscope, MENU, Multimeter) at 5 V DC: on 1.42 s, off 6.0 s | 5 V, `v5_wave` (1 of 1); not at 0 / 1 V | CMP_SENSE, ctsta 0 until 0.51 s, then 0x70 in every window, streak 10 at onset. A direct TEMP -> A at 5 V gave nothing |
| E3 | **False AC? hint (grey) in DC V on 5 V DC**: on in every DC V entry, 2.6 s after the switch (0-30 ms when the hint was already on: DC V -> DC V, MF / Wave / MENU / Multimeter within DC V), and it stays on (still on at 14 s). From VOLT:AUTO too. 26 / 26 steps that ended in DC V | 5 V only: every switch into DC V (SCPI, tap, Wave view / scope exit). 0 V / 1 V: never | kind CMP_PB1 with the alternating V-tap window: ctsta alternates **0x70** (V-tap window) and 0 (PB1 window) at 5 V; at 1 V it alternates 0x10 and 0 (one bit: no hint). So on a steady +5 V DC the comparator reports both CMPHO and CMPLO in the V-tap window, and `sample_cmp` takes `win_both == CTSTA_CMP` as "swings both ways". This is the open TODO item "the AC hint in DC V on 5 V DC" (Hz entry); the detector values are new |
| E4 | A-JACK (red) in RES at 5 V on every entry (12 / 12, from DC V, AC V, A, mA, TEMP, POW, the tap): on 2.65-2.70 s, stays on | 5 V | CMP_PB1, ctsta 0x70 from ~2.55 s, streak 2 at onset, vside **0** (free). The known 35fce2a item 7b (V_GATE premise fails on this device), owner decision pending (D): not counted as new |
| E5 | DC? hint (grey) in AC V on 1 V and 5 V DC: on 2.6-4.0 s after the switch (0.15 s inside Wave view / menu round trips), stays on | 1 V, 5 V | AD1PK_PB1, V-tap pk ~8 200-8 880 (1 V), ~38 100-38 800 (5 V). **Correct** (a DC voltage, no AC) |
| E6 | No AC? / DC? / A-JACK / A-PROT / V-JACK? anywhere else: with V-COM open, 0 alerts in the other 168 switches (DC V, AC V, AC+DC via the V tap, Hz, W, TEMP, AUTO coupling both ways, scope page / Wave view -> DMM in DC V, AC V, TEMP, A). **A-PROT never showed** (aprot_ms 0 throughout) | all | |

So the owner's "AC V hint" is E3 (5 V class DC in DC V, any entry), and the false current-jack alerts are E1 / E2 (the
amber V-JACK?, not A-JACK) plus the known E4. Neither E1 nor E2 is the hint carried over from the old function:
`configured()` resets streak / hints on a family change, and the alert comes from fresh 0x70 windows on the new
row. In E1 the comparator on SENSE sees the V input charged by the ohm source: it decays in ~4 s with V-COM open
and is held by a 1 V source, and a 5 V source swamps it. E2 needs the scope-rail round trip.

**Switch latency** (`settle.py`, 2 reps each at 1 V and 5 V; DMM RATE FAST; first valid = first FETCh? that is not
+9.91E37, polled every ~10 ms; 0.1 % band = max(0.1 % of the final value (mean of 5.5-7 s), a floor of 0.2 mV /
1 ohm / 2 mA / 20 nA); the screen columns are from the value strip read back at fixed delays, `shots.py`,
`out/shots_v1/*.png`, `out/sheet_v1.png`):

| switch | first valid FETCh? | within 0.1 % | screen |
|---|---|---|---|
| DC V -> AC V | 458-463 ms | 2.46-2.89 s (the AC RMS of a DC input decays from 15-38 mV to 0.3-0.7 mV) | blank at 0.2-0.5 s, a number at 0.5-0.8 s |
| AC V -> DC V | 457-462 ms | at once | |
| DC V -> RES | 2.67-2.68 s (1 V: 50.2 MOhm). 5 V: one 17.6 MOhm at 2.68 s, then OL | at once (1 V) | blank up to 2.0-2.3 s, a number at 3.0 s |
| RES -> DC V | 516-556 ms | at once | blank at 0.5-0.8 s, a number at 1.0 s |
| DC V -> A (10 A) | 752-762 ms | at once | blank at 0.5-0.8 s, 0.000 A at 1.0 s |
| DC V -> mA | 556-561 ms | at once | |
| AC V -> AC+DC | 1.32-1.37 s | at once | blank up to 1.0-1.3 s, a number at 1.5 s |
| DC V 6 V -> 60 V (`VOLT:RANG 60`) | 552-561 ms | at once | the old reading stays on screen until the new one |

The screen shows nothing (no dashes) while FETCh? is invalid. A reading then comes every ~100 ms (FAST). `READ?` right after a
settled switch answers in 5-100 ms. DC V -> RES (2.7 s) and AC V -> AC+DC (1.3 s) are the slow ones; V and A
switches take 0.45-0.76 s. A stock comparison was not made (no stock firmware on the meter).

### State at the end

- **Device:** ad74e93, back on the scope page as found (`end_scope.png` = the start screen), DMM DC V AUTO+
  (`VOLT:RANG:AUTO?` 1), `VOLT:COUP?` DC, RATE FAST (as found). LOG: AD1 / 4800 / HOST / DUR 60 / CHOP 1 / OSR 3 / STAT 0
  (sessA / settle had set AD2 / 76800 / 15: set back). `OUTP?` 0 throughout. `SYST:ERR?` "No error". `DBG:VDISK:RO?`
  1,1 (Files live). Crash count 0, marker VALID. The Settings pages were not opened, so volume, backlight, Dim /
  Shutdown and USB disk were not touched. The DMM page showed the bargraph row throughout.
- **Disk:** MMEM:CAT? names and used / free (3 534 848 / 13 205 504) equal the start. The two FAST000.DLG files were
  deleted by exact name. The volume is mounted read-only again after the eject test (port reset).
- **Calibrator:** back on the 10 Hz square (func 2, 10 000 mHz), enabled, as found. In check E it was off / 1 V / 5 V DC.

## d330877: stream-ui, pressed feedback, ereset dialog, V-JACK? exit experiments (2026-10-23 23:15 - 23:57)

The coordinator had installed d330877 (branch stream-ui = dad5500's stream / UI fixes rebased on rust-fw-poc 02611a5,
with the ereset / toast fix 5301fb7 and the pressed feedback 125a423). `*IDN?` = `Alientek,DM40A,<UID>,dm40-fw 0.1`,
`DBG:FLASH?` `EF4018,4096,0,...`. INFO/FIRMWARE.BIN, read with the page cache dropped, is byte-equal to the d330877
build (sha256 47dd11ad…4bcf). `dm40dbg.py boot`: marker 0x238DDC72 / 0x000968F4 / 0x0001563A VALID, crash count 0,
HEALTHY at the start and the end (no reset). No flashing. Files are in the session scratchpad's `chk2/`: `sessB.py` (a
disk session; `--cp` copies the 8 INFO/*.BIN with the cache dropped from state 3 on and compares them to references
taken before; the DLG is fetched, decoded with tools/dlog.py and deleted by exact name), `sessA.py`, `wave.py`,
`badge.py`, `pf.py` (pressed feedback), `ereset.py`, `sentE.py` (now with a fixed per-step watch, `STEP@seconds`),
`ctsum.py` (the ctsta timeline per step), `cal.py`, and `out/` (JSON, PNG; `out/ctsta_summary.txt`, `out/E_e1_*`,
`out/E_e4_*`). All tools ran with `DM40_USBTMC=real` (an emulator of another agent was attached over vhci).
The calibrator was found on a 10 Hz square (func 2, 10 000 mHz), enabled; the meter on the DMM page, DC V AUTO+, RATE FAST.

| # | check | result |
|---|---|---|
| 1a | Scope page (MCX), AD2 `LOG:DEST DISK`, `LOG:DUR 10`, `LOG:STAT ON` at 38400 (`mcx384`, generator queries polled too) and 76800 (`mcx768`) | **PASS**: 384 002 / 768 002 rows (= rate x 10 s), lost_before 0; late 0, overruns 0, max_lat 280 ns; the values follow the square (0..9.5 V). The 0-row bug is gone |
| 1a' | The same with `LOG:DEST HOST` (`dm40scpi.py log --source ad2 -d 10`) | **PASS**: 38400: 364 572 samples in 9.5 s, lost 0, overwritten 0. 76800: 400 628 drained, lost 0, ring overwritten 324 943: the known host limit (~40 k rows/s over LOG:DATA?, as 4c4b489 / bab5dd4 A3). late 0 both |
| 1b | Wave view (DC V, MF > Wave) at 1 ms/div (ACQ:SRAT? 76 800): MMEM:CAT? x3 (`wave.json`) | **PASS**: 7.4 / 20.0 / 7.5 ms per reply (was 123 ms), `dropped` +0 each, late / overruns +0; 1 s idle +0 |
| 1b' | V/div + (20 -> 200 V/div) and - (back) at 1 ms/div | **PASS**: the stream restarts (active 1), dropped 0, late 0, overruns 0 after each step; CONF? follows the range (60 / 600 / 60 V) |
| 1b'' | Source badge DMM -> MCX at 2 ms/div (38 400) | **PASS**: late 0 -> 0, overruns 0 -> 0, the stream stopped (active 0), DBG:VIEW? Oscilloscope, the MCX page drawn (`w_badge_exit.png`) |
| 1c | Settings > More > More > Device, MENU | **PASS**: the main menu (`s_device_menu.png`). The next action was a key, not a tap |
| 1d | The source badge tapped during a 38400 disk session on the MCX page (twice) | **Refusal PASS, toast FAIL (cosmetic)**: the view stays Oscilloscope, the session runs on (384 002 rows, lost 0, late 0). The toast "Wave: data logger running" is drawn under the DATA LOGGING panel: only its red ends ("W" … "g") show left and right of the panel (`badge_toast.png` at 0.4 s, `badge_toast2.png` at 0.02 s). New bug below |
| 2 | Pressed feedback, MF range pages V / A / Ω / F and Power Settings (`pf.py`: `DBG:DRAG` from a button to an empty spot, then a zero-travel drag on another button, then back) | **PASS for what synthetic input can show**: slide off (6V / 6000uA / 600Ω / AUTO / 60V): the screen afterwards is byte-equal to before, nothing selected (CONF?, coupling, AUTO, POW:COUP? unchanged); `DBG:TOUCH?` D, M, release. Tap: AC selects (V, A; `VOLT:COUP?` / `CURR:COUP?` AC), Low current ON (Ω), AC (Power Settings, `POW:COUP?` AC); each set back with a tap (DC / Normal) and the page byte-equal to the start. **Held screenshot: not possible from the host**: `DBG:DRAG` runs down, hold, move and release in one loop pass (gesture.rs `synth_drag`) and `DBG:TAP` injects only the Tap, so the lighter-blue pressed state is never on screen between two USB commands. It needs a real finger (the same limit as "hold repeat" in this doc) |
| 3a | Power meter, AC+DC, 1 V DC on V-COM: wait for the DC hint, then long press E (58,179), Yes (`er_a_*`) | **PASS**: SYST:ALER? DC at 3.7 s; the confirm "Reset energy?" opens (`er_a_2confirm.png`); Yes: the time 00:02:23 -> 00:00:00, FETC:POW:ALL? time 136 -> 1 s. Note: in AC+DC the E row is at y 170..192 (parts row); a long press at y 160 (the DC layout's row) lands in the swallowed band (no confirm, not a bug) |
| 3b | Long press E 0.5 s after entering (W tap, then POW:COUP ACDC: the W tap enters in DC) (`er_b_*`) | **PASS**: the confirm opens; SYST:ALER? DC at 9.0 s while it is up, the confirm stays (`er_b_2confirm_hint.png`, 1.3 s and 3.8 s later); Yes resets (time 3 s at the end); the hint toast "No AC: DC is more precise" shows 0.15 s after the confirm closes (`er_b_4after.png`). Under the confirm the page is not redrawn (readings "-- V / -- A", status RUN, no DC? badge until it closes) |
| 4 | E1 experiments (below) | data for the false-hint fix; **new bug**: V-JACK? in AC A with V-COM open |
| 5a | 38400 / 76800 disk sessions (`LOG:DUR 20`, AC V, the square) with `cp` of the 8 INFO/*.BIN from state 3 (`cp384`, `cp768`) | **PASS**: 768 003 / 1 536 004 rows, lost_before 0; late 0, overruns 0, dropped 0, max_lat 280 ns. FIRMWARE.BIN's cp ran 3.4 -> 23.8 s / 8.0 -> 29.3 s (waits for the session's end, as 35fce2a), all 8 copies byte-equal. (dmesg was not readable this time: `dmesg_new` in the JSONs is empty for that reason) |
| 5b | `*IDN?` serial, `lsusb` iSerial, `xxd -p -u INFO/UID.BIN`, /dev/disk/by-id | **PASS**: all `<UID>` |
| 5c | `udisksctl unmount` + udisks `Drive.Eject` (busctl), 30 s, then `usbreset.py` | **PASS**: sysfs size 0, `DBG:VDISK:EJECT?` 1,2,2 at +0 / 15 / 30 s, USBTMC answered; after the port reset: 4 160 541 blocks, auto-mounted read-only, EJECT? 0,2,2, RO? 1,1, README.TXT sha256 as before, /dev/usbtmc2 the same |

### 4. V-JACK? / ctsta after function switches (E1 follow-up)

`sentE.py` steps with a fixed watch each (`RES@6,DCV@1,A@8` = 6 s in RES, 1 s in DC V, 8 s in DC A); DBG:SENT?
polled every ~1-4 ms for 2.5 s, then every 50 ms. ctsta is printed in decimal by DBG:SENT? (112 = 0x70, 80 = 0x50,
16 = 0x10). The A jack is open throughout; the voltage goes only to V-COM. A = `CONF:CURR:DC 10`, mA = `CONF:CURR:DC
0.06`, ACA = `CONF:CURR:AC 10`. Per-step timelines: `out/ctsta_summary.txt`; raw rows: `out/E_<tag>_raw.json`.

| V-COM | sequence (reps) | CMP_SENSE ctsta in DC A / mA | alert |
|---|---|---|---|
| open | RES -> DC A (2), RES -> mA (1) (`e1_open_ctl`) | **E1 reproduces on d330877**: 0 until 0.47-0.67 s, 0x70 to 4.6 s, 0x50 to 4.8 s, then 0 | V-JACK 1.54-1.76 s -> 5.2-5.4 s |
| open | RES -> DC V 1 s -> DC A (3) (`e1_open_dcv`) | **0 throughout** (8 s) | none |
| open | RES -> AC A 2 s -> DC A (3) (`e1_open_aca`) | 0 throughout | none in DC A; **V-JACK in AC A from 1.67-1.78 s** |
| 1 V | RES -> DC A (2), RES -> mA (1) (`e1_v1_ctl`) | 0x70 0.66 -> 5.0-5.1 s, 0x50 to 5.3 s | V-JACK 1.57-1.72 -> 5.8-5.9 s |
| 1 V | RES -> DC V 1 s -> DC A (3) (`e1_v1_dcv`) | 0 throughout | none |
| 1 V | RES -> AC A 2 s -> DC A (2) (`e1_v1_aca`) | 0x70 0.65 -> 1.0 s, 0x50 to 1.16 s, then 0 | none in DC A; V-JACK in AC A from 1.67 s |
| 10 V | DC V -> DC A, DC V -> mA (`e4_v10`) | **0 throughout** (10 s) | none: the real misconnect is not seen |
| 10 V | TEMP -> DC A | 0x70 (from TEMP) to 0.66 s, then 0 | none |
| 10 V | AC V -> DC A | 0 | none |
| 24 V | DC V -> DC A, DC V -> mA (`e4_v24`) | **0 throughout** (10 s) | none: the real misconnect is not seen |
| 24 V | TEMP -> DC A, AC V -> DC A | **0x70 from the first poll to the end (10 s)**, streak +1 per window | V-JACK 1.71-1.72 s, stays |

So a 1 s stop in DC V (or 2 s in AC A at open) after RES clears E1 completely: the charge left by the ohm source is
gone (or not picked up) before the A row's first window. A real source on V-COM reads ctsta **0x70** in DC A when DC A
is entered from TEMP or AC V (24 V), but **0** when it is entered from DC V, where 10 / 24 V had autoranged DC V to 60
V; at 10 V even the TEMP entry gives 0 after 0.66 s. So whether the "A mode with a voltage on V" misconnect is seen at
all depends on the function before. A guess, not checked: DC V's 60 V range leaves the V input divider (or the
comparator's input) in a state the A row does not reset, so SENSE does not see the jack. Never 0x20 or 0x10 in DC A in
these runs.

Side observations in the same runs: in DC V at 10 / 24 V the sentinel kind turns `OFF` after 0.45 s (60 V range); in
AC V at 24 V DC: ctsta 0x70 and a short **A-JACK** (3.06 -> 4.07 s, kind AD1PK_PB1) where 10 V gave the DC? hint at
3.56 s (`e4_v24` step ACV@5).

**New bug: V-JACK? in AC A with V-COM open.** `CONF:CURR:AC 10`, `CURR:AC 0.06` and `CONF:CURR:AC` (600 uA), entered
from DC V, DC A or RES, nothing on any jack (calibrator disabled), also with 1 / 10 / 24 V on V-COM: kind AD1PK_SENSE,
ctsta 0, V-JACK from 1.66-1.92 s, still on after 30 s (`e1_open_acalong`), pkmin = pkmax = -262144 (AD1 at negative
full scale) once the row has settled, streak saturating at 255. The status bar shows the amber "V-JACK?"
(`aca_open.png`). The ad74e93 E matrix had no AC A step, so this is the first sighting.

### New bugs

1. **V-JACK? in every AC A range with V-COM open** (above). The detector takes AD1's negative full scale on the AC A
   row as a voltage on V.
2. **The "Wave: data logger running" toast is hidden by the DATA LOGGING panel** on the MCX page (1d): the refusal works,
   the text cannot be read. Fixed on branch smallbugs (TODO.md: the toast goes into the panel's body).
3. **DC A does not see a real 10 / 24 V on V-COM when entered from DC V** (ctsta 0 for 10 s), while TEMP / AC V entries
   at 24 V see 0x70 (above). The misconnect hint depends on the previous function.
4. Minor: `VOLT:RANG:AUTO?` in AC V and `CURR:RANG:AUTO?` in AC A answer +9.91E37 (the DC function's query outside
   it); from `pf.py`, not investigated. Fixed on branch smallbugs (TODO.md: a bare VOLT / CURR header is the
   quantity in any coupling).

### State at the end

- **Device:** d330877, DMM page, DC V AUTO+ (`VOLT:RANG:AUTO?` 1, `VOLT:COUP?` DC), RATE FAST, bargraph row
  (`end_dmm2.png`; the AC? hint toast there is the 10 Hz square, correct). LOG: AD1 / 4800 / HOST / DUR 60 / CHOP 1 / OSR
  3 / STAT 0, LOG:CSV:INT 0. POW:COUP DC. `OUTP?` 0 throughout. `SYST:ERR?` "No error". Scope page as found (10 us/div,
  CH1 2 V/div, offset -4.709 V, DC, AUTO sweep, trigger 2.894 V rising: `end_scope.png`). Crash count 0, marker VALID.
- **Settings:** Settings 1-3 at the end are byte-equal to the start screenshots (`s_set1..3.png` vs `end_set1..3.png`;
  only the live "9.4/s" rate digits differ): Dim / Shutdown Never, backlight 10 %, volume 0 %, Start page Multimeter,
  DMM row Bargraph, USB disk "Files live" (`DBG:VDISK:RO?` 1,1). Every MF-page selection made by the tap test was set
  back with a tap (page byte-equal).
- **Disk:** MMEM:CAT? equals the start string exactly (3 534 848 / 13 205 504, 27 names). Six LOGS/FAST000.DLG files
  (one per disk session: mcx384, mcx768, badge x2, cp384, cp768; the HOST runs write none) were deleted by exact name, each only after it was the one new name. The 76800 MCX run's
  CSV download was cut by a timeout; its DLG was then fetched, decoded and deleted by name in a second step.
- **Calibrator:** back on the 10 Hz square, enabled; registers 0x00..0x2F equal the start dump except 0x2C (a live
  value that changes between reads). Disabling it had set 0x12 to 0x3F80, and right after the frequency set 0x16,
  0x1A, 0x1C / 0x1D and 0x23 still differed. 0x12 was written back (0x4120); one read later 0x0C read 0x2711, and a
  second dump a minute later matched the start everywhere but 0x2C, with nothing more written.
  During the check it was off / 1 V / 10 V / 24 V DC; the meter's generator was never switched on.

## f0d21eb: false-hint fixes, USBTMC abort race, generator in region B (2026-10-24 00:04 - 00:57)

The coordinator had installed f0d21eb (rust-fw-poc: the falsehint sentinel fixes b9fb78e..f403cf1, the usbtmc
pump_in fix 9a230a3, Gen::apply / gen::plan in region B fb6be06). `*IDN?` = `Alientek,DM40A,<UID>,dm40-fw 0.1`,
`DBG:FLASH?` `EF4018,4096,0,...`. INFO/FIRMWARE.BIN, read with the page cache dropped (posix_fadvise DONTNEED), is
byte-equal to the f0d21eb build (sha256 0a2293c3…3bba). `dm40dbg.py boot`: marker VALID, crash count 0, HEALTHY at
the start and the end; uptime says the meter booted at ~00:02 (the install), no reset during the check. No flashing.
Files are in the session scratchpad's `chk3/`: `sentE.py` (now also logs `DBG:SENT:LOG?` and `CONF?` per step),
`ctsum.py`, `summE.py`, `sessG.py` (generator settings during a disk session), `genrestore.py`, `tmcabort.py` (the
Linux usbtmc driver's generic API), `tmcusb.py` (pyusb, exact 64-byte IN URBs), `settle.py`, `setshots.py`, `cal.py`,
and `out/`: `E_<tag>.json` / `E_<tag>_raw.json` per run (every DBG:SENT? row), `ctsta_summary.txt` (the ctsta
timeline per step of every run), `alerts_summary.txt`, `<tag>.txt` (one line per step with the new DBG:SENT:LOG?
entries), `tmc*_*.json`, `g384.json` / `g768.json`, `settle_v1.json`. The calibrator was found on the 10 Hz square
(func 2, 10 000 mHz), enabled; the meter on the DMM page, DC V AUTO+, RATE FAST, Settings as listed at the end.

| # | check | result |
|---|---|---|
| 1 | Falsehint retest item 1: 48 switches per level (DC V, AC V, AC+DC, V AUTO, A / mA / uA, AUTO A, RES, diode, continuity, cap, Hz, temp, W, AC A / mA / uA, AC+DC A, MF -> Wave -> MENU -> Multimeter in DC V and AC V, A and mA -> Scope -> back), V-COM open / 1 V / 5 V (`m_open`, `m_v1`, `m_v5`) | **PASS for what the fix targets**: no V-JACK? after RES, no AC? in DC V, no A-PROT anywhere, no A-JACK except the known RES at 5 V. The rest of DBG:SENT:LOG? is explained by the source (DC? in AC V at 1 / 5 V, LIVE in continuity at 1 V) or is a known / new bug: V-JACK? in AC A (2a), **OVERLOAD in AC A** and **A-JACK in continuity at 5 V** (new, below) |
| 1-E1 | RES -> DC A / mA / AUTO A / the A tap, each watched 10 s, plus RES -> DC V 1 s -> A and RES -> AC A 2 s -> A, open / 1 V / 5 V (`e1_open`, `e1_v1`, `e1_v5`; 15 steps each) | **PASS**: no V-JACK? in DC A after RES (0 of 21 RES -> A-family entries over the three levels, incl. the matrices). ctsta still reads 0x70 from 0.46-0.66 s to 4.8-4.9 s, then 0x50 for ~0.15 s, then 0 (as on d330877): the mask holds it off. RES -> DC V 1 s -> A and RES -> AC A 2 s -> A: ctsta 0 in DC A (item 5, as on d330877). The AC A step shows its own V-JACK (2a) |
| 1-E3 | DC V at 5 V DC, 15 s watches: from DC A, from AC V, Wave view -> back, VOLT:COUP AUTO -> DC (`e2_v5`), plus every DC V entry of `m_v5` | **PASS**: no AC? (0 of 20 steps ending in DC V at 5 V). The V-tap window still alternates ctsta 0x70 / 0 at 5 V (the input is unchanged; the AC? rule ignores both-bit reads now) |
| 1-E2 | DC A and DC mA -> MENU -> Oscilloscope -> MENU -> Multimeter at 5 V, 10 s watch (`e2_v5`), and the same in `m_open` / `m_v1` / `m_v5` | **PASS**: no V-JACK? (0 of 8). ctsta 0x70 0.51 -> 5.3-5.4 s, 0x50, then 0 |
| 2 (TODO 2) | DC V on the 0..9.5 V 40 Hz square, 15 s, three entries incl. Wave view -> back (`sq40`) | **PASS**: no AC?. A symmetric AC source was not at hand (the calibrator's square is unipolar) |
| 3 (TODO 3) | 24 V DC on V-COM: RES -> DC A, DC A -> Scope -> back; ctsta logged (`v24`, `v24b`, `v24sc`, `v24sc2`, `v24hist`) | **FAIL**: the real misconnect is missed in both cases. RES -> DC A (2 of 2): ctsta 0x70 0 -> 4.97 s, 0x50 to 5.18 s, then **0**, exactly the pattern of the open input: no V-JACK? in 10 s. DC A with V-JACK? on (entered TEMP -> AC V -> A), then Scope -> back (`v24sc2`): 0x70 0.51 -> 5.28 s, 0x50 to 5.43 s, then 0: **V-JACK? does not come back** (14 s). Details below |
| 4 | A-JACK / A-PROT with current in the A jack | **Not run** (the A jack is not wired) |
| 5 (TODO 5) | RES-exit: RES -> DC V 1 s -> DC A, RES -> AC A 2 s -> DC A | ctsta 0 in DC A at open / 1 V / 5 V (as d330877) |
| 6 (TODO 6) | Remaining alerts: DBG:SENT? rows and DBG:SENT:LOG? | In `out/E_*_raw.json`, `ctsta_summary.txt`, `<tag>.txt` and below. The SENT:LOG count went 0 -> 62 assertions over 302 switches |
| 2a | V-JACK? in AC A with V-COM open, 1, 5, 10, 24 V | **Unchanged (FAIL)**: every AC A entry, CONF:CURR:AC 10 / 0.06 / 600 uA, V-JACK? from 1.68-1.84 s (10 A from DC / AC V), at all levels. AD1PK_SENSE, ctsta 0. pkmin / pkmax run from ~-117 000 at 0.67 s down ~1 400 counts per 50 ms to -262 144 at 1.9-2.1 s; streak +1 per window from 0.78 s, V-JACK at streak 18-20. Rows in `E_m_open_raw.json` ACA@6 |
| 2b | DC A entered from DC V (60 V autorange) with 10 / 24 V on V-COM | **Unchanged**: ctsta 0 for 10 s, no V-JACK? (24 V: 2 / 2 DC A, 2 / 2 mA). 10 V is never seen in DC A from any entry (0x70 only for 0.66 s after TEMP), so 10 V may be under the comparator's threshold. New: the DC V state also carries through AC V (below) |
| 2c | A-JACK in AC V with 24 V DC on V-COM | **PASS (not reproduced)**: 5 AC V entries at 24 V (from DC A, AC A, DC V, TEMP), 5 s each: no A-JACK, also not in DBG:SENT:LOG?; DC? at 3.3 s where the entry came after AC A; ctsta 0x70 in AC V when entered after TEMP / a TEMP-entered DC A, 0 after DC V |
| 2d | VOLT:RANG:AUTO? in AC V, CURR:RANG:AUTO? in AC A | **Unchanged**: +9.91000000E+37 and -100 "Settings conflict" queued (also VOLT:RANG? / CURR:RANG?). The function-specific forms answer: VOLT:AC:RANG:AUTO? 1 in AC V, CURR:AC:RANG:AUTO? 0 in AC A. So the plain forms mean the DC function; whether that is wanted is for the owner (34401A: VOLT:RANG = VOLT:DC:RANG) |
| 3a | USBTMC abort-in race, exact URBs: pyusb on interface 0 (usbtmc detached, MSC untouched, re-attached in `finally`; `/dev/usbtmc2` back each time) with emu usbtmc_stress's c_abortin / c_stale / c_clear: `*IDN?` (52 + 12 = 64 bytes: one full packet + ZLP) aborted before any read, after its 64-byte packet (the ZLP still pending: the 9a230a3 race), DBG:LCD? (2007 bytes) after 1 / 3 packets, a REQUEST never read, INITIATE_CLEAR; each followed by a plain `*IDN?` (`tmcusb_abort2000.json`) | **PASS**: 2000 / 2000, 0 bad, 91 s. After the 64-byte packet: INITIATE 0x01 SUCCESS, CHECK `02 01 .. 34000000` (PENDING, read the IN FIFO, 52 bytes sent), the drain reads the ZLP (0 bytes), CHECK 0x01, the next `*IDN?` exact. DBG:LCD? after 3 packets: NBYTES 0xF4, drain 64 + ZLP. SYST:ERR? "No error" |
| 3b | The same through the Linux usbtmc driver (generic API: USBTMC_IOCTL_WRITE / READ with our own headers, CTRL_REQUEST for INITIATE / CHECK_ABORT_BULK_IN) (`tmc_abort500.json`) | **PASS**: 500 / 500. Note: this driver's generic READ submits URBs of its buffer size, so a "read 64 bytes" takes the whole transfer (incl. the ZLP) and INITIATE then answers 0x80 FAILED (correct: nothing left), and a drain must be one large read; a first try that drained with 64-byte reads looked like a missing ZLP: a host artifact, cleared by 3a |
| 3c | 2000 queries, random `*IDN?` / `READ?` / `WAV:PRE?`, plain write / read, no desync repair (`tmc_mix2000.json`) | **PASS**: 0 empty, 0 mismatched (`*IDN?` and `WAV:PRE?` byte-equal to their first reply, READ? a number every time). Worst reply 7.6 / 106.8 / 4.9 ms |
| 4a | Generator settings during a 38400 AD2 disk session (15 s): SOUR:FREQ 1000; SOUR:FUNC SQU; SOUR:VOLT 1.5; SOUR:FREQ 2345 about 1 s apart, OUTP? checked before each, the 6 generator queries every 0.25 s (`g384`) | **PASS**: late 0, overruns 0, dropped 0, max_lat 280 ns (260 ns after each setting), 576 002 rows, lost_before 0. Each setting answered in 0.1 ms, SYST:ERR? "No error". OUTP? 0 before, after each setting, in all 60 polls and after. SOUR:FREQ? 2345 reads back +2.344996E+03, DCYC +5.003000E+01 (the timer's resolution) |
| 4b | The same at 76800 (`g768`, generator first set back to the owner's values so every setting is a change) | **PASS**: late 0, overruns 0, max_lat 280 ns, 1 152 003 rows over 15.0 s (t 1.007 -> 16.007 s after the writer trial, gate 1 = kept the 76800 rung), no gap, lost_before 0. OUTP? 0 throughout |
| 5 | Switch latency, settle.py at 1 V, 2 reps (`settle_v1.json`), the baseline for the fast-switch build | Same as ad74e93: first valid FETCh? DC V -> AC V 460-462 ms (0.1 % at 2.46-2.47 s), AC V -> DC V 457-460 ms, DC V -> RES 2672 ms, RES -> DC V 554-558 ms, DC V -> A 758 ms, DC V -> mA 557-559 ms, AC V -> AC+DC 1372-1377 ms, DC V 6 V -> 60 V 509-557 ms |

### 24 V on V-COM in DC A: when the comparator sees it

ctsta is DBG:SENT? field 4 in decimal (112 = 0x70, 80 = 0x50, 16 = 0x10). Per step, from `ctsta_summary.txt`:

| entry into DC A (24 V on V-COM, A jack open) | ctsta in DC A | V-JACK? |
|---|---|---|
| DC V (60 V autorange) -> A / mA (`v24`, `v24b`) | 0 throughout (10 s) | none |
| TEMP -> A (`v24`, `v24b`, `v24hist`) | 0x70 from the first poll to the end | **8.00-8.01 s** (d330877: 1.71 s; now the 7 s OHM_EXIT cap, as designed) |
| AC V -> A, where AC V came after TEMP or after a TEMP-entered A (`v24`, `v24b`, `v24hist`, `v24sc2`) | 0x70 to 0.65 s, then **0x50 for good** | **1.76-1.77 s** |
| AC V -> A, where AC V came straight from DC V (`v24sc` x2, `v24hist`) | 0 throughout | none |
| RES -> A (`v24`, `v24b`) | 0x70 to 4.97 s, 0x50 to 5.18 s, then 0 | none |
| A (V-JACK? on) -> MENU -> Oscilloscope -> MENU -> Multimeter (`v24sc2`) | 0x70 0.51 -> 5.28 s, 0x50 to 5.43 s, then 0 | lost (none in 14 s) |
| A (no alert) -> Scope -> back (`v24`, `v24b`, `v24sc`) | the same pattern | none |

So a real 24 V source reads either 0x70 for good (TEMP -> A: the source outlasts the ~5.3 s 0x70 tail of an ohm
exit / chip restart) or 0x50 for good (AC V -> A after TEMP), and the row sees it only in those cases. The 0x70
phase after an ohm exit or a chip restart looks the same with and without a source; after it the row reads 0,
source or not. What decides it is the history: DC V on its 60 V range, the RES exit and the chip restart (the
Scope round trip) leave the A row blind (a seeing A -> RES -> A ends in 0, as does A -> Scope -> back); TEMP makes
it see again; AC V in between changes nothing either way. The guess of d330877 holds and is wider: some comparator /
divider setting that DC V 60 V, the RES exit and dmm.start leave behind is not rewritten by the AC V / DC A rows,
and the TEMP row rewrites it. In AC V, ctsta at 24 V is 0x70 after TEMP and 0 after DC V, the same split. With the
7 s cap, OHM_EXIT_CAP_MS cannot shrink on these data alone: in the RES case the source is invisible after the cap
anyway.

### New bugs

1. **False OVERLOAD (red) in AC A, and the 60 mA range falls back to 10 A.** With 1 V or 5 V on V-COM and the A
   jack open: `CONF:CURR:AC 10`, then `CONF:CURR:AC 0.06`: CONF? reads 60 mA at +0.05 s, then OVERLOAD (AD1PK_SENSE)
   at 0.5 s and CONF? is back to 10 A (`acarange2.txt`, twice: after 1 s and 6 s in 10 A; `m_v1` / `m_v5` step
   ACmA). pkmin / pkmax there are -207 000..-218 000 (the AD1 drift of 2a). From DC mA, CONF:CURR:AC 0.06 stays in
   60 mA (with the 2a V-JACK?), as does CURR:AC:RANG 0.06 after 600 uA (`acarange.txt`). Same detector input as 2a.
2. **A real 24 V on V-COM is missed in DC A after RES and after the Scope round trip** (above; TODO falsehint
   retest 3). A V-JACK? that was on goes away after Scope -> back.
3. **A-JACK in continuity at 5 V DC on V-COM** (`m_v5` CONT from diode: A-JACK at 0.41 s, AD2PK_PB1, stays). At 1 V
   the same step shows LIVE (right). Probably the V_GATE premise of the RES 5 V item (35fce2a 7b, owner decision),
   seen in continuity for the first time.

### State at the end

- **Device:** f0d21eb, DMM page, DC V AUTO+ (`VOLT:RANG:AUTO?` 1, `VOLT:COUP?` DC), RATE FAST, the bargraph row
  (`e_dmm.png`, the same layout as `s_dmm.png`). LOG: AD1 / 4800 / HOST / DUR 60 / CHOP 1 / OSR 3 / STAT 0, CSV:INT 0
  (sessG had set AD2 / 76800 / 15: set back). Generator: OUTP? 0 throughout, never switched on; its settings (SQU,
  2 MHz, 3 V, 1.5 V offset, 50 %) set back and read back equal after each session. POW:COUP DC. SYST:ERR? "No error",
  SYST:ALER? NONE. Crash count 0, marker VALID.
- **Settings:** Settings 1/3 and 3/3 at the end are byte-equal to the start screenshots (`s_set1..3.png` vs
  `e_set1..3.png`); 2/3 differs only in the live rate digits (x 277-308, y 50-65). Dim / Shutdown Never, backlight
  10 %, volume 0 %, Start page Multimeter, DMM row Bargraph, USB disk "Files live" (`DBG:VDISK:RO?` 1,1).
- **Disk:** MMEM:CAT? equals the start string exactly (3 534 848 / 13 205 504, 27 names). The two LOGS/FAST000.DLG
  (g384, g768) were deleted by exact name, each only after it was the one new name.
- **USB:** the usbtmc driver was detached from interface 0 only during the 3a runs and re-attached (`/dev/usbtmc2`
  each time); the volume stayed mounted read-only.
- **Calibrator:** back on the 10 Hz square, enabled; registers 0x00..0x2F equal the start dump except 0x2C (live).
  Right after the last frequency set 0x0C read 0x2711 and 0x23 0x31C9; a minute later both matched, with nothing
  written. During the check: off, 1 V, 5 V, the 0..9.5 V 40 Hz square, 10 V, 24 V DC.

## Crosstalk and cold-junction self-heating (f0d21eb) (2026-10-24 01:03 - 02:31)

The meter was still running f0d21eb, which is the same as rust-fw-poc 3eebbb4 for the DMM and the scope. Nothing was
flashed and the firmware was not changed. The files are in the session scratchpad's `chk4/`:
- `xt.py`: alternated blocks of DMM page / combined view / DMM page with `SYST:RAIL BOTH`, N `READ?` each, with
  pairwise differencing (`ana.py`);
- `fentry.py`: the first `READ?` values after `CONF:FREQ`;
- `sc2combo.py`: Scope page -> combined view;
- `cjlog.py`, `cjana.py`: the cold-junction log;
- `nav.py`: view changes, each checked with `DBG:VIEW?`;
- `out/`: `xt_*.json`, `fentry_*.json`, `f40_scope2combo.json`, `cj_run.csv`, and the PNGs.

**Wiring (observed):** the calibrator drives the DMM V / COM jacks and the scope probe in parallel. In the combined
view, 1 V on the calibrator moved `SCOP:MEAS:VAVG?` from -0.16 V to +0.87 V while the DMM read 1.0004 V. The probe is
always on the node, so a "without the probe" case cannot be run on this bench; only the scope's power and
acquisition differ between the views.

The generator stayed as the owner keeps it: settings untouched, `OUTP?` 0 throughout. The scope stayed as found:
10 us/div, 2 V/div, DC, AUTO. Rails AUTO: on the DMM page `SYST:RAIL:STAT?` reads 1,0, in the combined view 1,1. The
DMM rail stays on, so entering the combined view does not restart the chip.

### 1. Crosstalk: DMM page vs combined view

The calibrator's DC output wanders by several 0.1 mV over minutes (1 V: 1.0002 -> 1.0007 V across the blocks). Each
combined-view block is therefore compared with the mean of the DMM-page blocks before and after it. Rate FAST, 6 V
range (AUTO+).

| signal | readings (DMM / test) | combined - DMM (mean of pairs, sem) | noise sd DMM / test | present DMM-only? |
|---|---|---|---|---|
| calibrator off (input at ~0 V via the source) | 200 / 150 | **-0.052 mV** (0.006), 3 pairs | 0.030 / 0.039 mV | no: follows the scope rail |
| 1 V DC (`dc1`, `dc1b`) | 660 / 520 | **-0.24 mV** (0.06), 6 pairs, all negative (-0.04 .. -0.43); -24 ppm | 0.13 / 0.11 mV | no |
| 5 V DC (`dc5`) | 420 / 360 | **-0.39 mV** (0.06), 6 pairs, all negative (-0.12 .. -0.51); -77 ppm | 0.12 / 0.14 mV | no |
| 5 V DC, DMM page with `SYST:RAIL BOTH` instead of the combined view (`dc5both`) | 420 / 360 | **-0.24 mV** (0.07), 5 of 6 pairs negative | 0.12 / 0.14 mV | (this is the rail alone) |

- **DC V shift:** with the scope powered, DC V reads 0.05 mV (0 V) to 0.4 mV (5 V) lower; the noise does not change.
  Most of the shift comes back on the DMM page with only the scope rail forced on, so it is analog (rail / front end),
  not the view. It is within spec (< 0.01 %). It is larger than the "DC 0 / 5 V +-0.03 mV" of the f3aea2f follow-up
  (a shorter A/B without pairwise differencing). Not a firmware matter. Whether the meter or the node moves cannot
  be told without a second meter.
- **AC V of the 0.5 V DC source (`ac05`):** `CONF:VOLT:DC`, 3 s, `CONF:VOLT:AC`, then 200 `READ?` (~10/s), five
  blocks alternated. The first reading is 33-35 mV, then 13, 13, 10, 7, 5 mV. It is below 1 mV at 1.35-1.46 s and
  below 0.5 mV at 1.86-2.00 s. The floor after 5 s is 0.333-0.353 mV (sd 0.014 mV). **The DMM page and the combined
  view are the same:** DMM 0.335 / 0.336 / 0.349 mV, combined 0.333 / 0.353 mV. Changing the view while in AC V
  (`ac05v`) gives no transient (0.30-0.42 mV from the first reading). So the old "23-56 mV, decaying slowly" is
  gone. What is left is a 2 s settling tail after the DC V -> AC V function change, on the DMM page as well.
  `SYST:ALER?` DC (the hint), correct.
- **FREQ on the 40 Hz square (calibrator 40 000 mHz):**
  - Steady state, both views: 40.00173-40.00184 Hz (sd < 0.0004 Hz). This covers about 440 combined-view readings:
    `f40dc`, `f40acdc` (AC+DC -> Hz, the old trigger), `f40view` (view change while in Hz), and `f40_scope2combo`
    (Scope page -> combined, 3 times: the DMM rail wake, `SYST:RAIL:STAT?` wakes 14 -> 17). Wrong steady values:
    **0. 13.3 Hz was not seen.** The scope's own "Freq:" cell reads "--" at 10 us/div (`SCOP:MEAS:FREQ?` 0), so it
    is not the value in the strip either (`combo_freq.png`: strip 40.002Hz 50.0 %).
  - **Wrong first readings after `CONF:FREQ` from DC V, in both views:**
    - DMM page: 19.94 (x2), 0.00, 0.99 (x2) Hz;
    - combined view: 0.94 (x2), 0.00, 17.04 (x2) Hz;
    - 1-2 `READ?` (<= 0.7 s), then clean.
    - DC V -> Hz entries made within ~5 s of a view change: 6 of 6 had them (`f40dc` x5, plus the first `fentry`
      combined rep: 17.04 Hz). Entries made while staying on one page: 0 of 11 (`fentry_*`). AC+DC -> Hz entries
      right after a view change: 0 of 5 (`f40acdc`).
    - In the timed case, the wrong reading was returned 1.04 s after `CONF:FREQ`; every clean first reading came at
      1.37-1.41 s.
    - So this is not caused by the combination: it is the first counter result after the row write.
  - **Cause (from the code, `dmm.rs`):**
    - The Hz row is written by `hy::configure` (R20 = 0xF2 from `CFG_F14`). CTA is never preset (0xFC0000) and the
      counter is not restarted at the row write; only `read_counters` does that, after each read.
    - `freq.next` keeps the old schedule, so the first `poll_freq_mode` after the settle can read a gate that started
      before or across the row change.
    - In `freq_change`, `last` = 0 after `reset_values`, so a source's first result wins when it is lower.
    - A source that is stable but wrong wins as long as its duty stays below 99.95 % (`freq_glitch`).
    - The 13.33 / 26.67 Hz (k/3 of 40 Hz) of the 16b3eb9 bug fit the reciprocal gate (minimum 53.3 ms = 3 periods
      of 40 Hz) with 1 or 2 of 3 edges missed.
  - **Proposed fix (not implemented):**
    1. On every mode-14 row write (`set` / `start` / `write_config`), restart the counter: R20 = 0xF0, CTA = 0xFC0000,
       R20 = 0xF2. Also set `src_alt = false` and `freq.next = deadline + 350`, and drop the first result of each
       source.
    2. Treat a source's first result as unknown (change = 1e6 while `last == 0`). Show nothing until a source has two
       results that agree within ~0.1 %.
    3. Optional hardening: reject a result whose ratio to the other clean source is near k/2 or k/3.

### 2. UF-02: cold-junction self-heating

TEMP (thermocouple K, `TEMP:OFFS` 0), calibrator at 0 V DC enabled on V-COM. The TC readout was ~190-204 °C, but the
CJ comes from the HY3131's internal sensor and is independent of it. Logged every 3 s: `MEAS:TEMP:CJ?` (age <= 2.2 s
on every row used), `SYST:TEMP?` (the MCU die). One continuous run (`cj_run.csv`, 1 206 rows):

| state | duration | CJ: first 2 min -> last 10 min (range) | CJ fit (exp) | MCU die: first 2 min -> last 10 min | MCU fit |
|---|---|---|---|---|---|
| a. DMM page (scope rail off) | 25 min | 26.12 -> **25.62 °C** (25.47-26.59), sd 0.07 | -0.76 °C, tau 150 s (cooling from the crosstalk runs before, half of them with the scope on) | 36.16 -> 35.96 °C | -0.4 °C, tau 250 s |
| b. combined view (scope running, rails 1,1) | 25 min | 25.47 -> **25.58 °C** (25.43-25.65), sd 0.06 | **no step** (flat within +-0.1 °C) | 36.89 -> **37.59 °C** (max 38.7) | **+1.0 °C, tau ~2 min** |
| c. back to the DMM page | 10.5 min (cut short: meter needed for a flash) | 25.57 -> **25.43 °C** (25.33-25.61) | -0.2 °C, tau ~6 min (ambient drift?) | 36.35 -> 36.01 °C | back to the state-a level |

- **The CJ does not self-heat from the scope on f0d21eb:** within +-0.1-0.2 °C across a -> b -> c. The ~0.2 °C fall
  through b and c is the size of the room drift. The MCU die warms by 1.0-1.6 °C with the scope running
  (tau ~2 min) and cools back. The CJ sensor sits in the HY3131, away from the scope's power, and does not follow.
- The UF-02 step 2 threshold (> 0.5 °C bias -> correct or power down) is not reached. There is no need for a
  rail-dependent CJ correction. The steady CJ of 25.4-25.6 °C has not been checked against a room thermometer
  (FR-26 / the "3.6-6 °C low" units are a separate offset question).
- **Stock:** `docs/user-feedback.md` §3 has no number for the stock firmware, only U1's report: the CJ is wrong
  after AC+DC V and needs "~1 h idle" to match a weather station. The fw-rs time constants here are minutes, and
  there is no step to recover from. The stock-firmware side was not measured (not flashed).
- Side note: `MEAS:TEMP:CJ?` outside TEMP returns the last CJ with its age (34.41 °C, age 454 s at the start; 2 122 s
  at the start of the log). That value was taken in an earlier session's TEMP step and is 8.8 °C above the live
  25.6 °C. The age is reported and `SYST:ERR?` says "Data stale", as documented. A host that ignores the age gets a
  stale number. The cause of the 34.4 °C (taken on a warmer meter, or a first-phase value right after a switch) was
  not chased.

### 3. Other TODO items

Not run: the coordinator needed the meter for a flash. "60 V reads 0.2-0.36 mV below 6 V" stays open. The
`xt.py` alternation with pairwise differencing is the method to use for it, because the calibrator's wander is of
the same size.

### State at the end

- **Device:** f0d21eb, DMM page, DC V AUTO+ (`VOLT:RANG:AUTO?` 1, `CONF?` VOLT), RATE FAST, the bargraph row
  (`e_dmm.png`), `SYST:RAIL?` AUTO, `TEMP:TRAN?` TCK, `UNIT:TEMP?` C. `OUTP?` 0 throughout (never switched on).
  `SYST:ERR?` "No error", `SYST:ALER?` NONE. `dm40dbg.py boot`: crash count 0, HEALTHY, marker VALID.
- **Settings:** Settings 1/3 and 3/3 at the end are byte-equal to the start screenshots (`s_set1..3.png` vs
  `e_set1..3.png`); 2/3 differs only in the live rate digits (x 277-329, y 48-67). Dim / Shutdown Never, backlight
  10 %, volume 0 %, Start page Multimeter, DMM row Bargraph, USB disk "Files live" (`DBG:VDISK:RO?` 1,1).
- **Disk:** `MMEM:CAT?` equals the start string exactly. No files were created.
- **Calibrator:** back on the 10 Hz square, enabled. Registers 0x00..0x2F equal the start dump except:
  - 0x2C (live);
  - **0x0E and 0x2E: 0x1B -> 0x1A.** Both changed together while the calibrator was at 0 V DC and did not come back
    with 1 V or with the square. A write of 0x1B to 0x0E was ignored (it still read 0x1A), so these look like a
    read-only live value (perhaps a temperature: 27 -> 26). Nothing else was written.
  - During the check the calibrator was at 1 V, 5 V, off, 0.5 V, 40 Hz square, 0 V DC.

## 612c45c: INFO/ mirror, panel toast, range queries, danger alarm (no current) (2026-10-24 02:37 - 03:35)

The coordinator had installed 612c45c (rust-fw-poc: the INFO/ mirror f69222f..895b520, smallbugs b190073 / 4f560cd,
the danger alarm 7faf91b..612c45c). `*IDN?` = `Alientek,DM40A,<UID>,dm40-fw 0.1`, `DBG:FLASH?` `EF4018,4096,0,...`.
INFO/FIRMWARE.BIN, read with the page cache dropped, is byte-equal to the image (sha256 fb954da3…d389). Crash count
0, marker VALID, HEALTHY at the start and the end. No flashing. Files are in the session scratchpad's `chk5/`:
`sessM.py` (a long AD2 disk session with cp rounds of the 8 INFO/*.BIN every N s from state 3), `sessB.py` (now with
`CPFILES` / `CPDELAY`), `wv.py` / `wvidle.py` (Wave view at 1 ms/div), `tst.py` / `tst2.py` (toast), `rng.py`
(range queries), `alm.py` (alarm without current), `sentE.py`, `sessG.py`, `tmcabort.py`, and `out/` (JSON, PNG;
`out/ref/` = the 8 INFO files read at the start with no stream). The calibrator was found on the 10 Hz square,
enabled; the meter on the DMM page, DC V AUTO+, RATE FAST.

| # | check | result |
|---|---|---|
| M1 | First boot of the mirror build: `DBG:VDISK:MIR?` | **PASS**: `1,4,1,5164` (WRITTEN, 5.16 s for the rewrite; the emulator's typ estimate was 4.3 s). The boot time as a whole cannot be told: the mirror runs in `native::pre_main`, before the uptime clock and USB |
| M1' | Reboot (`DBG:RESET`; `*RST` is only a status clear here) | **PASS**: `1,3,0,302` (PRESENT, compare 0.30 s). USBTMC answered 3.2 s after the command at uptime 1.85 s; the host saw descriptor errors for ~1.3 s before the disconnect (the SW reset does not drop the pull-up first), then re-enumerated. Came back on the DMM page, DC V AUTO+ |
| M2a | 38400 AD2 disk session `LOG:DUR 300`, cp of the 8 INFO/*.BIN (FIRMWARE.BIN first) at state 3 + 0 / 60 / 120 / 180 / 240 s (`m384`) | **PASS**: every cp returned at once (FIRMWARE.BIN 1.06 s, LOADER.BIN 0.51 s), all 40 copies byte-equal to `out/ref`. The session ended **full** (state 6) at 286.2 s: 10 988 839 rows (= 38400 x 286.17 s), lost_before 0; late 0, overruns 0, dropped 0, max_lat 280 ns. A 5 min session does not fit: the free 12.9 MB hold ~286 s here (the prep allocated all of it, 33 s). DLG deleted by name |
| M2b | 76800, `LOG:DUR 0` (until full, ~130 s of samples), cp rounds every 30 s (`m768`) | **FAIL**: copies fast (FIRMWARE.BIN 2.0 s) and byte-equal, late 0, overruns 0, max_lat 280 ns, gate 1 (kept 76800), but **dropped / lost_before 234 147** (~46 000 per round, 2.4 % of 9.68 M rows) |
| M2c | 20 s 76800 sessions to isolate it (`b768`, `c768*`) | no cp: lost 0. The 8 files at state 3: lost 45 562. Non-mirrored files only (OTP, UID, OPTBYTES, ROMBOOT): 0. CALIB.BIN (4 KiB, mirrored) 6 s after state 3: 0. **LOADER.BIN (128 KiB, mirrored) 6 s after state 3: 21 745 lost**. LOADER.BIN at state 3 (inside the writer trial, 3 runs): the trial falls back to 38400 (gate 2, fill 2048), then lost 0. So the mirror's SPI reads, once the 76800 rung is kept, starve the logger's page programs |
| M3 | Wave view (DC V, MF > Wave) at 1 ms/div (ACQ:SRAT? 76 800) for 200 s, cp rounds every 40 s (`wv`) | **PASS for the copies**: 5 rounds, each file byte-equal, FIRMWARE.BIN 2.4-2.6 s; late 0, overruns 0; the stream stayed active. **Open**: DBG:ISR? `dropped` grew 295 -> 11 185 (~50/s, also between rounds); `wvidle.py`: a 30 s idle window +0, per-file reads (CALIB, ROMBOOT, LOADER, FIRMWARE) +0 each, the next 30 s idle +68. max_lat 300 ns (once). d330877 1b saw +0 in short windows. TIM:SCAL set back to 10 ms |
| T1 | Toast in the DATA LOGGING panel: MCX page, 38400 disk session, `DBG:TAP 177,10` (`tst`, `tst2`) | **PASS**: "Wave: data logger running" in the panel body under the title (`t_toast03.png`), nothing outside the panel box; one shot at 1.9 s shows the panel's rows back (`t2_19.png`). (With three 1.1 s screenshots in a row the toast was still up at 2.6 s, back at 4.0 s: the shots hold the loop.) The view stays Oscilloscope, 460 802 rows, lost 0, late 0 |
| R1 | Bare `VOLT` / `CURR` range queries (`rng.json`) | **PASS**: AC / AC+DC / AUTO V: VOLT:RANG:AUTO? 1 (0 after VOLT:RANG:AUTO OFF, 1 after ON), VOLT:RANG? the range (6 / 60 / 60 V); AC / AC+DC / AUTO A the same for CURR (600 uA / 60 mA / 60 mA). The other quantity: RANG:AUTO? 1, RANG? / RES? +9.91E37. RES: both +9.91E37. CONF:POW: VOLT:RANG? 6 = POW:VOLT:RANG? `AUTO,+6`, CURR:RANG? 0.06 = POW:CURR:RANG? `AUTO,+0.06`. SYST:ERR? "No error" after every row |
| D1 | Settings 3/3 "Danger alarm", `SYST:ALER:ALARm?`, `DBG:ALARm?` | **PASS**: the row read Loud at the start (`s_set3.png`), LOUD, `0,0,0,0`. At the end it reads **Off** (`e_set3.png`): the coordinator set OFF for the night (owner asleep, no sound); left OFF on purpose |
| D2 | V-JACK? (24 V DC on V-COM, CONF:TEMP 3 s -> CONF:CURR:DC 10, 15 s; `alm_vjack`) | **PASS**: V-JACK at 8.1 s after the A switch (the OHM_EXIT cap, as f0d21eb), `DBG:ALARm?` 0,0,0,0 in every poll |
| D3 | OVERLOAD without current (5 V on V-COM, CONF:CURR:AC 10 -> CONF:CURR:AC 0.06, the f0d21eb false OVERLOAD; `alm_ovl`) | **PASS** (no alarm): OVERLOAD 0.95 s after the switch, `DBG:ALARm?` 0,0,0,0. The one 300 ms beep is at volume 0 (not heard / not measurable) |
| D4 | Coupling check (doc 4.10 item 4) | **Not run**: no debug command makes the alarm sound without current (DBG:ALARm is a query only), and then the owner asked for no sound |
| S1 | E subsets at 5 V on V-COM: `seqE1` (RES -> A family, 15 steps) and `seqE2` (DC V 15 s watches, A / mA -> Scope -> back, Wave, AUTO; 20 steps) | **PASS (no new false alert)**: the same as f0d21eb: A-JACK in RES at 5 V (6 of 6, 2.86-3.01 s, CMP_PB1; the 35fce2a 7b item), V-JACK? in AC A (known), DC? in AC V (correct). No V-JACK? in DC A after RES, no AC? in DC V, nothing after Scope -> back. **But with the alarm on Loud, each of the 6 false A-JACKs started a danger alarm event** (`DBG:ALARm?` events 0 -> 6; nothing silenced it, each ended with the RES step after ~3 s): new bug 2 |
| S2 | USBTMC 500-query mix (`tmc_mix500.json`) | **PASS**: 0 empty, 0 mismatched; worst `*IDN?` / `WAV:PRE?` 2.4 ms, READ? 608 ms (one wait for a reading) |
| S3 | Generator settings during a 38400 disk session (`g384`) | **PASS**: 576 002 rows, lost 0, late 0, max_lat 280 ns; OUTP? 0 before, after each setting and in all 60 polls; settings set back and equal |

### New bugs

1. **A 76800 disk session loses samples while the host reads a mirrored INFO/ file** (M2b / M2c): ~22 000 per
   LOADER.BIN copy, ~46 000 per round of the 8 files; 0 at 38400, 0 for non-mirrored files and CALIB.BIN. A copy
   inside the writer trial makes the session fall back to 38400 (no loss). The emulator's fastlogtest (typ / max
   chip timing) showed lost 0, so its SPI model does not catch this.
2. **The known false A-JACK in RES with 5 V on V-COM now sounds the loud danger alarm** (S1): 6 events in 6 RES
   entries. A false A-JACK was a badge before; with the Loud default it is a full-volume two-tone for as long as it
   lasts. Needs the A-JACK false-positive fixed (or RES excluded from the alarm until it is).
3. Wave view at 1 ms/div: DBG:ISR? `dropped` grows ~50/s over minutes (M3). Not chased; cause unknown.

### State at the end

- **Device:** 612c45c, DMM page, DC V AUTO+ (`VOLT:RANG:AUTO?` 1), RATE FAST, the bargraph row (`e_dmm.png`).
  LOG: AD1 / 4800 / HOST / DUR 60 / CHOP 1 / OSR 3 / STAT 0, CSV:INT 0. Generator SQU 2 MHz 3 V 1.5 V 50 %, OUTP? 0
  throughout. Scope page as found (10 us/div, 2 V/div, offset -4.709 V, trigger 2.894 V, AUTO). POW:COUP DC.
  SYST:ERR? "No error", SYST:ALER? NONE, `DBG:VDISK:MIR?` 1,3,0,302. Crash count 0, marker VALID, HEALTHY.
- **Settings:** 1/3 byte-equal to the start; 2/3 only the live rate digits; 3/3 differs only in "Danger alarm"
  (Loud -> **Off**, set by the coordinator for the night). Volume 0 %, backlight 10 %, Dim / Shutdown Never, Start
  page Multimeter, DMM row Bargraph, USB disk "Files live" (`DBG:VDISK:RO?` 1,1).
- **Disk:** `MMEM:CAT?` equals the start string exactly (3 801 088 used: +266 240 B vs f0d21eb = the mirror). 14
  LOGS/FAST000.DLG files were made (one per disk session) and each deleted by exact name after it was the one new
  name. Two of them had filled the disk for a short while (M2a, M2b).
- **Calibrator:** back on the 10 Hz square, enabled; registers equal to the start dump except 0x2C (live) and
  0x0E / 0x2E 0x1A -> 0x19 (the read-only live value of the f0d21eb note). During the check: 24 V and 5 V DC.

## ab7184c: switch latency on the device (2026-10-24 04:15 - 05:35)

The coordinator had installed ab7184c (rust-fw-poc: the fastswitch branch, docs/switch-latency.md 6: the new
function's frame at once, provisional readings in grey, ohm entry 150 ms, V / A entry 300 ms, manual V range 250 ms,
vet_fresh, the AUTO READ? / MEAS? wait). `*IDN?` = `Alientek,DM40A,<UID>,dm40-fw 0.1`, `DBG:FLASH?`
`EF4018,4096,0,...`. INFO/FIRMWARE.BIN, read with the page cache dropped, is byte-equal to the image (sha256
89eebc77…18cc). `DBG:VDISK:MIR?` `1,4,1,5297` (WRITTEN on this first boot, 5.30 s). Crash count 0, marker VALID,
HEALTHY at the start and the end. No flashing. **Quiet rule (owner asleep):** `SYST:ALER:ALARm?` OFF and
`SYST:BEEP:VOL?` 0 at the start and the end, no sound tests (the "Leads in A jack?" reminder's beep after every
A -> V switch played at volume 0). Files are in the session scratchpad's `chk6/`: `settle.py` (the f0d21eb script,
three cases appended, `ONLY=` filter), `shots2.py` (one value-strip grab per switch at a fixed delay, delays swept,
pixels classed amber / grey / red), `extra.py` (frame time on the dash rows, READ? after CONF:RES, HOLD while AUTO
decides), `coup.py`, `fetchtl.py`, `autofetch.py` (AC+DC / AUTO entry), `rng30.py` (items 3 / 4), `holdrel*.py`,
`sentE.py`, and `out/` (`settle_<tag>.json`, `shots_<tag>/`, `coup_*.json`, `rng_*.json`, `E_*.json`, PNGs). The
calibrator was found on the 10 Hz square (func 2, 10 000 mHz), enabled; the meter on the DMM page, DC V AUTO+, RATE
FAST. A jack not wired (A reads ~0).

### Latency: first valid FETCh? after the switch (ms), f0d21eb vs ab7184c

settle.py unchanged (FETC? every ~10 ms after the timed command, 2 reps; in parentheses: within 0.1 % of the final,
where it differs). The f0d21eb column is `chk3/out/settle_v1.json` (1 V, FAST). The DC V -> RES rows here are on
the calibrator's **voltage** output, as in the baseline (a resistance of a source: the end value is ~50 Mohm).

| switch | f0d21eb 1 V FAST | ab7184c 1 V FAST | 1 V MED | 5 V FAST | 5 V MED |
|---|---|---|---|---|---|
| DC V -> AC V | 460 / 462 (2468 / 2461) | 311 / 307 (2351 / 2464) | 307 / 311 (2464 / 2351) | 311 / 311 (2889 / 2674) | 307 / 307 (2674) |
| AC V -> DC V | 457 / 460 | **305 / 305** | 305 / 305 (305 / 409) | 305 / 305 | 305 / 305 (968 / 408) |
| DC V -> RES (V source) | **2672 / 2672** | **319 / 319** (28.2 Mohm first; 50.18 Mohm from 403) | 569 / 569 | OL at 168, one 17.99 Mohm at 518, then OL | OL throughout |
| RES -> DC V | 558 / 554 | **349 / 354** | 319 / 319 | 319 / 319 | 319 / 319 |
| DC V -> A, `CONF:CURR:DC 10` (manual 10 A range: 650 ms, unchanged by design) | 758 / 758 | 756 / 756 | 671 / 666 | 756 / 756 | 671 / 666 |
| DC V -> A, A button tap (`DBG:TAP 103,297`) | - | **396 / 357** | 310 / 311 (554 / 311) | | |
| DC V -> A, `CONF:CURR:DC` (no range) | - | 356 / 352 | 312 / 312 | | |
| DC V -> mA (`CONF:CURR:DC 0.06`) | 557 / 559 | **355 / 349** | 306 / 310 | 355 / 349 | 311 / 306 |
| AC V -> AC+DC | 1372 / 1377 | 1374 / 1374 | 1378 / 1379 | 1380 / 1373 | 1381 / 1379 |
| DC V 6 -> 60 V manual | 509 / 557 | **358 / 363** | 375 / 374 | 318 / 358 | 376 / 374 |
| DC V 60 -> 6 V manual | - | 353 / 356 | 369 / 374 | 354 / 353 | 369 / 369 |
| DC A -> DC V | - | 355 / 350 | 310 / 310 (526) | 355 / 349 | 310 / 310 (516) |
| DC mA -> DC V | - | 354 / 354 | 309 / 309 | 349 / 356 | 309 / 309 |

- V / A / mA entries and exits: 0.55-0.76 s -> 0.31-0.40 s at FAST, 0.31 s at MED, as the emulator (0.32-0.35 s).
  `CONF:CURR:DC 10` keeps 0.76 s: it selects the manual 10 A range after the entry (dmm.rs `bt`/SCPI A branch, 650
  ms row write), as designed ("manual A ranges keep theirs"); the A button and `CONF:CURR:DC` take 0.36-0.40 s.
- The AC V tail of a DC input is unchanged (2.35-2.89 s to 0.1 %); the first AC value comes 0.15 s earlier.
- AC V -> AC+DC is unchanged (1.37-1.38 s), as the emulator.
- The A jack is not wired: every A entry read 0.000 (|I| < 1e-4 A) with no OVERLOAD and no red pixels in the shots.

### Ohm entry on a resistance (calibrator resistance output, 390 / 100 ohm; 3 reps)

| input | rate | first valid FETC? (ms) and its value | correct (0.1 %) from (ms) |
|---|---|---|---|
| 390 ohm (reads 392.6) | FAST | 344 (**8.78 Mohm**, then +9.91E37), 1070 (**219.4**), 1072 (**219.6**) | 1383 / 1164 / 1165 |
| 390 ohm | MED | 1110 (**126.0**), 1105 (**122.4**), 1105 (**125.8**), each held ~0.8 s | 1916 / 1908 / 1913 |
| 100 ohm (reads 99.95) | FAST | 1070 (99.92), 1070 (**57.2**), 1070 (**57.2**) | 1070 / 1163 / 1162 |
| 100 ohm | MED | 1315 (99.94), 1102 (**30.0**), 1105 (**33.5**) | 1315 / 1901 / 1917 |

The screen (shots2.py, 390 ohm): "- - Mohm" in grey from the first grab, then **7.136 Mohm / 7.110 Mohm in amber at
1.0-1.6 s** (FAST) and **12.065 Mohm at 1.3 s, 126.13 / 122.44 ohm at 1.6-2.0 s** (MED), 392.6 ohm from 1.6 s (FAST)
/ 3.0 s (MED) (`shots_r390f/DCV-RES.png`, `shots_r390m/DCV-RES.png`). `CONF:RES` then `READ?` at once: FAST 221.95
ohm at 1.07 s, 9.549 Mohm at 0.37 s; MED 12.19 Mohm at 0.37 s, 392.58 ohm at 1.31 s (`out/extra_read.log`). So the
host gets the wrong values too. New bug 1 below.

### Screen timing

- **Frame:** a 4-row read of the dash rows (y 106-121) right after the command (DC V -> RES, RES -> DC V, DC V -> mA)
  already holds the grey dashes at 0.00-0.06 s, every delay (`out/extra_frame3.log`): the frame is drawn in the
  same loop pass as the switch. The f0d21eb screen was blank until 2.0-2.3 s (RES) / 0.5-1.0 s.
- **First number** (one 0.28 s top-down strip grab per switch, 1 V FAST, `shots_v1f/`): DC V -> mA, A -> DC V, AC V
  -> DC V, DC V -> AC V, RES -> DC V: dashes in the 0.3 s grab, the number in the 0.5 s grab; DC V -> A (10 A manual)
  in the 0.8 s grab. DC V 6 -> 60 V: the old amber value to 0.2 s, a grey held "1.000" at 0.3 s (vet), amber 0.999
  at 0.5 s.
- **Placeholders, never 0.0000:** every switch showed two grey dashes (VALUE_PROV 144,152,152), never a zero. They
  read well: mid-height, centred, clearly "no value". The grey provisional digits (AC+DC, AUTO, the 6 -> 60 V vet
  hold) are the full digit size, legible, and cannot be taken for the amber value or the red >= 30 V colour.
  Cosmetic: `CONF:CURR:DC 10` from DC V shows the placeholder with the **mA** unit for 0.8 s, then "A" with the
  value (`shots_v1f/DCV-A.png`): `select(8)` enters mA, the 10 A row follows.

### PASS / FAIL (TODO "Switch latency" device retest)

| # | check | result |
|---|---|---|
| 1 | DC V -> RES: frame / first value / no range stuck | **Frame PASS** (dashes at <= 0.06 s). **Time PASS** (V source 2672 -> 319 ms FAST / 569 MED; 390 ohm correct at 1.16-1.38 s FAST, 1.91 s MED). **Values FAIL**: wrong values (8.8 / 7.1 / 12 Mohm, 219 / 126 / 57 / 30 ohm) shown in amber and answered to FETC? / READ? before the right one (new bug 1). An open input was not available (the calibrator disabled is no open) |
| 2 | DC V <-> A / mA, RES -> DC V, AC V -> DC V vs f0d21eb | **PASS**: 0.31-0.40 s (was 0.46-0.76 s), except `CONF:CURR:DC 10` 0.76 s (manual range, by design). A first reading 0.000, no OVERLOAD |
| 3 | Manual 6 <-> 60 V at 1 V, 30 switches, FAST and MED (`rng_man_f`, `rng_man_m`) | **PASS**: first FETC? 307-398 ms (FAST), 308-508 ms (MED); every value 0.9987-1.0003 V; no 25-51 % and no 0.767-0.790 V reading |
| 4 | `VOLT:RANG 600` -> `VOLT:RANG:AUTO ON`, 8 times FAST and MED (`rng_auto_f`, `rng_auto_m`) | **PASS**: ends on 6 V every time; values 0.9916-1.0033 V; never 0.767-0.790 V |
| 5 | AC+DC entry from DC V: 1 V DC, 10 Hz and 40 Hz square | **Display PASS** at 1 V DC: placeholder at 0.2 s, grey \|DC\| 0.9996 with "AC --" at 0.6 s, amber 0.9992 at 1.5 s (`shots_acdc_dc1/DCV-ACDC.png`). 40 Hz: grey through 3 s (AC part settling, 6.118 V grey); FETC? +9.91E37 until 4.72-4.77 s, then 6.79 V; READ? after CONF answers at 2.36 s +9.91E37 "Data stale" (the documented SCPI_QUICK rule). **Observation** 10 Hz: amber 5.355 V (AC 4.742 + DC 2.487) at 1.5 s and READ? 5.32 V at 1.28 s on the 6 V range for a 0 / 9.5 V square, then the range step (FETC? +9.91E37 at 2.27 s, grey 7.856 V at 3 s): an early "settled" AC+DC value before the overrange step, probably not new (the DC part of a 10 Hz square at FAST jumps 2.5-7 V) |
| 6 | AUTO coupling | **MEAS:VOLT:AUTO? PASS**: from DC V 1279 ms (1 V DC, DC), 3452 ms (10 Hz, AC 4.784 V), 3910 ms (40 Hz, the tie cap: DC 4.806 V); a later MEAS? 640-849 ms; never "Data stale". **Grey PASS**: the DC part grey with "AC --" until the decision (10 / 40 Hz grey at 3 s). **FETCh? FAIL vs the doc**: it never answers +9.91E37 while AUTO decides: it blocks (sent 0.5-3.0 s after `VOLT:COUP AUTO`, answered at 3.17-4.90 s; the first query after the command blocks until the DC phase's first reading, 1.28 s) and returns the grey DC part (new bug 2). **HOLD after the decision PASS**: amber 0.9994 frozen for 4 s while the input went 1 V -> 10 Hz square, no grey. **HOLD while AUTO decides FAIL** (new bug 3). **REL**: cannot engage before the decision (`rel_on` needs `last_raw`; provisional readings never set it), by design; after the decision REL showed the relative value in amber, no grey (its range hold gives OL on the 9.5 V square, expected) |
| 7 | No false V-JACK? / A-JACK / AC? after RES -> A / mA and Scope -> DMM: seqE1 + seqE2 at 1 V and 5 V, 70 switches (`E_e1_v1`, `E_e2_v1`, `E_e1_v5`, `E_e2_v5`) | **PASS for RES -> A / mA** (no V-JACK? in DC A, 12 entries) and DC V (no AC?). Known: V-JACK? in AC A (2a, 1.72 s), DC? in AC V (correct), A-JACK in RES at 5 V (6 of 6; now from **0.46-0.55 s**, was 2.86-3.01 s: the ohm entry is earlier; silent, alarm Off). **New (5 V only, 2 of 2): V-JACK? for 0.5 s at 8.06 s after A / mA -> Scope -> DMM** (new bug 4) |
| 8 | Screenshots of placeholders and grey values | **PASS** (legibility above); cosmetic mA unit on `CONF:CURR:DC 10` |

### New bugs

1. **Wrong ohm values during the ohm range hunt are shown and sent as final.** DC V -> RES on the calibrator's 390 /
   100 ohm: 8.8 / 7.1 / 12 Mohm on the high ranges, then 219 / 126 / 57 / 30 ohm on the right range, in amber and to
   FETC? / READ?, for one conversion (FAST) or ~0.8 s (MED), before the right value (1.16-1.92 s). The same shape
   on stiff sources: 1 V gives 28.2 Mohm then 50.18 Mohm; 5 V gives 17.99 Mohm between two OLs. f0d21eb hid all of
   it behind its 2.5 s entry deadline. `poll_ohm` shows the first conversion whose q is in range (`ohm_settled`
   after one 150 ms wait), with no vet of it after a range step. The calibrator simulates the resistance actively,
   so part of it may be its own lag; the stiff-source cases point at the meter (input / AD1 settling after a
   range step). Needs a real 4.7 kohm / 100 ohm / 10 Mohm to tell; a fix would vet the first in-range conversion
   after each ohm range step (as VET_N for V), or show it grey.
2. **FETCh? while AUTO decides is not +9.91E37.** switch-latency.md 6 and scpi.rs say FETCh? is +9.91E37 until the
   first decision. On the device it blocks and then answers the provisional DC part: `meter.shown()` is None, so
   main.rs parks it in `scpi_wait` (deadline + SCPI_WAIT_MS), and the wait expiry answers `auto_prov()`. A FETC?
   poller sees 1-3 s blocks. Either the doc or the code is wrong.
3. **RUN (HOLD) while AUTO decides** (10 Hz square, `VOLT:COUP AUTO`, RUN at 1 s; seen 2 of 2): the status bar stays
   "AUTO+ RUN", the grey provisional DC value (7.033 / 7.187 V) and "AC --" freeze for 8+ s, the decision does not
   come; the second RUN press gives the decided value at once (amber 6.855 V, AC 4.780 V) (`hold3_*.png`,
   `hr_hold_during_*.png`). Expected: either HOLD shown (label) with what it froze, or the press ignored.
4. **A 0.5 s V-JACK? at 8.06 s after A / mA -> Scope -> DMM with 5 V on V-COM** (2 of 2; not at 1 V; 612c45c S1 had
   none). ctsta reads 0x70 from 0.52 s to **7.75 s** (612c45c: to 6.34 s), 0x50 to 8.01 s, then 0; the 0x70 tail now
   outlasts the 7 s OHM_EXIT cap, the streak counts from 7.15 s and reaches 10 at 8.06 s; the alert clears at 8.5 s
   with the 5 V still there. So it is the restart tail, not a steady detection of the source.
5. Cosmetic: `CONF:CURR:DC 10` from DC V shows the placeholder with the mA unit for ~0.8 s.

### State at the end

- **Device:** ab7184c, DMM page, DC V AUTO+ (`VOLT:RANG:AUTO?` 1, `VOLT:COUP?` DC), RATE FAST, the bargraph row
  (`e_dmm.png`). `OUTP?` 0 throughout (never switched on). `SYST:ERR?` "No error", `SYST:ALER?` NONE,
  `SYST:ALER:ALARm?` OFF, `SYST:BEEP:VOL?` 0, `DBG:VDISK:MIR?` 1,4,1,5297. Crash count 0, marker VALID, HEALTHY.
- **Settings:** 1/3 and 3/3 byte-equal to the start (`s_set*.png` vs `e_set*.png`); 2/3 differs only in the live rate
  digits (x 277-329, y 48-67). Dim / Shutdown Never, backlight 10 %, volume 0 %, Start page Multimeter, DMM row
  Bargraph, USB disk "Files live" (`DBG:VDISK:RO?` 1,1), Danger alarm Off.
- **Disk:** `MMEM:CAT?` equals the start string exactly. No files were created.
- **Calibrator:** back on the 10 Hz square, enabled. Registers equal to the start dump except 0x2C (live) and
  0x0E / 0x2E 0x18 -> 0x17 (the read-only live value). Right after the restore 0x0C read 0x2711 and 0x23 0x31C9
  (as on f0d21eb); 15 min later both matched. During the check: 1 V, 5 V DC, 390 / 100 ohm, the 10 / 40 Hz square.

## 0774e70: FREQ first reading, mirror pacing, sentinel2 (2026-10-24 05:37 - 06:50)

The coordinator had installed 0774e70 (rust-fw-poc; code equal to 7bd7f6f: freqfirst 36ceea4, mirror-bw, sentinel2).
`*IDN?` = `Alientek,DM40A,<UID>,dm40-fw 0.1`, `DBG:FLASH?` `EF4018,4096,0,...`. INFO/FIRMWARE.BIN, read with the page
cache dropped, is byte-equal to the image (614 016 B, sha256 c039aae1…f432). `DBG:VDISK:MIR?` `1,4,1,5223,0,0,0`
(WRITTEN on this first boot, 5.22 s). Crash count 0, marker VALID, HEALTHY at the start and the end. No flashing.
**Quiet rule (owner asleep):** `SYST:ALER:ALARm?` OFF and `SYST:BEEP:VOL?` 0 at the start, during and at the end;
no sound tests; the danger alarm was never set to Loud. Files are in the session scratchpad's `chk7/`: `freq1.py`
(prev -> 3 s -> CONF:FREQ, 5 READ? with times, odd reps change the view first), `fscope.py`, `ftl.py`, `fshot.py`
(one shot per entry at a fixed delay), `powtl.py` (FETC:POW:ALL? timeline), `sessM.py` / `sessB.py` (chk5's, plus
DBG:VDISK:MIR? per round and the LOG:DISK? timeline), `wv.py` (plus DBG:PROF?), `spoll.py` (commands, then DBG:SENT?
/ SYST:ALER? / CONF? / FETC? polls), `sent5.py` (step 5: DBG:SENT:REG? before / after, raw ctsta), `sentE.py`, and
`out/` (JSON, PNG; `out/ref/` = the 8 INFO files read at the start with no stream). The calibrator was found on the
10 Hz square, enabled; the meter on the DMM page, DC V AUTO+, RATE FAST. A jack not wired.

### FREQ first reading (TODO "FREQ first-reading" device retest 1-6; 40 Hz square)

| # | check | result |
|---|---|---|
| 1 | DC V, 3 s, CONF:FREQ, 5 READ? x10, odd reps after a DMM <-> combined view change (4.4-4.6 s before) (`freq1_dcv.json`) | **FAIL, 10 of 10**: READ? at 1.37 / 1.71 / 2.06 s = **0.000 Hz**, 40.0017 Hz from 2.42 s; the same on both pages, with or without the view change. f0d21eb gave 40.0017 at 1.37 s in most entries (`chk4/out/fentry_*.json`) |
| 2 | In Hz: Scope page 2 s -> back to DMM / combined, 3x each (`fscope_*.json`) | **FAIL, 6 of 6**: 0.000 / 0.000 / 0.000 Hz at 0.16-0.87 s, 40.0017 from 1.21 s |
| 3 | AC V -> Hz and AC+DC -> Hz, 5x each (`freq1_acv.json`, `freq1_acdc.json`) | **FAIL, 10 of 10**: the same three 0.000 readings, 40.00 from 2.42 s. Hz -> Hz (no row write) is 40.00 at once |
| 4 | AC V on the square: the secondary line (shots at 0.6-4.0 s, `sheet_acv2.png`, `sheet_acv3.png`) | **PASS**: "--Hz --%" at 0.6 s, "40.00Hz 50.0%" from 0.7 s, never a wrong value |
| 5 | CONF:POW, POW:COUP AC (`powtl_a.json`, `powtl_rng.json`, `pow_5.0.png`, `pow_7.png`) | **PASS**: F (FETC:POW:ALL? field 7) +9.91E37 then 40.0017 one poll later (5.07 -> 5.17 s, after the I ranging); after `CONF:POW 60` (V range MAN 60) F stays 40.00, no wrong value. Screen: F "--" then "40.0 Hz" |
| 6 | Capacitance 1 uF / 100 nF entries | **Not run**: no capacitor on the bench (the calibrator has no C output) |

The screen during the zeros (`dcv_hz_s16_1.6.png`): **"0.000 Hz" and "0.0 %" in amber**, and MIN / MAX / AVG start
at 0.000, so the statistics hold a false 0.

**Cause (from the code, consistent with the fixed pattern; not probed on the device):** `poll_freq_mode` alternates
the two counter sources (0xF0, then 0x08). A result with f = 0 is not a glitch (`freq_glitch` needs f > 0), so it
sets `seen[i]` and `last[i] = 0`; `freq_change` ranks it 1e6. On this input source 0 (0xF0) reads 0 Hz and 0 %
duty (CTB = CTC = 0) in its first gates: read 1 (source 0 = 0, alone) shows 0; read 2 (source 1's first result,
change 1e6) ties with source 0's 1e6, and the tie goes to source 0 -> 0; read 3 (source 0 = 0 again, 1e6) -> 0;
read 4 (source 1, change 0) wins -> 40.00. The emulator's counter model gives both sources 40 Hz, so freqtest
passed. A fix would treat f == 0 like a glitch for `seen` (a source with no edges is no result), and/or let a
tie at 1e6 go to the non-zero source. Whether source 0 keeps reading 0 after the entry (the steady 40.00 then
comes from source 1 alone) is not known; a DBG counter readout per source would tell.

### mirror-bw (TODO "Device retest of mirror-bw" 1-6; the 10 Hz square on V, DC V)

| # | check | result |
|---|---|---|
| 1 | 76800 `LOG:DUR 0` (until full) disk session, cp of the 8 INFO/*.BIN every 30 s from state 3 (`m768.json`) | **PASS**: state 3 at 40.6 s, full (state 6) at 169.7 s; **9 820 200 rows, lost_before 0**, gate 1 (76800 kept), late 0, overruns 0, dropped 0, max_lat 280 ns. 5 rounds, 40 copies byte-equal: FIRMWARE.BIN 1.71-1.74 s (612c45c: 2.0 s), LOADER.BIN 0.68-0.71 s (> 0.5 s, as expected), the rest <= 0.11 s. DBG:VDISK:MIR? per round: +505 mirror sectors, +1 072..1 133 holds, longest hold 11 ms |
| 1' | LOADER.BIN 6 s after state 3, 20 s 76800 session (M2c's case, `c768L.json`) | **PASS**: lost 0 (612c45c: 21 745), 1 536 002 rows, copy 0.70 s, byte-equal |
| 2 | LOADER.BIN at state 3 = inside the writer trial, 3 runs (`trial1..3.json`) | **PASS**: gate 1 every time (no 38400 fallback; 612c45c fell back), lost 0, 1 536 002 rows; the copy takes 1.65-1.67 s; longest hold now **50 ms** (the "pressed" cap), ~12 900 holds per run |
| 3 | LOG:DISK? field 14 at 76800, no copy (`nocp768.json`) | median **508 permille** (min 487 in state 3 after the trial, max 526) = headroom ~492 (emulator: 589 / 411). With copies 491-506. One read of 1000 at 1 s after state 3 in `c768L` (the trial window's end), not repeated |
| 4 | 38400 `LOG:DUR 240`, cp rounds every 60 s (`m384.json`) | **PASS**: 9 216 006 rows (240 s), lost 0, late 0, overruns 0; 4 rounds byte-equal, FIRMWARE.BIN 0.86-0.87 s, LOADER.BIN 0.32 s (612c45c M2a: 1.06 / 0.51 s); field 14 ~268 |
| 5 | Wave view (DC V, MF > Wave) at 1 ms/div (ACQ:SRAT? 76 800), 200 s, cp rounds every 40 s (`wv.json`, `wv_end.png`) | **PASS**: DBG:ISR? **dropped 0** over 200 s (612c45c: ~50/s); late 0, overruns 0, max_lat 300 ns; 5 rounds byte-equal (FIRMWARE.BIN 1.9-2.3 s). DBG:PROF?: loop max **17.5 ms** (< 26), no pass >= 20 ms; s.paint max 14.6 ms, 12.5 fps |
| 6 | Host kernel log (`journalctl -k`; dmesg is not readable here) 05:52-06:10 | **PASS**: only the " sda:" rescans after each session's file create / delete; no usb-storage reset, timeout or sense line |

Every session's DLG (LOGS/FAST000.DLG) was the only new name and was deleted by exact name. The first 76800
attempt did not start because the meter was in the power meter (CONF:POW) when LOG:STATe ON came: state stayed 0
with no SYST:ERR? entry left (the script read it first); the scripts now stop after 10 s of state 0. Not checked
further.

### sentinel2 (TODO "Device retest (sentinel2 image)" 1-8; A jack open, calibrator on V-COM)

"Open" V-COM is not available on this bench; rows marked "off" are the calibrator with its output disabled.

| # | check | result |
|---|---|---|
| 1 | AC A 10 / 0.06 / 0.0006, 30 s each, at 1 V, 24 V DC and "off" (`sp_s1_*.json`) | **No false V-JACK? at 1 V / off: PASS** (9 of 9 quiet). **The AD1 peak is not constant: FAIL vs the criterion.** At 1 V and off pkhmin = pkhmax = **-262 144** (the rail) from the first fresh read (~0.9 s) on. At 24 V it **ramps** from ~-48 000 / -98 000 (0.5-0.9 s) to the rail by 3.3 s; the ramp's swing in one hold (9 000-12 000 codes) passes `PKV_SWING_THR` (4 270): **V-JACK? at 1.5-3.5 s in 60 mA and 600 uA** (then gone), not in 10 A (swing ~5 000, streak broke) |
| 1' | `DBG:SENT:VAR 32` (the old SENSE - PB5 row), 1 V, 10 A (`sp_s1_var32_v1_10.json`) | the rail from the first fresh read, no V-JACK?: with the swing rule the old row no longer alarms either, so this does not separate PB5 from AGND. VAR 0 after |
| 2 | AC on V: the calibrator's 0 / 9.5 V square, 10 Hz (10 A / 60 mA / 600 uA) and 40 Hz (10 A), 15 s each (`sp_s2_*.json`) | **PASS**: V-JACK? at 1.87 / 2.12 / 2.07 / 1.87 s, held to the end; pkhmax - pkhmin ~15 000-16 000 codes at 10 Hz (pk ~-196 000 / -180 000); at 40 Hz the peaks walk up to +217 000 / +230 000 |
| 3 | 1 V and 5 V: CONF:CURR:AC 10, 2 s, CONF:CURR:AC 0.06, x5 each, 4 s at 50 ms (`sp_s3_*.json`) | **PASS**: CONF? stays 60 mA 10 of 10, no OVERLOAD, no +9.9E37. Observation: for ~0.9 s FETC? still answers the 10 A row's last value (e.g. 1.90E-02), then one 60 mA reading of the same mantissa / 100 (1.90E-04): the filter's carry-over |
| 4 | Real overload through A | **Skipped** (needs a wire) |
| 5 | 24 V: DC A (`CONF:CURR:DC 10`) after DC V 60 V, RES, TEMP, AC V, DC V (auto, 60 V), Scope -> back; DBG:SENT? every ~70 ms for 12 s (`sent5_p0.json`) | **REG? the same 48 digits in every DC A entry**: `10DD1583 35000055 31000000 10000881 80C73EAC 00500000` (relay 0, mode 7, range 1; emulator's `...1783 35010055 ... 3DA4` differs in R22 / R25 / R34-35 but is constant too): no register carries the history, confirmed on the device. a_blind 1 / 1 / **0** / 1 / 1 / 1. ctsta raw: DC V 60 V -> 0 from 0.69 s (missed); RES -> 0x70 to 7.07 s, 0x50, 0 from 7.31 s (missed); **TEMP -> 0x70 throughout, V-JACK? from 7.98 s**; AC V -> 0 (one 0x50 at 0.69 s; missed); DC V -> 0 (missed); Scope -> back (entered DC A after DC V) -> 0x70 to 8.26 s, 0x50, 0 from 8.44 s (missed) |
| 5' | `DBG:SENT:PREC` 100 / 300 / 1000 ms, 24 V (`sent5_p100/p300/p1000.json`) | **The precondition brings V-JACK? back in every case**: DC V 60 V -> DC A: ctsta 0x70 from 0.81 / 1.01 / 1.71 s, V-JACK? at **1.90 / 2.10 / 2.80 s**; RES -> DC A: 0x70 throughout, V-JACK? at **1.89 / 2.09 / 2.79 s**; Scope -> back (DC A entered with PREC, a_blind 0): 0x70 throughout, V-JACK? at 8.74-8.77 s (the restart mask). 100 ms is enough. No false hit: calibrator off, RES / DC V 60 V -> PREC 300 / 1000 -> DC A: ctsta 0 throughout, no alert (`sent5_off_p*.json`); **5 V**, PREC 300: ctsta 0 from ~1 s, no alert; 5 V after TEMP (PREC 0): 0 from 0.7 s (`sent5_v5_*.json`). PREC 0 after. Each PREC run pulses the relay (a click, not the buzzer) |
| 6 | Continuity, 5 V DC, 30 s (`sp_s6_cont_v5.json`) | pkhmin / pkhmax **-99 100 / +124 800** (f0d21eb: -97 000 / +123 000); A-JACK badge from 0.42 s, held; soft 1 |
| 7 | Danger alarm (left **Off**, per the quiet rule): RES and continuity at 5 V 30 s; E1 subset at 5 V / 1 V / off (`sp_s7_res_v5.json`, `E_e1_5/1/off.json`) | **PASS within what Off can show**: DBG:ALARm? `0,0,0,2` before and after every run (events 0); DBG:SENT? last field **1** in every RES / continuity row, 0 elsewhere. A-JACK in RES at 5 V 6 of 6 (0.47-0.61 s, CMP_PB1; known owner item), none at 1 V / off; no V-JACK? in DC A after RES. With the alarm Off the events counter cannot show a Loud event, so the soft flag is the evidence |
| 8 | 24 V, TEMP 3.5 s -> AC+DC A (`CONF:CURR:ACDC 10`) and -> AUTO A (`CURR:COUP AUTO`), 20 s at 100 ms (`sp_s8_*.json`) | **PASS**: V-JACK? from 8.29 / 8.78 s and held through every later phase change (CMP_SENSE <-> AD1PK_SENSE every ~1 s), no flapping. Without TEMP (after DC V auto 60 V): never seen in 20 s (the step 5 blindness) |

**What decides the next fix (step 5):** the 24 V miss is the SENSE comparator's history, not a register: the same
row reads 0x70 after TEMP (or after the PREC temperature row, 100 ms is enough) and 0 after DC V 60 V / AC V / a
restart, while 5 V reads 0 either way and an open input stays 0. Making the precondition the default (100-300 ms,
on entry with a_blind) is supported by this data; its cost is the relay pulse and the blocked loop. The 7-8 s to
V-JACK? when 0x70 is present from the start (TEMP, Scope -> back with a_blind 0) is the restart / exit mask, not
the comparator.

### New bugs

1. **FREQ: three 0.000 Hz readings (~1 s) after every Hz entry** (DC V / AC V / AC+DC -> Hz, Scope -> back), shown
   in amber, answered to READ?, and kept in MIN / AVG. Worse than f0d21eb (40.00 at 1.37 s). Cause above (f = 0
   counts as a clean result of source 0; ties go to source 0).
2. **AC A at 24 V DC: a 2 s V-JACK? in 60 mA / 600 uA** (1.5-3.5 s after the entry): the SENSE - AGND peak still
   ramps to the rail at 24 V and the ramp's swing within one hold passes the swing threshold. At 1 V / off the
   peak sits at the rail (-262 144) from ~0.9 s, so the AC A sentinel's AD1 is at the rail in the normal case too;
   whether the swing rule still sees mains on V with the peak near the rail is answered only for the calibrator's
   square (step 2: yes).
3. Minor: a disk session started (LOG:STATe ON) while the meter is in the power meter stays in state 0 with no
   error left in SYST:ERR? (seen once; the script consumed the error queue, not checked further).

### State at the end

- **Device:** 0774e70, DMM page, DC V AUTO+ (`VOLT:RANG:AUTO?` 1, `VOLT:COUP?` DC), RATE FAST, the bargraph row
  (`e_dmm.png`). LOG: AD1 / 4800 / HOST / DUR 60 / STAT 0. `DBG:SENT:PREC?` 0, `DBG:SENT:VAR?` 0. `OUTP?` 0
  throughout (never switched on). `SYST:ERR?` "No error", `SYST:ALER?` NONE, `SYST:ALER:ALARm?` OFF,
  `SYST:BEEP:VOL?` 0, `DBG:ALARm?` 0,0,0,2, `DBG:VDISK:MIR?` 1,4,1,5223,8108,52402,50. Crash count 0, marker VALID,
  HEALTHY.
- **Settings:** 1/3 and 3/3 byte-equal to the start (`s_set*.png` vs `e_set*.png`); 2/3 differs only in the live
  rate digits (x 277-329, y 48-67). Dim / Shutdown Never, backlight 10 %, volume 0 %, Start page Multimeter, DMM row
  Bargraph, USB disk "Files live" (`DBG:VDISK:RO?` 1,1), Danger alarm Off.
- **Disk:** `MMEM:CAT?` equals the start string exactly. 8 LOGS/FAST000.DLG files were made (one per disk session)
  and each deleted by exact name after it was the one new name; two filled the disk for a short while (m768, m384).
- **Calibrator:** back on the 10 Hz square, enabled. Registers equal to the start dump except 0x2C (live) and 0x01
  0x1057 -> 0x1056 (not written by the tools; read-only). During the check: 1 V, 5 V, 24 V DC, output off, the
  10 / 40 Hz square.

## 213f9aa: FREQ first reading retest (2026-10-24 09:55 - 10:12)

The coordinator had installed 213f9aa (rust-fw-poc, freqfirst2). `*IDN?` = `Alientek,DM40A,<UID>,dm40-fw 0.1`,
`DBG:FLASH?` `EF4018,4096,0,...`, INFO/FIRMWARE.BIN (read after `POSIX_FADV_DONTNEED`) byte-equal to the image
(614 016 B, sha256 8c1563ad…0544), `DBG:VDISK:MIR?` `1,4,1,5301,0,0,0`. No flashing. Quiet rule kept:
`SYST:ALER:ALARm?` OFF, `SYST:BEEP:VOL?` 0 at the start and the end, no sound tests. Files in the session
scratchpad's `chk8/` (`fq.py`: PREV -> 3 s -> CONF:FREQ, 5 READ? with times, then CALC:AVER MIN / MAX / AVG / COUN;
odd reps change the view first, `back` mode = in Hz, Scope 2 s -> back; `nosig.py`, `offt.py`, `sback.py`,
`powshot.py`, chk7's `fshot.py` / `powtl.py`) and `chk8/out/`. Signal: the calibrator's 40 Hz square on V-COM.

| # | check | result |
|---|---|---|
| 1a | DC V -> Hz x10, odd reps 3.0 s after a DMM <-> combined view change (`fq_dcv.json`) | **PASS**: 50 of 50 READ? 40.0017-40.0018 Hz, the first at **1.37-1.40 s**; MIN / MAX / AVG 40.0017 / 40.0018 / 40.0018, COUN 5 |
| 1b | DC V -> Hz x4, odd reps 3.0 s after Scope -> back (`fq_dcv_scope.json`) | **PASS**: 20 of 20 40.00 Hz, first at 1.37 s |
| 1c | In Hz: Scope 2 s -> back, 3x on the DMM page, 6x on the combined view (`fq_back_dmm.json`, `fq_back_combo*.json`) | **PASS**: 45 of 45 40.00 Hz; first READ? at **0.86 s** (DMM) / 1.04-1.05 s (combined), counted from the tap; MIN stays 40.0017 |
| 1d | AC V -> Hz x5 (odd reps after Scope -> back) and AC+DC -> Hz x5 (odd reps after a view change) (`fq_acv.json`, `fq_acdc.json`) | **PASS**: 50 of 50 40.00 Hz, first at 1.37-1.40 s, MIN / MAX / AVG 40.00 |
| 1e | Screen at 0.5 / 1.0 / 1.5 s after DC V / AC V / AC+DC -> Hz (`sheet_entry.png`); Scope -> back (`sheet_back.png`) | **PASS**: "- -" placeholder (grey), no %, MIN / MAX / AVG empty at 0.5 and 1.0 s; "40.002 Hz", "50.0 %", MIN / MAX / AVG 40.002 at 1.5 s. Never "0.000 Hz" / "0.0 %". Scope -> back keeps the last 40.002 shown (bar greyed at 0.5 s) |
| 2 | Calibrator output off, DC V -> Hz x3, AC V -> Hz x2 (`nosig_dcv.json`, `nosig_acv.json`, `nosig_01.5.png`, `nosig_02.6.png`) | **PASS**: first READ? **0.000 Hz at 2.42 s**, no hang, `SYST:ERR?` clean; screen "- -" at 1.5 s, "0.000 Hz" / "0.0 %" at 2.6 s |
| 3 | In Hz on 40 Hz, calibrator off at t = 2 s, FETC? every 50 ms, 12 runs (`offt_a.json`, `offt_b.json`) | **PASS (0.000 Hz 0.45-1.02 s after the off, 12 of 12)**. Observation: in 3 of 12 one value of **3.19 / 6.07 / 6.03 Hz** at +0.30-0.36 s, held ~0.7 s before the 0.000 (the gate that straddled the off) |
| 4a | AC V on the 40 Hz square, secondary line at 0.5 / 0.7 / 1.0 / 1.5 / 3.0 s (`acv2_*.png`, `sheet_reg.png`) | **PASS**: "--Hz --%" at 0.5 s, "40.00Hz 50.0%" from 0.7 s |
| 4b | CONF:POW, POW:COUP AC (`powtl_a.json`, `powac_*.png`) | **PASS**: F +9.91E37 to 5.08 s, 40.0018 from 5.18 s (as 0774e70); screen F "--" at 1.0 / 4.5 s, "40.0 Hz" at 6.5 s |

**Verdict:** the 0774e70 failure (three 0.000 Hz / 0.0 % readings after every Hz entry) is gone: 165 of 165
READ? in steps 1a-1d are 40.00 Hz and the first comes at the f0d21eb time. The remaining item is the partial-gate
value when the signal stops mid-gate (3 of 12, ~0.7 s): a real count of the edges before the off, not a
`Freq::take` rule break; whether it should be dropped (e.g. a value < 1/2 of the last one followed by a dead gate)
is the owner's call.

**State at the end:** DMM page, DC V AUTO+ (`VOLT:RANG:AUTO?` 1, `VOLT:COUP?` DC), RATE FAST (`e_dmm.png`).
`OUTP?` 0 throughout, `SYST:ERR?` "No error", `SYST:ALER?` NONE, `SYST:ALER:ALARm?` OFF, `SYST:BEEP:VOL?` 0,
`DBG:ALARm?` 0,0,0,2, `DBG:VDISK:MIR?` 1,4,1,5301,0,0,0, `DBG:VDISK:RO?` 1,1, `DBG:CRASH?` empty. Settings 1/3 and
3/3 byte-equal to the start; 2/3 differs only in the live rate digits (x 277-329, y 48-67): Dim / Shutdown Never,
backlight 10 %, volume 0 %, Start page Multimeter, DMM row Bargraph, USB disk "Files live", Danger alarm Off.
`MMEM:CAT?` equal to the start string; no files made. Calibrator back on the 10 Hz square, enabled; registers equal
to the start dump except 0x2C (live).

## 10242c3: sentinel3 retest (2026-10-24 11:30 - 13:22)

The coordinator had installed 10242c3 (sentinel3 rebased onto rust-fw-poc 92ef9f6, so with the FREQ fix): the DC A
precondition on by default (100 ms), AC A V-JACK? needs a reversal, FETC? +9.91E37 when the shown reading is not
from the current row (power-meter-and-misconnect.md 4.12 on that branch). `*IDN?` = `Alientek,DM40A,<UID>,dm40-fw
0.1`, `DBG:FLASH?` `EF4018,4096,0,...`, INFO/FIRMWARE.BIN (read after `POSIX_FADV_DONTNEED`) byte-equal to the image
(614 016 B, sha256 4619f403…2821), `DBG:VDISK:MIR?` `1,4,1,5287,0,0,0`, `DBG:SENT:PREC?` 100, `DBG:SENT:VAR?` 0. No
flashing. Quiet rule kept: `SYST:ALER:ALARm?` OFF and `SYST:BEEP:VOL?` 0 checked at the start and the end of every
script, no sound tests. Each blind DC A entry pulses the relay (by design, doc 4.12; not audible over the link). The session paused
11:53 - 13:03 (an API limit on the agent side); the meter and its state were not touched in between except by the
coordinator's status check.

Files in the session scratchpad's `chk9/` (the chk7 scripts were gone, so these are new): `dev.py` (link,
calibrator, MENU -> tile navigation, DBG:LCD? shots), `sentE.py` (item 1: previous function, then the timed DC A /
mA entry, DBG:SENT? + SYST:ALER? every ~70 ms for 12-14 s, DBG:SENT:REG? before and at 0.6 s, the V-JACK? latency
from DBG:SENT:LOG? against DBG:PING? uptime), `settle.py` (item 2: FETC? every ~10 ms), `acA.py` (items 3 / 4),
`fetc5.py` and `fshot5.py` (item 5), `ma1st.py`, `ma2.py`, `rng06.py` (the DC mA findings), `start.py`,
`setshots.py`, `endstate.py`, and `out/` (JSON per run, logs, PNGs). The calibrator was found on the 10 Hz square
(func 2, 10 000 mHz), enabled; the meter on the DMM page, DC V AUTO+, **RATE SLOW**. The checks ran at **RATE FAST**
(as every baseline) unless noted. A jack not wired. "Off" = the calibrator's output disabled.

| # | check | result |
|---|---|---|
| 1 | 24 V on V-COM, DC A (`CONF:CURR:DC`) after DC V 60 V (manual), RES, AC V, DC V auto (60 V), AC V -> Scope 2 s -> back -> 3 s / 9 s -> DC A, and DC A -> Scope -> back; 2 reps; mA (`CONF:CURR:DC 0.06`) 1 rep (`sentE_v24A*.json`, `sentE_v24mA.json`) | **PASS**. a_blind 1 before every blind entry, 0 at 0.6 s (the precondition ran). V-JACK? (DBG:SENT:LOG? time - uptime at the command) in A: DC V 60 V **2.76 / 1.88 s**, RES 2.16 / 2.16, AC V 1.76 / 1.85 (not blind; the comparator history of the previous entry), DC V auto **2.78 / 2.80**, AC V -> Scope -> back, 9 s, DC A 1.88 / 1.88; mA: 1.41-1.53 s. Restart mask cases: entering DC A 3 s after Scope -> back gives 4.98 / 4.98 s (A) / 5.03 s (mA) = 8 s after the back; back into DC A from Scope 8.03 / 8.03 s (0774e70: 8.7 s). The 0774e70 misses after DC V 60 V / RES / AC V / DC V / Scope are gone. Observation: the DC V 60 V / auto entries at 2.8 s (3 of 4) show ctsta 0 in the first polls and 0x70 from 0.44 s: the first comparator read after the precondition sometimes still reads 0, which costs ~0.9 s |
| 1' | 5 V and off: the same entries (DC V 60 V manual, RES, AC V -> Scope -> back + 9 s for the blind cases; AC V, DC V auto as controls), A and mA (`sentE_v5*.json`, `sentE_voff*.json`) | **PASS**: no alert in 14 of 14 (no V-JACK?, nothing else in DBG:SENT:LOG?); streak 0 throughout; ctsta 0 after <= 0.4 s |
| 2 | First valid FETC? after the DC A / mA entry at 1 V: blind (after RES, PREC 100), the same with `DBG:SENT:PREC 0`, and non-blind (DC A once, then DC V 6 V); FAST x4, SLOW x2 (`settle_fast.json`, `settle_slow.json`) | **PASS**. FAST, A: blind **463 ms** (426-497) vs PREC 0 369 (331-397) vs non-blind 360 (331-392): **+94 ms**. mA (`0.06`, manual range): blind **540** (430-621) vs PREC 0 348 vs non-blind 362: **+192 ms**. SLOW: A blind 686 / 422 vs PREC 0 321 / 667 vs non-blind 312 / 307; mA 420 / 421 vs 320 / 320 vs 311 / 311. The cost is on blind entries only, as expected (~+0.1-0.2 s); non-blind is the ab7184c time (352-356 ms) |
| 3 | AC A from DC V: 10 A / 60 mA / 600 uA, 30 s each, at 24 V, 1 V and off (`acA_v24/v1/voff.json`) | **PASS**: no V-JACK? and no other alert in 9 of 9 (0774e70: 1.5-3.5 s in 60 mA / 600 uA at 24 V). pkhmin / pkhmax after 1 s: at 1 V / off the rail -262 144 throughout; at 24 V the ramp from ~-103 000 to the rail, as before, now without the alert |
| 3' | The square on V: 10 Hz (10 A / 60 mA / 600 uA) and 40 Hz (10 A / 60 mA), 15 s each (`acA_sq10.json`, `acA_sq40.json`) | **PASS**: V-JACK? at **1.80 / 1.86 / 1.94 s** (10 Hz) and 1.79 / 1.65 s (40 Hz), held to the end (185-189 of the ~212 polls). 0774e70: 1.87-2.12 s, so the reversal rule cost no time here |
| 4 | `DBG:SENT:VAR 64` (AD1 at IG 0.9), AC A 10 A / 60 mA / 600 uA, 15 s each, 1 V / off / 24 V (`acA_var64_*.json`) | **SENSE sits at ~-86 500 codes (~-0.46 V with the 1.39 V full scale) in every A range, at 1 V and with the source off alike**: -80 000 at 0.5-1.5 s, creeping to -86 200..-86 800 by 10-15 s; pkhmax - pkhmin ~100 codes. At 24 V it starts at ~-32 000 (-0.17 V) and decays to the same -86 600 (-60 000 at 1.5-3 s, -80 800 at 3-6 s). So at IG 3.6 (full scale 0.347 V) the node is below the rail in every case; the 24 V input only adds a slow transient. One spike to -216 968 in 600 uA at 1 V. No alert. `DBG:SENT:VAR 0` after (VAR? 0) |
| 5 | `CONF:CURR:AC 10`, 2 s, `CONF:CURR:AC 0.06`, FETC? every 50 ms, 5 reps at 1 V and 5 V (`fetc5_v1/v5.json`); screen strip at the first answer, 6 reps at 1 V (`fshot5.log`, `f5_sheet.png`) | **Carry-over PASS, +9.91E37 FAIL, first value FAIL**. The 10 A value no longer carries over. **+9.91E37 is never answered**: the first FETC? after the change **blocks 0.4-0.9 s** (as the AUTO case of ab7184c bug 2), then answers a 60 mA-row value. And that value is **9.3-43.6 mA on the open jack in 9 of 16 entries** (fetc5 5 of 10: 26.6 / 29.1 / 30.9 / 35.2 / 43.6 mA, fshot5 4 of 6: 9.27 / 26.64 / 26.64 / 32.15 mA; the 10 A row read 0.2-0.6 mA there). It is **shown in amber** ("26.636 mA", `f5_2.png`), held until ~0.8 s and gone by ~1.0 s through 1e-2, 1e-4 (an EMA decay), then ~1e-6 A. In the other 7 the first value is 0 .. 1.6e-7 A. The 0774e70 "1 % of the 10 A value" is gone; what is left looks like the restarted AC filter seeded by an unsettled first conversion after the range relay step |

### New bugs

1. **AC A 10 A -> 60 mA: a 9-44 mA first reading on the open jack** (item 5, 9 of 16), in amber and answered to
   FETC?, for up to ~0.5 s. With the filters now restarting on a range step, the first AC conversion after the
   step seeds them. The guard (no reading from the old row) works. Dropping the first conversion(s) after a range
   step (or marking them provisional) would fix it; not checked in the emulator, which gives a fresh first reading.
2. **DC mA entry at MED / SLOW: a first reading of +-33..56 mA on the open jack** (`ma1st_v1.json`: MED 3 of 12
   entries from DC V 6 V, e.g. +43.8 / -56.5 / +54.3 mA at 0.31 s; `settle_slow.json`: SLOW 2 of 2 from DC V 6 V,
   -32.6 / -37.8 mA), then ~1e-6 A from the next reading (0.2 s later). None in 36 FAST entries. Also seen once for
   `CONF:CURR:DC` (auto) at MED: +9.9E37 (OVERLOAD) at 0.31 s in the 600 mA row, then 60 mA, then **+0.23 mA in
   the 600 uA row** at 1.2 s (`ma2_v1.json`); at SLOW three auto entries read +-0.22..0.24 mA at 1.1-1.2 s (`ma1st`).
3. **`CONF:CURR:DC 0.06` (manual 60 mA) ends on the 10 A row** at MED / SLOW: `ma2_v1.json` 3 of 6 (CONF?
   `CURR +1.000000E+01` from 0.57-0.62 s and kept), `rng06.json` 1 of 5 at SLOW (0 of 5 at FAST), with OVERLOAD
   (kind CMP_SENSE) in DBG:SENT:LOG? at those times: the bogus first mA reading of bug 2 most likely trips the mA
   overload step to 10 A. The range the user asked for is lost.
4. Minor: the restart mask (8 s after Scope -> back) also delays V-JACK? for a DC A entry made within those 8 s
   (4.98 s when 3 s after the back), although the precondition has already run and ctsta reads 0x70 from the first
   poll.

### State at the end

- **Device:** 10242c3, DMM page, DC V AUTO+ (status bar "AUTO+"; `VOLT:RANG:AUTO?` 1, `VOLT:COUP?` DC), **RATE
  SLOW** (as found), the bargraph row (`e_dmm.png`). `OUTP?` 0 throughout (never switched on). `LOG:STAT?` 0.
  `DBG:SENT:PREC?` 100, `DBG:SENT:VAR?` 0. `SYST:ERR?` "No error", `SYST:ALER?` NONE, `SYST:ALER:ALARm?` OFF,
  `SYST:BEEP:VOL?` 0, `DBG:ALARm?` 0,0,0,2, `DBG:VDISK:MIR?` 1,4,1,5287,0,0,0, `DBG:VDISK:RO?` 1,1. `DBG:CRASH?` is
  not a command in this build: it answers a bare newline and queues `-100 "Undefined header"` (rechecked at the
  end; the error was read and the queue is clean).
- **Settings:** 1/3 and 3/3 byte-equal to the start (`s_set*.png` vs `e_set*.png`); 2/3 differs only in the live
  rate digits (x 299-308, y 50-65; 9.8/s vs 9.7/s): Dim / Shutdown Never, backlight 10 %, volume 0 %, Start page
  Multimeter, DMM row Bargraph, USB disk "Files live", Danger alarm Off (`e_sheet.png`).
- **Disk:** `MMEM:CAT?` equal to the start string; no files made (screens are DBG:LCD? reads).
- **Calibrator:** back on the 10 Hz square, enabled; registers equal to the start dump except 0x2C, 0x0E and 0x2E
  (live). During the check: 1 V, 5 V, 24 V DC, output off, the 10 / 40 Hz square.

## AD1 OSR characterization (10242c3) (2026-10-24 14:33 - 15:45)

The numbers the DC rate / filter decision needs (dmm-rate-filter-survey.md §8.3 tests 1-4, option E). Reads
only: AD1 streams through `LOG:SOURce AD1` + `LOG:OSR` / `LOG:CHOP` (no firmware change, no flashing), the
calibrator on V-COM, DC V on the **6 V range, manual** (`VOLT:RANG 6`) for every stream. The meter ran 10242c3
(`*IDN?` `Alientek,DM40A,<UID>,dm40-fw 0.1`). It was found on the DMM page, DC V AUTO+ (on the 60 V range,
hunting on the 10 Hz square), **RATE MED**, Danger alarm **Loud** and key / alert volume **50 %** (the owner's
settings: read, never changed; the run stayed in DC V, so no misconnect alarm). `OUTP?` 0 throughout. Files in
the session scratchpad's `osr/`: `st.py` (stream helper: one session per CSV, the LOG:PREamble?, LOG:COUNt? and
DBG:ISR? in its header), `model.py` (the dc-aliasing.md kernel), `lvl.py`, `fr.py`, `sqmean.py`, `settle.py`,
`drift.py` (the runs), `an_lvl.py`, `an_fr.py`, `an_fr2.py`, `an_st2.py`, `an_drift.py` (analysis), and `out/`
(one CSV per stream, logs, the analysis outputs `an_*.txt`, PNGs).

**Scale.** Raw codes are converted with the meter's own calibration: (READ? at 5 V - READ? at 0 V) / (the 0x17
code difference) = **1.6755 uV per code** on 6 V (nominal 1.389 V x 10 / 2^23 = 1.6557, +1.2 %); one display
count (100 uV) = 59.7 codes. 1 V reads 597 000 codes, as expected. The codes R22 = 0x15 / 0x16 / 0x17 are
OSR 8192 / 16384 / 32768 chopped, 0x06 / 0x07 OSR 16384 / 32768 unchopped. Measured periods: 100.782,
**200.789** (new; not in dc-aliasing.md), 400.784, 99.995, 200.000 ms.

**Logger quirk (not a converter effect).** Some sessions repeat each conversion: the record holds the same code
twice, 50-80 ms apart (`fr_40_15.csv` right after a 640/s session: 121 copies in 294 records; `fr_40_16.csv` 149
of 289; `fr_60_16.csv` 18). That is the main-loop sampler's rescue read (logger.rs `sample`) after `p_meas`
locked below the period. The analysis drops a record equal to its predecessor; the conversion count then
matches the session length / period (e.g. 140 for 28 s at 0x16). In those sessions the time stamps are not
conversion times. dc-aliasing.md's "duplicated records at --osr 4 / 6" is the same thing.

### 0 V: the calibrator cannot make a short

No jumper is on the bench, so there is no real short. Two stand-ins: the calibrator at 0.000 V, output on ("0
V"), and its output disabled ("off"). "Off" is the quieter one at every code (0x07: 33 vs 116 uV sd; 0x15: 40
vs 58 uV), and 0 V drifted: 0x15 read +160 uV more at the end of its 4-min block than at the start. So the
noise floor below comes from "off" (the calibrator's disabled output; its impedance is not known), and offsets
are taken from "off" as well.

### 1. Frequency response (the calibrator's square, 0 / 9.586 V)

The calibrator's frequency output is a square only (func 2, frequencies in mHz); there is no sine. Its levels,
from 640/s unchopped streams at 2.5 / 40 Hz: low 0.024 V (the OSR-256 offset), high 9.609 V, i.e.
**A = 9.586 V** (a 0.1 V overshoot on the rising edge, ~10 ms). Its mean also changes with each re-setting by
up to ~13 mV (4.736-4.818 V between settings), so the mean error below is measured against the in-session mean
of the codes that null that frequency (minus each code's offset vs 0x17 from the 40 Hz square, where all null),
not against an absolute mean. 2.5 and 16.7 Hz have no nulling code; there the reference is the mean of the
codes whose alias beats more than 20 times in the window. Stream lengths: 20 s (0x15, 0x06), 30 s (0x16, 0x07),
40 s (0x17), longer where the model beat is slow (0x16 at 5 Hz 115 s, 0x17 at 2.5 Hz 125 s); the first 2 s of
each session are dropped.

p-p = the reading's peak-to-peak over the window (V), in brackets the dc-aliasing.md kernel's p-p over all
phases for this square (chopped: the mean of two sinc^2 conversions one measured P apart; unchopped: one
sinc^2 per conversion). Mean error in mV; "(n beats)" = the window covered only n beats of a slow alias, so
that mean is biased by the unfinished beat, not a converter error.

| f, Hz | 0x15 p-p (model) | err | 0x16 p-p (model) | err | 0x17 p-p (model) | err | 0x06 p-p (model) | err | 0x07 p-p (model) | err |
|---|---|---|---|---|---|---|---|---|---|---|
| 2.5 | 9.583 (9.584) | +5.9 | 0.076 (0.077) | -7.3 | 3.515 (4.793) | (0.6 beats) | 9.255 (9.585) * | +9.2 | 4.617 (4.793) * | -7.8 |
| 5 | 0.151 (0.152) | +0.5 | 4.793 (4.792) | (2.2 beats) | 0.0003 (0) | 0.0 | 2.620 (4.793) * | -7.1 | 0.0003 (0) | 0.0 |
| 10 | 4.791 (4.792) | (2.3 beats) | 0.0004 (0) | +0.05 | 0.0003 (0) | 0.0 | 0.0004 (0) | +0.06 | 0.0003 (0) | -0.12 |
| 16.7 | 0.301 (0.302) | +0.2 | 0.148 (0.149) | -0.5 | 0.0485 (0.0488) | +0.1 | 0.330 (0.330) | 0.0 | 0.0856 (0.0854) | +0.1 |
| 20 | 0.0004 (0) | +0.2 | 0.0003 (0) | +0.1 | 0.0003 (0) | -0.1 | 0.0004 (0) | 0.0 | 0.0003 (0) | -0.2 |
| 25 | 0.0298 (0.0300) | +0.2 | 0.191 (0.191) | (2.7 beats) | 0.0003 (0) | +0.2 | 0.191 (0.192) | -0.1 | 0.0003 (0) | -0.2 |
| 40 | 0.0006 (0) | 0 (ref) | 0.0003 (0) | 0 | 0.0003 (0) | 0 | 0.0005 (0) | 0 | 0.0005 (0) | 0 |
| 50 | 0.191 (0.191) | +0.5 | 0.0003 (0) | +0.2 | 0.0003 (0) | 0.0 | 0.0005 (0) | 0.0 | 0.0004 (0) | -0.2 |
| 60 | 0.0003 (0) | +0.1 | 0.0003 (0) | +0.1 | 0.0002 (0) | 0.0 | 0.0004 (0) | 0.0 | 0.0004 (0) | -0.1 |

\* An exact sub-multiple of the unchopped rate (0x06 at 2.5 / 5 Hz, 0x07 at 2.5 Hz): the conversions lock to
fixed phases of the square, so the p-p depends on that phase; the model column is the maximum over all phases.
0x17 at 2.5 Hz: the alias is 0.0049 Hz (a 203 s beat); 123 s showed 3.5 V of the predicted 4.79 V p-p.

**The model holds at every code**: the p-p of all 45 streams agree with the kernel within 1 % (or within
0.6 mV where the model says 0), the 10 Hz beat of 0x15 included (4.791 V p-p). Each code leaks 0.405 of the
fundamental at the frequency equal to its reading rate (0x15: 10 Hz, 0x16 / 0x06: 5 Hz, 0x17 / 0x07: 2.5 Hz)
and that leak beats near DC. Where a code nulls the input the reading's mean is right within +-0.2 mV
(0.004 %, at the 0x07 / 0x15 noise). Where it leaks with a fast alias (16.7 Hz, 0x15 at 25 / 50 Hz) the mean is
also right within 0.5 mV, but the reading itself swings by the p-p.

**50 / 60 Hz rejection** (leak = p-p / 2 against the fundamental's 6.10 V peak). ">=": the p-p is at the floor of
this setup, which is the 40 Hz square (every code nulls it): 0.3-0.6 mV p-p at ~4.8 V, the calibrator's level
noise plus the converter's; the true rejection is at least the figure given:

| | 0x15 | 0x16 | 0x17 | 0x06 | 0x07 |
|---|---|---|---|---|---|
| 50 Hz | **36.1 dB** (model 36) | >= 92 dB | >= 94 dB | >= 89 dB | >= 90 dB |
| 60 Hz | >= 91 dB | >= 93 dB | >= 95 dB | >= 90 dB | >= 90 dB |

Only 0x15 leaks mains (50 Hz, -36 dB, a 0.19 V p-p beat at 0.39 Hz for this 9.6 V square). The others are at
the noise floor; the sinc^2 nulls are exact (crystal-timed), so the real rejection is set by how far the
mains frequency is from 50.000 / 60.000 Hz, not measured here (the calibrator's frequency is accurate to
~40 ppm).

### 2. Noise per code (6 V range, 195-230 conversions each)

sd and p-p over 18-83 s (the first 2 s dropped); counts = 100 uV display counts. "x sqrt(T)" = sd x sqrt(1 /
rate), the noise for equal measuring time.

| code | rate | off: sd uV (counts) | off: p-p uV | off: sd x sqrt(T), uV.s^0.5 | 0 V on: sd / p-p uV | 1 V: sd / p-p uV | 5 V: sd / p-p uV |
|---|---|---|---|---|---|---|---|
| 0x15 | 9.92/s | 39.7 (0.40) | 245 | 12.6 | 58 / 283 | 126 / 648 | 139 / 746 |
| 0x16 | 4.98/s | **19.9 (0.20)** | 109 | **8.9** | 20 / 101 | 145 / 811 | 127 / 665 |
| 0x17 | 2.50/s | 17.2 (0.17) | 96 | 10.9 | 20 / 161 | 161 / 754 | 126 / 578 |
| 0x06 | 10.0/s | 35.3 (0.35) | 208 | 11.2 | 56 / 305 | 132 / 650 | 151 / 791 |
| 0x07 | 5.00/s | 33.1 (0.33) | 174 | 14.8 | 116 / 529 | 130 / 603 | 139 / 687 |

In codes: off sd 23.7 / 11.9 / 10.2 / 21.1 / 19.7 (0x15 / 16 / 17 / 06 / 07). At 1 V and 5 V every code reads
125-160 uV sd: that is the calibrator's DC output noise (~25 ppm of 5 V), not the converter, so only the "off"
column measures the converter. There:

- 0x16 halves 0x15's sd (39.7 -> 19.9 uV, the white-noise sqrt(2) would give 28); **0x17 hardly improves on
  0x16** (17.2 uV). For the same measuring time 0x16 is the quietest code; the floor of ~17-20 uV
  (0.2 count) is reached at 0x16.
- Unchopped is about as noisy as chopped at 10/s (0x06 35.3 uV vs 0x15 39.7), and at 5/s 0x07 (33.1 uV) is
  no better than 0x06 and much worse than 0x16 (19.9): low-frequency (1/f) noise the chopper
  removes.
- The +1 digit (10 uV on 6 V) needs sd <~ 10 uV: 0x16 needs ~4 conversions averaged, 0x17 ~3, 0x15 ~16 (if the
  noise averages down as white; not checked beyond these windows).

### 3. Scale across codes, under one calibration

Each level ran the codes back to back (0x15, 0x16, 0x17, 0x06, 0x07, 0x15 again), ~4.5 min per level. Mean
differences vs 0x17, uV:

| level | 0x15 | 0x16 | 0x06 | 0x07 | 0x15 repeat |
|---|---|---|---|---|---|
| off | -3.0 | -2.0 | **+22 222** | **+22 233** | -5.0 |
| 0 V | +34.3 | +1.9 | +22 237 | +22 530 | +194.2 |
| 1 V | -33.6 | -109.5 | +22 227 | +22 345 | +41.2 |
| 5 V | +102.0 | +12.0 | +22 271 | +22 211 | +28.4 |
| 4.81 V (40 Hz square, §1) | -140 | -130 | +22 240 | +22 400 | |

Fit over 0 / 1 / 5 V (code mean = a x 0x17 mean + b): gain 0x15 +19 ppm, 0x16 +10, 0x06 +8, 0x07 -55, 0x15
repeat -25 ppm; residuals up to 72 uV. The 0x15 repeat moves by up to 160 uV against the first 0x15 in the same
block, so the calibrator's drift over a block (+-100 uV at 0-5 V) sets the resolution: **no gain difference
between the five codes is resolved above ~50 ppm** (0.005 %, 2.5 counts at 5 V).

**The unchopped offset is an offset, not gain: +22.22 mV at 0 V (off), +22.23 mV at 1 V, +22.27 mV at 5 V** (0x06;
0x07 the same within its noise). That is the +22 mV dcalias saw at 4.74 V; on the 6 V range it is 222 counts,
13 260 codes, i.e. ~2.2 mV at the AD1 input. The chopped codes agree with each other at "off" within 5 uV.

### 4. Settling

**0 -> 5 V step** (and 5 -> 0 V), `set_value` on the calibrator with its output left on, 3 steps per code at
different phases. The calibrator itself, seen at 640/s: 10-90 % in ~40 ms, within 5 mV after ~190 ms (up) /
~240 ms (down), and a slow tail: -2.1 mV at 0.4 s, -0.9 mV at 0.8 s, -0.25 mV at 3 s. Conversions after the
first one that moved (mV vs the end of the plateau, the three steps):

| code | conversion 1 (straddles) | conversion 2 | conversion 3 | conversion 4 | conv. 3 comes after |
|---|---|---|---|---|---|
| 0x15 | -3060..-4605 | -567..-2144 | -8..-41 | -2.6..-3.5 | 0.2 s |
| 0x16 | -2605..-4955 | -107..-2477 | -1.7..-22 | -0.9..-1.5 | 0.4 s |
| 0x17 | -3111..-4979 | -612..-2485 | -0.9..-5.1 | -0.4..-0.7 | 0.8 s |
| 0x06 | -3741..-4357 | -342..-780 | -6.7..-11 | -2.5..-2.7 | 0.2 s |
| 0x07 | -3916..-4986 | -330..-2361 | -2.2..-16 | -0.7..-1.4 | 0.4 s |

(Down steps alike, mirrored.) At every code the first two conversions after a step are partial: the chopped
kernel is two sinc^2 conversions one period apart, and the unchopped sinc^2 spans two periods, so neither
settles faster per conversion. The third conversion's residual is the calibrator's own ~20 ms edge and tail
(larger at the fast codes, which sample the tail earlier). So: **first good reading = the third conversion
after the step**, 0.2-0.3 s at 0x15 / 0x06, 0.4-0.6 s at 0x16 / 0x07, 0.8-1.2 s at 0x17.

**Range change.** A range command during a session is refused (`VOLT:RANG 60` mid-stream: `-100 "Settings
conflict"`, the session ran on), so this measures the session start on 6 V with 5 V applied, either straight
(already on 6 V) or right after `VOLT:RANG 60` -> `VOLT:RANG 6` (the session started immediately after the range
command), 2 reps each. Conversions 1, 2, 3 vs the settled value (mV):

| code | 6 V -> session | 60 V -> 6 V -> session | 3rd conversion at |
|---|---|---|---|
| 0x15 | +4540, +4540, 0.1 | -11000..-11700, -8700..-9500, 0.1 | 0.33 s |
| 0x16 | 0.1, 0.0, 0.1 | -2800..-4100, -560..-1890, 0.4 | 0.5-0.7 s |
| 0x17 | 0.4, 0.4, 0.3 | -2970..-3520, -720..-1270, 0.3 | 1.0 s |
| 0x06 | +9030, -10040, 0.1 | -3040..+5070, -2020..-2690, 0.2 | 0.2-0.36 s |
| 0x07 | -2510..-2530, 0..-11, 0.0 | -2550..-4580, -11..-1130, 0.2 | 0.44-0.56 s |

**A range change costs two conversions at every code**: after 60 V -> 6 V the first two are wrong by volts
(the second still by 0.01-2.7 V), the third is within 0.4 mV. Without a range change a session start at 0x16
/ 0x17 is clean from the first conversion; at 0x15 / 0x06 / 0x07 its first one or two records are stale codes
(+4.5 V, +9 V, -2.5 V off; the previous session's AD1 state), so the logger alone does not separate those two
cases there. The DMM path has its own discard rules, not measured here.

### 5. Option E: unchopped 0x06 as FAST

Noise and rejection are in §1-2: at 10/s, 0x06 is about as quiet as 0x15 (35.3 vs 39.7 uV sd, 0.35 vs 0.40 count)
and rejects 50 Hz by >= 89 dB instead of 36 dB; its blind spot moves from 10 Hz to 5 Hz (0.405 leak). Gain
equals the chopped codes within the ~50 ppm resolution of §3. What is left is the offset.

**Offset drift**: 10 blocks of 0x06 (28 s) and 0x16 (28 s) alternated for 9.5 min, calibrator output off,
6 V range (`drift.py off 10`, `an_drift.txt`; an earlier start at "0 V on" was stopped after 1.5 blocks because
that source drifts, `drift_0on_aborted.log`):

| | first block | last block | range over 9 min | sd of the block means |
|---|---|---|---|---|
| 0x06 (unchopped) | +22 242.9 uV | +22 205.4 uV | 39 uV | 11 uV |
| 0x16 (chopped) | -13.6 uV | -21.7 uV | 22 uV | 6 uV |
| 0x06 - 0x16 | 22 256.5 uV | 22 227.1 uV | 31 uV | 9.9 uV |

Block-mean noise alone would give ~2-3 uV, so the unchopped offset wanders by ~10 uV (0.1 count) on the scale of
minutes, about twice as much as the chopped one. A **periodic zero** (each 0x06 block corrected by the mean of
the neighbouring 0x16 blocks, the offset learnt once) leaves **6 uV sd, 20 uV range** (0.2 count); a constant
learnt once leaves 39 uV (0.4 count) over these 9 minutes. Both are far below one count; the 22.2 mV itself
(222 counts) must be subtracted. Not measured: the offset on the other ranges (only 6 V was run; if it is
the AD1 input's ~2.2 mV it scales with the range decade), its temperature coefficient (the meter sat at room
temperature, powered for hours), and the offset of the DMM path itself (the streams bypass dmm.rs).

**So E works** on these numbers, with a per-range offset subtraction that a periodic chopped reference keeps
within 0.2 count; a one-time constant would do over minutes at constant temperature, but nothing here bounds
it over temperature.

### Verdict for the survey (§8.3 summary table there)

| | B: FAST 0x15 / MED 0x16 / SLOW 0x17 (+ filter) | E: FAST 0x06 unchopped |
|---|---|---|
| 50 / 60 Hz | 0x15 36 dB / >= 91 dB; 0x16, 0x17 >= 92 dB both | >= 89 / >= 90 dB |
| leaks (0.405) at | 10 / 5 / 2.5 Hz | 5 Hz |
| noise, off, sd | 40 / 20 / 17 uV (0.40 / 0.20 / 0.17 count) | 35 uV (0.35 count) |
| scale vs 0x17 | gain within ~50 ppm, offset within 5 uV | gain within ~50 ppm, **offset +22.2 mV** (stable to 39 uV / 9 min; 6 uV sd with a 0x16 zero) |
| first good reading after a step / range change | 3rd conversion: 0.2-0.3 / 0.4-0.6 / 0.8-1.2 s | 3rd conversion: 0.2-0.3 s |

- **B is supported by every measurement**: the kernel model predicts all 45 streams, 0x16 nulls 10 Hz and
  both mains frequencies, is the quietest code per unit time, needs no recalibration (same scale as 0x15 /
  0x17), and settles in 3 conversions. 0x17 buys little noise over 0x16 (17.2 vs 19.9 uV), so SLOW's value is
  the 2.5 Hz-multiple nulls, not resolution.
- **E is feasible**: same noise as 0x15 at the same 10/s, mains rejected, but only with a per-range offset
  subtraction (+22.2 mV on 6 V); a periodic 0x16 zero holds it to 0.2 count. Temperature and the other ranges
  are untested.

### State at the end

- **Device:** 10242c3, DMM page, DC V AUTO+ (`VOLT:RANG:AUTO?` 1, `VOLT:COUP?` DC; the range hunts 6 <-> 60 V
  on the 10 Hz square, as at the start), RATE **MED** (as found). LOG settings as found: AD1, OSR 3, CHOP 1,
  SRAT 4800, DUR 60, DEST HOST, STAT 0. `OUTP?` 0 throughout. `SYST:ERR?` "No error", `SYST:ALER?` NONE,
  `SYST:ALER:ALARm?` LOUD and `SYST:BEEP:VOL?` 50 (the owner's, never written), `DBG:ALARm?` 0,0,0,0,
  `DBG:SENT:PREC?` 100, `DBG:SENT:VAR?` 0, `DBG:VDISK:MIR?` 1,3,0,303,0,0,0 (unchanged), `DBG:VDISK:RO?` 1,1.
  Every `osr/out/start.json` query answers the same in `end.json` except `CONF?` / `VOLT:RANG?` (the live
  autorange range).
- **Settings:** 1/3 and 3/3 byte-equal to the start (`s_set*.png` vs `e_set*.png`); 2/3 differs only in the
  live rate digits (x 266-318, y 48-67: "MED 10/s" vs "MED 9.6/s"). Dim screen 5 min, Shutdown 30 min,
  backlight 55 %, key / alert volume 50 %, Bluetooth ON, Start page Multimeter, Danger alarm Loud, USB disk
  "Files live". (The first screenshot try hit the dimmed screen: the first MENU key only woke it.)
- **Disk:** `MMEM:CAT?` equal to the start string; no files made (screens are DBG:LCD? reads).
- **Calibrator:** back on the 10 Hz square, enabled; registers equal to the start dump except 0x2C (live).
  During the check: 0 / 1 / 5 V DC, output off, the square at 2.5-60 Hz.

## v0.1-pre1: fastswitch2, first readings after a switch, the logger copies (2026-10-24 15:30 - 20:40)

The meter ran several images during this session; all hold the fastswitch2 code of v0.1-pre1 (8fc4eaa):
v0.1-pre1 itself (15:30 - 16:20), then the coordinator's `opendm40-trig2.bin`, `opendm40-trig-pinch.bin`
and `opendm40-grab-power.bin` (scope changes on top). Which image each run used is noted per row. The
fixes were then checked on e1cec01 and 83650bc (flashed by the coordinator). `*IDN?`
`Alientek,DM40A,<UID>,opendm40 0.1`. The owner power-cycled the meter in between: every setting went
back to its default (the backup SRAM does not survive a power-off), so the end state is compared with
the state after that. The owner's settings (Danger
alarm Loud, key / alert volume 50 %) were read and left as found. The alarm was set Off only during the
A -> V touch runs of the "Leads in A?" item and set back to Loud right after. The calibrator was found
on the 10 Hz square. The A jack is not wired: every current reading here is on an open jack. Files are
in the session scratchpad's `chk10/`: `dev.py`, `start.py` / `setshots.py` (state), `rep2.py` (task 2
repro), `dcv1st.py`, `osrstep.py`, `t1ohm.py`, `t1auto.py`, `t1vj.py`, `t1fuse.py`, `t1rt.py`, `ajk*.py`,
and `out/` (JSON per run, logs, PNGs).

### Summary

| item | result |
|---|---|
| 1. Ohm vet (DC V -> RES, calibrator 390 / 100 ohm and output off, FAST / MED, 2 reps each) | **FAIL** in 6 of 12 entries (grab-power): a wrong value in amber, also to READ? / FETC?: 390 ohm at MED 125.9 / 122.1 ohm (2 of 2), 100 ohm 57.3 (FAST) and 30.0 (MED), open (~420 kohm) 134 kohm (MED, 2 of 2). Each time the right value came first (grey), then two conversions at ~32 % that agreed with each other, so the vet took the second one. Then the right value. `CONF:RES;:READ?` answered the right value in 0.03-0.34 s (the meter had settled) |
| 2. FETCh? while AUTO decides (`VOLT:COUP AUTO`, 10 Hz square, FAST / MED, 2 reps each) | **PASS**: +9.91E37 in at most 31 ms every time (58-60 polls each) until the decision. Decision (first value) at 5.76-5.85 s in 3 runs, none within 6 s in 1 (grab-power; ab7184c: MEAS:VOLT:AUTO? 1.28-3.91 s) |
| 3. Armed HOLD (RUN 1 s after `VOLT:COUP AUTO`, 10 Hz square, FAST / MED, 2 reps each) | **PASS**: DBG:READout? `G,..,1` from the press. The grey value keeps moving (8-9 values). Where a decision came within 7 s (2 of 4), `F,..,1` with one value to the end. RUN again releases |
| 4. A / mA -> Scope -> DMM with 5 V on V-COM, 10 times, FAST (grab-power) | **PASS**: no V-JACK?, nothing new in DBG:SENT:LOG?. ctsta never read 0x70 or 0x50 in the 12 s after the return (ab7184c: 0x70 to 7.75 s) |
| 5. Placeholder unit (`CONF:CURR:DC 10` / `0.06` from DC V, DBG:READout? from the same message on, FAST / MED x 2) | **PASS**: "A" / "mA" from the first readout |
| 6. Fuse check, 10 times, open jacks (calibrator output off; `CONF:RES 600;:READ?` OL) | **FAIL** (grab-power): OPEN once, then `FAIL,285.83` 9 times (decided 0.85 s after SYST:FUSE:TEST, while the readout still showed a grey OL), and `FAIL,279.31` in a later run. Same cause as item 1 (below). No V/ohm -> A lead is possible on this bench, so the ~48 ohm OK path was not run |
| 7. CONF round trips (16 functions / ranges, `CONF:<f>;:DBG:READout?`, READ?, CONF?, FAST / MED x 2, calibrator off) | **PASS** for CONF? (always the function and range asked for), SYST:ERR? (no error) and the placeholder unit. CONF to READ? answered: DC V 0.40-0.80 s, DC A 10 A 0.71-0.96 s, AC V 0.43 s, AC A 0.43-0.97 s, RES 0.80-1.07 s, CONT / DIOD / CAP 0.41-0.78 s (OL, open), FREQ 2.41 s, TEMP 2.01 s, AC+DC 1.33 s, AUTO 1.28-1.33 s. **Wrong first READ?:** `CONF:CURR:DC 10` at MED +0.117 / +0.120 A on the open jack (2 of 2), `CONF:RES` at MED 135.6 kohm for 423 kohm (1 of 2) |
| Task 2a: AC A 10 A -> 60 mA (FETC? / DBG:READout?, FAST, 1 V on V, 6 reps) | **FAIL**, reproduced (v0.1-pre1): the first 60 mA reading was 28.7-59.5 mA in 6 of 6, final (amber) and to FETC?, which blocked until it came (0.31-0.84 s). Then it fell ~100x per result to ~1e-6 A |
| Task 2b: DC mA entry at MED / SLOW (from DC V 6 V, 1 V on V, 5 reps x 2 rates) | **FAIL**, reproduced (v0.1-pre1): `CONF:CURR:DC 0.06` ended on the 10 A range 5 of 10 (MED 3, SLOW 2), one first reading +53.8 mA; `CONF:CURR:DC` (auto) read OL first in the 600 mA row 3 of 5 at SLOW. FAST, 12 entries: none |
| Task 3: AD1 logger copies | cause found in the code; e1cec01 still made copies in 1 of 3 sessions; 83650bc: none (below) |
| "Current in A jack" after A -> V (owner report) | not A-JACK: the "Leads in A jack? Move to V" toast (below); removed |

### The first conversions after a switch

DC V 6 V entered at MED / SLOW (1 V, `dcv1st.py`, 4 reps per case; v0.1-pre1):

- from DC A: the first reading (0.30 s) was 0.90 / 1.07 / 2.11-2.13 V in 5 of 8, final. The next
  conversion, 10 ms later, was right.
- from AC V and from RES: 1.000 V at 0.30 s, then 0.376-0.405 V twice (0.45 and 0.55 s, the second
  one final), then 1.000 V from 0.96 s. 10 of 16. At SLOW from RES the first value was OL twice.

At FAST none of this showed (no R22 change in the function). The two wrong conversions come right after
the firmware's first OSR write after the entry: `osr` writes 0x17 once two conversions agree at ~0.4 s,
and RES's in-range branch writes the steady OSR. The same OSR steps later in a function did not do it:
DC V 6 V at MED / SLOW, calibrator 1 -> 2 -> 1 V (`osrstep.py`, 8 steps) showed only the input's own
two-conversion settling. Why the chip reads ~32-40 % there is **unknown**; it depends on how soon the
R22 write comes after the row write. The DC A -> DC V and DC mA cases are the entry's own first
conversion, which the switch-latency.md proposal 5 shortcut took unvetted at MED / SLOW (an R22
change at the row write). The ohm case (item 1) and the fuse check (item 6) are the in-range and the
OL (0x13) OSR writes of `poll_ohm`.

AC A 10 A -> 60 mA: the RMS block carries the switch step for up to ~0.85 s, then falls ~100x per
107 ms result. The filters restart on the range step since 0774e70, so the first in-range result seeds
them.

### Fix (e1cec01)

The fix and its tests are in the commit "DMM first readings after a switch" (merged in e1cec01) and are
listed under TODO "First readings after a switch". The emulator tests
[emu/hy3131/firstrdtest.py](../../emu/hy3131/firstrdtest.py) and
[emu/hy3131/logduptest.py](../../emu/hy3131/logduptest.py) fail on v0.1-pre1 (8fc4eaa) and pass on the
fix. The model's knobs that replay the device (`r22part` / `r22off` / `r22n`, `acasteptau`,
`chophalf`; [emu/hy3131/HY3131.cs](../../emu/hy3131/HY3131.cs)) are shapes inferred from the numbers
above, not measured waveforms.

### Logger copies (task 3)

`osr/out/fr_40_15.csv` (10242c3, the session right after a 640/s one): record 1 at 2.1 ms (a stale
code), record 2 at 54.0 ms, then a copy of each record stamped exactly 51.87 ms after it, and the real
records 100.8 ms apart. 51.87 ms is the interval from the session's first flag to the first conversion.
[logger.rs](../src/logger.rs) `sample` took it as `p_meas`. With p_meas at half the period, the
rescue (no flag 1.25 p_meas after the last sample) read the old result once more before every
conversion, and because `rescued != 0` at the next flag, the period was never measured again. The
same goes for 0x16 (149 of 289): its first interval was ~half of 200.8 ms. Fix: the period is measured
between two real flags, from the session's second flag on, whatever rescues came between. Emulator
`logduptest`: 59 / 29 / 60 copies per session on v0.1-pre1, 0 on the fix.

### "Current in A jack" after A -> V

Owner report: a red "check leads" box for ~1 s after tapping A, then V, nothing connected. A -> V by
DBG:TAP (hold 0.3 / 0.8 / 2.5 s, FAST / MED / SLOW, 22 times) and 31 SCPI / touch changes from A,
mA, uA, 10 A, AC A, AC+DC A, RES, CONT and DIOD into DC V: SYST:ALER? NONE every time, DBG:SENT:LOG?
unchanged. DBG:LCD shots at +0.4 / +1.0 / +1.6 s show the toast "Leads in A jack? Move to V" (not at
+2.2 s), and DBG:LEADs? counted each change. The owner had the reminder removed (commit "Remove the
'Leads in A jack? Move to V' reminder").

### Fix on the device (e1cec01, logger 83650bc)

| check | before | e1cec01 |
|---|---|---|
| AC A 10 A -> 60 mA, FAST, 6 reps (`rep2.py a`) | 28.7-59.5 mA final, 6 of 6 | first final value at 1.05 s; max final / FETC? 1.1e-6 A |
| `CONF:CURR:DC 0.06` / `CONF:CURR:DC` from DC V 6 V, MED / SLOW, 5 reps each | 10 A range 5 of 10, +53.8 mA once, OL first 3 of 5 | 10 A range 0 of 20; max final 1.2e-6 A, no OL; first final 0.52-0.72 s |
| DC V 6 V from AC V / DC A / RES at MED / SLOW, 4 reps each (`dcv1st.py`) | wrong final value in 16+ of 24 | 0 of 24; first value 0.41-0.50 s (AC V), 0.53-0.57 s (DC A), 1.65 s (RES) |
| DC V -> RES on 390 / 100 ohm / open, FAST / MED (`t1ohm.py`) | wrong final value 6 of 12 | 0 of 12; first final value FAST 1.72 s, MED 2.6 s (390 / 100 ohm), open 1.0 / 1.9 s |
| Fuse check, open jacks, 10 runs | OPEN 1, FAIL,279-286 ohm 9 | OPEN 10 of 10 (0.83-0.89 s) |
| Fuse check, calibrator 48.0 ohm on V-COM, 10 runs | not run | OK,47.04-47.06 10 of 10 (1.93-1.98 s; `CONF:RES 600;:READ?` 47.05) |
| CONF round trips (`t1rt.py`) | MED CURR:DC 10 first READ? +0.12 A, RES 135.6 kohm once | right values; CONF -> READ? DC V / DC A at MED 1.5-1.8 s (was 0.5-1.0 s) |
| FETC? in AUTO / armed HOLD (`t1auto.py`) | PASS | PASS (+9.91E37 in <= 32 ms; HOLD as before). The decision on the 10 Hz square: 3.4 s to over 6 s, as on pre1; 10 Hz is OSR 8192's alias, so the DC part beats there |
| Logger copies (`logdev.py`: 640/s 2 s, then OSR 8192 / 16384 / 8192 chopped, 20 s each) | 10242c3: 121 of 294 | e1cec01: 0, 0, 16 of 232 (flags 78.4 / 123.2 ms apart in turn); 83650bc, 6 sessions: 0 copies in 998 records |

The fix costs time at MED / SLOW: two dropped conversions after an early R22 change, also where the
device may not need them (TODO "First readings after a switch").

### State at the end

- **Device:** 83650bc (the coordinator's flash), DMM page, DC V AUTO+, RATE MED, Danger alarm Loud, key
  / alert volume 50 % (as found after the reflash). `OUTP?` 0 throughout (never switched on). Every
  `start4.json` query answers the same at the end except `CONF?`'s resolution field (live),
  `DBG:FLASH?`'s last field and `DBG:VDISK:MIR?` (the reflash). `SYST:ERR?` "No error".
- **Settings:** 1/3 and 3/3 byte-equal to the start shots (`s4_set*.png` vs `e4_set*.png`); 2/3 differs
  only in the live rate digits (x 266-297, y 50-65).
- **Disk:** `MMEM:CAT?` equal to the start string; no files made (screens are DBG:LCD? reads).
- **Calibrator:** back on the 10 Hz square, enabled; registers equal to the session's start dump except
  0x2C (live).
