//! The Power Wave page (View::PWave; src/app_pwave.rs, src/pwave.rs; doc
//! power-meter-and-misconnect.md 3.18): the power meter's voltage (U,
//! yellow) and current (I, cyan) on one plot, each with its own scale.
//!
//!   0..40     title bar "Power Wave", the timebase at the right
//!   44..70    U  I  P  f readouts (whole cycles of the frame)
//!   74..250   the plot (x 40..440, 10 x 40 px; 8 x 22 px, zero in the
//!             middle for both), "U" left of it, "I" right of it
//!   252..276  the scales: "U 100V/div" left, "I 500mA/div" right
//!   280..316  [ Back ] [ - ms/div ] [ + ms/div ] [ STOP / RUN ]

use super::*;
use crate::pwave::{PH, PW};

/// Trace colours: U as the scope's CH1, I cyan.
pub const C_U: Color = 0xFFE0;
pub const C_I: Color = 0x07FF;
pub const C_OL: Color = 0xF800;
/// Plot origin.
pub const PX: u16 = 40;
pub const PY: u16 = 74;
const GRID: Color = 0x4208;
const AXIS: Color = 0x7BEF;
/// Readout boxes (U, I, P, f) and the bottom buttons (4-column grid).
/// (Centred so that the widest texts, "U 999.9mV", "I 999.9uA",
/// "P -999.9mW", "f 999.9Hz", keep 1.5 word spaces apart.)
const RD: [sf::Rect; 4] = [rect(4, 44, 101, 26), rect(117, 44, 108, 26), rect(237, 44, 117, 26), rect(365, 44, 107, 26)];
pub const BTN_R: [sf::Rect; 4] = [rect(8, 280, 112, 36), rect(124, 280, 112, 36), rect(240, 280, 112, 36), rect(356, 280, 116, 36)];
const BTN_T: [&str; 3] = ["Back", "- ms/div", "+ ms/div"];
/// The channel names beside the plot at its zero line, the scales under it.
const NAME_R: [sf::Rect; 2] = [rect(4, 150, 32, 26), rect(444, 150, 32, 26)];
const SCL_R: [sf::Rect; 2] = [rect(40, 252, 196, 24), rect(244, 252, 196, 24)];
const TB_R: sf::Rect = rect(300, 0, 172, 40);

/// What the page shows (texts formatted by the app, `fmt`).
pub struct Vals<'a> {
    pub u: &'a str,
    pub i: &'a str,
    pub p: &'a str,
    pub f: &'a str,
    pub u_div: &'a str,
    pub i_div: &'a str,
    pub tb: &'a str,
    pub run: bool,
    pub u_ol: bool,
    pub i_ol: bool,
}

/// The whole page but the plot's traces (`plot` draws them).
pub fn page(v: &Vals) {
    lcd::fill_rect(0, 0, lcd::W, 40, BTN);
    lcd::fill_rect(0, 40, lcd::W, lcd::H - 40, BG);
    sf::glyphs(sf::TEXT22, rect(8, 0, 282, 40), "Power Wave", STATUS_FG, BTN, 1, 0);
    for (r, t) in BTN_R.iter().zip(BTN_T) {
        sf::rbox(*r, 6, BTN, BG);
        sf::glyphs(sf::TEXT22, *r, t, STATUS_FG, BTN, 8, 0);
    }
    live(v);
}

/// The parts that change per frame: timebase, readouts, scales, RUN /
/// STOP.
pub fn live(v: &Vals) {
    sf::label(sf::TEXT22, TB_R, v.tb, STATUS_FG, BTN, 4, 0);
    let items = [(v.u, if v.u_ol { C_OL } else { C_U }), (v.i, if v.i_ol { C_OL } else { C_I }), (v.p, STATUS_FG), (v.f, STATUS_FG)];
    for (r, (t, c)) in RD.iter().zip(items) {
        sf::label(sf::TEXT22, *r, t, c, BG, 8, 0);
    }
    let mut b = [0u8; 24];
    for (k, (name, step, c)) in [("U", v.u_div, C_U), ("I", v.i_div, C_I)].iter().enumerate() {
        sf::label(sf::TEXT22, NAME_R[k], name, *c, BG, 8, 0);
        let t = if step.is_empty() { "" } else { join(&mut b, &[name, " ", step, "/div"]) };
        sf::label(sf::TEXT22, SCL_R[k], t, *c, BG, if k == 0 { 1 } else { 4 }, 0);
    }
    run_button(v.run);
}

