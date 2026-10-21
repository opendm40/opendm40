//! Settings kept in the 4 KiB backup SRAM (0x40024000): it survives resets and,
//! with the backup domain powered, power-off. (The stock firmware keeps its
//! user settings in config.bin on the SPI disk.) Layout: magic, version,
//! payload, CRC32; anything that does not check out falls back to defaults.
//! The boot shim's words at the end of the 4 KiB (boot.rs, 0x40024FF0..) are
//! outside the record (v5: 0x40024000..0x400240CB).

use crate::mmio::{modify32, r32, w32};

const RCU_AHB1EN: u32 = 0x4002_3830;
const RCU_APB1EN: u32 = 0x4002_3840;
const PMU_CTL: u32 = 0x4000_7000;
const PMU_CS: u32 = 0x4000_7004;
const BKP: u32 = 0x4002_4000;
const MAGIC: u32 = 0x4446_5753; // "SWFD"
/// v4: 40 words (v3 had 24, v2 16; an older record is loaded zero-extended,
/// every new field reads 0 = its default). Layout: app.rs persist_words().
/// p[38] / p[39] were spare in v4 and are now W_DMM_EXT / W_GEN_OFFS. A v4
/// record from before reads 0 there = defaults, so no version change.
/// v5: 48 words; p[40..48] the scope page's math / FFT and trigger-type
/// settings (scope_page.rs sp_persist_ext; 0 = defaults, so a v4 record
/// loads zero-extended with nothing to migrate).
const VERSION: u32 = 5;
pub const WORDS: usize = 48;
/// First word of the scope page's extension block (8 words).
pub const W_SCOPE_EXT: usize = 40;
const V4_WORDS: usize = 40;
/// FR-05 conversion rate + display smoothing, UF-21 V-autorange migration
/// flag: meter::ext_word() / meter::ext_settings() pack and decode it.
pub const W_DMM_EXT: usize = 38;
/// FR-28 generator offset: 0 = auto (stock 0..Vpp), else the offset in V as
/// f32 bits with the sign bit set (app.rs persist_words / restore).
pub const W_GEN_OFFS: usize = 39;
const V3_WORDS: usize = 24;
/// The record (magic, version, WORDS, CRC) must end below the boot shim's
/// words at 0x40024FF0 (boot.rs BKP_CRASH..BKP_LAST).
const _: () = assert!(BKP + 4 * (WORDS as u32 + 3) <= 0x4002_4FF0, "persist record overlaps the boot words");
const V2_WORDS: usize = 16;

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

/// A record that passed the checks: its format version (2..4) and the
/// payload (a v2 record zero-extended to `WORDS`).
pub struct Record {
    pub version: u32,
    pub words: [u32; WORDS],
}

/// The stored record, or None when the magic, the version or the CRC does not
/// check out (never written, written by other firmware, a layout this build
/// does not know, a torn write): the caller then keeps its defaults.
pub fn load() -> Option<Record> {
    let rd = |i: usize| unsafe { r32(BKP + 4 * i as u32) };
    if rd(0) != MAGIC {
        return None;
    }
    let version = rd(1);
    let n = match version {
        VERSION => WORDS,
        4 => V4_WORDS,
        3 => V3_WORDS,
        2 => V2_WORDS,
        _ => return None,
    };
    let mut p = [0u32; WORDS];
    for (i, w) in p[..n].iter_mut().enumerate() {
        *w = rd(2 + i);
    }
    if crc(&p[..n]) != rd(2 + n) { None } else { Some(Record { version, words: p }) }
}

/// Whether the DMM function in the record belongs to the session the user
/// just left. The record is only rewritten while fw-rs runs; the stock
/// firmware (own settings in config.bin, power-on default DC V) and the
/// vendor loader never touch it. So after a round trip through other
/// firmware it holds whatever fw-rs measured last, possibly days and another
/// firmware ago (seen: the first native boot came up in AC+DC from an old RAM
/// session although the user had been measuring DC V on the stock firmware).
/// Native: the boot shim records every normal fw-rs boot in BKP_LAST
/// (boot.rs; DBG:UPDATE / DBG:RECOVER clear it, a recovery leaves "RCVR"), so
/// only a previous boot that was a normal fw-rs one continues the mode. RAM
/// builds (loaded by the stock) cannot tell: they keep restoring it.
pub fn same_session() -> bool {
    #[cfg(feature = "native")]
    {
        // boot.rs LAST_NORMAL ("NRML"), as the shim found BKP_LAST.
        const LAST_NORMAL: u32 = 0x4C4D_524E;
        crate::boot::info().is_some_and(|i| i.last == LAST_NORMAL)
    }
    #[cfg(not(feature = "native"))]
    {
        true
    }
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
