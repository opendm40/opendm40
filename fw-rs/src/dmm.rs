//! Multimeter engine: a port of the stock measurement behaviour.
//! Sources: firmware/re/analysis/hy3131_v2_6.h (bus, config rows, calibration
//! layout) and dmm_v2_6.h (per-mode formulas, filters, settle deadlines,
//! autorange, display formats). Stock field names in comments (m+x, S+x).
//!
//! Modes (HY3131 config tables): 0 ACV, 1 DCV, 2 ACmV, 3 DCmV, 4..6 AC A/mA/uA,
//! 7..9 DC A/mA/uA, 10 ohm, 11 continuity, 12 diode, 13 capacitance,
//! 14 frequency, 15 temperature. AC+DC (stock sub-mode m+5 == 2, offered by
//! the stock only for model byte != 0, here on every model) alternates the
//! HY3131 between the AC and the DC mode of the same quantity (`acdc`).
//!
//! Counts: always the DM40C (60000-count) limits, whatever the model byte says
//! (COUNTS_MODEL). The model byte itself (`model()`) still identifies the
//! instrument (About page, BT name / cmd 00).

use core::ptr::read_volatile;

use crate::clock::Clock;
use crate::hy3131 as hy;
use crate::hy3131_tables::*;
use crate::mmio::{modify32, w32};

const GPIOA: u32 = 0x4002_0000;
const GPIOC: u32 = 0x4002_0800;
const GPIOE: u32 = 0x4002_1000;

/// Settings record in internal flash (read only): u16 crc, u16 ~crc,
/// u32 version = 1, data. DMM calibration at record offset 0x168.
const SETTINGS: u32 = 0x080C_0000;
const CAL: u32 = SETTINGS + 0x168;

/// Model index used for the counts-dependent limits: 0 = DM40A (40000 counts),
/// 1 = DM40B (50000), 2 = DM40C (60000). The stock picks FACTOR/OVER by the
/// calibration model byte (dmm_v2_6.h:297-299, hy3131_v2_6.h:147-155); the HY3131
/// setup is identical for all three, so fw-rs always uses the DM40C values.
/// Set to `self.model` in factor()/over() to restore the stock behaviour.
const COUNTS_MODEL: u8 = 2;
const FACTOR: [f32; 3] = [4.0, 5.0, 6.0];
const OVER: [f32; 3] = [1.05, 1.04, 1.03];
const AD1_DIV: f32 = 8_388_606.0;
const K_ACV: [f32; 4] = [10.0, 100.0, 1000.0, 10000.0];
const MV_DIV: [f32; 2] = [10.0, 1.0]; // also the A op gain
const DEC4: [f32; 4] = [1.0, 10.0, 100.0, 1000.0];
const SHUNT: [f32; 3] = [0.01, 0.0005, 4.75e-5]; // ohm / kohm / Mohm
const OHM_RREF: [f32; 7] = [0.1, 1.0, 10.0, 100.0, 1000.0, 10000.0, 63000.0];
const OHM_DEC: [f32; 7] = [0.01, 0.1, 1.0, 10.0, 100.0, 1000.0, 10000.0];
const CAP_DEC: [f32; 8] = [0.001, 0.01, 0.1, 1.0, 10.0, 100.0, 1000.0, 10000.0];
const CAP_WIN: [f32; 8] = [0.0008, 0.0002, 0.0001, 0.0001, 0.0001, 3.0, 30.0, 200.0];
const CAP_K: [f32; 8] = [1020.0, 1003.0, 102.7, 10.68, 1.5, 4.06, 41.0, 400.0];
const CAP_R21: [u8; 3] = [0xDC, 0xCC, 0xBB];
const FREQ_DEC: [f32; 7] = [0.01, 0.1, 1.0, 10.0, 100.0, 1000.0, 10000.0];
const ACV_WIN: [f32; 4] = [0.002, 0.01, 0.1, 1.0];
const ACMV_WIN: [f32; 2] = [0.01, 0.1];
const DCV_NEGK: [f32; 4] = [1.0, 1.00008, 1.00011, 1.0015];
const ACA_WIN: [f32; 6] = [0.001, 0.01, 0.01, 0.1, 0.1, 1.0];
const TC_K_MV: [f32; 16] = [
    -6.942, -4.8, -1.051, 3.044, 7.087, 11.157, 15.346, 19.592, 23.854, 28.077, 32.223, 36.274, 40.224,
    44.067, 47.786, 51.358,
];
const CFG_F13_R7: [u8; hy::CFG_LEN] = [
    0x52, 0xbb, 0x14, 0x83, 0x43, 0x40, 0x00, 0x5d, 0x31, 0x00, 0x00, 0x00, 0x88, 0x08, 0x00, 0x8e, 0x80, 0xc2, 0xcc,
    0xa8, 0x00, 0x54,
];
const CFG_F13_DISCHARGE: [u8; hy::CFG_LEN] = [
    0x52, 0xdd, 0x13, 0x83, 0x43, 0x40, 0x00, 0x5d, 0x31, 0x00, 0x00, 0x00, 0x88, 0x88, 0x00, 0x8e, 0x80, 0xc2, 0xcc,
    0xa8, 0x00, 0x54,
];

// ------------------------------------------------------- +1 digit, mohm
//
// AD1 (hy3131_v2_6.h): signed 24 bit, full scale 1.25 V / 0.9 = 1.3889 V at
// 8388607 codes (0.166 uV/code); ohm is ratiometric, code = R / Rref * 0.9 *
// 2^23. Every DC front end maps its 60000-count full scale to ~0.6 V at AD1, so
// the codes per displayed count are nearly the same everywhere:
//   function  range                        codes/count  per +1 digit  +1 digit
//   DC V      r0..r2  6 / 60 / 600 V           60.4         6.0         yes
//   DC V      r3      1000 V (10000 counts)    60.4         6.0         no (HV divider self-heating)
//   DC mV     r1      600 mV                   60.4         6.0         yes
//   DC mV     r0      60 mV (OP1 x10)          60.4         6.0         no (0.1 uV step: amp noise, EMF)
//   DC A      r0, r1  6 A / 10 A (10 mohm)     60.4         6.0         no (0.1 uV across the shunt)
//   DC mA     r0, r1  60 / 600 mA (0.5 ohm)    30.2         3.0         no
//   DC uA     r0, r1  600 / 6000 uA (47.5 ohm) 28.7         2.9         no
//   ohm       r0      60 ohm (Rref 0.1k)       75.5 (1 mohm) 7.5        no (already mohm)
//   ohm       r1..r3  600 ohm .. 60 kohm       75.5         7.5         yes (r1: 600.000 ohm = mohm)
//   ohm       r4, r5  600 kohm, 6 Mohm         75.5         7.5         no (hum / leakage)
//   ohm       r6      60 Mohm (Rref 63 M)     119.8        12.0         no (hum / leakage)
//   continuity 75.5, diode 60.4, AC (AD2/RMS), cap, Hz, temp: no.
// A single conversion has only 6..7.5 codes per extra-digit step, so the
// extra digit comes from a moving average of the raw codes at the slow OSR
// (R22 = 0x17, OSR 32768, the stock's steady-state setting and already the
// slowest AD1OSR code): the mean of N codes has 1/N code granularity (N = 8:
// 48 levels per step, N = 16: 120) and the noise drops by sqrt(N). The window
// slides, so the reading rate stays the stock one (one reading per 400 ms
// conversion at OSR 32768 chopped, hy3131::ad1_period_us; a window of 8 spans
// 3.2 s, of 16 6.4 s); it restarts on a step of more than ~2 counts of the
// 60000-count format, on any OSR / range / mode change, and is not used in
// AC+DC. Noise floor: not measured yet.
// Ohm r0 is the stock 60 ohm row (f10 row 0: OPS1 = 1, the OP1 x10 gain also
// used by DC mV / A r0; the stock Rref constant 0.1 kohm = the 1 kohm r1
// reference / 10) that the stock never selects: manual only here, always with
// the default ohm variant (SFUVR = 2, the ~0.85 mA one; the "ON" variant gives
// ~0.27 mA - community measurements on r1). Calibration slot 0x124[0] is the
// factory default 1.005 on the dumped unit (r1..r6 are calibrated), and the
// single offset 0x144 is the r1..r6 one: use REL, check the gain on hardware.

// ------------------------------------------------- FR-05 conversion rate
//
// `Rate` sets how much converter time goes into one reading of the AD1 (DC)
// functions; the display smoothing on top of the readings is meter.rs
// (`Smooth`). Conversion periods from the datasheet formula
// (hy3131::ad1_period_us, OSR * 30 / SYSCLK, x2 with the chopper the stock
// rows always set; emu/hy3131/RESEARCH.md 3): OSR 8192 (0x15) 100 ms, OSR
// 32768 (0x17) 400 ms. The emulator models the same; on the device only the
// logger's 38/s at 0x13 (40/s by the formula) was measured so far, so `reading_period_ms` MEASURES
// the period (SENS:RATE:PER?, the rate chip). Before 2026-10-14 this table
// assumed ~107 ms at OSR 32768 ("~9.4/s"): every time below was 4x too short.
//   rate    AD1 OSR (in range)        code window (step reset ~2 counts)   readings
//   FAST    8192 (0x15) always         none                                 10/s (one per conversion)
//   MEDIUM  stock: 0x15 while moving,  V r0..r2 / mV r1: 8 (3.2 s),         10/s moving, 2.5/s steady
//   (dflt)  0x17 (32768) once stable   ohm r0..r3: 16 (6.4 s), others none  (stock)
//   SLOW    as MEDIUM                  16 every range (6.4 s; ohm r0..r3:    as MEDIUM (one per conversion):
//                                      32, 12.8 s)                          10/s moving, 2.5/s steady
// SLOW differs from MEDIUM only by the longer window (lower noise, slower to
// settle); it reports every conversion like MEDIUM, so READ? (SCPI_QUICK_MS
// 850 ms) always gets a fresh reading. (Until 2026-10-14 SLOW reported one
// reading per 4 conversions, sized for the old ~107 ms guess: 0.625/s at the
// real 400 ms.)
// The +1 digit (`Val::ext`, ext-capable ranges of the table above) is shown
// only while the window holds at least AVG_FULL slow (OSR 32768) conversions:
// never in FAST (one OSR-8192 conversion has ~2x the noise of one at 32768),
// in MEDIUM / SLOW from the 8th conversion after a step / range change, i.e.
// ~3.2 s at 400 ms plus the moving-OSR conversions before the switch (before
// fw-rs 2026-10-13 the digit was drawn from the first conversion on).
// Applies to DC V, DC mV, DC A / mA / uA and ohm, not in AC+DC (the DC phase
// stays at the row's OSR 8192, ACDC_STOCK_OSR; its own phase logic). Not applicable (stock timing in every rate):
// AC (AD2 / RMS converter, its filter is the RMS EMA), capacitance and
// frequency (counter gates of 420 / 350 ms), temperature (thermocouple: three
// phases, ~2.2 s; resistive sensors: fixed OSR 32768 + EMA), continuity
// (instant beep, fast OSR) and diode (stock EMA).

/// Conversion rate of the AD1 functions (see the table above).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rate {
    Slow = 0,
    Medium = 1,
    Fast = 2,
}

/// SCPI / settings names, indexed by `Rate as usize`.
pub const RATE_NAMES: [&str; 3] = ["SLOW", "MED", "FAST"];

impl Rate {
    pub const ALL: [Rate; 3] = [Rate::Slow, Rate::Medium, Rate::Fast];

    pub fn from_u8(v: u8) -> Option<Rate> {
        Rate::ALL.get(v as usize).copied()
    }
}

/// AD1OSR codes: the stock steady (32768) and moving (8192) settings.
const OSR_SLOW: u8 = 0x17;
const OSR_FAST: u8 = 0x15;

/// UF-21: text for the UI when the mV-first AUTO is selected for V
/// (`Dmm::mv_auto_warn`).
pub const MV_AUTO_WARN: &str = "AUTO starts on mV: AUTO+ is safer";

/// Code window of an AD1 range (FR-05 table): (window, restart threshold in
/// codes, extra digit possible); None = no averaging (avg_n 0). Thresholds
/// ~2 counts of the 60000-count format: V / mV / A 60.4 codes per count,
/// mA 30.2, uA 28.7, ohm 75.5, ohm r6 119.8 (see the +1 digit table).
fn avg_cfg(rate: Rate, acdc: bool, mode: u8, range: i8) -> Option<(usize, i32, bool)> {
    if acdc || rate == Rate::Fast {
        return None;
    }
    let (medium, thr, ext) = match (mode, range) {
        (1, 0..=2) | (3, 1) => (8, 120, true),
        (1 | 3 | 7, _) => (1, 120, false),
        (8, _) => (1, 60, false),
        (9, _) => (1, 57, false),
        (10, 0) => (16, 150, false),
        (10, 1..=3) => (16, 150, true),
        (10, 6) => (1, 240, false),
        (10, _) => (1, 150, false),
        _ => return None,
    };
    let n = match rate {
        Rate::Slow if mode == 10 && range <= 3 => 32,
        Rate::Slow => 16,
        _ => medium,
    };
    (n > 1).then_some((n, thr, ext))
}

/// The extra digit is real: an ext-capable range with at least AVG_FULL
/// slow conversions in the window (FAST never averages, see `avg_cfg`).
fn ext_ok(ext: bool, n: u8) -> bool {
    ext && n >= AVG_FULL
}

/// Moving average of raw AD1 codes (|AD1| in ohm).
struct Avg {
    buf: [i32; AVG_MAX],
    n: usize,
    i: usize,
    sum: i32,
    len: usize,
}

const AVG_MAX: usize = 32;
/// Readings with at least this many slow conversions averaged have the full
/// +1 digit resolution (`Val::ext`); also what SCPI MEAS? waits for.
pub const AVG_FULL: u8 = 8;

impl Avg {
    const fn new() -> Self {
        Avg { buf: [0; AVG_MAX], n: 0, i: 0, sum: 0, len: 0 }
    }

    fn reset(&mut self) {
        self.n = 0;
        self.i = 0;
        self.sum = 0;
    }

