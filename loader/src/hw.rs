//! Register access, delays, GPIO set-up. The only unsafe in the MMIO path.
use core::ptr::{read_volatile, write_volatile};

#[inline(always)]
pub fn r(a: u32) -> u32 {
    unsafe { read_volatile(a as *const u32) }
}
#[inline(always)]
pub fn w(a: u32, v: u32) {
    unsafe { write_volatile(a as *mut u32, v) }
}
#[inline(always)]
pub fn w16(a: u32, v: u16) {
    unsafe { write_volatile(a as *mut u16, v) }
}
#[inline(always)]
pub fn r8(a: u32) -> u8 {
    unsafe { read_volatile(a as *const u8) }
}
#[inline(never)]
pub fn m(a: u32, clear: u32, set: u32) {
    w(a, (r(a) & !clear) | set)
}
pub fn set(a: u32, bits: u32) {
    m(a, 0, bits)
}

/// `n` turns of a loop with a volatile access.
#[inline(never)]
pub fn spin(n: u32) {
    let mut c = 0u32;
    let p = core::ptr::addr_of_mut!(c);
    let mut i = 0;
    while i < n {
        unsafe { write_volatile(p, i) };
        i += 1;
    }
}
/// About a millisecond per unit at 96 MHz.
pub fn ms(n: u32) {
    spin(n * 14000)
}

/// Bounded poll: true when `a & mask == want`.
#[inline(never)]
pub fn wait(a: u32, mask: u32, want: u32) -> bool {
    let mut n = 0x8_0000u32;
    while n > 0 {
        if r(a) & mask == want {
            return true;
        }
        n -= 1;
    }
    false
}

pub const GPIOA: u32 = 0x4002_0000;
pub const GPIOE: u32 = 0x4002_1000;
pub const BOP: u32 = 0x18;
pub const RCU_AHB1EN: u32 = 0x4002_3830;
pub const RCU_AHB2EN: u32 = 0x4002_3834;
pub const RCU_AHB3EN: u32 = 0x4002_3838;
pub const RCU_APB1EN: u32 = 0x4002_3840;
pub const RCU_APB2EN: u32 = 0x4002_3844;

/// mode 0 in, 1 out, 2 AF; pull 0 none, 1 up. Bit 16 of `port_flags`-free: open drain by `od`.
#[inline(never)]
pub fn pins(port: u32, mask: u32, mode: u32, pull: u32, af: u32, od: bool) {
    let mut p = 0;
    while p < 16 {
        if mask & (1 << p) != 0 {
            let (reg, sh) = if p < 8 { (0x20, 4 * p) } else { (0x24, 4 * (p - 8)) };
            m(port + reg, 0xF << sh, af << sh);
            m(port + 4, 1 << p, (od as u32) << p);
            m(port + 8, 3 << (2 * p), 3 << (2 * p));
            m(port + 0xC, 3 << (2 * p), pull << (2 * p));
            m(port, 3 << (2 * p), mode << (2 * p));
        }
        p += 1;
    }
}

/// Free-running cycle counter (96 MHz in loader mode).
pub const CYC_MS: u32 = 96_000;
pub fn timer_init() {
    set(0xE000_EDFC, 1 << 24);
    set(0xE000_1000, 1);
}
#[inline(always)]
pub fn now() -> u32 {
    r(0xE000_1004)
}

/// All loader-mode pins but PA2: port index, mode | pull << 2 | open drain << 3, AF, mask.
static PINS: [(u8, u8, u8, u16); 10] = [
    (3, 2 | 1 << 2, 12, 0xCFB3), // FSMC: PD0 1 4 5 7 8 9 10 11 14 15
    (4, 2 | 1 << 2, 12, 0xFF80), // FSMC: PE7..15
    (4, 1, 0, 0x54),             // PE2 hold, PE4 SPI CS, PE6 touch reset
    (0, 1, 0, 1 << 6),           // PA6 backlight
    (3, 2, 5, 1 << 3),           // SPI SCK PD3
    (2, 2, 5, 3 << 2),           // SPI MISO PC2, MOSI PC3
    (4, 1 << 2, 0, 1 << 5),      // touch INT PE5
    (0, 2 | 1 << 2 | 1 << 3, 4, 1 << 8), // touch SCL PA8
    (2, 2 | 1 << 2 | 1 << 3, 4, 1 << 9), // touch SDA PC9
    (0, 2, 10, 3 << 11),         // USB PA11, PA12
];

pub fn pins_all() {
    set(RCU_AHB1EN, 0x1F);
    w(GPIOE + BOP, 1 << 2 | 1 << 4 | 1 << 22);
    for &(port, f, af, mask) in PINS.iter() {
        pins(GPIOA + 0x400 * port as u32, mask as u32, (f & 3) as u32, (f >> 2 & 1) as u32, af as u32, f & 8 != 0);
    }
}

pub fn sysreset() -> ! {
    w(0xE000_ED0C, 0x05FA_0004);
    loop {}
}
