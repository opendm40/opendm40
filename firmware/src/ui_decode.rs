//! The protocol decoder's bus band on the scope graph (decode.rs,
//! app_decode.rs) and its setup popup (scope MF > Settings > Decode), a
//! src/scope_ui.rs submodule (its colours, popup widgets, graph holes).
//!
//! The band is a graph hole at the bottom of the graph, above the timebase
//! tabs (both views: the frame's bottom edge is y 300): one row of byte
//! boxes (UART and Modbus), and for Modbus a row of frame boxes above it.
//! A byte box spans the character's time (start edge to the stop bit's
//! end) with its hex and, when there is room, the character; a frame box
//! the frame with its fields (decode::mb_text, shortened to fit) and, for
//! a reply whose request is on the screen, the reply time. Errors
//! (framing, parity, CRC) in red. Zoomed out (a character narrower than
//! `MIN_BOX` px, or too few samples per bit) the row is a bar over the
//! columns where the line toggles. What the band shows is `ANN`, filled by
//! app_decode.rs from each decoded screen; `Model::dec_gen` changes with it.

use super::*;
use crate::decode as dc;

/// Byte boxes / frame boxes kept for one screen.
pub const NB: usize = 44;
pub const NF: usize = 12;
/// A character narrower than this (px) gives the bar instead of boxes.
pub const MIN_BOX: i32 = 10;
/// Row height and the rows' y (both views share the frame's bottom).
pub const ROW_H: i16 = 17;
pub const BYTE_Y: i16 = 259;
pub const FRAME_Y: i16 = 241;

/// A byte box: start / end sample relative to the window start, the value,
/// F_FE / F_PE.
#[derive(Clone, Copy)]
pub struct BAnn {
    pub s0: i32,
    pub s1: i32,
    pub v: u8,
    pub fl: u8,
}

/// A frame box: start / end sample, kind, flags, length, first bytes, the
/// request's index in `f` (0xFF none).
#[derive(Clone, Copy)]
pub struct FAnn {
    pub s0: i32,
    pub s1: i32,
    pub kind: u8,
    pub fl: u8,
    pub len: u16,
    pub head: [u8; 8],
    pub req: u8,
}

/// Band modes.
pub const M_NONE: u8 = 0;
pub const M_BOXES: u8 = 1;
pub const M_BAR: u8 = 2;
/// The sample rate is too low for the bit rate (`Ann::note_baud`; 0: the
/// rate is not known, auto-baud saw bits too short).
pub const M_NOTE: u8 = 3;

/// One decoded screen. All zero = empty (a zeroed static is valid).
pub struct Ann {
    pub mode: u8,
    /// Window length (samples, the screen), sample rate, the window's first
    /// sample from the time zero (trigger point) in samples.
    pub n: u32,
    pub fs: u32,
    pub t0: i64,
    pub nb: u8,
    pub b: [BAnn; NB],
    pub nf: u8,
    pub f: [FAnn; NF],
    /// More bytes than NB on the screen.
    pub more: bool,
    /// Bar mode: one bit per graph column (the line toggles there).
    pub bar: [u8; 50],
    /// The bit rate and the middle level (code) used.
    pub baud: u32,
    pub note_baud: u32,
    pub mid: u16,
}

static mut ANN: Ann = unsafe { core::mem::zeroed() };

pub fn ann() -> &'static mut Ann {
    unsafe { &mut *core::ptr::addr_of_mut!(ANN) }
}

impl Ann {
    pub fn clear(&mut self) {
        self.mode = M_NONE;
        self.nb = 0;
        self.nf = 0;
        self.more = false;
        self.bar = [0; 50];
    }
    /// Graph x of a window sample (may lie off the graph).
    pub fn x_of(&self, s: i32) -> i32 {
        14 + (s as i64 * 399 / self.n.max(1) as i64) as i32
    }
}

/// The band's rect (the graph hole) for `dec` (1 UART, 2 Modbus).
pub fn band_rect(dec: u8) -> Rect {
    if dec == 2 { rect(14, FRAME_Y, 399, BYTE_Y + ROW_H - FRAME_Y) } else { rect(14, BYTE_Y, 399, ROW_H) }
}

const C_ERR: Color = 0xf800;
const C_BOX: Color = C_FRAME;
const C_FBOX: Color = C_SEL;
const C_BG: Color = 0x31a6;

