//! Screens. Geometry and colours of the multimeter page measured on read-backs
//! of the stock screen (fw-rs/ref/); the main reading is red instead of the
//! stock orange. No framebuffer: every element is drawn as complete cells /
//! columns so redraws do not flicker.

use crate::dmm::{Button, Reading};
use crate::gen;
use crate::lcd::{self, Color};
use crate::meter;

/// Power-meter screens (DMM page slots, entry dialog, settings page).
#[path = "ui_power.rs"]
pub mod pw;

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
/// Stock value colour in the V functions from 30 V (gui_v2_6.h:157/175,
/// GUI_C_VALUE_HV); the lightning glyph comes on at 100 V.
pub const VALUE_HV: Color = 0xC9A0;

pub const BAR_H: u16 = 34;
/// Left edge of the DMM status-bar items after the logo (4 + LOGO + 3).
const BAR_X: i16 = 4 + sf::LOGO + 3;
/// Extra-digit cell of the main readout (x, width); see DmmView::reading.
/// 375..402, before the limit badge (403..433, y 72..88) and the unit box
/// (from 404); 28 px = the widest half-size F78 digit ("4", 56 / 2).
const XT_X: u16 = 375;
const XT_W: u16 = 28;
const VALUE_BASE: u16 = 146;
pub const BTN_Y: u16 = 280;

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
    /// Generator value entry (frequency / Vpp / duty).
    Keypad,
    /// Device / About page (stock gui_v2_6.h section 6 "6.产品信息").
    Device,
    /// DMM AutoHold and Hi/Lo limits of the current function (MF).
    Limits,
    /// Screenshot + log gallery (FR-16, gallery.rs).
    Gallery,
    /// On-device help (UF-29), Settings 2/3 -> Help: HELP pages.
    Help,
}

