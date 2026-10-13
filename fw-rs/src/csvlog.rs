//! CSV recording of DMM readings to the SPI-flash USB disk (TODO "CSV-to-disk
//! recording", FR-35, UF-32) and the file-gallery backend (FR-16).
//!
//! File: the next free `LOG%03u.CSV` in the root (lowest unused number, as
//! the screenshots; the FAT code has no subdirectory support). Content, CRLF
//! line ends, '.' decimal point, ASCII only:
//!   # Alientek DM40A, fw-rs 0.1.0, function DC V, range AUTO, started uptime 12.345 s
//!   # time_s = seconds since the start of this recording (no real-time clock); empty value = overload
//!   time_s,value,unit,flags
//!   0.250,1.23456,V,
//!   0.750,,V,OL
//! There is no RTC in fw-rs (the dates in calbackup / screenshot are a
//! constant for the same reason), so "started" is the uptime and time_s counts
//! from `start`. `value` is the caller's (SI) value with 6 significant digits,
//! plain decimal for 1e-3 <= |v| < 1e6, else d.dddddE+xx; empty for overload
//! (flag OL) or a non-finite value. `flags`: space-separated OL REL HOLD HI LO,
//! plus GAP on the first row after rows were dropped (buffer full).
//! Which value (app.rs `reading`): the UNSMOOTHED reading with REL applied
//! (`meter::Update::raw`), one row per Dmm reading. The FR-05 display
//! smoothing is a display preference, like for MIN / MAX / AVG (meter.rs
//! header): a log of a moving average would hide the spread and lag steps,
//! and the time column would no longer be the time of the value.
//!
//! Non-blocking, like screenshot.rs: `start` / `push` / `stop` only touch RAM;
//! `poll` (every main-loop pass) does one bounded slice of a flush (one block
//! read, one Alloc FAT block, one `write_step` slice of <= ~2 ms of a data
//! block, or one whole FAT / root block write).
//! FAT and root blocks are written with `write_block` (erase + programs +
//! read back in one call, <= ~0.45 s, watchdog kicked in spiflash.rs), never
//! in `write_step` slices: a FAT / root block must not be left erased or half
//! programmed across a main-loop pass, where a power-off, a reset or a host
//! READ could see it (review 258c833 H1, as screenshot.rs).
//!
//! RAM: rows are formatted into FIFO (8 KiB ring, native) as they arrive; the
//! file's last, partially filled 4 KiB block lives in TAIL. A flush ("commit")
//! moves the FIFO into TAIL block by block and writes it, in this order:
//!   Data  TAIL to its block (`write_step`); a full TAIL moves on to the next
//!         block, a new cluster when the block starts one (Alloc: first free
//!         FAT 1 entry from the last cluster on, one FAT block per pass);
//!   Fat   the new clusters chained + EOC and the old last cluster linked to
//!         the first new one, in every FAT copy (FAT 1 first; per copy the
//!         block holding the link last);
//!   Dir   the root entry: start cluster and size (= every byte written).
//! Data -> FAT -> directory, as screenshot.rs / calbackup: the directory size
//! only ever covers written, linked data.
//!
//! Append without erase: unused bytes of TAIL are 0xFF, so rewriting the same
//! block with more rows only clears bits; spiflash.rs `write_step` then
//! programs the changed pages without the 4 KiB erase. Rows already committed
//! are therefore never inside an erase: a power cut while the tail block is
//! written cannot lose them. Only the first write to a new cluster erases it
//! (it held nothing of ours). The FAT blocks (new entries go 0 -> non-zero)
//! and the root block (the size field) need an erase per commit, as every
//! FAT writer; the second FAT copy is written after the first.
//!
//! When it commits: right after `start` (the file appears with its header),
//! when TAIL fills (one block; on this disk 1 cluster = 1 block, so the chain
//! grows by one cluster per commit), every COMMIT_MS with new rows, when FIFO
//! is 3/4 full, and at `stop`. Root-block wear: one erase per commit, i.e. at
//! ~30 bytes per row one per 4 KiB (~2 min at 1 reading/s, ~14 s at 10/s) or
//! per minute, whichever is first: ~1500..6000 a day of continuous logging
//! against the W25Q128's 100k cycles. FAT block: one erase per copy per
//! commit that adds a cluster. Power-loss window: rows not yet committed (at
//! most COMMIT_MS or one block of rows).
//!
//! Power cut, worst case (tools/fatren-test, cut after every block write):
//!   during Data           nothing visible changes (lost clusters at worst:
//!                         none, the new cluster is not linked yet);
//!   between Fat and Dir   the chain is one cluster longer than the size in
//!                         the entry (fsck "cluster chain longer than file
//!                         size; truncating" = that cluster is freed), or the
//!                         FAT copies differ (FAT 1 ahead). Unavoidable with
//!                         two separately written blocks; no committed byte
//!                         is lost and no other file is touched;
//!   in an erase           of a FAT or root block: that block (FAT 1 / the
//!                         root block) is damaged, as for every FAT writer
//!                         including the screenshot and a PC.
//!
//! Sharing the disk (one writer at a time):
//!   * Host: for the whole recording session host WRITE(10)s are refused (NOT
//!     READY, msc.rs `LOG_SESSION`); host reads are served. Between commits
//!     the cached FAT / directory state here would otherwise go stale under a
//!     PC that writes, exactly the screenshot's reasoning but for hours. A
//!     commit does not start while a host write is in progress
//!     (`HOST_WRITING`, only possible right at `start`). MEDIUM_CHANGED
//!     (UNIT ATTENTION, screenshot.rs / msc.rs) is set after the first commit
//!     (the new file) and after the last one.
//!   * Screenshot: a commit never starts while `screenshot::busy()`; the
//!     screenshot waits before its FAT scan while a commit runs
//!     (msc.rs `LOG_FLUSHING`). Both use calbackup::scratch() only while they
//!     hold the disk. Rows keep going into FIFO meanwhile (~27 s at 10 rows/s
//!     before rows are dropped and the next row is flagged GAP).
//!   * Before every link the old last cluster must still be end-of-chain in
//!     FAT 1 and the new ones free; before every directory update the entry
//!     must still be this file with the last committed size. Otherwise the
//!     recording stops with Error and nothing more is written.
//!
//! Gallery (FR-16): `list_on` (S*.BMP, LOG*.CSV in the root, sizes, free
//! space) and `delete_on` (root entry + its long-name entries 0xE5 first, then
//! the chain freed in every FAT copy: a cut in between = lost clusters).
//! Synchronous (a list is ~4 block reads, a delete one root-block erase + FAT
//! programs, ~50..400 ms); refused while a screenshot or a commit holds the
//! disk, while a host write is in progress, and for the file being recorded.

