//! The fast logger's per-pass core (fastlog.rs; docs/compact-log-format.md
//! "Fast stream (kind 3)"): the kind-3 row coder and the pager that moves
//! the coded bytes from a RAM FIFO into pre-erased SPI-flash pages.
//!
//! Hot: this runs on every main-loop pass while an AD2 stream ISR session
//! is live, so it lives in zero-wait flash (link-native.x region B: not in
//! any `.cold` pattern) and calls nothing in a `.cold` module (dlog.rs is
//! cold: the CRC and the Rice coder are local copies here). No crate
//! dependencies: the host tests include it by path (tools/dlog-test,
//! tools/fatren-test).
//!
//! Byte flow. The file is a contiguous byte range of pre-allocated,
//! pre-erased clusters (`ext`: up to NEXT extents of consecutive clusters).
//! Every byte gets its file offset `wr` when appended; the FIFO slot is
//! `wr % fifo.len()` (a power of two, a multiple of PAGE), so a 256-byte
//! flash page is contiguous in the FIFO. `done` (page aligned) = bytes in
//! whole programmed pages; [done, wr) is still in RAM.
//!   * a chunk opens with a CHDR-byte placeholder of 0xFF (a page program
//!     of 0xFF leaves the erased bytes as they are);
//!   * full pages go out one page program per `pump` call when the chip is
//!     idle (one RDSR to see that; never a wait);
//!   * when a chunk closes its header is final: the bytes still in the FIFO
//!     are patched there, the bytes of pages already programmed go out as a
//!     direct program of those bytes (`q`, programmed before the next page:
//!     NOR programming only clears bits and they are 0xFF on the chip);
//!   * a chunk closed by time also programs the partial last page (the rows
//!     are on the chip within ~1 s even at 2.5 S/s); the page is programmed
//!     again, whole, when it fills (same bytes: no bit changes).
//! Power cut: the chip holds every page programmed so far; a chunk whose
//! header was not programmed yet reads as 0xFF (no "DC"), one whose payload
//! is short fails its CRC: the readers stop there (a torn tail), as for
//! kind 1. Nothing here touches a FAT or directory block.

#![cfg_attr(not(target_os = "none"), allow(dead_code))]

/// Chunk header bytes (= dlog::CHDR).
pub const CHDR: usize = 28;
pub const PAGE: u32 = 256;
/// Extents of the pre-allocated cluster chain.
pub const NEXT: usize = 24;
/// Worst-case bytes one row adds (two Rice escapes, 2 x 56 bits) plus the
/// close's pad byte.
pub const ROW_WORST: u32 = 15;
/// A chunk closes at this payload size, ...
const CHUNK_BYTES: u32 = 4096;
/// ... or after this much time with at least CHUNK_MIN_ROWS rows, ...
const CHUNK_US: u64 = 1_000_000;
const CHUNK_MIN_ROWS: u32 = 64;
/// ... or after this much time anyway (2.5 S/s: 25 rows per chunk).
const CHUNK_MAX_US: u64 = 10_000_000;
const AT0: u32 = 4;
const AV0: u32 = 16;
const QMAX: u32 = 24;

/// Raw NOR access the run needs (spiflash.rs `SpiFlash`; an image in the
/// host tests).
pub trait Nor {
    /// An erase or program is running (one status read, no waiting).
    fn busy(&mut self) -> bool;
    /// Start programming `d` at `addr` (within one 256-byte page); returns
    /// once the data is clocked out, the chip is busy afterwards.
    fn program(&mut self, addr: u32, d: &[u8]);
    /// Start erasing the 4 KiB sector (or with `big` the 64 KiB block)
    /// holding `addr`; returns at once.
    fn erase(&mut self, addr: u32, big: bool);
    /// Read (waits for a running erase / program first). False: failed.
    fn read(&mut self, addr: u32, b: &mut [u8]) -> bool;
}

