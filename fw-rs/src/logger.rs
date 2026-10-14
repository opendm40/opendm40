//! Data-logger fast-stream mode (FR-38).
//!
//! A main-loop-polled, conversion-paced sampler for the HY3131 AD1 (DC) channel.
//! fw-rs runs with interrupts masked (`cpsid i`), so there are NO ISRs: this is
//! deliberately a cooperative sampler driven from the main loop, timestamped and
//! paced by the DWT cycle counter (clock.rs enables CYCCNT; HCLK = 200 MHz), not
//! by an interrupt. FR-38 trades resolution for speed: the HY3131 AD1OSR is
//! dropped to its fast setting (see `hy3131::set_ad1_osr`) and we stream the raw
//! signed 24-bit AD1 counts.
//!
//! Rate ceiling: the HY3131 itself, not the SPI link (hy3131.rs "AD1
//! conversion timing"): period = OSR * 30 / 4.9152 MHz, x2 with the chopper.
//!   LOG:OSR  OSR     chopped (default)   LOG:CHOP OFF
//!   3        2048    25 ms    40/s        12.5 ms    80/s   (default code)
//!   2        1024    12.5 ms  80/s        6.25 ms   160/s
//!   1        512     6.25 ms 160/s        3.125 ms  320/s
//!   0        256     3.125 ms 320/s       1.5625 ms 640/s   (the chip's max)
//!   4..7     4096..32768: 50..400 ms chopped (20..2.5/s)
//! The 38 Hz measured on the device at the default is this 40/s (the old
//! "SPI-limited" diagnosis assumed OSR / SYSCLK, 417 us). Each sample costs
//! ~122 us of bit-banged SPI, so even 640/s keeps the link < 10 % busy; the
//! limit there is the main-loop period, which must stay below the conversion
//! period or samples are lost (`missed`; not measured on the device above
//! 40/s yet). Trade-offs: a lower OSR has fewer effective bits (more noise
//! per sample); LOG:CHOP OFF (AD1CHOP = 00) leaves the converter offset in
//! every sample (the calibration was taken chopped: zero the host's scale
//! with a shorted input at the same settings) and adds low-frequency noise.
//! Both settings apply to the logger only and take effect at LOG:STATe ON;
//! LOG:STATe OFF restores the DMM row (main.rs: `app.dmm.start` rewrites the
//! full configuration, R22 included). SI scaling is intentionally not applied here (the
//! calibration path lives in dmm.rs); `LOG:PREamble?` reports mode/range/OSR so
//! the host can scale, per FR-38's "accuracy sacrificed for speed".
//!
//! Each record carries a monotonic sequence number and a device microsecond
//! timestamp, so loss is always detectable, never silently skipped:
//!   * ring overflow (host draining `LOG:DATA?` too slowly) -> the oldest record
//!     is overwritten and `overwritten` counts it; the host sees a SEQUENCE gap.
//!   * a stalled main loop (a slow redraw, a relay pulse, an SPI-flash write)
//!     -> fewer samples captured than the conversion grid; the host sees a
//!     TIMESTAMP gap between consecutive seqs, and `missed` estimates it.
//! The timestamp is taken when AD1F was observed (0..one loop period after the
//! conversion latched); the underlying conversion grid itself is regular.
//! The wire timestamp is the low 32 bits of a device microsecond clock that
//! starts at 0 on `start`: it wraps every 2^32 us (~71.6 min), so the host must
//! unwrap it (add 2^32 whenever it steps backwards). `measured_hz` uses the
//! full 64-bit clock.
//!
//! USB-disk writes void the interval: a host write to the mass-storage disk
//! erases a 4 KiB SPI-flash sector (45..400 ms) inside the main loop, which
//! shows up as a timestamp gap / `missed`. The MSC should refuse writes while
//! `disk_writes_blocked()` (see that method); until it does, a log taken while
//! the host writes to the disk has gaps.
//!
//! Transport: the records are read over USBTMC (`LOG:DATA?`, priority per FR-38).
//! BT hook: `drain(buf)` is transport-agnostic, so bt.rs can later push the same
//! records to the phone app where the 115200-baud link has the bandwidth.

