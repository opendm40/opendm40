//! Phase lock of the AD2 stream timer to the HY3131's conversion grid
//! (docs/dmm-input-scope-feasibility.md 10). Pure integer state machine: the
//! stream ISR (logger.rs `isr`) asks it for each timer interval and feeds it
//! the probe results; host tests drive it against a model chip.
//!
//! Why: reading INTF clears AD2F, and a conversion that completes while the
//! INTF frame is under way is lost to that read (device 381ea07: ~11.5 ms
//! bursts every ~3948 conversions while the 4 x tick walked over the edge at
//! +63 ppm). So the sample is not found by polling the flag. The data
//! register R03..R05 holds a result for a whole period, so the ISR reads it
//! BLIND, once per period, at a time locked half way between two conversion
//! edges; the flag only measures where the edge is:
//!
//! ```text
//!   edge          A (data)            B (probe)  edge          A
//!   |-------------|-------------------|----------|-------------|---
//!                 <------ beta ------>            P
//! ```
//!
//! * Entry A, every period: R03..R05 (one 3-byte frame), into the ring.
//! * Every `np`-th period an extra entry B, `beta` after A, at the place the
//!   edge should be: an INTF read (it clears AD2F). The A after it reads
//!   INTF once more, after its data frame: AD2F set = the edge came after B's
//!   read, clear = before (or B's read swallowed it - the same side). That
//!   bang-bang result moves the next period by `kp` counts and the period
//!   word by `ki` (a second-order loop: phase and frequency, so a +-100 ppm
//!   crystal mismatch is followed with no static phase error).
//! * `beta = (P + r + l) / 2`: the data frame (duration r, entry latency l)
//!   sits in the middle of the edge-free time: margin (P - r - l) / 2 on both
//!   sides (4.8 kS/s at /16: ~97 us; 38.4 kS/s at /8: ~8.7 us).
//!
//! The race can only touch the probe (B), never the data. A torn or doubled
//! sample needs a phase error of more than that margin; the loop holds it to
//! a few `kp`. All quantities are timer counts (50 MHz, 20 ns); the period
//! word `f` is Q16 counts, the fraction carried in `acc`, so the mean period
//! is exact to 2^-16 counts (the ARR rounding of the 4 x tick, +64 ppm at
//! 4.8 kS/s, is gone).

#![cfg_attr(not(feature = "ad2-isr"), allow(dead_code))]

/// Timer count rate (hy3131 `stream`: PSC 3 at the 200 MHz kernel clock).
pub const COUNT_HZ: u32 = 50_000_000;

/// Probe every NP-th period once locked (every period before).
pub const NP: u8 = 4;

/// Typical entry latency, counts (1 us): part of the data frame's slot.
pub const LAT0: u32 = 50;

/// Probes after which a phase step of kp_max / 4 counts as locked (`result`).
const LOCK_PROBES: u32 = 64;

/// Hybrid: probe results after the start (or after a run of more than
/// 2 x RUN_GEAR_UP) before the probing goes sparse (every NP-th period,
/// `Pll::np`): ~500 periods of dense probes, 7 ms at 76.8 kS/s, so a word
/// that starts off the chip's rate is pulled in before a sparse probe
/// interval's drift can carry the phase out of the window (`result`).
const CALM: u32 = 256;

/// Hybrid: the frequency step while the probes are dense (CALM), x ki.
const KI_DENSE: u32 = 4;

/// Consecutive probe results of one sign after which the phase step doubles
/// again (a large frequency error, or a phase jump: re-acquisition).
const RUN_GEAR_UP: u32 = 8;

/// Frequency step per probe: the period word / 2^KI_SHIFT (3.8 ppm). The
/// phase effect of one step over the NP periods to the next probe is
/// ~1.5e-5 P: nothing; a smaller step made the loop slow to follow a crystal
/// offset when the B read jitters by microseconds (the emulator: still 34
/// ppm off after 19000 periods at 2^-21).
const KI_SHIFT: u32 = 18;

/// The frequency word may move at most this far from the nominal (ppm): a
/// runaway loop cannot detune the timer by more than 0.5 %. Was 1000 (any
/// real crystal pair) while the 76.8 kS/s stream ran 2940 ppm slow (device
/// e8cc4e9): the word sat on the clamp and the probes' phase kicks alone
/// held the grid (1.48 ISR entries per sample, ~9 % more ISR CPU). That
/// rate came from the old loop's walks, not the chip (device 8ce42e2: with
/// this loop and a 76 574.6 nominal it locked at the datasheet's 76 800,
/// -2935 ppm, from a cold start); the nominal is the datasheet's again, and
/// 5000 keeps the margin that showed. Nothing else depends on it: the beta / late geometry
/// is the nominal's (a 0.5 % period change moves the frame's margins by
/// 0.5 % of a period), and the clamp only bounds the word, not the kicks.
const MAX_PPM: u64 = 5000;

