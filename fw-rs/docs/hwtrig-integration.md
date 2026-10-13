# Hardware trigger comparator: integrating `src/hwtrig.rs`

`src/hwtrig.rs` (commit 168875e) is the comparator trigger engine: DAC channel 2
(PA5) sets the level, the comparator output on PA15 (AF1 TIM2_CH1) starts TIM2,
whose OC2REF (TRGO) gates TIM1, the sample-request timer, off `post` samples
later. The record (circular over the whole 26 K ring) freezes in hardware, so no
interrupt is needed (fw-rs has no peripheral ISRs). Status: wired (H0..H8
integrated with `patch_scope.py` unchanged against the scope.rs of commit
aa51ae5; `DEFAULT_ENABLED = false`, so the device runs the software trigger
until `DBG:HWTRIG:ENABLE 1`). The script refuses the patched file (its anchors
are gone). The text below is the wiring as applied. The hooks were compile-checked
(`-D warnings`, thumbv7em-none-eabihf) against the 847-line scope.rs with
`hpos`; `tools/hwtrig-check/patch_scope.py` applies H1..H7 mechanically and
fails if an anchor has moved:

    python3 -I tools/hwtrig-check/patch_scope.py src/scope.rs src/scope.rs

Background (what the stock does, verified vs inferred, the DAC map, the
wire-free calibration) is in the module header of `src/hwtrig.rs` and in
`docs/dac-adc-paths.md` §1, §4, §5.

## Hooks to integrate

### H0: main.rs

Add `mod hwtrig;` next to `mod scope;`.

### H1: imports (top of scope.rs)

Replace

```rust
use crate::mmio::{modify32, r32, w32};
```

with

```rust
use crate::hwtrig::{self, HwTrig, Status};
use crate::mmio::{modify32, r32, w32};
```

### H2: `struct Scope`: new fields after `zoom_pending: bool,`

Replace

```rust
    /// Emit one re-render of the stored record on the next poll (set on any
    /// settings change while stopped and by `refresh`); no periodic re-render.
    zoom_pending: bool,
}
```

with

```rust
    /// Emit one re-render of the stored record on the next poll (set on any
    /// settings change while stopped and by `refresh`); no periodic re-render.
    zoom_pending: bool,
    /// Comparator trigger engine (hwtrig.rs): DAC2 level, TIM2 post count
    /// gating TIM1. `hw_mode`: the acquisition in flight uses it (circular
    /// record over CAP); `hw_t0`: its start time (health check).
    pub hw: HwTrig,
    hw_mode: bool,
    hw_t0: u64,
}
```

### H3: `Scope::new`: field initialisers and `sc.hw.init()` before the first `apply`

Replace

```rust
            stored_roll: false,
            zoom_pending: false,
        };
        sc.apply(Settings::DEFAULT, 0);
```

with

```rust
            stored_roll: false,
            zoom_pending: false,
            hw: HwTrig::new(),
            hw_mode: false,
            hw_t0: 0,
        };
        sc.hw.init();
        sc.apply(Settings::DEFAULT, 0);
```

### H4: `halt()`: release the TIM1 gate after the timers and streams are stopped

Replace

```rust
            while r32(S4) & 1 != 0 || r32(S3) & 1 != 0 {}
        }
        self.busy = false;
        self.rolling = false;
    }
```

with

```rust
            while r32(S4) & 1 != 0 || r32(S3) & 1 != 0 {}
        }
        self.hw.disarm();
        self.hw_mode = false;
        self.busy = false;
        self.rolling = false;
    }
```

### H5: `start()`: circular record and `prepare` before TIM1 starts; new methods `hw_split`, `hw_linearise`, `hw_poll`, `hw_calibrate` after `start`

Replace

```rust
            let (len, circ) = if self.rolling { (CAP as u32, 1 << 8) } else { (self.rec as u32, 0) };
            self.roll_written = 0;
            self.roll_last_wr = 0;
            w32(S4 + 0x04, len);
            w32(S4, (6 << 25) | (3 << 16) | (1 << 13) | (1 << 11) | (1 << 10) | circ | 1);
            w32(TIM8, 1);
            w32(TIM1, 1);
        }
        self.busy = true;
    }
```

