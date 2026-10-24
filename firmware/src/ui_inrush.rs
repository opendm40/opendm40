//! The Inrush Current page (View::Inrush; src/app_inrush.rs, src/inrush.rs;
//! docs/inrush-current.md): the MF range page's look (title bar 0x2966,
//! BG body, 0x2966 boxes, SEL accent) with the DMM page's strip chart
//! (frame (80,176,321,97), axis labels left, chips under them, a right
//! column). Region B: the page may repaint while the AD2 stream runs (a
//! toast ends, the live level), and no region C code may run then.
//!
//!   0..40     title bar "Inrush Current"
//!   44..80    "Trigger"   [ - ]   +1.0 A   [ + ]      (range-page grid)
//!   86..144   [ Irms 100 ms   12.345 A ] [ Peak   -34.56 A ]
//!   148..172  state line ("Armed: 0.12 A, step +1.0 A", "Saved FAST003")
//!   176..273  the captured trace, 2 s (100 ms / div), the 100 ms window
//!             shaded, t = 0 marked; axis labels, "100ms/div", the rate;
//!             right column: idle rms, the file
//!   280..316  [ Back ] [ Arm / Cancel ]

use super::*;

/// Trigger stepper keys and value (the range page's 4-column grid).
pub const THR_MINUS: sf::Rect = rect(125, 44, 112, 36);
pub const THR_PLUS: sf::Rect = rect(359, 44, 112, 36);
const THR_VAL: sf::Rect = rect(242, 44, 112, 36);
const THR_LABEL: sf::Rect = rect(12, 44, 108, 36);
/// Result boxes (name row, value + unit row).
const BOX: [sf::Rect; 2] = [rect(8, 86, 229, 58), rect(243, 86, 229, 58)];
const BOX_NAME: [&str; 2] = ["Irms 100 ms", "Peak"];
const INFO: sf::Rect = rect(8, 148, 464, 24);
/// Bottom buttons.
pub const BACK_BTN: sf::Rect = rect(8, 280, 150, 36);
pub const ARM_BTN: sf::Rect = rect(162, 280, 310, 36);
/// Chart frame (the DMM page's), its left labels / chips and the right
/// column (as `chart` / `Side`).
const CX: u16 = CHART_X;
const CW: u16 = CHART_W;
const CY: u16 = 176;
const CH: u16 = 97;
const LBL_Y: [i16; 3] = [176, 219, 262];
const RCOL_X: i16 = 405;
const RCOL_R: i16 = 476;
const RCOL_Y: [i16; 4] = [176, 190, 245, 259];
/// Shaded background of the 100 ms window and the t = 0 line.
const WIN_BG: Color = 0x1967;
const T0: Color = SEL;
/// Trace colour (the DMM chart's).
const TRACE: Color = 0xFC40;
/// State line colours.
pub const C_ARMED: Color = 0x04FC;
pub const C_TRIG: Color = VALUE;
pub const C_OK: Color = 0x07E0;
pub const C_WARN: Color = 0xFD20;
pub const C_PLAIN: Color = STATUS_FG;

/// What the page shows (texts formatted by the app).
pub struct Vals<'a> {
    pub thr: &'a str,
    pub rms: &'a str,
    pub peak: &'a str,
    pub info: &'a str,
    pub info_fg: Color,
    pub idle: &'a str,
    pub file: &'a str,
    /// Main button text and colour; `enabled` false = greyed (saving).
    pub btn: &'a str,
    pub btn_bg: Color,
    pub enabled: bool,
}

/// The whole page (`trace` draws the chart).
pub fn page(v: &Vals) {
    lcd::fill_rect(0, 0, lcd::W, 40, BTN);
    lcd::fill_rect(0, 40, lcd::W, lcd::H - 40, BG);
    sf::logo(4, 10, BTN);
    sf::glyphs(sf::TEXT22, rect(0, 0, 480, 40), "Inrush Current", STATUS_FG, BTN, 8, 0);
    sf::glyphs(sf::TEXT22, THR_LABEL, "Trigger", STATUS_FG, BG, 1, 0);
    for (r, t) in [(THR_MINUS, "-"), (THR_PLUS, "+")] {
        sf::rbox(r, 6, BTN, BG);
        sf::glyphs(sf::TEXT22, r, t, STATUS_FG, BTN, 8, 0);
    }
    thr(v.thr);
    for (i, r) in BOX.iter().enumerate() {
        sf::rbox(*r, 6, BTN, BG);
        sf::glyphs(sf::TEXT22, rect(r.x + 10, r.y + 1, 160, 26), BOX_NAME[i], STATUS_FG, BTN, 1, 0);
    }
    values(v.rms, v.peak);
    info(v.info, v.info_fg);
    side(v.idle, v.file);
    for (y, t) in [(192i16, "100ms/div"), (236, "9600/s")] {
        let r = rect(4, y, 73, 17);
        sf::rbox(r, 4, BTN, BG);
        sf::glyphs(sf::F14, rect(r.x, r.y + 1, r.w, r.h), t, STATUS_FG, BTN, 8, 1);
    }
    sf::rbox(BACK_BTN, 6, BTN, BG);
    sf::glyphs(sf::TEXT22, BACK_BTN, "Back", STATUS_FG, BTN, 8, 0);
    button(v.btn, v.btn_bg, v.enabled);
}

/// The threshold value between the stepper keys.
pub fn thr(t: &str) {
    sf::label(sf::TEXT22, THR_VAL, t, C_ARMED, BG, 8, 0);
}

