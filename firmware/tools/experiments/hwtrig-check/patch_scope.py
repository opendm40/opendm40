#!/usr/bin/python3
"""Apply the hwtrig hooks (../docs/history/hwtrig-integration.md H1..H7) to scope.rs.
Usage: python3 -I tools/experiments/hwtrig-check/patch_scope.py src/scope.rs src/scope.rs   (in place; or give a copy as the
second path for a dry run). Each `rep(old, new)` asserts its anchor text occurs exactly once, so a
concurrent scope.rs edit that moved an anchor fails loudly instead of patching the wrong place."""
import sys
src, dst = sys.argv[1], sys.argv[2]
s = open(src).read()

def rep(old, new, count=1):
    global s
    assert s.count(old) == count, (s.count(old), old)
    s = s.replace(old, new)

# H1: import.
rep("use crate::mmio::{modify32, r32, w32};\n",
    "use crate::hwtrig::{self, HwTrig, Status};\nuse crate::mmio::{modify32, r32, w32};\n")

# H2: Scope fields.
rep("""    /// Emit one re-render of the stored record on the next poll (set on any
    /// settings change while stopped and by `refresh`); no periodic re-render.
    zoom_pending: bool,
}
""", """    /// Emit one re-render of the stored record on the next poll (set on any
    /// settings change while stopped and by `refresh`); no periodic re-render.
    zoom_pending: bool,
    /// Comparator trigger engine (hwtrig.rs): DAC2 level, TIM2 post count
    /// gating TIM1. `hw_mode`: the acquisition in flight uses it (circular
    /// record over CAP); `hw_t0`: its start time (health check).
    pub hw: HwTrig,
    hw_mode: bool,
    hw_t0: u64,
}
""")

# H3: constructor.
rep("""            stored_roll: false,
            zoom_pending: false,
        };
        sc.apply(Settings::DEFAULT, 0);
""", """            stored_roll: false,
            zoom_pending: false,
            hw: HwTrig::new(),
            hw_mode: false,
            hw_t0: 0,
        };
        sc.hw.init();
        sc.apply(Settings::DEFAULT, 0);
""")

# H4: halt() releases the TIM1 gate (TIM1 SMCR = 0) after the timers stop.
rep("""            while r32(S4) & 1 != 0 || r32(S3) & 1 != 0 {}
        }
        self.busy = false;
        self.rolling = false;
    }
""", """            while r32(S4) & 1 != 0 || r32(S3) & 1 != 0 {}
        }
        self.hw.disarm();
        self.hw_mode = false;
        self.busy = false;
        self.rolling = false;
    }
""")

# H5: start(): circular record + prepare the engine before TIM1 starts.
rep("""            let (len, circ) = if self.rolling { (CAP as u32, 1 << 8) } else { (self.rec as u32, 0) };
            self.roll_written = 0;
            self.roll_last_wr = 0;
            w32(S4 + 0x04, len);
            w32(S4, (6 << 25) | (3 << 16) | (1 << 13) | (1 << 11) | (1 << 10) | circ | 1);
            w32(TIM8, 1);
            w32(TIM1, 1);
        }
        self.busy = true;
    }
""", """            // Comparator trigger: circular record over CAP, frozen in hardware
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
""")

# H6: poll(): record the start time, route hw-mode frames.
rep("""        if !self.busy {
            self.start();
            if self.deadline == 0 {
""", """        if !self.busy {
            self.start();
            self.hw_t0 = now;
            if self.deadline == 0 {
""")
rep("""        if self.rolling {
            return self.roll_poll(now);
        }
        if unsafe { r32(S4 + 0x04) } != 0 {
""", """        if self.rolling {
            return self.roll_poll(now);
        }
        if self.hw_mode {
            return self.hw_poll(now);
        }
        if unsafe { r32(S4 + 0x04) } != 0 {
""")

# H7: freeze(): a hardware-mode record in flight is circular; NDTR == 0 is
# not its completion. Treat it like roll: nothing to zoom unless Done.
rep("""        if was_rolling {
            self.snapshot_roll();
        } else if ndtr == 0 {
""", """        if was_rolling {
            self.snapshot_roll();
        } else if was_hw {
            // Circular hardware-trigger record: only a frozen (Done) one is
            // a complete frame; poll() would have shown it on the next tick,
            // so leave the screen as drawn (as a partial linear record).
        } else if ndtr == 0 {
""")
rep("""        let was_busy = self.busy;
        let was_rolling = self.rolling;
        let ndtr = unsafe { r32(S4 + 0x04) };
        self.halt();
""", """        let was_busy = self.busy;
        let was_rolling = self.rolling;
        let was_hw = self.hw_mode;
        let ndtr = unsafe { r32(S4 + 0x04) };
        self.halt();
""")

open(dst, "w").write(s)
print("patched", dst)