with

```rust
            // Comparator trigger: circular record over CAP, frozen in hardware
            // `post` samples after the edge (hwtrig.rs). Roll never triggers.
            let hw = self.hw.available() && !self.rolling;
            let (len, circ) = if self.rolling || hw { (CAP as u32, 1 << 8) } else { (self.rec as u32, 0) };
            self.roll_written = 0;
            self.roll_last_wr = 0;
            w32(S4 + 0x04, len);
            w32(S4, (6 << 25) | (3 << 16) | (1 << 13) | (1 << 11) | (1 << 10) | circ | 1);
            if hw {
                let (pre, _) = self.hw_split();
                let code = self.trig_code() as f32;
                self.hw.prepare(psc, arr, hwtrig::post_for(self.screen - pre, WIN), code, self.s.rising);
            }
            self.hw_mode = hw;
            w32(TIM8, 1);
            w32(TIM1, 1);
        }
        self.busy = true;
    }

    /// Screen samples left / right of the trigger point (hpos column).
    fn hw_split(&self) -> (usize, usize) {
        let pre = self.screen * self.s.hpos.clamp(0, COLS as i32) as usize / COLS;
        (pre, self.screen - pre)
    }

    /// Linearise the circular record after a hardware stop: with the ring
    /// wrapped (TCIF4) every sample is valid, rotate so it runs oldest ->
    /// newest; otherwise samples 0..wr are the record. Returns its length.
    fn hw_linearise(&mut self, wr: usize, wrapped: bool) -> usize {
        if wrapped {
            unsafe { (&mut (&mut *core::ptr::addr_of_mut!(BUF))[..]).rotate_left(wr) };
            CAP
        } else {
            wr
        }
    }

    /// Hardware-trigger acquisition in flight: arm after the pre-trigger part,
    /// then wait for TIM2 to gate TIM1 off (Done) or for the Auto timeout.
    fn hw_poll(&mut self, now: u64) -> Option<Frame<'static>> {
        let wr = CAP - (unsafe { r32(S4 + 0x04) } as usize).min(CAP);
        // HISR TCIF4: the ring has wrapped at least once.
        let wrapped = unsafe { r32(DMA1 + 0x04) } & (1 << 5) != 0;
        let st = self.hw.status();
        // Health: nothing written 100 ms after the start means the TIM1 gate
        // is stuck: give up on the comparator, the software path restarts.
        if wr == 0 && !wrapped && now >= self.hw_t0 + 100 {
            self.halt();
            self.hw.set_enabled(false);
            return None;
        }
        let (pre, post) = self.hw_split();
        match st {
            Status::Off => None,
            Status::Prepared => {
                if wrapped || wr >= hwtrig::pre_for(pre, WIN) {
                    self.hw.arm();
                }
                None
            }
            Status::Armed | Status::Fired => {
                // Timeout without an edge (Auto: the stock timeout; Normal /
                // Single: the same period, as a comparator health check): stop
                // and look for a crossing in software. One found while the
                // comparator stayed quiet is a miss (no comparator on this
                // board, or the level map is off); three in a row hand the
                // scope back to the software trigger (hwtrig::note_miss).
                if st != Status::Armed || now < self.deadline || !(wrapped || wr >= self.screen) {
                    return None;
                }
                self.halt();
                // Re-read after the stop: the DMA was still running above.
                let wr = CAP - (unsafe { r32(S4 + 0x04) } as usize).min(CAP);
                let wrapped = unsafe { r32(DMA1 + 0x04) } & (1 << 5) != 0;
                let len = self.hw_linearise(wr, wrapped);
                if len < self.screen {
                    self.stored_valid = false;
                    return None;
                }
                self.snapshot_mark(len, len - self.screen / 2, None, false);
                let data: &[u16] = unsafe { &(&*core::ptr::addr_of!(BUF))[..len] };
                let found = if len - post > pre { self.find_trigger(&data[pre..len - post]).map(|i| i + pre) } else { None };
                if found.is_some() {
                    self.hw.note_miss();
                }
                let start = match (found, self.s.mode) {
                    (Some(t), _) => t - pre,
                    // Auto: the newest screen, untriggered (stock TIM5 UIF
                    // "record done without trigger").
                    (None, TrigMode::Auto) => len - self.screen,
                    (None, _) => {
                        self.stored_valid = false;
                        return None;
                    }
                };
                self.stored_trig = found;
                self.stored_anchor = start + pre;
                self.deadline = 0;
                if found.is_some() && self.s.mode == TrigMode::Single {
                    self.s.run = false;
                }
                let data: &'static [u16] = unsafe { &(&*core::ptr::addr_of!(BUF))[start..start + self.screen] };
                self.seq = self.seq.wrapping_add(1);
                Some(Frame { data, cols: None, trig: found, fs: self.sample_rate(), seq: self.seq })
            }
            Status::Done => {
                self.halt();
                let len = self.hw_linearise(wr, wrapped);
                let post_n = hwtrig::post_for(post, WIN) as usize;
                if len < self.screen + WIN || len < post_n + pre {
                    // Cannot happen after `arm` (pre + post samples written),
                    // kept as a guard against a stale TCIF.
                    self.stored_valid = false;
                    return None;
                }
                // Hardware estimate: the edge is post_n samples before the
                // newest sample; the stock software search refines it.
                let th = len - post_n;
                let lo = th.saturating_sub(WIN / 2).max(pre);
                let hi = (th + WIN / 2).min(len - post);
                self.snapshot_mark(len, th, None, false);
                let data: &[u16] = unsafe { &(&*core::ptr::addr_of!(BUF))[..len] };
                let found = if lo < hi { self.find_trigger(&data[lo..hi]).map(|i| i + lo) } else { None };
                let t = match found {
                    Some(t) => {
                        self.hw.note_hit(t as i32 - th as i32);
                        t
                    }
                    None => {
                        // Fired, but no crossing near the estimate: the map
                        // is off. Auto shows the frame at the estimate;
                        // Normal / Single wait for the next one.
                        self.hw.note_miss();
                        if self.s.mode != TrigMode::Auto {
                            self.stored_valid = false;
                            return None;
                        }
                        th.clamp(pre, len - post)
                    }
                };
                self.stored_trig = found;
                self.stored_anchor = t;
                self.deadline = 0;
                if found.is_some() && self.s.mode == TrigMode::Single {
                    self.s.run = false;
                }
                let start = t - pre;
                let data: &'static [u16] = unsafe { &(&*core::ptr::addr_of!(BUF))[start..start + self.screen] };
                self.seq = self.seq.wrapping_add(1);
                Some(Frame { data, cols: None, trig: found, fs: self.sample_rate(), seq: self.seq })
            }
        }
    }

    /// Wire-free comparator calibration (hwtrig::calibrate) at range 6 / DC
    /// with the scope's own offset points; the current settings are restored
    /// (acquisition restarts from poll). Needs PC4 on and nothing on the MCX.
    pub fn hw_calibrate(&mut self, now: u64, clk: &mut crate::clock::Clock) -> Result<hwtrig::CalResult, hwtrig::CalError> {
        if !self.powered {
            return Err(hwtrig::CalError::NoComparator);
        }
        let saved = self.s;
        let mut s = saved;
        s.range = 6;
        s.ac = false;
        self.apply(s, now);
        // Relay settle (PC13 changes when coming from r0..4) + rail.
        clk.delay_ms(250);
        let r = self.hw.calibrate(6, None);
        self.apply(saved, now);
        r
    }
```

