//! The stock-replica scope page on the App side (docs/stock-scope-port-plan.md
//! S4/S5): the stock interaction model (docs/stock-scope-ui.md sec. 10) and
//! the responsive drag path, drawing through src/scope_ui.rs.
//!
//! Interaction (stock mode):
//! * selection on press: 32x32 zones around the markers (glyph rect grown by
//!   10 px), the stock search order (other group after its current index,
//!   then the same group cycling, then the other group from 0), so repeated
//!   taps on overlapping zones cycle through them; the selected marker turns
//!   filled, the guide lines of the selection show from the press until 2 s
//!   after the last event;
//! * per-event drag (raw `Move` reports, no slop or axis lock): |dx| <= |dy|
//!   moves the selected level item (CH1 position, trigger level, Y cursor)
//!   by dy rows, else the selected time item (trigger position / stored
//!   record pan, X cursor) by dx px; a 4 px tap tolerance before the first
//!   move applies (the stock nudges the item on a wobbly tap);
//! * a tap off the zones steps the timebase (left half slower, right half
//!   faster); the right panel: V/div + / -, CH1 popup, TRIG -> Trigger tab;
//! * no page swipe (MENU leaves the page) unless Settings "Scope swipes" is
//!   on (swipes that start outside the drag area: top bar, panel);
//! * alternatives (Settings "Scope drag"): Axis lock (the first 15 px decide
//!   the axis, then only that axis), Hold 300 ms (moves count after a still
//!   hold; the beep marks the grab).
//! Responsiveness (owner, 2026-10-13: "dragging the trace lags"): while a
//! drag is active the acquisition is not polled (no new frames, no restart
//! per event); each event re-renders the frame already shown from its
//! cached column rows, shifted / scaled (Graph::flush repaints only the rows
//! that change); the settings reach the hardware once, on release.
//! Pinch: horizontal = timebase, vertical = V/div, 1-2-5 steps, a value
//! bubble and a scaled preview of the shown frame; applied on release.
//! AUTO (autoset) follows the stock AUTO-set (scope_gen_ui_v2_6.h sec. 4)
//! with UF-04 (the user's coupling is kept; DC only if AC finds nothing).

use super::*;
use crate::gesture::{Axis, Gesture};
use crate::scope_ui::{self as su, Model};
use crate::stockfont::{self as sf, rect, Rect};
use crate::touch::Point;

/// Guide lines stay this long after the last drag event / release (stock).
const GUIDE_MS: u64 = 2000;
/// Movement (px from the press) a touch may wobble and still be a tap; the
/// drag starts beyond it (with the accumulated delta, nothing is lost).
const TAP_TOL: i32 = 4;
/// Axis-lock mode: travel that decides the axis (gesture.rs SLOP).
const LOCK_SLOP: i32 = 15;
/// Pressed-button feedback (one stock pressed colour frame), ms.
const PRESS_MS: u64 = 120;
/// Drag modes (Settings "Scope drag").
pub const DRAG_STOCK: u8 = 0;
pub const DRAG_AXIS: u8 = 1;
/// Names of the drag modes without hold-to-grab (Settings "Scope drag";
/// Hold is gesture::grab_ms != 0 with the Stock model, shown as "Hold
/// <ms> ms"; review 7dc32ac L4: a stored mode 2 showed as "Axis lock").
pub const DRAG_NAMES: [&str; 2] = ["Stock", "Axis lock"];
/// Persistence (Display tab: OFF, MIN, 1S, INF): layer period (ms).
const PERSIST_MS: [u64; 4] = [0, 40, 500, u64::MAX];

/// A touch on the scope page's drag area.
#[derive(Clone, Copy)]
pub(super) struct Touch {
    p0: Point,
    /// Last report seen / last point whose delta was applied.
    last: Point,
    applied: Point,
    far: i32,
    moving: bool,
    /// The press selected a marker zone (no timebase step on release).
    hit: bool,
    t0: u64,
    /// Settings / cursors at the press (undo for a second finger).
    snap: scope::Settings,
    cur: ui::Cursors,
    /// Stopped scope: stored-record anchor at the press (pan base), and the
    /// pan so far (px).
    anchor: Option<usize>,
    pan: i32,
    /// Axis-lock mode: the decided axis.
    axis: Option<Axis>,
    /// Hold mode: grabbed.
    grabbed: bool,
    /// Combined view: sub-row remainder of vertical moves.
    acc: i32,
    /// What moved (DBG:GEST? code) and its travel.
    code: u8,
    travel: i32,
}

#[derive(Clone, Copy)]
pub(super) struct Pinch {
    axis: Axis,
    /// Index at the start (tb / range) and the previewed one; always inside
    /// the table of `axis` (scope_ui::pinch_step).
    i0: usize,
    i: usize,
}

/// Column rows (280-row model) of the frame shown, per engine column, and
/// what the frame was captured with: the base of every preview.
struct Cache {
    seg: [(i16, i16); 400],
    valid: bool,
    pos: i32,
    hpos: i32,
    range: usize,
    tb: usize,
    /// Persistence layers (model rows per graph column) and the swap time.
    ps_a: [(i16, i16); 399],
    ps_b: [(i16, i16); 399],
    ps_t: u64,
}

// In SRAM (.dmabuf, NOLOAD) on the device (4.8 KB out of TCM); set up by
// `sp_init`.
#[cfg_attr(target_os = "none", link_section = ".dmabuf")]
static mut CACHE: Cache = Cache {
    seg: [su::EMPTY; 400],
    valid: false,
    pos: 0,
    hpos: 200,
    range: 6,
    tb: 13,
    ps_a: [su::EMPTY; 399],
    ps_b: [su::EMPTY; 399],
    ps_t: 0,
};

fn cache() -> &'static mut Cache {
    unsafe { &mut *core::ptr::addr_of_mut!(CACHE) }
}

/// Set the page buffers up in place (their statics are not loaded on the
/// device): the frame cache here, the graph in scope_ui.
pub(super) fn sp_init() {
    let c = cache();
    for i in 0..400 {
        c.seg[i] = su::EMPTY;
    }
    for i in 0..399 {
        c.ps_a[i] = su::EMPTY;
        c.ps_b[i] = su::EMPTY;
    }
    c.valid = false;
    c.pos = 0;
    c.hpos = 200;
    c.range = 6;
    c.tb = 13;
    c.ps_t = 0;
    su::init();
}

/// AUTO-set state (stock mode 4).
#[derive(Clone, Copy)]
pub(super) struct Autoset {
    phase: u8,
    idx: usize,
    round: u8,
    saved: scope::Settings,
    /// Records before this time are not used (range relays / offset PWM
    /// settle: stock 200 ms per candidate).
    settle: u64,
    /// Give up (stock 15 s abort timer).
    abort: u64,
}

