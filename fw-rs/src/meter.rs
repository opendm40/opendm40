//! What the user sees on top of the raw DMM readings: HOLD, REL, MIN/MAX.
//! Behaviour from dmm_v2_6.h section 8 and bt_meas_v2_6.h (BT commands 03..05).

use crate::dmm::{Reading, Val};

pub struct Meter {
    pub hold: bool,
    /// REL reference in display units of `rel_unit`.
    rel: Option<(f32, &'static str)>,
    shown: Option<Reading>,
    pub min: Option<f32>,
    pub max: Option<f32>,
    unit: &'static str,
}

impl Meter {
    pub const fn new() -> Self {
        Meter { hold: false, rel: None, shown: None, min: None, max: None, unit: "" }
    }

    pub fn rel_active(&self) -> bool {
        self.rel.is_some()
    }

    pub fn rel_value(&self) -> Option<f32> {
        self.rel.map(|r| r.0)
    }

    /// The reading as displayed (after REL), frozen while HOLD.
    pub fn shown(&self) -> Option<&Reading> {
        self.shown.as_ref()
    }

    /// Feed a new reading; returns the displayed one if it changed.
    pub fn update(&mut self, r: &Reading) -> Option<Reading> {
        if self.hold {
            return None;
        }
        if r.main.unit != self.unit {
            // Unit / mode change: REL and MIN/MAX no longer apply.
            self.unit = r.main.unit;
            self.min = None;
            self.max = None;
            if self.rel.map(|x| x.1 != r.main.unit).unwrap_or(false) {
                self.rel = None;
            }
        }
        let mut d = *r;
        if let (Some((ref_v, _)), false) = (self.rel, d.main.ol) {
            d.main.v -= ref_v;
            let k = if r.main.v != 0.0 { r.si / r.main.v } else { 0.0 };
            d.si = d.main.v * k;
        }
        if !d.main.ol {
            let v = d.main.v;
            self.min = Some(self.min.map_or(v, |m| m.min(v)));
            self.max = Some(self.max.map_or(v, |m| m.max(v)));
        }
        self.shown = Some(d);
        Some(d)
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
                self.min = None;
                self.max = None;
                true
            }
            _ => false,
        }
    }

    pub fn rel_off(&mut self) {
        self.rel = None;
        self.min = None;
        self.max = None;
    }

    /// Clear everything (mode change, BT command 03).
    pub fn reset(&mut self) {
        self.hold = false;
        self.rel_off();
        self.shown = None;
    }
}

/// Display text for a value ("0L" when over range).
pub fn text<'a>(buf: &'a mut [u8; 16], v: &Val) -> &'a str {
    if v.ol { "0L" } else { crate::dmm::format(buf, v.v, v.dec) }
}
