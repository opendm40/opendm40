//! The Bode page (View::Bode; src/app_bode.rs, src/bode.rs;
//! firmware/docs/bode.md): the Inrush page's look (title bar 0x2966, BG
//! body, 0x2966 buttons), two charts over one log-frequency axis. Region C
//! (link-native.x): drawn only on that page, where no AD2 stream runs.
//!
//!   0..36     title bar "Bode", the state at the right ("Thru 12/38")
//!   38..60    the cursor: f, gain, phase; the -3 dB frequency at the right
//!   62..162   gain, dB (scale from the data, 10 / 20 dB steps)
//!   166..256  phase, -180 .. 180 degrees (90 per division)
//!   258..274  the decades 10 Hz .. 10 kHz
//!   280..316  [ Back ] [ Thru ] [ Run / Stop ] [ Save ] [ 10/dec ]

use super::*;
use crate::bode;

/// Chart columns (x) and the two chart boxes (rows).
pub const PX: i16 = 46;
pub const PW: i16 = 426;
const GY: i16 = 62;
const GH: i16 = 101;
const PY: i16 = 166;
const PH: i16 = 91;
const AXIS_Y: i16 = 259;
const STATE: sf::Rect = rect(150, 9, 322, 20);
const CURSOR: sf::Rect = rect(8, 38, 346, 22);
const M3DB: sf::Rect = rect(356, 40, 116, 18);
/// Bottom buttons.
pub const BACK_BTN: sf::Rect = rect(8, 280, 86, 36);
pub const THRU_BTN: sf::Rect = rect(98, 280, 86, 36);
pub const RUN_BTN: sf::Rect = rect(188, 280, 112, 36);
pub const SAVE_BTN: sf::Rect = rect(304, 280, 80, 36);
pub const PPD_BTN: sf::Rect = rect(388, 280, 84, 36);
/// The whole chart area (a tap there moves the cursor).
pub const CHART: sf::Rect = rect(PX, GY, PW, PY + PH - GY);
/// Gain / phase curves, the cursor line, the grid, the chart frames.
pub const C_GAIN: Color = 0xFC40;
pub const C_PHASE: Color = 0x07FF;
const C_CURSOR: Color = 0xFFE0;
const C_GRID: Color = 0x2945;
const C_ZERO: Color = 0x52AA;
pub const C_OK: Color = 0x07E0;
pub const C_WARN: Color = 0xFD20;
pub const C_RUN: Color = 0x04FC;
pub const C_PLAIN: Color = STATUS_FG;

/// What the page shows (texts formatted by the app).
pub struct Vals<'a> {
    pub state: &'a str,
    pub state_fg: Color,
    pub cursor: &'a str,
    pub m3db: &'a str,
    pub running: bool,
    pub ppd: &'a str,
    /// Thru stored for the current grid (the Thru button in the accent).
    pub thru: bool,
}

/// The curves: frequency (Hz), gain (dB), phase (degrees) per measured
/// point, ascending f; the cursor's point.
pub struct Curves<'a> {
    pub f: &'a [f32],
    pub g: &'a [f32],
    pub p: &'a [f32],
    pub cursor: Option<usize>,
}

/// The whole page (tools/ui-render; the app draws `frame` + `parts`).
#[allow(dead_code)]
pub fn page(v: &Vals, c: &Curves) {
    frame();
    parts(v, c);
}

/// The title bar, the background and the frequency axis.
pub fn frame() {
    lcd::fill_rect(0, 0, lcd::W, 36, BTN);
    lcd::fill_rect(0, 36, lcd::W, lcd::H - 36, BG);
    sf::glyphs(sf::TEXT22, rect(8, 0, 140, 36), "Bode", STATUS_FG, BTN, 1, 0);
    axis();
}

/// What changes: the state, the readout, the charts, the buttons.
pub fn parts(v: &Vals, c: &Curves) {
    state(v.state, v.state_fg);
    readout(v.cursor, v.m3db);
    charts(c);
    buttons(v);
}

/// The state at the right of the title bar.
pub fn state(t: &str, fg: Color) {
    sf::label(sf::F14, STATE, t, fg, BTN, 4, 1);
}

/// The cursor line and the -3 dB frequency.
pub fn readout(cursor: &str, m3db: &str) {
    sf::label(sf::TEXT22, CURSOR, cursor, C_CURSOR, BG, 1, 0);
    sf::label(sf::F14, M3DB, m3db, STATUS_FG, BG, 4, 1);
}

