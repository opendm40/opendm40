//! Protocol decoder (TODO "Protocol decoders", phase 1): UART and Modbus RTU
//! decoded while the scope streams, so only the decoded frames are kept and
//! the raw samples go.
//!
//! Source: the scope's circular DMA record (scope.rs `dec_*`: BUF, CAP
//! samples, never stopped) at a rate chosen from the baud (`fs_for`, up to
//! 1.25 MS/s). The main loop calls `Dec::poll` with the ring and the
//! absolute number of samples written so far; the decoder keeps absolute
//! positions and must be called before the DMA laps them (CAP / fs: 21 ms
//! at 1.25 MS/s, 213 ms at 125 kS/s). A lap is counted as an overrun.
//!
//! Per poll:
//!  1. threshold (auto): min / max of every 4th sample over windows of
//!     `WIN` samples; a window whose swing is at least `MIN_SWING` codes sets
//!     the mid level and a hysteresis of 1/6 of the swing. Until a window
//!     has the swing nothing is decoded (an idle or open input gives no
//!     frames). Manual: a fixed code with `HYST_MANUAL` codes either side;
//!  2. bit rate (auto): the edge-to-edge runs (hysteresis) of the last
//!     `NRUN` edges; the shortest run (>= `RUN_MIN` samples) and the runs
//!     within 30 % of it give the bit time; within 8 % of a standard rate it
//!     snaps to it. Decoding starts after the lock from the oldest samples
//!     still in the ring (the bytes that gave the bit time are decoded too),
//!     from the first idle gap of a character or more if there is one. 8 or
//!     more framing errors among the last 16 characters drop the lock;
//!  3. characters: a start edge (hysteresis), then each bit sampled at its
//!     centre (majority of 3 samples around the mid level); start bit,
//!     data LSB first, parity, one stop bit (framing error if it is space);
//!  4. frames: UART bytes are grouped while the gap between characters is
//!     under 2 character times (at most `UART_MAX` bytes per frame); Modbus
//!     RTU frames end at a 3.5-character gap (1.75 ms above 19200 baud),
//!     CRC-16 checked, request / reply paired (`classify`).
//!
//! Frames go to `Store` (headers + a byte ring, `.sram_bss`), read by the
//! decoder view (ui_decode.rs) and the host (DECode:FRAMes?, its own cursor).
//! Pure: no hardware access, host-tested by tools/decode-test.

/// Standard bit rates (auto-baud snaps to these).
pub const STD_BAUD: [u32; 13] = [300, 600, 1200, 2400, 4800, 9600, 14400, 19200, 28800, 38400, 57600, 76800, 115_200];
/// Highest bit rate (1.25 MS/s: 10.9 samples per bit).
pub const MAX_BAUD: u32 = 115_200;
pub const MIN_BAUD: u32 = 300;
/// Sample rates of the scope timer table (scope.rs HW) the decoder uses.
const RATES: [u32; 12] = [6_250, 12_500, 25_000, 50_000, 62_500, 100_000, 125_000, 250_000, 500_000, 625_000, 1_000_000, 1_250_000];
/// Highest stream rate (scope.rs STREAM_FS).
pub const FS_MAX: u32 = 1_250_000;

/// Frame formats: data bits, parity (0 none, 1 even, 2 odd).
pub const FMT_NAMES: [&str; 4] = ["8N1", "8E1", "8O1", "7E1"];
const FMT: [(u8, u8); 4] = [(8, 0), (8, 1), (8, 2), (7, 1)];

/// Auto threshold: window (samples), the swing needed (codes; ~0.6 V at the
/// decoder's 2 V/div), the hysteresis of a manual threshold (codes).
const WIN: u32 = 2048;
pub const MIN_SWING: u16 = 32;
const HYST_MANUAL: u16 = 6;
/// Auto-baud: runs kept, the shortest run counted (samples; noise below).
const NRUN: usize = 32;
const RUN_MIN: u32 = 4;
/// Runs needed before a lock is tried.
const RUN_LOCK: u32 = 16;
/// Samples kept clear of the DMA's write position (a lap counts as lost).
const MARGIN: u64 = 256;

/// UART frame: at most this many bytes (the list shows one frame per row).
pub const UART_MAX: usize = 8;
/// Modbus RTU frame: at most 256 bytes.
pub const MB_MAX: usize = 256;

