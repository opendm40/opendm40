//! DBG:SRC (USB only, experimental): run a stock ohm / diode row with OP3's
//! VREF taken from another SFUVR tap and read the nodes around the
//! reference resistor with AD1, to map what the HY3131 can push out of the
//! V/ohm jack (host side: tools/srcmap.py). Pure part: the tap / path
//! tables, the row builder, the AD1 phase plan, the conversion to volts and
//! the parser (host test tools/dbgsrc-test). The Dmm glue (state, poll,
//! SCPI reply) is src/dbgsrc_dmm.rs.
//!
//! Row = the stock row of the path (f10_p8clr r1..r6, f12 diode) with only
//!   R31 SFUVR<3:0>  the tap (SMODE<3> stays 0: never the CMPO-switched column)
//!   R23 bits 3:2    AD1 reference buffers off (the references below are
//!                   low-impedance nodes; bits 1:0, the input buffers, kept)
//!   R24 SAD1FP/FN   per phase (SDIO, bit 3, kept)
//!   R28 SAD1RH/RL   per phase
//! changed. Relay, PE0 / PE1, PA / PS / DS / FS / SS switches, SMODE, OP1 /
//! OP2 and the generator are whatever the stock row has (Dmm::write_config).
//!
//! VREF taps (datasheet 12.2, VREF table, SMODE<3> = 0 column): index = the
//! SFUVR code. AGNDP1..5, AGNDN1..5 and VDS4..14 exist only as comparator
//! thresholds (SCMPRH / SCMPRL), OP3 cannot take them.
//!
//! AD1 phases (each: ENAD1 off, R24 / R28, ENAD1 on, one conversion dropped,
//! one kept). References: N = PB6 - AGND (1.25 V MAX6190, the DC V rows'),
//! W = VDDA - AGND, S = VDDA - VSSA. AD1 code = IG * Vin / Vref * 2^23 (IG
//! from the row's R25, 0.9 in every path).
//!   0 REFO - AGND / W   -> VDDA - AGND = IG * 1.25 * 2^23 / code
//!   1 REFO - AGND / S   -> VDDA - VSSA, so AGND - VSSA = S - W
//!   2 / 3  <sense> - AGND, N / W   (input node; default PB0)
//!   4 / 5  FB - AGND, N / W        (OP3 side of Rref: FS on the DS pin)
//!   6 / 7  VREF - AGND, N / W      (OP3's + input = the tap)
//!   8      FB - RLU, N             (the drop the stock ohm reference uses;
//!                                   saturated: FB - <sense> from 2..5)
//!   9      PB3 - AGND, N           (COM side of the shunt chain)
//! A node is taken from its N conversion when the W one puts it inside
//! 90 % of N's full scale (1.389 V), else from the W one. REFO = 1.25 V
//! (SREFO = 1: PB6 buffered) is nominal, not measured.
//! *Inferred*: RLU is the jack side of Rref (the stock ratiometric pair is
//! FB - RLU); PB0 is the jack through 900 k (/10 with SDIO in the diode row);
//! the AD1 input buffers may clip above VDDA - 1.1 V (OP common-mode spec,
//! assumed to apply to them).

use crate::hy3131::CFG_LEN;
use crate::hy3131_tables::{CFG_F10_P8CLR, CFG_F12};

/// VREF taps by SFUVR code (SMODE<3> = 0).
pub const VREF_NAMES: [&str; 16] = [
    "VDS17", "VDS16", "VDS15", "AGNDP9", "AGNDP8", "AGNDP7", "AGNDP6", "AGNDP0", "VDS1", "VDS2", "VDS3", "AGNDN9", "AGNDN8", "AGNDN7", "AGNDN6", "PB7",
];

/// The taps the stock current-source rows (SMODE 0110, SVDD + SCP + SCN)
/// use: VDS17 (f10 row 7), VDS16 (r6), VDS15 (diode). Others need FORCE
/// there (the current those rows give for another tap is not known).
pub const CS_SAFE: u16 = 0b111;

pub struct Path {
    pub name: &'static str,
    /// Dmm mode / range the row stands for (relay, PE, sentinel handling).
    pub mode: u8,
    pub range: u8,
    /// Stock nominal Rref (ohm, `dmm::OHM_RREF`); 0 = no series reference
    /// (current-source rows): no current computed.
    pub rref: f32,
    /// Jack volts per PB0 volt (SDIO divider 900 k / 100 k in the diode row).
    pub div: f32,
}

