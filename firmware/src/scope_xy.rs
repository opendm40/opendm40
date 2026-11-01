//! The scope's X-Y display (docs/scope-gen-channel.md section 3): CH1 (Y)
//! against the generator (X, computed from the record's generator phase)
//! or against the DMM CH2 (X, its samples at their DWT time).
//!
//! The plot is a 1-bit bitmap of the graph frame (column-major, one bit per
//! frame row), drawn by `scope_ui::Graph::flush` in the trace colour. A
//! column whose bits changed keeps the rows to repaint (`rep`) until the
//! flush has painted it. Persistence: the page clears the bitmap per frame
//! (Persist OFF), every `clear_ms` (MIN, 1S) or never (INF).
//!
//! Placement: the per-sample loops run for every frame (region B, the
//! default); nothing here runs in an interrupt. Main loop only.

use crate::scope_ui::EMPTY;

/// Bitmap words per column: frame rows 0..288 (the full page frame is 281
/// rows high).
const W: usize = 9;
pub const COLS: usize = 399;
/// Points plotted per frame at most (the frame's samples, decimated).
const MAX_PTS: usize = 4096;

pub struct Plot {
    bits: Bits,
    /// What the bitmap was drawn with (`key`); a change clears it.
    key: u32,
    /// The next persistence clear (ms).
    t_clear: u64,
}

/// The bitmap, one column per entry (bit r of the column = frame row r).
pub struct Bits([[u32; W]; COLS]);

/// Rows to repaint per column (EMPTY: none) and whether any column has
/// some: written by the plot, taken by the graph's flush (a static of its
/// own: the flush reads the bitmap meanwhile).
struct Rep {
    r: [(i16, i16); COLS],
    dirty: bool,
}

// SRAM (zeroed at reset; no TCM .bss): 14.4 KB + 1.6 KB. A zero `Rep` is
// "row 0 to repaint" in every column: harmless (`dirty` false, the first
// flush of an X-Y frame is a full repaint).
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut PLOT: Plot = Plot { bits: Bits([[0; W]; COLS]), key: 0, t_clear: 0 };
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut REP: Rep = Rep { r: [(0, 0); COLS], dirty: false };

pub fn plot() -> &'static mut Plot {
    // SAFETY: main loop only, no reference kept across another call.
    unsafe { &mut *core::ptr::addr_of_mut!(PLOT) }
}

/// The bitmap, read only (the graph's flush).
pub fn bits() -> &'static Bits {
    unsafe { &(*core::ptr::addr_of!(PLOT)).bits }
}

fn rep() -> &'static mut Rep {
    unsafe { &mut *core::ptr::addr_of_mut!(REP) }
}

/// The rows of column `i` to repaint (EMPTY: none); taken (cleared).
#[inline(always)]
pub fn take_rep(i: usize) -> (i16, i16) {
    let r = rep();
    let v = r.r[i];
    r.r[i] = EMPTY;
    v
}

/// Any column to repaint.
pub fn dirty() -> bool {
    rep().dirty
}

/// The flush painted every column (each `rep` taken).
pub fn clean() {
    rep().dirty = false;
}

impl Bits {
    /// Column `i` (0..399 = frame columns 1..399), frame row `fy` is set.
    #[inline(always)]
    pub fn bit(&self, i: usize, fy: i16) -> bool {
        let r = fy as usize;
        r < 32 * W && self.0[i][r / 32] >> (r % 32) & 1 != 0
    }
}

/// The X column of an X-Y CSV (scope_csv.rs `set_x`): the generator
/// against the stored record (its BUF[0]), or the DMM CH2 store with the
/// DWT time of BUF[0] and cycles per sample.
#[derive(Clone, Copy)]
pub enum XSrc {
    Gen(crate::gen::Sync),
    Ch2 { t0: u32, step: f32 },
}

struct CsvX {
    src: XSrc,
    run: crate::scope_ch2::Run,
}

// SRAM (`.sram_bss`, an all-zero initial value).
#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut CSVX: CsvX = CsvX {
    src: XSrc::Gen(crate::gen::Sync { t0: 0.0, s: 0.0, q: 0.0, k: 0.0, shape: crate::gen::Shape { wave: crate::gen::Wave::Sine, lo_v: 0.0, hi_v: 0.0, duty: 0.0 } }),
    run: crate::scope_ch2::Run::ZERO,
};

/// Use `src` for `csv_x` from now on.
pub fn csv_set(src: XSrc) {
    let c = unsafe { &mut *core::ptr::addr_of_mut!(CSVX) };
    c.src = src;
    c.run = crate::scope_ch2::store().run();
}

/// The X value of record sample `i` (scope_csv::XFn): the generator's
/// volts at its phase, or the CH2 sample nearest its time (rising `i`: an
/// incremental lookup; a step back costs one full search).
pub fn csv_x(i: usize) -> Option<f32> {
    let c = unsafe { &mut *core::ptr::addr_of_mut!(CSVX) };
    match c.src {
        XSrc::Gen(g) => Some(g.shape.volts(g.phase(i as f64))),
        XSrc::Ch2 { t0, step } => crate::scope_ch2::store().value_run(&mut c.run, t0.wrapping_add((i as f64 * step as f64) as u64 as u32)),
    }
}

/// Persistence clear interval per Persist setting (OFF, MIN, 1S, INF; OFF
/// clears every frame).
pub const CLEAR_MS: [u64; 4] = [0, 80, 1000, u64::MAX];

