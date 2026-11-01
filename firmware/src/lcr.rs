//! LCR meter (docs/lcr.md): the impedance of a DUT from one Power Wave
//! frame (src/pwave.rs `capture`: U and I sampled in turn, every sample
//! stamped) with the generator driving a sine. The pure part: the fit of
//! both channels at the test frequency, the corrections, the readouts and
//! the I range choice (host tests below, tools/meter-test). The page and
//! the generator are src/app_lcr.rs, src/ui_lcr.rs.
//!
//! Main loop only, once per frame (~200 ms): region C (.cold,
//! link-native.x), like the page.

#![allow(dead_code)]

use crate::pwave::{Cap, Scale, SLOT_CYC, T_AT, T_UNIT_US};

/// pwave's read spacing in us (156.25).
const SLOT_US: f32 = SLOT_CYC as f32 / (crate::clock::HCLK_HZ / 1_000_000) as f32;

const PI: f32 = core::f32::consts::PI;

/// Test frequencies (Hz): the page's button, `LCR:FREQ`.
pub const FREQS: [u32; 2] = [100, 1000];
/// Frame length (us): whole cycles of 100 Hz and 1 kHz, and of 50 / 60 Hz
/// mains; 640 samples per channel (pwave: at most 2048 in all, 262 ms).
pub const SPAN_US: u32 = 200_000;
/// Generator drive: Vpp and the centre (the output is unipolar 0..3 V).
pub const DRIVE_VPP: f32 = 3.0;
pub const DRIVE_OFS: f32 = 1.5;
/// I ranges used (power meter index k): 600 uA, 6 mA (47.5 ohm x10 / x1),
/// 60 mA (0.5 ohm x10). The start: 60 mA (a short draws ~30 mA peak).
pub const K_MAX: u8 = 2;
pub const K_START: u8 = 2;
/// Up-range above this |raw code| of the I peak (0.6 V at the converter:
/// OP1's output swing, INFERRED from the +0.7 V of doc 3.2), down below
/// DOWN_RAW (0.05 V: x10 later is 0.5 V).
pub const UP_RAW: i32 = 125_800;
pub const DOWN_RAW: i32 = 10_500;
/// The main reading is R below this |Q| (|theta| < 5.7 deg).
pub const Q_R: f32 = 0.1;
/// AUTO model: series below this |Z| (ohm), parallel above.
pub const AUTO_OHM: f32 = 1000.0;

/// A complex number.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cx {
    pub re: f32,
    pub im: f32,
}

impl Cx {
    pub const fn new(re: f32, im: f32) -> Cx {
        Cx { re, im }
    }
    pub fn add(self, o: Cx) -> Cx {
        Cx::new(self.re + o.re, self.im + o.im)
    }
    pub fn sub(self, o: Cx) -> Cx {
        Cx::new(self.re - o.re, self.im - o.im)
    }
    pub fn mul(self, o: Cx) -> Cx {
        Cx::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
    pub fn scale(self, k: f32) -> Cx {
        Cx::new(self.re * k, self.im * k)
    }
    pub fn abs2(self) -> f32 {
        self.re * self.re + self.im * self.im
    }
    pub fn abs(self) -> f32 {
        sqrtf(self.abs2())
    }
    /// 1 / self (NaN for 0).
    pub fn inv(self) -> Cx {
        let d = self.abs2();
        Cx::new(self.re / d, -self.im / d)
    }
    pub fn div(self, o: Cx) -> Cx {
        self.mul(o.inv())
    }
    /// e^(j a).
    pub fn cis(a: f32) -> Cx {
        let (s, c) = sin_cos(a);
        Cx::new(c, s)
    }
    pub fn arg(self) -> f32 {
        atan2(self.im, self.re)
    }
    pub fn is_finite(self) -> bool {
        self.re.is_finite() && self.im.is_finite()
    }
}

/// sqrt without libm (Newton from a bit-trick guess, as inrush::sqrtf).
pub fn sqrtf(x: f32) -> f32 {
    if !(x > 0.0) {
        return if x == 0.0 { 0.0 } else { f32::NAN };
    }
    if x == f32::INFINITY {
        return x;
    }
    let mut y = f32::from_bits((x.to_bits() >> 1) + 0x1FBD_1DF5);
    for _ in 0..4 {
        y = 0.5 * (y + x / y);
    }
    y
}

/// (sin, cos) of `a` in turns' radians, any finite `a` (reduced to
/// [-pi, pi] by whole turns, then a Taylor polynomial on [-pi/2, pi/2];
/// |error| < 1e-6).
pub fn sin_cos(a: f32) -> (f32, f32) {
    let t = a / (2.0 * PI);
    let t = t - round(t);
    sin_cos_turn(t)
}

/// (sin, cos) of 2 pi t for t in [-0.5, 0.5].
fn sin_cos_turn(t: f32) -> (f32, f32) {
    (sin_q(t), sin_q(0.25 - t))
}

/// sin(2 pi t), t in [-0.5, 0.75].
fn sin_q(t: f32) -> f32 {
    // Fold to [-0.25, 0.25] turns (sin(pi - x) = sin x).
    let t = if t > 0.25 { 0.5 - t } else if t < -0.25 { -0.5 - t } else { t };
    let x = 2.0 * PI * t;
    let x2 = x * x;
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0 * (1.0 - x2 / 110.0)))))
}