### H6a: `poll()`: remember the start time

Replace

```rust
        if !self.busy {
            self.start();
            if self.deadline == 0 {
```

with

```rust
        if !self.busy {
            self.start();
            self.hw_t0 = now;
            if self.deadline == 0 {
```

### H6b: `poll()`: route hardware-mode acquisitions

Replace

```rust
        if self.rolling {
            return self.roll_poll(now);
        }
        if unsafe { r32(S4 + 0x04) } != 0 {
```

with

```rust
        if self.rolling {
            return self.roll_poll(now);
        }
        if self.hw_mode {
            return self.hw_poll(now);
        }
        if unsafe { r32(S4 + 0x04) } != 0 {
```

### H7a: `freeze()`: a circular hardware record in flight is not a complete linear record

Replace

```rust
        if was_rolling {
            self.snapshot_roll();
        } else if ndtr == 0 {
```

with

```rust
        if was_rolling {
            self.snapshot_roll();
        } else if was_hw {
            // Circular hardware-trigger record: only a frozen (Done) one is
            // a complete frame; poll() would have shown it on the next tick,
            // so leave the screen as drawn (as a partial linear record).
        } else if ndtr == 0 {
```

### H7b: `freeze()`: capture the flag before `halt()` clears it

Replace

```rust
        let was_busy = self.busy;
        let was_rolling = self.rolling;
        let ndtr = unsafe { r32(S4 + 0x04) };
        self.halt();
```

