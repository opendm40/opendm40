//! Host test of the touch input path (tools/touch-test/run.sh):
//!   1. touch.rs `Debounce` with synthetic controller frames (jittering
//!      touch-down, quick taps, a one-frame blip, release bounce, release
//!      latency, side keys);
//!   2. gesture.rs `Tracker` tap / swipe / long-press / capture decisions;
//!   3. the DBG:TOUCH? log text;
//!   4. hit map vs. drawing: every page is drawn by the real src/ui.rs into
//!      the LCD emulator of tools/ui-render/host.rs (draw-call probes on);
//!      every drawn button box (an `sf::rbox` shape) whose centre hits
//!      something must hit the SAME target on every one of its pixels, two
//!      buttons must not share a target, and every target must be drawn.
//!      Reported per page (not failures): how far each hit area reaches past
//!      its drawn box, and the dead pixels next to buttons (a tap there does
//!      nothing).
//!   main [-v]     prints "cases N, failures M", exit 1 if M > 0
#![allow(dead_code, unused, static_mut_refs)]

#[path = "../ui-render/host.rs"]
mod host;
pub use host::mmio;

#[path = "../../src/clock.rs"]
mod clock;
#[path = "../../src/dialog.rs"]
mod dialog;
#[path = "../../src/dmm.rs"]
mod dmm;
#[path = "../../src/fonts.rs"]
mod fonts;
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
#[path = "../../src/scope_ui.rs"]
mod scope_ui;
#[path = "../../src/stockfont.rs"]
mod stockfont;
#[path = "../../src/text.rs"]
mod text;
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

use gesture::{Axis, Gesture as G, Tracker};
use host::{emu, H, W};
use touch::{Debounce, Event, Frame, Point};

struct Ck {
    cases: usize,
    fails: usize,
    verbose: bool,
}

impl Ck {
    fn t(&mut self, name: &str, ok: bool) {
        self.cases += 1;
        if !ok {
            self.fails += 1;
            println!("FAIL {name}");
        } else if self.verbose {
            println!("ok   {name}");
        }
    }
}

// ------------------------------------------------------------ 1. debounce

fn pressed(x: u16, y: u16) -> Frame {
    let mut f = Frame { pressed: true, n: 1, ..Frame::default() };
    f.pts[0] = Point { x, y };
    f
}

fn released() -> Frame {
    Frame::default()
}

fn key(code: u8) -> Frame {
    Frame { pressed: true, key: true, code, ..Frame::default() }
}

/// Run frames (time, Some(frame) or None = a poll without a frame) through a
/// fresh debounce; returns (time, event) for every emitted event.
fn deb(seq: &[(u64, Option<Frame>)]) -> Vec<(u64, Event)> {
    let mut d = Debounce::new();
    let mut out = vec![];
    for (t, f) in seq {
        let e = match f {
            Some(f) => d.frame(f, *t),
            None => d.idle(*t),
        };
        if let Some(e) = e {
            out.push((*t, e));
        }
    }
    out
}

/// Polls without a frame every 10 ms in [a, b).
fn idle(a: u64, b: u64) -> Vec<(u64, Option<Frame>)> {
    (a..b).step_by(10).map(|t| (t, None)).collect()
}

