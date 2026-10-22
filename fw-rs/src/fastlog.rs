//! Fast logger to the SPI disk (FR-38 to disk, TODO "FR-38 logger to disk" /
//! "(B) AD2 samples to the SPI disk"; docs/compact-log-format.md "Fast
//! stream (kind 3)"): the logger.rs ring (AD1 up to 640 S/s, the AD2 stream
//! 1200 .. 38400 S/s) into `LOGS/FASTnnn.DLG`, served as FASTnnn.CSV on the
//! USB volume (vdisk.rs) and by MMEM:DATA? like every DLG.
//!
//! Why not csvlog's way (commit = data, FAT, directory blocks every 4 KiB /
//! minute): a FAT / root `write_block` stalls the main loop up to ~0.45 s
//! (an erase), the HY3131 has no FIFO, and at these rates one erase per
//! commit would also wear the root block out within days. Here the run
//! itself only ever PROGRAMS pages of pre-erased clusters:
//!
//!   Prep  (before the logger starts; one bounded slice per pass)
//!     Bpb    boot sector, LOGS/ (fatdir::ensure);
//!     Scan   one directory block per pass: FAST numbers used in the root
//!            and LOGS/, the first free LOGS/ slot;
//!     Alloc  one FAT 1 block per pass: free clusters (first fit, merged
//!            into at most NEXT extents) up to the size planned from the
//!            duration and the rate (`begin`), or every free one;
//!     Erase  per pass one blank check (a 4 KiB read) or one erase start;
//!            an erase runs in the chip between passes. A 64 KiB block all
//!            ours with a dirty sector: one 64 KiB erase (150 ms typ)
//!            instead of up to sixteen 4 KiB ones (45 ms typ each);
//!     Chain  the whole chain in every FAT copy (FAT 1 first; per touched
//!            block one read-modify-write, `write_block`);
//!     Entry  LOGS/FASTnnn.DLG: start cluster, size = the allocation.
//!   Ready -> main.rs starts the logger session (`Ev::Start`), then `run`
//!            puts the file header in.
//!   Run   per pass (hot, fastenc.rs): drain the ring (Logger::drain, the
//!         consumer side, as LOG:DATA? does) into the coder while the FIFO
//!         and the file have room, at most RUN_BUDGET_CYC of CPU; one page
//!         program when the chip is idle. No erase, no FAT / directory
//!         write, no `.cold` code.
//!   Stop  (`Ev::Stop` -> main.rs stops the logger) the rest of the ring,
//!         the last chunk, the last pages; then the entry's size = the bytes
//!         written, then the unused clusters freed in every FAT copy (FAT 1
//!         first), entry before FAT: a cut in between = a chain longer than
//!         the file (fsck truncates it; no byte of the file lost).
//!
//! Power cut: during Prep, before Chain: nothing visible (erased free
//! clusters); between the FAT copies / before the entry: lost clusters at
//! worst; during Run: the entry covers the whole allocation, every chunk
//! programmed so far decodes (the unwritten rest is 0xFF: a torn tail, the
//! readers stop there; vdisk / MMEM:DATA? serve the CSV of what is there);
//! during Stop: see above. (tools/fatren-test "fast": a cut before every
//! block write / erase / page program of a whole session, fsck, decode.)
//!
//! Falling behind: the FIFO (8 KiB) full -> the run stops draining, the
//! ring (2048 records) fills, the producer drops the newest samples
//! (logger.rs `overwritten`) and the file shows the hole as a stamp gap:
//! the CSV's `lost_before` column, as dm40scpi.py's. Nothing is lost
//! silently. The sustainable rate is set by the chip's page program time
//! (docs/compact-log-format.md "Fast stream: rate limits").
//!
//! Sharing: the session holds the disk from `begin` to the end of Stop
//! (`holds_disk`): no csvlog recording / commit, screenshot, scope record,
//! update upload or delete runs meanwhile (they refuse or wait), and the
//! vdisk refresh waits. The FIFO is csvlog's 8 KiB row FIFO, lent while no
//! CSV recording runs (`csvlog::lend_fifo`; the two never run together:
//! `begin` refuses during a recording, csvlog::start during a session).
//! The block buffers of Prep / Stop are calbackup::scratch() (held disk).

#![cfg_attr(not(target_os = "none"), allow(dead_code))]

use crate::fastenc::{FastEnc, Nor, Pump, NEXT};
use crate::msc::BlockDevice;

const BLK: usize = 4096;
const PER: u32 = (BLK / 32) as u32;
const FPER: u32 = (BLK / 2) as u32;
/// Records drained per batch (12 bytes each on the stack).
const BATCH: usize = 32;
/// CPU per Run pass (cycles, 300 us at 200 MHz); a batch may run over.
const RUN_BUDGET_CYC: u32 = crate::clock::HCLK_HZ / 3333;
/// A chip operation that stays busy this long: the chip failed (cycles, 2 s;
/// tBE64 max 2 s).
const OP_TIMEOUT_CYC: u32 = crate::clock::HCLK_HZ * 2;
/// Planned bytes per sample (tools/dlog-test: 1.0..2.5 B on noisy /
/// moving signals, 2.93 B on full-scale white noise; docs/compact-log-
/// format.md "Fast stream"): AD1, AD2. A file that fills up before its
/// duration ends as Full.
pub const BPS_AD1: u32 = 3;
pub const BPS_AD2: u32 = 3;
/// Fixed allowance for the header text, parameters and chunk headers.
const HDR_ALLOW: u32 = 8192;
pub const DATE: u16 = (46 << 9) | (10 << 5) | 13;
const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Idle,
    /// Allocating / erasing / linking (no samples yet).
    Prep,
    /// The file exists, waiting for the logger session.
    Ready,
    Run,
    /// Last chunk, pages, size, freeing the rest.
    Stop,
    /// Ended normally (stopped, duration reached).
    Done,
    /// Ended: the allocation (the free space) is full.
    Full,
    /// No disk / not the expected FAT16 volume.
    NoDisk,
    /// A read / write / erase failed, or the disk changed under us.
    Error,
}

