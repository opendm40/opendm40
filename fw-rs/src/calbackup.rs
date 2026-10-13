//! Calibration / boot-data backup to the SPI-flash FAT16 volume (the USB
//! disk), so the owner always has a copy of the unit's irreplaceable data on
//! the PC side (docs/user-feedback.md UF-20), enough for a full restore after
//! an SWD mass erase: ROMBOOT.BIN + LOADER.BIN + CALIB.BIN written back to
//! their addresses, plus any firmware (stock update file through the loader,
//! or fw-rs native), give a working unit again.
//!
//! Files (root directory, 8.3 names, exact raw bytes; none matches the
//! FS-loader's `dm40*.bin` pattern). Bit i of `written` / `conflict` = row i:
//!   0 CALIB.BIN     0x080C0000   4096  settings + calibration record (sector 10).
//!                   The stock record is 800 bytes (u16 crc, u16 ~crc, u32
//!                   version 1, data; calibration struct at +0x168, hy3131_v2_6.h);
//!                   in this unit's 1 MiB dump the whole 128 KiB sector is 0xFF
//!                   beyond 0x31F. 4 KiB = exactly one cluster / one flash block
//!                   and leaves room for a longer record of another stock version.
//!   1 BOOTDATA.BIN  0x080A4000  16384  UID-bound FS-loader data (sector 9). The
//!                   device-specific block starts at 0x080A4000 (2 KiB dump
//!                   dumps/dump_boot_data_uid.bin; the first 0x14 bytes are data,
//!                   b[0x10..0x13] = the version bytes BT command 02 reports).
//!                   A subset of LOADER.BIN (its +0x4000..+0x8000), kept for
//!                   backward compatibility: earlier builds wrote it, calbackup
//!                   never deletes, so dropping it from this list would leave a
//!                   copy that goes stale when the loader rewrites that block.
//!   2 UID.BIN       0x1FFF7A10     12  96-bit unique ID.
//!   3 OPTBYTES.BIN  0x1FFFC000     16  option bytes.
//!   4 OTP.BIN       0x1FFF7800    528  OTP area + lock bytes (0xFF on this unit).
//!   5 ROMBOOT.BIN   0x08000000  16384  primary bootloader (sector 0).
//!   6 LOADER.BIN    0x080A0000 131072  the whole update-loader sector 9, incl.
//!                   the UID-bound data (non-0xFF run 0x080A4000..0x080AE6D3 on
//!                   this unit, so BOOTDATA.BIN alone does not cover all of it).
//!   7 BACKUP.TXT                       name, address, size, CRC-32 of each file
//!                                      + restore note (deterministic: no time,
//!                                      no counters); also the CRC-first index.
//!   8 _DM40PRV.BIN  0x08004000  12+len the running firmware's update file,
//!                   regenerated from flash (`FwFile`), byte-identical to the
//!                   dm40*.bin the loader installed (and deleted). Only when the
//!                   marker at 0x08008000 is a valid update header. Rename it to
//!                   DM40PRV.BIN + power-cycle / DBG:UPDATE to reinstall it.
//! New files are appended (bit indices of the older ones never change); 16 is
//! the limit of the u16 masks. Internal flash and the system areas are only
//! READ. Disk: ~430 KiB (~107 clusters of 4 KiB with fw-rs) of the 16 MiB volume.
//!
//! fatren (runs first) renames an installed dm40*.bin to _DM40PRV.BIN, deleting
//! the old one: that file is the same firmware (header == marker), so this
//! module finds it equal and only re-stamps its entry once; no fight. The
//! regenerated file has 0xFF in the 12-byte marker gap (body 0x4000..0x400B),
//! as build.sh and the vendor's file do; a file with other bytes there would
//! be rewritten once.
//!
//! When: every normal boot, before USB exists (the host cannot have the volume
//! mounted): native from native.rs `pre_main` (after board init and fatren),
//! the RAM build from main.rs `rust_start` before `main()`. Cargo feature
//! `calbackup` (implied by native; off in the RAM images for CODE space:
//! `./build.sh --features calbackup` currently does not fit, see Cargo.toml).
//! In the RAM build
//! the stock firmware may have had its own USB disk mounted on the host just
//! before the BT load; fw-rs comes back as a different USB device, so the
//! host re-reads the volume.
//!
//! CRC-first (boot backup only, `quick_trust`): BACKUP.TXT on the disk is read
//! once; a file whose exact line there (name, address, size, CRC-32 of the
//! source NOW) is present, and whose entry has that size, our write stamp and a
//! well-formed chain, is not read at all. Unchanged boot = 4 block reads (boot
//! sector, root, FAT 1, BACKUP.TXT). NOT detected: a data block changed in
//! place without a size / chain / stamp change; delete BACKUP.TXT to force the
//! full compare of every file on the next boot.
//!
//! Wear: every other file is compared with its source by reading it (directory
//! entry size + whole cluster chain + contents). Equal => nothing is written
//! (only a foreign write stamp is replaced by ours, once). Only a missing or
//! different file is (re)written; there is only ever one copy per name (no
//! versions).
//!
//! Write order for the files that need it (one pass each, so a power loss
//! anywhere leaves at worst lost clusters, never a live entry on free or
//! half-written clusters, never two entries with the same name):
//!   1. allocate NEW clusters first-fit from FAT 1 (copy-on-write: the old
//!      chain stays intact and referenced until step 4);
//!   2. write the data blocks (read, overlay, write, read back);
//!   3. link the new chains (EOC at the end) in every FAT copy, one write per
//!      touched FAT block and copy (0x0000 -> link needs an erase, so batched);
//!   4. directory entries: new start cluster + size (+ write date, archive
//!      bit), one write per touched root block for all files together;
//!   5. free the old chains in every FAT copy (only clears bits: programmed
//!      without erase), only if they were well formed (exact length, valid
//!      links, EOC) - a broken old chain is left as lost clusters rather than
//!      risk freeing clusters of another file;
//!   6. re-read every written file through its new entry and compare.
//! Every block write is verified by a read-back. The volume is not touched
//! unless fatren::parse_bpb accepts it (FAT16, 4096-byte sectors = device
//! block, sane geometry). A power loss during the erase of a FAT or root block
//! can still damage that block: a property of this NOR-backed FAT (host writes
//! have the same window); the steps above keep that to at most 2 x FAT blocks
//! (both copies, one after the other) + 1 root block per changed boot.
//!
//! Time: unchanged = 4 block reads (~13 ms each at the 3.125 MHz polled SPI
//! clock) + CRC-32 over ~165 KiB of sources and the ~250 KiB firmware file
//! (nibble table) ~= 0.1 s (DBG:BOOT? CALMS; was ~0.65 s reading 45 data
//! blocks). Writes: empty disk ~107 blocks (~10 s), each firmware update
//! _DM40PRV.BIN + BACKUP.TXT (~62 blocks, ~6 s), a loader update ~3 s. The
//! FWDGT is fed between blocks (wdt::kick_if_feeding; spiflash feeds during
//! erases).
//!
//! The same writer (compare, allocate, data, link, entry, free, re-read)
//! also stores screenshots (screenshot.rs): `write_file` with a streaming
//! `Source` instead of a byte slice, and `free_number` for the next S%03u.BMP.
//!
//! Host test: tools/fatren-test/run.sh.

