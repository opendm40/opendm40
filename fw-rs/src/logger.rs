//! Data-logger fast-stream mode (FR-38).
//!
//! A main-loop-polled, conversion-paced sampler for the HY3131 AD1 (DC) channel.
//! fw-rs runs with interrupts masked (`cpsid i`): this is a cooperative sampler
//! driven from the main loop, timestamped and paced by the DWT cycle counter
//! (clock.rs enables CYCCNT; HCLK = 200 MHz). The exceptions, with the
//! `ad2-isr` feature (module `isr` below, decisions-2026-10-22 #3): the AD2
//! source, and AD1 unchopped at 160 .. 640 S/s (`ad1_on_isr`, 2026-10-22: the
//! main-loop sampler lost 1.8 % at 640 S/s to long passes on the device):
//! a timer ISR is then the producer and the main loop only drains. FR-38 trades resolution for speed: the HY3131 AD1OSR is
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
//!   * ring overflow (host draining `LOG:DATA?` too slowly) -> the NEWEST sample
//!     is dropped (single-producer / single-consumer ring: the producer, which
//!     may be the stream ISR, never moves the consumer's tail) and `overwritten`
//!     counts it; the seqs stay consecutive, the host sees a TIMESTAMP gap.
//!     (Before the SPSC ring, 2026-10-22, the oldest record was overwritten and
//!     the host saw a sequence gap; dm40scpi.py's lost_before handles both.)
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
//! rows, at 1200 .. 9600 S/s instead of the row's 76.8 kS/s (hy3131::
//! ad2_r26_for: SAD2CLK, OSR2, ENCHOPAD2). The row's mux is kept, so the
//! source is DC-coupled in AC mV (OP2O - AGND) and AC A / mA / uA (OP1O -
//! PB3, the shunt drop through OP1) and AC-coupled (~0.5 Hz) in AC V (PB4 fed
//! through a capacitor from OP1O); the DC-coupled V row of the feasibility
//! doc (1.3, last row) is NOT implemented (needs device test 6.5). Same
//! pacing as AD1 (INTF.AD2F, a P/2 gate), but since a bit-banged flag check
//! + 3-byte read cost ~122 us and the period is 104 .. 1667 us, one main-loop
//! pass may take several samples: `poll` keeps sampling for up to
//! `AD2_BURST_US` while the next result is due within that budget. Without the
//! `ad2-isr` feature every main-loop stall longer than a period drops samples
//! (device 7889e6c, SPI2: 8 % lost at 4800, 11 % at 9600), which the
//! timestamps, `missed`, the span-based `lost()` and the gap histogram
//! (`gaps()`, LOG:GAPs?) report. With it, the timer ISR (`isr`) takes every
//! sample while the main loop redraws; the same counters report what it
//! still misses, DBG:ISR? its own numbers. A read whose bits 23:19
//! are not a copy of bit 18 (a torn read, feasibility 1.4) is dropped and
//! counted (`bad`); it shows up as a gap. Records: the same 12-byte wire
//! format; `raw` = signed AD2 code (+-262143 = +-1.25 V / IG at the AD2 input,
//! IG = 1.0 in every stock row; scaling on the host from LOG:PREamble?).

use crate::hy3131 as hy;
use crate::mmio::r32;
#[cfg_attr(not(feature = "ad2-isr"), allow(unused_imports))]
use crate::trigsoft::{Stream, StreamCfg};
use core::cell::UnsafeCell;
use core::sync::atomic::{
    AtomicU32,
    Ordering::{Acquire, Relaxed, Release},
};

const DWT_CYCCNT: u32 = 0xE000_1004;
const CYC_PER_US: u32 = crate::clock::HCLK_HZ / 1_000_000; // 200

/// Ring capacity in records (a power of two). 2048 * 8 B = 16 KiB (`Ring`);
/// ~51 s at the default 40/s, ~3.2 s at AD1's fastest 640/s, 427 ms at
/// AD2's 4800/s, 213 ms at 9600/s (the host must drain LOG:DATA? at least
/// ~5x/s there, else `overwritten`). A static in SRAM (`RING`), not on the
/// main stack and not inside `Logger` (the stream ISR writes it).
pub const CAP: usize = 2048;
const _: () = assert!(CAP.is_power_of_two());

/// Default AD1OSR code, applied at startup (main.rs). OSR 2048 chopped (R22 =
/// 0x13, 25 ms, 40/s) is the stock's fastest setting; faster ones are opt-in
/// (LOG:OSR, LOG:CHOP OFF; table above).
pub const DEFAULT_OSR: u8 = hy::AD1OSR_STREAM;

/// AD1 sessions with a conversion period up to this (us) run on the stream
/// ISR (`Logger::ad1_on_isr`), unchopped: OSR 256 / 512 / 1024 (640 / 320
/// / 160 S/s).
pub const AD1_ISR_MAX_US: u32 = 6250;

/// Main-loop AD1 sampler: blind reads in a row after a swallowed AD1F.
const RESCUE_MAX: u8 = 4;

/// AD2 rate when LOG:SRATe was never set, S/s: 4800 with the stream ISR
/// (decisions #3: ~10 % of the CPU at 3.125 MHz), 2400 for the main-loop
/// sampler.
pub const DEFAULT_AD2_HZ: u32 = if cfg!(feature = "ad2-isr") { 4800 } else { 2400 };

/// Longest a single `poll` keeps taking AD2 samples, us (USB and the rest of
/// the loop wait that long at most). Main-loop sampler only.
pub const AD2_BURST_US: u64 = 500;

/// One sample in the ring: 8 bytes. On the wire 12, little-endian (seq,
/// ts_us, raw); the seq is not stored: record number k (from 0) of the
/// session is seq k + 1, and the ring holds consecutive ones (a full ring
/// drops the NEWEST sample, so nothing ever leaves except by `drain`).
#[derive(Clone, Copy)]
struct Rec {
    ts_us: u32,
    raw: i32,
}

const REC_BYTES: usize = 12;

/// Gap histogram buckets (in conversion periods between two kept samples):
/// 2, 3..4, 5..16, 17..128, > 128.
pub const GAP_BUCKETS: usize = 5;

/// The single-producer / single-consumer ring and the producer's counters:
/// the only state shared with the stream ISR (decisions #3, invariant 4).
/// Producer = the stream ISR (AD2 with `ad2-isr`) or the main loop (AD1,
/// main-loop AD2), never both in one session; consumer = the main loop
/// (`drain`). Each atomic has one writer: `head` and every counter the
/// producer, `tail` the consumer; Release on `head` / Acquire on the other
/// side order the record slots. All-zero = empty (`new`, `reset`).
pub struct Ring {
    buf: UnsafeCell<[Rec; CAP]>,
    /// Records pushed this session (slot = head % CAP).
    head: AtomicU32,
    /// Records drained this session.
    tail: AtomicU32,
    captured: AtomicU32,
    /// Captured but dropped: the ring was full (LOG:COUNt? `overwritten`).
    dropped: AtomicU32,
    missed: AtomicU32,
    bad: AtomicU32,
    gaps: [AtomicU32; GAP_BUCKETS],
    max_gap_us: AtomicU32,
    /// First -> last captured sample, us, 64 bit as two words (main reads
    /// hi, lo, hi and retries on a change: the ISR may update between).
    span_lo: AtomicU32,
    span_hi: AtomicU32,
    // Stream ISR only (DBG:ISR?).
    /// Entries (A and B).
    ticks: AtomicU32,
    /// A entries too late for their data frame (skipped, a stamp gap).
    late: AtomicU32,
    /// Probes completed (B + the INTF read of the next A).
    probes: AtomicU32,
    overruns: AtomicU32,
    timeouts: AtomicU32,
    max_isr_cyc: AtomicU32,
    max_lat: AtomicU32,
    // The DMM wave view's trigger (`wave`, the stream ISR's `trig`): the
    // configuration words (main loop; `trg_gen` last, Release), the event
    // ring (ISR: `trg_ev` slots, then `trg_ev_n`, Release) and the first
    // event at or after the configured minimum index (ISR).
    trg_gen: AtomicU32,
    trg_w: [AtomicU32; 6],
    trg_ev: [AtomicU32; TRG_EV],
    trg_ev_n: AtomicU32,
    trg_first_at: AtomicU32,
    trg_first_gen: AtomicU32,
}

