//! Battery and power status (dmm_v2_6.h section 9).
//! Battery: ADC0 channel 15 on PC5, Vbat = raw * 3.0 / 4096 * 2, read every 2 s;
//! pct = (Vbat - 3.3) / 0.9; bars 0..5 at 85/70/50/25/0 % with 5 % hysteresis.
//! Charging (bt_meas_v2_6.h flags bit 3): PB10 high and PB11 high.
//! Bluetooth link: PC14 high = connected.

use crate::mmio::{modify32, r32, w32};

const RCU_APB2EN: u32 = 0x4002_3844;
const ADC0: u32 = 0x4001_2000;
const GPIOB: u32 = 0x4002_0400;
const GPIOC: u32 = 0x4002_0800;

pub struct Power {
    next: u64,
    pub vbat: f32,
    pub bars: u8,
}

impl Power {
    pub fn new() -> Self {
        unsafe {
            modify32(RCU_APB2EN, 0, 1 << 8); // ADC0
            modify32(GPIOC, 3 << 10, 3 << 10); // PC5 analog
            modify32(GPIOC, 3 << 28, 0); // PC14 input
            modify32(GPIOC + 0x0C, 3 << 28, 2 << 28); // pull-down
            modify32(ADC0 + 0x304, 3 << 16, 1 << 16); // ADC clock PCLK2 / 4
            w32(ADC0 + 0x08, 0); // CR2 off
            w32(ADC0 + 0x0C, 6 << 15); // SMPR1: ch15 144 cycles
            w32(ADC0 + 0x2C, 0); // SQR1: 1 conversion
            w32(ADC0 + 0x34, 15); // SQR3: ch15
            w32(ADC0 + 0x08, 1); // ADON
        }
        Power { next: 0, vbat: 4.2, bars: 5 }
    }

    pub fn poll(&mut self, now: u64) -> bool {
        if now < self.next {
            return false;
        }
        self.next = now + 2000;
        unsafe {
            w32(ADC0 + 0x00, 0);
            modify32(ADC0 + 0x08, 0, 1 << 30); // SWSTART
            let mut n = 0;
            while r32(ADC0 + 0x00) & 2 == 0 && n < 10000 {
                n += 1;
            }
            let raw = r32(ADC0 + 0x4C) & 0xFFF;
            self.vbat = raw as f32 * 3.0 / 4096.0 * 2.0;
        }
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

    pub fn charging(&self) -> bool {
        let b = unsafe { r32(GPIOB + 0x10) };
        b & (1 << 10) != 0 && b & (1 << 11) != 0
    }

    pub fn bt_connected(&self) -> bool {
        unsafe { r32(GPIOC + 0x10) & (1 << 14) != 0 }
    }
}