fn debounce_tests(ck: &mut Ck) {
    // A finger landing with 1 px of jitter per report (10 ms period), held
    // 100 ms. The old byte-identical rule never confirmed it.
    let mut s: Vec<(u64, Option<Frame>)> = (0..10).map(|i| (i * 10, Some(pressed(200 + (i % 2) as u16, 150 + i as u16)))).collect();
    s.push((100, Some(released())));
    s.extend(idle(110, 200));
    let ev = deb(&s);
    ck.t("jitter: down at the 2nd frame", ev.first().map(|e| (e.0, e.1.points)) == Some((10, 1)));
    ck.t("jitter: down point = 2nd frame", ev.first().map(|e| e.1.p) == Some(Point { x: 201, y: 151 }));
    let up = ev.iter().find(|e| e.1.points == 0).map(|e| e.0);
    ck.t("jitter: release within 25..35 ms of the release frame", up.is_some_and(|t| (125..=135).contains(&t)));
    // Quick tap: 3 reports (all different), release.
    let mut s: Vec<(u64, Option<Frame>)> = vec![(0, Some(pressed(100, 100))), (12, Some(pressed(102, 99))), (24, Some(pressed(103, 101))), (36, Some(released()))];
    s.extend(idle(46, 120));
    let ev = deb(&s);
    ck.t("quick tap: down + up", ev.len() >= 2 && ev[0].1.points == 1 && ev.last().unwrap().1.points == 0);
    // A one-frame blip (noise): nothing.
    let mut s = vec![(0, Some(pressed(300, 50))), (10, Some(released()))];
    s.extend(idle(20, 100));
    ck.t("one-frame blip: no event", deb(&s).is_empty());
    // Controller that only reports on change: one pressed frame, silence.
    let mut s = vec![(0, Some(pressed(300, 50)))];
    s.extend(idle(10, 60));
    let ev = deb(&s);
    ck.t("single frame + silence: down after 40 ms", ev.len() == 1 && ev[0].0 == 40);
    // Release bounce inside a touch: release frame, pressed again 15 ms later.
    let mut s: Vec<(u64, Option<Frame>)> = (0..5).map(|i| (i * 10, Some(pressed(250, 200)))).collect();
    s.push((50, Some(released())));
    s.push((60, None));
    s.push((65, Some(pressed(251, 200))));
    s.extend((75..150).step_by(10).map(|t| (t, Some(pressed(251, 201)))));
    s.push((150, Some(released())));
    s.extend(idle(160, 250));
    let ev = deb(&s);
    let downs = ev.iter().filter(|e| e.1.points > 0).count();
    let ups = ev.iter().filter(|e| e.1.points == 0).count();
    let first_down = ev.iter().position(|e| e.1.points > 0);
    ck.t("release bounce: one touch (one up)", ups == 1 && first_down == Some(0));
    ck.t("release bounce: moves only after the down", downs >= 1);
    // Two release frames: release at once.
    let s = vec![(0, Some(pressed(10, 10))), (10, Some(pressed(10, 10))), (20, Some(released())), (30, Some(released()))];
    ck.t("two release frames: up at the 2nd", deb(&s).last().map(|e| (e.0, e.1.points)) == Some((30, 0)));
    // Side key: down after two frames, up after the release; a key report
    // during a touch is dropped.
    let mut s = vec![(1000, Some(key(2))), (1010, Some(key(2))), (1020, Some(released()))];
    s.extend(idle(1030, 1100));
    let ev = deb(&s);
    ck.t("side key: down + up", ev.len() == 2 && ev[0].1.keys == 4 && ev[1].1.keys == 0);
    let s = vec![(0, Some(pressed(10, 10))), (10, Some(pressed(10, 10))), (20, Some(key(1))), (30, Some(key(1)))];
    ck.t("key report during a touch: dropped", deb(&s).iter().all(|e| e.1.keys == 0));
    // Points are clamped to the panel.
    let s = vec![(0, Some(pressed(600, 400))), (10, Some(pressed(600, 400)))];
    ck.t("clamp to 479,319", deb(&s).first().map(|e| e.1.p) == Some(Point { x: 479, y: 319 }));
}

// ------------------------------------------------------------- 2. gestures

/// One-finger touch along `path` (a point every 10 ms from t = 1000), then
/// released; `cap` = the app captures on Down. Returns the gestures.
fn touch(path: &[(u16, u16)], cap: bool, hold_ms: u64) -> Vec<G> {
    let mut tr = Tracker::new();
    let mut v = vec![];
    let mut t = 1000;
    let mut push = |tr: &mut Tracker, g: Option<G>, v: &mut Vec<G>| {
        if let Some(g) = g {
            if cap && matches!(g, G::Down(_)) {
                tr.capture();
            }
            v.push(g);
        }
    };
    for (i, &(x, y)) in path.iter().enumerate() {
        let mut e = Event { points: 1, p: Point { x, y }, ..Event::default() };
        e.pts[0] = e.p;
        let g = tr.event(&e, t);
        push(&mut tr, g, &mut v);
        if i == 0 && hold_ms > 0 {
            for k in (10..=hold_ms).step_by(10) {
                let g = tr.tick(t + k);
                push(&mut tr, g, &mut v);
            }
            t += hold_ms;
        }
        t += 10;
    }
    let g = tr.event(&Event::default(), t + 30);
    push(&mut tr, g, &mut v);
    v
}

fn line(a: (u16, u16), b: (u16, u16), n: usize) -> Vec<(u16, u16)> {
    (0..=n).map(|i| ((a.0 as i32 + (b.0 as i32 - a.0 as i32) * i as i32 / n as i32) as u16, (a.1 as i32 + (b.1 as i32 - a.1 as i32) * i as i32 / n as i32) as u16)).collect()
}

