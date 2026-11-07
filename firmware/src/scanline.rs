//! Software tearing sync for lcd::tear_free (every redrawn box: badges,
//! labels, steppers, the press feedback, popups). The
//! panel's TE output reaches no MCU pin we know of (DBG:TE?, #151), so a
//! box written while the panel's refresh scan crosses it shows half old,
//! half new (owner, device 2d47c562: tearing on the menu tiles). The
//! ILI9488 reports the line it is refreshing (Get Scanline, 0x45: a dummy
//! read, then GTS[9:8], GTS[7:0]); before a box is written, `wait` holds
//! until the scan has just left the box and will not reach it again before
//! the write ends (the box's last write time, DWT).
//!
//! Inferred, to be checked on the device (DBG:SCANLINE?): that 0x45 moves
//! at all on this panel, and which screen axis the scan sweeps. The refresh
//! runs along the panel's 480 gate lines; with MADCTL 0xE8 (MV | MX | MY)
//! our x is the gate line, mirrored (DIR 1: line = 479 - x) — or not
//! (DIR 0). DBG:SCANLINE:DIR switches it (2 / 3: y, 319 - y).
//!
//! Our writes go row by row across the box (lcd::strips_box), across the
//! scan, not along it: the box must be written between two passes of the
//! scan. With the scan along x a box of most of the screen's width has no
//! such window: `tear_free` then writes it in two halves, each just
//! behind the scan (`chase`). Region C (link-native.x): per box redraw
//! (never while the AD2 stream runs: lcd::tear_free checks), and DBG.

use crate::lcd;

#[derive(Clone, Copy)]
struct Sync {
    /// 0 not measured yet, 1 the scan line moves, 2 it does not (no sync).
    cal: u8,
    /// Lines per frame (the highest seen + 1) and the frame period (us).
    lines: u16,
    frame_us: u32,
    /// Which screen axis the line maps to (module doc).
    dir: u8,
    on: bool,
    /// Write time per box pixel (ns, compose + send), from the last box.
    ns_px: u32,
    /// Last box: write time, wait, the line at the start of the write and
    /// whether the wait found its window.
    last_us: u32,
    wait_us: u32,
    start: u16,
    ok: bool,
    /// Boxes written in two halves behind the scan, and those of them
    /// that missed the frame (DBG:SCANLINE?).
    chases: u16,
    chase_miss: u16,
}

static mut S: Sync = Sync { cal: 0, lines: 480, frame_us: 0, dir: 1, on: true, ns_px: 200, last_us: 0, wait_us: 0, start: 0, ok: false, chases: 0, chase_miss: 0 };

fn s() -> &'static mut Sync {
    // Main loop only (the press path, SCPI DBG).
    unsafe { &mut *core::ptr::addr_of_mut!(S) }
}

#[inline(always)]
fn cyc() -> u32 {
    #[cfg(target_os = "none")]
    unsafe {
        core::ptr::read_volatile(0xE000_1004 as *const u32)
    }
    #[cfg(not(target_os = "none"))]
    0
}

const CYC_US: u32 = crate::clock::HCLK_HZ / 1_000_000;

/// One Get Scanline: the three words read (dummy, GTS[9:8], GTS[7:0]).
#[inline(never)]
fn raw() -> [u16; 3] {
    lcd::defer_flush();
    lcd::cmd(0x45);
    [lcd::read_data(), lcd::read_data(), lcd::read_data()]
}

fn line() -> u16 {
    let r = raw();
    (r[1] & 3) << 8 | (r[2] & 0xFF)
}

/// Measure the frame: up to 60 ms of reads; two wraps (the line falling by
/// more than half the frame) give the period. Fewer than 20 changes or no
/// two wraps: the line does not move, no sync.
#[inline(never)]
fn calibrate() {
    let t0 = cyc();
    let (mut prev, mut max, mut changes) = (line(), 0u16, 0u32);
    let mut wraps = [0u32; 2];
    let mut nw = 0;
    while cyc().wrapping_sub(t0) < 60_000 * CYC_US {
        let l = line();
        if l != prev {
            changes += 1;
        }
        max = max.max(l);
        if l + 100 < prev {
            if nw < 2 {
                wraps[nw] = cyc();
            }
            nw += 1;
            if nw >= 2 && changes >= 20 {
                break;
            }
        }
        prev = l;
    }
    let st = s();
    if nw >= 2 && changes >= 20 && max >= 200 {
        st.cal = 1;
        st.lines = max + 1;
        st.frame_us = wraps[1].wrapping_sub(wraps[0]) / CYC_US;
    } else {
        st.cal = 2;
    }
}

/// The box's scan lines a..=b (`dir`: module doc).
fn span(dir: u8, x0: u16, y0: u16, x1: u16, y1: u16) -> (u16, u16) {
    match dir {
        0 => (x0, x1),
        1 => (479 - x1.min(479), 479 - x0.min(479)),
        2 => (y0, y1),
        _ => (319 - y1.min(319), 319 - y0.min(319)),
    }
}