/// Trigger events the ISR keeps (the newest TRG_EV - 1 are safe to read
/// while it runs, `wave::events`).
const TRG_EV: usize = 8;

// SAFETY: the slots are written only by the producer at `head` and read
// only by the consumer below `head` (Acquire); every other field is atomic.
unsafe impl Sync for Ring {}

const Z: AtomicU32 = AtomicU32::new(0);

impl Ring {
    pub const fn new() -> Self {
        Ring {
            buf: UnsafeCell::new([Rec { ts_us: 0, raw: 0 }; CAP]),
            head: Z,
            tail: Z,
            captured: Z,
            dropped: Z,
            missed: Z,
            bad: Z,
            gaps: [Z; GAP_BUCKETS],
            max_gap_us: Z,
            span_lo: Z,
            span_hi: Z,
            ticks: Z,
            late: Z,
            probes: Z,
            overruns: Z,
            timeouts: Z,
            max_isr_cyc: Z,
            max_lat: Z,
            trg_gen: Z,
            trg_w: [Z; 6],
            trg_ev: [Z; TRG_EV],
            trg_ev_n: Z,
            trg_first_at: Z,
            trg_first_gen: Z,
        }
    }

    /// New session (main loop, no producer running).
    fn reset(&self) {
        for a in [
            &self.head, &self.tail, &self.captured, &self.dropped, &self.missed, &self.bad, &self.max_gap_us,
            &self.span_lo, &self.span_hi, &self.ticks, &self.late, &self.probes, &self.overruns, &self.timeouts,
            &self.max_isr_cyc, &self.max_lat, &self.trg_gen, &self.trg_ev_n, &self.trg_first_at, &self.trg_first_gen,
        ] {
            a.store(0, Relaxed);
        }
        for g in &self.gaps {
            g.store(0, Relaxed);
        }
    }

    fn len(&self) -> u32 {
        self.head.load(Acquire).wrapping_sub(self.tail.load(Relaxed))
    }

    fn span_us(&self) -> u64 {
        loop {
            let h = self.span_hi.load(Relaxed);
            let l = self.span_lo.load(Relaxed);
            if self.span_hi.load(Relaxed) == h {
                return (h as u64) << 32 | l as u64;
            }
        }
    }

    /// Producer: append, or drop the sample when full (newest dropped, so
    /// the consumer's `tail` is never written here). True when stored.
    fn push(&self, ts_us: u32, raw: i32) -> bool {
        let h = self.head.load(Relaxed);
        if h.wrapping_sub(self.tail.load(Acquire)) as usize >= CAP {
            inc(&self.dropped);
            return false;
        }
        // SAFETY: slot h is outside [tail, head): the consumer does not read
        // it until the Release store below publishes it.
        unsafe { (self.buf.get() as *mut Rec).add(h as usize & (CAP - 1)).write_volatile(Rec { ts_us, raw }) };
        self.head.store(h.wrapping_add(1), Release);
        true
    }
}

/// `a += 1` for a counter with a single writer (no RMW needed).
#[inline(always)]
fn inc(a: &AtomicU32) {
    a.store(a.load(Relaxed).wrapping_add(1), Relaxed);
}

/// The ring of the firmware (SRAM .dmabuf, NOLOAD: `Logger::init_in_place`
/// resets it; the slots' contents do not matter).
#[cfg_attr(target_os = "none", link_section = ".dmabuf")]
pub static RING: Ring = Ring::new();

/// The producer side's clock and per-sample statistics: the main loop's in
/// `Logger` (AD1, main-loop AD2), the stream ISR's own copy in `isr::PROD`
/// (set up by the main loop before the hand-off, the ISR's alone after it).
#[derive(Clone, Copy)]
struct Producer {
    period_ns: u32,
    nominal_us: u32,
    // DWT-derived monotonic microsecond clock from the session start (own
    // wrap tracking; CYCCNT wraps every 21.5 s).
    last_cyc: u32,
    acc: u32,
    now_us: u64,
    first_us: u64,
    last_ts_us: u32,
    have_last: bool,
    /// `now_us` when the last conversion was seen (the gate runs from it;
    /// bad reads included).
    last_sample_us: u64,
    /// No flag read within this many us of `last_sample_us`: P/2 (the
    /// main-loop sampler; the stream ISR does not poll the flag).
    gate_us: u32,
    /// Blind AD1 reads after a swallowed flag in a row (`sample`: at most
    /// RESCUE_MAX).
    rescued: u8,
    /// AD1: the last flag-to-flag interval (us, 0 = none yet): the chip's
    /// real period (device 5394b21: 26.3 ms at the 25 ms OSR 2048 chopped
    /// setting), for a rescued sample's stamp and window.
    p_meas: u32,
}

impl Producer {
    const fn new() -> Self {
        Producer {
            period_ns: 0,
            nominal_us: 0,
            last_cyc: 0,
            acc: 0,
            now_us: 0,
            first_us: 0,
            last_ts_us: 0,
            have_last: false,
            last_sample_us: 0,
            gate_us: 0,
            rescued: 0,
            p_meas: 0,
        }
    }

    /// New session; gate = P/2 (the main-loop sampler).
    fn begin(&mut self, period_ns: u32) {
        *self = Producer::new();
        self.period_ns = period_ns;
        self.nominal_us = (period_ns + 500) / 1000;
        self.gate_us = self.nominal_us / 2;
        self.last_cyc = unsafe { r32(DWT_CYCCNT) };
    }

    /// Advance the microsecond clock to cycle count `c` (u32 maths only:
    /// called at least every few ms, far below the 21 s CYCCNT wrap).
    #[inline(always)]
    fn tick_at(&mut self, c: u32) {
        let d = c.wrapping_sub(self.last_cyc);
        self.last_cyc = c;
        let r = d % CYC_PER_US + self.acc;
        self.now_us += (d / CYC_PER_US + r / CYC_PER_US) as u64;
        self.acc = r % CYC_PER_US;
    }

    fn tick(&mut self) {
        self.tick_at(unsafe { r32(DWT_CYCCNT) });
    }

    /// One captured sample at `now_us` (wire stamp `ts_us`): statistics,
    /// then the ring. True when the ring took it (not full).
    fn push(&mut self, ring: &Ring, ts_us: u32, raw: i32) -> bool {
        inc(&ring.captured);
        // Timestamp-gap estimate of samples the producer was too late for.
        if self.have_last {
            let gap = ts_us.wrapping_sub(self.last_ts_us);
            let nom = self.nominal_us.max(1);
            if gap > nom * 3 / 2 {
                ring.missed.store(ring.missed.load(Relaxed).wrapping_add((gap / nom).saturating_sub(1)), Relaxed);
            }
            let pns = self.period_ns.max(1) as u64;
            let k = (gap as u64 * 1000 + pns / 2) / pns;
            if k >= 2 {
                inc(&ring.gaps[gap_bucket(k)]);
            }
            if gap > ring.max_gap_us.load(Relaxed) {
                ring.max_gap_us.store(gap, Relaxed);
            }
        } else {
            self.first_us = self.now_us;
            self.have_last = true;
        }
        self.last_ts_us = ts_us;
        let span = self.now_us - self.first_us;
        ring.span_hi.store((span >> 32) as u32, Relaxed);
        ring.span_lo.store(span as u32, Relaxed);
        ring.push(ts_us, raw)
    }
}

