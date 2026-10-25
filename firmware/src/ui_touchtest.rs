//! The touch test page (View::TouchTest; Settings 3/3 > Touch test, or
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
//!   204..224  legend: raw frame dot, debounced dot, touch-down cross
//!   228..280  the exit hint, the Clear box
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
/// A point of a debounced touch state (what the pages get).
pub const C_DEB: Color = 0x07E0;
/// The debounced touch-down point (where a tap lands).
pub const C_DOWN: Color = 0xF800;
const C_TARGET: Color = DIM;

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
const HINT: [sf::Rect; 2] = [rect(LINE_X, 230, 170, LINE_H), rect(LINE_X, 256, 170, LINE_H)];
/// "Clear" (a tap there clears the dots).
pub const CLEAR: sf::Rect = rect(330, 238, 104, 42);

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
/// Clear. The text lines are the caller's (`line`).
#[inline(never)]
pub fn page() {
    lcd::fill_rect(0, 0, lcd::W, lcd::H, M_BG);
    for &(x, y) in TARGETS.iter() {
        let (x, y, a) = (x as i16, y as i16, ARM as i16);
        #[cfg(test)]
        crate::probe::begin(crate::probe::SHAPE, M_BG);
        lcd::fill_rect((x - a) as u16, y as u16, (2 * a + 1) as u16, 1, C_TARGET);
        lcd::fill_rect(x as u16, (y - a) as u16, 1, (2 * a + 1) as u16, C_TARGET);
        #[cfg(test)]
        crate::probe::end();
    }
    for code in 0..4 {
        key(code, false, None);
    }
    // Legend: raw dot, debounced dot, down cross.
    let ly = LEGEND_Y + 10;
    shape(LINE_X + 2, ly - 2, 5, 5, C_RAW);
    sf::glyphs(sf::TEXT22, rect(LINE_X + 12, LEGEND_Y, 44, 20), "raw", STATUS_FG, M_BG, 1, 0);
    shape(LINE_X + 65, ly - 1, 3, 3, C_DEB);
    sf::glyphs(sf::TEXT22, rect(LINE_X + 74, LEGEND_Y, 100, 20), "debounced", STATUS_FG, M_BG, 1, 0);
    #[cfg(test)]
    crate::probe::begin(crate::probe::SHAPE, M_BG);
    cross_at(LINE_X + 191, ly);
    #[cfg(test)]
    crate::probe::end();
    sf::glyphs(sf::TEXT22, rect(LINE_X + 203, LEGEND_Y, 60, 20), "down", STATUS_FG, M_BG, 1, 0);
    hint(false);
    sf::rbox(CLEAR, 6, M_TILE, M_BG);
    sf::glyphs(sf::TEXT22, CLEAR, "Clear", STATUS_FG, M_TILE, 8, 0);
}

/// The exit hint; `holding`: a long press is under way (it exits at 2 s).
#[inline(never)]
pub fn hint(holding: bool) {
    let (a, b, c) = if holding { ("Keep holding", "to exit", C_DOWN) } else { ("Exit: power key", "or hold 2 s", M_ACCENT) };
    sf::label(sf::TEXT22, HINT[0], a, c, M_BG, 1, 0);
    sf::label(sf::TEXT22, HINT[1], b, c, M_BG, 1, 0);
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
    let (x, y) = (x as i16, y as i16);
    x >= CLEAR.x && x < CLEAR.x + CLEAR.w && y >= CLEAR.y && y < CLEAR.y + CLEAR.h
}

/// A short text built in place (the lines are at most 32 bytes).
pub struct Txt {
    b: [u8; 32],
    n: usize,
}

impl Txt {
    pub fn new() -> Self {
        Txt { b: [0; 32], n: 0 }
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