/// CRC-32 (zlib) (= dlog::crc32_upd, which is `.cold`). A byte table
/// (1 KiB, region B): a chunk's payload is checked in one pass at its close
/// (~5 instructions per byte).
pub fn crc32_upd(mut c: u32, d: &[u8]) -> u32 {
    const T: [u32; 256] = {
        let mut t = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = (c >> 1) ^ (0xEDB8_8320 & (c & 1).wrapping_neg());
                k += 1;
            }
            t[i] = c;
            i += 1;
        }
        t
    };
    for &b in d {
        c = (c >> 8) ^ T[((c ^ b as u32) & 0xFF) as usize];
    }
    c
}

/// Adaptive Rice parameter (= dlog's `Ad`).
#[derive(Clone, Copy)]
pub(crate) struct Ad {
    pub(crate) a: u32,
    pub(crate) n: u32,
}

impl Ad {
    pub(crate) const fn new(a: u32) -> Ad {
        Ad { a, n: 1 }
    }
    /// The smallest k <= 30 with n << k >= a (dlog's loop `while k < 30 &&
    /// (n << k) < a { k += 1 }`; a < 2^30 always: `upd` adds <= 2^24 per
    /// row and halves at n = 32), without the loop: n << d has a's bit
    /// length, so k is d or d + 1 (tools/dlog-test checks it against the
    /// loop).
    #[inline(always)]
    pub(crate) fn k(&self) -> u32 {
        let (n, a) = (self.n, self.a);
        if n >= a {
            return 0;
        }
        let d = n.leading_zeros() - a.leading_zeros();
        (d + ((n << d) < a) as u32).min(30)
    }

    /// The loop form (the reference for the host test).
    #[allow(dead_code)]
    pub(crate) fn k_loop(&self) -> u32 {
        let mut k = 0;
        while k < 30 && (self.n << k) < self.a {
            k += 1;
        }
        k
    }
    #[inline(always)]
    pub(crate) fn upd(&mut self, x: u32) {
        self.a = self.a.saturating_add(x.min(1 << 24));
        self.n += 1;
        if self.n >= 32 {
            self.a = (self.a + 1) >> 1;
            self.n >>= 1;
        }
    }
}

#[inline(always)]
fn zz(x: i32) -> u32 {
    ((x << 1) ^ (x >> 31)) as u32
}

/// Header bytes to program directly (their page already went out).
#[derive(Clone, Copy)]
struct Piece {
    addr: u32,
    n: u8,
    b: [u8; CHDR],
}

const PIECE0: Piece = Piece { addr: 0, n: 0, b: [0; CHDR] };

/// What `pump` did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pump {
    /// The chip is busy with the last operation.
    Busy,
    /// Started a program.
    Started,
    /// Nothing to program now.
    Idle,
}

/// Coder + pager state (all zero / const `EMPTY` at rest).
#[derive(Clone, Copy)]
pub struct FastEnc {
    /// Extents (first cluster, clusters) of the file, in file order; byte
    /// address of cluster 2; bytes per cluster; allocated bytes.
    pub ext: [(u32, u32); NEXT],
    pub next: usize,
    pub data: u32,
    pub cb: u32,
    pub cap: u32,
    /// Bytes appended (file offset of the next), bytes in whole programmed
    /// pages (page aligned).
    pub wr: u32,
    pub done: u32,
    /// Bytes [done, part) of the partial page are programmed too.
    part: u32,
    fl: u32,
    q: [Piece; 4],
    qn: usize,
    flush_req: bool,
    // coder
    /// Bit buffer: `nb` (< 8 between calls) bits pending in `acc`.
    acc: u32,
    nb: u32,
    at: Ad,
    av: Ad,
    dt: u32,
    p_us: u32,
    have: bool,
    last_ts: u32,
    t: u64,
    v: i32,
    // chunk
    open: bool,
    hpos: u32,
    row0: u32,
    nrows: u32,
    t0: u64,
    dt0: u32,
    v0: i32,
    /// Rows coded, chunks closed, programs started (all; of them partial
    /// pages and header pieces).
    pub rows: u32,
    pub chunks: u32,
    pub programs: u32,
    pub partials: u32,
    pub pieces: u32,
    /// Time of the first / last row (device us, unwrapped).
    pub t_first: u64,
    pub t_last: u64,
}

