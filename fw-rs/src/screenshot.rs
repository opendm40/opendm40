//! Screenshot to the SPI-flash USB disk (docs/user-feedback.md UF-17, stock
//! "PriScr": 3-finger swipe down, gui_v2_6.h "Screenshot").
//!
//! File: the next free `S%03u.BMP` in the root (lowest unused number, as the
//! stock "s%.3d.bmp" / first free number, spiflash_v2_6.h), byte-compatible
//! with the stock files on this unit's disk (dumps/spi_dumps
//! dm40_msc_full_2026-10-13.bin S000..S002.BMP): 480x320, 16 bpp RGB565,
//! BI_BITFIELDS (masks F800/07E0/001F), 66-byte header (14 file + 40 info +
//! 12 masks), positive height = bottom-up rows of 960 bytes (no padding),
//! pixels little-endian, 72 dpi fields 0: 307266 bytes = 76 clusters of 4 KiB.
//! (Not 24-bit: that would be 460854 bytes. The 17 KB S00x.BMP in
//! spi-disk-backup/ are older 4-bpp RLE factory files.)
//!
//! Incremental: `start` only takes the request; `poll` (every main-loop pass,
//! from App::tick) does one bounded slice of the job, so USB (SCPI and the
//! disk), the DMM, the scope and the watchdog keep running during the ~5 s
//! write. One shot at a time (`start` refuses while `busy`). Slices:
//!   Start    wait for a host WRITE(10) in progress to finish (msc.rs;
//!            at most 2 s, then "failed");
//!   Capture  GRAM -> RAM, file rows (bottom-up) in order, each row
//!            run-length coded into CAP (native: 96 KiB) while it fits;
//!            rows per pass until ~2 ms are spent (~0.3 ms per row);
//!   Bpb      boot sector (1 block read);
//!   Root     one root block per pass: used S%03u numbers, first free slot;
//!   Alloc    one FAT 1 block per pass: first-fit free clusters;
//!   Data     per 4 KiB block: fill (header / pixels from CAP or GRAM, CRC),
//!            then `BlockDevice::write_step` (spiflash.rs: compare, erase,
//!            16 page programs, read back; <= ~2 ms of CPU per pass, the
//!            erase and the programs run in the chip between passes);
//!   Fat      the chain in every FAT copy (FAT 1 first; read, link, write);
//!   Dir      the root entry (read, check the slot is still free, write);
//!   Verify   one data block per pass re-read through the new FAT 1 chain,
//!            CRC-32 compared with the CRC of what was written.
//! Order data -> FAT -> directory as calbackup / fatren: a power loss leaves
//! at worst lost clusters, never an entry on unwritten or free clusters.
//! Every block write is read back; nothing is written unless the boot sector
//! is the expected FAT16 volume.
//!
//! Frozen image: there is no frame buffer, so the rows must be read from the
//! panel while nothing draws over them. `frozen()` is true from `start` until
//! every row has been read (main.rs then passes can_draw = false; App::gesture
//! drops input): normally until the Capture slices are done (all 320 rows fit
//! in CAP: ~0.1 s of GRAM reads spread over ~50 passes; mostly flat UI screens
//! code to a fraction of the 300 KiB raw frame). If CAP fills up first (a busy
//! scope trace, or the RAM build, which has no room for CAP), the remaining
//! rows are read from the GRAM when their data block is filled, and the
//! screen stays frozen until the last data block (~4-5 s), as before but with
//! the main loop running. The verify pass compares CRCs, so it does not need
//! the panel again.
//!
//! While busy the MSC refuses host writes (NOT READY, msc.rs), so the host
//! cannot change the FAT between our scan and our update; host reads are
//! served (spiflash.rs waits for an erase / program of ours to finish before
//! any read). Host-cache caveat: a host that has the USB disk MOUNTED keeps
//! its own copy of the FAT and root directory and does not see the new file;
//! worse, its next write can put its cached (older) FAT block back over our
//! chain. After a write `MEDIUM_CHANGED` is set so the MSC layer can report
//! UNIT ATTENTION 06/28/00 "medium may have changed" on the next host command,
//! which makes Windows / macOS re-read the volume (Linux drops its block cache
//! but a mounted vfat keeps its in-memory state: unmount / re-plug before
//! writing to the disk from the PC). Safe without caveats when no PC has the
//! disk mounted (charger, power bank, battery).

use core::sync::atomic::{AtomicBool, Ordering};

