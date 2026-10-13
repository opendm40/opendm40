//! Host renderer + layout check for the fw-rs screens (tools/ui-render/run.sh).
//!
//! The real drawing code (src/ui.rs, src/stockfont.rs with the embedded stock
//! fonts, src/lcd.rs) runs against an ILI9488 emulator (host.rs). Built with
//! `--cfg test`, so the `#[cfg(test)]` probe calls in stockfont.rs / ui.rs
//! report every text / icon / shape draw call; the emulator records, per
//! pixel, which call last put ink there (overpainted ink is gone). The check
//! then takes the final ink bounding box of every call and enforces the
//! layout rules (owner, 2026-10-13):
//!   * every text / icon / shape ink box is >= 3 px from every other one
//!     (gap = background pixels in between), unless one contains the other;
//!   * a box inside a shape (pill, button) or a screen region (bar, strip,
//!     panel, measurement bar, band) keeps >= 3 px to its edges, >= 4 px to
//!     the screen edges;
//!   * inside one text, consecutive glyphs do not touch;
//!   * an update of the meter strip leaves no stale pixels (equal to a fresh
//!     draw of the new state).
//! over a matrix of worst cases, and writes PNG renders of named scenes.
//!   main <out dir>      prints "cases N, violations M", exit 1 if M > 0
#![allow(dead_code, unused, static_mut_refs)]

mod host;
pub use host::mmio;

#[path = "../../src/clock.rs"]
mod clock;
#[path = "../../src/dialog.rs"]
mod dialog;
#[path = "../../src/dmm.rs"]
mod dmm;
#[path = "../../src/fonts.rs"]
mod fonts;
#[path = "../../src/gen.rs"]
mod gen;
#[path = "../../src/gesture.rs"]
mod gesture;
#[path = "../../src/hwtrig.rs"]
mod hwtrig;
#[path = "../../src/hy3131.rs"]
mod hy3131;
#[path = "../../src/hy3131_tables.rs"]
mod hy3131_tables;
#[path = "../../src/lcd.rs"]
mod lcd;
#[path = "../../src/meter.rs"]
mod meter;
#[path = "../../src/scope.rs"]
mod scope;
#[path = "../../src/scope_meas.rs"]
mod scope_meas;
#[path = "../../src/stockfont.rs"]
mod stockfont;
#[path = "../../src/text.rs"]
mod text;
#[path = "../../src/touch.rs"]
mod touch;
#[path = "../../src/ui.rs"]
mod ui;

/// Probe hooks called by the drawing code under cfg(test).
pub mod probe {
    pub const TEXT: u8 = 0;
    pub const SHAPE: u8 = 1;
    pub const ICON: u8 = 2;
    pub fn begin(kind: u8, bg: u16) {
        crate::host::begin(kind, bg)
    }
    pub fn sub(g: u8) {
        crate::host::sub(g)
    }
    pub fn end() {
        crate::host::end()
    }
    pub fn group(on: bool) {
        crate::host::group(on)
    }
}

use dmm::{Button, Reading, Val};
use host::{emu, H, W};

// ------------------------------------------------------------------ check

#[derive(Clone, Copy, Debug, PartialEq)]
struct Bx {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Bx {
    fn new(x: i32, y: i32, w: i32, h: i32) -> Bx {
        Bx { x0: x, y0: y, x1: x + w - 1, y1: y + h - 1 }
    }
    fn add(&mut self, x: i32, y: i32) {
        self.x0 = self.x0.min(x);
        self.y0 = self.y0.min(y);
        self.x1 = self.x1.max(x);
        self.y1 = self.y1.max(y);
    }
    fn contains(&self, b: &Bx) -> bool {
        b.x0 >= self.x0 && b.x1 <= self.x1 && b.y0 >= self.y0 && b.y1 <= self.y1
    }
    /// Background pixels between two disjoint boxes (negative = overlap).
    fn gap(&self, b: &Bx) -> i32 {
        (b.x0 - self.x1 - 1).max(self.x0 - b.x1 - 1).max(b.y0 - self.y1 - 1).max(self.y0 - b.y1 - 1)
    }
    /// Smallest inner margin of `b` inside self.
    fn margin(&self, b: &Bx) -> i32 {
        (b.x0 - self.x0).min(self.x1 - b.x1).min(b.y0 - self.y0).min(self.y1 - b.y1)
    }
    fn area(&self) -> i32 {
        (self.x1 - self.x0 + 1) * (self.y1 - self.y0 + 1)
    }
}

const EMPTY: Bx = Bx { x0: i32::MAX, y0: i32::MAX, x1: i32::MIN, y1: i32::MIN };

struct Item {
    id: u32,
    kind: u8,
    b: Bx,
    glyphs: Vec<(u8, Bx)>,
    color: u16,
}

fn kind_name(k: u8) -> &'static str {
    ["text", "shape", "icon"][k as usize]
}

fn items() -> Vec<Item> {
    let e = emu();
    let mut m: std::collections::BTreeMap<u32, Item> = Default::default();
    for y in 0..H {
        for x in 0..W {
            let o = e.owner[y * W + x];
            if o == 0 {
                continue;
            }
            let (id, g) = (o >> 8, (o & 255) as u8);
            let it = m.entry(id).or_insert_with(|| Item { id, kind: e.kinds[id as usize - 1], b: EMPTY, glyphs: vec![], color: e.fb[y * W + x] });
            it.b.add(x as i32, y as i32);
            match it.glyphs.iter_mut().find(|(k, _)| *k == g) {
                Some((_, b)) => b.add(x as i32, y as i32),
                None => {
                    let mut b = EMPTY;
                    b.add(x as i32, y as i32);
                    it.glyphs.push((g, b));
                }
            }
        }
    }
    m.into_values().collect()
}

struct Checker {
    cases: usize,
    violations: usize,
    /// Distinct messages (draw-call ids stripped): count, first scene.
    seen: std::collections::BTreeMap<String, (usize, String)>,
}

