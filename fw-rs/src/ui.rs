//! Screens. Geometry and colours of the multimeter page measured on read-backs
//! of the stock screen (fw-rs/ref/); the main reading is red instead of the
//! stock orange. No framebuffer: every element is drawn as complete cells /
//! columns so redraws do not flicker.

use crate::dmm::{Button, Reading};
use crate::fonts::{LABEL, SMALL, UNIT, VALUE};
use crate::gen;
use crate::lcd::{self, Color};
use crate::meter;
use crate::scope;
use crate::text;

/// RGB565 of the stock colours (panel reads back RGB666).
pub const BG: Color = 0x0883; // (8, 16, 24)
pub const BAR: Color = 0x1904; // (24, 32, 32) status bar
pub const FG: Color = 0xF79E; // (240, 244, 240) text
pub const DIM: Color = 0x94D3; // (144, 152, 152) grid / frames
pub const BTN: Color = 0x2966; // (40, 44, 48) buttons / boxes
pub const SEL: Color = 0x0419; // (0, 128, 200) selected button
/// Main reading: red (the one intended difference from stock orange).
pub const VALUE_RED: Color = 0xF145;
pub const TRACE: Color = 0xFFE0;
pub const TRIG: Color = 0xFC40;

pub const BAR_H: u16 = 34;
const VALUE_X1: u16 = 396; // right edge of the last digit
const VALUE_BASE: u16 = 146;
const UNIT_X: u16 = 404;
const UNIT_BASE: u16 = 144;
pub const BTN_Y: u16 = 280;
pub const BTN_H: u16 = 40;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dmm,
    Scope,
    Gen,
    /// Multimeter reading on top, scope below.
    DmmScope,
    Menu,
}

impl View {
    /// Views reachable by swiping, in order.
    pub const CYCLE: [View; 4] = [View::Dmm, View::DmmScope, View::Scope, View::Gen];
}

fn bar(title: &str) {
    lcd::fill_rect(0, 0, lcd::W, BAR_H, BAR);
    text::draw(&LABEL, 4, 27, title, FG, BAR);
}

pub fn button_row(labels: &[&str], selected: Option<usize>) {
    let n = labels.len() as u16;
    let w = lcd::W / n;
    lcd::fill_rect(0, BTN_Y - 2, lcd::W, BTN_H + 2, BG);
    for (i, l) in labels.iter().enumerate() {
        let x = i as u16 * w;
        let c = if Some(i) == selected { SEL } else { BTN };
        lcd::fill_rect(x + 2, BTN_Y, w - 4, BTN_H - 2, c);
        let tw = LABEL.width(l);
        text::draw(&LABEL, x + (w - tw.min(w)) / 2, BTN_Y + 29, l, FG, c);
    }
}

/// Which of `n` bottom buttons a touch at (x, y) hits.
pub fn button_hit(n: usize, x: u16, y: u16) -> Option<usize> {
    if y >= BTN_Y && n > 0 {
        Some(((x as usize * n) / lcd::W as usize).min(n - 1))
    } else {
        None
    }
}

pub const DMM_BUTTONS: [&str; 6] = ["V", "A", "\u{3a9}", "Cap", "Diode", "Hz/\u{b0}C"];

fn button_index(b: Button) -> usize {
    Button::ALL.iter().position(|&x| x == b).unwrap_or(0)
}

fn sub_label(mode: u8) -> &'static str {
    match mode {
        0 | 2 | 4..=6 => "AC",
        1 | 3 | 7..=9 => "DC",
        11 => "Cont",
        12 => "Diode",
        _ => "",
    }
}

fn val_line<'a>(buf: &'a mut [u8; 40], v: &crate::dmm::Val) -> &'a str {
    let mut t = [0u8; 16];
    let s = meter::text(&mut t, v);
    let n = s.len().min(20);
    buf[..n].copy_from_slice(&s.as_bytes()[..n]);
    buf[n] = b' ';
    let u = v.unit.as_bytes();
    let k = u.len().min(40 - n - 1);
    buf[n + 1..n + 1 + k].copy_from_slice(&u[..k]);
    core::str::from_utf8(&buf[..n + 1 + k]).unwrap_or("")
}

