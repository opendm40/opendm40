//! USB mass storage (SCSI over Bulk-Only Transport, usbd-storage), as the
//! stock firmware's disk mode, but always present in the composite device.
//!
//! The medium is a `BlockDevice`: the on-board W25Q128 (spiflash.rs), whose
//! FAT16 volume with 4096-byte sectors the stock firmware made
//! (firmware/re/analysis/spiflash_v2_6.h).

use usb_device::class_prelude::*;
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

/// A host has the medium locked: its last PREVENT ALLOW MEDIUM REMOVAL
/// (0x1E) had Prevent = 1. Linux sends it on the first open of the block
/// device (mount) and Allow on the last close (umount / eject); a PC that has
/// the disk mounted keeps its own FAT / directory cache, so the device must
/// not write the volume meanwhile (screenshot.rs refuses) and must not send
/// it UNIT ATTENTION (Linux fails I/O on a mounted removable disk until the
/// next media poll). Cleared on a bus reset and while the device is not
/// configured (unplugged / suspended: `set_configured`).
static HOST_LOCKED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static CONFIGURED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// A PC has the USB disk mounted (see HOST_LOCKED).
pub fn host_mounted() -> bool {
    use core::sync::atomic::Ordering::Relaxed;
    HOST_LOCKED.load(Relaxed) && CONFIGURED.load(Relaxed)
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
/// "USBC", tag, data length 0, flags, LUN 0, CB length, CB[0] = 0x1E, CB[4]
/// bit 0 = Prevent. Every match is then defaced (signature zeroed), so a
/// stale CBW of an earlier 0x1E can never be found again; more than one
/// match left (host data that happens to look like one) counts as Prevent.
fn prevent_bit(io: &mut [u8]) -> bool {
    let (mut found, mut prevent) = (0u32, false);
    let mut o = 0usize;
    while o + 31 <= io.len() {
        let c = &io[o..o + 31];
        if c[..4] == *b"USBC" && c[8..12] == [0; 4] && c[13] == 0 && (6..=16).contains(&c[14]) && c[15] == 0x1E {
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
    sense: Sense,
    blk: [u8; MAX_BLOCK],
    /// Refuse host writes (NOT READY, becoming ready) while set (M4: the
    /// logger's constant sample interval cannot survive a 50..400 ms erase).
    write_inhibit: bool,
}

type Res = Result<(), TransportError<BulkOnlyError>>;

impl<'a, B: UsbBus> Msc<'a, B> {
    pub fn new(alloc: &'a UsbBusAllocator<B>, io: &'static mut [u8]) -> Self {
        let (p, n) = (io.as_mut_ptr(), io.len());
        let scsi = Scsi::new(alloc, 64, 0, IoBuf { p, n }).unwrap_or_else(|_| panic!("msc"));
        Msc { scsi, io: IoBuf { p, n }, xfer: Xfer::None, sense: SENSE_OK, blk: [0; MAX_BLOCK], write_inhibit: false }
    }

    /// The device is configured by a host (main loop, every pass): while not
    /// (unplugged, suspended), no host can have the disk mounted.
    pub fn set_configured(&mut self, on: bool) {
        CONFIGURED.store(on, core::sync::atomic::Ordering::Relaxed);
        if !on {
            HOST_LOCKED.store(false, core::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Serve pending SCSI commands from the main loop.
    pub fn poll(&mut self, dev: &mut dyn BlockDevice) {
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
        let _ = self.scsi.poll_command(|cmd| -> Res { handle(cmd, dev, xfer, sense, blk, io, inhibit) });
        HOST_WRITING.store(matches!(self.xfer, Xfer::Write { .. }), core::sync::atomic::Ordering::Relaxed);
    }

    /// Block (true) or allow host writes; a write already in progress finishes.
    pub fn set_write_inhibit(&mut self, on: bool) {
        self.write_inhibit = on;
    }
}

type Cmd<'c, 'a, B> = Command<'c, ScsiCommand, Scsi<BulkOnly<'a, B, IoBuf>>>;

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
    let bs = dev.block_size();
    // One-shot UNIT ATTENTION after an on-device file write (screenshot.rs
    // sets MEDIUM_CHANGED): the next command fails with 06/28/00, so the host
    // drops its cached FAT / directory and re-reads the volume. Only at the
    // start of a command (a multi-block transfer in progress is never cut),
    // and never for INQUIRY / REQUEST SENSE (SPC: they do not report it).
    // Held back while a host has the medium locked (mounted Linux vfat: the
    // UNIT ATTENTION would fail its I/O, review 258c833 M2); it goes out on
    // the first command after the Allow (umount).
    if *xfer == Xfer::None
        && !matches!(kind, ScsiCommand::Inquiry { .. } | ScsiCommand::RequestSense { .. })
        && !HOST_LOCKED.load(core::sync::atomic::Ordering::Relaxed)
        && crate::screenshot::MEDIUM_CHANGED.swap(false, core::sync::atomic::Ordering::Relaxed)
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
            d[16..32].copy_from_slice(b"DM40 disk       ");
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
            // Mode parameter header: no block descriptors, not write protected.
            let d = [3u8, 0, 0, 0];
            cmd.try_write_data_all(&d)?;
            cmd.pass(4);
        }
        ScsiCommand::ModeSense10 { .. } => {
            let d = [0u8, 6, 0, 0, 0, 0, 0, 0];
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
            if !dev.present() {
                *sense = SENSE_NO_MEDIUM;
                cmd.fail(0);
                return Ok(());
            }
            if *xfer == Xfer::None {
                if write_inhibit {
                    *sense = SENSE_BECOMING_READY;
                    cmd.fail(0);
                    return Ok(());
                }
                if lba + len as u32 > dev.block_count() {
                    *sense = SENSE_LBA_RANGE;
                    cmd.fail(0);
                    return Ok(());
                }
                *xfer = Xfer::Write { lba, left: len as u32, off: 0, done: 0 };
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
        // stop sending them: sdev->lockable = 0), Prevent bit tracked.
        ScsiCommand::Unknown { cmd: 0x1E } => {
            let p = prevent_bit(core::borrow::BorrowMut::<[u8]>::borrow_mut(io));
            HOST_LOCKED.store(p, core::sync::atomic::Ordering::Relaxed);
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