#![allow(dead_code)]

use crate::fatren::{free_chain, le16, le32, parse_bpb, sfn_is, verify, Vol};
use crate::msc::BlockDevice;

const BLK: usize = 4096;
/// Most clusters one run allocates: all backup files on an empty disk = 46
/// + the largest firmware update file (12 + 0x9C000 bytes = 157 clusters);
/// a 480x320 screenshot is 307266 bytes = 76 clusters.
const MAX_NEW: usize = 210;
/// Root slots per 4 KiB block.
const PER: u32 = (BLK / 32) as u32;
/// Write date 2026-10-13 (FAT: (year - 1980) << 9 | month << 5 | day), time 0.
/// Constant (no RTC): a varying date would make every rewrite differ.
const DATE: u16 = (46 << 9) | (10 << 5) | 13;
/// Directory entry bytes 0x16..0x1A (write time, write date) as this writer
/// sets them: time 0, DATE. A host that modifies a file sets its own stamp,
/// which is how the CRC-first check (`quick_trust`) notices host edits.
const STAMP: u32 = (DATE as u32) << 16;

/// The binary files: 8.3 name, source address, length.
/// Append only (bit i of the masks = entry i; BACKUP.TXT is bit FILES.len()).
pub const FILES: [([u8; 11], u32, u32); 7] = [
    (*b"CALIB   BIN", 0x080C_0000, 4096),
    (*b"BOOTDATABIN", 0x080A_4000, 16384),
    (*b"UID     BIN", 0x1FFF_7A10, 12),
    (*b"OPTBYTESBIN", 0x1FFF_C000, 16),
    (*b"OTP     BIN", 0x1FFF_7800, 528),
    (*b"ROMBOOT BIN", 0x0800_0000, 16384),
    (*b"LOADER  BIN", 0x080A_0000, 131072),
];
const TXT: [u8; 11] = *b"BACKUP  TXT";
/// The running firmware's update file (bit FW_BIT), see `FwFile`. Same name
/// fatren gives an installed dm40*.bin it renames.
pub const FW_NAME: [u8; 11] = crate::fatren::BACKUP;
/// Bit of BACKUP.TXT and of the firmware file in `written` / `conflict`.
pub const TXT_BIT: usize = FILES.len();
pub const FW_BIT: usize = FILES.len() + 1;
/// FILES + BACKUP.TXT + _DM40PRV.BIN.
pub const N: usize = FILES.len() + 2;
const _: () = assert!(N <= 16, "written / conflict are u16 bit masks");
/// BACKUP.TXT buffer (the text is ~1.1 KiB with the firmware line).
const TXT_MAX: usize = 2048;
const _: () = assert!(TXT_MAX <= BLK, "BACKUP.TXT must fit one block for quick_trust");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// Not run (recovery, feature off).
    NotRun,
    NoMedium,
    /// Boot sector is not the expected FAT16 volume; untouched.
    BadBpb,
    /// Every file already equal: nothing written.
    Unchanged,
    /// The files in `written` were (re)written and verified.
    Written,
    /// Root directory full or not enough free clusters; nothing written.
    NoSpace,
    /// A read, write or verify failed (see `written` for what completed).
    Error,
}

#[derive(Clone, Copy, Debug)]
pub struct Result {
    pub status: Status,
    /// Bit i = file i (FILES order, then BACKUP.TXT = TXT_BIT, then
    /// _DM40PRV.BIN = FW_BIT) (re)written, or only re-stamped (an equal file
    /// with a foreign write stamp gets ours once, see `quick_trust`).
    pub written: u16,
    /// Bit i = the name is taken by a directory / volume label: skipped.
    pub conflict: u16,
    /// Old chains freed (clusters).
    pub freed: u32,
    /// `boot()` duration in ms (DWT cycles at 200 MHz; 12.5x too low when
    /// the core runs at 16 MHz). 0 = not measured.
    pub ms: u32,
}

