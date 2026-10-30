//! Peak +/- and crest factor (TODO FR-03, as the Fluke 287/289 Peak, the
//! Hioki DT4282 and the Brymen BM869s crest): the pure part, over the raw
//! AD2 codes of the DMM's AC row (the wave view's stream, src/logger.rs
//! `wave`). No register access: host-tested by tools/meter-test (`tests`
//! below); the app glue is src/app_peak.rs / src/app_peak_live.rs.
//!
//! P+ is the largest value that two adjacent samples both reach (the max
//! over i of min(x[i], x[i + 1])), P- the symmetric minimum (FR-03: a
//! filter of 2 consecutive samples against single-sample noise spikes).
//! RMS is over the same samples (DC included: the DC-coupled rows' DC
//! part counts), the crest factor max(|P+|, |P-|) / RMS. Everything is kept
//! in codes; `Acc::si` scales with the row's c * per_code - offset.

/// The accumulator (main loop only). All-zero is `Acc::NEW` (it lives in
/// `.sram_bss`).
#[derive(Clone, Copy)]
pub struct Acc {
    /// Samples taken (the RMS's n).
    pub n: u32,
    /// The filtered extremes so far (valid once `pairs` > 0).
    hi: i32,
    lo: i32,
    /// The previous sample, valid while `have_prev` (a hole in the stream
    /// clears it: two samples around a hole are not adjacent).
    prev: i32,
    have_prev: bool,
    /// Adjacent pairs seen (P+ / P- exist).
    pairs: bool,
    /// Sum of the codes and of their squares (f64: no overflow over hours
    /// at 38 400 S/s; a pass adds an exact i64 / u64 block, `push_block`).
    s1: f64,
    s2: f64,
}

/// P+, P-, RMS (SI) and the crest factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Res {
    pub max: f32,
    pub min: f32,
    pub rms: f32,
    pub crest: f32,
}

impl Acc {
    pub const NEW: Acc = Acc { n: 0, hi: 0, lo: 0, prev: 0, have_prev: false, pairs: false, s1: 0.0, s2: 0.0 };

    pub fn clear(&mut self) {
        *self = Acc::NEW;
    }

    /// The next sample is not adjacent to the last one (a hole).
    pub fn hole(&mut self) {
        self.have_prev = false;
    }

    /// Feed samples through `feed`, which calls its argument once per raw
    /// code (logger::wave::drain): the extremes per sample, the sums per
    /// call in exact integers. The count of samples.
    #[inline(always)]
    pub fn push_block(&mut self, feed: &mut dyn FnMut(&mut dyn FnMut(i32)) -> u32) -> u32 {
        let (mut s1, mut s2) = (0i64, 0u64);
        let (mut hi, mut lo, mut prev, mut hp, mut pairs) = (self.hi, self.lo, self.prev, self.have_prev, self.pairs);
        let n = feed(&mut |c| {
            s1 += c as i64;
            s2 += (c as i64 * c as i64) as u64;
            if hp {
                let (a, b) = if c < prev { (c, prev) } else { (prev, c) };
                if !pairs {
                    (hi, lo, pairs) = (a, b, true);
                } else {
                    hi = hi.max(a);
                    lo = lo.min(b);
                }
            }
            prev = c;
            hp = true;
        });
        (self.hi, self.lo, self.prev, self.have_prev, self.pairs) = (hi, lo, prev, hp, pairs);
        self.n = self.n.saturating_add(n);
        self.s1 += s1 as f64;
        self.s2 += s2 as f64;
        n
    }

    /// One sample (tests; `push_block` in the firmware).
    #[cfg(test)]
    pub fn push(&mut self, c: i32) {
        self.push_block(&mut |f| {
            f(c);
            1
        });
    }