/// Two-finger touch: frames of (finger a, finger b) points, a frame every
/// 10 ms, then released. `swap`: report the points in alternating slot
/// order (the controller's order is not stable).
fn touch2(frames: &[((u16, u16), (u16, u16))], swap: bool) -> Vec<G> {
    let mut tr = Tracker::new();
    let mut v = vec![];
    let mut t = 1000;
    for (i, &(a, b)) in frames.iter().enumerate() {
        let (p, q) = if swap && i % 2 == 1 { (b, a) } else { (a, b) };
        let mut e = Event { points: 2, p: Point { x: p.0, y: p.1 }, ..Event::default() };
        e.pts[0] = Point { x: p.0, y: p.1 };
        e.pts[1] = Point { x: q.0, y: q.1 };
        if let Some(g) = tr.event(&e, t) {
            v.push(g);
        }
        t += 10;
    }
    if let Some(g) = tr.event(&Event::default(), t + 30) {
        v.push(g);
    }
    v
}

/// Controller frames through the real Debounce and Tracker, as the main
/// loop: `seq` = (time, frame points; empty = a release frame), a poll without
/// a frame (idle + tick) every 5 ms in between. The app is the scope graph:
/// it captures every Down with raw moves (scope_page.rs). Returns the gestures.
fn frames_to_gestures(seq: &[(u64, Vec<(u16, u16)>)]) -> Vec<G> {
    let mut d = Debounce::new();
    let mut tr = Tracker::new();
    let mut v = vec![];
    let mut feed = |tr: &mut Tracker, g: Option<G>, v: &mut Vec<G>| {
        if let Some(g) = g {
            if matches!(g, G::Down(_)) {
                gesture::raw_next(true);
                tr.capture();
            }
            v.push(g);
        }
    };
    let end = seq.last().map(|s| s.0).unwrap_or(0) + 200;
    let mut k = 0;
    let mut t = seq.first().map(|s| s.0).unwrap_or(0);
    while t <= end {
        let ev = if k < seq.len() && seq[k].0 <= t {
            let pts = &seq[k].1;
            k += 1;
            let mut f = Frame { pressed: !pts.is_empty(), n: pts.len() as u8, ..Frame::default() };
            for (i, &(x, y)) in pts.iter().enumerate() {
                f.pts[i] = Point { x, y };
            }
            d.frame(&f, t)
        } else {
            let e = d.idle(t);
            let g = tr.tick(t);
            feed(&mut tr, g, &mut v);
            t += 5;
            e
        };
        if let Some(e) = ev {
            let g = tr.event(&e, t);
            feed(&mut tr, g, &mut v);
        }
    }
    v
}

/// Owner finding: after a pinch the finger lifted last kept dragging. Once a
/// touch had two fingers nothing but the pinch end may come out of it, in
/// any lift order, also when the controller reports a release between the
/// two lifts (a one-finger touch-down shortly after a multi-finger release
/// is the remaining finger, not a new touch).
fn pinch_lift_tests(ck: &mut Ck) {
    let one_finger = |v: &[G]| v.iter().any(|g| matches!(g, G::Down(_) | G::Tap(_) | G::LongPress(_) | G::Drag(..) | G::Move(..) | G::Grab(_) | G::Up(_) | G::SwipeLeft | G::SwipeRight | G::SwipeUp | G::SwipeDown));
    let ends = |v: &[G]| v.iter().filter(|g| matches!(g, G::PinchEnd(_))).count();
    // Horizontal spread 100 -> 200 px (16 ms reports), then one finger lifts
    // and the other moves 60 px down and 40 px right, then lifts.
    let pinch = |lift_a: bool, gap: Option<u64>, first_alone: bool| {
        let mut seq = vec![];
        let mut t = 1000;
        if first_alone {
            for _ in 0..3 {
                seq.push((t, vec![(150, 160)]));
                t += 16;
            }
        }
        for i in 0..=10u16 {
            seq.push((t, vec![(150 - 5 * i, 160), (250 + 5 * i, 162)]));
            t += 16;
        }
        if let Some(g) = gap {
            seq.push((t, vec![]));
            t += g;
        }
        let rest = if lift_a { (300u16, 162u16) } else { (100u16, 160u16) };
        for i in 0..=12u16 {
            seq.push((t, vec![(rest.0 + 3 * i, rest.1 + 5 * i)]));
            t += 16;
        }
        seq.push((t, vec![]));
        frames_to_gestures(&seq)
    };
    for (name, lift_a, gap, first) in [
        ("pinch, lift left, right finger moves", true, None, false),
        ("pinch, lift right, left finger moves", false, None, false),
        ("pinch, release frame + 40 ms gap, last finger moves", true, Some(40), false),
        ("pinch, release frame + 120 ms gap, last finger moves", false, Some(120), false),
        ("1 finger first, pinch, lift, last finger moves", true, None, true),
    ] {
        let v = pinch(lift_a, gap, first);
        let pinched = v.iter().any(|g| matches!(g, G::Pinch(..)));
        let after_cancel = v.iter().skip_while(|g| !matches!(g, G::Cancel | G::Pinch(..))).cloned().collect::<Vec<_>>();
        ck.t(&format!("{name}: pinch, one PinchEnd(true), nothing one-finger after it"), pinched && ends(&v) == 1 && v.contains(&G::PinchEnd(true)) && !one_finger(&after_cancel));
        if !first {
            ck.t(&format!("{name}: no one-finger gesture at all"), !one_finger(&v));
        }
    }
    // Two-finger tap (both land in one report, lift together or one by one).
    let v = frames_to_gestures(&[(1000, vec![(150, 160), (250, 160)]), (1016, vec![(150, 160), (250, 161)]), (1032, vec![(150, 160), (250, 161)]), (1048, vec![])]);
    ck.t("two-finger tap: nothing", v.is_empty());
    let v = frames_to_gestures(&[(1000, vec![(150, 160), (250, 160)]), (1016, vec![(150, 160), (250, 161)]), (1032, vec![(250, 161)]), (1048, vec![(250, 161)]), (1064, vec![])]);
    ck.t("two-finger tap, lifted one by one: nothing", v.is_empty());
    // A new one-finger touch well after a pinch is a normal touch.
    let mut seq = vec![];
    let mut t = 1000;
    for i in 0..=10u16 {
        seq.push((t, vec![(150 - 5 * i, 160), (250 + 5 * i, 162)]));
        t += 16;
    }
    seq.push((t, vec![]));
    t += 500;
    for i in 0..=5u16 {
        seq.push((t, vec![(200, 100 + 6 * i)]));
        t += 16;
    }
    seq.push((t, vec![]));
    let v = frames_to_gestures(&seq);
    ck.t("pinch, then a new touch 500 ms later: Down, moves, Up", v.iter().filter(|g| matches!(g, G::Down(_))).count() == 1 && v.iter().any(|g| matches!(g, G::Move(..))) && matches!(v.last(), Some(G::Up(_))));
}