/// Nearest integer as f32 (no libm; |x| < 2^23).
fn round(x: f32) -> f32 {
    (if x >= 0.0 { x + 0.5 } else { x - 0.5 }) as i32 as f32
}

/// atan2 without libm (|error| < 2e-5 rad).
pub fn atan2(y: f32, x: f32) -> f32 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let (ax, ay) = (x.abs(), y.abs());
    let (r, swap) = if ay > ax { (ax / ay, true) } else { (ay / ax, false) };
    let r2 = r * r;
    let mut a = r * (0.999_977_26 + r2 * (-0.332_623_47 + r2 * (0.193_543_46 + r2 * (-0.116_432_87 + r2 * (0.052_653_32 - 0.011_721_2 * r2)))));
    if swap {
        a = PI / 2.0 - a;
    }
    if x < 0.0 {
        a = PI - a;
    }
    if y < 0.0 { -a } else { a }
}

/// One frame's fit: the phasors of U (V) and I (A) at the test frequency
/// (v(t) = Re(P e^(j w t)), t from the frame's first sample), the samples
/// used per channel, the largest |raw code| of I and whether a sample hit
/// a converter rail.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fit {
    pub u: Cx,
    pub i: Cx,
    pub n: u32,
    pub ipk_raw: i32,
    pub clip_u: bool,
    pub clip_i: bool,
}

impl Fit {
    /// The DUT side's raw impedance U / I (ohm).
    pub fn z(&self) -> Cx {
        self.u.div(self.i)
    }
}

/// Least-squares sine + DC at `f_hz` on one channel's samples: sums of
/// the 3 x 3 normal equations (1, cos, sin).
#[derive(Default)]
struct Acc {
    n: f32,
    c: f32,
    s: f32,
    cc: f32,
    ss: f32,
    cs: f32,
    v: f32,
    vc: f32,
    vs: f32,
}

impl Acc {
    fn add(&mut self, c: f32, s: f32, v: f32) {
        self.n += 1.0;
        self.c += c;
        self.s += s;
        self.cc += c * c;
        self.ss += s * s;
        self.cs += c * s;
        self.v += v;
        self.vc += v * c;
        self.vs += v * s;
    }

    /// The phasor A - jB of v = A cos + B sin + D (Cramer's rule on the
    /// symmetric system; None when singular).
    fn solve(&self) -> Option<Cx> {
        let m = [[self.n, self.c, self.s], [self.c, self.cc, self.cs], [self.s, self.cs, self.ss]];
        let r = [self.v, self.vc, self.vs];
        let det3 = |m: &[[f32; 3]; 3]| {
            m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
        };
        let d = det3(&m);
        if !(d.abs() > 1e-6 * self.n * self.n * self.n) {
            return None;
        }
        let col = |k: usize| {
            let mut a = m;
            for (row, rv) in a.iter_mut().zip(r) {
                row[k] = rv;
            }
            det3(&a) / d
        };
        Some(Cx::new(col(1), -col(2)))
    }
}

