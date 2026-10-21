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

// ===========================================================================
// Core-clock bring-up for the native (XIP) image.
//
// fw-rs runs from flash at 0x08004000 after the FS-loader, which leaves the
// clock tree in an unknown state, so the shim (src/shim.rs) calls `init_core`
// before anything else that depends on HCLK.
//
// Sequence = the stock bring-up FUN_08008710 (addresses and values from its
// literal pool), with the target register values MEASURED on a live device
// running stock at 200 MHz (memrw, 2026-10-13):
//   RCU_PLL  0x29406419   RCU_CFG0 0x0000940A   RCU_CTL 0x03037A83 (PLLSAI off)
//   DCKCFGR  0x01000000   PMU_CTL  0x0003C000   FLASH_ACR (0x40023C00) 0x00000000
// FLASH_ACR is NOT written: the stock runs at 200 MHz with 0 wait states and no
// prefetch/cache bits (the GD32F4 low 512 KiB is zero-wait), so we leave it as
// the boot stages left it (an earlier inferred 0x706 is dropped).
//
// Differences from the stock routine, all for robustness: every wait is bounded
// (the stock spins forever on a dead HXTAL), and the caller may retry with the
// internal 16 MHz RC as the PLL source (`Src::Irc16m`, same 1 MHz PLL input, so
// SYSCLK is 200 MHz and PLLSAI still gives 48 MHz; RC accuracy only).
//
// FPU (CPACR) and VTOR are set by the reset code (src/shim.rs, src/native.rs),
// not here.

const RCU_CTL: u32 = 0x4002_3800; // RCC_CR
const RCU_PLL: u32 = 0x4002_3804; // RCC_PLLCFGR
const RCU_CFG0: u32 = 0x4002_3808; // RCC_CFGR
const RCU_INT: u32 = 0x4002_380C; // RCC_CIR
const RCU_APB1EN: u32 = 0x4002_3840; // bit 28 = PMUEN
const RCU_CFG1: u32 = 0x4002_388C; // RCC_DCKCFGR (bit 24 = TIMPRE)
const PMU_CTL: u32 = 0x4000_7000; // PWR_CR
const PMU_CS: u32 = 0x4000_7004; // PWR_CSR

/// M=25 N=400 P=2 (bits 17:16 = 0) Q=9 R=2, PLLSRC=HXTAL (bit 22): 200 MHz.
/// Exactly the value read from a running stock.
pub const PLL_HXTAL: u32 = 0x2940_6419;
/// Same with PLLSRC=IRC16M and M=16 (1 MHz PLL input as above).
pub const PLL_IRC16M: u32 = (PLL_HXTAL & !(0x3F | (1 << 22))) | 16;
const PLL_RESET: u32 = 0x2400_3010; // RCU_PLL reset value (stock writes it back first)

/// Poll budget per wait (~0x40000 x >= 4 cycles at 16 MHz = >= 65 ms; HXTAL
/// start-up is a few ms, PLL lock and the over-drive handshake well under 1 ms).
const WAIT: u32 = 0x4_0000;

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
pub enum Src {
    Hxtal,
    Irc16m,
}

/// Poll `reg & mask == want` at most WAIT times.
#[inline(always)]
fn wait(reg: u32, mask: u32, want: u32) -> bool {
    let mut n = 0u32;
    while n < WAIT {
        if unsafe { r32(reg) } & mask == want {
            return true;
        }
        n += 1;
    }
    false
}

/// Bring SYSCLK to 200 MHz (AHB /1, APB2 /2, APB1 /4, TIMPRE) from `src`.
/// Every wait is bounded. On `false`, SYSCLK is the internal 16 MHz RC (the
/// state the preamble leaves) and the PLL is off or unused; the caller may
/// retry with another source. On `true`, the measured stock register values
/// above are in place (RCU_CTL apart from the HXTAL bits for `Irc16m`).
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
#[cfg_attr(target_os = "none", link_section = ".shim")]
#[inline(never)]
pub fn init_core(src: Src) -> bool {
    unsafe {
        // --- RCU reset preamble (FUN_08008710) ---
        modify32(RCU_CTL, 0, 1); // IRC16MEN
        if !wait(RCU_CTL, 2, 2) {
            // IRC16MSTB: the RC is our fallback clock; without it, give up
            // before touching SYSCLK.
            return false;
        }
        modify32(RCU_CFG0, 0, 0x80); // AHB /2 while switching
        let mut i = 0u32;
        while i < 0x14 {
            core::arch::asm!("nop", options(nomem, nostack));
            i += 1;
        }
        // SCS -> IRC16M, prescalers /1. Wait for the switch (SCSS = 0b00)
        // before PLLEN may be cleared (the boot stages may have left PLL on).
        w32(RCU_CFG0, 0);
        if !wait(RCU_CFG0, 0xC, 0) {
            return false;
        }
        modify32(RCU_CTL, 0x0109_0000, 0); // clear PLLEN(24), CKMEN(19), HXTALEN(16)
        w32(RCU_PLL, PLL_RESET);
        modify32(RCU_CTL, 0x0004_0000, 0); // clear HXTALBPS(18)
        w32(RCU_INT, 0); // RCU interrupts off
        if !wait(RCU_CTL, 0x0200_0000, 0) {
            return false; // PLLSTB must drop after PLLEN=0
        }

        // --- Clock source ---
        if src == Src::Hxtal {
            modify32(RCU_CTL, 0, 0x0001_0000); // HXTALEN
            if !wait(RCU_CTL, 0x0002_0000, 0x0002_0000) {
                // HXTALSTB never came: switch the oscillator off again.
                modify32(RCU_CTL, 0x0001_0000, 0);
                return false;
            }
        }

        // --- Voltage scaling (LDOVS = 0b11) before raising the frequency ---
        modify32(RCU_APB1EN, 0, 0x1000_0000); // PMUEN
        modify32(PMU_CTL, 0, 0xC000);

        // --- Bus prescalers: AHB /1, APB2 /2 (0b100 << 13), APB1 /4 (0b101 << 10) ---
        modify32(RCU_CFG0, 0, 0x8000);
        modify32(RCU_CFG0, 0, 0x1400);

        // --- PLL ---
        w32(RCU_PLL, if src == Src::Hxtal { PLL_HXTAL } else { PLL_IRC16M });
        modify32(RCU_CTL, 0, 0x0100_0000); // PLLEN
        if !wait(RCU_CTL, 0x0200_0000, 0x0200_0000) {
            modify32(RCU_CTL, 0x0100_0000, 0);
            return false;
        }

        // --- Over-drive (high-drive) handshake, SYSCLK still on IRC16M ---
        modify32(PMU_CTL, 0, 0x0001_0000); // ODEN / HDEN
        if !wait(PMU_CS, 0x0001_0000, 0x0001_0000) {
            return false;
        }
        modify32(PMU_CTL, 0, 0x0002_0000); // ODSWEN / HDS
        if !wait(PMU_CS, 0x0002_0000, 0x0002_0000) {
            return false;
        }

        // --- SYSCLK -> PLL ---
        modify32(RCU_CFG0, 0x3, 0x2);
        if !wait(RCU_CFG0, 0xC, 0x8) {
            // Not switched: make sure we stay on the RC.
            modify32(RCU_CFG0, 0x3, 0);
            return false;
        }

        // --- TIMPRE: timer clocks = 200 MHz (stock FUN_080069b6(0x1000000)) ---
        modify32(RCU_CFG1, 0, 0x0100_0000);
    }
    true
}
