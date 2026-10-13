//! Bluetooth: transparent SPP module on UART4 (PA0 TX / PA1 RX, AF8, 115200
//! 8N1; read back from the stock setup). Reception by circular DMA (DMA0
//! channel 2, request 4) so nothing is lost while the main loop is busy.
//!
//! Frames (README.md "DM40 Multimeter Protocol"):
//!   request  AF <id 05 03 | FF FF> <cmd> <len> <data..> <crc>
//!   response DF 05 03 <cmd> <len> <data..> <crc>   (discovery: DF FF FF ..)
//!   crc makes the byte sum 0 mod 256.

use crate::mmio::{modify32, r32, w32};

const GPIOA: u32 = 0x4002_0000;
const RCU_APB1EN: u32 = 0x4002_3840;
const UART4: u32 = 0x4000_4C00;
const STAT: u32 = UART4 + 0x00;
const DATA: u32 = UART4 + 0x04;
const BAUD: u32 = UART4 + 0x08;
const CTL0: u32 = UART4 + 0x0C;
const CTL2: u32 = UART4 + 0x14;

const DMA0: u32 = 0x4002_6000;
const CH: u32 = 2;
const CH_CTL: u32 = DMA0 + 0x10 + 0x18 * CH;
const CH_CNT: u32 = DMA0 + 0x14 + 0x18 * CH;
const CH_PADDR: u32 = DMA0 + 0x18 + 0x18 * CH;
const CH_M0ADDR: u32 = DMA0 + 0x1C + 0x18 * CH;
const DMA_INTC0: u32 = DMA0 + 0x08;

const RX_LEN: usize = 512;

#[link_section = ".dmabuf"]
static mut RX: [u8; RX_LEN] = [0; RX_LEN];

pub const MAX_DATA: usize = 0xC2;

pub struct Frame {
    pub cmd: u8,
    pub len: usize,
    pub data: [u8; MAX_DATA],
    /// Request used the broadcast id FF FF.
    pub broadcast: bool,
}

pub struct Bt {
    rd: usize,
    buf: [u8; 5 + MAX_DATA + 1],
    n: usize,
}

impl Bt {
    pub fn new() -> Self {
        unsafe {
            modify32(RCU_APB1EN, 0, 1 << 19); // UART4
            // PA0 / PA1: AF8.
            modify32(GPIOA + 0x20, 0xFF, 0x88);
            modify32(GPIOA + 0x08, 0xF, 0xA);
            modify32(GPIOA + 0x00, 0xF, 0xA);

            w32(CTL0, 0);
            w32(BAUD, 434); // 50 MHz / 434 = 115200
            w32(CTL2, 1 << 6); // DENR: DMA receive

            // DMA0 channel 2, request 4 = UART4 RX, P->M, byte, MINC, circular.
            w32(CH_CTL, 0);
            while r32(CH_CTL) & 1 != 0 {}
            w32(DMA_INTC0, 0x3D << 16); // clear channel 2 flags
            w32(CH_PADDR, DATA);
            w32(CH_M0ADDR, core::ptr::addr_of!(RX) as u32);
            w32(CH_CNT, RX_LEN as u32);
            w32(CH_CTL, (4 << 25) | (1 << 10) | (1 << 8) | 1);

            // UEN, TEN, REN (no interrupts: we run with IRQs masked).
            w32(CTL0, (1 << 13) | (1 << 3) | (1 << 2));
        }
        Bt { rd: 0, buf: [0; 5 + MAX_DATA + 1], n: 0 }
    }

    fn rx_byte(&mut self) -> Option<u8> {
        let wr = RX_LEN - unsafe { r32(CH_CNT) } as usize;
        if self.rd == wr % RX_LEN {
            return None;
        }
        let b = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(RX[self.rd])) };
        self.rd = (self.rd + 1) % RX_LEN;
        Some(b)
    }

    /// Next complete, valid request frame, if any.
    pub fn poll(&mut self) -> Option<Frame> {
        while let Some(b) = self.rx_byte() {
            if self.n == 0 && b != 0xAF {
                continue;
            }
            self.buf[self.n] = b;
            self.n += 1;
            if self.n == 3 {
                let id = (self.buf[1], self.buf[2]);
                if id != (0x05, 0x03) && id != (0xFF, 0xFF) {
                    self.resync();
                    continue;
                }
            }
            if self.n == 5 && self.buf[4] as usize > MAX_DATA {
                self.resync();
                continue;
            }
            if self.n >= 5 && self.n == 5 + self.buf[4] as usize + 1 {
                let n = self.n;
                self.n = 0;
                let sum = self.buf[..n].iter().fold(0u8, |a, &x| a.wrapping_add(x));
                if sum != 0 {
                    continue;
                }
                let len = self.buf[4] as usize;
                let mut f = Frame { cmd: self.buf[3], len, data: [0; MAX_DATA], broadcast: self.buf[1] == 0xFF };
                f.data[..len].copy_from_slice(&self.buf[5..5 + len]);
                return Some(f);
            }
        }
        None
    }

    fn resync(&mut self) {
        // Restart at the next 0xAF inside what we buffered.
        let start = self.buf[1..self.n].iter().position(|&x| x == 0xAF).map(|i| i + 1);
        match start {
            Some(s) => {
                self.buf.copy_within(s..self.n, 0);
                self.n -= s;
            }
            None => self.n = 0,
        }
    }

    fn tx(&self, b: u8) {
        unsafe {
            while r32(STAT) & (1 << 7) == 0 {} // TBE
            w32(DATA, b as u32);
        }
    }

    /// Send a response frame (blocking, a few hundred us at 115200).
    pub fn reply(&self, cmd: u8, data: &[u8], broadcast: bool) {
        let hdr = if broadcast { [0xDF, 0xFF, 0xFF] } else { [0xDF, 0x05, 0x03] };
        let mut sum = 0u8;
        for &b in hdr.iter().chain(&[cmd, data.len() as u8]).chain(data) {
            sum = sum.wrapping_add(b);
            self.tx(b);
        }
        self.tx(0u8.wrapping_sub(sum));
    }

    /// Send a raw AT command to the module (name changes etc.).
    pub fn at(&self, cmd: &[u8]) {
        for &b in cmd {
            self.tx(b);
        }
    }
}
