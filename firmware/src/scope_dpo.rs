//! Digital phosphor (Display > Persist DPO / HOT, feature `dpo`): CH1's
//! trace as a hit count per graph cell (1 column x CELL frame rows), drawn
//! by `scope_ui::Graph::flush` in a brightness (DPO) or heat (HOT) ramp.
//!
//! Counts are 4-bit and logarithmic: a frame's hit raises a cell's count c
//! with probability 2^(-c/4), and every DECAY frames every count drops by
//! one, so a cell hit in a share p of the frames settles near
//! 4 * log2(p * DECAY): an always-hit cell at the top (15), one hit in a
//! quarter of the frames at 8, a rare one flashes at 1 and fades.
//!
//! Memory: NCOL x NB nibbles (13 965 B) and the dirty cells (798 B) in the
//! data logger's ring slots, lent while no stream session runs
//! (`logger::lend_slots`; none of its own in SRAM, the scope record keeps
//! its length). While a session has them (CH2 on, LOG:STATe ON, a fast log)
//! the phosphor is off: `ready` false, CH1 drawn as a plain trace.

use crate::lcd::Color;
use crate::scope_ui as su;

pub const ENABLED: bool = cfg!(feature = "dpo") || cfg!(not(target_os = "none"));
/// Frame rows per cell and cells per column (the full page frame's rows
/// 1..280; the combined view uses the first ones).
pub const CELL: i16 = 4;
pub const NB: usize = 70;
const BYTES: usize = if ENABLED { su::NCOL * NB / 2 } else { 0 };
/// Frames between two decay steps.
pub const DECAY: u8 = 16;

/// The lent memory.
#[repr(C)]
struct Mem {
    cnt: [u8; BYTES],
    /// Per column: cells to repaint (first, last + 1; last + 1 == 0: none,
    /// so all zero is "nothing").
    dirty: [(u8, u8); su::NCOL],
}

#[cfg(target_os = "none")]
const _: () = assert!(core::mem::size_of::<Mem>() <= crate::logger::SLOT_BYTES, "phosphor counts exceed the logger ring");

pub struct Dpo {
    /// The lent memory as of the last `mem` (null: none).
    m: *mut Mem,
    any: bool,
    rng: u32,
    frames: u8,
    /// Frame height the counts were taken at (another view: start over).
    h: i16,
}

#[cfg_attr(target_os = "none", link_section = ".sram_bss")]
static mut D: Dpo = Dpo { m: core::ptr::null_mut(), any: false, rng: 0, frames: 0, h: 0 };

/// Host builds (ui-render, touch-test): always there.
#[cfg(not(target_os = "none"))]
static mut HOST: Mem = Mem { cnt: [0; BYTES], dirty: [(0, 0); su::NCOL] };

fn d() -> &'static mut Dpo {
    // SAFETY: main loop only (the scope page and its graph flush).
    unsafe { &mut *core::ptr::addr_of_mut!(D) }
}

/// The counts' memory now (None: the logger's session has it). Freshly
/// lent: zeroed and every cell marked to repaint (the screen may still
/// show counts of an earlier lend).
fn mem() -> Option<&'static mut Mem> {
    if !ENABLED {
        return None;
    }
    #[cfg(target_os = "none")]
    {
        let s = d();
        let Some((p, fresh)) = crate::logger::lend_slots() else {
            s.m = core::ptr::null_mut();
            return None;
        };
        s.m = p as *mut Mem;
        // SAFETY: lent to the main loop until the next session's start,
        // which no caller of this module spans.
        let m = unsafe { &mut *s.m };
        if fresh {
            m.cnt.fill(0);
            m.dirty = [(0, NB as u8); su::NCOL];
            s.any = true;
            s.frames = 0;
        }
        Some(m)
    }
    #[cfg(not(target_os = "none"))]
    {
        d().m = unsafe { core::ptr::addr_of_mut!(HOST) };
        Some(unsafe { &mut *d().m })
    }
}

/// The memory if it is lent already (clearing needs no new lend: fresh
/// memory starts empty).
fn held() -> Option<&'static mut Mem> {
    #[cfg(target_os = "none")]
    if !crate::logger::slots_lent() {
        return None;
    }
    mem()
}

/// The phosphor can run (its memory is lent); refreshes the memory for
/// `get` / `take` (Graph::flush calls it first).
#[inline(never)]
pub fn ready() -> bool {
    mem().is_some()
}

/// Raise thresholds: P(raise | count c) = 2^(-c/4), as u32 fractions.
const RAISE: [u32; 16] = {
    let mut t = [0u32; 16];
    // 2^(-k/4) for k = 0..3, then halvings.
    let q = [u32::MAX, 3_611_622_602, 3_037_000_499, 2_553_802_833];
    let mut c = 0;
    while c < 16 {
        t[c] = q[c % 4] >> (c / 4);
        c += 1;
    }
    t
};

#[inline]
fn rnd(s: &mut u32) -> u32 {
    let mut x = if *s == 0 { 0x9E37_79B9 } else { *s };
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *s = x;
    x
}

#[inline]
fn mark(dt: &mut (u8, u8), b: usize) {
    if dt.1 == 0 {
        *dt = (b as u8, b as u8 + 1);
    } else {
        dt.0 = dt.0.min(b as u8);
        dt.1 = dt.1.max(b as u8 + 1);
    }
}

