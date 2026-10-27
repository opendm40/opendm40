//! The protocol decoder view (View::Decode; src/app_decode.rs, src/decode.rs),
//! drawn in the scope page's look (a src/scope_ui.rs submodule: its
//! badges, panel buttons, measurement cells and the MF popup's controls).
//!
//!   0..20     top bar: run state (RUN / STOP / WAIT), protocol, bit rate
//!             and format, the threshold, INV (inverted polarity), battery
//!   20..301   the frame list in the graph frame: ROWS rows of ROW_H px,
//!             time (s since the first frame) | hex + ASCII or the Modbus
//!             fields | OK / FE / PE / CRC / OVR
//!   301..320  counters: frames, framing / parity errors, CRC errors or
//!             bytes, overruns
//!   panel     page up / LIVE (follow the newest) / page down; SETUP,
//!             CLEAR, STOP / RUN; SCOPE (back to the scope page)
//! The setup popup (MF key, SETUP) has the MF popup's frame and controls.
//! Region B: the view is drawn while the stream runs.

use super::*;
use crate::decode as dc;

pub const ROWS: usize = 16;
pub const ROW_H: i16 = 17;
const ROW_Y0: i16 = 22;
/// Row columns: time (right-aligned), content (UART: the hex, then the
/// ASCII at COL_A), status (centred).
const COL_T: (i16, i16) = (17, 74);
const COL_C: (i16, i16) = (107, 260);
const COL_A: (i16, i16) = (290, 78);
const COL_S: (i16, i16) = (373, 36);
/// The list's area inside the frame (taps / drags).
pub const LIST: Rect = rect(14, 21, 399, 279);

const C_OK: Color = 0x07e0;
const C_BAD: Color = 0xf800;
const C_GREY: Color = 0x9cd3;

/// One list row (texts formatted by the app).
pub struct Row<'a> {
    pub time: &'a str,
    pub text: &'a str,
    /// UART: the ASCII column ("" for Modbus / a note).
    pub ascii: &'a str,
    pub status: &'a str,
    /// Text colour (white; orange for a Modbus exception).
    pub fg: Color,
}

/// Top-bar values.
pub struct Top<'a> {
    /// 0 WAIT (no level / bit rate yet), 1 RUN, 2 STOP (paused).
    pub state: u8,
    pub proto: &'a str,
    pub rate: &'a str,
    pub thr: &'a str,
    pub inv: bool,
    pub battery: u8,
    pub charging: bool,
}

const PROTO_BADGE: Rect = rect(79, 1, 62, 17);
const RATE_BADGE: Rect = rect(145, 1, 120, 17);
const THR_BADGE: Rect = rect(269, 1, 79, 17);
const INV_BADGE: Rect = rect(352, 1, 62, 17);

/// Page background: bars, panel, the list frame and an empty list.
pub fn chrome() {
    lcd::fill_rect(0, 0, 480, 20, C_BAR);
    lcd::fill_rect(414, 20, 66, 300, C_BAR);
    lcd::fill_rect(0, 301, 414, 19, C_BAR);
    lcd::fill_rect(0, 20, 13, 281, C_BAR);
    lcd::fill_rect(13, 20, 401, 1, C_FRAME);
    lcd::fill_rect(13, 300, 401, 1, C_FRAME);
    lcd::fill_rect(13, 20, 1, 281, C_FRAME);
    lcd::fill_rect(413, 20, 1, 281, C_FRAME);
    lcd::fill_rect(14, 21, 399, 279, C_FILL);
}

pub fn top(t: &Top) {
    for k in 0..TOP_ITEMS {
        top_item(k, t);
    }
}

/// Top-bar items (badges, battery): drawn one at a time (`top_item`), a
/// badge paint is a few ms.
pub const TOP_ITEMS: usize = 6;

