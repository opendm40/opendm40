//! The Presets page (View::Presets; app_presets.rs, presets.rs): main menu >
//! Presets. Settings-page style: the title bar "< Back  Presets", the four
//! slots as rows (the selected one in the accent colour), a status line
//! (what the selected slot holds, or the question of a pending Replace /
//! Clear) and the Save / Recall / Rename / Clear buttons. Rename opens
//! `keyboard`: the name field, 40 character keys, abc / Space / Del / OK.
//!
//! Layout (y): title 0..40; rows 46 + 44 i, 40 px tall (to 218); status
//! 222..250; buttons 256..312 (4 x 110 px, 8 px apart). Keyboard: field
//! 46..86, key rows 92 + 44 r (40 px tall, 43 px keys on a 46 px pitch from
//! x 11), the bottom row 268..308.
//!
//! Region C (`.cold`, link-native.x): drawn and hit-tested only on this
//! page, where no stream runs.

use super::*;

const ROW_X: i16 = 8;
const ROW_W: i16 = 464;
const ROW_Y0: i16 = 46;
const ROW_PITCH: i16 = 44;
const ROW_H: i16 = 40;
const STATUS: sf::Rect = rect(8, 222, 464, 28);
const BTN_Y0: i16 = 256;
const BTN_H: i16 = 56;
const BTN_W: i16 = 110;
const BTN_PITCH: i16 = 118;
/// Button labels; a pending confirm shows CONFIRM_LABELS instead.
pub static BUTTONS: [&str; 4] = ["Save", "Recall", "Rename", "Clear"];
pub const B_SAVE: u8 = 0;
pub const B_RECALL: u8 = 1;
pub const B_RENAME: u8 = 2;
pub const B_CLEAR: u8 = 3;
/// Text dim (an empty slot, a button that does nothing now).
const DIMTXT: Color = 0x8410;
/// A pending Replace / Clear button; an unreadable slot's text.
const WARN: Color = 0xC800;

/// One slot row as the page shows it.
#[derive(Clone, Copy)]
pub struct Row<'a> {
    /// 0 empty, 1 used, 2 unreadable (presets::Slot).
    pub state: u8,
    pub name: &'a str,
}

/// A touch target of the page: a slot row or a button.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    Row(u8),
    Button(u8),
}

fn row_rect(i: usize) -> sf::Rect {
    rect(ROW_X, ROW_Y0 + i as i16 * ROW_PITCH, ROW_W, ROW_H)
}

fn btn_rect(i: usize) -> sf::Rect {
    rect(ROW_X + i as i16 * BTN_PITCH, BTN_Y0, BTN_W, BTN_H)
}

/// Slot row `i`: the number, the name ("Empty" / "Unreadable" dim).
pub fn row(i: usize, r: Row, selected: bool, pressed: bool) {
    let rr = row_rect(i);
    let bg = if pressed { KEY_PRESSED } else if selected { K_SEL } else { M_TILE };
    sf::rbox(rr, 6, bg, M_BG);
    let num = [b'1' + i as u8];
    let nb = rect(rr.x + 6, rr.y + 6, 28, 28);
    let nbg = if selected || pressed { M_TILE } else { M_ACCENT };
    sf::rbox(nb, 4, nbg, bg);
    sf::glyphs(sf::TEXT22, nb, core::str::from_utf8(&num).unwrap_or("?"), STATUS_FG, nbg, 8, 0);
    let (t, fg) = match r.state {
        1 => (r.name, STATUS_FG),
        2 => (crate::cold_str!("Unreadable"), if selected { STATUS_FG } else { WARN }),
        _ => (crate::cold_str!("Empty"), if selected { STATUS_FG } else { DIMTXT }),
    };
    sf::glyphs(sf::TEXT22, rect(rr.x + 54, rr.y, rr.w - 60, rr.h), t, fg, bg, 1, 0);
}

/// Button `i`: `on` it acts (accent), else dim; `confirm` a pending Replace
/// / Clear (red, `label` the question).
pub fn button(i: usize, label: &str, on: bool, confirm: bool, pressed: bool) {
    let r = btn_rect(i);
    let bg = if pressed {
        KEY_PRESSED
    } else if confirm {
        WARN
    } else if on {
        M_ACCENT
    } else {
        M_TILE
    };
    sf::rbox(r, 6, bg, M_BG);
    sf::glyphs(sf::TEXT22, r, label, if on || confirm { STATUS_FG } else { DIMTXT }, bg, 8, 0);
}

/// The status line (cleared first).
pub fn status(t: &str, warn: bool) {
    lcd::fill_rect(STATUS.x as u16, STATUS.y as u16, STATUS.w as u16, STATUS.h as u16, M_BG);
    sf::glyphs(sf::TEXT22, STATUS, t, if warn { 0xFD20 } else { STATUS_FG }, M_BG, 8, 0);
}

/// The whole page. `buttons`: each button's label, whether it acts, whether
/// it is a pending confirm.
pub fn page(title: &str, rows: &[Row; 4], sel: usize, buttons: &[(&str, bool, bool); 4], st: &str, warn: bool) {
    page_title(title);
    lcd::fill_rect(0, PAGE_BAR_H as u16, lcd::W, lcd::H - PAGE_BAR_H as u16, M_BG);
    for (i, r) in rows.iter().enumerate() {
        row(i, *r, i == sel, false);
    }
    status(st, warn);
    for (i, &(l, on, c)) in buttons.iter().enumerate() {
        button(i, l, on, c, false);
    }
}

