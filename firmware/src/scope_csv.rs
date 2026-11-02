//! Scope record on the SPI-flash USB disk (UF-13, the stock MF Settings
//! "Save CSV"): the stored record of a STOPPED scope, written as the next
//! free `SCOP%03u.DLG` by the screenshot writer (screenshot.rs, kind Csv:
//! non-blocking slices, data -> FAT -> directory -> CRC verify, the same
//! disk-sharing rules: refused while a PC has the disk mounted, host writes
//! refused while it runs, one disk job at a time).
//!
//! The file is the compact binary form (dlog.rs kind 2, docs/
//! compact-log-format.md): the CSV header text, the code -> volts map and
//! the sample time, then the raw 10-bit codes as u16: 53 KB for a
//! 26624-sample record instead of the CSV's 692 KB. The CSV exists on read
//! (SCOP%03u.CSV on the read-only LUN 1, MMEM:DATA?, tools/dlog.py), byte
//! for byte what this module wrote as text before:
//!   # Alientek DM40A, opendm40 0.1, scope record (no real-time clock)
//!   # timebase 200us/div, 1V/div, probe x1, coupling DC, sample rate 12.5MS/s, samples 26624
//!   # trigger Edge rising 993mV, mode Auto, triggered
//!   # time_s = seconds from the trigger point (untriggered: from the screen centre); volts at the probe tip
//!   time_s,volts            (a current record of the Wave view: time_s,amperes)
//!   -1.06496e-03,+1.2345e+00
//! (fixed-width 26-byte rows, dlog::scope_row). The file length is known
//! before the first block is written (screenshot.rs allocates the whole
//! chain first) and any block can be filled on its own (`fill`). The whole
//! record (up to scope::CAP samples, ~10 s) is written, not only the screen.
//!
//! A Power Wave frame (app_pwave.rs "CSV", `set_pwave`) goes the same way
//! into the same SCOPE/SCOP%03u.DLG names, as DLG kind 4 (dlog.rs "power
//! wave": the samples and their times, not coded): its CSV is
//!   time_s,volts,amperes
//!   +0.00000e+00,+3.2512e+02,
//!   +7.81200e-05,,+1.4100e+00
//! one row per sample, U and I in turn (the other column empty).
//!
//! The record lives in the scope buffer: while it is written the scope is
//! held stopped (`Scope::hold`; RUN / AUTO / SCPI RUN do not restart it)
//! and the code -> volts map is the one taken at `start` (a probe or V/div
//! change meanwhile does not change the file).

#![allow(dead_code)]

/// CSV row width (the generated CSV, dlog::scope_row).
pub const ROW: usize = crate::dlog::SCOPE_ROW;
const HDR_MAX: usize = 512;
/// Largest record (scope::CAP, asserted in scope_page.rs) and file.
pub const REC_MAX: usize = crate::dlog::SBLK * crate::dlog::SBLK_MAX;
pub const MAX_LEN: u32 = (crate::dlog::FHDR + HDR_MAX + crate::dlog::SCOPE_PAR + 8 * crate::dlog::SBLK_MAX + 4
    + REC_MAX.div_ceil(crate::dlog::SBLK) * crate::dlog::SBLK_RAW) as u32;
const _: () = assert!(REC_MAX <= crate::dlog::SBLK * crate::dlog::SBLK_MAX);

struct Snap {
    /// EMPTY until the first `set` (all zero, so SNAP is .bss, not 548 B
    /// of the .data load image in region C; region C audit #1).
    rec: crate::pack10::Codes,
    /// Volts = a + b * code; seconds per sample; trigger / centre index.
    a: f32,
    b: f32,
    dt: f32,
    t0: usize,
    hdr: [u8; HDR_MAX],
    hdr_n: usize,
    /// Coded blocks (dlog::SBLK samples each; PLAN holds their coding).
    nb: usize,
    /// PLAN's fill cursor is valid.
    cur_ok: bool,
    /// CRC-32 of the block table.
    tcrc: u32,
    /// A Power Wave frame (`set_pwave`) instead of a scope record: `pw_rec`
    /// is the page's buffer (codes at 0.., times at pwave::T_AT..), `pw_n`
    /// samples, scales, time unit, sample 0 is
    /// U, the samples' CRC-32.
    pw: bool,
    pw_rec: Option<&'static [u16]>,
    pw_n: usize,
    pw_sc: [f32; 4],
    pw_tu: f32,
    pw_y: bool,
    pw_crc: u32,
}

