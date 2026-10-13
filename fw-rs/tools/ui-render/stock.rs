//! Replica scope page scenes (src/scope_ui.rs) in the same states as the
//! stock reference scenes of tools/stock-ui/scope.rs, for the pixel diff
//! (tools/ui-render/stock-diff.sh): same names, same values, the same
//! synthetic traces (rows of the 280-row model, the stock column widening).
use crate::host;
use crate::scope_ui::{self as su, Model};

/// A synthetic trace: per column (min row, max row) of the 280-row model
/// (tools/stock-ui/scope.rs Trace).
#[derive(Clone)]
pub struct Trace {
    pub a: Vec<i32>,
    pub b: Vec<i32>,
}

impl Trace {
    pub fn sine(amp: f32, periods: f32, phase: f32) -> Trace {
        let (mut a, mut b) = (vec![], vec![]);
        for i in 0..400 {
            let t = (i as f32 / 400.0 + phase) * periods * core::f32::consts::TAU;
            let t1 = ((i + 1) as f32 / 400.0 + phase) * periods * core::f32::consts::TAU;
            let r0 = 140.0 - amp * 35.0 * t.sin();
            let r1 = 140.0 - amp * 35.0 * t1.sin();
            a.push(r0.min(r1).round() as i32);
            b.push(r0.max(r1).round() as i32);
        }
        Trace { a, b }
    }
    pub fn square(amp: f32, periods: f32, duty: f32) -> Trace {
        let (mut a, mut b) = (vec![], vec![]);
        for i in 0..400 {
            let ph = (i as f32 / 400.0 * periods).fract();
            let r = if ph < duty { 140 - (amp * 35.0) as i32 } else { 140 + (amp * 35.0) as i32 };
            a.push(r);
            b.push(r);
        }
        Trace { a, b }
    }
    pub fn noise(centre: i32, amp: i32, seed: u32) -> Trace {
        let mut s = seed;
        let (mut a, mut b) = (vec![], vec![]);
        for _ in 0..400 {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let r = centre + ((s >> 16) as i32 % (2 * amp + 1)) - amp;
            a.push(r);
            b.push(r);
        }
        Trace { a, b }
    }
    pub fn shifted(&self, rows: i32) -> Trace {
        Trace { a: self.a.iter().map(|r| r + rows).collect(), b: self.b.iter().map(|r| r + rows).collect() }
    }
    /// Engine widening (FUN_080310f8) + FUN_0802dc28 order and clamp.
    pub fn columns(&self, n: usize) -> Vec<(i32, i32)> {
        let mut out = vec![];
        let (mut pa, mut pb) = (0, 0);
        for i in 0..n.min(400) {
            let (mut a, mut b) = (self.a[i].min(self.b[i]), self.a[i].max(self.b[i]));
            if i > 0 {
                a = a.min(pb);
                b = b.max(pa);
            }
            pa = a;
            pb = b;
            out.push((a.min(b).clamp(0, 278), a.max(b).clamp(0, 278)));
        }
        out
    }
}

pub struct Scene {
    pub name: &'static str,
    pub m: Model,
    pub trace: Option<Trace>,
    pub persist: Option<Trace>,
    pub cols: usize,
    pub cells: Vec<&'static str>,
}

const VDIV: [f32; 22] = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0];
const PROBE: [f32; 13] = [0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];

/// The reference renderer's trigger-level volts (same formula, so the badge
/// texts match).
fn trig_v(m: &Model) -> f32 {
    (140 + m.pos as i32 - m.trig_row as i32) as f32 * 4.0 * VDIV[m.vdiv as usize] * PROBE[m.probe as usize] / 141.0
}

pub fn scenes() -> Vec<Scene> {
    let mut v = vec![];
    let d = Model::DEFAULT;
    let cells = vec!["Freq:1.00kHz", "Vp-p:3.00V", "Cycl:1.00ms", "Vrms:1.06V"];
    let mut add = |name: &'static str, m: Model, trace: Option<Trace>, persist: Option<Trace>, cols: usize, c: Vec<&'static str>| {
        let mut m = m;
        m.trig_v = trig_v(&m);
        v.push(Scene { name, m, trace, persist, cols, cells: c });
    };
    let sine = || Some(Trace::sine(1.5, 2.0, 0.0));
    add("st01-default-td", d, sine(), None, 399, cells.clone());
    let mut m = d;
    m.status = su::ST_STOP;
    m.record = (0.1, 0.3);
    add("st02-stop-zoomed", m, sine(), None, 399, cells.clone());
    let mut m = d;
    m.status = su::ST_AUTO;
    add("st03-auto-square", m, Some(Trace::square(2.0, 3.0, 0.3)), None, 399, cells.clone());
    let mut m = d;
    m.status = su::ST_WAIT;
    add("st04-wait-empty", m, None, None, 399, cells.clone());
    let mut m = d;
    m.tb = 23;
    m.status = su::ST_RUN;
    m.roll = true;
    add("st05-roll", m, Some(Trace::noise(150, 6, 7)), None, 180, cells.clone());
    let mut m = d;
    m.xcur = Some([120, 280]);
    m.sel_h = 2;
    add("st06-xcursors", m, sine(), None, 399, vec!["X2-1:800us", "Xf:1.25kHz"]);
    let mut m = d;
    m.ycur = Some([60, 220]);
    m.sel_v = 3;
    add("st07-ycursors", m, sine(), None, 399, vec!["Y2-1:4.54V"]);
    let mut m = d;
    m.sel_v = 1;
    m.trig_row = 60;
    m.pos = 40;
    m.guide = true;
    add("st08-drag-trig-level", m, Some(Trace::sine(1.5, 2.0, 0.0).shifted(40)), None, 399, cells.clone());
    let mut m = d;
    m.guide = true;
    m.trig_px = 120;
    m.pos = -30;
    add("st09-drag-pos-guides", m, Some(Trace::sine(1.5, 2.0, 0.2).shifted(-30)), None, 399, cells.clone());
    let mut m = d;
    m.pos = 200;
    m.trig_row = 295;
    m.trig_px = 5;
    add("st10-offscreen-markers", m, Some(Trace::sine(1.5, 2.0, 0.0).shifted(200)), None, 399, cells.clone());
    add("st12-persistence", d, sine(), Some(Trace::sine(1.5, 2.0, 0.03)), 399, cells.clone());
    let mut m = d;
    m.ac = true;
    m.probe = 6;
    m.vdiv = 12;
    m.rising = false;
    m.colour = 3;
    m.battery = 1;
    m.charging = true;
    add("st13-ac-x10-green-falling", m, sine(), None, 399, cells.clone());
    v
}

/// Load a scene's traces into the graph (frame rows).
pub fn load(sc: &Scene) {
    let g = su::graph();
    g.clear();
    let h = sc.m.frame_h();
    if let Some(t) = &sc.trace {
        for (i, (a, b)) in t.columns(sc.cols).iter().enumerate().take(399) {
            g.tr[i] = (su::trace_fy(h, *a), su::trace_fy(h, *b));
        }
    }
    if let Some(t) = &sc.persist {
        for (i, (a, b)) in t.columns(399).iter().enumerate().take(399) {
            g.ps[i] = (su::trace_fy(h, *a), su::trace_fy(h, *b));
        }
    }
}

/// Draw a scene on a fresh screen.
pub fn draw(sc: &Scene) {
    host::reset(0);
    su::invalidate();
    load(sc);
    su::update(&sc.m);
    su::cells(&sc.m, &sc.cells);
}