impl FastEnc {
    pub const EMPTY: FastEnc = FastEnc {
        ext: [(0, 0); NEXT],
        next: 0,
        data: 0,
        cb: 4096,
        cap: 0,
        wr: 0,
        done: 0,
        part: 0,
        fl: 0,
        q: [PIECE0; 4],
        qn: 0,
        flush_req: false,
        acc: 0,
        nb: 0,
        at: Ad::new(AT0),
        av: Ad::new(AV0),
        dt: 0,
        p_us: 0,
        have: false,
        last_ts: 0,
        t: 0,
        v: 0,
        open: false,
        hpos: 0,
        row0: 0,
        nrows: 0,
        t0: 0,
        dt0: 0,
        v0: 0,
        rows: 0,
        chunks: 0,
        programs: 0,
        partials: 0,
        pieces: 0,
        t_first: 0,
        t_last: 0,
    };

    /// A new file over extents `ext` (cluster numbers, counts), cluster 2 at
    /// byte `data`, `cb` bytes per cluster; FIFO of `fifo_len` bytes (a
    /// power of two, multiple of PAGE); nominal period `p_us`.
    pub fn new(ext: &[(u32, u32)], data: u32, cb: u32, fifo_len: usize, p_us: u32) -> FastEnc {
        let mut e = FastEnc::EMPTY;
        let n = ext.len().min(NEXT);
        e.ext[..n].copy_from_slice(&ext[..n]);
        e.next = n;
        e.data = data;
        e.cb = cb;
        e.cap = ext[..n].iter().map(|x| x.1 * cb).sum();
        e.fl = fifo_len as u32;
        e.p_us = p_us;
        e
    }

    /// Flash byte address of file offset `off`.
    pub fn addr(&self, off: u32) -> u32 {
        let mut ci = off / self.cb;
        for &(c, n) in &self.ext[..self.next] {
            if ci < n {
                return self.data + (c - 2 + ci) * self.cb + off % self.cb;
            }
            ci -= n;
        }
        u32::MAX
    }

    /// FIFO bytes free.
    #[inline(always)]
    pub fn free(&self) -> u32 {
        self.fl - (self.wr - self.done)
    }

    /// Room for `rows` more rows in the FIFO and in the file.
    #[inline(always)]
    pub fn room(&self, rows: u32) -> bool {
        let need = rows * ROW_WORST + CHDR as u32 + 1;
        self.free() >= need && self.wr + need <= self.cap
    }

    /// The file is full (no room for one more row).
    pub fn full(&self) -> bool {
        self.wr + ROW_WORST + CHDR as u32 + 1 > self.cap
    }

    #[inline(always)]
    fn byte(&mut self, fifo: &mut [u8], b: u8) {
        fifo[(self.wr & (self.fl - 1)) as usize] = b;
        self.wr += 1;
    }

    /// Raw bytes (the file header), outside any chunk; programmed at the
    /// next `pump`s (the partial page too).
    pub fn put_raw(&mut self, fifo: &mut [u8], d: &[u8]) {
        for &b in d {
            self.byte(fifo, b);
        }
        self.flush_req = true;
    }

    /// The low `bits` (<= 24) bits of `v`, MSB first. 32-bit only: with
    /// < 8 bits pending they fit (a u64 buffer cost library shift calls at
    /// opt-level z, ~10 % of the coder's CPU on the device model).
    #[inline(always)]
    fn put24(&mut self, fifo: &mut [u8], v: u32, bits: u32) {
        let acc = (self.acc << bits) | (v & ((1u32 << bits) - 1));
        let mut nb = self.nb + bits;
        while nb >= 8 {
            nb -= 8;
            self.byte(fifo, (acc >> nb) as u8);
        }
        self.acc = acc & ((1u32 << nb) - 1);
        self.nb = nb;
    }

