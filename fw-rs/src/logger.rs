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
//!
//! AD2 source (FR-38 (B), LOG:SOURce AD2; docs/dmm-input-scope-feasibility.md
//! sections 1, 2.3, 4): the HY3131's fast 19-bit converter in the stock AC
//! rows, at 600 .. 9600 S/s instead of the row's 76.8 kS/s (hy3131::
//! ad2_r26_for: SAD2CLK, OSR2, ENCHOPAD2). The row's mux is kept, so the
//! source is DC-coupled in AC mV (OP2O - AGND) and AC A / mA / uA (OP1O -
//! PB3, the shunt drop through OP1) and AC-coupled (~0.5 Hz) in AC V (PB4 fed
//! through a capacitor from OP1O); the DC-coupled V row of the feasibility
//! doc (1.3, last row) is NOT implemented (needs device test 6.5). Same
//! pacing as AD1 (INTF.AD2F, a P/2 gate), but since a bit-banged flag check
//! + 3-byte read cost ~122 us and the period is 104 .. 1667 us, one main-loop
//! pass may take several samples: `poll` keeps sampling for up to
//! `AD2_BURST_US` while the next result is due within that budget. Still no
//! ISR (owner decision pending): every main-loop stall longer than a period
//! drops samples, which the timestamps, `missed`, the span-based `lost()` and
//! the gap histogram (`gaps()`, LOG:GAPs?) report. A read whose bits 23:19
//! are not a copy of bit 18 (a torn read, feasibility 1.4) is dropped and
//! counted (`bad`); it shows up as a gap. Records: the same 12-byte wire
//! format; `raw` = signed AD2 code (+-262143 = +-1.25 V / IG at the AD2 input,
//! IG = 1.0 in every stock row; scaling on the host from LOG:PREamble?).

use crate::hy3131 as hy;
use crate::mmio::r32;

const DWT_CYCCNT: u32 = 0xE000_1004;
const CYC_PER_US: u64 = (crate::clock::HCLK_HZ / 1_000_000) as u64; // 200

/// Ring capacity in records. 2048 * 8 B = 16 KiB (`Rec`); ~51 s at the default
/// 40/s, ~3.2 s at AD1's fastest 640/s, 213 ms at AD2's 9600/s (the host must
/// drain LOG:DATA? at least ~5x/s there, else `overwritten`). Held in a
/// `static mut` (see main.rs) so it does not sit on the main stack next to
/// the USBTMC response buffer.
pub const CAP: usize = 2048;

/// Default AD1OSR code, applied at startup (main.rs). OSR 2048 chopped (R22 =
/// 0x13, 25 ms, 40/s) is the stock's fastest setting; faster ones are opt-in
/// (LOG:OSR, LOG:CHOP OFF; table above).
pub const DEFAULT_OSR: u8 = hy::AD1OSR_STREAM;

/// AD2 rate when LOG:SRATe was never set, S/s.
pub const DEFAULT_AD2_HZ: u32 = 2400;

/// Longest a single `poll` keeps taking AD2 samples, us (USB and the rest of
/// the loop wait that long at most).
pub const AD2_BURST_US: u64 = 500;

/// One sample in the ring: 8 bytes. On the wire 12, little-endian (seq,
/// ts_us, raw); the seq is not stored: the ring holds consecutive seqs (only
/// the oldest is ever dropped), so the one at `tail` is `seq - (len - 1)`.
#[derive(Clone, Copy)]
struct Rec {
    ts_us: u32,
    raw: i32,
}

const REC_BYTES: usize = 12;

/// Gap histogram buckets (in conversion periods between two kept samples):
/// 2, 3..4, 5..16, 17..128, > 128.
pub const GAP_BUCKETS: usize = 5;

pub struct Logger {
    active: bool,
    osr: u8,
    /// LOG:CHOP OFF. Stored inverted so that the all-zero `new()` (see
    /// `init_in_place`) is the chopped default.
    nochop: bool,
    /// LOG:SOURce AD2 (false = AD1, the all-zero default).
    ad2: bool,
    /// LOG:SRATe (AD2), S/s; 0 = DEFAULT_AD2_HZ.
    req_hz: u32,
    /// R26 low bits of the running AD2 session (preamble).
    r26: u8,
    nominal_us: u32,
    period_ns: u32,
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
    /// AD2 reads dropped by the sign-copy check (torn reads).
    bad: u32,
    gaps: [u32; GAP_BUCKETS],
    max_gap_us: u32,
    first_us: u64,
    last_us: u64,
    last_ts_us: u32,
    have_last: bool,
}

