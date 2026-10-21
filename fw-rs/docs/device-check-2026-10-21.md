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
