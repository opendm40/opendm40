//! RAM stub for dm40-fw: programs one chunk of the flash image into internal
//! flash sector 8 (0x08080000, unused by the stock firmware) and, once the
//! whole image is in place, starts it.
//!
//! Payload at 0x2000C000 (appended by tools/flash.py):
//!   u32 magic 'DM4F', u32 offset, u32 len, u32 crc32(chunk),
//!   u32 total image length, u32 crc32(image), then `len` bytes.
//! Flow: if flash already holds the image (crc) -> start it. A probe payload
//! (len 0) otherwise just resets back to stock. Else erase
//! the sector when offset == 0, program + verify the chunk; if it was the last
//! one start the image, else reset back to the stock firmware for the next
//! chunk. Progress is shown as a bar at the bottom of the LCD.
//! Only sector 8 is ever erased or programmed.

#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;
use core::ptr::{read_volatile, write_volatile};

global_asm!(
    r#"
    .section .vectors, "a", %progbits
    .global __VECTORS
    .balign 512
__VECTORS:
    .word _stack_top
    .word reset

    .section .text.reset, "ax", %progbits
    .thumb_func
    .global reset
reset:
    cpsid i
    ldr   sp, =_stack_top
    bl    stub_main
    b     .
"#
);

const IMAGE: u32 = 0x0808_0000;
const SECTOR: u32 = 8;
const SECTOR_LEN: u32 = 128 * 1024;
const PAYLOAD: u32 = 0x2000_C000;
const MAGIC: u32 = 0x4634_4D44; // "DM4F"

const FMC: u32 = 0x4002_3C00;
const FMC_KEY: u32 = FMC + 0x04;
const FMC_STAT: u32 = FMC + 0x0C;
const FMC_CTL: u32 = FMC + 0x10;
const STAT_BUSY: u32 = 1 << 16;
const STAT_ERR: u32 = 0xF2; // OPERR, WPERR, PGAERR, PGPERR, PGSERR
const CTL_PG: u32 = 1 << 0;
const CTL_SER: u32 = 1 << 1;
const CTL_PSZ32: u32 = 2 << 8;
const CTL_START: u32 = 1 << 16;
const CTL_LK: u32 = 1 << 31;

const LCD_CMD: u32 = 0x6001_FFFE;
const LCD_DATA: u32 = 0x6002_0000;

fn r32(a: u32) -> u32 {
    unsafe { read_volatile(a as *const u32) }
}
fn w32(a: u32, v: u32) {
    unsafe { write_volatile(a as *mut u32, v) }
}
fn kick() {
    w32(0x4000_3000, 0xAAAA);
}

fn lcd(c: u16, d: &[u16]) {
    unsafe {
        write_volatile(LCD_CMD as *mut u16, c);
        for &x in d {
            write_volatile(LCD_DATA as *mut u16, x);
        }
    }
}

/// Fill a rectangle in the landscape frame (MADCTL 0xE8, as dm40-fw).
fn rect(x: u16, y: u16, w: u16, h: u16, color: u16) {
    lcd(0x36, &[0xE8]);
    let (x1, y1) = (x + w - 1, y + h - 1);
    lcd(0x2A, &[x >> 8, x & 0xFF, x1 >> 8, x1 & 0xFF]);
    lcd(0x2B, &[y >> 8, y & 0xFF, y1 >> 8, y1 & 0xFF]);
    lcd(0x2C, &[]);
    for _ in 0..(w as u32 * h as u32) {
        unsafe { write_volatile(LCD_DATA as *mut u16, color) };
    }
}

const BLUE: u16 = 0x001F;
const YELLOW: u16 = 0xFFE0;
const GREEN: u16 = 0x07E0;
const RED: u16 = 0xF800;
const GREY: u16 = 0x4208;

fn bar(done: u32, total: u32, color: u16) {
    let w = (480 * done.min(total) / total.max(1)) as u16;
    rect(0, 300, 480, 20, GREY);
    if w > 0 {
        rect(0, 300, w, 20, color);
    }
}

