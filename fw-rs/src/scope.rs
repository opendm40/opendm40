//! Oscilloscope: acquisition (scope_gen_v2_6.h) with the stock timebase,
//! vertical and offset maps (scope_gen_ui_v2_6.h).
//!
//! ADC 3PA1030 (10 bit) on PB0..PB9, clocked by TIM8 CH1 on PC6 at 50 MHz.
//! fs >= 50 MS/s: DMA1 stream 4 ch 6 (TIM1_CH4) GPIOB->IDR -> record;
//! fs <  50 MS/s: stream 3 ch 7 (TIM8_CH2) latches IDR at 50 MHz, stream 4 copies
//! the latch into the record at fs (as the stock firmware).
//! One record per frame; the trigger is found in software in the record
//! (screen/2 samples of pre-trigger, then a search window, then screen/2).
//! The analog front end is powered by PC4 (the stock raises it on the scope /
//! generator pages and lowers PE2, the DMM side; see power()).

use crate::mmio::{modify32, r32, w32};

const RCU_AHB1EN: u32 = 0x4002_3830;
const RCU_APB1EN: u32 = 0x4002_3840;
const RCU_APB2EN: u32 = 0x4002_3844;
const GPIOB: u32 = 0x4002_0400;
const GPIOC: u32 = 0x4002_0800;
const GPIOD: u32 = 0x4002_0C00;
const GPIOE: u32 = 0x4002_1000;
const GPIOB_IDR: u32 = GPIOB + 0x10;
const TIM1: u32 = 0x4001_0000;
const TIM8: u32 = 0x4001_0400;
const TIM3: u32 = 0x4000_0400;
const DMA1: u32 = 0x4002_6400;
const S3: u32 = DMA1 + 0x10 + 0x18 * 3;
const S4: u32 = DMA1 + 0x10 + 0x18 * 4;
const LIFCR: u32 = DMA1 + 0x08;
const HIFCR: u32 = DMA1 + 0x0C;

/// Settings record (read only): scope calibration pairs.
const REC: u32 = 0x080C_0000;

/// Screen = 400 columns, 10 divisions; graph height 280 rows (8 x 35).
pub const COLS: usize = 400;
pub const ROWS: i32 = 280;
/// Trigger search window (samples), as the stock software refinement.
const WIN: usize = 800;
/// Record buffer (samples) in SRAM0.
pub const CAP: usize = 52 * 1024;

#[link_section = ".sram"]
static mut BUF: [u16; CAP] = [0; CAP];
#[link_section = ".dmabuf"]
static mut LATCH: [u32; 2] = [0; 2];

/// Hardware timer setups (flash 0x08049a54): fs, tim1 psc, tim1 arr, ccr4.
const HW: [(u32, u32, u32, u32); 31] = [
    (50_000_000, 0, 3, 2),
    (25_000_000, 0, 7, 4),
    (12_500_000, 0, 15, 8),
    (10_000_000, 0, 19, 10),
    (6_250_000, 0, 31, 16),
    (5_000_000, 0, 39, 20),
    (2_500_000, 0, 79, 40),
    (1_250_000, 0, 159, 80),
    (1_000_000, 0, 199, 100),
    (625_000, 0, 319, 160),
    (500_000, 0, 399, 200),
    (250_000, 0, 799, 400),
    (125_000, 0, 1599, 800),
    (100_000, 0, 1999, 1000),
    (62_500, 0, 3199, 1600),
    (50_000, 0, 3999, 2000),
    (25_000, 0, 7999, 4000),
    (12_500, 0, 15999, 8000),
    (10_000, 0, 19999, 10000),
    (6_250, 0, 31999, 16000),
    (5_000, 1, 19999, 10000),
    (2_500, 3, 19999, 10000),
    (1_250, 7, 19999, 10000),
    (1_000, 9, 19999, 10000),
    (625, 15, 19999, 10000),
    (500, 19, 19999, 10000),
    (250, 39, 19999, 10000),
    (125, 79, 19999, 10000),
    (100, 99, 19999, 10000),
    (50, 199, 19999, 10000),
    (25, 399, 19999, 10000),
];

