//! Bode plot / frequency response analyser (as the Keysight 1000X-G FRA):
//! the generator sweeps a sine over a log grid (10 Hz .. 50 kHz, the DAC
//! path's sine limit), CH1 is read at each point, and the gain (dB) and
//! phase (degrees) of CH1 against the generator are plotted against log f
//! (View::Bode, src/app_bode.rs, src/ui_bode.rs; firmware/docs/bode.md).
//!
//! This file is the arithmetic only (no `crate::` items), so the host test
//! tools/bode-test builds it as it is:
//! * the grid (`grid_n`, `grid_f`);
//! * the point's timebase (`tb_for`) and V/div step (`range_retry`);
//! * the single-bin DFT of a record against the generator's own phase
//!   (`lockin`): the scope's GEN channel (gen::Sync, docs/scope-gen-channel.md)
//!   gives the generator's phase at every sample of the record, so the
//!   projection on sin / cos of that phase gives CH1's amplitude and its
//!   phase relative to the generator's output (phase 0 = the sine's
//!   mid-rising crossing), over a whole number of periods;
//! * gain / phase against a reference (`relative`): the "Thru" pass (the
//!   probe on the generator's output) taken on the same grid, or the
//!   generator's set amplitude when there is none.
//!
//! No libm on the device: ln, atan2 and sin / cos are series here.

#![allow(dead_code)]

/// Sweep range (Hz): the low end is the slowest timebase without roll
/// (50 ms/div holds 5 periods of 10 Hz), the high end the DAC path's sine
/// limit (genplan DAC_WAVE_MAX).
pub const F_START: f64 = 10.0;
pub const F_STOP: f64 = 50_000.0;
/// Points per decade offered (the page's stepper order, 10 first) and the most points a sweep
/// has (20 per decade: 74 + the end point).
pub const PPD: [u8; 3] = [10, 20, 5];
pub const MAX_PTS: usize = 80;
/// Periods of the point's frequency the screen (10 divisions) holds at
/// least (`tb_for`).
pub const MIN_PERIODS: f64 = 4.0;

/// One measured point: the generator's actual frequency (Hz), CH1's
/// amplitude (V, peak) and its phase against the generator (degrees,
/// -180 .. 180].
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pt {
    pub f: f32,
    pub amp: f32,
    pub ph: f32,
}

impl Pt {
    pub const NONE: Pt = Pt { f: 0.0, amp: 0.0, ph: 0.0 };
}

/// 10^(1 / ppd) for the offered PPD.
fn step_ratio(ppd: u8) -> f64 {
    match ppd {
        5 => 1.584_893_192_461_113_5,
        20 => 1.122_018_454_301_963_4,
        _ => 1.258_925_411_794_167_2,
    }
}

/// Points of the grid at `ppd` per decade: F_START * r^i below F_STOP,
/// then F_STOP.
pub fn grid_n(ppd: u8) -> usize {
    let r = step_ratio(ppd);
    let mut f = F_START;
    let mut n = 0;
    while f < F_STOP * 0.999 && n < MAX_PTS - 1 {
        n += 1;
        f *= r;
    }
    n + 1
}

/// Frequency (Hz) of grid point `i` (0 .. grid_n).
pub fn grid_f(ppd: u8, i: usize) -> f64 {
    if i + 1 >= grid_n(ppd) {
        return F_STOP;
    }
    let r = step_ratio(ppd);
    let mut f = F_START;
    for _ in 0..i {
        f *= r;
    }
    f
}

/// The fastest timebase (index into `sdiv`, s/div; 10 divisions per
/// screen) at or above `lo` whose screen holds MIN_PERIODS periods of `f`,
/// at most `hi`.
pub fn tb_for(f: f64, sdiv: &[f32], lo: usize, hi: usize) -> usize {
    let need = MIN_PERIODS / f.max(1e-3);
    (lo..=hi).find(|&t| 10.0 * sdiv[t] as f64 >= need * 0.999).unwrap_or(hi)
}

/// The V/div range to take the point again at, from a record's code span
/// (`lo` .. `hi`, 10-bit codes) at range `r` (`vdiv` = V/div per range,
/// ascending): clipped (a code at either end) -> the next range up; a swing
/// under a fifth of the codes -> the range that puts it at 40 .. 80 % of
/// them. None: keep the point.
pub fn range_retry(lo: u16, hi: u16, r: usize, vdiv: &[f32]) -> Option<usize> {
    if lo <= 1 || hi >= 1022 {
        return (r + 1 < vdiv.len()).then_some(r + 1);
    }
    let pp = (hi - lo) as f32;
    if pp >= 0.2 * 1023.0 || r == 0 {
        return None;
    }
    // The smallest range with the swing under 80 % of the codes.
    let mut n = r;
    while n > 0 && pp * vdiv[r] / vdiv[n - 1] < 0.8 * 1023.0 {
        n -= 1;
    }
    (n != r).then_some(n)
}

