//! Screens. Geometry and colours of the multimeter page measured on read-backs
//! of the stock screen (fw-rs/ref/); the main reading is red instead of the
//! stock orange. No framebuffer: every element is drawn as complete cells /
//! columns so redraws do not flicker.

use crate::dmm::{Button, Reading};
use crate::fonts::LABEL;
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
/// DMM readings (main value, extra digit, unit, mode glyph, aux values,
/// meter strip): the stock value colour GUI_C_VALUE 0xFC40 (RGB 248,136,0;
/// firmware/re/analysis/gui_v2_6.h:157/169/243, every DMM value label of the
/// stock layout table gui_v2_6.h:330-339 uses it). Owner request: back to
/// the stock colour (an earlier fw-rs build drew the reading red).
pub const VALUE: Color = 0xFC40;
pub const TRACE: Color = 0xFFE0;

pub const BAR_H: u16 = 34;
/// Left edge of the DMM status-bar items after the logo (4 + LOGO + 3).
const BAR_X: i16 = 4 + sf::LOGO + 3;
const VALUE_X1: u16 = 396; // right edge of the last digit
/// Extra-digit cell of the main readout (x, width); see DmmView::reading.
/// 375..402, before the limit badge (403..433, y 72..88) and the unit box
/// (from 404); 28 px = the widest half-size F78 digit ("4", 56 / 2).
const XT_X: u16 = 375;
const XT_W: u16 = 28;
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
    Settings,
    /// MF range page of the multimeter.
    Range,
    /// MF settings window of the scope.
    ScopeMenu,
    /// Generator value entry (frequency / Vpp / duty).
    Keypad,
    /// Device / About page (stock gui_v2_6.h section 6 "6.产品信息").
    Device,
    /// DMM AutoHold and Hi/Lo limits of the current function (MF).
    Limits,
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
/// (DMM page function row: 6 buttons at x 4 + 79 i, see DmmView::draw_static.)
pub fn button_hit(n: usize, x: u16, y: u16) -> Option<usize> {
    if y >= BTN_Y && n > 0 {
        Some((x.saturating_sub(2) as usize / 79).min(n - 1))
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
/// Main-readout cells with the extra digit (formats 0..3 only, never OL):
/// sign 30, point 18 (F78 '-' / '.' are 29 / 17 px), digits 57 (F78 digits
/// are <= 56 px wide): centred, every glyph keeps >= 1 px to its neighbours
/// = 333 px: 40..372, then the half-size digit cell XT_X..XT_X + XT_W
/// (375..402, >= 3 px after the last digit).
const CELLS_X: [[u8; 7]; 4] = [
    [30, 57, 18, 57, 57, 57, 57],
    [30, 57, 57, 18, 57, 57, 57],
    [30, 57, 57, 57, 18, 57, 57],
    [30, 57, 57, 57, 57, 18, 57],
];
/// AC+DC aux rows: aux1 = AC part, aux2 = DC part (AUX_TAG 0x05 AC / 0x04 DC).
const AUX_TAGS: [&str; 2] = ["\u{5}", "\u{4}"];
/// Full view: left edge of each aux row's tag slot and of its value box
/// (value right-aligned in AUX_VX..AUX_VX + 138; aux1's unit box ends at 231).
const AUX_TAG_X: [i16; 2] = [8, 232];
const AUX_VX: [i16; 2] = [38, 260];

/// Mode glyph above the unit: 0x04 DC, 0x05 AC, 0x07 continuity, 0x08 diode.
fn mode_glyph(mode: u8, acdc: bool) -> &'static str {
    match mode {
        0..=9 if acdc => "\u{6}",
        0 | 2 | 4..=6 => "\u{5}",
        1 | 3 | 7..=9 => "\u{4}",
        11 => "\u{7}",
        12 => "\u{8}",
        _ => "",
    }
}

/// "%7.<dec>f" (right aligned in 7 characters), or the stock "   0L  ".
/// With `ext` (Val::ext, not OL): the value is formatted with dec + 1
/// decimals and split: the 7-character main part plus the last digit, which
/// is drawn small after it (so the main digits are never rounded up past it).
fn fmt7<'a>(buf: &'a mut [u8; 16], v: &crate::dmm::Val, ext: bool) -> (&'a str, usize, Option<u8>) {
    if v.ol {
        buf[..7].copy_from_slice(b"   0L  ");
        return (core::str::from_utf8(&buf[..7]).unwrap_or(""), 4, None);
    }
    let ext = ext && v.ext && v.dec >= 1;
    let mut t = [0u8; 16];
    let full = crate::dmm::format(&mut t, v.v, v.dec + ext as u8);
    let (s, x) = if ext {
        let l = full.len();
        (&full[..l - 1], Some(full.as_bytes()[l - 1]))
    } else {
        (full, None)
    };
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
    (core::str::from_utf8(&buf[..7]).unwrap_or(""), f, x)
}

pub struct DmmView {
    /// Main readout as last drawn (text, cell format) for redrawing only the
    /// digits that changed.
    last_txt: [u8; 8],
    last_fmt: usize,
    /// Extra (small) digit as last drawn, 0 = none.
    last_ext: u8,
    last_unit: &'static str,
    last_mode: u8,
    last_top: Option<(bool, u8, bool)>,
    last_aux: [bool; 2],
    /// Temperature sensor name shown instead of AUTO / MANU (set by the app,
    /// `None` outside the temperature function).
    pub label: Option<&'static str>,
    last_label: Option<&'static str>,
    /// Limit badge as last drawn (meter::V_*), 255 = not drawn.
    last_lim: u8,
    /// Meter strip of the combined view, as last drawn.
    st: Strip,
}

/// Bottom button `i` (0..5) with its stock glyphs, in the box (x, y, 76, 40)
/// (DMM page: stock (2 + 80 i, BTN_Y); combined view: the function band).
fn dmm_button(i: usize, x: i16, y: i16, h: i16, outside: Color, selected: bool, mode: u8, acdc: bool) {
    let c = if selected { SEL } else { BTN };
    sf::rbox(rect(x, y, 76, h), 8, c, outside);
    let ac = matches!(mode, 0 | 2 | 4..=6);
    let sub = if acdc { "\u{6}" } else if ac { "\u{5}" } else { "\u{4}" };
    let r = |dx: i16, dy: i16, w: i16| rect(x + dx, y + dy, w, h);
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

/// Function band of the combined view (a tap on the meter strip opens it):
/// the DMM page's six function buttons over the bottom of the graph, from
/// the 1 px line at FUNC_Y (3 px under the panel's T- button) down to the
/// measurement bar.
pub const FUNC_Y: u16 = 258;

pub fn func_band(button: Button, mode: u8, acdc: bool) {
    lcd::fill_rect(0, FUNC_Y, lcd::W, MEAS_Y - FUNC_Y, S_BAR);
    lcd::fill_rect(0, FUNC_Y, lcd::W, 1, S_FRAME);
    let sel = button_index(button);
    for i in 0..6 {
        dmm_button(i, 4 + 79 * i as i16, FUNC_Y as i16 + 4, 36, S_BAR, i == sel, mode, acdc);
    }
}

/// Repaint the band area with the chrome colour (band closed; the graph
/// columns and the panel are redrawn by the caller).
pub fn band_clear() {
    lcd::fill_rect(0, FUNC_Y, lcd::W, MEAS_Y - FUNC_Y, S_BAR);
}

/// Whether a touch at row `y` is on the meter strip.
pub fn strip_hit(y: u16) -> bool {
    (y as i16) >= STRIP_Y && (y as i16) < STRIP_Y + STRIP_H
}

/// Function button (0..5) of the band under a tap, None outside the band.
pub fn func_band_hit(x: u16, y: u16) -> Option<usize> {
    if (FUNC_Y..MEAS_Y).contains(&y) { Some((x.saturating_sub(2) / 79).min(5) as usize) } else { None }
}

// ------------------------------------------------------------ meter strip
// Combined DMM+Scope view (docs/combined-view-design.md §2.3/§2.4, geometry
// reworked for the layout rules: every ink box >= 3 px from any other and
// inside its container, >= 4 px from the screen edges; checked on the host by
// tools/ui-render). Band rows 27..83 under the 27 px scope-family top bar:
//   badges  x 4..50, rows 30..44 / 48..62 / 66..80 (F14 in 47x15 pills)
//   value   F78 at half size ("F39"), right-aligned to x 233, rows 40..79,
//           its bottom on the unit / sub row 1 bottom
//   sign    AUX_TAG glyph above the unit, unit LETTERS (F21 for "F"/"n",
//           which LETTERS lacks), from x 237
//   sub rows LETTERS right-aligned to x 475, ink rows 31..52 and 57..78; the
//           AC+DC parts (in the main unit, which is not repeated) get their
//           AC / DC tag 4 px before the text.
// Widths (all measured by the host check): "-444.44" = 180 px (value field
// 54..233), "MHz" = 62 px (237..298), "444.444KΩ" (NTC) = 151 px.

/// Top of the meter strip (= the scope-family bar height) and its height.
pub const STRIP_Y: i16 = SBAR_H as i16;
pub const STRIP_H: i16 = 57;
/// Strip background; dark red at >= 100 V (stock lightning threshold).
const ST_BG_HV: Color = 0x4800;
/// Badge column: taps left of this x toggle REL (app.rs).
pub const STRIP_BADGE_X1: u16 = 54;
const BADGE_X: i16 = 4;
const BADGE_W: i16 = 47;
const BADGE_H: i16 = 15;
const BADGE_Y: [i16; 3] = [30, 48, 66];
const CHIP: Color = 0x3186;
const SV_X: i16 = 54;
const SV_R: i16 = 234;
const SV_BASE: i16 = 79;
const SU_X: i16 = 237;
const SU_W: i16 = 62;
const SS_R: i16 = 476;
const SS_Y: [i16; 2] = [29, 55];

/// Short temperature-sensor names for the strip badge (F14 <= 41 px; the
/// B-value is on the sensor page; dmm.rs T_NAMES order).
pub const T_SHORT: [&str; 7] = ["TC-K", "PT100", "PT1000", "NTC10k", "NTC10k", "N100k", "NTC-U"];

/// Strip badge contents (codes, so the cache is a byte compare).
const B_NONE: u8 = 0;
const B_AUTO: u8 = 1;
const B_AUTOP: u8 = 2;
const B_MANU: u8 = 3;
const B_HV: u8 = 4;
const B_HOLD: u8 = 5;
const B_AHOLD_WAIT: u8 = 6;
const B_AHOLD_HELD: u8 = 7;
const B_REL: u8 = 8;
const B_PASS: u8 = 9;
const B_FAIL: u8 = 10;
/// Temperature sensor k: B_SENSOR + k.
const B_SENSOR: u8 = 16;

fn badge(slot: usize, code: u8, bg: Color) {
    let r = rect(BADGE_X, BADGE_Y[slot], BADGE_W, BADGE_H);
    let (t, fg, fill) = match code {
        B_AUTO => ("AUTO", FG, CHIP),
        B_AUTOP => ("AUTO+", FG, CHIP),
        B_MANU => ("MANU", FG, CHIP),
        B_HV => ("HV", 0xFFFF, 0xC9A0),
        B_HOLD => ("HOLD", BG, VALUE),
        B_AHOLD_WAIT => ("A-HOLD", 0x04FC, CHIP),
        B_AHOLD_HELD => ("A-HOLD", BG, VALUE),
        B_REL => ("REL", FG, SEL),
        B_PASS => ("PASS", 0xFFFF, 0x04A9),
        B_FAIL => ("FAIL", 0xFFFF, 0xF800),
        c if c >= B_SENSOR => (T_SHORT[((c - B_SENSOR) as usize).min(6)], FG, CHIP),
        _ => {
            lcd::fill_rect(r.x as u16, r.y as u16, r.w as u16, r.h as u16, bg);
            return;
        }
    };
    sf::rbox(r, 4, fill, bg);
    // F14 caps sit 1 px high in their line: centre the ink.
    sf::glyphs(sf::F14, rect(r.x, r.y + 1, r.w, r.h), t, fg, fill, 8, 1);
}

/// What the strip last drew (see DmmView::strip).
#[derive(Clone, Copy)]
struct Strip {
    /// Static part (band background, HV colour) drawn; false = full redraw.
    drawn: bool,
    hv: u8,
    value: [u8; 8],
    unit: (&'static str, &'static str),
    sub: [[u8; 24]; 2],
    /// Left edge of each sub row as drawn (SS_R = empty).
    sub_x: [i16; 2],
    badge: [u8; 3],
}

impl Strip {
    const fn new() -> Self {
        Strip { drawn: false, hv: 0, value: [0; 8], unit: ("", ""), sub: [[0; 24]; 2], sub_x: [SS_R; 2], badge: [255; 3] }
    }
}

/// Copy `parts` into `buf` (truncated), returning the text.
fn join<'a>(buf: &'a mut [u8; 24], parts: &[&str]) -> &'a str {
    let mut n = 0;
    for p in parts {
        let k = p.len().min(24 - n);
        buf[n..n + k].copy_from_slice(&p.as_bytes()[..k]);
        n += k;
    }
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}

