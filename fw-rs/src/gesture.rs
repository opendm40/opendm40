//! Touch gestures from the point track: tap, long press, swipes and drag, plus
//! the finger-count-aware global gestures (docs/scope-ui-plan.md §2.6):
//!
//! * One finger: `Down` at touch-down, `Drag` once it moved more than SLOP,
//!   `LongPress` held still, and on release `Tap` (did not move) or a swipe.
//!   A swipe needs >= SWIPE_MIN px within SWIPE_MAX_MS and a dominant axis
//!   (the travel along it more than twice the travel across). A touch the app
//!   captured (`capture()`, called when it took the `Down`/`Drag`, e.g. a drag
//!   that starts in the scope graph) is never a swipe or a tap: dragging a
//!   cursor or the trigger quickly no longer switches the view on release
//!   (docs/ui-review.md §2.3, the old classifier fired on any >= 60 px release).
//! * Two or more fingers: never a one-finger gesture. When a second finger
//!   lands during a one-finger touch the tracker emits `Cancel` once (the app
//!   undoes whatever the first finger's Down/Drag changed) and nothing else
//!   for the rest of that touch, until the release classifies it by the most
//!   fingers seen: the centroid of the points must move down (dy) at least
//!   MULTI_MIN[n] px, more than sideways, between the frame where n fingers
//!   were first seen and the last frame that still had n fingers (fingers
//!   lift one by one, and the slot order can change as they do). 2 fingers
//!   >= 60 px = `MultiSwipeDown(2)` (power-off confirm), 3 fingers >= 30 px
//!   (the stock threshold, gui_v2_6.h "Screenshot") = `MultiSwipeDown(3)`
//!   (screenshot). 4-5 points (a palm) are ignored.
//!
//! The controller only reports points; the stock firmware has no one-finger
//! gestures.

use crate::touch::{Event, Point, MAX_POINTS};

/// Minimum travel for a swipe, pixels.
const SWIPE_MIN: i32 = 60;
/// Swipes must finish within this time, ms.
const SWIPE_MAX_MS: u64 = 800;
/// A press without movement longer than this is a long press, ms.
const LONG_MS: u64 = 700;
/// Movement tolerated for a tap / long press, pixels.
const SLOP: i32 = 15;
/// Minimum downward centroid travel of an n-finger swipe, pixels (index = n;
/// 0 = not a gesture).
const MULTI_MIN: [i32; MAX_POINTS + 1] = [0, 0, 60, 30, 0, 0];
/// An n-finger swipe must be done within this time of the n-th finger landing.
const MULTI_MAX_MS: u64 = 2000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// Finger down (for immediate button feedback).
    Down(Point),
    Tap(Point),
    LongPress(Point),
    /// Finger moved while down (start, current).
    Drag(Point, Point),
    SwipeLeft,
    SwipeRight,
    SwipeUp,
    SwipeDown,
    /// A second finger landed during a one-finger touch: undo what its
    /// Down / Drag did and drop anything pending for it.
    Cancel,
    /// n >= 2 fingers swiped down (n = the most fingers seen in the touch).
    MultiSwipeDown(u8),
    /// Side key went down / up: code 0..3.
    KeyDown(u8),
    KeyUp(u8),
}

#[derive(Clone, Copy)]
struct Multi {
    /// Most fingers seen in this touch.
    n: u8,
    /// Centroid and time when `n` fingers were first seen.
    c0: (i32, i32),
    t0: u64,
    /// Centroid and time of the last frame with `n` fingers.
    c1: (i32, i32),
    t1: u64,
}

pub struct Tracker {
    /// A touch is in progress (any number of fingers).
    active: bool,
    /// One-finger touch: start point and time.
    down: Option<(Point, u64)>,
    last: Point,
    moved: bool,
    long_sent: bool,
    /// The app owns this touch (no swipe / tap on release).
    captured: bool,
    /// Set once the touch had two or more fingers.
    multi: Option<Multi>,
    keys: u8,
}

impl Tracker {
    pub const fn new() -> Self {
        Tracker {
            active: false,
            down: None,
            last: Point { x: 0, y: 0 },
            moved: false,
            long_sent: false,
            captured: false,
            multi: None,
            keys: 0,
        }
    }

    /// The app took the current touch (its `Down` or `Drag`): on release it
    /// is not classified as a swipe or a tap. No effect without a touch.
    pub fn capture(&mut self) {
        if self.active {
            self.captured = true;
        }
    }

