//! Entry after the boot decision: RAM init, then recovery or the normal opendm40
//! main with the board bring-up the stock firmware used to do.

use crate::boot::{self, BootInfo, Mode};
use crate::mmio;

extern "C" {
    static mut __bss_start: u32;
    static mut __bss_end: u32;
    static mut __sram_bss_start: u32;
    static mut __sram_bss_end: u32;
    static mut __sdata: u32;
    static mut __edata: u32;
    static __sidata: u32;
    static __VECTORS: u32;
    static _stack_top: u32;
}

const SCB_VTOR: u32 = 0xE000_ED08;
/// Stack paint word (review 2026-10-13 B1: high-water mark in DBG:BOOT?).
const STACK_PAINT: u32 = 0x5354_4B21; // "!KTS"
/// Not painted below the SP at paint time (init_ram's own frame + margin).
const PAINT_GAP: u32 = 256;

/// Copy .data, zero .bss, VTOR = our table.
unsafe fn init_ram() {
    let mut src = core::ptr::addr_of!(__sidata);
    let mut dst = core::ptr::addr_of_mut!(__sdata);
    let dend = core::ptr::addr_of_mut!(__edata);
    while dst < dend {
        dst.write_volatile(src.read_volatile());
        dst = dst.add(1);
        src = src.add(1);
    }
    let mut p = core::ptr::addr_of_mut!(__bss_start);
    let end = core::ptr::addr_of_mut!(__bss_end);
    while p < end {
        p.write_volatile(0);
        p = p.add(1);
    }
    zero_sram_bss();
    // Paint the free stack (__bss_end .. SP - PAINT_GAP) for the high-water mark.
    let sp: u32;
    core::arch::asm!("mov {}, sp", out(reg) sp);
    let mut q = core::ptr::addr_of_mut!(__bss_end);
    while (q as u32) < sp - PAINT_GAP {
        q.write_volatile(STACK_PAINT);
        q = q.add(1);
    }
    mmio::w32(SCB_VTOR, core::ptr::addr_of!(__VECTORS) as u32);
}

/// Zero .sram_bss (all-zero statics placed in SRAM to keep TCM for the
/// stack; the linker scripts' NOLOAD section).
unsafe fn zero_sram_bss() {
    let mut p = core::ptr::addr_of_mut!(__sram_bss_start);
    let end = core::ptr::addr_of_mut!(__sram_bss_end);
    while p < end {
        p.write_volatile(0);
        p = p.add(1);
    }
}

/// (deepest stack use so far, stack size) in bytes: the first word above
/// __bss_end that is no longer the paint. Stack = __bss_end .. _stack_top
/// (TCM); used == size means it reached .bss (overflow).
pub fn stack_use() -> (u32, u32) {
    let lo = core::ptr::addr_of!(__bss_end) as u32;
    let top = core::ptr::addr_of!(_stack_top) as u32;
    let mut a = lo;
    while a < top && unsafe { (a as *const u32).read_volatile() } == STACK_PAINT {
        a += 4;
    }
    (top - a, top - lo)
}

