//! AC+DC methods for V and A (docs/acdc-approaches.md 5 and 7; prototype
//! for one device session to compare them). A child module of dmm.rs
//! (`Dmm::am`), selected at run time with DBG:ACDC:METH, RAM only:
//!
//! * A (default) - the stock alternation of whole rows (dmm.rs `dc_out` /
//!   `poll_ac`, ~1.9 s per cycle). Nothing here runs.
//! * B - one fixed row: the AC row (AD2 + AD3, RMS, the factory AC
//!   calibration) with AD1 converting the DC part next to it (the DC row's
//!   AD1 bytes, OP1's chopper held: AD1 converting would start its 2 kHz
//!   square at OP1O -> AD2). Written with AD1 off, AD3 then AD2 and AD1
//!   last (datasheet 13.1 note 7), nothing toggled afterwards. AD1 shifts
//!   while AD2 / AD3 convert (device, feasibility 13.2: 0.03 .. 0.16 % of
//!   the range, a non-repeating gain up to -0.18 % on 60 V), so B takes a
//!   per-range zero: the stock DC row (AD1 alone, the reference) against
//!   the merged row's AD1, measured at entry / at the first visit of a
//!   range, or fixed with DBG:ACDC:ZERO. DC part = the mean of the last 5
//!   AD1 results (0.5 s) minus the zero. Kept for the comparison only.
//! * C - the primary candidate. V: the DC-coupled V row of E7 (ad2exp.rs
//!   `dcv_row`: AD2 on OP1O, OP1's chopper held) with AD3 on the same node
//!   and the RMS / LPF on: DC part = LPF, AC part = sqrt(RMS) (the HPF in
//!   the RMS block takes the DC out), both through E7's entry self-cal
//!   (`ad2exp::dc_enter`: AD1 alone, then the zero and the signal on AD2,
//!   gain and offset; `dmm::dc_cal`), one fixed row, nothing switched, both
//!   parts every LPF block (106.7 ms). The AC part rides the self-cal's DC
//!   gain, not the factory AC V calibration (which belongs to the AC-coupled
//!   PB4 node). A: the stock AC A row as it is (AD2 on the shunt node is
//!   DC-coupled): AC part = RMS with the factory AC A gain, DC part = the LPF
//!   with the same gain, corrected against the stock DC A row at entry (one
//!   point: a gain factor when the current is >= 10 % of the range, else
//!   an offset; the factory LPF gains are 1.0, "never calibrated").
//!
//! The 6 V range of B: the AC V 6 V row sits on the /100 tap with OP1 x10
//! (PB4), DC V 6 V on the /10 tap with OP1 x1, so no merged row is the
//! stock DC V 6 V path (doc 3, last bullet; test 7). DBG:ACDC:R0 1 (default)
//! = candidate (i): AD1 on SENSE - PB5 at the /100 tap = DC V 60 V's AD1
//! input and calibration, 10x less signal; 2 = candidate (ii): AD1 on OP1O -
//! RLU after the x10 network, DC V 6 V's calibration, the zero absorbs OP1's
//! held offset; its gain is nominal (test 7 measures it). C 6 V: E7's row
//! (the /100 tap, OP1 x10) and self-cal (AD1 on SENSE - PB5, DC V 60 V's
//! calibration), i.e. candidate (i) for the reference.
//!
//! DC mV / AC mV (modes 2 / 3) stay on A: no merged mV row is defined. No
//! misconnect sentinel in the method rows (Kind::Off): the AC V one uses
//! AD1, the DC V one takes R20 that the frequency counter needs.
//! Readings: one per RMS result (~9 / s), total = hypot(AC, DC) as A;
//! AUTO coupling evaluates once both parts were refreshed since its last
//! evaluation (~every 107 ms, AUTO_VOTES of them to switch).

use super::*;

