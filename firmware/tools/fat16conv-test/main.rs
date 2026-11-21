//! Host test of src/fat16conv.rs: a FAT12 image as the BY25Q128ES units
//! come with (Windows format: 4096 B sectors, 2 per cluster, "System Volume
//! Information" with a file inside, CONFIG.BIN, LOGO.JPG) plus a fragmented
//! file, an empty file, a label and a deleted entry. Checks:
//!   1. converted: fatren::parse_bpb takes it (spc 1, 4087 clusters, data
//!      at 9), every file reads back the same through the new FAT, FAT 2 =
//!      FAT 1, the SVI entry and its long name deleted, its clusters free,
//!      block 4095 bad, no data block written;
//!   2. a second run: NotFat12, nothing written;
//!   3. a subdirectory, a chain into cluster 2, a cross-link: Refused,
//!      nothing written; a FAT16 volume: NotFat12;
//!   4. with an argument: a real SPIFLASH.BIN dump, converted the same way
//!      (files compared before / after).
#![allow(dead_code)]

use std::collections::BTreeMap;

mod msc {
    pub trait BlockDevice {
        fn present(&self) -> bool;
        fn block_size(&self) -> u32;
        fn block_count(&self) -> u32;
        fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> bool;
        fn write_block(&mut self, lba: u32, buf: &[u8]) -> bool;
    }
}

#[path = "../../src/fatren.rs"]
mod fatren;
#[path = "../../src/fat16conv.rs"]
mod fat16conv;

use msc::BlockDevice;

const B: usize = 4096;

struct Img {
    d: Vec<u8>,
    writes: Vec<u32>,
}

impl BlockDevice for Img {
    fn present(&self) -> bool {
        true
    }
    fn block_size(&self) -> u32 {
        B as u32
    }
    fn block_count(&self) -> u32 {
        (self.d.len() / B) as u32
    }
    fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> bool {
        let o = lba as usize * B;
        buf[..B].copy_from_slice(&self.d[o..o + B]);
        true
    }
    fn write_block(&mut self, lba: u32, buf: &[u8]) -> bool {
        let o = lba as usize * B;
        self.d[o..o + B].copy_from_slice(&buf[..B]);
        self.writes.push(lba);
        true
    }
}

static mut FAILS: u32 = 0;
fn t(name: &str, ok: bool) {
    println!("{} {name}", if ok { "ok  " } else { "FAIL" });
    if !ok {
        unsafe { FAILS += 1 };
    }
}

fn le16(b: &[u8], o: usize) -> u32 {
    b[o] as u32 | (b[o + 1] as u32) << 8
}
fn le32(b: &[u8], o: usize) -> u32 {
    le16(b, o) | le16(b, o + 2) << 16
}

// --- The FAT12 image -------------------------------------------------------

fn set12(f: &mut [u8], n: usize, v: u16) {
    let o = n * 3 / 2;
    if n & 1 == 0 {
        f[o] = v as u8;
        f[o + 1] = (f[o + 1] & 0xF0) | (v >> 8) as u8 & 0x0F;
    } else {
        f[o] = (f[o] & 0x0F) | (v << 4) as u8;
        f[o + 1] = (v >> 4) as u8;
    }
}

fn entry(name: &[u8; 11], attr: u8, cl: u16, size: u32) -> [u8; 32] {
    let mut e = [0u8; 32];
    e[..11].copy_from_slice(name);
    e[11] = attr;
    e[24] = 0x8A; // a date
    e[25] = 0x58;
    e[26..28].copy_from_slice(&cl.to_le_bytes());
    e[28..32].copy_from_slice(&size.to_le_bytes());
    e
}

fn lfn(seq: u8, chk: u8) -> [u8; 32] {
    let mut e = [0xFFu8; 32];
    e[0] = seq;
    e[11] = 0x0F;
    e[12] = 0;
    e[13] = chk;
    e[26] = 0;
    e[27] = 0;
    e
}

fn pattern(seed: u32, n: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2654435761).wrapping_add(1);
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