/// Timebases (s/div) and the stock Auto entries {fs, num, den, ppp}: each
/// column covers num/den points of the ppp-times upsampled stream.
pub const SDIV: [f32; 30] = [
    10e-9, 20e-9, 50e-9, 100e-9, 200e-9, 500e-9, 1e-6, 2e-6, 5e-6, 10e-6, 20e-6, 50e-6, 100e-6, 200e-6, 500e-6,
    1e-3, 2e-3, 5e-3, 10e-3, 20e-3, 50e-3, 100e-3, 200e-3, 500e-3, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0,
];
const TB: [(u32, u32, u32, u32); 30] = [
    (200_000_000, 1, 1, 20),
    (200_000_000, 1, 1, 10),
    (200_000_000, 1, 1, 4),
    (50_000_000, 1, 1, 8),
    (50_000_000, 1, 1, 4),
    (50_000_000, 5, 4, 2),
    (50_000_000, 5, 4, 1),
    (50_000_000, 5, 2, 1),
    (50_000_000, 25, 4, 1),
    (50_000_000, 25, 2, 1),
    (50_000_000, 25, 1, 1),
    (50_000_000, 125, 2, 1),
    (50_000_000, 125, 1, 1),
    (25_000_000, 125, 1, 1),
    (10_000_000, 125, 1, 1),
    (5_000_000, 125, 1, 1),
    (2_500_000, 125, 1, 1),
    (1_000_000, 125, 1, 1),
    (500_000, 125, 1, 1),
    (250_000, 125, 1, 1),
    (100_000, 125, 1, 1),
    (50_000, 125, 1, 1),
    (25_000, 125, 1, 1),
    (10_000, 125, 1, 1),
    (5_000, 125, 1, 1),
    (2_500, 125, 1, 1),
    (1_000, 125, 1, 1),
    (500, 125, 1, 1),
    (250, 125, 1, 1),
    (100, 125, 1, 1),
];
/// User timebase range (stock: 100 ns/div .., YT up to 1 s/div; here up to
/// 500 ms/div with one record per frame).
pub const TB_MIN: usize = 3;
pub const TB_MAX: usize = 23;

/// V/div of hardware range r (x1 probe), front-end GPIO row {PC13, PB15, PD12,
/// PD13}, calibration voltage X_r and default gain point.
pub const VDIV: [f32; 10] = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0];
const GPIO_ROW: [[u8; 4]; 10] = [
    [1, 1, 1, 0],
    [1, 1, 1, 1],
    [1, 0, 1, 1],
    [1, 0, 0, 1],
    [1, 0, 1, 0],
    [0, 1, 1, 0],
    [0, 1, 1, 1],
    [0, 1, 0, 1],
    [0, 0, 0, 1],
    [0, 0, 1, 0],
];
const XR: [f32; 10] = [0.04, 0.04, 0.04, 0.4, 0.4, 0.4, 4.0, 4.0, 4.0, 4.0];
const G_DEFAULT: [f32; 10] = [944.0, 727.0, 598.0, 944.0, 727.0, 598.0, 944.0, 727.0, 598.0, 556.0];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrigMode {
    Auto,
    Normal,
    Single,
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub run: bool,
    pub tb: usize,
    pub range: usize,
    pub ac: bool,
    /// Trigger level and zero position in screen rows (0..280, 140 = centre).
    pub trig_row: i32,
    pub pos: i32,
    pub rising: bool,
    pub mode: TrigMode,
    /// Probe factor (x1, x10, x100): scales readouts and V/div labels only.
    pub probe: f32,
}

