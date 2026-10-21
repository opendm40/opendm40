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