/// Methods (DBG:ACDC:METH).
pub const METH_A: u8 = 0;
pub const METH_B: u8 = 1;
pub const METH_C: u8 = 2;
/// Steps of a range visit: the stock DC row (reference), the method row's
/// first results (zero / offset), running.
const ST_REF: u8 = 0;
const ST_MEAS: u8 = 1;
const ST_RUN: u8 = 2;
/// Results per step (the first one of the MEAS step dropped as well).
const REF_N: u8 = 3;
const MEAS_N: u8 = 3;
/// B: AD1 results in the DC part's moving mean, restart threshold (codes).
const B_AVG: usize = 5;
const B_AVG_THR: i32 = 0x206;
/// C V: AD2 / LPF headroom (codes of 2^18): up-range when |LPF| +
/// 1.414 x sqrt(RMS) exceeds it (the DC-coupled node has one window for
/// DC and AC; acdc-approaches.md 7 item 5).
const C_PEAK: f32 = 0.9 * 262_144.0;
/// C A: the entry reference sets a gain (else an offset) from this
/// fraction of the range's full scale.
const C_A_GAIN: f32 = 0.1;
/// Slots: V ranges 0..3, A 4 + (unit * 2 + range).
const SLOTS: usize = 10;

pub struct Am {
    pub meth: u8,
    /// B 6 V candidate: 1 = (i) SENSE - PB5, 2 = (ii) OP1O - RLU.
    pub r0: u8,
    st: u8,
    n: u8,
    acc: f32,
    ref_v: f32,
    /// Per slot: B zero (DC units of the range), C A offset; valid / fixed
    /// (DBG:ACDC:ZERO) bits.
    corr: [f32; SLOTS],
    /// C A: the LPF gain factor per A slot (1.0: offset only).
    cgain: [f32; SLOTS - 4],
    ok: u16,
    fixed: u16,
    /// Reference (stock DC row) taken this session, per slot.
    refd: u16,
    /// Parts refreshed since the last AUTO evaluation (bit 0 AC, 1 DC).
    fresh: u8,
    /// Counters (DBG:ACDC:STAT?): AC results, DC results, readings.
    pub n_ac: u32,
    pub n_dc: u32,
    pub n_rd: u32,
    /// Raw LPF / sqrt(RMS) codes of the last C result (window check, STAT?).
    lpf: f32,
    sq: f32,
}

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut AM: core::mem::MaybeUninit<Am> = core::mem::MaybeUninit::uninit();

/// Initialise and hand out the one `Am` (call once: `Dmm::new`).
pub fn am_static() -> &'static mut Am {
    unsafe {
        (*core::ptr::addr_of_mut!(AM)).write(Am {
            meth: METH_A,
            r0: 1,
            st: ST_REF,
            n: 0,
            acc: 0.0,
            ref_v: 0.0,
            corr: [0.0; SLOTS],
            cgain: [1.0; SLOTS - 4],
            ok: 0,
            fixed: 0,
            refd: 0,
            fresh: 0,
            n_ac: 0,
            n_dc: 0,
            n_rd: 0,
            lpf: 0.0,
            sq: 0.0,
        })
    }
}

/// Slot of DMM mode `m` (V or A, DC or AC) at range `r`.
fn slot(m: u8, r: i8) -> usize {
    let r = r.max(0) as usize;
    match m {
        0 | 1 => r.min(3),
        _ => 4 + ((dc_mode_of(m) - 7) as usize * 2 + r.min(1)).min(5),
    }
}

/// DC V volts of an AD1 code at DC V tap / calibration `r` (poll_dcv's
/// formula without the averaging).
pub(super) fn dcv_of(code: f32, r: usize) -> f32 {
    let mut v = dcv_volts(code, r);
    if v < 0.0 {
        v *= DCV_NEGK[r];
    }
    if r == 0 && v.abs() / 20000.0 > 3e-5 {
        v += 1e-4 - v.abs() / 20000.0;
    }
    v
}

