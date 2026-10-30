//! Power Wave: the voltage and current waveforms of the power meter on one
//! page (docs/power-meter-and-misconnect.md 3.18). The pure part (scaling,
//! trigger, the readouts, the plot columns: host-tested by tools/meter-test,
//! `tests` below) and the capture (`capture`, the HY3131 bus); the app glue
//! is src/app_pwave.rs, the page src/ui_pwave.rs.
//!
//! The chip converts U and I together: in the power row AD2 (I: OP1 on the
//! shunt) and AD3 (U: OP2 on the divider tap) share the clock and the OSR
//! and are phase-trimmed by SPHACAL (R36). It has ONE data register for
//! both, R03..R05, and R34's SDO23 picks which converter it shows. So a
//! frame reads the two converters alternately: a sample of one, R34 flipped,
//! a sample of the other, ... each stamped with the MCU cycle counter.
//! The plot puts every sample at its own time; P pairs each sample with the
//! other channel interpolated at that time.
//!
//! A read is made safe against two unknowns of the chip (INFERRED: the
//! datasheet does not say):
//!   * whether R03..R05 holds the selected converter's last result (a live
//!     mux) or latches the source at the conversion: after an R34 write the
//!     next read waits one conversion period (`GUARD_US`), right for both;
//!   * whether a 3-byte read across a conversion edge tears: a value is
//!     taken only when two reads in a row agree (`read_stable`).
//! The 3-byte read takes ~13 us on the main loop's bus (/16): the power
//! row's own 76.8 kS/s (13 us per conversion) would tear nearly every read,
//! so a frame runs the converters at 19.2 kS/s (R26 OSR 128, `R26_RUNG`).
//! One read every three conversions (156.25 us), U and I in turn: 3200
//! samples per second and channel, 64 per 50 Hz cycle.
//!
//! The frame blocks the main loop (at most the trigger wait + 200 ms); the
//! power meter's readings, its alerts and the energy pause while the page
//! is open (main.rs `busy`); leaving it restores the power row
//! (Dmm::start -> pw_restart).

#![allow(dead_code)]

use crate::hy3131 as hy;
use crate::inrush::sqrtf;

/// Timebases (ms/div), 10 divisions across.
pub const TB_MS: [u16; 4] = [2, 5, 10, 20];
pub const TB_DEFAULT: u8 = 1;
pub const DIVS_X: u32 = 10;
/// Plot size (px): 10 x 40 across, 8 x 22 down.
pub const PW: usize = 400;
pub const PH: usize = 176;
pub const DIV_PX_X: usize = PW / DIVS_X as usize;
pub const DIV_PX_Y: f32 = 22.0;
/// Samples per frame (both channels), the times after them, the plot
/// columns after those (all in the scope's lent buffer).
pub const NMAX: usize = 2048;
pub const T_AT: usize = NMAX;
pub const COL_AT: usize = 2 * NMAX;
pub const BUF_MIN: usize = COL_AT + 2 * PW;
/// Sample time unit (us): u16 times cover 262 ms.
pub const T_UNIT_US: u32 = 4;
/// R26 low bits of a frame: /2, OSR 128 = 19.2 kS/s (hy3131 ladder).
pub const R26_RUNG: u8 = 2;
/// One conversion at that rung (52.08 us) plus a margin.
pub const GUARD_US: u32 = 55;
/// Read spacing (MCU cycles): three conversions, 156.25 us (256 x 3 chip
/// clocks; 3 200 S/s per channel). Room for a read with retries, the R34
/// write and the guard; a whole number of conversions so every read sits
/// at the same phase of the conversion grid.
pub const SLOT_CYC: u32 = (3 * 256 * crate::clock::HCLK_HZ as u64 / hy::SYSCLK_HZ as u64) as u32;
const _: () = assert!(SLOT_CYC == 31_250);
/// Settle after the frame's row write (the decimators refill).
pub const SETTLE_US: u32 = 1000;
/// Trigger wait (ms) before a frame runs untriggered.
pub const TRIG_WAIT_MS: u32 = 60;
/// |code| above this = the converter's rail (as the Wave view's CLIP).
pub const CLIP_RAW: i32 = 257_000;
/// Reads of one sample before it counts as unreadable (`read_stable`).
const READS_MAX: u32 = 5;

const CYC_US: u32 = crate::clock::HCLK_HZ / 1_000_000;

