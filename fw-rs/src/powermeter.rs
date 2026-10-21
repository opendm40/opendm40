//! Power meter (W): Vrms, Irms, P, S, Q, PF, f and Wh / VAh at once
//! (docs/power-meter-and-misconnect.md section 3). A child module of dmm.rs
//! (`Dmm::pw`), so it drives the HY3131 with the Dmm's own state.
//!
//! Assignment B of the doc: the current on the stock AC A path (OP1 on the
//! shunt -> AD2 = OP1O - PB3, the f4 / f5_6 row bytes), the voltage through
//! OP2 <- SENSE (x1, DC-coupled) -> AD3 = OP2O - AGND, with the divider
//! switch bytes R2A..R30 of a stock V row (`power_row`). The RMS block has
//! one accumulator, so sum(X*Y) (P), sum(Y^2) (V) and sum(X^2) (I) are
//! sequential: R34 is switched P -> V -> P -> I -> .. and `disc` results are
//! discarded after each switch (device test T3 sets it). The LPF follows
//! SDO23: the DC of I in the P / I phases, of V in the V phase. The HPF
//! takes the DC out of every RMS statistic, so the totals add it back:
//!   P = P_ac + V_dc I_dc,  V = hypot(V_ac, V_dc),  I = hypot(I_ac, I_dc).
//! With the burden compensation on (R_path of the shunt in use) the values
//! are the load's: P_L = P - I^2 R, V_L^2 = V^2 - 2 R P + R^2 I^2 (exact for
//! any waveform: v_L = v - R i), S_L = V_L I, Q = sqrt(S^2 - P^2).
//!
//! The fallback (`alt`, Settings / SCPI POW:METH ALT, for when device test T1
//! shows OP2 on SENSE unusable) alternates the stock AC V and AC A rows as
//! AC+DC does: Vrms, Irms (stock calibration), S, VAh; no P, no PF.
//!
//! Relays: PA state 0 (V / A) in both methods, and the shunt MOSFETs always
//! follow the current range (`self.mode` = 4 + the I unit), also while the
//! fallback's V phase writes the stock AC V row: the stock AC V row would
//! select the 47.5 ohm shunt with the load current flowing.
//!
//! Positive direction: current into the A jack, out of COM (the stock's DC A
//! sign) with V/ohm on the source side of the broken conductor (doc 3.6
//! wiring 1): a consumer reads P > 0, a source P < 0.

use super::*;

/// Function code of the power meter (`Dmm::func`, SCPI, persist): after
/// 16 / 17 = V / A AC+DC.
pub const FUNC: u8 = 18;

/// V ranges (60000 counts as the AC V rows; 1000 V clamp): /10, /100,
/// /1000, /10000 taps.
pub const V_LIM: [f32; 4] = [6.0, 60.0, 600.0, 1000.0];
pub const V_NAMES: [&str; 4] = ["6V", "60V", "600V", "1000V"];
/// I ranges by index k (low to high): k = 0 600 uA .. 5 10 A; unit iu =
/// 2 - k / 2 (0 A, 1 mA, 2 uA), OP1 gain ir = k % 2 (0 x10, 1 x1).
pub const I_LIM: [f32; 6] = [600e-6, 6e-3, 0.06, 0.6, 6.0, 10.0];
pub const I_NAMES: [&str; 6] = ["600uA", "6mA", "60mA", "600mA", "6A", "10A"];
/// Shunt per unit (ohm): 10 mohm (PE0), 0.5 ohm (PE1), 47.5 ohm (none).
pub const SHUNT_OHM: [f32; 3] = [0.01, 0.5, 47.5];
const KV: [f32; 4] = [10.0, 100.0, 1000.0, 10_000.0];
/// AD2 / AD3 / LPF code at IG 1.0: 1.25 V / 2^18.
const CODE: f32 = 1.25 / 262_144.0;

/// R34 statistics: ENAD3 + SVXI = sum(X*Y) (P); ENAD3 = sum(X^2) (I);
/// ENAD3 + SVXI + SDO23 = sum(Y^2) (V; the LPF then follows AD3).
pub const R34_XY: u8 = 0x88;
pub const R34_XX: u8 = 0x80;
pub const R34_YY: u8 = 0x8C;
/// The phase cycle of method B.
const CYCLE: [u8; 4] = [R34_XY, R34_YY, R34_XY, R34_XX];
/// R20 for the frequency counter: SCMPI = SENSE (the V tap), ENCMP (the
/// counter restart writes R20 = this, then this | ENCTR).
const R20_SENSE: u8 = 0x10;
/// Settle after a row write (ms): the stock AC A one (AC V 450).
const SETTLE_MS: u64 = 550;
/// Results discarded after an R34 switch (doc 3.4: 1..4; T3 decides).
pub const DISC_DEFAULT: u8 = 2;
/// Fallback: results taken per phase after the settle.
const ALT_N: u8 = 2;
/// Fallback V phase: the stock AC V node is AC-coupled (tau ~0.3 s), its
/// first results after a row write still carry the coupling step (the stock
/// AC+DC V phase: 600 ms, 4 results): settle 600 ms and drop 4 (emulator:
/// 2 left +1.8 % of the coupling step in Vrms at 230 V).
const ALT_V_SETTLE_MS: u64 = 600;
const ALT_V_SKIP: u8 = 4;
/// Continuous-current limit defaults (doc 3.6, 5): above I_CONT a countdown
/// of T_LIM s. The manual gives 10 A max behind one 10 A ceramic fuse and no
/// duty-cycle figure; 6 A is the top regular range (DM40C 6 A / 10 A).
pub const I_CONT_DEFAULT: f32 = 6.0;
pub const T_LIM_DEFAULT: u16 = 30;
/// Integration counts as running (auto power-off held) above this current.
const I_FLOOR: f32 = 50e-6;
/// DBG:POW:RAW? ring.
pub const RING_N: usize = 16;

/// Coupling setting (`Pw::coup`, POW:COUP, persisted): AC = method B (the
/// RMS / LPF row; DC parts from the LPFs), DC = the AD1 method (the stock
/// DC V and DC A rows alternately, 24 bit, factory DC calibration), AUTO =
/// B until a full set is DC-like (`dc_like`), then DC until the comparator
/// on the V tap sees an AC swing in a V phase (`DC_AC_BITS`).
pub const COUP_AC: u8 = 0;
pub const COUP_DC: u8 = 1;
pub const COUP_AUTO: u8 = 2;
/// DC method: settle after a row write (no relay change: DC V and DC A
/// both use PA state 0, the shunt stays; the AD1 pre-filter and the first
/// chopped conversion), results dropped, results taken per phase (AD1 at
/// the DC rows' R22 0x15: OSR 8192 chopped, 100 ms). 150 ms is a guess (the
/// stock settles DC V 450 ms after a function change, with relay moves):
/// device re-check 5 of doc 3.12.
const DC_SETTLE_MS: u64 = 150;
const DC_SKIP: u8 = 1;
const DC_N: u8 = 2;
/// CTSTA CMPHO | CMPLO: both seen during a DC V phase = an AC swing on the
/// V tap around its DC (the window `cmp_window(c, 0)`-like, +-3 mV margin).
const DC_AC_BITS: u8 = 0x30;

/// Self-zero (POW:ZERO) and V self-calibration (POW:VOLT:CAL) jobs: results
/// dropped after each row write, results averaged per step.
const JOB_SKIP: u8 = 1;
const JOB_N: u8 = 3;
pub const JOB_ZERO: u8 = 1;
pub const JOB_VCAL: u8 = 2;
/// POW:ZERO refuses a step whose sqrt(RMS) (an AC signal) or |LPF - the
/// old zero| (a DC signal; the device's raw zeros are ~150..1100 codes)
/// exceeds these (codes).
const ZERO_MAX_RMS: f32 = 400.0;
const ZERO_MAX_LPF: i32 = 4000;
/// POW:VOLT:CAL level window (fraction of the V range): below, the ratio is
/// resolution-limited; above (AC), the 6 V tap clips (doc 3.11 T1: the
/// op amp's +0.7 V headroom).
const VCAL_MIN: f32 = 0.1;
const VCAL_MAX_AC: f32 = 0.7;
const VCAL_MAX_DC: f32 = 1.0;
/// V cal, AC reference: the stock AC V node is AC-coupled (tau ~0.3 s) and
/// starts from the power row's OP1O level: 2 s (> 6 tau) before its results
/// (the emulator: +0.46 % with the fallback's 0.6 s).
const VCAL_AC_SETTLE_MS: u64 = 2000;
/// Agreement of the two rounds (ratio, and the raw level: steadiness).
const VCAL_SPREAD: f32 = 0.003;
/// Job results (`Pw::job_res`).
pub const JR_NONE: u8 = 0;
pub const JR_RUN: u8 = 1;
pub const JR_OK: u8 = 2;
/// Zero: a signal / current is present.
pub const JR_SIGNAL: u8 = 3;
pub const JR_SMALL: u8 = 4;
pub const JR_BIG: u8 = 5;
pub const JR_UNSTABLE: u8 = 6;
/// V cal: the ratio is outside 0.9..1.1 (wrong range / wiring).
pub const JR_RANGE: u8 = 7;
pub const JR_ABORT: u8 = 8;
/// The fallback method (VA only) has no OP2 channel to calibrate / zero.
pub const JR_METHOD: u8 = 9;

/// The V channel's gain from the factory calibration (no self-cal stored):
/// 60 / 600 / 1000 V use the same divider taps as the stock AC V r1..r3
/// (OP1 x1 -> AD2 there, OP2 x1 -> AD3 here), so their AC V RMS gains
/// (cal 0x38..0x40) apply; 6 V uses the DC V r0 /10 tap (PA7, AD1 there):
/// its DC V gain (0x04) times the AD2-path / AD1-path ratio measured on the
/// 60 V tap by both calibrations (0x38 / 0x08). On the dumped unit:
/// 1.02065 / 1.01614 / 1.00453 / 0.99815 against the device's T1 needs
/// 1.0209 / 1.0170 / 1.0033 (doc 3.11): residuals -0.03 / -0.08 / +0.12 %.
/// A missing or implausible record gives 1.0.
pub fn v_gain_factory(vr: u8) -> f32 {
    let g = match vr.min(3) {
        0 => cal(0x04, 1.0) * cal(0x38, 1.0) / cal(0x08, 1.0),
        r => cal(0x34 + 4 * r as u32, 1.0),
    };
    if (0.9..=1.1).contains(&g) { g } else { 1.0 }
}

