//! Compact binary logs (DLG, docs/compact-log-format.md): the coder behind
//! the DMM recording (csvlog.rs writes LOGnnn.DLG instead of a CSV) and the
//! CSV generator that turns a DLG file back into the exact CSV the old
//! writer produced (virtual LOGnnn.CSV files on the read-only LUN 1,
//! MMEM:DATA?, the gallery).
//!
//! File = file header + chunks. One chunk per csvlog commit; each chunk is
//! self-contained (the coder restarts), CRC-32 protected and carries its first
//! row number, its CSV byte offset and its CSV length, so a power cut loses
//! only the chunk being written and the CSV export can seek without decoding.
//!
//!   file header (24 + hl bytes): "DLG1", kind (1 DMM, 2 scope record), unit
//!       length, u16 hl, u32 CRC-32 of bytes 12.., unit[12], the CSV header text
//!   chunk (28 + pl bytes): "DC", u16 pl, u32 CRC-32 of bytes 8.., u32 first
//!       row, u32 CSV offset, u32 CSV bytes, u32 t0 ms, u16 rows, u8 flags at
//!       the start, u8 0, pl bytes of bit stream (MSB first)
//!
//! Rows: time = adaptive Rice of the zigzag delta-of-delta (ms); value = the
//! 6-significant-digit decimal the old writer printed, as an integer at the
//! chunk's decimal scale, adaptive Rice of the zigzag delta + 1 (0 = escape:
//! empty value / new flags / new scale / "-0"). Rows with OL code one bit
//! (0 = unchanged, 1 = new flags follow). Exactness: the value is stored as
//! the printed decimal (`dec6`), never as a float, and printed by the same
//! `fmt_row` on both sides.
//!
//! Pure code (no hardware): the coder, `Gen` over any `Src`, `FatFile` over
//! any `Raw` byte reader (FAT16 chain walking) and the root scan `scan`.
//! Device glue (`vfiles` / `vread` for vdisk.rs, `export`) at the end.
//! Host tests: tools/dlog-test (round trip vs the old writer, truncation at
//! every byte), tools/fatren-test (csvlog on a FAT image, fsck, dlog.py).

#![allow(dead_code)]

pub const MAGIC: [u8; 4] = *b"DLG1";
pub const KIND_DMM: u8 = 1;
pub const KIND_SCOPE: u8 = 2;
/// File header bytes before the header text.
pub const FHDR: usize = 24;
/// Chunk header bytes.
pub const CHDR: usize = 28;
pub const UNIT_MAX: usize = 12;
/// Flag bits (csvlog F_*), GAP = rows were dropped just before this one.
pub const F_OL: u8 = 1;
pub const F_GAP: u8 = 32;
const FLAG_TXT: [(u8, &str); 6] = [(1, "OL"), (2, "REL"), (4, "HOLD"), (8, "HI"), (16, "LO"), (32, "GAP")];
/// Longest CSV row (time, value, unit, every flag).
pub const ROW_MAX: usize = 96;
/// Bytes one `Enc::row` may emit (worst case ~22).
pub const ROW_BYTES: usize = 32;
/// Rice: unary prefix length that switches to a raw 32-bit value.
const QMAX: u32 = 24;

// ------------------------------------------------------------- CRC-32 (zlib)

pub fn crc32_upd(mut c: u32, d: &[u8]) -> u32 {
    const T: [u32; 16] = {
        let mut t = [0u32; 16];
        let mut i = 0;
        while i < 16 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 4 {
                c = (c >> 1) ^ (0xEDB8_8320 & (c & 1).wrapping_neg());
                k += 1;
            }
            t[i] = c;
            i += 1;
        }
        t
    };
    for &b in d {
        c ^= b as u32;
        c = (c >> 4) ^ T[(c & 15) as usize];
        c = (c >> 4) ^ T[(c & 15) as usize];
    }
    c
}

fn le16(b: &[u8], o: usize) -> u32 {
    b[o] as u32 | (b[o + 1] as u32) << 8
}
fn le32(b: &[u8], o: usize) -> u32 {
    le16(b, o) | le16(b, o + 2) << 16
}

// ------------------------------------------------------------- values / rows

/// A CSV value as printed: empty, "-0", or 6 significant digits
/// (m = 100000..999999, value = m * 10^(e-5); m = 0 prints "0").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Val {
    Empty,
    NegZero,
    Num { neg: bool, m: u32, e: i32 },
}

const P10_MIN: i32 = -30;
const P10_MAX: i32 = 30;
const P10: [f32; 61] = [
    1e-30, 1e-29, 1e-28, 1e-27, 1e-26, 1e-25, 1e-24, 1e-23, 1e-22, 1e-21, 1e-20, 1e-19, 1e-18, 1e-17, 1e-16, 1e-15, 1e-14, 1e-13,
    1e-12, 1e-11, 1e-10, 1e-9, 1e-8, 1e-7, 1e-6, 1e-5, 1e-4, 1e-3, 1e-2, 1e-1, 1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9,
    1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22, 1e23, 1e24, 1e25, 1e26, 1e27, 1e28, 1e29, 1e30,
];

fn p10(e: i32) -> f32 {
    P10[(e.clamp(P10_MIN, P10_MAX) - P10_MIN) as usize]
}

