//! Multimeter screen. Geometry and colours measured on read-backs of the stock
//! screen (fw-rs/ref/); the main reading is red instead of the stock orange.

use crate::dmm::{self, Reading};
use crate::fonts::{LABEL, UNIT, VALUE};
use crate::lcd::{self, Color};
use crate::text;

/// RGB565 of the stock colours (panel reads back RGB666).
pub const BG: Color = 0x0883; // (8, 16, 24)
pub const BAR: Color = 0x1904; // (24, 32, 32) status bar
pub const FG: Color = 0xF79E; // (240, 244, 240) status text
/// Main reading: red (the one intended difference from stock orange).
pub const VALUE_RED: Color = 0xF145;

const BAR_H: u16 = 34;
const VALUE_X0: u16 = 0;
const VALUE_X1: u16 = 396; // right edge of the last digit
const VALUE_BASE: u16 = 146;
const UNIT_X: u16 = 404;
const UNIT_BASE: u16 = 144;

pub fn draw_static() {
    lcd::fill_rect(0, 0, lcd::W, BAR_H, BAR);
    lcd::fill_rect(0, BAR_H, lcd::W, lcd::H - BAR_H, BG);
}

pub struct Screen {
    last_unit: &'static str,
    last_mode: u8,
    last_auto: Option<(bool, bool)>,
    last_aux: [bool; 2],
}

fn mode_label(mode: u8) -> &'static str {
    match mode {
        0 | 2 | 4..=6 => "AC",
        1 | 3 | 7..=9 => "DC",
        10 => "",
        11 => "\u{266a}",
        12 => "\u{25b6}",
        13 => "",
        14 => "",
        _ => "",
    }
}

impl Screen {
    pub const fn new() -> Self {
        Screen { last_unit: "", last_mode: 255, last_auto: None, last_aux: [false; 2] }
    }

    /// Force a full redraw of the reading area on the next reading.
    pub fn invalidate(&mut self) {
        self.last_unit = "";
        self.last_mode = 255;
        self.last_auto = None;
    }

    fn val_text<'a>(buf: &'a mut [u8; 16], v: &dmm::Val) -> &'a str {
        if v.ol { "0L" } else { dmm::format(buf, v.v, v.dec) }
    }

    pub fn reading(&mut self, r: &Reading) {
        let mut buf = [0u8; 16];
        let s = Self::val_text(&mut buf, &r.main);
        text::draw_right(&VALUE, VALUE_X0, VALUE_X1, VALUE_BASE, s, VALUE_RED, BG);

        if self.last_unit != r.main.unit || self.last_mode != r.mode {
            self.last_unit = r.main.unit;
            self.last_mode = r.mode;
            let end = text::draw(&UNIT, UNIT_X, UNIT_BASE, r.main.unit, VALUE_RED, BG);
            if end < lcd::W {
                lcd::fill_rect(end, UNIT_BASE - UNIT.ascent, lcd::W - end, UNIT.height(), BG);
            }
            let end = text::draw(&LABEL, 404, 96, mode_label(r.mode), FG, BG);
            lcd::fill_rect(end, 96 - LABEL.ascent, lcd::W - end, LABEL.height(), BG);
        }
        // Secondary readouts (aux1 left, aux2 right) under the main value.
        for (i, a) in [r.aux1, r.aux2].iter().enumerate() {
            let x0 = if i == 0 { 8 } else { 248 };
            match a {
                Some(v) => {
                    let mut b = [0u8; 16];
                    let t = Self::val_text(&mut b, v);
                    let mut line = [0u8; 32];
                    let n = t.len().min(20);
                    line[..n].copy_from_slice(&t.as_bytes()[..n]);
                    let u = v.unit.as_bytes();
                    let k = u.len().min(32 - n - 1);
                    line[n] = b' ';
                    line[n + 1..n + 1 + k].copy_from_slice(&u[..k]);
                    let s = core::str::from_utf8(&line[..n + 1 + k]).unwrap_or("");
                    text::draw_right(&LABEL, x0, x0 + 224, 175, s, FG, BG);
                    self.last_aux[i] = true;
                }
                None if self.last_aux[i] => {
                    lcd::fill_rect(x0, 175 - LABEL.ascent, 224, LABEL.height(), BG);
                    self.last_aux[i] = false;
                }
                None => {}
            }
        }
        if self.last_auto != Some((r.auto, r.auto_plus)) {
            self.last_auto = Some((r.auto, r.auto_plus));
            let s = if !r.auto { "MAN   " } else if r.auto_plus { "AUTO+ " } else { "AUTO  " };
            text::draw(&LABEL, 4, 27, s, FG, BAR);
        }
    }

    /// Temporary: show the last gesture at the bottom and mark touches.
    pub fn gesture_debug(&mut self, g: &crate::gesture::Gesture) {
        use crate::gesture::Gesture as G;
        let mut b = [0u8; 40];
        let mut n = 0;
        let mut put = |s: &[u8]| {
            let k = s.len().min(b.len() - n);
            b[n..n + k].copy_from_slice(&s[..k]);
            n += k;
        };
        let num = |v: u16, t: &mut [u8; 16]| -> usize {
            let s = dmm::format(t, v as f32, 0);
            s.len()
        };
        let mut t = [0u8; 16];
        match *g {
            G::Down(p) | G::Tap(p) | G::LongPress(p) | G::Drag(_, p) => {
                put(match *g {
                    G::Down(_) => b"down ",
                    G::Tap(_) => b"tap ",
                    G::LongPress(_) => b"long ",
                    _ => b"drag ",
                });
                let k = num(p.x, &mut t);
                put(&t[..k]);
                put(b",");
                let k = num(p.y, &mut t);
                put(&t[..k]);
                if p.x < lcd::W - 4 && p.y < lcd::H - 4 {
                    lcd::fill_rect(p.x.saturating_sub(2), p.y.saturating_sub(2), 5, 5, 0xFFE0);
                }
            }
            G::SwipeLeft => put(b"swipe left"),
            G::SwipeRight => put(b"swipe right"),
            G::SwipeUp => put(b"swipe up"),
            G::SwipeDown => put(b"swipe down"),
            G::KeyDown(c) | G::KeyUp(c) => {
                put(if matches!(*g, G::KeyDown(_)) { b"key down " } else { b"key up " });
                put(&[b'0' + c]);
            }
        }
        put(b"            ");
        text::draw(&LABEL, 4, 312, core::str::from_utf8(&b[..n]).unwrap_or(""), FG, BG);
    }
}