/// Comparator thresholds relative to AGND for R21 (registers.md mux tables;
/// REFO - AGND = 1.25 V, VDDA = 3.6 V for the VDSC taps, which only fill
/// the levels the AGNDP / AGNDN taps lack). (code, volts).
const CMP_RH: [(u8, f32); 12] = [
    (0xF, -0.0208),
    (0xE, 0.0),
    (0xD, 0.0208),
    (0xC, 0.0417),
    (0xB, 0.0625),
    (0xA, 0.1042),
    (0x9, 0.2083),
    (0x8, 0.4167),
    (0x1, 0.8),
    (0x0, 1.6),
    (0x5, -0.2),
    (0x6, -0.4),
];
const CMP_RL: [(u8, f32); 12] = [
    (0x8, -0.4167),
    (0x9, -0.2083),
    (0xA, -0.1042),
    (0xB, -0.0625),
    (0xC, -0.0417),
    (0xD, -0.0208),
    (0xE, 0.0),
    (0xF, 0.0208),
    (0x1, -0.8),
    (0x0, -1.6),
    (0x5, 0.2),
    (0x6, 0.4),
];
/// The default window (+-20.8 mV, the stock AC rows' R21).
pub const R21_DEFAULT: u8 = 0xDD;
/// R20 for the counter on the current (PB4: OP1O coupled, as the stock
/// AC A rows' 0xF2): the fallback source when the V tap has no window.
const R20_PB4: u8 = 0xF0;

/// R21 for a DC-coupled comparator input at `c` V (DC) with `a` V rms of AC
/// on it (doc 3.11: the T1 square was 0..96 mV on the 60 V tap: it never
/// crossed the default -20.8 mV, f = 0): the nearest high threshold above
/// c + 6 mV and low threshold below c - 6 mV, both inside the swing (every
/// usual waveform reaches at least its rms around its mean). None = no
/// window (pure DC, too small an AC part).
pub fn cmp_window(c: f32, a: f32) -> Option<u8> {
    // 6 mV: an un-zeroed unit's Y phantom (~5 mV at the tap) keeps a
    // bipolar signal on the default window.
    const M: f32 = 0.006;
    let mut rh: Option<(u8, f32)> = None;
    for &(k, v) in &CMP_RH {
        if v >= c + M && rh.is_none_or(|(_, b)| v < b) {
            rh = Some((k, v));
        }
    }
    let mut rl: Option<(u8, f32)> = None;
    for &(k, v) in &CMP_RL {
        if v <= c - M && rl.is_none_or(|(_, b)| v > b) {
            rl = Some((k, v));
        }
    }
    let ((h, vh), (l, vl)) = (rh?, rl?);
    if vh < c + 0.9 * a && vl > c - 0.9 * a { Some(h << 4 | l) } else { None }
}

/// R36 for a signed SPHACAL step count: > 0 = AD2 (the current) leads
/// (bit 7), < 0 = AD2 lags; |steps| <= 127.
pub fn r36_of(steps: i8) -> u8 {
    let n = (steps as i16).unsigned_abs().min(127) as u8;
    if steps > 0 { 0x80 | n } else { n }
}

/// (unit, op gain index) of current range k.
pub fn iu_ir(k: u8) -> (u8, u8) {
    let k = k.min(5);
    (2 - k / 2, k % 2)
}

/// The power row (R20..R35) of doc 3.1 / 3.10: the stock AC A row of the
/// current range with OP2 <- SENSE x1 -> AD3 = OP2O - AGND, the V range's
/// switch bytes, the RMS / LPF at LPFBW 111 and the comparator on SENSE.
/// `stat`: R34 (R34_XY / _XX / _YY). Every byte is the I row's, the V row's
/// or one of the listed overrides (host test `power_row_bytes`).
pub fn power_row(vr: u8, k: u8, stat: u8) -> [u8; hy::CFG_LEN] {
    let (iu, ir) = iu_ir(k);
    let mut b = if iu == 0 { CFG_F4[ir as usize] } else { CFG_F5_6[ir as usize] };
    // V rows: 6 V is the DC V r0 /10 tap (PA7; the AC V r0 trick, /100 with
    // OP1 x10, is not needed on OP2), the others the AC V r1..r3 taps.
    let v = if vr == 0 { &CFG_F1[0] } else { &CFG_F0[vr.min(3) as usize] };
    b[0] = R20_SENSE | 0x02; // R20: SCMPI SENSE, ENCMP, ENCTR
    b[5] &= !0x02; // R25: OPS2 = 0 (OP2 x1), OPS1 as the I range
    b[9] = 0xF8; // R29: ENRMS ENLPF LPFBW 111, no peak hold
    b[10..17].copy_from_slice(&v[10..17]); // R2A..R30: divider taps, SMODE, ACC
    b[18] = 0x80 | (b[18] & 0x0F); // R32: ENOP2, SOP2P = SENSE; OP1 as the I row
    b[20] = stat; // R34
    b[21] = 0x54; // R35: AD3 = OP2O - AGND, AD3IG 1.0
    b
}

/// Fallback rows: the stock AC V row of the V range, the stock AC A row of
/// the current range.
pub fn alt_row(v_phase: bool, vr: u8, k: u8) -> [u8; hy::CFG_LEN] {
    let (iu, ir) = iu_ir(k);
    if v_phase {
        CFG_F0[vr.min(3) as usize]
    } else if iu == 0 {
        CFG_F4[ir as usize]
    } else {
        CFG_F5_6[ir as usize]
    }
}

/// What a power reading carries (`Reading::power`), SI units; the burden
/// compensation already applied when `comp`.
#[derive(Clone, Copy, PartialEq)]
pub struct PowerVals {
    pub p: f32,
    pub s: f32,
    pub q: f32,
    /// P / S, signed with P (0 when S = 0); NaN in the fallback.
    pub pf: f32,
    pub v: f32,
    pub i: f32,
    pub vdc: f32,
    pub idc: f32,
    pub f_hz: f32,
    /// I x R_path: the meter's own drop inside the V reading.
    pub burden_v: f32,
    pub e_wh: f32,
    pub vah: f32,
    pub secs: u32,
    /// Mean P over the integration time (Wh / h; NaN before any).
    pub p_avg: f32,
    pub vr: u8,
    pub ik: u8,
    pub v_auto: bool,
    pub i_auto: bool,
    /// Fallback (VA only): no P / Q / PF.
    pub alt: bool,
    /// The DC (AD1) method in force: P = V I; PF / S / Q / f not shown.
    pub dc: bool,
    /// Coupling setting (COUP_*).
    pub coup: u8,
    pub comp: bool,
    /// Energy integration running / paused.
    pub run: bool,
    pub v_ol: bool,
    pub i_ol: bool,
    /// Continuous-current limit: 0 below, 1 counting down (`over_left` s),
    /// 2 expired.
    pub over: u8,
    pub over_left: u16,
    /// The SPHACAL in force (steps).
    pub sph: i8,
}

impl PowerVals {
    pub const ZERO: PowerVals = PowerVals {
        p: 0.0,
        s: 0.0,
        q: 0.0,
        pf: 0.0,
        v: 0.0,
        i: 0.0,
        vdc: 0.0,
        idc: 0.0,
        f_hz: 0.0,
        burden_v: 0.0,
        e_wh: 0.0,
        vah: 0.0,
        secs: 0,
        p_avg: f32::NAN,
        vr: 3,
        ik: 5,
        v_auto: true,
        i_auto: true,
        alt: false,
        dc: false,
        coup: COUP_AC,
        comp: true,
        run: true,
        v_ol: false,
        i_ol: false,
        over: 0,
        over_left: 0,
        sph: 0,
    };
}

/// One RMS / LPF result as read (DBG:POW:RAW?, device test T3).
#[derive(Clone, Copy)]
pub struct Raw {
    /// R34 in force (fallback: 1 = V row, 2 = I row).
    pub stat: u8,
    pub disc: bool,
    pub rms: i64,
    pub lpf: i32,
    /// ms since the previous result.
    pub dt: u16,
}

/// Energy accumulator: whole mJ in an i64 plus the f32 remainder (an f32
/// alone stops adding mW s to kWh after a few hours).
#[derive(Clone, Copy)]
struct Acc {
    mj: i64,
    frac: f32,
}

impl Acc {
    const ZERO: Acc = Acc { mj: 0, frac: 0.0 };
    fn add(&mut self, joules: f32) {
        let x = self.frac + joules * 1000.0;
        let w = x as i64;
        self.mj += w;
        self.frac = x - w as f32;
    }
    fn wh(&self) -> f32 {
        ((self.mj as f64 + self.frac as f64) / 3.6e6) as f32
    }
}