impl Settings {
    /// Stock defaults: 200 us/div, 1 V/div, DC, rising, Auto, level row 105.
    pub const DEFAULT: Settings =
        Settings { run: true, tb: 13, range: 6, ac: false, trig_row: 105, pos: 0, rising: true, mode: TrigMode::Auto, probe: 1.0 };
}

/// A finished frame: the visible window of the record, or (roll mode, empty
/// `data`) per-column min/max codes in `cols` (u16::MAX = no data yet).
pub struct Frame<'a> {
    pub data: &'a [u16],
    pub cols: Option<&'a [(u16, u16); COLS]>,
    pub trig: Option<usize>,
    pub fs: u32,
    pub seq: u32,
}

fn cal_pair(off: u32, i: usize, dflt: (f32, f32)) -> (f32, f32) {
    let a = REC + off + 8 * i as u32;
    let rd = |x: u32| unsafe { core::ptr::read_volatile(x as *const f32) };
    let (p0, p1) = (rd(a), rd(a + 4));
    let ok = |v: f32| v.is_finite() && v > 0.0 && v < 1024.0;
    if ok(p0) && ok(p1) && (p0 - p1).abs() > 10.0 { (p0, p1) } else { dflt }
}

fn pin(port: u32, n: u32, high: bool) {
    unsafe { w32(port + 0x18, if high { 1 << n } else { 1 << (n + 16) }) };
}

fn out(port: u32, n: u32) {
    unsafe {
        modify32(port + 0x0C, 3 << (2 * n), 0);
        modify32(port + 0x08, 0, 3 << (2 * n));
        modify32(port, 3 << (2 * n), 1 << (2 * n));
    }
}

#[link_section = ".dmabuf"]
static mut ROLL_COLS: [(u16, u16); COLS] = [(0, 0); COLS];

/// Roll mode (stock: from 200 ms/div): continuous circular capture.
pub const TB_ROLL: usize = 22;

pub struct Scope {
    pub s: Settings,
    /// Roll display enabled (used from TB_ROLL up).
    pub roll: bool,
    rolling: bool,
    roll_written: u64,
    roll_last_wr: usize,
    roll_next: u64,
    busy: bool,
    deadline: u64,
    seq: u32,
    relay_until: u64,
    last_pc13: u8,
    powered: bool,
    screen: usize,
    rec: usize,
}

impl Scope {
    pub fn new() -> Self {
        unsafe {
            modify32(RCU_AHB1EN, 0, (1 << 22) | 0x1F);
            modify32(RCU_APB2EN, 0, 0b11);
            modify32(RCU_APB1EN, 0, 1 << 1);
            // PB0..PB9 inputs with pull-down.
            modify32(GPIOB, 0xF_FFFF, 0);
            modify32(GPIOB + 0x0C, 0xF_FFFF, 0xA_AAAA);
            // PC6: TIM8_CH1 (AF3). PC8: TIM3_CH3 (AF2) offset PWM.
            modify32(GPIOC + 0x20, 0xF << 24, 3 << 24);
            modify32(GPIOC + 0x08, 3 << 12, 3 << 12);
            modify32(GPIOC, 3 << 12, 2 << 12);
            modify32(GPIOC + 0x24, 0xF, 2);
            modify32(GPIOC, 3 << 16, 2 << 16);
            w32(TIM3 + 0x28, 0);
            w32(TIM3 + 0x2C, 20000);
            w32(TIM3 + 0x1C, (6 << 4) | (1 << 3));
            w32(TIM3 + 0x20, 1 << 8);
            w32(TIM3 + 0x14, 1);
            w32(TIM3, 1 | (1 << 7));
        }
        // Range / coupling GPIOs; PC4 / PE2 keep their current levels.
        for (p, n) in [(GPIOC, 13), (GPIOB, 15), (GPIOD, 12), (GPIOD, 13), (GPIOE, 3)] {
            out(p, n);
        }
        let mut sc = Scope {
            s: Settings::DEFAULT,
            busy: false,
            deadline: 0,
            seq: 0,
            relay_until: 0,
            last_pc13: 2,
            powered: false,
            screen: 0,
            rec: 0,
            roll: true,
            rolling: false,
            roll_written: 0,
            roll_last_wr: 0,
            roll_next: 0,
        };
        sc.apply(Settings::DEFAULT, 0);
        sc
    }

