//! Screenshot to the SPI-flash USB disk (docs/user-feedback.md UF-17, stock
//! "PriScr": 3-finger swipe down, gui_v2_6.h "Screenshot").
//!
//! File: the next free `S%03u.BMP` in the root (lowest unused number, as the
//! stock "s%.3d.bmp" / first free number, spiflash_v2_6.h), byte-compatible
//! with the stock files on this unit's disk (dumps/spi_dumps
//! dm40_msc_full_2026-10-13.bin S000..S002.BMP): 480x320, 16 bpp RGB565,
//! BI_BITFIELDS (masks F800/07E0/001F), 66-byte header (14 file + 40 info +
//! 12 masks), positive height = bottom-up rows of 960 bytes (no padding),
//! pixels little-endian, 72 dpi fields 0: 307266 bytes = 76 clusters of 4 KiB.
//! (Not 24-bit: that would be 460854 bytes. The 17 KB S00x.BMP in
//! spi-disk-backup/ are older 4-bpp RLE factory files.)
//!
//! Pixels come straight from the panel: the LCD GRAM is read back one row
//! segment at a time (lcd::read_span, RGB666 -> RGB565) while the FAT writer
//! asks for each 4 KiB block; no frame buffer exists.
//!
//! Disk writes go through calbackup::write_file, i.e. the same ordered path as
//! the calibration backup: free clusters allocated first-fit from FAT 1, data
//! blocks written and read back, the chain linked in every FAT copy, then the
//! root entry, then the whole file re-read through the new chain and compared
//! with a second GRAM read-back. A power loss at any point leaves at worst lost
//! clusters (no entry on half-written or free clusters). Each block write is
//! verified; nothing is written if the boot sector is not the expected FAT16.
//!
//! The call is synchronous (~76 x (erase + program + 2 read-backs) = about
//! 6-9 s): the main loop, so USB (`msc.poll`), the DMM, the scope and drawing,
//! all wait. Consequences:
//! * no host SCSI command (read or write) is served during the write, so the
//!   host cannot interleave its own writes with ours;
//! * the screen is not redrawn until the file is verified, so the GRAM read
//!   for the data and the one for the verify see the same picture;
//! * the watchdog is kicked between blocks (calbackup) and during erases
//!   (spiflash) as long as the main loop feeds it.
//! Host-cache caveat: a host that has the USB disk MOUNTED keeps its own copy
//! of the FAT and root directory and does not see the new file; worse, its
//! next write can put its cached (older) FAT block back over our chain. After
//! a write `MEDIUM_CHANGED` is set so the MSC layer can report UNIT ATTENTION
//! 06/28/00 "medium may have changed" on the next host command, which makes
//! Windows / macOS re-read the volume (Linux drops its block cache but a
//! mounted vfat keeps its in-memory state: unmount / re-plug before writing
//! to the disk from the PC). Safe without caveats when no PC has the disk
//! mounted (charger, power bank, battery).

use core::sync::atomic::{AtomicBool, Ordering};

use crate::calbackup::{self, Source, Status};
use crate::lcd;
use crate::msc::BlockDevice;

/// Set after a screenshot changed the disk; for msc.rs to report UNIT
/// ATTENTION (sense 06/28/00) once on the next command, then clear.
pub static MEDIUM_CHANGED: AtomicBool = AtomicBool::new(false);

const W: u32 = lcd::W as u32;
const H: u32 = lcd::H as u32;
const ROW: u32 = W * 2;
const HDR_LEN: u32 = 66;
const FILE_LEN: u32 = HDR_LEN + ROW * H; // 307266

/// The stock header, byte for byte (S000.BMP at 0xD000 in the disk dump).
const HDR: [u8; HDR_LEN as usize] = [
    b'B', b'M', 0x42, 0xB0, 0x04, 0x00, // file size 307266
    0x00, 0x00, 0x00, 0x00, // reserved
    0x42, 0x00, 0x00, 0x00, // pixel data offset 66
    0x28, 0x00, 0x00, 0x00, // BITMAPINFOHEADER (40)
    0xE0, 0x01, 0x00, 0x00, // width 480
    0x40, 0x01, 0x00, 0x00, // height 320 (bottom-up)
    0x01, 0x00, 0x10, 0x00, // 1 plane, 16 bpp
    0x03, 0x00, 0x00, 0x00, // BI_BITFIELDS
    0x00, 0xB0, 0x04, 0x00, // image size 307200
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // ppm x / y
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // colours used / important
    0x00, 0xF8, 0x00, 0x00, 0xE0, 0x07, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00, // R G B masks
];
const _: () = assert!(FILE_LEN == 307_266);

/// The current screen as a BMP file, generated from the GRAM on demand.
struct Gram;

impl Source for Gram {
    fn size(&self) -> u32 {
        FILE_LEN
    }

    fn fill(&self, off: u32, buf: &mut [u8]) {
        let mut i = 0usize;
        let mut off = off;
        while i < buf.len() && off < HDR_LEN {
            buf[i] = HDR[off as usize];
            i += 1;
            off += 1;
        }
        while i < buf.len() && off < FILE_LEN {
            let rel = off - HDR_LEN;
            let px = rel / 2;
            // File row r is screen row H - 1 - r (bottom-up).
            let (x, y) = (px % W, H - 1 - px / W);
            let whole = (buf.len() - i) / 2;
            if rel % 2 == 1 || whole == 0 {
                // A block edge inside a pixel (not with 4 KiB blocks: kept
                // for any caller).
                let mut c = 0u16;
                lcd::read_span(x as u16, y as u16, 1, |v| c = v);
                buf[i] = c.to_le_bytes()[(rel % 2) as usize];
                i += 1;
                off += 1;
                continue;
            }
            let n = (W - x).min(whole as u32);
            lcd::read_span(x as u16, y as u16, n as u16, |v| {
                buf[i..i + 2].copy_from_slice(&v.to_le_bytes());
                i += 2;
            });
            off += 2 * n;
        }
    }
}

/// Result of `take`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shot {
    /// Written and verified as S%03u.BMP.
    Saved(u16),
    /// No disk / not the expected FAT16 volume / read error.
    NoDisk,
    /// Root directory or disk full, or S000..S999 all taken.
    Full,
    /// A write or the verify failed (at worst lost clusters, see above).
    Error,
}

/// Write the current screen to the next free S%03u.BMP. Synchronous (see
/// the module notes); the caller must not draw before it returns.
pub fn take() -> Shot {
    take_on(&mut crate::spiflash::SpiFlash::new())
}

/// `take` on any block device (host test: tools/fatren-test).
pub fn take_on(disk: &mut dyn BlockDevice) -> Shot {
    let Some(n) = calbackup::free_number(disk, b"S", b"BMP") else {
        return if disk.present() { Shot::Full } else { Shot::NoDisk };
    };
    let r = calbackup::write_file(disk, calbackup::numbered_name(b"S", n, b"BMP"), &Gram);
    match r.status {
        Status::Written => {
            MEDIUM_CHANGED.store(true, Ordering::Relaxed);
            Shot::Saved(n)
        }
        Status::NoSpace => Shot::Full,
        Status::NoMedium | Status::BadBpb => Shot::NoDisk,
        // Unchanged cannot happen for a new name; Error may have written
        // blocks (lost clusters at worst): let the host re-read too.
        _ => {
            MEDIUM_CHANGED.store(true, Ordering::Relaxed);
            Shot::Error
        }
    }
}
