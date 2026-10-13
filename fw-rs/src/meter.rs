//! What the user sees on top of the raw DMM readings: HOLD, AutoHold, REL,
//! MIN/MAX/AVG and the Hi/Lo limits. Behaviour of HOLD / REL / MIN/MAX from
//! dmm_v2_6.h section 8 and bt_meas_v2_6.h (BT commands 03..05); AutoHold
//! (FR-01), AVG (FR-02) and limits (FR-06) are fw-rs additions
//! (../DM40C_feature_request.md).
//!
//! `hold` is the manual HOLD only (RUN key, BT command 04, BT status bit 7):
//! everything freezes, the statistics included. AutoHold is a separate mode:
//! the statistics, the chart and the HV warning keep running on the live
//! value, only the main readout is frozen on the last captured value.
//!
//! FR-05 display smoothing (`Smooth`, below) sits between the Dmm readings
//! and everything the user reads. Which value feeds what:
//!   - smoothed (REL applied): the readout (`shown`), the limit verdict,
//!     `live` (SCPI READ? / MEAS?, the trend chart), FETCh?, BT (shown);
//!     the REL reference is taken from the smoothed value (a quiet zero);
//!   - unsmoothed (the Dmm reading, REL applied): MIN / MAX / AVG, the
//!     AutoHold detector and its captured value, and `Update::raw` (the HV
//!     warning must not lag).
//! Why the statistics and AutoHold stay unsmoothed: smoothing is a display
//! preference and must not change what MIN / MAX mean (a moving average
//! clips the peaks, and AVG is the same either way); AutoHold has its own
//! stability window whose spread limits are tuned for the unsmoothed noise,
//! and feeding it a moving average would let a still-moving signal look
//! stable and capture a lagging mean. Both always saw the Dmm reading (with
//! the Dmm's own converter window, `dmm::Rate`), so this keeps their
//! behaviour independent of the new setting.

use crate::dmm::{Rate, Reading, Val};

/// SI multiplier of a display unit's prefix ("mV" -> 1e-3, "k\u{3a9}" ->
/// 1e3). One-character units and "\u{b0}C" have no prefix.
pub fn unit_scale(u: &str) -> f32 {
    let mut c = u.chars();
    match (c.next(), c.next()) {
        (Some(p), Some(_)) => match p {
            'm' => 1e-3,
            '\u{b5}' | 'u' => 1e-6,
            'n' => 1e-9,
            'k' => 1e3,
            'M' => 1e6,
            _ => 1.0,
        },
        _ => 1.0,
    }
}

