# Scope lag: suspects, proposed fixes, measurement plan

Static analysis of the owner report "the oscilloscope really lags" / "dragging
the trace lags" (2026-10-13). The profiler that confirms these numbers is
`src/prof.rs` (`DBG:PROF?`, `DBG:PROF:RESET`, commit c85afc1). Every
millisecond figure below is an estimate from the code; nothing was measured on
the device yet. The fixes are proposals; no source was edited.

**Cause.** In RUN mode every drag event restarts the scope acquisition, so
the trace freezes for the whole drag and only redraws after release. Drawing
speed is not the problem. Each Offset, Level or TrigPos drag event goes
`scope_drag` -> `scope_set` -> `Scope::apply` -> `halt()`, which aborts the
DMA and timers, then `start()` on the next poll. The Auto timeout is re-armed
each time, and `scope_invalidate()` forces a full 401-column redraw on the
next frame. Nothing redraws the graph between frames, so during a drag the
trace stands still and then jumps once a record completes after release.

## 1. Ranked suspects (estimates; the DBG:PROF? row that confirms each is in brackets)

1. **Acquisition restart on every drag event (RUN).** [`touch->frame` after a
   DBG:DRAG; fps near 0 during a real finger drag]
   - At 1 ms/div (tb 15: 2.5 MS/s, 25 000-sample screen, 10.3 ms record) a
     hardware-trigger record needs about 5 ms of pre-trigger, then the edge
     (up to 25 ms with the 40 Hz PWM), then 5 ms post-trigger.
   - With touch reports every ~10-17 ms a record almost never completes during
     a drag. The trace jumps 10-35 ms after release, then costs about 6 ms
     more for the full redraw.
   - At 100 ms/div (tb 21 is YT, not roll: 25 kS/s, 1 s record) nothing moves
     during the drag. After release it takes 1-1.5 s, and up to 2 s
     untriggered, because the Auto deadline is 2000 ms for tb >= 19.
   - Offset drags also rewrite the TIM3 offset PWM on every event; how long
     the analog side takes to settle is unknown (measure it).
   - Even with the freeze below, an untriggered drag at slow timebases still
     ends with up to 2 s of waiting after release (one wait instead of endless
     restarts). That overlaps the owner's "Auto is strange" complaint.
2. **Cursor drags in RUN.** [`s.frame` count and max during the drag]
   - CurX/CurY call `scope_invalidate()` on every event. The cursor only moves
     when the next frame arrives, and every frame becomes a full redraw
     (~4.5-6 ms).
   - At 100 ms/div the cursor moves about once per second.
3. **Panning while stopped (TrigPos with an anchor).** [`scope` max and
   `s.meas`/`s.mtxt` during the pan]
   - Each event runs `zoom_frame` -> `frame()` with prev invalidated:
     per-column min/max ~1 ms plus a full redraw ~5 ms.
   - `fresh` is always true when `!run`, so it also runs
     `scope_meas::compute` (~3.5-4 ms over 25 k samples), `cursor_values` and
     the measurement-bar text (~0.5-1 ms).
   - Total about 10-11 ms per event, roughly one touch report period.
4. **Per-frame CPU at 25 000 samples per screen (100 us/div and slower).**
   [`s.acq`, `s.frame`, `scope` avg]
   - `Scope::poll` completion: `hw_linearise` `rotate_left` of the
     26 624-sample buffer ~0.25-0.5 ms, `snapshot_mark` 10-bit mask
     ~0.3-0.4 ms, trigger search ~0.02 ms.
   - The `self.wave` decimation in `App::scope_poll` costs ~0.4 ms per frame
     and is dead code: grep finds no readers in app/bt/scpi/usbtmc/main.
   - `frame()` converts every sample to a row in float: ~8 cycles x 25 k
     ~ 1 ms.
   - Changed columns cost ~12 us each; a full redraw of 401 x 273 px is
     ~4.5-5.5 ms.
   - Altogether about 2.5 ms per frame with a stable trace, up to ~7 ms on a
     full redraw.
5. **Measurements every 300 ms.** `scope_meas::compute` does three passes over
   25 k samples, ~3.5-4 ms, plus `scope_measurements` text ~0.5-1 ms. The
   worst loop pass is therefore ~10-12 ms, which adds 0-12 ms to touch
   latency. [`s.meas`, `s.mtxt`, `loop max`/histogram]
6. **Smaller items.**
   - Per-second status block (`scope_status` + generator chip) ~1-2 ms once a
     second. [`s.status`]
   - `v.labels` (4 chips) ~0.5-1 ms per drag event. [`s.labels`]
   - `dmm.reassert_relay`: a 10 ms blocking call every 30 s on every view when
     anti-drop is on. It should be gated on the DMM rail.