/// Scope page state.
pub(super) struct Sp {
    pub sel_v: u8,
    pub sel_h: u8,
    /// Group of the last selection (false = level / V, true = time / H).
    last_h: bool,
    pub guide_until: u64,
    pub touch: Option<Touch>,
    pub pinch: Option<Pinch>,
    pub drag_mode: u8,
    pub swipes: bool,
    pub press_fx: bool,
    pressed: u8,
    pressed_until: u64,
    pub colour: u8,
    pub bright: u8,
    pub grid: u8,
    pub persist: u8,
    pub autoset: Option<Autoset>,
    /// Drag in progress with the acquisition held (no polling).
    frozen: bool,
    /// Open stock popup (modal; the page under it is not repainted), the
    /// Measure cell under the finger, a slider being dragged (0 brightness,
    /// 1 grid), the "Sampling" box shown.
    pub popup: su::Popup,
    pressed_cell: Option<u8>,
    slider: Option<u8>,
    sampling: bool,
    /// Pinch bubble shown / to be (re)drawn.
    bubble: bool,
    bubble_dirty: bool,
}

impl Sp {
    /// A drag or pinch holds the acquisition (no polling).
    pub fn frozen_or_pinch(&self) -> bool {
        self.frozen || self.pinch.is_some()
    }

    /// The page is covered by a popup / box (no page drawing).
    pub fn covered(&self) -> bool {
        self.popup != su::Popup::None || self.sampling
    }

    pub const fn new() -> Sp {
        Sp {
            sel_v: 0,
            sel_h: 0,
            last_h: false,
            guide_until: 0,
            touch: None,
            pinch: None,
            drag_mode: DRAG_STOCK,
            swipes: false,
            press_fx: true,
            pressed: 0,
            pressed_until: 0,
            colour: 2,
            bright: 100,
            grid: 50,
            persist: 0,
            autoset: None,
            frozen: false,
            popup: su::Popup::None,
            pressed_cell: None,
            slider: None,
            sampling: false,
            bubble: false,
            bubble_dirty: false,
        }
    }
}

/// Index of the probe factor in scope::PROBES (3 = x1).
fn probe_idx(p: f32) -> usize {
    scope::PROBES.iter().position(|&x| x == p).unwrap_or(3)
}

/// V/div label index (scope_ui::VDIV_LABELS) of a hardware range and probe.
fn vdiv_idx(range: usize, probe: f32) -> u8 {
    (range + probe_idx(probe)).min(21) as u8
}

/// Rows of the frame (13 + 280-row model) a pinch / pan preview maps: target
/// column j of the graph from the cached columns, with a vertical scale `ky`
/// around the zero row and a time scale `kx` around the trigger column.
fn preview_rows(c: &Cache, j: i32, d_pos: i32, hpos: i32, ky: f32, kx: f32, zr: f32) -> (i16, i16) {
    // Source columns covering target column j.
    let (s0, s1) = if kx == 1.0 {
        let s = j - (hpos - c.hpos);
        (s, s + 1)
    } else {
        let a = c.hpos as f32 + (j - hpos) as f32 / kx;
        let b = c.hpos as f32 + (j + 1 - hpos) as f32 / kx;
        let fl = |v: f32| if v >= 0.0 || v == (v as i32) as f32 { v as i32 } else { v as i32 - 1 };
        let (a, b) = (fl(a), (-fl(-b)).max(fl(a) + 1));
        (a, b)
    };
    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
    for s in s0.max(0)..s1.min(400) {
        let (a, b) = c.seg[s as usize];
        if a > b {
            continue;
        }
        lo = lo.min(a as i32);
        hi = hi.max(b as i32);
    }
    if lo > hi {
        return su::EMPTY;
    }
    let map = |r: i32| -> i32 {
        if ky == 1.0 {
            r + d_pos
        } else {
            let z0 = 140 + c.pos;
            (zr + (r - z0) as f32 * ky) as i32
        }
    };
    let (a, b) = (map(lo), map(hi));
    (a.min(b).clamp(-30000, 30000) as i16, a.max(b).clamp(-30000, 30000) as i16)
}

impl App {
    /// The page model from the engine and the page state.
    pub(super) fn sp_model(&self, now: u64) -> Model {
        let s = &self.scope.s;
        let sp = &self.sp;
        let status = if !s.run {
            su::ST_STOP
        } else if self.scope.rolling() {
            su::ST_RUN
        } else {
            self.scope.status.min(3)
        };
        let record = self.scope.record_frac();
        Model {
            status,
            tb: s.tb as u8,
            vdiv: vdiv_idx(s.range, s.probe),
            probe: probe_idx(s.probe) as u8,
            ac: s.ac,
            trig_row: s.trig_row as i16,
            trig_v: self.scope.trig_volts(),
            rising: s.rising,
            trig_px: s.hpos as i16,
            pos: s.pos as i16,
            colour: sp.colour,
            bright: sp.bright,
            grid: sp.grid,
            sel_v: sp.sel_v,
            sel_h: sp.sel_h,
            guide: now < sp.guide_until || sp.touch.is_some_and(|t| t.hit || t.moving),
            xcur: if self.cur.x_on { Some([self.cur.x[0] as i16, self.cur.x[1] as i16]) } else { None },
            ycur: if self.cur.y_on { Some([self.cur.y[0] as i16, self.cur.y[1] as i16]) } else { None },
            battery: self.power.bars.min(5),
            charging: self.power.charging(),
            roll: self.scope.rolling(),
            record,
            // Combined view: the strip's GEN badge shows it instead.
            gen_hz: if self.gen.s.on && self.view == View::Scope { self.gen.actual_hz() } else { 0.0 },
            ch1_open: matches!(sp.popup, su::Popup::Ch1(_)),
            math_open: matches!(sp.popup, su::Popup::Math(_)),
            pressed: if now < sp.pressed_until { sp.pressed } else { 0 },
            combined: self.view == View::DmmScope,
            math_mode: 0,
        }
    }

    /// Leaving the scope views: a drag / pinch in flight is ended (its
    /// settings applied, the acquisition no longer held), popups close.
    pub(super) fn sp_leave(&mut self, now: u64) {
        let held = self.sp.frozen || self.sp.pinch.is_some();
        self.sp.frozen = false;
        self.sp.touch = None;
        self.sp.pinch = None;
        self.sp.bubble = false;
        su::graph().set_hole(None);
        self.sp.popup = su::Popup::None;
        self.sp.pressed_cell = None;
        self.sp.slider = None;
        if held {
            let s = self.scope.s;
            self.scope.apply(s, now);
        }
        // A running AUTO-set ends with the page (review 7dc32ac L2: it went
        // on off-page and applied its result / toast on another view).
        if let Some(a) = self.sp.autoset.take() {
            self.sp.sampling = false;
            self.scope_set(a.saved, now);
        }
    }

    /// A stock-replica scope page is shown (the Scope page, or the combined
    /// view: the same page with the meter strip).
    pub(super) fn sp_on(&self) -> bool {
        matches!(self.view, View::Scope | View::DmmScope)
    }

    /// The page is covered (popup, "Sampling" box, the combined view's
    /// function band): no page drawing.
    pub(super) fn sp_covered(&self) -> bool {
        self.sp.covered() || (self.view == View::DmmScope && self.band_until != 0)
    }

