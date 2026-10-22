//! Host test for src/fatren.rs, src/calbackup.rs (+ src/vdisk.rs) against a real FAT16 image
//! (run.sh builds one with mkfs.fat/mtools in the DM40 geometry: 4096-byte
//! sectors, 2 FATs, 512 root entries, 16 MiB).
//!   main <image> dry|live [marker-hex]  fatren::run (marker = the 12-byte
//!                                       record at 0x08008000; none = invalid)
//!   main <image> trigger                fatren::make_trigger
//!   main <image> backup <dir>           calbackup::safe_copy (write-once safety copy)
//!                                       of calbackup::FILES[..SAFE] read from
//!                                       <dir>/CALIB.BIN, BOOTDATA.BIN, UID.BIN
//!   main <out> vdisk <flash> <uid> <otp> <optbytes> [step]...
//!                                       vdisk.rs (the read-only USB volume) over the
//!                                       1 MiB <flash> image (0x08000000..) and the
//!                                       system-memory files: every sector read through
//!                                       read_block into the sparse <out>; write_block /
//!                                       out-of-range reads refused; steps (spi <img>,
//!                                       locked, free, reset, forget <NAME>) each
//!                                       followed by a dump to <out>.<i> (vdisk_dump)
//!   main <image> fatdir boot|ensure <NAME>|rm <NAME> [cut <k>]
//!                                       fatdir.rs (fatdir_cmd)
//!   main <image> upload <file> [crc <hex>] [order] [chunk <n>] [cut <k>]
//!                                       upload.rs as main.rs drives it (upload_cmd)
//!   main <image> shot                   screenshot::take_on with the synthetic screen
//!                                       of the `lcd` stub below (pattern()).
//!   main <image> shotcut [k [abort]]    the same slice by slice on a NOR model of
//!                                       write_step, cut after k slices (or stopped
//!                                       there by screenshot::abort_on); see shotcut().
//!   main <image> csv rec <rows> [cut <k> [half]] [shot <row>]... [host <row> <rows>] [mount|mountwin <row> <rows>]
//!                                       csvlog recording of <rows> synthetic rows
//!                                       (row(i) below), polled pass by pass;
//!                                       cut = power cut before the k-th changing
//!                                       block write (half: a program-only write is
//!                                       cut half way, 8 of 16 pages); shot = start
//!                                       a screenshot at that row (interleaved);
//!                                       host = HOST_WRITING set for that many rows;
//!                                       mount = msc::host_mounted() for that many rows
//!                                       (no effect since the PC cannot mount the SPI
//!                                       disk; kept for the knob); mount_writes = block
//!                                       writes in that window.
//!   main <image> csv list | del <NAME> | busydel
//!                                       gallery list / delete; busydel = delete the
//!                                       file being recorded (must be refused).
//! Live runs rewrite the image.
use std::{env, fs};

