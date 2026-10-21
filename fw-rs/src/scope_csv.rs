//! Scope record as CSV on the SPI-flash USB disk (UF-13, the stock MF
//! Settings "Save CSV"): the stored record of a STOPPED scope, written as
//! the next free `SCOP%03u.CSV` by the screenshot writer (screenshot.rs,
//! kind Csv: non-blocking slices, data -> FAT -> directory -> CRC verify,
//! the same disk-sharing rules: refused while a PC has the disk mounted,
//! host writes refused while it runs, one disk job at a time).
//!
//! Content (CRLF, ASCII, '.' decimals):
//!   # Alientek DM40A, fw-rs 0.0.1, scope record (no real-time clock)
//!   # timebase 200us/div, 1V/div, probe x1, coupling DC, sample rate 12.5MS/s, samples 26624
//!   # trigger Edge rising 993mV, mode Auto, triggered
//!   # time_s = seconds from the trigger point (untriggered: from the screen centre); volts at the probe tip
//!   time_s,volts
//!   -1.06496e-03,+1.2345e+00
//! Rows are fixed width (26 bytes), so the file length is known before the
//! first block is written (screenshot.rs allocates the whole chain first)
//! and any block can be filled on its own (`fill`). The whole record (up to
//! scope::CAP samples, ~690 KB, ~10 s) is written, not only the screen.
//!
//! The record lives in the scope buffer: while a CSV is written the scope
//! is held stopped (`Scope::hold`; RUN / AUTO / SCPI RUN do not restart it)
//! and the code -> volts map is the one taken at `start` (a probe or V/div
//! change meanwhile does not change the file).

#![allow(dead_code)]

/// Fixed row: "+d.ddddde+xx,+d.dddde+xx\r\n".
pub const ROW: usize = 26;
const HDR_MAX: usize = 512;
/// Largest record (scope::CAP, asserted in scope_page.rs) and file.
pub const REC_MAX: usize = 26 * 1024;
pub const MAX_LEN: u32 = (HDR_MAX + REC_MAX * ROW) as u32;

struct Snap {
    rec: &'static [u16],
    /// Volts = a + b * code; seconds per sample; trigger / centre index.
    a: f32,
    b: f32,
    dt: f32,
    t0: usize,
    hdr: [u8; HDR_MAX],
    hdr_n: usize,
}

static mut SNAP: Snap = Snap { rec: &[], a: 0.0, b: 0.0, dt: 0.0, t0: 0, hdr: [0; HDR_MAX], hdr_n: 0 };

fn snap() -> &'static mut Snap {
    unsafe { &mut *core::ptr::addr_of_mut!(SNAP) }
}

/// Header text writer.
pub struct Hdr {
    b: [u8; HDR_MAX],
    n: usize,
}

impl Hdr {
    pub const fn new() -> Hdr {
        Hdr { b: [0; HDR_MAX], n: 0 }
    }
    pub fn s(&mut self, s: &str) -> &mut Hdr {
        for &c in s.as_bytes() {
            if self.n < HDR_MAX - 2 && c.is_ascii() {
                self.b[self.n] = c;
                self.n += 1;
            }
        }
        self
    }
}

/// Take the record for writing: `rec` (codes), volts = a + b * code,
/// sample rate `fs`, `t0` = sample index of time 0, the header lines
/// (without the column line). The writer is started separately
/// (screenshot::start_csv) and reads it through `fill`.
pub fn set(rec: &'static [u16], a: f32, b: f32, fs: f32, t0: usize, h: &Hdr) {
    let s = snap();
    s.rec = rec;
    s.a = a;
    s.b = b;
    s.dt = if fs > 0.0 { 1.0 / fs } else { 0.0 };
    s.t0 = t0;
    let mut n = 0;
    for &c in &h.b[..h.n] {
        s.hdr[n] = c;
        n += 1;
    }
    for &c in b"time_s,volts\r\n" {
        if n < HDR_MAX {
            s.hdr[n] = c;
            n += 1;
        }
    }
    s.hdr_n = n;
}

/// File length in bytes.
pub fn len() -> u32 {
    let s = snap();
    (s.hdr_n + s.rec.len() * ROW) as u32
}

/// "+d.dd..de+xx" with `dig` decimals into `out` (2 + dig + 4 bytes).
fn sci(out: &mut [u8], v: f32, dig: usize) {
    let mut v = if v.is_finite() { v } else { 0.0 };
    out[0] = if v < 0.0 { b'-' } else { b'+' };
    v = v.abs();
    let mut e = 0i32;
    if v != 0.0 {
        while v >= 10.0 {
            v *= 0.1;
            e += 1;
        }
        while v < 1.0 {
            v *= 10.0;
            e -= 1;
        }
    }
    let mut scale = 1u32;
    for _ in 0..dig {
        scale *= 10;
    }
    let mut m = (v * scale as f32 + 0.5) as u32;
    if m >= 10 * scale {
        m /= 10;
        e += 1;
    }
    let ip = m / scale;
    let mut fp = m % scale;
    out[1] = b'0' + ip as u8;
    out[2] = b'.';
    for i in (0..dig).rev() {
        out[3 + i] = b'0' + (fp % 10) as u8;
        fp /= 10;
    }
    let k = 3 + dig;
    out[k] = b'e';
    out[k + 1] = if e < 0 { b'-' } else { b'+' };
    let ea = e.unsigned_abs().min(99);
    out[k + 2] = b'0' + (ea / 10) as u8;
    out[k + 3] = b'0' + (ea % 10) as u8;
}

/// Row `i` of the record.
fn row(i: usize, out: &mut [u8; ROW]) {
    let s = snap();
    let t = (i as i32 - s.t0 as i32) as f32 * s.dt;
    sci(&mut out[0..12], t, 5);
    out[12] = b',';
    sci(&mut out[13..24], s.a + s.b * (s.rec[i] & 0x3FF) as f32, 4);
    out[24] = b'\r';
    out[25] = b'\n';
}

/// File bytes `off .. off + buf.len()`.
pub fn fill(off: u32, buf: &mut [u8]) {
    let s = snap();
    let mut off = off as usize;
    let mut i = 0;
    while i < buf.len() && off < s.hdr_n {
        buf[i] = s.hdr[off];
        i += 1;
        off += 1;
    }
    let mut r = [0u8; ROW];
    let mut cur = usize::MAX;
    while i < buf.len() {
        let k = (off - s.hdr_n) / ROW;
        if k >= s.rec.len() {
            break;
        }
        if k != cur {
            row(k, &mut r);
            cur = k;
        }
        let o = (off - s.hdr_n) % ROW;
        let n = (ROW - o).min(buf.len() - i);
        buf[i..i + n].copy_from_slice(&r[o..o + n]);
        i += n;
        off += n;
    }
}
