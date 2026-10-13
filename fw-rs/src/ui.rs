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

pub struct DmmView {
    last_unit: &'static str,
    last_mode: u8,
    last_top: Option<(bool, bool, bool, bool)>,
}

impl DmmView {
    pub const fn new() -> Self {
        DmmView { last_unit: "", last_mode: 255, last_top: None }
    }

    pub fn draw_static(&mut self, button: Button) {
        lcd::fill_rect(0, BAR_H, lcd::W, lcd::H - BAR_H, BG);
        lcd::fill_rect(0, 0, lcd::W, BAR_H, BAR);
        for i in 0..5u16 {
            lcd::fill_rect(8 + i * 94, 153, 88, 19, BTN);
        }
        button_row(&DMM_BUTTONS, Some(button_index(button)));
        self.invalidate();
    }

    pub fn invalidate(&mut self) {
        self.last_unit = "";
        self.last_mode = 255;
        self.last_top = None;
    }

    pub fn reading(&mut self, r: &Reading, hold: bool, rel: bool, compact: bool) {
        let mut buf = [0u8; 16];
        let s = meter::text(&mut buf, &r.main);
        if compact {
            // One line under the status bar (DMM + scope view).
            let mut l = [0u8; 40];
            let line = val_line(&mut l, &r.main);
            text::draw_right(&UNIT, 0, 400, 84, line, VALUE_RED, BG);
            if self.last_mode != r.mode {
                self.last_mode = r.mode;
                let e = text::draw(&LABEL, 408, 80, sub_label(r.mode), FG, BG);
                lcd::fill_rect(e, 80 - LABEL.ascent, lcd::W - e, LABEL.height(), BG);
            }
        } else {
            text::draw_right(&VALUE, 0, VALUE_X1, VALUE_BASE, s, VALUE_RED, BG);
            if self.last_unit != r.main.unit || self.last_mode != r.mode {
                self.last_unit = r.main.unit;
                self.last_mode = r.mode;
                let end = text::draw(&UNIT, UNIT_X, UNIT_BASE, r.main.unit, VALUE_RED, BG);
                if end < lcd::W {
                    lcd::fill_rect(end, UNIT_BASE - UNIT.ascent, lcd::W - end, UNIT.height(), BG);
                }
                let e = text::draw(&LABEL, 404, 96, sub_label(r.mode), FG, BG);
                lcd::fill_rect(e, 96 - LABEL.ascent, lcd::W - e, LABEL.height(), BG);
            }
            // Secondary readouts in the first two boxes.
            for (i, a) in [r.aux1, r.aux2].iter().enumerate() {
                let x0 = 8 + i as u16 * 94;
                let mut l = [0u8; 40];
                let line = match a {
                    Some(v) => val_line(&mut l, v),
                    None => "",
                };
                lcd::fill_rect(x0, 153, 88, 19, BTN);
                text::draw_right(&SMALL, x0, x0 + 86, 168, line, FG, BTN);
            }
        }
        let top = (r.auto, r.auto_plus, hold, rel);
        if self.last_top != Some(top) {
            self.last_top = Some(top);
            lcd::fill_rect(0, 0, 300, BAR_H, BAR);
            let s = if !r.auto { "MANU" } else if r.auto_plus { "AUTO+" } else { "AUTO" };
            text::draw(&LABEL, 4, 27, s, FG, BAR);
            text::draw(&LABEL, 120, 27, if hold { "HOLD" } else { "RUN" }, FG, BAR);
            if rel {
                text::draw(&LABEL, 210, 27, "REL", FG, BAR);
            }
        }
    }
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