/// Old cluster c: blocks 2c+3, 2c+4.
fn old_block(c: u32) -> usize {
    (2 * c + 3) as usize
}

/// (name, chain, size): the files of the synthetic image.
fn files() -> Vec<([u8; 11], Vec<u16>, u32)> {
    vec![
        (*b"LOGO    JPG", (5..10).collect(), 39831),
        (*b"CONFIG  BIN", vec![71], 432),
        (*b"FRAG    TXT", vec![12, 30, 13, 2045], 4 * 8192 - 100),
        (*b"EMPTY   TXT", vec![], 0),
    ]
}

fn fat12_image() -> Img {
    let mut d = vec![0xFFu8; 4096 * B];
    // Boot sector as on the unit (Windows: "MSDOS5.0", FAT12).
    let b = &mut d[..B];
    b.fill(0);
    b[..3].copy_from_slice(&[0xEB, 0x3C, 0x90]);
    b[3..11].copy_from_slice(b"MSDOS5.0");
    b[0x0B..0x0D].copy_from_slice(&4096u16.to_le_bytes());
    b[0x0D] = 2;
    b[0x0E] = 1;
    b[0x10] = 2;
    b[0x11..0x13].copy_from_slice(&512u16.to_le_bytes());
    b[0x13..0x15].copy_from_slice(&4096u16.to_le_bytes());
    b[0x15] = 0xF8;
    b[0x16] = 1;
    b[0x26] = 0x29;
    b[0x2B..0x36].copy_from_slice(b"NO NAME    ");
    b[0x36..0x3E].copy_from_slice(b"FAT12   ");
    b[0x1FE] = 0x55;
    b[0x1FF] = 0xAA;
    // FAT12 (1 block each), root 3..6 (zeroed), data.
    let mut fat = vec![0u8; B];
    set12(&mut fat, 0, 0xFF8);
    set12(&mut fat, 1, 0xFFF);
    let mut root = vec![0u8; 4 * B];
    let mut ri = 0;
    let mut put = |root: &mut Vec<u8>, e: [u8; 32]| {
        root[ri * 32..ri * 32 + 32].copy_from_slice(&e);
        ri += 1;
    };
    put(&mut root, entry(b"DM40       ", 0x08, 0, 0));
    put(&mut root, lfn(0x42, 0x31));
    put(&mut root, lfn(0x01, 0x31));
    put(&mut root, entry(b"SYSTEM~1   ", 0x16, 2, 0));
    let mut old = entry(b"OLD     TXT", 0x20, 0, 0);
    old[0] = 0xE5; // deleted
    put(&mut root, old);
    // SVI: cluster 2 (a directory) and a file in it at cluster 3.
    set12(&mut fat, 2, 0xFFF);
    set12(&mut fat, 3, 0xFFF);
    let svi = old_block(2) * B;
    d[svi..svi + 2 * B].fill(0);
    d[svi..svi + 32].copy_from_slice(&entry(b".          ", 0x10, 2, 0));
    d[svi + 32..svi + 64].copy_from_slice(&entry(b"..         ", 0x10, 0, 0));
    d[svi + 64..svi + 96].copy_from_slice(&entry(b"INDEXE~1   ", 0x20, 3, 76));
    // A lost cluster (allocated, no entry): freed.
    set12(&mut fat, 50, 0xFFF);
    // A bad cluster: kept bad.
    set12(&mut fat, 60, 0xFF7);
    for (i, (name, chain, size)) in files().into_iter().enumerate() {
        put(&mut root, entry(&name, 0x20, chain.first().copied().unwrap_or(0), size));
        let data = pattern(i as u32 + 1, size as usize);
        for (k, &c) in chain.iter().enumerate() {
            let next = chain.get(k + 1).copied().unwrap_or(0xFFF);
            set12(&mut fat, c as usize, next);
            let o = old_block(c as u32) * B;
            let part = &data[(k * 2 * B).min(data.len())..((k + 1) * 2 * B).min(data.len())];
            d[o..o + part.len()].copy_from_slice(part);
        }
    }
    d[B..2 * B].copy_from_slice(&fat);
    d[2 * B..3 * B].copy_from_slice(&fat);
    d[3 * B..7 * B].copy_from_slice(&root);
    // Block 4095 (outside the old volume): something there.
    d[4095 * B..].fill(0);
    Img { d, writes: vec![] }
}

