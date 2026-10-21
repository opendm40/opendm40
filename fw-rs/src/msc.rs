//! USB mass storage (SCSI over Bulk-Only Transport, usbd-storage), as the
//! stock firmware's disk mode, but always present in the composite device.
//!
//! The medium is a `BlockDevice`: the on-board W25Q128 (spiflash.rs), whose
//! FAT16 volume with 4096-byte sectors the stock firmware made
//! (firmware/re/analysis/spiflash_v2_6.h). LUN 0.
//!
//! Normal mode has a second, read-only LUN (`Msc::new(.., 2)`, GET MAX LUN =
//! 1): the information volume generated from the MCU memory (vdisk.rs). Every
//! command is dispatched to the device of its CBW's LUN; INQUIRY gives each
//! LUN its own product name (both removable), READ CAPACITY / READ FORMAT
//! CAPACITIES its own geometry, MODE SENSE the write-protect bit of a
//! read-only device, and a WRITE to it fails with DATA PROTECT, WRITE
//! PROTECTED (07/27/00) without touching anything. Sense data is per LUN.
//! The host lock (PREVENT ALLOW MEDIUM REMOVAL -> `host_mounted`) and the
//! one-shot UNIT ATTENTION after an on-device write concern the SPI disk
//! only: LUN 0. Recovery mode (recovery-usb) keeps a single LUN.

use usb_device::class_prelude::*;
use usb_device::prelude::UsbDeviceState;
use usbd_storage::subclass::scsi::{Scsi, ScsiCommand};
use usbd_storage::subclass::Command;
use usbd_storage::transport::bbb::{BulkOnly, BulkOnlyError};
use usbd_storage::transport::TransportError;

pub const MAX_BLOCK: usize = 4096;

/// Block storage behind the USB disk.
pub trait BlockDevice {
    /// Medium available (false = "no medium").
    fn present(&self) -> bool;
    fn block_size(&self) -> u32;
    fn block_count(&self) -> u32;
    fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> bool;
    fn write_block(&mut self, lba: u32, buf: &[u8]) -> bool;
    /// Write-protected medium (MODE SENSE WP bit; WRITE fails with DATA
    /// PROTECT before any data is taken).
    fn read_only(&self) -> bool {
        false
    }
    /// One-shot "the volume changed under the host" (vdisk.rs `set_extra`):
    /// true once, and the next command of this LUN gets UNIT ATTENTION
    /// 06/28/00. LUN 0 uses screenshot::MEDIUM_CHANGED instead.
    fn take_media_changed(&mut self) -> bool {
        false
    }
    /// `write_block` spread over several calls, for a caller that must not
    /// stall the main loop (screenshot.rs). Call again with the same `lba`,
    /// `buf` and `st` (starting from `Step::default()`) until it returns
    /// Some(ok). Default: the whole write at once.
    fn write_step(&mut self, lba: u32, buf: &[u8], st: &mut Step) -> Option<bool> {
        let _ = st;
        Some(self.write_block(lba, buf))
    }
}

/// Progress of one `BlockDevice::write_step` block write (spiflash.rs).
#[derive(Clone, Copy, Default)]
pub struct Step {
    pub started: bool,
    /// 0 compare, 1 erase, 2 program, 3 read back.
    pub phase: u8,
    pub page: u8,
    /// Pages to program (bit = page).
    pub dirty: u16,
    pub erase: bool,
    /// Start of the current wait (DWT cycles), for the timeout.
    pub t0: u32,
}

/// A host WRITE(10) is in progress (between its first and last block), so an
/// on-device writer (screenshot.rs) waits before reading the FAT.
pub static HOST_WRITING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// A CSV recording session is active (csvlog.rs): host writes are refused
/// for the whole session (its cached FAT / directory state must not go stale).
/// Set and cleared by csvlog only.
pub static LOG_SESSION: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// A csvlog commit is writing the disk: screenshot.rs waits before its FAT
/// scan (one on-device writer at a time). Set and cleared by csvlog only.
pub static LOG_FLUSHING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Xfer {
    None,
    Read { lba: u32, left: u32, off: usize, done: u32 },
    Write { lba: u32, left: u32, off: usize, done: u32 },
    /// A refused WRITE: its data phase is read and dropped, then the
    /// command fails (sense already set). `left` bytes still to take.
    Discard { left: u32 },
}