/// The value csvlog prints for `v` (the old `W::num`): empty unless finite.
pub fn dec6(v: f32) -> Val {
    if !v.is_finite() {
        return Val::Empty;
    }
    let neg = v < 0.0;
    let x = if neg { -v } else { v };
    if x < 1e-30 {
        return if neg { Val::NegZero } else { Val::Num { neg: false, m: 0, e: 0 } };
    }
    // Decimal exponent: p10(e) <= x < p10(e + 1); one rounding step for
    // the mantissa (m = 100000..999999).
    let mut e = P10_MIN;
    while e < P10_MAX && p10(e + 1) <= x {
        e += 1;
    }
    let f = if e - 5 >= P10_MIN { x / p10(e - 5) } else { x / p10(e) * 1e5 };
    let mut m = (f + 0.5) as u32;
    if m >= 1_000_000 {
        m /= 10;
        e += 1;
    }
    Val::Num { neg, m: m.clamp(100_000, 999_999), e }
}

impl Val {
    /// As f32 (gallery statistics); None when empty.
    pub fn to_f32(self) -> Option<f32> {
        match self {
            Val::Empty => None,
            Val::NegZero => Some(0.0),
            Val::Num { neg, m, e } => {
                let a = if e - 5 >= P10_MIN { m as f32 * p10(e - 5) } else { m as f32 * p10(e) * 1e-5 };
                Some(if neg { -a } else { a })
            }
        }
    }
}

/// A tiny byte writer (silently truncates).
pub struct Txt<'a> {
    pub b: &'a mut [u8],
    pub n: usize,
}

impl Txt<'_> {
    pub fn c(&mut self, c: u8) {
        if self.n < self.b.len() {
            self.b[self.n] = c;
            self.n += 1;
        }
    }
    pub fn bytes(&mut self, s: &[u8]) {
        for &c in s {
            self.c(c);
        }
    }
    pub fn s(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    pub fn u(&mut self, v: u64) {
        let mut d = [0u8; 20];
        let mut k = 0;
        let mut x = v;
        loop {
            d[k] = b'0' + (x % 10) as u8;
            k += 1;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        while k > 0 {
            k -= 1;
            self.c(d[k]);
        }
    }
    /// Milliseconds as seconds with three decimals.
    pub fn ms(&mut self, ms: u64) {
        self.u(ms / 1000);
        self.c(b'.');
        let r = (ms % 1000) as u32;
        self.c(b'0' + (r / 100) as u8);
        self.c(b'0' + (r / 10 % 10) as u8);
        self.c(b'0' + (r % 10) as u8);
    }
    /// A value as csvlog prints it: plain for 1e-3 <= |v| < 1e6, else
    /// d.dddddE+xx.
    pub fn val(&mut self, v: Val) {
        let (neg, m, e) = match v {
            Val::Empty => return,
            Val::NegZero => {
                self.s("-0");
                return;
            }
            Val::Num { neg, m, e } => (neg, m, e),
        };
        if neg {
            self.c(b'-');
        }
        if m == 0 {
            self.c(b'0');
            return;
        }
        let mut dg = [0u8; 6];
        let mut t = m;
        for i in (0..6).rev() {
            dg[i] = b'0' + (t % 10) as u8;
            t /= 10;
        }
        if (-3..6).contains(&e) {
            if e < 0 {
                self.s("0.");
                for _ in 0..(-e - 1) {
                    self.c(b'0');
                }
                self.bytes(&dg);
            } else {
                let ip = (e + 1) as usize;
                self.bytes(&dg[..ip]);
                if ip < 6 {
                    self.c(b'.');
                    self.bytes(&dg[ip..]);
                }
            }
        } else {
            self.c(dg[0]);
            self.c(b'.');
            self.bytes(&dg[1..]);
            self.c(b'E');
            self.c(if e < 0 { b'-' } else { b'+' });
            let a = e.unsigned_abs();
            if a < 10 {
                self.c(b'0');
            }
            self.u(a as u64);
        }
    }
}

/// One CSV row `time_s,value,unit,flags\r\n` (the csvlog format); returns
/// its length. An OL row prints no value.
pub fn fmt_row(t_ms: u64, v: Val, unit: &[u8], flags: u8, out: &mut [u8]) -> usize {
    let mut w = Txt { b: out, n: 0 };
    w.ms(t_ms);
    w.c(b',');
    if flags & F_OL == 0 {
        w.val(v);
    }
    w.c(b',');
    w.bytes(unit);
    w.c(b',');
    let mut sep = false;
    for (bit, t) in FLAG_TXT {
        if flags & bit != 0 {
            if sep {
                w.c(b' ');
            }
            w.s(t);
            sep = true;
        }
    }
    w.s("\r\n");
    w.n
}

/// `v` as an integer at decimal scale k (value = n * 10^k), when exact and
/// at most one decade above the scale (|n| < 10^7).
fn at_scale(m: u32, e: i32, neg: bool, k: i32) -> Option<i32> {
    if m == 0 {
        return Some(0);
    }
    let n = match e - 5 - k {
        0 => m as i32,
        1 => m as i32 * 10,
        _ => return None,
    };
    Some(if neg { -n } else { n })
}

fn from_scale(n: i32, k: i32) -> Val {
    if n == 0 {
        return Val::Num { neg: false, m: 0, e: 0 };
    }
    let mut a = n.unsigned_abs();
    let mut d = 1i32;
    let mut t = a;
    while t >= 10 {
        t /= 10;
        d += 1;
    }
    let e = k + d - 1;
    while d > 6 {
        a /= 10;
        d -= 1;
    }
    while d < 6 {
        a = a.saturating_mul(10);
        d += 1;
    }
    Val::Num { neg: n < 0, m: a, e }
}

fn zz(x: i32) -> u32 {
    ((x << 1) ^ (x >> 31)) as u32
}
fn unzz(u: u32) -> i32 {
    ((u >> 1) as i32) ^ -((u & 1) as i32)
}

/// LOCO-I style adaptive Rice parameter.
#[derive(Clone, Copy, Debug)]
struct Ad {
    a: u32,
    n: u32,
}

impl Ad {
    const fn new(a: u32) -> Ad {
        Ad { a, n: 1 }
    }
    fn k(&self) -> u32 {
        let mut k = 0;
        while k < 30 && (self.n << k) < self.a {
            k += 1;
        }
        k
    }
    fn upd(&mut self, x: u32) {
        self.a = self.a.saturating_add(x.min(1 << 24));
        self.n += 1;
        if self.n >= 32 {
            self.a = (self.a + 1) >> 1;
            self.n >>= 1;
        }
    }
}

const AT0: u32 = 4;
const AV0: u32 = 16;

// ------------------------------------------------------------- encoder

/// Bit writer into a byte slice; whole bytes go out, the rest stays in `acc`.
struct BitW<'a> {
    acc: u64,
    nb: u32,
    out: &'a mut [u8],
    n: usize,
}

