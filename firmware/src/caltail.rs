//! The calibration tail in sector 0 (proof for the minimal loader): 0x08003CC0
//! record 800 B, 0x08003FE0 boot block 24 B, 0x08003FF8 crc | !crc << 16 over both.
//! Valid: read it. Otherwise the old addresses.
use core::ptr::read_volatile;

const TAIL: u32 = 0x0800_3CC0;
const T_BLK: u32 = TAIL + 800;
const T_CRC: u32 = TAIL + 824;
const REC: u32 = 0x080C_0000;
const BLK: u32 = 0x080A_4000;

pub fn valid() -> bool {
    let mut c = 0xFFFFu16;
    for a in TAIL..T_CRC {
        c ^= (unsafe { read_volatile(a as *const u8) } as u16) << 8;
        for _ in 0..8 {
            c = if c & 0x8000 != 0 { (c << 1) ^ 0x8005 } else { c << 1 };
        }
    }
    unsafe { read_volatile(T_CRC as *const u32) == (c as u32 | (!c as u32) << 16) }
}

pub fn rec_base() -> u32 {
    if valid() { TAIL } else { REC }
}

pub fn blk_base() -> u32 {
    if valid() { T_BLK } else { BLK }
}

/// The safety copy's source for a FILES row.
pub fn backup_source(addr: u32, len: u32) -> (u32, u32) {
    match addr {
        REC if valid() => (TAIL, 800),
        BLK if valid() => (T_BLK, 24),
        _ => (addr, len),
    }
}