use core::sync::atomic::Ordering;

use crate::calbackup;
use crate::fatren::{le16, le32, parse_bpb, Vol};
use crate::msc::{BlockDevice, Step};

const BLK: usize = 4096;
const PER: u32 = (BLK / 32) as u32;
const FPER: u32 = (BLK / 2) as u32;
/// Write / create date 2026-10-13, time 0 (no RTC), as screenshot / calbackup.
const DATE: u16 = (46 << 9) | (10 << 5) | 13;
const NONE: u32 = u32::MAX;
const STEP0: Step = Step { started: false, phase: 0, page: 0, dirty: 0, erase: false, t0: 0 };
/// New rows are committed at least this often (ms).
pub const COMMIT_MS: u64 = 60_000;
const MODEL: &str = "DM40A";
const FW: &str = match option_env!("CARGO_PKG_VERSION") {
    Some(v) => v,
    None => "dev",
};

/// Row FIFO (formatted text). The RAM test image has no SRAM to spare.
const FIFO: usize = if cfg!(any(feature = "native", not(target_os = "none"))) { 8192 } else { 1024 };
const _: () = assert!(FIFO.is_power_of_two());
/// Clusters a single commit may add (FIFO / block + the block in TAIL).
const MAXNEW: usize = FIFO / BLK + 2;

#[cfg_attr(target_os = "none", link_section = ".dmabuf")]
static mut FIFO_BUF: [u8; FIFO] = [0; FIFO];
#[cfg_attr(target_os = "none", link_section = ".dmabuf")]
static mut TAIL: [u8; BLK] = [0; BLK];

fn fifo() -> &'static mut [u8; FIFO] {
    unsafe { &mut *core::ptr::addr_of_mut!(FIFO_BUF) }
}
fn tail() -> &'static mut [u8; BLK] {
    unsafe { &mut *core::ptr::addr_of_mut!(TAIL) }
}

/// `push` flags.
pub const F_OL: u8 = 1;
pub const F_REL: u8 = 2;
pub const F_HOLD: u8 = 4;
pub const F_HI: u8 = 8;
pub const F_LO: u8 = 16;
const FLAG_TXT: [(u8, &str); 5] = [(F_OL, "OL"), (F_REL, "REL"), (F_HOLD, "HOLD"), (F_HI, "HI"), (F_LO, "LO")];

/// Recorder state (`status().state`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// Never started since boot.
    Idle,
    Recording,
    /// `stop` called, the last commit pending / running.
    Stopping,
    /// Stopped, every row committed.
    Done,
    /// Disk or root directory full: what fitted is committed, recording ended.
    Full,
    /// No disk / not the expected FAT16 volume.
    NoDisk,
    /// A read / write failed, or the file changed under us: recording ended,
    /// the file holds what was committed before (at worst lost clusters).
    Error,
}

#[derive(Clone, Copy, Debug)]
pub struct Status {
    pub state: State,
    /// LOG%03u.CSV number, once chosen (first commit).
    pub file: Option<u16>,
    /// Rows accepted (dropped ones not counted).
    pub rows: u32,
    /// File bytes accepted (header included), committed or pending.
    pub bytes: u32,
    /// File size on the disk (directory entry).
    pub committed: u32,
    /// Rows dropped because FIFO was full.
    pub dropped: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    None,
    Bpb,
    Root,
    Data,
    Alloc,
    Fat,
    Dir,
}

struct Job {
    state: State,
    phase: Phase,
    stopping: bool,
    /// Disk full met in this commit: finish after it.
    full: bool,
    v: Option<Vol>,
    // --- file
    n: u16,
    slot: u32,
    /// Directory entry written at least once.
    has_file: bool,
    first: u32,
    last: u32,
    nlinked: u32,
    /// Clusters allocated in this commit, not yet in the FAT.
    newcl: [u32; MAXNEW],
    nnew: usize,
    /// Next cluster to try (Alloc), clusters scanned so far.
    hint: u32,
    scanned: u32,
    /// TAIL holds file block `tb`, bytes 0..tfill.
    tb: u32,
    tfill: u32,
    size: u32,
    csize: u32,
    // --- commit in flight
    tx_left: u32,
    filled: bool,
    lba: u32,
    st: Step,
    /// FAT 1 block cached in scratch[1].
    cur: u32,
    fat_k: u32,
    fbs: [u32; MAXNEW + 1],
    nfb: usize,
    fbi: usize,
    i: u32,
    taken: [u32; 32],
    /// A block write was started this session (disk changed).
    wrote: bool,
    // --- rows
    fh: u32,
    ft: u32,
    rows: u32,
    bytes: u32,
    dropped: u32,
    gap: bool,
    t0_ms: u64,
    last_commit: u64,
    now: u64,
    unit: [u8; 12],
    ulen: usize,
}