fn crc32(data: impl Iterator<Item = u8>) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

fn flash_bytes(off: u32, len: u32) -> impl Iterator<Item = u8> {
    (0..len).map(move |i| unsafe { read_volatile((IMAGE + off + i) as *const u8) })
}

fn wait() -> Result<(), u32> {
    while r32(FMC_STAT) & STAT_BUSY != 0 {
        kick();
    }
    let s = r32(FMC_STAT);
    w32(FMC_STAT, s & (STAT_ERR | 1)); // clear END / errors
    if s & STAT_ERR != 0 { Err(s) } else { Ok(()) }
}

fn unlock() {
    if r32(FMC_CTL) & CTL_LK != 0 {
        w32(FMC_KEY, 0x4567_0123);
        w32(FMC_KEY, 0xCDEF_89AB);
    }
}

fn lock() {
    w32(FMC_CTL, CTL_LK);
}

fn erase() -> Result<(), u32> {
    wait()?;
    w32(FMC_CTL, CTL_PSZ32 | (SECTOR << 3) | CTL_SER);
    w32(FMC_CTL, CTL_PSZ32 | (SECTOR << 3) | CTL_SER | CTL_START);
    let r = wait();
    w32(FMC_CTL, 0);
    r
}

fn program(off: u32, data: u32, len: u32) -> Result<(), u32> {
    wait()?;
    w32(FMC_CTL, CTL_PSZ32 | CTL_PG);
    let mut i = 0;
    while i < len {
        let w = r32(data + i);
        w32(IMAGE + off + i, w);
        wait()?;
        i += 4;
    }
    w32(FMC_CTL, 0);
    Ok(())
}

fn start_image() -> ! {
    let sp = r32(IMAGE);
    let pc = r32(IMAGE + 4);
    unsafe {
        core::arch::asm!(
            "msr msp, {sp}",
            "bx {pc}",
            sp = in(reg) sp,
            pc = in(reg) pc,
            options(noreturn)
        );
    }
}

fn reset_to_stock() -> ! {
    w32(0xE000_ED0C, 0x05FA_0004);
    loop {}
}

fn fail(_code: u32) -> ! {
    rect(0, 300, 480, 20, RED);
    for _ in 0..40_000_000u32 {
        unsafe { core::arch::asm!("nop") };
    }
    reset_to_stock()
}

#[no_mangle]
extern "C" fn stub_main() -> ! {
    kick();
    let magic = r32(PAYLOAD);
    let off = r32(PAYLOAD + 4);
    let len = r32(PAYLOAD + 8);
    let crc = r32(PAYLOAD + 12);
    let total = r32(PAYLOAD + 16);
    let image_crc = r32(PAYLOAD + 20);
    let data = PAYLOAD + 24;

    if magic != MAGIC || total == 0 || total > SECTOR_LEN || off + len > total || off % 4 != 0 {
        fail(1);
    }
    // Already installed: just start it.
    if crc32(flash_bytes(0, total)) == image_crc {
        bar(1, 1, GREEN);
        start_image();
    }
    // Probe pass (no data): the image is not installed, go back to stock.
    if len == 0 {
        reset_to_stock();
    }
    let chunk = (0..len).map(|i| unsafe { read_volatile((data + i) as *const u8) });
    if crc32(chunk) != crc {
        fail(2);
    }
    unlock();
    if off == 0 {
        bar(1, 1, BLUE);
        if let Err(e) = erase() {
            lock();
            fail(e);
        }
    }
    bar(off, total, YELLOW);
    let res = program(off, data, (len + 3) & !3);
    lock();
    if let Err(e) = res {
        fail(e);
    }
    if (0..len).any(|i| unsafe { read_volatile((data + i) as *const u8) != read_volatile((IMAGE + off + i) as *const u8) }) {
        fail(3);
    }
    bar(off + len, total, YELLOW);
    if off + len >= total {
        if crc32(flash_bytes(0, total)) != image_crc {
            fail(4);
        }
        bar(1, 1, GREEN);
        start_image();
    }
    reset_to_stock()
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    fail(0xFF)
}
