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

use crate::dmm::{Reading, Val};

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

// ------------------------------------------------------------ AutoHold
//
// FR-01 / Appendix B, on the raw (pre-REL) value: a window of readings that
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
    /// The live reading (REL applied): SCPI READ? / MEAS?, chart, HV warning.
    pub live: Reading,
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
        }
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
        let mut d = *r;
        let k = if r.main.v != 0.0 { r.si / r.main.v } else { unit_scale(r.main.unit) };
        if let (Some((ref_v, _)), false) = (self.rel, d.main.ol) {
            d.main.v -= ref_v;
            d.si = d.main.v * k;
        }
        self.live = Some(d);
        let mut u = Update { live: d, show: None, captured: false, fail_edge: false };
        if self.hold {
            return u;
        }
        if !d.main.ol {
            self.stats.add(d.main.v * unit_scale(d.main.unit), now);
        }
        let show = if !self.ahold {
            Some(d)
        } else if let Some(mean) = self.ah_step(r, now) {
            // Freeze the window mean (REL applied as for the live value).
            let mut h = d;
            h.main.v = mean - (r.main.v - d.main.v);
            h.si = h.main.v * k;
            self.ah_held = Some((h, mean * unit_scale(r.main.unit)));
            u.captured = true;
            Some(h)
        } else if self.ah_held.is_some() {
            None
        } else {
            Some(d)
        };
        if let Some(s) = show {
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

    /// REL on: reference = the current reading (not while HOLD or OL).
    /// Returns false if refused.
    pub fn rel_on(&mut self, raw: Option<&Reading>) -> bool {
        match raw {
            Some(r) if !self.hold && !r.main.ol => {
                self.rel = Some((r.main.v, r.main.unit));
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
    }
}

/// Display text for a value ("0L" when over range).
pub fn text<'a>(buf: &'a mut [u8; 16], v: &Val) -> &'a str {
    if v.ol { "0L" } else { crate::dmm::format(buf, v.v, v.dec) }
}
