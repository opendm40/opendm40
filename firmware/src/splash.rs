//! Boot splash (owner 2026-10-26: "at least show 'Loading'"): "opendm40",
//! the version, "Loading..." and a progress bar that `step` advances once per boot phase (native.rs pre_main, then
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

/// Top of "opendm40"; the version, "Loading...", the bar and the note
/// below it at fixed offsets: the block without the note sits a little
/// above the 320-px screen's centre, with the note a little below.
const TOP: i16 = 84;
/// Progress bar: outer frame x, y, w, h (1 px DIM frame, 1 px gap).
const BAR: (i16, i16, i16, i16) = (160, TOP + 114, 160, 8);
/// `note`'s two lines: named statics, so they are region C data (a string
/// literal's section has no module path: region B).
/// At most 48 characters each (stockfont `label`'s MAXG).
static NOTE1: [u8; 46] = *b"First start after update: copying system files";
static NOTE2: [u8; 22] = *b"Later starts are fast.";
const NOTE_FONT: sf::Font = sf::SC_F16;
/// `step` calls from the splash to the first frame: pre_main's six
/// phases, then main's App::new.
pub const STEPS: u8 = 7;
#[cfg(not(test))]
const TIM13_CCR1: u32 = 0x4000_1C34;

/// SRAM (`.sram_bss`), as native.rs PHASE: the stack's headroom.
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static STEP: AtomicU8 = AtomicU8::new(0);
/// The splash is up and `note` not drawn yet.
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
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
    sf::label(sf::ASCII26, rect(0, TOP, lcd::W as i16, 30), "opendm40", FG, BG, 8, 1);
    let mut b = [0u8; 24];
    b[0] = b'v';
    let v = crate::ident::VERSION.as_bytes();
    let n = v.len().min(b.len() - 1);
    b[1..1 + n].copy_from_slice(&v[..n]);
    let ver = core::str::from_utf8(&b[..1 + n]).unwrap_or("");
    sf::label(sf::ASCII26, rect(0, TOP + 32, lcd::W as i16, 30), ver, DIM, BG, 8, 1);
    sf::label(sf::ASCII26, rect(0, TOP + 74, lcd::W as i16, 30), "Loading...", DIM, BG, 8, 1);
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
        sf::label(NOTE_FONT, rect(0, TOP + 140 + 26 * i as i16, lcd::W as i16, 22), t, DIM, BG, 8, 0);
    }
}

/// The first frame replaced the splash: no `note` from now on.
pub fn done() {
    NOTE_DUE.store(false, Ordering::Relaxed);
}