/// Value + unit of the two result boxes ("" = none: "---").
pub fn values(rms: &str, peak: &str) {
    for (i, t) in [rms, peak].iter().enumerate() {
        let r = BOX[i];
        let t = if t.is_empty() { "---" } else { t };
        let f = if sf::has_all(sf::LETTERS, t) { sf::LETTERS } else { sf::TEXT22 };
        sf::label(f, rect(r.x + 6, r.y + 27, 174, 30), t, VALUE, BTN, 4, 0);
        let unit = if t == "OL" { "" } else { "A" };
        sf::label(sf::TEXT22, rect(r.x + 186, r.y + 29, 36, 26), unit, VALUE, BTN, 1, 0);
    }
}

/// The state line.
pub fn info(t: &str, fg: Color) {
    sf::label(sf::TEXT22, INFO, t, fg, BG, 1, 0);
}

/// Right column: idle rms and the saved file.
pub fn side(idle: &str, file: &str) {
    for (i, (t, fg)) in [("IDLE", DIM), (idle, STATUS_FG), ("FILE", DIM), (file, STATUS_FG)].iter().enumerate() {
        let al = if i % 2 == 0 { 1 } else { 4 };
        sf::label(sf::F14, rect(RCOL_X, RCOL_Y[i], RCOL_R - RCOL_X, 14), t, *fg, BG, al, 1);
    }
}

/// The main button (Arm / Cancel / Saving...).
pub fn button(t: &str, bg: Color, enabled: bool) {
    sf::rbox(ARM_BTN, 6, bg, BG);
    sf::glyphs(sf::TEXT22, ARM_BTN, t, if enabled { STATUS_FG } else { DIM }, bg, 8, 0);
}

fn hit(r: sf::Rect, x: u16, y: u16) -> bool {
    let (x, y) = (x as i16, y as i16);
    // 2 px of slop (the gaps between neighbours split between them).
    x >= r.x - 2 && x < r.x + r.w + 2 && y >= r.y - 2 && y < r.y + r.h + 2
}

/// Page touch: 0 "-", 1 "+", 2 Back, 3 the main button.
pub fn page_hit(x: u16, y: u16) -> Option<u8> {
    [THR_MINUS, THR_PLUS, BACK_BTN, ARM_BTN].iter().position(|&r| hit(r, x, y)).map(|i| i as u8)
}

/// "12.345" (A) with the decimals of its size (the result boxes, labels).
pub fn amps<'a>(b: &'a mut [u8; 16], v: f32) -> &'a str {
    let a = if v < 0.0 { -v } else { v };
    let dec = if a < 9.9995 { 3 } else if a < 99.995 { 2 } else { 1 };
    crate::dmm::format(b, v, dec)
}

/// The trace chart: `t` = the linear capture (inrush::LEN codes, PRE of
/// them before t = 0), `zf` = the zero code, `b` = A per code; None = an
/// empty grid (armed, nothing captured yet).
pub fn trace(t: Option<&[u16]>, zf: f32, b: f32) {
    use crate::inrush::{LEN, PRE, WIN100};
    // Code range of the trace, 0 A included; at least 1 A tall.
    let (mut lo, mut hi) = (zf, zf);
    if let Some(t) = t {
        for &c in &t[..LEN] {
            let c = c as i16 as f32;
            lo = lo.min(c);
            hi = hi.max(c);
        }
    }
    let min_span = 1.0 / b.max(1e-9);
    if hi - lo < min_span {
        let m = (hi + lo) / 2.0;
        (lo, hi) = (m - min_span / 2.0, m + min_span / 2.0);
    }
    let pad = (hi - lo) * 0.05;
    (lo, hi) = (lo - pad, hi + pad);
    let span = hi - lo;
    // Axis labels (A), right-aligned to x 76 as the DMM chart's.
    let mut nb = [0u8; 16];
    let mut lb = [0u8; 24];
    for (i, c) in [hi, (hi + lo) / 2.0, lo].iter().enumerate() {
        let s = if t.is_some() { join(&mut lb, &[amps(&mut nb, (c - zf) * b), "A"]) } else { "" };
        sf::label(sf::F14, rect(4, LBL_Y[i], 73, 14), s, STATUS_FG, BG, 4, 1);
    }
    let row = |c: f32| -> u16 { (((hi - c) / span) * (CH - 3) as f32 + 1.0).clamp(1.0, (CH - 2) as f32) as u16 };
    let x_of = |k: usize| k * CW as usize / LEN;
    let (w0, w1, x0) = (x_of(PRE), x_of(PRE + WIN100), x_of(PRE));
    for x in 0..CW as usize {
        // The column's samples: min / max code -> rows.
        let span_rows = t.map(|t| {
            let a = x * LEN / CW as usize;
            let e = ((x + 1) * LEN / CW as usize).max(a + 1).min(LEN);
            let (mut mn, mut mx) = (i16::MAX, i16::MIN);
            for &c in &t[a..e] {
                mn = mn.min(c as i16);
                mx = mx.max(c as i16);
            }
            (row(mx as f32), row(mn as f32))
        });
        let edge_x = x == 0 || x == CW as usize - 1;
        let vline = x % 16 == 0;
        let bg = if x >= w0 && x < w1 { WIN_BG } else { BG };
        lcd::set_window(CX + x as u16, CY, CX + x as u16, CY + CH - 1);
        for y in 0..CH {
            let edge = edge_x || y == 0 || y == CH - 1;
            let grid = (vline && (y % 4 == 0 || y % 16 == 0)) || (y % 16 == 0 && x % 4 == 0);
            let on = span_rows.is_some_and(|(a, e)| y >= a && y <= e);
            lcd::data(if on {
                TRACE
            } else if edge {
                DIM
            } else if x == x0 {
                T0
            } else if grid {
                DIM
            } else {
                bg
            });
        }
    }
}
