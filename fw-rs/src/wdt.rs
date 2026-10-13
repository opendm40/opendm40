//! Free watchdog timer (FWDGT on GD32F427 = IWDG layout).
//!
//! Once we take over with interrupts off, the stock TIM7 1 kHz ISR that used to
//! reload the watchdog no longer runs, so the main loop must do it. Reload is a
//! write of 0xAAAA to the key register at 0x40003000 (verified in the stock
//! firmware: FUN_08005dfc writes 0xAAAA there).

use core::sync::atomic::{AtomicBool, Ordering};

use crate::mmio::w32;

const FWDGT_CTL: u32 = 0x4000_3000;
const KEY_RELOAD: u32 = 0x0000_AAAA;

#[inline(always)]
pub fn kick() {
    unsafe { w32(FWDGT_CTL, KEY_RELOAD) };
}

static FEEDING: AtomicBool = AtomicBool::new(true);

/// Whether the main loop currently feeds the watchdog (debug builds stop
/// feeding past the keepalive deadline so the device falls back to stock).
pub fn set_feeding(on: bool) {
    FEEDING.store(on, Ordering::Relaxed);
}

/// Kick from inside a long blocking operation, only while the main loop would.
pub fn kick_if_feeding() {
    if FEEDING.load(Ordering::Relaxed) {
        kick();
    }
}
