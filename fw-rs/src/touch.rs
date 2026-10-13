//! Capacitive touch panel on the I2C3 pins (SCL = PA8, SDA = PC9), reset = PE6,
//! INT = PE5 (falling edge, pull-up). Two controllers are supported, selected at
//! init by probing in the stock order (firmware/re/analysis/touch_v2_6.h):
//!   1. Goodix GT1158 at 7-bit 0x14 (8-bit 0x28) — probed first (header §Goodix,
//!      lines 8/110-118/140). Absent on this unit; the probe NAKs and we fall
//!      back to 0x5A, so the 0x5A bus traffic below is byte-identical to before.
//!   2. Hynitron CST2xx-type at 7-bit 0x5A (8-bit 0xB4) — the chip on this unit.
//! The detected controller is stored in `Ctrl`; `poll()` dispatches on it so the
//! coordinate + side-key output to the rest of the firmware is identical either
//! way (gesture.rs / app.rs see the same `Event`).
//!
//! Register addresses are 16-bit big-endian on both chips and always sent as
//! their own write followed by STOP, then a separate read (no repeated start).
//!
//! Hynitron 0x5A (header §INIT/§FRAME, lines 41-68):
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
//! Goodix GT1158 (header §Goodix, lines 98/110-118; correct-by-construction,
//! not testable on this unit which has the 0x5A chip):
//!   id:    read 0x8140 (4 bytes) == ASCII "1158"
//!   init:  write 0x8040 = 0x02; wait 10 ms; write 0x8040 = 0x00 (normal report)
//!   frame: read 0x814E (1 byte status): bit7 = buffer ready, bit4 = HaveKey,
//!          bits 3..0 = point count. Only act when bit7 set; then clear the
//!          buffer with a write 0x814E = 0x00 (stock order: status, clear, then
//!          point/key reads — header line 112). Point i coords are 4 bytes at
//!          0x8150 + 8*i = X lo, X hi, Y lo, Y hi. The GT1158 reports native
//!          portrait (320x480); the stock swaps into the landscape GUI frame with
//!          NO scale/mirror (header 115-116): GUI x = Goodix Y, GUI y = Goodix X.
//!   keys:  key byte (value 1/2/4/8 -> code 0..3) at 0x814F + 8*n (header 98/113).
//!   Because this is a polled driver (no EXTI) and the GT1158 INT behaviour is
//!   unknown, the Goodix path also force-polls the buffer-ready flag every
//!   ~10 ms so a short INT pulse missed during a long loop iteration cannot leave
//!   touch stuck; the 0x5A path stays purely INT-level driven (unchanged).
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
const ADDR: u8 = 0x5A; // Hynitron, this unit
const ADDR_GOODIX: u8 = 0x14; // GT1158 (8-bit 0x28), header line 140

// Goodix GT1158 16-bit big-endian registers (header §Goodix, lines 110-118).
const GT_REG_PRODUCT_ID: u16 = 0x8140; // read 4 == "1158"
const GT_REG_COMMAND: u16 = 0x8040; // init: 0x02 then 0x00
const GT_REG_STATUS: u16 = 0x814E; // bit7 ready, bit4 HaveKey, bits3..0 count
const GT_REG_POINT0: u16 = 0x8150; // point i coords (4 bytes) at +8*i
const GT_REG_KEY0: u16 = 0x814F; // key byte at +8*n (point stride 8)
const GT_ID: [u8; 4] = *b"1158";
/// Force a Goodix buffer-ready read at least this often even without an INT
/// edge (the GT1158 INT behaviour is unknown and the loop can stall on a flash
/// write; the 0x5A path does NOT use this).
const GT_POLL_MS: u64 = 10;

/// Which touch controller was detected at init.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctrl {
    /// Hynitron CST2xx-type at 0x5A (this unit).
    Hynitron,
    /// Goodix GT1158 at 0x14 (fallback for panels without the 0x5A chip).
    Goodix,
}

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

