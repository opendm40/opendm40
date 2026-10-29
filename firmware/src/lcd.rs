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
    if defer_on() {
        if mem_cmd == CMD_RAMRD {
            defer_flush();
        } else {
            defer_touch(x0, y0, x1, y1);
        }
    }
    window_raw(x0, y0, x1, y1, mem_cmd);
}

fn window_raw(x0: u16, y0: u16, x1: u16, y1: u16, mem_cmd: u16) {
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
    if defer_on() && defer_fill(x, y, w, h, color) {
        return;
    }
    set_window(x, y, x + w - 1, y + h - 1);
    fill_n(w as u32 * h as u32, color);
}

/// `fill_rect` past any batch (the panic bar: no deferred-fill frames on
/// the panic path's stack).
pub fn fill_rect_raw(x: u16, y: u16, w: u16, h: u16, color: Color) {
    window_raw(x, y, x + w - 1, y + h - 1, CMD_RAMWR);
    fill_n(w as u32 * h as u32, color);
}

fn fill_n(count: u32, color: Color) {
    let mut i = 0u32;
    while i < count {
        data(color);
        i += 1;
    }
}

// ------------------------------------------------------------ deferred fills
//
// A page is drawn as "clear the background, then draw the widgets" with no
// frame buffer, so the panel showed the bare background for a refresh or
// more and then the widgets one by one (emu/lcdtrace.py). While a batch is
// open (`defer_start` .. `defer_flush`), a large fill (`DEFER_MIN` px) is
// not drawn at once: it is remembered as a layer, and its pixels are
// written only when needed: the 8 x 8 px cells under any later drawing
// window get the layers' colours just before that drawing, and the cells
// nothing drew over get them at `defer_flush`. Every pixel still gets the
// same writes in the same order, so the final picture is the same; until a
// widget is drawn the panel keeps showing the old screen there instead of
// the bare background. A fill over cells already drawn on in the batch
// goes there at once; a layer that does not fit (`DEFER_N`) is drawn as
// any other window. Read-back (RAMRD: screenshots) flushes first.

const CELL: u16 = 8; // 60 cell columns: one u64 per cell row
const CH: usize = (H / CELL) as usize; // 40 cell rows
const DEFER_N: usize = 6;
const DEFER_MIN: u32 = 2048;

struct Defer {
    on: bool,
    n: u8,
    /// Bit r: cell row r has waiting cells.
    rows: u64,
    /// Cell rows: bit c = cell (c, row) still waits for the layers.
    pend: [u64; CH],
    /// Cell rows: bit c = drawn on in this batch.
    drawn: [u64; CH],
    /// x, y, w, h, colour, in the order drawn.
    layer: [(u16, u16, u16, u16, Color); DEFER_N],
}

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut DEFER: Defer = Defer { on: false, n: 0, rows: 0, pend: [0; CH], drawn: [0; CH], layer: [(0, 0, 0, 0, 0); DEFER_N] };

#[inline(always)]
fn defer() -> &'static mut Defer {
    // Main loop only (no ISR draws on the panel).
    unsafe { &mut *core::ptr::addr_of_mut!(DEFER) }
}

#[inline(always)]
fn defer_on() -> bool {
    defer().on
}

/// Open a batch: large fills from now on wait for `defer_flush` (or for a
/// drawing over them). Nested starts are one batch.
pub fn defer_start() {
    let d = defer();
    if !d.on {
        d.on = true;
        d.n = 0;
        d.rows = 0;
        d.drawn = [0; CH];
    }
}

/// Close the batch: what the layers still cover is drawn, top to bottom.
pub fn defer_flush() {
    let d = defer();
    if !d.on {
        return;
    }
    if d.rows != 0 {
        cells_run(0, CH - 1, u64::MAX);
    }
    d.on = false;
    d.n = 0;
}

/// Cells of columns x0..=x1 (x0 <= x1 < W).
#[inline(always)]
fn cell_mask(x0: u16, x1: u16) -> u64 {
    let (c0, c1) = (x0 / CELL, x1 / CELL);
    let hi = if c1 >= 63 { u64::MAX } else { (1u64 << (c1 + 1)) - 1 };
    hi & !((1u64 << c0) - 1)
}

/// Rows r0..=r1 (r1 < CH) as summary bits.
#[inline(always)]
fn row_mask(r0: usize, r1: usize) -> u64 {
    ((1u64 << (r1 + 1)) - 1) & !((1u64 << r0) - 1)
}

