//! Host test for src/fatren.rs and src/calbackup.rs against a real FAT16 image
//! (run.sh builds one with mkfs.fat/mtools in the DM40 geometry: 4096-byte
//! sectors, 2 FATs, 512 root entries, 16 MiB).
//!   main <image> dry|live [marker-hex]  fatren::run (marker = the 12-byte
//!                                       record at 0x08008000; none = invalid)
//!   main <image> trigger                fatren::make_trigger
//!   main <image> backup <dir> [flash]   calbackup::run with the calbackup::FILES sources
//!                                       read from <dir>/CALIB.BIN etc.; [flash] = the
//!                                       bytes from 0x08004000 (installed body with the
//!                                       loader marker at +0x4000): also _DM40PRV.BIN
//!   main <image> shot                   screenshot::take_on with the synthetic screen
//!                                       of the `lcd` stub below (pattern()).
//! Live runs rewrite the image.
use std::{env, fs};

#[allow(dead_code)]
mod msc {
    /// Same trait as fw-rs src/msc.rs (kept in sync by hand; 6 methods,
    /// write_step with its default = the whole write at once).
    pub trait BlockDevice {
        fn present(&self) -> bool;
        fn block_size(&self) -> u32;
        fn block_count(&self) -> u32;
        fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> bool;
        fn write_block(&mut self, lba: u32, buf: &[u8]) -> bool;
        fn write_step(&mut self, lba: u32, buf: &[u8], st: &mut Step) -> Option<bool> {
            let _ = st;
            Some(self.write_block(lba, buf))
        }
    }
    #[derive(Clone, Copy, Default)]
    pub struct Step {
        pub started: bool,
        pub phase: u8,
        pub page: u8,
        pub dirty: u16,
        pub erase: bool,
        pub t0: u32,
    }
    pub static HOST_WRITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
}

/// Stubs of the fw-rs modules calbackup.rs refers to.
mod wdt {
    pub fn kick_if_feeding() {}
}
#[allow(dead_code)]
mod mmio {
    pub unsafe fn r32(_: u32) -> u32 {
        0
    }
}
#[allow(dead_code)]
mod clock {
    pub const HCLK_HZ: u32 = 200_000_000;
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

/// Stub of the fw-rs LCD: a synthetic 480x320 RGB565 screen (the same
/// pattern run.sh checks the BMP against).
#[allow(dead_code)]
mod lcd {
    pub const W: u16 = 480;
    pub const H: u16 = 320;
    pub type Color = u16;
    /// Rows y % 4 = 0..2: runs of 13, 3 and 2 equal pixels (run / literal
    /// coding of screenshot.rs CAP), else noise (CAP overflows part way, so
    /// the later rows come from the GRAM while writing).
    pub fn pattern(x: u32, y: u32) -> Color {
        match y % 4 {
            0 => ((x / 13) * 0x1234 + y) as u16,
            1 => ((x / 3) * 0x0F0F + y) as u16,
            2 => ((x / 2) * 0x3333 + y) as u16,
            _ => ((x * 0x9E37 + y * 0x79B9) ^ (y << 11)) as u16,
        }
    }
    pub fn read_span(x: u16, y: u16, n: u16, mut out: impl FnMut(Color)) {
        assert!(x as u32 + n as u32 <= W as u32 && y < H, "span outside the screen");
        for i in 0..n as u32 {
            out(pattern(x as u32 + i, y as u32));
        }
    }
}

#[path = "../../src/screenshot.rs"]
#[allow(dead_code)]
mod screenshot;

#[path = "../../src/fatren.rs"]
#[allow(dead_code)]
mod fatren;

#[path = "../../src/calbackup.rs"]
mod calbackup;

struct Img {
    d: Vec<u8>,
    writes: Vec<u32>,
    reads: Vec<u32>,
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
        self.reads.push(lba);
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
    let mut img = Img { d: fs::read(&a[1]).unwrap(), writes: vec![], reads: vec![] };
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
        let slices: Vec<&[u8]> = data.iter().map(|d| &d[..]).collect();
        let srcs: Vec<calbackup::Src> = calbackup::FILES
            .iter()
            .zip(&slices)
            .map(|(&(name, addr, _), d)| calbackup::Src { name, addr, data: d })
            .collect();
        let flash = a.get(4).map(|f| fs::read(f).unwrap());
        let fw = flash.as_ref().map(|f| calbackup::FwFile::from_flash(f).expect("flash: no valid marker"));
        let r = calbackup::run(&mut img, &srcs, fw.as_ref().map(|f| f as &dyn calbackup::Source));
        println!(
            "backup={:?} written=0x{:03x} conflict=0x{:03x} freed={} fw_skipped={}",
            r.status, r.written, r.conflict, r.freed, r.fw_skipped
        );
        // Geometry from the boot sector: blocks >= data_start are file data.
        let b = &img.d;
        let le16 = |o: usize| b[o] as u32 | (b[o + 1] as u32) << 8;
        let data_start = le16(0x0E) + b[0x10] as u32 * le16(0x16) + le16(0x11) * 32 / 4096;
        let data_reads: Vec<u32> = img.reads.iter().copied().filter(|&l| l >= data_start).collect();
        println!("  changed blocks: {:?}", img.writes);
        println!("  reads={} data_reads={:?}", img.reads.len(), data_reads);
        fs::write(&a[1], &img.d).unwrap();
        return;
    }
    if a[2] == "shot" {
        let r = screenshot::take_on(&mut img);
        let mc = screenshot::MEDIUM_CHANGED.load(std::sync::atomic::Ordering::Relaxed);
        println!("shot={:?} medium_changed={}", r, mc);
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
