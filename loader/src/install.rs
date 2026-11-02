//! Install: a verify pass over the whole file, then zero the marker, erase
//! sectors 1..8 and as far as the image needs 9..11, program, read-back CRC,
//! put the record and block back for a small image, program the marker.
use crate::boot::{self, APP, MARKER};
use crate::fat::Vol;
use crate::hw::*;
use crate::ui::{status, STATUS_Y};
use crate::{lcd, spi, tail};

const KEY: u32 = 0x4002_3C04;
const STAT: u32 = 0x4002_3C0C;
const CTL: u32 = 0x4002_3C10;
const BUSY: u32 = 1 << 16;

/// dm40fw.py decode_block: a short block with the key outside it stays as is.
fn decode(b: &mut [u8; 128], n: usize) {
    if n <= 0x1F {
        return;
    }
    let a = b[0x1F];
    let kp = if a < 128 { a } else { a >> 1 } as usize & 127;
    if kp >= n {
        return;
    }
    let k = b[kp];
    for c in b.iter_mut().take(n) {
        *c = (*c ^ k).wrapping_sub(a);
    }
    b[0x1F] = a;
    b[kp] = k;
}

fn idle() {
    while r(STAT) & BUSY != 0 {}
}

/// Status errors cleared, unlocked once; a second key sequence would fault.
pub fn unlock() {
    if r(CTL) >> 31 != 0 {
        w(KEY, 0x4567_0123);
        w(KEY, 0xCDEF_89AB);
    }
    w(STAT, 0x1F3);
}

pub fn lock() {
    w(CTL, 1 << 31);
}

pub fn prog(a: u32, v: u32) {
    w(CTL, 0x201);
    w(a, v);
    unsafe { core::arch::asm!("dsb") };
    idle();
}

pub fn erase(s: u32) {
    w(CTL, 0x202 | s << 3);
    w(CTL, 0x202 | s << 3 | 1 << 16);
    idle();
}

fn marker(h: &[u8; 12]) {
    let mut i = 0;
    while i < 12 {
        prog(MARKER + i as u32, u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]));
        i += 4;
    }
}

const BAR_X: u16 = 270;
const BAR_W: u32 = 200;

/// The progress bar on the status row, after the phase's word.
fn bar(done: u32, size: u32) {
    let w = (done / (size / BAR_W + 1)) as u16;
    lcd::fill(BAR_X, STATUS_Y + 6, BAR_W as u16, 12, 0x4208);
    if w > 0 {
        lcd::fill(BAR_X, STATUS_Y + 6, w, 12, 0x07E0);
    }
}

/// The file's decoded header equals the marker.
pub fn is_marker(v: &Vol, cl: u16, size: u32) -> bool {
    if size < 12 {
        return false;
    }
    let mut b = [0u8; 128];
    let n = size.min(128) as usize;
    spi::read(v.cl_addr(cl), &mut b[..n]);
    decode(&mut b, n);
    let mut i = 0;
    while i < 12 {
        if b[i] != r8(MARKER + i as u32) {
            return false;
        }
        i += 1;
    }
    true
}

/// One pass over the file. `flash` false: header, size, chain and body CRC, no
/// flash access. `flash` true: erase and program. Err = text for the status row.
#[inline(never)]
fn pass(v: &Vol, mut cl: u16, size: u32, hdr: &mut [u8; 12], flash: bool) -> Result<(), &'static [u8]> {
    let mut b = [0u8; 128];
    let mut fo = 0u32;
    let mut dst = APP;
    let mut c = 0xFFFFu16;
    while fo < size {
        let n = (size - fo).min(128) as usize;
        spi::read(v.cl_addr(cl) + (fo & (v.cl_size - 1)), &mut b[..n]);
        decode(&mut b, n);
        let mut i = 0;
        if fo == 0 {
            hdr.copy_from_slice(&b[..12]);
            if !boot::header_ok(hdr) || boot::body_len(hdr) + 12 != size {
                return Err(b"BAD FILE");
            }
            if flash {
                // Past this point the old application is gone.
                status(b"ERASING");
                unlock();
                marker(&[0; 12]);
                // Sectors 9..11 only as far as the image reaches, tail checked by `run`.
                let end = APP + boot::body_len(hdr);
                let mut s = 1;
                while s <= 11 {
                    if s <= 8 || end > 0x0800_0000 + ((s - 4) << 17) {
                        erase(s);
                    }
                    s += 1;
                }
                status(b"PROGRAMMING");
            }
            i = 12;
        }
        while i < n {
            // Whole words but for the last one, 0xFF padded.
            let mut wd = [0xFFu8; 4];
            let mut k = 0;
            while k < 4 && i + k < n {
                wd[k] = b[i + k];
                if !flash && !(MARKER..MARKER + 12).contains(&(dst + k as u32)) {
                    c = boot::crc(c, wd[k]);
                }
                k += 1;
            }
            if flash {
                prog(dst, u32::from_le_bytes(wd));
            }
            dst += 4;
            i += 4;
        }
        fo += n as u32;
        if fo & (v.cl_size - 1) == 0 && fo < size {
            bar(fo, size);
            cl = v.next(cl);
            if !(2..0xFFF8).contains(&cl) {
                return Err(b"SHORT FILE");
            }
        }
    }
    if !flash && c != u16::from_le_bytes([hdr[8], hdr[9]]) {
        return Err(b"CRC ERROR");
    }
    Ok(())
}

/// Returns an error text, or resets on success.
#[inline(never)]
pub fn run(v: &Vol, cl: u16, size: u32) -> &'static [u8] {
    if size < 12 {
        return b"BAD FILE";
    }
    let mut hdr = [0u8; 12];
    status(b"CHECKING");
    if cfg!(feature = "verify") {
        if let Err(e) = pass(v, cl, size, &mut hdr, false) {
            return e;
        }
    }
    let big = APP + boot::body_len(&hdr) > 0x080A_0000;
    if big && !tail::ok() {
        return b"NO CAL COPY";
    }
    let res = pass(v, cl, size, &mut hdr, true);
    let mut e: &'static [u8] = b"FLASH CRC ERROR";
    if let Err(x) = res {
        e = x;
    } else if boot::body_ok(&hdr) && (big || !tail::ok() || tail::restore()) {
        marker(&hdr);
        lock();
        status(b"OK");
        sysreset()
    }
    lock();
    e
}