/// A lock carried into the next session at another rung (`carry`) is at
/// most this far (ppm) from its nominal: the crystal pair's offset, which
/// every rung shares (+-100 ppm crystals). The 76.8 kS/s hybrid rung keeps
/// its own carried word (logger.rs `REL_HYB`): its offset may be the rung's
/// own (the device's e8cc4e9 numbers, see MAX_PPM).
const CARRY_PPM: i64 = 1000;
const CARRY_Q24: i32 = ((CARRY_PPM << 24) / 1_000_000) as i32;

#[derive(Clone, Copy)]
pub struct Pll {
    /// Period, timer counts Q16.
    pub f: u32,
    /// Nominal period (the ladder rung), Q16.
    pub f_nom: u32,
    /// `f` averaged over ~64 probes, Q24 (the reported rate, `rel_q24`):
    /// `f` itself dithers by a few `ki` around the chip's period.
    f_avg: u64,
    acc: u32,
    /// A -> B on a probe period, counts.
    pub beta: u32,
    /// The data frame, counts (late_max).
    pub frame: u32,
    /// Latest A entry latency that still reads (`late_max`), counts.
    pub late_a: u32,
    /// Latest C entry latency whose probe read still comes before the next
    /// edge (`hybrid`; u32::MAX = no check, the /16 geometry), counts.
    pub c_late: u32,
    kp: u32,
    kp_min: u32,
    kp_max: u32,
    ki: u32,
    /// Counts added to the next period (phase steps, a late interval's
    /// overshoot taken back).
    kick: i32,
    /// Periods until the next probe (0: this A schedules a B).
    left: u8,
    /// The second part of a probe period, counts (B -> A).
    rest: u32,
    /// The next entry is B.
    pub next_b: bool,
    /// The next entry is C (`late_read`).
    pub next_c: bool,
    /// The probe's INTF read-back is a separate entry C, `c_off` after the
    /// A of the period after B (after A's data frame), instead of a read
    /// at A itself: for data frames that A only starts (the DMA frame of
    /// hy3131 `stream::dma`), so the bus is free at A.
    pub late_read: bool,
    pub c_off: u32,
    /// Probe every `np`-th period once locked.
    pub np: u8,
    /// Samples count (and are taken) only once locked (`take`): where the
    /// edge search's error (~ an INTF frame) exceeds the margin.
    pub gate: bool,
    /// B read INTF: the next A completes the probe.
    probing: bool,
    last_s: i8,
    run: u32,
    /// Hybrid: consecutive results of one sign (not reset by a gear-up;
    /// above RUN_GEAR_UP the frequency word holds) and the results since the
    /// start or the last run of more than 2 x RUN_GEAR_UP (below CALM the
    /// probes stay dense), `result`.
    streak: u32,
    calm: u32,
    // statistics (DBG:ISR?)
    /// A entries = conversion periods since the start (LOG:COUNt? lost).
    pub periods: u32,
    pub probes: u32,
    /// Probes that saw the edge after B (the timer was early).
    pub late_edges: u32,
    pub max_run: u32,
    /// The phase step reached its minimum at least once.
    pub locked: bool,
}

impl Pll {
    pub const fn idle() -> Self {
        Pll {
            f: 0,
            f_nom: 0,
            f_avg: 0,
            acc: 0,
            beta: 0,
            frame: 0,
            late_a: 0,
            c_late: u32::MAX,
            kp: 0,
            kp_min: 0,
            kp_max: 0,
            ki: 0,
            kick: 0,
            left: 0,
            rest: 0,
            next_b: false,
            next_c: false,
            late_read: false,
            c_off: 0,
            np: NP,
            gate: false,
            probing: false,
            last_s: 0,
            run: 0,
            streak: 0,
            calm: 0,
            periods: 0,
            probes: 0,
            late_edges: 0,
            max_run: 0,
            locked: false,
        }
    }

    /// Nominal period (Q16 counts) of a conversion rate in mHz.
    pub const fn f_of_mhz(rate_mhz: u32) -> u32 {
        ((COUNT_HZ as u64 * 65536 * 1000 + rate_mhz as u64 / 2) / rate_mhz as u64) as u32
    }

