//! Power-meter screens (dmm::pw, docs/power-meter-and-misconnect.md 3.9),
//! a child of ui.rs, in the stock DMM page's slots and visual language:
//!
//!   status bar  AUTO / MANU (both channels), HOLD / alerts as everywhere
//!   aux rows    Vrms (aux1), Irms (aux2): the stock LETTERS rows
//!   main        P (fallback: S) in the F78 cells; unit "W" in the stock UNIT
//!               face (two of its V, stockfont::glyph_or_twin), the AC+DC
//!               glyph above it (P counts AC and DC)
//!   history row the five 88 x 19 boxes: PF, S, Q, f, burden voltage (F14
//!               tag + HIST value; units the HIST face lacks in F14)
//!   chart       P (the main value), as every function
//!   side column Wh, VAh, the energy box (run / pause, time; tap = run /
//!               pause, long press = reset), average P, ranges / the
//!               continuous-current countdown
//!
//! Entry: long press on the A button or "Power" on the Current Settings page
//! -> the wiring dialog (`entry_dialog`) -> OK. Any function button leaves.
//! The MF key opens the Power Settings page (`settings_page`).

use super::*;
use crate::dmm::pw::{self, PowerVals};

/// Caches of what the power rows / side column last drew (text per slot).
#[derive(Clone, Copy)]
pub struct PwCache {
    rows: [u32; 5],
    side: [u32; 5],
}

impl PwCache {
    pub const fn new() -> Self {
        PwCache { rows: [0; 5], side: [0; 5] }
    }
}

/// Small text buffer.
struct T {
    b: [u8; 32],
    n: usize,
}

