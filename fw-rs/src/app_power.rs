//! Power meter in the app (dmm::pw, ui::pw): entry dialog, DMM page taps,
//! the Power Settings page, the continuous-current alarm, settings words and
//! the SCPI commands (scpi::Pow). A child of app.rs.

use super::*;
use crate::dmm::pw::{self, PowerVals};
use crate::scpi::Pow;

/// Persisted words (persist::W_POWER..+4): p0 = SPHACAL (i8) | method << 8 |
/// burden comp OFF << 9 | (discards + 1) << 12 (0 = default) | I limit in
/// 0.1 A << 16 (0 = default) | time limit s << 24 (0 = default, 255 = none);
/// p1..p3 = R_path per unit (f32 bits, 0 = the shunt value).
pub const PW_WORDS: usize = 4;

impl App {
    /// Open the wiring dialog (long press on A, "Power W" on the Current
    /// Settings page). Auto-cancel after a minute.
    pub(super) fn pw_open_entry(&mut self, now: u64) {
        self.overlay = Some(crate::dialog::Overlay::PowerEntry { until: now + 60_000 });
        self.redraw = true;
        ui::pw::entry_dialog(self.dmm.pw.i_cont, self.dmm.pw.t_lim);
        self.beep(now, 50);
    }

    /// The dialog's OK: enter the power meter on the DMM page.
    pub(super) fn pw_enter(&mut self, clk: &mut Clock) {
        self.overlay = None;
        self.dmm.select(clk, pw::FUNC);
        self.func_changed();
        self.last_raw = None;
        self.show(View::Dmm, clk);
    }

    /// DMM page / settings page taps of the power meter; true = consumed.
    pub(super) fn pw_tap(&mut self, clk: &mut Clock, x: u16, y: u16) -> bool {
        let now = clk.now_ms();
        match self.view {
            View::Dmm if ui::pw::side_box_hit(x, y) => {
                self.dmm.pw.run = !self.dmm.pw.run;
                self.chart_dirty = true;
                true
            }
            // The side column and the power boxes have no other tap action
            // (no MIN / MAX session, no history slot).
            View::Dmm if (x >= 402 && (175..279).contains(&y)) || (153..172).contains(&y) => true,
            View::Range => {
                let pv = self.dmm.pw_state();
                match ui::pw::settings_hit(x, y) {
                    Some(c @ ui::pw::PS_V..=4) => {
                        let v = if c == 4 { None } else { Some(c) };
                        self.dmm.pw_set_ranges(clk, Some(v), None);
                    }
                    Some(c @ ui::pw::PS_I..=14) => {
                        let i = c - ui::pw::PS_I;
                        let i = if i == 6 { None } else { Some(i) };
                        self.dmm.pw_set_ranges(clk, None, Some(i));
                    }
                    Some(ui::pw::PS_METHOD_B) => self.dmm.pw_set_method(clk, false),
                    Some(ui::pw::PS_METHOD_ALT) => self.dmm.pw_set_method(clk, true),
                    Some(ui::pw::PS_BURDEN) => self.dmm.pw.comp = !pv.comp,
                    Some(ui::pw::PS_RESET) => {
                        self.dmm.pw.energy_reset();
                        self.beep(now, 300);
                    }
                    Some(ui::pw::PS_WIRING) => {
                        // Shown again for reference; OK / Cancel just close it.
                        self.pw_open_entry(now);
                        return true;
                    }
                    Some(ui::pw::PS_OK) => {
                        let v = self.back_view;
                        self.show(v, clk);
                        return true;
                    }
                    _ => return true,
                }
                self.redraw = true;
                true
            }
            _ => false,
        }
    }

    /// Long press on the side column in the power meter: reset the energy.
    pub(super) fn pw_long_side(&mut self, now: u64) {
        self.dmm.pw.energy_reset();
        self.beep(now, 300);
        self.chart_dirty = true;
    }

    /// Per tick: the continuous-current limit expired -> toast + the alert
    /// beep pattern while it stays above (A-JACK's); auto-off is held while
    /// the energy integrates (`App::tick`).
    pub(super) fn pw_tick(&mut self, now: u64) {
        if !self.dmm.pw.on {
            return;
        }
        if self.dmm.pw_over_expired() {
            self.toast(now, "Current above the limit: reduce it", false);
            self.alert_beep_t0 = Some(now);
        }
        if self.alert_beep_t0.is_some() && self.dmm.pw_state().over != 2 && self.dmm.sent.alert == crate::sentinel::Alert::None {
            self.alert_beep_t0 = None;
        }
    }

