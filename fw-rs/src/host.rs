//! Host link over the USB CDC-ACM port: keepalive, screenshots, LCD tests.
//!
//! Request (16 bytes, little endian): `D4 0D cmd 00 a:u32 b:u32 c:u32`.
//! Response: `D4 0D cmd status len:u32` followed by `len` bytes.
//!
//!   0 PING      a = keepalive ms (0 = default). Data: uptime_ms, flags (bit0
//!               PLLSAI locked, bit1 USB core idle), keepalive ms left (before
//!               this request), USBFS CID register.
//!   1 FILL      a = x | y<<16, b = w | h<<16, c = RGB565 colour.
//!   2 LCD_READ  a = x | y<<16, b = w | h<<16, c = number of u16 words to read
//!               after 0x2E (dummy word included); streams the raw words.
//!   3 LCD_REG   a = command, b = number of u16 words to read back (<= 32).
//!   4 RELEASE   a = 0: stop feeding the watchdog (reset to stock in ~20 s),
//!               a = 1: system reset right after the reply.
//!   5 DIAG      a = block id; returns a diagnostic block from the main loop
//!               (0 = touch: id[4], found, events u32, last raw frame,
//!               USBTMC control request log at 80..97).
//!   6 INPUT     inject a gesture for UI testing: a = 0 tap at b = x | y<<16,
//!               1 swipe left, 2 swipe right, 3 key down code b.
//!
//! Any valid request extends the watchdog keepalive.

use usb_device::bus::UsbBus;
use usbd_serial::SerialPort;

use crate::lcd;

const MAGIC: [u8; 2] = [0xD4, 0x0D];
const REQ_LEN: usize = 16;
const STAGE: usize = 512;

pub const CMD_PING: u8 = 0;
pub const CMD_FILL: u8 = 1;
pub const CMD_LCD_READ: u8 = 2;
pub const CMD_LCD_REG: u8 = 3;
pub const CMD_RELEASE: u8 = 4;
pub const CMD_DIAG: u8 = 5;
pub const CMD_INPUT: u8 = 6;
pub const DIAG_LEN: usize = 116;

const ST_OK: u8 = 0;
const ST_BAD_CMD: u8 = 1;
const ST_BAD_ARG: u8 = 2;

/// What the main loop must do after a request.
pub enum Action {
    None,
    Keepalive(u32),
    StopFeeding,
    /// Injected gesture (CMD_INPUT): kind, argument.
    Input(u32, u32),
    /// A text line (SCPI over the serial port); see `line()`.
    Scpi,
}

pub struct Info {
    pub uptime_ms: u32,
    pub flags: u32,
    pub keepalive_left_ms: u32,
    /// DIAG block 0 (touch).
    pub diag: [u8; DIAG_LEN],
}

enum Source {
    Idle,
    /// Raw GRAM words still to be read after RAMRD.
    Lcd(u32),
}