impl Checker {
    fn fail(&mut self, scene: &str, msg: String) {
        self.violations += 1;
        let key: String = msg.split(' ').filter(|w| !w.starts_with('#')).collect::<Vec<_>>().join(" ");
        let e = self.seen.entry(key).or_insert((0, scene.to_string()));
        e.0 += 1;
    }

    /// Check the emulator screen. `regions`: (name, box) screen areas that
    /// contain items (the screen itself is always one, margin 4).
    fn check(&mut self, scene: &str, regions: &[(&str, Bx)]) {
        self.check_opts(scene, regions, true)
    }

    /// `glyph_rule` false: skip "glyphs of one text do not touch" (the menu
    /// pages' stock TEXT22 rows are drawn at spacing 0, where that font's
    /// glyphs touch by design; gaps and margins are still checked).
    fn check_opts(&mut self, scene: &str, regions: &[(&str, Bx)], glyph_rule: bool) {
        self.cases += 1;
        let its = items();
        let screen = Bx::new(0, 0, W as i32, H as i32);
        let desc = |i: &Item| format!("{} #{} {:?}", kind_name(i.kind), i.id, i.b);
        // Container of each item: smallest shape, else smallest region.
        let mut parent: Vec<Option<usize>> = vec![None; its.len()];
        for (a, ia) in its.iter().enumerate() {
            let mut best: Option<(usize, i32)> = None;
            for (b, ib) in its.iter().enumerate() {
                if a != b && ib.kind == probe::SHAPE && ib.b.contains(&ia.b) && ib.b.area() > ia.b.area() {
                    if best.map_or(true, |(_, ar)| ib.b.area() < ar) {
                        best = Some((b, ib.b.area()));
                    }
                }
            }
            parent[a] = best.map(|x| x.0);
            if screen.margin(&ia.b) < 4 {
                self.fail(scene, format!("{} only {} px from the screen edge", desc(ia), screen.margin(&ia.b)));
            }
            match parent[a] {
                Some(p) => {
                    let m = its[p].b.margin(&ia.b);
                    if m < 3 {
                        self.fail(scene, format!("{} only {} px inside {}", desc(ia), m, desc(&its[p])));
                    }
                }
                None => {
                    let r = regions.iter().filter(|(_, r)| r.contains(&ia.b)).min_by_key(|(_, r)| r.area());
                    match r {
                        Some((name, r)) => {
                            let m = r.margin(&ia.b);
                            if m < 3 {
                                self.fail(scene, format!("{} only {} px inside region {} {:?}", desc(ia), m, name, r));
                            }
                        }
                        None => self.fail(scene, format!("{} not inside any region", desc(ia))),
                    }
                }
            }
            // Glyphs of one text must not touch (left to right).
            if ia.kind == probe::TEXT && ia.glyphs.len() > 1 {
                let mut g = ia.glyphs.clone();
                g.sort_by_key(|(k, _)| *k);
                // Widest gap between letters of one word.
                let letter = g.windows(2).filter(|w| w[1].0 == w[0].0 + 1).map(|w| w[1].1.x0 - w[0].1.x1 - 1).max().unwrap_or(1);
                for w in g.windows(2) {
                    let (a, b) = (w[0].1, w[1].1);
                    if glyph_rule && a.gap(&b) < 1 {
                        self.fail(scene, format!("{}: glyphs {} / {} touch ({:?} {:?})", desc(ia), w[0].0, w[1].0, a, b));
                    }
                    // Word gap (as tools/gallery-test): an ink-less glyph (a
                    // space) between two glyphs must leave >= 4 px and >= the
                    // widest letter gap + 2 ("40rows" with the 1 px F14 space).
                    let need = 4.max(letter + 2);
                    if w[1].0 > w[0].0 + 1 && b.x0 - a.x1 - 1 < need {
                        self.fail(scene, format!("{}: word gap after glyph {} only {} px (letters up to {})", desc(ia), w[0].0, b.x0 - a.x1 - 1, letter));
                    }
                }
            }
        }
        // Pairwise gaps (skip container relations).
        for a in 0..its.len() {
            for b in a + 1..its.len() {
                let (ia, ib) = (&its[a], &its[b]);
                if parent[a] == Some(b) || parent[b] == Some(a) {
                    continue;
                }
                // Nested deeper (text in a pill in a shape...): ancestor check.
                let anc = |mut x: usize, y: usize| {
                    while let Some(p) = parent[x] {
                        if p == y {
                            return true;
                        }
                        x = p;
                    }
                    false
                };
                if anc(a, b) || anc(b, a) {
                    continue;
                }
                let g = ia.b.gap(&ib.b);
                if g < 3 {
                    self.fail(scene, format!("{} and {} {} by {} px", desc(ia), desc(ib), if g < 0 { "overlap" } else { "too close" }, g));
                }
            }
        }
    }
}

// ------------------------------------------------------------------ scenes

fn val(v: f32, dec: u8, unit: &'static str) -> Val {
    Val { v, dec, unit, ol: false, ext: false }
}

fn ol(dec: u8, unit: &'static str) -> Val {
    Val { v: 0.0, dec, unit, ol: true, ext: false }
}

fn rd(mode: u8, main: Val, a1: Option<Val>, a2: Option<Val>, acdc: bool) -> Reading {
    let si = main.v * meter::unit_scale(main.unit);
    Reading { mode, range: 0, main, aux1: a1, aux2: a2, si, auto: true, auto_plus: false, acdc, seq: 0, avg_n: 0 }
}

