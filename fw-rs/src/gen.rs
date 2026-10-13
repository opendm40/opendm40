//! Signal generator (firmware/re/analysis/scope_gen_v2_6.h, GENERATOR).
//!
//! DAC path: DAC channel 1 on PA4, triggered by TIM6 TRGO at 10 MHz / (PSC+1),
//! fed by DMA0 stream 5 (request 7) circularly from a wave table of n samples.
//! PWM path (square above 50 kHz): TIM12 CH1 on PB14 (AF9), fixed frequencies.
//! PB12 selects the output path (1 = PWM, 0 = DAC). Everything runs from DMA /
//! timers, so the generator keeps going while the CPU does other things.

use crate::mmio::{modify32, r32, w32};

const RCU_APB1EN: u32 = 0x4002_3840;
const GPIOA: u32 = 0x4002_0000;
const GPIOB: u32 = 0x4002_0400;
const DAC: u32 = 0x4000_7400;
const DAC_CR: u32 = DAC + 0x00;
const DAC_DHR12R1: u32 = DAC + 0x08;
const TIM6: u32 = 0x4000_1000;
const TIM12: u32 = 0x4000_1800;
const DMA0: u32 = 0x4002_6000;
const S5: u32 = DMA0 + 0x10 + 0x18 * 5;
const DMA_HIFCR: u32 = DMA0 + 0x0C;

pub const N_MAX: usize = 2000;

#[link_section = ".dmabuf"]
static mut WAVE: [u16; N_MAX] = [0; N_MAX];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Wave {
    Sine,
    Square,
    Triangle,
    Ramp,
}

impl Wave {
    pub const ALL: [Wave; 4] = [Wave::Sine, Wave::Square, Wave::Triangle, Wave::Ramp];
    pub fn name(self) -> &'static str {
        match self {
            Wave::Sine => "Sine",
            Wave::Square => "Square",
            Wave::Triangle => "Triangle",
            Wave::Ramp => "Ramp",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub on: bool,
    pub wave: Wave,
    pub freq_hz: u32,
    /// Peak amplitude, V (0.5 .. 3.0). The output is unipolar 0..amp.
    pub amp_v: f32,
    /// Square duty, 0..10000 = 0..100.00 %.
    pub duty: u16,
}

impl Settings {
    pub const DEFAULT: Settings = Settings { on: false, wave: Wave::Sine, freq_hz: 1000, amp_v: 3.0, duty: 5000 };
}

/// PWM path frequencies above 50 kHz (TIM12 PSC / period at 200 MHz).
const PWM_HZ: [u32; 7] = [100_000, 200_000, 500_000, 1_000_000, 2_000_000, 5_000_000, 10_000_000];
const PWM_PSC: [u32; 7] = [4, 4, 4, 4, 0, 0, 0];
const PWM_PER: [u32; 7] = [400, 200, 80, 40, 100, 40, 20];

/// Frequency as the stock UI snaps it (FUN_08021cc8): < 1 kHz any integer,
/// 1..50 kHz to 1 kHz steps, above 50 kHz square only at the PWM steps.
pub fn snap_freq(wave: Wave, f: u32) -> u32 {
    if f < 1000 {
        f.max(1)
    } else if f <= 50_000 {
        f / 1000 * 1000
    } else if wave == Wave::Square {
        *PWM_HZ.iter().rev().find(|&&p| p <= f).unwrap_or(&PWM_HZ[0])
    } else {
        50_000
    }
}

/// sin(x) for x in [0, 2pi): range reduction + odd polynomial (|err| < 2e-6).
fn sin(x: f32) -> f32 {
    const PI: f32 = core::f32::consts::PI;
    let mut x = x;
    while x > PI {
        x -= 2.0 * PI;
    }
    let (x, s) = if x > PI / 2.0 {
        (PI - x, 1.0)
    } else if x < -PI / 2.0 {
        (-PI - x, 1.0)
    } else {
        (x, 1.0)
    };
    let x2 = x * x;
    s * x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0))))
}

pub struct Gen {
    pub s: Settings,
    /// Actual output frequency (after table length rounding), Hz.
    pub real_hz: f32,
}

fn stop_outputs() {
    unsafe {
        // DMA off, DAC off, timers off.
        w32(S5, 0);
        while r32(S5) & 1 != 0 {}
        w32(DAC_CR, r32(DAC_CR) & !0xFFFF);
        modify32(TIM6, 1, 0);
        modify32(TIM12, 1, 0);
        // PA4 output low, PB14 / PB12 inputs.
        w32(GPIOA + 0x18, 1 << (4 + 16));
        modify32(GPIOA, 3 << 8, 1 << 8);
        modify32(GPIOB, (3 << 28) | (3 << 24), 0);
    }
}

impl Gen {
    pub fn new() -> Self {
        unsafe { modify32(RCU_APB1EN, 0, (1 << 29) | (1 << 4) | (1 << 6)) }; // DAC, TIM6, TIM12
        stop_outputs();
        Gen { s: Settings::DEFAULT, real_hz: 0.0 }
    }

