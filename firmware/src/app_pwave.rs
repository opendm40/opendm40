//! Power Wave (View::PWave; src/pwave.rs, src/ui_pwave.rs; doc
//! power-meter-and-misconnect.md 3.18): the power meter's U and I
//! waveforms. Entry: Power Settings (MF in W) -> "Wave", or SCPI
//! POWer:WAVE ON. The page lends the scope's record buffer, and while it
//! is open main.rs treats the HY3131 as busy (no power readings, no BT
//! serve); each pass after FRAME_MS the page writes its row, captures a
//! frame (pwave::capture, blocking), draws it. The page checks the current
//! itself (`pwave_guard`: every frame, or a short I-only read once a second
//! while stopped): over 10 A, over the range, or the current limit run out
//! closes it into the power meter with the meter's own alarm. Back / MENU /
//! MF or any view change closes it: the buffer goes back, the power row is
//! rewritten (main.rs: busy -> idle, Dmm::start -> pw_restart).

use super::*;
use crate::pwave::{self as pv, Cap, Scale, Stats};
use crate::scope_ui::{fmt_div, Buf};

/// Least time between two frames (ms): touch, USB and the keys get passes.
const FRAME_MS: u64 = 250;
/// The current check of a stopped page (`pwave::i_probe`) this often (ms).
const CHECK_MS: u64 = 1000;

/// All zero at reset (.sram_bss, not the TCM's .data: the stack's
/// headroom); `pwave_open` sets the rest.
pub(super) struct Pwv {
    pub on: bool,
    run: bool,
    /// The timebase index + 1 (0: never chosen, pwave::TB_DEFAULT); kept
    /// across openings.
    tb1: u8,
    next: u64,
    pub frames: u32,
    cap: Cap,
    st: Stats,
    sc: Scale,
    /// V/div, A/div in force (0 = none yet) and their floors.
    du: f32,
    di: f32,
    fu: f32,
    fi: f32,
    /// The power meter's ranges at the entry (a change closes the page).
    vr: u8,
    ik: u8,
    /// Reads repeated / unreadable samples, all frames (POW:WAVE?).
    pub retries: u32,
    pub bad: u32,
    /// The DC coupling's current at the entry (`Dmm::pw_wave_iref`; NaN:
    /// none) and the I zero taken from it with the first frame
    /// (`pwave::anchor`, stored codes; NaN: none yet / none taken).
    iref: f32,
    oi_fix: f32,
    /// `POW:ZERO`'s X zero was stored at the entry: the anchor fits AD2's
    /// gain (`pwave::anchor_gain`, a factor on `Scale::si`, NaN: none),
    /// not its zero.
    zero_ok: bool,
    gi_fix: f32,
    /// The next current check of a stopped page (ms).
    check: u64,
    /// The last current check (`pwave_guard`): I (A), a sample on the
    /// rail, and why it closed the page (0 none, 1 above 10 A, 2 over the
    /// range, 3 the current limit); POW:WAVE? fields 12..14.
    chk_i: f32,
    chk_clip: bool,
    chk_why: u8,
}

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut PWV: Pwv = Pwv {
    on: false,
    run: false,
    tb1: 0,
    next: 0,
    frames: 0,
    cap: Cap { n: 0, y_first: false, trig: false, retries: 0, bad: 0, clip_u: false, clip_i: false },
    st: Stats { urms: 0.0, irms: 0.0, p: 0.0, f: 0.0, upk: 0.0, ipk: 0.0 },
    sc: Scale { su: 0.0, ou: 0.0, si: 0.0, oi: 0.0 },
    du: 0.0,
    di: 0.0,
    fu: 0.0,
    fi: 0.0,
    vr: 0,
    ik: 0,
    retries: 0,
    bad: 0,
    iref: 0.0,
    oi_fix: 0.0,
    zero_ok: false,
    gi_fix: 0.0,
    check: 0,
    chk_i: 0.0,
    chk_clip: false,
    chk_why: 0,
};

impl Pwv {
    /// The timebase in force (ms/div).
    #[inline(never)]
    fn tb_ms(&self) -> u16 {
        pv::TB_MS[self.tb1 as usize - 1]
    }
}

