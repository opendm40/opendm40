//! Boot splash (owner 2026-10-26: "at least show 'Loading'"): the wide
//! Wiren Board logo, "opendm40", the version, "Loading..." and a progress
//! bar that `step` advances once per boot phase (native.rs pre_main, then
//! main.rs up to the first frame, which replaces the splash); `note` under
//! it on a slow first start (calbackup.rs rewriting a file, mostly the
//! INFO/ mirror of vdisk.rs `mirror_boot`).
//!
//! A submodule of native.rs, so its code and data are region C (.cold,
//! link-native.x `8opendm406native`): it only runs before the main loop.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::lcd;
use crate::stockfont::{self as sf, rect};
use crate::ui::{BG, DIM, FG, SEL};

mod data {
    include!("splash_logo.rs");
}
use data::{BOARD_HALF, SPLASH_H as LH, SPLASH_W as LW, TEXT_BOX, TEXT_RUNS};

/// The logo's box, centred on the 480-px row.
const LOGO_X: i16 = (lcd::W as i16 - LW) / 2;
const LOGO_Y: i16 = 36;
/// Progress bar: outer frame x, y, w, h (1 px DIM frame, 1 px gap).
const BAR: (i16, i16, i16, i16) = (160, 214, 160, 8);
/// `note`'s two lines: named statics, so they are region C data (a string
/// literal's section has no module path: region B).
/// At most 48 characters each (stockfont `label`'s MAXG).
static NOTE1: [u8; 46] = *b"First start after update: copying system files";
static NOTE2: [u8; 22] = *b"Later starts are fast.";
const NOTE_FONT: sf::Font = sf::SC_F16;
/// `step` calls from the splash to the first frame: pre_main's six
/// phases, then main's App::new.
pub const STEPS: u8 = 7;
/// The stock logo green (#5cb400), as stockfont::logo.
const GREEN: lcd::Color = 0x5DA0;
#[cfg(not(test))]
const TIM13_CCR1: u32 = 0x4000_1C34;

static STEP: AtomicU8 = AtomicU8::new(0);
/// The splash is up and `note` not drawn yet.
static NOTE_DUE: AtomicBool = AtomicBool::new(false);

/// Draw the splash and light the backlight (at the backup-SRAM record's
/// level, else the default 55 %; App::new sets the saved level again).
#[cfg(not(test))]
pub fn show() {
    draw();
    crate::persist::init();
    // The record's word 1 (App::restore) without the CRC check: a torn
    // record only sets this level until App::new; persist::load would pull
    // an outlined copy into region B.
    let level = crate::persist::stored().map_or(55, |_| (crate::persist::word(1) & 0xFF).clamp(10, 100));
    unsafe { crate::mmio::w32(TIM13_CCR1, level) };
}

/// The splash with an empty bar (tools/ui-render draws it too).
pub fn draw() {
    NOTE_DUE.store(true, Ordering::Relaxed);
    lcd::fill_rect(0, 0, lcd::W, lcd::H, BG);
    logo();
    sf::label(sf::ASCII26, rect(0, 100, lcd::W as i16, 30), "opendm40", FG, BG, 8, 1);
    let mut b = [0u8; 24];
    b[0] = b'v';
    let v = crate::ident::VERSION.as_bytes();
    let n = v.len().min(b.len() - 1);
    b[1..1 + n].copy_from_slice(&v[..n]);
    let ver = core::str::from_utf8(&b[..1 + n]).unwrap_or("");
    sf::label(sf::ASCII26, rect(0, 132, lcd::W as i16, 30), ver, DIM, BG, 8, 1);
    sf::label(sf::ASCII26, rect(0, 174, lcd::W as i16, 30), "Loading...", DIM, BG, 8, 1);
    let (x, y, w, h) = BAR;
    lcd::fill_rect(x as u16, y as u16, w as u16, h as u16, DIM);
    lcd::fill_rect(x as u16 + 1, y as u16 + 1, w as u16 - 2, h as u16 - 2, BG);
}