    /// Combined view: the meter strip from the shown reading.
    pub(super) fn sp_strip(&mut self) {
        if self.sp.covered() {
            return;
        }
        let Some(r) = self.meter.shown().copied() else {
            su::meter_strip(&su::StripData::EMPTY);
            return;
        };
        let (hold, lim) = (self.meter.hold_state(), self.meter.verdict());
        let mut d = ui::strip_data(&r, hold, lim, self.meter.rel_active(), self.hv_level, self.strip_alt, self.dmm_view.label, crate::csvlog::active());
        if self.gen.s.on {
            let mut b = su::Buf::new();
            su::fmt_hz(&mut b, self.gen.actual_hz());
            d.gen = su::Txt::new(b.as_str());
        }
        su::meter_strip(&d);
    }

    /// Full page draw (after `redraw`).
    pub(super) fn sp_draw_static(&mut self) {
        su::invalidate();
        self.sp.bubble_dirty = self.sp.bubble;
        self.sp_render();
        self.sp_update(self.now);
        self.sp_cells();
        if self.view == View::DmmScope {
            su::strip_invalidate();
            self.sp_strip();
            if self.band_until != 0 {
                ui::func_band(self.dmm.button(), self.dmm.hy_mode(), self.dmm.sub() == 2);
            }
        }
        if self.sp.popup != su::Popup::None {
            self.sp_popup_draw();
        }
        if self.sp.sampling {
            su::sampling();
        }
    }

    /// Repaint whatever changed (model diff, graph rows).
    pub(super) fn sp_update(&mut self, now: u64) {
        let m = self.sp_model(now);
        su::update(&m);
        if self.sp.bubble && self.sp.bubble_dirty {
            self.sp.bubble_dirty = false;
            self.sp_bubble_draw();
        }
    }

    /// Measurement bar from the selection and the latest results.
    pub(super) fn sp_cells(&mut self) {
        let m = self.sp_model(self.now);
        let mut b: [[u8; 24]; 4] = [[0; 24]; 4];
        let mut n = [0usize; 4];
        for i in 0..self.meas_n.min(4) {
            let id = self.meas_sel[i];
            // Stock label "Vp-p:" (ours: "Vpp:").
            let mut t = [0u8; 24];
            let f = crate::scope_meas::format(&mut t, id, self.meas_res.v[id as usize]);
            let (pre, rest): (&[u8], &[u8]) = match f.strip_prefix("Vpp") {
                Some(r) => (b"Vp-p", r.as_bytes()),
                None => (b"", f.as_bytes()),
            };
            let k = (pre.len() + rest.len()).min(24);
            b[i][..pre.len()].copy_from_slice(pre);
            b[i][pre.len()..k].copy_from_slice(&rest[..k - pre.len()]);
            n[i] = k;
        }
        let txt = |i: usize| core::str::from_utf8(&b[i][..n[i]]).unwrap_or("--");
        let t: [&str; 4] = [txt(0), txt(1), txt(2), txt(3)];
        su::cells(&m, &t[..self.meas_n.min(4)]);
    }

    /// A new frame from the engine: column rows (cached for previews),
    /// persistence, the graph.
    pub(super) fn sp_frame(&mut self, f: &scope::Frame, now: u64) {
        let c = cache();
        let s = self.scope.s;
        // Persistence: the previous frame joins the current layer (stock
        // 4.3); layers swap every period; off in roll and STOP.
        let pm = if self.scope.rolling() || !s.run { 0 } else { self.sp.persist as usize };
        if pm == 0 {
            c.ps_a = [su::EMPTY; 399];
            c.ps_b = [su::EMPTY; 399];
        } else {
            if c.valid {
                for i in 0..399 {
                    let (a, b) = c.seg[i];
                    if a <= b {
                        let p = &mut c.ps_a[i];
                        *p = (p.0.min(a), p.1.max(b));
                    }
                }
            }
            if PERSIST_MS[pm] != u64::MAX && now >= c.ps_t {
                c.ps_t = now + PERSIST_MS[pm];
                c.ps_b = c.ps_a;
                c.ps_a = [su::EMPTY; 399];
            }
        }
        // Code -> row is affine: rows of the code min / max per column.
        let r0 = self.scope.row_of_code(0.0);
        let slope = (self.scope.row_of_code(1000.0) - r0) / 1000.0;
        let row = |code: u16| -> i32 { (r0 + slope * code as f32) as i32 };
        let n = f.data.len();
        let mut pseg: Option<(i32, i32)> = None;
        for i in 0..scope::COLS {
            let (lo, hi);
            if let Some(cols) = f.cols {
                let (a, b) = cols[i];
                if a == u16::MAX {
                    pseg = None;
                    c.seg[i] = su::EMPTY;
                    continue;
                }
                let (r1, r2) = (row(a), row(b));
                lo = r1.min(r2);
                hi = r1.max(r2);
            } else if n >= scope::COLS {
                let a = i * n / scope::COLS;
                let b = ((i + 1) * n / scope::COLS).max(a + 1).min(n);
                let (mut cl, mut ch) = (u16::MAX, 0u16);
                for &v in &f.data[a..b] {
                    cl = cl.min(v);
                    ch = ch.max(v);
                }
                let (r1, r2) = (row(cl), row(ch));
                lo = r1.min(r2);
                hi = r1.max(r2);
            } else if n >= 2 {
                // Fewer samples than columns: linear interpolation.
                let p = i as f32 * (n - 1) as f32 / (scope::COLS - 1) as f32;
                let k = p as usize;
                let fr = p - k as f32;
                let v0 = f.data[k] as f32;
                let v1 = f.data[(k + 1).min(n - 1)] as f32;
                let r = (r0 + slope * (v0 + (v1 - v0) * fr)) as i32;
                lo = r;
                hi = r;
            } else {
                c.seg[i] = su::EMPTY;
                continue;
            }
            // Stock widening: consecutive bars touch.
            let (mut a, mut b) = (lo, hi);
            if let Some((pa, pb)) = pseg {
                a = a.min(pb);
                b = b.max(pa);
            }
            pseg = Some((lo, hi));
            c.seg[i] = (a.clamp(-30000, 30000) as i16, b.clamp(-30000, 30000) as i16);
        }
        c.valid = true;
        c.pos = s.pos;
        c.hpos = s.hpos;
        c.range = s.range;
        c.tb = s.tb;
        self.sp_render();
    }

    /// Graph rows from the cache with the current settings (identity for a
    /// fresh frame; the drag / pinch preview otherwise).
    pub(super) fn sp_render(&mut self) {
        let c = cache();
        let g = su::graph();
        let h = if self.view == View::DmmScope { 301 - su::COMBINED_TOP } else { 281 };
        if !c.valid {
            g.clear();
            return;
        }
        let s = &self.scope.s;
        let (ky, kx) = match self.sp.pinch {
            // (Indices clamped again: a safety net, pinch_step keeps them in
            // their tables.)
            Some(p) if p.axis == Axis::V => (scope::VDIV[c.range.min(9)] / scope::VDIV[p.i.min(9)], 1.0),
            Some(p) => (1.0, scope::SDIV[c.tb.min(29)] / scope::SDIV[p.i.min(29)]),
            None => (scope::VDIV[c.range] / scope::VDIV[s.range], scope::SDIV[c.tb] / scope::SDIV[s.tb]),
        };
        // A stopped pan moves the record under the trigger column.
        let pan = self.sp.touch.map_or(0, |t| t.pan);
        let hpos = if s.run { s.hpos } else { c.hpos + pan };
        let zr = (140 + s.pos) as f32;
        let d_pos = s.pos - c.pos;
        for j in 0..399 {
            let (a, b) = preview_rows(c, j as i32, d_pos, hpos, ky, kx, zr);
            g.tr[j] = if a > b { su::EMPTY } else { (su::trace_fy(h, a as i32), su::trace_fy(h, b as i32)) };
            let (pa, pb) = (c.ps_a[j], c.ps_b[j]);
            let (lo, hi) = (pa.0.min(pb.0), pa.1.max(pb.1));
            g.ps[j] = if lo > hi || d_pos != 0 || hpos != c.hpos || ky != 1.0 || kx != 1.0 {
                su::EMPTY
            } else {
                (su::trace_fy(h, lo as i32), su::trace_fy(h, hi as i32))
            };
        }
    }

