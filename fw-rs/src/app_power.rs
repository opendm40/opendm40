//! Power meter in the app (dmm::pw, ui::pw): entry dialog, DMM page taps,
//! the Power Settings page, the continuous-current alarm, settings words and
//! the SCPI commands (scpi::Pow). A child of app.rs.

use super::*;
use crate::dmm::pw::{self, PowerVals};
use crate::scpi::Pow;

/// Persisted words (persist::W_POWER..+10): p0 = SPHACAL (i8) | method << 8 |
/// burden comp OFF << 9 | (discards + 1) << 12 (0 = default) | I limit in
/// 0.1 A << 16 (0 = default) | time limit s << 24 (0 = default, 255 = none);
/// p1..p3 = R_path per unit (f32 bits, 0 = the shunt value); p4 / p5 = the
/// V gains of 6 / 60 and 600 / 1000 V, u16 each (0 = factory-derived, else
/// 0.5 + n / 65536); p6 = the Y zero (i16) | zeros valid << 16 | wiring
/// dialog seen << 17 | coupling (pw::COUP_*) << 18; p7..p9 = the X zeros
/// of the 6 current ranges (i16).
pub const PW_WORDS: usize = 10;

fn gain_bits(g: f32) -> u32 {
    if g > 0.0 { ((g - 0.5) * 65536.0 + 0.5).clamp(1.0, 65535.0) as u32 } else { 0 }
}

fn gain_of(n: u32) -> f32 {
    let n = n & 0xFFFF;
    let g = 0.5 + n as f32 / 65536.0;
    if n != 0 && (0.9..=1.1).contains(&g) { g } else { 0.0 }
}

impl App {
    /// Open the wiring dialog (the W function button until it was confirmed
    /// once, a long press on A, Power Settings > Wiring). Auto-cancel after
    /// a minute.
    pub(super) fn pw_open_entry(&mut self, now: u64) {
        self.overlay = Some(crate::dialog::Overlay::PowerEntry { until: now + 60_000 });
        self.redraw = true;
        ui::pw::entry_dialog(self.dmm.pw.i_cont, self.dmm.pw.t_lim);
        self.beep(now, 50);
    }