/// Frame flags.
pub const F_FE: u8 = 1;
pub const F_PE: u8 = 2;
pub const F_CRC: u8 = 4;
pub const F_OVR: u8 = 8;
pub const F_LONG: u8 = 16;
/// Frame kinds.
pub const K_UART: u8 = 0;
pub const K_REQ: u8 = 1;
pub const K_RSP: u8 = 2;
pub const K_EXC: u8 = 3;
/// Modbus frame that is no request / reply (bad CRC, too short).
pub const K_MB: u8 = 4;

/// The lowest timer rate with >= 12 samples per bit (auto: the top rate).
pub fn fs_for(baud: u32) -> u32 {
    if baud == 0 {
        return FS_MAX;
    }
    for r in RATES {
        if r >= baud.saturating_mul(12) {
            return r;
        }
    }
    FS_MAX
}

/// Decoder settings (DECode:..., the setup popup).
#[derive(Clone, Copy, PartialEq)]
pub struct Cfg {
    pub modbus: bool,
    /// Bit rate, 0 = auto.
    pub baud: u32,
    /// FMT_NAMES index.
    pub fmt: u8,
    /// RS-232 polarity: idle low, a space high.
    pub inverted: bool,
}

impl Cfg {
    pub const DEFAULT: Cfg = Cfg { modbus: false, baud: 0, fmt: 0, inverted: false };
}

/// Error and traffic counters since the last clear.
#[derive(Clone, Copy, Default)]
pub struct Cnt {
    pub frames: u32,
    pub bytes: u32,
    /// Framing / parity errors (characters), CRC errors (Modbus frames).
    pub fe: u32,
    pub pe: u32,
    pub crc: u32,
    /// Stream laps (samples lost before they were decoded).
    pub ovr: u32,
}

/// One stored frame.
#[derive(Clone, Copy)]
pub struct Hdr {
    /// Start (the first start edge), us since boot.
    pub t_us: u64,
    /// Absolute byte offset in the byte ring.
    pub off: u32,
    pub len: u16,
    pub flags: u8,
    pub kind: u8,
    /// Modbus reply: the request's sequence number + 1 (0: none) and the
    /// time from the request's end to this frame's start.
    pub req: u32,
    pub lat_us: u32,
}

impl Hdr {
    const ZERO: Hdr = Hdr { t_us: 0, off: 0, len: 0, flags: 0, kind: 0, req: 0, lat_us: 0 };
}

/// Stored frames (headers) and their bytes.
pub const NF: usize = 256;
pub const NB: usize = 4096;

pub struct Store {
    hdr: [Hdr; NF],
    bytes: [u8; NB],
    /// Sequence numbers: oldest kept, next to come.
    pub first: u32,
    pub next: u32,
    boff: u32,
    /// The host's FIFO (DECode:FRAMes?): next sequence number to send.
    pub host: u32,
    pub cnt: Cnt,
}

impl Store {
    pub const fn new() -> Store {
        Store { hdr: [Hdr::ZERO; NF], bytes: [0; NB], first: 0, next: 0, boff: 0, host: 0, cnt: Cnt { frames: 0, bytes: 0, fe: 0, pe: 0, crc: 0, ovr: 0 } }
    }

    pub fn clear(&mut self) {
        self.first = self.next;
        self.host = self.next;
        self.cnt = Cnt::default();
    }

    pub fn len(&self) -> u32 {
        self.next - self.first
    }

    pub fn get(&self, seq: u32) -> Option<&Hdr> {
        (seq >= self.first && seq < self.next).then(|| &self.hdr[seq as usize % NF])
    }

    /// Byte `i` of a frame.
    pub fn byte(&self, h: &Hdr, i: usize) -> u8 {
        self.bytes[(h.off as usize + i) % NB]
    }

    /// Copy a frame's bytes into `out` (as many as fit): the count.
    pub fn copy(&self, h: &Hdr, out: &mut [u8]) -> usize {
        let n = (h.len as usize).min(out.len());
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o = self.byte(h, i);
        }
        n
    }

    fn push(&mut self, mut h: Hdr, data: &[u8]) -> u32 {
        let n = data.len().min(NB / 2);
        while self.first < self.next && (self.next - self.first) as usize >= NF {
            self.first += 1;
        }
        while self.first < self.next && self.boff + n as u32 - self.hdr[self.first as usize % NF].off > NB as u32 {
            self.first += 1;
        }
        h.off = self.boff;
        h.len = n as u16;
        for (i, &b) in data[..n].iter().enumerate() {
            self.bytes[(self.boff as usize + i) % NB] = b;
        }
        self.boff = self.boff.wrapping_add(n as u32);
        let seq = self.next;
        self.hdr[seq as usize % NF] = h;
        self.next += 1;
        if self.host < self.first {
            self.host = self.first;
        }
        self.cnt.frames += 1;
        seq
    }
}