pub struct Logger {
    active: bool,
    osr: u8,
    /// LOG:CHOP OFF. Stored inverted so that the all-zero `new()` (see
    /// `init_in_place`) is the chopped default.
    nochop: bool,
    /// LOG:SOURce AD2 (false = AD1, the all-zero default).
    ad2: bool,
    /// The running session's producer is the stream ISR.
    isr: bool,
    /// The running / last session's producer was the stream ISR (`lost`).
    isr_sess: bool,
    /// `plan` ran for the next session (`r26` holds its rung).
    planned: bool,
    /// LOG:SRATe (AD2), S/s; 0 = DEFAULT_AD2_HZ.
    req_hz: u32,
    /// R26 low bits of the running AD2 session (preamble).
    r26: u8,
    mode: u8,
    range: i8,
    /// Main-loop producer (AD1, main-loop AD2).
    prod: Producer,
    ring: *const Ring,
    /// The running session belongs to the DMM wave view (`start_wave`):
    /// the ring is its, not the host's (no LOG:DATA?, LOG:STATe? 0, the GUI
    /// runs). The host's LOG:SOURce / LOG:SRATe saved for the stop.
    wave: bool,
    saved_ad2: bool,
    saved_hz: u32,
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
    /// .dmabuf, not zeroed at reset): `new()` is all zero bits (false / 0 /
    /// null), and all zero bits is a valid Logger; then the ring.
    pub fn init_in_place(this: *mut Logger, ring: &'static Ring) {
        unsafe {
            core::ptr::write_bytes(this, 0, 1);
            (*this).ring = ring;
        }
        ring.reset();
    }

    /// All-zero (see `init_in_place`). Runtime defaults (OSR) are applied in
    /// main.rs via `set_osr(DEFAULT_OSR)`.
    pub const fn new() -> Self {
        Logger {
            active: false,
            osr: 0,
            nochop: false,
            ad2: false,
            isr: false,
            isr_sess: false,
            planned: false,
            req_hz: 0,
            r26: 0,
            mode: 0,
            range: 0,
            prod: Producer::new(),
            ring: core::ptr::null(),
            wave: false,
            saved_ad2: false,
            saved_hz: 0,
        }
    }

    fn ring(&self) -> &Ring {
        unsafe { &*self.ring }
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
    /// AD1 chopper (half the rate) / AD2 ENCHOPAD2 (same rate); offset cancelled; default
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
        if self.wave { self.saved_ad2 = on } else { self.ad2 = on }
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
        if self.wave { self.saved_hz = hz } else { self.req_hz = hz }
    }
    pub fn req_hz(&self) -> u32 {
        if self.req_hz == 0 { DEFAULT_AD2_HZ } else { self.req_hz }
    }
    /// R26 low bits the next / running AD2 session uses.
    pub fn r26(&self) -> u8 {
        if (self.active || self.planned) && self.ad2 { self.r26 } else { hy::ad2_r26_for(self.req_hz(), self.chop()) }
    }
    pub fn mode(&self) -> u8 {
        self.mode
    }
    pub fn range(&self) -> i8 {
        self.range
    }
    pub fn nominal_us(&self) -> u32 {
        self.prod.nominal_us
    }
    /// Nominal (chip) rate of the session in mHz.
    pub fn nominal_mhz(&self) -> u32 {
        let p = self.prod.period_ns;
        if p == 0 { 0 } else { (1_000_000_000_000u64 / p as u64) as u32 }
    }
    pub fn buffered(&self) -> u32 {
        self.ring().len()
    }
    pub fn captured(&self) -> u32 {
        self.ring().captured.load(Relaxed)
    }
    /// Captured samples dropped because the ring was full (the host did not
    /// drain LOG:DATA? fast enough). The ring keeps the OLDEST records (the
    /// stream ISR cannot move the consumer's tail), so the host sees the
    /// loss as a timestamp gap, the seqs stay consecutive.
    pub fn overwritten(&self) -> u32 {
        self.ring().dropped.load(Relaxed)
    }
    pub fn missed(&self) -> u32 {
        self.ring().missed.load(Relaxed)
    }
    pub fn bad(&self) -> u32 {
        self.ring().bad.load(Relaxed)
    }
    pub fn gaps(&self) -> [u32; GAP_BUCKETS] {
        let g = &self.ring().gaps;
        [0, 1, 2, 3, 4].map(|i| g[i].load(Relaxed))
    }
    pub fn max_gap_us(&self) -> u32 {
        self.ring().max_gap_us.load(Relaxed)
    }

    /// Conversions the session did not capture, from the span: the chip
    /// delivered round(span / period) + 1 results between the first and the
    /// last captured sample; `captured` of them were taken. Unlike `missed`
    /// (per gap, > 1.5 periods) it also counts the many slightly-late
    /// samples of a rate the loop cannot sustain (9600/s: a 104 us period vs
    /// ~122 us per bit-banged sample). The main-loop sampler counts against
    /// the nominal period (a chip clock off by x % shows as x % lost, or as
    /// 0); the stream ISR counts the periods (its A entries).
    pub fn lost(&self) -> u32 {
        let (captured, span) = (self.captured(), self.ring().span_us());
        // The stream ISR counts the conversion periods themselves (one entry
        // A each, phase-locked to the chip): lost = periods - captured,
        // exact, whatever the crystal offset.
        #[cfg(feature = "ad2-isr")]
        if self.isr_sess && isr::lock_stats()[8] != 0 {
            return isr::lock_stats()[8].saturating_sub(captured);
        }
        let p = self.prod.period_ns;
        if captured < 2 || p == 0 {
            return 0;
        }
        let due = (span * 1000 + p as u64 / 2) / p as u64 + 1;
        due.saturating_sub(captured as u64).min(u32::MAX as u64) as u32
    }

    /// Measured mean sample rate in Hz over the current session, from the first
    /// and last sample times (64-bit, no 71.6 min wrap) and the capture count
    /// (the real number, off the device, not the nominal rate). 0 until at
    /// least two samples.
    pub fn measured_hz(&self) -> u32 {
        let (captured, span) = (self.captured(), self.ring().span_us());
        if captured < 2 || span == 0 {
            return 0;
        }
        (((captured as u64 - 1) * 1_000_000) / span) as u32
    }

    /// The next session's settings, without a bus access: mode / range (the
    /// preamble), the AD2 rung (capped by the stream ISR's read divider),
    /// the period (ns, returned; `nominal_mhz` / `r26` report them). `start`
    /// runs it first; the fast logger's session start (main.rs
    /// `fast_start`, region C) runs it alone to write the file header
    /// before `start` hands the bus to the stream ISR (no `.cold` code may
    /// run while it streams: a region C fetch stalls the ISR's entry,
    /// link-native.x). Not during a session.
    pub fn plan(&mut self, mode: u8, range: i8) -> u32 {
        self.mode = mode;
        self.range = range;
        let period_ns = if self.ad2 {
            // The stream ISR's read divider caps the rung.
            #[cfg(feature = "ad2-isr")]
            let req = {
                let dma = hy::stream::use_dma();
                let (div, _) = hy::stream::choose_div(dma);
                self.req_hz().min(hy::stream::max_rate_hz(div, dma))
            };
            #[cfg(not(feature = "ad2-isr"))]
            let req = self.req_hz();
            self.r26 = hy::ad2_r26_for(req, self.chop());
            hy::ad2_period_ns(self.r26)
        } else {
            self.r26 = 0;
            hy::ad1_period_us(self.osr, self.chop()) * 1000
        };
        self.prod.period_ns = period_ns;
        self.planned = true;
        period_ns
    }