/// DC A value (A / mA / uA of unit `u`) of an AD1 code at range `r`.
pub(super) fn dca_of(code: f32, u: usize, r: usize) -> f32 {
    let i = (u * 2 + r) as u32;
    code * 1.25 / (SHUNT[u] * MV_DIV[r] * 0.9) / AD1_DIV * cal(0x94 + 4 * i, 1.0) - cal(0xAC + 4 * i, 0.0)
}

/// AC row scale of AC mode `m` at range `r` (poll_ac): (volts or units per
/// code, calibration index, RMS gain / offset offsets).
pub(super) fn ac_scale(m: u8, r: usize) -> (f32, u32, u32, u32) {
    match m {
        0 => (K_ACV[r] * 1.25 / 262_144.0, r as u32, 0x34, 0x44),
        2 => (1.25 / MV_DIV[r] / 262.144, r as u32, 0x54, 0x5C),
        _ => {
            let u = (m - 4) as usize;
            (1.25 / (SHUNT[u] * MV_DIV[r]) / 262_144.0, (u * 2 + r) as u32, 0xC4, 0xDC)
        }
    }
}

impl Dmm {
    /// A method other than A runs this V / A AC+DC session (not mV, not
    /// the power meter).
    pub(super) fn am_on(&self) -> bool {
        self.acdc && self.am.meth != METH_A && !self.pw.on && matches!(self.mode, 0 | 1 | 4..=9)
    }

    /// AC+DC (re)entry (`set_sub`): the zeros / offsets not fixed with
    /// DBG:ACDC:ZERO are measured again.
    #[inline(never)] // .cold (link-native.x): entry / range change / SCPI only
    pub(super) fn am_enter(&mut self) {
        let a = &mut *self.am;
        a.ok = a.fixed;
        a.refd = 0;
        a.st = ST_REF;
        a.n = 0;
        a.acc = 0.0;
        a.fresh = 0;
    }

    /// DBG:ACDC:METH: switch and restart the session (AC+DC / AUTO).
    #[inline(never)] // .cold (link-native.x): entry / range change / SCPI only
    pub fn am_set_meth(&mut self, clk: &mut Clock, meth: u8) {
        self.am.meth = meth.min(METH_C);
        if self.acdc && matches!(self.mode, 0 | 1 | 4..=9) && !self.pw.on {
            let s = self.sub();
            self.set_sub(clk, s);
        }
    }

    /// DBG:ACDC:ZERO <slot>,<value>: fix a slot's correction (B: zero, C A:
    /// offset; the DC unit of the slot's range); NaN = measure it again.
    #[inline(never)] // .cold (link-native.x): entry / range change / SCPI only
    pub fn am_set_zero(&mut self, s: u8, v: f32) {
        let s = (s as usize).min(SLOTS - 1);
        let a = &mut *self.am;
        if v.is_nan() {
            a.fixed &= !(1 << s);
            a.ok &= !(1 << s);
        } else {
            a.corr[s] = v;
            a.fixed |= 1 << s;
            a.ok |= 1 << s;
        }
    }

    /// DBG:ACDC:ZERO?: (valid mask, fixed mask, the corrections).
    #[inline(never)] // .cold (link-native.x): entry / range change / SCPI only
    pub fn am_zero(&self) -> (u16, u16, [f32; SLOTS]) {
        (self.am.ok, self.am.fixed, self.am.corr)
    }

    /// DBG:ACDC:STAT?: meth, step, mode, range, ac, dc, n_ac, n_dc, n_rd,
    /// Hz, the slot's correction, C V per_code, offset, LPF codes, sqrt(RMS)
    /// codes (app.rs formats the first four and n_* as integers).
    #[inline(never)] // .cold (link-native.x): entry / range change / SCPI only
    pub fn am_stat(&self) -> [f32; 15] {
        let a = &*self.am;
        let (pc, off) = dc_cal(self.range).unwrap_or((0.0, 0.0));
        [
            a.meth as f32,
            a.st as f32,
            self.mode as f32,
            self.range.max(0) as f32,
            self.f_ac,
            self.dc_val,
            a.n_ac as f32,
            a.n_dc as f32,
            a.n_rd as f32,
            self.freq.f_khz * 1000.0,
            a.corr[slot(self.mode, self.range)],
            pc,
            off,
            a.lpf,
            a.sq,
        ]
    }

