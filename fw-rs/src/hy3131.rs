//! HY3131 DMM front end over bit-banged SPI (firmware/re/analysis/hy3131_v2_6.h).
//!
//! SCK = PC10, MOSI = PC12, MISO = PC11, CS = PD2 (active low). MSB first, SCK
//! idle low, CS low for the whole transfer. Command byte = (reg << 1) | rd;
//! registers auto-increment. A read has one extra SCK pulse after the command
//! byte (the datasheet's read period), then MISO is sampled with SCK low before
//! each clock pulse.

use crate::mmio::{modify32, r32, spin, w32};

const GPIOC: u32 = 0x4002_0800;
const GPIOD: u32 = 0x4002_0C00;
const MODER: u32 = 0x00;
const OTYPER: u32 = 0x04;
const PUPD: u32 = 0x0C;
const IDR: u32 = 0x10;
const BOP: u32 = 0x18;

const SCK: u32 = 10; // PC10
const MISO: u32 = 11; // PC11
const MOSI: u32 = 12; // PC12
const CS: u32 = 2; // PD2

/// Half SCK period in spin iterations (~1 us at 200 MHz, well inside the
/// HY3131 SPI timing).
const HALF: u32 = 40;

pub const R_AD1: u8 = 0x00;
pub const R_INTF: u8 = 0x1E;
pub const R_INTE: u8 = 0x1F;
pub const R_CFG: u8 = 0x20; // R20..R35: 22 bytes of mode configuration
pub const CFG_LEN: usize = 22;

pub const INTF_AD1: u8 = 1 << 2;
pub const INTF_AD2: u8 = 1 << 1;
pub const INTF_LPF: u8 = 1 << 3;
pub const INTF_RMS: u8 = 1 << 4;
pub const INTF_CT: u8 = 1 << 0;

/// R26 bit 7 / R34 bit 7: enable AD2 / AD3.
const ENAD2: u8 = 0x80;
const ENAD3: u8 = 0x80;

#[inline(always)]
fn set(port: u32, pin: u32, high: bool) {
    let bit = if high { 1 << pin } else { 1 << (pin + 16) };
    unsafe { w32(port + BOP, bit) };
}

#[inline(always)]
fn miso() -> bool {
    (unsafe { r32(GPIOC + IDR) } & (1 << MISO)) != 0
}

fn out_pin(port: u32, pin: u32) {
    unsafe {
        modify32(port + MODER, 3 << (2 * pin), 1 << (2 * pin));
        modify32(port + OTYPER, 1 << pin, 0);
        modify32(port + PUPD, 3 << (2 * pin), 0);
    }
}

/// Configure the four bus pins (other pins on these ports are left alone).
pub fn init_pins() {
    set(GPIOD, CS, true);
    set(GPIOC, SCK, false);
    out_pin(GPIOC, SCK);
    out_pin(GPIOC, MOSI);
    out_pin(GPIOD, CS);
    unsafe { modify32(GPIOC + MODER, 3 << (2 * MISO), 0) }; // input
}

fn clock() {
    set(GPIOC, SCK, true);
    spin(HALF);
    set(GPIOC, SCK, false);
    spin(HALF);
}

fn tx(b: u8) {
    for i in (0..8).rev() {
        set(GPIOC, MOSI, b & (1 << i) != 0);
        spin(HALF);
        clock();
    }
}

fn rx() -> u8 {
    let mut b = 0u8;
    for _ in 0..8 {
        b = (b << 1) | miso() as u8;
        clock();
    }
    b
}

fn begin() {
    set(GPIOC, SCK, false);
    set(GPIOD, CS, false);
    spin(HALF);
}

fn end() {
    spin(HALF);
    set(GPIOD, CS, true);
    set(GPIOC, SCK, false);
    spin(HALF);
}

pub fn write(reg: u8, data: &[u8]) {
    begin();
    tx(reg << 1);
    for &b in data {
        tx(b);
    }
    end();
}

pub fn read(reg: u8, buf: &mut [u8]) {
    begin();
    tx((reg << 1) | 1);
    clock(); // read period
    for b in buf.iter_mut() {
        *b = rx();
    }
    end();
}

pub fn read1(reg: u8) -> u8 {
    let mut b = [0u8];
    read(reg, &mut b);
    b[0]
}

/// Signed 24-bit little-endian value at `b[0..3]`.
pub fn s24(b: &[u8]) -> i32 {
    ((b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16) << 8) as i32 >> 8
}

