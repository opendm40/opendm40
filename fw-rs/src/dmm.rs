//! Multimeter engine: a port of the stock measurement behaviour.
//! Sources: firmware/re/analysis/hy3131_v2_6.h (bus, config rows, calibration
//! layout) and dmm_v2_6.h (per-mode formulas, filters, settle deadlines,
//! autorange, display formats). Stock field names in comments (m+x, S+x).
//!
//! Modes (HY3131 config tables): 0 ACV, 1 DCV, 2 ACmV, 3 DCmV, 4..6 AC A/mA/uA,
//! 7..9 DC A/mA/uA, 10 ohm, 11 continuity, 12 diode, 13 capacitance,
//! 14 frequency, 15 temperature. AC+DC is not offered (DM40A has none).

use core::ptr::read_volatile;

use crate::clock::Clock;
use crate::hy3131 as hy;
use crate::hy3131_tables::*;
use crate::mmio::{modify32, w32};

const GPIOA: u32 = 0x4002_0000;
const GPIOC: u32 = 0x4002_0800;
const GPIOE: u32 = 0x4002_1000;

/// Settings record in internal flash (read only): u16 crc, u16 ~crc,
/// u32 version = 1, data. DMM calibration at record offset 0x168.
const SETTINGS: u32 = 0x080C_0000;
const CAL: u32 = SETTINGS + 0x168;

const FACTOR: [f32; 3] = [4.0, 5.0, 6.0];
const OVER: [f32; 3] = [1.05, 1.04, 1.03];
const AD1_DIV: f32 = 8_388_606.0;
const K_ACV: [f32; 4] = [10.0, 100.0, 1000.0, 10000.0];
const MV_DIV: [f32; 2] = [10.0, 1.0]; // also the A op gain
const DEC4: [f32; 4] = [1.0, 10.0, 100.0, 1000.0];
const SHUNT: [f32; 3] = [0.01, 0.0005, 4.75e-5]; // ohm / kohm / Mohm
const OHM_RREF: [f32; 7] = [0.1, 1.0, 10.0, 100.0, 1000.0, 10000.0, 63000.0];
const OHM_DEC: [f32; 7] = [0.01, 0.1, 1.0, 10.0, 100.0, 1000.0, 10000.0];
const CAP_DEC: [f32; 8] = [0.001, 0.01, 0.1, 1.0, 10.0, 100.0, 1000.0, 10000.0];
const CAP_WIN: [f32; 8] = [0.0008, 0.0002, 0.0001, 0.0001, 0.0001, 3.0, 30.0, 200.0];
const CAP_K: [f32; 8] = [1020.0, 1003.0, 102.7, 10.68, 1.5, 4.06, 41.0, 400.0];
const CAP_R21: [u8; 3] = [0xDC, 0xCC, 0xBB];
const FREQ_DEC: [f32; 7] = [0.01, 0.1, 1.0, 10.0, 100.0, 1000.0, 10000.0];
const ACV_WIN: [f32; 4] = [0.002, 0.01, 0.1, 1.0];
const ACMV_WIN: [f32; 2] = [0.01, 0.1];
const DCV_NEGK: [f32; 4] = [1.0, 1.00008, 1.00011, 1.0015];
const ACA_WIN: [f32; 6] = [0.001, 0.01, 0.01, 0.1, 0.1, 1.0];
const TC_K_MV: [f32; 16] = [
    -6.942, -4.8, -1.051, 3.044, 7.087, 11.157, 15.346, 19.592, 23.854, 28.077, 32.223, 36.274, 40.224,
    44.067, 47.786, 51.358,
];
const CFG_F13_R7: [u8; hy::CFG_LEN] = [
    0x52, 0xbb, 0x14, 0x83, 0x43, 0x40, 0x00, 0x5d, 0x31, 0x00, 0x00, 0x00, 0x88, 0x08, 0x00, 0x8e, 0x80, 0xc2, 0xcc,
    0xa8, 0x00, 0x54,
];
const CFG_F13_DISCHARGE: [u8; hy::CFG_LEN] = [
    0x52, 0xdd, 0x13, 0x83, 0x43, 0x40, 0x00, 0x5d, 0x31, 0x00, 0x00, 0x00, 0x88, 0x88, 0x00, 0x8e, 0x80, 0xc2, 0xcc,
    0xa8, 0x00, 0x54,
];

/// The six bottom buttons of the stock screen.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Volt,
    Amp,
    Ohm,
    Cap,
    ContDiode,
    HzTemp,
}

impl Button {
    pub const ALL: [Button; 6] = [Button::Volt, Button::Amp, Button::Ohm, Button::Cap, Button::ContDiode, Button::HzTemp];
}

/// A formatted readout: value in display units, digits, unit text, OL.
#[derive(Clone, Copy)]
pub struct Val {
    pub v: f32,
    pub dec: u8,
    pub unit: &'static str,
    pub ol: bool,
}

impl Val {
    const fn new(v: f32, dec: u8, unit: &'static str) -> Self {
        Val { v, dec, unit, ol: false }
    }
    const fn ol(dec: u8, unit: &'static str) -> Self {
        Val { v: 0.0, dec, unit, ol: true }
    }
}

#[derive(Clone, Copy)]
pub struct Reading {
    pub mode: u8,
    pub range: i8,
    pub main: Val,
    pub aux1: Option<Val>,
    pub aux2: Option<Val>,
    /// Main value in base SI units (V, A, ohm, F, Hz, C) for SCPI / BT.
    pub si: f32,
    pub auto: bool,
    /// Range label in auto: "AUTO+" instead of "AUTO".
    pub auto_plus: bool,
    pub seq: u32,
}

impl Reading {
    pub fn button(&self) -> Button {
        button_of(self.mode)
    }
    pub fn is_ac(&self) -> bool {
        matches!(self.mode, 0 | 2 | 4 | 5 | 6)
    }
}

pub fn button_of(mode: u8) -> Button {
    match mode {
        0..=3 => Button::Volt,
        4..=9 => Button::Amp,
        10 => Button::Ohm,
        11 | 12 => Button::ContDiode,
        13 => Button::Cap,
        _ => Button::HzTemp,
    }
}

fn cal_ok() -> bool {
    (unsafe { read_volatile((SETTINGS + 4) as *const u32) }) == 1
}