    /// Feed a touch state change. At most one gesture per call (keys first).
    pub fn event(&mut self, ev: &Event, now: u64) -> Option<Gesture> {
        if ev.keys != self.keys {
            let changed = ev.keys ^ self.keys;
            let code = changed.trailing_zeros() as u8;
            let down = ev.keys & changed != 0;
            self.keys = ev.keys;
            return Some(if down { Gesture::KeyDown(code) } else { Gesture::KeyUp(code) });
        }
        let n = (ev.points as usize).min(MAX_POINTS) as u8;
        if n == 0 {
            if !self.active {
                return None;
            }
            self.active = false;
            let single = self.down.take();
            if let Some(m) = self.multi.take() {
                return multi_gesture(&m);
            }
            let (p0, t0) = single?;
            return self.release(p0, t0, now);
        }
        if n >= 2 {
            let c = centroid(&ev.pts[..n as usize]);
            let had_single = self.active && self.down.is_some();
            match self.multi.as_mut() {
                // More fingers than before: the gesture (re)starts here.
                Some(m) if n <= m.n => {
                    if n == m.n {
                        m.c1 = c;
                        m.t1 = now;
                    } // else a finger lifted: keep the last full-count frame
                }
                _ => self.multi = Some(Multi { n, c0: c, t0: now, c1: c, t1: now }),
            }
            self.active = true;
            self.down = None;
            return if had_single { Some(Gesture::Cancel) } else { None };
        }
        // One finger.
        if self.multi.is_some() {
            return None; // fingers lifting after a multi-finger touch
        }
        let p = ev.p;
        match self.down {
            None => {
                self.active = true;
                self.down = Some((p, now));
                self.last = p;
                self.moved = false;
                self.long_sent = false;
                self.captured = false;
                Some(Gesture::Down(p))
            }
            Some((p0, _)) => {
                self.last = p;
                if dist(p0, p) > SLOP {
                    self.moved = true;
                }
                if self.moved { Some(Gesture::Drag(p0, p)) } else { None }
            }
        }
    }

    /// One-finger release: swipe (not captured, dominant axis), tap or nothing.
    fn release(&mut self, p0: Point, t0: u64, now: u64) -> Option<Gesture> {
        let p1 = self.last;
        let dx = p1.x as i32 - p0.x as i32;
        let dy = p1.y as i32 - p0.y as i32;
        let dt = now.saturating_sub(t0);
        if self.long_sent || self.captured {
            None
        } else if dt <= SWIPE_MAX_MS && (dx.abs() >= SWIPE_MIN || dy.abs() >= SWIPE_MIN) {
            if dx.abs() > 2 * dy.abs() {
                Some(if dx > 0 { Gesture::SwipeRight } else { Gesture::SwipeLeft })
            } else if dy.abs() > 2 * dx.abs() {
                Some(if dy > 0 { Gesture::SwipeDown } else { Gesture::SwipeUp })
            } else {
                None // diagonal: no swipe
            }
        } else if !self.moved {
            Some(Gesture::Tap(p0))
        } else {
            None
        }
    }

    /// Call periodically: reports a long press while the finger is still down.
    pub fn tick(&mut self, now: u64) -> Option<Gesture> {
        let (p0, t0) = self.down?;
        if !self.moved && !self.long_sent && now.saturating_sub(t0) >= LONG_MS {
            self.long_sent = true;
            return Some(Gesture::LongPress(p0));
        }
        None
    }

    /// Test hook (DBG:SWIPE D,n): run a synthetic `fingers`-finger downward
    /// swipe (120 px in 200 ms, fingers 60 px apart) through the tracker and
    /// return what the release yields (`MultiSwipeDown(n)` for 2 or 3 fingers,
    /// `SwipeDown` for one). Anything in progress is dropped first; the
    /// intermediate Down / Drag / Cancel are not reported.
    pub fn synth_swipe_down(&mut self, fingers: u8, now: u64) -> Option<Gesture> {
        let keys = self.keys;
        *self = Tracker::new();
        self.keys = keys;
        let n = fingers.clamp(1, MAX_POINTS as u8);
        let mut ev = Event { keys, ..Event::default() };
        for step in 0..=8u16 {
            ev.points = n;
            for i in 0..n as usize {
                ev.pts[i] = Point { x: 120 + 60 * i as u16, y: 100 + 15 * step };
            }
            ev.p = ev.pts[0];
            self.event(&ev, now + step as u64 * 25);
        }
        self.event(&Event { keys, ..Event::default() }, now + 225)
    }
}

/// Release of a touch that had n >= 2 fingers.
fn multi_gesture(m: &Multi) -> Option<Gesture> {
    let min = MULTI_MIN[m.n as usize];
    let dx = m.c1.0 - m.c0.0;
    let dy = m.c1.1 - m.c0.1;
    if min > 0 && dy >= min && dy > dx.abs() && m.t1.saturating_sub(m.t0) <= MULTI_MAX_MS {
        Some(Gesture::MultiSwipeDown(m.n))
    } else {
        None
    }
}

fn centroid(pts: &[Point]) -> (i32, i32) {
    let n = pts.len().max(1) as i32;
    let (sx, sy) = pts.iter().fold((0i32, 0i32), |(sx, sy), p| (sx + p.x as i32, sy + p.y as i32));
    (sx / n, sy / n)
}

fn dist(a: Point, b: Point) -> i32 {
    (a.x as i32 - b.x as i32).abs().max((a.y as i32 - b.y as i32).abs())
}
