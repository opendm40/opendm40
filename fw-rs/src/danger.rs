//! Danger alarm (docs/power-meter-and-misconnect.md 4.10): a loud two-tone
//! pattern while a wrong connection that can damage the meter or the
//! circuit is in force (A-PROT with current still flowing, A-JACK, a
//! current above the 10 A rating), at maximum buzzer duty regardless of the
//! key / alert volume (owner 2026-10-24: the owner keeps the volume at 0).
//!
//! Pure state (host tests below); `app.rs` decides what is dangerous
//! (`App::danger_now`), feeds `Alarm::feed` once per main-loop pass and
//! drives TIM14 with the returned tone (`App::alert_beep`).
//!
//! Rules:
//! * an event starts when the condition turns on; it sounds at once;
//! * a tap on the alert's badge / toast or any side key silences the event
//!   (`silence`); the keys still do their own job (stock: every key press
//!   beeps and acts, there is no cancel key);
//! * the condition gone ends the event at once (the next pass); a new one
//!   sounds again even if the last was silenced;
//! * cap: the full pattern for FULL_MS, then one burst every REMIND_MS
//!   until the event ends or is silenced (an A-PROT latch outside DC V / mV
//!   holds the condition until the user acts).

/// Settings 3/3 "Danger alarm" (persisted, meter::EXT_DANGER_SHIFT; 0 =
/// Loud, the default of every older record).
pub const LOUD: u8 = 0;
pub const VOLUME: u8 = 1;
pub const OFF: u8 = 2;
pub const NAMES: [&str; 3] = ["Loud", "Volume", "Off"];
/// SCPI SYSTem:ALERt:ALARm? answers.
pub const SCPI_NAMES: [&str; 3] = ["LOUD", "VOLUME", "OFF"];

/// One tone step of a burst (ms): high, low, high, low.
pub const STEP_MS: u64 = 125;
/// Tone steps per burst (500 ms of sound).
pub const STEPS: u64 = 4;
/// Burst period during the first FULL_MS of an event (500 ms on, 500 off).
pub const PERIOD_MS: u64 = 1000;
/// Full pattern this long from the start of an event ...
pub const FULL_MS: u64 = 30_000;
/// ... then one burst per this period.
pub const REMIND_MS: u64 = 10_000;
/// A current above the 10 A rating counts after this long (inrush peaks of
/// a motor / lamp / capacitor load are the fuse's normal life).
pub const I10_ASSERT_MS: u64 = 1000;
/// The 10 A rating (DM40 manual: 10 A max, one 10 A ceramic fuse).
pub const I_RATED: f32 = 10.0;

/// Tone of the pattern `dt` ms into an event: 0 silent, 1 high, 2 low.
pub fn tone_at(dt: u64) -> u8 {
    let ph = if dt < FULL_MS { dt % PERIOD_MS } else { (dt - FULL_MS) % REMIND_MS };
    if ph >= STEP_MS * STEPS {
        0
    } else if (ph / STEP_MS) % 2 == 0 {
        1
    } else {
        2
    }
}

/// What the buzzer should do this pass.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    /// Not the alarm's: leave the buzzer to the other beeps.
    Idle,
    /// The alarm just stopped: buzzer off, the stock pitch back.
    Stop,
    /// The alarm owns the buzzer: this tone (0 = the gap between tones).
    Tone(u8),
}

#[derive(Clone, Copy)]
pub struct Alarm {
    /// Start of the event in force (None: no dangerous condition).
    t0: Option<u64>,
    /// The event was silenced by the user.
    quiet: bool,
    /// The last `feed` drove the buzzer.
    driving: bool,
    /// Events so far (DBG:ALARm?).
    pub events: u32,
}

impl Alarm {
    pub const NEW: Alarm = Alarm { t0: None, quiet: false, driving: false, events: 0 };

    /// One pass: `active` = a dangerous condition now, `on` = the setting
    /// is not Off.
    pub fn feed(&mut self, now: u64, active: bool, on: bool) -> Out {
        match (self.t0, active) {
            (None, true) => {
                self.t0 = Some(now);
                self.quiet = false;
                self.events = self.events.wrapping_add(1);
            }
            (Some(_), false) => self.t0 = None,
            _ => {}
        }
        let drive = on && !self.quiet && self.t0.is_some();
        let was = core::mem::replace(&mut self.driving, drive);
        match (drive, self.t0) {
            (true, Some(t0)) => Out::Tone(tone_at(now.saturating_sub(t0))),
            _ if was => Out::Stop,
            _ => Out::Idle,
        }
    }

    /// The user silenced the alarm (badge / toast tap, a side key): true
    /// when an event was sounding.
    pub fn silence(&mut self) -> bool {
        let s = self.driving && !self.quiet;
        if self.t0.is_some() {
            self.quiet = true;
        }
        s
    }

    /// The alarm drives the buzzer (the last pass).
    pub fn sounding(&self) -> bool {
        self.driving
    }

    /// An event is in force, silenced.
    pub fn silenced(&self) -> bool {
        self.t0.is_some() && self.quiet
    }
}