/// One boot phase done: the bar's next part.
pub fn step() {
    let n = (STEP.load(Ordering::Relaxed) + 1).min(STEPS);
    STEP.store(n, Ordering::Relaxed);
    let (x, y, w, h) = BAR;
    let inner = w - 4;
    let fill = inner * n as i16 / STEPS as i16;
    lcd::fill_rect(x as u16 + 2, y as u16 + 2, fill as u16, h as u16 - 4, SEL);
}

/// Under the bar, once while the splash is up: why this start is slow.
#[inline(never)]
pub fn note() {
    if !NOTE_DUE.swap(false, Ordering::Relaxed) {
        return;
    }
    for (i, t) in [&NOTE1[..], &NOTE2[..]].into_iter().enumerate() {
        let t = core::str::from_utf8(t).unwrap_or("");
        sf::label(NOTE_FONT, rect(0, 240 + 26 * i as i16, lcd::W as i16, 22), t, DIM, BG, 8, 0);
    }
}

/// The first frame replaced the splash: no `note` from now on.
pub fn done() {
    NOTE_DUE.store(false, Ordering::Relaxed);
}

/// The lettering's coverage runs (tools/mklogo.py): 0, 1 (half), 2 (full).
struct Runs {
    k: usize,
    v: u8,
    left: u16,
}

impl Runs {
    fn nib(&mut self) -> u16 {
        let b = TEXT_RUNS.get(self.k >> 1).copied().unwrap_or(0);
        self.k += 1;
        (if self.k & 1 == 1 { b >> 4 } else { b & 15 }) as u16
    }

    /// Nibble: bit 3 = the next value, bits 0-2 = the run length 1..6;
    /// 7 = 7 + the next nibble; 0 = the next three nibbles.
    fn next(&mut self) -> u8 {
        if self.left == 0 {
            let t = self.nib();
            let n = match t & 7 {
                0 => self.nib() << 8 | self.nib() << 4 | self.nib(),
                7 => 7 + self.nib(),
                n => n,
            };
            self.v = (self.v + 1 + (t >> 3) as u8) % 3;
            self.left = n.max(1);
        }
        self.left -= 1;
        self.v
    }
}

/// The wide logo: the green board drawn from its half-height profile
/// (mirrored both ways, 4 sub-columns per pixel for the anti-aliasing),
/// the white lettering from its runs on top.
fn logo() {
    let green = sf::ramp(GREEN, BG);
    #[cfg(test)]
    crate::probe::begin(crate::probe::ICON, BG);
    lcd::set_window(LOGO_X as u16, LOGO_Y as u16, (LOGO_X + LW - 1) as u16, (LOGO_Y + LH - 1) as u16);
    let (tx, ty, tw, th) = TEXT_BOX;
    let mut runs = Runs { k: 0, v: 2, left: 0 };
    let half = LW / 2;
    for y in 0..LH {
        // Distance from the centre line to the row's inner edge, 1/8 px.
        let d = 8 * if y < LH / 2 { LH / 2 - 1 - y } else { y - LH / 2 } as i32;
        for x in 0..LW {
            // Left half as stored; the right half mirrored (the profile
            // read from the other end).
            let (a, b) = if x < half {
                (BOARD_HALF[x as usize] as i32, BOARD_HALF[x as usize + 1] as i32)
            } else {
                let m = (LW - 1 - x) as usize;
                (BOARD_HALF[m + 1] as i32, BOARD_HALF[m] as i32)
            };
            let mut cov = 0;
            for s in [1, 3, 5, 7] {
                cov += ((a * (8 - s) + b * s) / 8 - d).clamp(0, 8);
            }
            let mut c = green[(cov * 15 / 32) as usize];
            if x >= tx && x < tx + tw && y >= ty && y < ty + th {
                c = match runs.next() {
                    0 => c,
                    1 => sf::ramp(0xFFFF, c)[8],
                    _ => 0xFFFF,
                };
            }
            lcd::data(c);
        }
    }
    #[cfg(test)]
    crate::probe::end();
}
