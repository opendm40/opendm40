//! Multimeter measurement: HY3131 mode setup, relays, calibration, autorange.
//! All tables and formulas: firmware/re/analysis/hy3131_v2_6.h.

use core::ptr::read_volatile;

use crate::clock::Clock;
use crate::hy3131 as hy;
use crate::mmio::{modify32, w32};

const GPIOA: u32 = 0x4002_0000;
const GPIOE: u32 = 0x4002_1000;

/// Settings record in internal flash (read only): u16 crc, u16 ~crc,
/// u32 version = 1, data. DMM calibration at record offset 0x168.
const SETTINGS: u32 = 0x080C_0000;
const CAL: u32 = SETTINGS + 0x168;

/// AD1 full scale: 1.25 V REFO / AD1IG 0.9 at 2^23 - 1 counts.
const AD1_FS: f32 = 1.25 / 0.9 / 8_388_607.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    DcV,
    DcMv,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::DcV | Mode::DcMv => "DC",
        }
    }
}

// R20..R35 rows per range. Mode 1 DC V: flash 0x0804a320.
const CFG_DCV: [[u8; hy::CFG_LEN]; 4] = [
    [0xf0, 0xdd, 0x15, 0x83, 0x31, 0x40, 0x00, 0x55, 0x31, 0x00, 0x06, 0x00, 0x00, 0x90, 0x28, 0xa0, 0x80, 0xd7, 0x38, 0xac, 0x00, 0x54],
    [0xf0, 0xdd, 0x15, 0x83, 0x07, 0x40, 0x00, 0x55, 0x31, 0x00, 0x06, 0x00, 0x00, 0x09, 0x68, 0xa4, 0x80, 0xc7, 0x33, 0xac, 0x00, 0x54],
    [0xf0, 0xdd, 0x15, 0x83, 0x07, 0x40, 0x00, 0x55, 0x31, 0x00, 0x06, 0x00, 0x90, 0x00, 0x68, 0xa4, 0x80, 0xc7, 0x33, 0xac, 0x00, 0x54],
    [0xf0, 0xdd, 0x15, 0x83, 0x07, 0x40, 0x00, 0x55, 0x31, 0x00, 0x06, 0x00, 0x99, 0x00, 0x68, 0xa4, 0x80, 0xc7, 0x33, 0xac, 0x00, 0x54],
];

// Mode 3 DC mV: flash 0x0804a378.
const CFG_DCMV: [[u8; hy::CFG_LEN]; 2] = [
    [0xf0, 0xdd, 0x15, 0x83, 0x33, 0x01, 0x00, 0x55, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x91, 0x80, 0xd7, 0x3a, 0xac, 0x00, 0x50],
    [0xf0, 0xdd, 0x15, 0x83, 0x33, 0x00, 0x00, 0x55, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x91, 0x80, 0xd7, 0x3a, 0xac, 0x00, 0x50],
];

/// Over-range factor per model (DM40A/B/C).
const OVER: [f32; 3] = [1.05, 1.04, 1.03];

pub struct Reading {
    pub mode: Mode,
    pub range: u8,
    /// Calibrated value in `unit`s.
    pub value: f32,
    pub unit: &'static str,
    /// Digits after the decimal point for this range.
    pub decimals: u8,
    pub overload: bool,
    pub auto: bool,
}

pub struct Dmm {
    mode: Mode,
    range: u8,
    auto: bool,
    model: u8,
    settle: u8,
}

/// Calibration float at struct offset `off`; falls back to `dflt` if the
/// record is missing or the value is not sane.
fn cal_f32(off: u32, dflt: f32) -> f32 {
    let ok = unsafe { read_volatile((SETTINGS + 4) as *const u32) } == 1;
    let v = unsafe { read_volatile((CAL + off) as *const f32) };
    if ok && v.is_finite() && v.abs() < 1.0e6 { v } else { dflt }
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

impl Dmm {
    pub fn new() -> Self {
        hy::init_pins();
        let model = unsafe { read_volatile(CAL as *const u8) }.min(2);
        Dmm { mode: Mode::DcV, range: 0, auto: true, model, settle: 0 }
    }

    pub fn model(&self) -> u8 {
        self.model
    }

    fn ranges(&self) -> u8 {
        match self.mode {
            Mode::DcV => 4,
            Mode::DcMv => 2,
        }
    }

    /// Nominal range limit in display units (e.g. 4, 40, 400 V for DM40A).
    fn limit(&self, r: u8) -> f32 {
        let f = (4 + self.model) as f32;
        match self.mode {
            Mode::DcV => f * [1.0, 10.0, 100.0, 1000.0][r as usize],
            Mode::DcMv => f * [10.0, 100.0][r as usize],
        }
    }

    pub fn set_mode(&mut self, clk: &mut Clock, mode: Mode) {
        self.mode = mode;
        self.range = 0;
        self.auto = true;
        hy::quiesce();
        match mode {
            Mode::DcV => relay_pa(clk, 0),
            Mode::DcMv => relay_pa(clk, 1),
        }
        relay_pe(2);
        self.apply_range();
    }

    fn apply_range(&mut self) {
        let cfg = match self.mode {
            Mode::DcV => &CFG_DCV[self.range as usize],
            Mode::DcMv => &CFG_DCMV[self.range as usize],
        };
        hy::configure(cfg);
        self.settle = 2; // drop the first conversions after a change
    }

    /// Poll the converter; returns a reading when a new conversion is in.
    pub fn poll(&mut self) -> Option<Reading> {
        if hy::read1(hy::R_INTF) & hy::INTF_AD1 == 0 {
            return None;
        }
        let mut b = [0u8; 3];
        hy::read(hy::R_AD1, &mut b);
        let raw = hy::s24(&b) as f32;
        if self.settle > 0 {
            self.settle -= 1;
            return None;
        }
        let r = self.range as usize;
        let (nominal, value, unit) = match self.mode {
            Mode::DcV => {
                let n = raw * AD1_FS * [10.0, 100.0, 1e3, 1e4][r];
                (n, n * cal_f32(0x04 + 4 * r as u32, 1.0) - cal_f32(0x14 + 4 * r as u32, 0.0), "V")
            }
            Mode::DcMv => {
                let n = raw * AD1_FS * 1000.0 / [10.0, 1.0][r];
                (n, n * cal_f32(0x24 + 4 * r as u32, 1.0) - cal_f32(0x2C + 4 * r as u32, 0.0), "mV")
            }
        };

        let lim = self.limit(self.range);
        let over = OVER[self.model as usize];
        let overload = nominal.abs() > lim * over;
        let reading = Reading {
            mode: self.mode,
            range: self.range,
            value,
            unit,
            decimals: decimals_for(lim / (4 + self.model) as f32),
            overload,
            auto: self.auto,
        };

        if self.auto {
            if overload && self.range + 1 < self.ranges() {
                self.range += 1;
                self.apply_range();
            } else if self.range > 0 && nominal.abs() < lim * 0.1 / over {
                self.range -= 1;
                self.apply_range();
            }
        }
        Some(reading)
    }
}

/// 40000-count display: a 1-decade range shows 4 decimals, 10 -> 3, ...
fn decimals_for(decade: f32) -> u8 {
    let mut d = 4u8;
    let mut x = decade;
    while x >= 9.5 && d > 0 {
        x /= 10.0;
        d -= 1;
    }
    d
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