/// Meter-strip worst cases: (name, reading, temperature label).
fn strip_cases() -> Vec<(&'static str, Reading, Option<&'static str>)> {
    let c = "\u{b0}C";
    let f = "\u{b0}F";
    let mut v = vec![
        ("dcv", rd(1, val(4.8176, 4, "V"), None, None, false), None),
        ("dcv_neg6", rd(1, val(-6.6497, 4, "V"), None, None, false), None),
        ("dcv_44", rd(1, val(-44.444, 3, "V"), None, None, false), None),
        ("dcv_600", rd(1, val(-444.44, 2, "V"), None, None, false), None),
        ("dcv_1000", rd(1, val(-1000.0, 1, "V"), None, None, false), None),
        ("dcmv", rd(3, val(-444.44, 2, "mV"), None, None, false), None),
        ("dcv_ol", rd(1, ol(4, "V"), None, None, false), None),
        ("acv_230", rd(0, val(230.12, 2, "V"), Some(val(50.000, 3, "Hz")), Some(val(50.0, 1, "%")), false), None),
        ("acv_khz", rd(0, val(4.4444, 4, "V"), Some(val(99.999, 3, "kHz")), Some(val(100.0, 1, "%")), false), None),
        ("acmv", rd(2, val(444.44, 2, "mV"), Some(val(9.9999, 4, "kHz")), Some(val(44.4, 1, "%")), false), None),
        ("acdc_mv", rd(3, val(-444.44, 2, "mV"), Some(val(444.44, 2, "mV")), Some(val(-444.44, 2, "mV")), true), None),
        ("acdc_600mv", rd(3, val(-600.00, 2, "mV"), Some(val(53.63, 2, "mV")), Some(val(-501.67, 2, "mV")), true), None),
        ("acdc_v", rd(1, val(-6.6666, 4, "V"), Some(val(4.4444, 4, "V")), Some(val(-4.4444, 4, "V")), true), None),
        ("acdc_ua", rd(7, val(-444.44, 2, "\u{b5}A"), Some(val(444.44, 2, "\u{b5}A")), Some(val(-444.44, 2, "\u{b5}A")), true), None),
        ("acdc_ol", rd(3, ol(2, "mV"), Some(ol(2, "mV")), Some(val(-444.44, 2, "mV")), true), None),
        ("dca", rd(9, val(-9.9999, 4, "A"), None, None, false), None),
        ("dcma_420", rd(8, val(24.444, 3, "mA"), None, Some(val(125.0, 1, "%")), false), None),
        ("dcma_420ol", rd(8, val(44.444, 3, "mA"), None, Some(ol(1, "%")), false), None),
        ("aca", rd(6, val(4.4444, 4, "A"), Some(val(99.999, 3, "kHz")), Some(val(100.0, 1, "%")), false), None),
        ("ohm_mohm", rd(10, val(44.444, 3, "M\u{3a9}"), None, None, false), None),
        ("ohm_ol", rd(10, ol(3, "M\u{3a9}"), None, None, false), None),
        ("ohm_k", rd(10, val(444.44, 2, "k\u{3a9}"), None, None, false), None),
        ("cont", rd(11, val(444.4, 1, "\u{3a9}"), None, None, false), None),
        ("diode", rd(12, val(4.4444, 4, "V"), None, Some(val(1040.0, 1, "\u{3a9}")), false), None),
        ("diode_ol", rd(12, ol(4, "V"), None, Some(ol(1, "\u{3a9}")), false), None),
        ("cap_mf", rd(13, val(44.444, 3, "mF"), None, None, false), None),
        ("cap_nf", rd(13, val(4.444, 3, "nF"), None, None, false), None),
        ("cap_uf", rd(13, val(444.4, 1, "\u{b5}F"), None, None, false), None),
        ("hz_mhz", rd(14, val(9.9999, 4, "MHz"), None, Some(val(100.0, 1, "%")), false), None),
        ("hz_khz", rd(14, val(99.999, 3, "kHz"), None, Some(val(44.4, 1, "%")), false), None),
        ("hz_ol", rd(14, ol(4, "MHz"), None, Some(ol(1, "%")), false), None),
        ("temp_tc", rd(15, val(-200.00, 2, c), Some(val(26.5, 1, c)), Some(val(-328.00, 2, f)), false), Some("TC K")),
        ("temp_tc_ol", rd(15, ol(2, c), Some(val(-44.4, 1, c)), Some(ol(2, f)), false), Some("TC K")),
        ("temp_ntc", rd(15, val(125.00, 2, c), Some(val(444.444, 3, "k\u{3a9}")), Some(val(257.00, 2, f)), false), Some("NTC100k B3950")),
        ("temp_pt", rd(15, val(-44.44, 2, c), Some(val(444.44, 2, "\u{3a9}")), Some(val(-44.44, 2, f)), false), Some("PT1000")),
        ("temp_ntcu", rd(15, val(4.44, 2, c), Some(ol(2, "M\u{3a9}")), Some(ol(2, f)), false), Some("NTC user")),
    ];
    // AUTO+ / MANU variants of a few.
    let mut extra = vec![];
    for (n, r, l) in &v[..3] {
        let mut a = *r;
        a.auto_plus = true;
        extra.push((*n, a, *l));
        let mut m = *r;
        m.auto = false;
        extra.push((*n, m, *l));
    }
    v.extend(extra);
    v
}

fn hv_of(r: &Reading) -> u8 {
    let v = if r.mode <= 3 && !r.main.ol { r.si.abs() } else { 0.0 };
    if v >= 100.0 { 2 } else if v >= 30.0 { 1 } else { 0 }
}

struct World {
    sc: scope::Scope,
    gen: gen::Gen,
    data: Vec<u16>,
}

fn status(page: u8) -> ui::Status {
    ui::Status { uptime_s: 99 * 3600 + 59 * 60 + 59, bt: true, bars: 5, charging: false, locked: false, rec: true, csv: true, page }
}

fn frame(w: &World, v: &ui::ScopeView, clip: u16) {
    let f = scope::Frame { data: &w.data, cols: None, trig: Some(1000), fs: 1_000_000, seq: 1 };
    let mut prev = [(u16::MAX, 0u16); 401];
    let cur = ui::Cursors { x_on: false, y_on: false, x: [100, 300], y: [70, 210] };
    v.frame(&f, &w.sc, &mut prev, &cur, false, false, clip);
}

fn meas_worst() -> (Vec<u8>, scope_meas::Results) {
    let mut res = scope_meas::Results { v: [f32::NAN; 20] };
    // Widest strings per unit class.
    for (id, _, u) in scope_meas::ITEMS.iter() {
        res.v[*id as usize] = match u {
            scope_meas::Unit::V => -444.4e-3,
            scope_meas::Unit::Hz => 444.4e3,
            scope_meas::Unit::S => 444.4e-6,
            scope_meas::Unit::Pct => 44.4,
            scope_meas::Unit::N => 4444.0,
        };
    }
    (vec![0, 10, 9, 11], res)
}