static mut RESULT: Result = Result { status: Status::NotRun, written: 0, conflict: 0, freed: 0, ms: 0 };

/// Work buffers of run_inner (buf, tmp, fat) and free_number, 3 x 4 KiB, kept
/// off the TCM stack: a screenshot calls the writer from deep in the main
/// loop, whose frames (main ~26 KiB) leave too little of the ~38 KiB stack
/// for three more blocks. NOLOAD SRAM (.dmabuf, as the scope record): the
/// contents are undefined at reset and always written before they are read.
/// One user at a time (the boot backup before main(), a screenshot from the
/// main loop; both synchronous, no interrupts).
#[cfg_attr(target_os = "none", link_section = ".dmabuf")]
static mut SCRATCH: [[u8; BLK]; 3] = [[0; BLK]; 3];

fn scratch() -> &'static mut [[u8; BLK]; 3] {
    unsafe { &mut *core::ptr::addr_of_mut!(SCRATCH) }
}

pub fn result() -> Result {
    unsafe { RESULT }
}

/// One file: 8.3 name, source address (for BACKUP.TXT), contents.
#[derive(Clone, Copy)]
pub struct Src<'a> {
    pub name: [u8; 11],
    pub addr: u32,
    pub data: &'a dyn Source,
}

/// File contents for the writer: a byte slice, or bytes generated on demand
/// (screenshot.rs streams the LCD GRAM; nothing holds the whole file).
pub trait Source {
    /// File size, bytes.
    fn size(&self) -> u32;
    /// Copy bytes `off .. off + buf.len()` (never past `size`) into `buf`.
    /// Must return the same bytes every time (the writer reads them twice:
    /// to write and to verify).
    fn fill(&self, off: u32, buf: &mut [u8]);
    /// CRC-32 of the whole contents (BACKUP.TXT).
    fn crc(&self) -> u32 {
        let mut t = [0u8; 256];
        let mut c = !0u32;
        let mut off = 0u32;
        while off < self.size() {
            let n = (self.size() - off).min(t.len() as u32) as usize;
            self.fill(off, &mut t[..n]);
            c = crc32_upd(c, &t[..n]);
            off += n as u32;
        }
        !c
    }
}

impl Source for &[u8] {
    fn size(&self) -> u32 {
        self.len() as u32
    }
    fn fill(&self, off: u32, buf: &mut [u8]) {
        let o = off as usize;
        buf.copy_from_slice(&self[o..o + buf.len()]);
    }
    fn crc(&self) -> u32 {
        crc32(self)
    }
}

// --- The running firmware as its update file ------------------------------------

/// Flash address of the update body (stock and fw-rs) and of the loader
/// marker inside it; the body may reach at most up to the FS-loader.
pub const FW_BASE: u32 = 0x0800_4000;
pub const FW_MARKER_OFF: usize = 0x4000;
pub const FW_MAX_BODY: usize = 0x080A_0000 - 0x0800_4000;

/// `_DM40PRV.BIN`: the update file of the firmware that is running,
/// regenerated from flash, byte-identical to the dm40*.bin the FS-loader
/// installed (which the loader deletes after a good install). Decoded file =
/// the 12-byte header || body; the header is exactly the 12-byte record the
/// loader wrote to 0x08008000 after verifying it (its CRC covers word1 =
/// body length, word2 = type + body CRC); the body is flash 0x08004000 ..
/// +length, except its bytes 0x4000..0x400B (the marker itself, excluded
/// from the body CRC by the loader) which are 0xFF as in the packed file
/// (dm40fw.py pack; the vendor's dm40_v_2_6.bin has 0xFF there too). Then
/// encoded in 128-byte blocks (fatren::encode_block). Rename it to
/// dm40*.bin and the loader reinstalls exactly this firmware.
#[derive(Clone, Copy)]
pub struct FwFile<'a> {
    hdr: [u8; 12],
    body: &'a [u8],
}

impl<'a> FwFile<'a> {
    /// `flash` = the bytes from 0x08004000 (at most FW_MAX_BODY). None when
    /// the marker is not a valid update header (fatren::header_valid) or its
    /// length does not fit.
    pub fn from_flash(flash: &'a [u8]) -> Option<Self> {
        let m = flash.get(FW_MARKER_OFF..FW_MARKER_OFF + 12)?;
        let mut hdr = [0u8; 12];
        hdr.copy_from_slice(m);
        if !crate::fatren::header_valid(&hdr) {
            return None;
        }
        let len = crate::fatren::le32(&hdr, 4) as usize;
        if len < FW_MARKER_OFF + 12 || len > FW_MAX_BODY || len > flash.len() {
            return None;
        }
        Some(FwFile { hdr, body: &flash[..len] })
    }

    /// Decoded (plain) file byte `p`.
    fn plain(&self, p: usize) -> u8 {
        if p < 12 {
            self.hdr[p]
        } else if (FW_MARKER_OFF..FW_MARKER_OFF + 12).contains(&(p - 12)) {
            0xFF
        } else {
            self.body[p - 12]
        }
    }
}