pub struct Pw {
    pub on: bool,
    /// Method: false = simultaneous (P, PF), true = fallback (VA only).
    pub alt: bool,
    /// Coupling setting (COUP_*) and the DC method in force (`coup` DC, or
    /// AUTO after a DC-like set); CTSTA bits seen in the DC V phase.
    pub coup: u8,
    pub dc: bool,
    dc_cmp: u8,
    pub vr: u8,
    pub ik: u8,
    pub v_auto: bool,
    pub i_auto: bool,
    /// Index in CYCLE (fallback: 0 V phase, 1 I phase).
    phase: u8,
    /// Results still to drop in this phase / taken in it (fallback).
    skip: u8,
    taken: u8,
    /// Results discarded after an R34 switch (DBG:POW:DISC, persisted).
    pub disc: u8,
    /// R34 in force.
    stat: u8,
    /// Parts measured since the last (re)start: 1 P, 2 V, 4 I.
    seen: u8,
    p_ac: f32,
    v_ac: f32,
    v_dc: f32,
    i_ac: f32,
    i_dc: f32,
    /// P of the latest accepted result (unfiltered, for the energy).
    p_last: f32,
    /// Totals of the latest reading: P, S for the next energy interval.
    p_tot: f32,
    s_last: f32,
    /// Energy integration.
    pub run: bool,
    e: Acc,
    va: Acc,
    t_int: u64,
    t_last: Option<u64>,
    /// SPHACAL (steps, signed), burden R_path per unit (ohm), compensation,
    /// continuous-current limit (A, s; t_lim 0 = none).
    pub sph: i8,
    pub r_path: [f32; 3],
    pub comp: bool,
    pub i_cont: f32,
    pub t_lim: u16,
    over_t0: Option<u64>,
    over_beeped: bool,
    /// Phase calibration (POW:PHAS:CAL): 0 off, 1 / 2 averaging at s0 /
    /// s0 + delta; sums of P_ac / (V_ac I_ac).
    cal: u8,
    cal_n: u8,
    cal_acc: f32,
    cal_s0: i8,
    cal_pa: f32,
    pub cal_result: Option<bool>,
    /// The calibration moved SPHACAL: rewrite the row (`pw_service`).
    cal_req: bool,
    /// DBG: R34 frozen (0 = cycle), row byte overrides, raw ring.
    pub freeze: u8,
    pub poke: [(u8, u8); 4],
    pub poke_n: u8,
    ring: [Raw; RING_N],
    ring_k: u32,
    t_res: u64,
    /// The row last written and its R36 (DBG:POW:ROW?).
    pub row: [u8; hy::CFG_LEN],
    pub r36: u8,
    /// Range changes / restarts since entry (energy gaps).
    pub restarts: u32,
    /// Parts kept over the next `pw_apply` (2 V, 4 I: the other channel's
    /// range changed, its scale did not).
    keep: u8,
    /// V channel gain per range (raw x gain = V), 0 = `v_gain_factory`;
    /// set by POW:VOLT:CAL, persisted.
    pub vgain: [f32; 4],
    /// LPF zeros (codes; POW:ZERO, persisted when `zero_ok`): X per current
    /// range, Y once (after the divider: the same on every V range).
    pub zx: [i16; 6],
    pub zy: i16,
    pub zero_ok: bool,
    /// The wiring dialog was confirmed once (the W button enters directly
    /// afterwards; persisted).
    pub wired: bool,
    /// Running job (JOB_ZERO / JOB_VCAL, 0 none), its step, results taken,
    /// sums; the V cal's reference kind (DC V vs AC V) and its two rounds;
    /// the ranges to restore; the outcome (JR_*) and its V range.
    pub job: u8,
    pub job_step: u8,
    job_n: u8,
    job_a: f32,
    job_b: f32,
    job_dc: bool,
    job_raw: [f32; 2],
    job_ref: [f32; 2],
    job_zx: [i16; 6],
    job_save: (u8, u8, bool, bool),
    pub job_res: u8,
    pub job_vr: u8,
    /// The kind of the last job started (its `job_res`).
    pub job_last: u8,
    /// Counter source / comparator window (R20 without ENCTR, R21) and
    /// whether the frequency is meaningful (`pw_cmp_select`); the V range
    /// the window was chosen for.
    pub r20: u8,
    pub r21: u8,
    f_ok: bool,
    cmp_vr: u8,
}

impl Pw {
    pub const fn new() -> Self {
        Pw {
            on: false,
            alt: false,
            coup: COUP_AC,
            dc: false,
            dc_cmp: 0,
            vr: 3,
            ik: 5,
            v_auto: true,
            i_auto: true,
            phase: 0,
            skip: 0,
            taken: 0,
            disc: DISC_DEFAULT,
            stat: R34_XY,
            seen: 0,
            p_ac: 0.0,
            v_ac: 0.0,
            v_dc: 0.0,
            i_ac: 0.0,
            i_dc: 0.0,
            p_last: 0.0,
            p_tot: 0.0,
            s_last: 0.0,
            run: true,
            e: Acc::ZERO,
            va: Acc::ZERO,
            t_int: 0,
            t_last: None,
            sph: 0,
            r_path: SHUNT_OHM,
            comp: true,
            i_cont: I_CONT_DEFAULT,
            t_lim: T_LIM_DEFAULT,
            over_t0: None,
            over_beeped: false,
            cal: 0,
            cal_n: 0,
            cal_acc: 0.0,
            cal_s0: 0,
            cal_pa: 0.0,
            cal_result: None,
            cal_req: false,
            freeze: 0,
            poke: [(0, 0); 4],
            poke_n: 0,
            ring: [Raw { stat: 0, disc: false, rms: 0, lpf: 0, dt: 0 }; RING_N],
            ring_k: 0,
            t_res: 0,
            row: [0; hy::CFG_LEN],
            r36: 0,
            restarts: 0,
            keep: 0,
            vgain: [0.0; 4],
            zx: [0; 6],
            zy: 0,
            zero_ok: false,
            wired: false,
            job: 0,
            job_step: 0,
            job_n: 0,
            job_a: 0.0,
            job_b: 0.0,
            job_dc: false,
            job_raw: [0.0; 2],
            job_ref: [0.0; 2],
            job_zx: [0; 6],
            job_save: (3, 5, true, true),
            job_res: JR_NONE,
            job_vr: 0,
            job_last: 0,
            r20: R20_SENSE,
            r21: R21_DEFAULT,
            f_ok: true,
            cmp_vr: 255,
        }
    }

    /// The V gain in force for range `vr`.
    pub fn v_gain(&self, vr: u8) -> f32 {
        let g = self.vgain[vr.min(3) as usize];
        if g > 0.0 { g } else { v_gain_factory(vr) }
    }

    /// Zero Wh / VAh / time (POW:ENER:RES, the energy box).
    pub fn energy_reset(&mut self) {
        self.e = Acc::ZERO;
        self.va = Acc::ZERO;
        self.t_int = 0;
        self.t_last = None;
    }

    /// Energy integration running with a current above the floor: the app
    /// holds the idle power-off (and dimming) off.
    pub fn integrating(&self) -> bool {
        self.on && self.run && self.seen & 4 != 0 && hypot(self.i_ac, self.i_dc) > I_FLOOR
    }

    /// The raw ring, oldest first: "n;stat,d,rms,lpf,dt;..." (DBG:POW:RAW?).
    pub fn ring_text(&self, buf: &mut [u8]) -> usize {
        let mut at = 0;
        let mut put = |b: &[u8], at: &mut usize| {
            let k = b.len().min(buf.len() - *at);
            buf[*at..*at + k].copy_from_slice(&b[..k]);
            *at += k;
        };
        let mut t = [0u8; 24];
        put(i64_text(&mut t, self.ring_k as i64), &mut at);
        let n = (self.ring_k as usize).min(RING_N);
        for j in 0..n {
            let e = &self.ring[(self.ring_k as usize + RING_N - n + j) % RING_N];
            put(b";", &mut at);
            let st: &[u8] = match e.stat {
                R34_XY => b"XY",
                R34_XX => b"XX",
                R34_YY => b"YY",
                1 => b"V",
                2 => b"I",
                3 => b"DV",
                4 => b"DI",
                _ => b"?",
            };
            put(st, &mut at);
            put(if e.disc { b",1," } else { b",0," }, &mut at);
            put(i64_text(&mut t, e.rms), &mut at);
            put(b",", &mut at);
            put(i64_text(&mut t, e.lpf as i64), &mut at);
            put(b",", &mut at);
            put(i64_text(&mut t, e.dt as i64), &mut at);
        }
        at
    }
}

/// The power meter's state lives in SRAM (.sram_bss), not in the App on the
/// TCM stack: `Dmm::new` writes it once (`pw_static`).
#[cfg_attr(all(target_os = "none", feature = "native"), link_section = ".sram_bss")]
static mut PW: core::mem::MaybeUninit<Pw> = core::mem::MaybeUninit::uninit();

/// Initialise and hand out the one `Pw` (call once: `Dmm::new`).
pub fn pw_static() -> &'static mut Pw {
    unsafe { (*core::ptr::addr_of_mut!(PW)).write(Pw::new()) }
}

/// Decimal text of `v` (DBG replies).
pub fn i64_text(t: &mut [u8; 24], v: i64) -> &[u8] {
    let mut d = [0u8; 24];
    let mut k = 24;
    let mut u = v.unsigned_abs();
    loop {
        k -= 1;
        d[k] = b'0' + (u % 10) as u8;
        u /= 10;
        if u == 0 {
            break;
        }
    }
    if v < 0 {
        k -= 1;
        d[k] = b'-';
    }
    let n = 24 - k;
    t[..n].copy_from_slice(&d[k..]);
    &t[..n]
}

/// The (P, V, I) totals and load values of doc 3.3 from the parts, with the
/// burden compensation `r` (ohm, 0 = off). Returns (P, S, Q, PF, V, I).
pub fn totals(p_ac: f32, v_ac: f32, v_dc: f32, i_ac: f32, i_dc: f32, r: f32) -> (f32, f32, f32, f32, f32, f32) {
    let p_m = p_ac + v_dc * i_dc;
    let v_m = hypot(v_ac, v_dc);
    let i = hypot(i_ac, i_dc);
    let p = p_m - i * i * r;
    let v2 = v_m * v_m - 2.0 * r * p_m + r * r * i * i;
    let v = sqrt(v2.max(0.0));
    let s = v * i;
    // |P| can exceed S by the noise of the separate statistics: PF clamped.
    let pf = if s > 0.0 { (p / s).clamp(-1.0, 1.0) } else { 0.0 };
    let q = sqrt((s * s - p * p).max(0.0));
    (p, s, q, pf, v, i)
}

/// EMA with a relative jump: |x - y| > 5 % of max(|x|, |y|, `floor`) jumps.
fn ema_rel(a: f32, floor: f32, x: f32, y: &mut f32) {
    let m = x.abs().max(y.abs()).max(floor);
    ema(0.05 * m, a, x, y);
}

impl Dmm {
    /// Enter the power meter (SCPI CONF:POW, the wiring dialog's OK): both
    /// channels auto, V from 600 V (mains in range at once; the stock AC V
    /// starts at 6 V), I from 10 A (the 10 mohm shunt: the least burden).
    pub fn pw_enter(&mut self, clk: &mut Clock) {
        self.acdc = false;
        self.manual = false;
        let pw = &mut *self.pw;
        pw.on = true;
        pw.v_auto = true;
        pw.i_auto = true;
        pw.vr = 2;
        pw.ik = 5;
        pw.run = true;
        pw.energy_reset();
        pw.over_t0 = None;
        pw.cal = 0;
        pw.restarts = 0;
        pw.job = 0;
        pw.cal_req = false;
        pw.keep = 0;
        pw.cmp_vr = 255;
        // DC: the AD1 method at once; AUTO starts with B (a full set tells).
        pw.dc = pw.coup == COUP_DC;
        self.mode = 255;
        self.reset_values();
        self.entry = clk.now_ms();
        self.pw_apply(clk);
    }

    /// Leave the power meter (the next `set` does it: any function change).
    pub(super) fn pw_leave(&mut self) {
        self.pw_job_end(JR_ABORT);
        self.pw.on = false;
        self.pw.freeze = 0;
        self.pw.poke_n = 0;
    }