/// A large fill: a layer (true), or false = draw it now.
#[inline(never)]
fn defer_fill(x: u16, y: u16, w: u16, h: u16, color: Color) -> bool {
    let d = defer();
    if (w as u32) * (h as u32) < DEFER_MIN || d.n as usize >= DEFER_N || x >= W || y >= H {
        return false;
    }
    let (x1, y1) = ((x + w - 1).min(W - 1), (y + h - 1).min(H - 1));
    let m = cell_mask(x, x1);
    let (r0, r1) = ((y / CELL) as usize, (y1 / CELL) as usize);
    // Cells already drawn on in this batch show what is drawn now: the
    // fill goes there at once (only where it covers them).
    let mut r = r0;
    while r <= r1 {
        let live = d.drawn[r] & m;
        let mut e = r;
        while e < r1 && d.drawn[e + 1] & m == live {
            e += 1;
        }
        if live != 0 {
            let ry0 = (r as u16 * CELL).max(y);
            let ry1 = ((e as u16 + 1) * CELL - 1).min(y1);
            for_runs(live, |a, b| {
                let (rx0, rx1) = ((a * CELL).max(x), ((b + 1) * CELL - 1).min(x1));
                window_raw(rx0, ry0, rx1, ry1, CMD_RAMWR);
                fill_n((rx1 - rx0 + 1) as u32 * (ry1 - ry0 + 1) as u32, color);
            });
        }
        let wait = m & !live;
        for q in &mut d.pend[r..=e] {
            *q |= wait;
        }
        if wait != 0 {
            d.rows |= row_mask(r, e);
        }
        r = e + 1;
    }
    d.layer[d.n as usize] = (x, y, x1 - x + 1, y1 - y + 1, color);
    d.n += 1;
    true
}

/// A drawing window: the waiting cells under it get the layers first.
#[inline(never)]
fn defer_touch(x0: u16, y0: u16, x1: u16, y1: u16) {
    if x0 >= W || y0 >= H {
        return;
    }
    let (x1, y1) = (x1.min(W - 1), y1.min(H - 1));
    let (r0, r1, m) = ((y0 / CELL) as usize, (y1 / CELL) as usize, cell_mask(x0, x1));
    let d = defer();
    if d.rows & row_mask(r0, r1) != 0 {
        cells_run(r0, r1, m);
    }
    for q in &mut d.drawn[r0..=r1] {
        *q |= m;
    }
}

/// Draw the layers into the waiting cells of rows r0..=r1 under `m`, rows
/// with the same cells in one window per run.
fn cells_run(r0: usize, r1: usize, m: u64) {
    let d = defer();
    let mut r = r0;
    while r <= r1 {
        let p = d.pend[r] & m;
        if p == 0 {
            r += 1;
            continue;
        }
        let mut e = r;
        while e < r1 && d.pend[e + 1] & m == p {
            e += 1;
        }
        for k in r..=e {
            d.pend[k] &= !p;
            if d.pend[k] == 0 {
                d.rows &= !(1u64 << k);
            }
        }
        let (ry0, ry1) = (r as u16 * CELL, (e as u16 + 1) * CELL - 1);
        let n = d.n as usize;
        for_runs(p, |a, b| {
            let (rx0, rx1) = (a * CELL, (b + 1) * CELL - 1);
            for &(lx, ly, lw, lh, c) in &d.layer[..n] {
                let (ax, ay) = (lx.max(rx0), ly.max(ry0));
                let (bx, by) = ((lx + lw - 1).min(rx1), (ly + lh - 1).min(ry1));
                if ax <= bx && ay <= by {
                    window_raw(ax, ay, bx, by, CMD_RAMWR);
                    fill_n((bx - ax + 1) as u32 * (by - ay + 1) as u32, c);
                }
            }
        });
        r = e + 1;
    }
}

/// Runs of set bits in `m`: f(first cell, last cell).
#[inline(always)]
fn for_runs(mut m: u64, mut f: impl FnMut(u16, u16)) {
    while m != 0 {
        let a = m.trailing_zeros() as u16;
        let len = (!(m >> a)).trailing_zeros() as u16; // 64 when all ones
        let b = a + len - 1;
        f(a, b);
        m = if b >= 63 { 0 } else { m & !((1u64 << (b + 1)) - 1) };
    }
}

pub fn fill_screen(color: Color) {
    fill_rect(0, 0, W, H, color);
}