/// The part of the box x0..=x1, y0..=y1 on scan lines a..=b.
fn part(dir: u8, bx: (u16, u16, u16, u16), a: u16, b: u16) -> (u16, u16, u16, u16) {
    let (x0, y0, x1, y1) = bx;
    match dir {
        0 => (a, y0, b, y1),
        1 => (479 - b, y0, 479 - a, y1),
        2 => (x0, a, x1, b),
        _ => (x0, 319 - b, x1, 319 - a),
    }
}

/// The sync can run: measured, on, no AD2 stream (it needs the bus and
/// the CPU: the scope Wave view, an armed inrush capture).
fn ready() -> bool {
    let st = s();
    if st.on && st.cal == 0 && !crate::hy3131::streaming() {
        calibrate();
    }
    let st = s();
    st.on && st.cal == 1 && !crate::hy3131::streaming()
}

/// Before writing the box x0..=x1, y0..=y1: wait, at most a frame and 2
/// ms, until the scan is outside the box and will not reach it within the
/// expected write time (+ 0.5 ms). No wait when no such window exists
/// (the box and the write time cover the frame: `tear_free` writes such
/// a box in two halves behind the scan). Returns the start cycle count
/// for `done`.
#[inline(never)]
pub fn wait(x0: u16, y0: u16, x1: u16, y1: u16) -> u32 {
    let t_start = cyc();
    let mut ok = false;
    let mut l = 0;
    if ready() {
        let st = *s();
        let (a, b) = span(st.dir, x0, y0, x1, y1);
        let need = need_lines(&st, x0, y0, x1, y1);
        let n = st.lines as u32;
        if (b - a + 1) as u32 + need < n {
            let limit = (st.frame_us + 2000) * CYC_US;
            loop {
                l = line();
                let lv = l as u32;
                if lv < a as u32 || lv > b as u32 {
                    let dist = (a as u32 + n - lv) % n;
                    if dist >= need {
                        ok = true;
                        break;
                    }
                }
                if cyc().wrapping_sub(t_start) > limit {
                    break;
                }
            }
        }
    }
    let w = s();
    w.wait_us = cyc().wrapping_sub(t_start) / CYC_US;
    w.start = l;
    w.ok = ok;
    cyc()
}

/// Lines the scan passes while the box is written (the last boxes' time
/// per pixel, + 0.5 ms; at least the next line).
fn need_lines(st: &Sync, x0: u16, y0: u16, x1: u16, y1: u16) -> u32 {
    let area = (x1 - x0 + 1) as u32 * (y1 - y0 + 1) as u32;
    let need_us = area * st.ns_px / 1000 + 500;
    (need_us * st.lines as u32 / st.frame_us.max(1)).max(1)
}

/// After the box's write (started at `t0`, `area` pixels): its time, and
/// the per-pixel estimate for the next box (the larger of this one and
/// 7/8 of the last: slow to fall).
#[inline(never)]
pub fn done(t0: u32, area: u32) {
    let st = s();
    st.last_us = cyc().wrapping_sub(t0) / CYC_US;
    let ns = (st.last_us as u64 * 1000 / area.max(1) as u64) as u32;
    st.ns_px = ns.max(st.ns_px * 7 / 8).max(20);
}

/// A box of an eighth of the screen or more (`tear_free`).
const LARGE: u32 = 480 * 320 / 8;

/// lcd::tear_free's body (region C; the caller checked the AD2 stream,
/// strip and batch state). Composed in the strip buffer when it is free
/// (no disk job, no scope FFT job in flight for a small box: taking it
/// drops the job), else drawn directly with its large fills deferred. A box the
/// scan can be outrun on (`wait`) is written in one go; a wider one
/// whose compose fits a frame is written in two halves, each just after
/// the scan has left it, both in the same frame ("chase", `chase`).
/// What fits neither (the scope popups: 466 x 200+ px, 35..137 ms of
/// composing, 2..8 frames) is composed without a wait: the panel shows
/// the old box above, the new one below a line moving down.
#[inline(never)]
pub fn tear_free(x0: u16, y0: u16, x1: u16, y1: u16, draw: &mut dyn FnMut()) {
    let area = (x1 - x0 + 1) as u32 * (y1 - y0 + 1) as u32;
    // A large box (a popup, a dialog: a user's action) may drop the FFT
    // job (it is computed again); the small ones redrawn every frame must
    // not, or a spectrum would never finish.
    let buf = if area < LARGE && crate::app::fft_busy() { None } else { crate::diskbuf::strip_take() };
    let Some(buf) = buf else {
        let t0 = wait(x0, y0, x1, y1);
        lcd::draw_deferred(draw);
        done(t0, area);
        return;
    };
    let fits = ready() && {
        let st = *s();
        let (a, b) = span(st.dir, x0, y0, x1, y1);
        (b - a + 1) as u32 + need_lines(&st, x0, y0, x1, y1) < st.lines as u32
    };
    if fits || !ready() {
        let t0 = wait(x0, y0, x1, y1);
        lcd::strips_box(buf, x0, y0, x1, y1, draw);
        done(t0, area);
    } else if !chase(buf, (x0, y0, x1, y1), draw) {
        // Its time only: a popup's slow compose (~1 us/px) would hold the
        // estimate up and keep the small boxes from their windows.
        let t0 = cyc();
        lcd::strips_box(buf, x0, y0, x1, y1, draw);
        s().last_us = cyc().wrapping_sub(t0) / CYC_US;
    }
    if !crate::diskbuf::strip_give() {
        // A disk user took the buffer meanwhile: draw it again.
        lcd::draw_deferred(draw);
    }
}