/// The buttons (Run / Stop by `running`; Thru in the accent once stored).
pub fn buttons(v: &Vals) {
    let run = if v.running { ("Stop", 0xF800) } else { ("Run", SEL) };
    for (r, t, bg) in [(BACK_BTN, "Back", BTN), (THRU_BTN, "Thru", if v.thru { 0x0320 } else { BTN }), (RUN_BTN, run.0, run.1), (SAVE_BTN, "Save", BTN), (PPD_BTN, v.ppd, BTN)] {
        sf::rbox(r, 6, bg, BG);
        sf::glyphs(sf::TEXT22, r, t, STATUS_FG, bg, 8, 0);
    }
}

/// Column (0 .. PW) of frequency `f` (Hz) on the log axis.
pub fn col_of(f: f32) -> f32 {
    let (a, b) = (bode::log10_f(bode::F_START as f32), bode::log10_f(bode::F_STOP as f32));
    (bode::log10_f(f) - a) / (b - a) * (PW - 1) as f32
}

/// The decade labels under the charts.
pub fn axis() {
    lcd::fill_rect(0, AXIS_Y as u16, lcd::W, 16, BG);
    for (f, t) in [(10.0f32, "10"), (100.0, "100"), (1e3, "1k"), (1e4, "10k")] {
        let x = PX + col_of(f) as i16;
        sf::label(sf::F14, rect((x - 16).max(PX - 6), AXIS_Y, 32, 14), t, STATUS_FG, BG, if f < 11.0 { 1 } else { 8 }, 1);
    }
    sf::label(sf::F14, rect(PX + PW - 40, AXIS_Y, 40, 14), "50k Hz", STATUS_FG, BG, 4, 1);
}

/// Gain chart scale: (top, bottom, step) dB, from the data (0 dB and at
/// least 20 dB shown; 10 dB steps up to 60 dB, else 20).
pub fn gain_scale(g: &[f32]) -> (f32, f32, f32) {
    let (mut lo, mut hi) = (0.0f32, 0.0f32);
    for &x in g {
        if x.is_finite() && x > -150.0 {
            lo = lo.min(x);
            hi = hi.max(x);
        }
    }
    let step = if hi - lo > 50.0 { 20.0 } else { 10.0 };
    let ceil = |x: f32| {
        let n = (x / step) as i32;
        (if (n as f32) * step < x { n + 1 } else { n }) as f32 * step
    };
    let floor = |x: f32| {
        let n = (x / step) as i32;
        (if (n as f32) * step > x { n - 1 } else { n }) as f32 * step
    };
    let (mut top, mut bot) = (ceil(hi + 1.0), floor(lo - 1.0));
    if top - bot < 2.0 * step {
        bot = top - 2.0 * step;
    }
    if top - bot > 6.0 * step {
        top = bot + 6.0 * step;
    }
    (top, bot, step)
}

/// Both charts, their labels and the cursor line.
pub fn charts(c: &Curves) {
    let n = c.f.len().min(c.g.len()).min(c.p.len());
    let (top, bot, step) = gain_scale(&c.g[..n]);
    chart(GY, GH, &c.f[..n], &c.g[..n], top, bot, step, C_GAIN, c.cursor);
    chart(PY, PH, &c.f[..n], &c.p[..n], 180.0, -180.0, 90.0, C_PHASE, c.cursor);
    // Left labels: a value at every grid row, "dB" with the top one.
    lcd::fill_rect(0, 60, PX as u16 - 1, AXIS_Y as u16 - 60, BG);
    let mut b = [0u8; 16];
    let mut t = top;
    while t >= bot - 0.01 {
        let y = GY + 1 + ((top - t) / (top - bot) * (GH - 3) as f32) as i16;
        let v = crate::dmm::format(&mut b, t, 0);
        let mut lb = [0u8; 20];
        let n = v.len().min(14);
        lb[..n].copy_from_slice(&v.as_bytes()[..n]);
        let n = if t == top { lb[n..n + 2].copy_from_slice(b"dB"); n + 2 } else { n };
        let s = core::str::from_utf8(&lb[..n]).unwrap_or("");
        sf::label(sf::F14, rect(0, (y - 7).clamp(GY, GY + GH - 14), PX - 3, 14), s, C_GAIN, BG, 4, 1);
        t -= step;
    }
    for (k, l) in ["180", "90", "0", "-90", "-180"].iter().enumerate() {
        let y = PY + 1 + (k as i16 * (PH - 3)) / 4;
        sf::label(sf::F14, rect(0, (y - 7).clamp(PY, PY + PH - 14), PX - 3, 14), l, C_PHASE, BG, 4, 1);
    }
}

