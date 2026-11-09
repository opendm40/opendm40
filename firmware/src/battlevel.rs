//! The battery level as shown (pure; power.rs feeds it every 2 s).
//!
//! On a charger the battery voltage reads higher than the charge held: the
//! charge current rises it by RISE_V. The stock reads during a low pulse on
//! PB10, but that pulse ends the charge above the recharge threshold
//! (hw/README.md Unknowns 5), so PB10 stays alone here. Instead, on a charger:
//! the level never falls; it rises one bar at a time, not sooner than UP_POLLS
//! after the plug or the last rise, once the voltage less RISE_V says so; five
//! bars when the charger reports full. After an unplug the reading settles for
//! SETTLE_POLLS, then the plain battery rule applies. Booted on a charger: the
//! voltage less RISE_V.

/// The charge current's rise of the reading, volts. OpenDM40-Chinese measured
/// 189 mV (4155 mV charging, 3966 mV with the cable pulled); ~65 mV near full
/// (hw/README.md Unknowns 5). An allowance for the high end: a level low on a
/// charger corrects itself at the unplug, a level too high is the bug.
pub const RISE_V: f32 = 0.20;
/// A bar takes at least this long on a charger, polls of 2 s (15 min): the
/// rise right after a plug is not known well enough to act on it.
pub const UP_POLLS: u16 = 450;
/// Readings skipped after an unplug while the battery relaxes (4 s; the
/// relaxation's time is not measured).
pub const SETTLE_POLLS: u8 = 2;

/// Charge at `v` volts, %.
fn pct(v: f32) -> f32 {
    (v - 3.3) / 0.9 * 100.0
}

/// Bars 0..5 at 85 / 70 / 50 / 25 / 0 % from `b`, with 5 % hysteresis upward.
fn step(mut b: u8, pct: f32) -> u8 {
    let th = [0.0, 25.0, 50.0, 70.0, 85.0];
    while b < 5 && pct > th[b as usize] + 5.0 {
        b += 1;
    }
    while b > 0 && pct < th[b as usize - 1] {
        b -= 1;
    }
    b
}

pub struct Level {
    pub bars: u8,
    known: bool,
    settle: u8,
    up_wait: u16,
}

impl Level {
    pub const fn new() -> Self {
        Level { bars: 5, known: false, settle: 0, up_wait: UP_POLLS }
    }

    /// One reading: `v` volts, `pins` as `Power::pins` (bit 0 input power,
    /// bit 1 STDBY high: 1 = charged, 3 = charging).
    pub fn feed(&mut self, v: f32, pins: u8) {
        let known = core::mem::replace(&mut self.known, true);
        if pins & 1 == 0 {
            self.up_wait = UP_POLLS;
            if known && self.settle > 0 {
                self.settle -= 1;
            } else {
                self.bars = step(self.bars, pct(v));
            }
            return;
        }
        self.settle = SETTLE_POLLS;
        let held = pct(v - RISE_V);
        if pins == 1 {
            self.bars = 5;
        } else if !known {
            self.bars = step(self.bars, held);
        } else if self.up_wait > 0 {
            self.up_wait -= 1;
        } else if self.bars < 5 && step(self.bars, held) > self.bars {
            self.bars += 1;
            self.up_wait = UP_POLLS;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHARGING: u8 = 3;
    const FULL: u8 = 1;
    const OFF: u8 = 2;

    fn on_battery(v: f32) -> Level {
        let mut l = Level::new();
        l.feed(v, OFF);
        l
    }

    #[test]
    fn battery_rule() {
        // 3.966 V = 74 %: 4 bars from above.
        assert_eq!(on_battery(3.966).bars, 4);
        assert_eq!(on_battery(4.15).bars, 5);
        assert_eq!(on_battery(3.3).bars, 1);
        assert_eq!(on_battery(3.2).bars, 0);
    }

    #[test]
    fn plug_in_no_jump() {
        // OpenDM40-Chinese's numbers: 3966 mV, 4155 mV on the charger.
        let mut l = on_battery(3.966);
        assert_eq!(l.bars, 4);
        // The rest of the voltage is the charge current: still 4 (3.955 V =
        // 72.8 %, under the 90 % of the next bar).
        for _ in 0..2000 {
            l.feed(4.155, CHARGING);
            assert_eq!(l.bars, 4);
        }
    }

    #[test]
    fn rise_is_slow_and_one_bar_at_a_time() {
        let mut l = on_battery(3.6);
        assert_eq!(l.bars, 2);
        // A voltage far up at once: the bars come one per UP_POLLS.
        for _ in 0..UP_POLLS {
            l.feed(4.2 + RISE_V, CHARGING);
        }
        assert_eq!(l.bars, 2);
        l.feed(4.2 + RISE_V, CHARGING);
        assert_eq!(l.bars, 3);
        for _ in 0..UP_POLLS {
            l.feed(4.2 + RISE_V, CHARGING);
            assert_eq!(l.bars, 3);
        }
        l.feed(4.2 + RISE_V, CHARGING);
        assert_eq!(l.bars, 4);
    }

    #[test]
    fn never_falls_on_charger_full_gives_five() {
        let mut l = on_battery(4.0);
        let b = l.bars;
        for _ in 0..1000 {
            l.feed(3.5, CHARGING);
            assert!(l.bars >= b);
        }
        l.feed(4.2, FULL);
        assert_eq!(l.bars, 5);
    }

    #[test]
    fn unplug_settles_then_the_battery_rule() {
        let mut l = on_battery(3.966);
        for _ in 0..20 {
            l.feed(4.155, CHARGING);
        }
        // Falling back to the real value within seconds, up or down.
        l.feed(4.15, OFF);
        l.feed(4.05, OFF);
        assert_eq!(l.bars, 4);
        l.feed(3.6, OFF);
        assert_eq!(l.bars, 2);
        // A real charge gained shows after the unplug.
        let mut l = on_battery(3.6);
        l.feed(4.15, CHARGING);
        l.feed(4.15, OFF);
        l.feed(4.15, OFF);
        assert_eq!(l.bars, 2);
        l.feed(4.15, OFF);
        assert_eq!(l.bars, 5);
    }

    #[test]
    fn boot_on_charger() {
        // The voltage less the rise, also downward from the start value.
        let mut l = Level::new();
        l.feed(3.8, CHARGING);
        assert_eq!(l.bars, 2);
        let mut l = Level::new();
        l.feed(4.15, CHARGING);
        assert_eq!(l.bars, 4);
        let mut l = Level::new();
        l.feed(4.2, FULL);
        assert_eq!(l.bars, 5);
    }

    #[test]
    fn boot_on_battery_unplug_at_once() {
        // No settling before a first value exists.
        let mut l = Level::new();
        l.feed(3.6, OFF);
        assert_eq!(l.bars, 2);
    }
}
