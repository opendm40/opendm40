//! The experimental 600 Mohm range (ohm r7, `dmm::OHM_EXP`): the pure
//! part (row builder, AD1 phase plan, maths, the DBG:OHMX knobs and their
//! parser; host test tools/ohm600m-test). The Dmm glue (state, poll, the
//! range rules, the SCPI reply) is src/ohm600m_dmm.rs. Method, device
//! data and open questions: docs/ohm-600m.md.
//!
//! Row = the stock 6 Mohm row (f10_p8clr r5: Rref 10 Mohm, SFT1 100 k) with
//!   R31 SFUVR<3:0>  OP3's tap: AGNDP9 (~1.05 V) instead of VDS15 (~2.1 V)
//!   R23 bits 3:2    AD1 reference buffers off (as DBG:SRC; PB6 and FB are
//!                   low-impedance nodes)
//!   R22             the knob's OSR (default 0x16, 200 ms chopped)
//!   R24 / R28       per phase (SDIO kept)
//! and up to ROW_OVR knob byte overrides on top. At 600 Mohm the drop on
//! Rref is ~17 mV beside ~1.03 V on the jack: a ratio of ~60, beyond AD1's
//! ratiometric range, so the two voltages are converted against the
//! absolute reference N = PB6 - AGND (1.25 V MAX6190) in alternate
//! conversions and R = Rref * (V_jack - V_com) / (V_FB - V_jack - offset).
//!
//! AD1 phases (each: ENAD1 off, R24 / R28, ENAD1 on, `skip` conversions
//! dropped, one kept), selected by the plan's bits:
//!   0 X  OP1O - AGND / N   the jack through OP1 (PB0 itself is loaded)
//!   1 F  FB - AGND / N     OP3's side of Rref
//!   2 C  PB3 - AGND / N    COM (else taken as AGND)
//!   3 Q  OP1O - AGND / FB - AGND   jack / FB in one conversion (no
//!                          inter-conversion offset; SAD1FN has no OP1O,
//!                          so FB - OP1O cannot be one input pair)
//!   4 D  FB - RLU / N      the stock ratiometric pair (diagnostic only:
//!                          RLU is not the jack side of Rref in this row)
//! With Q in the plan the jack / drop come from q (V_FB measured when F is
//! in the plan, else VFB_NOM); without it from X and F.

use crate::hy3131::CFG_LEN;
use crate::hy3131_tables::CFG_F10_P8CLR;

/// The stock row the range starts from (6 Mohm, Rref 10 Mohm).
pub const BASE_ROW: usize = 5;
/// OP3 tap (SFUVR code) by default: AGNDP9 (dbgsrc::VREF_NAMES[3]).
pub const TAP_AGNDP9: u8 = 3;
/// FB with the AGNDP9 tap (V vs AGND): short.csv V_vref on R10M, one unit.
pub const VFB_NOM: f32 = 1.0476;
/// Full scale (ohm) and the display decimals (600.0 Mohm).
pub const FS_OHM: f32 = 600e6;
pub const DEC: u8 = 1;
/// LIVE? (a source of the other polarity on V/ohm): the jack this far
/// below COM. ~ dmm::LIVE_CODE (0.24 % of AD1's 1.389 V), inferred.
pub const LIVE_V: f32 = 0.0033;
/// Window / override sizes.
pub const NAVG_MAX: usize = 8;
pub const ROW_OVR: usize = 4;

pub const N_PH: usize = 5;
pub const PH_X: usize = 0;
pub const PH_F: usize = 1;
pub const PH_C: usize = 2;
pub const PH_Q: usize = 3;
pub const PH_D: usize = 4;
const REF_N: u8 = 0x31; // SAD1RH PB6, SAD1RL AGND
const REF_FB: u8 = 0x01; // SAD1RH FB, SAD1RL AGND
const FN_AGND: u8 = 3;
/// (SAD1FP, SAD1FN, R28) per phase (datasheet 12.3 AD1 input / reference mux).
const PHASES: [(u8, u8, u8); N_PH] = [(3, FN_AGND, REF_N), (1, FN_AGND, REF_N), (11, FN_AGND, REF_N), (3, FN_AGND, REF_FB), (1, 1, REF_N)];
const FS: f32 = 8_388_608.0;
/// PB6 (MAX6190), nominal.
pub const VREFO: f32 = 1.25;