    /// (Re)write the row for the current ranges / SPHACAL / method, restart
    /// the phase machine, settle. The parts measured so far are dropped (the
    /// scale changed); the energy keeps integrating the last P over the gap.
    pub fn pw_apply(&mut self, clk: &mut Clock) {
        let pw = &mut *self.pw;
        // After a range change of one channel its own statistic comes first
        // (B: XX / YY, DC: the I / V row), so an autorange hunt costs one
        // phase per step (~0.9 s from the settle), not a whole P V P I set.
        pw.phase = match (pw.keep, pw.dc, pw.alt) {
            (2, false, false) => 3,
            (4, false, false) => 1,
            (2, true, _) => 1,
            _ => 0,
        };
        // A range change of one channel keeps the other's parts (its scale
        // did not change): the page keeps V while I autoranges (doc 3.11).
        pw.seen &= pw.keep & !1;
        pw.keep = 0;
        pw.taken = 0;
        if pw.cmp_vr != pw.vr {
            // The window was for another tap: back to the default until the
            // next V result (`pw_cmp_select`).
            pw.cmp_vr = pw.vr;
            pw.r20 = R20_SENSE;
            pw.r21 = R21_DEFAULT;
            pw.f_ok = true;
        }
        pw.skip = if pw.dc { DC_SKIP } else if pw.alt { ALT_V_SKIP } else { 0 };
        pw.dc_cmp = 0;
        let settle = if pw.dc { DC_SETTLE_MS } else { SETTLE_MS };
        pw.restarts = pw.restarts.wrapping_add(1);
        self.write_config(clk);
        self.deadline = clk.now_ms() + settle;
        self.freq.next = 0;
    }

    /// `write_config` in the power meter: quiesce, PA relay state 0, the
    /// shunt of the current range, the row (method B: with SPHACAL before
    /// the enables), no sentinel (the comparator counts the V frequency).
    pub(super) fn pw_write_config(&mut self, clk: &mut Clock) {
        let (iu, ir) = iu_ir(self.pw.ik);
        self.mode = 4 + iu;
        self.range = ir as i8;
        let pw = &mut *self.pw;
        pw.stat = match pw.job {
            JOB_ZERO if pw.job_step < 6 => R34_XX,
            JOB_ZERO | JOB_VCAL => R34_YY,
            _ if pw.freeze != 0 => pw.freeze,
            _ => CYCLE[pw.phase as usize & 3],
        };
        // V cal, odd steps: the stock reference row of the range (DC V on
        // AD1 or AC V), with the power meter's shunt kept (below).
        let vref = pw.job == JOB_VCAL && pw.job_step & 1 == 1;
        let dcm = pw.dc && pw.job == 0;
        let stock = pw.alt || vref || dcm;
        let (iu_, ir_) = iu_ir(pw.ik);
        let mut row = if vref {
            if pw.job_dc { CFG_F1[pw.vr.min(3) as usize] } else { CFG_F0[pw.vr.min(3) as usize] }
        } else if dcm {
            // The DC method: the stock DC V row of the V range (phase 0) /
            // the DC A row of the current range (phase 1); the comparator
            // of the V row on SENSE (the tap) with a window around its DC
            // for the AUTO coupling's AC check (`pw_poll_dc`).
            if pw.phase & 1 == 0 {
                let mut r = CFG_F1[pw.vr.min(3) as usize];
                r[0] = R20_SENSE;
                r[1] = pw.r21;
                r
            } else if iu_ == 0 {
                CFG_F7[ir_ as usize]
            } else {
                CFG_F8_9[ir_ as usize]
            }
        } else if pw.alt {
            alt_row(pw.phase & 1 == 0, pw.vr, pw.ik)
        } else {
            power_row(pw.vr, pw.ik, pw.stat)
        };
        if !stock {
            // The counter's source and window (`pw_cmp_select`).
            row[0] = pw.r20 | 0x02;
            row[1] = pw.r21;
            for &(i, v) in &pw.poke[..pw.poke_n as usize] {
                if (i as usize) < row.len() {
                    row[i as usize] = v;
                }
            }
            pw.stat = row[20];
        }
        pw.row = row;
        pw.r36 = if stock { 0 } else { r36_of(pw.sph) };
        self.avg.reset();
        self.t_rd = None;
        self.t_conv = None;
        hy::quiesce();
        if self.pa_now != Some(0) {
            relay_pa(clk, 0);
            self.pa_now = Some(0);
        }
        relay_pe(iu);
        pc7(false);
        if stock {
            hy::configure(&row);
        } else {
            hy::configure_ph(&row, self.pw.r36);
        }
        self.r22 = row[2];
        self.r29 = row[9];
        self.sent_mode = 255;
        self.sent.configured(Kind::Off, clk.now_ms(), true);
        self.diag_on = false;
    }

    /// Restart after the DMM rail came back (`start`).
    pub(super) fn pw_restart(&mut self, clk: &mut Clock) {
        self.pa_now = None;
        self.pw_apply(clk);
    }

    /// Ranges: `v` / `i` None = keep, Some(None) = AUTO, Some(Some(r)) =
    /// that range (V 0..3, I index 0..5).
    pub fn pw_set_ranges(&mut self, clk: &mut Clock, v: Option<Option<u8>>, i: Option<Option<u8>>) {
        if self.pw.job != 0 {
            self.pw_job_end(JR_ABORT);
        }
        let pw = &mut *self.pw;
        let (vr, ik) = (pw.vr, pw.ik);
        match v {
            Some(None) => pw.v_auto = true,
            Some(Some(r)) => {
                pw.v_auto = false;
                pw.vr = r.min(3);
            }
            None => {}
        }
        match i {
            Some(None) => pw.i_auto = true,
            Some(Some(r)) => {
                pw.i_auto = false;
                pw.ik = r.min(5);
            }
            None => {}
        }
        self.manual = !(pw.v_auto && pw.i_auto);
        if pw.on && (pw.vr, pw.ik) != (vr, ik) {
            pw.keep = if pw.vr == vr { 2 } else if pw.ik == ik { 4 } else { 0 };
            self.pw_apply(clk);
        }
    }

    /// SPHACAL (POW:PHAS): rewritten with the row (datasheet order).
    pub fn pw_set_phase(&mut self, clk: &mut Clock, steps: i8) {
        self.pw.sph = steps.max(-127);
        if self.pw.on {
            self.pw_apply(clk);
        }
    }

    /// Method B (false) or the fallback (true).
    pub fn pw_set_method(&mut self, clk: &mut Clock, alt: bool) {
        if alt != self.pw.alt {
            self.pw.alt = alt;
            self.pw.freeze = 0;
            if self.pw.on {
                self.pw_apply(clk);
            }
        }
    }

    /// Phase self-calibration on a reactive load (PF ~ 0, any sign): P / S
    /// averaged at the current SPHACAL and 48 steps further, then the
    /// Newton step that nulls it. Result in `pw.cal_result`.
    pub fn pw_cal_start(&mut self) -> bool {
        let pw = &mut *self.pw;
        if !pw.on || pw.alt {
            return false;
        }
        pw.cal = 1;
        pw.cal_n = 0;
        pw.cal_acc = 0.0;
        pw.cal_s0 = pw.sph;
        pw.cal_result = None;
        true
    }

    /// Coupling (POW:COUP, the settings page): AC, DC or AUTO; the method
    /// follows at once (AUTO: from B).
    pub fn pw_set_coupling(&mut self, clk: &mut Clock, coup: u8) {
        if self.pw.job != 0 {
            self.pw_job_end(JR_ABORT);
        }
        let pw = &mut *self.pw;
        pw.coup = coup.min(COUP_AUTO);
        let dc = pw.coup == COUP_DC;
        if pw.on && (dc != pw.dc || pw.job_res == JR_ABORT) {
            pw.dc = dc;
            pw.keep = 0;
            self.pw_apply(clk);
        }
    }

    /// DBG:POW:STAT: freeze R34 (0 = cycle P / V / P / I again).
    pub fn pw_freeze(&mut self, clk: &mut Clock, stat: u8) {
        self.pw.freeze = stat;
        if self.pw.on {
            self.pw_apply(clk);
        }
    }

    /// DBG:POW:POKE: override row byte `i` (0..21 = R20..R35); i = 255
    /// clears all overrides.
    pub fn pw_poke(&mut self, clk: &mut Clock, i: u8, v: u8) -> bool {
        let pw = &mut *self.pw;
        if i == 255 {
            pw.poke_n = 0;
        } else if (i as usize) < hy::CFG_LEN && (pw.poke_n as usize) < pw.poke.len() {
            pw.poke[pw.poke_n as usize] = (i, v);
            pw.poke_n += 1;
        } else {
            return false;
        }
        if pw.on {
            self.pw_apply(clk);
        }
        true
    }

    /// Phase-machine cadence: (discards per switch, P period ms, full set
    /// ms) at the measured 106.7 ms per result (POW:CYC?).
    pub fn pw_cycle(&self) -> (u8, u32, u32) {
        let d = self.pw.disc as u32;
        if self.pw.alt {
            let t = (ALT_V_SETTLE_MS + SETTLE_MS) as u32 + (2 * ALT_N + ALT_V_SKIP) as u32 * 107;
            (0, 0, t)
        } else {
            (self.pw.disc, 2 * (d + 1) * 107, 4 * (d + 1) * 107)
        }
    }

    /// Values for the UI / SCPI (None before V, I and P were measured).
    pub fn pw_vals(&self) -> Option<PowerVals> {
        let pw = &*self.pw;
        let need = if pw.alt && !pw.dc { 6 } else { 7 };
        if !pw.on || pw.seen & need != need {
            return None;
        }
        let (iu, _) = iu_ir(pw.ik);
        let r = if pw.comp { pw.r_path[iu as usize] } else { 0.0 };
        let (p, s, q, pf, v, i) = if pw.alt && !pw.dc {
            let (v, i) = (pw.v_ac, hypot(pw.i_ac, pw.i_dc));
            (f32::NAN, v * i, f32::NAN, f32::NAN, v, i)
        } else {
            totals(pw.p_ac, pw.v_ac, pw.v_dc, pw.i_ac, pw.i_dc, r)
        };
        let ov = self.over();
        let (over, over_left) = match pw.over_t0 {
            Some(t0) if pw.t_lim > 0 => {
                let el = (self.t_poll.saturating_sub(t0) / 1000) as u16;
                if el >= pw.t_lim { (2, 0) } else { (1, pw.t_lim - el) }
            }
            Some(_) => (0, 0),
            None => (0, 0),
        };
        Some(PowerVals {
            p,
            s,
            q,
            pf,
            v,
            i,
            vdc: if pw.alt && !pw.dc { f32::NAN } else { pw.v_dc },
            idc: pw.i_dc,
            f_hz: if pw.dc { 0.0 } else if pw.alt || pw.f_ok { self.freq.f_khz * 1000.0 } else { 0.0 },
            burden_v: i * pw.r_path[iu as usize],
            e_wh: pw.e.wh(),
            vah: pw.va.wh(),
            secs: (pw.t_int / 1000) as u32,
            p_avg: if pw.t_int > 0 && (!pw.alt || pw.dc) { pw.e.wh() * 3.6e6 / pw.t_int as f32 } else { f32::NAN },
            vr: pw.vr,
            ik: pw.ik,
            v_auto: pw.v_auto,
            i_auto: pw.i_auto,
            alt: pw.alt && !pw.dc,
            dc: pw.dc,
            coup: pw.coup,
            comp: pw.comp,
            run: pw.run,
            v_ol: hypot(pw.v_ac, pw.v_dc) > ov * V_LIM[pw.vr as usize],
            i_ol: hypot(pw.i_ac, pw.i_dc) > ov * I_LIM[pw.ik as usize],
            over,
            over_left,
            sph: pw.sph,
        })
    }