/// The unit without its prefix ("mV" -> "V"): the measured quantity.
pub fn base_unit(u: &'static str) -> &'static str {
    match u.chars().next() {
        Some(p) if unit_scale(u) != 1.0 => &u[p.len_utf8()..],
        _ => u,
    }
}

/// 10^d.
fn p10(d: u8) -> f32 {
    let mut s = 1.0f32;
    for _ in 0..d {
        s *= 10.0;
    }
    s
}

/// SCPI-style function code of a reading (dmm.rs `Dmm::func`): the HY3131
/// mode, 16 / 17 = V / A AC+DC.
pub fn func_of(r: &Reading) -> u8 {
    match (r.acdc, r.mode) {
        (true, 0..=3) => 16,
        (true, _) => 17,
        (false, m) => m,
    }
}

// ------------------------------------------------------------- limits
//
// FR-06: one upper / lower pair per measuring function. Nine slots (the
// functions that measure something different): DC V, AC V, DC A, AC A, ohm
// (continuity shares it), diode, capacitance, frequency, temperature. AC+DC
// shares the AC slot (it is an RMS value). Values are SI (V, A, ohm, V, F,
// Hz, deg C) so a limit holds across auto ranges; it is compared with the
// displayed (REL-applied) value.

pub const LIM_SLOTS: usize = 9;
pub const LIM_NAMES: [&str; LIM_SLOTS] = ["DC V", "AC V", "DC A", "AC A", "\u{3a9}", "Diode", "Cap", "Hz", "Temp"];

/// Limit slot of a function code (`func_of` / `Dmm::func`).
pub fn lim_slot(func: u8) -> usize {
    match func {
        1 | 3 => 0,
        0 | 2 | 16 => 1,
        7..=9 => 2,
        4..=6 | 17 => 3,
        10 | 11 => 4,
        12 => 5,
        13 => 6,
        14 => 7,
        _ => 8,
    }
}

#[derive(Clone, Copy)]
pub struct Limit {
    /// Checking enabled for this function.
    pub on: bool,
    pub lo: Option<f32>,
    pub hi: Option<f32>,
}

/// Limit verdict of the displayed value.
pub const V_NONE: u8 = 0;
pub const V_PASS: u8 = 1;
pub const V_HI: u8 = 2;
pub const V_LO: u8 = 3;

impl Limit {
    const OFF: Limit = Limit { on: false, lo: None, hi: None };

    /// Verdict for a displayed reading. OL is above the range: a fail when
    /// an upper limit is set, a pass otherwise.
    pub fn check(&self, r: &Reading) -> u8 {
        if !self.on || (self.lo.is_none() && self.hi.is_none()) {
            return V_NONE;
        }
        if r.main.ol {
            return if self.hi.is_some() { V_HI } else { V_PASS };
        }
        let v = r.si;
        if self.hi.is_some_and(|h| v > h) {
            V_HI
        } else if self.lo.is_some_and(|l| v < l) {
            V_LO
        } else {
            V_PASS
        }
    }
}

// ---------------------------------------------------------- MIN/MAX/AVG

/// FR-02 statistics of the displayed value, in SI (so an auto-range step
/// mV <-> V keeps them, shown converted to the current unit). AVG is a
/// compensated (Kahan) f32 sum: no f64 in this build, and the error then does
/// not grow with the sample count.
#[derive(Clone, Copy)]
pub struct Stats {
    pub n: u32,
    min: f32,
    max: f32,
    sum: f32,
    comp: f32,
    /// Time of the first sample (ms).
    t0: u64,
    /// A deliberately started recording session (tap on the MIN/MAX column,
    /// or a host command): inhibits the idle shutdown (FR-02 / FR-10). The
    /// statistics accumulate whether or not it is set.
    pub rec: bool,
}

impl Stats {
    const fn new() -> Self {
        Stats { n: 0, min: 0.0, max: 0.0, sum: 0.0, comp: 0.0, t0: 0, rec: false }
    }

    /// Forget the samples (keeps `rec`).
    pub fn clear(&mut self) {
        *self = Stats { rec: self.rec, ..Stats::new() };
    }

    fn add(&mut self, x: f32, now: u64) {
        if !x.is_finite() {
            return;
        }
        if self.n == 0 {
            self.min = x;
            self.max = x;
            self.t0 = now;
        } else {
            self.min = self.min.min(x);
            self.max = self.max.max(x);
        }
        let y = x - self.comp;
        let t = self.sum + y;
        self.comp = (t - self.sum) - y;
        self.sum = t;
        self.n = self.n.saturating_add(1);
    }

    /// (min, max, avg) in SI, None before the first sample.
    pub fn si(&self) -> Option<(f32, f32, f32)> {
        (self.n > 0).then(|| (self.min, self.max, self.sum / self.n as f32))
    }

    /// Seconds since the first sample.
    pub fn secs(&self, now: u64) -> Option<u32> {
        (self.n > 0).then(|| (now.saturating_sub(self.t0) / 1000) as u32)
    }
}

// ----------------------------------------------------------- smoothing
//
// FR-05: a moving average over the last N READINGS (the converter window
// over AD1 conversions is dmm.rs `Rate`), restarted
//   - by a step: a reading further than max(cnt counts, rel x |mean|) from
//     the window mean (the same step-reset idea as the Dmm code window, so a
//     real change shows at once: P3, no slow last-digit crawl),
//   - by OL, by any function / unit / resolution change (autorange), by a
//     setting change and by `Meter::reset` (function change, SCPI CONF).
// Counts are of the 60000-count format (1 count = 10^-dec display units);
// temperature uses absolute deg C. Off by default (P3 in
// docs/user-feedback.md; the +1 digit does not need it: it comes from the
// Dmm code window). Per function (spans at the MEDIUM rate):
//   function                   reading every  Low (span)     High (span)    restart on
//   DC V / mV / A, ohm, diode  ~0.11 s        4 (~0.4 s)     16 (~1.7 s)    3 counts or 0.05 % (ohm 0.1 %)
//   AC V / A, AC+DC            RMS update     4              16             30 counts or 0.3 %
//   capacitance                0.42 s or more 2 (~0.8 s)     4 (~1.7 s)     5 counts or 1 %
//   frequency                  0.35 s         2 (0.7 s)      4 (1.4 s)      3 counts or 0.2 %
//   temperature, thermocouple  ~2.2 s         2 (~4.4 s)     4 (~9 s)       0.3 C
//   temperature, PT / NTC      ~0.11 s        4              16             0.3 C
//   continuity                 never (the readout must follow the beeper)
// SLOW rate: readings every ~0.43 s, so the DC spans are 4x longer.
// The +1 digit is decided by the Dmm (`Val::ext`: only with a full converter
// window, dmm.rs FR-05 table); smoothing never adds it.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Smooth {
    Off = 0,
    Low = 1,
    High = 2,
}

/// SCPI / settings names, indexed by `Smooth as usize`.
pub const SMOOTH_NAMES: [&str; 3] = ["OFF", "LOW", "HIGH"];

impl Smooth {
    pub const ALL: [Smooth; 3] = [Smooth::Off, Smooth::Low, Smooth::High];

    pub fn from_u8(v: u8) -> Option<Smooth> {
        Smooth::ALL.get(v as usize).copied()
    }
}

/// Longest smoothing window (readings).
const SM_MAX: usize = 16;

