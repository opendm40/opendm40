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
//!   DBG:AD2:DCV? range,r26,n[,flags]
//!       Device test 6.5 (the DC-coupled V row, feasibility 1.3 last row /
//!       E7; host: tools/ad2dcv.py). In DC V only; main.rs first sets the
//!       DC V `range` (0..3 = 6 / 60 / 600 / 1000 V, manual), then this
//!       writes `dcv_row` (the AC V row of the range with SAD2IP = OP1O)
//!       and runs four phases, n = 1..16 AD1 conversions (R22 0x14, OSR
//!       4096 chopped, 50 ms) each for A / B after 2 dropped:
//!         A  AD1 alone (ENAD2 off)          B  AD1 + AD2 (ENAD2 at r26)
//!         C  AD2 alone (AD1 off: the stream) D  C with OP1+ = AGND (zero)
//!       range,r26,flags,n;A ad1_n,ad1_mean_m,ad1_sd_m;
//!       B ad1_n,ad1_mean_m,ad1_sd_m,ad2_n,ad2_mean_m,ad2_sd_m,ad2_min,ad2_max;
//!       C ad2_n,..;D ad2_n,..;timeouts   (+ ";uV_A,uV_B" from main.rs:
//!       the AD1 means in volts with the DC V calibration of the tap).
//!       flags: bit 0 SAD2IN = RLU (else AGND); bits 2:1 OP1CHOP (0 off,
//!       1 1 kHz, 2 2 kHz (the stock AC V row), 3 DC); bit 3 OP2 instead
//!       of OP1 (OP2 x1 on SENSE, AD2 = OP2O; the power meter's V path,
//!       6 V on the DC V r0 /10 tap). C / D take 50 x n AD2 samples.
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
#[inline(never)] // .cold (link-native.x): not inlined into the hot caller
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
#[inline(never)] // .cold (link-native.x): not inlined into the hot caller
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

// ------------------------------------------------- the DC-coupled V row (E7)

/// R27 SAD2IP / SAD2IN fields (bits 7:6 / 5:4).
const R27_IP_MASK: u8 = 0xC0;
const R27_IN_MASK: u8 = 0x30;
/// SAD2IN = AGND / RLU.
const R27_IN_AGND: u8 = 0x10;
const R27_IN_RLU: u8 = 0x00;
/// SAD2IP = OP2O.
const R27_IP_OP2O: u8 = 0x40;
/// R24: AD1 on SENSE - PB5 (the DC V 60 V+ rows' AD1 input; sentinel.rs
/// R24_SENSE_PB5).
const R24_SENSE_PB5: u8 = 0x07;
/// R22 of the AD1 conversions here: OSR 4096, chopped (50 ms).
pub const DCV_R22: u8 = 0x14;
/// R32 OP1 part: ENOP1 with SOP1P = AGND (the zero phase).
const R32_OP1_AGND: u8 = 0x0B;

/// Options of `dcv_row` (DBG:AD2:DCV? flags).
pub const DCV_RLU: u8 = 1;
pub const DCV_OP2: u8 = 8;