/// A unit in the strip font: LETTERS, or F21 when LETTERS lacks a glyph of
/// it ("nF", "uF": no 'n' / 'F' in LETTERS). Returns the font and the y
/// offset that puts its caps on the same baseline.
fn unit_font(u: &str) -> (sf::Font, i16) {
    if u.chars().all(|c| sf::LETTERS.glyph(c as u32).is_some()) { (sf::LETTERS, 0) } else { (sf::F21, 3) }
}

impl DmmView {
    pub const fn new() -> Self {
        DmmView {
            last_txt: [0; 8],
            last_fmt: 255,
            last_ext: 0,
            last_unit: "",
            last_mode: 255,
            last_top: None,
            last_aux: [false; 2],
            label: None,
            last_label: None,
            last_lim: 255,
            st: Strip::new(),
        }
    }

    /// Static part of the standalone DMM page.
    pub fn draw_static(&mut self, button: Button, mode: u8, acdc: bool) {
        lcd::fill_rect(0, BAR_H, lcd::W, lcd::H - BAR_H, BG);
        lcd::fill_rect(0, 0, lcd::W, BAR_H, BAR);
        sf::logo(4, 7, BAR);
        for i in 0..5i16 {
            sf::rbox(rect(5 + 95 * i, 153, 88, 19), 3, BAR, BG);
        }
        let sel = button_index(button);
        // 36 px tall (rows 280..315) and from x 4: 4 px off the screen edges.
        for i in 0..6 {
            dmm_button(i, 4 + 79 * i as i16, BTN_Y as i16, 36, BG, i == sel, mode, acdc);
        }
        self.invalidate();
    }

    pub fn invalidate(&mut self) {
        self.last_label = Some("");
        self.last_fmt = 255;
        self.last_unit = "";
        self.last_mode = 255;
        self.last_top = None;
        self.last_lim = 255;
        self.st.drawn = false;
    }

    /// Draw a reading on the standalone DMM page. `hold`: Meter::hold_state
    /// (0 RUN, 1 HOLD, 2 AutoHold tracking, 3 AutoHold captured); `lim`:
    /// Meter::verdict.
    pub fn reading(&mut self, r: &Reading, hold: u8, lim: u8) {
        let mut buf = [0u8; 16];
        let (s, f, ext) = fmt7(&mut buf, &r.main, true);
        let mut ub = [0u8; 16];
        let unit = sf::unit_text(r.main.unit, &mut ub);
        // Only the changed digits are redrawn (cell layout unchanged).
        let same_layout = self.last_fmt == f && (self.last_ext != 0) == ext.is_some();
        let prev_txt = self.last_txt;
        let prev = if same_layout { core::str::from_utf8(&prev_txt[..7]).unwrap_or("") } else { "" };
        let n = s.len().min(7);
        self.last_txt[..n].copy_from_slice(&s.as_bytes()[..n]);
        self.last_fmt = f;
        let last_ext = self.last_ext;
        self.last_ext = ext.unwrap_or(0);
        let mkey = r.mode | (r.acdc as u8) << 7;
        // Extra digit: the main cells narrow to CELLS_X (40..372) and the
        // digit is the F78 glyph at half size (~38 px vs ~76) in
        // XT_X..XT_X + XT_W (375..402), on the F78 baseline (VALUE_BASE),
        // left of the limit badge / unit box (x >= 403). The full-draw
        // tail fill of the main cells clears the 373..374 gap, the digit
        // paints its whole 80 px high cell, and without the digit the main
        // cells span 40..400 again, so no stale pixels remain. x < 40
        // stays free for the HV warning glyph.
        let (w, cells) = match ext {
            Some(_) => (XT_X - 40, &CELLS_X[f & 3]),
            None => (360, &CELLS[f]),
        };
        sf::cells_diff(sf::F78, rect(40, 68, w as i16, 80), s, prev, cells, VALUE, BG);
        if let Some(e) = ext {
            if !same_layout || last_ext != e {
                let r = rect(XT_X as i16, 68, XT_W as i16, 80);
                sf::label_half(sf::F78, r, e as char, VALUE_BASE as i16, VALUE, BG);
            }
        } else if last_ext != 0 {
            // The full cells end at 400: the rest of the +1 digit cell.
            lcd::fill_rect(400, 68, XT_X + XT_W - 400, 80, BG);
        }
        if self.last_unit != r.main.unit || self.last_mode != mkey {
            self.last_unit = r.main.unit;
            self.last_mode = mkey;
            // From x 404: clear of the +1 digit cell (375..402).
            sf::label(sf::UNIT, rect(404, 114, 72, 32), unit, VALUE, BG, 4, -1);
            // Stock box (437,76,32,32) align 7, widened 3 px right so the
            // 35 px AC glyph is not clipped (ends at 471, 3 px after the
            // limit badge 403..433).
            sf::label(sf::MODE, rect(437, 76, 35, 32), mode_glyph(r.mode, r.acdc), VALUE, BG, 7, 0);
            if !r.acdc {
                // Leaving AC+DC: the tag slots left of the aux values.
                for i in 0..2 {
                    lcd::fill_rect(AUX_TAG_X[i] as u16, 36, (AUX_VX[i] - AUX_TAG_X[i]) as u16, 30, BG);
                }
            }
        }
        // Aux readouts (stock: value right-aligned cells at y 40, unit after).
        for (i, a) in [r.aux1, r.aux2].iter().enumerate() {
            // Value boxes end 4 px before the unit (separate items: >= 3 px).
        let (vx, ux) = (AUX_VX[i] + 30, if i == 0 { 178 } else { 400 });
            match a {
                Some(v) => {
                    let mut t = [0u8; 16];
                    let txt = meter::text(&mut t, v);
                    let mut u = [0u8; 16];
                    if r.acdc {
                        // AC+DC: the part's tag (AC for aux1, DC for aux2)
                        // right before its value, which stays right-aligned
                        // in its box: tag box from AUX_TAG_X to 4 px left
                        // of the text (both labels repaint their bg).
                        let end = vx - 30 + 137;
                        let te = (end - sf::text_width(sf::LETTERS, txt, 0) - 4).max(AUX_TAG_X[i] + 24);
                        sf::label(sf::AUX_TAG, rect(AUX_TAG_X[i], 36, te - AUX_TAG_X[i], 30), AUX_TAGS[i], VALUE, BG, 4, 0);
                        sf::label(sf::LETTERS, rect(te, 36, end - te, 30), txt, VALUE, BG, 4, 0);
                    } else {
                        sf::label(sf::LETTERS, rect(vx - 30, 36, 137, 30), txt, VALUE, BG, 4, 0);
                    }
                    // AC+DC: the aux1 unit box (V..uA, <= 48 px) ends at 231,
                    // the aux2 tag slot starts at 232; else full width (kHz 57 px).
                    let uw = if i == 0 && r.acdc { 54 } else { 76 };
                    lcd::fill_rect((ux - 1) as u16, 36, 4, 30, BG);
                    sf::label(sf::LETTERS, rect(ux + 3, 36, uw - 3, 30), sf::unit_text(v.unit, &mut u), VALUE, BG, 1, 0);
                    self.last_aux[i] = true;
                }
                None if self.last_aux[i] => {
                    lcd::fill_rect((vx - 30) as u16, 36, (ux + 76 - vx + 30) as u16, 30, BG);
                    self.last_aux[i] = false;
                }
                None => {}
            }
        }
        let top = (r.auto && !r.auto_plus, hold, r.auto_plus);
        if self.last_top != Some(top) || self.last_label != self.label {
            self.last_top = Some(top);
            self.last_label = self.label;
            let s = if !r.auto { "MANU" } else if r.auto_plus { "AUTO+" } else { "AUTO" };
            if let Some(l) = self.label {
                // Temperature sensor name (F21 has no C / N / T: F14, 1 px
                // spacing: "NTC100k B3950" is 87 px of the 90 px slot).
                sf::label(sf::F14, rect(BAR_X, 1, 90, 32), l, STATUS_FG, BAR, 1, 1);
            } else {
                sf::label(sf::F21, rect(BAR_X, 1, 90, 32), s, STATUS_FG, BAR, 1, 0);
            }
            hold_label(hold);
        }
        if self.last_lim != lim {
            self.last_lim = lim;
            lim_badge(lim);
        }
    }

    /// Empty meter strip (view drawn, no reading yet).
    pub fn strip_static(&mut self) {
        self.st = Strip::new();
        self.st.drawn = true;
        lcd::fill_rect(0, STRIP_Y as u16, lcd::W, STRIP_H as u16, BG);
    }