/// Fit frame `c` (`buf` as pwave::capture fills it) at `f_hz`: the
/// samples within the whole test cycles from the first sample; None with
/// fewer than 2 cycles or 16 samples of either channel.
#[inline(never)]
pub fn fit(buf: &[u16], c: &Cap, sc: &Scale, f_hz: f32) -> Option<Fit> {
    if c.n < 4 || !(f_hz > 0.0) {
        return None;
    }
    // A sample's time: its read is due k slots after the first (pwave
    // `capture`); a stamp within one unit (4 us, truncated) of that is
    // a read on time: the slot time, else the stamp + half a unit. (The
    // truncation alone differs by 0.25 us between the U and the I reads:
    // 0.1 deg at 1 kHz.)
    let t_of = |k: usize| {
        let st = (buf[T_AT + k] as u32 * T_UNIT_US) as f32;
        let due = k as f32 * SLOT_US;
        (if st <= due && st > due - T_UNIT_US as f32 { due } else { st + 0.5 * T_UNIT_US as f32 }) * 1e-6
    };
    let cycles = (t_of(c.n - 1) * f_hz) as u32;
    if cycles < 2 {
        return None;
    }
    let t_end = cycles as f32 / f_hz;
    let (mut au, mut ai) = (Acc::default(), Acc::default());
    let mut ipk = 0i32;
    for k in 0..c.n {
        let t = t_of(k);
        if t >= t_end {
            break;
        }
        let ph = t * f_hz;
        let (s, co) = sin_cos_turn(ph - round(ph));
        let v = buf[k] as i16 as f32;
        if c.is_u(k) {
            au.add(co, s, v);
        } else {
            ai.add(co, s, v);
            ipk = ipk.max((buf[k] as i16 as i32).abs() * 8);
        }
    }
    if au.n < 16.0 || ai.n < 16.0 {
        return None;
    }
    let (u, i) = (au.solve()?, ai.solve()?);
    Some(Fit { u: u.scale(sc.su), i: i.scale(sc.si), n: (au.n + ai.n) as u32, ipk_raw: ipk, clip_u: c.clip_u, clip_i: c.clip_i })
}

/// The I range for the next frame after one on `k` with I peak `ipk_raw`
/// (raw code, DC + AC) and a rail hit `clip`.
pub fn next_range(k: u8, ipk_raw: i32, clip: bool) -> u8 {
    if (clip || ipk_raw > UP_RAW) && k < K_MAX {
        k + 1
    } else if ipk_raw < DOWN_RAW && !clip && k > 0 {
        k - 1
    } else {
        k.min(K_MAX)
    }
}

/// The corrections of one I range and frequency (docs/lcr.md): the short
/// (the raw Z with the DUT clips joined), the delay of I behind U (s), the
/// open admittance (S, after the short and the delay).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corr {
    pub short: Cx,
    pub tau: f32,
    pub open: Cx,
}

impl Corr {
    /// No calibration: the bare shunt `r_path` (ohm) as the short.
    pub const fn none(r_path: f32) -> Corr {
        Corr { short: Cx::new(r_path, 0.0), tau: 0.0, open: Cx::new(0.0, 0.0) }
    }
}

/// The DUT's impedance from the raw U / I `zm` at `w` (rad/s).
pub fn correct(zm: Cx, k: &Corr, w: f32) -> Cx {
    let z = zm.sub(k.short).mul(Cx::cis(-w * k.tau));
    if k.open.abs2() > 0.0 { z.inv().sub(k.open).inv() } else { z }
}

/// The delay that makes a resistor's corrected Z real (`LCR:CORR:PHAS`):
/// from its raw `zm` with the short `k.short`. None for a DUT that is not
/// mostly resistive (|theta| > 10 deg) or no current.
pub fn tau_from_resistor(zm: Cx, k: &Corr, w: f32) -> Option<f32> {
    let z = zm.sub(k.short);
    let a = z.arg();
    (z.re > 0.0 && a.abs() < 10.0 * PI / 180.0 && w > 0.0).then_some(a / w)
}