use crate::calbackup::{self, crc32_upd};
use crate::fatren::{le16, parse_bpb, sfn_is, Vol};
use crate::lcd;
use crate::msc::{BlockDevice, Step};

/// Set after a screenshot changed the disk; for msc.rs to report UNIT
/// ATTENTION (sense 06/28/00) once on the next command, then clear.
pub static MEDIUM_CHANGED: AtomicBool = AtomicBool::new(false);

const W: u32 = lcd::W as u32;
const H: u32 = lcd::H as u32;
const ROW: u32 = W * 2;
const HDR_LEN: u32 = 66;
const FILE_LEN: u32 = HDR_LEN + ROW * H; // 307266
const BLK: usize = 4096;
/// Data blocks of the file (the last one holds 66 bytes).
const NBLK: u32 = FILE_LEN.div_ceil(BLK as u32);
/// Root slots per block.
const PER: u32 = (BLK / 32) as u32;
/// FAT16 entries per block.
const FPER: u32 = (BLK / 2) as u32;
/// Write / create date 2026-10-13, time 0 (no RTC), as calbackup.
const DATE: u16 = (46 << 9) | (10 << 5) | 13;
/// CPU time per `poll` slice (cycles; 2 ms at 200 MHz). A slice may run over
/// by one block read (~2.7 ms at the 12.5 MHz SPI clock) or one row.
const STEP_CYC: u32 = crate::clock::HCLK_HZ / 500;
const NONE: u32 = u32::MAX;
/// Longest wait for a host WRITE(10) in progress before giving up (cycles,
/// 2 s): a host that died mid-transfer must not freeze the screen for good.
const START_CYC: u32 = crate::clock::HCLK_HZ * 2;

/// The stock header, byte for byte (S000.BMP at 0xD000 in the disk dump).
const HDR: [u8; HDR_LEN as usize] = [
    b'B', b'M', 0x42, 0xB0, 0x04, 0x00, // file size 307266
    0x00, 0x00, 0x00, 0x00, // reserved
    0x42, 0x00, 0x00, 0x00, // pixel data offset 66
    0x28, 0x00, 0x00, 0x00, // BITMAPINFOHEADER (40)
    0xE0, 0x01, 0x00, 0x00, // width 480
    0x40, 0x01, 0x00, 0x00, // height 320 (bottom-up)
    0x01, 0x00, 0x10, 0x00, // 1 plane, 16 bpp
    0x03, 0x00, 0x00, 0x00, // BI_BITFIELDS
    0x00, 0xB0, 0x04, 0x00, // image size 307200
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // ppm x / y
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // colours used / important
    0x00, 0xF8, 0x00, 0x00, 0xE0, 0x07, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00, // R G B masks
];
const _: () = assert!(FILE_LEN == 307_266 && NBLK == 76);
const _: () = assert!(HDR_LEN % 2 == 0 && BLK % 2 == 0, "block edges fall between pixels");

/// Captured rows, run-length coded (u16 words, per row: tokens until W
/// pixels; `n` < 0x8000 = a run of n pixels of the next word's colour,
/// 0x8000 | n = n literal pixels follow). Worst case W + 1 words per row.
/// The RAM image has no SRAM to spare (its .dmabuf is full): 0 there, every
/// row is then read from the GRAM while writing.
const CAP_WORDS: usize = if cfg!(any(feature = "native", not(target_os = "none"))) { 48 * 1024 } else { 0 };
#[cfg_attr(target_os = "none", link_section = ".dmabuf")]
static mut CAP: [u16; CAP_WORDS] = [0; CAP_WORDS];

fn cap() -> &'static mut [u16; CAP_WORDS] {
    unsafe { &mut *core::ptr::addr_of_mut!(CAP) }
}

/// Result of a screenshot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shot {
    /// Written and verified as S%03u.BMP.
    Saved(u16),
    /// No disk / not the expected FAT16 volume.
    NoDisk,
    /// Root directory or disk full, or S000..S999 all taken.
    Full,
    /// A read, write or the verify failed (at worst lost clusters).
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Start,
    Capture,
    Bpb,
    Root,
    Alloc,
    Data,
    Fat,
    Dir,
    Verify,
}

const STEP0: Step = Step { started: false, phase: 0, page: 0, dirty: 0, erase: false, t0: 0 };