    /// Add `x` to a window of `len` codes, restarting when `x` is more than
    /// `thr` codes from the mean or the window length changed. Returns the
    /// mean (sum <= 32 * 2^23 fits i32; the fraction is added separately to
    /// keep it exact in f32).
    #[inline(never)]
    fn push(&mut self, x: i32, len: usize, thr: i32) -> f32 {
        let len = len.clamp(1, AVG_MAX);
        if len != self.len || (self.n > 0 && (x - self.sum / self.n as i32).abs() > thr) {
            self.reset();
            self.len = len;
        }
        if self.n == len {
            self.sum -= self.buf[self.i];
        } else {
            self.n += 1;
        }
        self.buf[self.i] = x;
        self.sum += x;
        self.i = (self.i + 1) % len;
        let n = self.n as i32;
        (self.sum / n) as f32 + (self.sum % n) as f32 / n as f32
    }
}

// ------------------------------------------------------------ temperature
//
// Sensors of the temperature function (mode 15). 0 = the stock type-K
// thermocouple path (cfg f15, three phases, cold junction = the HY3131 internal
// sensor). 1..6 are 2-wire resistive sensors read through the ohm path (cfg
// f10_p8clr, the stock default ohm variant m+8 == 0, ratiometric:
// R = |AD1| * Rref[r] / 0.9 / 2^23, gain cal 0x124 + 4r, offset cal 0x144):
//   PT100  fixed r1 (600 ohm, Rref 1 k), PT1000 fixed r2 (6 k, Rref 10 k):
//     Callendar-Van Dusen (IEC 60751), -200..850 C.
//   NTC (Beta model, 1/T = 1/T25 + ln(R/R25)/B), -55..300 C: r2..r6 with
//     hysteresis, starting at the lowest range that holds R25.
// Excitation: the ohm source looks like a ~0.85 V open-circuit source behind
// Rref (community: 0.85 mA short-circuit current on the 600 ohm range in the
// default variant, 0.27 mA in the "ON" variant; NOT from a datasheet - verify
// on hardware). I = VSRC / (Rref + R), P = I^2 R; see `t_excitation`.
// Lead resistance is not modelled separately: a 2-wire lead pair adds
// 0.39 C per 0.1 ohm on a PT100 (0.039 C on a PT1000, negligible for NTCs);
// trim it with the per-sensor offset (or REL).

/// Short sensor names (SCPI CONF:TEMP <name>, SENS:TEMP:TRAN?).
pub const T_NAMES: [&str; 7] = ["TCK", "PT100", "PT1000", "NTC10K3950", "NTC10K3435", "NTC100K3950", "NTCUSER"];
/// Inferred open-circuit voltage of the ohm source (see above).
pub const T_VSRC: f32 = 0.85;

/// Natural log (no libm): exponent * ln2 + atanh series of the mantissa.
fn ln(x: f32) -> f32 {
    let b = x.to_bits();
    let mut e = ((b >> 23) & 0xFF) as i32 - 127;
    let mut m = f32::from_bits((b & 0x007F_FFFF) | 0x3F80_0000);
    if m > 1.414_213_6 {
        m *= 0.5;
        e += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    e as f32 * core::f32::consts::LN_2 + 2.0 * s * (1.0 + s2 * (1.0 / 3.0 + s2 * (0.2 + s2 * (1.0 / 7.0))))
}

/// Platinum RTD, IEC 60751 Callendar-Van Dusen: R/R0 -> C (None outside
/// -200..850 C). T >= 0 closed form (written without the cancellation of the
/// textbook quadratic), T < 0 Newton on the full equation.
fn pt_temp(x: f32) -> Option<f32> {
    const A: f32 = 3.9083e-3;
    const B: f32 = -5.775e-7;
    const C: f32 = -4.183e-12;
    if !(0.18..=3.95).contains(&x) {
        return None;
    }
    let mut t = 2.0 * (x - 1.0) / (A + sqrt(A * A + 4.0 * B * (x - 1.0)));
    if x < 1.0 {
        for _ in 0..4 {
            let f = 1.0 + t * (A + t * B) + C * (t - 100.0) * t * t * t - x;
            let d = A + 2.0 * B * t + C * t * t * (4.0 * t - 300.0);
            t -= f / d;
        }
    }
    if (-200.0..=850.0).contains(&t) { Some(t) } else { None }
}

/// NTC, Beta model: R (ohm) -> C (None outside -55..300 C).
fn ntc_temp(r: f32, r25: f32, beta: f32) -> Option<f32> {
    if r <= 0.0 {
        return None;
    }
    let t = 1.0 / (1.0 / 298.15 + ln(r / r25) / beta) - 273.15;
    if (-55.0..=300.0).contains(&t) { Some(t) } else { None }
}

/// The six bottom buttons of the stock screen.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Volt,
    Amp,
    Ohm,
    Cap,
    ContDiode,
    HzTemp,
}

impl Button {
    pub const ALL: [Button; 6] = [Button::Volt, Button::Amp, Button::Ohm, Button::Cap, Button::ContDiode, Button::HzTemp];
}

/// A formatted readout: value in display units, digits, unit text, OL.
#[derive(Clone, Copy)]
pub struct Val {
    pub v: f32,
    /// Decimals of the 60000-count format (history, chart, BT, aux use this).
    pub dec: u8,
    pub unit: &'static str,
    pub ol: bool,
    /// `v` resolves one more digit (averaged reading, see `avg_cfg`): the
    /// main readout shows `dec + 1` decimals, the last one in a small font.
    pub ext: bool,
}

impl Val {
    const fn new(v: f32, dec: u8, unit: &'static str) -> Self {
        Val { v, dec, unit, ol: false, ext: false }
    }
    const fn ol(dec: u8, unit: &'static str) -> Self {
        Val { v: 0.0, dec, unit, ol: true, ext: false }
    }
    /// The readout with the extra digit (not for OL).
    const fn with_ext(self, ext: bool) -> Self {
        Val { ext: ext && !self.ol, ..self }
    }
}

#[derive(Clone, Copy)]
pub struct Reading {
    pub mode: u8,
    pub range: i8,
    pub main: Val,
    pub aux1: Option<Val>,
    pub aux2: Option<Val>,
    /// Main value in base SI units (V, A, ohm, F, Hz, C) for SCPI / BT.
    pub si: f32,
    pub auto: bool,
    /// Range label in auto: "AUTO+" instead of "AUTO".
    pub auto_plus: bool,
    /// AC+DC sub-mode: main = sqrt(AC^2 + DC^2), aux1 = AC part, aux2 = DC
    /// part; `mode` is then always the DC mode (1, 3, 7..9).
    pub acdc: bool,
    pub seq: u32,
    /// Conversions in the moving average behind `main` / `si` (0 = not an
    /// averaged range); `AVG_FULL` or more = full +1-digit resolution.
    pub avg_n: u8,
}

impl Reading {
    pub fn button(&self) -> Button {
        button_of(self.mode)
    }
    pub fn is_ac(&self) -> bool {
        matches!(self.mode, 0 | 2 | 4 | 5 | 6)
    }
}

pub fn button_of(mode: u8) -> Button {
    match mode {
        0..=3 => Button::Volt,
        4..=9 => Button::Amp,
        10 => Button::Ohm,
        11 | 12 => Button::ContDiode,
        13 => Button::Cap,
        _ => Button::HzTemp,
    }
}

fn cal_ok() -> bool {
    (unsafe { read_volatile((SETTINGS + 4) as *const u32) }) == 1
}

/// Calibration float at struct offset `off`, `dflt` if missing or not sane.
fn cal(off: u32, dflt: f32) -> f32 {
    let v = unsafe { read_volatile((CAL + off) as *const f32) };
    if cal_ok() && v.is_finite() && v.abs() < 1.0e6 { v } else { dflt }
}

/// FUN_080160c6: EMA that jumps (0.99) when the step exceeds `thr`.
fn ema(thr: f32, a: f32, x: f32, y: &mut f32) {
    let d = x - *y;
    let k = if d.abs() < thr { a } else { 0.99 };
    *y += k * d;
}

fn sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut y = f32::from_bits((x.to_bits() >> 1) + 0x1FBD_1DF5);
    for _ in 0..3 {
        y = 0.5 * (y + x / y);
    }
    y
}

fn hypot(a: f32, b: f32) -> f32 {
    sqrt(a * a + b * b)
}

/// AC+DC part readout: "0L" when |part / full scale| > 1.04 (dmm_v2_6.h:231).
fn part(x: f32, lim: f32, dec: u8, unit: &'static str) -> Val {
    // The value is kept with the OL flag: BT still sends it (bt_meas_v2_6.h:170).
    Val { v: x, dec, unit, ol: x.abs() / lim > 1.04, ext: false }
}

fn u24(b: &[u8]) -> u32 {
    b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16
}

fn relay_pa(clk: &mut Clock, arg: u8) {
    // Latching relay pair: 10 ms pulse. 0: PA10=1 PA9=0, 1: PA10=0 PA9=1.
    let (set, clr) = if arg == 0 { (10, 9) } else { (9, 10) };
    unsafe {
        w32(GPIOA + 0x18, (1 << set) | (1 << (clr + 16)));
        modify32(GPIOA + 0x00, (3 << 18) | (3 << 20), (1 << 18) | (1 << 20));
    }
    clk.delay_ms(10);
    unsafe { w32(GPIOA + 0x18, (1 << (9 + 16)) | (1 << (10 + 16))) };
}

fn relay_pe(arg: u8) {
    // Shunt select: 0: PE0=1 PE1=0, 1: PE0=0 PE1=1, 2: both 0.
    let bop = match arg {
        0 => (1 << 0) | (1 << (1 + 16)),
        1 => (1 << (0 + 16)) | (1 << 1),
        _ => (1 << (0 + 16)) | (1 << (1 + 16)),
    };
    unsafe {
        w32(GPIOE + 0x18, bop);
        modify32(GPIOE + 0x00, 3 | (3 << 2), 1 | (1 << 2));
    }
}

fn pc7(high: bool) {
    unsafe {
        w32(GPIOC + 0x18, if high { 1 << 7 } else { 1 << (7 + 16) });
        modify32(GPIOC + 0x00, 3 << 14, 1 << 14);
    }
}

/// Frequency counter state (FUN_08013450).
struct Freq {
    next: u64,
    src_alt: bool,
    last: [f32; 2],
    change: [f32; 2],
    f_khz: f32,
    duty: f32,
    range: u8,
}

/// HY3131 counter registers (CTC R15..17, CTB R18..1A, CTA R1B..1D) ->
/// (frequency kHz, duty %). n = CTA + 0x40000 ticks of the 4.9152 MHz time
/// base (CTA restarts at 0xfc0000), CTB input periods, CTC high ticks.
pub(crate) fn counter_vals(ctc: u32, ctb: u32, cta: u32) -> (f32, f32) {
    let n = cta as f32 + 262_144.0;
    (ctb as f32 / n * 4915.28, ctc as f32 / n * 100.0)
}

/// Frequency auto range for `f` kHz; x = factor * over (6.18 kHz at 60000
/// counts): range r spans up to x * 10^(r-2). Above 10000 x: `prev`.
pub(crate) fn freq_range_for(f: f32, x: f32, prev: u8) -> u8 {
    FREQ_DEC.iter().position(|&d| f < x * d).map_or(prev, |r| r as u8)
}

/// Frequency readout for `f` kHz in range `ri`. The stock notes give
/// printf format indices (fmt k = "%7.(4-k)f": Hz fmt ri+1, kHz fmt ri-2,
/// MHz fmt ri-5; aux one digit less), so the decimals are 4 - fmt, as in the
/// other functions: 60.000 Hz, 600.00 Hz, 6.0000 / 60.000 / 600.00 kHz,
/// 6.0000 / 60.000 MHz. (Using fmt as the decimals showed 0.7 kHz as "1".)
pub(crate) fn freq_val(f: f32, ri: u8, aux: bool) -> Val {
    let ri = ri as i32;
    let a = aux as i32;
    let (v, dec, unit) = if ri < 2 {
        (f * 1000.0, 3 - ri - a, "Hz")
    } else if ri < 5 {
        (f, 6 - ri - a, "kHz")
    } else {
        (f * 1e-3, 9 - ri - a, "MHz")
    };
    Val::new(v, dec.clamp(0, 4) as u8, unit)
}

/// Capacitance ramp state (r5..r7).
struct CapRamp {
    discharging: bool,
    prev_v: f32,
    prev_t: u64,
    med: [f32; 5],
    med_i: usize,
    class: u8,
    buf: [f32; 35],
    n: usize,
    cmp: Option<bool>,
    first: bool,
}