    /// The low `bits` (<= 32) bits of `v`.
    #[inline(never)]
    fn put(&mut self, fifo: &mut [u8], v: u32, bits: u32) {
        if bits > 24 {
            self.put24(fifo, v >> 16, bits - 16);
            self.put24(fifo, v & 0xFFFF, 16);
        } else {
            self.put24(fifo, v, bits);
        }
    }

    #[inline(always)]
    fn rice(&mut self, fifo: &mut [u8], x: u32, k: u32) {
        let q = x >> k;
        if q < QMAX {
            // q ones and the stop zero (<= 24 bits), then k low bits: one
            // put when both fit 24 bits (the usual case)
            let ones = ((1u32 << q) - 1) << 1;
            if q + 1 + k <= 24 {
                self.put24(fifo, ones << k | (x & ((1u32 << k) - 1)), q + 1 + k);
            } else {
                self.put(fifo, ones, q + 1);
                self.put(fifo, x, k);
            }
        } else {
            self.put(fifo, (1 << QMAX) - 1, QMAX);
            self.put(fifo, x, 32);
        }
    }

    /// One record (device stamp `ts` us, wrapping u32; raw value). The
    /// caller checked `room(1)`.
    pub fn add(&mut self, fifo: &mut [u8], ts: u32, raw: i32) {
        let (t, dt) = if self.have {
            let d = ts.wrapping_sub(self.last_ts);
            (self.t + d as u64, d)
        } else {
            self.t_first = ts as u64;
            (ts as u64, 0)
        };
        self.have = true;
        self.last_ts = ts;
        if !self.open {
            self.hpos = self.wr;
            for _ in 0..CHDR {
                self.byte(fifo, 0xFF);
            }
            self.open = true;
            self.row0 = self.rows;
            self.nrows = 1;
            self.t0 = t;
            self.dt0 = dt;
            self.v0 = raw;
            self.dt = self.p_us;
            self.at = Ad::new(AT0);
            self.av = Ad::new(AV0);
            self.acc = 0;
            self.nb = 0;
        } else {
            let x = zz(dt.wrapping_sub(self.dt) as i32);
            self.dt = dt;
            let k = self.at.k();
            self.rice(fifo, x, k);
            self.at.upd(x);
            let u = zz(raw.wrapping_sub(self.v));
            let k = self.av.k();
            self.rice(fifo, u, k);
            self.av.upd(u);
            self.nrows += 1;
        }
        self.t = t;
        self.t_last = t;
        self.v = raw;
        self.rows += 1;
        let pl = self.wr - self.hpos - CHDR as u32;
        let age = t - self.t0;
        if self.nrows >= 0xFFFF || pl >= CHUNK_BYTES {
            self.close(fifo, false);
        } else if age >= CHUNK_US && (self.nrows >= CHUNK_MIN_ROWS || age >= CHUNK_MAX_US) {
            self.close(fifo, true);
        }
    }

    /// Room for a chunk close's header pieces.
    #[inline(always)]
    pub fn can_close(&self) -> bool {
        self.qn + 2 <= self.q.len()
    }

