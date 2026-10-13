//! Time base from the DWT cycle counter.
//!
//! HCLK is assumed to be the 200 MHz the stock firmware configured (PLL from the
//! 25 MHz HXTAL, M=25 N=400 P=2; read back from RCU_PLL on the device). The
//! 32-bit CYCCNT wraps every ~21 s, so `now_ms` must be called more often than
//! that; the main loop does.

use crate::mmio::{modify32, r32, w32};

pub const HCLK_HZ: u32 = 200_000_000;
const CYC_PER_MS: u32 = HCLK_HZ / 1000;

const DEMCR: u32 = 0xE000_EDFC;
const DWT_CTRL: u32 = 0xE000_1000;
const DWT_CYCCNT: u32 = 0xE000_1004;

pub struct Clock {
    last: u32,
    acc: u32,
    ms: u64,
}

impl Clock {
    pub fn new() -> Self {
        unsafe {
            modify32(DEMCR, 0, 1 << 24); // TRCENA
            w32(DWT_CYCCNT, 0);
            modify32(DWT_CTRL, 0, 1); // CYCCNTENA
        }
        Clock { last: 0, acc: 0, ms: 0 }
    }

    pub fn now_ms(&mut self) -> u64 {
        let c = unsafe { r32(DWT_CYCCNT) };
        self.acc += c.wrapping_sub(self.last);
        self.last = c;
        self.ms += (self.acc / CYC_PER_MS) as u64;
        self.acc %= CYC_PER_MS;
        self.ms
    }

    pub fn delay_ms(&mut self, ms: u32) {
        let end = self.now_ms() + ms as u64;
        while self.now_ms() < end {}
    }
}

/// Busy-wait `us` microseconds on the cycle counter (needs `Clock::new` first).
pub fn delay_us(us: u32) {
    let start = unsafe { r32(DWT_CYCCNT) };
    let n = us * (HCLK_HZ / 1_000_000);
    while unsafe { r32(DWT_CYCCNT) }.wrapping_sub(start) < n {}
}