/// (window length, restart threshold in display units of `r`) for the
/// setting; length 1 = not smoothed.
fn sm_cfg(r: &Reading, s: Smooth) -> (usize, f32) {
    if s == Smooth::Off || r.mode == 11 {
        return (1, 0.0);
    }
    let hi = s == Smooth::High;
    let cnt = 1.0 / p10(r.main.dec);
    let v = r.main.v.abs();
    let (lo_n, hi_n, thr) = match r.mode {
        _ if r.acdc => (4, 16, (30.0 * cnt).max(0.003 * v)),
        0 | 2 | 4..=6 => (4, 16, (30.0 * cnt).max(0.003 * v)),
        10 => (4, 16, (3.0 * cnt).max(0.001 * v)),
        1 | 3 | 7..=9 | 12 => (4, 16, (3.0 * cnt).max(0.0005 * v)),
        13 => (2, 4, (5.0 * cnt).max(0.01 * v)),
        14 => (2, 4, (3.0 * cnt).max(0.002 * v)),
        // Temperature: the thermocouple shows 1 decimal (one reading per
        // ~2.2 s), the resistive sensors 2 (one per conversion).
        _ if r.main.dec <= 1 => (2, 4, 0.3),
        _ => (4, 16, 0.3),
    };
    (if hi { hi_n } else { lo_n }, thr)
}

/// Moving average of displayed values of one function / unit / resolution.
/// Values are kept as offsets from the first one of the window, so a 32-bit
/// float sum stays exact to well below the +1 digit (6.00000 V).
#[derive(Clone, Copy)]
struct Smoother {
    d: [f32; SM_MAX],
    n: u8,
    i: u8,
    len: u8,
    base: f32,
    func: u8,
    unit: &'static str,
    dec: u8,
}

impl Smoother {
    const fn new() -> Self {
        Smoother { d: [0.0; SM_MAX], n: 0, i: 0, len: 0, base: 0.0, func: 255, unit: "", dec: 0 }
    }

    fn reset(&mut self) {
        self.n = 0;
        self.i = 0;
    }

    fn mean(&self) -> f32 {
        let n = self.n as usize;
        self.base + self.d[..n].iter().sum::<f32>() / n as f32
    }

    /// The window mean when it holds values of this unit / resolution.
    fn current(&self, unit: &str, dec: u8) -> Option<f32> {
        (self.n > 0 && self.unit == unit && self.dec == dec).then(|| self.mean())
    }

    /// Feed one value; returns the smoothed value.
    fn push(&mut self, v: f32, len: usize, thr: f32, func: u8, unit: &'static str, dec: u8) -> f32 {
        let len = len.min(SM_MAX);
        if len <= 1 {
            self.reset();
            return v;
        }
        if self.n == 0
            || len != self.len as usize
            || func != self.func
            || unit != self.unit
            || dec != self.dec
            || (v - self.mean()).abs() > thr
        {
            *self = Smoother { len: len as u8, base: v, func, unit, dec, ..Smoother::new() };
        }
        self.d[self.i as usize] = v - self.base;
        if (self.n as usize) < len {
            self.n += 1;
        }
        self.i = ((self.i as usize + 1) % len) as u8;
        self.mean()
    }
}

/// The readout is drawn at most this often (ms): FAST and the stock
/// moving-OSR bursts give up to ~37 readings/s, more than the panel needs.
/// Continuity, a unit / resolution / OL change and an AutoHold capture are
/// drawn at once.
const SHOW_MIN_MS: u64 = 80;

// ------------------------------------------------------ persisted (p[38])
//
// Backup-SRAM word p[38] (persist.rs W_DMM_EXT, a spare word of record v4;
// every record written before reads 0 = all defaults):
//   bits 0..1  dmm::Rate + 1   (0 = default MEDIUM)
//   bits 2..3  Smooth + 1      (0 = default OFF)
//   bit 8      UF-21: the V autorange stored in p[2] is a choice made under
//              an AUTO+-default build. Without it the stored value is the old
//              AUTO default, and restores as AUTO+.

/// Settings decoded from p[38].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExtSettings {
    pub rate: Rate,
    pub smooth: Smooth,
    /// p[2]'s V autorange flag is to be trusted (UF-21 migration).
    pub v_auto_valid: bool,
}

pub const RATE_DEFAULT: Rate = Rate::Medium;
pub const SMOOTH_DEFAULT: Smooth = Smooth::Off;

pub fn ext_word(rate: Rate, smooth: Smooth) -> u32 {
    (rate as u32 + 1) | (smooth as u32 + 1) << 2 | 1 << 8
}

pub fn ext_settings(w: u32) -> ExtSettings {
    let f = |b: u32| (b as u8).checked_sub(1);
    ExtSettings {
        rate: f(w & 3).and_then(Rate::from_u8).unwrap_or(RATE_DEFAULT),
        smooth: f(w >> 2 & 3).and_then(Smooth::from_u8).unwrap_or(SMOOTH_DEFAULT),
        v_auto_valid: w >> 8 & 1 != 0,
    }
}

// ------------------------------------------------------------ AutoHold
//
// FR-01 / Appendix B, on the raw (pre-REL, unsmoothed) value: a window of readings that
// stay within `spread` of each other for `t_ms` (and at least `n_min`
// readings) is "stable"; its mean is captured (beep, readout frozen) when
//   - nothing was captured yet, or
//   - the input went idle since the last capture (OL, or below the minimum
//     level: probes lifted / open leads), so measuring the same value again
//     on the next part beeps again, or
//   - it differs from the captured value by more than `change` (2 % of the
//     range, Appendix B) without going idle.
// Idle / OL readings are never captured and restart the window. Deviation
// from Appendix B: there OL returns to IDLE and only a > 2 % change re-arms,
// so the same resistor measured twice through open leads would never beep
// again; here going idle re-arms.
//
// Thresholds are in counts of the current range (1 count = 10^-dec display
// units, 60000 counts = full scale), so they follow the range; temperature
// uses absolute deg C (its counts are not range-related).

