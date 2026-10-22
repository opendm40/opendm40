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
//! row's coupling capacitor), AC mV and AC A / mA / uA are DC-coupled; a
//! DC-coupled V row does not exist yet (feasibility 1.3, needs device test
//! 6.5): the coupling pill says so instead of switching.

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
}

/// The wave view exists in this build (the stream ISR is compiled in).
pub const WAVE_BUILD: bool = cfg!(feature = "ad2-isr");

/// src/logger.rs `wave` as the scope's source.
struct RingSrc;

impl WaveSource for RingSrc {
    fn drain(&mut self, f: &mut dyn FnMut(i32)) -> u32 {
        crate::logger::wave::drain(f)
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
        self.wave_req.take()
    }

    /// Poll with the ring as the source (scope_poll).
    pub(super) fn wave_poll(&mut self, now: u64) -> Option<scope::Frame<'static>> {
        if let Some(_) = self.scope.take_wave_restart() {
            self.wave_req = Some(WaveReq::Start);
        }
        self.scope.poll_src(now, Some(&mut RingSrc))
    }

    /// Open the wave view on the current DMM function (MF "Wave", the
    /// source badge). Refused (toast) where there is no AD2 waveform.
    pub(super) fn wave_open(&mut self, clk: &mut Clock) {
        let now = clk.now_ms();
        if !WAVE_BUILD {
            return;
        }
        let b = self.dmm.button();
        let why = if !matches!(b, Button::Volt | Button::Amp) || self.dmm.pw.on {
            Some("Wave: V or A function")
        } else if self.dmm.sub() == 2 {
            Some("Wave: not in AC+DC")
        } else if crate::csvlog::active() {
            Some("Wave: recording runs")
        } else {
            None
        };
        if let Some(t) = why {
            self.toast(now, t, false);
            return;
        }
        // The AC row of the function: DC V / mV / A have no AD2 path (AD1
        // only); AC V is AC-coupled, AC mV / A DC-coupled.
        let note = if self.dmm.sub() == 0 {
            self.dmm.set_sub(clk, 1);
            self.func_changed();
            Some(if b == Button::Volt { "DC V: no DC row on AD2, AC-coupled" } else { "Wave: AC A row (DC-coupled)" })
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
    /// then restores the DMM row). The autorange comes back.
    pub(super) fn wave_close(&mut self, clk: &mut Clock) {
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
        self.update_rails(clk);
        self.redraw = true;
    }

    /// main.rs: the stream runs at `scope.wave_fs()` on the current row.
    pub fn wave_live(&mut self) {
        if let Some(a) = self.dmm.ad2_scale() {
            let s = scale_of(a);
            if s != self.scope.wave_scale_now() {
                self.scope.wave_scale(s);
                self.redraw = self.redraw || self.view == View::Scope;
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
        self.wave_close(clk);
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

    /// The coupling pill / CH1 label in the wave view: what the row does
    /// (there is no choice: one AD2 row per function).
    pub(super) fn wave_coupling_note(&mut self, now: u64) {
        let ac = self.scope.wave_scale_now().ac;
        self.toast(now, if ac { "AC-coupled (no DC V row on AD2)" } else { "DC-coupled AC row" }, true);
    }

    /// Function / range label of the wave view ("AC V", "AC mA").
    pub(super) fn wave_func_label(&self) -> &'static str {
        match self.dmm.hy_mode() {
            0 => "AC V",
            2 => "AC mV",
            4 => "AC A",
            5 => "AC mA",
            6 => "AC uA",
            _ => "DMM",
        }
    }
}