/// A key of what top-bar item k shows (redraw when it changes).
pub fn top_key(k: usize, t: &Top) -> u32 {
    let h = |s: &str| s.bytes().fold(0x811c_9dc5u32, |a, b| (a ^ b as u32).wrapping_mul(0x0100_0193));
    match k {
        0 => t.state as u32,
        1 => h(t.proto),
        2 => h(t.rate),
        3 => h(t.thr),
        4 => t.inv as u32,
        _ => t.battery as u32 | (t.charging as u32) << 8,
    }
}

pub fn top_item(k: usize, t: &Top) {
    match k {
        0 => {
            let r = RUN_BADGE;
            let st = ["WAIT", "RUN", "STOP"][t.state.min(2) as usize];
            let i = Ink::new(sf::SC_C, r, st, 8, 0);
            badge(r, if t.state == 2 { C_ORANGE } else { C_PURPLE }, C_BAR, None, &[(&i, C_WHITE)]);
        }
        1 => {
            let p = Ink::new(sf::SC_F12, PROTO_BADGE, t.proto, 8, 0);
            badge(PROTO_BADGE, C_MAGENTA, C_BAR, None, &[(&p, C_WHITE)]);
        }
        2 => {
            let q = Ink::new(sf::SC_F12, RATE_BADGE, t.rate, 8, 0);
            badge(RATE_BADGE, C_YELLOW, C_BAR, None, &[(&q, C_FILL)]);
        }
        3 => {
            let h = Ink::new(sf::SC_F12, THR_BADGE, t.thr, 8, 0);
            badge(THR_BADGE, C_FRAME, C_BAR, Some((rect(THR_BADGE.x + 1, 2, THR_BADGE.w - 2, 15), C_BAR)), &[(&h, C_YELLOW)]);
        }
        4 => {
            if t.inv {
                let v = Ink::new(sf::SC_F12, INV_BADGE, "INV", 8, 0);
                badge(INV_BADGE, C_ORANGE, C_BAR, None, &[(&v, C_WHITE)]);
            } else {
                lcd::fill_rect(INV_BADGE.x as u16, INV_BADGE.y as u16, INV_BADGE.w as u16, INV_BADGE.h as u16, C_BAR);
            }
        }
        _ => battery_lv(t.battery, t.charging),
    }
}

/// Panel button labels; 0 / 2 are arrows.
const PANEL_TXT: [&str; 7] = ["", "LIVE", "", "SETUP", "CLEAR", "STOP", "SCOPE"];

/// Right-panel button k (0..6): the scope page's groups (yellow: up / LIVE
/// / down; grey: SETUP / CLEAR / STOP-RUN; orange: SCOPE). `live`: the
/// list follows the newest frame (LIVE filled); `paused`: RUN instead of
/// STOP; `pressed`: k drawn pressed.
pub fn panel_button(k: usize, live: bool, paused: bool, pressed: bool) {
    let r = B_RECT[k];
    let (fill, radius, mask, fg) = match k {
        0 => (C_YELLOW, 8, 0x3, C_FILL),
        1 => (if live { C_YELLOW } else { C_FRAME }, 0, 0, C_FILL),
        2 => (C_YELLOW, 8, 0xc, C_FILL),
        3 => (C_FRAME, 8, 0x3, C_WHITE),
        4 => (C_FRAME, 0, 0, C_WHITE),
        5 => (C_FRAME, 8, 0xc, C_WHITE),
        _ => (C_ORANGE, 8, 0xf, C_WHITE),
    };
    let fill = if pressed { C_DIS } else { fill };
    let txt = if k == 5 && paused { "RUN" } else { PANEL_TXT[k] };
    let t = Ink::new(sf::SC_F16, r, txt, 8, 0);
    let arrow = match k {
        0 => Some(true),
        2 => Some(false),
        _ => None,
    };
    paint(r, &[(K_SHAPE, C_BAR), (K_TEXT, fill), (K_ICON, fill)], |x, y| {
        if let Some(up) = arrow {
            if tri(r, up, x, y) {
                return (fg, tag(3, 0));
            }
        }
        if let Some(g) = t.at(x, y) {
            return (fg, tag(2, g));
        }
        if mask == 0 || in_rbox(r, radius, mask, x, y) { (fill, tag(1, 0)) } else { (C_BAR, 0) }
    });
}

