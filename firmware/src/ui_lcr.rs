//! The LCR page (View::Lcr; src/app_lcr.rs, src/lcr.rs; docs/lcr.md):
//! the Power Settings look (title bar, BTN boxes, SEL accent).
//!
//!   0..40     title bar "LCR", the test frequency and the I range right
//!   44..132   main box: the model's name ("Cs", "Lp", "Rs") top left, the
//!             value (F78) and its unit
//!   136..198  two rows of three readouts: ESR / Rp, D, Q; |Z|, phase, I
//!   200..226  the state line (wiring hint, calibration, OL)
//!   232..268  [ 100Hz ] [ 1kHz ]   [ Auto ] [ Series ] [ Parallel ]
//!   280..316  [ Back ] [ Short ] [ Open ] [ R cal ] [ STOP / RUN ]

use super::*;

/// The state line's colours.
pub const C_OK: Color = 0x07E0;
pub const C_WARN: Color = 0xFD20;
pub const C_PLAIN: Color = STATUS_FG;
pub const C_DIM: Color = DIM;

const TITLE_R: sf::Rect = rect(150, 0, 322, 40);
const MAIN: sf::Rect = rect(8, 44, 464, 88);
const NAME_R: sf::Rect = rect(16, 48, 40, 26);
const VAL_R: sf::Rect = rect(60, 50, 328, 78);
const UNIT_R: sf::Rect = rect(392, 78, 74, 40);
/// The readout cells (3 x 2).
const CELL_X: [i16; 3] = [8, 166, 324];
const CELL_W: i16 = 148;
const CELL_Y: [i16; 2] = [136, 168];
const INFO: sf::Rect = rect(8, 200, 464, 26);
/// The setting row: frequencies, then the models.
pub const SET_R: [sf::Rect; 5] = [rect(8, 232, 80, 36), rect(92, 232, 80, 36), rect(184, 232, 86, 36), rect(274, 232, 94, 36), rect(372, 232, 100, 36)];
const SET_T: [&str; 5] = ["100Hz", "1kHz", "Auto", "Series", "Parallel"];
/// The bottom row (STOP / RUN at the right).
pub const BTN_R: [sf::Rect; 5] = [rect(8, 280, 89, 36), rect(101, 280, 89, 36), rect(194, 280, 89, 36), rect(287, 280, 89, 36), rect(380, 280, 92, 36)];
const BTN_T: [&str; 4] = ["Back", "Short", "Open", "R cal"];

/// What the page shows (texts formatted by the app, `num` / `cell`).
pub struct Vals<'a> {
    /// Right of the title: "1kHz  I 60mA".
    pub title: &'a str,
    /// The model's name ("Cs"), the value ("4.700", "OL", "" = none) and
    /// its unit ("uF").
    pub name: &'a str,
    pub val: &'a str,
    pub unit: &'a str,
    /// ESR / Rp, D, Q, |Z|, phase, I.
    pub cells: [&'a str; 6],
    pub info: &'a str,
    pub info_fg: Color,
    /// The frequency index (lcr::FREQS) and the model (0 Auto, 1 Series,
    /// 2 Parallel).
    pub fi: u8,
    pub model: u8,
    pub run: bool,
}

/// The whole page.
#[inline(never)]
pub fn page(v: &Vals) {
    lcd::fill_rect(0, 0, lcd::W, 40, BTN);
    lcd::fill_rect(0, 40, lcd::W, lcd::H - 40, BG);
    sf::glyphs(sf::TEXT22, rect(8, 0, 140, 40), "LCR", STATUS_FG, BTN, 1, 0);
    sf::rbox(MAIN, 6, BTN, BG);
    for (k, t) in BTN_T.iter().enumerate() {
        sf::rbox(BTN_R[k], 6, BTN, BG);
        sf::glyphs(sf::TEXT22, BTN_R[k], t, STATUS_FG, BTN, 8, 0);
    }
    live(v);
}