#[derive(Clone, Copy)]
struct AhCfg {
    /// Stable for at least this long (ms) ...
    t_ms: u32,
    /// ... and this many readings.
    n_min: u16,
    /// Allowed peak-to-peak spread in the window: max(counts, rel x |value|).
    spread_cnt: f32,
    spread_rel: f32,
    /// Below this |value| (counts) the input counts as idle (0 = OL only).
    level_cnt: f32,
    /// A change bigger than this (counts) re-captures without going idle.
    change_cnt: f32,
    /// Temperature: the *_cnt fields are deg C.
    abs: bool,
}

fn ah_cfg(r: &Reading) -> AhCfg {
    const FS2: f32 = 1200.0; // 2 % of 60000 counts
    let c = |t_ms, n_min, spread_cnt, spread_rel, level_cnt, change_cnt| AhCfg {
        t_ms,
        n_min,
        spread_cnt,
        spread_rel,
        level_cnt,
        change_cnt,
        abs: false,
    };
    match r.mode {
        // AC V / A and AC+DC: RMS converter ripple, a few times the DC noise;
        // open AC leads pick up hum: idle below 2 % FS (12 mV on 600 mV).
        _ if r.acdc => c(1000, 4, 100.0, 0.003, FS2, FS2),
        0 | 2 | 4..=6 => c(1000, 4, 100.0, 0.003, FS2, FS2),
        // DC V / A: idle below 0.5 % FS (3 mV on the 600 mV range, 3 uA on
        // 600 uA): open leads / no current.
        1 | 3 | 7..=9 => c(1000, 4, 40.0, 0.001, 300.0, FS2),
        // Ohm / continuity / diode: open = OL (idle); a short is a valid
        // reading and may be captured.
        10 | 11 => c(1000, 3, 40.0, 0.002, 0.0, FS2),
        12 => c(800, 3, 40.0, 0.002, 0.0, FS2),
        // Capacitance: slow conversions (one per 0.3..several s), so a longer
        // window and fewer readings; 1 % spread; stray C of open leads below
        // 0.5 % FS is idle.
        13 => c(1500, 2, 40.0, 0.01, 300.0, FS2),
        // Frequency: no signal reads 0.
        14 => c(1000, 3, 20.0, 0.001, 1.0, FS2),
        // Temperature: 0.3 C peak-to-peak over 1.5 s, re-capture on 2 C.
        _ => AhCfg { t_ms: 1500, n_min: 2, spread_cnt: 0.3, spread_rel: 0.0, level_cnt: 0.0, change_cnt: 2.0, abs: true },
    }
}

/// Stability window over raw display values of one unit / resolution.
#[derive(Clone, Copy)]
struct Win {
    n: u16,
    t0: u64,
    lo: f32,
    hi: f32,
    sum: f32,
    unit: &'static str,
    dec: u8,
}

impl Win {
    const EMPTY: Win = Win { n: 0, t0: 0, lo: 0.0, hi: 0.0, sum: 0.0, unit: "", dec: 0 };

    fn start(&mut self, v: &Val, now: u64) {
        *self = Win { n: 1, t0: now, lo: v.v, hi: v.v, sum: v.v, unit: v.unit, dec: v.dec };
    }
}

/// What `Meter::update` produced.
pub struct Update {
    /// The live reading (REL and smoothing applied): SCPI READ? / MEAS?,
    /// chart.
    pub live: Reading,
    /// The live reading without the display smoothing (REL applied): the HV
    /// warning (must not lag); what the statistics recorded.
    pub raw: Reading,
    /// New readout to draw (None: unchanged / frozen).
    pub show: Option<Reading>,
    /// AutoHold captured a new value now (beep).
    pub captured: bool,
    /// The limit verdict of the readout just turned into a fail (beep).
    pub fail_edge: bool,
}

pub struct Meter {
    /// Manual HOLD (RUN key, BT 04).
    pub hold: bool,
    /// REL reference in display units of `rel_unit`.
    rel: Option<(f32, &'static str)>,
    shown: Option<Reading>,
    live: Option<Reading>,
    unit: &'static str,
    pub stats: Stats,
    /// AutoHold enabled (persisted setting).
    pub ahold: bool,
    /// AutoHold capture (the frozen readout), its raw SI value.
    ah_held: Option<(Reading, f32)>,
    /// Input went idle (OL / below level) since the last capture.
    ah_rearm: bool,
    win: Win,
    /// Limits per function slot (persisted) and the fail beep switch.
    pub lim: [Limit; LIM_SLOTS],
    pub lim_beep: bool,
    verdict: u8,
    /// FR-05 display smoothing (persisted; change it with `set_smoothing`).
    smooth: Smooth,
    sm: Smoother,
    /// When the readout was last handed out to be drawn (SHOW_MIN_MS).
    show_t: u64,
}

impl Meter {
    pub const fn new() -> Self {
        Meter {
            hold: false,
            rel: None,
            shown: None,
            live: None,
            unit: "",
            stats: Stats::new(),
            ahold: false,
            ah_held: None,
            ah_rearm: false,
            win: Win::EMPTY,
            lim: [Limit::OFF; LIM_SLOTS],
            lim_beep: true,
            verdict: V_NONE,
            smooth: SMOOTH_DEFAULT,
            sm: Smoother::new(),
            show_t: 0,
        }
    }

