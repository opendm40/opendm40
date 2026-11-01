//! MOCKUP (owner 2026-11-02, branch mock/scope-strip4, not for merge): the
//! scope page with a wider right strip of four channel blocks (CH1, DMM,
//! GEN, MATH: a title "CH1 1V" over a "+" and a "-" button) and no TRIG
//! button.
//!   tools/ui-render/run.sh OUT --strip4
//! Every state is drawn twice: the current page (the firmware's scope_ui,
//! unchanged) and the mock. The mock is the same firmware draw, then
//! narrowed here on the host: the graph goes from 10 x 40 px to 10 x 37 px
//! divisions (frame x 13..383, the strip x 384..479, 96 px). The grid and
//! traces lose 3 of every 40 columns (offsets 4, 20, 36 of a division, no
//! grid / tick column); every probed item inside the graph (markers, tabs,
//! FFT boxes) is moved whole, not squeezed: right-edge items by -30 px,
//! the others to their column's new place. The measurement cells and the
//! strip blocks are drawn here with the firmware's primitives
//! (stockfont paint / Ink / in_rbox, the stock fonts and colours), so the
//! probes see them and the layout rules apply.
use super::*;
use crate::scope_ui::{self as su, Model};
use crate::stockfont::{self as sf, in_rbox, on_ring, paint, rect, Ink, Rect};

/// Division width of the narrowed graph (px).
pub const D: i32 = 37;
/// Right frame column (old 413) and the strip's first column.
pub const FR: i32 = 13 + 10 * D;
pub const PX: i32 = FR + 1;
const OLD_FR: i32 = 413;
/// Strip blocks: x 387..475 (3 px inside the strip, 4 px from the screen
/// edge), title 26 px, 3 px gap, buttons 43 x 40 with a 3 px gap; blocks
/// 74 px apart from y 24 (5 px between blocks).
pub const BX: i16 = PX as i16 + 3;
pub const BW: i16 = 89;
pub const TITLE_H: i16 = 26;
pub const BTN_W: i16 = 43;
pub const BTN_H: i16 = 40;
pub const GAP: i16 = 3;
pub const PITCH: i16 = 74;
pub const Y0: i16 = 24;

/// Old graph column of new column `nx` (14..FR-1).
fn src_col(nx: i32) -> i32 {
    let f = nx - 13;
    let (div, k) = (f / D, f % D);
    let mut o = k;
    for drop in [4, 20, 36] {
        if o >= drop {
            o += 1;
        }
    }
    13 + div * 40 + o
}

/// New column of old column `x` (graph interior).
fn fwd(x: i32) -> i32 {
    13 + ((x - 13) * D + 20) / 40
}

/// Narrow the graph of the screen just drawn (frame top `top`), clear the
/// old strip and measurement bar.
fn narrow(top: i32) {
    let e = emu();
    let (fb0, own0) = (e.fb.clone(), e.owner.clone());
    let w = host::W;
    let interior = |x: i32, y: i32| x >= 14 && x <= OLD_FR - 1 && y > top && y < 300;
    // Overlay items (probed calls) inside the graph: their bounding boxes.
    let mut boxes: std::collections::BTreeMap<u32, (i32, i32)> = Default::default();
    for y in 0..host::H as i32 {
        for x in 0..w as i32 {
            let o = own0[y as usize * w + x as usize];
            if o != 0 && interior(x, y) {
                let b = boxes.entry(o >> 8).or_insert((x, x));
                b.0 = b.0.min(x);
                b.1 = b.1.max(x);
            }
        }
    }
    for y in top..=300 {
        for nx in 14..=FR {
            let sx = if nx == FR { OLD_FR } else { src_col(nx) };
            let (s, d) = (y as usize * w + sx as usize, y as usize * w + nx as usize);
            if own0[s] != 0 && interior(sx, y) {
                e.fb[d] = su::C_FILL;
                e.owner[d] = 0;
            } else {
                e.fb[d] = fb0[s];
                e.owner[d] = own0[s];
            }
        }
    }
    for y in 20..host::H {
        for x in PX as usize..w {
            e.fb[y * w + x] = su::C_BAR;
            e.owner[y * w + x] = 0;
        }
    }
    for y in 301..host::H {
        for x in 0..PX as usize {
            e.fb[y * w + x] = su::C_BAR;
            e.owner[y * w + x] = 0;
        }
    }
    // Move the overlay items whole, in draw order.
    for (&id, &(x0, x1)) in &boxes {
        let dx = if x1 >= 398 {
            FR - OLD_FR
        } else if x0 <= 16 {
            0
        } else {
            let c = (x0 + x1) / 2;
            fwd(c) - c
        };
        for y in top..=300 {
            for x in 14..OLD_FR {
                let s = y as usize * w + x as usize;
                if own0[s] >> 8 == id {
                    let nx = (x + dx).clamp(14, FR - 1) as usize;
                    e.fb[y as usize * w + nx] = fb0[s];
                    e.owner[y as usize * w + nx] = own0[s];
                }
            }
        }
    }
}

