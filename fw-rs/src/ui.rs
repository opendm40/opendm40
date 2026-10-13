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
/// Shorter status bar for the combined DMM+scope view, so the scope gets more
/// room (standalone DMM keeps the full `BAR_H`).
pub const BAR_H_C: u16 = 22;
const VALUE_X1: u16 = 396; // right edge of the last digit
/// Extra-digit cell of the main readout (x, width); see DmmView::reading.
/// Ends at 401, where the limit badge (401..436, y 70..112) and the unit box
/// start; 28 px = the widest half-size F78 digit ("4", 56 / 2).
const XT_X: u16 = 373;
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
/// Main-readout cells with the extra digit (formats 0..3 only, never OL):
/// sign 29 and point 17 (exactly the F78 '-' / '.' widths), digits 57 (F78
/// digits are <= 56 px wide) = 331 px: 40..371, then a 2 px gap and the
/// half-size digit cell XT_X..XT_X + XT_W (373..401).
const CELLS_X: [[u8; 7]; 4] = [
    [29, 57, 17, 57, 57, 57, 57],
    [29, 57, 57, 17, 57, 57, 57],
    [29, 57, 57, 57, 17, 57, 57],
    [29, 57, 57, 57, 57, 17, 57],
];
/// Compact main-readout cells for View::DmmScope: the LETTERS font (30 px,
/// digit advance 18) instead of F78 (78 px), roughly half height, so each cell
/// is sized to its glyph. Every row totals 152 px (the 152 px value rect) vs 360.
const CELLS_C: [[u8; 7]; 5] = [
    [20, 24, 12, 24, 24, 24, 24],
    [20, 24, 24, 12, 24, 24, 24],
    [20, 24, 24, 24, 12, 24, 24],
    [20, 24, 24, 24, 24, 12, 24],
    [20, 22, 22, 22, 24, 24, 18],
];

/// AC+DC aux rows: aux1 = AC part, aux2 = DC part (AUX_TAG 0x05 AC / 0x04 DC).
const AUX_TAGS: [&str; 2] = ["\u{5}", "\u{4}"];
/// Full view: left edge of each aux row's tag slot and of its value box
/// (value right-aligned in AUX_VX..AUX_VX + 138; aux1's unit box ends at 231).
const AUX_TAG_X: [i16; 2] = [8, 232];
const AUX_VX: [i16; 2] = [38, 260];
/// Compact view: AC / DC part lines right of the limit badge (302..351).
const AUX_C_X: u16 = 354;

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
    /// Main readout as last drawn (text, cell format, compact) for redrawing
    /// only the digits that changed.
    last_txt: [u8; 8],
    last_fmt: usize,
    last_compact: bool,
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
}