    /// Meter strip of the combined view (layout above). Draws only what
    /// changed since the last call: the value as one half-size text (a
    /// changed digit can move the others: proportional spacing), the
    /// sign / unit pair on a unit or mode change, each sub row on a text
    /// change, each badge on a change. `rel`: REL active; `hv`: 0 / 1 (>= 30 V,
    /// HV badge) / 2 (>= 100 V, red band) from the live value; `alt`: show
    /// REL instead of the limit verdict in the shared badge (the app flips it
    /// once a second when both apply).
    pub fn strip(&mut self, r: &Reading, hold: u8, lim: u8, rel: bool, hv: u8, alt: bool) {
        let st = &mut self.st;
        let bg = if hv >= 2 { ST_BG_HV } else { BG };
        if !st.drawn || st.hv != hv {
            *st = Strip::new();
            st.drawn = true;
            st.hv = hv;
            lcd::fill_rect(0, STRIP_Y as u16, lcd::W, STRIP_H as u16, bg);
        }
        // Value: "%7.<dec>f" without the extra digit (no room), OL = "0L".
        let mut buf = [0u8; 16];
        let (s, _, _) = fmt7(&mut buf, &r.main, false);
        let s = s.trim();
        let mut vb = [0u8; 8];
        let n = s.len().min(8);
        vb[..n].copy_from_slice(&s.as_bytes()[..n]);
        if st.value != vb {
            st.value = vb;
            // Half-size F78 rows: SV_BASE - 39 ..= SV_BASE.
            sf::half_text(sf::F78, rect(SV_X, SV_BASE - 40, SV_R - SV_X, 42), s, SV_BASE, 3, 5, VALUE, bg);
        }
        // Sub rows: AC+DC parts (tagged), Hz + duty, CJ + F, 4-20 mA %;
        // right-aligned, so each row's left edge moves with its text.
        let mut tb = [[0u8; 24]; 2];
        let mut left = [SS_R; 2];
        let mut tw = [0i16; 2];
        for (i, a) in [r.aux1, r.aux2].iter().enumerate() {
            let mut t = [0u8; 16];
            let mut u = [0u8; 16];
            let mut b = [0u8; 24];
            // AC+DC parts are in the main unit, shown right next to them:
            // no unit repeated (keeps a wide gap after the sign / unit).
            let txt = match a {
                Some(v) if r.acdc && v.unit == r.main.unit => meter::text(&mut t, v),
                Some(v) => join(&mut b, &[meter::text(&mut t, v), sf::unit_text(v.unit, &mut u)]),
                None => "",
            };
            let k = txt.len().min(23);
            tb[i][..k].copy_from_slice(&txt.as_bytes()[..k]);
            tb[i][23] = r.acdc as u8;
            tw[i] = if k == 0 { 0 } else { sf::text_width(sf::LETTERS, txt, 0) };
            left[i] = SS_R - tw[i] - if r.acdc && k > 0 { 4 + 24 } else { 0 };
        }
        // Sign above the unit (none for continuity / diode / non-V/A). The
        // pair repaints its column up to the new sub rows (an old wider unit
        // is erased there); the sub rows repaint from their old / new left
        // edge but never into the new unit's ink.
        let mut ub = [0u8; 16];
        let unit = sf::unit_text(r.main.unit, &mut ub);
        let sign = match r.mode {
            0..=9 => mode_glyph(r.mode, r.acdc),
            _ => "",
        };
        let (uf, dy) = unit_font(unit);
        let uend = [SU_X + sf::text_width(sf::AUX_TAG, sign, 0), SU_X + sf::text_width(uf, unit, 0)];
        let ukey = (r.main.unit, sign);
        if st.unit != ukey {
            st.unit = ukey;
            let w = |i: usize| SU_W.min(left[i] - SU_X);
            sf::label(sf::AUX_TAG, rect(SU_X, STRIP_Y + 3, w(0), 23), sign, VALUE, bg, 2, 0);
            let gap = SS_Y[1] - STRIP_Y - 26;
            lcd::fill_rect(SU_X as u16, (STRIP_Y + 26) as u16, w(0).min(w(1)) as u16, gap as u16, bg);
            sf::label(uf, rect(SU_X, SS_Y[1] + dy, w(1), 25 - dy), unit, VALUE, bg, 0, 0);
            if dy > 0 {
                lcd::fill_rect(SU_X as u16, SS_Y[1] as u16, w(1) as u16, dy as u16, bg);
            }
        }
        for i in 0..2 {
            if st.sub[i] == tb[i] {
                continue;
            }
            st.sub[i] = tb[i];
            let k = tb[i][..23].iter().position(|&c| c == 0).unwrap_or(23);
            let txt = core::str::from_utf8(&tb[i][..k]).unwrap_or("");
            let tx = SS_R - tw[i];
            let x0 = left[i].min(st.sub_x[i]).max(uend[i]);
            let (y, h) = (SS_Y[i], 25);
            if r.acdc && k > 0 {
                sf::label(sf::AUX_TAG, rect(x0, y, tx - 4 - x0, h), AUX_TAGS[i], VALUE, bg, 4, 0);
                lcd::fill_rect((tx - 4) as u16, y as u16, 4, h as u16, bg);
            } else if tx > x0 {
                lcd::fill_rect(x0 as u16, y as u16, (tx - x0) as u16, h as u16, bg);
            }
            if k > 0 {
                sf::label(sf::LETTERS, rect(tx, y, SS_R - tx, h), txt, VALUE, bg, 3, 0);
            }
            st.sub_x[i] = left[i];
        }
        // Badges.
        let b0 = if hv >= 1 {
            B_HV
        } else if let Some(k) = self.label.and_then(|l| T_LABELS.iter().position(|&x| x == l)) {
            B_SENSOR + k as u8
        } else if !r.auto {
            B_MANU
        } else if r.auto_plus {
            B_AUTOP
        } else {
            B_AUTO
        };
        let b1 = match hold {
            1 => B_HOLD,
            2 => B_AHOLD_WAIT,
            3 => B_AHOLD_HELD,
            _ => B_NONE,
        };
        let verdict = match lim {
            meter::V_PASS => B_PASS,
            meter::V_HI | meter::V_LO => B_FAIL,
            _ => B_NONE,
        };
        let b2 = if rel && (verdict == B_NONE || alt) { B_REL } else { verdict };
        for (k, b) in [b0, b1, b2].iter().enumerate() {
            if st.badge[k] != *b {
                st.badge[k] = *b;
                badge(k, *b, bg);
            }
        }
    }
}

/// HOLD / RUN slot of the DMM status bar. Manual HOLD keeps the stock F21
/// "HOLD"; AutoHold is a different face so the two cannot be confused:
/// "A-HOLD" in TEXT22 (66 px, F21 would need 103), accent blue while it waits
/// for a stable value, inverted (dark on orange) while it shows a captured
/// one. Slot 119..190 (after the logo-shifted range label; uptime from 205).
fn hold_label(hold: u8) {
    let r = rect(119, 1, 71, 32);
    match hold {
        2 | 3 => {
            let (fg, bg) = if hold == 3 { (BG, VALUE) } else { (0x04FC, BAR) };
            lcd::fill_rect(119, 1, 71, 4, BAR);
            lcd::fill_rect(119, 29, 71, 4, BAR);
            sf::label(sf::TEXT22, rect(119, 5, 71, 24), "A-HOLD", fg, bg, 8, 0);
        }
        _ => sf::label(sf::F21, r, if hold == 1 { "HOLD" } else { "RUN" }, STATUS_FG, BAR, 8, 0),
    }
}

/// FR-06 verdict badge (meter::V_*): a pill between the digits and the
/// mode glyph, green "OK" (pass), red "HI" / "LO" (fail above / below), none
/// when no limit applies. 31 px is all the room there is (>= 3 px to the
/// last digit and to the mode glyph): "PASS" / "FAIL HI" do not fit at a
/// legible size. The digits keep the value colour.
fn lim_badge(v: u8) {
    let r = rect(403, 72, 31, 17);
    let (t, bg) = match v {
        crate::meter::V_PASS => ("OK", 0x04A9),
        crate::meter::V_HI => ("HI", 0xF800),
        crate::meter::V_LO => ("LO", 0xF800),
        _ => {
            lcd::fill_rect(r.x as u16, r.y as u16, r.w as u16, r.h as u16, BG);
            return;
        }
    };
    sf::rbox(r, 4, bg, BG);
    sf::glyphs(sf::F14, rect(r.x, r.y + 1, r.w, r.h), t, 0xFFFF, bg, 8, 1);
}

/// MF range page (stock "Voltage / Current / Resistor Settings"): title bar,
/// range buttons, DC / AC buttons. Items: (label, set_range argument).
/// Labels: the stock DM40C set (60000 counts, flash strings 0x08078704..
/// 0x0807889c hold the 4/5/6 sets per model); fw-rs always uses the DM40C
/// limits (dmm.rs COUNTS_MODEL).
pub fn range_items(b: Button) -> &'static [(&'static str, Option<u8>)] {
    match b {
        Button::Volt => &[
            ("600mV", Some(0)), ("6V", Some(1)), ("60V", Some(2)), ("600V", Some(3)),
            ("1000V", Some(4)), ("AUTO+", Some(255)), ("AUTO", None),
        ],
        Button::Amp => &[
            ("600uA", Some(0)), ("6000uA", Some(1)), ("60mA", Some(2)), ("600mA", Some(3)),
            ("6A", Some(4)), ("10A", Some(5)), ("AUTO+", Some(255)), ("AUTO", None),
        ],
        // 60 ohm: the stock-table r0 row (OP1 x10, mohm resolution), manual
        // only (dmm.rs +1 digit notes).
        Button::Ohm => &[
            ("60\u{3a9}", Some(0)), ("600\u{3a9}", Some(1)), ("6K\u{3a9}", Some(2)), ("60K\u{3a9}", Some(3)),
            ("600K\u{3a9}", Some(4)), ("6M\u{3a9}", Some(5)), ("60M\u{3a9}", Some(6)), ("AUTO", None),
        ],
        _ => &[("AUTO", None)],
    }
}

fn range_rect(i: usize) -> sf::Rect {
    rect(8 + (i % 4) as i16 * 117, 50 + (i / 4) as i16 * 46, 112, 40)
}

/// `note`: a line under the range buttons (UF-21: the V page says AUTO+ is
/// the recommended auto range, or warns in orange while the mV-first AUTO
/// is in use), true = warning.
pub fn range_page(b: Button, sel: Option<usize>, sub: u8, note: Option<(&str, bool)>) {
    let title = match b {
        Button::Volt => "Voltage Settings",
        Button::Amp => "Current Settings",
        Button::Ohm => "Resistor Settings",
        _ => "Capacitance Settings",
    };
    lcd::fill_rect(0, 0, lcd::W, 40, 0x2966);
    lcd::fill_rect(0, 40, lcd::W, lcd::H - 40, BG);
    sf::logo(4, 10, 0x2966);
    sf::glyphs(sf::TEXT22, rect(0, 0, 480, 40), title, STATUS_FG, 0x2966, 8, 0);
    for (i, (name, _)) in range_items(b).iter().enumerate() {
        let c = if Some(i) == sel { SEL } else { 0x2966 };
        sf::rbox(range_rect(i), 6, c, BG);
        sf::glyphs(sf::TEXT22, range_rect(i), name, STATUS_FG, c, 8, 0);
    }
    if let Some((t, warn)) = note {
        range_note(t, warn);
    }
    if matches!(b, Button::Volt | Button::Amp) {
        // Stock: DC (8,182), AC (162,182), AC+DC (319,182), 150x38 each.
        for (i, name) in ["DC", "AC", "AC+DC"].iter().enumerate() {
            let c = if i as u8 == sub { SEL } else { 0x2966 };
            let r = rect([8, 162, 319][i], 182, 150, 38);
            sf::rbox(r, 6, c, BG);
            sf::glyphs(sf::TEXT22, r, name, STATUS_FG, c, 8, 0);
        }
    }
    ok_and_limits_buttons();
}

/// Range page note line (`range_page`): rows 142..177, under the second
/// button row (ends 136), above the DC / AC buttons (182); spacing 1 (TEXT22
/// glyphs touch at spacing 0).
pub fn range_note(t: &str, warn: bool) {
    sf::glyphs(sf::TEXT22, rect(8, 142, 464, 36), t, if warn { VALUE } else { DIM }, BG, 8, 1);
}

/// The "Hold/Limits" (opens View::Limits) and OK buttons at the bottom of
/// the MF range / temperature pages.
fn ok_and_limits_buttons() {
    for (x, t) in [(8i16, "Hold/Limits"), (319, "OK")] {
        let r = rect(x, 240, 150, 38);
        sf::rbox(r, 6, 0x2966, BG);
        sf::glyphs(sf::TEXT22, r, t, STATUS_FG, 0x2966, 8, 0);
    }
}

/// Range page touch: Ok(item index) / Err(0 DC, 1 AC, 3 AC+DC, 2 OK,
/// 4 Hold/Limits).
pub fn range_hit(b: Button, x: u16, y: u16) -> Option<Result<usize, u8>> {
    let (x, y) = (x as i16, y as i16);
    for i in 0..range_items(b).len() {
        let r = range_rect(i);
        if x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h {
            return Some(Ok(i));
        }
    }
    if (182..220).contains(&y) && matches!(b, Button::Volt | Button::Amp) {
        return Some(Err(if x < 158 { 0 } else if x < 316 { 1 } else { 3 }));
    }
    if (240..278).contains(&y) && x >= 319 {
        return Some(Err(2));
    }
    if (240..278).contains(&y) && (8..158).contains(&x) {
        return Some(Err(4));
    }
    None
}

/// Temperature sensor names on screen (dmm.rs T_NAMES order).
pub const T_LABELS: [&str; 7] = ["TC K", "PT100", "PT1000", "NTC10k B3950", "NTC10k B3435", "NTC100k B3950", "NTC user"];
const T_KEYS: [&str; 12] = [
    "TC K", "PT100", "PT1000", "10k B3950", "10k B3435", "100k B3950", "NTC user", "Offs 0", "-1.0", "-0.1", "+0.1",
    "+1.0",
];

