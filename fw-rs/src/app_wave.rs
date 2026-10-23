//! The DMM wave view (TODO (A), docs/dmm-input-scope-feasibility.md 5.2 /
//! 10): the stock-replica scope page with the source DMM, i.e. the HY3131
//! AD2 stream of the DMM's AC row (a slow, floating, isolated scope on the
//! DMM jacks: V, mV, A, mA, uA). Entered from the MF range page ("Wave",
//! V / A functions) or the scope page's source badge ("MCX | DMM"); left
//! with the badge, MENU or any view change. Needs the stream ISR (Cargo
//! feature `ad2-isr`, decision #3): without it the entries are hidden.
//!
//! Ownership: the stream is a `Logger::start_wave` session run by main.rs
//! (`take_wave_req`: start / restart / range step / stop), so the HY3131
//! belongs to the ISR while the view is live; the DMM poll, BT serve and
//! the sentinel pause (main.rs `busy`) and the DMM row is rewritten on exit
//! (main.rs, the logger's ON -> OFF edge: dmm.start). The record and the
//! trigger: src/scope.rs `WaveSource` (this file implements it over
//! src/logger.rs `wave`).
//!
//! Vertical: V/div = the DMM range (`scope::wave_vdiv`: the range fills
//! the half screen); the V/div buttons step the DMM range (the MF list
//! items). Coupling is the row's: AC V is AC-coupled (~0.5 Hz, the stock
//! row's coupling capacitor), AC mV and AC A / mA / uA are DC-coupled, DC
//! V streams the DC-coupled V row (feasibility 1.3 last row / E7,
//! ad2exp.rs `dc_enter`: the AC V row with AD2 on OP1O, its offset
//! self-calibrated against AD1 at every start) and DC mV the AC mV row.
//! In V the coupling pill switches DC <-> AC (the DMM's DC / AC V at the
//! same range); in A it says the AC A row is DC-coupled.

use super::*;
use crate::scope::{WaveScale, WaveSource};
use crate::trigsoft::StreamCfg;

/// What main.rs does with the stream next (`App::take_wave_req`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaveReq {
    /// (Re)start at `scope.wave_fs()` on the current row.
    Start,
    /// Stop, set this MF range item (dmm.set_range), start again.
    Range(u8),
    /// Stop (the view closed); main.rs restores the DMM.
    Stop,
    /// V: stop, the other coupling (DC <-> AC V, dmm.set_sub) at this MF
    /// range item, start again.
    Coupling(u8),
}

/// The wave view exists in this build (the stream ISR is compiled in).
pub const WAVE_BUILD: bool = cfg!(feature = "ad2-isr");

/// src/logger.rs `wave` as the scope's source.
struct RingSrc;

/// A sample within 2 % of AD2's full scale since the stream's start: the
/// view's autorange steps the range up (`wave_poll`).
static CLIP: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
const CLIP_CODE: i32 = 257_000;

impl WaveSource for RingSrc {
    fn drain(&mut self, f: &mut dyn FnMut(i32)) -> u32 {
        let mut clip = false;
        let n = crate::logger::wave::drain(&mut |v| {
            clip |= v.unsigned_abs() > CLIP_CODE as u32;
            f(v)
        });
        if clip {
            CLIP.store(true, core::sync::atomic::Ordering::Relaxed);
        }
        n
    }
    fn dropped(&self) -> u32 {
        crate::logger::wave::dropped()
    }
    fn set_trigger(&mut self, c: &StreamCfg, min: u32) -> u32 {
        crate::logger::wave::set_trigger(c, min)
    }
    fn trigger_off(&mut self) {
        crate::logger::wave::trigger_off()
    }
    fn events(&mut self, seen: &mut u32, f: &mut dyn FnMut(u32)) {
        crate::logger::wave::events(seen, f)
    }
    fn event_count(&self) -> u32 {
        crate::logger::wave::event_count()
    }
    fn first(&self, gen: u32) -> Option<u32> {
        crate::logger::wave::first(gen)
    }
}

/// Scope scale of a DMM AD2 scale.
fn scale_of(a: dmm::Ad2Scale) -> WaveScale {
    WaveScale::of(a.per_code, a.offset, a.full, a.amps, a.ac)
}

/// MF list length of the wave functions (V: 600 mV .. 1000 V, A: 600 uA
/// .. 10 A; `ui::range_items` without the AUTO entries).
fn items(b: Button) -> u8 {
    if b == Button::Amp { 6 } else { 5 }
}

impl App {
    pub fn take_wave_req(&mut self) -> Option<WaveReq> {
        if self.wave_exit_to.is_some() {
            // An exit is held for the stop (`show`): nothing overrides it.
            self.wave_req = None;
            return Some(WaveReq::Stop);
        }
        self.wave_req.take()
    }

    /// main.rs after a WaveReq::Stop (the stream has stopped): the view
    /// change `show` held back (leaving the live wave view) now.
    pub fn wave_exit_done(&mut self, clk: &mut Clock) {
        if let Some(v) = self.wave_exit_to.take() {
            if v == self.view {
                // The source badge (`wave_badge_close`): MCX on this page.
                self.wave_close(clk, true);
            } else {
                self.show(v, clk);
            }
            self.wave_req = None; // wave_close's Stop: done
        }
    }