impl Source for FwFile<'_> {
    fn size(&self) -> u32 {
        12 + self.body.len() as u32
    }
    fn fill(&self, off: u32, buf: &mut [u8]) {
        let size = self.size() as usize;
        let (start, end) = (off as usize, off as usize + buf.len());
        let mut p = start;
        while p < end {
            let b0 = p / 128 * 128;
            let blen = (size - b0).min(128);
            let mut blk = [0u8; 128];
            let q = b0.wrapping_sub(12); // body offset of the block start
            if b0 >= 12 && (q + blen <= FW_MARKER_OFF || q >= FW_MARKER_OFF + 12) {
                blk[..blen].copy_from_slice(&self.body[q..q + blen]);
            } else {
                for (i, d) in blk[..blen].iter_mut().enumerate() {
                    *d = self.plain(b0 + i);
                }
            }
            crate::fatren::encode_block(&mut blk[..blen]);
            let e = (end - b0).min(blen);
            buf[p - start..b0 + e - start].copy_from_slice(&blk[p - b0..e]);
            p = b0 + e;
        }
    }
}

/// BACKUP.TXT as expected for this run, for the CRC-first check: the text,
/// and each file's line in it (byte range, empty = no line).
struct Quick<'a> {
    text: &'a [u8],
    lines: &'a [(u16, u16); N],
    txt: usize,
}

/// A file for run_inner: 8.3 name + contents.
#[derive(Clone, Copy)]
struct F<'a> {
    name: [u8; 11],
    src: &'a dyn Source,
}

/// Boot entry: back up the internal sources and the running firmware's update
/// file (the RAM build and native). Never inlined: its ~15 KiB frame is gone
/// before main() runs (review 2026-10-13 B1).
#[inline(never)]
pub fn boot() {
    if !cfg!(feature = "calbackup") {
        return;
    }
    let t0 = unsafe { crate::mmio::r32(DWT_CYCCNT) };
    let mut slices: [&[u8]; FILES.len()] = [&[]; FILES.len()];
    for (s, &(_, addr, len)) in slices.iter_mut().zip(FILES.iter()) {
        // Read-only views of internal flash / system memory.
        *s = unsafe { core::slice::from_raw_parts(addr as *const u8, len as usize) };
    }
    static EMPTY: &[u8] = &[];
    let mut srcs = [Src { name: [0; 11], addr: 0, data: &EMPTY }; FILES.len()];
    for ((s, &(name, addr, _)), d) in srcs.iter_mut().zip(FILES.iter()).zip(slices.iter()) {
        *s = Src { name, addr, data: d };
    }
    // The installed image: 0x08004000 up to the FS-loader, read-only.
    let flash = unsafe { core::slice::from_raw_parts(FW_BASE as *const u8, FW_MAX_BODY) };
    let fw = FwFile::from_flash(flash);
    let mut disk = crate::spiflash::SpiFlash::new();
    run(&mut disk, &srcs, fw.as_ref().map(|f| f as &dyn Source));
    let cyc = unsafe { crate::mmio::r32(DWT_CYCCNT) }.wrapping_sub(t0);
    unsafe { RESULT.ms = cyc / (crate::clock::HCLK_HZ / 1000) };
}

const DWT_CYCCNT: u32 = 0xE000_1004;

/// Back up `srcs` (+ BACKUP.TXT describing them, + the firmware file `fw` if
/// given). CRC-first: BACKUP.TXT on the disk is read once; a file whose line
/// there (name, address, size, CRC-32 of the source NOW) is present and whose
/// directory entry still has its size, our write stamp and a well-formed
/// chain is not read at all (`quick_trust`). Every other file is compared in
/// full and (re)written if different. Records and returns the result.
#[inline(never)]
pub fn run(dev: &mut dyn BlockDevice, srcs: &[Src], fw: Option<&dyn Source>) -> Result {
    let mut text = [0u8; TXT_MAX];
    let mut lines = [(0u16, 0u16); N];
    let k = srcs.len().min(TXT_BIT);
    let n = describe(&srcs[..k], fw, &mut text, &mut lines);
    let txt: &[u8] = &text[..n];
    static EMPTY: &[u8] = &[];
    let mut fs = [F { name: TXT, src: &EMPTY }; N];
    for (f, s) in fs.iter_mut().zip(srcs[..k].iter()) {
        *f = F { name: s.name, src: s.data };
    }
    fs[k] = F { name: TXT, src: &txt };
    let mut cnt = k + 1;
    if let Some(f) = fw {
        // Bit FW_BIT when all FILES are given (describe numbers it k + 1 too).
        fs[cnt] = F { name: FW_NAME, src: f };
        cnt += 1;
    }
    let q = Quick { text: txt, lines: &lines, txt: k };
    let mut r = run_inner(dev, &fs[..cnt], Some(&q));
    r.ms = unsafe { RESULT.ms }; // previous value; boot() stores this run's after
    unsafe { RESULT = r };
    r
}

/// Write one file `name` (8.3, upper case) from `src` through the same
/// ordered copy-on-write path as the backup: unchanged = nothing written; new
/// or different = new clusters, data, FAT links, directory entry, old chain
/// freed, re-read and compared with `src`. Does not touch `result()`.
pub fn write_file(dev: &mut dyn BlockDevice, name: [u8; 11], src: &dyn Source) -> Result {
    run_inner(dev, &[F { name, src }], None)
}

