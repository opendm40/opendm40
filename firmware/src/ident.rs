//! opendm40's identity: the firmware version and the meter model.
//!
//! `VERSION` is the one version string, from Cargo.toml's package version
//! with a ".0" patch dropped (0.1.0 -> "0.1", 0.1.0-pre1 -> "0.1-pre1",
//! 0.1.2 -> "0.1.2"): *IDN? (also in the 34401A emulation), Settings >
//! Device, README.TXT / BACKUP.TXT, the CSV / log headers. build.sh names
//! the install file `dm40_opendm40_v<VERSION>.bin` by the same rule. Host
//! tests built without Cargo (no CARGO_PKG_VERSION) get "dev".

use core::sync::atomic::{AtomicU16, Ordering};

const PKG: &str = match option_env!("CARGO_PKG_VERSION") {
    Some(v) => v,
    None => "dev",
};

/// The byte range of `v` to drop: a ".0" patch field (the third
/// dot-separated field, before any "-pre1"); empty when there is none.
const fn trim(v: &[u8]) -> (usize, usize) {
    let mut dots = 0;
    let mut i = 0;
    while i < v.len() {
        if v[i] == b'.' {
            dots += 1;
            if dots == 2 {
                let end = if i + 2 <= v.len() { i + 2 } else { v.len() };
                if end == i + 2 && v[i + 1] == b'0' && (end == v.len() || v[end] == b'-' || v[end] == b'+') {
                    return (i, end);
                }
                break;
            }
        }
        i += 1;
    }
    (v.len(), v.len())
}

const CUT: (usize, usize) = trim(PKG.as_bytes());
const fn join() -> [u8; 32] {
    let v = PKG.as_bytes();
    let mut out = [0u8; 32];
    let mut n = 0;
    let mut i = 0;
    while i < v.len() && n < 32 {
        if i < CUT.0 || i >= CUT.1 {
            out[n] = v[i];
            n += 1;
        }
        i += 1;
    }
    out
}
const BUF: [u8; 32] = join();
const LEN: usize = {
    let n = PKG.len() - (CUT.1 - CUT.0);
    if n > 32 { 32 } else { n }
};

pub const VERSION: &str = match core::str::from_utf8(BUF.split_at(LEN).0) {
    Ok(s) => s,
    Err(_) => "?",
};

/// Model index (0 DM40A, 1 DM40B, 2 DM40C) and the raw calibration byte
/// (record 0x080C0000 + 0x168, docs/model-variants.md), set once by App::new
/// from `Dmm::model()` / `Dmm::model_raw()`. *IDN?, SYSTem:MODel? and the
/// CSV / log headers report it.
static MODEL: AtomicU16 = AtomicU16::new(0);

pub fn set_model(index: u8, raw: u8) {
    MODEL.store(index.min(2) as u16 | (raw as u16) << 8, Ordering::Relaxed);
}

/// (model index, raw calibration byte).
pub fn model() -> (u8, u8) {
    let v = MODEL.load(Ordering::Relaxed);
    (v as u8, (v >> 8) as u8)
}

/// "DM40A" / "DM40B" / "DM40C".
pub fn model_name() -> &'static str {
    match model().0 {
        1 => "DM40B",
        2 => "DM40C",
        _ => "DM40A",
    }
}