    /// Analog power as the stock pages do it: scope/generator side (PC4 = 1,
    /// 10 ms, PE2 = 0) or DMM side (PE2 = 1, 10 ms, PC4 = 0). The new rail always
    /// comes up before the old one goes down.
    pub fn power(&mut self, scope_side: bool, clk: &mut crate::clock::Clock) {
        if scope_side == self.powered {
            return;
        }
        self.powered = scope_side;
        if scope_side {
            pin(GPIOC, 4, true);
            out(GPIOC, 4);
            clk.delay_ms(10);
            pin(GPIOE, 2, false);
            out(GPIOE, 2);
            self.relay_until = clk.now_ms() + 200;
        } else {
            self.stop();
            pin(GPIOE, 2, true);
            out(GPIOE, 2);
            clk.delay_ms(10);
            pin(GPIOC, 4, false);
            out(GPIOC, 4);
        }
    }

    pub fn powered(&self) -> bool {
        self.powered
    }

    fn gain(&self, r: usize) -> (f32, f32) {
        cal_pair(0x08, r, (G_DEFAULT[r], 512.0))
    }

    /// ADC code -> volts (FUN_080301a0, without the position).
    pub fn volts(&self, code: f32) -> f32 {
        let r = self.s.range;
        let (g0, g1) = self.gain(r);
        (code - g1) * XR[r] / (g0 - g1) * self.s.probe
    }

    /// Volts -> screen row (FUN_08030146): 140 + pos - 141 v / (4 V/div).
    pub fn row_of_volts(&self, v: f32) -> f32 {
        140.0 + self.s.pos as f32 - 141.0 * v / (4.0 * VDIV[self.s.range] * self.s.probe)
    }

    pub fn row_of_code(&self, code: f32) -> f32 {
        self.row_of_volts(self.volts(code))
    }

    pub fn trig_volts(&self) -> f32 {
        (140.0 + self.s.pos as f32 - self.s.trig_row as f32) * 4.0 * VDIV[self.s.range] * self.s.probe / 141.0
    }

    fn trig_code(&self) -> i32 {
        let r = self.s.range;
        let (g0, g1) = self.gain(r);
        (g1 + self.trig_volts() / self.s.probe * (g0 - g1) / XR[r]) as i32
    }

    pub fn apply(&mut self, s: Settings, now: u64) {
        self.stop();
        let mut s = s;
        s.tb = s.tb.clamp(TB_MIN, TB_MAX);
        s.range = s.range.min(9);
        s.trig_row = s.trig_row.clamp(-10, 290);
        s.pos = s.pos.clamp(-392, 392);
        self.s = s;
        let g = GPIO_ROW[s.range];
        if g[0] != self.last_pc13 {
            self.last_pc13 = g[0];
            self.relay_until = now + 200;
        }
        pin(GPIOC, 13, g[0] != 0);
        pin(GPIOB, 15, g[1] != 0);
        pin(GPIOD, 12, g[2] != 0);
        pin(GPIOD, 13, g[3] != 0);
        pin(GPIOE, 3, !s.ac);
        // Offset PWM (FUN_080302d6): ADC code of 0 V at this position.
        let r = s.range;
        let (g0, g1) = self.gain(r);
        let code_t = g1 - s.pos as f32 * (4.0 * VDIV[r] / XR[r]) * (g0 - g1) / 141.0;
        let (o0, o1) = cal_pair(0xA8, r, (730.0, 280.0));
        let ccr = 8800.0 + (code_t - o1) * (11636.0 - 8800.0) / (o0 - o1);
        let ccr = if ccr.is_finite() && ccr > 0.0 { ccr.min(20000.0) as u32 } else { 0 };
        unsafe { w32(TIM3 + 0x3C, ccr) };
        let (_, num, den, ppp) = TB[s.tb];
        self.screen = ((num * COLS as u32 / den / ppp) as usize).max(2);
        self.rec = (self.screen + WIN).min(CAP);
        self.deadline = 0;
    }

