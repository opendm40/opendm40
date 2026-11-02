//! The touch test page (View::TouchTest; Settings > Other > Touch test, or
//! DBG:TOUCH:TEST ON; src/app_touchtest.rs feeds it): the raw controller
//! frames as dots against crosshair targets, to judge the calibration, the
//! jitter and the touch-key strip by eye. Region C (`.cold`, link-native.x):
//! the page is never open while the AD2 stream or a logger session runs.
//!
//!   targets   9 crosshairs: the corners 20 px in, the mid-edges, the centre
//!   40..272   the four touch keys (MENU RUN AUTO MF, top to bottom as on the
//!             meter): lit while the controller reports the key, with the
//!             raw X of the last key report
//!   42..200   text lines: the last raw frame, the last touch-down and its
//!             offset to the nearest target, frame / stale / bad counters
//!   146..174  the touch calibration in use (F14, right of the centre target)
//!   204..224  legend: raw frame dot, corrected (debounced) dot, touch-down
//!             cross
//!   228..286  the exit hint, the Calibrate and Clear boxes
//!
//! Calibrate (app_touchtest.rs) draws two more screens over the page: the
//! walk (`cal_page`: one target highlighted at a time, the prompt, a status
//! line, Cancel / Reset) and the result (`result_page`: the fit's residuals
//! and coefficients, Save or Try again, Cancel).
//!
//! No framebuffer: the dots stay on the panel until Clear (or any full
//! repaint) draws the page again; a text line repaints its own box only.

use super::*;
use crate::touch::Point;

/// Target centres (the corners 20 px in, the mid-edges, the centre).
pub const TARGETS: [(u16, u16); 9] =
    [(20, 20), (240, 20), (459, 20), (20, 160), (240, 160), (459, 160), (20, 299), (240, 299), (459, 299)];
/// Crosshair arm length, px (the target's box is 2 * ARM + 1).
const ARM: u16 = 8;

/// Raw frame point (record 0 of a pressed frame, before the debounce).
pub const C_RAW: Color = VALUE;
/// A point of a debounced touch state, through the calibration (what the
/// pages get).
pub const C_DEB: Color = 0x07E0;
/// The debounced touch-down point (where a tap lands).
pub const C_DOWN: Color = 0xF800;
const C_TARGET: Color = DIM;
/// The calibration walk's current target.
pub const C_HI: Color = 0xFFE0;

/// Touch-key boxes, top to bottom (MENU, RUN, AUTO, MF).
const KEY_X: i16 = 40;
const KEY_W: i16 = 96;
const KEY_Y0: i16 = 40;
const KEY_H: i16 = 52;
const KEY_PITCH: i16 = 60;
const KEY_NAMES: [&str; 4] = ["MENU", "RUN", "AUTO", "MF"];

/// Text lines (TEXT22): 0..3 above the centre target, 4 below it.
const LINE_X: i16 = 150;
const LINE_W: i16 = 290;
const LINE_H: i16 = 24;
const LINE_Y: [i16; 5] = [42, 68, 94, 120, 176];
pub const LINES: usize = LINE_Y.len();

const LEGEND_Y: i16 = 204;
const HINT: sf::Rect = rect(LINE_X, 228, LINE_W, LINE_H);
/// "Calibrate" (starts the calibration walk) and "Clear" (clears the dots).
/// Above the bottom row of targets (their crosses start at y 291).
pub const CALIB: sf::Rect = rect(LINE_X, 254, 124, 33);
pub const CLEAR: sf::Rect = rect(330, 254, 104, 33);
/// The calibration in use, two F14 lines right of the centre target.
const CAL_TXT: [sf::Rect; 2] = [rect(256, 145, 186, 14), rect(256, 160, 186, 14)];

/// Calibration walk / result screens: text lines across the page between
/// the target rows, the two buttons.
const W_X: i16 = 40;
const W_W: i16 = 400;
const W_Y: [i16; 4] = [44, 70, 96, 122];
const W_MSG_Y: i16 = 184;
pub const BTN: [sf::Rect; 2] = [rect(100, 224, 130, 42), rect(250, 224, 130, 42)];
/// The current target's crosshair arm, px.
const HI_ARM: i16 = 14;

fn line_rect(i: usize) -> sf::Rect {
    rect(LINE_X, LINE_Y[i.min(LINES - 1)], LINE_W, LINE_H)
}

fn key_rect(code: u8) -> sf::Rect {
    rect(KEY_X, KEY_Y0 + (code as i16 & 3) * KEY_PITCH, KEY_W, KEY_H)
}

/// A filled rectangle that the host layout check sees as a shape.
fn shape(x: i16, y: i16, w: i16, h: i16, c: Color) {
    #[cfg(test)]
    crate::probe::begin(crate::probe::SHAPE, M_BG);
    lcd::fill_rect(x as u16, y as u16, w as u16, h as u16, c);
    #[cfg(test)]
    crate::probe::end();
}