/// Measurement cell i of the narrowed bar: 4 cells of 90 px, 2 px apart.
fn meas_cell(m: &Model, i: usize, text: &str) {
    let r = rect(13 + 92 * i as i16, 302, 90, 17);
    let t = Ink::new(sf::SC_F12, r, text, 8, 0);
    let pal = m.pal_text();
    paint(r, &[(1, su::C_BAR), (0, su::C_BAR)], |x, y| {
        if let Some(g) = t.at(x, y) {
            return (pal, 2 << 8 | g as u16);
        }
        if on_ring(r, 5, 0x0f, x, y) { (su::C_FRAME, 1 << 8) } else { (su::C_BAR, 0) }
    });
}

/// One channel block.
pub struct Blk {
    pub name: &'static str,
    pub scale: String,
    pub col: u16,
    pub ink: u16,
    pub on: bool,
}

fn dim(c: u16) -> u16 {
    su::blend565(c, su::C_BAR, 80)
}

/// A rounded box with one centred text.
fn button(r: Rect, mask: u8, fill: u16, fg: u16, font: sf::Font, text: &str) {
    let t = Ink::new(font, r, text, 8, 0);
    paint(r, &[(1, su::C_BAR), (0, fill)], |x, y| {
        if let Some(g) = t.at(x, y) {
            return (fg, 2 << 8 | g as u16);
        }
        if in_rbox(r, 8, mask, x, y) { (fill, 1 << 8) } else { (su::C_BAR, 0) }
    });
}

/// Block k (0..3): the title (rounded top) over "+" (bottom-left) and "-"
/// (bottom-right). An off channel: its colour at 30 % over the strip, grey
/// text.
pub fn block(k: usize, b: &Blk) {
    let y = Y0 + PITCH * k as i16;
    let (fill, fg) = if b.on { (b.col, b.ink) } else { (dim(b.col), su::C_FRAME) };
    let title = format!("{} {}", b.name, b.scale);
    let font = if sf::text_width(sf::SC_F16, &title, 0) <= BW - 8 { sf::SC_F16 } else { sf::SC_F12 };
    button(rect(BX, y, BW, TITLE_H), 0x3, fill, fg, font, &title);
    let by = y + TITLE_H + GAP;
    button(rect(BX, by, BTN_W, BTN_H), 0x4, fill, fg, sf::SC_F20, "+");
    button(rect(BX + BW - BTN_W, by, BTN_W, BTN_H), 0x8, fill, fg, sf::SC_F20, "-");
}

/// The mock's channels.
#[derive(Clone, Copy)]
struct Chans {
    dmm: bool,
    gen: bool,
    math: bool,
}

