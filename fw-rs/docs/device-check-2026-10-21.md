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