    /// `pw_vals`, or before the first full set the settings / ranges with
    /// zero values (the settings page, SCPI queries).
    pub fn pw_state(&self) -> PowerVals {
        self.pw_vals().unwrap_or_else(|| {
            let pw = &*self.pw;
            PowerVals {
                vr: pw.vr,
                ik: pw.ik,
                v_auto: pw.v_auto,
                i_auto: pw.i_auto,
                alt: pw.alt && !pw.dc,
                dc: pw.dc,
                coup: pw.coup,
                comp: pw.comp,
                run: pw.run,
                e_wh: pw.e.wh(),
                vah: pw.va.wh(),
                secs: (pw.t_int / 1000) as u32,
                sph: pw.sph,
                ..PowerVals::ZERO
            }
        })
    }

    /// The reading of the power meter (`reading_vals`).
    fn pw_reading(&self, pv: &PowerVals) -> Reading {
        let (main, v, i, si) = reading_vals(pv);
        let mut rd = self.reading(main, si);
        rd.aux1 = Some(v);
        rd.aux2 = Some(i);
        rd.power = Some(*pv);
        rd
    }

    /// Poll in the power meter (from `poll`, after the settle deadline).
    pub(super) fn poll_power(&mut self, clk: &mut Clock, now: u64) -> Option<Reading> {
        if self.pw.job != 0 {
            return self.pw_job_poll(clk);
        }
        if self.pw.dc {
            return self.pw_poll_dc(clk, now);
        }
        // Frequency: the comparator on SENSE with a window around the V
        // tap's DC, or on PB4 (the current) when the V tap has none
        // (`pw_cmp_select`; method B); the fallback's AC V row has it on PB4
        // (the coupled V node). The sentinel is off in the power meter
        // (`pw_write_config`): R20 / R21 are the counter's alone.
        if !self.pw.alt {
            if self.pw.f_ok {
                self.freq_secondary(now, self.pw.r20);
            }
        } else if self.pw.phase & 1 == 0 {
            self.freq_secondary(now, 0xF0);
        }
        let need = hy::INTF_RMS | hy::INTF_LPF;
        if hy::read1(hy::R_INTF) & need != need {
            return None;
        }
        let mut b = [0u8; 14];
        hy::read(hy::R_AD1, &mut b);
        let lpf = hy::lpf19(&b[6..9]);
        let rms = hy::rms_signed(&b[9..14]);
        let alt = self.pw.alt;
        let stat = if alt { 1 + (self.pw.phase & 1) } else { self.pw.stat };
        let disc = self.pw.skip > 0;
        {
            let pw = &mut *self.pw;
            let dt = now.saturating_sub(pw.t_res).min(u16::MAX as u64) as u16;
            pw.t_res = now;
            pw.ring[pw.ring_k as usize % RING_N] = Raw { stat, disc, rms, lpf, dt };
            pw.ring_k = pw.ring_k.wrapping_add(1);
            if disc {
                pw.skip -= 1;
                return None;
            }
        }
        let accepted = if alt { self.pw_take_alt(rms, lpf) } else { self.pw_take(stat, rms, lpf) };
        if !accepted {
            return None;
        }
        // Next phase (method B: a new statistic, R34 only).
        let mut ranged = false;
        if alt {
            self.pw.taken += 1;
            if self.pw.taken >= ALT_N {
                ranged = self.pw_autorange(clk, self.pw.phase & 1 == 0);
                if !ranged {
                    self.pw.taken = 0;
                    self.pw.phase ^= 1;
                    let v = self.pw.phase & 1 == 0;
                    let settle = if v { ALT_V_SETTLE_MS } else { SETTLE_MS };
                    self.pw.skip = if v { ALT_V_SKIP } else { 0 };
                    self.write_config(clk);
                    self.deadline = clk.now_ms() + settle;
                }
            }
        } else {
            let st = self.pw.stat;
            if st != R34_XY {
                ranged = self.pw_autorange(clk, st == R34_YY);
                if !ranged {
                    self.pw_cmp_select(now);
                }
            }
            if !ranged && self.pw.freeze == 0 {
                let pw = &mut *self.pw;
                pw.phase = (pw.phase + 1) % CYCLE.len() as u8;
                let next = CYCLE[pw.phase as usize];
                if next != pw.stat {
                    pw.stat = next;
                    pw.skip = pw.disc;
                    hy::write(0x34, &[next]);
                }
            }
        }
        if ranged {
            return None;
        }
        let pv = self.pw_vals()?;
        self.pw_account(now, &pv);
        let pv = self.pw_vals()?;
        // AUTO coupling: a DC-like set after the I phase -> the DC method.
        if self.pw.coup == COUP_AUTO && !self.pw.alt && self.pw.stat == CYCLE[0] && self.pw.phase == 0 && self.pw_dc_like() {
            self.pw.dc = true;
            self.pw.keep = 6;
            self.pw_apply(clk);
        }
        Some(self.pw_reading(&pv))
    }

    /// Method B's parts are DC-like: each channel's AC part below 1 % of its
    /// DC part plus the noise floor (~100 codes of its converter).
    fn pw_dc_like(&self) -> bool {
        let pw = &*self.pw;
        let (iu, ir) = iu_ir(pw.ik);
        let fv = 100.0 * CODE * KV[pw.vr as usize];
        let fi = 100.0 * CODE / (SHUNT_OHM[iu as usize] * MV_DIV[ir as usize]);
        pw.seen & 6 == 6 && pw.v_ac < 0.01 * pw.v_dc.abs() + fv && pw.i_ac < 0.01 * pw.i_dc.abs() + fi
    }

    /// The DC method (`Pw::dc`): one AD1 result of the stock DC V row (phase
    /// 0) or DC A row (phase 1), scaled as `poll_dcv` / `poll_dca` with the
    /// factory DC calibration; DC_N per phase, then the autorange of that
    /// channel and the other row. P = V I (`totals` with the AC parts 0:
    /// the burden compensation as in B). A reading after each phase.
    fn pw_poll_dc(&mut self, clk: &mut Clock, now: u64) -> Option<Reading> {
        let v_phase = self.pw.phase & 1 == 0;
        if v_phase && self.pw.coup == COUP_AUTO {
            // AUTO: the comparator's high / low latches over the V phase.
            self.pw.dc_cmp |= hy::read1(0x14) & DC_AC_BITS;
        }
        let ad1 = self.ad1()?;
        let pw = &mut *self.pw;
        if pw.skip > 0 {
            pw.skip -= 1;
            return None;
        }
        {
            let dt = now.saturating_sub(pw.t_res).min(u16::MAX as u64) as u16;
            pw.t_res = now;
            pw.ring[pw.ring_k as usize % RING_N] = Raw { stat: if v_phase { 3 } else { 4 }, disc: false, rms: 0, lpf: ad1, dt };
            pw.ring_k = pw.ring_k.wrapping_add(1);
        }
        if v_phase {
            let r = pw.vr.min(3) as usize;
            let mut v = ad1 as f32 * K_ACV[r] * (1.25 / 0.9) / AD1_DIV * cal(0x04 + 4 * r as u32, 1.0) - cal(0x14 + 4 * r as u32, 0.0);
            if v < 0.0 {
                v *= DCV_NEGK[r];
            }
            if pw.seen & 2 == 0 {
                pw.v_dc = v;
            } else {
                ema(ACV_WIN[r], 0.5, v, &mut pw.v_dc);
            }
            pw.v_ac = 0.0;
            pw.seen |= 2;
        } else {
            let (iu, ir) = iu_ir(pw.ik);
            let i = (iu * 2 + ir) as u32;
            let us = [1.0, 1e-3, 1e-6][iu as usize];
            let a = (ad1 as f32 * 1.25 / (SHUNT[iu as usize] * MV_DIV[ir as usize] * 0.9) / AD1_DIV * cal(0x94 + 4 * i, 1.0) - cal(0xAC + 4 * i, 0.0)) * us;
            if pw.seen & 4 == 0 {
                pw.i_dc = a;
            } else {
                ema(ACA_WIN[i as usize] * us, 0.5, a, &mut pw.i_dc);
            }
            pw.i_ac = 0.0;
            pw.seen |= 4;
        }
        pw.p_ac = 0.0;
        if pw.seen & 6 == 6 {
            pw.seen |= 1;
        }
        pw.taken += 1;
        if pw.taken < DC_N {
            return None;
        }
        pw.taken = 0;
        // AUTO: an AC swing on the V tap in this V phase -> method B.
        if v_phase && pw.coup == COUP_AUTO && pw.dc_cmp == DC_AC_BITS {
            pw.dc = false;
            pw.keep = 0;
            self.pw_apply(clk);
            return None;
        }
        if v_phase {
            // The AC check's window around the tap's DC (no swing needed).
            let c = self.pw.v_dc / KV[self.pw.vr as usize];
            self.pw.r21 = cmp_window(c, 1.0).unwrap_or(R21_DEFAULT);
        }
        if self.pw_autorange(clk, v_phase) {
            return None;
        }
        let pw = &mut *self.pw;
        pw.phase ^= 1;
        pw.skip = DC_SKIP;
        pw.dc_cmp = 0;
        self.write_config(clk);
        self.deadline = clk.now_ms() + DC_SETTLE_MS;
        let pv = self.pw_vals()?;
        self.pw_account(now, &pv);
        let pv = self.pw_vals()?;
        Some(self.pw_reading(&pv))
    }