/// 8.3 name `<prefix>NNN.<ext>` (prefix 1..5 upper-case chars, NNN = `n` with
/// three digits), e.g. ("S", 7, "BMP") -> "S007    BMP" = S007.BMP.
pub fn numbered_name(prefix: &[u8], n: u16, ext: &[u8; 3]) -> [u8; 11] {
    let mut name = [b' '; 11];
    let k = prefix.len().min(5);
    name[..k].copy_from_slice(&prefix[..k]);
    name[k] = b'0' + (n / 100 % 10) as u8;
    name[k + 1] = b'0' + (n / 10 % 10) as u8;
    name[k + 2] = b'0' + (n % 10) as u8;
    name[8..].copy_from_slice(ext);
    name
}

/// Lowest n in 0..=999 with no root entry `numbered_name(prefix, n, ext)`
/// (any entry of that name counts, also a directory; deleted and LFN slots do
/// not) - the stock screenshot naming "s%.3d.bmp", first free number. None:
/// no medium, not the expected FAT16 volume, a read error, or all 1000 taken.
pub fn free_number(dev: &mut dyn BlockDevice, prefix: &[u8], ext: &[u8; 3]) -> Option<u16> {
    if !dev.present() || dev.block_size() as usize != BLK {
        return None;
    }
    let buf = &mut scratch()[0];
    if !dev.read_block(0, buf) {
        return None;
    }
    let v = parse_bpb(buf, dev.block_count())?;
    let k = prefix.len().min(5);
    let mut used = [0u32; 32]; // bit n = number n taken (0..=999)
    'scan: for blk in 0..v.root_blocks {
        if !dev.read_block(v.root_start + blk, buf) {
            return None;
        }
        for i in 0..PER {
            if blk * PER + i >= v.root_entries {
                break 'scan;
            }
            let e = &buf[(i * 32) as usize..(i * 32 + 32) as usize];
            if e[0] == 0x00 {
                break 'scan; // end of directory
            }
            if e[0] == 0xE5 || e[11] & 0x3F == 0x0F {
                continue;
            }
            let d = &e[k..k + 3];
            if !d.iter().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let n = (d[0] - b'0') as u16 * 100 + (d[1] - b'0') as u16 * 10 + (d[2] - b'0') as u16;
            if sfn_is(e, &numbered_name(prefix, n, ext)) {
                used[(n / 32) as usize] |= 1 << (n % 32);
            }
        }
    }
    (0..1000u16).find(|&n| used[(n / 32) as usize] & (1 << (n % 32)) == 0)
}

// --- BACKUP.TXT ----------------------------------------------------------------

/// CRC-32 (ISO-HDLC, as zlib / `crc32` / 7-Zip report it). Nibble table
/// (64 bytes): ~4x the bit loop; ~165 KiB of sources + the firmware file
/// (~250 KiB) at every boot.
pub fn crc32(d: &[u8]) -> u32 {
    !crc32_upd(!0, d)
}

/// CRC-32 register update (no pre / post inversion).
fn crc32_upd(mut c: u32, d: &[u8]) -> u32 {
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

struct W<'a> {
    b: &'a mut [u8],
    n: usize,
}

