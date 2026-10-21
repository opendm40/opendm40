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
}

impl Pw {
    pub const fn new() -> Self {
        Pw {
            on: false,
            alt: false,
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
        }
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
        self.mode = 255;
        self.reset_values();
        self.entry = clk.now_ms();
        self.pw_apply(clk);
    }

    /// Leave the power meter (the next `set` does it: any function change).
    pub(super) fn pw_leave(&mut self) {
        self.pw.on = false;
        self.pw.freeze = 0;
        self.pw.poke_n = 0;
    }

    /// (Re)write the row for the current ranges / SPHACAL / method, restart
    /// the phase machine, settle. The parts measured so far are dropped (the
    /// scale changed); the energy keeps integrating the last P over the gap.
    pub fn pw_apply(&mut self, clk: &mut Clock) {
        let pw = &mut *self.pw;
        pw.phase = 0;
        pw.seen = 0;
        pw.taken = 0;
        pw.skip = if pw.alt { ALT_V_SKIP } else { 0 };
        pw.restarts = pw.restarts.wrapping_add(1);
        self.write_config(clk);
        self.deadline = clk.now_ms() + SETTLE_MS;
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
        pw.stat = if pw.freeze != 0 { pw.freeze } else { CYCLE[0] };
        let mut row = if pw.alt { alt_row(pw.phase & 1 == 0, pw.vr, pw.ik) } else { power_row(pw.vr, pw.ik, pw.stat) };
        if !pw.alt {
            for &(i, v) in &pw.poke[..pw.poke_n as usize] {
                if (i as usize) < row.len() {
                    row[i as usize] = v;
                }
            }
            pw.stat = row[20];
        }
        pw.row = row;
        pw.r36 = if pw.alt { 0 } else { r36_of(pw.sph) };
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
        if self.pw.alt {
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
        let need = if pw.alt { 6 } else { 7 };
        if !pw.on || pw.seen & need != need {
            return None;
        }
        let (iu, _) = iu_ir(pw.ik);
        let r = if pw.comp { pw.r_path[iu as usize] } else { 0.0 };
        let (p, s, q, pf, v, i) = if pw.alt {
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
            vdc: if pw.alt { f32::NAN } else { pw.v_dc },
            idc: pw.i_dc,
            f_hz: self.freq.f_khz * 1000.0,
            burden_v: i * pw.r_path[iu as usize],
            e_wh: pw.e.wh(),
            vah: pw.va.wh(),
            secs: (pw.t_int / 1000) as u32,
            p_avg: if pw.t_int > 0 && !pw.alt { pw.e.wh() * 3.6e6 / pw.t_int as f32 } else { f32::NAN },
            vr: pw.vr,
            ik: pw.ik,
            v_auto: pw.v_auto,
            i_auto: pw.i_auto,
            alt: pw.alt,
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
                alt: pw.alt,
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
        // Frequency of the voltage: the comparator on SENSE (method B); the
        // fallback's AC V row has it on PB4 (the coupled V node).
        if !self.pw.alt {
            self.freq_secondary(now, R20_SENSE);
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
        Some(self.pw_reading(&pv))
    }

    /// Method B: one accepted result of statistic `st`. False = garbage.
    fn pw_take(&mut self, st: u8, rms: i64, lpf: i32) -> bool {
        let pw = &mut *self.pw;
        let (iu, ir) = iu_ir(pw.ik);
        let ci = (iu * 2 + ir) as u32;
        let s_i = CODE / (SHUNT_OHM[iu as usize] * MV_DIV[ir as usize]);
        let s_v = CODE * KV[pw.vr as usize];
        let (g_i, g_il) = (cal(0xC4 + 4 * ci, 1.0), cal(0xF4 + 4 * ci, 1.0));
        let i_dc = lpf as f32 * s_i * g_il;
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
                    let (a, d) = (sq * s_v, lpf as f32 * s_v);
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
                if !pw.alt {
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
    fn ring_text_format() {
        let mut pw = Pw::new();
        pw.ring[0] = Raw { stat: R34_XY, disc: true, rms: -12345678901, lpf: -7, dt: 107 };
        pw.ring_k = 1;
        let mut b = [0u8; 128];
        let n = pw.ring_text(&mut b);
        assert_eq!(core::str::from_utf8(&b[..n]).unwrap(), "1;XY,1,-12345678901,-7,107");
    }
}