7. **Not the cause.**
   - Touch read ~0.4 ms at 400 kHz (~1.5 ms at 100 kHz or bit-bang), only on
     an INT edge or the 10 ms fallback, one report per pass. [`touch`]
   - USB poll is microseconds. `bt.poll` every 5 ms is microseconds. The BT
     AT init is one-shot.
   - On the Scope page the DMM is suspended (`set_suspended` is a no-op when
     unchanged), so `dmm` costs nothing.
   - On the DMM+Scope page the DMM SPI poll plus strip drawing do run, a few
     ms per reading.
   - Other blocking waits: `halt()` spins on the DMA enable bit with no
     timeout (normally under 1 us); touch I2C flag waits have a 10 ms timeout
     (faults only); `bt.pump` falls back to blocking for good if `tx_dma_ok`
     turns false (DBG:DIAG byte 60); `update_rails` waits 10 ms on a page
     switch.

## 2. Optional profiler hooks (exact code)

Put them in app.rs, not ui.rs or scope.rs: ui-render compiles ui.rs and
scope.rs on the host, where there is no `prof` module.

```rust
// App::scope_poll
crate::prof::enter(crate::prof::ACQ); let polled = self.scope.poll(now); crate::prof::exit(crate::prof::ACQ);
// around the meas compute + cursor_values:
crate::prof::enter(crate::prof::MEAS); /* compute + cursor_values */ crate::prof::exit(crate::prof::MEAS);
// around v.frame(...):
crate::prof::enter(crate::prof::FRAME); v.frame(/* ... */); crate::prof::exit(crate::prof::FRAME); crate::prof::drawn();
// around ui::scope_measurements(...):
crate::prof::enter(crate::prof::MTXT); ui::scope_measurements(&self.meas_sel[..self.meas_n], &self.meas_res); crate::prof::exit(crate::prof::MTXT);
// App::tick, scope arm of the per-second status block:
crate::prof::enter(crate::prof::STAT); ui::scope_status(&st); ui::ScopeView::gen_chip(&self.gen); crate::prof::exit(crate::prof::STAT);
// App::scope_set:
crate::prof::enter(crate::prof::LABEL); v.labels(&self.scope); crate::prof::exit(crate::prof::LABEL);
// lcd::set_window_cmd, first line (cfg-gated because host suites compile lcd.rs):
#[cfg(target_os = "none")] crate::prof::pixel();
```

## 3. Proposed fixes (exact code)

### A. scope.rs: a settings setter that does not touch the hardware

A pure addition, so ui-render is unaffected.

```rust
/// Drag preview: take the position settings into `s` without touching the
/// hardware or the acquisition in flight (same clamps as `apply`); the caller
/// applies them once on release.
pub fn preview(&mut self, s: Settings) {
    self.s.trig_row = s.trig_row.clamp(-10, 290);
    self.s.pos = s.pos.clamp(-392, 392);
    self.s.hpos = s.hpos.clamp(0, COLS as i32);
}
```

### B. ui.rs: cache each column's segment in `frame()`, add `replot`, and use code min/max

- `frame()` gets a new last parameter `seg: &mut [(i16, i16); 400]`.
- The `tools/ui-render/main.rs:363` call must also change, e.g. add
  `let mut seg = [(i16::MAX, i16::MIN); 400];` before it and pass `&mut seg`
  as the new last argument.
- In the empty-column branch (`if a == u16::MAX {`) add
  `seg[i] = (i16::MAX, i16::MIN);`.
- Right after `let (a, b) = (lo.min(hi), lo.max(hi));` (before the
  persistence union) add
  `seg[i] = (a.clamp(-30000, 30000) as i16, b.clamp(-30000, 30000) as i16);`.
- Faster `n >= COLS` branch. It gives identical output: `row()` is affine and
  the `as i32` truncation is monotonic, so the row min/max equal the rows of
  the code min/max. It saves ~0.6-0.8 ms per 25 k-sample frame.

```rust
} else if n >= scope::COLS {
    let a = i * n / scope::COLS;
    let b = ((i + 1) * n / scope::COLS).max(a + 1).min(n);
    let (mut cl, mut ch) = (u16::MAX, 0u16);
    for &v in &f.data[a..b] { cl = cl.min(v); ch = ch.max(v); }
    let (r1, r2) = (row(cl), row(ch));
    lo = r1.min(r2);
    hi = r1.max(r2);
}
```