    fn stop(&mut self) {
        unsafe {
            modify32(TIM1, 1, 0);
            modify32(TIM8, 1, 0);
            w32(S4, 0);
            w32(S3, 0);
            while r32(S4) & 1 != 0 || r32(S3) & 1 != 0 {}
        }
        self.busy = false;
    }

    fn start(&mut self) {
        let fs = TB[self.s.tb].0;
        let (_, psc, arr, ccr4) = *HW.iter().find(|h| h.0 == fs).unwrap_or(&HW[0]);
        let buf = core::ptr::addr_of!(BUF) as u32;
        unsafe {
            w32(LIFCR, 0x3D << 22);
            w32(HIFCR, 0x3D);
            w32(TIM8, 0);
            w32(TIM8 + 0x28, 0);
            w32(TIM8 + 0x2C, 3);
            w32(TIM8 + 0x34, 2);
            w32(TIM8 + 0x38, 0);
            w32(TIM8 + 0x18, 6 << 4);
            w32(TIM8 + 0x20, 1);
            w32(TIM8 + 0x44, 1 << 15);
            w32(TIM1, 0);
            w32(TIM1 + 0x28, psc);
            w32(TIM1 + 0x2C, arr);
            w32(TIM1 + 0x40, ccr4);
            w32(TIM1 + 0x1C, 6 << 12);
            w32(TIM1 + 0x20, 1 << 12);
            w32(TIM1 + 0x44, 1 << 15);
            w32(TIM1 + 0x0C, 1 << 12);
            w32(TIM1 + 0x14, 1);
            let src = if fs >= 50_000_000 {
                w32(TIM8 + 0x0C, 0);
                GPIOB_IDR
            } else {
                w32(TIM8 + 0x0C, 1 << 10);
                w32(S3 + 0x08, GPIOB_IDR);
                w32(S3 + 0x0C, core::ptr::addr_of!(LATCH) as u32);
                w32(S3 + 0x04, 2);
                w32(S3, (7 << 25) | (3 << 16) | (1 << 13) | (1 << 11) | (1 << 8) | 1);
                core::ptr::addr_of!(LATCH) as u32
            };
            w32(S4 + 0x08, src);
            w32(S4 + 0x0C, buf);
            self.rolling = self.roll && self.s.tb >= TB_ROLL;
            let (len, circ) = if self.rolling { (CAP as u32, 1 << 8) } else { (self.rec as u32, 0) };
            self.roll_written = 0;
            self.roll_last_wr = 0;
            w32(S4 + 0x04, len);
            w32(S4, (6 << 25) | (3 << 16) | (1 << 13) | (1 << 11) | (1 << 10) | circ | 1);
            w32(TIM8, 1);
            w32(TIM1, 1);
        }
        self.busy = true;
    }

    pub fn sample_rate(&self) -> u32 {
        TB[self.s.tb].0
    }

    /// Display points per sample (> 1 below 1 us/div).
    pub fn ppp(&self) -> u32 {
        TB[self.s.tb].3
    }