fn txt(b: &[u8]) -> &str {
    core::str::from_utf8(b).unwrap_or("")
}

/// A text painted over its ink only, centred in `r` (on `bg`).
fn label_in(r: Rect, t: &str, fg: Color, bg: Color) {
    let k = Ink::new(sf::SC_F12, r, t, 8, 0);
    paint(isect(k.bx, r), &[(K_TEXT, bg)], |x, y| match k.at(x, y) {
        Some(g) => (fg, tag(1, g)),
        None => (bg, 0),
    });
}

/// The first of `cands` that fits `w` px with 3 px either side ("" none).
fn pick<'a>(cands: &[&'a str], w: i16) -> &'a str {
    cands.iter().copied().find(|t| !t.is_empty() && sf::text_width(sf::SC_F12, t, 0) + 6 <= w).unwrap_or("")
}

/// One box on a row: fill, outline (the sides only where the box ends on
/// the graph), a text.
fn draw_box(y: i16, x0: i32, x1: i32, edge: Color, t: &str, fg: Color) {
    let (a, b) = (x0.max(14), x1.min(412));
    if b - a < 2 {
        return;
    }
    let (a, b) = (a as u16, b as u16);
    let w = b - a + 1;
    lcd::fill_rect(a, y as u16, w, 1, edge);
    lcd::fill_rect(a, (y + ROW_H - 1) as u16, w, 1, edge);
    lcd::fill_rect(a, y as u16 + 1, w, ROW_H as u16 - 2, C_BG);
    if x0 >= 14 {
        lcd::fill_rect(a, y as u16, 1, ROW_H as u16, edge);
    }
    if x1 <= 412 {
        lcd::fill_rect(b, y as u16, 1, ROW_H as u16, edge);
    }
    if !t.is_empty() {
        label_in(rect(a as i16, y, w as i16, ROW_H), t, fg, C_BG);
    }
}

/// The bar of the columns where the line toggles (`a.bar`).
fn draw_bar(y: i16) {
    let a = ann();
    let mut i = 0;
    while i < 399 {
        if a.bar[i / 8] >> (i % 8) & 1 == 0 {
            i += 1;
            continue;
        }
        let s = i;
        while i < 399 && a.bar[i / 8] >> (i % 8) & 1 != 0 {
            i += 1;
        }
        lcd::fill_rect(14 + s as u16, (y + 5) as u16, (i - s) as u16, 7, C_BOX);
    }
}