- New method: it redraws the cached segments shifted, with no samples needed.
  That matters because `Frame.data` points into `BUF`, which the running DMA
  overwrites. `prev` sentinels follow `frame()`: `(MAX-1, 0)` = drawn empty,
  `(MAX, 0)` = cursor or force.

```rust
/// Drag preview: re-draw the segments cached by the last `frame` shifted by
/// `dy` model rows and `dx` columns; only columns whose pixels change are
/// written. `retrig`: the trigger row / dashed line moved, redraw its columns.
#[inline(never)]
pub fn replot(&self, seg: &[(i16, i16); 400], dy: i32, dx: i32, sc: &scope::Scope, prev: &mut [(u16, u16); 401], cur: &Cursors, trig_line: bool, retrig: bool, clip: u16) {
    let trig_row = Some(sc.s.trig_row);
    let mut tpl = [[0u16; 288]; 4];
    self.templates_c(&mut tpl, if cur.y_on { Some(cur.y) } else { None });
    let mut vt = tpl;
    for k in 0..3 {
        for y in (0..self.h as usize).step_by(3) {
            vt[k][y] = S_FRAME;
        }
    }
    for i in 0..scope::COLS {
        let j = i as i32 - dx;
        let s = if (0..scope::COLS as i32).contains(&j) { seg[j as usize] } else { (i16::MAX, i16::MIN) };
        let is_cur = cur.x_on && (cur.x[0] as usize == i || cur.x[1] as usize == i);
        let force = is_cur || (retrig && ((1..6).contains(&i) || (trig_line && i > 0 && i < 400 && i % 8 < 4)));
        let t = if is_cur { &vt } else { &tpl };
        if s.0 > s.1 {
            if prev[i] != (u16::MAX - 1, 0) || force {
                prev[i] = if is_cur { (u16::MAX, 0) } else { (u16::MAX - 1, 0) };
                self.column_c(i as u16, t, None, trig_row, trig_line, clip);
            }
            continue;
        }
        let (a, b) = (s.0 as i32 + dy, s.1 as i32 + dy);
        let px = (self.row_px(a), self.row_px(b));
        if prev[i] != px || force {
            prev[i] = if is_cur { (u16::MAX, 0) } else { px };
            self.column_c(i as u16, t, Some((a, b)), trig_row, trig_line, clip);
        }
    }
}
```

### C. app.rs: drag freeze

No new frames and no acquisition restart while a RUN drag is active; only a
shifted redraw of the frame already shown; settings applied once on release.

- Add a field `scope_seg: [(i16, i16); 400]`, initialised to
  `[(i16::MAX, i16::MIN); 400]`, and pass `&mut self.scope_seg` to `v.frame`.
- Add `frozen: bool` to `struct Drag` and set `frozen: false` where it is
  created in the `Gesture::Down` arm.
- New helpers:

```rust
/// Drag freeze (RUN): re-draw the shown frame shifted by the drag so far.
fn drag_replot(&mut self, retrig: bool) {
    let (Some(v), Some((s0, _))) = (self.scope_view(), self.touch_snap) else { return };
    let (dy, dx) = (self.scope.s.pos - s0.pos, self.scope.s.hpos - s0.hpos);
    let clip = if self.band_until != 0 { ui::FUNC_Y } else { u16::MAX };
    v.replot(&self.scope_seg, dy, dx, &self.scope, &mut self.scope_prev, &self.cur, self.trig_line_until != 0, retrig, clip);
    crate::prof::drawn();
}

/// Drag over (release, or a lost release): one apply if the position settings moved.
fn drag_release(&mut self, d: Drag, now: u64) {
    if !d.frozen {
        return;
    }
    let s = self.scope.s;
    match self.touch_snap {
        Some((s0, _)) if s0.pos != s.pos || s0.trig_row != s.trig_row || s0.hpos != s.hpos => self.scope_set(s, now),
        _ => self.scope_invalidate(), // cursor-only drag: next frame full redraw, no restart
    }
}
```

- First lines of `scope_poll`:

```rust
// Drag freeze: no new frames while a RUN drag moves trace / level / trigger point / cursors.
if self.drag.is_some_and(|d| d.frozen) {
    return;
}
```

- In `gesture()`, change the `if let Gesture::Down(_) = g { ... }` block. The
  `Gesture::Cancel` path already re-applies `s0` through `scope_set`, so it
  is covered.

```rust
if let Gesture::Down(_) = g {
    if let Some(d) = self.drag.take() {
        self.drag_release(d, now); // lost release
    }
    self.touch_snap = None;
}
```

- In the `Gesture::Up(_)` arm, after the gest_log block inside
  `if let Some(d) = self.drag.take() {`, add `self.drag_release(d, now);`.