/// STOP (running: tap to hold the frame) / RUN (held).
pub fn run_button(run: bool) {
    let (t, c) = if run { ("STOP", BTN) } else { ("RUN", SEL) };
    sf::rbox(BTN_R[3], 6, c, BG);
    sf::glyphs(sf::TEXT22, BTN_R[3], t, STATUS_FG, c, 8, 0);
}

/// The plot: the grid, then per column the rows of I and U (`cols`:
/// pwave::columns, I at [0..PW], U at [PW..2 PW]; None = the grid only).
/// U is drawn over I where both pass.
pub fn plot(cols: Option<&[u16]>) {
    for x in 0..PW {
        let span = |ch: usize| cols.map_or(0xFFFF, |c| c[ch * PW + x]);
        let (i, u) = (span(0), span(1));
        let vline = x % crate::pwave::DIV_PX_X == 0 || x == PW - 1;
        let cx = x == PW / 2;
        lcd::set_window(PX + x as u16, PY, PX + x as u16, PY + PH as u16 - 1);
        for y in 0..PH as u16 {
            let hit = |s: u16| s != 0xFFFF && y >= (s & 0xFF) && y <= (s >> 8);
            let hline = y % crate::pwave::DIV_PX_Y as u16 == 0 || y == PH as u16 - 1;
            let cy = y == PH as u16 / 2;
            lcd::data(if hit(u) {
                C_U
            } else if hit(i) {
                C_I
            } else if cx || cy {
                AXIS
            } else if (vline && y % 5 == 0) || (hline && x % 5 == 0) {
                GRID
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

/// Page touch: 0 Back, 1 "- ms/div" (shorter timebase), 2 "+ ms/div",
/// 3 RUN / STOP.
pub fn page_hit(x: u16, y: u16) -> Option<u8> {
    BTN_R.iter().position(|&r| hit(r, x, y)).map(|i| i as u8)
}

/// `v` with 4 significant digits and an SI prefix (k, -, m, u) on `unit`,
/// after `name` ("U 230.1V"); "--" for NaN.
pub fn fmt<'a>(b: &'a mut [u8; 24], name: &str, v: f32, unit: &str) -> &'a str {
    let mut nb = [0u8; 16];
    if !v.is_finite() {
        return join(b, &[name, " --", unit]);
    }
    let a = v.abs();
    let (s, pfx) = if a >= 9999.5 {
        (1e-3, "k")
    } else if a >= 0.99995 || a == 0.0 {
        (1.0, "")
    } else if a >= 0.00099995 {
        (1e3, "m")
    } else {
        (1e6, "u")
    };
    let x = v * s;
    let ax = x.abs();
    let dec = if ax < 9.9995 { 3 } else if ax < 99.995 { 2 } else if ax < 999.95 { 1 } else { 0 };
    let n = crate::dmm::format(&mut nb, x, dec);
    join(b, &[name, " ", n, pfx, unit])
}

/// A per-division step ("100V", "500mA", "20uA").
pub fn fmt_div<'a>(b: &'a mut [u8; 24], v: f32, unit: &str) -> &'a str {
    let (x, pfx) = if v >= 0.99995 { (v, "") } else if v >= 0.00099995 { (v * 1e3, "m") } else { (v * 1e6, "u") };
    let mut nb = [0u8; 16];
    let n = crate::dmm::format(&mut nb, x, 0);
    join(b, &[n, pfx, unit])
}