/// A reading above the 10 A rating: the DMM current functions (DC / AC /
/// AC+DC A; OL on the 10 A range itself, DC mode 7 / AC mode 4 range 1) or
/// the power meter's I channel on its 10 A range (ik 5). OL on a lower
/// range (a manual 6 A, the uA / mA shunts) says nothing about the amps.
pub fn over_rating(mode: u8, range: i8, ol: bool, si: f32, power: Option<(u8, bool, f32)>) -> bool {
    match power {
        Some((ik, i_ol, i)) => ik == 5 && (i_ol || i.abs() > I_RATED),
        None if (4..=9).contains(&mode) => {
            if ol {
                matches!(mode, 4 | 7) && range == 1
            } else {
                si.abs() > I_RATED
            }
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_two_tone_then_reminders() {
        assert_eq!(tone_at(0), 1);
        assert_eq!(tone_at(124), 1);
        assert_eq!(tone_at(125), 2);
        assert_eq!(tone_at(250), 1);
        assert_eq!(tone_at(375), 2);
        assert_eq!(tone_at(499), 2);
        assert_eq!(tone_at(500), 0);
        assert_eq!(tone_at(999), 0);
        assert_eq!(tone_at(1000), 1);
        assert_eq!(tone_at(29_999), 0);
        // After FULL_MS: one burst per REMIND_MS.
        assert_eq!(tone_at(30_000), 1);
        assert_eq!(tone_at(30_400), 2);
        assert_eq!(tone_at(30_500), 0);
        assert_eq!(tone_at(31_000), 0);
        assert_eq!(tone_at(39_999), 0);
        assert_eq!(tone_at(40_000), 1);
        // Sound duty: 50 % in the full phase, 5 % after it.
        let on = |a: u64, b: u64| (a..b).filter(|&t| tone_at(t) != 0).count() as u64;
        assert_eq!(on(0, FULL_MS), FULL_MS / 2);
        assert_eq!(on(FULL_MS, FULL_MS + 10 * REMIND_MS), 10 * 500);
    }

    #[test]
    fn event_silence_rearm_stop() {
        let mut a = Alarm::NEW;
        assert_eq!(a.feed(0, false, true), Out::Idle);
        assert_eq!(a.feed(10, true, true), Out::Tone(1));
        assert!(a.sounding());
        assert_eq!(a.feed(140, true, true), Out::Tone(2));
        // Silenced: one Stop, then Idle while the condition lasts.
        assert!(a.silence());
        assert_eq!(a.feed(150, true, true), Out::Stop);
        assert_eq!(a.feed(1010, true, true), Out::Idle);
        assert!(a.silenced());
        assert!(!a.silence());
        // Cleared and back: sounds again.
        assert_eq!(a.feed(2000, false, true), Out::Idle);
        assert_eq!(a.feed(2100, true, true), Out::Tone(1));
        assert_eq!(a.events, 2);
        // The condition gone stops it at once.
        assert_eq!(a.feed(2200, false, true), Out::Stop);
        assert_eq!(a.feed(2210, false, true), Out::Idle);
    }

    #[test]
    fn setting_off_and_switching() {
        let mut a = Alarm::NEW;
        assert_eq!(a.feed(0, true, false), Out::Idle);
        assert!(!a.sounding());
        // Turned on during the event: sounds, at the pattern's phase.
        assert_eq!(a.feed(600, true, true), Out::Tone(0));
        assert_eq!(a.feed(1000, true, true), Out::Tone(1));
        // Turned off: stops.
        assert_eq!(a.feed(1100, true, false), Out::Stop);
        assert_eq!(a.feed(1200, true, false), Out::Idle);
        // Silence with nothing in force: no effect on the next event.
        let mut b = Alarm::NEW;
        assert!(!b.silence());
        assert_eq!(b.feed(5, true, true), Out::Tone(1));
    }

    #[test]
    fn over_rating_rule() {
        // DC A 10 A range (mode 7 r1): OL or > 10 A.
        assert!(over_rating(7, 1, true, 0.0, None));
        assert!(over_rating(7, 1, false, 10.5, None));
        assert!(!over_rating(7, 1, false, 9.9, None));
        // 6 A range OL (manual): unknown amps, not counted.
        assert!(!over_rating(7, 0, true, 0.0, None));
        // AC A 10 A range (mode 4 r1); AC+DC readings carry the DC mode.
        assert!(over_rating(4, 1, true, 0.0, None));
        assert!(over_rating(4, 1, false, -11.0, None));
        // uA / mA ranges: OL there is the OVERLOAD latch's business.
        assert!(!over_rating(8, 0, true, 0.0, None));
        assert!(!over_rating(9, 1, true, 0.0, None));
        // Not a current function.
        assert!(!over_rating(1, 0, true, 50.0, None));
        // Power meter: ik 5 only.
        assert!(over_rating(5, 1, false, 0.0, Some((5, true, 0.0))));
        assert!(over_rating(5, 1, false, 0.0, Some((5, false, 10.2))));
        assert!(!over_rating(5, 1, false, 0.0, Some((4, true, 0.0))));
        assert!(!over_rating(5, 1, false, 0.0, Some((5, false, 9.0))));
    }
}
