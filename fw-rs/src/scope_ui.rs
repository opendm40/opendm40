//! Replica of the stock DM40 V2.6 oscilloscope page (docs/stock-scope-ui.md,
//! port plan docs/stock-scope-port-plan.md; host reference renderer
//! tools/stock-ui/scope.rs). Everything is drawn straight to the panel (no
//! frame buffer) by composition (stockfont::paint): each widget is one LCD
//! window whose pixels are computed from its layers, with AA off and the
//! stock corner tables, so a repaint of any part is exact and flicker-free.
//!
//! The page is described by a plain `Model` (filled by the app from the scope
//! engine and its own state, or by tools/ui-render from scene values); the
//! widgets whose inputs changed since the last draw are repainted (`update`).
//! The graph keeps, per column, the trace / persistence rows it shows and
//! repaints only the rows that changed (`Graph::flush`): a drag of the trace
//! costs a few rows per column, not the whole 399 x 279 area.
//!
//! Geometry: full page frame (13,20,401,281); the combined view moves the
//! frame to (13,76,401,225) under the meter strip (rows of the 280-row model
//! scaled), everything else as the stock.
#![allow(dead_code)]

use crate::lcd::{self, Color};
use crate::stockfont::{self as sf, in_rbox, on_ring, paint, rect, Font, Ink, Rect};

// ---- colours (RGB565, spec sec. 0) ----
pub const C_BAR: Color = 0x4208; // top bar, strip, panel, measurement bar
pub const C_FILL: Color = 0x2124; // graph fill, record-bar inner, dark text on yellow
pub const C_FRAME: Color = 0xad55; // frame border, rings, cursor lines, math off
pub const C_PURPLE: Color = 0xa81f; // status badge, toasts
pub const C_ORANGE: Color = 0xfaa0; // STOP badge, TRIG button, Clear
pub const C_MAGENTA: Color = 0xb833; // timebase badge
pub const C_WHITE: Color = 0xffff;
pub const C_GREEN: Color = 0x07e0; // battery bar while charging
pub const C_DIS: Color = 0x7bef; // label button while its popup is open / pressed
pub const C_POP_BG: Color = 0xd679; // popup page / selected tab
pub const C_POP_TAB: Color = 0x5acb; // unselected tab / header
pub const C_SEL: Color = 0x059e; // list selection, spinner band, slider fill
pub const C_YELLOW: Color = 0xffe0;
/// Palette 0x0806c730: red orange yellow green blue purple brown pink gold.
pub const PALETTE: [Color; 9] = [0xf800, 0xfd20, 0xffe0, 0x0400, 0x001f, 0x8010, 0xa145, 0xfe19, 0xfea0];
pub const COLOUR_NAMES: [&str; 9] = ["red", "orange", "yellow", "green", "blue", "purple", "brown", "pink", "gold"];

pub const TB_LABELS: [&str; 30] = [
    "10ns", "20ns", "50ns", "100ns", "200ns", "500ns", "1us", "2us", "5us", "10us", "20us", "50us", "100us", "200us", "500us", "1ms", "2ms",
    "5ms", "10ms", "20ms", "50ms", "100ms", "200ms", "500ms", "1s", "2s", "5s", "10s", "20s", "50s",
];
/// V/div labels 0x0807551c (index = V/div index + 3; 1mV .. 10kV).
pub const VDIV_LABELS: [&str; 22] =
    ["1m", "2m", "5m", "10m", "20m", "50m", "100m", "200m", "500m", "1", "2", "5", "10", "20", "50", "100", "200", "500", "1k", "2k", "5k", "10k"];
pub const STATUS: [&str; 5] = ["WAIT", "AUTO", "T'D", "RUN", "STOP"];
pub const ST_WAIT: u8 = 0;
pub const ST_AUTO: u8 = 1;
pub const ST_TD: u8 = 2;
pub const ST_RUN: u8 = 3;
pub const ST_STOP: u8 = 4;

// ---- geometry ----
pub const TOP_BAR: Rect = rect(0, 0, 480, 20);
pub const PANEL: Rect = rect(414, 20, 66, 300);
pub const MEAS_BAR: Rect = rect(0, 301, 414, 19);
/// Combined view: graph frame top (under the 56 px meter strip at y 20).
pub const COMBINED_TOP: i16 = 76;
/// The drag / tap rect of the stock overlay (sec. 10.1).
pub const INNER: Rect = rect(0, 15, 414, 290);

/// Right-panel buttons (sec. 5), 60 x 40 at x 417.
pub const B_RECT: [Rect; 7] =
    [rect(417, 20, 60, 40), rect(417, 61, 60, 40), rect(417, 102, 60, 40), rect(417, 150, 60, 40), rect(417, 191, 60, 40), rect(417, 232, 60, 40), rect(417, 280, 60, 40)];

/// Everything the page shows.
#[derive(Clone, Copy, PartialEq)]
pub struct Model {
    /// ST_WAIT .. ST_STOP.
    pub status: u8,
    /// Timebase index 0..29 (TB_LABELS).
    pub tb: u8,
    /// V/div label index 0..21 (VDIV_LABELS; the probe-scaled V/div).
    pub vdiv: u8,
    /// Probe index 0..12 (0.1X .. 1000X; 3 = 1X).
    pub probe: u8,
    pub ac: bool,
    /// Trigger level row (-10..290 of the 280-row model).
    pub trig_row: i16,
    /// Trigger level in volts (badge text).
    pub trig_v: f32,
    pub rising: bool,
    /// Trigger time column (0..399; outside = off-screen variants).
    pub trig_px: i16,
    /// CH1 position rows (0 = centre).
    pub pos: i16,
    /// Palette index of CH1 (default 2 = yellow).
    pub colour: u8,
    /// Trace brightness / grid intensity, percent.
    pub bright: u8,
    pub grid: u8,
    /// Selection: level item 0 CH1 / 1 trigger level / 3, 4 Y cursors; time
    /// item 0 trigger position / 2, 3 X cursors.
    pub sel_v: u8,
    pub sel_h: u8,
    /// Guide lines of the selection shown (press .. 2 s after the last event).
    pub guide: bool,
    /// X cursor columns (1..399) / Y cursor rows (1..279) when shown.
    pub xcur: Option<[i16; 2]>,
    pub ycur: Option<[i16; 2]>,
    /// Battery level 0..5, charging.
    pub battery: u8,
    pub charging: bool,
    /// Roll mode (record bar "ROLL").
    pub roll: bool,
    /// Record bar window (start, end) as fractions of the record.
    pub record: (f32, f32),
    /// Generator output frequency, shown in the record bar while running
    /// (<= 0: off).
    pub gen_hz: f32,
    /// CH1 / math label grey (their popup is open).
    pub ch1_open: bool,
    pub math_open: bool,
    /// Right-panel button 1..7 drawn pressed (0 none; our feedback option).
    pub pressed: u8,
    /// Combined view geometry.
    pub combined: bool,
}