impl W<'_> {
    fn s(&mut self, t: &[u8]) {
        for &c in t {
            if self.n < self.b.len() {
                self.b[self.n] = c;
                self.n += 1;
            }
        }
    }
    fn hex(&mut self, v: u32) {
        for i in (0..8).rev() {
            self.s(&[b"0123456789ABCDEF"[(v >> (4 * i)) as usize & 15]]);
        }
    }
    fn dec(&mut self, mut v: u32) {
        let mut t = [b' '; 7];
        for i in (0..7).rev() {
            t[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        self.s(&t);
    }
    /// 8.3 name as "NAME.EXT", padded to 13.
    fn name(&mut self, n: &[u8; 11]) {
        let start = self.n;
        self.s(n[..8].split(|&c| c == b' ').next().unwrap_or(&[]));
        self.s(b".");
        self.s(n[8..].split(|&c| c == b' ').next().unwrap_or(&[]));
        while self.n < start + 13 {
            self.s(b" ");
        }
    }
}

/// BACKUP.TXT for `srcs` (+ `fw`, numbered srcs.len() + 1). `lines[i]` = the
/// byte range of file i's line, its leading CR LF included (so a line can
/// only match a whole line of the disk copy).
fn describe(srcs: &[Src], fw: Option<&dyn Source>, out: &mut [u8], lines: &mut [(u16, u16); N]) -> usize {
    let mut w = W { b: out, n: 0 };
    w.s(b"DM40 backup written by fw-rs at boot (only when missing or changed).\r\n");
    w.s(b"Raw copies of internal flash / chip data; CRC-32 as zlib, 7-Zip, crc32.\r\n");
    w.s(b"file         address     bytes  crc32");
    let fw_src = fw.map(|f| (srcs.len() + 1, Src { name: FW_NAME, addr: FW_BASE, data: f }));
    for (i, s) in srcs.iter().enumerate().chain(fw_src.iter().map(|(i, s)| (*i, s))) {
        let start = w.n;
        w.s(b"\r\n");
        w.name(&s.name);
        w.hex(s.addr);
        w.s(b" ");
        w.dec(s.data.size());
        w.s(b"  ");
        w.hex(s.data.crc());
        if i < N {
            lines[i] = (start as u16, w.n as u16);
        }
    }
    w.s(b"\r\n\r\n");
    w.s(b"Full restore after a mass erase (SWD): write ROMBOOT.BIN at 0x08000000,\r\n");
    w.s(b"then LOADER.BIN at 0x080A0000 and CALIB.BIN at 0x080C0000, then install\r\n");
    w.s(b"a firmware. BOOTDATA.BIN = LOADER.BIN bytes 0x4000..0x7FFF (kept for\r\n");
    w.s(b"older copies). The loader data is bound to UID.BIN: this unit only.\r\n");
    if fw.is_some() {
        w.s(b"_DM40PRV.BIN = update file of the running firmware, from its flash;\r\n");
        w.s(b"to reinstall it: rename to DM40PRV.BIN, power-cycle (or DBG:UPDATE).\r\n");
    }
    w.n
}

// --- FAT writer --------------------------------------------------------------------

/// Bytes per cluster.
fn cbytes(v: &Vol) -> usize {
    v.spc as usize * BLK
}

fn nclusters(v: &Vol, len: u32) -> u32 {
    len.div_ceil(cbytes(v) as u32)
}

fn lba_of(v: &Vol, c: u32) -> u32 {
    v.data_start + (c - 2) * v.spc
}

/// FAT 1 entry `c`, with `fat`/`cur` as a one-block cache.
fn fat_get(dev: &mut dyn BlockDevice, v: &Vol, c: u32, fat: &mut [u8; BLK], cur: &mut u32) -> Option<u32> {
    let per = (BLK / 2) as u32;
    let fb = c / per;
    if fb >= v.fat_blocks {
        return None;
    }
    if fb != *cur {
        if !dev.read_block(v.fat_start + fb, fat) {
            return None;
        }
        *cur = fb;
    }
    Some(le16(fat, ((c % per) * 2) as usize))
}

/// Follow the chain of a file (`start`, `size` from its entry) through FAT 1.
/// Returns (well_formed, equal): well formed = exactly the clusters `size`
/// needs, every link valid, EOC at the end; equal = also well formed,
/// size == data.size() and the contents equal `data`. None = read error.
#[allow(clippy::too_many_arguments)]
fn walk(dev: &mut dyn BlockDevice, v: &Vol, start: u32, size: u32, data: &dyn Source, fat: &mut [u8; BLK], buf: &mut [u8; BLK], tmp: &mut [u8; BLK]) -> Option<(bool, bool)> {
    let n = nclusters(v, size);
    let max = v.clusters + 1;
    let len = data.size() as usize;
    if n == 0 {
        return Some((start == 0, start == 0 && len == 0));
    }
    if n > v.clusters {
        return Some((false, false));
    }
    let mut equal = size as usize == len;
    let mut cur = u32::MAX;
    let mut c = start;
    for i in 0..n {
        if c < 2 || c > max {
            return Some((false, false));
        }
        if equal {
            for b in 0..v.spc {
                let off = i as usize * cbytes(v) + b as usize * BLK;
                if off >= len {
                    break;
                }
                if !dev.read_block(lba_of(v, c) + b, buf) {
                    return None;
                }
                let end = (off + BLK).min(len);
                data.fill(off as u32, &mut tmp[..end - off]);
                if buf[..end - off] != tmp[..end - off] {
                    equal = false;
                    break;
                }
            }
        }
        crate::wdt::kick_if_feeding();
        let next = fat_get(dev, v, c, fat, &mut cur)?;
        if i + 1 == n {
            let ok = next >= 0xFFF8;
            return Some((ok, ok && equal));
        }
        c = next;
    }
    Some((false, false))
}

#[derive(Clone, Copy)]
struct Job {
    file: usize,
    /// Root slot (existing entry, or the free slot for a new one).
    slot: u32,
    new: bool,
    /// Old chain to free after the entry switch (0 = none / not well formed).
    old: u32,
    /// New clusters: cl[first..first + n].
    first: usize,
    n: usize,
    /// Contents equal, only the entry's write stamp is set to ours (no new
    /// clusters; start / size unchanged; nothing to free or re-read).
    stamp_only: bool,
}

const NO_JOB: Job = Job { file: 0, slot: 0, new: false, old: 0, first: 0, n: 0, stamp_only: false };

/// Follow a chain of `size` bytes from `start` through FAT 1 (`fat`/`cur`
/// one-block cache): Some(true) = well formed (exactly the clusters `size`
/// needs, valid links, EOC at the end), Some(false) = not, None = read error.
/// Reads FAT blocks only.
fn chain_ok(dev: &mut dyn BlockDevice, v: &Vol, start: u32, size: u32, fat: &mut [u8; BLK], cur: &mut u32) -> Option<bool> {
    let n = nclusters(v, size);
    if n == 0 {
        return Some(start == 0);
    }
    if n > v.clusters {
        return Some(false);
    }
    let mut c = start;
    for i in 0..n {
        if c < 2 || c > v.clusters + 1 {
            return Some(false);
        }
        let next = fat_get(dev, v, c, fat, cur)?;
        if i + 1 == n {
            return Some(next >= 0xFFF8);
        }
        c = next;
    }
    Some(false)
}

/// Does `hay` contain `needle`?
fn contains(hay: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || hay.windows(needle.len()).any(|w| w == needle)
}

/// CRC-first check (boot backup): the files that need no data read at all.
/// Reads BACKUP.TXT's single block and FAT 1 blocks (one, normally) only.
/// File i is trusted when ALL of:
///   - BACKUP.TXT on the disk has the exact line this run would write for it
///     (name, address, size, CRC-32 of the source as it is NOW), and
///     BACKUP.TXT itself carries our write stamp and a well-formed chain;
///   - its directory entry has that size and our write stamp (time 0, DATE;
///     any host modification sets the host's clock) and a well-formed chain.
/// BACKUP.TXT is trusted when it is byte-equal to this run's text (stamp,
/// chain too). Not detected: a data block changed in place without a size,
/// chain or stamp change (flash bit rot, a raw sector writer); deleting
/// BACKUP.TXT forces the full compare of every file on the next boot.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn quick_trust(dev: &mut dyn BlockDevice, v: &Vol, files: &[F], found: &[Option<(u32, u32, u32, u32)>; N], q: &Quick, fat: &mut [u8; BLK], buf: &mut [u8; BLK]) -> u16 {
    let Some((_, tstart, tsize, tstamp)) = found.get(q.txt).copied().flatten() else {
        return 0;
    };
    let mut cur = u32::MAX;
    if tsize == 0 || tsize as usize > BLK || tstamp != STAMP || chain_ok(dev, v, tstart, tsize, fat, &mut cur) != Some(true) {
        return 0;
    }
    if !dev.read_block(lba_of(v, tstart), buf) {
        return 0;
    }
    let disk = &buf[..tsize as usize];
    let mut ok = 0u16;
    if disk == q.text {
        ok |= 1 << q.txt;
    }
    for (i, f) in files.iter().enumerate() {
        let (a, b) = q.lines[i];
        if i == q.txt || a == b {
            continue;
        }
        let Some((_, start, size, stamp)) = found[i] else {
            continue;
        };
        if size == f.src.size()
            && stamp == STAMP
            && contains(disk, &q.text[a as usize..b as usize])
            && chain_ok(dev, v, start, size, fat, &mut cur) == Some(true)
        {
            ok |= 1 << i;
        }
    }
    ok
}