/// The whole page: background, targets, key boxes (not lit), legend, hint,
/// Calibrate, Clear. The text lines and the calibration lines are the
/// caller's (`line`, `cal_status`).
#[inline(never)]
pub fn page() {
    lcd::fill_rect(0, 0, lcd::W, lcd::H, M_BG);
    targets();
    for code in 0..4 {
        key(code, false, None);
    }
    // Legend: raw dot, debounced dot, down cross.
    let ly = LEGEND_Y + 10;
    shape(LINE_X + 2, ly - 2, 5, 5, C_RAW);
    sf::glyphs(sf::TEXT22, rect(LINE_X + 12, LEGEND_Y, 44, 20), "raw", STATUS_FG, M_BG, 1, 0);
    shape(LINE_X + 65, ly - 1, 3, 3, C_DEB);
    sf::glyphs(sf::TEXT22, rect(LINE_X + 74, LEGEND_Y, 100, 20), "corrected", STATUS_FG, M_BG, 1, 0);
    #[cfg(test)]
    crate::probe::begin(crate::probe::SHAPE, M_BG);
    cross_at(LINE_X + 191, ly);
    #[cfg(test)]
    crate::probe::end();
    sf::glyphs(sf::TEXT22, rect(LINE_X + 203, LEGEND_Y, 60, 20), "down", STATUS_FG, M_BG, 1, 0);
    hint(false);
    button(CALIB, "Calibrate");
    button(CLEAR, "Clear");
}

/// A button box with its centred text.
fn button(r: sf::Rect, text: &str) {
    sf::rbox(r, 6, M_TILE, M_BG);
    sf::glyphs(sf::TEXT22, r, text, STATUS_FG, M_TILE, 8, 0);
}

/// The 9 targets, dim.
fn targets() {
    for &(x, y) in TARGETS.iter() {
        let (x, y, a) = (x as i16, y as i16, ARM as i16);
        #[cfg(test)]
        crate::probe::begin(crate::probe::SHAPE, M_BG);
        lcd::fill_rect((x - a) as u16, y as u16, (2 * a + 1) as u16, 1, C_TARGET);
        lcd::fill_rect(x as u16, (y - a) as u16, 1, (2 * a + 1) as u16, C_TARGET);
        #[cfg(test)]
        crate::probe::end();
    }
}

/// The calibration in use (touchcal.rs a..f; None: the identity) in the
/// page's F14 lines.
#[inline(never)]
pub fn cal_status(k: Option<[f32; 6]>) {
    let mut t = [Txt::new(), Txt::new()];
    match k {
        None => {
            t[0].s("Cal: none (stock)");
        }
        Some(k) => {
            t[0].s("Cal ");
            t[1].s("Cal ");
            eq(&mut t[0], "x'=", &k[0..3], false);
            eq(&mut t[1], "y'=", &k[3..6], false);
        }
    }
    for i in 0..2 {
        sf::label(sf::F14, CAL_TXT[i], t[i].as_str(), STATUS_FG, M_BG, 1, 0);
    }
}

/// "x'=0.996x+0.010y-5.6" (`sp`: spaces before the signs).
fn eq(t: &mut Txt, lhs: &str, k: &[f32], sp: bool) {
    t.s(lhs).f(k[0], 3, false).s(if sp { "x " } else { "x" }).f(k[1], 3, true).s(if sp { "y " } else { "y" }).f(k[2], 1, true);
}

/// Target `i` of TARGETS highlighted (`on`) or back to a dim crosshair.
#[inline(never)]
pub fn cal_target(i: usize, on: bool) {
    let (x, y) = TARGETS[i.min(TARGETS.len() - 1)];
    let (x, y, a) = (x as i16, y as i16, HI_ARM);
    lcd::fill_rect((x - a - 1) as u16, (y - a - 1) as u16, (2 * a + 3) as u16, (2 * a + 3) as u16, M_BG);
    #[cfg(test)]
    crate::probe::begin(crate::probe::SHAPE, M_BG);
    if on {
        lcd::fill_rect((x - a) as u16, (y - 1) as u16, (2 * a + 1) as u16, 3, C_HI);
        lcd::fill_rect((x - 1) as u16, (y - a) as u16, 3, (2 * a + 1) as u16, C_HI);
    } else {
        let a = ARM as i16;
        lcd::fill_rect((x - a) as u16, y as u16, (2 * a + 1) as u16, 1, C_TARGET);
        lcd::fill_rect(x as u16, (y - a) as u16, 1, (2 * a + 1) as u16, C_TARGET);
    }
    #[cfg(test)]
    crate::probe::end();
}

