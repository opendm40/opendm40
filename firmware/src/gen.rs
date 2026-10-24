//! Signal generator (firmware/re/analysis/scope_gen_v2_6.h, GENERATOR).
//!
//! DAC path: DAC channel 1 on PA4, triggered by TIM6 TRGO at 200 MHz /
//! ((PSC+1)(ARR+1)) <= 10 MS/s, fed by DMA0 stream 5 (request 7) circularly
//! from a wave table of L samples holding K periods. PWM path (square above
//! 50 kHz): TIM12 CH1 on PB14 (AF9), 200 MHz / (ARR+1), any divisor 20..4000
//! with adjustable duty (the stock had 7 fixed 50 % steps). PB12 selects the
//! output path (1 = PWM, 0 = DAC). Everything runs from DMA / timers, so the
//! generator keeps going while the CPU does other things.
//!
//! The arithmetic (frequency search, per-wave limits, levels/offset, table
//! contents) is in genplan.rs, which the host test tools/gen-test also
//! builds. Differences from the stock (user feedback UF-14/15/16, FR-28):
//! - the set frequency is clamped to the wave's limit (`Wave::max_hz`, with
//!   `Out::freq_clamped` for a UI note), never silenced to 0 Hz, and not
//!   rounded to 1 kHz steps between 1 and 50 kHz;
//! - the timer divisor, table length and periods per table are searched for
//!   the smallest frequency error; `Gen::actual_hz` is the quantised result;
//! - the ramp rises (the stock's fell, against its own preview icon);
//! - an optional DC offset (`Settings::offset_v`) inside the 0..3 V window,
//!   and Vpp down to 0 V (a DC source).

#[path = "genplan.rs"]
mod plan;

#[allow(unused_imports)]
pub use plan::{
    clamp_freq, compute, limits, target_hz, Limits, Out, Path, Settings, Timing, Wave, AMP_MAX, AMP_MIN, F_MAX,
    F_MIN, N_MAX, SQUARE_DAC_MAX, V_FS,
};

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

#[link_section = ".dmabuf"]
static mut WAVE: [u16; N_MAX] = [0; N_MAX];

pub struct Gen {
    pub s: Settings,
    /// What the output does for `s` (actual frequency, effective levels and
    /// duty, clamp flag), computed also while the output is off.
    pub out: Out,
    /// Actual output frequency while on, Hz (0 when off).
    pub real_hz: f32,
    /// `out.hz()` as f32, kept by `new` / `apply`: SOUR:FREQ? may come
    /// while an AD2 stream runs (gen::plan, region B since the move out of
    /// region C, need not run for it).
    out_hz: f32,
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
        let out = compute(&Settings::DEFAULT);
        Gen { s: Settings::DEFAULT, out_hz: out.hz() as f32, out, real_hz: 0.0 }
    }

    /// Actual (quantised) output frequency, Hz, also while off (UF-15).
    pub fn actual_hz(&self) -> f32 {
        self.out_hz
    }

    /// Actual output frequency in full precision, Hz.
    pub fn actual_hz_f64(&self) -> f64 {
        self.out.hz()
    }

    /// The set frequency is above the current wave's limit and is clamped
    /// (show `self.s.wave.max_note()`, UF-14).
    pub fn freq_clamped(&self) -> bool {
        self.out.freq_clamped
    }

    /// Apply new settings (FUN_080166a4). `s` is normalised
    /// (`Settings::normalised`: frequency kept as set within 1 Hz .. 10 MHz,
    /// Vpp 0..3 V, offset clamped around Vpp); the output runs at
    /// `out.timing`.
    pub fn apply(&mut self, s: Settings) {
        // compute() sees the raw set frequency, so a value above 10 MHz
        // still flags `freq_clamped`; the stored settings are normalised.
        self.out = compute(&s);
        self.out_hz = self.out.hz() as f32;
        let s = s.normalised();
        self.s = s;
        stop_outputs();
        if !s.on {
            self.real_hz = 0.0;
            return;
        }
        let t = self.out.timing;
        match t.path {
            Path::Pwm => self.pwm(&t),
            Path::Dac => self.dac(&s, &t),
        }
        self.real_hz = t.hz() as f32;
    }

    fn pwm(&mut self, t: &Timing) {
        let ccr = plan::pwm_ccr(t, self.s.duty);
        unsafe {
            // PB14 AF9 (TIM12_CH1), PB12 = 1 (PWM path).
            modify32(GPIOB + 0x24, 0xF << 24, 9 << 24);
            modify32(GPIOB + 0x08, 3 << 28, 3 << 28);
            modify32(GPIOB, 3 << 28, 2 << 28);
            w32(GPIOB + 0x18, 1 << 12);
            modify32(GPIOB, 3 << 24, 1 << 24);

            w32(TIM12 + 0x28, t.psc as u32);
            w32(TIM12 + 0x2C, t.arr as u32);
            w32(TIM12 + 0x34, ccr);
            w32(TIM12 + 0x18, (6 << 4) | (1 << 3)); // CH1 PWM1, preload
            w32(TIM12 + 0x20, 1); // CC1E
            w32(TIM12 + 0x14, 1); // UG
            w32(TIM12, 1 | (1 << 7));
        }
    }

    fn dac(&mut self, s: &Settings, t: &Timing) {
        let wave = unsafe { &mut *core::ptr::addr_of_mut!(WAVE) };
        plan::fill(wave, s, t);
        let n = (t.len as usize).min(N_MAX);
        unsafe {
            // PA4 analog, PB12 = 0 (DAC path).
            modify32(GPIOA, 3 << 8, 3 << 8);
            w32(GPIOB + 0x18, 1 << (12 + 16));
            modify32(GPIOB, 3 << 24, 1 << 24);

            // TIM6: TRGO on update, 200 MHz / ((PSC+1)(ARR+1)).
            w32(TIM6 + 0x28, t.psc as u32);
            w32(TIM6 + 0x2C, t.arr as u32);
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
    }
}