/// Extra DBG:BOOT? fields, appended by boot::put_info (one call at its end):
/// `,STAGED,<n>,CALBK,<status>,<written>,<conflict>,<differs>,STACK,<used>,<size>,CALMS,<ms>,SPICLK,<div>,<check>`.
/// STAGED: fatren's dm40*.bin left because their header is not the marker.
/// CALBK = the write-once calibration safety copy on the SPI disk
/// (calbackup.rs): status NOTRUN NOMEDIUM BADBPB SAFECOPY_PRESENT (all three
/// files there, nothing written) SAFECOPY_WRITTEN (the missing ones written)
/// NOSPACE ERROR; written / conflict / differs: decimal bit masks, bit i =
/// file i of calbackup::FILES (0 CALIB, 1 BOOTDATA, 2 UID); conflict = the
/// name is a directory / label; differs = a present copy no longer equals the
/// flash (kept: write-once); used/size: stack high-water mark / stack bytes;
/// CALMS: duration of calbackup::boot in ms (200 MHz cycles; 12.5x low at
/// 16 MHz); SPICLK:
/// SPI-flash divider in use (4 / 16) and the boot self-check's verdict
/// (spiflash::ClockCheck: OK FORCED NOCHIP NOSIG DIFF1 DIFF2 NOTRUN);
/// then FATDIR (fatdir.rs), CALREC (the calibration record's CRC, dmm.rs
/// `load_record`) and SETTINGS (`put_settings`).
pub fn put_extra(s: &mut crate::scpi::Scpi) {
    use crate::calbackup::Status as C;
    let r = crate::calbackup::result();
    let (used, size) = stack_use();
    let vals = [crate::fatren::result().staged as u32, 0, r.written as u32, r.conflict as u32, r.differs as u32, 0, used, size, 0, r.ms];
    for (i, v) in vals.iter().enumerate() {
        if i == 0 {
            s.put_bytes(b",STAGED,");
        }
        if i == 1 {
            s.put_bytes(match r.status {
                C::NotRun => b"CALBK,NOTRUN" as &[u8],
                C::NoMedium => b"CALBK,NOMEDIUM",
                C::BadBpb => b"CALBK,BADBPB",
                C::Present => b"CALBK,SAFECOPY_PRESENT",
                C::Written => b"CALBK,SAFECOPY_WRITTEN",
                C::NoSpace => b"CALBK,NOSPACE",
                C::Error => b"CALBK,ERROR",
            });
        } else if i == 5 {
            s.put_bytes(b"STACK");
        } else if i == 8 {
            s.put_bytes(b"CALMS");
        } else {
            let mut t = [0u8; 10];
            let mut n = t.len();
            let mut v = *v;
            loop {
                n -= 1;
                t[n] = b'0' + (v % 10) as u8;
                v /= 10;
                if v == 0 {
                    break;
                }
            }
            s.put_bytes(&t[n..]);
        }
        if i + 1 < vals.len() {
            s.put_bytes(b",");
        }
    }
    // SPI-flash clock: divider in use and the boot self-check's verdict
    // (spiflash::clock_check: OK = /4 verified, else /16 and why).
    let (div, why) = crate::spiflash::clock();
    s.put_bytes(if div == 4 { b",SPICLK,4," } else { b",SPICLK,16," });
    s.put_bytes(why.name());
    // FATDIR,<ok>,<created>,<failed>: fatdir::boot (LOGS / SHOTS / SCOPE on
    // the SPI disk), bit i = fatdir::DIRS[i].
    let f = crate::fatdir::boot_result();
    s.put_bytes(b",FATDIR,");
    for (i, v) in [f.ok, f.created, f.failed].iter().enumerate() {
        s.put_bytes(&[b'0' + v / 10 % 10, b'0' + v % 10][(*v < 10) as usize..]);
        if i < 2 {
            s.put_bytes(b",");
        }
    }
    // CALREC,<OK|BADCRC>: the calibration record's CRC at start-up
    // (dmm.rs load_record; BADCRC = every calibration value at its default).
    s.put_bytes(if crate::dmm::rec_crc_ok_at_boot() { b",CALREC,OK" } else { b",CALREC,BADCRC" });
    put_settings(s);
}

/// `,SETTINGS,<from>,<slot>,<seq>,SETFILE,<state>,SAVE,<status>,<saves>,<seq>`
/// (setfile.rs): where this boot's settings came from (BKP = the backup
/// SRAM, FILE = `_SETTING.BIN` slot <slot>, DEFAULTS; <seq> its sequence
/// number, BKP: the one the backup SRAM continues, 0 = none), the file at
/// boot (PRESENT CREATED NOFILE ERROR NOTRUN), the last save (NONE OK
/// DEFERRED BADRECORD ERROR NOFILE), saves this boot and the newest slot's
/// sequence.
fn put_settings(s: &mut crate::scpi::Scpi) {
    use crate::setfile::{FileState as F, From, Save};
    let (from, slot, seq, file, save, saves, top) = crate::setfile::info();
    s.put_bytes(match from {
        From::Bkp => b",SETTINGS,BKP,-," as &[u8],
        From::File => b",SETTINGS,FILE,",
        From::Defaults => b",SETTINGS,DEFAULTS,-,",
        From::NotRun => b",SETTINGS,NOTRUN,-,",
    });
    if from == From::File {
        put_dec(s, slot as u32);
        s.put_bytes(b",");
    }
    put_dec(s, seq);
    s.put_bytes(match file {
        F::Present => b",SETFILE,PRESENT,SAVE," as &[u8],
        F::Created => b",SETFILE,CREATED,SAVE,",
        F::NoFile => b",SETFILE,NOFILE,SAVE,",
        F::Error => b",SETFILE,ERROR,SAVE,",
        F::NotRun => b",SETFILE,NOTRUN,SAVE,",
    });
    s.put_bytes(match save {
        Save::None => b"NONE," as &[u8],
        Save::Ok => b"OK,",
        Save::Deferred => b"DEFERRED,",
        Save::BadRecord => b"BADRECORD,",
        Save::Error => b"ERROR,",
        Save::NoFile => b"NOFILE,",
    });
    put_dec(s, saves);
    s.put_bytes(b",");
    put_dec(s, top);
}

