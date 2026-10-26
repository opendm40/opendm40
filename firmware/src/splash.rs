//! Boot splash (owner 2026-10-26: "at least show 'Loading'"): the round
//! Wiren Board logo, "opendm40", the version, "Loading..." and a progress
//! bar that `step` advances once per boot phase (native.rs pre_main, then
//! main.rs up to the first frame, which replaces the splash).
//!
//! A submodule of native.rs, so its code and data are region C (.cold,
//! link-native.x `8opendm406native`): it only runs before the main loop.

use core::sync::atomic::{AtomicU8, Ordering};

use crate::lcd;
use crate::stockfont::{self as sf, rect};
use crate::ui::{BG, DIM, FG, SEL};

mod data {
    include!("splash_logo.rs");
}
use data::{MARK, MARK_BITS, SPLASH_N as N};

/// Logo square, centred on the 480-px row.
const LOGO_X: i16 = (lcd::W as i16 - N) / 2;
const LOGO_Y: i16 = 56;
/// Progress bar: outer frame x, y, w, h (1 px DIM frame, 1 px gap).
const BAR: (i16, i16, i16, i16) = (160, 252, 160, 8);
/// `step` calls from the splash to the first frame: pre_main's six
/// phases, then main's App::new.
pub const STEPS: u8 = 7;
/// The stock logo green (#5cb400), as stockfont::logo.
const GREEN: lcd::Color = 0x5DA0;
#[cfg(not(test))]
const TIM13_CCR1: u32 = 0x4000_1C34;

static STEP: AtomicU8 = AtomicU8::new(0);

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
    lcd::fill_rect(0, 0, lcd::W, lcd::H, BG);
    logo();
    sf::label(sf::ASCII26, rect(0, 134, lcd::W as i16, 30), "opendm40", FG, BG, 8, 1);
    let mut b = [0u8; 24];
    b[0] = b'v';
    let v = crate::ident::VERSION.as_bytes();
    let n = v.len().min(b.len() - 1);
    b[1..1 + n].copy_from_slice(&v[..n]);
    let ver = core::str::from_utf8(&b[..1 + n]).unwrap_or("");
    sf::label(sf::ASCII26, rect(0, 168, lcd::W as i16, 30), ver, DIM, BG, 8, 1);
    sf::label(sf::ASCII26, rect(0, 212, lcd::W as i16, 30), "Loading...", DIM, BG, 8, 1);
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

/// The logo: an anti-aliased green circle drawn by code, the 1-bpp white
/// mark (tools/mklogo.py) on it.
fn logo() {
    let green = sf::ramp(GREEN, BG);
    #[cfg(test)]
    crate::probe::begin(crate::probe::ICON, BG);
    lcd::set_window(LOGO_X as u16, LOGO_Y as u16, (LOGO_X + N - 1) as u16, (LOGO_Y + N - 1) as u16);
    // Half-pixel units: pixel centre (2x + 1 - N), radius N. Coverage of the
    // edge pixel ~ 0.5 + (r - d) with r - d ~ (R^2 - D^2) / (4R).
    let r = N as i32;
    let (mx, my, mw, mh) = MARK;
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = ((2 * x + 1 - N) as i32, (2 * y + 1 - N) as i32);
            let cov = (7 + 15 * (r * r - dx * dx - dy * dy) / (4 * r)).clamp(0, 15);
            let (u, v) = (x - mx, y - my);
            let on = u >= 0 && v >= 0 && u < mw && v < mh && {
                let i = (v * mw + u) as usize;
                MARK_BITS[i >> 3] >> (7 - (i & 7)) & 1 != 0
            };
            lcd::data(if on { 0xFFFF } else { green[cov as usize] });
        }
    }
    #[cfg(test)]
    crate::probe::end();
}