    /// Method B: one accepted result of statistic `st`. False = garbage.
    fn pw_take(&mut self, st: u8, rms: i64, lpf: i32) -> bool {
        let pw = &mut *self.pw;
        let (iu, ir) = iu_ir(pw.ik);
        let ci = (iu * 2 + ir) as u32;
        let s_i = CODE / (SHUNT_OHM[iu as usize] * MV_DIV[ir as usize]);
        let s_v = CODE * KV[pw.vr as usize] * pw.v_gain(pw.vr);
        // The DC part runs through the same shunt / OP1 / AD2 as the AC part:
        // with the LPF gain at its default 1.0 (never calibrated: so on the
        // dumped unit, while the AC A RMS gains are 0.954..1.036) the RMS
        // gain applies to it too (INFERRED; device check T4: Idc vs DC A).
        let g_i = cal(0xC4 + 4 * ci, 1.0);
        let g_il = match cal(0xF4 + 4 * ci, 1.0) {
            g if g == 1.0 => g_i,
            g => g,
        };
        // LPF zeros (POW:ZERO): the X one of this current range; the Y one
        // in the V phase below. The RMS statistics are after the HPF.
        let (zx, zy) = if pw.zero_ok { (pw.zx[pw.ik.min(5) as usize] as i32, pw.zy as i32) } else { (0, 0) };
        let i_dc = (lpf - zx) as f32 * s_i * g_il;
        let first = |seen: u8, bit: u8| seen & bit == 0;
        let ia = (ACA_WIN[ci as usize] * [1.0, 1e-3, 1e-6][iu as usize], 0.15);
        match st {
            R34_XY => {
                if rms.unsigned_abs() > 1u64 << 36 {
                    return false;
                }
                let p = rms as f32 * s_i * g_i * s_v;
                pw.p_last = p;
                if first(pw.seen, 1) {
                    pw.p_ac = p;
                } else {
                    ema_rel(0.25, 0.01 * V_LIM[pw.vr as usize] * I_LIM[pw.ik as usize], p, &mut pw.p_ac);
                }
                if first(pw.seen, 4) {
                    pw.i_dc = i_dc;
                } else {
                    ema(ia.0, 0.35, i_dc, &mut pw.i_dc);
                }
                pw.seen |= 1;
                if pw.cal != 0 && pw.seen & 6 == 6 {
                    let s = pw.v_ac * pw.i_ac;
                    if s > 0.0 {
                        pw.cal_acc += p / s;
                        pw.cal_n += 1;
                    }
                }
            }
            R34_XX | R34_YY => {
                let sq = sqrt(rms.max(0) as f32);
                if sq > 262_144.0 {
                    return false;
                }
                if st == R34_XX {
                    let a = sq * s_i * g_i;
                    if first(pw.seen, 4) {
                        pw.i_ac = a;
                        pw.i_dc = i_dc;
                    } else {
                        ema(ia.0, ia.1, a, &mut pw.i_ac);
                        ema(ia.0, 0.35, i_dc, &mut pw.i_dc);
                    }
                    pw.seen |= 4;
                } else {
                    let (a, d) = (sq * s_v, (lpf - zy) as f32 * s_v);
                    let thr = ACV_WIN[pw.vr as usize];
                    if first(pw.seen, 2) {
                        pw.v_ac = a;
                        pw.v_dc = d;
                    } else {
                        ema(thr, 0.25, a, &mut pw.v_ac);
                        ema(thr, 0.25, d, &mut pw.v_dc);
                    }
                    pw.seen |= 2;
                }
            }
            _ => return false,
        }
        true
    }

    /// Fallback: one result of the stock AC V row (V phase) or AC A row,
    /// with the stock scaling and calibration (`poll_ac`).
    fn pw_take_alt(&mut self, rms: i64, lpf: i32) -> bool {
        let pw = &mut *self.pw;
        let sq = sqrt(rms.max(0) as f32);
        if sq > 262_144.0 {
            return false;
        }
        if pw.phase & 1 == 0 {
            let r = pw.vr as u32;
            let s = K_ACV[r as usize] * CODE;
            let a = (sq * s * cal(0x34 + 4 * r, 1.0) - cal(0x44 + 4 * r, 0.0)).max(0.0);
            if pw.seen & 2 == 0 {
                pw.v_ac = a;
            } else {
                ema(ACV_WIN[r as usize], 0.25, a, &mut pw.v_ac);
            }
            pw.v_dc = 0.0;
            pw.seen |= 2;
        } else {
            let (iu, ir) = iu_ir(pw.ik);
            let ci = (iu * 2 + ir) as u32;
            let us = [1.0, 1e-3, 1e-6][iu as usize];
            let s = CODE / (SHUNT_OHM[iu as usize] * MV_DIV[ir as usize]);
            let a = (sq * s * cal(0xC4 + 4 * ci, 1.0) - cal(0xDC + 4 * ci, 0.0) * us).max(0.0);
            let d = lpf as f32 * s * cal(0xF4 + 4 * ci, 1.0) - cal(0x10C + 4 * ci, 0.0) * us;
            let thr = ACA_WIN[ci as usize] * us;
            if pw.seen & 4 == 0 {
                pw.i_ac = a;
                pw.i_dc = d;
            } else {
                ema(thr, 0.15, a, &mut pw.i_ac);
                ema(thr, 0.35, d, &mut pw.i_dc);
            }
            pw.seen |= 4;
        }
        true
    }

    /// Autorange the V (`v`) or the I channel after its result (stock
    /// thresholds: up above 1.03 x the limit at 60000 counts, down below
    /// 0.1 x limit / 1.03). True = a range changed (row rewritten).
    fn pw_autorange(&mut self, clk: &mut Clock, v: bool) -> bool {
        let ov = self.over();
        let pw = &mut *self.pw;
        let changed = if v {
            let m = hypot(pw.v_ac, pw.v_dc);
            let r = pw.vr as usize;
            if !pw.v_auto {
                false
            } else if m > ov * V_LIM[r] && r < 3 {
                pw.vr += 1;
                true
            } else if m < 0.1 * V_LIM[r] / ov && r > 0 {
                pw.vr -= 1;
                true
            } else {
                false
            }
        } else {
            let m = hypot(pw.i_ac, pw.i_dc);
            let k = pw.ik as usize;
            // AUTO+ (stock m+7 = 1) never steps into the uA shunt.
            let kmin = if self.a_auto == 1 { 2 } else { 0 };
            if !pw.i_auto {
                false
            } else if m > ov * I_LIM[k] && k < 5 {
                pw.ik += 1;
                true
            } else if m < 0.1 * I_LIM[k] / ov && k > kmin {
                pw.ik -= 1;
                true
            } else {
                false
            }
        };
        if changed {
            self.pw.keep = if v { 4 } else { 2 };
            self.pw_apply(clk);
        }
        changed
    }

    /// Per accepted reading: energy (the previous P over the time since the
    /// previous reading, so a gap holds the last P), the current limit and
    /// the phase calibration.
    fn pw_account(&mut self, now: u64, pv: &PowerVals) {
        let pw = &mut *self.pw;
        if pw.run {
            if let Some(t) = pw.t_last {
                let dt = now.saturating_sub(t);
                let s = dt as f32 * 1e-3;
                if !pw.alt || pw.dc {
                    pw.e.add(pw.p_tot * s);
                }
                pw.va.add(pw.s_last * s);
                pw.t_int += dt;
            }
            pw.t_last = Some(now);
        } else {
            pw.t_last = None;
        }
        // The values for the next interval: this reading's.
        pw.s_last = if pv.s.is_finite() { pv.s } else { 0.0 };
        pw.p_tot = if pv.p.is_finite() { pv.p } else { 0.0 };
        if pv.i > pw.i_cont {
            if pw.over_t0.is_none() {
                pw.over_t0 = Some(now);
                pw.over_beeped = false;
            }
        } else if pv.i < 0.95 * pw.i_cont {
            pw.over_t0 = None;
        }
        if pw.cal != 0 && pw.cal_n >= 8 {
            let m = pw.cal_acc / pw.cal_n as f32;
            pw.cal_n = 0;
            pw.cal_acc = 0.0;
            if pw.cal == 1 {
                pw.cal_pa = m;
                pw.cal = 2;
                let d: i16 = if pw.cal_s0 > 79 { -48 } else { 48 };
                pw.sph = (pw.cal_s0 as i16 + d) as i8;
                pw.cal_req = true;
            } else {
                let d = pw.sph as f32 - pw.cal_s0 as f32;
                let slope = (m - pw.cal_pa) / d;
                pw.cal = 0;
                if slope.abs() > 1e-7 {
                    let s = (pw.cal_s0 as f32 - pw.cal_pa / slope).clamp(-127.0, 127.0);
                    pw.sph = (if s < 0.0 { s - 0.5 } else { s + 0.5 }) as i8;
                    pw.cal_result = Some(true);
                } else {
                    pw.sph = pw.cal_s0;
                    pw.cal_result = Some(false);
                }
                pw.cal_req = true;
            }
        }
    }

    /// After a V or I result: the counter's source and window. The V tap
    /// (SENSE, DC-coupled) with the R21 window around its DC (`cmp_window`);
    /// else the current on PB4 (AC-coupled, the default window) when its AC
    /// part at OP1O is >= 25 mV rms (INFERRED: the stock AC A rows count
    /// there); else no frequency ("--").
    fn pw_cmp_select(&mut self, now: u64) {
        let pw = &mut *self.pw;
        if pw.seen & 6 != 6 {
            return;
        }
        let k = KV[pw.vr as usize] * pw.v_gain(pw.vr);
        let (iu, ir) = iu_ir(pw.ik);
        let i_op = pw.i_ac * SHUNT_OHM[iu as usize] * MV_DIV[ir as usize];
        let (r20, r21, ok) = match cmp_window(pw.v_dc / k, pw.v_ac / k) {
            Some(w) => (R20_SENSE, w, true),
            None if i_op >= 0.025 => (R20_PB4, R21_DEFAULT, true),
            None => (pw.r20, pw.r21, false),
        };
        pw.f_ok = ok;
        if (r20, r21) != (pw.r20, pw.r21) {
            pw.r20 = r20;
            pw.r21 = r21;
            if r21 != self.pw.row[1] {
                hy::write(0x21, &[r21]);
                self.pw.row[1] = r21;
            }
            self.pw.row[0] = r20 | 0x02;
            // Restart the counter on the new source now.
            self.freq.next = 0;
            self.freq.f_khz = 0.0;
            self.freq_secondary(now, r20);
        }
    }

    /// POW:ZERO / the Power Settings "Zero" button: with the leads open
    /// (nothing on V-COM, nothing through A) the LPF zeros of the six
    /// current ranges (XX phase) and of the V channel (YY phase, the current
    /// V range), ~6 s. Refused per step when a signal is present
    /// (`JR_SIGNAL`: the old zeros stay).
    pub fn pw_zero_start(&mut self, clk: &mut Clock) -> bool {
        self.pw_job_start(clk, JOB_ZERO)
    }