    /// Settings changed outside a drag: forget the persistence.
    pub(super) fn sp_settings_changed(&mut self) {
        let c = cache();
        c.ps_a = [su::EMPTY; 399];
        c.ps_b = [su::EMPTY; 399];
    }

    /// Per main-loop pass on the scope page (from scope_poll): timers, the
    /// diff repaint.
    pub(super) fn sp_tick(&mut self, now: u64) {
        if self.sp.pressed != 0 && now >= self.sp.pressed_until {
            self.sp.pressed = 0;
        }
        if let Some(a) = self.sp.autoset {
            if now >= a.abort {
                self.sp.autoset = None;
                self.scope_set(a.saved, now);
            }
        }
        if self.sp.sampling && self.sp.autoset.is_none() {
            self.sp.sampling = false;
            self.redraw = true;
            return;
        }
        if self.sp_covered() {
            return;
        }
        self.sp_update(now);
    }

    // ------------------------------------------------------------ zones

    /// Stock selection on press (zone search, scope_ui::pick_zone): true
    /// when a marker zone was hit.
    fn sp_select(&mut self, p: Point) -> bool {
        let m = self.sp_model(self.now);
        let mut z = [su::Zone::NONE; 7];
        let n = su::zones(&m, &mut z);
        let mut pick = su::pick_zone(&z[..n], self.sp.sel_v, self.sp.sel_h, self.sp.last_h, p.x as i16, p.y as i16);
        if pick.is_none() {
            // Off the marker zones: the cursor lines themselves (fw-rs).
            let mut lz = [su::Zone::NONE; 4];
            let k = su::line_zones(&m, &mut lz);
            if let Some(i) = su::pick_zone(&lz[..k], self.sp.sel_v, self.sp.sel_h, self.sp.last_h, p.x as i16, p.y as i16) {
                z[0] = lz[i];
                pick = Some(0);
            }
        }
        match pick {
            Some(k) => {
                if z[k].level {
                    self.sp.sel_v = z[k].id;
                    self.sp.last_h = false;
                } else {
                    self.sp.sel_h = z[k].id;
                    self.sp.last_h = true;
                }
                true
            }
            None => false,
        }
    }

    // ------------------------------------------------------------ gestures

    /// Gesture on the scope page; None = not handled here (the generic
    /// handling in `gesture` runs), Some(captured) otherwise.
    pub(super) fn sp_gesture(&mut self, clk: &mut Clock, g: Gesture) -> Option<bool> {
        let now = clk.now_ms();
        // The end of a pinch always reaches it (before the AUTO / popup
        // early returns).
        if let Gesture::PinchEnd(ok) = g {
            self.sp_pinch_end(ok, now);
            return Some(false);
        }
        // Touch lock while AUTO-set runs (stock), keys too except MENU.
        if self.sp.autoset.is_some() {
            return match g {
                Gesture::KeyDown(0) => None,
                Gesture::KeyDown(1) => {
                    // RUN refused: the stock "Sampling" box until it is done.
                    self.beep(now, 50);
                    if !self.sp.sampling && !self.redraw {
                        self.sp.sampling = true;
                        su::sampling();
                    }
                    Some(false)
                }
                Gesture::KeyDown(_) | Gesture::Down(_) | Gesture::Tap(_) => Some(false),
                _ => None,
            };
        }
        if self.sp.popup != su::Popup::None {
            return self.sp_popup_gesture(clk, g);
        }
        if self.view == View::DmmScope {
            // Combined view: DMM keys; the strip and the function band keep
            // their own handling (app.rs); the strip is not a drag area (a
            // swipe from it changes the view).
            match g {
                Gesture::KeyDown(_) | Gesture::KeyUp(_) => return None,
                Gesture::Down(_) | Gesture::Tap(_) if self.band_until != 0 => return None,
                Gesture::Down(p) if su::strip_hit(p.x, p.y).is_some() => return Some(false),
                Gesture::Tap(p) => {
                    if let Some(badges) = su::strip_hit(p.x, p.y) {
                        self.beep(now, 30);
                        if badges {
                            self.rel_toggle(now);
                        } else {
                            self.band_open(now);
                        }
                        return Some(false);
                    }
                }
                Gesture::LongPress(p) => {
                    if su::strip_hit(p.x, p.y).is_some() {
                        self.beep(now, 60);
                        self.show(View::Dmm, clk);
                        return Some(true);
                    }
                }
                Gesture::SwipeLeft | Gesture::SwipeRight => return None,
                _ => {}
            }
        }
        match g {
            Gesture::Down(p) if su::INNER.has(p.x as i16, p.y as i16) => {
                let hit = self.sp_select(p);
                let s = self.scope.s;
                self.sp.touch = Some(Touch {
                    p0: p,
                    last: p,
                    applied: p,
                    far: 0,
                    moving: false,
                    hit,
                    t0: now,
                    snap: s,
                    cur: self.cur,
                    anchor: self.scope.anchor(),
                    pan: 0,
                    axis: None,
                    grabbed: gesture::grab_ms() == 0,
                    acc: 0,
                    code: 0,
                    travel: 0,
                });
                self.sp.guide_until = now + GUIDE_MS;
                gesture::raw_next(true);
                Some(true)
            }
            Gesture::Down(p) => {
                // Right panel: pressed feedback (stock: none; ours, switchable).
                if let Some(k) = su::panel_hit(p.x, p.y) {
                    if self.sp.press_fx {
                        self.sp.pressed = k as u8 + 1;
                        self.sp.pressed_until = now + PRESS_MS;
                    }
                }
                Some(false)
            }
            Gesture::Grab(_) => {
                if let Some(t) = self.sp.touch.as_mut() {
                    t.grabbed = true;
                    t.applied = t.last;
                    self.beep(now, 20);
                }
                Some(true)
            }
            Gesture::Move(_, p) => {
                self.sp_move(p, now);
                Some(true)
            }
            // A raw-move touch never sends Drag; a plain capture would.
            Gesture::Drag(_, p, _) if self.sp.touch.is_some() => {
                self.sp_move(p, now);
                Some(true)
            }
            Gesture::Up(_) => {
                self.sp_release(now);
                Some(false)
            }
            Gesture::Cancel => {
                // A second finger: undo what the first finger moved (a pinch
                // must not start with its accidental nudge), drop the touch.
                if let Some(t) = self.sp.touch.take() {
                    self.cur = t.cur;
                    self.cursor_values();
                    if self.sp.frozen {
                        self.sp.frozen = false;
                        self.scope.preview(t.snap);
                        self.sp_render();
                        self.scope.apply(t.snap, now);
                    }
                }
                Some(false)
            }
            Gesture::LongPress(_) => {
                // fw-rs: a still long press on a marker recentres its item:
                // trigger position -> screen centre, trigger level -> the
                // middle of the signal (Vmax + Vmin) / 2, CH1 -> 0 V at the
                // centre row.
                if let Some(t) = self.sp.touch {
                    if t.hit && !t.moving {
                        self.sp_recentre(now);
                    }
                }
                Some(true)
            }
            Gesture::Pinch(axis, s0, s) => {
                self.sp_pinch(axis, s0, s);
                Some(false)
            }
            Gesture::PinchEnd(ok) => {
                self.sp_pinch_end(ok, now);
                Some(false)
            }
            Gesture::Tap(p) => {
                if let Some(k) = su::panel_hit(p.x, p.y) {
                    self.beep(now, 30);
                    self.sp_panel(clk, k, now);
                    return Some(false);
                }
                // fw-rs additions (the stock top bar is not tappable): the
                // status badge = RUN/STOP, the trigger badge = Trigger tab.
                let (x, y) = (p.x as i16, p.y as i16);
                if y < 20 && (13..75).contains(&x) {
                    self.beep(now, 30);
                    self.sp_key(clk, 1);
                } else if y < 20 && (352..414).contains(&x) {
                    self.beep(now, 30);
                    self.sp_popup_open(su::Popup::Mf(2));
                }
                Some(false)
            }
            Gesture::SwipeLeft | Gesture::SwipeRight if !self.sp.swipes => Some(false),
            Gesture::KeyDown(c) => {
                self.beep(now, 50);
                self.sp_key(clk, c);
                Some(false)
            }
            _ => None,
        }
    }

