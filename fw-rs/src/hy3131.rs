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
    write(R_CFG, cfg);
    write(R_INTE, &[0]);
}
