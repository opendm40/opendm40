//! The fixed tail of sector 0: a copy of the calibration record (0x080C0000) and
//! the boot block (0x080A4000), so that sectors 9..11 can hold the application.
//! 0x08003CC0 record 800 B, 0x08003FE0 block 24 B, 0x08003FF8 crc | !crc << 16.
use crate::boot::crc_at;
use crate::hw::*;
use crate::install::{lock, prog, unlock};

pub const TAIL: u32 = 0x0800_3CC0;
const T_BLK: u32 = TAIL + 800;
const T_CRC: u32 = TAIL + 824;
pub const REC: u32 = 0x080C_0000;
pub const BLK: u32 = 0x080A_4000;

fn word(c: u16) -> u32 {
    c as u32 | (!c as u32) << 16
}

pub fn ok() -> bool {
    r(T_CRC) == word(crc_at(0xFFFF, TAIL, 824))
}

/// Record CRC words + version, as the stock reads it.
pub fn rec_ok() -> bool {
    r(REC) == word(crc_at(0xFFFF, REC + 4, 796)) && r(REC + 4) == 1
}

/// Word copy, 1->0 only; words already right are not programmed again. FMC unlocked.
pub fn copy(dst: u32, src: u32, n: u32) -> bool {
    let mut o = 0;
    while o < n {
        let (d, v) = (r(dst + o), r(src + o));
        if d != v {
            if d & v != v {
                return false;
            }
            prog(dst + o, v);
        }
        if r(dst + o) != v {
            return false;
        }
        o += 4;
    }
    true
}

pub fn blk_same() -> bool {
    let mut o = 0;
    while o < 24 {
        if r(BLK + o) != r(T_BLK + o) {
            return false;
        }
        o += 4;
    }
    true
}

/// Restores what stock V2.6 reads at the old addresses. Each sector is erased
/// only if its data is gone; a valid record stays (the stock may have saved a newer one).
pub fn restore() -> bool {
    use crate::install::erase;
    if !blk_same() {
        erase(9);
        if !copy(BLK, T_BLK, 24) {
            return false;
        }
    }
    if !rec_ok() {
        erase(10);
        return copy(REC, TAIL, 800);
    }
    true
}

/// First thing on every boot. Nothing is erased here, whatever the state.
pub fn migrate() {
    if ok() || !rec_ok() || matches!(r(BLK), 0 | 0xFFFF_FFFF) {
        return;
    }
    unlock();
    if copy(TAIL, REC, 800) && copy(T_BLK, BLK, 24) {
        let c = word(crc_at(0xFFFF, TAIL, 824));
        let d = r(T_CRC);
        if d != c && d & c == c {
            prog(T_CRC, c);
        }
    }
    lock();
}
