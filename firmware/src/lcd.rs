//! ILI9488 over FSMC bank 1, as wired on the DM40.
//!
//! Command register 0x6001FFFE (RS=0 via FSMC_A16), data register 0x60020000.
//! 16-bit 8080 bus, panel configured for 16 bpp RGB565 -> one 16-bit data write
//! per pixel. Addresses and the window/pixel protocol come from the stock
//! firmware (FUN_0800a0e4 init, FUN_0800a5ca window, FUN_0800a89a pixel).
//!
//! Milestone 1 reuses the FSMC/LCD setup the stock firmware already performed
//! before handing control over; it does not re-init the panel yet.

use core::ptr::read_volatile;

use crate::mmio::w16;

const LCD_CMD: u32 = 0x6001_FFFE;
const LCD_DATA: u32 = 0x6002_0000;

/// Landscape, as the stock multimeter screen uses it.
pub const W: u16 = 480;
pub const H: u16 = 320;

pub type Color = u16; // RGB565

pub const BLACK: Color = 0x0000;
pub const RED: Color = 0xF800;
pub const MAGENTA: Color = 0xF81F;

#[inline(always)]
pub fn cmd(c: u16) {
    unsafe { w16(LCD_CMD, c) };
}

#[inline(always)]
pub fn data(d: u16) {
    unsafe { w16(LCD_DATA, d) };
}

/// One read cycle on the data register (RS=1). Used for register reads and the
/// GRAM read-back (0x2E) behind screenshots; the stock FSMC read timing applies.
#[inline(always)]
pub fn read_data() -> u16 {
    unsafe { read_volatile(LCD_DATA as *const u16) }
}

/// Memory access control (0x36). The stock firmware draws landscape by rotating
/// in software with MADCTL=0x48 (MV=0, native 320x480). We instead address the
/// panel in landscape directly: MV | MX | MY | BGR = 0xE8 gives a 480x320 grid
/// with (0,0) at the top left as the meter is held (0x28 is upside down; checked
/// against a read-back of the stock screen).
pub const MADCTL_LANDSCAPE: u16 = 0xE8;

pub fn set_madctl(v: u16) {
    cmd(0x36);
    data(v);
}

pub const CMD_RAMWR: u16 = 0x2C;
pub const CMD_RAMRD: u16 = 0x2E;

/// Set the drawing window and leave the controller ready for pixel writes (0x2C).
pub fn set_window(x0: u16, y0: u16, x1: u16, y1: u16) {
    set_window_cmd(x0, y0, x1, y1, CMD_RAMWR);
}

/// Set the window, then issue `mem_cmd` (RAMWR 0x2C or RAMRD 0x2E).
pub fn set_window_cmd(x0: u16, y0: u16, x1: u16, y1: u16, mem_cmd: u16) {
    cmd(0x2A);
    data(x0 >> 8);
    data(x0 & 0xFF);
    data(x1 >> 8);
    data(x1 & 0xFF);
    cmd(0x2B);
    data(y0 >> 8);
    data(y0 & 0xFF);
    data(y1 >> 8);
    data(y1 & 0xFF);
    cmd(mem_cmd);
}

/// Read `n` pixels of GRAM row `y` starting at column `x` and pass them to
/// `out` as RGB565, left to right (screenshots). With COLMOD 16 bpp the
/// ILI9488 still returns RGB666 on a read: one dummy word after RAMRD, then 3
/// bytes per pixel (R, G, B, 6 significant bits each) packed big-endian into
/// the 16-bit words (README, tools/dm40dbg.py decode()), so 2 pixels = 3 words.
/// Leaves the controller in RAMRD; the next drawing call sets its own window.
pub fn read_span(x: u16, y: u16, n: u16, mut out: impl FnMut(Color)) {
    if n == 0 {
        return;
    }
    set_window_cmd(x, y, x + n - 1, y, CMD_RAMRD);
    let _ = read_data(); // dummy
    let mut left = n;
    while left > 0 {
        let w0 = read_data();
        let w1 = read_data();
        out(rgb666_to_565(w0 >> 8, w0 & 0xFF, w1 >> 8));
        left -= 1;
        if left == 0 {
            break;
        }
        let w2 = read_data();
        out(rgb666_to_565(w1 & 0xFF, w2 >> 8, w2 & 0xFF));
        left -= 1;
    }
}

#[inline(always)]
fn rgb666_to_565(r: u16, g: u16, b: u16) -> Color {
    (r & 0xF8) << 8 | (g & 0xFC) << 3 | b >> 3
}

pub fn fill_rect(x: u16, y: u16, w: u16, h: u16, color: Color) {
    if w == 0 || h == 0 {
        return;
    }
    set_window(x, y, x + w - 1, y + h - 1);
    let count = w as u32 * h as u32;
    let mut i = 0u32;
    while i < count {
        data(color);
        i += 1;
    }
}

pub fn fill_screen(color: Color) {
    fill_rect(0, 0, W, H, color);
}