/// What a touch at (x, y) below the title bar hits (each target owns half
/// of the gap around it).
pub fn hit(x: u16, y: u16) -> Option<Hit> {
    let (x, y) = (x as i16, y as i16);
    if !(ROW_X - 4..ROW_X + ROW_W + 4).contains(&x) {
        return None;
    }
    let half = (ROW_PITCH - ROW_H) / 2;
    if (ROW_Y0 - half..ROW_Y0 + 3 * ROW_PITCH + ROW_H + half).contains(&y) {
        let i = ((y - (ROW_Y0 - half)) / ROW_PITCH).min(3);
        return Some(Hit::Row(i as u8));
    }
    if (BTN_Y0 - 4..BTN_Y0 + BTN_H + 4).contains(&y) {
        let i = ((x - (ROW_X - 4)) / BTN_PITCH).min(3);
        return Some(Hit::Button(i as u8));
    }
    None
}

// ----------------------------------------------------------------- keyboard

/// Character keys, row by row (lower case on the `abc` layout: the letters).
pub const KEYS: [&[u8; 10]; 4] = [b"1234567890", b"QWERTYUIOP", b"ASDFGHJKL-", b"ZXCVBNM.+/"];
/// Bottom row keys after the 40 character keys: abc / ABC, Space, Del, OK.
pub const K_CASE: u8 = 40;
pub const K_SPACE: u8 = 41;
pub const K_DEL: u8 = 42;
pub const K_OK: u8 = 43;
const KEY_X0: i16 = 11;
const KEY_PITCH: i16 = 46;
const KEY_W: i16 = 43;
const KEY_Y0: i16 = 92;
const KEY_ROW_PITCH: i16 = 44;
const KEY_H: i16 = 40;
const FIELD: sf::Rect = rect(8, 46, 464, 40);
/// Bottom row: (x, w) of abc, Space, Del, OK.
const BOTTOM: [(i16, i16); 4] = [(11, 89), (106, 176), (288, 86), (380, 89)];
const BOTTOM_Y: i16 = KEY_Y0 + 4 * KEY_ROW_PITCH;

fn key_rect(k: u8) -> sf::Rect {
    if k < 40 {
        let (r, c) = ((k / 10) as i16, (k % 10) as i16);
        rect(KEY_X0 + c * KEY_PITCH, KEY_Y0 + r * KEY_ROW_PITCH, KEY_W, KEY_H)
    } else {
        let (x, w) = BOTTOM[(k - 40).min(3) as usize];
        rect(x, BOTTOM_Y, w, KEY_H)
    }
}

/// The character of key `k` (< 40), `lower`: letters in lower case.
pub fn key_char(k: u8, lower: bool) -> u8 {
    let c = KEYS[(k / 10) as usize % 4][(k % 10) as usize];
    if lower { c.to_ascii_lowercase() } else { c }
}

/// Key `k` normal or pressed.
pub fn key(k: u8, lower: bool, pressed: bool) {
    let r = key_rect(k);
    let bg = if pressed {
        K_SEL
    } else if k == K_OK {
        M_ACCENT
    } else {
        M_TILE
    };
    sf::rbox(r, 4, bg, M_BG);
    let mut c = [0u8; 1];
    let t = match k {
        K_CASE => {
            if lower { crate::cold_str!("ABC") } else { crate::cold_str!("abc") }
        }
        K_SPACE => crate::cold_str!("Space"),
        K_DEL => crate::cold_str!("Del"),
        K_OK => crate::cold_str!("OK"),
        _ => {
            c[0] = key_char(k, lower);
            core::str::from_utf8(&c).unwrap_or("?")
        }
    };
    sf::glyphs(sf::TEXT22, r, t, STATUS_FG, bg, 8, 0);
}

/// The name field: the text so far and a cursor bar after it.
pub fn field(t: &str) {
    sf::rbox(FIELD, 6, STATUS_FG, M_BG);
    let tr = rect(FIELD.x + 10, FIELD.y, FIELD.w - 20, FIELD.h);
    sf::glyphs(sf::TEXT22, tr, t, 0x0000, STATUS_FG, 1, 0);
    let x = tr.x + if t.is_empty() { 0 } else { sf::text_width(sf::TEXT22, t, 0) + 3 };
    lcd::fill_rect(x as u16, (FIELD.y + 8) as u16, 2, (FIELD.h - 16) as u16, M_ACCENT);
}

/// The rename keyboard for slot `slot` (0-based) with the name so far.
pub fn keyboard(slot: usize, t: &str, lower: bool) {
    static T: [u8; 15] = *b"Rename preset 1";
    let mut title = T;
    title[14] = b'1' + slot as u8;
    page_title(core::str::from_utf8(&title).unwrap_or("Rename"));
    lcd::fill_rect(0, PAGE_BAR_H as u16, lcd::W, lcd::H - PAGE_BAR_H as u16, M_BG);
    field(t);
    for k in 0..44 {
        key(k, lower, false);
    }
}

/// The key under a touch (2 px of slop around each key: the gaps split).
pub fn key_hit(x: u16, y: u16) -> Option<u8> {
    let (x, y) = (x as i16, y as i16);
    (0..44u8).find(|&k| {
        let r = key_rect(k);
        x >= r.x - 2 && x < r.x + r.w + 2 && y >= r.y - 2 && y < r.y + r.h + 2
    })
}