use crate::hy3131 as hy;
use crate::mmio::r32;

const DWT_CYCCNT: u32 = 0xE000_1004;
const CYC_PER_US: u64 = (crate::clock::HCLK_HZ / 1_000_000) as u64; // 200

/// Ring capacity in records. 2048 * 12 B = 24 KiB; ~51 s at the default
/// 40/s, ~3.2 s at the fastest 640/s (OSR 256 unchopped). Held
/// in a `static mut` (see main.rs) so it does not sit on the main stack next to
/// the ~12.5 KiB USBTMC buffers.
pub const CAP: usize = 2048;

/// Default AD1OSR code, applied at startup (main.rs). OSR 2048 chopped (R22 =
/// 0x13, 25 ms, 40/s) is the stock's fastest setting; faster ones are opt-in
/// (LOG:OSR, LOG:CHOP OFF; table above).
pub const DEFAULT_OSR: u8 = hy::AD1OSR_STREAM;

/// One sample: 12 bytes, little-endian on the wire (seq, ts_us, raw).
#[derive(Clone, Copy)]
struct Rec {
    seq: u32,
    ts_us: u32,
    raw: i32,
}

const REC_BYTES: usize = 12;

pub struct Logger {
    active: bool,
    osr: u8,
    /// LOG:CHOP OFF. Stored inverted so that the all-zero `new()` (see
    /// `init_in_place`) is the chopped default.
    nochop: bool,
    nominal_us: u32,
    mode: u8,
    range: i8,

    // DWT-derived monotonic microsecond clock (own wrap tracking; the Clock in
    // clock.rs keeps its accumulator private).
    last_cyc: u32,
    acc: u64,
    now_us: u64,
    /// `now_us` of the last captured sample (the pacing gate runs from it).
    last_sample_us: u64,

    // Ring buffer (oldest at `tail`, next write at `head`).
    buf: [Rec; CAP],
    head: usize,
    tail: usize,
    len: usize,

    // Monotonic stats over the session.
    seq: u32,
    captured: u32,
    overwritten: u32,
    missed: u32,
    first_us: u64,
    last_us: u64,
    last_ts_us: u32,
    have_last: bool,
}

impl Logger {
    /// `new()` in place for an instance in NOLOAD RAM (main.rs LOGGER is in
    /// .dmabuf, not zeroed at reset), without a ~24 KiB temporary on the
    /// stack: `new()` is all zero bits (false / 0 / zeroed records), and all
    /// zero bits is a valid Logger. Keep `new()` all-zero.
    pub fn init_in_place(this: *mut Logger) {
        unsafe { core::ptr::write_bytes(this, 0, 1) };
    }

