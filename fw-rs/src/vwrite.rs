//! Host writes on the generated USB volume (vdisk.rs): delete from the PC
//! and the drag-and-drop firmware update. The volume is writable (no WP
//! bit), but nothing the host writes is stored as sectors: every 512-byte
//! WRITE is compared with the sector the volume generates and accepted only
//! when its difference is one of the changes below, which then live as
//! small records (SRAM ~0.9 KiB) and regenerate exactly what the host
//! wrote. Anything else fails (DATA PROTECT 07/27/00; the host's write-back
//! gets an I/O error and its buffer is re-read later), so the SPI disk can
//! never take a host's structure, and a host never sees a sector other than
//! the one it wrote.
//!
//! Accepted:
//!   * boot sector byte 0x25 (Linux's "dirty" flag on a read-write mount);
//!     FAT[1] bits 15 / 14 (Windows / macOS dirty / error flags);
//!   * date / time fields of an existing entry (access date, the parent
//!     directory's mtime): accepted and dropped (a re-read shows the old
//!     date, nothing else differs);
//!   * a delete of one of our files in LOGS/, SCOPE/, SHOTS/: its entry 0xE5
//!     (the rest unchanged, `vdisk::T_DIR`) and its chain freed in the FAT
//!     (all of its entries in that FAT sector 0, `T_FAT`), in any order;
//!     when both have been seen the SPI file is deleted (`poll`, from the
//!     main loop, csvlog::delete_on; only if the SPI entry is still the same
//!     file). A 0xE5 without the chain freed (a rename / move to a trash
//!     directory) never deletes anything: at the session's end the file is
//!     back;
//!   * FAT entries of free clusters (the host allocates; `pair`), host
//!     entries in free root slots: an 8.3 entry DM40*.BIN (a regular file,
//!     start cluster free, at most upload::MAX bytes), long-name entries,
//!     0xE5 marks over them (raw 32-byte records, `rec`);
//!   * data in free clusters: staged in free clusters of the SPI disk
//!     (`stage`, SPI FAT untouched; while a staging is active it owns the
//!     SPI disk like a DBG:UPLOAD, upload::busy), served back on reads.
//! Refused: writing into INFO/, DEBUG/, README.TXT or into our files,
//! creating entries in our directories, directories / other names in the
//! root, renaming, truncating, anything past the capacities below.
//!
//! Firmware file (`poll`): a root record DM40*.BIN with a start cluster and
//! a size whose chain is complete in `pair` and whose clusters have every
//! sector up to the size staged (in whatever order the host wrote data,
//! FAT and directory) is decoded and checked (header test as the loader's,
//! word1 = size - 12, the body CRC-16 of word2 with the marker bytes
//! excluded, no hazardous short last block); then upload::commit_on links
//! the staged SPI clusters as DM40FW.BIN in the SPI root (other dm40*.bin
//! deleted; data -> FAT -> directory) and the meter asks "Install?"
//! (dialog.rs; Yes = DBG:UPDATE, No / timeout = DM40FW.BIN deleted). A
//! power cut before the commit leaves only unlinked data in free clusters.
//!
//! Session: while the host has the medium locked (PREVENT: Linux / macOS
//! mount) the layout is frozen (`frozen`): no device-side change and no
//! UNIT ATTENTION (a UA on a removable disk makes Linux fail the next
//! commands: a lost write-back), new recordings / screenshots appear at the
//! Allow (unmount) or a bus reset (re-plug), when every record here is
//! dropped (`release`) and the volume regenerated. A host that never locks
//! the medium: frozen while it has written in the last QUIET_MS, then
//! released.
//!
//! Host test: tools/fatren-test/run.sh "vwrite" (Linux sequences captured
//! over USB/IP, mtools-made sector diffs, fsck after each scenario, power
//! cuts in the staging).

use crate::dlog::{self, Raw as _};
use crate::msc::BlockDevice;
use crate::vdisk::{self, Owner, BS, CB, DATA_START, FAT_SECS, FAT_START, ID, ROOT_N, ROOT_START, SPC, T_DIR, T_FAT, T_SRC};

/// Host records in the root (raw 32-byte entries).
const NREC: usize = 6;
/// Host FAT entries (cluster -> value).
const NPAIR: usize = 32;
/// Staged clusters: one firmware file (upload::MAX) and one spare.
const NST: usize = 21;
const BLK: usize = 4096;
/// Volume sectors per SPI block, SPI blocks per volume cluster.
const SPB: u32 = (BLK / BS) as u32;
const BPC: usize = CB as usize / BLK;
const _: () = assert!((NST as u32 - 1) * CB >= crate::upload::MAX && SPC <= 64);
/// A lockless host: the session ends this long after its last write.
pub const QUIET_MS: u64 = 60_000;
/// A staging with no new data for this long and no complete file is
/// dropped (the SPI disk is free for the on-device writers again; Linux
/// writes back dirty pages within ~35 s).
pub const STAGE_IDLE_MS: u64 = 45_000;

