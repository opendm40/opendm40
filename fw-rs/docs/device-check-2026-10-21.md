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
