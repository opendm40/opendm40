//! Recovery mode (the shim decision). The shim takes it only on the
//! backup-SRAM request word (boot::REQ_MAGIC), which nothing sets now.
//!
//! Hands recovery to the vendor FS-loader's update mode by zeroing the 12-byte
//! app marker at 0x08008000 (boot::zero_marker: FMC word programs of 0, no
//! erase, as the loader itself does when it finds a dm40*.bin), then
//! SYSRESETREQ. The ROM and the loader both reject an app whose marker is not
//! valid, on any reset (cold or warm, whatever PA2 reads), so the loader shows
//! "open file error" and brings up its own working USB disk (28e9:028f) on
//! every boot until a valid dm40*.bin is installed through it
//! (stock/re/docs/fsloader-install-trigger.md, "Update-loader install & error paths").
//! The app bytes stay intact but the app cannot run again until that install:
//! this is a one-way door into the vendor updater, by design.
//!
//! User procedure: the meter restarts into the loader's update screen and a
//! USB disk appears -> copy a firmware dm40*.bin (the native opendm40 image, or
//! dm40_v_2_6.bin) onto it -> press "update" on the screen (or power-cycle).
//!
//! Recovery needs no SPI flash, FAT, USB, LCD, fonts, touch, HY3131, scope,
//! generator, BT or beeper: power hold (PE2), the FWDGT fed around the flash
//! operation, 3 word programs, a reset. The read-back decides: if the three
//! words do not read back as 0 (FMC error flags are only recorded, see
//! boot::fmc_last), it records the failure in backup SRAM
//! (BKP_TRIG = 0x54520011) and stops feeding the watchdog: the FWDGT resets,
//! and the shim (previous mode = recovery) boots normally.

use crate::boot;

/// Recovery entry.
pub fn run() -> ! {
    boot::feed();
    boot::latch_on();
    if boot::zero_marker() {
        boot::record_trigger(0x10);
        boot::sysreset();
    }
    boot::record_trigger(0x11);
    loop {
        core::hint::spin_loop();
    }
}
