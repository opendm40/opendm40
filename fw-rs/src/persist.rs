//! Settings kept in the 4 KiB backup SRAM (0x40024000): it survives resets and,
//! with the backup domain powered, power-off. (The stock firmware keeps its
//! user settings in config.bin on the SPI disk.) Layout: magic, version,
//! payload, CRC32; anything that does not check out falls back to defaults.

use crate::mmio::{modify32, r32, w32};

const RCU_AHB1EN: u32 = 0x4002_3830;
const RCU_APB1EN: u32 = 0x4002_3840;
const PMU_CTL: u32 = 0x4000_7000;
const PMU_CS: u32 = 0x4000_7004;
const BKP: u32 = 0x4002_4000;
const MAGIC: u32 = 0x4446_5753; // "SWFD"
const VERSION: u32 = 1;
pub const WORDS: usize = 16;

fn crc(words: &[u32]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &w in words {
        for b in w.to_le_bytes() {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            }
        }
    }
    !c
}

pub fn init() {
    unsafe {
        modify32(RCU_APB1EN, 0, 1 << 28); // PMU
        modify32(PMU_CTL, 0, 1 << 8); // BKPWEN: backup domain write access
        modify32(RCU_AHB1EN, 0, 1 << 18); // BKPSRAM clock
        modify32(PMU_CS, 0, 1 << 9); // BLDOON: keep backup SRAM on VBAT
    }
}

pub fn load() -> Option<[u32; WORDS]> {
    let rd = |i: usize| unsafe { r32(BKP + 4 * i as u32) };
    if rd(0) != MAGIC || rd(1) != VERSION {
        return None;
    }
    let mut p = [0u32; WORDS];
    for (i, w) in p.iter_mut().enumerate() {
        *w = rd(2 + i);
    }
    if crc(&p) != rd(2 + WORDS) { None } else { Some(p) }
}

pub fn save(p: &[u32; WORDS]) {
    unsafe {
        w32(BKP, MAGIC);
        w32(BKP + 4, VERSION);
        for (i, &v) in p.iter().enumerate() {
            w32(BKP + 8 + 4 * i as u32, v);
        }
        w32(BKP + 8 + 4 * WORDS as u32, crc(p));
    }
}