    /// One move report of the scope touch.
    fn sp_move(&mut self, p: Point, now: u64) {
        let Some(mut t) = self.sp.touch else { return };
        t.last = p;
        let d0 = (p.x as i32 - t.p0.x as i32).abs().max((p.y as i32 - t.p0.y as i32).abs());
        t.far = t.far.max(d0);
        if !t.moving {
            if t.far <= TAP_TOL || !t.grabbed {
                self.sp.touch = Some(t);
                return;
            }
            t.moving = true;
        }
        let (mut dx, mut dy) = (p.x as i32 - t.applied.x as i32, p.y as i32 - t.applied.y as i32);
        if self.sp.drag_mode == DRAG_AXIS {
            if t.axis.is_none() {
                let (ax, ay) = ((p.x as i32 - t.p0.x as i32).abs(), (p.y as i32 - t.p0.y as i32).abs());
                if 2 * ax >= 3 * ay && ax > LOCK_SLOP / 2 {
                    t.axis = Some(Axis::H);
                } else if 2 * ay >= 3 * ax && ay > LOCK_SLOP / 2 {
                    t.axis = Some(Axis::V);
                } else if ax.max(ay) > 2 * LOCK_SLOP {
                    t.axis = Some(if ax >= ay { Axis::H } else { Axis::V });
                } else {
                    self.sp.touch = Some(t);
                    return;
                }
            }
            match t.axis {
                Some(Axis::H) => dy = 0,
                _ => dx = 0,
            }
        } else if dx.abs() <= dy.abs() {
            // Stock per-event attribution.
            dx = 0;
        } else {
            dy = 0;
        }
        t.applied = p;
        self.sp.touch = Some(t);
        if dx == 0 && dy == 0 {
            return;
        }
        self.sp.guide_until = now + GUIDE_MS;
        if dy != 0 {
            self.sp_move_v(dy, now);
        } else {
            self.sp_move_h(dx, now);
        }
    }

    /// Vertical move of the selected level item by `dy` px.
    fn sp_move_v(&mut self, dy: i32, _now: u64) {
        let Some(mut t) = self.sp.touch else { return };
        // Pixels -> rows (combined view: the graph is shorter than 280 rows).
        let rows = if self.view == View::DmmScope {
            let h = 301 - su::COMBINED_TOP as i32;
            t.acc += dy * 280;
            let r = t.acc / (h - 1);
            t.acc -= r * (h - 1);
            r
        } else {
            dy
        };
        t.travel += dy;
        self.sp.touch = Some(t);
        if rows == 0 {
            return;
        }
        let mut st = self.scope.s;
        match self.sp.sel_v {
            0 => {
                // CH1 position; the trigger row moves with it (same voltage).
                let p = (st.pos + rows).clamp(-392, 392);
                st.trig_row += p - st.pos;
                st.pos = p;
                self.sp_code(3);
            }
            1 => {
                st.trig_row = (st.trig_row + rows).clamp(-10, 290);
                self.sp_code(1);
            }
            k @ (3 | 4) => {
                let i = (k - 3) as usize;
                self.cur.y[i] = (self.cur.y[i] + rows).clamp(1, 279);
                self.cursor_values();
                self.sp_code(5 + 10 * i as u8);
                return;
            }
            _ => return,
        }
        self.sp_preview(st);
    }

    /// Horizontal move of the selected time item by `dx` px.
    fn sp_move_h(&mut self, dx: i32, _now: u64) {
        let Some(mut t) = self.sp.touch else { return };
        t.travel += dx;
        self.sp.touch = Some(t);
        match self.sp.sel_h {
            0 => {
                if self.scope.rolling() {
                    return; // roll: the trigger items are not draggable
                }
                self.sp_code(2);
                if !self.scope.s.run {
                    // Stopped: pan the stored record (applied on release).
                    if let Some(mut t) = self.sp.touch {
                        if t.anchor.is_some() {
                            t.pan += dx;
                            self.sp.touch = Some(t);
                            self.sp.frozen = true;
                            self.sp_render();
                        }
                    }
                    return;
                }
                let mut st = self.scope.s;
                st.hpos = (st.hpos + dx).clamp(0, scope::COLS as i32);
                self.sp_preview(st);
            }
            k @ (2 | 3) => {
                let i = (k - 2) as usize;
                self.cur.x[i] = (self.cur.x[i] as i32 + dx).clamp(1, 399) as u16;
                self.cursor_values();
                self.sp_code(4 + 10 * i as u8);
            }
            _ => {}
        }
    }

    fn sp_code(&mut self, c: u8) {
        if let Some(t) = self.sp.touch.as_mut() {
            t.code = c;
        }
    }

    /// Drag preview of position settings: no hardware, no acquisition
    /// restart; the shown frame follows from the cache.
    fn sp_preview(&mut self, st: scope::Settings) {
        self.scope.preview(st);
        self.sp.frozen = true;
        self.sp_render();
    }

