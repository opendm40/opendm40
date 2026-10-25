//! Touch calibration (the touch test page's Calibrate, app_touchtest.rs;
//! DBG:TOUCH:CAL): a least-squares affine map from the controller's points
//! to the panel,
//!
//!   x' = a x + b y + c        y' = d x + e y + f
//!
//! fitted to taps on the page's nine targets, checked, and applied by
//! touch.rs `screen()` to every screen point (never to a touch-key report).
//!
//! Stored as six Q16 numbers that are the *difference from the identity*:
//! `[a - 1, b, c, d, e - 1, f]` x 65536. All zero is the identity, the
//! stock's behaviour (the controller's coordinates used unchanged): the
//! zeroed static in touch.rs, a settings record from before the calibration
//! (persist.rs v11: zero-extended) and a fresh unit all read as "none".
//!
//! Region C (`.cold`, link-native.x): the fit and the checks run only on the
//! touch test page, at a SCPI command and at boot. The per-frame apply is
//! touch.rs `screen()` (region B, integer).

/// Q16 deltas from the identity: a - 1, b, c, d, e - 1, f (x 65536).
pub type Cal = [i32; 6];
pub const IDENTITY: Cal = [0; 6];
const ONE: f32 = 65536.0;

/// Scale (a, e) within 1 +- this, cross terms (b, d) within +- this.
pub const MAX_SCALE_DEV: f32 = 0.15;
/// The correction at any target at most this, px.
pub const MAX_SHIFT_PX: i32 = 40;
/// The largest residual of an accepted fit, px.
pub const MAX_RESID_PX: i32 = 25;

/// Why a calibration is refused (the result screen's text).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bad {
    /// Too few or degenerate points (all on one line).
    Points,
    /// A scale outside 0.85..1.15 or a cross term over 0.15.
    Scale,
    /// A correction over MAX_SHIFT_PX at a target.
    Shift,
    /// A tap farther than MAX_RESID_PX from the fitted map.
    Resid,
}

impl Bad {
    pub fn text(self) -> &'static str {
        match self {
            Bad::Points => "Too few points",
            Bad::Scale => "Scale out of range",
            Bad::Shift => "Shift over 40 px",
            Bad::Resid => "Taps scatter over 25 px",
        }
    }
}

/// A fit and its residuals (tenths of a px): `mean` / `max` of the taps
/// against the fitted map, `before` the mean of the raw taps against the
/// targets (no calibration).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fit {
    pub cal: Cal,
    pub mean: u32,
    pub max: u32,
    pub before: u32,
    /// None: the fit passes `check` and its residual limit.
    pub bad: Option<Bad>,
}

/// The point (x, y) through `c`, in 1/16 px (no clamping).
pub fn apply16(c: &Cal, x: i32, y: i32) -> (i32, i32) {
    let fx = (x << 16) + c[0] * x + c[1] * y + c[2];
    let fy = (y << 16) + c[3] * x + c[4] * y + c[5];
    (fx >> 12, fy >> 12)
}

/// The scale / cross-term / shift limits (`targets`: where the shift is
/// measured; the touch test page's nine).
pub fn check(c: &Cal, targets: &[(u16, u16)]) -> Result<(), Bad> {
    let lim = (MAX_SCALE_DEV * ONE) as i32;
    if c.iter().enumerate().any(|(i, &v)| i % 3 != 2 && v.abs() > lim) {
        return Err(Bad::Scale);
    }
    for &(x, y) in targets {
        let (x, y) = (x as i32, y as i32);
        let (fx, fy) = apply16(c, x, y);
        if (fx - (x << 4)).abs() > MAX_SHIFT_PX << 4 || (fy - (y << 4)).abs() > MAX_SHIFT_PX << 4 {
            return Err(Bad::Shift);
        }
    }
    Ok(())
}