fn pinch_tests(ck: &mut Ck) {
    // Horizontal spread 100 -> 200 px, slot order swapping every frame.
    let f: Vec<_> = (0..=10u16).map(|i| ((150 - 5 * i, 160), (250 + 5 * i, 162))).collect();
    let v = touch2(&f, true);
    let last = v.iter().rev().find_map(|g| if let G::Pinch(ax, s0, s) = g { Some((*ax, *s0, *s)) } else { None });
    ck.t("pinch: horizontal, span 100 -> 200 (swapped slots)", last == Some((Axis::H, 100, 200)));
    ck.t("pinch: ends with PinchEnd(true)", v.last() == Some(&G::PinchEnd(true)));
    ck.t("pinch: no power-off swipe", !v.iter().any(|g| matches!(g, G::MultiSwipeDown(_))));
    // Vertical squeeze 160 -> 80.
    let f: Vec<_> = (0..=8u16).map(|i| ((200, 80 + 5 * i), (202, 240 - 5 * i))).collect();
    let v = touch2(&f, false);
    let last = v.iter().rev().find_map(|g| if let G::Pinch(ax, s0, s) = g { Some((*ax, *s0, *s)) } else { None });
    ck.t("pinch: vertical, span 160 -> 80", last == Some((Axis::V, 160, 80)));
    // Two fingers moving down together (power-off swipe): no pinch.
    let f: Vec<_> = (0..=10u16).map(|i| ((150, 60 + 10 * i), (260, 62 + 10 * i))).collect();
    let v = touch2(&f, true);
    ck.t("2-finger swipe down: no pinch", !v.iter().any(|g| matches!(g, G::Pinch(..) | G::PinchEnd(_))));
    ck.t("2-finger swipe down: MultiSwipeDown(2)", v.last() == Some(&G::MultiSwipeDown(2)));
}