fn run_inner(dev: &mut dyn BlockDevice, files: &[F], quick: Option<&Quick>) -> Result {
    let mut res = Result { status: Status::Error, written: 0, conflict: 0, freed: 0, ms: 0 };
    if !dev.present() || dev.block_size() as usize != BLK {
        res.status = Status::NoMedium;
        return res;
    }
    let [buf, tmp, fat] = scratch();
    if !dev.read_block(0, buf) {
        return res;
    }
    let Some(v) = parse_bpb(&buf[..], dev.block_count()) else {
        res.status = Status::BadBpb;
        return res;
    };

    // Root scan: the live entry of each name (slot, start, size, write
    // stamp), free slots.
    let mut found: [Option<(u32, u32, u32, u32)>; N] = [None; N];
    let mut free = [0u32; N];
    let mut nfree = 0usize;
    'scan: for blk in 0..v.root_blocks {
        if !dev.read_block(v.root_start + blk, buf) {
            return res;
        }
        for k in 0..PER {
            let idx = blk * PER + k;
            if idx >= v.root_entries {
                break 'scan;
            }
            let e = &buf[(k * 32) as usize..(k * 32 + 32) as usize];
            if e[0] == 0x00 {
                // End of directory: this and every later slot are free (FAT
                // spec: all entries after a 0x00 one are 0x00 too).
                let mut i = idx;
                while nfree < N && i < v.root_entries {
                    free[nfree] = i;
                    nfree += 1;
                    i += 1;
                }
                break 'scan;
            }
            if e[0] == 0xE5 {
                if nfree < N {
                    free[nfree] = idx;
                    nfree += 1;
                }
                continue;
            }
            if e[11] & 0x3F == 0x0F {
                continue; // LFN
            }
            for (i, f) in files.iter().enumerate() {
                if found[i].is_none() && res.conflict & (1 << i) == 0 && sfn_is(e, &f.name) {
                    if e[11] & 0x18 != 0 {
                        res.conflict |= 1 << i; // a directory / label of that name
                    } else {
                        found[i] = Some((idx, le16(e, 0x1A), le32(e, 0x1C), le32(e, 0x16)));
                    }
                }
            }
        }
    }

    // CRC-first (boot backup only): files proven unchanged without reading them.
    let trusted = match quick {
        Some(q) => quick_trust(dev, &v, files, &found, q, fat, buf),
        None => 0,
    };

    // Which files differ? Plan new chains for them.
    let mut jobs = [NO_JOB; N];
    let mut nj = 0usize;
    let mut need = 0usize;
    let mut nnew = 0usize;
    for (i, f) in files.iter().enumerate() {
        if (res.conflict | trusted) & (1 << i) != 0 {
            continue;
        }
        let mut j = Job { file: i, first: need, n: nclusters(&v, f.src.size()) as usize, ..NO_JOB };
        match found[i] {
            Some((slot, start, size, stamp)) => {
                let Some((wf, eq)) = walk(dev, &v, start, size, f.src, fat, buf, tmp) else {
                    return res;
                };
                if eq {
                    // Equal but stamped by someone else (a host copy, fatren's
                    // rename of the installed dm40*.bin): give it our stamp
                    // once, so the next boots trust it without a full read.
                    if quick.is_some() && stamp != STAMP {
                        jobs[nj] = Job { file: i, slot, stamp_only: true, ..NO_JOB };
                        nj += 1;
                    }
                    continue;
                }
                j.slot = slot;
                j.old = if wf { start } else { 0 };
            }
            None => {
                if nnew == nfree {
                    res.status = Status::NoSpace;
                    return res;
                }
                j.slot = free[nnew];
                j.new = true;
                nnew += 1;
            }
        }
        need += j.n;
        jobs[nj] = j;
        nj += 1;
    }
    if nj == 0 {
        res.status = Status::Unchanged;
        return res;
    }
    if need > MAX_NEW {
        res.status = Status::NoSpace;
        return res;
    }

    // 1. First-fit free clusters (FAT 1 entry 0).
    let mut cl = [0u32; MAX_NEW];
    let mut got = 0usize;
    let mut cur = u32::MAX;
    let mut c = 2u32;
    while got < need && c <= v.clusters + 1 {
        match fat_get(dev, &v, c, fat, &mut cur) {
            None => return res,
            Some(0) => {
                cl[got] = c;
                got += 1;
            }
            Some(_) => {}
        }
        c += 1;
    }
    if got < need {
        res.status = Status::NoSpace;
        return res;
    }

    // 2. Data blocks (the bytes past the end keep what the block held).
    for j in &jobs[..nj] {
        let d = files[j.file].src;
        let len = d.size() as usize;
        for (i, &c) in cl[j.first..j.first + j.n].iter().enumerate() {
            for b in 0..v.spc {
                let off = i * cbytes(&v) + b as usize * BLK;
                if off >= len {
                    break;
                }
                let lba = lba_of(&v, c) + b;
                let end = (off + BLK).min(len);
                // A partial last block keeps what it held past the end.
                if end - off < BLK && !dev.read_block(lba, buf) {
                    return res;
                }
                d.fill(off as u32, &mut buf[..end - off]);
                if !(dev.write_block(lba, &buf[..]) && verify(dev, lba, buf, tmp)) {
                    return res;
                }
                crate::wdt::kick_if_feeding();
            }
        }
    }

    // 3. Chains in every FAT copy, one write per touched block and copy.
    let per = (BLK / 2) as u32;
    for k in 0..v.nfats {
        let mut done = 0u32; // FAT blocks already written (bit = block, < 32)
        for &c0 in &cl[..need] {
            let fb = c0 / per;
            if fb < 32 && done & (1 << fb) != 0 {
                continue;
            }
            let lba = v.fat_start + k * v.fat_blocks + fb;
            if !dev.read_block(lba, buf) {
                return res;
            }
            for j in &jobs[..nj] {
                for i in 0..j.n {
                    let c = cl[j.first + i];
                    if c / per == fb {
                        let next = if i + 1 == j.n { 0xFFFF } else { cl[j.first + i + 1] as u16 };
                        let o = ((c % per) * 2) as usize;
                        buf[o..o + 2].copy_from_slice(&next.to_le_bytes());
                    }
                }
            }
            if !(dev.write_block(lba, &buf[..]) && verify(dev, lba, buf, tmp)) {
                return res;
            }
            if fb < 32 {
                done |= 1 << fb;
            }
            crate::wdt::kick_if_feeding();
        }
    }

    // 4. Directory entries, one write per touched root block. The block
    // holding BACKUP.TXT's entry goes last: a power loss between root-block
    // writes must not leave the new BACKUP.TXT (new CRC lines) next to an
    // old entry of a same-size file, which quick_trust would then accept.
    let txt_blk = quick.and_then(|q| jobs[..nj].iter().find(|j| j.file == q.txt)).map(|j| j.slot / PER);
    let last = txt_blk.unwrap_or(u32::MAX);
    for blk in (0..v.root_blocks).filter(|&b| b != last).chain(txt_blk) {
        let (lo, hi) = (blk * PER, blk * PER + PER);
        if !jobs[..nj].iter().any(|j| j.slot >= lo && j.slot < hi) {
            continue;
        }
        let lba = v.root_start + blk;
        if !dev.read_block(lba, buf) {
            return res;
        }
        for j in jobs[..nj].iter().filter(|j| j.slot >= lo && j.slot < hi) {
            let o = ((j.slot - lo) * 32) as usize;
            let e = &mut buf[o..o + 32];
            if j.new {
                e.fill(0);
                e[..11].copy_from_slice(&files[j.file].name);
                e[0x10..0x12].copy_from_slice(&DATE.to_le_bytes()); // created
            }
            e[11] |= 0x20; // archive
            e[0x12..0x14].copy_from_slice(&DATE.to_le_bytes()); // accessed
            e[0x14..0x16].fill(0); // cluster high (FAT32 only)
            e[0x16..0x18].fill(0); // write time
            e[0x18..0x1A].copy_from_slice(&DATE.to_le_bytes()); // write date
            if !j.stamp_only {
                let start = if j.n == 0 { 0 } else { cl[j.first] as u16 };
                e[0x1A..0x1C].copy_from_slice(&start.to_le_bytes());
                e[0x1C..0x20].copy_from_slice(&files[j.file].src.size().to_le_bytes());
            }
        }
        if !(dev.write_block(lba, &buf[..]) && verify(dev, lba, buf, tmp)) {
            return res;
        }
        crate::wdt::kick_if_feeding();
    }
    for j in &jobs[..nj] {
        res.written |= 1 << j.file;
    }

    // 5. Old chains (well-formed ones only).
    for j in &jobs[..nj] {
        if j.old != 0 {
            let (ok, freed) = free_chain(dev, &v, j.old, fat, buf, tmp);
            res.freed += freed;
            if !ok {
                return res;
            }
        }
    }

    // 6. Read every written file back through its new chain.
    for j in jobs[..nj].iter().filter(|j| !j.stamp_only) {
        let d = files[j.file].src;
        let start = if j.n == 0 { 0 } else { cl[j.first] };
        match walk(dev, &v, start, d.size(), d, fat, buf, tmp) {
            Some((true, true)) => {}
            _ => return res,
        }
    }
    res.status = Status::Written;
    res
}
