//! USBFS (Synopsys DWC2 core at 0x50000000, STM32 OTG_FS register layout).
//!
//! Only USBFS is wired (PA11 DM / PA12 DP, AF10). The 48 MHz USB clock is ours:
//! the stock PLLQ gives 400/9 = 44.4 MHz and IRC48M is off, so we start PLLSAI
//! (shared 1 MHz PLL input, N=288 -> 288 MHz VCO, P=6 -> 48 MHz) and select
//! CK48M = PLL48M = PLLSAIP. PLLSAI is unused by the stock firmware (disabled,
//! reset value), so reprogramming it does not disturb SYSCLK.
//!
//! VBUS sensing is off (GCCFG VBUSIG): PA9/PA10 are the HY3131 relay drive, not
//! VBUS, and must not be touched here.

use crate::clock::{Clock, HCLK_HZ};
use crate::mmio::{modify32, r32, w32};

const RCU: u32 = 0x4002_3800;
const RCU_CTL: u32 = RCU + 0x00;
const RCU_AHB2RST: u32 = RCU + 0x14;
const RCU_AHB1EN: u32 = RCU + 0x30;
const RCU_AHB2EN: u32 = RCU + 0x34;
const RCU_PLLSAI: u32 = RCU + 0x88;
const RCU_ADDCTL: u32 = RCU + 0xC0;

const CTL_PLLSAIEN: u32 = 1 << 28;
const CTL_PLLSAISTB: u32 = 1 << 29;
const AHB2_USBFS: u32 = 1 << 7;

const GPIOA: u32 = 0x4002_0000;

pub const USBFS: u32 = 0x5000_0000;
const GCCFG: u32 = USBFS + 0x38;
const GRSTCTL: u32 = USBFS + 0x10;
const GRSTCTL_AHBIDL: u32 = 1 << 31;
const GCCFG_PWRON: u32 = 1 << 16;
const GCCFG_VBUSIG: u32 = 1 << 21;

pub struct UsbFs;

unsafe impl synopsys_usb_otg::UsbPeripheral for UsbFs {
    const REGISTERS: *const () = USBFS as *const ();
    const HIGH_SPEED: bool = false;
    const FIFO_DEPTH_WORDS: usize = 320; // 1.25 KiB
    const ENDPOINT_COUNT: usize = 4;

    fn enable() {
        unsafe { modify32(RCU_AHB2EN, 0, AHB2_USBFS) };
    }

    fn ahb_frequency_hz(&self) -> u32 {
        HCLK_HZ
    }
}

pub struct HwStatus {
    pub pllsai_locked: bool,
    /// GRSTCTL.AHBIDL after reset: the core is clocked and answering. (GSNPSID,
    /// the usual DWC2 ID check, reads 0 on this GD32.)
    pub core_idle: bool,
}

/// Bring up the 48 MHz clock, PA11/PA12 and a freshly reset USBFS block. The
/// reset also drops the D+ pull-up, so a host that had the stock device
/// enumerated sees a disconnect before we attach.
pub fn init_hw(clk: &mut Clock) -> HwStatus {
    unsafe {
        // PLLSAI: N=288, P=6 (field 0b10); keep the other fields (R) as they are.
        modify32(RCU_CTL, CTL_PLLSAIEN, 0);
        modify32(RCU_PLLSAI, (0x1FF << 6) | (0b11 << 16), (288 << 6) | (0b10 << 16));
        modify32(RCU_CTL, 0, CTL_PLLSAIEN);
    }
    let t0 = clk.now_ms();
    let mut pllsai_locked = false;
    while clk.now_ms() - t0 < 10 {
        if unsafe { r32(RCU_CTL) } & CTL_PLLSAISTB != 0 {
            pllsai_locked = true;
            break;
        }
    }
    unsafe {
        // CK48MSEL=0 (PLL48M), PLL48MSEL=1 (PLLSAIP).
        modify32(RCU_ADDCTL, 0b11, 0b10);

        // Resetting the USB block alone does not make the host notice a
        // disconnect (observed: the D+ pull-up stays), so the host would keep
        // talking to a stale address. Drive DM/DP low as GPIO (SE0) for a while
        // to force a disconnect; attaching again in AF mode then re-enumerates.
        modify32(RCU_AHB1EN, 0, 1 << 0);
        w32(GPIOA + 0x18, (1 << (11 + 16)) | (1 << (12 + 16))); // BOP: PA11/PA12 low
        modify32(GPIOA + 0x04, (1 << 11) | (1 << 12), 0); // push-pull
        modify32(GPIOA + 0x00, (3 << 22) | (3 << 24), (1 << 22) | (1 << 24)); // output
    }
    clk.delay_ms(100);
    unsafe {
        // PA11/PA12: AF10, very high speed, no pull.
        modify32(GPIOA + 0x00, (3 << 22) | (3 << 24), (2 << 22) | (2 << 24));
        modify32(GPIOA + 0x08, 0, (3 << 22) | (3 << 24));
        modify32(GPIOA + 0x0C, (3 << 22) | (3 << 24), 0);
        modify32(GPIOA + 0x24, (0xF << 12) | (0xF << 16), (10 << 12) | (10 << 16));

        // Clean USBFS state, whatever the stock firmware left.
        modify32(RCU_AHB2EN, 0, AHB2_USBFS);
        modify32(RCU_AHB2RST, 0, AHB2_USBFS);
        modify32(RCU_AHB2RST, AHB2_USBFS, 0);
    }
    clk.delay_ms(50);
    HwStatus { pllsai_locked, core_idle: unsafe { r32(GRSTCTL) } & GRSTCTL_AHBIDL != 0 }
}

/// After the bus is enabled: transceiver on, VBUS ignored. The driver only does
/// this for core IDs it recognises, so force it.
pub fn force_power_no_vbus() {
    unsafe { modify32(GCCFG, 0, GCCFG_PWRON | GCCFG_VBUSIG) };
}