pub struct Dmm {
    model: u8,
    mode: u8,
    /// m+5 == 2: AC+DC. `phase_n` = S+10 (readings left in this phase),
    /// `dc_val` = S+0x20 / m+0xcc (last DC part), `ac_raw` = S+0x1c.
    acdc: bool,
    phase_n: u8,
    dc_val: f32,
    ac_raw: f32,
    /// AC+DC parts measured since the last reset (bit 0 AC, bit 1 DC): no
    /// reading until both are, so the first value (and a SCPI MEAS) is never
    /// |DC| or |AC| alone (the stock shows hypot(0, DC) at first).
    acdc_seen: u8,
    range: i8,
    manual: bool,
    /// m+6: V autorange (bit0 mV->V, bit1 V->mV); m+7: A (1 mA..A, 3 uA..A).
    v_auto: u8,
    a_auto: u8,
    /// m+8: ohm variant (selects f10_p8set when 1).
    ohm_var: u8,
    /// S+0x40: no reading accepted before this time.
    deadline: u64,
    entry: u64,
    prev_ad1: i32,
    r22: u8,
    ohm_settled: bool,
    f_ac: f32,
    f_lpf: f32,
    f_dc: f32,
    f_diode: f32,
    f_cap: f32,
    cap_c: f32,
    cap_cnt_up: u8,
    cap_cnt_dn: u8,
    cap_cnt_small: u8,
    ramp: CapRamp,
    beep_class: u8,
    beep_e: u8,
    beep_next: u64,
    beep_n: u8,
    t_phase: u8,
    t_code_a: i32,
    t_cj: f32,
    freq: Freq,
    seq: u32,
    pa_now: Option<u8>,
    /// Analog DMM rail (PE2) off: no polling (App::update_rails).
    suspended: bool,
    /// Temperature sensor (T_NAMES index), per-sensor offsets (0.1 C; the
    /// TC one is applied to the cold junction), user NTC R25 (ohm) / Beta.
    t_sensor: u8,
    t_offs: [i16; 7],
    ntc_user: (f32, f32),
    /// ms of the last cold-junction measurement (0 = never).
    t_cj_at: u64,
    /// Resistive sensors: filtered C, last R (ohm), range-change votes.
    t_f: f32,
    t_res: f32,
    t_votes: u8,
    /// +1 digit: moving average of the raw codes (`avg_cfg`); `avg_skip`
    /// drops the first conversion after the OSR was switched to 0x17.
    avg: Avg,
    avg_skip: bool,
    /// Conversions in the mean of the reading being produced (Reading::avg_n).
    avg_n: u8,
    /// AC+DC DC phase: the stock's AD1-OSR once-flag (DAT_080141fc and the
    /// mV / A ones): set when R22 = 0x17 was written, cleared by an AD1 step.
    acdc_osr_once: bool,
    /// AC+DC diagnostic: the last ACDC_DIAG_N AC phases (ring), the number
    /// of AC phases seen, and whether the current AC phase is being logged.
    diag: [AcPhaseDiag; ACDC_DIAG_N],
    diag_k: u32,
    diag_on: bool,
    /// Diagnostic: time of the previous logged result, counter restarts since.
    diag_t: u64,
    diag_ctr: u8,
    /// FR-05 conversion rate (persisted).
    rate: Rate,
    /// Time of `poll` (ms), for the interval measurements below.
    t_poll: u64,
    /// Last reading / last AD1 conversion (ms; None: the next interval spans
    /// a config write and is not counted) and their measured mean intervals.
    t_rd: Option<u64>,
    period_ms: f32,
    t_conv: Option<u64>,
    conv_ms: f32,
    /// `avg_n` of the latest reading (`avg_n` itself is per poll).
    rd_avg_n: u8,
}

/// Mean interval: first sample taken as is, then an EMA (1/4).
fn ema_ms(acc: &mut f32, last: &mut Option<u64>, now: u64) {
    if let Some(t) = *last {
        let dt = now.saturating_sub(t) as f32;
        *acc = if *acc == 0.0 { dt } else { *acc + 0.25 * (dt - *acc) };
    }
    *last = Some(now);
}

// AC+DC: what reaches the HY3131 and the board, as the stock does it
// (docs/acdc-stock-vs-fwrs.md sections 7 and 8.1), one switch per difference
// so they can be bisected on the device (false = the fw-rs 9421fd5 way).
//
/// DC phase at the config row's OSR 8192 (R22 = 0x15). The stock's 0x17
/// write is gated by a once-flag that stays set in AC+DC (it is only cleared
/// by an AD1 step), so its DC phase stays at 8192. false: R22 = 0x17 right
/// after every DC-phase row.
const ACDC_STOCK_OSR: bool = true;
/// FUN_08012c30 pulses the PA latching relay (10 ms) on every call, i.e. on
/// every phase switch, between the R26/R34/R36 zeroing and the 22-byte row.
/// false: only when the PA state changes (never inside V AC+DC).
const ACDC_RELAY_PULSE: bool = true;
/// FUN_08013734 runs the frequency counter (FUN_08013450, restart every
/// 350 ms: R20 = 0xf0, CTA, R20 = 0xf2) in AC+DC too: on every poll, also
/// during the settle, in the ACV / DCV / DCmV / DCA phases, and after every
/// result in the ACA phase (never in the ACmV phase).
const ACDC_FREQ_RESTART: bool = true;

/// AC+DC diagnostic ring: AC phases kept (`acdc_diag`).
const ACDC_DIAG_N: usize = 4;

/// AC+DC diagnostic: the RMS results of one AC phase (up to 4, the stock V
/// phase length), and when the first one came after the settle deadline.
#[derive(Clone, Copy)]
pub struct AcPhaseDiag {
    /// ms between the settle deadline and the first result.
    pub late_ms: u16,
    /// Results recorded (1..=4).
    pub n: u8,
    /// sqrt(RMS) raw counts (the stock S+0x50).
    pub sq: [u32; 4],
    /// LPF raw (19-bit signed, the stock S+0x4c).
    pub lpf: [i32; 4],
    /// LPF scaled = the DC seen by the AC path (V / mV / A unit of the
    /// phase), the stock S+0x54. Stock, 40 Hz 0..9.63 V PWM at r1: 0.07..0.10
    /// V at the first result; 0.3..0.8 V = the coupling node still settling.
    pub dcp: [f32; 4],
    /// ms since the previous result (first of an AC+DC phase: since the
    /// phase switch, i.e. settle + late_ms).
    pub dt: [u16; 4],
    /// INTF as read before the result, bit 8 set when the frequency counter
    /// was restarted (R20 / CTA writes) since the previous result.
    pub fl: [u16; 4],
}

impl AcPhaseDiag {
    const EMPTY: AcPhaseDiag = AcPhaseDiag { late_ms: 0, n: 0, sq: [0; 4], lpf: [0; 4], dcp: [0.0; 4], dt: [0; 4], fl: [0; 4] };
}

impl Dmm {
    pub fn new() -> Self {
        hy::init_pins();
        let model = unsafe { read_volatile(CAL as *const u8) }.min(2);
        Dmm {
            model,
            mode: 1,
            acdc: false,
            phase_n: 0,
            dc_val: 0.0,
            ac_raw: 0.0,
            acdc_seen: 0,
            range: 0,
            manual: false,
            // UF-21: AUTO+ (V first, up-only) by default; the stock default
            // AUTO (3) steps down to the mV ranges.
            v_auto: 1,
            a_auto: 3,
            ohm_var: 0,
            deadline: 0,
            entry: 0,
            prev_ad1: 0,
            r22: 0,
            ohm_settled: false,
            f_ac: 0.0,
            f_lpf: 0.0,
            f_dc: 0.0,
            f_diode: 0.0,
            f_cap: 0.0,
            cap_c: 0.0,
            cap_cnt_up: 0,
            cap_cnt_dn: 0,
            cap_cnt_small: 0,
            ramp: CapRamp {
                discharging: false,
                prev_v: 0.0,
                prev_t: 0,
                med: [0.0; 5],
                med_i: 0,
                class: 0,
                buf: [0.0; 35],
                n: 0,
                cmp: None,
                first: true,
            },
            beep_class: 4,
            beep_e: 0,
            beep_next: 0,
            beep_n: 0,
            t_phase: 0,
            t_code_a: 0,
            t_cj: 0.0,
            freq: Freq { next: 0, src_alt: false, last: [0.0; 2], change: [0.0; 2], f_khz: 0.0, duty: 0.0, range: 0 },
            seq: 0,
            pa_now: None,
            suspended: false,
            t_sensor: 0,
            t_offs: [0; 7],
            ntc_user: (10_000.0, 3950.0),
            t_cj_at: 0,
            t_f: 0.0,
            t_res: 0.0,
            t_votes: 0,
            avg: Avg::new(),
            avg_skip: false,
            avg_n: 0,
            acdc_osr_once: true,
            diag: [AcPhaseDiag::EMPTY; ACDC_DIAG_N],
            diag_k: 0,
            diag_on: false,
            diag_t: 0,
            diag_ctr: 0,
            rate: Rate::Medium,
            t_poll: 0,
            t_rd: None,
            period_ms: 0.0,
            t_conv: None,
            conv_ms: 0.0,
            rd_avg_n: 0,
        }
    }

    pub fn model(&self) -> u8 {
        self.model
    }
    /// Function code as seen from outside (= `func()`): the stock mode, or
    /// 16 / 17 in AC+DC. main.rs gates the data logger on this, so LOG:STATe ON
    /// is refused in AC+DC (its HY3131 mode alternates).
    pub fn mode(&self) -> u8 {
        self.func()
    }
    /// The HY3131 mode currently configured (AC+DC: the current phase).
    pub fn hy_mode(&self) -> u8 {
        self.mode
    }
    pub fn range(&self) -> i8 {
        self.range
    }
    pub fn button(&self) -> Button {
        button_of(self.mode)
    }
    pub fn is_auto(&self) -> bool {
        !self.manual
    }
    pub fn v_auto(&self) -> u8 {
        self.v_auto
    }
    pub fn a_auto(&self) -> u8 {
        self.a_auto
    }
    /// Select the ohm variant (stock m+8: 0 normal, 1 "ON" low current,
    /// the p8set rows; r0 always uses the normal row) and re-enter ohm in
    /// the current range, as the stock's second tap on the ohm button.
    pub fn set_ohm_var(&mut self, clk: &mut Clock, v: u8) {
        self.ohm_var = v & 1;
        if self.mode == 10 {
            let r = self.range.clamp(0, 6);
            self.set(clk, 10, r, 300);
        }
    }

    pub fn ohm_var(&self) -> u8 {
        self.ohm_var
    }
    /// Restore m+6 / m+7 / m+8 (settings): only the values the UI / BT can
    /// set (V, A: 1 = AUTO+, 3 = AUTO; ohm variant 0 / 1); anything else
    /// keeps the default. Takes effect with the next `select`.
    pub fn set_auto_flags(&mut self, v: u8, a: u8, ohm: u8) {
        if matches!(v, 1 | 3) {
            self.v_auto = v;
        }
        if matches!(a, 1 | 3) {
            self.a_auto = a;
        }
        if ohm <= 1 {
            self.ohm_var = ohm;
        }
    }

    /// UF-21: the V autorange is the mV-first AUTO (stock default, `v_auto`
    /// 3) and V is measured in auto range: the UI shows MV_AUTO_WARN (Range
    /// page / status line). AUTO+ (1, the fw-rs default) never warns.
    pub fn mv_auto_warn(&self) -> bool {
        self.button() == Button::Volt && !self.manual && self.v_auto == 3
    }

    // ------------------------------------------------- FR-05 rate / timing

    pub fn rate(&self) -> Rate {
        self.rate
    }

    /// Set the conversion rate (see the FR-05 table). Takes effect with the
    /// next conversion: the code window restarts and `osr` / `poll_ohm`
    /// write the new steady OSR; no settle, no function restart.
    pub fn set_rate(&mut self, r: Rate) {
        if r != self.rate {
            self.rate = r;
            self.avg.reset();
            self.t_rd = None;
            self.period_ms = 0.0;
        }
    }

    /// The rate setting changes something in the current function (DC V /
    /// mV / A, ohm; not AC+DC): the UI can grey the row out otherwise.
    pub fn rate_applies(&self) -> bool {
        !self.acdc && matches!(self.mode, 1 | 3 | 7..=10)
    }

    /// Measured mean interval between readings (ms, EMA over the last few;
    /// intervals across a function / range change are not counted). 0 until
    /// two readings were seen. Use for "x readings/s" in the UI and SCPI.
    pub fn reading_period_ms(&self) -> u32 {
        self.period_ms as u32
    }

    /// Converter time behind the latest reading (ms): the code window length
    /// times the measured AD1 conversion interval for the AD1 functions,
    /// else the reading period (the readings of AC / cap / Hz / temperature
    /// are not windowed here). The display smoothing (meter.rs) comes on top.
    pub fn integration_ms(&self) -> u32 {
        if self.rate_applies() && self.conv_ms > 0.0 {
            (self.rd_avg_n.max(1) as f32 * self.conv_ms) as u32
        } else {
            self.period_ms as u32
        }
    }

    /// Steady AD1OSR of the rate (in range, not moving).
    fn steady_osr(&self) -> u8 {
        if self.rate == Rate::Fast { OSR_FAST } else { OSR_SLOW }
    }
    fn factor(&self) -> f32 {
        FACTOR[COUNTS_MODEL as usize]
    }
    fn over(&self) -> f32 {
        OVER[COUNTS_MODEL as usize]
    }

    // ---------------------------------------------------------------- setup

    /// FUN_08012c30: quiesce, relays, 22 config bytes, INTE = 0.
    fn write_config(&mut self, clk: &mut Clock) {
        let m = self.mode;
        let r = self.range.max(0) as usize;
        let cfg: &[u8; hy::CFG_LEN] = match m {
            0 => &CFG_F0[r.min(3)],
            1 => &CFG_F1[r.min(3)],
            2 => &CFG_F2[r.min(1)],
            3 => &CFG_F3[r.min(1)],
            4 => &CFG_F4[r.min(1)],
            5 | 6 => &CFG_F5_6[r.min(1)],
            7 => &CFG_F7[r.min(1)],
            8 | 9 => &CFG_F8_9[r.min(1)],
            // r0 (60 ohm) always with the higher-current default variant.
            10 => {
                if self.ohm_var == 1 && r != 0 { &CFG_F10_P8SET[r.min(6)] } else { &CFG_F10_P8CLR[r.min(6)] }
            }
            11 => &CFG_F11[0],
            12 => &CFG_F12[0],
            13 => {
                if r == 7 { &CFG_F13_R7 } else { &CFG_F13[r.min(6)] }
            }
            14 => &CFG_F14[0],
            _ if self.t_sensor != 0 => &CFG_F10_P8CLR[r.clamp(1, 6)],
            _ => &CFG_F15[0],
        };
        self.avg.reset();
        self.t_rd = None;
        self.t_conv = None;
        hy::quiesce();
        let pa = if matches!(m, 0 | 1 | 4..=9) { 0 } else { 1 };
        // AC+DC: the stock pulses the relay on every FUN_08012c30 call, so
        // every phase switch has this 10 ms pause (AD1 / AD2 off) between the
        // quiesce and the new row (ACDC_RELAY_PULSE).
        if self.pa_now != Some(pa) || (ACDC_RELAY_PULSE && self.acdc) {
            relay_pa(clk, pa);
            self.pa_now = Some(pa);
        }
        relay_pe(match m {
            4 | 7 => 0,
            5 | 8 => 1,
            _ => 2,
        });
        pc7(m == 14);
        hy::configure(cfg);
        self.r22 = cfg[2];
        if m == 15 && self.t_sensor != 0 {
            // Resistive sensor: the stock in-range ohm OSR (32768) for a quiet
            // reading; the range is fixed, so no fast open-circuit switching.
            hy::write(0x22, &[0x17]);
            self.r22 = 0x17;
        }
        // AC+DC DC phase: the stock stays at the row's OSR 8192 (see
        // ACDC_STOCK_OSR and `acdc_osr`); every result after the settle
        // deadline is used, none dropped (docs/acdc-stock-vs-fwrs.md 8.2).
        if !ACDC_STOCK_OSR && self.acdc && matches!(m, 1 | 3 | 7..=9) {
            hy::write(0x22, &[0x17]);
            self.r22 = 0x17;
        }
        // AC+DC diagnostic: log the RMS results of an AC phase (plain AC V:
        // every 4 results as one entry, for comparison).
        self.diag_on = matches!(m, 0 | 2 | 4..=6) && (self.acdc || m == 0);
        if self.diag_on {
            self.diag_k = self.diag_k.wrapping_add(1);
            self.diag[self.diag_k as usize % ACDC_DIAG_N] = AcPhaseDiag::EMPTY;
            self.diag_t = clk.now_ms();
            self.diag_ctr = 0;
        }
    }