static mut SNAP: Snap = Snap {
    rec: crate::pack10::Codes::EMPTY,
    a: 0.0,
    b: 0.0,
    dt: 0.0,
    t0: 0,
    hdr: [0; HDR_MAX],
    hdr_n: 0,
    nb: 0,
    cur_ok: false,
    tcrc: 0,
    pw: false,
    pw_rec: None,
    pw_n: 0,
    pw_sc: [0.0; 4],
    pw_tu: 0.0,
    pw_y: false,
    pw_crc: 0,
};

/// Per block: mode (fixed predictor order / raw), coded bytes, CRC-32.
/// Written by `set` before any use: SRAM (.dmabuf, not zeroed at reset),
/// not TCM.
struct Plan {
    mode: [u32; crate::dlog::SBLK_MAX],
    len: [u32; crate::dlog::SBLK_MAX],
    crc: [u32; crate::dlog::SBLK_MAX],
    /// The fill cursor: block `cb` coded up to `enc.pos` (valid while
    /// Snap::cur_ok).
    enc: crate::dlog::SEnc,
    cb: usize,
}

// Backup SRAM (.bkp_bss, link-native.x; CPU only, zeroed at reset;
// docs/native-boot.md "Extra RAM areas").
#[cfg_attr(target_os = "none", link_section = ".bkp_bss")]
static mut PLAN: Plan = Plan {
    mode: [0; crate::dlog::SBLK_MAX],
    len: [0; crate::dlog::SBLK_MAX],
    crc: [0; crate::dlog::SBLK_MAX],
    enc: crate::dlog::SEnc::new(0, 0),
    cb: 0,
};

fn plan() -> &'static mut Plan {
    unsafe { &mut *core::ptr::addr_of_mut!(PLAN) }
}

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

/// Take the record for writing: `rec` (codes), value = a + b * code (volts,
/// or amperes for a current record of the Wave view), sample rate `fs`,
/// `t0` = sample index of time 0, the header lines (without the column
/// line), `amps`: the value column is "amperes" (else "volts"). The writer
/// is started separately
/// (screenshot::start_csv) and reads it through `fill`.
/// Region B (not this module's .cold): it runs in the Save CSV tap, while
/// the Wave view's stream may run (the stream pauses only for the write).
#[cfg_attr(target_os = "none", link_section = ".text.scope_plan")]
pub fn set(rec: crate::pack10::Codes, a: f32, b: f32, fs: f32, t0: usize, h: &Hdr, amps: bool) {
    let s = snap();
    s.pw = false;
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
    let col: &[u8] = if amps { b"time_s,amperes\r\n" } else { b"time_s,volts\r\n" };
    for &c in col {
        if n < HDR_MAX {
            s.hdr[n] = c;
            n += 1;
        }
    }
    s.hdr_n = n;
    // Code every block once (~1 ms per block at 200 MHz): the file length
    // must be known before the writer allocates; `fill` codes them again.
    let p = plan();
    s.cur_ok = false;
    s.nb = rec.len().div_ceil(crate::dlog::SBLK).min(crate::dlog::SBLK_MAX);
    let mut tc = !0u32;
    for b in 0..s.nb {
        let c = rec.sub(b * crate::dlog::SBLK, (b + 1) * crate::dlog::SBLK);
        (p.mode[b], p.len[b], p.crc[b]) = crate::dlog::scope_plan(&c);
        tc = crate::dlog::crc32_upd(tc, &p.len[b].to_le_bytes());
        tc = crate::dlog::crc32_upd(tc, &p.crc[b].to_le_bytes());
    }
    s.tcrc = !tc;
}

use crate::pwave::T_AT;

/// Take a Power Wave frame for writing (app_pwave.rs, the page stopped
/// and the scope buffer held until the writer's result): `buf` = the
/// page's buffer, `n` samples (codes at 0.., times at pwave::T_AT..), `sc` =
/// su, ou, si, oi (pwave::Scale), `tu`
/// seconds per time unit, `y_first` = sample 0 is U, the header lines
/// (without the column line).
pub fn set_pwave(buf: &'static [u16], n: usize, sc: [f32; 4], tu: f32, y_first: bool, h: &Hdr) {
    let s = snap();
    let n = n.min(T_AT);
    s.pw = true;
    s.pw_rec = Some(buf);
    s.pw_n = n;
    s.pw_sc = sc;
    s.pw_tu = tu;
    s.pw_y = y_first;
    s.cur_ok = false;
    s.nb = 0;
    let mut k = h.n;
    s.hdr[..k].copy_from_slice(&h.b[..k]);
    for &c in b"time_s,volts,amperes\r\n" {
        if k < HDR_MAX {
            s.hdr[k] = c;
            k += 1;
        }
    }
    s.hdr_n = k;
    let mut c = !0u32;
    for i in 0..n {
        c = crate::dlog::crc32_upd(c, &pw_sample(buf, i));
    }
    s.pw_crc = !c;
}