    /// POW:VOLT:CAL / the "V cal" button: with a steady AC or DC voltage on
    /// V-COM (10..70 % (AC) / 100 % (DC) of the V range), the power V
    /// channel and the stock's factory-calibrated DC V (AD1) or AC V row of
    /// the same range, alternately, two rounds; the ratio becomes the
    /// range's gain. The kind follows the larger part of the first round.
    pub fn pw_vcal_start(&mut self, clk: &mut Clock) -> bool {
        self.pw_job_start(clk, JOB_VCAL)
    }

    fn pw_job_start(&mut self, clk: &mut Clock, job: u8) -> bool {
        let pw = &mut *self.pw;
        if !pw.on || pw.job != 0 {
            return false;
        }
        if pw.alt {
            pw.job_last = job;
            pw.job_res = JR_METHOD;
            return false;
        }
        pw.job_save = (pw.vr, pw.ik, pw.v_auto, pw.i_auto);
        pw.job = job;
        pw.job_last = job;
        pw.job_step = 0;
        pw.job_res = JR_RUN;
        pw.job_vr = pw.vr;
        pw.job_zx = pw.zx;
        pw.cal = 0;
        self.pw_job_step(clk);
        true
    }

    /// Write the row of the current job step and settle.
    fn pw_job_step(&mut self, clk: &mut Clock) {
        let pw = &mut *self.pw;
        pw.job_n = 0;
        pw.job_a = 0.0;
        pw.job_b = 0.0;
        if pw.job == JOB_ZERO && pw.job_step < 6 {
            pw.ik = pw.job_step;
        }
        let vref = pw.job == JOB_VCAL && pw.job_step & 1 == 1;
        let (settle, skip) = match (vref, pw.job_dc) {
            (true, false) => (VCAL_AC_SETTLE_MS, ALT_V_SKIP),
            (true, true) => (450, JOB_SKIP),
            _ => (SETTLE_MS, JOB_SKIP),
        };
        pw.skip = skip;
        self.write_config(clk);
        self.deadline = clk.now_ms() + settle;
    }

    /// End the job (`res`): restore the ranges, back to the phase machine.
    fn pw_job_end(&mut self, res: u8) {
        let pw = &mut *self.pw;
        if pw.job == 0 {
            return;
        }
        pw.job = 0;
        pw.job_res = res;
        (pw.vr, pw.ik, pw.v_auto, pw.i_auto) = pw.job_save;
        // `pw_apply` follows from the caller (or the function changes).
        pw.cal_req = pw.on;
    }

    /// A job's result: the zero / gain steps (no reading meanwhile).
    fn pw_job_poll(&mut self, clk: &mut Clock) -> Option<Reading> {
        let pw = &*self.pw;
        let vref = pw.job == JOB_VCAL && pw.job_step & 1 == 1;
        let r = pw.vr.min(3) as usize;
        // (main value, second value, sqrt(RMS) for the zero's signal check)
        let (x, y, sq) = if vref && pw.job_dc {
            let ad1 = self.ad1()?;
            let mut v = ad1 as f32 * K_ACV[r] * (1.25 / 0.9) / AD1_DIV * cal(0x04 + 4 * r as u32, 1.0)
                - cal(0x14 + 4 * r as u32, 0.0);
            if v < 0.0 {
                v *= DCV_NEGK[r];
            }
            (v, 0.0, 0.0)
        } else {
            let need = hy::INTF_RMS | hy::INTF_LPF;
            if hy::read1(hy::R_INTF) & need != need {
                return None;
            }
            let mut b = [0u8; 14];
            hy::read(hy::R_AD1, &mut b);
            let lpf = hy::lpf19(&b[6..9]);
            let rms = hy::rms_signed(&b[9..14]);
            let sq = sqrt(rms.max(0) as f32);
            if vref {
                // The stock AC V row with its calibration (`poll_ac`).
                let v = (sq * K_ACV[r] * CODE * cal(0x34 + 4 * r as u32, 1.0) - cal(0x44 + 4 * r as u32, 0.0)).max(0.0);
                (v, 0.0, sq)
            } else if pw.job == JOB_VCAL {
                // The power V channel, raw (gain 1, zero applied).
                let zy = if pw.zero_ok { pw.zy as i32 } else { 0 };
                (sq * CODE * KV[r], (lpf - zy) as f32 * CODE * KV[r], sq)
            } else {
                (lpf as f32, 0.0, sq)
            }
        };
        let pw = &mut *self.pw;
        if pw.skip > 0 {
            pw.skip -= 1;
            return None;
        }
        if pw.job == JOB_ZERO {
            // Against the previous zero of the step (a stored zero of
            // ~1000 codes is not a signal).
            let old = if pw.job_step < 6 { pw.job_zx[pw.job_step as usize] } else { pw.zy } as i32;
            let old = if pw.zero_ok { old } else { 0 };
            if sq > ZERO_MAX_RMS || (x as i32 - old).abs() > ZERO_MAX_LPF {
                self.pw_job_end(JR_SIGNAL);
                return None;
            }
        }
        pw.job_a += x;
        pw.job_b += y;
        pw.job_n += 1;
        if pw.job_n < JOB_N {
            return None;
        }
        let (a, b) = (pw.job_a / JOB_N as f32, pw.job_b / JOB_N as f32);
        let step = pw.job_step;
        let res = match pw.job {
            JOB_ZERO => {
                let z = (if a < 0.0 { a - 0.5 } else { a + 0.5 }) as i32;
                let z = z.clamp(-32768, 32767) as i16;
                if step < 6 {
                    pw.job_zx[step as usize] = z;
                    None
                } else {
                    pw.zx = pw.job_zx;
                    pw.zy = z;
                    pw.zero_ok = true;
                    Some(JR_OK)
                }
            }
            _ if step & 1 == 0 => {
                // Power V channel: AC (a) or DC (b) part, the kind decided by
                // the first round.
                if step == 0 {
                    pw.job_dc = b.abs() > a;
                }
                let m = if pw.job_dc { b } else { a };
                let lim = V_LIM[r];
                let max = if pw.job_dc { VCAL_MAX_DC } else { VCAL_MAX_AC };
                pw.job_raw[(step / 2) as usize] = m;
                if m.abs() < VCAL_MIN * lim {
                    Some(JR_SMALL)
                } else if m.abs() > max * lim {
                    Some(JR_BIG)
                } else {
                    None
                }
            }
            _ => {
                pw.job_ref[(step / 2) as usize] = a;
                if step < 3 {
                    None
                } else {
                    let r0 = pw.job_ref[0] / pw.job_raw[0];
                    let r1 = pw.job_ref[1] / pw.job_raw[1];
                    let g = 0.5 * (r0 + r1);
                    let steady = (pw.job_raw[0] - pw.job_raw[1]).abs() <= VCAL_SPREAD * pw.job_raw[0].abs();
                    if !g.is_finite() || (r0 - r1).abs() > VCAL_SPREAD * g.abs() || !steady {
                        Some(JR_UNSTABLE)
                    } else if !(0.9..=1.1).contains(&g) {
                        Some(JR_RANGE)
                    } else {
                        pw.vgain[r] = g;
                        Some(JR_OK)
                    }
                }
            }
        };
        match res {
            Some(res) => self.pw_job_end(res),
            None => {
                self.pw.job_step += 1;
                self.pw_job_step(clk);
            }
        }
        None
    }

    /// What the DMM page shows while no full reading exists yet: the V of
    /// the latest V result (the I channel still autoranging from 10 A) and
    /// the state text. None when a reading exists or nothing is measured.
    pub fn pw_partial(&self) -> Option<(Option<Val>, &'static str)> {
        let pw = &*self.pw;
        if !pw.on {
            return None;
        }
        let txt = match pw.job {
            JOB_ZERO => "Zeroing: leads open",
            JOB_VCAL => "V calibration...",
            _ if self.pw_vals().is_some() => return None,
            _ => "I ranging...",
        };
        let v = (pw.seen & 2 != 0 && pw.job == 0).then(|| Val::new(hypot(pw.v_ac, pw.v_dc), 4 - pw.vr.min(3), "V"));
        Some((v, txt))
    }

    /// Pending SPHACAL rewrite of the calibration (from `poll`).
    pub(super) fn pw_service(&mut self, clk: &mut Clock) {
        if self.pw.cal_req {
            self.pw.cal_req = false;
            self.pw_apply(clk);
        }
    }

    /// The continuous-current limit just expired (once per expiry): the app
    /// toasts and beeps.
    pub fn pw_over_expired(&mut self) -> bool {
        let pw = &mut *self.pw;
        match pw.over_t0 {
            Some(t0) if pw.t_lim > 0 && !pw.over_beeped && self.t_poll.saturating_sub(t0) >= pw.t_lim as u64 * 1000 => {
                pw.over_beeped = true;
                true
            }
            _ => false,
        }
    }
}

/// The readouts of a power reading: main = P (fallback: S) in the
/// `p_format` of the range pair, V in the AC V format of its range, I in the
/// AC A format (A / mA / uA) of its range, and the main value in SI.
pub fn reading_vals(pv: &PowerVals) -> (Val, Val, Val, f32) {
    let (iu, ir) = iu_ir(pv.ik);
    let vd = 4 - pv.vr.min(3);
    let v = if pv.v_ol { Val::ol(vd, "V") } else { Val::new(pv.v, vd, "V") };
    let (us, unit, idec) = match iu {
        0 => (1.0, "A", 4 - ir),
        1 => (1e3, "mA", 3 - ir),
        _ => (1e6, "\u{b5}A", 2 - ir),
    };
    let i = if pv.i_ol { Val::ol(idec, unit) } else { Val::new(pv.i * us, idec, unit) };
    let (main_si, w) = if pv.alt { (pv.s, false) } else { (pv.p, true) };
    let (x, dec, u) = p_format(V_LIM[pv.vr.min(3) as usize] * I_LIM[pv.ik.min(5) as usize], main_si, w);
    let ol = pv.v_ol || pv.i_ol;
    let main = if ol { Val::ol(dec, u) } else { Val::new(x, dec, u) };
    (main, v, i, if ol { 0.0 } else { main_si })
}