/// The method's RAM knobs (DBG:OHMX; lost at reset).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Knobs {
    /// OP3 tap, SFUVR code 0..15.
    pub tap: u8,
    /// Phase bits (PH_*).
    pub plan: u8,
    /// Conversions dropped after each phase switch.
    pub skip: u8,
    /// R22 (AD1 OSR / chopper).
    pub osr: u8,
    /// Cycles in the moving average (1..NAVG_MAX).
    pub navg: u8,
    /// Readings are provisional this long after the row write (ms).
    pub settle_ms: u16,
    /// Two cycles agree within vet x R + VET_ABS_OHM.
    pub vet: f32,
    /// Gain on top of the 6 Mohm calibration slot (0x124 + 4 x 5).
    pub gain: f32,
    /// Subtracted from the drop (V): the X / F inter-conversion offset.
    pub offs: f32,
    /// Nominal Rref (ohm).
    pub rref: f32,
    /// Row byte overrides: (register 0x20..0x35, value); reg 0 = unused.
    pub ovr: [(u8, u8); ROW_OVR],
}

/// Absolute part of the vet (ohm): ~2 sigma of one cycle's noise at full
/// scale (6 uV rms per conversion on two conversions over a 17 mV drop).
pub const VET_ABS_OHM: f32 = 0.5e6;

impl Knobs {
    pub const DEF: Knobs = Knobs {
        tap: TAP_AGNDP9,
        plan: 1 << PH_X | 1 << PH_F,
        skip: 1,
        osr: 0x16,
        navg: 4,
        settle_ms: 3000,
        vet: 0.01,
        gain: 1.0,
        offs: 0.0,
        rref: 10e6,
        ovr: [(0, 0); ROW_OVR],
    };

    /// The plan can give R: Q, or X and F.
    pub fn plan_ok(plan: u8) -> bool {
        plan < 1 << N_PH && (plan & 1 << PH_Q != 0 || plan & (1 << PH_X | 1 << PH_F) == (1 << PH_X | 1 << PH_F))
    }
}

/// The plan's first phase at or after `ph` (wrapping).
pub fn next_phase(plan: u8, ph: usize) -> usize {
    (0..N_PH).map(|k| (ph + k) % N_PH).find(|&p| plan & 1 << p != 0).unwrap_or(PH_X)
}

/// R24 / R28 of phase `ph` for row `row` (keeps SDIO).
pub fn phase_regs(row: &[u8; CFG_LEN], ph: usize) -> (u8, u8) {
    let (fp, fnn, r28) = PHASES[ph % N_PH];
    (fp << 4 | (row[4] & 0x08) | fnn, r28)
}

/// The range's row for `k`, with the plan's first phase in R24 / R28.
pub fn build_row(k: &Knobs) -> [u8; CFG_LEN] {
    let mut c = CFG_F10_P8CLR[BASE_ROW];
    c[17] = (c[17] & 0xF0) | (k.tap & 0x0F);
    c[3] &= !0x0C;
    c[2] = k.osr;
    for &(reg, v) in &k.ovr {
        if (0x20..0x20 + CFG_LEN as u8).contains(&reg) {
            c[(reg - 0x20) as usize] = v;
        }
    }
    let (r24, r28) = phase_regs(&c, next_phase(k.plan, 0));
    c[4] = r24;
    c[8] = r28;
    c
}

/// AD1 input gain of the row (R25 AD1IG).
pub fn ig_of(row: &[u8; CFG_LEN]) -> f32 {
    [0.9, 1.8, 2.7, 3.6][(row[5] >> 4) as usize & 3]
}

/// One complete cycle (or a window of them) in volts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vals {
    /// Jack - COM (the drop across Rx).
    pub vx: f32,
    /// FB - jack - offs (the drop across Rref).
    pub drop: f32,
    pub vfb: f32,
    pub vcom: f32,
    /// D phase (FB - RLU), NaN when not in the plan.
    pub vd: f32,
}

/// Volts of the plan's codes (`codes[PH_*]`).
pub fn volts(codes: &[i32; N_PH], plan: u8, ig: f32, offs: f32) -> Vals {
    let v = |c: i32| c as f32 / FS * VREFO / ig;
    let has = |p: usize| plan & 1 << p != 0;
    let vfb = if has(PH_F) { v(codes[PH_F]) } else { VFB_NOM };
    let vcom = if has(PH_C) { v(codes[PH_C]) } else { 0.0 };
    let (jack, drop) = if has(PH_Q) {
        // code = IG * jack / FB * 2^23.
        let q = codes[PH_Q] as f32 / FS / ig;
        (q * vfb, (1.0 - q) * vfb)
    } else {
        let j = v(codes[PH_X]);
        (j, vfb - j)
    };
    Vals { vx: jack - vcom, drop: drop - offs, vfb, vcom, vd: if has(PH_D) { v(codes[PH_D]) } else { f32::NAN } }
}