    pub fn smoothing(&self) -> Smooth {
        self.smooth
    }

    /// Change the display smoothing: the window restarts; the statistics
    /// (unsmoothed) are kept.
    pub fn set_smoothing(&mut self, s: Smooth) {
        self.smooth = s;
        self.sm.reset();
    }

    /// Readings in the current smoothing window (0 = not smoothed).
    pub fn smooth_n(&self) -> u8 {
        self.sm.n
    }

    /// The Dmm reading with the display smoothing applied (pre-REL).
    fn smoothed(&mut self, r: &Reading) -> Reading {
        let mut f = *r;
        if r.main.ol {
            self.sm.reset();
            return f;
        }
        let (len, thr) = sm_cfg(r, self.smooth);
        let v = self.sm.push(r.main.v, len, thr, func_of(r), r.main.unit, r.main.dec);
        if v != r.main.v {
            let k = if r.main.v != 0.0 { r.si / r.main.v } else { unit_scale(r.main.unit) };
            f.main.v = v;
            f.si = v * k;
        }
        f
    }

    pub fn rel_active(&self) -> bool {
        self.rel.is_some()
    }

    pub fn rel_value(&self) -> Option<f32> {
        self.rel.map(|r| r.0)
    }

    /// The reading as displayed (after REL), frozen while HOLD or captured
    /// by AutoHold.
    pub fn shown(&self) -> Option<&Reading> {
        self.shown.as_ref()
    }

    /// The latest reading (after REL), also while the readout is frozen.
    pub fn live(&self) -> Option<&Reading> {
        self.live.as_ref()
    }

    /// Readout state for the status label: 0 RUN, 1 HOLD (manual), 2
    /// AutoHold waiting / tracking, 3 AutoHold showing a captured value.
    pub fn hold_state(&self) -> u8 {
        if self.hold {
            1
        } else if !self.ahold {
            0
        } else if self.ah_held.is_some() {
            3
        } else {
            2
        }
    }

    /// Limit verdict of the readout (V_NONE / V_PASS / V_HI / V_LO).
    pub fn verdict(&self) -> u8 {
        self.verdict
    }

    /// Re-evaluate the verdict of the readout after a limit change (no
    /// beep: only a reading that crosses a limit beeps).
    pub fn recheck(&mut self) {
        self.verdict = match self.shown {
            Some(s) => self.lim[lim_slot(func_of(&s))].check(&s),
            None => V_NONE,
        };
    }

    /// MIN / MAX / AVG in the current display unit.
    pub fn stats_disp(&self) -> Option<(f32, f32, f32)> {
        let k = unit_scale(self.unit);
        self.stats.si().map(|(a, b, c)| (a / k, b / k, c / k))
    }

    /// Feed a new reading.
    pub fn update(&mut self, r: &Reading, now: u64) -> Update {
        if r.main.unit != self.unit {
            // Another quantity (V -> A ...): statistics and the AutoHold
            // capture no longer apply. A prefix step of the auto range (mV ->
            // V) keeps the SI statistics. REL is in display units: it stays
            // only in its own unit (the range is held while REL is on).
            if base_unit(r.main.unit) != base_unit(self.unit) {
                self.stats.clear();
                self.ah_held = None;
                self.win = Win::EMPTY;
            }
            self.unit = r.main.unit;
            if self.rel.map(|x| x.1 != r.main.unit).unwrap_or(false) {
                self.rel = None;
            }
        }
        let k = if r.main.v != 0.0 { r.si / r.main.v } else { unit_scale(r.main.unit) };
        let rel = self.rel;
        let apply_rel = |x: &Reading| {
            let mut d = *x;
            if let (Some((ref_v, _)), false) = (rel, d.main.ol) {
                d.main.v -= ref_v;
                d.si = d.main.v * k;
            }
            d
        };
        // Unsmoothed (statistics, AutoHold, HV) and smoothed (everything
        // else), both with REL; see the module notes.
        let raw = apply_rel(r);
        let f = self.smoothed(r);
        let d = apply_rel(&f);
        self.live = Some(d);
        let mut u = Update { live: d, raw, show: None, captured: false, fail_edge: false };
        if self.hold {
            return u;
        }
        if !raw.main.ol {
            self.stats.add(raw.main.v * unit_scale(raw.main.unit), now);
        }
        let show = if !self.ahold {
            Some(d)
        } else if let Some(mean) = self.ah_step(r, now) {
            // Freeze the window mean (REL applied as for the live value).
            let mut h = raw;
            h.main.v = mean - (r.main.v - raw.main.v);
            h.si = h.main.v * k;
            self.ah_held = Some((h, mean * unit_scale(r.main.unit)));
            u.captured = true;
            Some(h)
        } else if self.ah_held.is_some() {
            None
        } else {
            Some(d)
        };
        // Draw-rate limit (SHOW_MIN_MS); `shown` stays the value drawn.
        let show = show.filter(|s| {
            let same = self.shown.is_some_and(|p| {
                p.main.unit == s.main.unit && p.main.dec == s.main.dec && p.main.ol == s.main.ol && p.main.ext == s.main.ext
            });
            u.captured || s.mode == 11 || !same || now >= self.show_t + SHOW_MIN_MS
        });
        if let Some(s) = show {
            self.show_t = now;
            self.shown = Some(s);
            let v = self.lim[lim_slot(func_of(&s))].check(&s);
            u.fail_edge = matches!(v, V_HI | V_LO) && !matches!(self.verdict, V_HI | V_LO);
            self.verdict = v;
            u.show = Some(s);
        }
        u
    }