/// Calibration float at struct offset `off`, `dflt` if missing or not sane.
fn cal(off: u32, dflt: f32) -> f32 {
    let v = unsafe { read_volatile((CAL + off) as *const f32) };
    if cal_ok() && v.is_finite() && v.abs() < 1.0e6 { v } else { dflt }
}

/// FUN_080160c6: EMA that jumps (0.99) when the step exceeds `thr`.
fn ema(thr: f32, a: f32, x: f32, y: &mut f32) {
    let d = x - *y;
    let k = if d.abs() < thr { a } else { 0.99 };
    *y += k * d;
}

fn sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut y = f32::from_bits((x.to_bits() >> 1) + 0x1FBD_1DF5);
    for _ in 0..3 {
        y = 0.5 * (y + x / y);
    }
    y
}

fn u24(b: &[u8]) -> u32 {
    b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16
}

fn relay_pa(clk: &mut Clock, arg: u8) {
    // Latching relay pair: 10 ms pulse. 0: PA10=1 PA9=0, 1: PA10=0 PA9=1.
    let (set, clr) = if arg == 0 { (10, 9) } else { (9, 10) };
    unsafe {
        w32(GPIOA + 0x18, (1 << set) | (1 << (clr + 16)));
        modify32(GPIOA + 0x00, (3 << 18) | (3 << 20), (1 << 18) | (1 << 20));
    }
    clk.delay_ms(10);
    unsafe { w32(GPIOA + 0x18, (1 << (9 + 16)) | (1 << (10 + 16))) };
}

fn relay_pe(arg: u8) {
    // Shunt select: 0: PE0=1 PE1=0, 1: PE0=0 PE1=1, 2: both 0.
    let bop = match arg {
        0 => (1 << 0) | (1 << (1 + 16)),
        1 => (1 << (0 + 16)) | (1 << 1),
        _ => (1 << (0 + 16)) | (1 << (1 + 16)),
    };
    unsafe {
        w32(GPIOE + 0x18, bop);
        modify32(GPIOE + 0x00, 3 | (3 << 2), 1 | (1 << 2));
    }
}

fn pc7(high: bool) {
    unsafe {
        w32(GPIOC + 0x18, if high { 1 << 7 } else { 1 << (7 + 16) });
        modify32(GPIOC + 0x00, 3 << 14, 1 << 14);
    }
}

/// Frequency counter state (FUN_08013450).
struct Freq {
    next: u64,
    src_alt: bool,
    last: [f32; 2],
    change: [f32; 2],
    f_khz: f32,
    duty: f32,
    range: u8,
}

/// Capacitance ramp state (r5..r7).
struct CapRamp {
    discharging: bool,
    prev_v: f32,
    prev_t: u64,
    med: [f32; 5],
    med_i: usize,
    class: u8,
    buf: [f32; 35],
    n: usize,
    cmp: Option<bool>,
    first: bool,
}

pub struct Dmm {
    model: u8,
    mode: u8,
    range: i8,
    manual: bool,
    /// m+6: V autorange (bit0 mV->V, bit1 V->mV); m+7: A (1 mA..A, 3 uA..A).
    v_auto: u8,
    a_auto: u8,
    /// m+8: ohm variant (selects f10_p8set when 1).
    ohm_var: u8,
    /// S+0x40: no reading accepted before this time.
    deadline: u64,
    entry: u64,
    prev_ad1: i32,
    r22: u8,
    ohm_settled: bool,
    f_ac: f32,
    f_lpf: f32,
    f_dc: f32,
    f_diode: f32,
    f_cap: f32,
    cap_c: f32,
    cap_cnt_up: u8,
    cap_cnt_dn: u8,
    cap_cnt_small: u8,
    ramp: CapRamp,
    beep_class: u8,
    beep_e: u8,
    beep_next: u64,
    beep_n: u8,
    t_phase: u8,
    t_code_a: i32,
    t_cj: f32,
    freq: Freq,
    seq: u32,
    pa_now: Option<u8>,
}

impl Dmm {
    pub fn new() -> Self {
        hy::init_pins();
        let model = unsafe { read_volatile(CAL as *const u8) }.min(2);
        Dmm {
            model,
            mode: 1,
            range: 0,
            manual: false,
            v_auto: 3,
            a_auto: 3,
            ohm_var: 0,
            deadline: 0,
            entry: 0,
            prev_ad1: 0,
            r22: 0,
            ohm_settled: false,
            f_ac: 0.0,
            f_lpf: 0.0,
            f_dc: 0.0,
            f_diode: 0.0,
            f_cap: 0.0,
            cap_c: 0.0,
            cap_cnt_up: 0,
            cap_cnt_dn: 0,
            cap_cnt_small: 0,
            ramp: CapRamp {
                discharging: false,
                prev_v: 0.0,
                prev_t: 0,
                med: [0.0; 5],
                med_i: 0,
                class: 0,
                buf: [0.0; 35],
                n: 0,
                cmp: None,
                first: true,
            },
            beep_class: 4,
            beep_e: 0,
            beep_next: 0,
            beep_n: 0,
            t_phase: 0,
            t_code_a: 0,
            t_cj: 0.0,
            freq: Freq { next: 0, src_alt: false, last: [0.0; 2], change: [0.0; 2], f_khz: 0.0, duty: 0.0, range: 0 },
            seq: 0,
            pa_now: None,
        }
    }

    pub fn model(&self) -> u8 {
        self.model
    }
    pub fn mode(&self) -> u8 {
        self.mode
    }
    pub fn range(&self) -> i8 {
        self.range
    }
    pub fn button(&self) -> Button {
        button_of(self.mode)
    }
    pub fn is_auto(&self) -> bool {
        !self.manual
    }
    pub fn v_auto(&self) -> u8 {
        self.v_auto
    }
    pub fn a_auto(&self) -> u8 {
        self.a_auto
    }
    pub fn ohm_var(&self) -> u8 {
        self.ohm_var
    }
    fn factor(&self) -> f32 {
        FACTOR[self.model as usize]
    }
    fn over(&self) -> f32 {
        OVER[self.model as usize]
    }

    // ---------------------------------------------------------------- setup

