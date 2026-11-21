//! The FAT12 disk of the later DM40s, made the DM40 FAT16 volume in place.
//!
//! Until 2026 the SPI flash came formatted FAT16: 4096 B sectors, 1 sector
//! per cluster, 2 FATs of 2 sectors, 512 root entries, 4087 clusters
//! (docs/native-boot.md). Units with the BoyaMicro BY25Q128ES come with a
//! Windows-made FAT12 volume on the same 16 MiB: 2 sectors per cluster, 2
//! FATs of 1 sector, 2044 clusters ("MSDOS5.0", "System Volume
//! Information", CONFIG.BIN, LOGO.JPG). Every opendm40 disk writer wants
//! the FAT16 one (fatren.rs parse_bpb), so the gallery said "No disk".
//!
//!   old  0 BPB | 1 FAT1 | 2 FAT2 | 3..6 root | 7.. data, cluster c = 2c+3, 2c+4
//!   new  0 BPB | 1..2 FAT1 | 3..4 FAT2 | 5..8 root | 9.. data, cluster c' = c'+7
//!
//! So every data block from 9 on stays where it is: old cluster c >= 3 is
//! the new clusters 2c-4 and 2c-3, chained. Only blocks 0..8 are written:
//! old cluster 2 (blocks 7, 8) becomes the end of the root, so it must be
//! free or the "System Volume Information" directory, which is dropped
//! (Windows makes it again). The new FAT holds the root files' chains only,
//! so the dropped directory's clusters and lost clusters come out free.
//! Any other directory (cluster numbers inside) or anything unexpected:
//! nothing is written. The last new cluster (block 4095, outside the old
//! volume) is marked bad, so a byte the stock firmware may keep there is
//! never overwritten.
//!
//! Order: everything is read and checked first, then root (8, 7, 6, 5 from
//! old 6, 5, 4, 3: backwards, so no old block is overwritten before it is
//! read), FAT 2, FAT 1, the boot sector last. A power loss inside those
//! ~0.5 s leaves a damaged disk (the old root is overwritten from the third
//! block on); the window is the boot after the first install only.
//! Runs before fatren in native.rs pre_main (region C).

#![cfg_attr(not(target_os = "none"), allow(dead_code))]
use core::sync::atomic::{AtomicU8, Ordering};

use crate::msc::BlockDevice;

/// This boot's `Res` (DBG:FLASH? last field: 0 not FAT12, 1 refused,
/// 2 converted, 3 failed).
static LAST: AtomicU8 = AtomicU8::new(0);

#[inline(always)]
pub fn last() -> u8 {
    LAST.load(Ordering::Relaxed)
}

/// native.rs pre_main: before fatren and calbackup (they want FAT16).
#[cfg(target_os = "none")]
#[inline(never)]
pub fn boot() {
    let mut disk = crate::spiflash::SpiFlash::new();
    let r = run(&mut disk, crate::diskbuf::blocks());
    LAST.store(r as u8, Ordering::Relaxed);
}

const BLK: usize = 4096;
/// The DM40 volume: 16 MiB of 4 KiB blocks.
const TOT: u32 = 4096;
const OLD_CL: u32 = (TOT - 7) / 2; // 2044
const NEW_CL: u32 = TOT - 9; // 4087

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Res {
    /// Not this FAT12 volume (the FAT16 one, blank, foreign): untouched.
    NotFat12,
    /// The FAT12 volume, but not convertible in place: untouched.
    Refused,
    /// Converted.
    Done,
    /// A write failed part way.
    Failed,
}

fn le16(b: &[u8], o: usize) -> u32 {
    b[o] as u32 | (b[o + 1] as u32) << 8
}

fn fat12(f: &[u8], n: u32) -> u32 {
    let o = (n * 3 / 2) as usize;
    let v = le16(f, o);
    if n & 1 != 0 { v >> 4 } else { v & 0xFFF }
}

fn put16(b: &mut [u8], n: u32, v: u32) {
    let o = (n as usize) * 2;
    b[o] = v as u8;
    b[o + 1] = (v >> 8) as u8;
}

/// The boot sector is the later DM40s' FAT12 volume.
fn is_old(b: &[u8]) -> bool {
    (b[0] == 0xEB || b[0] == 0xE9)
        && b[0x1FE] == 0x55
        && b[0x1FF] == 0xAA
        && le16(b, 0x0B) == BLK as u32
        && b[0x0D] == 2
        && le16(b, 0x0E) == 1
        && b[0x10] == 2
        && le16(b, 0x11) == 512
        && le16(b, 0x16) == 1
        && (le16(b, 0x13) == TOT || (le16(b, 0x13) == 0 && le16(b, 0x20) == TOT && le16(b, 0x22) == 0))
}

/// The "System Volume Information" entry: a directory at old cluster 2.
fn is_svi(e: &[u8]) -> bool {
    e[11] & 0x10 != 0 && le16(e, 26) == 2 && &e[..8] == b"SYSTEM~1"
}