const JOB0: Job = Job {
    state: State::Idle,
    phase: Phase::None,
    stopping: false,
    full: false,
    v: None,
    n: 0,
    slot: NONE,
    has_file: false,
    first: 0,
    last: 0,
    nlinked: 0,
    newcl: [0; MAXNEW],
    nnew: 0,
    hint: 2,
    scanned: 0,
    tb: 0,
    tfill: 0,
    size: 0,
    csize: 0,
    tx_left: 0,
    filled: false,
    lba: 0,
    st: STEP0,
    cur: NONE,
    fat_k: 0,
    fbs: [0; MAXNEW + 1],
    nfb: 0,
    fbi: 0,
    i: 0,
    taken: [0; 32],
    wrote: false,
    fh: 0,
    ft: 0,
    rows: 0,
    bytes: 0,
    dropped: 0,
    gap: false,
    t0_ms: 0,
    last_commit: 0,
    now: 0,
    unit: [0; 12],
    ulen: 0,
};

static mut JOB: Job = JOB0;
/// The last finished recording, not yet shown (`take_result`).
static mut RESULT: Option<Status> = None;
/// The device's own SpiFlash handle (created on first use).
static mut DISK: Option<crate::spiflash::SpiFlash> = None;

fn job() -> &'static mut Job {
    unsafe { &mut *core::ptr::addr_of_mut!(JOB) }
}

fn set_flags(session: bool, flushing: bool) {
    crate::msc::LOG_SESSION.store(session, Ordering::Relaxed);
    crate::msc::LOG_FLUSHING.store(flushing, Ordering::Relaxed);
}

// ------------------------------------------------------------- recording API

/// Start a recording: `func` / `range` go into the header (e.g. "DC V",
/// "AUTO"), `unit` into every row (e.g. "V"; non-ASCII is transliterated:
/// "\u{3a9}" -> "Ohm", "\u{b0}" -> "deg", "\u{b5}" -> "u"). `now_ms` = uptime,
/// the zero of time_s. False: a recording is already active, or a PC has
/// the disk mounted (`msc::host_mounted`, as screenshot.rs `start`: its
/// cached FAT would go stale under the commits; the caller can tell with
/// `msc::host_mounted()` and say "eject first").
pub fn start(func: &str, unit: &str, range: &str, now_ms: u64) -> bool {
    let j = job();
    if active() || crate::msc::host_mounted() {
        return false;
    }
    *j = JOB0;
    j.state = State::Recording;
    j.t0_ms = now_ms;
    j.last_commit = now_ms;
    j.now = now_ms;
    tail().fill(0xFF);
    j.ulen = ascii(unit, &mut j.unit);
    let mut h = [0u8; 384];
    let mut w = W { b: &mut h, n: 0 };
    w.s("# Alientek ");
    w.s(MODEL);
    w.s(", fw-rs ");
    w.s(FW);
    w.s(", function ");
    w.a(func);
    w.s(", range ");
    w.a(range);
    w.s(", started uptime ");
    w.ms(now_ms);
    w.s(" s\r\n# time_s = seconds since the start of this recording (no real-time clock); empty value = overload\r\n");
    w.s("time_s,value,unit,flags\r\n");
    let n = w.n;
    j.put(&h[..n]);
    set_flags(true, false);
    unsafe { *core::ptr::addr_of_mut!(RESULT) = None };
    true
}

/// One reading: `t_ms` = uptime (same clock as `start`), `value` in the
/// recording's unit, `flags` = F_* bits. Dropped (and counted) when FIFO is
/// full; the next row that fits carries GAP. Ignored unless recording.
pub fn push(t_ms: u64, value: f32, flags: u8) {
    let j = job();
    if j.state != State::Recording || j.stopping {
        return;
    }
    let mut row = [0u8; 96];
    let mut w = W { b: &mut row, n: 0 };
    w.ms(t_ms.saturating_sub(j.t0_ms));
    w.c(b',');
    if flags & F_OL == 0 && value.is_finite() {
        w.num(value);
    }
    w.c(b',');
    let u = j.unit;
    w.bytes(&u[..j.ulen]);
    w.c(b',');
    let mut sep = false;
    let mut tok = |w: &mut W, t: &str| {
        if sep {
            w.c(b' ');
        }
        w.s(t);
        sep = true;
    };
    for (bit, t) in FLAG_TXT {
        if flags & bit != 0 {
            tok(&mut w, t);
        }
    }
    if j.gap {
        tok(&mut w, "GAP");
    }
    w.s("\r\n");
    let n = w.n;
    if j.put(&row[..n]) {
        j.rows += 1;
        j.gap = false;
    } else {
        j.dropped += 1;
        j.gap = true;
    }
}

/// End the recording: the remaining rows are committed by the next polls,
/// then `take_result` reports it.
pub fn stop() {
    let j = job();
    if j.state == State::Recording {
        j.stopping = true;
        j.state = State::Stopping;
    }
}

/// A recording is active (recording or stopping): show REC.
pub fn active() -> bool {
    matches!(job().state, State::Recording | State::Stopping)
}

/// A commit is writing the disk right now.
pub fn writing() -> bool {
    job().phase != Phase::None
}

pub fn status() -> Status {
    status_of(job())
}

fn status_of(j: &Job) -> Status {
    Status {
        state: j.state,
        file: j.has_file.then_some(j.n),
        rows: j.rows,
        bytes: j.bytes,
        committed: j.csize,
        dropped: j.dropped,
    }
}

/// The outcome of the last recording, once (when it ended).
pub fn take_result() -> Option<Status> {
    unsafe { (*core::ptr::addr_of_mut!(RESULT)).take() }
}

fn disk() -> &'static mut crate::spiflash::SpiFlash {
    let d = unsafe { &mut *core::ptr::addr_of_mut!(DISK) };
    d.get_or_insert_with(crate::spiflash::SpiFlash::new)
}

/// One bounded slice (main loop, every pass).
pub fn poll(now_ms: u64) {
    if active() {
        poll_on(disk(), now_ms);
    }
}

