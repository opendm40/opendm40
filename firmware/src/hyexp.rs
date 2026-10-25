//! HY3131 front-end experiments on the current row (DBG-only, synchronous,
//! like ad2exp.rs: the main loop is blocked for the run, bounded by `N_MAX`
//! results and a per-result timeout, the watchdog kicked; main.rs restores
//! the row afterwards with dmm.start). The pre-filter capacitor on FTP / FTN
//! (R33 SFT1) and the AC V divider compensation (R2A..R2E, R30 ACC):
//! hw/README.md, firmware/docs/device-check-2026-10-21.md.
//!
//!   DBG:HY:CAP? kind,n,npre{,i,v}   (up to 6 pairs)
//!       i < 22: row byte i (R20 + i) = v in the pre row; 32 <= i < 54: byte
//!       i - 32 = v written alone at the step. Writes the row the DMM last
//!       wrote with the pre bytes, drops `npre` results, writes the step
//!       bytes in the order given (one SPI write each; t = 0 right after the
//!       last), then takes n results of `kind`: 0 = AD1 (R00..R02), 1 = RMS
//!       (R09..R0D, 37 bits unsigned), 2 = AD2 (R03..R05).
//!       Reply: <22 base row bytes, hex>;n,timeouts;t_us,value;...

use crate::hy3131 as hy;
use crate::mmio::r32;

const DWT_CYCCNT: u32 = 0xE000_1004;
const CYC_PER_US: u32 = crate::clock::HCLK_HZ / 1_000_000;

/// Results per call at most (~16 bytes each in the 6 kB reply).
pub const N_MAX: u16 = 360;

fn cyc() -> u32 {
    unsafe { r32(DWT_CYCCNT) }
}

/// Wait for the kind's flag (false on timeout) and read the result.
fn take(kind: u8, lim_us: u32) -> Option<i64> {
    let flag = match kind {
        0 => hy::INTF_AD1,
        1 => hy::INTF_RMS,
        _ => hy::INTF_AD2,
    };
    let t0 = cyc();
    let mut k = 0u32;
    while hy::read1(hy::R_INTF) & flag == 0 {
        k += 1;
        if k % 64 == 0 {
            crate::wdt::kick_if_feeding();
        }
        if cyc().wrapping_sub(t0) / CYC_PER_US > lim_us {
            return None;
        }
    }
    Some(match kind {
        0 => hy::read_ad1() as i64,
        1 => {
            let mut b = [0u8; 5];
            hy::read(0x09, &mut b);
            (b[0] as u64 | (b[1] as u64) << 8 | (b[2] as u64) << 16 | (b[3] as u64) << 24 | ((b[4] & 0x1F) as u64) << 32) as i64
        }
        _ => hy::read_ad2().0 as i64,
    })
}

fn put(out: &mut [u8], n: &mut usize, s: &[u8]) {
    let k = s.len().min(out.len().saturating_sub(*n));
    out[*n..*n + k].copy_from_slice(&s[..k]);
    *n += k;
}

fn put_i(out: &mut [u8], n: &mut usize, v: i64) {
    let mut b = [0u8; 24];
    let mut i = b.len();
    let neg = v < 0;
    let mut u = v.unsigned_abs();
    loop {
        i -= 1;
        b[i] = b'0' + (u % 10) as u8;
        u /= 10;
        if u == 0 {
            break;
        }
    }
    if neg {
        i -= 1;
        b[i] = b'-';
    }
    put(out, n, &b[i..]);
}

/// `a` = [kind, n_lo, n_hi, npre, i0, v0, .. i5, v5] (i = 255: unused).
#[inline(never)] // .cold (link-native.x)
pub fn cap(base: &[u8; hy::CFG_LEN], a: &[u8; 16], out: &mut [u8]) -> usize {
    let (kind, npre) = (a[0].min(2), a[3]);
    let cnt = (a[1] as u16 | (a[2] as u16) << 8).clamp(1, N_MAX);
    let pairs = a[4..].chunks(2).map(|p| (p[0], p[1]));
    let mut row = *base;
    for (i, v) in pairs.clone() {
        if (i as usize) < hy::CFG_LEN {
            row[i as usize] = v;
        }
    }
    let lim_us = match kind {
        0 => hy::ad1_period_us_r22(row[2]) * 3 + 20_000,
        1 => 1_500_000,
        _ => 20_000,
    };
    hy::quiesce();
    hy::configure(&row);
    let mut to = 0u32;
    for _ in 0..npre {
        to += take(kind, lim_us).is_none() as u32;
    }
    for (i, v) in pairs {
        if (32..32 + hy::CFG_LEN as u8).contains(&i) {
            hy::write(0x20 + i - 32, &[v]);
        }
    }
    let t0 = cyc();
    let mut n = 0;
    for b in base {
        let h = b"0123456789ABCDEF";
        put(out, &mut n, &[h[(b >> 4) as usize], h[(b & 15) as usize]]);
    }
    let head = n;
    put(out, &mut n, b";000,0000000"); // count and timeouts, filled in below
    let mut got = 0u16;
    for _ in 0..cnt {
        match take(kind, lim_us) {
            Some(v) => {
                let t = (cyc().wrapping_sub(t0) / CYC_PER_US) as i64;
                put(out, &mut n, b";");
                put_i(out, &mut n, t);
                put(out, &mut n, b",");
                put_i(out, &mut n, v);
                got += 1;
            }
            None => to += 1,
        }
        if n + 32 > out.len() {
            break;
        }
    }
    // Fixed-width digits over the placeholder.
    let w = |out: &mut [u8], at: usize, v: u32, d: usize| {
        let mut v = v.min(10u32.pow(d as u32) - 1);
        for j in (0..d).rev() {
            out[at + j] = b'0' + (v % 10) as u8;
            v /= 10;
        }
    };
    w(out, head + 1, got as u32, 3);
    w(out, head + 5, to, 7);
    n
}