/// Bottom button `i` (0..5) with its stock glyphs.
fn dmm_button(i: usize, selected: bool, mode: u8, acdc: bool) {
    let x = 2 + 80 * i as i16;
    let c = if selected { SEL } else { BTN };
    sf::rbox(rect(x, BTN_Y as i16, 76, 40), 8, c, BG);
    let ac = matches!(mode, 0 | 2 | 4..=6);
    let sub = if acdc { "\u{6}" } else if ac { "\u{5}" } else { "\u{4}" };
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
        DmmView {
            last_txt: [0; 8],
            last_fmt: 255,
            last_compact: false,
            last_ext: 0,
            last_unit: "",
            last_mode: 255,
            last_top: None,
            last_aux: [false; 2],
            label: None,
            last_label: None,
            last_lim: 255,
        }
    }

    pub fn draw_static(&mut self, button: Button, mode: u8, acdc: bool, compact: bool) {
        let bar_h = if compact { BAR_H_C } else { BAR_H };
        lcd::fill_rect(0, bar_h, lcd::W, lcd::H - bar_h, BG);
        lcd::fill_rect(0, 0, lcd::W, bar_h, BAR);
        // History boxes only on the standalone DMM page (no history row in the
        // combined view).
        if !compact {
            for i in 0..5i16 {
                sf::rbox(rect(5 + 95 * i, 153, 88, 19), 3, BAR, BG);
            }
        }
        let sel = button_index(button);
        for i in 0..6 {
            dmm_button(i, i == sel, mode, acdc);
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
    }

    /// Draw a reading. `hold`: Meter::hold_state (0 RUN, 1 HOLD, 2 AutoHold
    /// tracking, 3 AutoHold captured); `lim`: Meter::verdict.
    pub fn reading(&mut self, r: &Reading, hold: u8, lim: u8, compact: bool) {
        let mut buf = [0u8; 16];
        // The compact view has no room for the extra digit.
        let (s, f, ext) = fmt7(&mut buf, &r.main, !compact);
        let mut ub = [0u8; 16];
        let unit = sf::unit_text(r.main.unit, &mut ub);
        // Only the changed digits are redrawn (cell layout unchanged).
        let same_layout =
            self.last_fmt == f && self.last_compact == compact && (self.last_ext != 0) == ext.is_some();
        let prev_txt = self.last_txt;
        let prev = if same_layout { core::str::from_utf8(&prev_txt[..7]).unwrap_or("") } else { "" };
        let n = s.len().min(7);
        self.last_txt[..n].copy_from_slice(&s.as_bytes()[..n]);
        self.last_fmt = f;
        self.last_compact = compact;
        let last_ext = self.last_ext;
        self.last_ext = ext.unwrap_or(0);
        let mkey = r.mode | (r.acdc as u8) << 7;
        if compact {
            // Combined view: half-height value (LETTERS, 30 px) on one line,
            // right under the short bar, with the unit and mode glyph beside it,
            // leaving the rest of the panel to the scope.
            sf::cells_diff(sf::LETTERS, rect(40, 24, 152, 30), s, prev, &CELLS_C[f], VALUE_RED, BG);
            if self.last_unit != r.main.unit || self.last_mode != mkey {
                self.last_unit = r.main.unit;
                self.last_mode = mkey;
                sf::label(sf::UNIT, rect(198, 24, 64, 31), unit, VALUE_RED, BG, 1, 0);
                // 36 px wide: the AC glyph is 35 px, AC+DC 33 (a 32 px box
                // clipped their right columns); ends before the badge at 302.
                sf::label(sf::MODE, rect(266, 23, 36, 32), mode_glyph(r.mode, r.acdc), VALUE_RED, BG, 1, 0);
                if !r.acdc {
                    lcd::fill_rect(AUX_C_X, 22, lcd::W - AUX_C_X, 33, BG);
                }
            }
            // AC+DC: the AC and DC parts as two small lines right of the
            // limit badge, each with its tag right before the value.
            if r.acdc {
                for (i, a) in [r.aux1, r.aux2].iter().enumerate() {
                    let y = 23 + 16 * i as i16;
                    let mut t = [0u8; 16];
                    let mut u = [0u8; 16];
                    let mut b = [0u8; 32];
                    let txt = match a {
                        Some(v) => {
                            let vt = meter::text(&mut t, v);
                            let ut = sf::unit_text(v.unit, &mut u);
                            let n = (vt.len() + ut.len()).min(b.len());
                            b[..vt.len()].copy_from_slice(vt.as_bytes());
                            b[vt.len()..n].copy_from_slice(&ut.as_bytes()[..n - vt.len()]);
                            core::str::from_utf8(&b[..n]).unwrap_or("")
                        }
                        None => "",
                    };
                    sf::label(sf::AUX_TAG, rect(AUX_C_X as i16, y, 26, 16), AUX_TAGS[i], VALUE_RED, BG, 1, 0);
                    sf::label(sf::F14, rect(AUX_C_X as i16 + 26, y, (lcd::W - AUX_C_X) as i16 - 26, 16), txt, VALUE_RED, BG, 1, 1);
                }
            }
        } else {
            // Extra digit: the main cells narrow to CELLS_X (40..371) and the
            // digit is the F78 glyph at half size (~38 px vs ~76) in
            // XT_X..XT_X + XT_W (373..401), on the F78 baseline (VALUE_BASE),
            // left of the limit badge / unit box (x >= 401). The full-draw
            // tail fill of the main cells clears the 371..373 gap, the digit
            // paints its whole 80 px high cell, and without the digit the main
            // cells span 40..400 again, so no stale pixels remain. x < 40
            // stays free for the HV warning glyph.
            let (w, cells) = match ext {
                Some(_) => (XT_X - 40, &CELLS_X[f & 3]),
                None => (360, &CELLS[f]),
            };
            sf::cells_diff(sf::F78, rect(40, 68, w as i16, 80), s, prev, cells, VALUE_RED, BG);
            if let Some(e) = ext {
                if !same_layout || last_ext != e {
                    let r = rect(XT_X as i16, 68, XT_W as i16, 80);
                    sf::label_half(sf::F78, r, e as char, VALUE_BASE as i16, VALUE_RED, BG);
                }
            }
            if self.last_unit != r.main.unit || self.last_mode != mkey {
                self.last_unit = r.main.unit;
                self.last_mode = mkey;
                sf::label(sf::UNIT, rect(401, 114, 79, 32), unit, VALUE_RED, BG, 4, -1);
                // Stock box (437,76,32,32) align 7, widened 1 px left (the
                // limit badge ends at 435) and 2 px right so the 35 px AC
                // glyph is not clipped; DC / AC+DC land on the stock pixels.
                sf::label(sf::MODE, rect(436, 76, 35, 32), mode_glyph(r.mode, r.acdc), VALUE_RED, BG, 7, 0);
                if !r.acdc {
                    // Leaving AC+DC: the tag slots left of the aux values.
                    for i in 0..2 {
                        lcd::fill_rect(AUX_TAG_X[i] as u16, 36, (AUX_VX[i] - AUX_TAG_X[i]) as u16, 30, BG);
                    }
                }
            }
            // Aux readouts (stock: value right-aligned cells at y 40, unit after).
            for (i, a) in [r.aux1, r.aux2].iter().enumerate() {
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
                            let end = vx - 30 + 138;
                            let te = (end - sf::text_width(sf::LETTERS, txt, 0) - 4).max(AUX_TAG_X[i] + 24);
                            sf::label(sf::AUX_TAG, rect(AUX_TAG_X[i], 36, te - AUX_TAG_X[i], 30), AUX_TAGS[i], VALUE_RED, BG, 4, 0);
                            sf::label(sf::LETTERS, rect(te, 36, end - te, 30), txt, VALUE_RED, BG, 4, 0);
                        } else {
                            sf::label(sf::LETTERS, rect(vx - 30, 36, 138, 30), txt, VALUE_RED, BG, 4, 0);
                        }
                        // AC+DC: the aux1 unit box (V..uA, <= 48 px) ends at 231,
                        // the aux2 tag slot starts at 232; else full width (kHz 57 px).
                        let uw = if i == 0 && r.acdc { 54 } else { 80 };
                        sf::label(sf::LETTERS, rect(ux, 36, uw, 30), sf::unit_text(v.unit, &mut u), VALUE_RED, BG, 1, 0);
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
        if self.last_top != Some(top) || self.last_label != self.label {
            self.last_top = Some(top);
            self.last_label = self.label;
            let s = if !r.auto { "MANU" } else if r.auto_plus { "AUTO+" } else { "AUTO" };
            if let Some(l) = self.label {
                // Temperature sensor name (F21 has no C / N / T: F14).
                // F14 glyphs are as wide as their advance (no side bearing),
                // so spacing 0 ran "PT100" / "NTC10k B3950" into one smear:
                // 2 px as the other F14 labels (the longest, "NTC100k
                // B3950", is 97 px of 105), 1 px in the 92 px compact slot.
                if compact {
                    sf::label(sf::F14, rect(4, 0, 92, BAR_H_C as i16), l, STATUS_FG, BAR, 1, 1);
                } else {
                    sf::label(sf::F14, rect(4, 1, 105, 32), l, STATUS_FG, BAR, 1, 2);
                }
            } else if compact {
                // Range and HOLD/RUN in the short combined-view bar.
                sf::label(sf::F21, rect(4, 0, 90, BAR_H_C as i16), s, STATUS_FG, BAR, 1, 0);
            } else {
                sf::label(sf::F21, rect(4, 1, 105, 32), s, STATUS_FG, BAR, 1, -1);
            }
            hold_label(hold, compact);
        }
        if self.last_lim != lim {
            self.last_lim = lim;
            lim_badge(lim, compact);
        }
    }
}

/// HOLD / RUN slot of the DMM status bar. Manual HOLD keeps the stock F21
/// "HOLD"; AutoHold is a different face so the two cannot be confused:
/// "A-HOLD" in TEXT22 (66 px, F21 would need 103), accent blue while it waits
/// for a stable value, inverted (dark on orange) while it shows a captured
/// one. The compact slot is 72 px (98..170; the uptime starts at 205).
fn hold_label(hold: u8, compact: bool) {
    let r = if compact { rect(98, 0, 72, BAR_H_C as i16) } else { rect(115, 1, 71, 32) };
    match hold {
        2 | 3 => {
            let (fg, bg) = if hold == 3 { (BG, 0xFC40) } else { (0x04FC, BAR) };
            if compact {
                sf::label(sf::TEXT22, r, "A-HOLD", fg, bg, 8, 0);
            } else {
                lcd::fill_rect(115, 1, 71, 4, BAR);
                lcd::fill_rect(115, 29, 71, 4, BAR);
                sf::label(sf::TEXT22, rect(115, 5, 71, 24), "A-HOLD", fg, bg, 8, 0);
            }
        }
        _ => sf::label(sf::F21, r, if hold == 1 { "HOLD" } else { "RUN" }, STATUS_FG, BAR, 8, 0),
    }
}

/// FR-06 verdict badge (meter::V_*): green PASS, red FAIL HI / LO, nothing
/// when no limit applies. The main digits stay red. Full view: the free
/// 35 x 42 cell right of the digits, left of the mode glyph (above the unit);
/// compact: right of the mode glyph.
fn lim_badge(v: u8, compact: bool) {
    const PASS_BG: Color = 0x04A9;
    const FAIL_BG: Color = 0xF800;
    let (l1, l2, bg) = match v {
        crate::meter::V_PASS => ("PASS", "", PASS_BG),
        crate::meter::V_HI => ("FAIL", "HI", FAIL_BG),
        crate::meter::V_LO => ("FAIL", "LO", FAIL_BG),
        _ => ("", "", BG),
    };
    if compact {
        let t = match v {
            crate::meter::V_HI => "FAIL HI",
            crate::meter::V_LO => "FAIL LO",
            _ => l1,
        };
        sf::label(sf::F14, rect(302, 28, 50, 22), t, 0xFFFF, bg, 8, 0);
    } else {
        sf::label(sf::F14, rect(401, 70, 35, 21), l1, 0xFFFF, bg, 8, 0);
        sf::label(sf::F14, rect(401, 91, 35, 21), l2, 0xFFFF, bg, 8, 0);
    }
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

pub fn range_page(b: Button, sel: Option<usize>, sub: u8) {
    let title = match b {
        Button::Volt => "Voltage Settings",
        Button::Amp => "Current Settings",
        Button::Ohm => "Resistor Settings",
        _ => "Capacitance Settings",
    };
    lcd::fill_rect(0, 0, lcd::W, 40, 0x2966);
    lcd::fill_rect(0, 40, lcd::W, lcd::H - 40, BG);
    sf::glyphs(sf::TEXT22, rect(0, 0, 480, 40), title, STATUS_FG, 0x2966, 8, 0);
    for (i, (name, _)) in range_items(b).iter().enumerate() {
        let c = if Some(i) == sel { SEL } else { 0x2966 };
        sf::rbox(range_rect(i), 6, c, BG);
        sf::glyphs(sf::TEXT22, range_rect(i), name, STATUS_FG, c, 8, 0);
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

/// Right part of the status bar: uptime, lock, BT link, battery. `compact`
/// packs it into the shorter combined-view bar; standalone DMM passes false and
/// is pixel-identical to the stock layout.
pub fn status_right(uptime_s: u32, bt: bool, bars: u8, charging: bool, locked: bool, compact: bool) {
    let mut t = [0u8; 8];
    let (h, m, sec) = (uptime_s / 3600 % 100, uptime_s / 60 % 60, uptime_s % 60);
    for (i, v) in [h, m, sec].iter().enumerate() {
        t[i * 3] = b'0' + (v / 10) as u8;
        t[i * 3 + 1] = b'0' + (v % 10) as u8;
        if i < 2 {
            t[i * 3 + 2] = b':';
        }
    }
    let g = char::from_u32(0x22 + bars.min(5) as u32).unwrap_or('"');
    let mut b = [0u8; 4];
    let bc = if charging { 0x04A9 } else { STATUS_FG };
    if compact {
        sf::label(sf::F21, rect(205, 0, 112, 22), core::str::from_utf8(&t).unwrap_or(""), STATUS_FG, BAR, 1, 0);
        sf::label(sf::LOCK, rect(335, 1, 24, 20), if locked { "\u{20}" } else { "\u{21}" }, STATUS_FG, BAR, 8, 0);
        sf::label(sf::SAVE_BT, rect(390, 0, 15, BAR_H_C as i16), if bt { "\u{21}" } else { "" }, STATUS_FG, BAR, 1, 0);
        sf::label(sf::BATTERY, rect(435, 3, 40, 16), g.encode_utf8(&mut b), bc, BAR, 8, 0);
    } else {
        sf::label(sf::F21, rect(205, 5, 112, 24), core::str::from_utf8(&t).unwrap_or(""), STATUS_FG, BAR, 1, -1);
        sf::label(sf::LOCK, rect(335, 7, 24, 20), if locked { "\u{20}" } else { "\u{21}" }, STATUS_FG, BAR, 8, 0);
        sf::label(sf::SAVE_BT, rect(390, 0, 15, 32), if bt { "\u{21}" } else { "" }, STATUS_FG, BAR, 1, 0);
        sf::label(sf::BATTERY, rect(435, 9, 40, 16), g.encode_utf8(&mut b), bc, BAR, 8, 0);
    }
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

/// What the chart last drew besides the trace, to skip unchanged labels.
/// The side rows are keyed by their value rounded to the shown digits, so a
/// running AVG repaints only when a shown digit changes.
pub struct ChartCache {
    scale: Option<(u32, u32, u8, &'static str)>,
    side: Option<(u8, &'static str)>,
    rows: [i64; 4],
    time: Option<(u32, bool)>,
    rate: u8,
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

/// Rows of the right column: MAX, AVG, MIN, time (y); REL box in between.
const SIDE_Y: [i16; 4] = [175, 189, 249, 263];

impl ChartCache {
    pub const fn new() -> Self {
        ChartCache { scale: None, side: None, rows: [i64::MIN; 4], time: None, rate: 0 }
    }
    pub fn invalidate(&mut self) {
        *self = ChartCache::new();
    }
}

pub fn chart(c: &mut ChartCache, pts: &[f32], dec: u8, rate: u8, side: &Side, unit: &'static str) {
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
        sf::label(sf::F14, rect(0, 173, 78, 18), num_unit(&mut b, hi, dec, unit), STATUS_FG, BG, 4, 2);
        sf::label(sf::F14, rect(0, 218, 78, 18), num_unit(&mut b, (hi + lo) / 2.0, dec, unit), STATUS_FG, BG, 4, 2);
        sf::label(sf::F14, rect(0, 259, 78, 18), num_unit(&mut b, lo, dec, unit), STATUS_FG, BG, 4, 2);
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
        l[k..k + 6].copy_from_slice(b"mS/div");
        sf::rbox(rect(2, 188, 76, 32), 8, BTN, BG);
        sf::glyphs(sf::F14, rect(2, 188, 76, 32), core::str::from_utf8(&l[..k + 6]).unwrap_or(""), STATUS_FG, BTN, 8, 2);
    }
    // Right column. Stock: "MAX:" / value, REL box, "MIN:" / value, two
    // rows each; now one row each ("MAX" tag, value right-aligned) to make
    // room for AVG and the elapsed time (F14 at spacing 0: the widest value,
    // "-600.00mV" / "9.9999MHz", is 52 px of the 54 px value cell).
    if c.side != Some((dec, unit)) {
        c.side = Some((dec, unit));
        c.rows = [i64::MIN; 4];
        c.time = None;
        lcd::fill_rect(402, 175, 78, 30, BG);
        lcd::fill_rect(402, 243, 78, 36, BG);
        for (i, t) in ["MAX", "AVG", "MIN"].iter().enumerate() {
            sf::label(sf::F14, rect(403, SIDE_Y[i], 21, 14), t, DIM, BG, 1, 0);
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
    for (i, v) in [side.max, side.avg, side.min].iter().enumerate() {
        let k = q(*v);
        if c.rows[i] != k {
            c.rows[i] = k;
            let t = v.map(|v| num_unit(&mut b, v, dec, unit)).unwrap_or("");
            sf::label(sf::F14, rect(424, SIDE_Y[i], 54, 14), t, STATUS_FG, BG, 4, 0);
        }
    }
    let k = q(side.rel.or(Some(0.0)));
    if c.rows[3] != k {
        c.rows[3] = k;
        sf::rbox(rect(402, 205, 76, 38), 8, BTN, BG);
        sf::glyphs(sf::F14, rect(405, 207, 76, 19), "REL:", STATUS_FG, BTN, 1, 2);
        let mut b2 = [0u8; 32];
        let rv = num_unit(&mut b2, side.rel.unwrap_or(0.0), dec, unit);
        sf::glyphs(sf::F14, rect(405, 222, 76, 19), rv, STATUS_FG, BTN, 1, 2);
    }
    let tk = side.secs.map(|t| (t, side.rec));
    if c.time != tk {
        c.time = tk;
        // "REC 01:02:03" (recording session) / "T 01:02:03" since the start.
        let mut t = [0u8; 16];
        let mut n = 0;
        if let Some(sec) = side.secs {
            let pre: &[u8] = if side.rec { b"REC " } else { b"T " };
            t[..pre.len()].copy_from_slice(pre);
            n = pre.len();
            let (h, m, s2) = (sec / 3600 % 100, sec / 60 % 60, sec % 60);
            for (i, v) in [h, m, s2].iter().enumerate() {
                t[n] = b'0' + (v / 10) as u8;
                t[n + 1] = b'0' + (v % 10) as u8;
                n += 2;
                if i < 2 {
                    t[n] = b':';
                    n += 1;
                }
            }
        }
        let fg = if side.rec { 0xFC40 } else { DIM };
        sf::label(sf::F14, rect(403, SIDE_Y[3], 75, 14), core::str::from_utf8(&t[..n]).unwrap_or(""), fg, BG, 4, 0);
    }
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
/// Combined DMM+scope view: the scope now fills everything below the half-size
/// DMM readout down to the DMM button row (bottom stays at y 277, as before).
pub const SCOPE_COMPACT: ScopeView = ScopeView { x0: 13, y0: 78, h: 199 };

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
        let mut tpl = [[0u16; 288]; 4];
        self.templates(&mut tpl);
        for i in 0..=scope::COLS as u16 {
            self.column_t(i, &tpl, None, None, false);
        }
    }

    /// Generator state in the scope top bar (the generator keeps running):
    /// the frequency actually output, "--" when the wave is silent at the set
    /// frequency (non-square above 50 kHz).
    pub fn gen_state(&self, g: &gen::Gen) {
        let mut t = [0u8; 32];
        let mut n = 0;
        if g.s.on {
            let mut b = [0u8; 24];
            let hz = g.out_hz();
            let k = if hz == 0 {
                b[..2].copy_from_slice(b"--");
                2
            } else {
                eng(&mut b, hz as f32, "Hz")
            };
            let name = g.s.wave.name().as_bytes();
            let pre = b"GEN ";
            for part in [&pre[..], &name[..3.min(name.len())], b" ", &b[..k]] {
                let m = part.len().min(32 - n);
                t[n..n + m].copy_from_slice(&part[..m]);
                n += m;
            }
        }
        sf::label(sf::F14, rect(296, 1, 136, 17), core::str::from_utf8(&t[..n]).unwrap_or(""), 0x04A9, S_BAR, 1, 1);
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
        let n = eng(&mut b, scope::VDIV[s.range] * s.probe, if s.ac { "V~" } else { "V" });
        sf::label(sf::F14, rect(146, y, 62, 17), core::str::from_utf8(&b[..n]).unwrap_or(""), 0xFFE0, bg, 1, 1);
        let n = eng(&mut b, sc.trig_volts(), "V");
        let mut t = [0u8; 28];
        let _ = full;
        t[0] = if s.rising { b'/' } else { b'\\' };
        t[1..1 + n].copy_from_slice(&b[..n]);
        sf::label(sf::F14, rect(212, y, 80, 17), core::str::from_utf8(&t[..1 + n]).unwrap_or(""), FG, bg, 1, 1);
        if full {
            let n = eng(&mut b, scope::VDIV[s.range] * s.probe, "V");
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

    /// Background templates of a column: [plain, dotted (i % 8 == 0), vertical
    /// grid line (i % 40 == 0), frame edge].
    fn templates(&self, t: &mut [[u16; 288]; 4]) {
        self.templates_c(t, None);
    }

    /// Templates with horizontal cursor rows (280-row model) drawn dotted.
    fn templates_c(&self, t: &mut [[u16; 288]; 4], ycur: Option<[i32; 2]>) {
        let h = self.h as usize;
        for y in 0..h {
            let r = (y as u32 * 278 / (self.h as u32 - 3).max(1)) as usize;
            let edge = y == 0 || y == h - 1;
            let hline = r % 35 == 0 && !edge;
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
    /// (rows of the 280-row model) and the trigger level tick; `line`: the
    /// level as a dashed line across the graph (4 px on / 4 off, behind the
    /// trace).
    fn column_t(&self, i: u16, t: &[[u16; 288]; 4], seg: Option<(i32, i32)>, trig: Option<i32>, line: bool) {
        let x = self.x0 + i;
        let h = self.h;
        let k = if i == 0 || i == 400 { 3 } else if i % 40 == 0 { 2 } else if i % 8 == 0 { 1 } else { 0 };
        let tpl = &t[k];
        let (a, b) = match seg {
            Some((a, b)) if i > 0 => (self.row_px(a) - self.y0, self.row_px(b) - self.y0),
            _ => (u16::MAX, 0),
        };
        let tr = if i < 6 || (line && i % 8 < 4) { trig.map(|r| self.row_px(r) - self.y0).unwrap_or(u16::MAX) } else { u16::MAX };
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
    pub fn frame(&self, f: &scope::Frame, sc: &scope::Scope, prev: &mut [(u16, u16); 401], cur: &Cursors, persist: bool, trig_line: bool) {
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
                        self.column_t(i as u16, &tpl, None, trig_row, trig_line);
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
                self.column_t(i as u16, if is_cur { &vt } else { &tpl }, Some((a, b)), trig_row, trig_line);
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
        lcd::fill_rect(x as u16, 302, 98, 1, S_FRAME);
        sf::label(sf::F14, rect(x, 303, 98, 16), t, 0xFFE0, S_BAR, 1, 0);
    }
}

/// Scope settings window (stock MF popup (13,20,401,192)): tabs Measure /
/// Display / Trigger / Settings.
pub const SCOPE_TABS: [&str; 4] = ["Measure", "Display", "Trigger", "Settings"];

fn tab_btn(r: sf::Rect, text: &str, on: bool) {
    let c = if on { SEL } else { 0x2966 };
    sf::rbox(r, 4, c, 0x10A3);
    sf::glyphs(sf::F14, r, text, STATUS_FG, c, 8, 1);
}

pub fn scope_menu(tab: usize, sc: &scope::Scope, sel: &[u8], roll: bool, persist: bool) {
    lcd::fill_rect(13, 20, 401, 192, 0x10A3);
    for (i, t) in SCOPE_TABS.iter().enumerate() {
        tab_btn(rect(16 + i as i16 * 99, 23, 95, 28), t, i == tab);
    }
    let s = &sc.s;
    match tab {
        0 => {
            for (k, (id, name, _)) in crate::scope_meas::ITEMS.iter().enumerate() {
                let r = rect(16 + (k % 5) as i16 * 79, 56 + (k / 5) as i16 * 38, 75, 34);
                tab_btn(r, name, sel.contains(id));
            }
        }
        1 => {
            tab_btn(rect(16, 60, 190, 34), if roll { "Roll: ON" } else { "Roll: OFF" }, roll);
            tab_btn(rect(210, 60, 190, 34), if persist { "Persist: INF" } else { "Persist: OFF" }, persist);
        }
        2 => {
            tab_btn(rect(16, 60, 120, 34), "Rising", s.rising);
            tab_btn(rect(140, 60, 120, 34), "Falling", !s.rising);
            tab_btn(rect(16, 100, 120, 34), "Auto", s.mode == scope::TrigMode::Auto);
            tab_btn(rect(140, 100, 120, 34), "Normal", s.mode == scope::TrigMode::Normal);
            tab_btn(rect(264, 100, 120, 34), "Single", s.mode == scope::TrigMode::Single);
        }
        _ => {
            tab_btn(rect(16, 60, 120, 34), "DC", !s.ac);
            tab_btn(rect(140, 60, 120, 34), "AC", s.ac);
            for (k, p) in ["x1", "x10", "x100"].iter().enumerate() {
                let f = [1.0f32, 10.0, 100.0][k];
                tab_btn(rect(16 + k as i16 * 124, 100, 120, 34), p, s.probe == f);
            }
        }
    }
}

/// Scope settings window touch: Some((tab, item)) with item 255 = tab header.
pub fn scope_menu_hit(tab: usize, x: u16, y: u16) -> Option<(usize, usize)> {
    let (x, y) = (x as i16, y as i16);
    if !(13..414).contains(&x) || !(20..212).contains(&y) {
        return None;
    }
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
    // Values. The frequency shown is the one output; when this wave is silent
    // at the set frequency (non-square above 50 kHz) the set value is shown
    // greyed, as it is kept for a wave that can play it.
    let out = g.out_hz();
    let f = if out == 0 { s.freq_hz } else { out } as f32;
    let ffg = if out == 0 { 0x7BEF } else { STATUS_FG };
    let (fv, fu) = if f >= 1e6 { (f / 1e6, "MHz") } else if f >= 1e3 { (f / 1e3, "KHz") } else { (f, "Hz") };
    let mut t = [0u8; 16];
    let fs = crate::dmm::format(&mut t, fv, if fv >= 100.0 { 0 } else if fv >= 10.0 { 1 } else { 2 });
    sf::label(sf::LETTERS, rect(262, 83, 150, 30), fs, ffg, G_BOX, 4, 0);
    sf::label(sf::TEXT22, rect(421, 90, 50, 26), fu, ffg, G_BOX, 1, 0);
    let mut t = [0u8; 16];
    let a = crate::dmm::format(&mut t, s.amp_v, 1);
    sf::label(sf::LETTERS, rect(262, 173, 150, 30), a, STATUS_FG, G_BOX, 4, 0);
    sf::label(sf::TEXT22, rect(421, 176, 50, 26), "V", STATUS_FG, G_BOX, 1, 0);
    let mut t = [0u8; 16];
    let d = crate::dmm::format(&mut t, s.duty as f32 / 100.0, 1);
    let duty_ok = s.wave == gen::Wave::Square && out <= 50_000;
    let fg = if duty_ok { STATUS_FG } else { 0x7BEF };
    sf::label(sf::LETTERS, rect(262, 266, 150, 30), d, fg, G_BOX, 4, 0);
    sf::label(sf::TEXT22, rect(421, 269, 50, 26), "%", fg, G_BOX, 1, 0);
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
        sf::label(sf::LETTERS, rect(262, 83, 150, 30), input, 0x04FC, 0x3A08, 4, 0);
        sf::label(sf::TEXT22, rect(421, 90, 50, 26), unit, 0x04FC, 0x3A08, 1, 0);
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

/// System settings (subset of the stock sections that this firmware has).
/// "Device" (index 7) opens the stock §6 Device/About page. "Back" is the
/// tappable title bar (stock settings have a title-bar back, not a list row),
/// handled by `title_back_hit`, so the list itself is only the settings.
pub const SETTINGS: [&str; 8] = [
    "Dim screen", "Shutdown", "Backlight", "Volume", "Bluetooth", "Start page", "Analog power",
    "Device",
];

/// Settings/menu-page title bar: 40 px, bg 0x2966 (stock gui_v2_6.h:101).
pub const PAGE_BAR_H: i16 = 40;
/// Settings list geometry. Taller rows than the cramped 9-row version (vendor
/// rows are ~37 px, gui_v2_6.h:109); 8 rows at pitch 34 fit the 320 px panel.
const SET_Y0: i16 = 44;
const SET_PITCH: i16 = 34;
const SET_ROW_H: i16 = 31;
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
    sf::glyphs(sf::TEXT22, rect(12, 0, 110, PAGE_BAR_H), "< Back", STATUS_FG, 0x2966, 1, 0);
}

/// A tap on the title bar is "go back".
pub fn title_back_hit(y: u16) -> bool {
    (y as i16) < PAGE_BAR_H
}

pub fn settings(vals: &[&str; 8]) {
    list_page("Settings", &SETTINGS, vals);
}

/// A settings-style page: title bar with "< Back", up to 8 rows of
/// label + value (SET_* geometry; `settings_hit` / `list_hit` map touches).
pub fn list_page(title: &str, names: &[&str], vals: &[&str]) {
    page_title(title);
    lcd::fill_rect(0, PAGE_BAR_H as u16, lcd::W, lcd::H - PAGE_BAR_H as u16, M_BG);
    for (i, name) in names.iter().enumerate().take(8) {
        let r = set_row(i);
        sf::rbox(r, 6, M_TILE, M_BG);
        // Label then value kept close together (value left-aligned just past a
        // 190 px label column) instead of flung to the far right edge.
        sf::glyphs(sf::TEXT22, rect(r.x + 12, r.y, 190, r.h), name, STATUS_FG, M_TILE, 1, 0);
        sf::glyphs(sf::TEXT22, rect(r.x + 202, r.y, 250, r.h), vals[i], M_ACCENT, M_TILE, 1, 0);
    }
}

pub fn settings_hit(y: u16) -> Option<usize> {
    list_hit(y, SETTINGS.len())
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
    if i < n.min(8) && within { Some(i) } else { None }
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
