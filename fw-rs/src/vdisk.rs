//! The USB disk: a FAT16 volume synthesized on the fly
//! (DAPLink-style virtual FAT), the only mass-storage LUN of the native
//! build. Nothing of it is stored: the boot sector, both FATs and every
//! directory are generated from tables, file data is produced when the host
//! READs the sector. The SPI flash (our FAT16 internal storage, fatdir.rs)
//! is no longer exported, so no host can write it or hold a stale copy of
//! its FAT, and the device never has to wait for an eject.
//!
//! Volume "DM40", 512-byte sectors, 64 sectors (32 KiB) per cluster, 2 FATs
//! of 254 sectors, 512 root entries, 65000 clusters (FAT16 by the cluster
//! count), 4160541 sectors = 1.98 GiB: room for SPIFLASH.BIN, the CSV
//! files (a DLG log expands ~10-20x into CSV, a scope record more; a full
//! 16 MiB SPI disk of logs is a few hundred MiB of CSV) and a margin for
//! the append-only allocation below. Layout (fixed clusters first):
//!   README.TXT     what is where, how to update the firmware (cluster 7);
//!   INFO/          (cluster 2) CALIB / BOOTDATA / UID / OPTBYTES / OTP /
//!                  ROMBOOT / LOADER .BIN (raw bytes of the MCU regions,
//!                  calbackup::FILES), BACKUP.TXT (addresses, sizes, CRC-32s,
//!                  restore notes), FIRMWARE.BIN (the running firmware as its
//!                  vendor update file, calbackup::FwFile; only with a valid
//!                  loader record); from cluster 8, each contiguous;
//!   DEBUG/         (cluster 3) SPIFLASH.BIN: the raw 16 MiB of the SPI
//!                  flash (`fsck.fat -n` / mtools on it show the internal
//!                  FAT16), read on every access;
//!   LOGS/          (cluster 4) LOGnnn.CSV: the CSV generated from each
//!                  LOGnnn.DLG (dlog.rs `Gen`, the exact text the old CSV
//!                  writer produced);
//!   SCOPE/         (cluster 5) SCOPnnn.CSV from SCOPnnn.DLG;
//!   SHOTS/         (cluster 6) Snnn.BMP: the screenshots, streamed from the
//!                  SPI file (its FAT chain);
//! the LOGS / SCOPE / SHOTS files come from the SPI disk's LOGS/, SCOPE/,
//! SHOTS/ and from its root (earlier builds), numbered names only, up to
//! MAX_VF of them. A file being recorded (csvlog) is left out until it
//! ends. Every subdirectory is one cluster (1024 slots: "." / ".." and up to
//! 1022 files).
//!
//! Changes (`refresh`, from the main loop when dlog.rs `stale()` and the
//! disk has been quiet for a second, see main.rs): the SPI disk is scanned
//! (directories only; a DLG's CSV size is computed once, when it is new)
//! and the table updated:
//!   * an existing file keeps its directory slot and its clusters for as
//!     long as it exists (a host's cached directory / FAT / inode stays
//!     right for it);
//!   * a new file gets the next slot of its directory and clusters from the
//!     high-water mark (append-only: a freed slot shows as deleted, 0xE5,
//!     and its clusters stay unused);
//!   * a deleted / changed file disappears (`forget` does it at once for a
//!     delete on the device, so a new file of the same name never inherits
//!     its place);
//!   * when the slots of a directory or the clusters run out, the table is
//!     compacted (every file renumbered from the start), but only while no
//!     host holds the medium locked (msc.rs `host_mounted`: Linux sends
//!     PREVENT on mount); else the new files wait for the unmount. After a
//!     USB bus reset (re-plug) the next refresh compacts.
//! Any change raises a one-shot media change: the next command gets UNIT
//! ATTENTION 06/28/00 at once, also while the volume is mounted and locked
//! (msc.rs; the 2026-10-21 experiment: Linux then re-reads and shows the new
//! files, a file manager after a refresh; holding it back while locked hid
//! them until the unmount).
//!
//! Host writes (vwrite.rs): the volume is writable (no WP bit; DBG:VDISK:RO
//! 1 sets it), but a write is only accepted as a delete of a LOGS / SCOPE /
//! SHOTS file, a firmware file DM40*.BIN in the root, the hosts' own
//! housekeeping (trash, indexer: kept for the session) or metadata noise
//! (dirty flags, dates); everything else fails with DATA PROTECT. While a
//! host holds the volume (`vwrite::frozen`) `refresh` waits: the layout
//! changes only by the host's own writes (tomb flags `T_*` in the table,
//! vwrite.rs records) until the session ends.
//!
//! RAM: the file table (MAX_VF x 16 bytes) and the built-in table in SRAM
//! (.sram_bss / .dmabuf), the CSV / BMP read cursors in .bss.
//!
//! Host test: tools/fatren-test/run.sh section 19 (the whole volume dumped
//! through `read_block`, fsck.fat -n, mtools, contents vs the sources, the
//! allocation rules across refreshes).