impl Model {
    pub const DEFAULT: Model = Model {
        status: ST_TD,
        tb: 13,
        vdiv: 9,
        probe: 3,
        ac: false,
        trig_row: 105,
        trig_v: 0.993,
        rising: true,
        trig_px: 200,
        pos: 0,
        colour: 2,
        bright: 100,
        grid: 50,
        sel_v: 0,
        sel_h: 0,
        guide: false,
        xcur: None,
        ycur: None,
        battery: 4,
        charging: false,
        roll: false,
        record: (0.0, 1.0),
        gen_hz: 0.0,
        ch1_open: false,
        math_open: false,
        pressed: 0,
        combined: false,
    };

    pub fn pal(&self) -> Color {
        PALETTE[self.colour as usize % 9]
    }
    /// Trace colour: palette blended into the fill by the brightness.
    pub fn trace_colour(&self) -> Color {
        blend565(self.pal(), C_FILL, (self.bright.min(100) as u32 * 255 / 100) as u8)
    }
    /// Persistence: the same at half alpha.
    pub fn half_colour(&self) -> Color {
        blend565(self.pal(), C_FILL, ((self.bright.min(100) as u32 * 255 / 100) >> 1) as u8)
    }
    pub fn grid_colour(&self) -> Color {
        blend565(0xffff, C_FILL, (self.grid.min(100) as u32 * 255 / 100) as u8)
    }
    /// Graph frame top / height.
    pub fn top(&self) -> i16 {
        if self.combined { COMBINED_TOP } else { 20 }
    }
    pub fn frame_h(&self) -> i16 {
        301 - self.top()
    }
    /// Coupling / probe glyph of font 0x08054fc0.
    pub fn probe_glyph(&self) -> u32 {
        (if self.ac { 0x2c } else { 0x2d }) + 2 * self.probe as u32
    }
}

/// FUN_08019038: `fg` over `bg` with alpha 0..255 (8-bit channel expansion,
/// 16-bit truncation as the stock).
pub fn blend565(fg: Color, bg: Color, a: u8) -> Color {
    if a > 252 {
        return fg;
    }
    if a < 3 {
        return bg;
    }
    let (a, na) = (a as u32, 255 - a as u32);
    let b = ((((fg as u32 & 0x1f) << 3 | 7) * a + ((bg as u32 & 0x1f) << 3 | 7) * na) & 0xffff) >> 11;
    let g = (((((fg as u32 >> 3) & 0xfc) | 3) * a + (((bg as u32 >> 3) & 0xfc) | 3) * na) & 0xffff) >> 10;
    let r = ((((fg as u32 >> 8) & 0xf8) | 7) * a + (((bg as u32 >> 8) & 0xf8) | 7) * na) & 0xf800;
    (r | g << 5 | b) as Color
}

// ------------------------------------------------------------------ text

/// Small text buffer.
pub struct Buf {
    b: [u8; 24],
    n: usize,
}