/// Drive a stop to the end now (power-off path): blocks for the last commit
/// (normally < 0.5 s). The caller must not run a screenshot meanwhile.
pub fn finish_blocking(now_ms: u64) {
    stop();
    let mut k = 0u32;
    while active() && k < 200_000 {
        poll(now_ms);
        crate::wdt::kick_if_feeding();
        k += 1;
    }
}

/// `poll` on any block device (host tests).
pub fn poll_on(disk: &mut dyn BlockDevice, now_ms: u64) {
    let j = job();
    j.now = now_ms;
    if !active() {
        return;
    }
    if j.phase == Phase::None {
        let pending = j.fh.wrapping_sub(j.ft);
        let due = !j.has_file
            || j.stopping
            || j.tfill + pending >= BLK as u32
            || pending >= (FIFO as u32 / 4) * 3
            || now_ms.saturating_sub(j.last_commit) >= COMMIT_MS;
        if pending == 0 {
            if j.stopping {
                j.finish(State::Done);
            }
            return;
        }
        if !due || crate::screenshot::busy() || crate::msc::HOST_WRITING.load(Ordering::Relaxed) {
            return;
        }
        if !disk.present() || disk.block_size() as usize != BLK {
            j.finish(State::NoDisk);
            return;
        }
        j.tx_left = pending;
        j.cur = NONE; // scratch[1] was free for others since the last commit
        j.phase = if j.v.is_none() { Phase::Bpb } else { Phase::Data };
        set_flags(true, true);
    }
    let r = match j.phase {
        Phase::None => None,
        Phase::Bpb => j.bpb(disk),
        Phase::Root => j.root(disk),
        Phase::Data => j.data(disk),
        Phase::Alloc => j.alloc(disk),
        Phase::Fat => j.fat(disk),
        Phase::Dir => j.dir(disk),
    };
    if let Some(s) = r {
        j.finish(s);
    }
}

impl Job {
    /// Append bytes to FIFO, all or nothing.
    fn put(&mut self, b: &[u8]) -> bool {
        let used = self.fh.wrapping_sub(self.ft) as usize;
        if FIFO - used < b.len() {
            return false;
        }
        let f = fifo();
        for &c in b {
            f[self.fh as usize % FIFO] = c;
            self.fh = self.fh.wrapping_add(1);
        }
        self.bytes += b.len() as u32;
        true
    }

    fn finish(&mut self, s: State) {
        self.phase = Phase::None;
        self.state = s;
        set_flags(false, false);
        if self.wrote {
            crate::screenshot::MEDIUM_CHANGED.store(true, Ordering::Relaxed);
        }
        let st = status_of(self);
        unsafe { *core::ptr::addr_of_mut!(RESULT) = Some(st) };
    }

    /// End of a commit (its directory entry is written).
    fn committed(&mut self) -> Option<State> {
        self.phase = Phase::None;
        self.csize = self.size;
        self.last_commit = self.now;
        set_flags(true, false);
        if !self.has_file {
            self.has_file = true;
            crate::screenshot::MEDIUM_CHANGED.store(true, Ordering::Relaxed);
        }
        if self.full {
            return Some(State::Full);
        }
        if self.stopping && self.fh == self.ft {
            return Some(State::Done);
        }
        None
    }

    fn vol(&self) -> Vol {
        self.v.unwrap_or_else(|| unreachable!())
    }

    fn bpb(&mut self, disk: &mut dyn BlockDevice) -> Option<State> {
        let buf = &mut calbackup::scratch()[0];
        if !disk.read_block(0, buf) {
            return Some(State::Error);
        }
        let Some(v) = parse_bpb(&buf[..], disk.block_count()) else {
            return Some(State::NoDisk);
        };
        self.v = Some(v);
        self.i = 0;
        self.phase = Phase::Root;
        None
    }

    /// One root block: LOG%03u.CSV numbers in use, the first free slot.
    fn root(&mut self, disk: &mut dyn BlockDevice) -> Option<State> {
        let v = self.vol();
        let buf = &mut calbackup::scratch()[0];
        if !disk.read_block(v.root_start + self.i, buf) {
            return Some(State::Error);
        }
        let mut end = false;
        for k in 0..PER {
            let idx = self.i * PER + k;
            if idx >= v.root_entries {
                end = true;
                break;
            }
            let e = &buf[(k * 32) as usize..(k * 32 + 32) as usize];
            if e[0] == 0x00 || e[0] == 0xE5 {
                if self.slot == NONE {
                    self.slot = idx;
                }
                if e[0] == 0x00 {
                    end = true;
                    break;
                }
                continue;
            }
            if e[11] & 0x3F == 0x0F || !e[3..6].iter().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let n = (e[3] - b'0') as u16 * 100 + (e[4] - b'0') as u16 * 10 + (e[5] - b'0') as u16;
            if crate::fatren::sfn_is(e, &calbackup::numbered_name(b"LOG", n, b"CSV")) {
                self.taken[(n / 32) as usize] |= 1 << (n % 32);
            }
        }
        self.i += 1;
        if !end && self.i < v.root_blocks {
            return None;
        }
        let Some(n) = (0..1000u16).find(|&n| self.taken[(n / 32) as usize] & (1 << (n % 32)) == 0) else {
            return Some(State::Full);
        };
        if self.slot == NONE {
            return Some(State::Full);
        }
        self.n = n;
        self.hint = 2;
        self.phase = Phase::Data;
        None
    }

    /// FAT 1 entry `c` through the one-block cache in scratch[1].
    fn fat_get(&mut self, disk: &mut dyn BlockDevice, c: u32) -> Option<u32> {
        let v = self.vol();
        let fat = &mut calbackup::scratch()[1];
        let fb = c / FPER;
        if fb >= v.fat_blocks {
            return None;
        }
        if fb != self.cur {
            if !disk.read_block(v.fat_start + fb, fat) {
                self.cur = NONE;
                return None;
            }
            self.cur = fb;
        }
        Some(le16(fat, ((c % FPER) * 2) as usize))
    }

