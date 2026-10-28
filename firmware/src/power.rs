//! Battery and power status (dmm_v2_6.h section 9).
//! Battery: ADC0 channel 15 on PC5, Vbat = raw * 3.0 / 4096 * 2, read every 2 s;
//! pct = (Vbat - 3.3) / 0.9; bars 0..5 at 85/70/50/25/0 % with 5 % hysteresis.
//! Charging (stock code, FUN_0800b776 / FUN_08023840): PB10 high and PB11
//! high. The stock sets PB11 input with pull-up and PB10 open-drain released
//! (no pull); which TP4056 pins they reach (CHRG / STDBY / VBUS) is unknown.
//! Without the pull-up PB11 floats (opendm40 before 0.3.2: no charging icon,
//! probable cause). Not copied: the stock drives PB10 low below 85 % on USB
//! (FUN_0800b6aa); its effect on the charger is unknown.
//! Bluetooth link: PC14 high = connected.
//! MCU temperature: the GD32 internal sensor on ADC0 channel 16 (TSVREN, bit 23
//! of ADC_SYNCCTL at ADC0 + 0x304), read right after the battery every 2 s,
//! sample time 480 cycles (~19 us at PCLK2/4), against the same 3.0 V
//! reference: T = (V25 - V) / slope + 25. V25 / slope are typical GD32 values
//! (no factory TS calibration on the GD32F4, none found in OTP / system memory
//! by this code): expect several C of offset per chip.

use crate::mmio::{modify32, r32, w32};

const RCU_APB2EN: u32 = 0x4002_3844;
const ADC0: u32 = 0x4001_2000;
const GPIOB: u32 = 0x4002_0400;
const GPIOC: u32 = 0x4002_0800;

/// Typical GD32 temperature sensor constants (unverified for the F427).
const TS_V25: f32 = 1.43;
const TS_SLOPE: f32 = 0.0043;

pub struct Power {
    next: u64,
    pub vbat: f32,
    pub bars: u8,
    /// MCU die temperature, C (NaN until the first read).
    pub mcu_t: f32,
}

/// One software-started conversion of ADC0 channel `ch` (raw 12 bit).
fn convert(ch: u32) -> u32 {
    unsafe {
        w32(ADC0 + 0x34, ch);
        w32(ADC0 + 0x00, 0);
        modify32(ADC0 + 0x08, 0, 1 << 30); // SWSTART
        let mut n = 0;
        while r32(ADC0 + 0x00) & 2 == 0 && n < 10000 {
            n += 1;
        }
        r32(ADC0 + 0x4C) & 0xFFF
    }
}

impl Power {
    pub fn new() -> Self {
        unsafe {
            modify32(RCU_APB2EN, 0, 1 << 8); // ADC0
            modify32(GPIOC, 3 << 10, 3 << 10); // PC5 analog
            modify32(GPIOC, 3 << 28, 0); // PC14 input
            modify32(GPIOC + 0x0C, 3 << 28, 2 << 28); // pull-down
            modify32(GPIOB, 0xF << 20, 0); // PB10, PB11 input
            modify32(GPIOB + 0x0C, 0xF << 20, 1 << 22); // PB11 pull-up, PB10 none (stock)
            modify32(ADC0 + 0x304, 3 << 16, 1 << 16); // ADC clock PCLK2 / 4
            modify32(ADC0 + 0x304, 0, 1 << 23); // TSVREN: temperature sensor on
            w32(ADC0 + 0x08, 0); // CR2 off
            w32(ADC0 + 0x0C, 6 << 15 | 7 << 18); // SMPR1: ch15 144, ch16 480 cycles
            w32(ADC0 + 0x2C, 0); // SQR1: 1 conversion
            w32(ADC0 + 0x34, 15); // SQR3: ch15
            w32(ADC0 + 0x08, 1); // ADON
        }
        Power { next: 0, vbat: 4.2, bars: 5, mcu_t: f32::NAN }
    }

    pub fn poll(&mut self, now: u64) -> bool {
        if now < self.next {
            return false;
        }
        self.next = now + 2000;
        self.vbat = convert(15) as f32 * 3.0 / 4096.0 * 2.0;
        let v = convert(16) as f32 * 3.0 / 4096.0;
        unsafe { w32(ADC0 + 0x34, 15) };
        self.mcu_t = (TS_V25 - v) / TS_SLOPE + 25.0;
        let pct = (self.vbat - 3.3) / 0.9 * 100.0;
        let th = [0.0, 25.0, 50.0, 70.0, 85.0];
        let mut b = self.bars;
        while b < 5 && pct > th[b as usize] + 5.0 {
            b += 1;
        }
        while b > 0 && pct < th[b as usize - 1] {
            b -= 1;
        }
        let changed = b != self.bars;
        self.bars = b;
        changed
    }

    /// Debug: sample ADC0 channel `ch` n times back to back (raw 12-bit),
    /// then restore the battery channel. Returns (min, max, mid crossings,
    /// elapsed cycles).
    pub fn sample(&mut self, ch: u32, n: usize) -> (u16, u16, u32, u32) {
        let (mut lo, mut hi) = (u16::MAX, 0u16);
        let mut buf = [0u16; 512];
        let n = n.min(512);
        let t0 = unsafe { r32(0xE000_1004) };
        unsafe {
            w32(ADC0 + 0x10, 0); // SMPR2: shortest sample time on ch0..9
            w32(ADC0 + 0x34, ch);
            for v in buf[..n].iter_mut() {
                w32(ADC0 + 0x00, 0);
                modify32(ADC0 + 0x08, 0, 1 << 30);
                let mut k = 0;
                while r32(ADC0 + 0x00) & 2 == 0 && k < 10000 {
                    k += 1;
                }
                *v = (r32(ADC0 + 0x4C) & 0xFFF) as u16;
                crate::clock::delay_us(40); // ~20 ms window for 512 samples
            }
            w32(ADC0 + 0x34, 15);
        }
        let dt = unsafe { r32(0xE000_1004) }.wrapping_sub(t0);
        for &v in &buf[..n] {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        let mid = (lo as u32 + hi as u32) / 2;
        let hy = ((hi - lo) as u32 / 8).max(4);
        let (mut below, mut cross) = (buf[0] as u32 <= mid, 0u32);
        for &v in &buf[..n] {
            let v = v as u32;
            if below && v > mid + hy {
                below = false;
                cross += 1;
            } else if !below && v + hy < mid {
                below = true;
            }
        }
        self.next = 0;
        (lo, hi, cross, dt)
    }

    pub fn charging(&self) -> bool {
        self.pins() == 3
    }

    /// Charge status pins: bit 0 PB10, bit 1 PB11 (DBG:DIAG? byte 61).
    pub fn pins(&self) -> u8 {
        let b = unsafe { r32(GPIOB + 0x10) };
        (b >> 10 & 3) as u8
    }

    /// Battery level as shown: while charging, the top bar blinks (stock:
    /// level and level - 1, 1 -> 0 bars at 0, alternating every 1 s;
    /// FUN_08023840 and its timer 0x08023e88). `phase`: the 1 s alternation.
    pub fn shown_bars(&self, phase: bool) -> u8 {
        let b = self.bars.min(5);
        match (self.charging() && phase, b) {
            (false, _) => b,
            (true, 0) => 1,
            (true, _) => b - 1,
        }
    }

    pub fn bt_connected(&self) -> bool {
        unsafe { r32(GPIOC + 0x10) & (1 << 14) != 0 }
    }
}