    /// FUN_08012c30: quiesce, relays, 22 config bytes, INTE = 0.
    fn write_config(&mut self, clk: &mut Clock) {
        let m = self.mode;
        let r = self.range.max(0) as usize;
        let cfg: &[u8; hy::CFG_LEN] = match m {
            0 => &CFG_F0[r.min(3)],
            1 => &CFG_F1[r.min(3)],
            2 => &CFG_F2[r.min(1)],
            3 => &CFG_F3[r.min(1)],
            4 => &CFG_F4[r.min(1)],
            5 | 6 => &CFG_F5_6[r.min(1)],
            7 => &CFG_F7[r.min(1)],
            8 | 9 => &CFG_F8_9[r.min(1)],
            10 => {
                if self.ohm_var == 1 { &CFG_F10_P8SET[r.min(6)] } else { &CFG_F10_P8CLR[r.min(7)] }
            }
            11 => &CFG_F11[0],
            12 => &CFG_F12[0],
            13 => {
                if r == 7 { &CFG_F13_R7 } else { &CFG_F13[r.min(6)] }
            }
            14 => &CFG_F14[0],
            _ => &CFG_F15[0],
        };
        hy::quiesce();
        let pa = if matches!(m, 0 | 1 | 4..=9) { 0 } else { 1 };
        if self.pa_now != Some(pa) {
            relay_pa(clk, pa);
            self.pa_now = Some(pa);
        }
        relay_pe(match m {
            4 | 7 => 0,
            5 | 8 => 1,
            _ => 2,
        });
        pc7(m == 14);
        hy::configure(cfg);
        self.r22 = cfg[2];
    }

    fn set(&mut self, clk: &mut Clock, mode: u8, range: i8, settle: u64) {
        let changed_mode = mode != self.mode;
        self.mode = mode;
        self.range = range;
        self.write_config(clk);
        let now = clk.now_ms();
        self.deadline = now + settle;
        if changed_mode {
            self.entry = now;
            self.reset_values();
        }
    }

    fn reset_values(&mut self) {
        self.f_ac = 0.0;
        self.f_lpf = 0.0;
        self.f_dc = 0.0;
        self.f_diode = 0.0;
        self.f_cap = 0.0;
        self.cap_c = 0.0;
        self.ohm_settled = false;
        self.beep_class = 4;
        self.t_phase = 0;
        self.freq.f_khz = 0.0;
        self.freq.duty = 0.0;
        self.freq.range = 0;
        self.ramp.first = true;
    }

    /// Enter a stock mode directly (SCPI, BT command 06). Auto range.
    pub fn select(&mut self, clk: &mut Clock, mode: u8) {
        self.manual = false;
        let (r, settle) = match mode {
            10 => (6, 2500),
            11 => (0, 200),
            12 => (0, 100),
            13 => (0, 200),
            14 => (0, 1000),
            15 => (0, 500),
            _ => (0, 450),
        };
        self.set(clk, mode.min(15), r, settle);
    }

    /// Bottom button press (FUN_0803e108..e1a8): enter the function, or cycle
    /// its sub-modes when it is already active.
    pub fn press(&mut self, clk: &mut Clock, b: Button) {
        let same = self.button() == b;
        self.manual = false;
        match b {
            Button::Volt => {
                let m = if same && matches!(self.mode, 1 | 3) { 0 } else { 1 };
                self.set(clk, m, 0, 450);
            }
            Button::Amp => {
                let m = if same && matches!(self.mode, 7..=9) { 5 } else { 8 };
                self.set(clk, m, 0, 650);
            }
            Button::Ohm => {
                if same {
                    self.ohm_var ^= 1;
                    let r = self.range.clamp(1, 6);
                    self.set(clk, 10, r, 300);
                } else {
                    self.set(clk, 10, 6, 2500);
                }
            }
            Button::Cap => self.set(clk, 13, 0, 200),
            Button::ContDiode => {
                let m = if same && self.mode == 12 { 11 } else { 12 };
                self.set(clk, m, 0, if m == 11 { 200 } else { 100 });
            }
            Button::HzTemp => {
                let m = if same && self.mode == 14 { 15 } else { 14 };
                self.set(clk, m, 0, if m == 14 { 1000 } else { 500 });
            }
        }
    }

    /// AUTO key (FUN_0803ddf4).
    pub fn auto_key(&mut self, clk: &mut Clock) {
        self.manual = false;
        match self.mode {
            2 | 3 if self.v_auto == 1 => self.set(clk, self.mode - 2, 0, 350),
            9 => self.set(clk, 8, 0, 350),
            6 => self.set(clk, 5, 0, 650),
            _ => {}
        }
    }

    /// Freeze the current range (REL does this) or release it.
    pub fn hold_range(&mut self, manual: bool) {
        self.manual = manual;
    }

    /// Manual range from the MF popup: V items 400 mV, 4, 40, 400, 1000 V;
    /// A items 400 uA, 4000 uA, 40 mA, 400 mA, 4 A, 10 A; ohm 1..6.
    /// `None` = AUTO, `Some(255)` = AUTO+ (V/A only).
    pub fn set_range(&mut self, clk: &mut Clock, item: Option<u8>) {
        match (self.button(), item) {
            (Button::Volt, None) => {
                self.manual = false;
                self.v_auto = 3;
            }
            (Button::Volt, Some(255)) => {
                self.manual = false;
                self.v_auto = 1;
            }
            (Button::Amp, None) => {
                self.manual = false;
                self.a_auto = 3;
            }
            (Button::Amp, Some(255)) => {
                self.manual = false;
                self.a_auto = 1;
            }
            (Button::Volt, Some(i)) if i <= 4 => {
                let ac = matches!(self.mode, 0 | 2);
                self.manual = true;
                if i == 0 {
                    self.set(clk, if ac { 2 } else { 3 }, 1, 450);
                } else {
                    self.set(clk, if ac { 0 } else { 1 }, i as i8 - 1, 450);
                }
            }
            (Button::Amp, Some(i)) if i <= 5 => {
                let ac = matches!(self.mode, 4..=6);
                let unit = 2 - (i / 2); // 0 A, 1 mA, 2 uA
                let m = if ac { 4 } else { 7 } + unit;
                self.manual = true;
                self.set(clk, m, (i % 2) as i8, 650);
            }
            (Button::Ohm, None) => self.manual = false,
            (Button::Ohm, Some(i)) if (1..=6).contains(&i) => {
                self.manual = true;
                self.set(clk, 10, i as i8, 150);
            }
            _ => {}
        }
    }