// ------------------------------------------------------------------ DMM
// Stock layout (gui_v2_6.h section 4), drawn with the stock fonts from flash.

use crate::stockfont::{self as sf, rect};

const STATUS_FG: Color = 0xF7BE;
/// Main-readout cell widths per format (%7.4f .. %7.1f, "   0L  ").
const CELLS: [[u8; 7]; 5] = [
    [40, 60, 20, 60, 60, 60, 60],
    [40, 60, 60, 20, 60, 60, 60],
    [40, 60, 60, 60, 20, 60, 60],
    [40, 60, 60, 60, 60, 20, 60],
    [40, 52, 52, 52, 60, 60, 40],
];

/// Mode glyph above the unit: 0x04 DC, 0x05 AC, 0x07 continuity, 0x08 diode.
fn mode_glyph(mode: u8) -> &'static str {
    match mode {
        0 | 2 | 4..=6 => "\u{5}",
        1 | 3 | 7..=9 => "\u{4}",
        11 => "\u{7}",
        12 => "\u{8}",
        _ => "",
    }
}

/// "%7.<dec>f" (right aligned in 7 characters), or the stock "   0L  ".
fn fmt7<'a>(buf: &'a mut [u8; 16], v: &crate::dmm::Val) -> (&'a str, usize) {
    if v.ol {
        buf[..7].copy_from_slice(b"   0L  ");
        return (core::str::from_utf8(&buf[..7]).unwrap_or(""), 4);
    }
    let mut t = [0u8; 16];
    let s = crate::dmm::format(&mut t, v.v, v.dec);
    let n = s.len().min(7);
    let pad = 7 - n;
    buf[..pad].fill(b' ');
    buf[pad..7].copy_from_slice(&s.as_bytes()[s.len() - n..]);
    let f = match v.dec {
        4.. => 0,
        3 => 1,
        2 => 2,
        _ => 3,
    };
    (core::str::from_utf8(&buf[..7]).unwrap_or(""), f)
}

pub struct DmmView {
    last_unit: &'static str,
    last_mode: u8,
    last_top: Option<(bool, bool, bool)>,
    last_aux: [bool; 2],
}

/// Bottom button `i` (0..5) with its stock glyphs.
fn dmm_button(i: usize, selected: bool, mode: u8) {
    let x = 2 + 80 * i as i16;
    let c = if selected { SEL } else { BTN };
    sf::rbox(rect(x, BTN_Y as i16, 76, 40), 8, c, BG);
    let ac = matches!(mode, 0 | 2 | 4..=6);
    let sub = if ac { "\u{5}" } else { "\u{4}" };
    let r = |dx: i16, dy: i16, w: i16| rect(x + dx, BTN_Y as i16 + dy, w, 40);
    match i {
        0 => {
            sf::glyphs(sf::LETTERS, r(0, 0, 36), "V", STATUS_FG, c, 4, 0);
            sf::glyphs(sf::BTN_GLYPH, r(40, 0, 30), if mode <= 3 { sub } else { "\u{4}" }, STATUS_FG, c, 1, 0);
        }
        1 => {
            sf::glyphs(sf::LETTERS, r(0, 0, 36), "A", STATUS_FG, c, 4, 0);
            sf::glyphs(sf::BTN_GLYPH, r(40, 0, 30), if (4..=9).contains(&mode) { sub } else { "\u{4}" }, STATUS_FG, c, 1, 0);
        }
        2 => sf::glyphs(sf::BTN_GLYPH, r(0, -1, 76), "\u{3a9}", STATUS_FG, c, 8, 0),
        3 => sf::glyphs(sf::BTN_GLYPH, r(0, 0, 76), "\u{11}", STATUS_FG, c, 8, 0),
        4 => {
            sf::glyphs(sf::BTN_GLYPH, r(4, -1, 33), "\u{8}", STATUS_FG, c, 8, 0);
            sf::glyphs(sf::BTN_GLYPH, r(42, 0, 33), "\u{7}", STATUS_FG, c, 1, 0);
        }
        _ => {
            sf::glyphs(sf::BTN_GLYPH, r(3, -2, 36), "\u{10}", STATUS_FG, c, 8, 0);
            sf::glyphs(sf::BTN_GLYPH, r(40, 0, 30), "\u{2103}", STATUS_FG, c, 8, 0);
        }
    }
}

