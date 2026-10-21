//! HY3131 bus self-test (DBG only; hy3131.rs has the bus,
//! docs/dmm-input-scope-feasibility.md section 8).
//!
//!   DBG:HYSPI:SELF? n[,div]
//!       Synchronous (main loop blocked, watchdog kicked); main.rs pauses the
//!       DMM and restarts its row afterwards, refused while logging. Reads at
//!       `hy3131::DIV`, or at `div` 8 / 16 / 32 for this call. The HY3131 has
//!       no ID / version register: the reference is 8 reads of R1F..R35 (INTE
//!       + the row); bytes that differed between them are live and left out.
//!       n iterations of
//!         r1   read1 of the first stable non-0x00/0xFF byte = ref
//!         r6   read(R00, 6)          AD1 + AD2 (live; AD2 bits 23:19 = bit 18)
//!         r23  read(R1F, 23)         stable bytes = ref
//!         w1   write(R36, pattern) (SPHACAL, restored), read1(R36) = pattern
//!       Reply: div,n,bad_r1,bad_r23,bad_w1,signbad,us_r1,us_r6,us_r23,us_w1,
//!       stable,ref (us = mean per frame, DWT, incl. CS; ref = R1F..R35 in
//!       hex, live bytes as "..").

use crate::hy3131 as hy;
use crate::mmio::r32;

const DWT_CYCCNT: u32 = 0xE000_1004;
const CYC_PER_US: u64 = (crate::clock::HCLK_HZ / 1_000_000) as u64;

/// Iterations per call at most (~0.2 s).
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
    fn x(&mut self, v: u8) {
        const H: &[u8; 16] = b"0123456789ABCDEF";
        self.s(&[H[(v >> 4) as usize], H[(v & 15) as usize]]);
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
    let div = if matches!(div, 8 | 16 | 32) { div } else { hy::DIV };
    hy::set_div(div);
    // Reference: 8 reads; live bytes masked out.
    let mut reff = [0u8; REF_LEN];
    let mut stable = [true; REF_LEN];
    hy::read(hy::R_INTE, &mut reff);
    let mut buf = [0u8; REF_LEN];
    for _ in 0..7 {
        hy::read(hy::R_INTE, &mut buf);
        for k in 0..REF_LEN {
            stable[k] &= buf[k] == reff[k];
        }
    }
    let nstable = stable.iter().filter(|&&s| s).count();
    let k1 = (1..REF_LEN).find(|&k| stable[k] && reff[k] != 0 && reff[k] != 0xFF).unwrap_or(0);
    let sph = [hy::read1(R_SPHACAL)];
    let (mut bad1, mut bad23, mut badw, mut signbad) = (0u64, 0u64, 0u64, 0u64);
    let (mut c1, mut c6, mut c23, mut cw) = (0u64, 0u64, 0u64, 0u64);
    for i in 0..n {
        let t = cyc();
        let v = hy::read1(hy::R_INTE + k1 as u8);
        c1 += cyc().wrapping_sub(t) as u64;
        bad1 += (v != reff[k1]) as u64;

        let mut d = [0u8; 6];
        let t = cyc();
        hy::read(hy::R_AD1, &mut d);
        c6 += cyc().wrapping_sub(t) as u64;
        signbad += !hy::ad2_sign_ok(hy::s24(&d[3..6])) as u64;

        let t = cyc();
        hy::read(hy::R_INTE, &mut buf);
        c23 += cyc().wrapping_sub(t) as u64;
        bad23 += (0..REF_LEN).any(|k| stable[k] && buf[k] != reff[k]) as u64;

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
    hy::write(R_SPHACAL, &sph);
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
    o.s(b",");
    o.u(nstable as u64);
    // The reference R1F..R35, live bytes as "..".
    o.s(b",");
    for k in 0..REF_LEN {
        if stable[k] {
            o.x(reff[k]);
        } else {
            o.s(b"..");
        }
    }
    o.n
}