impl BitW<'_> {
    fn put(&mut self, v: u32, bits: u32) {
        if bits == 0 {
            return;
        }
        let v = if bits >= 32 { v as u64 } else { (v & ((1u32 << bits) - 1)) as u64 };
        self.acc = (self.acc << bits) | v;
        self.nb += bits;
        while self.nb >= 8 {
            self.nb -= 8;
            if self.n < self.out.len() {
                self.out[self.n] = (self.acc >> self.nb) as u8;
                self.n += 1;
            }
        }
        self.acc &= (1u64 << self.nb) - 1;
    }
    fn ones(&mut self, mut q: u32) {
        while q > 0 {
            let k = q.min(16);
            self.put((1 << k) - 1, k);
            q -= k;
        }
    }
    fn rice(&mut self, x: u32, k: u32) {
        let q = x >> k;
        if q < QMAX {
            self.ones(q);
            self.put(0, 1);
            self.put(x, k);
        } else {
            self.ones(QMAX);
            self.put(x, 32);
        }
    }
}

/// Chunk encoder state (Copy: csvlog snapshots it to make a row all or
/// nothing). Counters over the whole file: `rows`, `csv` (CSV bytes so far,
/// header text included).
#[derive(Clone, Copy, Debug)]
pub struct Enc {
    acc: u64,
    nb: u32,
    pub open: bool,
    // chunk
    row0: u32,
    csv_base: u32,
    csv_len: u32,
    t0: u32,
    pub nrows: u16,
    flags0: u8,
    /// Payload bytes emitted (whole bytes).
    pub pl: u32,
    // coder
    t_prev: u32,
    dt_prev: u32,
    flags: u8,
    has_k: bool,
    k: i32,
    pv: i32,
    at: Ad,
    av: Ad,
    // file
    pub rows: u32,
    pub csv: u32,
}

pub const ENC0: Enc = Enc {
    acc: 0,
    nb: 0,
    open: false,
    row0: 0,
    csv_base: 0,
    csv_len: 0,
    t0: 0,
    nrows: 0,
    flags0: 0,
    pl: 0,
    t_prev: 0,
    dt_prev: 0,
    flags: 0,
    has_k: false,
    k: 0,
    pv: 0,
    at: Ad::new(AT0),
    av: Ad::new(AV0),
    rows: 0,
    csv: 0,
};

impl Enc {
    /// A new file whose CSV header text is `hl` bytes.
    pub fn new(hl: usize) -> Enc {
        Enc { csv: hl as u32, ..ENC0 }
    }

    /// Code one row (`t_ms` since the start, the printed value, flags incl.
    /// GAP, `csv_len` = bytes of its CSV text). Whole bytes go to `out`
    /// (returned count, <= ROW_BYTES); a chunk opens with its first row.
    /// The caller writes the CHDR-byte chunk header placeholder before the
    /// first row's bytes (`open` false before the call).
    pub fn row(&mut self, t_ms: u32, v: Val, flags: u8, csv_len: u32, out: &mut [u8]) -> usize {
        if !self.open {
            self.open = true;
            self.row0 = self.rows;
            self.csv_base = self.csv;
            self.csv_len = 0;
            self.t0 = t_ms;
            self.nrows = 0;
            self.flags0 = flags;
            self.pl = 0;
            self.t_prev = t_ms;
            self.dt_prev = 0;
            self.flags = flags;
            self.has_k = false;
            self.k = 0;
            self.pv = 0;
            self.at = Ad::new(AT0);
            self.av = Ad::new(AV0);
            self.acc = 0;
            self.nb = 0;
        }
        let mut w = BitW { acc: self.acc, nb: self.nb, out, n: 0 };
        let dt = t_ms.wrapping_sub(self.t_prev);
        let x = zz(dt.wrapping_sub(self.dt_prev) as i32);
        w.rice(x, self.at.k());
        self.at.upd(x);
        self.t_prev = t_ms;
        self.dt_prev = dt;
        loop {
            if self.flags & F_OL != 0 {
                if flags == self.flags {
                    w.put(0, 1);
                    break;
                }
                w.put(1, 1);
                w.put(flags as u32, 6);
                self.flags = flags;
                continue;
            }
            let kv = self.av.k();
            if flags != self.flags {
                w.rice(0, kv);
                w.put(1, 2);
                w.put(flags as u32, 6);
                self.flags = flags;
                continue;
            }
            match v {
                Val::Empty => {
                    w.rice(0, kv);
                    w.put(0, 2);
                }
                Val::NegZero => {
                    w.rice(0, kv);
                    w.put(3, 2);
                }
                Val::Num { neg, m, e } => {
                    let n = if self.has_k { at_scale(m, e, neg, self.k) } else { None };
                    match n {
                        Some(n) => {
                            let u = zz(n.wrapping_sub(self.pv)) + 1;
                            w.rice(u, kv);
                            self.av.upd(u);
                            self.pv = n;
                        }
                        None => {
                            let k = if m == 0 { 0 } else { e - 5 };
                            let n = at_scale(m, e, neg, k).unwrap_or(0);
                            w.rice(0, kv);
                            w.put(2, 2);
                            w.put(k as u8 as u32, 8);
                            w.put((n < 0) as u32, 1);
                            w.put(n.unsigned_abs(), 24);
                            self.has_k = true;
                            self.k = k;
                            self.pv = n;
                        }
                    }
                }
            }
            break;
        }
        self.acc = w.acc;
        self.nb = w.nb;
        self.pl += w.n as u32;
        self.nrows += 1;
        self.csv_len += csv_len;
        self.rows += 1;
        self.csv += csv_len;
        w.n
    }