impl Buf {
    pub const fn new() -> Buf {
        Buf { b: [0; 24], n: 0 }
    }
    pub fn push(&mut self, s: &str) -> &mut Buf {
        let k = s.len().min(24 - self.n);
        self.b[self.n..self.n + k].copy_from_slice(&s.as_bytes()[..k]);
        self.n += k;
        self
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}

/// FUN_08032bfa (V / A / W): NaN " -- "; |v| < 1e-6 "%1.2fu"; else scaled
/// into [1, 1000) over u m - k M with "%1.2f" (< 9.995), "%2.1f" (< 99.95)
/// or "%3.0f".
pub fn fmt_v(out: &mut Buf, v: f32, unit: &str) {
    if !v.is_finite() {
        out.push(" -- ");
        return;
    }
    let mut t = [0u8; 16];
    if v.abs() < 1e-6 {
        out.push(crate::dmm::format(&mut t, v * 1e6, 2)).push("u").push(unit);
        return;
    }
    const P: [&str; 5] = ["u", "m", "", "k", "M"];
    let (mut x, mut i) = (v, 2usize);
    for _ in 0..5 {
        if x.abs() >= 1000.0 && i < 4 {
            x /= 1000.0;
            i += 1;
        } else if x.abs() < 1.0 && i > 0 {
            x *= 1000.0;
            i -= 1;
        } else {
            break;
        }
    }
    if !(1.0..1000.0).contains(&x.abs()) {
        out.push(" --");
        return;
    }
    let dec = if x.abs() < 9.995 { 2 } else if x.abs() < 99.95 { 1 } else { 0 };
    out.push(crate::dmm::format(&mut t, x, dec)).push(P[i]).push(unit);
}

/// "1V", "200mV": the V/div label and unit.
pub fn vdiv_text(out: &mut Buf, vdiv: u8) {
    out.push(VDIV_LABELS[vdiv.min(21) as usize]).push("V");
}

// ------------------------------------------------------------------ widgets

const K_TEXT: u8 = 0;
const K_SHAPE: u8 = 1;
const K_ICON: u8 = 2;

/// Layer tag for `paint`: layer (1-based) and glyph index.
#[inline]
fn tag(layer: u16, g: u8) -> u16 {
    layer << 8 | g as u16
}

/// A badge: rounded box `r` (radius 5, all corners) in `fill` over `outside`,
/// with up to three inks drawn in their colours on top.
fn badge(r: Rect, fill: Color, outside: Color, inner: Option<(Rect, Color)>, inks: &[(&Ink, Color)]) {
    let layers = [(K_SHAPE, outside), (K_SHAPE, fill), (K_TEXT, fill), (K_TEXT, fill), (K_TEXT, fill)];
    paint(r, &layers[..2 + inks.len()], |x, y| {
        for (k, (ink, c)) in inks.iter().enumerate() {
            if let Some(g) = ink.at(x, y) {
                return (*c, tag(3 + k as u16, g));
            }
        }
        if let Some((ir, ic)) = inner {
            if in_rbox(ir, 5, 0x0f, x, y) {
                return (ic, tag(2, 0));
            }
        }
        if in_rbox(r, 5, 0x0f, x, y) { (fill, tag(1, 0)) } else { (outside, 0) }
    });
}

/// 2.1 run-status badge.
pub fn status_badge(m: &Model) {
    let r = rect(13, 1, 62, 17);
    let t = Ink::new(sf::SC_C, r, STATUS[m.status.min(4) as usize], 8, 0);
    badge(r, if m.status == ST_STOP { C_ORANGE } else { C_PURPLE }, C_BAR, None, &[(&t, C_WHITE)]);
}

/// 2.2 timebase badge.
pub fn tb_badge(m: &Model) {
    let h = Ink::new(sf::SC_C, rect(83, 1, 12, 17), "H", 1, 0);
    let t = Ink::new(sf::SC_F12, rect(95, 1, 46, 17), TB_LABELS[m.tb.min(29) as usize], 8, 0);
    badge(rect(79, 1, 62, 17), C_MAGENTA, C_BAR, None, &[(&h, C_WHITE), (&t, C_WHITE)]);
}

/// 2.3 record-position bar; while running (record = screen) it shows the
/// generator frequency instead of the constant full-record line when the
/// generator is on (fw-rs addition, 12 px grey like "ROLL").
pub fn record_bar(m: &Model) {
    let r = rect(145, 1, 137, 17);
    let inner = rect(146, 2, 135, 15);
    let running = m.status != ST_STOP;
    let mut b = Buf::new();
    let (txt, line, lb, rb);
    if m.roll {
        txt = Ink::new(sf::SC_F12, inner, "ROLL", 8, 0);
        (line, lb, rb) = (None, Ink::none(), Ink::none());
    } else if running && m.gen_hz > 0.0 {
        b.push("G ");
        fmt_hz(&mut b, m.gen_hz);
        txt = Ink::new(sf::SC_F12, inner, b.as_str(), 8, 0);
        (line, lb, rb) = (None, Ink::none(), Ink::none());
    } else {
        txt = Ink::none();
        let (start, end) = if running { (0.0, 1.0) } else { m.record };
        let x0 = if start < 0.0 { 4 } else { (start * 129.0 + 4.0) as i16 };
        let len = (((end.min(1.0) - start.max(0.0)) * 129.0) as i16).max(0);
        line = Some((145 + x0, len));
        lb = if (0.0..1.0).contains(&start) { Ink::new(sf::SC_F12, rect(145 + x0 - 2, 2, 4, 17), "[", 1, 0) } else { Ink::none() };
        rb = if end <= 1.0 { Ink::new(sf::SC_F12, rect(145 + (end * 129.0 + 4.0) as i16 - 4, 2, 4, 17), "]", 1, 0) } else { Ink::none() };
    }
    let layers = [(K_SHAPE, C_BAR), (K_SHAPE, C_FRAME), (K_TEXT, C_FILL), (K_SHAPE, C_FILL), (K_ICON, C_FILL), (K_ICON, C_FILL)];
    paint(r, &layers, |x, y| {
        if let Some(g) = txt.at(x, y) {
            return (C_FRAME, tag(3, g));
        }
        if let Some(g) = lb.at(x, y) {
            return (C_FRAME, tag(5, g));
        }
        if let Some(g) = rb.at(x, y) {
            return (C_FRAME, tag(6, g));
        }
        if let Some((lx, ln)) = line {
            if y == 9 && x >= lx && x < lx + ln {
                return (C_YELLOW, tag(4, 0));
            }
        }
        if in_rbox(inner, 5, 0x0f, x, y) {
            (C_FILL, tag(2, 0))
        } else if in_rbox(r, 5, 0x0f, x, y) {
            (C_FRAME, tag(1, 0))
        } else {
            (C_BAR, 0)
        }
    });
}

/// "1.00kHz" (FUN_08032fa4 style, Hz .. MHz).
pub fn fmt_hz(out: &mut Buf, hz: f32) {
    let (x, p) = if hz >= 1e6 {
        (hz / 1e6, "MHz")
    } else if hz >= 1e3 {
        (hz / 1e3, "kHz")
    } else if hz >= 1.0 {
        (hz, "Hz")
    } else {
        (hz * 1e3, "mHz")
    };
    let dec = if x < 9.995 { 2 } else if x < 99.95 { 1 } else { 0 };
    let mut t = [0u8; 16];
    out.push(crate::dmm::format(&mut t, x * 1.000_001, dec)).push(p);
}

/// 2.4 CH1 V/div badge.
pub fn vdiv_badge(m: &Model) {
    let mut b = Buf::new();
    vdiv_text(&mut b, m.vdiv);
    let p = Ink::icon(sf::SC_PROBE, rect(286, 1, 18, 17), m.probe_glyph(), 4);
    let t = Ink::new(sf::SC_F12, rect(304, 1, 44, 17), b.as_str(), 8, 0);
    badge(rect(286, 1, 62, 17), m.pal(), C_BAR, None, &[(&p, C_FILL), (&t, C_FILL)]);
}

/// 2.5 trigger badge: edge glyph and level.
pub fn trig_badge(m: &Model) {
    let mut b = Buf::new();
    fmt_v(&mut b, m.trig_v, "V");
    let e = Ink::icon(sf::SC_EDGE, rect(352, 1, 12, 17), if m.rising { 0x20 } else { 0x21 }, 4);
    let t = Ink::new(sf::SC_F12, rect(364, 1, 50, 17), b.as_str(), 8, 0);
    badge(rect(352, 1, 62, 17), C_FRAME, C_BAR, Some((rect(353, 2, 60, 15), C_BAR)), &[(&e, m.pal()), (&t, m.pal())]);
}

/// 2.6 battery (outline + level clipped to (437,3,33,12)).
pub fn battery(m: &Model) {
    let r = rect(435, 1, 40, 16);
    let o = Ink::icon(sf::SC_BATT, r, 0x22, 0);
    let l = if m.battery > 0 { Ink::icon(sf::SC_BATT, r, 0x22 + m.battery.min(5) as u32, 0) } else { Ink::none() };
    let clip = rect(437, 3, 33, 12);
    let lc = if m.charging { C_GREEN } else { C_WHITE };
    paint(r, &[(K_ICON, C_BAR), (K_ICON, C_BAR)], |x, y| {
        if clip.has(x, y) {
            if let Some(g) = l.at(x, y) {
                return (lc, tag(2, g));
            }
        }
        match o.at(x, y) {
            Some(g) => (C_WHITE, tag(1, g)),
            None => (C_BAR, 0),
        }
    });
}

/// Right-panel button k (0..6) as the stock draws it (sec. 5).
pub fn panel_button(m: &Model, k: usize) {
    let r = B_RECT[k];
    let pal = m.pal();
    let mc = C_FRAME; // math off (no math engine): grey
    let mut b = Buf::new();
    vdiv_text(&mut b, m.vdiv);
    let pressed = m.pressed as usize == k + 1;
    let (fill, radius, mask, fg) = match k {
        0 => (pal, 8, 0x3, C_FILL),
        1 => (if m.ch1_open { C_DIS } else { pal }, 0, 0, C_FILL),
        2 => (pal, 8, 0xc, C_FILL),
        3 => (mc, 8, 0x3, C_WHITE),
        4 => (if m.math_open { C_DIS } else { mc }, 0, 0, C_WHITE),
        5 => (mc, 8, 0xc, C_WHITE),
        _ => (C_ORANGE, 8, 0xf, C_WHITE),
    };
    let fill = if pressed { C_DIS } else { fill };
    let (t1, t2) = match k {
        0 | 3 => (Ink::new(sf::SC_F20, r, "+", 8, 0), Ink::none()),
        2 | 5 => (Ink::new(sf::SC_F20, r, "-", 8, 0), Ink::none()),
        1 => (Ink::new(sf::SC_F20, rect(417, 61, 60, 24), "CHA", 8, 0), Ink::new(sf::SC_F12, rect(417, 85, 60, 16), b.as_str(), 8, 0)),
        4 => (Ink::new(sf::SC_F20, rect(417, 191, 60, 24), "--", 8, 0), Ink::new(sf::SC_F12, rect(417, 215, 60, 16), b.as_str(), 8, 0)),
        _ => (Ink::new(sf::SC_F20, r, "TRIG", 8, 0), Ink::none()),
    };
    paint(r, &[(K_SHAPE, C_BAR), (K_TEXT, fill), (K_TEXT, fill)], |x, y| {
        if let Some(g) = t1.at(x, y) {
            return (fg, tag(2, g));
        }
        if let Some(g) = t2.at(x, y) {
            return (fg, tag(3, g));
        }
        if mask == 0 || in_rbox(r, radius, mask, x, y) { (fill, tag(1, 0)) } else { (C_BAR, 0) }
    });
}

/// Measurement cell i (0..3): ringed box with `text` in the trace colour.
pub fn meas_cell(m: &Model, i: usize, text: &str) {
    let r = rect(13 + 100 * i as i16, 302, 98, 17);
    let t = Ink::new(sf::SC_F12, r, text, 8, 0);
    let pal = m.pal();
    paint(r, &[(K_SHAPE, C_BAR), (K_TEXT, C_BAR)], |x, y| {
        if let Some(g) = t.at(x, y) {
            return (pal, tag(2, g));
        }
        if on_ring(r, 5, 0x0f, x, y) {
            (C_FRAME, tag(1, 0))
        } else {
            (C_BAR, 0)
        }
    });
}

/// Every top-bar element.
pub fn top_bar(m: &Model) {
    status_badge(m);
    tb_badge(m);
    record_bar(m);
    vdiv_badge(m);
    trig_badge(m);
    battery(m);
}

pub fn panel(m: &Model) {
    for k in 0..7 {
        panel_button(m, k);
    }
}

/// Page background boxes and the frame border (no graph interior). The
/// combined view keeps y 20..`top` for the meter strip (drawn by ui.rs).
pub fn chrome(m: &Model) {
    let top = m.top() as u16;
    lcd::fill_rect(0, 0, 480, 20, C_BAR);
    lcd::fill_rect(414, 20, 66, 300, C_BAR);
    lcd::fill_rect(0, 301, 414, 19, C_BAR);
    lcd::fill_rect(0, 20, 13, 281, C_BAR);
    let h = m.frame_h() as u16;
    lcd::fill_rect(13, top, 401, 1, C_FRAME);
    lcd::fill_rect(13, top + h - 1, 401, 1, C_FRAME);
    lcd::fill_rect(13, top, 1, h, C_FRAME);
    lcd::fill_rect(413, top, 1, h, C_FRAME);
}

// ------------------------------------------------------------------ markers

/// Marker glyph codes of font 0x08065558 (sec. 3): family base + variant
/// (0 down, 1 left, 2 right, 3 up; + 4 filled; 0x31 / 0x35, an "A" pointing
/// left, do not exist).
const FAM_1: u32 = 0x20;
const FAM_2: u32 = 0x28;
const FAM_A: u32 = 0x30;
const FAM_T: u32 = 0x40;
const DOWN: u32 = 0;
const LEFT: u32 = 1;
const RIGHT: u32 = 2;
const UP: u32 = 3;
/// Eraser glyphs (by variant): down 'H', left 'I', right 'J', up 'K'.
const ERASER: [u32; 4] = [0x48, 0x49, 0x4a, 0x4b];

/// A 12 x 12 marker: icon and eraser glyph codes at (x, y), colour.
#[derive(Clone, Copy, PartialEq)]
pub struct Mark {
    pub x: i16,
    pub y: i16,
    pub icon: u8,
    pub mask: u8,
    pub col: Color,
}

impl Mark {
    pub const NONE: Mark = Mark { x: i16::MIN, y: 0, icon: 0, mask: 0, col: 0 };
    fn new(x: i16, y: i16, fam: u32, dir: u32, sel: bool, col: Color) -> Mark {
        Mark { x, y, icon: (fam + dir + if sel { 4 } else { 0 }) as u8, mask: ERASER[dir as usize] as u8, col }
    }
    pub fn some(&self) -> bool {
        self.x != i16::MIN
    }
    pub fn rect(&self) -> Rect {
        rect(self.x, self.y, 12, 12)
    }
}

/// Marker glyphs placed for compositing: (glyph, x, y) of the icon and the
/// eraser (both align 8 in the 12 x 12 box).
#[derive(Clone, Copy)]
struct MarkInk {
    icon: Option<(sf::Glyph, i16, i16)>,
    mask: Option<(sf::Glyph, i16, i16)>,
    col: Color,
}

fn place(font: Font, r: Rect, code: u32) -> Option<(sf::Glyph, i16, i16)> {
    let g = font.glyph(code)?;
    let lh = font.line_h() as i16;
    let x = r.x + ((r.w - g.w as i16) >> 1);
    let y = r.y + ((r.h - lh) >> 1) + lh - font.descent() - g.h as i16 - g.yoff as i16;
    Some((g, x, y))
}

#[inline]
fn ink(p: &Option<(sf::Glyph, i16, i16)>, x: i16, y: i16) -> bool {
    match p {
        Some((g, gx, gy)) => {
            let (dx, dy) = (x - gx, y - gy);
            dx >= 0 && dy >= 0 && dx < g.w as i16 && dy < g.h as i16 && g.alpha(dx as u32, dy as u32) >= 8
        }
        None => false,
    }
}

impl MarkInk {
    const NONE: MarkInk = MarkInk { icon: None, mask: None, col: 0 };
    fn of(m: &Mark) -> MarkInk {
        if !m.some() {
            return MarkInk::NONE;
        }
        let r = m.rect();
        MarkInk { icon: place(sf::SC_MARK, r, m.icon as u32), mask: place(sf::SC_MARK, r, m.mask as u32), col: m.col }
    }
    /// Some(colour) where the marker paints: icon colour, or the parent's
    /// background `bg` where only the eraser has ink.
    #[inline]
    fn at(&self, x: i16, y: i16, bg: Color) -> Option<Color> {
        if ink(&self.icon, x, y) {
            Some(self.col)
        } else if ink(&self.mask, x, y) {
            Some(bg)
        } else {
            None
        }
    }
}

/// Strip markers in draw order (creation order, the selected level item
/// last): CH1 zero, trigger level, Y cursors (sec. 3.1). Rows scaled to the
/// frame for the combined view.
pub fn strip_marks(m: &Model, out: &mut [Mark; 5]) -> usize {
    let top = m.top();
    let h = m.frame_h();
    let pal = m.pal();
    let level = |row: i16, fam: u32, sel: bool, col: Color| -> Mark {
        let fy = line_fy(h, row as i32);
        if fy < 0 {
            Mark::new(1, top, fam, UP, sel, col)
        } else if fy > h - 1 {
            Mark::new(1, top + h - 13, fam, DOWN, sel, col)
        } else {
            Mark::new(1, top + fy - 6, fam, RIGHT, sel, col)
        }
    };
    let ch1 = |sel| {
        // CH1 range is +-140 rows around the centre (the 280-row model).
        let row = 140 + m.pos;
        level(if m.pos < -140 { -1 } else if m.pos > 140 { 281 } else { row }, FAM_A, sel, pal)
    };
    let mut n = 0;
    let mut items: [(u8, Mark); 4] = [(0, Mark::NONE); 4];
    items[n] = (0, ch1(m.sel_v == 0));
    n += 1;
    items[n] = (1, level(if m.trig_row > 280 { 281 } else { m.trig_row }, FAM_T, m.sel_v == 1, pal));
    n += 1;
    if let Some(yc) = m.ycur {
        items[n] = (3, level(yc[0], FAM_1, m.sel_v == 3, C_WHITE));
        n += 1;
        items[n] = (4, level(yc[1], FAM_2, m.sel_v == 4, C_WHITE));
        n += 1;
    }
    let mut k = 0;
    for &(id, mk) in items[..n].iter().filter(|i| i.0 != m.sel_v) {
        let _ = id;
        out[k] = mk;
        k += 1;
    }
    for &(_, mk) in items[..n].iter().filter(|i| i.0 == m.sel_v) {
        out[k] = mk;
        k += 1;
    }
    for o in out[k..].iter_mut() {
        *o = Mark::NONE;
    }
    k
}

/// Top-of-graph marker for a column (FUN_08028bec), x graph-relative.
fn top_mark(top: i16, col: i16, fam: u32, sel: bool, colour: Color) -> Mark {
    let y = top + 1;
    if col < 12 {
        Mark::new(14 + col.max(0), y, fam, LEFT, sel, colour)
    } else if col + 12 <= 399 {
        Mark::new(14 + col - 6, y, fam, DOWN, sel, colour)
    } else {
        Mark::new(14 + if col > 399 { 387 } else { col - 13 }, y, fam, RIGHT, sel, colour)
    }
}

/// Graph markers in draw order: T, X cursor 1, X cursor 2.
pub fn graph_marks(m: &Model, out: &mut [Mark; 3]) {
    let top = m.top();
    out[0] = top_mark(top, m.trig_px, FAM_T, m.sel_h == 0, m.pal());
    out[1] = Mark::NONE;
    out[2] = Mark::NONE;
    if let Some(xc) = m.xcur {
        out[1] = top_mark(top, xc[0], FAM_1, m.sel_h == 2, C_WHITE);
        out[2] = top_mark(top, xc[1], FAM_2, m.sel_h == 3, C_WHITE);
    }
}

/// A touch zone of a marker (sec. 10.2): the 12x12 glyph rect grown by 10
/// px on every side; `level`: level group (CH1 0, trigger level 1, Y
/// cursors 3 / 4), else time group (trigger position 0, X cursors 2 / 3).
#[derive(Clone, Copy, PartialEq)]
pub struct Zone {
    pub level: bool,
    pub id: u8,
    pub r: Rect,
}

impl Zone {
    pub const NONE: Zone = Zone { level: false, id: 0, r: rect(0, 0, 0, 0) };
}

/// The marker zones of the page in creation order (level group first).
pub fn zones(m: &Model, out: &mut [Zone; 7]) -> usize {
    let grow = |r: Rect| rect(r.x - 10, r.y - 10, r.w + 20, r.h + 20);
    let mut n = 0;
    let mut mm = *m;
    mm.sel_v = 255; // creation order
    let mut marks = [Mark::NONE; 5];
    strip_marks(&mm, &mut marks);
    let ids: [u8; 4] = [0, 1, 3, 4];
    for (k, mk) in marks.iter().enumerate().take(4) {
        if mk.some() {
            out[n] = Zone { level: true, id: ids[k], r: grow(mk.rect()) };
            n += 1;
        }
    }
    let mut g = [Mark::NONE; 3];
    graph_marks(m, &mut g);
    let hids: [u8; 3] = [0, 2, 3];
    for (k, mk) in g.iter().enumerate() {
        if mk.some() {
            out[n] = Zone { level: false, id: hids[k], r: grow(mk.rect()) };
            n += 1;
        }
    }
    n
}

/// The stock zone search for a press at (x, y): the group other than the
/// last selected one (`last_h`: time group), zones after its current item;
/// else the same group cycling from its current item + 1 (wrapping, so
/// repeated presses on overlapping zones cycle through them); else the
/// other group from its first zone. Index into `z`.
pub fn pick_zone(z: &[Zone], sel_v: u8, sel_h: u8, last_h: bool, x: i16, y: i16) -> Option<usize> {
    let same_level = !last_h;
    let of = |level: bool| -> ([usize; 7], usize) {
        let mut l = [0usize; 7];
        let mut k = 0;
        for (i, q) in z.iter().enumerate() {
            if q.level == level && k < 7 {
                l[k] = i;
                k += 1;
            }
        }
        (l, k)
    };
    let cur = |level: bool, l: &[usize]| {
        let id = if level { sel_v } else { sel_h };
        l.iter().position(|&i| z[i].id == id)
    };
    let hit = |i: usize| z[i].r.has(x, y);
    let (ol, on) = of(!same_level);
    if let Some(c) = cur(!same_level, &ol[..on]) {
        if let Some(&i) = ol[c + 1..on].iter().find(|&&i| hit(i)) {
            return Some(i);
        }
    }
    let (sl, sn) = of(same_level);
    if sn > 0 {
        let c = cur(same_level, &sl[..sn]).unwrap_or(sn - 1);
        for d in 1..=sn {
            let i = sl[(c + d) % sn];
            if hit(i) {
                return Some(i);
            }
        }
    }
    ol[..on].iter().copied().find(|&i| hit(i))
}

/// Repaint strip rows y0..=y1 (x 1..12) with the markers `marks` (draw order).
fn strip_rows(marks: &[Mark], y0: i16, y1: i16) {
    let mut ink: [MarkInk; 5] = [MarkInk::NONE; 5];
    for (i, mk) in marks.iter().enumerate().take(5) {
        ink[i] = MarkInk::of(mk);
    }
    let n = marks.len().min(5);
    let layers = [(K_ICON, C_BAR), (K_ICON, C_BAR), (K_ICON, C_BAR), (K_ICON, C_BAR), (K_ICON, C_BAR)];
    paint(rect(1, y0, 12, y1 - y0 + 1), &layers[..n], |x, y| {
        for i in (0..n).rev() {
            if let Some(c) = ink[i].at(x, y, C_BAR) {
                return (c, if c == C_BAR { 0 } else { tag(i as u16 + 1, 0) });
            }
        }
        (C_BAR, 0)
    });
}

// ------------------------------------------------------------------ graph

/// Frame row of a level row (markers, guide lines: stock "20 + row").
pub fn line_fy(h: i16, row: i32) -> i16 {
    if h == 281 { row as i16 } else { (row * (h as i32 - 1) / 280) as i16 }
}

/// Frame row of trace row `r` of the 280-row model (clamped 0..278 as
/// FUN_0802dc28; the trace sits 1 px below its markers on the stock page).
pub fn trace_fy(h: i16, r: i32) -> i16 {
    1 + (r.clamp(0, 278) * (h as i32 - 3) / 278) as i16
}

/// One horizontal line of the graph overlay.
#[derive(Clone, Copy, PartialEq)]
struct HLine {
    fy: i16,
    solid: bool,
    col: Color,
}

/// One vertical line.
#[derive(Clone, Copy, PartialEq)]
struct VLine {
    fx: i16,
    col: Color,
}

const NL: usize = 3;

/// Overlay lines + top markers + colours: what besides the trace decides
/// the graph pixels.
#[derive(Clone, Copy, PartialEq)]
struct Deco {
    h: [Option<HLine>; NL],
    v: [Option<VLine>; NL],
    marks: [Mark; 3],
    grid: Color,
    trace: Color,
    half: Color,
    top: i16,
    fh: i16,
}

impl Deco {
    const NONE: Deco = Deco { h: [None; NL], v: [None; NL], marks: [Mark::NONE; 3], grid: 0, trace: 0, half: 0, top: 0, fh: 0 };

