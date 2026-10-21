//! Read-only "information" volume: the second USB mass-storage LUN, a FAT16
//! file system synthesized on the fly from the MCU memory (DAPLink-style
//! virtual FAT). Nothing is stored: the boot sector, both FATs and the root
//! directory are generated from a static file table, file data is read from
//! the source regions when the host READs the sector. Always current, no SPI
//! wear, no boot-time disk writes (it replaces calbackup's per-boot copies on
//! the SPI disk; calbackup keeps only a write-once safety copy there).
//!
//! Volume "DM40-INFO", 512-byte sectors, 4 sectors (2 KiB) per cluster, 2
//! FATs of 129 sectors, 128 root entries, 32768 clusters (FAT16 by the
//! cluster-count rule: >= 4085), 131339 sectors = 64.1 MiB. The fixed size
//! leaves room for generated files of other modules (`set_extra`); the
//! built-in files use ~600 KiB, every other data sector reads as zeros (free
//! clusters). Built-in files (root, 8.3, read-only attribute), each on
//! consecutive clusters in this order:
//!   CALIB.BIN BOOTDATA.BIN UID.BIN OPTBYTES.BIN OTP.BIN ROMBOOT.BIN
//!   LOADER.BIN   calbackup::FILES: raw bytes of the region (address, size
//!                there), read on every access;
//!   BACKUP.TXT   generated text: each file's address, size and CRC-32, the
//!                UID, the firmware version and loader record, restore notes;
//!   FIRMWARE.BIN the running firmware as its vendor update file
//!                (calbackup::FwFile: whole 128-byte blocks for a `pack --pad`
//!                build, byte-identical to the dm40*.bin that installed it).
//!                Only when the loader record at 0x08008000 is a valid
//!                header. The name does not match the FS-loader's dm40*.bin
//!                pattern (irrelevant here: the loader never sees this volume,
//!                but a copy dropped onto the SPI disk as is stays inert).
//!
//! The sources do not change while fw-rs runs (fw-rs never programs the
//! internal flash except the recovery marker, followed by a reset), so the
//! CRC-32s for BACKUP.TXT are computed once at boot (`init`, ~0.1 s for the
//! ~570 KiB) and BACKUP.TXT has a fixed size (fixed-width fields).
//!
//! Generic hook (`set_extra`): after the built-ins, a list of up to
//! MAX_EXTRA more files, each `Extra { name, size, read, arg }` with its size
//! known up front and its bytes produced on demand by `read(arg, offset,
//! buf)` (e.g. CSV files rendered from the binary DMM logs on the SPI disk).
//! Replacing the list re-allocates their clusters (the built-ins keep
//! theirs) and raises a one-shot media change: the next LUN-1 command gets
//! UNIT ATTENTION 06/28/00 (msc.rs, via `BlockDevice::take_media_changed`),
//! so the host drops its cached FAT / directory and re-reads them. Like the
//! SPI disk's, it is held back while a host has LUN 1 locked (PREVENT:
//! mounted on Linux, where a UNIT ATTENTION would fail its I/O) and goes out
//! after the unmount. Call it from the main loop (the context msc.poll runs
//! in); a READ of LUN 1 in progress then finishes on the new table.
//!
//! Writes are refused (msc.rs: DATA PROTECT, WRITE PROTECTED 07/27/00; the
//! MODE SENSE header has the WP bit, so hosts mount it read-only).
//!
//! Host test: tools/fatren-test/run.sh (section 19: the whole volume dumped
//! through `read_block`, fsck.fat -n, mtools, contents vs the sources).

use crate::calbackup::{crc32_upd, FwFile, Source, FILES};
use crate::msc::BlockDevice;

