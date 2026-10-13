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

// --- Data-logger panel (FR-38) ------------------------------------------------

/// Logger panel over any view while LOG:STATe ON: the GUI is frozen then (the
/// HY3131 belongs to the logger, the DMM reading is not updated), so the
/// panel says so instead of leaving a stale reading up. Covers the main
/// reading of the DMM view.
const LOG_BOX: (u16, u16, u16, u16) = (70, 64, 340, 146);
const LOG_ROW: [u16; 3] = [102, 138, 174];
/// Refresh of the rate / count rows, ms.
const LOG_REFRESH_MS: u64 = 1000;

/// Panel progress: `part` = next part to draw (0 title, 1 rate, 2 count,
/// 3 hint, 4 frame, 5 = nothing pending); `up` = the whole panel has been
/// drawn once (later passes redraw only 1 and 2); `next` = next refresh, ms.
struct LogPanel {
    part: u8,
    up: bool,
    next: u64,
}

static mut LOG: LogPanel = LogPanel { part: 0, up: false, next: 0 };

fn log_state() -> &'static mut LogPanel {
    unsafe { &mut *core::ptr::addr_of_mut!(LOG) }
}

/// Call every main-loop pass while logging and drawing is otherwise
/// allowed. Draws at most ONE part per call (one label, a ~340 x 36 fill +
/// glyphs, roughly 0.5 ms) so the logger's sampling (one conversion per
/// ~0.4 ms at the default OSR) loses at most a sample or two per part; the
/// rate and sample count are refreshed once per second (two passes).
/// `hz` = measured rate (0 until two samples: the nominal one from
/// `nominal_us` is shown with "~"), `n` = samples captured this session.
pub fn log_panel(now: u64, hz: u32, n: u32, nominal_us: u32) {
    let st = log_state();
    let b = LOG_BOX;
    let row = |i: usize| (b.0 + 1, LOG_ROW[i], b.2 - 2, 36);
    match st.part {
        0 => centred((b.0, b.1, b.2, 38), "DATA LOGGING", FG, ERR_BG),
        1 => {
            let mut t = [0u8; 24];
            let (k, approx) = if hz == 0 && nominal_us != 0 { (1_000_000 / nominal_us, true) } else { (hz, false) };
            let mut i = 0;
            if approx {
                t[0] = b'~';
                i = 1;
            }
            i += dec(&mut t[i..], k);
            t[i..i + 5].copy_from_slice(b" Sa/s");
            centred(row(0), core::str::from_utf8(&t[..i + 5]).unwrap_or(""), FG, BODY_BG);
        }
        2 => {
            let mut t = [0u8; 24];
            let i = dec(&mut t, n);
            t[i..i + 8].copy_from_slice(b" samples");
            centred(row(1), core::str::from_utf8(&t[..i + 8]).unwrap_or(""), FG, BODY_BG);
        }
        3 => centred(row(2), "Display paused", FRAME, BODY_BG),
        4 => frame(b, FRAME),
        _ => {
            if now >= st.next {
                st.part = 1; // rate now, count on the next pass
            }
            return;
        }
    }
    // The hint and the frame are static: a refresh ends after the count.
    st.part = if st.part == 2 && st.up { 5 } else { st.part + 1 };
    if st.part == 5 {
        st.up = true;
        st.next = now + LOG_REFRESH_MS;
    }
}

/// The logger stopped: the next session draws the whole panel again (the
/// caller repaints the view).
pub fn log_panel_reset() {
    *log_state() = LogPanel { part: 0, up: false, next: 0 };
}

/// `v` in decimal at the start of `t`; returns the length.
fn dec(t: &mut [u8], v: u32) -> usize {
    let mut d = [0u8; 10];
    let (mut k, mut v) = (d.len(), v);
    loop {
        k -= 1;
        d[k] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let n = d.len() - k;
    t[..n].copy_from_slice(&d[k..]);
    n
}