    /// Release of the scope touch: tap = timebase step; a drag applies its
    /// settings once.
    fn sp_release(&mut self, now: u64) {
        let Some(t) = self.sp.touch.take() else { return };
        self.sp.guide_until = now + GUIDE_MS;
        // DBG:GEST? log.
        if t.moving && t.code != 0 {
            let i = if self.gest_n < GEST_N {
                self.gest_n += 1;
                self.gest_n - 1
            } else {
                self.gest_log.copy_within(1.., 0);
                GEST_N - 1
            };
            let v = matches!(t.code, 1 | 3 | 5 | 15) as u8;
            self.gest_log[i] = (t.code, v, t.travel.clamp(-999, 999) as i16, now.saturating_sub(t.t0).min(9999) as u16);
        }
        if self.sp.frozen {
            self.sp.frozen = false;
            let s = self.scope.s;
            if !s.run && t.pan != 0 {
                if let Some(a) = t.anchor {
                    self.scope.pan_from(a, t.pan);
                }
            }
            if s.pos != t.snap.pos || s.trig_row != t.snap.trig_row || s.hpos != t.snap.hpos {
                self.scope.apply(s, now);
                self.sp_settings_changed();
            } else {
                self.scope.refresh();
            }
            return;
        }
        if !t.moving && !t.hit && t.far <= TAP_TOL {
            self.beep(now, 30);
            self.sp_tb_step(t.p0.x <= 207, now);
        }
    }

    fn sp_recentre(&mut self, now: u64) {
        let mut st = self.scope.s;
        if self.sp.last_h {
            if self.sp.sel_h != 0 {
                return;
            }
            st.hpos = 200;
        } else {
            match self.sp.sel_v {
                0 => {
                    st.trig_row -= st.pos;
                    st.pos = 0;
                }
                1 => {
                    let (hi, lo) = (self.meas_res.v[12], self.meas_res.v[13]);
                    if !(hi.is_finite() && lo.is_finite()) {
                        return;
                    }
                    st.trig_row = (self.scope.row_of_volts((hi + lo) / 2.0) as i32).clamp(-10, 290);
                }
                _ => return,
            }
        }
        self.beep(now, 60);
        self.sp.touch = None;
        self.sp.guide_until = now + GUIDE_MS;
        self.scope_set(st, now);
    }

    /// Tap-to-step timebase (stock: left half slower, right half faster).
    fn sp_tb_step(&mut self, slower: bool, now: u64) {
        let mut st = self.scope.s;
        st.tb = if slower { (st.tb + 1).min(scope::TB_MAX) } else { st.tb.saturating_sub(1).max(scope::TB_MIN) };
        self.scope_set(st, now);
    }

    /// End a held drag / pinch whose end may never arrive (a popup, AUTO,
    /// an overlay or a key took over): a pinch is dropped (nothing applied),
    /// a drag applies what it moved so far; the acquisition runs again
    /// (pre-flash review 7dc32ac B1: a stale pinch kept it held and carried
    /// its index into the other axis).
    pub(super) fn sp_end_hold(&mut self, now: u64) {
        if self.sp.pinch.is_some() {
            self.sp_pinch_end(false, now);
        }
        let held = core::mem::replace(&mut self.sp.frozen, false);
        if let Some(t) = self.sp.touch.take() {
            if held {
                let s = self.scope.s;
                if !s.run && t.pan != 0 {
                    if let Some(a) = t.anchor {
                        self.scope.pan_from(a, t.pan);
                    }
                }
                self.scope.apply(s, now);
                self.sp_settings_changed();
            }
        } else if held {
            let s = self.scope.s;
            self.scope.apply(s, now);
        }
    }

    // ------------------------------------------------------------ pinch

    fn sp_pinch(&mut self, axis: Axis, s0: u16, s: u16) {
        if self.sp.touch.is_some() {
            self.sp.touch = None;
        }
        let st = self.scope.s;
        let h = axis == Axis::H;
        let prev = self.sp.pinch.map(|p| su::PinchState { horizontal: p.axis == Axis::H, i0: p.i0, i: p.i });
        let tb_max = if self.scope.roll { scope::TB_MAX } else { scope::TB_YT_MAX };
        let n = su::pinch_step(prev, h, s0, s, st.tb, st.range, scope::TB_MIN, tb_max);
        let p = Pinch { axis, i0: n.i0, i: n.i };
        let changed = prev.map_or(true, |q| q != n);
        if changed || !self.sp.bubble {
            self.sp.pinch = Some(p);
            self.sp_render();
            if !self.redraw && self.overlay.is_none() {
                self.sp.bubble = true;
                self.sp.bubble_dirty = true;
                su::graph().set_hole(Some(BUBBLE));
                self.sp_update(self.now);
            }
        }
    }

    fn sp_pinch_end(&mut self, ok: bool, now: u64) {
        let Some(p) = self.sp.pinch.take() else { return };
        self.sp.bubble = false;
        su::graph().set_hole(None);
        let mut st = self.scope.s;
        if ok && p.i != p.i0 {
            if p.axis == Axis::H {
                st.tb = p.i;
            } else {
                st.range = p.i;
            }
            self.scope_set(st, now);
        } else {
            self.sp_render();
        }
    }

    /// The pinch value bubble (fw-rs addition, stock purple toast style).
    fn sp_bubble_draw(&self) {
        let Some(p) = self.sp.pinch else { return };
        let mut b = su::Buf::new();
        if p.axis == Axis::H {
            b.push(su::TB_LABELS[p.i.min(29)]).push("/div");
        } else {
            su::vdiv_text(&mut b, vdiv_idx(p.i, self.scope.s.probe));
            b.push("/div");
        }
        let t = sf::Ink::new(sf::SC_F16, BUBBLE, b.as_str(), 8, 0);
        let bg = su::C_FILL;
        sf::paint(BUBBLE, &[(1, bg), (0, su::C_PURPLE)], |x, y| {
            if let Some(g) = t.at(x, y) {
                return (su::C_WHITE, 2 << 8 | g as u16);
            }
            if sf::in_rbox(BUBBLE, 8, 0x0f, x, y) { (su::C_PURPLE, 1 << 8) } else { (bg, 0) }
        });
    }

    // ------------------------------------------------------------ panel, keys

    fn sp_panel(&mut self, _clk: &mut Clock, k: usize, now: u64) {
        let mut st = self.scope.s;
        match k {
            0 => st.range = (st.range + 1).min(9),
            2 => st.range = st.range.saturating_sub(1),
            1 => {
                self.sp_popup_open(su::Popup::Ch1(0));
                return;
            }
            4 => {
                self.sp_popup_open(su::Popup::Math(0));
                return;
            }
            6 => {
                self.sp_popup_open(su::Popup::Mf(2));
                return;
            }
            _ => return, // math scale: no engine
        }
        self.scope_set(st, now);
    }

    fn sp_key(&mut self, clk: &mut Clock, code: u8) {
        let now = clk.now_ms();
        match code {
            0 => self.show(View::Menu, clk),
            1 => {
                let mut s = self.scope.s;
                s.run = !s.run;
                self.scope_set(s, now);
            }
            2 => self.autoset_start(now),
            _ => self.sp_popup_open(su::Popup::Mf(0)),
        }
    }

    // ------------------------------------------------------------ popups