/// Pre-flash review 7dc32ac B1: a pinch whose end was lost (MF / AUTO /
/// overlay mid-pinch) must not carry its index into the other axis. The
/// review's sequence: CH1 at 10 V/div (range 9), vertical pinch, (MF,
/// release, MF), horizontal pinch closing to ~0.68 -> the index must be a
/// valid timebase, never a range >= 10; and every pinch result stays in its
/// own table.
fn pinch_state_tests(ck: &mut Ck) {
    use scope_ui::{pinch_step, PinchState};
    let (tb, range, tb_min, tb_max) = (13usize, 9usize, 3usize, 24usize);
    let v = pinch_step(None, false, 160, 300, tb, range, tb_min, tb_max);
    ck.t("pinch V from range 9: vertical, range index", !v.horizontal && v.i <= 9);
    // The stale V state is fed to a new horizontal pinch (its end was lost).
    let h = pinch_step(Some(v), true, 200, 136, tb, range, tb_min, tb_max);
    ck.t("stale V + H pinch to 0.68: horizontal, from the timebase", h.horizontal && h.i0 == tb && (tb_min..=tb_max).contains(&h.i));
    // Exhaustive: any stored state x any ratio stays inside its table.
    let mut ok = true;
    for horizontal in [false, true] {
        for prev in [None, Some(PinchState { horizontal: true, i0: 29, i: 29 }), Some(PinchState { horizontal: false, i0: 9, i: 0 })] {
            for s in [1u16, 10, 50, 99, 136, 200, 400, 999] {
                for (t, r) in [(3usize, 0usize), (29, 9), (13, 6)] {
                    let n = pinch_step(prev, horizontal, 200, s, t, r, tb_min, 29);
                    ok &= n.horizontal == horizontal && if horizontal { (tb_min..=29).contains(&n.i) } else { n.i <= scope_ui::RANGE_MAX };
                }
            }
        }
    }
    ck.t("pinch_step: every result inside its own table", ok);
}

fn raw_tests(ck: &mut Ck) {
    // Raw-move capture: every moved report is a Move from the previous point,
    // from the first pixel (no slop), and the release is Up.
    let mut tr = Tracker::new();
    let mut v = vec![];
    let pts = [(100u16, 100u16), (101, 100), (101, 100), (103, 98), (120, 98)];
    for (i, &(x, y)) in pts.iter().enumerate() {
        let mut e = Event { points: 1, p: Point { x, y }, ..Event::default() };
        e.pts[0] = e.p;
        if let Some(g) = tr.event(&e, 1000 + 10 * i as u64) {
            if matches!(g, G::Down(_)) {
                gesture::raw_next(true);
                tr.capture();
            }
            v.push(g);
        }
    }
    if let Some(g) = tr.event(&Event::default(), 1100) {
        v.push(g);
    }
    let p = |x, y| Point { x, y };
    ck.t(
        "raw moves: Down, Move x3 (none for a still report), Up",
        v == vec![G::Down(p(100, 100)), G::Move(p(100, 100), p(101, 100)), G::Move(p(101, 100), p(103, 98)), G::Move(p(103, 98), p(120, 98)), G::Up(p(120, 98))],
    );
    // A plain capture after a raw one has no moves.
    let v = touch(&line((100, 100), (100, 160), 6), true, 0);
    ck.t("plain capture: Drag, no Move", v.iter().any(|g| matches!(g, G::Drag(..))) && !v.iter().any(|g| matches!(g, G::Move(..))));
}

fn gesture_tests(ck: &mut Ck) {
    pinch_tests(ck);
    pinch_lift_tests(ck);
    pinch_state_tests(ck);
    raw_tests(ck);
    let tap = |v: &[G]| v.iter().any(|g| matches!(g, G::Tap(_)));
    ck.t("still tap", tap(&touch(&[(100, 100), (100, 101), (101, 100)], false, 0)));
    ck.t("tap point = down point", touch(&[(100, 100), (110, 104)], false, 0).last() == Some(&G::Tap(Point { x: 100, y: 100 })));
    ck.t("roll 25 px: still a tap (uncaptured)", tap(&touch(&line((100, 100), (100, 125), 5), false, 0)));
    ck.t("moved 45 px: no tap", !tap(&touch(&line((100, 100), (100, 145), 5), false, 0)));
    ck.t("swipe 80 px left", touch(&line((300, 100), (220, 104), 8), false, 0).last() == Some(&G::SwipeLeft));
    ck.t("captured: Up, no tap", { let v = touch(&[(100, 100), (101, 100)], true, 0); !tap(&v) && matches!(v.last(), Some(G::Up(_))) });
    let v = touch(&[(100, 100), (100, 100)], false, 800);
    ck.t("held 800 ms: LongPress", v.iter().any(|g| matches!(g, G::LongPress(_))));
    // TAP_AFTER_LONG on: an uncaptured long press still taps on release.
    ck.t("held 800 ms: tap after LongPress (uncaptured)", tap(&v));
    let v = touch(&[(100, 100), (100, 100)], true, 800);
    ck.t("held 800 ms captured: no tap", !tap(&v));
    ck.t("held 600 ms: tap", tap(&touch(&[(100, 100), (100, 100)], false, 600)));
    let v = touch(&[(100, 100), (100, 100)], false, 0);
    ck.t("one Down, one Tap per touch", v.iter().filter(|g| matches!(g, G::Down(_))).count() == 1 && v.iter().filter(|g| matches!(g, G::Tap(_))).count() == 1);
}

// ------------------------------------------------------------------ 3. log

