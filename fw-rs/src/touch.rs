//! Capacitive touch panel: Hynitron CST2xx-type controller at 7-bit 0x5A on
//! the I2C3 pins (SCL = PA8, SDA = PC9), reset = PE6, INT = PE5 (falling edge,
//! pull-up). Protocol from firmware/re/analysis/touch_v2_6.h (the stock also
//! probes a Goodix GT1158 at 0x14 first; this unit has the 0x5A chip).
//!
//! Register addresses are 16-bit big-endian and always sent as their own write
//! followed by STOP, then a separate read (no repeated start).
//!   init:  write D1 01; write D2 04 + read 4 (byte 2 = chip type, 0xA8);
//!          write D1 09
//!   frame: write D0 00 + read 7; byte 6 must be 0xAB; n = byte5 & 7 points;
//!          extra points at D0 07 / 0C / 11 / 16 (5 bytes each); then write
//!          D0 00 AB to acknowledge.
//!   point: b0 low nibble 6 = pressed; X = b1 << 4 | b3 >> 4,
//!          Y = b2 << 4 | b3 & 0xF (landscape, 480x320).
//!   keys:  b0 bit 7 = key report; b0 = 0x83 while held, 0x80 on release
//!          (measured). Code from X (measured 372/628/884/1140): < 500 -> 0,
//!          < 750 -> 1, < 1000 -> 2, else 3.
//!
//! Bit-banged I2C with open-drain GPIO, polled from the main loop when INT is
//! low (to be moved to the I2C3 peripheral).

use crate::clock::{delay_us, Clock};
use crate::mmio::{modify32, r32, w32};

const GPIOA: u32 = 0x4002_0000;
const GPIOC: u32 = 0x4002_0800;
const GPIOE: u32 = 0x4002_1000;
const SCL: u32 = 8; // PA8
const SDA: u32 = 9; // PC9
const RST: u32 = 6; // PE6
const INT: u32 = 5; // PE5

const T: u32 = 4; // half bit, us (~100 kHz)
const ADDR: u8 = 0x5A;

fn od_pin(port: u32, pin: u32) {
    unsafe {
        w32(port + 0x18, 1 << pin); // released (high)
        modify32(port + 0x04, 0, 1 << pin); // open drain
        modify32(port + 0x0C, 3 << (2 * pin), 1 << (2 * pin)); // pull-up
        modify32(port + 0x00, 3 << (2 * pin), 1 << (2 * pin)); // output
    }
}

#[inline(always)]
fn scl(high: bool) {
    unsafe { w32(GPIOA + 0x18, if high { 1 << SCL } else { 1 << (SCL + 16) }) };
}

#[inline(always)]
fn sda(high: bool) {
    unsafe { w32(GPIOC + 0x18, if high { 1 << SDA } else { 1 << (SDA + 16) }) };
}

#[inline(always)]
fn sda_in() -> bool {
    (unsafe { r32(GPIOC + 0x10) } & (1 << SDA)) != 0
}

#[inline(always)]
fn scl_in() -> bool {
    (unsafe { r32(GPIOA + 0x10) } & (1 << SCL)) != 0
}

fn scl_release() {
    scl(true);
    let mut n = 0;
    while !scl_in() && n < 1000 {
        delay_us(1);
        n += 1;
    }
}

fn start() {
    sda(true);
    scl_release();
    delay_us(T);
    sda(false);
    delay_us(T);
    scl(false);
    delay_us(T);
}

fn stop() {
    sda(false);
    delay_us(T);
    scl_release();
    delay_us(T);
    sda(true);
    delay_us(T);
}

fn tx(b: u8) -> bool {
    for i in (0..8).rev() {
        sda(b & (1 << i) != 0);
        delay_us(T);
        scl_release();
        delay_us(T);
        scl(false);
    }
    sda(true);
    delay_us(T);
    scl_release();
    delay_us(T);
    let ack = !sda_in();
    scl(false);
    delay_us(T);
    ack
}

fn rx(ack: bool) -> u8 {
    let mut b = 0u8;
    sda(true);
    for _ in 0..8 {
        delay_us(T);
        scl_release();
        delay_us(T);
        b = (b << 1) | sda_in() as u8;
        scl(false);
    }
    sda(!ack);
    delay_us(T);
    scl_release();
    delay_us(T);
    scl(false);
    sda(true);
    delay_us(T);
    b
}

fn write(data: &[u8]) -> bool {
    start();
    let mut ok = tx(ADDR << 1);
    for &b in data {
        ok = ok && tx(b);
    }
    stop();
    ok
}

fn read(buf: &mut [u8]) -> bool {
    start();
    if !tx((ADDR << 1) | 1) {
        stop();
        return false;
    }
    let n = buf.len();
    for (i, b) in buf.iter_mut().enumerate() {
        *b = rx(i + 1 < n);
    }
    stop();
    true
}