/// Count of cell `b` in column `i` (0: none).
#[inline]
pub fn get(i: usize, b: usize) -> u8 {
    if !ENABLED || b >= NB {
        return 0;
    }
    let m = d().m;
    if m.is_null() {
        return 0;
    }
    let k = i * NB + b;
    // SAFETY: set by the flush's `ready` this pass.
    let v = unsafe { (*m).cnt[k >> 1] };
    if k & 1 == 0 { v & 15 } else { v >> 4 }
}

/// Cell of frame row `fy` (rows from 1).
#[inline]
pub fn cell_of(fy: i16) -> usize {
    ((fy - 1).max(0) / CELL) as usize
}

/// Forget every count (settings changed, mode off); the columns that had
/// any are repainted.
#[cfg_attr(target_os = "none", link_section = ".cold")]
pub fn clear() {
    let Some(m) = held() else {
        return;
    };
    let s = d();
    for i in 0..su::NCOL {
        let col = &mut m.cnt[i * NB / 2..(i + 1) * NB / 2];
        let (mut lo, mut hi) = (u8::MAX, 0u8);
        for (j, v) in col.iter_mut().enumerate() {
            if *v != 0 {
                lo = lo.min(2 * j as u8);
                hi = 2 * j as u8 + 1;
                *v = 0;
            }
        }
        if lo <= hi {
            mark(&mut m.dirty[i], lo as usize);
            mark(&mut m.dirty[i], hi as usize);
            s.any = true;
        }
    }
    s.frames = 0;
}

/// One fresh frame: `tr` = CH1's frame rows per column (EMPTY: none),
/// frame height `h`.
pub fn frame(tr: &[(i16, i16); su::NCOL], h: i16) {
    if mem().is_none() {
        return;
    }
    if d().h != h {
        clear();
        d().h = h;
    }
    let Some(m) = mem() else {
        return;
    };
    let s = d();
    let mut r = s.rng;
    for (i, &(a, b)) in tr.iter().enumerate() {
        if a > b {
            continue;
        }
        let (c0, c1) = (cell_of(a.max(1)), cell_of(b.min(h - 2)).min(NB - 1));
        for c in c0..=c1 {
            let k = i * NB + c;
            let v = &mut m.cnt[k >> 1];
            let sh = (k & 1) * 4;
            let n = (*v >> sh) & 15;
            if n < 15 && (n == 0 || rnd(&mut r) < RAISE[n as usize]) {
                *v += 1 << sh;
                mark(&mut m.dirty[i], c);
                s.any = true;
            }
        }
    }
    s.rng = r;
    s.frames += 1;
    if s.frames >= DECAY {
        s.frames = 0;
        for i in 0..su::NCOL {
            for j in 0..NB / 2 {
                let v = &mut m.cnt[i * NB / 2 + j];
                if *v == 0 {
                    continue;
                }
                let (lo, hi) = (*v & 15, *v >> 4);
                *v = lo.saturating_sub(1) | hi.saturating_sub(1) << 4;
                if lo != 0 {
                    mark(&mut m.dirty[i], 2 * j);
                }
                if hi != 0 {
                    mark(&mut m.dirty[i], 2 * j + 1);
                }
                s.any = true;
            }
        }
    }
}

/// Any column to repaint.
#[inline]
pub fn any() -> bool {
    ENABLED && d().any
}

/// The frame rows of column `i` to repaint (taken: reset).
#[inline]
pub fn take(i: usize) -> Option<(i16, i16)> {
    let m = d().m;
    if m.is_null() {
        return None;
    }
    // SAFETY: set by the flush's `ready` this pass.
    let dt = unsafe { &mut (*m).dirty[i] };
    if dt.1 == 0 {
        return None;
    }
    let r = (1 + dt.0 as i16 * CELL, dt.1 as i16 * CELL);
    *dt = (0, 0);
    Some(r)
}

/// Nothing to repaint (the phosphor is not shown).
pub fn discard() {
    if let Some(m) = held() {
        m.dirty = [(0, 0); su::NCOL];
    }
    d().any = false;
}

/// Every column painted: nothing left to repaint.
pub fn done() {
    if ENABLED {
        d().any = false;
    }
}

/// Count -> colour: `heat` false: the trace colour over the graph fill in
/// 12 steps (the top three counts at full colour, so the settled core does
/// not pulse with the decay step); true: the heat ramp of the interface's
/// colours (blue .. red).
#[cfg_attr(target_os = "none", link_section = ".cold")]
pub fn colours(trace: Color, heat: bool) -> [Color; 16] {
    let mut t = [su::C_FILL; 16];
    const HEAT: [Color; 8] = [0x001f, 0x02bf, 0x07ff, 0x07e0, 0xffe0, 0xfd20, 0xfa00, 0xf800];
    const HI: [u8; 16] = [0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 7, 7];
    for c in 1..16usize {
        t[c] = if heat {
            HEAT[HI[c] as usize]
        } else {
            let a = 56 + (c.min(13) as u32 - 1) * (255 - 56) / 12;
            su::blend565(trace, su::C_FILL, a as u8)
        };
    }
    t
}