fn log_tests(ck: &mut Ck) {
    touch::log_clear();
    let _ = touch(&[(100, 100), (100, 101)], false, 0);
    touch::log(b'F', 5, b'e', 1, 0x06, Point { x: 7, y: 8 });
    let mut b = vec![0u8; 6144];
    let n = touch::log_text(&mut b, 1234);
    let s = std::str::from_utf8(&b[..n]).unwrap();
    if ck.verbose {
        print!("{s}");
    }
    ck.t("log: header", s.starts_with("TOUCH,1234,3,3\n"));
    ck.t("log: down / tap lines", s.contains(",G,D,0,100,100\n") && s.contains(",G,T,0,100,100\n"));
    ck.t("log: frame line", s.contains("5,F,e,1,7,8,06\n"));
    for i in 0..1000u32 {
        touch::log(b'E', i as u64, 1, 0, 0, Point { x: 479, y: 319 });
    }
    let n = touch::log_text(&mut b, 4_000_000_000);
    ck.t("log: full ring fits 6 KB", n < 6144 && std::str::from_utf8(&b[..n]).unwrap().lines().count() == touch::LOG_N + 1);
    let n = touch::log_text(&mut b[..100], 0);
    ck.t("log: truncated at a line end", b[n - 1] == b'\n' && n <= 100);
    touch::log_clear();
}

// ------------------------------------------------------- 4. hit vs. drawn

#[derive(Clone, Copy, Debug)]
struct Bx {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

/// Ink boxes of the shapes (sf::rbox) on the emulator screen.
fn shapes() -> Vec<Bx> {
    let e = emu();
    let mut m: std::collections::BTreeMap<u32, Bx> = Default::default();
    for y in 0..H {
        for x in 0..W {
            let o = e.owner[y * W + x];
            if o == 0 || e.kinds[(o >> 8) as usize - 1] != probe::SHAPE {
                continue;
            }
            let b = m.entry(o >> 8).or_insert(Bx { x0: x as i32, y0: y as i32, x1: x as i32, y1: y as i32 });
            b.x0 = b.x0.min(x as i32);
            b.y0 = b.y0.min(y as i32);
            b.x1 = b.x1.max(x as i32);
            b.y1 = b.y1.max(y as i32);
        }
    }
    m.into_values().collect()
}

/// Check the page now on the emulator against its hit function. `only`:
/// shapes to consider (e.g. one screen region); `want`: targets that must
/// be drawn.
fn page(ck: &mut Ck, name: &str, hit: &dyn Fn(u16, u16) -> Option<u32>, only: &dyn Fn(&Bx) -> bool, want: &[u32]) {
    let sh: Vec<Bx> = shapes().into_iter().filter(|b| only(b)).collect();
    let mut seen: Vec<(u32, Bx)> = vec![];
    let mut report = String::new();
    for b in &sh {
        let (cx, cy) = ((b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2);
        let Some(id) = hit(cx as u16, cy as u16) else { continue };
        // Nested shapes (a box inside a button) share the button's target.
        if seen.iter().any(|(i, o)| *i == id && o.x0 <= b.x0 && o.y0 <= b.y0 && o.x1 >= b.x1 && o.y1 >= b.y1) {
            continue;
        }
        let mut bad = 0;
        let mut first = None;
        for y in b.y0..=b.y1 {
            for x in b.x0..=b.x1 {
                let h = hit(x as u16, y as u16);
                if h != Some(id) {
                    bad += 1;
                    first.get_or_insert((x, y, h));
                }
            }
        }
        ck.t(&format!("{name}: target {id} hit on all of its box {b:?} ({bad} px off, first {first:?})"), bad == 0);
        let dup = seen.iter().find(|(i, _)| *i == id).map(|x| x.1);
        ck.t(&format!("{name}: target {id} drawn once ({b:?} and {dup:?})"), dup.is_none());
        // Reach of the hit area past the drawn box (left, top, right, bottom).
        let (mut hx0, mut hy0, mut hx1, mut hy1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                if hit(x as u16, y as u16) == Some(id) {
                    hx0 = hx0.min(x);
                    hy0 = hy0.min(y);
                    hx1 = hx1.max(x);
                    hy1 = hy1.max(y);
                }
            }
        }
        report += &format!("    {id:>4} drawn {:>3},{:>3}..{:>3},{:>3}  hit reach L{:+} T{:+} R{:+} B{:+}\n", b.x0, b.y0, b.x1, b.y1, b.x0 - hx0, b.y0 - hy0, hx1 - b.x1, hy1 - b.y1);
        seen.push((id, *b));
    }
    for w in want {
        ck.t(&format!("{name}: target {w} is drawn"), seen.iter().any(|(i, _)| i == w));
    }
    // Dead pixels (a tap there does nothing): `gap` = within 6 px of two
    // buttons (between neighbours), `rim` = within 3 px of one button's
    // outside edge (a near miss).
    let (mut gap, mut rim) = (0, 0);
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if hit(x as u16, y as u16).is_some() {
                continue;
            }
            let near = |d: i32| seen.iter().filter(|(_, b)| x >= b.x0 - d && x <= b.x1 + d && y >= b.y0 - d && y <= b.y1 + d).count();
            if near(6) >= 2 {
                gap += 1;
            } else if near(3) >= 1 {
                rim += 1;
            }
        }
    }
    println!("  {name}: {} buttons; dead px: {gap} between buttons, {rim} on the 3 px rim", seen.len());
    if ck.verbose {
        print!("{report}");
    }
}