with

```rust
        let was_busy = self.busy;
        let was_rolling = self.rolling;
        let was_hw = self.hw_mode;
        let ndtr = unsafe { r32(S4 + 0x04) };
        self.halt();
```

### H8: DBG commands (app.rs / scpi.rs; integrated: `Cmd::DbgHwTrig`, handled
as a scope command so the scope rail is on; replies also carry `post_ticks`
(DBG:HWTRIG?) and the two offset-point codes (CAL?, after `slope`))

| Command | Action |
|---|---|
| `DBG:HWTRIG:ENABLE <0\|1>` | `self.scope.hw.set_enabled(n != 0)` (resets the miss counter when enabling) |
| `DBG:HWTRIG:CAL?` | `self.scope.hw_calibrate(now, clk)`; reply `flip[0][0],flip[0][1],flip[1][0],flip[1][1],map,hyst,slope` or the `CalError` name |
| `DBG:HWTRIG?` | `hw.debug()` (TIM2 SR, CNT, CCR1, `PA15<<16 \| DOR2`), `hw.enabled()`, `hw.available()`, `hw.map(true)`, `hw.map(false)` |
| `DBG:HWTRIG:STAT?` | `hw.stats`: hits, misses, delta_last, delta_min, delta_max, mean = delta_sum / hits (samples) |
| `DBG:HWTRIG:STAT:RESET` | `hw.reset_stats()` |

The engine needs the scope rail (PC4) on, i.e. a scope page; `start()` already
runs only while `powered`, and `hw_calibrate` returns `NoComparator` otherwise.

## Fallback policy (as implemented by the hooks)

1. `hw.available()` false (disabled, or no plausible map in RAM or at
   REC+0x148): the existing software path, untouched. Roll mode never uses
   the comparator.
2. No sample written 100 ms after the start (TIM1 gate stuck: TIM2 clock or
   OC2REF not as expected): `set_enabled(false)`, the software path restarts.
3. The comparator never fires by the deadline (every mode; Auto's stock
   timeout, reused as a health period for Normal / Single): stop, search the
   ring in software. A crossing found while the comparator stayed quiet is a
   miss (no comparator on this board, or the level map is off). Auto shows
   the frame (aligned on the crossing if one was found, else the newest
   screen untriggered); Normal / Single show a frame only if a crossing was
   found, else restart.
4. Fired, but no crossing within +-400 samples of the estimate: a miss; Auto
   shows the frame at the estimate, Normal / Single wait for the next edge.

Three consecutive misses (`hwtrig::MAX_MISSES`) set `enabled = false`: the
scope is back on its software trigger until `DBG:HWTRIG:ENABLE 1`.