struct Job {
    phase: Phase,
    frozen: bool,
    /// A block write was started (the disk may have changed).
    wrote: bool,
    /// File rows 0..rows are in CAP; row r at CAP[off[r]..off[r + 1]].
    rows: u32,
    off: [u32; H as usize + 1],
    v: Option<Vol>,
    /// Phase cursor: root block, FAT entry, data / verify block.
    i: u32,
    /// Numbers 0..=999 taken by an S%03u.BMP entry (bit n).
    taken: [u32; 32],
    slot: u32,
    n: u16,
    /// Clusters of the new file, in chain order (ascending, first fit).
    cl: [u16; NBLK as usize],
    ncl: u32,
    got: u32,
    /// scratch[0] holds the block being written (`lba`), step `st`.
    filled: bool,
    lba: u32,
    st: Step,
    /// CRC-32 register of the bytes written / re-read.
    crc: u32,
    vcrc: u32,
    /// Fat phase: copy, block.
    fat_k: u32,
    fat_fb: u32,
    /// FAT 1 block cached in scratch[1].
    cur: u32,
    /// Verify: current cluster, and its link from the previous one checked.
    vc: u32,
    linked: bool,
    /// `start` time (DWT cycles), for the Start timeout.
    t0: u32,
}

/// All zero, so JOB is in .bss (`start` sets the non-zero fields).
const JOB0: Job = Job {
    phase: Phase::Idle,
    frozen: false,
    wrote: false,
    rows: 0,
    off: [0; H as usize + 1],
    v: None,
    i: 0,
    taken: [0; 32],
    slot: 0,
    n: 0,
    cl: [0; NBLK as usize],
    ncl: 0,
    got: 0,
    filled: false,
    lba: 0,
    st: STEP0,
    crc: 0,
    vcrc: 0,
    fat_k: 0,
    fat_fb: 0,
    cur: 0,
    vc: 0,
    linked: false,
    t0: 0,
};

static mut JOB: Job = JOB0;
/// The last finished shot, not yet shown (`take_result`).
static mut RESULT: Option<Shot> = None;
/// The device's own SpiFlash handle (created on the first shot).
static mut DISK: Option<crate::spiflash::SpiFlash> = None;

fn job() -> &'static mut Job {
    unsafe { &mut *core::ptr::addr_of_mut!(JOB) }
}

fn cyc() -> u32 {
    unsafe { crate::mmio::r32(0xE000_1004) }
}

/// Request a screenshot of the screen as it is now (captured from the next
/// `poll` on; the caller draws nothing more while `frozen`). False: one is
/// already in flight (the request is ignored).
pub fn start() -> bool {
    let j = job();
    if j.phase != Phase::Idle {
        return false;
    }
    *j = JOB0;
    j.slot = NONE;
    j.cur = NONE;
    j.crc = !0;
    j.vcrc = !0;
    j.phase = Phase::Start;
    j.frozen = true;
    j.t0 = cyc();
    true
}

/// A screenshot is in flight (host disk writes are refused meanwhile).
pub fn busy() -> bool {
    job().phase != Phase::Idle
}

/// Rows still to be read from the panel: draw nothing.
pub fn frozen() -> bool {
    job().frozen
}

/// The result of the last finished shot, once.
pub fn take_result() -> Option<Shot> {
    unsafe { (*core::ptr::addr_of_mut!(RESULT)).take() }
}

/// One bounded slice of the job in flight (main loop, every pass).
pub fn poll() {
    if !busy() {
        return;
    }
    let disk = unsafe { &mut *core::ptr::addr_of_mut!(DISK) };
    let d = disk.get_or_insert_with(crate::spiflash::SpiFlash::new);
    step(d);
}

/// A whole screenshot on any block device, synchronously (host test:
/// tools/fatren-test; the device uses start / poll).
#[allow(dead_code)]
pub fn take_on(disk: &mut dyn BlockDevice) -> Shot {
    if !start() {
        return Shot::Error;
    }
    loop {
        if let Some(r) = step(disk) {
            take_result();
            return r;
        }
    }
}