    /// The cluster holding TAIL (the chain's last, linked or new).
    fn tail_cluster(&self) -> u32 {
        if self.nnew > 0 {
            self.newcl[self.nnew - 1]
        } else {
            self.last
        }
    }

    /// Fill TAIL from FIFO (once) and write it.
    fn data(&mut self, disk: &mut dyn BlockDevice) -> Option<State> {
        let v = self.vol();
        if !self.filled {
            if self.tx_left == 0 {
                self.phase = if self.nnew > 0 { self.fat_begin() } else { Phase::Dir };
                return None;
            }
            // A full TAIL stays in RAM until the next block starts (disk
            // full: the size is then cut back to its last whole row).
            let next = if self.tfill == BLK as u32 { self.tb + 1 } else { self.tb };
            if next / v.spc >= self.nlinked + self.nnew as u32 {
                if self.nnew == MAXNEW {
                    // Rare (a very long deferral): commit, go on next pass.
                    self.phase = self.fat_begin();
                } else {
                    self.scanned = 0;
                    self.phase = Phase::Alloc;
                }
                return None;
            }
            if self.tfill == BLK as u32 {
                self.tb += 1;
                self.tfill = 0;
                tail().fill(0xFF);
            }
            let k = (BLK as u32 - self.tfill).min(self.tx_left);
            let t = tail();
            let f = fifo();
            for b in &mut t[self.tfill as usize..(self.tfill + k) as usize] {
                *b = f[self.ft as usize % FIFO];
                self.ft = self.ft.wrapping_add(1);
            }
            self.tfill += k;
            self.size += k;
            self.tx_left -= k;
            self.lba = v.data_start + (self.tail_cluster() - 2) * v.spc + self.tb % v.spc;
            self.filled = true;
            self.wrote = true;
            self.st = STEP0;
        }
        match disk.write_step(self.lba, &tail()[..], &mut self.st)? {
            false => Some(State::Error),
            true => {
                self.filled = false;
                None
            }
        }
    }

    /// One FAT 1 block of the free-cluster search (from `hint`, wrapping).
    fn alloc(&mut self, disk: &mut dyn BlockDevice) -> Option<State> {
        let v = self.vol();
        let max = v.clusters + 1;
        if self.hint < 2 || self.hint > max {
            self.hint = 2;
        }
        let fb = self.hint / FPER;
        while self.scanned < v.clusters && self.hint / FPER == fb {
            let c = self.hint;
            let Some(e) = self.fat_get(disk, c) else {
                return Some(State::Error);
            };
            self.hint = if c == max { 2 } else { c + 1 };
            self.scanned += 1;
            if e == 0 && !self.newcl[..self.nnew].contains(&c) && c != self.last {
                self.newcl[self.nnew] = c;
                self.nnew += 1;
                self.phase = Phase::Data;
                return None;
            }
            if self.hint == 2 {
                break; // wrapped: next block is 0
            }
        }
        if self.scanned >= v.clusters {
            // Disk full: commit what is written up to its last whole row
            // (TAIL = the last, full block), then stop.
            self.full = true;
            self.tx_left = 0;
            if self.tfill == BLK as u32 {
                let p = tail().iter().rposition(|&c| c == b'\n').map_or(0, |p| p + 1);
                self.size = self.tb * BLK as u32 + p as u32;
            }
            self.phase = if self.nnew > 0 { self.fat_begin() } else { Phase::Dir };
        }
        None
    }

    /// FAT blocks to update, the one with the link last.
    fn fat_begin(&mut self) -> Phase {
        let link = (self.nlinked > 0).then_some(self.last / FPER);
        self.nfb = 0;
        let add = |fb: u32, s: &mut Self| {
            if Some(fb) != link && !s.fbs[..s.nfb].contains(&fb) {
                s.fbs[s.nfb] = fb;
                s.nfb += 1;
            }
        };
        for i in 0..self.nnew {
            add(self.newcl[i] / FPER, self);
        }
        if let Some(l) = link {
            self.fbs[self.nfb] = l;
            self.nfb += 1;
        }
        self.fat_k = 0;
        self.fbi = 0;
        self.filled = false;
        Phase::Fat
    }

    fn fat(&mut self, disk: &mut dyn BlockDevice) -> Option<State> {
        let v = self.vol();
        if !self.filled {
            let fb = self.fbs[self.fbi];
            self.lba = v.fat_start + self.fat_k * v.fat_blocks + fb;
            let buf = &mut calbackup::scratch()[0];
            if !disk.read_block(self.lba, buf) {
                return Some(State::Error);
            }
            let first = self.fat_k == 0;
            let mut set = |c: u32, val: u32, want_free: bool| -> bool {
                if c / FPER != fb {
                    return true;
                }
                let o = ((c % FPER) * 2) as usize;
                let old = le16(&buf[..], o);
                if first && (if want_free { old != 0 } else { old < 0xFFF8 }) {
                    return false; // changed since the scan / the last commit
                }
                buf[o..o + 2].copy_from_slice(&(val as u16).to_le_bytes());
                true
            };
            for i in 0..self.nnew {
                let next = if i + 1 < self.nnew { self.newcl[i + 1] } else { 0xFFFF };
                if !set(self.newcl[i], next, true) {
                    return Some(State::Error);
                }
            }
            if self.nlinked > 0 && !set(self.last, self.newcl[0], false) {
                return Some(State::Error);
            }
            self.filled = true;
            return None; // the write from the next pass
        }
        // Whole block in one call (H1: never left erased across a pass).
        let buf = &calbackup::scratch()[0];
        match disk.write_block(self.lba, &buf[..]) {
            false => Some(State::Error),
            true => {
                self.filled = false;
                self.fbi += 1;
                if self.fbi == self.nfb {
                    self.fbi = 0;
                    self.fat_k += 1;
                    if self.fat_k == v.nfats {
                        if self.nlinked == 0 {
                            self.first = self.newcl[0];
                        }
                        self.nlinked += self.nnew as u32;
                        self.last = self.newcl[self.nnew - 1];
                        self.nnew = 0;
                        self.cur = NONE; // FAT 1 changed
                        self.phase = Phase::Dir;
                    }
                }
                None
            }
        }
    }

