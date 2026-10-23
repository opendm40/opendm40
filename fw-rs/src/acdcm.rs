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
//!   `dcv_row`: AD2 on OP1O, OP1's chopper held) with AD3 on the same node,
//!   the RMS / LPF on and AD2's peak hold: DC part = LPF, AC part =
//!   sqrt(RMS) (the HPF in the RMS block takes the DC out), one fixed row,
//!   nothing switched, both parts every LPF block (106.7 ms). DC part: per
//!   range and session an anchor (`am_cal_step`, 7409b92 device fix): the
//!   stock DC V row's reading (AD1 alone, the DC V function's own row and
//!   calibration = what READ? shows) against the LPF of this very row (AD3
//!   and the RMS on: E7's self-cal anchored AD1 in the AC V row's switches
//!   and averaged AD2 with AD3 off, and read the 60 V DC part +11 mV +
//!   0.05 % off); the gain from an internal zero (OP1+ on AGND) when the
//!   span is >= 10 k codes, kept per range over sessions, else the table.
//!   The internal zero is not a real 0 V input (device 2563fe1: ~15 codes
//!   off on 60 V, the gain 0.030 % high, -8 mV at 0 V), so once the input
//!   has settled >= 10 k codes away from a reference point of the session
//!   C takes the stock row's reading there too (`am_refine`, ~1 s of DC V
//!   readings) and the gain from the two READ? points: measured against the
//!   very reading the DC part is meant to equal, nothing assumed of the
//!   hardware. "Settled" = the LPF has stopped creeping (device c6f9670: 5..12
//!   codes still going after 3.6 s, a 6 V gain 143..195 ppm off): its slope
//!   over the last ~3 s below 1 code / s, the new point = the fitted line's
//!   last value, taken before the reference row is written (acdc-approaches.md
//!   10.12). A two-point gain over a shorter span than the gain in use does
//!   not replace it, except an internal zero's that it shows biased (their
//!   difference over its span beyond the window's noise: 60 V's AGND zero);
//!   kept per range until power-off.
//!   AC part: sqrt(RMS) x the factory AC V calibration (gain and offset, as
//!   A: device e2 put it within 0.014 % of A, the DC gain 0.13 / 0.23 %
//!   high), used only when AD2's peak hold over the last two results allows
//!   it (sqrt(RMS) <= p-p / 2: no waveform has more); otherwise the RMS
//!   block is in its HPF transient or returned garbage (device: >= 5 V DC on
//!   6 V) and, with a steady node, the AC part is that bound (an upper
//!   limit), else none: no reading, and after RMS_BAD_MS up a range. A: the
//!   stock AC A row as it is (AD2 on the shunt node is
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
//! (the /100 tap, OP1 x10), anchored to the stock DC V 6 V row (the /10
//! tap, its own calibration).
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
/// C V: the internal zero step (the method row with OP1+ on AGND).
const ST_ZERO: u8 = 3;
/// B: AD1 results in the DC part's moving mean, restart threshold (codes).
const B_AVG: usize = 5;
const B_AVG_THR: i32 = 0x206;
/// C V: AD2's peak hold at 90 % of its span (2^18 codes) = the node's
/// one window for DC and AC (acdc-approaches.md 7 item 5) full: up-range.
const C_PK_UP: i32 = 235_930;
/// C V anchor: LPF results dropped after a row write (its settle already
/// waited) / after the zero's OP1+ switch (the LPF window is 2 blocks),
/// then CAL_N_ZERO / CAL_N averaged whose spread is within max(60 codes,
/// 1 %: an AC part's LPF ripple, <= 0.5 % at 40 Hz, passes; a moving input
/// does not: taken again, CAL_TRIES times at most).
const CAL_DROP_ROW: u8 = 1;
const CAL_DROP_SW: u8 = 2;
const CAL_N_ZERO: u8 = 2;
const CAL_N: u8 = 4;
const CAL_TRIES: u8 = 4;
/// |signal - zero| (codes) from which the anchor takes the gain too, and
/// how far from the table's it may be (as E7's self-cal).
const CAL_GAIN_CODES: f32 = 10_000.0;
const CAL_GAIN_TOL: f32 = 0.03;
/// R32 with OP1+ on AGND (the zero), OP2 bits of the row kept.
const R32_OP1_AGND: u8 = 0x0B;
/// sqrt(RMS) may exceed half the peak hold's p-p by this (codes: noise,
/// the RMS window reaching ~1 block further back than the two holds).
const PK_MARGIN: f32 = 24.0;
/// The RMS not usable with AC on the node this long: up a range (ms).
const RMS_BAD_MS: u64 = 2500;
/// How long the last AC part from the RMS stands in for a peak bound (ms).
const AC_OK_MS: u64 = 2000;
/// C V refinement (`am_refine`): the last SLOPE_N LPF results (~3 s) within
/// the anchor's spread rule and their fitted slope below SLOPE_MAX codes / s
/// (LPF_T s per result) = a settled input and LPF (device c6f9670: the LPF
/// crept 5..12 codes for ~8 s after a step; noise ~1.7 codes, so ~3 s for a
/// slope error of ~0.3 code / s), and not before SLOPE_MIN results at the
/// level (~5 s: the device's tail stayed flat for ~4 s, then fell); not
/// settled after SLOPE_TMO results at one level: no reference there until
/// the input moves.
const SLOPE_N: usize = 28;
const SLOPE_MAX: f32 = 1.0;
const LPF_T: f32 = 0.1067;
const SLOPE_MIN: u8 = 48;
const SLOPE_TMO: u8 = 120;
/// ... and only on a node without a real AC part: AD2's peak hold p-p within
/// REF_PP_K x |LPF| + REF_PP_MIN codes (noise ~240 codes p-p; a reference
/// rewrites the row, and the RMS block's HPF transient after it passes the
/// p-p / 2 bound when the node has a large AC part: a spike in the AC part,
/// the emulator's 40 Hz square; the LPF's ripple also enters the window).
const REF_PP_K: f32 = 0.02;
const REF_PP_MIN: f32 = 1000.0;
/// The run row's first results after the second reference within this of the
/// window's end value (codes), else the input moved: no gain from it.
const GAIN_MOVE: f32 = 12.0;
/// A shorter-span two-point gain replaces an internal zero's when the two
/// differ at its far point by more than BIAS_K x the window's residual rms
/// (the scale of each of the four inputs' noise: two LPF means, two READ?s,
/// ~1 code each in the emulator) plus what a tail still creeping at the
/// window's slope may leave (|slope| x CREEP_TAU s: the time constant of
/// the c6f9670 trace's tail, fitted as two lags of 2.5 s), at least BIAS_MIN
/// codes.
/// The LPF's creep lags the input step: the new point sits toward the level
/// before it, which moves the two-point gain one way only (the "lag side":
/// a larger gain when the farther reference point lies beyond the level
/// before, as on every step down / up of e8). On the lag side the two lines
/// must differ by CREEP_K x the step into this level (codes) more. Device
/// f42a09e (acdc-approaches.md 10.14): 6 V's accurate internal zero vs the
/// two-point gains at the new point: -0.9..+13 codes, 16 of 18 on the lag
/// side, all beyond 1 code (5 -> 3 V, 41.6 k: +3.9..+13.0, 10 of 10); 60 V's
/// biased zero: -2.5..-10 codes, 12 of 12 on the other side.
const BIAS_K: f32 = 3.0;
const CREEP_TAU: f32 = 2.5;
const BIAS_MIN: f32 = 2.0;
const CREEP_K: f32 = 4.0e-4;
/// A window this many results steady before it restarts: its newest result
/// is the level before the step (`prevl`).
const PREV_N: u8 = 10;
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
    /// Counters (DBG:ACDC:STAT?): AC results, DC results, AC+DC readings
    /// (every method: dmm.rs `acdc_reading`).
    pub n_ac: u32,
    pub n_dc: u32,
    pub n_rd: u32,
    /// Raw LPF / sqrt(RMS) codes of the last C result (window check, STAT?).
    lpf: f32,
    sq: f32,
    /// C V per range: the session's anchor (per_code, offset; valid with
    /// the slot's `ok` bit), the measured gain and the span (codes) it was
    /// measured over (0: none, the table's; kept over sessions).
    cpc: [f32; 4],
    coff: [f32; 4],
    cg: [f32; 4],
    cgsp: [f32; 4],
    /// C V per range: the gain is from two references (bit per range), the
    /// span (codes) of the last two-point attempt (the next one needs 1.5 x
    /// more; kept as `cg`).
    c2p: u8,
    ctry: [f32; 4],
    /// C V: a second reference is being taken (`am_refine`): its range, the
    /// first point (LPF codes, reference V), the new point's LPF codes (the
    /// window's fitted end) and the window's residual rms (codes).
    pend: bool,
    pr: u8,
    pm: f32,
    pv: f32,
    pm2: f32,
    psig: f32,
    pslope: f32,
    /// C V per range, this session: the reference points (LPF codes, V) with
    /// the lowest / highest code (bit per range: any), paired with a new one
    /// for the larger span.
    plo: [(f32, f32); 4],
    phi: [(f32, f32); 4],
    pts: u8,
    /// C V per range: where the gain in use is from (0 the table, 1 the
    /// internal zero, 2 two references, 3 two references over a shorter span
    /// than a biased internal zero's); the last two-point gain's difference
    /// to the gain in use over its span (codes); the last window's slope
    /// (codes / s) (STAT?).
    gsrc: [u8; 4],
    /// C V: the LPF level (codes) before the last input step and its range
    /// (0xFF none); the step into the level of the pending second reference
    /// (new point - prevl, codes, 0 unknown) and the last bias threshold
    /// (codes, STAT?).
    prevl: f32,
    prevr: u8,
    pstep: f32,
    bthr: f32,
    /// C V: the range of the last row write (-1 none this session).
    lr: i8,
    dis: f32,
    slope: f32,
    /// C V in ST_RUN: the last SLOPE_N LPF results (ring, `wi` the oldest
    /// once full), how many, results since the window last restarted.
    ring: [f32; SLOPE_N],
    wn: u8,
    wi: u8,
    wst: u8,
    /// C V anchor in progress: the zero (LPF codes), the min / max of the
    /// results averaged (in `acc`), zero taken, retries.
    cz: f32,
    c1: f32,
    c2: f32,
    zdone: bool,
    tries: u8,
    /// AD2's peak hold of the previous result (min, max), the last p-p
    /// (codes, -1 none) and the AC part's source (0 RMS, 1 the peak bound,
    /// 2 none), since when (ms, 0 never) the RMS is not usable with AC on
    /// the node, and per V range the |DC part| at which its RMS failed (a
    /// down-range into it waits for |DC| < 0.9 x that; this session).
    pk_prev: (i32, i32),
    pp: f32,
    acsrc: u8,
    bad_t: u64,
    cfault: [f32; 4],
    /// The last AC part the RMS gave (V) and when (ms, 0 none): a bound
    /// above it keeps it (the bound only says "at most").
    ac_ok: f32,
    ac_ok_t: u64,
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
            cpc: [0.0; 4],
            coff: [0.0; 4],
            cg: [0.0; 4],
            cgsp: [0.0; 4],
            c2p: 0,
            ctry: [0.0; 4],
            pend: false,
            pr: 0,
            pm: 0.0,
            pv: 0.0,
            pm2: 0.0,
            psig: 0.0,
            pslope: 0.0,
            plo: [(0.0, 0.0); 4],
            phi: [(0.0, 0.0); 4],
            pts: 0,
            gsrc: [0; 4],
            prevl: 0.0,
            prevr: 0xFF,
            pstep: 0.0,
            bthr: 0.0,
            lr: -1,
            dis: 0.0,
            slope: 0.0,
            ring: [0.0; SLOPE_N],
            wn: 0,
            wi: 0,
            wst: 0,
            cz: 0.0,
            c1: 0.0,
            c2: 0.0,
            zdone: false,
            tries: 0,
            pk_prev: (i32::MAX, i32::MIN),
            pp: -1.0,
            acsrc: 2,
            bad_t: 0,
            cfault: [0.0; 4],
            ac_ok: 0.0,
            ac_ok_t: 0,
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

