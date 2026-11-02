//! Minimal DM40 loader prototype: primary bootloader + FS-loader in sector 0.
#![no_std]
#![no_main]

mod boot;
mod fat;
mod hw;
mod install;
mod lcd;
mod spi;
mod tail;
mod touch;
mod ui;
mod usb;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    hw::sysreset()
}

#[cfg(not(feature = "hook"))]
mod standalone {
    #[derive(Clone, Copy)]
    union Vector {
        f: unsafe extern "C" fn() -> !,
        v: *const u8,
    }
    unsafe impl Sync for Vector {}

    extern "C" {
        static _stack_top: u8;
    }

    unsafe extern "C" fn fault() -> ! {
        crate::hw::sysreset()
    }

    /// All core exceptions reset: nothing here uses interrupts.
    #[link_section = ".vectors"]
    #[used]
    static VECTORS: [Vector; 16] = {
        let f = Vector { f: fault };
        let mut t = [f; 16];
        t[0] = Vector { v: core::ptr::addr_of!(_stack_top) };
        t[1] = Vector { f: super::loader_main };
        t
    };
}

// Entered by a BL from the stock main; never returns to it.
#[cfg(feature = "hook")]
core::arch::global_asm!(
    ".section .entry,\"ax\",%progbits",
    ".thumb_func",
    ".global hook_entry",
    "hook_entry:",
    "cpsid i",
    "movw r0, #0x2000",
    "movt r0, #0x2000",
    "mov sp, r0",
    "b loader_main",
    ".text",
);

#[no_mangle]
unsafe extern "C" fn loader_main() -> ! {
    let first = boot::first_poll();
    tail::migrate();
    boot::clock();
    boot::gate(first);
    ui::run()
}

// Byte loops: nothing here copies enough for speed to matter. In asm, as LLVM
// would turn the same Rust loops back into calls to themselves.
core::arch::global_asm!(
    ".text",
    ".global memcpy",
    ".thumb_func",
    "memcpy:",
    ".global __aeabi_memcpy",
    ".thumb_func",
    "__aeabi_memcpy:",
    ".global __aeabi_memcpy4",
    ".thumb_func",
    "__aeabi_memcpy4:",
    ".global __aeabi_memcpy8",
    ".thumb_func",
    "__aeabi_memcpy8:",
    "subs r2, #1",
    "bcc 8f",
    "ldrb r3, [r1, r2]",
    "strb r3, [r0, r2]",
    "b memcpy",
    "8: bx lr",
    ".global __aeabi_memclr",
    ".thumb_func",
    "__aeabi_memclr:",
    ".global __aeabi_memclr4",
    ".thumb_func",
    "__aeabi_memclr4:",
    ".global __aeabi_memclr8",
    ".thumb_func",
    "__aeabi_memclr8:",
    "movs r2, #0",
    ".global __aeabi_memset",
    ".thumb_func",
    "__aeabi_memset:",
    ".global __aeabi_memset4",
    ".thumb_func",
    "__aeabi_memset4:",
    ".global __aeabi_memset8",
    ".thumb_func",
    "__aeabi_memset8:",
    "subs r1, #1",
    "bcc 8f",
    "strb r2, [r0, r1]",
    "b __aeabi_memset",
    "8: bx lr",
    ".global memset",
    ".thumb_func",
    "memset:",
    "subs r2, #1",
    "bcc 8f",
    "strb r1, [r0, r2]",
    "b memset",
    "8: bx lr",
);