    /// The dialog's OK: enter the power meter on the DMM page.
    pub(super) fn pw_enter(&mut self, clk: &mut Clock) {
        self.overlay = None;
        self.dmm.pw.wired = true;
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
                    Some(ui::pw::PS_COUP) => {
                        // AC -> DC -> AUTO -> AC.
                        self.dmm.pw_set_coupling(clk, (pv.coup + 1) % 3);
                    }
                    Some(ui::pw::PS_ZERO) => {
                        self.dmm.pw_zero_start(clk);
                        return true;
                    }
                    Some(ui::pw::PS_VCAL) => {
                        self.dmm.pw_vcal_start(clk);
                        return true;
                    }
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
        self.pw_settings_status();
        // DMM page before the first full reading: V and "I ranging..."
        // (the I channel walks down from 10 A, ~0.9 s per range).
        if self.view == View::Dmm && !self.redraw && self.meter.shown().is_none() {
            if let Some((v, txt)) = self.dmm.pw_partial() {
                self.pw_rng_key = ui::pw::ranging(v, txt, self.pw_rng_key);
            }
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
        w[4] = gain_bits(p.vgain[0]) | gain_bits(p.vgain[1]) << 16;
        w[5] = gain_bits(p.vgain[2]) | gain_bits(p.vgain[3]) << 16;
        w[6] = p.zy as u16 as u32 | (p.zero_ok as u32) << 16 | (p.wired as u32) << 17 | (p.coup as u32 & 3) << 18;
        for i in 0..3 {
            w[7 + i] = p.zx[2 * i] as u16 as u32 | (p.zx[2 * i + 1] as u16 as u32) << 16;
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
        p.vgain = [gain_of(w[4]), gain_of(w[4] >> 16), gain_of(w[5]), gain_of(w[5] >> 16)];
        p.zy = w[6] as u16 as i16;
        p.zero_ok = w[6] >> 16 & 1 != 0;
        p.wired = w[6] >> 17 & 1 != 0;
        p.coup = (w[6] >> 18 & 3).min(2) as u8;
        for i in 0..3 {
            p.zx[2 * i] = w[7 + i] as u16 as i16;
            p.zx[2 * i + 1] = (w[7 + i] >> 16) as u16 as i16;
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
            Pow::Coupling(Some(c)) => {
                self.dmm.pw_set_coupling(clk, c);
                self.redraw |= self.view == View::Range;
                false
            }
            Pow::Coupling(None) => {
                // The setting, and with AUTO the method in force.
                let p = &self.dmm.pw;
                out.put_bytes(match p.coup {
                    pw::COUP_DC => b"DC",
                    pw::COUP_AUTO if p.dc => b"AUTO,DC",
                    pw::COUP_AUTO => b"AUTO,AC",
                    _ => b"AC",
                });
                true
            }
            Pow::Zero(0) => {
                if !self.dmm.pw_zero_start(clk) {
                    out.set_err("Settings conflict");
                }
                self.redraw |= self.view == View::Range;
                false
            }
            Pow::Zero(1) => {
                let p = &mut self.dmm.pw;
                p.zero_ok = false;
                p.zx = [0; 6];
                p.zy = 0;
                false
            }
            Pow::Zero(_) => {
                // <last job result>,<SET|NONE>,<Y zero>,<X zeros 600 uA..10 A>
                let p = &self.dmm.pw;
                out.put_bytes(job_text(if p.job_last == pw::JOB_ZERO { p.job_res } else { pw::JR_NONE }).as_bytes());
                out.put_bytes(if p.zero_ok { b",SET," } else { b",NONE," });
                put_i(out, &mut t, p.zy as i64);
                for z in p.zx {
                    out.put_bytes(b",");
                    put_i(out, &mut t, z as i64);
                }
                true
            }
            Pow::VCal(0) => {
                if !self.dmm.pw_vcal_start(clk) {
                    out.set_err("Settings conflict");
                }
                self.redraw |= self.view == View::Range;
                false
            }
            Pow::VCal(1) => {
                let vr = self.dmm.pw.vr as usize;
                self.dmm.pw.vgain[vr] = 0.0;
                false
            }
            Pow::VCal(2) => {
                // <last job result>,<V range>,<gain in force>,<SELF|FACT>
                let p = &self.dmm.pw;
                let vr = if p.job_last == pw::JOB_VCAL { p.job_vr } else { p.vr };
                out.put_bytes(job_text(if p.job_last == pw::JOB_VCAL { p.job_res } else { pw::JR_NONE }).as_bytes());
                out.put_bytes(b",");
                out.put_f32(pw::V_LIM[vr as usize]);
                out.put_bytes(b",");
                out.put_f32(p.v_gain(vr));
                out.put_bytes(if p.vgain[vr as usize] > 0.0 { b",SELF" } else { b",FACT" });
                true
            }
            Pow::VCal(_) => {
                for vr in 0..4u8 {
                    if vr > 0 {
                        out.put_bytes(b",");
                    }
                    out.put_f32(self.dmm.pw.v_gain(vr));
                }
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

    /// Power Settings page: the status line (job progress / result, the
    /// gain and zero in force), redrawn when its text changes.
    pub(super) fn pw_settings_status(&mut self) {
        if self.view != View::Range || !self.dmm.pw.on || self.redraw {
            return;
        }
        let k = ui::pw::settings_status(&self.dmm.pw, self.pw_status_key);
        self.pw_status_key = k;
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

/// SCPI word of a job result (pw::JR_*).
fn job_text(r: u8) -> &'static str {
    match r {
        pw::JR_RUN => "RUN",
        pw::JR_OK => "OK",
        pw::JR_SIGNAL => "SIGNAL",
        pw::JR_SMALL => "SMALL",
        pw::JR_BIG => "BIG",
        pw::JR_UNSTABLE => "UNSTABLE",
        pw::JR_RANGE => "RANGE",
        pw::JR_ABORT => "ABORT",
        pw::JR_METHOD => "METHOD",
        _ => "NONE",
    }
}
