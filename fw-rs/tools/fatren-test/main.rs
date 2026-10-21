//! Host test for src/fatren.rs, src/calbackup.rs (+ src/vdisk.rs) against a real FAT16 image
//! (run.sh builds one with mkfs.fat/mtools in the DM40 geometry: 4096-byte
//! sectors, 2 FATs, 512 root entries, 16 MiB).
//!   main <image> dry|live [marker-hex]  fatren::run (marker = the 12-byte
//!                                       record at 0x08008000; none = invalid)
//!   main <image> trigger                fatren::make_trigger
//!   main <image> backup <dir>           calbackup::safe_copy (write-once safety copy)
//!                                       of calbackup::FILES[..SAFE] read from
//!                                       <dir>/CALIB.BIN, BOOTDATA.BIN, UID.BIN
//!   main <out> vdisk <flash> <uid> <otp> <optbytes> [extra <n> <size>]
//!                                       vdisk.rs (USB LUN 1 information volume) over
//!                                       the 1 MiB <flash> image (0x08000000..) and the
//!                                       system-memory files: every sector read through
//!                                       read_block into <out> (created), twice (must be
//!                                       equal); write_block / out-of-range reads refused;
//!                                       extra: vdisk::set_extra with n files LOGnnn.CSV of
//!                                       size * (i + 1) bytes, contents extra_byte(i, off)
//!   main <image> shot                   screenshot::take_on with the synthetic screen
//!                                       of the `lcd` stub below (pattern()).
//!   main <image> shotcut [k [abort]]    the same slice by slice on a NOR model of
//!                                       write_step, cut after k slices (or stopped
//!                                       there by screenshot::abort_on); see shotcut().
//!   main <image> shotmounted            start() with a host-mounted disk (refused).
//!   main <image> csv rec <rows> [cut <k> [half]] [shot <row>]... [host <row> <rows>] [mount|mountwin <row> <rows>]
//!                                       csvlog recording of <rows> synthetic rows
//!                                       (row(i) below), polled pass by pass;
//!                                       cut = power cut before the k-th changing
//!                                       block write (half: a program-only write is
//!                                       cut half way, 8 of 16 pages); shot = start
//!                                       a screenshot at that row (interleaved);
//!                                       host = HOST_WRITING set for that many rows;
//!                                       mount = msc::host_mounted() for that many rows
//!                                       (mountwin: the same window, not mounted);
//!                                       mount_writes = block writes in that window.
//!   main <image> csv list | del <NAME> [mounted] | busydel
//!                                       gallery list / delete; busydel = delete the
//!                                       file being recorded (must be refused).
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
        fn read_only(&self) -> bool {
            false
        }
        fn take_media_changed(&mut self) -> bool {
            false
        }
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
    pub static LOG_SESSION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub static LOG_FLUSHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    /// Test knob for msc.rs `host_mounted()` (a host sent PREVENT MEDIUM REMOVAL).
    pub static HOST_MOUNTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub fn host_mounted() -> bool {
        HOST_MOUNTED.load(std::sync::atomic::Ordering::Relaxed)
    }
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

#[path = "../../src/csvlog.rs"]
#[allow(dead_code)]
mod csvlog;

#[path = "../../src/fatren.rs"]
#[allow(dead_code)]
mod fatren;

#[path = "../../src/calbackup.rs"]
mod calbackup;

#[path = "../../src/vdisk.rs"]
mod vdisk;

/// Memory map for vdisk's reader: (address, bytes).
static MEM: std::sync::OnceLock<Vec<(u32, Vec<u8>)>> = std::sync::OnceLock::new();

fn mem_read(addr: u32, buf: &mut [u8]) {
    for (base, d) in MEM.get().unwrap() {
        if addr >= *base && (addr - base) as usize + buf.len() <= d.len() {
            let o = (addr - base) as usize;
            buf.copy_from_slice(&d[o..o + buf.len()]);
            return;
        }
    }
    panic!("vdisk read outside the memory map: {addr:#010x} + {}", buf.len());
}