/// The whole band (`dec` 1 / 2; `blank`: the background only, e.g. while
/// a drag moves the trace).
pub fn band(dec: u8, blank: bool) {
    let r = band_rect(dec);
    lcd::fill_rect(r.x as u16, r.y as u16, r.w as u16, r.h as u16, C_FILL);
    let a = ann();
    if blank || a.mode == M_NONE {
        return;
    }
    if a.mode == M_NOTE {
        let mut nb = [0u8; 32];
        let mut w = dc::W { b: &mut nb, n: 0 };
        if a.note_baud != 0 {
            w.s("Rate too low for ").u(a.note_baud).s(" baud");
        } else {
            w.s("Sample rate too low");
        }
        let n = w.n;
        // `label`: up to 48 glyphs (an Ink holds 24).
        sf::label(sf::SC_F12, rect(14, BYTE_Y, 399, ROW_H), txt(&nb[..n]), C_DIS, C_FILL, 8, 0);
        return;
    }
    if a.mode == M_BAR {
        draw_bar(BYTE_Y);
        if dec == 2 {
            draw_bar(FRAME_Y);
        }
        return;
    }
    let mut tb = [0u8; 8];
    for k in 0..a.nb as usize {
        let b = a.b[k];
        let (x0, x1) = (a.x_of(b.s0), a.x_of(b.s1));
        let w = (x1.min(412) - x0.max(14) + 1) as i16;
        let mut h = dc::W { b: &mut tb, n: 0 };
        h.hex(b.v);
        let hn = h.n;
        if (0x21..0x7F).contains(&b.v) {
            h.c(b' ').c(b.v);
        }
        let fn_ = h.n;
        let full = txt(&tb[..fn_]);
        let hex = txt(&tb[..hn]);
        let t = pick(&[full, hex, ".."], w);
        let (edge, fg) = if b.fl != 0 { (C_ERR, C_ERR) } else { (C_BOX, C_WHITE) };
        draw_box(BYTE_Y, x0, x1, edge, t, fg);
    }
    if dec != 2 {
        return;
    }
    for k in 0..a.nf as usize {
        let f = a.f[k];
        let (x0, x1) = (a.x_of(f.s0), a.x_of(f.s1));
        let w = (x1.min(412) - x0.max(14) + 1) as i16;
        let fr = dc::Fr { start: 0, end: 0, len: f.len, head: f.head, flags: f.fl, kind: f.kind };
        let mut lb = [0u8; 40];
        let mut l = dc::W { b: &mut lb, n: 0 };
        let mut sb = [0u8; 12];
        let mut s = dc::W { b: &mut sb, n: 0 };
        let (edge, fg) = if f.fl & (dc::F_CRC | dc::F_FE | dc::F_PE) != 0 {
            l.s("CRC ").hex(f.head[0]).c(b' ').hex(f.head[1]);
            s.s("CRC");
            (C_ERR, C_ERR)
        } else if f.fl & dc::F_PART != 0 {
            l.s("..");
            (C_BOX, C_DIS)
        } else {
            dc::mb_text(&mut l, &fr);
            // A reply whose request is on the screen: the reply time.
            if f.req != 0xFF && (f.req as usize) < a.nf as usize {
                let q = a.f[f.req as usize];
                let us = (f.s0 - q.s1).max(0) as u64 * 1_000_000 / a.fs.max(1) as u64;
                l.c(b' ').u((us / 1000) as u32).c(b'.').u((us / 100 % 10) as u32).s("ms");
            }
            s.s(match f.kind {
                dc::K_REQ => "REQ",
                dc::K_RSP => "RSP",
                dc::K_EXC => "EXC",
                _ => "?",
            });
            s.c(b' ').hex(f.head[0]);
            (C_FBOX, if f.kind == dc::K_EXC { C_ORANGE } else { C_YELLOW })
        };
        let (ln, sn) = (l.n, s.n);
        let short = txt(&sb[..sn]);
        let t = pick(&[txt(&lb[..ln]), short, ".."], w);
        draw_box(FRAME_Y, x0, x1, edge, t, fg);
    }
}

// ---- setup popup ----

/// Baud choices (0 = Auto) of the stepper.
pub const BAUDS: [u32; 14] = [0, 300, 600, 1200, 2400, 4800, 9600, 14400, 19200, 28800, 38400, 57600, 76800, 115_200];

#[derive(Clone, Copy, PartialEq)]
pub struct SetupVals {
    pub proto: u8,
    pub fmt: u8,
    pub inv: bool,
    pub baud: u32,
    /// The threshold set (NaN: auto) and the level in use (NaN: none yet).
    pub thr: f32,
    pub level: f32,
}

impl SetupVals {
    pub const DEFAULT: SetupVals = SetupVals { proto: 0, fmt: 0, inv: false, baud: 0, thr: f32::NAN, level: f32::NAN };
}

pub const SU_H: i16 = 180;
pub const SU_PROTO: Rect = rect(24, 81, 180, 24);
pub const SU_FMT: Rect = rect(214, 81, 176, 24);
pub const SU_BMINUS: Rect = rect(24, 125, 36, 24);
pub const SU_BVAL: Rect = rect(64, 125, 104, 24);
pub const SU_BPLUS: Rect = rect(172, 125, 36, 24);
pub const SU_POL: Rect = rect(214, 125, 176, 24);
pub const SU_TAUTO: Rect = rect(24, 169, 120, 24);
pub const SU_TMINUS: Rect = rect(214, 169, 36, 24);
pub const SU_TVAL: Rect = rect(254, 169, 104, 24);
pub const SU_TPLUS: Rect = rect(362, 169, 36, 24);
/// The setup page's labels.
pub const SU_LABELS: [Rect; 5] = [rect(24, 61, 189, 20), rect(214, 61, 189, 20), rect(24, 105, 189, 20), rect(214, 105, 189, 20), rect(24, 149, 189, 20)];

fn volts_text(w: &mut dc::W, v: f32) {
    let mut t = [0u8; 16];
    if v.is_finite() {
        w.s(crate::dmm::format(&mut t, v, 2)).s("V");
    } else {
        w.s("--");
    }
}