/// Modbus CRC-16 (poly 0xA001, init 0xFFFF).
pub fn crc16(d: &[u8]) -> u16 {
    let mut c = 0xFFFFu16;
    for &b in d {
        c ^= b as u16;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xA001 } else { c >> 1 };
        }
    }
    c
}

/// A frame's Modbus shape (`d` its bytes): (request-shaped, reply-shaped)
/// for the common functions; (false, false) for any other.
fn shape(d: &[u8]) -> (bool, bool) {
    let n = d.len();
    if n < 4 {
        return (false, false);
    }
    let f = d[1];
    if f & 0x80 != 0 {
        return (false, n == 5);
    }
    match f {
        1..=4 => (n == 8, n >= 6 && n == 5 + d[2] as usize),
        5 | 6 => (n == 8, n == 8),
        15 | 16 => (n >= 10 && n == 9 + d[6] as usize, n == 8),
        _ => (false, false),
    }
}

/// Modbus fields of a frame (for the list and DECode:FRAMes?).
pub struct MbInfo {
    pub addr: u8,
    pub func: u8,
    /// Request: start register / coil; reply to 03 / 04 with a paired
    /// request: its start; else u16::MAX.
    pub reg: u16,
    /// Request: count (03 / 04 / 01 / 02 / 15 / 16) or the value (05 / 06);
    /// reply: the byte count (01..04) or the count / value echoed.
    pub n: u16,
    /// Exception code (K_EXC).
    pub exc: u8,
}

pub fn mb_info(d: &[u8], kind: u8) -> MbInfo {
    let g = |i: usize| if i + 1 < d.len() { (d[i] as u16) << 8 | d[i + 1] as u16 } else { 0 };
    let mut m = MbInfo { addr: d.first().copied().unwrap_or(0), func: d.get(1).copied().unwrap_or(0), reg: u16::MAX, n: 0, exc: 0 };
    match kind {
        K_EXC => m.exc = d.get(2).copied().unwrap_or(0),
        K_REQ => {
            m.reg = g(2);
            m.n = g(4);
        }
        K_RSP => match m.func {
            1..=4 => m.n = d.get(2).copied().unwrap_or(0) as u16,
            _ => {
                m.reg = g(2);
                m.n = g(4);
            }
        },
        _ => {}
    }
    m
}

/// The UART character decoder (steps 1-3 of the module notes).
struct Uart {
    fs: u32,
    /// Samples per bit, Q8; 0 = not locked (auto).
    spb: u32,
    /// The bit rate of `spb` (set or snapped).
    baud: u32,
    bits: u8,
    parity: u8,
    inv: bool,
    manual: Option<u16>,
    lo: u16,
    mid: u16,
    hi: u16,
    thr_ok: bool,
    wmin: u16,
    wmax: u16,
    wcnt: u32,
    /// Threshold scan position (absolute sample).
    tpos: u64,
    /// Decode position (idle: the next sample to examine).
    pos: u64,
    mark: bool,
    in_char: bool,
    /// Start edge (Q8 samples), bit index, data, ones, errors.
    t0: u64,
    k: u8,
    acc: u16,
    ones: u8,
    err: u8,
    /// Auto-baud measurement: position, level, last edge, runs (length,
    /// end position, level after the run: true = the run was space).
    mpos: u64,
    mmark: bool,
    medge: u64,
    /// `medge` is the measurement's start, not an edge; the mark level
    /// from there to the first (falling) edge: (length, edge).
    mfirst: bool,
    lead: Option<(u32, u64)>,
    runs: [(u32, u64, bool); NRUN],
    nrun: u32,
    /// Framing errors of the last 16 characters (bit = 1).
    fe_hist: u16,
}

/// A decoded character: start edge, end (stop-bit centre) in samples, the
/// data, F_FE / F_PE.
#[derive(Clone, Copy)]
struct Ch {
    t: u64,
    end: u64,
    v: u16,
    err: u8,
}

impl Uart {
    const fn new() -> Uart {
        Uart {
            fs: FS_MAX,
            spb: 0,
            baud: 0,
            bits: 8,
            parity: 0,
            inv: false,
            manual: None,
            lo: 0,
            mid: 0,
            hi: 0,
            thr_ok: false,
            wmin: u16::MAX,
            wmax: 0,
            wcnt: 0,
            tpos: 0,
            pos: 0,
            mark: true,
            in_char: false,
            t0: 0,
            k: 0,
            acc: 0,
            ones: 0,
            err: 0,
            mpos: 0,
            mmark: true,
            medge: u64::MAX,
            mfirst: false,
            lead: None,
            runs: [(0, 0, false); NRUN],
            nrun: 0,
            fe_hist: 0,
        }
    }