/// Sense key / ASC / ASCQ for REQUEST SENSE.
#[derive(Clone, Copy)]
struct Sense(u8, u8, u8);

const SENSE_OK: Sense = Sense(0, 0, 0);
const SENSE_NO_MEDIUM: Sense = Sense(0x02, 0x3A, 0x00);
const SENSE_INVALID: Sense = Sense(0x05, 0x20, 0x00);
const SENSE_LBA_RANGE: Sense = Sense(0x05, 0x21, 0x00);
const SENSE_MEDIUM_ERROR: Sense = Sense(0x03, 0x11, 0x00);
/// NOT READY, LOGICAL UNIT IS IN PROCESS OF BECOMING READY (writes
/// inhibited). ASCQ 01, not 00 ("cause not reportable"): Linux
/// (scsi_io_completion_action) delays and retries 04/01 but fails 04/00 at
/// once, i.e. a host write during a screenshot would get EIO (review 258c833
/// M1).
const SENSE_BECOMING_READY: Sense = Sense(0x02, 0x04, 0x01);
/// UNIT ATTENTION, NOT READY TO READY CHANGE, MEDIUM MAY HAVE CHANGED: the
/// firmware itself wrote the disk (screenshot.rs), the host must re-read it.
const SENSE_MEDIUM_CHANGED: Sense = Sense(0x06, 0x28, 0x00);
/// DATA PROTECT, WRITE PROTECTED: a WRITE to a read-only LUN.
const SENSE_WRITE_PROTECTED: Sense = Sense(0x07, 0x27, 0x00);
/// ILLEGAL REQUEST, LOGICAL UNIT NOT SUPPORTED (a LUN above GET MAX LUN).
const SENSE_BAD_LUN: Sense = Sense(0x05, 0x25, 0x00);

/// LUNs at most (SPI disk, information volume).
pub const MAX_LUNS: usize = 2;
/// INQUIRY product identification per LUN (16 characters).
const PRODUCT: [&[u8; 16]; MAX_LUNS] = [b"DM40 disk       ", b"DM40 info       "];

/// A host has the medium locked: its last PREVENT ALLOW MEDIUM REMOVAL
/// (0x1E) had Prevent = 1. Linux sends it on the first open of the block
/// device (mount) and Allow on the last close (umount / eject); a PC that has
/// the disk mounted keeps its own FAT / directory cache, so the device must
/// not write the volume meanwhile (screenshot.rs refuses) and must not send
/// it UNIT ATTENTION (Linux fails I/O on a mounted removable disk until the
/// next media poll). Cleared by an Allow, a bus reset (`Msc::reset`) and an
/// unconfigure (`set_usb_state`: Default / Addressed), NOT by a USB suspend:
/// Linux runtime autosuspend of an idle mounted disk keeps the mount (and
/// sends no PREVENT again on resume), review 3443cea M2. VBUS sensing is off
/// (usb.rs), so an unplug without eject also looks like a suspend: the lock
/// then stays set ("Disk in use") until the replug's bus reset or a reboot,
/// the safe side.
static HOST_LOCKED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// The same for LUN 1 (information volume): only holds back its UNIT
/// ATTENTION; nothing on the device waits for it.
static INFO_LOCKED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn lock_of(lun: u8) -> &'static core::sync::atomic::AtomicBool {
    if lun == 0 { &HOST_LOCKED } else { &INFO_LOCKED }
}

/// A PC has the USB disk mounted (see HOST_LOCKED).
pub fn host_mounted() -> bool {
    HOST_LOCKED.load(core::sync::atomic::Ordering::Relaxed)
}

/// The I/O buffer handed to usbd-storage, shared through a raw pointer so
/// that `handle` can read the CDB bytes the crate does not expose (the
/// Prevent bit of 0x1E; usbd-storage 3.0.0 only reports the opcode). Neither
/// side keeps a slice borrowed across calls.
pub struct IoBuf {
    p: *mut u8,
    n: usize,
}

impl core::borrow::Borrow<[u8]> for IoBuf {
    fn borrow(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.p, self.n) }
    }
}

