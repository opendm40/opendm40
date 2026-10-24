//! Tiny volatile MMIO helpers. No PAC, deliberately minimal for the PoC.

use core::ptr::{read_volatile, write_volatile};

#[inline(always)]
pub unsafe fn r32(addr: u32) -> u32 {
    read_volatile(addr as *const u32)
}

#[inline(always)]
pub unsafe fn w32(addr: u32, val: u32) {
    write_volatile(addr as *mut u32, val);
}

#[inline(always)]
pub unsafe fn w16(addr: u32, val: u16) {
    write_volatile(addr as *mut u16, val);
}

#[inline(always)]
pub unsafe fn modify32(addr: u32, clear: u32, set: u32) {
    let v = read_volatile(addr as *const u32);
    write_volatile(addr as *mut u32, (v & !clear) | set);
}

/// Crude busy-wait; not cycle-accurate, only for coarse delays at ~200 MHz.
#[inline(never)]
pub fn spin(iters: u32) {
    let mut i = 0u32;
    while i < iters {
        unsafe { core::arch::asm!("nop", options(nomem, nostack)) };
        i = i.wrapping_add(1);
    }
}