/// Gap bucket of a gap of `k` periods (k >= 2).
fn gap_bucket(k: u64) -> usize {
    match k {
        0..=2 => 0,
        3..=4 => 1,
        5..=16 => 2,
        17..=128 => 3,
        _ => 4,
    }
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
            ad2: false,
            req_hz: 0,
            r26: 0,
            nominal_us: 0,
            period_ns: 0,
            mode: 0,
            range: 0,
            last_cyc: 0,
            acc: 0,
            now_us: 0,
            last_sample_us: 0,
            buf: [Rec { ts_us: 0, raw: 0 }; CAP],
            head: 0,
            tail: 0,
            len: 0,
            seq: 0,
            captured: 0,
            overwritten: 0,
            missed: 0,
            bad: 0,
            gaps: [0; GAP_BUCKETS],
            max_gap_us: 0,
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
    /// AD1 chopper / AD2 ENCHOPAD2 (offset cancelled, half the rate; default
    /// on). Takes effect at the next `start`.
    pub fn set_chop(&mut self, on: bool) {
        self.nochop = !on;
    }
    pub fn chop(&self) -> bool {
        !self.nochop
    }
    /// LOG:SOURce: false = AD1 (DC rows), true = AD2 (AC rows). Takes effect
    /// at the next `start`.
    pub fn set_ad2(&mut self, on: bool) {
        self.ad2 = on;
    }
    pub fn ad2(&self) -> bool {
        self.ad2
    }
    /// The DMM modes a source can stream from: AD1 = DC V / mV / A (1, 3,
    /// 7..9); AD2 = the stock AC rows (0 AC V, AC-coupled; 2 AC mV and 4..6
    /// AC A / mA / uA, DC-coupled). AC+DC (16 / 17) alternates rows: neither.
    pub fn mode_ok(ad2: bool, mode: u8) -> bool {
        if ad2 { matches!(mode, 0 | 2 | 4..=6) } else { matches!(mode, 1 | 3 | 7..=9) }
    }
    /// LOG:SRATe: requested AD2 rate, S/s (the nearest rung of the ladder is
    /// used, hy3131::ad2_r26_for). Takes effect at the next `start`.
    pub fn set_req_hz(&mut self, hz: u32) {
        self.req_hz = hz;
    }
    pub fn req_hz(&self) -> u32 {
        if self.req_hz == 0 { DEFAULT_AD2_HZ } else { self.req_hz }
    }
    /// R26 low bits the next / running AD2 session uses.
    pub fn r26(&self) -> u8 {
        if self.active && self.ad2 { self.r26 } else { hy::ad2_r26_for(self.req_hz(), self.chop()) }
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
    /// Nominal (chip) rate of the session in mHz.
    pub fn nominal_mhz(&self) -> u32 {
        if self.period_ns == 0 { 0 } else { (1_000_000_000_000u64 / self.period_ns as u64) as u32 }
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
    pub fn bad(&self) -> u32 {
        self.bad
    }
    pub fn gaps(&self) -> [u32; GAP_BUCKETS] {
        self.gaps
    }
    pub fn max_gap_us(&self) -> u32 {
        self.max_gap_us
    }

    /// Conversions the session did not capture, from the span: the chip
    /// delivered round(span / period) + 1 results between the first and the
    /// last captured sample; `captured` of them were taken. Unlike `missed`
    /// (per gap, > 1.5 periods) it also counts the many slightly-late
    /// samples of a rate the loop cannot sustain (9600/s: a 104 us period vs
    /// ~122 us per bit-banged sample). Depends on the nominal period: a chip
    /// clock off by x % shows as x % lost (or as 0).
    pub fn lost(&self) -> u32 {
        if self.captured < 2 || self.period_ns == 0 {
            return 0;
        }
        let span_ns = (self.last_us - self.first_us) * 1000;
        let due = (span_ns + self.period_ns as u64 / 2) / self.period_ns as u64 + 1;
        due.saturating_sub(self.captured as u64).min(u32::MAX as u64) as u32
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

    /// Begin a session: configure the HY3131 (AD1: R22 OSR / chopper; AD2:
    /// R26 rung, AD3 / RMS / LPF off) and reset all state. `mode`/`range` are
    /// the current DMM function (for the preamble); the caller must have
    /// selected a mode `mode_ok` for the source, written its full row
    /// (dmm.start) and powered the analog side.
    pub fn start(&mut self, mode: u8, range: i8) {
        self.mode = mode;
        self.range = range;
        if self.ad2 {
            self.r26 = hy::ad2_r26_for(self.req_hz(), self.chop());
            self.period_ns = hy::ad2_period_ns(self.r26);
        } else {
            self.r26 = 0;
            self.period_ns = hy::ad1_period_us(self.osr, self.chop()) * 1000;
        }
        self.nominal_us = (self.period_ns + 500) / 1000;
        self.reset_stats();
        self.last_cyc = unsafe { r32(DWT_CYCCNT) };
        if self.ad2 {
            hy::set_ad2_stream(self.r26);
        } else {
            hy::set_ad1_osr(self.osr, self.chop());
        }
        self.active = true;
    }

    fn reset_stats(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.len = 0;
        self.seq = 0;
        self.captured = 0;
        self.overwritten = 0;
        self.missed = 0;
        self.bad = 0;
        self.gaps = [0; GAP_BUCKETS];
        self.max_gap_us = 0;
        self.have_last = false;
        self.acc = 0;
        self.now_us = 0;
        self.last_sample_us = 0;
        self.first_us = 0;
        self.last_us = 0;
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

    /// Main-loop entry: capture fresh conversions. Cheap when idle (one DWT
    /// read and an early return). The HY3131 AD1F / AD2F flag paces this at
    /// the conversion rate. For half a conversion period after the last
    /// captured sample no new conversion can be due, so INTF is not read
    /// then; after that it is read on every call until the next conversion
    /// is seen. AD1: at most one sample per call. AD2: keeps going while the
    /// next result is due within `AD2_BURST_US` of the call (see the header).
    /// The timestamp is taken when the flag was seen, before the data read.
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
        let end = self.now_us + if self.ad2 { AD2_BURST_US } else { 0 };
        let half = self.nominal_us as u64 / 2;
        loop {
            let open = if self.have_last { self.last_sample_us + half } else { 0 };
            if self.now_us < open {
                // Gate closed: wait here only if it opens within the burst.
                if open > end {
                    return;
                }
                while self.now_us < open {
                    self.tick();
                }
            }
            let ready = if self.ad2 { hy::ad2_ready() } else { hy::ad1_ready() };
            if ready {
                self.tick(); // the flag-observed moment
                self.last_sample_us = self.now_us;
                let ts = self.now_us as u32;
                if self.ad2 {
                    let (raw, ok) = hy::read_ad2();
                    if ok {
                        self.push(ts, raw);
                    } else {
                        self.bad = self.bad.wrapping_add(1);
                    }
                } else {
                    self.push(ts, hy::read_ad1());
                }
            }
            self.tick();
            if self.now_us >= end {
                return;
            }
        }
    }

    fn push(&mut self, ts_us: u32, raw: i32) {
        self.seq = self.seq.wrapping_add(1);
        self.captured = self.captured.wrapping_add(1);

        // Timestamp-gap estimate of samples the loop was too busy to take.
        if self.have_last {
            let gap = ts_us.wrapping_sub(self.last_ts_us) as u64;
            let pns = self.period_ns.max(1) as u64;
            let nom = self.nominal_us.max(1) as u64;
            if gap > nom * 3 / 2 {
                self.missed = self.missed.wrapping_add((gap / nom).saturating_sub(1) as u32);
            }
            let k = (gap * 1000 + pns / 2) / pns;
            if k >= 2 {
                self.gaps[gap_bucket(k)] = self.gaps[gap_bucket(k)].wrapping_add(1);
            }
            self.max_gap_us = self.max_gap_us.max(gap.min(u32::MAX as u64) as u32);
        } else {
            self.first_us = self.now_us;
            self.have_last = true;
        }
        self.last_ts_us = ts_us;
        self.last_us = self.now_us;

        self.buf[self.head] = Rec { ts_us, raw };
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
            // The ring holds consecutive seqs ending at `self.seq`.
            let seq = self.seq.wrapping_sub(self.len as u32 - 1);
            let r = self.buf[self.tail];
            self.tail = (self.tail + 1) % CAP;
            self.len -= 1;
            let o = i * REC_BYTES;
            out[o..o + 4].copy_from_slice(&seq.to_le_bytes());
            out[o + 4..o + 8].copy_from_slice(&r.ts_us.to_le_bytes());
            out[o + 8..o + 12].copy_from_slice(&r.raw.to_le_bytes());
        }
        n * REC_BYTES
    }

    /// Fill `out` with a USBTMC definite-length `#4nnnn` block of drained
    /// records and return the total byte count (header + payload). Mirrors the
    /// `Dbg::Lcd` framing in main.rs: `#` + one length-of-length digit (4) + a
    /// 4-digit payload byte count + the payload. Up to 511 records per block
    /// (6132 payload bytes) fit the 6144-byte USBTMC response buffer.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn session(ad2: bool, period_ns: u32) -> std::boxed::Box<Logger> {
        let mut l = std::boxed::Box::new(Logger::new());
        l.ad2 = ad2;
        l.period_ns = period_ns;
        l.nominal_us = (period_ns + 500) / 1000;
        l.active = true;
        l
    }

    fn feed(l: &mut Logger, ts: u64, raw: i32) {
        l.now_us = ts;
        l.push(ts as u32, raw);
    }

    fn recs(l: &mut Logger) -> std::vec::Vec<(u32, u32, i32)> {
        let mut b = std::vec![0u8; CAP * REC_BYTES];
        let n = l.drain(&mut b);
        b[..n]
            .chunks(12)
            .map(|c| {
                let w = |i: usize| u32::from_le_bytes([c[i], c[i + 1], c[i + 2], c[i + 3]]);
                (w(0), w(4), w(8) as i32)
            })
            .collect()
    }

    #[test]
    fn seq_is_implicit_and_consecutive_across_overflow() {
        let mut l = session(true, 104_167);
        for i in 0..(CAP as u64 + 10) {
            feed(&mut l, 1000 + i * 104, i as i32 - 5);
        }
        assert_eq!(l.overwritten(), 10);
        let r = recs(&mut l);
        assert_eq!(r.len(), CAP);
        assert_eq!(r[0].0, 11); // seqs 1..10 were dropped
        assert_eq!(r[0].2, 5);
        assert!(r.windows(2).all(|w| w[1].0 == w[0].0 + 1));
        assert_eq!(r.last().unwrap().0, CAP as u32 + 10);
        assert_eq!(l.buffered(), 0);
        // Partial drains keep the numbering.
        feed(&mut l, 1_000_000, -7);
        feed(&mut l, 1_000_104, -8);
        let mut b = [0u8; 12];
        assert_eq!(l.drain(&mut b), 12);
        assert_eq!(u32::from_le_bytes([b[0], b[1], b[2], b[3]]), CAP as u32 + 11);
        assert_eq!(recs(&mut l)[0].0, CAP as u32 + 12);
    }

    #[test]
    fn lost_and_gaps_from_the_timestamps() {
        // 9600/s: 104.167 us. Samples every 125 us (a loop that cannot keep
        // up): no single gap > 1.5 P, but 1 in 6 conversions is lost.
        let mut l = session(true, 104_167);
        for i in 0..6001u64 {
            feed(&mut l, i * 125, 0);
        }
        assert_eq!(l.missed(), 0);
        let due = (6000 * 125_000 + 52_083) / 104_167 + 1; // 7201
        assert_eq!(l.lost(), due as u32 - 6001);
        assert_eq!(l.measured_hz(), 8000);
        // One 10 ms stall: ~96 periods -> bucket 17..128.
        let mut l = session(true, 104_167);
        feed(&mut l, 0, 0);
        feed(&mut l, 104, 0);
        feed(&mut l, 10_104, 0);
        feed(&mut l, 10_208, 0);
        assert_eq!(l.gaps(), [0, 0, 0, 1, 0]);
        assert_eq!(l.max_gap_us(), 10_000);
        assert_eq!(l.missed(), 95);
        assert_eq!(l.lost(), 98 - 4 + 1); // 10208 us / 104.167 = 98 periods
        // A regular stream loses nothing.
        let mut l = session(false, 25_000_000);
        for i in 0..100u64 {
            feed(&mut l, i * 25_000, 0);
        }
        assert_eq!((l.lost(), l.missed(), l.gaps()), (0, 0, [0; GAP_BUCKETS]));
    }

    #[test]
    fn source_gates_the_modes() {
        assert!(Logger::mode_ok(false, 1) && Logger::mode_ok(false, 9) && !Logger::mode_ok(false, 2));
        assert!(Logger::mode_ok(true, 0) && Logger::mode_ok(true, 2) && Logger::mode_ok(true, 6));
        assert!(!Logger::mode_ok(true, 1) && !Logger::mode_ok(true, 16) && !Logger::mode_ok(true, 17));
        let mut l = Logger::new();
        assert_eq!(l.req_hz(), DEFAULT_AD2_HZ);
        l.set_req_hz(9600);
        assert_eq!(hy::ad2_rate_mhz(l.r26()), 9_600_000);
        l.set_chop(false);
        l.set_req_hz(600);
        assert_eq!(hy::ad2_rate_mhz(l.r26()), 1_200_000);
    }
}