/// Main-value format of P (or S) for a range pair whose full-scale product
/// is `fs` W: "W" with 4 .. 0 decimals (60000-count style: 3.6000 W, 36.000,
/// 360.00, 3600.0, 10000.0 W), "mW" below 1 W of full scale. Returns
/// (value in the unit, decimals, unit).
pub fn p_format(fs: f32, si: f32, watts: bool) -> (f32, u8, &'static str) {
    // 1000 V x 10 A: kW (the F78 cells have no 6-digit integer format).
    if fs >= 9000.0 {
        return (si * 1e-3, 3, if watts { "kW" } else { "kVA" });
    }
    let (k, unit) = if fs < 1.0 { (1e3, if watts { "mW" } else { "mVA" }) } else { (1.0, if watts { "W" } else { "VA" }) };
    let f = fs * k * 1.05;
    let dec = if f < 10.0 {
        4
    } else if f < 100.0 {
        3
    } else if f < 1000.0 {
        2
    } else if f < 10_000.0 {
        1
    } else {
        1
    };
    (si * k, dec, unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every byte of every power row is the stock I row's, the stock V
    /// row's (R2A..R30) or one of the listed overrides (doc 3.10 step 2).
    #[test]
    fn power_row_bytes() {
        for vr in 0..4u8 {
            for k in 0..6u8 {
                for st in [R34_XY, R34_XX, R34_YY] {
                    let row = power_row(vr, k, st);
                    let (iu, ir) = iu_ir(k);
                    let irow = if iu == 0 { CFG_F4[ir as usize] } else { CFG_F5_6[ir as usize] };
                    let vrow = if vr == 0 { CFG_F1[0] } else { CFG_F0[vr as usize] };
                    for (j, (&b, &i)) in row.iter().zip(irow.iter()).enumerate() {
                        let want = match j {
                            0 => 0x12,
                            5 => i & !0x02,
                            9 => 0xF8,
                            10..=16 => vrow[j],
                            18 => 0x80 | (i & 0x0F),
                            20 => st,
                            21 => 0x54,
                            _ => i,
                        };
                        assert_eq!(b, want, "vr {vr} k {k} st {st:#x} R{:02X}", 0x20 + j);
                    }
                    // The decoded intent: OP1 on the shunt (PB2 for A, PB1
                    // for mA / uA), OP2 on SENSE, AD2 = OP1O - PB3, AD3 =
                    // OP2O - AGND (different negative inputs, datasheet
                    // note 8), OP2 x1, OP1 x10 in the low range, AD1 off.
                    assert_eq!(row[18] & 0x07, if iu == 0 { 6 } else { 5 });
                    assert_eq!(row[18] & 0xF8, 0x88);
                    assert_eq!(row[7] >> 4, 0x2);
                    assert_eq!(row[21] >> 4, 0x5);
                    assert_eq!(row[5] & 0x03, if ir == 0 { 1 } else { 0 });
                    assert_eq!(row[3] & 0x80, 0);
                }
            }
        }
        // Examples of the doc / the emulator agent's table.
        assert_eq!(power_row(1, 5, R34_XY), [0x12, 0xdd, 0x15, 0x03, 0x52, 0x40, 0x80, 0x2d, 0x31, 0xf8, 0x20, 0x00, 0x00, 0x09, 0x28, 0xa0, 0x80, 0xc7, 0x8e, 0xac, 0x88, 0x54]);
        assert_eq!(power_row(0, 2, R34_XY), [0x12, 0xdd, 0x14, 0x03, 0x35, 0x41, 0x80, 0x2d, 0x31, 0xf8, 0x06, 0x00, 0x00, 0x90, 0x28, 0xa0, 0x80, 0xc7, 0x8d, 0xac, 0x88, 0x54]);
        assert_eq!(power_row(2, 1, R34_YY)[20], 0x8C);
    }

    #[test]
    fn ranges_and_r36() {
        assert_eq!([iu_ir(0), iu_ir(1), iu_ir(2), iu_ir(3), iu_ir(4), iu_ir(5)], [(2, 0), (2, 1), (1, 0), (1, 1), (0, 0), (0, 1)]);
        assert_eq!([r36_of(0), r36_of(5), r36_of(-5), r36_of(127), r36_of(-128)], [0x00, 0x85, 0x05, 0xFF, 0x7F]);
        // Fallback rows are the stock ones.
        assert_eq!(alt_row(true, 2, 5), CFG_F0[2]);
        assert_eq!(alt_row(false, 2, 5), CFG_F4[1]);
        assert_eq!(alt_row(false, 2, 0), CFG_F5_6[0]);
    }

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol * b.abs().max(1e-9)
    }

    #[test]
    fn totals_and_burden() {
        // 230 V / 1 A resistive, no burden: P = S = 230, PF 1, Q 0.
        let (p, s, q, pf, v, i) = totals(230.0, 230.0, 0.0, 1.0, 0.0, 0.0);
        assert!(close(p, 230.0, 1e-6) && close(s, 230.0, 1e-6) && q < 1e-3 && close(pf, 1.0, 1e-6) && close(v, 230.0, 1e-6) && close(i, 1.0, 1e-6));
        // PF 0.5.
        let (p, s, _, pf, _, _) = totals(115.0, 230.0, 0.0, 1.0, 0.0, 0.0);
        assert!(close(p, 115.0, 1e-6) && close(s, 230.0, 1e-6) && close(pf, 0.5, 1e-6));
        // DC: P = Vdc Idc, V and I the DC parts.
        let (p, s, q, pf, v, i) = totals(0.0, 0.0, 12.0, 0.0, 2.0, 0.0);
        assert!(close(p, 24.0, 1e-6) && close(s, 24.0, 1e-6) && q < 1e-3 && close(pf, 1.0, 1e-6) && close(v, 12.0, 1e-6) && close(i, 2.0, 1e-6));
        // Reversed current: P < 0, PF -1.
        let (p, _, _, pf, _, _) = totals(0.0, 0.0, 12.0, 0.0, -2.0, 0.0);
        assert!(close(p, -24.0, 1e-6) && close(pf, -1.0, 1e-6));
        // Burden: 5 V source, 0.4 A, R_path 0.6 ohm (DC, wiring 1): the
        // load sees 5 - 0.24 = 4.76 V and takes 1.904 W; the meter 0.096 W.
        let (p, s, _, pf, v, i) = totals(0.0, 0.0, 5.0, 0.0, 0.4, 0.6);
        assert!(close(p, 1.904, 1e-5) && close(v, 4.76, 1e-5) && close(s, 1.904, 1e-5) && close(pf, 1.0, 1e-5) && close(i, 0.4, 1e-6));
        // Reversed (a source on the load side), burden still subtracted.
        let (p, _, _, _, v, _) = totals(0.0, 0.0, 5.0, 0.0, -0.4, 0.6);
        assert!(close(p, -2.096, 1e-5) && close(v, 5.24, 1e-5));
        // AC: v_L = v - R i holds for any phase: 230 V, 1 A at PF 0.5, R 1:
        // P_L = 115 - 1, V_L^2 = 230^2 - 2 * 115 + 1.
        let (p, s, _, _, v, _) = totals(115.0, 230.0, 0.0, 1.0, 0.0, 1.0);
        assert!(close(p, 114.0, 1e-5) && close(v, (230.0f32 * 230.0 - 230.0 + 1.0).sqrt(), 1e-5) && close(s, v, 1e-6));
    }

    #[test]
    fn energy_accumulator_keeps_small_steps() {
        // 1 mW for 10 h in 100 ms steps: 0.01 Wh (an f32 sum of joules in
        // the mJ range keeps them; check a large base too).
        let mut a = Acc::ZERO;
        a.add(3.6e6); // 1 kWh already
        for _ in 0..360_000 {
            a.add(1e-3 * 0.1);
        }
        assert!((a.wh() - 1000.01).abs() < 1e-4, "{}", a.wh());
        let mut b = Acc::ZERO;
        for _ in 0..1000 {
            b.add(-0.25);
        }
        assert!((b.wh() + 250.0 / 3600.0).abs() < 1e-6);
    }

    #[test]
    fn p_formats() {
        // (full-scale product) -> decimals and unit.
        let f = |fs: f32| {
            let (_, d, u) = p_format(fs, 0.0, true);
            (d, u)
        };
        assert_eq!(f(6.0 * 600e-6), (4, "mW")); // 3.6 mW
        assert_eq!(f(6.0 * 0.06), (2, "mW")); // 360 mW
        assert_eq!(f(6.0 * 6.0), (3, "W"));
        assert_eq!(f(60.0 * 6.0), (2, "W"));
        assert_eq!(f(600.0 * 6.0), (1, "W"));
        assert_eq!(f(1000.0 * 10.0), (3, "kW"));
        assert_eq!(p_format(0.36, 0.1234, true).0, 123.4);
        assert_eq!(p_format(36.0, 1.0, false).2, "VA");
    }

    #[test]
    fn comparator_windows() {
        // A bipolar signal: the stock window (+-20.8 mV).
        assert_eq!(cmp_window(0.0, 0.1), Some(R21_DEFAULT));
        assert_eq!(cmp_window(0.001, 0.03), Some(R21_DEFAULT));
        // The T1 square on the 60 V tap: 0..96 mV (DC 48 mV, AC 48 mV rms):
        // RH 62.5 mV (0xB), RL 20.8 mV (0xF, the highest RL tap below
        // +0.2 V), both inside 0..96 mV.
        assert_eq!(cmp_window(0.048, 0.048), Some(0xBF));
        // An un-zeroed Y phantom (5 mV) keeps the default window.
        assert_eq!(cmp_window(0.005, 0.1), Some(R21_DEFAULT));
        // Negative DC with ripple: -0.3 V +- 0.15 V.
        let w = cmp_window(-0.3, 0.15).unwrap();
        let rh = CMP_RH.iter().find(|e| e.0 == w >> 4).unwrap().1;
        let rl = CMP_RL.iter().find(|e| e.0 == w & 15).unwrap().1;
        assert!(rh > -0.3 && rh < -0.3 + 0.135 && rl < -0.3 && rl > -0.3 - 0.135, "{rh} {rl}");
        // Pure DC / too small an AC part: no window.
        assert_eq!(cmp_window(0.048, 0.0), None);
        assert_eq!(cmp_window(0.0, 0.01), None);
        assert_eq!(cmp_window(0.5, 0.05), None);
    }

    #[test]
    fn ring_text_format() {
        let mut pw = Pw::new();
        pw.ring[0] = Raw { stat: R34_XY, disc: true, rms: -12345678901, lpf: -7, dt: 107 };
        pw.ring_k = 1;
        let mut b = [0u8; 128];
        let n = pw.ring_text(&mut b);
        assert_eq!(core::str::from_utf8(&b[..n]).unwrap(), "1;XY,1,-12345678901,-7,107");
    }
}