impl State {
    pub fn code(self) -> u32 {
        self as u32
    }
}

/// What main.rs must do after `poll`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ev {
    None,
    /// The file is ready: start the logger session, then `run`.
    Start,
    /// Stop the logger session (duration reached, file full, a stop
    /// request, an error); the rest is drained afterwards.
    Stop,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ph {
    None,
    Bpb,
    Scan,
    Alloc,
    Erase,
    /// The page-program probe (`set_probe`): a few pages programmed and timed
    /// in the last cluster of the allocation, which is then erased again.
    Probe,
    Chain,
    Entry,
    Wait,
    Run,
    Drain,
    Flush,
    Size,
    Trim,
    Unlink,
}

/// The header the session's file starts with (main.rs, from the logger and
/// the DMM at `Ev::Start`).
#[derive(Clone, Copy)]
pub struct Head<'a> {
    pub par: crate::dlog::FPar,
    pub unit: &'a [u8],
    pub text: &'a [u8],
}

pub struct Job {
    state: State,
    ph: Ph,
    v: Option<crate::fatren::Vol>,
    dur_us: u64,
    need: u32,
    // file
    n: u16,
    slot: u32,
    taken: [u32; 32],
    i: u32,
    ext: [(u32, u32); NEXT],
    next: usize,
    ncl: u32,
    alloc_c: u32,
    /// FAT 1 block cached in scratch[1].
    cur: u32,
    // erase
    ei: u32,
    group: u32,
    dirty: bool,
    erasing: bool,
    t_op: u32,
    pct: u8,
    /// Time the chip's page program (`set_probe`).
    probe: bool,
    // chain / trim
    fat_k: u32,
    fat_fb: u32,
    used: u32,
    // run
    enc: FastEnc,
    stop_req: bool,
    ended: bool,
    ev_sent: bool,
    end_state: State,
    busy_since: Option<u32>,
    hw: u32,
    dropped_end: u32,
    wrote: bool,
    /// The LOGS/ entry exists.
    has_file: bool,
}

const JOB0: Job = Job {
    state: State::Idle,
    ph: Ph::None,
    v: None,
    dur_us: 0,
    need: 0,
    n: 0,
    slot: NONE,
    taken: [0; 32],
    i: 0,
    ext: [(0, 0); NEXT],
    next: 0,
    ncl: 0,
    alloc_c: 2,
    cur: NONE,
    ei: 0,
    group: 0,
    dirty: false,
    erasing: false,
    t_op: 0,
    pct: 0,
    probe: false,
    fat_k: 0,
    fat_fb: 0,
    used: 0,
    enc: FastEnc::EMPTY,
    stop_req: false,
    ended: false,
    ev_sent: false,
    end_state: State::Done,
    busy_since: None,
    hw: 0,
    dropped_end: 0,
    wrote: false,
    has_file: false,
};

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut JOB: Job = JOB0;

#[inline(always)]
fn job() -> &'static mut Job {
    unsafe { &mut *core::ptr::addr_of_mut!(JOB) }
}

fn cyc() -> u32 {
    #[cfg(target_os = "none")]
    return unsafe { crate::mmio::r32(0xE000_1004) };
    #[cfg(not(target_os = "none"))]
    return host_cyc();
}

/// Host tests: a fake cycle counter (advanced by the test's chip model).
#[cfg(not(target_os = "none"))]
pub static HOST_CYC: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
#[cfg(not(target_os = "none"))]
fn host_cyc() -> u32 {
    HOST_CYC.load(core::sync::atomic::Ordering::Relaxed)
}

/// Raw disk access for the session: FAT / directory blocks and the NOR
/// operations of the same chip.
pub trait Disk {
    fn bd(&mut self) -> &mut dyn BlockDevice;
    fn nor(&mut self) -> &mut dyn Nor;
}

impl<T: BlockDevice + Nor> Disk for T {
    fn bd(&mut self) -> &mut dyn BlockDevice {
        self
    }
    fn nor(&mut self) -> &mut dyn Nor {
        self
    }
}

// ------------------------------------------------------------- API

/// A session is between `begin` and its end (Prep .. Stop).
#[inline(always)]
pub fn active() -> bool {
    matches!(job().state, State::Prep | State::Ready | State::Run | State::Stop)
}

/// The session holds the SPI disk (and calbackup::scratch, csvlog's FIFO):
/// other on-device writers refuse or wait.
#[inline(always)]
pub fn holds_disk() -> bool {
    active()
}

/// The logger's ring is the session's (LOG:DATA? answers an empty block).
#[inline(always)]
pub fn owns_ring() -> bool {
    matches!(job().state, State::Run | State::Stop)
}

/// Preparing (allocating / erasing): the panel shows the progress.
pub fn preparing() -> bool {
    matches!(job().state, State::Prep | State::Ready)
}