fn step(disk: &mut dyn BlockDevice) -> Option<Shot> {
    let j = job();
    let r = match j.phase {
        Phase::Idle => return None,
        Phase::Start => {
            if crate::msc::HOST_WRITING.load(Ordering::Relaxed) {
                (cyc().wrapping_sub(j.t0) > START_CYC).then_some(Shot::Error)
            } else if !disk.present() || disk.block_size() as usize != BLK {
                Some(Shot::NoDisk)
            } else {
                j.phase = Phase::Capture;
                None
            }
        }
        Phase::Capture => {
            j.capture();
            None
        }
        Phase::Bpb => j.bpb(disk),
        Phase::Root => j.root(disk),
        Phase::Alloc => j.alloc(disk),
        Phase::Data => j.data(disk),
        Phase::Fat => j.fat(disk),
        Phase::Dir => j.dir(disk),
        Phase::Verify => j.verify(disk),
    };
    if let Some(r) = r {
        j.phase = Phase::Idle;
        j.frozen = false;
        unsafe { *core::ptr::addr_of_mut!(RESULT) = Some(r) };
        if j.wrote {
            MEDIUM_CHANGED.store(true, Ordering::Relaxed);
        }
    }
    r
}

impl Job {
    fn vol(&self) -> Vol {
        self.v.unwrap_or_else(|| unreachable!())
    }

    /// GRAM rows into CAP (file row r = screen row H - 1 - r).
    fn capture(&mut self) {
        let t = cyc();
        let cap = cap();
        let mut px = [0u16; W as usize];
        while self.rows < H {
            let mut k = 0usize;
            lcd::read_span(0, (H - 1 - self.rows) as u16, W as u16, |c| {
                px[k] = c;
                k += 1;
            });
            let o = self.off[self.rows as usize] as usize;
            let Some(n) = encode(&px, &mut cap[o..]) else {
                break; // CAP full: the rest from the GRAM while writing
            };
            self.rows += 1;
            self.off[self.rows as usize] = (o + n) as u32;
            if self.rows < H && cyc().wrapping_sub(t) >= STEP_CYC {
                return;
            }
        }
        if self.rows == H {
            self.frozen = false;
        }
        self.phase = Phase::Bpb;
    }

    fn bpb(&mut self, disk: &mut dyn BlockDevice) -> Option<Shot> {
        let buf = &mut calbackup::scratch()[0];
        if !disk.read_block(0, buf) {
            return Some(Shot::Error);
        }
        let Some(v) = parse_bpb(&buf[..], disk.block_count()) else {
            return Some(Shot::NoDisk);
        };
        self.v = Some(v);
        self.ncl = FILE_LEN.div_ceil(v.spc * BLK as u32);
        self.i = 0;
        self.phase = Phase::Root;
        None
    }