/// One chart: `v` against `f` from `top` to `bot` (grid every `step`),
/// drawn column by column (a line through the points, linear in log f).
#[allow(clippy::too_many_arguments)]
fn chart(y0: i16, h: i16, f: &[f32], v: &[f32], top: f32, bot: f32, step: f32, fg: Color, cursor: Option<usize>) {
    let n = f.len().min(v.len());
    let row = |x: f32| -> i16 { (((top - x) / (top - bot)) * (h - 3) as f32 + 1.0).clamp(1.0, (h - 2) as f32) as i16 };
    let xs = |i: usize| col_of(f[i]);
    let cur_x = cursor.filter(|&i| i < n).map(|i| (xs(i) + 0.5) as i16);
    // Grid rows (every step) and the zero row.
    let mut grid = [i16::MIN; 8];
    let mut k = 0;
    let mut g = top;
    while g >= bot - 0.01 && k < grid.len() {
        grid[k] = row(g);
        k += 1;
        g -= step;
    }
    let zero = if top > 0.0 && bot < 0.0 { row(0.0) } else { i16::MIN };
    // Decade columns.
    let dec: [i16; 4] = [col_of(10.0) as i16, col_of(100.0) as i16, col_of(1e3) as i16, col_of(1e4) as i16];
    let mut seg = 0usize; // the segment [seg, seg + 1] of the column
    let mut prev: Option<i16> = None;
    for x in 0..PW {
        // The curve's rows in this column: from the previous column's value
        // to this one's (a vertical run joins them).
        let xf = x as f32;
        let val = if n >= 1 && xf >= xs(0) - 0.5 && xf <= xs(n - 1) + 0.5 {
            while seg + 2 < n && xs(seg + 1) < xf {
                seg += 1;
            }
            Some(if n == 1 {
                v[0]
            } else {
                let (a, b) = (xs(seg), xs(seg + 1));
                let u = if b > a { ((xf - a) / (b - a)).clamp(0.0, 1.0) } else { 0.0 };
                v[seg] + u * (v[seg + 1] - v[seg])
            })
        } else {
            None
        };
        let r = val.filter(|v| v.is_finite()).map(&row);
        let (ra, rb) = match (r, prev) {
            (Some(r), Some(p)) => (r.min(p), r.max(p)),
            (Some(r), None) => (r, r),
            _ => (i16::MAX, i16::MIN),
        };
        prev = r;
        let at_pt = (0..n).any(|i| xs(i) as i16 == x);
        let edge_x = x == 0 || x == PW - 1;
        let vline = dec.contains(&x);
        let is_cur = cur_x == Some(x);
        lcd::set_window((PX + x) as u16, y0 as u16, (PX + x) as u16, (y0 + h - 1) as u16);
        for y in 0..h {
            let edge = edge_x || y == 0 || y == h - 1;
            let on = y >= ra && y <= rb || (at_pt && r.is_some_and(|r| (y - r).abs() <= 1));
            lcd::data(if on {
                fg
            } else if edge {
                DIM
            } else if is_cur {
                C_CURSOR
            } else if y == zero {
                C_ZERO
            } else if vline || grid[..k].contains(&y) {
                C_GRID
            } else {
                BG
            });
        }
    }
}

fn hit(r: sf::Rect, x: u16, y: u16) -> bool {
    let (x, y) = (x as i16, y as i16);
    x >= r.x - 2 && x < r.x + r.w + 2 && y >= r.y - 2 && y < r.y + r.h + 2
}

/// Page touch: 0 Back, 1 Thru, 2 Run / Stop, 3 Save, 4 points per decade,
/// 5 the chart (the cursor).
pub fn page_hit(x: u16, y: u16) -> Option<u8> {
    [BACK_BTN, THRU_BTN, RUN_BTN, SAVE_BTN, PPD_BTN, CHART].iter().position(|&r| hit(r, x, y)).map(|i| i as u8)
}

/// The measured point nearest to a tap at column `x` (screen x).
pub fn nearest(f: &[f32], x: u16) -> Option<usize> {
    let c = x as f32 - PX as f32;
    (0..f.len()).min_by(|&a, &b| (col_of(f[a]) - c).abs().partial_cmp(&(col_of(f[b]) - c).abs()).unwrap_or(core::cmp::Ordering::Equal))
}