    /// A session at `rate_mhz` with the frequency word pre-set `rel_q24`
    /// (period offset / nominal, 2^-24 units: the last session's lock) and a
    /// data frame of `frame_counts` (+ LAT0 of entry latency).
    pub fn new(rate_mhz: u32, rel_q24: i32, frame_counts: u32) -> Self {
        let f_nom = Self::f_of_mhz(rate_mhz);
        let off = (f_nom as i64 * rel_q24 as i64) >> 24;
        let pc = f_nom >> 16;
        let kp_min = (pc >> 11).max(1);
        let mut p = Pll::idle();
        p.f_nom = f_nom;
        p.f = (f_nom as i64 + off) as u32;
        p.f = p.clamp(p.f);
        p.f_avg = (p.f as u64) << 8;
        p.frame = frame_counts;
        p.beta = ((pc + frame_counts + LAT0) / 2).min(pc - 1);
        p.late_a = p.beta.saturating_sub(frame_counts + LAT0);
        p.kp_min = kp_min;
        p.kp_max = (pc >> 6).max(kp_min);
        p.kp = p.kp_max;
        p.ki = (f_nom >> KI_SHIFT).max(1);
        p.left = 0; // probe from the first period on
        p
    }

    fn clamp(&self, f: u32) -> u32 {
        let d = (self.f_nom as u64 * MAX_PPM / 1_000_000) as u32;
        f.clamp(self.f_nom - d, self.f_nom + d)
    }

    /// The period offset / nominal in 2^-24 units (kept for the next session).
    pub fn rel_q24(&self) -> i32 {
        (((self.f_avg as i64 - ((self.f_nom as i64) << 8)) << 16) / self.f_nom as i64) as i32
    }

    /// The lock to carry into the next session (`new`'s `rel_q24`), if
    /// locked: any for the same rung (`same_rung`), else only within
    /// CARRY_PPM of the nominal (a crystal-sized offset).
    pub fn carry(&self, same_rung: bool) -> Option<i32> {
        let r = self.rel_q24();
        (self.locked && (same_rung || r.abs() <= CARRY_Q24)).then_some(r)
    }

    /// The averaged period, Q16.
    pub fn f_mean(&self) -> u32 {
        (self.f_avg >> 8) as u32
    }

    /// The locked conversion rate, mHz of the MCU clock.
    pub fn rate_mhz(&self) -> u32 {
        let f = self.f_mean();
        if f == 0 { 0 } else { (COUNT_HZ as u64 * 65536 * 1000 / f as u64) as u32 }
    }

    /// Entry A (at the timer update that ends the previous interval): the
    /// length of the interval starting now, counts, and whether this A
    /// completes a probe (read INTF after the data frame; never with
    /// `late_read`: then a C entry follows instead).
    pub fn on_a(&mut self) -> (u32, bool) {
        let probe = self.probing;
        self.probing = false;
        if self.locked || !self.gate {
            self.periods = self.periods.wrapping_add(1);
        }
        let s = self.acc + self.f;
        self.acc = s & 0xFFFF;
        let len = ((s >> 16) as i32 + self.kick).max(2 * self.kp_min as i32 + 4) as u32;
        self.kick = 0;
        if self.late_read && probe && len > self.c_off + 4 {
            // This period carries the read-back C, the B waits.
            self.left = self.left.saturating_sub(1);
            self.rest = len - self.c_off;
            self.next_c = true;
            return (self.c_off, false);
        }
        if self.left == 0 && len > self.beta + 4 {
            let calm = self.c_late == u32::MAX || self.calm >= CALM;
            self.left = if self.kp > self.kp_min || !calm { 0 } else { self.np - 1 };
            self.rest = len - self.beta;
            self.next_b = true;
            (self.beta, probe)
        } else {
            self.left = self.left.saturating_sub(1);
            (len, probe)
        }
    }

    /// Entry B: the interval to the next A. `valid`: B's INTF read is made
    /// (the entry was on time); else the probe is dropped.
    pub fn on_b(&mut self, valid: bool) -> u32 {
        self.next_b = false;
        self.probing = valid;
        self.rest
    }

    /// This period's sample is taken (`gate`: only once locked).
    pub fn take(&self) -> bool {
        self.locked || !self.gate
    }

    /// Entry C (`late_read`): the interval to the next A; the ISR reads INTF
    /// here and passes it to `result`.
    pub fn on_c(&mut self) -> u32 {
        self.next_c = false;
        self.rest
    }