    fn of(m: &Model) -> Deco {
        let h = m.frame_h();
        let mut d = Deco {
            h: [None; NL],
            v: [None; NL],
            marks: [Mark::NONE; 3],
            grid: m.grid_colour(),
            trace: m.trace_colour(),
            half: m.half_colour(),
            top: m.top(),
            fh: h,
        };
        graph_marks(m, &mut d.marks);
        let ctr = d.trace;
        // 4.4: guide lines of the selected items, then the cursor lines.
        if m.guide {
            if m.sel_h == 0 && (1..=399).contains(&m.trig_px) {
                d.v[0] = Some(VLine { fx: m.trig_px, col: ctr });
            }
            match m.sel_v {
                0 => d.h[0] = Some(HLine { fy: line_fy(h, 140 + m.pos as i32), solid: true, col: ctr }),
                1 => d.h[0] = Some(HLine { fy: line_fy(h, m.trig_row as i32), solid: false, col: ctr }),
                _ => {}
            }
        }
        if let Some(xc) = m.xcur {
            d.v[1] = Some(VLine { fx: xc[0] + 1, col: C_FRAME });
            d.v[2] = Some(VLine { fx: xc[1] + 1, col: C_FRAME });
        }
        if let Some(yc) = m.ycur {
            d.h[1] = Some(HLine { fy: trace_fy(h, yc[0] as i32), solid: false, col: C_FRAME });
            d.h[2] = Some(HLine { fy: trace_fy(h, yc[1] as i32), solid: false, col: C_FRAME });
        }
        d
    }
}

/// No data in a column.
pub const EMPTY: (i16, i16) = (i16::MAX, i16::MIN);

/// The graph: what each column shows and what is on the panel.
pub struct Graph {
    /// Trace / persistence frame rows per graph column (EMPTY = none).
    pub tr: [(i16, i16); 399],
    pub ps: [(i16, i16); 399],
    d_tr: [(i16, i16); 399],
    d_ps: [(i16, i16); 399],
    deco: Deco,
    d_deco: Deco,
    valid: bool,
    /// A screen rect the graph leaves alone (an overlay drawn on top, e.g.
    /// the pinch bubble), and the one of the last flush.
    hole: Option<Rect>,
    d_hole: Option<Rect>,
    /// Pixels written by the last flush (host budget check / DBG).
    pub px: u32,
}

static mut GRAPH: Graph = Graph {
    tr: [EMPTY; 399],
    ps: [EMPTY; 399],
    d_tr: [EMPTY; 399],
    d_ps: [EMPTY; 399],
    deco: Deco::NONE,
    d_deco: Deco::NONE,
    valid: false,
    hole: None,
    d_hole: None,
    px: 0,
};

/// The graph state (single-threaded firmware).
pub fn graph() -> &'static mut Graph {
    unsafe { &mut *core::ptr::addr_of_mut!(GRAPH) }
}