pub struct Host {
    req: [u8; REQ_LEN],
    req_len: usize,
    stage: [u8; STAGE],
    stage_len: usize,
    stage_off: usize,
    src: Source,
    pending_reset: bool,
    lcd_read: bool,
    /// Text line being received (anything not starting with the magic).
    line: [u8; 256],
    line_n: usize,
    line_ready: bool,
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl Host {
    pub const fn new() -> Self {
        Host {
            req: [0; REQ_LEN],
            req_len: 0,
            stage: [0; STAGE],
            stage_len: 0,
            stage_off: 0,
            src: Source::Idle,
            pending_reset: false,
            lcd_read: false,
            line: [0; 256],
            line_n: 0,
            line_ready: false,
        }
    }

    /// True while a reply is still being sent. The caller must not draw on the
    /// LCD meanwhile: a GRAM read-back may be in progress.
    pub fn busy(&self) -> bool {
        self.stage_off < self.stage_len || !matches!(self.src, Source::Idle)
    }

    /// The received SCPI line (after Action::Scpi); clears it.
    pub fn take_line(&mut self, out: &mut [u8; 256]) -> usize {
        let n = self.line_n;
        out[..n].copy_from_slice(&self.line[..n]);
        self.line_n = 0;
        self.line_ready = false;
        n
    }

    /// Queue a text reply (SCPI) followed by LF.
    pub fn reply_text(&mut self, t: &[u8]) {
        let n = t.len().min(STAGE - 1 - self.stage_len);
        self.push(&t[..n]);
        self.push(b"\n");
    }

    /// True once after a GRAM read-back request. A screenshot is a series of
    /// them; the caller pauses drawing meanwhile so the image does not tear.
    pub fn take_lcd_read(&mut self) -> bool {
        core::mem::take(&mut self.lcd_read)
    }

    /// Reset requested and the reply fully handed to the USB stack.
    pub fn reset_due(&self) -> bool {
        self.pending_reset && !self.busy()
    }

    /// Move reply bytes to the port, then parse new requests when idle.
    pub fn poll<B: UsbBus>(
        &mut self,
        port: &mut SerialPort<'_, B>,
        info: impl FnOnce() -> Info,
    ) -> Action {
        self.pump(port);
        if self.busy() {
            return Action::None;
        }

        let mut buf = [0u8; 64];
        let n = match port.read(&mut buf[..REQ_LEN - self.req_len]) {
            Ok(n) => n,
            Err(_) => return Action::None,
        };
        for &b in &buf[..n] {
            // Text mode: a line of SCPI, terminated by LF.
            if self.req_len == 0 && (self.line_n > 0 || (b != MAGIC[0] && b >= 0x20)) {
                if b == b'\n' || b == b'\r' {
                    if self.line_n > 0 {
                        self.line_ready = true;
                        return Action::Scpi;
                    }
                } else if self.line_n < self.line.len() {
                    self.line[self.line_n] = b;
                    self.line_n += 1;
                }
                continue;
            }
            self.req[self.req_len] = b;
            self.req_len += 1;
            // Resync on the magic so stray bytes (e.g. a modem probe) are dropped.
            if self.req_len <= 2 && self.req[self.req_len - 1] != MAGIC[self.req_len - 1] {
                self.req_len = 0;
            }
        }
        if self.req_len < REQ_LEN {
            return Action::None;
        }
        self.req_len = 0;
        let action = self.handle(info);
        self.pump(port);
        action
    }

    fn handle(&mut self, info: impl FnOnce() -> Info) -> Action {
        let r = self.req;
        let cmd = r[2];
        let (a, b, c) = (u32_at(&r, 4), u32_at(&r, 8), u32_at(&r, 12));
        let (x, y) = (a as u16, (a >> 16) as u16);
        let (w, h) = (b as u16, (b >> 16) as u16);
        let rect_ok = w > 0 && h > 0 && x as u32 + w as u32 <= lcd::W as u32
            && y as u32 + h as u32 <= lcd::H as u32;

        match cmd {
            CMD_PING => {
                let i = info();
                self.header(cmd, ST_OK, 16);
                for v in [i.uptime_ms, i.flags, i.keepalive_left_ms, unsafe { crate::mmio::r32(crate::usb::USBFS + 0x3C) }] {
                    self.push(&v.to_le_bytes());
                }
                Action::Keepalive(a)
            }
            CMD_FILL => {
                if !rect_ok {
                    self.header(cmd, ST_BAD_ARG, 0);
                } else {
                    lcd::fill_rect(x, y, w, h, c as u16);
                    self.header(cmd, ST_OK, 0);
                }
                Action::Keepalive(0)
            }
            CMD_LCD_READ => {
                if !rect_ok || c == 0 {
                    self.header(cmd, ST_BAD_ARG, 0);
                } else {
                    lcd::set_window_cmd(x, y, x + w - 1, y + h - 1, lcd::CMD_RAMRD);
                    self.header(cmd, ST_OK, c * 2);
                    self.src = Source::Lcd(c);
                    self.lcd_read = true;
                }
                Action::Keepalive(0)
            }
            CMD_LCD_REG => {
                if b > 32 {
                    self.header(cmd, ST_BAD_ARG, 0);
                } else {
                    lcd::cmd(a as u16);
                    self.header(cmd, ST_OK, b * 2);
                    for _ in 0..b {
                        self.push(&lcd::read_data().to_le_bytes());
                    }
                }
                Action::Keepalive(0)
            }
            CMD_DIAG => {
                let i = info();
                if a == 0 {
                    self.header(cmd, ST_OK, DIAG_LEN as u32);
                    self.push(&i.diag);
                    Action::Keepalive(0)
                } else {
                    self.header(cmd, ST_BAD_ARG, 0);
                    Action::Keepalive(0)
                }
            }
            CMD_INPUT => {
                self.header(cmd, ST_OK, 0);
                Action::Input(a, b)
            }
            CMD_RELEASE => {
                self.header(cmd, ST_OK, 0);
                if a == 1 {
                    self.pending_reset = true;
                    Action::None
                } else {
                    Action::StopFeeding
                }
            }
            _ => {
                self.header(cmd, ST_BAD_CMD, 0);
                Action::Keepalive(0)
            }
        }
    }

    fn header(&mut self, cmd: u8, status: u8, len: u32) {
        self.push(&[MAGIC[0], MAGIC[1], cmd, status]);
        self.push(&len.to_le_bytes());
    }

    /// Append to the stage buffer; only used for short replies that fit.
    fn push(&mut self, d: &[u8]) {
        self.stage[self.stage_len..self.stage_len + d.len()].copy_from_slice(d);
        self.stage_len += d.len();
    }

    fn refill(&mut self) {
        self.stage_len = 0;
        self.stage_off = 0;
        if let Source::Lcd(left) = self.src {
            let n = left.min((STAGE / 2) as u32);
            for _ in 0..n {
                self.push(&lcd::read_data().to_le_bytes());
            }
            self.src = if left > n { Source::Lcd(left - n) } else { Source::Idle };
        }
    }

    fn pump<B: UsbBus>(&mut self, port: &mut SerialPort<'_, B>) {
        loop {
            if self.stage_off == self.stage_len {
                if matches!(self.src, Source::Idle) {
                    self.stage_len = 0;
                    self.stage_off = 0;
                    let _ = port.flush();
                    return;
                }
                self.refill();
            }
            match port.write(&self.stage[self.stage_off..self.stage_len]) {
                Ok(n) if n > 0 => self.stage_off += n,
                _ => return,
            }
        }
    }
}
