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

#[path = "../../src/bargraph.rs"]
mod bargraph;
#[path = "../../src/clock.rs"]
mod clock;
#[path = "../../src/dialog.rs"]
mod dialog;
#[path = "../../src/sentinel.rs"]
mod sentinel;
#[path = "../../src/dmm.rs"]
mod dmm;
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
#[path = "../../src/trigsoft.rs"]
mod trigsoft;
#[path = "../../src/scope_ui.rs"]
mod scope_ui;
mod scope_add;
mod stock;
#[path = "../../src/stockfont.rs"]
mod stockfont;
#[path = "../../src/touch.rs"]
mod touch;
#[path = "../../src/ui.rs"]
mod ui;

/// Probe hooks called by the drawing code under cfg(test).
pub mod probe {
    pub fn open(kind: u8, bg: u16) -> u32 {
        crate::host::open(kind, bg)
    }
    pub fn set(id: u32, bg: u16, g: u8) {
        crate::host::set(id, bg, g)
    }
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
        self.check_area(scene, regions, glyph_rule, None)
    }

    /// `check_opts` on the items inside `area` only (our additions on a
    /// stock-replica page: the combined view's meter strip).
    fn check_area(&mut self, scene: &str, regions: &[(&str, Bx)], glyph_rule: bool, area: Option<Bx>) {
        self.cases += 1;
        let its: Vec<Item> = items().into_iter().filter(|i| area.map_or(true, |a| a.contains(&i.b))).collect();
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

impl Checker {
    /// Stock-replica scope page (src/scope_ui.rs): the spacing / margin rules
    /// are exempt (the stock geometry has 1-2 px gaps by design, port plan
    /// sec. 8); texts must still not collide (two text ink boxes overlapping
    /// without one containing the other).
    fn check_replica(&mut self, scene: &str) {
        self.cases += 1;
        let its = items();
        let texts: Vec<&Item> = its.iter().filter(|i| i.kind == probe::TEXT).collect();
        for a in 0..texts.len() {
            for b in a + 1..texts.len() {
                let (ia, ib) = (texts[a], texts[b]);
                if ia.b.contains(&ib.b) || ib.b.contains(&ia.b) {
                    continue;
                }
                if ia.b.gap(&ib.b) < 0 {
                    self.fail(scene, format!("texts #{} {:?} and #{} {:?} collide", ia.id, ia.b, ib.id, ib.b));
                }
            }
        }
    }

    /// fw-rs additions on the stock-replica page (timebase tabs, list
    /// popup): the full layout rules for every item inside one of the `add`
    /// rects: >= 3 px inside its container (the smallest shape holding it,
    /// else the smallest of `regions`), glyphs of one text not touching,
    /// and >= 3 px from every other item that is not its container or
    /// content, among all items on the screen inside `against` (the replica
    /// chrome around it included; the replica's own items among themselves
    /// stay exempt).
    fn check_add(&mut self, scene: &str, regions: &[(&str, Bx)], add: &[Bx], against: Bx) {
        self.cases += 1;
        let its: Vec<Item> = items().into_iter().filter(|i| against.contains(&i.b) || add.iter().any(|a| a.contains(&i.b))).collect();
        let is_add: Vec<bool> = its.iter().map(|i| add.iter().any(|a| a.contains(&i.b))).collect();
        let desc = |i: &Item| format!("{} #{} {:?}", kind_name(i.kind), i.id, i.b);
        let mut parent: Vec<Option<usize>> = vec![None; its.len()];
        for (a, ia) in its.iter().enumerate() {
            let mut best: Option<(usize, i32)> = None;
            for (b, ib) in its.iter().enumerate() {
                if a != b && ib.kind == probe::SHAPE && ib.b.contains(&ia.b) && ib.b.area() > ia.b.area() && best.map_or(true, |(_, ar)| ib.b.area() < ar) {
                    best = Some((b, ib.b.area()));
                }
            }
            parent[a] = best.map(|x| x.0);
        }
        for (a, ia) in its.iter().enumerate() {
            if !is_add[a] {
                continue;
            }
            match parent[a] {
                Some(p) => {
                    let m = its[p].b.margin(&ia.b);
                    if m < 3 {
                        self.fail(scene, format!("{} only {} px inside {}", desc(ia), m, desc(&its[p])));
                    }
                }
                None => match regions.iter().filter(|(_, r)| r.contains(&ia.b)).min_by_key(|(_, r)| r.area()) {
                    Some((name, r)) if r.margin(&ia.b) < 3 => self.fail(scene, format!("{} only {} px inside region {} {:?}", desc(ia), r.margin(&ia.b), name, r)),
                    Some(_) => {}
                    None => self.fail(scene, format!("{} not inside any region", desc(ia))),
                },
            }
            if ia.kind == probe::TEXT {
                let mut g = ia.glyphs.clone();
                g.sort_by_key(|(k, _)| *k);
                for w in g.windows(2) {
                    if w[0].1.gap(&w[1].1) < 1 {
                        self.fail(scene, format!("{}: glyphs {} / {} touch", desc(ia), w[0].0, w[1].0));
                    }
                }
            }
        }
        let anc = |mut x: usize, y: usize| {
            while let Some(p) = parent[x] {
                if p == y {
                    return true;
                }
                x = p;
            }
            false
        };
        for a in 0..its.len() {
            for b in a + 1..its.len() {
                if !(is_add[a] || is_add[b]) || anc(a, b) || anc(b, a) {
                    continue;
                }
                let g = its[a].b.gap(&its[b].b);
                if g < 3 {
                    self.fail(scene, format!("{} and {} {} by {} px", desc(&its[a]), desc(&its[b]), if g < 0 { "overlap" } else { "too close" }, g));
                }
            }
        }
    }

    /// The screen equals `want` (an incremental update left no stale pixel).
    fn same_as(&mut self, scene: &str, want: &[u16]) {
        self.cases += 1;
        let fb = &emu().fb;
        let bad: Vec<usize> = (0..fb.len()).filter(|&i| fb[i] != want[i]).collect();
        if let Some(&i) = bad.first() {
            self.fail(scene, format!("{} stale pixels after an update (first at {},{})", bad.len(), i % W, i / W));
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

/// Wrong-connection alerts (sentinel.rs) cycled through the page / strip matrices.
const ALERTS: [sentinel::Alert; 8] = [
    sentinel::Alert::None,
    sentinel::Alert::AJack,
    sentinel::Alert::VJack,
    sentinel::Alert::Live,
    sentinel::Alert::Overload,
    sentinel::Alert::Dc,
    sentinel::Alert::Ac,
    sentinel::Alert::Fuse,
];

fn rd(mode: u8, main: Val, a1: Option<Val>, a2: Option<Val>, acdc: bool) -> Reading {
    let si = main.v * meter::unit_scale(main.unit);
    Reading { mode, range: 0, main, aux1: a1, aux2: a2, si, auto: true, auto_plus: false, acdc, seq: 0, avg_n: 0, alert: sentinel::Alert::None, power: None }
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
        // OVERLOAD latch: the 10 A range after the mA shunt overloaded.
        ("dca10", rd(7, val(2.345, 3, "A"), None, None, false), None),
        ("dcma_420", rd(8, val(24.444, 3, "mA"), None, Some(val(125.0, 1, "%")), false), None),
        ("dcma_420ol", rd(8, val(44.444, 3, "mA"), None, Some(ol(1, "%")), false), None),
        ("dcma_420lo", rd(8, val(0.0, 3, "mA"), None, Some(val(-25.0, 1, "%")), false), None),
        ("dcma_420mid", rd(8, val(12.0, 3, "mA"), None, Some(val(50.0, 1, "%")), false), None),
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
        ("temp_tc", rd(15, val(-200.0, 1, c), Some(val(26.5, 1, c)), None, false), Some("TC K")),
        ("temp_tc_ol", rd(15, ol(1, c), Some(val(-44.4, 1, c)), None, false), Some("TC K")),
        ("temp_ntc", rd(15, val(125.00, 2, c), Some(val(444.444, 3, "k\u{3a9}")), None, false), Some("NTC100k B3950")),
        ("temp_pt", rd(15, val(-44.44, 2, c), Some(val(444.44, 2, "\u{3a9}")), None, false), Some("PT1000")),
        ("temp_ntcu", rd(15, val(4.44, 2, c), Some(ol(2, "M\u{3a9}")), None, false), Some("NTC user")),
        // Global unit deg F (widest: -328.0 / 1562.00 F, CJ in F).
        ("temp_tc_f", rd(15, val(-328.0, 1, f), Some(val(-40.0, 1, f)), None, false), Some("TC K")),
        ("temp_pt_f", rd(15, val(1562.00, 2, f), Some(val(3904.8, 1, "\u{3a9}")), None, false), Some("PT100")),
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
    // Power meter readings (strip: "POWER", aux V / I; DMM page).
    v.extend(pw_cases().into_iter().map(|(n, p)| (n, pw_rd(p), None)));
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

/// The combined view as the app draws it: the replica page (graph empty),
/// the meter strip from a reading, optionally the DMM function band.
fn combined(r: &Reading, label: Option<&'static str>, hold: u8, lim: u8, rel: bool, alt: bool, band: bool, gen: &str) {
    host::reset(0);
    let mut m = scope_ui::Model::DEFAULT;
    m.combined = true;
    scope_ui::invalidate();
    scope_ui::strip_invalidate();
    scope_ui::graph().clear();
    scope_ui::update(&m);
    scope_ui::cells(&m, &["Freq:444.4kHz", "Vp-p:-444mV", "Cycl:444.4us", "Vrms:-444mV"]);
    let mut d = ui::strip_data(r, hold, lim, rel, hv_of(r), alt, label, false);
    d.gen = scope_ui::Txt::new(gen);
    scope_ui::meter_strip(&d);
    if band {
        // Ohm cases with the "ON" variant tag (worst case for the button).
        ui::func_band(r_button(r), r.mode, r.acdc, r.mode == 10);
    }
}

/// Pixels per drawn item (probe owner ids) on the screen now.
fn owner_counts() -> std::collections::HashMap<u32, usize> {
    let mut m = std::collections::HashMap::new();
    for &o in emu().owner.iter() {
        if o != 0 {
            *m.entry(o >> 8).or_insert(0) += 1;
        }
    }
    m
}

fn r_button(r: &Reading) -> Button {
    dmm::button_of(r.mode)
}


/// A power-meter reading (dmm::pw::reading_vals, as the Dmm builds it).
fn pw_rd(pv: dmm::pw::PowerVals) -> Reading {
    let (main, v, _, si) = dmm::pw::reading_vals(&pv);
    let (iu, ir) = dmm::pw::iu_ir(pv.ik);
    let (_, _, i, _) = dmm::pw::reading_vals(&pv);
    let mut r = rd(4 + iu, main, Some(v), Some(i), false);
    r.range = ir as i8;
    r.si = si;
    r.auto = pv.v_auto && pv.i_auto;
    r.power = Some(pv);
    r
}

/// Power-meter worst cases: (name, values).
fn pw_cases() -> Vec<(&'static str, dmm::pw::PowerVals)> {
    use dmm::pw::PowerVals as P;
    let z = P::ZERO;
    let base = P { p: 229.43, s: 229.9, q: 14.7, pf: 0.998, v: 229.98, i: 0.99962, f_hz: 50.0, burden_v: 0.02, e_wh: 0.1234, vah: 0.1236, secs: 754, p_avg: 0.58918, vr: 2, ik: 4, ..z };
    vec![
        ("mains", base),
        ("worst", P { p: -10499.9, s: 10499.9, q: 10499.9, pf: -0.999, v: -1049.99, i: 10.4999, vdc: -1049.0, idc: -10.4, f_hz: 99999.0, burden_v: 0.2851, e_wh: -99999.0, vah: 99999.0, secs: 99 * 3600 + 59 * 60 + 59, p_avg: -9999.9, vr: 3, ik: 5, v_auto: false, i_auto: false, run: false, over: 1, over_left: 3600, comp: false, ..z }),
        ("mw", P { p: -3.7799e-3, s: 3.78e-3, q: 1.2e-3, pf: -0.5, v: 6.2999, i: 599.99e-6, f_hz: 999.99, burden_v: 0.285, e_wh: 0.00001, vah: 0.0, secs: 1, p_avg: -0.036, vr: 0, ik: 0, over: 2, ..z }),
        ("ol", P { p: 0.0, v_ol: true, i_ol: true, pf: 0.0, f_hz: 0.0, e_wh: 12345.6, vah: 99999.9, vr: 3, ik: 1, ..z }),
        ("alt", P { p: f32::NAN, q: f32::NAN, pf: f32::NAN, vdc: f32::NAN, s: 1234.5, v: 230.0, i: 5.367, f_hz: 60.0, alt: true, vah: 1.5, secs: 3600, vr: 2, ik: 4, ..z }),
        ("dc", P { p: -24.012, s: 24.012, q: 0.0, pf: -1.0, v: 12.006, i: 2.0, vdc: 12.006, idc: -2.0, f_hz: 0.0, burden_v: 0.02, e_wh: -1.25, vah: 1.25, secs: 187, p_avg: -24.064, vr: 1, ik: 4, ..z }),
        // The DC (AD1) method: PF / S / Q / f blank, the DC glyph; AUTO.
        ("dc-ad1", P { p: -24.012, s: 24.012, q: 0.0, pf: -1.0, v: -12.006, i: 2.0, vdc: -12.006, idc: 2.0, f_hz: 0.0, burden_v: 0.02, e_wh: -1.25, vah: 1.25, secs: 187, p_avg: -24.064, vr: 1, ik: 4, dc: true, coup: dmm::pw::COUP_AUTO, ..z }),
        // Main readout cycled to the energy (Wh: kWh worst; VAh) and the
        // coupling chip in its states.
        ("main-wh", P { main: dmm::pw::MAIN_WH, e_wh: -99999.0, coup: dmm::pw::COUP_AUTO, ..base }),
        ("main-vah", P { main: dmm::pw::MAIN_VAH, vah: 0.12345, coup: dmm::pw::COUP_DC, dc: true, ..base }),
        ("main-mwh", P { main: dmm::pw::MAIN_WH, e_wh: -0.99999, ..base }),
        // No current: values that round to zero (never "-0.0000").
        ("zero", P { p: -2.0e-8, s: 1.0e-8, q: -1.0e-8, pf: -0.0001, v: 6.738, i: -1.0e-9, f_hz: 40.0, e_wh: -1.66e-9, vah: 1.0e-9, secs: 55, p_avg: -1.0e-9, vr: 1, ik: 0, ..z }),
        ("main-zero", P { main: dmm::pw::MAIN_WH, p: -2.0e-8, v: 6.738, i: -1.0e-9, f_hz: 40.0, e_wh: -1.27e-10, secs: 70, p_avg: -1.0e-9, vr: 1, ik: 0, ..z }),
        ("dc-ua", P { p: 5.9999e-3, s: 5.9999e-3, pf: 1.0, v: 9.9999, i: 599.99e-6, vdc: 9.9999, idc: 599.99e-6, vr: 1, ik: 0, dc: true, coup: dmm::pw::COUP_DC, ..z }),
    ]
}

/// The DMM page in the power meter with `pv` (status, rows, chart, side).
fn power_page(pv: dmm::pw::PowerVals, hold: u8, alert: sentinel::Alert, rc: ui::RateChip) -> Reading {
    power_page_lim(pv, hold, alert, rc, 0)
}

/// `power_page` with a limit verdict (meter::V_*: the W limits' badge in
/// the AUTO label's slot).
fn power_page_lim(pv: dmm::pw::PowerVals, hold: u8, alert: sentinel::Alert, rc: ui::RateChip, lim: u8) -> Reading {
    let mut r = pw_rd(pv);
    r.alert = alert;
    let mut dv = ui::DmmView::new();
    dv.power = true;
    dv.hv_red = pv.v.abs() >= 30.0;
    // The W button's glyph: the method in use (the app's pw_btn_glyph).
    ui::set_power_dc(pv.dc);
    dv.draw_static(Button::Power, r.mode, true, false);
    let mut pc = ui::pw::PwCache::new();
    ui::pw::side_clear(&mut pc);
    dv.reading(&r, hold, lim);
    ui::status_right(&status(0));
    let v = if r.main.ol { 1.0 } else { r.main.v };
    let pts: Vec<f32> = (0..321).map(|i| v + (i % 50) as f32 * v.abs() * 1e-3).collect();
    ui::chart(&mut ui::ChartCache::new(), &pts, r.main.dec, 2, None, r.main.unit, rc);
    ui::pw::side(&mut pc, &pv);
    r
}

/// (job running, last job, result) of the settings status line.
fn pw_status_cases() -> Vec<(u8, u8, u8)> {
    use dmm::pw::*;
    let mut v = vec![(JOB_ZERO, JOB_ZERO, JR_RUN), (JOB_VCAL, JOB_VCAL, JR_RUN), (0, 0, JR_NONE)];
    for r in [JR_OK, JR_SIGNAL, JR_ABORT, JR_METHOD] {
        v.push((0, JOB_ZERO, r));
    }
    for r in [JR_OK, JR_SMALL, JR_BIG, JR_UNSTABLE, JR_RANGE, JR_ABORT] {
        v.push((0, JOB_VCAL, r));
    }
    v
}

fn pw_status_pw(job: u8, last: u8, res: u8) -> dmm::pw::Pw {
    let mut p = dmm::pw::Pw::new();
    p.job = job;
    p.job_last = last;
    p.job_res = res;
    p.vr = 3;
    p.job_vr = 3;
    p.vgain[3] = 1.00453;
    p
}

fn page_regions_pw() -> Vec<(&'static str, Bx)> {
    vec![("screen", Bx::new(0, 0, 480, 320)), ("bar", Bx::new(0, 0, 480, 40))]
}

fn save(out: &str, name: &str) {
    host::save_ppm(&format!("{out}/{name}.ppm"), 2);
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    host::map_flash();
    // `main <out> --stock <dir>`: only the replica scope scenes, 1x, for the
    // pixel diff against tools/stock-ui (stock-diff.sh).
    if std::env::args().nth(2).as_deref() == Some("--stock") {
        let dir = std::env::args().nth(3).unwrap_or_else(|| out.clone());
        std::fs::create_dir_all(&dir).unwrap();
        for sc in stock::scenes() {
            stock::draw(&sc);
            host::save_ppm(&format!("{dir}/{}.ppm", sc.name), 1);
        }
        for sc in stock::emu_scenes() {
            stock::draw_with(&sc, &stock::emu_popvals(&sc.m));
            host::save_ppm(&format!("{dir}/{}.ppm", sc.name), 1);
        }
        return;
    }
    let mut w = World {
        sc: scope::Scope::new(),
        gen: gen::Gen::new(),
        data: (0..2000).map(|i| if (i / 250) % 2 == 0 { 700 } else { 300 }).collect(),
    };
    let mut ck = Checker { cases: 0, violations: 0, seen: Default::default() };
    let mut dv = ui::DmmView::new();
    let cases = strip_cases();
    let st = status(1);

    // 0. Stock-replica scope page: every reference scene (text collisions),
    // and every scene drawn as an update over every other one equals its
    // fresh draw (no stale pixels from the per-widget / per-row diffs), plus
    // a trace dragged up and down by 1..40 rows in steps.
    {
        let sc = stock::scenes();
        let mut fresh = vec![];
        for s in &sc {
            stock::draw(s);
            ck.check_replica(&format!("replica/{}", s.name));
            fresh.push(emu().fb.clone());
        }
        save(&out, "s00-replica-st01");
        for (k, s) in sc.iter().enumerate() {
            if s.name.starts_with("st20") || s.name.starts_with("st31") {
                emu().fb.copy_from_slice(&fresh[k]);
                save(&out, &format!("s00-replica-{}", s.name));
            }
        }
        for (i, a) in sc.iter().enumerate() {
            for (j, b) in sc.iter().enumerate() {
                // Popups close with a full redraw (the app): page states only.
                if i == j || a.popup != scope_ui::Popup::None || b.popup != scope_ui::Popup::None {
                    continue;
                }
                stock::draw(a);
                stock::load(b);
                scope_ui::update(&b.m);
                scope_ui::cells(&b.m, &b.cells);
                ck.same_as(&format!("replica/{}->{}", a.name, b.name), &fresh[j]);
            }
        }
        let base = &sc[0];
        stock::draw(base);
        let mut m = base.m;
        for (k, d) in [1, 3, -7, 12, 40, -40, -1, 0].iter().enumerate() {
            m.pos = *d;
            m.guide = k % 2 == 0;
            m.sel_v = (k % 2) as u8;
            let s2 = stock::Scene { name: "drag", m, popup: scope_ui::Popup::None, trace: base.trace.as_ref().map(|t| t.shifted(*d as i32)), persist: None, cols: 399, cells: base.cells.clone() };
            stock::load(&s2);
            scope_ui::update(&m);
            let inc = emu().fb.clone();
            stock::draw(&s2);
            let want = emu().fb.clone();
            emu().fb.copy_from_slice(&inc);
            ck.same_as(&format!("replica/drag{d}"), &want);
            stock::draw(base);
            stock::load(&s2);
            scope_ui::update(&m);
        }
        // Redraw budget of one drag event (port plan sec. 6: <= 4 ms; the
        // panel takes ~36 Mpx/s): pixels written by the update after one
        // event of each kind, from the default page with the guide shown.
        let mut m0 = base.m;
        m0.guide = true;
        let s0 = stock::Scene { name: "budget", m: m0, popup: scope_ui::Popup::None, trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() };
        let mut cost = |what: &str, m: scope_ui::Model, shift: i32| {
            stock::draw(&s0);
            let s1 = stock::Scene { name: "budget", m, popup: scope_ui::Popup::None, trace: base.trace.as_ref().map(|t| t.shifted(shift)), persist: None, cols: 399, cells: base.cells.clone() };
            stock::load(&s1);
            emu().writes = 0;
            scope_ui::update(&m);
            let px = emu().writes;
            println!("drag budget: {what}: {px} px ({:.2} ms at 36 Mpx/s)", px as f64 / 36e3);
            ck.cases += 1;
            if px > 36_000 * 4 {
                ck.fail("replica/budget", format!("{what}: {px} px per drag event"));
            }
        };
        let mut m = m0;
        m.pos = 3;
        m.trig_row += 3;
        cost("CH1 position +3 rows", m, 3);
        let mut m = m0;
        m.sel_v = 1;
        m.trig_row = 102;
        m.trig_v = 1.1;
        cost("trigger level -3 rows (select + move)", m, 0);
        let mut m = m0;
        m.sel_v = 1;
        let mut m1 = m;
        m1.trig_row = 99;
        m1.trig_v = 1.2;
        stock::draw(&stock::Scene { name: "b", m, popup: scope_ui::Popup::None, trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() });
        stock::load(&stock::Scene { name: "b", m: m1, popup: scope_ui::Popup::None, trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() });
        emu().writes = 0;
        scope_ui::update(&m1);
        println!("drag budget: trigger level -6 rows (selected): {} px", emu().writes);
        let mut m = m0;
        m.trig_px = 203;
        cost("trigger position +3 px", m, 0);
        stock::draw(&s0);
        emu().writes = 0;
        scope_ui::update(&m0);
        println!("drag budget: no change: {} px", emu().writes);
        // Stock zone selection (sec. 10.2): 32x32 zones, search order,
        // cycling on repeated presses over overlapping zones.
        let pick = |m: &scope_ui::Model, sv: u8, sh: u8, lh: bool, x: i16, y: i16| -> Option<(bool, u8)> {
            let mut z = [scope_ui::Zone::NONE; scope_ui::NZONE];
            let n = scope_ui::zones(m, &mut z);
            scope_ui::pick_zone(&z[..n], sv, sh, lh, x, y).map(|k| (z[k].level, z[k].id))
        };
        let d = scope_ui::Model::DEFAULT;
        let mut t = |name: &str, ok: bool| {
            ck.cases += 1;
            if !ok {
                ck.fail("replica/zones", name.to_string());
            }
        };
        t("CH1 marker zone (x 20, 10 px into the graph)", pick(&d, 0, 0, false, 20, 160) == Some((true, 0)));
        t("trigger level marker zone", pick(&d, 0, 0, false, 6, 125) == Some((true, 1)));
        t("T marker zone (incl. 10 px of the top bar)", pick(&d, 0, 0, false, 214, 12) == Some((false, 0)));
        t("graph middle: no zone (timebase tap)", pick(&d, 0, 0, false, 120, 220).is_none());
        let mut m = d;
        m.trig_row = 140;
        let a = pick(&m, 0, 0, false, 6, 160);
        let b = pick(&m, 1, 0, false, 6, 160);
        t("overlapping zones cycle: CH1 -> level -> CH1", a == Some((true, 1)) && b == Some((true, 0)));
        let mut m = d;
        m.xcur = Some([200, 205]);
        let a = pick(&m, 0, 0, true, 214, 25);
        let b = pick(&m, 0, 2, true, 214, 25);
        let c = pick(&m, 0, 3, true, 214, 25);
        t("T + 2 X cursors at one spot cycle", a == Some((false, 2)) && b == Some((false, 3)) && c == Some((false, 0)));
        let mut m = d;
        m.xcur = Some([100, 300]);
        m.ycur = Some([60, 220]);
        let mut lz = [scope_ui::Zone::NONE; 4];
        let n = scope_ui::line_zones(&m, &mut lz);
        let lp = |x: i16, y: i16| scope_ui::pick_zone(&lz[..n], 0, 0, false, x, y).map(|k| (lz[k].level, lz[k].id));
        t("cursor line zones: X1 line, Y2 line, none between", lp(124, 200) == Some((false, 2)) && lp(200, 230) == Some((true, 4)) && lp(200, 150).is_none());
    }

    // 0a. fw-rs timebase tap step (docs/scope-scale-ux.md sec. 3): the
    // preview tabs (every label, rest / pressed, both views, markers and
    // cursors clamped at the bottom, worst measurement cells) and the list
    // popup (every centre index) under the full layout rules against the
    // replica around them; the flashed badge at every timebase; tabs shown
    // / pressed / hidden over a trace as updates equal their fresh draw.
    {
        let base = &stock::scenes()[0];
        let rb = |r: stockfont::Rect| Bx::new(r.x as i32, r.y as i32, r.w as i32, r.h as i32);
        let tabs_add = [rb(scope_ui::TAB_L), rb(scope_ui::TAB_R)];
        let screen = Bx::new(0, 0, 480, 320);
        let graph_region = |m: &scope_ui::Model| [("graph", Bx::new(14, m.top() as i32 + 1, 399, m.frame_h() as i32 - 2))];
        let (_, res) = meas_worst();
        let mut wb = [[0u8; 24]; 4];
        let worst: Vec<String> = [10u8, 11, 0, 9].iter().enumerate().map(|(k, id)| scope_meas::format(&mut wb[k], *id, res.v[*id as usize]).to_string()).collect();
        let wc: Vec<&'static str> = worst.into_iter().map(|s| &*Box::leak(s.into_boxed_str())).collect();
        let mut n_tab = 0;
        for combined in [false, true] {
            for tb in 0..30u8 {
                for (k, press) in [None, Some(true), Some(false)].iter().enumerate() {
                    let mut m = base.m;
                    m.combined = combined;
                    m.tb = tb;
                    m.tabs = scope_ui::tabs_of(scope_ui::TAP_CORNERS, tb, 29, false, *press);
                    // Markers / cursors at the bottom edge next to the tabs.
                    if k == 1 {
                        m.pos = 300;
                        m.trig_row = 300;
                        m.ycur = Some([279, 278]);
                        m.xcur = Some([12, 395]);
                    }
                    let mut sc = stock::Scene { name: "tabs", m, popup: scope_ui::Popup::None, trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() };
                    if k == 2 {
                        sc.cells = wc.clone();
                    }
                    stock::draw(&sc);
                    let name = format!("tabs/{}/tb{tb}/{k}", if combined { "dmmscope" } else { "scope" });
                    ck.check_replica(&name);
                    ck.check_add(&name, &graph_region(&m), &tabs_add, screen);
                    n_tab += 1;
                    if tb == 13 && !combined && k == 1 {
                        save(&out, "s01-tabs-corners-pressed-left");
                    }
                    if tb == 13 && combined && k == 0 {
                        save(&out, "s01-tabs-dmmscope");
                    }
                }
            }
        }
        // Limits: no slower tab at the top index, no faster one at TB_MIN.
        ck.cases += 1;
        let (lo, top) = (scope::TB_MIN as u8, scope_ui::tb_top(false, false));
        if scope_ui::tabs_of(scope_ui::TAP_CORNERS, top, top, false, None) != scope_ui::TAB_SHOW_R || scope_ui::tabs_of(scope_ui::TAP_CORNERS, lo, top, false, None) != scope_ui::TAB_SHOW_L {
            ck.fail("tabs/limits", "a tab shown at its limit".into());
        }
        // Halves: hidden at rest, shown while armed / after a step; Off: never.
        ck.cases += 1;
        let h = |tr: bool, p: Option<bool>| scope_ui::tabs_of(scope_ui::TAP_HALVES, 13, 24, tr, p);
        if h(false, None) != 0 || h(true, None) != 3 || h(false, Some(false)) != 3 | scope_ui::TAB_PRESS_R || scope_ui::tabs_of(scope_ui::TAP_OFF, 13, 24, true, Some(true)) != 0 {
            ck.fail("tabs/modes", "Halves / Off visibility".into());
        }
        println!("timebase tabs: {n_tab} scenes");
        // Updates: tabs appear / press / move label / vanish over the trace;
        // each step equals its fresh draw (the hole rows come back).
        let states: [(u8, u8); 6] = [(13, 0), (13, 3), (13, 3 | scope_ui::TAB_PRESS_L), (14, 3), (14, 0), (3, scope_ui::TAB_SHOW_L)];
        for combined in [false, true] {
            for (i, a) in states.iter().enumerate() {
                for (j, b) in states.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    let mk = |s: &(u8, u8)| {
                        let mut m = base.m;
                        m.combined = combined;
                        m.tb = s.0;
                        m.tabs = s.1;
                        stock::Scene { name: "tabs", m, popup: scope_ui::Popup::None, trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() }
                    };
                    let (sa, sb) = (mk(a), mk(b));
                    stock::draw(&sb);
                    let want = emu().fb.clone();
                    stock::draw(&sa);
                    stock::load(&sb);
                    scope_ui::update(&sb.m);
                    scope_ui::cells(&sb.m, &sb.cells);
                    ck.same_as(&format!("tabs/update/{}/{i}->{j}", combined as u8), &want);
                }
            }
        }
        // The trace dragged under shown tabs: no stale pixels, no ink on them.
        for d in [5, -9, 40] {
            let mut m = base.m;
            m.tabs = 3;
            let s0 = stock::Scene { name: "tabs", m, popup: scope_ui::Popup::None, trace: base.trace.as_ref().map(|t| t.shifted(110)), persist: None, cols: 399, cells: base.cells.clone() };
            let s1 = stock::Scene { name: "tabs", m, popup: scope_ui::Popup::None, trace: base.trace.as_ref().map(|t| t.shifted(110 + d)), persist: None, cols: 399, cells: base.cells.clone() };
            stock::draw(&s1);
            let want = emu().fb.clone();
            stock::draw(&s0);
            stock::load(&s1);
            scope_ui::update(&m);
            ck.same_as(&format!("tabs/drag{d}"), &want);
        }
        // Flashed timebase badge, every label.
        for tb in 0..30u8 {
            let mut m = scope_ui::Model::DEFAULT;
            m.tb = tb;
            m.tb_flash = true;
            host::reset(0);
            scope_ui::invalidate();
            scope_ui::graph().clear();
            scope_ui::update(&m);
            ck.check_replica(&format!("tb-flash/{tb}"));
            if tb == 13 {
                save(&out, "s01-tb-flash");
            }
        }
        // Timebase list popup, every centre index, both views: its items
        // under the full rules (the box as their region), the box against
        // the top bar.
        let bx = scope_ui::TB_BOX;
        let box_b = Bx::new(bx.x as i32, bx.y as i32, bx.w as i32, bx.h as i32);
        for combined in [false, true] {
            for cur in scope::TB_MIN as u8..=scope::TB_MAX as u8 {
                let mut m = base.m;
                m.combined = combined;
                m.tb = cur;
                let sc = stock::Scene { name: "tblist", m, popup: scope_ui::Popup::Tb(cur), trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() };
                stock::draw(&sc);
                let name = format!("tb-list/{}/{cur}", combined as u8);
                ck.check_area(&name, &[("screen", screen)], true, Some(box_b));
                ck.check_add(&name, &[("screen", screen)], &[box_b], Bx::new(0, 0, 480, 20));
                if cur == 13 && !combined {
                    save(&out, "s01-tb-list");
                }
                if cur == 27 && !combined {
                    save(&out, "s01-tb-list-roll");
                }
            }
        }
        // DMM wave view: the list starts at 5 ms/div (WAVE_TB_MIN).
        scope_ui::set_tb_lo(scope::WAVE_TB_MIN);
        for cur in scope::WAVE_TB_MIN as u8..=scope::WAVE_TB_MIN as u8 + 2 {
            let mut m = base.m;
            m.tb = cur;
            let sc = stock::Scene { name: "tblist", m, popup: scope_ui::Popup::Tb(cur), trace: base.trace.clone(), persist: None, cols: 399, cells: base.cells.clone() };
            stock::draw(&sc);
            let name = format!("tb-list/wave/{cur}");
            ck.check_area(&name, &[("screen", screen)], true, Some(box_b));
            ck.check_add(&name, &[("screen", screen)], &[box_b], Bx::new(0, 0, 480, 20));
            if cur == scope::WAVE_TB_MIN as u8 {
                save(&out, "s01-tb-list-wave");
            }
        }
        scope_ui::set_tb_lo(scope::TB_MIN);
    }

    // 0c. fw-rs additions on the replica page (FFT boxes / peak, ...).
    scope_add::run(&mut ck, &out);

    // 0b. Combined view in the stock language (scope_ui meter strip, port
    // plan S7): every strip case x hold x verdict x REL x HV x REC x GEN,
    // the full layout rules on the strip's items, and A -> B updates equal
    // a fresh B (no stale pixels).
    {
        let mut m = scope_ui::Model::DEFAULT;
        m.combined = true;
        let strip_area = Bx::new(13, 20, 401, 56);
        let strip_regions = [("screen", Bx::new(0, 0, 480, 320))];
        let page = |m: &scope_ui::Model| {
            host::reset(0);
            scope_ui::invalidate();
            scope_ui::strip_invalidate();
            scope_ui::graph().clear();
            scope_ui::update(m);
        };
        let mut datas = vec![];
        for (name, r, label) in &cases {
            for hold in 0..4u8 {
                for lim in [meter::V_NONE, meter::V_PASS, meter::V_HI] {
                    for rel in [false, true] {
                        for (j, (hv, rec, gen)) in [(0u8, false, ""), (hv_of(r), true, "10.00MHz"), (1, false, "1.00kHz"), (2, false, "")].into_iter().enumerate() {
                          // Both phases of the 1 s alternation (REL / verdict;
                          // A-JACK / LIVE? with HOLD, decision 2026-10-22 #5).
                          for alt in [false, true] {
                            let mut r = *r;
                            if !(hold == 0 && lim == 0 && !rel) {
                                r.alert = ALERTS[(j + hold as usize + rel as usize) % ALERTS.len()];
                            }
                            let r = &r;
                            let mut d = ui::strip_data(r, hold, lim, rel, hv, alt, *label, rec);
                            d.gen = scope_ui::Txt::new(gen);
                            page(&m);
                            scope_ui::meter_strip(&d);
                            ck.check_area(&format!("strip/{name}/h{hold}/l{lim}/r{}/hv{hv}/a{}/p{}", rel as u8, r.alert.name(), alt as u8), &strip_regions, true, Some(strip_area));
                            if hold == 0 && lim == 0 && !rel && !alt {
                                datas.push((name, d));
                            }
                          }
                        }
                    }
                }
            }
        }
        page(&m);
        scope_ui::meter_strip(&datas[0].1);
        save(&out, "s00-replica-combined");
        // Updates: A then B equals a fresh B (strip area).
        for (na, a) in datas.iter().step_by(3) {
            for (nb, b) in datas.iter().step_by(5) {
                page(&m);
                scope_ui::meter_strip(a);
                scope_ui::meter_strip(b);
                let got = emu().fb.clone();
                page(&m);
                scope_ui::meter_strip(b);
                ck.cases += 1;
                let bad = (20..76).flat_map(|y| (0..480).map(move |x| y * W + x)).filter(|&i| got[i] != emu().fb[i]).count();
                if bad > 0 {
                    ck.fail(&format!("strip update {na} -> {nb}"), format!("{bad} stale pixels"));
                }
            }
        }
    }

    // 1. Combined view: the DMM function band over the replica page (full
    //    layout rules on the band's items), every strip case.
    let band_area = Bx::new(0, ui::FUNC_Y as i32, 480, 301 - ui::FUNC_Y as i32);
    for (name, r, label) in &cases {
        // The page items the band cuts: drawn before the band, partly
        // overwritten by it (a half key / cell left visible) = a violation;
        // fully covered or untouched is fine.
        combined(r, *label, 0, 0, false, false, false, "");
        let before = owner_counts();
        let boxes: std::collections::HashMap<u32, Bx> = items().into_iter().map(|i| (i.id, i.b)).collect();
        ui::func_band(r_button(r), r.mode, r.acdc, r.mode == 10);
        let after = owner_counts();
        ck.cases += 1;
        for (id, n) in &before {
            let m = after.get(id).copied().unwrap_or(0);
            if m > 0 && m < *n {
                ck.fail(&format!("combined/{name}/band-cut"), format!("page item {:?} cut by the band: {m} of {n} px left", boxes.get(id)));
            }
        }
        ck.check_area(&format!("combined/{name}/band"), &[("band", band_area)], true, Some(band_area));
        // The strip's sub texts fit their boxes (scope_ui::ST_SUB; the label
        // clips, so an overlong text loses its first glyphs silently).
        let d = ui::strip_data(r, 0, 0, false, hv_of(r), false, *label, false);
        for i in 0..2 {
            ck.cases += 1;
            let w = stockfont::text_width(stockfont::SC_F12, d.sub[i].as_str(), 1);
            let bw = scope_ui::st_sub(i, false).w;
            if w > bw {
                ck.fail(&format!("combined/{name}/strip-sub{i}"), format!("strip sub text {:?} {} px > its {} px box", d.sub[i].as_str(), w, bw));
            }
        }
    }

    // 3. Replica top bar / panel / cells over timebase x V/div x probe x
    //    coupling x status x trigger level x edge: no text collisions.
    for tb in 0..30u8 {
        for vdiv in 0..22u8 {
            for (k, tv) in [-444.4e-3f32, 0.0, 9.876e3].iter().enumerate() {
                let mut m = scope_ui::Model::DEFAULT;
                m.tb = tb;
                m.vdiv = vdiv;
                m.probe = (vdiv % 13).min(12);
                m.ac = vdiv % 2 == 0;
                m.status = (tb + vdiv) % 5;
                m.trig_v = *tv;
                m.rising = k != 1;
                m.roll = k == 2;
                m.gen_hz = if k == 1 { 10.0e6 } else { 0.0 };
                m.battery = vdiv % 6;
                m.charging = k == 0;
                host::reset(0);
                scope_ui::invalidate();
                scope_ui::graph().clear();
                scope_ui::update(&m);
                ck.check_replica(&format!("replica-bar/tb{tb}/v{vdiv}/{k}"));
            }
        }
    }
    // Every measurement item, worst values, in every cell.
    let (_, res) = meas_worst();
    for (id, name, _) in scope_meas::ITEMS.iter() {
        let mut b = [0u8; 24];
        let t = scope_meas::format(&mut b, *id, res.v[*id as usize]);
        let _ = name;
        host::reset(0);
        scope_ui::invalidate();
        scope_ui::graph().clear();
        let m = scope_ui::Model::DEFAULT;
        scope_ui::update(&m);
        scope_ui::cells(&m, &[t, t, t, t]);
        ck.check_replica(&format!("replica-meas/{id}"));
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
                    dv.draw_static(Button::Volt, 1, false, false);
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
        // History row 153..171, or the UF-30 bargraph in its place
        // (rows 147..172).
        ("history", Bx::new(0, 144, 480, 32)),
        ("axis", Bx::new(0, 174, 80, 106)),
        ("side", Bx::new(402, 174, 78, 106)),
        ("buttons", Bx::new(0, 277, 480, 43)),
    ];
    for (name, r, label) in &cases {
        // Power readings: their own page below (no history row there).
        if r.power.is_some() {
            continue;
        }
        for ext in [false, true] {
            for (hold, lim) in [(0u8, meter::V_NONE), (1, meter::V_PASS), (2, meter::V_HI), (3, meter::V_LO)] {
                // Timebase x rate chip (FR-05: worst widths SLOW 2.5/s,
                // FAST 37/s, MED --/s, grey n/a) x REC.
                let chips = [
                    ui::RateChip { rate: 0, period_ms: 400, applies: true },
                    ui::RateChip { rate: 2, period_ms: 27, applies: true },
                    ui::RateChip { rate: 1, period_ms: 0, applies: true },
                    ui::RateChip { rate: 1, period_ms: 400, applies: false },
                    ui::RateChip { rate: 0, period_ms: 2200, applies: true },
                ];
                for (k, (rate, rec)) in [(1u8, false), (4, true), (2, false), (1, true), (4, false)].into_iter().enumerate() {
                  for alt in [false, true] {
                    let rc = chips[k];
                    let mut r = *r;
                    r.main.ext = ext && !r.main.ol;
                    // Every alert badge in the status slot, against every hold state.
                    r.alert = ALERTS[(k + hold as usize) % ALERTS.len()];
                    // The alert phase of A-JACK / LIVE? with HOLD (decision
                    // 2026-10-22 #5); the other cases look the same in both.
                    if alt && !ui::alert_alternates(hold, r.alert) {
                        continue;
                    }
                    host::reset(0);
                    let mut dv = ui::DmmView::new();
                    dv.alt = alt;
                    dv.label = *label;
                    // UF-30: every other chip variant with the bargraph
                    // instead of the history row.
                    dv.bar_on = k % 2 == 0;
                    dv.hv_red = hv_of(&r) >= 1;
                    dv.draw_static(dmm::button_of(r.mode), r.mode, r.acdc, r.mode == 10);
                    // History / chart / statistics: the case's own value (an
                    // OL case: 1.0 in its unit and resolution).
                    let v = if r.main.ol { 1.0 } else { r.main.v };
                    let dec = r.main.dec;
                    let hist = [(v, dec, r.main.unit); 5];
                    if dv.bar_on {
                        dv.bar_update(&r);
                    } else {
                        ui::history(&hist, 5, 4);
                    }
                    dv.reading(&r, hold, lim);
                    ui::status_right(&status(0));
                    ui::hv_warning(hv_of(&r) >= 2);
                    let pts: Vec<f32> = (0..321).map(|i| v + (i % 50) as f32 * v.abs() * 1e-3).collect();
                    let side = ui::Side { max: Some(v), avg: Some(v), min: Some(v), rel: Some(v), secs: Some(99 * 3600 + 59 * 60 + 59), rec };
                    let mut cc = ui::ChartCache::new();
                    ui::chart(&mut cc, &pts, dec, rate, Some(&side), r.main.unit, rc);
                    ck.check(&format!("dmm/{name}/x{}/h{hold}/l{lim}/r{rate}/c{k}/a{}/p{}", ext as u8, r.alert.name(), alt as u8), &dmm_page_regions);
                  }
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
                // Status slot: an alert badge (the widest, A-JACK, or V-JACK?)
                // then RUN or another badge.
                a.alert = if xb { sentinel::Alert::VJack } else { sentinel::Alert::AJack };
                b.alert = if xa && xb { sentinel::Alert::Live } else { sentinel::Alert::None };
                let page = |dv: &mut ui::DmmView, steps: &[(Reading, Option<&'static str>, u8)]| {
                    host::reset(0);
                    dv.label = steps[0].1;
                    dv.draw_static(Button::Volt, steps[0].0.mode, steps[0].0.acdc, false);
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

    // 5c0. Every alert badge fits the status slot box (119..201, TEXT22,
    //      spacing 1): sf::label clips silently.
    for a in ALERTS {
        ck.cases += 1;
        let w = stockfont::text_width(stockfont::TEXT22, a.badge(), 1);
        if w > 83 {
            ck.fail(&format!("slot badge {}", a.name()), format!("{w} px > 83"));
        }
    }

    // 5c'. Status slot alternation (decision 2026-10-22 #5): HOLD / A-HOLD
    //     with A-JACK / LIVE? flipped phase by phase (and from / to RUN
    //     with an alert, and a V-JACK? that must not alternate) equals a
    //     fresh page in the last phase: each phase clears the whole box.
    {
        use sentinel::Alert;
        let (r0, l0) = cases.iter().find(|c| c.0 == "dcv").map(|c| (c.1, c.2)).unwrap();
        for hold in 0..4u8 {
            for a in [Alert::AJack, Alert::Live, Alert::VJack, Alert::Overload, Alert::None] {
                for seq in [&[false, true][..], &[true, false], &[false, true, false], &[true, false, true]] {
                    let mut r = r0;
                    r.alert = a;
                    let run = |dv: &mut ui::DmmView, steps: &[(u8, bool)]| {
                        host::reset(0);
                        dv.label = l0;
                        dv.draw_static(Button::Volt, r.mode, r.acdc, false);
                        for &(h, p) in steps {
                            dv.alt = p;
                            dv.reading(&r, h, 0);
                        }
                    };
                    // From RUN with the alert, a hold change, then the phases.
                    let mut steps = vec![(0u8, false)];
                    steps.extend(seq.iter().map(|&p| (hold, p)));
                    let mut dv = ui::DmmView::new();
                    run(&mut dv, &steps);
                    let got = emu().fb.clone();
                    let last = *steps.last().unwrap();
                    let mut dv = ui::DmmView::new();
                    run(&mut dv, &[last]);
                    ck.cases += 1;
                    let bad = (0..W * 40).filter(|&i| got[i] != emu().fb[i]).count();
                    if bad > 0 {
                        ck.fail(&format!("slot phase h{hold} {} {:?}", a.name(), seq), format!("{bad} stale pixels"));
                    }
                    // Back to RUN.
                    steps.push((0, !last.1));
                    let mut dv = ui::DmmView::new();
                    run(&mut dv, &steps);
                    let got = emu().fb.clone();
                    let mut dv = ui::DmmView::new();
                    run(&mut dv, &[(0, !last.1)]);
                    ck.cases += 1;
                    let bad = (0..W * 40).filter(|&i| got[i] != emu().fb[i]).count();
                    if bad > 0 {
                        ck.fail(&format!("slot phase h{hold} {} {:?} -> RUN", a.name(), seq), format!("{bad} stale pixels"));
                    }
                    // The phase shows what slot_alert says.
                    let alerting = ui::slot_alert(hold, a, last.1);
                    if alerting != (hold != 0 && last.1 && matches!(a, Alert::AJack | Alert::Live | Alert::Overload)) {
                        ck.fail(&format!("slot_alert h{hold} {}", a.name()), "rule".into());
                    }
                }
            }
        }
    }

    // 5d. DMM trend chart: points added one at a time (ui::chart redraws only
    //     the columns whose trace span changed) equal a fresh chart of the
    //     final points after every point: flat, noise, a step, range growth
    //     and shrink, the 321-column scroll, a unit change (n back to 0) and
    //     a resolution change. Plus the redraw budget of one point.
    {
        let mut g = 12345u32;
        let mut noise = move |k: i32| {
            g = g.wrapping_mul(1664525).wrapping_add(1013904223);
            ((g >> 16) % (2 * k as u32 + 1)) as i32 - k
        };
        let mut runs: Vec<(&str, Vec<(f32, u8, &'static str)>)> = vec![];
        runs.push(("flat", (0..340).map(|_| (0.5, 4, "V")).collect()));
        runs.push(("noise", (0..340).map(|_| (0.5 + noise(2) as f32 * 1e-4, 4, "V")).collect()));
        runs.push(("step", (0..340).map(|i| (if i < 150 { 0.5 } else { 1.0 }, 4, "V")).collect()));
        runs.push(("square", (0..340).map(|i| (if (i / 20) % 2 == 0 { 0.5 } else { 1.5 }, 4, "V")).collect()));
        // A spike that scrolls out: the range grows, then shrinks back.
        runs.push(("spike", (0..400).map(|i| (if i == 30 { 3.0 } else { 0.5 + noise(2) as f32 * 1e-4 }, 4, "V")).collect()));
        let mut uc: Vec<(f32, u8, &'static str)> = (0..330).map(|_| (0.5 + noise(2) as f32 * 1e-4, 4, "V")).collect();
        uc.extend((0..20).map(|_| (444.44 + noise(5) as f32 * 0.01, 2, "mV")));
        runs.push(("unit", uc));
        runs.push(("dec", (0..340).map(|i| (0.5 + noise(2) as f32 * 1e-4, if i < 325 { 4 } else { 3 }, "V")).collect()));
        let rc = ui::RateChip { rate: 1, period_ms: 106, applies: true };
        for (name, vals) in &runs {
            let mut cc = ui::ChartCache::new();
            let mut pts: Vec<f32> = vec![];
            let mut unit = vals[0].2;
            let (mut words, mut wins, mut steps) = (0u64, 0u64, 0u64);
            let mut bad = 0;
            for (i, &(v, dec, u)) in vals.iter().enumerate() {
                if u != unit {
                    unit = u;
                    pts.clear();
                }
                if pts.len() == ui::CHART_W as usize {
                    pts.remove(0);
                }
                pts.push(v);
                let side = ui::Side { max: Some(v), avg: Some(v), min: Some(v), rel: None, secs: Some(i as u32), rec: false };
                if i == 0 {
                    host::reset(0);
                }
                emu().bus = 0;
                emu().wins = 0;
                ui::chart(&mut cc, &pts, dec, 1, Some(&side), unit, rc);
                if i > 0 {
                    words += emu().bus;
                    wins += emu().wins;
                    steps += 1;
                }
                // Every 7th point (and the last ones): compare with a fresh chart.
                if i % 7 == 0 || i + 3 >= vals.len() {
                    let got = emu().fb.clone();
                    host::reset(0);
                    ui::chart(&mut ui::ChartCache::new(), &pts, dec, 1, Some(&side), unit, rc);
                    ck.cases += 1;
                    let diff = (0..W * H).filter(|&k| got[k] != emu().fb[k]).count();
                    if diff > 0 && bad == 0 {
                        ck.fail(&format!("chart/{name}/{i}"), format!("{diff} stale pixels after an incremental point"));
                        bad += 1;
                    }
                    // Continue from the fresh frame (same state as `got`).
                    emu().fb.copy_from_slice(&got);
                }
            }
            let n = steps.max(1) as f64;
            println!("dmm chart budget: {name}: {:.0} bus words / point ({:.1} windows; {:.3} ms at 25 ns)", words as f64 / n, wins as f64 / n, words as f64 / n * 25e-6);
        }
    }

    // 6. Generator page bar, menu-page title bars.
    for on in [false, true] {
        w.gen.s.on = on;
        host::reset(0);
        ui::gen_static(&w.gen);
        // Bar status (ui-review #9), worst case: 99:59:59, REC, lock, BT.
        let mut st = status(255);
        st.locked = on;
        ui::gen_status(&st);
        if on {
            save(&out, "g05-genbar-status");
        }
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
            ui::keypad_input(true, "1234567", "KHz", false);
            ck.check_opts("gen/keypad-freq-input", &gen_regions, false);
            // Prefilled current value, selected (ui-review #10).
            ui::keypad_input(true, "2", "MHz", true);
            ck.check_opts("gen/keypad-freq-sel", &gen_regions, false);
            ui::keypad(ui::KP_FREQ, "Frequency", "2", "MHz", true);
            save(&out, "g03-keypad-freq-sel");
            host::reset(0);
            ui::gen_static(&w.gen);
            ui::keypad(ui::KP_NUM, "Vpp", "3.00", "V", true);
            // Key '5' held (ui-review #11 pressed feedback).
            ui::keypad_key(ui::KP_NUM, b'5', true);
            save(&out, "g04-keypad-vpp-sel");
        }
    }
    w.gen.apply(gen::Settings::DEFAULT);
    for title in ["Hold & Limits: Temperature", "Settings", "Hold & Limits: Capacitance"] {
        host::reset(0);
        ui::list_page(title, &ui::LIMIT_ROWS, &["ON"; 9]);
        let keep = 40 * W;
        for p in emu().owner[keep..].iter_mut() {
            *p = 0;
        }
        ck.check(&format!("title/{title}"), &[("screen", Bx::new(0, 0, 480, 320)), ("bar", Bx::new(0, 0, 480, 40))]);
    }

    // 5e. Power meter (dmm::pw, ui::pw): the DMM page with the power boxes
    //     and the energy column over every worst case x hold x alert; an
    //     update from each case to each other equals its fresh draw; the
    //     entry dialog; the Power Settings page in several states.
    {
        let pcs = pw_cases();
        let rc = ui::RateChip { rate: 1, period_ms: 430, applies: false };
        for (name, pv) in &pcs {
            for hold in 0..4u8 {
                for a in ALERTS {
                    host::reset(0);
                    power_page(*pv, hold, a, rc);
                    ck.check(&format!("power/{name}/h{hold}/a{}", a.name()), &dmm_page_regions);
                }
            }
        }
        for (na, a) in &pcs {
            for (nb, b) in &pcs {
                host::reset(0);
                let ra = pw_rd(*a);
                let rb = pw_rd(*b);
                let mut dv = ui::DmmView::new();
                dv.power = true;
                // (A method change redraws the page in the app: pw_tick.)
                ui::set_power_dc(b.dc);
                dv.draw_static(Button::Power, ra.mode, true, false);
                let mut pc = ui::pw::PwCache::new();
                ui::pw::side_clear(&mut pc);
                dv.reading(&ra, 0, 0);
                ui::pw::side(&mut pc, a);
                dv.reading(&rb, 0, 0);
                ui::pw::side(&mut pc, b);
                let inc = emu().fb.clone();
                host::reset(0);
                let mut dv = ui::DmmView::new();
                dv.power = true;
                ui::set_power_dc(b.dc);
                dv.draw_static(Button::Power, rb.mode, true, false);
                let mut pc = ui::pw::PwCache::new();
                ui::pw::side_clear(&mut pc);
                dv.reading(&rb, 0, 0);
                ui::pw::side(&mut pc, b);
                let want = emu().fb.clone();
                emu().fb.copy_from_slice(&inc);
                ck.same_as(&format!("power-update/{na}->{nb}"), &want);
            }
        }
        // The W limits' badge (OK / HI / LO) in the AUTO label's slot,
        // in every case and coupling; then every verdict change as an
        // update (badge <-> AUTO label) equal to its fresh draw.
        let lims = [meter::V_NONE, meter::V_PASS, meter::V_HI, meter::V_LO];
        for (name, pv) in &pcs {
            for coup in [dmm::pw::COUP_AC, dmm::pw::COUP_AUTO] {
                for lim in &lims[1..] {
                    host::reset(0);
                    power_page_lim(dmm::pw::PowerVals { coup, ..*pv }, 0, sentinel::Alert::None, rc, *lim);
                    ck.check(&format!("power-lim/{name}/c{coup}/v{lim}"), &dmm_page_regions);
                }
                for la in lims {
                    for lb in lims {
                        let pv = dmm::pw::PowerVals { coup, ..*pv };
                        let r = pw_rd(pv);
                        host::reset(0);
                        let mut dv = ui::DmmView::new();
                        dv.power = true;
                        ui::set_power_dc(pv.dc);
                        dv.draw_static(Button::Power, r.mode, true, false);
                        dv.reading(&r, 0, la);
                        dv.reading(&r, 0, lb);
                        let inc = emu().fb.clone();
                        host::reset(0);
                        let mut dv = ui::DmmView::new();
                        dv.power = true;
                        ui::set_power_dc(pv.dc);
                        dv.draw_static(Button::Power, r.mode, true, false);
                        dv.reading(&r, 0, lb);
                        let want = emu().fb.clone();
                        emu().fb.copy_from_slice(&inc);
                        ck.same_as(&format!("power-lim-update/{name}/c{coup}/{la}->{lb}"), &want);
                    }
                }
            }
        }
        host::reset(0);
        power_page_lim(dmm::pw::PowerVals { coup: dmm::pw::COUP_AUTO, ..pcs[0].1 }, 0, sentinel::Alert::None, rc, meter::V_HI);
        save(&out, "p17-power-limit-hi");
        // Hold & Limits of the power meter (the W limits; widest values).
        for (hi, lo) in [("10600 W", "-10600 W"), ("-9999.99 mW", "-63.650 W")] {
            host::reset(0);
            ui::list_page("Hold & Limits: Power", &ui::LIMIT_ROWS, &["ON", "ON", hi, lo, "", "", "", "ON", "Every reading"]);
            ck.check_opts(&format!("limits-power/{hi}"), &[("screen", Bx::new(0, 0, 480, 320)), ("bar", Bx::new(0, 0, 480, 40))], false);
        }
        save(&out, "p18-power-limits-page");
        for (ic, tl) in [(5.0f32, 30u16), (10.0, 0), (0.5, 3600)] {
            host::reset(0);
            ui::pw::entry_dialog(ic, tl);
            let (x, y, w2, h2) = ui::pw::DLG;
            ck.check(&format!("power-dialog/{ic}/{tl}"), &[("screen", Bx::new(0, 0, 480, 320)), ("dialog", Bx::new(x as i32, y as i32, w2 as i32, h2 as i32))]);
        }
        for (name, pv) in &pcs {
            for (va, ia) in [(true, true), (false, false), (true, false)] {
                host::reset(0);
                ui::pw::settings_page(&dmm::pw::PowerVals { v_auto: va, i_auto: ia, ..*pv });
                ck.check_opts(&format!("power-settings/{name}/{va}/{ia}"), &page_regions_pw(), false);
            }
        }
        // The settings page's status line in every job state (the longest
        // texts: the V cal result / refusals on 1000V).
        for (k, (job, last, res)) in pw_status_cases().iter().enumerate() {
            host::reset(0);
            ui::pw::settings_page(&pcs[0].1);
            ui::pw::settings_status(&pw_status_pw(*job, *last, *res), 0);
            ck.check_opts(&format!("power-settings-status/{k}"), &page_regions_pw(), false);
        }
        // Before the first full reading: V + "I ranging..." (and the job
        // texts) over the power page's static parts.
        for txt in ["I ranging...", "Zeroing: leads open", "V calibration..."] {
            for v in [Some(dmm::Val { v: -999.99, dec: 2, unit: "V", ol: false, ext: false }), None] {
                host::reset(0);
                let mut dv = ui::DmmView::new();
                dv.power = true;
                dv.draw_static(Button::Power, 4, true, false);
                ui::pw::ranging(v, txt, 0);
                ck.check_opts(&format!("power-ranging/{txt}/{}", v.is_some()), &page_regions_pw(), false);
            }
        }
    }

    // 6b. Settings pages 1 / 2 (worst values), Range pages with the UF-21
    //     note: whole page.
    let page_regions = vec![("screen", Bx::new(0, 0, 480, 320)), ("bar", Bx::new(0, 0, 480, 40))];
    for (page, vals) in [
        (0u8, vec!["10 min", "1 hour", "100%", "100%", "OFF", "DMM + Scope", "Auto (per view)", "Hold 5000 ms", ""]),
        (1, vec!["SLOW (n/a here)", "HIGH", "Bargraph", "F (Fahrenheit)", "Top bar", "OFF (stock)", "Halves (stock)", "", ""]),
        (1, vec!["FAST  37/s", "LOW", "History", "C (Celsius)", "Off (stock)", "ON", "Corners", "", ""]),
        (1, vec!["MED  --/s", "OFF", "Bargraph", "C (Celsius)", "Off (stock)", "ON", "Off", "", ""]),
        // Page 3 with each "USB disk" value (decision 2026-10-22 #6).
        (2, vec!["A-JACK only", ui::usb_disk_name(true, false), ""]),
        (2, vec!["ON", ui::usb_disk_name(false, true), ""]),
        (2, vec!["Mask 5", ui::usb_disk_name(false, false), ""]),
        (2, vec!["OFF", ui::usb_disk_name(true, true), ""]),
    ] {
        host::reset(0);
        ui::settings(page, &vals);
        ck.check_opts(&format!("settings/{page}/{}", vals[0]), &page_regions, false);
    }
    // Hold & Limits with its ninth row "Record" (decision 2026-10-22 #1),
    // the widest limit values.
    for rec in ui::REC_INTERVALS {
        host::reset(0);
        ui::list_page("Hold & Limits: Diode", &ui::LIMIT_ROWS, &["OFF", "OFF", "-99999.9 mA", "-99999.9 mA", "", "", "", "OFF", rec]);
        ck.check_opts(&format!("limits/{rec}"), &page_regions, false);
        if rec == "Every reading" {
            save(&out, "m11-hold-limits");
        }
    }
    // Record page: every interval x duration value in the steppers (they
    // fit the 134 px between the keys), the estimate texts, Start / Stop.
    for (k, (iv, du, free, on)) in [
        ("Every reading", "Until stopped", "about 1193 h (limit)", false),
        ("1 min", "24 h", "about 62 h left", true),
        ("10 s", "10 min", "-- (disk busy)", false),
        ("1 s", "8 h", "< 1 h left", true),
        ("Every reading", "1 h", "-- (no disk)", false),
    ]
    .into_iter()
    .enumerate()
    {
        host::reset(0);
        ui::record_page([iv, du], free, on);
        ck.check_opts(&format!("record/{iv}/{du}/{free}/{on}"), &page_regions, false);
        if k < 2 {
            save(&out, &format!("m12-record-{}", if on { "stop" } else { "start" }));
        }
    }
    for t in ui::REC_INTERVALS.iter().chain(ui::REC_DURATIONS.iter()) {
        ck.cases += 1;
        let w = stockfont::text_width(stockfont::TEXT22, t, 0);
        let vw = ui::step_value_w(ui::REC_STEP_MINUS_X);
        if w > vw {
            ck.fail("record/stepper", format!("too wide: {t:?} {w} > {vw}"));
        }
    }
    for b in [Button::Volt, Button::Amp, Button::Ohm] {
        for note in [None, Some((dmm::MV_AUTO_WARN, true)), Some(("AUTO+ recommended: it never steps to mV", false))] {
            for sub in 0..3u8 {
                host::reset(0);
                let n = ui::range_items(b).len();
                ui::range_page(b, Some(n - 1), sub, note);
                // The wave view's entry (builds with the AD2 stream ISR).
                if b != Button::Ohm {
                    ui::wave_button();
                }
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

    // 6g. Temperature settings page: sensors, unit, offset stepper (value
    //     extremes, both signs), NTC user R25 / Beta fields, info lines;
    //     every key drawn pressed equals its own box (no stale pixels).
    for (k, offs, unit_f, r25, beta, info) in [
        (0u8, -20.0f32, false, 10_000.0f32, 3950.0f32, "Cold junction -44.4 C"),
        (3, 20.0, true, 10_000.0, 3950.0, "Sensor current ~0.850 mA"),
        (6, 0.0, false, 10_000_000.0, 10_000.0, "Sensor current ~0.000 mA"),
        (6, -13.7, true, 100.0, 1000.0, ""),
        (6, 7.3, false, 4_700.0, 3977.0, "Sensor current ~0.008 mA"),
        (1, -0.1, false, 10_000.0, 3950.0, "Sensor current ~0.850 mA"),
        (2, 0.1, true, 10_000.0, 3950.0, ""),
        (5, 7.3, false, 10_000.0, 3950.0, "Sensor current ~0.008 mA"),
    ] {
        let tp = ui::TempPage { sensor: k, offs, unit_f, r25, beta, info };
        host::reset(0);
        ui::temp_page(&tp);
        ck.check_opts(&format!("temp/{k}/{offs}/{r25}"), &page_regions, false);
        let before: Vec<u16> = emu().fb.clone();
        for code in [0u8, 6, ui::TK_UNIT, ui::TK_MINUS, ui::TK_OFFS, ui::TK_PLUS, ui::TK_R25, ui::TK_BETA, ui::TK_LIMITS, ui::TK_OK] {
            ui::temp_key(&tp, code, true);
            ui::temp_key(&tp, code, false);
        }
        ck.cases += 1;
        if emu().fb != before {
            ck.fail(&format!("temp/{k}/{offs}"), "pressed / released key differs from a full draw".into());
        }
        if k == 5 {
            save(&out, "m06-temp-offset");
        }
        if k == 6 && offs > 0.0 {
            save(&out, "m08-temp-ntc-user");
            ui::temp_key(&tp, ui::TK_PLUS, true);
            save(&out, "m09-temp-pressed");
        }
    }

    // 6f. Help pages (UF-29): whole page; every text fits its column.
    for page in 0..ui::HELP.len() {
        host::reset(0);
        ui::help_page(page);
        ck.check_opts(&format!("help/{page}"), &page_regions, false);
        for (a, b) in ui::HELP[page].1.iter() {
            ck.cases += 1;
            let (wa, wb) = (stockfont::text_width(stockfont::TEXT22, a, 0), stockfont::text_width(stockfont::TEXT22, b, 0));
            if wa > ui::HELP_COL - 4 || wb > 464 - 15 - ui::HELP_COL {
                ck.fail(&format!("help/{page}"), format!("too wide: {a:?} {wa} / {b:?} {wb}"));
            }
        }
        save(&out, &format!("m05-help{page}"));
    }

    // 6e. Confirm dialogs (dialog.rs, one box and Yes / No): power off,
    // energy reset, the firmware install question (vwrite.rs).
    for (k, f) in [("power-off", dialog::power_off as fn()), ("energy-reset", dialog::energy_reset), ("fw-install", dialog::fw_install)] {
        host::reset(0);
        f();
        let (x, y, w2, h2) = dialog::BOX;
        // The stock confirm draws ASCII26 at spacing 0, where its glyphs
        // touch by design (as the menu rows): gaps and margins checked.
        ck.check_opts(&format!("confirm/{k}"), &[("screen", Bx::new(0, 0, 480, 320)), ("dialog", Bx::new(x as i32, y as i32, w2 as i32, h2 as i32))], false);
        if k == "fw-install" {
            save(&out, "t04-confirm-fw-install");
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
        ("Saved LOG000.DLG", true),
        ("Disk full: log ended", false),
        ("No reading to log", false),
        ("Recording CSV", true),
        ("Screenshot failed", false),
        (dialog::AUTO_DC, true),
        ("Auto power-off in 30 s", false),
        // Tap on the DMM value (resolution / rate), widest variants.
        ("SLOW 600000 counts / 12.8 s", true),
        ("FAST 60000 counts / 10/s", true),
        ("Fixed rate in this function", false),
        // Wrong-connection alerts (sentinel.rs).
        (sentinel::Alert::AJack.toast(), false),
        (sentinel::Alert::VJack.toast(), false),
        (sentinel::Alert::Live.toast(), false),
        (sentinel::Alert::Overload.toast(), false),
        // Firmware file on the USB disk (vwrite.rs, app.rs).
        (dialog::FW_BAD, false),
        ("Installing firmware...", true),
        // Settings 3/3 "USB disk" (decision 2026-10-22 #6).
        ("Re-plug USB to apply", true),
        (sentinel::Alert::Dc.toast(), false),
        (sentinel::Alert::Ac.toast(), false),
        (sentinel::Alert::Fuse.toast(), false),
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
        if k == 11 {
            save(&out, "t03-toast-ajack");
        }
        if k == 8 {
            save(&out, "t02-toast-resolution");
        }
    }

    // 6c'. Fuse-check dialog (ui::fuse_screen): every stage and verdict,
    //      the widest resistance texts; each line fits the box.
    {
        use sentinel::FuseRes as F;
        for (k, (stage, res)) in [
            (0u8, F::Idle),
            (1, F::Busy),
            (2, F::Ok(48.12)),
            (2, F::Open),
            (2, F::Odd(0.25)),
            (2, F::Odd(599.9)),
            (2, F::NoReading),
        ]
        .iter()
        .enumerate()
        {
            host::reset(0);
            let mut b = [0u8; 32];
            ui::fuse_screen(*stage, *res, &mut b);
            let (x, y, w2, h2) = dialog::FBOX;
            ck.check(&format!("fuse/{k}"), &[("screen", Bx::new(0, 0, 480, 320)), ("box", Bx::new(x as i32, y as i32, w2 as i32, h2 as i32))]);
            save(&out, &format!("f0{k}-fuse-check"));
        }
        for t in ["Lead from V/ohm jack to A", "nothing else connected", "OL: no path from V to A", "Replace: 10 A ceramic fuse", "One lead V/ohm to A only", "599.9 ohm, expect ~48"] {
            ck.cases += 1;
            let w = stockfont::text_width(stockfont::ASCII26, t, 2);
            if w > dialog::FBOX.2 as i16 - 16 {
                ck.fail("fuse/width", format!("{t:?} {w} px"));
            }
        }
    }

    // 6d. Data-logger panel (FR-38, dialog::log_panel) over the DMM page:
    //     the widest rows (AD2 measured / nominal, a big loss), AD1 and the
    //     nominal-only start.
    for (k, info) in [
        dialog::LogInfo { hz: 0, n: 0, nominal_hz: 40, ad2: false, lost: 0 },
        dialog::LogInfo { hz: 9599, n: 4_294_967_295, nominal_hz: 9600, ad2: true, lost: 4_000_000_000 },
        dialog::LogInfo { hz: 0, n: 0, nominal_hz: 600, ad2: true, lost: 0 },
        dialog::LogInfo { hz: 638, n: 123_456, nominal_hz: 640, ad2: false, lost: 17 },
    ]
    .iter()
    .enumerate()
    {
        host::reset(0);
        dialog::log_panel_reset();
        for i in 0..5 {
            dialog::log_panel(i, *info);
        }
        ck.check(&format!("log-panel/{k}"), &[("screen", Bx::new(0, 0, 480, 320))]);
        if k == 1 {
            save(&out, "t03-log-panel-ad2");
        }
    }
    dialog::log_panel_reset();

    // 7. Gestures: zone table, axis lock, hold-to-grab, Up on captured
    //    touches (gesture.rs through its DBG:DRAG hook).
    {
        use gesture::{Axis, Gesture as G};
        let mut t = |name: &str, ok: bool| {
            ck.cases += 1;
            if !ok {
                ck.fail(&format!("gesture/{name}"), "unexpected result".into());
            }
        };
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
        ("c10-band", "dcma_420mid", 0, 0, false, false),
    ];
    for (file, case, hold, lim, rel, alt) in scenes {
        let (r, l) = find(case);
        let mut r = r;
        if case == "hz_mhz" {
            r.auto_plus = true;
        }
        combined(&r, l, hold, lim, rel, alt, file == "c10-band", if file == "c01-dcv" { "1.00kHz" } else { "" });
        save(&out, file);
    }
    {
        host::reset(0);
        let (r, _) = find("dcv");
        dv.label = None;
        dv.draw_static(Button::Volt, 1, false, false);
        ui::history(&[(4.8170, 4, "V"); 5], 3, 2);
        dv.reading(&Reading { main: Val { ext: true, ..r.main }, ..r }, 3, meter::V_PASS);
        ui::status_right(&status(0));
        let pts: Vec<f32> = (0..321).map(|i| 4.8170 + ((i / 40) % 3) as f32 * 1e-4).collect();
        let side = ui::Side { max: Some(4.8172), avg: Some(4.8171), min: Some(4.8169), rel: None, secs: Some(5), rec: false };
        ui::chart(&mut ui::ChartCache::new(), &pts, 4, 2, Some(&side), "V", ui::RateChip { rate: 1, period_ms: 106, applies: true });
        save(&out, "d01-dmm");
        host::reset(0);
        let (r, l) = find("acdc_600mv");
        dv.label = l;
        dv.draw_static(Button::Volt, r.mode, r.acdc, false);
        ui::history(&[(-600.00, 2, "mV"); 5], 5, 4);
        dv.reading(&r, 1, meter::V_HI);
        ui::status_right(&status(0));
        let pts: Vec<f32> = (0..321).map(|i| -600.0 + (i % 64) as f32 * 0.5).collect();
        let side = ui::Side { max: Some(-568.50), avg: Some(-584.25), min: Some(-600.00), rel: Some(-501.67), secs: Some(99 * 3600 + 59 * 60 + 59), rec: true };
        ui::chart(&mut ui::ChartCache::new(), &pts, 2, 4, Some(&side), "mV", ui::RateChip { rate: 0, period_ms: 430, applies: false });
        save(&out, "d02-dmm-acdc");
        host::reset(0);
        let (r, _) = find("dcma_420mid");
        dv.label = None;
        dv.draw_static(Button::Amp, r.mode, false, false);
        dv.reading(&r, 0, 0);
        ui::status_right(&status(0));
        save(&out, "d03-dmm-420ma");
        for (case, file) in [("temp_ntc", "d04-dmm-temp-ntc"), ("temp_tc", "d05-dmm-temp-tc"), ("temp_pt_f", "d10-dmm-temp-f")] {
            host::reset(0);
            let (r, l) = find(case);
            dv.label = l;
            dv.draw_static(Button::HzTemp, r.mode, false, false);
            dv.reading(&r, 0, 0);
            ui::status_right(&status(0));
            save(&out, file);
        }
        dv.label = None;
        host::reset(0);
        ui::gen_static(&w.gen);
        save(&out, "g01-gen");
        host::reset(0);
        ui::settings(0, &["5 min", "30 min", "50%", "50%", "ON", "Multimeter", "Auto (per view)", "Hold 300 ms", ""]);
        save(&out, "m01-settings");
        host::reset(0);
        ui::settings(1, &["SLOW (n/a here)", "HIGH", "Bargraph", "F (Fahrenheit)", "Top bar", "OFF (stock)", "Halves (stock)", "", ""]);
        save(&out, "m02-settings2");
        host::reset(0);
        ui::settings(2, &["A-JACK only", ui::usb_disk_name(true, false), ""]);
        save(&out, "m10-settings3");
        // Pressed feedback (ui-review #11): the item drawn pressed from
        // touch-down (one item redrawn), then released = the page as drawn.
        {
            let combo = ui::view_name(ui::View::DmmScope);
            let s0 = ["5 min", "30 min", "50%", "50%", "ON", "Multimeter", "Auto (per view)", "Hold 300 ms", ""];
            let lim = ["ON", "ON", "1.000 V", "--", "", "", "", "OFF", "1 s"];
            let row = |r: u8, part: i8, on: bool, f: &dyn Fn()| {
                ui::set_row_only(Some(ui::RowPress { row: r, part, on }));
                f();
                ui::set_row_only(None);
            };
            let d_menu = || ui::main_menu(combo);
            let d_set = || ui::settings(0, &s0);
            let d_set2 = || ui::settings(1, &["SLOW (n/a here)", "HIGH", "Bargraph", "F (Fahrenheit)", "Top bar", "OFF (stock)", "Halves (stock)", "", ""]);
            let d_set3 = || ui::settings(2, &["A-JACK only", ui::usb_disk_name(false, false), ""]);
            let d_lim = || ui::list_page("Hold & Limits: Voltage", &ui::LIMIT_ROWS, &lim);
            let d_rec = || ui::record_page(["1 s", "8 h"], "about 62 h left", false);
            let d_recon = || ui::record_page(["1 s", "8 h"], "about 62 h left", true);
            let g = &w.gen;
            let d_gen = || ui::gen_static(g);
            let cases: Vec<(String, &dyn Fn(), Box<dyn Fn(bool) + '_>)> = (0..6)
                .map(|i| (format!("menu{i}"), &d_menu as &dyn Fn(), Box::new(move |on| ui::menu_item(i, combo, on)) as Box<dyn Fn(bool)>))
                .chain((0..9u8).map(|r| {
                    let part = if ui::SET_STEP_ROWS.contains(&(r as usize)) { 1 } else { 0 };
                    (format!("settings{r}/{part}"), &d_set as &dyn Fn(), Box::new(move |on| row(r, part, on, &d_set)) as Box<dyn Fn(bool)>)
                }))
                .chain([(2u8, -1i8), (3, -1)].into_iter().map(|(r, part)| {
                    (format!("settings{r}/{part}"), &d_set as &dyn Fn(), Box::new(move |on| row(r, part, on, &d_set)) as Box<dyn Fn(bool)>)
                }))
                .chain((0..9u8).map(|r| (format!("settings2-{r}"), &d_set2 as &dyn Fn(), Box::new(move |on| row(r, 0, on, &d_set2)) as Box<dyn Fn(bool)>)))
                .chain((0..3u8).map(|r| (format!("settings3-{r}"), &d_set3 as &dyn Fn(), Box::new(move |on| row(r, 0, on, &d_set3)) as Box<dyn Fn(bool)>)))
                .chain((0..9u8).map(|r| (format!("limits{r}"), &d_lim as &dyn Fn(), Box::new(move |on| row(r, 0, on, &d_lim)) as Box<dyn Fn(bool)>)))
                .chain([(0u8, -1i8), (0, 1), (1, -1), (1, 1), (3, 0)].into_iter().flat_map(|(r, part)| {
                    [
                        (format!("record{r}/{part}"), &d_rec as &dyn Fn(), Box::new(move |on| row(r, part, on, &d_rec)) as Box<dyn Fn(bool)>),
                        (format!("record-on{r}/{part}"), &d_recon as &dyn Fn(), Box::new(move |on| row(r, part, on, &d_recon)) as Box<dyn Fn(bool)>),
                    ]
                }))
                .chain((0..5).map(|k| (format!("gen{k}"), &d_gen as &dyn Fn(), Box::new(move |on| ui::gen_press(g, k, on)) as Box<dyn Fn(bool)>)))
                .collect();
            for (name, draw, press) in cases.iter() {
                host::reset(0);
                draw();
                let want = emu().fb.clone();
                press(true);
                ck.check_opts(&format!("pressed/{name}"), &[("screen", Bx::new(0, 0, 480, 320))], false);
                let file = match name.as_str() {
                    "menu1" => "m04b-menu-pressed",
                    "settings4/0" => "m01b-settings-row-pressed",
                    "settings2/1" => "m01c-settings-step-pressed",
                    "record3/0" => "m12b-record-start-pressed",
                    "gen1" => "g01b-gen-box-pressed",
                    "gen4" => "g01c-gen-wave-pressed",
                    _ => "",
                };
                if !file.is_empty() {
                    save(&out, file);
                }
                press(false);
                ck.same_as(&format!("pressed-release/{name}"), &want);
            }
        }
        host::reset(0);
        ui::range_page(Button::Volt, Some(6), 0, Some((dmm::MV_AUTO_WARN, true)));
        save(&out, "m03-range-v-warn");
        host::reset(0);
        ui::range_page(Button::Amp, Some(7), 1, None);
        ui::wave_button();
        save(&out, "m03b-range-a-wave");
        // Second ohm mode ("ON" low current): Resistor Settings with it
        // selected, the DMM page with the tag on the ohm button.
        host::reset(0);
        ui::range_page(Button::Ohm, Some(7), 1, None);
        save(&out, "m07-range-ohm-on");
        host::reset(0);
        let (r, _) = find("ohm_k");
        dv.label = None;
        dv.draw_static(Button::Ohm, r.mode, false, true);
        dv.reading(&r, 0, 0);
        ui::status_right(&status(0));
        save(&out, "d06-dmm-ohm-on");
        // ui-review #13: the stock >= 30 V red value (230 V AC).
        host::reset(0);
        let (r, _) = find("acv_230");
        let mut dv = ui::DmmView::new();
        dv.hv_red = true;
        dv.draw_static(Button::Volt, r.mode, false, false);
        dv.bar_update(&r);
        dv.reading(&r, 0, 0);
        ui::status_right(&status(0));
        ui::hv_warning(true);
        save(&out, "d11-dmm-hv-red");
        // Wrong-connection alerts in the status slot (sentinel.rs): A-JACK in
        // DC V (red), V-JACK? in DC mA, LIVE? in ohm; the combined strip badge.
        for (case, file, b, a) in [
            ("dcv", "d12-dmm-ajack", Button::Volt, sentinel::Alert::AJack),
            ("dcma_420lo", "d13-dmm-vjack", Button::Amp, sentinel::Alert::VJack),
            ("ohm_k", "d14-dmm-live", Button::Ohm, sentinel::Alert::Live),
            ("dca10", "d17-dmm-overload", Button::Amp, sentinel::Alert::Overload),
            ("dcv", "d18-dmm-achint", Button::Volt, sentinel::Alert::Ac),
            ("dcma_420lo", "d19-dmm-fusehint", Button::Amp, sentinel::Alert::Fuse),
        ] {
            host::reset(0);
            let (mut r, _) = find(case);
            r.alert = a;
            let mut dv = ui::DmmView::new();
            dv.draw_static(b, r.mode, false, false);
            dv.bar_update(&r);
            dv.reading(&r, 0, 0);
            ui::status_right(&status(0));
            save(&out, file);
        }
        {
            let (mut r, l) = find("acv_230");
            r.alert = sentinel::Alert::AJack;
            combined(&r, l, 0, 0, false, false, false, "");
            save(&out, "c11-strip-ajack");
            // HOLD with A-JACK: both phases (decision 2026-10-22 #5).
            combined(&r, l, 1, 0, false, true, false, "");
            save(&out, "c13-strip-hold-ajack-p1");
            combined(&r, l, 1, 0, false, false, false, "");
            save(&out, "c14-strip-hold-ajack-p0");
            let (mut r, l) = find("dca10");
            r.alert = sentinel::Alert::Overload;
            combined(&r, l, 0, 0, false, false, false, "");
            save(&out, "c15-strip-overload");
        }
        // DMM status slot: HOLD / A-HOLD with A-JACK / LIVE?, both phases.
        for (case, file, b, a, hold) in [
            ("dcv", "d15-dmm-hold-ajack", Button::Volt, sentinel::Alert::AJack, 1u8),
            ("ohm_k", "d16-dmm-ahold-live", Button::Ohm, sentinel::Alert::Live, 3),
        ] {
            for alt in [true, false] {
                host::reset(0);
                let (mut r, _) = find(case);
                r.alert = a;
                let mut dv = ui::DmmView::new();
                dv.alt = alt;
                dv.draw_static(b, r.mode, false, false);
                dv.bar_update(&r);
                dv.reading(&r, hold, 0);
                ui::status_right(&status(0));
                save(&out, &format!("{file}-p{}", alt as u8));
            }
        }
        // UF-30 bargraph: bipolar DC V (negative), unipolar ohm, overload.
        // Continuity / diode: their OL ceilings as full scale (1000 ohm, 3 V).
        for (case, file, b) in [("dcv_neg6", "d07-dmm-bar-dcv", Button::Volt), ("ohm_k", "d08-dmm-bar-ohm", Button::Ohm), ("dcv_ol", "d09-dmm-bar-ol", Button::Volt), ("cont", "d18-dmm-bar-cont", Button::ContDiode), ("diode", "d19-dmm-bar-diode", Button::ContDiode)] {
            host::reset(0);
            let (r, _) = find(case);
            let mut dv = ui::DmmView::new();
            dv.draw_static(b, r.mode, false, false);
            dv.bar_update(&r);
            dv.reading(&r, 0, 0);
            ui::status_right(&status(0));
            let side = ui::Side { max: None, avg: None, min: None, rel: None, secs: None, rec: false };
            ui::chart(&mut ui::ChartCache::new(), &[], r.main.dec, 1, Some(&side), r.main.unit, ui::RateChip { rate: 1, period_ms: 100, applies: true });
            save(&out, file);
        }
    }

    // Power meter scenes.
    {
        let pcs = pw_cases();
        let rc = ui::RateChip { rate: 1, period_ms: 430, applies: false };
        for (file, k, hold) in [("p01-power-mains", 0usize, 0u8), ("p02-power-worst", 1, 1), ("p03-power-mw-over", 2, 0), ("p04-power-alt", 4, 0), ("p05-power-dc-source", 5, 0), ("p11-power-dc-ad1", 6, 0), ("p12-power-main-wh", 7, 0), ("p13-power-main-vah", 8, 0), ("p15-power-zero", 10, 0), ("p16-power-main-zero", 11, 0)] {
            host::reset(0);
            power_page(pcs[k].1, hold, sentinel::Alert::None, rc);
            save(&out, file);
        }
        host::reset(0);
        power_page(pcs[0].1, 0, sentinel::Alert::None, rc);
        ui::pw::entry_dialog(5.0, 30);
        save(&out, "p06-power-entry");
        host::reset(0);
        power_page(pcs[0].1, 0, sentinel::Alert::None, rc);
        crate::dialog::energy_reset();
        save(&out, "p14-power-energy-reset");
        host::reset(0);
        ui::pw::settings_page(&dmm::pw::PowerVals { v_auto: false, i_auto: true, ..pcs[0].1 });
        ui::pw::settings_status(&pw_status_pw(0, dmm::pw::JOB_VCAL, dmm::pw::JR_OK), 0);
        save(&out, "p07-power-settings");
        host::reset(0);
        let mut dv = ui::DmmView::new();
        dv.power = true;
        dv.draw_static(Button::Power, 4, true, false);
        ui::pw::ranging(Some(dmm::Val { v: 229.97, dec: 2, unit: "V", ol: false, ext: false }), "I ranging...", 0);
        save(&out, "p09-power-ranging");
        host::reset(0);
        ui::pw::settings_page(&pcs[0].1);
        ui::pw::settings_status(&pw_status_pw(0, dmm::pw::JOB_VCAL, dmm::pw::JR_BIG), 0);
        save(&out, "p10-power-settings-refused");
        host::reset(0);
        ui::range_page(Button::Amp, Some(7), 0, None);
        save(&out, "p08-range-a-power");
        combined(&pw_rd(pcs[0].1), None, 0, 0, false, false, false, "");
        save(&out, "c12-strip-power");
    }

    for (msg, (n, scene)) in &ck.seen {
        println!("{n:6} x  {msg}   [first: {scene}]");
    }
    println!("cases {}, violations {}", ck.cases, ck.violations);
    std::process::exit(if ck.violations == 0 { 0 } else { 1 });
}
