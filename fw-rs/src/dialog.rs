//! Modal overlays drawn over any view: the power-off confirm (2-finger swipe
//! down, docs/user-feedback.md UF-18, scope-ui-plan.md §2.6: never an instant
//! off) and short toasts (screenshot / CSV recording results). There is no frame buffer: while
//! an overlay is up the app keeps its full redraw pending and draws nothing
//! else, and when it goes the view is redrawn from scratch.
//!
//! Layout (stock confirm dialog 0x08074ce0: box (104,92,292,150), title bar
//! 40 px, colours below), all in screen coordinates for DBG:TAP:
//!   box    (94, 80) 292 x 160
//!   No     (106, 180) 128 x 48   -> cancel
//!   Yes    (246, 180) 128 x 48   -> power off
//! Toast: (40, 132) 400 x 56, centred text at glyph spacing 2 (ASCII26
//! "f" touches its neighbour below 2; "Disk in use by PC - eject first" is
//! 388 px).

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
pub const TOAST: (u16, u16, u16, u16) = (40, 132, 400, 56);

/// Auto-cancel of the power-off confirm, ms.
pub const CONFIRM_MS: u64 = 3000;
/// Toast duration, ms.
pub const TOAST_MS: u64 = 1500;
/// Scope AUTO-set fell back from AC to DC coupling (UF-04, app.rs autoset).
pub const AUTO_DC: &str = "AUTO: DC (no AC signal)";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// Power-off confirm, cancelled at `until` (ms).
    PowerOff { until: u64 },
    /// Toast, gone at `until` (ms).
    Toast { until: u64 },
    /// Power-meter wiring dialog (ui::pw::entry_dialog), cancelled at
    /// `until` (ms).
    PowerEntry { until: u64 },
    /// Power-meter energy reset confirm (`energy_reset`, the power-off
    /// confirm's box and buttons), cancelled at `until` (ms).
    EnergyReset { until: u64 },
}

impl Overlay {
    pub fn until(&self) -> u64 {
        match *self {
            Overlay::PowerOff { until } | Overlay::Toast { until } | Overlay::PowerEntry { until } | Overlay::EnergyReset { until } => until,
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
    centred_sp(r, s, fg, bg, 0);
}

fn centred_sp(r: (u16, u16, u16, u16), s: &str, fg: Color, bg: Color, spacing: i8) {
    sf::label(sf::ASCII26, rect(r.0 as i16, r.1 as i16, r.2 as i16, r.3 as i16), s, fg, bg, 8, spacing);
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

/// Draw the energy reset confirm (power meter: a long press on the energy
/// readout / the side column); hit test = `power_off_hit`.
pub fn energy_reset() {
    let b = BOX;
    centred((b.0, b.1, b.2, 40), "Reset energy?", FG, TITLE_BG);
    centred((b.0, b.1 + 40, b.2, 60), "Wh, VAh and the time", FG, BODY_BG);
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
    centred_sp(r, s, FG, bg, 2);
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

/// What the logger panel shows (main.rs fills it from the Logger).
#[derive(Clone, Copy)]
pub struct LogInfo {
    /// Measured rate, S/s (0 until two samples: the nominal one is shown
    /// with "~").
    pub hz: u32,
    /// Samples captured this session.
    pub n: u32,
    /// Nominal (chip) rate, S/s (Logger::nominal_mhz, rounded).
    pub nominal_hz: u32,
    /// AD2 source (FR-38 B): the rate row reads "AD2 <measured>/<nominal>".
    pub ad2: bool,
    /// Conversions not captured (Logger::lost); 0 = the "Display paused" hint.
    pub lost: u32,
}

/// The three text rows of the logger panel (rate, count, hint / loss).
pub fn log_panel_text(i: &LogInfo, row: usize, t: &mut [u8; 32]) -> usize {
    let nominal = i.nominal_hz;
    let mut k = 0;
    let mut put = |t: &mut [u8; 32], s: &[u8]| {
        let n = s.len().min(32 - k);
        t[k..k + n].copy_from_slice(&s[..n]);
        k += n;
    };
    let mut d = [0u8; 10];
    match row {
        0 => {
            if i.ad2 {
                put(t, b"AD2 ");
            }
            if i.hz == 0 && nominal != 0 {
                put(t, b"~");
                let n = dec(&mut d, nominal);
                put(t, &d[..n]);
            } else {
                let n = dec(&mut d, i.hz);
                put(t, &d[..n]);
                if i.ad2 {
                    put(t, b"/");
                    let n = dec(&mut d, nominal);
                    put(t, &d[..n]);
                }
            }
            put(t, b" Sa/s");
        }
        1 => {
            let n = dec(&mut d, i.n);
            put(t, &d[..n]);
            put(t, b" samples");
        }
        _ => {
            if i.lost == 0 {
                put(t, b"Display paused");
            } else {
                put(t, b"lost ");
                let n = dec(&mut d, i.lost);
                put(t, &d[..n]);
                // per mille of the conversions due, one decimal of a percent
                let pm = (i.lost as u64 * 1000 / (i.lost as u64 + i.n as u64).max(1)) as u32;
                put(t, b" (");
                let n = dec(&mut d, pm / 10);
                put(t, &d[..n]);
                put(t, b".");
                let n = dec(&mut d, pm % 10);
                put(t, &d[..n]);
                put(t, b"%)");
            }
        }
    }
    k
}

/// Call every main-loop pass while logging and drawing is otherwise
/// allowed. Draws at most ONE part per call (one label, a ~340 x 36 fill +
/// glyphs, roughly 0.5 ms) so the logger's sampling loses at most a sample
/// or two per part at AD1 rates (at AD2 rates a part costs several
/// conversions: LOG:GAPs? shows it); the rate, sample count and loss rows
/// are refreshed once per second (three passes).
pub fn log_panel(now: u64, info: LogInfo) {
    let st = log_state();
    let b = LOG_BOX;
    let row = |i: usize| (b.0 + 1, LOG_ROW[i], b.2 - 2, 36);
    let mut t = [0u8; 32];
    match st.part {
        // Letter spacing 2 as the toasts (ASCII26 glyphs touch at 0).
        0 => centred_sp((b.0, b.1, b.2, 38), "DATA LOGGING", FG, ERR_BG, 2),
        p @ 1..=3 => {
            let k = log_panel_text(&info, p as usize - 1, &mut t);
            let fg = if p == 3 && info.lost == 0 { FRAME } else { FG };
            centred_sp(row(p as usize - 1), core::str::from_utf8(&t[..k]).unwrap_or(""), fg, BODY_BG, 2);
        }
        4 => frame(b, FRAME),
        _ => {
            if now >= st.next {
                st.part = 1; // rate now, count and loss on the next passes
            }
            return;
        }
    }
    // The frame is static: a refresh ends after the loss row.
    st.part = if st.part == 3 && st.up { 5 } else { st.part + 1 };
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