/// `last_error` codes (msc.rs write_error).
const E_IO: u8 = 0;
const E_REFUSED: u8 = 1;
const E_BUSY: u8 = 2;

#[derive(Clone, Copy)]
struct Rec {
    /// Root slot (0 = unused; host slots are > ROOT_N).
    slot: u16,
    e: [u8; 32],
}

#[derive(Clone, Copy)]
struct Pair {
    /// Cluster (0 = unused).
    c: u16,
    v: u16,
}

/// One staged volume cluster.
#[derive(Clone, Copy)]
struct St {
    /// Volume cluster (0 = unused).
    c: u16,
    /// Linked into DM40FW.BIN on the SPI disk (read-only from then on).
    sealed: bool,
    /// Sectors written (bit = sector of the cluster).
    map: u64,
    /// SPI cluster of each 4 KiB block (0 = none yet: its written sectors
    /// were all zero).
    blk: [u16; BPC],
}

const REC0: Rec = Rec { slot: 0, e: [0; 32] };
const PAIR0: Pair = Pair { c: 0, v: 0 };
const ST0: St = St { c: 0, sealed: false, map: 0, blk: [0; BPC] };

/// The firmware file.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Fw {
    None = 0,
    /// Committed: the dialog is due.
    Ask,
    /// The dialog is up.
    Asked,
    /// No / timeout: delete DM40FW.BIN.
    Decline,
    /// Yes: main.rs zeroes the marker and resets.
    Install,
    /// A complete DM40*.BIN that is no update file: a toast is due.
    Bad,
}

#[derive(Clone, Copy)]
pub struct Stats {
    pub ok: u32,
    pub refused: u32,
    pub busy: u32,
    pub last_refused: u32,
    pub deleted: u16,
    pub fw_ok: u16,
    pub fw_bad: u16,
}

struct W {
    /// DBG:VDISK:RO 1: write-protected as before (live updates while
    /// mounted; no delete / update from the PC).
    ro: bool,
    boot: u8,
    /// FAT[1] as written (bit 16 = set).
    fat1: u32,
    rec: [Rec; NREC],
    pair: [Pair; NPAIR],
    st: [St; NST],
    /// The host changed something since the last release.
    session: bool,
    /// The host has locked the medium since the last bus reset.
    locks: bool,
    /// Time of the last accepted write (0 = none), now (poll).
    last: u64,
    now: u64,
    /// Staged data not linked yet: the SPI free space is ours.
    staging: bool,
    stage_t: u64,
    /// SPI first-fit hint (only grows while staging).
    hint: u32,
    err: u8,
    fw: Fw,
    /// The last candidate decided (slot, start, size): not tried again.
    done: (u16, u16, u32),
    n: Stats,
}

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut W0: W = W {
    ro: false,
    boot: 0,
    fat1: 0,
    rec: [REC0; NREC],
    pair: [PAIR0; NPAIR],
    st: [ST0; NST],
    session: false,
    locks: false,
    last: 0,
    now: 0,
    staging: false,
    stage_t: 0,
    hint: 0,
    err: 0,
    fw: Fw::None,
    done: (0, 0, 0),
    n: Stats { ok: 0, refused: 0, busy: 0, last_refused: 0, deleted: 0, fw_ok: 0, fw_bad: 0 },
};
/// The logger needs a constant interval (main.rs `disk_writes_blocked`):
/// no SPI write for a staging meanwhile.
static BLOCKED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn w() -> &'static mut W {
    unsafe { &mut *core::ptr::addr_of_mut!(W0) }
}

#[cfg(target_os = "none")]
fn dev() -> Option<&'static mut dyn BlockDevice> {
    Some(crate::upload::spi())
}

#[cfg(not(target_os = "none"))]
static mut TEST_DEV: Option<&'static mut dyn BlockDevice> = None;

#[cfg(not(target_os = "none"))]
fn dev() -> Option<&'static mut dyn BlockDevice> {
    unsafe { (*core::ptr::addr_of_mut!(TEST_DEV)).as_deref_mut() }
}

/// Host test: the SPI disk the staging writes (dlog's reader must see it).
#[cfg(not(target_os = "none"))]
pub fn set_dev(d: &'static mut dyn BlockDevice) {
    unsafe { *core::ptr::addr_of_mut!(TEST_DEV) = Some(d) };
}

fn le16(b: &[u8], i: u32) -> u16 {
    u16::from_le_bytes([b[i as usize * 2], b[i as usize * 2 + 1]])
}