pub const PATHS: [Path; 7] = [
    Path { name: "R1K", mode: 10, range: 1, rref: 1e3, div: 1.0 },
    Path { name: "R10K", mode: 10, range: 2, rref: 1e4, div: 1.0 },
    Path { name: "R100K", mode: 10, range: 3, rref: 1e5, div: 1.0 },
    Path { name: "R1M", mode: 10, range: 4, rref: 1e6, div: 1.0 },
    Path { name: "R10M", mode: 10, range: 5, rref: 1e7, div: 1.0 },
    // f10_p8clr r6 (the stock 60 M range): PA2 on the power bus, SMODE
    // 0110 (SVDD + SCP + SCN, the MOS current source on RLD).
    Path { name: "CS", mode: 10, range: 6, rref: 0.0, div: 1.0 },
    // f12: PA1 / PA4 / PA5 on the power bus, SMODE 0110, SDIO.
    Path { name: "DIODE", mode: 12, range: 0, rref: 0.0, div: 10.0 },
];

/// Input-node choices: (name, SAD1FP code).
pub const SENSES: [(&str, u8); 4] = [("PB0", 8), ("OP1O", 3), ("SENSE", 0), ("RLU", 2)];

pub const N_PH: usize = 10;
const REF_N: u8 = 0x31; // SAD1RH PB6, SAD1RL AGND
const REF_W: u8 = 0x51; // VDDA, AGND
const REF_S: u8 = 0x55; // VDDA, VSSA
const FP_SENSE: u8 = 0xFF;
const FN_AGND: u8 = 3;
/// (SAD1FP, SAD1FN, R28) per phase.
const PHASES: [(u8, u8, u8); N_PH] = [
    (6, FN_AGND, REF_W),
    (6, FN_AGND, REF_S),
    (FP_SENSE, FN_AGND, REF_N),
    (FP_SENSE, FN_AGND, REF_W),
    (1, FN_AGND, REF_N),
    (1, FN_AGND, REF_W),
    (7, FN_AGND, REF_N),
    (7, FN_AGND, REF_W),
    (1, 1, REF_N),
    (11, FN_AGND, REF_N),
];
/// Conversions dropped after each phase switch.
pub const SKIP: u8 = 1;
const FS: f32 = 8_388_608.0;
/// PB6 / REFO, nominal (MAX6190).
pub const VREFO: f32 = 1.25;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Req {
    Off,
    /// vref (SFUVR code), path index, sense index, FORCE.
    On(u8, u8, u8, bool),
}

/// The stock row of `path` with tap `vref` and the AD1 bytes of phase 0.
pub fn build_row(vref: u8, path: u8, sense: u8) -> [u8; CFG_LEN] {
    let p = &PATHS[path as usize % PATHS.len()];
    let mut c = if p.mode == 12 { CFG_F12[0] } else { CFG_F10_P8CLR[p.range as usize] };
    c[17] = (c[17] & 0xF0) | (vref & 0x0F);
    c[3] &= !0x0C;
    let (r24, r28) = phase_regs(&c, 0, sense);
    c[4] = r24;
    c[8] = r28;
    c
}

/// R24 / R28 of phase `ph` for row `row` (keeps SDIO).
pub fn phase_regs(row: &[u8; CFG_LEN], ph: usize, sense: u8) -> (u8, u8) {
    let (fp, fnn, r28) = PHASES[ph % N_PH];
    let fp = if fp == FP_SENSE { SENSES[sense as usize % SENSES.len()].1 } else { fp };
    (fp << 4 | (row[4] & 0x08) | fnn, r28)
}

/// AD1 input gain of the row (R25 AD1IG).
pub fn ig_of(row: &[u8; CFG_LEN]) -> f32 {
    [0.9, 1.8, 2.7, 3.6][(row[5] >> 4) as usize & 3]
}

/// Whether `vref` may run on `path` (current-source rows: stock taps only
/// unless forced).
pub fn allowed(vref: u8, path: u8, force: bool) -> bool {
    let p = &PATHS[path as usize % PATHS.len()];
    vref < 16 && (p.rref != 0.0 || force || CS_SAFE & 1 << vref != 0)
}

/// Computed results of one complete phase cycle (V, relative to AGND
/// unless named otherwise; NaN where not available).
#[derive(Clone, Copy, Debug)]
pub struct Vals {
    pub v_in: f32,
    pub v_far: f32,
    pub i: f32,
    pub v_vref: f32,
    pub v_drop: f32,
    pub v_com: f32,
    pub vdda: f32,
    pub agnd_vss: f32,
}

fn narrow(code: i32, ig: f32) -> f32 {
    code as f32 / FS * VREFO / ig
}