    /// BT mode byte (bt_meas_v2_6.h section 2, forward FUN_0803c842):
    /// bits 0-2 group, 3-5 range field, 6-7 sub-mode.
    pub fn mode_byte(&self) -> u8 {
        let m = self.mode;
        match m {
            0..=3 => {
                let sub = matches!(m, 0 | 2) as u8;
                let r = if self.manual {
                    if m >= 2 { 0 } else { self.range as u8 + 1 }
                } else if self.v_auto == 1 {
                    6
                } else {
                    5
                };
                sub << 6 | r << 3
            }
            4..=9 => {
                let sub = (m <= 6) as u8;
                let unit = (m - 4) % 3; // 0 A, 1 mA, 2 uA
                let r = if self.manual {
                    [4, 2, 0][unit as usize] + self.range as u8
                } else if self.a_auto == 1 {
                    7
                } else {
                    6
                };
                sub << 6 | r << 3 | 1
            }
            10 => {
                let r = if self.manual { self.range as u8 - 1 } else { 6 };
                self.ohm_var << 6 | r << 3 | 2
            }
            11 => 0x44,
            12 => 0x04,
            13 => 0x03,
            14 => 0x05,
            _ => 0x45,
        }
    }

    /// BT command 06 (reverse, FUN_0803c6a8 + FUN_0803c3b4). Returns true if
    /// the function or sub-mode changed (the caller then clears HOLD/REL).
    pub fn apply_mode_byte(&mut self, clk: &mut Clock, b: u8) -> bool {
        let (g, r, s) = (b & 7, (b >> 3) & 7, b >> 6);
        let before = (self.button(), self.mode_byte() & 0xC0);
        match g {
            0 => {
                if self.button() != Button::Volt {
                    self.select(clk, 1);
                }
                let ac = s == 1;
                if r <= 4 {
                    self.manual = true;
                    if r == 0 {
                        self.set(clk, if ac { 2 } else { 3 }, 1, 450);
                    } else {
                        self.set(clk, if ac { 0 } else { 1 }, r as i8 - 1, 450);
                    }
                } else {
                    self.manual = false;
                    if r == 5 {
                        self.v_auto = 3;
                    } else if r == 6 {
                        self.v_auto = 1;
                    }
                    let mv = self.mode >= 2;
                    let (m, rr) = if r == 6 && mv { (0, 0) } else { ((mv as u8) * 2, self.range) };
                    let m = m + (!ac) as u8;
                    if m != self.mode {
                        self.set(clk, m, rr, 450);
                    }
                }
            }
            1 => {
                if self.button() != Button::Amp {
                    self.select(clk, 8);
                }
                let ac = s == 1;
                if r <= 5 {
                    self.manual = true;
                    let m = if ac { 6 } else { 9 } - r / 2;
                    self.set(clk, m, (r % 2) as i8, 650);
                } else {
                    self.manual = false;
                    if r == 6 {
                        self.a_auto = 3;
                    } else {
                        self.a_auto = 1;
                    }
                    let mut unit = (self.mode - 4) % 3;
                    let mut rr = self.range;
                    if r == 7 && unit == 2 {
                        unit = 1;
                        rr = 0;
                    }
                    let m = if ac { 4 } else { 7 } + unit;
                    if m != self.mode || rr != self.range {
                        self.set(clk, m, rr, 650);
                    }
                }
            }
            2 => {
                if self.mode != 10 {
                    self.select(clk, 10);
                }
                self.ohm_var = s & 1;
                if r == 6 {
                    self.manual = false;
                    let rr = self.range;
                    self.set(clk, 10, rr, 300);
                } else {
                    self.manual = true;
                    self.set(clk, 10, (r as i8 + 1).min(6), 150);
                }
            }
            3 => self.select(clk, 13),
            4 => self.select(clk, if s == 1 { 11 } else { 12 }),
            5 => self.select(clk, if s == 1 { 15 } else { 14 }),
            _ => {}
        }
        before != (self.button(), self.mode_byte() & 0xC0)
    }

    /// Anti-drop: pulse the latching V/A relay again into its current state.
    pub fn reassert_relay(&mut self, clk: &mut Clock) {
        if let Some(pa) = self.pa_now {
            relay_pa(clk, pa);
        }
    }

    /// (Re)start measuring in the current mode.
    pub fn start(&mut self, clk: &mut Clock) {
        let (m, r) = (self.mode, self.range);
        self.mode = 255;
        self.set(clk, m, r, 450);
    }

    // ----------------------------------------------------------------- poll

    /// Poll the converter (every ~10 ms). Returns a reading when one is ready.
    pub fn poll(&mut self, clk: &mut Clock) -> Option<Reading> {
        let now = clk.now_ms();
        self.diode_beep(now);
        if now < self.deadline {
            return None;
        }
        let r = match self.mode {
            0 | 2 | 4..=6 => self.poll_ac(clk, now),
            1 | 3 => self.poll_dcv(clk),
            7..=9 => self.poll_dca(clk),
            10 => self.poll_ohm(clk, now),
            11 => self.poll_cont(),
            12 => self.poll_diode(now),
            13 => self.poll_cap(clk, now),
            14 => self.poll_freq_mode(now),
            _ => self.poll_temp(now),
        };
        r.map(|mut r| {
            self.seq = self.seq.wrapping_add(1);
            r.seq = self.seq;
            r.auto = !self.manual;
            r.auto_plus = (self.button() == Button::Volt && self.v_auto == 1)
                || (self.button() == Button::Amp && self.a_auto == 1);
            r
        })
    }

    fn reading(&self, main: Val, si: f32) -> Reading {
        Reading { mode: self.mode, range: self.range, main, aux1: None, aux2: None, si, auto: true, auto_plus: false, seq: 0 }
    }

    fn ad1(&mut self) -> Option<i32> {
        if hy::read1(hy::R_INTF) & hy::INTF_AD1 == 0 {
            return None;
        }
        let mut b = [0u8; 3];
        hy::read(hy::R_AD1, &mut b);
        Some(hy::s24(&b))
    }

    /// AD1 OSR switching: stable -> 0x17 (32768, written once), else `fast`.
    fn osr(&mut self, ad1: i32, thr: i32, fast: u8) {
        if (ad1 - self.prev_ad1).abs() <= thr {
            if self.r22 != 0x17 {
                hy::write(0x22, &[0x17]);
                self.r22 = 0x17;
            }
        } else {
            hy::write(0x22, &[fast]);
            self.r22 = fast;
        }
        self.prev_ad1 = ad1;
    }