    /// One AutoHold step on the raw reading; Some(window mean, raw display
    /// units) when a value is captured.
    fn ah_step(&mut self, r: &Reading, now: u64) -> Option<f32> {
        let c = ah_cfg(r);
        let cnt = if c.abs { 1.0 } else { p10(r.main.dec) };
        let v = &r.main;
        if v.ol || (c.level_cnt > 0.0 && v.v.abs() * cnt < c.level_cnt) {
            self.ah_rearm = true;
            self.win = Win::EMPTY;
            return None;
        }
        let w = &mut self.win;
        // (A window that stays stable for minutes restarts now and then, so
        // its f32 sum stays exact.)
        if w.n == 0 || w.unit != v.unit || w.dec != v.dec || w.n >= 512 {
            w.start(v, now);
        } else {
            let (lo, hi) = (w.lo.min(v.v), w.hi.max(v.v));
            let spread = (c.spread_cnt / cnt).max(c.spread_rel * v.v.abs());
            if hi - lo > spread {
                // Not settled yet: the window restarts at this reading.
                w.start(v, now);
            } else {
                w.lo = lo;
                w.hi = hi;
                w.sum += v.v;
                w.n = w.n.saturating_add(1);
            }
        }
        if w.n < c.n_min || now.saturating_sub(w.t0) < c.t_ms as u64 {
            return None;
        }
        let mean = w.sum / w.n as f32;
        let si = mean * unit_scale(v.unit);
        let fresh = match self.ah_held {
            None => true,
            Some((_, held)) => {
                let change = c.change_cnt / cnt * unit_scale(v.unit);
                self.ah_rearm || (si - held).abs() > change
            }
        };
        if fresh {
            self.ah_rearm = false;
            // Keep the window: the next capture needs a change or idle.
            Some(mean)
        } else {
            None
        }
    }

    /// AutoHold on / off. Turning it on releases a manual HOLD; off unfreezes.
    pub fn set_ahold(&mut self, on: bool) {
        self.ahold = on;
        self.hold = false;
        self.ah_held = None;
        self.ah_rearm = false;
        self.win = Win::EMPTY;
        if let Some(l) = self.live {
            self.shown = Some(l);
        }
    }

    pub fn set_hold(&mut self, on: bool) {
        self.hold = on;
    }

    /// REL on: reference = the current reading (not while HOLD or OL); with
    /// smoothing on, the smoothed value of the same unit / resolution (the
    /// zero is then as quiet as the readout). Returns false if refused.
    pub fn rel_on(&mut self, raw: Option<&Reading>) -> bool {
        match raw {
            Some(r) if !self.hold && !r.main.ol => {
                let v = self.sm.current(r.main.unit, r.main.dec).unwrap_or(r.main.v);
                self.rel = Some((v, r.main.unit));
                self.stats.clear();
                self.ah_held = None;
                true
            }
            _ => false,
        }
    }

    pub fn rel_off(&mut self) {
        if self.rel.take().is_some() {
            self.ah_held = None;
        }
        self.stats.clear();
    }

    /// Clear everything but the settings (mode change, BT command 03, SCPI
    /// CONF): HOLD, REL, statistics and the recording session, the AutoHold
    /// capture. AutoHold on/off and the limits stay.
    pub fn reset(&mut self) {
        self.hold = false;
        self.rel_off();
        self.stats.rec = false;
        self.shown = None;
        self.live = None;
        self.ah_held = None;
        self.ah_rearm = false;
        self.win = Win::EMPTY;
        self.verdict = V_NONE;
        self.sm.reset();
    }
}

/// Display text for a value ("0L" when over range).
pub fn text<'a>(buf: &'a mut [u8; 16], v: &Val) -> &'a str {
    if v.ol { "0L" } else { crate::dmm::format(buf, v.v, v.dec) }
}

// Host tests: tools/meter-test/run.sh (rustc --test with dmm.rs and its
// register-level dependencies compiled for the host; nothing touches them).
#[cfg(test)]
mod tests {
    use super::*;

    /// A DC V reading on the 6 V range (dec 4, 60000 counts = 6 V).
    fn dcv(v: f32) -> Reading {
        rd(1, v, 4, "V", 1.0)
    }