pub const BS: usize = 512;
/// Sectors per cluster, and bytes per cluster.
const SPC: u32 = 4;
const CB: u32 = SPC * BS as u32;
const RSVD: u32 = 1;
const NFATS: u32 = 2;
const ROOT_ENTRIES: u32 = 128;
const ROOT_SECS: u32 = ROOT_ENTRIES * 32 / BS as u32;
pub const CLUSTERS: u32 = 32768;
const FAT_SECS: u32 = ((CLUSTERS + 2) * 2).div_ceil(BS as u32);
const FAT_START: u32 = RSVD;
const ROOT_START: u32 = RSVD + NFATS * FAT_SECS;
const DATA_START: u32 = ROOT_START + ROOT_SECS;
pub const TOTAL: u32 = DATA_START + CLUSTERS * SPC;
const _: () = assert!(CLUSTERS >= 4085 + 16 && CLUSTERS < 65525 - 16, "FAT16 by the cluster count, with a margin");
pub const LABEL: [u8; 11] = *b"DM40-INFO  ";
/// Directory dates 2026-10-13 (no RTC), as calbackup / screenshot.
const DATE: u16 = (46 << 9) | (10 << 5) | 13;
const TXT: [u8; 11] = *b"BACKUP  TXT";
const FW: [u8; 11] = *b"FIRMWAREBIN";
/// Built-ins: FILES + BACKUP.TXT + FIRMWARE.BIN.
pub const NF: usize = FILES.len() + 2;
/// Files of other modules (`set_extra`) at most.
pub const MAX_EXTRA: usize = 64;
const _: () = assert!(1 + NF + MAX_EXTRA <= ROOT_ENTRIES as usize, "root directory: label + files");
/// Upper bound of BACKUP.TXT (asserted against the worst case below).
const TXT_MAX: u32 = 4096;
const _: () = {
    let mut sum = 0u32;
    let mut i = 0;
    while i < FILES.len() {
        sum += FILES[i].2.div_ceil(CB);
        i += 1;
    }
    sum += TXT_MAX.div_ceil(CB);
    sum += (12 + crate::calbackup::FW_MAX_BODY as u32).div_ceil(CB);
    assert!(sum <= CLUSTERS / 8, "the built-ins must leave most of the data area to set_extra");
};
const UID_ADDR: u32 = 0x1FFF_7A10;
const VERSION: &str = match option_env!("CARGO_PKG_VERSION") {
    Some(v) => v,
    None => "?",
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Mem,
    Txt,
    Fw,
}

#[derive(Clone, Copy)]
struct VFile {
    name: [u8; 11],
    kind: Kind,
    addr: u32,
    size: u32,
    /// First cluster (0 = empty file).
    start: u32,
    crc: u32,
}

/// A file of another module on this volume (`set_extra`): 8.3 name (upper
/// case, space padded), size in bytes (fixed until the next `set_extra`),
/// and its contents: `read(arg, off, buf)` fills `buf` with bytes `off ..
/// off + buf.len()` (never past `size`; at most one sector, 512 bytes, per
/// call; the same bytes every time for the same table).
#[derive(Clone, Copy)]
pub struct Extra {
    pub name: [u8; 11],
    pub size: u32,
    pub read: fn(arg: u32, off: u32, buf: &mut [u8]),
    pub arg: u32,
}

fn no_read(_: u32, _: u32, _: &mut [u8]) {}

const NO_EXTRA: Extra = Extra { name: [b' '; 11], size: 0, read: no_read, arg: 0 };

struct State {
    files: [VFile; NF],
    n: usize,
    fw: Option<FwFile<'static>>,
    mem: fn(u32, &mut [u8]),
    uid: [u8; 12],
    extra: [Extra; MAX_EXTRA],
    /// First cluster of each extra (0 = empty file).
    extra_start: [u32; MAX_EXTRA],
    nx: usize,
}

/// The volume (~2 KiB). NOLOAD SRAM (.dmabuf, as calbackup's scratch): not
/// initialised at reset, written by `init` before READY is set; TCM is kept
/// for the stack.
#[cfg_attr(target_os = "none", link_section = ".dmabuf")]
static mut STATE: core::mem::MaybeUninit<State> = core::mem::MaybeUninit::uninit();
static READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// One-shot media change for LUN 1 (`set_extra`), taken by msc.rs.
static CHANGED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn state() -> Option<&'static State> {
    if READY.load(core::sync::atomic::Ordering::Acquire) {
        Some(unsafe { (*core::ptr::addr_of!(STATE)).assume_init_ref() })
    } else {
        None
    }
}

/// Target memory reader: byte loads (the OTP / option-byte areas included).
pub fn mem_read(addr: u32, buf: &mut [u8]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = unsafe { core::ptr::read_volatile((addr as usize + i) as *const u8) };
    }
}

fn mem_crc(mem: fn(u32, &mut [u8]), addr: u32, len: u32) -> u32 {
    let mut t = [0u8; 256];
    let mut c = !0u32;
    let mut off = 0u32;
    while off < len {
        let n = (len - off).min(t.len() as u32) as usize;
        mem(addr + off, &mut t[..n]);
        c = crc32_upd(c, &t[..n]);
        off += n as u32;
    }
    !c
}