    /// The next AD1 session runs on the stream ISR (feature `ad2-isr`): the
    /// fast unchopped AD1 settings (160 .. 640 S/s, period <= AD1_ISR_MAX_US), where a
    /// main-loop pass (a panel redraw, a page program, USB) can outlast the
    /// period (device 036cc55: 1.8 % lost at 640 S/s with the main-loop
    /// sampler). The slower ones keep the main-loop sampler (a period of
    /// 12.5 ms or more; no lock-in time before the first sample).
    /// Unchopped only: the chopped settings run ~5 % slower than their
    /// nominal (device 5394b21: 26.3 ms at OSR 2048 chopped, nominal 25),
    /// beyond the Pll's +-1000 ppm, which locks to the nominal rate.
    pub fn ad1_on_isr(&self) -> bool {
        cfg!(feature = "ad2-isr") && !self.ad2 && !self.chop() && hy::ad1_period_us(self.osr, false) <= AD1_ISR_MAX_US
    }

    /// Begin a session: configure the HY3131 (AD1: R22 OSR / chopper; AD2:
    /// R26 rung, AD3 / RMS / LPF off), reset all state and, for AD2 with the
    /// `ad2-isr` feature (and the fast AD1 settings, `ad1_on_isr`), hand the
    /// bus to the stream ISR. `mode`/`range` are the current DMM function
    /// (for the preamble); the caller must have selected a mode `mode_ok`
    /// for the source, written its full row (dmm.start) and powered the
    /// analog side.
    pub fn start(&mut self, mode: u8, range: i8) {
        self.stop();
        let period_ns = self.plan(mode, range);
        let on_isr = self.ad2 || self.ad1_on_isr();
        #[cfg(feature = "ad2-isr")]
        if self.ad2 {
            let _ = hy::stream::pick_div(hy::stream::use_dma());
        } else if on_isr {
            hy::stream::use_div(hy::DIV, false);
        }
        self.ring().reset();
        // The chip set-up is the last main-loop bus access of an ISR session.
        if self.ad2 {
            hy::set_ad2_stream(self.r26);
        } else {
            hy::set_ad1_osr(self.osr, self.chop());
        }
        self.prod.begin(period_ns);
        self.active = true;
        self.isr_sess = false;
        #[cfg(feature = "ad2-isr")]
        if on_isr {
            let rate = if self.ad2 { hy::ad2_rate_mhz(self.r26) } else { hy::ad1_rate_mhz(self.osr, self.chop()) };
            isr::start(&self.prod, self.ring, rate, !self.ad2);
            self.isr = true;
            self.isr_sess = true;
        }
        #[cfg(not(feature = "ad2-isr"))]
        let _ = on_isr;
    }

    /// End the session (records stay readable). With the stream ISR: the
    /// timer is stopped and the bus is the main loop's again on return; the
    /// caller restores the HY3131 (app.dmm.start).
    pub fn stop(&mut self) {
        #[cfg(feature = "ad2-isr")]
        if self.isr {
            hy::stream::stop();
            isr::stopped();
        }
        self.isr = false;
        self.active = false;
        self.planned = false;
        self.end_wave();
    }

    /// A session for the DMM wave view (scope page, source DMM): AD2 at
    /// `hz` (a ladder rung) in the current AC row, the host's LOG:SOURce /
    /// LOG:SRATe untouched. Same chip set-up and ISR hand-off as `start`.
    pub fn start_wave(&mut self, mode: u8, range: i8, hz: u32) {
        self.stop();
        (self.saved_ad2, self.saved_hz) = (self.ad2, self.req_hz);
        self.ad2 = true;
        self.req_hz = hz;
        self.start(mode, range);
        self.wave = true;
    }

    fn end_wave(&mut self) {
        if self.wave {
            self.wave = false;
            (self.ad2, self.req_hz) = (self.saved_ad2, self.saved_hz);
        }
    }

    /// The running (or last) session is the wave view's.
    pub fn wave(&self) -> bool {
        self.wave
    }

    /// A host (LOG:STATe ON) session runs: the GUI is frozen for it.
    pub fn host_active(&self) -> bool {
        self.active && !self.wave
    }

    /// The running session's producer is the stream ISR.
    pub fn on_isr(&self) -> bool {
        self.isr
    }

    /// DBG:ISR? numbers of the last / running ISR session: ticks (entries),
    /// late (A entries that skipped their read), probes, overruns (a tick
    /// already pending at exit), frame timeouts, max cycles in the handler,
    /// max entry latency in timer counts (20 ns).
    pub fn isr_stats(&self) -> [u32; 7] {
        let r = self.ring();
        [&r.ticks, &r.late, &r.probes, &r.overruns, &r.timeouts, &r.max_isr_cyc, &r.max_lat].map(|a| a.load(Relaxed))
    }

    /// Main-loop entry. With the stream ISR: no bus access; returns true
    /// once when the stream was stopped behind the logger's back (a
    /// main-loop bus access, hy3131 `guard`): the session is ended then and
    /// main.rs reports "Settings conflict". Otherwise the main-loop sampler
    /// (below); returns false.
    pub fn poll(&mut self) -> bool {
        if !self.active {
            return false;
        }
        if self.isr {
            if !hy::streaming() {
                self.isr = false;
                self.active = false;
                self.end_wave();
                return true;
            }
            return false;
        }
        self.sample();
        false
    }

    /// The main-loop sampler: capture fresh conversions. Cheap when idle (one DWT
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
    fn sample(&mut self) {
        let ring: &Ring = unsafe { &*self.ring };
        self.prod.tick();
        let end = self.prod.now_us + if self.ad2 { AD2_BURST_US } else { 0 };
        let half = self.prod.gate_us as u64;
        loop {
            let open = if self.prod.have_last { self.prod.last_sample_us + half } else { 0 };
            if self.prod.now_us < open {
                // Gate closed: wait here only if it opens within the burst.
                if open > end {
                    return;
                }
                while self.prod.now_us < open {
                    self.prod.tick();
                }
            }
            let ready = if self.ad2 { hy::ad2_ready() } else { hy::ad1_ready() };
            let p = if self.prod.p_meas != 0 { self.prod.p_meas } else { self.prod.nominal_us } as u64;
            let since = self.prod.now_us.wrapping_sub(self.prod.last_sample_us);
            if !ready && !self.ad2 && self.prod.have_last && self.prod.rescued < RESCUE_MAX && since >= p + p / 4 && since < p + 3 * p / 4 {
                // No AD1F a quarter period past the conversion's due time:
                // an INTF read raced the conversion and cleared its flag
                // unreported (the register clears on read, as for AD2F,
                // feasibility 9.5; emulator IntfRaceAd1). R00..R02 still hold
                // the result for the rest of the period: read it blind,
                // stamped at its due time (one period after the last
                // sample's observation, the measured period); not near the next conversion
                // (a read across its update could tear). At most RESCUE_MAX
                // in a row (a converter that stopped is not read as a
                // stream of copies; a tight polling loop, the emulator's,
                // loses one flag in ~4 at 40 S/s); counted in DBG:ISR? `late`.
                self.prod.rescued += 1;
                self.prod.last_sample_us += p;
                inc(&ring.late);
                let _ = self.prod.push(ring, self.prod.last_sample_us as u32, hy::read_ad1());
            } else if ready {
                self.prod.tick(); // the flag-observed moment
                let d = self.prod.now_us.wrapping_sub(self.prod.last_sample_us);
                let nom = self.prod.nominal_us as u64;
                if !self.ad2 && self.prod.have_last && self.prod.rescued == 0 && d > nom / 2 && d < nom * 3 / 2 {
                    self.prod.p_meas = d as u32;
                }
                self.prod.rescued = 0;
                self.prod.last_sample_us = self.prod.now_us;
                let ts = self.prod.now_us as u32;
                if self.ad2 {
                    let (raw, ok) = hy::read_ad2();
                    if ok {
                        let _ = self.prod.push(ring, ts, raw);
                    } else {
                        inc(&ring.bad);
                    }
                } else {
                    let _ = self.prod.push(ring, ts, hy::read_ad1());
                }
            }
            self.prod.tick();
            if self.prod.now_us >= end {
                return;
            }
        }
    }