    /// Settings words (persist).
    pub(super) fn pw_words(&self) -> [u32; PW_WORDS] {
        let p = &self.dmm.pw;
        let ic = ((p.i_cont * 10.0 + 0.5) as u32).clamp(1, 100);
        let tl = if p.t_lim == 0 { 255 } else { p.t_lim.min(254) as u32 };
        let mut w = [0u32; PW_WORDS];
        w[0] = p.sph as u8 as u32 | (p.alt as u32) << 8 | (!p.comp as u32) << 9 | ((p.disc as u32 + 1) & 0xF) << 12 | ic << 16 | tl << 24;
        for i in 0..3 {
            w[1 + i] = if p.r_path[i] == pw::SHUNT_OHM[i] { 0 } else { p.r_path[i].to_bits() };
        }
        w
    }

    pub(super) fn pw_restore(&mut self, w: &[u32]) {
        let p = &mut self.dmm.pw;
        p.sph = (w[0] & 0xFF) as u8 as i8;
        p.alt = w[0] >> 8 & 1 != 0;
        p.comp = w[0] >> 9 & 1 == 0;
        let d = (w[0] >> 12 & 0xF) as u8;
        p.disc = if d == 0 { pw::DISC_DEFAULT } else { (d - 1).min(8) };
        let ic = (w[0] >> 16 & 0xFF) as f32 / 10.0;
        p.i_cont = if ic == 0.0 { pw::I_CONT_DEFAULT } else { ic.min(10.0) };
        p.t_lim = match (w[0] >> 24) as u8 {
            0 => pw::T_LIM_DEFAULT,
            255 => 0,
            t => t as u16,
        };
        for i in 0..3 {
            let r = f32::from_bits(w[1 + i]);
            p.r_path[i] = if w[1 + i] != 0 && r.is_finite() && (0.0..=1000.0).contains(&r) { r } else { pw::SHUNT_OHM[i] };
        }
    }