/// Contents of the synthetic extra file `arg` (run.sh recomputes it).
fn extra_read(arg: u32, off: u32, buf: &mut [u8]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = b"0123456789,\n"[(arg as usize * 5 + off as usize + i) % 12];
    }
}

/// `main <out> vdisk <flash> <uid> <otp> <optbytes> [extra <n> <size>]`.
fn vdisk_dump(a: &[String]) {
    use msc::BlockDevice;
    let flash = fs::read(&a[3]).unwrap();
    assert_eq!(flash.len(), 1 << 20, "1 MiB flash image");
    let body: &'static [u8] = Vec::leak(flash[0x4000..0x4000 + calbackup::FW_MAX_BODY].to_vec());
    MEM.set(vec![
        (0x0800_0000, flash),
        (0x1FFF_7A10, fs::read(&a[4]).unwrap()),
        (0x1FFF_7800, fs::read(&a[5]).unwrap()),
        (0x1FFF_C000, fs::read(&a[6]).unwrap()),
    ])
    .unwrap();
    let mut d = vdisk::VDisk;
    println!("present_before_init={}", d.present());
    vdisk::init(mem_read, body);
    println!("media_changed_after_init={}", d.take_media_changed());
    if a.get(7).map(|s| s.as_str()) == Some("extra") {
        let n: usize = a[8].parse().unwrap();
        let size: u32 = a[9].parse().unwrap();
        let x: Vec<vdisk::Extra> = (0..n)
            .map(|i| vdisk::Extra { name: calbackup::numbered_name(b"LOG", i as u16, b"CSV"), size: size * (i as u32 + 1), read: extra_read, arg: i as u32 })
            .collect();
        println!("set_extra taken={}", vdisk::set_extra(&x));
        println!("media_changed={} again={}", d.take_media_changed(), d.take_media_changed());
    }
    let n = d.block_count();
    let bs = d.block_size() as usize;
    let mut img = vec![0u8; n as usize * bs];
    for pass in 0..2 {
        let mut b = vec![0u8; bs];
        for lba in 0..n {
            assert!(d.read_block(lba, &mut b), "read {lba}");
            let o = lba as usize * bs;
            if pass == 0 {
                img[o..o + bs].copy_from_slice(&b);
            } else {
                assert!(img[o..o + bs] == b[..], "sector {lba} differs between two reads");
            }
        }
    }
    let mut b = vec![0u8; bs];
    let past = d.read_block(n, &mut b);
    let wr = d.write_block(0, &img[..bs]);
    println!("vdisk sectors={} bs={} read_only={} write_block={} read_past_end={}", n, bs, d.read_only(), wr, past);
    fs::write(&a[1], &img).unwrap();
}

struct Img {
    d: Vec<u8>,
    writes: Vec<u32>,
    reads: Vec<u32>,
    /// NOR model: erases per block (a write that needs any 0 -> 1 bit).
    erases: std::collections::HashMap<u32, u32>,
    programs: u32,
    /// Power cut before changing write number `cut` (0-based); `half`: a
    /// program-only write being cut keeps its first 8 pages.
    cut: Option<usize>,
    half: bool,
    dead: bool,
    cut_kind: &'static str,
    /// NOR model of spiflash.rs `write_step` (screenshot tests): the first
    /// call compares and, if any bit must go 0 -> 1, erases the block (all
    /// 0xFF on the "chip" from then on); each further call programs 4 pages
    /// (old & new); the last one reads back. Off: `write_step` = `write_block`.
    nor_step: bool,
}