    /// Pop oldest-first records into `out` (raw 12-byte LE records, no framing).
    /// Returns the number of bytes written. Transport-agnostic: USBTMC today,
    /// BT later.
    pub fn drain(&mut self, out: &mut [u8]) -> usize {
        let ring = self.ring();
        let t = ring.tail.load(Relaxed);
        let n = (ring.head.load(Acquire).wrapping_sub(t) as usize).min(out.len() / REC_BYTES);
        let base = ring.buf.get() as *const Rec;
        for i in 0..n {
            let k = t.wrapping_add(i as u32);
            // SAFETY: k is below the Acquire-loaded head: published, and the
            // producer does not write it again before `tail` passes it.
            let r = unsafe { base.add(k as usize & (CAP - 1)).read_volatile() };
            let seq = k.wrapping_add(1);
            let o = i * REC_BYTES;
            out[o..o + 4].copy_from_slice(&seq.to_le_bytes());
            out[o + 4..o + 8].copy_from_slice(&r.ts_us.to_le_bytes());
            out[o + 8..o + 12].copy_from_slice(&r.raw.to_le_bytes());
        }
        ring.tail.store(t.wrapping_add(n as u32), Release);
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
        // The wave view's ring is the scope's, not the host's: an empty block.
        let avail = if self.active && self.wave { 0 } else { self.ring().len() as usize };
        let nrec = avail.min(maxrec).min(9999 / REC_BYTES);
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

/// The stream ISR (feature `ad2-isr`; AD2, and AD1 unchopped at 160 .. 640 S/s with
/// R00..R02 / AD1F in place of R03..R05 / AD2F, CPU frames at /16, the
/// timer at 20 ns << KSH per count; docs/decisions-2026-10-22.md #3,
/// docs/dmm-input-scope-feasibility.md 10): TIMER6 (STM32 TIM7, IRQ 55, the
/// only interrupt fw-rs ever enables), phase-locked to the HY3131's
/// conversion grid by src/ad2pll.rs. Entry A, once per conversion period,
/// half way between two conversion edges: R03..R05 blind (one 3-byte SPI2
/// frame, the flag is not needed: the register holds the result for a whole
/// period), the sign-copy check, the ring. Every 4th period an entry B where
/// the edge should be: one INTF read (clears AD2F); the next A reads INTF
/// once more after its data frame, and AD2F there says on which side of B the
/// edge came (`Pll::result`). Reading INTF clears AD2F and loses a
/// conversion that completes during the frame (device 5f14d38: the old 4 x
/// tick lost ~1.4 % in bursts while its read phase sat on the edge): here
/// that race can only blur a probe, never cost a sample.
///
/// Stamps: the DWT time of entry A (decision #2, the observation time), one
/// period apart to within the entry latency. A late A (latency above
/// `Pll::late_max`, where the frame could reach the next edge) skips its read
/// (`late`; the host sees a stamp gap of 2 periods); an interval whose end
/// had passed before the ISR set it is stretched and the next one shortened
/// by as much (`slips`), so the grid keeps its phase.
///
/// Bounded: at most two frames with capped status waits (hy3131
/// `spi::wait`), no loops otherwise, no allocation, no float, no flash / LCD
/// / watchdog, never a register write (HY3131) and never a call into dmm.rs.
/// Shared with the main loop: `RING`, plus the statistics words below (each
/// written by the ISR only, read by DBG:ISR?).
#[cfg(feature = "ad2-isr")]
pub mod isr {
    use super::*;
    use crate::ad2pll::{Pll, COUNT_HZ};

    /// The ISR's producer state and the lock (SRAM .sram_bss, all zero at
    /// reset: off the TCM stack's budget). Written by `start` (main loop)
    /// before the hand-off, by the ISR alone after it; the main loop reads
    /// the Pll's statistics words (DBG:ISR?, u32 loads) only.
    #[cfg_attr(target_os = "none", link_section = ".sram_bss")]
    static mut PROD: Producer = Producer::new();
    #[cfg_attr(target_os = "none", link_section = ".sram_bss")]
    static mut PLL: Pll = Pll::idle();
    static mut RINGP: *const Ring = core::ptr::null();
    /// The session's source: AD1 (R00..R02, INTF.AD1F; CPU frames) or AD2
    /// (R03..R05, AD2F), and the timer's count scale: one count = 20 ns <<
    /// KSH (PSC 4 << KSH - 1), so that an AD1 period (1.56 .. 6.25 ms) fits
    /// the 16-bit ARR; the Pll runs in those counts (`Pll::new` at the rate
    /// << KSH), the statistics (`max_lat`, `lock_stats`) in 20 ns / mHz.
    #[cfg_attr(target_os = "none", link_section = ".sram_bss")]
    static mut AD1: bool = false;
    #[cfg_attr(target_os = "none", link_section = ".sram_bss")]
    static mut KSH: u32 = 0;
    /// The last session's frequency word offset (Pll::rel_q24): the next
    /// session starts on it, so it locks at once (AD1 and AD2 share it: both
    /// rates derive from the chip's crystal).
    #[cfg_attr(target_os = "none", link_section = ".sram_bss")]
    static mut REL: i32 = 0;
    /// Data frame (CS low .. CS high) + ISR entry, timer counts, per divider
    /// (device SELF? 8.6: us_r6 13.4 / 21.8 at /8 / 16, ~1.3 / 2.8 us per byte).
    const fn frame_counts(div: u8) -> u32 {
        match div {
            8 => 400,   // 8 us
            16 => 700,  // 14 us
            _ => 1300,  // 26 us
        }
    }
    /// The DMA frame in flight (hy3131 `stream::dma_*`): started by A, ended
    /// by the next entry. `on`: this session uses DMA frames.
    struct Dma {
        on: bool,
        pending: bool,
        /// The frame in flight: DATA (R03..R05, `ts` = A's stamp), CLEAR
        /// (B's INTF read) or PROBE (C's INTF read: the probe result).
        kind: u8,
        ts: u32,
    }
    const DATA: u8 = 0;
    const CLEAR: u8 = 1;
    const PROBE: u8 = 2;
    #[cfg_attr(target_os = "none", link_section = ".sram_bss")]
    static mut DMA: Dma = Dma { on: false, pending: false, kind: 0, ts: 0 };
    /// The INTF frame: command (R1E, read) + one byte (the read period and
    /// INTF bits 7..1: AD2F = bit 0 of the reply's byte 1).
    #[cfg_attr(target_os = "none", link_section = ".sram_bss")]
    static mut TXI: [u8; 2] = [0; 2];
    /// The frame's bytes (SRAM: DMA0 cannot reach the TCM): the command
    /// (R03, read) + 4 x 0xFF out, the 5 replies in.
    #[cfg_attr(target_os = "none", link_section = ".sram_bss")]
    static mut TXB: [u8; 5] = [0; 5];
    #[cfg_attr(target_os = "none", link_section = ".sram_bss")]
    static mut RXB: [u8; 5] = [0; 5];
    /// DMA frame (5 bytes + CS / start), timer counts, per divider.
    const fn dma_frame_counts(div: u8) -> u32 {
        match div {
            8 => 360,   // 7.2 us
            16 => 680,  // 13.6 us
            _ => 1320,  // 26.4 us
        }
    }

    /// A probe B whose entry came later than this (counts) is not used.
    const B_LATE: u32 = 100;
    /// The stacked PC of the entry with the largest latency (DBG:ISR?):
    /// where the main loop was when the ISR was held off longest.
    #[cfg_attr(target_os = "none", link_section = ".sram_bss")]
    static LAT_PC: AtomicU32 = AtomicU32::new(0);
    #[cfg_attr(target_os = "none", link_section = ".sram_bss")]
    static SLIPS: AtomicU32 = AtomicU32::new(0);
    /// Cycles spent in the handler (body, from the CNT read to the exit
    /// checks), / 16 (wraps after ~340 s): the CPU share (DBG:ISR?).
    #[cfg_attr(target_os = "none", link_section = ".sram_bss")]
    static BUSY16: AtomicU32 = AtomicU32::new(0);

    /// The ISR's copy of the wave trigger (`trig`): configuration of
    /// generation `gen`, the detector state, the first event at or after
    /// `min` reported.
    struct Trg {
        gen: u32,
        cfg: StreamCfg,
        min: u32,
        st: Stream,
        first: bool,
    }
    static mut TRG: Trg = Trg { gen: 0, cfg: StreamCfg::OFF, min: 0, st: Stream::NEW, first: false };

    /// Per stored sample `idx` (the ring index = the wave record's sample
    /// number) with AD2 code `v`: the wave trigger (trigsoft::Stream on the
    /// record's 10-bit code; generation 0 = off). A few loads and compares;
    /// an event goes to the event ring, the first one at or after the
    /// configured minimum also to `trg_first_*`.
    #[inline(always)]
    unsafe fn trig(ring: &Ring, idx: u32, v: i32) {
        let g = ring.trg_gen.load(Acquire);
        if g == 0 {
            return;
        }
        let t = &mut *core::ptr::addr_of_mut!(TRG);
        if g != t.gen {
            let w = |k: usize| ring.trg_w[k].load(Relaxed);
            t.gen = g;
            t.cfg = StreamCfg { ttype: w(0) as u8, rising: w(0) & 0x100 != 0, l: w(1) as i32, l2: w(2) as i32, wlo: w(3), whi: w(4) };
            t.min = w(5);
            t.st = Stream::NEW;
            t.first = false;
        }
        if t.st.step(&t.cfg, idx, crate::trigsoft::ad2_code10(v)) {
            let n = ring.trg_ev_n.load(Relaxed);
            ring.trg_ev[n as usize % TRG_EV].store(idx, Relaxed);
            ring.trg_ev_n.store(n.wrapping_add(1), Release);
            if !t.first && idx.wrapping_sub(t.min) < 0x8000_0000 {
                t.first = true;
                ring.trg_first_at.store(idx, Relaxed);
                ring.trg_first_gen.store(g, Release);
            }
        }
    }

    /// Main loop, after the session's chip set-up (the last bus access but
    /// the edge search here; the read divider is set): find one conversion
    /// edge by polling INTF (bounded: 3 periods), start the timer so that
    /// the first A comes `P - beta` after it, hand the bus over.
    pub(super) fn start(prod: &Producer, ring: *const Ring, rate_mhz: u32, ad1: bool) {
        let div = hy::stream::div_used(); // Logger::start picked it
        let mut ksh = 0u32;
        while (prod.period_ns / 20) >> ksh > 60_000 {
            ksh += 1;
        }
        let flag = if ad1 { hy::INTF_AD1 } else { hy::INTF_AD2 };
        unsafe {
            *core::ptr::addr_of_mut!(AD1) = ad1;
            *core::ptr::addr_of_mut!(KSH) = ksh;
            (*core::ptr::addr_of_mut!(TRG)).gen = 0;
            let p = &mut *core::ptr::addr_of_mut!(PROD);
            p.begin(prod.period_ns);
            *core::ptr::addr_of_mut!(RINGP) = ring;
            let pll = &mut *core::ptr::addr_of_mut!(PLL);
            let dma = !ad1 && hy::stream::use_dma();
            *core::ptr::addr_of_mut!(DMA) = Dma { on: dma, pending: false, kind: DATA, ts: 0 };
            if dma {
                *core::ptr::addr_of_mut!(TXB) = [(hy::R_AD2 << 1) | 1, 0xFF, 0xFF, 0xFF, 0xFF];
                *core::ptr::addr_of_mut!(TXI) = [(hy::R_INTF << 1) | 1, 0xFF];
                let fc = dma_frame_counts(div);
                *pll = Pll::new(rate_mhz, *core::ptr::addr_of!(REL), fc);
                pll.late_read = true;
                pll.c_off = fc + 50;
                pll.np = 8;
                pll.gate = true;
                hy::stream::dma_setup(true);
            } else {
                *pll = Pll::new(rate_mhz << ksh, *core::ptr::addr_of!(REL), frame_counts(div) >> ksh);
            }
        }
        LAT_PC.store(0, Relaxed);
        SLIPS.store(0, Relaxed);
        BUSY16.store(0, Relaxed);
        let pll = unsafe { &*core::ptr::addr_of!(PLL) };
        let pc = pll.f >> 16; // period, counts
        // Edge search: AD2F seen by read j and not by read j - 1 = the edge
        // between their sampling points (~ the start + the command byte +
        // the read period; CS setup .. the INTF bit ~ 2.5 us at /16).
        let cyc_per_count = (crate::clock::HCLK_HZ / COUNT_HZ) << ksh; // 4 << KSH
        let mut f = [0u8];
        let _ = hy::isr_read(hy::R_INTF, &mut f);
        let t_begin = unsafe { r32(DWT_CYCCNT) };
        let mut prev = t_begin;
        let mut edge: Option<u32> = None;
        loop {
            let t = unsafe { r32(DWT_CYCCNT) };
            if !hy::isr_read(hy::R_INTF, &mut f) {
                break;
            }
            if f[0] & flag != 0 {
                edge = Some(prev.wrapping_add(t.wrapping_sub(prev) / 2));
                break;
            }
            prev = t;
            // 3 periods (AD2); AD1: 8 (its first result after the R22
            // write may take longer: device 5394b21 found no edge in 3).
            if t.wrapping_sub(t_begin) > if ad1 { 8 } else { 3 } * pc * cyc_per_count {
                break;
            }
        }
        // First A: P - beta after the edge (the next edge then beta after
        // A); without an edge anywhere (the loop finds it, ad2pll tests).
        let lead = (pc - pll.beta) * cyc_per_count;
        let now = unsafe { r32(DWT_CYCCNT) };
        let first = match edge {
            Some(e) => {
                let mut due = e.wrapping_add(lead);
                while due.wrapping_sub(now) as i32 <= (50 * cyc_per_count) as i32 {
                    due = due.wrapping_add(pc * cyc_per_count);
                }
                due.wrapping_sub(now) / cyc_per_count
            }
            None => pc,
        };
        if ad1 && edge.is_none() {
            // No edge seen: blind reads only once locked (a read across the
            // update would tear the 24-bit result; AD1 has no sign copy).
            unsafe { (*core::ptr::addr_of_mut!(PLL)).gate = true };
        }
        hy::stream::start(first.saturating_sub(4), (4 << ksh) - 1);
    }

    /// Main loop, after `hy::stream::stop`: keep the lock for the next one.
    pub(super) fn stopped() {
        unsafe {
            let pll = &*core::ptr::addr_of!(PLL);
            if pll.locked {
                *core::ptr::addr_of_mut!(REL) = pll.rel_q24();
            }
        }
    }

    /// The lock's numbers for DBG:ISR?: (probes, edges after B, max run,
    /// locked, locked rate mHz, period offset ppm x 10 vs the nominal rung
    /// (+ = the chip's conversions come later than nominal in MCU time),
    /// slips, the PC of the worst entry latency, conversion periods, the
    /// handler's CPU share in 1/1000 of the session's periods - the
    /// exception entry and exit, ~30 cycles each, not included - and whether
    /// the session uses DMA frames).
    pub fn lock_stats() -> [u32; 11] {
        let p = unsafe { &*core::ptr::addr_of!(PLL) };
        let ksh = unsafe { *core::ptr::addr_of!(KSH) };
        let ppm10 = ((p.f_mean() as i64 - p.f_nom as i64) * 10_000_000 / (p.f_nom.max(1) as i64)) as i32;
        [
            p.probes,
            p.late_edges,
            p.max_run,
            p.locked as u32,
            p.rate_mhz() >> ksh,
            ppm10 as u32,
            SLIPS.load(Relaxed),
            LAT_PC.load(Relaxed),
            // A DMA frame still in flight (the last one of a stopped session
            // is never stored) is not a lost conversion.
            p.periods.wrapping_sub(unsafe { (*core::ptr::addr_of!(DMA)).pending } as u32),
            {
                let busy = BUSY16.load(Relaxed) as u64 * 16;
                let total = p.periods as u64 * (p.f_mean() as u64 >> 16) * ((crate::clock::HCLK_HZ / COUNT_HZ) << ksh) as u64;
                if total == 0 { 0 } else { (busy * 1000 / total) as u32 }
            },
            unsafe { (*core::ptr::addr_of!(DMA)).on } as u32,
        ]
    }

    /// IRQ 55 through the shim's `ad2_irq_entry` (src/shim.rs), which passes
    /// the exception frame (MSP at entry: r0 r1 r2 r3 r12 lr pc xpsr).
    #[no_mangle]
    pub unsafe extern "C" fn ad2_stream_irq(frame: *const u32) {
        let lat = hy::stream::ack();
        if !hy::streaming() || RINGP.is_null() {
            hy::stream::disarm();
            return;
        }
        let t0 = r32(DWT_CYCCNT);
        let ad1 = AD1;
        let flag = if ad1 { hy::INTF_AD1 } else { hy::INTF_AD2 };
        let ring = &*RINGP;
        let pll = &mut *core::ptr::addr_of_mut!(PLL);
        let p = &mut *core::ptr::addr_of_mut!(PROD);
        let d = &mut *core::ptr::addr_of_mut!(DMA);
        if pll.next_b || pll.next_c {
            let c = pll.next_c;
            let ok = lat <= B_LATE;
            let iv = if c { pll.on_c() } else { pll.on_b(ok) };
            let over = hy::stream::set_interval(iv, lat);
            if over != 0 {
                pll.overshoot(over);
                inc(&SLIPS);
            }
            if d.pending {
                dma_done(d, p, ring, pll);
            }
            if d.on && (c || ok) {
                // The INTF read as a DMA frame (2 bytes: 5.1 us at /16 - a CPU
                // frame, ~8 us, overran the B -> A interval at 38.4 kS/s).
                d.kind = if c { PROBE } else { CLEAR };
                d.pending = true;
                hy::stream::dma_kick(core::ptr::addr_of!(TXI) as *const u8, core::ptr::addr_of_mut!(RXB) as *mut u8, 2);
            } else if c || ok {
                let mut f = [0u8];
                if !hy::isr_read(hy::R_INTF, &mut f) {
                    inc(&ring.timeouts);
                } else if c {
                    pll.result(f[0] & flag != 0);
                    inc(&ring.probes);
                }
            }
        } else {
            let (iv, probe) = pll.on_a();
            let over = hy::stream::set_interval(iv, lat);
            if over != 0 {
                pll.overshoot(over);
                inc(&SLIPS);
            }
            p.tick_at(t0);
            if d.pending {
                dma_done(d, p, ring, pll);
            }
            if !pll.take() {
                // Acquiring (DMA frames, or AD1 without an edge from the
                // search: until locked): no sample. A CPU-frame probe still
                // completes here (else a gated CPU stream never locks:
                // device 5394b21, AD1 640 S/s, probes 0 for 700 s).
                if probe && !d.on {
                    let mut f = [0u8];
                    if hy::isr_read(hy::R_INTF, &mut f) {
                        pll.result(f[0] & flag != 0);
                        inc(&ring.probes);
                    } else {
                        inc(&ring.timeouts);
                    }
                }
            } else if lat > pll.late_max() {
                inc(&ring.late);
            } else if d.on {
                d.ts = p.now_us as u32;
                d.kind = DATA;
                d.pending = true;
                hy::stream::dma_kick(core::ptr::addr_of!(TXB) as *const u8, core::ptr::addr_of_mut!(RXB) as *mut u8, 5);
            } else {
                let ts = p.now_us as u32;
                let mut b = [0u8; 3];
                if !hy::isr_read(if ad1 { hy::R_AD1 } else { hy::R_AD2 }, &mut b) {
                    inc(&ring.timeouts);
                } else if ad1 {
                    // 24-bit two's complement, no sign copy to check.
                    let _ = p.push(ring, ts, hy::s24(&b));
                } else {
                    store(p, ring, ts, hy::s24(&b));
                }
                if probe {
                    let mut f = [0u8];
                    if hy::isr_read(hy::R_INTF, &mut f) {
                        pll.result(f[0] & flag != 0);
                        inc(&ring.probes);
                    } else {
                        inc(&ring.timeouts);
                    }
                }
            }
        }
        inc(&ring.ticks);
        let dt = r32(DWT_CYCCNT).wrapping_sub(t0);
        BUSY16.store(BUSY16.load(Relaxed).wrapping_add(dt >> 4), Relaxed);
        if dt > ring.max_isr_cyc.load(Relaxed) {
            ring.max_isr_cyc.store(dt, Relaxed);
        }
        let lat20 = lat << KSH; // 20 ns units
        if lat20 > ring.max_lat.load(Relaxed) {
            ring.max_lat.store(lat20, Relaxed);
            LAT_PC.store(*frame.add(6), Relaxed);
        }
        if hy::stream::overrun() {
            inc(&ring.overruns);
        }
    }

    /// A sample into the ring (sign-copy check) and the wave trigger.
    #[inline(always)]
    unsafe fn store(p: &mut Producer, ring: &Ring, ts: u32, v: i32) {
        if hy::ad2_sign_ok(v) {
            let idx = ring.head.load(Relaxed);
            if p.push(ring, ts, v) {
                trig(ring, idx, v);
            }
        } else {
            inc(&ring.bad);
        }
    }

    /// The DMA frame started at the last entry is over (this entry): CS up,
    /// then the data (decoded, stored with A's stamp) or the probe result.
    #[inline(always)]
    unsafe fn dma_done(d: &mut Dma, p: &mut Producer, ring: &Ring, pll: &mut Pll) {
        d.pending = false;
        if !hy::stream::dma_finish() {
            inc(&ring.timeouts);
            return;
        }
        let rx = &*core::ptr::addr_of!(RXB);
        match d.kind {
            DATA => store(p, ring, d.ts, decode_shift(rx)),
            PROBE => {
                pll.result(rx[1] & 1 != 0); // AD2F (INTF bit 1) one bit late
                inc(&ring.probes);
            }
            _ => {}
        }
    }
}

/// R03..R05 from a shift-framed 5-byte reply (hy3131 `spi::dma_kick`): byte
/// 0 answers the command; bytes 1..4 carry the read-period bit, then R03,
/// R04, R05 (each MSB first), then 7 bits of R06.
pub fn decode_shift(rx: &[u8; 5]) -> i32 {
    let w = u32::from_be_bytes([rx[1], rx[2], rx[3], rx[4]]);
    let b = [(w >> 23) as u8, (w >> 15) as u8, (w >> 7) as u8];
    hy::s24(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(ad2: bool, period_ns: u32) -> std::boxed::Box<Logger> {
        let ring: &'static Ring = std::boxed::Box::leak(std::boxed::Box::new(Ring::new()));
        let mut l = std::boxed::Box::new(Logger::new());
        l.ring = ring;
        l.ad2 = ad2;
        l.prod.period_ns = period_ns;
        l.prod.nominal_us = (period_ns + 500) / 1000;
        l.active = true;
        l
    }

    fn feed(l: &mut Logger, ts: u64, raw: i32) {
        l.prod.now_us = ts;
        let ring: &Ring = unsafe { &*l.ring };
        l.prod.push(ring, ts as u32, raw);
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
    fn full_ring_drops_the_newest_and_seqs_stay_consecutive() {
        let mut l = session(true, 104_167);
        for i in 0..(CAP as u64 + 10) {
            feed(&mut l, 1000 + i * 104, i as i32 - 5);
        }
        assert_eq!(l.overwritten(), 10);
        assert_eq!(l.captured(), CAP as u32 + 10);
        assert_eq!(l.buffered(), CAP as u32);
        let r = recs(&mut l);
        assert_eq!(r.len(), CAP);
        assert_eq!(r[0].0, 1); // the oldest kept; the 10 newest were dropped
        assert_eq!(r[0].2, -5);
        assert!(r.windows(2).all(|w| w[1].0 == w[0].0 + 1));
        assert_eq!(r.last().unwrap().0, CAP as u32);
        assert_eq!(l.buffered(), 0);
        // After the drain the ring takes samples again; the gap shows in
        // the timestamps, the seqs go on.
        feed(&mut l, 1_000_000, -7);
        feed(&mut l, 1_000_104, -8);
        let mut b = [0u8; 12];
        assert_eq!(l.drain(&mut b), 12);
        assert_eq!(u32::from_le_bytes([b[0], b[1], b[2], b[3]]), CAP as u32 + 1);
        assert_eq!(i32::from_le_bytes([b[8], b[9], b[10], b[11]]), -7);
        assert_eq!(recs(&mut l)[0].0, CAP as u32 + 2);
    }

    #[test]
    fn ring_wraps_many_times() {
        let mut l = session(true, 104_167);
        let mut next = 1u32;
        for round in 0..20u64 {
            for i in 0..1500u64 {
                feed(&mut l, (round * 1500 + i) * 104, (round * 1500 + i) as i32);
            }
            let r = recs(&mut l);
            assert_eq!(r.len(), 1500);
            for (k, x) in r.iter().enumerate() {
                assert_eq!(x.0, next + k as u32);
                assert_eq!(x.2 as u32, next + k as u32 - 1);
            }
            next += 1500;
        }
        assert_eq!(l.overwritten(), 0);
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
        // A span past 2^32 us (71.6 min) keeps the 64-bit maths.
        let mut l = session(false, 25_000_000);
        feed(&mut l, 0, 0);
        feed(&mut l, 5_000_000_000, 0);
        assert_eq!(l.ring().span_us(), 5_000_000_000);
    }

    #[test]
    fn producer_clock_counts_cycles() {
        let mut p = Producer::new();
        p.last_cyc = u32::MAX - 99;
        p.tick_at(100); // 200 cycles across the wrap
        assert_eq!((p.now_us, p.acc), (1, 0));
        for _ in 0..3 {
            let c = p.last_cyc.wrapping_add(150);
            p.tick_at(c);
        }
        assert_eq!((p.now_us, p.acc), (3, 50));
        let c = p.last_cyc.wrapping_add(4_000_000_000);
        p.tick_at(c);
        assert_eq!(p.now_us, 3 + 20_000_000);
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

/// The DMM wave view's side of the ring (main loop; consumer + trigger
/// configuration). The ring is `RING`; the session is a `Logger::start_wave`
/// one, so the stream ISR is the producer.
pub mod wave {
    use super::*;

    /// Pop every buffered sample (oldest first) into `f`; the count.
    pub fn drain(f: &mut dyn FnMut(i32)) -> u32 {
        let ring = &RING;
        let t = ring.tail.load(Relaxed);
        let n = ring.head.load(Acquire).wrapping_sub(t);
        let base = ring.buf.get() as *const Rec;
        for i in 0..n {
            // SAFETY: below the Acquire-loaded head (Logger::drain).
            let r = unsafe { base.add(t.wrapping_add(i) as usize & (CAP - 1)).read_volatile() };
            f(r.raw);
        }
        ring.tail.store(t.wrapping_add(n), Release);
        n
    }

    /// Holes in the stream so far: samples dropped because the ring was
    /// full, plus conversions the ISR missed (`missed`, from the stamps: a
    /// gap > 1.5 periods; the device check of 5f14d38 found ~11.5 ms bursts
    /// every ~3950 conversions). The wave record restarts after a hole
    /// instead of splicing across it.
    pub fn dropped() -> u32 {
        RING.dropped.load(Relaxed).wrapping_add(RING.missed.load(Relaxed))
    }

    /// New trigger configuration (`min`: the first sample index whose
    /// event `first` reports); returns its generation (never 0).
    pub fn set_trigger(c: &StreamCfg, min: u32) -> u32 {
        let r = &RING;
        let w = [c.ttype as u32 | (c.rising as u32) << 8, c.l as u32, c.l2 as u32, c.wlo, c.whi, min];
        for (a, v) in r.trg_w.iter().zip(w) {
            a.store(v, Relaxed);
        }
        let g = r.trg_gen.load(Relaxed).wrapping_add(1).max(1);
        r.trg_gen.store(g, Release);
        g
    }

    /// No trigger search in the ISR.
    pub fn trigger_off() {
        RING.trg_gen.store(0, Release);
    }

    /// Events since `*seen` (sample indices, oldest first; at most the
    /// TRG_EV - 1 newest: older ones are gone); `*seen` advances.
    pub fn events(seen: &mut u32, f: &mut dyn FnMut(u32)) {
        let r = &RING;
        let n = r.trg_ev_n.load(Acquire);
        let from = if n.wrapping_sub(*seen) > TRG_EV as u32 - 1 { n.wrapping_sub(TRG_EV as u32 - 1) } else { *seen };
        let mut k = from;
        while k != n {
            f(r.trg_ev[k as usize % TRG_EV].load(Relaxed));
            k = k.wrapping_add(1);
        }
        *seen = n;
    }

    /// Event count so far (for `events`).
    pub fn event_count() -> u32 {
        RING.trg_ev_n.load(Acquire)
    }

    /// The first event at or after the minimum of generation `gen`.
    pub fn first(gen: u32) -> Option<u32> {
        let r = &RING;
        (gen != 0 && r.trg_first_gen.load(Acquire) == gen).then(|| r.trg_first_at.load(Relaxed))
    }
}
