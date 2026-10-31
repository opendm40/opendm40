//! DMM Math (MF page -> Math): dBm / dBV of a voltage reading (Brymen
//! BM869s / Siglent SDM3065X dBm with a reference impedance), m x X + b
//! scaling with a unit label of up to 4 bytes (Keysight 34465A "Mx-B").
//!
//! Applied where a reading is shown or sent, never upstream of the meter:
//! `App::shown_reading` (DMM page, the combined view's strip, DBG:READout?)
//! and SCPI READ? / FETCh? / MEASure? (main.rs). Statistics, limits,
//! AutoHold, the trend chart, the bargraph and the CSV recording keep the
//! raw reading (their thresholds, ranges and units are the function's).
//!
//! X is the reading in base units (V, A, ohm: `Reading::si`), so the
//! function's range and prefix (mV, uA, kohm) do not change the result.
//! `apply` is per reading (region B); the page, keypad and SCPI glue are
//! .cold (app_math.rs).

use crate::dmm::{Reading, Val};

pub const OFF: u8 = 0;
pub const DBM: u8 = 1;
pub const DBV: u8 = 2;
pub const SCAL: u8 = 3;
/// On-screen names of OFF..SCAL (the Math page, the MF page button).
pub const NAMES: [&str; 4] = ["Off", "dBm", "dBV", "mX+b"];
/// dBm reference impedances, ohm (CALC:DBM:REF).
pub const REFS: [u16; 3] = [50, 75, 600];
/// Unit labels the Math page steps through; SCPI CALC:SCAL:UNIT takes any
/// text of up to 4 bytes.
pub const UNITS: [&str; 16] = ["", "A", "mA", "V", "W", "\u{b0}C", "\u{b0}F", "%", "Pa", "bar", "psi", "rpm", "lx", "ppm", "N", "g"];

/// The settings in their persisted form (persist::W_MATH, 4 words): [0]
/// bits 0..1 the function, bits 4..5 the REFS index; [1] m (f32 bits, 0 =
/// 1.0, so a zeroed or older record means m = 1); [2] b (f32 bits); [3]
/// the unit's UTF-8 bytes, little-endian, 0-padded. In SRAM (.sram_bss,
/// zeroed at reset = the defaults), not in the App on the TCM stack.
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut W: [u32; 4] = [0; 4];
/// The unit text `unit` hands out (the bytes of W[3]). It changes only on
/// a settings change, from the main loop; the app then repaints the
/// DMM page (`App::math_changed`), whose unit caches compare by content.
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut UNIT: [u8; 4] = [0; 4];

#[inline(always)]
fn w(i: usize) -> u32 {
    unsafe { (*core::ptr::addr_of!(W))[i] }
}

#[inline(always)]
fn set_w(i: usize, v: u32) {
    unsafe { (*core::ptr::addr_of_mut!(W))[i] = v }
}

pub fn func() -> u8 {
    (w(0) & 3) as u8
}

pub fn ref_idx() -> usize {
    ((w(0) >> 4 & 3) as usize).min(REFS.len() - 1)
}

pub fn m() -> f32 {
    match w(1) {
        0 => 1.0,
        b => f32::from_bits(b),
    }
}

pub fn b() -> f32 {
    f32::from_bits(w(2))
}

pub fn unit() -> &'static str {
    let u = unsafe { &*core::ptr::addr_of!(UNIT) };
    let n = u.iter().position(|&c| c == 0).unwrap_or(4);
    core::str::from_utf8(&u[..n]).unwrap_or("")
}

pub fn set_func(f: u8) {
    set_w(0, w(0) & !3 | (f & 3) as u32);
}

pub fn set_ref_idx(i: usize) {
    set_w(0, w(0) & !0x30 | (i.min(REFS.len() - 1) as u32) << 4);
}

/// m: finite and not 0 (false: refused).
pub fn set_m(v: f32) -> bool {
    if !v.is_finite() || v == 0.0 {
        return false;
    }
    set_w(1, if v == 1.0 { 0 } else { v.to_bits() });
    true
}

pub fn set_b(v: f32) -> bool {
    if !v.is_finite() {
        return false;
    }
    set_w(2, (v + 0.0).to_bits());
    true
}

/// Unit label: UTF-8, up to 4 bytes, no control characters (false:
/// refused).
pub fn set_unit(t: &[u8]) -> bool {
    if t.len() > 4 || core::str::from_utf8(t).is_err() || t.iter().any(|&c| c < 0x20 || c == 0x7F) {
        return false;
    }
    let mut u = [0u8; 4];
    u[..t.len()].copy_from_slice(t);
    set_w(3, u32::from_le_bytes(u));
    unsafe { *core::ptr::addr_of_mut!(UNIT) = u };
    true
}

pub fn words() -> [u32; 4] {
    [w(0), w(1), w(2), w(3)]
}