use crate::calbackup::{crc32_upd, FwFile, Source, FILES};
use crate::dlog::{self, FatFile, FnRaw, Gen, Geo, Raw, Src as _};
use crate::msc::BlockDevice;

pub const BS: usize = 512;
/// Sectors per cluster, and bytes per cluster.
pub(crate) const SPC: u32 = 64;
pub(crate) const CB: u32 = SPC * BS as u32;
const RSVD: u32 = 1;
const NFATS: u32 = 2;
const ROOT_ENTRIES: u32 = 512;
const ROOT_SECS: u32 = ROOT_ENTRIES * 32 / BS as u32;
pub const CLUSTERS: u32 = 65_000;
pub(crate) const FAT_SECS: u32 = ((CLUSTERS + 2) * 2).div_ceil(BS as u32);
pub(crate) const FAT_START: u32 = RSVD;
pub(crate) const ROOT_START: u32 = RSVD + NFATS * FAT_SECS;
pub(crate) const DATA_START: u32 = ROOT_START + ROOT_SECS;
pub const TOTAL: u32 = DATA_START + CLUSTERS * SPC;
const _: () = assert!(CLUSTERS >= 4085 + 16 && CLUSTERS < 65525 - 16, "FAT16 by the cluster count, with a margin");
pub const LABEL: [u8; 11] = *b"DM40       ";
/// Directory dates 2026-10-13 (no RTC), as calbackup / screenshot.
const DATE: u16 = (46 << 9) | (10 << 5) | 13;
const TXT: [u8; 11] = *b"BACKUP  TXT";
const FW: [u8; 11] = *b"FIRMWAREBIN";
/// Built-ins of INFO/: FILES + BACKUP.TXT + FIRMWARE.BIN.
pub const NF: usize = FILES.len() + 2;
/// Upper bound of BACKUP.TXT (asserted against the worst case below).
const TXT_MAX: u32 = 4096;
const UID_ADDR: u32 = 0x1FFF_7A10;
const VERSION: &str = match option_env!("CARGO_PKG_VERSION") {
    Some(v) => v,
    None => "?",
};

/// Fixed clusters: the five subdirectories (one cluster each), README.TXT,
/// then the INFO/ files from C_FILES.
const C_INFO: u32 = 2;
const C_DEBUG: u32 = 3;
/// LOGS, SCOPE, SHOTS: C_VDIR + d (d = the file kind, `VDIRS`).
const C_VDIR: u32 = 4;
const C_README: u32 = 7;
const C_FILES: u32 = 8;
/// DEBUG/SPIFLASH.BIN: the W25Q128, 16 MiB.
pub const SPI_BYTES: u32 = 16 << 20;
/// Root entries after the label: README.TXT, INFO, DEBUG, LOGS, SCOPE, SHOTS.
pub(crate) const ROOT_N: usize = 6;
/// Generated directories, file kinds: (name, file prefix, file extension).
const VDIRS: [([u8; 11], &[u8], [u8; 3]); 3] = [
    (*b"LOGS       ", b"LOG", *b"CSV"),
    (*b"SCOPE      ", b"SCOP", *b"CSV"),
    (*b"SHOTS      ", b"S", *b"BMP"),
];
/// Slots of a one-cluster directory.
const SLOTS: u32 = CB / 32;
/// Files of LOGS / SCOPE / SHOTS at most (16 bytes of SRAM each).
pub const MAX_VF: usize = 400;

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

struct State {
    files: [VFile; NF],
    n: usize,
    fw: Option<FwFile<'static>>,
    mem: fn(u32, &mut [u8]),
    uid: [u8; 12],
    /// DEBUG/SPIFLASH.BIN's first cluster, and the first dynamic cluster.
    spi_start: u32,
    dyn0: u32,
}

/// One generated file of LOGS / SCOPE / SHOTS. `id` = flags | kind << 10 |
/// number (kind + number = the name, `ID`; flags `T_*`: a host delete in
/// progress, vwrite.rs), `slot` its directory slot, `vstart` its first cluster here,
/// `src` / `ssize` the SPI file (first cluster, bytes), `vsize` its size
/// here (the CSV's for a DLG).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct VF {
    pub id: u16,
    pub slot: u16,
    pub vstart: u16,
    pub src: u16,
    pub ssize: u32,
    pub vsize: u32,
}

const VF0: VF = VF { id: 0, slot: 0, vstart: 0, src: 0, ssize: 0, vsize: 0 };
/// `VF::id`: kind + number (the name).
pub(crate) const ID: u16 = 0x0FFF;
/// The host marked its directory entry deleted (0xE5): the slot shows the
/// entry with 0xE5 (vwrite.rs).
pub(crate) const T_DIR: u16 = 1 << 12;
/// The host freed its chain in the FAT: shown free, data reads zeros.
pub(crate) const T_FAT: u16 = 1 << 13;
/// The SPI file is gone (deleted while the layout is frozen, by the host's
/// delete or on the meter): data reads zeros, the entry stays until the
/// session ends.
pub(crate) const T_SRC: u16 = 1 << 14;
/// Its entry (start cluster + size) was written into a host-created
/// directory (a move to the trash): deleted at the session's end.
pub(crate) const T_MOVED: u16 = 1 << 15;
/// Every flag.
pub(crate) const T_ALL: u16 = T_DIR | T_FAT | T_SRC | T_MOVED;