/// Screen mapping of one frame: frame row = ya + yb * code (CH1 code), frame
/// column = x0 + x * kx (x in the X source's unit; x0 = the column of
/// x = 0, plus 0.5 for the rounding), `h` the frame height.
#[derive(Clone, Copy)]
pub struct Map {
    pub ya: f32,
    pub yb: f32,
    pub x0: f32,
    pub kx: f32,
    pub h: i16,
}

impl Plot {
    fn mark(&mut self, i: usize, a: i16, b: i16) {
        let r = rep();
        let c = &mut r.r[i];
        *c = (c.0.min(a), c.1.max(b));
        r.dirty = true;
    }

    /// Clear every bit (the rows that had any are repainted).
    pub fn clear(&mut self) {
        for i in 0..COLS {
            let c = &self.bits.0[i];
            let (mut lo, mut hi) = (-1i32, -1i32);
            for (k, &w) in c.iter().enumerate() {
                if w != 0 {
                    if lo < 0 {
                        lo = (32 * k as u32 + w.trailing_zeros()) as i32;
                    }
                    hi = (32 * k as u32 + 31 - w.leading_zeros()) as i32;
                }
            }
            if lo >= 0 {
                self.bits.0[i] = [0; W];
                self.mark(i, lo as i16, hi as i16);
            }
        }
    }

    /// Clear at the next frame (a settings change).
    pub fn invalidate(&mut self) {
        self.key = u32::MAX;
    }

    /// A new frame's mapping / source: clear when it changed (`key`), or
    /// at the persistence interval `ms` (0: every frame). `now` in ms.
    #[inline(never)]
    pub fn begin(&mut self, key: u32, ms: u64, now: u64) {
        if key != self.key || ms == 0 || (ms != u64::MAX && now >= self.t_clear) {
            self.clear();
            self.key = key;
            self.t_clear = now.saturating_add(ms);
        }
    }

    /// Set frame point (fx, fy) (outside the frame's inside: nothing).
    #[inline(always)]
    fn set(&mut self, fx: i32, fy: i32, h: i16) {
        if fx < 1 || fx > COLS as i32 || fy < 1 || fy > h as i32 - 2 {
            return;
        }
        let (i, r) = ((fx - 1) as usize, fy as usize);
        let w = &mut self.bits.0[i][r / 32];
        let m = 1u32 << (r % 32);
        if *w & m == 0 {
            *w |= m;
            self.mark(i, fy as i16, fy as i16);
        }
    }

    /// A line from the last point to (fx, fy) (`last` None: the point),
    /// at most 64 steps (a longer jump: the end point only). One copy for
    /// both plots (region B).
    #[inline(never)]
    fn to(&mut self, last: &mut Option<(i32, i32)>, fx: i32, fy: i32, h: i16) {
        if let Some((x0, y0)) = *last {
            let (dx, dy) = (fx - x0, fy - y0);
            let n = dx.abs().max(dy.abs());
            if n > 1 && n <= 64 {
                for k in 1..n {
                    self.set(x0 + dx * k / n, y0 + dy * k / n, h);
                }
            }
        }
        self.set(fx, fy, h);
        *last = Some((fx, fy));
    }

    /// Plot CH1 codes `data` (record samples `off ..`) against the generator
    /// `g` (the record's phase): X = the generator's volts.
    #[inline(never)]
    pub fn plot_gen(&mut self, data: &[u16], off: usize, g: &crate::gen::Sync, m: Map) {
        let n = data.len();
        if n < 2 {
            return;
        }
        let step = n.div_ceil(MAX_PTS);
        // The phase in f32 from the first sample on (one f64 phase per
        // frame; f64 per sample is soft-float).
        let ph0 = g.phase(off as f64);
        let dph = (g.s * g.k / g.q) as f32;
        let mut last = None;
        let mut j = 0;
        while j < n {
            let ph = ph0 + j as f32 * dph;
            let x = g.shape.volts(ph - (ph as i32) as f32);
            let fx = (m.x0 + x * m.kx) as i32;
            let fy = (m.ya + m.yb * (data[j] & 0x3FF) as f32) as i32;
            self.to(&mut last, fx, fy, m.h);
            j += step;
        }
    }

    /// Plot CH1 codes `data` (its first sample at DWT `t0`, `step` DWT
    /// cycles per sample) against the DMM CH2 store: one point per CH2
    /// sample in the frame's time, Y = the CH1 sample nearest it.
    #[inline(never)]
    pub fn plot_ch2(&mut self, data: &[u16], t0: u32, step: f32, st: &crate::scope_ch2::Store, m: Map) {
        let n = data.len();
        if n < 2 || step <= 0.0 {
            return;
        }
        let mut run = st.run();
        let span = n as f32 * step;
        let Some((ia, ib)) = st.range_run(&mut run, t0, span) else { return };
        let inv = 1.0 / step;
        let mut last = None;
        let mut k = 0;
        st.each(ia, ib, &mut |t, x| {
            k += 1;
            if k > MAX_PTS {
                return;
            }
            let rel = t.wrapping_sub(t0) as i32;
            if rel < 0 {
                return;
            }
            let j = (rel as f32 * inv + 0.5) as usize;
            if j >= n {
                return;
            }
            let fx = (m.x0 + x * m.kx) as i32;
            let fy = (m.ya + m.yb * (data[j] & 0x3FF) as f32) as i32;
            self.to(&mut last, fx, fy, m.h);
        });
    }
}