fn all(_: &Bx) -> bool {
    true
}

fn hit_tests(ck: &mut Ck) {
    host::map_flash();
    let o = |v: Option<usize>| v.map(|i| i as u32);
    // Main menu.
    host::reset(0);
    ui::main_menu("DMM + Scope");
    page(ck, "menu", &|x, y| o(ui::main_menu_hit(x, y)), &all, &[0, 1, 2, 3, 4, 5]);
    // Settings 1/2 and 2/2, Hold & Limits (list pages).
    let vals = ["10 min", "1 hour", "100%", "100%", "OFF", "DMM + Scope", "Auto", "Hold 300 ms", "", "", ""];
    for p in 0..2u8 {
        host::reset(0);
        ui::settings(p, &vals);
        let n = if p == 0 { ui::SETTINGS.len() } else { ui::SETTINGS2.len() };
        let want: Vec<u32> = (0..n as u32).collect();
        page(ck, &format!("settings {}", p + 1), &|_, y| o(ui::settings_hit(p, y)), &|b| b.y0 >= 40, &want);
    }
    host::reset(0);
    ui::list_page("Hold & Limits", &ui::LIMIT_ROWS, &["ON"; 8]);
    page(ck, "limits", &|_, y| o(ui::list_hit(y, ui::LIMIT_ROWS.len())), &|b| b.y0 >= 40, &(0..8).collect::<Vec<u32>>());
    // Range pages and the temperature page.
    for b in [dmm::Button::Volt, dmm::Button::Amp, dmm::Button::Ohm, dmm::Button::Cap] {
        host::reset(0);
        let n = ui::range_items(b).len();
        ui::range_page(b, Some(0), 0, None);
        let hit = move |x, y| {
            ui::range_hit(b, x, y).map(|r| match r {
                Ok(i) => i as u32,
                Err(k) => 100 + k as u32,
            })
        };
        let mut want: Vec<u32> = (0..n as u32).collect();
        want.extend([102, 104]);
        if matches!(b, dmm::Button::Volt | dmm::Button::Amp) {
            want.extend([100, 101, 103]);
        }
        page(ck, &format!("range {n} items"), &hit, &|b| b.y0 >= 40, &want);
    }
    host::reset(0);
    ui::temp_page(0, 0.0, "info");
    page(ck, "temperature", &|x, y| ui::temp_hit(x, y).map(|v| v as u32), &|b| b.y0 >= 40, &[0, 254, 255]);
    // Keypads.
    for kind in [ui::KP_FREQ, ui::KP_NUM, ui::KP_SIGNED] {
        host::reset(0);
        ui::keypad(kind, "Vpp", "1.5", "V");
        page(ck, &format!("keypad {kind}"), &|x, y| ui::keypad_hit(kind, x, y).map(|c| c as u32), &|b| b.x0 >= 224, &[b'0' as u32, b'9' as u32, b'=' as u32, b'<' as u32]);
    }
    // DMM page: function row (ui::button_hit) and the app.rs View::Dmm tap
    // order for the rest (copied from App::tap: REL box, MIN/MAX column,
    // history row, rate chip, chart axis column, function row).
    host::reset(0);
    let mut dv = ui::DmmView::new();
    dv.draw_static(dmm::Button::Volt, 1, false);
    ui::history(&[(1.0, 3, "V"); 5], 5, 4);
    let side = ui::Side { max: Some(1.0), avg: Some(1.0), min: Some(1.0), rel: Some(1.0), secs: Some(5), rec: false };
    let pts: Vec<f32> = (0..321).map(|i| i as f32).collect();
    ui::chart(&mut ui::ChartCache::new(), &pts, 3, 1, &side, "V", ui::RateChip { rate: 1, period_ms: 100, applies: true });
    let dmm_hit = |x: u16, y: u16| -> Option<u32> {
        if x >= 402 && (205..243).contains(&y) {
            Some(100) // REL
        } else if x >= 402 && (175..279).contains(&y) {
            Some(101) // MIN/MAX column
        } else if (153..172).contains(&y) {
            Some(102) // history row (one target for all five boxes)
        } else if ui::rate_chip_hit(x, y) {
            Some(103)
        } else if x < ui::CHART_X && (176..280).contains(&y) {
            Some(104) // chart rate
        } else {
            o(ui::button_hit(6, x, y))
        }
    };
    page(ck, "dmm function row", &dmm_hit, &|b| b.y0 >= ui::BTN_Y as i32 - 4, &[0, 1, 2, 3, 4, 5]);
    page(ck, "dmm REL + rate chip", &dmm_hit, &|b| b.y0 >= 175 && b.y1 < 279, &[100, 103]);
    // Combined view: the DMM function band over the replica page.
    host::reset(0);
    let mut cm = scope_ui::Model::DEFAULT;
    cm.combined = true;
    scope_ui::invalidate();
    scope_ui::update(&cm);
    ui::func_band(dmm::Button::Volt, 1, false);
    page(ck, "function band", &|x, y| o(ui::func_band_hit(x, y)), &|b| b.y0 >= ui::FUNC_Y as i32, &[0, 1, 2, 3, 4, 5]);
    // Stock-replica scope page: right panel (4 px vertical slop), MF popup
    // tabs, CH1 popup.
    host::reset(0);
    scope_ui::invalidate();
    scope_ui::update(&scope_ui::Model::DEFAULT);
    page(ck, "replica panel", &|x, y| scope_ui::panel_hit(x, y).map(|k| k as u32), &|b| b.x0 >= 414, &[0, 1, 2, 3, 4, 5, 6]);
    let pv = scope_ui::PopVals { meas: [0, 10, 9, 11], meas_n: 4, roll: false, persist: 0, bright: 100, grid: 50, rising: true, mode: 0, ac: false, probe: 3, colour: 2, math_colour: 1, pressed_cell: None };
    for tab in 0..4u8 {
        host::reset(0);
        scope_ui::draw_mf(tab, &pv);
        let hit = move |x: u16, y: u16| match scope_ui::mf_hit(tab, &pv, x as i16, y as i16) {
            scope_ui::MfHit::Tab(t) => Some(1000 + t as u32),
            scope_ui::MfHit::Cell(i) => Some(i as u32),
            scope_ui::MfHit::SelCell(i) => Some(100 + i as u32),
            scope_ui::MfHit::Clear => Some(200),
            scope_ui::MfHit::Default => Some(201),
            scope_ui::MfHit::Roll(on) => Some(300 + on as u32),
            scope_ui::MfHit::Persist(i) => Some(310 + i as u32),
            scope_ui::MfHit::Bright => Some(320),
            scope_ui::MfHit::Grid => Some(321),
            scope_ui::MfHit::Rising(r) => Some(330 + r as u32),
            scope_ui::MfHit::Mode(m) => Some(340 + m as u32),
            scope_ui::MfHit::PriScr => Some(350),
            scope_ui::MfHit::SaveCsv => Some(351),
            _ => None,
        };
        page(ck, &format!("replica MF tab {tab}"), &hit, &|b| b.y0 >= 21, &[]);
    }
    for tab in 0..3u8 {
        host::reset(0);
        scope_ui::draw_ch1(tab, &pv);
        let hit = move |x: u16, y: u16| match scope_ui::side_hit(false, tab, &pv, x as i16, y as i16) {
            scope_ui::SideHit::Tab(t) => Some(1000 + t as u32),
            scope_ui::SideHit::Coupling(c) => Some(c as u32),
            _ => None,
        };
        page(ck, &format!("replica CH1 tab {tab}"), &hit, &|b| b.x0 >= 215 && b.y0 >= 50, &[1000, 1001, 1002]);
    }
    // Generator page: value boxes (ui::gen_box_hit) + the wave box (app.rs
    // View::Gen: x < 243, y 40..82).
    host::reset(0);
    let g = gen::Gen::new();
    ui::gen_static(&g);
    let gen_hit = |x: u16, y: u16| if x < 243 && (40..82).contains(&y) { Some(100) } else { o(ui::gen_box_hit(x, y)) };
    page(ck, "generator", &gen_hit, &|b| b.y0 >= 36, &[0, 1, 2, 3, 100]);
}

fn main() {
    let verbose = std::env::args().any(|a| a == "-v");
    let mut ck = Ck { cases: 0, fails: 0, verbose };
    debounce_tests(&mut ck);
    gesture_tests(&mut ck);
    log_tests(&mut ck);
    hit_tests(&mut ck);
    println!("cases {}, failures {}", ck.cases, ck.fails);
    std::process::exit(if ck.fails == 0 { 0 } else { 1 });
}