/// The 4 file bytes of Power Wave sample `i`: u16 time, i16 code.
fn pw_sample(buf: &[u16], i: usize) -> [u8; 4] {
    let (t, c) = (buf[T_AT + i], buf[i]);
    [t as u8, (t >> 8) as u8, c as u8, (c >> 8) as u8]
}

/// Power Wave: bytes before the samples (DLG header, parameters).
fn pw_head_len(s: &Snap) -> usize {
    crate::dlog::FHDR + s.hdr_n + crate::dlog::PW_PAR
}

/// Bytes before the first coded block: DLG header, parameters, table.
fn head_len(s: &Snap) -> usize {
    crate::dlog::FHDR + s.hdr_n + crate::dlog::SCOPE_PAR + 8 * s.nb + 4
}

/// File length in bytes (the DLG file).
pub fn len() -> u32 {
    let s = snap();
    if s.pw {
        return (pw_head_len(s) + 4 * s.pw_n + 4) as u32;
    }
    let p = plan();
    (head_len(s) + p.len[..s.nb].iter().map(|&l| l as usize).sum::<usize>()) as u32
}

/// File bytes `off .. off + buf.len()`: the DLG header, parameters and
/// block table, then the coded blocks. Sequential calls continue the
/// coding where the last one stopped (screenshot.rs fills a disk block in
/// pieces: each piece costs only its own samples).
pub fn fill(off: u32, buf: &mut [u8]) {
    let s = snap();
    if s.pw {
        pw_fill(s, off as usize, buf);
        return;
    }
    let p = plan();
    let off = off as usize;
    let head = head_len(s);
    let rec = s.rec;
    let mut i = 0;
    if off < head {
        let mut h = [0u8; crate::dlog::FHDR + HDR_MAX + crate::dlog::SCOPE_PAR];
        let k = crate::dlog::scope_header(&s.hdr[..s.hdr_n], s.a, s.b, s.dt, s.t0 as u32, rec.len() as u32, &mut h);
        // Then the block table (dlog::scope_table's bytes), made here: no
        // copy of it on the stack.
        while i < buf.len() && off + i < head {
            let o = off + i;
            buf[i] = if o < k {
                h[o]
            } else {
                let t = o - k;
                let v = if t >= 8 * s.nb {
                    s.tcrc
                } else if t % 8 < 4 {
                    p.len[t / 8]
                } else {
                    p.crc[t / 8]
                };
                (v >> (8 * (t % 4))) as u8
            };
            i += 1;
        }
    }
    let codes = |b: usize| rec.sub(b * crate::dlog::SBLK, (b + 1) * crate::dlog::SBLK);
    while i < buf.len() {
        let at = (off + i) as u32;
        if !s.cur_ok || p.enc.pos != at {
            // Seek: the block holding `at`, coded from its start.
            let mut o = head as u32;
            let mut b = 0;
            while b < s.nb && o + p.len[b] <= at {
                o += p.len[b];
                b += 1;
            }
            if b == s.nb {
                return;
            }
            p.cb = b;
            p.enc = crate::dlog::SEnc::new(p.mode[b], o);
            let c = codes(b);
            while p.enc.pos < at && p.enc.next(&c).is_some() {}
            s.cur_ok = true;
        }
        match p.enc.next(&codes(p.cb)) {
            Some(x) => {
                buf[i] = x;
                i += 1;
            }
            None => {
                if p.cb + 1 >= s.nb {
                    return;
                }
                p.cb += 1;
                p.enc = crate::dlog::SEnc::new(p.mode[p.cb], at);
            }
        }
    }
}

/// `fill` of a Power Wave frame (kind 4): header + parameters, the samples,
/// their CRC-32; zeros past the end.
fn pw_fill(s: &Snap, off: usize, buf: &mut [u8]) {
    let head = pw_head_len(s);
    let rec = s.pw_rec.unwrap_or(&[]);
    let end = head + 4 * s.pw_n;
    let mut h = [0u8; crate::dlog::FHDR + HDR_MAX + crate::dlog::PW_PAR];
    if off < head {
        crate::dlog::pw_header(&s.hdr[..s.hdr_n], s.pw_sc, s.pw_tu, s.pw_n as u32, s.pw_y, &mut h);
    }
    for (j, b) in buf.iter_mut().enumerate() {
        let at = off + j;
        *b = if at < head {
            h[at]
        } else if at < end {
            pw_sample(rec, (at - head) / 4)[(at - head) % 4]
        } else if at < end + 4 {
            s.pw_crc.to_le_bytes()[at - end]
        } else {
            0
        };
    }
}