    /// Close the chunk: the padded last byte (0 or 1 bytes into `out`).
    /// Then `header` gives its header (with the CRC of the payload).
    pub fn close(&mut self, out: &mut [u8]) -> usize {
        let mut n = 0;
        if self.nb > 0 {
            out[0] = (self.acc << (8 - self.nb)) as u8;
            n = 1;
        }
        self.acc = 0;
        self.nb = 0;
        self.pl += n as u32;
        self.open = false;
        n
    }

    /// Header of the chunk just closed, without the CRC (bytes 4..8 zero):
    /// the caller sets it to !crc32_upd(crc32_upd(!0, &h[8..]), payload).
    pub fn header(&self) -> [u8; CHDR] {
        let mut h = [0u8; CHDR];
        h[0] = b'D';
        h[1] = b'C';
        h[2..4].copy_from_slice(&(self.pl as u16).to_le_bytes());
        h[8..12].copy_from_slice(&self.row0.to_le_bytes());
        h[12..16].copy_from_slice(&self.csv_base.to_le_bytes());
        h[16..20].copy_from_slice(&self.csv_len.to_le_bytes());
        h[20..24].copy_from_slice(&self.t0.to_le_bytes());
        h[24..26].copy_from_slice(&self.nrows.to_le_bytes());
        h[26] = self.flags0;
        h
    }
}

/// The file header (kind, unit, CSV header text) into `out`; returns its
/// length (FHDR + text).
pub fn file_header(kind: u8, unit: &[u8], text: &[u8], out: &mut [u8]) -> usize {
    let ul = unit.len().min(UNIT_MAX);
    let n = FHDR + text.len();
    out[..FHDR].fill(0);
    out[..4].copy_from_slice(&MAGIC);
    out[4] = kind;
    out[5] = ul as u8;
    out[6..8].copy_from_slice(&(text.len() as u16).to_le_bytes());
    out[12..12 + ul].copy_from_slice(&unit[..ul]);
    out[FHDR..n].copy_from_slice(text);
    let c = !crc32_upd(!0, &out[12..n]);
    out[8..12].copy_from_slice(&c.to_le_bytes());
    n
}

// ------------------------------------------------------------- decoder

/// Random-access bytes of one file.
pub trait Src {
    /// File bytes `off .. off + buf.len()`; false = read error.
    fn read(&mut self, off: u32, buf: &mut [u8]) -> bool;
}

impl Src for &[u8] {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> bool {
        let o = off as usize;
        if o + buf.len() > self.len() {
            return false;
        }
        buf.copy_from_slice(&self[o..o + buf.len()]);
        true
    }
}

const WIN: usize = 256;

/// A read window over a `Src` (file bytes `base .. base + n`).
pub struct Win {
    base: u32,
    n: u32,
    b: [u8; WIN],
}

impl Win {
    pub const fn new() -> Win {
        Win { base: 0, n: 0, b: [0; WIN] }
    }
    /// Byte `off` (< `end`, the readable limit).
    fn byte<S: Src>(&mut self, s: &mut S, off: u32, end: u32) -> Option<u8> {
        if off < self.base || off >= self.base + self.n {
            if off >= end {
                return None;
            }
            let n = (end - off).min(WIN as u32);
            if !s.read(off, &mut self.b[..n as usize]) {
                self.n = 0;
                return None;
            }
            self.base = off;
            self.n = n;
        }
        Some(self.b[(off - self.base) as usize])
    }
}

/// A chunk header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chunk {
    /// File offset of the header.
    pub off: u32,
    pub pl: u32,
    pub row0: u32,
    pub csv_base: u32,
    pub csv_len: u32,
    pub t0: u32,
    pub nrows: u32,
    pub flags0: u8,
}

impl Chunk {
    pub fn end(&self) -> u32 {
        self.off + CHDR as u32 + self.pl
    }
}

/// Row decoder of one chunk.
#[derive(Clone, Copy)]
struct RowDec {
    /// Bit position: byte offset in the file + bit (0 = MSB).
    pos: u32,
    bit: u32,
    end: u32,
    left: u32,
    t_prev: u32,
    dt_prev: u32,
    flags: u8,
    has_k: bool,
    k: i32,
    pv: i32,
    at: Ad,
    av: Ad,
}

impl RowDec {
    fn new(c: &Chunk) -> RowDec {
        RowDec {
            pos: c.off + CHDR as u32,
            bit: 0,
            end: c.end(),
            left: c.nrows,
            t_prev: c.t0,
            dt_prev: 0,
            flags: c.flags0,
            has_k: false,
            k: 0,
            pv: 0,
            at: Ad::new(AT0),
            av: Ad::new(AV0),
        }
    }