/// A filled triangle (16 x 9) centred in `r`, pointing up or down.
fn tri(r: Rect, up: bool, x: i16, y: i16) -> bool {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    let dy = if up { y - (cy - 4) } else { (cy + 4) - y };
    (0..9).contains(&dy) && (x - cx).abs() <= dy
}

pub fn panel(live: bool, paused: bool) {
    for k in 0..7 {
        panel_button(k, live, paused, false);
    }
}

/// Panel button under (x, y).
pub fn panel_hit(x: u16, y: u16) -> Option<usize> {
    let (x, y) = (x as i16, y as i16);
    if x < PANEL.x {
        return None;
    }
    B_RECT.iter().position(|r| y >= r.y - 1 && y < r.y + r.h + 1)
}

/// Row i of the list: a frame, or a grey note (`r.status` empty), or
/// nothing (None: the row cleared).
pub fn row(i: usize, r: Option<&Row>) {
    let y = ROW_Y0 + i as i16 * ROW_H;
    let Some(r) = r else {
        lcd::fill_rect(14, y as u16, 399, ROW_H as u16, C_FILL);
        return;
    };
    lcd::fill_rect(14, y as u16, COL_T.0 as u16 - 14, ROW_H as u16, C_FILL);
    sf::label(sf::SC_F12, rect(COL_T.0, y, COL_T.1, ROW_H), r.time, C_YELLOW, C_FILL, 4, 0);
    let gap = COL_C.0 - COL_T.0 - COL_T.1;
    lcd::fill_rect((COL_T.0 + COL_T.1) as u16, y as u16, gap as u16, ROW_H as u16, C_FILL);
    let note = r.status.is_empty();
    let w = if note { 413 - COL_C.0 } else if r.ascii.is_empty() { COL_C.1 } else { COL_A.0 - COL_C.0 };
    // Spacing 1 (Modbus fields, notes): some glyph pairs of the 12 px
    // font touch at 0.
    let sp = if r.ascii.is_empty() { 1 } else { 0 };
    sf::label(sf::SC_F12, rect(COL_C.0, y, w, ROW_H), r.text, if note { C_GREY } else { r.fg }, C_FILL, 1, sp);
    if !note {
        let mut e = COL_C.0 + w;
        if !r.ascii.is_empty() {
            // Spacing 1: the wide letters (M, W) of the 12 px font touch.
            sf::label(sf::SC_F12, rect(COL_A.0, y, COL_A.1, ROW_H), r.ascii, C_GREY, C_FILL, 1, 1);
            e = COL_A.0 + COL_A.1;
        }
        let g = COL_S.0 - e;
        lcd::fill_rect(e as u16, y as u16, g as u16, ROW_H as u16, C_FILL);
        let c = if r.status == "OK" { C_OK } else { C_BAD };
        sf::label(sf::SC_F12, rect(COL_S.0, y, COL_S.1, ROW_H), r.status, c, C_FILL, 8, 0);
        let e = COL_S.0 + COL_S.1;
        lcd::fill_rect(e as u16, y as u16, (413 - e) as u16, ROW_H as u16, C_FILL);
    }
}

/// Fit `s` into the content column (`note`: the full width): the longest
/// prefix that fits, with ">" at its end when cut. Returns its length.
pub fn fit(s: &str, note: bool, out: &mut [u8; 64]) -> usize {
    let w = if note { 413 - COL_C.0 - 4 } else { COL_C.1 - 2 };
    let b = s.as_bytes();
    let mut n = b.len().min(out.len());
    out[..n].copy_from_slice(&b[..n]);
    if sf::text_width(sf::SC_F12, txt(&out[..n]), 0) <= w {
        return n;
    }
    while n > 1 {
        n -= 1;
        out[n] = b'>';
        if sf::text_width(sf::SC_F12, txt(&out[..n + 1]), 0) <= w {
            return n + 1;
        }
    }
    n
}

fn txt(b: &[u8]) -> &str {
    core::str::from_utf8(b).unwrap_or("")
}

