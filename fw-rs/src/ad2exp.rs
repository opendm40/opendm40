//! Device experiments for the AD2 fast path (FR-38 (B) / the DMM-input scope
//! (A)), docs/dmm-input-scope-feasibility.md section 6. DBG-only, synchronous
//! (the main loop is blocked for the run: bounded by `N_MAX` samples and a
//! per-sample timeout, the watchdog kicked), only in a stock AC row (main.rs
//! checks: AC V / mV / A / mA / uA, not AC+DC, logger off) and the caller
//! restores the row afterwards (dmm.start). Each call sets one R26 rung
//! (hy3131::set_ad2_stream: AD3 / RMS / LPF off), so a host loop sweeps them
//! (tools/dm40scpi.py `ad2 torn` / `ad2 noise`).
//!
//!   DBG:AD2:TORN? r26,n[,SYNC]
//!       n pairs of back-to-back R03..R05 reads (SYNC: each pair right after
//!       an AD2F was seen; else blind), then INTF. Settles feasibility 1.4
//!       (does the chip latch the 3-byte register during a read?):
//!       r26,period_ns,n,timeouts,same,diff_conv,diff_noconv,signbad;h0,..,h24
//!       same = both reads equal; diff_conv = different and an AD2F was set
//!       by the time the second read ended (a new conversion in between,
//!       legitimate); diff_noconv = different with no new conversion (the
//!       register changed without a flag: suspicious); signbad = reads whose
//!       bits 23:19 are not a copy of bit 18 (a torn read for sure); hK =
//!       pairs with bit_length(|a - b|) = K (h0 = equal). Noise lands in the
//!       low buckets; a tear shows as a bump at K = 9 or 17 (a byte boundary).
//!   DBG:AD2:NOISE? r26,n[,op1chop]
//!       n flag-synchronised samples (input shorted for the ENOB test):
//!       r26,period_ns,n,timeouts,signbad,mean_mcode,sd_mcode,min,max,elapsed_us
//!       (mcode = 1/1000 code; ENOB = log2(2^19 / sd), computed on the host;
//!       elapsed_us / n = the readout cost per sample). op1chop 0..3 sets
//!       R33 OP1CHOP (off, 1 kHz, 2 kHz, DC) for the run (restored with the
//!       row); omitted = the row's.
//!   DBG:AD2:DRAW 0|1|2  (main.rs) loss-per-rate experiment knob for a
//!       logger session: 0 = no panel (display frozen), 1 = the logger panel
//!       (default), 2 = the panel plus a full DMM-view redraw every 100 ms
//!       (the DMM page's redraw load).

use crate::hy3131 as hy;
use crate::mmio::r32;

const DWT_CYCCNT: u32 = 0xE000_1004;
const CYC_PER_US: u32 = crate::clock::HCLK_HZ / 1_000_000;

/// Samples per call at most (the host loops for more): ~1.2 s at the
/// slowest rung (1667 us), ~0.3 s bit-bang-limited at the fast ones.
pub const N_MAX: u32 = 2000;

fn cyc() -> u32 {
    unsafe { r32(DWT_CYCCNT) }
}

/// Wait for AD2F; false after 4 periods + 1 ms.
fn wait_ad2(period_ns: u32) -> bool {
    let t0 = cyc();
    let lim = (period_ns / 1000 * 4 + 1000) * CYC_PER_US;
    loop {
        if hy::ad2_ready() {
            return true;
        }
        if cyc().wrapping_sub(t0) > lim {
            return false;
        }
    }
}

/// Set the rung and let it settle: two periods (+ the comb filter's three
/// stages) and drop what is pending.
fn setup(r26: u8) -> u32 {
    hy::set_ad2_stream(r26);
    let p = hy::ad2_period_ns(r26);
    for _ in 0..4 {
        let _ = wait_ad2(p);
    }
    p
}

struct Out<'a> {
    b: &'a mut [u8],
    n: usize,
}

impl Out<'_> {
    fn s(&mut self, t: &[u8]) {
        let k = t.len().min(self.b.len() - self.n);
        self.b[self.n..self.n + k].copy_from_slice(&t[..k]);
        self.n += k;
    }
    fn i(&mut self, v: i64) {
        if v < 0 {
            self.s(b"-");
        }
        let mut v = v.unsigned_abs();
        let mut t = [0u8; 20];
        let mut k = t.len();
        loop {
            k -= 1;
            t[k] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        self.s(&t[k..]);
    }
    fn list(&mut self, v: &[i64], sep: u8) {
        for (k, x) in v.iter().enumerate() {
            if k > 0 {
                self.s(&[sep]);
            }
            self.i(*x);
        }
    }
}