/// Restore from the persisted words (an unknown field: its default).
pub fn set_words(p: &[u32]) {
    set_w(0, 0);
    set_func(p[0] as u8);
    let i = (p[0] >> 4 & 3) as usize;
    set_ref_idx(if i < REFS.len() { i } else { 0 });
    set_w(1, 0);
    set_m(f32::from_bits(p[1]));
    set_w(2, 0);
    set_b(f32::from_bits(p[2]));
    if !set_unit_word(p[3]) {
        set_unit(&[]);
    }
}

/// `set_unit` of the bytes of `w` (little-endian, up to the first 0).
pub fn set_unit_word(w: u32) -> bool {
    let u = w.to_le_bytes();
    set_unit(&u[..u.iter().position(|&c| c == 0).unwrap_or(4)])
}

/// Math function `f` applies to HY3131 mode `mode`: dBm / dBV in the V
/// functions (AC / DC V and mV, modes 0..3), mX+b also in A (4..9) and
/// ohm (10).
pub fn applies(f: u8, mode: u8) -> bool {
    match f {
        DBM | DBV => mode <= 3,
        SCAL => mode <= 10,
        _ => false,
    }
}

/// 10 x log10(x) = 10 / ln 10 x ln x (dmm.rs ln, no libm).
fn db10(x: f32) -> f32 {
    (10.0 / core::f32::consts::LN_10) * crate::dmm::ln(x)
}

/// dBV of `v` volts (20 log10 |v| / 1 V); NaN at 0 V.
pub fn dbv(v: f32) -> f32 {
    let a = v.abs();
    if a < 1e-15 { f32::NAN } else { 2.0 * db10(a) }
}

/// dBm of `v` volts across `r` ohm (10 log10 (v^2 / r / 1 mW)); NaN at 0 V.
pub fn dbm(v: f32, r: f32) -> f32 {
    let a = v.abs();
    if a < 1e-15 { f32::NAN } else { 2.0 * db10(a) - db10(r) + 30.0 }
}

/// Decimals of an mX+b result on the 7-cell readout (sign, digits, point:
/// ui.rs fmt7 keeps formats %7.4f .. %7.1f), None above +-9999.9.
pub fn dec_for(y: f32) -> Option<u8> {
    let a = y.abs();
    Some(if a < 9.999_95 {
        4
    } else if a < 99.999_5 {
        3
    } else if a < 999.995 {
        2
    } else if a < 9_999.95 {
        1
    } else {
        return None;
    })
}

/// `r` with the math function applied (unchanged when it does not apply):
/// main = the result in its unit (dB: 2 decimals; mX+b: `dec_for`, "0L"
/// beyond it), `si` = the result, aux1 = the raw reading (the secondary
/// line; AC+DC / AUTO coupling keep their AC / DC parts there instead).
/// An overload stays "0L"; 0 V in dB is NaN ("--", SCPI NaN).
#[inline(never)] // one copy (main.rs READ? sites, App::shown_reading)
pub fn apply(r: &Reading) -> Reading {
    let f = func();
    if !applies(f, r.mode) || r.power.is_some() {
        return *r;
    }
    let x = r.si;
    let (y, dec, unit) = match f {
        DBM => (dbm(x, REFS[ref_idx()] as f32), Some(2), "dBm"),
        DBV => (dbv(x), Some(2), "dBV"),
        _ => {
            let y = m() * x + b();
            (y, dec_for(y), unit())
        }
    };
    let mut o = *r;
    let ol = r.main.ol || (dec.is_none() && !y.is_nan());
    let v = if r.main.ol || r.main.v.is_nan() { f32::NAN } else { y };
    o.main = Val { v: if ol { 0.0 } else { v }, dec: dec.unwrap_or(1), unit, ol, ext: false };
    o.si = v;
    if !r.parts() {
        o.aux1 = Some(Val { ext: false, ..r.main });
    }
    o
}