impl DmmView {
    pub const fn new() -> Self {
        DmmView { last_unit: "", last_mode: 255, last_top: None, last_aux: [false; 2] }
    }

    pub fn draw_static(&mut self, button: Button, mode: u8) {
        lcd::fill_rect(0, BAR_H, lcd::W, lcd::H - BAR_H, BG);
        lcd::fill_rect(0, 0, lcd::W, BAR_H, BAR);
        for i in 0..5i16 {
            sf::rbox(rect(5 + 95 * i, 153, 88, 19), 3, BAR, BG);
        }
        let sel = button_index(button);
        for i in 0..6 {
            dmm_button(i, i == sel, mode);
        }
        self.invalidate();
    }

    pub fn invalidate(&mut self) {
        self.last_unit = "";
        self.last_mode = 255;
        self.last_top = None;
    }

    pub fn reading(&mut self, r: &Reading, hold: bool, rel: bool, compact: bool) {
        let mut buf = [0u8; 16];
        let (s, f) = fmt7(&mut buf, &r.main);
        let mut ub = [0u8; 16];
        let unit = sf::unit_text(r.main.unit, &mut ub);
        if compact {
            sf::cells(sf::F78, rect(40, 40, 360, 54), s, &CELLS[f], VALUE_RED, BG);
            sf::label(sf::UNIT, rect(401, 56, 78, 32), unit, VALUE_RED, BG, 4, -1);
        } else {
            sf::cells(sf::F78, rect(40, 68, 360, 80), s, &CELLS[f], VALUE_RED, BG);
            if self.last_unit != r.main.unit || self.last_mode != r.mode {
                self.last_unit = r.main.unit;
                self.last_mode = r.mode;
                sf::label(sf::UNIT, rect(401, 114, 79, 32), unit, VALUE_RED, BG, 4, -1);
                sf::label(sf::MODE, rect(437, 76, 32, 32), mode_glyph(r.mode), VALUE_RED, BG, 7, 0);
            }
            // Aux readouts (stock: value right-aligned cells at y 40, unit after).
            for (i, a) in [r.aux1, r.aux2].iter().enumerate() {
                let (vx, ux) = if i == 0 { (68, 178) } else { (290, 400) };
                match a {
                    Some(v) => {
                        let mut t = [0u8; 16];
                        let txt = meter::text(&mut t, v);
                        let mut u = [0u8; 16];
                        sf::label(sf::LETTERS, rect(vx - 30, 36, 138, 30), txt, VALUE_RED, BG, 4, 0);
                        sf::label(sf::LETTERS, rect(ux, 36, 80, 30), sf::unit_text(v.unit, &mut u), VALUE_RED, BG, 1, 0);
                        self.last_aux[i] = true;
                    }
                    None if self.last_aux[i] => {
                        lcd::fill_rect((vx - 30) as u16, 36, (ux + 80 - vx + 30) as u16, 30, BG);
                        self.last_aux[i] = false;
                    }
                    None => {}
                }
            }
        }
        let top = (r.auto && !r.auto_plus, hold, r.auto_plus);
        if self.last_top != Some(top) {
            self.last_top = Some(top);
            let s = if !r.auto { "MANU" } else if r.auto_plus { "AUTO+" } else { "AUTO" };
            sf::label(sf::F21, rect(4, 1, 105, 32), s, STATUS_FG, BAR, 1, -1);
            sf::label(sf::F21, rect(115, 1, 71, 32), if hold { "HOLD" } else { "RUN" }, STATUS_FG, BAR, 8, 0);
            let _ = rel;
        }
    }
}

