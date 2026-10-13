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

/// Battery on the scope page top bar (stock (435,1,40,16)).
pub fn scope_battery(bars: u8, charging: bool) {
    let g = char::from_u32(0x22 + bars.min(5) as u32).unwrap_or('"');
    let mut b = [0u8; 4];
    sf::label(sf::BATTERY, rect(435, 1, 40, 18), g.encode_utf8(&mut b), if charging { 0x04A9 } else { STATUS_FG }, S_BAR, 8, 0);
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
// Stock scope page geometry (scope_gen_ui_v2_6.h section 6).

pub const S_BAR: Color = 0x4208;
const S_FRAME: Color = 0xAD55;
const S_FILL: Color = 0x2124;
const S_GRID: Color = 0x9492; // white blended 50 % into the fill
const S_PURPLE: Color = 0xA81F;
const S_ORANGE: Color = 0xFAA0;

/// Graph placement: `x0, y0` of frame column / row 0, height in pixels for the
/// 280-row model (stock: frame (13,20,401,281), 1:1 rows).
pub struct ScopeView {
    pub x0: u16,
    pub y0: u16,
    pub h: u16,
}

pub const SCOPE_FULL: ScopeView = ScopeView { x0: 13, y0: 20, h: 281 };
pub const SCOPE_COMPACT: ScopeView = ScopeView { x0: 13, y0: 98, h: 178 };

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
    let dec = if x.abs() >= 100.0 || (x - (x as i32) as f32).abs() < 1e-3 {
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

fn s_btn(y: i16, text: &str, fg: Color) {
    sf::rbox(rect(417, y, 60, 40), 6, 0x2966, S_BAR);
    sf::glyphs(sf::F14, rect(417, y, 60, 40), text, fg, 0x2966, 8, 1);
}

pub const SCOPE_RIGHT: [(i16, &str); 7] =
    [(20, "+"), (61, ""), (102, "-"), (150, "+"), (191, ""), (232, "-"), (280, "TRIG")];

impl ScopeView {
    fn row_px(&self, r: i32) -> u16 {
        self.y0 + 1 + ((r.clamp(0, 278) as u32 * (self.h as u32 - 3)) / 278) as u16
    }

    pub fn draw_static(&self, sc: &scope::Scope, full: bool) {
        if full {
            lcd::fill_rect(0, 0, lcd::W, 20, S_BAR);
            lcd::fill_rect(0, 20, 13, 300, S_BAR);
            lcd::fill_rect(414, 20, 66, 300, S_BAR);
            lcd::fill_rect(0, 301, 414, 19, S_BAR);
        }
        self.labels(sc, full);
        for i in 0..scope::COLS as u16 {
            self.column(i, None, None);
        }
    }

    /// Top bar / right panel labels.
    pub fn labels(&self, sc: &scope::Scope, full: bool) {
        let s = &sc.s;
        let mut b = [0u8; 24];
        let (status, sc_col) = if !s.run {
            ("STOP", S_ORANGE)
        } else {
            match s.mode {
                scope::TrigMode::Auto => ("AUTO", S_PURPLE),
                scope::TrigMode::Normal => ("WAIT", S_PURPLE),
                scope::TrigMode::Single => ("T'D", S_PURPLE),
            }
        };
        let (y, bg) = if full { (1i16, S_BAR) } else { (self.y0 as i16 - 19, BG) };
        sf::label(sf::F14, rect(13, y, 62, 17), status, sc_col, bg, 1, 1);
        let n = eng(&mut b, scope::SDIV[s.tb], "s");
        sf::label(sf::F14, rect(79, y, 62, 17), core::str::from_utf8(&b[..n]).unwrap_or(""), FG, bg, 1, 1);
        let n = eng(&mut b, scope::VDIV[s.range], if s.ac { "V~" } else { "V" });
        sf::label(sf::F14, rect(146, y, 62, 17), core::str::from_utf8(&b[..n]).unwrap_or(""), 0xFFE0, bg, 1, 1);
        let n = eng(&mut b, sc.trig_volts(), "V");
        let mut t = [0u8; 28];
        t[0] = if s.rising { b'/' } else { b'\\' };
        t[1..1 + n].copy_from_slice(&b[..n]);
        sf::label(sf::F14, rect(212, y, 80, 17), core::str::from_utf8(&t[..1 + n]).unwrap_or(""), FG, bg, 1, 1);
        if full {
            let n = eng(&mut b, scope::VDIV[s.range], "V");
            for (yy, txt) in SCOPE_RIGHT {
                let lbl = match yy {
                    61 => core::str::from_utf8(&b[..n]).unwrap_or(""),
                    191 => "Time",
                    _ => txt,
                };
                let fg = if yy < 140 { 0xFFE0 } else if yy < 270 { S_PURPLE } else { FG };
                s_btn(yy, lbl, fg);
            }
        }
    }

    /// One graph column `i` (0..399): fill, grid, trace segment (rows), and
    /// the trigger level tick on the left edge.
    fn column(&self, i: u16, seg: Option<(i32, i32)>, trig: Option<i32>) {
        let x = self.x0 + i;
        let h = self.h;
        let seg = seg.map(|(a, b)| (self.row_px(a) - self.y0, self.row_px(b) - self.y0));
        let trig = trig.map(|t| self.row_px(t) - self.y0);
        let vline = i % 40 == 0;
        let centre_v = i == 200;
        lcd::set_window(x, self.y0, x, self.y0 + h - 1);
        for y in 0..h {
            // Rows of the 280-row model for the horizontal grid lines.
            let r = (y as u32 * 278 / (h as u32 - 3).max(1)) as u16;
            let hline = r % 35 == 0 && y > 0 && y < h - 1;
            let mut c = S_FILL;
            if i == 0 || i == 400 || y == 0 || y == h - 1 {
                c = S_FRAME;
            } else if (vline && y % 5 == 0) || (hline && i % 8 == 0) || (centre_v && y % 5 == 0) {
                c = S_GRID;
            }
            if let Some((a, b)) = seg {
                if y >= a && y <= b && i > 0 {
                    c = 0xFFE0;
                }
            }
            if trig == Some(y) && i < 6 {
                c = S_ORANGE;
            }
            lcd::data(c);
        }
    }

    /// Draw a frame: 400 columns of min/max (peak detect), widened to touch
    /// the previous column, as the stock trace.
    pub fn frame(&self, f: &scope::Frame, sc: &scope::Scope) {
        let n = f.data.len();
        let trig_row = Some(sc.s.trig_row);
        let row = |code: u16| -> i32 { sc.row_of_code(code as f32) as i32 };
        let (mut pa, mut pb) = (0i32, 0i32);
        for i in 0..scope::COLS {
            let (mut lo, mut hi);
            if n >= scope::COLS {
                let a = i * n / scope::COLS;
                let b = ((i + 1) * n / scope::COLS).max(a + 1);
                lo = i32::MAX;
                hi = i32::MIN;
                for &v in &f.data[a..b.min(n)] {
                    let r = row(v);
                    lo = lo.min(r);
                    hi = hi.max(r);
                }
            } else {
                // Fewer samples than columns: linear interpolation.
                let pos = i as f32 * (n - 1) as f32 / (scope::COLS - 1) as f32;
                let k = pos as usize;
                let fr = pos - k as f32;
                let v0 = f.data[k] as f32;
                let v1 = f.data[(k + 1).min(n - 1)] as f32;
                let r = sc.row_of_code(v0 + (v1 - v0) * fr) as i32;
                lo = r;
                hi = r;
            }
            if i > 0 {
                lo = lo.min(pb);
                hi = hi.max(pa);
            }
            pa = lo;
            pb = hi;
            let (a, b) = (lo.min(hi), lo.max(hi));
            self.column(i as u16, Some((a, b)), trig_row);
        }
    }
}

/// Measurements of a frame: Vpp, mean, RMS (V) and frequency (Hz).
pub fn measure(f: &scope::Frame, sc: &scope::Scope) -> (f32, f32, f32, f32) {
    let (mut lo, mut hi, mut sum, mut sq) = (u16::MAX, 0u16, 0f32, 0f32);
    for &v in f.data {
        lo = lo.min(v);
        hi = hi.max(v);
        let x = sc.volts(v as f32);
        sum += x;
        sq += x * x;
    }
    let n = f.data.len() as f32;
    let mean = sum / n;
    let rms = {
        let m = sq / n;
        let mut y = if m > 0.0 { f32::from_bits((m.to_bits() >> 1) + 0x1FBD_1DF5) } else { 0.0 };
        if m > 0.0 {
            for _ in 0..3 {
                y = 0.5 * (y + m / y);
            }
        }
        y
    };
    let mid = (lo as u32 + hi as u32) / 2;
    // Hysteresis: 10 % of the swing; no frequency below 20 codes of swing.
    let hy = ((hi - lo) as u32 / 10).max(2);
    let (mut first, mut last, mut count) = (None, 0usize, 0u32);
    let mut below = f.data[0] as u32 <= mid;
    for (i, &v) in f.data.iter().enumerate() {
        if below && v as u32 > mid + hy {
            below = false;
            if first.is_none() {
                first = Some(i);
            } else {
                count += 1;
            }
            last = i;
        } else if !below && (v as u32) + hy < mid {
            below = true;
        }
    }
    let freq = match first {
        Some(a) if count > 0 && hi > lo + 20 => count as f32 * f.fs as f32 / (last - a) as f32,
        _ => 0.0,
    };
    (sc.volts(hi as f32) - sc.volts(lo as f32), mean, rms, freq)
}

/// Bottom measurement cells (stock: x 13/113/213/313, 98x17).
pub fn scope_measurements(vpp: f32, mean: f32, rms: f32, freq: f32) {
    let items: [(&str, f32, &str); 4] = [("Vpp:", vpp, "V"), ("Freq:", freq, "Hz"), ("Vavg:", mean, "V"), ("Vrms:", rms, "V")];
    for (i, (name, v, u)) in items.iter().enumerate() {
        let x = 13 + 100 * i as i16;
        let mut b = [0u8; 24];
        let n = eng(&mut b, *v, u);
        let mut t = [0u8; 32];
        let k = name.len();
        t[..k].copy_from_slice(name.as_bytes());
        t[k..k + n].copy_from_slice(&b[..n]);
        lcd::fill_rect(x as u16, 302, 98, 1, S_FRAME);
        sf::label(sf::F14, rect(x, 303, 98, 16), core::str::from_utf8(&t[..k + n]).unwrap_or(""), 0xFFE0, S_BAR, 1, 1);
    }
}

// ------------------------------------------------------------ generator
// Stock generator page (scope_gen_ui_v2_6.h section 6).

const G_BG: Color = 0x10A3;
const G_BAR: Color = 0x2146;
const G_BOX: Color = 0x3A08;
const G_TRACE: Color = 0x04A9;

pub fn gen_static(g: &gen::Gen) {
    lcd::fill_rect(0, 0, lcd::W, 36, G_BAR);
    lcd::fill_rect(0, 36, lcd::W, lcd::H - 36, G_BG);
    for (y, name) in [(40i16, "Frequency"), (133, "Vpp"), (226, "Duty")] {
        sf::rbox(rect(246, y, 230, 90), 3, G_BOX, G_BG);
        sf::glyphs(sf::TEXT22, rect(257, y + if y == 226 { 0 } else { 8 }, 140, 26), name, STATUS_FG, G_BOX, 1, 0);
    }
    gen_values(g);
}

fn gen_preview(w: gen::Wave) {
    // Two periods over 238 points between h/4 and 3h/4, as the stock preview.
    let (x0, y0, ww, hh) = (4u16, 85u16, 239u16, 231u16);
    let sel = match w {
        gen::Wave::Sine => 0,
        gen::Wave::Ramp => 1,
        gen::Wave::Triangle => 2,
        gen::Wave::Square => 3,
    };
    let yv = |i: u16| -> u16 {
        let ph = (i as f32 * 2.0 / 238.0) % 1.0;
        let v = match sel {
            0 => 0.5 + 0.5 * libsin(ph * 2.0 * core::f32::consts::PI),
            1 => ph,
            2 => if ph < 0.5 { ph * 2.0 } else { 2.0 - ph * 2.0 },
            _ => if ph < 0.5 { 1.0 } else { 0.0 },
        };
        y0 + hh * 3 / 4 - (v * (hh / 2) as f32) as u16
    };
    let mut prev = yv(0);
    for i in 0..ww {
        let y = yv(i.min(237));
        let (a, b) = if y < prev { (y, prev) } else { (prev, y) };
        prev = y;
        lcd::set_window(x0 + i, y0, x0 + i, y0 + hh - 1);
        for yy in 0..hh {
            let edge = i == 0 || i == ww - 1 || yy == 0 || yy == hh - 1;
            let grid = (i % 48 == 0 && i > 0) || (yy % 46 == 0 && yy > 0);
            let c = if y0 + yy >= a && y0 + yy <= b {
                G_TRACE
            } else if edge {
                0x73CF
            } else if grid {
                G_BOX
            } else {
                G_BG
            };
            lcd::data(c);
        }
    }
}

fn libsin(x: f32) -> f32 {
    const PI: f32 = core::f32::consts::PI;
    let mut x = x;
    while x > PI {
        x -= 2.0 * PI;
    }
    let x = if x > PI / 2.0 { PI - x } else if x < -PI / 2.0 { -PI - x } else { x };
    let x2 = x * x;
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0)))
}