/// Equivalent circuit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Auto,
    Series,
    Parallel,
}

/// What the main reading is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    L,
    C,
    R,
}

/// The readouts of Z at `w`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Res {
    pub z: f32,
    /// Phase, degrees (+ inductive).
    pub theta: f32,
    /// The model used (Series / Parallel).
    pub par: bool,
    /// ESR (series) or Rp (parallel), ohm.
    pub r: f32,
    /// Series X or parallel Xp, ohm.
    pub x: f32,
    /// L (H) or C (F) of the model, by the sign of X.
    pub l: f32,
    pub c: f32,
    pub d: f32,
    pub q: f32,
    pub kind: Kind,
}

/// The readouts of `z` at `w` in `model`.
pub fn solve(z: Cx, w: f32, model: Model) -> Res {
    let a2 = z.abs2();
    let zabs = sqrtf(a2);
    let par = match model {
        Model::Auto => zabs > AUTO_OHM,
        Model::Series => false,
        Model::Parallel => true,
    };
    let (r, x) = if par { (a2 / z.re, a2 / z.im) } else { (z.re, z.im) };
    let d = if z.im != 0.0 { z.re / z.im.abs() } else { f32::INFINITY };
    let q = if z.re != 0.0 { z.im.abs() / z.re } else { f32::INFINITY };
    let kind = if q < Q_R && z.re > 0.0 {
        Kind::R
    } else if z.im > 0.0 {
        Kind::L
    } else {
        Kind::C
    };
    Res { z: zabs, theta: z.arg() * 180.0 / PI, par, r, x, l: x / w, c: -1.0 / (w * x), d, q, kind }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pwave::{code_of, BUF_MIN};

    const W1K: f32 = 2.0 * PI * 1000.0;

    /// A frame as pwave::capture fills it: U and I in turn every 156.25 us
    /// (times in 4 us units, truncated), U first; u = dc_u + |U| cos(w t +
    /// arg U), i likewise; codes = value / scale + offset, stored as
    /// code_of(raw) (raw = 8 x code), with `noise` raw codes of
    /// pseudo-random noise.
    fn frame(f: f32, u: Cx, i: Cx, dc_u: f32, dc_i: f32, sc: &Scale, noise: i32) -> (Vec<u16>, Cap) {
        let mut b = vec![0u16; BUF_MIN];
        let mut seed = 12345u32;
        let mut k = 0;
        loop {
            // Every 37th read held up 30 us (retries): stamped late.
            let t_us = k as f32 * 156.25 + if k % 37 == 5 { 30.0 } else { 0.0 };
            if t_us > SPAN_US as f32 + 100.0 || k >= crate::pwave::NMAX {
                break;
            }
            let y = k % 2 == 0;
            let w = 2.0 * PI * f * t_us * 1e-6;
            let (p, dc, s, o) = if y { (u, dc_u, sc.su, sc.ou) } else { (i, dc_i, sc.si, sc.oi) };
            let v = dc + p.re * (w as f64).cos() as f32 - p.im * (w as f64).sin() as f32;
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let nz = if noise > 0 { ((seed >> 16) as i32 % (2 * noise + 1)) - noise } else { 0 };
            let raw = ((v / s + o) * 8.0).round() as i32 + nz;
            b[k] = code_of(raw) as u16;
            b[T_AT + k] = (t_us as u32 / T_UNIT_US) as u16;
            k += 1;
        }
        (b, Cap { n: k, y_first: true, ..Cap::default() })
    }

    /// The power row's scales: 6 V range (47.7 uV per raw code at the
    /// input), the I range `k`'s (0.5 ohm x10 = 0.954 uA, 47.5 ohm x10 =
    /// 0.01 uA per raw code).
    fn sc(k: u8) -> Scale {
        let code = 1.25 / 262_144.0;
        let sh = [47.5 * 10.0, 47.5, 0.5 * 10.0][k as usize];
        Scale { su: 8.0 * code * 10.0, ou: 3.0, si: 8.0 * code / sh, oi: -5.0 }
    }

    /// The generator (1.5 V DC + 1.5 V peak, 50 ohm) on Z_dut + the shunt
    /// path `rp`: U at V-COM and I; `rdc` the DUT's DC resistance
    /// (infinite for a capacitor).
    fn drive(zd: Cx, rdc: f32, rp: f32) -> (Cx, Cx, f32, f32) {
        let zt = zd.add(Cx::new(rp, 0.0));
        let i = Cx::new(1.5, 0.0).div(zt.add(Cx::new(50.0, 0.0)));
        let u = i.mul(zt);
        let idc = 1.5 / (rdc + rp + 50.0);
        (u, i, 1.5 - 50.0 * idc, idc)
    }

    fn measure(zd: Cx, f: f32, k: u8, noise: i32) -> Cx {
        measure_dc(zd, zd.re, f, k, noise)
    }

    fn measure_dc(zd: Cx, rdc: f32, f: f32, k: u8, noise: i32) -> Cx {
        let rp = [47.5, 47.5, 0.5][k as usize];
        let s = sc(k);
        let (u, i, udc, idc) = drive(zd, rdc, rp);
        let (b, c) = frame(f, u, i, udc, idc, &s, noise);
        let ft = fit(&b, &c, &s, f).unwrap();
        correct(ft.z(), &Corr::none(rp), 2.0 * PI * f)
    }

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol * b.abs()
    }

    #[test]
    fn math_helpers() {
        for k in -40..40 {
            let a = k as f32 * 0.37;
            let (s, c) = sin_cos(a);
            assert!((s - (a as f64).sin() as f32).abs() < 2e-6 && (c - (a as f64).cos() as f32).abs() < 2e-6, "{a}");
            for m in [0.3f32, -2.0, 7.5] {
                let (y, x) = (a, m);
                assert!((atan2(y, x) - (y as f64).atan2(x as f64) as f32).abs() < 3e-5, "{y} {x}");
            }
        }
        assert!((sqrtf(2.0) - 1.414_213_5).abs() < 1e-6);
        assert!((Cx::new(3.0, 4.0).abs() - 5.0).abs() < 1e-5);
    }

    #[test]
    fn resistor() {
        for (r, k) in [(100.0, 2), (1000.0, 1), (100e3, 0)] {
            for f in [100.0, 1000.0] {
                let z = measure(Cx::new(r, 0.0), f, k, 0);
                let s = solve(z, 2.0 * PI * f, Model::Auto);
                assert!(close(s.z, r, 0.01) && s.theta.abs() < 0.2 && s.kind == Kind::R, "{r} {f}: {s:?}");
            }
        }
    }

    #[test]
    fn inductor() {
        // 10 mH at 1 kHz = 62.8 ohm; 100 mH at 100 Hz.
        for (l, f, k) in [(10e-3, 1000.0, 2), (100e-3, 100.0, 2), (1.0, 1000.0, 1)] {
            let w = 2.0 * PI * f;
            let z = measure(Cx::new(0.0, w * l), f, k, 0);
            let s = solve(z, w, Model::Series);
            assert!(s.kind == Kind::L && close(s.l, l, 0.01), "{l} {f}: {s:?}");
            assert!((s.theta - 90.0).abs() < 0.3, "{s:?}");
        }
    }

    #[test]
    fn capacitor() {
        for (cap, f, k) in [(1e-6, 1000.0, 2), (100e-9, 1000.0, 1), (10e-9, 100.0, 0), (100e-6, 100.0, 2)] {
            let w = 2.0 * PI * f;
            let z = measure_dc(Cx::new(0.0, -1.0 / (w * cap)), f32::INFINITY, f, k, 0);
            let s = solve(z, w, Model::Series);
            assert!(s.kind == Kind::C && close(s.c, cap, 0.01), "{cap} {f}: {s:?}");
            assert!((s.theta + 90.0).abs() < 0.3, "{s:?}");
        }
    }

    #[test]
    fn r_plus_l_and_r_plus_c() {
        // 10 mH with 20 ohm (Q 3.14 at 1 kHz).
        let w = W1K;
        let z = measure(Cx::new(20.0, w * 10e-3), 1000.0, 2, 0);
        let s = solve(z, w, Model::Series);
        assert!(s.kind == Kind::L && close(s.l, 10e-3, 0.01) && close(s.r, 20.0, 0.01) && close(s.q, w * 10e-3 / 20.0, 0.01), "{s:?}");
        // 10 uF with 2 ohm ESR at 100 Hz (D 0.0126), and at 1 kHz (0.126).
        for f in [100.0, 1000.0] {
            let w = 2.0 * PI * f;
            let z = measure_dc(Cx::new(2.0, -1.0 / (w * 10e-6)), f32::INFINITY, f, 2, 0);
            let s = solve(z, w, Model::Series);
            assert!(s.kind == Kind::C && close(s.c, 10e-6, 0.01) && close(s.r, 2.0, 0.01), "{f}: {s:?}");
            assert!(close(s.d, 2.0 * w * 10e-6, 0.01), "{f}: {s:?}");
        }
        // Parallel: 100 nF || 100 kOhm at 1 kHz.
        let y = Cx::new(1e-5, W1K * 100e-9);
        let z = measure_dc(y.inv(), 100e3, 1000.0, 1, 0);
        let s = solve(z, W1K, Model::Parallel);
        assert!(s.par && close(s.c, 100e-9, 0.01) && close(s.r, 100e3, 0.01), "{s:?}");
    }

    #[test]
    fn noise_averages_out() {
        // 50 raw codes of noise on both channels (the U floor of doc 3.5).
        let w = W1K;
        let z = measure(Cx::new(5.0, w * 1e-3), 1000.0, 2, 87);
        let s = solve(z, w, Model::Series);
        assert!(close(s.l, 1e-3, 0.01), "{s:?}");
    }

    #[test]
    fn corrections() {
        // A 2 deg delay of I (tau at 1 kHz) and a 0.8 ohm path: the short
        // and the resistor fit it back.
        let w = W1K;
        let tau = 2.0 / 360.0 / 1000.0;
        let lag = Cx::cis(w * tau);
        let zpath = Cx::new(0.8, 0.0);
        let raw = |zd: Cx| zd.add(zpath).mul(lag);
        let mut k = Corr::none(0.5);
        k.short = raw(Cx::new(0.0, 0.0));
        let t = tau_from_resistor(raw(Cx::new(100.0, 0.0)), &k, w).unwrap();
        assert!((t - tau).abs() < 1e-8, "{t}");
        k.tau = t;
        let zd = Cx::new(3.0, w * 2e-3);
        let z = correct(raw(zd), &k, w);
        assert!(z.sub(zd).abs() < 1e-3 * zd.abs(), "{z:?}");
        // Open: 10 pF of lead capacitance across 1 MOhm.
        let yo = Cx::new(0.0, w * 10e-12);
        k.open = Cx::new(0.0, 0.0);
        let zm = Cx::new(1e6, 0.0).inv().add(yo).inv();
        k.open = correct(raw(Cx::new(1e12, 0.0).inv().add(yo).inv()), &k, w).inv();
        let z = correct(raw(zm), &k, w);
        assert!(close(z.re, 1e6, 0.01) && z.im.abs() < 0.01e6, "{z:?}");
        // A capacitor is not a resistor.
        assert!(tau_from_resistor(raw(Cx::new(1.0, -100.0)), &k, w).is_none());
    }

    #[test]
    fn ranges() {
        assert_eq!(next_range(2, 200_000, false), 2);
        assert_eq!(next_range(1, 130_000, false), 2);
        assert_eq!(next_range(0, 1000, true), 1);
        assert_eq!(next_range(2, 5000, false), 1);
        assert_eq!(next_range(0, 5000, false), 0);
        assert_eq!(next_range(1, 50_000, false), 1);
    }

    #[test]
    fn short_frame_none() {
        let s = sc(2);
        let (b, mut c) = frame(100.0, Cx::new(1.0, 0.0), Cx::new(0.01, 0.0), 0.0, 0.0, &s, 0);
        c.n = 60;
        assert!(fit(&b, &c, &s, 100.0).is_none());
    }
}
