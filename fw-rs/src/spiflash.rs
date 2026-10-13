//! On-board SPI NOR flash (W25Q128JV, 16 MiB) as the USB disk medium
//! (firmware/re/analysis/spiflash_v2_6.h).
//!
//! SPI1 @ 0x40003800 (APB1 50 MHz): SCK = PD3, MISO = PC2, MOSI = PC3 (AF5),
//! CS = PE4 (GPIO, active low). Mode 3, MSB first, polled byte exchange, the
//! same setup as the stock driver. The whole chip is one FAT16 volume with
//! 4096-byte sectors at offset 0, so block N = flash bytes N * 4096.., which is
//! also the erase unit (SE4K): a block write is erase + 16 page programs.
//!
//! A write first compares against the old contents: unchanged blocks are not
//! touched, and blocks that only need 1 -> 0 bits are programmed without the
//! erase (45..400 ms, the main loop stalls meanwhile).

use crate::mmio::{modify32, r32, w16, w32};
use crate::msc::BlockDevice;
use crate::wdt;

const RCU_AHB1EN: u32 = 0x4002_3830;
const RCU_APB1EN: u32 = 0x4002_3840;
const GPIOC: u32 = 0x4002_0800;
const GPIOD: u32 = 0x4002_0C00;
const GPIOE: u32 = 0x4002_1000;
const MODER: u32 = 0x00;
const OTYPER: u32 = 0x04;
const OSPEED: u32 = 0x08;
const PUPD: u32 = 0x0C;
const BOP: u32 = 0x18;
const AFRL: u32 = 0x20;

const SPI: u32 = 0x4000_3800;
const CTL0: u32 = SPI + 0x00;
const STAT: u32 = SPI + 0x08;
const DATA: u32 = SPI + 0x0C;
const I2SCTL: u32 = SPI + 0x1C;
const STAT_RBNE: u32 = 1 << 0;
const STAT_TBE: u32 = 1 << 1;
/// Mode 3, master, software NSS, 8 bit, PSC /16 = 3.125 MHz (stock value).
const CTL0_CFG: u32 = 0x031F;
const CTL0_SPIEN: u32 = 1 << 6;

const CS: u32 = 4; // PE4

const CMD_PP: u8 = 0x02;
const CMD_READ: u8 = 0x03;
const CMD_RDSR1: u8 = 0x05;
const CMD_WREN: u8 = 0x06;
const CMD_SE4K: u8 = 0x20;
const CMD_JEDEC_ID: u8 = 0x9F;

const SECTOR: usize = 4096;
const PAGE: usize = 256;
/// RDSR polls before giving up (stock: 1e6, ~6 s at 3.125 MHz; tSE max 400 ms).
const BUSY_POLLS: u32 = 1_000_000;

pub struct SpiFlash {
    /// JEDEC ID (mfr << 16 | type << 8 | capacity), as read at init.
    pub id: u32,
    blocks: u32,
    /// Sticky: a busy wait timed out, later writes fail without trying.
    failed: bool,
}

fn af_pin(port: u32, pin: u32) {
    unsafe {
        modify32(port + AFRL, 0xF << (4 * pin), 5 << (4 * pin));
        modify32(port + OTYPER, 1 << pin, 0);
        modify32(port + OSPEED, 0, 3 << (2 * pin));
        modify32(port + PUPD, 3 << (2 * pin), 0);
        modify32(port + MODER, 3 << (2 * pin), 2 << (2 * pin));
    }
}

#[inline(always)]
fn cs(active: bool) {
    let bit = if active { 1 << (CS + 16) } else { 1 << CS };
    unsafe { w32(GPIOE + BOP, bit) };
}

#[inline(always)]
fn xfer(b: u8) -> u8 {
    unsafe {
        while r32(STAT) & STAT_TBE == 0 {}
        w16(DATA, b as u16);
        while r32(STAT) & STAT_RBNE == 0 {}
        r32(DATA) as u8
    }
}

fn cmd_addr(cmd: u8, addr: u32) {
    xfer(cmd);
    xfer((addr >> 16) as u8);
    xfer((addr >> 8) as u8);
    xfer(addr as u8);
}

fn read(addr: u32, buf: &mut [u8]) {
    cs(true);
    cmd_addr(CMD_READ, addr);
    for b in buf.iter_mut() {
        *b = xfer(0xFF);
    }
    cs(false);
}

