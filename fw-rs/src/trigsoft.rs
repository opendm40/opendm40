//! Software pulse-width and runt triggers (UF-13), searched in a complete
//! linear record (scope.rs `finish`; the comparator and the continuous
//! stream search only know edges). Pure, host-tested by tools/trig-test.
//!
//! Limits (by construction, stated in the Trigger tab notes and the TODO):
//!  * not a hardware trigger: each record (scope::CAP = 26624 samples) is
//!    searched after it is complete, so an event in the dead time between
//!    two records (search + drawing + restart) is missed; a rare glitch is
//!    caught with probability ~ record time / (record time + dead time);
//!  * only events that lie wholly inside one record and leave a full screen
//!    around the trigger point (the trigger position) count;
//!  * width resolution = one sample (1 / sample rate of the timebase), the
//!    10-bit codes with +-2 codes of hysteresis around each level;
//!  * slow timebases need the whole record per frame (26624 samples: 0.5 ms
//!    at 50 MS/s, 0.2 s at 125 kS/s, 10.6 s at 1 s/div);
//!  * roll mode has no trigger (as the edge trigger).

/// Trigger types (Settings::ttype).
pub const EDGE: u8 = 0;
pub const PULSE: u8 = 1;
pub const RUNT: u8 = 2;
pub const TYPE_NAMES: [&str; 3] = ["Edge", "Pulse", "Runt"];
/// Pulse-width conditions (Settings::pcond).
pub const GT: u8 = 0;
pub const LT: u8 = 1;
pub const EQ: u8 = 2;
pub const COND_NAMES: [&str; 3] = [">", "<", "="];
/// Hysteresis around a level, codes (as the edge search).
const HYS: i32 = 2;

/// Pulse-width trigger: the first pulse whose trailing edge lies in
/// [lo, hi) and whose width (samples between the leading and trailing
/// crossings of level `l`) meets `cond` against `w` samples (`EQ`: within
/// +-5 %, at least +-1 sample). `positive`: a high pulse (rising, then
/// falling), else a low one. Returns the trailing-edge index.
pub fn pulse(d: &[u16], lo: usize, hi: usize, l: i32, positive: bool, cond: u8, w: f32) -> Option<usize> {
    let hi = hi.min(d.len());
    // state: 0 waiting for the far side (arm), 1 armed, 2 inside a pulse
    // (start = leading edge), the trailing edge needs the near side first.
    let (mut armed, mut start, mut inside) = (false, 0usize, false);
    let mut deep = false;
    let tol = (w * 0.05).max(1.0);
    for (i, &v) in d[..hi].iter().enumerate() {
        let v = v as i32;
        let (pre, past, back) = if positive { (v < l - HYS, v > l, v < l) } else { (v > l + HYS, v < l, v > l) };
        let deep_now = if positive { v > l + HYS } else { v < l - HYS };
        if !inside {
            if pre {
                armed = true;
            } else if armed && past {
                inside = true;
                deep = deep_now;
                start = i;
            }
        } else {
            deep |= deep_now;
            if deep && back {
                let width = (i - start) as f32;
                let ok = match cond {
                    GT => width > w,
                    LT => width < w,
                    _ => (width - w).abs() <= tol,
                };
                if ok && i >= lo {
                    return Some(i);
                }
                inside = false;
                armed = pre;
            }
        }
    }
    None
}

/// Runt trigger between levels `l1` and `l2` (any order): `positive`: a
/// pulse that rises through the lower level and falls back below it
/// without reaching the upper one; else a pulse that falls through the
/// upper level and returns above it without reaching the lower one. The
/// return crossing in [lo, hi) is the trigger point.
pub fn runt(d: &[u16], lo: usize, hi: usize, l1: i32, l2: i32, positive: bool) -> Option<usize> {
    let hi = hi.min(d.len());
    let (a, b) = (l1.min(l2), l1.max(l2));
    if b - a <= 2 * HYS {
        return None;
    }
    let (mut armed, mut inside, mut reached, mut deep) = (false, false, false, false);
    for (i, &v) in d[..hi].iter().enumerate() {
        let v = v as i32;
        if positive {
            if !inside {
                if v < a - HYS {
                    armed = true;
                } else if armed && v > a {
                    inside = true;
                    reached = v > b;
                    deep = v > a + HYS;
                }
            } else {
                reached |= v > b;
                deep |= v > a + HYS;
                if deep && v < a {
                    if !reached && i >= lo {
                        return Some(i);
                    }
                    inside = false;
                    armed = v < a - HYS;
                }
            }
        } else if !inside {
            if v > b + HYS {
                armed = true;
            } else if armed && v < b {
                inside = true;
                reached = v < a;
                deep = v < b - HYS;
            }
        } else {
            reached |= v < a;
            deep |= v < b - HYS;
            if deep && v > b {
                if !reached && i >= lo {
                    return Some(i);
                }
                inside = false;
                armed = v > b + HYS;
            }
        }
    }
    None
}

/// Pulse-width steps (s): 1-2-5 values from 20 ns to 10 s.
pub const W_MIN: f32 = 20e-9;
pub const W_MAX: f32 = 10.0;

/// The next 1-2-5 width above (`up`) or below `w`, clamped to W_MIN..W_MAX.
pub fn width_step(w: f32, up: bool) -> f32 {
    let mut dec = 1e-8f32;
    let mut prev = W_MIN;
    while dec < 20.0 {
        for k in [1.0f32, 2.0, 5.0] {
            let x = dec * k;
            if !(W_MIN * 0.99..=W_MAX * 1.01).contains(&x) {
                continue;
            }
            if up && x > w * 1.001 {
                return x;
            }
            if !up && x >= w * 0.999 {
                return prev;
            }
            prev = x;
        }
        dec *= 10.0;
    }
    if up { W_MAX } else { prev }
}