    fn rd(mode: u8, v: f32, dec: u8, unit: &'static str, k: f32) -> Reading {
        Reading {
            mode,
            range: 0,
            main: Val { v, dec, unit, ol: false, ext: false },
            aux1: None,
            aux2: None,
            si: v * k,
            auto: true,
            auto_plus: true,
            acdc: false,
            seq: 0,
            avg_n: 0,
        }
    }

    /// Feed `vals` 110 ms apart (MEDIUM rate); the live values.
    fn feed(m: &mut Meter, t: &mut u64, vals: &[f32]) -> [f32; 64] {
        let mut out = [0.0; 64];
        for (i, &v) in vals.iter().enumerate() {
            *t += 110;
            out[i] = m.update(&dcv(v), *t).live.main.v;
        }
        out
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 2e-6
    }

    #[test]
    fn off_passes_through() {
        let mut m = Meter::new();
        assert_eq!(m.smoothing(), Smooth::Off);
        let mut t = 0;
        let v = [0.50003, 0.49998, 0.50001, 0.50004];
        let o = feed(&mut m, &mut t, &v);
        for i in 0..4 {
            assert_eq!(o[i], v[i]);
        }
        assert_eq!(m.smooth_n(), 0);
    }

    #[test]
    fn low_and_high_converge_to_the_window_mean() {
        // +-1 count (0.1 mV) noise around 0.5 V: within the restart threshold.
        let noise = |i: usize| if i % 2 == 0 { 0.5001 } else { 0.4999 };
        for (s, n) in [(Smooth::Low, 4usize), (Smooth::High, 16)] {
            let mut m = Meter::new();
            m.set_smoothing(s);
            let mut t = 0;
            let mut last = 0.0;
            for i in 0..40 {
                t += 110;
                last = m.update(&dcv(noise(i)), t).live.main.v;
                if i + 1 >= n {
                    // Full window of an even length: exactly the mean.
                    assert!(close(last, 0.5), "{s:?} i={i} {last}");
                }
            }
            assert!(close(last, 0.5));
            assert_eq!(m.smooth_n() as usize, n);
        }
    }

    #[test]
    fn small_change_settles_in_one_window() {
        // A 2-count change (below the 3-count threshold) is averaged in.
        let mut m = Meter::new();
        m.set_smoothing(Smooth::Low);
        let mut t = 0;
        feed(&mut m, &mut t, &[0.5; 8]);
        let o = feed(&mut m, &mut t, &[0.5002; 4]);
        assert!(close(o[0], 0.50005) && close(o[1], 0.5001) && close(o[2], 0.50015));
        assert!(close(o[3], 0.5002));
    }

    #[test]
    fn step_restarts_the_window_at_once() {
        for s in [Smooth::Low, Smooth::High] {
            let mut m = Meter::new();
            m.set_smoothing(s);
            let mut t = 0;
            feed(&mut m, &mut t, &[0.5; 20]);
            // 0.5 V -> 0.6 V: the first reading after the step is shown as is.
            let o = feed(&mut m, &mut t, &[0.6, 0.6001, 0.5999]);
            assert_eq!(o[0], 0.6, "{s:?}");
            assert_eq!(m.smooth_n(), 3);
            assert!(close(o[2], 0.6));
            // Also downwards and by a relative step on a large value (0.05 %
            // of 5 V = 2.5 mV > 3 counts = 0.3 mV).
            let o = feed(&mut m, &mut t, &[5.0, 5.0, 5.002, 5.004]);
            assert_eq!(o[0], 5.0);
            assert!(close(o[2], 5.0 + 0.002 / 3.0));
            assert_eq!(o[3], 5.004, "{s:?}: 4 mV > 0.05 % restarts");
        }
    }

    #[test]
    fn unit_resolution_ol_and_function_restart() {
        let mut m = Meter::new();
        m.set_smoothing(Smooth::High);
        let mut t = 0;
        feed(&mut m, &mut t, &[0.5; 10]);
        // Autorange 6 V -> 60 V (dec 4 -> 3): restart.
        t += 110;
        let mut r = dcv(0.5);
        r.main.dec = 3;
        m.update(&r, t);
        assert_eq!(m.smooth_n(), 1);
        // OL passes through and empties the window.
        t += 110;
        let mut o = dcv(0.0);
        o.main.ol = true;
        assert!(m.update(&o, t).live.main.ol);
        assert_eq!(m.smooth_n(), 0);
        // Continuity is never smoothed.
        let mut m = Meter::new();
        m.set_smoothing(Smooth::High);
        for i in 0..5 {
            let v = 10.0 + i as f32;
            assert_eq!(m.update(&rd(11, v, 2, "\u{3a9}", 1.0), 1000 * i).live.main.v, v);
        }
        // A setting change and reset() restart the window.
        let mut m = Meter::new();
        m.set_smoothing(Smooth::Low);
        let mut t = 0;
        feed(&mut m, &mut t, &[0.5; 6]);
        m.set_smoothing(Smooth::High);
        assert_eq!(m.smooth_n(), 0);
        feed(&mut m, &mut t, &[0.5; 3]);
        m.reset();
        assert_eq!(m.smooth_n(), 0);
    }