fn kind(id: u16) -> usize {
    ((id >> 10) & 3) as usize
}

/// The dynamic table, ascending `vstart` (allocation is append-only, a
/// compaction keeps the order), so the FAT and data lookups bisect it.
struct Tab {
    e: [VF; MAX_VF],
    n: usize,
    /// Next free cluster, next free slot per directory.
    hw: u32,
    slot_hw: [u16; 3],
    /// Compact at the next refresh (bus reset), or when a file did not fit.
    want_compact: bool,
    /// Files left out (no slot / cluster / table room) at the last refresh.
    left_out: u16,
}

/// NOLOAD SRAM (.dmabuf): written by `init` before READY is set; TCM is kept
/// for the stack.
#[cfg_attr(target_os = "none", link_section = ".dmabuf")]
static mut STATE: core::mem::MaybeUninit<State> = core::mem::MaybeUninit::uninit();
#[cfg_attr(target_os = "none", link_section = ".dmabuf")]
static mut TAB: core::mem::MaybeUninit<Tab> = core::mem::MaybeUninit::uninit();
static READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// One-shot media change, taken by msc.rs.
static CHANGED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn state() -> Option<&'static State> {
    if READY.load(core::sync::atomic::Ordering::Acquire) {
        Some(unsafe { (*core::ptr::addr_of!(STATE)).assume_init_ref() })
    } else {
        None
    }
}

fn tab() -> &'static mut Tab {
    unsafe { (*core::ptr::addr_of_mut!(TAB)).assume_init_mut() }
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

/// Build the built-in tables and the CRC-32s (boot, before USB; ~0.1 s).
/// `mem` reads the source regions, `flash` = the bytes from 0x08004000 (at
/// most calbackup::FW_MAX_BODY) for FIRMWARE.BIN. The LOGS / SCOPE / SHOTS
/// files come with the first `refresh`. Never inlined: its buffers are gone
/// before main().
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
    let mut st = State { files, n, fw, mem, uid, spi_start: 0, dyn0: 0 };
    // The text's length does not depend on the CRC values (fixed width).
    st.files[txt].size = describe(&st, 0, &mut []) as u32;
    let mut c = C_FILES;
    for f in st.files[..n].iter_mut() {
        let k = f.size.div_ceil(CB);
        f.start = if k == 0 { 0 } else { c };
        c += k;
    }
    st.spi_start = c;
    st.dyn0 = c + SPI_BYTES / CB;
    unsafe {
        (*core::ptr::addr_of_mut!(STATE)).write(st);
        (*core::ptr::addr_of_mut!(TAB)).write(Tab { e: [VF0; MAX_VF], n: 0, hw: 0, slot_hw: [2; 3], want_compact: false, left_out: 0 });
    }
    tab().hw = st_dyn0();
    READY.store(true, core::sync::atomic::Ordering::Release);
}

fn st_dyn0() -> u32 {
    unsafe { (*core::ptr::addr_of!(STATE)).assume_init_ref().dyn0 }
}

// --- the dynamic files -----------------------------------------------------------

/// (kind, number) of an SPI file of ours with a numbered name (LOGnnn.DLG,
/// SCOPnnn.DLG, Snnn.BMP), else None.
fn ours(n: &[u8; 11]) -> Option<(usize, u16)> {
    let k = if dlog::dlg_name(n) == Some(3) {
        0
    } else if dlog::dlg_name(n).is_some() {
        1
    } else if n[8..].eq_ignore_ascii_case(b"BMP") && n[0].eq_ignore_ascii_case(&b'S') {
        2
    } else {
        return None;
    };
    let p = VDIRS[k].1.len();
    let d = &n[p..p + 3];
    if !d.iter().all(|c| c.is_ascii_digit()) || n[p + 3..8].iter().any(|&c| c != b' ') {
        return None;
    }
    Some((k, (d[0] - b'0') as u16 * 100 + (d[1] - b'0') as u16 * 10 + (d[2] - b'0') as u16))
}

/// 8.3 name of table id `id` on this volume ("LOG007  CSV").
pub(crate) fn vname(id: u16) -> [u8; 11] {
    let (pre, ext) = (VDIRS[kind(id)].1, &VDIRS[kind(id)].2);
    crate::calbackup::numbered_name(pre, id & 0x3FF, ext)
}

/// 8.3 name of table id `id` on the SPI disk ("LOG007  DLG").
pub(crate) fn spi_name(id: u16) -> [u8; 11] {
    let ext: &[u8; 3] = if kind(id) == 2 { b"BMP" } else { b"DLG" };
    crate::calbackup::numbered_name(VDIRS[kind(id)].1, id & 0x3FF, ext)
}