    /// All-zero so the ~24 KiB instance lands in `.bss`, not `.data` (the RAM
    /// image has no room for 24 KiB of initialised data), and so that
    /// `init_in_place` equals it. Runtime defaults (OSR) are applied in
    /// main.rs via `set_osr(DEFAULT_OSR)`.
    pub const fn new() -> Self {
        Logger {
            active: false,
            osr: 0,
            nochop: false,
            nominal_us: 0,
            mode: 0,
            range: 0,
            last_cyc: 0,
            acc: 0,
            now_us: 0,
            last_sample_us: 0,
            buf: [Rec { seq: 0, ts_us: 0, raw: 0 }; CAP],
            head: 0,
            tail: 0,
            len: 0,
            seq: 0,
            captured: 0,
            overwritten: 0,
            missed: 0,
            first_us: 0,
            last_us: 0,
            last_ts_us: 0,
            have_last: false,
        }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// True while USB-disk (MSC) writes must be refused: each one erases a
    /// SPI-flash sector and stalls the main loop for 45..400 ms, i.e. hundreds
    /// of lost conversions. The MSC should answer WRITE with NOT READY
    /// (sense 02/04/00) while this holds; reads stay allowed.
    pub fn disk_writes_blocked(&self) -> bool {
        self.active
    }

    /// AD1OSR code (0..7). Takes effect at the next `start`.
    pub fn set_osr(&mut self, code: u8) {
        self.osr = code & 7;
    }

    pub fn osr(&self) -> u8 {
        self.osr
    }
    /// AD1 chopper (offset cancelled, half the rate; default on). Takes
    /// effect at the next `start`.
    pub fn set_chop(&mut self, on: bool) {
        self.nochop = !on;
    }
    pub fn chop(&self) -> bool {
        !self.nochop
    }
    pub fn mode(&self) -> u8 {
        self.mode
    }
    pub fn range(&self) -> i8 {
        self.range
    }
    pub fn nominal_us(&self) -> u32 {
        self.nominal_us
    }
    pub fn buffered(&self) -> u32 {
        self.len as u32
    }
    pub fn captured(&self) -> u32 {
        self.captured
    }
    pub fn overwritten(&self) -> u32 {
        self.overwritten
    }
    pub fn missed(&self) -> u32 {
        self.missed
    }

    /// Measured mean sample rate in Hz over the current session, from the first
    /// and last sample times (64-bit, no 71.6 min wrap) and the capture count
    /// (the real number, off the device, not the nominal rate). 0 until at
    /// least two samples.
    pub fn measured_hz(&self) -> u32 {
        if self.captured < 2 {
            return 0;
        }
        let span = self.last_us - self.first_us;
        if span == 0 {
            return 0;
        }
        (((self.captured as u64 - 1) * 1_000_000) / span) as u32
    }

    /// Begin a session: configure the HY3131 AD1OSR / chopper and reset all state.
    /// `mode`/`range` are the current DMM function (for the preamble); the caller
    /// must have selected an AD1 DC mode (1/3/7/8/9) and powered the analog side.
    pub fn start(&mut self, mode: u8, range: i8) {
        self.mode = mode;
        self.range = range;
        self.nominal_us = hy::ad1_period_us(self.osr, self.chop());
        self.head = 0;
        self.tail = 0;
        self.len = 0;
        self.seq = 0;
        self.captured = 0;
        self.overwritten = 0;
        self.missed = 0;
        self.have_last = false;
        self.acc = 0;
        self.now_us = 0;
        self.last_sample_us = 0;
        self.first_us = 0;
        self.last_us = 0;
        self.last_cyc = unsafe { r32(DWT_CYCCNT) };
        hy::set_ad1_osr(self.osr, self.chop());
        self.active = true;
    }

    pub fn stop(&mut self) {
        self.active = false;
        // The caller restores the HY3131 (app.dmm.start) so the DMM resumes with
        // a clean config; nothing to undo here.
    }

    /// Advance the microsecond clock from the DWT cycle counter. Safe to call
    /// often; handles the ~21 s CYCCNT wrap via wrapping_sub.
    fn tick(&mut self) {
        let c = unsafe { r32(DWT_CYCCNT) };
        self.acc += c.wrapping_sub(self.last_cyc) as u64;
        self.last_cyc = c;
        self.now_us += self.acc / CYC_PER_US;
        self.acc %= CYC_PER_US;
    }

    /// Main-loop entry: capture at most one fresh conversion. Cheap when idle
    /// (one DWT read and an early return). The HY3131 AD1F flag paces this at the
    /// conversion rate. For half a conversion period after the last captured
    /// sample no new conversion can be due, so INTF is not read then; after that
    /// it is read on every call until the next conversion is seen.
    ///
    /// The gate runs from the last SAMPLE, not the last INTF check: gated from
    /// the check, a miss pushed the next check another gate period out, so the
    /// observation drifted late and conversions got overwritten unread when the
    /// loop period L was a sizeable fraction of the period P (0.8 P gate,
    /// L = 0.7 P: checks 1.4 P apart). With a P/2 gate from the sample, the
    /// first check after it comes ceil(P/2 / L) * L < P after the sample for a
    /// steady L < P, so the delay between a conversion and its observation
    /// stays under L and no conversion is lost while the loop is faster than
    /// the conversion rate (a gate of 0.8 P would not guarantee that).
    pub fn poll(&mut self) {
        if !self.active {
            return;
        }
        self.tick();
        if self.have_last && self.now_us - self.last_sample_us < self.nominal_us as u64 / 2 {
            return;
        }
        if !hy::ad1_ready() {
            return;
        }
        let raw = hy::read_ad1();
        self.tick(); // stamp after the SPI read (AD1F-observed moment)
        self.last_sample_us = self.now_us;
        let ts = self.now_us as u32;
        self.push(ts, raw);
    }

    fn push(&mut self, ts_us: u32, raw: i32) {
        self.seq = self.seq.wrapping_add(1);
        self.captured = self.captured.wrapping_add(1);

        // Timestamp-gap estimate of samples the loop was too busy to take.
        if self.have_last {
            let gap = ts_us.wrapping_sub(self.last_ts_us) as u64;
            let nom = self.nominal_us.max(1) as u64;
            if gap > nom * 3 / 2 {
                self.missed = self.missed.wrapping_add((gap / nom).saturating_sub(1) as u32);
            }
        } else {
            self.first_us = self.now_us;
            self.have_last = true;
        }
        self.last_ts_us = ts_us;
        self.last_us = self.now_us;

        self.buf[self.head] = Rec { seq: self.seq, ts_us, raw };
        self.head = (self.head + 1) % CAP;
        if self.len == CAP {
            // Overflow: drop the oldest. The host sees a sequence jump.
            self.tail = (self.tail + 1) % CAP;
            self.overwritten = self.overwritten.wrapping_add(1);
        } else {
            self.len += 1;
        }
    }

    /// Pop oldest-first records into `out` (raw 12-byte LE records, no framing).
    /// Returns the number of bytes written. Transport-agnostic: USBTMC today,
    /// BT later.
    pub fn drain(&mut self, out: &mut [u8]) -> usize {
        let room = out.len() / REC_BYTES;
        let n = self.len.min(room);
        for i in 0..n {
            let r = self.buf[self.tail];
            self.tail = (self.tail + 1) % CAP;
            self.len -= 1;
            let o = i * REC_BYTES;
            out[o..o + 4].copy_from_slice(&r.seq.to_le_bytes());
            out[o + 4..o + 8].copy_from_slice(&r.ts_us.to_le_bytes());
            out[o + 8..o + 12].copy_from_slice(&r.raw.to_le_bytes());
        }
        n * REC_BYTES
    }

    /// Fill `out` with a USBTMC definite-length `#4nnnn` block of drained
    /// records and return the total byte count (header + payload). Mirrors the
    /// `Dbg::Lcd` framing in main.rs: `#` + one length-of-length digit (4) + a
    /// 4-digit payload byte count + the payload. Up to 511 records per block
    /// (6138 payload bytes) fit the 6144-byte USBTMC response buffer.
    pub fn encode(&mut self, out: &mut [u8]) -> usize {
        const HDR: usize = 6; // "#4nnnn"
        let maxrec = out.len().saturating_sub(HDR) / REC_BYTES;
        let nrec = self.len.min(maxrec).min(9999 / REC_BYTES);
        let bytes = nrec * REC_BYTES;
        out[0] = b'#';
        out[1] = b'4';
        for (i, d) in [1000usize, 100, 10, 1].iter().enumerate() {
            out[2 + i] = b'0' + ((bytes / d) % 10) as u8;
        }
        self.drain(&mut out[HDR..HDR + bytes]);
        HDR + bytes
    }
}