pub(super) fn pwv() -> &'static mut Pwv {
    // SAFETY: main loop only.
    unsafe { &mut *core::ptr::addr_of_mut!(PWV) }
}

/// The page holds the HY3131 (main.rs `busy`).
pub fn pwave_on() -> bool {
    pwv().on
}

impl App {
    /// Open the page (Power Settings "Wave", POW:WAVE ON); Err = why not
    /// (also a toast).
    #[inline(never)]
    pub(super) fn pwave_open(&mut self, clk: &mut Clock) -> Result<(), &'static str> {
        let s = pwv();
        if s.on {
            return Ok(());
        }
        let now = clk.now_ms();
        let pw = &self.dmm.pw;
        let why = if !pw.on {
            Some("Wave: power meter only")
        } else if pw.alt {
            Some("Wave: not with POW:METH ALT")
        } else if pw.job != 0 {
            Some("Wave: a Zero / V cal runs")
        } else if self.logger_busy || crate::hy3131::streaming() || crate::fastlog::active() {
            Some("Wave: data logger running")
        } else if crate::csvlog::active() {
            Some("Wave: recording runs")
        } else if self.scope.held() {
            Some("Wave: scope record in use")
        } else if self.scope.log_on() {
            // The decoder's frame log has BUF; closing it returns BUF.
            Some("Wave: decode log runs")
        } else {
            None
        };
        if let Some(t) = why {
            self.toast(now, t, false);
            return Err("Settings conflict");
        }
        if self.scope.wave_on() {
            self.wave_close(clk, false);
        }
        let (_, _, sc, fu, fi) = self.dmm.pw_wave_setup();
        self.scope.lend_buf();
        let tb1 = if s.tb1 == 0 { pv::TB_DEFAULT + 1 } else { s.tb1 };
        let iref = self.dmm.pw_wave_iref();
        // SAFETY: all zero is a valid Pwv (its reset state); the rest below.
        unsafe { core::ptr::write_bytes(s as *mut Pwv, 0, 1) };
        s.on = true;
        s.run = true;
        s.tb1 = tb1;
        s.next = now;
        s.check = now;
        s.st = Stats::NONE;
        s.sc = sc;
        s.fu = pv::floor_for(fu);
        s.fi = pv::floor_for(fi);
        s.vr = self.dmm.pw.vr;
        s.ik = self.dmm.pw.ik;
        s.iref = iref.map_or(f32::NAN, |r| r.0);
        s.oi_fix = f32::NAN;
        s.zero_ok = iref.is_some_and(|r| r.1);
        s.gi_fix = f32::NAN;
        s.chk_i = f32::NAN;
        self.show(View::PWave, clk);
        Ok(())
    }

    /// Close the page (`show` leaving it): the buffer back to the scope,
    /// the energy gap; main.rs restores the power row next pass.
    #[inline(never)]
    pub(super) fn pwave_close(&mut self) {
        let s = pwv();
        if !s.on {
            return;
        }
        s.on = false;
        self.scope.return_buf();
        self.dmm.pw_wave_gap();
    }

    /// The page: `full` the whole of it (draw_static), else its live parts,
    /// with the last frame.
    #[inline(never)]
    pub(super) fn pwave_draw(&mut self, full: bool) {
        let s = pwv();
        let mut b = [[0u8; 24]; 5];
        let [bu, bi, bp, bf, bt] = &mut b;
        let (mut du, mut di) = (Buf::new(), Buf::new());
        if s.du > 0.0 {
            fmt_div(&mut du, s.du, "V");
        }
        if s.di > 0.0 {
            fmt_div(&mut di, s.di, "A");
        }
        let st = &s.st;
        let v = ui::pwv::Vals {
            u: if s.cap.clip_u { "U OL" } else { ui::pwv::fmt(bu, 0, st.urms) },
            i: if s.cap.clip_i { "I OL" } else { ui::pwv::fmt(bi, 1, st.irms) },
            p: ui::pwv::fmt(bp, 2, st.p),
            f: ui::pwv::fmt(bf, 3, st.f),
            u_div: du.as_str(),
            i_div: di.as_str(),
            tb: ui::pwv::fmt_tb(bt, s.tb_ms()),
            run: s.run,
            u_ol: s.cap.clip_u,
            i_ol: s.cap.clip_i,
        };
        if full {
            ui::pwv::page(&v);
        } else {
            ui::pwv::live(&v);
        }
        let buf = self.scope.lent_buf();
        ui::pwv::plot(buf.filter(|_| s.frames > 0).map(|b| &b[pv::COL_AT..pv::COL_AT + 2 * pv::PW]));
    }

    /// Per pass on the page (App::tick, drawing allowed): a frame when due.
    #[inline(never)]
    pub(super) fn pwave_tick(&mut self, clk: &mut Clock) {
        let s = pwv();
        if !s.on {
            return;
        }
        let pw = &self.dmm.pw;
        if !pw.on || pw.alt || pw.job != 0 || pw.vr != s.vr || pw.ik != s.ik {
            // The power meter changed under the page (SCPI): back to it.
            let v = self.back_view;
            self.show(v, clk);
            return;
        }
        let now = clk.now_ms();
        if crate::hy3131::streaming() {
            return;
        }
        let (row, r36, mut sc, _, _) = self.dmm.pw_wave_setup();
        if s.oi_fix.is_finite() {
            sc.oi += s.oi_fix;
        }
        if s.gi_fix.is_finite() {
            sc.si *= s.gi_fix;
        }
        if !s.run || now < s.next {
            // Stopped: no frames, so a short I-only read keeps the check.
            if !s.run && now >= s.check {
                if s.frames == 0 {
                    pv::arm(&row, r36);
                }
                let (i, clip) = pv::i_probe(pv::PROBE_US, row[20], &sc);
                s.check = clk.now_ms() + CHECK_MS;
                self.pwave_guard(clk, i, clip);
            }
            return;
        }
        let Some(buf) = self.scope.lent_buf() else {
            return;
        };
        let span_us = s.tb_ms() as u32 * pv::DIVS_X * 1000;
        // The trigger: from the last frame (U, else I), else free run.
        let trig = if s.frames > 0 { pv::next_trig(buf, &s.cap, (s.fu / sc.su / 2.0) as i32, (s.fi / sc.si / 2.0) as i32) } else { None };
        pv::arm(&row, r36);
        s.cap = pv::capture(buf, span_us, trig, row[20]);
        s.retries = s.retries.wrapping_add(s.cap.retries);
        s.bad = s.bad.wrapping_add(s.cap.bad);
        s.frames = s.frames.wrapping_add(1);
        // The first frame: the I zero from the DC meter's current (doc
        // 3.18: AD2's zero is not in the factory record, POW:ZERO may not
        // have run), once.
        // With POW:ZERO's zero stored the difference is AD2's gain against
        // AD1 (device 27a0e81f: the zero taken grew with the current, 1.15
        // mA at 20 mA, 1.69 mA at 50 mA: ~0.8 mA zero + ~1.8 % gain); without
        // it one reading cannot tell the two apart: the zero takes both
        // (exact at this current, the gain error stays on the AC part and
        // on other currents; doc 3.18).
        if s.iref.is_finite() {
            if s.zero_ok {
                if let Some(g) = pv::anchor_gain(buf, &s.cap, &sc, s.iref) {
                    s.gi_fix = g;
                    sc.si *= g;
                }
            } else if let Some(d) = pv::anchor(buf, &s.cap, &sc, s.iref) {
                s.oi_fix = d;
                sc.oi += d;
            }
            s.iref = f32::NAN;
        }
        s.sc = sc;
        let y = trig.map_or(true, |t| t.y);
        s.st = pv::stats(buf, &s.cap, &sc, y, if y { s.fu } else { s.fi } / 2.0);
        s.du = pv::div_for(s.st.upk, s.du, s.fu);
        s.di = pv::div_for(s.st.ipk, s.di, s.fi);
        pv::columns(buf, &s.cap, &sc, s.du, s.di, span_us);
        s.next = clk.now_ms() + FRAME_MS;
        s.check = s.next.max(now + CHECK_MS);
        if self.pwave_guard(clk, s.st.irms, s.cap.clip_i) {
            return;
        }
        self.pwave_draw(false);
    }

    /// The current check (the frame's I, or `pwave::i_probe` while
    /// stopped; `i` RMS in A with the DC part, `clip` a sample on the
    /// rail). The meter's own rules, which do not run while the page holds
    /// the HY3131:
    ///   * above the 10 A rating (`danger::over_rating`) for
    ///     I10_ASSERT_MS: the danger alarm (`i10_since`, `alert_beep`);
    ///   * over the range on a range below 10 A: the meter's OVERLOAD trip
    ///     (uA / mA shunt) or I AUTO up-range would act;
    ///   * the continuous-current limit ran out (`Dmm::pw_wave_current`
    ///     keeps its countdown).
    /// Each closes the page into the power meter (toast), which takes over
    /// with its alarm / range change / limit beeps. True = closed.
    /// (A-PROT does not apply: the power meter measures through A on
    /// purpose, `pw_enter` ends it.)
    #[inline(never)]
    fn pwave_guard(&mut self, clk: &mut Clock, i: f32, clip: bool) -> bool {
        let now = clk.now_ms();
        let ik = self.dmm.pw.ik;
        if crate::danger::over_rating(0, 0, false, 0.0, Some((ik, clip, i))) {
            self.i10_since.get_or_insert(now);
        } else {
            self.i10_since = None;
        }
        let i10 = self.i10_since.is_some_and(|t| now.saturating_sub(t) >= crate::danger::I10_ASSERT_MS);
        let r = self.dmm.pw_wave_current(now, i, clip);
        let s = pwv();
        (s.chk_i, s.chk_clip) = (i, clip);
        let (w, why) = match r {
            _ if i10 => (1, Some("Wave: current above 10 A")),
            1 => (2, Some("Wave: current over range")),
            // The meter's own toast and beeps follow (`pw_over_expired`).
            2 => (3, None),
            _ => return false,
        };
        s.chk_why = w;
        // Into a view with the power meter's readout (the DMM rail on: the
        // alarm sounds only with it), whatever the page was opened from.
        let v = if matches!(self.back_view, View::Dmm | View::DmmScope) { self.back_view } else { View::Dmm };
        self.show(v, clk);
        if let Some(t) = why {
            self.toast(now, t, false);
        }
        true
    }

    /// Page taps.
    pub(super) fn pwave_tap(&mut self, clk: &mut Clock, x: u16, y: u16) {
        let s = pwv();
        let now = clk.now_ms();
        match ui::pwv::page_hit(x, y) {
            Some(0) => {
                let v = self.back_view;
                self.show(v, clk);
            }
            Some(1 | 2) if self.scope.hold => self.pwave_run(now),
            Some(k @ (1 | 2)) => {
                let tb = s.tb1 - 1;
                let t = if k == 1 { tb.checked_sub(1) } else { (tb as usize + 1 < pv::TB_MS.len()).then_some(tb + 1) };
                match t {
                    Some(t) => {
                        s.tb1 = t + 1;
                        s.next = now;
                        s.run = true;
                        self.pwave_draw(false);
                    }
                    None => self.beep(now, 300),
                }
            }
            Some(3) => self.pwave_run(now),
            Some(4) => self.pwave_csv(now),
            _ => {}
        }
    }

    /// "CSV": the frame on the screen to the disk as the next free
    /// SCOPE/SCOPnnn.DLG (DLG kind 4, served as SCOPnnn.CSV; scope_csv.rs
    /// `set_pwave`), by the screenshot writer like a scope record: the page
    /// stops first, the scope buffer stays held (Scope::hold: no frame, no
    /// RUN, no timebase change, the scope does not take it back) until the
    /// writer's result (scope_page.rs `sp_csv_poll`: "Saved SCOPnnn.DLG").
    #[inline(never)]
    fn pwave_csv(&mut self, now: u64) {
        let s = pwv();
        if s.frames == 0 || s.cap.n < 2 {
            self.toast(now, "No record to save", false);
            return;
        }
        if self.scope.hold || crate::screenshot::busy() || crate::csvlog::writing() {
            self.toast(now, "Disk busy, try again", false);
            return;
        }
        let Some(buf) = self.scope.lent_buf() else {
            return;
        };
        if s.run {
            s.run = false;
            ui::pwv::run_button(false);
        }
        let mut h = crate::scope_csv::Hdr::new();
        let mut b = [0u8; 24];
        let mut nb = [0u8; 16];
        h.s("# Alientek ").s(crate::ident::model_name()).s(", opendm40 ").s(crate::ident::VERSION).s(", power wave frame (no real-time clock)\r\n");
        h.s("# timebase ").s(ui::pwv::fmt_tb(&mut b, s.tb_ms())).s(", U ");
        let (mut du, mut di) = (Buf::new(), Buf::new());
        fmt_div(&mut du, s.du, "V");
        fmt_div(&mut di, s.di, "A");
        h.s(du.as_str()).s("/div, I ").s(di.as_str()).s("/div, samples ");
        h.s(crate::dmm::format(&mut nb, s.cap.n as f32, 0)).s(" (U and I in turn), ").s(if s.cap.trig { "triggered\r\n" } else { "untriggered\r\n" });
        let st = s.st;
        h.s("# ");
        for (k, v) in [st.urms, st.irms, st.p, st.f].into_iter().enumerate() {
            h.s(ui::pwv::fmt(&mut b, k, v)).s(if k < 3 { ", " } else { " (over the frame's whole cycles)\r\n" });
        }
        h.s("# time_s = seconds from the first sample; each row is one sample: U in volts or I in amperes, the other column empty\r\n");
        let sc = s.sc;
        crate::scope_csv::set_pwave(buf, s.cap.n, [sc.su, sc.ou, sc.si, sc.oi], pv::T_UNIT_US as f32 * 1e-6, s.cap.y_first, &h);
        if !crate::screenshot::start_csv() {
            self.toast(now, "Save CSV Error", false);
            return;
        }
        self.scope.hold = true;
        self.toast(now, "Saving CSV...", true);
    }

    /// RUN / STOP (the button, the RUN key).
    pub(super) fn pwave_run(&mut self, now: u64) {
        let s = pwv();
        if self.scope.hold {
            // A CSV of this frame is being written (`pwave_csv`).
            self.toast(now, "Saving CSV...", true);
            return;
        }
        s.run = !s.run;
        s.next = now;
        ui::pwv::run_button(s.run);
    }

    /// POW:WAVE? : open, frames, Urms, Irms, P, f, samples per channel,
    /// repeated reads, unreadable samples, the I zero taken from the DC
    /// meter (A; 0 = none), the I gain taken from it (1 = none), the last
    /// current check's I (A), its rail flag and why it closed the page.
    pub(super) fn pwave_query(&self, out: &mut crate::scpi::Scpi) {
        let s = pwv();
        let mut t = [0u8; 24];
        out.put_bytes(if s.on { b"1," } else { b"0," });
        out.put_bytes(crate::dmm::pw::i64_text(&mut t, s.frames as i64));
        for v in [s.st.urms, s.st.irms, s.st.p, s.st.f] {
            out.put_bytes(b",");
            out.put_f32(if v.is_finite() { v } else { 9.91e37 });
        }
        for v in [(s.cap.n / 2) as i64, s.retries as i64, s.bad as i64] {
            out.put_bytes(b",");
            out.put_bytes(crate::dmm::pw::i64_text(&mut t, v));
        }
        for (v, none) in [(s.oi_fix * s.sc.si, 0.0), (s.gi_fix, 1.0), (s.chk_i, 9.91e37)] {
            out.put_bytes(b",");
            out.put_f32(if v.is_finite() { v } else { none });
        }
        out.put_bytes(if s.chk_clip { b",1," } else { b",0," });
        out.put_bytes(&[b'0' + s.chk_why]);
    }
}