    /// AC+DC DC phase, ACDC_STOCK_OSR: the stock's OSR rule with its
    /// once-flag (FUN_08013734 cases 1, 3, 7..9): |AD1 - previous| <= `thr`
    /// -> R22 = 0x17 only if the flag is clear (then set); else R22 = 0x15 and
    /// the flag cleared. FUN_08012c30 does not touch the flag, so in steady
    /// AC+DC no 0x17 is written and every DC phase runs at the row's 0x15.
    fn acdc_osr(&mut self, ad1: i32, thr: i32) {
        if (ad1 - self.prev_ad1).abs() <= thr {
            if !self.acdc_osr_once {
                self.acdc_osr_once = true;
                hy::write(0x22, &[OSR_SLOW]);
                self.r22 = OSR_SLOW;
            }
        } else {
            self.acdc_osr_once = false;
            hy::write(0x22, &[OSR_FAST]);
            self.r22 = OSR_FAST;
        }
        self.prev_ad1 = ad1;
    }

    /// AC+DC diagnostic: the last AC phases (oldest first) and the number of
    /// AC phases started so far. The entry for the running phase may hold
    /// fewer than its 4 (V) / 3 (mV, A) results.
    pub fn acdc_diag(&self) -> (u32, [AcPhaseDiag; ACDC_DIAG_N]) {
        let mut d = [AcPhaseDiag::EMPTY; ACDC_DIAG_N];
        let n = (self.diag_k as usize).min(ACDC_DIAG_N);
        for (i, e) in d.iter_mut().take(n).enumerate() {
            *e = self.diag[(self.diag_k as usize + 1 + i + ACDC_DIAG_N - n) % ACDC_DIAG_N];
        }
        (self.diag_k, d)
    }

    /// `acdc_diag` as text for a debug query:
    /// `phases;late:sq/lpf/dcp/dt/intf,...;...` (oldest phase first, dcp with
    /// 4 decimals in the phase's unit, dt ms since the previous result, intf
    /// hex with a `*` when the frequency counter was restarted since the
    /// previous result), e.g. `41;2:9926/183/0.0891/802/1a,...`. Plain AC V
    /// logs every 4 results as one entry. Returns the length used; the
    /// oldest phases are left out when `buf` is too small.
    pub fn acdc_diag_text(&self, buf: &mut [u8]) -> usize {
        fn put(buf: &mut [u8], at: &mut usize, b: &[u8]) -> bool {
            let ok = *at + b.len() <= buf.len();
            if ok {
                buf[*at..*at + b.len()].copy_from_slice(b);
                *at += b.len();
            }
            ok
        }
        fn num(buf: &mut [u8], at: &mut usize, v: f32, dec: u8) -> bool {
            let mut t = [0u8; 16];
            put(buf, at, format(&mut t, v, dec).as_bytes())
        }
        fn hex(buf: &mut [u8], at: &mut usize, v: u16) -> bool {
            const H: &[u8; 16] = b"0123456789abcdef";
            let b = (v & 0xFF) as usize;
            put(buf, at, &[H[b >> 4], H[b & 15]]) && (v & 0x100 == 0 || put(buf, at, b"*"))
        }
        let (k, d) = self.acdc_diag();
        let n = d.iter().filter(|e| e.n > 0).count();
        for skip in 0..=n {
            let mut at = 0;
            let mut ok = num(buf, &mut at, k as f32, 0);
            for e in d.iter().filter(|e| e.n > 0).skip(skip) {
                ok = ok && put(buf, &mut at, b";") && num(buf, &mut at, e.late_ms as f32, 0) && put(buf, &mut at, b":");
                for j in 0..e.n as usize {
                    ok = ok
                        && (j == 0 || put(buf, &mut at, b","))
                        && num(buf, &mut at, e.sq[j] as f32, 0)
                        && put(buf, &mut at, b"/")
                        && num(buf, &mut at, e.lpf[j] as f32, 0)
                        && put(buf, &mut at, b"/")
                        && num(buf, &mut at, e.dcp[j].clamp(-1e9, 1e9), 4)
                        && put(buf, &mut at, b"/")
                        && num(buf, &mut at, e.dt[j] as f32, 0)
                        && put(buf, &mut at, b"/")
                        && hex(buf, &mut at, e.fl[j]);
                }
            }
            if ok {
                return at;
            }
        }
        0
    }

    fn set(&mut self, clk: &mut Clock, mode: u8, range: i8, settle: u64) {
        let changed_mode = mode != self.mode;
        self.mode = mode;
        self.range = range;
        self.write_config(clk);
        let now = clk.now_ms();
        self.deadline = now + settle;
        if changed_mode {
            self.entry = now;
            self.reset_values();
        }
    }

    fn reset_values(&mut self) {
        self.acdc_seen = 0;
        self.dc_val = 0.0;
        self.ac_raw = 0.0;
        self.f_ac = 0.0;
        self.f_lpf = 0.0;
        self.f_dc = 0.0;
        self.f_diode = 0.0;
        self.f_cap = 0.0;
        self.cap_c = 0.0;
        self.ohm_settled = false;
        self.beep_class = 4;
        self.t_phase = 0;
        self.t_votes = 0;
        self.freq.f_khz = 0.0;
        self.freq.duty = 0.0;
        self.freq.range = 0;
        self.ramp.first = true;
    }

    /// AC+DC phase switch: the other HY3131 mode of the same quantity, values
    /// kept (stock FUN_08012c30 without the m+5 change).
    fn switch_phase(&mut self, clk: &mut Clock, mode: u8, range: i8, settle: u64) {
        self.mode = mode;
        self.range = range;
        self.write_config(clk);
        self.deadline = clk.now_ms() + settle;
    }

    /// SCPI function code: the stock mode, or 16 = V AC+DC, 17 = A AC+DC.
    pub fn func(&self) -> u8 {
        match (self.acdc, self.mode) {
            (true, 0..=3) => 16,
            (true, _) => 17,
            (false, m) => m,
        }
    }

    /// V / A sub-mode (stock m+5): 0 DC, 1 AC, 2 AC+DC.
    pub fn sub(&self) -> u8 {
        if self.acdc {
            2
        } else {
            matches!(self.mode, 0 | 2 | 4..=6) as u8
        }
    }

    /// Set the V / A sub-mode (FUN_08013262 / FUN_08013134 cycle step, range
    /// page DC / AC / AC+DC buttons, BT command 06): auto range, V keeps the
    /// range (mV: r1), A keeps range % 2, values cleared (FUN_0803aad6).
    /// AC+DC starts in the DC phase, as the stock.
    pub fn set_sub(&mut self, clk: &mut Clock, sub: u8) {
        let m = self.mode;
        let (dc, r, t) = match m {
            0..=3 => (m | 1, if m >= 2 { 1 } else { self.range }, 450),
            4..=9 => (if m >= 7 { m } else { m + 3 }, self.range.max(0) % 2, 650),
            _ => return,
        };
        let ac = if dc <= 3 { dc - 1 } else { dc - 3 };
        self.manual = false;
        self.acdc = sub == 2;
        self.phase_n = 0;
        // The stock enters AC+DC from a steady DC reading with its OSR flag
        // set (doc section 1), so its DC phase never writes 0x17.
        self.acdc_osr_once = true;
        self.reset_values();
        self.set(clk, if sub == 1 { ac } else { dc }, r, t);
    }

    /// Enter a stock mode directly (SCPI, BT command 06). Auto range.
    /// 16 / 17 = V / A AC+DC (see `func`).
    pub fn select(&mut self, clk: &mut Clock, mode: u8) {
        if (32..32 + T_NAMES.len() as u8).contains(&mode) {
            // 32 + k: temperature with sensor k (SCPI CONF/MEAS:TEMP <type>).
            self.t_sensor = mode - 32;
            self.mode = 255;
            self.select(clk, 15);
            return;
        }
        if mode >= 16 {
            self.select(clk, if mode == 16 { 1 } else { 8 });
            self.set_sub(clk, 2);
            return;
        }
        self.acdc = false;
        self.manual = false;
        let (r, settle) = match mode {
            10 => (6, 2500),
            11 => (0, 200),
            12 => (0, 100),
            13 => (0, 200),
            14 => (0, 1000),
            15 => (self.t_range0(), 500),
            _ => (0, 450),
        };
        self.set(clk, mode.min(15), r, settle);
    }

    // ------------------------------------------------------ temperature API

    pub fn t_sensor(&self) -> u8 {
        self.t_sensor
    }

    /// Change the temperature sensor; re-enters mode 15 when it is active.
    pub fn set_t_sensor(&mut self, clk: &mut Clock, k: u8) {
        self.t_sensor = k.min(T_NAMES.len() as u8 - 1);
        if self.mode == 15 {
            self.mode = 255;
            self.select(clk, 15);
        }
    }

    /// Offset of sensor `k` in C (TC: applied to the cold junction).
    pub fn t_offs(&self, k: u8) -> f32 {
        self.t_offs[k as usize % 7] as f32 * 0.1
    }

    pub fn set_t_offs(&mut self, k: u8, c: f32) {
        self.t_offs[k as usize % 7] = (c.clamp(-20.0, 20.0) * 10.0 + if c < 0.0 { -0.5 } else { 0.5 }) as i16;
    }

    /// Persisted form: 7 offsets (0.1 C, i16) packed 2 per word.
    pub fn t_offs_raw(&self) -> [i16; 7] {
        self.t_offs
    }

    pub fn set_t_offs_raw(&mut self, o: [i16; 7]) {
        self.t_offs = o.map(|v| v.clamp(-200, 200));
    }

    pub fn ntc_user(&self) -> (f32, f32) {
        self.ntc_user
    }

    pub fn set_ntc_user(&mut self, r25: f32, beta: f32) {
        if r25.is_finite() && beta.is_finite() {
            self.ntc_user = (r25.clamp(100.0, 10.0e6), beta.clamp(1000.0, 10_000.0));
        }
    }

    /// Cold junction (HY3131 internal sensor, C incl. the TC offset) and its
    /// age in ms; None before the first thermocouple-mode measurement.
    pub fn t_cj(&self, now: u64) -> Option<(f32, u64)> {
        if self.t_cj_at == 0 { None } else { Some((self.t_cj, now - self.t_cj_at)) }
    }

    /// R25 / Beta of a resistive sensor, R0 for an RTD (sensor 1..6).
    fn t_params(&self) -> (f32, f32) {
        match self.t_sensor {
            1 => (100.0, 0.0),
            2 => (1000.0, 0.0),
            3 => (10_000.0, 3950.0),
            4 => (10_000.0, 3435.0),
            5 => (100_000.0, 3950.0),
            _ => self.ntc_user,
        }
    }

    /// Entry range of mode 15: TC r0; PT100 r1; PT1000 r2; NTC the lowest of
    /// r2..r6 whose full scale holds R25 (room temperature).
    fn t_range0(&self) -> i8 {
        match self.t_sensor {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => {
                let r25 = self.t_params().0;
                (2..6).find(|&r| r25 < 0.95e3 * self.factor() * OHM_DEC[r]).unwrap_or(6) as i8
            }
        }
    }

    /// Resistive sensor excitation estimate: (range, R ohm, I A, P W), with
    /// the inferred source T_VSRC behind Rref. None for the thermocouple.
    pub fn t_excitation(&self) -> Option<(i8, f32, f32, f32)> {
        if self.t_sensor == 0 {
            return None;
        }
        let r = self.range.clamp(1, 6);
        let i = T_VSRC / (OHM_RREF[r as usize] * 1e3 + self.t_res.max(0.0));
        Some((r, self.t_res, i, i * i * self.t_res.max(0.0)))
    }

    /// DMM analog rail off (true) / back on (false: the HY3131 configuration
    /// is rewritten, it may have lost power).
    pub fn set_suspended(&mut self, clk: &mut Clock, off: bool) {
        if off != self.suspended {
            self.suspended = off;
            if !off {
                self.start(clk);
            }
        }
    }

    pub fn suspended(&self) -> bool {
        self.suspended
    }

