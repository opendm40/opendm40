//! The primary bootloader's job: clock, PA2 gate, marker + body CRC, launch.
use crate::hw::*;

pub const APP: u32 = 0x0800_4000;
pub const MARKER: u32 = 0x0800_8000;
/// Body limit: 0x08004000..0x08100000, sectors 1..11.
pub const MAX_BODY: u32 = 0x10_0000 - 0x4000;

/// HXTAL / 25 x 192 / 2 = 96 MHz, Q = 4: 48 MHz for USB without PLLSAI or the
/// over-drive handshake. AHB /1, APB2 /2, APB1 /4 (24 MHz). A dead crystal stays on IRC16M.
pub fn clock() {
    const CTL: u32 = 0x4002_3800;
    const PLL: u32 = 0x4002_3804;
    const CFG0: u32 = 0x4002_3808;
    // Off the stock ROM's PLL first: its registers only change while it is off.
    set(CTL, 1);
    wait(CTL, 2, 2);
    w(CFG0, 0);
    wait(CFG0, 0xC, 0);
    m(CTL, 1 << 24, 0);
    wait(CTL, 1 << 25, 0);
    set(CTL, 1 << 16);
    if wait(CTL, 1 << 17, 1 << 17) {
        set(CFG0, 0x9400);
        w(PLL, 0x2440_3019);
        set(CTL, 1 << 24);
        if wait(CTL, 1 << 25, 1 << 25) {
            set(CFG0, 2);
            wait(CFG0, 0xC, 8);
        }
    }
}

pub fn crc(mut c: u16, b: u8) -> u16 {
    c ^= (b as u16) << 8;
    let mut i = 0;
    while i < 8 {
        c = if c & 0x8000 != 0 { (c << 1) ^ 0x8005 } else { c << 1 };
        i += 1;
    }
    c
}

/// Type, complement and CRC of a 12-byte header (flash marker or decoded file).
pub fn header_ok(h: &[u8; 12]) -> bool {
    let lo = u16::from_le_bytes([h[0], h[1]]);
    let hi = u16::from_le_bytes([h[2], h[3]]);
    let mut c = 0xFFFF;
    let mut i = 4;
    while i < 12 {
        c = crc(c, h[i]);
        i += 1;
    }
    h[10] == 1 && h[11] == 0 && lo == !hi && c == lo && body_len(h) <= MAX_BODY
}

pub fn body_len(h: &[u8; 12]) -> u32 {
    u32::from_le_bytes([h[4], h[5], h[6], h[7]])
}

/// CRC-16 of a flash range.
pub fn crc_at(mut c: u16, a: u32, n: u32) -> u16 {
    let mut o = 0;
    while o < n {
        c = crc(c, r8(a + o));
        o += 1;
    }
    c
}

/// CRC of the flashed body without the 12 marker bytes == low half of word 2.
pub fn body_ok(h: &[u8; 12]) -> bool {
    let mut c = 0xFFFF;
    let mut o = 0;
    while o < body_len(h) {
        if !(0x4000..0x400C).contains(&o) {
            c = crc(c, r8(APP + o));
        }
        o += 1;
    }
    c == u16::from_le_bytes([h[8], h[9]])
}

pub fn marker() -> [u8; 12] {
    let mut h = [0u8; 12];
    let mut i = 0;
    while i < 12 {
        h[i] = r8(MARKER + i as u32);
        i += 1;
    }
    h
}

fn pa2_high() -> bool {
    set(RCU_AHB1EN, 1);
    pins(GPIOA, 1 << 2, 0, 1, 0, false);
    spin(20000);
    let mut n = 0;
    while n < 1000 {
        if r(GPIOA + 0x10) & (1 << 2) != 0 {
            return true;
        }
        spin(20000);
        n += 1;
    }
    false
}

pub fn app_valid() -> bool {
    let h = marker();
    header_ok(&h) && body_ok(&h)
}

/// The first look at PA2. Hook build: the stock routine at 0x080005F4 itself,
/// so the window is the stock one (it returns 0 when PA2 was high).
pub fn first_poll() -> bool {
    #[cfg(feature = "hook")]
    {
        let stock: extern "C" fn() -> u32 = unsafe { core::mem::transmute(0x0800_05F5usize) };
        stock() == 0
    }
    #[cfg(not(feature = "hook"))]
    pa2_high()
}

/// Launches a valid application when PA2 was high in the first poll or is in
/// this second one (the FS-loader's second poll in the stock chain); returns
/// otherwise.
pub fn gate(first: bool) {
    if first || pa2_high() {
        if app_valid() {
            let mut i = 0;
            while i < 8 {
                w(0xE000_E180 + i, !0);
                w(0xE000_E280 + i, !0);
                i += 4;
            }
            unsafe {
                core::arch::asm!("msr msp, {}", "cpsie i", "bx {}", in(reg) r(APP), in(reg) r(APP + 4), options(noreturn));
            }
        }
    }
}
