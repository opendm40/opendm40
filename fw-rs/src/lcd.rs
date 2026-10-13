//! ILI9488 over FSMC bank 1, as wired on the DM40.
//!
//! Command register 0x6001FFFE (RS=0 via FSMC_A16), data register 0x60020000.
//! 16-bit 8080 bus, panel configured for 16 bpp RGB565 -> one 16-bit data write
//! per pixel. Addresses and the window/pixel protocol come from the stock
//! firmware (FUN_0800a0e4 init, FUN_0800a5ca window, FUN_0800a89a pixel).
//!
//! Milestone 1 reuses the FSMC/LCD setup the stock firmware already performed
//! before handing control over; it does not re-init the panel yet.

use crate::mmio::w16;

const LCD_CMD: u32 = 0x6001_FFFE;
const LCD_DATA: u32 = 0x6002_0000;

/// Landscape, as the stock multimeter screen uses it.
pub const W: u16 = 480;
pub const H: u16 = 320;

pub type Color = u16; // RGB565

pub const BLACK: Color = 0x0000;
pub const WHITE: Color = 0xFFFF;
pub const RED: Color = 0xF800;

#[inline(always)]
fn cmd(c: u16) {
    unsafe { w16(LCD_CMD, c) };
}

#[inline(always)]
fn data(d: u16) {
    unsafe { w16(LCD_DATA, d) };
}

/// Set the drawing window and leave the controller ready for pixel writes (0x2C).
pub fn set_window(x0: u16, y0: u16, x1: u16, y1: u16) {
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
    cmd(0x2C);
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