impl core::borrow::BorrowMut<[u8]> for IoBuf {
    fn borrow_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.p, self.n) }
    }
}

/// The Prevent bit of the PREVENT ALLOW MEDIUM REMOVAL command being handled,
/// read from its CBW in the transport buffer. When the callback runs, the
/// 31-byte CBW the crate has just parsed is still in `io` (it is read in
/// place, the buffer is only written again for the data / status phase):
/// "USBC", tag, data length 0, flags, LUN `lun`, CB length, CB[0] = 0x1E,
/// CB[4] bit 0 = Prevent. Every match is then defaced (signature zeroed), so
/// a stale CBW of an earlier 0x1E can never be found again; more than one
/// match left (host data that happens to look like one) counts as Prevent.
fn prevent_bit(io: &mut [u8], lun: u8) -> bool {
    let (mut found, mut prevent) = (0u32, false);
    let mut o = 0usize;
    while o + 31 <= io.len() {
        let c = &io[o..o + 31];
        if c[..4] == *b"USBC" && c[8..12] == [0; 4] && c[13] == lun && (6..=16).contains(&c[14]) && c[15] == 0x1E {
            found += 1;
            prevent = c[19] & 1 != 0;
            io[o..o + 4].fill(0);
            o += 31;
        } else {
            o += 1;
        }
    }
    found != 1 || prevent
}

pub struct Msc<'a, B: UsbBus> {
    pub scsi: Scsi<BulkOnly<'a, B, IoBuf>>,
    io: IoBuf,
    xfer: Xfer,
    /// Per LUN.
    sense: [Sense; MAX_LUNS],
    blk: [u8; MAX_BLOCK],
    /// Refuse host writes (NOT READY, becoming ready) while set (M4: the
    /// logger's constant sample interval cannot survive a 50..400 ms erase).
    write_inhibit: bool,
}

type Res = Result<(), TransportError<BulkOnlyError>>;

impl<'a, B: UsbBus> Msc<'a, B> {
    /// `luns`: 1 (SPI disk) or 2 (+ the information volume); GET MAX LUN
    /// answers `luns - 1`, `poll` takes that many devices.
    pub fn new(alloc: &'a UsbBusAllocator<B>, io: &'static mut [u8], luns: u8) -> Self {
        let (p, n) = (io.as_mut_ptr(), io.len());
        let max_lun = luns.clamp(1, MAX_LUNS as u8) - 1;
        let scsi = Scsi::new(alloc, 64, max_lun, IoBuf { p, n }).unwrap_or_else(|_| panic!("msc"));
        Msc { scsi, io: IoBuf { p, n }, xfer: Xfer::None, sense: [SENSE_OK; MAX_LUNS], blk: [0; MAX_BLOCK], write_inhibit: false }
    }