fn write_at(addr: u8, data: &[u8]) -> bool {
    start();
    let mut ok = tx(addr << 1);
    for &b in data {
        ok = ok && tx(b);
    }
    stop();
    ok
}

fn read_at(addr: u8, buf: &mut [u8]) -> bool {
    start();
    if !tx((addr << 1) | 1) {
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

fn read_reg_at(addr: u8, reg: u16, buf: &mut [u8]) -> bool {
    write_at(addr, &reg.to_be_bytes()) && read_at(addr, buf)
}

// 0x5A (Hynitron) wrappers — unchanged behaviour (addr fixed to ADDR = 0x5A).
fn write(data: &[u8]) -> bool {
    write_at(ADDR, data)
}

fn read(buf: &mut [u8]) -> bool {
    read_at(ADDR, buf)
}

fn read_reg(reg: u16, buf: &mut [u8]) -> bool {
    read_reg_at(ADDR, reg, buf)
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Point {
    pub x: u16,
    pub y: u16,
}

/// Most points one frame carries (both controllers report up to 5).
pub const MAX_POINTS: usize = 5;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Event {
    /// Number of pressed points (0 = released), at most MAX_POINTS.
    pub points: u8,
    /// First pressed point (= pts[0]).
    pub p: Point,
    /// Pressed points in the controller's slot order: pts[..points]; the rest
    /// are zero (Event equality drives the debounce).
    pub pts: [Point; MAX_POINTS],
    /// Side key held: 1 << code (code 0..3 from the key report), 0 = none.
    pub keys: u8,
}

/// One decoded controller frame, identical for both chips.
struct Frame {
    pressed: bool,
    key: bool,
    /// Pressed screen points (pts[..n]).
    n: u8,
    pts: [Point; MAX_POINTS],
    /// Side-key code 0..3 (valid when `key`).
    code: u8,
}

pub struct Touch {
    /// Detected controller; `poll()` dispatches on it.
    ctrl: Ctrl,
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
    /// Goodix only: last time the buffer-ready flag was force-polled (see
    /// GT_POLL_MS). Unused on the Hynitron path.
    last_gt_poll: u64,
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

        // Probe the Goodix GT1158 first (stock order, header lines 8/110-118).
        // On this unit it NAKs at 0x14, so read_reg_at returns false and we fall
        // through to the 0x5A chip; the 0x5A traffic below is unchanged.
        let mut id = [0u8; 4];
        if read_reg_at(ADDR_GOODIX, GT_REG_PRODUCT_ID, &mut id) && id == GT_ID {
            // GT1158 config: command 0x02, settle, then 0x00 = normal reporting
            // (header line 111). Clear any stale buffer so the first INT is fresh.
            // The PE5 pull-up is left enabled (electrically safe); the stock drops
            // it after a Goodix probe (header 34-35) but that is not required here.
            write_at(ADDR_GOODIX, &[(GT_REG_COMMAND >> 8) as u8, GT_REG_COMMAND as u8, 0x02]);
            clk.delay_ms(10);
            write_at(ADDR_GOODIX, &[(GT_REG_COMMAND >> 8) as u8, GT_REG_COMMAND as u8, 0x00]);
            write_at(ADDR_GOODIX, &[(GT_REG_STATUS >> 8) as u8, GT_REG_STATUS as u8, 0x00]);
            return Some(Touch {
                ctrl: Ctrl::Goodix,
                chip: id, // "1158"
                raw: [0; 7],
                ring: [[0; 7]; 8],
                ring_i: 0,
                last: Event::default(),
                pending: None,
                last_point: 0,
                last_gt_poll: 0,
            });
        }

        // Hynitron 0x5A (this unit): unchanged init. ACK-only identify.
        let mut chip = [0u8; 4];
        let ok = write(&[0xD1, 0x01]) && read_reg(0xD204, &mut chip) && write(&[0xD1, 0x09]);
        if !ok {
            return None;
        }
        Some(Touch { ctrl: Ctrl::Hynitron, chip, raw: [0; 7], ring: [[0; 7]; 8], ring_i: 0, last: Event::default(), pending: None, last_point: 0, last_gt_poll: 0 })
    }

    /// Poll: reads a frame when due, dispatches to the detected controller, and
    /// returns the new (debounced) state on a change. The decoded `Frame`
    /// (pressed, key, the pressed points, key code) is identical for both chips,
    /// so the debounce tail below and the rest of the firmware never see the
    /// panel. A change of the point count or of any point while the screen
    /// stays touched is reported at once (no debounce), like a moving finger.
    pub fn poll(&mut self, now: u64) -> Option<Event> {
        // Hynitron is driven purely by INT level, exactly as before. Goodix is
        // additionally force-polled on a timer: its INT behaviour is unknown and
        // the main loop can stall (flash writes), so a short INT pulse must not
        // be able to leave touch stuck. The `ctrl == Goodix` guard short-circuits
        // on this 0x5A unit, so the 0x5A timing is unchanged.
        let due = int_low()
            || (self.ctrl == Ctrl::Goodix && now.saturating_sub(self.last_gt_poll) >= GT_POLL_MS);

        let frame = if due {
            match self.ctrl {
                // A bad/absent 0x5A frame returns without touching pending, as
                // the original inline path did.
                Ctrl::Hynitron => match self.read_hyn() {
                    Some(f) => Some(f),
                    None => return None,
                },
                // A "no new data" Goodix read (buffer-ready clear) returns None
                // and falls through to the debounce flush below.
                Ctrl::Goodix => {
                    self.last_gt_poll = now;
                    self.read_goodix()
                }
            }
        } else {
            None
        };

        let Frame { pressed, key, n, pts, code } = match frame {
            Some(f) => f,
            None => {
                // No new frame this call: flush a pending debounced state after
                // DEBOUNCE_MS of silence (same logic as the old !int_low path).
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
        };

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
                ev.keys = 1 << code;
            } else {
                ev.points = n.clamp(1, MAX_POINTS as u8);
                ev.pts = pts;
                ev.p = pts[0];
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
            Some((q, _)) if q == ev => {
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

    /// Hynitron 0x5A frame read — byte-identical bus traffic to the original
    /// inline path (D0 00 read 7, 0xAB check, extra points D0 07/0C/11/16, then
    /// the D0 00 AB ack). Multi-touch (touch_v2_6.h FRAME READ / DRIVER OUTPUT):
    /// rx[5] & 7 = n records; record 0 is in the 7-byte frame, records 1..n-1
    /// (5 bytes each, same layout) follow. A record is a pressed point when its
    /// status low nibble is 6; released records are dropped and the pressed ones
    /// packed in slot order (as the stock queue entry: "n, minus 1 for each
    /// released record"). `code` is the side-key code derived from the X of a
    /// key report (record 0 bit 7). None (bad/absent frame) means "no frame"
    /// and is handled without touching pending, as before.
    fn read_hyn(&mut self) -> Option<Frame> {
        let mut b = [0u8; 7];
        if !read_reg(0xD000, &mut b) || b[6] != 0xAB {
            return None;
        }
        self.raw = b;
        self.ring[(self.ring_i & 7) as usize] = b;
        self.ring_i = self.ring_i.wrapping_add(1);
        let n = b[5] & 7;
        let (pressed0, key, p) = decode(&b);
        let mut f = Frame { pressed: pressed0, key, n: 0, pts: [Point::default(); MAX_POINTS], code: 0 };
        if pressed0 && !key {
            f.pts[0] = p;
            f.n = 1;
        }
        // Extra points: always read, so the controller sees the complete
        // transaction, as the stock driver does.
        for i in 1..n.min(MAX_POINTS as u8) {
            let mut e = [0u8; 5];
            if read_reg(0xD000 + 7 + 5 * (i as u16 - 1), &mut e) && !key {
                let (pr, k, q) = decode(&e);
                if pr && !k && (f.n as usize) < MAX_POINTS {
                    f.pts[f.n as usize] = q;
                    f.n += 1;
                }
            }
        }
        write(&[0xD0, 0x00, 0xAB]);
        if !key {
            f.pressed = f.n > 0;
        }
        f.code = match p.x {
            0..=499 => 0,
            500..=749 => 1,
            750..=999 => 2,
            _ => 3,
        };
        Some(f)
    }

    /// Goodix GT1158 frame read (header §Goodix, lines 98/110-118). Correct-by-
    /// construction from the stock reverse-engineering; it cannot be exercised on
    /// this 0x5A unit. Normalises Goodix coordinates and keys into the same tuple
    /// the shared debounce path (and therefore gesture.rs / app.rs) expect, so no
    /// other file changes. None = no new data (buffer-ready bit clear); the buffer
    /// is NOT acked in that case (header 112 clears only a ready buffer) and the
    /// force-poll timer re-checks, so there is no hang.
    fn read_goodix(&mut self) -> Option<Frame> {
        let mut st = [0u8; 1];
        if !read_reg_at(ADDR_GOODIX, GT_REG_STATUS, &mut st) {
            return None;
        }
        let status = st[0];
        if status & 0x80 == 0 {
            return None; // buffer not ready: nothing new, nothing to clear
        }
        let n = status & 0x0F; // bits 3..0 = point count
        let have_key = status & 0x10 != 0; // bit 4 = HaveKey

        // Stock order (header line 112): read status, clear the buffer, then read
        // the point/key records.
        write_at(ADDR_GOODIX, &[(GT_REG_STATUS >> 8) as u8, GT_REG_STATUS as u8, 0x00]);

        let mut pressed = false;
        let mut pts = [Point::default(); MAX_POINTS];
        let mut np = 0u8;
        // Every reported point i: 4 bytes at 0x8150 + 8*i = X lo, X hi, Y lo,
        // Y hi (header line 113; the stock reads them all, table 0x0804976c).
        for i in 0..n.min(MAX_POINTS as u8) {
            let mut pt = [0u8; 4];
            if read_reg_at(ADDR_GOODIX, GT_REG_POINT0 + 8 * i as u16, &mut pt) {
                let gx = (pt[1] as u16) << 8 | pt[0] as u16; // Goodix native X
                let gy = (pt[3] as u16) << 8 | pt[2] as u16; // Goodix native Y
                // Native portrait (320x480) -> landscape GUI, no scale/mirror
                // (header 115-116): GUI x = Goodix Y, GUI y = Goodix X.
                pts[np as usize] = Point { x: gy, y: gx };
                np += 1;
                pressed = true;
            }
        }
        let p = pts[0];

        let mut key = false;
        let mut code = 0u8;
        if have_key {
            // Key byte at 0x814F + 8*n (header 98/113): one-hot 1/2/4/8 -> code
            // 0..3. Mask to the 4 valid bits so a spurious value cannot shift the
            // event bit past bit 3.
            let mut kb = [0u8; 1];
            if read_reg_at(ADDR_GOODIX, GT_REG_KEY0 + 8 * n as u16, &mut kb) {
                let kv = kb[0] & 0x0F;
                if kv != 0 {
                    code = kv.trailing_zeros() as u8;
                    key = true;
                    pressed = true;
                }
            }
        }

        // Debug mirror for DBG:DIAG: [status, GUIx lo, GUIx hi, GUIy lo, GUIy hi,
        // key|0x80, 0] — the Goodix format has no 7-byte frame of its own.
        let rawf = [
            status,
            (p.x & 0xFF) as u8,
            (p.x >> 8) as u8,
            (p.y & 0xFF) as u8,
            (p.y >> 8) as u8,
            if key { code | 0x80 } else { 0 },
            0,
        ];
        self.raw = rawf;
        self.ring[(self.ring_i & 7) as usize] = rawf;
        self.ring_i = self.ring_i.wrapping_add(1);

        if key {
            np = 0; // a key report carries no screen point
        }
        Some(Frame { pressed, key, n: np, pts, code })
    }
}
