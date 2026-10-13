//! Modal overlays drawn over any view: the power-off confirm (2-finger swipe
//! down, docs/user-feedback.md UF-18, scope-ui-plan.md §2.6: never an instant
//! off) and short toasts (screenshot result). There is no frame buffer: while
//! an overlay is up the app keeps its full redraw pending and draws nothing
//! else, and when it goes the view is redrawn from scratch.
//!
//! Layout (stock confirm dialog 0x08074ce0: box (104,92,292,150), title bar
//! 40 px, colours below), all in screen coordinates for DBG:TAP:
//!   box    (94, 80) 292 x 160
//!   No     (106, 180) 128 x 48   -> cancel
//!   Yes    (246, 180) 128 x 48   -> power off
//! Toast: (90, 132) 300 x 56, centred text.

use crate::lcd::{self, Color};
use crate::stockfont::{self as sf, rect};

/// Stock dialog colours (gui_v2_6.h: title 0x04fc, body 0x10a3).
const TITLE_BG: Color = 0x04FC;
const BODY_BG: Color = 0x10A3;
const FG: Color = 0xF79E;
const BTN: Color = 0x2966;
const BTN_RED: Color = 0xB0A2;
const FRAME: Color = 0x94D3;
const OK_BG: Color = 0x0340;
const ERR_BG: Color = 0x9800;

pub const BOX: (u16, u16, u16, u16) = (94, 80, 292, 160);
pub const NO: (u16, u16, u16, u16) = (106, 180, 128, 48);
pub const YES: (u16, u16, u16, u16) = (246, 180, 128, 48);
const TOAST: (u16, u16, u16, u16) = (90, 132, 300, 56);

/// Auto-cancel of the power-off confirm, ms.
pub const CONFIRM_MS: u64 = 3000;
/// Toast duration, ms.
pub const TOAST_MS: u64 = 1500;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// Power-off confirm, cancelled at `until` (ms).
    PowerOff { until: u64 },
    /// Toast, gone at `until` (ms).
    Toast { until: u64 },
}

impl Overlay {
    pub fn until(&self) -> u64 {
        match *self {
            Overlay::PowerOff { until } | Overlay::Toast { until } => until,
        }
    }
}

fn inside(r: (u16, u16, u16, u16), x: u16, y: u16) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

fn frame(r: (u16, u16, u16, u16), c: Color) {
    lcd::fill_rect(r.0, r.1, r.2, 1, c);
    lcd::fill_rect(r.0, r.1 + r.3 - 1, r.2, 1, c);
    lcd::fill_rect(r.0, r.1, 1, r.3, c);
    lcd::fill_rect(r.0 + r.2 - 1, r.1, 1, r.3, c);
}

/// `s` centred in `r` (the stock 26 px ASCII font; fills `r` with `bg`).
fn centred(r: (u16, u16, u16, u16), s: &str, fg: Color, bg: Color) {
    sf::label(sf::ASCII26, rect(r.0 as i16, r.1 as i16, r.2 as i16, r.3 as i16), s, fg, bg, 8, 0);
}

fn button(r: (u16, u16, u16, u16), label: &str, bg: Color) {
    centred(r, label, FG, bg);
    frame(r, FRAME);
}

/// Draw the power-off confirm.
pub fn power_off() {
    let b = BOX;
    centred((b.0, b.1, b.2, 40), "Power off?", FG, TITLE_BG);
    centred((b.0, b.1 + 40, b.2, 60), "Cancels in 3 s", FG, BODY_BG);
    lcd::fill_rect(b.0, b.1 + 100, b.2, b.3 - 100, BODY_BG);
    frame(b, FRAME);
    button(NO, "No", BTN);
    button(YES, "Yes", BTN_RED);
}

/// Power-off confirm hit test: Some(true) = Yes, Some(false) = No, None =
/// elsewhere.
pub fn power_off_hit(x: u16, y: u16) -> Option<bool> {
    if inside(YES, x, y) {
        Some(true)
    } else if inside(NO, x, y) {
        Some(false)
    } else {
        None
    }
}

/// Draw a toast (`ok` = green, else red).
pub fn toast(s: &str, ok: bool) {
    let r = TOAST;
    let bg = if ok { OK_BG } else { ERR_BG };
    centred(r, s, FG, bg);
    frame(r, FRAME);
}
