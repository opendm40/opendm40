//! Bluetooth: transparent SPP module on UART4 (PA0 TX / PA1 RX, AF8, 115200
//! 8N1; read back from the stock setup). Reception by circular DMA (DMA0
//! channel 2, request 4) so nothing is lost while the main loop is busy.
//!
//! Frames (README.md "DM40 Multimeter Protocol"):
//!   request  AF <id 05 03 | FF FF> <cmd> <len> <data..> <crc>
//!   response DF 05 03 <cmd> <len> <data..> <crc>   (discovery: DF FF FF ..)
//!   crc makes the byte sum 0 mod 256.

use crate::clock::Clock;
use crate::dmm::{Dmm, Val};
use crate::meter::Meter;
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

/// Bootloader data block in internal flash (read only): version bytes.
const BOOT_DATA: u32 = 0x080A_4000;

/// Unit prefix step (x1000) relative to the base unit of the mode group
/// (V group: mV, A group: uA, ohm, nF, Hz; diode V, duty %, temperature C/F).
fn prefix(mode: u8, unit: &str) -> u8 {
    match (mode, unit) {
        (0..=3, "V") => 1,
        (4..=9, "mA") => 1,
        (4..=9, "A") => 2,
        (_, u) if u.starts_with('k') => 1,
        (_, u) if u.starts_with('M') => 2,
        (13, "\u{b5}F") => 1,
        (13, "mF") => 2,
        _ => 0,
    }
}

/// Format byte + 16-bit value (bt_meas_v2_6.h section 3). OL -> 0xFFFF.
fn encode(mode: u8, v: Option<&Val>, dec_override: Option<u8>) -> (u8, u16) {
    let Some(v) = v else { return (0, 0xFFFF) };
    let dec = dec_override.unwrap_or(v.dec).min(4);
    let mut fmt = prefix(mode, v.unit) << 4 | dec << 1;
    if v.ol {
        return (fmt, 0xFFFF);
    }
    if v.v < 0.0 {
        fmt |= 1;
    }
    let mut k = 1.0f32;
    for _ in 0..dec {
        k *= 10.0;
    }
    let mut n = ((if v.v < 0.0 { -v.v } else { v.v }) * k + 0.5) as u32;
    if mode == 11 && n > 0xFFF0 {
        n -= 0xFFF0;
        fmt |= 0x80;
    }
    (fmt, n as u16)
}

impl Bt {
    /// Collect an AT answer from the module for up to `ms` (blocking).
    fn at_answer(&mut self, clk: &mut Clock, ms: u32, out: &mut [u8; 64]) -> usize {
        let end = clk.now_ms() + ms as u64;
        let mut n = 0;
        while clk.now_ms() < end {
            if let Some(b) = self.rx_byte() {
                if n < out.len() {
                    out[n] = b;
                    n += 1;
                }
            }
        }
        n
    }

    /// Serve one request frame (stock command set 00..09).
    pub fn serve(&mut self, f: &Frame, clk: &mut Clock, dmm: &mut Dmm, meter: &mut Meter, raw: Option<&crate::dmm::Reading>) {
        const LENS: [usize; 10] = [0, 0, 0, 1, 1, 1, 1, 20, 0, 0];
        if f.cmd > 9 {
            return;
        }
        let st = (f.len != LENS[f.cmd as usize]) as u8;
        match f.cmd {
            0x00 => self.reply(0, &[0x05, 0x03, b'A' + dmm.model()], true),
            0x01 => self.reply(1, &[st], false),
            0x02 => {
                let b = |o: u32| unsafe { core::ptr::read_volatile((BOOT_DATA + o) as *const u8) };
                self.reply(2, &[b(0x10).wrapping_mul(10), 26, b(0x11).wrapping_mul(10).wrapping_add(b(0x12))], false);
            }
            0x03..=0x06 => {
                self.reply(f.cmd, &[st], false);
                let d = if f.len > 0 { f.data[0] } else { 0 };
                match f.cmd {
                    0x03 => {
                        meter.reset();
                        dmm.hold_range(false);
                    }
                    0x04 => meter.set_hold(d != 0),
                    0x05 => {
                        if d == 0 {
                            meter.rel_off();
                            dmm.hold_range(false);
                        } else if meter.rel_on(raw) && dmm.mode() <= 10 || dmm.mode() == 14 {
                            dmm.hold_range(true);
                        }
                    }
                    _ => {
                        if dmm.apply_mode_byte(clk, d) {
                            meter.reset();
                        }
                    }
                }
            }
            0x07 => {
                let name_len = f.data[..f.len.min(20)].iter().position(|&b| b == 0).unwrap_or(f.len.min(20));
                self.at(b"AT+SBN");
                self.at(&f.data[..name_len]);
                self.at(b"\r\n");
                let mut a = [0u8; 64];
                let n = self.at_answer(clk, 300, &mut a);
                let ok = a[..n].windows(2).any(|w| w == b"OK");
                self.reply(7, &[!ok as u8], false);
            }
            0x08 => {
                self.at(b"AT+CBN\r\n");
                let mut a = [0u8; 64];
                let n = self.at_answer(clk, 300, &mut a);
                let mut name = [0u8; 20];
                if let Some(p) = a[..n].windows(4).position(|w| w == b"CBN+") {
                    let rest = &a[p + 4..n];
                    let e = rest.iter().position(|&c| c == b'\r' || c == b'\n').unwrap_or(rest.len()).min(19);
                    name[..e].copy_from_slice(&rest[..e]);
                }
                self.reply(8, &name, false);
            }
            _ => {
                let mut d = [0u8; 11];
                let mode = dmm.mode();
                d[0] = dmm.mode_byte();
                d[1] = (meter.hold as u8) << 7 | (meter.rel_active() as u8) << 5 | 5;
                let shown = meter.shown();
                let main = shown.map(|r| r.main);
                let (fm, vm) = encode(mode, main.as_ref(), None);
                let a1 = shown.and_then(|r| r.aux1);
                let a2 = shown.and_then(|r| r.aux2);
                let d3 = match mode {
                    8 => Some(2),
                    12 => Some(1),
                    _ => None,
                };
                let (f2, v2) = encode(mode, a1.as_ref(), None);
                let (f3, v3) = encode(mode, a2.as_ref(), d3);
                d[2] = if a1.is_some() { f2 } else { 0 };
                d[3] = if a2.is_some() { f3 } else { 0 };
                d[4] = fm;
                d[5..7].copy_from_slice(&(if a1.is_some() { v2 } else { 0xFFFF }).to_le_bytes());
                d[7..9].copy_from_slice(&(if a2.is_some() { v3 } else { 0xFFFF }).to_le_bytes());
                d[9..11].copy_from_slice(&vm.to_le_bytes());
                self.reply(9, &d, false);
            }
        }
    }
}