/// bit_length(x): 0 for 0, 1 for 1, 9 for 256..511.
fn bitlen(x: u32) -> usize {
    (32 - x.leading_zeros()) as usize
}

/// DBG:AD2:TORN? (see the module header). Returns the reply length.
pub fn torn(r26: u8, n: u32, sync: bool, out: &mut [u8]) -> usize {
    let n = n.clamp(1, N_MAX);
    let p = setup(r26);
    let (mut timeouts, mut same, mut dconv, mut dnoconv, mut signbad) = (0i64, 0i64, 0i64, 0i64, 0i64);
    let mut h = [0i64; 25];
    for k in 0..n {
        if k % 64 == 0 {
            crate::wdt::kick_if_feeding();
        }
        if sync && !wait_ad2(p) {
            timeouts += 1;
            continue;
        }
        if !sync {
            let _ = hy::read1(hy::R_INTF); // clear: the flag after the pair is about the pair
        }
        let (a, oka) = hy::read_ad2();
        let (b, okb) = hy::read_ad2();
        let conv = hy::read1(hy::R_INTF) & hy::INTF_AD2 != 0;
        signbad += !oka as i64 + !okb as i64;
        let d = (a - b).unsigned_abs().min(0xFF_FFFF);
        h[bitlen(d)] += 1;
        if a == b {
            same += 1;
        } else if conv {
            dconv += 1;
        } else {
            dnoconv += 1;
        }
    }
    let mut o = Out { b: out, n: 0 };
    o.list(&[(r26 & 0x3F) as i64, p as i64, n as i64, timeouts, same, dconv, dnoconv, signbad], b',');
    o.s(b";");
    o.list(&h, b',');
    o.n
}

/// DBG:AD2:NOISE? (see the module header). `op1chop` > 3 = keep the row's.
pub fn noise(r26: u8, n: u32, op1chop: u8, out: &mut [u8]) -> usize {
    let n = n.clamp(2, N_MAX);
    if op1chop <= 3 {
        let r33 = hy::read1(0x33);
        hy::write(0x33, &[(r33 & 0x3F) | op1chop << 6]);
    }
    let p = setup(r26);
    let (mut timeouts, mut signbad, mut sum, mut sq, mut got) = (0i64, 0i64, 0i64, 0i64, 0i64);
    let (mut lo, mut hi) = (i64::MAX, i64::MIN);
    let t0 = cyc();
    let mut us = 0u64;
    let mut last = t0;
    for k in 0..n {
        if k % 64 == 0 {
            crate::wdt::kick_if_feeding();
        }
        if !wait_ad2(p) {
            timeouts += 1;
            continue;
        }
        let (v, ok) = hy::read_ad2();
        let c = cyc();
        us += (c.wrapping_sub(last) / CYC_PER_US) as u64; // no 21 s CYCCNT wrap issue
        last = c;
        if !ok {
            signbad += 1;
            continue;
        }
        let v = v as i64;
        got += 1;
        sum += v;
        sq += v * v;
        lo = lo.min(v);
        hi = hi.max(v);
    }
    let (mean_m, sd_m) = if got >= 2 {
        // n * sum(x^2) - sum(x)^2 <= 2000 * 2000 * 2^36: fits i64.
        let var_nn = (got * sq - sum * sum) as f32 / (got * (got - 1)) as f32;
        (sum * 1000 / got, (sqrt(var_nn.max(0.0)) * 1000.0) as i64)
    } else {
        (0, 0)
    };
    if got == 0 {
        lo = 0;
        hi = 0;
    }
    let mut o = Out { b: out, n: 0 };
    o.list(&[(r26 & 0x3F) as i64, p as i64, got, timeouts, signbad, mean_m, sd_m, lo, hi, us as i64], b',');
    o.n
}

/// Newton square root (no libm).
fn sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut r = if x > 1.0 { x / 2.0 } else { 1.0 };
    for _ in 0..30 {
        r = 0.5 * (r + x / r);
    }
    r
}