/// MF page of the temperature function: sensor (items 0..6), offset of that
/// sensor (7 zero, 8..11 -1 / -0.1 / +0.1 / +1 C), an info line, OK.
pub fn temp_page(sensor: u8, info: &str) {
    lcd::fill_rect(0, 0, lcd::W, 40, 0x2966);
    lcd::fill_rect(0, 40, lcd::W, lcd::H - 40, BG);
    sf::logo(4, 10, 0x2966);
    sf::glyphs(sf::TEXT22, rect(0, 0, 480, 40), "Temperature Settings", STATUS_FG, 0x2966, 8, 0);
    for (i, name) in T_KEYS.iter().enumerate() {
        let c = if i == sensor as usize { SEL } else { 0x2966 };
        sf::rbox(range_rect(i), 6, c, BG);
        sf::glyphs(sf::TEXT22, range_rect(i), name, STATUS_FG, c, 8, 0);
    }
    sf::glyphs(sf::TEXT22, rect(8, 188, 464, 40), info, STATUS_FG, BG, 1, 0);
    ok_and_limits_buttons();
}

/// Temperature page touch: item 0..11, 255 = OK, 254 = Hold/Limits.
pub fn temp_hit(x: u16, y: u16) -> Option<u8> {
    let (x, y) = (x as i16, y as i16);
    for i in 0..T_KEYS.len() {
        let r = range_rect(i);
        if x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h {
            return Some(i as u8);
        }
    }
    if (240..278).contains(&y) && x >= 319 {
        Some(255)
    } else if (240..278).contains(&y) && (8..158).contains(&x) {
        Some(254)
    } else {
        None
    }
}