    /// The root entry: created on the first commit, then size updates.
    fn dir(&mut self, disk: &mut dyn BlockDevice) -> Option<State> {
        let v = self.vol();
        if !self.filled {
            if self.has_file && self.size == self.csize {
                return self.committed(); // nothing new (disk full at once)
            }
            self.lba = v.root_start + self.slot / PER;
            let buf = &mut calbackup::scratch()[0];
            if !disk.read_block(self.lba, buf) {
                return Some(State::Error);
            }
            let o = ((self.slot % PER) * 32) as usize;
            let name = calbackup::numbered_name(b"LOG", self.n, b"CSV");
            let e = &mut buf[o..o + 32];
            let ok = if self.has_file {
                e[..11] == name[..] && le16(e, 0x1A) == self.first && le32(e, 0x1C) == self.csize
            } else {
                e[0] == 0x00 || e[0] == 0xE5
            };
            if !ok {
                return Some(State::Error); // taken / changed since
            }
            if self.size == 0 || self.first == 0 {
                return Some(State::Full); // nothing could be written
            }
            e.fill(0);
            e[..11].copy_from_slice(&name);
            e[11] = 0x20; // archive
            e[0x10..0x12].copy_from_slice(&DATE.to_le_bytes());
            e[0x12..0x14].copy_from_slice(&DATE.to_le_bytes());
            e[0x18..0x1A].copy_from_slice(&DATE.to_le_bytes());
            e[0x1A..0x1C].copy_from_slice(&(self.first as u16).to_le_bytes());
            e[0x1C..0x20].copy_from_slice(&self.size.to_le_bytes());
            self.filled = true;
            self.wrote = true;
            return None;
        }
        // Whole block in one call (H1: never left erased across a pass).
        let buf = &calbackup::scratch()[0];
        match disk.write_block(self.lba, &buf[..]) {
            false => Some(State::Error),
            true => {
                self.filled = false;
                self.committed()
            }
        }
    }
}

// ------------------------------------------------------------- formatting

/// A tiny byte writer (silently truncates).
struct W<'a> {
    b: &'a mut [u8],
    n: usize,
}