fn combined_regions(band: bool) -> Vec<(&'static str, Bx)> {
    let mut r = vec![
        ("screen", Bx::new(0, 0, 480, 320)),
        ("bar", Bx::new(0, 0, 480, ui::SBAR_H as i32)),
        ("strip", Bx::new(0, ui::STRIP_Y as i32, 480, ui::STRIP_H as i32)),
        ("panel", Bx::new(414, ui::SCOPE_COMBINED.y0 as i32, 66, ui::SCOPE_COMBINED.h as i32)),
        ("meas", Bx::new(0, 301, 480, 19)),
    ];
    if band {
        r.push(("band", Bx::new(0, ui::FUNC_Y as i32 + 1, 480, 301 - ui::FUNC_Y as i32 - 1)));
    }
    r
}

fn full_regions() -> Vec<(&'static str, Bx)> {
    vec![
        ("screen", Bx::new(0, 0, 480, 320)),
        ("bar", Bx::new(0, 0, 480, ui::SCOPE_FULL.y0 as i32)),
        ("panel", Bx::new(414, ui::SCOPE_FULL.y0 as i32, 66, ui::SCOPE_FULL.h as i32)),
        ("meas", Bx::new(0, 301, 480, 19)),
    ]
}

/// The combined view as the app draws it after a view switch + one reading.
fn combined(w: &World, dv: &mut ui::DmmView, r: &Reading, label: Option<&'static str>, hold: u8, lim: u8, rel: bool, alt: bool, band: bool, st: &ui::Status) {
    host::reset(0);
    dv.invalidate();
    dv.strip_static();
    dv.label = label;
    ui::SCOPE_COMBINED.draw_static(&w.sc);
    let (sel, res) = meas_worst();
    ui::scope_measurements(&sel, &res);
    if band {
        ui::func_band(r_button(r), r.mode, r.acdc);
    }
    frame(w, &ui::SCOPE_COMBINED, if band { ui::FUNC_Y } else { u16::MAX });
    ui::scope_status(st);
    ui::ScopeView::gen_chip(&w.gen);
    dv.strip(r, hold, lim, rel, hv_of(r), alt);
}

fn r_button(r: &Reading) -> Button {
    dmm::button_of(r.mode)
}