/// R (ohm) from the jack / drop voltages and the total gain x Rref (`g`):
/// infinite when the drop is not positive (open leads, a positive source
/// above the tap).
pub fn ohms(vx: f32, drop: f32, g: f32) -> f32 {
    if drop > 0.0 { g * vx / drop } else { f32::INFINITY }
}

/// Over range (OL) at `over` (dmm OVER, 1.03 on the DM40C counts).
pub fn is_ol(r: f32, over: f32) -> bool {
    !(r <= FS_OHM * over)
}

/// Two cycles agree (the dmm OHM_VET_* idea at this range's noise).
pub fn agree(x: f32, p: f32, vet: f32) -> bool {
    if x.is_infinite() || p.is_infinite() {
        return x.is_infinite() && p.is_infinite();
    }
    (x - p).abs() <= vet * p.abs() + VET_ABS_OHM
}

/// The phase cycle and the moving window (Dmm glue keeps one).
#[derive(Clone, Copy)]
pub struct Cycle {
    pub phase: u8,
    pub skip: u8,
    /// Complete cycles since the row write.
    pub n: u32,
    pub codes: [i32; N_PH],
    /// The last complete cycle.
    pub done: [i32; N_PH],
    /// Window of (vx, drop) per cycle, `wn` filled, `wi` next slot.
    pub wx: [f32; NAVG_MAX],
    pub wd: [f32; NAVG_MAX],
    pub wn: u8,
    pub wi: u8,
}

impl Cycle {
    pub const NEW: Cycle = Cycle { phase: 0, skip: 0, n: 0, codes: [0; N_PH], done: [0; N_PH], wx: [0.0; NAVG_MAX], wd: [0.0; NAVG_MAX], wn: 0, wi: 0 };

    /// After a row write: the first phase of `k`'s plan (in the row), its
    /// skip.
    pub fn start(k: &Knobs) -> Cycle {
        Cycle { phase: next_phase(k.plan, 0) as u8, skip: k.skip, ..Cycle::NEW }
    }

    /// One AD1 conversion. Returns (R24, R28, cycle complete) when the
    /// phase advanced (the caller writes R23 with ENAD1 off, R24, R28, R23).
    pub fn step(&mut self, code: i32, row: &[u8; CFG_LEN], k: &Knobs) -> Option<(u8, u8, bool)> {
        if self.skip > 0 {
            self.skip -= 1;
            return None;
        }
        let ph = self.phase as usize % N_PH;
        self.codes[ph] = code;
        let next = next_phase(k.plan, ph + 1);
        let done = next <= ph;
        if done {
            self.done = self.codes;
            self.n = self.n.wrapping_add(1);
        }
        self.phase = next as u8;
        self.skip = k.skip;
        let (r24, r28) = phase_regs(row, next);
        Some((r24, r28, done))
    }

    /// Add a cycle to the window (`navg` long); a reset first empties it.
    pub fn push(&mut self, v: &Vals, navg: u8, reset: bool) {
        let n = (navg as usize).clamp(1, NAVG_MAX);
        if reset || self.wn as usize > n {
            self.wn = 0;
            self.wi = 0;
        }
        let i = self.wi as usize % n;
        self.wx[i] = v.vx;
        self.wd[i] = v.drop;
        self.wi = ((i + 1) % n) as u8;
        self.wn = (self.wn + 1).min(n as u8);
    }

    /// The window's mean (vx, drop): R from the ratio of the sums.
    pub fn mean(&self) -> (f32, f32) {
        let n = (self.wn as usize).max(1);
        let sx: f32 = self.wx[..n].iter().sum();
        let sd: f32 = self.wd[..n].iter().sum();
        (sx / n as f32, sd / n as f32)
    }
}

// ------------------------------------------------------------- DBG:OHMX

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Key {
    Tap,
    Plan,
    Skip,
    Osr,
    Navg,
    Settle,
    Vet,
    Gain,
    Offs,
    Rref,
}

