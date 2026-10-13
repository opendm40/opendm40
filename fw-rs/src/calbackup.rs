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
//!                                      no counters).
//! New files are appended (bit indices of the older ones never change); 8 is
//! the limit of the u8 masks. Internal flash and the system areas are only READ.
//! Disk: 180 KiB (45 clusters of 4 KiB) of the 16 MiB volume.
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
//! Wear: each file is compared with its source by reading it (directory entry
//! size + whole cluster chain + contents). Equal => nothing is written, not
//! even the directory entry. Only a missing or different file is (re)written;
//! there is only ever one copy per name (no versions).
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
//! Time: unchanged = one root block + one FAT block + 45 data blocks read
//! (~14 ms per 4 KiB block at the 3.125 MHz polled SPI clock: ~0.65 s; it was
//! ~0.15 s with 9 data blocks before ROMBOOT/LOADER) + the BACKUP.TXT CRC-32
//! over ~165 KiB (nibble table, ~10 ms). The first write (or a loader update)
//! adds ~47 block erase + program + read-back (~4-5 s once, ~3 s for a
//! LOADER.BIN rewrite). The FWDGT is fed between blocks
//! (wdt::kick_if_feeding; spiflash feeds during erases).
//!
//! Host test: tools/fatren-test/run.sh.

#![allow(dead_code)]

use crate::fatren::{free_chain, le16, le32, parse_bpb, sfn_is, verify, Vol};
use crate::msc::BlockDevice;

const BLK: usize = 4096;
/// Most clusters one run allocates (45 with the device geometry: all files).
const MAX_NEW: usize = 64;
/// Root slots per 4 KiB block.
const PER: u32 = (BLK / 32) as u32;
/// Write date 2026-10-13 (FAT: (year - 1980) << 9 | month << 5 | day), time 0.
/// Constant (no RTC): a varying date would make every rewrite differ.
const DATE: u16 = (46 << 9) | (10 << 5) | 13;

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
/// FILES + BACKUP.TXT.
pub const N: usize = FILES.len() + 1;
const _: () = assert!(N <= 8, "written / conflict are u8 bit masks");

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
    /// Bit i = file i (FILES order, BACKUP.TXT last) (re)written.
    pub written: u8,
    /// Bit i = the name is taken by a directory / volume label: skipped.
    pub conflict: u8,
    /// Old chains freed (clusters).
    pub freed: u32,
}

static mut RESULT: Result = Result { status: Status::NotRun, written: 0, conflict: 0, freed: 0 };

pub fn result() -> Result {
    unsafe { RESULT }
}

/// One file: 8.3 name, source address (for BACKUP.TXT), contents.
#[derive(Clone, Copy)]
pub struct Src<'a> {
    pub name: [u8; 11],
    pub addr: u32,
    pub data: &'a [u8],
}

/// Boot entry: back up the internal sources (the RAM build and native).
/// Never inlined: its ~14 KiB frame is gone before main() runs (review
/// 2026-10-13 B1).
#[inline(never)]
pub fn boot() {
    if !cfg!(feature = "calbackup") {
        return;
    }
    let mut srcs = [Src { name: [0; 11], addr: 0, data: &[] }; FILES.len()];
    for (s, &(name, addr, len)) in srcs.iter_mut().zip(FILES.iter()) {
        // Read-only views of internal flash / system memory.
        *s = Src { name, addr, data: unsafe { core::slice::from_raw_parts(addr as *const u8, len as usize) } };
    }
    let mut disk = crate::spiflash::SpiFlash::new();
    run(&mut disk, &srcs);
}

/// Back up `srcs` (+ BACKUP.TXT describing them). Records and returns the result.
#[inline(never)]
pub fn run(dev: &mut dyn BlockDevice, srcs: &[Src]) -> Result {
    let mut text = [0u8; 1024];
    let n = describe(srcs, &mut text);
    let mut all = [Src { name: TXT, addr: 0, data: &[] }; N];
    let k = srcs.len().min(N - 1);
    all[..k].copy_from_slice(&srcs[..k]);
    all[k] = Src { name: TXT, addr: 0, data: &text[..n] };
    let r = run_inner(dev, &all[..k + 1]);
    unsafe { RESULT = r };
    r
}

// --- BACKUP.TXT ----------------------------------------------------------------