/// List row i showing frame `seq` of the store: time since `base_us`,
/// UART hex + ASCII / the Modbus fields (decode::mb_text, the reply time),
/// the status.
pub fn frame_row(i: usize, st: &dc::Store, seq: u32, base_us: u64) {
    let Some(h) = st.get(seq) else {
        row(i, None);
        return;
    };
    let h = *h;
    let mut tb = [0u8; 16];
    let mut w = dc::W { b: &mut tb, n: 0 };
    let rel = h.t_us.saturating_sub(base_us);
    let dec = if rel < 100_000_000 { 4 } else if rel < 10_000_000_000 { 2 } else { 0 };
    w.secs(rel, dec);
    let tn = w.n;
    let mut cb = [0u8; 64];
    let mut c = dc::W { b: &mut cb, n: 0 };
    // The first bytes (as many as a row can show).
    let mut d = [0u8; 24];
    let n = st.copy(&h, &mut d);
    let mut ab = [0u8; dc::UART_MAX];
    if h.kind == dc::K_UART || h.kind == dc::K_MB {
        // UART: hex + ASCII; a Modbus frame without fields: its hex.
        for (k, &b) in d[..n].iter().enumerate() {
            if k > 0 {
                c.c(b' ');
            }
            c.hex(b);
            if k < ab.len() {
                ab[k] = if (0x20..0x7F).contains(&b) { b } else { b'.' };
            }
        }
    } else {
        dc::mb_text(&mut c, &d[..n], &h);
        if h.kind == dc::K_RSP && h.req != 0 {
            c.s(" ").u(h.lat_us / 1000).c(b'.').u(h.lat_us / 100 % 10).s("ms");
        }
        // A read reply: its registers / coil bytes.
        if h.kind == dc::K_RSP && (1..=4).contains(&d[1]) {
            let words = d[1] >= 3;
            let end = n.min((h.len as usize).saturating_sub(2));
            let mut k = 3;
            while k < end {
                c.c(b' ');
                if words && k + 1 < end {
                    c.hex(d[k]).hex(d[k + 1]);
                    k += 2;
                } else {
                    c.hex(d[k]);
                    k += 1;
                }
            }
        }
    }
    let cn = c.n;
    let mut fb = [0u8; 64];
    let fnn = fit(txt(&cb[..cn]), false, &mut fb);
    let fg = if h.kind == dc::K_EXC { C_ORANGE } else { C_WHITE };
    let an = if h.kind == dc::K_UART { n.min(ab.len()) } else { 0 };
    row(i, Some(&Row { time: txt(&tb[..tn]), text: txt(&fb[..fnn]), ascii: txt(&ab[..an]), status: dc::status(&h), fg }));
}

/// `v` with at most 5 characters: 12345, 123k, 12M.
fn short(w: &mut dc::W, v: u32) {
    if v < 100_000 {
        w.u(v);
    } else if v < 10_000_000 {
        w.u(v / 1000).c(b'k');
    } else {
        w.u(v / 1_000_000).c(b'M');
    }
}

/// Counter cell i (0..3): Frames, FE, PE (UART) / CRC (Modbus), Lost, of
/// v = frames, framing errors, parity errors, CRC errors, lost (laps),
/// Modbus.
pub fn cell_draw(i: usize, v: &[u32; 6]) {
    let mut b = [0u8; 24];
    let mut w = dc::W { b: &mut b, n: 0 };
    let (label, x) = match i {
        0 => ("Frames:", v[0]),
        1 => ("FE:", v[1]),
        2 if v[5] != 0 => ("CRC:", v[3]),
        2 => ("PE:", v[2]),
        _ => ("Lost:", v[4]),
    };
    w.s(label);
    short(&mut w, x);
    let n = w.n;
    cell(i, txt(&b[..n]));
}

/// What cell i shows (redraw when it changes).
pub fn cell_key(i: usize, v: &[u32; 6]) -> u32 {
    match i {
        0 => v[0],
        1 => v[1],
        2 if v[5] != 0 => v[3] | 1 << 31,
        2 => v[2] & !(1 << 31),
        _ => v[4],
    }
}