    /// Poll: start / finish frames. Returns the visible window when ready.
    pub fn poll(&mut self, now: u64) -> Option<Frame<'static>> {
        if !self.s.run || !self.powered || now < self.relay_until {
            return None;
        }
        if !self.busy {
            self.start();
            if self.deadline == 0 {
                // Auto: untriggered frame after the stock timeout.
                let t = match self.s.tb {
                    0..=15 => 100,
                    16 => 200,
                    17 => 500,
                    18 => 1000,
                    _ => 2000,
                };
                self.deadline = now + t;
            }
            return None;
        }
        if self.rolling {
            return self.roll_poll(now);
        }
        if unsafe { r32(S4 + 0x04) } != 0 {
            return None;
        }
        self.stop();
        let n = self.rec;
        {
            let raw = unsafe { &mut (&mut *core::ptr::addr_of_mut!(BUF))[..n] };
            for v in raw.iter_mut() {
                *v &= 0x3FF;
            }
        }
        let data: &'static [u16] = unsafe { &(&*core::ptr::addr_of!(BUF))[..n] };
        let half = self.screen / 2;
        let win = WIN.min(n - self.screen);
        let trig = self.find_trigger(&data[half..half + win]).map(|i| i + half);
        let show = match (trig, self.s.mode) {
            (Some(_), _) => true,
            (None, TrigMode::Auto) => now >= self.deadline,
            (None, _) => false,
        };
        if !show {
            return None;
        }
        self.deadline = 0;
        if trig.is_some() && self.s.mode == TrigMode::Single {
            self.s.run = false;
        }
        let start = trig.map(|t| t - half).unwrap_or(win / 2);
        self.seq = self.seq.wrapping_add(1);
        Some(Frame { data: &data[start..start + self.screen], cols: None, trig, fs: self.sample_rate(), seq: self.seq })
    }

    /// Roll: every 100 ms, min/max of 400 columns over the latest screen.
    fn roll_poll(&mut self, now: u64) -> Option<Frame<'static>> {
        if now < self.roll_next {
            return None;
        }
        self.roll_next = now + 100;
        let wr = CAP - unsafe { r32(S4 + 0x04) } as usize % (CAP + 1);
        let wr = wr % CAP;
        let d = (wr + CAP - self.roll_last_wr) % CAP;
        self.roll_written += d as u64;
        self.roll_last_wr = wr;
        let per = (self.screen / COLS).max(1);
        let buf = unsafe { &*core::ptr::addr_of!(BUF) };
        let cols = unsafe { &mut *core::ptr::addr_of_mut!(ROLL_COLS) };
        for i in 0..COLS {
            // Column i ends (COLS - 1 - i) columns before the write position.
            let back = (COLS - i) * per;
            if back as u64 > self.roll_written {
                cols[i] = (u16::MAX, u16::MAX);
                continue;
            }
            let start = (wr + CAP - back) % CAP;
            let (mut lo, mut hi) = (u16::MAX, 0u16);
            for k in 0..per {
                let v = buf[(start + k) % CAP] & 0x3FF;
                lo = lo.min(v);
                hi = hi.max(v);
            }
            cols[i] = (lo, hi);
        }
        self.seq = self.seq.wrapping_add(1);
        Some(Frame { data: &[], cols: Some(unsafe { &*core::ptr::addr_of!(ROLL_COLS) }), trig: None, fs: self.sample_rate(), seq: self.seq })
    }

    /// Stock software trigger: beyond the hysteresis band (code -+ 2) on the
    /// far side, then the first sample across the level.
    fn find_trigger(&self, d: &[u16]) -> Option<usize> {
        let l = self.trig_code();
        let mut armed = false;
        for (i, &v) in d.iter().enumerate() {
            let v = v as i32;
            if self.s.rising {
                if v < l - 2 {
                    armed = true;
                } else if armed && v > l {
                    return Some(i);
                }
            } else if v > l + 2 {
                armed = true;
            } else if armed && v < l {
                return Some(i);
            }
        }
        None
    }
}

/// Debug: latch word, record DMA count left, first sample, GPIOB IDR.
pub fn debug() -> [u32; 4] {
    unsafe {
        [
            core::ptr::read_volatile(core::ptr::addr_of!(LATCH[0])),
            r32(S4 + 0x04),
            core::ptr::read_volatile(core::ptr::addr_of!(BUF[0])) as u32,
            r32(GPIOB_IDR),
        ]
    }
}
