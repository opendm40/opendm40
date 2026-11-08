//! The digital phosphor (src/scope_dpo.rs, Display > Persist DPO / HOT): a
//! noisy sine with an occasional larger swing and a square wave with edge
//! jitter, ringing and a rare runt, 600 frames each through
//! `scope_dpo::frame` and drawn by `Graph::flush` frame by frame. Checks:
//! the per-cell repaint equals a fresh draw of the same counts, the counts
//! reach a spread of colour levels (the rare parts dim, the core at the
//! top), both modes; the screens for the owner (dpo-*.ppm).
use super::*;

struct Rng(u32);

impl Rng {
    fn u(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / 16_777_216.0
    }
    /// About N(0, 1) (Irwin-Hall of 4).
    fn n(&mut self) -> f32 {
        (self.u() + self.u() + self.u() + self.u() - 2.0) * 1.732
    }
}

/// One frame's CH1 rows per column (model rows, 40 per division, centre
/// 140): 1 kHz at 200 us/div (2 periods over the 400 columns), 3 Vpp at
/// 1 V/div.
fn frame_rows(kind: u8, r: &mut Rng, h: i16) -> [(i16, i16); scope_ui::NCOL] {
    let mut out = [scope_ui::EMPTY; scope_ui::NCOL];
    let (amp, ph, big, runt) = (60.0 + 1.8 * r.n(), 0.004 * r.n(), r.u() < 0.06, r.u() < 0.02);
    let y = |t: f32, r: &mut Rng| -> f32 {
        if kind == 0 {
            let a = if big { amp * 1.2 } else { amp };
            a * (core::f32::consts::TAU * 2.0 * (t + ph)).sin() + 1.6 * r.n()
        } else {
            let p = ((t + ph) * 2.0).rem_euclid(1.0);
            let mut v = if p < 0.5 { 60.0 } else { -60.0 };
            for (e0, s) in [(0.0f32, 1.0f32), (0.5, -1.0)] {
                let d = (p - e0).rem_euclid(1.0);
                if d < 0.1 {
                    v += s * 16.0 * (-d / 0.015).exp() * (core::f32::consts::TAU * d / 0.01).cos();
                }
            }
            if runt && p > 0.66 && p < 0.71 {
                v += 70.0;
            }
            v + 1.4 * r.n()
        }
    };
    let mut prev: Option<(i32, i32)> = None;
    for (j, o) in out.iter_mut().enumerate() {
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for k in 0..4 {
            let t = (j * 4 + k) as f32 / (scope_ui::NCOL * 4) as f32;
            let row = (140.0 - y(t, r)) as i32;
            lo = lo.min(row);
            hi = hi.max(row);
        }
        // Consecutive bars touch (scope_page.rs sp_frame).
        let (a, b) = prev.map_or((lo, hi), |(pa, pb)| (lo.min(pb), hi.max(pa)));
        prev = Some((lo, hi));
        *o = (scope_ui::trace_fy(h, a), scope_ui::trace_fy(h, b));
    }
    out
}

pub fn run(ck: &mut Checker, out: &str) {
    let base = &stock::scenes()[0];
    for (kind, name) in [(0u8, "sine"), (1, "square")] {
        for mode in [1u8, 2] {
            let m = base.m;
            let h = m.frame_h();
            let sc = stock::Scene { name: "dpo", m, popup: scope_ui::Popup::None, trace: None, persist: None, cols: 399, cells: base.cells.clone() };
            let fresh = || {
                host::reset(0);
                scope_ui::invalidate();
                stock::load(&sc);
                scope_ui::graph().dpo = mode;
                scope_ui::update(&m);
                scope_ui::cells(&m, &sc.cells);
            };
            scope_dpo::clear();
            scope_dpo::discard();
            fresh();
            let mut r = Rng(0x1234_5677 + kind as u32 * 77 + mode as u32);
            for _ in 0..600 {
                let tr = frame_rows(kind, &mut r, h);
                scope_dpo::frame(&tr, h);
                let g = scope_ui::graph();
                g.tr = [scope_ui::EMPTY; scope_ui::NCOL];
                g.dpo = mode;
                scope_ui::update(&m);
            }
            let inc = emu().fb.clone();
            fresh();
            let scene = format!("dpo/{name}/{}", if mode == 1 { "dpo" } else { "hot" });
            ck.same_as(&format!("{scene}/incremental"), &inc);
            ck.check_replica(&scene);
            // Colour levels on the graph.
            let cols = scope_dpo::colours(m.trace_colour(), mode == 2);
            let fb = &emu().fb;
            let mut seen = [0u32; 16];
            for y in (m.top() + 1) as usize..(m.top() + h - 1) as usize {
                for x in (scope_ui::GX + 1) as usize..(scope_ui::GX + scope_ui::NCOL as i16) as usize {
                    let p = fb[y * W + x];
                    if let Some(k) = (1..16).find(|&k| cols[k] == p) {
                        seen[k] += 1;
                    }
                }
            }
            let levels = seen.iter().filter(|&&n| n > 0).count();
            println!("{scene}: {levels} colour levels, px per count {:?}", &seen[1..]);
            if levels < 5 {
                ck.fail(&scene, format!("only {levels} phosphor colour levels on the graph"));
            }
            save(out, &format!("dpo-{name}-{}", if mode == 1 { "dpo" } else { "hot" }));
        }
    }
    scope_dpo::clear();
    scope_dpo::discard();
}