/// What `lockin` found in a record.
#[derive(Clone, Copy, Debug)]
pub struct Lock {
    /// Amplitude (codes, peak) and phase (degrees, -180 .. 180] of the
    /// component at the generator's frequency; the mean code.
    pub amp: f32,
    pub ph: f32,
    pub mean: f32,
    /// Lowest / highest code of the window, samples and whole periods in it.
    pub lo: u16,
    pub hi: u16,
    pub n: u32,
    pub periods: u32,
}

/// Samples between two exact (f64) phase seeds; the phasor is rotated in
/// f32 between them.
const SEED: usize = 256;

/// The component of record `d` (10-bit codes, sample 0 first) at the
/// generator's frequency, against its phase: generator time at sample n is
/// t0 + n s ticks, k periods per q ticks (gen::Sync), and its output is
/// mid + A sin(2 pi periods). A CH1 signal B sin(2 pi periods + theta)
/// gives amp = B (codes) and ph = theta. The window is the longest whole
/// number of periods from sample 0. None: under one period, or no record.
pub fn lockin(d: &[u16], t0: f64, s: f64, q: f64, k: f64) -> Option<Lock> {
    if q <= 0.0 || k <= 0.0 || s <= 0.0 {
        return None;
    }
    let per = q / (k * s); // samples per period
    let m = (d.len() as f64 / per) as u32;
    if m == 0 {
        return None;
    }
    let n = ((m as f64 * per + 0.5) as usize).min(d.len());
    let w = &d[..n];
    let (mut lo, mut hi, mut sum) = (u16::MAX, 0u16, 0u32);
    for &c in w {
        lo = lo.min(c);
        hi = hi.max(c);
        sum += c as u32;
    }
    let mean = sum as f32 / n as f32;
    let tau = 2.0 * core::f64::consts::PI;
    let dph = tau * k * s / q; // radians per sample
    let (rc, rs) = (cos_f(dph as f32), sin_f(dph as f32));
    let (mut ss, mut sc) = (0.0f64, 0.0f64);
    let mut i = 0;
    while i < n {
        // Exact phase at the block's first sample.
        let p = (t0 + i as f64 * s) * k / q;
        let fr = (p - floor_f64(p)) as f32;
        let (mut c, mut sn) = (cos_f(fr * core::f32::consts::TAU), sin_f(fr * core::f32::consts::TAU));
        let e = (i + SEED).min(n);
        let (mut bs, mut bc) = (0.0f32, 0.0f32);
        for &v in &w[i..e] {
            let x = v as f32 - mean;
            bs += x * sn;
            bc += x * c;
            let c2 = c * rc - sn * rs;
            sn = sn * rc + c * rs;
            c = c2;
        }
        ss += bs as f64;
        sc += bc as f64;
        i = e;
    }
    let (ss, sc) = (ss as f32, sc as f32);
    let amp = 2.0 * sqrt_f(ss * ss + sc * sc) / n as f32;
    let ph = wrap_deg(atan2_f(sc, ss) * (180.0 / core::f32::consts::PI));
    Some(Lock { amp, ph, mean, lo, hi, n: n as u32, periods: m })
}

/// Gain (dB) and phase (degrees, -180 .. 180]) of `p` against reference
/// amplitude `ref_amp` (V, peak) and phase `ref_ph` (degrees).
pub fn relative(p: &Pt, ref_amp: f32, ref_ph: f32) -> (f32, f32) {
    let g = if p.amp > 0.0 && ref_amp > 0.0 { 20.0 * log10_f(p.amp / ref_amp) } else { -200.0 };
    (g, wrap_deg(p.ph - ref_ph))
}

/// Degrees into (-180, 180].
pub fn wrap_deg(x: f32) -> f32 {
    let mut x = x;
    while x > 180.0 {
        x -= 360.0;
    }
    while x <= -180.0 {
        x += 360.0;
    }
    x
}

/// Gain (dB) and phase where the gain first falls 3 dB below the first
/// point's (linear interpolation in log f): the -3 dB frequency, if any.
pub fn minus3db(f: &[f32], g: &[f32]) -> Option<f32> {
    let n = f.len().min(g.len());
    if n < 2 {
        return None;
    }
    let t = g[0] - 3.0;
    for i in 1..n {
        if g[i] <= t && g[i - 1] > t {
            let u = (g[i - 1] - t) / (g[i - 1] - g[i]);
            let (a, b) = (ln_f(f[i - 1]), ln_f(f[i]));
            return Some(exp_f(a + u * (b - a)));
        }
    }
    None
}