fn blocks(m: &Model, c: Chans) -> [Blk; 4] {
    let mut v = su::Buf::new();
    m.vdiv_str(&mut v);
    let ch1 = v.as_str().to_string();
    let math_scale = if !m.math_scale.as_str().is_empty() { m.math_scale.as_str().to_string() } else { ch1.clone() };
    let (math_name, mc, mi) = if m.math_mode == 5 { ("FFT", m.math_pal(), m.math_ink()) } else { ("MATH", m.math_pal(), m.math_ink()) };
    [
        Blk { name: "CH1", scale: ch1.clone(), col: m.pal(), ink: m.pal_ink(), on: true },
        Blk { name: "DMM", scale: "2V".into(), col: su::C_CH2, ink: su::C_WHITE, on: c.dmm },
        // GEN has no scale of its own today: CH1's.
        Blk { name: "GEN", scale: ch1, col: su::C_GEN_TR, ink: su::C_FILL, on: c.gen },
        Blk { name: math_name, scale: math_scale, col: mc, ink: mi, on: c.math },
    ]
}

/// The base scene (manual.rs scope_base): a 1 kHz 3 Vpp sine at 200 us/div,
/// 1 V/div, the timebase tabs and the CUR tab.
fn base() -> stock::Scene {
    let b = &stock::scenes()[0];
    let mut m = b.m;
    m.tabs = su::tabs_of(su::TAP_CORNERS, m.tb, su::tb_top(true, false), false, None);
    m.cur_tab = su::CUR_OFF;
    m.src = su::SRC_MCX;
    let mut sc = stock::Scene { name: "strip4", m, popup: su::Popup::None, trace: None, persist: None, cols: 399, cells: b.cells.clone() };
    sc.trace = Some(sine(&sc.m, 1.5, 2.0));
    sc
}

fn sine(m: &Model, amp: f32, periods: f32) -> stock::Trace {
    let s = ((140 + m.pos as i32 - m.trig_row as i32) as f32 / (amp * 35.0)).clamp(-1.0, 1.0);
    let th = s.asin() / core::f32::consts::TAU;
    let phase = th / periods - m.trig_px as f32 / 400.0;
    stock::Trace::sine(amp, periods, phase).shifted(-(m.pos as i32))
}

/// Draw the current page of `sc` with the extra traces of `c`.
fn draw(sc: &stock::Scene, c: Chans) {
    let mut pv = stock::popvals(&sc.m);
    pv.pressed_cell = None;
    pv.gen_trace = c.gen;
    pv.ch2_on = c.dmm;
    stock::draw_with(sc, &pv);
    let h = sc.m.frame_h();
    let g = su::graph();
    let shape = gen::Shape { wave: gen::Wave::Square, lo_v: 0.0, hi_v: 2.0, duty: 0.5 };
    for i in 0..399 {
        if c.gen {
            let (lo, hi) = shape.span(i as f32 * 4.0 / 399.0, 4.0 / 399.0);
            let r = |v: f32| (140.0 - 141.0 * v / 4.0) as i32;
            g.gn[i] = (su::trace_fy(h, r(hi)), su::trace_fy(h, r(lo)));
        }
        if c.dmm {
            // A slow DMM reading, 2 V/div, about +1.2 V.
            let v = 1.2 + 0.4 * (i as f32 / 399.0 * core::f32::consts::TAU).sin();
            let r = (140.0 - 141.0 * v / 8.0) as i32;
            g.c2[i] = (su::trace_fy(h, r), su::trace_fy(h, r + 1));
        }
        if c.math && sc.m.math_mode != 5 {
            // CH1 - GEN at 2 V/div, 1.5 div down (its offset marker); a
            // column spans its value and the next one (steps drawn).
            let v = |i: usize| {
                let t = (i as f32 / 400.0) * 2.0 * core::f32::consts::TAU;
                1.5 * t.sin() - if c.gen { shape.span(i as f32 * 4.0 / 399.0, 0.0).0 } else { 0.0 }
            };
            let r = |v: f32| (140.0 + 52.0 - 141.0 * v / 8.0) as i32;
            let (a, b) = (r(v(i)), r(v(i + 1)));
            g.ma[i] = (su::trace_fy(h, a.min(b)), su::trace_fy(h, a.max(b) + 1));
        }
    }
    su::update(&sc.m);
}

fn save1(out: &str, name: &str) {
    host::save_ppm(&format!("{out}/{name}.ppm"), 1);
}

