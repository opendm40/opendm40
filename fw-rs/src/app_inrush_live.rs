//! Inrush current, the part that runs while the AD2 stream does (region B,
//! never `.cold`: a region C fetch stalls the stream ISR's entry,
//! link-native.x): the ring drain into the capture, the page drawing and
//! its live state line, the SCPI queries a host polls. The rest:
//! app_inrush.rs.

use super::*;
use crate::inrush::{self as inr, Phase, Req, Save};
use crate::scpi::Inr;

/// Append `parts` to `b` (a state line).
pub(super) fn cat<'a>(b: &'a mut [u8; 48], parts: &[&str]) -> &'a str {
    let mut n = 0;
    for p in parts {
        let k = p.len().min(48 - n);
        b[n..n + k].copy_from_slice(&p.as_bytes()[..k]);
        n += k;
    }
    core::str::from_utf8(&b[..n]).unwrap_or("")
}

impl App {
    /// main.rs: what to do with the stream (`inrush::Req`).
    #[inline(always)]
    pub fn take_inrush_req(&mut self) -> Option<Req> {
        inr::ses().req.take()
    }

    /// The fast logger's session is the inrush save (main.rs: its drain
    /// is the trace, the GUI stays live).
    #[inline(always)]
    pub fn inrush_saving(&self) -> bool {
        inr::ses().saving()
    }

    /// Per pass while the stream runs: the AD2 ring into the capture (the
    /// lent BUF); a hole in the stream (ring full / ISR misses) is the
    /// detector's; complete -> the stream stops (main.rs).
    #[inline(never)]
    pub fn inrush_poll(&mut self) {
        let s = inr::ses();
        if !s.streaming || !matches!(s.cap.phase, Phase::Arming | Phase::Armed | Phase::Post) {
            return;
        }
        let Some(buf) = self.scope.lent_buf() else { return };
        let d = crate::logger::wave::dropped();
        if d != s.dropped {
            s.cap.hole(d.wrapping_sub(s.dropped));
            s.dropped = d;
        }
        let cap = &mut s.cap;
        let mut done = false;
        crate::logger::wave::drain(&mut |raw| {
            done |= cap.push(buf, inr::code_of(raw));
        });
        if done {
            s.req = Some(Req::Stop);
        }
    }

    /// The live level (rms of the last 100 ms of the stream), A.
    fn inrush_live_a(&self) -> f32 {
        let s = inr::ses();
        s.cap.live_codes() * s.b
    }

    /// The state line and its colour.
    /// While streaming here (region B); the other states:
    /// app_inrush.rs `inrush_info_idle` (the stream has stopped by then).
    pub(super) fn inrush_info<'a>(&self, b: &'a mut [u8; 48]) -> (&'a str, crate::lcd::Color) {
        let s = inr::ses();
        let mut t = [0u8; 16];
        match s.cap.phase {
            Phase::Arming => ("Arming ...", ui::inr::C_ARMED),
            Phase::Armed => (cat(b, &["Armed: ", ui::inr::amps(&mut t, self.inrush_live_a()), " A now"]), ui::inr::C_ARMED),
            Phase::Post => ("Triggered: recording 2 s ...", ui::inr::C_TRIG),
            _ => self.inrush_info_idle(b),
        }
    }

    /// The main button: (text, colour, enabled).
    pub(super) fn inrush_btn(&self) -> (&'static str, crate::lcd::Color, bool) {
        let s = inr::ses();
        if s.streaming || s.req == Some(Req::Start) {
            ("Cancel", 0xF800, true)
        } else if s.saving() {
            ("Saving ...", ui::BTN, false)
        } else {
            ("Arm", ui::SEL, true)
        }
    }

    /// What the live part shows (phase, save, stop, the live level in mA).
    pub(super) fn inrush_key(&self) -> u32 {
        let s = inr::ses();
        let live = if s.cap.phase == Phase::Armed { (self.inrush_live_a() * 1000.0) as u32 & 0xFFFFF } else { 0 };
        let save = match s.save {
            Save::None => 0,
            Save::Pending | Save::Running => 1,
            Save::Saved(_) => 2,
            Save::Failed(_) => 3,
        };
        (s.cap.phase as u32) | save << 4 | (s.streaming as u32) << 6 | (s.req.is_some() as u32) << 7 | live << 8
    }

    /// Per pass on the page: the state line and the button when they
    /// change (the live level at most 4 times a second).
    #[inline(never)]
    pub(super) fn inrush_tick(&mut self, now: u64) {
        let s = inr::ses();
        if now < s.next_live {
            return;
        }
        let k = self.inrush_key();
        if k == s.shown {
            return;
        }
        s.next_live = now + 250;
        let btn_changed = (k ^ s.shown) & 0xFF != 0;
        s.shown = k;
        let mut nb = [0u8; 48];
        let (t, fg) = self.inrush_info(&mut nb);
        ui::inr::info(t, fg);
        if btn_changed {
            let (b, bg, en) = self.inrush_btn();
            ui::inr::button(b, bg, en);
        }
    }

    /// INR:STATe? / :CONDition? (what a host polls while a capture runs:
    /// region B); the rest in app_inrush.rs `inrush_scpi` (region C).
    /// True: a reply.
    pub fn inrush_query(&mut self, clk: &mut Clock, c: Inr, out: &mut crate::scpi::Scpi) -> bool {
        let s = inr::ses();
        match c {
            Inr::State(None) => {
                out.len = 0;
                out.put_bytes(if s.on { b"1" } else { b"0" });
                true
            }
            Inr::Cond => {
                out.len = 0;
                out.put_bytes(match s.cap.phase {
                    Phase::Arming => b"ARMING",
                    Phase::Armed => b"ARMED",
                    Phase::Post => b"TRIGGERED",
                    _ if s.res.is_some() && s.saving() => b"SAVING",
                    _ if s.res.is_some() => b"DONE",
                    _ if s.on => b"IDLE",
                    _ => b"OFF",
                });
                true
            }
            c => self.inrush_scpi(clk, c, out),
        }
    }
}
