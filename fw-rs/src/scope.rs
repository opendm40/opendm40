//! Oscilloscope acquisition (firmware/re/analysis/scope_gen_v2_6.h).
//!
//! ADC: 3PEAK 3PA1030, 10 bit, data on PB0..PB9, clock = TIM8 CH1 on PC6 at
//! 50 MHz. Samples are moved by DMA1 (STM32 "DMA2"):
//!   fs >= 50 MS/s: stream 4 ch 6 (TIM1_CH4 request) GPIOB->IDR -> record;
//!   fs <  50 MS/s: stream 3 ch 7 (TIM8_CH2) latches GPIOB->IDR at 50 MHz into
//!                  a word, stream 4 ch 6 copies that word into the record at fs.
//! Frames are captured one at a time (normal DMA), so the CPU never races the
//! DMA; the trigger is found in software in the finished frame.
//! Front end: range relay/4051 on PC13, PB15, PD12, PD13; coupling PE3;
//! offset = TIM3 CH3 PWM on PC8.

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

/// Record length per frame (samples).
pub const REC: usize = 1024;

#[link_section = ".dmabuf"]
static mut BUF: [u16; REC] = [0; REC];
#[link_section = ".dmabuf"]
static mut LATCH: [u32; 2] = [0; 2];

/// Timebase table: fs, tim1 psc, tim1 arr, ccr4 (flash 0x08049a54).
const TB: [(u32, u32, u32, u32); 31] = [
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

/// Hardware ranges: V/div, {PC13, PB15, PD12, PD13}, nominal counts per volt.
pub const RANGES: [(f32, [u8; 4], f32); 11] = [
    (0.01, [1, 1, 1, 0], 10800.0),
    (0.02, [1, 1, 1, 1], 5375.0),
    (0.05, [1, 0, 1, 1], 2150.0),
    (0.1, [1, 0, 0, 1], 1080.0),
    (0.2, [1, 0, 1, 0], 537.5),
    (0.5, [0, 1, 1, 0], 215.0),
    (1.0, [0, 1, 1, 1], 108.0),
    (2.0, [0, 1, 0, 1], 53.75),
    (5.0, [0, 0, 0, 1], 21.5),
    (10.0, [0, 0, 1, 0], 11.0),
    (20.0, [0, 0, 0, 0], 5.5),
];

/// Debug: latch word, record DMA count left, first record sample, GPIOB IDR.
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

pub fn sample_rate(tb: usize) -> u32 {
    TB[tb.min(30)].0
}

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
    /// Trigger level in ADC codes (0..1023) and edge.
    pub trig_level: u16,
    pub rising: bool,
    pub mode: TrigMode,
    /// Offset PWM compare value (0..20000).
    pub offset: u16,
}

impl Settings {
    pub const DEFAULT: Settings = Settings {
        run: true,
        tb: 10,
        range: 6,
        ac: false,
        trig_level: 512,
        rising: true,
        mode: TrigMode::Auto,
        offset: 10000,
    };
}

/// A finished frame: samples (10-bit codes) and the trigger index.
pub struct Frame<'a> {
    pub data: &'a [u16],
    pub trig: Option<usize>,
    pub fs: u32,
    pub seq: u32,
}

pub struct Scope {
    pub s: Settings,
    busy: bool,
    started: u64,
    auto_misses: u8,
    seq: u32,
    relay_until: u64,
    last_pc13: u8,
}

fn pin(port: u32, n: u32, high: bool) {
    unsafe { w32(port + 0x18, if high { 1 << n } else { 1 << (n + 16) }) };
}

fn out(port: u32, n: u32) {
    unsafe { modify32(port, 3 << (2 * n), 1 << (2 * n)) };
}

impl Scope {
    pub fn new() -> Self {
        unsafe {
            modify32(RCU_AHB1EN, 0, (1 << 22) | 0x1F); // DMA1, GPIOA..E
            modify32(RCU_APB2EN, 0, 0b11); // TIM1, TIM8
            modify32(RCU_APB1EN, 0, 1 << 1); // TIM3

            // PB0..PB9 inputs with pull-down.
            modify32(GPIOB, 0xF_FFFF, 0);
            modify32(GPIOB + 0x0C, 0xF_FFFF, 0xA_AAAA);
            // PC6: TIM8_CH1 (AF3), fast.
            modify32(GPIOC + 0x20, 0xF << 24, 3 << 24);
            modify32(GPIOC + 0x08, 3 << 12, 3 << 12);
            modify32(GPIOC, 3 << 12, 2 << 12);
            // PC8: TIM3_CH3 (AF2) offset PWM.
            modify32(GPIOC + 0x24, 0xF, 2);
            modify32(GPIOC, 3 << 16, 2 << 16);
        }
        // Range / coupling outputs.
        out(GPIOC, 13);
        out(GPIOB, 15);
        out(GPIOD, 12);
        out(GPIOD, 13);
        out(GPIOE, 3);
        unsafe {
            // Offset PWM: PSC 0, ARR 20000 (~10 kHz).
            w32(TIM3 + 0x28, 0);
            w32(TIM3 + 0x2C, 20000);
            w32(TIM3 + 0x1C, (6 << 4) | (1 << 3)); // CH3 PWM1 preload
            w32(TIM3 + 0x20, 1 << 8); // CC3E
            w32(TIM3 + 0x14, 1);
            w32(TIM3, 1 | (1 << 7));
        }
        let mut sc =
            Scope { s: Settings::DEFAULT, busy: false, started: 0, auto_misses: 0, seq: 0, relay_until: 0, last_pc13: 2 };
        sc.apply(Settings::DEFAULT, 0);
        sc
    }