/// Build the file table and the CRC-32s (boot, before USB; ~0.1 s). `mem`
/// reads the source regions, `flash` = the bytes from 0x08004000 (at most
/// calbackup::FW_MAX_BODY) for FIRMWARE.BIN. Never inlined: its buffers are
/// gone before main().
#[inline(never)]
pub fn init(mem: fn(u32, &mut [u8]), flash: &'static [u8]) {
    let none = VFile { name: [b' '; 11], kind: Kind::Mem, addr: 0, size: 0, start: 0, crc: 0 };
    let mut files = [none; NF];
    let mut n = 0;
    for &(name, addr, len) in FILES.iter() {
        files[n] = VFile { name, kind: Kind::Mem, addr, size: len, start: 0, crc: mem_crc(mem, addr, len) };
        crate::wdt::kick_if_feeding();
        n += 1;
    }
    let txt = n;
    files[n] = VFile { name: TXT, kind: Kind::Txt, ..none };
    n += 1;
    let fw = FwFile::from_flash(flash);
    if let Some(f) = fw {
        files[n] = VFile { name: FW, kind: Kind::Fw, addr: crate::calbackup::FW_BASE, size: f.size(), start: 0, crc: f.crc() };
        n += 1;
    }
    let mut uid = [0u8; 12];
    mem(UID_ADDR, &mut uid);
    let mut st = State { files, n, fw, mem, uid, extra: [NO_EXTRA; MAX_EXTRA], extra_start: [0; MAX_EXTRA], nx: 0 };
    // The text's length does not depend on the CRC values (fixed width).
    st.files[txt].size = describe(&st, 0, &mut []) as u32;
    let mut c = 2u32;
    for f in st.files[..n].iter_mut() {
        let k = f.size.div_ceil(CB);
        f.start = if k == 0 { 0 } else { c };
        c += k;
    }
    unsafe { (*core::ptr::addr_of_mut!(STATE)).write(st) };
    READY.store(true, core::sync::atomic::Ordering::Release);
}

/// First cluster after the built-ins.
#[allow(dead_code)] // with set_extra
fn builtin_end(st: &State) -> u32 {
    st.files[..st.n].iter().map(|f| f.start + f.size.div_ceil(CB)).max().unwrap_or(2).max(2)
}

/// Replace the extra files (after the built-ins, in this order) and raise
/// the LUN-1 media change. Files that do not fit (more than MAX_EXTRA, or
/// past the data area) are left out: returns how many were taken (a prefix
/// of `files`). No effect before `init`. (The hook for the binary-log CSV
/// files; no caller yet.)
#[allow(dead_code)]
pub fn set_extra(files: &[Extra]) -> usize {
    if !READY.load(core::sync::atomic::Ordering::Acquire) {
        return 0;
    }
    let st = unsafe { (*core::ptr::addr_of_mut!(STATE)).assume_init_mut() };
    let mut c = builtin_end(st);
    let mut nx = 0;
    for f in files.iter().take(MAX_EXTRA) {
        let k = f.size.div_ceil(CB);
        if c + k > CLUSTERS + 2 {
            break;
        }
        st.extra[nx] = *f;
        st.extra_start[nx] = if k == 0 { 0 } else { c };
        c += k;
        nx += 1;
    }
    st.nx = nx;
    CHANGED.store(true, core::sync::atomic::Ordering::Relaxed);
    nx
}

/// (name, size, first cluster) of root file `i` (built-ins, then extras).
fn entry(st: &State, i: usize) -> Option<([u8; 11], u32, u32)> {
    if i < st.n {
        let f = &st.files[i];
        Some((f.name, f.size, f.start))
    } else if i - st.n < st.nx {
        let f = &st.extra[i - st.n];
        Some((f.name, f.size, st.extra_start[i - st.n]))
    } else {
        None
    }
}

/// The LUN-1 block device (a handle; the volume itself is `STATE`).
pub struct VDisk;