    fn get<S: Src>(&mut self, s: &mut S, w: &mut Win, mut bits: u32) -> Option<u32> {
        let mut v = 0u32;
        while bits > 0 {
            let b = w.byte(s, self.pos, self.end)?;
            let take = (8 - self.bit).min(bits);
            let x = (b >> (8 - self.bit - take)) as u32 & ((1u32 << take) - 1);
            v = if take == 32 { x } else { (v << take) | x };
            self.bit += take;
            bits -= take;
            if self.bit == 8 {
                self.bit = 0;
                self.pos += 1;
            }
        }
        Some(v)
    }

    fn rice<S: Src>(&mut self, s: &mut S, w: &mut Win, k: u32) -> Option<u32> {
        let mut q = 0;
        while q < QMAX && self.get(s, w, 1)? == 1 {
            q += 1;
        }
        if q == QMAX {
            return self.get(s, w, 32);
        }
        Some((q << k) | self.get(s, w, k)?)
    }

    /// The next row: (t ms, value, flags).
    fn next<S: Src>(&mut self, s: &mut S, w: &mut Win) -> Option<(u32, Val, u8)> {
        if self.left == 0 {
            return None;
        }
        let x = self.rice(s, w, self.at.k())?;
        self.at.upd(x);
        let dt = self.dt_prev.wrapping_add(unzz(x) as u32);
        let t = self.t_prev.wrapping_add(dt);
        self.t_prev = t;
        self.dt_prev = dt;
        let v;
        loop {
            if self.flags & F_OL != 0 {
                if self.get(s, w, 1)? == 1 {
                    self.flags = self.get(s, w, 6)? as u8;
                    continue;
                }
                v = Val::Empty;
                break;
            }
            let u = self.rice(s, w, self.av.k())?;
            if u != 0 {
                self.av.upd(u);
                self.pv = self.pv.wrapping_add(unzz(u - 1));
                v = from_scale(self.pv, self.k);
                break;
            }
            match self.get(s, w, 2)? {
                0 => {
                    v = Val::Empty;
                    break;
                }
                1 => {
                    self.flags = self.get(s, w, 6)? as u8;
                    continue;
                }
                2 => {
                    self.k = self.get(s, w, 8)? as u8 as i8 as i32;
                    let neg = self.get(s, w, 1)? == 1;
                    let a = self.get(s, w, 24)? as i32;
                    self.pv = if neg { -a } else { a };
                    self.has_k = true;
                    v = from_scale(self.pv, self.k);
                    break;
                }
                _ => {
                    v = Val::NegZero;
                    break;
                }
            }
        }
        self.left -= 1;
        Some((t, v, self.flags))
    }
}

/// Parse the file header: (kind, hl, unit length, unit).
pub fn parse_file<S: Src>(s: &mut S, size: u32) -> Option<(u8, u32, usize, [u8; UNIT_MAX])> {
    let mut h = [0u8; FHDR];
    if size < FHDR as u32 || !s.read(0, &mut h) || h[..4] != MAGIC {
        return None;
    }
    let hl = le16(&h, 6);
    let ul = (h[5] as usize).min(UNIT_MAX);
    if FHDR as u32 + hl > size {
        return None;
    }
    let mut c = crc32_upd(!0, &h[12..]);
    let mut t = [0u8; 64];
    let mut o = 0u32;
    while o < hl {
        let n = (hl - o).min(64) as usize;
        if !s.read(FHDR as u32 + o, &mut t[..n]) {
            return None;
        }
        c = crc32_upd(c, &t[..n]);
        o += n as u32;
    }
    if !c != le32(&h, 8) {
        return None;
    }
    let mut unit = [0u8; UNIT_MAX];
    unit.copy_from_slice(&h[12..24]);
    Some((h[4], hl, ul, unit))
}

/// The chunk header at `off` if it is whole within `size` (and, with
/// `check`, its CRC matches).
pub fn chunk_at<S: Src>(s: &mut S, w: &mut Win, off: u32, size: u32, check: bool) -> Option<Chunk> {
    let mut h = [0u8; CHDR];
    if off.checked_add(CHDR as u32)? > size || !s.read(off, &mut h) || h[0] != b'D' || h[1] != b'C' {
        return None;
    }
    let c = Chunk {
        off,
        pl: le16(&h, 2),
        row0: le32(&h, 8),
        csv_base: le32(&h, 12),
        csv_len: le32(&h, 16),
        t0: le32(&h, 20),
        nrows: le16(&h, 24),
        flags0: h[26],
    };
    if c.end() > size {
        return None;
    }
    if check {
        let mut crc = crc32_upd(!0, &h[8..]);
        let mut o = off + CHDR as u32;
        while o < c.end() {
            let b = w.byte(s, o, c.end())?;
            crc = crc32_upd(crc, &[b]);
            o += 1;
        }
        if !crc != le32(&h, 4) {
            return None;
        }
    }
    Some(c)
}

/// Rows of chunk `c` to `f(t_ms, value, flags)` (gallery). False on a
/// decode error.
pub fn rows<S: Src>(s: &mut S, w: &mut Win, c: &Chunk, f: &mut dyn FnMut(u32, Val, u8)) -> bool {
    let mut rd = RowDec::new(c);
    for _ in 0..c.nrows {
        match rd.next(s, w) {
            Some((t, v, fl)) => f(t, v, fl),
            None => return false,
        }
    }
    true
}

/// Summary of a DLG file (`Gen::open`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Info {
    pub kind: u8,
    /// CSV header text length.
    pub hl: u32,
    /// Exact CSV length.
    pub csv: u32,
    pub rows: u32,
    /// Whole valid chunks, and the file bytes they cover (the rest is a
    /// torn tail: a power cut, a full disk).
    pub chunks: u32,
    pub valid: u32,
    /// Time of the last row, ms.
    pub t_last: u32,
}