    /// Bottom button press (FUN_0803e108..e1a8 -> m+0xf..0x14, consumed by
    /// FUN_08013262, app_decompile.c:14575): enter the function, or cycle its
    /// sub-mode when it is already active. The in-function cycle is also what
    /// the stock does on a PD6 click while the touch lock is on (FUN_08013134).
    ///   V: DC -> AC -> AC+DC -> DC (`set_sub`), the range is kept, in mV it
    ///      becomes r1; from elsewhere DC V r0.
    ///   A: the same cycle in the same unit, range % 2; from elsewhere DC mA
    ///      (mode 8) with the old range % 2.
    ///   ohm: again toggles m+8 (range and manual kept); from elsewhere r6 auto.
    ///   cap: from elsewhere r0 auto; again does nothing.
    ///   cont/diode: diode first, then 11 <-> 12. Hz/temp: 14 first, 14 <-> 15.
    pub fn press(&mut self, clk: &mut Clock, b: Button) {
        let same = self.button() == b;
        let m = self.mode;
        if same && matches!(b, Button::Volt | Button::Amp) {
            // DC -> AC -> AC+DC -> DC (the stock skips AC+DC for model 0).
            let next = (self.sub() + 1) % 3;
            self.set_sub(clk, next);
            return;
        }
        if !same {
            self.acdc = false;
        }
        match b {
            Button::Volt => {
                self.manual = false;
                self.set(clk, 1, 0, 450);
            }
            Button::Amp => {
                self.manual = false;
                let r = self.range.max(0) % 2;
                self.set(clk, 8, r, 650);
            }
            Button::Ohm => {
                if same {
                    self.set_ohm_var(clk, self.ohm_var ^ 1);
                } else {
                    self.manual = false;
                    self.set(clk, 10, 6, 2500);
                }
            }
            Button::Cap => {
                if !same {
                    self.manual = false;
                    self.set(clk, 13, 0, 200);
                }
            }
            Button::ContDiode => {
                self.manual = false;
                let to = if same && m == 12 { 11 } else { 12 };
                self.set(clk, to, 0, if to == 11 { 200 } else { 100 });
            }
            Button::HzTemp => {
                self.manual = false;
                let to = if same && m == 14 { 15 } else { 14 };
                let r = if to == 15 { self.t_range0() } else { 0 };
                self.set(clk, to, r, if to == 14 { 1000 } else { 500 });
            }
        }
    }

    /// AUTO key (FUN_0803ddf4, app_decompile.c:59745-59777): back to auto
    /// range; only when the range was fixed (manual or REL, m+3 != 0): V in mV
    /// with AUTO (m+6 == 1) -> V r0, A with AUTO (m+7 == 1) -> mA r0.
    pub fn auto_key(&mut self, clk: &mut Clock) {
        let was_manual = self.manual;
        self.manual = false;
        if !was_manual {
            return;
        }
        match self.mode {
            2 | 3 if self.v_auto == 1 => self.set(clk, self.mode - 2, 0, 350),
            // The 60 ohm range is manual only: AUTO continues from 600 ohm.
            10 if self.range == 0 => self.set(clk, 10, 1, 150),
            // Stock: m+5 == 1 (AC) -> 5, else (DC, AC+DC) -> 8.
            4..=9 if self.a_auto == 1 => {
                if self.sub() == 1 {
                    self.set(clk, 5, 0, 650)
                } else {
                    self.set(clk, 8, 0, 350)
                }
            }
            _ => {}
        }
    }

    /// Freeze the current range (REL does this) or release it.
    pub fn hold_range(&mut self, manual: bool) {
        self.manual = manual;
    }

    /// Manual range from the MF popup: V items 600 mV, 6, 60, 600, 1000 V;
    /// A items 600 uA, 6000 uA, 60 mA, 600 mA, 6 A, 10 A; ohm 1..6
    /// (DM40C labels, COUNTS_MODEL), ohm 0 = the manual-only 60 ohm range.
    /// `None` = AUTO, `Some(255)` = AUTO+ (V/A only).
    pub fn set_range(&mut self, clk: &mut Clock, item: Option<u8>) {
        match (self.button(), item) {
            (Button::Volt, None) => {
                self.manual = false;
                self.v_auto = 3;
            }
            (Button::Volt, Some(255)) => {
                self.manual = false;
                self.v_auto = 1;
            }
            (Button::Amp, None) => {
                self.manual = false;
                self.a_auto = 3;
            }
            (Button::Amp, Some(255)) => {
                self.manual = false;
                self.a_auto = 1;
            }
            (Button::Volt, Some(i)) if i <= 4 => {
                let ac = self.sub() == 1;
                self.manual = true;
                if i == 0 {
                    self.set(clk, if ac { 2 } else { 3 }, 1, 450);
                } else {
                    self.set(clk, if ac { 0 } else { 1 }, i as i8 - 1, 450);
                }
            }
            (Button::Amp, Some(i)) if i <= 5 => {
                let ac = self.sub() == 1;
                let unit = 2 - (i / 2); // 0 A, 1 mA, 2 uA
                let m = if ac { 4 } else { 7 } + unit;
                self.manual = true;
                self.set(clk, m, (i % 2) as i8, 650);
            }
            (Button::Ohm, None) => {
                self.manual = false;
                if self.mode == 10 && self.range == 0 {
                    self.set(clk, 10, 1, 150);
                }
            }
            (Button::Ohm, Some(i)) if i <= 6 => {
                self.manual = true;
                self.set(clk, 10, i as i8, 150);
            }
            _ => {}
        }
    }

    /// BT mode byte (bt_meas_v2_6.h section 2, forward FUN_0803c842):
    /// bits 0-2 group, 3-5 range field, 6-7 sub-mode.
    pub fn mode_byte(&self) -> u8 {
        let m = self.mode;
        match m {
            0..=3 => {
                let sub = self.sub();
                let r = if self.manual {
                    if m >= 2 { 0 } else { self.range as u8 + 1 }
                } else if self.v_auto == 1 {
                    6
                } else {
                    5
                };
                sub << 6 | r << 3
            }
            4..=9 => {
                let sub = self.sub();
                let unit = (m - 4) % 3; // 0 A, 1 mA, 2 uA
                let r = if self.manual {
                    [4, 2, 0][unit as usize] + self.range as u8
                } else if self.a_auto == 1 {
                    7
                } else {
                    6
                };
                sub << 6 | r << 3 | 1
            }
            10 => {
                // BT has no 60 ohm range: report it as 600 ohm.
                let r = if self.manual { self.range.max(1) as u8 - 1 } else { 6 };
                self.ohm_var << 6 | r << 3 | 2
            }
            11 => 0x44,
            12 => 0x04,
            13 => 0x03,
            14 => 0x05,
            _ => 0x45,
        }
    }

    /// BT command 06 (reverse, FUN_0803c6a8 + FUN_0803c3b4). Returns true if
    /// the function or sub-mode changed (the caller then clears HOLD/REL).
    pub fn apply_mode_byte(&mut self, clk: &mut Clock, b: u8) -> bool {
        let (g, r, s) = (b & 7, (b >> 3) & 7, b >> 6);
        let before = (self.button(), self.mode_byte() & 0xC0);
        match g {
            0 => {
                if self.button() != Button::Volt {
                    self.select(clk, 1);
                }
                self.bt_acdc(s);
                let ac = s == 1;
                if r <= 4 {
                    self.manual = true;
                    if r == 0 {
                        self.set(clk, if ac { 2 } else { 3 }, 1, 450);
                    } else {
                        self.set(clk, if ac { 0 } else { 1 }, r as i8 - 1, 450);
                    }
                } else {
                    self.manual = false;
                    if r == 5 {
                        self.v_auto = 3;
                    } else if r == 6 {
                        self.v_auto = 1;
                    }
                    let mv = self.mode >= 2;
                    let (m, rr) = if r == 6 && mv { (0, 0) } else { ((mv as u8) * 2, self.range) };
                    let m = m + (!ac) as u8;
                    if m != self.mode {
                        self.set(clk, m, rr, 450);
                    }
                }
            }
            1 => {
                if self.button() != Button::Amp {
                    self.select(clk, 8);
                }
                self.bt_acdc(s);
                let ac = s == 1;
                if r <= 5 {
                    self.manual = true;
                    let m = if ac { 6 } else { 9 } - r / 2;
                    self.set(clk, m, (r % 2) as i8, 650);
                } else {
                    self.manual = false;
                    if r == 6 {
                        self.a_auto = 3;
                    } else {
                        self.a_auto = 1;
                    }
                    let mut unit = (self.mode - 4) % 3;
                    let mut rr = self.range;
                    if r == 7 && unit == 2 {
                        unit = 1;
                        rr = 0;
                    }
                    let m = if ac { 4 } else { 7 } + unit;
                    if m != self.mode || rr != self.range {
                        self.set(clk, m, rr, 650);
                    }
                }
            }
            2 => {
                if self.mode != 10 {
                    self.select(clk, 10);
                }
                self.ohm_var = s & 1;
                if r == 6 {
                    self.manual = false;
                    let rr = self.range;
                    self.set(clk, 10, rr, 300);
                } else {
                    self.manual = true;
                    self.set(clk, 10, (r as i8 + 1).min(6), 150);
                }
            }
            3 => self.select(clk, 13),
            4 => self.select(clk, if s == 1 { 11 } else { 12 }),
            5 => self.select(clk, if s == 1 { 15 } else { 14 }),
            _ => {}
        }
        before != (self.button(), self.mode_byte() & 0xC0)
    }

    /// BT command 06 sub-mode 2 = AC+DC (page+9 = s; AC+DC uses the DC HY
    /// mode, which the caller then selects for s != 1).
    fn bt_acdc(&mut self, s: u8) {
        if s != 2 {
            self.diag_on = false;
        }
        if (s == 2) != self.acdc {
            self.acdc = s == 2;
            self.phase_n = 0;
            self.acdc_osr_once = true; // as set_sub
            self.reset_values();
        }
    }

    /// Anti-drop: pulse the latching V/A relay again into its current state.
    pub fn reassert_relay(&mut self, clk: &mut Clock) {
        if self.suspended {
            return;
        }
        if let Some(pa) = self.pa_now {
            relay_pa(clk, pa);
        }
    }

    /// (Re)start measuring in the current mode.
    pub fn start(&mut self, clk: &mut Clock) {
        let (m, r) = (self.mode, self.range);
        self.mode = 255;
        self.set(clk, m, r, 450);
    }

    // ----------------------------------------------------------------- poll

    /// Poll the converter (every ~10 ms). Returns a reading when one is ready.
    pub fn poll(&mut self, clk: &mut Clock) -> Option<Reading> {
        let now = clk.now_ms();
        if self.suspended {
            return None;
        }
        self.diode_beep(now);
        // AC+DC: the stock runs the frequency counter at the top of the ACV /
        // DCV / DCmV / DCA cases, before its settle check (ACDC_FREQ_RESTART;
        // the ACA phase does it after each result, see poll_ac).
        if ACDC_FREQ_RESTART && self.acdc && matches!(self.mode, 0 | 1 | 3 | 7..=9) {
            self.freq_secondary(now, 0xF0);
        }
        if now < self.deadline {
            return None;
        }
        self.t_poll = now;
        self.avg_n = 0;
        let r = match self.mode {
            0 | 2 | 4..=6 => self.poll_ac(clk, now),
            1 | 3 => self.poll_dcv(clk),
            7..=9 => self.poll_dca(clk),
            10 => self.poll_ohm(clk, now),
            11 => self.poll_cont(),
            12 => self.poll_diode(now),
            13 => self.poll_cap(clk, now),
            14 => self.poll_freq_mode(now),
            _ if self.t_sensor != 0 => self.poll_rtd(clk, now),
            _ => self.poll_temp(now),
        };
        if r.is_some() {
            ema_ms(&mut self.period_ms, &mut self.t_rd, now);
            self.rd_avg_n = self.avg_n;
        }
        r.map(|mut r| {
            self.seq = self.seq.wrapping_add(1);
            r.seq = self.seq;
            r.auto = !self.manual;
            r.auto_plus = (self.button() == Button::Volt && self.v_auto == 1)
                || (self.button() == Button::Amp && self.a_auto == 1);
            r.acdc = self.acdc;
            r.avg_n = self.avg_n;
            r
        })
    }

    fn reading(&self, main: Val, si: f32) -> Reading {
        Reading {
            mode: self.mode,
            range: self.range,
            main,
            aux1: None,
            aux2: None,
            si,
            auto: true,
            auto_plus: false,
            acdc: false,
            seq: 0,
            avg_n: 0,
        }
    }

    fn ad1(&mut self) -> Option<i32> {
        if hy::read1(hy::R_INTF) & hy::INTF_AD1 == 0 {
            return None;
        }
        let mut b = [0u8; 3];
        hy::read(hy::R_AD1, &mut b);
        ema_ms(&mut self.conv_ms, &mut self.t_conv, self.t_poll);
        Some(hy::s24(&b))
    }

    /// AD1 OSR switching: stable -> 0x17 (32768, written once), else `fast`.
    /// `rated` (DC V / A): the FR-05 rate applies; FAST then keeps OSR 8192.
    fn osr(&mut self, ad1: i32, thr: i32, fast: u8, rated: bool) {
        if rated && self.rate == Rate::Fast {
            if self.r22 != OSR_FAST {
                hy::write(0x22, &[OSR_FAST]);
                self.r22 = OSR_FAST;
                self.avg_skip = true;
            }
        } else if (ad1 - self.prev_ad1).abs() <= thr {
            if self.r22 != OSR_SLOW {
                hy::write(0x22, &[OSR_SLOW]);
                self.r22 = OSR_SLOW;
                self.avg_skip = true;
            }
        } else {
            hy::write(0x22, &[fast]);
            self.r22 = fast;
        }
        self.prev_ad1 = ad1;
    }

    /// Averaged ranges (see the +1 digit and FR-05 tables): (window,
    /// restart threshold in codes = ~2 counts, extra digit possible).
    fn avg_cfg(&self) -> Option<(usize, i32, bool)> {
        avg_cfg(self.rate, self.acdc, self.mode, self.range)
    }

    /// Code for the value formula: the moving average in averaged ranges
    /// (restarted by a moving-OSR conversion), else the raw code. Also
    /// returns whether the extra digit is shown (`ext_ok`).
    #[inline(never)]
    fn avg_code(&mut self, code: i32, slow: bool) -> (f32, bool) {
        match self.avg_cfg() {
            Some((n, thr, ext)) => {
                if !slow {
                    self.avg.reset();
                }
                let mean = self.avg.push(code, n, thr);
                self.avg_n = self.avg.n as u8;
                (mean, ext_ok(ext, self.avg_n))
            }
            None => (code as f32, false),
        }
    }

