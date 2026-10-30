//! Power Wave (View::PWave; src/pwave.rs, src/ui_pwave.rs; doc
//! power-meter-and-misconnect.md 3.18): the power meter's U and I
//! waveforms. Entry: Power Settings (MF in W) -> "Wave", or SCPI
//! POWer:WAVE ON. The page lends the scope's record buffer, and while it
//! is open main.rs treats the HY3131 as busy (no power readings, no BT
//! serve, the alerts pause); each pass after FRAME_MS the page writes its
//! row, captures a frame (pwave::capture, blocking), draws it. Back / MENU /
//! MF or any view change closes it: the buffer goes back, the power row is
//! rewritten (main.rs: busy -> idle, Dmm::start -> pw_restart).

use super::*;
use crate::pwave::{self as pv, Cap, Scale, Stats};

/// Least time between two frames (ms): touch, USB and the keys get passes.
const FRAME_MS: u64 = 250;

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
};

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
        *s = Pwv {
            on: true,
            run: true,
            tb1: if s.tb1 == 0 { pv::TB_DEFAULT + 1 } else { s.tb1 },
            next: now,
            frames: 0,
            cap: Cap::default(),
            st: Stats::NONE,
            sc,
            du: 0.0,
            di: 0.0,
            fu: pv::floor_for(fu),
            fi: pv::floor_for(fi),
            vr: self.dmm.pw.vr,
            ik: self.dmm.pw.ik,
            retries: 0,
            bad: 0,
            iref: self.dmm.pw_wave_iref().map_or(f32::NAN, |r| r.0),
            oi_fix: f32::NAN,
            zero_ok: self.dmm.pw_wave_iref().is_some_and(|r| r.1),
            gi_fix: f32::NAN,
        };
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

    /// The whole page (draw_static): the texts and the last frame.
    #[inline(never)]
    pub(super) fn pwave_page(&mut self) {
        self.pwave_draw(true);
    }

    #[inline(never)]
    fn pwave_draw(&mut self, full: bool) {
        let s = pwv();
        let (mut bu, mut bi, mut bp, mut bf, mut du, mut di, mut bt) = ([0u8; 24], [0u8; 24], [0u8; 24], [0u8; 24], [0u8; 24], [0u8; 24], [0u8; 24]);
        let st = &s.st;
        let v = ui::pwv::Vals {
            u: if s.cap.clip_u { "U OL" } else { ui::pwv::fmt(&mut bu, "U", st.urms, "V") },
            i: if s.cap.clip_i { "I OL" } else { ui::pwv::fmt(&mut bi, "I", st.irms, "A") },
            p: ui::pwv::fmt(&mut bp, "P", st.p, "W"),
            f: ui::pwv::fmt(&mut bf, "f", st.f, "Hz"),
            u_div: if s.du > 0.0 { ui::pwv::fmt_div(&mut du, s.du, "V") } else { "" },
            i_div: if s.di > 0.0 { ui::pwv::fmt_div(&mut di, s.di, "A") } else { "" },
            tb: {
                let mut nb = [0u8; 16];
                let n = crate::dmm::format(&mut nb, pv::TB_MS[s.tb1 as usize - 1] as f32, 0);
                let l = n.len();
                bt[..l].copy_from_slice(n.as_bytes());
                bt[l..l + 6].copy_from_slice(b"ms/div");
                core::str::from_utf8(&bt[..l + 6]).unwrap_or("")
            },
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
        if !s.run || now < s.next || crate::hy3131::streaming() {
            return;
        }
        let Some(buf) = self.scope.lent_buf() else {
            return;
        };
        let (row, r36, mut sc, _, _) = self.dmm.pw_wave_setup();
        if s.oi_fix.is_finite() {
            sc.oi += s.oi_fix;
        }
        if s.gi_fix.is_finite() {
            sc.si *= s.gi_fix;
        }
        let span_us = pv::TB_MS[s.tb1 as usize - 1] as u32 * pv::DIVS_X * 1000;
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
        self.pwave_draw(false);
        s.next = clk.now_ms() + FRAME_MS;
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
            _ => {}
        }
    }

    /// RUN / STOP (the button, the RUN key).
    pub(super) fn pwave_run(&mut self, now: u64) {
        let s = pwv();
        s.run = !s.run;
        s.next = now;
        ui::pwv::run_button(s.run);
    }

    /// POW:WAVE? : open, frames, Urms, Irms, P, f, samples per channel,
    /// repeated reads, unreadable samples, the I zero taken from the DC
    /// meter (A; 0 = none), the I gain taken from it (1 = none).
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
        out.put_bytes(b",");
        out.put_f32(if s.oi_fix.is_finite() { s.oi_fix * s.sc.si } else { 0.0 });
        out.put_bytes(b",");
        out.put_f32(if s.gi_fix.is_finite() { s.gi_fix } else { 1.0 });
    }
}