/// The logger's next-session settings (main.rs, every pass: LOG:OSR,
/// LOG:CHOP, LOG:SRATe) for the Record page's estimate: osr | chop << 8.
static CFG: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(3 | 1 << 8);
static REQ_HZ: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(4800);

#[inline(always)]
pub fn set_cfg(osr: u8, chop: bool, req_hz: u32) {
    CFG.store(osr as u32 | (chop as u32) << 8, core::sync::atomic::Ordering::Relaxed);
    REQ_HZ.store(req_hz, core::sync::atomic::Ordering::Relaxed);
}

/// (osr, chop, requested AD2 S/s) of `set_cfg`.
pub fn cfg() -> (u8, bool, u32) {
    let c = CFG.load(core::sync::atomic::Ordering::Relaxed);
    (c as u8, c & 1 << 8 != 0, REQ_HZ.load(core::sync::atomic::Ordering::Relaxed))
}

/// Bytes the next session would plan for `dur_s` at `hz` (0 = all free).
pub fn plan_bytes(dur_s: u32, hz: u32, ad2: bool) -> u32 {
    if dur_s == 0 {
        return u32::MAX;
    }
    let bps = if ad2 { BPS_AD2 } else { BPS_AD1 };
    (dur_s as u64 * hz as u64 * bps as u64 + HDR_ALLOW as u64).min(u32::MAX as u64 - 1) as u32
}

/// Start a session (main.rs, LOG:STATe ON with LOG:DEST DISK or the Record
/// page): `bytes` = `plan_bytes` (the free space bounds it), `dur_s` = the
/// recording length in device time (0 = until stopped / full). Err: why
/// not (an SCPI error text).
pub fn begin(bytes: u32, dur_s: u32) -> Result<(), &'static str> {
    if active() {
        return Err("Settings conflict");
    }
    if crate::csvlog::active() || crate::screenshot::busy() || crate::upload::busy() {
        return Err("Mass storage busy");
    }
    let j = job();
    *j = JOB0;
    j.state = State::Prep;
    j.ph = Ph::Bpb;
    j.dur_us = dur_s as u64 * 1_000_000;
    j.need = bytes.div_ceil(BLK as u32);
    Ok(())
}

/// The session just begun (Prep) measures the chip's page program time at
/// the end of Prep (`tpp_us`): main.rs asks for it when the session's rate
/// needs more than the datasheet's worst case allows (76.8 kS/s,
/// decision 2026-10-22 #9), and caps the rate when the chip is too slow.
pub fn set_probe() {
    let j = job();
    if j.state == State::Prep {
        j.probe = true;
        TPP.store(0, core::sync::atomic::Ordering::Relaxed);
    }
}

/// The last probe's page program times: (mean, max) in us, (0, 0) = none
/// measured (no probe, or it failed). Programs of 256 zero bytes into an
/// erased page, timed from the end of the clock-out to BUSY clear (polled).
pub fn tpp_us() -> (u32, u32) {
    let v = TPP.load(core::sync::atomic::Ordering::Relaxed);
    (v & 0xFFFF, v >> 16)
}

static TPP: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Pages timed by the probe.
const PROBE_PAGES: u32 = 4;

/// Mean / max page program (us) under which 76.8 kS/s goes to the disk: at
/// the coder's worst case (2.93 B/sample, 225 KB/s = 880 pages/s) a page
/// may take 1.14 ms in all, ~0.2 ms of it clocking out and ~0.1 ms of the
/// pass: 0.8 ms of tPP on average; a single slow page is absorbed by the
/// 8 KiB FIFO (32 pages), so the max only has to stay under 1.5 ms. The
/// W25Q128JV: 0.4 ms typ / 3 ms max (docs/compact-log-format.md "Rate
/// limits").
pub const TPP_OK_MEAN_US: u32 = 800;
pub const TPP_OK_MAX_US: u32 = 1500;

/// The probe's verdict: the chip is fast enough for 76.8 kS/s.
pub fn tpp_ok() -> bool {
    let (mean, max) = tpp_us();
    mean != 0 && mean <= TPP_OK_MEAN_US && max <= TPP_OK_MAX_US
}