/// Grid rule of the frame (sec. 4.1) at frame (fx, fy), 1 <= fx <= 399,
/// 1 <= fy <= h - 2; `cy` the centre row, `dh` the division height.
#[inline]
fn grid_on(fx: i16, fy: i16, cy: i16, dh: i16) -> bool {
    fx == 200
        || fy == cy
        || (fx % 40 == 0 && fy % 7 == 0)
        || (fy % dh == 0 && fx % 8 == 0)
        || (fx % 8 == 0 && (fy - cy).abs() <= 2)
        || (fy % 7 == 0 && (fx - 200).abs() <= 2)
}

/// Row intervals of one column to repaint.
struct Spans {
    s: [(i16, i16); 12],
    n: usize,
}

impl Spans {
    fn add(&mut self, a: (i16, i16)) {
        if a.0 > a.1 {
            return;
        }
        if self.n == self.s.len() {
            // Out of slots: widen the last one.
            let l = &mut self.s[self.n - 1];
            *l = (l.0.min(a.0), l.1.max(a.1));
            return;
        }
        self.s[self.n] = a;
        self.n += 1;
    }
    /// Sort and merge overlapping / adjacent spans, clamp to lo..=hi.
    fn merged(&mut self, lo: i16, hi: i16) -> &[(i16, i16)] {
        let s = &mut self.s[..self.n];
        s.sort_unstable_by_key(|a| a.0);
        let mut k = 0;
        for i in 0..s.len() {
            let (a, b) = (s[i].0.max(lo), s[i].1.min(hi));
            if a > b {
                continue;
            }
            if k > 0 && a <= s[k - 1].1 + 1 {
                s[k - 1].1 = s[k - 1].1.max(b);
            } else {
                s[k] = (a, b);
                k += 1;
            }
        }
        &self.s[..k]
    }
}

