//! Peak +/- and crest factor (src/peak.rs), the part that runs while the
//! AD2 stream does (region B, never `.cold`: a region C fetch stalls the
//! stream ISR's entry, link-native.x): the ring drain, the side column's
//! values, the stop on a view change, the SCPI queries. The start (MF
//! "Peak", CALC:PEAK:STAT ON) and the end: app_peak.rs (region C, run
//! with no stream).
//!
//! While Peak is on the DMM page stays (View::Dmm), the range is fixed
//! and the AD2 stream of the function's AC row runs (as the wave view's,
//! main.rs `peak_service`): the AD1 reading is paused (the digits grey,
//! "PEAK" in the RUN / HOLD slot) and the side column shows P+, CF, P-
//! in place of MAX, AVG, MIN. Any view change, function or range change
//! ends it.

use super::*;
use crate::peak::Acc;

/// The stream's rate (S/s): the CH2 rung (app_ch2.rs CH2_MAX_FS, a ring of
/// ~53 ms); 26 us per sample, a 2-sample peak >= ~52 us wide.
pub const PEAK_FS: u32 = 38_400;

/// What main.rs does with the stream next (`App::take_peak_req`).
pub const REQ_NONE: u8 = 0;
pub const REQ_START: u8 = 1;
pub const REQ_STOP: u8 = 2;

/// Peak's state: in SRAM (`.sram_bss`, all zero = off), not in App (the
/// App is on the main stack, whose headroom is the tight budget). 56 B.
pub(super) struct Pk {
    /// Peak mode is on (asked by MF / SCPI; the stream may still start).
    pub on: bool,
    /// The stream runs for Peak (main.rs `peak_started`).
    pub live: bool,
    /// A view change waits for the stream's stop (`show`): then `exit_to`.
    pub exit: bool,
    pub exit_to: View,
    /// DC A took the AC A row (DC-coupled): DC A again at the end.
    pub dc_amp: bool,
    /// Autorange at the start: 0 none, else 1 + v_auto / a_auto (restored
    /// at the end, as the wave view's `wave_auto`).
    pub auto: u8,
    /// REQ_*.
    pub req: u8,
    /// logger::wave::dropped() last seen (a hole breaks the 2-sample
    /// adjacency).
    pub dropped: u32,
    /// The side column: next refresh (ms, low 32 bits) and the sample count
    /// it last showed.
    pub next: u32,
    pub shown_n: u32,
    pub acc: Acc,
}

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut PK: Pk = Pk {
    on: false,
    live: false,
    exit: false,
    exit_to: View::Dmm,
    dc_amp: false,
    auto: 0,
    req: REQ_NONE,
    dropped: 0,
    next: 0,
    shown_n: 0,
    acc: Acc::NEW,
};

#[inline(always)]
pub(super) fn pk() -> &'static mut Pk {
    // SAFETY: main loop only (the ISR never sees it), no reference kept
    // across a call that takes another.
    unsafe { &mut *core::ptr::addr_of_mut!(PK) }
}

/// Peak mode is on (the idle shutdown waits, as for a recording session).
#[inline(always)]
pub fn peak_on() -> bool {
    pk().on
}

impl App {
    /// main.rs: start / stop the stream (REQ_*), taken once.
    #[inline(always)]
    pub fn take_peak_req(&mut self) -> u8 {
        core::mem::replace(&mut pk().req, REQ_NONE)
    }

    /// main.rs: the stream runs at PEAK_FS for Peak.
    pub fn peak_started(&mut self) {
        let p = pk();
        p.live = true;
        p.dropped = crate::logger::wave::dropped();
        p.acc.hole();
        // The look changed at the start (`peak_open`, no stream yet): only
        // the side column now (a status-bar repaint is region C code).
        self.chart_dirty = true;
    }

    /// The reading's look and the side column follow the mode now.
    pub(super) fn redraw_peak(&mut self) {
        self.chart_dirty = true;
        self.hold_shown = u32::MAX;
    }