    /// The probe result: AD2F seen by A = the edge came after B's read.
    pub fn result(&mut self, edge_after_b: bool) {
        let s: i8 = if edge_after_b { 1 } else { -1 };
        self.probes = self.probes.wrapping_add(1);
        if edge_after_b {
            self.late_edges = self.late_edges.wrapping_add(1);
        }
        self.calm = self.calm.saturating_add(1);
        if s == self.last_s {
            self.run += 1;
            self.streak += 1;
            if self.streak > 2 * RUN_GEAR_UP {
                // A walk (see the frequency step below) or a word off the
                // chip's rate: dense probes again for CALM results.
                self.calm = 0;
            }
            if self.run >= RUN_GEAR_UP {
                if self.kp < self.kp_max {
                    self.kp = (self.kp * 2).min(self.kp_max);
                    self.run = 0;
                }
            }
        } else {
            if self.last_s != 0 {
                self.kp = (self.kp / 2).max(self.kp_min);
                // Locked: the phase step at its minimum, or (after
                // LOCK_PROBES probes) at most kp_max / 4 = P / 256: a dither
                // far inside the data frame's margin. Where the B entry's
                // jitter exceeds kp_min, the results are a coin toss around
                // the edge and runs of RUN_GEAR_UP double the step now and
                // then: kp_min alone was a race the gated (DMA) stream
                // could lose for good (emulator, 19200 S/s: 3 of 9 sessions
                // never locked, so never took a sample).
                if self.kp == self.kp_min || (self.probes >= LOCK_PROBES && self.kp <= self.kp_max / 4) {
                    self.locked = true;
                }
            }
            self.run = 1;
            self.streak = 1;
        }
        self.max_run = self.max_run.max(self.run);
        self.last_s = s;
        // Edge after B: B (and A) are early: a longer period.
        self.kick += s as i32 * self.kp as i32;
        // Hybrid: the frequency word does not follow a long run of one sign
        // (more than RUN_GEAR_UP). The hybrid window is lopsided (`hybrid`):
        // a B more than P - (C - B) late (~1.2 us at 76.8 kS/s) has its
        // read-back C after the NEXT edge and reads "edge after B": the
        // phase then walks forward to that next edge (one conversion slips)
        // with the step geared up and every result "lengthen". Integrating
        // that walk lengthened the word, which put the next B later still:
        // the loop ran onto the MAX_PPM clamp and stayed there, the phase
        // held by kicks only and slipping over and over (the emulator
        // reproduces the device's e8cc4e9 state so with a chip at the
        // nominal rate: rate_mhz 76 723.6, ~86 % of the probes "after B",
        // ~0.3 probes per sample). A real frequency offset still integrates:
        // the kicks hold the phase with runs of a few results broken by the
        // other sign, biased to the offset's side; and the probes stay dense
        // (every period the C allows, `on_a`) for CALM results after the
        // start or a long run, with KI_DENSE x the step, so the drift of a
        // word still off cannot carry the phase out of the window between
        // two sparse probes.
        let hyb = self.c_late != u32::MAX;
        if !hyb || self.streak <= RUN_GEAR_UP {
            // Dense (acquisition): KI_DENSE x the step, both signs alike.
            let ki = if hyb && self.calm < CALM { self.ki * KI_DENSE } else { self.ki };
            let f = if edge_after_b { self.f + ki } else { self.f - ki };
            self.f = self.clamp(f);
        }
        let fa = (self.f as u64) << 8;
        self.f_avg = if fa >= self.f_avg { self.f_avg + ((fa - self.f_avg) >> 6) } else { self.f_avg - ((self.f_avg - fa) >> 6) };
    }

    /// Latest entry latency (counts) at which a data frame of `frame` counts
    /// still ends before the next edge, with 1 us to spare (`new`; the
    /// hybrid geometry: `hybrid`).
    pub fn late_max(&self) -> u32 {
        self.late_a
    }

    /// The geometry of the hybrid frames (hy3131 `stream::hybrid_kick`, 76.8
    /// kS/s, feasibility 11.12), after `new` with `frame` = the A entry to
    /// the data frame's last bit and `c_off` = `frame` (C right after it).
    /// The probe frames (B, C) are 2-byte DMA frames whose INTF byte the
    /// chip latches `delta` counts after the entry (the lock puts B's latch
    /// on the edge); `delta_min` is its physical minimum (CS setup, the
    /// command byte and one clock). With A at `a` after an edge the window
    /// is: A's frame after B's tail (~a >= 0), C's latch before the next
    /// edge (a <= P - c_off - delta): `beta` puts A in the middle of it,
    /// `a = (P - c_off - delta) / 2`, so beta = (P + c_off - delta) / 2.
    /// A late A still reads while its frame ends `margin` before the next
    /// edge (that edge is at least beta + delta_min after A); a late C drops
    /// its probe (`c_late`): its latch would come after the next edge.
    pub fn hybrid(&mut self, delta: u32, delta_min: u32, margin: u32) {
        let pc = self.f_nom >> 16;
        self.beta = ((pc + self.c_off).saturating_sub(delta) / 2).min(pc - 8);
        self.late_a = (self.beta + delta_min).saturating_sub(self.frame + margin);
        self.c_late = self.beta.saturating_sub(self.c_off + margin);
    }

    /// The phase of the first A after an edge: P - beta, less the probe
    /// latch delay `delta` (hybrid; 0 = the read at the entry).
    pub fn lead(&self, delta: u32) -> u32 {
        ((self.f >> 16) - self.beta).saturating_sub(delta)
    }