/// CSV generator over one DLG file (random access, sequential reads O(1)).
pub struct Gen<S: Src> {
    pub s: S,
    size: u32,
    pub info: Info,
    ul: usize,
    unit: [u8; UNIT_MAX],
    win: Win,
    // cursor
    cur: Option<Chunk>,
    rd: RowDec,
    /// CSV offset of `row`'s first byte, its text.
    rpos: u32,
    row: [u8; ROW_MAX],
    rlen: u32,
}

impl<S: Src> Gen<S> {
    /// Check the header and every chunk (CRC): the exact CSV size. None: not
    /// a DLG file of a known kind.
    pub fn open(mut s: S, size: u32) -> Option<Gen<S>> {
        let (kind, hl, ul, unit) = parse_file(&mut s, size)?;
        if kind != KIND_DMM {
            return None;
        }
        let mut g = Gen {
            s,
            size,
            info: Info { kind, hl, csv: hl, valid: FHDR as u32 + hl, ..Info::default() },
            ul,
            unit,
            win: Win::new(),
            cur: None,
            rd: RowDec::new(&Chunk { off: 0, pl: 0, row0: 0, csv_base: 0, csv_len: 0, t0: 0, nrows: 0, flags0: 0 }),
            rpos: 0,
            row: [0; ROW_MAX],
            rlen: 0,
        };
        let mut off = g.info.valid;
        while let Some(c) = chunk_at(&mut g.s, &mut g.win, off, size, true) {
            if c.csv_base != g.info.csv || c.row0 != g.info.rows {
                break;
            }
            // The last row's time needs a decode of the last chunk only.
            g.info.csv += c.csv_len;
            g.info.rows += c.nrows;
            g.info.chunks += 1;
            g.info.valid = c.end();
            off = c.end();
            if c.nrows > 0 {
                g.cur = Some(c);
            }
        }
        if let Some(c) = g.cur.take() {
            let mut rd = RowDec::new(&c);
            while let Some((t, _, _)) = rd.next(&mut g.s, &mut g.win) {
                g.info.t_last = t;
            }
        }
        g.size = g.info.valid;
        Some(g)
    }

    pub fn unit(&self) -> &[u8] {
        &self.unit[..self.ul]
    }

    /// The first chunk at or after file offset `off` (None at the end).
    pub fn chunk(&mut self, off: u32) -> Option<Chunk> {
        let off = off.max(FHDR as u32 + self.info.hl);
        chunk_at(&mut self.s, &mut self.win, off, self.size, false)
    }

    /// Rows of chunk `c` to `f(t_ms, value, flags)` (gallery). False on a
    /// decode error.
    pub fn rows(&mut self, c: &Chunk, f: &mut dyn FnMut(u32, Val, u8)) -> bool {
        rows(&mut self.s, &mut self.win, c, f)
    }

    /// The CSV header text bytes `off..` (into `buf`).
    fn header_text(&mut self, off: u32, buf: &mut [u8]) -> bool {
        self.s.read(FHDR as u32 + off, buf)
    }

    /// Decode the next row of the cursor into `row`; false at the chunk end.
    fn advance(&mut self) -> bool {
        match self.rd.next(&mut self.s, &mut self.win) {
            Some((t, v, fl)) => {
                self.rpos += self.rlen;
                let mut r = [0u8; ROW_MAX];
                let unit = self.unit;
                self.rlen = fmt_row(t as u64, v, &unit[..self.ul], fl, &mut r) as u32;
                self.row = r;
                true
            }
            None => false,
        }
    }

    /// Put the cursor at the chunk holding CSV offset `off` (a data row).
    fn seek(&mut self, off: u32) -> bool {
        let mut at = FHDR as u32 + self.info.hl;
        if let Some(c) = self.cur {
            if off >= c.csv_base {
                at = c.off;
            }
        }
        while let Some(c) = chunk_at(&mut self.s, &mut self.win, at, self.size, false) {
            if off < c.csv_base + c.csv_len {
                self.cur = Some(c);
                self.rd = RowDec::new(&c);
                self.rpos = c.csv_base;
                self.rlen = 0;
                return self.advance();
            }
            at = c.end();
        }
        false
    }

    /// CSV bytes `off .. off + buf.len()` (zeros past the end).
    pub fn read(&mut self, mut off: u32, buf: &mut [u8]) {
        let mut i = 0;
        buf.fill(0);
        while i < buf.len() && off < self.info.csv {
            if off < self.info.hl {
                let n = ((self.info.hl - off) as usize).min(buf.len() - i);
                if !self.header_text(off, &mut buf[i..i + n]) {
                    return;
                }
                i += n;
                off += n as u32;
                continue;
            }
            let inrow = self.cur.is_some() && off >= self.rpos && off < self.rpos + self.rlen;
            if !inrow {
                let next = self.cur.is_some_and(|c| off >= self.rpos + self.rlen && off < c.csv_base + c.csv_len);
                let ok = if next {
                    let mut ok = true;
                    while ok && off >= self.rpos + self.rlen {
                        ok = self.advance();
                    }
                    ok
                } else {
                    self.seek(off) && {
                        let mut ok = true;
                        while ok && off >= self.rpos + self.rlen {
                            ok = self.advance();
                        }
                        ok
                    }
                };
                if !ok {
                    self.cur = None;
                    return;
                }
            }
            let o = (off - self.rpos) as usize;
            let n = (self.rlen as usize - o).min(buf.len() - i);
            buf[i..i + n].copy_from_slice(&self.row[o..o + n]);
            i += n;
            off += n as u32;
        }
    }
}