    fn poll_dcv(&mut self, clk: &mut Clock) -> Option<Reading> {
        let ad1 = self.ad1()?;
        let r = self.range as usize;
        let (f, ov) = (self.factor(), self.over());
        if self.mode == 1 {
            self.osr(ad1, 0x206, 0x15);
            let mut v = ad1 as f32 * K_ACV[r] * (1.25 / 0.9) / AD1_DIV * cal(0x04 + 4 * r as u32, 1.0)
                - cal(0x14 + 4 * r as u32, 0.0);
            if v < 0.0 {
                v *= DCV_NEGK[r];
            }
            if r == 0 && v.abs() / 20000.0 > 3e-5 {
                v += 1e-4 - v.abs() / 20000.0;
            }
            let lim = (f * DEC4[r]).min(1000.0);
            let ol = v.abs() / lim > ov;
            if !self.manual {
                if ol && r < 3 {
                    self.set(clk, 1, r as i8 + 1, 250);
                } else if v.abs() < 0.1 * lim / ov {
                    if r > 0 {
                        self.set(clk, 1, r as i8 - 1, 250);
                    } else if self.v_auto & 2 != 0 {
                        self.set(clk, 3, 1, 450);
                    }
                }
            }
            let main = if ol { Val::ol(4 - r as u8, "V") } else { Val::new(v, 4 - r as u8, "V") };
            Some(self.reading(main, v))
        } else {
            self.osr(ad1, 0xE22 - 0x960 * r as i32, 0x15);
            let mv = ad1 as f32 * (1.25 / 0.9) / MV_DIV[r] / (AD1_DIV / 1000.0) * cal(0x24 + 4 * r as u32, 1.0)
                - cal(0x2C + 4 * r as u32, 0.0);
            let lim = f * [10.0, 100.0][r];
            let ol = mv.abs() / lim > ov;
            if !self.manual && ol {
                if r == 0 {
                    self.set(clk, 3, 1, 250);
                } else if self.v_auto & 1 != 0 {
                    self.set(clk, 1, 0, 350);
                }
            }
            let main = if ol { Val::ol(3 - r as u8, "mV") } else { Val::new(mv, 3 - r as u8, "mV") };
            Some(self.reading(main, mv * 1e-3))
        }
    }

    fn poll_dca(&mut self, clk: &mut Clock) -> Option<Reading> {
        let ad1 = self.ad1()?;
        self.osr(ad1, 0x168, 0x15);
        let (m, r) = (self.mode, self.range as usize);
        let u = (m - 7) as usize; // 0 A, 1 mA, 2 uA
        let i = u * 2 + r;
        let mut a = ad1 as f32 * 1.25 / (SHUNT[u] * MV_DIV[r] * 0.9) / AD1_DIV * cal(0x94 + 4 * i as u32, 1.0)
            - cal(0xAC + 4 * i as u32, 0.0);
        if m == 7 && r == 1 {
            ema(0.0007, 0.15, a, &mut self.f_dc);
            a = self.f_dc;
        }
        let lim = (self.factor() * DEC4[u + r]).min(if m == 7 { 10.0 } else { f32::MAX });
        let ov = self.over();
        let ol = a.abs() / lim > ov;
        if !self.manual {
            if ol {
                if r == 0 {
                    self.set(clk, m, 1, 250);
                } else if m > 7 && self.a_auto & 1 != 0 {
                    self.set(clk, m - 1, 1, 250);
                }
            } else if a.abs() < 0.1 * lim / ov {
                if r == 1 {
                    self.set(clk, m, 0, 250);
                } else if m < if self.a_auto == 3 { 9 } else { 8 } {
                    self.set(clk, m + 1, 0, 350);
                }
            }
        }
        let (unit, dec, si) = match u {
            0 => ("A", 4 - r as u8, a),
            1 => ("mA", 3 - r as u8, a * 1e-3),
            _ => ("\u{b5}A", 2 - r as u8, a * 1e-6),
        };
        let mut rd = self.reading(if ol { Val::ol(dec, unit) } else { Val::new(a, dec, unit) }, si);
        if m == 8 {
            // 4-20 mA loop percentage.
            let p = (a.abs() - 4.0) * 6.25;
            rd.aux2 = Some(if p > 125.0 { Val::ol(1, "%") } else { Val::new(p, 1, "%") });
        }
        Some(rd)
    }

    fn poll_ac(&mut self, clk: &mut Clock, now: u64) -> Option<Reading> {
        let m = self.mode;
        let need = if m <= 2 { hy::INTF_RMS | hy::INTF_LPF } else { hy::INTF_RMS };
        if hy::read1(hy::R_INTF) & need != need {
            return None;
        }
        let mut b = [0u8; 20];
        hy::read(hy::R_AD1, &mut b);
        let rms = (b[9] as u64) | (b[10] as u64) << 8 | (b[11] as u64) << 16 | (b[12] as u64) << 24
            | ((b[13] & 0x1F) as u64) << 32;
        let sq = sqrt(rms as f32);
        if sq > 262_144.0 {
            return None;
        }
        let lpf = (((b[6] as u32) << 13 | (b[7] as u32) << 21 | (b[8] as u32) << 29) as i32 >> 13) as f32;
        let r = self.range as usize;
        let (f, ov) = (self.factor(), self.over());
        let (s, ci, g_rms, o_rms, g_lpf, o_lpf) = match m {
            0 => (K_ACV[r] * 1.25 / 262_144.0, r, 0x34, 0x44, 0x64, 0x74),
            2 => (1.25 / MV_DIV[r] / 262.144, r, 0x54, 0x5C, 0x84, 0x8C),
            _ => {
                let u = (m - 4) as usize;
                (1.25 / (SHUNT[u] * MV_DIV[r]) / 262_144.0, u * 2 + r, 0xC4, 0xDC, 0xF4, 0x10C)
            }
        };
        let ci = ci as u32;
        let mut ac = sq * s * cal(g_rms + 4 * ci, 1.0) - cal(o_rms + 4 * ci, 0.0);
        let dcp = lpf * s * cal(g_lpf + 4 * ci, 1.0) - cal(o_lpf + 4 * ci, 0.0);
        if ac < 0.0 {
            ac = 0.0;
        }
        let (thr, a_rms, a_lpf) = match m {
            0 => (ACV_WIN[r], 0.25, 0.25),
            2 => (ACMV_WIN[r], 0.35, 0.35),
            _ => (ACA_WIN[(m - 4) as usize * 2 + r], 0.15, 0.35),
        };
        ema(thr, a_rms, ac, &mut self.f_ac);
        ema(thr, a_lpf, dcp, &mut self.f_lpf);

        let mag = if m <= 2 { ac } else { sqrt(ac * ac + 0.25 * dcp * dcp) };
        let lim = match m {
            0 => (f * DEC4[r]).min(1000.0),
            2 => f * [10.0, 100.0][r],
            _ => (f * DEC4[(m - 4) as usize + r]).min(if m == 4 { 10.0 } else { f32::MAX }),
        };
        let ol = self.f_ac / lim > ov;
        if !self.manual {
            let up = mag > ov * lim;
            let down = mag < 0.1 * lim / ov;
            match m {
                0 => {
                    if up && r < 3 {
                        self.set(clk, 0, r as i8 + 1, 450);
                    } else if down && r > 0 {
                        self.set(clk, 0, r as i8 - 1, 450);
                    } else if down && self.v_auto & 2 != 0 {
                        self.set(clk, 2, 1, 600);
                    }
                }
                2 => {
                    if up && r == 0 {
                        self.set(clk, 2, 1, 450);
                    } else if up && self.v_auto & 1 != 0 {
                        self.set(clk, 0, 0, 350);
                    }
                }
                _ => {
                    if up && r == 0 {
                        self.set(clk, m, 1, 550);
                    } else if up && m > 4 && self.a_auto & 1 != 0 {
                        self.set(clk, m - 1, 0, 650);
                    } else if down && r == 1 {
                        self.set(clk, m, 0, 550);
                    } else if down && r == 0 && m < if self.a_auto == 3 { 6 } else { 5 } {
                        self.set(clk, m + 1, 1, 650);
                    }
                }
            }
        }
        let (unit, dec, si_k) = match m {
            0 => ("V", 4 - r as u8, 1.0),
            2 => ("mV", 3 - r as u8, 1e-3),
            4 => ("A", 4 - r as u8, 1.0),
            5 => ("mA", 3 - r as u8, 1e-3),
            _ => ("\u{b5}A", 2 - r as u8, 1e-6),
        };
        let v = self.f_ac;
        let mut rd = self.reading(if ol { Val::ol(dec, unit) } else { Val::new(v, dec, unit) }, v * si_k);
        self.freq_secondary(now, 0xF0);
        let (fv, dv) = self.freq_vals(true);
        rd.aux1 = Some(fv);
        rd.aux2 = Some(dv);
        Some(rd)
    }