    /// The interval just set could not be met (the entry came after its end
    /// was due): it was stretched by `over` counts; the next period gives
    /// them back, so the grid keeps its phase.
    pub fn overshoot(&mut self, over: u32) {
        self.kick -= over as i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model of the chip and the ISR: conversions every `p_chip` counts
    /// (f64, the crystal offset), the timer intervals from the Pll, the probe
    /// INTF read at B (+ latency) with the read-clears race of window `w`, the
    /// data frame at A + latency for `frame` counts. Returns (samples, bad):
    /// bad = a data frame overlapping an edge, or a period with no / two new
    /// conversions between consecutive data reads.
    fn run(rate_mhz: u32, ppm: f64, periods: u32, frame: f64, lat: &dyn Fn(u64) -> f64, w: f64) -> (u32, u32, Pll) {
        let (s, bad, _, pll) = run2(rate_mhz, ppm, periods, frame, lat, w, 0.37, false);
        (s, bad, pll)
    }

    fn run2(
        rate_mhz: u32,
        ppm: f64,
        periods: u32,
        frame: f64,
        lat: &dyn Fn(u64) -> f64,
        w: f64,
        ph: f64,
        dma: bool,
    ) -> (u32, u32, u32, Pll) {
        let mut pll = Pll::new(rate_mhz, 0, frame as u32);
        if dma {
            pll.late_read = true;
            pll.c_off = frame as u32 + 50;
            pll.np = 8;
            pll.gate = true;
        }
        let p_chip = (COUNT_HZ as f64 * 1000.0 / rate_mhz as f64) / (1.0 + ppm * 1e-6);
        let phase0 = ph * p_chip; // the first edge, relative to the first A
        let edge = |k: i64| phase0 + k as f64 * p_chip;
        let edge_index = |t: f64| ((t - phase0) / p_chip).floor() as i64; // last edge <= t
        let mut t = 0.0f64; // the update event of this entry
        let mut last_data_k: Option<i64> = None;
        let (mut samples, mut bad, mut skipped) = (0u32, 0u32, 0u32);
        let mut b_read = f64::NAN;
        let mut n = 0u64;
        while samples + skipped < periods {
            let l = lat(n);
            n += 1;
            if pll.next_b {
                let valid = l < 100.0;
                let iv = pll.on_b(valid);
                b_read = t + l + 40.0; // the INTF byte inside the frame
                t += iv as f64;
            } else if pll.next_c {
                let iv = pll.on_c();
                let c_read = t + l + 40.0;
                pll.result(edge_index(c_read) > edge_index(b_read));
                t += iv as f64;
            } else {
                let (iv, probe) = pll.on_a();
                if !pll.take() {
                    t += iv as f64;
                    continue;
                }
                if l > pll.late_max() as f64 {
                    // The ISR skips a late data read (counted, a stamp gap).
                    last_data_k = last_data_k.map(|k| k + 1);
                    skipped += 1;
                    t += iv as f64;
                    continue;
                }
                let start = t + l;
                let end = start + frame;
                let k = edge_index(start);
                if edge_index(end) != k || edge(k + 1) - end < 0.0 {
                    bad += 1; // straddles an edge
                }
                if let Some(k0) = last_data_k {
                    if k != k0 + 1 {
                        bad += 1;
                    }
                }
                last_data_k = Some(k);
                samples += 1;
                if probe {
                    // INTF after the data frame: an edge in (b_read, now]?
                    let now = end + 40.0;
                    let kb = edge_index(b_read);
                    let mut seen = edge_index(now) > kb;
                    // B's read swallowed an edge just before it: also "before".
                    if !seen && b_read - edge(kb) < w {
                        seen = false;
                    }
                    pll.result(seen);
                }
                t += iv as f64;
            }
        }
        (samples, bad, skipped, pll)
    }

    #[test]
    fn locks_and_loses_nothing_at_any_rate_and_crystal_offset() {
        for &rate in &[1_200_000u32, 4_800_000, 9_600_000, 19_200_000, 38_400_000] {
            for &ppm in &[-100.0, -63.0, 0.0, 63.0, 100.0] {
                let frame = if rate > 20_000_000 { 400.0 } else { 700.0 }; // 8 / 14 us
                let (s, bad, pll) = run(rate, ppm, 200_000, frame, &|_| 20.0, 35.0);
                assert_eq!(bad, 0, "rate {rate} ppm {ppm}: {bad} bad of {s}");
                assert!(pll.locked, "rate {rate} ppm {ppm} not locked");
                // The frequency word follows the chip: rate within 2 ppm.
                let got = pll.rate_mhz() as f64 / rate as f64 - 1.0;
                assert!((got * 1e6 - ppm).abs() < 3.0, "rate {rate} ppm {ppm}: locked at {:.1} ppm", got * 1e6);
                assert!(pll.max_run < 64, "rate {rate} ppm {ppm}: run {}", pll.max_run);
            }
        }
    }

    #[test]
    fn latency_jitter_and_spikes_do_not_break_the_lock() {
        // 0.2 .. 3 us of jitter on every entry, a 20 us spike every 1000
        // entries (the old slow-flash stall), at 19.2 kS/s.
        let lat = |n: u64| -> f64 {
            let j = ((n.wrapping_mul(2654435761) >> 7) % 140) as f64 + 10.0;
            if n % 1000 == 999 { 1000.0 } else { j }
        };
        let (s, bad, skipped, pll) = run2(19_200_000, 80.0, 200_000, 700.0, &lat, 35.0, 0.37, false);
        assert_eq!(bad, 0, "{bad} of {s}");
        assert!(pll.locked);
        assert!(skipped > 0 && skipped <= 200_000 / 600, "the 20 us spikes are skipped, not misread: {skipped}");
    }

    #[test]
    fn any_start_phase_locks() {
        // No acquisition at all: the first A anywhere in the period. Until
        // locked a read may straddle; after 2000 periods none does.
        for k in 0..20 {
            let ph = k as f64 / 20.0;
            let (_, bad, _, pll) = run2(9_600_000, -100.0, 30_000, 700.0, &|_| 20.0, 35.0, ph, false);
            assert!(pll.locked, "phase {ph}");
            assert!(bad <= 40, "phase {ph}: {bad}");
        }
    }

    #[test]
    fn dma_frames_with_a_late_read_back() {
        // hy3131 stream::dma: the data frame (5 bytes, /16: 13.3 us; /8:
        // 7.2 us) only started at A, the probe read back by C after it.
        for &(rate, frame) in &[(19_200_000u32, 665.0), (38_400_000, 665.0), (76_800_000, 360.0)] {
            for &ppm in &[-100.0, 0.0, 100.0] {
                let jit = |n: u64| ((n.wrapping_mul(2654435761) >> 7) % 40) as f64 + 10.0;
                // Start where hy3131 / logger `isr::start` puts the first A:
                // P - beta after an edge (+ 3 us of edge-search error).
                let p0 = Pll::new(rate, 0, frame as u32);
                let ph = (p0.beta as f64 + 150.0) / (p0.f >> 16) as f64;
                let (s, bad, skipped, pll) = run2(rate, ppm, 200_000, frame, &jit, 35.0, ph, true);
                assert_eq!((bad, skipped), (0, 0), "rate {rate} ppm {ppm}: of {s}");
                assert!(pll.locked, "rate {rate} ppm {ppm}");
                let got = (pll.rate_mhz() as f64 / rate as f64 - 1.0) * 1e6;
                assert!((got - ppm).abs() < 10.0, "rate {rate} ppm {ppm}: {got:.1}");
            }
        }
    }

    /// The hybrid frames' timing (hy3131 `stream::hybrid_kick`, logger.rs
    /// `isr` HYB_*): A's command byte starts `pa` after its entry (+
    /// latency), or after the last probe frame's last bit, and its data's
    /// last bit comes `fa` later; a probe entry's CS goes low `pb` after it
    /// (or after A's last bit: `dma_finish` waits), the chip latches INTF 72
    /// counts later (CS setup, the command byte, one clock), the frame's
    /// last bit 56 after that. Returns (samples, bad, skipped, c_wrong,
    /// pll): bad = a data frame with an edge between its command and its
    /// last bit, or not exactly the next conversion; c_wrong = a probe read
    /// back after the edge that follows the probed one.
    fn run_hyb(rate_mhz: u32, ppm: f64, periods: u32, pa: f64, fa: f64, pb: f64, lat: &dyn Fn(u64) -> f64) -> (u32, u32, u32, u32, Pll) {
        let (r, _) = run_hyb_rel(rate_mhz, ppm, periods, pa, fa, pb, lat, 0);
        r
    }

    /// `run_hyb` from a preset `rel` (the carried word, `Pll::new`); also
    /// returns the bad samples after the first quarter (the steady state).
    #[allow(clippy::too_many_arguments)]
    fn run_hyb_rel(rate_mhz: u32, ppm: f64, periods: u32, pa: f64, fa: f64, pb: f64, lat: &dyn Fn(u64) -> f64, rel: i32) -> ((u32, u32, u32, u32, Pll), u32) {
        let mut pll = Pll::new(rate_mhz, rel, 370);
        pll.late_read = true;
        pll.c_off = 370;
        pll.np = 16;
        pll.gate = true;
        pll.hybrid(160, 82, 25);
        let p_chip = (COUNT_HZ as f64 * 1000.0 / rate_mhz as f64) / (1.0 + ppm * 1e-6);
        // Start as logger.rs `isr::start`: the first A at lead(160) after an
        // edge (+ 3 us of edge-search error).
        let phase0 = pll.lead(160) as f64 + 150.0;
        let edge = |k: i64| phase0 + k as f64 * p_chip;
        let edge_index = |t: f64| ((t - phase0) / p_chip).floor() as i64;
        let (mut t, mut bus_free) = (0.0f64, 0.0f64);
        let (mut samples, mut bad, mut skipped, mut c_wrong) = (0u32, 0u32, 0u32, 0u32);
        let mut bad_late = 0u32;
        let mut last_k: Option<i64> = None;
        let mut b_latch = f64::NAN;
        let mut n = 0u64;
        while samples + skipped < periods {
            let l = lat(n);
            n += 1;
            if pll.next_b || pll.next_c {
                let c = pll.next_c;
                let ok = l <= 100.0 && (!c || l <= pll.c_late as f64);
                let iv = if c { pll.on_c() } else { pll.on_b(ok) };
                if ok {
                    let cs = (t + l + pb).max(bus_free + 10.0);
                    let latch = cs + 72.0;
                    bus_free = latch + 56.0;
                    if c {
                        if edge_index(latch) > edge_index(b_latch) + 1 {
                            c_wrong += 1;
                        }
                        pll.result(edge_index(latch) > edge_index(b_latch));
                    } else {
                        b_latch = latch;
                    }
                }
                t += iv as f64;
            } else {
                let (iv, _) = pll.on_a();
                if pll.take() {
                    if l > pll.late_max() as f64 {
                        last_k = last_k.map(|k| k + 1);
                        skipped += 1;
                    } else {
                        let start = (t + l + pa).max(bus_free + 10.0);
                        let end = start + fa;
                        bus_free = end;
                        let k = edge_index(start);
                        if edge_index(end) != k || edge(k + 1) - end < 0.0 {
                            bad += 1;
                            bad_late += (samples > periods / 4) as u32;
                        }
                        if let Some(k0) = last_k {
                            if k != k0 + 1 {
                                bad += 1;
                                bad_late += (samples > periods / 4) as u32;
                            }
                        }
                        last_k = Some(k);
                        samples += 1;
                    }
                }
                t += iv as f64;
            }
        }
        ((samples, bad, skipped, c_wrong, pll), bad_late)
    }

    #[test]
    fn hybrid_frames_at_76k8() {
        // The device's estimate (pa 50, fa 300, pb 76: A's last bit ~7.3 us,
        // a probe latch ~3.2 us after the entry) and the emulator's faster
        // ISR (pa 15, fa 280, pb 20), 0.2..1 us of latency jitter, +-100 ppm.
        let jit = |n: u64| ((n.wrapping_mul(2654435761) >> 7) % 40) as f64 + 10.0;
        for &(pa, fa, pb) in &[(50.0, 300.0, 76.0), (15.0, 280.0, 20.0), (60.0, 320.0, 90.0)] {
            for &ppm in &[-100.0, 0.0, 100.0] {
                let (s, bad, skipped, c_wrong, pll) = run_hyb(76_800_000, ppm, 200_000, pa, fa, pb, &jit);
                assert_eq!((bad, skipped, c_wrong), (0, 0, 0), "pa {pa} fa {fa} pb {pb} ppm {ppm}: of {s}");
                assert!(pll.locked, "pa {pa} ppm {ppm}");
                let got = (pll.rate_mhz() as f64 / 76_800_000.0 - 1.0) * 1e6;
                assert!((got - ppm).abs() < 10.0, "pa {pa} ppm {ppm}: {got:.1}");
            }
        }
        // The geometry: B 8.6 us after A, A ~1.2 us (device) after an edge.
        let mut p = Pll::new(76_800_000, 0, 370);
        p.c_off = 370;
        p.hybrid(160, 82, 25);
        assert_eq!((p.beta, p.late_max(), p.c_late), (430, 117, 35));
        // A 2 us latency spike every 500 entries: the A still reads (late_max
        // 2.3 us), its frame still ends before the edge; a late C drops its probe.
        let spikes = |n: u64| if n % 500 == 499 { 100.0 } else { 12.0 };
        let (s, bad, skipped, c_wrong, pll) = run_hyb(76_800_000, 50.0, 100_000, 50.0, 300.0, 76.0, &spikes);
        assert_eq!((bad, skipped, c_wrong), (0, 0, 0), "spikes: of {s}");
        assert!(pll.locked);
    }

    #[test]
    fn hybrid_76k8_far_from_the_nominal() {
        // The device's e8cc4e9 state: rate_mhz on the 1000 ppm clamp, 86 %
        // of the probes "edge after B", ~0.25 probes per sample. Before the
        // run hold / dense probing in `result` this model got there from a
        // chip at the nominal with the word started 1000 ppm long (the
        // carried lock of a railed session), and from any chip >= ~600 ppm
        // fast: the B-late side of the hybrid window is ~1.2 us (`hybrid`),
        // beyond it the read-back C reads the next edge and the loop ran the
        // wrong way. Now every chip within -3000 .. +3000 ppm of the nominal
        // from a start at -1000 / 0 / +1000 ppm (period) up to a start 3000
        // ppm slower than the chip: the word on the chip's rate, no bad
        // sample after the first quarter (some while it acquires a large
        // error), probes ~1 in 16 periods.
        let jit = |n: u64| ((n.wrapping_mul(2654435761) >> 7) % 40) as f64 + 10.0;
        for &ppm in &[-3000.0, -2000.0, -900.0, -300.0, 0.0, 300.0, 900.0, 2000.0, 3000.0] {
            for &pre in &[-1000.0f64, 0.0, 1000.0] {
                if ppm + pre > 3000.0 {
                    continue;
                }
                let rel = (pre * 1e-6 * 16_777_216.0) as i32;
                let ((s, bad, skipped, c_wrong, pll), late) = run_hyb_rel(76_800_000, ppm, 200_000, 50.0, 300.0, 76.0, &jit, rel);
                let what = format!("chip {ppm:+} ppm, start {pre:+} ppm (period): bad {bad} ({late} late), of {s}");
                assert_eq!((late, skipped, c_wrong), (0, 0, 0), "{what}");
                assert!(bad < s / 40, "{what}");
                assert!(pll.locked, "{what}");
                let got = (pll.rate_mhz() as f64 / 76_800_000.0 - 1.0) * 1e6;
                assert!((got - ppm).abs() < 10.0, "{what}: rate {got:.1} ppm");
                assert!(pll.probes < s / 10, "{what}: {} probes", pll.probes);
            }
        }
    }

    #[test]
    fn only_a_crystal_sized_lock_is_carried_to_another_rung() {
        let mut p = Pll::new(38_400_000, 0, 370);
        p.locked = true;
        for &(ppm, other) in &[(-1.6, true), (900.0, true), (-1100.0, false), (2943.0, false)] {
            let f = (p.f_nom as f64 * (1.0 + ppm * 1e-6)) as u64;
            p.f_avg = f << 8;
            assert_eq!(p.carry(false).is_some(), other, "{ppm} ppm");
            assert!(p.carry(true).is_some(), "{ppm} ppm, same rung");
        }
        p.locked = false;
        assert_eq!(p.carry(true), None);
    }

    #[test]
    fn the_model_sees_a_bad_lock() {
        // 3 % off (beyond the 5000 ppm clamp and what P/64 phase steps per
        // period can follow): the grid walks through the reads, which the
        // check must see.
        let (_, bad, _) = run(9_600_000, 30_000.0, 20_000, 700.0, &|_| 20.0, 35.0);
        assert!(bad > 10, "{bad}");
    }

    #[test]
    fn frequency_converges_fast() {
        // Within 3 ppm after 10000 periods, with or without 0 .. 4 us of
        // entry-latency jitter (the probe's B read then lands anywhere in a
        // 4 us window around the edge: a noisy phase detector; Renode).
        let jit = |n: u64| ((n.wrapping_mul(2654435761) >> 7) % 200) as f64;
        for &ppm in &[-100.0, 100.0] {
            for j in 0..2 {
                let (_, bad, pll) = if j == 0 {
                    run(4_800_000, ppm, 10_000, 700.0, &|_| 20.0, 35.0)
                } else {
                    run(4_800_000, ppm, 10_000, 700.0, &jit, 35.0)
                };
                let got = (pll.rate_mhz() as f64 / 4_800_000.0 - 1.0) * 1e6;
                assert!((got - ppm).abs() < 3.0, "ppm {ppm} jitter {j}: {got:.1}");
                assert_eq!(bad, 0);
            }
        }
    }

    #[test]
    fn periods_sum_to_the_frequency_word() {
        let mut p = Pll::new(4_800_000, 0, 650);
        let mut total = 0u64;
        let mut a = 0;
        while a < 48_000 {
            if p.next_b {
                total += p.on_b(false) as u64;
            } else {
                total += p.on_a().0 as u64;
                a += 1;
            }
        }
        // 48000 periods of 10416.667 counts (no probes ran: no kicks).
        assert!((total as i64 - 500_000_000).abs() <= 10417, "{total}");
        assert_eq!(Pll::f_of_mhz(4_800_000) >> 16, 10416);
        assert_eq!(p.rel_q24(), 0);
    }
}