// --- maths without libm -----------------------------------------------------

fn floor_f64(x: f64) -> f64 {
    let t = x as i64 as f64;
    if t > x { t - 1.0 } else { t }
}

/// sin(x) for |x| <= pi / 2 (Taylor to x^11).
fn sin_q(x: f32) -> f32 {
    let x2 = x * x;
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0 * (1.0 - x2 / 110.0)))))
}

pub fn sin_f(x: f32) -> f32 {
    use core::f32::consts::PI;
    let mut x = x;
    while x > PI {
        x -= 2.0 * PI;
    }
    while x < -PI {
        x += 2.0 * PI;
    }
    let x = if x > PI / 2.0 { PI - x } else if x < -PI / 2.0 { -PI - x } else { x };
    sin_q(x)
}

pub fn cos_f(x: f32) -> f32 {
    sin_f(x + core::f32::consts::FRAC_PI_2)
}

pub fn sqrt_f(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut y = f32::from_bits((x.to_bits() >> 1) + 0x1FC0_0000);
    for _ in 0..4 {
        y = 0.5 * (y + x / y);
    }
    y
}

/// atan(x), |x| <= tan(pi / 12): Taylor to x^13.
fn atan_s(x: f32) -> f32 {
    let x2 = x * x;
    x * (1.0 - x2 * (1.0 / 3.0 - x2 * (1.0 / 5.0 - x2 * (1.0 / 7.0 - x2 * (1.0 / 9.0 - x2 * (1.0 / 11.0 - x2 / 13.0))))))
}

/// atan(x), any x.
fn atan_f(x: f32) -> f32 {
    use core::f32::consts::FRAC_PI_2;
    const T15: f32 = 0.267_949_2; // tan(pi / 12)
    const R3: f32 = 0.577_350_26; // 1 / sqrt(3)
    let (neg, a) = if x < 0.0 { (true, -x) } else { (false, x) };
    let (inv, a) = if a > 1.0 { (true, 1.0 / a) } else { (false, a) };
    let r = if a > T15 { core::f32::consts::FRAC_PI_6 + atan_s((a - R3) / (1.0 + a * R3)) } else { atan_s(a) };
    let r = if inv { FRAC_PI_2 - r } else { r };
    if neg { -r } else { r }
}

/// atan2(y, x), radians (-pi, pi].
pub fn atan2_f(y: f32, x: f32) -> f32 {
    use core::f32::consts::{FRAC_PI_2, PI};
    if x > 0.0 {
        atan_f(y / x)
    } else if x < 0.0 {
        if y >= 0.0 { atan_f(y / x) + PI } else { atan_f(y / x) - PI }
    } else if y > 0.0 {
        FRAC_PI_2
    } else if y < 0.0 {
        -FRAC_PI_2
    } else {
        0.0
    }
}

/// ln(x), x > 0: exponent * ln 2 + the atanh series of the mantissa
/// (1 .. 2, centred on sqrt 2).
pub fn ln_f(x: f32) -> f32 {
    if x <= 0.0 || !x.is_finite() {
        return f32::NEG_INFINITY;
    }
    let b = x.to_bits();
    let mut e = ((b >> 23) & 0xFF) as i32 - 127;
    let mut m = f32::from_bits((b & 0x007F_FFFF) | 0x3F80_0000);
    if m > core::f32::consts::SQRT_2 {
        m *= 0.5;
        e += 1;
    }
    let t = (m - 1.0) / (m + 1.0);
    let t2 = t * t;
    let s = 2.0 * t * (1.0 + t2 * (1.0 / 3.0 + t2 * (1.0 / 5.0 + t2 * (1.0 / 7.0 + t2 / 9.0))));
    e as f32 * core::f32::consts::LN_2 + s
}

pub fn log10_f(x: f32) -> f32 {
    ln_f(x) * core::f32::consts::LOG10_E
}

/// e^x (|x| < 80): 2^integer by the exponent bits, the rest by Taylor.
pub fn exp_f(x: f32) -> f32 {
    let y = x * core::f32::consts::LOG2_E;
    let n = if y >= 0.0 { y as i32 } else { y as i32 - 1 };
    let r = (y - n as f32) * core::f32::consts::LN_2; // 0 .. ln 2
    let p = 1.0 + r * (1.0 + r / 2.0 * (1.0 + r / 3.0 * (1.0 + r / 4.0 * (1.0 + r / 5.0 * (1.0 + r / 6.0 * (1.0 + r / 7.0))))));
    let e = (n + 127).clamp(1, 254) as u32;
    p * f32::from_bits(e << 23)
}