/// Integer square root.
fn isqrt(v: u64) -> u64 {
    if v < 2 {
        return v;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// Distance in tenths of a px of two points given in 1/16 px.
fn dist10(ax: i32, ay: i32, bx: i32, by: i32) -> u32 {
    let (dx, dy) = ((ax - bx) as i64, (ay - by) as i64);
    (isqrt((dx * dx + dy * dy) as u64) * 10 / 16) as u32
}

/// Mean / max distance (tenths of a px) of the points through `c` to the
/// targets.
pub fn resid(c: &Cal, raw: &[(i16, i16)], tgt: &[(i16, i16)]) -> (u32, u32) {
    let n = raw.len().min(tgt.len());
    if n == 0 {
        return (0, 0);
    }
    let (mut sum, mut max) = (0u32, 0u32);
    for i in 0..n {
        let (fx, fy) = apply16(c, raw[i].0 as i32, raw[i].1 as i32);
        let d = dist10(fx, fy, (tgt[i].0 as i32) << 4, (tgt[i].1 as i32) << 4);
        sum += d;
        max = max.max(d);
    }
    (sum / n as u32, max)
}

fn q16(v: f32) -> Option<i32> {
    let s = v * ONE;
    // NaN and anything beyond i32 fail.
    if !(s > -2.0e9 && s < 2.0e9) {
        return None;
    }
    Some(if s < 0.0 { (s - 0.5) as i32 } else { (s + 0.5) as i32 })
}

/// Least-squares affine fit raw -> target (f32, the coordinates centred
/// first), checked: `targets` as `check`'s. None: fewer than three points
/// or all of them on one line.
#[inline(never)]
pub fn fit(raw: &[(i16, i16)], tgt: &[(i16, i16)], targets: &[(u16, u16)]) -> Option<Fit> {
    let n = raw.len().min(tgt.len());
    if n < 3 {
        return None;
    }
    let nf = n as f32;
    let (mut mx, mut my, mut mu, mut mv) = (0f32, 0f32, 0f32, 0f32);
    for i in 0..n {
        mx += raw[i].0 as f32;
        my += raw[i].1 as f32;
        mu += tgt[i].0 as f32;
        mv += tgt[i].1 as f32;
    }
    mx /= nf;
    my /= nf;
    mu /= nf;
    mv /= nf;
    let (mut sxx, mut sxy, mut syy, mut sxu, mut syu, mut sxv, mut syv) = (0f32, 0f32, 0f32, 0f32, 0f32, 0f32, 0f32);
    for i in 0..n {
        let (x, y) = (raw[i].0 as f32 - mx, raw[i].1 as f32 - my);
        let (u, v) = (tgt[i].0 as f32 - mu, tgt[i].1 as f32 - mv);
        sxx += x * x;
        sxy += x * y;
        syy += y * y;
        sxu += x * u;
        syu += y * u;
        sxv += x * v;
        syv += y * v;
    }
    let det = sxx * syy - sxy * sxy;
    // Collinear (or coincident) points: no plane to fit.
    if !(det > 1e-3 * sxx * syy) || sxx < 1.0 || syy < 1.0 {
        return None;
    }
    let a = (sxu * syy - syu * sxy) / det;
    let b = (syu * sxx - sxu * sxy) / det;
    let d = (sxv * syy - syv * sxy) / det;
    let e = (syv * sxx - sxv * sxy) / det;
    let c = mu - a * mx - b * my;
    let f = mv - d * mx - e * my;
    let cal = [q16(a - 1.0)?, q16(b)?, q16(c)?, q16(d)?, q16(e - 1.0)?, q16(f)?];
    let (mean, max) = resid(&cal, &raw[..n], &tgt[..n]);
    let (before, _) = resid(&IDENTITY, &raw[..n], &tgt[..n]);
    let bad = match check(&cal, targets) {
        Err(b) => Some(b),
        Ok(()) if max > MAX_RESID_PX as u32 * 10 => Some(Bad::Resid),
        Ok(()) => None,
    };
    Some(Fit { cal, mean, max, before, bad })
}

/// The coefficients a..f as numbers (DBG:TOUCH:CAL?, the page's text).
pub fn to_f32(c: &Cal) -> [f32; 6] {
    let mut o = [0f32; 6];
    for i in 0..6 {
        o[i] = c[i] as f32 / ONE + if i % 4 == 0 { 1.0 } else { 0.0 };
    }
    o
}

/// a..f -> Cal (DBG:TOUCH:CAL a,b,c,d,e,f), None for a NaN / huge value.
pub fn from_f32(v: &[f32; 6]) -> Option<Cal> {
    let mut c = IDENTITY;
    for i in 0..6 {
        c[i] = q16(v[i] - if i % 4 == 0 { 1.0 } else { 0.0 })?;
    }
    Some(c)
}

/// DBG:TOUCH:CAL a,b,c,d,e,f: the values the parser read (scpi.rs
/// `tcal_arg`), until the command runs (`scpi_take`). Flag 1 = set. In
/// SRAM (`.sram_bss`, zeroed at start-up).
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut PENDING: (u8, Cal) = (0, [0; 6]);

/// Keep the parsed a..f for the command (false: NaN / huge).
pub fn scpi_put(v: &[f32; 6]) -> bool {
    let c = from_f32(v);
    // SAFETY: single-threaded, no ISR touches it.
    unsafe { *core::ptr::addr_of_mut!(PENDING) = (c.is_some() as u8, c.unwrap_or(IDENTITY)) };
    c.is_some()
}

/// The parsed values, once.
pub fn scpi_take() -> Option<Cal> {
    // SAFETY: as `scpi_put`.
    let (f, c) = unsafe { core::mem::replace(&mut *core::ptr::addr_of_mut!(PENDING), (0, IDENTITY)) };
    if f != 0 { Some(c) } else { None }
}