/// History boxes (stock (5+95i,153,88,19)): values with units, newest orange.
pub fn history(items: &[(f32, u8, &'static str); 5], n: usize, newest: usize) {
    for i in 0..5 {
        let r = rect(5 + 95 * i as i16, 153, 88, 19);
        sf::rbox(r, 3, BAR, BG);
        if i < n {
            let mut b = [0u8; 32];
            let (v, d, u) = items[i];
            let t = num_unit(&mut b, v, d, u);
            sf::glyphs(sf::HIST, r, t, if i == newest { 0xFC40 } else { STATUS_FG }, BAR, 8, 0);
        }
    }
}

/// High-voltage lightning (stock (0,80,40,64), >= 100 V in V modes).
pub fn hv_warning(on: bool) {
    sf::label(sf::LIGHTNING, rect(0, 80, 40, 64), if on { " " } else { "" }, 0xC9A0, BG, 8, 0);
}

/// What the status block at the right of every instrument bar shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub uptime_s: u32,
    pub bt: bool,
    pub bars: u8,
    pub charging: bool,
    pub locked: bool,
    /// MIN/MAX recording session on (the DMM statistics, FR-02).
    pub rec: bool,
    /// Index of the shown view in View::CYCLE (page dots), 255 = none.
    pub page: u8,
}

fn hms(t: &mut [u8; 8], s: u32) -> &str {
    let (h, m, sec) = (s / 3600 % 100, s / 60 % 60, s % 60);
    for (i, v) in [h, m, sec].iter().enumerate() {
        t[i * 3] = b'0' + (v / 10) as u8;
        t[i * 3 + 1] = b'0' + (v % 10) as u8;
        if i < 2 {
            t[i * 3 + 2] = b':';
        }
    }
    core::str::from_utf8(&t[..]).unwrap_or("")
}

fn battery_glyph(b: &mut [u8; 4], bars: u8) -> &str {
    char::from_u32(0x22 + bars.min(5) as u32).unwrap_or('"').encode_utf8(b)
}

/// Right part of the DMM page status bar (34 px, stock positions): uptime,
/// lock, BT link, battery.
pub fn status_right(st: &Status) {
    let mut t = [0u8; 8];
    let mut b = [0u8; 4];
    let bc = if st.charging { 0x04A9 } else { STATUS_FG };
    sf::label(sf::F21, rect(205, 5, 112, 24), hms(&mut t, st.uptime_s), STATUS_FG, BAR, 1, -1);
    sf::label(sf::LOCK, rect(335, 7, 24, 20), if st.locked { "\u{20}" } else { "\u{21}" }, STATUS_FG, BAR, 8, 0);
    sf::label(sf::SAVE_BT, rect(390, 0, 15, 32), if st.bt { "\u{21}" } else { "" }, STATUS_FG, BAR, 1, 0);
    sf::label(sf::BATTERY, rect(435, 9, 40, 16), battery_glyph(&mut b, st.bars), bc, BAR, 8, 0);
}

// ------------------------------------------------- scope-family top bar
// Scope page and combined view (docs/combined-view-design.md §2.3 / §4),
// 27 px tall so the 20 px status icons keep >= 4 px to the screen edge and
// >= 3 px to the bar's bottom edge (the stock 20 px bar could not hold them
// with any margin):
//   logo 4..23 | chips (17 px pills, rows 5..21): run state 27..63, timebase
//   67..108, V/div + coupling 112..171, trigger 175..236 | REC 240..263 |
//   page dots 301..332 | uptime ..385 | lock ..412 | BT 416..431 |
//   battery 436..475. Ink rows of the icons 4..23.

/// Scope-family bar height (rows 0..26; the full Scope page paints row 27
/// too, its graph starts at 28).
pub const SBAR_H: u16 = 27;
const CHIPS: [(i16, i16); 4] = [(27, 37), (67, 42), (112, 68), (183, 62)];
const CHIP_Y: i16 = 5;
const CHIP_H: i16 = 17;

fn chip(k: usize, text: &str, fg: Color) {
    let (x, w) = CHIPS[k];
    let r = rect(x, CHIP_Y, w, CHIP_H);
    sf::rbox(r, 4, CHIP, S_BAR);
    sf::glyphs(sf::F14, r, text, fg, CHIP, 8, 1);
}

/// Which top-bar chip (0 run state, 1 timebase, 2 V/div, 3 trigger) a tap
/// at (x, y) hits (8 px of slop below the bar).
pub fn chip_hit(x: u16, y: u16) -> Option<usize> {
    let (x, y) = (x as i16, y as i16);
    if y >= SBAR_H as i16 + 8 {
        return None;
    }
    CHIPS.iter().position(|&(cx, w)| x >= cx - 1 && x < cx + w + 2)
}

/// Status block of the scope-family bar: page dots, uptime, lock, BT,
/// battery, REC (every second, and on a change).
pub fn scope_status(st: &Status) {
    let mut t = [0u8; 8];
    let mut b = [0u8; 4];
    lcd::fill_rect(249, 5, 28, 17, S_BAR);
    if st.rec {
        sf::label(sf::F14, rect(249, 6, 28, 16), "REC", 0xF800, S_BAR, 1, 1);
    }
    for k in 0..4i16 {
        let c = if k as u8 == st.page { FG } else { 0x6B4D };
        sf::rbox(rect(301 + 9 * k, 11, 5, 5), 2, c, S_BAR);
    }
    sf::label(sf::F14, rect(336, 5, 50, 17), hms(&mut t, st.uptime_s), STATUS_FG, S_BAR, 4, 1);
    sf::label(sf::LOCK, rect(389, 4, 24, 20), if st.locked { "\u{20}" } else { "\u{21}" }, STATUS_FG, S_BAR, 4, 0);
    sf::label(sf::SAVE_BT, rect(416, 3, 16, 21), if st.bt { "\u{21}" } else { "" }, STATUS_FG, S_BAR, 0, 0);
    let bc = if st.charging { 0x04A9 } else { STATUS_FG };
    sf::label(sf::BATTERY, rect(436, 4, 40, 20), battery_glyph(&mut b, st.bars), bc, S_BAR, 8, 0);
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

/// What the chart last drew besides the trace, to skip unchanged labels.
/// The side rows are keyed by their value rounded to the shown digits, so a
/// running AVG repaints only when a shown digit changes.
pub struct ChartCache {
    scale: Option<(u32, u32, u8, &'static str)>,
    side: Option<(u8, &'static str)>,
    rows: [i64; 4],
    time: Option<(u32, bool)>,
    rate: u8,
    rchip: Option<RateChip>,
}

/// DMM reading-rate chip under the chart's timebase chip (owner request):
/// the FR-05 rate setting (`rate` = dmm::Rate as u8) and the measured
/// readings/s (`period_ms` = Dmm::reading_period_ms, 0 = not measured yet).
/// `applies` = Dmm::rate_applies: otherwise grey "n/a" + the measured rate.
/// Tap (`rate_chip_hit`) = next rate.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RateChip {
    pub rate: u8,
    pub period_ms: u32,
    pub applies: bool,
}

/// Rate chip rows (between the middle and the bottom axis labels, x 4..76
/// as the timebase chip).
const RATE_CHIP_Y: i16 = 236;
const RATE_CHIP_H: i16 = 17;

/// A tap on the DMM page's rate chip (3 px of slop above and below).
pub fn rate_chip_hit(x: u16, y: u16) -> bool {
    let y = y as i16;
    x < CHART_X && y >= RATE_CHIP_Y - 3 && y < RATE_CHIP_Y + RATE_CHIP_H + 3
}

/// Readings per second from a reading period: "9.4/s", "37/s", "--/s"
/// (period 0 = not measured yet).
pub fn rps_text(buf: &mut [u8; 16], period_ms: u32) -> &str {
    if period_ms == 0 {
        return "--/s";
    }
    let r10 = (10_000 + period_ms / 2) / period_ms;
    let mut t = [0u8; 16];
    let s = if r10 >= 100 {
        crate::dmm::format(&mut t, ((r10 + 5) / 10) as f32, 0)
    } else {
        crate::dmm::format(&mut t, r10 as f32 / 10.0, 1)
    };
    let n = s.len().min(13);
    buf[..n].copy_from_slice(&s.as_bytes()[..n]);
    buf[n..n + 2].copy_from_slice(b"/s");
    core::str::from_utf8(&buf[..n + 2]).unwrap_or("")
}

/// Right column of the DMM chart (FR-02): MAX / AVG above the REL box, MIN
/// and the time since the statistics started below it (display units).
/// `rec`: a recording session is on (no idle shutdown): "REC" in orange.
#[derive(Clone, Copy)]
pub struct Side {
    pub max: Option<f32>,
    pub avg: Option<f32>,
    pub min: Option<f32>,
    pub rel: Option<f32>,
    pub secs: Option<u32>,
    pub rec: bool,
}

/// Right column: text x range and the line tops of MAX, AVG, MIN, time
/// (F14 caps 2 px below: rows 178, 190, 247, 261); REL box (405,205,71,38).
const SIDE_X: i16 = 405;
const SIDE_R: i16 = 476;
const SIDE_Y: [i16; 4] = [176, 188, 245, 259];
/// Line tops of the chart's left axis labels (caps on rows 178, 221, 264).
const AXIS_Y: [i16; 3] = [176, 219, 262];

impl ChartCache {
    pub const fn new() -> Self {
        ChartCache { scale: None, side: None, rows: [i64::MIN; 4], time: None, rate: 0, rchip: None }
    }
    pub fn invalidate(&mut self) {
        *self = ChartCache::new();
    }
}

pub fn chart(c: &mut ChartCache, pts: &[f32], dec: u8, rate: u8, side: &Side, unit: &'static str, rc: RateChip) {
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
    let key = (hi.to_bits(), lo.to_bits(), dec, unit);
    if c.scale != Some(key) {
        c.scale = Some(key);
        // Axis labels right-aligned to x 76 (3 px left of the frame), caps on
        // the frame's top row, its middle and its bottom row; 1 px spacing.
        for (y, v) in [(AXIS_Y[0], hi), (AXIS_Y[1], (hi + lo) / 2.0), (AXIS_Y[2], lo)] {
            sf::label(sf::F14, rect(4, y, 73, 14), num_unit(&mut b, v, dec, unit), STATUS_FG, BG, 4, 1);
        }
    }
    // Column templates: plain / dotted column / vertical grid line / frame.
    let mut tpl = [[0u16; CHART_H as usize]; 4];
    for y in 0..CHART_H as usize {
        let edge = y == 0 || y == CHART_H as usize - 1;
        let hline = y % 16 == 0;
        tpl[0][y] = if edge { DIM } else { BG };
        tpl[1][y] = if edge || hline { DIM } else { BG };
        tpl[2][y] = if edge || hline || y % 4 == 0 { DIM } else { BG };
        tpl[3][y] = DIM;
    }
    let row = |v: f32| -> u16 {
        (((hi - v) / span) * (CHART_H - 3) as f32 + 1.0).clamp(1.0, (CHART_H - 2) as f32) as u16
    };
    let n = pts.len();
    let first = CHART_W as usize - n.min(CHART_W as usize);
    let mut prev: Option<u16> = None;
    for x in 0..CHART_W {
        let (a, b2) = if (x as usize) >= first {
            let y = row(pts[x as usize - first]);
            let s = match prev {
                Some(p) if p < y => (p, y),
                Some(p) => (y, p),
                None => (y, y),
            };
            prev = Some(y);
            s
        } else {
            (u16::MAX, 0)
        };
        let k = if x == 0 || x == CHART_W - 1 { 3 } else if x % 16 == 0 { 2 } else if x % 4 == 0 { 1 } else { 0 };
        let t = &tpl[k];
        lcd::set_window(CHART_X + x, CHART_Y, CHART_X + x, CHART_Y + CHART_H - 1);
        for y in 0..CHART_H {
            lcd::data(if y >= a && y <= b2 { 0xFC40 } else { t[y as usize] });
        }
    }
    if c.rate != rate {
        c.rate = rate;
        let mut t = [0u8; 16];
        let s = crate::dmm::format(&mut t, rate as f32 * 160.0, 0);
        let mut l = [0u8; 16];
        let k = s.len();
        l[..k].copy_from_slice(s.as_bytes());
        l[k..k + 6].copy_from_slice(b"ms/div");
        // Timebase chip (tap: next rate), between the top and middle labels.
        let r = rect(4, 192, 73, 17);
        sf::rbox(r, 4, BTN, BG);
        sf::glyphs(sf::F14, rect(r.x, r.y + 1, r.w, r.h), core::str::from_utf8(&l[..k + 6]).unwrap_or(""), STATUS_FG, BTN, 8, 1);
    }
    if c.rchip != Some(rc) {
        c.rchip = Some(rc);
        // "MED 9.4/s" (<= 62 px of F14: "SLOW 2.3/s"); grey "n/a 2.5/s"
        // where the rate setting changes nothing (AC, Hz, cap, temperature..).
        let mut rb = [0u8; 16];
        let mut b = [0u8; 24];
        let name = if rc.applies { crate::dmm::RATE_NAMES[rc.rate as usize % 3] } else { "n/a" };
        let txt = join(&mut b, &[name, " ", rps_text(&mut rb, rc.period_ms)]);
        let r = rect(4, RATE_CHIP_Y, 73, RATE_CHIP_H);
        sf::rbox(r, 4, BTN, BG);
        sf::glyphs(sf::F14, rect(r.x, r.y + 1, r.w, r.h), txt, if rc.applies { STATUS_FG } else { DIM }, BTN, 8, 1);
    }
    // Right column (x 405..475, the stock REL box rows 205..242 kept as the
    // tap target): MAX and AVG above the REL box, MIN and the session time
    // below; one F14 row each, label left (dim), value right-aligned
    // without the unit, which is shown once, in the REL box header
    // ("REL" .. "mV"). The stock version crammed "4.8170V" into 54 px with
    // touching glyphs.
    if c.side != Some((dec, unit)) {
        c.side = Some((dec, unit));
        c.rows = [i64::MIN; 4];
        c.time = None;
        lcd::fill_rect(402, 174, 78, 31, BG);
        lcd::fill_rect(402, 243, 78, 36, BG);
        for (i, t) in ["MAX", "AVG", "MIN"].iter().enumerate() {
            sf::label(sf::F14, rect(SIDE_X, SIDE_Y[i], 22, 14), t, DIM, BG, 0, 1);
        }
    }
    let q = |v: Option<f32>| -> i64 {
        match v {
            Some(x) if x.is_finite() => {
                let mut s = 1.0f32;
                for _ in 0..dec {
                    s *= 10.0;
                }
                let k = x * s;
                (if k < 0.0 { k - 0.5 } else { k + 0.5 }) as i64
            }
            _ => i64::MIN,
        }
    };
    let mut t = [0u8; 16];
    for (i, v) in [side.max, side.avg, side.min].iter().enumerate() {
        let k = q(*v);
        if c.rows[i] != k {
            c.rows[i] = k;
            let s = v.map(|v| crate::dmm::format(&mut t, v, dec)).unwrap_or("");
            sf::label(sf::F14, rect(SIDE_X + 25, SIDE_Y[i], SIDE_R - SIDE_X - 25, 14), s, STATUS_FG, BG, 3, 1);
        }
    }
    let k = q(side.rel.or(Some(0.0)));
    if c.rows[3] != k {
        c.rows[3] = k;
        let r = rect(405, 205, 71, 38);
        sf::rbox(r, 6, BTN, BG);
        sf::glyphs(sf::F14, rect(409, 209, 30, 14), "REL", STATUS_FG, BTN, 0, 1);
        let mut u = [0u8; 16];
        sf::glyphs(sf::F14, rect(430, 209, 42, 14), sf::unit_text(unit, &mut u), DIM, BTN, 3, 1);
        let rv = crate::dmm::format(&mut t, side.rel.unwrap_or(0.0), dec);
        sf::glyphs(sf::F14, rect(409, 225, 63, 14), rv, STATUS_FG, BTN, 3, 1);
    }
    let tk = side.secs.map(|t| (t, side.rec));
    if c.time != tk {
        c.time = tk;
        // "REC 01:02:03" (recording session, orange) / "T 01:02:03" since
        // the statistics started.
        let mut tb = [0u8; 8];
        let mut b = [0u8; 24];
        let txt = match side.secs {
            Some(sec) => join(&mut b, &[if side.rec { "REC " } else { "T " }, hms(&mut tb, sec)]),
            None => "",
        };
        let fg = if side.rec { 0xFC40 } else { DIM };
        sf::label(sf::F14, rect(SIDE_X, SIDE_Y[3], SIDE_R - SIDE_X, 14), txt, fg, BG, 3, 1);
    }
}

// ---------------------------------------------------------------- scope
// Stock scope page geometry (scope_gen_ui_v2_6.h section 6), with the
// scope-family top bar above (SBAR_H) and the right panel / measurement bar
// laid out for the >= 3 px margin rules.

pub const S_BAR: Color = 0x4208;
const S_FRAME: Color = 0xAD55;
const S_FILL: Color = 0x2124;
const S_GRID: Color = 0x9492; // white blended 50 % into the fill
const S_PURPLE: Color = 0xA81F;
const S_ORANGE: Color = 0xFAA0;

/// Graph placement: `x0, y0` of frame column / row 0, height in pixels for the
/// 280-row model (rows scale by (h - 3) / 278; grid lines every (h - 1) / 8).
pub struct ScopeView {
    pub x0: u16,
    pub y0: u16,
    pub h: u16,
}

/// Full Scope page: 401 x 273 (8 x 34 px divisions) under the 28 px bar
/// (stock 281 rows 1:1; the taller bar costs 8 rows).
pub const SCOPE_FULL: ScopeView = ScopeView { x0: 13, y0: 28, h: 273 };
/// Combined DMM+Scope view: under the bar and the meter strip, 401 x 217
/// (8 x 27 px divisions), down to the measurement bar.
pub const SCOPE_COMBINED: ScopeView = ScopeView { x0: 13, y0: (STRIP_Y + STRIP_H) as u16, h: 217 };
/// Measurement bar rows (both scope views).
const MEAS_Y: u16 = 301;

/// "1.25us" / "2MHz" style engineering text.
fn eng(buf: &mut [u8; 24], v: f32, unit: &str) -> usize {
    let (k, p) = if v.abs() >= 1e6 {
        (1e-6, "M")
    } else if v.abs() >= 1e3 {
        (1e-3, "k")
    } else if v.abs() >= 1.0 || v == 0.0 {
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

/// Right-panel buttons. Combined view: V+ V- T+ T- AUTOSET (doc §2.3); full
/// Scope page: the same zoom keys plus coupling, trigger mode and slope, each
/// labelled with its current state.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pb {
    VIn,
    VOut,
    TIn,
    TOut,
    Autoset,
    Coupling,
    Mode,
    Slope,
}

const PANEL_COMBINED: [Pb; 5] = [Pb::VIn, Pb::VOut, Pb::TIn, Pb::TOut, Pb::Autoset];
const PANEL_FULL: [Pb; 7] = [Pb::VIn, Pb::Coupling, Pb::VOut, Pb::TIn, Pb::Mode, Pb::TOut, Pb::Slope];
const PANEL_X: i16 = 417;
const PANEL_W: i16 = 59;

/// (buttons, top y, pitch, height) of the right panel.
fn panel(full: bool) -> (&'static [Pb], i16, i16, i16) {
    if full {
        (&PANEL_FULL, SCOPE_FULL.y0 as i16 + 3, 38, 35)
    } else {
        (&PANEL_COMBINED, SCOPE_COMBINED.y0 as i16 + 3, 43, 39)
    }
}

/// Which right-panel button a tap at (x, y) hits (gaps count to the
/// nearer button).
pub fn panel_hit(full: bool, x: u16, y: u16) -> Option<Pb> {
    let (b, y0, pitch, h) = panel(full);
    let y = y as i16;
    if x < 414 || y < y0 - 2 || y >= y0 + (b.len() as i16 - 1) * pitch + h + 2 {
        return None;
    }
    Some(b[((y - y0 + (pitch - h) / 2) / pitch).clamp(0, b.len() as i16 - 1) as usize])
}

/// Magnifier-style ring icon (a 2 px circle of radius 9 around a 10 px bar,
/// plus a vertical bar for "+"), 20 x 20 at (x, y), 2x2 supersampled.
fn ring_icon(x: i16, y: i16, plus: bool, fg: Color, bg: Color) {
    let lut = sf::ramp(fg, bg);
    #[cfg(test)]
    crate::probe::begin(crate::probe::ICON, bg);
    lcd::set_window(x as u16, y as u16, (x + 19) as u16, (y + 19) as u16);
    for py in 0..20i32 {
        for px in 0..20i32 {
            let mut n = 0;
            for (ox, oy) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
                let (sx, sy) = (4 * px + ox - 40, 4 * py + oy - 40);
                let d2 = sx * sx + sy * sy;
                let ring = (28 * 28..=36 * 36).contains(&d2);
                let bar = (sy.abs() < 4 && sx.abs() <= 20) || (plus && sx.abs() < 4 && sy.abs() <= 20);
                n += (ring || bar) as usize;
            }
            lcd::data(lut[[0, 4, 8, 11, 15][n]]);
        }
    }
    #[cfg(test)]
    crate::probe::end();
}

/// Right panel of a scope view.
pub fn scope_panel(sc: &scope::Scope, full: bool) {
    let (b, y0, pitch, h) = panel(full);
    let s = &sc.s;
    for (k, &p) in b.iter().enumerate() {
        let y = y0 + k as i16 * pitch;
        let r = rect(PANEL_X, y, PANEL_W, h);
        sf::rbox(r, 6, BTN, S_BAR);
        let icon = |letter: &str, fg: Color, plus: bool| {
            sf::glyphs(sf::F21, rect(PANEL_X + 6, y, 20, h), letter, fg, BTN, 8, 0);
            ring_icon(PANEL_X + 32, y + (h - 20) / 2, plus, fg, BTN);
        };
        match p {
            Pb::VIn => icon("V", TRACE, true),
            Pb::VOut => icon("V", TRACE, false),
            Pb::TIn => icon("T", S_PURPLE, true),
            Pb::TOut => icon("T", S_PURPLE, false),
            _ => {
                let (t, fg) = match p {
                    Pb::Autoset => ("AUTOSET", FG),
                    Pb::Coupling => (if s.ac { "AC" } else { "DC" }, TRACE),
                    Pb::Mode => (
                        match s.mode {
                            scope::TrigMode::Auto => "AUTO",
                            scope::TrigMode::Normal => "NORM",
                            scope::TrigMode::Single => "SINGLE",
                        },
                        S_ORANGE,
                    ),
                    _ => (if s.rising { "EDGE /" } else { "EDGE \\" }, S_ORANGE),
                };
                sf::glyphs(sf::F14, r, t, fg, BTN, 8, 1);
            }
        }
    }
}

impl ScopeView {
    fn row_px(&self, r: i32) -> u16 {
        self.y0 + 1 + ((r.clamp(0, 278) as u32 * (self.h as u32 - 3)) / 278) as u16
    }

    /// Pixel row (from the graph top) of row `r` of the 280-row model.
    pub fn row_y(&self, r: i32) -> i32 {
        (self.row_px(r) - self.y0) as i32
    }

    /// Full Scope page or the combined view's graph part.
    fn full(&self) -> bool {
        self.y0 == SCOPE_FULL.y0
    }

    /// Whether (x, y) is inside the graph (frame included).
    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x0 && x <= self.x0 + 400 && y >= self.y0 && y < self.y0 + self.h
    }

    /// Chrome, chips, panel and an empty graph. The status block and the
    /// generator chip are drawn by the app (scope_status, gen_chip).
    /// Not inlined (here and `frame`): their 2-5 KB of column templates
    /// would otherwise all add up in main()'s frame.
    #[inline(never)]
    pub fn draw_static(&self, sc: &scope::Scope) {
        let full = self.full();
        let bar = if full { self.y0 } else { SBAR_H };
        lcd::fill_rect(0, 0, lcd::W, bar, S_BAR);
        lcd::fill_rect(0, self.y0, self.x0, self.h, S_BAR);
        lcd::fill_rect(414, self.y0, 66, self.h, S_BAR);
        lcd::fill_rect(0, MEAS_Y, lcd::W, lcd::H - MEAS_Y, S_BAR);
        sf::logo(4, 4, S_BAR);
        self.labels(sc);
        scope_panel(sc, full);
        let mut tpl = [[0u16; 288]; 4];
        self.templates(&mut tpl);
        for i in 0..=scope::COLS as u16 {
            self.column_t(i, &tpl, None, None, false);
        }
    }

    /// Generator chip at the right end of the measurement bar: the frequency
    /// actually output, "--" when the wave is silent at the set frequency
    /// (non-square above 50 kHz); nothing while the output is off.
    pub fn gen_chip(g: &gen::Gen) {
        let mut b = [0u8; 24];
        let mut t = [0u8; 24];
        let txt = if g.s.on {
            let k = eng(&mut b, g.actual_hz(), "Hz");
            join(&mut t, &["G ", core::str::from_utf8(&b[..k]).unwrap_or("")])
        } else {
            ""
        };
        sf::label(sf::F14, rect(415, MEAS_Y as i16 + 1, 61, 15), txt, 0x04A9, S_BAR, 4, 1);
    }

    /// Top-bar chips: run state, timebase, V/div + coupling, trigger.
    pub fn labels(&self, sc: &scope::Scope) {
        let s = &sc.s;
        let mut b = [0u8; 24];
        let mut t = [0u8; 24];
        let (status, sc_col) = if !s.run {
            ("STOP", S_ORANGE)
        } else {
            match s.mode {
                scope::TrigMode::Auto => ("AUTO", S_PURPLE),
                scope::TrigMode::Normal => ("WAIT", S_PURPLE),
                scope::TrigMode::Single => ("T'D", S_PURPLE),
            }
        };
        chip(0, status, sc_col);
        let n = eng(&mut b, scope::SDIV[s.tb], "s");
        chip(1, core::str::from_utf8(&b[..n]).unwrap_or(""), FG);
        let n = eng(&mut b, scope::VDIV[s.range] * s.probe, "V");
        let v = join(&mut t, &[core::str::from_utf8(&b[..n]).unwrap_or(""), if s.ac { " AC" } else { " DC" }]);
        chip(2, v, TRACE);
        let n = eng(&mut b, sc.trig_volts(), "V");
        let v = join(&mut t, &[if s.rising { "/" } else { "\\" }, core::str::from_utf8(&b[..n]).unwrap_or("")]);
        chip(3, v, S_ORANGE);
    }

    /// Background templates of a column: [plain, dotted (i % 8 == 0), vertical
    /// grid line (i % 40 == 0), frame edge].
    fn templates(&self, t: &mut [[u16; 288]; 4]) {
        self.templates_c(t, None);
    }

    /// Templates with horizontal cursor rows (280-row model) drawn dotted.
    /// Grid rows every (h - 1) / 8 pixels, from the pixel row (any height,
    /// not only the stock 1:1 281).
    fn templates_c(&self, t: &mut [[u16; 288]; 4], ycur: Option<[i32; 2]>) {
        let h = self.h as usize;
        let pitch = ((h - 1) / 8).max(1);
        for y in 0..h {
            let edge = y == 0 || y == h - 1;
            let hline = y % pitch == 0 && !edge;
            t[0][y] = if edge { S_FRAME } else { S_FILL };
            t[1][y] = if edge { S_FRAME } else if hline { S_GRID } else { S_FILL };
            t[2][y] = if edge { S_FRAME } else if y % 5 == 0 || hline { S_GRID } else { S_FILL };
            t[3][y] = S_FRAME;
        }
        if let Some(yc) = ycur {
            for r in yc {
                let y = (self.row_px(r) - self.y0) as usize;
                if y > 0 && y < h - 1 {
                    for k in 0..3 {
                        t[k][y] = S_FRAME;
                    }
                }
            }
        }
    }

    /// One graph column `i` (0..400) from a template, with the trace segment
    /// (rows of the 280-row model) and the trigger level tick (columns 1..5,
    /// inside the frame); `line`: the level as a dashed line across the graph
    /// (4 px on / 4 off, behind the trace).
    fn column_t(&self, i: u16, t: &[[u16; 288]; 4], seg: Option<(i32, i32)>, trig: Option<i32>, line: bool) {
        self.column_c(i, t, seg, trig, line, u16::MAX)
    }

    /// `column_t` drawing only the rows above screen row `clip`.
    fn column_c(&self, i: u16, t: &[[u16; 288]; 4], seg: Option<(i32, i32)>, trig: Option<i32>, line: bool, clip: u16) {
        let x = self.x0 + i;
        let h = self.h.min(clip.saturating_sub(self.y0));
        if h == 0 {
            return;
        }
        let k = if i == 0 || i == 400 { 3 } else if i % 40 == 0 { 2 } else if i % 8 == 0 { 1 } else { 0 };
        let tpl = &t[k];
        let (a, b) = match seg {
            Some((a, b)) if i > 0 => (self.row_px(a) - self.y0, self.row_px(b) - self.y0),
            _ => (u16::MAX, 0),
        };
        let on = (1..6).contains(&i) || (line && i > 0 && i < 400 && i % 8 < 4);
        let tr = if on { trig.map(|r| self.row_px(r) - self.y0).unwrap_or(u16::MAX) } else { u16::MAX };
        lcd::set_window(x, self.y0, x, self.y0 + h - 1);
        for y in 0..h {
            let c = if y >= a && y <= b {
                0xFFE0
            } else if y == tr {
                S_ORANGE
            } else {
                tpl[y as usize]
            };
            lcd::data(c);
        }
    }

    fn column(&self, i: u16, seg: Option<(i32, i32)>, trig: Option<i32>) {
        let mut t = [[0u16; 288]; 4];
        self.templates(&mut t);
        self.column_t(i, &t, seg, trig, false);
    }

    /// Draw a frame: 400 columns of min/max (peak detect), widened to touch
    /// the previous column, as the stock trace.
    /// `prev` caches the drawn segment per column (pixel rows); only changed
    /// columns are redrawn. Fill it with (u16::MAX, 0) to force a full redraw
    /// (also needed when `trig_line` changes).
    /// `clip`: draw only the rows above this screen row (the combined view's
    /// function band covers the rest).
    #[inline(never)]
    pub fn frame(&self, f: &scope::Frame, sc: &scope::Scope, prev: &mut [(u16, u16); 401], cur: &Cursors, persist: bool, trig_line: bool, clip: u16) {
        let n = f.data.len();
        let trig_row = Some(sc.s.trig_row);
        let mut tpl = [[0u16; 288]; 4];
        self.templates_c(&mut tpl, if cur.y_on { Some(cur.y) } else { None });
        // Vertical cursor columns use a dotted template.
        let mut vt = tpl;
        for k in 0..3 {
            for y in (0..self.h as usize).step_by(3) {
                vt[k][y] = S_FRAME;
            }
        }
        // Code -> row is linear: precompute both ends once.
        let r0 = sc.row_of_code(0.0);
        let slope = (sc.row_of_code(1000.0) - r0) / 1000.0;
        let row = |code: u16| -> i32 { (r0 + slope * code as f32) as i32 };
        // Previous column's segment, for joining; None at the left edge and
        // after an empty column (roll start, or a stopped record narrower than
        // the screen), so the first column after a gap is not stretched to row 0.
        let mut pseg: Option<(i32, i32)> = None;
        for i in 0..scope::COLS {
            let (mut lo, mut hi);
            if let Some(cols) = f.cols {
                let (a, b) = cols[i];
                if a == u16::MAX {
                    pseg = None;
                    if prev[i] != (u16::MAX - 1, 0) {
                        prev[i] = (u16::MAX - 1, 0);
                        self.column_c(i as u16, &tpl, None, trig_row, trig_line, clip);
                    }
                    continue;
                }
                lo = row(b);
                hi = row(a);
            } else if n >= scope::COLS {
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
                let r = (r0 + slope * (v0 + (v1 - v0) * fr)) as i32;
                lo = r;
                hi = r;
            }
            if let Some((pa, pb)) = pseg {
                lo = lo.min(pb);
                hi = hi.max(pa);
            }
            pseg = Some((lo, hi));
            let (a, b) = (lo.min(hi), lo.max(hi));
            let (a, b) = if persist && prev[i].0 != u16::MAX && prev[i].0 != u16::MAX - 1 {
                // Infinite persistence: keep the union of all segments drawn.
                let (pa, pb) = prev[i];
                let ra = ((pa - self.y0 - 1) as i32 * 278 / (self.h as i32 - 3).max(1)).min(a);
                let rb = ((pb - self.y0 - 1) as i32 * 278 / (self.h as i32 - 3).max(1)).max(b);
                (ra, rb)
            } else {
                (a, b)
            };
            let px = (self.row_px(a), self.row_px(b));
            let is_cur = cur.x_on && (cur.x[0] as usize == i || cur.x[1] as usize == i);
            if prev[i] != px || is_cur {
                prev[i] = if is_cur { (u16::MAX, 0) } else { px };
                self.column_c(i as u16, if is_cur { &vt } else { &tpl }, Some((a, b)), trig_row, trig_line, clip);
            }
        }
    }
}

/// Scope cursors: two vertical (columns 1..399) and two horizontal (rows of
/// the 280-row model), as the stock X2-1 / Xf / Y2-1 items.
#[derive(Clone, Copy)]
pub struct Cursors {
    pub x_on: bool,
    pub y_on: bool,
    pub x: [u16; 2],
    pub y: [i32; 2],
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

/// Bottom measurement cells (stock: x 13/113/213/313, 98x17), selected items.
pub fn scope_measurements(sel: &[u8], res: &crate::scope_meas::Results) {
    for i in 0..4 {
        let x = 13 + 100 * i as i16;
        let mut b = [0u8; 24];
        let t = match sel.get(i) {
            Some(&id) => crate::scope_meas::format(&mut b, id, res.v[id as usize]),
            None => "--",
        };
        // Name and value on one baseline, caps at row 304 (ink rows
        // 304..315: 3 px under the graph frame, 4 above the screen edge),
        // 1 px lower when a glyph reaches above the caps ('i', '%'); a part
        // that would then hang below 315 (a descender: "+Duty:" next to
        // "44.4%") keeps row 304. 1 px glyph spacing (F14 glyphs have no
        // side bearing: at 0 they touch); the value 3 px after the colon.
        let (name, val) = match t.find(':') {
            Some(k) => (&t[..=k], t[k + 1..].trim_start()),
            None => (t, ""),
        };
        let wn = sf::text_width(sf::F14, name, 1) + 3;
        let (top, y0, y1) = (MEAS_Y as i16 + 1, MEAS_Y as i16 + 3, lcd::H as i16 - 5);
        let rows = [sf::glyph_rows(sf::F14, name), sf::glyph_rows(sf::F14, val)];
        let cell = rows.iter().flatten().map(|r| y0 - r.0).fold(y0 - 2, i16::max);
        for (k, (s, x0, w)) in [(name, x, wn), (val, x + wn, 98 - wn)].into_iter().enumerate() {
            let y = rows[k].map_or(cell, |r| cell.min(y1 - r.1));
            if y > top {
                lcd::fill_rect(x0 as u16, top as u16, w as u16, (y - top) as u16, S_BAR);
            }
            sf::label(sf::F14, rect(x0, y, w, lcd::H as i16 - y), s, 0xFFE0, S_BAR, 0, 1);
        }
    }
}

/// Scope settings window (stock MF popup (13,20,401,192)): tabs Measure /
/// Display / Trigger / Settings.
/// Top of the scope settings window (under the 28 px bar).
const MENU_Y: u16 = 28;
pub const SCOPE_TABS: [&str; 4] = ["Measure", "Display", "Trigger", "Settings"];

fn tab_btn(r: sf::Rect, text: &str, on: bool) {
    let c = if on { SEL } else { 0x2966 };
    sf::rbox(r, 4, c, 0x10A3);
    sf::glyphs(sf::F14, r, text, STATUS_FG, c, 8, 1);
}

pub fn scope_menu(tab: usize, sc: &scope::Scope, sel: &[u8], roll: bool, persist: bool) {
    lcd::fill_rect(13, MENU_Y, 401, 192, 0x10A3);
    for (i, t) in SCOPE_TABS.iter().enumerate() {
        tab_btn(rect(16 + i as i16 * 99, MENU_Y as i16 + 3, 95, 28), t, i == tab);
    }
    let s = &sc.s;
    match tab {
        0 => {
            for (k, (id, name, _)) in crate::scope_meas::ITEMS.iter().enumerate() {
                let r = rect(16 + (k % 5) as i16 * 79, MENU_Y as i16 + 36 + (k / 5) as i16 * 38, 75, 34);
                tab_btn(r, name, sel.contains(id));
            }
        }
        1 => {
            tab_btn(rect(16, MENU_Y as i16 + 40, 190, 34), if roll { "Roll: ON" } else { "Roll: OFF" }, roll);
            tab_btn(rect(210, MENU_Y as i16 + 40, 190, 34), if persist { "Persist: INF" } else { "Persist: OFF" }, persist);
        }
        2 => {
            tab_btn(rect(16, MENU_Y as i16 + 40, 120, 34), "Rising", s.rising);
            tab_btn(rect(140, MENU_Y as i16 + 40, 120, 34), "Falling", !s.rising);
            tab_btn(rect(16, MENU_Y as i16 + 80, 120, 34), "Auto", s.mode == scope::TrigMode::Auto);
            tab_btn(rect(140, MENU_Y as i16 + 80, 120, 34), "Normal", s.mode == scope::TrigMode::Normal);
            tab_btn(rect(264, MENU_Y as i16 + 80, 120, 34), "Single", s.mode == scope::TrigMode::Single);
        }
        _ => {
            tab_btn(rect(16, MENU_Y as i16 + 40, 120, 34), "DC", !s.ac);
            tab_btn(rect(140, MENU_Y as i16 + 40, 120, 34), "AC", s.ac);
            for (k, p) in ["x1", "x10", "x100"].iter().enumerate() {
                let f = [1.0f32, 10.0, 100.0][k];
                tab_btn(rect(16 + k as i16 * 124, MENU_Y as i16 + 80, 120, 34), p, s.probe == f);
            }
        }
    }
}

/// Scope settings window touch: Some((tab, item)) with item 255 = tab header.
pub fn scope_menu_hit(tab: usize, x: u16, y: u16) -> Option<(usize, usize)> {
    let (x, y) = (x as i16, y as i16);
    if !(13..414).contains(&x) || !(MENU_Y as i16..MENU_Y as i16 + 192).contains(&y) {
        return None;
    }
    let y = y - (MENU_Y as i16 - 20);
    if y < 53 {
        return Some((((x - 16) / 99).clamp(0, 3) as usize, 255));
    }
    match tab {
        0 => {
            let col = ((x - 16) / 79) as usize;
            let row = ((y - 56) / 38) as usize;
            let k = row * 5 + col.min(4);
            if k < crate::scope_meas::ITEMS.len() { Some((0, k)) } else { None }
        }
        1 => Some((1, if x < 208 { 0 } else { 1 })),
        _ => {
            let row = if y < 96 { 0 } else { 1 };
            let col = ((x - 16) / 124).clamp(0, 2) as usize;
            Some((tab, row * 3 + col))
        }
    }
}

// ------------------------------------------------------------ generator
// Stock generator page (scope_gen_ui_v2_6.h section 6).

const G_BG: Color = 0x10A3;
const G_BAR: Color = 0x2146;
const G_BOX: Color = 0x3A08;
const G_TRACE: Color = 0x04A9;

const G_DIM: Color = 0x7BEF;
/// Generator value boxes (right column x 246..475, 66 px each): Frequency,
/// Vpp, Offset (FR-28), Duty. A tap opens the box's keypad (`gen_box_hit`).
/// Each box: the name (TEXT22) and a note (F14, right) on the top row, the
/// value (LETTERS, right-aligned to 411) and its unit (from 421) below.
pub const GEN_BOX_Y: [i16; 4] = [40, 110, 180, 250];
const GEN_BOX_H: i16 = 66;
const GEN_NAMES: [&str; 4] = ["Frequency", "Vpp", "Offset", "Duty"];

/// Generator box (0..3) under a tap.
pub fn gen_box_hit(x: u16, y: u16) -> Option<usize> {
    let y = y as i16;
    if x < 246 {
        return None;
    }
    GEN_BOX_Y.iter().position(|&b| y >= b && y < b + GEN_BOX_H)
}

/// Value / unit row of generator box `k` (the frequency keypad types into
/// box 0's). `letters`: the value in LETTERS (digits), else TEXT22 ("auto").
fn gen_box_value(k: usize, v: &str, letters: bool, unit: &str, fg: Color) {
    let y = GEN_BOX_Y[k];
    sf::label(if letters { sf::LETTERS } else { sf::TEXT22 }, rect(262, y + 33, 150, 30), v, fg, G_BOX, 4, 0);
    sf::label(sf::TEXT22, rect(421, y + 36, 50, 26), unit, fg, G_BOX, 1, 0);
}

/// Note at the right of box `k`'s name row ("" clears it).
fn gen_box_note(k: usize, note: &str, fg: Color) {
    sf::label(sf::F14, rect(360, GEN_BOX_Y[k] + 3, 112, 26), note, fg, G_BOX, 4, 1);
}

pub fn gen_static(g: &gen::Gen) {
    lcd::fill_rect(0, 0, lcd::W, 36, G_BAR);
    lcd::fill_rect(0, 36, lcd::W, lcd::H - 36, G_BG);
    sf::logo(4, 8, G_BAR);
    for (k, name) in GEN_NAMES.iter().enumerate() {
        let y = GEN_BOX_Y[k];
        sf::rbox(rect(246, y, 230, GEN_BOX_H), 3, G_BOX, G_BG);
        sf::glyphs(sf::TEXT22, rect(257, y + 2, 100, 26), name, STATUS_FG, G_BOX, 1, 0);
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
    let out = &g.out;
    // Top bar: RUN/STOP (output state).
    sf::label(sf::F21, rect(BAR_X, 0, 80, 36), if s.on { "RUN" } else { "STOP" }, if s.on { 0x07E0 } else { STATUS_FG }, G_BAR, 8, 0);
    // Wave drop-down.
    sf::rbox(rect(4, 40, 239, 42), 3, G_BOX, G_BG);
    sf::glyphs(sf::TEXT22, rect(14, 40, 180, 42), s.wave.name(), STATUS_FG, G_BOX, 1, 0);
    sf::glyphs(sf::Font(0x0805_9428), rect(197, 40, 42, 42), " ", STATUS_FG, G_BOX, 8, 0);
    gen_preview(s.wave);
    let mut t = [0u8; 16];
    // Frequency: the actual (quantised) output in full digits, "32768.08",
    // 3 decimals below 1 kHz (UF-15). Above the wave's limit (UF-14): the
    // set value grey and the limit ("max 50 kHz") as the note.
    let clamped = g.freq_clamped();
    let f = if clamped { s.freq_hz as f32 } else { g.actual_hz_f64() as f32 };
    let dec = if f < 1e3 { 3 } else if f < 1e5 { 2 } else if f < 1e6 { 1 } else { 0 };
    gen_box_value(0, crate::dmm::format(&mut t, f, dec), true, "Hz", if clamped { G_DIM } else { STATUS_FG });
    let note = s.wave.max_note();
    gen_box_note(0, if clamped { note.split(": ").nth(1).unwrap_or(note) } else { "" }, VALUE);
    // Vpp / Offset: the set values; "auto" = the stock 0..Vpp (centre
    // Vpp/2). On the PWM path (square above 50 kHz) both are fixed by the
    // logic output: grey, note "logic level".
    let fixed = out.fixed_level();
    let lv = if fixed { G_DIM } else { STATUS_FG };
    gen_box_value(1, crate::dmm::format(&mut t, s.amp_v, 2), true, "V", lv);
    match s.offset_v {
        Some(o) => gen_box_value(2, crate::dmm::format(&mut t, o, 2), true, "V", lv),
        None => gen_box_value(2, "auto", false, "", lv),
    }
    for k in [1, 2] {
        gen_box_note(k, if fixed { "logic level" } else { "" }, G_DIM);
    }
    // Duty: square only (DAC table or PWM counter, any frequency); the
    // effective, quantised value.
    let sq = s.wave == gen::Wave::Square;
    let d = if sq { out.duty_pct() } else { s.duty as f32 / 100.0 };
    gen_box_value(3, crate::dmm::format(&mut t, d, 1), true, "%", if sq { STATUS_FG } else { G_DIM });
}

// --------------------------------------------------------------- keypads
// Stock generator keypads: key bg 0x6b4d, panel 0xe73c, keys 60x40.

const K_KEY: Color = 0x6B4D;
const K_PANEL: Color = 0xE73C;
const K_FONT: sf::Font = sf::Font(0x0805_C5DC);

type Key = (sf::Rect, &'static str, u8);

/// Keypad kinds: the generator frequency field, the generator Vpp / duty
/// popup, and the same popup with a "+/-" key (DMM limits).
pub const KP_FREQ: u8 = 0;
pub const KP_NUM: u8 = 1;
pub const KP_SIGNED: u8 = 2;

/// Keys: (rect, label, code). Codes: digits, '.', 'H'/'K'/'M', '<' backspace,
/// '-' sign toggle, '=' OK. Constant tables (cheaper in the RAM image than
/// building them).
pub fn keypad_keys(kind: u8) -> &'static [Key; 15] {
    const fn r(x: i16, y: i16, w: i16, h: i16) -> sf::Rect {
        rect(x, y, w, h)
    }
    const FREQ: [Key; 15] = [
            (r(228, 146, 60, 40), "1", b'1'), (r(292, 146, 60, 40), "2", b'2'), (r(356, 146, 60, 40), "3", b'3'),
            (r(420, 146, 56, 40), "H", b'H'), (r(228, 190, 60, 40), "4", b'4'), (r(292, 190, 60, 40), "5", b'5'),
            (r(356, 190, 60, 40), "6", b'6'), (r(420, 190, 56, 40), "K", b'K'), (r(228, 234, 60, 40), "7", b'7'),
            (r(292, 234, 60, 40), "8", b'8'), (r(356, 234, 60, 40), "9", b'9'), (r(420, 234, 56, 40), "M", b'M'),
            (r(228, 277, 60, 40), "0", b'0'), (r(291, 277, 123, 40), "<", b'<'), (r(417, 277, 59, 40), "OK", b'='),
        ];
    const NUM: [Key; 15] = [
            (r(228, 146, 60, 40), "1", b'1'), (r(292, 146, 60, 40), "2", b'2'), (r(356, 146, 60, 40), "3", b'3'),
            (r(228, 190, 60, 40), "4", b'4'), (r(292, 190, 60, 40), "5", b'5'), (r(356, 190, 60, 40), "6", b'6'),
            (r(228, 234, 60, 40), "7", b'7'), (r(292, 234, 60, 40), "8", b'8'), (r(356, 234, 60, 40), "9", b'9'),
            (r(228, 277, 60, 40), "0", b'0'), (r(291, 277, 123, 40), ".", b'.'), (r(417, 146, 59, 84), "<", b'<'),
            (r(417, 234, 59, 83), "OK", b'='), (r(0, 0, 0, 0), "", 0), (r(0, 0, 0, 0), "", 0),
        ];
    // NUM with the wide '.' split into '.' and "+/-".
    const SIGNED: [Key; 15] = [
            (r(228, 146, 60, 40), "1", b'1'), (r(292, 146, 60, 40), "2", b'2'), (r(356, 146, 60, 40), "3", b'3'),
            (r(228, 190, 60, 40), "4", b'4'), (r(292, 190, 60, 40), "5", b'5'), (r(356, 190, 60, 40), "6", b'6'),
            (r(228, 234, 60, 40), "7", b'7'), (r(292, 234, 60, 40), "8", b'8'), (r(356, 234, 60, 40), "9", b'9'),
            (r(228, 277, 60, 40), "0", b'0'), (r(292, 277, 60, 40), ".", b'.'), (r(417, 146, 59, 84), "<", b'<'),
            (r(417, 234, 59, 83), "OK", b'='), (r(356, 277, 58, 40), "+/-", b'-'), (r(0, 0, 0, 0), "", 0),
        ];
    match kind {
        KP_FREQ => &FREQ,
        KP_NUM => &NUM,
        _ => &SIGNED,
    }
}

pub fn keypad(kind: u8, title: &str, input: &str, unit: &str) {
    let freq = kind == KP_FREQ;
    if freq {
        lcd::fill_rect(224, 142, 256, 178, K_PANEL);
    } else {
        lcd::fill_rect(224, 100, 256, 220, K_PANEL);
        sf::label(sf::TEXT22, rect(228, 108, 100, 26), title, 0x0000, K_PANEL, 1, 0);
    }
    keypad_input(freq, input, unit);
    for &(r, l, c) in keypad_keys(kind) {
        if c == 0 {
            continue;
        }
        sf::rbox(r, 4, K_KEY, K_PANEL);
        sf::glyphs(if l.len() > 1 || c == b'<' { sf::TEXT22 } else { K_FONT }, r, l, STATUS_FG, K_KEY, 8, 0);
    }
}

/// The typed text: in the page's frequency field, or the popup's value box.
pub fn keypad_input(freq: bool, input: &str, unit: &str) {
    if freq {
        gen_box_value(0, input, true, unit, 0x04FC);
        gen_box_note(0, "", G_DIM);
    } else {
        // 300..439: room for "-12.345" (112 px); titles end before 300.
        sf::label(sf::LETTERS, rect(300, 104, 139, 34), input, 0x0000, K_PANEL, 4, 0);
        sf::label(sf::TEXT22, rect(446, 110, 30, 22), unit, 0x0000, K_PANEL, 1, 0);
    }
}

pub fn keypad_hit(kind: u8, x: u16, y: u16) -> Option<u8> {
    let (x, y) = (x as i16, y as i16);
    for &(r, _, c) in keypad_keys(kind) {
        if c != 0 && x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h {
            return Some(c);
        }
    }
    None
}

// ----------------------------------------------------------------- menu
// Stock main menu (gui_v2_6.h section 2): logo, four tiles 190x106 with the
// stock icons; plus a fifth row for the combined views of this firmware.

const M_BG: Color = 0x10A3;
const M_TILE: Color = 0x2146;
const M_ACCENT: Color = 0x04FC;
const ICONS: sf::Font = sf::Font(0x0805_592C);

pub const TILES: [(i16, i16, &str, u32); 4] = [
    (46, 52, "Multimeter", 0x20),
    (244, 52, "Oscilloscope", 0x21),
    (46, 166, "Signal", 0x22),
    (244, 166, "Settings", 0x23),
];

pub fn main_menu(combo: &str) {
    lcd::fill_rect(0, 0, lcd::W, lcd::H, M_BG);
    for (x, y, name, icon) in TILES {
        sf::rbox(rect(x, y, 190, 106), 8, M_TILE, M_BG);
        let g = char::from_u32(icon).unwrap_or(' ');
        let mut b = [0u8; 4];
        sf::glyphs(ICONS, rect(x + 79, y + 25, 30, 30), g.encode_utf8(&mut b), STATUS_FG, M_TILE, 8, 0);
        sf::glyphs(sf::TEXT22, rect(x, y + 58, 190, 27), name, STATUS_FG, M_TILE, 8, 0);
    }
    sf::rbox(rect(46, 280, 388, 34), 8, M_TILE, M_BG);
    let mut t = [0u8; 48];
    let pre = b"Combined view: ";
    t[..pre.len()].copy_from_slice(pre);
    let k = combo.len().min(48 - pre.len());
    t[pre.len()..pre.len() + k].copy_from_slice(&combo.as_bytes()[..k]);
    sf::glyphs(sf::TEXT22, rect(46, 280, 388, 34), core::str::from_utf8(&t[..pre.len() + k]).unwrap_or(""), STATUS_FG, M_TILE, 8, 0);
}

/// Which tile (0..3) or the combined-view row (4) a touch hits.
pub fn main_menu_hit(x: u16, y: u16) -> Option<usize> {
    let (x, y) = (x as i16, y as i16);
    for (i, (tx, ty, _, _)) in TILES.iter().enumerate() {
        if x >= *tx && x < tx + 190 && y >= *ty && y < ty + 106 {
            return Some(i);
        }
    }
    if (46..434).contains(&x) && (280..314).contains(&y) {
        return Some(4);
    }
    None
}

/// System settings (subset of the stock sections that this firmware has),
/// two pages of at most SET_ROWS rows. Page 1: the last row "More" opens
/// page 2 (SETTINGS2): the DMM rate / smoothing (FR-05) and "Device", the
/// stock §6 Device/About page. "Back" is the tappable title bar (stock
/// settings have a title-bar back, not a list row), handled by
/// `title_back_hit` (page 2 -> page 1 -> menu), so the lists hold only
/// settings.
pub const SETTINGS: [&str; 9] = [
    "Dim screen", "Shutdown", "Backlight", "Volume", "Bluetooth", "Start page", "Analog power",
    "Scope drag", "More",
];
pub const SETTINGS2: [&str; 3] = ["DMM rate", "Smoothing", "Device"];

/// Settings/menu-page title bar: 40 px, bg 0x2966 (stock gui_v2_6.h:101).
pub const PAGE_BAR_H: i16 = 40;
/// Settings list geometry (vendor rows are ~37 px, gui_v2_6.h:109): 9 rows
/// at pitch 30 fit the 320 px panel (44 + 8 * 30 + 27 = 311).
const SET_Y0: i16 = 44;
const SET_PITCH: i16 = 30;
const SET_ROW_H: i16 = 27;
const SET_ROWS: usize = 9;
/// Row `i` outer box. Shared by `settings()` and `settings_hit()` so the render
/// and the touch map cannot drift.
fn set_row(i: usize) -> sf::Rect {
    rect(8, SET_Y0 + i as i16 * SET_PITCH, 464, SET_ROW_H)
}

/// Draw the shared menu-page title bar with a left "< Back" affordance; the
/// whole bar is the back target (`title_back_hit`).
fn page_title(title: &str) {
    lcd::fill_rect(0, 0, lcd::W, PAGE_BAR_H as u16, 0x2966);
    sf::glyphs(sf::TEXT22, rect(0, 0, 480, PAGE_BAR_H), title, STATUS_FG, 0x2966, 8, 0);
    sf::logo(4, 10, 0x2966);
    sf::glyphs(sf::TEXT22, rect(BAR_X + 3, 0, 110, PAGE_BAR_H), "< Back", STATUS_FG, 0x2966, 1, 0);
}

/// A tap on the title bar is "go back".
pub fn title_back_hit(y: u16) -> bool {
    (y as i16) < PAGE_BAR_H
}

/// Settings page `page` (0 / 1) with its row values.
pub fn settings(page: u8, vals: &[&str]) {
    if page == 0 {
        list_page("Settings", &SETTINGS, vals);
    } else {
        list_page("Settings 2/2", &SETTINGS2, vals);
    }
}

/// A settings-style page: title bar with "< Back", up to SET_ROWS rows of
/// label + value (SET_* geometry; `settings_hit` / `list_hit` map touches).
pub fn list_page(title: &str, names: &[&str], vals: &[&str]) {
    page_title(title);
    lcd::fill_rect(0, PAGE_BAR_H as u16, lcd::W, lcd::H - PAGE_BAR_H as u16, M_BG);
    for (i, name) in names.iter().enumerate().take(SET_ROWS) {
        let r = set_row(i);
        sf::rbox(r, 6, M_TILE, M_BG);
        // Label then value kept close together (value left-aligned just past a
        // 190 px label column) instead of flung to the far right edge.
        sf::glyphs(sf::TEXT22, rect(r.x + 12, r.y, 190, r.h), name, STATUS_FG, M_TILE, 1, 0);
        sf::glyphs(sf::TEXT22, rect(r.x + 202, r.y, 250, r.h), vals[i], M_ACCENT, M_TILE, 1, 0);
    }
}

pub fn settings_hit(page: u8, y: u16) -> Option<usize> {
    list_hit(y, if page == 0 { SETTINGS.len() } else { SETTINGS2.len() })
}

/// Row of a `list_page` with `n` rows under a touch at `y`.
pub fn list_hit(y: u16, n: usize) -> Option<usize> {
    let y = y as i16;
    if y < SET_Y0 {
        return None;
    }
    let i = ((y - SET_Y0) / SET_PITCH) as usize;
    // Reject taps that land in the gap between rows (pitch > row height).
    let within = (y - SET_Y0) % SET_PITCH < SET_ROW_H;
    if i < n.min(SET_ROWS) && within { Some(i) } else { None }
}

/// Rows of the DMM Hold & Limits page (View::Limits).
pub const LIMIT_ROWS: [&str; 8] = [
    "Auto Hold", "Limits", "Upper limit", "Lower limit", "Upper = reading", "Lower = reading", "Clear limits",
    "Fail beep",
];

pub fn view_name(v: View) -> &'static str {
    match v {
        View::Dmm => "Multimeter",
        View::Scope => "Oscilloscope",
        View::Gen => "Signal",
        View::DmmScope => "DMM + Scope",
        View::Menu => "Main menu",
        View::Settings => "Settings",
        View::Range => "Range",
        View::ScopeMenu => "Scope settings",
        View::Keypad => "Keypad",
        View::Device => "Device",
        View::Limits => "Hold & Limits",
    }
}

/// fw-rs firmware version, from Cargo.toml at compile time. This is the
/// replacement firmware's own version, NOT the stock "Software V2.6".
pub const FW_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Device / About page (stock gui_v2_6.h section 6: Vendor / Web / Model /
/// versions, panel 0x08074a10 as left-aligned "label:value" lines). Rendered
/// in the fw-rs settings style (title bar + rows) so it matches the other
/// menu pages. `lines` are (label, value) pairs built by the caller.
pub fn device_page(lines: &[(&str, &str)]) {
    page_title("Device");
    lcd::fill_rect(0, PAGE_BAR_H as u16, lcd::W, lcd::H - PAGE_BAR_H as u16, M_BG);
    for (i, (label, value)) in lines.iter().enumerate() {
        let y = 48 + i as i16 * 36;
        sf::rbox(rect(8, y, 464, 33), 6, M_TILE, M_BG);
        sf::glyphs(sf::TEXT22, rect(20, y, 180, 33), label, STATUS_FG, M_TILE, 1, 0);
        sf::glyphs(sf::TEXT22, rect(200, y, 260, 33), value, M_ACCENT, M_TILE, 1, 0);
    }
}