    fn poll_ohm(&mut self, clk: &mut Clock, now: u64) -> Option<Reading> {
        let ad1 = self.ad1()?;
        let r = self.range.clamp(1, 6) as usize;
        let mut kohm = (ad1.unsigned_abs() as f32) * cal(0x124 + 4 * r as u32, 1.0) * OHM_RREF[r] / 0.9 / AD1_DIV
            - cal(0x144, 0.0);
        if kohm < 0.0 {
            kohm = 0.0;
        }
        let lim = self.factor() * OHM_DEC[r];
        let ov = self.over();
        let ol = kohm / lim > ov;
        let r22 = hy::read1(0x22);
        if ol {
            if r22 != 0x13 {
                hy::write(0x22, &[0x13]);
            }
        } else if r22 != 0x17 {
            hy::write(0x22, &[0x17]);
            self.deadline = now + 150;
        }
        if !self.manual {
            let q = ov * lim / (kohm + 1e-5);
            if q < 1.0 {
                if r < 6 {
                    let to = if self.ohm_settled { 6 } else { r as i8 + 1 };
                    self.ohm_settled = false;
                    self.set(clk, 10, to, 150);
                    return None;
                }
            } else if q > 10.0 && r > 1 {
                let step = if now < self.entry + 2500 {
                    1
                } else if q > 1e5 {
                    5
                } else if q > 1e4 {
                    4
                } else if q > 1e3 {
                    3
                } else if q > 100.0 {
                    2
                } else {
                    1
                };
                self.ohm_settled = false;
                self.set(clk, 10, (r as i8 - step).max(1), 150);
                return None;
            } else if !self.ohm_settled {
                self.ohm_settled = true;
                self.deadline = now + 150;
                return None;
            }
        }
        let (v, dec, unit) = match r {
            1 => (kohm * 1000.0, 2, "\u{3a9}"),
            2..=4 => (kohm, 6 - r as u8, "k\u{3a9}"),
            _ => (kohm * 1e-3, 9 - r as u8, "M\u{3a9}"),
        };
        Some(self.reading(if ol { Val::ol(dec, unit) } else { Val::new(v, dec, unit) }, kohm * 1e3))
    }

    fn poll_cont(&mut self) -> Option<Reading> {
        let ad1 = self.ad1()?;
        self.osr(ad1, 0x206, 0x14);
        let ohm = -(ad1 as f32) * cal(0x15C, 1.0) / 0.9 / (AD1_DIV / 1000.0) - cal(0x160, 0.0);
        let ol = ohm > 1000.0;
        Some(self.reading(if ol { Val::ol(2, "\u{3a9}") } else { Val::new(ohm, 2, "\u{3a9}") }, ohm))
    }

    fn poll_diode(&mut self, now: u64) -> Option<Reading> {
        let ad1 = self.ad1()?;
        let g = cal(0x164, 1.0);
        let mut v = ad1 as f32 * g * 1.25 * 10.0 / 0.9 / AD1_DIV - cal(0x168, 0.0);
        if v < 0.0 {
            v = 0.0;
        }
        ema(0.0012, 0.05, v, &mut self.f_diode);
        let v = self.f_diode;
        let ol = v / 3.0 > 1.04;
        let class = if v < 0.022 {
            0
        } else if v < 0.45 {
            1
        } else if v < 0.75 {
            2
        } else if v < 1.5 {
            3
        } else {
            4
        };
        if class != self.beep_class {
            self.beep_class = class;
            self.beep_e = 0;
            self.beep_n = 0;
            self.beep_next = now;
        } else {
            self.beep_n = self.beep_n.saturating_add(1);
        }
        let mut rd = self.reading(if ol { Val::ol(4, "V") } else { Val::new(v, 4, "V") }, v);
        rd.aux2 = Some(Val::new(v * g / 0.765 * 1000.0, 3, "\u{3a9}"));
        Some(rd)
    }

