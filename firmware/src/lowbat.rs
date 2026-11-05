//! The low-battery warning's rules (power.rs feeds it every 2 s; App::tick
//! shows it). The stock's levels (dmm_v2_6.h section 9): "Low battery! Please
//! charge" for 3 s at pct < 17 and again at pct < 8, pct = (Vbat - 3.3) / 0.9;
//! power off at pct < 1 (~3.31 V).
//!
//! The power-off here (the stock's timing was not traced; ours, inferred):
//! `CUT_CONFIRM` reads in a row under 1 %, then `CUT_WARN` more reads of
//! warning (the countdown, 30 s); a read at `CUT_CLEAR` or above ends it, one
//! between 1 % and that holds it; USB power clears everything.
//! The rest is ours, the stock's code path for it was not traced:
//!   - a level counts after `CONFIRM` reads in a row, so a load step (scope
//!     rails, backlight) or one noisy conversion does not warn;
//!   - each level warns once. A battery recovers only on USB power, so what
//!     a load's end gives back does not re-arm it; USB power re-arms both.
//!   - on USB power nothing warns and the battery is not "low";
//!   - a read under 2.5 V is no battery voltage (the cell's protection PCB
//!     cuts off above it, inferred; an unmodelled ADC reads 0 in the
//!     emulator): it does not count.

/// The warning levels, % of the bar scale (stock), highest first.
pub const LEVELS: [f32; 2] = [17.0, 8.0];
/// Under this pct (2.5 V) a read does not count.
pub const NO_READ: f32 = -88.8;
/// Reads in a row under a level before it counts (2 s apart).
pub const CONFIRM: u8 = 5;

/// The power-off level, % (stock).
pub const CUT: f32 = 1.0;
/// Reads in a row under `CUT` before the countdown (30 s).
pub const CUT_CONFIRM: u8 = 15;
/// The countdown, reads (30 s, as the idle power-off's warning).
pub const CUT_WARN: u8 = 15;
/// A read at or above this ends a countdown (a load's end gives this back).
pub const CUT_CLEAR: f32 = 3.0;

#[derive(Clone, Copy, Default)]
pub struct LowBat {
    /// Levels passed: 0 none, 1 under 17 %, 2 under 8 %.
    stage: u8,
    below: u8,
    due: bool,
    /// Reads counted toward the power-off.
    cut: u8,
    /// The countdown's last announced seconds (`take_cut_warn`).
    cut_shown: u8,
}

impl LowBat {
    pub const fn new() -> Self {
        LowBat { stage: 0, below: 0, due: false, cut: 0, cut_shown: 0 }
    }

    /// One battery read: its `pct`, whether there is USB power
    /// (autooff::usb_power).
    pub fn feed(&mut self, pct: f32, charger: bool) {
        if charger {
            *self = Self::new();
            return;
        }
        if pct < NO_READ {
            self.below = 0;
            self.cut = 0;
            self.cut_shown = 0;
            return;
        }
        if pct < CUT {
            self.cut = (self.cut + 1).min(CUT_CONFIRM + CUT_WARN);
        } else if self.cut < CUT_CONFIRM || pct >= CUT_CLEAR {
            self.cut = 0;
            self.cut_shown = 0;
        }
        let want = LEVELS.iter().filter(|&&l| pct < l).count() as u8;
        if want <= self.stage {
            self.below = 0;
            return;
        }
        self.below += 1;
        if self.below >= CONFIRM {
            self.stage = want;
            self.below = 0;
            self.due = true;
        }
    }

    /// Under a warning level: the battery symbol turns red.
    pub fn low(&self) -> bool {
        self.stage > 0
    }

    /// A warning waits to be shown (it stays until `shown`, e.g. through a
    /// recording; two levels passed meanwhile are one warning).
    pub fn due(&self) -> bool {
        self.due
    }

    pub fn shown(&mut self) {
        self.due = false;
    }

    /// The countdown has run out: power off.
    pub fn cut_due(&self) -> bool {
        self.cut >= CUT_CONFIRM + CUT_WARN
    }

    /// The countdown runs.
    pub fn cut_warning(&self) -> bool {
        self.cut >= CUT_CONFIRM
    }

    /// The countdown's seconds to announce, once each: 30, 20, 10.
    pub fn take_cut_warn(&mut self) -> Option<u8> {
        if self.cut < CUT_CONFIRM || self.cut_due() {
            return None;
        }
        let left = (CUT_CONFIRM + CUT_WARN - self.cut) * 2;
        let secs = left.div_ceil(10) * 10;
        if secs == self.cut_shown {
            return None;
        }
        self.cut_shown = secs;
        Some(secs)
    }
}
