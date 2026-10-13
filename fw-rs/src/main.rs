#![no_std]
#![no_main]

mod lcd;
mod mmio;
mod startup;
mod wdt;

use core::panic::PanicInfo;

/// System control block: vector table offset register.
const SCB_VTOR: u32 = 0xE000_ED08;

extern "C" {
    static mut __bss_start: u32;
    static mut __bss_end: u32;
    static __VECTORS: u32;
}

/// Called from the reset trampoline (startup.rs) with MSP set and IRQs masked.
#[no_mangle]
pub unsafe extern "C" fn rust_start() -> ! {
    // Zero .bss.
    let mut p = core::ptr::addr_of_mut!(__bss_start);
    let end = core::ptr::addr_of_mut!(__bss_end);
    while p < end {
        p.write_volatile(0);
        p = p.add(1);
    }

    // Point the vector table at our table in SRAM.
    mmio::w32(SCB_VTOR, core::ptr::addr_of!(__VECTORS) as u32);

    main()
}

fn main() -> ! {
    // Milestone 1: prove load + run + display by filling the screen, and keep
    // the watchdog fed now that the stock TIM7 ISR no longer runs.
    wdt::kick();
    lcd::fill_screen(lcd::RED);

    loop {
        wdt::kick();
        mmio::spin(50_000);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // Keep feeding the watchdog so a panic shows as a steady screen, not a reset.
    loop {
        wdt::kick();
        mmio::spin(50_000);
    }
}