// ------------------------------------------------------------- FAT16 files

/// Raw bytes of the disk (SPI flash: byte address = block * 4096 + offset).
pub trait Raw {
    fn read_at(&mut self, addr: u32, buf: &mut [u8]) -> bool;
}

/// FAT16 volume geometry in bytes.
#[derive(Clone, Copy, Debug)]
pub struct Geo {
    pub fat: u32,
    pub root: u32,
    pub root_entries: u32,
    pub data: u32,
    pub cb: u32,
    pub clusters: u32,
}

pub fn geometry(raw: &mut dyn Raw) -> Option<Geo> {
    let mut b = [0u8; 64];
    if !raw.read_at(0, &mut b) {
        return None;
    }
    let bps = le16(&b, 0x0B);
    let spc = b[0x0D] as u32;
    let rsvd = le16(&b, 0x0E);
    let nfats = b[0x10] as u32;
    let re = le16(&b, 0x11);
    let tot = if le16(&b, 0x13) != 0 { le16(&b, 0x13) } else { le32(&b, 0x20) };
    let fatsz = le16(&b, 0x16);
    if !(b[0] == 0xEB || b[0] == 0xE9) || bps < 512 || spc == 0 || nfats == 0 || re == 0 || fatsz == 0 {
        return None;
    }
    let root = (rsvd + nfats * fatsz) * bps;
    let data = root + re * 32;
    let clusters = (tot * bps).saturating_sub(data) / (spc * bps);
    Some(Geo { fat: rsvd * bps, root, root_entries: re, data, cb: spc * bps, clusters })
}

/// One root file as a `Src` (follows its FAT 1 chain; caches the position).
pub struct FatFile<R: Raw> {
    pub raw: R,
    g: Geo,
    start: u32,
    pub size: u32,
    ci: u32,
    cl: u32,
}

impl<R: Raw> FatFile<R> {
    pub fn new(raw: R, g: Geo, start: u32, size: u32) -> Self {
        FatFile { raw, g, start, size, ci: 0, cl: start }
    }
    fn next(&mut self, c: u32) -> Option<u32> {
        let mut b = [0u8; 2];
        if c < 2 || c > self.g.clusters + 1 || !self.raw.read_at(self.g.fat + c * 2, &mut b) {
            return None;
        }
        let n = le16(&b, 0);
        (2..0xFFF0).contains(&n).then_some(n)
    }
}

impl<R: Raw> Src for FatFile<R> {
    fn read(&mut self, mut off: u32, buf: &mut [u8]) -> bool {
        if off as u64 + buf.len() as u64 > self.size as u64 {
            return false;
        }
        let mut i = 0;
        while i < buf.len() {
            let ci = off / self.g.cb;
            if ci < self.ci {
                self.ci = 0;
                self.cl = self.start;
            }
            while self.ci < ci {
                match self.next(self.cl) {
                    Some(n) => {
                        self.cl = n;
                        self.ci += 1;
                    }
                    None => return false,
                }
            }
            if self.cl < 2 || self.cl > self.g.clusters + 1 {
                return false;
            }
            let o = off % self.g.cb;
            let n = ((self.g.cb - o) as usize).min(buf.len() - i);
            if !self.raw.read_at(self.g.data + (self.cl - 2) * self.g.cb + o, &mut buf[i..i + n]) {
                return false;
            }
            i += n;
            off += n as u32;
        }
        true
    }
}

/// A root entry: 8.3 name, start cluster, size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    pub name: [u8; 11],
    pub start: u32,
    pub size: u32,
}

/// `<prefix>NNN.DLG` (prefix LOG or SCOP): Some(prefix length).
pub fn dlg_name(n: &[u8]) -> Option<usize> {
    let up = |c: u8| c.to_ascii_uppercase();
    if up(n[8]) != b'D' || up(n[9]) != b'L' || up(n[10]) != b'G' {
        return None;
    }
    let p = if n[..3].eq_ignore_ascii_case(b"LOG") {
        3
    } else if n[..4].eq_ignore_ascii_case(b"SCOP") {
        4
    } else {
        return None;
    };
    (n[p..p + 3].iter().all(|c| c.is_ascii_digit()) && n[p + 3..8].iter().all(|&c| c == b' ')).then_some(p)
}

/// Root files `LOGnnn.DLG` / `SCOPnnn.DLG` (directory order) to `f`.
pub fn scan(raw: &mut dyn Raw, g: &Geo, f: &mut dyn FnMut(&Ent)) -> bool {
    let mut e = [0u8; 32];
    for i in 0..g.root_entries {
        if !raw.read_at(g.root + i * 32, &mut e) {
            return false;
        }
        if e[0] == 0 {
            break;
        }
        if e[0] == 0xE5 || e[11] & 0x18 != 0 || e[11] & 0x3F == 0x0F || dlg_name(&e[..11]).is_none() {
            continue;
        }
        let mut name = [0u8; 11];
        name.copy_from_slice(&e[..11]);
        f(&Ent { name, start: le16(&e, 0x1A), size: le32(&e, 0x1C) });
    }
    true
}

/// "LOG001  DLG" -> "LOG001  CSV".
pub fn csv_name(n: &[u8; 11]) -> [u8; 11] {
    let mut c = *n;
    c[8..].copy_from_slice(b"CSV");
    c
}

// ------------------------------------------------------------- device glue

/// A `Raw` through a plain function (the SPI flash on the device, an image
/// in the host tests).
#[derive(Clone, Copy)]
pub struct FnRaw(pub fn(u32, &mut [u8]) -> bool);