/// Mode change as the stock driver does it: stop AD2/AD3 and R36, then the
/// caller switches relays, then the 22 configuration bytes, then INTE = 0.
pub fn quiesce() {
    write(0x26, &[0]);
    write(0x34, &[0]);
    write(0x36, &[0]);
}

pub fn configure(cfg: &[u8; CFG_LEN]) {
    let (r26, r34) = (cfg[6], cfg[20]);
    if r26 & ENAD2 != 0 && r34 & ENAD3 != 0 {
        // Every AC row (f0, f2, f4, f5_6) runs the RMS converter on
        // sum(AD2 * AD3) / N (R34 SVXI). With the row written as is, ENAD2
        // (R26) is set some 14 bytes before ENAD3 (R34), and when AD3 then
        // starts in a bad phase of the shared AD2/AD3 decimation (period
        // OSR2 / FAD2CLK = 13 us) sqrt(RMS) beats against the signal for as
        // long as the row stays: +-11 % on the 40 Hz PWM at AC V r1, both in
        // plain AC and in AC+DC (docs/acdc-stock-vs-fwrs.md, "Read-path
        // investigation"). Enabling AD3 first and AD2 last was clean for
        // every skew tried (0..15 us), so the row goes out with both off
        // and they are switched on in that order.
        let mut c = *cfg;
        c[6] = r26 & !ENAD2;
        c[20] = r34 & !ENAD3;
        write(R_CFG, &c);
        write(R_INTE, &[0]);
        write(0x34, &[r34]);
        write(0x26, &[r26]);
    } else {
        write(R_CFG, cfg);
        write(R_INTE, &[0]);
    }
}

// -------------------------------------------------------- data-logger fast AD1
//
// Fast-stream support for the data logger (FR-38, src/logger.rs). The HY3131
// conversion rate for the main (AD1) converter is set by R22 AD1OSR:
//   R22 = AD1OS[7:5] AD1CHOP[4:3] AD1OSR[2:0]   (hy3131_v2_6.h, datasheet p.17)
//   OSR = 256 << (AD1OSR)        sample/conversion interval ~= OSR / SYSCLK
// Every DC-mode config row (f1 DCV, f3 DCmV, f7/f8_9 DCA) writes R22 = 0x15, i.e.
// AD1OS=000, AD1CHOP=10, AD1OSR=101 (OSR 8192); stock only ever rewrites the low
// three bits at run time (0x13=OSR2048 its fastest, 0x15=OSR8192, 0x17=OSR32768).
// So the logger keeps the same 0x10 base and varies only AD1OSR:
//   code 0 -> OSR 256 (~52 us)   ... code 3 -> 0x13 OSR 2048 (~417 us, stock's
//   fastest) ... code 7 -> 0x17 OSR 32768. SYSCLK = 4.9152 MHz.
// Practical floor is the bit-banged SPI link, not the ADC: with HALF ~1 us a
// read1(INTF) is ~45 us and a 3-byte AD1 read ~77 us, so ~122 us/sample before
// any loop work. OSR 256/512 convert faster than the link can read them; OSR
// 2048 (0x13) is the realistic default and is already proven by the stock fw.

/// AD1OSR run-time code (0..7); OSR = 256 << code. Default logger value is 3.
pub const AD1OSR_STREAM: u8 = 3; // R22 = 0x13, OSR 2048, ~417 us/conv

/// Set only the AD1OSR field of R22, keeping the stock DC base (AD1OS=000,
/// AD1CHOP=10). `code` is clamped to 0..7. Lower = faster, fewer effective bits.
pub fn set_ad1_osr(code: u8) {
    write(0x22, &[0x10 | (code & 7)]);
}

/// Nominal conversion interval in microseconds for an AD1OSR code, from the
/// datasheet-inferred `OSR / SYSCLK` (SYSCLK = 4.9152 MHz). This is the ADC
/// pacing only; the real sample interval is max(this, SPI+loop time).
pub fn osr_period_us(code: u8) -> u32 {
    let osr = 256u32 << (code & 7);
    ((osr as u64 * 1_000_000) / 4_915_200) as u32
}

/// True when AD1 has a fresh conversion (INTF AD1F). Reading the AD1 data
/// registers afterwards clears the flag, so this paces the logger at the
/// conversion rate without an external timer.
pub fn ad1_ready() -> bool {
    read1(R_INTF) & INTF_AD1 != 0
}

/// Read the current signed 24-bit AD1 conversion (R00..R02). Clears AD1F.
pub fn read_ad1() -> i32 {
    let mut b = [0u8; 3];
    read(R_AD1, &mut b);
    s24(&b)
}