    /// `write_config` with a method on: the mode of the step (the stock DC
    /// row for a reference, the AC mode for the method row). Returns
    /// whether the AC mode's method row is to be written.
    #[inline(never)] // .cold (link-native.x): entry / range change / SCPI only
    pub(super) fn am_mode(&mut self) -> bool {
        let s = slot(self.mode, self.range);
        let v = self.mode <= 1;
        let cv = v && self.am.meth == METH_C;
        let need_ref = (self.am.ok | self.am.refd) & (1 << s) == 0 && !cv;
        let dm = dc_mode_of(self.mode);
        self.mode = if need_ref { dm } else { ac_mode_of(dm) };
        let a = &mut *self.am;
        a.n = 0;
        a.acc = 0.0;
        a.st = if need_ref {
            ST_REF
        } else if cv || a.ok & (1 << s) != 0 {
            ST_RUN
        } else {
            ST_MEAS
        };
        !need_ref
    }

    /// The row of the step (`write_config`, after the relays): the stock DC
    /// row `cfg` (V: OP1 on SENSE, R32 0x38, as the stock DC phase keeps
    /// PB4 charged) or the method row. C V runs E7's self-cal here first
    /// when the range has none (~0.45 s, blocking).
    #[inline(never)] // .cold (link-native.x): entry / range change / SCPI only
    pub(super) fn am_row(&mut self, cfg: &[u8; hy::CFG_LEN]) -> [u8; hy::CFG_LEN] {
        let (m, r) = (self.mode, self.range.max(0) as usize);
        let mut c = *cfg;
        if matches!(m, 1 | 7..=9) {
            if m == 1 {
                c[18] = 0x38;
            }
            return c;
        }
        if self.am.meth == METH_C {
            if m == 0 {
                let s = slot(m, self.range);
                if self.am.ok & (1 << s) == 0 {
                    crate::ad2exp::dc_enter(1, r as u8, 0, true);
                    self.am.ok |= 1 << s;
                }
                c = crate::ad2exp::dcv_row(r as u8, 0, 0, 0, true);
                c[9] = 0xF8; // RMS, LPF, LPFBW 111 (the AC V row's)
                c[20] = 0x88; // AD3 on, sum(X * Y)
                c[21] = 0x14; // AD3 = OP1O - AGND, AD3IG 1.0
            }
            return c;
        }
        // B: the DC row's AD1 bytes (OSR, enable + buffers, inputs, refs).
        let d = if m == 0 {
            CFG_F1[r.max(1).min(3)]
        } else if m == 4 {
            CFG_F7[r.min(1)]
        } else {
            CFG_F8_9[r.min(1)]
        };
        for j in [2usize, 3, 4, 8] {
            c[j] = d[j];
        }
        c[5] = (c[5] & 0xCF) | (d[5] & 0x30);
        c[9] &= 0xF8; // no peak hold
        c[19] &= 0x3F; // OP1's chopper held
        if m == 0 && r == 0 && self.am.r0 == 2 {
            c[4] = CFG_F1[0][4]; // OP1O - RLU
        }
        c
    }