impl Graph {
    /// Forget what is on the panel: the next flush repaints the whole graph.
    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    /// Leave `r` (screen) alone from now on (None: paint it again).
    pub fn set_hole(&mut self, r: Option<Rect>) {
        self.hole = r;
    }

    /// Clear the trace and persistence (an empty graph).
    pub fn clear(&mut self) {
        self.tr = [EMPTY; 399];
        self.ps = [EMPTY; 399];
    }

    /// Bring the panel up to date with `tr` / `ps` and the model's lines,
    /// markers and colours, repainting only the rows that changed.
    pub fn flush(&mut self, m: &Model) {
        self.deco = Deco::of(m);
        let d = self.deco;
        let o = self.d_deco;
        let full = !self.valid || d.grid != o.grid || d.trace != o.trace || d.half != o.half || d.top != o.top || d.fh != o.fh;
        let h = d.fh;
        let (cy, dh) = ((h - 1) / 2, (h - 1) / 8);
        let mut minks = [MarkInk::NONE; 3];
        for (i, mk) in d.marks.iter().enumerate() {
            minks[i] = MarkInk::of(mk);
        }
        // Rows of changed horizontal lines (repainted in every column).
        let mut hrows = [0i16; 2 * NL];
        let mut nh = 0;
        if !full {
            for k in 0..NL {
                if d.h[k] != o.h[k] {
                    for l in [d.h[k], o.h[k]].into_iter().flatten() {
                        hrows[nh] = l.fy;
                        nh += 1;
                    }
                }
            }
        }
        let marks_changed = d.marks != o.marks;
        let covers = |marks: &[Mark; 3], fx: i16| marks.iter().any(|mk| mk.some() && 13 + fx >= mk.x && 13 + fx < mk.x + 12);
        let vl = |dd: &Deco, fx: i16| dd.v.iter().flatten().find(|l| l.fx == fx).map(|l| l.col);
        self.px = 0;
        for i in 0..399usize {
            let fx = i as i16 + 1;
            let mut sp = Spans { s: [(0, 0); 12], n: 0 };
            if full || vl(&d, fx) != vl(&o, fx) {
                sp.add((1, h - 2));
            } else {
                if self.tr[i] != self.d_tr[i] {
                    sp.add(self.tr[i]);
                    sp.add(self.d_tr[i]);
                }
                if self.ps[i] != self.d_ps[i] {
                    sp.add(self.ps[i]);
                    sp.add(self.d_ps[i]);
                }
                if marks_changed && (covers(&d.marks, fx) || covers(&o.marks, fx)) {
                    sp.add((1, 12));
                }
                for &r in &hrows[..nh] {
                    sp.add((r, r));
                }
                // A hole that went away / moved: its rows come back.
                if let Some(o) = self.d_hole.filter(|_| self.hole != self.d_hole) {
                    if 13 + fx >= o.x && 13 + fx < o.x + o.w {
                        sp.add((o.y - d.top, o.y + o.h - 1 - d.top));
                    }
                }
            }
            if sp.n == 0 {
                continue;
            }
            let (tr, ps) = (self.tr[i], self.ps[i]);
            let vcol = vl(&d, fx);
            let sx = 13 + fx;
            // Rows of this column under the hole (not painted).
            let hole = self.hole.filter(|r| sx >= r.x && sx < r.x + r.w).map(|r| (r.y - d.top, r.y + r.h - 1 - d.top));
            let mut cut = [(0i16, 0i16); 24];
            let mut nc = 0;
            for &(a, b) in sp.merged(1, h - 2) {
                match hole {
                    Some((ha, hb)) if a <= hb && b >= ha => {
                        if a < ha {
                            cut[nc] = (a, ha - 1);
                            nc += 1;
                        }
                        if b > hb {
                            cut[nc] = (hb + 1, b);
                            nc += 1;
                        }
                    }
                    _ => {
                        cut[nc] = (a, b);
                        nc += 1;
                    }
                }
            }
            for &(a, b) in &cut[..nc] {
                self.px += (b - a + 1) as u32;
                lcd::set_window(sx as u16, (d.top + a) as u16, sx as u16, (d.top + b) as u16);
                for fy in a..=b {
                    let sy = d.top + fy;
                    let c = 'px: {
                        if fy <= 12 {
                            for mi in (0..3).rev() {
                                if let Some(c) = minks[mi].at(sx, sy, C_FILL) {
                                    break 'px c;
                                }
                            }
                        }
                        if fy >= tr.0 && fy <= tr.1 {
                            break 'px d.trace;
                        }
                        if fy >= ps.0 && fy <= ps.1 {
                            break 'px d.half;
                        }
                        if let Some(c) = vcol {
                            if (fy - 1) % 2 == 0 && fy <= h - 4 {
                                break 'px c;
                            }
                        }
                        for l in d.h.iter().flatten() {
                            if l.fy == fy && fx <= 397 && (l.solid || fx % 2 == 1) {
                                break 'px l.col;
                            }
                        }
                        if grid_on(fx, fy, cy, dh) { d.grid } else { C_FILL }
                    };
                    lcd::data(c);
                }
            }
        }
        self.d_tr = self.tr;
        self.d_ps = self.ps;
        self.d_deco = d;
        self.d_hole = self.hole;
        self.valid = true;
    }
}