    /// The results in SI for a row whose value is c * per_code - offset;
    /// None before two adjacent samples.
    pub fn si(&self, per_code: f32, offset: f32) -> Option<Res> {
        if !self.pairs || self.n == 0 {
            return None;
        }
        // The mean square of (c k - o): k^2 var(c) + (k mean(c) - o)^2; the
        // variance in f64 (s2 / n and mean^2 nearly cancel on a DC input).
        let n = self.n as f64;
        let m1 = self.s1 / n;
        let var = (self.s2 / n - m1 * m1) as f32;
        let dc = m1 as f32 * per_code - offset;
        let rms = sqrt(per_code * per_code * var.max(0.0) + dc * dc);
        let (a, b) = (self.hi as f32 * per_code - offset, self.lo as f32 * per_code - offset);
        // A negative scale (none today) swaps the extremes.
        let (max, min) = if a >= b { (a, b) } else { (b, a) };
        let pk = if max.abs() > min.abs() { max.abs() } else { min.abs() };
        let crest = if rms > 0.0 { pk / rms } else { f32::NAN };
        Some(Res { max, min, rms, crest })
    }
}

/// sqrt without libm (Newton from a bit-trick guess, as inrush.rs).
fn sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut y = f32::from_bits((x.to_bits() >> 1) + 0x1FBD_1DF5);
    for _ in 0..4 {
        y = 0.5 * (y + x / y);
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: impl Fn(usize) -> i32, n: usize) -> Acc {
        let mut a = Acc::NEW;
        for i in 0..n {
            a.push(f(i));
        }
        a
    }

    #[test]
    fn sine_crest_is_sqrt2() {
        let a = run(|i| (100_000.0 * (2.0 * core::f32::consts::PI * 50.0 * i as f32 / 38_400.0).sin()).round() as i32, 38_400);
        let r = a.si(1e-5, 0.0).unwrap();
        assert!((r.max - 1.0).abs() < 0.002 && (r.min + 1.0).abs() < 0.002, "{r:?}");
        assert!((r.crest - core::f32::consts::SQRT_2).abs() < 0.005, "{r:?}");
    }

    #[test]
    fn square_crest_is_one() {
        let a = run(|i| if (i / 384) % 2 == 0 { 50_000 } else { -50_000 }, 38_400);
        let r = a.si(2e-5, 0.0).unwrap();
        assert!((r.max - 1.0).abs() < 1e-4 && (r.min + 1.0).abs() < 1e-4 && (r.crest - 1.0).abs() < 1e-3, "{r:?}");
    }

    #[test]
    fn one_sample_spike_is_rejected_two_are_not() {
        let mut a = run(|_| 1000, 100);
        a.push(9000);
        a.push(1000);
        let r = a.si(1.0, 0.0).unwrap();
        assert_eq!((r.max, r.min), (1000.0, 1000.0));
        a.push(-7000);
        a.push(-7000);
        let r = a.si(1.0, 0.0).unwrap();
        assert_eq!(r.min, -7000.0);
    }

    #[test]
    fn a_hole_breaks_adjacency() {
        let mut a = run(|_| 0, 10);
        a.push(500);
        a.hole();
        a.push(500);
        a.push(0);
        assert_eq!(a.si(1.0, 0.0).unwrap().max, 0.0);
    }

    #[test]
    fn dc_with_offset_and_scale() {
        // c * 2e-4 - 0.1 with c = 30 000: 5.9 V DC, CF 1.
        let a = run(|_| 30_000, 1000);
        let r = a.si(2e-4, 0.1).unwrap();
        assert!((r.max - 5.9).abs() < 1e-4 && (r.rms - 5.9).abs() < 1e-3 && (r.crest - 1.0).abs() < 1e-3, "{r:?}");
    }

    #[test]
    fn none_before_a_pair() {
        let mut a = Acc::NEW;
        assert!(a.si(1.0, 0.0).is_none());
        a.push(1);
        assert!(a.si(1.0, 0.0).is_none());
        a.push(1);
        assert!(a.si(1.0, 0.0).is_some());
    }
}