    /// The power meter's SCPI commands. Queries answer from the latest
    /// values; outside the power meter the value queries are a "Settings
    /// conflict" (CONF:POW first), the settings work anywhere.
    pub(super) fn pw_scpi(&mut self, clk: &mut Clock, c: Pow, out: &mut crate::scpi::Scpi) -> bool {
        let on = self.dmm.pw.on;
        let pv = self.dmm.pw_state();
        let mut t = [0u8; 24];
        let put_i = |out: &mut crate::scpi::Scpi, t: &mut [u8; 24], v: i64| out.put_bytes(pw::i64_text(t, v));
        match c {
            Pow::Conf(v, i) => {
                if !on {
                    self.dmm_power(clk);
                    self.select_func(clk, pw::FUNC);
                }
                let m = |x: Option<u8>| x.map(|r| if r == 255 { None } else { Some(r) });
                self.dmm.pw_set_ranges(clk, Some(m(v).unwrap_or(None)), Some(m(i).unwrap_or(None)));
                self.redraw = true;
                false
            }
            Pow::All | Pow::Get(_) if !on => {
                out.set_err("Settings conflict");
                false
            }
            Pow::All => {
                // P,S,Q,PF,Vrms,Irms,F,E_Wh,seconds,Vdc,Idc,VAh,burden_V
                let vals = [pv.p, pv.s, pv.q, pv.pf, pv.v, pv.i, pv.f_hz, pv.e_wh, pv.secs as f32, pv.vdc, pv.idc, pv.vah, pv.burden_v];
                for (k, v) in vals.iter().enumerate() {
                    if k > 0 {
                        out.put_bytes(b",");
                    }
                    if v.is_finite() {
                        out.put_f32(*v);
                    } else {
                        out.put_bytes(b"+9.91000000E+37"); // NaN (SCPI)
                    }
                }
                true
            }
            Pow::Get(k) => {
                let v = [pv.s, pv.pf, pv.e_wh, pv.q, pv.v, pv.i][k.min(5) as usize];
                if v.is_finite() { out.put_f32(v) } else { out.put_bytes(b"+9.91000000E+37") }
                true
            }
            Pow::VRange(None) => {
                out.put_bytes(if pv.v_auto { b"AUTO," } else { b"MAN," });
                out.put_f32(pw::V_LIM[pv.vr as usize]);
                true
            }
            Pow::IRange(None) => {
                out.put_bytes(if pv.i_auto { b"AUTO," } else { b"MAN," });
                out.put_f32(pw::I_LIM[pv.ik as usize]);
                true
            }
            Pow::VRange(Some(r)) => {
                self.dmm.pw_set_ranges(clk, Some(r), None);
                self.redraw |= self.view == View::Range;
                false
            }
            Pow::IRange(Some(r)) => {
                self.dmm.pw_set_ranges(clk, None, Some(r));
                self.redraw |= self.view == View::Range;
                false
            }
            Pow::EnergyReset => {
                self.dmm.pw.energy_reset();
                false
            }
            Pow::EnergyState(Some(b)) => {
                self.dmm.pw.run = b;
                false
            }
            Pow::EnergyState(None) => {
                out.put_bytes(if pv.run { b"1" } else { b"0" });
                true
            }
            Pow::Burden(Some(r)) => {
                let (iu, _) = pw::iu_ir(pv.ik);
                self.dmm.pw.r_path[iu as usize] = if r.is_nan() { pw::SHUNT_OHM[iu as usize] } else { r };
                false
            }
            Pow::Burden(None) => {
                let (iu, _) = pw::iu_ir(pv.ik);
                out.put_f32(self.dmm.pw.r_path[iu as usize]);
                true
            }
            Pow::BurdenState(Some(b)) => {
                self.dmm.pw.comp = b;
                false
            }
            Pow::BurdenState(None) => {
                out.put_bytes(if pv.comp { b"1" } else { b"0" });
                true
            }
            Pow::Phase(Some(s)) => {
                self.dmm.pw_set_phase(clk, s);
                false
            }
            Pow::Phase(None) => {
                put_i(out, &mut t, self.dmm.pw.sph as i64);
                true
            }
            Pow::PhaseCal(reactive) => {
                // The resistive optimum is flat (cos): at 50 Hz one SPHACAL
                // step moves PF 1 by 1e-8; only the reactive null works.
                if !reactive || !self.dmm.pw_cal_start() {
                    out.set_err("Settings conflict");
                }
                false
            }
            Pow::Cycle => {
                let (d, p, f) = self.dmm.pw_cycle();
                put_i(out, &mut t, d as i64);
                out.put_bytes(b",");
                put_i(out, &mut t, p as i64);
                out.put_bytes(b",");
                put_i(out, &mut t, f as i64);
                true
            }
            Pow::Method(Some(alt)) => {
                self.dmm.pw_set_method(clk, alt);
                self.redraw = true;
                false
            }
            Pow::Method(None) => {
                out.put_bytes(if pv.alt { b"ALT" } else { b"SIM" });
                true
            }
            Pow::Limit(Some((i, s))) => {
                self.dmm.pw.i_cont = i.max(0.1);
                self.dmm.pw.t_lim = s;
                false
            }
            Pow::Limit(None) => {
                out.put_f32(self.dmm.pw.i_cont);
                out.put_bytes(b",");
                put_i(out, &mut t, self.dmm.pw.t_lim as i64);
                true
            }
            Pow::DbgRow => {
                // 22 row bytes R20..R35 then R36, hex; the R34 in force.
                const H: &[u8; 16] = b"0123456789ABCDEF";
                let p = &self.dmm.pw;
                for (k, &b) in p.row.iter().chain(core::iter::once(&p.r36)).enumerate() {
                    if k > 0 {
                        out.put_bytes(b" ");
                    }
                    out.put_bytes(&[H[(b >> 4) as usize], H[(b & 15) as usize]]);
                }
                out.put_bytes(if on { b";ON" } else { b";OFF" });
                true
            }
            Pow::DbgPoke(i, v) => {
                if !self.dmm.pw_poke(clk, i, v) {
                    out.set_err("Illegal parameter value");
                }
                false
            }
            Pow::DbgStat(s) => {
                self.dmm.pw_freeze(clk, s);
                false
            }
            Pow::DbgRaw => {
                let n = self.dmm.pw.ring_text(&mut out.buf[..]);
                out.len = n;
                true
            }
            Pow::DbgDisc(Some(d)) => {
                self.dmm.pw.disc = d;
                false
            }
            Pow::DbgDisc(None) => {
                put_i(out, &mut t, self.dmm.pw.disc as i64);
                true
            }
        }
    }

    /// CONFigure? in the power meter: "POW <V range>,<I range>" (AUTO or the
    /// range limits in V / A).
    pub(super) fn pw_conf(&self, out: &mut crate::scpi::Scpi) {
        let pv: PowerVals = self.dmm.pw_state();
        out.put_bytes(b"\"POW ");
        if pv.v_auto {
            out.put_bytes(b"AUTO");
        } else {
            out.put_f32(pw::V_LIM[pv.vr as usize]);
        }
        out.put_bytes(b",");
        if pv.i_auto {
            out.put_bytes(b"AUTO");
        } else {
            out.put_f32(pw::I_LIM[pv.ik as usize]);
        }
        out.put_bytes(b"\"");
    }
}
