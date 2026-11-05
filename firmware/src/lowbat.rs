//! The low-battery warning's rules (power.rs feeds it every 2 s; App::tick
//! shows it). The stock's levels (dmm_v2_6.h section 9): "Low battery! Please
//! charge" for 3 s at pct < 17 and again at pct < 8, pct = (Vbat - 3.3) / 0.9.
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

#[derive(Clone, Copy, Default)]
pub struct LowBat {
    /// Levels passed: 0 none, 1 under 17 %, 2 under 8 %.
    stage: u8,
    below: u8,
    due: bool,
}

impl LowBat {
    pub const fn new() -> Self {
        LowBat { stage: 0, below: 0, due: false }
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
            return;
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
}