    /// The DC row's reading in a reference step (`dc_out`): the stock DC
    /// part; after REF_N of them the method row of the range.
    #[inline(never)] // .cold (link-native.x): entry / range change / SCPI only
    pub(super) fn am_ref(&mut self, clk: &mut Clock, x: f32, lim: f32, dec: u8, unit: &'static str, si_k: f32, before: (u8, i8)) -> Option<Reading> {
        if (self.mode, self.range) != before {
            return None;
        }
        self.dc_val = x;
        self.acdc_seen |= 2;
        self.am.fresh |= 2;
        let rd = (self.acdc_seen == 3).then(|| self.acdc_reading(lim, dec, unit, si_k));
        let a = &mut *self.am;
        a.acc += x;
        a.n += 1;
        if a.n >= REF_N {
            a.ref_v = a.acc / a.n as f32;
            let (m, r) = (ac_mode_of(self.mode), self.range);
            a.refd |= 1 << slot(m, r);
            // The method row: MEAS (`am_mode`: a reference taken).
            self.switch_phase(clk, m, r, if m == 0 { 450 } else { 550 });
            self.avg.reset();
        }
        if rd.is_some() {
            self.am.n_rd += 1;
        }
        rd
    }

    /// Poll a method row (AC mode, `am_on`): one INTF read serves AD1 (B)
    /// and the RMS / LPF block.
    pub(super) fn poll_am(&mut self, clk: &mut Clock, now: u64) -> Option<Reading> {
        let (m, r) = (self.mode, self.range.max(0) as usize);
        let intf = hy::read1(hy::R_INTF);
        let s = slot(m, self.range);
        let meth = self.am.meth;
        let v = m == 0;
        if meth == METH_B && intf & hy::INTF_AD1 != 0 {
            let mut b = [0u8; 3];
            hy::read(hy::R_AD1, &mut b);
            let code = hy::s24(&b);
            if self.am.st == ST_MEAS {
                let x = self.am_dc_of(code as f32, m, r);
                let a = &mut *self.am;
                a.n += 1;
                if a.n > 1 {
                    a.acc += x;
                }
                if a.n > MEAS_N {
                    a.corr[s] = a.acc / (a.n - 1) as f32 - a.ref_v;
                    a.ok |= 1 << s;
                    a.st = ST_RUN;
                    self.avg.reset();
                }
            } else if self.am.st == ST_RUN {
                let mean = self.avg.push(code, B_AVG, B_AVG_THR);
                self.dc_val = self.am_dc_of(mean, m, r) - self.am.corr[s];
                self.acdc_seen |= 2;
                self.am.fresh |= 2;
                self.am.n_dc += 1;
            }
        }
        if intf & hy::INTF_RMS == 0 {
            return None;
        }
        let mut b = [0u8; 14];
        hy::read(hy::R_AD1, &mut b);
        let rms = (b[9] as u64) | (b[10] as u64) << 8 | (b[11] as u64) << 16 | (b[12] as u64) << 24 | ((b[13] & 0x1F) as u64) << 32;
        let sq = sqrt(rms as f32);
        if sq > 262_144.0 {
            return None;
        }
        let lpf = hy::lpf19(&b[6..9]) as f32;
        let (sc, ci, g_rms, o_rms) = ac_scale(m, r);
        let gain = sc * cal(g_rms + 4 * ci, 1.0);
        let cv = meth == METH_C && v;
        let (pc, off) = if cv {
            dc_cal(self.range).unwrap_or_else(|| (ad2_scale_of(1, self.range).map_or(sc, |a| a.per_code), 0.0))
        } else {
            (0.0, 0.0)
        };
        let ac = if cv { sq * pc } else { (sq * gain - cal(o_rms + 4 * ci, 0.0)).max(0.0) };
        self.am.lpf = lpf;
        self.am.sq = sq;
        if meth == METH_C {
            // C: the DC part from the LPF (V: E7's self-cal; A: the AC A
            // gain with the entry correction: a gain factor when the
            // reference was >= C_A_GAIN of the range, else an offset).
            let x = if cv { lpf * pc - off } else { lpf * gain };
            let fs = self.factor() * DEC4[((m - 4) as usize + r).min(3)];
            if self.am.st == ST_MEAS {
                let a = &mut *self.am;
                a.n += 1;
                if a.n > 1 {
                    a.acc += x;
                }
                if a.n > MEAS_N {
                    let mean = a.acc / (a.n - 1) as f32;
                    a.corr[s] = mean - a.ref_v;
                    a.cgain[s - 4] = 1.0;
                    if a.ref_v.abs() >= C_A_GAIN * fs && (a.ref_v / mean - 1.0).abs() < 0.05 {
                        a.cgain[s - 4] = a.ref_v / mean;
                        a.corr[s] = 0.0;
                    }
                    a.ok |= 1 << s;
                    a.st = ST_RUN;
                }
                return None;
            }
            self.dc_val = if cv { x } else { x * self.am.cgain[s - 4] - self.am.corr[s] };
            self.acdc_seen |= 2;
            self.am.fresh |= 2;
            self.am.n_dc += 1;
        }
        if self.am.st != ST_RUN {
            return None;
        }
        let (thr, a_rms) = if v { (ACV_WIN[r], 0.25) } else { (ACA_WIN[(m - 4) as usize * 2 + r], 0.15) };
        self.ac_raw = ac;
        ema(thr, a_rms, ac, &mut self.f_ac);
        self.acdc_seen |= 1;
        self.am.fresh |= 1;
        self.am.n_ac += 1;
        let f = self.factor();
        let ov = self.over();
        let (lim, unit, dec, si_k) = if v {
            ((f * DEC4[r]).min(1000.0), "V", 4 - r as u8, 1.0)
        } else {
            let u = (m - 4) as usize;
            let lim = (f * DEC4[u + r]).min(if m == 4 { 10.0 } else { f32::MAX });
            match u {
                0 => (lim, "A", 4 - r as u8, 1.0),
                1 => (lim, "mA", 3 - r as u8, 1e-3),
                _ => (lim, "\u{b5}A", 2 - r as u8, 1e-6),
            }
        };
        let mag = hypot(ac, self.dc_val);
        if !v && self.ovl_trip(clk, m - 4, mag > ov * lim) {
            return None;
        }
        let before = (m, self.range);
        if !self.manual {
            let up = mag > ov * lim || (cv && lpf.abs() + 1.414 * sq > C_PEAK);
            let down = mag < 0.1 * lim / ov;
            if v {
                if up && r < 3 {
                    self.set(clk, 0, r as i8 + 1, 450);
                } else if down && r > 0 {
                    self.set(clk, 0, r as i8 - 1, 450);
                } else if down && self.v_auto & 2 != 0 {
                    self.set(clk, 3, 1, 600);
                }
            } else if up && r == 0 {
                self.set(clk, m, 1, 550);
            } else if up && m > 4 && self.a_auto & 1 != 0 {
                self.set(clk, m - 1, 0, 650);
            } else if down && r == 1 {
                self.set(clk, m, 0, 550);
            } else if down && r == 0 && m < if self.a_auto == 3 { 6 } else { 5 } && self.ovl_allows(m + 1 - 4) {
                self.set(clk, m + 1, 1, 650);
            }
        }
        if (self.mode, self.range) != before || self.acdc_seen != 3 {
            return None;
        }
        if self.cpl_auto && self.am.fresh == 3 {
            self.auto_eval(lim);
            self.am.fresh = 0;
        }
        if !v {
            self.freq_secondary(now, 0xF0);
        }
        self.am.n_rd += 1;
        Some(self.acdc_reading(lim, dec, unit, si_k))
    }

    /// B's DC part of an AD1 code in the merged row of AC mode `m`.
    fn am_dc_of(&self, code: f32, m: u8, r: usize) -> f32 {
        if m == 0 {
            dcv_of(code, if r == 0 && self.am.r0 == 2 { 0 } else { r.max(1) })
        } else {
            dca_of(code, (m - 4) as usize, r)
        }
    }
}