pub(crate) fn nclus(size: u32) -> u32 {
    size.div_ceil(CB)
}

/// Every SPI file of ours (root, LOGS/, SCOPE/, SHOTS/) to `f(id, start,
/// size)`, except `exclude` (8.3 SPI name). False: no disk / a read failed.
pub(crate) fn scan_spi(exclude: Option<[u8; 11]>, f: &mut dyn FnMut(u16, u16, u32)) -> bool {
    let mut d = dlog::disk();
    let Some(g) = dlog::geometry(&mut d) else {
        return false;
    };
    let dirs = [None, dlog::dir_start(&mut d, &g, &dlog::DIR_LOGS), dlog::dir_start(&mut d, &g, &dlog::DIR_SCOPE), dlog::dir_start(&mut d, &g, &dlog::DIR_SHOTS)];
    for (i, start) in dirs.iter().enumerate() {
        if i > 0 && start.is_none() {
            continue;
        }
        let ok = dlog::each_entry(&mut d, &g, *start, &mut |e| {
            if e[0] == 0xE5 || e[11] & 0x18 != 0 || e[11] & 0x3F == 0x0F {
                return true;
            }
            let mut n = [0u8; 11];
            n.copy_from_slice(&e[..11]);
            if Some(n) == exclude {
                return true;
            }
            if let Some((k, num)) = ours(&n) {
                let start = e[0x1A] as u16 | (e[0x1B] as u16) << 8;
                let size = u32::from_le_bytes([e[0x1C], e[0x1D], e[0x1E], e[0x1F]]);
                f((k as u16) << 10 | num, start, size);
            }
            true
        });
        if !ok {
            return false;
        }
    }
    true
}

/// Size on this volume of SPI file `id` (start, size): the generated CSV's
/// for a DLG (None: not a valid DLG), the file's own for a BMP.
fn vsize_of(id: u16, src: u16, ssize: u32) -> Option<u32> {
    if kind(id) == 2 {
        return (ssize > 0).then_some(ssize);
    }
    let mut d = dlog::disk();
    let g = dlog::geometry(&mut d)?;
    #[cfg(target_os = "none")]
    crate::wdt::kick_if_feeding();
    Gen::open(FatFile::new(d, g, src as u32, ssize), ssize).map(|g| g.info.csv)
}

/// Remove table entry `i` (its slot reads as deleted, its clusters stay
/// unused until a compaction).
fn remove(t: &mut Tab, i: usize) {
    t.e.copy_within(i + 1..t.n, i);
    t.n -= 1;
}

/// Renumber every file from the start: slots 2.. per directory, clusters
/// from dyn0, in table order. True if anything moved.
fn compact(t: &mut Tab) -> bool {
    let mut c = st_dyn0();
    let mut sl = [2u16; 3];
    let mut moved = false;
    for e in t.e[..t.n].iter_mut() {
        let k = kind(e.id);
        moved |= e.slot != sl[k] || e.vstart as u32 != c;
        e.slot = sl[k];
        e.vstart = c as u16;
        sl[k] += 1;
        c += nclus(e.vsize);
    }
    moved |= t.hw != c || t.slot_hw != sl;
    t.hw = c;
    t.slot_hw = sl;
    t.want_compact = false;
    moved
}

/// Add the SPI files not in the table yet. Returns (added, left out for
/// want of slots / clusters, left out because the table is full).
fn add_new(t: &mut Tab, exclude: Option<[u8; 11]>) -> (u32, u32, u32) {
    let (mut added, mut out, mut full) = (0u32, 0u32, 0u32);
    scan_spi(exclude, &mut |id, src, ssize| {
        if t.e[..t.n].iter().any(|e| e.id & ID == id) {
            return; // present (or a duplicate number: the first one stays)
        }
        let Some(vsize) = vsize_of(id, src, ssize) else { return };
        let k = kind(id);
        let need = nclus(vsize);
        if t.n == MAX_VF {
            full += 1;
            return;
        }
        if t.slot_hw[k] as u32 >= SLOTS || t.hw + need > CLUSTERS + 2 {
            out += 1;
            return;
        }
        t.e[t.n] = VF { id, slot: t.slot_hw[k], vstart: t.hw as u16, src, ssize, vsize };
        t.n += 1;
        t.slot_hw[k] += 1;
        t.hw += need;
        added += 1;
    });
    (added, out, full)
}