    /// Apply new settings (FUN_080166a4).
    pub fn apply(&mut self, s: Settings) {
        let mut s = s;
        s.amp_v = s.amp_v.clamp(0.5, 3.0);
        s.duty = s.duty.min(10000);
        s.freq_hz = snap_freq(s.wave, s.freq_hz);
        self.s = s;
        stop_outputs();
        if !s.on {
            self.real_hz = 0.0;
            return;
        }
        if s.wave == Wave::Square && s.freq_hz > 50_000 {
            self.pwm(s.freq_hz);
        } else {
            self.dac(&s);
        }
    }

    fn pwm(&mut self, f: u32) {
        let i = PWM_HZ.iter().position(|&p| p == f).unwrap_or(0);
        unsafe {
            // PB14 AF9 (TIM12_CH1), PB12 = 1 (PWM path).
            modify32(GPIOB + 0x24, 0xF << 24, 9 << 24);
            modify32(GPIOB + 0x08, 3 << 28, 3 << 28);
            modify32(GPIOB, 3 << 28, 2 << 28);
            w32(GPIOB + 0x18, 1 << 12);
            modify32(GPIOB, 3 << 24, 1 << 24);

            w32(TIM12 + 0x28, PWM_PSC[i]);
            w32(TIM12 + 0x2C, PWM_PER[i] - 1);
            w32(TIM12 + 0x34, PWM_PER[i] / 2);
            w32(TIM12 + 0x18, (6 << 4) | (1 << 3)); // CH1 PWM1, preload
            w32(TIM12 + 0x20, 1); // CC1E
            w32(TIM12 + 0x14, 1); // UG
            w32(TIM12, 1 | (1 << 7));
        }
        self.real_hz = 200e6 / ((PWM_PSC[i] + 1) * PWM_PER[i]) as f32;
    }

    fn dac(&mut self, s: &Settings) {
        let f = s.freq_hz;
        let (psc, n) = if f > 5000 {
            (0, (10_000_000 / f) as usize)
        } else {
            let k = ((5000 + f / 2) / f).max(1);
            (k - 1, ((10_000_000 + f * k / 2) / (f * k)) as usize)
        };
        let n = n.clamp(2, N_MAX);
        let a = s.amp_v / 3.0;
        let wave = unsafe { &mut *core::ptr::addr_of_mut!(WAVE) };
        let nf = n as f32;
        for (i, w) in wave[..n].iter_mut().enumerate() {
            let fi = i as f32;
            let v = match s.wave {
                Wave::Sine => {
                    let (c, wd) = if a > 0.5 { (50.0, 4045.0) } else { (200.0, 4095.0) };
                    a * (c + wd / 2.0 + wd / 2.0 * sin(2.0 * core::f32::consts::PI * fi / nf))
                }
                Wave::Square => {
                    if (i as u32) < (n as u32 * s.duty as u32 / 10000) { a * 4095.0 } else { 0.0 }
                }
                Wave::Triangle => {
                    if i < n / 2 { 8190.0 / nf * fi * a } else { (8190.0 - 8190.0 / nf * fi) * a }
                }
                Wave::Ramp => (4095.0 - 4095.0 / nf * fi) * a,
            };
            *w = (v.clamp(0.0, 4095.0)) as u16;
        }
        unsafe {
            // PA4 analog, PB12 = 0 (DAC path).
            modify32(GPIOA, 3 << 8, 3 << 8);
            w32(GPIOB + 0x18, 1 << (12 + 16));
            modify32(GPIOB, 3 << 24, 1 << 24);

            // TIM6: TRGO on update, 10 MHz / (PSC+1).
            w32(TIM6 + 0x28, psc);
            w32(TIM6 + 0x2C, 19);
            w32(TIM6 + 0x04, 0x20);
            w32(TIM6 + 0x14, 1);

            // DMA0 stream 5, request 7: M->P, 16 bit, MINC, circular, high prio.
            w32(DMA_HIFCR, 0x3D << 6);
            w32(S5 + 0x08, DAC_DHR12R1);
            w32(S5 + 0x0C, core::ptr::addr_of!(WAVE) as u32);
            w32(S5 + 0x04, n as u32);
            w32(S5, (7 << 25) | (2 << 16) | (1 << 13) | (1 << 11) | (1 << 10) | (1 << 8) | (1 << 6) | 1);

            // DAC ch1: trigger TIM6 (TSEL 000), DMA, buffer off except square.
            let boff = if s.wave == Wave::Square { 0 } else { 1 << 1 };
            w32(DAC_CR, (r32(DAC_CR) & !0xFFFF) | (1 << 12) | (1 << 2) | boff | 1);
            w32(TIM6, 1);
        }
        self.real_hz = 10e6 / ((psc + 1) as f32 * n as f32);
    }
}