fn put_dec(s: &mut crate::scpi::Scpi, mut v: u32) {
    let mut t = [0u8; 10];
    let mut n = t.len();
    loop {
        n -= 1;
        t[n] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    s.put_bytes(&t[n..]);
}

/// Board bring-up + fatren + calibration backup, then opendm40 main (normal mode).
fn normal(_info: &BootInfo, dry: bool) -> ! {
    pre_main(dry);
    crate::main()
}

/// Everything before main(), in its own never-inlined frame so the SPI-disk
/// block buffers (fatren ~9 KiB, calbackup ~2 KiB + scratch, each in its own
/// never-inlined function too) are popped before main()'s 26 KiB frame is
/// pushed (review 2026-10-13 B1).
#[inline(never)]
fn pre_main(dry: bool) {
    let before = crate::board::init();
    boot::set_board(before);
    boot::feed();
    // fatren (rename the INSTALLED dm40*.bin to _DM40PRV.BIN): only the file
    // whose update header equals the 0x08008000 marker the loader wrote (none
    // if the marker is not valid); any other dm40*.bin is staged and stays.
    // Before USB exists. No reset-cause gate (review 2026-10-13 H1).
    let marker = if boot::marker_valid() {
        let mut m = [0u8; 12];
        for (d, w) in m.chunks_mut(4).zip(boot::marker()) {
            d.copy_from_slice(&w.to_le_bytes());
        }
        Some(m)
    } else {
        None
    };
    {
        let mut disk = crate::spiflash::SpiFlash::new();
        crate::fatren::run(&mut disk, dry, marker);
    }
    boot::feed();
    // Write-once calibration safety copy (CALIB.BIN, BOOTDATA.BIN, UID.BIN,
    // only the missing ones) on the same volume, after fatren, still before
    // USB. Not affected by fatren-dry-run: it only creates its own files; it never
    // touches dm40*.bin or _DM40PRV.BIN names.
    crate::calbackup::boot();
    boot::feed();
    // LOGS/ SHOTS/ SCOPE/ for our files (fatdir.rs): created on the first
    // boot (~1 s), nothing written afterwards.
    {
        let mut disk = crate::spiflash::SpiFlash::new();
        crate::fatdir::boot(&mut disk);
    }
    boot::feed();
    // The settings' copy (setfile.rs `_SETTING.BIN`): made on the first
    // boot (2 blocks), then only its slots are read here.
    {
        let mut disk = crate::spiflash::SpiFlash::new();
        crate::setfile::boot(&mut disk);
    }
    boot::feed();
    // The INFO/ files' slow-bank bytes mirrored on the SPI disk (vdisk.rs
    // `mirror_boot`: compared every boot, rewritten only when they differ),
    // served from there while an AD2 stream runs.
    #[cfg(feature = "vdisk")]
    crate::vdisk::mirror_boot();
    boot::feed();
    // The generated USB volume (vdisk.rs): built-in tables + CRC-32s of the
    // INFO sources (~0.1 s), before USB.
    #[cfg(feature = "vdisk")]
    crate::vdisk::init(crate::vdisk::mem_read, unsafe {
        core::slice::from_raw_parts(crate::calbackup::FW_BASE as *const u8, crate::calbackup::FW_MAX_BODY)
    });
    boot::feed();
    // Its LOGS / SCOPE / SHOTS files from the SPI disk, so the host's first
    // look is complete (a DLG's CSV size: ~10 ms per 100 KB of logs).
    #[cfg(feature = "vdisk")]
    crate::vdisk::boot_refresh();
    boot::feed();
}

/// Called by the shim (src/shim.rs) with the decision.
pub unsafe fn start(info: BootInfo) -> ! {
    init_ram();
    // The serial number (USB iSerial, SCSI VPD 0x80, *IDN?) for both modes.
    crate::uid::init();
    boot::set_info(info);
    match info.mode {
        Mode::Recovery => crate::recovery::run(),
        Mode::Normal => normal(&info, cfg!(feature = "fatren-dry-run")),
    }
}