fn w_line(i: usize, text: &str, c: Color) {
    sf::label(sf::TEXT22, rect(W_X, W_Y[i.min(W_Y.len() - 1)], W_W, LINE_H), text, c, M_BG, 8, 0);
}

/// The calibration walk's screen: the targets (none highlighted yet), the
/// title, Cancel and Reset. The caller draws the prompt and the target.
#[inline(never)]
pub fn cal_page() {
    lcd::fill_rect(0, 0, lcd::W, lcd::H, M_BG);
    targets();
    w_line(0, "Touch calibration", M_ACCENT);
    w_line(1, "Tap the yellow cross", STATUS_FG);
    button(BTN[0], "Cancel");
    button(BTN[1], "Reset");
}

/// The walk's step line: target `i` (0-based) of 9, tap `tap` (0-based) of
/// `taps`.
#[inline(never)]
pub fn cal_step(i: usize, tap: u8, taps: u8) {
    let mut t = Txt::new();
    t.s("Point ").u(i as u32 + 1).s(" of ").u(TARGETS.len() as u32).s(", tap ").u(tap as u32 + 1).s(" of ").u(taps as u32);
    w_line(2, t.as_str(), STATUS_FG);
}

/// The status line under the centre target (`err`: red).
#[inline(never)]
pub fn cal_msg(text: &str, err: bool) {
    sf::label(sf::TEXT22, rect(W_X, W_MSG_Y, W_W, LINE_H), text, if err { C_DOWN } else { C_DEB }, M_BG, 8, 0);
}

/// The fit's screen: residuals in tenths of a px (`mean`, `max`, `before`:
/// the raw taps' mean error), the coefficients a..f, the verdict (`bad`:
/// why it is refused) and its buttons (Save or Try again, Cancel). The
/// caller draws the taps' dots.
#[inline(never)]
pub fn result_page(mean: u32, max: u32, before: u32, k: &[f32; 6], bad: Option<&str>) {
    lcd::fill_rect(0, 0, lcd::W, lcd::H, M_BG);
    targets();
    let mut t = Txt::new();
    t.s("Fit error: mean ").d(mean).s(", max ").d(max).s(" px");
    w_line(0, t.as_str(), STATUS_FG);
    let mut t = Txt::new();
    t.s("Before: mean ").d(before).s(" px");
    w_line(1, t.as_str(), STATUS_FG);
    let mut t = Txt::new();
    eq(&mut t, "x' = ", &k[0..3], true);
    w_line(2, t.as_str(), STATUS_FG);
    let mut t = Txt::new();
    eq(&mut t, "y' = ", &k[3..6], true);
    w_line(3, t.as_str(), STATUS_FG);
    match bad {
        None => cal_msg("Good fit: Save applies it", false),
        Some(why) => cal_msg(why, true),
    }
    button(BTN[0], if bad.is_none() { "Save" } else { "Try again" });
    button(BTN[1], "Cancel");
}

/// Which of BTN (0, 1) holds (x, y).
pub fn btn_hit(x: u16, y: u16) -> Option<usize> {
    BTN.iter().position(|r| in_rect(r, x, y))
}

pub fn calib_hit(x: u16, y: u16) -> bool {
    in_rect(&CALIB, x, y)
}