    /// Diode beep patterns through the HY3131 CMP pin buffer (R20 0xb4 / 0xb0).
    fn diode_beep(&mut self, now: u64) {
        if self.mode != 12 || now < self.beep_next {
            return;
        }
        let (on, step, max_e) = match self.beep_class {
            0 => (true, 50, 0),
            1 if self.beep_n <= 6 => (self.beep_e & 1 == 0, 50, 4),
            2 if self.beep_n <= 6 => (self.beep_e & 1 == 0, 100, 2),
            3 if self.beep_n <= 6 => (self.beep_e & 1 != 0, 25, 7),
            _ => (false, 100, 0),
        };
        hy::write(0x20, &[if on { 0xB4 } else { 0xB0 }]);
        if self.beep_e < max_e {
            self.beep_e += 1;
        }
        self.beep_next = now + step;
    }

    fn read_counters(&mut self, src: u8) -> (f32, f32) {
        let mut b = [0u8; 9];
        hy::read(0x15, &mut b);
        let ctc = u24(&b[0..3]) as f32;
        let ctb = u24(&b[3..6]) as f32;
        let cta = u24(&b[6..9]) as f32;
        let n = cta + 262_144.0;
        hy::write(0x20, &[src]);
        hy::write(0x1B, &[0x00, 0x00, 0xFC]);
        hy::write(0x20, &[src | 2]);
        (ctb / n * 4915.28, ctc / n * 100.0)
    }

    fn freq_range(&mut self) {
        let x = self.factor() * self.over();
        let f = self.freq.f_khz;
        let r = if f < x * 0.01 {
            0
        } else if f < x * 0.1 {
            1
        } else if f < x {
            2
        } else if f < 10.0 * x {
            3
        } else if f < 100.0 * x {
            4
        } else if f < 1000.0 * x {
            5
        } else if f < 10000.0 * x {
            6
        } else {
            self.freq.range
        };
        self.freq.range = r;
    }

    /// FUN_08013450 as the secondary readout in V/A modes, every 350 ms.
    fn freq_secondary(&mut self, now: u64, src: u8) {
        if now < self.freq.next {
            return;
        }
        self.freq.next = now + 350;
        let (f, d) = self.read_counters(src);
        self.freq.f_khz = f;
        self.freq.duty = d;
        self.freq_range();
    }

    /// Frequency / duty readouts. `aux`: the secondary format of V/A modes.
    fn freq_vals(&self, aux: bool) -> (Val, Val) {
        let ri = self.freq.range as i32;
        let f = self.freq.f_khz;
        let fv = if ri < 2 {
            Val::new(f * 1000.0, (ri + if aux { 2 } else { 1 }).clamp(0, 4) as u8, "Hz")
        } else if ri < 5 {
            Val::new(f, (ri - if aux { 1 } else { 2 }).clamp(0, 4) as u8, "kHz")
        } else {
            Val::new(f * 1e-3, (ri - if aux { 4 } else { 5 }).clamp(0, 4) as u8, "MHz")
        };
        (fv, Val::new(self.freq.duty, 1, "%"))
    }

    fn poll_freq_mode(&mut self, now: u64) -> Option<Reading> {
        if now < self.freq.next {
            return None;
        }
        self.freq.next = now + 350;
        // The counter just read was gated with the source selected last time;
        // restart it with the other one (0xf0 comparator / 0x08 ENCNTI).
        let i = self.freq.src_alt as usize;
        let next_src = if self.freq.src_alt { 0xF0 } else { 0x08 };
        let (f, d) = self.read_counters(next_src);
        let ch = if f == 0.0 { 1e6 } else { (f - self.freq.last[i]).abs() };
        self.freq.change[i] = ch;
        self.freq.last[i] = f;
        self.freq.src_alt = !self.freq.src_alt;
        let best = if self.freq.change[0] <= self.freq.change[1] { 0 } else { 1 };
        self.freq.f_khz = self.freq.last[best];
        if best == i {
            self.freq.duty = d;
        }
        if !self.manual {
            self.freq_range();
        }
        let r = self.freq.range as usize;
        let fs = FREQ_DEC[r] * self.factor();
        let ol = self.freq.f_khz / fs > self.over();
        let (fv, dv) = self.freq_vals(false);
        let mut rd = self.reading(if ol { Val::ol(fv.dec, fv.unit) } else { fv }, self.freq.f_khz * 1e3);
        rd.range = r as i8;
        rd.aux2 = Some(dv);
        Some(rd)
    }

    fn poll_temp(&mut self, now: u64) -> Option<Reading> {
        let ad1 = self.ad1()?;
        match self.t_phase {
            0 => {
                self.t_code_a = ad1;
                hy::write(0x33, &[0xAE]);
                self.t_phase = 1;
                self.deadline = now + 700;
                None
            }
            1 => {
                self.t_cj = (self.t_code_a - ad1) as f32 / 640_000.0 * 314.15 - 289.15 + cal(0x194, 0.0);
                hy::write(0x33, &[0xAC]);
                self.t_phase = 2;
                self.deadline = now + 800;
                None
            }
            _ => {
                let vtc = ad1 as f32 * cal(0x24, 1.0) * 1.25 / 0.9 * 100.0 / AD1_DIV - cal(0x2C, 0.0);
                let mut i = 1;
                while i < 14 && TC_K_MV[i + 1] < vtc {
                    i += 1;
                }
                let t_tc = (i as f32 - 2.0 + (vtc - TC_K_MV[i]) / (TC_K_MV[i + 1] - TC_K_MV[i])) * 100.0;
                let t = self.t_cj - 26.0 + t_tc;
                hy::write(0x33, &[0xAF]);
                self.t_phase = 0;
                self.deadline = now + 700;
                let ol = !(-40.0..=1000.0).contains(&t);
                let mut rd = self.reading(if ol { Val::ol(1, "\u{b0}C") } else { Val::new(t, 1, "\u{b0}C") }, t);
                rd.aux1 = Some(Val::new(self.t_cj, 1, "\u{b0}C"));
                rd.aux2 = Some(if ol { Val::ol(1, "\u{b0}F") } else { Val::new(32.0 + t * 1.8, 1, "\u{b0}F") });
                Some(rd)
            }
        }
    }

    fn cap_val(&self, c_uf: f32, ol: bool) -> Reading {
        let r = self.range as usize;
        let (v, dec, unit) = match r {
            0..=2 => (c_uf * 1000.0, 3 - r as u8, "nF"),
            3..=5 => (c_uf, 6 - r as u8, "\u{b5}F"),
            _ => (c_uf * 1e-3, 9 - r as u8, "mF"),
        };
        let dec = dec.min(3);
        self.reading(if ol { Val::ol(dec, unit) } else { Val::new(v, dec, unit) }, c_uf * 1e-6)
    }