    /// Apply new settings; the next frame uses them.
    pub fn apply(&mut self, s: Settings, now: u64) {
        self.stop();
        self.s = s;
        let g = RANGES[s.range.min(10)].1;
        if g[0] != self.last_pc13 {
            self.last_pc13 = g[0];
            self.relay_until = now + 200; // input relay settle
        }
        pin(GPIOC, 13, g[0] != 0);
        pin(GPIOB, 15, g[1] != 0);
        pin(GPIOD, 12, g[2] != 0);
        pin(GPIOD, 13, g[3] != 0);
        pin(GPIOE, 3, !s.ac); // coupling value 1 (AC) -> PE3 = 0
        unsafe { w32(TIM3 + 0x3C, s.offset as u32) };
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

    fn start(&mut self, now: u64) {
        let (fs, psc, arr, ccr4) = TB[self.s.tb.min(30)];
        let buf = core::ptr::addr_of!(BUF) as u32;
        unsafe {
            w32(LIFCR, 0x3D << 22);
            w32(HIFCR, 0x3D);
            // TIM8: 50 MHz ADC clock on CH1, latch request on CH2.
            w32(TIM8, 0);
            w32(TIM8 + 0x28, 0);
            w32(TIM8 + 0x2C, 3);
            w32(TIM8 + 0x34, 2); // CCR1
            w32(TIM8 + 0x38, 0); // CCR2
            w32(TIM8 + 0x18, 6 << 4); // CH1 PWM1, CH2 frozen
            w32(TIM8 + 0x20, 1); // CC1E
            w32(TIM8 + 0x44, 1 << 15); // MOE
            // TIM1: sample request on CH4.
            w32(TIM1, 0);
            w32(TIM1 + 0x28, psc);
            w32(TIM1 + 0x2C, arr);
            w32(TIM1 + 0x40, ccr4);
            w32(TIM1 + 0x1C, 6 << 12); // CH4 PWM1
            w32(TIM1 + 0x20, 1 << 12); // CC4E
            w32(TIM1 + 0x44, 1 << 15);
            w32(TIM1 + 0x0C, 1 << 12); // CC4DE
            w32(TIM1 + 0x14, 1);

            let src = if fs >= 50_000_000 {
                w32(TIM8 + 0x0C, 0);
                GPIOB_IDR
            } else {
                // Latch: GPIOB->IDR -> LATCH at 50 MHz, circular.
                w32(TIM8 + 0x0C, 1 << 10); // CC2DE
                w32(S3 + 0x08, GPIOB_IDR);
                w32(S3 + 0x0C, core::ptr::addr_of!(LATCH) as u32);
                w32(S3 + 0x04, 2);
                w32(S3, (7 << 25) | (3 << 16) | (1 << 13) | (1 << 11) | (1 << 8) | 1);
                core::ptr::addr_of!(LATCH) as u32
            };
            // Record: src -> BUF, 16 bit, MINC, one frame.
            w32(S4 + 0x08, src);
            w32(S4 + 0x0C, buf);
            w32(S4 + 0x04, REC as u32);
            w32(S4, (6 << 25) | (3 << 16) | (1 << 13) | (1 << 11) | (1 << 10) | 1);

            w32(TIM8, 1);
            w32(TIM1, 1);
        }
        self.busy = true;
        self.started = now;
    }

    /// Frame duration in ms (rounded up).
    pub fn frame_ms(&self) -> u64 {
        (REC as u64 * 1000) / sample_rate(self.s.tb) as u64 + 1
    }

    /// Poll: start / finish frames. Returns a frame when one is ready to show.
    pub fn poll(&mut self, now: u64) -> Option<Frame<'static>> {
        if !self.s.run || now < self.relay_until {
            return None;
        }
        if !self.busy {
            self.start(now);
            return None;
        }
        if unsafe { r32(S4 + 0x04) } != 0 {
            return None;
        }
        self.stop();
        // Mask to 10 bits in place (the record holds raw GPIOB levels).
        {
            let raw = unsafe { &mut *core::ptr::addr_of_mut!(BUF) };
            for v in raw.iter_mut() {
                *v &= 0x3FF;
            }
        }
        let data: &'static [u16] = unsafe { &*core::ptr::addr_of!(BUF) };
        let trig = self.find_trigger(data);
        let show = match (trig, self.s.mode) {
            (Some(_), _) => true,
            (None, TrigMode::Auto) => {
                self.auto_misses += 1;
                self.auto_misses >= 2
            }
            (None, _) => false,
        };
        if trig.is_some() {
            self.auto_misses = 0;
            if self.s.mode == TrigMode::Single {
                self.s.run = false;
            }
        }
        if !show {
            return None;
        }
        self.seq = self.seq.wrapping_add(1);
        Some(Frame { data, trig, fs: sample_rate(self.s.tb), seq: self.seq })
    }

    /// First edge crossing after the first quarter of the frame, with a small
    /// hysteresis (as the stock software search: level / level +- 2).
    fn find_trigger(&self, d: &[u16]) -> Option<usize> {
        let l = self.s.trig_level as i32;
        let start = d.len() / 4;
        let end = d.len() - d.len() / 2;
        let mut armed = false;
        for i in start..end {
            let v = d[i] as i32;
            if self.s.rising {
                if v < l - 2 {
                    armed = true;
                } else if armed && v >= l {
                    return Some(i);
                }
            } else if v > l + 2 {
                armed = true;
            } else if armed && v <= l {
                return Some(i);
            }
        }
        None
    }

    /// Code -> volts with the nominal gains (calibration TBD), zero at 512.
    pub fn volts(&self, code: f32) -> f32 {
        (code - 512.0) / RANGES[self.s.range.min(10)].2
    }
}