    /// Character time (start + data + parity + stop), samples.
    fn char_samples(&self) -> u64 {
        (self.spb as u64 * (self.bits as u64 + 2 + (self.parity != 0) as u64)) >> 8
    }

    fn unlock(&mut self) {
        self.spb = 0;
        self.baud = 0;
        self.mpos = self.pos;
        self.medge = u64::MAX;
        self.lead = None;
        self.nrun = 0;
        self.in_char = false;
        self.fe_hist = 0;
    }

    fn set_manual(&mut self, code: Option<u16>) {
        self.manual = code;
        if let Some(c) = code {
            self.mid = c;
            self.lo = c.saturating_sub(HYST_MANUAL);
            self.hi = c + HYST_MANUAL;
            self.thr_ok = true;
        } else {
            self.thr_ok = false;
            self.wmin = u16::MAX;
            self.wmax = 0;
            self.wcnt = 0;
        }
    }

    /// Line level of a code (true = mark) with the hysteresis around `was`.
    #[inline(always)]
    fn level(&self, v: u16, was: bool) -> bool {
        let high = if v > self.hi {
            true
        } else if v < self.lo {
            false
        } else {
            return was;
        };
        high != self.inv
    }
}

/// Ring access with the write position: absolute sample n (wr - len <= n
/// < wr) is at `wr_idx - (wr - n)` modulo the ring length.
struct Ring<'a> {
    d: &'a [u16],
    wr: u64,
    wi: usize,
}

impl Ring<'_> {
    #[inline(always)]
    fn at(&self, n: u64) -> u16 {
        let back = (self.wr - n) as usize;
        let i = if back <= self.wi { self.wi - back } else { self.wi + self.d.len() - back };
        self.d[i] & 0x3FF
    }
    #[inline(always)]
    fn idx(&self, n: u64) -> usize {
        let back = (self.wr - n) as usize;
        if back <= self.wi { self.wi - back } else { self.wi + self.d.len() - back }
    }
}

/// The decoder: characters, frames, the store.
pub struct Dec {
    pub cfg: Cfg,
    u: Uart,
    /// Session start (us since boot) and its sample rate.
    t0_us: u64,
    /// The frame being collected: bytes, start, end (samples), flags.
    buf: [u8; MB_MAX],
    n: usize,
    f_t: u64,
    f_end: u64,
    f_flags: u8,
    /// Modbus: the request waiting for its reply (seq, addr, func, end us).
    pend: Option<(u32, u8, u8, u64)>,
    pub st: Store,
}

impl Dec {
    pub const fn new() -> Dec {
        Dec { cfg: Cfg::DEFAULT, u: Uart::new(), t0_us: 0, buf: [0; MB_MAX], n: 0, f_t: 0, f_end: 0, f_flags: 0, pend: None, st: Store::new() }
    }

    /// A zero-filled Dec (a static in a zeroed section): everything but the
    /// store (all zero in `new` too) set as `new` does.
    pub fn init(&mut self) {
        self.cfg = Cfg::DEFAULT;
        self.u = Uart::new();
        self.t0_us = 0;
        self.n = 0;
        self.f_flags = 0;
        self.pend = None;
    }

    /// A new stream from sample 0 at `fs` (started at `t0_us`): positions
    /// reset; a lock, the thresholds and the stored frames are kept unless
    /// the settings changed (then everything but the store restarts).
    pub fn restart(&mut self, cfg: Cfg, fs: u32, t0_us: u64, manual: Option<u16>) {
        self.end_frame();
        let same = cfg == self.cfg && fs == self.u.fs;
        self.cfg = cfg;
        let (bits, parity) = FMT[cfg.fmt as usize % 4];
        let u = &mut self.u;
        if !same {
            let keep = u.spb != 0 && cfg.baud == 0 && fs == u.fs;
            let (spb, baud) = if cfg.baud != 0 { ((fs as u64 * 256 / cfg.baud as u64) as u32, cfg.baud) } else if keep { (u.spb, u.baud) } else { (0, 0) };
            *u = Uart::new();
            u.spb = spb;
            u.baud = baud;
        }
        u.fs = fs;
        u.bits = bits;
        u.parity = parity;
        u.inv = cfg.inverted;
        u.tpos = 0;
        u.pos = 0;
        u.mpos = 0;
        u.medge = u64::MAX;
        u.lead = None;
        u.nrun = 0;
        u.in_char = false;
        // A start edge counts only after a mark level was seen.
        u.mark = false;
        u.mmark = true;
        u.set_manual(manual);
        self.t0_us = t0_us;
    }