    fn sp_popvals(&self) -> su::PopVals {
        let s = &self.scope.s;
        su::PopVals {
            meas: self.meas_sel,
            meas_n: self.meas_n as u8,
            roll: self.roll,
            persist: self.sp.persist,
            bright: self.sp.bright,
            grid: self.sp.grid,
            rising: s.rising,
            mode: match s.mode {
                scope::TrigMode::Auto => 0,
                scope::TrigMode::Normal => 1,
                scope::TrigMode::Single => 2,
            },
            ac: s.ac,
            probe: probe_idx(s.probe) as u8,
            colour: self.sp.colour,
            math_colour: 1,
            pressed_cell: self.sp.pressed_cell,
        }
    }

    fn sp_popup_draw(&mut self) {
        let v = self.sp_popvals();
        su::draw_popup(self.sp.popup, &v);
    }

    /// Open (or switch) the popup; the CH1 / math label turns grey.
    fn sp_popup_open(&mut self, p: su::Popup) {
        self.sp_end_hold(self.now);
        self.sp.popup = p;
        if !self.redraw {
            // The CH1 / math pill label shows the open popup (0x7bef).
            let m = self.sp_model(self.now);
            su::panel_button(&m, 1);
            su::panel_button(&m, 4);
            self.sp_popup_draw();
        }
    }

    fn sp_popup_close(&mut self) {
        self.sp.popup = su::Popup::None;
        self.sp.pressed_cell = None;
        self.sp.slider = None;
        self.redraw = true;
    }

    /// Every gesture while a popup is open (modal).
    fn sp_popup_gesture(&mut self, clk: &mut Clock, g: Gesture) -> Option<bool> {
        let now = clk.now_ms();
        let v = self.sp_popvals();
        match g {
            Gesture::KeyDown(0) => {
                self.beep(now, 50);
                self.sp_popup_close();
                self.show(View::Menu, clk);
            }
            Gesture::KeyDown(3) => {
                self.beep(now, 50);
                self.sp_popup_close();
            }
            Gesture::KeyDown(_) => self.beep(now, 50), // RUN / AUTO swallowed
            Gesture::Down(p) => {
                let (x, y) = (p.x as i16, p.y as i16);
                if let su::Popup::Mf(t) = self.sp.popup {
                    match su::mf_hit(t, &v, x, y) {
                        su::MfHit::Cell(i) => {
                            self.sp.pressed_cell = Some(i);
                            self.sp_popup_draw();
                        }
                        h @ (su::MfHit::Bright | su::MfHit::Grid) => {
                            self.sp.slider = Some((h == su::MfHit::Grid) as u8);
                            self.sp_slider(x);
                            gesture::raw_next(true);
                            return Some(true);
                        }
                        _ => {}
                    }
                }
            }
            Gesture::Move(_, p) | Gesture::Drag(_, p, _) if self.sp.slider.is_some() => {
                self.sp_slider(p.x as i16);
                return Some(true);
            }
            Gesture::Up(_) => self.sp.slider = None,
            Gesture::Tap(p) => {
                self.beep(now, 30);
                let (x, y) = (p.x as i16, p.y as i16);
                let had_press = self.sp.pressed_cell.take().is_some();
                match self.sp.popup {
                    su::Popup::Mf(t) => self.sp_mf_tap(clk, t, su::mf_hit(t, &v, x, y), now),
                    su::Popup::Ch1(t) => self.sp_side_tap(false, t, su::side_hit(false, t, &v, x, y), now),
                    su::Popup::Math(t) => self.sp_side_tap(true, t, su::side_hit(true, t, &v, x, y), now),
                    su::Popup::None => {}
                }
                if had_press && self.sp.popup != su::Popup::None && !self.redraw {
                    self.sp_popup_draw();
                }
            }
            Gesture::MultiSwipeDown(_) => return None, // screenshot / power-off
            _ => {
                // A touch that ended as no tap releases a pressed cell.
                if self.sp.pressed_cell.take().is_some() && !self.redraw {
                    self.sp_popup_draw();
                }
            }
        }
        Some(false)
    }

    fn sp_slider(&mut self, x: i16) {
        let val = (su::slider_value(x) * 100.0 + 0.5) as u8;
        match self.sp.slider {
            Some(0) => self.sp.bright = val,
            Some(_) => self.sp.grid = val,
            None => return,
        }
        let v = self.sp_popvals();
        su::mf_display_values(&v);
    }

    fn sp_mf_tap(&mut self, clk: &mut Clock, tab: u8, h: su::MfHit, now: u64) {
        let mut st = self.scope.s;
        match h {
            su::MfHit::Close => {
                self.sp_popup_close();
                return;
            }
            su::MfHit::Outside | su::MfHit::Inside | su::MfHit::Bright | su::MfHit::Grid => return,
            su::MfHit::Tab(t) => {
                if t != tab {
                    // A shorter page: the page under the old box comes back.
                    self.sp.popup = su::Popup::Mf(t);
                    self.redraw = true;
                }
                return;
            }
            su::MfHit::Cell(i) => {
                // Refuse a duplicate or a 5th item silently (stock).
                if self.meas_n < 4 && !self.meas_sel[..self.meas_n].contains(&i) {
                    self.meas_sel[self.meas_n] = i;
                    self.meas_n += 1;
                    self.cursors_from_selection();
                }
            }
            su::MfHit::SelCell(k) => {
                let k = k as usize;
                if k < self.meas_n {
                    self.meas_sel.copy_within(k + 1..self.meas_n, k);
                    self.meas_n -= 1;
                    self.cursors_from_selection();
                }
            }
            su::MfHit::Clear => {
                self.meas_n = 0;
                self.cursors_from_selection();
            }
            su::MfHit::Default => {
                self.meas_sel = crate::scope_meas::DEFAULT_SEL;
                self.meas_n = 4;
                self.cursors_from_selection();
            }
            su::MfHit::Roll(on) => {
                self.roll = on;
                self.scope.roll = on;
                // Stock: roll on with a slower index enters roll at 22.
                self.scope_set(st, now);
            }
            su::MfHit::Persist(i) => {
                self.sp.persist = i.min(3);
                self.sp_settings_changed();
            }
            su::MfHit::Rising(r) => {
                st.rising = r;
                self.scope_set(st, now);
            }
            su::MfHit::Mode(m) => {
                st.mode = [scope::TrigMode::Auto, scope::TrigMode::Normal, scope::TrigMode::Single][m.min(2) as usize];
                st.run = true;
                self.scope_set(st, now);
            }
            su::MfHit::PriScr => {
                // Close first: the screenshot shows the page (fw-rs).
                self.sp_popup_close();
                self.shot_req = true;
                return;
            }
            su::MfHit::SaveCsv => {
                self.sp_popup_close();
                self.toast(now, "Scope CSV: use SCPI WAV:DATA?", false);
                return;
            }
        }
        let _ = clk;
        self.sp_popup_draw();
    }