    fn poll_dcv(&mut self, clk: &mut Clock) -> Option<Reading> {
        let ad1 = self.ad1()?;
        // This conversion ran at the steady OSR of the rate (and not the first
        // one after the switch, which `osr` flags for the next reading).
        let slow = self.r22 == self.steady_osr() && !self.avg_skip;
        self.avg_skip = false;
        let r = self.range as usize;
        let (f, ov) = (self.factor(), self.over());
        let m = self.mode;
        let before = (m, self.range);
        // AC+DC: the stock's once-flag OSR rule (ACDC_STOCK_OSR), else the
        // OSR stays as write_config left it.
        let acdc = self.acdc;
        let (x, lim, dec, unit, si_k, ext) = if m == 1 {
            if !acdc {
                self.osr(ad1, 0x206, 0x15, true);
            } else if ACDC_STOCK_OSR {
                self.acdc_osr(ad1, 0x206);
            }
            let (code, ext) = self.avg_code(ad1, slow);
            let mut v = code * K_ACV[r] * (1.25 / 0.9) / AD1_DIV * cal(0x04 + 4 * r as u32, 1.0)
                - cal(0x14 + 4 * r as u32, 0.0);
            if v < 0.0 {
                v *= DCV_NEGK[r];
            }
            if r == 0 && v.abs() / 20000.0 > 3e-5 {
                v += 1e-4 - v.abs() / 20000.0;
            }
            (v, (f * DEC4[r]).min(1000.0), 4 - r as u8, "V", 1.0, ext)
        } else {
            if !acdc {
                self.osr(ad1, 0xE22 - 0x960 * r as i32, 0x15, true);
            } else if ACDC_STOCK_OSR {
                self.acdc_osr(ad1, 0xE22 - 0x960 * r as i32);
            }
            let (code, ext) = self.avg_code(ad1, slow);
            let mv = code * (1.25 / 0.9) / MV_DIV[r] / (AD1_DIV / 1000.0) * cal(0x24 + 4 * r as u32, 1.0)
                - cal(0x2C + 4 * r as u32, 0.0);
            (mv, f * [10.0, 100.0][r], 3 - r as u8, "mV", 1e-3, ext)
        };
        // Autorange magnitude S+0x60: |DC|, AC+DC: hypot(AC raw, DC).
        let mag = if self.acdc { hypot(self.ac_raw, x) } else { x.abs() };
        if !self.manual {
            let up = mag > ov * lim;
            if m == 1 {
                if up && r < 3 {
                    self.set(clk, 1, r as i8 + 1, 250);
                } else if mag < 0.1 * lim / ov {
                    if r > 0 {
                        self.set(clk, 1, r as i8 - 1, 250);
                    } else if self.v_auto & 2 != 0 {
                        self.set(clk, 3, 1, 450);
                    }
                }
            } else if up {
                if r == 0 {
                    self.set(clk, 3, 1, 250);
                } else if self.v_auto & 1 != 0 {
                    self.set(clk, 1, 0, 350);
                }
            }
        }
        self.dc_out(clk, x, lim, dec, unit, si_k, before, ext)
    }

    /// End of a DC reading (modes 1, 3, 7..9): plain DC, or the DC phase of
    /// AC+DC (FUN_08013734 cases 1, 3, 7..9): shown = hypot(AC filtered, DC),
    /// aux1/aux2 = AC/DC parts; after S+10 + 1 readings back to the AC mode
    /// (DCV / DCmV 800 ms, DCA 900 ms settle) with S+10 = 3 (ACV: 4 results)
    /// or 2 (ACmV, ACA: 3 results). A range / unit
    /// change in AC+DC gives no value this time (stock: OL, m+0xd8 = 0).
    /// `ext`: the value is averaged and shows the extra digit (not in AC+DC).
    #[allow(clippy::too_many_arguments)]
    fn dc_out(
        &mut self,
        clk: &mut Clock,
        x: f32,
        lim: f32,
        dec: u8,
        unit: &'static str,
        si_k: f32,
        before: (u8, i8),
        ext: bool,
    ) -> Option<Reading> {
        let ov = self.over();
        if !self.acdc {
            let ol = x.abs() / lim > ov;
            let v = if ol { Val::ol(dec, unit) } else { Val::new(x, dec, unit).with_ext(ext) };
            return Some(self.reading(v, x * si_k));
        }
        if (self.mode, self.range) != before {
            return None;
        }
        self.dc_val = x;
        self.acdc_seen |= 2;
        let t = hypot(self.f_ac, x);
        let ol = t / lim > ov;
        let mut rd = self.reading(if ol { Val::ol(dec, unit) } else { Val::new(t, dec, unit) }, t * si_k);
        rd.aux1 = Some(part(self.f_ac, lim, dec, unit));
        rd.aux2 = Some(part(x, lim, dec, unit));
        if self.phase_n == 0 {
            let (to, settle, n) = match self.mode {
                1 => (0, 800, 3),
                3 => (2, 800, 2),
                dm => (dm - 3, 900, 2),
            };
            let r = self.range;
            self.switch_phase(clk, to, r, settle);
            self.phase_n = n;
        } else {
            self.phase_n -= 1;
        }
        (self.acdc_seen == 3).then_some(rd)
    }

    fn poll_dca(&mut self, clk: &mut Clock) -> Option<Reading> {
        let ad1 = self.ad1()?;
        // As in poll_dcv: a conversion at the steady OSR may join the window.
        let slow = self.r22 == self.steady_osr() && !self.avg_skip;
        self.avg_skip = false;
        if !self.acdc {
            self.osr(ad1, 0x168, 0x15, true);
        } else if ACDC_STOCK_OSR {
            self.acdc_osr(ad1, 0x168);
        }
        // FR-05 SLOW only (MEDIUM: no window in A, as the stock).
        let (code, _) = self.avg_code(ad1, slow);
        let (m, r) = (self.mode, self.range as usize);
        let before = (m, self.range);
        let u = (m - 7) as usize; // 0 A, 1 mA, 2 uA
        let i = u * 2 + r;
        let raw = code * 1.25 / (SHUNT[u] * MV_DIV[r] * 0.9) / AD1_DIV * cal(0x94 + 4 * i as u32, 1.0)
            - cal(0xAC + 4 * i as u32, 0.0);
        let mut a = raw;
        if m == 7 && r == 1 {
            ema(0.0007, 0.15, a, &mut self.f_dc);
            a = self.f_dc;
        }
        let lim = (self.factor() * DEC4[u + r]).min(if m == 7 { 10.0 } else { f32::MAX });
        let ov = self.over();
        let mag = if self.acdc { hypot(self.ac_raw, raw) } else { a.abs() };
        if !self.manual {
            if mag > ov * lim {
                if r == 0 {
                    self.set(clk, m, 1, 250);
                } else if m > 7 && self.a_auto & 1 != 0 {
                    self.set(clk, m - 1, 1, 250);
                }
            } else if mag < 0.1 * lim / ov {
                if r == 1 {
                    self.set(clk, m, 0, 250);
                } else if m < if self.a_auto == 3 { 9 } else { 8 } {
                    self.set(clk, m + 1, 0, 350);
                }
            }
        }
        let (unit, dec, si_k) = match u {
            0 => ("A", 4 - r as u8, 1.0),
            1 => ("mA", 3 - r as u8, 1e-3),
            _ => ("\u{b5}A", 2 - r as u8, 1e-6),
        };
        let mut rd = self.dc_out(clk, a, lim, dec, unit, si_k, before, false)?;
        if m == 8 && !self.acdc {
            // 4-20 mA loop percentage.
            let p = (a.abs() - 4.0) * 6.25;
            rd.aux2 = Some(if p > 125.0 { Val::ol(1, "%") } else { Val::new(p, 1, "%") });
        }
        Some(rd)
    }

    /// AC+DC reading during the AC phase: hypot(AC filtered, DC part), parts
    /// in aux1 / aux2, reported with the DC mode; none before both parts
    /// were measured.
    fn acdc_ac_reading(&self, lim: f32, dec: u8, unit: &'static str, si_k: f32) -> Option<Reading> {
        if self.acdc_seen != 3 {
            return None;
        }
        let m = self.mode;
        let t = hypot(self.f_ac, self.dc_val);
        let ol = t / lim > self.over();
        let mut rd = self.reading(if ol { Val::ol(dec, unit) } else { Val::new(t, dec, unit) }, t * si_k);
        rd.mode = if m <= 2 { m + 1 } else { m + 3 };
        rd.aux1 = Some(part(self.f_ac, lim, dec, unit));
        rd.aux2 = Some(part(self.dc_val, lim, dec, unit));
        Some(rd)
    }

    fn poll_ac(&mut self, clk: &mut Clock, now: u64) -> Option<Reading> {
        let m = self.mode;
        let need = if m <= 2 { hy::INTF_RMS | hy::INTF_LPF } else { hy::INTF_RMS };
        let intf = hy::read1(hy::R_INTF);
        if intf & need != need {
            return None;
        }
        let mut b = [0u8; 20];
        hy::read(hy::R_AD1, &mut b);
        let rms = (b[9] as u64) | (b[10] as u64) << 8 | (b[11] as u64) << 16 | (b[12] as u64) << 24
            | ((b[13] & 0x1F) as u64) << 32;
        let sq = sqrt(rms as f32);
        if sq > 262_144.0 {
            return None;
        }
        let lpf = (((b[6] as u32) << 13 | (b[7] as u32) << 21 | (b[8] as u32) << 29) as i32 >> 13) as f32;
        let r = self.range as usize;
        let (f, ov) = (self.factor(), self.over());
        let (s, ci, g_rms, o_rms, g_lpf, o_lpf) = match m {
            0 => (K_ACV[r] * 1.25 / 262_144.0, r, 0x34, 0x44, 0x64, 0x74),
            2 => (1.25 / MV_DIV[r] / 262.144, r, 0x54, 0x5C, 0x84, 0x8C),
            _ => {
                let u = (m - 4) as usize;
                (1.25 / (SHUNT[u] * MV_DIV[r]) / 262_144.0, u * 2 + r, 0xC4, 0xDC, 0xF4, 0x10C)
            }
        };
        let ci = ci as u32;
        let mut ac = sq * s * cal(g_rms + 4 * ci, 1.0) - cal(o_rms + 4 * ci, 0.0);
        let dcp = lpf * s * cal(g_lpf + 4 * ci, 1.0) - cal(o_lpf + 4 * ci, 0.0);
        if self.diag_on {
            if !self.acdc && self.diag[self.diag_k as usize % ACDC_DIAG_N].n >= 4 {
                self.diag_k = self.diag_k.wrapping_add(1);
                self.diag[self.diag_k as usize % ACDC_DIAG_N] = AcPhaseDiag::EMPTY;
            }
            let e = &mut self.diag[self.diag_k as usize % ACDC_DIAG_N];
            let j = e.n as usize;
            if j == 0 {
                e.late_ms = now.saturating_sub(self.deadline).min(u16::MAX as u64) as u16;
            }
            if j < 4 {
                let t0 = self.diag_t;
                e.sq[j] = sq as u32;
                e.lpf[j] = lpf as i32;
                e.dcp[j] = dcp;
                e.dt[j] = now.saturating_sub(t0).min(u16::MAX as u64) as u16;
                e.fl[j] = intf as u16 | if self.diag_ctr > 0 { 0x100 } else { 0 };
                e.n += 1;
            }
            self.diag_t = now;
            self.diag_ctr = 0;
        }
        if ac < 0.0 {
            ac = 0.0;
        }
        let (thr, a_rms, a_lpf) = match m {
            0 => (ACV_WIN[r], 0.25, 0.25),
            2 => (ACMV_WIN[r], 0.35, 0.35),
            _ => (ACA_WIN[(m - 4) as usize * 2 + r], 0.15, 0.35),
        };
        ema(thr, a_lpf, dcp, &mut self.f_lpf);
        let lim = match m {
            0 => (f * DEC4[r]).min(1000.0),
            2 => f * [10.0, 100.0][r],
            _ => (f * DEC4[(m - 4) as usize + r]).min(if m == 4 { 10.0 } else { f32::MAX }),
        };
        let (unit, dec, si_k) = match m {
            0 => ("V", 4 - r as u8, 1.0),
            2 => ("mV", 3 - r as u8, 1e-3),
            4 => ("A", 4 - r as u8, 1.0),
            5 => ("mA", 3 - r as u8, 1e-3),
            _ => ("\u{b5}A", 2 - r as u8, 1e-6),
        };
        let acdc = self.acdc;
        // AC+DC: every result after the settle deadline goes through the EMA
        // (thr = the plain-AC window, a 0.25 V / 0.35 mV / 0.15 A), as the
        // stock; nothing dropped or gated (docs/acdc-stock-vs-fwrs.md 8.2).
        self.ac_raw = ac;
        ema(thr, a_rms, ac, &mut self.f_ac);

        // AC+DC: hypot(AC, last DC part) for every quantity.
        let mag = if acdc {
            hypot(ac, self.dc_val)
        } else if m <= 2 {
            ac
        } else {
            sqrt(ac * ac + 0.25 * dcp * dcp)
        };
        let before = (m, self.range);
        if !self.manual {
            let up = mag > ov * lim;
            let down = mag < 0.1 * lim / ov;
            match m {
                // AC+DC: a V / mV range change is applied through the DC
                // phase (600 ms, S+10 = 2); unit changes go to the DC mode.
                0 => {
                    if (up && r < 3) || (down && r > 0) {
                        let to = if up { r as i8 + 1 } else { r as i8 - 1 };
                        if acdc {
                            self.switch_phase(clk, 1, to, 600);
                            self.phase_n = 2;
                        } else {
                            self.set(clk, 0, to, 450);
                        }
                    } else if down && self.v_auto & 2 != 0 {
                        self.set(clk, if acdc { 3 } else { 2 }, 1, 600);
                    }
                }
                2 => {
                    if up && r == 0 {
                        if acdc {
                            self.switch_phase(clk, 3, 1, 600);
                            self.phase_n = 2;
                        } else {
                            self.set(clk, 2, 1, 450);
                        }
                    } else if up && self.v_auto & 1 != 0 {
                        self.set(clk, acdc as u8, 0, 350);
                    }
                }
                _ => {
                    if up && r == 0 {
                        self.set(clk, m, 1, 550);
                    } else if up && m > 4 && self.a_auto & 1 != 0 {
                        self.set(clk, m - 1, 0, 650);
                    } else if down && r == 1 {
                        self.set(clk, m, 0, 550);
                    } else if down && r == 0 && m < if self.a_auto == 3 { 6 } else { 5 } {
                        self.set(clk, m + 1, 1, 650);
                    }
                }
            }
        }
        if acdc {
            // AC phase of AC+DC (FUN_08013734 cases 0, 2, 4..6): shown =
            // hypot(AC filtered, DC part) on every result. After S+10 + 1
            // results (ACV 4, ACmV / ACA 3) to the DC mode (ACV 600 ms + R32 =
            // 0x38, ACmV 500 ms, ACA 700 ms) for 3 results (S+10 = 2; the DC
            // part is not averaged in AC+DC, the last one is shown).
            if (self.mode, self.range) != before {
                return None;
            }
            self.acdc_seen |= 1;
            let rd = self.acdc_ac_reading(lim, dec, unit, si_k);
            if ACDC_FREQ_RESTART && m >= 4 {
                self.freq_secondary(now, 0xF0);
            }
            if self.phase_n == 0 {
                let r = self.range;
                self.switch_phase(clk, if m <= 2 { m + 1 } else { m + 3 }, r, match m {
                    0 => 600,
                    2 => 500,
                    _ => 700,
                });
                if m == 0 {
                    hy::write(0x32, &[0x38]);
                }
                self.phase_n = 2;
            } else {
                self.phase_n -= 1;
            }
            return rd;
        }
        let ol = self.f_ac / lim > ov;
        let v = self.f_ac;
        let mut rd = self.reading(if ol { Val::ol(dec, unit) } else { Val::new(v, dec, unit) }, v * si_k);
        self.freq_secondary(now, 0xF0);
        let (fv, dv) = self.freq_vals(true);
        rd.aux1 = Some(fv);
        rd.aux2 = Some(dv);
        Some(rd)
    }

