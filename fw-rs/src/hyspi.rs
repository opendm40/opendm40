//! One-shot verification of the HY3131 bus (DBG only; hy3131.rs has the
//! bus, docs/dmm-input-scope-feasibility.md section 8 the device plan).
//!
//!   DBG:HYSPI:SELF? n[,div]
//!       Run it in an AC row (AD2 on, so the sign check means something).
//!       Synchronous (main loop blocked, watchdog kicked; main.rs pauses the
//!       DMM and restarts its row afterwards, refused while logging). On the
//!       build's bus (SPI2 at `hy3131::DIV`; with `div` 8 / 16 / 32 SPI2 at
//!       that divider for this call only; the bit-banged fallback build
//!       ignores `div`). The HY3131 has no ID / version register, so the
//!       constants are the row the DMM just wrote: the first read of
//!       R1F..R35 is the reference, then n iterations of four frames:
//!         r1   read1(R20)            1 byte (an INTF-poll-sized frame; the
//!                                    row's first byte, mixed bits) = ref
//!         r6   read(R00, 6)          AD1 + AD2 data (live: not compared;
//!                                    AD2 bits 23:19 must copy bit 18)
//!         r23  read(R1F, 23)         R1F..R35 = ref
//!         w1   write(R36, pattern)   then read1(R36) = pattern (absolute:
//!                                    catches a consistently shifted bus;
//!                                    SPHACAL, restored at the end)
//!       Reply: div,n,bad_r1,bad_r23,bad_w1,signbad,us_r1,us_r6,us_r23,us_w1
//!       (div 0 = bit-banged build; bad_* = frames that differ, signbad = r6
//!       frames with a broken AD2 sign copy, us_* = mean time per frame in
//!       us, 2 decimals, DWT cycles incl. CS handling).

use crate::hy3131 as hy;
use crate::mmio::r32;

const DWT_CYCCNT: u32 = 0xE000_1004;
const CYC_PER_US: u64 = (crate::clock::HCLK_HZ / 1_000_000) as u64;

/// Iterations per call at most (~0.1 s on SPI2, ~1 s bit-banged).
pub const N_MAX: u32 = 2000;

const REF_LEN: usize = 1 + hy::CFG_LEN; // R1F..R35
const R_SPHACAL: u8 = 0x36;

fn cyc() -> u32 {
    unsafe { r32(DWT_CYCCNT) }
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
    fn u(&mut self, mut v: u64) {
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
    /// Mean cycles per frame as us with 2 decimals.
    fn us(&mut self, cycles: u64, n: u32) {
        let c = (cycles * 100 / CYC_PER_US + n as u64 / 2) / n.max(1) as u64;
        self.u(c / 100);
        self.s(b".");
        self.s(&[b'0' + (c / 10 % 10) as u8, b'0' + (c % 10) as u8]);
    }
}

/// DBG:HYSPI:SELF? (see the module header). Returns the reply length.
pub fn self_test(n: u32, div: u8, out: &mut [u8]) -> usize {
    let n = n.clamp(1, N_MAX);
    let div = if hy::DIV == 0 || !matches!(div, 8 | 16 | 32) { hy::DIV } else { div };
    let mut reff = [0u8; REF_LEN];
    hy::read(hy::R_INTE, &mut reff);
    let sphacal = hy::read1(R_SPHACAL);
    hy::set_div(div);
    let (mut bad1, mut bad23, mut badw, mut signbad) = (0u64, 0u64, 0u64, 0u64);
    let (mut c1, mut c6, mut c23, mut cw) = (0u64, 0u64, 0u64, 0u64);
    let mut buf = [0u8; REF_LEN];
    for i in 0..n {
        let t = cyc();
        let v = hy::read1(hy::R_CFG);
        c1 += cyc().wrapping_sub(t) as u64;
        bad1 += (v != reff[1]) as u64;

        let mut d = [0u8; 6];
        let t = cyc();
        hy::read(hy::R_AD1, &mut d);
        c6 += cyc().wrapping_sub(t) as u64;
        signbad += !hy::ad2_sign_ok(hy::s24(&d[3..6])) as u64;

        let t = cyc();
        hy::read(hy::R_INTE, &mut buf);
        c23 += cyc().wrapping_sub(t) as u64;
        bad23 += (buf != reff) as u64;

        let pat = (i as u8).wrapping_mul(37) ^ 0xA5;
        let t = cyc();
        hy::write(R_SPHACAL, &[pat]);
        cw += cyc().wrapping_sub(t) as u64;
        badw += (hy::read1(R_SPHACAL) != pat) as u64;
        if i & 63 == 0 {
            crate::wdt::kick_if_feeding();
        }
    }
    hy::set_div(hy::DIV);
    hy::write(R_SPHACAL, &[sphacal]);
    let mut o = Out { b: out, n: 0 };
    o.u(div as u64);
    for v in [n as u64, bad1, bad23, badw, signbad] {
        o.s(b",");
        o.u(v);
    }
    for c in [c1, c6, c23, cw] {
        o.s(b",");
        o.us(c, n);
    }
    o.n
}