pub fn setup(v: &SetupVals) {
    let screen = rect(0, 0, 480, 320);
    fill_c(rect(13, 20, 401, SU_H + 1), screen, C_FRAME);
    // The header as the MF popup's selected tab.
    let r = rect(14, 21, 399, 40);
    fill_c(rect(14, 21, 399, 38), screen, C_POP_BG);
    fill_c(rect(14, 59, 399, 2), screen, C_POP_TAB);
    let t = Ink::new(sf::SC_F22, r, "Decoder", 8, 0);
    paint(isect(t.bx, r), &[(K_TEXT, C_POP_BG)], |x, y| match t.at(x, y) {
        Some(g) => (C_FILL, tag(1, g)),
        None => (C_POP_BG, 0),
    });
    let pc = rect(14, 61, 399, SU_H - 41);
    fill_c(pc, pc, C_POP_BG);
    for (r, t) in SU_LABELS.iter().zip(["Protocol:", "Format:", "Baud:", "Polarity:", "Threshold:"]) {
        let k = Ink::new(sf::SC_F16, *r, t, 1, 0);
        paint(isect(isect(k.bx, *r), pc), &[(K_TEXT, C_POP_BG)], |x, y| match k.at(x, y) {
            Some(g) => (C_FILL, tag(1, g)),
            None => (C_POP_BG, 0),
        });
    }
    option_list(SU_PROTO, pc, &["Off", "UART", "Modbus"], 60, v.proto.min(2) as usize);
    option_list(SU_FMT, pc, &dc::FMT_NAMES, 44, v.fmt as usize % 4);
    option_list(SU_POL, pc, &["Normal", "Inverted"], 88, v.inv as usize);
    option_list(SU_TAUTO, pc, &["Auto", "Manual"], 60, v.thr.is_finite() as usize);
    let mut bb = [0u8; 12];
    let mut b = dc::W { b: &mut bb, n: 0 };
    if v.baud == 0 {
        b.s("Auto");
    } else {
        b.u(v.baud);
    }
    let bn = b.n;
    let mut hb = [0u8; 16];
    let mut h = dc::W { b: &mut hb, n: 0 };
    volts_text(&mut h, if v.thr.is_finite() { v.thr } else { v.level });
    let hn = h.n;
    for (m, val, p, txt) in [(SU_BMINUS, SU_BVAL, SU_BPLUS, txt(&bb[..bn])), (SU_TMINUS, SU_TVAL, SU_TPLUS, txt(&hb[..hn]))] {
        rbox_text(m, pc, 8, C_BAR, C_POP_BG, sf::SC_F16, "-", C_WHITE);
        rbox_text(p, pc, 8, C_BAR, C_POP_BG, sf::SC_F16, "+", C_WHITE);
        fill_text(val, pc, C_POP_BG, sf::SC_F16, val, txt, C_FILL, 8, 0);
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SetupHit {
    Proto(u8),
    Fmt(u8),
    Baud(bool),
    Inv(bool),
    ThrAuto(bool),
    Thr(bool),
    Inside,
    /// Below the box: closes.
    Close,
    Outside,
}

pub fn setup_hit(x: i16, y: i16) -> SetupHit {
    if !rect(13, 20, 401, SU_H + 1).has(x, y) {
        return if y > 20 + SU_H && x < 414 { SetupHit::Close } else { SetupHit::Outside };
    }
    let near = |r: Rect| rect(r.x - 2, r.y - 3, r.w + 4, r.h + 6).has(x, y);
    if let Some(i) = option_hit(SU_PROTO, 3, 60, x, y) {
        SetupHit::Proto(i)
    } else if let Some(i) = option_hit(SU_FMT, 4, 44, x, y) {
        SetupHit::Fmt(i)
    } else if let Some(i) = option_hit(SU_POL, 2, 88, x, y) {
        SetupHit::Inv(i == 1)
    } else if let Some(i) = option_hit(SU_TAUTO, 2, 60, x, y) {
        SetupHit::ThrAuto(i == 0)
    } else if near(SU_BMINUS) {
        SetupHit::Baud(false)
    } else if near(SU_BPLUS) {
        SetupHit::Baud(true)
    } else if near(SU_TMINUS) {
        SetupHit::Thr(false)
    } else if near(SU_TPLUS) {
        SetupHit::Thr(true)
    } else {
        SetupHit::Inside
    }
}