/// Volts / amperes of a stored code `c` (raw >> 3): (c - o) * s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    pub su: f32,
    pub ou: f32,
    pub si: f32,
    pub oi: f32,
}

impl Scale {
    pub fn u(&self, c: i16) -> f32 {
        (c as f32 - self.ou) * self.su
    }
    pub fn i(&self, c: i16) -> f32 {
        (c as f32 - self.oi) * self.si
    }
    /// The value of sample `k` of a frame (`y`: it is a U sample).
    pub fn val(&self, y: bool, c: i16) -> f32 {
        if y { self.u(c) } else { self.i(c) }
    }
}

/// A stored code (as inrush::code_of: 19 bit -> 16 bit, 8 raw codes each).
pub fn code_of(raw: i32) -> i16 {
    ((raw + 4) >> 3).clamp(-32768, 32767) as i16
}

/// What a frame holds: `n` samples, alternating; sample 0 is U when
/// `y_first`. Codes at buf[0..n], times (T_UNIT_US from sample 0) at
/// buf[T_AT..T_AT + n].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cap {
    pub n: usize,
    pub y_first: bool,
    /// The frame started on the trigger (else it ran free).
    pub trig: bool,
    /// Reads repeated because two in a row differed (a conversion edge
    /// inside, or a torn read), and samples that never agreed (kept as
    /// the last read) or had a broken sign copy.
    pub retries: u32,
    pub bad: u32,
    pub clip_u: bool,
    pub clip_i: bool,
}

impl Cap {
    /// Sample `k` is a U sample.
    pub fn is_u(&self, k: usize) -> bool {
        (k % 2 == 0) == self.y_first
    }
}

/// Trigger of the next frame: on U (`y`) or I, rising through `level`
/// (raw code) after being `hyst` below it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trig {
    pub y: bool,
    pub level: i32,
    pub hyst: i32,
}

/// The trigger for the next frame from the last one: U when its peak-peak
/// is at least `min_pp_u` (stored codes), else I when its is at least
/// `min_pp_i`, else none (free run). Level = the middle, hysteresis 1/8
/// of the peak-peak.
#[inline(never)]
pub fn next_trig(buf: &[u16], c: &Cap, min_pp_u: i32, min_pp_i: i32) -> Option<Trig> {
    let (mut lo, mut hi) = ([i32::MAX; 2], [i32::MIN; 2]);
    for k in 0..c.n {
        let ch = c.is_u(k) as usize;
        let v = buf[k] as i16 as i32;
        lo[ch] = lo[ch].min(v);
        hi[ch] = hi[ch].max(v);
    }
    for (ch, min) in [(1usize, min_pp_u), (0, min_pp_i)] {
        if hi[ch] > lo[ch] && hi[ch] - lo[ch] >= min {
            let pp = (hi[ch] - lo[ch]) * 8;
            return Some(Trig { y: ch == 1, level: (hi[ch] + lo[ch]) * 4, hyst: (pp / 8).max(16) });
        }
    }
    None
}

/// The rising-edge detector (`Trig`).
pub struct Edge {
    t: Trig,
    armed: bool,
}

impl Edge {
    pub fn new(t: Trig) -> Edge {
        Edge { t, armed: false }
    }
    /// One sample (raw code): true on the crossing.
    pub fn step(&mut self, v: i32) -> bool {
        if v < self.t.level - self.t.hyst {
            self.armed = true;
            false
        } else {
            self.armed && v >= self.t.level
        }
    }
}

/// The readouts of a frame: RMS of U and I and the mean of u x i over the
/// whole cycles of the trigger channel (all of the frame without two
/// crossings), the frequency from those cycles (NaN without), the peaks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub urms: f32,
    pub irms: f32,
    pub p: f32,
    pub f: f32,
    pub upk: f32,
    pub ipk: f32,
}

impl Stats {
    pub const NONE: Stats = Stats { urms: f32::NAN, irms: f32::NAN, p: f32::NAN, f: f32::NAN, upk: 0.0, ipk: 0.0 };
}

fn t_us(buf: &[u16], k: usize) -> f32 {
    (buf[T_AT + k] as u32 * T_UNIT_US) as f32
}

