#![no_std]
#![no_main]

mod lcd;
mod mmio;
mod startup;
mod wdt;

use core::panic::PanicInfo;

/// System control block: vector table offset register.
const SCB_VTOR: u32 = 0xE000_ED08;

/// How many kick iterations to hold before letting the watchdog reset us back to
/// the stock firmware. This is the recovery path: we must NEVER feed the
/// watchdog unconditionally, or the only way out is pulling the battery. The
/// count is approximate (busy-wait timed); tune against the observed duration.
const HOLD_ITERS: u32 = 20_000;

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
    // Milestone 1: prove load + run + display. Address the panel in landscape
    // directly (MV=1) and fill it red; the stock firmware left it in native
    // 320x480 portrait addressing, which is why a 480x320 fill showed a square.
    wdt::kick();
    lcd::set_madctl(lcd::MADCTL_LANDSCAPE);
    lcd::fill_screen(lcd::RED);

    // Hold the screen while feeding the watchdog, then stop: the watchdog resets
    // the device back to the stock firmware so there is always a way out.
    let mut n = 0u32;
    while n < HOLD_ITERS {
        wdt::kick();
        mmio::spin(50_000);
        n += 1;
    }

    release_to_stock()
}

/// Stop feeding the watchdog and wait for it to reset us back to stock.
fn release_to_stock() -> ! {
    loop {
        mmio::spin(1_000_000);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // Do not feed the watchdog: a crash recovers to the stock firmware on reset.
    release_to_stock()
}
