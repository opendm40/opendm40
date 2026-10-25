//! The stock DM40 V2.6 scope page, drawn from the spec stock/re/docs/stock-scope-ui.md
//! (sources: stock/re/stock-ui-notes/ layout.md, graph.md, popups.md). Every number
//! here is a spec value; the spec says where it comes from. Scenes at the end,
//! including our combined-view mockup in the stock's visual language.
#![allow(dead_code)]

use crate::gfx::{self, rect, Aa, Color, Font, Rect};

// ---- colours (RGB565, code) ----
pub const C_BAR: Color = 0x4208; // top bar, strip, panel, measurement bar
pub const C_FILL: Color = 0x2124; // graph fill, record-bar inner, dark text on yellow
pub const C_FRAME: Color = 0xad55; // frame border, rings, cursor lines, math off
pub const C_PURPLE: Color = 0xa81f; // status badge, toasts, math record default
pub const C_ORANGE: Color = 0xfaa0; // STOP badge, TRIG button, Clear
pub const C_MAGENTA: Color = 0xb833; // timebase badge
pub const C_WHITE: Color = 0xffff;
pub const C_GREEN: Color = 0x07e0; // battery bar while charging
pub const C_DIS: Color = 0x7bef; // label button while its popup is open, pressed
pub const C_POP_BG: Color = 0xd679; // popup page / selected tab
pub const C_POP_TAB: Color = 0x5acb; // unselected tab / header
pub const C_SEL: Color = 0x059e; // list selection, spinner band, slider fill
pub const PALETTE: [Color; 9] = [0xf800, 0xfd20, 0xffe0, 0x0400, 0x001f, 0x8010, 0xa145, 0xfe19, 0xfea0];

// ---- fonts (stock font-object addresses) ----
pub const F_C: Font = Font(gfx::FONT_C); // 10x16 status letters, "H"
pub const F12: Font = Font(0x0804_F24C); // 12 px B+ (ASCII from 0x0805471c)
pub const F16: Font = Font(gfx::FONT16); // 16 px wrapper (popups)
pub const F20: Font = Font(0x0805_4148); // 20 px ASCII (panel buttons)
pub const F22: Font = Font(0x0806_B13C); // 22 px (tab labels, toasts)
pub const F_MARK: Font = Font(0x0806_5558); // 12 px marker glyphs
pub const F_EDGE: Font = Font(0x0805_5014); // trigger edge 0x20 / 0x21
pub const F_PROBE: Font = Font(0x0805_4FC0); // coupling/probe glyphs 0x2c..0x45
pub const F_BATT: Font = Font(0x0806_BD44); // battery 0x22..0x27

// ---- geometry (code) ----
pub const FRAME: Rect = rect(13, 20, 401, 281);
pub const GRAPH: Rect = rect(14, 21, 399, 279);
pub const TOP_BAR: Rect = rect(0, 0, 480, 20);
pub const STRIP: Rect = rect(0, 0, 13, 320);
pub const PANEL: Rect = rect(414, 20, 66, 300);
pub const MEAS_BAR: Rect = rect(0, 301, 414, 19);

/// Screen regions for the layout check and diff.py (regions.json).
pub const REGIONS: [(&str, Rect); 10] = [
    ("top_bar", TOP_BAR),
    ("marker_strip", rect(0, 20, 13, 281)),
    ("graph", FRAME),
    ("right_panel", PANEL),
    ("meas_bar", MEAS_BAR),
    ("record_bar", rect(145, 1, 137, 17)),
    ("vdiv_label", rect(286, 1, 62, 17)),
    ("trig_label", rect(352, 1, 62, 17)),
    ("status_label", rect(13, 1, 62, 17)),
    ("tb_label", rect(79, 1, 62, 17)),
];