pub fn gen_values(g: &gen::Gen) {
    let s = &g.s;
    // Top bar: RUN/STOP (output state).
    sf::label(sf::F21, rect(0, 0, 84, 36), if s.on { "RUN" } else { "STOP" }, if s.on { 0x07E0 } else { STATUS_FG }, G_BAR, 8, 0);
    // Wave drop-down.
    sf::rbox(rect(4, 40, 239, 42), 3, G_BOX, G_BG);
    sf::glyphs(sf::TEXT22, rect(14, 40, 180, 42), s.wave.name(), STATUS_FG, G_BOX, 1, 0);
    sf::glyphs(sf::Font(0x0805_9428), rect(197, 40, 42, 42), " ", STATUS_FG, G_BOX, 8, 0);
    gen_preview(s.wave);
    // Values.
    let f = s.freq_hz as f32;
    let (fv, fu) = if f >= 1e6 { (f / 1e6, "MHz") } else if f >= 1e3 { (f / 1e3, "KHz") } else { (f, "Hz") };
    let mut t = [0u8; 16];
    let fs = crate::dmm::format(&mut t, fv, if fv >= 100.0 { 0 } else if fv >= 10.0 { 1 } else { 2 });
    sf::label(sf::LETTERS, rect(262, 83, 150, 30), fs, STATUS_FG, G_BOX, 4, 0);
    sf::label(sf::TEXT22, rect(421, 90, 50, 26), fu, STATUS_FG, G_BOX, 1, 0);
    let mut t = [0u8; 16];
    let a = crate::dmm::format(&mut t, s.amp_v, 1);
    sf::label(sf::LETTERS, rect(262, 173, 150, 30), a, STATUS_FG, G_BOX, 4, 0);
    sf::label(sf::TEXT22, rect(421, 176, 50, 26), "V", STATUS_FG, G_BOX, 1, 0);
    let mut t = [0u8; 16];
    let d = crate::dmm::format(&mut t, s.duty as f32 / 100.0, 1);
    let duty_ok = s.wave == gen::Wave::Square && s.freq_hz <= 50_000;
    let fg = if duty_ok { STATUS_FG } else { 0x7BEF };
    sf::label(sf::LETTERS, rect(262, 266, 150, 30), d, fg, G_BOX, 4, 0);
    sf::label(sf::TEXT22, rect(421, 269, 50, 26), "%", fg, G_BOX, 1, 0);
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