// --- Reading a FAT16 volume back -------------------------------------------

/// name -> content, read through the FAT16 volume; panics on a bad chain.
fn read16(img: &Img) -> (fatren::Vol, BTreeMap<String, Vec<u8>>, Vec<u8>) {
    let d = &img.d;
    let v = fatren::parse_bpb(&d[..B], 4096).expect("parse_bpb");
    let fat: Vec<u8> = d[v.fat_start as usize * B..(v.fat_start + v.fat_blocks) as usize * B].to_vec();
    let root = &d[v.root_start as usize * B..(v.root_start + v.root_blocks) as usize * B];
    let mut out = BTreeMap::new();
    for e in root.chunks(32) {
        if e[0] == 0 {
            break;
        }
        if e[0] == 0xE5 || e[11] == 0x0F || e[11] & 0x08 != 0 {
            continue;
        }
        let name = String::from_utf8_lossy(&e[..11]).to_string();
        let size = le32(e, 28) as usize;
        let mut c = le16(e, 26);
        let mut data = vec![];
        let mut n = 0;
        while c != 0 && c < 0xFFF8 {
            assert!((2..v.clusters + 2).contains(&c), "{name}: cluster {c}");
            let o = (v.data_start + (c - 2) * v.spc) as usize * B;
            data.extend_from_slice(&d[o..o + v.spc as usize * B]);
            c = le16(&fat, c as usize * 2);
            n += 1;
            assert!(n < 5000);
        }
        data.truncate(size);
        out.insert(name, data);
    }
    (v, out, fat)
}

/// name -> content through the FAT12 volume (for the real dump).
fn read12(img: &Img) -> BTreeMap<String, Vec<u8>> {
    let d = &img.d;
    let fat = &d[B..2 * B];
    let f12 = |n: u32| {
        let v = le16(fat, (n * 3 / 2) as usize);
        if n & 1 != 0 { v >> 4 } else { v & 0xFFF }
    };
    let mut out = BTreeMap::new();
    for e in d[3 * B..7 * B].chunks(32) {
        if e[0] == 0 {
            break;
        }
        if e[0] == 0xE5 || e[11] == 0x0F || e[11] & 0x18 != 0 {
            continue;
        }
        let name = String::from_utf8_lossy(&e[..11]).to_string();
        let size = le32(e, 28) as usize;
        let mut c = le16(e, 26);
        let mut data = vec![];
        while c != 0 && c < 0xFF8 {
            let o = old_block(c) * B;
            data.extend_from_slice(&d[o..o + 2 * B]);
            c = f12(c);
        }
        data.truncate(size);
        out.insert(name, data);
    }
    out
}

fn conv(img: &mut Img) -> fat16conv::Res {
    let mut bufs = Box::new([[0u8; B]; 3]);
    img.writes.clear();
    fat16conv::run(img, &mut bufs)
}