    /// One root block: S%03u.BMP numbers in use, the first free slot.
    fn root(&mut self, disk: &mut dyn BlockDevice) -> Option<Shot> {
        let v = self.vol();
        let buf = &mut calbackup::scratch()[0];
        if !disk.read_block(v.root_start + self.i, buf) {
            return Some(Shot::Error);
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
                    end = true; // end of directory
                    break;
                }
                continue;
            }
            if e[11] & 0x3F == 0x0F || !e[1..4].iter().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let n = (e[1] - b'0') as u16 * 100 + (e[2] - b'0') as u16 * 10 + (e[3] - b'0') as u16;
            if sfn_is(e, &calbackup::numbered_name(b"S", n, b"BMP")) {
                self.taken[(n / 32) as usize] |= 1 << (n % 32);
            }
        }
        self.i += 1;
        if !end && self.i < v.root_blocks {
            return None;
        }
        let Some(n) = (0..1000u16).find(|&n| self.taken[(n / 32) as usize] & (1 << (n % 32)) == 0) else {
            return Some(Shot::Full);
        };
        if self.slot == NONE || self.ncl as usize > self.cl.len() {
            return Some(Shot::Full);
        }
        self.n = n;
        self.i = 2;
        self.cur = NONE;
        self.phase = Phase::Alloc;
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

    /// Free clusters, first fit, from the entries of one FAT 1 block.
    fn alloc(&mut self, disk: &mut dyn BlockDevice) -> Option<Shot> {
        let v = self.vol();
        let fb = self.i / FPER;
        while self.got < self.ncl && self.i <= v.clusters + 1 && self.i / FPER == fb {
            match self.fat_get(disk, self.i) {
                None => return Some(Shot::Error),
                Some(0) => {
                    self.cl[self.got as usize] = self.i as u16;
                    self.got += 1;
                }
                Some(_) => {}
            }
            self.i += 1;
        }
        if self.got == self.ncl {
            self.i = 0;
            self.filled = false;
            self.phase = Phase::Data;
        } else if self.i > v.clusters + 1 {
            return Some(Shot::Full);
        }
        None
    }

    /// File bytes `off .. off + buf.len()` (whole pixels).
    fn fill(&self, off: u32, buf: &mut [u8]) {
        let mut i = 0usize;
        let mut off = off;
        while i < buf.len() && off < HDR_LEN {
            buf[i] = HDR[off as usize];
            i += 1;
            off += 1;
        }
        let cap = cap();
        while i + 1 < buf.len() {
            let px = (off - HDR_LEN) / 2;
            let (r, x) = (px / W, px % W);
            let n = (W - x).min(((buf.len() - i) / 2) as u32);
            let put = |c: u16| {
                buf[i..i + 2].copy_from_slice(&c.to_le_bytes());
                i += 2;
            };
            if r < self.rows {
                decode(&cap[self.off[r as usize] as usize..], x, n, put);
            } else {
                lcd::read_span(x as u16, (H - 1 - r) as u16, n as u16, put);
            }
            off += 2 * n;
        }
    }

    /// Fill (once) and write data block `i`.
    fn data(&mut self, disk: &mut dyn BlockDevice) -> Option<Shot> {
        let v = self.vol();
        if !self.filled {
            let off = self.i * BLK as u32;
            let len = (FILE_LEN - off).min(BLK as u32) as usize;
            let buf = &mut calbackup::scratch()[0];
            self.fill(off, &mut buf[..len]);
            buf[len..].fill(0xFF);
            self.crc = crc32_upd(self.crc, &buf[..len]);
            let c = self.cl[(self.i / v.spc) as usize] as u32;
            self.lba = v.data_start + (c - 2) * v.spc + self.i % v.spc;
            self.filled = true;
            self.wrote = true;
            self.st = STEP0;
            if self.i + 1 == NBLK {
                self.frozen = false; // every row has been read
            }
        }
        let buf = &calbackup::scratch()[0];
        match disk.write_step(self.lba, &buf[..], &mut self.st)? {
            false => Some(Shot::Error),
            true => {
                self.filled = false;
                self.i += 1;
                if self.i == NBLK {
                    self.fat_k = 0;
                    self.fat_fb = self.cl[0] as u32 / FPER;
                    self.phase = Phase::Fat;
                }
                None
            }
        }
    }

    /// Link the chain in FAT copy `fat_k`, block `fat_fb` (blocks between
    /// the first and the last cluster's that hold one of ours).
    fn fat(&mut self, disk: &mut dyn BlockDevice) -> Option<Shot> {
        let v = self.vol();
        let last = self.cl[self.ncl as usize - 1] as u32 / FPER;
        if !self.filled {
            let all = self.cl;
            let cl = &all[..self.ncl as usize];
            while self.fat_fb <= last && !cl.iter().any(|&c| c as u32 / FPER == self.fat_fb) {
                self.fat_fb += 1;
            }
            self.lba = v.fat_start + self.fat_k * v.fat_blocks + self.fat_fb;
            let buf = &mut calbackup::scratch()[0];
            if !disk.read_block(self.lba, buf) {
                return Some(Shot::Error);
            }
            for (i, &c) in cl.iter().enumerate() {
                let c = c as u32;
                if c / FPER != self.fat_fb {
                    continue;
                }
                let o = ((c % FPER) * 2) as usize;
                if self.fat_k == 0 && le16(&buf[..], o) != 0 {
                    return Some(Shot::Error); // taken since the scan
                }
                let next = cl.get(i + 1).copied().unwrap_or(0xFFFF);
                buf[o..o + 2].copy_from_slice(&next.to_le_bytes());
            }
            self.filled = true;
            self.st = STEP0;
            return None; // the write from the next pass (one block read per pass)
        }
        let buf = &calbackup::scratch()[0];
        match disk.write_step(self.lba, &buf[..], &mut self.st)? {
            false => Some(Shot::Error),
            true => {
                self.filled = false;
                self.fat_fb += 1;
                if self.fat_fb > last {
                    self.fat_k += 1;
                    self.fat_fb = self.cl[0] as u32 / FPER;
                    if self.fat_k == v.nfats {
                        self.phase = Phase::Dir;
                    }
                }
                None
            }
        }
    }

    /// The root entry, in the slot found free by the scan.
    fn dir(&mut self, disk: &mut dyn BlockDevice) -> Option<Shot> {
        let v = self.vol();
        if !self.filled {
            self.lba = v.root_start + self.slot / PER;
            let buf = &mut calbackup::scratch()[0];
            if !disk.read_block(self.lba, buf) {
                return Some(Shot::Error);
            }
            let o = ((self.slot % PER) * 32) as usize;
            let e = &mut buf[o..o + 32];
            if e[0] != 0x00 && e[0] != 0xE5 {
                return Some(Shot::Error); // taken since the scan
            }
            e.fill(0);
            e[..11].copy_from_slice(&calbackup::numbered_name(b"S", self.n, b"BMP"));
            e[11] = 0x20; // archive
            e[0x10..0x12].copy_from_slice(&DATE.to_le_bytes()); // created
            e[0x12..0x14].copy_from_slice(&DATE.to_le_bytes()); // accessed
            e[0x18..0x1A].copy_from_slice(&DATE.to_le_bytes()); // written (time 0)
            e[0x1A..0x1C].copy_from_slice(&self.cl[0].to_le_bytes());
            e[0x1C..0x20].copy_from_slice(&FILE_LEN.to_le_bytes());
            self.filled = true;
            self.st = STEP0;
            return None; // the write from the next pass
        }
        let buf = &calbackup::scratch()[0];
        match disk.write_step(self.lba, &buf[..], &mut self.st)? {
            false => Some(Shot::Error),
            true => {
                self.filled = false;
                self.i = 0;
                self.vc = self.cl[0] as u32;
                self.cur = NONE; // FAT 1 changed
                self.phase = Phase::Verify;
                None
            }
        }
    }

    /// Re-read block `i` through the new FAT 1 chain; CRC at the end.
    fn verify(&mut self, disk: &mut dyn BlockDevice) -> Option<Shot> {
        let v = self.vol();
        if self.i > 0 && self.i % v.spc == 0 && !self.linked {
            let want = self.cl[(self.i / v.spc) as usize] as u32;
            let miss = self.vc / FPER != self.cur;
            if self.fat_get(disk, self.vc) != Some(want) {
                return Some(Shot::Error);
            }
            self.vc = want;
            self.linked = true;
            if miss {
                return None; // a FAT block was read: the data block next pass
            }
        }
        self.linked = false;
        let off = self.i * BLK as u32;
        let len = (FILE_LEN - off).min(BLK as u32) as usize;
        let buf = &mut calbackup::scratch()[2];
        if !disk.read_block(v.data_start + (self.vc - 2) * v.spc + self.i % v.spc, buf) {
            return Some(Shot::Error);
        }
        self.vcrc = crc32_upd(self.vcrc, &buf[..len]);
        self.i += 1;
        if self.i < NBLK {
            return None;
        }
        let eoc = self.fat_get(disk, self.vc).is_some_and(|n| n >= 0xFFF8);
        Some(if eoc && self.vcrc == self.crc { Shot::Saved(self.n) } else { Shot::Error })
    }
}