/// A node from its narrow / wide pair.
fn node(cn: i32, cw: i32, ig: f32, w: f32) -> f32 {
    let vw = cw as f32 / FS * w / ig;
    if !w.is_finite() || vw.abs() < 0.9 * VREFO / ig { narrow(cn, ig) } else { vw }
}

pub fn compute(codes: &[i32; N_PH], ig: f32, path: u8, sense: u8) -> Vals {
    let p = &PATHS[path as usize % PATHS.len()];
    let ratio = |c: i32| if c != 0 { ig * VREFO * FS / c as f32 } else { f32::NAN };
    let w = ratio(codes[0]);
    let s = ratio(codes[1]);
    // The divider applies to the PB0 / OP1O reading of the jack.
    let div = if sense < 2 { p.div } else { 1.0 };
    let v_node = node(codes[2], codes[3], ig, w);
    let v_far = node(codes[4], codes[5], ig, w);
    // Phase 8 has only the narrow reference: saturated -> FB - <sense>.
    let v_drop = if (codes[8].unsigned_abs() as f32) < 0.97 * FS { narrow(codes[8], ig) } else { v_far - v_node };
    Vals {
        v_in: v_node * div,
        v_far,
        i: if p.rref > 0.0 { v_drop / p.rref } else { f32::NAN },
        v_vref: node(codes[6], codes[7], ig, w),
        v_drop,
        v_com: narrow(codes[9], ig),
        vdda: w,
        agnd_vss: s - w,
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

fn index(t: &[u8], names: &[&str], n: usize) -> Option<u8> {
    if let Some(i) = names.iter().position(|nm| eq_nc(t, nm)) {
        return Some(i as u8);
    }
    // VDSC<n> = VDS<n> (datasheet spelling).
    if t.len() > 4 && eq_nc(&t[..4], "VDSC") {
        let mut s = [0u8; 8];
        let k = t.len() - 1;
        if k <= s.len() {
            s[..3].copy_from_slice(b"VDS");
            s[3..k].copy_from_slice(&t[4..]);
            return index(&s[..k], names, n);
        }
    }
    if t.is_empty() || t.len() > 2 || !t.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let v = t.iter().fold(0usize, |a, &d| a * 10 + (d - b'0') as usize);
    (v < n).then_some(v as u8)
}

/// `DBG:SRC <vref>,<path>[,<sense>][,FORCE]` / `DBG:SRC OFF` argument.
pub fn parse(arg: &[u8]) -> Option<Req> {
    let arg = trim(arg);
    if eq_nc(arg, "OFF") || arg == b"0" {
        return Some(Req::Off);
    }
    let mut it = arg.split(|&b| b == b',').map(trim);
    let v = index(it.next()?, &VREF_NAMES, 16)?;
    let pnames: [&str; 7] = core::array::from_fn(|i| PATHS[i].name);
    let p = index(it.next()?, &pnames, PATHS.len())?;
    let snames: [&str; 4] = core::array::from_fn(|i| SENSES[i].0);
    let (mut s, mut force) = (0, false);
    for (k, t) in it.enumerate() {
        if eq_nc(t, "FORCE") {
            force = true;
        } else if k == 0 {
            s = index(t, &snames, SENSES.len())?;
        } else {
            return None;
        }
    }
    Some(Req::On(v, p, s, force))
}

/// The phase cycle (Dmm glue keeps one).
#[derive(Clone, Copy)]
pub struct Cycle {
    pub phase: u8,
    pub skip: u8,
    /// Complete cycles since the row was written.
    pub n: u32,
    pub codes: [i32; N_PH],
    /// The last complete cycle.
    pub done: [i32; N_PH],
}

impl Cycle {
    pub const NEW: Cycle = Cycle { phase: 0, skip: SKIP, n: 0, codes: [0; N_PH], done: [0; N_PH] };

    /// One AD1 conversion. Returns the (R23 with ENAD1 off, R24, R28) of the
    /// next phase when the phase advanced (the caller writes them, then R23
    /// = row[3]).
    pub fn step(&mut self, code: i32, row: &[u8; CFG_LEN], sense: u8) -> Option<(u8, u8)> {
        if self.skip > 0 {
            self.skip -= 1;
            return None;
        }
        let ph = self.phase as usize % N_PH;
        self.codes[ph] = code;
        let next = (ph + 1) % N_PH;
        if next == 0 {
            self.done = self.codes;
            self.n = self.n.wrapping_add(1);
        }
        self.phase = next as u8;
        self.skip = SKIP;
        Some(phase_regs(row, next, sense))
    }
}