- In `scope_drag`:
  - In `Target::CurX` replace `self.scope_invalidate();` with:

    ```rust
    if self.scope.s.run { d.frozen = true; self.drag = Some(d); self.drag_replot(false); } else { self.scope_invalidate(); }
    ```

  - In `Target::CurY` use the same code, but put
    `self.scope_prev = [(u16::MAX, 0); 401];` before `self.drag_replot(false)`,
    because the templates change.
  - In `Target::Level`, inside `if st.trig_row == self.scope.s.trig_row {`,
    replace `if !line_was { self.scope_invalidate(); }` with:

    ```rust
    if !line_was {
        if self.scope.s.run { d.frozen = true; self.drag = Some(d); self.drag_replot(true); } else { self.scope_invalidate(); }
    }
    ```

  - In the stopped pan (`Some(a) =>`) drop `self.scope_invalidate();`.
    `pan_from` already sets `zoom_pending`, and `prev` then gives a per-column
    diff instead of a full redraw.
  - Replace the final `if ... { self.scope_set(st, now); }` with:

    ```rust
    if st.trig_row != self.scope.s.trig_row || st.pos != self.scope.s.pos || st.hpos != self.scope.s.hpos {
        if self.scope.s.run {
            let retrig = st.trig_row != self.scope.s.trig_row;
            self.scope.preview(st);
            d.frozen = true;
            self.drag = Some(d);
            if let Some(v) = self.scope_view() {
                v.labels(&self.scope); // trigger voltage chip follows the finger
            }
            self.drag_replot(retrig);
        } else {
            self.scope_set(st, now);
        }
    }
    ```

- Expected result: under 1 ms per event for an Offset or TrigPos drag on a
  quiet trace (changed columns only); ~2.5 ms worst case for a level drag
  (~200 dashed-line columns); a full redraw for CurY. There is exactly one
  restart, on release.
- **Stopped pan, measurements.** In `scope_poll` change the `fresh` condition
  to `(now >= self.meas_next || !self.scope.s.run) && !f.data.is_empty() &&
  self.drag.is_none()`, and call `self.scope.refresh()` in `drag_release` (or
  in Up) so the measurements are recomputed once after release.

### D. Throttles and clean-ups

- Delete the `self.wave` loop and field in `App::scope_poll` (~0.4 ms per
  frame).
- Gate `dmm.reassert_relay` on `self.scope.dmm_rail()`.
- Optional: a span-fill `column_c` (template rows, then one run of trace
  pixels, then template rows) for ~30-40% faster full redraws.
- Look at `scope_meas` decimation and the BUF rotate/mask only if `s.meas` /
  `s.acq` turn out large on the device.

## 4. Device measurement plan (owner)

Each step is: `DBG:PROF:RESET`, run the scenario, then `DBG:PROF?` via
`tools/dm40dbg.py scpi "DBG:PROF?"`. Set `DBG:GRAB 0` first for direct drags,
and confirm the drag target with `DBG:GEST?`. On the full Scope page the graph
spans x 13..413, y 28..300. Vertical drags with x >= ~373 hit Level, other
vertical drags hit Offset, horizontal drags hit TrigPos.

1. Scope page, 40 Hz PWM, RUN, 1 ms/div, idle 10 s: fps (~40 expected),
   `scope` avg/max, loop histogram, `touch`/`gesture` = 0.
2. Same at 100 ms/div: fps ~1.
3. DBG:DRAG at both timebases:
   - Offset: `DBG:DRAG 200,120,200,200,600`
   - Level: `DBG:DRAG 400,120,400,200,600`
   - TrigPos: `DBG:DRAG 100,160,300,160,600`
   - Repeat about 5 times with `DBG:PROF?` after each. The `scpi` row's
     `last_us` is the handler-chain cost for ~30 events; `touch->frame` is the
     restart penalty after release (expect ~10-35 ms at 1 ms/div and 1-2 s at
     100 ms/div).
   - DBG:DRAG feeds the whole down/move/up sequence in one pass, so it does
     not measure paced finger latency.
4. Owner's finger drag for 5 s at both timebases: `touch` calls / 5 s =
   report rate; fps during the drag (expect ~0 before the fix);
   `touch->frame` max; `gesture` max.
5. Steps 1-4 on the DMM+Scope page: the `dmm` row is non-zero there.
6. After the app hooks and fix C land: rerun and compare fps during the drag
   (should equal the replot rate), `touch->frame` (should be one pass), and
   `s.frame`/`s.acq`/`s.meas`.
7. STOP, then pan over the stored record: `scope` max per event before and
   after the stopped-pan change.

Cross-check against DBG:DIAG touch bytes 6/8 (last and max touch-frame us).