    /// The source badge in the wave view (DMM -> MCX). With the stream on,
    /// the close waits for its stop (as `show` does for a view change):
    /// `wave_close` switches the rails to the scope side (the DMM rail off
    /// under the stream) and the MCX page's acquisition and full repaint
    /// would run for a pass while the stream ISR still owns the bus.
    pub(super) fn wave_badge_close(&mut self, clk: &mut Clock) {
        if crate::hy3131::streaming() {
            self.wave_req = Some(WaveReq::Stop);
            self.wave_exit_to = Some(self.view);
        } else {
            self.wave_close(clk, true);
        }
    }

    /// Poll with the ring as the source (scope_poll).
    pub(super) fn wave_poll(&mut self, now: u64) -> Option<scope::Frame<'static>> {
        if let Some(_) = self.scope.take_wave_restart() {
            self.wave_req = Some(WaveReq::Start);
        }
        // Autorange (the DMM was in AUTO at the entry, no range picked
        // since): AD2 clipped -> the next range up. The DMM's autorange
        // chose by the DC (or RMS) reading; a DC V entry with AC on top
        // (AUTO on DC mV, say) clips at once.
        if self.wave_auto.is_some() && self.wave_req.is_none() && CLIP.swap(false, core::sync::atomic::Ordering::Relaxed) {
            if let Some(a) = self.dmm.ad2_scale() {
                if a.item + 1 < items(self.dmm.button()) {
                    self.wave_req = Some(WaveReq::Range(a.item + 1));
                }
            }
        }
        self.scope.poll_src(now, Some(&mut RingSrc))
    }

    /// Open the wave view on the current DMM function (MF "Wave", the
    /// source badge). Refused (toast) where there is no AD2 waveform.
    #[inline(never)] // .cold (link-native.x): on a tap
    pub(super) fn wave_open(&mut self, clk: &mut Clock) {
        let now = clk.now_ms();
        if !WAVE_BUILD {
            return;
        }
        let b = self.dmm.button();
        let why = if !matches!(b, Button::Volt | Button::Amp) || self.dmm.pw.on {
            Some("Wave: V or A function")
        } else if self.dmm.sub() >= 2 {
            // AC+DC / AUTO coupling: the HY3131 row alternates.
            Some("Wave: not in AC+DC/AUTO")
        } else if crate::csvlog::active() {
            Some("Wave: recording runs")
        } else {
            None
        };
        if let Some(t) = why {
            self.toast(now, t, false);
            return;
        }
        // DC A has no AD2 path (AD1 only): the AC A row (DC-coupled). DC V
        // / mV stream their DC rows (E7); AC V is AC-coupled.
        let note = if self.dmm.sub() == 0 && b == Button::Amp {
            self.dmm.set_sub(clk, 1);
            self.func_changed();
            Some("Wave: AC A row (DC-coupled)")
        } else {
            None
        };
        // A fixed range while streaming (the DMM autorange does not run):
        // the current one, restored to AUTO / AUTO+ on exit.
        let Some(a) = self.dmm.ad2_scale() else {
            self.toast(now, "Wave: no AD2 row here", false);
            return;
        };
        self.wave_auto = if self.dmm.is_auto() {
            Some(if b == Button::Volt { self.dmm.v_auto() } else { self.dmm.a_auto() })
        } else {
            None
        };
        if self.dmm.is_auto() {
            self.dmm.set_range(clk, Some(a.item));
        }
        let Some(a) = self.dmm.ad2_scale() else { return };
        self.meter.rel_off();
        self.scope.wave_enter(scale_of(a), now);
        crate::scope_ui::set_tb_lo(scope::WAVE_TB_MIN);
        crate::scope_ui::graph().clear();
        self.wave_req = None; // the scope's restart flag asks for the start
        if self.view != View::Scope {
            self.show(View::Scope, clk);
        } else {
            self.update_rails(clk);
            self.redraw = true;
        }
        if let Some(t) = note {
            self.toast(now, t, true);
        }
    }

    /// Close the wave view (MCX again; the stream stops in main.rs, which
    /// then restores the DMM row). The autorange comes back. `rails` false:
    /// the caller (`show`, leaving the Scope view) sets the rails for the
    /// view it shows; the wave view's DMM rail must not drop in between (a
    /// MENU from the wave view keeps the DMM measuring, as the menu keeps
    /// whatever rail is on; a drop + wake started the DMM's AC node over).
    pub(super) fn wave_close(&mut self, clk: &mut Clock, rails: bool) {
        if !self.scope.wave_on() {
            return;
        }
        let now = clk.now_ms();
        self.scope.wave_exit(now);
        crate::scope_ui::set_tb_lo(scope::TB_MIN);
        crate::scope_ui::graph().clear();
        self.wave_req = Some(WaveReq::Stop);
        if let Some(v) = self.wave_auto.take() {
            self.dmm.set_range(clk, if v == 1 { Some(255) } else { None });
        }
        self.meter.reset();
        if rails {
            self.update_rails(clk);
        }
        self.redraw = true;
    }

    /// main.rs, a range / coupling step with the stream stopped, before it
    /// starts again: the new scale, and the full repaint it needs (V/div,
    /// the unit, the coupling) now, while nothing streams (`draw_ok`: the
    /// main loop may draw). At 76800 S/s the ring holds 27 ms, less than
    /// the scope page's repaint: run with the new stream on, it overflowed
    /// the ring and the record restarted (wavetest V/div at 2 / 1 ms/div).
    /// `wave_live` then finds the scale unchanged.
    #[inline(never)] // not inlined into run (stack depth)
    pub fn wave_rescale(&mut self, draw_ok: bool) {
        let Some(a) = self.dmm.ad2_scale() else { return };
        let s = scale_of(a);
        let now = self.scope.wave_scale_now();
        if s == now {
            return;
        }
        self.scope.wave_scale(s);
        if (s.vdiv, s.amps, s.ac) != (now.vdiv, now.amps, now.ac) && self.view == View::Scope {
            if draw_ok && self.overlay.is_none() {
                self.repaint(self.now);
            } else {
                self.redraw = true;
            }
        }
    }

    /// main.rs: the stream runs at `scope.wave_fs()` on the current row.
    pub fn wave_live(&mut self) {
        CLIP.store(false, core::sync::atomic::Ordering::Relaxed);
        if let Some(a) = self.dmm.ad2_scale() {
            let s = scale_of(a);
            let now = self.scope.wave_scale_now();
            if s != now {
                self.scope.wave_scale(s);
                // A full repaint only for what it shows (V/div, the unit,
                // the coupling): it runs with the stream on, and at 38400
                // S/s the ring holds ~50 ms. A DC V start's self-cal moves
                // only a / b (device 5e52761: `overwritten` +7087 at every
                // DC V entry, the repaint after the new cal).
                if (s.vdiv, s.amps, s.ac) != (now.vdiv, now.amps, now.ac) {
                    self.redraw = self.redraw || self.view == View::Scope;
                }
            }
        }
        self.scope.wave_live();
        self.scope.refresh(); // stopped: the stored record at the new V/div
        self.sp_settings_changed();
    }

    /// main.rs: the stream died (a main-loop HY3131 access: a host CONF,
    /// the power key's sub-mode cycle) or could not start: the view closes
    /// with `why`.
    pub fn wave_lost(&mut self, clk: &mut Clock, why: &str) {
        if !self.scope.wave_on() {
            return;
        }
        self.scope.wave_dead();
        self.wave_close(clk, true);
        self.wave_req = None; // main.rs already stopped it
        let now = clk.now_ms();
        self.toast(now, why, false);
    }

    /// V/div + / - in the wave view: the next DMM range (`smaller` = the
    /// lower range, zoom in).
    pub(super) fn wave_range_step(&mut self, smaller: bool, now: u64) {
        let Some(a) = self.dmm.ad2_scale() else { return };
        let n = items(self.dmm.button());
        let i = if smaller { a.item.checked_sub(1) } else { (a.item + 1 < n).then_some(a.item + 1) };
        match i {
            Some(i) => {
                self.wave_req = Some(WaveReq::Range(i));
                self.wave_auto = None; // the user picked a range
            }
            None => self.toast(now, if smaller { "Lowest range" } else { "Highest range" }, false),
        }
    }

    /// The coupling pill / CH1 label in the wave view: V switches DC <->
    /// AC (main.rs restarts the stream in the other row at the same range
    /// item); A has one row (the AC A row, DC-coupled): a note.
    pub(super) fn wave_coupling_note(&mut self, now: u64) {
        if self.dmm.button() == Button::Volt {
            let Some(a) = self.dmm.ad2_scale() else { return };
            let ac = self.dmm.sub() == 1;
            self.wave_req = Some(WaveReq::Coupling(a.item));
            self.toast(now, if ac { "DC coupling (DC V row)" } else { "AC coupling (AC V row)" }, true);
        } else {
            self.toast(now, "DC-coupled AC row", true);
        }
    }

    /// main.rs, WaveReq::Coupling (the stream stopped): V DC <-> AC at
    /// MF range item `item`.
    pub fn wave_coupling(&mut self, clk: &mut Clock, item: u8) {
        let sub = self.dmm.sub();
        self.dmm.set_sub(clk, if sub == 1 { 0 } else { 1 });
        self.func_changed();
        self.dmm.set_range(clk, Some(item));
        self.redraw = true;
    }

    /// Function / range label of the wave view ("AC V", "AC mA").
    pub(super) fn wave_func_label(&self) -> &'static str {
        match self.dmm.hy_mode() {
            0 => "AC V",
            1 => "DC V",
            2 => "AC mV",
            3 => "DC mV",
            4 => "AC A",
            5 => "AC mA",
            6 => "AC uA",
            _ => "DMM",
        }
    }
}