/// A wide box (the scan cannot be outrun on it) in two halves along the
/// scan, in scan order, each composed and sent once the scan has passed
/// it and long before it comes back: the panel shows the old box for a
/// whole frame and the new one from the next. Each half costs a compose
/// of the whole box's rows (`draw` paints them all; strips_box keeps the
/// half): false (nothing drawn) when two composes do not fit in the time
/// the scan gives a half. Bounded: two frames and 2 ms of waiting.
#[inline(never)]
fn chase(buf: &mut [lcd::Color], bx: (u16, u16, u16, u16), draw: &mut dyn FnMut()) -> bool {
    let st = *s();
    let (x0, y0, x1, y1) = bx;
    let (a, b) = span(st.dir, x0, y0, x1, y1);
    let n = st.lines as u32;
    let line_ns = (st.frame_us as u64 * 1000 / n as u64) as u32;
    let area = (x1 - x0 + 1) as u32 * (y1 - y0 + 1) as u32;
    // One half: the whole box's compose (the estimate per box pixel).
    let half_us = area * st.ns_px / 1000 + 500;
    let m = a + (b - a) / 2;
    let half_lines = (b - m) as u32 + 1;
    // A half must be sent before the scan comes back to it (the frame
    // less its own lines), the second one at most a frame after the
    // first's start.
    if half_us * 1000 >= n.saturating_sub(half_lines) * line_ns || 2 * half_us >= st.frame_us {
        return false;
    }
    let need = half_us * 1000 / line_ns.max(1) + 1;
    let t_start = cyc();
    let limit = (2 * st.frame_us + 2000) * CYC_US;
    let l0 = line() as u32;
    // A half the scan passed in this frame but comes back to before it is
    // sent (the first one now, the second after the first): the next frame.
    let late1 = l0 > a as u32 && a as u32 + n - l0 < need;
    let late2 = l0 > m as u32 && m as u32 + 1 + n - l0 < 2 * need;
    let off = if late1 || late2 { n } else { 0 };
    let (mut prev, mut wraps) = (l0, 0u32);
    let mut ok = true;
    let mut pos = || {
        let l = line() as u32;
        if l + n / 2 < prev {
            wraps += 1;
        }
        prev = l;
        l + wraps * n
    };
    for (la, lb) in [(a, m), (m + 1, b)] {
        while pos() <= lb as u32 + off {
            if cyc().wrapping_sub(t_start) > limit {
                ok = false;
                break;
            }
        }
        let p = part(st.dir, bx, la, lb);
        lcd::strips_box(buf, p.0, p.1, p.2, p.3, draw);
        // Sent before the scan came back to the half.
        ok &= pos() < la as u32 + off + n;
    }
    let w = s();
    w.wait_us = cyc().wrapping_sub(t_start) / CYC_US;
    w.start = l0 as u16;
    w.ok = ok;
    w.chases = w.chases.wrapping_add(1);
    if !ok {
        w.chase_miss = w.chase_miss.wrapping_add(1);
    }
    true
}

/// DBG:SCANLINE:DIR n / :SYNC 0|1.
pub fn set(dir: Option<u8>, on: Option<bool>) {
    let st = s();
    if let Some(d) = dir {
        st.dir = d & 3;
    }
    if let Some(o) = on {
        st.on = o;
    }
}

/// DBG:SCANLINE?: measures again, then the state and 24 samples 1 ms
/// apart: <cal>,<lines>,<frame us>,<dir>,<sync>,<last box us>,<ns/px>,
/// <last wait us>,<line at its write>,<window found>,<chased boxes>,
/// <chases that missed the frame>,<raw dummy>,<raw hi>,
/// <raw lo>;<us>:<line>,...
#[inline(never)]
pub fn query(out: &mut crate::scpi::Scpi) {
    s().cal = 0;
    calibrate();
    let r = raw();
    let st = *s();
    for (i, v) in [st.cal as u32, st.lines as u32, st.frame_us, st.dir as u32, st.on as u32, st.last_us, st.ns_px, st.wait_us, st.start as u32, st.ok as u32, st.chases as u32, st.chase_miss as u32, r[0] as u32, r[1] as u32, r[2] as u32]
        .iter()
        .enumerate()
    {
        if i > 0 {
            out.put_bytes(b",");
        }
        out.put_u32(*v);
    }
    out.put_bytes(b";");
    let t0 = cyc();
    for k in 0..24u32 {
        while cyc().wrapping_sub(t0) < k * 1000 * CYC_US {}
        let t = cyc().wrapping_sub(t0) / CYC_US;
        let l = line();
        if k > 0 {
            out.put_bytes(b",");
        }
        out.put_u32(t);
        out.put_bytes(b":");
        out.put_u32(l as u32);
    }
}