fn le16o(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn le32o(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

// --- queries (vdisk.rs, msc.rs, main.rs) -------------------------------------

pub fn read_only() -> bool {
    w().ro
}

/// DBG:VDISK:RO: a host reads the WP bit when it mounts.
pub fn set_read_only(on: bool) {
    w().ro = on;
}

pub fn last_error() -> u8 {
    w().err
}

pub fn stats() -> Stats {
    w().n
}

/// (records, pairs, staged clusters, frozen, staging, firmware state).
pub fn state() -> (usize, usize, usize, bool, bool, u8) {
    let w = w();
    (
        w.rec.iter().filter(|r| r.slot != 0).count(),
        w.pair.iter().filter(|p| p.c != 0).count(),
        w.st.iter().filter(|s| s.c != 0).count(),
        frozen(),
        w.staging,
        w.fw as u8,
    )
}

/// The layout must not change under the host (module text).
pub fn frozen() -> bool {
    let w = w();
    !w.ro && (crate::msc::host_mounted() || (!w.locks && (w.session || (w.last != 0 && w.now.saturating_sub(w.last) < QUIET_MS))))
}

/// A staging owns the SPI free space (upload::busy).
pub fn staging() -> bool {
    w().staging
}

pub fn boot_state() -> u8 {
    w().boot
}

pub fn fat1() -> u16 {
    let f = w().fat1;
    if f & 0x1_0000 != 0 { f as u16 } else { 0xFFFF }
}

fn pair_idx(c: u32) -> Option<usize> {
    w().pair.iter().position(|p| p.c != 0 && p.c as u32 == c)
}

/// The host's FAT entry of cluster `c`, if it wrote one.
pub fn pair(c: u32) -> Option<u16> {
    pair_idx(c).map(|i| w().pair[i].v)
}

/// The host's record in root slot `idx`.
pub fn root_rec(idx: usize) -> Option<[u8; 32]> {
    w().rec.iter().find(|r| r.slot != 0 && r.slot as usize == idx).map(|r| r.e)
}

fn slot_of(c: u32) -> Option<usize> {
    w().st.iter().position(|s| s.c != 0 && s.c as u32 == c)
}

/// Sector `s` of volume cluster `c` if the host wrote data there (staged):
/// into `b` (zeros where it wrote nothing). False: not a staged cluster.
pub fn staged_read(c: u32, s: u32, b: &mut [u8]) -> bool {
    let Some(i) = slot_of(c) else { return false };
    let st = w().st[i];
    let b = &mut b[..BS];
    b.fill(0);
    let sc = st.blk[(s / SPB) as usize];
    if st.map & (1u64 << s) != 0 && sc != 0 {
        let mut d = dlog::disk();
        let ok = dlog::geometry(&mut d).is_some_and(|g| d.read_at(g.data + (sc as u32 - 2) * g.cb + (s % SPB) * BS as u32, b));
        if !ok {
            b.fill(0);
        }
    }
    true
}

// --- the write classifier ------------------------------------------------------

/// A host WRITE of sector `lba` (vdisk.rs write_block). False: refused,
/// `last_error` says why.
pub fn write(lba: u32, d: &[u8]) -> bool {
    let w = w();
    let r = if w.ro { Err(E_REFUSED) } else { classify(lba, d) };
    match r {
        Ok(changed) => {
            w.last = w.now.max(1);
            w.session |= changed;
            w.n.ok += 1;
            true
        }
        Err(e) => {
            w.err = e;
            if e == E_BUSY {
                w.n.busy += 1;
            } else {
                w.n.refused += 1;
                w.n.last_refused = lba;
            }
            false
        }
    }
}

type R = Result<bool, u8>;

fn classify(lba: u32, d: &[u8]) -> R {
    let mut g = [0u8; BS];
    if !vdisk::gen_sector(lba, &mut g) {
        return Err(E_IO);
    }
    if lba >= DATA_START {
        // Data of a host file: staged even when it equals what is shown
        // (zeros): the sector counts as written.
        let c = (lba - DATA_START) / SPC + 2;
        if vdisk::dir_kind(c).is_none() && host_cluster(c) {
            return stage(c, (lba - DATA_START) % SPC, d);
        }
    }
    if g[..] == d[..BS] {
        return Ok(false);
    }
    if lba == 0 {
        return boot(d, &g);
    }
    if lba < ROOT_START {
        return fat((lba - FAT_START) % FAT_SECS, d, &g);
    }
    if lba < DATA_START {
        return root(lba - ROOT_START, d, &g);
    }
    let s = lba - DATA_START;
    let c = s / SPC + 2;
    match vdisk::dir_kind(c) {
        Some(k) => dir(k, s % SPC, d, &g),
        None => data(c, s % SPC, d),
    }
}

fn boot(d: &[u8], g: &[u8]) -> R {
    if (0..BS).any(|i| i != 0x25 && d[i] != g[i]) {
        return Err(E_REFUSED);
    }
    w().boot = d[0x25];
    Ok(true)
}

/// Bytes of a directory entry that are dates / times (created, accessed,
/// written).
fn is_date(i: usize) -> bool {
    matches!(i, 0x0D..=0x13 | 0x16..=0x19)
}

fn dates_only(a: &[u8], b: &[u8]) -> bool {
    (0..32).all(|i| a[i] == b[i] || is_date(i))
}

/// Our file `j` with its chain partly in FAT sector `s`: "the host frees
/// it" when every one of its entries in the sector is 0 in `d`.
fn frees_all(j: usize, s: u32, d: &[u8]) -> bool {
    let per = (BS / 2) as u32;
    let e = vdisk::entries()[j];
    let (a, b) = (e.vstart as u32, e.vstart as u32 + vdisk::nclus(e.vsize));
    (a.max(s * per)..b.min(s * per + per)).all(|c| le16(d, c - s * per) == 0)
}

/// A cluster still ours in the model (a live file's chain) that the host
/// has not freed or reused.
fn ours_live(j: usize, c: u32) -> bool {
    vdisk::entries()[j].id & T_FAT == 0 && pair_idx(c).is_none()
}

fn fat(s: u32, d: &[u8], g: &[u8]) -> R {
    let per = (BS / 2) as u32;
    let mut frees = [0u32; vdisk::MAX_VF.div_ceil(32)];
    let mut need = 0usize;
    for i in 0..per {
        let c = s * per + i;
        let (o, n) = (le16(g, i), le16(d, i));
        if o == n {
            continue;
        }
        match c {
            0 => return Err(E_REFUSED),
            1 if n | 0xC000 == 0xFFFF => continue,
            1 => return Err(E_REFUSED),
            _ => {}
        }
        match vdisk::owner(c) {
            Owner::Reserved | Owner::Fixed => return Err(E_REFUSED),
            Owner::Dyn(j) if ours_live(j, c) => {
                if n != 0 || !frees_all(j, s, d) {
                    return Err(E_REFUSED); // a truncation / relink of our file
                }
                frees[j / 32] |= 1 << (j % 32);
            }
            _ => {
                // A cluster the host owns: free in the model, freed by the
                // host, or allocated by it.
                if n != 0 && !(2..vdisk::CLUSTERS + 2).contains(&(n as u32)) && n < 0xFFF8 {
                    return Err(E_REFUSED);
                }
                if n != 0 && pair_idx(c).is_none() {
                    need += 1;
                }
            }
        }
    }
    let w = w();
    if need > w.pair.iter().filter(|p| p.c == 0).count() {
        return Err(E_REFUSED);
    }
    for i in 0..per {
        let c = s * per + i;
        let (o, n) = (le16(g, i), le16(d, i));
        if o == n {
            continue;
        }
        if c == 1 {
            w.fat1 = n as u32 | 0x1_0000;
            continue;
        }
        if let Owner::Dyn(j) = vdisk::owner(c) {
            if frees[j / 32] & (1 << (j % 32)) != 0 {
                continue;
            }
        }
        set_pair(c, n);
    }
    let ents = vdisk::entries();
    for (j, e) in ents.iter_mut().enumerate() {
        if frees[j / 32] & (1 << (j % 32)) != 0 {
            e.id |= T_FAT;
        }
    }
    Ok(true)
}

/// The host's FAT entry of cluster `c` = `v` (0: freed, its staged data
/// dropped).
fn set_pair(c: u32, v: u16) {
    let w = w();
    match (pair_idx(c), v) {
        (Some(i), 0) => {
            w.pair[i] = PAIR0;
            if let Some(k) = slot_of(c) {
                w.st[k] = ST0;
            }
        }
        (Some(i), v) => w.pair[i].v = v,
        (None, 0) => {}
        (None, v) => {
            if let Some(p) = w.pair.iter_mut().find(|p| p.c == 0) {
                *p = Pair { c: c as u16, v };
            }
        }
    }
}

/// 8.3 name DM40*.BIN (any case: Linux's "dm40fw.bin" is the 8.3 name with
/// the lower-case flags; a long name's alias keeps the first characters).
fn update_name(e: &[u8]) -> bool {
    e[..4].eq_ignore_ascii_case(b"DM40") && e[8..11].eq_ignore_ascii_case(b"BIN")
}

/// A cluster the host may own: free in the model, freed or taken by it.
fn host_cluster(c: u32) -> bool {
    match vdisk::owner(c) {
        Owner::Free => true,
        Owner::Dyn(j) => !ours_live(j, c),
        _ => false,
    }
}

/// A host record we accept in a free root slot.
fn rec_ok(h: &[u8]) -> bool {
    if h[0] == 0 {
        return false;
    }
    if h[0] == 0xE5 || h[11] & 0x3F == 0x0F {
        return true; // a deleted entry, a long-name entry
    }
    if h[11] & 0x18 != 0 || !update_name(h) {
        return false; // directory / label / another name
    }
    let start = le16o(h, 0x1A) as u32;
    le32o(h, 0x1C) <= crate::upload::MAX && le16o(h, 0x14) == 0 && (start == 0 || host_cluster(start))
}

fn root(s: u32, d: &[u8], g: &[u8]) -> R {
    let per = (BS / 32) as u32;
    let mut need = 0usize;
    let mut changed = false;
    for i in 0..per {
        let idx = (s * per + i) as usize;
        let r = (i * 32) as usize..(i * 32 + 32) as usize;
        let (ge, he) = (&g[r.clone()], &d[r]);
        if ge == he {
            continue;
        }
        if idx <= ROOT_N {
            if !dates_only(ge, he) {
                return Err(E_REFUSED);
            }
            continue; // dropped
        }
        if !rec_ok(he) {
            return Err(E_REFUSED);
        }
        if root_rec(idx).is_none() {
            need += 1;
        }
        changed = true;
    }
    let w = w();
    if need > w.rec.iter().filter(|r| r.slot == 0).count() {
        return Err(E_REFUSED);
    }
    for i in 0..per {
        let idx = (s * per + i) as usize;
        let r = (i * 32) as usize..(i * 32 + 32) as usize;
        if idx <= ROOT_N || g[r.clone()] == d[r.clone()] {
            continue;
        }
        let k = w.rec.iter().position(|x| x.slot as usize == idx).or_else(|| w.rec.iter().position(|x| x.slot == 0));
        if let Some(k) = k {
            w.rec[k].slot = idx as u16;
            w.rec[k].e.copy_from_slice(&d[r]);
        }
    }
    Ok(changed)
}

/// Sector `s` of a generated subdirectory (`k` = file kind, 3 = INFO /
/// DEBUG).
fn dir(k: usize, s: u32, d: &[u8], g: &[u8]) -> R {
    let per = (BS / 32) as u32;
    let mut marks = [usize::MAX; BS / 32];
    for i in 0..per {
        let slot = s * per + i;
        let r = (i * 32) as usize..(i * 32 + 32) as usize;
        let (ge, he) = (&g[r.clone()], &d[r]);
        if ge == he || dates_only(ge, he) {
            continue;
        }
        let ents = vdisk::entries();
        let hit = (k < 3)
            .then(|| ents.iter().position(|e| ((e.id >> 10) & 3) as usize == k && e.slot as u32 == slot && e.id & T_DIR == 0))
            .flatten();
        if let Some(j) = hit {
            let mut x = [0u8; 32];
            x.copy_from_slice(ge);
            x[0] = 0xE5;
            if he[0] == 0xE5 && dates_only(&x, he) {
                marks[i as usize] = j;
                continue;
            }
        }
        return Err(E_REFUSED);
    }
    let ents = vdisk::entries();
    let mut changed = false;
    for &j in marks.iter().filter(|&&j| j != usize::MAX) {
        ents[j].id |= T_DIR;
        changed = true;
    }
    Ok(changed)
}

fn data(c: u32, s: u32, d: &[u8]) -> R {
    match vdisk::owner(c) {
        Owner::Reserved | Owner::Fixed => Err(E_REFUSED),
        Owner::Dyn(j) if ours_live(j, c) => Err(E_REFUSED),
        Owner::Dyn(j) if vdisk::entries()[j].id & T_FAT == 0 => Err(E_REFUSED),
        _ => stage(c, s, d),
    }
}

/// The SPI disk is free for a staging write / a commit / a delete.
fn spi_free() -> bool {
    !crate::csvlog::writing() && !crate::screenshot::busy() && !crate::upload::scpi_busy() && !BLOCKED.load(core::sync::atomic::Ordering::Relaxed)
}

/// Data for host cluster `c`, sector `s`: into its SPI block (read-modify-
/// write of the 4 KiB block; a block whose sectors so far were all zero is
/// not allocated yet).
fn stage(c: u32, s: u32, d: &[u8]) -> R {
    if !spi_free() {
        return Err(E_BUSY);
    }
    let w = w();
    if let Some(i) = slot_of(c).filter(|&i| w.st[i].sealed) {
        // The committed file: the same bytes again (a page rewritten, the
        // zeros past its end) change nothing.
        let mut cur = [0u8; BS];
        staged_read(c, s, &mut cur);
        if cur[..] == d[..BS] {
            return Ok(false);
        }
        let _ = i;
    }
    let i = match slot_of(c) {
        Some(i) if !w.st[i].sealed => i,
        found => {
            // A sealed cluster rewritten: the host overwrites its file, the
            // committed DM40FW.BIN keeps the old data; a new staging.
            if let Some(i) = found {
                w.st[i] = ST0;
            }
            let Some(i) = w.st.iter().position(|x| x.c == 0) else { return Err(E_REFUSED) };
            w.st[i] = St { c: c as u16, ..ST0 };
            i
        }
    };
    if !w.staging {
        w.staging = true;
        w.hint = 2;
    }
    w.stage_t = w.now;
    let j = (s / SPB) as usize;
    let zero = d[..BS].iter().all(|&b| b == 0);
    if w.st[i].blk[j] == 0 && zero {
        w.st[i].map |= 1u64 << s;
        return Ok(true);
    }
    let dev = dev().ok_or(E_IO)?;
    let [buf, fat, tmp] = crate::calbackup::scratch();
    if !dev.read_block(0, tmp) {
        return Err(E_IO);
    }
    let v = crate::fatren::parse_bpb(&tmp[..], dev.block_count()).ok_or(E_IO)?;
    if v.spc != 1 {
        return Err(E_IO);
    }
    let mut sc = w.st[i].blk[j];
    if sc == 0 {
        sc = crate::upload::next_free(dev, &v, &mut w.hint, fat).map_err(|_| E_REFUSED)?;
        // Sectors written so far in this block were zeros, the rest 0xFF
        // (unwritten: read as zeros through `map`).
        for q in 0..SPB {
            let o = (q as usize) * BS;
            buf[o..o + BS].fill(if w.st[i].map & (1u64 << (j as u32 * SPB + q)) != 0 { 0 } else { 0xFF });
        }
    } else if !dev.read_block(v.clba(sc as u32), buf) {
        return Err(E_IO);
    }
    let o = (s % SPB) as usize * BS;
    buf[o..o + BS].copy_from_slice(&d[..BS]);
    let lba = v.clba(sc as u32);
    if !(dev.write_block(lba, buf) && crate::fatren::verify(dev, lba, buf, tmp)) {
        return Err(E_IO);
    }
    w.st[i].blk[j] = sc;
    w.st[i].map |= 1u64 << s;
    Ok(true)
}

// --- main loop -------------------------------------------------------------------

/// Main loop, every pass: the session's end for a lockless host, then (when
/// the SPI disk is free) one of: the SPI delete of a file the host deleted,
/// a declined firmware file's delete, a firmware file's check and commit,
/// the timeout of an idle staging. `blocked` = the logger needs a constant
/// interval (no SPI writes).
pub fn poll(now: u64, blocked: bool) {
    let w = w();
    w.now = now;
    BLOCKED.store(blocked, core::sync::atomic::Ordering::Relaxed);
    let mounted = crate::msc::host_mounted();
    w.locks |= mounted;
    if w.session && !w.locks && !mounted && now.saturating_sub(w.last) >= QUIET_MS {
        release();
        dlog::set_stale();
    }
    if !spi_free() || crate::msc::HOST_WRITING.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Some(dev) = dev() else { return };
    // 1. A host delete seen whole (entry 0xE5 and chain freed).
    let both = T_DIR | T_FAT;
    if let Some(e) = vdisk::entries().iter().find(|e| e.id & both == both && e.id & T_SRC == 0).copied() {
        let id = e.id & ID;
        let mut same = false;
        let ok = vdisk::scan_spi(None, &mut |i, s, z| same |= i == id && s == e.src && z == e.ssize);
        if !ok {
            return; // no disk / read error: try again
        }
        if same {
            match crate::csvlog::delete_on(dev, &vdisk::spi_name(id)) {
                Err(crate::csvlog::GErr::Busy) => return,
                Ok(()) => w.n.deleted += 1,
                Err(_) => {}
            }
        }
        if let Some(x) = vdisk::entries().iter_mut().find(|x| x.id & ID == id) {
            x.id |= T_SRC; // gone (or another file now: never touched)
        }
        return;
    }
    // 2. Firmware declined: the loader must not find it.
    if w.fw == Fw::Decline {
        let [buf, tmp, _] = crate::calbackup::scratch();
        if crate::fatren::delete_updates(dev, buf, tmp).is_some() {
            dlog::set_stale();
        }
        for x in w.st.iter_mut().filter(|x| x.sealed) {
            *x = ST0; // its data is gone: reads zeros
        }
        w.fw = Fw::None;
        return;
    }
    // 3. A firmware file complete?
    if w.st.iter().any(|x| x.c != 0 && !x.sealed) {
        for k in 0..NREC {
            let r = w.rec[k];
            let e = &r.e;
            if r.slot == 0 || e[0] == 0xE5 || e[11] & 0x3F == 0x0F || e[11] & 0x18 != 0 || !update_name(e) {
                continue;
            }
            let (start, size) = (le16o(e, 0x1A), le32o(e, 0x1C));
            if start == 0 || size < 128 || (r.slot, start, size) == w.done {
                continue;
            }
            let Some((chain, n)) = complete(start as u32, size) else { continue };
            w.done = (r.slot, start, size);
            finalize(dev, &chain[..n], size);
            return;
        }
    }
    // 4. An idle staging that never completed: the SPI disk is free again.
    if w.staging && now.saturating_sub(w.stage_t) >= STAGE_IDLE_MS {
        drop_unsealed();
    }
}

fn drop_unsealed() {
    let w = w();
    for x in w.st.iter_mut().filter(|x| !x.sealed) {
        *x = ST0;
    }
    w.staging = false;
}

/// The chain of a host file (start, size) if it is complete: every cluster
/// host-owned with its FAT entry, the last one end-of-chain, every sector
/// up to the size staged. Returns the staging slots in file order.
fn complete(start: u32, size: u32) -> Option<([u8; NST], usize)> {
    let n = vdisk::nclus(size) as usize;
    if n == 0 || n > NST {
        return None;
    }
    let mut chain = [0u8; NST];
    let mut c = start;
    for k in 0..n {
        let next = pair(c)?;
        let i = slot_of(c)?;
        let st = w().st[i];
        if st.sealed || chain[..k].contains(&(i as u8)) {
            return None;
        }
        let left = size - (k as u32) * CB;
        let secs = left.min(CB).div_ceil(BS as u32);
        let need = if secs >= 64 { u64::MAX } else { (1u64 << secs) - 1 };
        if st.map & need != need {
            return None;
        }
        chain[k] = i as u8;
        if k + 1 == n {
            if next < 0xFFF8 {
                return None;
            }
        } else {
            c = next as u32;
        }
    }
    Some((chain, n))
}

/// Decode one 128-byte block of an update file in place (the inverse of
/// fatren::encode_block). False: a short last block the FS-loader would
/// decode wrongly (dm40fw.py tail_hazard).
fn decode_block(p: &mut [u8]) -> bool {
    const ADD: usize = 0x1F;
    if p.len() <= ADD {
        return true;
    }
    let a = p[ADD];
    let kp = if a < 128 { a } else { a >> 1 } as usize;
    if kp >= p.len() {
        return false;
    }
    let k = p[kp];
    for (i, c) in p.iter_mut().enumerate() {
        if i != ADD && i != kp {
            *c = (*c ^ k).wrapping_sub(a);
        }
    }
    true
}

fn crc16_upd(mut c: u16, data: &[u8]) -> u16 {
    for &b in data {
        c ^= (b as u16) << 8;
        for _ in 0..8 {
            c = if c & 0x8000 != 0 { (c << 1) ^ 0x8005 } else { c << 1 };
        }
    }
    c
}

/// Check the staged file (decoded: the loader's header test, word1 = size -
/// 12, the body CRC-16 = word2's low half with the 12 marker bytes at body
/// 0x4000 excluded, no hazardous last block).
fn check(chain: &[u8], size: u32) -> bool {
    let mut h = [0u8; 12];
    let mut crc = 0xFFFFu16;
    let mut sec = [0u8; BS];
    let mut off = 0u32;
    for &i in chain {
        let c = w().st[i as usize].c as u32;
        for s in 0..SPC {
            if off >= size {
                break;
            }
            staged_read(c, s, &mut sec);
            #[cfg(target_os = "none")]
            crate::wdt::kick_if_feeding();
            let len = (size - off).min(BS as u32) as usize;
            for (q, blk) in sec[..len].chunks_mut(128).enumerate() {
                if !decode_block(blk) {
                    return false;
                }
                let f0 = off + (q * 128) as u32;
                for (t, &b) in blk.iter().enumerate() {
                    let f = f0 + t as u32;
                    if f < 12 {
                        h[f as usize] = b;
                    } else if !(12 + 0x4000..12 + 0x400C).contains(&f) {
                        crc = crc16_upd(crc, &[b]);
                    }
                }
            }
            off += len as u32;
        }
    }
    crate::fatren::header_valid(&h) && le32o(&h, 4) == size - 12 && le32o(&h, 8) & 0xFFFF == crc as u32
}

/// Check, then link the staged SPI clusters as DM40FW.BIN (upload.rs).
fn finalize(dev: &mut dyn BlockDevice, chain: &[u8], size: u32) {
    let w = w();
    let ok = check(chain, size) && commit(dev, chain, size);
    if ok {
        // Earlier committed files were deleted by the commit: reads zeros.
        for (i, x) in w.st.iter_mut().enumerate() {
            if chain.contains(&(i as u8)) {
                x.sealed = true;
            } else if x.sealed {
                *x = ST0;
            }
        }
        w.fw = Fw::Ask;
        w.n.fw_ok += 1;
    } else {
        for &i in chain {
            w.st[i as usize] = ST0;
        }
        w.fw = Fw::Bad;
        w.n.fw_bad += 1;
    }
    w.staging = w.st.iter().any(|x| x.c != 0 && !x.sealed);
}

fn commit(dev: &mut dyn BlockDevice, chain: &[u8], size: u32) -> bool {
    let w = w();
    let nb = (size as usize).div_ceil(BLK);
    let mut cl = [0u16; crate::upload::MAX as usize / BLK + 1];
    if nb > cl.len() {
        return false;
    }
    let [buf, fat, tmp] = crate::calbackup::scratch();
    if !dev.read_block(0, tmp) {
        return false;
    }
    let Some(v) = crate::fatren::parse_bpb(&tmp[..], dev.block_count()) else { return false };
    for b in 0..nb {
        let i = chain[b / BPC] as usize;
        let j = b % BPC;
        if w.st[i].blk[j] == 0 {
            // Every sector of it read as zeros: a zero block.
            let Ok(sc) = crate::upload::next_free(dev, &v, &mut w.hint, fat) else { return false };
            buf.fill(0);
            let lba = v.clba(sc as u32);
            if !(dev.write_block(lba, buf) && crate::fatren::verify(dev, lba, buf, tmp)) {
                return false;
            }
            w.st[i].blk[j] = sc;
        } else {
            // Unwritten sectors of a block are 0xFF on the flash but zeros
            // for the host: make them zeros.
            let first = j as u32 * SPB;
            let m = (w.st[i].map >> first) & 0xFF;
            if m != 0xFF {
                let lba = v.clba(w.st[i].blk[j] as u32);
                if !dev.read_block(lba, buf) {
                    return false;
                }
                for q in 0..SPB {
                    if m & (1 << q) == 0 {
                        buf[q as usize * BS..(q as usize + 1) * BS].fill(0);
                    }
                }
                if !(dev.write_block(lba, buf) && crate::fatren::verify(dev, lba, buf, tmp)) {
                    return false;
                }
                w.st[i].map |= 0xFF << first;
            }
        }
        cl[b] = w.st[i].blk[j];
    }
    crate::upload::commit_on(dev, &cl[..nb], size).is_ok()
}

/// The host session ended (Allow, bus reset, or a lockless host's quiet
/// time): every record dropped, the tomb flags cleared (vdisk.rs
/// clear_flags; the next refresh regenerates). `reset`: a bus reset (a new
/// host). True if there was anything.
pub fn release_all(reset: bool) -> bool {
    let w = w();
    let had = w.session || w.boot != 0 || w.fat1 != 0 || w.rec.iter().any(|r| r.slot != 0) || w.pair.iter().any(|p| p.c != 0) || w.st.iter().any(|x| x.c != 0);
    w.boot = 0;
    w.fat1 = 0;
    w.rec = [REC0; NREC];
    w.pair = [PAIR0; NPAIR];
    w.st = [ST0; NST];
    w.staging = false;
    w.session = false;
    w.last = 0;
    w.done = (0, 0, 0);
    if reset {
        w.locks = false;
    }
    vdisk::clear_flags();
    had
}

pub fn release() -> bool {
    release_all(false)
}

// --- the meter's dialog (app.rs) ---------------------------------------------------

/// Some(true): a firmware file was committed, ask now; Some(false): a
/// DM40*.BIN that is no valid update file arrived (toast). Once each.
pub fn take_dialog() -> Option<bool> {
    let w = w();
    match w.fw {
        Fw::Ask => {
            w.fw = Fw::Asked;
            Some(true)
        }
        Fw::Bad => {
            w.fw = Fw::None;
            Some(false)
        }
        _ => None,
    }
}

/// The dialog's answer: Yes = install (main.rs `take_install`), No / timeout
/// = delete DM40FW.BIN (`poll`).
pub fn answer(yes: bool) {
    let w = w();
    if w.fw == Fw::Asked {
        w.fw = if yes { Fw::Install } else { Fw::Decline };
    }
}

/// Yes was answered: main.rs zeroes the loader marker and resets (as
/// DBG:UPDATE).
pub fn take_install() -> bool {
    let w = w();
    if w.fw == Fw::Install {
        w.fw = Fw::None;
        true
    } else {
        false
    }
}

/// Host test: the records as text.
#[cfg(not(target_os = "none"))]
pub fn debug_text() -> std::string::String {
    let w = w();
    let mut s = std::string::String::new();
    for r in w.rec.iter().filter(|r| r.slot != 0) {
        s += &std::format!("rec slot={} name={:?} attr={:#x} start={} size={}\n", r.slot, core::str::from_utf8(&r.e[..11]).unwrap_or("?"), r.e[11], le16o(&r.e, 0x1A), le32o(&r.e, 0x1C));
    }
    for p in w.pair.iter().filter(|p| p.c != 0) {
        s += &std::format!("pair {} -> {:#x}\n", p.c, p.v);
    }
    for x in w.st.iter().filter(|x| x.c != 0) {
        s += &std::format!("st c={} sealed={} map={:#x} blk={:?}\n", x.c, x.sealed, x.map, x.blk);
    }
    s += &std::format!("done={:?} fw={} staging={}\n", w.done, w.fw as u8, w.staging);
    s
}