    /// Manual threshold (code) or auto (None).
    pub fn set_threshold(&mut self, code: Option<u16>) {
        if code != self.u.manual {
            self.u.set_manual(code);
        }
    }

    /// Clear the frames and the counters (the lock stays).
    pub fn clear(&mut self) {
        self.st.clear();
        self.n = 0;
        self.f_flags = 0;
        self.pend = None;
    }

    /// The bit rate in use (0: not locked yet).
    pub fn baud(&self) -> u32 {
        if self.u.spb == 0 { 0 } else { self.u.baud }
    }

    /// Thresholds known: (mid, low, high) codes.
    pub fn threshold(&self) -> Option<(u16, u16, u16)> {
        self.u.thr_ok.then_some((self.u.mid, self.u.lo, self.u.hi))
    }

    pub fn fs(&self) -> u32 {
        self.u.fs
    }

    fn t_us(&self, n: u64) -> u64 {
        self.t0_us + n * 1_000_000 / self.u.fs.max(1) as u64
    }

    /// Samples lost (the main loop came too late): a break in the frame
    /// being collected; decoding resumes at the oldest valid sample.
    pub fn lost(&mut self) {
        self.st.cnt.ovr += 1;
        self.f_flags |= F_OVR;
        self.u.in_char = false;
    }

    /// Decode the samples written up to `wr` (absolute). `ring`: the DMA
    /// ring, sample n at index n % ring.len().
    pub fn poll(&mut self, ring: &[u16], wr: u64) {
        let len = ring.len() as u64;
        if wr < 2 {
            return;
        }
        let r = Ring { d: ring, wr, wi: (wr % len) as usize };
        let oldest = wr.saturating_sub(len - MARGIN);
        // 0. A lap: whatever is older than the ring is gone.
        if self.u.pos < oldest || (self.u.in_char && (self.u.t0 >> 8) < oldest) {
            if self.u.thr_ok && self.u.spb != 0 {
                self.lost();
            }
            self.u.pos = oldest;
            self.u.in_char = false;
            self.u.mark = false;
        }
        self.u.tpos = self.u.tpos.max(oldest);
        self.u.mpos = self.u.mpos.max(oldest);
        // 1. Threshold.
        if self.u.manual.is_none() {
            self.thr_scan(&r);
        }
        if !self.u.thr_ok {
            return;
        }
        // 2. Bit rate.
        if self.u.spb == 0 {
            if !self.measure(&r) {
                return;
            }
        }
        // 3. + 4.
        self.decode(&r);
        // A frame whose gap has passed.
        if self.n > 0 && wr > self.f_end + self.gap() {
            self.end_frame();
        }
    }

    fn thr_scan(&mut self, r: &Ring) {
        let u = &mut self.u;
        let mut n = u.tpos;
        let (mut lo, mut hi, mut c) = (u.wmin, u.wmax, u.wcnt);
        while n < r.wr {
            let v = r.at(n);
            lo = lo.min(v);
            hi = hi.max(v);
            n += 4;
            c += 4;
            if c >= WIN {
                if hi >= lo + MIN_SWING {
                    // Kept unless it moved by more than 1/16 of the swing
                    // (a steady level for the decoder and the display).
                    let s = hi - lo;
                    let mid = lo + s / 2;
                    if !u.thr_ok || mid.abs_diff(u.mid) > s / 16 {
                        u.mid = mid;
                        u.lo = mid - s / 6;
                        u.hi = mid + s / 6;
                        u.thr_ok = true;
                    }
                }
                lo = u16::MAX;
                hi = 0;
                c = 0;
            }
        }
        (u.tpos, u.wmin, u.wmax, u.wcnt) = (n, lo, hi, c);
    }

