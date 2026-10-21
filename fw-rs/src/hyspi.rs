//! HY3131 bus bring-up (DBG only; hy3131.rs has the bus,
//! docs/dmm-input-scope-feasibility.md section 8 the device plan). DIAG and
//! RAW are for the SPI2 bring-up only: REMOVE BEFORE FINAL.
//!
//! All three are synchronous (main loop blocked, watchdog kicked); main.rs
//! pauses the DMM and restarts its row afterwards (DIAG too), refused while
//! logging. Scratch register: R36 (SPHACAL), restored afterwards. Every write
//! to it is followed by an empty write frame to R00 (`hy3131::guard`): if a
//! CS high were missed the next command byte would land in R37 (test mode;
//! 0x60 resets the chip) as data, and the guard's 0x00 is the safe value.
//!
//!   DBG:HYSPI:SELF? n[,div]
//!       On the current bus (`div` 8 / 16 / 32 for this call only). The HY3131
//!       has no ID / version register: the reference is 8 bit-banged reads
//!       of R1F..R35 (INTE + the row); bytes that differed between them are
//!       live and left out. n iterations of
//!         r1   read1 of the first stable non-0x00/0xFF byte = ref
//!         r6   read(R00, 6)          AD1 + AD2 (live; AD2 bits 23:19 = bit 18)
//!         r23  read(R1F, 23)         stable bytes = ref
//!         w1   write(R36, pattern), guard, read1(R36) = pattern
//!       Reply: div,n,bad_r1,bad_r23,bad_w1,signbad,us_r1,us_r6,us_r23,us_w1,
//!       stable,ref (div 0 = bit-bang; us = mean per frame, DWT, incl. CS;
//!       ref = R1F..R35 in hex, live bytes as "..").
//!   DBG:HYSPI:DIAG div,cpol,cpha,frame,setup,hold,idle,phi,plo,ospeed
//!       Set bus parameters (empty field = keep): div 2..256, frame 0 = SPI2 +
//!       GPIO read-period pulse, 1 = SPI2 shifted, 2 = bit-bang; setup / hold
//!       / idle / phi / plo in spin iterations (DIAG? measures one). Not
//!       persisted: every boot has the defaults (hy3131.rs PAR).
//!   DBG:HYSPI:DIAG?
//!       div,cpol,cpha,frame,setup,hold,idle,phi,plo,ospeed,spin1000_cycles
//!   DBG:HYSPI:RAW?
//!       All 256 values into R36 in order, three ways: A = SPI2 write, bit-
//!       bang read; B = bit-bang write, SPI2 read; C = SPI2 write and read
//!       ("SPI2" = the current framing; frame 2 makes all three bit-bang).
//!       Reply: A,B,C mismatch counts, then per mismatch `Xpp>gg/qq[rr..]`:
//!       combo, value written, value read, the value written before it in
//!       that combo (gg = qq: the write was lost), and the raw RX bytes of
//!       an SPI2 read (n + 1 in the shift framing).

use crate::hy3131 as hy;
use crate::mmio::{r32, spin};

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

const KEEP: [u16; hy::P_N] = [0xFFFF; hy::P_N];

/// DBG:HYSPI:SELF? (see the module header). Returns the reply length.
pub fn self_test(n: u32, div: u8, out: &mut [u8]) -> usize {
    let n = n.clamp(1, N_MAX);
    let saved = hy::diag_get();
    if matches!(div, 8 | 16 | 32) && hy::div() != 0 {
        let mut p = KEEP;
        p[hy::P_DIV] = div as u16;
        hy::diag_set(&p);
    }
    // Reference: 8 bit-banged reads; live bytes masked out.
    let mut reff = [0u8; REF_LEN];
    let mut stable = [true; REF_LEN];
    hy::diag_read(false, hy::R_INTE, &mut reff, None);
    let mut buf = [0u8; REF_LEN];
    for _ in 0..7 {
        hy::diag_read(false, hy::R_INTE, &mut buf, None);
        for k in 0..REF_LEN {
            stable[k] &= buf[k] == reff[k];
        }
    }
    let nstable = stable.iter().filter(|&&s| s).count();
    let k1 = (1..REF_LEN).find(|&k| stable[k] && reff[k] != 0 && reff[k] != 0xFF).unwrap_or(0);
    let mut sph = [0u8];
    hy::diag_read(false, R_SPHACAL, &mut sph, None);
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
        hy::guard();
        badw += (hy::read1(R_SPHACAL) != pat) as u64;
        if i & 63 == 0 {
            crate::wdt::kick_if_feeding();
        }
    }
    let div = hy::div();
    hy::diag_write(false, R_SPHACAL, &sph);
    hy::diag_set(&saved);
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

/// DBG:HYSPI:DIAG?
pub fn diag_text(out: &mut [u8]) -> usize {
    let mut o = Out { b: out, n: 0 };
    for v in hy::diag_get() {
        o.u(v as u64);
        o.s(b",");
    }
    let t = cyc();
    spin(1000);
    o.u(cyc().wrapping_sub(t) as u64);
    o.n
}

/// DBG:HYSPI:DIAG a,b,..: fields 0xFFFF = keep.
pub fn diag_set(p: &[u16; hy::P_N]) {
    hy::diag_set(p);
}

/// DBG:HYSPI:RAW? (see the module header).
pub fn raw(out: &mut [u8]) -> usize {
    let mut sph = [0u8];
    hy::diag_read(false, R_SPHACAL, &mut sph, None);
    let shift = hy::diag_get()[hy::P_FRAME] == 1;
    let hw = hy::div() != 0;
    let mut o = Out { b: out, n: 0 };
    // Counts first (patched in at the end): "A=ddd,B=ddd,C=ddd;"
    o.s(b"A=000,B=000,C=000;");
    let mut counts = [0u32; 3];
    for combo in 0..3usize {
        let (wh, rh) = [(true, false), (false, true), (true, true)][combo];
        let (wh, rh) = (wh && hw, rh && hw);
        let mut prev = sph[0];
        for p in 0..=255u8 {
            hy::diag_write(wh, R_SPHACAL, &[p]);
            hy::diag_write(wh, hy::R_AD1, &[]); // guard
            let mut g = [0u8];
            let mut rawb = [0u8; 2];
            hy::diag_read(rh, R_SPHACAL, &mut g, Some(&mut rawb));
            if g[0] != p {
                counts[combo] += 1;
                o.s(&[b' ', b"ABC"[combo]]);
                o.x(p);
                o.s(b">");
                o.x(g[0]);
                o.s(b"/");
                o.x(prev);
                if rh {
                    o.s(b"[");
                    o.x(rawb[0]);
                    if shift {
                        o.x(rawb[1]);
                    }
                    o.s(b"]");
                }
            }
            prev = p;
        }
        crate::wdt::kick_if_feeding();
    }
    hy::diag_write(false, R_SPHACAL, &sph);
    for (i, c) in counts.iter().enumerate() {
        let c = (*c).min(999);
        let at = 2 + i * 6;
        if at + 3 <= o.b.len() {
            o.b[at] = b'0' + (c / 100) as u8;
            o.b[at + 1] = b'0' + (c / 10 % 10) as u8;
            o.b[at + 2] = b'0' + (c % 10) as u8;
        }
    }
    o.n
}