impl BlockDevice for VDisk {
    fn present(&self) -> bool {
        state().is_some()
    }
    fn block_size(&self) -> u32 {
        BS as u32
    }
    fn block_count(&self) -> u32 {
        TOTAL
    }
    fn read_only(&self) -> bool {
        true
    }
    fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> bool {
        let Some(st) = state() else {
            return false;
        };
        if lba >= TOTAL || buf.len() < BS {
            return false;
        }
        let buf = &mut buf[..BS];
        buf.fill(0);
        if lba == 0 {
            boot_sector(st, buf);
        } else if lba < ROOT_START {
            fat_sector(st, (lba - FAT_START) % FAT_SECS, buf);
        } else if lba < DATA_START {
            root_sector(st, lba - ROOT_START, buf);
        } else {
            data_sector(st, lba - DATA_START, buf);
        }
        true
    }
    fn write_block(&mut self, _: u32, _: &[u8]) -> bool {
        false
    }
    fn take_media_changed(&mut self) -> bool {
        CHANGED.swap(false, core::sync::atomic::Ordering::Relaxed)
    }
}

fn put16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

fn boot_sector(st: &State, b: &mut [u8]) {
    b[..3].copy_from_slice(&[0xEB, 0x3C, 0x90]);
    b[3..11].copy_from_slice(b"DM40-FW ");
    put16(b, 0x0B, BS as u16);
    b[0x0D] = SPC as u8;
    put16(b, 0x0E, RSVD as u16);
    b[0x10] = NFATS as u8;
    put16(b, 0x11, ROOT_ENTRIES as u16);
    if TOTAL <= 0xFFFF {
        put16(b, 0x13, TOTAL as u16);
    } else {
        put32(b, 0x20, TOTAL);
    }
    b[0x15] = 0xF8; // fixed media
    put16(b, 0x16, FAT_SECS as u16);
    put16(b, 0x18, 63); // sectors per track
    put16(b, 0x1A, 255); // heads
    b[0x24] = 0x80; // drive number
    b[0x26] = 0x29; // extended boot signature
    // Volume serial: from the UID (stable per unit, differs between units;
    // hosts remember volumes by it).
    put32(b, 0x27, !crc32_upd(!0, &st.uid));
    b[0x2B..0x36].copy_from_slice(&LABEL);
    b[0x36..0x3E].copy_from_slice(b"FAT16   ");
    // Boot code: an endless loop (never booted).
    b[0x3E..0x40].copy_from_slice(&[0xEB, 0xFE]);
    b[0x1FE] = 0x55;
    b[0x1FF] = 0xAA;
}

/// FAT entry of cluster `c`: media / EOC marks, the next cluster of a
/// file's contiguous chain, 0xFFFF at its end, 0 = free.
fn fat_entry(st: &State, c: u32) -> u16 {
    match c {
        0 => 0xFFF8,
        1 => 0xFFFF,
        _ => {
            let mut i = 0;
            while let Some((_, size, start)) = entry(st, i) {
                let end = start + size.div_ceil(CB);
                if start != 0 && c >= start && c < end {
                    return if c + 1 == end { 0xFFFF } else { (c + 1) as u16 };
                }
                i += 1;
            }
            0
        }
    }
}

fn fat_sector(st: &State, s: u32, b: &mut [u8]) {
    let per = (BS / 2) as u32;
    for i in 0..per {
        put16(b, (i * 2) as usize, fat_entry(st, s * per + i));
    }
}

fn root_sector(st: &State, s: u32, b: &mut [u8]) {
    let per = (BS / 32) as u32;
    for i in 0..per {
        let idx = (s * per + i) as usize;
        let e = &mut b[(i * 32) as usize..(i * 32 + 32) as usize];
        if idx == 0 {
            e[..11].copy_from_slice(&LABEL);
            e[11] = 0x08; // volume label
            put16(e, 0x18, DATE);
        } else if let Some((name, size, start)) = entry(st, idx - 1) {
            e[..11].copy_from_slice(&name);
            e[11] = 0x01; // read-only
            put16(e, 0x10, DATE); // created
            put16(e, 0x12, DATE); // accessed
            put16(e, 0x18, DATE); // written
            put16(e, 0x1A, start as u16);
            put32(e, 0x1C, size);
        }
    }
}

/// Data sector `s` (0 = the first sector of cluster 2).
fn data_sector(st: &State, s: u32, b: &mut [u8]) {
    let c = s / SPC + 2;
    let mut i = 0;
    while let Some((_, size, start)) = entry(st, i) {
        i += 1;
        let k = size.div_ceil(CB);
        if start == 0 || c < start || c >= start + k {
            continue;
        }
        let off = (c - start) * CB + (s % SPC) * BS as u32;
        if off >= size {
            return; // slack of the last cluster
        }
        let len = (size - off).min(BS as u32) as usize;
        let d = &mut b[..len];
        if i - 1 >= st.n {
            let x = &st.extra[i - 1 - st.n];
            (x.read)(x.arg, off, d);
            return;
        }
        let f = &st.files[i - 1];
        match f.kind {
            Kind::Mem => (st.mem)(f.addr + off, d),
            Kind::Txt => {
                describe(st, off as usize, d);
            }
            Kind::Fw => {
                if let Some(fw) = st.fw.as_ref() {
                    fw.fill(off, d);
                }
            }
        }
        return;
    }
}