/// Right part of the status bar: uptime, lock, BT link, battery.
pub fn status_right(uptime_s: u32, bt: bool, bars: u8, charging: bool) {
    let mut t = [0u8; 8];
    let (h, m, sec) = (uptime_s / 3600 % 100, uptime_s / 60 % 60, uptime_s % 60);
    for (i, v) in [h, m, sec].iter().enumerate() {
        t[i * 3] = b'0' + (v / 10) as u8;
        t[i * 3 + 1] = b'0' + (v % 10) as u8;
        if i < 2 {
            t[i * 3 + 2] = b':';
        }
    }
    sf::label(sf::F21, rect(205, 5, 112, 24), core::str::from_utf8(&t).unwrap_or(""), STATUS_FG, BAR, 1, -1);
    sf::label(sf::LOCK, rect(335, 7, 24, 20), "\u{21}", STATUS_FG, BAR, 8, 0);
    sf::label(sf::SAVE_BT, rect(390, 0, 15, 32), if bt { "\u{21}" } else { "" }, STATUS_FG, BAR, 1, 0);
    let g = char::from_u32(0x22 + bars.min(5) as u32).unwrap_or('"');
    let mut b = [0u8; 4];
    sf::label(sf::BATTERY, rect(435, 9, 40, 16), g.encode_utf8(&mut b), if charging { 0x04A9 } else { STATUS_FG }, BAR, 8, 0);
}

/// Strip chart (stock frame (80,176,321,97)) with labels, rate button and
/// MAX / REL / MIN. `pts` are display values, oldest first.
pub const CHART_X: u16 = 80;
pub const CHART_W: u16 = 321;
const CHART_Y: u16 = 176;
const CHART_H: u16 = 97;

fn num_unit<'a>(b: &'a mut [u8; 32], v: f32, dec: u8, unit: &str) -> &'a str {
    let mut t = [0u8; 16];
    let s = crate::dmm::format(&mut t, v, dec);
    let mut u = [0u8; 16];
    let u = sf::unit_text(unit, &mut u);
    let n = s.len().min(16);
    b[..n].copy_from_slice(&s.as_bytes()[..n]);
    let k = u.len().min(32 - n);
    b[n..n + k].copy_from_slice(&u.as_bytes()[..k]);
    core::str::from_utf8(&b[..n + k]).unwrap_or("")
}