pub fn run(out: &str) -> usize {
    std::fs::create_dir_all(out).unwrap();
    let mut ck = Checker { cases: 0, violations: 0, seen: Default::default() };
    let mut states: Vec<(&str, stock::Scene, Chans)> = vec![];
    let all = Chans { dmm: true, gen: true, math: true };
    let none = Chans { dmm: false, gen: false, math: false };
    let mut s = base();
    s.m.math_on = true;
    s.m.math_mode = 2;
    s.m.math_off = -52;
    s.m.math_scale = su::Txt::new("2V");
    states.push(("1-all-on", s, all));
    states.push(("2-ch1-only", base(), none));
    let mut s = base();
    s.m.status = su::ST_STOP;
    s.m.record = (0.45, 0.55);
    s.m.tb = 12;
    s.m.tabs = su::tabs_of(su::TAP_CORNERS, 12, su::tb_top(true, false), false, None);
    s.trace = Some(sine(&s.m, 1.5, 1.0));
    states.push(("3-stop", s, Chans { dmm: true, gen: false, math: false }));
    let mut s = base();
    s.m.cur_tab = su::CUR_Y;
    s.m.xcur = Some([120, 280]);
    s.m.ycur = Some([87, 193]);
    s.m.sel_v = 3;
    s.cells = vec!["X2-1:800us", "Y2-1:3.01V", "Cycl:1.00ms", "Vrms:1.06V"];
    states.push(("4-cursors", s, none));
    let mut s = base();
    s.name = "st11-fft"; // stock::math_trace: the FFT noise trace
    s.m.math_on = true;
    s.m.math_mode = 5;
    s.m.math_scale = su::Txt::new("20dB");
    s.m.fft_hz = su::Txt::new("5.00kHz");
    states.push(("5-fft", s, Chans { dmm: false, gen: false, math: true }));
    // The trigger level at the top right edge, selected (the right-edge
    // marker next to the strip).
    let mut s = base();
    s.m.sel_v = 1;
    s.m.guide = true;
    s.m.trig_row = 70;
    s.m.trig_v = 1.99;
    s.trace = Some(sine(&s.m, 1.5, 2.0));
    states.push(("6-trig-level", s, Chans { dmm: false, gen: true, math: false }));

    let strip = Bx::new(PX, 20, 480 - PX, 300);
    let meas = Bx::new(0, 301, PX, 19);
    for (name, sc, c) in &states {
        draw(sc, *c);
        ck.check_replica(&format!("strip4/{name}/current"));
        save1(out, &format!("{name}-a-current"));
        draw(sc, *c);
        narrow(sc.m.top() as i32);
        for (i, t) in sc.cells.iter().enumerate().take(4) {
            meas_cell(&sc.m, i, t);
        }
        for (k, b) in blocks(&sc.m, *c).iter().enumerate() {
            block(k, b);
        }
        ck.check_replica(&format!("strip4/{name}/new"));
        // The full rules for the new strip and cells, against the whole
        // screen (edge markers, tabs, FFT boxes included).
        ck.check_add(&format!("strip4/{name}/new"), &[("strip", strip), ("meas", meas), ("screen", Bx::new(0, 0, 480, 320))], &[strip], Bx::new(0, 0, 480, 320));
        save1(out, &format!("{name}-b-new"));
    }
    // Worst-case titles: which font each gets in the 89 px title.
    for t in ["CH1 500mV", "CH1 10kV", "DMM 250mA", "DMM 1.5V", "DMM 200mV", "GEN 500mV", "MATH 500mV", "MATH 10kV", "FFT 100mdB", "FFT 20dB"] {
        let w16 = sf::text_width(sf::SC_F16, t, 0);
        println!("title {t:12} F16 {w16} px -> {}", if w16 <= BW - 8 { "F16" } else { "F12" });
    }
    for (msg, (n, scene)) in &ck.seen {
        println!("{n:6} x  {msg}   [first: {scene}]");
    }
    println!("cases {}, violations {}", ck.cases, ck.violations);
    ck.violations
}