// Host tests: tools/meter-test/run.sh.
#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn db_formulas() {
        assert!(close(dbv(1.0), 0.0, 1e-4));
        assert!(close(dbv(-1.0), 0.0, 1e-4));
        assert!(close(dbv(0.1), -20.0, 1e-4));
        assert!(close(dbv(10.0), 20.0, 1e-4));
        assert!(close(dbv(0.001), -60.0, 1e-3));
        // 0 dBm into 600 ohm = 0.7746 V.
        assert!(close(dbm(0.774_597, 600.0), 0.0, 1e-3));
        assert!(close(dbm(2.24, 600.0), 9.225, 0.01));
        assert!(close(dbm(1.0, 50.0), 13.010, 0.01));
        assert!(close(dbm(-1.0, 75.0), 11.249, 0.01));
        assert!(close(dbm(0.223_607, 50.0), 0.0, 1e-3));
        assert!(dbv(0.0).is_nan() && dbm(0.0, 50.0).is_nan());
        // Across the f32 range the ln series stays within 1e-4 dB.
        let mut v = 1e-7f32;
        while v < 2000.0 {
            let exact = 20.0 * (v as f64).log10();
            assert!(close(dbv(v), exact as f32, 1e-4 + 1e-6 * exact.abs() as f32), "{v}");
            v *= 1.37;
        }
    }

    #[test]
    fn decimals() {
        assert_eq!(dec_for(1.23456), Some(4));
        assert_eq!(dec_for(-9.99996), Some(3));
        assert_eq!(dec_for(12.3), Some(3));
        assert_eq!(dec_for(999.994), Some(2));
        assert_eq!(dec_for(-9999.9), Some(1));
        assert_eq!(dec_for(9999.96), None);
    }

    fn rd(mode: u8, v: f32, dec: u8, unit: &'static str, k: f32, ol: bool) -> Reading {
        Reading {
            mode,
            range: 0,
            main: Val { v, dec, unit, ol, ext: true },
            aux1: None,
            aux2: None,
            si: v * k,
            auto: true,
            auto_plus: true,
            acdc: false,
            cpl_auto: false,
            seq: 0,
            avg_n: 0,
            alert: crate::sentinel::Alert::None,
            power: None,
            prov: false,
        }
    }

    // One test (the settings are a static): every case in sequence.
    #[test]
    fn apply_and_settings() {
        set_words(&[0, 0, 0, 0]);
        assert_eq!((func(), REFS[ref_idx()], m(), b(), unit()), (OFF, 50, 1.0, 0.0, ""));
        let r = rd(1, 2.24, 4, "V", 1.0, false);
        assert_eq!(apply(&r).main.unit, "V");

        set_func(DBM);
        set_ref_idx(2);
        let o = apply(&r);
        assert_eq!((o.main.unit, o.main.dec, o.main.ol, o.main.ext), ("dBm", 2, false, false));
        assert!(close(o.main.v, 9.225, 0.01) && o.si == o.main.v);
        let a = o.aux1.unwrap();
        assert_eq!((a.v, a.unit, a.ext), (2.24, "V", false));
        // mV: X in volts.
        let o = apply(&rd(3, 100.0, 2, "mV", 1e-3, false));
        assert!(close(o.main.v, -17.782, 0.01));
        // Not in A / ohm / Hz; not on the power meter.
        assert_eq!(apply(&rd(5, 1.0, 4, "A", 1.0, false)).main.unit, "A");
        assert_eq!(apply(&rd(13, 50.0, 2, "Hz", 1.0, false)).main.unit, "Hz");
        // Overload: "0L" in dBm.
        let o = apply(&rd(1, 0.0, 4, "V", 1.0, true));
        assert!(o.main.ol && o.main.unit == "dBm");
        // 0 V: no value.
        assert!(apply(&rd(1, 0.0, 4, "V", 1.0, false)).main.v.is_nan());
        set_func(DBV);
        assert!(close(apply(&rd(0, 1.0, 4, "V", 1.0, false)).main.v, 0.0, 1e-4));
        assert!(close(apply(&rd(1, -0.5, 4, "V", 1.0, false)).main.v, -6.0206, 1e-3));

        // A 1 mV/A clamp on the 600 mV range: m = 1000 A/V, unit "A".
        set_func(SCAL);
        assert!(set_m(1000.0) && set_b(-0.5) && set_unit(b"A"));
        let o = apply(&rd(3, 12.34, 2, "mV", 1e-3, false));
        assert_eq!((o.main.unit, o.main.dec), ("A", 3));
        assert!(close(o.main.v, 11.84, 1e-3));
        // In ohm and A too.
        assert!(apply(&rd(10, 1.0, 4, "k\u{3a9}", 1e3, false)).main.ol);
        assert!(set_m(0.01));
        assert!(close(apply(&rd(10, 1.0, 4, "k\u{3a9}", 1e3, false)).main.v, 9.5, 1e-4));
        // AC+DC keeps its parts in aux1 / aux2.
        let mut p = rd(1, 1.0, 4, "V", 1.0, false);
        p.acdc = true;
        p.aux1 = Some(Val { v: 0.6, dec: 4, unit: "V", ol: false, ext: false });
        assert_eq!(apply(&p).aux1.unwrap().v, 0.6);
        // Refused: m = 0, a non-finite b, a 5-byte or control-character unit.
        assert!(!set_m(0.0) && !set_b(f32::INFINITY) && !set_unit(b"kg/m2") && !set_unit(b"\tA"));
        assert!(set_unit("\u{b0}C".as_bytes()) && unit() == "\u{b0}C");
        // Round trip through the persisted words.
        let w = words();
        set_words(&[0, 0, 0, 0]);
        set_words(&w);
        assert_eq!((func(), REFS[ref_idx()], m(), b(), unit()), (SCAL, 600, 0.01, -0.5, "\u{b0}C"));
        // Garbage words: defaults where invalid.
        set_words(&[0xFF, f32::NAN.to_bits(), 0, 0xFFFF_FFFF]);
        assert_eq!((func(), REFS[ref_idx()], m(), unit()), (SCAL, 50, 1.0, ""));
        set_words(&[0, 0, 0, 0]);
    }
}