/// Every counter cell.
pub fn cells(v: &[u32; 6]) {
    for i in 0..4 {
        cell_draw(i, v);
    }
}

/// Counter cells (as the scope's measurement row).
pub fn cell(i: usize, text: &str) {
    let r = rect(13 + 100 * i as i16, 302, 98, 17);
    let t = Ink::new(sf::SC_F12, r, text, 8, 0);
    paint(r, &[(K_SHAPE, C_BAR), (K_TEXT, C_BAR)], |x, y| {
        if let Some(g) = t.at(x, y) {
            return (C_YELLOW, tag(2, g));
        }
        if on_ring(r, 5, 0x0f, x, y) { (C_FRAME, tag(1, 0)) } else { (C_BAR, 0) }
    });
}

// ---- setup popup ----

/// Baud choices (0 = Auto) of the stepper.
pub const BAUDS: [u32; 14] = [0, 300, 600, 1200, 2400, 4800, 9600, 14400, 19200, 28800, 38400, 57600, 76800, 115_200];

pub struct SetupVals<'a> {
    pub modbus: bool,
    pub fmt: u8,
    pub inv: bool,
    /// The baud stepper's text ("Auto", "9600").
    pub baud: &'a str,
    pub thr_auto: bool,
    /// The threshold stepper's text ("1.65V").
    pub thr: &'a str,
}

pub const SU_H: i16 = 180;
pub const SU_PROTO: Rect = rect(24, 81, 120, 24);
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

pub fn setup(v: &SetupVals) {
    let screen = rect(0, 0, 480, 320);
    fill_c(rect(13, 20, 401, SU_H + 1), screen, C_FRAME);
    let r = rect(14, 21, 399, 40);
    let t = Ink::new(sf::SC_F22, r, "Decoder", 8, 0);
    let ul = rect(14, 59, 399, 2);
    paint(r, &[(K_TEXT, C_POP_BG), (K_SHAPE, C_FRAME)], |x, y| {
        if let Some(g) = t.at(x, y) {
            return (C_FILL, tag(1, g));
        }
        (if ul.has(x, y) { C_POP_TAB } else { C_POP_BG }, tag(2, 0))
    });
    let pc = rect(14, 61, 399, SU_H - 41);
    fill_c(pc, pc, C_POP_BG);
    for (r, t) in SU_LABELS.iter().zip(["Protocol:", "Format:", "Baud:", "Polarity:", "Threshold:"]) {
        let r = *r;
        fill_text(r, pc, C_POP_BG, sf::SC_F16, r, t, C_FILL, 1, 0);
    }
    option_list(SU_PROTO, pc, &["UART", "Modbus"], 60, v.modbus as usize);
    option_list(SU_FMT, pc, &crate::decode::FMT_NAMES, 44, v.fmt as usize);
    option_list(SU_POL, pc, &["Normal", "Inverted"], 88, v.inv as usize);
    option_list(SU_TAUTO, pc, &["Auto", "Manual"], 60, !v.thr_auto as usize);
    setup_values(v);
}

/// The two steppers (repainted on a step).
pub fn setup_values(v: &SetupVals) {
    let pc = rect(14, 61, 399, SU_H - 41);
    for (m, val, p, txt) in [(SU_BMINUS, SU_BVAL, SU_BPLUS, v.baud), (SU_TMINUS, SU_TVAL, SU_TPLUS, v.thr)] {
        rbox_text(m, pc, 8, C_BAR, C_POP_BG, sf::SC_F16, "-", C_WHITE);
        rbox_text(p, pc, 8, C_BAR, C_POP_BG, sf::SC_F16, "+", C_WHITE);
        fill_text(val, pc, C_POP_BG, sf::SC_F16, val, txt, C_FILL, 8, 0);
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SetupHit {
    Modbus(bool),
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
    if let Some(i) = option_hit(SU_PROTO, 2, 60, x, y) {
        SetupHit::Modbus(i == 1)
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