/// The DC-coupled V row of DC V range `r` (feasibility 1.3 last row, E7):
/// the stock AC V row CFG_F0[r] - its divider tap (R2A..R2F; r0 = the /100
/// tap with OP1 x10, OPS R25 0x43, as the stock AC V 6 V), ENOP1 with OP1+
/// = SENSE (R32 0x38), AD2IG 1.0, VR = PB6 - AGND (R27 low nibble) - with
/// SAD2IP = OP1O instead of PB4 (PB4 is OP1O through the coupling
/// capacitor), so AD2 sees the tap DC-coupled. The relays are DC V's = AC
/// V's (dmm.rs write_config: PA 0, PE 2 for both), so the row is register
/// only; the DMM's DC V row comes back with dmm.start. Also: AD3, RMS, LPF
/// and the peak hold off (R34 = R29 = 0: AD2F paces AD2 alone, as
/// set_ad2_stream), AD1 on SENSE - PB5 at R22 `DCV_R22` but NOT enabled
/// (R23 bit 7 clear: the caller enables it after AD2, datasheet 13.1 note
/// 7), OP1CHOP = `op1chop` (0..3; the stock row's 2 kHz chopper runs only
/// while AD1 converts, sentinel.rs: with AD1 off in the stream it would
/// stop at a random phase, so the self-cal and the stream hold it off,
/// 0), R26 = `r26` with ENAD2 iff `ad2`. `opt` DCV_RLU: SAD2IN = RLU
/// (else AGND); DCV_OP2: OP2 x1 on SENSE and AD2 = OP2O instead (the
/// power meter's V path; 6 V on the DC V r0 /10 tap as there, OP1 off).
pub fn dcv_row(r: u8, r26: u8, opt: u8, op1chop: u8, ad2: bool) -> [u8; hy::CFG_LEN] {
    use crate::hy3131_tables::{CFG_F0, CFG_F1};
    let r = r.min(3) as usize;
    let mut c = CFG_F0[r];
    let inp = if opt & DCV_RLU != 0 { R27_IN_RLU } else { R27_IN_AGND };
    if opt & DCV_OP2 != 0 {
        if r == 0 {
            c[10..17].copy_from_slice(&CFG_F1[0][10..17]);
        }
        c[5] &= !0x03; // OPS2 = OPS1 = 0 (x1)
        c[18] = 0x80 | 0x03; // ENOP2 SOP2P = SENSE; OP1 off (SOP1P AGND)
        c[7] = R27_IP_OP2O | inp | (c[7] & 0x0F);
    } else {
        c[7] = (c[7] & !(R27_IP_MASK | R27_IN_MASK)) | inp;
    }
    c[2] = DCV_R22;
    c[3] &= !0x80;
    c[4] = R24_SENSE_PB5;
    c[6] = (r26 & 0x3F) | if ad2 { 0x80 } else { 0 };
    c[9] = 0;
    c[19] = (c[19] & 0x3F) | (op1chop & 3) << 6;
    c[20] = 0;
    c
}

/// Running sums of one quantity, centred on its first value (no f64: the
/// soft-float routines would land in region B).
#[derive(Clone, Copy)]
struct Acc {
    n: u32,
    v0: i32,
    sum: i64,
    sq: i64,
    lo: i32,
    hi: i32,
}

impl Acc {
    const fn new() -> Self {
        Acc { n: 0, v0: 0, sum: 0, sq: 0, lo: i32::MAX, hi: i32::MIN }
    }
    fn add(&mut self, v: i32) {
        if self.n == 0 {
            self.v0 = v;
        }
        let d = (v - self.v0) as i64;
        self.n += 1;
        self.sum += d;
        self.sq += d * d;
        self.lo = self.lo.min(v);
        self.hi = self.hi.max(v);
    }
    /// Mean in 1/1000 code.
    fn m(&self) -> i64 {
        if self.n == 0 { 0 } else { self.v0 as i64 * 1000 + self.sum * 1000 / self.n as i64 }
    }
    fn mean(&self) -> i32 {
        (self.m() / 1000) as i32
    }
    /// Sample standard deviation in 1/1000 code.
    fn sd_m(&self) -> i64 {
        if self.n < 2 {
            return 0;
        }
        let n = self.n as f32;
        let (m, e2) = (self.sum as f32 / n, self.sq as f32 / n);
        (sqrt(((e2 - m * m) * n / (n - 1.0)).max(0.0)) * 1000.0) as i64
    }
}