    /// Auto-baud: collect runs; true once locked (decoding may start).
    fn measure(&mut self, r: &Ring) -> bool {
        let u = &mut self.u;
        let mut n = u.mpos;
        let mut mark = u.mmark;
        if u.medge == u64::MAX {
            u.medge = n;
            u.mfirst = true;
        }
        while n < r.wr {
            let m = u.level(r.at(n), mark);
            if m != mark {
                mark = m;
                let run = (n - u.medge).min(u32::MAX as u64) as u32;
                if u.mfirst {
                    u.mfirst = false;
                    if !m {
                        u.lead = Some((run, n));
                    }
                } else {
                    u.runs[u.nrun as usize % NRUN] = (run, n, !m);
                    u.nrun += 1;
                }
                u.medge = n;
                if u.nrun >= RUN_LOCK && Self::try_lock(u) {
                    u.mmark = mark;
                    u.mpos = n + 1;
                    return true;
                }
            }
            n += 1;
        }
        u.mmark = mark;
        u.mpos = n;
        false
    }

    fn try_lock(u: &mut Uart) -> bool {
        let k = (u.nrun as usize).min(NRUN);
        let min = u.runs[..k].iter().map(|r| r.0).filter(|&r| r >= RUN_MIN).min().unwrap_or(0);
        if min == 0 {
            return false;
        }
        let (mut sum, mut cnt) = (0u64, 0u64);
        for r in &u.runs[..k] {
            if r.0 >= RUN_MIN && r.0 * 10 <= min * 13 {
                sum += r.0 as u64;
                cnt += 1;
            }
        }
        if cnt < 3 {
            return false;
        }
        // Bit time (Q8 samples) and the rate, snapped to a standard one.
        let spb = (sum * 256 / cnt) as u32;
        let baud = (u.fs as u64 * 256 / spb.max(1) as u64) as u32;
        if !(MIN_BAUD * 9 / 10..=MAX_BAUD * 11 / 10).contains(&baud) {
            return false;
        }
        let snap = STD_BAUD.iter().copied().find(|&s| baud.abs_diff(s) * 100 <= s * 8);
        (u.spb, u.baud) = match snap {
            Some(s) => ((u.fs as u64 * 256 / s as u64) as u32, s),
            None => (spb, baud),
        };
        // Start at the oldest kept run's start that follows an idle gap of a
        // character (mark run), else at the oldest run.
        let ch = u.char_samples();
        let first = u.nrun as usize - k;
        let mut start = u.lead.and_then(|(len, end)| (len as u64 >= ch).then_some(end));
        for j in first..u.nrun as usize {
            if start.is_some() {
                break;
            }
            let (len, end, space_after) = u.runs[j % NRUN];
            if space_after && len as u64 >= ch {
                start = Some(end);
                break;
            }
        }
        // No idle gap: from the oldest run, after the next mark level.
        u.mark = start.is_some();
        let start = start.unwrap_or_else(|| {
            let (len, end, _) = u.runs[first % NRUN];
            end - len as u64
        });
        u.pos = start.max(u.pos);
        u.in_char = false;
        u.fe_hist = 0;
        true
    }

    fn gap(&self) -> u64 {
        let ch = self.u.char_samples();
        if self.cfg.modbus {
            if self.baud() > 19200 { self.u.fs as u64 * 1750 / 1_000_000 } else { ch * 7 / 2 }
        } else {
            ch * 2
        }
    }