/// Run-length code one row into `out`; None = does not fit.
fn encode(px: &[u16], out: &mut [u16]) -> Option<usize> {
    let mut o = 0usize;
    let mut lit = 0usize;
    let mut i = 0usize;
    while i < px.len() {
        let mut j = i + 1;
        while j < px.len() && px[j] == px[i] {
            j += 1;
        }
        if j - i >= 3 {
            o = literal(&px[lit..i], out, o)?;
            out.get_mut(o..o + 2)?.copy_from_slice(&[(j - i) as u16, px[i]]);
            o += 2;
            lit = j;
        }
        i = j;
    }
    literal(&px[lit..], out, o)
}

fn literal(px: &[u16], out: &mut [u16], o: usize) -> Option<usize> {
    if px.is_empty() {
        return Some(o);
    }
    let d = out.get_mut(o..o + 1 + px.len())?;
    d[0] = 0x8000 | px.len() as u16;
    d[1..].copy_from_slice(px);
    Some(o + 1 + px.len())
}

/// Pixels `x0 .. x0 + n` of a row coded at the start of `cap`.
fn decode(cap: &[u16], x0: u32, n: u32, mut out: impl FnMut(u16)) {
    let end = x0 + n;
    let (mut x, mut p) = (0u32, 0usize);
    while x < end {
        let h = cap[p] as u32;
        let k = h & 0x7FFF;
        let (lo, hi) = (x.max(x0), (x + k).min(end));
        if h & 0x8000 != 0 {
            for q in lo..hi {
                out(cap[p + 1 + (q - x) as usize]);
            }
            p += 1 + k as usize;
        } else {
            for _ in lo..hi {
                out(cap[p + 1]);
            }
            p += 2;
        }
        x += k;
    }
}