impl T {
    fn new() -> T {
        T { b: [0; 32], n: 0 }
    }
    fn push(&mut self, s: &str) -> &mut T {
        let k = s.len().min(32 - self.n);
        self.b[self.n..self.n + k].copy_from_slice(&s.as_bytes()[..k]);
        self.n += k;
        self
    }
    fn s(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
    /// FNV-1a of the text (cache key; never 0 = "not drawn").
    fn key(&self) -> u32 {
        let mut h = 0x811C_9DC5u32;
        for &b in &self.b[..self.n] {
            h = (h ^ b as u32).wrapping_mul(0x0100_0193);
        }
        h | 1
    }
}

/// `v` with `sig` significant digits (at most 4 decimals, none above
/// 10^sig): "229.98", "0.9987", "12345"; "--" for NaN.
pub fn sig_text(buf: &mut [u8; 16], v: f32, sig: u8) -> &str {
    if !v.is_finite() {
        buf[..2].copy_from_slice(b"--");
        return core::str::from_utf8(&buf[..2]).unwrap_or("");
    }
    let a = v.abs();
    let mut dec = sig as i32 - 1;
    let mut x = 1.0f32;
    while a >= x * 10.0 && dec > 0 {
        x *= 10.0;
        dec -= 1;
    }
    // 999.99 at 4 digits rounds to 1000.0: one decimal less.
    let mut p = 1.0f32;
    for _ in 0..dec.clamp(0, 4) {
        p *= 10.0;
    }
    if dec > 0 && a * p + 0.5 >= x * 10.0 * p {
        dec -= 1;
    }
    crate::dmm::format(buf, v, dec.clamp(0, 4) as u8)
}

/// History-row box `i` (stock 5 + 95 i, 153, 88 x 19).
fn row_rect(i: usize) -> sf::Rect {
    rect(5 + 95 * i as i16, 153, 88, 19)
}

/// Every character of `s` in the HIST face.
fn in_hist(s: &str) -> bool {
    s.chars().all(|c| sf::HIST.glyph(c as u32).is_some())
}

/// One power box: tag (F14, dim) left, value (HIST) and unit (HIST, or F14
/// when the HIST face lacks a letter) right-aligned.
fn power_box(i: usize, tag: &str, val: &str, unit: &str, warn: bool) {
    let r = row_rect(i);
    sf::rbox(r, 3, BAR, BG);
    sf::glyphs(sf::F14, rect(r.x + 4, r.y, 30, r.h), tag, DIM, BAR, 1, 1);
    let fg = if warn { VALUE } else { STATUS_FG };
    let end = r.x + r.w - 4;
    let (uf, us) = if in_hist(unit) { (sf::HIST, 0) } else { (sf::F14, 1) };
    let uw = if unit.is_empty() { 0 } else { sf::text_width(uf, unit, us) + 1 };
    if uw > 0 {
        sf::glyphs(uf, rect(end - uw, r.y, uw, r.h), unit, fg, BAR, 4, us);
    }
    // 3 px between the value's ink and the unit's.
    let ve = if uw > 0 { end - uw - 3 } else { end };
    sf::glyphs(sf::HIST, rect(r.x + 4, r.y, ve - r.x - 4, r.h), val, fg, BAR, 4, 0);
}

impl DmmView {
    /// The power boxes in the history row's place (redrawn per box when its
    /// text changed). PF, Q are "--" in the fallback (no phase relation).
    pub fn power_row(&mut self, pv: &PowerVals) {
        let mut b = [0u8; 16];
        let pf = if pv.pf.is_finite() { crate::dmm::format(&mut b, pv.pf, 3) } else { "--" };
        let mut pft = T::new();
        pft.push(pf);
        // Below 1 VA: mVA / mvar (the mW ranges).
        let (k, su, qu, sg) = if pv.s < 1.0 { (1e3, "mVA", "mvar", 3) } else { (1.0, "VA", "var", 4) };
        let mut st = T::new();
        st.push(sig_text(&mut b, pv.s * k, sg));
        let mut qt = T::new();
        qt.push(sig_text(&mut b, pv.q * k, sg));
        let (fv, fu) = if pv.f_hz >= 1000.0 { (pv.f_hz * 1e-3, "kHz") } else { (pv.f_hz, "Hz") };
        let mut ft = T::new();
        ft.push(if pv.f_hz > 0.0 { sig_text(&mut b, fv, if pv.f_hz >= 1000.0 { 3 } else { 4 }) } else { "--" });
        let mut bt = T::new();
        bt.push(sig_text(&mut b, pv.burden_v, 4));
        let items: [(&str, &T, &str); 5] = [("PF", &pft, ""), ("S", &st, su), ("Q", &qt, qu), ("f", &ft, fu), ("Vb", &bt, "V")];
        for (i, (tag, t, u)) in items.iter().enumerate() {
            let mut k = T::new();
            k.push(t.s()).push(u).push(if pv.comp { "c" } else { "" });
            let key = k.key();
            if self.pwc.rows[i] != key {
                self.pwc.rows[i] = key;
                // Burden box: amber with the compensation off and the drop
                // above 1 % of the V reading (it is inside P then).
                let warn = i == 4 && !pv.comp && pv.burden_v > 0.01 * pv.v;
                power_box(i, tag, t.s(), u, warn);
            }
        }
    }
}

/// The side column in the power meter (replaces MAX / AVG / REL / MIN /
/// time; same rows, same F14 face, the REL box is the energy box).
pub fn side(c: &mut PwCache, pv: &PowerVals) {
    let mut b = [0u8; 16];
    // Wh / VAh rows, label dim left, value right (no unit: the label is it).
    let wh = if pv.alt { "--" } else { sig_text(&mut b, pv.e_wh, 5) };
    let mut t0 = T::new();
    t0.push(wh);
    let mut b1 = [0u8; 16];
    let mut t1 = T::new();
    t1.push(sig_text(&mut b1, pv.vah, 5));
    for (i, (lbl, t)) in [("Wh", &t0), ("VAh", &t1)].iter().enumerate() {
        let k = t.key();
        if c.side[i] != k {
            c.side[i] = k;
            sf::label(sf::F14, rect(SIDE_X, SIDE_Y[i], 24, 14), lbl, DIM, BG, 0, 1);
            sf::label(sf::F14, rect(SIDE_X + 26, SIDE_Y[i], SIDE_R - SIDE_X - 26, 14), t.s(), STATUS_FG, BG, 3, 1);
        }
    }
    // Energy box (405, 205, 71, 38): "E" + RUN / PAUSE, the time below.
    let mut tb = [0u8; 8];
    let mut k2 = T::new();
    k2.push(if pv.run { "RUN" } else { "PAUSE" }).push(hms(&mut tb, pv.secs));
    if c.side[2] != k2.key() {
        c.side[2] = k2.key();
        let r = rect(405, 205, 71, 38);
        sf::rbox(r, 6, BTN, BG);
        sf::glyphs(sf::F14, rect(409, 209, 14, 14), "E", STATUS_FG, BTN, 0, 1);
        sf::glyphs(sf::F14, rect(426, 209, 46, 14), if pv.run { "RUN" } else { "PAUSE" }, if pv.run { 0x07E0 } else { VALUE }, BTN, 3, 1);
        sf::glyphs(sf::F14, rect(409, 225, 63, 14), hms(&mut tb, pv.secs), STATUS_FG, BTN, 3, 1);
    }
    // Average P over the integration time.
    let mut t3 = T::new();
    let mut b3 = [0u8; 16];
    t3.push(sig_text(&mut b3, pv.p_avg, 5));
    if c.side[3] != t3.key() {
        c.side[3] = t3.key();
        sf::label(sf::F14, rect(SIDE_X, SIDE_Y[2], 24, 14), "AVG", DIM, BG, 0, 1);
        sf::label(sf::F14, rect(SIDE_X + 26, SIDE_Y[2], SIDE_R - SIDE_X - 26, 14), t3.s(), STATUS_FG, BG, 3, 1);
    }
    // Ranges, or the continuous-current countdown / alarm.
    let mut t4 = T::new();
    let fg = match pv.over {
        1 => {
            let mut b4 = [0u8; 16];
            t4.push("I>LIM ").push(crate::dmm::format(&mut b4, pv.over_left as f32, 0)).push("s");
            VALUE
        }
        2 => {
            t4.push("I>LIM!");
            0xF800
        }
        _ => {
            t4.push(pw::V_NAMES[pv.vr as usize & 3]).push(" ").push(pw::I_NAMES[pv.ik.min(5) as usize]);
            DIM
        }
    };
    let mut k4 = T::new();
    k4.push(t4.s()).push(if fg == DIM { "d" } else if fg == VALUE { "v" } else { "r" });
    if c.side[4] != k4.key() {
        c.side[4] = k4.key();
        sf::label(sf::F14, rect(SIDE_X, SIDE_Y[3], SIDE_R - SIDE_X, 14), t4.s(), fg, BG, 3, 1);
    }
}

/// Clear the side column (entering the power meter: the MIN / MAX labels).
pub fn side_clear(c: &mut PwCache) {
    lcd::fill_rect(402, 174, 78, 105, BG);
    c.side = [0; 5];
}

/// Energy box tap target (the stock REL box).
pub fn side_box_hit(x: u16, y: u16) -> bool {
    x >= 402 && (205..243).contains(&y)
}

// ------------------------------------------------------------ entry dialog

/// Wiring dialog box and its buttons (screen coordinates, DBG:TAP).
pub const DLG: (i16, i16, i16, i16) = (10, 8, 460, 304);
pub const DLG_CANCEL: sf::Rect = rect(30, 262, 190, 40);
pub const DLG_OK: sf::Rect = rect(260, 262, 190, 40);
const DLG_TITLE: Color = 0x04FC;
const DLG_BODY: Color = 0x10A3;
const WIRE: Color = 0xFC40;

fn hit(r: sf::Rect, x: u16, y: u16) -> bool {
    let (x, y) = (x as i16, y as i16);
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// Dialog hit: Some(true) OK, Some(false) Cancel.
pub fn entry_hit(x: u16, y: u16) -> Option<bool> {
    if hit(DLG_OK, x, y) {
        Some(true)
    } else if hit(DLG_CANCEL, x, y) {
        Some(false)
    } else {
        None
    }
}

/// A jack of the meter: a ring with its name under it.
fn jack(cx: i16, cy: i16, name: &str) {
    sf::rbox(rect(cx - 11, cy - 11, 22, 22), 11, DIM, DLG_BODY);
    sf::rbox(rect(cx - 6, cy - 6, 12, 12), 6, DLG_BODY, DIM);
    sf::label(sf::F14, rect(cx - 20, cy + 15, 40, 14), name, STATUS_FG, DLG_BODY, 8, 1);
}

/// Wire segment (2 px).
fn hwire(x0: i16, x1: i16, y: i16) {
    lcd::fill_rect(x0 as u16, y as u16, (x1 - x0) as u16, 2, WIRE);
}
fn vwire(x: i16, y0: i16, y1: i16) {
    lcd::fill_rect(x as u16, y0 as u16, 2, (y1 - y0) as u16, WIRE);
}

/// The power-meter entry dialog (a modal overlay, dialog.rs): the wiring of
/// doc 3.6 (V/ohm on the source side, the return conductor broken through
/// A and COM), the sign convention, the current rating, Cancel / OK.
/// `i_cont` / `t_lim`: the continuous-current limit setting.
pub fn entry_dialog(i_cont: f32, t_lim: u16) {
    let (x, y, w, h) = DLG;
    lcd::fill_rect(x as u16, y as u16, w as u16, 34, DLG_TITLE);
    sf::glyphs(sf::TEXT22, rect(x, y, w, 34), "Power meter: wiring", STATUS_FG, DLG_TITLE, 8, 0);
    lcd::fill_rect(x as u16, (y + 34) as u16, w as u16, (h - 34) as u16, DLG_BODY);
    // Source (left) and load (right): rounded boxes with + / - terminals.
    for (bx, name) in [(28i16, "SOURCE"), (372, "LOAD")] {
        sf::rbox(rect(bx, 56, 80, 112), 8, BTN, DLG_BODY);
        sf::glyphs(sf::F14, rect(bx, 104, 80, 16), name, STATUS_FG, BTN, 8, 1);
        sf::glyphs(sf::F14, rect(bx, 62, 80, 16), "+", STATUS_FG, BTN, 8, 1);
        sf::glyphs(sf::F14, rect(bx, 146, 80, 16), "-", STATUS_FG, BTN, 8, 1);
    }
    // Jacks COM, V/ohm, A (left to right: no crossing wires).
    let jy = 186;
    jack(180, jy, "COM");
    jack(240, jy, "V");
    jack(300, jy, "A");
    // + : source -> load, V taps it.
    hwire(108, 372, 69);
    vwire(239, 69, jy - 11);
    // The return conductor broken through the meter: load - -> A, COM ->
    // source - (the load current into A, out of COM: P > 0).
    hwire(108, 181, 155);
    vwire(179, 155, jy - 11);
    hwire(299, 372, 155);
    vwire(299, 155, jy - 11);
    // Current direction marks on the two wires.
    sf::label(sf::F14, rect(120, 137, 50, 14), "< I", STATUS_FG, DLG_BODY, 0, 1);
    sf::label(sf::F14, rect(306, 137, 64, 14), "< I in A", STATUS_FG, DLG_BODY, 0, 1);
    sf::label(sf::F14, rect(250, 80, 116, 14), "V: source side", DIM, DLG_BODY, 0, 1);
    // Notes: what COM carries, the sign, the rating.
    let mut t = T::new();
    let mut b = [0u8; 16];
    t.push("Max 10 A; above ").push(sig_text(&mut b, i_cont, 2)).push(" A: ");
    let mut b2 = [0u8; 16];
    if t_lim > 0 {
        t.push(crate::dmm::format(&mut b2, t_lim as f32, 0)).push(" s");
    } else {
        t.push("no limit");
    }
    sf::label(sf::F14, rect(x + 16, 222, w - 32, 14), "COM carries the load current; P > 0: I into A", STATUS_FG, DLG_BODY, 8, 1);
    sf::label(sf::F14, rect(x + 16, 240, w - 32, 14), t.s(), VALUE, DLG_BODY, 8, 1);
    for (r, l, c) in [(DLG_CANCEL, "Cancel", BTN), (DLG_OK, "OK", SEL)] {
        sf::rbox(r, 6, c, DLG_BODY);
        sf::glyphs(sf::TEXT22, r, l, STATUS_FG, c, 8, 0);
    }
}

// ------------------------------------------------------------ settings page

/// Power Settings page touch codes (besides range items).
pub const PS_V: u8 = 0; // + 0..3 = V range, 4 = V AUTO
pub const PS_I: u8 = 8; // + 0..5 = I range, 6 = I AUTO
pub const PS_METHOD_B: u8 = 20;
pub const PS_METHOD_ALT: u8 = 21;
pub const PS_BURDEN: u8 = 22;
pub const PS_RESET: u8 = 23;
pub const PS_WIRING: u8 = 24;
pub const PS_OK: u8 = 25;

/// Grid: label column 8..40, five 83 px columns from 44 (87 px pitch).
fn ps_cell(row: i16, col: i16) -> sf::Rect {
    rect(44 + 87 * col, 46 + 44 * row, 83, 38)
}

const PS_ROW4: [i16; 3] = [8, 162, 319];

fn ps_button(r: sf::Rect, t: &str, sel: bool) {
    let c = if sel { SEL } else { 0x2966 };
    sf::rbox(r, 6, c, BG);
    sf::glyphs(sf::TEXT22, r, t, STATUS_FG, c, 8, 0);
}

/// The page: V ranges + AUTO, I ranges + AUTO (two rows), method, burden
/// compensation, Reset energy / Wiring / OK.
pub fn settings_page(pv: &PowerVals) {
    lcd::fill_rect(0, 0, lcd::W, 40, 0x2966);
    lcd::fill_rect(0, 40, lcd::W, lcd::H - 40, BG);
    sf::logo(4, 10, 0x2966);
    sf::glyphs(sf::TEXT22, rect(0, 0, 480, 40), "Power Settings", STATUS_FG, 0x2966, 8, 0);
    sf::label(sf::TEXT22, rect(8, 46, 32, 38), "V", STATUS_FG, BG, 8, 0);
    sf::label(sf::TEXT22, rect(8, 90, 32, 38), "A", STATUS_FG, BG, 8, 0);
    for (i, n) in pw::V_NAMES.iter().enumerate() {
        ps_button(ps_cell(0, i as i16), n, !pv.v_auto && pv.vr as usize == i);
    }
    ps_button(ps_cell(0, 4), "AUTO", pv.v_auto);
    for (i, n) in pw::I_NAMES.iter().enumerate() {
        ps_button(ps_cell((1 + i / 5) as i16, (i % 5) as i16), n, !pv.i_auto && pv.ik as usize == i);
    }
    ps_button(ps_cell(2, 1), "AUTO", pv.i_auto);
    for (i, (t, sel)) in [("P + PF", !pv.alt), ("VA only", pv.alt), (if pv.comp { "Burden ON" } else { "Burden OFF" }, pv.comp)].iter().enumerate() {
        ps_button(rect(PS_ROW4[i], 182, 150, 38), t, *sel);
    }
    for (i, t) in ["Reset Wh", "Wiring", "OK"].iter().enumerate() {
        ps_button(rect(PS_ROW4[i], 240, 150, 38), t, false);
    }
}

/// Settings page hit (PS_* codes).
pub fn settings_hit(x: u16, y: u16) -> Option<u8> {
    for row in 0..3i16 {
        for col in 0..5i16 {
            let r = ps_cell(row, col);
            if hit(rect(r.x - 2, r.y - 3, r.w + 4, r.h + 6), x, y) {
                return match (row, col) {
                    (0, c) => Some(PS_V + c as u8),
                    (1, c) => Some(PS_I + c as u8),
                    (2, 0) => Some(PS_I + 5),
                    (2, 1) => Some(PS_I + 6),
                    _ => None,
                };
            }
        }
    }
    for (i, &bx) in PS_ROW4.iter().enumerate() {
        if hit(rect(bx, 182, 150, 38), x, y) {
            return Some(PS_METHOD_B + i as u8);
        }
        if hit(rect(bx, 240, 150, 38), x, y) {
            return Some(PS_RESET + i as u8);
        }
    }
    None
}