/// `Stats` of frame `c` (`y`: the channel the cycles are counted on; no
/// cycles when its peak-peak is below `min_pp`, volts / amperes: noise on
/// a DC level is not a frequency).
#[inline(never)]
pub fn stats(buf: &[u16], c: &Cap, sc: &Scale, y: bool, min_pp: f32) -> Stats {
    let n = c.n;
    if n < 4 {
        return Stats::NONE;
    }
    // Level and hysteresis of the cycle channel.
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    let (mut upk, mut ipk) = (0.0f32, 0.0f32);
    for k in 0..n {
        let u = c.is_u(k);
        let v = sc.val(u, buf[k] as i16);
        if u == y {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        if u {
            upk = upk.max(v.abs());
        } else {
            ipk = ipk.max(v.abs());
        }
    }
    let (lvl, h) = ((lo + hi) / 2.0, (hi - lo) / 8.0);
    // Rising crossings (the time interpolated between the two samples).
    let (mut first, mut last, mut cycles) = (f32::NAN, f32::NAN, 0u32);
    let mut armed = false;
    let mut prev: Option<(f32, f32)> = None;
    for k in 0..n {
        if c.is_u(k) != y {
            continue;
        }
        let (t, v) = (t_us(buf, k), sc.val(y, buf[k] as i16));
        if v < lvl - h {
            armed = true;
        } else if armed && v >= lvl {
            armed = false;
            let tc = match prev {
                Some((tp, vp)) if v > vp => tp + (t - tp) * (lvl - vp) / (v - vp),
                _ => t,
            };
            if first.is_nan() {
                first = tc;
            } else {
                cycles += 1;
            }
            last = tc;
        }
        prev = Some((t, v));
    }
    let whole = cycles >= 1 && hi - lo >= min_pp && hi - lo > 0.0;
    let (ta, tb) = if whole { (first, last) } else { (f32::MIN, f32::MAX) };
    let (mut su, mut nu, mut si, mut ni, mut sp, mut np) = (0.0f32, 0u32, 0.0f32, 0u32, 0.0f32, 0u32);
    for k in 0..n {
        let t = t_us(buf, k);
        if t < ta || t >= tb {
            continue;
        }
        let u = c.is_u(k);
        let v = sc.val(u, buf[k] as i16);
        if u {
            su += v * v;
            nu += 1;
        } else {
            si += v * v;
            ni += 1;
        }
        // The other channel at t: its neighbours k - 1 and k + 1.
        if k >= 1 && k + 1 < n {
            let (t0, t1) = (t_us(buf, k - 1), t_us(buf, k + 1));
            let (a, b) = (sc.val(!u, buf[k - 1] as i16), sc.val(!u, buf[k + 1] as i16));
            let o = if t1 > t0 { a + (b - a) * (t - t0) / (t1 - t0) } else { a };
            sp += v * o;
            np += 1;
        }
    }
    let rms = |s: f32, n: u32| if n > 0 { sqrtf(s / n as f32) } else { f32::NAN };
    Stats {
        urms: rms(su, nu),
        irms: rms(si, ni),
        p: if np > 0 { sp / np as f32 } else { f32::NAN },
        f: if whole && last > first { cycles as f32 * 1e6 / (last - first) } else { f32::NAN },
        upk,
        ipk,
    }
}

/// Largest I zero correction `anchor` takes, raw codes: ~2 x the AD2 X
/// zeros seen on the device (155 codes with OP1 x1, ~880 with x10, doc
/// 3.11 / 3.12); a bigger difference is not a zero (a gain error, a
/// current that changed): no correction.
pub const ANCHOR_MAX_RAW: f32 = 2000.0;

/// The I zero from a reference current `i_ref` (A, the power meter's DC
/// coupling: AD1 on the same OP1 output, calibrated): the stored-code
/// shift that makes frame `c`'s mean I equal it, None when the frame has
/// fewer than 8 I samples or the shift exceeds ANCHOR_MAX_RAW. Add it to
/// `Scale::oi`.
pub fn anchor(buf: &[u16], c: &Cap, sc: &Scale, i_ref: f32) -> Option<f32> {
    let (mut s, mut n) = (0.0f32, 0u32);
    for k in 0..c.n {
        if !c.is_u(k) {
            s += buf[k] as i16 as f32;
            n += 1;
        }
    }
    if n < 8 || !(sc.si > 0.0) || !i_ref.is_finite() {
        return None;
    }
    let d = s / n as f32 - (i_ref / sc.si + sc.oi);
    (d.abs() * 8.0 <= ANCHOR_MAX_RAW).then_some(d)
}

/// Gain anchor (`POW:ZERO`'s X zero stored, so AD2's zero is known): the
/// factor on `Scale::si` that makes frame `c`'s mean I equal `i_ref`;
/// None below ANCHOR_GAIN_MIN_RAW of mean (a small current says nothing
/// of the gain), or outside 0.9 .. 1.1.
pub const ANCHOR_GAIN_MIN_RAW: f32 = 2000.0;

pub fn anchor_gain(buf: &[u16], c: &Cap, sc: &Scale, i_ref: f32) -> Option<f32> {
    let (mut s, mut n) = (0.0f32, 0u32);
    for k in 0..c.n {
        if !c.is_u(k) {
            s += buf[k] as i16 as f32;
            n += 1;
        }
    }
    let m = if n >= 8 { s / n as f32 - sc.oi } else { 0.0 };
    if (m * 8.0).abs() < ANCHOR_GAIN_MIN_RAW || !(sc.si > 0.0) || !i_ref.is_finite() {
        return None;
    }
    let g = i_ref / (m * sc.si);
    (0.9..=1.1).contains(&g).then_some(g)
}

/// 1-2-5 steps (per division) from 1 uV / 1 uA up.
pub fn step(i: i32) -> f32 {
    let m = [1.0f32, 2.0, 5.0][i.rem_euclid(3) as usize];
    let mut p = 1e-6f32;
    for _ in 0..i.div_euclid(3) {
        p *= 10.0;
    }
    m * p
}

/// The step index of the smallest step >= `x`.
fn step_at_least(x: f32) -> i32 {
    let mut i = 0;
    while i < 40 && step(i) < x * 0.9999 {
        i += 1;
    }
    i
}

/// Per-division step for peak `pk`: kept while the peak stays within 1 ..
/// 3.9 divisions of it (no jumping between frames), else the smallest
/// step that shows the peak within 3.5 divisions, never below `floor`.
pub fn div_for(pk: f32, cur: f32, floor: f32) -> f32 {
    let f = step(step_at_least(floor));
    if cur > 0.0 && pk <= 3.9 * cur && (pk >= cur || cur <= f) {
        return cur.max(f);
    }
    step(step_at_least(pk / 3.5)).max(f)
}

/// The floor of the per-division step for a range of full scale `fs`:
/// fs / 100 on the 1-2-5 grid (6 V -> 0.1 V/div, 10 A -> 0.1 A/div).
pub fn floor_for(fs: f32) -> f32 {
    step(step_at_least(fs / 100.0))
}

/// The plot columns of frame `c`: per channel (I at cols[0..PW], U at
/// cols[PW..2 PW]) and column, the rows its line passes (lo | hi << 8,
/// 0xFFFF = none), `du` / `di` per division, `span_us` across the plot.
#[inline(never)]
pub fn columns(buf: &mut [u16], c: &Cap, sc: &Scale, du: f32, di: f32, span_us: u32) {
    let (data, cols) = buf.split_at_mut(COL_AT);
    let cols = &mut cols[..2 * PW];
    cols.fill(0xFFFF);
    let mid = (PH / 2) as f32;
    for ch in 0..2usize {
        let y = ch == 1;
        let div = if y { du } else { di };
        let col = &mut cols[ch * PW..(ch + 1) * PW];
        let mut prev: Option<(f32, f32)> = None;
        for k in 0..c.n {
            if c.is_u(k) != y {
                continue;
            }
            let x = (data[T_AT + k] as u32 * T_UNIT_US) as f32 * PW as f32 / span_us as f32;
            let v = sc.val(y, data[k] as i16);
            let r = (mid - v / div * DIV_PX_Y).clamp(0.0, (PH - 1) as f32);
            let (xa, ya) = prev.unwrap_or((x, r));
            prev = Some((x, r));
            if xa >= PW as f32 {
                break;
            }
            // Columns xa .. x: the segment's rows within each.
            let (c0, c1) = (xa.max(0.0) as usize, (x.min(PW as f32 - 0.001)).max(0.0) as usize);
            for cx in c0..=c1.min(PW - 1) {
                let (l, rr) = ((cx as f32).max(xa), ((cx + 1) as f32).min(x));
                let at = |t: f32| if x > xa { ya + (r - ya) * (t - xa) / (x - xa) } else { r };
                let (p, q) = (at(l), at(rr.max(l)));
                let (a, b) = ((p.min(q) + 0.5) as u16, (p.max(q) + 0.5) as u16);
                let e = col[cx];
                col[cx] = if e == 0xFFFF { a | b << 8 } else { (a.min(e & 0xFF)) | (b.max(e >> 8)) << 8 };
            }
        }
    }
}

// ------------------------------------------------------------- the capture

fn cyc() -> u32 {
    // SAFETY: the DWT cycle counter (clock.rs starts it), read only.
    unsafe { core::ptr::read_volatile(0xE000_1004 as *const u32) }
}

fn wait_until(t: u32) {
    while (cyc().wrapping_sub(t) as i32) < 0 {}
}

/// One value of R03..R05: two reads in a row that agree (`Cap::retries`
/// counts the extra reads); after READS_MAX reads, or with a broken sign
/// copy, the last read and `ok` false.
fn read_stable(retries: &mut u32) -> (i32, bool) {
    let mut b = [0u8; 3];
    hy::read(hy::R_AD2, &mut b);
    let mut prev = hy::s24(&b);
    for _ in 1..READS_MAX {
        hy::read(hy::R_AD2, &mut b);
        let v = hy::s24(&b);
        if v == prev {
            return (v, hy::ad2_sign_ok(v));
        }
        *retries += 1;
        prev = v;
    }
    (prev, false)
}

/// The frame's row: the power row (`row`, R34 = XY: SDO23 0, the data
/// register on AD2 = I) at R26_RUNG, written as the power meter writes it
/// (`configure_ph`: SPHACAL `r36`, AD3 then AD2), then a settle.
pub fn arm(row: &[u8; hy::CFG_LEN], r36: u8) {
    let mut r = *row;
    r[6] = (r[6] & 0x80) | R26_RUNG;
    hy::configure_ph(&r, r36);
    crate::clock::delay_us(SETTLE_US);
}

/// R34 with SDO23 = `y` (AD3 = U in the data register), the rest of the
/// row's R34 (`r34`) kept: ENAD3 never changes.
fn sel(r34: u8, y: bool) {
    hy::write(0x34, &[if y { r34 | 0x04 } else { r34 & !0x04 }]);
}

/// One frame into `buf` (`arm` first): wait up to TRIG_WAIT_MS for `trig`,
/// then sample U and I alternately for `span_us` (one every SLOT_CYC, at
/// most NMAX). `r34`: the row's R34.
#[inline(never)]
pub fn capture(buf: &mut [u16], span_us: u32, trig: Option<Trig>, r34: u8) -> Cap {
    let mut c = Cap::default();
    let mut y = trig.is_some_and(|t| t.y);
    sel(r34, y);
    let guard = GUARD_US * CYC_US;
    let mut t_w = cyc();
    wait_until(t_w.wrapping_add(guard));
    let mut first: Option<i32> = None;
    if let Some(t) = trig {
        let mut e = Edge::new(t);
        let t0 = cyc();
        while cyc().wrapping_sub(t0) < TRIG_WAIT_MS * 1000 * CYC_US {
            let (v, ok) = read_stable(&mut c.retries);
            if ok && e.step(v) {
                c.trig = true;
                first = Some(v);
                break;
            }
        }
    }
    c.y_first = y;
    // One read every SLOT_CYC from t0 (three conversions: the read lands at
    // the same place between two conversion edges every time, so U and I
    // lag their conversions alike; a read held up runs late, the next ones
    // keep the schedule).
    let t0 = cyc();
    let mut k = 0;
    while k < NMAX {
        let t_rd = cyc();
        let dt = t_rd.wrapping_sub(t0) / (T_UNIT_US * CYC_US);
        if dt * T_UNIT_US > span_us {
            break;
        }
        let (v, ok) = match first.take() {
            Some(v) => (v, true),
            None => read_stable(&mut c.retries),
        };
        if !ok {
            c.bad += 1;
        }
        if v.abs() > CLIP_RAW {
            if y {
                c.clip_u = true;
            } else {
                c.clip_i = true;
            }
        }
        buf[k] = code_of(v) as u16;
        buf[T_AT + k] = dt as u16;
        k += 1;
        y = !y;
        sel(r34, y);
        t_w = cyc();
        wait_until(t_w.wrapping_add(guard).max_wrapping(t0.wrapping_add(k as u32 * SLOT_CYC)));
    }
    c.n = k;
    c
}

/// The later of two cycle-counter times (wrapping).
trait MaxWrap {
    fn max_wrapping(self, o: u32) -> u32;
}

impl MaxWrap for u32 {
    fn max_wrapping(self, o: u32) -> u32 {
        if (o.wrapping_sub(self) as i32) > 0 { o } else { self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame of U = a_u sin + dc_u, I = a_i sin(.. - ph), alternating
    /// every `dt_us`, starting with U at t = 0 (codes = value / s).
    fn frame(buf: &mut Vec<u16>, n: usize, dt_us: u32, f: f32, a_u: f32, a_i: f32, ph: f32, sc: &Scale) -> Cap {
        buf.clear();
        buf.resize(BUF_MIN, 0);
        for k in 0..n {
            let t = (k as u32 * dt_us) as f32 * 1e-6;
            let w = 2.0 * core::f32::consts::PI * f * t;
            let (y, v) = if k % 2 == 0 { (true, a_u * w.sin()) } else { (false, a_i * (w - ph).sin()) };
            let c = if y { v / sc.su + sc.ou } else { v / sc.si + sc.oi };
            buf[k] = (c.round() as i32 as i16) as u16;
            buf[T_AT + k] = (k as u32 * dt_us / T_UNIT_US) as u16;
        }
        Cap { n, y_first: true, ..Cap::default() }
    }

    const SC: Scale = Scale { su: 0.01, ou: 5.0, si: 0.0001, oi: -3.0 };

    #[test]
    fn mains_stats() {
        // 230 V rms, 1 A rms, 60 deg: P = 115 W; 100 ms at 100 us per sample.
        let mut b = Vec::new();
        let c = frame(&mut b, 1000, 100, 50.0, 325.27, 1.4142, core::f32::consts::PI / 3.0, &SC);
        let s = stats(&b, &c, &SC, true, 1.0);
        assert!((s.urms - 230.0).abs() < 0.3, "{s:?}");
        assert!((s.irms - 1.0).abs() < 0.003, "{s:?}");
        assert!((s.p - 115.0).abs() < 1.0, "{s:?}");
        assert!((s.f - 50.0).abs() < 0.05, "{s:?}");
        assert!((s.upk - 325.27).abs() < 0.5);
    }

    #[test]
    fn dc_without_cycles() {
        let mut b = Vec::new();
        let mut c = frame(&mut b, 200, 100, 0.0, 0.0, 0.0, 0.0, &SC);
        // DC 12 V, 2 A.
        for k in 0..c.n {
            b[k] = if k % 2 == 0 { (1200 + 5) as u16 } else { (20000 - 3) as u16 };
        }
        c.y_first = true;
        let s = stats(&b, &c, &SC, true, 1.0);
        assert!((s.urms - 12.0).abs() < 1e-3 && (s.irms - 2.0).abs() < 1e-3 && (s.p - 24.0).abs() < 0.01, "{s:?}");
        assert!(s.f.is_nan());
        assert!(next_trig(&b, &c, 10, 10).is_none());
        // Noise of a few codes on the DC: no frequency.
        for k in 0..c.n {
            b[k] = b[k].wrapping_add([0u16, 3, 0xFFFE, 1][k % 4]);
        }
        let s = stats(&b, &c, &SC, true, 1.0);
        assert!(s.f.is_nan() && (s.urms - 12.0).abs() < 0.05, "{s:?}");
    }

    #[test]
    fn trigger_from_last_frame() {
        let mut b = Vec::new();
        let c = frame(&mut b, 1000, 100, 50.0, 325.0, 1.0, 0.0, &SC);
        let t = next_trig(&b, &c, 100, 100).unwrap();
        assert!(t.y);
        // Level ~ U = 0 V: code ou = 5 (x 8 raw).
        assert!((t.level - 40).abs() <= 16, "{t:?}");
        let mut e = Edge::new(t);
        assert!(!e.step(t.level + 100)); // not armed yet
        assert!(!e.step(t.level - t.hyst - 1));
        assert!(e.step(t.level));
        // U flat: the trigger goes to I.
        let c2 = frame(&mut b, 1000, 100, 50.0, 0.0, 1.0, 0.0, &SC);
        assert!(!next_trig(&b, &c2, 100, 100).unwrap().y);
    }

    #[test]
    fn anchor_to_the_dc_meter() {
        // DC 20 mA read 4 % high by an AD2 zero of 888 raw codes (OP1 x10,
        // the 60 mA range: 0.954 uA per raw code): the anchor to the DC
        // meter's 20.000 mA takes the zero out.
        let sc = Scale { su: 8.0 * 4.768e-5, ou: 0.0, si: 8.0 * 1.25 / 5.0 / 262_144.0, oi: 0.0 };
        let mut b = vec![0u16; BUF_MIN];
        let n = 200;
        for k in 0..n {
            let raw = if k % 2 == 0 { 5.0 / (sc.su / 8.0) } else { 0.020 / (sc.si / 8.0) + 888.0 };
            b[k] = code_of(raw as i32) as u16;
            b[T_AT + k] = (k * 40) as u16;
        }
        let c = Cap { n, y_first: true, ..Cap::default() };
        let s0 = stats(&b, &c, &sc, true, 1.0);
        assert!((s0.irms / 0.020 - 1.042).abs() < 0.003, "{s0:?}");
        let d = anchor(&b, &c, &sc, 0.020).unwrap();
        assert!((d - 111.0).abs() < 1.0, "{d}");
        let sc2 = Scale { oi: sc.oi + d, ..sc };
        let s1 = stats(&b, &c, &sc2, true, 1.0);
        assert!((s1.irms - 0.020).abs() < 2e-6 && (s1.p - 0.100).abs() < 1e-4, "{s1:?}");
        // A difference too large to be a zero: none.
        assert!(anchor(&b, &c, &sc, 0.010).is_none());
        // With the zero known (POW:ZERO: oi = 888 / 8) a 2 % gain error
        // is fitted as a gain, not as a zero.
        let scz = Scale { oi: 111.0, ..sc };
        let g = anchor_gain(&b, &c, &scz, 0.020 / 1.02).unwrap();
        assert!((g - 1.0 / 1.02).abs() < 1e-3, "{g}");
        let s2 = stats(&b, &c, &Scale { si: scz.si * g, ..scz }, true, 1.0);
        assert!((s2.irms - 0.020 / 1.02).abs() < 3e-6, "{s2:?}");
        // Too little current to say anything of the gain: none.
        assert!(anchor_gain(&b, &c, &Scale { oi: 2000.0, ..sc }, 0.001).is_none());
    }

    #[test]
    fn steps() {
        assert_eq!(step(0), 1e-6);
        assert!((step(24) - 100.0).abs() < 1e-3 && (step(20) - 5.0).abs() < 1e-5);
        assert!((div_for(325.0, 0.0, 0.1) - 100.0).abs() < 1e-3);
        // Kept while within 1 .. 3.9 divisions.
        assert!((div_for(200.0, 100.0, 0.1) - 100.0).abs() < 1e-3);
        assert!((div_for(50.0, 100.0, 0.1) - 20.0).abs() < 1e-3);
        // The floor holds a quiet channel.
        assert!((div_for(0.0001, 0.0, 0.1) - 0.1).abs() < 1e-6);
        assert!((floor_for(6.0) - 0.1).abs() < 1e-6);
        assert!((floor_for(10.0) - 0.1).abs() < 1e-6);
        assert!((floor_for(600e-6) - 1e-5).abs() < 1e-9);
    }

    #[test]
    fn columns_follow_the_trace() {
        let mut b = Vec::new();
        let c = frame(&mut b, 1000, 100, 50.0, 325.0, 1.0, 0.0, &SC);
        columns(&mut b, &c, &SC, 100.0, 0.5, 100_000);
        let cols = &b[COL_AT..COL_AT + 2 * PW];
        // Every column of both channels is drawn (100 ms of data, 100 ms span).
        assert!(cols.iter().all(|&v| v != 0xFFFF));
        // U at t = 5 ms (column 20): the positive peak, 3.25 div above the middle.
        let u = cols[PW + 20];
        let top = (PH / 2) as f32 - 3.25 * DIV_PX_Y;
        assert!(((u & 0xFF) as f32 - top).abs() <= 2.0, "{:x}", u);
        // I = 1 A peak at 0.5 A/div: 2 divisions.
        let i = cols[20];
        assert!(((i & 0xFF) as f32 - ((PH / 2) as f32 - 2.0 * DIV_PX_Y)).abs() <= 2.0, "{:x}", i);
    }
}