    /// Per pass: the ring into the accumulator.
    #[inline(never)]
    pub fn peak_poll(&mut self) {
        let p = pk();
        if !p.live {
            return;
        }
        let d = crate::logger::wave::dropped();
        if d != p.dropped {
            p.dropped = d;
            p.acc.hole();
        }
        p.acc.push_block(&mut |f| crate::logger::wave::drain(f));
        // The side column at most 4 times a second, when there is news.
        let now = self.now as u32;
        if now.wrapping_sub(p.next) < 0x8000_0000 && p.acc.n != p.shown_n {
            p.next = now.wrapping_add(250);
            p.shown_n = p.acc.n;
            self.chart_dirty = true;
        }
    }

    /// P+, P-, RMS, CF in SI (the current row's scale); None before two
    /// adjacent samples or outside Peak's functions.
    pub(super) fn peak_res(&self) -> Option<crate::peak::Res> {
        let a = self.dmm.ad2_scale()?;
        pk().acc.si(a.per_code, a.offset)
    }

    /// The side column while Peak is on: (P+, CF, P-) in the column's
    /// display unit (CF a ratio).
    pub(super) fn peak_side(&self) -> (Option<f32>, Option<f32>, Option<f32>) {
        let Some(r) = self.peak_res() else { return (None, None, None) };
        let k = crate::meter::unit_scale(self.chart_unit);
        (Some(r.max / k), r.crest.is_finite().then_some(r.crest), Some(r.min / k))
    }

    /// The statistics reset (long press on the column, CALC:AVER:CLE) also
    /// restarts P+ / P- / CF.
    pub(super) fn peak_clear(&mut self) {
        let p = pk();
        p.acc.clear();
        p.shown_n = u32::MAX;
    }

    /// `show` to another view while the stream runs: the stream stops
    /// first (main.rs), then `peak_stopped` shows `v` (no page code, region
    /// C, while the ISR owns the bus). True: held.
    #[inline(always)]
    pub(super) fn peak_hold_view(&mut self, v: View) -> bool {
        let p = pk();
        if !p.on || v == View::Dmm {
            return false;
        }
        if p.live {
            p.req = REQ_STOP;
            p.exit = true;
            p.exit_to = v;
            return true;
        }
        // Asked but not started: nothing streams, `show` ends it now.
        false
    }

    /// main.rs: the stream runs for Peak.
    #[inline(always)]
    pub fn peak_live(&self) -> bool {
        pk().live
    }

    /// main.rs: the stream died (a DMM change, the bus guard) or could not
    /// start: Peak ends with `why` (app_peak.rs `peak_end`).
    #[inline(always)]
    pub fn peak_lost(&mut self, clk: &mut Clock, why: &'static str) {
        if pk().on {
            self.peak_end(clk, Some(why));
        }
    }

    /// CALC:PEAK:* (the queries a host polls while Peak runs: region B).
    #[inline(never)] // not inlined into scpi_dmm (its stack frame)
    pub(super) fn peak_scpi(&mut self, clk: &mut Clock, c: crate::scpi::Cmd, out: &mut crate::scpi::Scpi) -> bool {
        use crate::scpi::Cmd as C;
        match c {
            C::CalcPeakState(None) => {
                out.put_bytes(if pk().on { b"1" } else { b"0" });
                true
            }
            C::CalcPeakState(Some(true)) => {
                if !pk().on {
                    match self.peak_open(clk) {
                        Ok(()) if self.view != View::Dmm => self.show(View::Dmm, clk),
                        Ok(()) => {}
                        Err(_) => out.set_err("Settings conflict"),
                    }
                }
                false
            }
            C::CalcPeakState(Some(false)) => {
                self.peak_off();
                false
            }
            C::CalcPeak(k) => {
                match self.peak_res() {
                    Some(r) if !(k == 2 && !r.crest.is_finite()) => out.put_f32([r.max, r.min, r.crest][k.min(2) as usize]),
                    _ => {
                        out.set_err("Data stale");
                        out.put_bytes(b"+9.90000000E+37");
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// End Peak (SCPI OFF): the stream stops in main.rs, then
    /// `peak_stopped`.
    pub(super) fn peak_off(&mut self) {
        if pk().on {
            pk().req = REQ_STOP;
        }
    }
}