/// Check (`fix` false) or translate (`fix` true) one root block. Check:
/// false = refuse. Translate: clusters c -> 2c-4, the SVI entry and its
/// long-name entries deleted.
fn root_block(b: &mut [u8], fix: bool) -> bool {
    for i in 0..BLK / 32 {
        let e = &mut b[i * 32..i * 32 + 32];
        if e[0] == 0 {
            break;
        }
        if e[0] == 0xE5 || e[11] == 0x0F || e[11] & 0x08 != 0 {
            continue; // deleted, long name, volume label
        }
        let c = le16(e, 26);
        if le16(e, 20) != 0 {
            return false;
        }
        if is_svi(e) {
            if fix {
                e[0] = 0xE5;
                let mut j = i;
                while j > 0 && b[(j - 1) * 32 + 11] == 0x0F && b[(j - 1) * 32] != 0xE5 {
                    j -= 1;
                    b[j * 32] = 0xE5;
                }
            }
            continue;
        }
        if e[11] & 0x10 != 0 || c == 1 || c == 2 || c >= OLD_CL + 2 {
            return false; // a directory, or a cluster number out of place
        }
        if fix && c != 0 {
            let n = 2 * c - 4;
            e[26] = n as u8;
            e[27] = (n >> 8) as u8;
        }
    }
    true
}

/// Convert the volume if it is the FAT12 one. `bufs`: three 4 KiB blocks
/// (diskbuf::blocks()).
#[inline(never)]
pub fn run(dev: &mut dyn BlockDevice, bufs: &mut [[u8; BLK]; 3]) -> Res {
    let [a, f1, f2] = bufs;
    if dev.block_count() < TOT || !dev.read_block(0, a) || !is_old(a) {
        return Res::NotFat12;
    }
    if !dev.read_block(1, a) {
        return Res::Refused;
    }
    // The clusters of the root's files (their chains in the old FAT, in
    // `a`): only those go to the new FAT, so the SVI directory's clusters
    // and lost clusters come out free. A chain that leaves the volume,
    // meets cluster 2, loops or crosses another: refuse.
    let mut used = [0u32; (OLD_CL as usize + 2).div_ceil(32)];
    for lba in 3..7 {
        if !dev.read_block(lba, f1) || !root_block(f1, false) {
            return Res::Refused;
        }
        for e in f1.chunks(32) {
            if e[0] == 0 {
                break;
            }
            if e[0] == 0xE5 || e[11] == 0x0F || e[11] & 0x18 != 0 {
                continue;
            }
            let mut c = le16(e, 26);
            while c != 0 {
                if !(3..OLD_CL + 2).contains(&c) || used[c as usize / 32] & 1 << (c % 32) != 0 {
                    return Res::Refused;
                }
                used[c as usize / 32] |= 1 << (c % 32);
                let v = fat12(a, c);
                c = match v {
                    0xFF8.. => 0,
                    0 => return Res::Refused,
                    v => v,
                };
            }
        }
    }
    // The FAT16 (two blocks, 4096 entries).
    f1.fill(0);
    f2.fill(0);
    let set = |f1: &mut [u8; BLK], f2: &mut [u8; BLK], n: u32, v: u32| {
        if n < 2048 { put16(f1, n, v) } else { put16(f2, n - 2048, v) }
    };
    set(f1, f2, 0, 0xFFF8);
    set(f1, f2, 1, 0xFFFF);
    // Block 4095: not in the old volume.
    set(f1, f2, NEW_CL + 1, 0xFFF7);
    for c in 3..OLD_CL + 2 {
        let n = 2 * c - 4;
        let v = fat12(a, c);
        if v == 0xFF7 {
            set(f1, f2, n, 0xFFF7);
            set(f1, f2, n + 1, 0xFFF7);
        } else if used[c as usize / 32] & 1 << (c % 32) != 0 {
            set(f1, f2, n, n + 1);
            set(f1, f2, n + 1, if v >= 0xFF8 { 0xFFFF } else { 2 * v - 4 });
        }
    }
    // Root: new 8..5 from old 6..3, translated.
    for lba in (5..9).rev() {
        if !dev.read_block(lba - 2, a) {
            return Res::Failed;
        }
        root_block(a, true);
        if !dev.write_block(lba, a) {
            return Res::Failed;
        }
    }
    for base in [3, 1] {
        if !dev.write_block(base, f1) || !dev.write_block(base + 1, f2) {
            return Res::Failed;
        }
    }
    if !dev.read_block(0, a) {
        return Res::Failed;
    }
    a[0x0D] = 1;
    a[0x16] = 2;
    a[0x17] = 0;
    if a[0x26] == 0x29 {
        a[0x36..0x3E].copy_from_slice(b"FAT16   ");
    }
    if !dev.write_block(0, a) {
        return Res::Failed;
    }
    Res::Done
}