/// Sample AD1 and / or AD2 on one INTF read per pass (it clears both
/// flags): until `ad1_n` AD1 results after `skip` (AD2 counted only between
/// the first and the last of them: the AD2 mean over the same AD1
/// conversions), or, with `ad1_n` = 0, `ad2_n` AD2 results. Bounded by a
/// deadline; watchdog kicked. Returns (ad1, ad2, timed out).
fn sample(ad1_n: u32, skip: u32, ad2_n: u32, ad2_period_ns: u32) -> (Acc, Acc, bool) {
    let (mut a1, mut a2) = (Acc::new(), Acc::new());
    let lim_us = if ad1_n > 0 {
        (ad1_n + skip + 2) * hy::ad1_period_us_r22(DCV_R22) * 3 / 2
    } else {
        ad2_n * (ad2_period_ns / 1000 + 1) * 3 + 50_000
    } + 100_000;
    let t0 = cyc();
    let mut seen1 = 0u32;
    let mut k = 0u32;
    loop {
        k += 1;
        if k % 64 == 0 {
            crate::wdt::kick_if_feeding();
        }
        if cyc().wrapping_sub(t0) / CYC_PER_US > lim_us {
            return (a1, a2, true);
        }
        let f = hy::read1(hy::R_INTF);
        if f & hy::INTF_AD2 != 0 && (ad1_n == 0 || (seen1 >= skip && seen1 < skip + ad1_n)) {
            let (v, ok) = hy::read_ad2();
            if ok {
                a2.add(v);
            }
            if ad1_n == 0 && a2.n >= ad2_n {
                return (a1, a2, false);
            }
        }
        if f & hy::INTF_AD1 != 0 {
            let v = hy::read_ad1();
            seen1 += 1;
            if seen1 > skip {
                a1.add(v);
                if a1.n >= ad1_n && ad1_n > 0 {
                    return (a1, a2, false);
                }
            }
        }
    }
}

/// Write `row` (AD1 off) and then enable AD1 when `ad1` (note 7: AD2 is
/// never switched while AD1 converts).
fn put_row(row: &[u8; hy::CFG_LEN], ad1: bool) {
    hy::write(0x23, &[row[3] & 0x7F]);
    hy::configure(row);
    if ad1 {
        hy::write(0x23, &[row[3] | 0x80]);
    }
}

/// DBG:AD2:DCV? (module header): the AD2 / AD1 numbers of the DC-coupled
/// V row at the current DC V range `r`. Reply length; the caller appends
/// the AD1 volts and restores the DMM row (dmm.start).
#[inline(never)] // .cold (link-native.x): not inlined into the hot caller
pub fn dcv_test(r: u8, r26: u8, n: u32, flags: u8, out: &mut [u8]) -> (usize, [i32; 2]) {
    let n = n.clamp(1, 16);
    let opt = flags & (DCV_RLU | DCV_OP2);
    let chop = (flags >> 1) & 3;
    let p = hy::ad2_period_ns(r26);
    let mut to = 0i64;
    // A: AD1 alone (the row with ENAD2 off).
    let row = dcv_row(r, r26, opt, chop, false);
    put_row(&row, true);
    let (a1, _, t) = sample(n, 2, 0, p);
    to += t as i64;
    // B: AD2 on (AD1 off around the switch), then AD1 again.
    hy::write(0x23, &[row[3] & 0x7F]);
    hy::write(0x26, &[0x80 | (r26 & 0x3F)]);
    hy::write(0x23, &[row[3] | 0x80]);
    let (b1, b2, t) = sample(n, 2, 0, p);
    to += t as i64;
    // C: AD1 off: AD2 as the stream sees it (OP1's chopper stops with AD1).
    hy::write(0x23, &[row[3] & 0x7F]);
    let _ = sample(0, 0, 4, p);
    let (_, c2, t) = sample(0, 0, 50 * n, p);
    to += t as i64;
    // D: the zero, OP1+ (or OP2+) on AGND.
    let r32 = if opt & DCV_OP2 != 0 { 0xB0 | (row[18] & 0x0F) } else { (row[18] & 0xF0) | R32_OP1_AGND };
    hy::write(0x32, &[r32]);
    let _ = sample(0, 0, 4, p);
    let (_, d2, t) = sample(0, 0, 50 * n, p);
    to += t as i64;
    let mut o = Out { b: out, n: 0 };
    o.list(&[r as i64, (r26 & 0x3F) as i64, flags as i64, n as i64], b',');
    o.s(b";");
    o.list(&[a1.n as i64, a1.m(), a1.sd_m()], b',');
    o.s(b";");
    o.list(&[b1.n as i64, b1.m(), b1.sd_m(), b2.n as i64, b2.m(), b2.sd_m(), b2.lo as i64, b2.hi as i64], b',');
    for x in [&c2, &d2] {
        o.s(b";");
        o.list(&[x.n as i64, x.m(), x.sd_m(), x.lo as i64, x.hi as i64], b',');
    }
    o.s(b";");
    o.i(to);
    (o.n, [a1.mean(), b1.mean()])
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
