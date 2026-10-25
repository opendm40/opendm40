//! Layout-rule check of a rendered scene (ported from tools/ui-render/main.rs:
//! the owner's >= 3 px gap / margin rules). The stock layout is NOT expected
//! to pass; the violations are reported as a finding (stock/re/docs/stock-scope-ui.md).
#![allow(dead_code)]
use crate::host::{emu, H, W};
pub mod probe {
    pub const TEXT: u8 = 0;
    pub const SHAPE: u8 = 1;
    pub const ICON: u8 = 2;
}


#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bx {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Bx {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Bx {
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

pub struct Checker {
    pub cases: usize,
    pub violations: usize,
    /// Distinct messages (draw-call ids stripped): count, first scene.
    pub seen: std::collections::BTreeMap<String, (usize, String)>,
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
    pub fn check(&mut self, scene: &str, regions: &[(&str, Bx)]) {
        self.check_opts(scene, regions, true)
    }

    /// `glyph_rule` false: skip "glyphs of one text do not touch" (the menu
    /// pages' stock TEXT22 rows are drawn at spacing 0, where that font's
    /// glyphs touch by design; gaps and margins are still checked).
    pub fn check_opts(&mut self, scene: &str, regions: &[(&str, Bx)], glyph_rule: bool) {
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
            if glyph_rule && ia.kind == probe::TEXT && ia.glyphs.len() > 1 {
                let mut g = ia.glyphs.clone();
                g.sort_by_key(|(k, _)| *k);
                for w in g.windows(2) {
                    let (a, b) = (w[0].1, w[1].1);
                    if a.gap(&b) < 1 {
                        self.fail(scene, format!("{}: glyphs {} / {} touch ({:?} {:?})", desc(ia), w[0].0, w[1].0, a, b));
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