#[allow(dead_code)]
mod msc {
    /// Same trait as fw-rs src/msc.rs (kept in sync by hand; 8 methods,
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
        fn media_change_immediate(&self) -> bool {
            false
        }
        fn product(&self) -> &'static [u8; 16] {
            b"DM40 disk       "
        }
        fn gates_writes(&self) -> bool {
            false
        }
        fn write_error(&self) -> u8 {
            0
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

#[path = "../../src/scope_csv.rs"]
mod scope_csv;

#[path = "../../src/dlog.rs"]
mod dlog;

#[path = "../../src/csvlog.rs"]
#[allow(dead_code)]
mod csvlog;

#[path = "../../src/fatdir.rs"]
mod fatdir;
#[path = "../../src/upload.rs"]
#[allow(dead_code)]
mod upload;
#[path = "../../src/fatren.rs"]
#[allow(dead_code)]
mod fatren;

#[path = "../../src/calbackup.rs"]
mod calbackup;

#[path = "../../src/vdisk.rs"]
mod vdisk;
#[path = "../../src/vwrite.rs"]
mod vwrite;

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

/// The SPI-disk image behind dlog's reader (vdisk ... spi <img>), replaced
/// between the refresh steps.
static SPI: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

fn spi_read(addr: u32, buf: &mut [u8]) -> bool {
    let d = SPI.lock().unwrap();
    let o = addr as usize;
    if o + buf.len() > d.len() {
        return false;
    }
    buf.copy_from_slice(&d[o..o + buf.len()]);
    true
}

/// The whole volume through read_block into the sparse file `out` (only
/// non-zero sectors written); every written sector read a second time and
/// compared.
fn vdisk_write(d: &mut vdisk::VDisk, out: &str) {
    use msc::BlockDevice;
    use std::io::{Seek, SeekFrom, Write};
    let n = d.block_count();
    let bs = d.block_size() as usize;
    let mut f = fs::File::create(out).unwrap();
    f.set_len(n as u64 * bs as u64).unwrap();
    let mut b = vec![0u8; bs];
    let mut c = vec![0u8; bs];
    let mut nz = 0u32;
    for lba in 0..n {
        assert!(d.read_block(lba, &mut b), "read {lba}");
        if b.iter().any(|&x| x != 0) {
            assert!(d.read_block(lba, &mut c) && b == c, "sector {lba} differs between two reads");
            f.seek(SeekFrom::Start(lba as u64 * bs as u64)).unwrap();
            f.write_all(&b).unwrap();
            nz += 1;
        }
    }
    println!("  dump {out}: {n} sectors, {nz} non-zero");
}

/// `main <out> vdisk <flash> <uid> <otp> <optbytes> [step]...`: vdisk.rs
/// over the 1 MiB <flash> image (0x08000000..) and the system-memory files;
/// the volume dumped to <out> after init, and to <out>.<i> after step i:
///   spi <img>        the SPI disk is now <img>; refresh
///   locked / free    a host holds / released the medium lock (msc stub);
///                    refresh
///   reset            USB bus reset (vdisk::on_bus_reset); refresh
///   forget <NAME>    vdisk::forget (8.3 SPI name, e.g. LOG001.DLG)
fn vdisk_dump(a: &[String]) {
    use msc::BlockDevice;
    use std::sync::atomic::Ordering::Relaxed;
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
    dlog::set_disk(spi_read);
    let mut d = vdisk::VDisk;
    println!("present_before_init={}", d.present());
    vdisk::init(mem_read, body);
    println!("media_changed_after_init={}", d.take_media_changed());
    let mut b = vec![0u8; 512];
    let past = d.read_block(d.block_count(), &mut b);
    let wr = d.write_block(0, &b);
    println!("vdisk sectors={} bs={} read_only={} write_block={} read_past_end={}", d.block_count(), d.block_size(), d.read_only(), wr, past);
    vdisk_write(&mut d, &a[1]);
    let (mut k, mut step) = (7, 0);
    while k < a.len() {
        step += 1;
        match a[k].as_str() {
            "spi" => {
                *SPI.lock().unwrap() = fs::read(&a[k + 1]).unwrap();
                k += 2;
            }
            "locked" | "free" => {
                msc::HOST_MOUNTED.store(a[k] == "locked", Relaxed);
                k += 1;
            }
            "reset" => {
                vdisk::on_bus_reset();
                k += 1;
            }
            "forget" => {
                let n = csvlog::parse_name(a[k + 1].as_bytes()).unwrap();
                vdisk::forget(&n);
                println!("step {step}: forget {} media_changed={}", a[k + 1], d.take_media_changed());
                vdisk_write(&mut d, &format!("{}.{step}", a[1]));
                k += 2;
                continue;
            }
            x => panic!("{x}"),
        }
        let n = vdisk::refresh(None);
        let (_, hw, out) = vdisk::stats();
        println!("step {step}: {} files={n} next_cluster={hw} left_out={out} media_changed={} stale={}", a[k - 1], d.take_media_changed(), dlog::stale());
        let n2 = vdisk::refresh(None);
        println!("  again: files={n2} media_changed={}", d.take_media_changed());
        vdisk_write(&mut d, &format!("{}.{step}", a[1]));
    }
}

/// The SPI disk of `vserve` as a block device (4 KiB blocks) over `SPI`
/// (the same bytes dlog's reader sees): vwrite.rs stages into it,
/// csvlog::delete_on / upload::commit_on write it. `cut`: a power cut
/// before changing write number `cut` (nothing written from then on).
struct SpiDev {
    writes: usize,
    cut: Option<usize>,
    dead: bool,
}

impl msc::BlockDevice for SpiDev {
    fn present(&self) -> bool {
        true
    }
    fn block_size(&self) -> u32 {
        4096
    }
    fn block_count(&self) -> u32 {
        (SPI.lock().unwrap().len() / 4096) as u32
    }
    fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> bool {
        let d = SPI.lock().unwrap();
        let o = lba as usize * 4096;
        buf[..4096].copy_from_slice(&d[o..o + 4096]);
        true
    }
    fn write_block(&mut self, lba: u32, buf: &[u8]) -> bool {
        let mut d = SPI.lock().unwrap();
        let o = lba as usize * 4096;
        if self.dead {
            return false;
        }
        if d[o..o + 4096] != buf[..4096] {
            if self.cut == Some(self.writes) {
                self.dead = true;
                return false;
            }
            self.writes += 1;
        }
        d[o..o + 4096].copy_from_slice(&buf[..4096]);
        true
    }
}

/// `main <spi.img> vserve <flash> <uid> <otp> <optbytes> [cut <k>]`: the USB
/// volume (vdisk.rs + vwrite.rs) over the SPI image, served on stdin /
/// stdout for tools/fatren-test/usbip_msc.py (a USB/IP mass-storage device
/// for a real kernel) and vreplay.py (captured write sequences). Requests:
/// one op byte + a u32 LE argument, replies one status byte unless noted:
///   R lba        -> status, 512 bytes (status 0 ok)
///   W lba + 512  -> 0 accepted, 1 + vwrite::last_error() refused
///   T ms         the main loop at time ms: vwrite::poll, the install /
///                dialog state, vdisk::refresh when dlog is stale
///   L 0|1        PREVENT (1) / ALLOW (0) MEDIUM REMOVAL (vdisk::on_unlock)
///   B            USB bus reset (vdisk::on_bus_reset)
///   U            -> 1 if a UNIT ATTENTION is due (one-shot)
///   P            -> 1 if the volume is write-protected (MODE SENSE WP)
///   D            -> the meter's dialog: 0 none, 1 "install?", 2 "not an
///                update file" (vwrite::take_dialog)
///   A 0|1        answer the dialog (No / Yes)
///   I            -> 1 if Yes was answered (install: zero marker + reset)
///   S            save the SPI image to <spi.img>.out
///   V n          dump the volume (sparse) to <spi.img>.vol<n>
///   Q            -> u16 length + text: DBG:VDISK:WR? and STAT? fields
///   G + 11 bytes a delete on the meter (csvlog::delete_on, 8.3 SPI name)
///   X            exit
fn vserve(a: &[String]) {
    use msc::BlockDevice;
    use std::io::{Read, Write};
    use std::sync::atomic::Ordering::Relaxed;
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
    *SPI.lock().unwrap() = fs::read(&a[1]).unwrap();
    dlog::set_disk(spi_read);
    let cut = a.iter().position(|x| x == "cut").map(|i| a[i + 1].parse::<usize>().unwrap());
    let dev: &'static mut SpiDev = Box::leak(Box::new(SpiDev { writes: 0, cut, dead: false }));
    let devp: *mut SpiDev = dev;
    vwrite::set_dev(dev);
    vdisk::init(mem_read, body);
    vdisk::boot_refresh();
    let mut d = vdisk::VDisk;
    let mut inp = std::io::stdin().lock();
    let mut out = std::io::stdout().lock();
    let mut hdr = [0u8; 5];
    let mut sec = vec![0u8; 512];
    let (mut gen, mut gen_t, mut now) = (dlog::gen(), 0u64, 0u64);
    while inp.read_exact(&mut hdr).is_ok() {
        let arg = u32::from_le_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]);
        let mut reply: Vec<u8> = vec![0];
        match hdr[0] {
            b'R' => {
                let ok = d.read_block(arg, &mut sec);
                reply[0] = !ok as u8;
                reply.extend_from_slice(&sec);
            }
            b'W' => {
                inp.read_exact(&mut sec).unwrap();
                msc::HOST_WRITING.store(true, Relaxed);
                reply[0] = if d.write_block(arg, &sec) { 0 } else { 1 + d.write_error() };
                msc::HOST_WRITING.store(false, Relaxed);
            }
            b'T' => {
                now = arg as u64;
                vwrite::poll(now, false);
                // main.rs: refresh once dlog::gen() has not moved for 1 s.
                if dlog::gen() != gen {
                    gen = dlog::gen();
                    gen_t = now;
                } else if dlog::stale() && now.saturating_sub(gen_t) >= 1000 {
                    vdisk::refresh(None);
                }
            }
            b'L' => {
                msc::HOST_MOUNTED.store(arg != 0, Relaxed);
                if arg == 0 {
                    vdisk::on_unlock();
                }
            }
            b'B' => {
                msc::HOST_MOUNTED.store(false, Relaxed);
                vdisk::on_bus_reset();
            }
            b'U' => reply[0] = d.take_media_changed() as u8,
            b'P' => reply[0] = d.read_only() as u8,
            b'D' => reply[0] = match vwrite::take_dialog() {
                None => 0,
                Some(true) => 1,
                Some(false) => 2,
            },
            b'A' => vwrite::answer(arg != 0),
            b'I' => reply[0] = vwrite::take_install() as u8,
            b'S' => fs::write(format!("{}.out", a[1]), &*SPI.lock().unwrap()).unwrap(),
            b'V' => {
                vdisk_write_quiet(&mut d, &format!("{}.vol{arg}", a[1]));
            }
            b'Q' => {
                let n = vwrite::stats();
                let (rec, pairs, st, fz, stg, fw) = vwrite::state();
                let (files, hw, left) = vdisk::stats();
                let cutn = unsafe { (*devp).writes };
                let t = format!(
                    "ok={} refused={} busy={} last_refused={} deleted={} fw_ok={} fw_bad={} rec={rec} pairs={pairs} staged={st} frozen={fz} staging={stg} fw={fw} files={files} hw={hw} left_out={left} spi_writes={cutn} cut_dead={} now={now}",
                    n.ok, n.refused, n.busy, n.last_refused, n.deleted, n.fw_ok, n.fw_bad, unsafe { (*devp).dead }
                );
                reply = (t.len() as u16).to_le_bytes().to_vec();
                reply.extend_from_slice(t.as_bytes());
            }
            b'G' => {
                // A delete on the meter (gallery / MMEM:DEL): 11-byte 8.3 name.
                let mut n = [0u8; 11];
                inp.read_exact(&mut n).unwrap();
                reply[0] = csvlog::delete_on(unsafe { &mut *devp }, &n).is_err() as u8;
            }
            b'Z' => {
                let t = vwrite::debug_text();
                reply = (t.len() as u16).to_le_bytes().to_vec();
                reply.extend_from_slice(t.as_bytes());
            }
            b'X' => break,
            x => panic!("vserve: op {x}"),
        }
        out.write_all(&reply).unwrap();
        out.flush().unwrap();
    }
}