impl W<'_> {
    fn c(&mut self, c: u8) {
        if self.n < self.b.len() {
            self.b[self.n] = c;
            self.n += 1;
        }
    }
    fn bytes(&mut self, s: &[u8]) {
        for &c in s {
            self.c(c);
        }
    }
    fn s(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    /// User text: ASCII, no CSV specials.
    fn a(&mut self, s: &str) {
        let mut t = [0u8; 24];
        let n = ascii(s, &mut t);
        self.bytes(&t[..n]);
    }
    fn u(&mut self, v: u64) {
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
    fn ms(&mut self, ms: u64) {
        self.u(ms / 1000);
        self.c(b'.');
        let r = (ms % 1000) as u32;
        self.c(b'0' + (r / 100) as u8);
        self.c(b'0' + (r / 10 % 10) as u8);
        self.c(b'0' + (r % 10) as u8);
    }
    /// 6 significant digits: plain for 1e-3 <= |v| < 1e6, else d.dddddE+xx.
    fn num(&mut self, v: f32) {
        if v < 0.0 {
            self.c(b'-');
        }
        let x = if v < 0.0 { -v } else { v };
        if x < 1e-30 {
            self.c(b'0');
            return;
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
        let m = m.clamp(100_000, 999_999);
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

/// ASCII copy of `s` (transliterating the meter's symbols, no ',' '"' or
/// control characters); returns the length.
fn ascii(s: &str, out: &mut [u8]) -> usize {
    let mut w = W { b: out, n: 0 };
    for ch in s.chars() {
        match ch {
            '\u{3a9}' | '\u{2126}' => w.s("Ohm"),
            '\u{b0}' => w.s("deg"),
            '\u{b5}' | '\u{3bc}' => w.c(b'u'),
            ',' | '"' => w.c(b' '),
            c if c.is_ascii_graphic() || c == ' ' => w.c(c as u8),
            _ => w.c(b'?'),
        }
    }
    w.n
}

/// Header name of a function code (meter::func_of / Dmm::func codes).
#[allow(dead_code)]
pub fn func_label(code: u8) -> &'static str {
    match code {
        0 | 2 => "AC V",
        1 | 3 => "DC V",
        4..=6 => "AC A",
        7..=9 => "DC A",
        10 => "Resistance",
        11 => "Continuity",
        12 => "Diode",
        13 => "Capacitance",
        14 => "Frequency",
        16 => "AC+DC V",
        17 => "AC+DC A",
        _ => "Temperature",
    }
}

// ------------------------------------------------------------- gallery

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Bmp,
    Csv,
}

#[derive(Clone, Copy, Debug)]
pub struct FileInfo {
    /// 8.3 name, space padded ("LOG001  CSV"); `name_str` formats it.
    pub name: [u8; 11],
    pub size: u32,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug)]
pub struct Catalog {
    /// Entries stored in `out` (directory order).
    pub n: usize,
    /// Matching files in the root (may exceed `out.len()`).
    pub total: usize,
    pub free_bytes: u32,
    pub disk_bytes: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GErr {
    NoDisk,
    /// A screenshot / a commit / a host write holds the disk, or the file is
    /// the one being recorded: try again later.
    Busy,
    NotFound,
    Error,
}

/// The disk is free for a synchronous gallery operation.
fn gallery_free() -> bool {
    !crate::screenshot::busy() && !writing() && !crate::msc::HOST_WRITING.load(Ordering::Relaxed)
}

fn gallery_vol(disk: &mut dyn BlockDevice) -> Result<Vol, GErr> {
    if !gallery_free() {
        return Err(GErr::Busy);
    }
    if !disk.present() || disk.block_size() as usize != BLK {
        return Err(GErr::NoDisk);
    }
    let buf = &mut calbackup::scratch()[0];
    if !disk.read_block(0, buf) {
        return Err(GErr::Error);
    }
    parse_bpb(&buf[..], disk.block_count()).ok_or(GErr::NoDisk)
}

fn kind_of(e: &[u8]) -> Option<Kind> {
    if e[11] & 0x18 != 0 || e[11] & 0x3F == 0x0F {
        return None; // directory, volume label, LFN
    }
    kind_of_name(&e[..11])
}

fn kind_of_name(e: &[u8]) -> Option<Kind> {
    let up = |c: u8| c.to_ascii_uppercase();
    let ext = [up(e[8]), up(e[9]), up(e[10])];
    if up(e[0]) == b'S' && &ext == b"BMP" {
        Some(Kind::Bmp)
    } else if up(e[0]) == b'L' && up(e[1]) == b'O' && up(e[2]) == b'G' && &ext == b"CSV" {
        Some(Kind::Csv)
    } else {
        None
    }
}

/// Root files S*.BMP and LOG*.CSV (directory order) into `out`, with the
/// free space. Synchronous: the root blocks + FAT 1.
pub fn list_on(disk: &mut dyn BlockDevice, out: &mut [FileInfo]) -> Result<Catalog, GErr> {
    let mut n = 0usize;
    let mut cat = for_each_file(disk, &mut |f| {
        if n < out.len() {
            out[n] = *f;
            n += 1;
        }
    })?;
    cat.n = n;
    Ok(cat)
}

/// Every gallery file to `f` (directory order); `n` = 0 in the result.
fn for_each_file(disk: &mut dyn BlockDevice, f: &mut dyn FnMut(&FileInfo)) -> Result<Catalog, GErr> {
    let v = gallery_vol(disk)?;
    let mut cat = Catalog { n: 0, total: 0, free_bytes: 0, disk_bytes: v.clusters * v.spc * BLK as u32 };
    let buf = &mut calbackup::scratch()[0];
    'blocks: for b in 0..v.root_blocks {
        if !disk.read_block(v.root_start + b, buf) {
            return Err(GErr::Error);
        }
        for k in 0..PER {
            if b * PER + k >= v.root_entries {
                break 'blocks;
            }
            let e = &buf[(k * 32) as usize..(k * 32 + 32) as usize];
            if e[0] == 0x00 {
                break 'blocks;
            }
            if e[0] == 0xE5 {
                continue;
            }
            if let Some(kind) = kind_of(e) {
                let mut name = [0u8; 11];
                name.copy_from_slice(&e[..11]);
                f(&FileInfo { name, size: le32(e, 0x1C), kind });
                cat.total += 1;
            }
        }
    }
    let mut free = 0u32;
    let mut fb = NONE;
    for c in 2..=v.clusters + 1 {
        if c / FPER != fb {
            fb = c / FPER;
            if !disk.read_block(v.fat_start + fb, buf) {
                return Err(GErr::Error);
            }
        }
        if le16(&buf[..], ((c % FPER) * 2) as usize) == 0 {
            free += 1;
        }
    }
    cat.free_bytes = free * v.spc * BLK as u32;
    Ok(cat)
}

/// SCPI MMEMory:CATalog? reply into `out`: `<used>,<free>{,"<name>,<type>,<size>"}`
/// (bytes; type BIN for a BMP, ASC for a CSV; gallery files only). Entries
/// that do not fit are left out. Returns the length.
pub fn catalog_scpi_on(disk: &mut dyn BlockDevice, out: &mut [u8]) -> Result<usize, GErr> {
    // The two numbers first need the free count, known only after the scan:
    // write the entries after a 24-byte gap, then move them up.
    const GAP: usize = 24;
    if out.len() < GAP + 2 {
        return Err(GErr::Error);
    }
    let (head, rest) = out.split_at_mut(GAP);
    let mut w = W { b: rest, n: 0 };
    let cat = for_each_file(disk, &mut |f| {
        let mut nb = [0u8; 12];
        let mut e = [0u8; 40];
        let mut t = W { b: &mut e, n: 0 };
        t.s(",\"");
        t.s(name_str(&f.name, &mut nb));
        t.s(if f.kind == Kind::Bmp { ",BIN," } else { ",ASC," });
        t.u(f.size as u64);
        t.c(b'"');
        let n = t.n;
        if w.n + n <= w.b.len() {
            w.bytes(&e[..n]);
        }
    })?;
    let body = w.n;
    let mut h = W { b: head, n: 0 };
    h.u((cat.disk_bytes - cat.free_bytes) as u64);
    h.c(b',');
    h.u(cat.free_bytes as u64);
    let hn = h.n;
    out.copy_within(GAP..GAP + body, hn);
    Ok(hn + body)
}

#[allow(dead_code)]
pub fn catalog_scpi(out: &mut [u8]) -> Result<usize, GErr> {
    catalog_scpi_on(disk(), out)
}

/// `status()` as the SCPI LOG:CSV? reply: `<state>,<file>,<rows>,<bytes>,<committed>,<dropped>`
/// (state 0 idle, 1 recording, 2 stopping, 3 done, 4 full, 5 no disk,
/// 6 error; file -1 = none yet). Returns the length.
#[allow(dead_code)]
pub fn status_scpi(out: &mut [u8]) -> usize {
    let st = status();
    let mut w = W { b: out, n: 0 };
    w.u(st.state as u64);
    w.c(b',');
    match st.file {
        Some(n) => w.u(n as u64),
        None => w.s("-1"),
    }
    for v in [st.rows, st.bytes, st.committed, st.dropped] {
        w.c(b',');
        w.u(v as u64);
    }
    w.n
}

fn sfn_checksum(e: &[u8]) -> u8 {
    let mut s = 0u8;
    for &c in &e[0..11] {
        s = ((s & 1) << 7).wrapping_add(s >> 1).wrapping_add(c);
    }
    s
}

/// Delete root file `name` (8.3, space padded; only S*.BMP / LOG*.CSV):
/// its entry and long-name entries 0xE5 (read back), then its chain freed in
/// every FAT copy (fatren::free_chain: bits cleared, no erase).
pub fn delete_on(disk: &mut dyn BlockDevice, name: &[u8; 11]) -> Result<(), GErr> {
    if kind_of_name(name).is_none() {
        return Err(GErr::NotFound);
    }
    let j = job();
    if active() && j.has_file && *name == calbackup::numbered_name(b"LOG", j.n, b"CSV") {
        return Err(GErr::Busy);
    }
    let v = gallery_vol(disk)?;
    let [buf, fat1, tmp] = calbackup::scratch();
    // Find the entry; remember the live LFN run just before it (global indexes).
    let mut run = [0u32; 20];
    let mut nrun = 0usize;
    let mut hit: Option<(u32, u32, [u32; 20], usize)> = None;
    'blocks: for b in 0..v.root_blocks {
        if !disk.read_block(v.root_start + b, buf) {
            return Err(GErr::Error);
        }
        for k in 0..PER {
            let idx = b * PER + k;
            if idx >= v.root_entries {
                break 'blocks;
            }
            let e = &buf[(k * 32) as usize..(k * 32 + 32) as usize];
            if e[0] == 0x00 {
                break 'blocks;
            }
            if e[0] == 0xE5 {
                nrun = 0;
                continue;
            }
            if e[11] & 0x3F == 0x0F {
                if nrun == run.len() {
                    run.copy_within(1.., 0);
                    nrun -= 1;
                }
                run[nrun] = idx;
                nrun += 1;
                continue;
            }
            if e[11] & 0x18 == 0 && crate::fatren::sfn_is(e, name) {
                // The LFN run before it; only entries with its checksum are
                // marked below.
                hit = Some((idx, le16(e, 0x1A) | (sfn_checksum(e) as u32) << 16, run, nrun));
                break 'blocks;
            }
            nrun = 0;
        }
    }
    let Some((idx, start_ck, lfn, nl)) = hit else {
        return Err(GErr::NotFound);
    };
    let (start, ck) = (start_ck & 0xFFFF, (start_ck >> 16) as u8);
    // Entries 0xE5, block by block (LFN entries only with a matching checksum).
    for b in 0..v.root_blocks {
        let (lo, hi) = (b * PER, b * PER + PER);
        let inb = |i: u32| i >= lo && i < hi;
        if !inb(idx) && !lfn[..nl].iter().any(|&i| inb(i)) {
            continue;
        }
        if !disk.read_block(v.root_start + b, buf) {
            return Err(GErr::Error);
        }
        for &i in &lfn[..nl] {
            let o = ((i - lo) * 32) as usize;
            if inb(i) && buf[o + 13] == ck {
                buf[o] = 0xE5;
            }
        }
        if inb(idx) {
            buf[((idx - lo) * 32) as usize] = 0xE5;
        }
        if !(disk.write_block(v.root_start + b, buf) && crate::fatren::verify(disk, v.root_start + b, buf, tmp)) {
            return Err(GErr::Error);
        }
    }
    crate::screenshot::MEDIUM_CHANGED.store(true, Ordering::Relaxed);
    let (ok, _) = crate::fatren::free_chain(disk, &v, start, fat1, buf, tmp);
    if ok {
        Ok(())
    } else {
        Err(GErr::Error)
    }
}