    /// Close the open chunk (if any): pad bits, header with its CRC, placed
    /// in the FIFO and / or queued for a direct program. `flush`: also
    /// program the partial last page.
    pub fn close(&mut self, fifo: &mut [u8], flush: bool) {
        if !self.open || !self.can_close() {
            return;
        }
        if self.nb > 0 {
            let b = (self.acc << (8 - self.nb)) as u8;
            self.byte(fifo, b);
        }
        self.acc = 0;
        self.nb = 0;
        let pl = self.wr - self.hpos - CHDR as u32;
        let mut h = [0u8; CHDR];
        h[0] = b'D';
        h[1] = b'C';
        h[2..4].copy_from_slice(&(pl as u16).to_le_bytes());
        h[8..12].copy_from_slice(&self.row0.to_le_bytes());
        h[12..16].copy_from_slice(&(self.t0 as u32).to_le_bytes());
        h[16..18].copy_from_slice(&((self.t0 >> 32) as u16).to_le_bytes());
        h[18..20].copy_from_slice(&(self.nrows as u16).to_le_bytes());
        h[20..24].copy_from_slice(&self.dt0.to_le_bytes());
        h[24..28].copy_from_slice(&self.v0.to_le_bytes());
        // The payload's CRC from the FIFO: a chunk (<= CHUNK_BYTES + a row +
        // CHDR) is shorter than the FIFO, so all of its bytes are still
        // there, programmed or not (at most one wrap).
        let s = self.hpos + CHDR as u32;
        let len = (self.wr - s) as usize;
        let a0 = (s & (self.fl - 1)) as usize;
        let first = len.min(self.fl as usize - a0);
        let c = crc32_upd(!0, &fifo[a0..a0 + first]);
        let c = crc32_upd(c, &fifo[..len - first]);
        let c = !crc32_upd(c, &h[8..]);
        h[4..8].copy_from_slice(&c.to_le_bytes());
        // Below `done`: direct pieces (split at page edges); the rest: FIFO.
        let mut i = 0usize;
        while i < CHDR {
            let off = self.hpos + i as u32;
            if off >= self.done {
                fifo[(off & (self.fl - 1)) as usize] = h[i];
                // Programmed (0xFF) by a partial program: again.
                if off < self.part {
                    self.part = off;
                }
                i += 1;
                continue;
            }
            let page_end = (off / PAGE + 1) * PAGE;
            let n = ((page_end.min(self.done) - off) as usize).min(CHDR - i);
            let addr = self.addr(off);
            let p = &mut self.q[self.qn];
            p.addr = addr;
            p.n = n as u8;
            p.b[..n].copy_from_slice(&h[i..i + n]);
            self.qn += 1;
            i += n;
        }
        self.open = false;
        self.chunks += 1;
        if flush {
            self.flush_req = true;
        }
    }

    /// One chip operation if the chip is idle: a queued header piece, else
    /// a full page, else (a flush was asked) the partial last page.
    pub fn pump(&mut self, fifo: &[u8], nor: &mut dyn Nor) -> Pump {
        if nor.busy() {
            return Pump::Busy;
        }
        if self.qn > 0 {
            let p = self.q[0];
            nor.program(p.addr, &p.b[..p.n as usize]);
            self.q.copy_within(1..self.qn, 0);
            self.qn -= 1;
            self.programs += 1;
            self.pieces += 1;
            return Pump::Started;
        }
        if self.wr - self.done >= PAGE {
            let s = (self.done & (self.fl - 1)) as usize;
            nor.program(self.addr(self.done), &fifo[s..s + PAGE as usize]);
            self.done += PAGE;
            self.part = self.done;
            self.programs += 1;
            return Pump::Started;
        }
        if self.flush_req {
            self.flush_req = false;
            if self.part < self.wr {
                let s = (self.done & (self.fl - 1)) as usize;
                let n = (self.wr - self.done) as usize;
                nor.program(self.addr(self.done), &fifo[s..s + n]);
                self.part = self.wr;
                self.programs += 1;
                self.partials += 1;
                return Pump::Started;
            }
        }
        Pump::Idle
    }

    /// Everything appended is on the chip (or programming): no piece, no
    /// full page, the partial page programmed.
    pub fn flushed(&self) -> bool {
        self.qn == 0 && self.wr - self.done < PAGE && (self.wr == self.done || self.part >= self.wr) && !self.flush_req
    }

    /// End of the stream: close the chunk and program everything.
    pub fn finish(&mut self, fifo: &mut [u8]) {
        if self.open {
            self.close(fifo, true);
        }
        self.flush_req = true;
    }

    /// A chunk is open.
    pub fn is_open(&self) -> bool {
        self.open
    }
}