impl Img {
    fn new(d: Vec<u8>) -> Self {
        Img { d, writes: vec![], reads: vec![], erases: Default::default(), programs: 0, cut: None, half: false, dead: false, cut_kind: "none", nor_step: false }
    }
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
        if self.dead {
            return false;
        }
        if self.d[o..o + 4096] != buf[..4096] {
            let erase = self.d[o..o + 4096].iter().zip(&buf[..4096]).any(|(&a, &b)| a & b != b);
            if self.cut == Some(self.writes.len()) {
                self.dead = true;
                self.cut_kind = if erase { "erase-write (not started)" } else { "program-only" };
                if self.half && !erase {
                    self.d[o..o + 2048].copy_from_slice(&buf[..2048]);
                    self.cut_kind = "program-only, half applied";
                }
                return false;
            }
            self.writes.push(lba);
            if erase {
                *self.erases.entry(lba).or_insert(0) += 1;
            } else {
                self.programs += 1;
            }
        }
        self.d[o..o + 4096].copy_from_slice(&buf[..4096]);
        true
    }
    fn write_step(&mut self, lba: u32, buf: &[u8], st: &mut msc::Step) -> Option<bool> {
        if !self.nor_step {
            return Some(self.write_block(lba, buf));
        }
        let o = lba as usize * 4096;
        match st.phase {
            0 => {
                st.started = true;
                if self.d[o..o + 4096] == buf[..4096] {
                    return Some(true);
                }
                st.erase = self.d[o..o + 4096].iter().zip(&buf[..4096]).any(|(&a, &b)| a & b != b);
                if st.erase {
                    self.d[o..o + 4096].fill(0xFF);
                    *self.erases.entry(lba).or_insert(0) += 1;
                } else {
                    self.programs += 1;
                }
                self.writes.push(lba);
                st.phase = 2;
                st.page = 0;
                None
            }
            2 => {
                for p in st.page as usize..st.page as usize + 4 {
                    for i in o + p * 256..o + p * 256 + 256 {
                        self.d[i] &= buf[i - o];
                    }
                }
                st.page += 4;
                if st.page == 16 {
                    st.phase = 3;
                }
                None
            }
            _ => Some(self.d[o..o + 4096] == buf[..4096]),
        }
    }
}

