//! Peak +/- and crest factor: the start and the end (region C,
//! link-native.x: they run with no AD2 stream, before main.rs starts it
//! and after it stopped). The live part: app_peak_live.rs.

use super::app_peak_live::{pk, REQ_NONE, REQ_START};
use super::*;

impl App {
    /// Start Peak on the current DMM function (MF "Peak", CALC:PEAK:STAT
    /// ON): the range fixed, DC A on the AC A row (DC-coupled), the stream
    /// asked for (main.rs). Err: why not (a toast).
    #[inline(never)] // .cold (link-native.x): on a tap / a SCPI set
    pub(super) fn peak_open(&mut self, clk: &mut Clock) -> Result<(), &'static str> {
        let b = self.dmm.button();
        if !app_wave::WAVE_BUILD {
            return Err("Peak: not in this build");
        } else if !matches!(self.view, View::Dmm | View::Range) {
            return Err("Peak: DMM page only");
        } else if !matches!(b, Button::Volt | Button::Amp) || self.dmm.pw.on {
            return Err("Peak: V or A function");
        } else if self.dmm.sub() >= 2 {
            return Err("Peak: not in AC+DC/AUTO");
        } else if crate::csvlog::active() {
            return Err("Peak: recording runs");
        }
        let p = pk();
        // DC A has no AD2 path (AD1 only): the AC A row, DC-coupled, as the
        // wave view; DC A again at the end.
        p.dc_amp = self.dmm.sub() == 0 && b == Button::Amp;
        if p.dc_amp {
            self.dmm.set_sub(clk, 1);
            self.func_changed();
        }
        let Some(a) = self.dmm.ad2_scale() else {
            if p.dc_amp {
                self.dmm.set_sub(clk, 0);
                self.func_changed();
            }
            return Err("Peak: no AD2 row here");
        };
        // A fixed range while streaming (no autorange runs): the current
        // one, AUTO / AUTO+ again at the end.
        p.auto = if self.dmm.is_auto() { 1 + if b == Button::Volt { self.dmm.v_auto() } else { self.dmm.a_auto() } } else { 0 };
        if self.dmm.is_auto() {
            self.dmm.set_range(clk, Some(a.item));
        }
        p.acc.clear();
        p.on = true;
        p.live = false;
        p.exit = false;
        p.shown_n = u32::MAX;
        p.req = REQ_START;
        self.redraw_peak();
        Ok(())
    }

    /// MF "Peak": on, back to the DMM page (the stream starts there); a
    /// refusal is a toast.
    #[inline(never)] // .cold (link-native.x): on a tap
    pub(super) fn peak_tap(&mut self, clk: &mut Clock) {
        let now = clk.now_ms();
        match self.peak_open(clk) {
            Ok(()) => {
                self.show(View::Dmm, clk);
                self.toast(now, "Peak: reading paused", true);
            }
            Err(t) => self.toast(now, t, false),
        }
    }

    /// main.rs: the stream stopped on REQ_STOP (SCPI OFF, a view change
    /// held by `peak_hold_view`): Peak ends, a held view change runs.
    #[inline(never)] // .cold (link-native.x): the stream has stopped
    pub fn peak_stopped(&mut self, clk: &mut Clock) {
        let p = pk();
        let to = if p.exit { Some(p.exit_to) } else { None };
        self.peak_end(clk, None);
        if let Some(v) = to {
            self.show(v, clk);
        }
    }

    /// End Peak (no stream runs): the range / DC A come back unless the
    /// DMM was changed (`why`: the stream was lost, a toast says so).
    #[inline(never)] // .cold (link-native.x): no stream runs
    pub(super) fn peak_end(&mut self, clk: &mut Clock, why: Option<&'static str>) {
        let p = pk();
        if !p.on {
            return;
        }
        let (auto, dc_amp) = (p.auto, p.dc_amp);
        p.on = false;
        p.live = false;
        p.exit = false;
        p.req = REQ_NONE;
        p.auto = 0;
        p.dc_amp = false;
        if why.is_none() {
            if dc_amp && self.dmm.button() == Button::Amp && self.dmm.sub() == 1 {
                self.dmm.set_sub(clk, 0);
                self.func_changed();
            }
            if auto != 0 {
                self.dmm.set_range(clk, if auto == 2 { Some(255) } else { None });
            }
        }
        self.redraw_peak();
        if let Some(t) = why {
            let now = clk.now_ms();
            self.toast(now, t, false);
        }
    }
}