/// Ask the session to end (LOG:STATe OFF, a tap, the Record page): in Prep
/// / Ready it is abandoned (the file, if made, removed); in Run the next
/// `poll` returns `Ev::Stop`.
pub fn request_stop() {
    let j = job();
    match j.state {
        State::Run => j.stop_req = true,
        State::Prep | State::Ready => j.stop_req = true,
        _ => {}
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Status {
    pub state: State,
    pub file: Option<u16>,
    /// Bytes allocated, bytes written.
    pub alloc: u32,
    pub bytes: u32,
    pub rows: u32,
    /// Prep progress, %.
    pub pct: u8,
    /// FIFO high water, bytes; pages / partial programs; chunks.
    pub hw: u32,
    pub programs: u32,
    pub chunks: u32,
}

/// The logger panel's disk rows (main.rs, while logging).
#[cfg(target_os = "none")]
pub fn panel() -> Option<crate::dialog::LogDisk> {
    let j = job();
    active().then(|| crate::dialog::LogDisk {
        prep: preparing(),
        pct: j.pct,
        file: if j.has_file { j.n } else { 0 },
        kb: j.enc.wr / 1024,
    })
}

pub fn status() -> Status {
    let j = job();
    Status {
        state: j.state,
        file: j.has_file.then_some(j.n),
        alloc: j.enc.cap.max(j.ncl * BLK as u32),
        bytes: j.enc.wr,
        rows: j.enc.rows,
        pct: j.pct,
        hw: j.hw,
        programs: j.enc.programs,
        chunks: j.enc.chunks,
    }
}

/// The 8.3 name of the session's file while it is being written (vdisk
/// refresh leaves it out).
pub fn recording_name() -> Option<[u8; 11]> {
    let j = job();
    (active() && j.has_file).then(|| crate::calbackup::numbered_name(b"FAST", j.n, b"DLG"))
}

#[cfg(target_os = "none")]
static mut DISK: Option<crate::spiflash::SpiFlash> = None;

#[cfg(target_os = "none")]
fn disk() -> &'static mut crate::spiflash::SpiFlash {
    let d = unsafe { &mut *core::ptr::addr_of_mut!(DISK) };
    d.get_or_insert_with(crate::spiflash::SpiFlash::new)
}

/// One slice (main loop, every pass). `logger_on`: the logger session
/// runs (a session that ended behind the session's back, e.g. a function
/// change, ends the run); `drain`: Logger::drain.
#[cfg(target_os = "none")]
#[inline(always)]
pub fn poll(logger_on: bool, drain: &mut dyn FnMut(&mut [u8]) -> usize) -> Ev {
    if !active() {
        return Ev::None;
    }
    poll_on(disk(), logger_on, drain)
}

/// `poll` on any disk (host tests).
pub fn poll_on(d: &mut dyn Disk, logger_on: bool, drain: &mut dyn FnMut(&mut [u8]) -> usize) -> Ev {
    let j = job();
    match j.ph {
        Ph::None => Ev::None,
        Ph::Run => j.run(d, logger_on, drain),
        _ => prep::step(j, d, drain),
    }
}

/// main.rs after `Ev::Start` and a started logger session: the file header
/// goes in, the run begins. `p_us` = the nominal period.
pub fn run(h: &Head) {
    prep::run_begin(job(), h);
}

/// main.rs after `Ev::Start` when the logger could not start (the function
/// changed meanwhile): the session ends, the file is removed.
pub fn not_started() {
    let j = job();
    if j.state == State::Ready {
        j.stop_req = true;
        j.ph = Ph::Unlink;
        j.state = State::Stop;
        j.end_state = State::Error;
    }
}

/// The outcome of the last session, once it ended (cleared by `begin`).
pub fn ended() -> Option<State> {
    let s = job().state;
    matches!(s, State::Done | State::Full | State::NoDisk | State::Error).then_some(s)
}

/// Power-off / reset paths (app::finish_disk_writes): drive a session to a
/// consistent end, at most `max_ms` (the logger is already stopped or is
/// stopped by the caller first).
#[cfg(target_os = "none")]
pub fn finish_blocking(max_ms: u32) {
    if !active() {
        return;
    }
    request_stop();
    let max = max_ms.min(20_000) * (crate::clock::HCLK_HZ / 1000);
    let t0 = cyc();
    let mut nothing = |_: &mut [u8]| 0usize;
    while active() && cyc().wrapping_sub(t0) < max {
        let _ = poll(false, &mut nothing);
        crate::wdt::kick_if_feeding();
    }
}

// ------------------------------------------------------------- run (hot)

impl Job {
    #[inline(never)]
    fn run(&mut self, d: &mut dyn Disk, logger_on: bool, drain: &mut dyn FnMut(&mut [u8]) -> usize) -> Ev {
        let fifo = &mut crate::csvlog::lend_fifo()[..];
        let t0 = cyc();
        let mut buf = [0u8; BATCH * 12];
        let mut stop = None;
        loop {
            if !self.pump(d.nor(), fifo) {
                stop = Some(State::Error);
                break;
            }
            if self.ended || self.stop_req || !logger_on {
                break;
            }
            // A batch, or one record at a time near the file's end.
            let per = if self.enc.room(BATCH as u32) {
                BATCH
            } else if self.enc.room(1) {
                1
            } else {
                if self.enc.full() {
                    stop = Some(State::Full);
                }
                break; // else: the FIFO is full, the pages go out first
            };
            let n = drain(&mut buf[..per * 12]) / 12;
            if n == 0 {
                break;
            }
            for r in buf[..n * 12].chunks_exact(12) {
                let ts = u32::from_le_bytes([r[4], r[5], r[6], r[7]]);
                let raw = i32::from_le_bytes([r[8], r[9], r[10], r[11]]);
                if self.ended {
                    self.dropped_end += 1;
                    continue;
                }
                self.enc.add(fifo, ts, raw);
                if self.dur_us > 0 && self.enc.t_last - self.enc.t_first >= self.dur_us {
                    self.ended = true;
                }
            }
            let used = self.enc.wr - self.enc.done;
            if used > self.hw {
                self.hw = used;
            }
            if cyc().wrapping_sub(t0) >= RUN_BUDGET_CYC {
                break;
            }
        }
        let st = stop.or(if self.stop_req || self.ended || !logger_on { Some(State::Done) } else { None });
        match st {
            Some(s) => {
                self.end_state = s;
                self.ph = Ph::Drain;
                self.state = State::Stop;
                Ev::Stop
            }
            None => Ev::None,
        }
    }

    /// One chip operation if the chip is idle; false: the chip stayed busy
    /// past OP_TIMEOUT_CYC.
    #[inline(always)]
    fn pump(&mut self, nor: &mut dyn Nor, fifo: &[u8]) -> bool {
        match self.enc.pump(fifo, nor) {
            Pump::Busy => {
                let now = cyc();
                let since = *self.busy_since.get_or_insert(now);
                now.wrapping_sub(since) < OP_TIMEOUT_CYC
            }
            _ => {
                self.busy_since = None;
                true
            }
        }
    }
}

/// Prep and Stop: rarely run (`.cold`, link-native.x `fastlog4prep`).
/// Never runs while the AD2 stream ISR does (Prep is before the logger
/// session, Stop after it).
mod prep {
    use super::*;
    use crate::calbackup::{numbered_name, scratch};
    use crate::fatren::{le16, le32, parse_bpb, sfn_is, verify};

    /// The session ends with `s`.
    fn end(j: &mut Job, s: State) -> Ev {
        j.state = s;
        j.ph = Ph::None;
        if j.wrote {
            #[cfg(feature = "vdisk")]
            crate::dlog::set_stale();
        }
        Ev::None
    }

    #[inline(never)]
    pub(super) fn step(j: &mut Job, d: &mut dyn Disk, drain: &mut dyn FnMut(&mut [u8]) -> usize) -> Ev {
        // Abandoned before the run: before Chain nothing is linked (erased
        // free clusters); Chain / Entry run to the end, then the file is
        // removed again (Unlink).
        if j.stop_req && matches!(j.ph, Ph::Bpb | Ph::Scan | Ph::Alloc) {
            return end(j, State::Done);
        }
        if j.stop_req && matches!(j.ph, Ph::Erase | Ph::Probe) {
            if j.erasing && d.nor().busy() {
                return Ev::None;
            }
            return end(j, State::Done);
        }
        if j.stop_req && j.ph == Ph::Wait {
            j.ph = Ph::Unlink;
            j.state = State::Stop;
            j.end_state = State::Done;
        }
        let r = match j.ph {
            Ph::Bpb => bpb(j, d.bd()),
            Ph::Scan => scan(j, d.bd()),
            Ph::Alloc => alloc(j, d.bd()),
            Ph::Erase => erase(j, d),
            Ph::Probe => probe(j, d),
            Ph::Chain => chain(j, d.bd(), false),
            Ph::Entry => entry(j, d.bd()),
            Ph::Wait => {
                if !j.ev_sent {
                    j.ev_sent = true;
                    return Ev::Start;
                }
                Ok(())
            }
            Ph::Drain => {
                let fifo = &mut crate::csvlog::lend_fifo()[..];
                if !j.pump(d.nor(), fifo) {
                    return end(j, State::Error);
                }
                // The ring's rest (the logger is stopped: nothing new comes);
                // a full FIFO waits for the pages to go out.
                let mut buf = [0u8; BATCH * 12];
                let mut empty = false;
                for _ in 0..64 {
                    let per = if j.enc.room(BATCH as u32) { BATCH } else { 1 };
                    if j.ended || j.enc.full() || !j.enc.room(1) {
                        break;
                    }
                    let n = drain(&mut buf[..per * 12]) / 12;
                    if n == 0 {
                        empty = true;
                        break;
                    }
                    for r in buf[..n * 12].chunks_exact(12) {
                        let ts = u32::from_le_bytes([r[4], r[5], r[6], r[7]]);
                        let raw = i32::from_le_bytes([r[8], r[9], r[10], r[11]]);
                        if j.ended {
                            j.dropped_end += 1;
                            continue;
                        }
                        j.enc.add(fifo, ts, raw);
                        if j.dur_us > 0 && j.enc.t_last - j.enc.t_first >= j.dur_us {
                            j.ended = true;
                        }
                    }
                }
                if empty || j.ended || j.enc.full() {
                    if j.enc.full() && j.end_state == State::Done {
                        j.end_state = State::Full;
                    }
                    j.enc.finish(fifo);
                    j.ph = Ph::Flush;
                }
                Ok(())
            }
            Ph::Flush => {
                let fifo = &mut crate::csvlog::lend_fifo()[..];
                if j.enc.is_open() {
                    j.enc.finish(fifo); // the close waited for queue room
                }
                if !j.pump(d.nor(), fifo) {
                    return end(j, State::Error);
                }
                if j.enc.flushed() && !d.nor().busy() {
                    j.ph = Ph::Size;
                }
                Ok(())
            }
            Ph::Size => size(j, d.bd()),
            Ph::Trim => chain(j, d.bd(), true),
            Ph::Unlink => unlink(j, d),
            Ph::None | Ph::Run => Ok(()),
        };
        match r {
            Ok(()) => Ev::None,
            Err(s) => end(j, s),
        }
    }

    fn vol(j: &Job) -> crate::fatren::Vol {
        j.v.unwrap_or_else(|| unreachable!())
    }

    fn bpb(j: &mut Job, d: &mut dyn BlockDevice) -> Result<(), State> {
        if !d.present() || d.block_size() as usize != BLK {
            return Err(State::NoDisk);
        }
        let buf = &mut scratch()[0];
        if !d.read_block(0, buf) {
            return Err(State::Error);
        }
        let v = parse_bpb(&buf[..], d.block_count()).ok_or(State::NoDisk)?;
        let v = match crate::fatdir::ensure(d, &v, &crate::fatdir::LOGS, scratch()) {
            Ok(v) => v,
            Err(crate::fatdir::Err::Io) => return Err(State::Error),
            Err(_) => return Err(State::Full),
        };
        if v.spc != 1 {
            return Err(State::NoDisk); // 1 cluster = 1 block on this disk
        }
        j.v = Some(v);
        j.cur = NONE;
        j.i = 0;
        j.ph = Ph::Scan;
        Ok(())
    }

    /// One directory block (root, then LOGS/): FAST numbers, a free slot.
    fn scan(j: &mut Job, d: &mut dyn BlockDevice) -> Result<(), State> {
        let v = vol(j);
        let (lba, sub, base, lim) = crate::fatdir::seq(&v, j.i);
        let buf = &mut scratch()[0];
        if !d.read_block(lba, buf) {
            return Err(State::Error);
        }
        let mut end = false;
        for k in 0..PER {
            let idx = base + k;
            if idx >= lim {
                end = true;
                break;
            }
            let e = &buf[(k * 32) as usize..(k * 32 + 32) as usize];
            if e[0] == 0x00 || e[0] == 0xE5 {
                if sub && j.slot == NONE {
                    j.slot = idx;
                }
                if e[0] == 0x00 {
                    end = true;
                    break;
                }
                continue;
            }
            if e[11] & 0x3F == 0x0F || !e[4..7].iter().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let n = (e[4] - b'0') as u16 * 100 + (e[5] - b'0') as u16 * 10 + (e[6] - b'0') as u16;
            if sfn_is(e, &numbered_name(b"FAST", n, b"DLG")) {
                j.taken[(n / 32) as usize] |= 1 << (n % 32);
            }
        }
        j.i += 1;
        if end {
            j.i = if sub { crate::fatdir::seq_blocks(&v) } else { v.root_blocks };
        }
        if j.i < crate::fatdir::seq_blocks(&v) {
            return Ok(());
        }
        let n = (0..1000u16).find(|&n| j.taken[(n / 32) as usize] & (1 << (n % 32)) == 0).ok_or(State::Full)?;
        if j.slot == NONE {
            return Err(State::Full);
        }
        j.n = n;
        j.alloc_c = 2;
        j.ph = Ph::Alloc;
        Ok(())
    }

    /// FAT 1 entry `c` through the one-block cache in scratch[1].
    fn fat_get(j: &mut Job, d: &mut dyn BlockDevice, c: u32) -> Option<u32> {
        let v = vol(j);
        let fat = &mut scratch()[1];
        let fb = c / FPER;
        if fb >= v.fat_blocks {
            return None;
        }
        if fb != j.cur {
            if !d.read_block(v.fat_start + fb, fat) {
                j.cur = NONE;
                return None;
            }
            j.cur = fb;
        }
        Some(le16(fat, ((c % FPER) * 2) as usize))
    }

    /// One FAT 1 block of free clusters into the extents.
    fn alloc(j: &mut Job, d: &mut dyn BlockDevice) -> Result<(), State> {
        let v = vol(j);
        let max = v.clusters + 1;
        let fb = j.alloc_c / FPER;
        while j.alloc_c <= max && j.alloc_c / FPER == fb && j.ncl < j.need {
            let c = j.alloc_c;
            let e = fat_get(j, d, c).ok_or(State::Error)?;
            j.alloc_c += 1;
            if e != 0 {
                continue;
            }
            match j.next {
                0 => {}
                n if j.ext[n - 1].0 + j.ext[n - 1].1 == c => {
                    j.ext[n - 1].1 += 1;
                    j.ncl += 1;
                    continue;
                }
                _ => {}
            }
            if j.next == NEXT {
                j.alloc_c = max + 1; // fragmented: this much
                break;
            }
            j.ext[j.next] = (c, 1);
            j.next += 1;
            j.ncl += 1;
        }
        if j.ncl < j.need && j.alloc_c <= max {
            return Ok(());
        }
        // Room for at least the header and a few chunks.
        if j.ncl < 2 {
            return Err(State::Full);
        }
        j.ei = 0;
        j.erasing = false;
        j.ph = Ph::Erase;
        Ok(())
    }

    /// Cluster number of file cluster `i`.
    fn cl_of(j: &Job, mut i: u32) -> u32 {
        for &(c, n) in &j.ext[..j.next] {
            if i < n {
                return c + i;
            }
            i -= n;
        }
        0
    }

    /// The page-program probe (`set_probe`): PROBE_PAGES programs of zeros at
    /// the start of the allocation's last cluster (erased by now, not yet
    /// linked: a power cut leaves a dirty free cluster, the next Prep's
    /// blank check erases it), each timed to BUSY clear by polling (Prep
    /// may block a pass for a few ms), then that cluster erased again (one
    /// SE4K, waited for like the others). TPP = (mean, max) us.
    fn probe(j: &mut Job, d: &mut dyn Disk) -> Result<(), State> {
        let v = vol(j);
        let cb = BLK as u32;
        let a = v.data_start * cb + (cl_of(j, j.ncl - 1) - 2) * cb;
        if j.i == 0 {
            let zeros = [0u8; 256];
            let (mut sum, mut max) = (0u32, 0u32);
            for k in 0..PROBE_PAGES {
                d.nor().program(a + k * 256, &zeros);
                let t0 = cyc();
                let mut n = 0u32;
                while d.nor().busy() {
                    n += 1;
                    if cyc().wrapping_sub(t0) > crate::clock::HCLK_HZ / 100 || n > 1_000_000 {
                        return Err(State::Error); // 10 ms: tPP max is 3 ms
                    }
                }
                let us = cyc().wrapping_sub(t0) / (crate::clock::HCLK_HZ / 1_000_000);
                sum += us;
                max = max.max(us);
            }
            let mean = (sum / PROBE_PAGES).clamp(1, 0xFFFF);
            TPP.store(mean | max.min(0xFFFF) << 16, core::sync::atomic::Ordering::Relaxed);
            d.nor().erase(a, false);
            j.wrote = true;
            j.erasing = true;
            j.t_op = cyc();
            j.i = 1;
            return Ok(());
        }
        if d.nor().busy() {
            if cyc().wrapping_sub(j.t_op) > OP_TIMEOUT_CYC {
                return Err(State::Error);
            }
            return Ok(());
        }
        j.erasing = false;
        j.ph = Ph::Chain;
        Ok(())
    }

    /// Clusters `i..` that are consecutive in the chain (in one extent).
    fn run_left(j: &Job, mut i: u32) -> u32 {
        for &(_, n) in &j.ext[..j.next] {
            if i < n {
                return n - i;
            }
            i -= n;
        }
        0
    }

    /// One blank check or erase step.
    fn erase(j: &mut Job, d: &mut dyn Disk) -> Result<(), State> {
        let v = vol(j);
        let cb = BLK as u32;
        let base = v.data_start * cb; // byte address of cluster 2
        if j.erasing {
            if d.nor().busy() {
                if cyc().wrapping_sub(j.t_op) > OP_TIMEOUT_CYC {
                    return Err(State::Error);
                }
                return Ok(());
            }
            j.erasing = false;
            j.ei += j.group.max(1);
            j.group = 0;
            j.dirty = false;
        }
        if j.ei >= j.ncl {
            j.pct = 100;
            j.fat_k = 0;
            j.fat_fb = 0;
            j.ph = if j.probe && j.ncl > 0 { Ph::Probe } else { Ph::Chain };
            j.i = 0;
            return Ok(());
        }
        j.pct = (j.ei as u64 * 100 / j.ncl as u64) as u8;
        // A group = an aligned 64 KiB block of 16 clusters, all ours.
        if j.group == 0 {
            let c = cl_of(j, j.ei);
            let a = base + (c - 2) * cb;
            j.group = if a % 0x1_0000 == 0 && run_left(j, j.ei) >= 16 { 16 } else { 1 };
            j.dirty = false;
            j.i = 0; // clusters of the group checked
        }
        if j.i < j.group {
            // Blank check of one cluster (a 4 KiB read).
            let c = cl_of(j, j.ei + j.i);
            let a = base + (c - 2) * cb;
            let buf = &mut scratch()[2];
            if !d.nor().read(a, &mut buf[..]) {
                return Err(State::Error);
            }
            if buf.iter().any(|&b| b != 0xFF) {
                // One dirty sector: erase it (a group: the whole block).
                j.dirty = true;
                j.i = j.group;
            } else {
                j.i += 1;
            }
            if j.i < j.group {
                return Ok(());
            }
        }
        if !j.dirty {
            j.ei += j.group;
            j.group = 0;
            return Ok(());
        }
        let c = cl_of(j, j.ei);
        d.nor().erase(base + (c - 2) * cb, j.group == 16);
        j.wrote = true;
        j.erasing = true;
        j.t_op = cyc();
        Ok(())
    }

    /// FAT copies: the chain (`trim` false: every allocated cluster, each to
    /// the next, the last EOC; `trim`: clusters past `used` freed, the last
    /// used one EOC). One block per pass: read (check), modify, write.
    fn chain(j: &mut Job, d: &mut dyn BlockDevice, trim: bool) -> Result<(), State> {
        let v = vol(j);
        // The FAT blocks holding one of ours, in order: from `fat_fb` on.
        let touched = |j: &Job, fb: u32| j.ext[..j.next].iter().any(|&(c, n)| c / FPER <= fb && fb <= (c + n - 1) / FPER);
        while j.fat_fb < v.fat_blocks && !touched(j, j.fat_fb) {
            j.fat_fb += 1;
        }
        if j.fat_fb >= v.fat_blocks {
            j.fat_k += 1;
            j.fat_fb = 0;
            if j.fat_k == v.nfats {
                j.cur = NONE;
                if trim {
                    return finish_ok(j);
                }
                j.ph = Ph::Entry;
            }
            return Ok(());
        }
        let fb = j.fat_fb;
        let lba = v.fat_start + j.fat_k * v.fat_blocks + fb;
        let [buf, _, tmp] = scratch();
        if !d.read_block(lba, buf) {
            return Err(State::Error);
        }
        let first = j.fat_k == 0;
        let mut k = 0u32; // file cluster index
        for x in 0..j.next {
            let (c0, n) = j.ext[x];
            for i in 0..n {
                let c = c0 + i;
                let fi = k + i;
                if c / FPER != fb {
                    continue;
                }
                let o = ((c % FPER) * 2) as usize;
                let old = le16(&buf[..], o);
                let want = if trim {
                    if fi + 1 < j.used {
                        cl_of(j, fi + 1)
                    } else if fi + 1 == j.used {
                        0xFFFF
                    } else {
                        0
                    }
                } else if fi + 1 < j.ncl {
                    cl_of(j, fi + 1)
                } else {
                    0xFFFF
                };
                // FAT 1 must hold what we left there (free before the chain;
                // our chain before the trim).
                let expect = if trim {
                    if fi + 1 < j.ncl { cl_of(j, fi + 1) } else { 0xFFFF }
                } else {
                    0
                };
                if first && old != expect {
                    return Err(State::Error);
                }
                buf[o..o + 2].copy_from_slice(&(want as u16).to_le_bytes());
            }
            k += n;
        }
        if !(d.write_block(lba, &buf[..]) && verify(d, lba, buf, tmp)) {
            return Err(State::Error);
        }
        j.wrote = true;
        j.fat_fb += 1;
        Ok(())
    }

    /// The LOGS/ entry, size = the allocation (Entry) .
    fn entry(j: &mut Job, d: &mut dyn BlockDevice) -> Result<(), State> {
        let v = vol(j);
        let lba = v.dlba(j.slot / PER);
        let [buf, _, tmp] = scratch();
        if !d.read_block(lba, buf) {
            return Err(State::Error);
        }
        let o = ((j.slot % PER) * 32) as usize;
        let e = &mut buf[o..o + 32];
        if e[0] != 0x00 && e[0] != 0xE5 {
            return Err(State::Error); // taken since the scan
        }
        e.fill(0);
        e[..11].copy_from_slice(&numbered_name(b"FAST", j.n, b"DLG"));
        e[11] = 0x20;
        for x in [0x10, 0x12, 0x18] {
            e[x..x + 2].copy_from_slice(&DATE.to_le_bytes());
        }
        e[0x1A..0x1C].copy_from_slice(&(j.ext[0].0 as u16).to_le_bytes());
        e[0x1C..0x20].copy_from_slice(&(j.ncl * BLK as u32).to_le_bytes());
        if !(d.write_block(lba, &buf[..]) && verify(d, lba, buf, tmp)) {
            return Err(State::Error);
        }
        j.wrote = true;
        j.has_file = true;
        j.ph = Ph::Wait;
        j.state = State::Ready;
        j.ev_sent = false;
        Ok(())
    }

    /// main.rs: the logger runs; the file header into the FIFO.
    #[inline(never)]
    pub(super) fn run_begin(j: &mut Job, h: &Head) {
        if j.state != State::Ready {
            return;
        }
        let v = vol(j);
        let fifo = &mut crate::csvlog::lend_fifo()[..];
        j.enc = FastEnc::new(&j.ext[..j.next], v.data_start * BLK as u32, BLK as u32, fifo.len(), h.par.p_us());
        let mut f = [0u8; crate::dlog::FHDR + 512];
        let n = crate::dlog::file_header(crate::dlog::KIND_FAST, h.unit, &h.text[..h.text.len().min(512)], &mut f);
        j.enc.put_raw(fifo, &f[..n]);
        j.enc.put_raw(fifo, &h.par.bytes());
        j.ph = Ph::Run;
        j.state = State::Run;
    }

    /// Stop: the entry's size = the bytes written (checked: still ours).
    fn size(j: &mut Job, d: &mut dyn BlockDevice) -> Result<(), State> {
        let v = vol(j);
        let lba = v.dlba(j.slot / PER);
        let [buf, _, tmp] = scratch();
        if !d.read_block(lba, buf) {
            return Err(State::Error);
        }
        let o = ((j.slot % PER) * 32) as usize;
        let e = &mut buf[o..o + 32];
        if e[..11] != numbered_name(b"FAST", j.n, b"DLG")[..] || le16(e, 0x1A) != j.ext[0].0 || le32(e, 0x1C) != j.ncl * BLK as u32 {
            return Err(State::Error);
        }
        let size = j.enc.wr;
        e[0x1C..0x20].copy_from_slice(&size.to_le_bytes());
        if !(d.write_block(lba, &buf[..]) && verify(d, lba, buf, tmp)) {
            return Err(State::Error);
        }
        j.used = size.div_ceil(BLK as u32).max(1);
        if j.used >= j.ncl {
            return finish_ok(j);
        }
        j.fat_k = 0;
        j.fat_fb = 0;
        j.ph = Ph::Trim;
        Ok(())
    }

    /// Abandoned after the entry (Ready, or the logger did not start): the
    /// entry deleted, then the chain freed (a cut between: lost clusters).
    fn unlink(j: &mut Job, d: &mut dyn Disk) -> Result<(), State> {
        let v = vol(j);
        let bd = d.bd();
        let lba = v.dlba(j.slot / PER);
        let [buf, fat1, tmp] = scratch();
        if !bd.read_block(lba, buf) {
            return Err(State::Error);
        }
        let o = ((j.slot % PER) * 32) as usize;
        if buf[o..o + 11] == numbered_name(b"FAST", j.n, b"DLG")[..] {
            buf[o] = 0xE5;
            if !(bd.write_block(lba, &buf[..]) && verify(bd, lba, buf, tmp)) {
                return Err(State::Error);
            }
            j.wrote = true;
            let (ok, _) = crate::fatren::free_chain(bd, &v, j.ext[0].0, fat1, buf, tmp);
            if !ok {
                return Err(State::Error);
            }
        }
        let s = j.end_state;
        Err(s) // ends the session with that state
    }

    fn finish_ok(j: &mut Job) -> Result<(), State> {
        let s = j.end_state;
        Err(if s == State::Error { State::Error } else { s })
    }
}