fn in_rect(r: &sf::Rect, x: u16, y: u16) -> bool {
    let (x, y) = (x as i16, y as i16);
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// The exit hint; `holding`: a long press is under way (it exits at 2 s).
#[inline(never)]
pub fn hint(holding: bool) {
    let (a, c) = if holding { ("Keep holding to exit", C_DOWN) } else { ("Exit: power key, hold 2 s", M_ACCENT) };
    sf::label(sf::TEXT22, HINT, a, c, M_BG, 1, 0);
}

/// Text line `i` (0..LINES), its box repainted.
#[inline(never)]
pub fn line(i: usize, text: &str) {
    sf::label(sf::TEXT22, line_rect(i), text, STATUS_FG, M_BG, 1, 0);
}

/// Touch key `code` (0 MENU .. 3 MF): lit while held; `raw_x` the X of
/// its last key report ("-" before any).
#[inline(never)]
pub fn key(code: u8, lit: bool, raw_x: Option<u16>) {
    let r = key_rect(code);
    let bg = if lit { M_ACCENT } else { M_TILE };
    sf::rbox(r, 6, bg, M_BG);
    sf::glyphs(sf::TEXT22, rect(r.x, r.y + 3, r.w, 24), KEY_NAMES[code as usize & 3], STATUS_FG, bg, 8, 0);
    let mut b = Txt::new();
    match raw_x {
        Some(x) => {
            b.s("x ").u(x as u32);
        }
        None => {
            b.s("-");
        }
    }
    sf::glyphs(sf::F14, rect(r.x, r.y + 29, r.w, 18), b.as_str(), STATUS_FG, bg, 8, 0);
}

/// A raw frame point: a 5 x 5 dot (C_RAW).
pub const DOT_RAW: u16 = 2;
/// A debounced point: a 3 x 3 dot (C_DEB), so a raw dot under it stays
/// visible as a ring.
pub const DOT_DEB: u16 = 1;

/// A (2 r + 1)-square dot at `p` (clipped to the panel; data, not a layout
/// item).
#[inline(never)]
pub fn dot(p: Point, r: u16, c: Color) {
    let x0 = p.x.saturating_sub(r).min(lcd::W - 1);
    let y0 = p.y.saturating_sub(r).min(lcd::H - 1);
    let x1 = (p.x + r).min(lcd::W - 1);
    let y1 = (p.y + r).min(lcd::H - 1);
    lcd::fill_rect(x0, y0, x1 - x0 + 1, y1 - y0 + 1, c);
}

fn cross_at(x: i16, y: i16) {
    let h = |x0: i16, x1: i16, y: i16| {
        let (x0, x1) = (x0.max(0), x1.min(lcd::W as i16 - 1));
        if (0..lcd::H as i16).contains(&y) && x0 <= x1 {
            lcd::fill_rect(x0 as u16, y as u16, (x1 - x0 + 1) as u16, 1, C_DOWN);
        }
    };
    let v = |x: i16, y0: i16, y1: i16| {
        let (y0, y1) = (y0.max(0), y1.min(lcd::H as i16 - 1));
        if (0..lcd::W as i16).contains(&x) && y0 <= y1 {
            lcd::fill_rect(x as u16, y0 as u16, 1, (y1 - y0 + 1) as u16, C_DOWN);
        }
    };
    h(x - 5, x + 5, y);
    v(x, y - 5, y + 5);
}

/// The touch-down cross (11 x 11) at `p` (data, as the dots: not a layout
/// item for the host check).
#[inline(never)]
pub fn cross(p: Point) {
    cross_at(p.x as i16, p.y as i16);
}

/// The target nearest to `p`.
pub fn nearest(p: Point) -> Point {
    let d = |&&(x, y): &&(u16, u16)| {
        let (dx, dy) = (x as i32 - p.x as i32, y as i32 - p.y as i32);
        dx * dx + dy * dy
    };
    let &(x, y) = TARGETS.iter().min_by_key(d).unwrap_or(&TARGETS[4]);
    Point { x, y }
}

pub fn clear_hit(x: u16, y: u16) -> bool {
    in_rect(&CLEAR, x, y)
}

/// A short text built in place (the lines are at most 40 bytes).
pub struct Txt {
    b: [u8; 40],
    n: usize,
}

impl Txt {
    pub fn new() -> Self {
        Txt { b: [0; 40], n: 0 }
    }
    /// Tenths as "12.3".
    pub fn d(&mut self, v: u32) -> &mut Self {
        self.u(v / 10).s(".").u(v % 10)
    }
    /// `v` with `dec` (1..3) decimals, rounded; `plus`: a "+" before a
    /// value >= 0.
    pub fn f(&mut self, v: f32, dec: u8, plus: bool) -> &mut Self {
        let m = [10u32, 100, 1000][(dec.clamp(1, 3) - 1) as usize];
        let neg = v < 0.0;
        let a = if neg { -v } else { v };
        let q = (a * m as f32 + 0.5) as u32;
        if neg && q != 0 {
            self.s("-");
        } else if plus {
            self.s("+");
        }
        self.u(q / m).s(".");
        let mut r = m / 10;
        while r > 0 {
            self.u(q / r % 10);
            r /= 10;
        }
        self
    }
    pub fn s(&mut self, t: &str) -> &mut Self {
        for &c in t.as_bytes() {
            if self.n < self.b.len() {
                self.b[self.n] = c;
                self.n += 1;
            }
        }
        self
    }
    pub fn u(&mut self, mut v: u32) -> &mut Self {
        let mut d = [0u8; 10];
        let mut k = 0;
        loop {
            d[k] = b'0' + (v % 10) as u8;
            k += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while k > 0 {
            k -= 1;
            if self.n < self.b.len() {
                self.b[self.n] = d[k];
                self.n += 1;
            }
        }
        self
    }
    /// Signed, always with its sign ("+0", "-7").
    pub fn i(&mut self, v: i32) -> &mut Self {
        self.s(if v < 0 { "-" } else { "+" });
        self.u(v.unsigned_abs())
    }
    /// "x,y".
    pub fn p(&mut self, p: Point) -> &mut Self {
        self.u(p.x as u32).s(",").u(p.y as u32)
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}