// --- BACKUP.TXT ---------------------------------------------------------------

/// Text writer over a window: byte `n` of the text goes to `b[n - base]`
/// when that is inside `b`; `n` counts the whole text (its length at the end).
struct W<'a> {
    b: &'a mut [u8],
    base: usize,
    n: usize,
}

impl W<'_> {
    fn c(&mut self, ch: u8) {
        if let Some(d) = self.n.checked_sub(self.base).and_then(|i| self.b.get_mut(i)) {
            *d = ch;
        }
        self.n += 1;
    }
    fn s(&mut self, t: &[u8]) {
        for &ch in t {
            self.c(ch);
        }
    }
    fn hex(&mut self, v: u32) {
        for i in (0..8).rev() {
            self.c(b"0123456789ABCDEF"[(v >> (4 * i)) as usize & 15]);
        }
    }
    /// Right-aligned in 7 columns.
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
        self.c(b'.');
        self.s(n[8..].split(|&c| c == b' ').next().unwrap_or(&[]));
        while self.n < start + 13 {
            self.c(b' ');
        }
    }
}

/// BACKUP.TXT bytes `base .. base + out.len()` into `out`; returns the
/// text's whole length (constant for a given file table).
fn describe(st: &State, base: usize, out: &mut [u8]) -> usize {
    let mut w = W { b: out, base, n: 0 };
    w.s(b"DM40 information volume: generated on the fly by dm40-fw ");
    w.s(VERSION.as_bytes());
    w.s(b" from the\r\nMCU memory, read-only. Raw copies of internal flash / chip data;\r\n");
    w.s(b"CRC-32 as zlib, 7-Zip, crc32.\r\n\r\n");
    w.s(b"file         address     bytes  crc32");
    for f in &st.files[..st.n] {
        w.s(b"\r\n");
        w.name(&f.name);
        if f.kind == Kind::Txt {
            w.s(b"(this file)");
            continue;
        }
        w.hex(f.addr);
        w.s(b" ");
        w.dec(f.size);
        w.s(b"  ");
        w.hex(f.crc);
    }
    w.s(b"\r\n\r\nUID ");
    for ch in st.uid.chunks(4) {
        w.c(b' ');
        w.hex(u32::from_le_bytes([ch[0], ch[1], ch[2], ch[3]]));
    }
    w.s(b"\r\nFirmware dm40-fw ");
    w.s(VERSION.as_bytes());
    match st.fw.as_ref() {
        Some(fw) => {
            w.s(b", loader record at 08008000:");
            for ch in fw.header().chunks(4) {
                w.c(b' ');
                w.hex(u32::from_le_bytes([ch[0], ch[1], ch[2], ch[3]]));
            }
            w.s(b"\r\n\r\nFIRMWARE.BIN = the running firmware as its vendor update file. To\r\n");
            w.s(b"reinstall it: copy it to the DM40 disk as dm40fw.bin, power-cycle\r\n");
            w.s(b"(or DBG:UPDATE).\r\n");
        }
        None => w.s(b"; no FIRMWARE.BIN (no valid loader record at 08008000).\r\n"),
    }
    w.s(b"\r\nFull restore after a mass erase (SWD): write ROMBOOT.BIN at 0x08000000,\r\n");
    w.s(b"then LOADER.BIN at 0x080A0000 and CALIB.BIN at 0x080C0000, then install\r\n");
    w.s(b"a firmware. BOOTDATA.BIN = LOADER.BIN bytes 0x4000..0x7FFF. The loader\r\n");
    w.s(b"data is bound to UID.BIN: this unit only.\r\n");
    w.s(b"Copy these files to a PC: this volume is made from the MCU flash and\r\n");
    w.s(b"goes with it. The DM40 disk keeps one write-once safety copy of\r\n");
    w.s(b"CALIB.BIN, BOOTDATA.BIN and UID.BIN (written when missing, never\r\n");
    w.s(b"overwritten; it survives an SWD mass erase of the MCU).\r\n");
    debug_assert!(w.n <= TXT_MAX as usize);
    w.n
}