fn read_reg(reg: u16, buf: &mut [u8]) -> bool {
    write(&reg.to_be_bytes()) && read(buf)
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Point {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Event {
    /// Number of pressed points (0 = released).
    pub points: u8,
    pub p: Point,
    /// Side key held: 1 << code (code 0..3 from the key report), 0 = none.
    pub keys: u8,
}

pub struct Touch {
    pub chip: [u8; 4],
    /// Raw first 7 bytes of the last frame (for debugging).
    pub raw: [u8; 7],
    /// Debug ring of the last 8 frames (7 bytes each) and its next index.
    pub ring: [[u8; 7]; 8],
    pub ring_i: u8,
    last: Event,
    /// Debounce: a new state is accepted after two identical frames, or when
    /// it has held for DEBOUNCE_MS without further frames (a single release
    /// frame followed by silence).
    pending: Option<(Event, u64)>,
    /// Last time a screen point was reported: key reports during a touch
    /// or right after it are the controller's own long-press gesture, not a
    /// side key, and are dropped.
    last_point: u64,
}

const DEBOUNCE_MS: u64 = 40;

fn int_low() -> bool {
    (unsafe { r32(GPIOE + 0x10) } & (1 << INT)) == 0
}

fn decode(b: &[u8]) -> (bool, bool, Point) {
    let key = b[0] & 0x80 != 0;
    let pressed = if key { b[0] & 0x0F != 0 } else { b[0] & 0x0F == 6 };
    let x = (b[1] as u16) << 4 | (b[3] >> 4) as u16;
    let y = (b[2] as u16) << 4 | (b[3] & 0x0F) as u16;
    (pressed, key, Point { x, y })
}

impl Touch {
    /// Reset and initialise the controller. None if it does not answer.
    pub fn new(clk: &mut Clock) -> Option<Self> {
        od_pin(GPIOA, SCL);
        od_pin(GPIOC, SDA);
        unsafe {
            // INT: input, pull-up. RST: push-pull output, low.
            modify32(GPIOE + 0x00, 3 << (2 * INT), 0);
            modify32(GPIOE + 0x0C, 3 << (2 * INT), 1 << (2 * INT));
            modify32(GPIOE + 0x04, 1 << RST, 0);
            w32(GPIOE + 0x18, 1 << (RST + 16));
            modify32(GPIOE + 0x00, 3 << (2 * RST), 1 << (2 * RST));
        }
        // Free a possibly stuck bus.
        for _ in 0..9 {
            if sda_in() {
                break;
            }
            scl(false);
            delay_us(T);
            scl_release();
            delay_us(T);
        }
        stop();

        clk.delay_ms(20);
        unsafe { w32(GPIOE + 0x18, 1 << RST) };
        clk.delay_ms(200);

        let mut chip = [0u8; 4];
        let ok = write(&[0xD1, 0x01]) && read_reg(0xD204, &mut chip) && write(&[0xD1, 0x09]);
        if !ok {
            return None;
        }
        Some(Touch { chip, raw: [0; 7], ring: [[0; 7]; 8], ring_i: 0, last: Event::default(), pending: None, last_point: 0 })
    }

    /// Poll: reads a frame when INT is low. Returns the new (debounced) state
    /// on a change.
    pub fn poll(&mut self, now: u64) -> Option<Event> {
        if !int_low() {
            if let Some((ev, t)) = self.pending {
                if now.saturating_sub(t) >= DEBOUNCE_MS {
                    self.pending = None;
                    if ev != self.last {
                        self.last = ev;
                        return Some(ev);
                    }
                }
            }
            return None;
        }
        let mut b = [0u8; 7];
        if !read_reg(0xD000, &mut b) || b[6] != 0xAB {
            return None;
        }
        self.raw = b;
        self.ring[(self.ring_i & 7) as usize] = b;
        self.ring_i = self.ring_i.wrapping_add(1);
        let n = b[5] & 7;
        let (pressed, key, p) = decode(&b);
        // Extra points are read (and dropped) so the controller sees the
        // complete transaction, as the stock driver does.
        for i in 1..n.min(5) {
            let mut e = [0u8; 5];
            read_reg(0xD000 + 7 + 5 * (i as u16 - 1), &mut e);
        }
        write(&[0xD0, 0x00, 0xAB]);

        let mut ev = Event::default();
        if pressed && !key {
            self.last_point = now;
        }
        if pressed && key && (self.last.points > 0 || now.saturating_sub(self.last_point) < 300) {
            // Long press on the screen reported as a key: ignore.
            return None;
        }
        if pressed {
            if key {
                let code = match p.x {
                    0..=499 => 0,
                    500..=749 => 1,
                    750..=999 => 2,
                    _ => 3,
                };
                ev.keys = 1 << code;
            } else {
                ev.points = n.max(1);
                ev.p = p;
            }
        }
        // Moving finger: accept position updates of the same contact at once.
        if ev.points > 0 && self.last.points > 0 && ev.keys == self.last.keys {
            self.pending = None;
            if ev != self.last {
                self.last = ev;
                return Some(ev);
            }
            return None;
        }
        if ev == self.last {
            self.pending = None;
            return None;
        }
        match self.pending {
            Some((p, _)) if p == ev => {
                self.pending = None;
                self.last = ev;
                Some(ev)
            }
            _ => {
                self.pending = Some((ev, now));
                None
            }
        }
    }
}