    /// The USB device state (main loop, every pass). Default / Addressed
    /// (bus reset, SET_CONFIGURATION 0): no host can have the disk mounted,
    /// the lock is cleared. Suspend keeps it (see HOST_LOCKED).
    pub fn set_usb_state(&mut self, st: UsbDeviceState) {
        if matches!(st, UsbDeviceState::Default | UsbDeviceState::Addressed) {
            HOST_LOCKED.store(false, core::sync::atomic::Ordering::Relaxed);
            INFO_LOCKED.store(false, core::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Serve pending SCSI commands from the main loop. `devs[i]` = LUN i.
    pub fn poll(&mut self, devs: &mut [&mut dyn BlockDevice]) {
        let xfer = &mut self.xfer;
        let sense = &mut self.sense;
        let blk = &mut self.blk;
        let io = &mut self.io;
        // Also no host writes while a screenshot is being written (its FAT /
        // directory update spans many main-loop passes) or a CSV recording
        // session is active (csvlog.rs).
        let inhibit = self.write_inhibit
            || crate::screenshot::busy()
            || LOG_SESSION.load(core::sync::atomic::Ordering::Relaxed);
        let _ = self.scsi.poll_command(|cmd| -> Res {
            let lun = cmd.lun as usize;
            match (devs.get_mut(lun), sense.get_mut(lun)) {
                (Some(dev), Some(sense)) => handle(cmd, &mut **dev, xfer, sense, blk, io, inhibit),
                _ => bad_lun(cmd),
            }
        });
        HOST_WRITING.store(matches!(self.xfer, Xfer::Write { .. }), core::sync::atomic::Ordering::Relaxed);
    }

    /// Block (true) or allow host writes; a write already in progress finishes.
    pub fn set_write_inhibit(&mut self, on: bool) {
        self.write_inhibit = on;
    }
}

type Cmd<'c, 'a, B> = Command<'c, ScsiCommand, Scsi<BulkOnly<'a, B, IoBuf>>>;

/// A LUN above GET MAX LUN (a host should never address one): INQUIRY says
/// "no device here" (peripheral qualifier 3), REQUEST SENSE "LUN not
/// supported", anything else fails.
fn bad_lun<B: UsbBus>(mut cmd: Cmd<'_, '_, B>) -> Res {
    match cmd.kind {
        ScsiCommand::Inquiry { .. } => {
            let mut d = [0u8; 36];
            d[0] = 0x7F;
            d[4] = 31;
            cmd.try_write_data_all(&d)?;
            cmd.pass(36);
        }
        ScsiCommand::RequestSense { .. } => {
            let s = SENSE_BAD_LUN;
            let d = [0x70, 0, s.0, 0, 0, 0, 0, 10, 0, 0, 0, 0, s.1, s.2, 0, 0, 0, 0];
            cmd.try_write_data_all(&d)?;
            cmd.pass(d.len() as u32);
        }
        _ => cmd.fail(0),
    }
    Ok(())
}

fn handle<B: UsbBus>(
    mut cmd: Cmd<'_, '_, B>,
    dev: &mut dyn BlockDevice,
    xfer: &mut Xfer,
    sense: &mut Sense,
    blk: &mut [u8; MAX_BLOCK],
    io: &mut IoBuf,
    write_inhibit: bool,
) -> Res {
    let kind = cmd.kind;
    let lun = cmd.lun;
    let bs = dev.block_size();
    // One-shot UNIT ATTENTION after an on-device file write (screenshot.rs
    // sets MEDIUM_CHANGED): the next command fails with 06/28/00, so the host
    // drops its cached FAT / directory and re-reads the volume. Only at the
    // start of a command (a multi-block transfer in progress is never cut),
    // and never for INQUIRY / REQUEST SENSE (SPC: they do not report it).
    // Held back while a host has the medium locked (mounted Linux vfat: the
    // UNIT ATTENTION would fail its I/O, review 258c833 M2); it goes out on
    // the first command after the Allow (umount). Per LUN: the SPI disk's
    // flag (screenshot::MEDIUM_CHANGED) is only taken by a LUN-0 command, the
    // information volume's (vdisk.rs set_extra, `take_media_changed`) only by
    // a LUN-1 one, each held back by its own LUN's lock.
    if *xfer == Xfer::None
        && !matches!(kind, ScsiCommand::Inquiry { .. } | ScsiCommand::RequestSense { .. })
        && !lock_of(lun).load(core::sync::atomic::Ordering::Relaxed)
        && if lun == 0 {
            crate::screenshot::MEDIUM_CHANGED.swap(false, core::sync::atomic::Ordering::Relaxed)
        } else {
            dev.take_media_changed()
        }
    {
        *sense = SENSE_MEDIUM_CHANGED;
        cmd.fail(0);
        return Ok(());
    }
    match kind {
        ScsiCommand::Inquiry { .. } => {
            let mut d = [0u8; 36];
            d[0] = 0x00; // direct access block device
            d[1] = 0x80; // removable
            d[2] = 0x04; // SPC-2
            d[3] = 0x02;
            d[4] = 31;
            d[8..16].copy_from_slice(b"ALIENTEK");
            d[16..32].copy_from_slice(PRODUCT[(lun as usize).min(MAX_LUNS - 1)]);
            d[32..36].copy_from_slice(b"0.2 ");
            cmd.try_write_data_all(&d)?;
            cmd.pass(36);
        }
        ScsiCommand::TestUnitReady => {
            if dev.present() {
                *sense = SENSE_OK;
                cmd.pass(0);
            } else {
                *sense = SENSE_NO_MEDIUM;
                cmd.fail(0);
            }
        }
        ScsiCommand::RequestSense { .. } => {
            let s = *sense;
            let d = [0x70, 0, s.0, 0, 0, 0, 0, 10, 0, 0, 0, 0, s.1, s.2, 0, 0, 0, 0];
            cmd.try_write_data_all(&d)?;
            *sense = SENSE_OK;
            cmd.pass(d.len() as u32);
        }
        ScsiCommand::ModeSense6 { .. } => {
            // Mode parameter header only (no block descriptors, no pages);
            // device-specific parameter bit 7 = write protected.
            let wp = if dev.read_only() { 0x80 } else { 0 };
            let d = [3u8, 0, wp, 0];
            cmd.try_write_data_all(&d)?;
            cmd.pass(4);
        }
        ScsiCommand::ModeSense10 { .. } => {
            let wp = if dev.read_only() { 0x80 } else { 0 };
            let d = [0u8, 6, 0, wp, 0, 0, 0, 0];
            cmd.try_write_data_all(&d)?;
            cmd.pass(8);
        }
        ScsiCommand::ReadCapacity10 => {
            if !dev.present() {
                *sense = SENSE_NO_MEDIUM;
                cmd.fail(0);
            } else {
                let mut d = [0u8; 8];
                d[..4].copy_from_slice(&(dev.block_count() - 1).to_be_bytes());
                d[4..].copy_from_slice(&bs.to_be_bytes());
                cmd.try_write_data_all(&d)?;
                cmd.pass(8);
            }
        }
        ScsiCommand::ReadFormatCapacities { .. } => {
            let mut d = [0u8; 12];
            d[3] = 8;
            let (n, kind) = if dev.present() { (dev.block_count(), 2u8) } else { (0xFFFF_FFFF, 3u8) };
            d[4..8].copy_from_slice(&n.to_be_bytes());
            d[8] = kind; // 2 formatted media, 3 no media
            d[9..12].copy_from_slice(&bs.to_be_bytes()[1..]);
            cmd.try_write_data_all(&d)?;
            cmd.pass(12);
        }
        ScsiCommand::Read { lba, len } => {
            if !dev.present() {
                *sense = SENSE_NO_MEDIUM;
                cmd.fail(0);
                return Ok(());
            }
            if *xfer == Xfer::None {
                if lba + len as u32 > dev.block_count() {
                    *sense = SENSE_LBA_RANGE;
                    cmd.fail(0);
                    return Ok(());
                }
                *xfer = Xfer::Read { lba, left: len as u32, off: 0, done: 0 };
            }
            if let Xfer::Read { lba, left, off, done } = *xfer {
                let mut off = off;
                if off == 0 && !dev.read_block(lba, &mut blk[..bs as usize]) {
                    *xfer = Xfer::None;
                    *sense = SENSE_MEDIUM_ERROR;
                    cmd.fail(done);
                    return Ok(());
                }
                let n = cmd.write_data(&blk[off..bs as usize])?;
                off += n;
                let done = done + n as u32;
                if off >= bs as usize {
                    if left <= 1 {
                        *xfer = Xfer::None;
                        cmd.pass(done);
                        return Ok(());
                    }
                    *xfer = Xfer::Read { lba: lba + 1, left: left - 1, off: 0, done };
                } else {
                    *xfer = Xfer::Read { lba, left, off, done };
                }
            }
        }
        ScsiCommand::Write { lba, len } => {
            // A refused WRITE still takes its whole data phase (dropped) and
            // only then fails: failing at once leaves the data to
            // usbd-storage's "read up to the residue" path, which never sent
            // the CSW in the emulator (USB/IP, 2026-10-21: the host waited
            // forever for it). The host's data length is len x block size.
            if *xfer == Xfer::None {
                let refuse = if !dev.present() {
                    Some(SENSE_NO_MEDIUM)
                } else if dev.read_only() {
                    Some(SENSE_WRITE_PROTECTED)
                } else if write_inhibit {
                    Some(SENSE_BECOMING_READY)
                } else if lba + len as u32 > dev.block_count() {
                    Some(SENSE_LBA_RANGE)
                } else {
                    None
                };
                *xfer = match refuse {
                    Some(s) => {
                        *sense = s;
                        Xfer::Discard { left: len as u32 * bs }
                    }
                    None => Xfer::Write { lba, left: len as u32, off: 0, done: 0 },
                };
            }
            if let Xfer::Discard { left } = *xfer {
                let n = if left == 0 { 0 } else { cmd.read_data(&mut blk[..(left as usize).min(MAX_BLOCK)])? as u32 };
                if n >= left {
                    *xfer = Xfer::None;
                    cmd.fail(0);
                } else {
                    *xfer = Xfer::Discard { left: left - n };
                }
                return Ok(());
            }
            if let Xfer::Write { lba, left, off, done } = *xfer {
                let n = cmd.read_data(&mut blk[off..bs as usize])?;
                let off = off + n;
                let done = done + n as u32;
                if off >= bs as usize {
                    if !dev.write_block(lba, &blk[..bs as usize]) {
                        *xfer = Xfer::None;
                        *sense = SENSE_MEDIUM_ERROR;
                        cmd.fail(done);
                        return Ok(());
                    }
                    if left <= 1 {
                        *xfer = Xfer::None;
                        cmd.pass(done);
                        return Ok(());
                    }
                    *xfer = Xfer::Write { lba: lba + 1, left: left - 1, off: 0, done };
                } else {
                    *xfer = Xfer::Write { lba, left, off, done };
                }
            }
        }
        // PREVENT ALLOW MEDIUM REMOVAL: accepted (a failed one makes Linux
        // stop sending them: sdev->lockable = 0), Prevent bit tracked per
        // LUN: only LUN 0's lock is `host_mounted` (mounting the read-only
        // information volume must not block the on-device writers); LUN 1's
        // only holds back its UNIT ATTENTION.
        ScsiCommand::Unknown { cmd: 0x1E } => {
            let p = prevent_bit(core::borrow::BorrowMut::<[u8]>::borrow_mut(io), lun);
            lock_of(lun).store(p, core::sync::atomic::Ordering::Relaxed);
            *sense = SENSE_OK;
            cmd.pass(0)
        }
        // START STOP UNIT (eject / power-off by the host): noted for recovery.
        #[cfg(any(feature = "native", feature = "test-recovery"))]
        ScsiCommand::Unknown { cmd: 0x1B } => { STOP_UNIT.store(true, core::sync::atomic::Ordering::Relaxed); cmd.pass(0) }
        _ => { *sense = SENSE_INVALID; cmd.fail(0); }
    }
    Ok(())
}

impl<B: UsbBus> UsbClass<B> for Msc<'_, B> {
    fn get_configuration_descriptors(&self, w: &mut DescriptorWriter) -> usb_device::Result<()> {
        self.scsi.get_configuration_descriptors(w)
    }
    fn reset(&mut self) {
        self.xfer = Xfer::None;
        HOST_WRITING.store(false, core::sync::atomic::Ordering::Relaxed);
        // A bus reset starts a fresh session: a pending one-shot UNIT
        // ATTENTION from a screenshot taken while unplugged is moot (review
        // 2026-10-13 L3).
        crate::screenshot::MEDIUM_CHANGED.store(false, core::sync::atomic::Ordering::Relaxed);
        HOST_LOCKED.store(false, core::sync::atomic::Ordering::Relaxed);
        INFO_LOCKED.store(false, core::sync::atomic::Ordering::Relaxed);
        self.scsi.reset()
    }
    fn control_in(&mut self, xfer: ControlIn<B>) {
        self.scsi.control_in(xfer)
    }
    fn control_out(&mut self, xfer: ControlOut<B>) {
        self.scsi.control_out(xfer)
    }
    fn endpoint_out(&mut self, addr: EndpointAddress) {
        self.scsi.endpoint_out(addr)
    }
    fn endpoint_in_complete(&mut self, addr: EndpointAddress) {
        self.scsi.endpoint_in_complete(addr)
    }
    fn poll(&mut self) {
        self.scsi.poll()
    }
}

/// Set when the host sent SCSI START STOP UNIT (opcode 0x1B: eject, or a
/// stop / power-off request). usbd-storage hides the CDB bytes (LoEj/Start),
/// so any 0x1B counts; the native recovery mode powers off on it.
#[cfg(any(feature = "native", feature = "test-recovery"))]
pub static STOP_UNIT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