    fn poll_ohm(&mut self, clk: &mut Clock, now: u64) -> Option<Reading> {
        let ad1 = self.ad1()?;
        // r0 (60 ohm) only by a manual choice (set_range); autorange r1..r6.
        let r = self.range.clamp(0, 6) as usize;
        let r22 = hy::read1(0x22);
        let steady = self.steady_osr();
        let slow = r22 == steady && !self.avg_skip;
        self.avg_skip = false;
        let (code, ext) = self.avg_code(ad1.unsigned_abs() as i32, slow);
        let mut kohm = code * cal(0x124 + 4 * r as u32, 1.0) * OHM_RREF[r] / 0.9 / AD1_DIV - cal(0x144, 0.0);
        if kohm < 0.0 {
            kohm = 0.0;
        }
        let lim = self.factor() * OHM_DEC[r];
        let ov = self.over();
        let ol = kohm / lim > ov;
        if ol {
            if r22 != 0x13 {
                hy::write(0x22, &[0x13]);
            }
        } else if r22 != steady {
            hy::write(0x22, &[steady]);
            self.deadline = now + 150;
            self.avg_skip = true;
        }
        if !self.manual {
            let q = ov * lim / (kohm + 1e-5);
            if q < 1.0 {
                if r < 6 {
                    let to = if self.ohm_settled { 6 } else { r as i8 + 1 };
                    self.ohm_settled = false;
                    self.set(clk, 10, to, 150);
                    return None;
                }
            } else if q > 10.0 * ov * ov && r > 1 {
                // Down below 0.1 * lim / over, as V / A. The stock tests
                // q > 10 (R < over * lim / 10), which is exactly the next lower
                // range's up threshold: no hysteresis band at all.
                let step = if now < self.entry + 2500 {
                    1
                } else if q > 1e5 {
                    5
                } else if q > 1e4 {
                    4
                } else if q > 1e3 {
                    3
                } else if q > 100.0 {
                    2
                } else {
                    1
                };
                self.ohm_settled = false;
                self.set(clk, 10, (r as i8 - step).max(1), 150);
                return None;
            } else if !self.ohm_settled {
                self.ohm_settled = true;
                self.deadline = now + 150;
                return None;
            }
        }
        // Stock formats (r < 2: x1000 ohm, fmt r + 1): r0 60.000 ohm.
        let (v, dec, unit) = match r {
            0 | 1 => (kohm * 1000.0, 3 - r as u8, "\u{3a9}"),
            2..=4 => (kohm, 6 - r as u8, "k\u{3a9}"),
            _ => (kohm * 1e-3, 9 - r as u8, "M\u{3a9}"),
        };
        let val = if ol { Val::ol(dec, unit) } else { Val::new(v, dec, unit).with_ext(ext) };
        Some(self.reading(val, kohm * 1e3))
    }

    fn poll_cont(&mut self) -> Option<Reading> {
        let ad1 = self.ad1()?;
        self.osr(ad1, 0x206, 0x14, false);
        // Negative results (a source, a diode the wrong way or a charged
        // capacitor on the jacks) clamp to 0 as the stock (dmm_v2_6.h m+0x18:
        // only its raw mode skips the clamp); emulator cross table.
        let ohm = (-(ad1 as f32) * cal(0x15C, 1.0) / 0.9 / (AD1_DIV / 1000.0) - cal(0x160, 0.0)).max(0.0);
        let ol = ohm > 1000.0;
        Some(self.reading(if ol { Val::ol(2, "\u{3a9}") } else { Val::new(ohm, 2, "\u{3a9}") }, ohm))
    }

    fn poll_diode(&mut self, now: u64) -> Option<Reading> {
        let ad1 = self.ad1()?;
        let g = cal(0x164, 1.0);
        let mut v = ad1 as f32 * g * 1.25 * 10.0 / 0.9 / AD1_DIV - cal(0x168, 0.0);
        if v < 0.0 {
            v = 0.0;
        }
        ema(0.0012, 0.05, v, &mut self.f_diode);
        let v = self.f_diode;
        let ol = v / 3.0 > 1.04;
        let class = if v < 0.022 {
            0
        } else if v < 0.45 {
            1
        } else if v < 0.75 {
            2
        } else if v < 1.5 {
            3
        } else {
            4
        };
        if class != self.beep_class {
            self.beep_class = class;
            self.beep_e = 0;
            self.beep_n = 0;
            self.beep_next = now;
        } else {
            self.beep_n = self.beep_n.saturating_add(1);
        }
        let mut rd = self.reading(if ol { Val::ol(4, "V") } else { Val::new(v, 4, "V") }, v);
        // Aux "ohm" readout (stock m+0xdc = V*gain/0.765, shown only below 1.04).
        let k = v * g / 0.765;
        rd.aux2 = Some(if k < 1.04 { Val::new(k * 1000.0, 1, "\u{3a9}") } else { Val::ol(1, "\u{3a9}") });
        Some(rd)
    }

    /// Diode beep patterns through the HY3131 CMP pin buffer (R20 0xb4 / 0xb0).
    fn diode_beep(&mut self, now: u64) {
        if self.mode != 12 || now < self.beep_next {
            return;
        }
        let (on, step, max_e) = match self.beep_class {
            0 => (true, 50, 0),
            1 if self.beep_n <= 6 => (self.beep_e & 1 == 0, 50, 4),
            2 if self.beep_n <= 6 => (self.beep_e & 1 == 0, 100, 2),
            3 if self.beep_n <= 6 => (self.beep_e & 1 != 0, 25, 7),
            _ => (false, 100, 0),
        };
        hy::write(0x20, &[if on { 0xB4 } else { 0xB0 }]);
        if self.beep_e < max_e {
            self.beep_e += 1;
        }
        self.beep_next = now + step;
    }

    fn read_counters(&mut self, src: u8) -> (f32, f32) {
        let mut b = [0u8; 9];
        hy::read(0x15, &mut b);
        self.diag_ctr = self.diag_ctr.saturating_add(1);
        hy::write(0x20, &[src]);
        hy::write(0x1B, &[0x00, 0x00, 0xFC]);
        hy::write(0x20, &[src | 2]);
        counter_vals(u24(&b[0..3]), u24(&b[3..6]), u24(&b[6..9]))
    }

    fn freq_range(&mut self) {
        self.freq.range = freq_range_for(self.freq.f_khz, self.factor() * self.over(), self.freq.range);
    }

    /// FUN_08013450 as the secondary readout in V/A modes, every 350 ms.
    fn freq_secondary(&mut self, now: u64, src: u8) {
        if now < self.freq.next {
            return;
        }
        self.freq.next = now + 350;
        let (f, d) = self.read_counters(src);
        self.freq.f_khz = f;
        self.freq.duty = d;
        self.freq_range();
    }

    /// Frequency / duty readouts. `aux`: the secondary format of V/A modes.
    fn freq_vals(&self, aux: bool) -> (Val, Val) {
        (freq_val(self.freq.f_khz, self.freq.range, aux), Val::new(self.freq.duty, 1, "%"))
    }

    fn poll_freq_mode(&mut self, now: u64) -> Option<Reading> {
        if now < self.freq.next {
            return None;
        }
        self.freq.next = now + 350;
        // The counter just read was gated with the source selected last time;
        // restart it with the other one (0xf0 comparator / 0x08 ENCNTI).
        let i = self.freq.src_alt as usize;
        let next_src = if self.freq.src_alt { 0xF0 } else { 0x08 };
        let (f, d) = self.read_counters(next_src);
        let ch = if f == 0.0 { 1e6 } else { (f - self.freq.last[i]).abs() };
        self.freq.change[i] = ch;
        self.freq.last[i] = f;
        self.freq.src_alt = !self.freq.src_alt;
        let best = if self.freq.change[0] <= self.freq.change[1] { 0 } else { 1 };
        self.freq.f_khz = self.freq.last[best];
        if best == i {
            self.freq.duty = d;
        }
        if !self.manual {
            self.freq_range();
        }
        let r = self.freq.range as usize;
        let fs = FREQ_DEC[r] * self.factor();
        let ol = self.freq.f_khz / fs > self.over();
        let (fv, dv) = self.freq_vals(false);
        let mut rd = self.reading(if ol { Val::ol(fv.dec, fv.unit) } else { fv }, self.freq.f_khz * 1e3);
        rd.range = r as i8;
        rd.aux2 = Some(dv);
        Some(rd)
    }

    fn poll_temp(&mut self, now: u64) -> Option<Reading> {
        let ad1 = self.ad1()?;
        match self.t_phase {
            0 => {
                self.t_code_a = ad1;
                hy::write(0x33, &[0xAE]);
                self.t_phase = 1;
                self.deadline = now + 700;
                None
            }
            1 => {
                // User offset of the thermocouple sensor on the cold junction:
                // T = Tcj - 26 + T_tc(Vtc), so it shifts T by the same amount
                // and also corrects the CJ readout (community: the internal
                // sensor itself reads 3.6..6 C low on early units).
                self.t_cj = (self.t_code_a - ad1) as f32 / 640_000.0 * 314.15 - 289.15 + cal(0x194, 0.0) + self.t_offs(0);
                self.t_cj_at = now.max(1);
                hy::write(0x33, &[0xAC]);
                self.t_phase = 2;
                self.deadline = now + 800;
                None
            }
            _ => {
                let vtc = ad1 as f32 * cal(0x24, 1.0) * 1.25 / 0.9 * 100.0 / AD1_DIV - cal(0x2C, 0.0);
                let mut i = 1;
                while i < 14 && TC_K_MV[i + 1] < vtc {
                    i += 1;
                }
                let t_tc = (i as f32 - 2.0 + (vtc - TC_K_MV[i]) / (TC_K_MV[i + 1] - TC_K_MV[i])) * 100.0;
                let t = self.t_cj - 26.0 + t_tc;
                hy::write(0x33, &[0xAF]);
                self.t_phase = 0;
                self.deadline = now + 700;
                let ol = !(-40.0..=1000.0).contains(&t);
                let mut rd = self.reading(if ol { Val::ol(1, "\u{b0}C") } else { Val::new(t, 1, "\u{b0}C") }, t);
                rd.aux1 = Some(Val::new(self.t_cj, 1, "\u{b0}C"));
                rd.aux2 = Some(if ol { Val::ol(1, "\u{b0}F") } else { Val::new(32.0 + t * 1.8, 1, "\u{b0}F") });
                Some(rd)
            }
        }
    }

    /// Resistive temperature sensor on the ohm path (see the T_NAMES notes).
    fn poll_rtd(&mut self, clk: &mut Clock, now: u64) -> Option<Reading> {
        let ad1 = self.ad1()?;
        let r = self.range.clamp(1, 6) as usize;
        let ohm = ((ad1.unsigned_abs() as f32) * cal(0x124 + 4 * r as u32, 1.0) * OHM_RREF[r] / 0.9 / AD1_DIV
            - cal(0x144, 0.0))
            * 1e3;
        self.t_res = ohm;
        let lim = self.factor() * OHM_DEC[r] * 1e3;
        let (r0, beta) = self.t_params();
        if self.t_sensor >= 3 {
            // NTC range policy: up at 95 % of full scale (before OL at 103 %),
            // down below 8.5 % (= 85 % of the lower range), two readings in a
            // row; r2..r6. Every candidate range keeps the sensor power far
            // below 0.1 mW, so the choice is about resolution (t_excitation).
            let to = if ohm > 0.95 * lim && r < 6 {
                r + 1
            } else if ohm < 0.085 * lim && r > 2 {
                r - 1
            } else {
                r
            };
            if to != r {
                self.t_votes += 1;
                if self.t_votes >= 2 {
                    self.t_votes = 0;
                    self.range = to as i8;
                    self.write_config(clk);
                    self.deadline = now + 300;
                    return None;
                }
            } else {
                self.t_votes = 0;
            }
        }
        let t = if ohm > self.over() * lim {
            None
        } else if self.t_sensor <= 2 {
            pt_temp(ohm / r0)
        } else {
            ntc_temp(ohm, r0, beta)
        };
        let t = t.map(|t| t + self.t_offs(self.t_sensor));
        if let Some(t) = t {
            if self.t_phase == 0 {
                self.t_phase = 1;
                self.t_f = t;
            } else {
                ema(0.5, 0.3, t, &mut self.t_f);
            }
        } else {
            self.t_phase = 0;
        }
        let tv = self.t_f;
        let c = "\u{b0}C";
        let mut rd = self.reading(if t.is_some() { Val::new(tv, 2, c) } else { Val::ol(2, c) }, tv);
        rd.aux1 = Some(if ohm > self.over() * lim {
            Val::ol(2, "\u{3a9}")
        } else if ohm < 1e3 {
            Val::new(ohm, 2, "\u{3a9}")
        } else if ohm < 1e6 {
            Val::new(ohm * 1e-3, 3, "k\u{3a9}")
        } else {
            Val::new(ohm * 1e-6, 3, "M\u{3a9}")
        });
        rd.aux2 = Some(if t.is_some() { Val::new(32.0 + tv * 1.8, 2, "\u{b0}F") } else { Val::ol(2, "\u{b0}F") });
        Some(rd)
    }

