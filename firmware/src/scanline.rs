//! Software tearing sync for the pressed feedback (app_press.rs). The
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
//! scan. Region C (link-native.x): per press, and DBG.

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
}

static mut S: Sync = Sync { cal: 0, lines: 480, frame_us: 0, dir: 1, on: true, ns_px: 200, last_us: 0, wait_us: 0, start: 0, ok: false };

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

/// Before writing the box x0..=x1, y0..=y1 (`area` pixels): wait, at most
/// a frame and 2 ms, until the scan is outside the box and will not reach
/// it within the expected write time (+ 0.5 ms). Returns the start cycle
/// count for `done`.
#[inline(never)]
pub fn wait(x0: u16, y0: u16, x1: u16, y1: u16) -> u32 {
    let st = s();
    if st.on && st.cal == 0 && !crate::hy3131::streaming() {
        calibrate();
    }
    let st = *s();
    let t_start = cyc();
    let mut ok = false;
    let mut l = 0;
    // Not while the AD2 stream ISR needs the bus and the CPU (the scope /
    // Wave stream, an armed inrush capture).
    if st.on && st.cal == 1 && !crate::hy3131::streaming() {
        let (a, b) = match st.dir {
            0 => (x0, x1),
            1 => (479 - x1.min(479), 479 - x0.min(479)),
            2 => (y0, y1),
            _ => (319 - y1.min(319), 319 - y0.min(319)),
        };
        let n = st.lines as u32;
        let area = (x1 - x0 + 1) as u32 * (y1 - y0 + 1) as u32;
        let need_us = area * st.ns_px / 1000 + 500;
        // Lines the scan passes while we write (at least the next line).
        let need = (need_us * n / st.frame_us.max(1)).max(1);
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
    let w = s();
    w.wait_us = cyc().wrapping_sub(t_start) / CYC_US;
    w.start = l;
    w.ok = ok;
    cyc()
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
/// <last wait us>,<line at its write>,<window found>,<raw dummy>,<raw hi>,
/// <raw lo>;<us>:<line>,...
#[inline(never)]
pub fn query(out: &mut crate::scpi::Scpi) {
    s().cal = 0;
    calibrate();
    let r = raw();
    let st = *s();
    for (i, v) in [st.cal as u32, st.lines as u32, st.frame_us, st.dir as u32, st.on as u32, st.last_us, st.ns_px, st.wait_us, st.start as u32, st.ok as u32, r[0] as u32, r[1] as u32, r[2] as u32]
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