pub const TB_LABELS: [&str; 30] = [
    "10ns", "20ns", "50ns", "100ns", "200ns", "500ns", "1us", "2us", "5us", "10us", "20us", "50us", "100us", "200us", "500us", "1ms", "2ms", "5ms", "10ms", "20ms", "50ms", "100ms", "200ms", "500ms", "1s", "2s", "5s", "10s", "20s", "50s",
];
pub const VDIV_LABELS: [&str; 22] = ["1m", "2m", "5m", "10m", "20m", "50m", "100m", "200m", "500m", "1", "2", "5", "10", "20", "50", "100", "200", "500", "1k", "2k", "5k", "10k"];
pub const VDIV: [f32; 22] = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0];
pub const PROBE: [f32; 13] = [0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
pub const PROBE_NAMES: [&str; 13] = ["0.1X", "0.2X", "0.5X", "1X", "2X", "5X", "10X", "20X", "50X", "100X", "200X", "500X", "1000X"];
pub const STATUS: [&str; 5] = ["WAIT", "AUTO", "T'D", "RUN", "STOP"];
pub const MATH_LABELS: [&str; 7] = ["--", "+", "-", "*", "/", "FFT", "ABS"];
pub const FFT_DB: [&str; 13] = ["10mdB", "20mdB", "50mdB", "100mdB", "200mdB", "500mdB", "1dB", "2dB", "5dB", "10dB", "20dB", "50dB", "100dB"];
pub const MEAS_NAMES: [&str; 20] = ["Freq", "+Duty", "-Duty", "+Wid", "-Wid", "+Pul", "-Pul", "X2-1", "Xf", "Cycl", "Vp-p", "Vrms", "Vmax", "Vmin", "Vtop", "Vbas", "Vamp", "Y2-1", "Vcrm", "Vavg"];
pub const COLOUR_NAMES: [&str; 9] = ["red", "orange", "yellow", "green", "blue", "purple", "brown", "pink", "gold"];
pub const WINDOWS: [&str; 5] = ["Rectangular", "Blackman", "Hanning", "Hanming", "Triangular"];

#[derive(Clone, Copy, PartialEq)]
pub enum Popup {
    None,
    /// MF settings popup, tab 0..3, pressed cell (Measure) for the feedback state.
    Settings(u8),
    /// CH1 popup, tab 0..2.
    Ch1(u8),
    /// Math popup, tab 0..3.
    Math(u8),
    Sampling,
    /// Centred toast of the given size.
    Toast(&'static str, i32, i32),
}

#[derive(Clone)]
pub struct Scene {
    pub status: usize,      // index into STATUS (4 = STOP)
    pub tb: usize,          // timebase index 0..29
    pub vdiv: i32,          // V/div index -3..18
    pub probe: i32,         // probe index -3..9
    pub ac: bool,
    pub unit: usize,        // 0 V 1 A 2 W
    pub trig_row: i32,      // U+0x20 (default 105), -10..290
    pub trig_rising: bool,
    pub trig_mode: u8,      // 0 auto 1 normal 2 single
    pub trig_px: i32,       // x_t, trigger column 0..399 (default 200)
    pub pos: i32,           // CH1 position rows (default 0), +-392
    pub colour: usize,      // palette index (default 2)
    pub brightness: f32,    // S+0xa8 (1.0)
    pub grid: f32,          // S+0xc4 (0.5)
    pub sel_v: u8,          // selected level item 0 CH1 1 trig 2 math 3 Y1 4 Y2
    pub sel_h: u8,          // selected time item 0 trig pos 1 FFT 2 X1 3 X2
    pub guide: bool,        // guide lines of the selection shown (press .. +2 s)
    pub xcur: Option<(i32, i32)>, // X cursors (cols 1..399) when X2-1 / Xf selected
    pub ycur: Option<(i32, i32)>, // Y cursors (rows 1..279) when Y2-1 selected
    pub math: usize,        // S+0xd4 0 off .. 5 FFT 6 ABS
    pub math_on: bool,      // S+0x10b
    pub math_colour: usize, // S+0x108 (default 1 orange)
    pub math_off: i32,      // math/FFT offset rows
    pub fft_db: usize,      // index into FFT_DB (text2 of the math button)
    pub fft_hz: &'static str, // FFT centre label
    pub meas: Vec<&'static str>, // cell texts (<= 4), "--" when fewer
    pub meas_sel: Vec<usize>,    // selected measurement ids (popup G2)
    pub battery: u8,        // 0..5
    pub charging: bool,
    pub popup: Popup,
    pub trace: Option<Trace>,
    pub math_trace: Option<Trace>,
    pub persist: Option<Trace>, // previous frame shown as the persistence layer
    pub roll: bool,         // roll mode (record bar shows "ROLL")
    pub record: (f32, f32), // record bar start/end (0..1)
    pub pressed: Option<u8>, // 6 = TRIG pressed (the only button with feedback)
    pub ch1_open: bool,     // CH1 label grey (popup open)
    pub math_open: bool,
    pub roll_cols: Option<usize>, // roll: columns filled so far
    pub pressed_cell: Option<usize>, // Measure grid cell under the finger
    pub combined: Option<Meter>, // our combined-view mockup (stock style)
}

/// Our DMM meter strip in the stock visual language (combined view mockup).
#[derive(Clone)]
pub struct Meter {
    pub func: &'static str,  // "DC V"
    pub value: &'static str, // "-4.8176"
    pub unit: &'static str,  // "V"
    pub sub1: &'static str,  // "AC 53.63mV"
    pub sub2: &'static str,
    pub range: &'static str, // "AUTO"
    pub hold: bool,
    pub rel: bool,
    pub gen: &'static str, // "G 1.00kHz" or ""
}

/// A synthetic trace: per column (min row, max row) of the 280-row model.
#[derive(Clone)]
pub struct Trace {
    pub a: Vec<i32>,
    pub b: Vec<i32>,
}

impl Scene {
    pub fn default() -> Scene {
        Scene {
            status: 2,
            tb: 13,
            vdiv: 6,
            probe: 0,
            ac: false,
            unit: 0,
            trig_row: 105,
            trig_rising: true,
            trig_mode: 0,
            trig_px: 200,
            pos: 0,
            colour: 2,
            brightness: 1.0,
            grid: 0.5,
            sel_v: 0,
            sel_h: 0,
            guide: false,
            xcur: None,
            ycur: None,
            math: 0,
            math_on: false,
            math_colour: 1,
            math_off: 0,
            fft_db: 10,
            fft_hz: "1.25kHz",
            meas: vec!["Freq:1.00kHz", "Vp-p:3.00V", "Cycl:1.00ms", "Vrms:1.06V"],
            meas_sel: vec![0, 10, 9, 11],
            battery: 4,
            charging: false,
            popup: Popup::None,
            trace: Some(Trace::sine(1.5, 2.0, 0.0)),
            math_trace: None,
            persist: None,
            roll: false,
            record: (0.0, 1.0),
            pressed: None,
            ch1_open: false,
            math_open: false,
            roll_cols: None,
            pressed_cell: None,
            combined: None,
        }
    }
    pub fn pal(&self) -> Color {
        PALETTE[self.colour]
    }
    pub fn trace_colour(&self) -> Color {
        gfx::blend565(self.pal(), C_FILL, ((self.brightness * 255.0) as i32).clamp(0, 255) as u8)
    }
    pub fn half_colour(&self) -> Color {
        gfx::blend565(self.pal(), C_FILL, (((self.brightness * 255.0) as i32).clamp(0, 255) >> 1) as u8)
    }
    pub fn grid_colour(&self) -> Color {
        gfx::blend565(0xffff, C_FILL, ((self.grid * 255.0) as i32).clamp(0, 255) as u8)
    }
    pub fn math_colour(&self) -> Color {
        if self.math_on { PALETTE[self.math_colour] } else { C_FRAME }
    }
    pub fn vdiv_volts(&self) -> f32 {
        VDIV[(self.vdiv + 3) as usize] * PROBE[(self.probe + 3) as usize]
    }
    pub fn vdiv_text(&self) -> String {
        format!("{}{}", VDIV_LABELS[(self.vdiv + 3) as usize], ["V", "A", "W"][self.unit])
    }
    /// Coupling/probe glyph of font 0x08054fc0: DC codes 0x2d + 2*(probe+3),
    /// AC codes 0x2c + 2*(probe+3).
    pub fn probe_glyph(&self) -> u32 {
        (if self.ac { 0x2c } else { 0x2d }) + 2 * (self.probe + 3) as u32
    }
    /// Volts of a screen row: v = (140 + pos - row) * 4 V/div / 141.
    pub fn volts_of_row(&self, row: i32) -> f32 {
        (140 + self.pos - row) as f32 * 4.0 * self.vdiv_volts() / 141.0
    }
    /// Graph geometry: the full page or the combined mockup (shorter graph).
    fn graph_top(&self) -> i32 {
        if self.combined.is_some() { 76 } else { 20 }
    }
}

impl Trace {
    pub fn sine(amp: f32, periods: f32, phase: f32) -> Trace {
        let mut a = vec![];
        let mut b = vec![];
        for i in 0..400 {
            let t = (i as f32 / 400.0 + phase) * periods * core::f32::consts::TAU;
            let t1 = ((i + 1) as f32 / 400.0 + phase) * periods * core::f32::consts::TAU;
            let r0 = 140.0 - amp * 35.0 * t.sin();
            let r1 = 140.0 - amp * 35.0 * t1.sin();
            a.push(r0.min(r1).round() as i32);
            b.push(r0.max(r1).round() as i32);
        }
        Trace { a, b }
    }
    pub fn square(amp: f32, periods: f32, duty: f32) -> Trace {
        let mut a = vec![];
        let mut b = vec![];
        for i in 0..400 {
            let ph = (i as f32 / 400.0 * periods).fract();
            let r = if ph < duty { 140 - (amp * 35.0) as i32 } else { 140 + (amp * 35.0) as i32 };
            a.push(r);
            b.push(r);
        }
        Trace { a, b }
    }
    pub fn noise(centre: i32, amp: i32, seed: u32) -> Trace {
        let mut s = seed;
        let mut a = vec![];
        let mut b = vec![];
        for _ in 0..400 {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let r = centre + ((s >> 16) as i32 % (2 * amp + 1)) - amp;
            a.push(r);
            b.push(r);
        }
        Trace { a, b }
    }
    pub fn shifted(&self, rows: i32) -> Trace {
        Trace { a: self.a.iter().map(|r| r + rows).collect(), b: self.b.iter().map(|r| r + rows).collect() }
    }
    /// Engine widening (FUN_080310f8): a[i] = min(rowMax_i, b[i-1]),
    /// b[i] = max(rowMin_i, a[i-1]); then FUN_0802dc28 order + clamp 0..278.
    pub fn columns(&self, n: usize) -> Vec<(i32, i32)> {
        let mut out = vec![];
        let mut pa = 0;
        let mut pb = 0;
        for i in 0..n.min(400) {
            let (mut a, mut b) = (self.a[i].min(self.b[i]), self.a[i].max(self.b[i]));
            if i > 0 {
                a = a.min(pb);
                b = b.max(pa);
            }
            pa = a;
            pb = b;
            let (lo, hi) = (a.min(b).clamp(0, 278), a.max(b).clamp(0, 278));
            out.push((lo, hi));
        }
        out
    }
}

// ------------------------------------------------------------------ helpers

fn aa() -> Aa {
    Aa::Off
}

fn text(f: Font, r: Rect, s: &str, fg: Color, align: u8) {
    gfx::text(f, r, s, fg, align, 0, aa());
}

fn text_sp(f: Font, r: Rect, s: &str, fg: Color, align: u8, sp: i32) {
    gfx::text(f, r, s, fg, align, sp, aa());
}

/// A 12x12 marker: mask glyph in the parent's background, then the icon.
fn marker(x: i32, y: i32, icon: u32, mask: u32, col: Color, bg: Color) {
    let r = rect(x, y, 12, 12);
    gfx::icon(F_MARK, r, mask, bg, 8, aa());
    gfx::icon(F_MARK, r, icon, col, 8, aa());
}

// ------------------------------------------------------------------ page

pub fn draw(sc: &Scene) {
    // Five boxes tile the screen (the root draws nothing): draw order of the
    // page: panel, top bar, measurement bar, strip, frame (+ graph, T marker).
    gfx::fill(PANEL, C_BAR);
    gfx::fill(TOP_BAR, C_BAR);
    gfx::fill(MEAS_BAR, C_BAR);
    gfx::fill(STRIP, C_BAR);
    if let Some(m) = &sc.combined {
        draw_meter_strip(sc, m);
    }
    draw_frame(sc);
    draw_graph(sc);
    draw_graph_markers(sc);
    draw_strip_markers(sc);
    draw_top_bar(sc);
    draw_panel(sc);
    draw_meas_bar(sc);
    draw_popup(sc);
}

/// Grid frame FUN_0802e73e (frame-relative rule, sec. 4 of the spec).
pub fn draw_frame(sc: &Scene) {
    let top = sc.graph_top();
    let h = 301 - top; // 281 on the stock page
    let f = rect(13, top, 401, h);
    gfx::fill(f, C_FILL);
    gfx::frame(f, C_FRAME);
    let g = sc.grid_colour();
    let (w, hh) = (401, h);
    let (cx, cy) = (200, (hh - 1) / 2); // 200, 140 on the stock page
    let div_h = (hh - 1) / 8; // 35
    for y in 1..hh - 1 {
        for x in 1..w - 1 {
            let on = x == cx
                || y == cy
                || (x % 40 == 0 && y % 7 == 0)
                || (y % div_h == 0 && x % 8 == 0)
                || (x % 8 == 0 && (cy - 2..=cy + 2).contains(&y))
                || (y % 7 == 0 && (cx - 2..=cx + 2).contains(&x));
            if on {
                gfx::put(13 + x, top + y, g);
            }
        }
    }
}

/// Graph FUN_0802d874: overlay lines, persistence layers, trace bars.
pub fn draw_graph(sc: &Scene) {
    let top = sc.graph_top();
    let gh = 299 - top; // 279
    let g = rect(14, top + 1, 399, gh);
    gfx::set_clip(g);
    let ctr = sc.trace_colour();
    // overlay / guide lines (drawn first, under the trace)
    if sc.guide {
        match sc.sel_h {
            0 => gfx::dots_v(13 + sc.trig_px, g.y, gh - 2, 2, ctr),
            1 => gfx::dots_v(g.x, g.y, gh - 2, 2, sc.math_colour()),
            _ => {}
        }
        match sc.sel_v {
            0 => gfx::fill(rect(g.x, top + 140 + sc.pos, 397, 1), ctr),
            1 => gfx::dots_h(g.x, top + sc.trig_row, 397, 2, ctr),
            2 => gfx::fill(rect(g.x, top + 140 + sc.math_off, 397, 1), sc.math_colour()),
            _ => {}
        }
    }
    if let Some((x1, x2)) = sc.xcur {
        gfx::dots_v(14 + x1, g.y, gh - 2, 2, C_FRAME);
        gfx::dots_v(14 + x2, g.y, gh - 2, 2, C_FRAME);
    }
    if let Some((y1, y2)) = sc.ycur {
        gfx::dots_h(g.x, g.y + y1, 397, 2, C_FRAME);
        gfx::dots_h(g.x, g.y + y2, 397, 2, C_FRAME);
    }
    let n = sc.roll_cols.unwrap_or(399).min(399);
    if let Some(p) = &sc.persist {
        let half = sc.half_colour();
        for (i, (a, b)) in p.columns(399).iter().enumerate() {
            gfx::fill(rect(14 + i as i32, g.y + a, 1, b - a + 1), half);
        }
    }
    if let Some(t) = &sc.trace {
        for (i, (a, b)) in t.columns(n).iter().enumerate() {
            gfx::fill(rect(14 + i as i32, g.y + a, 1, b - a + 1), ctr);
        }
    }
    if let Some(t) = &sc.math_trace {
        let cm = PALETTE[sc.math_colour];
        for (i, (a, b)) in t.columns(399).iter().enumerate() {
            gfx::fill(rect(14 + i as i32, g.y + a, 1, b - a + 1), cm);
        }
    }
    gfx::clip_screen();
}

/// Markers that are children of the graph: "T", X cursors, FFT centre + label.
pub fn draw_graph_markers(sc: &Scene) {
    let top = sc.graph_top();
    let y = top + 1;
    // top-of-graph marker placement (FUN_08028bec): (x, icon) for a column
    let place = |col: i32, sel: bool, down: (u32, u32), left: (u32, u32), right: (u32, u32)| -> (i32, u32, u32) {
        let pick = |p: (u32, u32)| if sel { p.1 } else { p.0 };
        if col < 12 {
            (col.max(0), pick(left), 0x49)
        } else if col + 12 <= 399 {
            (col - 6, pick(down), 0x48)
        } else {
            (if col > 399 { 387 } else { col - 13 }, pick(right), 0x4a)
        }
    };
    let (x, icon, mask) = place(sc.trig_px, sc.sel_h == 0, (0x40, 0x44), (0x41, 0x45), (0x42, 0x46));
    marker(14 + x, y, icon, mask, sc.pal(), C_FILL);
    if let Some((c1, c2)) = sc.xcur {
        let (x, icon, mask) = place(c1, sc.sel_h == 2, (0x20, 0x24), (0x21, 0x25), (0x22, 0x26));
        marker(14 + x, y, icon, mask, C_WHITE, C_FILL);
        let (x, icon, mask) = place(c2, sc.sel_h == 3, (0x28, 0x2c), (0x29, 0x2d), (0x2a, 0x2e));
        marker(14 + x, y, icon, mask, C_WHITE, C_FILL);
    }
    if sc.math_on && sc.math == 5 {
        let cm = PALETTE[sc.math_colour];
        marker(14 + 194, y, if sc.sel_h == 1 { 0x3c } else { 0x38 }, 0x48, cm, C_FILL);
        let r = rect(214, top + 13, 60, 17);
        gfx::rborder(r, 5, 0x0e, cm);
        text(F12, r, sc.fft_hz, cm, 8);
    }
}

/// Strip markers (x = 1): CH1 zero, trigger level, math offset, Y cursors.
pub fn draw_strip_markers(sc: &Scene) {
    let top = sc.graph_top();
    let bottom = top + 280; // 300 on the stock page
    let pal = sc.pal();
    // level item at glyph top y with right / up / down variants
    let level = |y_in: i32, in_range: bool, above: bool, sel: bool, fam: (u32, u32, u32), col: Color| {
        let (right, up, down) = fam; // outline codes; filled = +4
        let f = |c: u32| if sel { c + 4 } else { c };
        if in_range {
            marker(1, y_in, f(right), 0x4a, col, C_BAR);
        } else if above {
            marker(1, top, f(up), 0x4b, col, C_BAR);
        } else {
            marker(1, bottom - 12, f(down), 0x48, col, C_BAR);
        }
    };
    // Order: creation order, the selected one last (on top).
    let ch1 = |sel: bool| level(sc.pos + top + 134, sc.pos.abs() <= 140, sc.pos < -140, sel, (0x32, 0x33, 0x30), pal);
    let trig = |sel: bool| level(sc.trig_row + top - 6, (0..=280).contains(&sc.trig_row), sc.trig_row < 0, sel, (0x42, 0x43, 0x40), pal);
    let math = |sel: bool| level(sc.math_off + top + 134, sc.math_off.abs() <= 140, sc.math_off < -140, sel, (0x3a, 0x3b, 0x38), PALETTE[sc.math_colour]);
    let y1 = |sel: bool, row: i32| level(row + top - 6, (0..=280).contains(&row), row < 0, sel, (0x22, 0x23, 0x20), C_WHITE);
    let y2 = |sel: bool, row: i32| level(row + top - 6, (0..=280).contains(&row), row < 0, sel, (0x2a, 0x2b, 0x28), C_WHITE);
    let order: [u8; 5] = [0, 1, 2, 3, 4];
    for &k in order.iter().filter(|&&k| k != sc.sel_v).chain(core::iter::once(&sc.sel_v)) {
        match k {
            0 => ch1(sc.sel_v == 0),
            1 => trig(sc.sel_v == 1),
            2 if sc.math_on => math(sc.sel_v == 2),
            3 => {
                if let Some((r, _)) = sc.ycur {
                    y1(sc.sel_v == 3, r)
                }
            }
            4 => {
                if let Some((_, r)) = sc.ycur {
                    y2(sc.sel_v == 4, r)
                }
            }
            _ => {}
        }
    }
}

/// Trigger level text: FUN_08032bfa formatter of the level in volts.
pub fn fmt_v(v: f32, unit: &str) -> String {
    let units = ["u", "m", "", "k", "M"];
    if !v.is_finite() {
        return " -- ".into();
    }
    if v.abs() < 1e-6 {
        return format!("{:.2}u{}", v * 1e6, unit);
    }
    let mut x = v;
    let mut i = 2usize;
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
        return " --".into();
    }
    let s = if x.abs() < 9.995 {
        format!("{:.2}", x)
    } else if x.abs() < 99.95 {
        format!("{:.1}", x)
    } else {
        format!("{:.0}", x)
    };
    format!("{s}{}{unit}", units[i])
}

/// Top bar FUN_0802d1c6: five badges and the battery (y 1, h 17).
pub fn draw_top_bar(sc: &Scene) {
    let pal = sc.pal();
    // 3.1 status badge
    let r = rect(13, 1, 62, 17);
    gfx::rbox(r, 5, 0x0f, if sc.status == 4 { C_ORANGE } else { C_PURPLE });
    text(F_C, r, STATUS[sc.status], C_WHITE, 8);
    // 3.2 timebase badge
    let r = rect(79, 1, 62, 17);
    gfx::rbox(r, 5, 0x0f, C_MAGENTA);
    text(F_C, rect(83, 1, 12, 17), "H", C_WHITE, 1);
    text(F12, rect(95, 1, 46, 17), TB_LABELS[sc.tb], C_WHITE, 8);
    // 3.3 record-position bar
    let r = rect(145, 1, 137, 17);
    gfx::rbox(r, 5, 0x0f, C_FRAME);
    gfx::rbox(rect(146, 2, 135, 15), 5, 0x0f, C_FILL);
    if sc.roll {
        text(F12, rect(146, 2, 135, 15), "ROLL", C_FRAME, 8);
    } else {
        let (start, end) = sc.record;
        let x0 = if start < 0.0 { 4 } else { (start * 129.0 + 4.0) as i32 };
        let len = (((end.min(1.0) - start.max(0.0)) * 129.0) as i32).max(0);
        gfx::fill(rect(145 + x0, 9, len, 1), 0xffe0);
        if (0.0..1.0).contains(&start) {
            text(F12, rect(145 + x0 - 2, 2, 4, 17), "[", C_FRAME, 1);
        }
        if end <= 1.0 {
            text(F12, rect(145 + (end * 129.0 + 4.0) as i32 - 4, 2, 4, 17), "]", C_FRAME, 1);
        }
    }
    // 3.4 CH1 V/div badge
    let r = rect(286, 1, 62, 17);
    gfx::rbox(r, 5, 0x0f, pal);
    gfx::icon(F_PROBE, rect(286, 1, 18, 17), sc.probe_glyph(), C_FILL, 4, aa());
    text(F12, rect(304, 1, 44, 17), &sc.vdiv_text(), C_FILL, 8);
    // 3.5 trigger badge
    let r = rect(352, 1, 62, 17);
    gfx::rbox(r, 5, 0x0f, C_FRAME);
    gfx::rbox(rect(353, 2, 60, 15), 5, 0x0f, C_BAR);
    gfx::icon(F_EDGE, rect(352, 1, 12, 17), if sc.trig_rising { 0x20 } else { 0x21 }, pal, 4, aa());
    let lvl = fmt_v(sc.volts_of_row(sc.trig_row), ["V", "A", "W"][sc.unit]);
    text(F12, rect(364, 1, 50, 17), &lvl, pal, 8);
    // 3.6 battery
    gfx::fill(rect(435, 1, 40, 16), C_BAR);
    gfx::icon(F_BATT, rect(435, 1, 40, 16), 0x22, C_WHITE, 0, aa());
    if sc.battery > 0 {
        gfx::set_clip(rect(437, 3, 33, 12));
        gfx::icon(F_BATT, rect(435, 1, 40, 16), 0x22 + sc.battery.min(5) as u32, if sc.charging { C_GREEN } else { C_WHITE }, 0, aa());
        gfx::clip_screen();
    }
}

/// Right panel FUN_0802c5c8: the CH1 pill, the math pill, TRIG.
pub fn draw_panel(sc: &Scene) {
    let pal = sc.pal();
    let mc = sc.math_colour();
    // B1 "+" (top corners), B2 label (plain), B3 "-" (bottom corners)
    gfx::rbox(rect(417, 20, 60, 40), 8, 0x3, pal);
    text(F20, rect(417, 20, 60, 40), "+", C_FILL, 8);
    gfx::fill(rect(417, 61, 60, 40), if sc.ch1_open { C_DIS } else { pal });
    text(F20, rect(417, 61, 60, 24), "CHA", C_FILL, 8);
    text(F12, rect(417, 85, 60, 16), &sc.vdiv_text(), C_FILL, 8);
    gfx::rbox(rect(417, 102, 60, 40), 8, 0xc, pal);
    text(F20, rect(417, 102, 60, 40), "-", C_FILL, 8);
    // B4..B6 math
    gfx::rbox(rect(417, 150, 60, 40), 8, 0x3, mc);
    text(F20, rect(417, 150, 60, 40), "+", C_WHITE, 8);
    gfx::fill(rect(417, 191, 60, 40), if sc.math_open { C_DIS } else { mc });
    text(F20, rect(417, 191, 60, 24), MATH_LABELS[sc.math], C_WHITE, 8);
    let t2 = if sc.math == 5 { FFT_DB[sc.fft_db].to_string() } else { sc.vdiv_text() };
    text(F12, rect(417, 215, 60, 16), &t2, C_WHITE, 8);
    gfx::rbox(rect(417, 232, 60, 40), 8, 0xc, mc);
    text(F20, rect(417, 232, 60, 40), "-", C_WHITE, 8);
    // B7 TRIG (press: style[1] = same style, no visible change)
    gfx::rbox(rect(417, 280, 60, 40), 8, 0x0f, C_ORANGE);
    text(F20, rect(417, 280, 60, 40), "TRIG", C_WHITE, 8);
}

/// Measurement bar: 4 cells (13 + 100 i, 302, 98, 17).
pub fn draw_meas_bar(sc: &Scene) {
    for i in 0..4usize {
        let r = rect(13 + 100 * i as i32, 302, 98, 17);
        gfx::rbox(r, 5, 0x0f, C_BAR);
        gfx::rborder(r, 5, 0x0f, C_FRAME);
        let t = sc.meas.get(i).copied().unwrap_or("--");
        text(F12, r, t, sc.pal(), 8);
    }
}

// ------------------------------------------------------------------ popups

/// Option list FUN_08026cc8: frame fill, items 1 px inset, last item fills.
fn option_list(r: Rect, items: &[&str], item_w: i32, sel: usize) {
    gfx::fill(r, C_BAR);
    for (i, t) in items.iter().enumerate() {
        let x = 1 + i as i32 * item_w;
        let w = if i + 1 == items.len() { r.w - x - 1 } else { item_w };
        let ir = rect(r.x + x, r.y + 1, w, r.h - 2);
        let (bg, fg) = if i == sel { (C_SEL, C_WHITE) } else { (C_POP_BG, C_BAR) };
        gfx::fill(ir, bg);
        text(F16, ir, t, fg, 8);
    }
}

/// Spinner FUN_080274b2: 5 rows of 30 px, the current item in the band.
fn spinner(r: Rect, items: &[&str], cur: usize, colours: Option<&[Color]>) {
    gfx::rbox(r, 8, 0x0f, C_BAR);
    let rows = (r.h / 30) as i32;
    let centre = -(rows / 2);
    let band = rect(r.x, r.y + (r.h - 30) / 2, r.w, 30);
    gfx::fill(band, C_SEL);
    for k in 0..rows {
        let idx = cur as i32 + centre + k;
        if idx < 0 || idx >= items.len() as i32 {
            continue;
        }
        let y = r.y + k * 30;
        let in_band = y == band.y;
        let col = match colours {
            Some(c) => c[idx as usize],
            None => if in_band { C_WHITE } else { C_POP_BG },
        };
        let tr = match colours {
            Some(_) => rect(r.x + (r.w - 56) / 2, y + (30 - 13) / 2, 56, 13),
            None => rect(r.x, y, r.w, 30),
        };
        text(F16, tr, items[idx as usize], col, 8);
    }
}

/// Slider FUN_0802639a (brightness / grid intensity).
fn slider(r: Rect, pos: i32) {
    let track = rect(r.x, r.y + (r.h - 10) / 2, r.w, 10);
    gfx::rbox(track, 5, 0x0f, C_FILL);
    let inner = track.inset(1);
    let split = r.x + pos + 5;
    gfx::set_clip(rect(inner.x, inner.y, (split - inner.x).max(0), inner.h));
    gfx::rbox(inner, 5, 0x0f, C_SEL);
    gfx::set_clip(rect(split, inner.y, inner.x + inner.w - split, inner.h));
    gfx::rbox(inner, 5, 0x0f, C_POP_BG);
    gfx::clip_screen();
    gfx::fill(rect(r.x + pos, r.y + (r.h - 24) / 2, 10, 24), C_FILL);
    gfx::fill(rect(r.x + pos + 1, r.y + (r.h - 24) / 2 + 1, 8, 22), C_WHITE);
}

fn label_button(r: Rect, t: &str, bg: Color) {
    gfx::rbox(r, 8, 0x0f, bg);
    text(F16, r, t, C_WHITE, 8);
}

pub fn draw_settings(sc: &Scene, tab: u8) {
    let page_h = [150, 100, 55, 55][tab as usize];
    let hh = 40 + page_h;
    gfx::fill(rect(13, 20, 401, hh), C_FRAME);
    // tab bar (14,21,399,hh) clipped to the box
    gfx::set_clip(rect(14, 21, 399, hh - 1));
    for i in 0..4 {
        let x = 14 + 101 * i;
        let r = rect(x, 21, 100, 40);
        let name = ["Measure", "Display", "Trigger", "Settings"][i as usize];
        if i == tab as i32 {
            gfx::fill(r, C_POP_BG);
            gfx::fill(rect(x, 59, 100, 2), C_POP_TAB);
            text(F22, r, name, C_FILL, 8);
        } else {
            gfx::fill(r, C_POP_TAB);
            text(F22, r, name, C_WHITE, 8);
        }
    }
    gfx::set_clip(rect(14, 61, 399, page_h - 1));
    match tab {
        0 => {
            gfx::fill(rect(15, 61, 398, 110), C_POP_BG);
            for i in 0..20 {
                let (c, rr) = (i % 7, i / 7);
                let r = rect(16 + 57 * c, 66 + 35 * rr, 55, 30);
                let pressed = sc.pressed_cell == Some(i as usize);
                gfx::rbox(r, 8, 0x0f, if pressed { C_DIS } else { C_BAR });
                text(F16, r, MEAS_NAMES[i as usize], C_WHITE, 8);
            }
            gfx::fill(rect(15, 171, 398, 40), C_POP_BG);
            for (k, id) in sc.meas_sel.iter().take(4).enumerate() {
                let r = rect(16 + 57 * k as i32, 176, 55, 30);
                gfx::rbox(r, 8, 0x0f, C_BAR);
                text(F16, r, MEAS_NAMES[*id], 0xffe0, 8);
            }
            label_button(rect(281, 176, 60, 30), "Clear", C_ORANGE);
            label_button(rect(348, 176, 60, 30), "Default", C_PURPLE);
        }
        1 => {
            gfx::fill(rect(14, 61, 399, 100), C_POP_BG);
            text(F16, rect(24, 61, 189, 20), "Roll:", C_FILL, 1);
            text(F16, rect(24, 105, 189, 20), "Persist:", C_FILL, 1);
            text(F16, rect(214, 61, 189, 20), "Brightness:", C_FILL, 1);
            text(F16, rect(214, 105, 189, 20), "Grid intensity:", C_FILL, 1);
            option_list(rect(24, 81, 88, 24), &["On", "Off"], 44, if sc.roll { 0 } else { 1 });
            option_list(rect(24, 125, 176, 24), &["OFF", "MIN", "1S", "INF"], 44, if sc.persist.is_some() { 1 } else { 0 });
            slider(rect(214, 81, 140, 24), ((130.0 * sc.brightness) as i32).clamp(0, 130));
            slider(rect(214, 125, 140, 24), ((130.0 * sc.grid) as i32).clamp(0, 130));
            text(F16, rect(364, 81, 49, 24), &format!("{:.0}%", sc.brightness * 100.0), C_FILL, 1);
            text(F16, rect(364, 125, 49, 24), &format!("{:.0}%", sc.grid * 100.0), C_FILL, 1);
        }
        2 => {
            gfx::fill(rect(14, 61, 399, 55), C_POP_BG);
            text(F16, rect(24, 61, 189, 20), "Edge:", C_FILL, 1);
            text(F16, rect(214, 61, 189, 20), "Mode:", C_FILL, 1);
            option_list(rect(24, 81, 120, 24), &["Rising", "Falling"], 60, if sc.trig_rising { 0 } else { 1 });
            option_list(rect(214, 81, 180, 24), &["Auto", "Normal", "Single"], 60, sc.trig_mode as usize);
        }
        _ => {
            gfx::fill(rect(14, 61, 399, 55), C_POP_BG);
            label_button(rect(24, 78, 80, 30), "PriScr", C_BAR);
            label_button(rect(114, 78, 80, 30), "Save CSV", C_BAR);
        }
    }
    gfx::clip_screen();
}

/// CH1 / Math popup frame: box, header, vertical tabs, page area.
fn side_popup(title: &str, tabs: &[&str], tab: u8) {
    gfx::fill(rect(214, 20, 200, 281), C_FRAME);
    gfx::fill(rect(215, 21, 198, 29), C_POP_TAB);
    text(F16, rect(215, 21, 198, 29), title, 0xffe0, 8);
    gfx::fill(rect(215, 50, 198, 250), C_FRAME);
    for (i, t) in tabs.iter().enumerate() {
        let y = 50 + 41 * i as i32;
        let r = rect(215, y, 78, 40);
        if i == tab as usize {
            gfx::fill(r, C_POP_BG);
            gfx::fill(rect(291, y, 2, 40), C_BAR);
            text_sp(F16, r, t, C_BAR, 8, 1);
        } else {
            gfx::fill(r, C_BAR);
            text_sp(F16, r, t, C_WHITE, 8, 1);
        }
    }
    gfx::fill(rect(293, 50, 120, 250), C_POP_BG);
}

pub fn draw_ch1(sc: &Scene, tab: u8) {
    side_popup("CHA", &["Coupling", "Probe", "Color"], tab);
    match tab {
        0 => option_list(rect(308, 71, 90, 24), &["DC", "AC"], 44, sc.ac as usize),
        1 => {
            text(F16, rect(293, 50, 120, 20), "Scale", C_BAR, 8);
            spinner(rect(298, 70, 110, 150), &PROBE_NAMES, (sc.probe + 3) as usize, None);
            text(F16, rect(293, 224, 120, 24), "Type", C_BAR, 8);
            option_list(rect(308, 250, 90, 24), &["Vol", "Cur"], 44, sc.unit.min(1));
        }
        _ => spinner(rect(298, 70, 110, 150), &COLOUR_NAMES, sc.colour, Some(&PALETTE)),
    }
}

pub fn draw_math(sc: &Scene, tab: u8) {
    side_popup("MATH", &["Mode", "Color", "Hz/Div", "Other"], tab);
    match tab {
        0 => option_list(rect(308, 71, 90, 24), &["FFT", "ABS"], 44, if sc.math == 5 { 0 } else { 1 }),
        1 => spinner(rect(298, 70, 110, 150), &COLOUR_NAMES, sc.math_colour, Some(&PALETTE)),
        2 => spinner(rect(298, 70, 110, 150), &["250Hz", "500Hz", "1.25kHz", "2.50kHz"], 2, None),
        _ => {
            text(F16, rect(293, 50, 120, 20), "Window", C_BAR, 8);
            spinner(rect(298, 70, 110, 150), &WINDOWS, 0, None);
            text(F16, rect(293, 224, 120, 24), "FFT Unit", C_BAR, 8);
            option_list(rect(308, 250, 90, 24), &["dB", "Vrms"], 44, 0);
        }
    }
}

pub fn draw_popup(sc: &Scene) {
    match sc.popup {
        Popup::None => {}
        Popup::Settings(t) => draw_settings(sc, t),
        Popup::Ch1(t) => draw_ch1(sc, t),
        Popup::Math(t) => draw_math(sc, t),
        Popup::Sampling => {
            let r = rect(190, 150, 100, 20);
            gfx::rbox(r, 8, 0x0f, C_PURPLE);
            text(F16, r, "Sampling", C_WHITE, 8);
        }
        Popup::Toast(t, w, h) => {
            let r = rect((480 - w) >> 1, (320 - h) >> 1, w, h);
            gfx::rbox(r, 8, 0x0f, C_PURPLE);
            let lines: Vec<&str> = t.split('\n').collect();
            let th = 22 * lines.len() as i32;
            for (i, l) in lines.iter().enumerate() {
                text(F22, rect(r.x, r.y + ((h - th) >> 1) + 22 * i as i32, w, 22), l, C_WHITE, 8);
            }
        }
    }
}

// ------------------------------------------------------------------ mockup

/// Our combined DMM+scope view in the stock's visual language: the stock page
/// with a 56 px meter strip (y 20..75) between the top bar and the graph; the
/// graph keeps the stock frame rules at 8 x 28 rows (225 px). Badges r5 and
/// the 12/16/20 px fonts as the stock; the reading in the 20 px digit font
/// 0x0805c5dc of the stock's generator page.
pub fn draw_meter_strip(sc: &Scene, m: &Meter) {
    let f20d: Font = Font(0x0805_C5DC);
    let s = rect(13, 20, 401, 56);
    gfx::fill(s, C_BAR);
    // tile like the trigger badge: grey ring around a dark box
    gfx::rbox(rect(13, 21, 401, 54), 5, 0x0f, C_FRAME);
    gfx::rbox(rect(14, 22, 399, 52), 5, 0x0f, C_FILL);
    // left column: function badge (purple), range / HOLD / REL badge below
    let fb = rect(19, 25, 62, 17);
    gfx::rbox(fb, 5, 0x0f, C_PURPLE);
    text(F16, fb, m.func, C_WHITE, 8);
    let b2 = rect(19, 46, 62, 17);
    let (bt, bc) = if m.hold { ("HOLD", C_ORANGE) } else if m.rel { ("REL", C_MAGENTA) } else { (m.range, C_POP_TAB) };
    gfx::rbox(b2, 5, 0x0f, bc);
    text(F12, b2, bt, C_WHITE, 8);
    // main reading: 20 px digits right-aligned at x 206, unit in 16 px
    text(f20d, rect(88, 24, 118, 30), m.value, 0xffe0, 4);
    text(F16, rect(210, 29, 40, 20), m.unit, 0xffe0, 1);
    // sub readings (AC part / Hz / duty) right-aligned at x 340, 12 px white
    text(F12, rect(250, 25, 90, 14), m.sub1, C_WHITE, 4);
    text(F12, rect(250, 43, 90, 14), m.sub2, C_WHITE, 4);
    // generator state: green badge "G" + frequency at the strip's right end
    if !m.gen.is_empty() {
        let gb = rect(348, 25, 60, 17);
        gfx::rbox(gb, 5, 0x0f, 0x0400);
        text(F12, gb, "GEN", C_WHITE, 8);
        text(F12, rect(348, 46, 60, 17), m.gen, 0x07e0, 8);
    }
}

// ------------------------------------------------------------------ scenes

pub fn scenes() -> Vec<(String, Scene)> {
    let mut v: Vec<(String, Scene)> = vec![];
    let d = Scene::default();
    let mut add = |n: &str, s: Scene| v.push((n.to_string(), s));
    add("st01-default-td", d.clone());
    let mut s = d.clone();
    s.status = 4;
    s.record = (0.1, 0.3);
    s.persist = None;
    add("st02-stop-zoomed", s);
    let mut s = d.clone();
    s.status = 1;
    s.trace = Some(Trace::square(2.0, 3.0, 0.3));
    add("st03-auto-square", s);
    let mut s = d.clone();
    s.status = 0;
    s.trig_mode = 1;
    s.trace = None;
    add("st04-wait-empty", s);
    let mut s = d.clone();
    s.tb = 23;
    s.status = 3;
    s.roll = true;
    s.roll_cols = Some(180);
    s.trace = Some(Trace::noise(150, 6, 7));
    add("st05-roll", s);
    let mut s = d.clone();
    s.xcur = Some((120, 280));
    s.meas = vec!["X2-1:800us", "Xf:1.25kHz"];
    s.meas_sel = vec![7, 8];
    s.sel_h = 2;
    add("st06-xcursors", s);
    let mut s = d.clone();
    s.ycur = Some((60, 220));
    s.meas = vec!["Y2-1:4.54V"];
    s.meas_sel = vec![17];
    s.sel_v = 3;
    add("st07-ycursors", s);
    let mut s = d.clone();
    s.sel_v = 1;
    s.trig_row = 60;
    s.pos = 40;
    s.guide = true;
    s.trace = Some(Trace::sine(1.5, 2.0, 0.0).shifted(40));
    add("st08-drag-trig-level", s);
    let mut s = d.clone();
    s.guide = true;
    s.trig_px = 120;
    s.pos = -30;
    s.trace = Some(Trace::sine(1.5, 2.0, 0.2).shifted(-30));
    add("st09-drag-pos-guides", s);
    let mut s = d.clone();
    s.pos = 200;
    s.trig_row = 295;
    s.trig_px = 5;
    s.trace = Some(Trace::sine(1.5, 2.0, 0.0).shifted(200));
    add("st10-offscreen-markers", s);
    let mut s = d.clone();
    s.math_on = true;
    s.math = 5;
    s.math_trace = Some(Trace::noise(230, 3, 3));
    s.math_colour = 1;
    add("st11-fft", s);
    let mut s = d.clone();
    s.persist = Some(Trace::sine(1.5, 2.0, 0.03));
    add("st12-persistence", s);
    let mut s = d.clone();
    s.ac = true;
    s.probe = 3;
    s.vdiv = 9;
    s.trig_rising = false;
    s.colour = 3;
    s.battery = 1;
    s.charging = true;
    add("st13-ac-x10-green-falling", s);
    let mut s = d.clone();
    s.popup = Popup::Toast("PriScr OK", 240, 40);
    add("st14-toast", s);
    let mut s = d.clone();
    s.popup = Popup::Sampling;
    add("st15-sampling", s);
    for t in 0..4 {
        let mut s = d.clone();
        s.popup = Popup::Settings(t);
        if t == 0 {
            s.pressed_cell = Some(11);
        }
        add(&format!("st2{t}-settings-{}", ["measure", "display", "trigger", "settings"][t as usize]), s);
    }
    for t in 0..3 {
        let mut s = d.clone();
        s.popup = Popup::Ch1(t);
        s.ch1_open = true;
        add(&format!("st3{t}-ch1-{}", ["coupling", "probe", "color"][t as usize]), s);
    }
    for t in 0..4 {
        let mut s = d.clone();
        s.popup = Popup::Math(t);
        s.math_open = true;
        s.math_on = true;
        s.math = 5;
        add(&format!("st4{t}-math-{}", ["mode", "color", "hzdiv", "other"][t as usize]), s);
    }
    let mut s = d.clone();
    s.combined = Some(Meter { func: "DC V", value: "-4.8176", unit: "V", sub1: "AC 53.63mV", sub2: "50.00Hz", range: "AUTO", hold: false, rel: false, gen: "1.00kHz" });
    s.trace = Some(Trace::sine(1.2, 3.0, 0.0));
    add("mk01-combined", s);
    let mut s = d.clone();
    s.combined = Some(Meter { func: "AC V", value: "230.12", unit: "V", sub1: "50.000Hz", sub2: "50.0%", range: "AUTO", hold: true, rel: false, gen: "" });
    s.trace = Some(Trace::sine(2.5, 2.0, 0.0));
    s.status = 4;
    add("mk02-combined-hold-stop", s);
    v
}