pub const KEYS: [(&str, Key); 10] = [
    ("TAP", Key::Tap),
    ("PLAN", Key::Plan),
    ("SKIP", Key::Skip),
    ("OSR", Key::Osr),
    ("NAVG", Key::Navg),
    ("SETTLE", Key::Settle),
    ("VET", Key::Vet),
    ("GAIN", Key::Gain),
    ("OFFS", Key::Offs),
    ("RREF", Key::Rref),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Req {
    /// DEF: every knob back to Knobs::DEF.
    Def,
    Set(Key, f32),
    /// ROW,<reg>,<value> / ROW,CLR.
    Row(u8, u8),
    RowClr,
}

impl Knobs {
    /// Apply `r`; false = refused (out of range, no override slot left).
    pub fn apply(&mut self, r: Req) -> bool {
        let int = |v: f32, lo: f32, hi: f32| ((lo..=hi).contains(&v) && v == (v as u32) as f32).then_some(v as u32);
        match r {
            Req::Def => *self = Knobs::DEF,
            Req::RowClr => self.ovr = [(0, 0); ROW_OVR],
            Req::Row(reg, v) => {
                if !(0x20..0x20 + CFG_LEN as u8).contains(&reg) {
                    return false;
                }
                let Some(i) = self.ovr.iter().position(|o| o.0 == reg).or_else(|| self.ovr.iter().position(|o| o.0 == 0)) else {
                    return false;
                };
                self.ovr[i] = (reg, v);
            }
            Req::Set(k, v) => match k {
                Key::Tap => match int(v, 0.0, 15.0) {
                    Some(t) => self.tap = t as u8,
                    None => return false,
                },
                Key::Plan => match int(v, 1.0, 31.0).filter(|&p| Knobs::plan_ok(p as u8)) {
                    Some(p) => self.plan = p as u8,
                    None => return false,
                },
                Key::Skip => match int(v, 0.0, 4.0) {
                    Some(s) => self.skip = s as u8,
                    None => return false,
                },
                Key::Osr => match int(v, 0.0, 255.0) {
                    Some(o) => self.osr = o as u8,
                    None => return false,
                },
                Key::Navg => match int(v, 1.0, NAVG_MAX as f32) {
                    Some(n) => self.navg = n as u8,
                    None => return false,
                },
                Key::Settle => match int(v, 0.0, 60000.0) {
                    Some(ms) => self.settle_ms = ms as u16,
                    None => return false,
                },
                Key::Vet if (0.0..=1.0).contains(&v) => self.vet = v,
                Key::Gain if (0.5..=2.0).contains(&v) => self.gain = v,
                Key::Offs if (-0.05..=0.05).contains(&v) => self.offs = v,
                Key::Rref if (1e6..=1e8).contains(&v) => self.rref = v,
                _ => return false,
            },
        }
        true
    }
}

fn eq_nc(a: &[u8], b: &str) -> bool {
    a.len() == b.len() && a.iter().zip(b.bytes()).all(|(x, y)| x.to_ascii_uppercase() == y)
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [b' ' | b'\t' | b'"', r @ ..] = s {
        s = r;
    }
    while let [r @ .., b' ' | b'\t' | b'"' | b'\r' | b'\n'] = s {
        s = r;
    }
    s
}

/// A byte: decimal or 0x<hex> (ROW's register and value).
fn byte(t: &[u8], num: fn(&[u8]) -> Option<f32>) -> Option<u8> {
    if let [b'0', b'x' | b'X', h @ ..] = t {
        let mut v = 0u32;
        for &c in h {
            v = (v * 16 + (c as char).to_digit(16)?).min(256);
        }
        return u8::try_from(v).ok().filter(|_| !h.is_empty());
    }
    let x = num(t)?;
    ((0.0..=255.0).contains(&x) && x == (x as u8) as f32).then_some(x as u8)
}

/// `DBG:OHMX DEF | <key>,<value> | ROW,<reg>,<value> | ROW,CLR` argument;
/// `num` parses a number (scpi::parse_num).
pub fn parse(arg: &[u8], num: fn(&[u8]) -> Option<f32>) -> Option<Req> {
    let mut it = trim(arg).split(|&b| b == b',').map(trim);
    let k = it.next()?;
    let r = if eq_nc(k, "DEF") {
        Req::Def
    } else if eq_nc(k, "ROW") {
        let a = it.next()?;
        if eq_nc(a, "CLR") {
            Req::RowClr
        } else {
            Req::Row(byte(a, num)?, byte(it.next()?, num)?)
        }
    } else {
        let key = KEYS.iter().find(|(n, _)| eq_nc(k, n))?.1;
        Req::Set(key, num(it.next()?)?)
    };
    it.next().is_none().then_some(r)
}