// ------------------------------------------------------------------ page

/// What the page last drew (None: nothing / invalid).
struct Shown {
    m: Option<Model>,
    strip: [Mark; 5],
    cells: [[u8; 16]; 4],
    cells_n: [u8; 4],
}

static mut SHOWN: Shown = Shown { m: None, strip: [Mark::NONE; 5], cells: [[0; 16]; 4], cells_n: [0; 4] };

fn shown() -> &'static mut Shown {
    unsafe { &mut *core::ptr::addr_of_mut!(SHOWN) }
}

/// Forget everything drawn (the next `update` draws the whole page).
pub fn invalidate() {
    let s = shown();
    s.m = None;
    s.cells_n = [255; 4];
    graph().invalidate();
}

/// Bring the page up to date with `m`: the chrome on a fresh page, then every
/// widget whose inputs changed, the strip markers and the graph.
pub fn update(m: &Model) {
    let s = shown();
    let old = s.m;
    let fresh = old.map_or(true, |o| o.combined != m.combined);
    if fresh {
        chrome(m);
        s.strip = [Mark::NONE; 5];
        s.cells_n = [255; 4];
        graph().invalidate();
    }
    let o = old.filter(|_| !fresh);
    let d = |a: bool| a || o.is_none();
    let ol = o.unwrap_or(*m);
    if d(ol.status != m.status) {
        status_badge(m);
    }
    if d(ol.tb != m.tb) {
        tb_badge(m);
    }
    if d(ol.roll != m.roll || ol.record != m.record || ol.status != m.status || ol.gen_hz != m.gen_hz) {
        record_bar(m);
    }
    let pal = ol.colour != m.colour;
    if d(pal || ol.vdiv != m.vdiv || ol.probe != m.probe || ol.ac != m.ac) {
        vdiv_badge(m);
    }
    if d(pal || ol.trig_v != m.trig_v || ol.rising != m.rising) {
        trig_badge(m);
    }
    if d(ol.battery != m.battery || ol.charging != m.charging) {
        battery(m);
    }
    for k in 0..7 {
        let was = |mm: &Model| mm.pressed as usize == k + 1;
        let dep = match k {
            0 | 2 => pal,
            1 => pal || ol.vdiv != m.vdiv || ol.ch1_open != m.ch1_open,
            4 => ol.vdiv != m.vdiv || ol.math_open != m.math_open,
            _ => false,
        };
        if d(dep || was(&ol) != was(m)) {
            panel_button(m, k);
        }
    }
    if d(pal) {
        s.cells_n = [255; 4];
    }
    // Strip markers: repaint the rows of every marker that changed.
    let mut nm = [Mark::NONE; 5];
    let n = strip_marks(m, &mut nm);
    if nm != s.strip {
        let lo = m.top();
        let hi = lo + m.frame_h() - 1;
        let mut sp = Spans { s: [(0, 0); 12], n: 0 };
        for k in 0..5 {
            if nm[k] != s.strip[k] {
                for mk in [nm[k], s.strip[k]] {
                    if mk.some() {
                        sp.add((mk.y, mk.y + 11));
                    }
                }
            }
        }
        for &(a, b) in sp.merged(lo, hi) {
            strip_rows(&nm[..n], a, b);
        }
        s.strip = nm;
    }
    s.m = Some(*m);
    graph().flush(m);
}

/// Measurement bar texts (<= 4, "--" for unused cells); only changed cells
/// are repainted.
pub fn cells(m: &Model, texts: &[&str]) {
    let s = shown();
    for i in 0..4 {
        let t = texts.get(i).copied().unwrap_or("--");
        let b = t.as_bytes();
        let n = b.len().min(16);
        if s.cells_n[i] as usize == n && s.cells[i][..n] == b[..n] {
            continue;
        }
        meas_cell(m, i, t);
        s.cells[i][..n].copy_from_slice(&b[..n]);
        s.cells_n[i] = n as u8;
    }
}