fn main() {
    use fat16conv::Res;
    // 1. The unit's FAT12 disk.
    let mut img = fat12_image();
    let before = read12(&img);
    let data_before = img.d[9 * B..4095 * B].to_vec();
    let r = conv(&mut img);
    t(&format!("converted ({r:?})"), r == Res::Done);
    t(&format!("only blocks 0..8 written ({:?})", img.writes), img.writes.iter().all(|&l| l <= 8) && img.writes.last() == Some(&0));
    t("data blocks unchanged", img.d[9 * B..4095 * B] == data_before[..]);
    let (v, after, fat) = read16(&img);
    t(&format!("FAT16 geometry (spc {} data {} clusters {})", v.spc, v.data_start, v.clusters), v.spc == 1 && v.data_start == 9 && v.clusters == 4087 && v.fat_blocks == 2 && v.root_start == 5);
    t("FS type FAT16", &img.d[0x36..0x3E] == b"FAT16   ");
    t("FAT 2 = FAT 1", img.d[B..3 * B] == img.d[3 * B..5 * B]);
    for (i, (name, _, size)) in files().into_iter().enumerate() {
        let n = String::from_utf8_lossy(&name).to_string();
        t(&format!("{n} same"), after.get(&n) == Some(&pattern(i as u32 + 1, size as usize)));
    }
    t(&format!("files: {:?}", after.keys().collect::<Vec<_>>()), after.len() == files().len());
    t("same files as the FAT12 read", before.iter().filter(|(k, _)| !k.starts_with("SYSTEM")).all(|(k, v)| after.get(k) == Some(v)));
    let root = &img.d[5 * B..6 * B];
    t("SVI entry and its long name deleted", root[3 * 32] == 0xE5 && root[32] == 0xE5 && root[2 * 32] == 0xE5);
    t("label kept", &root[..11] == b"DM40       ");
    let f = |n: u32| le16(&fat, n as usize * 2);
    t("SVI clusters (old 3 = new 2, 3) free", f(2) == 0 && f(3) == 0);
    t("lost cluster 50 freed", f(2 * 50 - 4) == 0 && f(2 * 50 - 3) == 0);
    t("bad cluster 60 bad", f(2 * 60 - 4) == 0xFFF7 && f(2 * 60 - 3) == 0xFFF7);
    t("block 4095 (cluster 4088) bad", f(4088) == 0xFFF7);
    t("old 2045 -> 4086, 4087", f(4086) == 4087 && f(4087) == 0xFFFF);
    t("FAT media", f(0) == 0xFFF8 && f(1) == 0xFFFF);
    // 2. Again: nothing.
    let r = conv(&mut img);
    t(&format!("second run NotFat12, no write ({r:?})"), r == Res::NotFat12 && img.writes.is_empty());
    // 3. Refusals.
    let refuse = |name: &str, f: &dyn Fn(&mut Img)| {
        let mut img = fat12_image();
        f(&mut img);
        let d0 = img.d.clone();
        let r = conv(&mut img);
        t(&format!("{name}: {r:?}, nothing written"), r == Res::Refused && img.writes.is_empty() && img.d == d0);
    };
    refuse("a subdirectory", &|img| {
        let o = 3 * B + 9 * 32;
        img.d[o..o + 32].copy_from_slice(&entry(b"LOGS       ", 0x10, 100, 0));
    });
    refuse("a file at cluster 2", &|img| {
        let o = 3 * B + 9 * 32;
        img.d[o..o + 32].copy_from_slice(&entry(b"X       BIN", 0x20, 2, 10));
    });
    refuse("a cross-link", &|img| {
        let o = 3 * B + 9 * 32;
        img.d[o..o + 32].copy_from_slice(&entry(b"X       BIN", 0x20, 7, 10));
    });
    refuse("a chain into free", &|img| {
        // LOGO's 5 -> 6: make 6 free.
        let mut fat = img.d[B..2 * B].to_vec();
        set12(&mut fat, 6, 0);
        img.d[B..2 * B].copy_from_slice(&fat);
    });
    let mut blank = Img { d: vec![0xFF; 4096 * B], writes: vec![] };
    t("blank: NotFat12", conv(&mut blank) == Res::NotFat12 && blank.writes.is_empty());
    // 4. A real dump.
    if let Some(p) = std::env::args().nth(1) {
        let mut img = Img { d: std::fs::read(&p).expect("dump"), writes: vec![] };
        let before = read12(&img);
        let r = conv(&mut img);
        t(&format!("dump: {r:?}"), r == Res::Done);
        let (_, after, _) = read16(&img);
        for (k, v) in &before {
            t(&format!("dump {k} ({} B) same", v.len()), after.get(k) == Some(v));
        }
        let out = format!("{p}.fat16");
        std::fs::write(&out, &img.d).unwrap();
        println!("     converted dump: {out}");
    }
    let n = unsafe { FAILS };
    if n != 0 {
        println!("{n} FAILED");
        std::process::exit(1);
    }
    println!("all ok");
}