/// `vdisk_write` without the log line (vserve's stdout is the protocol).
fn vdisk_write_quiet(d: &mut vdisk::VDisk, out: &str) {
    use msc::BlockDevice;
    use std::io::{Seek, SeekFrom, Write};
    let n = d.block_count();
    let mut f = fs::File::create(out).unwrap();
    f.set_len(n as u64 * 512).unwrap();
    let mut b = vec![0u8; 512];
    for lba in 0..n {
        assert!(d.read_block(lba, &mut b), "read {lba}");
        if b.iter().any(|&x| x != 0) {
            f.seek(SeekFrom::Start(lba as u64 * 512)).unwrap();
            f.write_all(&b).unwrap();
        }
    }
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
    let dirs = dir_blocks(b);
    let pages: Vec<usize> = (fat_start * 16..data * 16).chain(dirs.iter().flat_map(|&l| l as usize * 16..l as usize * 16 + 16)).collect();
    let erased: Vec<usize> = pages.into_iter().filter(|&p| b[p * 256..p * 256 + 256].iter().all(|&x| x == 0xFF)).collect();
    let meta: Vec<String> = fat_ops.iter().filter(|&&(_, l)| (l as usize) < data || dirs.contains(&l)).map(|(n, l)| format!("{n}:{l}")).collect();
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
    if a[2] == "vserve" {
        vserve(&a);
        return;
    }
    let mut img = Img::new(fs::read(&a[1]).unwrap());
    if a[2] == "csv" {
        csv(&a, &mut img);
        fs::write(&a[1], &img.d).unwrap();
        return;
    }
    if a[2] == "upload" {
        upload_cmd(&a, &mut img);
        fs::write(&a[1], &img.d).unwrap();
        return;
    }
    if a[2] == "fatdir" {
        fatdir_cmd(&a, &mut img);
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
    if a[2] == "scopecsv" {
        // A synthetic scope record (sine, 10-bit codes) of n samples as
        // SCOPnnn.CSV, driven slice by slice; a screenshot request while it
        // runs is refused.
        let n: usize = a.get(3).map_or(26 * 1024, |v| v.parse().unwrap());
        let rec: &'static mut [u16] = Box::leak(vec![0u16; n].into_boxed_slice());
        for (i, c) in rec.iter_mut().enumerate() {
            *c = (512.0 + 400.0 * (i as f32 * 0.01).sin()) as u16;
        }
        let mut h = scope_csv::Hdr::new();
        h.s("# test record\r\n");
        scope_csv::set(rec, -5.0, 10.0 / 1023.0, 1.0e6, n / 2, &h);
        assert!(screenshot::start_csv());
        let refused = !screenshot::start();
        let mut slices = 0;
        let r = loop {
            screenshot::poll_on(&mut img);
            slices += 1;
            if let Some(r) = screenshot::take_csv_result() {
                break r;
            }
        };
        println!("csv={:?} len={} slices={} shot_refused={} shot_result={:?}", r, scope_csv::len(), slices, refused, screenshot::take_result());
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

/// `main <img> fatdir boot|ensure <NAME>|rm <NAME> [cut <k>]`: fatdir.rs on
/// the image (boot = the native boot step: LOGS, SHOTS, SCOPE ensured);
/// cut = power cut before the k-th changing block write.
fn fatdir_cmd(a: &[String], img: &mut Img) {
    let mut k = 4;
    let name = a.get(4).filter(|_| a[3] != "boot").map(|n| {
        k = 5;
        let mut b = [b' '; 11];
        b[..n.len()].copy_from_slice(n.as_bytes());
        b
    });
    if a.get(k).is_some_and(|x| x == "cut") {
        img.cut = Some(a[k + 1].parse().unwrap());
    }
    let s = calbackup::scratch();
    let mut v = || {
        let mut b = [0u8; 4096];
        msc::BlockDevice::read_block(img, 0, &mut b);
        fatren::parse_bpb(&b, (img.d.len() / 4096) as u32).unwrap()
    };
    match a[3].as_str() {
        "boot" => {
            let r = fatdir::boot(img);
            println!("fatdir boot ok={:#x} created={:#x} failed={:#x}", r.ok, r.created, r.failed);
        }
        "ensure" => {
            let v = v();
            match fatdir::ensure(img, &v, &name.unwrap(), s) {
                Ok(d) => println!("fatdir ensure ok clusters={:?}", &d.dcl[..d.dn as usize]),
                Err(e) => println!("fatdir ensure err={e:?}"),
            }
        }
        "rm" => {
            let v = v();
            println!("fatdir rm {:?}", fatdir::remove_empty(img, &v, &name.unwrap(), s));
        }
        x => panic!("{x}"),
    }
    println!("  writes={} changed blocks: {:?} cut_kind={}", img.writes.len(), img.writes, img.cut_kind);
}

/// `main <img> upload <file> [crc <hex>] [order [keep]] [chunk <n>] [cut <k>]`:
/// upload.rs as main.rs drives it (BEGIN, DATA messages of `chunk` bytes,
/// END?); crc = announce this CRC instead of the file's; order = send the
/// second chunk twice; cut = power cut before the k-th changing write.
fn upload_cmd(a: &[String], img: &mut Img) {
    let f = fs::read(&a[3]).unwrap();
    let mut crc = calbackup::crc32(&f);
    let (mut order, mut chunk, mut keep) = (false, 240usize, false);
    let mut k = 4;
    while k < a.len() {
        match a[k].as_str() {
            "crc" => {
                crc = u32::from_str_radix(&a[k + 1], 16).unwrap();
                k += 2;
            }
            "order" => {
                order = true;
                k += 1;
            }
            // keep: go on after a DATA error (as the host tool streams) and
            // let END? report it.
            "keep" => {
                keep = true;
                k += 1;
            }
            "chunk" => {
                chunk = a[k + 1].parse().unwrap();
                k += 2;
            }
            "cut" => {
                img.cut = Some(a[k + 1].parse().unwrap());
                k += 2;
            }
            x => panic!("{x}"),
        }
    }
    let r = (|| -> Result<(u32, u32), &'static str> {
        upload::begin_on(img, f.len() as u32, crc, 0)?;
        let mut off = 0usize;
        let mut i = 0;
        while off < f.len() {
            let n = chunk.min(f.len() - off);
            let mut m = format!("{off},").into_bytes();
            m.extend_from_slice(&f[off..off + n]);
            let r = upload::data_msg_on(img, &m, i);
            if !keep {
                r?;
            }
            if order && i == 1 {
                let r = upload::data_msg_on(img, &m, i);
                if !keep {
                    r?;
                }
            }
            off += n;
            i += 1;
        }
        upload::end_on(img)
    })();
    println!("upload {:?} busy={} writes={} changed blocks: {:?}", r, upload::busy(), img.writes.len(), img.writes.len());
}

/// Blocks of the root's subdirectories (their clusters; 1 cluster = 1
/// block on this disk): directory metadata like the root region.
fn dir_blocks(b: &[u8]) -> Vec<u32> {
    let le16 = |o: usize| b[o] as u32 | (b[o + 1] as u32) << 8;
    let fat = le16(0x0E) as usize * 4096;
    let root = (le16(0x0E) + b[0x10] as u32 * le16(0x16)) as usize * 4096;
    let ne = le16(0x11) as usize;
    let data = (root / 4096 + ne * 32 / 4096) as u32;
    let mut out = vec![];
    for i in 0..ne {
        let e = &b[root + 32 * i..root + 32 * i + 32];
        if e[0] == 0 {
            break;
        }
        if e[0] == 0xE5 || e[11] & 0x3F == 0x0F || e[11] & 0x10 == 0 {
            continue;
        }
        let mut c = e[26] as u32 | (e[27] as u32) << 8;
        let mut n = 0;
        while (2..0xFFF0).contains(&c) && n < 64 {
            out.push(data + c - 2);
            c = le16(fat + 2 * c as usize);
            n += 1;
        }
    }
    out
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
            let mut out = [csvlog::FileInfo { name: [0; 11], size: 0, kind: csvlog::Kind::Csv, in_dir: false }; 64];
            let c = csvlog::list_on(img, &mut out).expect("list");
            println!("list n={} total={} free={} disk={}", c.n, c.total, c.free_bytes, c.disk_bytes);
            for f in &out[..c.n] {
                let mut b = [0u8; 12];
                println!("  {} {} {:?}", csvlog::name_str(&f.name, &mut b), f.size, f.kind);
            }
            println!("  changed blocks: {:?}", img.writes);
            return;
        }
        "ival" => {
            // ival <interval idx> <duration idx> <readings> [<push until ms>]:
            // interval mode (Record page). Reading i at T0 + 100 i ms, value
            // i, flags: i = 25 OL, 30..39 all OL, 42 HOLD, 43 REL, 44 HI,
            // 45 LO; readings 60..69 left out (an interval with none). Then
            // polls until the recording ends: by its duration, or a stop
            // after the last reading when the duration is 0.
            let (ii, di, n): (u8, u8, u32) = (a[4].parse().unwrap(), a[5].parse().unwrap(), a[6].parse().unwrap());
            csvlog::set_next(ii, di);
            assert_eq!(csvlog::next(), (ii, di));
            assert!(csvlog::start("DC V", "V", "AUTO", T0));
            let mut now = T0;
            for i in 0..n {
                now = T0 + 100 * i as u64;
                if (60..70).contains(&i) {
                    csvlog::poll_on(img, now);
                    continue;
                }
                let mut f = 0u8;
                if i == 25 || (30..40).contains(&i) {
                    f |= csvlog::F_OL;
                }
                f |= match i {
                    42 => csvlog::F_HOLD,
                    43 => csvlog::F_REL,
                    44 => csvlog::F_HI,
                    45 => csvlog::F_LO,
                    _ => 0,
                };
                csvlog::push(now, i as f32, f);
                csvlog::poll_on(img, now);
                if csvlog::status().state != csvlog::State::Recording {
                    println!("  ended by its duration at reading {i}");
                    break;
                }
            }
            if di == 0 {
                csvlog::stop();
            }
            let mut k = 0u64;
            while csvlog::active() && k < 100_000_000 {
                now += 10;
                csvlog::poll_on(img, now);
                k += 1;
            }
            let st = csvlog::status();
            println!("ival={:?} file={:?} rows={} dropped={} end_ms={}", st.state, st.file, st.rows, st.dropped, now - T0);
            match csvlog::free_bytes_on(img) {
                Ok(f) => println!("free={f} est_1s={} est_every={} cap={}", csvlog::est_hours(f, 1000), csvlog::est_hours(f, 100), csvlog::EST_CAP_H),
                Err(e) => println!("free={e:?}"),
            }
            return;
        }
        "busydel" => {
            assert!(csvlog::start("DC V", "V", "AUTO", T0));
            for k in 0..20 {
                csvlog::poll_on(img, T0 + k);
            }
            let st = csvlog::status();
            let mut b = [0u8; 12];
            let name = calbackup::numbered_name(b"LOG", st.file.expect("file"), b"DLG");
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
    let dirs = dir_blocks(b);
    for (&l, &c) in &img.erases {
        if l < root {
            fat_e += c;
        } else if l < data || dirs.contains(&l) {
            root_e += c;
        } else {
            data_e += c;
            data_max = data_max.max(c);
        }
    }
    println!(
        "  writes={} programs={} erases: fat={} dir={} data={} data_max_per_block={} cut={:?} cut_kind={}",
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