## `DEFAULT_ENABLED`

`hwtrig::DEFAULT_ENABLED = false`. With the stock pair at REC+0x148 present,
`available()` would otherwise be true from the first boot and every
acquisition would take the comparator path on the first flash. Enable it with
`DBG:HWTRIG:ENABLE 1` for the device test below; flip the constant once the
test has passed. The calibration result (`hw.calibrate`) lives in RAM only;
writing it back to the settings record is a follow-up, as is writing DAC2 on a
level drag without a restart (`docs/scope-ui-plan.md`, "write DAC2 only").

## Cost

Scratch build (opt-level z, no LTO): hwtrig.rs about 1.9 KB of text; hwtrig +
the scope hooks measured as a 3.2 KB text delta over the unpatched scope.rs
(somewhat less in-crate with LTO). RAM: `size_of::<HwTrig>() == 104` bytes
inside `Scope`, no static buffers (the circular record reuses `BUF`). The RAM
image is not a target (it already overflows).

## Device test plan

Calibrator: MR9270S+ via `tools/mr9270.py`, PWM = function 2 mode 5 (fixed
~40 Hz, the value is the duty in %), 0..9.6 V square as in the AC+DC tests.

1. **Comparator present, map.** Flash the integrated build, open the scope
   page (PC4 on), nothing on the MCX. `DBG:HWTRIG:CAL?`: expect `Ok` with
   `slope` about 1.29 DAC/code, flips near 1750 / 2330 (+-200) for codes
   280 / 730, `hyst` a few DAC codes. Compare the map with the stock REC+0x148
   pair (`DBG:HWTRIG?`). `NoComparator` with the rail on means no comparator
   output on this board; `OffsetNoEffect` falsifies "comparator after the
   offset stage" (`docs/dac-adc-paths.md` §4): report either. Optional: run
   `hw.calibrate(3, None)` at range 3 (hook: pass the range) and compare with
   range 6; agreement confirms "after the gain stage".
2. **Basic trigger.** `DBG:HWTRIG:ENABLE 1`; 40 Hz PWM into the MCX; 1 V/div,
   DC, level about 4 V, rising, Auto, 200 us/div. The edge must sit at the
   hpos column, stable. `DBG:HWTRIG:STAT?`: hits grow about 40/s, misses 0,
   mean delta about -(2 us x fs) (-50 samples at 25 MS/s) if the stock
   `trig_delay` is the comparator latency; `delta_max - delta_min` is the
   latency jitter in samples.
3. **Catch rate (the real win).** Normal mode, 1 us/div and 100 ns/div, 40 Hz
   input. Software path (`ENABLE 0`): the 800-sample window is 16 us of a
   25 ms period, expect WAIT most of the time; hardware path: every edge,
   about 40 frames/s (UI loop permitting). Count frames per 10 s (the `seq`
   counter / screen updates).
4. **Jitter, honest form.** Both paths place the trace with the same
   `find_trigger`, so the visible alignment jitter is identical
   (sample-quantised); compare (a) frames/s triggered and (b) the delta spread
   from step 2. Repeat with falling edge, with the level near the top and the
   bottom of the swing (map accuracy: misses must stay 0), and with a 1 kHz
   generator sine through an MCX-MCX cable at 100 ns/div.
5. **Fallback.** `ENABLE 1`, MCX open, Normal: WAIT, misses 0, `enabled()`
   stays 1. Level deliberately outside the signal swing, Auto: untriggered
   frames keep coming and after 3 misses `enabled()` reads 0, the software
   path resumes. Generator (DAC ch1) must keep working with the engine armed
   (shared DAC CR, bits 0..15 vs 16..31).
6. **Regression.** STOP / zoom in hardware mode (stored record = the
   linearised ring, anchored at the trigger), roll mode unaffected, Single
   stops on the first hit, the plain software path unchanged after
   `ENABLE 0` (TIM1 SMCR back to 0: `halt()` calls `disarm()`).