/// Rebuild LOGS / SCOPE / SHOTS from the SPI disk (main loop, when dlog.rs
/// `stale()` and no on-device writer holds the disk; see the module text;
/// not while frozen).
/// `exclude` = the SPI file being recorded. Raises the media change when the
/// volume changed. Returns the number of files.
pub fn refresh(exclude: Option<[u8; 11]>) -> usize {
    if state().is_none() {
        return 0;
    }
    // A host that can write holds the volume (vwrite.rs): its cached FAT /
    // directories must stay what it wrote; the change waits (stale stays
    // set) for the session's end.
    if crate::vwrite::frozen() {
        return tab().n;
    }
    dlog::clear_stale();
    let t = tab();
    // 1. Keep the files that are still there unchanged (same first cluster
    //    and size), drop the others.
    let mut seen = [0u32; MAX_VF.div_ceil(32)];
    let ok = scan_spi(exclude, &mut |id, src, ssize| {
        if let Some(i) = t.e[..t.n].iter().position(|e| e.id & ID == id && e.src == src && e.ssize == ssize) {
            seen[i / 32] |= 1 << (i % 32);
        }
    });
    if !ok {
        return t.n; // no disk / a read error: the volume stays as it is
    }
    let mut changed = false;
    for i in (0..t.n).rev() {
        if seen[i / 32] & (1 << (i % 32)) == 0 {
            remove(t, i);
            changed = true;
        }
    }
    // 2. Compaction (after a bus reset, or files left out last time), only
    //    while no host has the volume locked.
    let free = !crate::msc::host_mounted();
    if t.want_compact && free {
        changed |= compact(t);
    }
    // 3. New files, append-only; what does not fit for want of slots /
    //    clusters triggers a compaction (now if free, else after the unmount:
    //    `on_unlock`) and a second try. A full table stays full (MAX_VF).
    let (mut added, mut out, mut full) = add_new(t, exclude);
    if out > 0 && free && compact(t) {
        changed = true;
        let (a2, o2, f2) = add_new(t, exclude);
        added += a2;
        out = o2;
        full = f2;
    }
    t.want_compact |= out > 0;
    t.left_out = (out + full) as u16;
    if changed || added > 0 {
        CHANGED.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    t.n
}

/// The first `refresh`, at boot before USB: no host has seen the volume
/// yet, so no media change is raised for it.
#[inline(never)]
pub fn boot_refresh() {
    refresh(None);
    CHANGED.store(false, core::sync::atomic::Ordering::Relaxed);
}

/// A file of ours was deleted on the device (csvlog::delete_on, 8.3 SPI
/// name): drop it from the volume at once (before the next refresh, so a
/// new file of the same name can never inherit its slot / clusters while a
/// host caches them).
pub fn forget(name: &[u8; 11]) {
    if state().is_none() {
        return;
    }
    let Some((k, num)) = ours(name) else { return };
    let t = tab();
    let id = (k as u16) << 10 | num;
    if let Some(i) = t.e[..t.n].iter().position(|e| e.id & ID == id) {
        if crate::vwrite::frozen() || t.e[i].id & T_ALL != 0 {
            // The host may hold it (or is deleting it): the entry stays as
            // the host knows it, its data reads zeros from now on.
            t.e[i].id |= T_SRC;
        } else {
            remove(t, i);
            CHANGED.store(true, core::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// The table (vwrite.rs).
pub(crate) fn entries() -> &'static mut [VF] {
    if state().is_none() {
        return &mut [];
    }
    let t = tab();
    &mut t.e[..t.n]
}

/// Who owns cluster `c` in the generated layout (overlays not counted).
pub(crate) enum Owner {
    /// Media / reserved entries 0 and 1, or past the last cluster.
    Reserved,
    /// A directory, README.TXT, an INFO/ file, SPIFLASH.BIN.
    Fixed,
    /// Table entry `i`.
    Dyn(usize),
    Free,
}

pub(crate) fn owner(c: u32) -> Owner {
    if c < 2 || c >= CLUSTERS + 2 || state().is_none() {
        return Owner::Reserved;
    }
    if c < st_dyn0() {
        return Owner::Fixed;
    }
    let t = tab();
    let i = t.e[..t.n].partition_point(|e| e.vstart as u32 <= c);
    match i.checked_sub(1) {
        Some(j) if c < t.e[j].vstart as u32 + nclus(t.e[j].vsize) => Owner::Dyn(j),
        _ => Owner::Free,
    }
}

/// The generated directory entry of table entry `e` (vwrite.rs compares
/// host writes with it).
pub(crate) fn dyn_entry(e: &VF, out: &mut [u8]) {
    out[..32].fill(0);
    dirent(out, &vname(e.id), 0x01, e.vstart as u32, e.vsize);
}

/// Which generated directory cluster `c` is: Some(kind) for LOGS / SCOPE /
/// SHOTS, Some(3) for INFO / DEBUG, None otherwise.
pub(crate) fn dir_kind(c: u32) -> Option<usize> {
    match c {
        C_INFO | C_DEBUG => Some(3),
        C_VDIR..=6 => Some((c - C_VDIR) as usize),
        _ => None,
    }
}

/// The generated sector `lba` (overlays included), as read_block.
pub(crate) fn gen_sector(lba: u32, buf: &mut [u8]) -> bool {
    VDisk.read_block(lba, buf)
}

/// USB bus reset (msc.rs): a new session, compact at the next refresh.
pub fn on_bus_reset() {
    if state().is_some() {
        crate::vwrite::release_all(true);
        tab().want_compact = true;
        dlog::set_stale();
    }
}

/// (files, next free cluster, files left out) for DBG / tests.
pub fn stats() -> (usize, u32, u16) {
    if state().is_none() {
        return (0, 0, 0);
    }
    let t = tab();
    (t.n, t.hw, t.left_out)
}

/// The entry holding cluster `c` (>= dyn0): bisect on `vstart`.
fn at_cluster(t: &Tab, c: u32) -> Option<&VF> {
    let i = t.e[..t.n].partition_point(|e| e.vstart as u32 <= c);
    let e = t.e[..t.n].get(i.checked_sub(1)?)?;
    (c < e.vstart as u32 + nclus(e.vsize)).then_some(e)
}

// --- read cursors ------------------------------------------------------------

/// The CSV generator of the last DLG read (key: SPI start, size), ~0.7 KiB
/// of .bss; a BMP's chain cursor likewise.
static mut CSV_CUR: dlog::Slot<(u16, u32)> = dlog::Slot::new();
static mut BMP_KEY: (u16, u32) = (0, 0);
static mut BMP_CUR: core::mem::MaybeUninit<FatFile<FnRaw>> = core::mem::MaybeUninit::uninit();

fn geo() -> Option<(FnRaw, Geo)> {
    let mut d = dlog::disk();
    let g = dlog::geometry(&mut d)?;
    Some((d, g))
}

/// Bytes `off..` of table file `e` into `buf` (zeros on a read error).
fn file_read(e: &VF, off: u32, buf: &mut [u8]) {
    let key = (e.src, e.ssize);
    if kind(e.id) == 2 {
        let cur = unsafe { &mut *core::ptr::addr_of_mut!(BMP_CUR) };
        let k = unsafe { &mut *core::ptr::addr_of_mut!(BMP_KEY) };
        if *k != key || k.1 == 0 {
            let Some((d, g)) = geo() else { return };
            cur.write(FatFile::new(d, g, e.src as u32, e.ssize));
            *k = key;
        }
        if !unsafe { cur.assume_init_mut() }.read(off, buf) {
            buf.fill(0);
            *k = (0, 0);
        }
        return;
    }
    let cur = unsafe { &mut *core::ptr::addr_of_mut!(CSV_CUR) };
    let open = || {
        let (d, g) = geo()?;
        Gen::open(FatFile::new(d, g, e.src as u32, e.ssize), e.ssize)
    };
    if let Some(g) = cur.get(key, open) {
        g.read(off, buf);
    }
}

// --- the block device --------------------------------------------------------------

/// The USB block device (a handle; the volume itself is `STATE` / `TAB`).
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
        crate::vwrite::read_only()
    }
    fn gates_writes(&self) -> bool {
        true
    }
    fn write_error(&self) -> u8 {
        crate::vwrite::last_error()
    }
    fn product(&self) -> &'static [u8; 16] {
        b"DM40 files      "
    }
    fn media_change_immediate(&self) -> bool {
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
            root_sector(lba - ROOT_START, buf);
        } else {
            data_sector(st, lba - DATA_START, buf);
        }
        true
    }
    fn write_block(&mut self, lba: u32, buf: &[u8]) -> bool {
        if state().is_none() || lba >= TOTAL || buf.len() < BS {
            return false;
        }
        crate::vwrite::write(lba, &buf[..BS])
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
    b[0x25] = crate::vwrite::boot_state(); // the host's dirty flag (Linux)
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

/// FAT entry of cluster `c`: media / EOC marks, one-cluster directories and
/// README, the contiguous chains of the INFO / DEBUG files and of every
/// table file, 0 = free.
fn fat_entry(st: &State, c: u32) -> u16 {
    let chain = |start: u32, size: u32| -> Option<u16> {
        let end = start + nclus(size);
        (start != 0 && c >= start && c < end).then(|| if c + 1 == end { 0xFFFF } else { (c + 1) as u16 })
    };
    match c {
        0 => 0xFFF8,
        1 => crate::vwrite::fat1(),
        C_INFO..=C_README => 0xFFFF,
        _ if c < st.spi_start => st.files[..st.n].iter().find_map(|f| chain(f.start, f.size)).unwrap_or(0),
        _ if c < st.dyn0 => chain(st.spi_start, SPI_BYTES).unwrap_or(0),
        _ => match crate::vwrite::pair(c) {
            Some(v) => v,
            None => at_cluster(tab(), c).filter(|e| e.id & T_FAT == 0).and_then(|e| chain(e.vstart as u32, e.vsize)).unwrap_or(0),
        },
    }
}

fn fat_sector(st: &State, s: u32, b: &mut [u8]) {
    let per = (BS / 2) as u32;
    for i in 0..per {
        put16(b, (i * 2) as usize, fat_entry(st, s * per + i));
    }
}

/// A short directory entry.
fn dirent(e: &mut [u8], name: &[u8; 11], attr: u8, start: u32, size: u32) {
    e[..11].copy_from_slice(name);
    e[11] = attr;
    put16(e, 0x10, DATE); // created
    put16(e, 0x12, DATE); // accessed
    put16(e, 0x18, DATE); // written
    put16(e, 0x1A, start as u16);
    put32(e, 0x1C, size);
}

/// Root entry `i` (0 = label).
fn root_entry(i: usize, e: &mut [u8]) {
    match i {
        0 => {
            e[..11].copy_from_slice(&LABEL);
            e[11] = 0x08; // volume label
            put16(e, 0x18, DATE);
        }
        1 => dirent(e, b"README  TXT", 0x01, C_README, readme(0, &mut [])),
        2 => dirent(e, b"INFO       ", 0x10, C_INFO, 0),
        3 => dirent(e, b"DEBUG      ", 0x10, C_DEBUG, 0),
        4..=6 => dirent(e, &VDIRS[i - 4].0, 0x10, C_VDIR + (i - 4) as u32, 0),
        _ => {}
    }
}

fn root_sector(s: u32, b: &mut [u8]) {
    let per = (BS / 32) as u32;
    for i in 0..per {
        let idx = (s * per + i) as usize;
        let e = &mut b[(i * 32) as usize..(i * 32 + 32) as usize];
        if idx <= ROOT_N {
            root_entry(idx, e);
        } else if let Some(r) = crate::vwrite::root_rec(idx) {
            e.copy_from_slice(&r);
        }
    }
}

/// Sector `s` of a one-cluster subdirectory at cluster `c`: "." / "..",
/// then its files by slot (a slot below the high-water mark without a file
/// = a deleted entry, 0xE5).
fn dir_sector(st: &State, c: u32, s: u32, b: &mut [u8]) {
    let per = (BS / 32) as u32;
    let (lo, hi) = (s * per, s * per + per);
    let ent = |slot: u32| {
        let o = ((slot - lo) * 32) as usize;
        o..o + 32
    };
    for slot in lo..hi.min(2) {
        let name = if slot == 0 { b".          " } else { b"..         " };
        let r = ent(slot);
        dirent(&mut b[r], name, 0x10, if slot == 0 { c } else { 0 }, 0);
    }
    match c {
        C_INFO => {
            for (i, f) in st.files[..st.n].iter().enumerate() {
                let slot = i as u32 + 2;
                if (lo..hi).contains(&slot) {
                    let r = ent(slot);
                    dirent(&mut b[r], &f.name, 0x01, f.start, f.size);
                }
            }
        }
        C_DEBUG => {
            if (lo..hi).contains(&2) {
                let r = ent(2);
                dirent(&mut b[r], b"SPIFLASHBIN", 0x01, st.spi_start, SPI_BYTES);
            }
        }
        _ => {
            let k = (c - C_VDIR) as usize;
            let t = tab();
            for slot in lo.max(2)..hi.min(t.slot_hw[k] as u32) {
                let r = ent(slot);
                b[r.start] = 0xE5;
            }
            for e in t.e[..t.n].iter().filter(|e| kind(e.id) == k && (lo..hi).contains(&(e.slot as u32))) {
                let r = ent(e.slot as u32);
                let o = r.start;
                dyn_entry(e, &mut b[r]);
                if e.id & T_DIR != 0 {
                    b[o] = 0xE5; // the host deleted it: as it wrote it
                }
            }
        }
    }
}

/// Data sector `s` (0 = the first sector of cluster 2).
fn data_sector(st: &State, s: u32, b: &mut [u8]) {
    let c = s / SPC + 2;
    let within = (s % SPC) * BS as u32;
    // (file start cluster, size): the bytes of this sector.
    let span = |start: u32, size: u32| -> Option<(u32, usize)> {
        let off = (c - start) * CB + within;
        (off < size).then(|| (off, (size - off).min(BS as u32) as usize))
    };
    match c {
        C_INFO..=C_DEBUG | C_VDIR..=6 => dir_sector(st, c, s % SPC, b),
        C_README => {
            if let Some((off, len)) = span(C_README, readme(0, &mut [])) {
                readme(off as usize, &mut b[..len]);
            }
        }
        _ if c < st.spi_start => {
            let Some(f) = st.files[..st.n].iter().find(|f| f.start != 0 && c >= f.start && c < f.start + nclus(f.size)) else { return };
            let Some((off, len)) = span(f.start, f.size) else { return };
            let d = &mut b[..len];
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
        }
        _ if c < st.dyn0 => {
            if let Some((off, len)) = span(st.spi_start, SPI_BYTES) {
                if !dlog::disk().read_at(off, &mut b[..len]) {
                    b.fill(0);
                }
            }
        }
        _ => {
            if crate::vwrite::staged_read(c, s % SPC, b) {
                return; // a host file's data (vwrite.rs)
            }
            let Some(e) = at_cluster(tab(), c).copied() else { return };
            if e.id & (T_FAT | T_SRC) != 0 {
                return; // freed by the host / gone: zeros
            }
            if let Some((off, len)) = span(e.vstart as u32, e.vsize) {
                file_read(&e, off, &mut b[..len]);
            }
        }
    }
}

// --- README.TXT -------------------------------------------------------------------

/// README.TXT's length: the text padded with spaces (the volume's layout,
/// and so the host writes captured for the tests, do not change with the
/// text or the version string).
const README_LEN: usize = 2048;

/// README.TXT bytes `base .. base + out.len()` into `out`; returns its
/// whole length (README_LEN).
fn readme(base: usize, out: &mut [u8]) -> u32 {
    let mut w = W { b: out, base, n: 0 };
    w.s(b"DM40 (dm40-fw ");
    w.s(VERSION.as_bytes());
    w.s(b") - USB disk generated by the meter.\r\n\r\n");
    w.s(b"LOGS\\    LOGnnn.CSV   DMM recordings (swipe down on the DMM page)\r\n");
    w.s(b"SCOPE\\   SCOPnnn.CSV  scope records (Save on the scope page)\r\n");
    w.s(b"SHOTS\\   Snnn.BMP     screenshots (three-finger swipe down)\r\n");
    w.s(b"INFO\\    calibration, boot data, UID, option bytes, OTP, boot\r\n");
    w.s(b"         loader images and FIRMWARE.BIN (the running firmware as\r\n");
    w.s(b"         an update file); BACKUP.TXT tells what each one is.\r\n");
    w.s(b"DEBUG\\   SPIFLASH.BIN: raw image of the 16 MiB SPI flash (the\r\n");
    w.s(b"         meter's internal FAT16 storage).\r\n\r\n");
    w.s(b"The files are stored on the meter in a compact form and converted\r\n");
    w.s(b"when read. Files saved while the disk is mounted appear after an\r\n");
    w.s(b"eject / re-plug.\r\n\r\n");
    w.s(b"Delete: delete a file in LOGS, SCOPE or SHOTS here and it is\r\n");
    w.s(b"deleted on the meter (moved to the trash: at the eject). Or on the\r\n");
    w.s(b"meter (Gallery), or over USB/SCPI: MMEM:DEL.\r\n");
    w.s(b"Nothing else can be changed: other files copied here are not kept\r\n");
    w.s(b"(gone at the next eject / re-plug), INFO and DEBUG are read-only.\r\n\r\n");
    w.s(b"Firmware update: copy the update file (dm40*.bin, e.g. dm40fw.bin)\r\n");
    w.s(b"here, next to this README. The meter checks it and asks \"Install it\r\n");
    w.s(b"now?\": Yes installs it (the meter restarts into the vendor loader,\r\n");
    w.s(b"then into the new firmware), No discards it. Or tools/reload.sh\r\n");
    w.s(b"(fw-rs repository: DBG:UPLOAD + DBG:UPDATE over USBTMC).\r\n");
    w.s(b"Without the tools (also when the meter does not start): from off,\r\n");
    w.s(b"hold the power key ~4 s until the beep and 0.5 s more. The vendor\r\n");
    w.s(b"loader's update screen shows its own USB disk: copy the update file\r\n");
    w.s(b"there as dm40fw.bin (INFO\\FIRMWARE.BIN = the running firmware) and\r\n");
    w.s(b"press \"update\".\r\n");
    debug_assert!(w.n + 2 <= README_LEN);
    while w.n + 2 < README_LEN {
        w.c(b' ');
    }
    w.s(b"\r\n");
    w.n as u32
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
    w.s(b"DM40 INFO: generated on the fly by dm40-fw ");
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
            w.s(b"reinstall it: tools/reload.sh --image FIRMWARE.BIN, or copy it as\r\n");
            w.s(b"dm40fw.bin to the vendor loader's own USB disk (see README.TXT).\r\n");
        }
        None => w.s(b"; no FIRMWARE.BIN (no valid loader record at 08008000).\r\n"),
    }
    w.s(b"\r\nFull restore after a mass erase (SWD): write ROMBOOT.BIN at 0x08000000,\r\n");
    w.s(b"then LOADER.BIN at 0x080A0000 and CALIB.BIN at 0x080C0000, then install\r\n");
    w.s(b"a firmware. BOOTDATA.BIN = LOADER.BIN bytes 0x4000..0x7FFF. The loader\r\n");
    w.s(b"data is bound to UID.BIN: this unit only.\r\n");
    w.s(b"Copy these files to a PC: they are made from the MCU flash and go\r\n");
    w.s(b"with it. The meter's SPI flash keeps one write-once safety copy of\r\n");
    w.s(b"CALIB.BIN, BOOTDATA.BIN and UID.BIN (written when missing, never\r\n");
    w.s(b"overwritten; it survives an SWD mass erase of the MCU).\r\n");
    debug_assert!(w.n <= TXT_MAX as usize);
    w.n
}


/// The host released the volume (Allow): a compaction it held back can run
/// now (msc.rs).
pub fn on_unlock() {
    if state().is_some() {
        if crate::vwrite::release() || tab().want_compact {
            dlog::set_stale();
        }
    }
}