    fn decode(&mut self, r: &Ring) {
        loop {
            if !self.u.in_char {
                // Idle: look for a start edge.
                let u = &mut self.u;
                let mut n = u.pos;
                let mut i = r.idx(n);
                let (lo, hi, inv) = (u.lo, u.hi, u.inv);
                let mut mark = u.mark;
                let mut found = false;
                while n < r.wr {
                    let v = r.d[i] & 0x3FF;
                    let high = v > hi;
                    let low = v < lo;
                    if mark {
                        if (inv && high) || (!inv && low) {
                            found = true;
                            break;
                        }
                    } else if (inv && low) || (!inv && high) {
                        mark = true;
                    }
                    n += 1;
                    i += 1;
                    if i == r.d.len() {
                        i = 0;
                    }
                }
                u.mark = mark;
                u.pos = n;
                if !found {
                    return;
                }
                // The edge lies between n - 1 and n.
                u.in_char = true;
                u.t0 = (n << 8).saturating_sub(128);
                u.k = 0;
                u.acc = 0;
                u.ones = 0;
                u.err = 0;
                // A frame gap before this start?
                if self.n > 0 && n > self.f_end + self.gap() {
                    self.end_frame();
                }
            }
            // In a character: the next bit centres as far as written.
            let u = &mut self.u;
            let nbits = 1 + u.bits + (u.parity != 0) as u8 + 1;
            while u.k < nbits {
                let c = (u.t0 + ((2 * u.k as u64 + 1) * u.spb as u64) / 2 + 128) >> 8;
                if c + 2 > r.wr {
                    return;
                }
                let mid = u.mid;
                let hv = |v: u16| v >= mid;
                let ones = hv(r.at(c - 1)) as u8 + hv(r.at(c)) as u8 + hv(r.at(c + 1)) as u8;
                let bit = (ones >= 2) != u.inv;
                let k = u.k;
                u.k += 1;
                if k == 0 {
                    if bit {
                        // A glitch, no start bit: back to idle.
                        u.in_char = false;
                        u.mark = true;
                        u.pos = c + 1;
                        break;
                    }
                } else if k <= u.bits {
                    u.acc |= (bit as u16) << (k - 1);
                    u.ones += bit as u8;
                } else if k == u.bits + 1 && u.parity != 0 {
                    let odd = (u.ones + bit as u8) & 1 == 1;
                    if odd != (u.parity == 2) {
                        u.err |= F_PE;
                    }
                } else {
                    // Stop bit.
                    if !bit {
                        u.err |= F_FE;
                    }
                    u.fe_hist = u.fe_hist << 1 | !bit as u16;
                    u.in_char = false;
                    u.mark = bit;
                    u.pos = c + 1;
                    let ch = Ch { t: u.t0 >> 8, end: c + (u.spb as u64 >> 9), v: u.acc, err: u.err };
                    self.char_done(ch);
                    if self.cfg.baud == 0 && self.u.fe_hist.count_ones() >= 8 {
                        // Wrong rate (or a different one now): measure again.
                        self.u.unlock();
                        self.end_frame();
                        return;
                    }
                    break;
                }
            }
        }
    }

    fn char_done(&mut self, c: Ch) {
        let st = &mut self.st;
        st.cnt.bytes += 1;
        if c.err & F_FE != 0 {
            st.cnt.fe += 1;
        }
        if c.err & F_PE != 0 {
            st.cnt.pe += 1;
        }
        let max = if self.cfg.modbus { MB_MAX } else { UART_MAX };
        if self.n >= max {
            if self.cfg.modbus {
                self.f_flags |= F_LONG;
                self.f_end = c.end;
                return;
            }
            self.end_frame();
        }
        if self.n == 0 {
            self.f_t = c.t;
        }
        self.buf[self.n] = c.v as u8;
        self.n += 1;
        self.f_end = c.end;
        self.f_flags |= c.err;
    }

    /// Store the collected frame (Modbus: CRC, kind, pairing).
    fn end_frame(&mut self) {
        let n = self.n;
        self.n = 0;
        let flags = self.f_flags;
        self.f_flags = 0;
        if n == 0 {
            return;
        }
        let mut h = Hdr { t_us: self.t_us(self.f_t), off: 0, len: 0, flags, kind: K_UART, req: 0, lat_us: 0 };
        if self.cfg.modbus {
            let d = &self.buf[..n];
            let ok = n >= 4 && crc16(&d[..n - 2]) == (d[n - 2] as u16 | (d[n - 1] as u16) << 8);
            let (a, f, sh) = (d[0], d.get(1).copied().unwrap_or(0), shape(d));
            if !ok {
                h.flags |= F_CRC;
                self.st.cnt.crc += 1;
                h.kind = K_MB;
            } else {
                h.kind = classify(&mut self.pend, a, f, sh, h.t_us);
            }
            if h.kind == K_RSP || h.kind == K_EXC {
                if let Some((rs, _, _, rend)) = self.pend.take() {
                    h.req = rs + 1;
                    h.lat_us = h.t_us.saturating_sub(rend).min(u32::MAX as u64) as u32;
                }
            } else if h.kind == K_REQ {
                let end_us = self.t_us(self.f_end);
                self.pend = (a != 0).then_some((self.st.next, a, f, end_us));
            }
        }
        self.st.push(h, &self.buf[..n]);
    }
}

/// Request, reply or exception (CRC good): the shape first, the pending
/// request (same address and function, the reply within 1 s) for the
/// ambiguous ones. A stale pending request is dropped.
fn classify(pend: &mut Option<(u32, u8, u8, u64)>, a: u8, f: u8, (rq, rs): (bool, bool), t: u64) -> u8 {
    if pend.is_some_and(|(_, _, _, end)| t < end || t - end >= 1_000_000) {
        *pend = None;
    }
    let matches = pend.is_some_and(|(_, pa, pf, _)| pa == a && pf == f & 0x7F);
    if f & 0x80 != 0 && rs {
        return K_EXC;
    }
    match (rq, rs) {
        (true, false) => K_REQ,
        (false, true) => K_RSP,
        (true, true) => {
            if matches { K_RSP } else { K_REQ }
        }
        _ => {
            if matches { K_RSP } else { K_MB }
        }
    }
}