fn save(out: &str, name: &str) {
    host::save_ppm(&format!("{out}/{name}.ppm"), 2);
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    host::map_flash();
    let mut w = World {
        sc: scope::Scope::new(),
        gen: gen::Gen::new(),
        data: (0..2000).map(|i| if (i / 250) % 2 == 0 { 700 } else { 300 }).collect(),
    };
    let mut ck = Checker { cases: 0, violations: 0, seen: Default::default() };
    let mut dv = ui::DmmView::new();
    let cases = strip_cases();
    let st = status(1);

    // 1. Combined view: every strip case x hold x verdict x REL x alt.
    for (name, r, label) in &cases {
        for hold in 0..4u8 {
            for lim in [meter::V_NONE, meter::V_PASS, meter::V_HI] {
                for rel in [false, true] {
                    combined(&w, &mut dv, r, *label, hold, lim, rel, false, false, &st);
                    ck.check(&format!("combined/{name}/h{hold}/l{lim}/r{}", rel as u8), &combined_regions(false));
                }
            }
        }
        // HV forced on V cases.
        if r.mode <= 3 {
            for hv in 0..3u8 {
                host::reset(0);
                dv.invalidate();
                ui::SCOPE_COMBINED.draw_static(&w.sc);
                dv.strip(r, 0, 0, false, hv, false);
                ck.check(&format!("combined/{name}/hv{hv}"), &combined_regions(false));
            }
        }
        // Function band open.
        combined(&w, &mut dv, r, *label, 0, 0, false, false, true, &st);
        ck.check(&format!("combined/{name}/band"), &combined_regions(true));
    }

    // 2. Strip updates: A then B equals a fresh B (no stale pixels).
    let mut stale = 0;
    for (na, a, la) in &cases {
        for (nb, b, lb) in &cases {
            let draw = |dv: &mut ui::DmmView, r: &Reading, l: Option<&'static str>, hold: u8, rel: bool| {
                dv.label = l;
                dv.strip(r, hold, if rel { meter::V_PASS } else { 0 }, rel, hv_of(r), false);
            };
            host::reset(0);
            dv.invalidate();
            draw(&mut dv, a, *la, 3, true);
            draw(&mut dv, b, *lb, 0, false);
            let got: Vec<u16> = emu().fb.clone();
            host::reset(0);
            dv.invalidate();
            draw(&mut dv, b, *lb, 0, false);
            ck.cases += 1;
            let y0 = ui::STRIP_Y as usize * W;
            let y1 = (ui::STRIP_Y + ui::STRIP_H) as usize * W;
            let diff: Vec<usize> = (y0..y1).filter(|&i| got[i] != emu().fb[i]).collect();
            if !diff.is_empty() {
                stale += 1;
                let mut b = EMPTY;
                for &i in &diff {
                    b.add((i % W) as i32, (i / W) as i32);
                }
                ck.fail(&format!("update {na} -> {nb}"), format!("{} stale pixels in the strip at {:?}", diff.len(), b));
            }
        }
    }

    // 3. Scope-family top bar: chips over timebase x V/div x probe x
    //    coupling x run state x trigger level x slope.
    let modes = [scope::TrigMode::Auto, scope::TrigMode::Normal, scope::TrigMode::Single];
    let bar_regions = vec![("screen", Bx::new(0, 0, 480, 320)), ("bar", Bx::new(0, 0, 480, ui::SBAR_H as i32))];
    for tb in scope::TB_MIN..=scope::TB_MAX {
        for range in 0..10 {
            for probe in [1.0f32, 10.0, 100.0] {
                for (k, trig_row) in [-10, 140, 290].iter().enumerate() {
                    let s = scope::Settings {
                        run: k != 1,
                        tb,
                        range,
                        ac: range % 2 == 0,
                        trig_row: *trig_row,
                        pos: if k == 2 { -392 } else { 0 },
                        rising: k != 0,
                        mode: modes[(tb + range) % 3],
                        probe,
                        hpos: 200,
                    };
                    w.sc.apply(s, 0);
                    host::reset(0);
                    lcd::fill_rect(0, 0, 480, ui::SBAR_H, ui::S_BAR);
                    stockfont::logo(4, 4, ui::S_BAR);
                    ui::SCOPE_COMBINED.labels(&w.sc);
                    ui::scope_status(&st);
                    ck.check(&format!("bar/tb{tb}/r{range}/p{probe}/t{trig_row}"), &bar_regions);
                }
            }
        }
    }
    w.sc.apply(scope::Settings::DEFAULT, 0);
    // Status block combinations (with the widest chips).
    let mut wide = scope::Settings::DEFAULT;
    wide.range = 5;
    wide.ac = true;
    wide.tb = 14;
    wide.trig_row = -10;
    wide.probe = 100.0;
    wide.run = false;
    w.sc.apply(wide, 0);
    for bits in 0..128u32 {
        for page in [0u8, 1, 2, 3, 255] {
            let s = ui::Status {
                uptime_s: if bits & 1 != 0 { 0 } else { 99 * 3600 + 59 * 60 + 59 },
                bt: bits & 2 != 0,
                bars: if bits & 4 != 0 { 0 } else { 5 },
                charging: bits & 8 != 0,
                locked: bits & 16 != 0,
                rec: bits & 32 != 0,
                csv: bits & 64 != 0,
                page,
            };
            host::reset(0);
            lcd::fill_rect(0, 0, 480, ui::SBAR_H, ui::S_BAR);
            stockfont::logo(4, 4, ui::S_BAR);
            ui::SCOPE_COMBINED.labels(&w.sc);
            ui::scope_status(&s);
            ck.check(&format!("status/{bits}/{page}"), &bar_regions);
        }
    }

    // 4. Full Scope page: panel states, measurement bar, generator chip.
    for (k, gen_hz) in [0u32, 1, 999, 50_000, 999_000, 2_000_000, 10_000_000].iter().enumerate() {
        let mut s = wide;
        s.mode = modes[k % 3];
        s.ac = k % 2 == 0;
        s.rising = k % 2 == 1;
        s.run = k % 3 != 0;
        w.sc.apply(s, 0);
        let mut g = w.gen.s;
        g.on = *gen_hz != 0;
        g.freq_hz = (*gen_hz).max(1);
        w.gen.apply(g);
        host::reset(0);
        ui::SCOPE_FULL.draw_static(&w.sc);
        frame(&w, &ui::SCOPE_FULL, u16::MAX);
        let (sel, res) = meas_worst();
        ui::scope_measurements(&sel, &res);
        ui::scope_status(&status(2));
        ui::ScopeView::gen_chip(&w.gen);
        ck.check(&format!("scope/{k}/gen{gen_hz}"), &full_regions());
    }
    // Every measurement item in every cell.
    let (_, res) = meas_worst();
    for (id, _, _) in scope_meas::ITEMS.iter() {
        host::reset(0);
        ui::SCOPE_FULL.draw_static(&w.sc);
        ui::scope_measurements(&[*id, *id, *id, *id], &res);
        ui::ScopeView::gen_chip(&w.gen);
        ck.check(&format!("meas/{id}"), &full_regions());
    }

    // 5. DMM page status bar.
    let dmm_regions = vec![("screen", Bx::new(0, 0, 480, 320)), ("bar", Bx::new(0, 0, 480, ui::BAR_H as i32))];
    let labels: Vec<Option<&'static str>> = std::iter::once(None).chain(ui::T_LABELS.iter().map(|l| Some(*l))).collect();
    for l in &labels {
        for hold in 0..4u8 {
            for (auto, plus) in [(true, false), (true, true), (false, false)] {
                for (locked, csv) in [(false, false), (true, true), (false, true), (true, false)] {
                    host::reset(0);
                    let mut r = cases[0].1;
                    r.auto = auto;
                    r.auto_plus = plus;
                    dv.label = *l;
                    dv.draw_static(Button::Volt, 1, false);
                    dv.reading(&r, hold, 0);
                    let mut s = status(0);
                    s.locked = locked;
                    s.csv = csv;
                    ui::status_right(&s);
                    // Only the bar is checked (the stock page body is unchanged).
                    let keep = ui::BAR_H as usize * W;
                    for p in emu().owner[keep..].iter_mut() {
                        *p = 0;
                    }
                    ck.check(&format!("dmmbar/{l:?}/h{hold}/a{auto}{plus}/l{locked}/c{csv}"), &dmm_regions);
                }
            }
        }
    }

    // 5b. Full DMM page: readout (+1 digit), aux rows, limit badge, HV,
    //     history row, trend chart (axis labels, timebase chip) and the
    //     MAX / AVG / MIN / REL / time column, every case.
    let dmm_page_regions = vec![
        ("screen", Bx::new(0, 0, 480, 320)),
        ("bar", Bx::new(0, 0, 480, ui::BAR_H as i32)),
        ("top", Bx::new(0, 34, 480, 118)),
        ("history", Bx::new(0, 150, 480, 25)),
        ("axis", Bx::new(0, 174, 80, 106)),
        ("side", Bx::new(402, 174, 78, 106)),
        ("buttons", Bx::new(0, 277, 480, 43)),
    ];
    for (name, r, label) in &cases {
        for ext in [false, true] {
            for (hold, lim) in [(0u8, meter::V_NONE), (1, meter::V_PASS), (2, meter::V_HI), (3, meter::V_LO)] {
                // Timebase x rate chip (FR-05: worst widths SLOW 2.3/s,
                // FAST 37/s, MED --/s, grey n/a) x REC.
                let chips = [
                    ui::RateChip { rate: 0, period_ms: 430, applies: true },
                    ui::RateChip { rate: 2, period_ms: 27, applies: true },
                    ui::RateChip { rate: 1, period_ms: 0, applies: true },
                    ui::RateChip { rate: 1, period_ms: 400, applies: false },
                    ui::RateChip { rate: 0, period_ms: 2200, applies: true },
                ];
                for (k, (rate, rec)) in [(1u8, false), (4, true), (2, false), (1, true), (4, false)].into_iter().enumerate() {
                    let rc = chips[k];
                    let mut r = *r;
                    r.main.ext = ext && !r.main.ol;
                    host::reset(0);
                    let mut dv = ui::DmmView::new();
                    dv.label = *label;
                    dv.draw_static(dmm::button_of(r.mode), r.mode, r.acdc);
                    // History / chart / statistics: the case's own value (an
                    // OL case: 1.0 in its unit and resolution).
                    let v = if r.main.ol { 1.0 } else { r.main.v };
                    let dec = r.main.dec;
                    let hist = [(v, dec, r.main.unit); 5];
                    ui::history(&hist, 5, 4);
                    dv.reading(&r, hold, lim);
                    ui::status_right(&status(0));
                    ui::hv_warning(hv_of(&r) >= 2);
                    let pts: Vec<f32> = (0..321).map(|i| v + (i % 50) as f32 * v.abs() * 1e-3).collect();
                    let side = ui::Side { max: Some(v), avg: Some(v), min: Some(v), rel: Some(v), secs: Some(99 * 3600 + 59 * 60 + 59), rec };
                    let mut cc = ui::ChartCache::new();
                    ui::chart(&mut cc, &pts, dec, rate, &side, r.main.unit, rc);
                    ck.check(&format!("dmm/{name}/x{}/h{hold}/l{lim}/r{rate}/c{k}", ext as u8), &dmm_page_regions);
                }
            }
        }
    }

    // 5c. DMM page updates: reading A then B (format / +1 digit / unit /
    //     aux / verdict changes) equals a fresh page with B.
    for (na, a, la) in &cases {
        for (nb, b, lb) in &cases {
            for (xa, xb) in [(false, true), (true, false), (true, true)] {
                let (mut a, mut b) = (*a, *b);
                a.main.ext = xa && !a.main.ol;
                b.main.ext = xb && !b.main.ol;
                let page = |dv: &mut ui::DmmView, steps: &[(Reading, Option<&'static str>, u8)]| {
                    host::reset(0);
                    dv.label = steps[0].1;
                    dv.draw_static(Button::Volt, steps[0].0.mode, steps[0].0.acdc);
                    for (r, l, lim) in steps {
                        dv.label = *l;
                        dv.reading(r, 0, *lim);
                    }
                };
                let mut dv = ui::DmmView::new();
                page(&mut dv, &[(a, *la, meter::V_HI), (b, *lb, meter::V_NONE)]);
                let got = emu().fb.clone();
                let mut dv = ui::DmmView::new();
                page(&mut dv, &[(b, *lb, meter::V_NONE)]);
                ck.cases += 1;
                let diff: Vec<usize> = (0..W * 152).filter(|&i| got[i] != emu().fb[i]).collect();
                if !diff.is_empty() {
                    let mut bx = EMPTY;
                    for &i in &diff {
                        bx.add((i % W) as i32, (i / W) as i32);
                    }
                    ck.fail(&format!("dmm update {na} -> {nb} ext {xa}->{xb}"), format!("{} stale pixels at {:?}", diff.len(), bx));
                }
            }
        }
    }

    // 6. Generator page bar, menu-page title bars.
    for on in [false, true] {
        w.gen.s.on = on;
        host::reset(0);
        ui::gen_static(&w.gen);
        let keep = 36 * W;
        for p in emu().owner[keep..].iter_mut() {
            *p = 0;
        }
        ck.check(&format!("genbar/{on}"), &[("screen", Bx::new(0, 0, 480, 320)), ("bar", Bx::new(0, 0, 480, 36))]);
    }
    // 6a. Full generator page (FR-28 / UF-14 / UF-15): four value boxes with
    //     the actual frequency in full digits, the clamp note, "auto"
    //     offset, the PWM path's "logic level", duty; the frequency keypad
    //     typing into box 0. TEXT22 box names: glyph rule off (stock font).
    let gen_regions = vec![("screen", Bx::new(0, 0, 480, 320)), ("bar", Bx::new(0, 0, 480, 36))];
    let gcases: [(gen::Wave, u32, f32, Option<f32>, u16, bool); 9] = [
        (gen::Wave::Square, 2_000_000, 3.0, None, 5000, true),
        (gen::Wave::Square, 10_000_000, 3.0, Some(1.5), 3333, false),
        (gen::Wave::Sine, 75_000, 2.5, Some(1.5), 5000, true),
        (gen::Wave::Sine, 32_768, 3.0, None, 5000, false),
        (gen::Wave::Triangle, 999, 0.0, Some(3.0), 5000, true),
        (gen::Wave::Square, 1_000, 1.25, None, 3333, true),
        (gen::Wave::Ramp, 10_000_000, 3.0, Some(0.0), 0, false),
        (gen::Wave::Square, 50_001, 3.0, Some(2.25), 10000, true),
        (gen::Wave::Sine, 1, 0.5, None, 5000, true),
    ];
    for (k, (wave, f, amp, offs, duty, on)) in gcases.iter().enumerate() {
        let g = gen::Settings { on: *on, wave: *wave, freq_hz: *f, amp_v: *amp, duty: *duty, offset_v: *offs };
        w.gen.apply(g);
        host::reset(0);
        ui::gen_static(&w.gen);
        ck.check_opts(&format!("gen/{k}/{}/{f}", wave.name()), &gen_regions, false);
        if k == 2 {
            save(&out, "g02-gen-clamped");
        }
        // The frequency keypad types into box 0 (7 digits); the keys
        // themselves are the stock keypad geometry (not checked here: its
        // 40 px keys sit 1 px apart and 3 px from the screen edge).
        if k == 0 {
            ui::keypad_input(true, "1234567", "KHz");
            ck.check_opts("gen/keypad-freq-input", &gen_regions, false);
        }
    }
    w.gen.apply(gen::Settings::DEFAULT);
    for title in ["Hold & Limits: Temperature", "Settings", "Hold & Limits: Capacitance"] {
        host::reset(0);
        ui::list_page(title, &ui::LIMIT_ROWS, &["ON"; 8]);
        let keep = 40 * W;
        for p in emu().owner[keep..].iter_mut() {
            *p = 0;
        }
        ck.check(&format!("title/{title}"), &[("screen", Bx::new(0, 0, 480, 320)), ("bar", Bx::new(0, 0, 480, 40))]);
    }

    // 6b. Settings pages 1 / 2 (worst values), Range pages with the UF-21
    //     note: whole page.
    let page_regions = vec![("screen", Bx::new(0, 0, 480, 320)), ("bar", Bx::new(0, 0, 480, 40))];
    for (page, vals) in [
        (0u8, vec!["10 min", "1 hour", "100%", "100%", "OFF", "DMM + Scope", "Auto (per view)", "Hold 5000 ms", ""]),
        (1, vec!["SLOW (n/a here)", "HIGH", ""]),
        (1, vec!["FAST  37/s", "LOW", ""]),
        (1, vec!["MED  --/s", "OFF", ""]),
    ] {
        host::reset(0);
        ui::settings(page, &vals);
        ck.check_opts(&format!("settings/{page}/{}", vals[0]), &page_regions, false);
    }
    for b in [Button::Volt, Button::Amp, Button::Ohm] {
        for note in [None, Some((dmm::MV_AUTO_WARN, true)), Some(("AUTO+ recommended: it never steps to mV", false))] {
            for sub in 0..3u8 {
                host::reset(0);
                let n = ui::range_items(b).len();
                ui::range_page(b, Some(n - 1), sub, note);
                ck.check_opts(&format!("range/{}/{}/{sub}", n, note.map_or("-", |x| x.0)), &page_regions, false);
                // The note itself (spacing 1) keeps the glyph rule: only it.
                if let Some((t, warn)) = note {
                    host::reset(0);
                    ui::range_note(t, warn);
                    ck.check(&format!("range-note/{t}"), &page_regions);
                }
            }
        }
    }

    // 6d. Main menu (gallery row on top, four tiles, combined-view row).
    for combo in [ui::View::DmmScope, ui::View::Dmm] {
        host::reset(0);
        ui::main_menu(ui::view_name(combo));
        ck.check_opts(&format!("menu/{}", ui::view_name(combo)), &[("screen", Bx::new(0, 0, 480, 320))], false);
    }
    host::reset(0);
    ui::main_menu(ui::view_name(ui::View::DmmScope));
    save(&out, "m04-main-menu");

    // 6c. Toasts (dialog.rs) over the DMM page: the longest texts.
    for (k, (t, ok)) in [
        ("Disk in use by PC - eject first", false),
        ("Saved LOG000.CSV", true),
        ("Disk full: log ended", false),
        ("No reading to log", false),
        ("Recording CSV", true),
        ("Screenshot failed", false),
    ]
    .iter()
    .enumerate()
    {
        host::reset(0);
        dialog::toast(t, *ok);
        let (x, y, w2, h2) = dialog::TOAST;
        ck.check(&format!("toast/{t}"), &[("screen", Bx::new(0, 0, 480, 320)), ("toast", Bx::new(x as i32, y as i32, w2 as i32, h2 as i32))]);
        if k == 0 {
            save(&out, "t01-toast-disk-in-use");
        }
    }

    // 7. Gestures: zone table, axis lock, hold-to-grab, Up on captured
    //    touches (gesture.rs through its DBG:DRAG hook).
    {
        use gesture::{Axis, Gesture as G, Target as T};
        let mut t = |name: &str, ok: bool| {
            ck.cases += 1;
            if !ok {
                ck.fail(&format!("gesture/{name}"), "unexpected result".into());
            }
        };
        let lv = 100; // trigger level pixel row
        let cx = Some([50, 300]);
        let cy = Some([30, 180]);
        t("band V = level", gesture::target(380, 10, Axis::V, false, lv, None, None) == T::Level);
        t("band H = trigger position", gesture::target(380, 10, Axis::H, false, lv, None, None) == T::TrigPos);
        t("body V = offset", gesture::target(200, 10, Axis::V, false, lv, None, None) == T::Offset);
        t("gutter V = offset", gesture::target(0, 200, Axis::V, false, lv, None, None) == T::Offset);
        t("body H = trigger position", gesture::target(200, 10, Axis::H, false, lv, None, None) == T::TrigPos);
        t("near Y cursor V", gesture::target(200, 170, Axis::V, false, lv, cx, cy) == T::CurY(1));
        t("near Y cursor H = position", gesture::target(200, 170, Axis::H, false, lv, cx, cy) == T::TrigPos);
        t("near X cursor H", gesture::target(60, 250, Axis::H, false, lv, cx, cy) == T::CurX(0));
        t("near X cursor V = offset", gesture::target(60, 250, Axis::V, false, lv, cx, cy) == T::Offset);
        t("X cursor 21 px away", gesture::target(321, 250, Axis::H, false, lv, cx, cy) == T::TrigPos);
        t("hold: near level line anywhere", gesture::target(20, 120, Axis::V, true, lv, None, None) == T::Level);
        t("hold: far from the line", gesture::target(380, 130, Axis::V, true, lv, None, None) == T::Offset);
        t("hold: cursor within 24", gesture::target(200, 204, Axis::V, true, lv, cx, cy) == T::CurY(1));
        let run = |grab: u16, p0: (u16, u16), p1: (u16, u16), hold: u16| -> Vec<G> {
            gesture::set_grab_ms(grab);
            let mut tr = gesture::Tracker::new();
            let mut v = vec![];
            let pt = |p: (u16, u16)| touch::Point { x: p.0, y: p.1 };
            tr.synth_drag(pt(p0), pt(p1), 300, hold, 1000, |g| {
                v.push(g);
                matches!(g, G::Down(_) | G::Drag(..))
            });
            gesture::set_grab_ms(0);
            v
        };
        let drags = |v: &[G], a: Axis| v.iter().filter(|g| matches!(g, G::Drag(_, _, x) if *x == a)).count();
        let v = run(0, (200, 150), (210, 250), 0);
        t("direct vertical: V drags only", drags(&v, Axis::V) > 0 && drags(&v, Axis::H) == 0);
        t("direct: captured touch ends with Up", matches!(v.last(), Some(G::Up(_))));
        let v = run(0, (100, 150), (300, 160), 0);
        t("direct horizontal: H drags only", drags(&v, Axis::H) > 0 && drags(&v, Axis::V) == 0);
        let v = run(300, (200, 150), (210, 250), 0);
        t("hold 300, no hold: no drag, no grab", drags(&v, Axis::V) == 0 && !v.iter().any(|g| matches!(g, G::Grab(_))));
        let v = run(300, (200, 150), (210, 250), 350);
        t("hold 300, held 350: grab then V drags", v.iter().any(|g| matches!(g, G::Grab(_))) && drags(&v, Axis::V) > 0);
        let v = run(0, (200, 150), (200, 150), 0);
        t("captured tap: Up, no Tap", matches!(v.last(), Some(G::Up(_))) && !v.iter().any(|g| matches!(g, G::Tap(_))));
    }

    // 7. Named scenes for the contact sheet.
    w.sc.apply(scope::Settings::DEFAULT, 0);
    let mut g = w.gen.s;
    g.on = true;
    g.freq_hz = 1000;
    w.gen.apply(g);
    let find = |n: &str| cases.iter().find(|c| c.0 == n).map(|c| (c.1, c.2)).unwrap();
    let mut st2 = status(1);
    st2.rec = false;
    let scenes: [(&str, &str, u8, u8, bool, bool); 10] = [
        ("c01-dcv", "dcv", 0, 0, false, false),
        ("c02-acdc-hold-rel", "acdc_600mv", 1, 0, true, false),
        ("c03-acdc-worst", "acdc_mv", 3, meter::V_HI, true, true),
        ("c04-hz-ahold", "hz_mhz", 2, 0, false, false),
        ("c05-acv-hv", "acv_230", 0, meter::V_PASS, false, false),
        ("c06-temp-tc-ol", "temp_tc_ol", 0, 0, false, false),
        ("c07-temp-ntc", "temp_ntc", 0, 0, true, false),
        ("c08-ohm-ol", "ohm_ol", 0, 0, false, false),
        ("c09-cap-nf", "cap_nf", 0, 0, false, false),
        ("c10-band", "dcma_420", 0, 0, false, false),
    ];
    for (file, case, hold, lim, rel, alt) in scenes {
        let (r, l) = find(case);
        let mut r = r;
        if case == "hz_mhz" {
            r.auto_plus = true;
        }
        combined(&w, &mut dv, &r, l, hold, lim, rel, alt, file == "c10-band", &st2);
        save(&out, file);
    }
    {
        let mut s = wide;
        s.run = false;
        w.sc.apply(s, 0);
        host::reset(0);
        ui::SCOPE_FULL.draw_static(&w.sc);
        frame(&w, &ui::SCOPE_FULL, u16::MAX);
        let (sel, res) = meas_worst();
        ui::scope_measurements(&sel, &res);
        let mut s2 = status(2);
        s2.charging = true;
        s2.locked = true;
        ui::scope_status(&s2);
        ui::ScopeView::gen_chip(&w.gen);
        save(&out, "s01-scope");
        host::reset(0);
        let (r, _) = find("dcv");
        dv.label = None;
        dv.draw_static(Button::Volt, 1, false);
        ui::history(&[(4.8170, 4, "V"); 5], 3, 2);
        dv.reading(&Reading { main: Val { ext: true, ..r.main }, ..r }, 3, meter::V_PASS);
        ui::status_right(&status(0));
        let pts: Vec<f32> = (0..321).map(|i| 4.8170 + ((i / 40) % 3) as f32 * 1e-4).collect();
        let side = ui::Side { max: Some(4.8172), avg: Some(4.8171), min: Some(4.8169), rel: None, secs: Some(5), rec: false };
        ui::chart(&mut ui::ChartCache::new(), &pts, 4, 2, &side, "V", ui::RateChip { rate: 1, period_ms: 106, applies: true });
        save(&out, "d01-dmm");
        host::reset(0);
        let (r, l) = find("acdc_600mv");
        dv.label = l;
        dv.draw_static(Button::Volt, r.mode, r.acdc);
        ui::history(&[(-600.00, 2, "mV"); 5], 5, 4);
        dv.reading(&r, 1, meter::V_HI);
        ui::status_right(&status(0));
        let pts: Vec<f32> = (0..321).map(|i| -600.0 + (i % 64) as f32 * 0.5).collect();
        let side = ui::Side { max: Some(-568.50), avg: Some(-584.25), min: Some(-600.00), rel: Some(-501.67), secs: Some(99 * 3600 + 59 * 60 + 59), rec: true };
        ui::chart(&mut ui::ChartCache::new(), &pts, 2, 4, &side, "mV", ui::RateChip { rate: 0, period_ms: 430, applies: false });
        save(&out, "d02-dmm-acdc");
        host::reset(0);
        ui::gen_static(&w.gen);
        save(&out, "g01-gen");
        host::reset(0);
        ui::settings(0, &["5 min", "30 min", "50%", "50%", "ON", "Multimeter", "Auto (per view)", "Hold 300 ms", ""]);
        save(&out, "m01-settings");
        host::reset(0);
        ui::settings(1, &["SLOW (n/a here)", "HIGH", ""]);
        save(&out, "m02-settings2");
        host::reset(0);
        ui::range_page(Button::Volt, Some(6), 0, Some((dmm::MV_AUTO_WARN, true)));
        save(&out, "m03-range-v-warn");
    }

    for (msg, (n, scene)) in &ck.seen {
        println!("{n:6} x  {msg}   [first: {scene}]");
    }
    println!("cases {}, violations {}", ck.cases, ck.violations);
    std::process::exit(if ck.violations == 0 { 0 } else { 1 });
}