    fn cap_val(&self, c_uf: f32, ol: bool) -> Reading {
        let r = self.range as usize;
        let (v, dec, unit) = match r {
            0..=2 => (c_uf * 1000.0, 3 - r as u8, "nF"),
            3..=5 => (c_uf, 6 - r as u8, "\u{b5}F"),
            _ => (c_uf * 1e-3, 9 - r as u8, "mF"),
        };
        let dec = dec.min(3);
        self.reading(if ol { Val::ol(dec, unit) } else { Val::new(v, dec, unit) }, c_uf * 1e-6)
    }

    fn cap_set_range(&mut self, clk: &mut Clock, r: i8) {
        let now = clk.now_ms();
        self.range = r;
        if r >= 5 {
            // Discharge first, then start the ramp when |V| < 30 mV.
            hy::quiesce();
            hy::configure(&CFG_F13_DISCHARGE);
            self.ramp.discharging = true;
            self.ramp.n = 0;
            self.ramp.med_i = 0;
            self.ramp.class = 0;
            self.ramp.cmp = None;
            self.ramp.first = true;
            self.deadline = now + 255;
        } else {
            self.write_config(clk);
            self.deadline = now + if r < 4 { 120 } else { 320 };
        }
        self.cap_cnt_up = 0;
        self.cap_cnt_dn = 0;
        self.cap_cnt_small = 0;
    }

    fn poll_cap(&mut self, clk: &mut Clock, now: u64) -> Option<Reading> {
        let r = self.range.max(0) as usize;
        let (f_dec, ov) = (self.factor(), self.over());
        let lim = f_dec * CAP_DEC[r];
        if r < 5 {
            let (f, _) = self.read_counters(0x50);
            self.deadline = now + 420;
            ema(CAP_WIN[r], 0.05, f, &mut self.f_cap);
            let fk = self.f_cap.max(1e-9);
            // Negative results are clamped to 0 as in the stock (m+0x18 == 0).
            let c = (cal(0x170 + 4 * r as u32, 1.0) * 0.72135 / (CAP_K[r] * fk) - cal(0x190, 0.0)).clamp(0.0, 1.06 * lim);
            let ol = c / lim > ov;
            if !self.manual {
                if r >= 1 && self.f_cap > 2.0 {
                    self.cap_cnt_small += 1;
                    if r < 4 || self.cap_cnt_small >= 4 {
                        self.cap_set_range(clk, 0);
                        return None;
                    }
                } else {
                    self.cap_cnt_small = 0;
                }
                if c > ov * lim {
                    self.cap_set_range(clk, r as i8 + 1);
                    return Some(self.cap_val(0.0, true));
                }
            }
            if !ol {
                self.cap_c = c;
            }
            return Some(self.cap_val(self.cap_c, ol));
        }

        // r5..r7: constant-current ramp on AD1.
        let ad1 = self.ad1()?;
        let v_mv = ad1 as f32 * 1000.0 * 1.25 / 0.9 / AD1_DIV;
        if self.ramp.discharging {
            if v_mv.abs() < 30.0 {
                self.ramp.discharging = false;
                self.write_config(clk);
                self.ramp.prev_t = now;
                self.ramp.prev_v = v_mv;
            }
            self.deadline = now + 255;
            return None;
        }
        let _ = self.read_counters(0x50);
        let dv = (self.ramp.prev_v - v_mv).abs();
        let dt = now.saturating_sub(self.ramp.prev_t) as f32;
        self.ramp.prev_v = v_mv;
        self.ramp.prev_t = now;
        self.deadline = now + 255;
        if dv == 0.0 {
            return None;
        }
        if self.ramp.med_i == 0 {
            self.ramp.med = [dv; 5];
        } else {
            self.ramp.med[self.ramp.med_i % 5] = dv;
        }
        self.ramp.med_i += 1;
        let mut s = self.ramp.med;
        s.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        let dvm = s[2];
        let class = if dvm < 2.5 {
            1
        } else if dvm < 9.0 {
            2
        } else {
            3
        };
        if class != self.ramp.class {
            self.ramp.class = class;
            self.ramp.med_i = 0;
            // Comparator threshold by the REAL model byte (dmm_v2_6.h:164,326):
            // an HY3131 analog setting, not a counts limit, so not COUNTS_MODEL.
            let v = CAP_R21[class as usize - 1] - if self.model == 2 { 0 } else { 1 };
            hy::write(0x21, &[v]);
            self.deadline = now + 255;
            return Some(self.cap_val(0.0, true));
        }
        let c = (cal(0x170 + 4 * r as u32, 1.0) * CAP_K[r] * dt / dvm).clamp(0.0, 1.06 * lim);
        if self.ramp.n < 35 {
            self.ramp.buf[self.ramp.n] = c;
            self.ramp.n += 1;
        }
        let cmp = hy::read1(0x14) & 0x40 != 0;
        let flip = self.ramp.cmp.map(|p| p != cmp).unwrap_or(false);
        self.ramp.cmp = Some(cmp);
        if flip || self.ramp.n == 35 {
            let n = self.ramp.n;
            let mean = if n == 35 {
                self.ramp.buf[33]
            } else if n > 4 {
                let mut b = self.ramp.buf;
                b[..n].sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
                b[2..n - 2].iter().sum::<f32>() / (n - 4) as f32
            } else {
                c
            };
            if self.ramp.first {
                self.cap_c = c;
                self.ramp.first = false;
            } else {
                ema(CAP_WIN[r], 0.25, mean, &mut self.cap_c);
            }
            self.ramp.n = 0;
        }
        let cc = self.cap_c;
        let ol = cc / lim > ov;
        if !self.manual {
            if cc > ov * lim && r < 7 {
                self.cap_cnt_up += 1;
                if self.cap_cnt_up >= 5 {
                    self.cap_set_range(clk, r as i8 + 1);
                }
            } else if cc < 0.1 * lim / ov && r >= 6 {
                self.cap_cnt_dn += 1;
                if self.cap_cnt_dn >= 3 {
                    self.cap_set_range(clk, r as i8 - 1);
                }
            } else {
                self.cap_cnt_up = 0;
                self.cap_cnt_dn = 0;
            }
        }
        Some(self.cap_val(cc, ol))
    }

    /// Time (ms) before which `poll` gives no reading: the settle after a
    /// function / range change (SCPI starts its reply timeout from here).
    pub fn deadline(&self) -> u64 {
        self.deadline
    }
}

/// Format `v` with `dec` decimals into `buf`; returns the text.
pub fn format(buf: &mut [u8; 16], v: f32, dec: u8) -> &str {
    let mut scale = 1.0f32;
    for _ in 0..dec {
        scale *= 10.0;
    }
    let neg = v < 0.0;
    let mut n = ((if neg { -v } else { v }) * scale + 0.5) as u64;
    let mut tmp = [0u8; 16];
    let mut i = 0;
    loop {
        if i == dec as usize && dec > 0 {
            tmp[i] = b'.';
            i += 1;
        }
        tmp[i] = b'0' + (n % 10) as u8;
        i += 1;
        n /= 10;
        if n == 0 && i > dec as usize + (dec > 0) as usize {
            break;
        }
    }
    let mut o = 0;
    if neg {
        buf[o] = b'-';
        o += 1;
    }
    while i > 0 {
        i -= 1;
        buf[o] = tmp[i];
        o += 1;
    }
    core::str::from_utf8(&buf[..o]).unwrap_or("")
}

// Host tests: tools/meter-test/run.sh (pure logic only; no register access).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_window_per_rate() {
        // MEDIUM = the stock-plus-+1-digit windows of before.
        assert_eq!(avg_cfg(Rate::Medium, false, 1, 0), Some((8, 120, true)));
        assert_eq!(avg_cfg(Rate::Medium, false, 3, 1), Some((8, 120, true)));
        assert_eq!(avg_cfg(Rate::Medium, false, 10, 2), Some((16, 150, true)));
        assert_eq!(avg_cfg(Rate::Medium, false, 10, 0), Some((16, 150, false)));
        assert_eq!(avg_cfg(Rate::Medium, false, 1, 3), None);
        assert_eq!(avg_cfg(Rate::Medium, false, 8, 0), None);
        assert_eq!(avg_cfg(Rate::Medium, false, 10, 5), None);
        // SLOW: every AD1 DC range, longer.
        assert_eq!(avg_cfg(Rate::Slow, false, 1, 0), Some((16, 120, true)));
        assert_eq!(avg_cfg(Rate::Slow, false, 1, 3), Some((16, 120, false)));
        assert_eq!(avg_cfg(Rate::Slow, false, 8, 1), Some((16, 60, false)));
        assert_eq!(avg_cfg(Rate::Slow, false, 9, 0), Some((16, 57, false)));
        assert_eq!(avg_cfg(Rate::Slow, false, 10, 3), Some((32, 150, true)));
        assert_eq!(avg_cfg(Rate::Slow, false, 10, 6), Some((16, 240, false)));
        // FAST, AC+DC, other functions: none.
        for m in [1, 3, 7, 8, 9, 10] {
            assert_eq!(avg_cfg(Rate::Fast, false, m, 1), None);
            assert_eq!(avg_cfg(Rate::Medium, true, m, 1), None);
        }
        for m in [0, 2, 4, 11, 12, 13, 14, 15] {
            assert_eq!(avg_cfg(Rate::Slow, false, m, 1), None);
        }
    }

    #[test]
    fn extra_digit_only_with_a_full_window() {
        assert!(!ext_ok(true, 1));
        assert!(!ext_ok(true, AVG_FULL - 1));
        assert!(ext_ok(true, AVG_FULL));
        assert!(ext_ok(true, 32));
        assert!(!ext_ok(false, 32));
    }

    #[test]
    fn code_window_step_reset_and_convergence() {
        let mut a = Avg::new();
        // Noise of +-3 codes around 1000: the mean is exact once full.
        let mut m = 0.0;
        for i in 0..20 {
            m = a.push(if i % 2 == 0 { 1003 } else { 997 }, 8, 120);
        }
        assert_eq!(a.n, 8);
        assert_eq!(m, 1000.0);
        // Sub-code resolution: 1/n steps.
        let mut a = Avg::new();
        for x in [1000, 1001, 1001] {
            m = a.push(x, 8, 120);
        }
        assert!((m - 1000.6667).abs() < 1e-3);
        // A step of more than the threshold restarts the window at once.
        let mut a = Avg::new();
        for _ in 0..8 {
            a.push(1000, 8, 120);
        }
        assert_eq!(a.push(1200, 8, 120), 1200.0);
        assert_eq!(a.n, 1);
        // Within the threshold it is averaged in.
        assert_eq!(a.push(1300, 8, 120), 1250.0);
        // A window length change (rate change) restarts it.
        assert_eq!(a.push(1250, 16, 120), 1250.0);
        assert_eq!(a.n, 1);
        // 32 full-scale codes do not overflow.
        let mut a = Avg::new();
        for _ in 0..40 {
            m = a.push(8_388_607, 32, 120);
        }
        assert_eq!(m, 8_388_607.0);
    }

    #[test]
    fn fr05_conversion_periods() {
        // The FR-05 table's times (datasheet: OSR * 30 / SYSCLK, x2 chopped).
        assert_eq!(hy::ad1_period_us_r22(OSR_FAST), 100_000);
        assert_eq!(hy::ad1_period_us_r22(OSR_SLOW), 400_000);
        assert_eq!(AVG_FULL as u32 * hy::ad1_period_us_r22(OSR_SLOW), 3_200_000);
        // Window spans (one reading per conversion in every rate): MEDIUM
        // V r0 8 (3.2 s), SLOW 16 (6.4 s), SLOW ohm r0..r3 32 (12.8 s).
        let span = |r, m, g| avg_cfg(r, false, m, g).unwrap().0 as u32 * hy::ad1_period_us_r22(OSR_SLOW);
        assert_eq!(span(Rate::Medium, 1, 0), 3_200_000);
        assert_eq!(span(Rate::Slow, 1, 0), 6_400_000);
        assert_eq!(span(Rate::Slow, 10, 3), 12_800_000);
    }

    #[test]
    fn rate_names_and_codes() {
        for (i, r) in Rate::ALL.iter().enumerate() {
            assert_eq!(Rate::from_u8(i as u8), Some(*r));
            assert_eq!(*r as usize, i);
        }
        assert_eq!(Rate::from_u8(3), None);
        assert_eq!(RATE_NAMES[Rate::Medium as usize], "MED");
    }

    #[test]
    fn interval_mean() {
        let (mut acc, mut last) = (0.0, None);
        ema_ms(&mut acc, &mut last, 1000);
        assert_eq!(acc, 0.0);
        ema_ms(&mut acc, &mut last, 1107);
        assert_eq!(acc, 107.0);
        for i in 2..40 {
            ema_ms(&mut acc, &mut last, 1000 + 107 * i);
        }
        assert!((acc - 107.0).abs() < 0.01);
    }
}