/// The parts that change per frame or tap.
#[inline(never)]
pub fn live(v: &Vals) {
    sf::label(sf::TEXT22, TITLE_R, v.title, STATUS_FG, BTN, 4, 0);
    sf::label(sf::TEXT22, NAME_R, v.name, STATUS_FG, BTN, 1, 0);
    let val = if v.val.is_empty() { "-----" } else { v.val };
    let f = if sf::has_all(sf::F78, val) { sf::F78 } else { sf::LETTERS };
    sf::label(f, VAL_R, val, VALUE, BTN, 4, 0);
    let mut uf = if sf::has_all(sf::UNIT, v.unit) { sf::UNIT } else { sf::LETTERS };
    if sf::text_width(uf, v.unit, 0) > UNIT_R.w || !sf::has_all(uf, v.unit) {
        uf = sf::TEXT22;
    }
    sf::label(uf, UNIT_R, v.unit, VALUE, BTN, 1, 0);
    for (k, t) in v.cells.iter().enumerate() {
        let r = rect(CELL_X[k % 3], CELL_Y[k / 3], CELL_W, 30);
        sf::label(sf::TEXT22, r, t, STATUS_FG, BG, 1, 0);
    }
    info(v.info, v.info_fg);
    for (k, t) in SET_T.iter().enumerate() {
        let sel = if k < 2 { k == v.fi as usize } else { k - 2 == v.model as usize };
        let c = if sel { SEL } else { BTN };
        sf::rbox(SET_R[k], 6, c, BG);
        sf::glyphs(sf::TEXT22, SET_R[k], t, STATUS_FG, c, 8, 0);
    }
    run_button(v.run);
}

/// The state line.
pub fn info(t: &str, fg: Color) {
    sf::label(sf::TEXT22, INFO, t, fg, BG, 1, 0);
}

/// STOP (running) / RUN (held).
pub fn run_button(run: bool) {
    let (t, c) = if run { ("STOP", BTN) } else { ("RUN", SEL) };
    sf::rbox(BTN_R[4], 6, c, BG);
    sf::glyphs(sf::TEXT22, BTN_R[4], t, STATUS_FG, c, 8, 0);
}

fn hit(r: sf::Rect, x: u16, y: u16) -> bool {
    let (x, y) = (x as i16, y as i16);
    x >= r.x - 2 && x < r.x + r.w + 2 && y >= r.y - 2 && y < r.y + r.h + 2
}

/// Page touch: 0 Back, 1 Short, 2 Open, 3 R cal, 4 RUN / STOP; 10 + i a
/// frequency, 20 + m a model.
pub fn page_hit(x: u16, y: u16) -> Option<u8> {
    if let Some(i) = BTN_R.iter().position(|&r| hit(r, x, y)) {
        return Some(i as u8);
    }
    SET_R.iter().position(|&r| hit(r, x, y)).map(|i| if i < 2 { 10 + i as u8 } else { 18 + i as u8 })
}

/// `parts` joined (at most 24 bytes).
pub fn cat<'a>(b: &'a mut [u8; 24], parts: &[&str]) -> &'a str {
    join(b, parts)
}

/// `v` as 4 significant digits with an SI prefix: the number and the
/// prefix (p n u m "" k M); None for NaN / infinite.
pub fn eng<'a>(b: &'a mut [u8; 16], v: f32) -> Option<(&'a str, &'static str)> {
    if !v.is_finite() {
        return None;
    }
    const P: [(f32, &str); 7] = [(1e6, "M"), (1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "u"), (1e-9, "n"), (1e-12, "p")];
    let a = v.abs();
    let (s, pfx) = P.iter().find(|(s, _)| a >= *s * 0.99995).copied().unwrap_or(P[6]);
    let x = v / s;
    let ax = x.abs();
    let dec = if ax < 9.9995 { 3 } else if ax < 99.995 { 2 } else { 1 };
    Some((crate::dmm::format(b, x, dec), pfx))
}

/// A readout cell: `name` " " the value with its prefix and `unit`
/// ("ESR 1.234ohm" as "ESR 1.234" + "Ω"); "--" for NaN.
pub fn cell<'a>(b: &'a mut [u8; 24], name: &str, v: f32, unit: &str) -> &'a str {
    let mut nb = [0u8; 16];
    match eng(&mut nb, v) {
        Some((n, p)) => join(b, &[name, " ", n, p, unit]),
        None => join(b, &[name, " --"]),
    }
}

/// A plain number cell with `dec` decimals ("D 0.0123"), "--" for NaN,
/// ">999" above 999.
pub fn plain<'a>(b: &'a mut [u8; 24], name: &str, v: f32, dec: u8) -> &'a str {
    let mut nb = [0u8; 16];
    if !v.is_finite() {
        return join(b, &[name, " --"]);
    }
    if v.abs() > 999.0 {
        return join(b, &[name, " >999"]);
    }
    let dec = if v.abs() >= 99.95 { 1 } else if v.abs() >= 9.995 { 2 } else { dec };
    join(b, &[name, " ", crate::dmm::format(&mut nb, v, dec)])
}
