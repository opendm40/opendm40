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
    last_auto: Option<bool>,
}

impl Screen {
    pub const fn new() -> Self {
        Screen { last_unit: "", last_auto: None }
    }

    pub fn reading(&mut self, r: &Reading) {
        let mut buf = [0u8; 16];
        let s = if r.overload { "OL" } else { dmm::format(&mut buf, r.value, r.decimals) };
        text::draw_right(&VALUE, VALUE_X0, VALUE_X1, VALUE_BASE, s, VALUE_RED, BG);

        if self.last_unit != r.unit {
            self.last_unit = r.unit;
            let end = text::draw(&UNIT, UNIT_X, UNIT_BASE, r.unit, VALUE_RED, BG);
            if end < lcd::W {
                lcd::fill_rect(end, UNIT_BASE - UNIT.ascent, lcd::W - end, UNIT.height(), BG);
            }
            text::draw(&LABEL, 404, 96, r.mode.label(), FG, BG);
        }
        if self.last_auto != Some(r.auto) {
            self.last_auto = Some(r.auto);
            text::draw(&LABEL, 4, 27, if r.auto { "AUTO  " } else { "MAN   " }, FG, BAR);
        }
    }
}