    fn sp_side_tap(&mut self, math: bool, tab: u8, h: su::SideHit, now: u64) {
        let mut st = self.scope.s;
        match h {
            su::SideHit::Close => {
                self.sp_popup_close();
                return;
            }
            su::SideHit::Inside => return,
            su::SideHit::Tab(t) => {
                if t == tab {
                    return;
                }
                self.sp.popup = if math { su::Popup::Math(t) } else { su::Popup::Ch1(t) };
            }
            su::SideHit::Coupling(c) => {
                st.ac = c == 1;
                self.scope_set(st, now);
            }
            su::SideHit::Probe(i) => {
                st.probe = scope::PROBES[(i as usize).min(12)];
                self.scope_set(st, now);
            }
            su::SideHit::Colour(i) => self.sp.colour = i.min(8),
        }
        self.sp_popup_draw();
    }

    // ------------------------------------------------------------ AUTO-set

    /// AUTO-set (stock mode 4, scope_gen_ui_v2_6.h sec. 4), phase 1: 5 us/div,
    /// position 0, V/div 10 V, 1 V, 100 mV, 10 mV until a record with a swing
    /// (30 % / 70 % levels more than 19 codes apart) is seen; each candidate
    /// waits 200 ms (relays, offset PWM) before its record counts. UF-04: the
    /// user's coupling is kept (the stock forces DC); only when an AC search
    /// finds nothing, it is repeated in DC.
    pub(super) fn autoset_start(&mut self, now: u64) {
        self.sp_end_hold(now);
        let saved = self.scope.s;
        let mut st = saved;
        st.tb = 8;
        st.range = AS_CAND[0];
        st.pos = 0;
        st.mode = scope::TrigMode::Auto;
        st.run = true;
        self.sp.autoset = Some(Autoset { phase: 1, idx: 0, round: 0, saved, settle: now + 200, abort: now + 15_000 });
        self.scope.apply(st, now);
    }

    /// One record during AUTO-set.
    pub(super) fn autoset_step(&mut self, f: &scope::Frame, now: u64) {
        let Some(mut a) = self.sp.autoset else { return };
        if now < a.settle || f.data.len() < 16 {
            return;
        }
        let mut st = self.scope.s;
        let (mut lo, mut hi) = (1023u16, 0u16);
        for &v in f.data {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        let pp = (hi - lo) as f32;
        let (l30, l70) = (lo as f32 + 0.3 * pp, lo as f32 + 0.7 * pp);
        // Period from rising crossings of the 30 % / 70 % levels.
        let (mut armed, mut first, mut last, mut n) = (false, 0usize, 0usize, 0u32);
        for (i, &v) in f.data.iter().enumerate() {
            let v = v as f32;
            if v < l30 {
                armed = true;
            } else if armed && v > l70 {
                armed = false;
                if n == 0 {
                    first = i;
                }
                last = i;
                n += 1;
            }
        }
        let period = if n >= 2 { (last - first) as f32 / (n - 1) as f32 / f.fs as f32 } else { 0.0 };
        if a.phase == 1 {
            if l70 - l30 > 19.0 {
                if period > 0.0 {
                    st.tb = tb_for(period);
                }
                if lo == 0 || hi > 1022 {
                    st.range = (st.range + 3).min(9);
                }
                st.trig_row = 140;
                a.phase = 2;
            } else if a.idx + 1 < AS_CAND.len() {
                a.idx += 1;
                st.range = AS_CAND[a.idx];
            } else if a.round == 0 || a.round == 2 {
                // Nothing at 5 us/div: the same search at 20 ms/div.
                a.round += 1;
                a.idx = 0;
                st.tb = 19;
                st.range = AS_CAND[0];
            } else if a.round == 1 && st.ac {
                // Nothing in AC: the same search in DC (UF-04).
                a.round = 2;
                a.idx = 0;
                st.ac = false;
                st.tb = 8;
                st.range = AS_CAND[0];
            } else {
                self.sp.autoset = None;
                self.scope_set(a.saved, now);
                self.toast(now, "AUTO: no signal found", false);
                return;
            }
            a.settle = now + 200;
            self.sp.autoset = Some(a);
            self.scope.apply(st, now);
            return;
        }
        // Phase 2: V/div so that the signal fits +-4 div within 6.4 div,
        // centred; trigger at the middle; timebase from the measured cycle.
        let (vmax, vmin) = (self.scope.volts(hi as f32), self.scope.volts(lo as f32));
        let probe = st.probe;
        let mut r = 9;
        for (i, &d) in scope::VDIV.iter().enumerate() {
            let d = d * probe;
            if vmin >= -4.0 * d && vmax <= 4.0 * d && (vmax - vmin) <= 6.4 * d {
                r = i;
                break;
            }
        }
        st.range = r;
        let mid = (vmax + vmin) / 2.0;
        st.pos = ((141.0 * mid / (4.0 * scope::VDIV[r] * probe)) as i32).clamp(-392, 392);
        st.trig_row = 140;
        let res = crate::scope_meas::compute(f.data, f.fs as f32, &self.scope);
        let cyc = res.v[9];
        if cyc.is_finite() && cyc > 0.0 {
            st.tb = tb_for(cyc);
        } else if period > 0.0 {
            st.tb = tb_for(period);
        }
        st.mode = scope::TrigMode::Auto;
        self.sp.autoset = None;
        self.scope_set(st, now);
        self.redraw = true;
        if a.saved.ac && !st.ac {
            self.toast(now, crate::dialog::AUTO_DC, true);
        }
    }

    // ------------------------------------------------------------ persist

    /// Page settings -> persist words (p[1] bits 17..27, p[11] bits 1..7, 12..15).
    pub(super) fn sp_persist(&self, p1: &mut u32, p11: &mut u32) {
        let s = &self.sp;
        *p1 |= ((s.colour as u32 ^ 2) & 0xF) << 17 | ((100 - s.bright.min(100)) as u32 & 0x7F) << 21;
        *p11 |= (s.drag_mode as u32 & 3) << 1 | (s.swipes as u32) << 3 | (s.persist as u32 & 3) << 4 | (!s.press_fx as u32) << 6;
        *p11 |= ((s.grid.min(100) as u32 / 10) ^ 5) << 12;
    }

    pub(super) fn sp_restore(&mut self, p1: u32, p11: u32) {
        let s = &mut self.sp;
        s.colour = ((p1 >> 17 & 0xF) ^ 2).min(8) as u8;
        s.bright = 100 - (p1 >> 21 & 0x7F).min(100) as u8;
        s.drag_mode = (p11 >> 1 & 3).min(1) as u8;
        s.swipes = p11 >> 3 & 1 != 0;
        s.persist = (p11 >> 4 & 3) as u8;
        s.press_fx = p11 >> 6 & 1 == 0;
        s.grid = (((p11 >> 12 & 0xF) ^ 5).min(10) * 10) as u8;
    }
}

/// AUTO-set V/div candidates (hardware ranges 10 V, 1 V, 100 mV, 10 mV).
const AS_CAND: [usize; 4] = [9, 6, 3, 0];

/// First timebase whose s/div exceeds a quarter period (FUN_08033388).
fn tb_for(period: f32) -> usize {
    scope::SDIV.iter().position(|&d| period / 4.0 < d).unwrap_or(scope::TB_MAX).clamp(scope::TB_MIN, scope::TB_YT_MAX)
}

/// The pinch value bubble: centred in the graph.
const BUBBLE: Rect = rect(153, 140, 120, 40);