/// Device wrappers (the recorder's SpiFlash handle).
#[allow(dead_code)]
pub fn list(out: &mut [FileInfo]) -> Result<Catalog, GErr> {
    list_on(disk(), out)
}
#[allow(dead_code)]
pub fn delete(name: &[u8; 11]) -> Result<(), GErr> {
    delete_on(disk(), name)
}

/// "log1.csv" / "LOG001.CSV" -> "LOG1    CSV" (8.3, upper case). None: not
/// an 8.3 name.
#[allow(dead_code)]
pub fn parse_name(s: &[u8]) -> Option<[u8; 11]> {
    let s = match s {
        [b'"', m @ .., b'"'] => m,
        _ => s,
    };
    let dot = s.iter().position(|&c| c == b'.').unwrap_or(s.len());
    let (base, ext) = (&s[..dot], if dot < s.len() { &s[dot + 1..] } else { &[][..] });
    if base.is_empty() || base.len() > 8 || ext.len() > 3 {
        return None;
    }
    let mut n = [b' '; 11];
    for (i, &c) in base.iter().enumerate() {
        n[i] = c.to_ascii_uppercase();
    }
    for (i, &c) in ext.iter().enumerate() {
        n[8 + i] = c.to_ascii_uppercase();
    }
    n.iter().all(|&c| c.is_ascii_graphic() || c == b' ').then_some(n)
}

/// "LOG001  CSV" -> "LOG001.CSV".
#[allow(dead_code)]
pub fn name_str<'a>(n: &[u8; 11], out: &'a mut [u8; 12]) -> &'a str {
    let mut k = 0;
    for &c in n[..8].iter().filter(|&&c| c != b' ') {
        out[k] = c;
        k += 1;
    }
    if n[8] != b' ' {
        out[k] = b'.';
        k += 1;
        for &c in n[8..].iter().filter(|&&c| c != b' ') {
            out[k] = c;
            k += 1;
        }
    }
    core::str::from_utf8(&out[..k]).unwrap_or("?")
}
