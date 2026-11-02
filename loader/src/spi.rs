//! W25Q128 on SPI1 (0x40003800), SCK PD3, MISO PC2, MOSI PC3, CS PE4.
use crate::hw::*;

const CTL0: u32 = 0x4000_3800;
const STAT: u32 = 0x4000_3808;
const DATA: u32 = 0x4000_380C;

#[inline(never)]
pub fn init() {
    set(RCU_APB1EN, 1 << 14);
    w(CTL0, 0x030F); // mode 3, master, soft NSS, APB1 / 4 = 6 MHz
    w(CTL0, 0x034F);
    let _ = r(DATA);
}

#[inline(always)]
fn cs(on: bool) {
    w(GPIOE + BOP, if on { 1 << 20 } else { 1 << 4 });
}

#[inline(never)]
fn x(b: u8) -> u8 {
    while r(STAT) & 2 == 0 {}
    w16(DATA, b as u16);
    while r(STAT) & 1 == 0 {}
    r(DATA) as u8
}

#[inline(never)]
fn cmd(c: u8, a: u32) {
    busy();
    cs(true);
    x(c);
    x((a >> 16) as u8);
    x((a >> 8) as u8);
    x(a as u8);
}

fn busy() {
    let mut n = 0x40_0000u32;
    cs(true);
    x(5);
    while n > 0 && x(0xFF) & 1 != 0 {
        n -= 1;
    }
    cs(false);
}

fn wren() {
    busy();
    cs(true);
    x(6);
    cs(false);
}

pub fn read(a: u32, d: &mut [u8]) {
    cmd(3, a);
    for b in d.iter_mut() {
        *b = x(0xFF);
    }
    cs(false);
}

pub fn erase(a: u32) {
    wren();
    cmd(0x20, a);
    cs(false);
}

pub fn program(a: u32, d: &[u8]) {
    wren();
    cmd(2, a);
    for &b in d {
        x(b);
    }
    cs(false);
}

pub fn read_u16(a: u32) -> u16 {
    let mut b = [0u8; 2];
    read(a, &mut b);
    u16::from_le_bytes(b)
}
