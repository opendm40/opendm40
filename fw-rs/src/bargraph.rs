//! Fast analog bargraph for the DMM page (UF-30, docs/user-feedback.md
//! section 4): a thin bar under the readout, fed with every conversion
//! (`App::reading`, the un-smoothed `Reading::main`), so it moves at the
//! converter rate while the digits stay averaged / smoothed. The stock
//! firmware has no bargraph (no bar widget in gui_v2_6.h / the DMM layout).
//!
//! Rate (measured facts, dmm.rs FR-05 table): fw-rs has no separate fast
//! channel; a Reading arrives once per AD1 conversion: 10/s while moving and
//! in FAST (OSR 8192), 2.5/s once a MEDIUM / SLOW reading is steady (OSR
//! 32768); AC runs on the RMS converter's own rate. The >= 40/s asked for in
//! UF-30 needs AD1 at OSR 0x13 (the logger's ~38/s) and a firmware moving
//! average for the main readout: a measurement-chain change, not done here.
//!
//! Scale: the 60000-count format of the value (`Val::dec`), so no range
//! tables: frac = |v| * 10^dec / 60000. Unipolar (0 at the left) or
//! bipolar (0 in the middle, DC functions). Overload: the whole track red.
//!
//! Drawing: `draw_static` once (track + ticks), then `update` repaints only
//! the columns whose fill changed (at most 4 fill_rect runs per update).

use crate::lcd::{self, Color};

const TRACK: Color = 0x2104;
const TICK: Color = 0x7BEF;
const FILL: Color = 0xFC40; // ui::VALUE
const OL: Color = 0xF800;
/// Tick rows under the track.
const TICK_H: u16 = 4;

/// Signed fraction of full scale (60000 counts) of a value with `dec`
/// decimals in the 60000-count format; None = overload.
pub fn frac(v: f32, dec: u8, ol: bool) -> Option<f32> {
    if ol || !v.is_finite() {
        return None;
    }
    let mut s = v;
    for _ in 0..dec {
        s *= 10.0;
    }
    Some((s / 60000.0).clamp(-1.0, 1.0))
}

/// A bar at (x, y), `w` x `h` (track `h - TICK_H` rows, ticks below).
pub struct Bar {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub bipolar: bool,
    /// Filled columns [a, b) relative to x, and overload; None = repaint all.
    shown: Option<(u16, u16, bool)>,
}

impl Bar {
    pub const fn new(x: u16, y: u16, w: u16, h: u16, bipolar: bool) -> Self {
        Bar { x, y, w, h, bipolar, shown: None }
    }

    fn th(&self) -> u16 {
        self.h - TICK_H
    }

    /// Track (empty) and ticks: every 10000 counts, longer at 0 / 30000 /
    /// 60000. Forces the next `update` to paint every column.
    pub fn draw_static(&mut self, bg: Color) {
        lcd::fill_rect(self.x, self.y, self.w, self.th(), TRACK);
        lcd::fill_rect(self.x, self.y + self.th(), self.w, TICK_H, bg);
        let n: i32 = if self.bipolar { 12 } else { 6 };
        for k in 0..=n {
            let tx = self.x + ((self.w as i32 - 1) * k / n) as u16;
            let major = if self.bipolar { (k - 6) % 3 == 0 } else { k % 3 == 0 };
            lcd::fill_rect(tx, self.y + self.th(), 1, if major { TICK_H } else { TICK_H / 2 }, TICK);
        }
        let (a, b) = self.span(0.0);
        self.shown = Some((a, b, false));
    }

    /// Columns [a, b) filled for fraction `f`.
    fn span(&self, f: f32) -> (u16, u16) {
        let w = self.w as f32;
        if self.bipolar {
            let mid = self.w / 2;
            let n = (f.abs() * (w / 2.0) + 0.5) as u16;
            if f < 0.0 { (mid - n.min(mid), mid) } else { (mid, (mid + n).min(self.w)) }
        } else {
            (0, ((f.max(0.0) * w + 0.5) as u16).min(self.w))
        }
    }

    /// Show fraction `f` (`frac`; None = overload).
    pub fn update(&mut self, f: Option<f32>) {
        let (a, b, ol) = match f {
            Some(f) => {
                let (a, b) = self.span(if self.bipolar { f } else { f.abs() });
                (a, b, false)
            }
            None => (0, self.w, true),
        };
        let th = self.th();
        let col = |x0: u16, x1: u16, c: Color| {
            if x1 > x0 {
                lcd::fill_rect(self.x + x0, self.y, x1 - x0, th, c);
            }
        };
        match self.shown {
            Some((pa, pb, pol)) if pol == ol && !ol => {
                // Columns that changed state: [min(a,pa), max(a,pa)) and the
                // same at the right end (spans are contiguous).
                let fill = |x: u16| x >= a && x < b;
                for (s, e) in [(a.min(pa), a.max(pa)), (b.min(pb), b.max(pb))] {
                    // Each run is all-filled or all-empty in the new state.
                    if e > s {
                        col(s, e, if fill(s) { FILL } else { TRACK });
                    }
                }
            }
            Some((_, _, pol)) if pol == ol && ol => {}
            _ => {
                col(0, self.w, TRACK);
                col(a, b, if ol { OL } else { FILL });
            }
        }
        self.shown = Some((a, b, ol));
    }
}