pub fn chart(pts: &[f32], dec: u8, rate: u8, max: Option<f32>, min: Option<f32>, rel: Option<f32>, unit: &str) {
    let mut step = 1.0f32;
    for _ in 0..dec {
        step /= 10.0;
    }
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for &p in pts {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    if pts.is_empty() {
        lo = 0.0;
        hi = 0.0;
    }
    lo -= 50.0 * step;
    hi += 50.0 * step;
    let span = (hi - lo).max(1e-12);
    let mut b = [0u8; 32];
    sf::label(sf::F14, rect(0, 173, 78, 18), num_unit(&mut b, hi, dec, unit), STATUS_FG, BG, 4, 2);
    sf::label(sf::F14, rect(0, 218, 78, 18), num_unit(&mut b, (hi + lo) / 2.0, dec, unit), STATUS_FG, BG, 4, 2);
    sf::label(sf::F14, rect(0, 259, 78, 18), num_unit(&mut b, lo, dec, unit), STATUS_FG, BG, 4, 2);
    let row = |v: f32| -> u16 {
        (((hi - v) / span) * (CHART_H - 3) as f32 + 1.0).clamp(1.0, (CHART_H - 2) as f32) as u16
    };
    let n = pts.len();
    let first = CHART_W as usize - n.min(CHART_W as usize);
    let mut prev: Option<u16> = None;
    for x in 0..CHART_W {
        let seg = if (x as usize) >= first {
            let y = row(pts[x as usize - first]);
            let s = match prev {
                Some(p) if p < y => (p, y),
                Some(p) => (y, p),
                None => (y, y),
            };
            prev = Some(y);
            Some(s)
        } else {
            None
        };
        lcd::set_window(CHART_X + x, CHART_Y, CHART_X + x, CHART_Y + CHART_H - 1);
        for y in 0..CHART_H {
            let edge = x == 0 || x == CHART_W - 1 || y == 0 || y == CHART_H - 1;
            let grid = (x % 16 == 0 && y % 4 == 0) || (y % 16 == 0 && x % 4 == 0);
            let mut c = if edge || grid { DIM } else { BG };
            if let Some((a, b2)) = seg {
                if y >= a && y <= b2 {
                    c = 0xFC40;
                }
            }
            lcd::data(c);
        }
    }
    // Rate button.
    let mut t = [0u8; 16];
    let s = crate::dmm::format(&mut t, rate as f32 * 160.0, 0);
    let mut l = [0u8; 16];
    let k = s.len();
    l[..k].copy_from_slice(s.as_bytes());
    l[k..k + 6].copy_from_slice(b"mS/div");
    sf::rbox(rect(2, 188, 76, 32), 8, BTN, BG);
    sf::glyphs(sf::F14, rect(2, 188, 76, 32), core::str::from_utf8(&l[..k + 6]).unwrap_or(""), STATUS_FG, BTN, 8, 2);
    // MAX / REL / MIN.
    sf::label(sf::F14, rect(403, 175, 72, 12), "MAX:", STATUS_FG, BG, 1, 2);
    sf::label(sf::F14, rect(407, 187, 72, 12), max.map(|v| num_unit(&mut b, v, dec, unit)).unwrap_or(""), STATUS_FG, BG, 1, 2);
    sf::rbox(rect(402, 205, 76, 38), 8, BTN, BG);
    sf::glyphs(sf::F14, rect(405, 207, 76, 19), "REL:", STATUS_FG, BTN, 1, 2);
    let mut b2 = [0u8; 32];
    let rv = num_unit(&mut b2, rel.unwrap_or(0.0), dec, unit);
    sf::glyphs(sf::F14, rect(405, 222, 76, 19), rv, STATUS_FG, BTN, 1, 2);
    sf::label(sf::F14, rect(403, 249, 72, 12), "MIN:", STATUS_FG, BG, 1, 2);
    sf::label(sf::F14, rect(407, 261, 72, 12), min.map(|v| num_unit(&mut b, v, dec, unit)).unwrap_or(""), STATUS_FG, BG, 1, 2);
}

// ---------------------------------------------------------------- scope

pub struct ScopeView {
    pub x0: u16,
    pub y0: u16,
    pub w: u16,
    pub h: u16,
    pub divs_y: u16,
}

pub const SCOPE_FULL: ScopeView = ScopeView { x0: 0, y0: 36, w: 400, h: 240, divs_y: 8 };
pub const SCOPE_COMPACT: ScopeView = ScopeView { x0: 0, y0: 96, w: 400, h: 180, divs_y: 6 };
pub const SCOPE_BUTTONS: [&str; 6] = ["T-", "T+", "V-", "V+", "Edge", "Mode"];

/// "1.25us" style engineering text.
fn eng(buf: &mut [u8; 24], v: f32, unit: &str) -> usize {
    let (k, p) = if v.abs() >= 1.0 || v == 0.0 {
        (1.0, "")
    } else if v.abs() >= 1e-3 {
        (1e3, "m")
    } else if v.abs() >= 1e-6 {
        (1e6, "u")
    } else {
        (1e9, "n")
    };
    let x = v * k;
    let dec = if x.abs() >= 100.0 {
        0
    } else if x.abs() >= 10.0 {
        1
    } else {
        2
    };
    let mut t = [0u8; 16];
    let s = crate::dmm::format(&mut t, x, dec);
    let mut n = 0;
    for part in [s.as_bytes(), p.as_bytes(), unit.as_bytes()] {
        let k = part.len().min(24 - n);
        buf[n..n + k].copy_from_slice(&part[..k]);
        n += k;
    }
    n
}

impl ScopeView {
    pub fn draw_static(&self, s: &scope::Settings, full: bool) {
        if full {
            bar("");
            lcd::fill_rect(0, BAR_H, lcd::W, lcd::H - BAR_H, BG);
            button_row(&SCOPE_BUTTONS, None);
        }
        self.labels(s);
        for x in 0..self.w {
            self.column(x, None, None);
        }
    }

    pub fn labels(&self, s: &scope::Settings) {
        let fs = scope::sample_rate(s.tb) as f32;
        let mut b = [0u8; 24];
        let y = if self.y0 > 60 { self.y0 - 6 } else { 27 };
        let bgc = if self.y0 > 60 { BG } else { BAR };
        lcd::fill_rect(0, y - SMALL.ascent - 4, 400, SMALL.height() + 4, bgc);
        let n = eng(&mut b, 40.0 / fs, "s/div");
        let e = text::draw(&SMALL, 4, y, core::str::from_utf8(&b[..n]).unwrap_or(""), FG, bgc);
        let n = eng(&mut b, scope::RANGES[s.range].0, "V/div");
        let e = text::draw(&SMALL, e + 16, y, core::str::from_utf8(&b[..n]).unwrap_or(""), FG, bgc);
        let mode = match s.mode {
            scope::TrigMode::Auto => "Auto",
            scope::TrigMode::Normal => "Norm",
            scope::TrigMode::Single => "Single",
        };
        let e = text::draw(&SMALL, e + 16, y, mode, FG, bgc);
        let e = text::draw(&SMALL, e + 8, y, if s.rising { "/" } else { "\\" }, TRIG, bgc);
        text::draw(&SMALL, e + 16, y, if s.run { "RUN" } else { "STOP" }, FG, bgc);
    }

    /// One column: background, grid dots, trace segment [lo, hi] (rows in the
    /// plot), trigger level marker.
    fn column(&self, x: u16, seg: Option<(u16, u16)>, trig_row: Option<u16>) {
        let gx = self.w / 10;
        let gy = self.h / self.divs_y;
        let on_vline = x % gx == 0;
        let edge_x = x == 0 || x == self.w - 1;
        lcd::set_window(self.x0 + x, self.y0, self.x0 + x, self.y0 + self.h - 1);
        for y in 0..self.h {
            let mut c = BG;
            if edge_x || y == 0 || y == self.h - 1 || (on_vline && y % 4 == 0) || (y % gy == 0 && x % 4 == 0) {
                c = DIM;
            }
            if trig_row == Some(y) && x < 6 {
                c = TRIG;
            }
            if let Some((lo, hi)) = seg {
                if y >= lo && y <= hi {
                    c = TRACE;
                }
            }
            lcd::data(c);
        }
    }

    /// Draw a frame centred on the trigger (or the middle).
    pub fn frame(&self, f: &scope::Frame, s: &scope::Settings) {
        let n = f.data.len();
        let center = f.trig.unwrap_or(n / 2);
        let start = center.saturating_sub(self.w as usize / 2).min(n - self.w as usize);
        let counts_per_div = scope::RANGES[s.range].2 * scope::RANGES[s.range].0;
        let px_per_count = (self.h / self.divs_y) as f32 / counts_per_div;
        let mid = self.h as f32 / 2.0;
        let row = |code: u16| -> u16 {
            let y = mid - (code as f32 - 512.0) * px_per_count;
            y.clamp(1.0, self.h as f32 - 2.0) as u16
        };
        let trig_row = Some(row(s.trig_level));
        let mut prev = row(f.data[start]);
        for x in 0..self.w {
            let y = row(f.data[start + x as usize]);
            let (lo, hi) = if y < prev { (y, prev) } else { (prev, y) };
            self.column(x, Some((lo, hi)), trig_row);
            prev = y;
        }
    }
}

/// Measurements of a frame: Vpp, mean (V), frequency (Hz) from crossings.
pub fn measure(f: &scope::Frame, sc: &scope::Scope) -> (f32, f32, f32) {
    let (mut lo, mut hi, mut sum) = (u16::MAX, 0u16, 0u32);
    for &v in f.data {
        lo = lo.min(v);
        hi = hi.max(v);
        sum += v as u32;
    }
    let mean = sum as f32 / f.data.len() as f32;
    let mid = (lo as u32 + hi as u32) / 2;
    let (mut first, mut last, mut count) = (None, 0usize, 0u32);
    let mut below = f.data[0] as u32 <= mid;
    for (i, &v) in f.data.iter().enumerate() {
        if below && v as u32 > mid + 2 {
            below = false;
            if first.is_none() {
                first = Some(i);
            } else {
                count += 1;
            }
            last = i;
        } else if !below && (v as u32) + 2 < mid {
            below = true;
        }
    }
    let freq = match first {
        Some(a) if count > 0 && hi > lo + 8 => count as f32 * f.fs as f32 / (last - a) as f32,
        _ => 0.0,
    };
    (sc.volts(hi as f32) - sc.volts(lo as f32), sc.volts(mean), freq)
}

pub fn scope_measurements(vpp: f32, mean: f32, freq: f32, y0: u16) {
    let mut b = [0u8; 24];
    let items: [(&str, f32, &str); 3] = [("Vpp", vpp, "V"), ("Avg", mean, "V"), ("Freq", freq, "Hz")];
    for (i, (name, v, u)) in items.iter().enumerate() {
        let y = y0 + 20 + i as u16 * 48;
        text::draw(&SMALL, 404, y, name, DIM, BG);
        let n = eng(&mut b, *v, u);
        lcd::fill_rect(404, y + 4, 76, SMALL.height(), BG);
        text::draw(&SMALL, 404, y + 4 + SMALL.ascent, core::str::from_utf8(&b[..n]).unwrap_or(""), FG, BG);
    }
}

// ------------------------------------------------------------ generator

pub const GEN_BUTTONS: [&str; 6] = ["Wave", "F-", "F+", "A-", "A+", "Out"];

pub fn gen_static() {
    bar("Generator");
    lcd::fill_rect(0, BAR_H, lcd::W, lcd::H - BAR_H, BG);
    button_row(&GEN_BUTTONS, None);
}

pub fn gen_values(g: &gen::Gen) {
    let s = &g.s;
    let mut b = [0u8; 24];
    let rows: [&str; 5] = ["Output", "Wave", "Freq", "Ampl", "Duty"];
    for (i, name) in rows.iter().enumerate() {
        let y = 80 + i as u16 * 40;
        text::draw(&LABEL, 20, y, name, DIM, BG);
        let mut t = [0u8; 16];
        let v: &str = match i {
            0 => {
                if s.on { "ON" } else { "OFF" }
            }
            1 => s.wave.name(),
            2 => {
                let n = eng(&mut b, s.freq_hz as f32, "Hz");
                core::str::from_utf8(&b[..n]).unwrap_or("")
            }
            3 => {
                let n = crate::dmm::format(&mut t, s.amp_v, 1).len();
                t[n] = b' ';
                t[n + 1] = b'V';
                core::str::from_utf8(&t[..n + 2]).unwrap_or("")
            }
            _ => {
                let n = crate::dmm::format(&mut t, s.duty as f32 / 100.0, 1).len();
                t[n] = b'%';
                core::str::from_utf8(&t[..n + 1]).unwrap_or("")
            }
        };
        let c = if i == 0 && s.on { VALUE_RED } else { FG };
        lcd::fill_rect(160, y - LABEL.ascent, 320, LABEL.height(), BG);
        text::draw(&LABEL, 160, y, v, c, BG);
    }
}

// ----------------------------------------------------------------- menu

pub const MENU_ITEMS: [&str; 6] = ["View", "Bluetooth", "Backlight", "Beep", "About", "Back"];

pub fn menu(view_name: &str, bt_on: bool, backlight: u8, beep: bool) {
    bar("Menu");
    lcd::fill_rect(0, BAR_H, lcd::W, lcd::H - BAR_H, BG);
    let mut t = [0u8; 16];
    for (i, name) in MENU_ITEMS.iter().enumerate() {
        let y = BAR_H + 6 + i as u16 * 46;
        lcd::fill_rect(8, y, lcd::W - 16, 40, BTN);
        text::draw(&LABEL, 20, y + 30, name, FG, BTN);
        let v: &str = match i {
            0 => view_name,
            1 => {
                if bt_on { "On" } else { "Off" }
            }
            2 => {
                let n = crate::dmm::format(&mut t, backlight as f32, 0).len();
                t[n] = b'%';
                core::str::from_utf8(&t[..n + 1]).unwrap_or("")
            }
            3 => {
                if beep { "On" } else { "Off" }
            }
            4 => "dm40-fw 0.2",
            _ => "",
        };
        let w = LABEL.width(v);
        text::draw(&LABEL, lcd::W - 24 - w, y + 30, v, FG, BTN);
    }
}

pub fn menu_hit(y: u16) -> Option<usize> {
    if y < BAR_H + 6 {
        return None;
    }
    let i = ((y - BAR_H - 6) / 46) as usize;
    if i < MENU_ITEMS.len() { Some(i) } else { None }
}

pub fn view_name(v: View) -> &'static str {
    match v {
        View::Dmm => "Multimeter",
        View::Scope => "Scope",
        View::Gen => "Generator",
        View::DmmScope => "DMM + Scope",
        View::Menu => "Menu",
    }
}