impl SpiFlash {
    /// Set up the pins and SPI1 and identify the chip. A missing or unknown
    /// chip leaves the medium absent rather than showing 0xFF garbage.
    pub fn new() -> Self {
        unsafe {
            modify32(RCU_AHB1EN, 0, (1 << 2) | (1 << 3) | (1 << 4));
            modify32(RCU_APB1EN, 0, 1 << 14);
        }
        cs(false);
        unsafe {
            modify32(GPIOE + MODER, 3 << (2 * CS), 1 << (2 * CS));
            modify32(GPIOE + OTYPER, 1 << CS, 0);
            modify32(GPIOE + OSPEED, 0, 3 << (2 * CS));
        }
        af_pin(GPIOD, 3);
        af_pin(GPIOC, 2);
        af_pin(GPIOC, 3);
        unsafe {
            w32(CTL0, 0);
            modify32(I2SCTL, 1 << 11, 0);
            w32(CTL0, CTL0_CFG);
            w32(CTL0, CTL0_CFG | CTL0_SPIEN);
            // Drop a byte the stock may have left in the receive buffer.
            let _ = r32(DATA);
        }

        cs(true);
        xfer(CMD_JEDEC_ID);
        let id = (xfer(0xFF) as u32) << 16 | (xfer(0xFF) as u32) << 8 | xfer(0xFF) as u32;
        cs(false);
        let (mfr, cap) = (id >> 16, id & 0xFF);
        // 3-byte addressing covers up to 16 MiB (capacity code 0x18).
        let blocks = if mfr != 0 && mfr != 0xFF && (0x11..=0x18).contains(&cap) {
            (1u32 << cap) / SECTOR as u32
        } else {
            0
        };
        SpiFlash { id, blocks, failed: false }
    }

    /// A busy wait timed out (writes disabled until reset).
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// Wait for BUSY to clear, kicking the watchdog while the main loop does
    /// (an erase takes up to 400 ms, longer than the loop may stall).
    fn wait_ready(&mut self) -> bool {
        if self.failed {
            return false;
        }
        cs(true);
        xfer(CMD_RDSR1);
        let mut n = 0;
        while xfer(0xFF) & 1 != 0 {
            n += 1;
            if n & 0x3FF == 0 {
                wdt::kick_if_feeding();
            }
            if n >= BUSY_POLLS {
                self.failed = true;
                break;
            }
        }
        cs(false);
        !self.failed
    }

    fn write_enable() {
        cs(true);
        xfer(CMD_WREN);
        cs(false);
    }

    fn erase(&mut self, addr: u32) -> bool {
        Self::write_enable();
        cs(true);
        cmd_addr(CMD_SE4K, addr);
        cs(false);
        self.wait_ready()
    }

    fn program(&mut self, addr: u32, data: &[u8]) -> bool {
        Self::write_enable();
        cs(true);
        cmd_addr(CMD_PP, addr);
        for &b in data {
            xfer(b);
        }
        cs(false);
        self.wait_ready()
    }
}

impl BlockDevice for SpiFlash {
    fn present(&self) -> bool {
        self.blocks != 0
    }
    fn block_size(&self) -> u32 {
        SECTOR as u32
    }
    fn block_count(&self) -> u32 {
        self.blocks
    }

    fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> bool {
        if lba >= self.blocks || buf.len() < SECTOR {
            return false;
        }
        read(lba * SECTOR as u32, &mut buf[..SECTOR]);
        true
    }

    fn write_block(&mut self, lba: u32, buf: &[u8]) -> bool {
        if lba >= self.blocks || buf.len() < SECTOR || self.failed {
            return false;
        }
        let base = lba * SECTOR as u32;
        let data = &buf[..SECTOR];

        // Which pages differ, and can they be programmed over the old data?
        let mut dirty = 0u16;
        let mut need_erase = false;
        let mut old = [0u8; PAGE];
        for (p, new) in data.chunks(PAGE).enumerate() {
            read(base + (p * PAGE) as u32, &mut old);
            if old[..] != new[..] {
                dirty |= 1 << p;
                need_erase |= old.iter().zip(new).any(|(&o, &n)| o & n != n);
            }
        }
        if dirty == 0 {
            return true;
        }
        if need_erase {
            if !self.erase(base) {
                return false;
            }
            // Everything is 0xFF now: program every page that is not.
            dirty = 0;
            for (p, new) in data.chunks(PAGE).enumerate() {
                if new.iter().any(|&b| b != 0xFF) {
                    dirty |= 1 << p;
                }
            }
        }
        for (p, new) in data.chunks(PAGE).enumerate() {
            if dirty & (1 << p) != 0 && !self.program(base + (p * PAGE) as u32, new) {
                return false;
            }
        }
        // Read back.
        for (p, new) in data.chunks(PAGE).enumerate() {
            read(base + (p * PAGE) as u32, &mut old);
            if old[..] != new[..] {
                return false;
            }
        }
        true
    }
}
