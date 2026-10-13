//! Host test for src/fatren.rs and src/calbackup.rs against a real FAT16 image
//! (run.sh builds one with mkfs.fat/mtools in the DM40 geometry: 4096-byte
//! sectors, 2 FATs, 512 root entries, 16 MiB).
//!   main <image> dry|live [marker-hex]  fatren::run (marker = the 12-byte
//!                                       record at 0x08008000; none = invalid)
//!   main <image> trigger                fatren::make_trigger
//!   main <image> backup <dir>           calbackup::run with the calbackup::FILES sources
//!                                       read from <dir>/CALIB.BIN etc.
//! Live runs rewrite the image.
use std::{env, fs};

mod msc {
    /// Same trait as fw-rs src/msc.rs (kept in sync by hand; 5 methods).
    pub trait BlockDevice {
        fn present(&self) -> bool;
        fn block_size(&self) -> u32;
        fn block_count(&self) -> u32;
        fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> bool;
        fn write_block(&mut self, lba: u32, buf: &[u8]) -> bool;
    }
}

/// Stubs of the fw-rs modules calbackup.rs refers to.
mod wdt {
    pub fn kick_if_feeding() {}
}
#[allow(dead_code)]
mod spiflash {
    pub struct SpiFlash;
    impl SpiFlash {
        pub fn new() -> Self {
            SpiFlash
        }
    }
    impl crate::msc::BlockDevice for SpiFlash {
        fn present(&self) -> bool {
            false
        }
        fn block_size(&self) -> u32 {
            4096
        }
        fn block_count(&self) -> u32 {
            0
        }
        fn read_block(&mut self, _: u32, _: &mut [u8]) -> bool {
            false
        }
        fn write_block(&mut self, _: u32, _: &[u8]) -> bool {
            false
        }
    }
}

#[path = "../../src/fatren.rs"]
#[allow(dead_code)]
mod fatren;

#[path = "../../src/calbackup.rs"]
mod calbackup;

struct Img {
    d: Vec<u8>,
    writes: Vec<u32>,
}

impl msc::BlockDevice for Img {
    fn present(&self) -> bool {
        true
    }
    fn block_size(&self) -> u32 {
        4096
    }
    fn block_count(&self) -> u32 {
        (self.d.len() / 4096) as u32
    }
    fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> bool {
        let o = lba as usize * 4096;
        buf[..4096].copy_from_slice(&self.d[o..o + 4096]);
        true
    }
    fn write_block(&mut self, lba: u32, buf: &[u8]) -> bool {
        let o = lba as usize * 4096;
        if self.d[o..o + 4096] != buf[..4096] {
            self.writes.push(lba);
        }
        self.d[o..o + 4096].copy_from_slice(&buf[..4096]);
        true
    }
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn main() {
    let a: Vec<String> = env::args().collect();
    let mut img = Img { d: fs::read(&a[1]).unwrap(), writes: vec![] };
    if a[2] == "trigger" {
        let t = fatren::make_trigger(&mut img);
        println!("trigger={:?}\n  changed blocks: {:?}", t, img.writes);
        fs::write(&a[1], &img.d).unwrap();
        return;
    }
    if a[2] == "backup" {
        let data: Vec<Vec<u8>> = calbackup::FILES
            .iter()
            .map(|(n, _, _)| {
                let name = format!(
                    "{}.{}",
                    String::from_utf8_lossy(&n[..8]).trim_end(),
                    String::from_utf8_lossy(&n[8..]).trim_end()
                );
                fs::read(format!("{}/{}", a[3], name)).unwrap()
            })
            .collect();
        let srcs: Vec<calbackup::Src> = calbackup::FILES
            .iter()
            .zip(&data)
            .map(|(&(name, addr, _), d)| calbackup::Src { name, addr, data: d })
            .collect();
        let r = calbackup::run(&mut img, &srcs);
        println!(
            "backup={:?} written=0x{:02x} conflict=0x{:02x} freed={}",
            r.status, r.written, r.conflict, r.freed
        );
        println!("  changed blocks: {:?}", img.writes);
        fs::write(&a[1], &img.d).unwrap();
        return;
    }
    let dry = a[2] == "dry";
    let marker = a.get(3).map(|h| {
        let mut m = [0u8; 12];
        m.copy_from_slice(&unhex(h));
        m
    });
    let r = fatren::run(&mut img, dry, marker);
    println!(
        "status={:?} found={} renamed='{}' old_backup={} old_deleted={} freed_clusters={} others={} staged={} fake_deleted={}",
        r.status,
        r.found,
        String::from_utf8_lossy(&r.renamed),
        r.old_backup,
        r.old_deleted,
        r.freed_clusters,
        r.others,
        r.staged,
        r.fake_deleted
    );
    println!("  changed blocks: {:?}", img.writes);
    if !dry {
        fs::write(&a[1], &img.d).unwrap();
    }
}