    #[test]
    fn per_function_windows() {
        let n = |r: Reading, s| sm_cfg(&r, s).0;
        let dc = dcv(1.0);
        assert_eq!((n(dc, Smooth::Off), n(dc, Smooth::Low), n(dc, Smooth::High)), (1, 4, 16));
        assert_eq!(n(rd(0, 1.0, 4, "V", 1.0), Smooth::High), 16);
        assert_eq!(n(rd(13, 1.0, 3, "nF", 1e-9), Smooth::Low), 2);
        assert_eq!(n(rd(14, 1.0, 3, "kHz", 1e3), Smooth::High), 4);
        assert_eq!(n(rd(15, 25.0, 1, "\u{b0}C", 1.0), Smooth::High), 4);
        assert_eq!(n(rd(15, 25.0, 2, "\u{b0}C", 1.0), Smooth::High), 16);
        assert_eq!(n(rd(11, 1.0, 2, "\u{3a9}", 1.0), Smooth::High), 1);
        // Thresholds: 3 counts on DC (6 V range: 0.3 mV), 30 counts on AC.
        assert!(close(sm_cfg(&dcv(0.1), Smooth::Low).1, 0.0003));
        assert!(close(sm_cfg(&rd(0, 0.1, 4, "V", 1.0), Smooth::Low).1, 0.003));
        assert!(close(sm_cfg(&rd(15, 25.0, 2, "\u{b0}C", 1.0), Smooth::Low).1, 0.3));
    }

    #[test]
    fn statistics_and_autohold_see_unsmoothed_values() {
        let mut m = Meter::new();
        m.set_smoothing(Smooth::High);
        m.stats.clear();
        let mut t = 0;
        let v = [0.5, 0.5002, 0.4998, 0.5, 0.5002, 0.4998];
        let mut raw_seen = [0.0; 6];
        for (i, &x) in v.iter().enumerate() {
            t += 110;
            let u = m.update(&dcv(x), t);
            raw_seen[i] = u.raw.main.v;
        }
        assert_eq!(raw_seen, v);
        let (mn, mx, avg) = m.stats.si().unwrap();
        assert!(close(mn, 0.4998) && close(mx, 0.5002) && close(avg, 0.5));
        // AutoHold: stable within its spread for 1 s -> captures the mean of
        // the unsmoothed window; the smoothed live value keeps running.
        let mut m = Meter::new();
        m.set_smoothing(Smooth::High);
        m.set_ahold(true);
        let mut t = 0;
        let mut cap = None;
        for i in 0..12 {
            t += 110;
            let x = if i % 2 == 0 { 1.2001 } else { 1.1999 };
            let u = m.update(&dcv(x), t);
            if u.captured {
                cap = u.show.map(|s| s.main.v);
                break;
            }
        }
        // (The mean of an odd number of +-1-count samples: within 0.2 count.)
        let c = cap.expect("AutoHold captured");
        assert!((c - 1.2).abs() < 2e-5, "{c}");
    }

    #[test]
    fn rel_uses_the_smoothed_value() {
        let mut m = Meter::new();
        m.set_smoothing(Smooth::Low);
        let mut t = 0;
        feed(&mut m, &mut t, &[0.5001, 0.4999, 0.5001, 0.4999]);
        assert!(m.rel_on(Some(&dcv(0.4999))));
        assert!(close(m.rel_value().unwrap(), 0.5));
        // Off: the reading itself.
        let mut m = Meter::new();
        let mut t = 0;
        feed(&mut m, &mut t, &[0.5001, 0.4999]);
        assert!(m.rel_on(Some(&dcv(0.4999))));
        assert_eq!(m.rel_value(), Some(0.4999));
    }

    #[test]
    fn draw_rate_is_limited() {
        // 27 ms readings (FAST): drawn at most every SHOW_MIN_MS; 110 ms
        // readings: every one.
        let mut m = Meter::new();
        let mut drawn = 0;
        for i in 1..=37u64 {
            if m.update(&dcv(0.5 + i as f32 * 1e-4), i * 27).show.is_some() {
                drawn += 1;
            }
        }
        assert!((10..=13).contains(&drawn), "{drawn}");
        let mut m = Meter::new();
        let mut t = 0;
        for i in 0..10 {
            t += 110;
            assert!(m.update(&dcv(0.5 + i as f32 * 1e-4), t).show.is_some());
        }
        // A resolution change is drawn at once.
        let mut r = dcv(0.5);
        r.main.dec = 3;
        assert!(m.update(&r, t + 1).show.is_some());
    }

    #[test]
    fn persisted_word() {
        // Records written before FR-05: 0 = defaults, V autorange not valid.
        let d = ext_settings(0);
        assert_eq!(d, ExtSettings { rate: Rate::Medium, smooth: Smooth::Off, v_auto_valid: false });
        for r in Rate::ALL {
            for s in Smooth::ALL {
                let e = ext_settings(ext_word(r, s));
                assert_eq!(e, ExtSettings { rate: r, smooth: s, v_auto_valid: true });
            }
        }
        // Bits outside the fields are ignored.
        assert_eq!(ext_settings(ext_word(Rate::Slow, Smooth::Low) | 0xFFFF_0000).rate, Rate::Slow);
    }
}