/// C V: the table's per_code of range `r` (dmm::DCV_AD2_GAIN: the anchor's
/// gain until one is measured).
fn c_tab(r: usize) -> f32 {
    K_ACV[r.min(3)] * 1.25 / 262_144.0 * DCV_AD2_GAIN[r.min(3)]
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
        a.cfault = [0.0; 4];
        a.bad_t = 0;
        a.pend = false;
        a.pts = 0;
        a.lr = -1;
        a.prevr = 0xFF;
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
    /// codes, C V: AD2 peak p-p codes (-1 none), the AC part's source (0
    /// RMS, 1 the peak bound, 2 none), the gain's span in codes (0: the
    /// table), where the gain is from (`gsrc`), the last two-point gain's
    /// difference to the gain in use over its span (codes), the last steady
    /// window's slope (codes / s), and of the last second-reference attempt:
    /// the window's residual rms (codes), the bias threshold gdis was held
    /// to (codes), the window's slope, the step into the level (codes, 0
    /// unknown) (app.rs formats the first four, n_*, the source and gsrc as
    /// integers).
    #[inline(never)] // .cold (link-native.x): entry / range change / SCPI only
    pub fn am_stat(&self) -> [f32; 25] {
        let a = &*self.am;
        let r = self.range.clamp(0, 3) as usize;
        let cal = self.mode <= 1 && a.meth == METH_C && a.ok & (1 << r) != 0;
        let (pc, off) = if cal { (a.cpc[r], a.coff[r]) } else { (0.0, 0.0) };
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
            a.pp,
            a.acsrc as f32,
            a.cgsp[r],
            a.gsrc[r] as f32,
            a.dis,
            a.slope,
            a.psig,
            a.bthr,
            a.pslope,
            a.pstep,
        ]
    }

    /// `write_config` with a method on: the mode of the step (the stock DC
    /// row for a reference, the AC mode for the method row). Returns
    /// whether the AC mode's method row is to be written.
    #[inline(never)] // .cold (link-native.x): entry / range change / SCPI only
    pub(super) fn am_mode(&mut self) -> bool {
        let s = slot(self.mode, self.range);
        let r = self.range.clamp(0, 3) as usize;
        let cv = self.mode <= 1 && self.am.meth == METH_C;
        if cv {
            // C V back on a range whose gain is still the table's (no internal
            // zero / two references yet: up to ~0.1 % off): its old anchor may
            // be far from the input now, so anchor it again (and the internal
            // zero with it if the span allows) rather than wait ~6 s for a
            // settled second reference (`am_refine`).
            let a = &mut *self.am;
            if a.lr != r as i8 && a.gsrc[r] == 0 && a.fixed & (1 << s) == 0 && !a.pend {
                a.ok &= !(1 << s);
                a.refd &= !(1 << s);
            }
            if a.lr != r as i8 {
                a.prevr = 0xFF;
            }
            a.lr = r as i8;
        }
        let need_ref = (self.am.ok | self.am.refd) & (1 << s) == 0;
        let dm = dc_mode_of(self.mode);
        self.mode = if need_ref { dm } else { ac_mode_of(dm) };
        let a = &mut *self.am;
        a.n = 0;
        a.acc = 0.0;
        a.zdone = false;
        a.tries = 0;
        a.pk_prev = (i32::MAX, i32::MIN);
        a.bad_t = 0;
        a.ac_ok_t = 0;
        a.wn = 0;
        a.wi = 0;
        a.wst = 0;
        if a.pend && (!cv || a.pr as usize != r) {
            a.pend = false; // the range changed during a refinement
        }
        // C V: the zero only when this anchor can measure a gain (a span of
        // >= CAL_GAIN_CODES, half again the one measured before) and the
        // range has no gain from two references; none in a refinement.
        let span = a.ref_v.abs() / c_tab(r);
        a.st = if need_ref {
            ST_REF
        } else if a.ok & (1 << s) != 0 {
            ST_RUN
        } else if cv && !a.pend && a.c2p & (1 << r) == 0 && span >= CAL_GAIN_CODES && span > 1.5 * a.cgsp[r] {
            ST_ZERO
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
            // B: OP1 kept on SENSE (PB4 charged, as the stock DC phase); C
            // V: the DC V function's row as it is (its READ?).
            if m == 1 && self.am.meth != METH_C {
                c[18] = 0x38;
            }
            return c;
        }
        if self.am.meth == METH_C {
            if m == 0 {
                c = crate::ad2exp::dcv_row(r as u8, 0, 0, 0, true);
                c[9] = 0xFC; // RMS, LPF, LPFBW 111 (the AC V row's), AD2's peak hold
                c[20] = 0x88; // AD3 on, sum(X * Y)
                c[21] = 0x14; // AD3 = OP1O - AGND, AD3IG 1.0
                if self.am.st == ST_ZERO {
                    c[18] = (c[18] & 0xF0) | R32_OP1_AGND;
                }
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
        let cv = meth == METH_C && v;
        let mut b = [0u8; 20];
        hy::read(hy::R_AD1, &mut b[..if cv { 20 } else { 14 }]);
        let rms = (b[9] as u64) | (b[10] as u64) << 8 | (b[11] as u64) << 16 | (b[12] as u64) << 24 | ((b[13] & 0x1F) as u64) << 32;
        let sq = sqrt(rms as f32);
        let lpf = hy::lpf19(&b[6..9]) as f32;
        self.am.lpf = lpf;
        self.am.sq = sq;
        if cv {
            return self.poll_am_cv(clk, r, &b, sq, lpf);
        }
        if sq > 262_144.0 {
            return None;
        }
        let (sc, ci, g_rms, o_rms) = ac_scale(m, r);
        let gain = sc * cal(g_rms + 4 * ci, 1.0);
        let ac = (sq * gain - cal(o_rms + 4 * ci, 0.0)).max(0.0);
        if meth == METH_C {
            // C A: the DC part from the LPF with the AC A gain and the
            // entry correction (a gain factor when the reference was >=
            // C_A_GAIN of the range, else an offset).
            let x = lpf * gain;
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
            self.dc_val = x * self.am.cgain[s - 4] - self.am.corr[s];
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
            let up = mag > ov * lim;
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
            self.auto_eval(lim, now);
            self.am.fresh = 0;
        }
        if !v {
            self.freq_secondary(now, 0xF0);
        }
        Some(self.acdc_reading(lim, dec, unit, si_k))
    }

    /// Method C, V: one RMS / LPF result of the DC-coupled row (`b`: R00..
    /// R13 with AD2's peak hold, `sq` sqrt(RMS), `lpf` the LPF codes). The
    /// anchor steps (`am_cal_step`), then the DC part from the LPF and the
    /// anchor, the AC part from the RMS with the factory AC V calibration
    /// when AD2's peak hold says it can be (module header), the range.
    fn poll_am_cv(&mut self, clk: &mut Clock, r: usize, b: &[u8; 20], sq: f32, lpf: f32) -> Option<Reading> {
        let (pmin, pmax) = (hy::lpf19(&b[14..17]), hy::lpf19(&b[17..20]));
        // Restart the hold for the next result (the R29 bits of the RMS /
        // LPF unchanged: poll_ac's PKH_RESTART).
        hy::write(0x29, &[self.r29 & !sentinel::R29_ENPKH]);
        hy::write(0x29, &[self.r29]);
        if self.am.st != ST_RUN {
            return self.am_cal_step(clk, r, lpf);
        }
        if self.am_refine(clk, r, lpf) {
            return None;
        }
        let now = clk.now_ms();
        let lim = (self.factor() * DEC4[r]).min(1000.0);
        let ov = self.over();
        let (sc, ci, g_rms, o_rms) = ac_scale(0, r);
        let gain = sc * cal(g_rms + 4 * ci, 1.0);
        let off_ac = cal(o_rms + 4 * ci, 0.0);
        let a = &mut *self.am;
        let x = lpf * a.cpc[r] - a.coff[r];
        // AD2's extremes over this result and the one before (~2 blocks:
        // the RMS window's length).
        let (lo, hi) = (pmin.min(a.pk_prev.0), pmax.max(a.pk_prev.1));
        a.pk_prev = (pmin, pmax);
        let pp = if hi >= lo && pmax >= pmin { (hi - lo) as f32 } else { -1.0 };
        a.pp = pp;
        let clip = pmax >= pmin && pmax.max(-pmin) >= C_PK_UP;
        a.fresh |= 2;
        a.n_dc += 1;
        // sqrt(RMS) <= p-p / 2 for any waveform (Popoviciu): more is the
        // RMS block's HPF transient after a row write / an input step, or
        // garbage. A steady node (the bound below the AUTO "off" floor):
        // the AC part is the last one the RMS gave within AC_OK_MS if the
        // bound allows it, else the bound (an upper limit, ~3 x the noise).
        let bound = 0.5 * pp + PK_MARGIN;
        let ac = if pp >= 0.0 && sq <= bound {
            a.acsrc = 0;
            let v = (sq * gain - off_ac).max(0.0);
            a.ac_ok = v;
            a.ac_ok_t = now.max(1);
            Some(v)
        } else if pp >= 0.0 && bound * gain <= AUTO_FLOOR_V[1] * lim {
            a.acsrc = 1;
            let fresh = a.ac_ok_t != 0 && now.saturating_sub(a.ac_ok_t) <= AC_OK_MS && a.ac_ok <= bound * gain;
            Some(if fresh { a.ac_ok } else { bound * gain })
        } else {
            a.acsrc = 2;
            None
        };
        if ac.is_some() {
            a.bad_t = 0;
        } else if a.bad_t == 0 {
            a.bad_t = now.max(1);
        }
        let failed = a.bad_t != 0 && now.saturating_sub(a.bad_t) >= RMS_BAD_MS;
        // The range below failed while the total was over it anyway (a
        // large AC part, measured here): not an RMS fault, forget it.
        let mag_ok = hypot(if a.acsrc == 0 { ac.unwrap_or(0.0) } else { 0.0 }, x);
        if r > 0 && a.acsrc == 0 && mag_ok > ov * lim / 10.0 {
            a.cfault[r - 1] = 0.0;
        }
        let fault = if r > 0 { a.cfault[r - 1] } else { 0.0 };
        self.dc_val = x;
        self.acdc_seen |= 2;
        let mag = hypot(ac.unwrap_or(0.0), x);
        if let Some(ac) = ac {
            self.ac_raw = ac;
        }
        if !self.manual {
            let up = clip || x.abs() > ov * lim || mag > ov * lim || failed;
            // Down only on a usable AC part, and not back into a range
            // whose RMS failed at about this DC.
            let down = ac.is_some() && mag < 0.1 * lim / ov && (fault == 0.0 || x.abs() < 0.9 * fault);
            if up && r < 3 {
                if failed && !clip {
                    self.am.cfault[r] = x.abs();
                }
                self.set(clk, 0, r as i8 + 1, 450);
                return None;
            } else if down && r > 0 {
                self.set(clk, 0, r as i8 - 1, 450);
                return None;
            } else if down && self.v_auto & 2 != 0 {
                self.set(clk, 3, 1, 600);
                return None;
            }
        }
        let ac = ac?;
        // The first AC part of the session / range as it is (the stock
        // EMA's 0.99 jump from 0 would show it 1 % low).
        if self.acdc_seen & 1 == 0 {
            self.f_ac = ac;
        } else {
            ema(ACV_WIN[r], 0.25, ac, &mut self.f_ac);
        }
        self.acdc_seen |= 1;
        self.am.fresh |= 1;
        self.am.n_ac += 1;
        if self.acdc_seen != 3 {
            return None;
        }
        if self.cpl_auto && self.am.fresh == 3 {
            self.auto_eval(lim, now);
            self.am.fresh = 0;
        }
        Some(self.acdc_reading(lim, 4 - r as u8, "V", 1.0))
    }

    /// Method C, V: a step of the range's anchor (module header; ST_ZERO /
    /// ST_MEAS after the stock DC V row's REF_N readings in `ref_v`): the
    /// LPF codes of the zero (OP1+ on AGND) and of the signal, then per_code
    /// (the measured gain when the zero was taken and the span is large
    /// enough, else the one kept for the range, else the table) and offset
    /// = signal x per_code - ref_v. A signal away from what the reference
    /// read (the input changed) takes the reference again.
    #[inline(never)] // .cold (link-native.x): a few results per range visit
    fn am_cal_step(&mut self, clk: &mut Clock, r: usize, lpf: f32) -> Option<Reading> {
        let a = &mut *self.am;
        a.n += 1;
        let drop = if a.st == ST_MEAS && a.zdone { CAL_DROP_SW } else { CAL_DROP_ROW };
        if a.n <= drop {
            a.acc = 0.0;
            a.c1 = f32::MAX;
            a.c2 = f32::MIN;
            return None;
        }
        a.acc += lpf;
        a.c1 = a.c1.min(lpf);
        a.c2 = a.c2.max(lpf);
        let k = a.n - drop;
        if k < if a.st == ST_ZERO { CAL_N_ZERO } else { CAL_N } {
            return None;
        }
        let m = a.acc / k as f32;
        if a.c2 - a.c1 > 60f32.max(0.01 * m.abs()) && a.tries < CAL_TRIES {
            a.tries += 1;
            a.n = drop; // the next CAL_N results again
            a.acc = 0.0;
            a.c1 = f32::MAX;
            a.c2 = f32::MIN;
            return None;
        }
        a.n = 0;
        a.tries = 0;
        if a.st == ST_ZERO {
            a.cz = m;
            a.zdone = true;
            a.st = ST_MEAS;
            hy::write(0x32, &[CFG_F0[r][18]]); // OP1+ back on SENSE
            let _ = hy::read1(hy::R_INTF);
            return None;
        }
        let (v, tab) = (a.ref_v, c_tab(r));
        let z = if a.zdone { a.cz } else { 0.0 };
        if ((m - z) * tab - v).abs() > 0.02 * v.abs() + 2500.0 * tab {
            a.refd &= !(1 << r);
            self.switch_phase(clk, 1, r as i8, 450);
            return None;
        }
        let mut m = m;
        if a.pend {
            // The second reference (`am_refine`): the gain between the first
            // point and the settled window's end (taken before this row was
            // written again), if these results still agree with it.
            a.pend = false;
            if (m - a.pm2).abs() <= GAIN_MOVE {
                m = a.pm2;
                let span = (m - a.pm).abs();
                let g = (v - a.pv) / (m - a.pm);
                if span >= CAL_GAIN_CODES && (g / tab - 1.0).abs() <= CAL_GAIN_TOL {
                    // A shorter span than the gain in use: only over an
                    // internal zero's that this one shows biased (the two
                    // lines apart at the new point by more than the points'
                    // noise and a creep's rest).
                    let cur = a.cgsp[r];
                    a.dis = if cur > 0.0 { (g / a.cg[r] - 1.0) * span } else { 0.0 };
                    // On the creep's lag side (the new point moved toward
                    // the level before: dis > 0 when the farther point p
                    // lies on that level's side) the step's creep allowance
                    // on top.
                    let lag = a.pstep * (m - a.pm);
                    let creep = if a.dis * lag > 0.0 { CREEP_K * a.pstep.abs() } else { 0.0 };
                    a.bthr = (BIAS_K * a.psig + a.pslope.abs() * CREEP_TAU).max(BIAS_MIN) + creep;
                    let take = cur == 0.0 || span >= cur || (a.gsrc[r] == 1 && a.dis.abs() > a.bthr);
                    if take {
                        a.gsrc[r] = if cur > 0.0 && span < cur { 3 } else { 2 };
                        a.cg[r] = g;
                        a.cgsp[r] = span;
                        a.c2p |= 1 << r;
                    }
                }
            }
        } else if a.zdone {
            let span = (m - a.cz).abs();
            let g = v / (m - a.cz);
            if span >= CAL_GAIN_CODES && (g / tab - 1.0).abs() <= CAL_GAIN_TOL {
                a.cg[r] = g;
                a.cgsp[r] = span;
                a.gsrc[r] = 1;
            }
        }
        // The session's reference points with the lowest / highest code.
        let b = 1u8 << r;
        if a.pts & b == 0 || m < a.plo[r].0 {
            a.plo[r] = (m, v);
        }
        if a.pts & b == 0 || m > a.phi[r].0 {
            a.phi[r] = (m, v);
        }
        a.pts |= b;
        let pc = if a.cgsp[r] > 0.0 { a.cg[r] } else { tab };
        a.cpc[r] = pc;
        a.coff[r] = m * pc - v;
        a.ok |= 1 << r;
        a.st = ST_RUN;
        a.pk_prev = (i32::MAX, i32::MIN);
        a.bad_t = 0;
        None
    }

    /// Method C, V, in ST_RUN: one LPF result into the steady window (the
    /// last SLOPE_N results within the anchor's spread rule); once it is full,
    /// SLOPE_MIN results old and its fitted slope below SLOPE_MAX (the LPF no
    /// longer creeping),
    /// if the fitted end value is >= CAL_GAIN_CODES from the session's
    /// reference point farthest from it (and 1.5 x further than the last
    /// two-point attempt there), with an AC part (RMS or the peak bound) and
    /// AD2's p-p small (no real AC on the node), the stock DC V row's reading is taken again: the anchor
    /// moves here and `am_cal_step` takes the gain from the two references
    /// (module header). Returns whether the reference was started.
    #[inline(never)] // per result (region B, as poll_am)
    fn am_refine(&mut self, clk: &mut Clock, r: usize, lpf: f32) -> bool {
        let a = &mut *self.am;
        let n = a.wn as usize;
        let (mut lo, mut hi) = (lpf, lpf);
        for &y in &a.ring[..n] {
            lo = lo.min(y);
            hi = hi.max(y);
        }
        if hi - lo > 60f32.max(0.01 * lpf.abs()) {
            if a.wst >= PREV_N && n > 0 {
                a.prevl = a.ring[(a.wi as usize + SLOPE_N - 1) % SLOPE_N];
                a.prevr = r as u8;
            }
            a.wn = 0;
            a.wi = 0;
            a.wst = 0;
        }
        a.ring[a.wi as usize] = lpf;
        a.wi = ((a.wi as usize + 1) % SLOPE_N) as u8;
        if (a.wn as usize) < SLOPE_N {
            a.wn += 1;
        }
        a.wst = a.wst.saturating_add(1);
        if (a.wn as usize) < SLOPE_N || a.wst < SLOPE_MIN || a.wst > SLOPE_TMO || a.acsrc == 2 || a.cpc[r] == 0.0 || a.pts & (1 << r) == 0 {
            return false;
        }
        // Least squares over the window (oldest at `wi`), relative to the
        // newest result (f32: codes up to 2^18).
        let nf = SLOPE_N as f32;
        let kb = 0.5 * (nf - 1.0);
        let (mut sy, mut sky) = (0.0f32, 0.0f32);
        for k in 0..SLOPE_N {
            let y = a.ring[(a.wi as usize + k) % SLOPE_N] - lpf;
            sy += y;
            sky += (k as f32 - kb) * y;
        }
        let mean = sy / nf;
        let b = sky / (nf * (nf * nf - 1.0) / 12.0);
        a.slope = b / LPF_T;
        if a.slope.abs() > SLOPE_MAX {
            return false;
        }
        let mut ss = 0.0f32;
        for k in 0..SLOPE_N {
            let e = a.ring[(a.wi as usize + k) % SLOPE_N] - lpf - mean - b * (k as f32 - kb);
            ss += e * e;
        }
        let end = lpf + mean + b * kb;
        if a.pp < 0.0 || a.pp > REF_PP_K * end.abs() + REF_PP_MIN {
            return false;
        }
        let (lo, hi) = (a.plo[r], a.phi[r]);
        let (p, d) = if (end - lo.0).abs() >= (end - hi.0).abs() { (lo, (end - lo.0).abs()) } else { (hi, (end - hi.0).abs()) };
        if d < CAL_GAIN_CODES || d <= 1.5 * a.ctry[r] {
            return false;
        }
        a.ctry[r] = d;
        a.pend = true;
        a.pr = r as u8;
        a.pm = p.0;
        a.pv = p.1;
        a.pm2 = end;
        a.psig = sqrt(ss / (nf - 2.0));
        a.pslope = a.slope;
        a.pstep = if a.prevr == r as u8 { end - a.prevl } else { 0.0 };
        // This level is the one before the next step, even when the next
        // step comes before the window after the reference is PREV_N steady.
        a.prevl = end;
        a.prevr = r as u8;
        a.ok &= !(1 << r);
        a.refd &= !(1 << r);
        self.switch_phase(clk, 1, r as i8, 450);
        true
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