/// Screenshot power-cut test (review 258c833 H1 / L4): the shot driven slice
/// by slice (`poll_on`) on the NOR model, stopped after `k` slices (power
/// cut: the image as it is then, incl. a half-written data block), or with
/// `abort` stopped there by `screenshot::abort_on` (the power-off / reset
/// path). Prints the slice count of an uncut shot, the phase sequence, and
/// the FAT / root pages that are all 0xFF (an erased or half-programmed
/// metadata block; a real FAT / root page is never all 0xFF here).
fn shotcut(a: &[String], img: &mut Img) {
    img.nor_step = true;
    let k: Option<usize> = a.get(3).map(|x| x.parse().unwrap());
    let abort = a.get(4).map(|x| x == "abort").unwrap_or(false);
    assert!(screenshot::start());
    let mut n = 0usize;
    let mut res = None;
    let mut fat_ops = vec![];
    loop {
        if Some(n) == k {
            if abort {
                screenshot::abort_on(img);
                res = screenshot::take_result();
            }
            break;
        }
        let w0 = img.writes.len();
        let r = screenshot::poll_on(img);
        n += 1;
        if img.writes.len() > w0 {
            fat_ops.push((n, img.writes[w0]));
        }
        if let Some(r) = r {
            screenshot::take_result();
            res = Some(r);
            break;
        }
    }
    let b = &img.d;
    let le16 = |o: usize| b[o] as u32 | (b[o + 1] as u32) << 8;
    let fat_start = le16(0x0E) as usize;
    let data = fat_start + b[0x10] as usize * le16(0x16) as usize + le16(0x11) as usize * 32 / 4096;
    let erased: Vec<usize> = (fat_start * 16..data * 16).filter(|&p| b[p * 256..p * 256 + 256].iter().all(|&x| x == 0xFF)).collect();
    let meta: Vec<String> = fat_ops.iter().filter(|&&(_, l)| (l as usize) < data).map(|(n, l)| format!("{n}:{l}")).collect();
    println!("shotcut slices={} result={:?} busy={} erased_meta_pages={:?}", n, res, screenshot::busy(), erased);
    println!("  metadata writes (slice:lba) {:?}", meta);
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn main() {
    let a: Vec<String> = env::args().collect();
    if a[2] == "vdisk" {
        vdisk_dump(&a);
        return;
    }
    let mut img = Img::new(fs::read(&a[1]).unwrap());
    if a[2] == "csv" {
        csv(&a, &mut img);
        fs::write(&a[1], &img.d).unwrap();
        return;
    }
    if a[2] == "trigger" {
        let t = fatren::make_trigger(&mut img);
        println!("trigger={:?}\n  changed blocks: {:?}", t, img.writes);
        fs::write(&a[1], &img.d).unwrap();
        return;
    }
    if a[2] == "backup" {
        let data: Vec<Vec<u8>> = calbackup::FILES[..calbackup::SAFE]
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
        let r = calbackup::safe_copy(&mut img, &srcs);
        println!(
            "backup={:?} written=0x{:03x} conflict=0x{:03x} differs=0x{:03x}",
            r.status, r.written, r.conflict, r.differs
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
    if a[2] == "shotcut" {
        shotcut(&a, &mut img);
        fs::write(&a[1], &img.d).unwrap();
        return;
    }
    if a[2] == "shotmounted" {
        // M2: a host has the disk mounted -> start() refuses, nothing written.
        msc::HOST_MOUNTED.store(true, std::sync::atomic::Ordering::Relaxed);
        println!("start={} busy={} changed blocks: {:?}", screenshot::start(), screenshot::busy(), img.writes);
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

/// Synthetic reading i: value over 1e-9 .. 1e4 magnitudes, flags pattern (run.sh
/// csvok() recomputes both).
fn row(i: u32) -> (f32, u8) {
    const P: [f32; 13] = [1e-9, 1e-8, 1e-7, 1e-6, 1e-5, 1e-4, 1e-3, 1e-2, 1e-1, 1e0, 1e1, 1e2, 1e3];
    let m = ((i * 7919) % 20001) as f32 - 10000.0;
    let mut f = 0u8;
    if i % 97 == 3 {
        f |= csvlog::F_OL;
    }
    if i % 5 == 0 {
        f |= csvlog::F_REL;
    }
    if i % 7 == 0 {
        f |= csvlog::F_HOLD;
    }
    if i % 11 == 0 {
        f |= csvlog::F_HI;
    }
    if i % 13 == 1 {
        f |= csvlog::F_LO;
    }
    (m * P[(i % 13) as usize], f)
}

/// Uptime of row i (ms): the recording starts at T0, a row every 100 ms.
const T0: u64 = 5000;
const POLLS_PER_ROW: u32 = 4;

fn csv(a: &[String], img: &mut Img) {
    use std::sync::atomic::Ordering::Relaxed;
    let mc = &screenshot::MEDIUM_CHANGED;
    match a[3].as_str() {
        "cat" => {
            let mut b = [0u8; 6144];
            let n = csvlog::catalog_scpi_on(img, &mut b).expect("cat");
            println!("cat={}", String::from_utf8_lossy(&b[..n]));
            return;
        }
        "list" | "del" => {
            if a[3] == "del" {
                let n = csvlog::parse_name(a[4].as_bytes()).expect("name");
                // del <NAME> mounted: a PC has the disk mounted (refused).
                let m = a.get(5).is_some_and(|x| x == "mounted");
                msc::HOST_MOUNTED.store(m, Relaxed);
                println!("del={:?}", csvlog::delete_on(img, &n));
                msc::HOST_MOUNTED.store(false, Relaxed);
            }
            let mut out = [csvlog::FileInfo { name: [0; 11], size: 0, kind: csvlog::Kind::Csv }; 64];
            let c = csvlog::list_on(img, &mut out).expect("list");
            println!("list n={} total={} free={} disk={}", c.n, c.total, c.free_bytes, c.disk_bytes);
            for f in &out[..c.n] {
                let mut b = [0u8; 12];
                println!("  {} {} {:?}", csvlog::name_str(&f.name, &mut b), f.size, f.kind);
            }
            println!("  changed blocks: {:?}", img.writes);
            return;
        }
        "busydel" => {
            assert!(csvlog::start("DC V", "V", "AUTO", T0));
            for k in 0..20 {
                csvlog::poll_on(img, T0 + k);
            }
            let st = csvlog::status();
            let mut b = [0u8; 12];
            let name = calbackup::numbered_name(b"LOG", st.file.expect("file"), b"CSV");
            println!("busydel {} -> {:?}", csvlog::name_str(&name, &mut b), csvlog::delete_on(img, &name));
            csvlog::stop();
            for k in 0..20 {
                csvlog::poll_on(img, T0 + 100 + k);
            }
            println!("then del -> {:?}", csvlog::delete_on(img, &name));
            return;
        }
        _ => {}
    }
    assert_eq!(a[3], "rec");
    let n: u32 = a[4].parse().unwrap();
    let mut shots: Vec<u32> = vec![];
    let mut host: Option<(u32, u32)> = None;
    // mount <row> <rows>: a PC has the disk mounted (msc::host_mounted) for
    // that many rows: no commit meanwhile (review 3443cea H1).
    // mountwin <row> <rows>: the same window, not mounted (the baseline: the
    // writes a commit makes there).
    let mut mount: Option<(u32, u32)> = None;
    let mut mounted = true;
    // shotw <row>: from that row on, start a screenshot the moment a commit
    // is in flight (the shot must wait before its FAT scan).
    let mut shotw: Option<u32> = None;
    let mut shot_waits = 0u32;
    // del <row> <name>: gallery delete during the recording (retried while busy).
    let mut del: Option<(u32, [u8; 11])> = None;
    let mut k = 5;
    while k < a.len() {
        match a[k].as_str() {
            "cut" => {
                img.cut = Some(a[k + 1].parse().unwrap());
                k += 2;
            }
            "half" => {
                img.half = true;
                k += 1;
            }
            "shot" => {
                shots.push(a[k + 1].parse().unwrap());
                k += 2;
            }
            "del" => {
                del = Some((a[k + 1].parse().unwrap(), csvlog::parse_name(a[k + 2].as_bytes()).expect("name")));
                k += 3;
            }
            "shotw" => {
                shotw = Some(a[k + 1].parse().unwrap());
                k += 2;
            }
            "host" => {
                host = Some((a[k + 1].parse().unwrap(), a[k + 2].parse().unwrap()));
                k += 3;
            }
            "mount" | "mountwin" => {
                mount = Some((a[k + 1].parse().unwrap(), a[k + 2].parse().unwrap()));
                mounted = a[k] == "mount";
                k += 3;
            }
            x => panic!("{x}"),
        }
    }
    mc.store(false, Relaxed);
    assert!(csvlog::start("DC V", "\u{3a9}", "AUTO 6 V", T0));
    assert!(!csvlog::start("DC V", "V", "AUTO", T0), "second start must be refused");
    let mut shot_res = vec![];
    let mut host_writes = 0usize;
    let mut mount_writes = 0usize;
    let mut lock_ok = true;
    let mut i = 0u32;
    let mut extra = 0u32;
    loop {
        let now = T0 + 100 * (i as u64 + 1);
        if i < n {
            if shots.contains(&i) {
                println!("  shot start at row {i}: {}", screenshot::start());
            }
            let h = host.is_some_and(|(s, l)| i >= s && i < s + l);
            msc::HOST_WRITING.store(h, Relaxed);
            let m = mount.is_some_and(|(s, l)| i >= s && i < s + l);
            msc::HOST_MOUNTED.store(m && mounted, Relaxed);
            if let Some((r, name)) = del {
                if i >= r {
                    let res = csvlog::delete_on(img, &name);
                    if res != Err(csvlog::GErr::Busy) {
                        println!("  gallery delete at row {i}: {res:?}");
                        del = None;
                    }
                }
            }
            let (v, f) = row(i);
            csvlog::push(now, v, f);
            let w0 = img.writes.len();
            for _ in 0..POLLS_PER_ROW {
                // One writer at a time: a commit never starts while a
                // screenshot is in flight (the shot waits for a running one).
                let (was, sb) = (csvlog::writing(), screenshot::busy());
                csvlog::poll_on(img, now);
                if !was && csvlog::writing() && sb {
                    lock_ok = false;
                }
                if shotw.is_some_and(|r| i >= r) && csvlog::writing() && !screenshot::busy() {
                    shotw = None;
                    println!("  shot start at row {i} during a commit: {}", screenshot::start());
                }
                if screenshot::busy() && msc::LOG_FLUSHING.load(Relaxed) {
                    shot_waits += 1;
                }
                if let Some(r) = screenshot::poll_on(img) {
                    shot_res.push(r);
                }
            }
            if h {
                host_writes += img.writes.len() - w0;
            }
            if m {
                mount_writes += img.writes.len() - w0;
            }
            i += 1;
            if i == n {
                msc::HOST_WRITING.store(false, Relaxed);
                msc::HOST_MOUNTED.store(false, Relaxed);
                csvlog::stop();
                assert!(csvlog::active() || img.dead || csvlog::status().state == csvlog::State::Full);
            }
        } else {
            csvlog::poll_on(img, now);
            if let Some(r) = screenshot::poll_on(img) {
                shot_res.push(r);
            }
            extra += 1;
            if (!csvlog::active() && !screenshot::busy()) || extra > 1_000_000 {
                break;
            }
        }
        if img.dead && i >= n {
            break;
        }
    }
    let st = csvlog::status();
    let res = csvlog::take_result();
    let mut sb = [0u8; 64];
    let sn = csvlog::status_scpi(&mut sb);
    println!("  LOG:CSV? -> {}", String::from_utf8_lossy(&sb[..sn]));
    println!(
        "csv={:?} file={:?} rows={} bytes={} committed={} dropped={} medium_changed={} result={}",
        st.state,
        st.file,
        st.rows,
        st.bytes,
        st.committed,
        st.dropped,
        mc.load(Relaxed),
        res.is_some()
    );
    println!(
        "  shots={:?} shot_waits={} host_writes={} mount_writes={} lock_ok={} session_flag={}",
        shot_res,
        shot_waits,
        host_writes,
        mount_writes,
        lock_ok,
        msc::LOG_SESSION.load(Relaxed)
    );
    // Geometry: classify erases.
    let b = &img.d;
    let le16 = |o: usize| b[o] as u32 | (b[o + 1] as u32) << 8;
    let fat_start = le16(0x0E);
    let root = fat_start + b[0x10] as u32 * le16(0x16);
    let data = root + le16(0x11) * 32 / 4096;
    let (mut fat_e, mut root_e, mut data_e, mut data_max) = (0, 0, 0, 0);
    for (&l, &c) in &img.erases {
        if l < root {
            fat_e += c;
        } else if l < data {
            root_e += c;
        } else {
            data_e += c;
            data_max = data_max.max(c);
        }
    }
    println!(
        "  writes={} programs={} erases: fat={} root={} data={} data_max_per_block={} cut={:?} cut_kind={}",
        img.writes.len(),
        img.programs,
        fat_e,
        root_e,
        data_e,
        data_max,
        img.cut,
        img.cut_kind
    );
}