    fn cap_set_range(&mut self, clk: &mut Clock, r: i8) {
        let now = clk.now_ms();
        self.range = r;
        if r >= 5 {
            // Discharge first, then start the ramp when |V| < 30 mV.
            hy::quiesce();
            hy::configure(&CFG_F13_DISCHARGE);
            self.ramp.discharging = true;
            self.ramp.n = 0;
            self.ramp.med_i = 0;
            self.ramp.class = 0;
            self.ramp.cmp = None;
            self.ramp.first = true;
            self.deadline = now + 255;
        } else {
            self.write_config(clk);
            self.deadline = now + if r < 4 { 120 } else { 320 };
        }
        self.cap_cnt_up = 0;
        self.cap_cnt_dn = 0;
        self.cap_cnt_small = 0;
    }

    fn poll_cap(&mut self, clk: &mut Clock, now: u64) -> Option<Reading> {
        let r = self.range.max(0) as usize;
        let (f_dec, ov) = (self.factor(), self.over());
        let lim = f_dec * CAP_DEC[r];
        if r < 5 {
            let (f, _) = self.read_counters(0x50);
            self.deadline = now + 420;
            ema(CAP_WIN[r], 0.05, f, &mut self.f_cap);
            let fk = self.f_cap.max(1e-9);
            let c = (cal(0x170 + 4 * r as u32, 1.0) * 0.72135 / (CAP_K[r] * fk) - cal(0x190, 0.0)).min(1.06 * lim);
            let ol = c / lim > ov;
            if !self.manual {
                if r >= 1 && self.f_cap > 2.0 {
                    self.cap_cnt_small += 1;
                    if r < 4 || self.cap_cnt_small >= 4 {
                        self.cap_set_range(clk, 0);
                        return None;
                    }
                } else {
                    self.cap_cnt_small = 0;
                }
                if c > ov * lim {
                    self.cap_set_range(clk, r as i8 + 1);
                    return Some(self.cap_val(0.0, true));
                }
            }
            if !ol {
                self.cap_c = c;
            }
            return Some(self.cap_val(self.cap_c, ol));
        }

        // r5..r7: constant-current ramp on AD1.
        let ad1 = self.ad1()?;
        let v_mv = ad1 as f32 * 1000.0 * 1.25 / 0.9 / AD1_DIV;
        if self.ramp.discharging {
            if v_mv.abs() < 30.0 {
                self.ramp.discharging = false;
                self.write_config(clk);
                self.ramp.prev_t = now;
                self.ramp.prev_v = v_mv;
            }
            self.deadline = now + 255;
            return None;
        }
        let _ = self.read_counters(0x50);
        let dv = (self.ramp.prev_v - v_mv).abs();
        let dt = now.saturating_sub(self.ramp.prev_t) as f32;
        self.ramp.prev_v = v_mv;
        self.ramp.prev_t = now;
        self.deadline = now + 255;
        if dv == 0.0 {
            return None;
        }
        if self.ramp.med_i == 0 {
            self.ramp.med = [dv; 5];
        } else {
            self.ramp.med[self.ramp.med_i % 5] = dv;
        }
        self.ramp.med_i += 1;
        let mut s = self.ramp.med;
        s.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        let dvm = s[2];
        let class = if dvm < 2.5 {
            1
        } else if dvm < 9.0 {
            2
        } else {
            3
        };
        if class != self.ramp.class {
            self.ramp.class = class;
            self.ramp.med_i = 0;
            let v = CAP_R21[class as usize - 1] - if self.model == 2 { 0 } else { 1 };
            hy::write(0x21, &[v]);
            self.deadline = now + 255;
            return Some(self.cap_val(0.0, true));
        }
        let c = (cal(0x170 + 4 * r as u32, 1.0) * CAP_K[r] * dt / dvm).min(1.06 * lim);
        if self.ramp.n < 35 {
            self.ramp.buf[self.ramp.n] = c;
            self.ramp.n += 1;
        }
        let cmp = hy::read1(0x14) & 0x40 != 0;
        let flip = self.ramp.cmp.map(|p| p != cmp).unwrap_or(false);
        self.ramp.cmp = Some(cmp);
        if flip || self.ramp.n == 35 {
            let n = self.ramp.n;
            let mean = if n == 35 {
                self.ramp.buf[33]
            } else if n > 4 {
                let mut b = self.ramp.buf;
                b[..n].sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
                b[2..n - 2].iter().sum::<f32>() / (n - 4) as f32
            } else {
                c
            };
            if self.ramp.first {
                self.cap_c = c;
                self.ramp.first = false;
            } else {
                ema(CAP_WIN[r], 0.25, mean, &mut self.cap_c);
            }
            self.ramp.n = 0;
        }
        let cc = self.cap_c;
        let ol = cc / lim > ov;
        if !self.manual {
            if cc > ov * lim && r < 7 {
                self.cap_cnt_up += 1;
                if self.cap_cnt_up >= 5 {
                    self.cap_set_range(clk, r as i8 + 1);
                }
            } else if cc < 0.1 * lim / ov && r >= 6 {
                self.cap_cnt_dn += 1;
                if self.cap_cnt_dn >= 3 {
                    self.cap_set_range(clk, r as i8 - 1);
                }
            } else {
                self.cap_cnt_up = 0;
                self.cap_cnt_dn = 0;
            }
        }
        Some(self.cap_val(cc, ol))
    }
}

/// Format `v` with `dec` decimals into `buf`; returns the text.
pub fn format(buf: &mut [u8; 16], v: f32, dec: u8) -> &str {
    let mut scale = 1.0f32;
    for _ in 0..dec {
        scale *= 10.0;
    }
    let neg = v < 0.0;
    let mut n = ((if neg { -v } else { v }) * scale + 0.5) as u64;
    let mut tmp = [0u8; 16];
    let mut i = 0;
    loop {
        if i == dec as usize && dec > 0 {
            tmp[i] = b'.';
            i += 1;
        }
        tmp[i] = b'0' + (n % 10) as u8;
        i += 1;
        n /= 10;
        if n == 0 && i > dec as usize + (dec > 0) as usize {
            break;
        }
    }
    let mut o = 0;
    if neg {
        buf[o] = b'-';
        o += 1;
    }
    while i > 0 {
        i -= 1;
        buf[o] = tmp[i];
        o += 1;
    }
    core::str::from_utf8(&buf[..o]).unwrap_or("")
}