// ---- text (the list rows and DECode:FRAMes?) ----

/// A small text writer over a byte slice (cut at its end).
pub struct W<'a> {
    pub b: &'a mut [u8],
    pub n: usize,
}

impl W<'_> {
    pub fn s(&mut self, s: &str) -> &mut Self {
        for &c in s.as_bytes() {
            self.c(c);
        }
        self
    }
    pub fn c(&mut self, c: u8) -> &mut Self {
        if self.n < self.b.len() {
            self.b[self.n] = c;
            self.n += 1;
        }
        self
    }
    pub fn hex(&mut self, v: u8) -> &mut Self {
        const H: &[u8; 16] = b"0123456789ABCDEF";
        self.c(H[(v >> 4) as usize]).c(H[(v & 15) as usize])
    }
    pub fn hex16(&mut self, v: u16) -> &mut Self {
        self.hex((v >> 8) as u8).hex(v as u8)
    }
    pub fn u(&mut self, v: u32) -> &mut Self {
        let mut t = [0u8; 10];
        let mut i = t.len();
        let mut x = v;
        loop {
            i -= 1;
            t[i] = b'0' + (x % 10) as u8;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        for k in i..t.len() {
            self.c(t[k]);
        }
        self
    }
    /// `us` as seconds with `dec` decimals (0..6).
    pub fn secs(&mut self, us: u64, dec: u32) -> &mut Self {
        let s = us / 1_000_000;
        self.u(s as u32);
        if dec > 0 {
            self.c(b'.');
            let mut f = us % 1_000_000;
            let mut d = 100_000;
            for _ in 0..dec.min(6) {
                self.c(b'0' + (f / d) as u8);
                f %= d;
                d /= 10;
            }
        }
        self
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}

/// Status word of a frame: OK, OVR, FE, PE or CRC (the worst first).
pub fn status(h: &Hdr) -> &'static str {
    if h.flags & F_OVR != 0 {
        "OVR"
    } else if h.flags & F_FE != 0 {
        "FE"
    } else if h.flags & F_PE != 0 {
        "PE"
    } else if h.flags & F_CRC != 0 {
        "CRC"
    } else {
        "OK"
    }
}

/// Modbus fields as text: "REQ 01 03 @0000 n=10", "RSP 01 03 20B",
/// "EXC 01 83 E02", "RSP 01 06 @0005 =1234"; "?" for a frame without them.
pub fn mb_text(w: &mut W, d: &[u8], h: &Hdr) {
    if h.kind == K_MB || h.kind == K_UART || d.len() < 2 {
        w.s("?");
        return;
    }
    let m = mb_info(d, h.kind);
    w.s(match h.kind {
        K_REQ => "REQ ",
        K_RSP => "RSP ",
        _ => "EXC ",
    });
    w.hex(m.addr).c(b' ').hex(m.func);
    match h.kind {
        K_EXC => {
            w.s(" E").hex(m.exc);
        }
        K_RSP if (1..=4).contains(&m.func) => {
            w.c(b' ').u(m.n as u32).c(b'B');
        }
        _ => {
            w.s(" @").hex16(m.reg);
            if matches!(m.func, 5 | 6) {
                w.s(" =").hex16(m.n);
            } else {
                w.s(" n=").u(m.n as u32);
            }
        }
    }
}

/// One DECode:FRAMes? line: "seq,time_s,proto,status,hex,info\n" (info:
/// Modbus fields, the paired request and the reply time).
pub fn scpi_line(w: &mut W, st: &Store, seq: u32) {
    let Some(h) = st.get(seq) else { return };
    let h = *h;
    let mb = h.kind != K_UART;
    w.u(seq).c(b',').secs(h.t_us, 6).c(b',').s(if mb { "MB" } else { "UART" }).c(b',').s(status(&h)).c(b',');
    for i in 0..h.len as usize {
        w.hex(st.byte(&h, i));
    }
    w.c(b',');
    if mb {
        let mut d = [0u8; 8];
        let n = st.copy(&h, &mut d);
        mb_text(w, &d[..n], &h);
        if h.req != 0 {
            w.s(" req=").u(h.req - 1).s(" dt=").u(h.lat_us).s("us");
        }
    }
    w.c(b'\n');
}