impl Raw for FnRaw {
    fn read_at(&mut self, addr: u32, buf: &mut [u8]) -> bool {
        (self.0)(addr, buf)
    }
}

pub type DiskGen = Gen<FatFile<FnRaw>>;

#[cfg(target_os = "none")]
static mut FLASH: Option<crate::spiflash::SpiFlash> = None;

/// The SPI flash, raw (waits for a running erase / program first).
#[cfg(target_os = "none")]
pub fn flash_read(addr: u32, buf: &mut [u8]) -> bool {
    let f = unsafe { &mut *core::ptr::addr_of_mut!(FLASH) };
    f.get_or_insert_with(crate::spiflash::SpiFlash::new).read_at(addr, buf)
}

#[cfg(not(target_os = "none"))]
fn flash_read(_: u32, _: &mut [u8]) -> bool {
    false
}

static mut DISK: FnRaw = FnRaw(flash_read);

/// Host tests: read the "SPI disk" through `f`.
pub fn set_disk(f: fn(u32, &mut [u8]) -> bool) {
    unsafe { *core::ptr::addr_of_mut!(DISK) = FnRaw(f) };
}

fn disk() -> FnRaw {
    unsafe { *core::ptr::addr_of!(DISK) }
}

/// Open root file `name` (8.3, a .DLG) of the SPI disk.
pub fn open(name: &[u8; 11]) -> Option<DiskGen> {
    let mut d = disk();
    let g = geometry(&mut d)?;
    let mut hit = None;
    scan(&mut d, &g, &mut |e| {
        if hit.is_none() && e.name.eq_ignore_ascii_case(name) {
            hit = Some(*e);
        }
    });
    let e = hit?;
    Gen::open(FatFile::new(d, g, e.start, e.size), e.size)
}

#[cfg(feature = "vdisk")]
pub use vfs::*;

/// The virtual CSV files of LUN 1 (vdisk.rs `set_extra`): one LOGnnn.CSV
/// per finished LOGnnn.DLG on the SPI disk (docs/compact-log-format.md 5).
#[cfg(feature = "vdisk")]
mod vfs {
    use super::*;
    use crate::vdisk::{Extra, MAX_EXTRA};
    use core::sync::atomic::{AtomicBool, Ordering};

    const ENT0: Ent = Ent { name: [0; 11], start: 0, size: 0 };
    static mut TAB: [Ent; MAX_EXTRA] = [ENT0; MAX_EXTRA];
    static mut NT: usize = 0;
    /// The read cursor: (table index, generator), ~0.4 KiB of .bss (not
    /// .dmabuf: that is not zeroed at reset).
    static mut CUR: Option<(u32, DiskGen)> = None;
    static STALE: AtomicBool = AtomicBool::new(true);

    /// The DLG set changed (a recording ended, a file was deleted, the PC
    /// wrote the disk): `refresh` rebuilds the table.
    pub fn set_stale() {
        STALE.store(true, Ordering::Relaxed);
    }

    pub fn stale() -> bool {
        STALE.load(Ordering::Relaxed)
    }

    /// Main loop, when `stale()` and nothing writes the disk (no recording
    /// commit, no screenshot / scope CSV job, no host write): rebuild the
    /// LUN-1 extra files (vdisk::set_extra raises its media change). The
    /// file being recorded (`exclude`) is left out until it ends. Reads every
    /// chunk header + CRC of every DLG file: ~10 ms per 100 KB of logs.
    /// Returns the number of files.
    pub fn refresh(exclude: Option<[u8; 11]>) -> usize {
        STALE.store(false, Ordering::Relaxed);
        unsafe { *core::ptr::addr_of_mut!(CUR) = None };
        let tab = unsafe { &mut *core::ptr::addr_of_mut!(TAB) };
        let mut x = [Extra { name: [b' '; 11], size: 0, read: vread, arg: 0 }; MAX_EXTRA];
        let mut n = 0;
        let mut d = disk();
        if let Some(g) = geometry(&mut d) {
            // The root's DLG files into TAB, then the valid ones compacted.
            let mut ne = 0;
            scan(&mut d, &g, &mut |e| {
                if ne < MAX_EXTRA && Some(e.name) != exclude {
                    tab[ne] = *e;
                    ne += 1;
                }
            });
            for i in 0..ne {
                let e = tab[i];
                #[cfg(target_os = "none")]
                crate::wdt::kick_if_feeding();
                let Some(gen) = Gen::open(FatFile::new(d, g, e.start, e.size), e.size) else { continue };
                tab[n] = e;
                x[n] = Extra { name: csv_name(&e.name), size: gen.info.csv, read: vread, arg: n as u32 };
                n += 1;
            }
        }
        unsafe { *core::ptr::addr_of_mut!(NT) = n };
        crate::vdisk::set_extra(&x[..n])
    }

    /// `Extra::read`: CSV bytes `off..` of table entry `arg`.
    pub fn vread(arg: u32, off: u32, buf: &mut [u8]) {
        buf.fill(0);
        let cur = unsafe { &mut *core::ptr::addr_of_mut!(CUR) };
        if cur.as_ref().map(|c| c.0) != Some(arg) {
            *cur = None;
            if arg as usize >= unsafe { *core::ptr::addr_of!(NT) } {
                return;
            }
            let e = unsafe { (*core::ptr::addr_of!(TAB))[arg as usize] };
            let mut d = disk();
            let Some(g) = geometry(&mut d) else { return };
            let Some(gen) = Gen::open(FatFile::new(d, g, e.start, e.size), e.size) else { return };
            *cur = Some((arg, gen));
        }
        if let Some((_, g)) = cur.as_mut() {
            g.read(off, buf);
        }
    }
}