impl View {
    /// Views reachable by swiping, in order.
    pub const CYCLE: [View; 4] = [View::Dmm, View::DmmScope, View::Scope, View::Gen];
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

fn button_index(b: Button) -> usize {
    Button::ALL.iter().position(|&x| x == b).unwrap_or(0)
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

/// UF-26: caption of the 4-20 mA loop percentage (DC mA, aux2 "%").
const LOOP_CAP: &str = "4-20mA";

/// The aux readouts as shown. UF-26: the DC mA loop percentage (aux2 "%",
/// dmm.rs: (|mA| - 4) x 6.25, OL above 125 %) is hidden outside -25..125 %
/// (a current that is not a 4-20 mA loop signal); the bool says it is that
/// percentage (drawn with the LOOP_CAP caption).
fn aux_shown(r: &Reading) -> ([Option<crate::dmm::Val>; 2], bool) {
    let lp = r.mode == 8 && !r.acdc && r.aux2.is_some_and(|v| v.unit == "%");
    let a2 = r.aux2.filter(|v| !lp || (!v.ol && (-25.0..=125.0).contains(&v.v)));
    ([r.aux1, a2], lp && a2.is_some())
}

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
    last_top: Option<(bool, u8, bool, crate::sentinel::Alert)>,
    last_aux: [bool; 2],
    /// Temperature sensor name shown instead of AUTO / MANU (set by the app,
    /// `None` outside the temperature function).
    pub label: Option<&'static str>,
    last_label: Option<&'static str>,
    /// Limit badge as last drawn (meter::V_*), 255 = not drawn.
    last_lim: u8,
    /// Temperature sensor chip as last drawn (aux2 slot), None = not drawn.
    last_chip: Option<&'static str>,
    /// Per aux row: the value part (text, AC+DC / loop flags) and the unit
    /// part (unit, AC+DC) as last drawn; None = repaint. A reading that
    /// leaves a part unchanged does not repaint it.
    aux_val: [Option<([u8; 16], u8)>; 2],
    aux_unit: [Option<(&'static str, bool)>; 2],
    /// UF-30: the fast bargraph in the history row's place (Settings 2/2
    /// "DMM row": Bargraph, the default / History). Set by the app before
    /// `draw_static`.
    pub bar_on: bool,
    bar: crate::bargraph::Bar,
    /// Bargraph scale as last labelled: (full scale bits, unit, bipolar),
    /// None = labels to draw; and the overload marker as last drawn.
    bar_scale: Option<(u32, &'static str, bool)>,
    bar_ol: Option<bool>,
    /// ui-review #13: draw the value in the stock >= 30 V red (set by the
    /// app before `reading`); as last drawn.
    pub hv_red: bool,
    last_red: bool,
    /// Power meter shown (set by the app before `draw_static`): the
    /// history row holds the power boxes, never the bargraph.
    pub power: bool,
    pub pwc: pw::PwCache,
}

/// Bargraph geometry in the history row's place (rows 150..172 between the
/// F78 ink, which ends at 146, and the chart frame at 176):
///   segments  12..410 x 150..158 (50 x 8 px pitch, bargraph.rs)
///   labels    F14 row 161..172 under the scale: 0 / half / full scale
///             (bipolar: -FS / 0 / +FS), the full scale with its unit
///   OL marker 418..472 x 147..160 (red "OL" on overload)
pub const BAR_RECT: (u16, u16, u16, u16) = (12, 150, 400, 9);
const BAR_LBL_Y: i16 = 161;
const BAR_LBL_H: i16 = 12;
const BAR_OL: sf::Rect = rect(418, 147, 54, 14);

/// Bottom button `i` (0..5) with its stock glyphs, in the box (x, y, 76, 40)
/// (DMM page: stock (2 + 80 i, BTN_Y); combined view: the function band).
/// `ohm_on`: the stock m+8 "ON" (low-current, in-circuit) ohm variant is
/// selected (dmm.rs `ohm_var`): the ohm button carries the stock small "ON"
/// tag (gui_v2_6.h: glyph 0x20 of font 0x0806c724 at +(50,19,20,12) of the
/// 40 px button, align 1; here 2 px higher for the 36 px button and 1 px
/// right, 3 px clear of the ohm glyph's ink).
fn dmm_button(i: usize, x: i16, y: i16, h: i16, outside: Color, selected: bool, mode: u8, acdc: bool, ohm_on: bool) {
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
        2 => {
            sf::glyphs(sf::BTN_GLYPH, r(0, -1, 76), "\u{3a9}", STATUS_FG, c, 8, 0);
            if ohm_on {
                sf::glyphs(sf::ON_TAG, rect(x + 51, y + h / 2 - 3, 20, 12), " ", STATUS_FG, c, 1, 0);
            }
        }
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

pub fn func_band(button: Button, mode: u8, acdc: bool, ohm_on: bool) {
    lcd::fill_rect(0, FUNC_Y, lcd::W, MEAS_Y - FUNC_Y, S_BAR);
    lcd::fill_rect(0, FUNC_Y, lcd::W, 1, S_FRAME);
    let sel = button_index(button);
    for i in 0..6 {
        dmm_button(i, 4 + 79 * i as i16, FUNC_Y as i16 + 4, 36, S_BAR, i == sel, mode, acdc, ohm_on);
    }
}

/// Function button (0..5) of the band under a tap, None outside the band.
pub fn func_band_hit(x: u16, y: u16) -> Option<usize> {
    if (FUNC_Y..MEAS_Y).contains(&y) { Some((x.saturating_sub(2) / 79).min(5) as usize) } else { None }
}

// ------------------------------------------------------------ chips

/// Chip fill (temperature sensor pill).
const CHIP: Color = 0x3186;

/// Temperature sensor chip (owner 2026-10-13: the sensor type must be
/// legible at a glance): the full T_LABELS name in TEXT22 on a CHIP pill,
/// on the DMM page in the aux2 slot, in the meter strip as the second sub
/// row. Pill: text + 2 x CHIP_PAD wide.
const CHIP_PAD: i16 = 8;

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
            last_chip: None,
            aux_val: [None; 2],
            aux_unit: [None; 2],
            bar_on: true,
            bar: crate::bargraph::Bar::new(BAR_RECT.0, BAR_RECT.1, BAR_RECT.2, BAR_RECT.3, false),
            bar_scale: None,
            bar_ol: None,
            hv_red: false,
            last_red: false,
            power: false,
            pwc: pw::PwCache::new(),
        }
    }

    /// Bargraph scale labels for full scale `fs` in `unit` (empty = none,
    /// temperature).
    fn bar_labels(&self, fs: f32, unit: &str) {
        lcd::fill_rect(8, BAR_LBL_Y as u16, 468, BAR_LBL_H as u16, BG);
        if unit.is_empty() {
            return;
        }
        let num = |b: &mut [u8; 16], v: f32| -> usize {
            let mut t = [0u8; 16];
            let s = crate::dmm::format(&mut t, v, 2);
            let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s };
            let n = s.len().min(16);
            b[..n].copy_from_slice(&s.as_bytes()[..n]);
            n
        };
        let mut u = crate::scope_ui::Txt::new("");
        ascii_unit(unit, &mut u);
        let (bx, mid, end) = (self.bar.x as i16, self.bar.mid_x() as i16, self.bar.end_x() as i16);
        // Left / centre / right labels.
        let mut l = [0u8; 16];
        let mut c = [0u8; 16];
        let mut r = [0u8; 24];
        let mut v = [0u8; 16];
        let k = num(&mut v, fs);
        let (nl, nc, mut nr);
        if self.bar.bipolar {
            l[0] = b'-';
            l[1..1 + k].copy_from_slice(&v[..k]);
            nl = 1 + k;
            c[0] = b'0';
            nc = 1;
            r[0] = b'+';
            r[1..1 + k].copy_from_slice(&v[..k]);
            nr = 1 + k;
        } else {
            l[0] = b'0';
            nl = 1;
            nc = num(&mut c, fs / 2.0);
            r[..k].copy_from_slice(&v[..k]);
            nr = k;
        }
        sf::label(sf::F14, rect(bx, BAR_LBL_Y, 80, BAR_LBL_H), core::str::from_utf8(&l[..nl]).unwrap_or(""), DIM, BG, 1, 1);
        let us = u.as_str();
        r[nr] = b' ';
        let k = us.len().min(24 - nr - 1);
        r[nr + 1..nr + 1 + k].copy_from_slice(&us.as_bytes()[..k]);
        nr += 1 + k;
        sf::label(sf::F14, rect(mid - 40, BAR_LBL_Y, 81, BAR_LBL_H), core::str::from_utf8(&c[..nc]).unwrap_or(""), DIM, BG, 8, 1);
        sf::label(sf::F14, rect(end - 120, BAR_LBL_Y, 120, BAR_LBL_H), core::str::from_utf8(&r[..nr]).unwrap_or(""), DIM, BG, 4, 1);
    }

    /// UF-30: feed the bargraph with a raw (un-smoothed, pre-REL) reading;
    /// bipolar in DC V / mV / A (not AC+DC), unipolar elsewhere, empty in
    /// temperature (no meaningful full scale). No-op in history mode.
    pub fn bar_update(&mut self, r: &Reading) {
        if !self.bar_on || self.power {
            return;
        }
        let temp = r.mode == 15;
        let fs = crate::bargraph::full_scale(r.main.dec, r.main.unit);
        let key = if temp { (0, "", false) } else { (fs.to_bits(), r.main.unit, self.bar.bipolar) };
        if self.bar_scale != Some(key) {
            self.bar_scale = Some(key);
            self.bar_labels(fs, key.1);
        }
        let f = if temp { Some(0.0) } else { crate::bargraph::frac(r.main.v, fs, r.main.ol) };
        let ol = f.is_none();
        if self.bar_ol != Some(ol) {
            self.bar_ol = Some(ol);
            sf::label(sf::F14, BAR_OL, if ol { "OL" } else { "" }, 0xF800, BG, 1, 1);
        }
        self.bar.update(f);
    }

    /// Static part of the standalone DMM page.
    pub fn draw_static(&mut self, button: Button, mode: u8, acdc: bool, ohm_on: bool) {
        lcd::fill_rect(0, BAR_H, lcd::W, lcd::H - BAR_H, BG);
        lcd::fill_rect(0, 0, lcd::W, BAR_H, BAR);
        sf::logo(4, 7, BAR);
        if self.bar_on && !self.power {
            self.bar.bipolar = matches!(mode, 1 | 3 | 7..=9) && !acdc;
            self.bar.draw_static(BG);
            self.bar_scale = None;
            self.bar_ol = None;
        } else {
            for i in 0..5i16 {
                sf::rbox(rect(5 + 95 * i, 153, 88, 19), 3, BAR, BG);
            }
        }
        let sel = button_index(button);
        // 36 px tall (rows 280..315) and from x 4: 4 px off the screen edges.
        for i in 0..6 {
            dmm_button(i, 4 + 79 * i as i16, BTN_Y as i16, 36, BG, i == sel, mode, acdc, ohm_on);
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
        self.last_chip = None;
        self.aux_val = [None; 2];
        self.aux_unit = [None; 2];
        self.pwc = pw::PwCache::new();
    }

    /// Draw a reading on the standalone DMM page. `hold`: Meter::hold_state
    /// (0 RUN, 1 HOLD, 2 AutoHold tracking, 3 AutoHold captured); `lim`:
    /// Meter::verdict.
    pub fn reading(&mut self, r: &Reading, hold: u8, lim: u8) {
        let mut buf = [0u8; 16];
        let (s, f, ext) = fmt7(&mut buf, &r.main, true);
        let mut ub = [0u8; 16];
        let unit = sf::unit_text(r.main.unit, &mut ub);
        // ui-review #13: the stock >= 30 V cue, the value in VALUE_HV (V
        // functions, set by the app from the live reading). A colour change
        // repaints every digit.
        let color = if self.hv_red { VALUE_HV } else { VALUE };
        let recolor = self.last_red != self.hv_red;
        self.last_red = self.hv_red;
        // Only the changed digits are redrawn (cell layout unchanged).
        let same_layout = self.last_fmt == f && (self.last_ext != 0) == ext.is_some() && !recolor;
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
        sf::cells_diff(sf::F78, rect(40, 68, w as i16, 80), s, prev, cells, color, BG);
        if let Some(e) = ext {
            if !same_layout || last_ext != e {
                let r = rect(XT_X as i16, 68, XT_W as i16, 80);
                sf::label_half(sf::F78, r, e as char, VALUE_BASE as i16, color, BG);
            }
        } else if last_ext != 0 {
            // The full cells end at 400: the rest of the +1 digit cell.
            lcd::fill_rect(400, 68, XT_X + XT_W - 400, 80, BG);
        }
        if self.last_unit != r.main.unit || self.last_mode != mkey {
            self.last_unit = r.main.unit;
            self.last_mode = mkey;
            // The aux row layout follows the mode (AC+DC tags, loop caption).
            self.aux_val = [None; 2];
            self.aux_unit = [None; 2];
            // From x 404: clear of the +1 digit cell (375..402).
            // Power meter fallback "VA": its V and A touch at the stock -1.
            let sp = if r.main.unit.ends_with("VA") { 1 } else { -1 };
            sf::label(sf::UNIT, rect(404, 114, 72, 32), unit, VALUE, BG, 4, sp);
            // Stock box (437,76,32,32) align 7, widened 3 px right so the
            // 35 px AC glyph is not clipped (ends at 471, 3 px after the
            // limit badge 403..433).
            sf::label(sf::MODE, rect(437, 76, 35, 32), mode_glyph(r.mode, r.acdc || r.power.is_some()), VALUE, BG, 7, 0);
            if !r.acdc {
                // Leaving AC+DC: the tag slots left of the aux values.
                for i in 0..2 {
                    lcd::fill_rect(AUX_TAG_X[i] as u16, 36, (AUX_VX[i] - AUX_TAG_X[i]) as u16, 30, BG);
                }
            }
        }
        // Aux readouts (stock: value right-aligned cells at y 40, unit after).
        let (aux, lp) = aux_shown(r);
        let temp = r.mode == 15 && self.label.is_some();
        for (i, a) in aux.iter().enumerate() {
            // Value boxes end 4 px before the unit (separate items: >= 3 px).
            let (vx, ux) = (AUX_VX[i] + 30, if i == 0 { 178 } else { 400 });
            if i == 1 && temp {
                // Sensor chip (CHIP_PAD note), right-aligned in the aux2
                // slot (260..476); redrawn only on a change.
                let l = self.label.unwrap_or("");
                if self.last_chip != self.label || !self.last_aux[1] {
                    let w = sf::text_width(sf::TEXT22, l, 0) + 2 * CHIP_PAD;
                    let c = rect(472 - w, 38, w, 26);
                    lcd::fill_rect(260, 36, (c.x - 260) as u16, 30, BG);
                    lcd::fill_rect(c.x as u16, 36, (476 - c.x) as u16, 2, BG);
                    lcd::fill_rect(c.x as u16, 64, (476 - c.x) as u16, 2, BG);
                    lcd::fill_rect(472, 38, 4, 26, BG);
                    sf::rbox(c, 6, CHIP, BG);
                    sf::glyphs(sf::TEXT22, c, l, FG, CHIP, 8, 0);
                    self.last_chip = self.label;
                    self.aux_val[1] = None;
                    self.aux_unit[1] = None;
                }
                self.last_aux[1] = true;
                continue;
            }
            if i == 1 && self.last_chip.take().is_some() {
                // Leaving temperature: the chip spans columns the value /
                // unit boxes below do not repaint (397..398).
                lcd::fill_rect(260, 36, 216, 30, BG);
                self.aux_val[1] = None;
                self.aux_unit[1] = None;
            }
            match a {
                Some(v) => {
                    let mut t = [0u8; 16];
                    let txt = meter::text(&mut t, v);
                    let mut u = [0u8; 16];
                    let mut key = [0u8; 16];
                    let kn = txt.len().min(16);
                    key[..kn].copy_from_slice(&txt.as_bytes()[..kn]);
                    let vkey = Some((key, (i == 1 && lp) as u8 | (r.acdc as u8) << 1));
                    if self.aux_val[i] == vkey {
                        // Value part unchanged.
                    } else if i == 1 && lp {
                        // UF-26: "-25.0".."125.0" (<= 78 px: ink from 319)
                        // right-aligned in 316..396, the F14 caption
                        // right-aligned in 256..315 (ink from ~272: clear of
                        // aux1's unit box, which ends at 254; the None
                        // branch below clears it). The two boxes tile
                        // 256..396, so an older text leaves nothing behind.
                        sf::label(sf::LETTERS, rect(316, 36, 81, 30), txt, VALUE, BG, 4, 0);
                        sf::label(sf::F14, rect(256, 36, 60, 30), LOOP_CAP, VALUE, BG, 4, 1);
                    } else if r.acdc {
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
                    self.aux_val[i] = vkey;
                    // AC+DC: the aux1 unit box (V..uA, <= 48 px) ends at 231,
                    // the aux2 tag slot starts at 232; else full width (kHz 57 px).
                    let ukey = Some((v.unit, r.acdc));
                    if self.aux_unit[i] != ukey {
                        self.aux_unit[i] = ukey;
                        let uw = if i == 0 && r.acdc { 54 } else { 76 };
                        lcd::fill_rect((ux - 1) as u16, 36, 4, 30, BG);
                        sf::label(sf::LETTERS, rect(ux + 3, 36, uw - 3, 30), sf::unit_text(v.unit, &mut u), VALUE, BG, 1, 0);
                    }
                    self.last_aux[i] = true;
                }
                None if self.last_aux[i] => {
                    lcd::fill_rect((vx - 30) as u16, 36, (ux + 76 - vx + 30) as u16, 30, BG);
                    self.last_aux[i] = false;
                    self.aux_val[i] = None;
                    self.aux_unit[i] = None;
                }
                None => {}
            }
        }
        let top = (r.auto && !r.auto_plus, hold, r.auto_plus, r.alert);
        // Temperature: the status-bar slot shows AUTO / MANU like the other
        // functions (the sensor name is the chip above; the deg F second
        // readout that used to sit here is gone: one global unit, owner).
        if self.last_top != Some(top) || self.last_label != self.label {
            self.last_top = Some(top);
            self.last_label = self.label;
            let s = if !r.auto { "MANU" } else if r.auto_plus { "AUTO+" } else { "AUTO" };
            if temp {
                sf::label(sf::F21, rect(BAR_X, 1, 90, 32), s, STATUS_FG, BAR, 1, 0);
            } else if let Some(l) = self.label {
                sf::label(sf::F14, rect(BAR_X, 1, 90, 32), l, STATUS_FG, BAR, 1, 1);
            } else {
                sf::label(sf::F21, rect(BAR_X, 1, 90, 32), s, STATUS_FG, BAR, 1, 0);
            }
            hold_label(hold, r.alert);
        }
        if self.last_lim != lim {
            self.last_lim = lim;
            lim_badge(lim);
        }
        if let Some(pv) = r.power.filter(|_| self.power) {
            self.power_row(&pv);
        }
    }

}

/// ASCII form of a unit for the 12 px stock font (no ohm / micro / degree
/// glyphs): ohm -> "Ohm", micro -> "u", "deg C" -> "C".
fn ascii_unit(u: &str, out: &mut crate::scope_ui::Txt) {
    for c in u.chars() {
        match c {
            '\u{3a9}' => out.push("Ohm"),
            '\u{b5}' => out.push("u"),
            '\u{b0}' => {}
            c => {
                let mut b = [0u8; 4];
                out.push(c.encode_utf8(&mut b));
            }
        }
    }
}

/// Meter strip contents of the stock-style combined view (scope_ui::
/// meter_strip): the reading as on the old strip (value without the extra
/// digit, OL = "0L"), sub readings (AC+DC parts tagged "AC" / "DC", Hz and
/// duty, the 4-20 mA percentage, the temperature sensor name), one state
/// badge by priority HOLD / A-HOLD > A-JACK / V-JACK? / LIVE? (r.alert) >
/// HV (>= 30 V) > REC > REL / limit
/// verdict (alternating with `alt`) > range (AUTO / AUTO+ / MANU).
pub fn strip_data(r: &Reading, hold: u8, lim: u8, rel: bool, hv: u8, alt: bool, label: Option<&'static str>, rec: bool) -> crate::scope_ui::StripData {
    use crate::scope_ui::{StripData, Txt};
    let mut d = StripData::EMPTY;
    d.func = match crate::meter::func_of(r) {
        0 | 2 => "AC V",
        1 | 3 => "DC V",
        4..=6 => "AC A",
        7..=9 => "DC A",
        10 => "OHM",
        11 => "CONT",
        12 => "DIODE",
        13 => "CAP",
        14 => "FREQ",
        16 | 17 => "AC+DC",
        crate::dmm::pw::FUNC => "POWER",
        _ => "TEMP",
    };
    let mut buf = [0u8; 16];
    let (v, _, _) = fmt7(&mut buf, &r.main, false);
    d.value = Txt::new(v.trim());
    let mut ub = [0u8; 16];
    let unit = sf::unit_text(r.main.unit, &mut ub);
    d.unit = Txt::new(unit);
    d.unit_letters = !unit.chars().all(|c| sf::SC_DIGITS.glyph(c as u32).is_some());
    let (aux, lp) = aux_shown(r);
    let temp = r.mode == 15 && label.is_some();
    for (i, a) in aux.iter().enumerate() {
        let mut t = Txt::EMPTY;
        if temp && i == 1 {
            t.push(label.unwrap_or(""));
        } else if let Some(v) = a {
            if r.acdc {
                t.push(["AC ", "DC "][i]);
            } else if lp && i == 1 {
                t.push("4-20mA ");
            }
            let mut b = [0u8; 16];
            t.push(meter::text(&mut b, v));
            ascii_unit(v.unit, &mut t);
        }
        d.sub[i] = t;
    }
    let verdict = match lim {
        meter::V_PASS => Some(("PASS", 0x0400)),
        meter::V_HI => Some(("HI", 0xf800)),
        meter::V_LO => Some(("LO", 0xf800)),
        _ => None,
    };
    let alert = r.alert;
    let (b, c) = match hold {
        1 => ("HOLD", 0xfaa0),
        2 => ("A-HOLD", 0x5acb),
        3 => ("A-HOLD", 0xfaa0),
        // Wrong-connection alert (sentinel.rs): above HV, below HOLD.
        _ if alert != crate::sentinel::Alert::None => (alert.strip_badge(), alert.color()),
        _ if hv >= 1 => ("HV", 0xc9a0),
        _ if rec => ("REC", 0xf800),
        _ if rel && (verdict.is_none() || alt) => ("REL", 0xb833),
        _ if verdict.is_some() => verdict.unwrap_or(("", 0)),
        _ if temp => ("", 0x5acb),
        _ if !r.auto => ("MANU", 0x5acb),
        _ if r.auto_plus => ("AUTO+", 0x5acb),
        _ => ("AUTO", 0x5acb),
    };
    d.badge = b;
    d.badge_col = c;
    d.hv = hv >= 2;
    d
}

/// HOLD / RUN slot of the DMM status bar. Manual HOLD keeps the stock F21
/// "HOLD"; AutoHold is a different face so the two cannot be confused:
/// "A-HOLD" in TEXT22 (66 px, F21 would need 103), accent blue while it waits
/// for a stable value, inverted (dark on orange) while it shows a captured
/// one. Slot 119..190 (after the logo-shifted range label; uptime from 205).
///
/// With no HOLD / AutoHold the slot carries the wrong-connection alert
/// (sentinel.rs) instead of "RUN", in the same TEXT22 face: "A-JACK" white
/// on red (the HI / LO badge red), "V-JACK?" / "LIVE?" in the stock value
/// amber on the bar; letter-spaced 1 px, so the alert box runs to x 201
/// (3 px before the uptime at 205) and the other states clear 190..201.
fn hold_label(hold: u8, alert: crate::sentinel::Alert) {
    use crate::sentinel::Alert;
    let r = rect(119, 1, 71, 32);
    if hold == 0 && alert != Alert::None {
        let (fg, bg) = if alert == Alert::AJack { (0xFFFF, alert.color()) } else { (alert.color(), BAR) };
        lcd::fill_rect(119, 1, 83, 4, BAR);
        lcd::fill_rect(119, 29, 83, 4, BAR);
        sf::label(sf::TEXT22, rect(119, 5, 83, 24), alert.badge(), fg, bg, 8, 1);
        return;
    }
    lcd::fill_rect(190, 1, 12, 32, BAR);
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
    if b == Button::Ohm {
        // The stock's second ohm mode (m+8, toggled by a second tap on the
        // ohm button; "ON" tag on the button): the p8set rows, a lower test
        // current for in-circuit measurements. `sub` = Dmm::ohm_var here.
        for (i, name) in OHM_VARS.iter().enumerate() {
            let c = if i as u8 == sub { SEL } else { 0x2966 };
            let r = rect([8, 244][i], 182, 228, 38);
            sf::rbox(r, 6, c, BG);
            sf::glyphs(sf::TEXT22, r, name, STATUS_FG, c, 8, 0);
        }
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
    if b == Button::Amp {
        // Power meter entry (the wiring dialog first): between Hold/Limits
        // and OK.
        let r = rect(162, 240, 150, 38);
        sf::rbox(r, 6, 0x2966, BG);
        sf::glyphs(sf::TEXT22, r, "Power W", STATUS_FG, 0x2966, 8, 0);
    }
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

/// Ohm variant buttons of the Resistor Settings page (Dmm::ohm_var 0 / 1).
pub const OHM_VARS: [&str; 2] = ["Normal", "Low current ON"];

/// Range page touch: Ok(item index) / Err(0 DC, 1 AC, 3 AC+DC, 2 OK, 5 Power
/// (Current page),
/// 4 Hold/Limits); ohm: Err(0 / 1) = the OHM_VARS buttons.
pub fn range_hit(b: Button, x: u16, y: u16) -> Option<Result<usize, u8>> {
    let (x, y) = (x as i16, y as i16);
    for i in 0..range_items(b).len() {
        let r = range_rect(i);
        // 2 px (x) / 3 px (y) of slop: the 5 / 6 px gaps split between buttons.
        if x >= r.x - 2 && x < r.x + r.w + 2 && y >= r.y - 3 && y < r.y + r.h + 3 {
            return Some(Ok(i));
        }
    }
    if (179..223).contains(&y) && b == Button::Ohm {
        return Some(Err(if x < 240 { 0 } else { 1 }));
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
    if (240..278).contains(&y) && (162..312).contains(&x) && b == Button::Amp {
        return Some(Err(5));
    }
    None
}

/// Temperature sensor names on screen (dmm.rs T_NAMES order).
pub const T_LABELS: [&str; 7] = ["TC K", "PT100", "PT1000", "NTC10k B3950", "NTC10k B3435", "NTC100k B3950", "NTC user"];
/// Sensor buttons of the temperature page (dmm.rs T_NAMES order).
const T_KEYS: [&str; 7] = ["TC K", "PT100", "PT1000", "10k 3950", "10k 3435", "100k 3950", "NTC user"];

/// Temperature page (MF on the temperature function), redesigned 2026-10-21
/// (owner: the "Offset 0" key was unclear, the offset slider unintuitive,
/// "NTC user" unexplained, the buttons "fuzzy" = hit boxes exactly the drawn
/// boxes, so a touch in the 5..6 px gaps did nothing):
///   rows 50 / 96   sensors (range-page grid), slot 7 = the global deg C /
///                  deg F unit (the Settings 2/2 row; a shortcut here)
///   row 144..180   "Offset"  [ - ]  value  [ + ]  (settings-stepper look;
///                  0.1 C steps; a tap on the value opens the signed keypad)
///   row 186..222   NTC user only: R25 and Beta fields (tap = keypad)
///   row 240..278   Hold/Limits, OK;  282..316 the info line.
/// Every hit box reaches 3 px into the gaps around its button.
#[derive(Clone, Copy)]
pub struct TempPage<'a> {
    pub sensor: u8,
    /// Offset of the sensor, C.
    pub offs: f32,
    pub unit_f: bool,
    /// NTC user parameters (ohm, K).
    pub r25: f32,
    pub beta: f32,
    pub info: &'a str,
}

/// Temperature page hit codes besides the sensors 0..6.
pub const TK_UNIT: u8 = 7;
pub const TK_MINUS: u8 = 8;
pub const TK_PLUS: u8 = 9;
pub const TK_OFFS: u8 = 10;
pub const TK_R25: u8 = 11;
pub const TK_BETA: u8 = 12;
pub const TK_LIMITS: u8 = 254;
pub const TK_OK: u8 = 255;

const T_ROW_Y: i16 = 144;
const T_NTC_Y: i16 = 186;
const T_ROW_H: i16 = 36;

/// Button rectangle of a temperature-page code (None: not on this page).
fn temp_rect(sensor: u8, code: u8) -> Option<sf::Rect> {
    Some(match code {
        0..=7 => range_rect(code as usize),
        TK_MINUS => rect(132, T_ROW_Y, 72, T_ROW_H),
        TK_OFFS => rect(210, T_ROW_Y, 186, T_ROW_H),
        TK_PLUS => rect(402, T_ROW_Y, 70, T_ROW_H),
        TK_R25 if sensor == 6 => rect(8, T_NTC_Y, 228, T_ROW_H),
        TK_BETA if sensor == 6 => rect(244, T_NTC_Y, 228, T_ROW_H),
        TK_LIMITS => rect(8, 240, 150, 38),
        TK_OK => rect(319, 240, 150, 38),
        _ => return None,
    })
}

const T_CODES: [u8; 15] = [0, 1, 2, 3, 4, 5, 6, TK_UNIT, TK_MINUS, TK_OFFS, TK_PLUS, TK_R25, TK_BETA, TK_LIMITS, TK_OK];

/// Offset text "+7.3 C" (TEXT22 has no degree sign).
fn temp_offs_text<'a>(b: &'a mut [u8; 24], offs: f32) -> &'a str {
    let mut t = [0u8; 16];
    let v = crate::dmm::format(&mut t, offs, 1);
    join(b, &[if offs > 0.0 { "+" } else { "" }, v, " C"])
}

/// One temperature-page button, normal or pressed (stock pressed colour,
/// ui-review #11: feedback from touch-down).
pub fn temp_key(p: &TempPage, code: u8, pressed: bool) {
    let Some(r) = temp_rect(p.sensor, code) else { return };
    let sel = code < 7 && code == p.sensor;
    let c = if pressed { K_SEL } else if sel { SEL } else { 0x2966 };
    let mut b = [0u8; 24];
    let mut t = [0u8; 16];
    let txt: &str = match code {
        0..=6 => T_KEYS[code as usize],
        TK_UNIT => if p.unit_f { "Unit: F" } else { "Unit: C" },
        TK_MINUS => "-",
        TK_PLUS => "+",
        TK_OFFS => temp_offs_text(&mut b, p.offs),
        TK_R25 => {
            let k = p.r25 / 1000.0;
            let v = crate::dmm::format(&mut t, k, if k < 10.0 { 2 } else if k < 100.0 { 1 } else { 0 });
            join(&mut b, &["R25  ", v, " kohm"])
        }
        TK_BETA => {
            let v = crate::dmm::format(&mut t, p.beta, 0);
            join(&mut b, &["Beta  ", v, " K"])
        }
        TK_LIMITS => "Hold/Limits",
        _ => "OK",
    };
    if code == TK_OFFS {
        // The value field: darker than the keys, the value in orange.
        let c = if pressed { K_SEL } else { BAR };
        sf::rbox(r, 6, c, BG);
        sf::glyphs(sf::TEXT22, r, txt, VALUE, c, 8, 0);
    } else {
        sf::rbox(r, 6, c, BG);
        sf::glyphs(sf::TEXT22, r, txt, STATUS_FG, c, 8, 0);
    }
}

pub fn temp_page(p: &TempPage) {
    lcd::fill_rect(0, 0, lcd::W, 40, 0x2966);
    lcd::fill_rect(0, 40, lcd::W, lcd::H - 40, BG);
    sf::logo(4, 10, 0x2966);
    sf::glyphs(sf::TEXT22, rect(0, 0, 480, 40), "Temperature Settings", STATUS_FG, 0x2966, 8, 0);
    sf::glyphs(sf::TEXT22, rect(8, T_ROW_Y, 118, T_ROW_H), "Offset", STATUS_FG, BG, 1, 0);
    for code in T_CODES {
        temp_key(p, code, false);
    }
    if p.sensor != 6 {
        // The NTC row's place: how to enter the offset exactly.
        let hint = if p.sensor == 0 { "Tap the value to type it (TC: on the CJ)" } else { "Tap the value to type it" };
        sf::glyphs(sf::TEXT22, rect(8, T_NTC_Y, 464, T_ROW_H), hint, DIM, BG, 8, 0);
    }
    sf::glyphs(sf::TEXT22, rect(8, 282, 464, 34), p.info, STATUS_FG, BG, 1, 0);
}

/// Temperature page touch: a T_CODES code (3 px of slop into the gaps:
/// the gaps are >= 6 px, so the halves split between neighbours).
pub fn temp_hit(sensor: u8, x: u16, y: u16) -> Option<u8> {
    let (x, y) = (x as i16, y as i16);
    for s in [0i16, 3] {
        for code in T_CODES {
            if let Some(r) = temp_rect(sensor, code) {
                if x >= r.x - s && x < r.x + r.w + s && y >= r.y - s && y < r.y + r.h + s {
                    return Some(code);
                }
            }
        }
    }
    None
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
    /// CSV recording to the disk running (csvlog.rs).
    pub csv: bool,
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

/// CSV recording badge colour (status blocks): red. The MIN/MAX session
/// alone shows REC in the value orange (as "REC hh:mm:ss" in the DMM chart
/// column).
const REC_CSV: Color = 0xF800;

/// Right part of the DMM page status bar (34 px, stock positions): uptime,
/// REC (CSV recording; the MIN/MAX session has its own line in the chart
/// column), lock, BT link, battery.
pub fn status_right(st: &Status) {
    let mut t = [0u8; 8];
    let mut b = [0u8; 4];
    let bc = if st.charging { 0x04A9 } else { STATUS_FG };
    sf::label(sf::F21, rect(205, 5, 112, 24), hms(&mut t, st.uptime_s), STATUS_FG, BAR, 1, -1);
    // Between the lock (ink ..357) and the BT icon (390..).
    sf::label(sf::F14, rect(361, 10, 27, 15), if st.csv { "REC" } else { "" }, REC_CSV, BAR, 8, 1);
    sf::label(sf::LOCK, rect(335, 7, 24, 20), if st.locked { "\u{20}" } else { "\u{21}" }, STATUS_FG, BAR, 8, 0);
    sf::label(sf::SAVE_BT, rect(390, 0, 15, 32), if st.bt { "\u{21}" } else { "" }, STATUS_FG, BAR, 1, 0);
    sf::label(sf::BATTERY, rect(435, 9, 40, 16), battery_glyph(&mut b, st.bars), bc, BAR, 8, 0);
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
    rchip: Option<(u8, u32, bool)>,
    /// Trace rows (first, last) each column shows, NO_SPAN = none; only
    /// valid with `drawn` (else every column is drawn in full). A new point
    /// rewrites just the columns whose span changed, and in them only the
    /// rows of the old and the new span (one window per column).
    cols: [(u8, u8); CHART_W as usize],
    drawn: bool,
}

/// Column without trace (first > last).
const NO_SPAN: (u8, u8) = (u8::MAX, 0);

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

impl RateChip {
    /// What the chip shows (rate, rounded readings/s, applies): the cache
    /// key, so the EMA period's millisecond jitter does not repaint it.
    fn shown(&self) -> (u8, u32, bool) {
        let p = self.period_ms;
        let r10 = if p == 0 { 0 } else { (10_000 + p / 2) / p };
        (self.rate, if r10 >= 100 { (r10 + 5) / 10 * 10 } else { r10 }, self.applies)
    }
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

/// A tap on the DMM page's main value (the F78 cells 40..400 x 68..148,
/// the +1 digit cell, 4 px around): resolution / rate step (App).
pub fn value_hit(x: u16, y: u16) -> bool {
    (36..402).contains(&x) && (64..151).contains(&y)
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
        ChartCache {
            scale: None,
            side: None,
            rows: [i64::MIN; 4],
            time: None,
            rate: 0,
            rchip: None,
            cols: [NO_SPAN; CHART_W as usize],
            drawn: false,
        }
    }
    pub fn invalidate(&mut self) {
        *self = ChartCache::new();
    }
}

/// `side` None: the right column is not touched (the power meter draws its
/// own, `pw::side`).
pub fn chart(c: &mut ChartCache, pts: &[f32], dec: u8, rate: u8, side: Option<&Side>, unit: &'static str, rc: RateChip) {
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
    let full = !c.drawn;
    c.drawn = true;
    for x in 0..CHART_W {
        // Trace rows of this column (rows 1..CHART_H - 2: fit a u8).
        let span = if (x as usize) >= first {
            let y = row(pts[x as usize - first]);
            let s = match prev {
                Some(p) if p < y => (p, y),
                Some(p) => (y, p),
                None => (y, y),
            };
            prev = Some(y);
            (s.0 as u8, s.1 as u8)
        } else {
            NO_SPAN
        };
        let old = core::mem::replace(&mut c.cols[x as usize], span);
        // Rows to rewrite: the whole column on a full draw, else the union
        // of the old and the new trace span (the rest is unchanged).
        let (y0, y1) = if full {
            (0, CHART_H - 1)
        } else if old == span {
            continue;
        } else if old == NO_SPAN {
            (span.0 as u16, span.1 as u16)
        } else if span == NO_SPAN {
            (old.0 as u16, old.1 as u16)
        } else {
            (old.0.min(span.0) as u16, old.1.max(span.1) as u16)
        };
        let k = if x == 0 || x == CHART_W - 1 { 3 } else if x % 16 == 0 { 2 } else if x % 4 == 0 { 1 } else { 0 };
        let t = &tpl[k];
        lcd::set_window(CHART_X + x, CHART_Y + y0, CHART_X + x, CHART_Y + y1);
        // Template above the trace, trace, template below (NO_SPAN: the
        // first loop covers the column).
        let (a, b2) = (span.0 as u16, span.1 as u16);
        let a_end = a.min(y1 + 1);
        for y in y0..a_end {
            lcd::data(t[y as usize]);
        }
        for _ in a.max(y0)..=b2.min(y1) {
            lcd::data(0xFC40);
        }
        for y in (b2 + 1).max(y0).max(a_end)..=y1 {
            lcd::data(t[y as usize]);
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
    if c.rchip != Some(rc.shown()) {
        c.rchip = Some(rc.shown());
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
    let Some(side) = side else { return };
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
// The scope page and the combined view's graph are the stock replica
// (src/scope_ui.rs, src/scope_page.rs); what is left here: the colours the
// combined view's function band uses and the cursor state.

const S_BAR: Color = 0x4208;
const S_FRAME: Color = 0xAD55;
/// Measurement bar rows (the function band ends there).
const MEAS_Y: u16 = 301;

/// Scope cursors: two vertical (columns 1..399) and two horizontal (rows of
/// the 280-row model), as the stock X2-1 / Xf / Y2-1 items.
#[derive(Clone, Copy)]
pub struct Cursors {
    pub x_on: bool,
    pub y_on: bool,
    pub x: [u16; 2],
    pub y: [i32; 2],
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
    gen_box_value_bg(k, v, letters, unit, fg, G_BOX);
}

/// `gen_box_value` with the value field's background (the keypad's
/// selected prefill: K_SEL).
fn gen_box_value_bg(k: usize, v: &str, letters: bool, unit: &str, fg: Color, bg: Color) {
    let y = GEN_BOX_Y[k];
    sf::label(if letters { sf::LETTERS } else { sf::TEXT22 }, rect(262, y + 33, 150, 30), v, fg, bg, 4, 0);
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

/// RUN/STOP label of the generator bar (logo + label, x < GEN_RUN_W): a tap
/// toggles the output like the RUN key (ui-review #9).
const GEN_RUN_W: u16 = BAR_X as u16 + 80;

pub fn gen_run_hit(x: u16, y: u16) -> bool {
    x < GEN_RUN_W && y < 36
}

/// Generator bar status (ui-review #9; stock: uptime (186,0,120,36), battery
/// (430,8,40,18)): uptime, REC, lock, BT link, battery, at the DMM bar's x
/// positions (`status_right`), centred in the 36 px bar.
pub fn gen_status(st: &Status) {
    let mut t = [0u8; 8];
    let mut b = [0u8; 4];
    let bc = if st.charging { 0x04A9 } else { STATUS_FG };
    sf::label(sf::F21, rect(186, 6, 120, 24), hms(&mut t, st.uptime_s), STATUS_FG, G_BAR, 1, -1);
    sf::label(sf::F14, rect(361, 11, 27, 15), if st.csv { "REC" } else { "" }, REC_CSV, G_BAR, 8, 1);
    sf::label(sf::LOCK, rect(335, 8, 24, 20), if st.locked { "\u{20}" } else { "\u{21}" }, STATUS_FG, G_BAR, 8, 0);
    sf::label(sf::SAVE_BT, rect(390, 2, 15, 32), if st.bt { "\u{21}" } else { "" }, STATUS_FG, G_BAR, 1, 0);
    sf::label(sf::BATTERY, rect(435, 10, 40, 16), battery_glyph(&mut b, st.bars), bc, G_BAR, 8, 0);
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

/// `sel`: `input` is the current value, prefilled and selected (the next
/// digit replaces it): drawn on the stock pressed-key colour.
pub fn keypad(kind: u8, title: &str, input: &str, unit: &str, sel: bool) {
    let freq = kind == KP_FREQ;
    if freq {
        lcd::fill_rect(224, 142, 256, 178, K_PANEL);
    } else {
        lcd::fill_rect(224, 100, 256, 220, K_PANEL);
        sf::label(sf::TEXT22, rect(228, 108, 100, 26), title, 0x0000, K_PANEL, 1, 0);
    }
    keypad_input(freq, input, unit, sel);
    for &(_, _, c) in keypad_keys(kind) {
        if c != 0 {
            keypad_key(kind, c, false);
        }
    }
}

/// Keypad key `code` normal or pressed (stock pressed colour 0x04FC;
/// ui-review #11: feedback at touch-down).
pub fn keypad_key(kind: u8, code: u8, pressed: bool) {
    if let Some(&(r, l, c)) = keypad_keys(kind).iter().find(|k| k.2 == code && code != 0) {
        let bg = if pressed { K_SEL } else { K_KEY };
        sf::rbox(r, 4, bg, K_PANEL);
        sf::glyphs(if l.len() > 1 || c == b'<' { sf::TEXT22 } else { K_FONT }, r, l, STATUS_FG, bg, 8, 0);
    }
}

/// Selected keypad text (the prefilled current value): the stock pressed-key
/// colour as the field background.
const K_SEL: Color = 0x04FC;

/// The typed text: in the page's frequency field, or the popup's value box.
/// `sel`: the prefilled value, selected (see `keypad`).
pub fn keypad_input(freq: bool, input: &str, unit: &str, sel: bool) {
    if freq {
        if sel {
            gen_box_value_bg(0, input, true, unit, STATUS_FG, K_SEL);
        } else {
            gen_box_value(0, input, true, unit, K_SEL);
        }
        gen_box_note(0, "", G_DIM);
    } else {
        // 300..439: room for "-12.345" (112 px); titles end before 300.
        let (fg, bg) = if sel { (STATUS_FG, K_SEL) } else { (0x0000, K_PANEL) };
        sf::label(sf::LETTERS, rect(300, 104, 139, 34), input, fg, bg, 4, 0);
        sf::label(sf::TEXT22, rect(446, 110, 30, 22), unit, 0x0000, K_PANEL, 1, 0);
    }
}

pub fn keypad_hit(kind: u8, x: u16, y: u16) -> Option<u8> {
    let (x, y) = (x as i16, y as i16);
    // A key's own box first, then 2 px of slop around it (the 1..5 px gaps
    // between keys go to a neighbour instead of nowhere).
    for s in [0, 2] {
        for &(r, _, c) in keypad_keys(kind) {
            if c != 0 && x >= r.x - s && x < r.x + r.w + s && y >= r.y - s && y < r.y + r.h + s {
                return Some(c);
            }
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
    // Gallery row above the tiles (FR-16).
    sf::rbox(rect(46, 10, 388, 34), 8, M_TILE, M_BG);
    sf::glyphs(sf::TEXT22, rect(46, 10, 388, 34), "Gallery: screenshots and logs", STATUS_FG, M_TILE, 8, 0);
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
        // 4 px of slop: the 8 px gaps between tiles split between them.
        if x >= *tx - 4 && x < tx + 194 && y >= *ty - 4 && y < ty + 110 {
            return Some(i);
        }
    }
    if (42..438).contains(&x) && (276..318).contains(&y) {
        return Some(4);
    }
    if (42..438).contains(&x) && (6..48).contains(&y) {
        return Some(5);
    }
    None
}

/// System settings (subset of the stock sections that this firmware has),
/// three pages of at most SET_ROWS rows. Page 1: the last row "More" opens
/// page 2 (SETTINGS2): the DMM rate / smoothing (FR-05), the DMM page's
/// bargraph / history row (UF-30), Help; its "More" opens page 3
/// (SETTINGS3): the wrong-connection alerts and "Device", the stock §6
/// Device/About page. "Back" is the tappable title bar (stock settings have
/// a title-bar back, not a list row), handled by `title_back_hit` (page 3 ->
/// page 2 -> page 1 -> menu), so the lists hold only settings.
pub const SETTINGS: [&str; 9] = [
    "Dim screen", "Shutdown", "Backlight", "Key/alert volume", "Bluetooth", "Start page", "Analog power",
    "Scope drag", "More",
];
pub const SETTINGS2: [&str; 9] = ["DMM rate", "Smoothing", "DMM row", "Temp unit", "Scope swipe", "Scope press", "Scope tap step", "Help", "More"];
/// Page 3: the wrong-connection alerts (sentinel.rs; ON = all, A-JACK only,
/// OFF) and the Device / About page.
pub const SETTINGS3: [&str; 2] = ["Jack alerts", "Device"];

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

/// Settings page 1 rows with -/+ steppers (Backlight, Key/alert volume;
/// ui-review #12): no more wrap-around +10 % per tap.
pub const SET_STEP_ROWS: [usize; 2] = [2, 3];
/// Stepper keys of a settings row (x of "-", "+"), value between them.
const STEP_MINUS_X: i16 = 210;
const STEP_PLUS_X: i16 = 410;
const STEP_KEY_W: i16 = 54;

/// Settings page `page` (0 / 1 / 2) with its row values.
pub fn settings(page: u8, vals: &[&str]) {
    if page == 0 {
        let mut v = [""; 9];
        for (i, s) in vals.iter().enumerate().take(9) {
            v[i] = if SET_STEP_ROWS.contains(&i) { "" } else { s };
        }
        list_page("Settings", &SETTINGS, &v[..vals.len().min(9)]);
        for i in SET_STEP_ROWS {
            if let Some(t) = vals.get(i) {
                set_stepper(i, t);
            }
        }
    } else if page == 1 {
        list_page("Settings 2/3", &SETTINGS2, vals);
    } else {
        list_page("Settings 3/3", &SETTINGS3, vals);
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

/// "-" key, value, "+" key in the value column of settings row `i`.
fn set_stepper(i: usize, val: &str) {
    let r = set_row(i);
    for (x, t) in [(STEP_MINUS_X, "-"), (STEP_PLUS_X, "+")] {
        let k = rect(x, r.y + 3, STEP_KEY_W, r.h - 6);
        sf::rbox(k, 4, M_ACCENT, M_TILE);
        sf::glyphs(sf::TEXT22, k, t, STATUS_FG, M_ACCENT, 8, 0);
    }
    let vx = STEP_MINUS_X + STEP_KEY_W + 6;
    sf::glyphs(sf::TEXT22, rect(vx, r.y, STEP_PLUS_X - 6 - vx, r.h), val, M_ACCENT, M_TILE, 8, 0);
}

/// A tap at `x` on a stepper row: -1 (left of the value's centre: the
/// label and the "-" key) or +1.
pub fn settings_step(x: u16) -> i8 {
    let mid = (STEP_MINUS_X + STEP_KEY_W + STEP_PLUS_X) / 2;
    if (x as i16) < mid { -1 } else { 1 }
}

pub fn settings_hit(page: u8, y: u16) -> Option<usize> {
    list_hit(y, [SETTINGS.len(), SETTINGS2.len(), SETTINGS3.len()][page.min(2) as usize])
}

/// Row of a `list_page` with `n` rows under a touch at `y`.
pub fn list_hit(y: u16, n: usize) -> Option<usize> {
    // Each row owns the gap rows around it (half above, half below): a
    // fingertip that lands between two rows still picks the nearer one.
    let y = y as i16 - (SET_Y0 - (SET_PITCH - SET_ROW_H) / 2);
    if y < 0 {
        return None;
    }
    let i = (y / SET_PITCH) as usize;
    if i < n.min(SET_ROWS) { Some(i) } else { None }
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
        View::Keypad => "Keypad",
        View::Device => "Device",
        View::Limits => "Hold & Limits",
        View::Gallery => "Gallery",
        View::Help => "Help",
    }
}

/// On-device help (UF-29: owners have no manual; gestures, resets and limits
/// were undiscoverable). Pages of (topic, what it does) rows in the settings
/// list style; a tap below the title bar turns the page, the title bar or
/// MENU goes back. Only behaviour of this firmware (app.rs, gesture.rs) and
/// limits the repo documents (genplan.rs, docs/user-feedback.md section
/// 6.3 / 7 / 3) - no input ratings that are not sourced.
pub const HELP: [(&str, [(&str, &str); 8]); 4] = [
    (
        "Gestures",
        [
            ("Swipe left / right", "Next / previous view"),
            ("Swipe down (DMM)", "CSV record on / off"),
            ("3 fingers down", "Screenshot to disk"),
            ("2 fingers down", "Power off (confirm)"),
            ("Scope: drag up/down", "Move the trace"),
            ("Scope: right edge", "Drag trigger level"),
            ("Scope: drag sideways", "Trigger position"),
            ("Scope drag setting", "Direct or hold first"),
        ],
    ),
    (
        "Side keys",
        [
            ("MENU", "Main menu / back"),
            ("RUN", "HOLD; scope run/stop"),
            ("RUN held 1 s (DMM)", "AutoHold on / off"),
            ("AUTO", "Autorange / auto-set"),
            ("MF", "Range / scope menu"),
            ("Power key held 1.5 s", "Power off"),
            ("Power key twice", "Touch lock on / off"),
            ("Power key, locked", "Next DMM sub-mode"),
        ],
    ),
    (
        "Multimeter",
        [
            ("Tap history row", "Store the reading"),
            ("Hold history row", "Clear the history"),
            ("Tap MIN/MAX column", "Session on / off"),
            ("Hold MIN/MAX column", "Reset MIN/MAX/AVG"),
            ("Tap REL", "Zero on / off"),
            ("Tap value, rate chip", "Digits / rate"),
            ("Tap a function again", "Next sub-mode"),
            ("Hold meter strip", "Full DMM page"),
        ],
    ),
    (
        "Limits",
        [
            ("AUTO on volts", "Starts on mV: AUTO+"),
            ("Scope bandwidth", "Flat to about 3 MHz"),
            ("Gen sine/tri/ramp", "Up to 50 kHz"),
            ("Gen square", "Up to 10 MHz"),
            ("Gen amplitude", "0 to 3 Vpp"),
            ("Gen output", "Never apply voltage"),
            ("Supplied TC probe", "Rated to 300 C"),
            ("Key/alert volume", "Keys and alerts"),
        ],
    ),
];

/// Help page `page` (0..HELP.len()): title "Help n/N: topic", the rows as
/// the settings list (wider topic column: HELP_COL), the last row says
/// what a tap does.
pub const HELP_COL: i16 = 228;
pub fn help_page(page: usize) {
    let (topic, rows) = &HELP[page % HELP.len()];
    let mut t = [0u8; 40];
    let mut n = 0;
    let mut nb = [0u8; 4];
    let mut put = |p: &str| {
        let k = p.len().min(40 - n);
        t[n..n + k].copy_from_slice(&p.as_bytes()[..k]);
        n += k;
    };
    let digit = |b: &mut [u8; 4], d: usize| -> usize {
        b[0] = b'0' + d as u8;
        1
    };
    put("Help ");
    let k = digit(&mut nb, page % HELP.len() + 1);
    put(core::str::from_utf8(&nb[..k]).unwrap_or(""));
    put("/");
    let k = digit(&mut nb, HELP.len());
    put(core::str::from_utf8(&nb[..k]).unwrap_or(""));
    put(": ");
    put(topic);
    page_title(core::str::from_utf8(&t[..n]).unwrap_or("Help"));
    lcd::fill_rect(0, PAGE_BAR_H as u16, lcd::W, lcd::H - PAGE_BAR_H as u16, M_BG);
    let next = HELP[(page + 1) % HELP.len()].0;
    let last = ("Tap: next page", next);
    for (i, (name, val)) in rows.iter().chain(core::iter::once(&last)).enumerate().take(SET_ROWS) {
        let r = set_row(i);
        sf::rbox(r, 6, M_TILE, M_BG);
        let fg = if i + 1 == SET_ROWS { M_ACCENT } else { STATUS_FG };
        sf::glyphs(sf::TEXT22, rect(r.x + 12, r.y, HELP_COL, r.h), name, fg, M_TILE, 1, 0);
        sf::glyphs(sf::TEXT22, rect(r.x + 12 + HELP_COL, r.y, r.w - 15 - HELP_COL, r.h), val, M_ACCENT, M_TILE, 1, 0);
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