/// CRC-32 (ISO-HDLC, as zlib / `crc32` / 7-Zip report it). Nibble table
/// (64 bytes): ~4x the bit loop, ~165 KiB of sources at every boot.
pub fn crc32(d: &[u8]) -> u32 {
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
    let mut c = !0u32;
    for &b in d {
        c ^= b as u32;
        c = (c >> 4) ^ T[(c & 15) as usize];
        c = (c >> 4) ^ T[(c & 15) as usize];
    }
    !c
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

fn describe(srcs: &[Src], out: &mut [u8]) -> usize {
    let mut w = W { b: out, n: 0 };
    w.s(b"DM40 backup written by fw-rs at boot (only when missing or changed).\r\n");
    w.s(b"Raw copies of internal flash / chip data; CRC-32 as zlib, 7-Zip, crc32.\r\n");
    w.s(b"file         address     bytes  crc32\r\n");
    for s in srcs {
        w.name(&s.name);
        w.hex(s.addr);
        w.s(b" ");
        w.dec(s.data.len() as u32);
        w.s(b"  ");
        w.hex(crc32(s.data));
        w.s(b"\r\n");
    }
    w.s(b"\r\n");
    w.s(b"Full restore after a mass erase (SWD): write ROMBOOT.BIN at 0x08000000,\r\n");
    w.s(b"then LOADER.BIN at 0x080A0000 and CALIB.BIN at 0x080C0000, then install\r\n");
    w.s(b"a firmware. BOOTDATA.BIN = LOADER.BIN bytes 0x4000..0x7FFF (kept for\r\n");
    w.s(b"older copies). The loader data is bound to UID.BIN: this unit only.\r\n");
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
/// size == data.len() and the contents equal `data`. None = read error.
fn walk(dev: &mut dyn BlockDevice, v: &Vol, start: u32, size: u32, data: &[u8], fat: &mut [u8; BLK], buf: &mut [u8; BLK]) -> Option<(bool, bool)> {
    let n = nclusters(v, size);
    let max = v.clusters + 1;
    if n == 0 {
        return Some((start == 0, start == 0 && data.is_empty()));
    }
    if n > v.clusters {
        return Some((false, false));
    }
    let mut equal = size as usize == data.len();
    let mut cur = u32::MAX;
    let mut c = start;
    for i in 0..n {
        if c < 2 || c > max {
            return Some((false, false));
        }
        if equal {
            for b in 0..v.spc {
                let off = i as usize * cbytes(v) + b as usize * BLK;
                if off >= data.len() {
                    break;
                }
                if !dev.read_block(lba_of(v, c) + b, buf) {
                    return None;
                }
                let end = (off + BLK).min(data.len());
                if buf[..end - off] != data[off..end] {
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
}

const NO_JOB: Job = Job { file: 0, slot: 0, new: false, old: 0, first: 0, n: 0 };

fn run_inner(dev: &mut dyn BlockDevice, files: &[Src]) -> Result {
    let mut res = Result { status: Status::Error, written: 0, conflict: 0, freed: 0 };
    if !dev.present() || dev.block_size() as usize != BLK {
        res.status = Status::NoMedium;
        return res;
    }
    let mut buf = [0u8; BLK];
    let mut tmp = [0u8; BLK];
    let mut fat = [0u8; BLK];
    if !dev.read_block(0, &mut buf) {
        return res;
    }
    let Some(v) = parse_bpb(&buf, dev.block_count()) else {
        res.status = Status::BadBpb;
        return res;
    };

    // Root scan: the live entry of each name (slot, start, size), free slots.
    let mut found: [Option<(u32, u32, u32)>; N] = [None; N];
    let mut free = [0u32; N];
    let mut nfree = 0usize;
    'scan: for blk in 0..v.root_blocks {
        if !dev.read_block(v.root_start + blk, &mut buf) {
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
                        found[i] = Some((idx, le16(e, 0x1A), le32(e, 0x1C)));
                    }
                }
            }
        }
    }

    // Which files differ? Plan new chains for them.
    let mut jobs = [NO_JOB; N];
    let mut nj = 0usize;
    let mut need = 0usize;
    let mut nnew = 0usize;
    for (i, f) in files.iter().enumerate() {
        if res.conflict & (1 << i) != 0 {
            continue;
        }
        let mut j = Job { file: i, first: need, n: nclusters(&v, f.data.len() as u32) as usize, ..NO_JOB };
        match found[i] {
            Some((slot, start, size)) => {
                let Some((wf, eq)) = walk(dev, &v, start, size, f.data, &mut fat, &mut buf) else {
                    return res;
                };
                if eq {
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
        match fat_get(dev, &v, c, &mut fat, &mut cur) {
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
        let d = files[j.file].data;
        for (i, &c) in cl[j.first..j.first + j.n].iter().enumerate() {
            for b in 0..v.spc {
                let off = i * cbytes(&v) + b as usize * BLK;
                if off >= d.len() {
                    break;
                }
                let lba = lba_of(&v, c) + b;
                if !dev.read_block(lba, &mut buf) {
                    return res;
                }
                let end = (off + BLK).min(d.len());
                buf[..end - off].copy_from_slice(&d[off..end]);
                if !(dev.write_block(lba, &buf) && verify(dev, lba, &buf, &mut tmp)) {
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
            if !dev.read_block(lba, &mut buf) {
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
            if !(dev.write_block(lba, &buf) && verify(dev, lba, &buf, &mut tmp)) {
                return res;
            }
            if fb < 32 {
                done |= 1 << fb;
            }
            crate::wdt::kick_if_feeding();
        }
    }

    // 4. Directory entries, one write per touched root block.
    for blk in 0..v.root_blocks {
        let (lo, hi) = (blk * PER, blk * PER + PER);
        if !jobs[..nj].iter().any(|j| j.slot >= lo && j.slot < hi) {
            continue;
        }
        let lba = v.root_start + blk;
        if !dev.read_block(lba, &mut buf) {
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
            let start = if j.n == 0 { 0 } else { cl[j.first] as u16 };
            e[0x1A..0x1C].copy_from_slice(&start.to_le_bytes());
            e[0x1C..0x20].copy_from_slice(&(files[j.file].data.len() as u32).to_le_bytes());
        }
        if !(dev.write_block(lba, &buf) && verify(dev, lba, &buf, &mut tmp)) {
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
            let (ok, freed) = free_chain(dev, &v, j.old, &mut fat, &mut buf, &mut tmp);
            res.freed += freed;
            if !ok {
                return res;
            }
        }
    }

    // 6. Read every written file back through its new chain.
    for j in &jobs[..nj] {
        let d = files[j.file].data;
        let start = if j.n == 0 { 0 } else { cl[j.first] };
        match walk(dev, &v, start, d.len() as u32, d, &mut fat, &mut buf) {
            Some((true, true)) => {}
            _ => return res,
        }
    }
    res.status = Status::Written;
    res
}
